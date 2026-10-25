//! Porte de `src/vs/platform/hover/browser/hoverService.ts`: o `HoverService` (`showInstantHover`, `showDelayedHover`,
//! `setupDelayedHover`, `setupDelayedHoverAtMouse`, `setupManagedHover`, a pilha de hovers aninhados, o hover fixo
//! (`sticky`), o travamento por Alt, o foco e o teclado, `showAndFocusLastHover` e o atalho `workbench.action.showHover`).
//!
//! Divergências de tradução:
//! - **DOM.** O DOM que o serviço lê e ouve entra pela interface [`IHoverDom`] (`addDisposableListener`,
//!   `getActiveElement`, `isAncestorOfActiveElement`, `hasAttribute`/`setAttribute`/`title`, `IntersectionObserver`,
//!   `isEditableElement`), com o [`HoverDomEvent`] no lugar de `MouseEvent`, `KeyboardEvent` e `FocusEvent`. O
//!   `isAncestor` é a cadeia de [`ViewElement::parent_element`]. Os alvos de ouvinte são o elemento, o `document` e a
//!   `window` ([`HoverDomTarget`]).
//! - **Instanciação.** `instantiationService.createInstance(HoverWidget, options)` é a fábrica [`IHoverWidgetFactory`]
//!   (o `HoverWidget` posicionado de `platform/hover/browser/hoverWidget.ts`, que é outro arquivo do oráculo),
//!   `new ContextView(container, ABSOLUTE)` é a [`IHoverContextViewFactory`] e `new ManagedHoverWidget(...)` (de
//!   `updatableHoverWidget.ts`) é a [`IManagedHoverWidgetFactory`]. Os testes montam as três.
//! - **`IContextMenuService`.** Só o `onDidShowContextMenu` é lido: entra como [`Event`] no construtor.
//! - **Identidade de opções.** `getHoverOptionsIdentity` devolve `options.id ?? options` e a segunda forma é a identidade do
//!   objeto: aqui cada [`IHoverOptions`] criado por [`IHoverOptions::new`] ou [`IHoverOptions::respread`] (o `{ ...options }`)
//!   carrega um número de objeto, e a identidade é [`HoverIdentity`].
//! - **Opções mutáveis.** O original altera o objeto do chamador (`id`, `container`, `target`); aqui a opção é valor do
//!   serviço e a alteração vale para a cópia que a pilha e o `showAndFocusLastHover` guardam.
//! - **Conteúdo gerenciado.** `IManagedHoverContentOrFactory` assíncrono e `IManagedHoverTooltipMarkdownString` são
//!   [`ManagedHoverContent`]; o `ManagedHoverWidget` os renderiza.
//! - **Registro.** O `registerSingleton` do fim do módulo e o participante de tema (borda das linhas do hover) são do
//!   carregamento do módulo e entram quando o serviço for ligado ao contêiner; o atalho `workbench.action.showHover`
//!   é registrado no construtor, como no original.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{TimeoutTimer, disposable_timeout};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::html_content::IMarkdownString;
use je_base::icon_labels::strip_icons;
use je_base::key_codes::{KeyCode, KeyMod, key_chord};
use je_base::keybindings::ResolvedKeybinding;
use je_base::layout::{AnchorAlignment, AnchorPosition, IAnchor};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_instantiation::ServicesAccessor;

use crate::accessibility::IAccessibilityService;
use crate::configuration::service::IConfigurationService;
use crate::contextview::context_view::{AnchorTarget, IContextViewDelegate};
use crate::contextview::view_element::ViewElement;
use crate::keybinding::keybinding::{IKeybindingService, KeyboardEvent};
use crate::keybinding::keybinding_resolver::ResolutionResult;
use crate::keybinding::keybindings_registry::{CommandAndKeybindingRule, KeybindingRule, KeybindingWeight, Keybindings, keybindings_registry};
use crate::layout::layout_service::ILayoutService;

/// Maximum nesting depth for hovers. This prevents runaway nesting.
pub const MAX_HOVER_NESTING_DEPTH: usize = 3;

// region Contratos de `hover.ts` e `hoverDelegate.ts` no recorte da plataforma (elementos são `ViewElement`)

/// `HoverPosition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverPosition {
    Left = 1,
    Right,
    Below,
    Above,
}

/// `HoverStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverStyle {
    Pointer = 1,
    Mouse = 2,
}

/// `IMarkdownString | string | HTMLElement`.
#[derive(Clone)]
pub enum HoverContent {
    Text(String),
    Markdown(IMarkdownString),
    Element(Rc<ViewElement>),
}

impl HoverContent {
    /// `content === ''`.
    pub fn is_empty_text(&self) -> bool {
        matches!(self, HoverContent::Text(text) if text.is_empty())
    }
}

/// `IHoverTarget` com os elementos e a posição do ponteiro.
#[derive(Clone)]
pub struct StructuredHoverTarget {
    pub target_elements: Vec<Rc<ViewElement>>,
    pub x: Option<f64>,
    pub y: Option<f64>,
}

/// `IHoverTarget | HTMLElement`.
#[derive(Clone)]
pub enum HoverTarget {
    Element(Rc<ViewElement>),
    Structured(StructuredHoverTarget),
}

impl HoverTarget {
    /// `isHTMLElement(target) ? [target] : target.targetElements`.
    pub fn target_elements(&self) -> Vec<Rc<ViewElement>> {
        match self {
            HoverTarget::Element(element) => vec![element.clone()],
            HoverTarget::Structured(target) => target.target_elements.clone(),
        }
    }

    /// O primeiro elemento (`isHTMLElement(target) ? target : target.targetElements[0]`).
    pub fn first_element(&self) -> Option<Rc<ViewElement>> {
        match self {
            HoverTarget::Element(element) => Some(element.clone()),
            HoverTarget::Structured(target) => target.target_elements.first().cloned(),
        }
    }
}

/// `IHoverAction`.
#[derive(Clone)]
pub struct IHoverAction {
    pub label: String,
    pub command_id: String,
    pub icon_class: Option<String>,
    pub run: Rc<dyn Fn(&Rc<ViewElement>)>,
}

/// `hoverPosition?: HoverPosition | MouseEvent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HoverPositionOption {
    Position(HoverPosition),
    /// O `MouseEvent` (`x` e `y`).
    Mouse { x: f64, y: f64 },
}

#[derive(Clone, Default)]
pub struct IHoverPositionOptions {
    pub hover_position: Option<HoverPositionOption>,
    pub anchor_alignment: Option<AnchorAlignment>,
    pub force_position: Option<bool>,
    pub clipping_element: Option<Rc<ViewElement>>,
}

#[derive(Clone, Copy, Default)]
pub struct IHoverPersistenceOptions {
    pub hide_on_hover: Option<bool>,
    pub hide_on_key_down: Option<bool>,
    pub sticky: Option<bool>,
}

#[derive(Clone, Copy, Default)]
pub struct IHoverAppearanceOptions {
    pub show_pointer: Option<bool>,
    pub compact: Option<bool>,
    pub show_hover_hint: Option<bool>,
    pub skip_fade_in_animation: Option<bool>,
    pub max_height_ratio: Option<f64>,
}

thread_local! {
    static NEXT_OPTIONS_OBJECT: Cell<u64> = const { Cell::new(1) };
}

fn next_options_object() -> u64 {
    NEXT_OPTIONS_OBJECT.with(|next| {
        let value = next.get();
        next.set(value + 1);
        value
    })
}

/// `IHoverOptions`.
#[derive(Clone)]
pub struct IHoverOptions {
    pub content: HoverContent,
    pub target: HoverTarget,
    pub container: Option<Rc<ViewElement>>,
    pub id: Option<String>,
    pub actions: Vec<IHoverAction>,
    pub additional_classes: Vec<String>,
    pub link_handler: Option<Rc<dyn Fn(&str)>>,
    pub trap_focus: bool,
    pub style: Option<HoverStyle>,
    pub position: Option<IHoverPositionOptions>,
    pub persistence: Option<IHoverPersistenceOptions>,
    pub appearance: Option<IHoverAppearanceOptions>,
    pub on_did_show: Option<Rc<dyn Fn()>>,
    pub on_did_hide: Option<Rc<dyn Fn()>>,
    object: u64,
}

