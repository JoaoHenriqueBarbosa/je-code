//! Porte de `src/vs/workbench/browser/parts/paneCompositePart.ts` (e de `media/paneCompositePart.css`): a
//! [`AbstractPaneCompositePart`], a base das partes de painéis (barra lateral, painel e barra auxiliar): a barra de composites no
//! título, no cabeçalho ou no rodapé, a mensagem de painel vazio, o arrastar e soltar, o menu de contexto do título, a abertura
//! de painéis e o layout flutuante do Modern UI.
//!
//! Divergências:
//! - **Herança vira composição.** `AbstractPaneCompositePart extends CompositePart<PaneComposite>` é a
//!   [`AbstractPaneCompositePart`] concreta, que contém a [`CompositePart`] e se apresenta a ela como um [`CompositePartBehavior`]
//!   (o repasse fraco `PaneCompositePartProxy`). O que `SidebarPart`, `PanelPart` e `AuxiliaryBarPart` sobrescrevem ou declaram
//!   abstrato é o trait [`PaneCompositePartBehavior`] (limites, `toJSON`, `snap`, `updateStyles`, `layout`, `setBoundarySashes`,
//!   `getTitleAreaDropDownAnchorAlignment`, `getToolbarResponsiveBehavior`, `createCompositeBar`, os três `getPinned...Ids`,
//!   `shouldShowCompositeBar`, `getCompositeBarOptions`, `getCompositeBarPosition` e `getFloatingBorderWidth`). A chamada a
//!   `super` de uma sobrescrita é o método `*_base` da parte (`update_styles_base`, `layout_base`, `set_boundary_sashes_base`,
//!   `create_composite_bar_base`, `get_*_ids_base`). O que o original deixa `protected` (`updateCompositeBar`,
//!   `getViewsSubmenuAction`, `focusCompositeBar`, `getToolbarWidth`, `contentDimension`, `onTitleAreaUpdate`...) é público,
//!   porque quem estende a parte é um [`PaneCompositePartBehavior`] que recebe a `&AbstractPaneCompositePart`.
//! - **`showComposite`, `hideActiveComposite` e `onTitleAreaUpdate` sobrescritos** são os ganchos `did_show_composite`,
//!   `did_hide_active_composite` e `did_title_area_update` do [`CompositePartBehavior`], chamados depois do `super`.
//! - **Promessas.** `openPaneComposite` e `doOpenPaneComposite` (`async`) devolvem a [`Promise`] do original; a parte `async` de
//!   `doOpenPaneComposite` não tem `await`, então corre inteira na chamada, e a corrida do `blockOpening` é a mesma (a chamada
//!   reentrante devolve a promessa da chamada original).
//! - **`PaneComposite`.** O painel é o [`IPaneComposite`] (ver `browser/pane_composite`): a parte guarda o painel de cada
//!   identificador instanciado (o [`PaneInstanceRegistry`] cria o painel e entrega à `CompositePart` o [`Composite`] dele), e
//!   `getOptimalWidth`, `getViewPaneContainer().viewContainer.id` e `openView` são membros do [`IPaneComposite`].
//! - **`PaneCompositeBar`.** A barra é o [`IPaneCompositeBar`], criada pela [`CompositeBarFactory`] que o `createInstance` da
//!   barra representa (o mesmo contrato da `ActivitybarPart`); o `dndHandler` da barra são os métodos `on_drag_enter` e `on_drop`
//!   dela.
//! - **DOM vira estado nativo.** Tudo que o original faz com o DOM e com widgets além do que o [`ViewElement`] guarda passa pela
//!   [`IPaneCompositePartView`] (texto, barra global, menu de contexto, quadro de animação, classes do contêiner do workbench e
//!   a posição dos nós no layout). O `Gesture.addTarget` e o `GestureEventType.Contextmenu` (toque longo) são o mesmo
//!   `on_context_menu` da vista. O `toggleDropEffect` só escreve o `dropEffect` do `DataTransfer`, que não tem estado nativo, e
//!   não é feito.
//! - **`getActionBarActions`/`getContextMenuActions`** são os do porte de `menu_entry_action_view_item`; o
//!   `getActionViewItem` do menu de contexto do título é da vista nativa do menu.
//! - **`mainWindow`.** As partes de painéis são de janela única, então `getWindow(this.element)` é [`MAIN_WINDOW`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IActionRunner, SubmenuAction};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::layout::AnchorAlignment;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::nls::localize;
use je_base::promise::{DeferredPromise, Promise};
use je_editor::code_action::platform::IProgressIndicator;
use je_editor::core_2d::dimension::Dimension;
use je_instantiation::InstantiationService;
use je_platform::actions::menu_entry_action_view_item::get_action_bar_actions;
use je_platform::actions::{IMenuService, MenuActionOptions, MenuId};
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService};
use je_platform::contextview::context_view::{AnchorTarget, ContextMenuRequest, IContextMenuDelegate, IContextMenuService};
use je_platform::contextview::view_element::ViewElement;
use je_widgets::sash::BoundarySashes;
use je_widgets::toolbar::IToolBarResponsiveBehaviorOptions;

use crate::browser::composite::{Composite, CompositeEntry, CompositeFactory, FocusTrackerFactory, ICompositeRegistry, IFocusTracker};
use crate::browser::dnd::{CompositeDragAndDropObserver, CompositeDragAndDropObserverCallbacks, CompositeDragData, DragAndDropHost, DraggedCompositeData, ViewType};
use crate::browser::part::{LayoutContentResult, Part, PartOptions, assert_returns_defined};
use crate::browser::pane_composite::{PaneCompositeDescriptor, PaneCompositeRegistry, pane_composite_registry};
use crate::browser::parts::activitybar::activitybar_part::{CompositeBarFactory, CompositeBarRequest};
use crate::browser::parts::composite_part::{
    CompositeOpenEvent, CompositePart, CompositePartBehavior, CompositePartServices, ICompositePartOptions, ICompositePartView, ICompositeTitleLabel,
    ICompositeToolBar, IToolBarItems,
};
use crate::browser::parts::pane_composite_bar::{ContentElement, IPaneCompositeBar, IPaneCompositePart, PaneCompositeBarOptions};
use crate::browser::parts::part_support::{ColorValue, ViewElementTarget, set_style_or_remove};
use crate::common::views::{IViewDescriptorService, ViewContainerLocation};
use crate::services::extensions::extensions::IExtensionService;
use crate::services::layout::layout_service::{
    HorizontalPair, ILayoutService, IWorkbenchLayoutService, LayoutContainerId, MAIN_WINDOW, Parts, VerticalPair, get_floating_outer_gutter_edges,
    get_floating_pane_composite_horizontal_margins, get_floating_pane_composite_vertical_margins, get_floating_pane_composite_vertical_outer_edges,
};
use crate::services::panecomposite::panecomposite::IPaneComposite;

/// `EDITOR_DRAG_AND_DROP_BACKGROUND` (`workbench/common/theme.ts`).
pub const EDITOR_DRAG_AND_DROP_BACKGROUND: &str = "editorGroup.dropBackground";


/// `ViewsSubMenu` (`views/viewPaneContainer.ts`): `new MenuId('Views')`.
pub fn views_sub_menu() -> MenuId {
    MenuId::for_id("Views")
}

/// A largura mínima da barra de composites (`MIN_COMPOSITE_BAR_WIDTH`).
pub const MIN_COMPOSITE_BAR_WIDTH: f64 = 50.0;

/// `CompositeBarPosition`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompositeBarPosition {
    Top,
    Title,
    Bottom,
}

/// Os argumentos do `createInstance(MenuWorkbenchToolBar, globalTitleActionsContainer, globalActionsMenuId, { ... })`. A orientação
/// é a horizontal, o `getKeyBinding` é a busca de atalho do serviço de atalhos, o `hiddenItemStrategy` é `NoHide` e o
/// `highlightToggledItems` é `true`.
pub struct GlobalToolBarArgs {
    /// O `.global-actions`.
    pub container: Rc<ViewElement>,
    /// O menu que alimenta a barra.
    pub menu_id: MenuId,
    /// `actionViewItemProvider`.
    pub action_view_item_provider: je_widgets::action_bar::IActionViewItemProvider,
    /// `anchorAlignmentProvider`.
    pub anchor_alignment_provider: Rc<dyn Fn() -> AnchorAlignment>,
    /// `toggleMenuTitle`.
    pub toggle_menu_title: String,
    /// `hoverDelegate`.
    pub hover_delegate: Rc<dyn je_widgets::hover_api::IHoverDelegate>,
    /// `telemetrySource`.
    pub telemetry_source: String,
}

