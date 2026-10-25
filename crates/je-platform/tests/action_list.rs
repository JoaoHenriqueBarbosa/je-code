//! Porte de `src/vs/platform/actionWidget/test/browser/actionList.test.ts` (suíte ActionListWidget), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **DOM.** O `ActionListWidget` nativo é o estado da lista. `widget.domNode.querySelector('.monaco-list-row...')` é
//!   `row_state(índice)`, o estado que o `ActionItemRenderer` deixa na linha (`aria-label`, selos, ícone, barra de
//!   ferramentas, `Switch`, `aria-expanded`); `row.ariaLabel`, `badge.ariaHidden`, `style.display` e `previousElementSibling`
//!   são campos dele. O `renderElement` reescreve a linha inteira a cada reuso, então "limpa no reuso" é conferir o
//!   `row_state` depois do `update_items`; a identidade do elemento reaproveitado (`reused: true`) não existe no estado nativo
//!   e sai das asserções.
//! - **Eventos.** `dispatchEvent(new MouseEvent('mouseover'|'mousemove'|'mousedown', ...))` são `pointer_over`,
//!   `pointer_move` (com `movementX` e `movementY` no [`PointerEvent`]) e `pointer_down`; `row.click()` e o toque (`Tap`)
//!   são `click_row`; o clique no chevron é `click_submenu_indicator`. `dispatchKeyDown(alvo, init)` é `dispatch_key_down`
//!   com o [`KeyTarget`] do alvo (`widget.domNode` é `DomNode`, o `.monaco-list` é `List`, o painel é `SubmenuPanel`).
//!   `typeFilter` é `filter_input_input` e os eventos de composição são `filter_input_composition_*`.
//! - **Foco.** `document.activeElement` é o foco que a lista guarda: `is_list_focused`, `is_panel_focused`,
//!   `panel_contains_focus` e `FilterInput::is_focused`.
//! - **Painel.** `.action-list-submenu-panel` é o estado do painel: `panel_displayed` (`style.display !== 'none'`),
//!   `panel_text` (`textContent`), `panel_contains`, `panel_animated` e `submenu_rows`; o conteúdo `HTMLElement` do hover é
//!   um [`HoverElement`] (texto, tamanho e identidade).
//! - **CSS calculado.** `getComputedStyle(...)` de regras do `actionWidget.css` vira função do estado nativo:
//!   `action_item_badge_metrics`, `row_outline_color` (com as cores dos tokens em [`CssColor`]) e
//!   `toggle_info_icon_center_offset`. A cor herdada e as variáveis `--vscode-*` que o teste põe no `domNode` são a tabela
//!   `theme_color` do teste.
//! - **Geometria.** `getBoundingClientRect()` do painel e das linhas são `panel_rect` e `visible_row_rect`; o `zoom` CSS do
//!   popup é `set_zoom_level`, e o retângulo do `domNode` e do popup (que o motor de layout do DOM calcularia) o teste
//!   monta com `ViewElement::set_position` e `set_size`. `window.innerWidth` e `innerHeight` são `set_window_size`.
//! - **Tempo.** `runWithFakedTimers` e `timeout(n)` são `run_with_faked_timers` e `timeout`, o relógio virtual do `runtime`.
//! - **`sinon.spy(row, 'getAttribute')`.** A chamada a `getAttribute('data-index')` é o mapeamento do evento do ponteiro
//!   para a linha pelo ouvinte `initialMouseMove`; o equivalente observável é `is_mapping_pointer_moves`.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::actions::{ActionRef, SubmenuAction, ToActionProps, to_action};
use je_base::async_::timeout;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::key_codes::KeyCode;
use je_base::promise::{DeferredPromise, Promise};
use je_platform::action_widget::action_list::{
    ActionListBadge, ActionListGroup, ActionListItemData, ActionListItemKind, ActionListWidget, CssColor, FilterRequest, HoverContent, HoverElement,
    IActionListDelegate, IActionListItem, IActionListItemHover, IActionListItemInlineToggle, IActionListOptions, IActionListUpdateOptions, KeyTarget,
    PointerEvent, PointerTarget, RowState, action_item_badge_metrics, toggle_info_icon_center_offset,
};
use je_platform::contextview::view_element::{UiKeyEvent, ViewElement};

/// `ITestActionItem`: `{ id, checked? }`.
#[derive(Clone)]
struct Item {
    id: String,
}

impl ActionListItemData for Item {
    fn list_item_id(&self) -> Option<String> {
        Some(self.id.clone())
    }
}

type TestItem = IActionListItem<Item>;

/// `action(id)`.
fn action(id: &str) -> TestItem {
    IActionListItem::action(id, Item { id: id.to_owned() })
}

/// `separator(label)`.
fn separator(label: Option<&str>) -> TestItem {
    IActionListItem { label: label.map(str::to_owned), ..IActionListItem::new(ActionListItemKind::Separator) }
}

fn rc(item: TestItem) -> Rc<TestItem> {
    Rc::new(item)
}

fn rcs(items: Vec<TestItem>) -> Vec<Rc<TestItem>> {
    items.into_iter().map(Rc::new).collect()
}

/// `toAction({ id, label, tooltip?, class?, run })`.
fn to_action_with(id: &str, label: &str, tooltip: Option<&str>, class: Option<&str>, run: Rc<dyn Fn()>) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label: label.to_owned(),
        tooltip: tooltip.map(str::to_owned),
        enabled: None,
        checked: None,
        class: class.map(str::to_owned),
        alt: None,
        run: Rc::new(move |_| {
            run();
            Dynamic::Undefined.into()
        }),
    })
}

fn noop() -> Rc<dyn Fn()> {
    Rc::new(|| {})
}

type FilterFn = Rc<dyn Fn(&str, &FilterRequest) -> Promise<Vec<Rc<TestItem>>>>;

/// O delegate do `createActionListWidget`: `onHide`, `onSelect` e o `onFilter` opcional.
struct TestDelegate {
    on_hide: Rc<dyn Fn()>,
    on_select: Rc<dyn Fn(&Item)>,
    on_filter: Option<FilterFn>,
}

impl IActionListDelegate<Item> for TestDelegate {
    fn on_hide(&self, _did_cancel: Option<bool>) {
        (self.on_hide)();
    }

    fn on_select(&self, action: &Item, _preview: bool) {
        (self.on_select)(action);
    }

    fn handles_filter(&self) -> bool {
        self.on_filter.is_some()
    }

    fn on_filter(&self, filter: &str, request: &FilterRequest) -> Promise<Vec<Rc<TestItem>>> {
        match &self.on_filter {
            Some(on_filter) => on_filter(filter, request),
            None => Promise::resolved(Vec::new()),
        }
    }
}

/// As opções de `createActionListWidget(disposables, options)`.
struct Config {
    items: Vec<TestItem>,
    on_filter: Option<FilterFn>,
    on_hide: Rc<dyn Fn()>,
    on_select: Rc<dyn Fn(&Item)>,
    list_options: IActionListOptions,
}

/// Os `listOptions` de um caso (`{ showFilter: true, ...listOptions }`: o `show_filter` de quem chama vence).
fn config(items: Vec<TestItem>) -> Config {
    Config {
        items,
        on_filter: None,
        on_hide: noop(),
        on_select: Rc::new(|_| {}),
        list_options: IActionListOptions { show_filter: true, ..IActionListOptions::default() },
    }
}

/// `listOptions: { showFilter: false }`.
fn no_filter() -> IActionListOptions {
    IActionListOptions { show_filter: false, ..IActionListOptions::default() }
}

/// O widget do caso, anexado a um `body` e descartado no fim (`disposables.add(widget)`).
struct Widget {
    widget: Rc<ActionListWidget<Item>>,
    _body: Rc<ViewElement>,
}

impl std::ops::Deref for Widget {
    type Target = ActionListWidget<Item>;

    fn deref(&self) -> &Self::Target {
        &self.widget
    }
}

impl Drop for Widget {
    fn drop(&mut self) {
        self.widget.dispose();
    }
}

/// `createActionListWidget(disposables, options)`.
fn create_action_list_widget(_guard: &LeakGuard, config: Config) -> Widget {
    let delegate = Rc::new(TestDelegate { on_hide: config.on_hide, on_select: config.on_select, on_filter: config.on_filter });
    let items = rcs(if config.items.is_empty() { vec![action("initial")] } else { config.items });
    let widget = ActionListWidget::new(false, &items, delegate, Some(Rc::new(config.list_options)));
    let body = ViewElement::new();
    widget.dom_node().set_parent(Some(body.clone()));
    widget.layout(200.0, Some(200.0));
    Widget { widget, _body: body }
}

/// `new MouseEvent(type, { movementX, movementY })`.
fn moved(movement_x: f64, movement_y: f64) -> PointerEvent {
    PointerEvent { movement_x, movement_y, ..PointerEvent::default() }
}

fn key_event(key: &str) -> UiKeyEvent {
    UiKeyEvent::new(key, KeyCode::Unknown)
}

