//! Porte de `src/vs/platform/hover/test/browser/hoverService.test.ts` (suíte `HoverService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **DOM.** `document.body.contains(domNode)` é a cadeia de `parent_element` até o `fixture` (`is_ancestor`), que o `render`
//!   do hover cria ao pôr o nó na visão de contexto e a destruição desfaz. `dispatchEvent(new MouseEvent(...))` é
//!   `FakeDom::dispatch` (o evento sobe pela cadeia de pais e termina no `document`) e `document.dispatchEvent` é
//!   `FakeDom::dispatch_document`. `querySelectorAll('.monaco-hover')` é `Env::live_hover_nodes` e `.length` é o número de
//!   hovers vivos na árvore. `element.textContent` é `FakeDom::text_content`, sobre os textos que o teste registra.
//! - **Serviços.** `TestInstantiationService.stub(...)` vira o `HoverServiceDeps`: `TestConfigurationService` com
//!   `workbench.hover.delay` e `reducedDelay` em 0, o `MockKeybindingService`, o `FakeLayoutService` do teste do
//!   `actionWidget` (o `fixture` como todos os contêineres), um `IAccessibilityService` sem leitor de tela e a
//!   `ContextViewService` real, uma por hover (o `new ContextView(container, ABSOLUTE)` do original).
//! - **Dois ambientes.** `setup` monta o serviço com o `FakeHoverWidget`, que guarda `isLocked`, `isDisposed`, as classes
//!   adicionais e o nó na visão de contexto, e basta aos casos que só o `HoverService` responde. `setup_real` monta o
//!   `HoverWidget` real (`HoverWidgetFactory`) e o `ManagedHoverWidget` real (`ManagedHoverWidgetFactory`), para os casos de
//!   geometria, de rastreador de mouse e de hover gerenciável que abre o hover de verdade.
//! - **Medidas do navegador.** O `TestHost` (`IHoverWidgetHost`) é o que o original simula com `getBoundingClientRect` do
//!   alvo (`set_rect`), `Object.defineProperty(domNode, 'clientWidth', ...)` (`set_client_width` e o getter da largura
//!   restrita), a altura do conteúdo rolável (`contents_height`) e `window.innerHeight` (800, a `Viewport` do layout). O resto
//!   (`x`, `max-width`, `max-height`, `top` e `left` do ponteiro) é calculado pelo `HoverWidget` e lido dos estilos dos
//!   `ViewElement`s, como o original lê do DOM. A barra de status das ações soma `STATUS_BAR_HEIGHT` ao que fica fora do
//!   conteúdo, e `statusBarIsVisible` é "o conteúdo limitado mais o que fica fora cabe na altura do hover".
//! - **`WorkbenchHoverDelegate`.** `WorkbenchDelegate` reproduz `showHover` (`hideOnKeyDown`, `id` pelo texto, `compact`, o
//!   `overrideOptions` de `persistence`) e é o `IHoverShower` do `ManagedHoverWidget`. O fechamento por `Escape` no alvo não
//!   existe (nenhum caso o exercita). Como o `IHoverDelegate` da plataforma não carrega `showHover`, a `DelegateRoutingFactory`
//!   entrega à fábrica real o delegate que o `ManagedHoverWidget` recebeu.
//! - **Ações do hover.** Quem desenha as ações é o pintor nativo, então `.hover-row.status-bar .action-container` é a lista de
//!   rótulos que a fábrica real recebeu nas opções de cada hover vivo (`Env::live_action_labels`).
//! - **`sinon.spy(hover, 'layout')`.** O `layout` do `HoverWidget` termina sempre em `IHoverWidgetHost::on_contents_changed`,
//!   e o `TestHost` conta essas chamadas.
//! - **`await hover.update(...)`.** O conteúdo gerenciado é síncrono, então a promessa já volta resolvida.
//! - **`mouseout`.** Nem o serviço nem o widget ouvem `mouseout` (só `mouseleave`), então o evento do caso `mouseleave closes
//!   the rendered hover` não tem efeito e fica só a espera.
//! - **Timers.** `runWithFakedTimers({ useFakeTimers: true }, ...)` é `run_with_faked_timers`.
//!
//! Caso que fica de fora: `should size the hover pointer as a border box so its centre matches the pointer size`, que lê o
//! estilo calculado do `::after` do ponteiro (`box-sizing`, `width`, `height`, `transform-origin` de `hover.css`). O pintor
//! nativo não tem CSS; o que o caso protege (o centro do ponteiro a 3px da borda) é medido por `should centre the hover
//! pointer on its target for odd and even hover widths` e pelo caso vertical, que fixam a meia largura do ponteiro em 3.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use common::keybinding::MockKeybindingService;
use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::timeout;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::layout::{AnchorAlignment, AnchorPosition};
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
    HoverContent, HoverDelay, HoverDelegateTarget, HoverDomEvent, HoverDomEventKind, HoverDomHandler, HoverDomTarget, HoverPlacement, HoverPosition, HoverPositionOption,
    HoverService, HoverServiceDeps, HoverTarget, IHoverAction, IHoverAppearanceOptions, IHoverContextView, IHoverContextViewFactory, IHoverDelegate, IHoverDom,
    IHoverLifecycleOptions, IHoverOptions, IHoverPersistenceOptions, IHoverPositionOptions, IHoverWidget, IHoverWidgetFactory, IHoverWidgetHandle, IManagedHover,
    IManagedHoverOptions, IManagedHoverWidget, IManagedHoverWidgetFactory, ManagedHoverContent, ManagedHoverContentOrFactory, ManagedHoverTooltip,
    StructuredHoverTarget, is_ancestor,
};
use je_platform::hover::hover_widget::{HoverContentMetrics, HoverMeasureRequest, HoverWidget, HoverWidgetFactory, IHoverWidgetHost};
use je_platform::hover::updatable_hover_widget::{IHoverShower, ManagedHoverWidgetFactory};
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
    texts: RefCell<Vec<(Rc<ViewElement>, String)>>,
}

