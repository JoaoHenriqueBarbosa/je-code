//! Porte de `src/vs/workbench/browser/parts/activitybar/activitybarPart.ts`: a `ActivitybarPart` (dimensões, estilos,
//! gutters do Modern UI, criação da barra de painéis), a `ActivityBarCompositeBar` (a reserva de espaço do menu compacto e dos
//! ícones globais no layout) e as ações de posição e de foco da barra de atividades.
//!
//! Divergências:
//! - **DOM vira estado nativo.** O elemento da parte é o [`PartElement`] (classes, propriedades `--activity-bar-*`, cor de fundo
//!   e de borda inline) e o `.content` é o [`ContentElement`] (o `size(element, w, h)` do `PartLayout`). As cores inline guardam o
//!   texto que o CSSOM devolveria (`rgb(...)` ou `rgba(...)`). `clearNode(this.content)` não tem corpo próprio: quem desenha a
//!   barra (o [`IPaneCompositeBar`]) tira o que criou ao ser descartado, que é o que `compositeBar.clear()` faz.
//! - **`Part` (`part.ts`) e `Component`/`Themable`.** A base não foi portada como classe: o registro no layout
//!   (`layoutService.registerPart`), o `onDidChange`, o `dimension`, o `layoutContents` sem título, cabeçalho nem rodapé (a parte
//!   tem `hasTitle: false`) e a reação a `onDidColorThemeChange` (só depois de `create`) vivem na própria [`ActivitybarPart`].
//!   O `IStorageService` que o construtor repassa à base não é lido por ela e por isso não entra.
//! - **`instantiationService.createInstance(ActivityBarCompositeBar, ...)`** é a [`CompositeBarFactory`] que recebe o
//!   [`CompositeBarRequest`] (localização, opções, parte, parte de painéis e `globalActivities`).
//! - **`mainContainer.classList.toggle('activitybar-compact', ...)`.** O contêiner principal é um identificador opaco do layout;
//!   a classe é publicada em [`ActivitybarPart::root_has_compact_class`], que o estilo do Modern UI lê.
//! - **`mainWindow.document.hasFocus()`** (janela ativa ainda desconhecida) é o `hasFocus` do `IHostService`, e
//!   `mainWindow.vscodeWindowId` é [`MAIN_WINDOW`].
//! - **`toJSON`** devolve o [`ActivitybarPartJson`].
//! - **Menu compacto, teclado e menu de contexto.** O `CustomMenubarControl` é o trait [`ICustomMenubarControl`] (o corpo da
//!   classe, que liga o `MenuBar` aos menus do `MenuId.MenubarMain`, entra com o porte da barra de título) e o
//!   `createInstance(CustomMenubarControl)` é a [`MenubarFactory`]. `getMenuBarVisibility` é o `get_menu_bar_visibility` do
//!   `TitleBarHost`. As dependências que a `ActivityBarCompositeBar` injeta (`IConfigurationService`, `IMenuService`,
//!   `IContextKeyService`, `IWorkbenchLayoutService`, `IInstantiationService`) são o [`ActivityBarServices`], e o
//!   `super(location, {...options, fillExtraContextMenuActions})` é o [`ActivityBarCompositeBar::with_services`], que monta a
//!   base já com o gancho embrulhado. Os `addDisposableListener(..., KEY_DOWN)` de `registerKeyboardNavigationListeners` são
//!   os métodos `on_*_key_down`, que a árvore de widgets chama com o [`StandardKeyboardEvent`] do elemento. O
//!   `isAncestor(e.target, menuBarContainer)` chega pelo [`ContextMenuTarget`].
//! - **Regras de tema em CSS** (`registerThemingParticipant`). A UI nativa não tem folha de estilo: o que cada regra
//!   escolheria é o [`ActivityBarItemThemeRules`], calculado por [`activity_bar_item_theme_rules`] e lido pelo desenho dos
//!   itens, com as mesmas precedências (cor de contraste, depois a borda de foco).
//! - **Ainda não portado**: a base `PaneCompositeBar`/`GlobalCompositeBar` em si (só o contrato, ver `pane_composite_bar`) e o
//!   corpo do `CustomMenubarControl`.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, Separator, SubmenuAction, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::key_codes::KeyCode;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, MutableDisposable, combined_disposable, to_disposable};
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::theme_service::{IColorTheme, IThemeService};
use je_instantiation::{InstantiationService, ServicesAccessor};
use je_platform::action::action_common_categories::Categories;
use je_platform::action::{Category, CommandAction, CommandActionTitle, LocalizedString, Title, Toggled, localize2};
use je_platform::actions::actions::{Action2, Action2MenuItem, Action2Options, AnyMenuItem, IMenuService, MenuActionOptions, SubmenuItem, menu_registry, register_action2};
use je_platform::actions::menu_entry_action_view_item::get_context_menu_actions;
use je_platform::actions::menu_ids;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::{IConfigurationService, I_CONFIGURATION_SERVICE};
use je_platform::contextkey::contextkey::{ContextKeyExpr, ContextKeyExpression, IContextKeyService};
use je_theme::color::Color;
use je_theme::color::format::css::format_rgb;
use je_widgets::action_bar::ActionsOrientation;
use je_widgets::dom::StandardKeyboardEvent;
use je_widgets::gridview::ViewDimensions;
use je_widgets::hover_api::HoverPosition;

use crate::browser::actions::layout_actions::{ToggleSidebarPositionAction, ToggleSidebarVisibilityAction};
use crate::browser::parts::composite_bar_actions::SwitchCompositeViewAction;
use crate::browser::parts::pane_composite_bar::{
    ActivityHoverOptions, CompositeBarColors, ContentElement, ContextMenuTarget, GlobalActivities, IGlobalCompositeBar, IPaneCompositeBar, IPaneCompositePart,
    PaneCompositeBarOptions,
};
use crate::browser::parts::titlebar::menubar_control::{ICustomMenubarControl, MENU_BAR_VISIBILITY_SETTING, MenuBarVisibility, MenubarFactory};
use crate::common::contextkeys::is_sessions_window_context;
use crate::common::views::{ViewContainerLocation, view_container_location_to_string};
use crate::services::host::host::IHostService;
use crate::services::layout::layout_service::{
    ActivityBarPosition, FLOATING_PANEL_INNER_MARGIN, IWorkbenchLayoutService, I_WORKBENCH_LAYOUT_SERVICE, LayoutPart, LayoutSettings, MAIN_WINDOW, Parts,
    Position, TitleBarHost, get_floating_panel_margin, get_floating_panel_outer_margin, is_floating_top_edge_exposed,
};

// As cores de `workbench/common/theme.ts` e de `colorRegistry.ts` que a parte lê.
pub const ACTIVITY_BAR_BACKGROUND: &str = "activityBar.background";
pub const ACTIVITY_BAR_FOREGROUND: &str = "activityBar.foreground";
pub const ACTIVITY_BAR_INACTIVE_FOREGROUND: &str = "activityBar.inactiveForeground";
pub const ACTIVITY_BAR_BORDER: &str = "activityBar.border";
pub const ACTIVITY_BAR_ACTIVE_BORDER: &str = "activityBar.activeBorder";
pub const ACTIVITY_BAR_ACTIVE_BACKGROUND: &str = "activityBar.activeBackground";
pub const ACTIVITY_BAR_DRAG_AND_DROP_BORDER: &str = "activityBar.dropBorder";
pub const ACTIVITY_BAR_ACTIVE_FOCUS_BORDER: &str = "activityBar.activeFocusBorder";
pub const ACTIVITY_BAR_BADGE_BACKGROUND: &str = "activityBarBadge.background";
pub const ACTIVITY_BAR_BADGE_FOREGROUND: &str = "activityBarBadge.foreground";
pub const MODERN_ACTIVITY_BAR_BACKGROUND: &str = "modernActivityBar.background";
pub const MODERN_ACTIVITY_BAR_INACTIVE_BACKGROUND: &str = "modernActivityBar.inactiveBackground";
pub const CONTRAST_BORDER: &str = "contrastBorder";
pub const ACTIVE_CONTRAST_BORDER: &str = "contrastActiveBorder";
pub const FOCUS_BORDER: &str = "focusBorder";

