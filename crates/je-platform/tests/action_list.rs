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
use je_base::async_::{async_fn, timeout};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::key_codes::KeyCode;
use je_base::layout::{AnchorPosition, IAnchor};
use je_base::promise::{DeferredPromise, Promise};
use je_platform::action_widget::action_list::{
    ACCEPT_SELECTED_ACTION_COMMAND, ACTION_WIDGET_VERTICAL_CHROME_HEIGHT, ActionList, ActionListBadge, ActionListGroup, ActionListItemData, ActionListItemKind,
    ActionListWidget, CssColor, FilterRequest, HoverContent, HoverElement, IActionListDelegate, IActionListHeaderLink, IActionListItem, IActionListItemHover,
    IActionListItemInlineToggle, IActionListOptions, IActionListUpdateOptions, KeyTarget, KeybindingResolver, LIST_VERTICAL_SCROLLBAR_SIZE, OnRemove, PointerEvent,
    PointerTarget, REMOVE_TOOLBAR_ACTION_ID, RemovableAction, ReserveSubmenuSpace, RowState, SubmenuGutter, action_item_badge_metrics, toggle_info_icon_center_offset,
};
use je_base::uri::URI;
use je_platform::contextview::context_view::{AnchorTarget, IContextViewDelegate, IContextViewService};
use je_platform::contextview::view_element::{UiKeyEvent, ViewElement};
use je_platform::layout::layout_service::{ILayoutService, LayoutContainerEvent, Viewport};

/// `ITestActionItem`: `{ id, checked? }`.
#[derive(Clone)]
struct Item {
    id: String,
    checked: bool,
}

impl ActionListItemData for Item {
    fn list_item_id(&self) -> Option<String> {
        Some(self.id.clone())
    }

    fn list_item_checked(&self) -> bool {
        self.checked
    }
}

type TestItem = IActionListItem<Item>;

/// `action(id)`.
fn action(id: &str) -> TestItem {
    IActionListItem::action(id, Item { id: id.to_owned(), checked: false })
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
    /// O `keybindingService` do `createActionListWidget`: o `softDispatch` que o campo de filtro de um submenu consulta.
    keybinding: Option<KeybindingResolver>,
}

/// Os `listOptions` de um caso (`{ showFilter: true, ...listOptions }`: o `show_filter` de quem chama vence).
fn config(items: Vec<TestItem>) -> Config {
    Config {
        items,
        on_filter: None,
        on_hide: noop(),
        on_select: Rc::new(|_| {}),
        list_options: IActionListOptions { show_filter: true, ..IActionListOptions::default() },
        keybinding: None,
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
    if let Some(keybinding) = config.keybinding {
        widget.set_keybinding_resolver(keybinding);
    }
    let body = ViewElement::new();
    // The header banner is a standalone element the caller attaches (like the filter container), so the test appends it.
    if let Some(header) = widget.header_container() {
        header.set_parent(Some(body.clone()));
    }
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
fn keeps_detail_row_geometry_stable_when_its_toolbar_becomes_visible() {
    // O CSS calculado e os retângulos são a `row_toolbar_geometry` do estado nativo; a classe `focused` posta à mão nas duas
    // linhas é o argumento `focused_or_hovered`.
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let toolbar = || Some(vec![to_action_with("toolbar", "Toolbar", None, None, noop())]);
    let mut items = vec![
        IActionListItem { toolbar_actions: toolbar(), ..action("plain") },
        IActionListItem { detail: Some("Description".to_owned()), toolbar_actions: toolbar(), ..action("detail") },
    ];
    items.extend((0..20).map(|index| action(&format!("filler-{index}"))));
    let widget = create_action_list_widget(&guard, config(items));

    let has_class = |row: &RowState, class: &str| row.classes.iter().any(|name| name == class);
    let plain_row = widget.row_state(0).unwrap();
    let detail_row = widget.row_state(1).unwrap();
    let initial = widget.row_toolbar_geometry(1, false).unwrap();
    let focused = widget.row_toolbar_geometry(1, true).unwrap();
    let plain_focused = widget.row_toolbar_geometry(0, true).unwrap();

    assert_eq!(
        (has_class(&plain_row, "has-detail"), has_class(&plain_row, "has-toolbar"), has_class(&detail_row, "has-detail"), has_class(&detail_row, "has-toolbar")),
        (false, true, true, true),
    );
    assert_eq!(
        (initial.row_height, initial.display, initial.visibility, initial.margin_right),
        (48.0, "flex", "hidden", 6.0),
    );
    assert_eq!(
        (
            focused.row_height,
            focused.detail_top == initial.detail_top,
            focused.display,
            focused.visibility,
            focused.margin_right,
            // `detailRow.right - toolbar.right >= verticalScrollbar.width`
            focused.right_inset >= LIST_VERTICAL_SCROLLBAR_SIZE,
            // `detailRow.right - toolbar.right === plainRow.right - plainToolbar.right`
            focused.right_inset == plain_focused.right_inset,
        ),
        (48.0, true, "flex", "visible", 6.0, true, true),
    );
}

#[test]
fn detail_links_open_from_pointer_and_keyboard_without_selecting_their_item() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let links = Rc::new(RefCell::new(Vec::<String>::new()));
    let selections = Rc::new(RefCell::new(Vec::<String>::new()));
    let link_handler: Rc<dyn Fn(&URI)> = {
        let links = links.clone();
        Rc::new(move |uri| links.borrow_mut().push(uri.to_string(true).to_string()))
    };
    let widget = create_action_list_widget(
        &guard,
        Config {
            on_select: recording(&selections),
            list_options: IActionListOptions { show_filter: false, link_handler: Some(link_handler), ..IActionListOptions::default() },
            ..config(vec![
                action("first"),
                IActionListItem {
                    detail: Some("Description".to_owned()),
                    detail_link: Some(IActionListHeaderLink { label: "Learn more".to_owned(), uri: URI::parse("https://example.com/docs", false) }),
                    ..action("documented")
                },
            ])
        },
    );
    widget.focus();
    widget.focus_next();
    let link = widget.detail_link_element(1).unwrap();

    dispatch_key(&widget, key_event("Tab"), KeyTarget::List);
    let tab_focused_link = widget.is_control_focused(&link);
    dispatch_key(&widget, key_with_code("Enter", KeyCode::Enter), KeyTarget::DetailLink);
    widget.click_detail_link(1);
    let mut shift_tab = key_event("Tab");
    shift_tab.shift_key = true;
    dispatch_key(&widget, shift_tab, KeyTarget::DetailLink);

    assert_eq!(
        (tab_focused_link, widget.is_list_focused(), links.borrow().clone(), selections.borrow().clone()),
        (true, true, strings(&["https://example.com/docs", "https://example.com/docs"]), Vec::<String>::new()),
    );
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

thread_local! {
    /// `window.innerHeight` da janela dos casos de altura (o `Viewport` do serviço de layout).
    static WINDOW_INNER_HEIGHT: Cell<f64> = const { Cell::new(800.0) };
}

/// `withWindowInnerHeight(height, callback)`: o valor anterior volta ao fim, mesmo se o `callback` falhar.
fn with_window_inner_height<R>(height: f64, callback: impl FnOnce() -> R) -> R {
    struct Restore(f64);
    impl Drop for Restore {
        fn drop(&mut self) {
            WINDOW_INNER_HEIGHT.with(|inner_height| inner_height.set(self.0));
        }
    }
    let _restore = Restore(WINDOW_INNER_HEIGHT.with(|inner_height| inner_height.replace(height)));
    callback()
}

/// `instantiationService.stub(ILayoutService, { getContainer, mainContainer, activeContainer, ... })`.
struct StubLayoutService {
    container: Rc<ViewElement>,
    layout: Rc<Emitter<LayoutContainerEvent>>,
}

impl ILayoutService for StubLayoutService {
    fn main_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }

    fn active_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }

    fn get_container(&self) -> Rc<ViewElement> {
        self.container.clone()
    }

    fn on_did_layout_container(&self) -> Event<LayoutContainerEvent> {
        self.layout.event()
    }

    fn window_viewport(&self) -> Viewport {
        Viewport { inner_width: 1280.0, inner_height: WINDOW_INNER_HEIGHT.with(Cell::get), page_x_offset: 0.0, page_y_offset: 0.0 }
    }
}