fn key(element: &Rc<ViewElement>) -> usize {
    Rc::as_ptr(element) as usize
}

impl FakeDom {
    fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self { this: this.clone(), next_id: Cell::new(1), listeners: RefCell::new(Vec::new()), attributes: RefCell::new(Vec::new()), active: RefCell::new(None), texts: RefCell::new(Vec::new()) })
    }

    /// `element.textContent = text`.
    fn set_text(&self, element: &Rc<ViewElement>, text: &str) {
        self.texts.borrow_mut().push((element.clone(), text.to_owned()));
    }

    /// `node.textContent`: o texto dos elementos que estão dentro de `node`.
    fn text_content(&self, node: &Rc<ViewElement>) -> String {
        self.texts.borrow().iter().filter(|(element, _)| is_ancestor(element, node)).map(|(_, text)| text.as_str()).collect()
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

/// `window.innerHeight` do teste, a altura da `Viewport` do `FakeLayoutService`.
const WINDOW_HEIGHT: f64 = 800.0;
/// A borda do hover (`HOVER_BORDER_WIDTH`) que o `offsetHeight` do contêiner soma ao conteúdo.
const HOVER_BORDER: f64 = 2.0;
/// A altura da barra de status (`.hover-row.status-bar`) que as ações acrescentam ao que fica fora do conteúdo.
const STATUS_BAR_HEIGHT: f64 = 27.0;

/// `Object.defineProperty(domNode, 'clientWidth', { value })`.
fn fixed_width(value: f64) -> Rc<dyn Fn(Option<f64>) -> f64> {
    Rc::new(move |_| value)
}

/// As medidas do navegador que o original simula: `Object.defineProperty(domNode, 'clientWidth', ...)`, a altura do
/// conteúdo (`scrollHeight`) e a janela. Tudo o mais que o `HoverWidget` lê, ele calcula.
struct TestHost {
    client_width: RefCell<Rc<dyn Fn(Option<f64>) -> f64>>,
    contents_height: Cell<f64>,
    contents_changed: Cell<usize>,
}

impl TestHost {
    fn new() -> Rc<Self> {
        Rc::new(Self { client_width: RefCell::new(fixed_width(200.0)), contents_height: Cell::new(20.0), contents_changed: Cell::new(0) })
    }

    /// `Object.defineProperty(domNode, 'clientWidth', { value })`.
    fn set_client_width(&self, value: f64) {
        *self.client_width.borrow_mut() = fixed_width(value);
    }

    /// `Object.defineProperty(domNode, 'clientWidth', { get })`: a largura depende do `max-width` em vigor.
    fn set_client_width_getter(&self, getter: impl Fn(Option<f64>) -> f64 + 'static) {
        *self.client_width.borrow_mut() = Rc::new(getter);
    }

    /// O que fica fora do conteúdo: a borda e, com ações, a barra de status.
    fn outside_height(has_status_bar: bool) -> f64 {
        HOVER_BORDER + if has_status_bar { STATUS_BAR_HEIGHT } else { 0.0 }
    }
}

impl IHoverWidgetHost for TestHost {
    fn window_inner_height(&self) -> f64 {
        WINDOW_HEIGHT
    }
    fn document_client_width(&self) -> f64 {
        1280.0
    }
    fn document_client_left(&self) -> f64 {
        0.0
    }
    fn zoom_level(&self, _element: &Rc<ViewElement>) -> f64 {
        1.0
    }
    fn measure(&self, request: &HoverMeasureRequest<'_>) -> HoverContentMetrics {
        let client_width = self.client_width.borrow().clone();
        HoverContentMetrics {
            width: client_width(request.max_width),
            contents_height: self.contents_height.get(),
            outside_height: Self::outside_height(request.has_status_bar),
            vertical_scrollbar_size: 10.0,
        }
    }
    fn last_focusable_child(&self, _container: &Rc<ViewElement>) -> Option<Rc<ViewElement>> {
        None
    }
    fn accessible_view_hint(&self) -> Option<String> {
        None
    }
    fn announce(&self, _message: &str) {}
    fn on_contents_changed(&self) {
        self.contents_changed.set(self.contents_changed.get() + 1);
    }
}

/// O que o teste guarda de cada `HoverWidget` real criado: o widget e os rótulos das ações (que o pintor nativo desenha).
struct CreatedHover {
    widget: Rc<HoverWidget>,
    actions: Vec<String>,
}

/// A `HoverWidgetFactory` real, com o registro dos hovers criados.
struct RealHoverWidgets {
    factory: HoverWidgetFactory,
    created: RefCell<Vec<CreatedHover>>,
}

impl IHoverWidgetFactory for RealHoverWidgets {
    fn create(&self, options: &mut IHoverOptions) -> Rc<dyn IHoverWidgetHandle> {
        let widget = self.factory.create_widget(options);
        self.created.borrow_mut().push(CreatedHover { widget: widget.clone(), actions: options.actions.iter().map(|action| action.label.clone()).collect() });
        widget
    }
}

/// O `overrideOptions` do `WorkbenchHoverDelegate` em forma de função: só a parte de `persistence`, que é a que os casos usam.
type PersistenceOverride = Rc<dyn Fn(&IHoverOptions, bool) -> IHoverPersistenceOptions>;

/// `WorkbenchHoverDelegate` (`platform/hover/browser/hover.ts`): o delegate que mostra o hover pelo serviço, com o
/// `hideOnKeyDown`, o `id` pelo texto, o `compact` e o que o chamador sobrescreve. É também o `IHoverShower` do
/// `ManagedHoverWidget`.
struct WorkbenchDelegate {
    service: Weak<HoverService>,
    placement: HoverPlacement,
    overrides: PersistenceOverride,
}

impl IHoverDelegate for WorkbenchDelegate {
    fn delay(&self) -> HoverDelay {
        // `workbench.hover.delay` é 0 no teste e `instantHover` não está ligado.
        HoverDelay::Fixed(0.0)
    }
    fn placement(&self) -> Option<HoverPlacement> {
        Some(self.placement)
    }
}

impl IHoverShower for WorkbenchDelegate {
    fn show_hover(&self, options: IHoverOptions, focus: bool) -> Option<Rc<dyn IHoverWidget>> {
        let service = self.service.upgrade()?;
        let mut options = options;
        let overrides = (self.overrides)(&options, focus);
        options.persistence = Some(IHoverPersistenceOptions { hide_on_key_down: overrides.hide_on_key_down.or(Some(true)), ..overrides });
        options.id = match &options.content {
            HoverContent::Element(_) => None,
            HoverContent::Text(text) => Some(text.clone()),
            HoverContent::Markdown(markdown) => Some(markdown.value.to_string()),
        };
        let mut appearance = options.appearance.unwrap_or_default();
        appearance.compact = Some(true);
        appearance.skip_fade_in_animation = Some(false);
        options.appearance = Some(appearance);
        let hover = service.show_instant_hover(options, focus, false)?;
        Some(hover as Rc<dyn IHoverWidget>)
    }
}

/// `new ManagedHoverWidget(hoverDelegate, target, fadeInAnimation)` pela fábrica real, com o `IHoverShower` do delegate
/// recebido (no original o `showHover` é do próprio delegate).
struct DelegateRoutingFactory {
    delegates: Rc<RefCell<Vec<Rc<WorkbenchDelegate>>>>,
}

impl IManagedHoverWidgetFactory for DelegateRoutingFactory {
    fn create(&self, delegate: &Rc<dyn IHoverDelegate>, target: Rc<HoverDelegateTarget>, fade_in_animation: bool) -> Rc<dyn IManagedHoverWidget> {
        let shower = self
            .delegates
            .borrow()
            .iter()
            .find(|candidate| std::ptr::addr_eq(Rc::as_ptr(*candidate), Rc::as_ptr(delegate)))
            .cloned()
            .expect("o delegate do hover gerenciável foi criado por Env::workbench_delegate");
        ManagedHoverWidgetFactory::new(shower).create(delegate, target, fade_in_animation)
    }
}

struct Env {
    fixture: Rc<ViewElement>,
    dom: Rc<FakeDom>,
    config: Rc<TestConfigurationService>,
    widgets: Rc<FakeHoverWidgetFactory>,
    host: Rc<TestHost>,
    real_widgets: Rc<RealHoverWidgets>,
    delegates: Rc<RefCell<Vec<Rc<WorkbenchDelegate>>>>,
    service: Rc<HoverService>,
}

/// O serviço com o `FakeHoverWidget`, que guarda só o estado que o serviço lê.
fn setup(guard: &LeakGuard) -> Env {
    setup_backend(guard, false)
}

/// O serviço com as fábricas reais: `HoverWidget`, `ManagedHoverWidget` e as medidas do [`TestHost`].
fn setup_real(guard: &LeakGuard) -> Env {
    setup_backend(guard, true)
}

fn setup_backend(guard: &LeakGuard, real: bool) -> Env {
    let fixture = ViewElement::new();
    let layout = Emitter::new();
    guard.add_rc(layout.clone());
    let layout_service: Rc<dyn ILayoutService> = Rc::new(FakeLayoutService { container: fixture.clone(), layout: layout.clone() });
    let config = TestConfigurationService::new(None);
    let _ = config.set_user_configuration("workbench.hover.delay", Dynamic::Number(0.0), None);
    let _ = config.set_user_configuration("workbench.hover.reducedDelay", Dynamic::Number(0.0), None);
    let dom = FakeDom::new();
    let widgets = Rc::new(FakeHoverWidgetFactory { created: RefCell::new(Vec::new()) });
    let host = TestHost::new();
    let real_widgets = Rc::new(RealHoverWidgets { factory: HoverWidgetFactory::new(dom.clone(), host.clone()), created: RefCell::new(Vec::new()) });
    let delegates: Rc<RefCell<Vec<Rc<WorkbenchDelegate>>>> = Rc::new(RefCell::new(Vec::new()));
    let hover_widgets = if real { real_widgets.clone() as Rc<dyn IHoverWidgetFactory> } else { widgets.clone() as Rc<dyn IHoverWidgetFactory> };
    let managed_hover_widgets =
        if real { Rc::new(DelegateRoutingFactory { delegates: delegates.clone() }) as Rc<dyn IManagedHoverWidgetFactory> } else { Rc::new(UnusedManagedHoverWidgetFactory) as Rc<dyn IManagedHoverWidgetFactory> };
    let on_did_show_context_menu = Event::none();
    let keybinding_service: Rc<dyn IKeybindingService> = Rc::new(MockKeybindingService);
    let service = guard.add_rc(HoverService::new(HoverServiceDeps {
        dom: dom.clone(),
        configuration_service: config.clone() as Rc<dyn IConfigurationService>,
        on_did_show_context_menu,
        keybinding_service,
        layout_service: layout_service.clone(),
        accessibility_service: Rc::new(FakeAccessibilityService),
        hover_widgets,
        context_views: Rc::new(ContextViewFactory { layout_service }),
        managed_hover_widgets,
    }));
    Env { fixture, dom, config, widgets, host, real_widgets, delegates, service }
}

impl Env {
    /// `instantiationService.createInstance(WorkbenchHoverDelegate, placement, undefined, overrideOptions)`.
    fn workbench_delegate(&self, placement: HoverPlacement, overrides: PersistenceOverride) -> Rc<dyn IHoverDelegate> {
        let delegate = Rc::new(WorkbenchDelegate { service: Rc::downgrade(&self.service), placement, overrides });
        self.delegates.borrow_mut().push(delegate.clone());
        delegate
    }

    /// O `WorkbenchHoverDelegate` sem `overrideOptions` (`{}`).
    fn element_delegate(&self) -> Rc<dyn IHoverDelegate> {
        self.workbench_delegate(HoverPlacement::Element, Rc::new(|_, _| IHoverPersistenceOptions::default()))
    }

    /// O `HoverWidget` concreto de um hover mostrado pela fábrica real (`hover as HoverWidget`).
    fn widget_of(&self, hover: &Rc<dyn IHoverWidgetHandle>) -> Rc<HoverWidget> {
        let node = hover.dom_node();
        self.real_widgets
            .created
            .borrow()
            .iter()
            .find(|created| Rc::ptr_eq(&created.widget.dom_node(), &node))
            .map(|created| created.widget.clone())
            .expect("o hover foi criado pela fábrica real")
    }

    /// Os nós `.monaco-hover` que estão na árvore do `fixture`, na ordem de criação (`fixture.querySelectorAll`).
    fn live_hover_nodes(&self) -> Vec<Rc<ViewElement>> {
        let fake = self.widgets.created.borrow().iter().map(|widget| widget.node.clone()).collect::<Vec<_>>();
        let real = self.real_widgets.created.borrow().iter().map(|created| created.widget.dom_node()).collect::<Vec<_>>();
        fake.into_iter().chain(real).filter(|node| is_ancestor(node, &self.fixture)).collect()
    }

    /// O texto de cada ação nos hovers vivos (`.monaco-hover .hover-row.status-bar .action-container`).
    fn live_action_labels(&self) -> Vec<String> {
        self.real_widgets
            .created
            .borrow()
            .iter()
            .filter(|created| is_ancestor(&created.widget.dom_node(), &self.fixture))
            .flat_map(|created| created.actions.clone())
            .collect()
    }

    /// `document.body`: um elemento fora de qualquer hover, para o `relatedTarget`.
    fn body(&self) -> Rc<ViewElement> {
        ViewElement::new()
    }

    /// `new FocusEvent(type, { bubbles: true, relatedTarget })` e `new MouseEvent(type, { bubbles: true, relatedTarget })`.
    fn related_event(&self, related_target: Option<Rc<ViewElement>>) -> HoverDomEvent {
        let mut event = HoverDomEvent::new(None);
        event.related_target = related_target;
        event
    }

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
        self.live_hover_nodes().len()
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

    /// `showHoverWithManualLayout()`: texto simples, que não dispara o `ResizeObserver`, e sem esconder ao passar o mouse.
    fn show_hover_with_manual_layout(&self) -> Rc<dyn IHoverWidgetHandle> {
        self.show_hover("Content", None, |options| options.persistence = Some(IHoverPersistenceOptions { hide_on_hover: Some(false), ..Default::default() }))
    }
}

/// `target.getBoundingClientRect = () => new DOMRect(x, y, width, height)`.
fn set_rect(element: &Rc<ViewElement>, x: f64, y: f64, width: f64, height: f64) {
    element.set_position(y, x);
    element.set_size(width, height);
}

/// `Number.parseFloat(element.style.<property>)`.
fn style_px(element: &Rc<ViewElement>, property: &str) -> f64 {
    let value = element.style_get_property(property).unwrap_or_else(|| panic!("o estilo {property} deveria estar definido"));
    value.trim_end_matches("px").parse().unwrap_or_else(|_| panic!("o estilo {property} não é um número em px: {value}"))
}

/// `{ position: { hoverPosition, anchorAlignment, clippingElement }, appearance: { showPointer: true } }`.
fn pointer_hover(position: HoverPosition, anchor_alignment: Option<AnchorAlignment>, clipping_element: Option<Rc<ViewElement>>) -> impl FnOnce(&mut IHoverOptions) {
    move |options| {
        options.position = Some(IHoverPositionOptions { hover_position: Some(HoverPositionOption::Position(position)), anchor_alignment, clipping_element, ..Default::default() });
        options.appearance = Some(IHoverAppearanceOptions { show_pointer: Some(true), ..Default::default() });
    }
}

/// `{ commandId, label, run: () => { } }`.
fn test_action(command_id: &str, label: &str) -> IHoverAction {
    IHoverAction { label: label.to_owned(), command_id: command_id.to_owned(), icon_class: None, run: Rc::new(|_| {}) }
}

/// `{ appearance: { maxHeightRatio: 0.25 }, actions: [{ commandId: 'test.action', ... }] }`.
fn scrollable_hover(options: &mut IHoverOptions) {
    options.appearance = Some(IHoverAppearanceOptions { max_height_ratio: Some(0.25), ..Default::default() });
    options.actions = vec![test_action("test.action", "Test Action")];
}

/// `{ element: (token) => HTMLElement, contentOwnsPadding }`.
fn element_content(create: impl Fn(CancellationToken) -> Rc<ViewElement> + 'static, content_owns_padding: bool) -> ManagedHoverContentOrFactory {
    ManagedHoverContentOrFactory::Content(ManagedHoverContent::Tooltip(ManagedHoverTooltip { element: Some(Rc::new(create)), content_owns_padding, ..Default::default() }))
}

/// `content(text)` do caso do hover fixado: guarda cada `token` recebido e põe o texto no elemento.
fn text_element_content(dom: &Rc<FakeDom>, text: &str, tokens: &Rc<RefCell<Vec<CancellationToken>>>) -> ManagedHoverContentOrFactory {
    let (dom, text, tokens) = (dom.clone(), text.to_owned(), tokens.clone());
    element_content(
        move |token| {
            tokens.borrow_mut().push(token);
            let element = ViewElement::new();
            dom.set_text(&element, &text);
            element
        },
        false,
    )
}

/// `tokens.map(token => token.isCancellationRequested)`.
fn cancelled(tokens: &Rc<RefCell<Vec<CancellationToken>>>) -> Vec<bool> {
    tokens.borrow().iter().map(|token| token.is_cancellation_requested()).collect()
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
fn should_align_the_right_edge_of_a_hover_with_its_target() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup_real(&guard);
    let target = env.create_target();
    set_rect(&target, 300.0, 100.0, 100.0, 20.0);
    let hover = env.show_hover("Right aligned hover", Some(target), pointer_hover(HoverPosition::Below, Some(AnchorAlignment::Right), None));
    env.host.set_client_width(200.0);

    hover.layout();

    assert_eq!(hover.x(), 198.0);
    hover.dispose();
}

#[test]
fn should_centre_the_hover_pointer_on_its_target_for_odd_and_even_hover_widths() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup_real(&guard);
    let target = env.create_target();
    set_rect(&target, 300.0, 100.0, 100.0, 20.0);
    let hover = env.show_hover("Pointer centering", Some(target), pointer_hover(HoverPosition::Below, None, None));
    let pointer = env.widget_of(&hover).hover_pointer().expect("the hover pointer should exist");

    let pointer_origin_for = |client_width: f64| {
        env.host.set_client_width(client_width);
        hover.layout();
        hover.x() + style_px(&pointer, "left")
    };

    // An even width halves exactly, so it pins the caret's own half width.
    let even_origin = pointer_origin_for(200.0);
    let caret_half_width = 350.0 - even_origin;
    let odd_origin = pointer_origin_for(201.0);

    assert_eq!((caret_half_width, even_origin + caret_half_width, odd_origin + caret_half_width), (3.0, 350.0, 350.0));
    hover.dispose();
}

