//! Porte de `src/vs/workbench/browser/parts/compositePart.ts` (e de `media/compositepart.css`): a [`CompositePart`], a parte que
//! mostra um composite por vez, com a barra de título (rótulo e barra de ferramentas), a troca de composite, o estado do
//! último composite ativo, as ações do composite e o progresso de cada composite.
//!
//! Divergências:
//! - **Herança vira composição.** A classe abstrata que `SidebarPart`, `PanelPart` e `AuxiliaryBarPart` estendem é o
//!   [`CompositePart`] concreto, que contém a [`Part`] e se apresenta a ela como um [`PartBehavior`] (um repasse fraco, para a
//!   `Part` poder ser criada antes do `CompositePart`). O que as subclasses sobrescrevem é o trait [`CompositePartBehavior`]: os
//!   limites e o `toJSON` (que o original declara abstratos), o `saveState`, o `getRelayoutDimension`, o `createTitleArea`, o
//!   `createContentArea`, o `createTitleLabel`, o `getToolbarResponsiveBehavior`, o `actionViewItemProvider`, o
//!   `actionsContextProvider`, o `getTitleAreaDropDownAnchorAlignment`, o `updateStyles` e o `layout`. A chamada a `super` de uma
//!   sobrescrita é o método `*_base` do [`CompositePart`] (`create_title_area_base`, `create_content_area_base`,
//!   `create_title_label_base`, `update_styles_base`, `action_view_item_provider_base`, `actions_context_provider_base` e
//!   `layout_base`). O que o original deixa `protected` (`openComposite`, `createComposite`, `showComposite`,
//!   `onTitleAreaUpdate`, `getActiveComposite`, `getLastActiveCompositeId`, `hideActiveComposite`, `removeComposite`, os dois
//!   emissores de abertura e fechamento, a barra de ferramentas e o elemento do rótulo) é público, porque quem estende a parte
//!   é um [`CompositePartBehavior`] que recebe a `&CompositePart`.
//! - **`element`.** O `ISerializableView.element` é atribuído pelas subclasses (`this.element = parent` no `create`) e lido pelo
//!   `openComposite`; é o [`CompositePart::set_element`].
//! - **DOM vira estado nativo.** Os elementos são [`ViewElement`] (pai, classes, estilo). Tudo que o original cria ou desenha com o
//!   DOM e com widgets (`$('.composite')`, `append`, `remove`, o `h2` do título com o hover gerenciado, a `ProgressBar`, a
//!   `WorkbenchToolBar` e o `createActionViewItem`) passa pela [`ICompositePartView`], que quem monta a parte entrega: a
//!   implementação nativa cria os nós da árvore de widgets (o estilo do título, que vem do CSS de cada parte, é dela). O
//!   `compositeContainer.id = composite.getId()` não tem estado nativo e não é guardado.
//! - **`WorkbenchToolBar`.** O que a parte usa da barra é o trait [`ICompositeToolBar`], implementado pela [`WorkbenchToolBar`]; a
//!   barra, o `ProgressBar` ([`ICompositeProgressBar`]) e o hover do título são criados pela [`ICompositePartView`] com as opções
//!   que o original passa ([`CompositeToolBarArgs`]). O `IContextMenuService` e o `IHoverService` do construtor são dela.
//! - **`instantiationService.createChild(new ServiceCollection([IEditorProgressService, ...]))`** é o `create_child` com o
//!   `I_EDITOR_PROGRESS_SERVICE`; o [`ScopedProgressIndicator`] é o `IEditorProgressService` (um marcador do
//!   [`IProgressIndicator`]).
//! - **`CompositePart<T>`.** O parâmetro de tipo do composite desaparece: o registro é o [`ICompositeRegistry`] (nome e fábrica
//!   por identificador) e os composites são [`Composite`].
//! - **`onDidCompositeOpen`/`onDidCompositeClose`** carregam o `Rc<Composite>`, que implementa o `IComposite` do original.
//! - **`focus?: boolean`** ausente é `false`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IActionRunner};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error, is_cancellation_error};
use je_base::event::Emitter;
use je_base::id_generator::default_generator_next_id;
use je_base::layout::AnchorAlignment;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, MutableDisposable};
use je_base::nls::localize;
use je_editor::code_action::platform::{I_EDITOR_PROGRESS_SERVICE, IEditorProgressService, IProgressIndicator};
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::theme_service::IThemeService;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::actions::MenuId;
use je_platform::contextview::view_element::ViewElement;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::notification::{INotificationService, NotificationMessage};
use je_platform::storage::{StorageScope, StorageService, StorageTarget, StorageValue};
use je_theme::color::Color;
use je_widgets::action_bar::{IActionViewItemProvider, prepare_actions};
use je_widgets::action_view_items::{IActionViewItem, IActionViewItemOptions};
use je_widgets::hover_api::{IHoverDelegate, IScopedHoverDelegate, create_instant_hover_delegate};
use je_widgets::sash::BoundarySashes;
use je_widgets::toolbar::{IToolBarResponsiveBehaviorOptions, ToolBar};

use crate::browser::composite::{Composite, IComposite, ICompositeRegistry};
use crate::browser::part::{Part, PartBehavior, PartOptions, assert_returns_defined};
use crate::platform::actions::toolbar::WorkbenchToolBar;
use crate::services::layout::layout_service::IWorkbenchLayoutService;
use crate::services::progress::progress_indicator::{AbstractProgressScope, IProgressBar, IProgressScope, ScopedProgressIndicator};

