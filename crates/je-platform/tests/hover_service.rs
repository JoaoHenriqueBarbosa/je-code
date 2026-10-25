//! Porte de `src/vs/platform/hover/test/browser/hoverService.test.ts` (suíte `HoverService`), caso a caso e na mesma ordem,
//! até onde o `HoverService` sozinho responde.
//!
//! Notas de tradução:
//! - **DOM.** `document.body.contains(domNode)` é a cadeia de `parent_element` até o `fixture` (`is_ancestor`), que o `render`
//!   do hover cria ao pôr o nó na visão de contexto e a destruição desfaz. `dispatchEvent(new MouseEvent(...))` é
//!   `FakeDom::dispatch` (o evento sobe pela cadeia de pais e termina no `document`) e `document.dispatchEvent` é
//!   `FakeDom::dispatch_document`. `querySelectorAll('.monaco-hover').length` é o número de hovers vivos na árvore.
//! - **Serviços.** `TestInstantiationService.stub(...)` vira o `HoverServiceDeps`: `TestConfigurationService` com
//!   `workbench.hover.delay` e `reducedDelay` em 0, o `MockKeybindingService`, o `FakeLayoutService` do teste do
//!   `actionWidget` (o `fixture` como todos os contêineres), um `IAccessibilityService` sem leitor de tela e a
//!   `ContextViewService` real, uma por hover (o `new ContextView(container, ABSOLUTE)` do original).
//! - **Widget.** O `HoverWidget` posicionado (`platform/hover/browser/hoverWidget.ts`) é outro arquivo do oráculo; aqui
//!   entra o `FakeHoverWidget`, que guarda `isLocked`, `isDisposed`, as classes adicionais e o nó na visão de contexto.
//! - **Timers.** `runWithFakedTimers({ useFakeTimers: true }, ...)` é `run_with_faked_timers`.
//!
//! Casos que ficam para quando o `HoverWidget` posicionado for portado (dependem de geometria, de `ResizeObserver`, do
//! rastreador de mouse ou do `ManagedHoverWidget` renderizando de verdade): os nove primeiros de `showInstantHover`
//! (alinhamento à direita, centro do ponteiro, recorte, caixa de borda, largura restrita, conteúdo rolável e conteúdo curto),
//! `should apply additional classes to hover DOM`, os casos de `setupManagedHover` que abrem o hover real (`updates a pinned
//! hover`, `forwards onDidShow`, `should update options dynamically`, `managed HTML content`, `mouseleave`, `focus an already
//! visible`, `retain replacement`, os dois `should not re-show`) e a suíte `layout and resize` salvo `clicking outside`.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use common::keybinding::MockKeybindingService;
use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::timeout;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::layout::AnchorPosition;
use je_base::lifecycle::{DisposableRef, IDisposable, none, to_disposable};
use je_base::promise::Promise;
use je_instantiation::Injectable;
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextview::context_view::{IContextViewDelegate, IContextViewService};
use je_platform::contextview::context_view_service::ContextViewService;
use je_platform::contextview::view_element::ViewElement;
use je_platform::hover::hover_service::{
    HoverContent, HoverDelay, HoverDelegateTarget, HoverDomEvent, HoverDomEventKind, HoverDomHandler, HoverDomTarget, HoverService, HoverServiceDeps, HoverTarget,
    IHoverContextView, IHoverContextViewFactory, IHoverDelegate, IHoverDom, IHoverLifecycleOptions, IHoverOptions, IHoverPersistenceOptions, IHoverWidget,
    IHoverWidgetFactory, IHoverWidgetHandle, IManagedHover, IManagedHoverWidget, IManagedHoverWidgetFactory, ManagedHoverContentOrFactory,
    StructuredHoverTarget, is_ancestor,
};
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::layout::layout_service::{ILayoutService, LayoutContainerEvent, Viewport};

/// `upcastPartial<ILayoutService>({ activeContainer, mainContainer, getContainer, onDidLayoutContainer })`.
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
        Viewport { inner_width: 1280.0, inner_height: 800.0, page_x_offset: 0.0, page_y_offset: 0.0 }
    }
}

/// `new TestAccessibilityService()`.
struct FakeAccessibilityService;

impl IAccessibilityService for FakeAccessibilityService {
    fn on_did_change_screen_reader_optimized(&self) -> Event<()> {
        Event::none()
    }
    fn on_did_change_reduced_motion(&self) -> Event<()> {
        Event::none()
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
        false
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

struct Listener {
    id: u64,
    target: HoverDomTarget,
    kind: HoverDomEventKind,
    handler: HoverDomHandler,
}

/// O DOM do teste: ouvintes por elemento, `document` e `window`, e os atributos.
struct FakeDom {
    this: Weak<FakeDom>,
    next_id: Cell<u64>,
    listeners: RefCell<Vec<Listener>>,
    attributes: RefCell<Vec<(usize, String, String)>>,
    active: RefCell<Option<Rc<ViewElement>>>,
}

fn key(element: &Rc<ViewElement>) -> usize {
    Rc::as_ptr(element) as usize
}

impl FakeDom {
    fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self { this: this.clone(), next_id: Cell::new(1), listeners: RefCell::new(Vec::new()), attributes: RefCell::new(Vec::new()), active: RefCell::new(None) })
    }