/// O que as regras de `registerThemingParticipant` do `activitybarPart.ts` escolheriam para os itens da barra de atividades
/// (`.monaco-workbench .activitybar > .content :not(.monaco-menu) > .monaco-action-bar .action-item`), já resolvido contra o
/// tema. Cada campo é a declaração de uma regra, ausente quando a regra não seria emitida.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActivityBarItemThemeRules {
    /// `.checked .active-item-indicator:before { border-left-color }` (`activityBar.activeBorder`).
    pub checked_indicator_color: Option<Color>,
    /// `.checked:focus .active-item-indicator:before { visibility: visible; border-left-color }`
    /// (`activityBar.activeFocusBorder`); quando presente, o `.checked:focus::before` fica escondido.
    pub checked_focus_indicator_color: Option<Color>,
    /// `.checked .active-item-indicator { z-index: 0; background-color }` (`activityBar.activeBackground`).
    pub checked_indicator_background: Option<Color>,
    /// `.action-label::before { padding: 6px }`, só com a cor de contraste (alto contraste).
    pub label_outline_padding: Option<f64>,
    /// `.active`, `.active:hover`, `.checked` e `.checked:hover` em `.action-label::before { outline: 1px solid }`.
    pub active_outline: Option<Color>,
    /// `:hover .action-label::before { outline: 1px dashed }`.
    pub hover_outline: Option<Color>,
    /// `:focus .active-item-indicator:before { border-left-color }`: a cor de contraste ou, sem ela, a `focusBorder`.
    pub focus_indicator_color: Option<Color>,
}

/// O corpo do `registerThemingParticipant` do `activitybarPart.ts`.
pub fn activity_bar_item_theme_rules(theme: &dyn IColorTheme) -> ActivityBarItemThemeRules {
    let mut rules = ActivityBarItemThemeRules {
        checked_indicator_color: theme.get_color(ACTIVITY_BAR_ACTIVE_BORDER, None),
        checked_focus_indicator_color: theme.get_color(ACTIVITY_BAR_ACTIVE_FOCUS_BORDER, None),
        checked_indicator_background: theme.get_color(ACTIVITY_BAR_ACTIVE_BACKGROUND, None),
        ..ActivityBarItemThemeRules::default()
    };

    // Styling with Outline color (e.g. high contrast theme)
    if let Some(outline) = theme.get_color(ACTIVE_CONTRAST_BORDER, None) {
        rules.label_outline_padding = Some(6.0);
        rules.active_outline = Some(outline.clone());
        rules.hover_outline = Some(outline.clone());
        rules.focus_indicator_color = Some(outline);
    }
    // Styling without outline color
    else {
        rules.focus_indicator_color = theme.get_color(FOCUS_BORDER, None);
    }
    rules
}

/// O elemento da parte (o `parent` que `create` recebe): classes, propriedades CSS personalizadas e as cores inline.
#[derive(Default)]
pub struct PartElement {
    classes: RefCell<BTreeSet<String>>,
    properties: RefCell<BTreeMap<String, String>>,
    background_color: RefCell<String>,
    border_color: RefCell<String>,
}

impl PartElement {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// `classList.toggle(name, force)`.
    pub fn toggle_class(&self, name: &str, force: bool) {
        if force {
            self.classes.borrow_mut().insert(name.to_owned());
        } else {
            self.classes.borrow_mut().remove(name);
        }
    }

    /// `classList.contains(name)`.
    pub fn has_class(&self, name: &str) -> bool {
        self.classes.borrow().contains(name)
    }

    /// `style.setProperty(name, value)`.
    pub fn set_property(&self, name: &str, value: String) {
        self.properties.borrow_mut().insert(name.to_owned(), value);
    }

    /// `style.getPropertyValue(name)`: o texto vazio para a propriedade que nunca foi escrita.
    pub fn get_property_value(&self, name: &str) -> String {
        self.properties.borrow().get(name).cloned().unwrap_or_default()
    }

    /// `style.backgroundColor`.
    pub fn background_color(&self) -> String {
        self.background_color.borrow().clone()
    }

    /// `style.borderColor`.
    pub fn border_color(&self) -> String {
        self.border_color.borrow().clone()
    }
}

/// O que `toJSON` devolve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActivitybarPartJson {
    pub part_type: Parts,
}

/// Os argumentos do `createInstance(ActivityBarCompositeBar, location, options, part, paneCompositePart, globalActivities)`.
pub struct CompositeBarRequest {
    pub location: ViewContainerLocation,
    pub options: PaneCompositeBarOptions,
    pub part: Parts,
    pub pane_composite_part: Rc<dyn IPaneCompositePart>,
    pub global_activities: Option<GlobalActivities>,
}

/// A fábrica que `instantiationService.createInstance(ActivityBarCompositeBar, ...)` representa.
pub type CompositeBarFactory = Rc<dyn Fn(CompositeBarRequest) -> Rc<dyn IPaneCompositeBar>>;

struct PartState {
    element: Option<Rc<PartElement>>,
    content: Option<Rc<ContentElement>>,
    dimension: Option<Dimension>,
    is_compact: bool,
    active_window_id: Option<u32>,
    theme: Rc<dyn IColorTheme>,
}

/// `ActivitybarPart`.
pub struct ActivitybarPart {
    location: ViewContainerLocation,
    pane_composite_part: Rc<dyn IPaneCompositePart>,
    composite_bar_factory: CompositeBarFactory,
    layout_service: Rc<dyn IWorkbenchLayoutService>,
    configuration_service: Rc<dyn IConfigurationService>,
    host_service: Rc<dyn IHostService>,
    composite_bar: Rc<MutableDisposable<dyn IPaneCompositeBar>>,
    on_did_change: Rc<Emitter<Option<ViewDimensions>>>,
    root_compact_class: Cell<bool>,
    state: RefCell<PartState>,
    disposables: Rc<Disposable>,
}

impl ActivitybarPart {
    pub const ACTION_HEIGHT: f64 = 48.0;
    pub const COMPACT_ACTION_HEIGHT: f64 = 28.0;

    pub const ACTIVITYBAR_WIDTH: f64 = 48.0;
    pub const COMPACT_ACTIVITYBAR_WIDTH: f64 = 36.0;

    /// Narrower dimensions used when the floating panels (Modern UI) experiment is enabled.
    pub const FLOATING_ACTION_HEIGHT: f64 = 36.0;
    pub const FLOATING_ACTIVITYBAR_WIDTH: f64 = 36.0;
    pub const FLOATING_COMPACT_ACTIVITYBAR_WIDTH: f64 = 28.0;

