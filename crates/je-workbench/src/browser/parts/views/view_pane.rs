//! Porte de `src/vs/workbench/browser/parts/views/viewPane.ts`: a [`ViewPane`] (o `Pane` de
//! `base/browser/ui/splitview/paneview.ts` com cabeçalho, ações, barra de progresso, mensagem de boas-vindas e foco), o
//! controlador da mensagem de boas-vindas ([`ViewWelcomeController`]), `getLocationBasedViewColors` e as opções.
//!
//! Divergências:
//! - **Herança.** `ViewPane extends Pane` é um [`ViewPane`] que guarda um [`Pane`] (`Deref` leva a ele). Quem herda da
//!   `ViewPane` (o `ExplorerView`...) implementa [`ViewPaneDelegate`] (os métodos que o original deixa para sobrescrever:
//!   `renderBody`, `layoutBody`, `getActionsContext`, `getActionRunner`, `getOptimalWidth`, `saveState`, `shouldShowWelcome`,
//!   `onDidScrollRoot`) e o registra com [`ViewPane::set_delegate`] (fraco: o delegado costuma guardar a `ViewPane`). O
//!   `primaryActionGroups`, que o construtor lê da subclasse, é o campo [`IViewPaneOptions::primary_action_groups`].
//! - **Injeção.** Os dez serviços do construtor e os de `createInstance` (`WorkbenchToolBar`, `createActionViewItem`, a
//!   mensagem de boas-vindas) são os campos de [`ViewPaneServices`]. A barra de ferramentas, o item de ação e a mensagem de
//!   boas-vindas são fábricas ([`ViewPaneToolbarFactory`], [`ActionViewItemFactory`], [`ViewWelcomeViewFactory`]) porque o
//!   `WorkbenchToolBar`, o `createActionViewItem` e os widgets de parágrafo com `Link` e rolagem (`DomScrollableElement`) ainda
//!   não existem no porte; a lógica (itens, visibilidade por `when`, valor padrão, precondições) é toda deste módulo.
//! - **`IHoverService`.** É o `IHoverDelegate2` de `je-widgets` (o que tem `setupManagedHover`).
//! - **Cabeçalho.** O flex do `.pane-header` é calculado em [`ViewPane::layout_header`]: o gatilho é o `layoutBody`, então um
//!   pane horizontal recolhido (que o `Pane` não manda desenhar o corpo) mantém o último layout. `text-transform: uppercase`
//!   do título é o texto em caixa alta. O ícone por `URI` do original vira uma máscara CSS (`mask: url(...)`), que o `je-ui`
//!   não tem: o nó recebe a classe `view-<id>` e o `aria-label`, e quem desenha resolve a imagem.
//! - **Ícones do twisty.** O `registerIcon('view-pane-container-expanded', Codicon.chevronDown)` é o próprio codicon
//!   (`chevron-down` e `chevron-right`).
//! - **Fora deste módulo.** `FilterViewPane`, `VIEWPANE_FILTER_ACTION` e o item de filtro do cabeçalho dependem do
//!   `FilterWidget` (`viewFilter.ts`), e o `ViewAction` do `IViewsService.getActiveViewWithId` genérico; nenhum dos dois
//!   existe no porte ainda.
//! - **`accessibleViewInformationService`.** O serviço (`accessibleViewInformationService.ts`) não foi portado; o recorte que a
//!   `ViewPane` usa é o trait [`IAccessibleViewInformationService`] daqui.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, IActionRunner};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{Error, on_unexpected_error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::themables::ThemeIcon;
use je_platform::actions::menu_ids;
use je_platform::actions::{IMenuService, MenuActionOptions, MenuId};
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::{IContextKey, IContextKeyService, IScopedContextKeyService};
use je_platform::contextview::context_view::IContextMenuService;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::opener::opener::IOpenerService;
use je_render::Rgba;
use je_ui::{Content, EventKind, NodeId, Role, Ui};
use je_widgets::action_bar::{IActionViewItemProvider, prepare_actions};
use je_widgets::action_view_items::{IActionViewItem, IActionViewItemOptions};
use je_widgets::dom::{FocusTracker, create_el, is_active_element, is_ancestor_of_active_element};
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::hover_api::{HoverPlacement, IHoverDelegate2, IManagedHover, ManagedHoverContent, ManagedHoverContentOrFactory, get_default_hover_delegate};
use je_widgets::paneview::{Pane, PaneContent, PaneOptions, PaneStyles};
use je_widgets::progress_bar::{IProgressBarOptions, ProgressBar};
use je_widgets::sash::Orientation;
use je_widgets::text::{measure_text, ui_font};
use je_widgets::toolbar::{IToolBarOptions, ToolBar};

use crate::browser::parts::views::view_menu_actions::{IViewMenuActionsOptions, ViewMenuActions};
use crate::common::views::{
    IView, IViewContentDescriptor, IViewDescriptorService, ViewContainerLocation, ViewContentWhen, ViewIcon, default_view_icon, view_container_location_to_string, views_registry,
};
use crate::services::lifecycle::lifecycle::ILifecycleService;
use crate::services::progress::progress_indicator::{AbstractProgressScope, IProgressScope, ScopedProgressIndicator};
use crate::services::views::views_service::IViewPane;

/// `ViewPaneShowActions`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewPaneShowActions {
    /// Show the actions when the view is hovered. This is the default behavior.
    #[default]
    Default,

    /// Always shows the actions when the view is expanded.
    WhenExpanded,

    /// Always shows the actions.
    Always,
}

/// `IViewPaneOptions` (que estende `IPaneOptions`).
#[derive(Clone, Default)]
pub struct IViewPaneOptions {
    pub pane: PaneOptions,
    pub id: String,
    pub show_actions: Option<ViewPaneShowActions>,
    pub title_menu_id: Option<MenuId>,
    pub donot_forward_args: bool,
    /// The title of the container pane when it is merged with the view container.
    pub single_view_pane_container_title: Option<String>,
    /// O `primaryActionGroups` que a subclasse sobrescreve: grupos de menu (além de `navigation`) que viram ações primárias
    /// (em linha) na barra do título, separados por separadores.
    pub primary_action_groups: Option<Vec<String>>,
}

/// O `IAccessibleViewInformationService` no que a `ViewPane` usa.
pub trait IAccessibleViewInformationService {
    /// `hasShownAccessibleView(viewId)`.
    fn has_shown_accessible_view(&self, view_id: &str) -> bool;
}

/// A barra de ferramentas do cabeçalho (o `WorkbenchToolBar` no que a `ViewPane` usa).
pub trait IViewPaneToolbar: IDisposable {
    /// `setActions(primaryActions, secondaryActions)`.
    fn set_actions(&self, primary_actions: &[ActionRef], secondary_actions: &[ActionRef]);

    /// `context = value`.
    fn set_context(&self, context: Option<Dynamic>);
}

impl IViewPaneToolbar for ToolBar {
    fn set_actions(&self, primary_actions: &[ActionRef], secondary_actions: &[ActionRef]) {
        ToolBar::set_actions(self, primary_actions, secondary_actions);
    }

    fn set_context(&self, context: Option<Dynamic>) {
        ToolBar::set_context(self, context);
    }
}