    fn fire(&self, matches: impl Fn(&HoverDomTarget) -> bool, kind: HoverDomEventKind, event: &HoverDomEvent) {
        let snapshot: Vec<(u64, HoverDomHandler)> = self.listeners.borrow().iter().filter(|l| l.kind == kind && matches(&l.target)).map(|l| (l.id, l.handler.clone())).collect();
        for (id, handler) in snapshot {
            if self.listeners.borrow().iter().any(|l| l.id == id) {
                handler(event);
            }
        }
    }

    /// `target.dispatchEvent(new XEvent(type, { bubbles: true }))`: o elemento, os pais e o `document`.
    fn dispatch(&self, target: &Rc<ViewElement>, kind: HoverDomEventKind, mut event: HoverDomEvent) {
        event.target = Some(target.clone());
        let mut candidate = Some(target.clone());
        while let Some(element) = candidate {
            self.fire(|t| matches!(t, HoverDomTarget::Element(e) if Rc::ptr_eq(e, &element)), kind, &event);
            candidate = element.parent_element();
        }
        self.fire(|t| matches!(t, HoverDomTarget::Document), kind, &event);
    }

    /// `document.dispatchEvent(new MouseEvent(type, { bubbles: true }))`.
    fn dispatch_document(&self, kind: HoverDomEventKind, mut event: HoverDomEvent) {
        event.target = None;
        self.fire(|t| matches!(t, HoverDomTarget::Document), kind, &event);
    }
}

impl IHoverDom for FakeDom {
    fn add_listener(&self, target: HoverDomTarget, kind: HoverDomEventKind, _capture: bool, handler: HoverDomHandler) -> DisposableRef {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.listeners.borrow_mut().push(Listener { id, target, kind, handler });
        let dom = self.this.clone();
        to_disposable(move || {
            if let Some(dom) = dom.upgrade() {
                dom.listeners.borrow_mut().retain(|l| l.id != id);
            }
        })
    }

    fn active_element(&self) -> Option<Rc<ViewElement>> {
        self.active.borrow().clone()
    }

    fn is_ancestor_of_active_element(&self, element: &Rc<ViewElement>) -> bool {
        self.active.borrow().as_ref().is_some_and(|active| is_ancestor(active, element))
    }

    fn focus(&self, element: &Rc<ViewElement>) {
        *self.active.borrow_mut() = Some(element.clone());
    }

    fn has_attribute(&self, element: &Rc<ViewElement>, name: &str) -> bool {
        self.get_attribute(element, name).is_some()
    }

    fn get_attribute(&self, element: &Rc<ViewElement>, name: &str) -> Option<String> {
        self.attributes.borrow().iter().find(|(k, n, _)| *k == key(element) && n == name).map(|(_, _, value)| value.clone())
    }

    fn set_attribute(&self, element: &Rc<ViewElement>, name: &str, value: &str) {
        let mut attributes = self.attributes.borrow_mut();
        attributes.retain(|(k, n, _)| !(*k == key(element) && n == name));
        attributes.push((key(element), name.to_owned(), value.to_owned()));
    }

    fn remove_attribute(&self, element: &Rc<ViewElement>, name: &str) {
        self.attributes.borrow_mut().retain(|(k, n, _)| !(*k == key(element) && n == name));
    }

    fn is_editable_element(&self, _element: &Rc<ViewElement>) -> bool {
        false
    }

    fn observe_intersection(&self, _element: &Rc<ViewElement>, _on_change: Rc<dyn Fn(bool)>) -> Option<DisposableRef> {
        None
    }
}

/// O hover posicionado do teste: o estado que o serviço lê.
struct FakeHoverWidget {
    node: Rc<ViewElement>,
    locked: Cell<bool>,
    disposed: Cell<bool>,
    on_dispose: Rc<Emitter<()>>,
    on_layout: Rc<Emitter<()>>,
}

impl IDisposable for FakeHoverWidget {
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }
        self.node.set_parent(None);
        self.on_dispose.fire(());
        self.on_dispose.dispose();
        self.on_layout.dispose();
    }
}

impl IHoverWidget for FakeHoverWidget {
    fn is_disposed(&self) -> bool {
        self.disposed.get()
    }
}

impl IHoverWidgetHandle for FakeHoverWidget {
    fn dom_node(&self) -> Rc<ViewElement> {
        self.node.clone()
    }
    fn is_locked(&self) -> bool {
        self.locked.get()
    }
    fn set_is_locked(&self, locked: bool) {
        self.locked.set(locked);
    }
    fn is_mouse_in(&self) -> bool {
        false
    }
    fn on_dispose(&self) -> Event<()> {
        self.on_dispose.event()
    }
    fn on_request_layout(&self) -> Event<()> {
        self.on_layout.event()
    }
    fn add_mouse_tracking_element(&self, _element: &Rc<ViewElement>) -> DisposableRef {
        none()
    }
    fn render(&self, container: &Rc<ViewElement>) {
        self.node.set_parent(Some(container.clone()));
    }
    fn focus(&self) {}
    fn layout(&self) {}
    fn anchor(&self) -> AnchorPosition {
        AnchorPosition::Above
    }
    fn x(&self) -> f64 {
        0.0
    }
    fn y(&self) -> f64 {
        0.0
    }
}