/// `getVisibleRowText(widget)`: o `textContent` de cada linha visível, sem as vazias.
fn visible_row_text(widget: &ActionListWidget<Item>) -> Vec<String> {
    (0..widget.visible_labels().len())
        .filter_map(|index| widget.row_state(index))
        .map(|row| {
            let mut text = row.title_text.clone();
            text.push_str(&row.badge_text);
            for badge in &row.additional_badges {
                text.push_str(&badge.text);
            }
            for button in &row.toolbar {
                text.push_str(&button.text);
            }
            text
        })
        .filter(|text| !text.is_empty())
        .collect()
}

/// `widget.domNode.querySelector('.monaco-list-row.action')`: a primeira linha de ação.
fn first_action_row(widget: &ActionListWidget<Item>) -> RowState {
    let index = (0..widget.visible_labels().len()).find(|index| widget.row_state(*index).is_some_and(|row| row.classes.iter().any(|class| class == "action"))).unwrap();
    widget.row_state(index).unwrap()
}

fn focused_id(widget: &ActionListWidget<Item>) -> Option<String> {
    widget.get_focused_element().and_then(|element| element.item_id())
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// `selected.push(item.id)`: o `onSelect` que registra os ids.
fn recording(selected: &Rc<RefCell<Vec<String>>>) -> Rc<dyn Fn(&Item)> {
    let selected = selected.clone();
    Rc::new(move |item| selected.borrow_mut().push(item.id.clone()))
}

#[test]
fn keeps_shared_badges_borderless_and_sized_without_workbench_size_tokens() {
    // Os tokens `--vscode-spacing-size60` e companhia em `initial` (sem valor) deixam valer os valores de reserva do CSS, que
    // são os das métricas nativas; `--vscode-contrastBorder` não cria borda no selo.
    let default_style = action_item_badge_metrics(None);
    let customized = action_item_badge_metrics(Some(16.0));

    assert_eq!(
        (
            (default_style.padding_vertical, default_style.padding_horizontal),
            default_style.border_width,
            default_style.border_radius,
            default_style.font_size,
            default_style.height,
            customized.height,
        ),
        ((0.0, 6.0), 0.0, 9999.0, 11.0, 18.0, 16.0),
    );
}

#[test]
fn includes_primary_and_additional_badges_in_accessible_labels_and_clears_them_on_reuse() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![IActionListItem {
                badge: Some("Experimental".to_owned()),
                additional_badges: vec![ActionListBadge {
                    label: "Org default".to_owned(),
                    class_name: Some("organization-default".to_owned()),
                    tooltip: Some("Default for new chats.".to_owned()),
                }],
                detail: Some("Evaluates risk before running tools".to_owned()),
                ..action("Assisted permissions")
            }])
        },
    );
    #[derive(Debug, PartialEq)]
    struct Read {
        label: Option<String>,
        badge: String,
        badge_hidden_attribute: String,
        badge_visible: bool,
        after_title: bool,
        additional_badges: Vec<(String, String)>,
    }
    let read_row = || {
        let row = first_action_row(&widget);
        Read {
            label: row.aria_label.clone(),
            badge: row.badge_text.clone(),
            badge_hidden_attribute: row.badge_aria_hidden.clone(),
            badge_visible: !row.badge_hidden,
            after_title: row.badge_after_title,
            additional_badges: row.additional_badges.iter().map(|badge| (badge.text.clone(), badge.class_name.clone())).collect(),
        }
    };
    let initial = read_row();
    widget.update_items(&[rc(action("Manual permissions"))], None, None);

    assert_eq!(
        (initial, read_row()),
        (
            Read {
                label: Some("Assisted permissions, Experimental, Org default, Evaluates risk before running tools".to_owned()),
                badge: "Experimental".to_owned(),
                badge_hidden_attribute: "true".to_owned(),
                badge_visible: true,
                after_title: true,
                additional_badges: vec![("Org default".to_owned(), "action-item-badge organization-default".to_owned())],
            },
            Read {
                label: Some("Manual permissions".to_owned()),
                badge: String::new(),
                badge_hidden_attribute: "true".to_owned(),
                badge_visible: false,
                after_title: true,
                additional_badges: Vec::new(),
            },
        ),
    );
}

#[test]
fn keeps_leading_badges_in_visual_and_accessible_order_when_rows_resolve_or_are_reused() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![IActionListItem { badge: Some("#123".to_owned()), badge_before_label: true, ..action("Pull Request") }])
        },
    );
    let read = || {
        let row = first_action_row(&widget);
        (row.aria_label.clone(), row.title_and_badge_order.clone())
    };
    let initial = read();
    widget.update_items(&[rc(IActionListItem { badge: Some("#123".to_owned()), badge_before_label: true, ..action("Resolved title") })], None, None);
    let resolved = read();
    widget.update_items(&[rc(IActionListItem { badge: Some("New".to_owned()), ..action("Ordinary action") })], None, None);

    assert_eq!(
        (initial, resolved, read()),
        (
            (Some("#123, Pull Request".to_owned()), strings(&["#123", "Pull Request"])),
            (Some("#123, Resolved title".to_owned()), strings(&["#123", "Resolved title"])),
            (Some("Ordinary action, New".to_owned()), strings(&["Ordinary action", "New"])),
        ),
    );
}

#[test]
fn toolbar_labels_remain_actionable_and_revert_to_icons_when_a_row_is_reused() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let selected = Rc::new(RefCell::new(Vec::new()));
    let configured = Rc::new(Cell::new(0));
    let configure = {
        let configured = configured.clone();
        to_action_with("configure", "Medium", Some("Configure Model, Medium"), Some("codicon-settings-gear"), Rc::new(move || configured.set(configured.get() + 1)))
    };
    let widget = create_action_list_widget(
        &guard,
        Config {
            on_select: recording(&selected),
            list_options: IActionListOptions { show_filter: false, stop_toolbar_pointer_propagation: true, ..IActionListOptions::default() },
            ..config(vec![IActionListItem { toolbar_labels: true, toolbar_actions: Some(vec![configure.clone()]), ..action("Model") }])
        },
    );
    let read = || {
        let button = first_action_row(&widget).toolbar[0].clone();
        (button.text, button.aria_label, button.icon)
    };
    let initial = read();
    let _ = widget.run_item_toolbar_action("Model", "configure");
    // `ActionBar` roda a ação de forma assíncrona: o corpo dela corre nos microtasks.
    je_base::runtime::run_microtasks();
    widget.update_items(&[rc(IActionListItem { toolbar_actions: Some(vec![configure]), ..action("Model") })], None, None);

    assert_eq!(
        (initial, read(), configured.get(), selected.borrow().clone()),
        (
            ("Medium".to_owned(), "Configure Model, Medium".to_owned(), false),
            (String::new(), "Configure Model, Medium".to_owned(), true),
            1,
            Vec::<String>::new(),
        ),
    );
}

#[test]
fn visible_toolbar_labels_are_searchable_without_searching_icon_only_action_labels() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let readout = to_action_with("configure", "Medium \u{b7} 264K", None, None, noop());
    let widget = create_action_list_widget(
        &guard,
        config(vec![
            IActionListItem { toolbar_labels: true, toolbar_actions: Some(vec![readout.clone()]), ..action("Model with readout") },
            IActionListItem { toolbar_actions: Some(vec![readout]), ..action("Model with icon") },
        ]),
    );
    widget.filter_input_input("264k");

    assert_eq!(visible_row_text(&widget), strings(&["Model with readoutMedium \u{b7} 264K"]));
}

#[test]
fn section_badges_are_rendered_and_cleared_on_reuse() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![
                IActionListItem {
                    additional_badges: vec![ActionListBadge {
                        label: "Org default".to_owned(),
                        class_name: None,
                        tooltip: Some("Default for new chats.".to_owned()),
                    }],
                    ..separator(Some("Optimize for"))
                },
                action("Balance"),
            ])
        },
    );
    let read = || {
        (0..widget.visible_labels().len())
            .filter_map(|index| widget.row_state(index))
            .flat_map(|row| row.additional_badges.into_iter().map(|badge| badge.text))
            .collect::<Vec<_>>()
    };
    let initial = read();
    widget.update_items(&rcs(vec![separator(Some("Models")), action("A model")]), None, None);

    assert_eq!((initial, read()), (strings(&["Org default"]), Vec::<String>::new()));
}

/// A cor calculada do ícone: a variável `--vscode-*` que o teste pôs no `domNode`, ou a cor herdada dele.
fn theme_color(inline_color: &str) -> (u8, u8, u8) {
    match inline_color {
        "var(--vscode-editorLightBulb-foreground)" => (255, 204, 0),
        "var(--vscode-problemsWarningIcon-foreground)" => (255, 170, 0),
        "" => (0x12, 0x34, 0x56),
        other => panic!("variável sem valor no teste: {other}"),
    }
}

