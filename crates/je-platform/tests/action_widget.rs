//! Porte de `src/vs/platform/actionWidget/test/browser/actionWidget.test.ts` (suíte `ActionWidgetService`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - **DOM.** O widget nativo é o estado do `ActionWidgetService` e da visão de contexto. `container`, `anchor`, `input` e
//!   `.action-widget` são `ViewElement` e o `FilterInput`; `getBoundingClientRect()` é `ViewElement::rect()` e
//!   `style.display === 'none'` é `is_hidden()`. `dispatchEvent(new KeyboardEvent('keydown', ...))` é
//!   `dispatch_key_down` com o alvo; `dispatchEvent(new Event('input'))` e os eventos de composição são
//!   `dispatch_filter_input` e `dispatch_filter_composition`. `document.activeElement` é o foco que a lista guarda
//!   (`FilterInput::is_focused`) ou, para `trigger.focus()`/`warning.focus()`, uma variável do teste.
//! - **`getSingletonServiceDescriptors`.** `showWidget` confere que o `registerSingleton` registrou o serviço; os serviços
//!   são montados com o `create` do `Injectable`, que o `createServices` chama com as dependências resolvidas.
//! - **`sinon.useFakeTimers()`** é o relógio virtual do `runtime` (`with_faked_timers` e `advance_fake_time`); os casos
//!   assíncronos rodam em `run_with_faked_timers`, e `timeout(n)` espera no mesmo relógio.
//! - **Remoção por clique.** `removeButton.click()` (e a sequência `mousedown`, `mouseup`, `click`) é
//!   `click_submenu_toolbar_action` com o id `actionList.remove`, que roda a ação da barra da linha.
//! - **`Object.assign(toAction(...), { onRemove })`** é o `RemovableAction`.
//! - **Viewport.** `window.innerWidth/innerHeight` são o `Viewport` do `FakeLayoutService` (1280 por 800).

mod common;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::actions::{ActionRef, ToActionProps, to_action};
use je_base::async_::{async_fn, timeout};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::key_codes::KeyCode;
use je_base::layout::{AnchorPosition, IAnchor, ISize};
use je_base::promise::Promise;
use je_base::runtime::{advance_fake_time, with_faked_timers};
use je_instantiation::{Injectable, get_singleton_service_descriptors};
use je_platform::action_widget::action_list::{
    ActionListItemData, IActionListDelegate, IActionListItem, IActionListOptions, IActionListUpdateOptions, KeyTarget, OnRemove, REMOVE_TOOLBAR_ACTION_ID,
    RemovableAction,
};
use je_platform::action_widget::action_widget::{ActionWidgetService, register};
use je_platform::action_widget::action_widget_dropdown::{action_widget_dropdown_close_animation, with_action_widget_dropdown_motion};
use je_platform::action_widget::action_widget_motion::{
    ACTION_WIDGET_ANIMATED_CLASS, ACTION_WIDGET_DROPDOWN_MOTION_CLASS, ACTION_WIDGET_DROPDOWN_MOTION_CLOSING_CLASS,
};
use je_platform::contextkey::IContextKeyService;
use je_platform::contextview::context_view::{AnchorTarget, IContextViewService};
use je_platform::contextview::context_view_service::ContextViewService;
use je_platform::contextview::view_element::{UiKeyEvent, ViewElement};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::layout::layout_service::{ILayoutService, LayoutContainerEvent, Viewport};

const VIEWPORT_WIDTH: f64 = 1280.0;
const VIEWPORT_HEIGHT: f64 = 800.0;

/// O item `{ id }` dos casos.
#[derive(Clone)]
struct Item {
    id: String,
}

impl ActionListItemData for Item {
    fn list_item_id(&self) -> Option<String> {
        Some(self.id.clone())
    }
}

fn item(id: &str) -> Item {
    Item { id: id.to_owned() }
}

/// `{ kind: ActionListItemKind.Action, label: id, item: { id } }`.
fn action_item(id: &str) -> Rc<IActionListItem<Item>> {
    Rc::new(IActionListItem::action(id, item(id)))
}

/// `upcastPartial<ILayoutService>({ getContainer, mainContainer, activeContainer, onDidLayoutContainer })`.
struct FakeLayoutService {
    container: Rc<ViewElement>,
    layout: Rc<Emitter<LayoutContainerEvent>>,
}

