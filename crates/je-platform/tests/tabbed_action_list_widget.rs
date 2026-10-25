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
//! - **Fora desta fatia.** Os casos que dependem de `tabThroughItemActions` do `ActionList` (a barra de ferramentas por linha com
//!   Tab), de `elementFromPoint` e dos `sinon.spy` do protótipo do `ActionList` dependem do `ActionList` completo e não estão
//!   aqui: `Back keeps the picker open when the pointer lands on another {row,toolbar}`, `search Tab reaches row actions...`
//!   e `refresh forwards scroll position preservation to the active list`.

#![allow(dead_code)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::action_list_support::{Item, StubLayoutService, TestItem, action, rcs};
use common::keybinding::{MockKeybindingService, os};
use common::run_with_faked_timers;
use je_base::async_::timeout;
use je_base::event::{Emitter, Event};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService};
use je_platform::action_widget::action_list::{IActionListDelegate, IActionListItem, IActionListOptions};
use je_platform::action_widget::action_widget_motion::{ACTION_WIDGET_ANIMATED_CLASS, ACTION_WIDGET_DROPDOWN_MOTION_CLASS};
use je_platform::action_widget::tabbed_action_list_widget::{
    ITabBarAction, ITabDescriptor, ITabbedActionListBuildResult, ITabbedActionListDetailsOptions, ITabbedActionListShowOptions, PopupFocus,
    TabbedActionListWidget, TabbedKeyTarget, details_focus_outline,
};
use je_platform::contextview::context_view::{AnchorTarget, HideData, IContextViewDelegate, IContextViewService};
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