/// `instantiationService.stub(IContextViewService, { layout, hideContextView, getContextViewElement })`.
struct StubContextViewService {
    element: Rc<ViewElement>,
}

impl IContextViewService for StubContextViewService {
    fn show_context_view(&self, _delegate: Rc<dyn IContextViewDelegate>, _container: Option<Rc<ViewElement>>, _shadow_root: bool) {}

    fn hide_context_view(&self, _data: Option<bool>) {}

    fn layout(&self) {}

    fn get_context_view_element(&self) -> Rc<ViewElement> {
        self.element.clone()
    }
}

/// O que o `createActionList` devolve: a lista e o `.action-widget` que a contém (o pai de `list.domNode`).
struct ListFixture {
    list: Rc<ActionList<Item>>,
    widget: Rc<ViewElement>,
}

/// `createActionList(disposables, items, { listOptions, anchor })`: o `showFilter: true` de quem chama vem em `list_options`.
fn create_action_list(guard: &LeakGuard, items: Vec<TestItem>, list_options: IActionListOptions, anchor: Option<IAnchor>) -> ListFixture {
    let body = ViewElement::new();
    let layout = Emitter::new();
    guard.add_rc(layout.clone());
    let layout_service: Rc<dyn ILayoutService> = Rc::new(StubLayoutService { container: body.clone(), layout });
    let context_view_service: Rc<dyn IContextViewService> = Rc::new(StubContextViewService { element: body.clone() });
    let delegate = Rc::new(TestDelegate { on_hide: noop(), on_select: Rc::new(|_| {}), on_filter: None });
    let list = guard.add_rc(ActionList::new(
        false,
        &rcs(items),
        delegate,
        Some(Rc::new(list_options)),
        AnchorTarget::Anchor(anchor.unwrap_or(IAnchor { x: 10.0, y: 150.0, width: Some(20.0), height: Some(20.0) })),
        context_view_service,
        layout_service,
    ));
    let widget = ViewElement::with_classes(&["action-widget"]);
    widget.set_parent(Some(body));
    if let Some(filter_container) = list.filter_container() {
        filter_container.set_parent(Some(widget.clone()));
    }
    list.dom_node().set_parent(Some(widget.clone()));
    ListFixture { list, widget }
}

/// `options.items` do `createActionList` para N ações `item-${i}`.
fn numbered_actions(count: usize) -> Vec<TestItem> {
    (0..count).map(|index| action(&format!("item-{index}"))).collect()
}

fn with_filter() -> IActionListOptions {
    IActionListOptions { show_filter: true, ..IActionListOptions::default() }
}

/// `parseFloat(element.style.height)`.
fn style_height(element: &Rc<ViewElement>) -> f64 {
    element.style_get_property("height").and_then(|value| value.trim_end_matches("px").parse().ok()).unwrap_or(f64::NAN)
}

#[test]
fn leaves_room_for_action_widget_chrome_when_clamping_dynamic_height() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_window_inner_height(300.0, || {
        let fixture = create_action_list(&guard, numbered_actions(50), with_filter(), None);

        fixture.list.layout(200.0, None);

        let filter_height = 36.0;
        let available_space_above_anchor = 150.0;
        let list_height = style_height(&fixture.list.dom_node());
        assert!(list_height + filter_height + ACTION_WIDGET_VERTICAL_CHROME_HEIGHT <= available_space_above_anchor);
    });
}

#[test]
fn forced_above_anchor_position_can_clamp_dynamic_height_without_the_default_minimum_floor() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_window_inner_height(300.0, || {
        let fixture = create_action_list(
            &guard,
            numbered_actions(50),
            IActionListOptions { anchor_position: Some(AnchorPosition::Above), ..with_filter() },
            Some(IAnchor { x: 10.0, y: 20.0, width: Some(20.0), height: Some(20.0) }),
        );

        fixture.list.layout(200.0, None);

        assert_eq!((fixture.list.anchor_position(), style_height(&fixture.list.dom_node())), (Some(AnchorPosition::Above), 0.0));
    });
}