    /// Vertical gap between activity bar items at the default size under the floating panels experiment. Published to CSS
    /// as `--activity-bar-action-gap` so that the stylesheet and the overflow computation cannot drift apart.
    pub const FLOATING_ACTION_GAP: f64 = 8.0;

    /// The compact Modern UI density tightens that gap to match its denser rhythm.
    pub const FLOATING_COMPACT_ACTION_GAP: f64 = 4.0;

    pub const ICON_SIZE: f64 = 24.0;
    pub const COMPACT_ICON_SIZE: f64 = 16.0;

    /// Base gutter reserved around the activity bar under the floating panels experiment. Must match the margins applied in
    /// `floatingPanels.css`.
    pub const FLOATING_MARGIN: f64 = crate::services::layout::layout_service::FLOATING_PANEL_MARGIN;

    /// The rail's own horizontal padding, doubled: the space inside the card beside the icon column.
    pub const FLOATING_LANE: f64 = 8.0;
    pub const FLOATING_COMPACT_LANE: f64 = 4.0;

    /// The card border the activity bar draws on each edge under the floating panels experiment.
    pub const FLOATING_BORDER: f64 = 1.0;

    pub const PINNED_VIEW_CONTAINERS_KEY: &'static str = "workbench.activity.pinnedViewlets2";
    pub const PLACEHOLDER_VIEW_CONTAINERS_KEY: &'static str = "workbench.activity.placeholderViewlets";
    pub const VIEW_CONTAINERS_WORKSPACE_STATE_KEY: &'static str = "workbench.activity.viewletsWorkspaceState";