struct FakeHoverWidgetFactory {
    created: RefCell<Vec<Rc<FakeHoverWidget>>>,
}

impl IHoverWidgetFactory for FakeHoverWidgetFactory {
    fn create(&self, options: &mut IHoverOptions) -> Rc<dyn IHoverWidgetHandle> {
        let node = ViewElement::with_classes(&["monaco-hover"]);
        for class_name in &options.additional_classes {
            node.class_list_add(class_name);
        }
        let widget = Rc::new(FakeHoverWidget { node, locked: Cell::new(false), disposed: Cell::new(false), on_dispose: Emitter::new(), on_layout: Emitter::new() });
        self.created.borrow_mut().push(widget.clone());
        widget
    }
}

/// `new ContextView(container, ABSOLUTE)` sobre a `ContextViewService` real, uma por hover.
struct ServiceBackedContextView {
    service: Rc<ContextViewService>,
    container: Rc<ViewElement>,
}

impl IHoverContextView for ServiceBackedContextView {
    fn show(&self, delegate: Rc<dyn IContextViewDelegate>) {
        self.service.show_context_view(delegate, Some(self.container.clone()), false);
    }
    fn layout(&self) {
        self.service.layout();
    }
    fn dispose(&self) {
        self.service.dispose();
    }
}

struct ContextViewFactory {
    layout_service: Rc<dyn ILayoutService>,
}

impl IHoverContextViewFactory for ContextViewFactory {
    fn create(&self, container: Rc<ViewElement>) -> Rc<dyn IHoverContextView> {
        Rc::new(ServiceBackedContextView { service: ContextViewService::create((), (self.layout_service.clone(),)), container })
    }
}

/// O `ManagedHoverWidget` não é exercitado por estes casos: nenhum hover gerenciável do teste chega a mostrar.
struct UnusedManagedHoverWidgetFactory;

struct UnusedManagedHoverWidget;

impl IDisposable for UnusedManagedHoverWidget {
    fn dispose(&self) {}
}

impl IManagedHoverWidget for UnusedManagedHoverWidget {
    fn is_disposed(&self) -> bool {
        false
    }
    fn update(&self, _content: je_platform::hover::hover_service::ManagedHoverContent, _focus: Option<bool>, _options: Option<je_platform::hover::hover_service::IManagedHoverOptions>) -> Promise<()> {
        Promise::resolved(())
    }
}

impl IManagedHoverWidgetFactory for UnusedManagedHoverWidgetFactory {
    fn create(&self, _delegate: &Rc<dyn IHoverDelegate>, _target: Rc<HoverDelegateTarget>, _fade_in_animation: bool) -> Rc<dyn IManagedHoverWidget> {
        Rc::new(UnusedManagedHoverWidget)
    }
}

/// `{ showHover: () => undefined, delay: 0, showNativeHover: true }`.
struct NativeDelegate;

impl IHoverDelegate for NativeDelegate {
    fn delay(&self) -> HoverDelay {
        HoverDelay::Fixed(0.0)
    }
    fn show_native_hover(&self) -> bool {
        true
    }
}

struct Env {
    fixture: Rc<ViewElement>,
    dom: Rc<FakeDom>,
    config: Rc<TestConfigurationService>,
    widgets: Rc<FakeHoverWidgetFactory>,
    service: Rc<HoverService>,
}

fn setup(guard: &LeakGuard) -> Env {
    let fixture = ViewElement::new();
    let layout = Emitter::new();
    guard.add_rc(layout.clone());
    let layout_service: Rc<dyn ILayoutService> = Rc::new(FakeLayoutService { container: fixture.clone(), layout: layout.clone() });
    let config = TestConfigurationService::new(None);
    let _ = config.set_user_configuration("workbench.hover.delay", Dynamic::Number(0.0), None);
    let _ = config.set_user_configuration("workbench.hover.reducedDelay", Dynamic::Number(0.0), None);
    let dom = FakeDom::new();
    let widgets = Rc::new(FakeHoverWidgetFactory { created: RefCell::new(Vec::new()) });
    let on_did_show_context_menu = Event::none();
    let keybinding_service: Rc<dyn IKeybindingService> = Rc::new(MockKeybindingService);
    let service = guard.add_rc(HoverService::new(HoverServiceDeps {
        dom: dom.clone(),
        configuration_service: config.clone() as Rc<dyn IConfigurationService>,
        on_did_show_context_menu,
        keybinding_service,
        layout_service: layout_service.clone(),
        accessibility_service: Rc::new(FakeAccessibilityService),
        hover_widgets: widgets.clone(),
        context_views: Rc::new(ContextViewFactory { layout_service }),
        managed_hover_widgets: Rc::new(UnusedManagedHoverWidgetFactory),
    }));
    Env { fixture, dom, config, widgets, service }
}