impl ILayoutService for FakeLayoutService {
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
        Viewport { inner_width: VIEWPORT_WIDTH, inner_height: VIEWPORT_HEIGHT, page_x_offset: 0.0, page_y_offset: 0.0 }
    }
}

/// Os serviços de um caso: o contêiner, o `layout` (`onDidLayoutContainer`), a visão de contexto e o serviço.
struct Env {
    container: Rc<ViewElement>,
    layout: Rc<Emitter<LayoutContainerEvent>>,
    context_view: Rc<ContextViewService>,
    service: Rc<ActionWidgetService>,
}

impl Env {
    /// `layout.fire({ container, dimension })`.
    fn fire_layout(&self, width: f64, height: f64) {
        self.layout.fire(LayoutContainerEvent { container: self.container.clone(), dimension: ISize { width, height } });
    }
}

fn element_at(top: f64, left: f64, width: f64, height: f64) -> Rc<ViewElement> {
    let element = ViewElement::new();
    element.set_position(top, left);
    element.set_size(width, height);
    element
}

fn setup_env(guard: &LeakGuard, container_client_height: f64) -> Env {
    let container = ViewElement::new();
    container.set_client_height(container_client_height);
    let layout = Emitter::new();
    guard.add_rc(layout.clone());
    let layout_service: Rc<dyn ILayoutService> = Rc::new(FakeLayoutService { container: container.clone(), layout: layout.clone() });
    let context_view = guard.add_rc(ContextViewService::create((), (layout_service.clone(),)));
    let context_keys: Rc<dyn IContextKeyService> = MockContextKeyService::new();
    let context_view_service: Rc<dyn IContextViewService> = context_view.clone();
    let service = guard.add_rc(ActionWidgetService::create((), (context_view_service, context_keys, layout_service)));
    Env { container, layout, context_view, service }
}

/// O delegate dos casos que registram o que foi selecionado e os `onHide`, e que escondem o widget ao selecionar.
struct RecordingDelegate {
    service: Weak<ActionWidgetService>,
    selected: Rc<RefCell<Vec<String>>>,
    hides: Rc<RefCell<Vec<Option<bool>>>>,
    hide_on_select: bool,
}

impl IActionListDelegate<Item> for RecordingDelegate {
    fn on_hide(&self, did_cancel: Option<bool>) {
        self.hides.borrow_mut().push(did_cancel);
    }

    fn on_select(&self, action: &Item, _preview: bool) {
        self.selected.borrow_mut().push(action.id.clone());
        if self.hide_on_select {
            if let Some(service) = self.service.upgrade() {
                service.hide(None);
            }
        }
    }
}

struct Shown {
    env: Env,
    selected: Rc<RefCell<Vec<String>>>,
    cancelled: Rc<RefCell<Vec<Option<bool>>>>,
}

/// `showWidget(filterAsCombobox, contextViewLayer)`.
fn show_widget(guard: &LeakGuard, filter_as_combobox: bool, context_view_layer: Option<i32>) -> Shown {
    register();
    assert!(get_singleton_service_descriptors().iter().any(|descriptor| descriptor.service_name() == "actionWidgetService"));
    let env = setup_env(guard, 400.0);
    // container.style.cssText = 'position: relative; width: 400px; height: 400px;'
    env.container.set_size(400.0, 400.0);
    // anchor.style.cssText = 'position: absolute; bottom: 0; width: 100px; height: 22px;'
    let anchor = element_at(400.0 - 22.0, 0.0, 100.0, 22.0);
    anchor.set_parent(Some(env.container.clone()));
    let selected = Rc::new(RefCell::new(Vec::new()));
    let cancelled = Rc::new(RefCell::new(Vec::new()));
    env.service.show(
        "test",
        false,
        &[action_item("first match"), action_item("second match")],
        Rc::new(RecordingDelegate { service: Rc::downgrade(&env.service), selected: selected.clone(), hides: cancelled.clone(), hide_on_select: true }),
        AnchorTarget::Element(anchor),
        None,
        Some(Vec::new()),
        Some(Rc::new(IActionListOptions {
            show_filter: true,
            focus_filter_on_open: true,
            initial_filter_value: Some("match".to_owned()),
            filter_as_combobox,
            ..IActionListOptions::default()
        })),
        context_view_layer,
    );
    Shown { env, selected, cancelled }
}

fn key_down(key: &str, key_code: KeyCode) -> UiKeyEvent {
    UiKeyEvent::new(key, key_code)
}

