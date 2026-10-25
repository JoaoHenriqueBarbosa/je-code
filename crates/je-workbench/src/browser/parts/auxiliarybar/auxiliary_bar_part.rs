//! Porte de `src/vs/workbench/browser/parts/auxiliarybar/auxiliaryBarPart.ts` (e de `media/auxiliaryBarPart.css`): a
//! [`AuxiliaryBarPart`], a barra lateral secundária: os limites, o `preferredHeight` e o `preferredWidth`, a configuração de
//! posição e rótulos da barra de composites, os estilos, o comportamento responsivo das ações do título e o rastreador de
//! contêineres visíveis para esconder a barra.
//!
//! Divergências:
//! - **Herança vira composição.** `AuxiliaryBarPart extends AbstractPaneCompositePart` é a [`AuxiliaryBarPart`], que contém a
//!   [`AbstractPaneCompositePart`] (`Deref`) e se apresenta a ela como o [`PaneCompositePartBehavior`] [`AuxiliaryBarBehavior`]. Uma
//!   subclasse (que sobrescreve um membro `protected`) é um [`PaneCompositePartBehavior`] que repassa ao [`AuxiliaryBarBehavior`] e
//!   que entra por [`AuxiliaryBarPart::extend`].
//! - **`ToggleAuxiliaryBarAction.ID`** (`auxiliaryBarActions.ts`) e **`ToggleSidebarPositionAction.ID`** (`layoutActions.ts`) são
//!   [`TOGGLE_AUXILIARY_BAR_ACTION_ID`] e o `ID` da ação portada.
//! - **`get overflowActionSize()` e os getters de `colors`**, avaliados a cada leitura no original, são avaliados a cada
//!   chamada de `colors(theme)` (as cores) e a cada montagem das opções (o tamanho do botão de transbordo).
//! - **`configuration.position`** é o valor da configuração; um texto que não é uma posição conhecida é `None`, que o `switch` do
//!   original trata como o padrão.
//! - **`container.style.<propriedade>`** são as propriedades CSS do `ViewElement`, com o valor normalizado do CSSOM (`rgb(...)`).

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, Separator, SubmenuAction, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_editor::services::theme_service::IColorTheme;
use je_platform::actions::menu_entry_action_view_item::get_context_menu_actions;
use je_platform::actions::{MenuActionOptions, SubmenuItemAction, menu_ids};
use je_platform::commands::ICommandService;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;
use je_widgets::action_bar::ActionsOrientation;
use je_widgets::hover_api::HoverPosition;
use je_widgets::toolbar::{IToolBarResponsiveBehaviorOptions, OverflowFrom, ResponsiveKind};

use crate::browser::actions::layout_actions::ToggleSidebarPositionAction;
use crate::browser::pane_composite::extensions;
use crate::browser::part::PartOptions;
use crate::browser::parts::pane_composite_bar::{ActivityHoverOptions, CompositeBarColors, ContextMenuTarget, PaneCompositeBarOptions};
use crate::browser::parts::pane_composite_part::{
    AbstractPaneCompositePart, CompositeBarPosition, PaneCompositePartArgs, PaneCompositePartBehavior, PaneCompositePartServices,
};
use crate::browser::parts::part_support::set_style_or_remove;
use crate::browser::parts::visible_view_containers_tracker::{VisibleCountChange, VisibleViewContainersTracker};
use crate::common::contextkeys::{active_auxiliary_context, auxiliary_bar_focus_context};
use crate::common::views::ViewContainerLocation;
use crate::services::layout::layout_service::{ActivityBarPosition, LayoutSettings, Parts, Position};

// As cores de `workbench/common/theme.ts` e de `colorRegistry.ts` que a parte lê.
pub const SIDE_BAR_BACKGROUND: &str = "sideBar.background";
pub const SIDE_BAR_FOREGROUND: &str = "sideBar.foreground";
pub const SIDE_BAR_BORDER: &str = "sideBar.border";
pub const SIDE_BAR_TITLE_BORDER: &str = "sideBarTitle.border";
pub const ACTIVITY_BAR_BADGE_BACKGROUND: &str = "activityBarBadge.background";
pub const ACTIVITY_BAR_BADGE_FOREGROUND: &str = "activityBarBadge.foreground";
pub const ACTIVITY_BAR_TOP_ACTIVE_BORDER: &str = "activityBarTop.activeBorder";
pub const ACTIVITY_BAR_TOP_DRAG_AND_DROP_BORDER: &str = "activityBarTop.dropBorder";
pub const ACTIVITY_BAR_TOP_FOREGROUND: &str = "activityBarTop.foreground";
pub const ACTIVITY_BAR_TOP_INACTIVE_FOREGROUND: &str = "activityBarTop.inactiveForeground";
pub const PANEL_ACTIVE_TITLE_BORDER: &str = "panelTitle.activeBorder";
pub const PANEL_ACTIVE_TITLE_FOREGROUND: &str = "panelTitle.activeForeground";
pub const PANEL_DRAG_AND_DROP_BORDER: &str = "panel.dropBorder";
pub const PANEL_INACTIVE_TITLE_FOREGROUND: &str = "panelTitle.inactiveForeground";
pub const CONTRAST_BORDER: &str = "contrastBorder";