impl Env {
    /// `createTarget()`.
    fn create_target(&self) -> Rc<ViewElement> {
        let target = ViewElement::new();
        target.set_size(100.0, 100.0);
        target.set_parent(Some(self.fixture.clone()));
        target
    }

    fn options(&self, content: &str, target: &Rc<ViewElement>) -> IHoverOptions {
        IHoverOptions::new(HoverContent::Text(content.to_owned()), HoverTarget::Element(target.clone()))
    }

    /// `showHover(content, target?, options?)`.
    fn show_hover(&self, content: &str, target: Option<Rc<ViewElement>>, tweak: impl FnOnce(&mut IHoverOptions)) -> Rc<dyn IHoverWidgetHandle> {
        let target = target.unwrap_or_else(|| self.create_target());
        let mut options = self.options(content, &target);
        tweak(&mut options);
        self.service.show_instant_hover(options, false, false).unwrap_or_else(|| panic!("Hover with content \"{content}\" should be created"))
    }

    /// `mainWindow.document.body.contains(asHoverWidget(hover).domNode)`.
    fn is_in_dom(&self, hover: &Rc<dyn IHoverWidgetHandle>) -> bool {
        is_ancestor(&hover.dom_node(), &self.fixture)
    }

    /// `document.querySelectorAll('.monaco-hover').length`.
    fn hover_count(&self) -> usize {
        self.widgets.created.borrow().iter().filter(|widget| is_ancestor(&widget.node, &self.fixture)).count()
    }

    /// `createNestedHover(parentHover, content)`.
    fn create_nested_hover(&self, parent: &Rc<dyn IHoverWidgetHandle>, content: &str) -> Rc<dyn IHoverWidgetHandle> {
        let nested_target = ViewElement::new();
        nested_target.set_parent(Some(parent.dom_node()));
        self.show_hover(content, Some(nested_target), |_| {})
    }

    /// `createHoverChain(depth)`.
    fn create_hover_chain(&self, depth: usize) -> Vec<Rc<dyn IHoverWidgetHandle>> {
        let mut hovers: Vec<Rc<dyn IHoverWidgetHandle>> = Vec::new();
        let mut current_target = self.create_target();
        for index in 0..depth {
            let options = self.options(&format!("Hover {}", index + 1), &current_target);
            let Some(hover) = self.service.show_instant_hover(options, false, false) else {
                break;
            };
            current_target = ViewElement::new();
            current_target.set_parent(Some(hover.dom_node()));
            hovers.push(hover);
        }
        hovers
    }

    fn mouse_event(&self) -> HoverDomEvent {
        HoverDomEvent::new(None)
    }
}

fn sticky(options: &mut IHoverOptions) {
    options.persistence = Some(IHoverPersistenceOptions { sticky: Some(true), ..Default::default() });
}

fn dispose_hovers(hovers: &[Rc<dyn IHoverWidgetHandle>]) {
    for hover in hovers.iter().rev() {
        hover.dispose();
    }
}

fn same(a: &Rc<dyn IHoverWidgetHandle>, b: &Rc<dyn IHoverWidgetHandle>) -> bool {
    Rc::ptr_eq(a, b)
}

// region showInstantHover

#[test]
fn should_not_show_hover_with_empty_content() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let target = env.create_target();
    let hover = env.service.show_instant_hover(env.options("", &target), false, false);
    assert!(hover.is_none(), "Hover should not be created for empty content");
}

#[test]
fn should_call_on_did_show_callback_when_hover_is_shown() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let target = env.create_target();
    let did_show_called = Rc::new(Cell::new(false));
    let mut options = env.options("Test", &target);
    let flag = did_show_called.clone();
    options.on_did_show = Some(Rc::new(move || flag.set(true)));

    let hover = env.service.show_instant_hover(options, false, false);

    assert!(did_show_called.get(), "onDidShow should be called");
    let hover = hover.expect("hover");
    assert!(env.is_in_dom(&hover), "Hover should be in DOM after showing");

    hover.dispose();
    assert!(!env.is_in_dom(&hover), "Hover should be removed from DOM after dispose");
}

#[test]
fn should_call_on_did_hide_exactly_once_when_hover_is_disposed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let target = env.create_target();
    let did_hide_count = Rc::new(Cell::new(0));
    let mut options = env.options("Test", &target);
    let count = did_hide_count.clone();
    options.on_did_hide = Some(Rc::new(move || count.set(count.get() + 1)));

    let hover = env.service.show_instant_hover(options, false, false).expect("hover");
    hover.dispose();
    hover.dispose();

    assert_eq!(did_hide_count.get(), 1);
}

