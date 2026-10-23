//! Os contratos de hover da camada `base`: `hover.ts` (`IHoverDelegate2`, `IHoverOptions`,
//! `IManagedHover`...), `hoverDelegate.ts` (`IHoverDelegate`) e `hoverDelegateFactory.ts`/
//! `hoverDelegate2.ts` (os delegados globais que a camada de plataforma injeta).
//!
//! Só as interfaces e os globais. O `HoverWidget` (a caixa) é o módulo `hover`; o serviço de hover que
//! implementa [`IHoverDelegate2`] é da camada de plataforma (TODO(integração)). Por padrão os globais são
//! os do original sem serviço: nenhum hover aparece.
//!
//! Traduções: `HTMLElement` é [`NodeId`]; `IDisposable` é [`DisposableRef`]; `IMarkdownString | string |
//! HTMLElement` é [`HoverContent`]; as callbacks opcionais são `Option<Rc<dyn Fn..>>`; `anchorAlignment`
//! e `HoverPosition` ficam como estão no original (`AnchorAlignment` de `je_base::layout`).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::html_content::IMarkdownString;
use je_base::layout::AnchorAlignment;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_ui::NodeId;

/// O `linkHandler?(url)` das opções de hover.
pub type LinkHandler = Rc<dyn Fn(&str)>;

/// O `(content?) => number` do `delay` dinâmico.
pub type DynamicDelay = Rc<dyn Fn(Option<&ManagedHoverContentOrFactory>) -> f64>;

/// `HoverPosition` de `hoverWidget.ts`.
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
#[derive(Clone, Debug)]
pub enum HoverContent {
    Text(String),
    Markdown(IMarkdownString),
    Node(NodeId),
}

/// `IHoverTarget`: os elementos que o hover cobre e, opcionalmente, a posição do ponteiro.
pub trait IHoverTarget {
    fn target_elements(&self) -> Vec<NodeId>;
    fn x(&self) -> Option<f64> {
        None
    }
    fn y(&self) -> Option<f64> {
        None
    }
    fn dispose(&self) {}
}

/// `IHoverTarget | HTMLElement`.
#[derive(Clone)]
pub enum HoverTarget {
    Node(NodeId),
    Target(Rc<dyn IHoverTarget>),
}

/// `IHoverWidget`.
pub trait IHoverWidget: IDisposable {
    fn is_disposed(&self) -> bool;
}

/// `IHoverAction`.
#[derive(Clone)]
pub struct IHoverAction {
    pub label: String,
    pub command_id: String,
    pub icon_class: Option<String>,
    pub run: Rc<dyn Fn(NodeId)>,
}

