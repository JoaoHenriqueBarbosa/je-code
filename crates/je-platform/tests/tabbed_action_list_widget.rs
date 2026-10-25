//! Porte de `src/vs/platform/actionWidget/test/browser/tabbedActionListWidget.test.ts`, caso a caso e na ordem do original.
//!
//! Notas de tradução:
//! - **DOM.** O popup é a árvore de [`ViewElement`] do `TabbedActionListWidget`; `querySelector('.monaco-list-row...')`,
//!   `.tabbed-action-list-tabstrip .monaco-button` e companhia são o estado que ele expõe (`tabs()`, `tab_bar_actions()`,
//!   `elements()`, `body_state()`, `details_state()`, `list_visible_labels()`). O `FakeContextViewService` do teste é o mesmo:
//!   guarda o último delegate e chama o `render` na hora.
//! - **Eventos.** `dispatchEvent(new KeyboardEvent('keydown', ...))` é `dispatch_key_down` com o [`TabbedKeyTarget`] do alvo,
//!   `tab.click()` precedido de `pointerdown` é `click_tab`, `pointerdown` na página é `pointer_down`, e o `defaultPrevented`
//!   é o do [`UiKeyEvent`]. `await settleLayout()` (dois quadros de animação) é `settle_layout`, o relógio virtual do `runtime`.
//! - **Foco.** `document.activeElement` fora da lista é o [`PopupFocus`]; um controle que o chamador cria e foca avisa com
//!   `set_focus_target`.
//! - **Geometria.** `offsetHeight` e `getBoundingClientRect` são a geometria que o widget calcula. O teste do CSS dos detalhes
//!   (`details use theme tokens...`) lê [`details_focus_outline`] no lugar da folha de estilo, e o que `getComputedStyle`
//!   media da borda e do `display` do botão Voltar é o [`DetailsBackButton`].
//! - **Fora desta fatia.** Nada: os casos que dependem de `tabThroughItemActions` do `ActionList` (a barra de ferramentas por
//!   linha com Tab), de `elementFromPoint` e dos `sinon.spy` do protótipo do `ActionList` entram aqui com o `ActionList`: a
//!   opção é `tab_through_item_actions`, o `sinon.spy(ActionList.prototype, 'updateItems')` é `last_update_options` e o
//!   `elementFromPoint` do clique imediato é a geometria da linha dentro do retângulo do popup.
//! - **Elementos do chamador.** O `HTMLElement` do hover, do rodapé e dos detalhes é um [`HoverElement`] ou o [`ViewElement`]
//!   que o widget entrega: `control.focus()` é `focus_control` (hover) ou `set_focus_target` (rodapé e detalhes), e o
//!   `ResizeObserver` do rodapé e dos detalhes é `notify_footer_resized` e `notify_details_resized`.
//! - **Animações.** `body.getAnimations()` é `body_animation()` (`animation.finish()` é `finish_body_animation()`, `pause()` e
//!   `currentTime` são `set_body_animation_time`), `tab.getAnimations()` é `TabElement::animations` e o `finish` da animação
//!   de movimento de linha é `finish_item_move_animations`.

#![allow(dead_code)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::action_list_support::{Item, StubLayoutService, TestItem, action, rcs};
use common::keybinding::{MockKeybindingService, os};
use common::{run_with_faked_timers, uri_file};
use je_base::actions::{ActionRef, ToActionProps, to_action};
use je_base::async_::timeout;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::key_codes::KeyCode;
use je_base::layout::AnchorPosition;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_instantiation::Injectable;
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService};
use je_platform::action_widget::action_list::{
    ActionList, ActionListItemKind, HoverContent, HoverElement, IActionListDelegate, IActionListItem, IActionListItemHover, IActionListItemInlineToggle,
    IActionListOptions, KeyTarget, PointerEvent, PointerTarget,
};
use je_platform::action_widget::action_widget_motion::{ACTION_WIDGET_ANIMATED_CLASS, ACTION_WIDGET_DROPDOWN_MOTION_CLASS};
use je_platform::action_widget::tabbed_action_list_widget::{
    ITabBarAction, ITabDescriptor, ITabbedActionListBuildResult, ITabbedActionListDetailsOptions, ITabbedActionListRefreshOptions,
    ITabbedActionListShowOptions, PopupFocus, TabIcon, TabLabels, TabbedActionListWidget, TabbedKeyTarget, details_focus_outline,
};
use je_platform::contextview::context_view::{AnchorTarget, HideData, IContextViewDelegate, IContextViewService};
use je_platform::contextview::context_view_service::ContextViewService;
use je_platform::contextview::view_element::{UiKeyEvent, ViewElement};
use je_platform::layout::layout_service::ILayoutService;

/// `sinon`-livre: o `IContextViewService` mínimo, que guarda o delegate e chama o `render` na hora.
struct FakeContextViewService {
    container: RefCell<Option<Rc<ViewElement>>>,
    delegate: RefCell<Option<Rc<dyn IContextViewDelegate>>>,
    render_disposables: RefCell<Option<DisposableRef>>,
    layout_count: Cell<usize>,
}

impl FakeContextViewService {
    fn new() -> Rc<Self> {
        Rc::new(Self { container: RefCell::new(None), delegate: RefCell::new(None), render_disposables: RefCell::new(None), layout_count: Cell::new(0) })
    }

    fn active_layer(&self) -> Option<i32> {
        self.delegate.borrow().as_ref().and_then(|delegate| delegate.layer())
    }

    fn layout_count(&self) -> usize {
        self.layout_count.get()
    }
}

impl IContextViewService for FakeContextViewService {
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, _container: Option<Rc<ViewElement>>, _shadow_root: bool) {
        // Tear down any previous render before showing a new one.
        self.hide_context_view(None);
        *self.delegate.borrow_mut() = Some(delegate.clone());
        let container = ViewElement::new();
        *self.container.borrow_mut() = Some(container.clone());
        let result = delegate.render(&container);
        *self.render_disposables.borrow_mut() = result;
    }

    fn hide_context_view(&self, _data: Option<HideData>) {
        let delegate = self.delegate.borrow_mut().take();
        let render_disposables = self.render_disposables.borrow_mut().take();
        *self.container.borrow_mut() = None;
        // Notify the delegate first so its `onHide` runs against the still-mounted DOM, mirroring the real `ContextView` order.
        if let Some(delegate) = delegate {
            delegate.on_hide(None);
        }
        if let Some(render_disposables) = render_disposables {
            render_disposables.dispose();
        }
    }

    fn layout(&self) {
        self.layout_count.set(self.layout_count.get() + 1);
    }

    fn get_context_view_element(&self) -> Rc<ViewElement> {
        self.container.borrow().clone().unwrap_or_else(ViewElement::new)
    }
}

/// `TestAccessibilityService` com o movimento reduzido ajustável.
struct StubAccessibilityService {
    motion_reduced: Rc<Cell<bool>>,
    reduced_motion: Rc<Emitter<()>>,
}

impl IAccessibilityService for StubAccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_motion(&self) -> Event<()> {
        self.reduced_motion.event()
    }
    fn on_did_change_reduced_transparency(&self) -> Event<()> {
        Event::none()
    }
    fn always_underline_access_keys(&self) -> Promise<bool> {
        Promise::resolved(false)
    }
    fn is_screen_reader_optimized(&self) -> bool {
        false
    }
    fn is_motion_reduced(&self) -> bool {
        self.motion_reduced.get()
    }
    fn is_transparency_reduced(&self) -> bool {
        false
    }
    fn get_accessibility_support(&self) -> AccessibilitySupport {
        AccessibilitySupport::Unknown
    }
    fn set_accessibility_support(&self, _accessibility_support: AccessibilitySupport) {}
    fn alert(&self, _message: &str) {}
    fn status(&self, _message: &str) {}
}

/// `createWidget(disposables, motionReduced)`.
struct Created {
    widget: Rc<TabbedActionListWidget>,
    context_view: Rc<FakeContextViewService>,
    motion_reduced: Rc<Cell<bool>>,
    reduced_motion: Rc<Emitter<()>>,
}

impl Created {
    /// `setMotionReduced(reduced)`.
    fn set_motion_reduced(&self, reduced: bool) {
        self.motion_reduced.set(reduced);
        self.reduced_motion.fire(());
    }
}

impl Drop for Created {
    fn drop(&mut self) {
        self.widget.dispose();
        self.reduced_motion.dispose();
    }
}

fn create_widget(motion_reduced: bool) -> Created {
    let context_view = FakeContextViewService::new();
    let motion = Rc::new(Cell::new(motion_reduced));
    let reduced_motion = Emitter::new();
    let body = ViewElement::new();
    // `document.body`, tall enough for a list that is not anchored to a fixed side.
    body.set_client_height(800.0);
    let layout_service: Rc<dyn ILayoutService> = Rc::new(StubLayoutService { container: body, layout: Emitter::new() });
    let widget = TabbedActionListWidget::new(
        context_view.clone(),
        layout_service,
        Rc::new(StubAccessibilityService { motion_reduced: motion.clone(), reduced_motion: reduced_motion.clone() }),
        Rc::new(MockKeybindingService),
    );
    Created { widget, context_view, motion_reduced: motion, reduced_motion }
}

/// O `anchor` que o teste cria: `position: fixed; top; width; height`.
fn anchor_at(top: f64, left: f64, width: f64, height: f64) -> AnchorTarget {
    let element = ViewElement::new();
    element.set_position(top, left);
    element.set_size(width, height);
    AnchorTarget::Element(element)
}

/// `document.body` como âncora.
fn body_anchor() -> AnchorTarget {
    AnchorTarget::Element(ViewElement::new())
}

/// `{ onSelect, onHide }`.
struct ShowDelegate {
    on_select: Rc<dyn Fn(&Item)>,
    on_hide: Rc<dyn Fn()>,
}

impl IActionListDelegate<Item> for ShowDelegate {
    fn on_hide(&self, _did_cancel: Option<bool>) {
        (self.on_hide)();
    }

    fn on_select(&self, action: &Item, _preview: bool) {
        (self.on_select)(action);
    }
}

fn delegate() -> Rc<dyn IActionListDelegate<Item>> {
    Rc::new(ShowDelegate { on_select: Rc::new(|_| {}), on_hide: Rc::new(|| {}) })
}

/// `delegate: { onSelect: () => assert.fail(message), onHide }`.
fn failing_delegate(message: &'static str) -> Rc<dyn IActionListDelegate<Item>> {
    Rc::new(ShowDelegate { on_select: Rc::new(move |_| panic!("{message}")), on_hide: Rc::new(|| {}) })
}

fn items_result(items: Vec<TestItem>) -> ITabbedActionListBuildResult<Item> {
    ITabbedActionListBuildResult { items: rcs(items), list_options: None }
}