/// O corpo de `preferred anchor ${preferred} at ${top} resolves to ${expected} and stays stable after filtering`.
fn preferred_anchor(preferred: AnchorPosition, top: f64, expected: AnchorPosition) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_window_inner_height(600.0, || {
        let fixture = create_action_list(
            &guard,
            vec![action("first"), action("second"), action("third")],
            IActionListOptions { preferred_anchor_position: Some(preferred), ..with_filter() },
            Some(IAnchor { x: 10.0, y: top, width: Some(20.0), height: Some(20.0) }),
        );
        fixture.list.layout(200.0, None);
        let initial = (fixture.list.anchor_position(), fixture.list.dom_node().size().height);
        fixture.list.widget().filter_input_input("first");
        assert_eq!(
            (initial, fixture.list.anchor_position(), fixture.list.dom_node().size().height),
            ((Some(expected), 72.0), Some(expected), 24.0),
        );
    });
}

#[test]
fn preferred_anchor_above_at_250_resolves_to_above_and_stays_stable_after_filtering() {
    preferred_anchor(AnchorPosition::Above, 250.0, AnchorPosition::Above);
}

#[test]
fn preferred_anchor_below_at_250_resolves_to_below_and_stays_stable_after_filtering() {
    preferred_anchor(AnchorPosition::Below, 250.0, AnchorPosition::Below);
}

#[test]
fn preferred_anchor_above_at_118_resolves_to_above_and_stays_stable_after_filtering() {
    preferred_anchor(AnchorPosition::Above, 118.0, AnchorPosition::Above);
}

#[test]
fn preferred_anchor_below_at_432_resolves_to_below_and_stays_stable_after_filtering() {
    preferred_anchor(AnchorPosition::Below, 432.0, AnchorPosition::Below);
}

#[test]
fn preferred_anchor_above_at_110_resolves_to_below_and_stays_stable_after_filtering() {
    preferred_anchor(AnchorPosition::Above, 110.0, AnchorPosition::Below);
}

#[test]
fn preferred_anchor_below_at_460_resolves_to_above_and_stays_stable_after_filtering() {
    preferred_anchor(AnchorPosition::Below, 460.0, AnchorPosition::Above);
}

#[test]
fn preferred_placement_sizes_against_the_capped_height_and_fixed_placement_takes_precedence() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_window_inner_height(600.0, || {
        let positions: Vec<Option<AnchorPosition>> = [None, Some(AnchorPosition::Above)]
            .into_iter()
            .map(|anchor_position| {
                let fixture = create_action_list(
                    &guard,
                    numbered_actions(50),
                    IActionListOptions { preferred_anchor_position: Some(AnchorPosition::Below), anchor_position, ..with_filter() },
                    Some(IAnchor { x: 10.0, y: 150.0, width: Some(20.0), height: Some(20.0) }),
                );
                fixture.list.layout(200.0, None);
                fixture.list.anchor_position()
            })
            .collect();
        assert_eq!(positions, vec![Some(AnchorPosition::Below), Some(AnchorPosition::Above)]);
    });
}

#[test]
fn preferred_placement_uses_the_larger_available_side_when_neither_fits_without_a_minimum_height_floor() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_window_inner_height(180.0, || {
        let fixture = create_action_list(
            &guard,
            numbered_actions(50),
            IActionListOptions { preferred_anchor_position: Some(AnchorPosition::Below), ..with_filter() },
            Some(IAnchor { x: 10.0, y: 75.0, width: Some(20.0), height: Some(20.0) }),
        );
        fixture.list.layout(200.0, None);
        // `parentElement.getBoundingClientRect().height`: o filtro, a lista e o enchimento mais a borda do `.action-widget`.
        fixture.widget.set_size(0.0, fixture.list.chrome_height() + fixture.list.dom_node().size().height + ACTION_WIDGET_VERTICAL_CHROME_HEIGHT);
        assert_eq!(
            (fixture.list.anchor_position(), fixture.widget.rect().height <= 75.0),
            (Some(AnchorPosition::Above), true),
        );
    });
}

#[test]
fn full_height_menus_show_all_content_instead_of_scrolling_within_the_viewport_fraction_cap() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_window_inner_height(560.0, || {
        let states: Vec<(bool, f64, f64, f64)> = [false, true]
            .into_iter()
            .map(|use_full_height| {
                let items = (0..17)
                    .map(|index| IActionListItem::action(&format!("item-{index}"), Item { id: format!("item-{index}"), checked: index == 16 }))
                    .collect();
                let fixture = create_action_list(
                    &guard,
                    items,
                    IActionListOptions { show_filter: false, anchor_position: Some(AnchorPosition::Above), use_full_height, ..IActionListOptions::default() },
                    Some(IAnchor { x: 10.0, y: 432.0, width: Some(100.0), height: Some(24.0) }),
                );
                fixture.list.layout(260.0, None);
                fixture.list.focus();
                let widget = fixture.list.widget();
                (use_full_height, fixture.list.dom_node().size().height, widget.rows_height(), widget.rows_top())
            })
            .collect();

        assert_eq!(states, vec![(false, 336.0, 408.0, -72.0), (true, 408.0, 408.0, 0.0)]);
    });
}

#[test]
fn max_visible_items_caps_the_height_at_the_rows_through_that_many_actions_recomputed_after_filtering() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_window_inner_height(600.0, || {
        let mut items = vec![action("first"), separator(None), IActionListItem { detail: Some("Second line".to_owned()), ..action("detailed") }, separator(Some("Group"))];
        items.extend(numbered_actions(20));
        let fixture = create_action_list(
            &guard,
            items,
            IActionListOptions { anchor_position: Some(AnchorPosition::Below), max_visible_items: Some(3), ..with_filter() },
            Some(IAnchor { x: 10.0, y: 20.0, width: Some(20.0), height: Some(20.0) }),
        );
        fixture.list.layout(200.0, None);
        let initial = fixture.list.dom_node().size().height;
        fixture.list.widget().filter_input_input("item-1");

        // 24px action + 8px separator + 48px detail action + 24px labeled separator + 24px action,
        // then the labeled separator kept as a section header above three 24px matches.
        assert_eq!((initial, fixture.list.dom_node().size().height), (128.0, 96.0));
    });
}