    /// `hasTitle: false`: a parte não tem título, cabeçalho nem rodapé.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        location: ViewContainerLocation,
        pane_composite_part: Rc<dyn IPaneCompositePart>,
        composite_bar_factory: CompositeBarFactory,
        layout_service: Rc<dyn IWorkbenchLayoutService>,
        theme_service: Rc<dyn IThemeService>,
        configuration_service: Rc<dyn IConfigurationService>,
        host_service: Rc<dyn IHostService>,
    ) -> Rc<Self> {
        let is_compact = Self::read_compact(configuration_service.as_ref());
        let this = Rc::new(Self {
            location,
            pane_composite_part,
            composite_bar_factory,
            layout_service: layout_service.clone(),
            configuration_service: configuration_service.clone(),
            host_service: host_service.clone(),
            composite_bar: MutableDisposable::new(),
            on_did_change: Emitter::new(),
            root_compact_class: Cell::new(false),
            state: RefCell::new(PartState { element: None, content: None, dimension: None, is_compact, active_window_id: None, theme: theme_service.get_color_theme() }),
            disposables: Disposable::new(),
        });
        let register = |handle: DisposableRef| {
            this.disposables.register(handle);
        };
        register(this.composite_bar.clone());
        register(this.on_did_change.clone());

        // Part: `this._register(layoutService.registerPart(this))`.
        register(layout_service.register_part(this.clone()));

        // Themable: `this._register(themeService.onDidColorThemeChange(theme => this.onThemeChange(theme)))`.
        let weak = Rc::downgrade(&this);
        register(theme_service.on_did_color_theme_change().listen(move |theme| {
            if let Some(this) = weak.upgrade() {
                this.on_theme_change(theme.clone());
            }
        }));

        let weak = Rc::downgrade(&this);
        register(configuration_service.on_did_change_configuration().listen(move |event| {
            if let Some(this) = weak.upgrade() {
                this.on_configuration_changed(event);
            }
        }));

        let weak = Rc::downgrade(&this);
        register(host_service.on_did_change_focus().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_styles_if_created();
            }
        }));
        let weak = Rc::downgrade(&this);
        register(host_service.on_did_change_active_window().listen(move |window_id| {
            if let Some(this) = weak.upgrade() {
                this.state.borrow_mut().active_window_id = Some(*window_id);
                this.update_styles_if_created();
            }
        }));

        // Showing or hiding the primary side bar decides whether the rail connects to it or stands alone, which can change the
        // gutter it reserves (see `needs_floating_leading_gap`).
        let weak = Rc::downgrade(&this);
        register(layout_service.on_did_change_part_visibility().listen(move |event| {
            if let Some(this) = weak.upgrade()
                && event.part_id == Parts::SidebarPart.as_str()
                && this.layout_service.is_floating_panels_enabled()
            {
                this.on_did_change.fire(None);
            }
        }));
        this
    }

    fn read_compact(configuration_service: &dyn IConfigurationService) -> bool {
        matches!(configuration_service.get_value(Some(LayoutSettings::ActivityBarCompact.as_str()), None), Dynamic::Bool(true))
    }

    fn on_configuration_changed(&self, event: &Rc<ConfigurationChangeEvent>) {
        if event.affects_configuration(LayoutSettings::ActivityBarCompact.as_str(), None) {
            self.state.borrow_mut().is_compact = Self::read_compact(self.configuration_service.as_ref());
            self.update_compact_style();
            self.recreate_composite_bar();
            self.on_did_change.fire(None); // Signal grid that size constraints changed
        }

        // Floating panels changes the reserved left/bottom gutter (and therefore the fixed part width): signal the grid that the
        // size constraint changed.
        if event.affects_configuration(LayoutSettings::ModernUi.as_str(), None) || event.affects_configuration(LayoutSettings::ModernUiDensity.as_str(), None) {
            self.update_compact_style();
            self.recreate_composite_bar();
            self.on_did_change.fire(None);
            self.update_styles_if_created();
        }
    }

    /// `Part.onThemeChange`: só depois de `create`.
    fn on_theme_change(&self, theme: Rc<dyn IColorTheme>) {
        self.state.borrow_mut().theme = theme;
        self.update_styles_if_created();
    }

    fn update_styles_if_created(&self) {
        if self.state.borrow().element.is_some() {
            self.update_styles();
        }
    }

    // region IView

    pub fn minimum_width(&self) -> f64 {
        self.base_width() + self.floating_horizontal_gutter()
    }

    pub fn maximum_width(&self) -> f64 {
        self.base_width() + self.floating_horizontal_gutter()
    }

    pub fn minimum_height(&self) -> f64 {
        0.0
    }

    pub fn maximum_height(&self) -> f64 {
        f64::INFINITY
    }

    /// `onDidChange`: dispara `None` (o `undefined` do original) quando as restrições de tamanho mudam.
    pub fn on_did_change(&self) -> Event<Option<ViewDimensions>> {
        self.on_did_change.event()
    }

    // endregion

    fn is_compact(&self) -> bool {
        self.state.borrow().is_compact
    }

    /// The intrinsic activity bar width (excludes any floating gutter).
    fn base_width(&self) -> f64 {
        if self.layout_service.is_floating_panels_enabled() {
            if self.is_compact() {
                return Self::FLOATING_COMPACT_ACTIVITYBAR_WIDTH;
            }
            return Self::FLOATING_ACTIVITYBAR_WIDTH;
        }
        if self.is_compact() { Self::COMPACT_ACTIVITYBAR_WIDTH } else { Self::ACTIVITYBAR_WIDTH }
    }

    /// The action (item) height that drives visible item sizing.
    fn action_height(&self) -> f64 {
        if self.is_compact() {
            return Self::COMPACT_ACTION_HEIGHT;
        }
        if self.layout_service.is_floating_panels_enabled() {
            return Self::FLOATING_ACTION_HEIGHT;
        }
        Self::ACTION_HEIGHT
    }

    /// Vertical gap rendered between two adjacent items. Only the floating panels experiment separates items, and only at the
    /// default activity bar size; the compact Modern UI density tightens it to match its denser rhythm.
    fn action_gap(&self) -> f64 {
        if !self.layout_service.is_floating_panels_enabled() || self.is_compact() {
            return 0.0;
        }
        if self.layout_service.is_modern_ui_compact() { Self::FLOATING_COMPACT_ACTION_GAP } else { Self::FLOATING_ACTION_GAP }
    }

    /// The vertical space a single item occupies in the bar (its height plus the gap that separates it from the next one). This
    /// drives the overflow computation, so it has to track the current activity bar size, otherwise items collapse into the
    /// overflow menu prematurely.
    fn composite_size(&self) -> f64 {
        self.action_height() + self.action_gap()
    }

    fn floating_horizontal_gutter(&self) -> f64 {
        if !self.layout_service.is_floating_panels_enabled() {
            return 0.0;
        }

        // The cluster perimeter on the window side, plus the rail's own lane beside the icon column, where the primary side bar
        // meets it flush.
        let lane = if self.layout_service.is_modern_ui_compact() { Self::FLOATING_COMPACT_LANE } else { Self::FLOATING_LANE };
        get_floating_panel_outer_margin(self.layout_service.as_ref())
            + lane
            + if self.needs_floating_leading_gap() { get_floating_panel_margin(self.layout_service.as_ref()) } else { 0.0 }
    }

    /// Whether the rail has to supply the gap to the card before it. Under this system's leading-margin convention each card
    /// owns the gap on its leading edge, so a rail that follows the editor (on the right, with no side bar to connect to)
    /// reserves one more margin than it does when it leads. Compact density keeps its cards joined, so it never needs the gap.
    fn needs_floating_leading_gap(&self) -> bool {
        !self.layout_service.is_modern_ui_compact()
            && self.layout_service.get_side_bar_position() == Position::Right
            && !self.layout_service.is_visible(Parts::SidebarPart, None)
    }

    fn update_compact_style(&self) {
        let (element, is_compact) = {
            let state = self.state.borrow();
            (state.element.clone(), state.is_compact)
        };
        if let Some(element) = element {
            element.toggle_class("compact", is_compact);
            // Mirrored on the workbench root for floatingPanels.css
            self.root_compact_class.set(is_compact);
            element.set_property("--activity-bar-width", format!("{}px", self.base_width()));
            element.set_property("--activity-bar-action-height", format!("{}px", self.action_height()));
            element.set_property("--activity-bar-action-gap", format!("{}px", self.action_gap()));
            element.set_property("--activity-bar-icon-size", format!("{}px", if is_compact { Self::COMPACT_ICON_SIZE } else { Self::ICON_SIZE }));
        }
    }

    /// A classe `activitybar-compact` do contêiner principal do workbench.
    pub fn root_has_compact_class(&self) -> bool {
        self.root_compact_class.get()
    }

    fn recreate_composite_bar(&self) {
        let content = self.state.borrow().content.clone();
        let Some(content) = content else {
            return;
        };
        if self.composite_bar.value().is_none() {
            return;
        }

        self.composite_bar.clear();
        let bar = self.create_composite_bar();
        self.composite_bar.set(Some(bar.clone()));
        bar.create(&content);

        self.relayout();
    }

    /// `if (this.dimension) this.layout(this.dimension.width, this.dimension.height)`.
    fn relayout(&self) {
        let dimension = self.state.borrow().dimension;
        if let Some(dimension) = dimension {
            self.layout(dimension.width, dimension.height);
        }
    }

    fn create_composite_bar(&self) -> Rc<dyn IPaneCompositeBar> {
        let composite_size = self.composite_size();
        let icon_size = if self.is_compact() { Self::COMPACT_ICON_SIZE } else { Self::ICON_SIZE };
        let layout_service = self.layout_service.clone();

        let options = PaneCompositeBarOptions {
            part_container_class: "activitybar".to_owned(),
            pinned_view_containers_key: Self::PINNED_VIEW_CONTAINERS_KEY.to_owned(),
            placeholder_view_containers_key: Self::PLACEHOLDER_VIEW_CONTAINERS_KEY.to_owned(),
            view_containers_workspace_state_key: Self::VIEW_CONTAINERS_WORKSPACE_STATE_KEY.to_owned(),
            orientation: ActionsOrientation::Vertical,
            icon: true,
            icon_size,
            activity_hover_options: ActivityHoverOptions {
                position: Rc::new(move || if layout_service.get_side_bar_position() == Position::Left { HoverPosition::Right } else { HoverPosition::Left }),
            },
            prevent_loop_navigation: true,
            recompute_sizes: false,
            fill_extra_context_menu_actions: Rc::new(|_actions, _target| {}),
            composite_size,
            colors: Rc::new(|theme: &dyn IColorTheme| CompositeBarColors {
                active_foreground_color: theme.get_color(ACTIVITY_BAR_FOREGROUND, None),
                inactive_foreground_color: theme.get_color(ACTIVITY_BAR_INACTIVE_FOREGROUND, None),
                active_border_color: theme.get_color(ACTIVITY_BAR_ACTIVE_BORDER, None),
                active_background: theme.get_color(ACTIVITY_BAR_ACTIVE_BACKGROUND, None),
                badge_background: theme.get_color(ACTIVITY_BAR_BADGE_BACKGROUND, None),
                badge_foreground: theme.get_color(ACTIVITY_BAR_BADGE_FOREGROUND, None),
                drag_and_drop_border: theme.get_color(ACTIVITY_BAR_DRAG_AND_DROP_BORDER, None),
                active_background_color: None,
                inactive_background_color: None,
                active_border_bottom_color: None,
            }),
            overflow_action_size: composite_size,
        };

        (self.composite_bar_factory)(CompositeBarRequest {
            location: self.location,
            options,
            part: Parts::ActivitybarPart,
            pane_composite_part: self.pane_composite_part.clone(),
            global_activities: Some(GlobalActivities { action_height: self.action_height(), action_gap: self.action_gap() }),
        })
    }

    /// `Part.create(parent)` com o `createContentArea` da barra de atividades, seguido de `updateStyles`.
    pub fn create(&self, parent: &Rc<PartElement>) {
        let content = Rc::new(ContentElement::default());
        {
            let mut state = self.state.borrow_mut();
            state.element = Some(parent.clone());
            state.content = Some(content);
        }

        self.update_compact_style();

        if self.layout_service.is_visible(Parts::ActivitybarPart, None) {
            self.show(false);
        }

        self.update_styles();
    }

    /// O `.content` da parte, depois de `create`.
    pub fn content(&self) -> Option<Rc<ContentElement>> {
        self.state.borrow().content.clone()
    }

    pub fn get_pinned_pane_composite_ids(&self) -> Vec<String> {
        self.composite_bar.value().map(|bar| bar.get_pinned_pane_composite_ids()).unwrap_or_default()
    }

    pub fn get_visible_pane_composite_ids(&self) -> Vec<String> {
        self.composite_bar.value().map(|bar| bar.get_visible_pane_composite_ids()).unwrap_or_default()
    }

    pub fn get_pane_composite_ids(&self) -> Vec<String> {
        self.composite_bar.value().map(|bar| bar.get_pane_composite_ids()).unwrap_or_default()
    }

    pub fn focus(&self) {
        if let Some(bar) = self.composite_bar.value() {
            bar.focus(None);
        }
    }

    /// `getColor(id)` do `Themable`: o texto CSS da cor do tema, ou `None`.
    fn get_color(&self, id: &str) -> Option<String> {
        let theme = self.state.borrow().theme.clone();
        theme.get_color(id, None).map(|color| format_rgb(&color))
    }

    /// As regras de tema dos itens da barra para o tema atual (`registerThemingParticipant`).
    pub fn item_theme_rules(&self) -> ActivityBarItemThemeRules {
        let theme = self.state.borrow().theme.clone();
        activity_bar_item_theme_rules(theme.as_ref())
    }

    pub fn update_styles(&self) {
        let element = self.state.borrow().element.clone().expect("a parte não foi criada");
        let mut background_id = ACTIVITY_BAR_BACKGROUND;
        if matches!(self.configuration_service.get_value(Some(LayoutSettings::ModernUi.as_str()), None), Dynamic::Bool(true)) {
            let window_focused = self.host_service.has_focus();
            let is_window_active = match self.state.borrow().active_window_id {
                None => window_focused,
                Some(window_id) => window_id == MAIN_WINDOW.0,
            };
            let is_inactive = !window_focused || !is_window_active;
            background_id = if is_inactive { MODERN_ACTIVITY_BAR_INACTIVE_BACKGROUND } else { MODERN_ACTIVITY_BAR_BACKGROUND };
        }
        *element.background_color.borrow_mut() = self.get_color(background_id).unwrap_or_default();

        // In Modern UI the rail is a floating card whose border is owned by the stylesheet via the `modernActivityBar.border`
        // token. Leaving it out of the inline style lets CSS vary it per edge (the seam it shares with the side bar).
        let border_color = if self.layout_service.is_floating_panels_enabled() {
            String::new()
        } else {
            self.get_color(ACTIVITY_BAR_BORDER).or_else(|| self.get_color(CONTRAST_BORDER)).unwrap_or_default()
        };
        element.toggle_class("bordered", !border_color.is_empty());
        *element.border_color.borrow_mut() = border_color;
    }

    pub fn show(&self, focus: bool) {
        let content = self.state.borrow().content.clone();
        let Some(content) = content else {
            return;
        };

        if self.composite_bar.value().is_none() {
            let bar = self.create_composite_bar();
            self.composite_bar.set(Some(bar.clone()));
            bar.create(&content);

            self.relayout();
        }

        if focus {
            self.focus();
        }
    }

    pub fn hide(&self) {
        if self.composite_bar.value().is_none() {
            return;
        }

        self.composite_bar.clear();
    }

    pub fn layout(&self, width: f64, height: f64) {
        self.state.borrow_mut().dimension = Some(Dimension { width, height });

        let content = self.state.borrow().content.clone();
        let Some(content) = content else {
            return; // not created yet
        };

        let (top, bottom) = self.get_floating_gutters();
        let content_width = (width - self.floating_horizontal_gutter()).max(0.0);
        let content_height = (height - top - bottom).max(0.0);

        // Layout contents: sem título, cabeçalho nem rodapé, a área de conteúdo ocupa tudo.
        content.set_size(content_width, content_height);

        // The first item has no preceding gap, so give one gap back to the composite bar.
        if let Some(bar) = self.composite_bar.value() {
            bar.layout(content_width, content_height + self.action_gap());
        }
    }

    /// Vertical gutters (in pixels) mirroring the margins in `floatingPanels.css`, plus the card's 1px border on each edge. The
    /// top is flush with title/banner chrome and doubles only at an exposed window edge.
    fn get_floating_gutters(&self) -> (f64, f64) {
        if !self.layout_service.is_floating_panels_enabled() {
            return (0.0, 0.0);
        }

        let layout_service = self.layout_service.as_ref();
        let margin = get_floating_panel_margin(layout_service);
        let outer_margin = get_floating_panel_outer_margin(layout_service);
        let border = Self::FLOATING_BORDER;
        let top = border + if is_floating_top_edge_exposed(layout_service, MAIN_WINDOW) { outer_margin } else { FLOATING_PANEL_INNER_MARGIN };
        let bottom = border
            + if layout_service.is_modern_ui_compact() {
                outer_margin
            } else if layout_service.is_visible(Parts::StatusbarPart, Some(MAIN_WINDOW)) {
                margin
            } else {
                outer_margin
            };
        (top, bottom)
    }

    pub fn to_json(&self) -> ActivitybarPartJson {
        ActivitybarPartJson { part_type: Parts::ActivitybarPart }
    }
}