/// O que a vista nativa recebe para posicionar os nós de uma parte depois do `layout`: o elemento, as áreas e os tamanhos que o
/// `layoutContents` calculou (o `size(element, w, h)` do `PartLayout` e o `display: flex` do CSS).
pub struct PaneCompositeLayout {
    pub part_element: Rc<ViewElement>,
    pub title_area: Option<Rc<ViewElement>>,
    pub header_area: Option<Rc<ViewElement>>,
    pub footer_area: Option<Rc<ViewElement>>,
    pub content_area: Option<Rc<ViewElement>>,
    pub empty_pane_message_area: Option<Rc<ViewElement>>,
    pub width: f64,
    pub height: f64,
    pub result: LayoutContentResult,
}

/// Tudo que a [`AbstractPaneCompositePart`] faz com o DOM além da [`ICompositePartView`](crate::browser::parts::composite_part::ICompositePartView)
/// (ver as divergências do módulo).
pub trait IPaneCompositePartView {
    /// `element.textContent = text`.
    fn set_text_content(&self, element: &Rc<ViewElement>, text: &str);

    /// `createInstance(MenuWorkbenchToolBar, container, menuId, options)`.
    fn create_global_tool_bar(&self, args: GlobalToolBarArgs) -> Rc<dyn IToolBarItems>;

    /// O `contextmenu` do elemento (e o toque longo): o ponteiro vira a âncora do menu.
    fn on_context_menu(&self, element: &Rc<ViewElement>, handler: Rc<dyn Fn(AnchorTarget)>) -> DisposableRef;

    /// `scheduleAtNextAnimationFrame(window, runner)`.
    fn schedule_at_next_animation_frame(&self, runner: Box<dyn FnOnce()>) -> DisposableRef;

    /// `container.classList.toggle(name, force)` do contêiner do workbench.
    fn toggle_container_class(&self, container: LayoutContainerId, class_name: &str, force: bool);

    /// `computeScreenAwareSize(window, 1)`: a largura de 1px da borda na densidade de pixels da tela.
    fn get_floating_border_width(&self) -> f64;

    /// O elemento onde a barra de composites se cria, ligado ao nó nativo do `element`.
    fn content_element_for(&self, element: &Rc<ViewElement>) -> Rc<ContentElement>;

    /// Posiciona os nós da parte conforme o `layout`.
    fn layout_part(&self, layout: &PaneCompositeLayout);
}

/// Os serviços que o construtor da `AbstractPaneCompositePart` injeta além dos da [`CompositePart`].
#[derive(Clone)]
pub struct PaneCompositePartServices {
    pub composite: CompositePartServices,
    pub pane_view: Rc<dyn IPaneCompositePartView>,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    pub view_descriptor_service: Rc<dyn IViewDescriptorService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub extension_service: Rc<dyn IExtensionService>,
    pub menu_service: Rc<dyn IMenuService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    /// `createInstance(PaneCompositeBar, ...)` (e `createInstance(ActivityBarCompositeBar, ...)` da barra lateral).
    pub composite_bar_factory: CompositeBarFactory,
    /// O que liga os eventos de arrasto aos elementos.
    pub drag_and_drop_host: Rc<dyn DragAndDropHost>,
    /// `trackFocus(parent)`; sem fábrica, os contextos de foco nunca mudam.
    pub focus_tracker_factory: Option<FocusTrackerFactory>,
}

/// Os argumentos do construtor da `AbstractPaneCompositePart` que cada subclasse escolhe.
pub struct PaneCompositePartArgs {
    pub part_id: Parts,
    pub part_options: PartOptions,
    pub trailing_separator: bool,
    pub active_pane_composite_settings_key: String,
    pub active_pane_context_key: Rc<dyn IContextKey>,
    pub pane_focus_context_key: Rc<dyn IContextKey>,
    pub name_for_telemetry: String,
    pub composite_css_class: String,
    pub title_foreground_color: Option<String>,
    pub title_border_color: Option<String>,
    pub location: ViewContainerLocation,
    pub registry_id: String,
    pub global_actions_menu_id: MenuId,
}

/// O que as subclasses de `AbstractPaneCompositePart` sobrescrevem ou declaram abstrato. Os padrões são os do original.
pub trait PaneCompositePartBehavior {
    /// `minimumWidth`.
    fn minimum_width(&self) -> f64;

    /// `maximumWidth`.
    fn maximum_width(&self) -> f64;

    /// `minimumHeight`.
    fn minimum_height(&self) -> f64;

    /// `maximumHeight`.
    fn maximum_height(&self) -> f64;

    /// `toJSON()`.
    fn to_json(&self) -> Dynamic;

    /// `snap`: always allow snapping closed; only allow dragging open if the panel contains view containers.
    fn snap(&self, part: &AbstractPaneCompositePart) -> bool {
        part.layout_service().is_visible(part.part_id(), None) || !part.pane_composite_bar_visible_ids().is_empty()
    }

    /// `updateStyles()`: o padrão é o `CompositePart.updateStyles`.
    fn update_styles(&self, part: &AbstractPaneCompositePart) {
        part.update_styles_base();
    }

    /// `layout(width, height, top, left)`: o padrão é o `AbstractPaneCompositePart.layout`.
    fn layout(&self, part: &AbstractPaneCompositePart, width: f64, height: f64, top: f64, left: f64) {
        part.layout_base(width, height, top, left);
    }

    /// `setBoundarySashes(sashes)`: o padrão é o `CompositePart.setBoundarySashes`.
    fn set_boundary_sashes(&self, part: &AbstractPaneCompositePart, sashes: &BoundarySashes) {
        part.set_boundary_sashes_base(sashes);
    }

    /// `getTitleAreaDropDownAnchorAlignment()`.
    fn get_title_area_drop_down_anchor_alignment(&self, _part: &AbstractPaneCompositePart) -> AnchorAlignment {
        AnchorAlignment::Right
    }

    /// `getToolbarResponsiveBehavior()`.
    fn get_toolbar_responsive_behavior(&self) -> Option<IToolBarResponsiveBehaviorOptions> {
        None
    }

    /// `createCompositeBar()`: o padrão é o `PaneCompositeBar`.
    fn create_composite_bar(&self, part: &AbstractPaneCompositePart) -> Rc<dyn IPaneCompositeBar> {
        part.create_composite_bar_base()
    }

    /// `getPinnedPaneCompositeIds()`.
    fn get_pinned_pane_composite_ids(&self, part: &AbstractPaneCompositePart) -> Vec<String> {
        part.get_pinned_pane_composite_ids_base()
    }

    /// `getVisiblePaneCompositeIds()`.
    fn get_visible_pane_composite_ids(&self, part: &AbstractPaneCompositePart) -> Vec<String> {
        part.get_visible_pane_composite_ids_base()
    }

    /// `getPaneCompositeIds()`.
    fn get_pane_composite_ids(&self, part: &AbstractPaneCompositePart) -> Vec<String> {
        part.get_pane_composite_ids_base()
    }

    /// `getFloatingBorderWidth()`: o padrão é a borda de 1px na densidade de pixels da tela.
    fn get_floating_border_width(&self, part: &AbstractPaneCompositePart) -> f64 {
        part.pane_view.get_floating_border_width()
    }

    /// `shouldShowCompositeBar()`.
    fn should_show_composite_bar(&self, part: &AbstractPaneCompositePart) -> bool;

    /// `getCompositeBarOptions()`.
    fn get_composite_bar_options(&self, part: &AbstractPaneCompositePart) -> PaneCompositeBarOptions;

    /// `getCompositeBarPosition()`.
    fn get_composite_bar_position(&self, part: &AbstractPaneCompositePart) -> CompositeBarPosition;
}

/// O registro que a `CompositePart` consulta: instancia o painel do identificador, guarda o painel (para devolvê-lo em
/// `openPaneComposite`) e entrega à parte o [`Composite`] dele.
struct PaneInstanceRegistry {
    registry: Rc<PaneCompositeRegistry>,
    instances: Rc<RefCell<HashMap<String, Rc<dyn IPaneComposite>>>>,
}

impl ICompositeRegistry for PaneInstanceRegistry {
    fn get_composite_entry(&self, id: &str) -> Option<CompositeEntry> {
        let descriptor = self.registry.get_pane_composite(id)?;
        let instances = self.instances.clone();
        let (entry_id, name) = (descriptor.id.clone(), descriptor.name.clone());
        let instantiate: CompositeFactory<Composite> = Rc::new(move |instantiation_service: &Rc<InstantiationService>| {
            let pane = descriptor.instantiate(instantiation_service);
            let composite = assert_returns_defined(pane.composite());
            instances.borrow_mut().insert(descriptor.id.clone(), pane);
            composite
        });
        Some(CompositeEntry::new(&entry_id, &name, instantiate))
    }
}