#[test]
fn opts_plain_popups_into_shared_motion_without_a_caller_provided_dropdown_class() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let shown = show_widget(&guard, true, None);
    let widget = shown.env.service.widget_element().unwrap();
    assert_eq!(
        (widget.class_list_contains(ACTION_WIDGET_ANIMATED_CLASS), widget.class_list_contains(ACTION_WIDGET_DROPDOWN_MOTION_CLASS)),
        (true, false),
    );
    shown.env.service.hide(None);
}

/// O corpo de `only combobox popups handle Escape before the shared keybindings: ${filterAsCombobox}`.
fn only_combobox_popups_handle_escape(filter_as_combobox: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let shown = show_widget(&guard, filter_as_combobox, None);
    let escape = key_down("Escape", KeyCode::Escape);
    shown.env.service.dispatch_key_down(&escape, KeyTarget::FilterInput);

    assert_eq!(
        (shown.env.service.is_visible(), escape.default_prevented(), shown.cancelled.borrow().clone()),
        (!filter_as_combobox, filter_as_combobox, if filter_as_combobox { vec![Some(true)] } else { Vec::new() }),
    );
    shown.env.service.hide(None);
}

#[test]
fn only_combobox_popups_handle_escape_before_the_shared_keybindings_undefined() {
    only_combobox_popups_handle_escape(false);
}

#[test]
fn only_combobox_popups_handle_escape_before_the_shared_keybindings_true() {
    only_combobox_popups_handle_escape(true);
}

#[test]
fn renders_above_a_containing_context_view_when_requested() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let shown = show_widget(&guard, false, Some(1));

    assert_eq!(shown.env.context_view.get_context_view_element().style_get_property("z-index").as_deref(), Some("2576"));
    shown.env.service.hide(None);
}

#[test]
fn search_navigation_keeps_input_focus_and_enter_accepts_and_closes_the_popup() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let shown = show_widget(&guard, true, None);
    let service = shown.env.service.clone();
    service.dispatch_key_down(&key_down("ArrowDown", KeyCode::DownArrow), KeyTarget::FilterInput);
    let input_focused_after_navigation = service.is_filter_input_focused();
    service.dispatch_key_down(&key_down("Enter", KeyCode::Enter), KeyTarget::FilterInput);

    assert_eq!(
        (input_focused_after_navigation, shown.selected.borrow().clone(), service.is_visible()),
        (true, vec!["second match".to_owned()], false),
    );
}

#[test]
fn escape_does_not_dismiss_the_combobox_popup_during_composition() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let shown = show_widget(&guard, true, None);
    shown.env.service.dispatch_filter_composition(true);
    let escape = key_down("Escape", KeyCode::Escape);
    shown.env.service.dispatch_key_down(&escape, KeyTarget::FilterInput);

    assert_eq!(
        (shown.env.service.is_visible(), escape.default_prevented(), shown.cancelled.borrow().clone()),
        (true, false, Vec::new()),
    );
    shown.env.service.hide(None);
}

/// O delegate do `picker closing motion`: só conta os `onHide`.
struct HideCounter {
    hides: Rc<Cell<u32>>,
}

impl IActionListDelegate<String> for HideCounter {
    fn on_hide(&self, _did_cancel: Option<bool>) {
        self.hides.set(self.hides.get() + 1);
    }

    fn on_select(&self, _action: &String, _preview: bool) {}
}

/// O corpo de `picker closing motion with ${name}`.
fn picker_closing_motion(classes: &str, animate: bool) {
    with_faked_timers(|| {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_env(&guard, 0.0);
        for class_name in classes.split(' ') {
            env.container.class_list_add(class_name);
        }
        let hides = Rc::new(Cell::new(0));
        env.service.show(
            "test",
            false,
            &[Rc::new(IActionListItem::<String>::action("Action", "action".to_owned()))],
            Rc::new(HideCounter { hides: hides.clone() }),
            AnchorTarget::Anchor(IAnchor { x: 100.0, y: 100.0, width: None, height: None }),
            Some(env.container.clone()),
            None,
            Some(Rc::new(with_action_widget_dropdown_motion(None))),
            None,
        );
        let view = env.context_view.get_context_view_element();
        let widget = env.service.widget_element().unwrap();
        let read = || (hides.get(), view.is_hidden());

        env.service.hide(None);
        env.service.hide(None);
        let (initial_hides, initial_hidden) = read();
        let initial = (initial_hides, initial_hidden, widget.class_list_contains(ACTION_WIDGET_DROPDOWN_MOTION_CLOSING_CLASS));
        let duration = action_widget_dropdown_close_animation().duration as u64;
        advance_fake_time(duration - 1);
        let before_end = read();
        advance_fake_time(1);
        assert_eq!((initial, before_end, read()), ((1, !animate, animate), (1, !animate), (1, true)));
    });
}