impl LayoutPart for ActivitybarPart {
    fn get_id(&self) -> String {
        Parts::ActivitybarPart.as_str().to_owned()
    }
}

impl IDisposable for ActivitybarPart {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

/// O `{ width, height }` que `ActivityBarCompositeBar.layout` desconta do menu compacto: `clientWidth` e `clientHeight`.
#[derive(Default)]
pub struct MenuBarContainer {
    pub client_width: Cell<f64>,
    pub client_height: Cell<f64>,
}

/// O que a `ActivityBarCompositeBar` injeta do construtor além da base: `IConfigurationService`, `IMenuService`,
/// `IContextKeyService`, `IWorkbenchLayoutService` e `IInstantiationService` (o `invokeFunction` das ações), mais o
/// `CustomMenubarControl` e o `part` que a barra serve.
pub struct ActivityBarServices {
    pub part: Parts,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub menu_service: Rc<dyn IMenuService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub layout_service: Rc<dyn IWorkbenchLayoutService>,
    pub title_bar_host: Rc<dyn TitleBarHost>,
    pub instantiation_service: Rc<InstantiationService>,
    pub menubar_factory: MenubarFactory,
}

/// `ActivityBarCompositeBar`: a barra de painéis da barra de atividades, que reserva no layout o espaço do menu compacto e
/// dos ícones globais (conta e gerenciar).
pub struct ActivityBarCompositeBar {
    base: Rc<dyn IPaneCompositeBar>,
    orientation: ActionsOrientation,
    element: RefCell<Option<Rc<ContentElement>>>,
    menu_bar: Rc<MutableDisposable<dyn ICustomMenubarControl>>,
    menu_bar_container: RefCell<Option<Rc<MenuBarContainer>>>,
    global_composite_bar: Option<Rc<dyn IGlobalCompositeBar>>,
    global_activities: Option<GlobalActivities>,
    last_layout_dimensions: RefCell<Option<Dimension>>,
    services: Option<ActivityBarServices>,
    disposables: Rc<Disposable>,
}

impl ActivityBarCompositeBar {
    /// O construtor sem as dependências de menu: `base` é o `PaneCompositeBar` da superclasse; quando há `globalActivities`, a
    /// barra global entra e um `onDidChange` dela refaz o layout com as últimas dimensões. Sem [`ActivityBarServices`] a barra
    /// só faz o layout (o menu compacto vem de [`ActivityBarCompositeBar::set_menu_bar_container`]); o menu de contexto, o
    /// menu compacto instalado pela configuração e o teclado exigem [`ActivityBarCompositeBar::with_services`].
    pub fn new(
        base: Rc<dyn IPaneCompositeBar>,
        orientation: ActionsOrientation,
        global_composite_bar: Option<Rc<dyn IGlobalCompositeBar>>,
        global_activities: Option<GlobalActivities>,
    ) -> Rc<Self> {
        Self::build(base, orientation, global_composite_bar, global_activities, None)
    }