#[test]
fn recycled_action_icons_do_not_retain_a_previous_fallback_or_theme_color() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(&guard, Config { list_options: no_filter(), ..config(vec![action("fallback")]) });
    let icon_state = |widget: &ActionListWidget<Item>| {
        let row = first_action_row(widget);
        (row.icon.inline_color.clone(), theme_color(&row.icon.inline_color))
    };
    let mut states = vec![icon_state(&widget)];
    let icons = [("shield", None), ("warning", Some("problemsWarningIcon.foreground")), ("shield", None)];
    for (id, color) in icons {
        let group = ActionListGroup { title: String::new(), icon: Some(id.to_owned()), icon_color: color.map(str::to_owned) };
        widget.update_items(&[rc(IActionListItem { group: Some(group), ..action(id) })], None, None);
        states.push(icon_state(&widget));
    }

    assert_eq!(
        states,
        vec![
            ("var(--vscode-editorLightBulb-foreground)".to_owned(), (255, 204, 0)),
            (String::new(), (18, 52, 86)),
            ("var(--vscode-problemsWarningIcon-foreground)".to_owned(), (255, 170, 0)),
            (String::new(), (18, 52, 86)),
        ],
    );
}

#[test]
fn opening_under_a_stationary_pointer_preserves_keyboard_focus_and_selection() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let selected = Rc::new(RefCell::new(Vec::new()));
    let widget = create_action_list_widget(
        &guard,
        Config { on_select: recording(&selected), list_options: no_filter(), ..config(vec![action("first"), action("second"), action("third")]) },
    );
    widget.focus();
    widget.pointer_over(2, &PointerEvent::default());
    widget.pointer_move(2, &PointerEvent::default());
    assert_eq!(focused_id(&widget).as_deref(), Some("first"));

    widget.focus_next();
    widget.pointer_over(2, &PointerEvent::default());
    widget.pointer_move(2, &PointerEvent::default());
    widget.accept_selected(false);
    assert_eq!(selected.borrow().clone(), strings(&["second"]));
}

#[test]
fn pointer_movement_enables_hover_on_the_initial_row_and_subsequent_rows() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(&guard, Config { list_options: no_filter(), ..config(vec![action("first"), action("second"), action("third")]) });
    widget.focus();
    widget.pointer_over(1, &PointerEvent::default());
    widget.pointer_move(1, &moved(1.0, 0.0));
    assert_eq!(focused_id(&widget).as_deref(), Some("second"));

    widget.pointer_over(2, &PointerEvent::default());
    assert_eq!(focused_id(&widget).as_deref(), Some("third"));
}

/// O corpo de `only keyboard navigation draws the selection border (combobox: ${filterAsCombobox})`.
fn only_keyboard_navigation_draws_the_selection_border(filter_as_combobox: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: IActionListOptions {
                show_filter: filter_as_combobox,
                focus_filter_on_open: filter_as_combobox,
                filter_as_combobox,
                ..IActionListOptions::default()
            },
            ..config(vec![action("first"), action("second")])
        },
    );
    // --vscode-menu-selectionBorder e --vscode-list-focusOutline: #0069cc; --vscode-contrastActiveBorder: transparent.
    let selection_border = CssColor::rgb(0, 105, 204);
    let outline = |widget: &ActionListWidget<Item>| widget.row_outline_color(widget.focused_index().unwrap(), selection_border, CssColor::TRANSPARENT);

    widget.focus();
    widget.focus_next();
    let mut states = vec![outline(&widget)];
    widget.pointer_move(0, &moved(1.0, 0.0));
    states.push(outline(&widget));
    widget.focus_next();
    states.push(outline(&widget));
    widget.pointer_move(1, &moved(0.0, 1.0));
    states.push(outline(&widget));
    widget.focus_previous();
    states.push(outline(&widget));
    widget.pointer_down();
    states.push(outline(&widget));
    let shift = UiKeyEvent::new("Shift", KeyCode::Shift);
    widget.dispatch_key_down(&shift, if filter_as_combobox { KeyTarget::FilterInput } else { KeyTarget::List });
    states.push(outline(&widget));
    if filter_as_combobox {
        widget.focus();
        states.push(outline(&widget));
    }

    let blue = CssColor::rgb(0, 105, 204);
    let none = CssColor::TRANSPARENT;
    let mut expected = vec![blue, none, blue, none, blue, none, blue];
    if filter_as_combobox {
        expected.push(none);
    }
    assert_eq!(states, expected);
}

#[test]
fn only_keyboard_navigation_draws_the_selection_border_combobox_false() {
    only_keyboard_navigation_draws_the_selection_border(false);
}

#[test]
fn only_keyboard_navigation_draws_the_selection_border_combobox_true() {
    only_keyboard_navigation_draws_the_selection_border(true);
}

#[test]
fn preserves_high_contrast_outlines_during_pointer_navigation() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(&guard, Config { list_options: no_filter(), ..config(vec![action("first"), action("second")]) });
    // --vscode-menu-selectionBorder e --vscode-contrastActiveBorder: #f38518.
    let orange = CssColor::rgb(243, 133, 24);
    widget.focus();
    widget.pointer_move(1, &moved(1.0, 0.0));

    assert_eq!(widget.row_outline_color(1, orange, orange), CssColor::rgb(243, 133, 24));
}

#[test]
fn the_first_click_selects_its_row_without_prior_pointer_movement() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let selected = Rc::new(RefCell::new(Vec::new()));
    let widget = create_action_list_widget(&guard, Config { on_select: recording(&selected), list_options: no_filter(), ..config(vec![action("first"), action("second")]) });
    widget.focus();
    widget.pointer_over(1, &PointerEvent::default());
    widget.pointer_down();
    widget.click_row(1, PointerTarget::Row);
    assert_eq!(selected.borrow().clone(), strings(&["second"]));
}

/// O corpo de `${activation} on an opted-in submenu row opens its submenu without selecting it` (o toque `Tap` e o clique
/// chegam à lista pelo mesmo `onDidChangeSelection` com o alvo na linha).
fn submenu_row_activation(_activation: &str) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let selected = Rc::new(RefCell::new(Vec::new()));
    let widget = create_action_list_widget(
        &guard,
        Config {
            on_select: recording(&selected),
            list_options: no_filter(),
            ..config(vec![
                IActionListItem {
                    submenu_actions: Some(vec![to_action_with("child", "Child", None, None, noop())]),
                    open_submenu_on_click: true,
                    ..action("submenu-only")
                },
                IActionListItem { submenu_actions: Some(vec![to_action_with("option", "Option", None, None, noop())]), ..action("actionable") },
            ])
        },
    );

    widget.click_row(0, PointerTarget::Row);
    let submenu_only_state = (
        selected.borrow().clone(),
        widget.row_state(0).unwrap().aria_expanded,
        widget.submenu_rows().and_then(|rows| rows.into_iter().next()).flatten(),
    );
    widget.click_row(1, PointerTarget::Row);

    assert_eq!(
        (submenu_only_state, selected.borrow().clone()),
        ((Vec::<String>::new(), Some("true".to_owned()), Some("Child".to_owned())), strings(&["actionable"])),
    );
}

#[test]
fn click_on_an_opted_in_submenu_row_opens_its_submenu_without_selecting_it() {
    submenu_row_activation("click");
}

#[test]
fn tap_on_an_opted_in_submenu_row_opens_its_submenu_without_selecting_it() {
    submenu_row_activation("tap");
}

/// O corpo de `${activation} opens opted-in live details and preserves them through refresh`.
fn live_details(activation: &str) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let selected = Rc::new(RefCell::new(Vec::new()));
    let content = HoverElement::new("Running, Attached, 1s");
    let item = {
        let content = content.clone();
        move || {
            rc(IActionListItem {
                hover: Some(IActionListItemHover { content: Some(HoverContent::Element(content.clone())), expandable: true, ..IActionListItemHover::default() }),
                open_submenu_on_click: true,
                ..action("shell")
            })
        }
    };
    let widget = create_action_list_widget(&guard, Config { on_select: recording(&selected), list_options: no_filter(), ..config(vec![(*item()).clone()]) });
    widget.focus();
    if activation == "keyboard" {
        widget.accept_selected(false);
    } else {
        widget.click_row(0, PointerTarget::Row);
    }
    let opened = widget.panel_displayed() && widget.panel_contains(&content);
    let keyboard_focused = activation != "keyboard" || widget.is_panel_focused();
    content.set_text("Running, Attached, 2s");
    let preserve_hover = Some(IActionListUpdateOptions { preserve_hover: true, ..IActionListUpdateOptions::default() });
    widget.update_items(&[item()], None, preserve_hover);
    let retained = widget.panel_displayed() && widget.panel_contains(&content);
    let elapsed = widget.panel_text().contains("Running, Attached, 2s");
    widget.update_items(&[], None, preserve_hover);

    assert_eq!(
        (selected.borrow().clone(), opened, keyboard_focused, retained, elapsed, !widget.panel_displayed()),
        (Vec::<String>::new(), true, true, true, true, true),
    );
}

#[test]
fn click_opens_opted_in_live_details_and_preserves_them_through_refresh() {
    live_details("click");
}

#[test]
fn tap_opens_opted_in_live_details_and_preserves_them_through_refresh() {
    live_details("tap");
}

#[test]
fn keyboard_opens_opted_in_live_details_and_preserves_them_through_refresh() {
    live_details("keyboard");
}