/// O [`IPaneCompositePart`] que a barra de composites recebe: um repasse fraco, para a parte não ficar presa pela própria barra.
struct WeakPaneCompositePart {
    part: Weak<AbstractPaneCompositePart>,
    part_id: Parts,
}

impl IPaneCompositePart for WeakPaneCompositePart {
    fn part_id(&self) -> Parts {
        self.part_id
    }

    fn get_pinned_pane_composite_ids(&self) -> Vec<String> {
        self.part.upgrade().map(|part| part.get_pinned_pane_composite_ids()).unwrap_or_default()
    }

    fn get_visible_pane_composite_ids(&self) -> Vec<String> {
        self.part.upgrade().map(|part| part.get_visible_pane_composite_ids()).unwrap_or_default()
    }

    fn get_pane_composite_ids(&self) -> Vec<String> {
        self.part.upgrade().map(|part| part.get_pane_composite_ids()).unwrap_or_default()
    }
}

/// O [`CompositePartBehavior`] que a [`CompositePart`] recebe: repassa tudo à [`AbstractPaneCompositePart`] que a contém.
struct PaneCompositePartProxy {
    part: Weak<AbstractPaneCompositePart>,
    behavior: Rc<dyn PaneCompositePartBehavior>,
}

impl CompositePartBehavior for PaneCompositePartProxy {
    fn minimum_width(&self) -> f64 {
        self.behavior.minimum_width()
    }

    fn maximum_width(&self) -> f64 {
        self.behavior.maximum_width()
    }

    fn minimum_height(&self) -> f64 {
        self.behavior.minimum_height()
    }

    fn maximum_height(&self) -> f64 {
        self.behavior.maximum_height()
    }

    fn to_json(&self) -> Dynamic {
        self.behavior.to_json()
    }

    fn relayout_dimension(&self, part: &Part) -> Option<Dimension> {
        // `this.floatingLayoutDimension ?? super.getRelayoutDimension()`
        self.part.upgrade().and_then(|this| this.state.borrow().floating_layout_dimension).or_else(|| part.dimension())
    }

    fn create_title_area(&self, part: &CompositePart, parent: &Rc<ViewElement>) -> Option<Rc<ViewElement>> {
        let title_area = part.create_title_area_base(parent)?;
        if let Some(this) = self.part.upgrade() {
            this.create_title_area_extras(&title_area);
        }
        Some(title_area)
    }

    fn create_title_label(&self, part: &CompositePart, parent: &Rc<ViewElement>) -> Rc<dyn ICompositeTitleLabel> {
        if let Some(this) = self.part.upgrade() {
            this.state.borrow_mut().title_container = Some(parent.clone());
        }

        let title_label = part.create_title_label_base(parent);
        if let Some(this) = self.part.upgrade() {
            this.create_title_label_extras(&title_label);
        }
        title_label
    }

    fn get_toolbar_responsive_behavior(&self) -> Option<IToolBarResponsiveBehaviorOptions> {
        self.behavior.get_toolbar_responsive_behavior()
    }

    fn get_title_area_drop_down_anchor_alignment(&self) -> AnchorAlignment {
        match self.part.upgrade() {
            Some(this) => self.behavior.get_title_area_drop_down_anchor_alignment(&this),
            None => AnchorAlignment::Right,
        }
    }

    fn update_styles(&self, _part: &CompositePart) {
        if let Some(this) = self.part.upgrade() {
            self.behavior.update_styles(&this);
        }
    }

    fn layout(&self, _part: &CompositePart, width: f64, height: f64, top: f64, left: f64) {
        if let Some(this) = self.part.upgrade() {
            self.behavior.layout(&this, width, height, top, left);
        }
    }

    fn did_show_composite(&self, _part: &CompositePart) {
        if let Some(this) = self.part.upgrade() {
            this.layout_composite_bar();
            this.layout_empty_message();
        }
    }

    fn did_hide_active_composite(&self, _part: &CompositePart) {
        if let Some(this) = self.part.upgrade() {
            this.layout_composite_bar();
            this.layout_empty_message();
        }
    }

    fn did_title_area_update(&self, _part: &CompositePart, _composite_id: &str) {
        if let Some(this) = self.part.upgrade() {
            this.schedule_composite_bar_layout();
        }
    }
}

#[derive(Default)]
struct PaneCompositePartState {
    title_container: Option<Rc<ViewElement>>,
    header_footer_composite_bar_container: Option<Rc<ViewElement>>,
    pane_composite_bar_container: Option<Rc<ViewElement>>,
    composite_bar_position: Option<CompositeBarPosition>,
    empty_pane_message_element: Option<Rc<ViewElement>>,
    global_tool_bar: Option<Rc<dyn IToolBarItems>>,
    block_opening: Option<Rc<DeferredPromise<Option<Rc<dyn IPaneComposite>>>>>,
    content_dimension: Option<Dimension>,
    floating_layout_dimension: Option<Dimension>,
}

/// O menu de contexto do título: as ações do painel ativo ou da barra de composites, ancoradas no ponteiro.
struct TitleContextMenuDelegate {
    anchor: AnchorTarget,
    actions: Vec<ActionRef>,
    action_runner: Option<Rc<dyn IActionRunner>>,
}

impl IContextMenuDelegate for TitleContextMenuDelegate {
    fn get_anchor(&self) -> AnchorTarget {
        self.anchor.clone()
    }

    fn get_actions(&self) -> Vec<ActionRef> {
        self.actions.clone()
    }

    fn action_runner(&self) -> Option<Rc<dyn IActionRunner>> {
        self.action_runner.clone()
    }

    fn skip_telemetry(&self) -> bool {
        true
    }
}

/// `AbstractPaneCompositePart`.
pub struct AbstractPaneCompositePart {
    weak_self: Weak<AbstractPaneCompositePart>,
    composite_part: Rc<CompositePart>,
    base: Rc<Disposable>,
    part_id: Parts,
    registry_id: String,
    location: ViewContainerLocation,
    global_actions_menu_id: MenuId,
    active_pane_context_key: Rc<dyn IContextKey>,
    pane_focus_context_key: Rc<dyn IContextKey>,
    services: PaneCompositePartServices,
    pane_view: Rc<dyn IPaneCompositePartView>,
    registry: Rc<PaneCompositeRegistry>,
    instances: Rc<RefCell<HashMap<String, Rc<dyn IPaneComposite>>>>,
    behavior: Rc<dyn PaneCompositePartBehavior>,
    on_did_pane_composite_open: Rc<Emitter<Rc<dyn IPaneComposite>>>,
    on_did_pane_composite_close: Rc<Emitter<Rc<dyn IPaneComposite>>>,
    header_footer_composite_bar_disposables: Rc<DisposableStore>,
    pane_composite_bar: Rc<MutableDisposable<dyn IPaneCompositeBar>>,
    pending_composite_bar_layout: Rc<MutableDisposable<dyn IDisposable>>,
    state: RefCell<PaneCompositePartState>,
}

impl AbstractPaneCompositePart {
    /// `super(...)` e `registerListeners()`, com o que a subclasse sobrescreve em `behavior`.
    pub fn new(args: PaneCompositePartArgs, services: PaneCompositePartServices, behavior: Rc<dyn PaneCompositePartBehavior>) -> Rc<Self> {
        let PaneCompositePartArgs {
            part_id,
            part_options,
            trailing_separator,
            active_pane_composite_settings_key,
            active_pane_context_key,
            pane_focus_context_key,
            name_for_telemetry,
            composite_css_class,
            title_foreground_color,
            title_border_color,
            location,
            registry_id,
            global_actions_menu_id,
        } = args;

        let registry = pane_composite_registry(&registry_id);
        let instances: Rc<RefCell<HashMap<String, Rc<dyn IPaneComposite>>>> = Rc::new(RefCell::new(HashMap::new()));
        let composite_registry: Rc<dyn ICompositeRegistry> = Rc::new(PaneInstanceRegistry { registry: registry.clone(), instances: instances.clone() });
        let default_composite_id = services.view_descriptor_service.get_default_view_container(location).map(|container| container.id.clone()).unwrap_or_default();

        let base = Disposable::new();
        let on_did_pane_composite_open: Rc<Emitter<Rc<dyn IPaneComposite>>> = Emitter::new();
        base.register(on_did_pane_composite_open.clone());
        let on_did_pane_composite_close: Rc<Emitter<Rc<dyn IPaneComposite>>> = Emitter::new();
        base.register(on_did_pane_composite_close.clone());
        let header_footer_composite_bar_disposables = DisposableStore::new();
        base.register(header_footer_composite_bar_disposables.clone());
        let pane_composite_bar = MutableDisposable::<dyn IPaneCompositeBar>::new();
        base.register(pane_composite_bar.clone());
        let pending_composite_bar_layout = MutableDisposable::<dyn IDisposable>::new();
        base.register(pending_composite_bar_layout.clone());

        let this = Rc::new_cyclic(|weak: &Weak<AbstractPaneCompositePart>| {
            let proxy: Rc<dyn CompositePartBehavior> = Rc::new(PaneCompositePartProxy { part: weak.clone(), behavior: behavior.clone() });
            let composite_part = CompositePart::new(
                services.composite.clone(),
                composite_registry,
                &active_pane_composite_settings_key,
                &default_composite_id,
                &name_for_telemetry,
                &composite_css_class,
                title_foreground_color.as_deref(),
                title_border_color.as_deref(),
                part_id.as_str(),
                ICompositePartOptions { part: part_options, trailing_separator },
                proxy,
            );

            AbstractPaneCompositePart {
                weak_self: weak.clone(),
                composite_part,
                base,
                part_id,
                registry_id,
                location,
                global_actions_menu_id,
                active_pane_context_key,
                pane_focus_context_key,
                pane_view: services.pane_view.clone(),
                services: services.clone(),
                registry,
                instances,
                behavior,
                on_did_pane_composite_open,
                on_did_pane_composite_close,
                header_footer_composite_bar_disposables,
                pane_composite_bar,
                pending_composite_bar_layout,
                state: RefCell::new(PaneCompositePartState::default()),
            }
        });

        this.register_listeners();
        this
    }