    /// O construtor completo. `make_base` cria o `PaneCompositeBar` da superclasse com as opções já embrulhadas
    /// (`{...options, fillExtraContextMenuActions: (actions, e) => { options.fill...(actions, e); this.fillContextMenuActions(actions, e); }}`)
    /// e `make_global` cria a `GlobalCompositeBar` (só chamada quando há `globalActivities`) a partir da base.
    pub fn with_services(
        options: PaneCompositeBarOptions,
        make_base: impl FnOnce(PaneCompositeBarOptions) -> Rc<dyn IPaneCompositeBar>,
        make_global: impl FnOnce(&Rc<dyn IPaneCompositeBar>) -> Rc<dyn IGlobalCompositeBar>,
        global_activities: Option<GlobalActivities>,
        services: ActivityBarServices,
    ) -> Rc<Self> {
        // O gancho precisa da própria barra, que só existe depois da base: a referência é preenchida logo após a construção.
        let slot: Rc<RefCell<Weak<Self>>> = Rc::new(RefCell::new(Weak::new()));
        let orientation = options.orientation;
        let original = options.fill_extra_context_menu_actions.clone();
        let hook_slot = slot.clone();
        let wrapped = PaneCompositeBarOptions {
            fill_extra_context_menu_actions: Rc::new(move |actions, target| {
                original(actions, target);
                let this = hook_slot.borrow().upgrade();
                if let Some(this) = this {
                    this.fill_context_menu_actions(actions, target);
                }
            }),
            ..options
        };
        let base = make_base(wrapped);
        let global = global_activities.map(|_| make_global(&base));
        let this = Self::build(base, orientation, global, global_activities, Some(services));
        *slot.borrow_mut() = Rc::downgrade(&this);
        this
    }

    fn build(
        base: Rc<dyn IPaneCompositeBar>,
        orientation: ActionsOrientation,
        global_composite_bar: Option<Rc<dyn IGlobalCompositeBar>>,
        global_activities: Option<GlobalActivities>,
        services: Option<ActivityBarServices>,
    ) -> Rc<Self> {
        let global_composite_bar = global_activities.and(global_composite_bar);
        let this = Rc::new(Self {
            base,
            orientation,
            element: RefCell::new(None),
            menu_bar: MutableDisposable::new(),
            menu_bar_container: RefCell::new(None),
            global_composite_bar: global_composite_bar.clone(),
            global_activities,
            last_layout_dimensions: RefCell::new(None),
            services,
            disposables: Disposable::new(),
        });
        this.disposables.register(this.menu_bar.clone());
        if let Some(global) = global_composite_bar {
            this.disposables.register(global.clone());
            let weak = Rc::downgrade(&this);
            this.disposables.register(global.on_did_change().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    let last = *this.last_layout_dimensions.borrow();
                    if let Some(last) = last {
                        this.layout(last.width, last.height);
                    }
                }
            }));
        }
        this.disposables.register(this.base.clone());

        // Register for configuration changes
        if let Some(services) = &this.services {
            let weak = Rc::downgrade(&this);
            this.disposables.register(services.configuration_service.on_did_change_configuration().listen(move |event| {
                let Some(this) = weak.upgrade() else {
                    return;
                };
                if event.affects_configuration(MENU_BAR_VISIBILITY_SETTING, None) {
                    if this.menu_bar_visibility() == Some(MenuBarVisibility::Compact) {
                        this.install_menubar();
                    } else {
                        this.uninstall_menubar();
                    }
                }
            }));
        }
        this
    }

    /// O `menuBarContainer` criado por `installMenubar`.
    pub fn set_menu_bar_container(&self, container: Option<Rc<MenuBarContainer>>) {
        *self.menu_bar_container.borrow_mut() = container;
    }

    /// O `menuBarContainer` atual: `Some` enquanto o menu compacto está instalado.
    pub fn menu_bar_container(&self) -> Option<Rc<MenuBarContainer>> {
        self.menu_bar_container.borrow().clone()
    }

    /// `getMenuBarVisibility(this.configurationService)`.
    fn menu_bar_visibility(&self) -> Option<MenuBarVisibility> {
        let services = self.services.as_ref()?;
        MenuBarVisibility::parse(&services.title_bar_host.get_menu_bar_visibility()?)
    }

    fn fill_context_menu_actions(&self, actions: &mut Vec<ActionRef>, target: Option<ContextMenuTarget>) {
        if self.services.is_none() {
            return;
        }

        // Menu
        let menu_bar_visibility = self.menu_bar_visibility();
        let mut head: Vec<ActionRef> = Vec::new();
        if matches!(menu_bar_visibility, Some(MenuBarVisibility::Compact | MenuBarVisibility::Hidden | MenuBarVisibility::Toggle)) {
            let is_compact = menu_bar_visibility == Some(MenuBarVisibility::Compact);
            head.push(self.update_menu_bar_visibility_action("toggleMenuVisibility", localize("menu", "Menu", &[]), Some(is_compact), if is_compact { "toggle" } else { "compact" }));
            head.push(Separator::new());
        }

        if menu_bar_visibility == Some(MenuBarVisibility::Compact)
            && self.menu_bar_container.borrow().is_some()
            && target.is_some_and(|target| target.in_menu_bar_container)
        {
            // `unshift` depois do anterior: o item de esconder fica na frente.
            let mut hide = vec![self.update_menu_bar_visibility_action("hideCompactMenu", localize("hideMenu", "Hide Menu", &[]), None, "toggle"), Separator::new() as ActionRef];
            hide.append(&mut head);
            head = hide;
        }
        actions.splice(0..0, head);

        // Global Composite Bar
        if let Some(global) = &self.global_composite_bar {
            actions.push(Separator::new());
            actions.extend(global.get_context_menu_actions());
        }
        actions.push(Separator::new());
        actions.extend(self.get_activity_bar_context_menu_actions());
    }

    /// `toAction({ id, label, checked, run: () => configurationService.updateValue(MenuSettings.MenuBarVisibility, value) })`.
    fn update_menu_bar_visibility_action(&self, id: &str, label: String, checked: Option<bool>, value: &'static str) -> ActionRef {
        let configuration_service = self.services.as_ref().expect("as dependências foram injetadas").configuration_service.clone();
        to_action(ToActionProps {
            id: id.to_owned(),
            label,
            tooltip: None,
            enabled: None,
            checked,
            class: None,
            alt: None,
            run: Rc::new(move |_| {
                let _ = configuration_service.update_value(MENU_BAR_VISIBILITY_SETTING, Dynamic::string(value), None, None);
                MaybePromise::Value(Dynamic::Undefined)
            }),
        })
    }

    /// `toAction({ id, label, checked, run: () => configurationService.updateValue(ACTIVITY_BAR_COMPACT, value) })`.
    fn activity_bar_size_action(configuration_service: &Rc<dyn IConfigurationService>, id: &str, label: String, compact: bool, is_compact: bool) -> ActionRef {
        let configuration_service = configuration_service.clone();
        to_action(ToActionProps {
            id: id.to_owned(),
            label,
            tooltip: None,
            enabled: None,
            checked: Some(is_compact == compact),
            class: None,
            alt: None,
            run: Rc::new(move |_| {
                let _ = configuration_service.update_value(LayoutSettings::ActivityBarCompact.as_str(), Dynamic::Bool(compact), None, None);
                MaybePromise::Value(Dynamic::Undefined)
            }),
        })
    }

    fn uninstall_menubar(&self) {
        self.menu_bar.clear();
        *self.menu_bar_container.borrow_mut() = None;
    }

    fn install_menubar(&self) {
        let Some(services) = &self.services else {
            return;
        };
        if self.menu_bar.value().is_some() {
            return; // prevent menu bar from installing twice #110720
        }

        let container = Rc::new(MenuBarContainer::default());
        *self.menu_bar_container.borrow_mut() = Some(container.clone());
        assert!(self.element.borrow().is_some(), "a barra não foi criada");

        // Menubar: install a custom menu bar depending on configuration
        let menu_bar = (services.menubar_factory)();
        self.menu_bar.set(Some(menu_bar.clone()));
        menu_bar.create(&container);
    }

    /// O `KEY_DOWN` do menu compacto: Baixo ou Direita passam o foco para os ícones.
    pub fn on_menu_bar_key_down(&self, event: &StandardKeyboardEvent) {
        if Self::is_any_key(event, &[KeyCode::DownArrow, KeyCode::RightArrow]) {
            self.focus(None);
        }
    }

    /// O `KEY_DOWN` do contêiner dos ícones: Baixo ou Direita passam o foco para os ícones globais, Cima ou Esquerda para o
    /// menu compacto.
    pub fn on_composite_bar_key_down(&self, event: &StandardKeyboardEvent) {
        if Self::is_any_key(event, &[KeyCode::DownArrow, KeyCode::RightArrow]) {
            if let Some(global) = &self.global_composite_bar {
                global.focus();
            }
        } else if Self::is_any_key(event, &[KeyCode::UpArrow, KeyCode::LeftArrow]) {
            if let Some(menu_bar) = self.menu_bar.value() {
                menu_bar.toggle_focus();
            }
        }
    }

    /// O `KEY_DOWN` dos ícones globais: Cima ou Esquerda devolvem o foco ao último ícone de painel visível.
    pub fn on_global_composite_bar_key_down(&self, event: &StandardKeyboardEvent) {
        if Self::is_any_key(event, &[KeyCode::UpArrow, KeyCode::LeftArrow]) {
            self.focus(Some(self.get_visible_pane_composite_ids().len().saturating_sub(1)));
        }
    }

    /// `kbEvent.equals(KeyCode.X) || kbEvent.equals(KeyCode.Y)`.
    fn is_any_key(event: &StandardKeyboardEvent, key_codes: &[KeyCode]) -> bool {
        key_codes.iter().any(|key_code| event.equals(key_code.bits()))
    }

    /// `getActivityBarContextMenuActions()`.
    pub fn get_activity_bar_context_menu_actions(&self) -> Vec<ActionRef> {
        let Some(services) = &self.services else {
            return Vec::new();
        };

        let options = MenuActionOptions { arg: None, args: None, should_forward_args: true, render_short_title: true };
        let activity_bar_position_menu =
            services.menu_service.get_menu_actions(&menu_ids::activity_bar_position_menu(), services.context_key_service.as_ref(), Some(&options));
        let position_actions = get_context_menu_actions(&activity_bar_position_menu, None).secondary;
        let position_submenu: ActionRef =
            SubmenuAction::new("workbench.action.activityBar.position", &localize("activity bar position", "Activity Bar Position", &[]), position_actions, None);
        let mut actions: Vec<ActionRef> = vec![position_submenu];

        // Show size submenu only when activity bar is in default position
        let configuration_service = &services.configuration_service;
        let activity_bar_position = configuration_service.get_value(Some(LayoutSettings::ActivityBarLocation.as_str()), None);
        if activity_bar_position.strict_equals(&Dynamic::from(ActivityBarPosition::Default.as_str())) {
            let is_compact = ActivitybarPart::read_compact(configuration_service.as_ref());
            let size_actions = vec![
                Self::activity_bar_size_action(configuration_service, "workbench.action.activityBar.size.default", localize("activityBarSizeDefault", "Default", &[]), false, is_compact),
                Self::activity_bar_size_action(configuration_service, "workbench.action.activityBar.size.compact", localize("activityBarSizeCompact", "Compact", &[]), true, is_compact),
            ];
            let size_submenu: ActionRef = SubmenuAction::new("workbench.action.activityBar.size", &localize("activity bar size", "Activity Bar Size", &[]), size_actions, None);
            actions.push(size_submenu);
        }

        let layout_service = services.layout_service.clone();
        actions.push(self.invoke_action(
            ToggleSidebarPositionAction::ID,
            ToggleSidebarPositionAction::get_label(layout_service.as_ref()),
            Rc::new(ToggleSidebarPositionAction),
        ));

        if services.part == Parts::SidebarPart {
            actions.push(self.invoke_action(ToggleSidebarVisibilityAction::ID, ToggleSidebarVisibilityAction::label(), Rc::new(ToggleSidebarVisibilityAction)));
        }

        actions
    }

    /// `toAction({ id, label, run: () => this.instantiationService.invokeFunction(accessor => new Action().run(accessor)) })`.
    fn invoke_action(&self, id: &str, label: String, action: Rc<dyn Action2>) -> ActionRef {
        let instantiation_service = self.services.as_ref().expect("as dependências foram injetadas").instantiation_service.clone();
        to_action(ToActionProps {
            id: id.to_owned(),
            label,
            tooltip: None,
            enabled: None,
            checked: None,
            class: None,
            alt: None,
            run: Rc::new(move |_| {
                instantiation_service.invoke_function(|accessor| action.run(accessor, &[]));
                MaybePromise::Value(Dynamic::Undefined)
            }),
        })
    }
}