#[test]
fn picker_closing_motion_with_editor_motion() {
    picker_closing_motion("modern-ui monaco-enable-motion", true);
}

#[test]
fn picker_closing_motion_with_agents_glass_motion() {
    picker_closing_motion("agent-sessions-workbench modern-ui-frosted-glass monaco-enable-motion", true);
}

#[test]
fn picker_closing_motion_with_agents_without_glass() {
    picker_closing_motion("agent-sessions-workbench monaco-enable-motion", false);
}

#[test]
fn picker_closing_motion_with_agents_reduced_motion() {
    picker_closing_motion("agent-sessions-workbench modern-ui-frosted-glass monaco-reduce-motion", false);
}

#[test]
fn picker_closing_motion_with_editor_reduced_motion() {
    picker_closing_motion("modern-ui monaco-reduce-motion", false);
}

#[test]
fn picker_closing_motion_with_legacy_motion() {
    picker_closing_motion("monaco-enable-motion", false);
}

/// O corpo de `resolves preferred side ${preferredAnchorPosition} before context view placement with fallback ${fallback}`.
fn resolves_preferred_side(preferred_anchor_position: AnchorPosition, fallback: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup_env(&guard, 0.0);
    let y = if fallback {
        if preferred_anchor_position == AnchorPosition::Above { 20.0 } else { VIEWPORT_HEIGHT - 44.0 }
    } else {
        VIEWPORT_HEIGHT / 2.0
    };
    let expected_above = if fallback { preferred_anchor_position == AnchorPosition::Below } else { preferred_anchor_position == AnchorPosition::Above };
    let selected = Rc::new(RefCell::new(Vec::new()));
    let hides = Rc::new(RefCell::new(Vec::new()));
    env.service.show(
        "placement",
        false,
        &[action_item("first"), action_item("second"), action_item("third")],
        Rc::new(RecordingDelegate { service: Rc::downgrade(&env.service), selected, hides, hide_on_select: false }),
        AnchorTarget::Anchor(IAnchor { x: 200.0, y, width: Some(100.0), height: Some(24.0) }),
        None,
        Some(Vec::new()),
        Some(Rc::new(IActionListOptions {
            preferred_anchor_position: Some(preferred_anchor_position),
            show_filter: true,
            focus_filter_on_open: true,
            ..IActionListOptions::default()
        })),
        None,
    );
    let popup = env.context_view.get_context_view_element();
    let before = popup.rect();
    env.service.dispatch_filter_input("first");
    env.fire_layout(900.0, VIEWPORT_HEIGHT);
    let after = popup.rect();
    assert_eq!(
        (
            if expected_above { before.top + before.height <= y + 1.0 } else { before.top >= y + 23.0 },
            if expected_above { after.top + after.height <= y + 1.0 } else { after.top >= y + 23.0 },
            after.height < before.height,
            env.service.is_filter_input_focused(),
            env.service.is_visible(),
        ),
        (true, true, true, true, true),
    );
    env.service.hide(None);
}

#[test]
fn resolves_preferred_side_above_before_context_view_placement_with_fallback_false() {
    resolves_preferred_side(AnchorPosition::Above, false);
}

#[test]
fn resolves_preferred_side_above_before_context_view_placement_with_fallback_true() {
    resolves_preferred_side(AnchorPosition::Above, true);
}

#[test]
fn resolves_preferred_side_below_before_context_view_placement_with_fallback_false() {
    resolves_preferred_side(AnchorPosition::Below, false);
}

#[test]
fn resolves_preferred_side_below_before_context_view_placement_with_fallback_true() {
    resolves_preferred_side(AnchorPosition::Below, true);
}

/// `toAction({ id, label, checked, run })`.
fn action(id: &str, label: &str, checked: Option<bool>, run: Rc<dyn Fn()>) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label: label.to_owned(),
        tooltip: None,
        enabled: None,
        checked,
        class: None,
        alt: None,
        run: Rc::new(move |_| {
            run();
            Dynamic::Undefined.into()
        }),
    })
}