#[test]
fn should_centre_the_hover_pointer_vertically_for_odd_hover_heights() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup_real(&guard);
    let target = env.create_target();
    set_rect(&target, 300.0, 100.0, 100.0, 200.0);
    let hover = env.show_hover("Pointer centering", Some(target), pointer_hover(HoverPosition::Right, None, None));
    let pointer = env.widget_of(&hover).hover_pointer().expect("the hover pointer should exist");

    // O `clientHeight` do contêiner é a altura do conteúdo mais a borda, menos a borda.
    let pointer_top_for = |client_height: f64| {
        env.host.contents_height.set(client_height);
        hover.layout();
        style_px(&pointer, "top")
    };

    let even_top = pointer_top_for(40.0);
    let caret_half_height = 20.0 - even_top;
    let odd_top = pointer_top_for(41.0);

    assert_eq!((caret_half_height, even_top + caret_half_height, odd_top + caret_half_height), (3.0, 20.0, 20.5));
    hover.dispose();
}

#[test]
fn should_position_the_hover_next_to_the_part_of_the_target_visible_within_the_clipping_element() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup_real(&guard);
    let layout_hover_x = |clipping_rect: Option<(f64, f64, f64, f64)>| {
        let target = env.create_target();
        set_rect(&target, 0.0, 100.0, 300.0, 22.0);
        let clipping_element = clipping_rect.map(|(x, y, width, height)| {
            let clipping_element = env.create_target();
            set_rect(&clipping_element, x, y, width, height);
            clipping_element
        });

        let hover = env.show_hover("Clipped target", Some(target), pointer_hover(HoverPosition::Right, None, clipping_element));
        env.host.set_client_width(200.0);
        hover.layout();
        let x = hover.x();
        hover.dispose();
        x
    };

    // The hover sits 3px (the pointer size) to the right of the target's right edge
    let without_clipping_element = layout_hover_x(None);
    let partially_visible = layout_hover_x(Some((0.0, 0.0, 200.0, 400.0)));
    let not_visible = layout_hover_x(Some((400.0, 0.0, 200.0, 400.0)));
    assert_eq!((without_clipping_element, partially_visible, not_visible), (303.0, 203.0, 303.0));
}