/// O que o `WorkbenchToolBar` do cabeçalho recebe: o contêiner das ações, as opções e o `resetMenu`.
pub struct ViewPaneToolbarArgs {
    pub container: NodeId,
    pub options: IToolBarOptions,
    pub reset_menu: MenuId,
}

/// `createInstance(WorkbenchToolBar, actions, options)`.
pub type ViewPaneToolbarFactory = Rc<dyn Fn(ViewPaneToolbarArgs) -> Rc<dyn IViewPaneToolbar>>;

/// `createActionViewItem(instantiationService, action, { ...options, menuAsChild })`: `menu_as_child` é o
/// `action instanceof SubmenuItemAction`.
pub type ActionViewItemFactory = Rc<dyn Fn(&ActionRef, &IActionViewItemOptions, bool) -> Option<Rc<dyn IActionViewItem>>>;

/// A mensagem de boas-vindas desenhada: o conteúdo (parágrafos, botões e links) montado no contêiner `.welcome-view-content`.
pub trait IViewWelcomeView: IDisposable {
    /// `render()`: desenha os descritores (apaga o desenho anterior). As linhas com um único link viram botões; as outras,
    /// parágrafos com texto e links. As precondições habilitam e desabilitam os botões e os links conforme o contexto.
    fn render(&self, contents: &[Rc<IViewContentDescriptor>]);

    /// `layout(height, width)` do elemento (a classe `wide` é `width > 640`, calculada por quem chama).
    fn layout(&self, height: f64, width: f64, wide: bool);

    /// `element.focus()`.
    fn focus(&self);
}

/// O que a fábrica da mensagem de boas-vindas recebe.
pub struct ViewWelcomeViewArgs {
    /// O `.welcome-view` dentro do corpo do pane.
    pub container: NodeId,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub opener_service: Rc<dyn IOpenerService>,
}

pub type ViewWelcomeViewFactory = Rc<dyn Fn(ViewWelcomeViewArgs) -> Rc<dyn IViewWelcomeView>>;

/// Os serviços do construtor da `ViewPane` e dos widgets que ela cria.
#[derive(Clone)]
pub struct ViewPaneServices {
    pub ui: Ui,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub view_descriptor_service: Rc<dyn IViewDescriptorService>,
    pub menu_service: Rc<dyn IMenuService>,
    pub opener_service: Rc<dyn IOpenerService>,
    pub hover_service: Rc<dyn IHoverDelegate2>,
    pub lifecycle_service: Rc<dyn ILifecycleService>,
    pub accessible_view_information_service: Option<Rc<dyn IAccessibleViewInformationService>>,
    /// O estilo do texto do cabeçalho: `foreground`, 11px, negrito, `line-height` do `--pane-header-size`.
    pub header_styles: HighlightedLabelStyles,
    /// `--vscode-panelTitle-inactiveForeground`: a cor da descrição do título.
    pub description_foreground: Rgba,
    pub progress_bar_options: IProgressBarOptions,
    pub create_toolbar: ViewPaneToolbarFactory,
    pub create_action_view_item: ActionViewItemFactory,
    pub create_welcome_view: ViewWelcomeViewFactory,
}

/// O que a subclasse da `ViewPane` sobrescreve. Os padrões são os do original.
pub trait ViewPaneDelegate {
    /// `renderBody(container)` (a `ViewPane` cuida do `super.renderBody`, que cria a mensagem de boas-vindas).
    fn render_body(&self, _container: NodeId) {}

    /// `layoutBody(height, width)` (depois do `super.layoutBody`).
    fn layout_body(&self, _height: f64, _width: f64) {}

    /// `getActionsContext()`.
    fn get_actions_context(&self) -> Option<Dynamic> {
        None
    }

    /// `getActionRunner()`.
    fn get_action_runner(&self) -> Option<Rc<dyn IActionRunner>> {
        None
    }

    /// `getOptimalWidth()`.
    fn get_optimal_width(&self) -> f64 {
        0.0
    }

    /// `saveState()`: as subclasses implementam para salvar o estado.
    fn save_state(&self) {}

    /// `shouldShowWelcome()`.
    fn should_show_welcome(&self) -> bool {
        false
    }

    /// `onDidScrollRoot()`.
    fn on_did_scroll_root(&self) {}
}

/// O `IViewWelcomeDelegate`.
pub trait IViewWelcomeDelegate {
    fn id(&self) -> String;
    fn on_did_change_view_welcome_state(&self) -> Event<()>;
    fn should_show_welcome(&self) -> bool;
}

/// `IItem`.
struct WelcomeItem {
    descriptor: Rc<IViewContentDescriptor>,
    visible: Cell<bool>,
}

/// `ViewWelcomeController`.
pub struct ViewWelcomeController {
    ui: Ui,
    container: NodeId,
    delegate: Rc<dyn IViewWelcomeDelegate>,
    context_key_service: Rc<dyn IContextKeyService>,
    opener_service: Rc<dyn IOpenerService>,
    create_welcome_view: ViewWelcomeViewFactory,
    weak_self: Weak<ViewWelcomeController>,
    default_item: RefCell<Option<Rc<WelcomeItem>>>,
    items: RefCell<Vec<Rc<WelcomeItem>>>,
    enabled: Cell<bool>,
    wide: Cell<bool>,
    view: RefCell<Option<Rc<dyn IViewWelcomeView>>>,
    welcome_view_container: Cell<Option<NodeId>>,
    disposables: Rc<DisposableStore>,
    enabled_disposables: Rc<DisposableStore>,
}

impl ViewWelcomeController {
    pub fn new(
        ui: &Ui,
        container: NodeId,
        delegate: Rc<dyn IViewWelcomeDelegate>,
        context_key_service: Rc<dyn IContextKeyService>,
        opener_service: Rc<dyn IOpenerService>,
        lifecycle_service: Rc<dyn ILifecycleService>,
        create_welcome_view: ViewWelcomeViewFactory,
    ) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let enabled_disposables = DisposableStore::new();
        disposables.add(enabled_disposables.clone());

        let this = Rc::new_cyclic(|weak| Self {
            ui: ui.clone(),
            container,
            delegate,
            context_key_service,
            opener_service,
            create_welcome_view,
            weak_self: weak.clone(),
            default_item: RefCell::new(None),
            items: RefCell::new(Vec::new()),
            enabled: Cell::new(false),
            wide: Cell::new(false),
            view: RefCell::new(None),
            welcome_view_container: Cell::new(None),
            disposables,
            enabled_disposables,
        });

