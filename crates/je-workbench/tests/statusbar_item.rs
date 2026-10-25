//! Porte de `src/vs/workbench/test/browser/parts/statusbar/statusbarItem.test.ts` (suíte `StatusbarEntryItem`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **DOM.** O `fixture`, o `otherTarget` (`button`) e o `container` (`.statusbar-item`) são `ViewElement`s encadeados
//!   por `set_parent`. `dispatchEvent` é `FakeDom::dispatch`, com as fases do DOM (captura do `document` até o alvo, o
//!   alvo, e a subida só quando o evento borbulha); `element.focus()` é `FakeDom::focus`, que troca o elemento ativo e
//!   dispara `blur` no anterior e `focus` (sem subir) no novo, com o `relatedTarget`. `new PointerEvent`, `MouseEvent`,
//!   `KeyboardEvent`, `FocusEvent` e o `CustomEvent` de `Tap` são o [`DomEvent`] com os campos que o item e o
//!   `HoverService` leem. `keyCode: 13`, `32` e `27` são `KeyCode::Enter`, `Space` e `Escape` (o item lê o `keyCode` já na
//!   codificação do `StandardKeyboardEvent`). `HTMLElement.click()` é um `click` com `detail` 0.
//! - **Hover.** `HoverService`, `HoverWidget` e `ManagedHoverWidget` são os reais, montados como em
//!   `je-platform/tests/hover_service.rs`; o `WorkbenchHoverDelegate` do original, com `dynamicDelay: () => 500` e o
//!   `overrideOptions` `{ persistence: { hideOnKeyDown: true, sticky: focus } }`, é o `WorkbenchDelegate` deste arquivo.
//!   `fixture.querySelector('.monaco-hover')` é o primeiro hover vivo na árvore do `fixture`, e `isFocused` é o elemento
//!   ativo estar dentro dele.
//! - **Serviços.** O `workbenchInstantiationService` vira os serviços reais de notificação e dublês de comando
//!   (`executeCommand` guarda `{ id, args }`), telemetria e tema.
//! - **Conteúdo da dica.** `$('div', undefined, 'Dashboard')` é um `ViewElement` (o porte não modela o texto do nó).
//! - **Timers.** `runWithFakedTimers({ useFakeTimers: true }, ...)` é `run_with_faked_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::timeout;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::key_codes::KeyCode;
use je_base::keybindings::{KeyCodeChord, Keybinding, ResolvedKeybinding};
use je_base::lifecycle::{DisposableRef, none, plain_disposable, to_disposable};
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_editor::code_action::languages::Command;
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_instantiation::Injectable;
use je_platform::contextview::context_view::IContextViewService;
use je_base::lifecycle::IDisposable as _;
use je_platform::accessibility::{AccessibilitySupport, IAccessibilityService};
use je_platform::commands::commands::{CommandEvent, ICommandService};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::{IContextKeyService, IContextKeyServiceTarget};
use je_platform::contextview::context_view::IContextViewDelegate;
use je_platform::contextview::context_view_service::ContextViewService;
use je_platform::contextview::view_element::ViewElement;
use je_platform::hover::hover_service::{
    HoverContent, HoverDelay, HoverDelegateTarget, HoverDomEvent, HoverDomEventKind, HoverDomHandler, HoverDomTarget, HoverPlacement, HoverService, HoverServiceDeps, IHoverContextView,
    IHoverContextViewFactory, IHoverDelegate, IHoverDom, IHoverOptions, IHoverPersistenceOptions, IHoverWidget, IHoverWidgetFactory, IHoverWidgetHandle, IManagedHoverWidget,
    IManagedHoverWidgetFactory, ManagedHoverTooltip, is_ancestor,
};
use je_platform::hover::hover_widget::{HoverContentMetrics, HoverMeasureRequest, HoverWidget, HoverWidgetFactory, IHoverWidgetHost};
use je_platform::hover::updatable_hover_widget::{IHoverShower, ManagedHoverWidgetFactory};
use je_platform::keybinding::keybinding::{IKeybindingService, KeybindingsSchemaContribution, KeyboardEvent};
use je_platform::keybinding::keybinding_resolver::ResolutionResult;
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use je_platform::keybinding::us_layout_resolved_keybinding::USLayoutResolvedKeybinding;
use je_platform::layout::layout_service::{ILayoutService, LayoutContainerEvent, Viewport};
use je_platform::notification::INotificationService;
use je_platform::storage::StorageService;
use je_platform::storage::storage::InMemoryStorageService;
use je_platform::telemetry::ITelemetryService;
use je_theme::color::Color;
use je_workbench::browser::parts::statusbar::statusbar_item::{
    StatusbarEntryItem, StatusbarItemEvent, StatusbarItemEventKind, StatusbarItemServices, StatusbarItemView, StatusbarProgressIcon,
};
use je_workbench::services::notification::common::notification_service::NotificationService;
use je_workbench::services::statusbar::statusbar::{StatusbarCommand, StatusbarEntry, StatusbarTooltip, TooltipContent};
use je_workbench::test::common::test_notification_telemetry::TestNotificationTelemetryService;