#[test]
fn should_constrain_a_right_aligned_hover_to_the_available_width() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup_real(&guard);
    let target = env.create_target();
    set_rect(&target, 100.0, 100.0, 50.0, 20.0);
    let hover = env.show_hover("Constrained right aligned hover", Some(target.clone()), pointer_hover(HoverPosition::Below, Some(AnchorAlignment::Right), None));
    let dom_node = hover.dom_node();
    env.host.set_client_width_getter(|max_width| max_width.filter(|width| *width != 0.0).map_or(200.0, |width| width.min(200.0)));

    hover.layout();
    let constrained_max_width = dom_node.style_get_property("max-width").unwrap_or_default();
    set_rect(&target, 300.0, 100.0, 50.0, 20.0);
    hover.layout();

    assert_eq!((constrained_max_width, dom_node.style_get_property("max-width").unwrap_or_default()), ("146px".to_owned(), String::new()));
    hover.dispose();
}

#[test]
fn should_constrain_scrollable_content_without_clipping_hover_actions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup_real(&guard);
    let hover = env.show_hover("Scrollable hover", None, scrollable_hover);
    let contents = env.widget_of(&hover).contents_dom_node();
    // `overflowingContent.style.height = innerHeight` dentro do conteúdo.
    env.host.contents_height.set(WINDOW_HEIGHT);

    hover.layout();
    let expected_max_height = format!("{}px", WINDOW_HEIGHT * 0.25);
    let hover_height = hover.dom_node().size().height;
    let contents_offset_height = env.host.contents_height.get().min(style_px(&contents, "max-height"));
    let height_outside_contents = hover_height - contents_offset_height;
    // A barra de status fica dentro do hover quando o conteúdo limitado mais o que fica fora dele cabem na altura do hover.
    let status_bar_is_visible = contents_offset_height + TestHost::outside_height(true) <= hover_height;

    assert_eq!(
        (hover.dom_node().style_get_property("max-height").unwrap_or_default(), contents.style_get_property("max-height").unwrap_or_default(), env.host.contents_height.get() > contents_offset_height, status_bar_is_visible),
        (expected_max_height, format!("{}px", (WINDOW_HEIGHT * 0.25 - height_outside_contents).max(0.0)), true, true)
    );
    hover.dispose();
}