fn tabs(ids: &[&str]) -> Vec<ITabDescriptor> {
    ids.iter().map(|id| ITabDescriptor::new(id)).collect()
}

/// As opções de um caso: as abas, a aba inicial e o `createActionList`.
fn options(
    anchor: AnchorTarget,
    tab_ids: &[&str],
    initial_tab: &str,
    create: impl Fn(&str, bool) -> ITabbedActionListBuildResult<Item> + 'static,
    delegate: Rc<dyn IActionListDelegate<Item>>,
) -> ITabbedActionListShowOptions<Item> {
    ITabbedActionListShowOptions::new("test", anchor, tabs(tab_ids), initial_tab, Rc::new(create), delegate)
}

fn key(key: &str, key_code: KeyCode) -> UiKeyEvent {
    UiKeyEvent::new(key, key_code)
}

fn key_with(key: &str, key_code: KeyCode, modifiers: impl FnOnce(&mut UiKeyEvent)) -> UiKeyEvent {
    let mut event = UiKeyEvent::new(key, key_code);
    modifiers(&mut event);
    event
}

/// `list.dispatchEvent(event); return event.defaultPrevented`.
fn press(widget: &TabbedActionListWidget, event: UiKeyEvent, target: TabbedKeyTarget) -> bool {
    widget.dispatch_key_down(&event, target);
    event.default_prevented()
}

/// `settleLayout()`: dois quadros de animação no relógio virtual.
async fn settle_layout() {
    timeout(40).await;
}

fn popup_classes(widget: &TabbedActionListWidget) -> Vec<String> {
    let mut classes: Vec<String> = widget.elements().unwrap().widget.class_list().into_iter().filter(|name| name != "action-widget").collect();
    classes.sort();
    classes
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn noop() -> Rc<dyn Fn()> {
    Rc::new(|| {})
}

/// `toAction({ id, label, run })`.
fn to_action_with(id: &str, label: &str, run: Rc<dyn Fn()>) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label: label.to_owned(),
        tooltip: None,
        enabled: None,
        checked: None,
        class: None,
        alt: None,
        run: Rc::new(move |_| {
            run();
            Dynamic::Undefined.into()
        }),
    })
}

/// `{ ...action(id), toolbarActions }`.
fn with_toolbar(item: TestItem, actions: Vec<ActionRef>) -> TestItem {
    IActionListItem { toolbar_actions: Some(actions), ..item }
}

/// `{ ...action(id), item: { id, checked }, hover }`.
fn checked_with_hover(id: &str, checked: bool, hover: IActionListItemHover) -> TestItem {
    IActionListItem { item: Some(Item { id: id.to_owned(), checked }), hover: Some(hover), ..action(id) }
}

/// O `ActionList` do popup aberto.
fn list_of(widget: &TabbedActionListWidget) -> Rc<ActionList<Item>> {
    widget.action_list::<Item>().expect("o popup está aberto")
}

/// `new MouseEvent('mousemove', { bubbles: true, movementY: 1 })` no `target` da linha.
fn moved_over(target: PointerTarget) -> PointerEvent {
    PointerEvent { movement_y: 1.0, target, ..PointerEvent::default() }
}

/// O botão da barra de uma linha que é o `document.activeElement`: o `aria-label`.
fn focused_toolbar_label(widget: &TabbedActionListWidget) -> Option<String> {
    widget.list_focused_toolbar_button().map(|(_, label)| label)
}

/// `element.closest('[inert]')`: o `main` do popup recebe a classe `inert` enquanto os detalhes estão abertos.
fn has_inert_ancestor(element: &Rc<ViewElement>) -> bool {
    let mut current = Some(element.clone());
    while let Some(candidate) = current {
        if candidate.class_list_contains("inert") {
            return true;
        }
        current = candidate.parent_element();
    }
    false
}

/// O alvo de um `keydown` num botão da barra de uma linha ou no `Switch` dela: controles da própria linha, dentro da lista.
fn row_control_target() -> TabbedKeyTarget {
    TabbedKeyTarget { on_own_controls: true, list: Some(KeyTarget::EmbeddedControl), ..TabbedKeyTarget::default() }
}

/// `Math.round(width * 100) / 100`.
fn round_width(width: f64) -> f64 {
    (width * 100.0).round() / 100.0
}

/// `createCollapsibleWidget(disposables, motionReduced, collapsed)`.
struct Collapsible {
    created: Created,
    collapsed: Rc<Cell<bool>>,
}

impl Collapsible {
    fn widget(&self) -> &Rc<TabbedActionListWidget> {
        &self.created.widget
    }

    fn popup(&self) -> Rc<ViewElement> {
        self.created.widget.elements().expect("o popup está aberto").widget
    }

    fn body(&self) -> Rc<ViewElement> {
        self.created.widget.elements().expect("o popup está aberto").body
    }

    /// `setCollapsed(next)`.
    fn set_collapsed(&self, next: bool) {
        self.collapsed.set(next);
        self.created.widget.refresh_active_list(None);
    }

    /// `document.activeElement === button`: o botão do rodapé.
    fn footer_focused(&self) -> bool {
        self.created.widget.focus_target() == Some(PopupFocus::Footer)
    }
}

fn create_collapsible_widget(motion_reduced: bool, collapsed: bool) -> Collapsible {
    let created = create_widget(motion_reduced);
    let collapsed_state = Rc::new(Cell::new(collapsed));
    // `hover.style.height = '100px'`.
    let hover = HoverElement::new("");
    hover.set_size(0.0, 100.0);
    let reading_for_items = collapsed_state.clone();
    let reading_for_body = collapsed_state.clone();
    let weak_widget = Rc::downgrade(&created.widget);
    let mut show = options(
        anchor_at(400.0, 0.0, 120.0, 20.0),
        &["Local", "Remote"],
        "Local",
        move |_, _| ITabbedActionListBuildResult {
            items: rcs(
                ["a", "b", "c", "d"]
                    .iter()
                    .map(|id| IActionListItem {
                        hover: if reading_for_items.get() {
                            None
                        } else {
                            Some(IActionListItemHover {
                                content: Some(HoverContent::Element(hover.clone())),
                                align_to_parent: true,
                                preserve_vertical_position: true,
                                ..IActionListItemHover::default()
                            })
                        },
                        ..action(id)
                    })
                    .collect(),
            ),
            list_options: Some(IActionListOptions { anchor_position: Some(AnchorPosition::Above), persistent_hover: true, ..IActionListOptions::default() }),
        },
        delegate(),
    );
    show.sizing_tab = Some("Local".to_owned());
    show.width = Some(300.0);
    show.is_body_collapsed = Some(Rc::new(move || reading_for_body.get()));
    show.render_footer = Some(Rc::new(|container: &Rc<ViewElement>, _: &str| {
        container.set_size(0.0, 20.0);
        to_disposable(|| {})
    }));
    show.focus_footer = Some(Rc::new(move || {
        if let Some(widget) = weak_widget.upgrade() {
            widget.set_focus_target(PopupFocus::Footer);
        }
    }));
    created.widget.show(show);
    // `contextView.getContextViewElement().style.cssText = 'position: fixed; bottom: 32px; left: 20px;'`.
    let elements = created.widget.elements().expect("o popup está aberto");
    elements.widget.set_position(290.0, 20.0);
    elements.list.set_position(300.0, 20.0);
    Collapsible { created, collapsed: collapsed_state }
}

/// A tecla Voltar da página de detalhes no sistema do teste.
fn back_key() -> UiKeyEvent {
    match os() {
        OperatingSystem::Windows => key_with("ArrowLeft", KeyCode::LeftArrow, |e| e.alt_key = true),
        OperatingSystem::Macintosh => key_with("-", KeyCode::Minus, |e| e.ctrl_key = true),
        OperatingSystem::Linux => key_with("-", KeyCode::Minus, |e| {
            e.ctrl_key = true;
            e.alt_key = true;
        }),
    }
}

/// A tecla de próxima aba no sistema do teste.
fn next_tab_key() -> UiKeyEvent {
    if os() == OperatingSystem::Macintosh {
        key_with("ArrowRight", KeyCode::RightArrow, |e| {
            e.meta_key = true;
            e.alt_key = true;
        })
    } else {
        key_with("PageDown", KeyCode::PageDown, |e| e.ctrl_key = true)
    }
}

/// `createSearchableWidget(disposables, itemIds)`.
struct Searchable {
    created: Created,
    selected: Rc<RefCell<Vec<String>>>,
    tab_changes: Rc<RefCell<Vec<String>>>,
    /// O widget que o `onSelect` esconde: solto no fim do caso para não formar ciclo.
    widget_slot: Rc<RefCell<Option<Rc<TabbedActionListWidget>>>>,
    _tab_subscription: DisposableRef,
}

impl Drop for Searchable {
    fn drop(&mut self) {
        *self.widget_slot.borrow_mut() = None;
    }
}

fn create_searchable_widget(item_ids: &[&str]) -> Searchable {
    let created = create_widget(true);
    let selected = Rc::new(RefCell::new(Vec::new()));
    let tab_changes = Rc::new(RefCell::new(Vec::new()));
    let changes = tab_changes.clone();
    let subscription = created.widget.on_did_change_tab().listen(move |tab| changes.borrow_mut().push(tab.clone()));
    let widget_slot: Rc<RefCell<Option<Rc<TabbedActionListWidget>>>> = Rc::new(RefCell::new(Some(created.widget.clone())));
    let widget_slot_for_case = widget_slot.clone();
    let recording = selected.clone();
    let slot = widget_slot.clone();
    let ids: Vec<String> = item_ids.iter().map(|id| (*id).to_owned()).collect();
    let mut show = options(
        body_anchor(),
        &["Local", "Remote"],
        "Local",
        move |_, _| ITabbedActionListBuildResult {
            items: rcs(ids.iter().map(|id| action(id)).collect()),
            list_options: Some(IActionListOptions {
                show_filter: true,
                focus_filter_on_open: true,
                filter_as_combobox: true,
                initial_filter_value: Some("match".to_owned()),
                ..IActionListOptions::default()
            }),
        },
        Rc::new(ShowDelegate {
            on_select: Rc::new(move |item| {
                recording.borrow_mut().push(item.id.clone());
                if let Some(widget) = slot.borrow().as_ref() {
                    widget.hide();
                }
            }),
            on_hide: Rc::new(|| {}),
        }),
    );
    show.filter_in_tab_bar = true;
    created.widget.show(show);
    Searchable { created, selected, tab_changes, widget_slot: widget_slot_for_case, _tab_subscription: subscription }
}

#[test]
fn construct_and_dispose_without_crashing() {
    let created = create_widget(true);
    assert!(!created.widget.is_visible());
}