#[test]
fn should_call_on_did_hide_when_hover_is_hidden_during_on_did_show() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let target = env.create_target();
    let calls = Rc::new(RefCell::new(Vec::<&'static str>::new()));
    let mut options = env.options("Test", &target);
    let (show_calls, service) = (calls.clone(), Rc::downgrade(&env.service));
    options.on_did_show = Some(Rc::new(move || {
        show_calls.borrow_mut().push("show");
        if let Some(service) = service.upgrade() {
            service.hide_hover(true);
        }
    }));
    let hide_calls = calls.clone();
    options.on_did_hide = Some(Rc::new(move || hide_calls.borrow_mut().push("hide")));

    env.service.show_instant_hover(options, false, false);

    assert_eq!(*calls.borrow(), vec!["show", "hide"]);
}

#[test]
fn should_deduplicate_hovers_by_id() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let target = env.create_target();
    let with_id = |content: &str, id: &str| {
        let mut options = env.options(content, &target);
        options.id = Some(id.to_owned());
        options
    };

    let hover1 = env.service.show_instant_hover(with_id("Same content", "same-id"), false, false);
    let hover2 = env.service.show_instant_hover(with_id("Same content", "same-id"), false, false);

    let hover1 = hover1.expect("First hover should be created");
    assert!(env.is_in_dom(&hover1), "First hover should be in DOM");
    assert!(hover2.is_none(), "Second hover with same id should not be created");

    // Different id should create new hover
    let hover3 = env.service.show_instant_hover(with_id("Content 3", "different-id"), false, false).expect("Hover with different id should be created");
    assert!(env.is_in_dom(&hover3), "Third hover should be in DOM");

    hover1.dispose();
    hover3.dispose();
}

// endregion

// region hideHover

#[test]
fn should_hide_non_locked_hover() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let hover = env.show_hover("Test", None, |_| {});
    assert!(env.is_in_dom(&hover), "Hover should be in DOM initially");

    env.service.hide_hover(false);

    assert!(hover.is_disposed(), "Hover should be disposed after hideHover");
    assert!(!env.is_in_dom(&hover), "Hover should be removed from DOM after hideHover");
}

#[test]
fn should_not_hide_locked_hover_without_force_flag() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let hover = env.show_hover("Test", None, sticky);
    assert!(env.is_in_dom(&hover), "Locked hover should be in DOM");

    env.service.hide_hover(false);
    assert!(!hover.is_disposed(), "Locked hover should not be disposed without force");
    assert!(env.is_in_dom(&hover), "Locked hover should remain in DOM");

    env.service.hide_hover(true);
    assert!(hover.is_disposed(), "Locked hover should be disposed with force=true");
    assert!(!env.is_in_dom(&hover), "Locked hover should be removed from DOM with force");
}

#[test]
fn should_cancel_a_delayed_hover_that_has_not_been_shown_yet() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        let _ = env.config.set_user_configuration("workbench.hover.delay", Dynamic::Number(500.0), None);

        let target = env.create_target();
        let hover = env.service.show_delayed_hover(env.options("Manage", &target), &IHoverLifecycleOptions::default()).expect("Hover should be created");
        assert!(!env.is_in_dom(&hover), "Hover should not be visible before the delay elapses");

        // Simulates something else taking over, e.g. a context menu opening
        env.service.hide_hover(false);

        timeout(500).await.unwrap();
        assert!(!env.is_in_dom(&hover), "Cancelled delayed hover should never be shown");
    });
}

// endregion

// region nested hovers

#[test]
fn should_keep_parent_hover_visible_when_nested_hover_is_created() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let parent = env.show_hover("Parent", None, |_| {});
    assert!(env.is_in_dom(&parent), "Parent hover should be in DOM");

    let nested = env.create_nested_hover(&parent, "Nested");
    assert!(env.is_in_dom(&nested), "Nested hover should be in DOM");
    assert!(env.is_in_dom(&parent), "Parent hover should still be in DOM after nested hover created");

    assert!(!parent.is_disposed(), "Parent hover should remain visible");
    assert!(!nested.is_disposed(), "Nested hover should be visible");

    nested.dispose();
    assert!(!env.is_in_dom(&nested), "Nested hover should be removed from DOM after dispose");
    assert!(env.is_in_dom(&parent), "Parent hover should remain in DOM after nested is disposed");

    parent.dispose();
    assert!(!env.is_in_dom(&parent), "Parent hover should be removed from DOM after dispose");
}

#[test]
fn should_dispose_nested_hover_when_parent_is_disposed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let parent = env.show_hover("Parent", None, |_| {});
    let nested = env.create_nested_hover(&parent, "Nested");

    assert!(env.is_in_dom(&parent), "Parent hover should be in DOM");
    assert!(env.is_in_dom(&nested), "Nested hover should be in DOM");

    parent.dispose();

    assert!(nested.is_disposed(), "Nested hover should be disposed when parent is disposed");
    assert!(!env.is_in_dom(&parent), "Parent hover should be removed from DOM");
    assert!(!env.is_in_dom(&nested), "Nested hover should be removed from DOM when parent is disposed");
}

#[test]
fn should_dispose_entire_hover_chain_when_root_is_disposed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let hovers = env.create_hover_chain(3);
    assert_eq!(hovers.len(), 3, "Should create 3 hovers");

    // Verify all hovers are in DOM
    for (index, hover) in hovers.iter().enumerate() {
        assert!(env.is_in_dom(hover), "Hover {} should be in DOM", index + 1);
    }

    // Dispose the root hover
    hovers[0].dispose();

    // All hovers in the chain should be disposed and removed from DOM
    for (index, hover) in hovers.iter().enumerate() {
        assert!(hover.is_disposed(), "Hover {} should be disposed", index + 1);
        assert!(!env.is_in_dom(hover), "Hover {} should be removed from DOM", index + 1);
    }
}