#[test]
fn should_not_make_short_hover_content_scrollable() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup_real(&guard);
    let hover = env.show_hover("Short hover", None, scrollable_hover);
    let contents = env.widget_of(&hover).contents_dom_node();
    let contents_offset_height = env.host.contents_height.get().min(style_px(&contents, "max-height"));

    assert_eq!((env.host.contents_height.get() > contents_offset_height, contents.style_get_property("padding-right").unwrap_or_default()), (false, String::new()));
    hover.dispose();
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

#[test]
fn should_apply_additional_classes_to_hover_dom() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = setup_real(&guard);
    let hover = env.show_hover("Test", None, |options| options.additional_classes = vec!["custom-class-1".to_owned(), "custom-class-2".to_owned()]);

    let dom_node = hover.dom_node();
    assert!(env.is_in_dom(&hover), "Hover should be in DOM");
    assert!(dom_node.class_list_contains("custom-class-1"), "Should have custom-class-1");
    assert!(dom_node.class_list_contains("custom-class-2"), "Should have custom-class-2");

    hover.dispose();
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
fn updates_a_pinned_hover_without_losing_its_content_or_focus() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_real(&guard);
        let delegate = env.workbench_delegate(HoverPlacement::Element, Rc::new(|_, focus| IHoverPersistenceOptions { sticky: Some(focus), ..Default::default() }));
        let tokens = Rc::new(RefCell::new(Vec::new()));
        let hover = env.service.setup_managed_hover(delegate, &env.create_target(), text_element_content(&env.dom, "Initial", &tokens), None);
        hover.show(true);
        timeout(0).await.unwrap();
        // `await hover.update(...)`: o conteúdo é síncrono, a promessa já volta resolvida.
        let _ = hover.update(text_element_content(&env.dom, "Updated", &tokens), None);
        let element = env.live_hover_nodes().into_iter().next().expect("the replacement hover should be in the DOM");
        let updated = (env.dom.text_content(&element), env.dom.is_ancestor_of_active_element(&element), cancelled(&tokens));
        hover.hide();

        assert_eq!((updated, cancelled(&tokens)), (("Updated".to_owned(), true, vec![true, false]), vec![true, true]));
        hover.dispose();
    });
}