/// `ICompositeTitleLabel`.
pub trait ICompositeTitleLabel {
    /// Asks to update the title for the composite with the given ID.
    fn update_title(&self, id: &str, title: &str, keybinding: Option<&str>);

    /// Called when theming information changes.
    fn update_styles(&self);
}

/// O `h2` do rótulo do título com o hover gerenciado dele (o que `createTitleLabel` faz com o DOM).
pub trait ITitleLabelView: IDisposable {
    /// `titleLabel.textContent = title`.
    fn set_text(&self, text: &str);

    /// `hover.update(content)`.
    fn update_hover(&self, content: &str);

    /// `titleLabel.style.color = ...`: `None` é o `''` (a cor herdada).
    fn set_color(&self, color: Option<&Color>);
}

/// O que a [`CompositePart`] usa da `ProgressBar`: os membros dos indicadores mais o descarte.
pub trait ICompositeProgressBar: IProgressBar + IDisposable {}

impl<T: IProgressBar + IDisposable> ICompositeProgressBar for T {}

/// O que a [`CompositePart`] usa da `WorkbenchToolBar`.
pub trait ICompositeToolBar: IDisposable {
    /// `setActions(primaryActions, secondaryActions, menuIds)`.
    fn set_actions(&self, primary_actions: &[ActionRef], secondary_actions: &[ActionRef], menu_ids: Option<&[MenuId]>);

    /// `context = value`.
    fn set_context(&self, context: Option<Dynamic>);

    /// `actionRunner = value`.
    fn set_action_runner(&self, action_runner: Rc<dyn IActionRunner>);

    /// `actionRunner`.
    fn action_runner(&self) -> Rc<dyn IActionRunner>;

    /// `setAriaLabel(label)`.
    fn set_aria_label(&self, label: &str);
}

impl ICompositeToolBar for WorkbenchToolBar {
    fn set_actions(&self, primary_actions: &[ActionRef], secondary_actions: &[ActionRef], menu_ids: Option<&[MenuId]>) {
        WorkbenchToolBar::set_actions(self, primary_actions, secondary_actions, menu_ids);
    }

    fn set_context(&self, context: Option<Dynamic>) {
        ToolBar::set_context(self, context);
    }

    fn set_action_runner(&self, action_runner: Rc<dyn IActionRunner>) {
        ToolBar::set_action_runner(self, action_runner);
    }

    fn action_runner(&self) -> Rc<dyn IActionRunner> {
        ToolBar::action_runner(self)
    }

    fn set_aria_label(&self, label: &str) {
        ToolBar::set_aria_label(self, label);
    }
}

/// Os argumentos do `createInstance(WorkbenchToolBar, titleActionsContainer, { ... })` do `createTitleArea`. A orientação é a
/// horizontal e o `getKeyBinding` é a busca de atalho do serviço de atalhos (o padrão da `WorkbenchToolBar`).
pub struct CompositeToolBarArgs {
    /// O `.title-actions`.
    pub container: Rc<ViewElement>,
    /// `actionViewItemProvider`.
    pub action_view_item_provider: IActionViewItemProvider,
    /// `anchorAlignmentProvider`.
    pub anchor_alignment_provider: Rc<dyn Fn() -> AnchorAlignment>,
    /// `toggleMenuTitle`.
    pub toggle_menu_title: String,
    /// `telemetrySource`.
    pub telemetry_source: String,
    /// `hoverDelegate`.
    pub hover_delegate: Rc<dyn IHoverDelegate>,
    /// `trailingSeparator`.
    pub trailing_separator: bool,
    /// `responsiveBehavior`.
    pub responsive_behavior: Option<IToolBarResponsiveBehaviorOptions>,
}

/// Tudo que a [`CompositePart`] desenha com o DOM e com widgets (ver as divergências do módulo).
pub trait ICompositePartView {
    /// `$('tag.classes')` (e, com `parent`, o `append(parent, ...)`).
    fn create_element(&self, parent: Option<&Rc<ViewElement>>, tag: &str, class_name: &str) -> Rc<ViewElement>;

    /// `prepend(parent, child)`.
    fn prepend_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>);

    /// `parent.appendChild(child)`.
    fn append_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>);

    /// `child.remove()`.
    fn remove_child(&self, child: &Rc<ViewElement>);

    /// O texto e o hover gerenciado (`setupManagedHover(getDefaultHoverDelegate('mouse'), titleLabel, '')`) do `h2`.
    fn create_title_label_view(&self, title_label: &Rc<ViewElement>) -> Rc<dyn ITitleLabelView>;

    /// `new ProgressBar(contentContainer, defaultProgressBarStyles)`.
    fn create_progress_bar(&self, container: &Rc<ViewElement>) -> Rc<dyn ICompositeProgressBar>;

    /// `createInstance(WorkbenchToolBar, titleActionsContainer, options)`.
    fn create_tool_bar(&self, args: CompositeToolBarArgs) -> Rc<dyn ICompositeToolBar>;

    /// `createActionViewItem(instantiationService, action, options)`.
    fn create_action_view_item(&self, action: &ActionRef, options: &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>>;
}

/// `ICompositePartOptions`.
#[derive(Clone, Default)]
pub struct ICompositePartOptions {
    /// `IPartOptions`.
    pub part: PartOptions,
    pub trailing_separator: bool,
}