#[test]
fn tab_bar_actions_use_managed_hovers() {
    let created = create_widget(true);
    let mut show = options(body_anchor(), &["Models"], "Models", |_, _| items_result(Vec::new()), delegate());
    show.tab_bar_actions = vec![
        ITabBarAction {
            id: "workflow".to_owned(),
            icon: "layers".to_owned(),
            tooltip: "Compare Models".to_owned(),
            align_end: false,
            use_managed_hover: true,
            checked: None,
            run: Rc::new(|| {}),
        },
        ITabBarAction {
            id: "search".to_owned(),
            icon: "search".to_owned(),
            tooltip: "Search Models".to_owned(),
            align_end: false,
            use_managed_hover: false,
            checked: None,
            run: Rc::new(|| {}),
        },
    ];
    created.widget.show(show);
    let elements = created.widget.tab_bar_actions();
    let managed_hovers: Vec<(String, String, Option<String>)> = elements
        .iter()
        .filter_map(|element| element.managed_hover.clone().map(|content| (element.id.clone(), content, element.title.clone())))
        .collect();
    let search_title = elements.iter().find(|element| element.id == "search").and_then(|element| element.title.clone());
    assert_eq!(
        (managed_hovers, search_title),
        (vec![("workflow".to_owned(), "Compare Models".to_owned(), None)], Some("Search Models".to_owned())),
    );
}

/// `widget.showDetails({ label, backLabel, render })` sem conteúdo.
fn empty_details(label: &str, back_label: &str) -> ITabbedActionListDetailsOptions {
    ITabbedActionListDetailsOptions {
        label: label.to_owned(),
        back_label: back_label.to_owned(),
        render_header: None,
        render: Rc::new(|_| to_disposable(|| {})),
        focus: None,
        restore_focus: None,
        on_back: None,
    }
}

#[test]
fn escape_from_a_details_radio_dismisses_the_entire_popup_including_retained_search() {
    run_with_faked_timers(async move {
        let search = create_searchable_widget(&["first match", "second match"]);
        let widget = &search.created.widget;
        widget.show_details(empty_details("Model details", "Back"));
        widget.set_focus_target(PopupFocus::DetailsContent);
        let input_connected = || widget.elements().is_some();
        let during = (widget.is_showing_details(), widget.body_state().unwrap().main_inert, input_connected());
        assert!(press(widget, key("Escape", KeyCode::Escape), TabbedKeyTarget::details()));
        settle_layout().await;
        let after = (widget.is_visible(), widget.is_showing_details(), input_connected());
        assert_eq!((during, after, search.selected.borrow().clone()), ((true, true, true), (false, false, false), Vec::<String>::new()));
    });
}

#[test]
fn details_track_keyboard_and_pointer_navigation_without_moving_page_focus() {
    let search = create_searchable_widget(&["model match"]);
    let widget = &search.created.widget;
    press(widget, key("Tab", KeyCode::Tab), TabbedKeyTarget::filter_input(true));
    let page_focus: Rc<dyn Fn(&Rc<ViewElement>)> = {
        let widget = widget.clone();
        Rc::new(move |_| widget.set_focus_target(PopupFocus::DetailsPage))
    };
    widget.show_details(ITabbedActionListDetailsOptions { focus: Some(page_focus), ..empty_details("Model details", "Back") });
    let popup = widget.elements().unwrap().widget;
    let keyboard_entry = popup.class_list_contains("keyboard-navigation");
    widget.pointer_down();
    let pointer_navigation = popup.class_list_contains("keyboard-navigation");
    press(widget, key("Tab", KeyCode::Tab), TabbedKeyTarget::details());
    assert_eq!(
        (keyboard_entry, pointer_navigation, popup.class_list_contains("keyboard-navigation"), widget.focus_target() == Some(PopupFocus::DetailsPage)),
        (true, false, true, true),
    );
}

#[test]
fn details_use_theme_tokens_for_an_inset_keyboard_only_outline() {
    assert_eq!(
        (details_focus_outline(false), details_focus_outline(true).map(|outline| (outline.outline, outline.offset))),
        (
            None,
            Some((
                "var(--vscode-strokeThickness) solid var(--vscode-focusBorder)".to_owned(),
                "calc(-1 * var(--vscode-strokeThickness))".to_owned(),
            )),
        ),
    );
}

#[test]
fn back_uses_the_shared_icon_action_without_text_button_borders_or_outset_focus_styling() {
    let search = create_searchable_widget(&["model match"]);
    let widget = &search.created.widget;
    widget.show_details(ITabbedActionListDetailsOptions {
        render_header: Some(Rc::new(|_| to_disposable(|| {}))),
        ..empty_details("Model details", "Back")
    });
    widget.set_focus_target(PopupFocus::DetailsBack);
    let back = widget.details_state().unwrap().back;
    assert_eq!(
        (back.aria_label.starts_with("Back"), back.icon_action, back.text_button, widget.focus_target() == Some(PopupFocus::DetailsBack)),
        (true, true, false, true),
    );
}

#[test]
fn the_go_back_keys_leave_details_and_return_focus_to_the_requested_row() {
    run_with_faked_timers(async move {
        let created = create_widget(true);
        let widget = created.widget.clone();
        created.widget.show(options(
            body_anchor(),
            &["Models"],
            "Models",
            |_, _| items_result(vec![action("first"), action("second")]),
            failing_delegate("Back must not select"),
        ));
        let restoring = widget.clone();
        created.widget.show_details(ITabbedActionListDetailsOptions {
            restore_focus: Some(Rc::new(move || restoring.focus_item("second"))),
            ..empty_details("Details", "Back")
        });
        widget.set_focus_target(PopupFocus::DetailsContent);
        let event = back_key();
        widget.dispatch_key_down(&event, TabbedKeyTarget::details());
        settle_layout().await;
        assert_eq!(
            (event.default_prevented(), widget.is_visible(), widget.is_showing_details(), widget.is_list_focused(), widget.list_focused_label()),
            (true, true, false, true, Some("second".to_owned())),
        );
    });
}

#[test]
fn left_and_right_belong_to_list_rows_and_the_editor_tab_keys_switch_tabs() {
    let created = create_widget(true);
    let widget = &created.widget;
    let tab_changes = Rc::new(RefCell::new(Vec::<String>::new()));
    let changes = tab_changes.clone();
    let _subscription = widget.on_did_change_tab().listen(move |tab| changes.borrow_mut().push(tab.clone()));
    let requested = Rc::new(RefCell::new(Vec::<String>::new()));
    let recording = requested.clone();
    let mut show = options(
        body_anchor(),
        &["Local", "Remote"],
        "Local",
        |_, _| items_result(vec![action("plain"), action("details")]),
        failing_delegate("Arrows must not select"),
    );
    show.open_item_details = Some(Rc::new(move |item| {
        let id = item.item.as_ref().map(|item| item.id.clone()).unwrap_or_default();
        recording.borrow_mut().push(id.clone());
        id == "details"
    }));
    widget.show(show);
    let list = TabbedKeyTarget::list();
    let without_details = (press(widget, key("ArrowLeft", KeyCode::LeftArrow), list), press(widget, key("ArrowRight", KeyCode::RightArrow), list));
    press(widget, key("ArrowDown", KeyCode::DownArrow), list);
    let with_details = press(widget, key("ArrowRight", KeyCode::RightArrow), list);
    let tabs_before_shortcut = tab_changes.borrow().clone();
    let next_tab = press(widget, next_tab_key(), list);
    assert_eq!(
        (without_details, with_details, requested.borrow().clone(), tabs_before_shortcut, next_tab, tab_changes.borrow().clone()),
        ((false, false), true, strings(&["plain", "details"]), Vec::<String>::new(), true, strings(&["Remote"])),
    );
    widget.hide();
}

#[test]
fn left_collapses_and_right_expands_a_collapsible_section_like_a_tree() {
    let created = create_widget(true);
    let widget = &created.widget;
    let toggles = Rc::new(RefCell::new(Vec::<bool>::new()));
    let recording = toggles.clone();
    let requested = Rc::new(RefCell::new(Vec::<String>::new()));
    let opened = requested.clone();
    let mut show = options(
        body_anchor(),
        &["Models"],
        "Models",
        move |_, _| {
            let mut items = vec![action("promoted")];
            items.push(IActionListItem { section: Some("other".to_owned()), is_section_toggle: true, ..action("other-models") });
            items.extend(["a", "b"].map(|id| IActionListItem { section: Some("other".to_owned()), ..action(id) }));
            let recording = recording.clone();
            ITabbedActionListBuildResult {
                items: rcs(items),
                list_options: Some(IActionListOptions {
                    collapsed_by_default: Some(["other".to_owned()].into_iter().collect()),
                    on_did_toggle_section: Some(Rc::new(move |_, collapsed| recording.borrow_mut().push(collapsed))),
                    ..IActionListOptions::default()
                }),
            }
        },
        failing_delegate("Arrows must not select"),
    );
    show.open_item_details = Some(Rc::new(move |item| {
        opened.borrow_mut().push(item.item.as_ref().map(|item| item.id.clone()).unwrap_or_default());
        false
    }));
    widget.show(show);
    let list = TabbedKeyTarget::list();
    let press_key = |key_name: &str, code: KeyCode| press(widget, key(key_name, code), list);
    // Count rows through the list's own labels: the collapsed section hides its children.
    let state = || widget.list_visible_labels().len();
    let left_on_unsectioned = press_key("ArrowLeft", KeyCode::LeftArrow);
    press_key("ArrowDown", KeyCode::DownArrow);
    let left_on_collapsed = press_key("ArrowLeft", KeyCode::LeftArrow);
    let expand = press_key("ArrowRight", KeyCode::RightArrow);
    let after_expand = (state(), widget.list_focused_label());
    let right_on_expanded = press_key("ArrowRight", KeyCode::RightArrow);
    press_key("ArrowDown", KeyCode::DownArrow);
    let right_on_child = press_key("ArrowRight", KeyCode::RightArrow);
    let collapse_from_child = press_key("ArrowLeft", KeyCode::LeftArrow);
    let after_collapse = (state(), widget.list_focused_label());
    assert_eq!(
        (
            left_on_unsectioned,
            left_on_collapsed,
            expand,
            after_expand,
            right_on_expanded,
            right_on_child,
            collapse_from_child,
            after_collapse,
            toggles.borrow().clone(),
            requested.borrow().clone(),
        ),
        (
            false,
            false,
            true,
            (4, Some("other-models".to_owned())),
            false,
            false,
            true,
            (2, Some("other-models".to_owned())),
            vec![false, true],
            strings(&["a"]),
        ),
    );
    widget.hide();
}