impl IHoverOptions {
    pub fn new(content: HoverContent, target: HoverTarget) -> Self {
        Self {
            content,
            target,
            container: None,
            id: None,
            actions: Vec::new(),
            additional_classes: Vec::new(),
            link_handler: None,
            trap_focus: false,
            style: None,
            position: None,
            persistence: None,
            appearance: None,
            on_did_show: None,
            on_did_hide: None,
            object: next_options_object(),
        }
    }

    /// `{ ...options }`: as mesmas opções num objeto novo (identidade nova).
    pub fn respread(&self) -> Self {
        Self { object: next_options_object(), ..self.clone() }
    }

    /// `persistence?.sticky`.
    pub fn is_sticky(&self) -> bool {
        self.persistence.and_then(|persistence| persistence.sticky).unwrap_or(false)
    }
}

/// O resultado de `getHoverOptionsIdentity`: `options.id ?? options`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HoverIdentity {
    Id(String),
    Object(u64),
}

/// `getHoverOptionsIdentity`.
pub fn get_hover_options_identity(options: Option<&IHoverOptions>) -> Option<HoverIdentity> {
    let options = options?;
    Some(match &options.id {
        Some(id) => HoverIdentity::Id(id.clone()),
        None => HoverIdentity::Object(options.object),
    })
}

/// `getHoverIdFromContent`.
pub fn get_hover_id_from_content(content: &HoverContent) -> Option<String> {
    match content {
        HoverContent::Element(_) => None,
        HoverContent::Text(text) => Some(text.clone()),
        HoverContent::Markdown(markdown) => Some(markdown.value.to_string()),
    }
}

/// `IHoverLifecycleOptions`.
#[derive(Clone, Default)]
pub struct IHoverLifecycleOptions {
    pub group_id: Option<String>,
    pub reduced_delay: Option<bool>,
    pub setup_keyboard_events: Option<bool>,
}

/// O `IManagedHoverTooltipMarkdownString` e o conteúdo de elemento do hover gerenciável.
#[derive(Clone, Default)]
pub struct ManagedHoverTooltip {
    pub markdown: Option<HoverContent>,
    pub markdown_not_supported_fallback: Option<String>,
    /// `element: (token) => HTMLElement`.
    pub element: Option<Rc<dyn Fn(CancellationToken) -> Rc<ViewElement>>>,
    pub content_owns_padding: bool,
}

/// `IManagedHoverContent`.
#[derive(Clone)]
pub enum ManagedHoverContent {
    Undefined,
    Text(String),
    Tooltip(ManagedHoverTooltip),
}

/// `IManagedHoverContentOrFactory`.
#[derive(Clone)]
pub enum ManagedHoverContentOrFactory {
    Content(ManagedHoverContent),
    Factory(Rc<dyn Fn() -> ManagedHoverContent>),
}

impl ManagedHoverContentOrFactory {
    /// `typeof content === 'function' ? content() : content`.
    pub fn resolve(&self) -> ManagedHoverContent {
        match self {
            ManagedHoverContentOrFactory::Content(content) => content.clone(),
            ManagedHoverContentOrFactory::Factory(factory) => factory(),
        }
    }
}

impl From<&str> for ManagedHoverContentOrFactory {
    fn from(text: &str) -> Self {
        Self::Content(ManagedHoverContent::Text(text.to_owned()))
    }
}

/// `IManagedHoverOptions`.
#[derive(Clone, Default)]
pub struct IManagedHoverOptions {
    pub actions: Vec<IHoverAction>,
    pub additional_classes: Vec<String>,
    pub link_handler: Option<Rc<dyn Fn(&str)>>,
    pub trap_focus: Option<bool>,
    pub on_did_show: Option<Rc<dyn Fn()>>,
    pub show_hover_hint: Option<bool>,
    pub anchor_alignment: Option<AnchorAlignment>,
}

/// `IHoverDelegateTarget`: os elementos e o `x` que o rastreio do mouse atualiza.
pub struct HoverDelegateTarget {
    pub target_elements: Vec<Rc<ViewElement>>,
    pub x: Cell<Option<f64>>,
}

/// O `delay: number | ((content?) => number)` do `IHoverDelegate`.
#[derive(Clone)]
pub enum HoverDelay {
    Fixed(f64),
    Dynamic(Rc<dyn Fn(&ManagedHoverContentOrFactory) -> f64>),
}