#[test]
fn max_visible_items_placement_accounts_for_rows_hidden_by_the_initial_filter() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_window_inner_height(600.0, || {
        let fixture = create_action_list(
            &guard,
            vec![action("first"), action("second"), action("third")],
            IActionListOptions {
                initial_filter_value: Some("first".to_owned()),
                max_visible_items: Some(3),
                preferred_anchor_position: Some(AnchorPosition::Below),
                ..with_filter()
            },
            Some(IAnchor { x: 10.0, y: 460.0, width: Some(20.0), height: Some(20.0) }),
        );
        fixture.list.layout(200.0, None);

        assert_eq!((fixture.list.anchor_position(), fixture.list.dom_node().size().height), (Some(AnchorPosition::Above), 24.0));
    });
}

#[test]
fn header_dismiss_removes_the_banner_and_requests_a_re_layout() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let dismissed = Rc::new(Cell::new(false));
    let layout_requested = Rc::new(Cell::new(false));
    let on_dismiss = dismissed.clone();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: IActionListOptions {
                show_filter: true,
                header_text: Some("Cache hint".to_owned()),
                header_dismiss: Some(Rc::new(move || on_dismiss.set(true))),
                ..IActionListOptions::default()
            },
            ..config(Vec::new())
        },
    );
    let on_layout = layout_requested.clone();
    guard.add(widget.on_did_request_layout().listen(move |_| on_layout.set(true)));

    let header = widget.header_container().expect("header banner should render when headerText + headerDismiss are set");
    assert!(widget.header_dismiss_button().is_some(), "dismiss button should render");

    widget.header_dismiss_mouse_up();

    assert_eq!(
        (dismissed.get(), layout_requested.get(), widget.header_container().is_none(), header.parent_element().is_some()),
        (true, true, true, false),
    );
}

/// `rows()`: as linhas `.monaco-list-row.action` do widget, fora do painel.
fn action_rows(widget: &ActionListWidget<Item>) -> Vec<RowState> {
    (0..widget.visible_labels().len())
        .filter_map(|index| widget.row_state(index))
        .filter(|row| row.classes.iter().any(|class| class == "action"))
        .collect()
}

fn hover_panel(content: &str, expandable: bool) -> Option<IActionListItemHover> {
    Some(IActionListItemHover { content: Some(HoverContent::Text(content.to_owned())), expandable, ..IActionListItemHover::default() })
}

#[test]
fn an_expandable_row_names_the_panel_it_opens_and_stops_when_it_closes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: IActionListOptions {
                show_filter: true,
                reserve_submenu_space: Some(ReserveSubmenuSpace::Always),
                ..IActionListOptions::default()
            },
            ..config(vec![IActionListItem { hover: hover_panel("panel", true), ..action("auto") }, action("plain")])
        },
    );
    let state = || -> Vec<(Option<String>, Option<String>)> { action_rows(&widget).into_iter().map(|row| (row.aria_haspopup, row.aria_expanded)).collect() };
    let popup = |expanded: &str| vec![(Some("dialog".to_owned()), Some(expanded.to_owned())), (None, None)];

    let initial = state();
    // The chevron is what opens the panel; ArrowRight does the same from the keyboard.
    widget.click_submenu_indicator(0);
    let opened = state();
    let panel_role = widget.panel_role();
    let panel_label = widget.panel_aria_label();
    // Escape inside the panel is the way back to the row.
    widget.dispatch_key_down(&key_event("Escape"), KeyTarget::SubmenuPanel);
    let closed = state();

    // The plain row opens nothing, so it says nothing.
    assert_eq!(
        (initial, opened, closed, panel_role, panel_label),
        (popup("false"), popup("true"), popup("false"), Some("dialog".to_owned()), Some("auto".to_owned())),
    );
}

#[test]
fn a_submenu_beneath_hover_content_is_still_advertised_by_its_chevron_and_popup_state() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let child = to_action_with("child", "Child", None, None, noop());
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![
                IActionListItem { hover: hover_panel("About this item", false), submenu_actions: Some(vec![child]), ..action("menu") },
                IActionListItem { hover: hover_panel("About this item", false), ..action("hover") },
            ])
        },
    );
    let state = || -> Vec<(bool, Option<String>, Option<String>)> {
        action_rows(&widget).into_iter().map(|row| (row.submenu_indicator, row.aria_haspopup, row.aria_expanded)).collect()
    };

    let initial = state();
    widget.click_submenu_indicator(0);

    // A row with only hover content has no menu to advertise.
    assert_eq!(
        (initial, state()[0].2.clone(), widget.panel_hover_text(), widget.submenu_rows()),
        (
            vec![(true, Some("menu".to_owned()), Some("false".to_owned())), (false, None, None)],
            Some("true".to_owned()),
            Some("About this item".to_owned()),
            Some(vec![Some("Child".to_owned())]),
        ),
    );
}

#[test]
fn the_submenu_gutter_follows_the_items_the_list_currently_holds() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expandable = |id: &str| IActionListItem { hover: hover_panel("panel", true), ..action(id) };
    let gutters = |widget: &ActionListWidget<Item>| -> Vec<SubmenuGutter> { action_rows(widget).into_iter().map(|row| row.submenu_gutter).collect() };

    let widget = create_action_list_widget(&guard, config(vec![expandable("one"), action("two")]));
    let always = create_action_list_widget(
        &guard,
        Config {
            list_options: IActionListOptions {
                show_filter: true,
                reserve_submenu_space: Some(ReserveSubmenuSpace::Always),
                ..IActionListOptions::default()
            },
            ..config(vec![expandable("one"), action("two")])
        },
    );

    let before = (gutters(&widget), gutters(&always));
    // The chevrons go away, so by default the gutter goes with them.
    widget.update_items(&[rc(action("one")), rc(action("two"))], None, None);
    always.update_items(&[rc(action("one")), rc(action("two"))], None, None);

    assert_eq!(
        (before, (gutters(&widget), gutters(&always))),
        (
            (vec![SubmenuGutter::Shown, SubmenuGutter::Hidden], vec![SubmenuGutter::Shown, SubmenuGutter::Hidden]),
            (vec![SubmenuGutter::None, SubmenuGutter::None], vec![SubmenuGutter::Hidden, SubmenuGutter::Hidden]),
        ),
    );
}