        let weak = this.weak_self.clone();
        this.disposables.add(this.delegate.on_did_change_view_welcome_state().run_and_subscribe(
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_view_welcome_state();
                }
            },
            None,
        ));

        // Fixes https://github.com/microsoft/vscode/issues/208878
        let weak = this.weak_self.clone();
        this.disposables.add(lifecycle_service.on_will_shutdown().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.dispose();
            }
        }));

        this
    }

    /// `enabled`.
    pub fn enabled(&self) -> bool {
        self.enabled.get()
    }

    /// `layout(height, width)`.
    pub fn layout(&self, height: f64, width: f64) {
        if !self.enabled.get() {
            return;
        }

        self.wide.set(width > 640.0);
        if let Some(view) = self.view.borrow().clone() {
            view.layout(height, width, self.wide.get());
        }
    }

    /// `focus()`.
    pub fn focus(&self) {
        if !self.enabled.get() {
            return;
        }

        if let Some(view) = self.view.borrow().clone() {
            view.focus();
        }
    }

    fn on_did_change_view_welcome_state(self: &Rc<Self>) {
        let enabled = self.delegate.should_show_welcome();

        if self.enabled.get() == enabled {
            return;
        }

        self.enabled.set(enabled);

        if !enabled {
            self.enabled_disposables.clear();
            return;
        }

        self.ui.tree_mut().add_class(self.container, "welcome");
        let welcome_view_container = create_el(&mut self.ui.tree_mut(), self.container, Role::Region, "welcome-view");
        self.welcome_view_container.set(Some(welcome_view_container));
        let view = (self.create_welcome_view)(ViewWelcomeViewArgs {
            container: welcome_view_container,
            context_key_service: self.context_key_service.clone(),
            opener_service: self.opener_service.clone(),
        });
        *self.view.borrow_mut() = Some(view.clone());

        let (weak, ui, container) = (self.weak_self.clone(), self.ui.clone(), self.container);
        self.enabled_disposables.add(je_base::lifecycle::to_disposable(move || {
            ui.tree_mut().remove_class(container, "welcome");
            if let Some(this) = weak.upgrade() {
                if let Some(view) = this.view.borrow_mut().take() {
                    view.dispose();
                }
                if let Some(node) = this.welcome_view_container.take() {
                    ui.remove(node);
                }
            }
        }));

        let weak = self.weak_self.clone();
        self.enabled_disposables.add(self.context_key_service.on_did_change_context().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_context();
            }
        }));

        let (weak, id) = (self.weak_self.clone(), self.delegate.id());
        self.enabled_disposables.add(views_registry().on_did_change_view_welcome_content().listen(move |changed| {
            if *changed == id
                && let Some(this) = weak.upgrade()
            {
                this.on_did_change_view_welcome_content();
            }
        }));
        self.on_did_change_view_welcome_content();
    }

    fn on_did_change_view_welcome_content(&self) {
        let descriptors = views_registry().get_view_welcome_content(&self.delegate.id());

        let mut items = Vec::new();
        for descriptor in descriptors {
            if matches!(descriptor.when, Some(ViewContentWhen::Default)) {
                *self.default_item.borrow_mut() = Some(Rc::new(WelcomeItem { descriptor, visible: Cell::new(true) }));
            } else {
                let visible = match &descriptor.when {
                    Some(ViewContentWhen::Expression(expression)) => self.context_key_service.context_matches_rules(Some(expression)),
                    _ => true,
                };
                items.push(Rc::new(WelcomeItem { descriptor, visible: Cell::new(visible) }));
            }
        }
        *self.items.borrow_mut() = items;

        self.render();
    }

    fn on_did_change_context(&self) {
        let mut did_change = false;

        for item in self.items.borrow().iter() {
            let Some(ViewContentWhen::Expression(expression)) = &item.descriptor.when else { continue };

            let visible = self.context_key_service.context_matches_rules(Some(expression));

            if item.visible.get() == visible {
                continue;
            }

            item.visible.set(visible);
            did_change = true;
        }

        if did_change {
            self.render();
        }
    }

    fn render(&self) {
        let Some(view) = self.view.borrow().clone() else { return };
        let contents = self.get_content_descriptors();

        view.render(&contents);

        // `contents.length === 0`: `container.classList.remove('welcome')`; senão, `add('welcome')`.
        self.ui.tree_mut().toggle_class(self.container, "welcome", !contents.is_empty());
    }

    fn get_content_descriptors(&self) -> Vec<Rc<IViewContentDescriptor>> {
        let visible_items: Vec<Rc<WelcomeItem>> = self.items.borrow().iter().filter(|item| item.visible.get()).cloned().collect();

        if visible_items.is_empty()
            && let Some(default_item) = self.default_item.borrow().as_ref()
        {
            return vec![default_item.descriptor.clone()];
        }

        visible_items.into_iter().map(|item| item.descriptor.clone()).collect()
    }
}

impl IDisposable for ViewWelcomeController {
    fn dispose(&self) {
        self.disposables.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ViewWelcomeController"
    }
}

/// O delegado de boas-vindas que a `ViewPane` é no original (`new ViewWelcomeController(container, this)`).
struct PaneWelcomeDelegate {
    id: String,
    on_did_change_view_welcome_state: Event<()>,
    pane: Weak<ViewPane>,
}

impl IViewWelcomeDelegate for PaneWelcomeDelegate {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn on_did_change_view_welcome_state(&self) -> Event<()> {
        self.on_did_change_view_welcome_state.clone()
    }

    fn should_show_welcome(&self) -> bool {
        self.pane.upgrade().is_some_and(|pane| pane.should_show_welcome())
    }
}

/// Os três métodos abstratos do `Pane`, delegados à `ViewPane`.
struct ViewPaneContent {
    pane: RefCell<Weak<ViewPane>>,
}

impl PaneContent for ViewPaneContent {
    fn render_header(&self, container: NodeId) {
        if let Some(pane) = self.pane.borrow().upgrade() {
            pane.render_header(container);
        }
    }

    fn render_body(&self, container: NodeId) {
        if let Some(pane) = self.pane.borrow().upgrade() {
            pane.render_body(container);
        }
    }

    fn layout_body(&self, height: f64, width: f64) {
        if let Some(pane) = self.pane.borrow().upgrade() {
            pane.layout_body(height, width);
        }
    }
}

/// Os nós e os recursos do cabeçalho, criados em `renderHeader`.
#[derive(Default)]
struct HeaderParts {
    header_container: Option<NodeId>,
    twisties_container: Option<NodeId>,
    icon_container: Option<NodeId>,
    title_container: Option<NodeId>,
    title_description_container: Option<NodeId>,
    actions_container: Option<NodeId>,
    title_container_hover: Option<Rc<dyn IManagedHover>>,
    title_description_container_hover: Option<Rc<dyn IManagedHover>>,
    icon_container_hover: Option<Rc<dyn IManagedHover>>,
}

/// A barra de progresso do pane e o indicador sobre ela.
#[derive(Default)]
struct ProgressParts {
    bar: Option<Rc<ProgressBar>>,
    indicator: Option<Rc<ScopedProgressIndicator>>,
}

const ALWAYS_SHOW_ACTIONS_CONFIG: &str = "workbench.view.alwaysShowHeaderActions";

/// A altura do cabeçalho (`--pane-header-size`).
const HEADER_HEIGHT: f64 = 22.0;

/// `ViewPane`.
pub struct ViewPane {
    base: Rc<Disposable>,
    weak_self: Weak<ViewPane>,
    ui: Ui,
    pane: Rc<Pane>,
    services: ViewPaneServices,
    delegate: RefCell<Option<Weak<dyn ViewPaneDelegate>>>,

    on_did_focus: Rc<Emitter<()>>,
    on_did_blur: Rc<Emitter<()>>,
    on_did_change_body_visibility: Rc<Emitter<bool>>,
    on_did_change_title_area: Rc<Emitter<()>>,
    on_did_change_view_welcome_state: Rc<Emitter<()>>,

    is_visible: Cell<bool>,
    pub id: String,
    title: RefCell<String>,
    title_description: RefCell<Option<String>>,
    single_view_pane_container_title: Option<String>,
    show_actions: ViewPaneShowActions,
    pub menu_actions: Rc<ViewMenuActions>,
    scoped_context_key_service: Rc<dyn IScopedContextKeyService>,