impl HoverDelay {
    pub fn resolve(&self, content: &ManagedHoverContentOrFactory) -> f64 {
        match self {
            HoverDelay::Fixed(delay) => *delay,
            HoverDelay::Dynamic(f) => f(content),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverPlacement {
    Mouse,
    Element,
}

/// `IHoverDelegate`.
pub trait IHoverDelegate {
    fn on_did_hide_hover(&self) {}
    fn delay(&self) -> HoverDelay;
    /// `placement`: `undefined` é `None`.
    fn placement(&self) -> Option<HoverPlacement> {
        None
    }
    fn show_native_hover(&self) -> bool {
        false
    }
}

/// `IHoverWidget` (o que o serviço devolve): descartar esconde a dica.
pub trait IHoverWidget: IDisposable {
    fn is_disposed(&self) -> bool;
}

/// `IManagedHover`.
pub trait IManagedHover: IDisposable {
    fn show(&self, focus: bool);
    fn hide(&self);
    fn update(&self, content: ManagedHoverContentOrFactory, options: Option<IManagedHoverOptions>) -> Promise<()>;
}

// endregion

// region Costuras com o DOM e com os widgets

/// Os eventos de DOM que o serviço ouve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverDomEventKind {
    MouseOver,
    MouseDown,
    MouseUp,
    MouseLeave,
    MouseMove,
    Click,
    KeyDown,
    KeyUp,
    Focus,
    Blur,
    Resize,
}

/// `EventTarget` do `addDisposableListener`.
#[derive(Clone)]
pub enum HoverDomTarget {
    Element(Rc<ViewElement>),
    /// `window.document` do alvo.
    Document,
    /// A `window` do alvo.
    Window,
}

/// `MouseEvent`, `KeyboardEvent` e `FocusEvent` no que o serviço lê.
#[derive(Clone)]
pub struct HoverDomEvent {
    pub target: Option<Rc<ViewElement>>,
    pub related_target: Option<Rc<ViewElement>>,
    /// O `fromElement` não padrão do `mouseleave`.
    pub from_element: Option<Rc<ViewElement>>,
    pub x: f64,
    pub key: String,
    pub key_code: KeyCode,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
}

impl HoverDomEvent {
    pub fn new(kind_target: Option<Rc<ViewElement>>) -> Self {
        Self {
            target: kind_target,
            related_target: None,
            from_element: None,
            x: 0.0,
            key: String::new(),
            key_code: KeyCode::Unknown,
            ctrl_key: false,
            shift_key: false,
            alt_key: false,
            meta_key: false,
        }
    }
}

pub type HoverDomHandler = Rc<dyn Fn(&HoverDomEvent)>;

/// O DOM que o `HoverService` toca.
pub trait IHoverDom {
    /// `addDisposableListener(target, type, handler, capture)`.
    fn add_listener(&self, target: HoverDomTarget, kind: HoverDomEventKind, capture: bool, handler: HoverDomHandler) -> DisposableRef;
    /// `getActiveElement()`.
    fn active_element(&self) -> Option<Rc<ViewElement>>;
    /// `isAncestorOfActiveElement(element)`.
    fn is_ancestor_of_active_element(&self, element: &Rc<ViewElement>) -> bool;
    /// `element.focus()`.
    fn focus(&self, element: &Rc<ViewElement>);
    fn has_attribute(&self, element: &Rc<ViewElement>, name: &str) -> bool;
    fn get_attribute(&self, element: &Rc<ViewElement>, name: &str) -> Option<String>;
    fn set_attribute(&self, element: &Rc<ViewElement>, name: &str, value: &str);
    fn remove_attribute(&self, element: &Rc<ViewElement>, name: &str);
    /// `isEditableElement(element)`.
    fn is_editable_element(&self, element: &Rc<ViewElement>) -> bool;
    /// O `IntersectionObserver` (`threshold: 0`) do primeiro alvo; `None` quando a plataforma não tem (`'IntersectionObserver' in mainWindow`).
    fn observe_intersection(&self, element: &Rc<ViewElement>, on_change: Rc<dyn Fn(bool)>) -> Option<DisposableRef>;
}

/// `isAncestor(testChild, testAncestor)`: o próprio elemento conta.
pub fn is_ancestor(test_child: &Rc<ViewElement>, test_ancestor: &Rc<ViewElement>) -> bool {
    let mut candidate = Some(test_child.clone());
    while let Some(current) = candidate {
        if Rc::ptr_eq(&current, test_ancestor) {
            return true;
        }
        candidate = current.parent_element();
    }
    false
}

/// O `HoverWidget` posicionado, no que o serviço usa.
pub trait IHoverWidgetHandle: IHoverWidget {
    /// `domNode`.
    fn dom_node(&self) -> Rc<ViewElement>;
    fn is_locked(&self) -> bool;
    fn set_is_locked(&self, locked: bool);
    fn is_mouse_in(&self) -> bool;
    fn on_dispose(&self) -> Event<()>;
    fn on_request_layout(&self) -> Event<()>;
    /// `addMouseTrackingElement(element)`.
    fn add_mouse_tracking_element(&self, element: &Rc<ViewElement>) -> DisposableRef;
    fn render(&self, container: &Rc<ViewElement>);
    fn focus(&self);
    fn layout(&self);
    /// `anchor`.
    fn anchor(&self) -> AnchorPosition;
    fn x(&self) -> f64;
    fn y(&self) -> f64;
}

/// `createInstance(HoverWidget, options)`: as opções chegam com `container`, `id` e `target` já ajustados.
pub trait IHoverWidgetFactory {
    fn create(&self, options: &mut IHoverOptions) -> Rc<dyn IHoverWidgetHandle>;
}

/// `ContextView` (`contextview.ts`) com `ContextViewDOMPosition.ABSOLUTE`.
pub trait IHoverContextView {
    fn show(&self, delegate: Rc<dyn IContextViewDelegate>);
    fn layout(&self);
    fn dispose(&self);
}

pub trait IHoverContextViewFactory {
    fn create(&self, container: Rc<ViewElement>) -> Rc<dyn IHoverContextView>;
}

/// `ManagedHoverWidget` (`updatableHoverWidget.ts`).
pub trait IManagedHoverWidget: IDisposable {
    fn is_disposed(&self) -> bool;
    /// `update(content, focus, options)`.
    fn update(&self, content: ManagedHoverContent, focus: Option<bool>, options: Option<IManagedHoverOptions>) -> Promise<()>;
}

pub trait IManagedHoverWidgetFactory {
    /// `new ManagedHoverWidget(hoverDelegate, target, fadeInAnimation)`.
    fn create(&self, delegate: &Rc<dyn IHoverDelegate>, target: Rc<HoverDelegateTarget>, fade_in_animation: bool) -> Rc<dyn IManagedHoverWidget>;
}

// endregion

/// Uma entrada da pilha de hovers: o hover e o estado associado.
#[derive(Clone)]
struct HoverStackEntry {
    hover: Rc<dyn IHoverWidgetHandle>,
    options: IHoverOptions,
    context_view: Rc<dyn IHoverContextView>,
    last_focused_element_before_open: Option<Rc<ViewElement>>,
}

/// O resultado de criar um hover.
#[derive(Clone)]
struct CreateHoverResult {
    hover: Rc<dyn IHoverWidgetHandle>,
    store: Rc<DisposableStore>,
    last_focused_element_before_open: Option<Rc<ViewElement>>,
}

/// Os serviços de que o `HoverService` depende.
pub struct HoverServiceDeps {
    pub dom: Rc<dyn IHoverDom>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    /// `contextMenuService.onDidShowContextMenu`.
    pub on_did_show_context_menu: Event<()>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub layout_service: Rc<dyn ILayoutService>,
    pub accessibility_service: Rc<dyn IAccessibilityService>,
    pub hover_widgets: Rc<dyn IHoverWidgetFactory>,
    pub context_views: Rc<dyn IHoverContextViewFactory>,
    pub managed_hover_widgets: Rc<dyn IManagedHoverWidgetFactory>,
}

#[derive(Default)]
struct HoverServiceState {
    /// Stack of currently visible hovers. The last entry is the topmost hover.
    hover_stack: Vec<HoverStackEntry>,
    current_delayed_hover: Option<Rc<dyn IHoverWidgetHandle>>,
    current_delayed_hover_was_shown: bool,
    current_delayed_hover_group_id: Option<String>,
    last_hover_options: Option<IHoverOptions>,
    delayed_hovers: Vec<(Rc<ViewElement>, Rc<dyn Fn(bool)>)>,
    managed_hovers: Vec<(Rc<ViewElement>, Rc<dyn IManagedHover>)>,
}

type ResolveHoverOptions = Rc<dyn Fn(Option<&HoverDomEvent>) -> IHoverOptions>;

/// `HoverService`.
pub struct HoverService {
    this: Weak<HoverService>,
    deps: HoverServiceDeps,
    state: RefCell<HoverServiceState>,
    disposables: Rc<DisposableStore>,
}

impl HoverService {
    pub fn new(deps: HoverServiceDeps) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let service = Rc::new_cyclic(|this| Self { this: this.clone(), deps, state: RefCell::new(HoverServiceState::default()), disposables: disposables.clone() });

        let weak = Rc::downgrade(&service);
        disposables.add(service.deps.on_did_show_context_menu.listen(move |_| {
            if let Some(service) = weak.upgrade() {
                service.hide_hover(false);
            }
        }));

        let weak = Rc::downgrade(&service);
        let handler: Rc<dyn Fn(&ServicesAccessor, &[Dynamic]) -> Dynamic> = Rc::new(move |_, _| {
            if let Some(service) = weak.upgrade() {
                service.show_and_focus_hover_for_active_element();
            }
            Dynamic::Undefined
        });
        disposables.add(keybindings_registry().register_command_and_keybinding_rule(CommandAndKeybindingRule {
            rule: KeybindingRule {
                keybindings: Keybindings {
                    primary: Some(key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::KeyI.bits())),
                    secondary: None,
                    win: None,
                    linux: None,
                    mac: None,
                },
                id: "workbench.action.showHover".to_owned(),
                weight: KeybindingWeight::EDITOR_CORE,
                args: Dynamic::Undefined,
                when: None,
            },
            handler,
            metadata: None,
        }));
        service
    }

    fn current_hover(&self) -> Option<Rc<dyn IHoverWidgetHandle>> {
        self.state.borrow().hover_stack.last().map(|entry| entry.hover.clone())
    }

    fn current_hover_options(&self) -> Option<IHoverOptions> {
        self.state.borrow().hover_stack.last().map(|entry| entry.options.clone())
    }

    /// Returns whether the target element is inside any of the hovers in the stack. If it is, returns the index of the
    /// containing hover.
    fn get_containing_hover_index(&self, target: &HoverTarget) -> Option<usize> {
        let target_elements = target.target_elements();
        let nodes: Vec<Rc<ViewElement>> = self.state.borrow().hover_stack.iter().map(|entry| entry.hover.dom_node()).collect();
        // Search from top of stack to bottom (most recent hover first)
        for index in (0..nodes.len()).rev() {
            for target_element in &target_elements {
                if is_ancestor(target_element, &nodes[index]) {
                    return Some(index);
                }
            }
        }
        None
    }