/// O delegate dos casos de `IAction`: `onSelect: action => action.run()`.
struct RunActionDelegate {
    on_hide: Rc<dyn Fn()>,
}

impl IActionListDelegate<ActionRef> for RunActionDelegate {
    fn on_hide(&self, _did_cancel: Option<bool>) {
        (self.on_hide)();
    }

    fn on_select(&self, action: &ActionRef, _preview: bool) {
        let _ = action.run(&[]);
    }
}

#[test]
fn closes_an_inline_permission_action_once_before_focusing_a_warning_dialog() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup_env(&guard, 0.0);
    // `document.activeElement`: o botão que tem o foco.
    let active = Rc::new(Cell::new("none"));
    let events: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
    let service = Rc::downgrade(&env.service);
    let permissions_toggle = {
        let mut toggle = IActionListItem::<ActionRef>::action("Permissions", action("permissions", "Permissions", None, Rc::new(|| {})));
        toggle.section = Some("permissions".to_owned());
        toggle.is_section_toggle = true;
        Rc::new(toggle)
    };
    let allow_all = {
        let (events, active, service) = (events.clone(), active.clone(), service.clone());
        let run: Rc<dyn Fn()> = Rc::new(move || {
            if let Some(service) = service.upgrade() {
                service.hide(None);
            }
            events.borrow_mut().push("warning");
            active.set("warning");
        });
        let mut allow_all = IActionListItem::<ActionRef>::action("Allow All", action("allowAll", "Allow All", Some(true), run));
        allow_all.section = Some("permissions".to_owned());
        Rc::new(allow_all)
    };
    let on_hide: Rc<dyn Fn()> = {
        let (events, active) = (events.clone(), active.clone());
        Rc::new(move || {
            events.borrow_mut().push("hide");
            active.set("trigger");
        })
    };
    env.service.show(
        "permissions",
        false,
        &[permissions_toggle, allow_all],
        Rc::new(RunActionDelegate { on_hide }),
        AnchorTarget::Anchor(IAnchor { x: 400.0, y: 400.0, width: Some(100.0), height: Some(24.0) }),
        None,
        Some(Vec::new()),
        Some(Rc::new(IActionListOptions {
            anchor_position: Some(AnchorPosition::Above),
            initial_focus_item_id: Some("allowAll".to_owned()),
            ..IActionListOptions::default()
        })),
        None,
    );
    env.service.accept_selected(false);
    assert_eq!(
        (events.borrow().clone(), active.get() == "warning", env.service.is_visible()),
        (vec!["hide", "warning"], true, false),
    );
}

#[test]
fn keeps_inline_menus_open_across_workbench_layout_changes_with_or_without_initial_item_focus() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup_env(&guard, 0.0);
    let mut states = Vec::new();
    for initial_focus_item_id in [None, Some("manual")] {
        let hides = Rc::new(Cell::new(0));
        let on_hide: Rc<dyn Fn()> = {
            let hides = hides.clone();
            Rc::new(move || hides.set(hides.get() + 1))
        };
        env.service.show(
            "mode",
            false,
            &[
                Rc::new(IActionListItem::<ActionRef>::action("Mode", action("mode", "Mode", Some(true), Rc::new(|| {})))),
                Rc::new(IActionListItem::<ActionRef>::action("Manual", action("manual", "Manual", Some(true), Rc::new(|| {})))),
            ],
            Rc::new(RunActionDelegate { on_hide }),
            AnchorTarget::Anchor(IAnchor { x: 400.0, y: 400.0, width: Some(100.0), height: Some(24.0) }),
            None,
            Some(Vec::new()),
            Some(Rc::new(IActionListOptions {
                anchor_position: Some(AnchorPosition::Above),
                initial_focus_item_id: initial_focus_item_id.map(str::to_owned),
                use_full_height: true,
                ..IActionListOptions::default()
            })),
            None,
        );
        let open_after_initial_layout = env.service.is_visible();
        env.fire_layout(900.0, 600.0);
        states.push((initial_focus_item_id, open_after_initial_layout, env.service.is_visible(), hides.get()));
        env.service.hide(None);
    }
    assert_eq!(states, vec![(None, true, true, 0), (Some("manual"), true, true, 0)]);
}