#[test]
fn should_dispose_only_nested_hovers_when_middle_hover_is_disposed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let hovers = env.create_hover_chain(3);
    assert_eq!(hovers.len(), 3, "Should create 3 hovers");

    // Verify all hovers are in DOM
    for hover in &hovers {
        assert!(env.is_in_dom(hover), "All hovers should be in DOM initially");
    }

    // Dispose the middle hover
    hovers[1].dispose();

    assert!(!hovers[0].is_disposed(), "Root hover should remain");
    assert!(env.is_in_dom(&hovers[0]), "Root hover should remain in DOM");

    assert!(hovers[1].is_disposed(), "Middle hover should be disposed");
    assert!(!env.is_in_dom(&hovers[1]), "Middle hover should be removed from DOM");

    assert!(hovers[2].is_disposed(), "Innermost hover should be disposed");
    assert!(!env.is_in_dom(&hovers[2]), "Innermost hover should be removed from DOM");

    hovers[0].dispose();
}

#[test]
fn should_enforce_maximum_nesting_depth() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    // Create hovers up to the max depth (3)
    let hovers = env.create_hover_chain(3);
    assert_eq!(hovers.len(), 3, "Should create exactly 3 hovers (max depth)");

    // Verify all 3 hovers are in DOM
    for hover in &hovers {
        assert!(env.is_in_dom(hover), "Hover should be in DOM");
    }

    // Try to create a 4th nested hover - should fail
    let nested_target = ViewElement::new();
    nested_target.set_parent(Some(hovers[2].dom_node()));
    let fourth_hover = env.service.show_instant_hover(env.options("Hover 4", &nested_target), false, false);

    assert!(fourth_hover.is_none(), "Fourth hover should not be created due to max nesting depth");

    dispose_hovers(&hovers);
}

#[test]
fn should_allow_new_hover_chain_after_disposing_previous_chain() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    // Create and dispose a chain
    let first_chain = env.create_hover_chain(3);
    for hover in &first_chain {
        assert!(env.is_in_dom(hover), "First chain hover should be in DOM");
    }
    dispose_hovers(&first_chain);
    for hover in &first_chain {
        assert!(!env.is_in_dom(hover), "First chain hover should be removed from DOM");
    }

    // Should be able to create a new chain
    let second_chain = env.create_hover_chain(3);
    assert_eq!(second_chain.len(), 3, "Should create new chain after disposing previous");
    for hover in &second_chain {
        assert!(env.is_in_dom(hover), "Second chain hover should be in DOM");
    }

    dispose_hovers(&second_chain);
}

#[test]
fn hide_hover_should_close_innermost_hover_first() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let hovers = env.create_hover_chain(2);

    // Verify both are in DOM
    assert!(env.is_in_dom(&hovers[0]), "Outer hover should be in DOM");
    assert!(env.is_in_dom(&hovers[1]), "Inner hover should be in DOM");

    env.service.hide_hover(false);

    // Innermost hover should be disposed and removed from DOM
    assert!(hovers[1].is_disposed(), "Innermost hover should be disposed");
    assert!(!env.is_in_dom(&hovers[1]), "Innermost hover should be removed from DOM");
    assert!(!hovers[0].is_disposed(), "Outer hover should remain");
    assert!(env.is_in_dom(&hovers[0]), "Outer hover should remain in DOM");

    env.service.hide_hover(false);

    assert!(hovers[0].is_disposed(), "Outer hover should be disposed on second call");
    assert!(!env.is_in_dom(&hovers[0]), "Outer hover should be removed from DOM");
}

// endregion

// region setupDelayedHover

#[test]
fn should_evaluate_function_options_on_mouseover() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        let target = env.create_target();
        let call_count = Rc::new(Cell::new(0));

        let (counter, element) = (call_count.clone(), target.clone());
        let disposable = env.service.setup_delayed_hover(
            &target,
            Rc::new(move || {
                counter.set(counter.get() + 1);
                IHoverOptions::new(HoverContent::Text(format!("Call {}", counter.get())), HoverTarget::Element(element.clone()))
            }),
            None,
        );

        // First mouseover
        env.dom.dispatch(&target, HoverDomEventKind::MouseOver, env.mouse_event());
        assert_eq!(call_count.get(), 1, "Options function should be called on first mouseover");

        timeout(0).await.unwrap();
        env.service.hide_hover(true);

        // Second mouseover should call function again
        env.dom.dispatch(&target, HoverDomEventKind::MouseOver, env.mouse_event());
        assert_eq!(call_count.get(), 2, "Options function should be called on second mouseover");

        timeout(0).await.unwrap();
        disposable.dispose();
        env.service.hide_hover(true);
    });
}