    fn config_number(&self, key: &str) -> u64 {
        match self.deps.configuration_service.get_value(Some(key), None) {
            Dynamic::Number(value) if value.is_finite() && value > 0.0 => value as u64,
            _ => 0,
        }
    }

    pub fn show_instant_hover(&self, options: IHoverOptions, focus: bool, skip_last_focused_update: bool) -> Option<Rc<dyn IHoverWidgetHandle>> {
        let mut options = options;
        let hover = self.create_hover(&mut options, skip_last_focused_update)?;
        self.show_hover(&hover, &options, focus);
        Some(hover.hover)
    }

    pub fn show_delayed_hover(&self, options: IHoverOptions, lifecycle_options: &IHoverLifecycleOptions) -> Option<Rc<dyn IHoverWidgetHandle>> {
        let mut options = options;
        // Set `id` to default if it's undefined
        if options.id.is_none() {
            options.id = get_hover_id_from_content(&options.content);
        }

        let (current_delayed, was_shown, group_id) = {
            let state = self.state.borrow();
            (state.current_delayed_hover.clone(), state.current_delayed_hover_was_shown, state.current_delayed_hover_group_id.clone())
        };
        let current_identity = get_hover_options_identity(self.current_hover_options().as_ref());
        let identity = get_hover_options_identity(Some(&options));

        if current_delayed.is_none() || was_shown {
            // Current hover is locked, reject, unless this is a nesting scenario
            if self.current_hover().is_some_and(|hover| hover.is_locked()) && self.get_containing_hover_index(&options.target).is_none() {
                return None;
            }

            // Identity is the same, return current hover
            if current_identity == identity {
                return self.current_hover();
            }

            // Check group identity, if it's the same skip the delay and show the hover immediately
            if self.current_hover().is_some_and(|hover| !hover.is_disposed()) && group_id.is_some() && group_id == lifecycle_options.group_id {
                let mut instant = options.respread();
                let mut appearance = instant.appearance.unwrap_or_default();
                appearance.skip_fade_in_animation = Some(true);
                instant.appearance = Some(appearance);
                return self.show_instant_hover(instant, false, false);
            }
        } else if current_delayed.is_some() && current_identity == identity {
            // If the hover is the same but timeout is not finished yet, return the current hover
            return current_delayed;
        }

        let Some(hover) = self.create_hover(&mut options, false) else {
            let mut state = self.state.borrow_mut();
            state.current_delayed_hover = None;
            state.current_delayed_hover_was_shown = false;
            state.current_delayed_hover_group_id = None;
            return None;
        };

        {
            let mut state = self.state.borrow_mut();
            state.current_delayed_hover = Some(hover.hover.clone());
            state.current_delayed_hover_was_shown = false;
            state.current_delayed_hover_group_id = lifecycle_options.group_id.clone();
        }

        let delay = if lifecycle_options.reduced_delay.unwrap_or(false) { self.config_number("workbench.hover.reducedDelay") } else { self.config_number("workbench.hover.delay") };
        let weak = self.this.clone();
        let shown = hover.clone();
        let shown_options = options.clone();
        // O timer pertence ao `hoverDisposables` do hover: dispara e sai do store, ou é cancelado junto com o hover (clique,
        // cancelamento, mouseout, dispose do serviço), sem deixar timer solto.
        let store = hover.store.clone();
        disposable_timeout(
            move || {
                let Some(service) = weak.upgrade() else {
                    return;
                };
                if !shown.hover.is_disposed() {
                    service.state.borrow_mut().current_delayed_hover_was_shown = true;
                    service.show_hover(&shown, &shown_options, false);
                }
            },
            delay,
            Some(store),
        );

        Some(hover.hover)
    }

    /// `setupDelayedHover(target, options, lifecycleOptions)`: `options` devolve as opções (o `target` delas é ignorado e
    /// substituído pelo elemento).
    pub fn setup_delayed_hover(&self, target: &Rc<ViewElement>, options: Rc<dyn Fn() -> IHoverOptions>, lifecycle_options: Option<IHoverLifecycleOptions>) -> DisposableRef {
        let element = target.clone();
        let resolve: ResolveHoverOptions = Rc::new(move |e| {
            let mut resolved = options();
            resolved.target = HoverTarget::Element(element.clone());
            if resolved.style == Some(HoverStyle::Mouse) {
                if let Some(e) = e {
                    resolved.target = resolve_mouse_style_hover_target(&element, e);
                }
            }
            resolved
        });
        self.setup_delayed_hover_inner(target, resolve, lifecycle_options)
    }

    /// `setupDelayedHoverAtMouse(target, options, lifecycleOptions)`.
    pub fn setup_delayed_hover_at_mouse(&self, target: &Rc<ViewElement>, options: Rc<dyn Fn() -> IHoverOptions>, lifecycle_options: Option<IHoverLifecycleOptions>) -> DisposableRef {
        let element = target.clone();
        let resolve: ResolveHoverOptions = Rc::new(move |e| {
            let mut resolved = options();
            resolved.target = match e {
                Some(e) => resolve_mouse_style_hover_target(&element, e),
                None => HoverTarget::Element(element.clone()),
            };
            resolved
        });
        self.setup_delayed_hover_inner(target, resolve, lifecycle_options)
    }

    fn setup_delayed_hover_inner(&self, target: &Rc<ViewElement>, resolve_hover_options: ResolveHoverOptions, lifecycle_options: Option<IHoverLifecycleOptions>) -> DisposableRef {
        let store = DisposableStore::new();
        let lifecycle = lifecycle_options.clone().unwrap_or_default();

        let weak = self.this.clone();
        let resolve = resolve_hover_options.clone();
        let group = IHoverLifecycleOptions { group_id: lifecycle.group_id.clone(), reduced_delay: lifecycle.reduced_delay, setup_keyboard_events: None };
        store.add(self.deps.dom.add_listener(
            HoverDomTarget::Element(target.clone()),
            HoverDomEventKind::MouseOver,
            false,
            Rc::new(move |e| {
                if let Some(service) = weak.upgrade() {
                    service.show_delayed_hover(resolve(Some(e)), &group);
                }
            }),
        ));
        if lifecycle.setup_keyboard_events.unwrap_or(false) {
            let weak = self.this.clone();
            let resolve = resolve_hover_options.clone();
            store.add(self.deps.dom.add_listener(
                HoverDomTarget::Element(target.clone()),
                HoverDomEventKind::KeyDown,
                false,
                Rc::new(move |e| {
                    if e.key_code == KeyCode::Space || e.key_code == KeyCode::Enter {
                        if let Some(service) = weak.upgrade() {
                            service.show_instant_hover(resolve(None), true, false);
                        }
                    }
                }),
            ));
        }

        let weak = self.this.clone();
        let resolve = resolve_hover_options;
        let show: Rc<dyn Fn(bool)> = Rc::new(move |focus| {
            if let Some(service) = weak.upgrade() {
                service.show_instant_hover(resolve(None), focus, false);
            }
        });
        self.state.borrow_mut().delayed_hovers.push((target.clone(), show));
        let weak = self.this.clone();
        let element = target.clone();
        store.add(to_disposable(move || {
            if let Some(service) = weak.upgrade() {
                service.state.borrow_mut().delayed_hovers.retain(|(candidate, _)| !Rc::ptr_eq(candidate, &element));
            }
        }));

        store
    }