/// `makeParent(children)`.
fn make_parent(children: Vec<ActionRef>) -> Rc<IActionListItem<Item>> {
    let mut parent = IActionListItem::<Item>::action("Remote", item("remote"));
    parent.submenu_actions = Some(children);
    Rc::new(parent)
}

fn preserve_hover() -> IActionListUpdateOptions {
    IActionListUpdateOptions { preserve_hover: true, ..IActionListUpdateOptions::default() }
}

/// O `removable` dos dois últimos casos: a ação `Remove Me` com o `onRemove` que espera `delay` ms e então troca os itens do
/// pai por um que só tem `keep`.
fn removable_action(service: Weak<ActionWidgetService>, keep: ActionRef, delay: u64) -> ActionRef {
    let on_remove: OnRemove = Rc::new(move || {
        let (service, keep) = (service.clone(), keep.clone());
        async_fn(async move {
            timeout(delay).await?;
            if let Some(service) = service.upgrade() {
                service.update_items(&[make_parent(vec![keep])], None, Some(preserve_hover()));
            }
            Ok::<(), Error>(())
        })
    });
    RemovableAction::new(action("remove", "Remove Me", None, Rc::new(|| {})), on_remove)
}

/// O `service.show('remote', ...)` dos dois últimos casos.
fn show_remote(env: &Env, removable: ActionRef, keep: ActionRef, selected: Rc<RefCell<Vec<String>>>, hides: Rc<RefCell<Vec<Option<bool>>>>, hide_on_select: bool) {
    env.service.show(
        "remote",
        false,
        &[make_parent(vec![removable, keep])],
        Rc::new(RecordingDelegate { service: Rc::downgrade(&env.service), selected, hides, hide_on_select }),
        AnchorTarget::Anchor(IAnchor { x: 400.0, y: 400.0, width: Some(100.0), height: Some(24.0) }),
        None,
        Some(Vec::new()),
        Some(Rc::new(IActionListOptions { show_filter: true, ..IActionListOptions::default() })),
        None,
    );
}

#[test]
fn keeps_a_nested_submenu_open_when_removing_a_remote_row_refreshes_parent_items() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_env(&guard, 0.0);
        let selected = Rc::new(RefCell::new(Vec::new()));
        let hides = Rc::new(RefCell::new(Vec::new()));
        let keep = action("keep", "Keep", None, Rc::new(|| {}));
        let removable = removable_action(Rc::downgrade(&env.service), keep.clone(), 0);
        show_remote(&env, removable, keep, selected.clone(), hides.clone(), true);

        // widget.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }))
        env.service.dispatch_key_down(&key_down("ArrowRight", KeyCode::RightArrow), KeyTarget::List);
        assert!(env.service.submenu_rows().is_some());
        let click = env.service.click_submenu_toolbar_action("Remove Me", REMOVE_TOOLBAR_ACTION_ID);
        assert!(click.is_some());
        timeout(0).await.unwrap();
        timeout(0).await.unwrap();

        assert_eq!(
            (env.service.is_visible(), hides.borrow().len(), selected.borrow().len(), env.service.submenu_rows()),
            (true, 0, 0, Some(vec![Some("Keep".to_owned())])),
        );
        env.service.hide(None);
    });
}

#[test]
fn keeps_submenu_open_while_remove_action_completes_asynchronously() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_env(&guard, 0.0);
        let selected = Rc::new(RefCell::new(Vec::new()));
        let hides = Rc::new(RefCell::new(Vec::new()));
        let keep = action("keep", "Keep", None, Rc::new(|| {}));
        let removable = removable_action(Rc::downgrade(&env.service), keep.clone(), 350);
        show_remote(&env, removable, keep, selected, hides.clone(), false);

        env.service.dispatch_key_down(&key_down("ArrowRight", KeyCode::RightArrow), KeyTarget::List);
        assert!(env.service.submenu_rows().is_some());
        // mousedown, mouseup e click no primeiro botão da barra do painel.
        let first_row = env.service.submenu_rows().unwrap().into_iter().next().flatten().unwrap();
        let click: Option<Promise<()>> = env.service.click_submenu_toolbar_action(&first_row, REMOVE_TOOLBAR_ACTION_ID);
        assert!(click.is_some());
        timeout(450).await.unwrap();

        assert_eq!((env.service.is_visible(), hides.borrow().len()), (true, 0));
        timeout(100).await.unwrap();
        env.service.hide(None);
    });
}