#[test]
fn should_not_call_on_did_hide_when_delayed_hover_is_never_shown() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let target = env.create_target();
    let did_hide_count = Rc::new(Cell::new(0));

    let (count, element) = (did_hide_count.clone(), target.clone());
    let disposable = env.service.setup_delayed_hover(
        &target,
        Rc::new(move || {
            let mut options = IHoverOptions::new(HoverContent::Text("Test".to_owned()), HoverTarget::Element(element.clone()));
            let count = count.clone();
            options.on_did_hide = Some(Rc::new(move || count.set(count.get() + 1)));
            options
        }),
        None,
    );

    disposable.dispose();

    assert_eq!(did_hide_count.get(), 0);
}

#[test]
fn should_use_reduced_delay_when_reduced_delay_is_true_on_setup_delayed_hover() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        let target = env.create_target();

        // Configure reducedDelay to 150ms for this test
        let _ = env.config.set_user_configuration("workbench.hover.reducedDelay", Dynamic::Number(150.0), None);

        let element = target.clone();
        let disposable = env.service.setup_delayed_hover(
            &target,
            Rc::new(move || IHoverOptions::new(HoverContent::Text("Reduced delay".to_owned()), HoverTarget::Element(element.clone()))),
            Some(IHoverLifecycleOptions { reduced_delay: Some(true), ..Default::default() }),
        );

        // Trigger mouseover
        env.dom.dispatch(&target, HoverDomEventKind::MouseOver, env.mouse_event());

        // Hover should not be visible before delay
        timeout(75).await.unwrap();
        assert_eq!(env.hover_count(), 0, "Hover should not be visible before delay completes");

        // Hover should be visible after delay
        timeout(150).await.unwrap();
        assert_eq!(env.hover_count(), 1, "Hover should be visible after reduced delay");

        disposable.dispose();
        env.service.hide_hover(true);
    });
}

#[test]
fn should_not_show_a_pending_hover_after_the_target_was_clicked() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        let target = env.create_target();
        let _ = env.config.set_user_configuration("workbench.hover.delay", Dynamic::Number(500.0), None);

        let element = target.clone();
        let disposable = env.service.setup_delayed_hover(
            &target,
            Rc::new(move || IHoverOptions::new(HoverContent::Text("Manage".to_owned()), HoverTarget::Element(element.clone()))),
            None,
        );
        env.dom.dispatch(&target, HoverDomEventKind::MouseOver, env.mouse_event());
        env.dom.dispatch(&target, HoverDomEventKind::Click, env.mouse_event());

        timeout(500).await.unwrap();
        assert_eq!(env.hover_count(), 0, "Pending hover should be cancelled by the click");

        disposable.dispose();
    });
}

// endregion

// region getStickyHover

#[test]
fn returns_only_a_visible_sticky_hover_for_the_requested_target() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let target = env.create_target();
    let other_target = env.create_target();
    let before_show = env.service.get_sticky_hover(&target);
    let hover = env.show_hover("Test", Some(target.clone()), sticky);
    let while_visible = env.service.get_sticky_hover(&target);
    let for_other_target = env.service.get_sticky_hover(&other_target);
    hover.dispose();

    assert!(before_show.is_none());
    assert!(while_visible.is_some_and(|visible| same(&visible, &hover)));
    assert!(for_other_target.is_none());
    assert!(env.service.get_sticky_hover(&target).is_none());
}

#[test]
fn ignores_passive_previews_even_when_temporarily_locked() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let target = env.create_target();
    let hover = env.show_hover("Test", Some(target.clone()), |_| {});
    hover.set_is_locked(true);

    assert!(env.service.get_sticky_hover(&target).is_none());
    hover.dispose();
}

#[test]
fn matches_each_element_of_a_structured_hover_target() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let first_target = env.create_target();
    let second_target = env.create_target();
    let structured = HoverTarget::Structured(StructuredHoverTarget { target_elements: vec![first_target.clone(), second_target.clone()], x: None, y: None });
    let hover = env.show_hover("Test", None, |options| {
        options.target = structured;
        sticky(options);
    });
    let matches: Vec<bool> = [&first_target, &second_target].iter().map(|target| env.service.get_sticky_hover(target).is_some_and(|found| same(&found, &hover))).collect();
    env.service.hide_hover(true);

    assert_eq!(matches, vec![true, true]);
    assert!([&first_target, &second_target].iter().all(|target| env.service.get_sticky_hover(target).is_none()));
}

// endregion

// region setupManagedHover

#[test]
fn should_use_native_title_attribute_when_show_native_hover_is_true() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let target = env.create_target();
    let hover = env.service.setup_managed_hover(Rc::new(NativeDelegate), &target, "Native hover content".into(), None);

    assert_eq!(env.dom.get_attribute(&target, "title").as_deref(), Some("Native hover content"));

    hover.dispose();

    assert_eq!(env.dom.get_attribute(&target, "title"), None, "Title should be removed on dispose");
}

#[test]
fn should_update_content_dynamically() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let target = env.create_target();
    let hover = env.service.setup_managed_hover(Rc::new(NativeDelegate), &target, "Initial".into(), None);

    assert_eq!(env.dom.get_attribute(&target, "title").as_deref(), Some("Initial"));

    let _ = hover.update(ManagedHoverContentOrFactory::from("Updated"), None);
    assert_eq!(env.dom.get_attribute(&target, "title").as_deref(), Some("Updated"));

    let _ = hover.update(ManagedHoverContentOrFactory::from("Final"), None);
    assert_eq!(env.dom.get_attribute(&target, "title").as_deref(), Some("Final"));

    hover.dispose();
}