#[test]
fn forwards_on_did_show_only_for_a_displayed_managed_hover() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_real(&guard);
        let calls = Rc::new(RefCell::new(Vec::<&str>::new()));
        let sink = calls.clone();
        let options = IManagedHoverOptions { on_did_show: Some(Rc::new(move || sink.borrow_mut().push("show"))), ..Default::default() };
        let hover = env.service.setup_managed_hover(env.element_delegate(), &env.create_target(), "Promo".into(), Some(options));
        let blocker = env.show_hover("Locked", None, sticky);
        hover.show(false);
        timeout(0).await.unwrap();
        let blocked = calls.borrow().clone();
        blocker.dispose();
        hover.show(false);
        timeout(0).await.unwrap();
        hover.hide();

        assert_eq!((blocked, calls.borrow().clone()), (vec![], vec!["show"]));
        hover.dispose();
    });
}

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

#[test]
fn should_update_options_dynamically() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_real(&guard);
        let target = env.create_target();
        let with_action = |command_id: &str, label: &str| IManagedHoverOptions { actions: vec![test_action(command_id, label)], ..Default::default() };
        let hover = env.service.setup_managed_hover(env.element_delegate(), &target, "Test".into(), Some(with_action("test.first", "First")));

        let _ = hover.update("Test".into(), Some(with_action("test.second", "Second")));

        env.dom.dispatch(&target, HoverDomEventKind::Focus, env.related_event(Some(env.body())));
        timeout(500).await.unwrap();

        assert_eq!(env.live_action_labels(), vec!["Second".to_owned()]);
        hover.dispose();
    });
}