/// O evento `{ composite: IComposite; focus: boolean }` de `onDidCompositeOpen`.
#[derive(Clone)]
pub struct CompositeOpenEvent {
    pub composite: Rc<Composite>,
    pub focus: bool,
}

/// Os serviços que o construtor da `CompositePart` injeta (ver as divergências do módulo para os que a vista cobre).
#[derive(Clone)]
pub struct CompositePartServices {
    pub notification_service: Rc<dyn INotificationService>,
    pub storage_service: Rc<dyn StorageService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub instantiation_service: Rc<InstantiationService>,
    pub theme_service: Rc<dyn IThemeService>,
    pub layout_service: Rc<dyn IWorkbenchLayoutService>,
    pub view: Rc<dyn ICompositePartView>,
}

/// O que as subclasses de `CompositePart` sobrescrevem. Os padrões são os do original.
pub trait CompositePartBehavior {
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

    /// `saveState()`.
    fn save_state(&self) {}

    /// `getRelayoutDimension()`: o padrão é a dimensão guardada pelo último `layout`.
    fn relayout_dimension(&self, part: &Part) -> Option<Dimension> {
        part.dimension()
    }

    /// `createTitleArea(parent)`.
    fn create_title_area(&self, part: &CompositePart, parent: &Rc<ViewElement>) -> Option<Rc<ViewElement>> {
        part.create_title_area_base(parent)
    }

    /// `createContentArea(parent)`.
    fn create_content_area(&self, part: &CompositePart, parent: &Rc<ViewElement>) -> Rc<ViewElement> {
        part.create_content_area_base(parent)
    }

    /// `createTitleLabel(parent)`.
    fn create_title_label(&self, part: &CompositePart, parent: &Rc<ViewElement>) -> Rc<dyn ICompositeTitleLabel> {
        part.create_title_label_base(parent)
    }

    /// `getToolbarResponsiveBehavior()`.
    fn get_toolbar_responsive_behavior(&self) -> Option<IToolBarResponsiveBehaviorOptions> {
        None
    }

    /// `actionViewItemProvider(action, options)`.
    fn action_view_item_provider(&self, part: &CompositePart, action: &ActionRef, options: &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>> {
        part.action_view_item_provider_base(action, options)
    }

    /// `actionsContextProvider()`.
    fn actions_context_provider(&self, part: &CompositePart) -> Option<Dynamic> {
        part.actions_context_provider_base()
    }

    /// `getTitleAreaDropDownAnchorAlignment()`.
    fn get_title_area_drop_down_anchor_alignment(&self) -> AnchorAlignment {
        AnchorAlignment::Right
    }

    /// `updateStyles()`.
    fn update_styles(&self, part: &CompositePart) {
        part.update_styles_base();
    }

    /// `layout(width, height, top, left)`.
    fn layout(&self, part: &CompositePart, width: f64, height: f64, top: f64, left: f64) {
        part.layout_base(width, height, top, left);
    }
}

/// `CompositeItem`.
struct CompositeItem {
    composite: Rc<Composite>,
    disposable: Rc<DisposableStore>,
    progress: Rc<ScopedProgressIndicator>,
}

#[derive(Default)]
struct CompositePartState {
    element: Option<Rc<ViewElement>>,
    active_composite: Option<Rc<Composite>>,
    last_active_composite_id: String,
    title_label: Option<Rc<dyn ICompositeTitleLabel>>,
    title_label_element: Option<Rc<ViewElement>>,
    tool_bar: Option<Rc<dyn ICompositeToolBar>>,
    progress_bar: Option<Rc<dyn ICompositeProgressBar>>,
    content_area_size: Option<Dimension>,
    current_composite_open_token: Option<String>,
    boundary_sashes: Option<BoundarySashes>,
    composite_containers: HashMap<String, Rc<ViewElement>>,
    actions_bindings: HashMap<String, Rc<dyn Fn()>>,
    instantiated: HashMap<String, CompositeItem>,
}

/// O [`PartBehavior`] que a [`Part`] recebe: repassa tudo ao [`CompositePart`] que a contém.
struct PartBehaviorProxy {
    part: Weak<CompositePart>,
}

impl PartBehavior for PartBehaviorProxy {
    fn create_title_area(&self, parent: &Rc<ViewElement>, _options: Option<&Dynamic>) -> Option<Rc<ViewElement>> {
        let this = self.part.upgrade()?;
        this.behavior.create_title_area(&this, parent)
    }

    fn create_content_area(&self, parent: &Rc<ViewElement>, _options: Option<&Dynamic>) -> Option<Rc<ViewElement>> {
        let this = self.part.upgrade()?;
        Some(this.behavior.create_content_area(&this, parent))
    }

    fn update_styles(&self, _part: &Part) {
        if let Some(this) = self.part.upgrade() {
            this.behavior.update_styles(&this);
        }
    }

    fn layout(&self, part: &Part, width: f64, height: f64, top: f64, left: f64) {
        match self.part.upgrade() {
            Some(this) => this.behavior.layout(&this, width, height, top, left),
            None => part.layout_base(width, height, top, left),
        }
    }

    fn relayout_dimension(&self, part: &Part) -> Option<Dimension> {
        match self.part.upgrade() {
            Some(this) => this.behavior.relayout_dimension(part),
            None => part.dimension(),
        }
    }