    progress: RefCell<ProgressParts>,
    toolbar: RefCell<Option<Rc<dyn IViewPaneToolbar>>>,
    header: RefCell<HeaderParts>,
    view_welcome_controller: RefCell<Option<Rc<ViewWelcomeController>>>,
    header_action_view_items: RefCell<HashMap<String, Rc<dyn IActionViewItem>>>,
    header_width: Cell<f64>,
    hovered: Cell<bool>,
    focus_within: Cell<bool>,
    header_focused: Cell<bool>,
    render_disposables: Rc<DisposableStore>,
}

impl Deref for ViewPane {
    type Target = Pane;

    fn deref(&self) -> &Pane {
        &self.pane
    }
}

/// `this._register(new Emitter<T>())`.
fn registered_emitter<T: 'static>(base: &Rc<Disposable>) -> Rc<Emitter<T>> {
    let emitter = Emitter::new();
    base.register(emitter.clone());
    emitter
}

/// `registerIcon('view-pane-container-expanded', Codicon.chevronDown)` e `...-collapsed` (`Codicon.chevronRight`).
fn twisty_icon(expanded: bool) -> ThemeIcon {
    ThemeIcon::from_id(if expanded { "chevron-down" } else { "chevron-right" })
}

impl ViewPane {
    pub fn new(options: IViewPaneOptions, services: ViewPaneServices) -> Rc<Self> {
        let ui = services.ui.clone();
        let location = services.view_descriptor_service.get_view_location_by_id(&options.id);

        let mut pane_options = options.pane.clone();
        pane_options.orientation = Some(if location == Some(ViewContainerLocation::Panel) { Orientation::Horizontal } else { Orientation::Vertical });

        let content = Rc::new(ViewPaneContent { pane: RefCell::new(Weak::new()) });
        let pane = Pane::new(&ui, pane_options, content.clone());

        let base = Disposable::new();
        let scoped_context_key_service = services.context_key_service.create_scoped(crate::browser::parts::editor::editor_group_view::ContextKeyTarget::new());
        base.register(scoped_context_key_service.clone());
        scoped_context_key_service.create_key("view", Dynamic::from(options.id.as_str()));
        let location_name = |service: &dyn IViewDescriptorService, id: &str| Dynamic::from(service.get_view_location_by_id(id).map_or("", view_container_location_to_string));
        let view_location_key: Rc<dyn IContextKey> = scoped_context_key_service.create_key("viewLocation", location_name(services.view_descriptor_service.as_ref(), &options.id));
        {
            let (service, id) = (services.view_descriptor_service.clone(), options.id.clone());
            base.register(services.view_descriptor_service.on_did_change_location().listen(move |change| {
                if change.views.iter().any(|view| view.id == id) {
                    view_location_key.set(location_name(service.as_ref(), &id));
                }
            }));
        }

        let menu_actions = ViewMenuActions::new(
            options.title_menu_id.clone().unwrap_or_else(menu_ids::view_title),
            Some(menu_ids::view_title_context()),
            Some(MenuActionOptions { should_forward_args: !options.donot_forward_args, render_short_title: true, ..MenuActionOptions::default() }),
            Some(IViewMenuActionsOptions { primary_action_groups: options.primary_action_groups.clone() }),
            scoped_context_key_service.clone(),
            services.menu_service.clone(),
        );
        base.register(menu_actions.clone());

        let this = Rc::new_cyclic(|weak| Self {
            weak_self: weak.clone(),
            ui,
            pane,
            delegate: RefCell::new(None),
            on_did_focus: registered_emitter(&base),
            on_did_blur: registered_emitter(&base),
            on_did_change_body_visibility: registered_emitter(&base),
            on_did_change_title_area: registered_emitter(&base),
            on_did_change_view_welcome_state: registered_emitter(&base),
            is_visible: Cell::new(false),
            id: options.id.clone(),
            title: RefCell::new(options.pane.title.clone()),
            title_description: RefCell::new(options.pane.title_description.clone()),
            single_view_pane_container_title: options.single_view_pane_container_title.clone(),
            show_actions: options.show_actions.unwrap_or_default(),
            menu_actions,
            scoped_context_key_service,
            progress: RefCell::new(ProgressParts::default()),
            toolbar: RefCell::new(None),
            header: RefCell::new(HeaderParts::default()),
            view_welcome_controller: RefCell::new(None),
            header_action_view_items: RefCell::new(HashMap::new()),
            header_width: Cell::new(0.0),
            hovered: Cell::new(false),
            focus_within: Cell::new(false),
            header_focused: Cell::new(false),
            render_disposables: DisposableStore::new(),
            services,
            base,
        });
        *content.pane.borrow_mut() = Rc::downgrade(&this);
        this.base.register(this.render_disposables.clone());

        let weak = this.weak_self.clone();
        this.base.register(this.menu_actions.on_did_change().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_actions();
            }
        }));

        this
    }

    /// Registra a subclasse (ver o `//!` do módulo).
    pub fn set_delegate(&self, delegate: Weak<dyn ViewPaneDelegate>) {
        *self.delegate.borrow_mut() = Some(delegate);
    }

    fn delegate(&self) -> Option<Rc<dyn ViewPaneDelegate>> {
        self.delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    // Eventos

    /// `onDidFocus`.
    pub fn on_did_focus(&self) -> Event<()> {
        self.on_did_focus.event()
    }

    /// `onDidBlur`.
    pub fn on_did_blur(&self) -> Event<()> {
        self.on_did_blur.event()
    }

    /// `onDidChangeBodyVisibility`.
    pub fn on_did_change_body_visibility(&self) -> Event<bool> {
        self.on_did_change_body_visibility.event()
    }

    /// `onDidChangeTitleArea`.
    pub fn on_did_change_title_area(&self) -> Event<()> {
        self.on_did_change_title_area.event()
    }

    /// `onDidChangeViewWelcomeState`.
    pub fn on_did_change_view_welcome_state(&self) -> Event<()> {
        self.on_did_change_view_welcome_state.event()
    }

    /// Dispara o `_onDidChangeViewWelcomeState` (o `protected` do original, para a subclasse).
    pub fn fire_did_change_view_welcome_state(&self) {
        self.on_did_change_view_welcome_state.fire(());
    }

    /// `scopedContextKeyService` (`protected`).
    pub fn scoped_context_key_service(&self) -> Rc<dyn IScopedContextKeyService> {
        self.scoped_context_key_service.clone()
    }

    /// Os serviços do construtor (os `protected` do original, para a subclasse).
    pub fn services(&self) -> &ViewPaneServices {
        &self.services
    }

    // Propriedades

    /// `title`.
    pub fn title(&self) -> String {
        self.title.borrow().clone()
    }

    /// `titleDescription`.
    pub fn title_description(&self) -> Option<String> {
        self.title_description.borrow().clone()
    }

    /// `singleViewPaneContainerTitle`.
    pub fn single_view_pane_container_title(&self) -> Option<String> {
        self.single_view_pane_container_title.clone()
    }

    /// `set headerVisible(visible)`.
    pub fn set_header_visible(&self, visible: bool) {
        self.pane.set_header_visible(visible);
        self.ui.tree_mut().toggle_class(self.pane.element(), "merged-header", !visible);
        self.update_progress_bar_position();
    }

    // Visibilidade

    /// `setVisible(visible)`.
    pub fn set_visible(&self, visible: bool) {
        if self.is_visible.get() != visible {
            self.is_visible.set(visible);

            if self.pane.is_expanded() {
                self.on_did_change_body_visibility.fire(visible);
            }
        }
    }

    /// `isVisible()`.
    pub fn is_visible(&self) -> bool {
        self.is_visible.get()
    }

    /// `isBodyVisible()`.
    pub fn is_body_visible(&self) -> bool {
        self.is_visible.get() && self.pane.is_expanded()
    }

    /// `setExpanded(expanded)`.
    pub fn set_expanded(&self, expanded: bool) -> bool {
        let changed = self.pane.set_expanded(expanded);
        if changed {
            self.on_did_change_body_visibility.fire(expanded);
        }
        self.update_twisty_icon();
        self.update_actions_display();
        changed
    }

    /// `render()`: monta o pane e passa a seguir o foco dele.
    pub fn render(self: &Rc<Self>) {
        self.pane.render();
        let element = self.pane.element();

        let focus_tracker = FocusTracker::new(&self.ui, element);
        self.render_disposables.add(focus_tracker.clone());
        let weak = self.weak_self.clone();
        self.render_disposables.add(focus_tracker.on_did_focus().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.focus_within.set(true);
                this.update_actions_display();
                this.on_did_focus.fire(());
            }
        }));
        let weak = self.weak_self.clone();
        self.render_disposables.add(focus_tracker.on_did_blur().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.focus_within.set(false);
                this.update_actions_display();
                this.on_did_blur.fire(());
            }
        }));

        // `.pane:hover > .pane-header.expanded > .actions`
        let weak = self.weak_self.clone();
        self.render_disposables.add(self.ui.listen(element, EventKind::PointerEnter, false, move |_| {
            if let Some(this) = weak.upgrade() {
                this.hovered.set(true);
                this.update_actions_display();
            }
        }));
        let weak = self.weak_self.clone();
        self.render_disposables.add(self.ui.listen(element, EventKind::PointerLeave, false, move |_| {
            if let Some(this) = weak.upgrade() {
                this.hovered.set(false);
                this.update_actions_display();
            }
        }));
    }

    // Cabeçalho

    fn render_header(self: &Rc<Self>, container: NodeId) {
        self.header.borrow_mut().header_container = Some(container);

        let twisties_container = {
            let mut tree = self.ui.tree_mut();
            let node = create_el(&mut tree, container, Role::Presentation, "twisty-container");
            tree.style_mut(node).pointer_events = je_ui::PointerEvents::None;
            node
        };
        self.header.borrow_mut().twisties_container = Some(twisties_container);
        self.update_twisty_icon();

        let title = self.title();
        self.render_header_title(container, &title);

        let actions = {
            let mut tree = self.ui.tree_mut();
            let actions = create_el(&mut tree, container, Role::Generic, "actions");
            tree.toggle_class(actions, "show-always", self.show_actions == ViewPaneShowActions::Always);
            tree.toggle_class(actions, "show-expanded", self.show_actions == ViewPaneShowActions::WhenExpanded);
            actions
        };
        self.header.borrow_mut().actions_container = Some(actions);

        let weak = self.weak_self.clone();
        let action_view_item_provider: IActionViewItemProvider = Rc::new(move |action: &ActionRef, options: &IActionViewItemOptions| weak.upgrade()?.create_action_view_item(action, options));
        let keybinding_service = self.services.keybinding_service.clone();
        let toolbar = (self.services.create_toolbar)(ViewPaneToolbarArgs {
            container: actions,
            options: IToolBarOptions {
                orientation: Some(je_widgets::action_bar::ActionsOrientation::Horizontal),
                action_view_item_provider: Some(action_view_item_provider),
                aria_label: Some(localize("viewToolbarAriaLabel", "{0} actions", &[&self.title()])),
                get_key_binding: Some(Rc::new(move |action: &ActionRef| keybinding_service.lookup_keybinding(&action.id(), None, false))),
                render_dropdown_as_child_element: true,
                action_runner: self.delegate().and_then(|delegate| delegate.get_action_runner()),
                ..IToolBarOptions::default()
            },
            reset_menu: self.menu_actions.menu_id.clone(),
        });
        self.base.register(toolbar.clone());
        *self.toolbar.borrow_mut() = Some(toolbar);
        self.set_actions();

        self.render_disposables.add(self.ui.listen(actions, EventKind::Click, false, |e| e.prevent_default()));

        // `.pane > .pane-header.focused`: o cabeçalho com foco mostra as ações.
        let weak = self.weak_self.clone();
        self.render_disposables.add(self.ui.listen(container, EventKind::Focus, true, move |_| {
            if let Some(this) = weak.upgrade() {
                this.header_focused.set(true);
                this.update_actions_display();
            }
        }));
        let weak = self.weak_self.clone();
        self.render_disposables.add(self.ui.listen(container, EventKind::Blur, true, move |_| {
            if let Some(this) = weak.upgrade() {
                this.header_focused.set(false);
                this.update_actions_display();
            }
        }));

        let view_container = self.services.view_descriptor_service.get_view_container_by_view_id(&self.id);
        if let Some(view_container) = view_container {
            let weak = self.weak_self.clone();
            self.base.register(self.services.view_descriptor_service.get_view_container_model(&view_container).on_did_change_container_info().listen(move |change| {
                let _ = change.title;
                if let Some(this) = weak.upgrade() {
                    let title = this.title();
                    this.update_title(&title);
                }
            }));
        } else {
            on_unexpected_error(&Error::new(format!("View container model not found for view {}", self.id)));
        }

        let weak = self.weak_self.clone();
        self.base.register(self.services.configuration_service.on_did_change_configuration().listen(move |e| {
            if e.affects_configuration(ALWAYS_SHOW_ACTIONS_CONFIG, None)
                && let Some(this) = weak.upgrade()
            {
                this.update_actions_visibility();
            }
        }));
        self.update_actions_visibility();
        self.update_actions_display();
    }

    /// O que `updateHeader` faz a mais depois do `Pane`: o ícone do twisty acompanha o estado.
    fn update_twisty_icon(&self) {
        let Some(node) = self.header.borrow().twisties_container else { return };
        let expanded = self.pane.is_expanded();
        let color = self.pane.styles().header_foreground.unwrap_or(self.services.header_styles.foreground);
        self.ui.tree_mut().style_mut(node).content = Content::Codicon { id: twisty_icon(expanded).id, size: 16.0, color };
        self.layout_header(self.header_width.get());
    }

    /// `getTwistyIcon(expanded)` (`protected`).
    pub fn get_twisty_icon(&self, expanded: bool) -> ThemeIcon {
        twisty_icon(expanded)
    }

    /// `style(styles)`.
    pub fn style(&self, styles: PaneStyles) {
        self.pane.style(styles);

        let icon = self.get_icon();
        let icon_container = self.header.borrow().icon_container;
        if let Some(icon_container) = icon_container {
            let fg_color = styles.header_foreground.unwrap_or(self.services.header_styles.foreground);
            let mut tree = self.ui.tree_mut();
            match icon {
                // Apply background color to activity bar item provided with iconUrls
                ViewIcon::Uri(_) => tree.style_mut(icon_container).background = Some(fg_color),
                // Apply foreground color to activity bar items provided with codicons
                ViewIcon::Theme(theme_icon) => {
                    tree.style_mut(icon_container).background = None;
                    tree.style_mut(icon_container).content = Content::Codicon { id: theme_icon.id, size: 16.0, color: fg_color };
                }
            }
        }
        self.update_twisty_icon();
        self.set_title_texts();
    }

    fn get_icon(&self) -> ViewIcon {
        self.services.view_descriptor_service.get_view_descriptor_by_id(&self.id).and_then(|descriptor| descriptor.container_icon.clone()).unwrap_or_else(|| ViewIcon::Theme(default_view_icon()))
    }

    fn render_header_title(&self, container: NodeId, title: &str) {
        let icon = self.get_icon();
        let (icon_container, title_container) = {
            let mut tree = self.ui.tree_mut();
            let icon_container = create_el(&mut tree, container, Role::Image, "icon");
            tree.style_mut(icon_container).visible = false;
            let title_container = create_el(&mut tree, container, Role::Heading, "title");
            (icon_container, title_container)
        };

        {
            let mut tree = self.ui.tree_mut();
            match &icon {
                ViewIcon::Uri(_) => tree.add_class(icon_container, &format!("view-{}", self.id.replace(['.', ':'], "-"))),
                ViewIcon::Theme(theme_icon) => {
                    tree.style_mut(icon_container).content = Content::Codicon { id: theme_icon.id.clone(), size: 16.0, color: self.services.header_styles.foreground };
                    for class_name in theme_icon.as_class_name().split(' ') {
                        tree.add_class(icon_container, class_name);
                    }
                }
            }
        }

        let calculated_title = self.calculate_title(title);
        {
            let mut header = self.header.borrow_mut();
            header.icon_container = Some(icon_container);
            header.title_container = Some(title_container);
        }
        self.set_title_texts();

        let hover = |node: NodeId, text: &str| -> Rc<dyn IManagedHover> {
            let hover = self.services.hover_service.setup_managed_hover(get_default_hover_delegate(HoverPlacement::Mouse), node, ManagedHoverContentOrFactory::Content(ManagedHoverContent::Text(text.to_owned())), None);
            self.base.register(hover.clone());
            hover
        };
        self.header.borrow_mut().title_container_hover = Some(hover(title_container, &calculated_title));

        if let Some(description) = self.title_description() {
            self.set_title_description(Some(&description));
        }

        self.header.borrow_mut().icon_container_hover = Some(hover(icon_container, &calculated_title));
        let description = self.title_description();
        self.ui.tree_mut().set_label(icon_container, self.get_aria_label(&calculated_title, description.as_deref()));
    }

    fn get_aria_label(&self, title: &str, description: Option<&str>) -> String {
        let view_has_accessibility_help_content = self.services.view_descriptor_service.get_view_descriptor_by_id(&self.id).is_some_and(|descriptor| descriptor.accessibility_help_content.is_some());
        let accessible_view_has_shown_for_view = self.services.accessible_view_information_service.as_ref().is_some_and(|service| service.has_shown_accessible_view(&self.id));
        if !view_has_accessibility_help_content || accessible_view_has_shown_for_view {
            return match description {
                Some(description) if !description.is_empty() => format!("{title} - {description}"),
                _ => title.to_owned(),
            };
        }

        localize("viewAccessibilityHelp", "Use Alt+F1 for accessibility help {0}", &[title])
    }

    /// `updateTitle(title)` (`protected`).
    pub fn update_title(&self, title: &str) {
        let calculated_title = self.calculate_title(title);
        // `this._title = title` vem depois do `textContent` no original, mas só os textos lêem o campo.
        *self.title.borrow_mut() = title.to_owned();
        let (title_container, hover) = {
            let header = self.header.borrow();
            (header.title_container, header.title_container_hover.clone())
        };
        if title_container.is_some() {
            self.set_title_texts();
            if let Some(hover) = hover {
                hover.update(ManagedHoverContentOrFactory::Content(ManagedHoverContent::Text(calculated_title.clone())), None);
            }
        }

        let description = self.title_description();
        self.update_aria_header_label(&calculated_title, description.as_deref());

        self.on_did_change_title_area.fire(());
    }

    fn update_aria_header_label(&self, title: &str, description: Option<&str>) {
        let aria_label = self.get_aria_label(title, description);
        let (icon_container, hover) = {
            let header = self.header.borrow();
            (header.icon_container, header.icon_container_hover.clone())
        };
        if let Some(icon_container) = icon_container {
            if let Some(hover) = hover {
                hover.update(ManagedHoverContentOrFactory::Content(ManagedHoverContent::Text(title.to_owned())), None);
            }
            self.ui.tree_mut().set_label(icon_container, aria_label.clone());
        }
        self.pane.set_aria_header_label(&localize("viewSection", "{0} Section", &[&aria_label]));
    }

    fn set_title_description(&self, description: Option<&str>) {
        let existing = self.header.borrow().title_description_container;
        if let Some(container) = existing {
            self.set_title_texts_for(container, description.unwrap_or(""));
            let hover = self.header.borrow().title_description_container_hover.clone();
            if let Some(hover) = hover {
                hover.update(ManagedHoverContentOrFactory::Content(ManagedHoverContent::Text(description.unwrap_or("").to_owned())), None);
            }
        } else if let (Some(description), true) = (description.filter(|description| !description.is_empty()), self.header.borrow().title_container.is_some()) {
            let header_container = self.header.borrow().header_container.expect("`renderHeader` já criou o cabeçalho");
            let title_container = self.header.borrow().title_container.expect("o título já foi criado");
            let container = {
                let mut tree = self.ui.tree_mut();
                // `after(this.titleContainer, ...)`
                let node = tree.create_detached(Role::Generic);
                tree.set_class_name(node, "description");
                let next = tree.children(header_container).iter().position(|child| *child == title_container).and_then(|index| tree.children(header_container).get(index + 1).copied());
                tree.insert_before(header_container, node, next);
                node
            };
            self.header.borrow_mut().title_description_container = Some(container);
            self.set_title_texts_for(container, description);
            let hover = self.services.hover_service.setup_managed_hover(
                get_default_hover_delegate(HoverPlacement::Mouse),
                container,
                ManagedHoverContentOrFactory::Content(ManagedHoverContent::Text(description.to_owned())),
                None,
            );
            self.base.register(hover.clone());
            self.header.borrow_mut().title_description_container_hover = Some(hover);
        }
        self.layout_header(self.header_width.get());
    }

    /// `updateTitleDescription(description?)` (`protected`).
    pub fn update_title_description(&self, description: Option<&str>) {
        self.set_title_description(description);
        let title = self.title();
        self.update_aria_header_label(&title, description);
        *self.title_description.borrow_mut() = description.map(str::to_owned);
        self.on_did_change_title_area.fire(());
    }

    fn calculate_title(&self, title: &str) -> String {
        let service = &self.services.view_descriptor_service;
        let view_container = service.get_view_container_by_view_id(&self.id).expect("a view tem um contêiner");
        let model = service.get_view_container_model(&view_container);
        let view_descriptor = service.get_view_descriptor_by_id(&self.id);
        let is_default = service.get_default_container_by_id(&self.id).is_some_and(|default| Rc::ptr_eq(&default, &view_container));

        if !is_default
            && let Some(container_title) = view_descriptor.as_ref().and_then(|descriptor| descriptor.container_title.as_ref())
            && model.title() != *container_title
            && title != container_title
        {
            return format!("{container_title}: {title}");
        }

        title.to_owned()
    }

    /// Escreve o texto do título (em caixa alta, como o `text-transform: uppercase`) e o da descrição nos nós.
    fn set_title_texts(&self) {
        let Some(title_container) = self.header.borrow().title_container else { return };
        let calculated = self.calculate_title(&self.title());
        self.set_title_texts_for(title_container, &calculated);
        if let (Some(container), Some(description)) = (self.header.borrow().title_description_container, self.title_description()) {
            self.set_title_texts_for(container, &description);
        }
        self.layout_header(self.header_width.get());
    }

    fn set_title_texts_for(&self, node: NodeId, text: &str) {
        let is_title = self.header.borrow().title_container == Some(node);
        let styles = &self.services.header_styles;
        let mut tree = self.ui.tree_mut();
        if is_title {
            let color = self.pane.styles().header_foreground.unwrap_or(styles.foreground);
            tree.style_mut(node).content = Content::Text { text: text.to_uppercase(), style: ui_font(styles.font_size, styles.font_weight), color, line_height: HEADER_HEIGHT };
        } else {
            // `.description`: `font-weight: normal`, sem caixa alta, na cor `panelTitle.inactiveForeground`.
            tree.style_mut(node).content = Content::Text { text: text.to_owned(), style: ui_font(styles.font_size, 400), color: self.services.description_foreground, line_height: HEADER_HEIGHT };
        }
    }

    /// O flex do `.pane-header`: o twisty, o título, a descrição e as ações dentro de `width`.
    pub fn layout_header(&self, width: f64) {
        self.header_width.set(width);
        let header = self.header.borrow();
        let Some(_) = header.header_container else { return };
        let expanded = self.pane.is_expanded();
        let collapsible = self.pane.collapsible();
        let mut tree = self.ui.tree_mut();
        let mut x = 0.0;

        if let Some(twisty) = header.twisties_container {
            tree.style_mut(twisty).visible = collapsible;
            if collapsible {
                // `margin: 0 2px`, e `transform: translateY(1px)` quando expandido.
                tree.set_bounds(twisty, je_render::kurbo::Rect::new(x + 2.0, if expanded { 3.0 } else { 2.0 }, x + 18.0, if expanded { 19.0 } else { 18.0 }));
                x += 20.0;
            }
        }

        if let Some(icon) = header.icon_container {
            tree.set_bounds(icon, je_render::kurbo::Rect::new(0.0, 4.0, 16.0, 20.0));
        }

        let actions_left = if let Some(title) = header.title_container {
            // `.not-collapsible .title { margin-left: 8px }`
            if !collapsible {
                x += 8.0;
            }
            let text_width = match &tree.style(title).content {
                Content::Text { text, style, .. } => measure_text(text, style),
                _ => 0.0,
            };
            let available = (width - x - 8.0).max(0.0);
            let title_width = text_width.min(available);
            tree.set_bounds(title, je_render::kurbo::Rect::new(x, 0.0, x + title_width, HEADER_HEIGHT));
            x += title_width;

            if let Some(description) = header.title_description_container {
                // `.description { margin-left: 10px }`, escondida quando o cabeçalho não está expandido.
                tree.style_mut(description).visible = expanded;
                if expanded {
                    x += 10.0;
                    let text_width = match &tree.style(description).content {
                        Content::Text { text, style, .. } => measure_text(text, style),
                        _ => 0.0,
                    };
                    let available = (width - x - 8.0).max(0.0);
                    tree.set_bounds(description, je_render::kurbo::Rect::new(x, 0.0, x + text_width.min(available), HEADER_HEIGHT));
                    x += text_width.min(available);
                }
            }
            x
        } else {
            x
        };

        if let Some(actions) = header.actions_container {
            // `margin-left: auto; margin-right: 8px`
            tree.set_bounds(actions, je_render::kurbo::Rect::new(actions_left, 0.0, (width - 8.0).max(actions_left), HEADER_HEIGHT));
        }
    }

    // Corpo

    fn render_body(self: &Rc<Self>, container: NodeId) {
        let delegate: Rc<dyn IViewWelcomeDelegate> = Rc::new(PaneWelcomeDelegate { id: self.id.clone(), on_did_change_view_welcome_state: self.on_did_change_view_welcome_state(), pane: self.weak_self.clone() });
        let controller = ViewWelcomeController::new(
            &self.ui,
            container,
            delegate,
            self.scoped_context_key_service.clone(),
            self.services.opener_service.clone(),
            self.services.lifecycle_service.clone(),
            self.services.create_welcome_view.clone(),
        );
        self.base.register(controller.clone());
        *self.view_welcome_controller.borrow_mut() = Some(controller);

        if let Some(delegate) = self.delegate() {
            delegate.render_body(container);
        }
    }

    fn layout_body(&self, height: f64, width: f64) {
        let controller = self.view_welcome_controller.borrow().clone();
        if let Some(controller) = controller {
            controller.layout(height, width);
        }

        self.layout_header(width);
        self.update_progress_bar_position();

        if let Some(delegate) = self.delegate() {
            delegate.layout_body(height, width);
        }
    }

    /// `onDidScrollRoot()`.
    pub fn on_did_scroll_root(&self) {
        if let Some(delegate) = self.delegate() {
            delegate.on_did_scroll_root();
        }
    }

    // Progresso

    /// `.monaco-progress-container { position: absolute; left: 0; top: 20px }` (`top: -2px` com o cabeçalho mesclado).
    fn update_progress_bar_position(&self) {
        let Some(bar) = self.progress.borrow().bar.clone() else { return };
        let container = bar.get_container();
        let top = if self.pane.header_visible() { 20.0 } else { -2.0 };
        let mut tree = self.ui.tree_mut();
        let bounds = tree.bounds(container);
        tree.set_bounds(container, je_render::kurbo::Rect::new(bounds.x0, top, bounds.x1, top + bounds.height()));
        tree.style_mut(container).z_index = 5;
    }

    /// `getProgressIndicator()`.
    pub fn get_progress_indicator(&self) -> Rc<ScopedProgressIndicator> {
        let existing_bar = self.progress.borrow().bar.clone();
        let bar = match existing_bar {
            Some(bar) => bar,
            None => {
                let bar = ProgressBar::new(&self.ui, self.pane.element(), Some(&self.services.progress_bar_options));
                self.base.register(bar.clone());
                bar.hide();
                self.progress.borrow_mut().bar = Some(bar.clone());
                self.update_progress_bar_position();
                bar
            }
        };

        let existing_indicator = self.progress.borrow().indicator.clone();
        match existing_indicator {
            Some(indicator) => indicator,
            None => {
                let scope = AbstractProgressScope::new(&self.id, self.is_body_visible());
                self.base.register(scope.clone());
                let (listener_scope, id) = (scope.clone(), self.id.clone());
                self.base.register(self.on_did_change_body_visibility().listen(move |is_visible| {
                    if *is_visible {
                        listener_scope.on_scope_opened(&id);
                    } else {
                        listener_scope.on_scope_closed(&id);
                    }
                }));
                let scope: Rc<dyn IProgressScope> = scope;
                let indicator = ScopedProgressIndicator::new(bar, scope);
                self.base.register(indicator.clone());
                self.progress.borrow_mut().indicator = Some(indicator.clone());
                indicator
            }
        }
    }

    /// `getProgressLocation()` (`protected`).
    pub fn get_progress_location(&self) -> String {
        self.services.view_descriptor_service.get_view_container_by_view_id(&self.id).expect("a view tem um contêiner").id.clone()
    }

    /// `getLocationBasedColors()` (`protected`).
    pub fn get_location_based_colors(&self) -> IViewPaneLocationColors {
        get_location_based_view_colors(self.services.view_descriptor_service.get_view_location_by_id(&self.id))
    }

    // Foco

    /// `focus()`.
    pub fn focus(&self) {
        let controller = self.view_welcome_controller.borrow().clone();
        match controller {
            Some(controller) if controller.enabled() => controller.focus(),
            _ => self.ui.focus(self.pane.element()),
        }
        if is_active_element(&self.ui, self.pane.element()) || is_ancestor_of_active_element(&self.ui, self.pane.element()) {
            self.on_did_focus.fire(());
        }
    }

    // Ações

    fn set_actions(&self) {
        let Some(toolbar) = self.toolbar.borrow().clone() else { return };
        let primary_actions = self.menu_actions.get_primary_actions();
        toolbar.set_actions(&prepare_actions(primary_actions), &prepare_actions(self.menu_actions.get_secondary_actions()));
        toolbar.set_context(self.get_actions_context());
    }

    fn update_actions_visibility(&self) {
        let Some(header_container) = self.header.borrow().header_container else { return };
        let should_always_show_actions = self.services.configuration_service.get_value(Some(ALWAYS_SHOW_ACTIONS_CONFIG), None).is_truthy();
        self.ui.tree_mut().toggle_class(header_container, "actions-always-visible", should_always_show_actions);
        self.update_actions_display();
    }

    /// As regras de `display` do `.actions` (`paneview.css` e `paneviewlet.css`): visível com `show-always`, com `show-expanded`
    /// e o pane expandido, ou com o cabeçalho expandido e o pane com o ponteiro, com o foco dentro, com
    /// `actions-always-visible` ou com o cabeçalho focado.
    fn update_actions_display(&self) {
        let (Some(header_container), Some(actions)) = (self.header.borrow().header_container, self.header.borrow().actions_container) else { return };
        let expanded = self.pane.is_expanded();
        let mut tree = self.ui.tree_mut();
        let always_visible = tree.has_class(header_container, "actions-always-visible");
        let visible = self.show_actions == ViewPaneShowActions::Always
            || (self.show_actions == ViewPaneShowActions::WhenExpanded && expanded)
            || (expanded && (self.hovered.get() || self.focus_within.get() || always_visible || self.header_focused.get()));
        tree.style_mut(actions).visible = visible;
    }

    /// `updateActions()` (`protected`).
    pub fn update_actions(&self) {
        self.set_actions();
        self.on_did_change_title_area.fire(());
    }

    /// `createActionViewItem(action, options?)`.
    pub fn create_action_view_item(&self, action: &ActionRef, options: &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>> {
        let menu_as_child = action.as_any().is::<je_base::actions::SubmenuAction>();
        let item = (self.services.create_action_view_item)(action, options, menu_as_child)?;
        self.header_action_view_items.borrow_mut().insert(item.action().id(), item.clone());
        Some(item)
    }

    /// `getActionsContext()`.
    pub fn get_actions_context(&self) -> Option<Dynamic> {
        self.delegate().and_then(|delegate| delegate.get_actions_context())
    }

    /// `getActionRunner()`.
    pub fn get_action_runner(&self) -> Option<Rc<dyn IActionRunner>> {
        self.delegate().and_then(|delegate| delegate.get_action_runner())
    }

    /// `getOptimalWidth()`.
    pub fn get_optimal_width(&self) -> f64 {
        self.delegate().map_or(0.0, |delegate| delegate.get_optimal_width())
    }

    /// `saveState()`.
    pub fn save_state(&self) {
        if let Some(delegate) = self.delegate() {
            delegate.save_state();
        }
    }

    /// `shouldShowWelcome()`.
    pub fn should_show_welcome(&self) -> bool {
        self.delegate().is_some_and(|delegate| delegate.should_show_welcome())
    }
}