impl IDisposable for ActivityBarCompositeBar {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}

impl IPaneCompositeBar for ActivityBarCompositeBar {
    fn create(&self, parent: &Rc<ContentElement>) {
        *self.element.borrow_mut() = Some(parent.clone());

        // Install menubar if compact
        if self.menu_bar_visibility() == Some(MenuBarVisibility::Compact) {
            self.install_menubar();
        }

        // View Containers action bar
        self.base.create(parent);

        // Global action bar
        if let Some(global) = &self.global_composite_bar {
            global.create(parent);
        }

        // Keyboard Navigation: `on_menu_bar_key_down`, `on_composite_bar_key_down` e `on_global_composite_bar_key_down`.
    }

    fn layout(&self, mut width: f64, mut height: f64) {
        *self.last_layout_dimensions.borrow_mut() = Some(Dimension { width, height });

        if let Some(menu_bar) = &*self.menu_bar_container.borrow() {
            if self.orientation == ActionsOrientation::Vertical {
                height -= menu_bar.client_height.get();
            } else {
                width -= menu_bar.client_width.get();
            }
        }
        if let (Some(global), Some(activities)) = (&self.global_composite_bar, &self.global_activities) {
            if self.orientation == ActionsOrientation::Vertical {
                height -= global.get_height(activities.action_height, activities.action_gap);
            } else {
                width -= global.client_width();
            }
        }
        self.base.layout(width, height);
    }

    fn focus(&self, index: Option<usize>) {
        self.base.focus(index);
    }

    fn get_pinned_pane_composite_ids(&self) -> Vec<String> {
        self.base.get_pinned_pane_composite_ids()
    }

    fn get_visible_pane_composite_ids(&self) -> Vec<String> {
        self.base.get_visible_pane_composite_ids()
    }

    fn get_pane_composite_ids(&self) -> Vec<String> {
        self.base.get_pane_composite_ids()
    }
}