#[test]
fn should_let_managed_html_content_own_the_hover_boundary_padding() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_real(&guard);
        let target = env.create_target();
        let hover = env.service.setup_managed_hover(env.element_delegate(), &target, element_content(|_| ViewElement::new(), true), None);

        env.dom.dispatch(&target, HoverDomEventKind::Focus, env.related_event(Some(env.body())));
        timeout(500).await.unwrap();

        let node = env.live_hover_nodes().into_iter().next().expect("the managed hover should be in the DOM");
        assert_eq!((node.class_list_contains("compact"), node.class_list_contains("managed-hover-content-owns-padding")), (true, true));
        hover.dispose();
    });
}

#[test]
fn should_cancel_managed_html_content_when_mouseleave_closes_the_rendered_hover() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_real(&guard);
        let target = env.create_target();
        let tokens = Rc::new(RefCell::new(Vec::new()));
        let sink = tokens.clone();
        let hover = env.service.setup_managed_hover(
            env.element_delegate(),
            &target,
            element_content(
                move |token| {
                    sink.borrow_mut().push(token);
                    ViewElement::new()
                },
                false,
            ),
            None,
        );

        env.dom.dispatch(&target, HoverDomEventKind::MouseOver, env.related_event(Some(env.body())));
        timeout(0).await.unwrap();
        let before_hide = cancelled(&tokens);
        // `new MouseEvent('mouseout', ...)`: nem o serviço nem o widget ouvem `mouseout` (só `mouseleave`), então o evento
        // não tem efeito e só a espera do `timeout(0)` do original permanece.
        timeout(0).await.unwrap();
        let after_mouse_leave = cancelled(&tokens);
        env.service.hide_hover(true);

        assert_eq!((before_hide, after_mouse_leave, cancelled(&tokens)), (vec![false], vec![false], vec![true]));
        hover.dispose();
    });
}

#[test]
fn should_focus_an_already_visible_managed_hover_when_shown_explicitly() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_real(&guard);
        let target = env.create_target();
        let hover = env.service.setup_managed_hover(env.element_delegate(), &target, "Test".into(), None);
        env.dom.dispatch(&target, HoverDomEventKind::MouseOver, env.related_event(Some(env.body())));
        timeout(0).await.unwrap();
        let first_hover = env.live_hover_nodes().first().cloned();

        env.service.show_managed_hover(&target);
        timeout(0).await.unwrap();
        let live = env.live_hover_nodes();
        let focused_hover = live.first().cloned();

        let recreated = !matches!((&first_hover, &focused_hover), (Some(first), Some(focused)) if Rc::ptr_eq(first, focused));
        let focused = focused_hover.as_ref().is_some_and(|node| env.dom.is_ancestor_of_active_element(node));
        assert_eq!((first_hover.is_some(), recreated, focused, live.len()), (true, true, true, 1));
        hover.dispose();
    });
}

#[test]
fn should_retain_replacement_managed_html_content_when_updating_a_visible_hover() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_real(&guard);
        let target = env.create_target();
        let tokens = Rc::new(RefCell::new(Vec::new()));
        let content = || {
            let sink = tokens.clone();
            element_content(
                move |token| {
                    sink.borrow_mut().push(token);
                    ViewElement::new()
                },
                false,
            )
        };
        let hover = env.service.setup_managed_hover(env.element_delegate(), &target, content(), None);
        env.dom.dispatch(&target, HoverDomEventKind::MouseOver, env.related_event(Some(env.body())));
        timeout(0).await.unwrap();

        let _ = hover.update(content(), None);

        assert_eq!(cancelled(&tokens), vec![true, false]);
        hover.dispose();
    });
}