#[test]
fn details_defer_list_updates_and_restore_toolbar_focus_after_pin_like_changes() {
    run_with_faked_timers(async move {
        let created = create_widget(true);
        let widget = created.widget.clone();
        let builds = Rc::new(Cell::new(0usize));
        let updated = Rc::new(Cell::new(false));
        let counting = builds.clone();
        let reading = updated.clone();
        widget.show(options(
            body_anchor(),
            &["Models"],
            "Models",
            move |_, _| {
                counting.set(counting.get() + 1);
                let item = with_toolbar(action("model"), vec![to_action_with("details", "Details", noop())]);
                items_result(if reading.get() { vec![item, action("new")] } else { vec![item] })
            },
            failing_delegate("Details must not select"),
        ));
        let list = widget.elements().unwrap().list;
        let restoring = widget.clone();
        widget.show_details(ITabbedActionListDetailsOptions {
            restore_focus: Some(Rc::new(move || restoring.focus_item_action("model", "details"))),
            ..empty_details("Details", "Back")
        });
        // `button.focus()` no conteúdo dos detalhes.
        widget.set_focus_target(PopupFocus::DetailsContent);
        updated.set(true);
        widget.refresh_active_list(Some(ITabbedActionListRefreshOptions { focus_item_id: Some("new".to_owned()), ..ITabbedActionListRefreshOptions::default() }));
        let during = (builds.get(), widget.focus_target() == Some(PopupFocus::DetailsContent));
        // O clique no botão Voltar do cabeçalho.
        widget.hide_details();
        settle_layout().await;
        assert_eq!(
            (
                during,
                builds.get(),
                widget.is_visible(),
                Rc::ptr_eq(&list, &widget.elements().unwrap().list),
                focused_toolbar_label(&widget),
                widget.list_visible_labels().len(),
            ),
            ((1, true), 2, true, true, Some("Details".to_owned()), 2),
        );
    });
}

#[test]
fn back_keeps_the_picker_open_when_the_pointer_lands_on_another_row_or_toolbar() {
    run_with_faked_timers(async move {
        for hover_target in [PointerTarget::Row, PointerTarget::Toolbar] {
            let created = create_widget(true);
            let widget = created.widget.clone();
            widget.show(options(
                body_anchor(),
                &["Models"],
                "Models",
                |_, _| ITabbedActionListBuildResult {
                    items: rcs(
                        ["first", "second"]
                            .iter()
                            .map(|id| with_toolbar(action(id), vec![to_action_with("details", &format!("{id} details"), noop())]))
                            .collect(),
                    ),
                    list_options: Some(IActionListOptions { tab_through_item_actions: true, ..IActionListOptions::default() }),
                },
                failing_delegate("Back must not select"),
            ));
            let restoring = widget.clone();
            widget.show_details(ITabbedActionListDetailsOptions {
                restore_focus: Some(Rc::new(move || restoring.focus_item_action("first", "details"))),
                ..empty_details("Details", "Back")
            });
            widget.hide_details();
            let restored = focused_toolbar_label(&widget);
            // O `mousemove` na linha, ou no botão da barra dela, que sobe até a linha.
            list_of(&widget).widget().pointer_move(1, &moved_over(hover_target));
            settle_layout().await;
            assert_eq!(
                (restored, widget.is_visible(), widget.is_list_focused(), widget.list_focused_label()),
                (Some("first details".to_owned()), true, true, Some("second".to_owned())),
                "{hover_target:?}",
            );
            widget.hide();
        }
    });
}

#[test]
fn details_remain_interactive_while_the_footer_changes_the_collapsed_body() {
    run_with_faked_timers(async move {
        let result = create_collapsible_widget(true, true);
        let widget = result.widget().clone();
        widget.show_details(empty_details("Auto details", "Back"));
        // `button.focus()` e o clique do botão, que expande o corpo pelo rodapé.
        widget.set_focus_target(PopupFocus::DetailsContent);
        result.set_collapsed(false);
        let content = widget.details_state().unwrap().content;
        let during = (widget.focus_target() == Some(PopupFocus::DetailsContent), widget.body_state().unwrap().inert, has_inert_ancestor(&content));
        widget.hide_details();
        settle_layout().await;
        assert_eq!(
            (during, widget.is_visible(), widget.body_state().unwrap().inert),
            ((true, true, false), true, false),
        );
    });
}

#[test]
fn search_tab_reaches_row_actions_without_opening_a_hover_or_selecting_the_row() {
    let created = create_widget(true);
    let widget = created.widget.clone();
    let opened = Rc::new(Cell::new(0));
    let counting = opened.clone();
    widget.show(options(
        body_anchor(),
        &["Models"],
        "Models",
        move |_, _| {
            let counting = counting.clone();
            ITabbedActionListBuildResult {
                items: rcs(vec![
                    with_toolbar(action("model"), vec![to_action_with("details", "Details", Rc::new(move || counting.set(counting.get() + 1)))]),
                    with_toolbar(action("other"), vec![to_action_with("otherDetails", "Other Details", noop())]),
                ]),
                list_options: Some(IActionListOptions {
                    show_filter: true,
                    filter_as_combobox: true,
                    focus_filter_on_open: true,
                    tab_through_item_actions: true,
                    ..IActionListOptions::default()
                }),
            }
        },
        failing_delegate("Toolbar must not select"),
    ));
    let list = list_of(&widget);
    let tab_indices: Vec<i32> = (0..2).filter_map(|index| list.widget().toolbar_tab_index(index)).collect();
    widget.dispatch_key_down(&key("Tab", KeyCode::Tab), TabbedKeyTarget::filter_input(false));
    let label = focused_toolbar_label(&widget);
    // `toolbar.click()` roda a ação do botão; o clique não sobe pela lista, que não selecionaria a linha.
    let _ = list.widget().run_item_toolbar_action("model", "details");
    widget.dispatch_key_down(&key_with("Tab", KeyCode::Tab, |e| e.shift_key = true), row_control_target());
    assert_eq!(
        (label, opened.get(), widget.is_filter_input_focused(), widget.is_visible(), tab_indices),
        (Some("Details".to_owned()), 1, true, true, vec![0, -1]),
    );
}

#[test]
fn keeps_its_rows_when_the_page_leaves_no_room_beside_the_anchor() {
    let created = create_widget(true);
    // A long page pushes the anchor's free space out of view: the anchor, the whole body, is 5000px tall.
    created.widget.show(options(
        anchor_at(0.0, 0.0, 0.0, 5000.0),
        &["Models"],
        "Models",
        |_, _| ITabbedActionListBuildResult {
            items: rcs(vec![action("first"), action("second")]),
            list_options: Some(IActionListOptions { show_filter: true, ..IActionListOptions::default() }),
        },
        delegate(),
    ));
    assert_eq!(created.widget.list_visible_labels(), vec![Some("first".to_owned()), Some("second".to_owned())]);
}

#[test]
fn enter_on_a_row_switch_is_left_to_the_switch_instead_of_selecting_the_row() {
    let created = create_widget(true);
    let widget = created.widget.clone();
    let changes = Rc::new(RefCell::new(Vec::<bool>::new()));
    let recording = changes.clone();
    widget.show(options(
        body_anchor(),
        &["Models"],
        "Models",
        move |_, _| {
            let recording = recording.clone();
            items_result(vec![IActionListItem {
                inline_toggle: Some(IActionListItemInlineToggle {
                    label: "Use in Auto".to_owned(),
                    checked: true,
                    on_change: Rc::new(move |checked| recording.borrow_mut().push(checked)),
                    title: None,
                    show_info_icon: false,
                    disabled: false,
                }),
                ..action("model")
            }])
        },
        failing_delegate("The switch must not select the row"),
    ));
    let enter = key("Enter", KeyCode::Enter);
    widget.dispatch_key_down(&enter, row_control_target());
    // A synthetic keydown has no default action, so emulate the button's native activation.
    if !enter.default_prevented() {
        list_of(&widget).widget().click_inline_toggle(0);
    }
    let checked = list_of(&widget).widget().row_state(0).and_then(|row| row.switch).map(|switch| switch.checked);
    assert_eq!(
        (enter.default_prevented(), changes.borrow().clone(), checked, widget.is_visible()),
        (false, vec![false], Some(false), true),
    );
}

#[test]
fn details_scroll_independently_and_retain_a_fixed_back_button() {
    run_with_faked_timers(async move {
        let created = create_widget(true);
        let widget = created.widget.clone();
        widget.show(options(
            anchor_at(0.0, 0.0, 0.0, 20.0),
            &["Models"],
            "Models",
            |_, _| ITabbedActionListBuildResult {
                items: rcs(vec![action("model")]),
                list_options: Some(IActionListOptions { anchor_position: Some(AnchorPosition::Below), ..IActionListOptions::default() }),
            },
            failing_delegate("Scrolling must not select"),
        ));
        widget.show_details(ITabbedActionListDetailsOptions {
            render_header: Some(Rc::new(|container: &Rc<ViewElement>| {
                // O título `Model name`.
                container.set_size(0.0, 14.0);
                to_disposable(|| {})
            })),
            render: Rc::new(|container: &Rc<ViewElement>| {
                container.set_size(0.0, 1200.0);
                to_disposable(|| {})
            }),
            ..empty_details("Details", "Back")
        });
        settle_layout().await;
        let before = widget.details_state().unwrap();
        let header_before = before.header.rect().top;
        let back = before.back.clone();
        // `PageDown` no botão Voltar.
        let page_down = key("PageDown", KeyCode::PageDown);
        widget.dispatch_key_down(&page_down, TabbedKeyTarget::details());
        let after = widget.details_state().unwrap();
        let labels = widget.keybinding_labels();
        let back_label = if labels.back.is_empty() { "Back".to_owned() } else { format!("Back ({})", labels.back) };
        assert_eq!(
            (
                after.scroll_top > 0.0,
                after.header.rect().top == header_before,
                back.text.is_empty() && back.icon_class == "codicon-arrow-left",
                back.icon_action,
                back.aria_label == back_label,
                widget.is_showing_details(),
                widget.body_state().unwrap().main_aria_hidden,
            ),
            (true, true, true, true, true, true, true),
        );
    });
}