    fn create_hover(&self, options: &mut IHoverOptions, skip_last_focused_update: bool) -> Option<CreateHoverResult> {
        let delayed = {
            let mut state = self.state.borrow_mut();
            state.current_delayed_hover.take()
        };
        if let Some(delayed) = delayed {
            delayed.dispose();
        }

        if options.content.is_empty_text() {
            return None;
        }

        // Set `id` to default if it's undefined
        if options.id.is_none() {
            options.id = get_hover_id_from_content(&options.content);
        }

        // Check if the target is inside an existing hover (nesting scenario)
        let is_nesting = self.get_containing_hover_index(&options.target).is_some();

        if is_nesting {
            // Check max nesting depth
            if self.state.borrow().hover_stack.len() >= MAX_HOVER_NESTING_DEPTH {
                return None;
            }
            // When nesting, don't check if the parent is locked - we allow nested hovers inside locked parents
        } else {
            // Not nesting: check if current top-level hover is locked
            if self.current_hover().is_some_and(|hover| hover.is_locked()) {
                return None;
            }

            // Check if identity is the same as current hover
            if get_hover_options_identity(self.current_hover_options().as_ref()) == get_hover_options_identity(Some(options)) {
                return None;
            }
        }

        let trap_focus = options.trap_focus || self.deps.accessibility_service.is_screen_reader_optimized();
        let active_element = self.deps.dom.active_element();
        let mut last_focused_element_before_open: Option<Rc<ViewElement>> = None;
        // HACK, remove this check when #189076 is fixed
        if !skip_last_focused_update && trap_focus {
            if let Some(active_element) = &active_element {
                if !active_element.class_list_contains("monaco-hover") {
                    last_focused_element_before_open = Some(active_element.clone());
                }
            }
        }

        let hover_disposables = DisposableStore::new();
        let hover = self.deps.hover_widgets.create(options);
        if options.is_sticky() {
            hover.set_is_locked(true);
        }

        // Adjust target position when a mouse event is provided as the hover position
        if let Some(HoverPositionOption::Mouse { x, .. }) = options.position.as_ref().and_then(|position| position.hover_position) {
            options.target = HoverTarget::Structured(StructuredHoverTarget { target_elements: options.target.target_elements(), x: Some(x + 10.0), y: None });
        }

        {
            let weak = self.this.clone();
            let hover_ref = hover.clone();
            let disposables = hover_disposables.clone();
            let listener = hover.on_dispose().listen(move |_| {
                if let Some(service) = weak.upgrade() {
                    service.on_hover_disposed(&hover_ref);
                }
                disposables.dispose();
            });
            hover_disposables.add(listener);
        }

        // Set the container explicitly to enable aux window support
        if options.container.is_none() {
            options.container = Some(self.deps.layout_service.get_container());
        }

        {
            let weak = self.this.clone();
            let hover_ref = hover.clone();
            hover_disposables.add(self.deps.dom.add_listener(
                HoverDomTarget::Document,
                HoverDomEventKind::MouseDown,
                false,
                Rc::new(move |e| {
                    let inside = e.target.as_ref().is_some_and(|target| is_ancestor(target, &hover_ref.dom_node()));
                    if !inside {
                        if let Some(service) = weak.upgrade() {
                            service.hide_hover_and_descendants(&hover_ref);
                        }
                    }
                }),
            ));
        }

        if !options.is_sticky() {
            for element in options.target.target_elements() {
                let weak = self.this.clone();
                let hover_ref = hover.clone();
                hover_disposables.add(self.deps.dom.add_listener(
                    HoverDomTarget::Element(element),
                    HoverDomEventKind::Click,
                    false,
                    Rc::new(move |_| {
                        if let Some(service) = weak.upgrade() {
                            service.hide_hover_and_descendants(&hover_ref);
                        }
                    }),
                ));
            }
            if let Some(focused_element) = self.deps.dom.active_element() {
                let hide_on_key_down = options.persistence.and_then(|persistence| persistence.hide_on_key_down).unwrap_or(false);
                for (target, kind) in [
                    (HoverDomTarget::Element(focused_element.clone()), HoverDomEventKind::KeyDown),
                    (HoverDomTarget::Document, HoverDomEventKind::KeyDown),
                    (HoverDomTarget::Element(focused_element), HoverDomEventKind::KeyUp),
                    (HoverDomTarget::Document, HoverDomEventKind::KeyUp),
                ] {
                    let weak = self.this.clone();
                    let hover_ref = hover.clone();
                    hover_disposables.add(self.deps.dom.add_listener(
                        target,
                        kind,
                        false,
                        Rc::new(move |e| {
                            if let Some(service) = weak.upgrade() {
                                if kind == HoverDomEventKind::KeyDown {
                                    service.key_down(e, &hover_ref, hide_on_key_down);
                                } else {
                                    service.key_up(e);
                                }
                            }
                        }),
                    ));
                }
            }
        }

        if let Some(first_target_element) = options.target.first_element() {
            let hover_ref = hover.clone();
            let observer = self.deps.dom.observe_intersection(
                &first_target_element,
                Rc::new(move |is_intersecting| {
                    if !is_intersecting {
                        hover_ref.dispose();
                    }
                }),
            );
            if let Some(observer) = observer {
                hover_disposables.add(observer);
            }
        }

        self.state.borrow_mut().last_hover_options = Some(options.clone());
        Some(CreateHoverResult { hover, last_focused_element_before_open, store: hover_disposables })
    }

    /// O `hover.onDispose` do `_createHover`: tira o hover da pilha.
    fn on_hover_disposed(&self, hover: &Rc<dyn IHoverWidgetHandle>) {
        // Pop this hover from the stack if it's still there
        let entry = {
            let state = self.state.borrow();
            state.hover_stack.iter().position(|entry| Rc::ptr_eq(&entry.hover, hover)).map(|index| (index, state.hover_stack[index].clone()))
        };
        let Some((stack_index, entry)) = entry else {
            return;
        };
        // Restore focus if this hover was focused
        let hover_was_focused = self.deps.dom.is_ancestor_of_active_element(&hover.dom_node());
        if hover_was_focused {
            if let Some(element) = &entry.last_focused_element_before_open {
                self.deps.dom.focus(element);
            }
        }
        // Also dispose all nested hovers (hovers at higher indices in the stack). Dispose from end to avoid index shifting issues
        loop {
            let nested = {
                let mut state = self.state.borrow_mut();
                if state.hover_stack.len() > stack_index + 1 { state.hover_stack.pop() } else { None }
            };
            let Some(nested) = nested else {
                break;
            };
            nested.context_view.dispose();
            nested.hover.dispose();
        }
        // Remove this hover from stack and dispose its context view
        {
            let mut state = self.state.borrow_mut();
            if let Some(index) = state.hover_stack.iter().position(|candidate| Rc::ptr_eq(&candidate.hover, hover)) {
                state.hover_stack.remove(index);
            }
        }
        entry.context_view.dispose();
    }