// region Ações

fn and_of(items: impl IntoIterator<Item = ContextKeyExpression>) -> ContextKeyExpression {
    ContextKeyExpr::and(items).expect("a conjunção tem termos")
}

fn config_key_of_location() -> String {
    format!("config.{}", LayoutSettings::ActivityBarLocation.as_str())
}

/// Uma das quatro ações `workbench.action.activityBarLocation.*`.
struct ActivityBarLocationAction {
    position: ActivityBarPosition,
    id: &'static str,
    title_key: &'static str,
    title_message: &'static str,
    mnemonic_key: &'static str,
    mnemonic_message: &'static str,
    short_key: &'static str,
    short_message: &'static str,
    order: f64,
}

impl Action2 for ActivityBarLocationAction {
    fn desc(&self) -> Action2Options {
        let title = localize2(self.title_key, self.title_message);
        let in_position = ContextKeyExpr::equals(&config_key_of_location(), self.position.as_str());
        let not_in_position = ContextKeyExpr::not_equals(&config_key_of_location(), self.position.as_str());
        Action2Options {
            command: CommandAction {
                short_title: Some(Title::Plain(localize(self.short_key, self.short_message, &[]))),
                category: Some(Category::Localized(Categories::new().view)),
                toggled: Some(Toggled::Expression(in_position)),
                ..CommandAction::new(
                    self.id,
                    Title::Localized(CommandActionTitle {
                        value: title.value,
                        original: title.original,
                        mnemonic_title: Some(localize(self.mnemonic_key, self.mnemonic_message, &[])),
                    }),
                )
            },
            menu: vec![
                Action2MenuItem {
                    id: menu_ids::activity_bar_position_menu(),
                    null_precondition: false,
                    alt: None,
                    when: None,
                    group: None,
                    order: Some(self.order),
                    is_hidden_by_default: None,
                },
                Action2MenuItem {
                    id: menu_ids::command_palette(),
                    null_precondition: false,
                    alt: None,
                    when: Some(and_of([not_in_position, is_sessions_window_context().to_negated()])),
                    group: None,
                    order: None,
                    is_hidden_by_default: None,
                },
            ],
            keybinding: Vec::new(),
            f1: false,
        }
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let configuration_service = accessor.get(I_CONFIGURATION_SERVICE);
        let _ = configuration_service.update_value(LayoutSettings::ActivityBarLocation.as_str(), Dynamic::string(self.position.as_str()), None, None);
    }
}

/// `FocusActivityBarAction`.
struct FocusActivityBarAction;

impl Action2 for FocusActivityBarAction {
    fn desc(&self) -> Action2Options {
        view_f1_options("workbench.action.focusActivityBar", localize2("focusActivityBar", "Focus Activity Bar"))
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let layout_service = accessor.get(I_WORKBENCH_LAYOUT_SERVICE);
        layout_service.focus_part(Parts::ActivitybarPart, None);
    }
}

/// `{ id, title, category: Categories.View, f1: true }`: as opções das ações `focusActivityBar`, `previousSideBarView` e
/// `nextSideBarView`.
fn view_f1_options(id: &str, title: LocalizedString) -> Action2Options {
    Action2Options {
        command: CommandAction {
            category: Some(Category::Localized(Categories::new().view)),
            ..CommandAction::new(id, Title::Localized(CommandActionTitle { value: title.value, original: title.original, mnemonic_title: None }))
        },
        menu: Vec::new(),
        keybinding: Vec::new(),
        f1: true,
    }
}

/// Os quatro `registerAction2` de posição, o de foco, os dois de troca de vista e os dois `MenuRegistry.appendMenuItem` do
/// submenu de posição.
pub fn register_activitybar_actions() -> DisposableRef {
    let locations = [
        (ActivityBarPosition::Default, "workbench.action.activityBarLocation.default", "positionActivityBarDefault", "Move Activity Bar to Side", "miDefaultActivityBar", "&&Default", "default", "Default", 1.0),
        (ActivityBarPosition::Top, "workbench.action.activityBarLocation.top", "positionActivityBarTop", "Move Activity Bar to Top", "miTopActivityBar", "&&Top", "top", "Top", 2.0),
        (ActivityBarPosition::Bottom, "workbench.action.activityBarLocation.bottom", "positionActivityBarBottom", "Move Activity Bar to Bottom", "miBottomActivityBar", "&&Bottom", "bottom", "Bottom", 3.0),
        (ActivityBarPosition::Hidden, "workbench.action.activityBarLocation.hide", "hideActivityBar", "Hide Activity Bar", "miHideActivityBar", "&&Hidden", "hide", "Hidden", 4.0),
    ];
    let mut disposables: Vec<DisposableRef> = locations
        .into_iter()
        .map(|(position, id, title_key, title_message, mnemonic_key, mnemonic_message, short_key, short_message, order)| {
            register_action2(Rc::new(ActivityBarLocationAction { position, id, title_key, title_message, mnemonic_key, mnemonic_message, short_key, short_message, order }))
        })
        .collect();
    disposables.push(register_action2(Rc::new(FocusActivityBarAction)));
    disposables.push(register_action2(Rc::new(SwitchCompositeViewAction::new(
        view_f1_options("workbench.action.previousSideBarView", localize2("previousSideBarView", "Previous Primary Side Bar View")),
        ViewContainerLocation::Sidebar,
        -1,
    ))));
    disposables.push(register_action2(Rc::new(SwitchCompositeViewAction::new(
        view_f1_options("workbench.action.nextSideBarView", localize2("nextSideBarView", "Next Primary Side Bar View")),
        ViewContainerLocation::Sidebar,
        1,
    ))));
    disposables.push(register_activitybar_position_menus());
    combined_disposable(disposables)
}

/// Os dois `MenuRegistry.appendMenuItem` que penduram `ActivityBarPositionMenu` na barra de menus e no título do contêiner.
pub fn register_activitybar_position_menus() -> DisposableRef {
    let registry = menu_registry();
    let appearance = registry.append_menu_item(
        &menu_ids::menubar_appearance_menu(),
        AnyMenuItem::Submenu(SubmenuItem {
            title: Title::Plain(localize("positionActivituBar", "Activity Bar Position", &[])),
            submenu: menu_ids::activity_bar_position_menu(),
            icon: None,
            when: Some(is_sessions_window_context().to_negated()),
            group: Some("3_workbench_layout_move".to_owned()),
            order: Some(2.0),
            is_selection: None,
            is_split_button: None,
        }),
    );
    let location_key = "viewContainerLocation";
    let title_context = registry.append_menu_item(
        &menu_ids::view_container_title_context(),
        AnyMenuItem::Submenu(SubmenuItem {
            title: Title::Plain(localize("positionActivituBar", "Activity Bar Position", &[])),
            submenu: menu_ids::activity_bar_position_menu(),
            icon: None,
            when: ContextKeyExpr::or([
                ContextKeyExpr::equals(location_key, view_container_location_to_string(ViewContainerLocation::Sidebar)),
                ContextKeyExpr::equals(location_key, view_container_location_to_string(ViewContainerLocation::AuxiliaryBar)),
            ]),
            group: Some("3_workbench_layout_move".to_owned()),
            order: Some(1.0),
            is_selection: None,
            is_split_button: None,
        }),
    );
    to_disposable(move || {
        appearance.dispose();
        title_context.dispose();
    })
}

// endregion