#[test]
fn details_stay_anchored_after_their_rendered_header_height_settles() {
    run_with_faked_timers(async move {
        let layout_service: Rc<dyn ILayoutService> = Rc::new(StubLayoutService { container: ViewElement::new(), layout: Emitter::new() });
        let service = ContextViewService::create((), (layout_service.clone(),));
        let service_view: Rc<dyn IContextViewService> = service.clone();
        let widget = TabbedActionListWidget::new(
            service_view,
            layout_service,
            Rc::new(StubAccessibilityService { motion_reduced: Rc::new(Cell::new(true)), reduced_motion: Emitter::new() }),
            Rc::new(MockKeybindingService),
        );
        // `anchor.style.cssText = 'position: fixed; top: 400px; left: 100px; width: 100px; height: 22px;'`.
        let anchor_top = 400.0;
        let gap = {
            let service = service.clone();
            move || {
                let view = service.get_context_view_element().rect();
                (anchor_top - (view.top + view.height)).round()
            }
        };
        let mut show = options(
            anchor_at(anchor_top, 100.0, 100.0, 22.0),
            &["Models"],
            "Models",
            |_, _| ITabbedActionListBuildResult {
                items: rcs(vec![action("model")]),
                list_options: Some(IActionListOptions { anchor_position: Some(AnchorPosition::Above), ..IActionListOptions::default() }),
            },
            delegate(),
        );
        show.width = Some(300.0);
        show.is_body_collapsed = Some(Rc::new(|| true));
        show.render_footer = Some(Rc::new(|container: &Rc<ViewElement>, _: &str| {
            container.set_size(0.0, 24.0);
            to_disposable(|| {})
        }));
        widget.show(show);
        let focus_state = Rc::new(RefCell::new(None::<(f64, bool)>));
        let recording = focus_state.clone();
        let measuring = gap.clone();
        widget.show_details(ITabbedActionListDetailsOptions {
            render_header: Some(Rc::new(|container: &Rc<ViewElement>| {
                // O título `Auto`, de 50px.
                container.set_size(0.0, 50.0);
                to_disposable(|| {})
            })),
            render: Rc::new(|container: &Rc<ViewElement>| {
                container.set_size(0.0, 60.0);
                to_disposable(|| {})
            }),
            focus: Some(Rc::new(move |page: &Rc<ViewElement>| {
                let details_visible = page.parent_element().is_some_and(|parent| parent.class_list_contains("showing-details"));
                *recording.borrow_mut() = Some((measuring(), details_visible));
            })),
            ..empty_details("Auto details", "Back")
        });
        settle_layout().await;
        let before = gap();
        // `title.style.height = '22px'` e o `ResizeObserver` dos detalhes.
        widget.details_state().unwrap().header.set_size(0.0, 22.0);
        widget.notify_details_resized();
        settle_layout().await;
        assert_eq!(
            (*focus_state.borrow(), before, gap(), widget.is_visible()),
            (Some((0.0, true)), 0.0, 0.0, true),
        );
        widget.hide();
    });
}

#[test]
fn show_makes_the_popup_visible_and_hide_dismisses_it() {
    let created = create_widget(true);
    created.widget.show(options(body_anchor(), &["Local", "Remote"], "Local", |_, _| items_result(vec![action("a")]), delegate()));
    assert!(created.widget.is_visible());
    created.widget.hide();
    assert!(!created.widget.is_visible());
}

#[test]
fn animates_fresh_popups_without_replaying_the_entrance_on_tab_changes() {
    let created = create_widget(true);
    let widget = &created.widget;
    let build = |initial_tab: &str| {
        let mut show = options(body_anchor(), &["Local", "Remote"], initial_tab, |_, _| items_result(vec![action("item")]), delegate());
        show.widget_class_names = Some(Rc::new(|_| vec!["custom-picker".to_owned(), ACTION_WIDGET_DROPDOWN_MOTION_CLASS.to_owned()]));
        show
    };
    let read = || {
        let popup = widget.elements().unwrap().widget;
        (
            popup.class_list_contains(ACTION_WIDGET_ANIMATED_CLASS),
            popup.class_list_contains(ACTION_WIDGET_DROPDOWN_MOTION_CLASS),
            popup.class_list_contains("custom-picker"),
        )
    };
    widget.show(build("Local"));
    let initial = read();
    widget.show(build("Remote"));
    let swapped = read();
    widget.hide();
    widget.show(build("Local"));
    assert_eq!((initial, swapped, read()), ((true, true, true), (false, false, true), (true, true, true)));
    widget.hide();
}

#[test]
fn passes_the_requested_context_view_layer_to_the_popup() {
    let created = create_widget(true);
    let mut show = options(body_anchor(), &["Models"], "Models", |_, _| items_result(vec![action("a")]), delegate());
    show.context_view_layer = Some(1);
    created.widget.show(show);
    assert_eq!(created.context_view.active_layer(), Some(1));
    created.widget.hide();
}

#[test]
fn items_receive_pointer_input_immediately_after_opening() {
    let created = create_widget(true);
    let widget = created.widget.clone();
    let selected = Rc::new(RefCell::new(Vec::<String>::new()));
    let recording = selected.clone();
    widget.show(options(
        body_anchor(),
        &["Local", "Remote"],
        "Local",
        |_, _| items_result(vec![action("first"), action("second")]),
        Rc::new(ShowDelegate { on_select: Rc::new(move |item| recording.borrow_mut().push(item.id.clone())), on_hide: Rc::new(|| {}) }),
    ));
    // `container.style.cssText = 'position: fixed; top: 20px; left: 20px; z-index: 10000;'`.
    let elements = widget.elements().unwrap();
    elements.widget.set_position(20.0, 20.0);
    elements.list.set_position(25.0, 25.0);
    let list = list_of(&widget);
    // `row.contains(document.elementFromPoint(centro da linha))`: o centro da linha cai dentro do popup, e não de outro elemento.
    let row = list.widget().visible_row_rect(1);
    let popup = elements.widget.rect();
    let (center_x, center_y) = (row.left + row.width / 2.0, row.top + row.height / 2.0);
    let center_inside_popup =
        center_x >= popup.left && center_x <= popup.left + popup.width && center_y >= popup.top && center_y <= popup.top + popup.height;
    list.widget().click_row(1, PointerTarget::Row);
    assert_eq!((center_inside_popup, selected.borrow().clone()), (true, strings(&["second"])));
    widget.hide();
}

#[test]
fn tab_bar_search_accepts_after_arrow_down_with_matching_results() {
    for matches in [vec!["first match"], vec!["first match", "second match"]] {
        let mut item_ids = matches.clone();
        item_ids.push("other");
        let search = create_searchable_widget(&item_ids);
        let widget = &search.created.widget;
        let down = key("ArrowDown", KeyCode::DownArrow);
        widget.dispatch_key_down(&down, TabbedKeyTarget::filter_input(true));
        let input_focused_after_navigation = widget.is_filter_input_focused();
        let enter = key("Enter", KeyCode::Enter);
        widget.dispatch_key_down(&enter, TabbedKeyTarget::filter_input(true));
        assert_eq!(
            (input_focused_after_navigation, down.default_prevented(), enter.default_prevented(), search.selected.borrow().clone(), widget.is_visible()),
            (true, true, true, strings(&[matches[matches.len() - 1]]), false),
            "{} matching results",
            matches.len(),
        );
    }
}

#[test]
fn tab_bar_search_keeps_text_editing_and_ime_keys_out_of_popup_navigation() {
    let search = create_searchable_widget(&["first match", "second match"]);
    let widget = &search.created.widget;
    let target = TabbedKeyTarget::filter_input(true);
    let mut events = vec![
        key("ArrowLeft", KeyCode::LeftArrow),
        key("ArrowRight", KeyCode::RightArrow),
        key("Home", KeyCode::Home),
        key("End", KeyCode::End),
        key_with("ArrowLeft", KeyCode::LeftArrow, |e| e.shift_key = true),
        key_with("ArrowDown", KeyCode::DownArrow, |e| e.shift_key = true),
        key_with("a", KeyCode::KeyA, |e| e.meta_key = true),
        key_with("Enter", KeyCode::Enter, |e| e.is_composing = true),
        key("Enter", KeyCode::KeyInComposition),
    ];
    for event in &events {
        widget.dispatch_key_down(event, target);
    }
    widget.dispatch_filter_composition(true);
    for event in [key("Enter", KeyCode::Enter), key("Escape", KeyCode::Escape)] {
        widget.dispatch_key_down(&event, target);
        events.push(event);
    }
    widget.dispatch_filter_composition(false);
    let visible_before_escape = widget.is_visible();
    widget.dispatch_key_down(&key("Escape", KeyCode::Escape), target);
    let prevented: Vec<String> = events.iter().filter(|event| event.default_prevented()).map(|event| event.key.clone()).collect();
    assert_eq!(
        (prevented, search.tab_changes.borrow().clone(), search.selected.borrow().clone(), visible_before_escape, widget.is_visible()),
        (Vec::<String>::new(), Vec::<String>::new(), Vec::<String>::new(), true, false),
    );
}

#[test]
fn checked_hover_on_open() {
    // `{ showCheckedItemHover, checked, shown }`.
    for (show_checked_item_hover, checked, shown) in [(true, true, true), (false, true, false), (true, false, false)] {
        let created = create_widget(true);
        let renders = Rc::new(Cell::new(0usize));
        let counting = renders.clone();
        let mut show = options(
            body_anchor(),
            &["Models"],
            "Models",
            move |_, _| {
                let counting = counting.clone();
                items_result(vec![
                    action("first"),
                    checked_with_hover(
                        "active",
                        checked,
                        IActionListItemHover {
                            expandable: true,
                            content: Some(HoverContent::Lazy(Rc::new(move || {
                                counting.set(counting.get() + 1);
                                HoverElement::new("Active model details")
                            }))),
                            ..IActionListItemHover::default()
                        },
                    ),
                ])
            },
            delegate(),
        );
        show.show_checked_item_hover = show_checked_item_hover;
        created.widget.show(show);
        let (panel_shown, focus_in_panel) = created.widget.list_panel_state();
        assert_eq!(
            (panel_shown, renders.get(), focus_in_panel),
            (shown, usize::from(shown), false),
            "showCheckedItemHover {show_checked_item_hover}, checked {checked}",
        );
        created.widget.hide();
    }
}

/// O popup de duas abas, em que a primeira decide a altura, dos casos `a size-preserving refresh...` e `a refresh deferred...`.
fn show_sized_popup(widget: &TabbedActionListWidget, item_count: &Rc<Cell<usize>>, sizing_builds: &Rc<Cell<usize>>) {
    let counting_items = item_count.clone();
    let counting_builds = sizing_builds.clone();
    let mut show = options(
        anchor_at(600.0, 0.0, 120.0, 20.0),
        &["Local", "Remote"],
        "Local",
        move |_, for_sizing| {
            if for_sizing {
                counting_builds.set(counting_builds.get() + 1);
            }
            ITabbedActionListBuildResult {
                items: rcs((0..counting_items.get()).map(|index| action(&format!("item-{index}"))).collect()),
                list_options: Some(IActionListOptions { anchor_position: Some(AnchorPosition::Above), ..IActionListOptions::default() }),
            }
        },
        delegate(),
    );
    show.sizing_tab = Some("Local".to_owned());
    show.width = Some(300.0);
    widget.show(show);
}