/// O corpo de `bottom-aligned details remain above the input boundary at ${zoom} zoom with ${contentHeight}px content`.
fn bottom_aligned_details(zoom: f64, content_height: f64) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let content = HoverElement::new("Background shell details");
    // content.style.cssText = `width: 200px; height: ${contentHeight}px;`
    content.set_size(200.0, content_height);
    let item = {
        let content = content.clone();
        move || {
            rc(IActionListItem {
                hover: Some(IActionListItemHover {
                    content: Some(HoverContent::Element(content.clone())),
                    expandable: true,
                    align_to_parent_bottom: true,
                    ..IActionListItemHover::default()
                }),
                open_submenu_on_click: true,
                ..action("shell")
            })
        }
    };
    let widget = create_action_list_widget(&guard, Config { list_options: no_filter(), ..config(vec![(*item()).clone()]) });
    // popup: position: fixed; top: 160px; left: 40px; zoom. O `.action-widget` tem padding de 4px e borda de 1px.
    let popup = ViewElement::with_classes(&["action-widget"]);
    popup.set_position(160.0, 40.0);
    widget.dom_node().set_parent(Some(popup.clone()));
    widget.set_zoom_level(zoom);
    widget.layout(24.0, Some(240.0));
    popup.set_size((240.0 + 10.0) * zoom, (24.0 + 10.0) * zoom);
    widget.dom_node().set_position(160.0 + 5.0 * zoom, 40.0 + 5.0 * zoom);
    widget.dom_node().set_size(240.0 * zoom, 24.0 * zoom);
    widget.focus();
    widget.accept_selected(false);
    let initial = widget.panel_rect().unwrap();
    let preserve_hover = Some(IActionListUpdateOptions { preserve_hover: true, ..IActionListUpdateOptions::default() });
    for _ in 0..3 {
        widget.update_items(&[item()], None, preserve_hover);
    }
    let updated = widget.panel_rect().unwrap();
    let popup_rect = popup.rect();
    let bottom = popup_rect.top + popup_rect.height;

    assert_eq!(
        (
            initial.top + initial.height <= bottom + 1.0,
            updated.top + updated.height <= bottom + 1.0,
            updated.top >= -1.0,
            (updated.height - initial.height).abs() < 1.0,
            updated.height > 0.0,
        ),
        (true, true, true, true, true),
    );
}

#[test]
fn bottom_aligned_details_remain_above_the_input_boundary_at_1_zoom_with_80px_content() {
    bottom_aligned_details(1.0, 80.0);
}

#[test]
fn bottom_aligned_details_remain_above_the_input_boundary_at_1_zoom_with_800px_content() {
    bottom_aligned_details(1.0, 800.0);
}

#[test]
fn bottom_aligned_details_remain_above_the_input_boundary_at_1_25_zoom_with_80px_content() {
    bottom_aligned_details(1.25, 80.0);
}

#[test]
fn bottom_aligned_details_remain_above_the_input_boundary_at_1_25_zoom_with_800px_content() {
    bottom_aligned_details(1.25, 800.0);
}

#[test]
fn keyboard_activation_on_an_opted_in_submenu_row_focuses_its_filter_without_selecting_it() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let selected = Rc::new(RefCell::new(Vec::new()));
    let widget = create_action_list_widget(
        &guard,
        Config {
            on_select: recording(&selected),
            list_options: no_filter(),
            ..config(vec![IActionListItem {
                submenu_actions: Some(vec![to_action_with("child", "Child", None, None, noop())]),
                open_submenu_on_click: true,
                submenu_options: Some(Rc::new(IActionListOptions { show_filter: true, focus_filter_on_open: true, ..IActionListOptions::default() })),
                ..action("remote")
            }])
        },
    );
    widget.focus();

    widget.accept_selected(false);

    let row = (0..widget.visible_labels().len())
        .filter_map(|index| widget.row_state(index))
        .find(|row| row.title_text == "remote")
        .unwrap();
    let filter_focused = widget.submenu_widget().and_then(|submenu| submenu.filter_input()).is_some_and(|input| input.is_focused());
    assert_eq!((selected.borrow().clone(), row.aria_expanded, filter_focused), (Vec::<String>::new(), Some("true".to_owned()), true));
}

/// O corpo de `stops mapping mouse moves after ${activation} enables hover`.
fn stops_mapping_mouse_moves_after(activation: &str) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(&guard, Config { list_options: no_filter(), ..config(vec![action("first"), action("second")]) });

    let mapped_activation = widget.is_mapping_pointer_moves();
    if activation == "mousemove" {
        widget.pointer_move(1, &moved(1.0, 0.0));
    } else {
        widget.pointer_down();
    }
    let mut mapped_subsequent_movement = false;
    for event in [moved(1.0, 0.0), moved(0.0, 1.0)] {
        mapped_subsequent_movement |= widget.is_mapping_pointer_moves();
        widget.pointer_move(1, &event);
    }
    assert_eq!((mapped_activation, mapped_subsequent_movement), (true, false));
}

#[test]
fn stops_mapping_mouse_moves_after_mousemove_enables_hover() {
    stops_mapping_mouse_moves_after("mousemove");
}

#[test]
fn stops_mapping_mouse_moves_after_mousedown_enables_hover() {
    stops_mapping_mouse_moves_after("mousedown");
}

#[test]
fn initial_pointer_movement_and_clicks_do_not_select_disabled_items() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let selected = Rc::new(RefCell::new(Vec::new()));
    let widget = create_action_list_widget(
        &guard,
        Config {
            on_select: recording(&selected),
            list_options: no_filter(),
            ..config(vec![action("first"), IActionListItem { disabled: true, ..action("disabled") }])
        },
    );
    widget.focus();
    widget.pointer_over(1, &PointerEvent::default());
    widget.pointer_move(1, &moved(1.0, 0.0));
    widget.pointer_down();
    widget.click_row(1, PointerTarget::Row);
    assert_eq!(selected.borrow().clone(), Vec::<String>::new());
}

#[test]
fn initial_hover_does_not_open_a_submenu_until_the_pointer_moves() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let widget = create_action_list_widget(
            &guard,
            Config {
                list_options: no_filter(),
                ..config(vec![
                    action("first"),
                    IActionListItem {
                        hover: Some(IActionListItemHover { content: Some(HoverContent::Text("Details".to_owned())), ..IActionListItemHover::default() }),
                        ..action("second")
                    },
                ])
            },
        );
        widget.focus();
        widget.pointer_over(1, &PointerEvent::default());
        widget.pointer_move(1, &PointerEvent::default());
        timeout(500).await.unwrap();
        assert!(!widget.panel_displayed());

        widget.pointer_move(1, &moved(0.0, 1.0));
        timeout(500).await.unwrap();
        assert!(widget.panel_displayed());
    });
}

/// O corpo de `preserves pending hover through replacement`, `cancels pending hover when its entry is removed` e
/// `cancels pending hover when preservation is disabled`.
fn pending_hover(preserve_hover: bool, remove: bool, expected: &'static str) {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let entry = |details: &str| {
            IActionListItem {
                hover: Some(IActionListItemHover { content: Some(HoverContent::Text(details.to_owned())), ..IActionListItemHover::default() }),
                ..action("entry")
            }
        };
        let widget = create_action_list_widget(&guard, Config { list_options: no_filter(), ..config(vec![entry("Initial details")]) });
        widget.focus();
        widget.pointer_over(0, &PointerEvent::default());
        widget.pointer_move(0, &moved(1.0, 0.0));
        timeout(100).await.unwrap();
        let replacement = if remove { Vec::new() } else { vec![rc(entry("Updated details"))] };
        widget.update_items(&replacement, None, Some(IActionListUpdateOptions { preserve_hover, ..IActionListUpdateOptions::default() }));
        timeout(399).await.unwrap();
        let before_delay = widget.panel_text();
        timeout(1).await.unwrap();

        assert_eq!((before_delay, widget.panel_text()), (String::new(), expected.to_owned()));
    });
}

#[test]
fn preserves_pending_hover_through_replacement() {
    pending_hover(true, false, "Updated details");
}

#[test]
fn cancels_pending_hover_when_its_entry_is_removed() {
    pending_hover(true, true, "");
}

#[test]
fn cancels_pending_hover_when_preservation_is_disabled() {
    pending_hover(false, false, "");
}

/// O corpo de `opening ${count} interactive previews stays quiet until intentional hover`.
fn interactive_previews_stay_quiet(count: usize) {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let rendered = Rc::new(Cell::new(0));
        let items = (0..count)
            .map(|index| {
                let rendered = rendered.clone();
                IActionListItem {
                    hover: Some(IActionListItemHover {
                        content: Some(HoverContent::Lazy(Rc::new(move || {
                            rendered.set(rendered.get() + 1);
                            HoverElement::new(&format!("Details {index}"))
                        }))),
                        expandable: true,
                        tab_through_panel: true,
                        ..IActionListItemHover::default()
                    }),
                    ..action(&format!("item-{index}"))
                }
            })
            .collect();
        let widget = create_action_list_widget(&guard, Config { list_options: no_filter(), ..config(items) });
        widget.focus();
        widget.pointer_over(0, &PointerEvent::default());
        widget.pointer_move(0, &PointerEvent::default());
        timeout(1000).await.unwrap();
        let initial = (focused_id(&widget), !widget.panel_displayed(), rendered.get());
        widget.pointer_move(0, &moved(1.0, 0.0));
        timeout(499).await.unwrap();
        let before_delay = rendered.get();
        timeout(1).await.unwrap();

        assert_eq!(
            (initial, before_delay, rendered.get(), widget.panel_text()),
            ((Some("item-0".to_owned()), true, 0), 0, 1, "Details 0".to_owned()),
        );
    });
}