// region Serviços

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

/// O `MockKeybindingService`: sem atalho nenhum, resolve pelo layout US.
struct MockKeybindingService;

impl IKeybindingService for MockKeybindingService {
    fn in_chord_mode(&self) -> bool {
        false
    }
    fn on_did_update_keybindings(&self) -> Event<()> {
        Event::none()
    }
    fn resolve_keybinding(&self, keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>> {
        USLayoutResolvedKeybinding::resolve_keybinding(keybinding, Platform::detect().os)
    }
    fn resolve_keyboard_event(&self, e: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding> {
        let chord = KeyCodeChord::new(e.ctrl_key, e.shift_key, e.alt_key, e.meta_key, e.key_code);
        self.resolve_keybinding(&chord.to_keybinding()).remove(0)
    }
    fn resolve_user_binding(&self, _user_binding: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        Vec::new()
    }
    fn dispatch_event(&self, _e: &KeyboardEvent, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> bool {
        false
    }
    fn soft_dispatch(&self, _keyboard_event: &KeyboardEvent, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> ResolutionResult {
        ResolutionResult::NoMatchingKb
    }
    fn enable_keybinding_hold_mode(&self, _command_id: &str) -> Option<Promise<()>> {
        None
    }
    fn dispatch_by_user_settings_label(&self, _user_settings_label: &str, _target: Option<Rc<dyn IContextKeyServiceTarget>>) {}
    fn lookup_keybindings(&self, _command_id: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        Vec::new()
    }
    fn lookup_keybinding(&self, _command_id: &str, _context: Option<&dyn IContextKeyService>, _enforce_context_check: bool) -> Option<Rc<dyn ResolvedKeybinding>> {
        None
    }
    fn get_default_keybindings_content(&self) -> String {
        String::new()
    }
    fn get_default_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        Vec::new()
    }
    fn get_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        Vec::new()
    }
    fn custom_keybindings_count(&self) -> usize {
        0
    }
    fn might_produce_printable_character(&self, _event: &KeyboardEvent) -> bool {
        false
    }
    fn register_schema_contribution(&self, _contribution: Rc<dyn KeybindingsSchemaContribution>) -> DisposableRef {
        plain_disposable(|| {})
    }
    fn toggle_logging(&self) -> bool {
        false
    }
    fn append_keybinding(&self, label: &str, _command_id: Option<&str>, _context: Option<&dyn IContextKeyService>, _enforce_context_check: bool) -> String {
        label.to_owned()
    }
    fn dump_debug_info(&self) -> String {
        String::new()
    }
    fn dump_debug_info_json(&self) -> String {
        String::new()
    }
}

/// `{ executeCommand: async (id, ...args) => { commands.push({ id, args }); } }`.
struct RecordingCommandService {
    commands: Rc<RefCell<Vec<(String, Vec<String>)>>>,
}

impl ICommandService for RecordingCommandService {
    fn on_will_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn on_did_execute_command(&self) -> Event<CommandEvent> {
        Event::none()
    }
    fn execute_command(&self, command_id: &str, args: Vec<Dynamic>) -> Promise<Dynamic> {
        let args = args
            .iter()
            .map(|arg| match arg {
                Dynamic::String(text) => text.to_string_lossy(),
                other => panic!("argumento inesperado: {other:?}"),
            })
            .collect();
        self.commands.borrow_mut().push((command_id.to_owned(), args));
        Promise::resolved(Dynamic::Undefined)
    }
}

struct EmptyColorTheme;

impl IColorTheme for EmptyColorTheme {
    fn get_color(&self, _color: &str, _use_default: Option<bool>) -> Option<Color> {
        None
    }
}

struct EmptyThemeService;

impl IThemeService for EmptyThemeService {
    fn get_color_theme(&self) -> Rc<dyn IColorTheme> {
        Rc::new(EmptyColorTheme)
    }
    fn on_did_color_theme_change(&self) -> Event<Rc<dyn IColorTheme>> {
        Event::none()
    }
}

// endregion

// region DOM

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    PointerDown,
    PointerUp,
    PointerCancel,
    MouseDown,
    MouseUp,
    MouseOver,
    MouseLeave,
    MouseMove,
    Click,
    KeyDown,
    KeyUp,
    Focus,
    Blur,
    Tap,
    ContextMenu,
    Resize,
}

/// `PointerEvent`, `MouseEvent`, `KeyboardEvent`, `FocusEvent` e o `CustomEvent` de `Tap`.
#[derive(Clone)]
struct DomEvent {
    kind: Kind,
    target: Option<Rc<ViewElement>>,
    related_target: Option<Rc<ViewElement>>,
    bubbles: bool,
    button: i32,
    pointer_id: Option<i32>,
    detail: i32,
    key_code: KeyCode,
    key: String,
}

impl DomEvent {
    fn new(kind: Kind) -> Self {
        Self { kind, target: None, related_target: None, bubbles: false, button: 0, pointer_id: None, detail: 0, key_code: KeyCode::Unknown, key: String::new() }
    }