    fn save_state(&self) {
        if let Some(this) = self.part.upgrade() {
            this.behavior.save_state();
        }
    }

    fn minimum_width(&self) -> f64 {
        self.part.upgrade().map_or(0.0, |this| this.behavior.minimum_width())
    }

    fn maximum_width(&self) -> f64 {
        self.part.upgrade().map_or(f64::INFINITY, |this| this.behavior.maximum_width())
    }

    fn minimum_height(&self) -> f64 {
        self.part.upgrade().map_or(0.0, |this| this.behavior.minimum_height())
    }

    fn maximum_height(&self) -> f64 {
        self.part.upgrade().map_or(f64::INFINITY, |this| this.behavior.maximum_height())
    }

    fn to_json(&self) -> Dynamic {
        self.part.upgrade().map_or(Dynamic::Undefined, |this| this.behavior.to_json())
    }

    fn prepend_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>) {
        match self.part.upgrade() {
            Some(this) => this.view.prepend_child(parent, child),
            None => child.set_parent(Some(parent.clone())),
        }
    }

    fn append_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>) {
        match self.part.upgrade() {
            Some(this) => this.view.append_child(parent, child),
            None => child.set_parent(Some(parent.clone())),
        }
    }

    fn remove_child(&self, child: &Rc<ViewElement>) {
        match self.part.upgrade() {
            Some(this) => this.view.remove_child(child),
            None => child.set_parent(None),
        }
    }
}

/// O `ICompositeTitleLabel` que `createTitleLabel` cria.
struct DefaultTitleLabel {
    part: Weak<CompositePart>,
    label: Rc<dyn ITitleLabelView>,
    parent: Rc<ViewElement>,
}

impl ICompositeTitleLabel for DefaultTitleLabel {
    fn update_title(&self, id: &str, title: &str, keybinding: Option<&str>) {
        let Some(part) = self.part.upgrade() else { return };

        // The title label is shared for all composites in the base CompositePart
        let active = part.get_active_composite();
        if active.is_none_or(|active| active.get_id() == id) {
            self.label.set_text(title);
            self.label.update_hover(&match keybinding {
                Some(keybinding) => localize("titleTooltip", "{0} ({1})", &[title, keybinding]),
                None => title.to_owned(),
            });
        }
    }

    fn update_styles(&self) {
        let Some(part) = self.part.upgrade() else { return };

        let color = part.title_foreground_color.as_deref().and_then(|id| part.part.get_color(id, None));
        self.label.set_color(color.as_ref());

        let border_color = part.title_border_color.as_deref().and_then(|id| part.part.get_color(id, None));
        match border_color {
            Some(border_color) => self.parent.style_set_property("border-bottom", &format!("1px solid {border_color}")),
            None => self.parent.style_remove_property("border-bottom"),
        }
    }
}

impl IEditorProgressService for ScopedProgressIndicator {}

/// `CompositePart`.
pub struct CompositePart {
    weak_self: Weak<CompositePart>,
    part: Rc<Part>,
    base: Rc<Disposable>,
    notification_service: Rc<dyn INotificationService>,
    storage_service: Rc<dyn StorageService>,
    keybinding_service: Rc<dyn IKeybindingService>,
    instantiation_service: Rc<InstantiationService>,
    view: Rc<dyn ICompositePartView>,
    registry: Rc<dyn ICompositeRegistry>,
    active_composite_settings_key: String,
    default_composite_id: String,
    name_for_telemetry: String,
    composite_css_class: String,
    title_foreground_color: Option<String>,
    title_border_color: Option<String>,
    options: ICompositePartOptions,
    behavior: Rc<dyn CompositePartBehavior>,
    on_did_composite_open: Rc<Emitter<CompositeOpenEvent>>,
    on_did_composite_close: Rc<Emitter<Rc<Composite>>>,
    toolbar_hover_delegate: Rc<dyn IScopedHoverDelegate>,
    actions_listener: Rc<MutableDisposable<dyn IDisposable>>,
    state: RefCell<CompositePartState>,
}