/// `ToggleAuxiliaryBarAction.ID` (`auxiliaryBarActions.ts`).
pub const TOGGLE_AUXILIARY_BAR_ACTION_ID: &str = "workbench.action.toggleAuxiliaryBar";

/// `AuxiliaryBarPart.activeViewSettingsKey`.
pub const ACTIVE_VIEW_SETTINGS_KEY: &str = "workbench.auxiliarybar.activepanelid";
/// `AuxiliaryBarPart.pinnedViewsKey`.
pub const PINNED_VIEWS_KEY: &str = "workbench.auxiliarybar.pinnedPanels";
/// `AuxiliaryBarPart.placeholdeViewContainersKey`.
pub const PLACEHOLDER_VIEW_CONTAINERS_KEY: &str = "workbench.auxiliarybar.placeholderPanels";
/// `AuxiliaryBarPart.viewContainersWorkspaceStateKey`.
pub const VIEW_CONTAINERS_WORKSPACE_STATE_KEY: &str = "workbench.auxiliarybar.viewContainersWorkspaceState";

/// `workbench.secondarySideBar.showLabels`.
const SHOW_LABELS_SETTING: &str = "workbench.secondarySideBar.showLabels";

/// Os serviços que o construtor da `AuxiliaryBarPart` injeta além dos da base.
#[derive(Clone)]
pub struct AuxiliaryBarPartServices {
    pub pane: PaneCompositePartServices,
    pub command_service: Rc<dyn ICommandService>,
}

/// `IAuxiliaryBarPartConfiguration`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxiliaryBarPartConfiguration {
    pub position: Option<ActivityBarPosition>,
    pub can_show_labels: bool,
    pub show_labels: bool,
}

/// A posição da barra de atividades que o valor de configuração é, se for uma delas.
fn activity_bar_position_of(value: &Dynamic) -> Option<ActivityBarPosition> {
    [ActivityBarPosition::Default, ActivityBarPosition::Top, ActivityBarPosition::Bottom, ActivityBarPosition::Hidden]
        .into_iter()
        .find(|position| value.strict_equals(&Dynamic::from(position.as_str())))
}

fn is_top_or_bottom(position: Option<ActivityBarPosition>) -> bool {
    matches!(position, Some(ActivityBarPosition::Top | ActivityBarPosition::Bottom))
}

/// `toAction({ id, label, enabled, run })`.
fn action_of(id: &str, label: String, enabled: bool, run: impl Fn() + 'static) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label,
        tooltip: None,
        enabled: Some(enabled),
        checked: None,
        class: None,
        alt: None,
        run: Rc::new(move |_| {
            run();
            MaybePromise::Value(Dynamic::Undefined)
        }),
    })
}

/// O [`PaneCompositePartBehavior`] da barra auxiliar.
pub struct AuxiliaryBarBehavior {
    configuration: Rc<RefCell<AuxiliaryBarPartConfiguration>>,
    visible_view_containers_tracker: RefCell<Option<Rc<VisibleViewContainersTracker>>>,
    command_service: Rc<dyn ICommandService>,
}

impl AuxiliaryBarBehavior {
    fn new(configuration: Rc<RefCell<AuxiliaryBarPartConfiguration>>, command_service: Rc<dyn ICommandService>) -> Rc<Self> {
        Rc::new(Self { configuration, visible_view_containers_tracker: RefCell::new(None), command_service })
    }

    fn configuration(&self) -> AuxiliaryBarPartConfiguration {
        *self.configuration.borrow()
    }