#[test]
fn a_size_preserving_refresh_keeps_the_popup_height_and_skips_re_measuring_the_sizing_tab() {
    let created = create_widget(true);
    let item_count = Rc::new(Cell::new(3usize));
    let sizing_builds = Rc::new(Cell::new(0usize));
    show_sized_popup(&created.widget, &item_count, &sizing_builds);
    let popup = created.widget.elements().unwrap().widget;
    let initial_height = popup.size().height;
    let initial_builds = sizing_builds.get();
    item_count.set(8);
    created.widget.refresh_active_list(Some(ITabbedActionListRefreshOptions { preserve_size: true, ..ITabbedActionListRefreshOptions::default() }));
    let preserved = (popup.size().height == initial_height, sizing_builds.get() - initial_builds);
    created.widget.refresh_active_list(None);
    assert_eq!(
        (preserved, (popup.size().height > initial_height, sizing_builds.get() - initial_builds)),
        ((true, 0), (true, 1)),
    );
}

#[test]
fn a_refresh_deferred_while_details_are_open_keeps_its_size_preserving_request() {
    let created = create_widget(true);
    let widget = created.widget.clone();
    let item_count = Rc::new(Cell::new(3usize));
    let sizing_builds = Rc::new(Cell::new(0usize));
    show_sized_popup(&widget, &item_count, &sizing_builds);
    let popup = widget.elements().unwrap().widget;
    let initial_height = popup.size().height;
    let refresh_while_details_open = |refresh: Option<ITabbedActionListRefreshOptions>| {
        widget.show_details(empty_details("Details", "Back"));
        item_count.set(item_count.get() + 1);
        widget.refresh_active_list(refresh);
        let builds = sizing_builds.get();
        widget.hide_details();
        (popup.size().height == initial_height, sizing_builds.get() - builds)
    };
    let preserved = refresh_while_details_open(Some(ITabbedActionListRefreshOptions {
        preserve_size: true,
        preserve_scroll_position: true,
        ..ITabbedActionListRefreshOptions::default()
    }));
    let remeasured = refresh_while_details_open(None);
    assert_eq!((preserved, remeasured), ((true, 0), (false, 1)));
}

#[test]
fn refresh_keeps_the_popup_open_when_rebuilding_removes_the_focused_card_control() {
    run_with_faked_timers(async move {
        let created = create_widget(true);
        let widget = created.widget.clone();
        let control = HoverElement::new("Configure");
        let refreshed = Rc::new(Cell::new(false));
        let reading = refreshed.clone();
        let content = control.clone();
        let mut show = options(
            body_anchor(),
            &["Models"],
            "Models",
            move |_, _| {
                let item = if reading.get() {
                    IActionListItem { item: Some(Item { id: "active".to_owned(), checked: true }), ..action("active") }
                } else {
                    checked_with_hover(
                        "active",
                        true,
                        IActionListItemHover { content: Some(HoverContent::Element(content.clone())), expandable: true, ..IActionListItemHover::default() },
                    )
                };
                ITabbedActionListBuildResult {
                    items: rcs(vec![item]),
                    list_options: Some(IActionListOptions { persistent_hover: true, ..IActionListOptions::default() }),
                }
            },
            delegate(),
        );
        show.show_checked_item_hover = true;
        widget.show(show);
        list_of(&widget).widget().focus_control(control.clone());
        refreshed.set(true);
        widget.refresh_active_list(None);
        timeout(0).await;
        let (card_shown, focus_in_panel) = widget.list_panel_state();
        assert_eq!((widget.is_visible(), widget.is_list_focused() || focus_in_panel, !card_shown), (true, true, true));
    });
}

#[test]
fn escape_from_a_detail_panel_or_button_returns_to_the_list_before_dismissing_the_picker() {
    run_with_faked_timers(async move {
        for focus_panel in [false, true] {
            let created = create_widget(true);
            let widget = created.widget.clone();
            let button = HoverElement::new("Pin Model");
            let content = button.clone();
            let mut show = options(
                body_anchor(),
                &["Models"],
                "Models",
                move |_, _| ITabbedActionListBuildResult {
                    items: rcs(vec![checked_with_hover(
                        "model",
                        true,
                        IActionListItemHover { content: Some(HoverContent::Element(content.clone())), expandable: true, ..IActionListItemHover::default() },
                    )]),
                    list_options: Some(IActionListOptions { persistent_hover: true, ..IActionListOptions::default() }),
                },
                delegate(),
            );
            show.show_checked_item_hover = true;
            widget.show(show);
            let list = list_of(&widget);
            if focus_panel {
                list.widget().focus_panel();
            } else {
                list.widget().focus_control(button.clone());
            }
            // O `keydown` sobe do botão, ou do próprio painel, até o painel.
            widget.dispatch_key_down(
                &key("Escape", KeyCode::Escape),
                TabbedKeyTarget { on_own_controls: true, list: Some(KeyTarget::SubmenuPanel), ..TabbedKeyTarget::default() },
            );
            timeout(0).await;
            let after_first_escape = (widget.is_visible(), !widget.list_panel_state().0, widget.is_list_focused());
            widget.dispatch_key_down(&key("Escape", KeyCode::Escape), TabbedKeyTarget::list());
            assert_eq!((after_first_escape, widget.is_visible()), ((true, true, true), false), "focus no painel: {focus_panel}");
        }
    });
}

#[test]
fn refresh_preserves_the_focused_detail_control_and_moves_its_row_with_reduced_motion() {
    run_with_faked_timers(async move {
        for motion_reduced in [false, true] {
            let created = create_widget(motion_reduced);
            let widget = created.widget.clone();
            let content = HoverElement::new("Pin Model");
            let pinned = Rc::new(Cell::new(false));
            let reading = pinned.clone();
            let shown_content = content.clone();
            let mut show = options(
                anchor_at(400.0, 20.0, 100.0, 20.0),
                &["Models"],
                "Models",
                move |_, _| {
                    let model = checked_with_hover(
                        "model",
                        true,
                        IActionListItemHover {
                            content: Some(HoverContent::Element(shown_content.clone())),
                            expandable: true,
                            preserve_vertical_position: true,
                            ..IActionListItemHover::default()
                        },
                    );
                    let others: Vec<TestItem> = ["one", "two", "three"].iter().map(|id| action(id)).collect();
                    let items: Vec<TestItem> = if reading.get() {
                        std::iter::once(model).chain(others).collect()
                    } else {
                        others.into_iter().chain(std::iter::once(model)).collect()
                    };
                    ITabbedActionListBuildResult {
                        items: rcs(items),
                        list_options: Some(IActionListOptions { show_filter: false, persistent_hover: true, ..IActionListOptions::default() }),
                    }
                },
                delegate(),
            );
            show.show_checked_item_hover = true;
            widget.show(show);
            let list = list_of(&widget);
            // `button.focus()`, no conteúdo do painel.
            list.widget().focus_control(content.clone());
            let before = list.widget().panel_rect().unwrap();
            pinned.set(true);
            widget.refresh_active_list(Some(ITabbedActionListRefreshOptions {
                focus_item_id: Some("model".to_owned()),
                preserve_hover: true,
                animate_item_move: true,
                ..ITabbedActionListRefreshOptions::default()
            }));
            let animated = !list.widget().item_move_animations().is_empty();
            list.widget().finish_item_move_animations();
            settle_layout().await;
            let after = list.widget().panel_rect().unwrap();
            assert_eq!(
                (
                    widget.is_visible(),
                    list.widget().panel_contains(&content),
                    list.widget().is_control_focused(&content),
                    (before.left - after.left).abs() < 1.0 && (before.top - after.top).abs() < 1.0,
                    animated,
                ),
                (true, true, true, true, !motion_reduced),
                "movimento reduzido: {motion_reduced}",
            );
            widget.hide();
        }
    });
}

#[test]
fn refresh_forwards_scroll_position_preservation_to_the_active_list() {
    let created = create_widget(true);
    created.widget.show(options(body_anchor(), &["Models"], "Models", |_, _| items_result(vec![action("model")]), delegate()));
    created.widget.refresh_active_list(Some(ITabbedActionListRefreshOptions { preserve_scroll_position: true, ..ITabbedActionListRefreshOptions::default() }));
    let last_update = list_of(&created.widget).widget().last_update_options();
    assert_eq!(last_update.map(|update| update.preserve_scroll_position), Some(true));
    created.widget.hide();
}

#[test]
fn buildable_items_are_requested_for_the_initial_tab() {
    let created = create_widget(true);
    let calls = Rc::new(RefCell::new(Vec::<String>::new()));
    let recording = calls.clone();
    created.widget.show(options(
        body_anchor(),
        &["Local", "Remote"],
        "Remote",
        move |tab, _| {
            recording.borrow_mut().push(tab.to_owned());
            items_result(vec![action(tab)])
        },
        delegate(),
    ));
    assert_eq!(calls.borrow().clone(), strings(&["Remote"]));
}

#[test]
fn popup_class_names_are_re_read_on_tab_switch_not_replayed_from_show() {
    let created = create_widget(true);
    let widget = &created.widget;
    let dimmed = Rc::new(Cell::new(false));
    let reading = dimmed.clone();
    let mut show = options(body_anchor(), &["Local", "Remote"], "Local", |_, _| items_result(vec![action("a")]), delegate());
    show.widget_class_names = Some(Rc::new(move |tab| {
        let mut names = vec!["picker".to_owned(), format!("tab-{tab}")];
        if reading.get() {
            names.push("dimmed".to_owned());
        }
        names
    }));
    widget.show(show);
    let on_show = popup_classes(widget);
    // State the popup reports changes while it stays open.
    dimmed.set(true);
    widget.refresh_active_list(None);
    let after_refresh = popup_classes(widget);
    // Switching tabs re-renders the popup, which must not bring back the old state.
    widget.click_tab(1);
    let after_tab_switch = popup_classes(widget);
    assert_eq!(
        (on_show, after_refresh, after_tab_switch),
        (
            strings(&[ACTION_WIDGET_ANIMATED_CLASS, "picker", "tab-Local"]),
            strings(&[ACTION_WIDGET_ANIMATED_CLASS, "dimmed", "picker", "tab-Local"]),
            strings(&["dimmed", "picker", "tab-Remote"]),
        ),
    );
    widget.hide();
}

#[test]
fn opening_with_a_collapsed_body_focuses_the_footer_without_animating() {
    let result = create_collapsible_widget(false, true);
    let state = result.widget().body_state().unwrap();
    assert_eq!(
        (result.body().size().height, state.inert, state.visibility(), result.footer_focused(), result.widget().body_animation().is_none()),
        (0.0, true, "hidden", true, true),
    );
}