    fn bubbling(mut self) -> Self {
        self.bubbles = true;
        self
    }

    fn button(mut self, button: i32) -> Self {
        self.button = button;
        self
    }

    /// `{ pointerId, pointerType: 'mouse' }`.
    fn pointer(mut self, pointer_id: i32) -> Self {
        self.pointer_id = Some(pointer_id);
        self
    }

    fn detail(mut self, detail: i32) -> Self {
        self.detail = detail;
        self
    }

    fn related(mut self, related_target: &Rc<ViewElement>) -> Self {
        self.related_target = Some(related_target.clone());
        self
    }

    /// `new KeyboardEvent('keydown', { keyCode, bubbles: true, cancelable: true })`.
    fn key_down(key_code: KeyCode, key: &str) -> Self {
        let mut event = Self::new(Kind::KeyDown).bubbling();
        event.key_code = key_code;
        event.key = key.to_owned();
        event
    }

    fn to_hover(&self) -> HoverDomEvent {
        let mut event = HoverDomEvent::new(self.target.clone());
        event.related_target = self.related_target.clone();
        event.key = self.key.clone();
        event.key_code = self.key_code;
        event
    }

    fn to_item(&self) -> StatusbarItemEvent {
        let mut event = StatusbarItemEvent::new();
        event.button = self.button;
        event.pointer_id = self.pointer_id;
        event.detail = self.detail;
        event.key_code = self.key_code;
        event
    }
}

type Handler = Rc<dyn Fn(&DomEvent)>;

struct Listener {
    id: u64,
    target: HoverDomTarget,
    kind: Kind,
    capture: bool,
    handler: Handler,
}

/// O DOM do teste: ouvintes por elemento e `document`, atributos e o elemento ativo.
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

fn hover_kind(kind: HoverDomEventKind) -> Kind {
    match kind {
        HoverDomEventKind::MouseOver => Kind::MouseOver,
        HoverDomEventKind::MouseDown => Kind::MouseDown,
        HoverDomEventKind::MouseUp => Kind::MouseUp,
        HoverDomEventKind::MouseLeave => Kind::MouseLeave,
        HoverDomEventKind::MouseMove => Kind::MouseMove,
        HoverDomEventKind::Click => Kind::Click,
        HoverDomEventKind::KeyDown => Kind::KeyDown,
        HoverDomEventKind::KeyUp => Kind::KeyUp,
        HoverDomEventKind::Focus => Kind::Focus,
        HoverDomEventKind::Blur => Kind::Blur,
        HoverDomEventKind::Resize => Kind::Resize,
    }
}

fn item_kind(kind: StatusbarItemEventKind) -> Kind {
    match kind {
        StatusbarItemEventKind::PointerDown => Kind::PointerDown,
        StatusbarItemEventKind::Click => Kind::Click,
        StatusbarItemEventKind::Tap => Kind::Tap,
        StatusbarItemEventKind::KeyDown => Kind::KeyDown,
    }
}

impl FakeDom {
    fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self { this: this.clone(), next_id: Cell::new(1), listeners: RefCell::new(Vec::new()), attributes: RefCell::new(Vec::new()), active: RefCell::new(None) })
    }

    fn add(&self, target: HoverDomTarget, kind: Kind, capture: bool, handler: Handler) -> DisposableRef {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        self.listeners.borrow_mut().push(Listener { id, target, kind, capture, handler });
        let dom = self.this.clone();
        to_disposable(move || {
            if let Some(dom) = dom.upgrade() {
                dom.listeners.borrow_mut().retain(|listener| listener.id != id);
            }
        })
    }

    /// Os ouvintes de um passo do caminho (`None` é o `document`) e fase, na ordem de registro.
    fn fire(&self, step: &Option<Rc<ViewElement>>, capture: bool, event: &DomEvent) {
        let snapshot: Vec<(u64, Handler)> = self
            .listeners
            .borrow()
            .iter()
            .filter(|listener| {
                listener.kind == event.kind
                    && listener.capture == capture
                    && match (&listener.target, step) {
                        (HoverDomTarget::Element(element), Some(step)) => Rc::ptr_eq(element, step),
                        (HoverDomTarget::Document, None) => true,
                        _ => false,
                    }
            })
            .map(|listener| (listener.id, listener.handler.clone()))
            .collect();
        for (id, handler) in snapshot {
            if self.listeners.borrow().iter().any(|listener| listener.id == id) {
                handler(event);
            }
        }
    }

    /// `target.dispatchEvent(event)`: captura do `document` até o pai, o alvo e, se o evento borbulha, a subida.
    fn dispatch(&self, target: &Rc<ViewElement>, mut event: DomEvent) {
        event.target = Some(target.clone());
        let mut path: Vec<Option<Rc<ViewElement>>> = Vec::new();
        let mut candidate = Some(target.clone());
        while let Some(element) = candidate {
            candidate = element.parent_element();
            path.push(Some(element));
        }
        path.push(None);
        for step in path.iter().skip(1).rev() {
            self.fire(step, true, &event);
        }
        self.fire(&path[0], true, &event);
        self.fire(&path[0], false, &event);
        if event.bubbles {
            for step in path.iter().skip(1) {
                self.fire(step, false, &event);
            }
        }
    }

    /// `element.focus()`.
    fn focus_element(&self, element: &Rc<ViewElement>) {
        let previous = self.active.borrow().clone();
        if previous.as_ref().is_some_and(|previous| Rc::ptr_eq(previous, element)) {
            return;
        }
        *self.active.borrow_mut() = Some(element.clone());
        if let Some(previous) = &previous {
            self.dispatch(previous, DomEvent::new(Kind::Blur).related(element));
        }
        let mut focus = DomEvent::new(Kind::Focus);
        focus.related_target = previous;
        self.dispatch(element, focus);
    }
}