#[test]
fn should_not_re_show_hover_on_focus_when_related_target_is_from_a_dismissed_hover() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_real(&guard);
        let target = env.create_target();
        let hover = env.service.setup_managed_hover(env.element_delegate(), &target, "Test".into(), None);

        // Show hover explicitly
        env.dom.dispatch(&target, HoverDomEventKind::Focus, env.related_event(Some(env.body())));
        timeout(500).await.unwrap();
        assert!(env.hover_count() > 0, "Hover should be visible after focus");

        // Dismiss via hoverService (simulates Esc / external dismissal)
        env.service.hide_hover(true);
        timeout(0).await.unwrap();

        // Simulate focus returning from the hover element
        let hover_element = ViewElement::with_classes(&["monaco-hover"]);
        env.dom.dispatch(&target, HoverDomEventKind::Focus, env.related_event(Some(hover_element)));
        timeout(500).await.unwrap();

        assert_eq!(env.hover_count(), 0, "Hover should not re-show when focus comes from dismissed hover");
        hover.dispose();
    });
}

#[test]
fn should_not_re_show_hover_on_focus_when_related_target_is_null_window_reactivation() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_real(&guard);
        let target = env.create_target();
        let hover = env.service.setup_managed_hover(env.element_delegate(), &target, "Test".into(), None);

        // Show hover via focus and dismiss externally
        env.dom.dispatch(&target, HoverDomEventKind::Focus, env.related_event(Some(env.body())));
        timeout(500).await.unwrap();
        env.service.hide_hover(true);
        timeout(0).await.unwrap();

        // Simulate focus from window reactivation (relatedTarget is null)
        env.dom.dispatch(&target, HoverDomEventKind::Focus, env.related_event(None));
        timeout(500).await.unwrap();

        assert_eq!(env.hover_count(), 0, "Hover should not re-show on window reactivation");
        hover.dispose();
    });
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
fn layout_should_suppress_pending_mouseout_so_content_resize_does_not_dismiss_hover() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_real(&guard);
        let hover = env.show_hover_with_manual_layout();
        assert!(env.is_in_dom(&hover), "Hover should be in DOM");
        let hover_container = hover.dom_node().parent_element().expect("the hover container should exist");

        // Simulate a mouseleave on the hover container (as happens when content shrinks)
        env.dom.dispatch(&hover_container, HoverDomEventKind::MouseLeave, env.mouse_event());

        // Before the debounce timer fires, trigger a layout (as ResizeObserver would)
        hover.layout();

        timeout(200).await.unwrap();

        assert!(env.is_in_dom(&hover), "Hover should remain in DOM after layout suppresses mouseout");

        hover.dispose();
        assert!(!env.is_in_dom(&hover), "Hover should be removed from DOM after dispose");
    });
}

#[test]
fn hover_should_still_dismiss_on_mouseout_when_no_layout_occurs() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_real(&guard);
        let hover = env.show_hover_with_manual_layout();
        assert!(env.is_in_dom(&hover), "Hover should be in DOM");
        let hover_container = hover.dom_node().parent_element().expect("the hover container should exist");
        // `sinon.spy(hover, 'layout')`: o `layout` termina sempre em `on_contents_changed` do host, que é o contador.
        let layout_calls_before = env.host.contents_changed.get();

        env.dom.dispatch(&hover_container, HoverDomEventKind::MouseLeave, env.mouse_event());

        timeout(199).await.unwrap();
        assert!(env.is_in_dom(&hover), "Hover should remain in DOM until the mouseout debounce fires");
        timeout(1).await.unwrap();

        assert_eq!(
            (env.host.contents_changed.get() - layout_calls_before, hover.is_mouse_in(), hover.is_disposed(), env.is_in_dom(&hover)),
            (0, false, true, false)
        );
    });
}

#[test]
fn suppression_clears_after_mouse_re_enters_and_a_new_mouseleave_dismisses_normally() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = setup_real(&guard);
        let hover = env.show_hover_with_manual_layout();
        assert!(env.is_in_dom(&hover), "Hover should be in DOM");
        let hover_container = hover.dom_node().parent_element().expect("the hover container should exist");

        // Simulate mouseleave + layout to suppress
        env.dom.dispatch(&hover_container, HoverDomEventKind::MouseLeave, env.mouse_event());
        hover.layout();
        timeout(200).await.unwrap();
        assert!(env.is_in_dom(&hover), "Hover should remain after suppressed mouseout");

        // Mouse re-enters, clearing the suppression flag
        env.dom.dispatch(&hover_container, HoverDomEventKind::MouseOver, env.mouse_event());

        // Mouse leaves again, this time no layout, so it should dismiss
        env.dom.dispatch(&hover_container, HoverDomEventKind::MouseLeave, env.mouse_event());
        timeout(200).await.unwrap();

        assert!(!env.is_in_dom(&hover), "Hover should dismiss on normal mouseout after suppression was cleared");
    });
}

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