#[test]
fn hidden_hover_chevrons_preserve_keyboard_access_without_reserving_a_gutter() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: IActionListOptions { show_filter: false, reserve_submenu_space: Some(ReserveSubmenuSpace::Never), ..IActionListOptions::default() },
            ..config(vec![IActionListItem {
                hover: Some(IActionListItemHover {
                    content: Some(HoverContent::Text("Model details".to_owned())),
                    expandable: true,
                    show_indicator: Some(false),
                    ..IActionListItemHover::default()
                }),
                ..action("model")
            }])
        },
    );

    let row = || first_action_row(&widget);
    let initial = (row().submenu_gutter, row().aria_haspopup, row().aria_expanded);
    widget.focus();
    dispatch_key(&widget, key_event("ArrowRight"), KeyTarget::DomNode);
    let opened = (widget.panel_role(), widget.is_panel_focused(), row().aria_expanded);
    dispatch_key(&widget, key_event("ArrowLeft"), KeyTarget::SubmenuPanel);

    assert_eq!(
        (initial, opened, (row().aria_expanded, widget.is_list_focused() && !widget.panel_contains_focus())),
        (
            (SubmenuGutter::None, Some("dialog".to_owned()), Some("false".to_owned())),
            (Some("dialog".to_owned()), true, Some("true".to_owned())),
            (Some("false".to_owned()), true),
        ),
    );
}

/// `parseFloat(style.top)` e companhia: o número de um valor `Npx`.
fn px(value: Option<String>) -> f64 {
    value.expect("a propriedade de estilo foi definida").trim_end_matches("px").parse().expect("o valor é um número em px")
}

/// `document.activeElement` está dentro de `widget.domNode`: na lista, no filtro, no painel ou no que o painel abriu.
fn focus_inside(widget: &ActionListWidget<Item>) -> bool {
    widget.is_list_focused() || widget.is_panel_focused() || widget.panel_contains_focus() || widget.filter_input().is_some_and(|input| input.is_focused())
}

/// `widget.focus()` e o `ArrowRight` em `widget.domNode`, que abre o submenu da linha focada.
fn open_submenu(widget: &ActionListWidget<Item>) -> Rc<ActionListWidget<ActionRef>> {
    widget.focus();
    dispatch_key(widget, key_event("ArrowRight"), KeyTarget::DomNode);
    widget.submenu_widget().expect("ArrowRight abre o submenu da linha focada")
}

/// `hover: { preserveVerticalPosition: true, alignToAnchorTop: true }`.
fn anchored_hover() -> Option<IActionListItemHover> {
    Some(IActionListItemHover { preserve_vertical_position: true, align_to_anchor_top: true, ..IActionListItemHover::default() })
}

/// `Object.assign(toAction({ id, label, run }), { onRemove: async () => { await timeout(20); } })`.
fn removable_after_20ms(id: &str, label: &str, run: Rc<dyn Fn()>) -> ActionRef {
    let on_remove: OnRemove = Rc::new(|| {
        async_fn(async move {
            timeout(20).await?;
            Ok::<(), Error>(())
        })
    });
    RemovableAction::new(to_action_with(id, label, None, None, run), on_remove)
}

/// As opções de submenu de `{ showFilter: true, filterAsCombobox: true, focusFilterOnOpen: true, ... }`.
fn combobox_submenu_options(stop_toolbar_pointer_propagation: bool) -> Option<Rc<IActionListOptions>> {
    Some(Rc::new(IActionListOptions {
        show_filter: true,
        filter_as_combobox: true,
        focus_filter_on_open: true,
        stop_toolbar_pointer_propagation,
        ..IActionListOptions::default()
    }))
}