impl CompositePart {
    /// `new CompositePart(notificationService, storageService, contextMenuService, layoutService, keybindingService,
    /// hoverService, instantiationService, themeService, registry, activeCompositeSettingsKey, defaultCompositeId,
    /// nameForTelemetry, compositeCSSClass, titleForegroundColor, titleBorderColor, id, options)`, com o que a subclasse
    /// sobrescreve em `behavior`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        services: CompositePartServices,
        registry: Rc<dyn ICompositeRegistry>,
        active_composite_settings_key: &str,
        default_composite_id: &str,
        name_for_telemetry: &str,
        composite_css_class: &str,
        title_foreground_color: Option<&str>,
        title_border_color: Option<&str>,
        id: &str,
        options: ICompositePartOptions,
        behavior: Rc<dyn CompositePartBehavior>,
    ) -> Rc<Self> {
        let CompositePartServices { notification_service, storage_service, keybinding_service, instantiation_service, theme_service, layout_service, view } = services;

        let last_active_composite_id = storage_service.get(active_composite_settings_key, StorageScope::Workspace, Some(default_composite_id)).unwrap_or_else(|| default_composite_id.to_owned());

        let base = Disposable::new();
        let on_did_composite_open: Rc<Emitter<CompositeOpenEvent>> = Emitter::new();
        base.register(on_did_composite_open.clone());
        let on_did_composite_close: Rc<Emitter<Rc<Composite>>> = Emitter::new();
        base.register(on_did_composite_close.clone());
        let actions_listener = MutableDisposable::<dyn IDisposable>::new();
        base.register(actions_listener.clone());
        let toolbar_hover_delegate = create_instant_hover_delegate();
        base.register(toolbar_hover_delegate.clone());

        Rc::new_cyclic(|weak| {
            let proxy: Rc<dyn PartBehavior> = Rc::new(PartBehaviorProxy { part: weak.clone() });
            let part = Part::new(id, options.part.clone(), theme_service, storage_service.clone(), layout_service, proxy);

            CompositePart {
                weak_self: weak.clone(),
                part,
                base,
                notification_service,
                storage_service,
                keybinding_service,
                instantiation_service,
                view,
                registry,
                active_composite_settings_key: active_composite_settings_key.to_owned(),
                default_composite_id: default_composite_id.to_owned(),
                name_for_telemetry: name_for_telemetry.to_owned(),
                composite_css_class: composite_css_class.to_owned(),
                title_foreground_color: title_foreground_color.map(str::to_owned),
                title_border_color: title_border_color.map(str::to_owned),
                options,
                behavior,
                on_did_composite_open,
                on_did_composite_close,
                toolbar_hover_delegate,
                actions_listener,
                state: RefCell::new(CompositePartState { last_active_composite_id, ..CompositePartState::default() }),
            }
        })
    }

    /// A [`Part`] que esta parte estende.
    pub fn part(&self) -> &Rc<Part> {
        &self.part
    }

    /// `nameForTelemetry`.
    pub fn name_for_telemetry(&self) -> &str {
        &self.name_for_telemetry
    }

    /// `options`.
    pub fn options(&self) -> &ICompositePartOptions {
        &self.options
    }

    /// `onDidCompositeOpen`: o emissor (um `Emitter` protegido no original, de onde o `hasListeners` e o `event` saem).
    pub fn on_did_composite_open_emitter(&self) -> &Rc<Emitter<CompositeOpenEvent>> {
        &self.on_did_composite_open
    }

    /// `onDidCompositeClose`: o emissor.
    pub fn on_did_composite_close_emitter(&self) -> &Rc<Emitter<Rc<Composite>>> {
        &self.on_did_composite_close
    }

    /// `element = parent` (o `ISerializableView.element` que as subclasses atribuem no `create`).
    pub fn set_element(&self, element: Option<Rc<ViewElement>>) {
        self.state.borrow_mut().element = element;
    }

    /// `element`.
    pub fn element(&self) -> Option<Rc<ViewElement>> {
        self.state.borrow().element.clone()
    }

    /// `toolBar`.
    pub fn tool_bar(&self) -> Option<Rc<dyn ICompositeToolBar>> {
        self.state.borrow().tool_bar.clone()
    }

    /// `titleLabelElement`.
    pub fn title_label_element(&self) -> Option<Rc<ViewElement>> {
        self.state.borrow().title_label_element.clone()
    }

    /// `titleLabel`.
    pub fn title_label(&self) -> Option<Rc<dyn ICompositeTitleLabel>> {
        self.state.borrow().title_label.clone()
    }

    fn progress_bar(&self) -> Option<Rc<dyn ICompositeProgressBar>> {
        self.state.borrow().progress_bar.clone()
    }

    /// O composite com o identificador está ativo.
    fn is_active(&self, id: &str) -> bool {
        self.get_active_composite().is_some_and(|active| active.get_id() == id)
    }

    pub fn open_composite(&self, id: &str, focus: bool) -> Option<Rc<Composite>> {
        // Check if composite already visible and just focus in that case
        if let Some(active) = self.get_active_composite()
            && active.get_id() == id
        {
            if focus {
                active.focus();
            }

            // Fullfill promise with composite that is being opened
            return Some(active);
        }

        // We cannot open the composite if we have not been created yet
        self.element()?;

        // Open
        self.do_open_composite(id, focus)
    }

    fn do_open_composite(&self, id: &str, focus: bool) -> Option<Rc<Composite>> {
        // Use a generated token to avoid race conditions from long running promises
        let current_composite_open_token = default_generator_next_id();
        self.state.borrow_mut().current_composite_open_token = Some(current_composite_open_token.clone());

        // Hide current
        if self.get_active_composite().is_some() {
            self.hide_active_composite();
        }

        // Update Title
        self.update_title(id, None);

        // Create composite
        let composite = self.create_composite(id, true);

        // Check if another composite opened meanwhile and return in that case
        let active = self.get_active_composite();
        let token_changed = self.state.borrow().current_composite_open_token.as_ref() != Some(&current_composite_open_token);
        if token_changed || active.as_ref().is_some_and(|active| active.get_id() != composite.get_id()) {
            return None;
        }

        // Check if composite already visible and just focus in that case
        if active.as_ref().is_some_and(|active| active.get_id() == composite.get_id()) {
            if focus {
                composite.focus();
            }

            self.on_did_composite_open.fire(CompositeOpenEvent { composite: composite.clone(), focus });
            return Some(composite);
        }

        // Show Composite and Focus
        self.show_composite(&composite);
        if focus {
            composite.focus();
        }

        // Return with the composite that is being opened
        self.on_did_composite_open.fire(CompositeOpenEvent { composite: composite.clone(), focus });

        Some(composite)
    }

    pub fn create_composite(&self, id: &str, is_active: bool) -> Rc<Composite> {
        // Check if composite is already created
        if let Some(composite_item) = self.state.borrow().instantiated.get(id) {
            return composite_item.composite.clone();
        }

        // Instantiate composite from registry otherwise
        let Some(composite_descriptor) = self.registry.get_composite_entry(id) else {
            errors::throw(Error::new(format!("Unable to find composite with id {id}")));
        };

        let disposable = DisposableStore::new();
        let progress_bar = assert_returns_defined(self.progress_bar());
        let progress_bar: Rc<dyn IProgressBar> = progress_bar;

        let scope = AbstractProgressScope::new(&composite_descriptor.id, is_active);
        disposable.add(scope.clone());
        let scope_to_open = scope.clone();
        disposable.add(self.on_did_composite_open.event().listen(move |event| scope_to_open.on_scope_opened(&event.composite.get_id())));
        let scope_to_close = scope.clone();
        disposable.add(self.on_did_composite_close.event().listen(move |composite| scope_to_close.on_scope_closed(&composite.get_id())));

        let composite_progress_indicator = ScopedProgressIndicator::new(progress_bar, scope);
        disposable.add(composite_progress_indicator.clone());

        // provide the editor progress service for any editors instantiated within the composite
        let editor_progress_service: Rc<dyn IEditorProgressService> = composite_progress_indicator.clone();
        let composite_instantiation_service = self.instantiation_service.create_child(ServiceCollection::new().with(I_EDITOR_PROGRESS_SERVICE, editor_progress_service), Some(&*disposable));

        let composite = composite_descriptor.instantiate(&composite_instantiation_service);

        // Remember as Instantiated
        self.state.borrow_mut().instantiated.insert(id.to_owned(), CompositeItem { composite: composite.clone(), disposable: disposable.clone(), progress: composite_progress_indicator });

        // Register to title area update events from the composite
        let weak = self.weak_self.clone();
        let composite_id = composite.get_id();
        disposable.add(composite.on_title_area_update().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_title_area_update(&composite_id);
            }
        }));

        composite
    }

    pub fn show_composite(&self, composite: &Rc<Composite>) {
        // Remember Composite
        self.state.borrow_mut().active_composite = Some(composite.clone());

        // Store in preferences
        let id = composite.get_id();
        if id != self.default_composite_id {
            self.storage_service.store(&self.active_composite_settings_key, StorageValue::String(id.clone()), StorageScope::Workspace, StorageTarget::Machine);
        } else {
            self.storage_service.remove(&self.active_composite_settings_key, StorageScope::Workspace);
        }

        // Remember
        self.state.borrow_mut().last_active_composite_id = id.clone();

        // Composites created for the first time
        let existing_container = self.state.borrow().composite_containers.get(&id).cloned();
        let composite_container = match existing_container {
            Some(composite_container) => composite_container,
            None => {
                // Build Container off-DOM
                let composite_container = self.view.create_element(None, "div", "composite");
                for class_name in self.composite_css_class.split(' ') {
                    composite_container.class_list_add(class_name);
                }

                composite.create(&composite_container);
                composite.update_styles();

                // Remember composite container
                self.state.borrow_mut().composite_containers.insert(id.clone(), composite_container.clone());
                composite_container
            }
        };

        // Fill Content and Actions
        // Make sure that the user meanwhile did not open another composite or closed the part containing the composite
        if !self.is_active(&id) {
            return;
        }

        // Take Composite on-DOM and show
        if let Some(content_area) = self.part.content_area() {
            self.view.append_child(&content_area, &composite_container);
        }
        composite_container.show();

        // Setup action runner
        let tool_bar = self.tool_bar();
        if let Some(tool_bar) = &tool_bar {
            tool_bar.set_action_runner(composite.get_action_runner());
        }

        // Update title with composite title if it differs from descriptor
        if let Some(descriptor) = self.registry.get_composite_entry(&id) {
            let composite_title = composite.get_title();
            if composite_title.as_ref() != Some(&descriptor.name) {
                self.update_title(&id, composite_title);
            }
        }

        // Handle Composite Actions
        let existing_binding = self.state.borrow().actions_bindings.get(&id).cloned();
        let actions_binding = match existing_binding {
            Some(actions_binding) => actions_binding,
            None => {
                let actions_binding = self.collect_composite_actions(Some(composite));
                self.state.borrow_mut().actions_bindings.insert(id.clone(), actions_binding.clone());
                actions_binding
            }
        };
        actions_binding();

        // Action Run Handling
        if let Some(tool_bar) = &tool_bar {
            let weak = self.weak_self.clone();
            self.actions_listener.set(Some(tool_bar.action_runner().on_did_run().listen(move |event| {
                // Check for Error
                if let Some(error) = &event.error
                    && !is_cancellation_error(error)
                    && let Some(this) = weak.upgrade()
                {
                    this.notification_service.error(vec![NotificationMessage::Error(error.clone())]);
                }
            })));
        }

        // Indicate to composite that it is now visible
        composite.set_visible(true);

        // Make sure that the user meanwhile did not open another composite or closed the part containing the composite
        if !self.is_active(&id) {
            return;
        }

        // Make sure the composite is layed out
        let content_area_size = self.state.borrow().content_area_size;
        if let Some(content_area_size) = content_area_size {
            composite.layout(content_area_size, None);
        }

        // Make sure boundary sashes are propagated
        let boundary_sashes = self.state.borrow().boundary_sashes.clone();
        if let Some(boundary_sashes) = boundary_sashes {
            composite.set_boundary_sashes(&boundary_sashes);
        }
    }

    pub fn on_title_area_update(&self, composite_id: &str) {
        // Title
        let composite = self.state.borrow().instantiated.get(composite_id).map(|item| item.composite.clone());
        if let Some(composite) = composite {
            self.update_title(composite_id, composite.get_title());
        }

        // Active Composite
        if let Some(active_composite) = self.get_active_composite().filter(|active| active.get_id() == composite_id) {
            // Actions
            let actions_binding = self.collect_composite_actions(Some(&active_composite));
            self.state.borrow_mut().actions_bindings.insert(active_composite.get_id(), actions_binding.clone());
            actions_binding();
        }
        // Otherwise invalidate actions binding for next time when the composite becomes visible
        else {
            self.state.borrow_mut().actions_bindings.remove(composite_id);
        }
    }

    fn update_title(&self, composite_id: &str, composite_title: Option<String>) {
        let Some(composite_descriptor) = self.registry.get_composite_entry(composite_id) else { return };
        let Some(title_label) = self.title_label() else { return };

        let composite_title = composite_title.filter(|title| !title.is_empty()).unwrap_or(composite_descriptor.name);

        let keybinding = self.keybinding_service.lookup_keybinding(composite_id, None, false);

        title_label.update_title(composite_id, &composite_title, keybinding.and_then(|keybinding| keybinding.get_label()).as_deref());

        if let Some(tool_bar) = self.tool_bar() {
            tool_bar.set_aria_label(&localize("ariaCompositeToolbarLabel", "{0} actions", &[composite_title.as_str()]));
        }
    }

    fn collect_composite_actions(&self, composite: Option<&Rc<Composite>>) -> Rc<dyn Fn()> {
        // From Composite
        let menu_ids: Option<Vec<MenuId>> = composite.map(|composite| composite.get_menu_ids());
        let primary_actions: Vec<ActionRef> = composite.map(|composite| composite.get_actions()).unwrap_or_default();
        let secondary_actions: Vec<ActionRef> = composite.map(|composite| composite.get_secondary_actions()).unwrap_or_default();

        // Update context
        if let Some(tool_bar) = self.tool_bar() {
            tool_bar.set_context(self.behavior.actions_context_provider(self));
        }

        // Return fn to set into toolbar
        let weak = self.weak_self.clone();
        Rc::new(move || {
            let Some(this) = weak.upgrade() else { return };

            if let Some(tool_bar) = this.tool_bar() {
                tool_bar.set_actions(&prepare_actions(primary_actions.clone()), &prepare_actions(secondary_actions.clone()), menu_ids.as_deref());
            }
            if let Some(title_area) = this.part.title_area() {
                if !primary_actions.is_empty() || !secondary_actions.is_empty() {
                    title_area.class_list_add("has-actions");
                } else {
                    title_area.class_list_remove("has-actions");
                }
            }
        })
    }

    pub fn get_active_composite(&self) -> Option<Rc<Composite>> {
        self.state.borrow().active_composite.clone()
    }

    pub fn get_last_active_composite_id(&self) -> String {
        self.state.borrow().last_active_composite_id.clone()
    }

    pub fn hide_active_composite(&self) -> Option<Rc<Composite>> {
        // Nothing to do
        let composite = self.state.borrow_mut().active_composite.take()?;

        let composite_container = self.state.borrow().composite_containers.get(&composite.get_id()).cloned();

        // Indicate to Composite
        composite.set_visible(false);

        // Take Container Off-DOM and hide
        if let Some(composite_container) = composite_container {
            self.view.remove_child(&composite_container);
            composite_container.hide();
        }

        // Clear any running Progress
        if let Some(progress_bar) = self.progress_bar() {
            progress_bar.stop();
            progress_bar.hide();
        }

        // Empty Actions
        if self.tool_bar().is_some() {
            (self.collect_composite_actions(None))();
        }
        self.on_did_composite_close.fire(composite.clone());

        Some(composite)
    }

    /// `createTitleArea(parent)` da `CompositePart`.
    pub fn create_title_area_base(&self, parent: &Rc<ViewElement>) -> Option<Rc<ViewElement>> {
        if !self.options.part.has_title {
            return None;
        }

        // Title Area Container
        let title_area = self.view.create_element(Some(parent), "div", "composite");
        title_area.class_list_add("title");

        // Left Title Label
        let title_label = self.behavior.create_title_label(self, &title_area);
        self.state.borrow_mut().title_label = Some(title_label);

        // Right Actions Container
        let title_actions_container = self.view.create_element(Some(&title_area), "div", "title-actions");

        // Toolbar
        let weak_provider = self.weak_self.clone();
        let action_view_item_provider: IActionViewItemProvider = Rc::new(move |action: &ActionRef, options: &IActionViewItemOptions| {
            let this = weak_provider.upgrade()?;
            this.behavior.action_view_item_provider(&this, action, options)
        });
        let weak_alignment = self.weak_self.clone();
        let anchor_alignment_provider: Rc<dyn Fn() -> AnchorAlignment> = Rc::new(move || weak_alignment.upgrade().map_or(AnchorAlignment::Right, |this| this.behavior.get_title_area_drop_down_anchor_alignment()));
        let hover_delegate: Rc<dyn IHoverDelegate> = self.toolbar_hover_delegate.clone();
        let tool_bar = self.view.create_tool_bar(CompositeToolBarArgs {
            container: title_actions_container,
            action_view_item_provider,
            anchor_alignment_provider,
            toggle_menu_title: localize("viewsAndMoreActions", "Views and More Actions...", &[]),
            telemetry_source: self.name_for_telemetry.clone(),
            hover_delegate,
            trailing_separator: self.options.trailing_separator,
            responsive_behavior: self.behavior.get_toolbar_responsive_behavior(),
        });
        self.base.register(tool_bar.clone());
        self.state.borrow_mut().tool_bar = Some(tool_bar);

        (self.collect_composite_actions(None))();

        Some(title_area)
    }

    /// `createTitleLabel(parent)` da `CompositePart`.
    pub fn create_title_label_base(&self, parent: &Rc<ViewElement>) -> Rc<dyn ICompositeTitleLabel> {
        let title_container = self.view.create_element(Some(parent), "div", "title-label");
        let title_label = self.view.create_element(Some(&title_container), "h2", "");
        self.state.borrow_mut().title_label_element = Some(title_label.clone());
        let label = self.view.create_title_label_view(&title_label);
        self.base.register(label.clone());

        Rc::new(DefaultTitleLabel { part: self.weak_self.clone(), label, parent: parent.clone() })
    }

    /// `createHeaderArea()`.
    pub fn create_header_area(&self) -> Rc<ViewElement> {
        self.view.create_element(None, "div", "composite")
    }

    /// `createFooterArea()`.
    pub fn create_footer_area(&self) -> Rc<ViewElement> {
        self.view.create_element(None, "div", "composite")
    }

    /// `updateStyles()` da `CompositePart`: forward to title label if present.
    pub fn update_styles_base(&self) {
        if let Some(title_label) = self.title_label() {
            title_label.update_styles();
        }
    }

    /// `actionViewItemProvider(action, options)` da `CompositePart`.
    pub fn action_view_item_provider_base(&self, action: &ActionRef, options: &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>> {
        // Check Active Composite
        if let Some(active_composite) = self.get_active_composite() {
            return active_composite.get_action_view_item(action, options);
        }

        self.view.create_action_view_item(action, options)
    }

    /// `actionsContextProvider()` da `CompositePart`.
    pub fn actions_context_provider_base(&self) -> Option<Dynamic> {
        // Check Active Composite
        self.get_active_composite().and_then(|active_composite| active_composite.get_actions_context())
    }

    /// `createContentArea(parent)` da `CompositePart`.
    pub fn create_content_area_base(&self, parent: &Rc<ViewElement>) -> Rc<ViewElement> {
        let content_container = self.view.create_element(Some(parent), "div", "content");

        let progress_bar = self.view.create_progress_bar(&content_container);
        progress_bar.hide();
        self.base.register(progress_bar.clone());
        self.state.borrow_mut().progress_bar = Some(progress_bar);

        content_container
    }

    pub fn get_progress_indicator(&self, id: &str) -> Option<Rc<dyn IProgressIndicator>> {
        let progress: Option<Rc<dyn IProgressIndicator>> = self.state.borrow().instantiated.get(id).map(|composite_item| composite_item.progress.clone() as Rc<dyn IProgressIndicator>);

        progress
    }

    /// `layout(width, height, top, left)` da `CompositePart`.
    pub fn layout_base(&self, width: f64, height: f64, top: f64, left: f64) {
        self.part.layout_base(width, height, top, left);

        // Layout contents
        let content_area_size = self.part.layout_contents(width, height).content_size;
        self.state.borrow_mut().content_area_size = Some(content_area_size);

        // Layout composite
        if let Some(active_composite) = self.get_active_composite() {
            active_composite.layout(content_area_size, None);
        }
    }

    /// `setBoundarySashes(sashes)`.
    pub fn set_boundary_sashes(&self, sashes: &BoundarySashes) {
        self.state.borrow_mut().boundary_sashes = Some(sashes.clone());
        if let Some(active_composite) = self.get_active_composite() {
            active_composite.set_boundary_sashes(sashes);
        }
    }

    pub fn remove_composite(&self, composite_id: &str) -> bool {
        // do not remove active composite
        if self.is_active(composite_id) {
            return false;
        }

        let composite_item = {
            let mut state = self.state.borrow_mut();
            state.composite_containers.remove(composite_id);
            state.actions_bindings.remove(composite_id);
            state.instantiated.remove(composite_id)
        };
        if let Some(composite_item) = composite_item {
            composite_item.composite.dispose();
            composite_item.disposable.dispose();
        }

        true
    }
}

impl IDisposable for CompositePart {
    fn dispose(&self) {
        let instantiated: Vec<CompositeItem> = {
            let mut state = self.state.borrow_mut();
            state.composite_containers.clear();
            state.actions_bindings.clear();
            state.instantiated.drain().map(|(_, composite_item)| composite_item).collect()
        };

        for composite_item in instantiated {
            composite_item.composite.dispose();
            composite_item.disposable.dispose();
        }

        self.base.dispose();
        self.part.dispose();
    }
}