#[test]
fn opening_1_interactive_previews_stays_quiet_until_intentional_hover() {
    interactive_previews_stay_quiet(1);
}

#[test]
fn opening_3_interactive_previews_stays_quiet_until_intentional_hover() {
    interactive_previews_stay_quiet(3);
}

#[test]
fn opening_30_interactive_previews_stays_quiet_until_intentional_hover() {
    interactive_previews_stay_quiet(30);
}

/// O corpo de `${key} reveals an interactive preview after quiet initial focus`.
fn key_reveals_an_interactive_preview(key: &str) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let controls: Rc<RefCell<Vec<Rc<HoverElement>>>> = Rc::new(RefCell::new(Vec::new()));
    let items = ["first", "second"]
        .into_iter()
        .map(|id| {
            let (build_controls, tabbable_controls) = (controls.clone(), controls.clone());
            IActionListItem {
                hover: Some(IActionListItemHover {
                    content: Some(HoverContent::Lazy(Rc::new(move || {
                        let button = HoverElement::new(&format!("Details {id}"));
                        *build_controls.borrow_mut() = vec![button.clone()];
                        button
                    }))),
                    expandable: true,
                    tab_through_panel: true,
                    get_tabbable_elements: Some(Rc::new(move || tabbable_controls.borrow().clone())),
                    ..IActionListItemHover::default()
                }),
                ..action(id)
            }
        })
        .collect();
    let widget = create_action_list_widget(&guard, Config { list_options: no_filter(), ..config(items) });
    widget.focus();
    let initially_hidden = !widget.panel_displayed();
    if key == "ArrowDown" {
        widget.focus_next();
    } else {
        widget.dispatch_key_down(&key_event(key), KeyTarget::DomNode);
    }
    let expected = format!("Details {}", if key == "ArrowDown" { "second" } else { "first" });

    assert_eq!((initially_hidden, widget.panel_displayed(), widget.panel_text()), (true, true, expected));
}

#[test]
fn arrowright_reveals_an_interactive_preview_after_quiet_initial_focus() {
    key_reveals_an_interactive_preview("ArrowRight");
}

#[test]
fn tab_reveals_an_interactive_preview_after_quiet_initial_focus() {
    key_reveals_an_interactive_preview("Tab");
}

#[test]
fn arrowdown_reveals_an_interactive_preview_after_quiet_initial_focus() {
    key_reveals_an_interactive_preview("ArrowDown");
}

#[test]
fn unrelated_keys_and_modified_tab_do_not_render_an_initially_quiet_preview() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let renders = Rc::new(Cell::new(0));
    let build_renders = renders.clone();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![IActionListItem {
                hover: Some(IActionListItemHover {
                    content: Some(HoverContent::Lazy(Rc::new(move || {
                        build_renders.set(build_renders.get() + 1);
                        HoverElement::new("")
                    }))),
                    tab_through_panel: true,
                    ..IActionListItemHover::default()
                }),
                ..action("reference")
            }])
        },
    );
    widget.focus();
    let mut ctrl_tab = key_event("Tab");
    ctrl_tab.ctrl_key = true;
    let mut meta_tab = key_event("Tab");
    meta_tab.meta_key = true;
    let events = [key_event("x"), ctrl_tab, meta_tab, key_event("Escape")];
    for event in &events {
        widget.dispatch_key_down(event, KeyTarget::DomNode);
    }
    assert_eq!((renders.get(), !widget.panel_displayed()), (0, true));
}

/// O `onChange` do toggle que guarda o último valor.
fn toggle_with(label: &str, checked: bool, on_change: Rc<dyn Fn(bool)>) -> IActionListItemInlineToggle {
    IActionListItemInlineToggle { label: label.to_owned(), checked, on_change, title: None, show_info_icon: false, disabled: false }
}

#[test]
fn renders_and_activates_a_standalone_toggle_row() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let checked = Rc::new(Cell::new(false));
    let record = checked.clone();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![IActionListItem {
                standalone_toggle: Some(toggle_with("Sandboxing for terminal", false, Rc::new(move |value| record.set(value)))),
                ..action("Sandboxing for terminal")
            }])
        },
    );

    widget.focus();
    widget.accept_selected(false);

    let row = widget.row_state(0).unwrap();
    assert_eq!(
        (
            checked.get(),
            row.classes.iter().any(|class| class == "has-standalone-toggle"),
            row.title_text.clone(),
            row.inline_toggle_label_count,
            row.switch.map(|switch| switch.checked),
            row.title_attribute.clone(),
        ),
        (true, true, "Sandboxing for terminal".to_owned(), 0, Some(true), String::new()),
    );
}

#[test]
fn does_not_activate_a_disabled_standalone_toggle_row() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let change_count = Rc::new(Cell::new(0));
    let record = change_count.clone();
    let widget = create_action_list_widget(
        &guard,
        config(vec![IActionListItem {
            standalone_toggle: Some(IActionListItemInlineToggle {
                title: Some("Managed by your organization".to_owned()),
                disabled: true,
                ..toggle_with("Sandboxing for terminal", true, Rc::new(move |_| record.set(record.get() + 1)))
            }),
            ..action("Sandboxing for terminal")
        }]),
    );

    widget.focus();
    widget.accept_selected(false);
    let toggle = widget.row_state(0).unwrap().switch.unwrap();

    assert_eq!(
        (change_count.get(), toggle.checked, toggle.disabled, toggle.aria_label),
        (0, true, true, "Managed by your organization".to_owned()),
    );
}

#[test]
fn renders_an_optional_information_icon_after_the_standalone_toggle_label() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let label = "Sandboxing in Dev Container";
    let title = "This relaxes the outer container isolation.";
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![IActionListItem {
                hover: Some(IActionListItemHover { content: Some(HoverContent::Text(title.to_owned())), ..IActionListItemHover::default() }),
                standalone_toggle: Some(IActionListItemInlineToggle {
                    title: Some(title.to_owned()),
                    show_info_icon: true,
                    ..toggle_with(label, true, Rc::new(|_| {}))
                }),
                ..action(label)
            }])
        },
    );
    let row = widget.row_state(0).unwrap();
    let icon = row.info_icon.clone().expect("o ícone de informação");
    assert_eq!(
        (row.title_label_text.clone(), icon.is_last_child_of_title, icon.aria_hidden, row.switch.unwrap().aria_label),
        (label.to_owned(), true, "true".to_owned(), title.to_owned()),
    );

    widget.update_items(
        &[rc(IActionListItem {
            standalone_toggle: Some(toggle_with("Sandboxing for terminal", true, Rc::new(|_| {}))),
            ..action("Sandboxing for terminal")
        })],
        None,
        None,
    );
    assert_eq!(widget.row_state(0).unwrap().info_icon, None);
}

/// O corpo de `centers the toggle information icon beside its label at ${fontSize}px`: o rótulo tem `line-height` de 20px.
fn centers_the_toggle_information_icon(_font_size: f64) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let label = "Sandboxing in Dev Container";
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![IActionListItem {
                standalone_toggle: Some(IActionListItemInlineToggle { show_info_icon: true, ..toggle_with(label, true, Rc::new(|_| {})) }),
                ..action(label)
            }])
        },
    );
    widget.layout(400.0, Some(200.0));
    let row = widget.row_state(0).unwrap();
    assert!(row.info_icon.is_some() && row.title_centers_info_icon);
    let offset = toggle_info_icon_center_offset(20.0);
    assert!(offset.abs() <= 1.0, "Information icon is vertically offset by {offset}px");
}

#[test]
fn centers_the_toggle_information_icon_beside_its_label_at_13px() {
    centers_the_toggle_information_icon(13.0);
}

#[test]
fn centers_the_toggle_information_icon_beside_its_label_at_16px() {
    centers_the_toggle_information_icon(16.0);
}

#[test]
fn centers_the_toggle_information_icon_beside_its_label_at_18px() {
    centers_the_toggle_information_icon(18.0);
}