impl IHoverDom for FakeDom {
    fn add_listener(&self, target: HoverDomTarget, kind: HoverDomEventKind, capture: bool, handler: HoverDomHandler) -> DisposableRef {
        self.add(target, hover_kind(kind), capture, Rc::new(move |event| handler(&event.to_hover())))
    }

    fn active_element(&self) -> Option<Rc<ViewElement>> {
        self.active.borrow().clone()
    }

    fn is_ancestor_of_active_element(&self, element: &Rc<ViewElement>) -> bool {
        self.active.borrow().as_ref().is_some_and(|active| is_ancestor(active, element))
    }

    fn focus(&self, element: &Rc<ViewElement>) {
        self.focus_element(element);
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

/// O que o item faz com o DOM, sobre o [`FakeDom`].
struct ItemView {
    dom: Rc<FakeDom>,
}

impl StatusbarItemView for ItemView {
    fn add_listener(&self, element: &Rc<ViewElement>, kind: StatusbarItemEventKind, handler: Rc<dyn Fn(&StatusbarItemEvent)>) -> DisposableRef {
        self.dom.add(HoverDomTarget::Element(element.clone()), item_kind(kind), false, Rc::new(move |event| handler(&event.to_item())))
    }

    fn add_touch_target(&self, _element: &Rc<ViewElement>) -> DisposableRef {
        none()
    }

    fn set_attribute(&self, element: &Rc<ViewElement>, name: &str, value: &str) {
        IHoverDom::set_attribute(self.dom.as_ref(), element, name, value);
    }

    fn append_content(&self, container: &Rc<ViewElement>, content: &Rc<ViewElement>) {
        content.set_parent(Some(container.clone()));
    }

    fn render_label(&self, _label_container: &Rc<ViewElement>, _progress: Option<StatusbarProgressIcon>, _text: &str) {}
}

// endregion

// region Hover

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

/// As medidas do navegador que o `HoverWidget` não calcula.
struct TestHost;

impl IHoverWidgetHost for TestHost {
    fn window_inner_height(&self) -> f64 {
        800.0
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
    fn measure(&self, _request: &HoverMeasureRequest<'_>) -> HoverContentMetrics {
        HoverContentMetrics { width: 200.0, contents_height: 20.0, outside_height: 2.0, vertical_scrollbar_size: 10.0 }
    }
    fn last_focusable_child(&self, _container: &Rc<ViewElement>) -> Option<Rc<ViewElement>> {
        None
    }
    fn accessible_view_hint(&self) -> Option<String> {
        None
    }
    fn announce(&self, _message: &str) {}
    fn on_contents_changed(&self) {}
}

/// A `HoverWidgetFactory` real, com o registro dos hovers criados.
struct RealHoverWidgets {
    factory: HoverWidgetFactory,
    created: RefCell<Vec<Rc<HoverWidget>>>,
}

impl IHoverWidgetFactory for RealHoverWidgets {
    fn create(&self, options: &mut IHoverOptions) -> Rc<dyn IHoverWidgetHandle> {
        let widget = self.factory.create_widget(options);
        self.created.borrow_mut().push(widget.clone());
        widget
    }
}

/// `WorkbenchHoverDelegate` com `dynamicDelay: () => 500` e `overrideOptions: (_, focus) => ({ persistence: { hideOnKeyDown: true, sticky: focus } })`.
struct WorkbenchDelegate {
    service: Weak<HoverService>,
}

impl IHoverDelegate for WorkbenchDelegate {
    fn delay(&self) -> HoverDelay {
        HoverDelay::Fixed(500.0)
    }
    fn placement(&self) -> Option<HoverPlacement> {
        Some(HoverPlacement::Element)
    }
}

impl IHoverShower for WorkbenchDelegate {
    fn show_hover(&self, options: IHoverOptions, focus: bool) -> Option<Rc<dyn IHoverWidget>> {
        let service = self.service.upgrade()?;
        let mut options = options;
        options.persistence = Some(IHoverPersistenceOptions { hide_on_key_down: Some(true), sticky: Some(focus), hide_on_hover: None });
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
            .expect("o delegate do hover gerenciável foi criado pelo teste");
        ManagedHoverWidgetFactory::new(shower).create(delegate, target, fade_in_animation)
    }
}

// endregion

/// O que `createItem` devolve.
struct Fixture {
    item: Rc<StatusbarEntryItem>,
    entry: StatusbarEntry,
    container: Rc<ViewElement>,
    other_target: Rc<ViewElement>,
    fixture: Rc<ViewElement>,
    dom: Rc<FakeDom>,
    widgets: Rc<RealHoverWidgets>,
    commands: Rc<RefCell<Vec<(String, Vec<String>)>>>,
}

impl Fixture {
    fn label(&self) -> Rc<ViewElement> {
        self.item.label_container.clone()
    }

    /// `!!fixture.querySelector('.monaco-hover')`.
    fn live_hovers(&self) -> Vec<Rc<ViewElement>> {
        self.widgets.created.borrow().iter().map(|widget| widget.dom_node()).filter(|node| is_ancestor(node, &self.fixture)).collect()
    }

    fn is_visible(&self) -> bool {
        !self.live_hovers().is_empty()
    }

    /// `fixture.querySelector('.monaco-hover')?.contains(document.activeElement)`.
    fn is_focused(&self) -> bool {
        match self.live_hovers().first() {
            Some(hover) => self.dom.active.borrow().as_ref().is_some_and(|active| is_ancestor(active, hover)),
            None => false,
        }
    }

    async fn click(&self) {
        let label = self.label();
        self.dom.dispatch(&label, DomEvent::new(Kind::PointerDown).bubbling().pointer(1));
        self.dom.dispatch(&label, DomEvent::new(Kind::MouseDown).bubbling());
        self.dom.focus_element(&label);
        self.dom.dispatch(&label, DomEvent::new(Kind::PointerUp).bubbling().pointer(1));
        self.dom.dispatch(&label, DomEvent::new(Kind::MouseUp).bubbling());
        self.dom.dispatch(&label, DomEvent::new(Kind::Click).bubbling().pointer(1).detail(1));
        timeout(0).await.unwrap();
    }

    /// `labelContainer.click()`: um `click` programático, com `detail` 0.
    fn programmatic_click(&self) {
        self.dom.dispatch(&self.label(), DomEvent::new(Kind::Click).bubbling().pointer(0));
    }

    /// `labelContainer.focus()` e o `keydown` da tecla.
    fn press(&self, key_code: KeyCode, key: &str) {
        let label = self.label();
        self.dom.focus_element(&label);
        self.dom.dispatch(&label, DomEvent::key_down(key_code, key));
    }
}

fn create_item(guard: &LeakGuard, command: Option<StatusbarCommand>) -> Fixture {
    let fixture = ViewElement::new();
    let other_target = ViewElement::new();
    other_target.set_parent(Some(fixture.clone()));
    let container = ViewElement::with_classes(&["statusbar-item"]);
    container.set_parent(Some(fixture.clone()));

    let layout = Emitter::new();
    guard.add_rc(layout.clone());
    let layout_service: Rc<dyn ILayoutService> = Rc::new(FakeLayoutService { container: fixture.clone(), layout });
    let config = TestConfigurationService::new(None);
    let _ = config.set_user_configuration("workbench.hover.delay", Dynamic::Number(0.0), None);
    let _ = config.set_user_configuration("workbench.hover.reducedDelay", Dynamic::Number(0.0), None);

    let dom = FakeDom::new();
    let widgets = Rc::new(RealHoverWidgets { factory: HoverWidgetFactory::new(dom.clone(), Rc::new(TestHost)), created: RefCell::new(Vec::new()) });
    let delegates: Rc<RefCell<Vec<Rc<WorkbenchDelegate>>>> = Rc::new(RefCell::new(Vec::new()));
    let hover_service = guard.add_rc(HoverService::new(HoverServiceDeps {
        dom: dom.clone(),
        configuration_service: config.clone() as Rc<dyn IConfigurationService>,
        on_did_show_context_menu: Event::none(),
        keybinding_service: Rc::new(MockKeybindingService),
        layout_service: layout_service.clone(),
        accessibility_service: Rc::new(FakeAccessibilityService),
        hover_widgets: widgets.clone(),
        context_views: Rc::new(ContextViewFactory { layout_service }),
        managed_hover_widgets: Rc::new(DelegateRoutingFactory { delegates: delegates.clone() }),
    }));
    let delegate = Rc::new(WorkbenchDelegate { service: Rc::downgrade(&hover_service) });
    delegates.borrow_mut().push(delegate.clone());
    let hover_delegate: Rc<dyn IHoverDelegate> = delegate;

    let commands: Rc<RefCell<Vec<(String, Vec<String>)>>> = Rc::new(RefCell::new(Vec::new()));
    let storage = guard.add_rc(InMemoryStorageService::new());
    let storage_service: Rc<dyn StorageService> = Rc::new(storage);
    let notification_service = guard.add_rc(NotificationService::new(storage_service));
    let notification_service: Rc<dyn INotificationService> = notification_service;
    let telemetry_service: Rc<dyn ITelemetryService> = Rc::new(TestNotificationTelemetryService::new());

    let mut entry = StatusbarEntry::new("Test", "Test", "Test");
    entry.tooltip = Some(StatusbarTooltip::Content(TooltipContent::Managed(Rc::new(ManagedHoverTooltip { element: Some(Rc::new(|_| ViewElement::new())), ..ManagedHoverTooltip::default() }))));
    entry.command = command;

    let item = StatusbarEntryItem::new(
        StatusbarItemServices {
            command_service: Rc::new(RecordingCommandService { commands: commands.clone() }),
            hover_service,
            notification_service,
            telemetry_service,
            theme_service: Rc::new(EmptyThemeService),
            view: Rc::new(ItemView { dom: dom.clone() }),
        },
        container.clone(),
        entry.clone(),
        hover_delegate,
    );
    guard.add_rc(item.clone());

    Fixture { item, entry, container, other_target, fixture, dom, widgets, commands }
}

fn toggle() -> Option<StatusbarCommand> {
    Some(StatusbarCommand::ToggleTooltip)
}

#[test]
fn successive_clicks_show_hide_and_reopen_the_tooltip() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = create_item(&guard, toggle());
        let mut visibility = vec![f.is_visible()];
        for _ in 0..4 {
            f.click().await;
            visibility.push(f.is_visible());
        }

        assert_eq!(visibility, vec![false, true, false, true, false]);
    });
}