// endregion

// region showDelayedHover

#[test]
fn should_reject_hover_when_current_hover_is_locked_and_target_is_outside() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let locked_hover = env.show_hover("Locked", None, sticky);
    assert!(env.is_in_dom(&locked_hover), "Locked hover should be in DOM");

    let other_target = env.create_target();
    let rejected_hover = env.service.show_delayed_hover(env.options("Should not show", &other_target), &IHoverLifecycleOptions::default());

    assert!(rejected_hover.is_none(), "Should reject hover when locked hover exists");
    assert!(env.is_in_dom(&locked_hover), "Locked hover should remain in DOM after rejection");

    locked_hover.dispose();
    assert!(!env.is_in_dom(&locked_hover), "Locked hover should be removed from DOM after dispose");
}

#[test]
fn should_use_reduced_delay_when_reduced_delay_is_true() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        let target = env.create_target();
        let reduced_delay = 100;

        // Configure reducedDelay setting for this test
        let _ = env.config.set_user_configuration("workbench.hover.reducedDelay", Dynamic::Number(reduced_delay as f64), None);

        let hover = env
            .service
            .show_delayed_hover(env.options("Reduced delay hover", &target), &IHoverLifecycleOptions { reduced_delay: Some(true), ..Default::default() })
            .expect("Hover should be created");
        assert!(!env.is_in_dom(&hover), "Hover should not be visible immediately");

        // Wait less than reduced delay - hover should still not be visible
        timeout(reduced_delay / 2).await.unwrap();
        assert!(!env.is_in_dom(&hover), "Hover should not be visible before delay completes");

        // Wait for full delay - hover should now be visible
        timeout(reduced_delay).await.unwrap();
        assert!(env.is_in_dom(&hover), "Hover should be visible after reduced delay");

        hover.dispose();
    });
}

#[test]
fn should_use_default_delay_when_custom_delay_is_undefined() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        let target = env.create_target();
        // Default delay is set to 0 in test setup
        let hover = env.service.show_delayed_hover(env.options("Default delay hover", &target), &IHoverLifecycleOptions::default()).expect("Hover should be created");

        // Since default delay is 0 in tests, hover should appear after minimal timeout
        timeout(0).await.unwrap();
        assert!(env.is_in_dom(&hover), "Hover should be visible with default delay");

        hover.dispose();
    });
}

// endregion

// region hover locking

#[test]
fn is_locked_should_be_settable_on_hover_widget() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let hover = env.show_hover("Test", None, |_| {});
    assert!(env.is_in_dom(&hover), "Hover should be in DOM");

    assert!(!hover.is_locked(), "Should not be locked initially");

    hover.set_is_locked(true);
    assert!(hover.is_locked(), "Should be locked after setting");
    assert!(env.is_in_dom(&hover), "Hover should remain in DOM after locking");

    hover.set_is_locked(false);
    assert!(!hover.is_locked(), "Should be unlocked after unsetting");

    hover.dispose();
    assert!(!env.is_in_dom(&hover), "Hover should be removed from DOM after dispose");
}

#[test]
fn sticky_option_should_set_is_locked_to_true() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let hover = env.show_hover("Test", None, sticky);
    assert!(env.is_in_dom(&hover), "Sticky hover should be in DOM");

    assert!(hover.is_locked(), "Should be locked when sticky");

    hover.dispose();
    assert!(!env.is_in_dom(&hover), "Sticky hover should be removed from DOM after dispose");
}

// endregion

// region showAndFocusLastHover

#[test]
fn should_recreate_last_disposed_hover() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup(&guard);
    let target = env.create_target();
    let hover = env.service.show_instant_hover(env.options("Remember me", &target), false, false).expect("hover");
    assert!(env.is_in_dom(&hover), "Initial hover should be in DOM");

    hover.dispose();
    assert!(!env.is_in_dom(&hover), "Hover should be removed from DOM after dispose");

    // Should recreate the hover - verify a new hover is shown
    env.service.show_and_focus_last_hover();

    // Verify there is a hover in the DOM (it's a new hover instance)
    assert!(env.hover_count() > 0, "A hover should be recreated and in the DOM");

    // Clean up
    env.service.hide_hover(true);

    // Verify cleanup
    assert_eq!(env.hover_count(), 0, "No hovers should remain in DOM after cleanup");
}

// endregion

// region layout and resize

#[test]
fn clicking_outside_should_dismiss_non_sticky_hover() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup(&guard);
        let target = env.create_target();
        let content = ViewElement::new();

        let options = IHoverOptions::new(HoverContent::Element(content), HoverTarget::Element(target));
        let hover = env.service.show_instant_hover(options, false, false).expect("hover");
        assert!(env.is_in_dom(&hover), "Hover should be in DOM");

        // Click outside the hover
        env.dom.dispatch_document(HoverDomEventKind::MouseDown, env.mouse_event());

        assert!(!env.is_in_dom(&hover), "Non-sticky hover should be dismissed after clicking outside");
    });
}

// endregion