impl IView for ViewPane {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn focus(&self) {
        ViewPane::focus(self);
    }

    fn is_visible(&self) -> bool {
        ViewPane::is_visible(self)
    }

    fn is_body_visible(&self) -> bool {
        ViewPane::is_body_visible(self)
    }

    fn set_expanded(&self, expanded: bool) -> bool {
        ViewPane::set_expanded(self, expanded)
    }

    fn get_progress_indicator(&self) -> Option<Rc<dyn je_editor::code_action::platform::IProgressIndicator>> {
        Some(ViewPane::get_progress_indicator(self))
    }
}

impl IViewPane for ViewPane {
    fn set_expanded(&self, expanded: bool) -> bool {
        ViewPane::set_expanded(self, expanded)
    }

    fn focus(&self) {
        ViewPane::focus(self);
    }
}

impl IDisposable for ViewPane {
    fn dispose(&self) {
        self.base.dispose();
        self.pane.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ViewPane"
    }
}

/// `IViewPaneLocationColors`: os ids de cor do tema (`string`), e os estilos de lista que dependem do lugar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IViewPaneLocationColors {
    pub background: &'static str,
    pub overlay_background: &'static str,
    pub list_override_styles: ListOverrideStyleColors,
}

/// `PartialExcept<IListStyles, 'listBackground' | 'treeStickyScrollBackground'>` com as chaves que o original preenche.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListOverrideStyleColors {
    pub list_background: &'static str,
    pub tree_sticky_scroll_background: &'static str,
    pub tree_sticky_scroll_border: &'static str,
    pub tree_sticky_scroll_shadow: &'static str,
}