#[test]
fn collapsing_and_expanding_preserves_focus_sizing_and_hover_placement() {
    run_with_faked_timers(async move {
        for motion_reduced in [false, true] {
            let result = create_collapsible_widget(motion_reduced, false);
            let widget = result.widget().clone();
            settle_layout().await;
            let initial_height = result.popup().size().height;
            let body_height = result.body().size().height;
            let list_height = widget.elements().unwrap().list.size().height;

            result.set_collapsed(true);
            let collapse_animated = widget.body_animation().is_some();
            widget.finish_body_animation();
            settle_layout().await;
            let state = widget.body_state().unwrap();
            let collapsed = (result.popup().size().height, result.body().size().height, state.inert, result.footer_focused(), state.visibility());

            result.set_collapsed(false);
            let expand_animated = widget.body_animation().is_some();
            let hover_deferred = !widget.list_panel_state().0;
            widget.finish_body_animation();
            settle_layout().await;
            let list = list_of(&widget);
            let panel = list.widget().panel_rect().expect("o hover reaparece com o corpo expandido");
            let expanded_row = (0..widget.list_visible_labels().len())
                .find(|index| list.widget().row_state(*index).is_some_and(|row| row.aria_expanded.as_deref() == Some("true")))
                .expect("a linha do hover aberto");
            let row = list.widget().visible_row_rect(expanded_row);
            let popup = result.popup().rect();
            let state = widget.body_state().unwrap();
            assert_eq!(
                (
                    collapse_animated,
                    expand_animated,
                    hover_deferred,
                    (panel.top + panel.height / 2.0 - row.top - row.height / 2.0).abs() < 1.0,
                    (panel.left - (popup.left + popup.width)).abs() < 1.0,
                    collapsed,
                    (
                        result.popup().size().height,
                        result.body().size().height,
                        widget.elements().unwrap().list.size().height,
                        state.inert,
                        result.footer_focused(),
                        state.visibility(),
                        widget.is_visible(),
                    ),
                ),
                (
                    !motion_reduced,
                    !motion_reduced,
                    !motion_reduced,
                    true,
                    true,
                    (initial_height - body_height, 0.0, true, true, "hidden"),
                    (initial_height, body_height, list_height, false, true, "visible", true),
                ),
                "movimento reduzido: {motion_reduced}",
            );
            widget.hide();
        }
    });
}

#[test]
fn reversing_a_collapse_continues_from_its_current_height_and_keeps_reanchoring() {
    run_with_faked_timers(async move {
        let result = create_collapsible_widget(false, false);
        let widget = result.widget().clone();
        let initial_height = result.body().size().height;
        result.set_collapsed(true);
        let collapse = widget.body_animation().expect("recolher anima");
        // `collapse.pause()` e `collapse.currentTime = 100`.
        widget.set_body_animation_time(100.0, true);
        let halfway_height = result.body().size().height;
        let layout_count = result.created.context_view.layout_count();
        settle_layout().await;
        let reanchored = result.created.context_view.layout_count() > layout_count;

        result.set_collapsed(false);
        let expand = widget.body_animation().expect("expandir anima");
        widget.finish_body_animation();
        settle_layout().await;
        // `collapse.playState === 'idle'`: a animação do recolher foi cancelada, e a que anda é outra.
        let cancelled = collapse != expand;
        assert_eq!(
            (
                halfway_height > 0.0 && halfway_height < initial_height,
                reanchored,
                cancelled,
                expand.from_height,
                result.body().size().height,
                widget.body_state().unwrap().animating,
            ),
            (true, true, true, halfway_height, initial_height, false),
        );
    });
}

#[test]
fn enabling_reduced_motion_finishes_an_active_collapse_immediately() {
    let result = create_collapsible_widget(false, false);
    let initial_height = result.body().size().height;
    result.set_collapsed(true);
    result.created.set_motion_reduced(true);
    let collapsed_height = result.body().size().height;
    result.set_collapsed(false);
    assert_eq!(
        (collapsed_height, result.body().size().height, result.widget().body_animation().is_none(), result.widget().body_state().unwrap().animating),
        (0.0, initial_height, true, false),
    );
}

#[test]
fn hiding_the_popup_cancels_its_active_collapse_animation() {
    let result = create_collapsible_widget(false, false);
    result.set_collapsed(true);
    let started = result.widget().body_animation().is_some();
    result.widget().hide();
    // `animation.playState === 'idle'` e `body.getAnimations().length === 0`: sem popup, não há animação.
    assert_eq!((result.widget().is_visible(), started, result.widget().body_animation().is_none()), (false, true, true));
}

#[test]
fn resizing_a_collapsed_footer_does_not_corrupt_the_expanded_popup_height() {
    run_with_faked_timers(async move {
        let result = create_collapsible_widget(true, true);
        let widget = result.widget().clone();
        settle_layout().await;
        let elements = widget.elements().unwrap();
        let expanded_height = result.popup().size().height + elements.body_content.size().height;
        let collapsed_height = result.popup().size().height;
        // `button.parentElement.style.height = '80px'` e o `ResizeObserver` do rodapé.
        elements.footer.as_ref().expect("o rodapé").set_size(0.0, 80.0);
        widget.notify_footer_resized();
        settle_layout().await;
        let resized_height = result.popup().size().height;
        result.set_collapsed(false);
        assert_eq!(
            (resized_height, result.popup().size().height, result.footer_focused()),
            (collapsed_height + 60.0, expanded_height, true),
        );
    });
}

/// `{ id: 'Local', label: 'Local models', icon: Codicon.deviceDesktop }` e `{ id: 'Remote', label: 'Remote models', icon: Codicon.cloud }`.
fn local_and_remote_tabs() -> Vec<ITabDescriptor> {
    vec![
        ITabDescriptor { label: Some("Local models".to_owned()), icon: Some(TabIcon::Theme("device-desktop".to_owned())), ..ITabDescriptor::new("Local") },
        ITabDescriptor { label: Some("Remote models".to_owned()), icon: Some(TabIcon::Theme("cloud".to_owned())), ..ITabDescriptor::new("Remote") },
    ]
}

/// O popup com `tabLabels: 'active'` dos casos de animação das abas.
fn show_tabs_with_active_labels(widget: &TabbedActionListWidget, tabs: Vec<ITabDescriptor>) {
    let mut show = ITabbedActionListShowOptions::new("test", body_anchor(), tabs, "Local", Rc::new(|_, _| items_result(vec![action("a")])), delegate());
    show.tab_labels = Some(TabLabels::Active);
    widget.show(show);
}

#[test]
fn switching_tabs_animates_them_between_their_old_and_new_widths() {
    let created = create_widget(false);
    // Only the active tab is labelled, so a switch resizes both the tab being left and the one being entered.
    show_tabs_with_active_labels(&created.widget, local_and_remote_tabs());
    let widths_before: Vec<Option<f64>> = created.widget.tabs().iter().map(|tab| Some(round_width(tab.box_.width))).collect();
    // The widths are captured on the way in, so the press precedes the click exactly as a real one does.
    created.widget.click_tab(1);
    // What matters is that each rebuilt tab starts from the width its predecessor had.
    let start_widths: Vec<Option<f64>> =
        created.widget.tabs().iter().map(|tab| tab.animations.first().map(|animation| round_width(animation.from.width))).collect();
    assert_eq!(start_widths, widths_before);
    // Leave nothing mounted.
    created.widget.hide();
}

#[test]
fn switching_tabs_does_not_animate_when_motion_is_reduced() {
    let created = create_widget(true);
    show_tabs_with_active_labels(&created.widget, local_and_remote_tabs());
    created.widget.click_tab(1);
    assert_eq!(created.widget.tabs().iter().filter(|tab| !tab.animations.is_empty()).count(), 0);
    created.widget.hide();
}

/// O rótulo da aba de saída: se o texto ainda tem `Local models` e se ela está marcada `label-collapsing`.
fn outgoing_tab_label(widget: &TabbedActionListWidget) -> (bool, bool) {
    let tabs = widget.tabs();
    (tabs[0].text_content().contains("Local models"), tabs[0].has_class("label-collapsing"))
}

#[test]
fn a_tab_that_loses_its_label_keeps_it_until_it_has_finished_shrinking() {
    run_with_faked_timers(async move {
        let created = create_widget(false);
        show_tabs_with_active_labels(&created.widget, local_and_remote_tabs());
        created.widget.click_tab(1);
        let while_shrinking = outgoing_tab_label(&created.widget);
        // The label is put back from the animation's finish event, which is dispatched on a later turn than `finish()` itself.
        created.widget.finish_tab_animations(0);
        timeout(0).await;
        assert_eq!((while_shrinking, outgoing_tab_label(&created.widget)), ((true, true), (false, false)));
        created.widget.hide();
    });
}

#[test]
fn an_image_tab_icon_is_drawn_by_the_tab_with_or_without_its_label() {
    let created = create_widget(true);
    let image_tab = ITabDescriptor {
        label: Some("Provider models".to_owned()),
        icon: Some(TabIcon::Image { light: uri_file("/provider-light.svg"), dark: uri_file("/provider-dark.svg") }),
        ..ITabDescriptor::new("Provider")
    };
    let local_tab = ITabDescriptor {
        label: Some("Local models".to_owned()),
        icon: Some(TabIcon::Theme("device-desktop".to_owned())),
        ..ITabDescriptor::new("Local")
    };
    show_tabs_with_active_labels(&created.widget, vec![local_tab, image_tab]);
    // `getComputedStyle(tab, '::before').backgroundImage` no tema claro e no escuro (`vs-dark` no contêiner).
    let describe = || {
        created
            .widget
            .tabs()
            .iter()
            .map(|tab| {
                let images = [false, true].map(|dark| {
                    tab.image_icon_for_theme(dark).map(|uri| {
                        let uri = uri.to_string(false).to_string();
                        if uri.contains("provider-light.svg") {
                            "light"
                        } else if uri.contains("provider-dark.svg") {
                            "dark"
                        } else {
                            "other"
                        }
                    })
                });
                // An image-only tab has no text, but must still look and act like the other tabs.
                (tab.text_content(), tab.has_codicon(), images, tab.image_icon_opacity().unwrap_or(1.0), tab.has_class("monaco-text-button"))
            })
            .collect::<Vec<_>>()
    };
    let before = describe();
    created.widget.click_tab(1);
    let after = describe();
    // Inactive tabs mute their codicons through color, which an image can only follow by fading.
    let expected = |codicon_text: &str, image_text: &str, image_opacity: f64| {
        vec![
            (codicon_text.to_owned(), true, [None, None], 1.0, true),
            (image_text.to_owned(), false, [Some("light"), Some("dark")], image_opacity, true),
        ]
    };
    assert_eq!((before, after), (expected("Local models", "", 0.7), expected("", "Provider models", 1.0)));
    created.widget.hide();
}