    fn show_hover(&self, result: &CreateHoverResult, options: &IHoverOptions, focus: bool) {
        let CreateHoverResult { hover, last_focused_element_before_open, store } = result;

        // Check if the target is inside an existing hover (nesting scenario)
        let containing_hover_index = self.get_containing_hover_index(&options.target);
        let is_nesting = containing_hover_index.is_some();

        match containing_hover_index {
            // If not nesting, close all existing hovers first
            None => self.hide_all_hovers(),
            // When nesting, close any sibling hovers (hovers at the same level or deeper than the containing hover).
            // This ensures hovers within the same container are exclusive.
            Some(containing) => {
                let siblings: Vec<Rc<dyn IHoverWidgetHandle>> = self.state.borrow().hover_stack.iter().skip(containing + 1).rev().map(|entry| entry.hover.clone()).collect();
                for sibling in siblings {
                    sibling.dispose();
                }
                self.state.borrow_mut().hover_stack.truncate(containing + 1);
            }
        }

        // When nesting, add the new hover's container to all parent hovers' mouse trackers. This makes the parent hovers
        // treat the nested hover as part of themselves, so they won't close when the mouse moves into the nested hover.
        if let (true, Some(containing)) = (is_nesting, containing_hover_index) {
            let parents: Vec<Rc<dyn IHoverWidgetHandle>> = self.state.borrow().hover_stack.iter().take(containing + 1).map(|entry| entry.hover.clone()).collect();
            for parent in parents {
                store.add(parent.add_mouse_tracking_element(&hover.dom_node()));
            }
        }

        // Create a new ContextView for this hover with higher z-index for nested hovers
        let container = options.container.clone().unwrap_or_else(|| self.deps.layout_service.get_container());
        let context_view = self.deps.context_views.create(container);

        // Push to stack
        let depth = {
            let mut state = self.state.borrow_mut();
            state.hover_stack.push(HoverStackEntry {
                hover: hover.clone(),
                options: options.clone(),
                context_view: context_view.clone(),
                last_focused_element_before_open: last_focused_element_before_open.clone(),
            });
            state.hover_stack.len()
        };

        // Show the hover in its context view
        let delegate: Rc<dyn IContextViewDelegate> = Rc::new(HoverContextViewDelegate::new(hover.clone(), focus, depth as i32));
        context_view.show(delegate);

        // Set up layout handling
        {
            let context_view = context_view.clone();
            store.add(hover.on_request_layout().listen(move |_| context_view.layout()));
        }

        // Re-layout when the window resizes so the hover tracks its anchor. Only for focused/sticky hovers that persist
        // long enough for a resize to matter; transient hovers dismiss on mouse movement anyway.
        if focus || options.is_sticky() {
            let context_view = context_view.clone();
            store.add(self.deps.dom.add_listener(HoverDomTarget::Window, HoverDomEventKind::Resize, false, Rc::new(move |_| context_view.layout())));
        }

        if let Some(on_did_hide) = options.on_did_hide.clone() {
            store.add(to_disposable(move || on_did_hide()));
        }
        if let Some(on_did_show) = &options.on_did_show {
            on_did_show();
        }
    }

    /// Hides a specific hover and all hovers nested inside it.
    fn hide_hover_and_descendants(&self, hover: &Rc<dyn IHoverWidgetHandle>) {
        let stack_index = self.state.borrow().hover_stack.iter().position(|entry| Rc::ptr_eq(&entry.hover, hover));
        let Some(stack_index) = stack_index else {
            // The hover is not on the stack, so it may still be waiting for its delay to elapse. Cancel it, otherwise it would
            // show up after this dismiss request.
            let is_pending = self.state.borrow().current_delayed_hover.as_ref().is_some_and(|delayed| Rc::ptr_eq(delayed, hover));
            if is_pending {
                self.cancel_pending_delayed_hover();
            }
            return;
        };

        // Dispose all hovers from this index onwards (including nested ones)
        let hovers: Vec<Rc<dyn IHoverWidgetHandle>> = self.state.borrow().hover_stack.iter().skip(stack_index).rev().map(|entry| entry.hover.clone()).collect();
        for hover in hovers {
            hover.dispose();
        }
        self.state.borrow_mut().hover_stack.truncate(stack_index);
    }

    /// Cancels a delayed hover that was created but whose delay has not elapsed yet, so it never gets shown.
    fn cancel_pending_delayed_hover(&self) {
        let delayed = {
            let mut state = self.state.borrow_mut();
            if state.current_delayed_hover.is_none() || state.current_delayed_hover_was_shown {
                return;
            }
            state.current_delayed_hover_group_id = None;
            state.current_delayed_hover.take()
        };
        if let Some(delayed) = delayed {
            delayed.dispose();
        }
    }

    /// Hides all hovers in the stack.
    fn hide_all_hovers(&self) {
        let hovers: Vec<Rc<dyn IHoverWidgetHandle>> = self.state.borrow().hover_stack.iter().rev().map(|entry| entry.hover.clone()).collect();
        for hover in hovers {
            hover.dispose();
        }
        self.state.borrow_mut().hover_stack.clear();
    }

    pub fn hide_hover(&self, force: bool) {
        // If not forcing and the topmost hover is locked, don't hide
        if !force && self.current_hover().is_some_and(|hover| hover.is_locked()) {
            return;
        }

        // A delayed hover that has not been shown yet is not part of the stack, so it has to be cancelled explicitly.
        // Otherwise it pops up once its delay elapses, on top of whatever took over in the meantime such as a context menu.
        self.cancel_pending_delayed_hover();

        if self.state.borrow().hover_stack.is_empty() {
            return;
        }

        // Hide only the topmost hover (pop from stack)
        self.do_hide_hover();
    }

    pub fn get_sticky_hover(&self, target_element: &Rc<ViewElement>) -> Option<Rc<dyn IHoverWidgetHandle>> {
        let state = self.state.borrow();
        state
            .hover_stack
            .iter()
            .find(|entry| {
                entry.options.is_sticky()
                    && match &entry.options.target {
                        HoverTarget::Element(element) => Rc::ptr_eq(element, target_element),
                        HoverTarget::Structured(target) => target.target_elements.iter().any(|element| Rc::ptr_eq(element, target_element)),
                    }
            })
            .map(|entry| entry.hover.clone())
    }

    fn do_hide_hover(&self) {
        // Pop and dispose the topmost hover
        // O tamanho é capturado antes do dispose, como `const length` no original: o dispose já tira o hover da pilha pelo
        // `onDispose`, e truncar pelo tamanho atual removeria também o pai.
        let (length, top) = {
            let state = self.state.borrow();
            (state.hover_stack.len(), state.hover_stack.last().map(|entry| entry.hover.clone()))
        };
        if let Some(top) = top {
            top.dispose();
        }
        self.state.borrow_mut().hover_stack.truncate(length.saturating_sub(1));
    }

    pub fn show_and_focus_last_hover(&self) {
        let last = self.state.borrow().last_hover_options.clone();
        let Some(last) = last else {
            return;
        };
        self.show_instant_hover(last, true, true);
    }

    fn show_and_focus_hover_for_active_element(&self) {
        // O original anota aqui que um hover já visível deveria receber o foco para evitar cintilação.
        let mut active_element = self.deps.dom.active_element();
        while let Some(element) = active_element {
            let delayed = self.state.borrow().delayed_hovers.iter().find(|(candidate, _)| Rc::ptr_eq(candidate, &element)).map(|(_, show)| show.clone());
            if let Some(show) = delayed {
                show(true);
                return;
            }
            let managed = self.state.borrow().managed_hovers.iter().find(|(candidate, _)| Rc::ptr_eq(candidate, &element)).map(|(_, hover)| hover.clone());
            if let Some(hover) = managed {
                hover.show(true);
                return;
            }
            active_element = element.parent_element();
        }
    }

    fn key_down(&self, e: &HoverDomEvent, hover: &Rc<dyn IHoverWidgetHandle>, hide_on_key_down: bool) {
        if e.key == "Alt" {
            // Lock all hovers in the stack when Alt is pressed
            let hovers: Vec<Rc<dyn IHoverWidgetHandle>> = self.state.borrow().hover_stack.iter().map(|entry| entry.hover.clone()).collect();
            for hover in hovers {
                hover.set_is_locked(true);
            }
            return;
        }
        let event = KeyboardEvent { ctrl_key: e.ctrl_key, shift_key: e.shift_key, alt_key: e.alt_key, meta_key: e.meta_key, alt_graph_key: false, key_code: e.key_code, code: None };
        let keybinding = self.deps.keybinding_service.resolve_keyboard_event(&event);
        if keybinding.get_single_modifier_dispatch_chords().iter().any(|value| value.is_some())
            || !matches!(self.deps.keybinding_service.soft_dispatch(&event, None), ResolutionResult::NoMatchingKb)
        {
            return;
        }
        let trap_focus = self.current_hover_options().is_some_and(|options| options.trap_focus);
        if hide_on_key_down && (!trap_focus || e.key != "Tab") {
            // Find the entry for this hover to get its lastFocusedElementBeforeOpen
            let last_focused = self.state.borrow().hover_stack.iter().find(|entry| Rc::ptr_eq(&entry.hover, hover)).and_then(|entry| entry.last_focused_element_before_open.clone());
            self.hide_hover_and_descendants(hover);
            if let Some(element) = last_focused {
                self.deps.dom.focus(&element);
            }
        }
    }