    /// `fillExtraContextMenuActions(actions)`.
    fn fill_extra_context_menu_actions(
        configuration: AuxiliaryBarPartConfiguration,
        command_service: &Rc<dyn ICommandService>,
        part: &AbstractPaneCompositePart,
        actions: &mut Vec<ActionRef>,
    ) {
        let current_position_right = part.layout_service().get_side_bar_position() == Position::Left;

        if part.get_composite_bar_position() == CompositeBarPosition::Title
            && let Some(views_submenu_action) = part.get_views_submenu_action()
        {
            actions.push(Separator::new());
            actions.push(views_submenu_action);
        }

        let options = MenuActionOptions { arg: None, args: None, should_forward_args: true, render_short_title: true };
        let activity_bar_position_menu = part.menu_service().get_menu_actions(&menu_ids::activity_bar_position_menu(), part.context_key_service().as_ref(), Some(&options));
        let position_actions = get_context_menu_actions(&activity_bar_position_menu, None).secondary;

        let configuration_service = part.configuration_service().clone();
        let toggle_show_labels_action = action_of(
            "workbench.action.auxiliarybar.toggleShowLabels",
            if configuration.show_labels { localize("showIcons", "Show Icons", &[]) } else { localize("showLabels", "Show Labels", &[]) },
            configuration.can_show_labels,
            move || {
                let _ = configuration_service.update_value(SHOW_LABELS_SETTING, Dynamic::Bool(!configuration.show_labels), None, None);
            },
        );

        actions.push(Separator::new());
        let position_submenu: ActionRef = SubmenuAction::new("workbench.action.panel.position", &localize("activity bar position", "Activity Bar Position", &[]), position_actions, None);
        actions.push(position_submenu);
        let toggle_sidebar_position_service = command_service.clone();
        actions.push(action_of(
            ToggleSidebarPositionAction::ID,
            if current_position_right { localize("move second side bar left", "Move Secondary Side Bar Left", &[]) } else { localize("move second side bar right", "Move Secondary Side Bar Right", &[]) },
            true,
            move || {
                let _ = toggle_sidebar_position_service.execute_command(ToggleSidebarPositionAction::ID, Vec::new());
            },
        ));
        actions.push(toggle_show_labels_action);
        let toggle_auxiliary_bar_service = command_service.clone();
        actions.push(action_of(TOGGLE_AUXILIARY_BAR_ACTION_ID, localize("hide second side bar", "Hide Secondary Side Bar", &[]), true, move || {
            let _ = toggle_auxiliary_bar_service.execute_command(TOGGLE_AUXILIARY_BAR_ACTION_ID, Vec::new());
        }));
    }
}

impl PaneCompositePartBehavior for AuxiliaryBarBehavior {
    // Use the side bar dimensions
    fn minimum_width(&self) -> f64 {
        170.0
    }

    fn maximum_width(&self) -> f64 {
        f64::INFINITY
    }

    fn minimum_height(&self) -> f64 {
        0.0
    }

    fn maximum_height(&self) -> f64 {
        f64::INFINITY
    }

    fn to_json(&self) -> Dynamic {
        Dynamic::object([("type", Dynamic::from(Parts::AuxiliarybarPart.as_str()))])
    }

    fn get_toolbar_responsive_behavior(&self) -> Option<IToolBarResponsiveBehaviorOptions> {
        Some(IToolBarResponsiveBehaviorOptions {
            enabled: true,
            kind: ResponsiveKind::All,
            min_items: Some(0),
            overflow_from: Some(OverflowFrom::Start),
            action_min_width: Some(22.0),
            get_action_min_width: Some(Rc::new(|action: &ActionRef| {
                SubmenuItemAction::from_action(action).filter(|submenu| submenu.item.is_split_button.is_some()).map(|_| 36.0)
            })),
            ..IToolBarResponsiveBehaviorOptions::default()
        })
    }

    fn update_styles(&self, part: &AbstractPaneCompositePart) {
        part.update_styles_base();

        let Some(container) = part.get_container() else { return };
        set_style_or_remove(&container, "background-color", part.get_color_css(SIDE_BAR_BACKGROUND).as_deref());
        let border_color = part.get_color_css(SIDE_BAR_BORDER).or_else(|| part.get_color_css(CONTRAST_BORDER));
        let is_position_left = part.layout_service().get_side_bar_position() == Position::Right;

        set_style_or_remove(&container, "color", part.get_color_css(SIDE_BAR_FOREGROUND).as_deref());

        let has_border = border_color.is_some();
        set_style_or_remove(&container, "border-left-color", Some(border_color.as_deref().unwrap_or("")));
        set_style_or_remove(&container, "border-right-color", Some(border_color.as_deref().unwrap_or("")));

        set_style_or_remove(&container, "border-left-style", Some(if has_border && !is_position_left { "solid" } else { "none" }));
        set_style_or_remove(&container, "border-right-style", Some(if has_border && is_position_left { "solid" } else { "none" }));

        set_style_or_remove(&container, "border-left-width", Some(if has_border && !is_position_left { "1px" } else { "0px" }));
        set_style_or_remove(&container, "border-right-width", Some(if has_border && is_position_left { "1px" } else { "0px" }));
    }