#[test]
fn nested_submenu_options_enable_filtering() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let action_widget_commands: Vec<(KeyCode, &'static str)> = vec![
        (KeyCode::Space, "toggleSectionCodeAction"),
        (KeyCode::Enter, ACCEPT_SELECTED_ACTION_COMMAND),
        (KeyCode::Period, ACCEPT_SELECTED_ACTION_COMMAND),
    ];
    let keybinding: KeybindingResolver =
        Rc::new(move |event| action_widget_commands.iter().find(|(key_code, _)| *key_code == event.key_code).map(|(_, command_id)| (*command_id).to_owned()));
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            keybinding: Some(keybinding),
            ..config(vec![IActionListItem {
                hover: anchored_hover(),
                submenu_actions: Some(vec![
                    to_action_with("alpha", "A long remote host label", Some("Remote host description"), None, noop()),
                    to_action_with("beta", "Beta", None, None, noop()),
                ]),
                submenu_options: Some(Rc::new(IActionListOptions {
                    show_filter: true,
                    filter_placeholder: Some("Search Remote".to_owned()),
                    filter_as_combobox: true,
                    focus_filter_on_open: true,
                    min_width: Some(180.0),
                    max_width: Some(180.0),
                    ..IActionListOptions::default()
                })),
                ..action("remote")
            }])
        },
    );
    let hidden = Rc::new(Cell::new(0));
    let on_hide = hidden.clone();
    let dismissible_widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            on_hide: Rc::new(move || on_hide.set(on_hide.get() + 1)),
            ..config(vec![IActionListItem {
                submenu_actions: Some(vec![to_action_with("alpha", "Alpha", None, None, noop())]),
                submenu_options: combobox_submenu_options(false),
                ..action("remote")
            }])
        },
    );
    let submenu = open_submenu(&widget);
    let description_tooltip = (0..submenu.visible_labels().len())
        .filter_map(|index| submenu.row_state(index))
        .find(|row| row.title_text == "A long remote host label")
        .map(|row| row.title_attribute);
    // `filter.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, keyCode, ...init }))` e o ouvinte do `domNode` pai.
    let mut bubbled_keys = Vec::new();
    let key_events: [(&str, KeyCode, bool, bool); 7] = [
        ("r", KeyCode::Unknown, false, false),
        ("P", KeyCode::Unknown, true, false),
        ("F1", KeyCode::Unknown, false, true),
        (" ", KeyCode::Space, false, false),
        ("Enter", KeyCode::Enter, true, false),
        (".", KeyCode::Period, true, false),
        ("Process", KeyCode::Unknown, false, false),
    ];
    for (key, key_code, ctrl_key, alt_key) in key_events {
        let mut event = UiKeyEvent::new(key, key_code);
        event.ctrl_key = ctrl_key;
        event.alt_key = alt_key;
        submenu.dispatch_key_down(&event, KeyTarget::FilterInput);
        if !event.propagation_stopped() {
            bubbled_keys.push(event.key.clone());
        }
    }
    submenu.filter_input_input("bet");
    let filter_aria = submenu.filter_aria().expect("o filtro tem os atributos de acessibilidade");
    let filtered_state = (
        filter_aria.label,
        filter_aria.role,
        widget.panel_style_width(),
        (widget.panel_style_top(), Some(format!("{}px", widget.visible_row_rect(0).top - widget.dom_node().rect().top))),
        bubbled_keys,
        submenu.dom_node().style_get_property("width"),
        description_tooltip,
        widget.submenu_rows().expect("o submenu continua aberto"),
    );

    let dismissible_submenu = open_submenu(&dismissible_widget);
    let focused_on_open = dismissible_submenu.filter_input().is_some_and(|input| input.is_focused());
    // O `Escape` do campo sobe até o painel (nenhum tratador do campo o consome).
    let escape = key_event("Escape");
    dismissible_submenu.dispatch_key_down(&escape, KeyTarget::FilterInput);
    if !escape.propagation_stopped() {
        dismissible_widget.dispatch_key_down(&escape, KeyTarget::SubmenuPanel);
    }
    let hidden_from_filter = hidden.get();
    let reopened = open_submenu(&dismissible_widget);
    let escape = key_event("Escape");
    reopened.dispatch_key_down(&escape, KeyTarget::List);
    if !escape.propagation_stopped() {
        dismissible_widget.dispatch_key_down(&escape, KeyTarget::SubmenuPanel);
    }
    assert_eq!(
        (filtered_state, (focused_on_open, hidden_from_filter, hidden.get())),
        (
            (
                "Search Remote".to_owned(),
                Some("combobox".to_owned()),
                Some("190px".to_owned()),
                (Some("0px".to_owned()), Some("0px".to_owned())),
                strings(&["P", "F1", "Process"]),
                Some("180px".to_owned()),
                Some("Remote host description".to_owned()),
                vec![Some("Beta".to_owned())],
            ),
            (true, 1, 2),
        ),
    );
}

#[test]
fn submenu_rows_do_not_duplicate_labels_as_descriptions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![IActionListItem {
                submenu_actions: Some(vec![to_action_with("local", "Local", None, None, noop()), to_action_with("remote", "Test Remote B", None, None, noop())]),
                submenu_options: combobox_submenu_options(true),
                ..action("chat")
            }])
        },
    );

    let submenu = open_submenu(&widget);
    let rows: Vec<(String, Option<String>)> = (0..submenu.visible_labels().len())
        .filter_map(|index| submenu.row_state(index))
        .map(|row| {
            let description = row.description_text.trim().to_owned();
            (row.title_text, (!description.is_empty()).then_some(description))
        })
        .collect();

    assert_eq!(rows, vec![("Local".to_owned(), None), ("Test Remote B".to_owned(), None)]);
}

#[test]
fn mousedown_on_submenu_remove_toolbar_keeps_focus_in_the_picker() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![IActionListItem {
                submenu_actions: Some(vec![removable_after_20ms("remove", "Test Remote A", noop())]),
                submenu_options: combobox_submenu_options(true),
                ..action("remote")
            }])
        },
    );
    let submenu = open_submenu(&widget);

    let default_prevented = submenu.toolbar_pointer_down(0);

    assert_eq!((default_prevented, focus_inside(&widget)), (true, true));
}

#[test]
fn clicking_submenu_remove_toolbar_does_not_select_the_row() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let selected = Rc::new(RefCell::new(Vec::<String>::new()));
        let run_selected = selected.clone();
        let widget = create_action_list_widget(
            &guard,
            Config {
                list_options: no_filter(),
                on_select: recording(&selected),
                ..config(vec![IActionListItem {
                    submenu_actions: Some(vec![removable_after_20ms(
                        "remove",
                        "Test Remote A",
                        Rc::new(move || run_selected.borrow_mut().push("submenu-select".to_owned())),
                    )]),
                    submenu_options: combobox_submenu_options(true),
                    ..action("remote")
                }])
            },
        );
        let submenu = open_submenu(&widget);
        submenu.toolbar_pointer_down(0);
        let _ = submenu.click_toolbar_action(0, REMOVE_TOOLBAR_ACTION_ID);
        timeout(50).await.unwrap();

        assert_eq!((selected.borrow().clone(), widget.panel_displayed()), (Vec::<String>::new(), true));
    });
}

#[test]
fn clicking_remove_in_a_wrapped_submenu_group_keeps_the_submenu_open() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let selected = Rc::new(RefCell::new(Vec::<String>::new()));
        let run_selected = selected.clone();
        let removable = removable_after_20ms("remove", "Manage Provider remote-a", Rc::new(move || run_selected.borrow_mut().push("submenu-select".to_owned())));
        let widget = create_action_list_widget(
            &guard,
            Config {
                list_options: no_filter(),
                on_select: recording(&selected),
                ..config(vec![IActionListItem {
                    submenu_actions: Some(vec![SubmenuAction::new("workspacePicker.remote.options", "", vec![removable], None) as ActionRef]),
                    submenu_options: combobox_submenu_options(false),
                    ..action("remote")
                }])
            },
        );
        let submenu = open_submenu(&widget);
        submenu.toolbar_pointer_down(0);
        let _ = submenu.click_toolbar_action(0, REMOVE_TOOLBAR_ACTION_ID);
        timeout(50).await.unwrap();

        assert_eq!(
            (selected.borrow().clone(), widget.panel_displayed(), widget.panel_contains_focus()),
            (Vec::<String>::new(), true, true),
        );
    });
}