#[test]
fn reopens_after_clicking_outside_the_tooltip() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = create_item(&guard, toggle());
        f.click().await;
        // `mainWindow.document.body`: um elemento fora do `fixture`.
        let body = ViewElement::new();
        f.dom.dispatch(&body, DomEvent::new(Kind::MouseDown).bubbling());
        let after_dismiss = f.is_visible();
        f.click().await;

        assert_eq!((after_dismiss, f.is_visible()), (false, true));
    });
}

async fn click_only_activation_reopens_after(interaction: &str) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_item(&guard, toggle());
    f.click().await;
    let button = if interaction == "right-click" { 2 } else { 0 };
    let label = f.label();
    f.dom.dispatch(&label, DomEvent::new(Kind::PointerDown).bubbling().button(button).pointer(1));
    f.dom.dispatch(&label, DomEvent::new(Kind::MouseDown).bubbling().button(button));

    if interaction == "right-click" {
        f.dom.dispatch(&label, DomEvent::new(Kind::ContextMenu).bubbling().button(button));
    } else if interaction == "drag-away" {
        f.dom.dispatch(&f.container, DomEvent::new(Kind::MouseLeave));
        f.dom.dispatch(&f.other_target, DomEvent::new(Kind::PointerUp).bubbling().pointer(1));
        f.dom.dispatch(&f.other_target, DomEvent::new(Kind::MouseUp).bubbling());
    } else {
        f.dom.dispatch(&label, DomEvent::new(Kind::PointerCancel).bubbling().pointer(1));
    }

    let after_dismiss = f.is_visible();
    f.programmatic_click();
    timeout(0).await.unwrap();

    assert_eq!((after_dismiss, f.is_visible()), (false, true));
}