#[test]
fn nested_action_groups_expose_a_keyboard_accessible_submenu_and_select_the_parent_item() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let selected = Rc::new(RefCell::new(Vec::new()));
    let record = selected.clone();
    let container = to_action_with("container", "Use Dev Container", None, None, Rc::new(move || record.borrow_mut().push("container".to_owned())));
    let workspace = SubmenuAction::new("workspace", "Workspace", vec![container], None);
    let hosts = SubmenuAction::new("hosts", "", vec![workspace as ActionRef], None);
    let widget = create_action_list_widget(
        &guard,
        Config {
            on_select: recording(&selected),
            ..config(vec![IActionListItem { submenu_actions: Some(vec![hosts as ActionRef]), ..action("remote") }])
        },
    );
    widget.focus();
    widget.dispatch_key_down(&key_event("ArrowRight"), KeyTarget::DomNode);
    let submenu = widget.submenu_widget().expect("o submenu");
    assert!(submenu.row_state(0).unwrap().submenu_indicator);
    submenu.dispatch_key_down(&key_event("ArrowRight"), KeyTarget::DomNode);
    assert!(submenu.submenu_widget().is_some());
    let submenu_animated = widget.panel_animated();
    let nested_animated = submenu.panel_animated();
    submenu.dispatch_key_down(&key_event("Enter"), KeyTarget::SubmenuPanel);

    assert_eq!((selected.borrow().clone(), submenu_animated, nested_animated), (strings(&["container", "remote"]), true, true));
}

#[test]
fn escape_from_a_submenu_hides_the_action_list() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let hide_count = Rc::new(Cell::new(0));
    let record = hide_count.clone();
    let widget = create_action_list_widget(
        &guard,
        Config {
            on_hide: Rc::new(move || record.set(record.get() + 1)),
            ..config(vec![IActionListItem { submenu_actions: Some(vec![to_action_with("child", "Child", None, None, noop())]), ..action("parent") }])
        },
    );

    widget.dispatch_key_down(&key_event("ArrowRight"), KeyTarget::DomNode);
    assert!(widget.submenu_widget().is_some());
    widget.dispatch_key_down(&key_event("Escape"), KeyTarget::SubmenuPanel);

    assert_eq!(hide_count.get(), 1);
}

#[test]
fn hovering_back_to_the_parent_keeps_focus_in_the_menu_when_a_submenu_was_focused() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut states = Vec::new();
        for hover in [None, Some(IActionListItemHover { content: Some(HoverContent::Text("Mode details".to_owned())), ..IActionListItemHover::default() })] {
            let widget = create_action_list_widget(
                &guard,
                Config {
                    list_options: no_filter(),
                    ..config(vec![
                        IActionListItem { hover, ..action("mode") },
                        IActionListItem {
                            label: Some("Permissions".to_owned()),
                            submenu_actions: Some(vec![to_action_with("manual", "Manual", None, None, noop())]),
                            ..action("permissions")
                        },
                    ])
                },
            );
            widget.focus();
            widget.focus_next();
            widget.dispatch_key_down(&key_event("ArrowRight"), KeyTarget::List);
            let initially_focused_submenu = widget.panel_contains_focus();

            widget.pointer_over(0, &PointerEvent::default());
            widget.pointer_move(0, &moved(1.0, 0.0));
            timeout(600).await.unwrap();

            states.push((initially_focused_submenu, widget.is_list_focused(), focused_id(&widget), widget.panel_text()));
        }
        assert_eq!(
            states,
            vec![
                (true, true, Some("mode".to_owned()), String::new()),
                (true, true, Some("mode".to_owned()), "Mode details".to_owned()),
            ],
        );
    });
}

#[test]
fn replacing_a_focused_submenu_moves_focus_before_disposing_its_contents() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(
                ["permissions", "configuration"]
                    .into_iter()
                    .map(|id| IActionListItem { submenu_actions: Some(vec![to_action_with(id, id, None, None, noop())]), ..action(id) })
                    .collect(),
            )
        },
    );
    widget.focus();
    widget.dispatch_key_down(&key_event("ArrowRight"), KeyTarget::List);
    widget.click_submenu_indicator(1);

    assert_eq!(
        (widget.is_list_focused(), widget.submenu_rows().and_then(|rows| rows.into_iter().next()).flatten()),
        (true, Some("configuration".to_owned())),
    );
}

#[test]
fn runs_dynamic_filter_updates_immediately() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let filters = Rc::new(RefCell::new(Vec::new()));
        let record = filters.clone();
        let widget = create_action_list_widget(
            &guard,
            Config {
                on_filter: Some(Rc::new(move |filter, _| {
                    record.borrow_mut().push(filter.to_owned());
                    let name = if filter == "ma" { "ranked" } else { filter };
                    Promise::resolved(vec![rc(action(&format!("server-{name}-result")))])
                })),
                ..config(Vec::new())
            },
        );

        widget.filter_input_input("m");
        widget.filter_input_input("ma");
        assert_eq!(filters.borrow().clone(), strings(&["m", "ma"]));
        timeout(0).await.unwrap();
        assert!(widget.visible_labels().contains(&Some("server-ranked-result".to_owned())));
    });
}

#[test]
fn ignores_stale_dynamic_filter_results() {
    common::run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let first_result: Rc<DeferredPromise<Vec<Rc<TestItem>>>> = Rc::new(DeferredPromise::new());
        let second_result: Rc<DeferredPromise<Vec<Rc<TestItem>>>> = Rc::new(DeferredPromise::new());
        let filters = Rc::new(RefCell::new(Vec::new()));
        let (record, first, second) = (filters.clone(), first_result.clone(), second_result.clone());
        let widget = create_action_list_widget(
            &guard,
            Config {
                on_filter: Some(Rc::new(move |filter, _| {
                    record.borrow_mut().push(filter.to_owned());
                    if filter == "m" { first.promise.clone() } else { second.promise.clone() }
                })),
                ..config(Vec::new())
            },
        );

        widget.filter_input_input("m");
        widget.filter_input_input("ma");
        assert_eq!(filters.borrow().clone(), strings(&["m", "ma"]));

        first_result.complete(vec![rc(action("ma-stale-result"))]);
        timeout(0).await.unwrap();
        assert!(!widget.visible_labels().contains(&Some("ma-stale-result".to_owned())));

        second_result.complete(vec![rc(action("ma-fresh-result"))]);
        timeout(0).await.unwrap();
        assert!(widget.visible_labels().contains(&Some("ma-fresh-result".to_owned())));
    });
}

#[test]
fn does_not_filter_while_an_ime_composition_is_in_progress() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let filters = Rc::new(RefCell::new(Vec::new()));
    let record = filters.clone();
    let widget = create_action_list_widget(
        &guard,
        Config {
            on_filter: Some(Rc::new(move |filter, _| {
                record.borrow_mut().push(filter.to_owned());
                Promise::resolved(vec![rc(action(&format!("result-{filter}")))])
            })),
            ..config(Vec::new())
        },
    );

    assert!(widget.filter_input().is_some());
    widget.filter_input_composition_start();
    widget.filter_input_input("d");
    widget.filter_input_input("deepseek");
    widget.filter_input().unwrap().set_value("DeepSeek");
    widget.filter_input_composition_end();
    // Chromium fires a trailing `input` for the committed text, which must not re-filter.
    widget.filter_input_input("DeepSeek");

    assert_eq!(filters.borrow().clone(), strings(&["DeepSeek"]));
}

#[test]
fn cancels_an_in_flight_dynamic_filter_when_a_composition_starts() {
    common::run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let pending: Rc<DeferredPromise<Vec<Rc<TestItem>>>> = Rc::new(DeferredPromise::new());
        let source = pending.clone();
        let widget = create_action_list_widget(&guard, Config { on_filter: Some(Rc::new(move |_, _| source.promise.clone())), ..config(Vec::new()) });

        widget.filter_input_input("d");
        assert!(widget.filter_input().is_some());
        widget.filter_input_composition_start();

        // Resolving now must not splice/re-layout the list underneath the IME candidate window.
        pending.complete(vec![rc(action("stale-result"))]);
        timeout(0).await.unwrap();
        assert!(!widget.visible_labels().contains(&Some("stale-result".to_owned())));
    });
}

/// `keyCode` na codificação do VS Code, com o `key` do DOM.
fn key_with_code(key: &str, key_code: KeyCode) -> UiKeyEvent {
    UiKeyEvent::new(key, key_code)
}

/// `dispatchKeyDown(target, init)`: devolve o evento para ler `defaultPrevented`.
fn dispatch_key(widget: &ActionListWidget<Item>, event: UiKeyEvent, target: KeyTarget) -> UiKeyEvent {
    widget.dispatch_key_down(&event, target);
    event
}

/// O `aria-activedescendant` do campo de filtro resolvido para a linha (`document.getElementById(...)?.textContent`).
fn active_result_text(widget: &ActionListWidget<Item>) -> Option<String> {
    let id = widget.filter_aria()?.active_descendant?;
    widget.row_by_element_id(&id).map(|row| row.title_text)
}

mod combobox_filtering {
    use super::*;

    /// `{ filterAsCombobox: true, focusFilterOnOpen: true }` com o filtro à mostra (`showFilter: true` é o padrão do helper).
    fn combobox_options() -> IActionListOptions {
        IActionListOptions { show_filter: true, filter_as_combobox: true, focus_filter_on_open: true, ..IActionListOptions::default() }
    }