#[derive(Clone, Default)]
pub struct IHoverPositionOptions {
    pub hover_position: Option<HoverPosition>,
    pub anchor_alignment: Option<AnchorAlignment>,
    pub force_position: Option<bool>,
    pub clipping_element: Option<NodeId>,
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

/// `IHoverOptions`.
#[derive(Clone)]
pub struct IHoverOptions {
    pub content: HoverContent,
    pub target: HoverTarget,
    pub container: Option<NodeId>,
    pub id: Option<String>,
    pub actions: Vec<IHoverAction>,
    pub additional_classes: Vec<String>,
    pub link_handler: Option<LinkHandler>,
    pub trap_focus: bool,
    pub style: Option<HoverStyle>,
    pub position: Option<IHoverPositionOptions>,
    pub persistence: Option<IHoverPersistenceOptions>,
    pub appearance: Option<IHoverAppearanceOptions>,
    pub on_did_show: Option<Rc<dyn Fn()>>,
    pub on_did_hide: Option<Rc<dyn Fn()>>,
}

impl IHoverOptions {
    pub fn new(content: HoverContent, target: HoverTarget) -> Self {
        Self { content, target, container: None, id: None, actions: Vec::new(), additional_classes: Vec::new(), link_handler: None, trap_focus: false, style: None, position: None, persistence: None, appearance: None, on_did_show: None, on_did_hide: None }
    }
}

/// `IDelayedHoverOptions`/`IDelayedHoverAtMouseOptions`: as opções sem o alvo (o alvo vem do método).
pub type IDelayedHoverOptions = IHoverOptions;

/// `IHoverLifecycleOptions`.
#[derive(Clone, Default)]
pub struct IHoverLifecycleOptions {
    pub group_id: Option<String>,
    pub reduced_delay: Option<bool>,
    pub setup_keyboard_events: Option<bool>,
}

/// `IManagedHoverContent`.
#[derive(Clone)]
pub enum ManagedHoverContent {
    Undefined,
    Text(String),
    /// `IManagedHoverTooltipMarkdownString` com o valor já resolvido (a forma assíncrona do original vira
    /// `TODO(integração)` no serviço de hover).
    Markdown { markdown: Option<HoverContent>, markdown_not_supported_fallback: Option<String> },
    Node(NodeId),
}

/// `IManagedHoverContentOrFactory`.
#[derive(Clone)]
pub enum ManagedHoverContentOrFactory {
    Content(ManagedHoverContent),
    Factory(Rc<dyn Fn() -> ManagedHoverContent>),
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
    pub link_handler: Option<LinkHandler>,
    pub trap_focus: bool,
    pub on_did_show: Option<Rc<dyn Fn()>>,
    pub show_hover_hint: Option<bool>,
    pub anchor_alignment: Option<AnchorAlignment>,
}

/// `IManagedHover`.
pub trait IManagedHover: IDisposable {
    fn show(&self, focus: bool);
    fn hide(&self);
    fn update(&self, tooltip: ManagedHoverContentOrFactory, options: Option<IManagedHoverOptions>);
}

/// `IHoverDelegateTarget`.
pub trait IHoverDelegateTarget: IDisposable {
    fn target_elements(&self) -> Vec<NodeId>;
    fn x(&self) -> Option<f64> {
        None
    }
}

/// `IHoverDelegateOptions`: as opções de [`IManagedHoverOptions`] mais conteúdo e alvo.
#[derive(Clone)]
pub struct IHoverDelegateOptions {
    pub managed: IManagedHoverOptions,
    pub content: HoverContent,
    pub target: HoverTarget,
    pub container: Option<NodeId>,
    pub hover_position: Option<HoverPosition>,
    pub show_pointer: Option<bool>,
    pub skip_fade_in_animation: Option<bool>,
}

/// O `delay: number | ((content?) => number)` do `IHoverDelegate`.
#[derive(Clone)]
pub enum HoverDelay {
    Fixed(f64),
    Dynamic(DynamicDelay),
}

impl HoverDelay {
    pub fn resolve(&self, content: Option<&ManagedHoverContentOrFactory>) -> f64 {
        match self {
            Self::Fixed(delay) => *delay,
            Self::Dynamic(f) => f(content),
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
    fn show_hover(&self, options: IHoverDelegateOptions, focus: bool) -> Option<Rc<dyn IHoverWidget>>;
    fn on_did_hide_hover(&self) {}
    fn delay(&self) -> HoverDelay;
    fn placement(&self) -> Option<HoverPlacement> {
        None
    }
    fn show_native_hover(&self) -> bool {
        false
    }
}

/// `IScopedHoverDelegate`: um delegado que o consumidor descarta.
pub trait IScopedHoverDelegate: IHoverDelegate + IDisposable {}

/// `IHoverDelegate2`: o serviço de hover.
pub trait IHoverDelegate2 {
    fn show_delayed_hover(&self, options: IDelayedHoverOptions, target: HoverTarget, lifecycle: IHoverLifecycleOptions) -> Option<Rc<dyn IHoverWidget>>;
    fn setup_delayed_hover(&self, target: NodeId, options: Rc<dyn Fn() -> IDelayedHoverOptions>, lifecycle: Option<IHoverLifecycleOptions>) -> DisposableRef;
    fn setup_delayed_hover_at_mouse(&self, target: NodeId, options: Rc<dyn Fn() -> IDelayedHoverOptions>, lifecycle: Option<IHoverLifecycleOptions>) -> DisposableRef;
    fn show_instant_hover(&self, options: IHoverOptions, focus: bool) -> Option<Rc<dyn IHoverWidget>>;
    fn hide_hover(&self, force: bool);
    fn show_and_focus_last_hover(&self);
    fn setup_managed_hover(&self, delegate: Rc<dyn IHoverDelegate>, target: NodeId, content: ManagedHoverContentOrFactory, options: Option<IManagedHoverOptions>) -> Rc<dyn IManagedHover>;
    fn show_managed_hover(&self, target: NodeId);
}

struct NullManagedHover;
impl IDisposable for NullManagedHover {
    fn dispose(&self) {}
}
impl IManagedHover for NullManagedHover {
    fn show(&self, _focus: bool) {}
    fn hide(&self) {}
    fn update(&self, _tooltip: ManagedHoverContentOrFactory, _options: Option<IManagedHoverOptions>) {}
}

/// O delegado base do original: nada aparece.
struct NullHoverDelegate2;

impl IHoverDelegate2 for NullHoverDelegate2 {
    fn show_delayed_hover(&self, _: IDelayedHoverOptions, _: HoverTarget, _: IHoverLifecycleOptions) -> Option<Rc<dyn IHoverWidget>> {
        None
    }
    fn setup_delayed_hover(&self, _: NodeId, _: Rc<dyn Fn() -> IDelayedHoverOptions>, _: Option<IHoverLifecycleOptions>) -> DisposableRef {
        je_base::lifecycle::none()
    }
    fn setup_delayed_hover_at_mouse(&self, _: NodeId, _: Rc<dyn Fn() -> IDelayedHoverOptions>, _: Option<IHoverLifecycleOptions>) -> DisposableRef {
        je_base::lifecycle::none()
    }
    fn show_instant_hover(&self, _: IHoverOptions, _: bool) -> Option<Rc<dyn IHoverWidget>> {
        None
    }
    fn hide_hover(&self, _: bool) {}
    fn show_and_focus_last_hover(&self) {}
    fn setup_managed_hover(&self, _: Rc<dyn IHoverDelegate>, _: NodeId, _: ManagedHoverContentOrFactory, _: Option<IManagedHoverOptions>) -> Rc<dyn IManagedHover> {
        Rc::new(NullManagedHover)
    }
    fn show_managed_hover(&self, _: NodeId) {}
}

/// O delegado nulo do `hoverDelegateFactory.ts`: `delay` -1 e `showHover` sem efeito.
struct NullHoverDelegate;

impl IDisposable for NullHoverDelegate {
    fn dispose(&self) {}
}
impl IHoverDelegate for NullHoverDelegate {
    fn show_hover(&self, _: IHoverDelegateOptions, _: bool) -> Option<Rc<dyn IHoverWidget>> {
        None
    }
    fn delay(&self) -> HoverDelay {
        HoverDelay::Fixed(-1.0)
    }
}
impl IScopedHoverDelegate for NullHoverDelegate {}

type HoverDelegateFactory = Rc<dyn Fn(HoverPlacement, bool) -> Rc<dyn IScopedHoverDelegate>>;

thread_local! {
    static BASE_HOVER_DELEGATE: RefCell<Rc<dyn IHoverDelegate2>> = RefCell::new(Rc::new(NullHoverDelegate2));
    static HOVER_DELEGATE_FACTORY: RefCell<HoverDelegateFactory> = RefCell::new(Rc::new(|_, _| Rc::new(NullHoverDelegate)));
    static DEFAULT_MOUSE: RefCell<Option<Rc<dyn IHoverDelegate>>> = const { RefCell::new(None) };
    static DEFAULT_ELEMENT: RefCell<Option<Rc<dyn IHoverDelegate>>> = const { RefCell::new(None) };
}

/// `setBaseLayerHoverDelegate`.
pub fn set_base_layer_hover_delegate(delegate: Rc<dyn IHoverDelegate2>) {
    BASE_HOVER_DELEGATE.with(|d| *d.borrow_mut() = delegate);
}

/// `getBaseLayerHoverDelegate`.
pub fn get_base_layer_hover_delegate() -> Rc<dyn IHoverDelegate2> {
    BASE_HOVER_DELEGATE.with(|d| d.borrow().clone())
}

/// `setHoverDelegateFactory`. Zera os delegados padrão já criados (o `Lazy` do original os cria uma vez;
/// quem troca a fábrica troca antes do primeiro uso).
pub fn set_hover_delegate_factory(factory: impl Fn(HoverPlacement, bool) -> Rc<dyn IScopedHoverDelegate> + 'static) {
    HOVER_DELEGATE_FACTORY.with(|f| *f.borrow_mut() = Rc::new(factory));
    DEFAULT_MOUSE.with(|d| *d.borrow_mut() = None);
    DEFAULT_ELEMENT.with(|d| *d.borrow_mut() = None);
}

/// `getDefaultHoverDelegate`.
pub fn get_default_hover_delegate(placement: HoverPlacement) -> Rc<dyn IHoverDelegate> {
    let slot = match placement {
        HoverPlacement::Mouse => &DEFAULT_MOUSE,
        HoverPlacement::Element => &DEFAULT_ELEMENT,
    };
    slot.with(|s| {
        s.borrow_mut()
            .get_or_insert_with(|| {
                let scoped = HOVER_DELEGATE_FACTORY.with(|f| f.borrow().clone())(placement, false);
                let delegate: Rc<dyn IHoverDelegate> = Rc::new(ScopedAsDelegate(scoped));
                delegate
            })
            .clone()
    })
}

/// `createInstantHoverDelegate`.
pub fn create_instant_hover_delegate() -> Rc<dyn IScopedHoverDelegate> {
    HOVER_DELEGATE_FACTORY.with(|f| f.borrow().clone())(HoverPlacement::Element, true)
}

/// Faz um `IScopedHoverDelegate` valer como `IHoverDelegate` (o upcast do TypeScript).
struct ScopedAsDelegate(Rc<dyn IScopedHoverDelegate>);

impl IHoverDelegate for ScopedAsDelegate {
    fn show_hover(&self, options: IHoverDelegateOptions, focus: bool) -> Option<Rc<dyn IHoverWidget>> {
        self.0.show_hover(options, focus)
    }
    fn on_did_hide_hover(&self) {
        self.0.on_did_hide_hover();
    }
    fn delay(&self) -> HoverDelay {
        self.0.delay()
    }
    fn placement(&self) -> Option<HoverPlacement> {
        self.0.placement()
    }
    fn show_native_hover(&self) -> bool {
        self.0.show_native_hover()
    }
}