#[test]
fn click_only_activation_reopens_after_right_click() {
    run_with_faked_timers(click_only_activation_reopens_after("right-click"));
}

#[test]
fn click_only_activation_reopens_after_drag_away() {
    run_with_faked_timers(click_only_activation_reopens_after("drag-away"));
}

#[test]
fn click_only_activation_reopens_after_pointer_cancel() {
    run_with_faked_timers(click_only_activation_reopens_after("pointer-cancel"));
}

#[test]
fn click_only_activation_uses_live_visibility_after_keyboard_activation() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = create_item(&guard, toggle());
        let label = f.label();
        f.dom.dispatch(&label, DomEvent::new(Kind::PointerDown).bubbling().pointer(1));
        f.dom.dispatch(&label, DomEvent::new(Kind::PointerCancel).bubbling().pointer(1));
        f.press(KeyCode::Enter, "Enter");
        timeout(0).await.unwrap();

        let after_keyboard = f.is_visible();
        f.programmatic_click();
        timeout(0).await.unwrap();

        assert_eq!((after_keyboard, f.is_visible()), (true, false));
    });
}

async fn activation_focuses_a_passive_preview_before_toggling_it_closed(preview: &str) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let f = create_item(&guard, toggle());
    let label = f.label();
    f.dom.focus_element(&f.other_target);
    if preview == "hover" {
        f.dom.dispatch(&label, DomEvent::new(Kind::MouseOver).bubbling());
    } else {
        f.dom.focus_element(&label);
        f.dom.dispatch(&label, DomEvent::new(Kind::Focus).bubbling().related(&f.other_target));
    }
    timeout(500).await.unwrap();

    let mut states = vec![(f.is_visible(), f.is_focused())];
    for _ in 0..2 {
        if preview == "hover" {
            f.click().await;
        } else {
            f.press(KeyCode::Enter, "Enter");
            timeout(0).await.unwrap();
        }
        states.push((f.is_visible(), f.is_focused()));
    }

    assert_eq!(states, vec![(true, false), (true, true), (false, false)]);
}