    #[test]
    fn initializes_the_query_and_exposes_the_active_result_to_assistive_technology() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let widget = create_action_list_widget(
            &guard,
            Config {
                list_options: IActionListOptions {
                    initial_filter_value: Some("al".to_owned()),
                    filter_placeholder: Some("Search models".to_owned()),
                    ..combobox_options()
                },
                ..config(vec![action("alpha"), action("beta"), action("alpine")])
            },
        );
        widget.focus();
        let input = widget.filter_input().expect("o campo de filtro");
        let aria = widget.filter_aria().expect("os atributos do campo");
        let controlled_list_role = aria.controls.as_deref().and_then(|id| widget.list_role_by_id(id));

        assert_eq!(
            (
                input.value(),
                visible_row_text(&widget),
                input.is_focused(),
                aria.role.clone(),
                aria.label.clone(),
                aria.autocomplete.clone(),
                aria.expanded.clone(),
                controlled_list_role,
                active_result_text(&widget),
            ),
            (
                "al".to_owned(),
                strings(&["alpha", "alpine"]),
                true,
                Some("combobox".to_owned()),
                "Search models".to_owned(),
                Some("list".to_owned()),
                Some("true".to_owned()),
                Some("listbox".to_owned()),
                Some("alpha".to_owned()),
            ),
        );
    }

    #[test]
    fn arrow_navigation_skips_disabled_rows_and_separators_without_leaving_the_input() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let widget = create_action_list_widget(
            &guard,
            Config {
                list_options: IActionListOptions { initial_filter_value: Some("a".to_owned()), ..combobox_options() },
                ..config(vec![action("alpha"), separator(Some("Models")), IActionListItem { disabled: true, ..action("beta") }, action("gamma")])
            },
        );
        widget.focus();
        let input = widget.filter_input().expect("o campo de filtro");
        input.set_selection_range(0, 1);

        #[derive(Debug, PartialEq)]
        struct State {
            result: Option<String>,
            input_focused: bool,
            selection: (usize, usize),
            active_result: Option<String>,
            default_prevented: bool,
        }
        let states: Vec<State> = [("ArrowDown", KeyCode::DownArrow), ("ArrowUp", KeyCode::UpArrow), ("ArrowUp", KeyCode::UpArrow), ("ArrowDown", KeyCode::DownArrow)]
            .into_iter()
            .map(|(key, key_code)| {
                let event = dispatch_key(&widget, key_with_code(key, key_code), KeyTarget::FilterInput);
                State {
                    result: widget.get_focused_element().and_then(|element| element.label.clone()),
                    input_focused: input.is_focused(),
                    selection: input.selection(),
                    active_result: active_result_text(&widget),
                    default_prevented: event.default_prevented(),
                }
            })
            .collect();

        assert_eq!(
            states,
            ["gamma", "alpha", "gamma", "alpha"]
                .into_iter()
                .map(|result| State {
                    result: Some(result.to_owned()),
                    input_focused: true,
                    selection: (0, 1),
                    active_result: Some(result.to_owned()),
                    default_prevented: true,
                })
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn typing_can_refine_the_query_after_navigation_and_enter_accepts_the_new_result() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let selected = Rc::new(RefCell::new(Vec::new()));
        let widget = create_action_list_widget(
            &guard,
            Config { on_select: recording(&selected), list_options: combobox_options(), ..config(vec![action("first"), action("second"), action("third")]) },
        );
        widget.focus();
        let input = widget.filter_input().expect("o campo de filtro");
        dispatch_key(&widget, key_with_code("ArrowDown", KeyCode::DownArrow), KeyTarget::FilterInput);
        widget.filter_input_input("thi");
        dispatch_key(&widget, key_with_code("Enter", KeyCode::Enter), KeyTarget::FilterInput);

        assert_eq!(
            (input.value(), input.is_focused(), visible_row_text(&widget), selected.borrow().clone()),
            ("thi".to_owned(), true, strings(&["third"]), strings(&["third"])),
        );
    }

    #[test]
    fn empty_and_disabled_results_cannot_accept_a_stale_selection() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let selected = Rc::new(RefCell::new(Vec::new()));
        let widget = create_action_list_widget(
            &guard,
            Config {
                on_select: recording(&selected),
                list_options: combobox_options(),
                ..config(vec![action("first"), IActionListItem { disabled: true, ..action("disabled") }])
            },
        );
        widget.focus();
        let input = widget.filter_input().expect("o campo de filtro");
        let states: Vec<_> = ["missing", "disabled"]
            .into_iter()
            .map(|query| {
                widget.filter_input_input(query);
                dispatch_key(&widget, key_with_code("ArrowDown", KeyCode::DownArrow), KeyTarget::FilterInput);
                dispatch_key(&widget, key_with_code("ArrowUp", KeyCode::UpArrow), KeyTarget::FilterInput);
                dispatch_key(&widget, key_with_code("Enter", KeyCode::Enter), KeyTarget::FilterInput);
                (input.is_focused(), widget.filter_aria().and_then(|aria| aria.active_descendant), focused_id(&widget), selected.borrow().clone())
            })
            .collect();

        assert_eq!(states, vec![(true, None, None, Vec::<String>::new()); 2]);
    }

    #[test]
    fn unrelated_shortcuts_still_reach_the_workbench_keybinding_handler() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let widget = create_action_list_widget(&guard, Config { list_options: combobox_options(), ..config(Vec::new()) });
        widget.focus();
        let mut ctrl_shift_p = key_with_code("P", KeyCode::KeyP);
        ctrl_shift_p.ctrl_key = true;
        ctrl_shift_p.shift_key = true;
        let mut meta_shift_p = key_with_code("P", KeyCode::KeyP);
        meta_shift_p.meta_key = true;
        meta_shift_p.shift_key = true;
        let mut ctrl_enter = key_with_code("Enter", KeyCode::Enter);
        ctrl_enter.ctrl_key = true;
        let events: Vec<UiKeyEvent> = [ctrl_shift_p, meta_shift_p, ctrl_enter].into_iter().map(|event| dispatch_key(&widget, event, KeyTarget::FilterInput)).collect();

        // O ouvinte de `keydown` no `filterContainer` só recebe o evento que ninguém parou no campo.
        let received: Vec<String> = events.iter().filter(|event| !event.propagation_stopped()).map(|event| event.key.clone()).collect();
        let prevented: Vec<String> = events.iter().filter(|event| event.default_prevented()).map(|event| event.key.clone()).collect();
        assert_eq!((received, prevented), (strings(&["P", "P", "Enter"]), Vec::<String>::new()));
    }

    /// O corpo de `retaining filter focus is opt-in: ${filterAsCombobox}`.
    fn retaining_filter_focus_is_opt_in(filter_as_combobox: bool) {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let widget = create_action_list_widget(
            &guard,
            Config {
                list_options: IActionListOptions { filter_as_combobox, ..combobox_options() },
                ..config(vec![action("first"), action("second")])
            },
        );
        widget.focus();
        widget.focus_next();

        let aria = widget.filter_aria().expect("os atributos do campo");
        assert_eq!(
            (focused_id(&widget), widget.filter_input().is_some_and(|input| input.is_focused()), aria.role),
            (Some("second".to_owned()), filter_as_combobox, if filter_as_combobox { Some("combobox".to_owned()) } else { None }),
        );
    }

    #[test]
    fn retaining_filter_focus_is_opt_in_undefined() {
        retaining_filter_focus_is_opt_in(false);
    }

    #[test]
    fn retaining_filter_focus_is_opt_in_true() {
        retaining_filter_focus_is_opt_in(true);
    }
}

#[test]
fn typing_requests_a_filter_instead_of_performing_list_type_navigation_when_opted_in() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let typed = Rc::new(RefCell::new(Vec::new()));
    let record = typed.clone();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: IActionListOptions { show_filter: false, on_type: Some(Rc::new(move |text| record.borrow_mut().push(text.to_owned()))), ..IActionListOptions::default() },
            ..config(vec![action("alpha"), action("beta")])
        },
    );
    widget.focus();
    let event = dispatch_key(&widget, key_with_code("b", KeyCode::KeyB), KeyTarget::List);

    assert_eq!((typed.borrow().clone(), focused_id(&widget), event.default_prevented()), (strings(&["b"]), Some("alpha".to_owned()), true));
}

#[test]
fn type_to_filter_leaves_shortcuts_composition_and_embedded_controls_alone() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let typed = Rc::new(RefCell::new(Vec::new()));
    let record = typed.clone();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: IActionListOptions { show_filter: false, on_type: Some(Rc::new(move |text| record.borrow_mut().push(text.to_owned()))), ..IActionListOptions::default() },
            ..config(Vec::new())
        },
    );
    widget.focus();
    let modified = |ctrl: bool, meta: bool, alt: bool, composing: bool| {
        let mut event = key_with_code("b", KeyCode::KeyB);
        event.ctrl_key = ctrl;
        event.meta_key = meta;
        event.alt_key = alt;
        event.is_composing = composing;
        event
    };
    let mut events: Vec<UiKeyEvent> = vec![
        dispatch_key(&widget, key_with_code(" ", KeyCode::Space), KeyTarget::List),
        dispatch_key(&widget, modified(true, false, false, false), KeyTarget::List),
        dispatch_key(&widget, modified(false, true, false, false), KeyTarget::List),
        dispatch_key(&widget, modified(false, false, true, false), KeyTarget::List),
        dispatch_key(&widget, modified(false, false, false, true), KeyTarget::List),
    ];
    // `input`, `textarea`, `button`, `a` e o `span` dentro de um `contenteditable="true"`.
    for _ in 0..5 {
        events.push(dispatch_key(&widget, key_with_code("b", KeyCode::KeyB), KeyTarget::EmbeddedControl));
    }
    events.push(dispatch_key(&widget, key_with_code("b", KeyCode::KeyB), KeyTarget::SubmenuPanel));

    let prevented: Vec<String> = events.iter().filter(|event| event.default_prevented()).map(|event| event.key.clone()).collect();
    assert_eq!((typed.borrow().clone(), prevented), (Vec::<String>::new(), Vec::<String>::new()));
}