    fn register_listeners(&self) {
        let weak = self.weak_self.clone();
        self.base.register(self.composite_part.on_did_composite_open_emitter().event().listen(move |event: &CompositeOpenEvent| {
            let Some(this) = weak.upgrade() else { return };
            this.fire_pane_composite(&event.composite, true);
        }));
        let weak = self.weak_self.clone();
        self.base.register(self.composite_part.on_did_composite_close_emitter().event().listen(move |composite: &Rc<Composite>| {
            let Some(this) = weak.upgrade() else { return };
            this.fire_pane_composite(composite, false);
        }));

        let weak = self.weak_self.clone();
        self.base.register(self.registry.on_did_deregister().listen(move |viewlet_descriptor: &PaneCompositeDescriptor| {
            let Some(this) = weak.upgrade() else { return };

            let active_containers: Vec<_> = this
                .services
                .view_descriptor_service
                .get_view_containers_by_location(this.location)
                .into_iter()
                .filter(|container| !this.services.view_descriptor_service.get_view_container_model(container).active_view_descriptors().is_empty())
                .collect();

            if !active_containers.is_empty() {
                if this.get_active_composite_id().as_deref() == Some(viewlet_descriptor.id.as_str()) {
                    let default_viewlet_id = this.services.view_descriptor_service.get_default_view_container(this.location).map(|container| container.id.clone());
                    let container_to_open = active_containers
                        .iter()
                        .find(|container| Some(&container.id) == default_viewlet_id.as_ref())
                        .or_else(|| active_containers.first())
                        .map(|container| container.id.clone());
                    if let Some(container_to_open) = container_to_open {
                        this.do_open_pane_composite(&container_to_open, false);
                    }
                }
            } else {
                this.layout_service().set_part_hidden(true, this.part_id);
            }

            this.remove_composite(&viewlet_descriptor.id);
        }));

        let weak = self.weak_self.clone();
        self.base.register(self.services.extension_service.on_did_register_extensions().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.layout_composite_bar();
            }
        }));
    }

    /// `onDidPaneCompositeOpen`/`onDidPaneCompositeClose` e o `onDidOpen`/`onDidClose` do original, na ordem em que os ouvintes
    /// do construtor os registram (o da própria parte primeiro).
    fn fire_pane_composite(&self, composite: &Rc<Composite>, open: bool) {
        use crate::browser::composite::IComposite;

        let composite_id = composite.get_id();
        if open {
            self.on_did_open(&composite_id);
        } else {
            self.on_did_close(&composite_id);
        }

        let pane = self.instances.borrow().get(&composite_id).cloned();
        if let Some(pane) = pane {
            if open {
                self.on_did_pane_composite_open.fire(pane);
            } else {
                self.on_did_pane_composite_close.fire(pane);
            }
        }
    }

    fn on_did_open(&self, composite_id: &str) {
        self.active_pane_context_key.set(Dynamic::from(composite_id));
        if let Some(element) = self.element() {
            element.style_set_property("data-active-composite", composite_id);
        }
    }

    fn on_did_close(&self, composite_id: &str) {
        if self.active_pane_context_key.get().strict_equals(&Dynamic::from(composite_id)) {
            self.active_pane_context_key.reset();
            if let Some(element) = self.element() {
                element.style_remove_property("data-active-composite");
            }
        }
    }

    // region: acessos

    /// A [`CompositePart`] que esta parte estende.
    pub fn composite_part(&self) -> &Rc<CompositePart> {
        &self.composite_part
    }

    /// `partId`.
    pub fn part_id(&self) -> Parts {
        self.part_id
    }

    /// `registryId`.
    pub fn registry_id(&self) -> &str {
        &self.registry_id
    }

    /// `location`.
    pub fn location(&self) -> ViewContainerLocation {
        self.location
    }

    /// `layoutService`.
    pub fn layout_service(&self) -> &Rc<dyn IWorkbenchLayoutService> {
        self.composite_part.part().layout_service()
    }

    /// `configurationService`.
    pub fn configuration_service(&self) -> &Rc<dyn IConfigurationService> {
        &self.services.configuration_service
    }

    /// `menuService`.
    pub fn menu_service(&self) -> &Rc<dyn IMenuService> {
        &self.services.menu_service
    }

    /// `contextKeyService`.
    pub fn context_key_service(&self) -> &Rc<dyn IContextKeyService> {
        &self.services.context_key_service
    }

    /// `getColor(id)` do `Themable`.
    pub fn get_color(&self, id: &str) -> Option<je_theme::color::Color> {
        self.composite_part.part().get_color(id, None)
    }

    /// `getContainer()`.
    pub fn get_container(&self) -> Option<Rc<ViewElement>> {
        self.composite_part.part().get_container()
    }

    /// `element`.
    pub fn element(&self) -> Option<Rc<ViewElement>> {
        self.composite_part.element()
    }

    /// `titleArea`.
    pub fn title_area(&self) -> Option<Rc<ViewElement>> {
        self.composite_part.part().title_area()
    }

    /// `contentDimension`.
    pub fn content_dimension(&self) -> Option<Dimension> {
        self.state.borrow().content_dimension
    }

    /// `snap`.
    pub fn snap(&self) -> bool {
        self.behavior.snap(self)
    }

    /// Uma referência fraca a esta parte, para os ganchos que ela entrega às barras e aos serviços.
    pub fn weak(&self) -> Weak<AbstractPaneCompositePart> {
        self.weak_self.clone()
    }

    /// `shouldShowCompositeBar()`.
    pub fn should_show_composite_bar(&self) -> bool {
        self.behavior.should_show_composite_bar(self)
    }

    /// `getCompositeBarPosition()`.
    pub fn get_composite_bar_position(&self) -> CompositeBarPosition {
        self.behavior.get_composite_bar_position(self)
    }

    /// `getColor(id)` como o CSSOM o devolve depois de uma atribuição a `style.<cor>` (`rgb(...)`), ou `None`.
    pub fn get_color_css(&self, id: &str) -> Option<String> {
        self.get_color(id).map(|color| ColorValue::new(color).css())
    }

    /// `onDidPaneCompositeOpen`.
    pub fn on_did_pane_composite_open(&self) -> Event<Rc<dyn IPaneComposite>> {
        self.on_did_pane_composite_open.event()
    }

    /// `onDidPaneCompositeClose`.
    pub fn on_did_pane_composite_close(&self) -> Event<Rc<dyn IPaneComposite>> {
        self.on_did_pane_composite_close.event()
    }

    /// `getActiveComposite()?.getId()`.
    pub fn get_active_composite_id(&self) -> Option<String> {
        use crate::browser::composite::IComposite;
        self.composite_part.get_active_composite().map(|composite| composite.get_id())
    }

    /// `getActiveComposite()`.
    pub fn get_active_composite(&self) -> Option<Rc<Composite>> {
        self.composite_part.get_active_composite()
    }

    /// `toolBar`.
    pub fn tool_bar(&self) -> Option<Rc<dyn crate::browser::parts::composite_part::ICompositeToolBar>> {
        self.composite_part.tool_bar()
    }

    /// `titleLabel`.
    pub fn title_label(&self) -> Option<Rc<dyn ICompositeTitleLabel>> {
        self.composite_part.title_label()
    }

    /// `onTitleAreaUpdate(compositeId)`.
    pub fn on_title_area_update(&self, composite_id: &str) {
        self.composite_part.on_title_area_update(composite_id);
    }

    /// `getTitleAreaDropDownAnchorAlignment()`.
    pub fn get_title_area_drop_down_anchor_alignment(&self) -> AnchorAlignment {
        self.behavior.get_title_area_drop_down_anchor_alignment(self)
    }

    /// `actionViewItemProvider(action, options)`.
    pub fn action_view_item_provider(&self, action: &ActionRef, options: &je_widgets::action_view_items::IActionViewItemOptions) -> Option<Rc<dyn je_widgets::action_view_items::IActionViewItem>> {
        self.composite_part.action_view_item_provider_base(action, options)
    }

    /// `getProgressIndicator(id)`.
    pub fn get_progress_indicator(&self, id: &str) -> Option<Rc<dyn IProgressIndicator>> {
        self.composite_part.get_progress_indicator(id)
    }

    /// `removeComposite(compositeId)`: o painel guardado sai junto com o composite.
    pub fn remove_composite(&self, composite_id: &str) -> bool {
        let removed = self.composite_part.remove_composite(composite_id);
        if removed {
            self.instances.borrow_mut().remove(composite_id);
        }
        removed
    }

    /// O que a barra lateral passa à `ActivitybarPart` e às barras de composites: a parte vista como [`IPaneCompositePart`].
    pub fn as_pane_composite_part(&self) -> Rc<dyn IPaneCompositePart> {
        Rc::new(WeakPaneCompositePart { part: self.weak_self.clone(), part_id: self.part_id })
    }

    // endregion

    // region: criação

    /// `create(parent)`.
    pub fn create(&self, parent: &Rc<ViewElement>) {
        self.composite_part.set_element(Some(parent.clone()));
        parent.class_list_add("pane-composite-part");

        self.composite_part.part().create(parent, None);

        if let Some(content_area) = self.composite_part.part().content_area() {
            self.create_empty_pane_message(&content_area);
        }

        self.update_composite_bar(false);

        if let Some(factory) = self.services.focus_tracker_factory.clone() {
            let focus_tracker: Rc<dyn IFocusTracker> = factory(parent);
            self.base.register(focus_tracker.clone());
            let key = self.pane_focus_context_key.clone();
            self.base.register(focus_tracker.on_did_focus().listen(move |_| key.set(Dynamic::Bool(true))));
            let key = self.pane_focus_context_key.clone();
            self.base.register(focus_tracker.on_did_blur().listen(move |_| key.set(Dynamic::Bool(false))));
        }
    }

    fn create_empty_pane_message(&self, parent: &Rc<ViewElement>) {
        let view = &self.services.composite.view;
        let empty_pane_message_element = view.create_element(None, "div", "empty-pane-message-area");

        let message_element = view.create_element(None, "div", "empty-pane-message");
        self.pane_view.set_text_content(&message_element, &localize("pane.emptyMessage", "Drag a view here to display.", &[]));

        view.append_child(&empty_pane_message_element, &message_element);
        view.append_child(parent, &empty_pane_message_element);
        self.state.borrow_mut().empty_pane_message_element = Some(empty_pane_message_element.clone());

        if self.services.view_descriptor_service.can_move_views() {
            let weak = self.weak_self.clone();
            let set_drop_background_feedback = {
                let weak = weak.clone();
                Rc::new(move |visible: bool| {
                    let Some(this) = weak.upgrade() else { return };
                    let update_activity_bar_background = this.get_active_composite().is_none() || !visible;
                    let background_color = if visible { this.get_color(EDITOR_DRAG_AND_DROP_BACKGROUND).map(|color| color.to_string()).unwrap_or_default() } else { String::new() };

                    let (title_container, header_footer, empty) = {
                        let state = this.state.borrow();
                        (state.title_container.clone(), state.header_footer_composite_bar_container.clone(), state.empty_pane_message_element.clone())
                    };
                    if let Some(title_container) = title_container
                        && update_activity_bar_background
                    {
                        set_style_or_remove(&title_container, "background-color", Some(background_color.as_str()));
                    }
                    if let Some(header_footer) = header_footer
                        && update_activity_bar_background
                    {
                        set_style_or_remove(&header_footer, "background-color", Some(background_color.as_str()));
                    }
                    if let Some(empty) = empty {
                        set_style_or_remove(&empty, "background-color", Some(background_color.as_str()));
                    }
                })
            };

            let (feedback_enter, feedback_leave, feedback_end, feedback_drop) =
                (set_drop_background_feedback.clone(), set_drop_background_feedback.clone(), set_drop_background_feedback.clone(), set_drop_background_feedback);

            let (weak_over, weak_enter, weak_drop) = (weak.clone(), weak.clone(), weak);
            let callbacks = CompositeDragAndDropObserverCallbacks {
                on_drag_over: Some(Rc::new(move |e: &DraggedCompositeData| {
                    let Some(this) = weak_over.upgrade() else { return };
                    // EventHelper.stop(e.eventData, true)
                    e.event_data.prevent_default();
                    if let Some(bar) = this.pane_composite_bar.value() {
                        // `toggleDropEffect(e.eventData.dataTransfer, 'move', validDropTarget)` não tem estado nativo.
                        let _valid_drop_target = bar.on_drag_enter(&e.drag_and_drop_data, &e.event_data);
                    }
                })),
                on_drag_enter: Some(Rc::new(move |e: &DraggedCompositeData| {
                    let Some(this) = weak_enter.upgrade() else { return };
                    e.event_data.prevent_default();
                    if let Some(bar) = this.pane_composite_bar.value() {
                        let valid_drop_target = bar.on_drag_enter(&e.drag_and_drop_data, &e.event_data);
                        feedback_enter(valid_drop_target);
                    }
                })),
                on_drag_leave: Some(Rc::new(move |e: &DraggedCompositeData| {
                    e.event_data.prevent_default();
                    feedback_leave(false);
                })),
                on_drag_end: Some(Rc::new(move |e: &DraggedCompositeData| {
                    e.event_data.prevent_default();
                    feedback_end(false);
                })),
                on_drop: Some(Rc::new(move |e: &DraggedCompositeData| {
                    let Some(this) = weak_drop.upgrade() else { return };
                    e.event_data.prevent_default();
                    feedback_drop(false);
                    this.on_drop(e);
                })),
                on_drag_start: None,
            };

            if let Some(element) = self.element() {
                self.base.register(CompositeDragAndDropObserver::instance().register_target(&self.services.drag_and_drop_host, &element, callbacks));
            }
        }
    }

    fn on_drop(&self, e: &DraggedCompositeData) {
        if let Some(bar) = self.pane_composite_bar.value() {
            bar.on_drop(&e.drag_and_drop_data, &e.event_data);
            return;
        }

        // Allow opening views/composites if the composite bar is hidden
        let drag_data = e.drag_and_drop_data.get_data();
        let view_descriptor_service = &self.services.view_descriptor_service;

        if drag_data.view_type == ViewType::Composite {
            let current_container = assert_returns_defined(view_descriptor_service.get_view_container_by_id(&drag_data.id));
            view_descriptor_service.move_view_container_to_location(&current_container, self.location, None, Some("dnd"));
            self.open_pane_composite(Some(&current_container.id), true);
        } else if drag_data.view_type == ViewType::View {
            let view_to_move = assert_returns_defined(view_descriptor_service.get_view_descriptor_by_id(&drag_data.id));
            if view_to_move.can_move_view {
                view_descriptor_service.move_view_to_location(&view_to_move, self.location, Some("dnd"));

                let new_container = assert_returns_defined(view_descriptor_service.get_view_container_by_view_id(&view_to_move.id));

                let view_id = view_to_move.id.clone();
                self.open_pane_composite(Some(&new_container.id), true).then(move |composite| {
                    if let Some(composite) = composite {
                        composite.open_view(&view_id, true);
                    }
                });
            }
        }
    }

    /// O que `createTitleArea` faz depois do `super.createTitleArea`: o menu de contexto e a barra de ações globais.
    fn create_title_area_extras(&self, title_area: &Rc<ViewElement>) {
        let weak = self.weak_self.clone();
        self.base.register(self.pane_view.on_context_menu(
            title_area,
            Rc::new(move |anchor: AnchorTarget| {
                if let Some(this) = weak.upgrade() {
                    this.on_title_area_context_menu(anchor);
                }
            }),
        ));

        let view = &self.services.composite.view;
        let global_title_actions_container = view.create_element(Some(title_area), "div", "global-actions");

        // Global Actions Toolbar
        let weak_provider = self.weak_self.clone();
        let action_view_item_provider: je_widgets::action_bar::IActionViewItemProvider = Rc::new(move |action: &ActionRef, options: &je_widgets::action_view_items::IActionViewItemOptions| {
            let this = weak_provider.upgrade()?;
            this.action_view_item_provider(action, options)
        });
        let weak_alignment = self.weak_self.clone();
        let anchor_alignment_provider: Rc<dyn Fn() -> AnchorAlignment> =
            Rc::new(move || weak_alignment.upgrade().map_or(AnchorAlignment::Right, |this| this.get_title_area_drop_down_anchor_alignment()));
        let hover_delegate: Rc<dyn je_widgets::hover_api::IHoverDelegate> = self.composite_part.toolbar_hover_delegate();
        let global_tool_bar = self.pane_view.create_global_tool_bar(GlobalToolBarArgs {
            container: global_title_actions_container,
            menu_id: self.global_actions_menu_id.clone(),
            action_view_item_provider,
            anchor_alignment_provider,
            toggle_menu_title: localize("moreActions", "More Actions...", &[]),
            hover_delegate,
            telemetry_source: self.composite_part.name_for_telemetry().to_owned(),
        });
        self.base.register(global_tool_bar.clone());
        self.state.borrow_mut().global_tool_bar = Some(global_tool_bar);
    }

    /// O que `createTitleLabel` faz depois do `super.createTitleLabel`: o rótulo arrastável.
    fn create_title_label_extras(&self, _title_label: &Rc<dyn ICompositeTitleLabel>) {
        let Some(title_label_element) = self.composite_part.title_label_element() else { return };

        self.services.drag_and_drop_host.set_draggable(&title_label_element, self.services.view_descriptor_service.can_move_views());
        let weak = self.weak_self.clone();
        let dragged_item_provider: Rc<dyn Fn() -> CompositeDragData> = Rc::new(move || {
            let active_viewlet = weak.upgrade().and_then(|this| this.get_active_pane_composite());
            let id = assert_returns_defined(active_viewlet).get_id();
            CompositeDragData { view_type: ViewType::Composite, id }
        });
        self.base.register(CompositeDragAndDropObserver::instance().register_draggable(
            &self.services.drag_and_drop_host,
            &title_label_element,
            dragged_item_provider,
            CompositeDragAndDropObserverCallbacks::default(),
        ));
    }

    // endregion

    // region: barra de composites

    /// `updateCompositeBar(updateCompositeBarOption)`.
    pub fn update_composite_bar(&self, update_composite_bar_option: bool) {
        let previous_position = self.state.borrow().composite_bar_position;
        let was_composite_bar_visible = previous_position.is_some();
        let is_composite_bar_visible = self.behavior.should_show_composite_bar(self);
        let new_position = if is_composite_bar_visible { Some(self.behavior.get_composite_bar_position(self)) } else { None };

        // Only update if the visibility or position has changed or if the composite bar options should be updated
        if !update_composite_bar_option && previous_position == new_position {
            return;
        }

        // Remove old composite bar
        if was_composite_bar_visible {
            let (title_container, header_footer, pane_composite_bar_container) = {
                let state = self.state.borrow();
                (state.title_container.clone(), state.header_footer_composite_bar_container.clone(), state.pane_composite_bar_container.clone())
            };
            let previous_composite_bar_container = if previous_position == Some(CompositeBarPosition::Title) { title_container } else { header_footer };
            let (Some(pane_composite_bar_container), Some(previous_composite_bar_container), Some(_)) =
                (pane_composite_bar_container, previous_composite_bar_container, self.pane_composite_bar.value())
            else {
                errors::throw(Error::new("Composite bar containers should exist when removing the previous composite bar"));
            };

            self.services.composite.view.remove_child(&pane_composite_bar_container);
            self.state.borrow_mut().pane_composite_bar_container = None;
            self.pane_composite_bar.set(None);

            previous_composite_bar_container.class_list_remove("has-composite-bar");

            if previous_position == Some(CompositeBarPosition::Top) {
                self.remove_footer_header_area(true);
            } else if previous_position == Some(CompositeBarPosition::Bottom) {
                self.remove_footer_header_area(false);
            }
        }

        // Create new composite bar
        let new_composite_bar_container = match new_position {
            Some(CompositeBarPosition::Top) => Some(self.create_header_area()),
            Some(CompositeBarPosition::Title) => self.state.borrow().title_container.clone(),
            Some(CompositeBarPosition::Bottom) => Some(self.create_footer_area()),
            None => None,
        };
        if is_composite_bar_visible {
            let has_container = self.state.borrow().pane_composite_bar_container.is_some();
            let Some(new_composite_bar_container) = new_composite_bar_container.filter(|_| !has_container && self.pane_composite_bar.value().is_none()) else {
                errors::throw(Error::new("Invalid composite bar state when creating the new composite bar"));
            };

            new_composite_bar_container.class_list_add("has-composite-bar");
            let view = &self.services.composite.view;
            let pane_composite_bar_container = view.create_element(None, "div", "composite-bar-container");
            view.prepend_child(&new_composite_bar_container, &pane_composite_bar_container);
            self.state.borrow_mut().pane_composite_bar_container = Some(pane_composite_bar_container.clone());

            let bar = self.behavior.create_composite_bar(self);
            self.pane_composite_bar.set(Some(bar.clone()));
            bar.create(&self.pane_view.content_element_for(&pane_composite_bar_container));

            if new_position == Some(CompositeBarPosition::Top) {
                self.composite_part.part().set_header_area(&new_composite_bar_container);
            } else if new_position == Some(CompositeBarPosition::Bottom) {
                self.composite_part.part().set_footer_area(&new_composite_bar_container);
            }
        }

        self.state.borrow_mut().composite_bar_position = new_position;

        if update_composite_bar_option {
            self.layout_composite_bar();
        }
    }

    /// `createHeaderArea()`.
    fn create_header_area(&self) -> Rc<ViewElement> {
        let header_area = self.composite_part.create_header_area();
        self.create_header_footer_composite_bar_area(header_area)
    }

    /// `createFooterArea()`.
    fn create_footer_area(&self) -> Rc<ViewElement> {
        let footer_area = self.composite_part.create_footer_area();
        self.create_header_footer_composite_bar_area(footer_area)
    }

    /// `createHeaderFooterCompositeBarArea(area)`.
    pub fn create_header_footer_composite_bar_area(&self, area: Rc<ViewElement>) -> Rc<ViewElement> {
        if self.state.borrow().header_footer_composite_bar_container.is_some() {
            // A pane composite part has either a header or a footer, but not both
            errors::throw(Error::new("Header or Footer composite bar already exists"));
        }
        self.state.borrow_mut().header_footer_composite_bar_container = Some(area.clone());

        let weak = self.weak_self.clone();
        self.header_footer_composite_bar_disposables.add(self.pane_view.on_context_menu(
            &area,
            Rc::new(move |anchor: AnchorTarget| {
                if let Some(this) = weak.upgrade() {
                    this.on_composite_bar_context_menu(anchor);
                }
            }),
        ));

        area
    }

    fn remove_footer_header_area(&self, header: bool) {
        self.state.borrow_mut().header_footer_composite_bar_container = None;
        self.header_footer_composite_bar_disposables.clear();
        if header {
            self.composite_part.part().remove_header_area();
        } else {
            self.composite_part.part().remove_footer_area();
        }
    }

    /// `createCompositeBar()` do `AbstractPaneCompositePart`: `createInstance(PaneCompositeBar, location, options, partId, this)`.
    pub fn create_composite_bar_base(&self) -> Rc<dyn IPaneCompositeBar> {
        (self.services.composite_bar_factory)(CompositeBarRequest {
            location: self.location,
            options: self.behavior.get_composite_bar_options(self),
            part: self.part_id,
            pane_composite_part: self.as_pane_composite_part(),
            global_activities: None,
        })
    }

    /// `focusCompositeBar()`.
    pub fn focus_composite_bar(&self) {
        if let Some(bar) = self.pane_composite_bar.value() {
            bar.focus(None);
        }
    }

    fn pane_composite_bar_visible_ids(&self) -> Vec<String> {
        self.pane_composite_bar.value().map(|bar| bar.get_visible_pane_composite_ids()).unwrap_or_default()
    }

    /// `getPinnedPaneCompositeIds()` da `AbstractPaneCompositePart`.
    pub fn get_pinned_pane_composite_ids_base(&self) -> Vec<String> {
        self.pane_composite_bar.value().map(|bar| bar.get_pinned_pane_composite_ids()).unwrap_or_default()
    }

    /// `getVisiblePaneCompositeIds()` da `AbstractPaneCompositePart`.
    pub fn get_visible_pane_composite_ids_base(&self) -> Vec<String> {
        self.pane_composite_bar_visible_ids()
    }

    /// `getPaneCompositeIds()` da `AbstractPaneCompositePart`.
    pub fn get_pane_composite_ids_base(&self) -> Vec<String> {
        self.pane_composite_bar.value().map(|bar| bar.get_pane_composite_ids()).unwrap_or_default()
    }

    /// `getPinnedPaneCompositeIds()`.
    pub fn get_pinned_pane_composite_ids(&self) -> Vec<String> {
        self.behavior.get_pinned_pane_composite_ids(self)
    }

    /// `getVisiblePaneCompositeIds()`.
    pub fn get_visible_pane_composite_ids(&self) -> Vec<String> {
        self.behavior.get_visible_pane_composite_ids(self)
    }

    /// `getPaneCompositeIds()`.
    pub fn get_pane_composite_ids(&self) -> Vec<String> {
        self.behavior.get_pane_composite_ids(self)
    }

    /// O agendamento de `onTitleAreaUpdate`: refaz o layout da barra no próximo quadro, uma vez.
    fn schedule_composite_bar_layout(&self) {
        if self.pending_composite_bar_layout.value().is_some() {
            return;
        }

        let weak = self.weak_self.clone();
        let scheduled = self.pane_view.schedule_at_next_animation_frame(Box::new(move || {
            if let Some(this) = weak.upgrade() {
                this.pending_composite_bar_layout.clear();
                this.layout_composite_bar();
            }
        }));
        self.pending_composite_bar_layout.set(Some(scheduled));
    }

    // endregion

    // region: abertura

    /// `openPaneComposite(id, focus)`.
    pub fn open_pane_composite(&self, id: Option<&str>, focus: bool) -> Promise<Option<Rc<dyn IPaneComposite>>> {
        if let Some(id) = id
            && self.get_pane_composite(id).is_some()
        {
            return self.do_open_pane_composite(id, focus);
        }

        let weak = self.weak_self.clone();
        let id = id.map(str::to_owned);
        self.services.extension_service.when_installed_extensions_registered().then_promise(move |_| {
            let this = weak.upgrade();
            match (this, id) {
                (Some(this), Some(id)) if this.get_pane_composite(&id).is_some() => this.do_open_pane_composite(&id, focus),
                _ => Promise::resolved(None),
            }
        })
    }

    fn do_open_pane_composite(&self, id: &str, focus: bool) -> Promise<Option<Rc<dyn IPaneComposite>>> {
        let block_opening = self.state.borrow().block_opening.clone();
        if let Some(block_opening) = block_opening {
            // Workaround against a potential race condition when calling `setPartHidden` we may end up in `openPaneComposite`
            // again. But we still want to return the result of the original call, so we return the promise of the original call.
            return block_opening.promise.clone();
        }

        let mut block_opening: Option<Rc<DeferredPromise<Option<Rc<dyn IPaneComposite>>>>> = None;
        if !self.layout_service().is_visible(self.part_id, None) {
            let deferred = Rc::new(DeferredPromise::new());
            block_opening = Some(deferred.clone());
            self.state.borrow_mut().block_opening = Some(deferred);
            self.layout_service().set_part_hidden(false, self.part_id);
            self.state.borrow_mut().block_opening = None;
        }

        let composite = self.composite_part.open_composite(id, focus);
        let result = composite.and_then(|composite| self.pane_of(&composite));
        if let Some(block_opening) = block_opening {
            block_opening.complete(result.clone());
        }

        Promise::resolved(result)
    }

    fn pane_of(&self, composite: &Rc<Composite>) -> Option<Rc<dyn IPaneComposite>> {
        use crate::browser::composite::IComposite;
        self.instances.borrow().get(&composite.get_id()).cloned()
    }

    /// `getPaneComposite(id)`.
    pub fn get_pane_composite(&self, id: &str) -> Option<PaneCompositeDescriptor> {
        self.registry.get_pane_composite(id)
    }

    /// `getPaneComposites()`: ordenados por `order`, os sem `order` por último.
    pub fn get_pane_composites(&self) -> Vec<PaneCompositeDescriptor> {
        let mut composites = self.registry.get_pane_composites();
        composites.sort_by(|v1, v2| match (v1.order, v2.order) {
            (None, None) => std::cmp::Ordering::Equal,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (Some(_), None) => std::cmp::Ordering::Less,
            (Some(o1), Some(o2)) => o1.partial_cmp(&o2).unwrap_or(std::cmp::Ordering::Equal),
        });
        composites
    }

    /// `getActivePaneComposite()`.
    pub fn get_active_pane_composite(&self) -> Option<Rc<dyn IPaneComposite>> {
        self.get_active_composite().and_then(|composite| self.pane_of(&composite))
    }

    /// `getLastActivePaneCompositeId()`.
    pub fn get_last_active_pane_composite_id(&self) -> String {
        self.composite_part.get_last_active_composite_id()
    }

    /// `hideActivePaneComposite()`.
    pub fn hide_active_pane_composite(&self) {
        if self.layout_service().is_visible(self.part_id, None) {
            self.layout_service().set_part_hidden(true, self.part_id);
        }

        self.composite_part.hide_active_composite();
    }

    // endregion

    // region: layout

    /// `layout(width, height, top, left)`.
    pub fn layout(&self, width: f64, height: f64, top: f64, left: f64) {
        self.composite_part.part().layout(width, height, top, left);
    }

    /// `layout(...)` do `AbstractPaneCompositePart`.
    pub fn layout_base(&self, width: f64, height: f64, top: f64, left: f64) {
        if !self.layout_service().is_visible(self.part_id, None) {
            return;
        }
        let Some(element) = self.element() else { return };

        // Remember the dimension as provided by the grid (before the floating inset is applied) so relayouts triggered by
        // internal changes (title/header/footer) feed back this original dimension instead of a repeatedly shrunk one.
        self.state.borrow_mut().floating_layout_dimension = Some(Dimension { width, height });

        // When the floating panels experiment is enabled, shrink the content to leave room for the card margin and border applied
        // via CSS on the part.
        let floating_inset = self.get_floating_inset();
        if self.layout_service().is_floating_panels_enabled() {
            element.style_set_property("height", &css_calc_percent_minus(floating_inset.vertical_margin));
        } else {
            element.style_remove_property("height");
        }
        let (mut width, mut height) = (width, height);
        if floating_inset.width > 0.0 || floating_inset.height > 0.0 {
            width = (width - floating_inset.width).max(0.0);
            height = (height - floating_inset.height).max(0.0);
        }

        let content_dimension = Dimension { width, height };
        self.state.borrow_mut().content_dimension = Some(content_dimension);

        // Reflect which cluster edges this part is the outermost floating card on so the matching outer gutter and corner radius
        // can be applied in CSS (kept in sync with `getFloatingInset`).
        let outer_gutter = self.get_floating_outer_gutter_edges();
        let vertical_outer_edges = get_floating_pane_composite_vertical_outer_edges(self.layout_service().as_ref(), self.part_id, MAIN_WINDOW);
        toggle_class(&element, "floating-part-outer-left", outer_gutter.left);
        toggle_class(&element, "floating-part-outer-right", outer_gutter.right);
        toggle_class(&element, "floating-part-outer-top", vertical_outer_edges.top);
        toggle_class(&element, "floating-part-outer-bottom", vertical_outer_edges.bottom);

        // Mirror the panel's outer-edge state onto the workbench container so the horizontal grid sash highlight can match the
        // panel card's outer gutter by selecting a direct class, rather than a `:has()` query on the workbench root.
        if self.part_id == Parts::PanelPart {
            let workbench_container = self.layout_service().get_container(MAIN_WINDOW);
            self.pane_view.toggle_container_class(workbench_container, "floating-panel-outer-left", outer_gutter.left);
            self.pane_view.toggle_container_class(workbench_container, "floating-panel-outer-right", outer_gutter.right);
        }

        // Layout contents
        self.composite_part.layout_base(content_dimension.width, content_dimension.height, top, left);

        // Posiciona os nós da parte
        if let Some(result) = self.composite_part.layout_result() {
            let part = self.composite_part.part();
            let (empty_pane_message_area, header_footer, position) = {
                let state = self.state.borrow();
                (state.empty_pane_message_element.clone(), state.header_footer_composite_bar_container.clone(), state.composite_bar_position)
            };
            self.pane_view.layout_part(&PaneCompositeLayout {
                part_element: element,
                title_area: part.title_area(),
                header_area: header_footer.clone().filter(|_| position == Some(CompositeBarPosition::Top)),
                footer_area: header_footer.filter(|_| position == Some(CompositeBarPosition::Bottom)),
                content_area: part.content_area(),
                empty_pane_message_area,
                width: content_dimension.width,
                height: content_dimension.height,
                result,
            });
        }

        // Layout composite bar
        self.layout_composite_bar();

        // Add empty pane message
        self.layout_empty_message();
    }

    /// `setBoundarySashes(sashes)`.
    pub fn set_boundary_sashes(&self, sashes: &BoundarySashes) {
        self.behavior.set_boundary_sashes(self, sashes);
    }

    /// `setBoundarySashes(sashes)` da `CompositePart`.
    pub fn set_boundary_sashes_base(&self, sashes: &BoundarySashes) {
        self.composite_part.set_boundary_sashes(sashes);
    }

    /// `updateStyles()` da `CompositePart`.
    pub fn update_styles_base(&self) {
        self.composite_part.update_styles_base();
    }

    /// `updateStyles()`.
    pub fn update_styles(&self) {
        self.composite_part.part().update_styles();
    }

    /// The horizontal cluster edges on which this part is the outermost floating card and therefore adopts the density-specific
    /// outer gutter. Applies to the primary side bar, the secondary side bar and the panel; a horizontal panel can own both edges
    /// at once.
    fn get_floating_outer_gutter_edges(&self) -> HorizontalPair<bool> {
        get_floating_outer_gutter_edges(self.layout_service().as_ref(), self.part_id)
    }

    /// Amount (in pixels) to subtract from each axis when the floating panels experiment is enabled: a margin on each side plus a
    /// 1px border on each side (the border is drawn inside the box, as `.monaco-workbench .part` is `box-sizing: border-box` in
    /// `part.css`). On each cluster edge this part is the outermost floating card on (see `get_floating_outer_gutter_edges`) it
    /// gets the outer margin, so its width inset is larger on that side.
    fn get_floating_inset(&self) -> FloatingInset {
        if !self.layout_service().is_floating_panels_enabled() {
            return FloatingInset { width: 0.0, height: 0.0, vertical_margin: 0.0 };
        }

        let border_total = self.behavior.get_floating_border_width(self) * 2.0;
        let VerticalPair { top, bottom } = get_floating_pane_composite_vertical_margins(self.layout_service().as_ref(), self.part_id, MAIN_WINDOW);
        let HorizontalPair { left, right } = get_floating_pane_composite_horizontal_margins(self.layout_service().as_ref(), self.part_id);
        FloatingInset { width: left + right + border_total, height: top + bottom + border_total, vertical_margin: top + bottom }
    }

    /// `layoutCompositeBar()`.
    pub fn layout_composite_bar(&self) {
        self.pending_composite_bar_layout.clear();

        let content_dimension = self.state.borrow().content_dimension;
        let dimension = self.composite_part.part().dimension();
        let bar = self.pane_composite_bar.value();
        if let (Some(content_dimension), Some(dimension), Some(bar)) = (content_dimension, dimension, bar) {
            let padding = if self.state.borrow().composite_bar_position == Some(CompositeBarPosition::Title) { 16.0 } else { 8.0 };
            let border_width = if self.part_id == Parts::PanelPart { 0.0 } else { 1.0 };
            let mut available_width = content_dimension.width - padding - border_width;
            available_width = MIN_COMPOSITE_BAR_WIDTH.max(available_width - self.get_toolbar_width());
            bar.layout(available_width, dimension.height);
        }
    }

    /// `layoutEmptyMessage()`.
    pub fn layout_empty_message(&self) {
        let visible = self.get_active_composite().is_none();
        if let Some(element) = self.element() {
            toggle_class(&element, "empty", visible);
        }
        if visible && let Some(title_label) = self.title_label() {
            title_label.update_title("", "", None);
        }
    }

    /// `getToolbarWidth()`.
    pub fn get_toolbar_width(&self) -> f64 {
        let (position, global_tool_bar) = {
            let state = self.state.borrow();
            (state.composite_bar_position, state.global_tool_bar.clone())
        };
        let Some(tool_bar) = self.tool_bar() else { return 0.0 };
        if position != Some(CompositeBarPosition::Title) {
            return 0.0;
        }

        if self.get_active_pane_composite().is_none() {
            return 0.0;
        }

        // Each toolbar item has 4px margin
        let tool_bar_width = tool_bar.get_items_width() + tool_bar.get_items_length() as f64 * 4.0;
        let global_tool_bar_width = global_tool_bar.map_or(0.0, |global| global.get_items_width() + global.get_items_length() as f64 * 4.0);
        tool_bar_width + global_tool_bar_width + 8.0 // 8px padding left
    }

    // endregion

    // region: menus de contexto

    fn on_title_area_context_menu(&self, anchor: AnchorTarget) {
        if self.behavior.should_show_composite_bar(self) && self.behavior.get_composite_bar_position(self) == CompositeBarPosition::Title {
            self.on_composite_bar_context_menu(anchor);
        } else {
            let active_composite = self.get_active_composite();
            let active_pane_composite_actions = active_composite.as_ref().map(|composite| composite.get_context_menu_actions()).unwrap_or_default();
            if !active_pane_composite_actions.is_empty() {
                self.services.context_menu_service.show_context_menu(ContextMenuRequest::Delegate(Rc::new(TitleContextMenuDelegate {
                    anchor,
                    actions: active_pane_composite_actions,
                    action_runner: active_composite.map(|composite| composite.get_action_runner()),
                })));
            }
        }
    }

    fn on_composite_bar_context_menu(&self, anchor: AnchorTarget) {
        if let Some(bar) = self.pane_composite_bar.value() {
            let actions: Vec<ActionRef> = bar.get_context_menu_actions();
            if !actions.is_empty() {
                self.services.context_menu_service.show_context_menu(ContextMenuRequest::Delegate(Rc::new(TitleContextMenuDelegate { anchor, actions, action_runner: None })));
            }
        }
    }

    /// `getViewsSubmenuAction()`.
    pub fn get_views_submenu_action(&self) -> Option<ActionRef> {
        let view_container_id = self.get_active_pane_composite()?.get_view_container_id()?;
        let element = self.element()?;

        let scoped_context_key_service = self.services.context_key_service.create_scoped(ViewElementTarget::new(element));
        scoped_context_key_service.create_key("viewContainer", Dynamic::from(view_container_id.as_str()));
        let options = MenuActionOptions { arg: None, args: None, should_forward_args: true, render_short_title: true };
        let menu = self.services.menu_service.get_menu_actions(&views_sub_menu(), &*scoped_context_key_service, Some(&options));
        let views_actions = get_action_bar_actions(&menu, &|_| true, false).primary;
        scoped_context_key_service.dispose();

        if views_actions.len() > 1 && views_actions.iter().any(|action| action.enabled()) {
            let submenu: ActionRef = SubmenuAction::new("views", &localize("views", "Views", &[]), views_actions, None);
            Some(submenu)
        } else {
            None
        }
    }

    // endregion
}