    fn get_composite_bar_options(&self, part: &AbstractPaneCompositePart) -> PaneCompositeBarOptions {
        let configuration = self.configuration();
        let position_weak = part.weak();
        let colors_weak = part.weak();
        let fill_weak = part.weak();
        let command_service = self.command_service.clone();
        let configuration_cell = self.configuration.clone();
        let title = part.get_composite_bar_position() == CompositeBarPosition::Title;
        PaneCompositeBarOptions {
            part_container_class: "auxiliarybar".to_owned(),
            pinned_view_containers_key: PINNED_VIEWS_KEY.to_owned(),
            placeholder_view_containers_key: PLACEHOLDER_VIEW_CONTAINERS_KEY.to_owned(),
            view_containers_workspace_state_key: VIEW_CONTAINERS_WORKSPACE_STATE_KEY.to_owned(),
            icon: !configuration.show_labels,
            orientation: ActionsOrientation::Horizontal,
            icon_size: 16.0,
            prevent_loop_navigation: false,
            recompute_sizes: true,
            activity_hover_options: ActivityHoverOptions {
                position: Rc::new(move || {
                    let bottom = position_weak.upgrade().is_some_and(|part| part.get_composite_bar_position() == CompositeBarPosition::Bottom);
                    if bottom { HoverPosition::Above } else { HoverPosition::Below }
                }),
            },
            fill_extra_context_menu_actions: Rc::new(move |actions: &mut Vec<ActionRef>, _target: Option<ContextMenuTarget>| {
                if let Some(part) = fill_weak.upgrade() {
                    let configuration = *configuration_cell.borrow();
                    AuxiliaryBarBehavior::fill_extra_context_menu_actions(configuration, &command_service, &part, actions);
                }
            }),
            composite_size: 0.0,
            // Add 10px spacing if the overflow action is visible to no confuse the user with ... between the toolbars
            overflow_action_size: if title { 40.0 } else { 30.0 },
            colors: Rc::new(move |theme: &dyn IColorTheme| {
                let title = colors_weak.upgrade().is_none_or(|part| part.get_composite_bar_position() == CompositeBarPosition::Title);
                CompositeBarColors {
                    active_background_color: theme.get_color(SIDE_BAR_BACKGROUND, None),
                    inactive_background_color: theme.get_color(SIDE_BAR_BACKGROUND, None),
                    active_border_bottom_color: if title { theme.get_color(PANEL_ACTIVE_TITLE_BORDER, None) } else { theme.get_color(ACTIVITY_BAR_TOP_ACTIVE_BORDER, None) },
                    active_foreground_color: if title { theme.get_color(PANEL_ACTIVE_TITLE_FOREGROUND, None) } else { theme.get_color(ACTIVITY_BAR_TOP_FOREGROUND, None) },
                    inactive_foreground_color: if title {
                        theme.get_color(PANEL_INACTIVE_TITLE_FOREGROUND, None)
                    } else {
                        theme.get_color(ACTIVITY_BAR_TOP_INACTIVE_FOREGROUND, None)
                    },
                    badge_background: theme.get_color(ACTIVITY_BAR_BADGE_BACKGROUND, None),
                    badge_foreground: theme.get_color(ACTIVITY_BAR_BADGE_FOREGROUND, None),
                    drag_and_drop_border: if title { theme.get_color(PANEL_DRAG_AND_DROP_BORDER, None) } else { theme.get_color(ACTIVITY_BAR_TOP_DRAG_AND_DROP_BORDER, None) },
                    active_border_color: None,
                    active_background: None,
                }
            }),
            compact: true,
        }
    }