/// Os pares `(filter, request)` que um `onFilter` dinâmico recebeu, e a promessa que cada chamada devolve.
struct FilterLog {
    filters: Rc<RefCell<Vec<String>>>,
    requests: Rc<RefCell<Vec<FilterRequest>>>,
}

impl FilterLog {
    fn new() -> Self {
        Self { filters: Rc::default(), requests: Rc::default() }
    }

    /// O `onFilter` que registra e devolve `answer(número da chamada)`.
    fn on_filter(&self, answer: impl Fn(usize) -> Promise<Vec<Rc<TestItem>>> + 'static) -> FilterFn {
        let (filters, requests) = (self.filters.clone(), self.requests.clone());
        Rc::new(move |filter, request| {
            filters.borrow_mut().push(filter.to_owned());
            requests.borrow_mut().push(request.clone());
            answer(filters.borrow().len())
        })
    }

    fn cancelled(&self) -> Vec<bool> {
        self.requests.borrow().iter().map(FilterRequest::is_cancellation_requested).collect()
    }
}

fn deferred() -> Rc<DeferredPromise<Vec<Rc<TestItem>>>> {
    Rc::new(DeferredPromise::new())
}

fn shows(widget: &ActionListWidget<Item>, label: &str) -> bool {
    widget.visible_labels().contains(&Some(label.to_owned()))
}

#[test]
fn restarts_a_cancelled_dynamic_filter_when_ime_composition_leaves_the_query_unchanged() {
    common::run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (first, second) = (deferred(), deferred());
        let log = FilterLog::new();
        let (answer_first, answer_second) = (first.clone(), second.clone());
        let widget = create_action_list_widget(
            &guard,
            Config { on_filter: Some(log.on_filter(move |call| if call == 1 { answer_first.promise.clone() } else { answer_second.promise.clone() })), ..config(Vec::new()) },
        );

        widget.filter_input_input("release");
        widget.filter_input_composition_start();
        widget.filter_input_input("releases");
        widget.filter_input().unwrap().set_value("release");
        widget.filter_input_composition_end();
        widget.filter_input_input("release");
        first.complete(vec![rc(action("release-stale-result"))]);
        second.complete(vec![rc(action("release-fresh-result"))]);
        timeout(0).await.unwrap();

        assert_eq!(
            (log.filters.borrow().clone(), log.cancelled(), shows(&widget, "release-stale-result"), shows(&widget, "release-fresh-result")),
            (strings(&["release", "release"]), vec![true, false], false, true),
        );
    });
}

/// O corpo de `does not retry a ${outcome} dynamic filter when IME composition leaves the query unchanged`.
fn does_not_retry_a_settled_dynamic_filter(completed: bool) {
    common::run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let result = deferred();
        let log = FilterLog::new();
        let answer = result.clone();
        let widget = create_action_list_widget(&guard, Config { on_filter: Some(log.on_filter(move |_| answer.promise.clone())), ..config(Vec::new()) });
        widget.filter_input_input("release");
        if completed {
            result.complete(vec![rc(action("release-result"))]);
        } else {
            result.error(Error::new("Search failed".to_owned()));
        }
        timeout(0).await.unwrap();
        let rows = visible_row_text(&widget);
        widget.filter_input_composition_start();
        widget.filter_input_input("releases");
        widget.filter_input().unwrap().set_value("release");
        widget.filter_input_composition_end();
        widget.filter_input_input("release");
        timeout(0).await.unwrap();

        assert_eq!((log.filters.borrow().clone(), log.cancelled(), visible_row_text(&widget)), (strings(&["release"]), vec![false], rows));
    });
}

#[test]
fn does_not_retry_a_completed_dynamic_filter_when_ime_composition_leaves_the_query_unchanged() {
    does_not_retry_a_settled_dynamic_filter(true);
}

#[test]
fn does_not_retry_a_failed_dynamic_filter_when_ime_composition_leaves_the_query_unchanged() {
    does_not_retry_a_settled_dynamic_filter(false);
}

#[test]
fn settling_a_superseded_dynamic_filter_preserves_ime_retry_for_the_pending_request() {
    common::run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (first, second, retry) = (deferred(), deferred(), deferred());
        let log = FilterLog::new();
        let (answer_first, answer_second, answer_retry) = (first.clone(), second.clone(), retry.clone());
        let widget = create_action_list_widget(
            &guard,
            Config {
                on_filter: Some(log.on_filter(move |call| match call {
                    1 => answer_first.promise.clone(),
                    2 => answer_second.promise.clone(),
                    _ => answer_retry.promise.clone(),
                })),
                ..config(Vec::new())
            },
        );
        widget.filter_input_input("old");
        widget.filter_input_input("release");
        first.complete(vec![rc(action("old-result"))]);
        timeout(0).await.unwrap();
        widget.filter_input_composition_start();
        widget.filter_input_composition_end();
        widget.filter_input_input("release");
        second.complete(vec![rc(action("release-stale-result"))]);
        retry.complete(vec![rc(action("release-fresh-result"))]);
        timeout(0).await.unwrap();

        assert_eq!(
            (log.filters.borrow().clone(), shows(&widget, "release-stale-result"), shows(&widget, "release-fresh-result")),
            (strings(&["old", "release", "release"]), false, true),
        );
    });
}

#[test]
fn batches_row_width_writes_before_reading_layout() {
    // O DOM mede as linhas com `getBoundingClientRect` depois de pôr todas em `width: auto` (escritas antes das leituras, sem
    // layout forçado entre elas) e as restaura depois. A lista nativa não escreve estilo nas linhas: a medida de cada linha é a
    // do `ItemMeasurer`, que o `measure_item_widths` lê de uma vez para todas, e a largura é a maior mais a barra de ferramentas.
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![
                action("first"),
                IActionListItem { toolbar_actions: Some(vec![to_action_with("toolbar", "Toolbar", None, None, noop())]), ..action("second") },
                action("third"),
            ])
        },
    );
    widget.set_item_measurer(Rc::new(|item| match item.label.as_deref() {
        Some("first") => 120.0,
        Some("second") => 240.0,
        _ => 180.0,
    }));

    assert_eq!(widget.compute_max_width(0.0), 278.0);
}

#[test]
fn does_not_double_count_a_detail_row_toolbar_when_computing_max_width() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![IActionListItem {
                detail: Some("Description".to_owned()),
                toolbar_actions: Some(vec![to_action_with("toolbar", "Toolbar", None, None, noop())]),
                ..action("detail")
            }])
        },
    );
    widget.set_item_measurer(Rc::new(|_| 240.0));

    assert_eq!(widget.compute_max_width(0.0), 240.0);
}

#[test]
fn keeps_titled_separator_above_first_filtered_match() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        config(vec![separator(Some("Provider A")), action("alpha"), separator(Some("Provider B")), action("beta")]),
    );

    widget.filter_input_input("alpha");

    assert_eq!(visible_row_text(&widget), strings(&["Provider A", "alpha"]));
}

#[test]
fn keeps_only_titled_separators_for_sections_with_filtered_matches() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        config(vec![
            separator(Some("Provider A")),
            action("alpha"),
            separator(Some("Provider B")),
            action("beta"),
            separator(Some("Provider C")),
            action("gamma"),
        ]),
    );

    widget.filter_input_input("beta");

    assert_eq!(visible_row_text(&widget), strings(&["Provider B", "beta"]));
}

#[test]
fn excludes_separators_from_accessible_list_positions_after_filtering() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(&guard, config(vec![action("selected"), separator(None), action("alpha"), action("beta")]));
    let aria_positions = || -> Vec<(Option<String>, usize, usize)> {
        (0..widget.visible_labels().len())
            .filter_map(|index| widget.row_state(index))
            .filter(|row| row.role == "option")
            .map(|row| (row.aria_label, row.aria_set_size, row.aria_pos_in_set))
            .collect()
    };

    let initial = aria_positions();
    widget.filter_input_input("a");
    let filtered = aria_positions();

    let position = |label: &str, set_size: usize, pos_in_set: usize| (Some(label.to_owned()), set_size, pos_in_set);
    assert_eq!(
        (initial, filtered),
        (
            vec![position("selected", 3, 1), position("alpha", 3, 2), position("beta", 3, 3)],
            vec![position("alpha", 2, 1), position("beta", 2, 2)],
        ),
    );
}