#[test]
fn filtering_replaces_a_submenu_trigger_with_matching_child_items_and_preserves_parent_matches() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        config(vec![
            action("local"),
            IActionListItem {
                filter_items: Some(vec![rc(action("Alpha")), rc(action("Beta"))]),
                submenu_actions: Some(vec![to_action_with("alpha", "Remote Alpha", None, None, noop())]),
                ..action("remote")
            },
        ]),
    );

    widget.filter_input_input("alpha");
    let child_match = visible_row_text(&widget);
    widget.filter_input_input("remote");
    let parent_match = visible_row_text(&widget);

    assert_eq!((child_match, parent_match), (strings(&["Alpha"]), strings(&["Alpha", "Beta"])));
}

#[test]
fn submenu_hover_labels_preserve_literal_markdown_characters() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let label = "host_*_[name](target)";
        let child_action: ActionRef = RemovableAction::with_hover_content(to_action_with("remote", label, None, None, noop()), "**Online**");
        let widget = create_action_list_widget(
            &guard,
            Config { list_options: no_filter(), ..config(vec![IActionListItem { submenu_actions: Some(vec![child_action]), ..action("remote") }]) },
        );
        let submenu = open_submenu(&widget);
        submenu.pointer_over(0, &PointerEvent::default());
        submenu.pointer_move(0, &moved(1.0, 0.0));
        timeout(600).await.unwrap();

        assert_eq!((submenu.panel_hover_text(), submenu.panel_hover_links()), (Some(format!("{label}Online")), 0));
    });
}

#[test]
fn nested_submenu_renders_footer_and_removes_the_list_strip_when_filtering_has_no_matches() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![IActionListItem {
                hover: anchored_hover(),
                submenu_actions: Some(vec![to_action_with("alpha", "Alpha", None, None, noop())]),
                submenu_options: Some(Rc::new(IActionListOptions {
                    show_filter: true,
                    filter_as_combobox: true,
                    footer_text: Some("Remote footer".to_owned()),
                    ..IActionListOptions::default()
                })),
                ..action("remote")
            }])
        },
    );
    let submenu = open_submenu(&widget);
    submenu.filter_input_input("missing");

    assert_eq!(
        (submenu.footer_text(), submenu.dom_node().style_get_property("height")),
        (Some("Remote footer".to_owned()), Some("0px".to_owned())),
    );
}

#[test]
fn refreshing_parent_items_while_removing_a_nested_row_keeps_the_submenu_open() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let hidden = Rc::new(Cell::new(0));
        let on_hide = hidden.clone();
        let widget_ref: Rc<RefCell<Option<std::rc::Weak<ActionListWidget<Item>>>>> = Rc::new(RefCell::new(None));
        let remaining = to_action_with("beta", "Beta", None, None, noop());
        let on_remove: OnRemove = {
            let (widget_ref, remaining) = (widget_ref.clone(), remaining.clone());
            Rc::new(move || {
                if let Some(widget) = widget_ref.borrow().as_ref().and_then(std::rc::Weak::upgrade) {
                    widget.update_items(
                        &[rc(IActionListItem {
                            submenu_actions: Some(vec![remaining.clone()]),
                            submenu_options: combobox_submenu_options(false),
                            ..action("remote")
                        })],
                        None,
                        Some(IActionListUpdateOptions { preserve_hover: true, ..IActionListUpdateOptions::default() }),
                    );
                }
                Promise::resolved(())
            })
        };
        let removed: ActionRef = RemovableAction::new(to_action_with("alpha", "Alpha", None, None, noop()), on_remove);
        let widget = create_action_list_widget(
            &guard,
            Config {
                list_options: no_filter(),
                on_hide: Rc::new(move || on_hide.set(on_hide.get() + 1)),
                ..config(vec![IActionListItem {
                    submenu_actions: Some(vec![removed, remaining]),
                    submenu_options: combobox_submenu_options(false),
                    ..action("remote")
                }])
            },
        );
        *widget_ref.borrow_mut() = Some(Rc::downgrade(&widget.widget));
        let submenu = open_submenu(&widget);
        submenu.filter_input_input("a");
        let alpha_index = submenu.visible_labels().iter().position(|label| label.as_deref() == Some("Alpha")).expect("Alpha está nas linhas filtradas");
        let _ = submenu.click_toolbar_action(alpha_index, REMOVE_TOOLBAR_ACTION_ID);
        timeout(0).await.unwrap();

        assert_eq!(
            (hidden.get(), widget.panel_displayed(), submenu.filter_text(), widget.submenu_rows(), widget.panel_contains_focus()),
            (0, true, "a".to_owned(), Some(vec![Some("Beta".to_owned())]), true),
        );
    });
}

/// `withWindowInnerHeight(300, ...)` com a lista de dez linhas de `a filtered submenu near the viewport bottom shifts enough to
/// show all rows`: o `getBoundingClientRect` do `domNode` e da linha é o retângulo `40, 260, 180, 24`.
#[test]
fn a_filtered_submenu_near_the_viewport_bottom_shifts_enough_to_show_all_rows() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![IActionListItem {
                hover: anchored_hover(),
                submenu_actions: Some((0..10).map(|index| to_action_with(&format!("remote-{index}"), &format!("Remote {index}"), None, None, noop())).collect()),
                submenu_options: Some(Rc::new(IActionListOptions {
                    show_filter: true,
                    filter_placeholder: Some("Search Remote".to_owned()),
                    filter_as_combobox: true,
                    min_width: Some(180.0),
                    max_width: Some(180.0),
                    ..IActionListOptions::default()
                })),
                ..action("remote")
            }])
        },
    );
    widget.set_window_size(1280.0, 300.0);
    widget.dom_node().set_position(260.0, 40.0);
    widget.dom_node().set_size(180.0, 24.0);
    let submenu = open_submenu(&widget);
    // `Object.defineProperty(filter, 'offsetHeight', { value: 30 })`, e o `resize` da janela.
    submenu.filter_container().expect("o submenu tem filtro").set_size(190.0, 30.0);
    widget.window_resized();

    let top = px(widget.panel_style_top());
    let list_height = px(submenu.dom_node().style_get_property("height"));
    let viewport_height = widget.panel_viewport_height().expect("o painel tem viewport");
    assert_eq!(
        (list_height, 260.0 + top + 10.0 + 30.0 + list_height <= 300.0 - 8.0, viewport_height),
        (240.0, true, 270.0),
    );
}