#[test]
fn activation_focuses_a_passive_hover_preview_before_toggling_it_closed() {
    run_with_faked_timers(activation_focuses_a_passive_preview_before_toggling_it_closed("hover"));
}

#[test]
fn activation_focuses_a_passive_focus_preview_before_toggling_it_closed() {
    run_with_faked_timers(activation_focuses_a_passive_preview_before_toggling_it_closed("focus"));
}

#[test]
fn still_toggles_after_leaving_the_status_entry() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = create_item(&guard, toggle());
        f.click().await;
        f.dom.dispatch(&f.container, DomEvent::new(Kind::MouseLeave));
        let after_mouse_leave = f.is_visible();
        f.click().await;

        assert_eq!((after_mouse_leave, f.is_visible()), (true, false));
    });
}

#[test]
fn keyboard_activation_toggles_the_tooltip_and_escape_restores_focus() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = create_item(&guard, toggle());
        let mut visibility = Vec::new();
        for (key_code, key) in [(KeyCode::Enter, "Enter"), (KeyCode::Enter, "Enter"), (KeyCode::Space, " "), (KeyCode::Escape, "Escape")] {
            f.press(key_code, key);
            timeout(0).await.unwrap();
            visibility.push(f.is_visible());
        }

        let focus_restored = f.dom.active.borrow().as_ref().is_some_and(|active| Rc::ptr_eq(active, &f.label()));
        assert_eq!((visibility, focus_restored), (vec![true, false, true, false], true));
    });
}