/// `getLocationBasedViewColors(location)`.
pub fn get_location_based_view_colors(location: Option<ViewContainerLocation>) -> IViewPaneLocationColors {
    let (background, overlay_background, sticky_scroll_background, sticky_scroll_border, sticky_scroll_shadow) = match location {
        // PANEL_BACKGROUND, PANEL_SECTION_DRAG_AND_DROP_BACKGROUND, PANEL_STICKY_SCROLL_*
        Some(ViewContainerLocation::Panel) => ("panel.background", "panelSection.dropBackground", "panelStickyScroll.background", "panelStickyScroll.border", "panelStickyScroll.shadow"),
        // SIDE_BAR_BACKGROUND, SIDE_BAR_DRAG_AND_DROP_BACKGROUND, SIDE_BAR_STICKY_SCROLL_*
        Some(ViewContainerLocation::Sidebar | ViewContainerLocation::AuxiliaryBar) | None => {
            ("sideBar.background", "sideBar.dropBackground", "sideBarStickyScroll.background", "sideBarStickyScroll.border", "sideBarStickyScroll.shadow")
        }
    };

    IViewPaneLocationColors {
        background,
        overlay_background,
        list_override_styles: ListOverrideStyleColors {
            list_background: background,
            tree_sticky_scroll_background: sticky_scroll_background,
            tree_sticky_scroll_border: sticky_scroll_border,
            tree_sticky_scroll_shadow: sticky_scroll_shadow,
        },
    }
}