    fn should_show_composite_bar(&self, part: &AbstractPaneCompositePart) -> bool {
        let configuration = self.configuration();
        if configuration.position == Some(ActivityBarPosition::Hidden) {
            return false;
        }

        // Check if auto-hide is enabled and there's only one visible view container while the activity bar is configured to be top
        // or bottom.
        if is_top_or_bottom(configuration.position) {
            let auto_hide = matches!(part.configuration_service().get_value(Some(LayoutSettings::ActivityBarAutoHide.as_str()), None), Dynamic::Bool(true));
            if auto_hide {
                // Use visible composite count from the composite bar if available (considers pinned state), otherwise fall back to
                // the tracker's count (based on active view descriptors). Note: We access paneCompositeBar directly to avoid
                // circular calls with getVisiblePaneCompositeIds()
                let visible_count = self.visible_view_containers_tracker.borrow().as_ref().map_or(0, |tracker| tracker.visible_count());
                if visible_count <= 1 {
                    return false;
                }
            }
        }

        true
    }

    fn get_composite_bar_position(&self, _part: &AbstractPaneCompositePart) -> CompositeBarPosition {
        match self.configuration().position {
            Some(ActivityBarPosition::Top) => CompositeBarPosition::Top,
            Some(ActivityBarPosition::Bottom) => CompositeBarPosition::Bottom,
            Some(ActivityBarPosition::Hidden) | Some(ActivityBarPosition::Default) | None => CompositeBarPosition::Title,
        }
    }
}

/// `AuxiliaryBarPart`.
pub struct AuxiliaryBarPart {
    pane: Rc<AbstractPaneCompositePart>,
    base: Rc<Disposable>,
    weak_self: Weak<AuxiliaryBarPart>,
    configuration: Rc<RefCell<AuxiliaryBarPartConfiguration>>,
    services: AuxiliaryBarPartServices,
}

impl Deref for AuxiliaryBarPart {
    type Target = AbstractPaneCompositePart;

    fn deref(&self) -> &AbstractPaneCompositePart {
        &self.pane
    }
}

impl AuxiliaryBarPart {
    pub const ACTIVE_VIEW_SETTINGS_KEY: &'static str = ACTIVE_VIEW_SETTINGS_KEY;
    pub const PINNED_VIEWS_KEY: &'static str = PINNED_VIEWS_KEY;
    pub const PLACEHOLDE_VIEW_CONTAINERS_KEY: &'static str = PLACEHOLDER_VIEW_CONTAINERS_KEY;
    pub const VIEW_CONTAINERS_WORKSPACE_STATE_KEY: &'static str = VIEW_CONTAINERS_WORKSPACE_STATE_KEY;

    pub fn new(services: AuxiliaryBarPartServices) -> Rc<Self> {
        Self::extend(services, |behavior| behavior)
    }