/// O que o `getFloatingInset()` devolve.
struct FloatingInset {
    width: f64,
    height: f64,
    vertical_margin: f64,
}

/// `element.classList.toggle(name, force)`.
fn toggle_class(element: &ViewElement, class_name: &str, force: bool) {
    if force {
        element.class_list_add(class_name);
    } else {
        element.class_list_remove(class_name);
    }
}

/// O texto que o CSSOM devolve para `style.height = calc(100% - Npx)`: um `calc` com termo negativo ou zero é serializado como
/// soma (`calc(100% + 0px)`), e com termo positivo como subtração.
fn css_calc_percent_minus(pixels: f64) -> String {
    if pixels > 0.0 { format!("calc(100% - {pixels}px)") } else { format!("calc(100% + {}px)", -pixels + 0.0) }
}

/// `hasTitle` nunca é imposto pela base: as subclasses informam.
fn services_has_title(options: &PartOptions) -> bool {
    options.has_title
}

impl IPaneCompositePart for AbstractPaneCompositePart {
    fn part_id(&self) -> Parts {
        self.part_id
    }

    fn get_pinned_pane_composite_ids(&self) -> Vec<String> {
        AbstractPaneCompositePart::get_pinned_pane_composite_ids(self)
    }

    fn get_visible_pane_composite_ids(&self) -> Vec<String> {
        AbstractPaneCompositePart::get_visible_pane_composite_ids(self)
    }

    fn get_pane_composite_ids(&self) -> Vec<String> {
        AbstractPaneCompositePart::get_pane_composite_ids(self)
    }
}

impl IDisposable for AbstractPaneCompositePart {
    fn dispose(&self) {
        self.base.dispose();
        self.composite_part.dispose();
    }
}