/// `heightsAcrossTabs(sizingTab, sizingItemIds)`: a altura da lista na aba curta, depois do refresh e na aba que dimensiona.
fn list_heights_across_tabs(sizing_tab: Option<&str>, sizing_item_ids: &[&str]) -> (f64, f64, f64) {
    let created = create_widget(true);
    let ids: Vec<String> = sizing_item_ids.iter().map(|id| (*id).to_owned()).collect();
    // Opens on the short tab on purpose: the height has to come from the sizing tab regardless of which tab the popup opens on.
    let mut show = options(
        body_anchor(),
        &["Copilot", "Ollama"],
        "Ollama",
        move |tab, _| items_result(if tab == "Copilot" { ids.iter().map(|id| action(id)).collect() } else { vec![action("only")] }),
        delegate(),
    );
    show.sizing_tab = sizing_tab.map(str::to_owned);
    created.widget.show(show);
    let on_short_tab = created.widget.list_height();
    created.widget.refresh_active_list(None);
    let after_refresh = created.widget.list_height();
    created.widget.click_tab(0);
    let on_sizing_tab = created.widget.list_height();
    created.widget.hide();
    (on_short_tab, after_refresh, on_sizing_tab)
}

#[test]
fn the_sizing_tab_decides_the_list_height_whichever_tab_the_popup_opens_on() {
    let all_items = ["a", "b", "c", "d", "e", "f"];
    let unsized_heights = list_heights_across_tabs(None, &all_items);
    let sized = list_heights_across_tabs(Some("Copilot"), &all_items);
    let empty = list_heights_across_tabs(Some("Copilot"), &[]);
    // Clamping depends on the room around the anchor, so compare how each tab is sized rather than the pixels.
    assert_eq!(
        (unsized_heights.0 < unsized_heights.2, sized.0 > unsized_heights.0, [empty.0, empty.1]),
        (true, true, [unsized_heights.0, unsized_heights.0]),
    );
}

#[test]
fn a_collapsed_section_in_the_sizing_tab_is_left_out_of_the_fixed_height() {
    // The sizing tab keeps most of its models behind a collapsed "other" section, so only the two rows on screen should count.
    let height_with_section = |collapsed: bool| {
        let created = create_widget(true);
        let mut show = options(
            body_anchor(),
            &["Copilot", "Ollama"],
            "Ollama",
            move |tab, _| {
                if tab != "Copilot" {
                    return items_result(vec![action("only")]);
                }
                let mut items = vec![action("promoted"), IActionListItem { is_section_toggle: true, section: Some("other".to_owned()), ..action("other-models") }];
                items.extend(["a", "b", "c", "d", "e", "f"].iter().map(|id| IActionListItem { section: Some("other".to_owned()), ..action(id) }));
                ITabbedActionListBuildResult {
                    items: rcs(items),
                    list_options: collapsed
                        .then(|| IActionListOptions { collapsed_by_default: Some(["other".to_owned()].into_iter().collect()), ..IActionListOptions::default() }),
                }
            },
            delegate(),
        );
        show.sizing_tab = Some("Copilot".to_owned());
        created.widget.show(show);
        let height = created.widget.list_height();
        created.widget.hide();
        height
    };
    assert_eq!(height_with_section(true) < height_with_section(false), true);
}

#[test]
fn refreshing_pins_preserves_collapsed_sizing_across_tabs_and_search() {
    run_with_faked_timers(async move {
        let created = create_widget(true);
        let widget = created.widget.clone();
        let content = HoverElement::new("Unpin Model");
        let pinned = Rc::new(Cell::new(true));
        let searching = Rc::new(Cell::new(false));
        let (reading_pinned, reading_search, shown_content) = (pinned.clone(), searching.clone(), content.clone());
        let mut show = options(
            anchor_at(400.0, 0.0, 120.0, 20.0),
            &["Copilot", "Other"],
            "Copilot",
            move |tab, for_sizing| {
                let model = IActionListItem {
                    section: if reading_pinned.get() { None } else { Some("other".to_owned()) },
                    ..checked_with_hover(
                        "model",
                        true,
                        IActionListItemHover { content: Some(HoverContent::Element(shown_content.clone())), expandable: true, ..IActionListItemHover::default() },
                    )
                };
                let items: Vec<TestItem> = if reading_search.get() && !for_sizing {
                    ["one", "two", "three", "four", "five", "six"].iter().map(|id| action(id)).collect()
                } else if tab == "Copilot" {
                    let mut items = Vec::new();
                    if reading_pinned.get() {
                        items.push(IActionListItem { label: Some("Pinned".to_owned()), ..IActionListItem::new(ActionListItemKind::Separator) });
                        items.push(model.clone());
                    }
                    items.push(action("suggested"));
                    items.push(IActionListItem { section: Some("other".to_owned()), is_section_toggle: true, ..action("other-models") });
                    if !reading_pinned.get() {
                        items.push(model);
                    }
                    items.extend(["one", "two", "three"].iter().map(|id| IActionListItem { section: Some("other".to_owned()), ..action(id) }));
                    items
                } else {
                    vec![action("provider-model")]
                };
                ITabbedActionListBuildResult {
                    items: rcs(items),
                    list_options: Some(IActionListOptions {
                        collapsed_by_default: Some(["other".to_owned()].into_iter().collect()),
                        anchor_position: Some(AnchorPosition::Above),
                        persistent_hover: true,
                        ..IActionListOptions::default()
                    }),
                }
            },
            delegate(),
        );
        show.sizing_tab = Some("Copilot".to_owned());
        show.show_checked_item_hover = true;
        widget.show(show);

        let list_height = || widget.elements().unwrap().list.size().height;
        let initial_height = list_height();
        let toggle_other = || {
            let list = list_of(&widget);
            let index = widget
                .list_visible_labels()
                .iter()
                .position(|label| label.as_deref() == Some("other-models"))
                .expect("a linha que recolhe a seção");
            list.widget().click_row(index, PointerTarget::Row);
        };
        // `content.focus()`.
        list_of(&widget).widget().focus_control(content.clone());
        pinned.set(false);
        widget.refresh_active_list(Some(ITabbedActionListRefreshOptions {
            focus_item_id: Some("model".to_owned()),
            preserve_hover: true,
            ..ITabbedActionListRefreshOptions::default()
        }));
        toggle_other();
        timeout(0).await;
        let after_unpin = list_height();
        let mut tab_heights = Vec::new();
        for index in [1, 0] {
            widget.click_tab(index);
            tab_heights.push(list_height());
        }
        searching.set(true);
        widget.refresh_active_list(None);
        let search_height = list_height();
        pinned.set(true);
        widget.refresh_active_list(None);
        let after_pin = list_height();
        widget.hide();

        assert_eq!(
            (after_unpin, tab_heights, search_height, after_pin),
            (initial_height / 2.0, vec![initial_height / 2.0, initial_height / 2.0], initial_height / 2.0, initial_height),
        );
    });
}

#[test]
fn resizing_the_footer_preserves_the_fixed_popup_height() {
    run_with_faked_timers(async move {
        for initial_footer_height in [20.0, 80.0] {
            let created = create_widget(true);
            let widget = created.widget.clone();
            let current_footer_height = Rc::new(Cell::new(initial_footer_height));
            let make_show = || {
                let reading = current_footer_height.clone();
                let mut show = options(
                    anchor_at(400.0, 0.0, 120.0, 20.0),
                    &["Copilot", "Other"],
                    "Copilot",
                    |tab, _| {
                        let ids = if tab == "Copilot" { vec!["a", "b", "c", "d", "e", "f"] } else { vec!["other"] };
                        ITabbedActionListBuildResult {
                            items: rcs(ids.into_iter().map(action).collect()),
                            list_options: Some(IActionListOptions { anchor_position: Some(AnchorPosition::Above), ..IActionListOptions::default() }),
                        }
                    },
                    delegate(),
                );
                show.sizing_tab = Some("Copilot".to_owned());
                show.width = Some(300.0);
                show.render_footer = Some(Rc::new(move |container: &Rc<ViewElement>, _: &str| {
                    container.set_size(0.0, reading.get());
                    to_disposable(|| {})
                }));
                show
            };
            widget.show(make_show());
            settle_layout().await;
            let elements = widget.elements().unwrap();
            let (popup, list, footer) = (elements.widget.clone(), elements.list.clone(), elements.footer.clone().expect("o rodapé"));
            let initial_height = popup.size().height;
            let initial_list_height = list.size().height;
            // `button.focus()`.
            widget.set_focus_target(PopupFocus::Footer);

            let other_footer_height = if initial_footer_height == 20.0 { 80.0 } else { 20.0 };
            let heights = [other_footer_height, initial_footer_height, 100.0, other_footer_height];
            let mut states = Vec::new();
            for height in heights {
                let layout_count = created.context_view.layout_count();
                current_footer_height.set(height);
                footer.set_size(0.0, height);
                widget.notify_footer_resized();
                settle_layout().await;
                states.push((
                    popup.size().height,
                    list.size().height,
                    footer.size().height,
                    widget.focus_target() == Some(PopupFocus::Footer),
                    created.context_view.layout_count() > layout_count,
                ));
            }

            let mut tab_heights = Vec::new();
            for index in [1, 0] {
                widget.click_tab(index);
                settle_layout().await;
                tab_heights.push(widget.elements().unwrap().widget.size().height);
            }
            widget.hide();
            widget.show(make_show());
            settle_layout().await;
            let reopened_height = widget.elements().unwrap().widget.size().height;

            assert_eq!(
                (states, tab_heights, reopened_height),
                (
                    heights.iter().map(|height| (initial_height, initial_list_height + initial_footer_height - height, *height, true, true)).collect(),
                    vec![initial_height, initial_height],
                    initial_height + current_footer_height.get() - initial_footer_height,
                ),
                "rodapé inicial de {initial_footer_height}px",
            );
            widget.hide();
        }
    });
}

#[test]
fn hide_then_show_resets_visibility_cleanly() {
    let created = create_widget(true);
    let show_once = || created.widget.show(options(body_anchor(), &["Local"], "Local", |_, _| items_result(vec![action("a")]), delegate()));
    show_once();
    created.widget.hide();
    assert!(!created.widget.is_visible());
    show_once();
    assert!(created.widget.is_visible());
    created.widget.hide();
}

#[test]
fn on_did_hide_fires_when_the_popup_dismisses() {
    let created = create_widget(true);
    let hidden = Rc::new(Cell::new(0));
    let counting = hidden.clone();
    let _subscription = created.widget.on_did_hide().listen(move |_| counting.set(counting.get() + 1));
    created.widget.show(options(body_anchor(), &["Local"], "Local", |_, _| items_result(vec![action("a")]), delegate()));
    // Simulate an external dismissal (e.g. user clicked outside).
    created.context_view.hide_context_view(None);
    assert_eq!((hidden.get(), created.widget.is_visible()), (1, false));
}