/// O popup `.action-widget` em `top: 100px; left: 40px; width: 200px; padding: 8px` da lista de trinta linhas: a lista do
/// `domNode` fica em `108, 48`.
#[test]
fn a_long_filtered_submenu_scrolls_its_rows_beneath_the_fixed_filter() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: no_filter(),
            ..config(vec![IActionListItem {
                hover: anchored_hover(),
                submenu_actions: Some((0..30).map(|index| to_action_with(&format!("remote-{index}"), &format!("Remote {index}"), None, None, noop())).collect()),
                submenu_options: Some(Rc::new(IActionListOptions {
                    show_filter: true,
                    filter_placeholder: Some("Search Remote".to_owned()),
                    filter_as_combobox: true,
                    min_width: Some(180.0),
                    max_width: Some(180.0),
                    ..IActionListOptions::default()
                })),
                ..action("remote")
            }])
        },
    );
    widget.set_window_size(1280.0, 300.0);
    let popup = ViewElement::with_classes(&["action-widget"]);
    popup.set_position(100.0, 40.0);
    popup.set_size(200.0, 400.0);
    widget.dom_node().set_parent(Some(popup.clone()));
    widget.dom_node().set_position(108.0, 48.0);
    widget.layout(24.0, Some(180.0));
    let submenu = open_submenu(&widget);

    let list = submenu.dom_node();
    let filter = submenu.filter_container().expect("o submenu tem filtro");
    assert_eq!(
        (!Rc::ptr_eq(&list, &filter), px(list.style_get_property("height")) < 30.0 * 24.0, submenu.rows_height() > list.size().height),
        (true, true, true),
    );
}

#[test]
fn scrolling_a_submenu_relayouts_an_open_child_hover() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let submenu_actions: Vec<ActionRef> = (0..30)
            .map(|index| RemovableAction::with_hover_content(to_action_with(&format!("remote-{index}"), &format!("Remote {index}"), None, None, noop()), &format!("Status {index}")) as ActionRef)
            .collect();
        let widget = create_action_list_widget(
            &guard,
            Config { list_options: no_filter(), ..config(vec![IActionListItem { submenu_actions: Some(submenu_actions), ..action("remote") }]) },
        );
        widget.layout(24.0, Some(180.0));
        let submenu = open_submenu(&widget);
        submenu.pointer_over(0, &PointerEvent::default());
        submenu.pointer_move(0, &moved(1.0, 0.0));
        timeout(600).await.unwrap();
        let layouts_before = submenu.submenu_layout_count();
        submenu.set_scroll_top(24.0);

        assert_eq!((submenu.panel_displayed(), submenu.submenu_layout_count() - layouts_before), (true, 1));
    });
}

#[test]
fn mouse_removal_from_a_row_toolbar_does_not_select_the_row() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let selected = Rc::new(RefCell::new(Vec::<String>::new()));
        let removed = Rc::new(Cell::new(0));
        let on_run = removed.clone();
        let widget = create_action_list_widget(
            &guard,
            Config {
                list_options: no_filter(),
                on_select: recording(&selected),
                ..config(vec![IActionListItem {
                    toolbar_actions: Some(vec![to_action_with("remove-reference", "Remove Reference from Session", None, None, Rc::new(move || on_run.set(on_run.get() + 1)))]),
                    ..action("reference")
                }])
            },
        );

        let _ = widget.click_toolbar_action(0, "remove-reference");
        timeout(0).await.unwrap();

        assert_eq!((removed.get(), selected.borrow().clone()), (1, Vec::<String>::new()));
    });
}

#[test]
fn rebuilding_the_items_in_place_re_measures_only_when_the_row_count_changed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(&guard, config(vec![action("one"), action("two")]));
    let layouts = Rc::new(RefCell::new(Vec::<String>::new()));
    let (recorded, observed) = (layouts.clone(), widget.widget.clone());
    guard.add(widget.on_did_request_layout().listen(move |_| recorded.borrow_mut().push(visible_row_text(&observed).join(","))));

    widget.update_items(&rcs(vec![action("one"), action("two-renamed")]), None, None);
    let after_same_count = layouts.borrow().len();
    widget.update_items(&rcs(vec![action("one"), action("two-renamed"), action("three")]), None, None);

    assert_eq!(
        (after_same_count, layouts.borrow().clone(), visible_row_text(&widget)),
        (0, strings(&["one,two-renamed,three"]), strings(&["one", "two-renamed", "three"])),
    );
}

#[test]
fn rebuilding_the_items_in_place_leaves_focus_alone_when_the_list_does_not_have_it() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let widget = create_action_list_widget(&guard, config(vec![action("one"), action("two")]));

    widget.update_items(&rcs(vec![action("one"), action("two"), action("three")]), None, None);

    assert_eq!((focus_inside(&widget), visible_row_text(&widget)), (false, strings(&["one", "two", "three"])));
}

#[test]
fn rebuilding_the_items_in_place_can_preserve_the_scroll_position() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let items: Vec<TestItem> = (0..20).map(|index| action(&format!("item-{index}"))).collect();
    let checked_first = |items: &[TestItem]| -> Vec<TestItem> {
        items.iter().cloned().enumerate().map(|(index, item)| if index == 0 { IActionListItem { item: Some(Item { id: "item-0".to_owned(), checked: true }), ..item } } else { item }).collect()
    };
    let widget = create_action_list_widget(
        &guard,
        Config {
            list_options: IActionListOptions { show_filter: true, focus_filter_on_open: true, ..IActionListOptions::default() },
            ..config(checked_first(&items))
        },
    );
    widget.layout(120.0, Some(200.0));
    widget.focus();
    widget.set_scroll_top(120.0);

    widget.update_items(&rcs(items), None, Some(IActionListUpdateOptions { preserve_scroll_position: true, ..IActionListUpdateOptions::default() }));

    // `list.scrollTop` é o deslocamento negativo de `.monaco-list-rows`.
    assert_eq!(widget.rows_top(), -120.0);
}