    /// `class X extends AuxiliaryBarPart`: `wrap` recebe o [`AuxiliaryBarBehavior`] e devolve o comportamento que o sobrescreve.
    pub fn extend(services: AuxiliaryBarPartServices, wrap: impl FnOnce(Rc<AuxiliaryBarBehavior>) -> Rc<dyn PaneCompositePartBehavior>) -> Rc<Self> {
        let pane_services = services.pane.clone();
        let context_key_service = pane_services.context_key_service.clone();
        let configuration_service = pane_services.configuration_service.clone();

        let configuration = Rc::new(RefCell::new(Self::resolve_configuration(configuration_service.as_ref())));
        let behavior = AuxiliaryBarBehavior::new(configuration.clone(), services.command_service.clone());

        let slot: Rc<RefCell<Weak<AbstractPaneCompositePart>>> = Rc::new(RefCell::new(Weak::new()));
        let border_slot = slot.clone();
        let pane = AbstractPaneCompositePart::new(
            PaneCompositePartArgs {
                part_id: Parts::AuxiliarybarPart,
                part_options: PartOptions {
                    has_title: true,
                    border_width: Some(Rc::new(move || {
                        let has_border = border_slot
                            .borrow()
                            .upgrade()
                            .is_some_and(|part| part.get_color(SIDE_BAR_BORDER).is_some() || part.get_color(CONTRAST_BORDER).is_some());
                        if has_border { 1.0 } else { 0.0 }
                    })),
                    title_height: None,
                },
                trailing_separator: true,
                active_pane_composite_settings_key: ACTIVE_VIEW_SETTINGS_KEY.to_owned(),
                active_pane_context_key: active_auxiliary_context().bind_to(context_key_service.as_ref()),
                pane_focus_context_key: auxiliary_bar_focus_context().bind_to(context_key_service.as_ref()),
                name_for_telemetry: "auxiliarybar".to_owned(),
                composite_css_class: "auxiliarybar".to_owned(),
                title_foreground_color: None,
                title_border_color: Some(SIDE_BAR_TITLE_BORDER.to_owned()),
                location: ViewContainerLocation::AuxiliaryBar,
                registry_id: extensions::AUXILIARY.to_owned(),
                global_actions_menu_id: menu_ids::auxiliary_bar_title(),
            },
            pane_services.clone(),
            wrap(behavior.clone()),
        );
        *slot.borrow_mut() = Rc::downgrade(&pane);

        let base = Disposable::new();

        // Track visible view containers for auto-hide
        let tracker = VisibleViewContainersTracker::new(ViewContainerLocation::AuxiliaryBar, pane_services.view_descriptor_service.clone());
        base.register(tracker.clone());
        *behavior.visible_view_containers_tracker.borrow_mut() = Some(tracker.clone());

        let this = Rc::new_cyclic(|weak| AuxiliaryBarPart { pane, base, weak_self: weak.clone(), configuration, services });

        let weak = this.weak_self.clone();
        this.base.register(tracker.on_did_change().listen(move |change: &VisibleCountChange| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_auto_hide_view_containers(change);
            }
        }));

        let weak = this.weak_self.clone();
        this.base.register(configuration_service.on_did_change_configuration().listen(move |event: &Rc<ConfigurationChangeEvent>| {
            let Some(this) = weak.upgrade() else { return };
            if event.affects_configuration(LayoutSettings::ActivityBarLocation.as_str(), None) {
                this.reload_configuration();
                this.on_did_change_activity_bar_location();
            } else if event.affects_configuration(SHOW_LABELS_SETTING, None) {
                this.reload_configuration();
                this.pane.update_composite_bar(true);
            } else if event.affects_configuration(LayoutSettings::ActivityBarAutoHide.as_str(), None) {
                this.on_did_change_activity_bar_location();
            }
        }));

        this
    }

    fn reload_configuration(&self) {
        let configuration = Self::resolve_configuration(self.pane.configuration_service().as_ref());
        *self.configuration.borrow_mut() = configuration;
    }

    /// `resolveConfiguration()`.
    fn resolve_configuration(configuration_service: &dyn IConfigurationService) -> AuxiliaryBarPartConfiguration {
        let position = activity_bar_position_of(&configuration_service.get_value(Some(LayoutSettings::ActivityBarLocation.as_str()), None));

        let can_show_labels = !is_top_or_bottom(position); // use same style as activity bar in this case
        let show_labels = can_show_labels && !configuration_service.get_value(Some(SHOW_LABELS_SETTING), None).strict_equals(&Dynamic::Bool(false));

        AuxiliaryBarPartConfiguration { position, can_show_labels, show_labels }
    }

    /// `configuration`.
    pub fn configuration(&self) -> AuxiliaryBarPartConfiguration {
        *self.configuration.borrow()
    }

    fn on_did_change_auto_hide_view_containers(&self, change: &VisibleCountChange) {
        // Only update if auto-hide is enabled and composite bar would show
        let auto_hide = matches!(self.pane.configuration_service().get_value(Some(LayoutSettings::ActivityBarAutoHide.as_str()), None), Dynamic::Bool(true));
        if auto_hide && is_top_or_bottom(self.configuration().position) {
            let visible_before = change.before > 1;
            let visible_after = change.after > 1;
            if visible_before != visible_after {
                self.on_did_change_activity_bar_location();
            }
        }
    }

    fn on_did_change_activity_bar_location(&self) {
        self.pane.update_composite_bar(false);

        if let Some(id) = self.pane.get_active_composite_id() {
            self.pane.on_title_area_update(&id);
        }
    }

    /// `preferredHeight`: don't worry about titlebar or statusbar visibility, the difference is minimal and keeps this function
    /// clean.
    pub fn preferred_height(&self) -> f64 {
        self.pane.layout_service().main_container_dimension().height * 0.4
    }

    /// `preferredWidth`.
    pub fn preferred_width(&self) -> Option<f64> {
        let active_composite = self.pane.get_active_pane_composite()?;

        let width = active_composite.get_optimal_width()?;
        Some(width.max(300.0))
    }

    /// `toJSON()`.
    pub fn to_json(&self) -> Dynamic {
        Dynamic::object([("type", Dynamic::from(Parts::AuxiliarybarPart.as_str()))])
    }

    /// Os serviços com que a parte foi criada.
    pub fn services(&self) -> &AuxiliaryBarPartServices {
        &self.services
    }
}

impl IDisposable for AuxiliaryBarPart {
    fn dispose(&self) {
        self.base.dispose();
        self.pane.dispose();
    }
}