#[test]
fn touch_activation_toggles_the_tooltip() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = create_item(&guard, toggle());
        let label = f.label();
        let mut visibility = Vec::new();
        for _ in 0..2 {
            f.dom.dispatch(&label, DomEvent::new(Kind::PointerDown).bubbling());
            f.dom.dispatch(&label, DomEvent::new(Kind::Tap).bubbling());
            timeout(0).await.unwrap();
            visibility.push(f.is_visible());
        }

        assert_eq!(visibility, vec![true, false]);
    });
}

#[test]
fn preserves_show_only_tooltip_commands() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let f = create_item(&guard, Some(StatusbarCommand::ShowTooltip));
        let mut visibility = Vec::new();
        for _ in 0..2 {
            f.click().await;
            visibility.push(f.is_visible());
        }

        assert_eq!(visibility, vec![true, true]);
    });
}

#[test]
fn updates_command_listeners_when_switching_to_the_toggle_command() {
    run_with_faked_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let command = Command { id: "test.command".to_owned(), title: "Test".to_owned(), tooltip: None, arguments: Some(vec![Dynamic::string("argument")]) };
        let f = create_item(&guard, Some(StatusbarCommand::Command(Rc::new(command))));
        f.click().await;
        f.item.update(StatusbarEntry { command: toggle(), ..f.entry.clone() });
        f.click().await;
        f.click().await;

        assert_eq!(*f.commands.borrow(), vec![("test.command".to_owned(), vec!["argument".to_owned()])]);
        assert!(!f.is_visible());
    });
}