    fn key_up(&self, e: &HoverDomEvent) {
        if e.key != "Alt" {
            return;
        }
        // Unlock all hovers in the stack when Alt is released
        let entries: Vec<HoverStackEntry> = self.state.borrow().hover_stack.clone();
        for entry in &entries {
            // Only unlock if not sticky
            if !entry.options.is_sticky() {
                entry.hover.set_is_locked(false);
            }
        }
        // Hide all hovers if the mouse is not over any of them
        let any_mouse_in = entries.iter().any(|entry| entry.hover.is_mouse_in());
        if !any_mouse_in {
            let top = entries.last().and_then(|entry| entry.last_focused_element_before_open.clone());
            self.hide_all_hovers();
            if let Some(element) = top {
                self.deps.dom.focus(&element);
            }
        }
    }

    /// `setupManagedHover(hoverDelegate, targetElement, content, options)`.
    pub fn setup_managed_hover(&self, hover_delegate: Rc<dyn IHoverDelegate>, target_element: &Rc<ViewElement>, content: ManagedHoverContentOrFactory, options: Option<IManagedHoverOptions>) -> Rc<dyn IManagedHover> {
        if hover_delegate.show_native_hover() {
            return setup_native_hover(self.deps.dom.clone(), target_element, &content);
        }

        let dom = self.deps.dom.clone();
        dom.set_attribute(target_element, "custom-hover", "true");

        if dom.get_attribute(target_element, "title").is_some_and(|title| !title.is_empty()) {
            // O original avisa no console: o elemento já tem `title`, que conflita com o hover customizado.
            dom.set_attribute(target_element, "title", "");
        }

        let controller = Rc::new(ManagedHoverController {
            dom: dom.clone(),
            widgets: self.deps.managed_hover_widgets.clone(),
            delegate: hover_delegate,
            target_element: target_element.clone(),
            hover_preparation: RefCell::new(None),
            hover_widget: RefCell::new(None),
            content: RefCell::new(content),
            options: RefCell::new(options),
            is_mouse_down: Cell::new(false),
        });

        let store = DisposableStore::new();
        {
            let controller = controller.clone();
            store.add(dom.add_listener(
                HoverDomTarget::Element(target_element.clone()),
                HoverDomEventKind::MouseDown,
                true,
                Rc::new(move |_| {
                    controller.is_mouse_down.set(true);
                    controller.hide_hover(true, true);
                }),
            ));
        }
        {
            let controller = controller.clone();
            store.add(dom.add_listener(HoverDomTarget::Element(target_element.clone()), HoverDomEventKind::MouseUp, true, Rc::new(move |_| controller.is_mouse_down.set(false))));
        }
        {
            let controller = controller.clone();
            store.add(dom.add_listener(
                HoverDomTarget::Element(target_element.clone()),
                HoverDomEventKind::MouseLeave,
                true,
                Rc::new(move |e| {
                    controller.is_mouse_down.set(false);
                    // HACK: `fromElement` is a non-standard property. Not sure what to replace it with, `relatedTarget` is NOT equivalent.
                    let from_target = e.from_element.as_ref().is_some_and(|from| Rc::ptr_eq(from, &controller.target_element));
                    controller.hide_hover(false, from_target);
                }),
            ));
        }
        {
            let controller = controller.clone();
            store.add(dom.add_listener(
                HoverDomTarget::Element(target_element.clone()),
                HoverDomEventKind::MouseOver,
                true,
                Rc::new(move |e| controller.on_mouse_over(e)),
            ));
        }

        // Do not show hover when focusing an input or textarea
        if !dom.is_editable_element(target_element) {
            let controller = controller.clone();
            store.add(dom.add_listener(HoverDomTarget::Element(target_element.clone()), HoverDomEventKind::Focus, true, Rc::new(move |e| controller.on_focus(e))));
        }

        let hover: Rc<dyn IManagedHover> = Rc::new(ManagedHover { service: self.this.clone(), controller, store });
        self.state.borrow_mut().managed_hovers.push((target_element.clone(), hover.clone()));
        hover
    }

    pub fn show_managed_hover(&self, target: &Rc<ViewElement>) {
        let hover = self.state.borrow().managed_hovers.iter().find(|(candidate, _)| Rc::ptr_eq(candidate, target)).map(|(_, hover)| hover.clone());
        if let Some(hover) = hover {
            hover.show(true);
        }
    }
}

impl IDisposable for HoverService {
    fn dispose(&self) {
        let managed: Vec<Rc<dyn IManagedHover>> = self.state.borrow().managed_hovers.iter().map(|(_, hover)| hover.clone()).collect();
        for hover in managed {
            hover.dispose();
        }
        self.disposables.dispose();
    }
}

/// O estado de um hover gerenciável (as variáveis capturadas do `setupManagedHover`).
struct ManagedHoverController {
    dom: Rc<dyn IHoverDom>,
    widgets: Rc<dyn IManagedHoverWidgetFactory>,
    delegate: Rc<dyn IHoverDelegate>,
    target_element: Rc<ViewElement>,
    hover_preparation: RefCell<Option<DisposableRef>>,
    hover_widget: RefCell<Option<Rc<dyn IManagedHoverWidget>>>,
    content: RefCell<ManagedHoverContentOrFactory>,
    options: RefCell<Option<IManagedHoverOptions>>,
    is_mouse_down: Cell<bool>,
}

impl ManagedHoverController {
    fn hide_hover(&self, dispose_widget: bool, dispose_preparation: bool) {
        let had_hover = self.hover_widget.borrow().is_some();
        if dispose_widget {
            let widget = self.hover_widget.borrow_mut().take();
            if let Some(widget) = widget {
                widget.dispose();
            }
        }
        if dispose_preparation {
            let preparation = self.hover_preparation.borrow_mut().take();
            if let Some(preparation) = preparation {
                preparation.dispose();
            }
        }
        if had_hover {
            self.delegate.on_did_hide_hover();
        }
    }

    fn trigger_show_hover(self: &Rc<Self>, delay: f64, focus: Option<bool>, target: Option<Rc<HoverDelegateTarget>>, trap_focus: Option<bool>) -> Rc<TimeoutTimer> {
        let controller = self.clone();
        TimeoutTimer::with_runner(
            move || {
                let needs_widget = controller.hover_widget.borrow().as_ref().is_none_or(|widget| widget.is_disposed());
                if !needs_widget {
                    return;
                }
                let target = target.unwrap_or_else(|| Rc::new(HoverDelegateTarget { target_elements: vec![controller.target_element.clone()], x: Cell::new(None) }));
                let widget = controller.widgets.create(&controller.delegate, target, delay > 0.0);
                *controller.hover_widget.borrow_mut() = Some(widget.clone());
                let content = controller.content.borrow().resolve();
                let mut options = controller.options.borrow().clone().unwrap_or_default();
                options.trap_focus = trap_focus;
                let _ = widget.update(content, focus, Some(options));
            },
            if delay.is_finite() && delay > 0.0 { delay as u64 } else { 0 },
        )
    }

    fn current_delay(&self) -> f64 {
        self.delegate.delay().resolve(&self.content.borrow())
    }

    fn on_mouse_over(self: &Rc<Self>, e: &HoverDomEvent) {
        if self.hover_preparation.borrow().is_some() {
            return;
        }

        let mouse_over_store = DisposableStore::new();

        let target = Rc::new(HoverDelegateTarget { target_elements: vec![self.target_element.clone()], x: Cell::new(None) });
        if self.delegate.placement().is_none_or(|placement| placement == HoverPlacement::Mouse) {
            // track the mouse position
            let controller = self.clone();
            let tracked = target.clone();
            mouse_over_store.add(self.dom.add_listener(
                HoverDomTarget::Element(self.target_element.clone()),
                HoverDomEventKind::MouseMove,
                true,
                Rc::new(move |e| {
                    tracked.x.set(Some(e.x + 10.0));
                    if !controller.event_is_related_to_target(e) {
                        controller.hide_hover(true, true);
                    }
                }),
            ));
        }

        *self.hover_preparation.borrow_mut() = Some(mouse_over_store.clone());

        if !self.event_is_related_to_target(e) {
            return; // Do not show hover when the mouse is over another hover target
        }

        mouse_over_store.add_rc(self.trigger_show_hover(self.current_delay(), Some(false), Some(target), None));
    }

    fn on_focus(self: &Rc<Self>, e: &HoverDomEvent) {
        if self.is_mouse_down.get() || self.hover_preparation.borrow().is_some() {
            return;
        }
        // Clean up stale reference if the hover was dismissed externally
        if self.hover_widget.borrow().as_ref().is_some_and(|widget| widget.is_disposed()) {
            *self.hover_widget.borrow_mut() = None;
        }
        // If focus is returning from a dismissed hover (e.g. Esc) or from window reactivation (e.g. Alt-tab), don't re-show.
        let from_hover = e.related_target.as_ref().is_some_and(|related| related.closest("monaco-hover").is_some());
        if from_hover || e.related_target.is_none() {
            return;
        }
        if !self.event_is_related_to_target(e) {
            return; // Do not show hover when the focus is on another hover target
        }

        let target = Rc::new(HoverDelegateTarget { target_elements: vec![self.target_element.clone()], x: Cell::new(None) });
        let to_dispose = DisposableStore::new();
        let controller = self.clone();
        to_dispose.add(self.dom.add_listener(HoverDomTarget::Element(self.target_element.clone()), HoverDomEventKind::Blur, true, Rc::new(move |_| controller.hide_hover(true, true))));
        to_dispose.add_rc(self.trigger_show_hover(self.current_delay(), Some(false), Some(target), None));
        *self.hover_preparation.borrow_mut() = Some(to_dispose);
    }

    /// `eventIsRelatedToTarget(event, target)`.
    fn event_is_related_to_target(&self, event: &HoverDomEvent) -> bool {
        let Some(target) = &event.target else {
            return false;
        };
        let found = get_hover_target_element(self.dom.as_ref(), target, Some(&self.target_element));
        found.is_some_and(|found| Rc::ptr_eq(&found, &self.target_element))
    }
}

/// `getHoverTargetElement(element, stopElement)`: sobe até um elemento com `custom-hover` ou até o elemento de parada.
fn get_hover_target_element(dom: &dyn IHoverDom, element: &Rc<ViewElement>, stop_element: Option<&Rc<ViewElement>>) -> Option<Rc<ViewElement>> {
    let mut current = element.clone();
    loop {
        if dom.has_attribute(&current, "custom-hover") || stop_element.is_some_and(|stop| Rc::ptr_eq(&current, stop)) {
            return Some(current);
        }
        current = current.parent_element()?;
    }
}

/// O `IManagedHover` que `setupManagedHover` devolve.
struct ManagedHover {
    service: Weak<HoverService>,
    controller: Rc<ManagedHoverController>,
    store: Rc<DisposableStore>,
}

impl IDisposable for ManagedHover {
    fn dispose(&self) {
        if let Some(service) = self.service.upgrade() {
            service.state.borrow_mut().managed_hovers.retain(|(element, _)| !Rc::ptr_eq(element, &self.controller.target_element));
        }
        self.store.dispose();
        self.controller.hide_hover(true, true);
    }
}

impl IManagedHover for ManagedHover {
    fn show(&self, focus: bool) {
        // terminate an ongoing mouse over preparation and recreate the hover with the requested focus
        self.controller.hide_hover(true, true);
        // show hover immediately
        let _ = self.controller.trigger_show_hover(0.0, Some(focus), None, Some(focus));
    }

    fn hide(&self) {
        self.controller.hide_hover(true, true);
    }

    fn update(&self, new_content: ManagedHoverContentOrFactory, hover_options: Option<IManagedHoverOptions>) -> Promise<()> {
        *self.controller.content.borrow_mut() = new_content;
        // Keep the options for the next time the hover is shown, so an updated tooltip does not keep rendering the actions it was created with.
        *self.controller.options.borrow_mut() = hover_options.clone();
        let widget = self.controller.hover_widget.borrow().clone();
        match widget {
            Some(widget) => widget.update(self.controller.content.borrow().resolve(), None, hover_options),
            None => Promise::resolved(()),
        }
    }
}

/// `getStringContent`: o texto do hover nativo.
fn get_string_content(content_or_factory: &ManagedHoverContentOrFactory) -> Option<String> {
    match content_or_factory.resolve() {
        // Icons don't render in the native hover so we strip them out
        ManagedHoverContent::Text(text) => {
            let units: Vec<u16> = text.encode_utf16().collect();
            Some(strip_icons(&units).to_string())
        }
        ManagedHoverContent::Tooltip(tooltip) => tooltip.markdown_not_supported_fallback,
        ManagedHoverContent::Undefined => None,
    }
}

/// `setupNativeHover(targetElement, content)`.
fn setup_native_hover(dom: Rc<dyn IHoverDom>, target_element: &Rc<ViewElement>, content: &ManagedHoverContentOrFactory) -> Rc<dyn IManagedHover> {
    let hover = NativeManagedHover { dom, target_element: target_element.clone() };
    hover.update_title(get_string_content(content));
    Rc::new(hover)
}

struct NativeManagedHover {
    dom: Rc<dyn IHoverDom>,
    target_element: Rc<ViewElement>,
}

impl NativeManagedHover {
    fn update_title(&self, title: Option<String>) {
        match title.filter(|title| !title.is_empty()) {
            Some(title) => self.dom.set_attribute(&self.target_element, "title", &title),
            None => self.dom.remove_attribute(&self.target_element, "title"),
        }
    }
}

impl IDisposable for NativeManagedHover {
    fn dispose(&self) {
        self.update_title(None);
    }
}

impl IManagedHover for NativeManagedHover {
    fn show(&self, _focus: bool) {}

    fn hide(&self) {}

    fn update(&self, content: ManagedHoverContentOrFactory, _options: Option<IManagedHoverOptions>) -> Promise<()> {
        self.update_title(get_string_content(&content));
        Promise::resolved(())
    }
}

/// `HoverContextViewDelegate`.
pub struct HoverContextViewDelegate {
    hover: Rc<dyn IHoverWidgetHandle>,
    focus: bool,
    /// Render over all other context views, with higher layers for nested hovers.
    layer: i32,
}

impl HoverContextViewDelegate {
    pub fn new(hover: Rc<dyn IHoverWidgetHandle>, focus: bool, stack_depth: i32) -> Self {
        // Base layer is 1, nested hovers get higher layers
        Self { hover, focus, layer: stack_depth }
    }
}

impl IContextViewDelegate for HoverContextViewDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        AnchorTarget::Anchor(IAnchor { x: self.hover.x(), y: self.hover.y(), width: None, height: None })
    }

    fn render(&self, container: &Rc<ViewElement>) -> Option<DisposableRef> {
        self.hover.render(container);
        if self.focus {
            self.hover.focus();
        }
        let hover = self.hover.clone();
        Some(to_disposable(move || hover.dispose()))
    }

    fn layout(&self) {
        self.hover.layout();
    }

    fn anchor_position(&self) -> Option<AnchorPosition> {
        Some(self.hover.anchor())
    }

    fn layer(&self) -> Option<i32> {
        Some(self.layer)
    }
}

/// `resolveMouseStyleHoverTarget(target, e)`.
fn resolve_mouse_style_hover_target(target: &Rc<ViewElement>, e: &HoverDomEvent) -> HoverTarget {
    HoverTarget::Structured(StructuredHoverTarget { target_elements: vec![target.clone()], x: Some(e.x + 10.0), y: None })
}
