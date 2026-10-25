//! Porte de `src/vs/workbench/browser/parts/panel/panelPart.ts` (e de `media/panelpart.css`): a [`PanelPart`], o painel (terminal,
//! problemas, saída...): os limites, o `preferredHeight` e o `preferredWidth`, os estilos, a barra de composites no título com
//! as ações de posição, alinhamento e rótulos, e o layout que desconta a borda de 1px do painel à direita ou no topo.
//!
//! Divergências:
//! - **Herança vira composição.** `PanelPart extends AbstractPaneCompositePart` é a [`PanelPart`], que contém a
//!   [`AbstractPaneCompositePart`] (`Deref`) e se apresenta a ela como o [`PaneCompositePartBehavior`] [`PanelBehavior`]. Uma
//!   subclasse do `PanelPart` (que sobrescreve um membro `protected`) é um [`PaneCompositePartBehavior`] que repassa ao
//!   [`PanelBehavior`] e que entra por [`PanelPart::extend`].
//! - **`TogglePanelAction.ID`** (`panelActions.ts`, ainda não portado) é [`TOGGLE_PANEL_ACTION_ID`].
//! - **`container.style.<propriedade>`** são as propriedades CSS do `ViewElement`, com o valor normalizado do CSSOM (`rgb(...)`); o
//!   valor vazio remove a propriedade.
//! - **`toAction({ run })`** é o `to_action` com a execução do comando ou a atualização da configuração; o retorno da promessa
//!   não é aguardado, como no original.

use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, Separator, SubmenuAction, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::promise::MaybePromise;
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::theme_service::IColorTheme;
use je_platform::actions::menu_entry_action_view_item::get_context_menu_actions;
use je_platform::actions::{MenuActionOptions, menu_ids};
use je_platform::commands::ICommandService;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_widgets::action_bar::ActionsOrientation;
use je_widgets::hover_api::HoverPosition;

use crate::browser::pane_composite::extensions;
use crate::browser::part::PartOptions;
use crate::browser::parts::pane_composite_bar::{ActivityHoverOptions, CompositeBarColors, ContextMenuTarget, PaneCompositeBarOptions};
use crate::browser::parts::pane_composite_part::{
    AbstractPaneCompositePart, CompositeBarPosition, PaneCompositePartArgs, PaneCompositePartBehavior, PaneCompositePartServices,
};
use crate::browser::parts::part_support::set_style_or_remove;
use crate::common::contextkeys::{active_panel_context, panel_focus_context};
use crate::common::views::ViewContainerLocation;
use crate::services::layout::layout_service::{Parts, Position};

// As cores de `workbench/common/theme.ts` e de `colorRegistry.ts` que a parte lê.
pub const PANEL_BACKGROUND: &str = "panel.background";
pub const PANEL_BORDER: &str = "panel.border";
pub const PANEL_TITLE_BORDER: &str = "panelTitle.border";
pub const PANEL_ACTIVE_TITLE_FOREGROUND: &str = "panelTitle.activeForeground";
pub const PANEL_INACTIVE_TITLE_FOREGROUND: &str = "panelTitle.inactiveForeground";
pub const PANEL_ACTIVE_TITLE_BORDER: &str = "panelTitle.activeBorder";
pub const PANEL_DRAG_AND_DROP_BORDER: &str = "panel.dropBorder";
pub const PANEL_TITLE_BADGE_BACKGROUND: &str = "panelTitleBadge.background";
pub const PANEL_TITLE_BADGE_FOREGROUND: &str = "panelTitleBadge.foreground";
pub const CONTRAST_BORDER: &str = "contrastBorder";

/// `TogglePanelAction.ID` (`panelActions.ts`).
pub const TOGGLE_PANEL_ACTION_ID: &str = "workbench.action.togglePanel";

/// `PanelPart.activePanelSettingsKey`.
pub const ACTIVE_PANEL_SETTINGS_KEY: &str = "workbench.panelpart.activepanelid";

/// `workbench.panel.showLabels`.
const SHOW_LABELS_SETTING: &str = "workbench.panel.showLabels";

/// Os serviços que o construtor da `PanelPart` injeta além dos da base.
#[derive(Clone)]
pub struct PanelPartServices {
    pub pane: PaneCompositePartServices,
    pub command_service: Rc<dyn ICommandService>,
}

/// `toAction({ id, label, run })`.
fn action_of(id: &str, label: String, run: impl Fn() + 'static) -> ActionRef {
    to_action(ToActionProps {
        id: id.to_owned(),
        label,
        tooltip: None,
        enabled: None,
        checked: None,
        class: None,
        alt: None,
        run: Rc::new(move |_| {
            run();
            MaybePromise::Value(Dynamic::Undefined)
        }),
    })
}

/// O [`PaneCompositePartBehavior`] do painel.
pub struct PanelBehavior {
    command_service: Rc<dyn ICommandService>,
}

impl PanelBehavior {
    pub fn new(command_service: Rc<dyn ICommandService>) -> Rc<Self> {
        Rc::new(Self { command_service })
    }

    /// `fillExtraContextMenuActions(actions)`.
    fn fill_extra_context_menu_actions(command_service: &Rc<dyn ICommandService>, part: &AbstractPaneCompositePart, actions: &mut Vec<ActionRef>) {
        if part.get_composite_bar_position() == CompositeBarPosition::Title
            && let Some(views_submenu_action) = part.get_views_submenu_action()
        {
            actions.push(Separator::new());
            actions.push(views_submenu_action);
        }

        let options = MenuActionOptions { arg: None, args: None, should_forward_args: true, render_short_title: false };
        let panel_position_menu = part.menu_service().get_menu_actions(&menu_ids::panel_position_menu(), part.context_key_service().as_ref(), Some(&options));
        let panel_align_menu = part.menu_service().get_menu_actions(&menu_ids::panel_alignment_menu(), part.context_key_service().as_ref(), Some(&options));
        let position_actions = get_context_menu_actions(&panel_position_menu, None).secondary;
        let align_actions = get_context_menu_actions(&panel_align_menu, None).secondary;

        let panel_show_labels = matches!(part.configuration_service().get_value(Some(SHOW_LABELS_SETTING), None), Dynamic::Bool(true));
        let configuration_service = part.configuration_service().clone();
        let toggle_show_labels_action = action_of(
            "workbench.action.panel.toggleShowLabels",
            if panel_show_labels { localize("showIcons", "Show Icons", &[]) } else { localize("showLabels", "Show Labels", &[]) },
            move || {
                let _ = configuration_service.update_value(SHOW_LABELS_SETTING, Dynamic::Bool(!panel_show_labels), None, None);
            },
        );

        let command_service = command_service.clone();
        actions.push(Separator::new());
        let position_submenu: ActionRef = SubmenuAction::new("workbench.action.panel.position", &localize("panel position", "Panel Position", &[]), position_actions, None);
        actions.push(position_submenu);
        let align_submenu: ActionRef = SubmenuAction::new("workbench.action.panel.align", &localize("align panel", "Align Panel", &[]), align_actions, None);
        actions.push(align_submenu);
        actions.push(toggle_show_labels_action);
        actions.push(action_of(TOGGLE_PANEL_ACTION_ID, localize("hidePanel", "Hide Panel", &[]), move || {
            let _ = command_service.execute_command(TOGGLE_PANEL_ACTION_ID, Vec::new());
        }));
    }
}

impl PaneCompositePartBehavior for PanelBehavior {
    fn minimum_width(&self) -> f64 {
        300.0
    }

    fn maximum_width(&self) -> f64 {
        f64::INFINITY
    }

    fn minimum_height(&self) -> f64 {
        77.0
    }

    fn maximum_height(&self) -> f64 {
        f64::INFINITY
    }

    fn to_json(&self) -> Dynamic {
        Dynamic::object([("type", Dynamic::from(Parts::PanelPart.as_str()))])
    }

    fn update_styles(&self, part: &AbstractPaneCompositePart) {
        part.update_styles_base();

        let Some(container) = part.get_container() else { return };
        set_style_or_remove(&container, "background-color", part.get_color_css(PANEL_BACKGROUND).as_deref());
        let border_color = part.get_color_css(PANEL_BORDER).or_else(|| part.get_color_css(CONTRAST_BORDER)).unwrap_or_default();
        set_style_or_remove(&container, "border-left-color", Some(border_color.as_str()));
        set_style_or_remove(&container, "border-right-color", Some(border_color.as_str()));
        set_style_or_remove(&container, "border-bottom-color", Some(border_color.as_str()));

        if let Some(title_area) = part.title_area() {
            set_style_or_remove(&title_area, "border-top-color", Some(border_color.as_str()));
        }
    }

    fn layout(&self, part: &AbstractPaneCompositePart, width: f64, height: f64, top: f64, left: f64) {
        let mut dimensions = Dimension { width, height };
        if !part.layout_service().is_floating_panels_enabled() {
            match part.layout_service().get_panel_position() {
                Position::Right => dimensions = Dimension { width: width - 1.0, height }, // Take into account the 1px border when layouting
                Position::Top => dimensions = Dimension { width, height: height - 1.0 }, // Take into account the 1px border when layouting
                _ => {}
            }
        }

        // Layout contents
        part.layout_base(dimensions.width, dimensions.height, top, left);
    }

    fn get_composite_bar_options(&self, part: &AbstractPaneCompositePart) -> PaneCompositeBarOptions {
        let layout_service = part.layout_service().clone();
        let weak = part.weak();
        let show_labels_false = part.configuration_service().get_value(Some(SHOW_LABELS_SETTING), None).strict_equals(&Dynamic::Bool(false));
        PaneCompositeBarOptions {
            part_container_class: "panel".to_owned(),
            pinned_view_containers_key: "workbench.panel.pinnedPanels".to_owned(),
            placeholder_view_containers_key: "workbench.panel.placeholderPanels".to_owned(),
            view_containers_workspace_state_key: "workbench.panel.viewContainersWorkspaceState".to_owned(),
            icon: show_labels_false,
            orientation: ActionsOrientation::Horizontal,
            icon_size: 16.0,
            prevent_loop_navigation: false,
            recompute_sizes: true,
            activity_hover_options: ActivityHoverOptions {
                position: Rc::new(move || {
                    if layout_service.get_panel_position() == Position::Bottom && !layout_service.is_panel_maximized() { HoverPosition::Above } else { HoverPosition::Below }
                }),
            },
            fill_extra_context_menu_actions: {
                let command_service = self.command_service.clone();
                Rc::new(move |actions: &mut Vec<ActionRef>, _target: Option<ContextMenuTarget>| {
                    if let Some(part) = weak.upgrade() {
                        PanelBehavior::fill_extra_context_menu_actions(&command_service, &part, actions);
                    }
                })
            },
            composite_size: 0.0,
            // Only applies to icons, not labels
            compact: true,
            overflow_action_size: 44.0,
            colors: Rc::new(|theme: &dyn IColorTheme| CompositeBarColors {
                // Background color for overflow action
                active_background_color: theme.get_color(PANEL_BACKGROUND, None),
                inactive_background_color: theme.get_color(PANEL_BACKGROUND, None),
                active_border_bottom_color: theme.get_color(PANEL_ACTIVE_TITLE_BORDER, None),
                active_foreground_color: theme.get_color(PANEL_ACTIVE_TITLE_FOREGROUND, None),
                inactive_foreground_color: theme.get_color(PANEL_INACTIVE_TITLE_FOREGROUND, None),
                badge_background: theme.get_color(PANEL_TITLE_BADGE_BACKGROUND, None),
                badge_foreground: theme.get_color(PANEL_TITLE_BADGE_FOREGROUND, None),
                drag_and_drop_border: theme.get_color(PANEL_DRAG_AND_DROP_BORDER, None),
                active_border_color: None,
                active_background: None,
            }),
        }
    }

    fn should_show_composite_bar(&self, _part: &AbstractPaneCompositePart) -> bool {
        true
    }

    fn get_composite_bar_position(&self, _part: &AbstractPaneCompositePart) -> CompositeBarPosition {
        CompositeBarPosition::Title
    }
}

/// `PanelPart`.
pub struct PanelPart {
    pane: Rc<AbstractPaneCompositePart>,
    base: Rc<Disposable>,
    weak_self: Weak<PanelPart>,
}

impl Deref for PanelPart {
    type Target = AbstractPaneCompositePart;

    fn deref(&self) -> &AbstractPaneCompositePart {
        &self.pane
    }
}

impl PanelPart {
    pub const ACTIVE_PANEL_SETTINGS_KEY: &'static str = ACTIVE_PANEL_SETTINGS_KEY;

    pub fn new(services: PanelPartServices) -> Rc<Self> {
        Self::extend(services, |behavior| behavior)
    }

    /// `class X extends PanelPart`: `wrap` recebe o [`PanelBehavior`] e devolve o comportamento que o sobrescreve.
    pub fn extend(services: PanelPartServices, wrap: impl FnOnce(Rc<PanelBehavior>) -> Rc<dyn PaneCompositePartBehavior>) -> Rc<Self> {
        let pane_services = services.pane.clone();
        let context_key_service = pane_services.context_key_service.clone();
        let configuration_service = pane_services.configuration_service.clone();

        let behavior = wrap(PanelBehavior::new(services.command_service.clone()));
        let pane = AbstractPaneCompositePart::new(
            PaneCompositePartArgs {
                part_id: Parts::PanelPart,
                part_options: PartOptions { has_title: true, border_width: None, title_height: None },
                trailing_separator: true,
                active_pane_composite_settings_key: ACTIVE_PANEL_SETTINGS_KEY.to_owned(),
                active_pane_context_key: active_panel_context().bind_to(context_key_service.as_ref()),
                pane_focus_context_key: panel_focus_context().bind_to(context_key_service.as_ref()),
                name_for_telemetry: "panel".to_owned(),
                composite_css_class: "panel".to_owned(),
                title_foreground_color: None,
                title_border_color: Some(PANEL_TITLE_BORDER.to_owned()),
                location: ViewContainerLocation::Panel,
                registry_id: extensions::PANELS.to_owned(),
                global_actions_menu_id: menu_ids::panel_title(),
            },
            pane_services,
            behavior,
        );

        let this = Rc::new_cyclic(|weak| PanelPart { pane, base: Disposable::new(), weak_self: weak.clone() });

        let weak = this.weak_self.clone();
        this.base.register(configuration_service.on_did_change_configuration().listen(move |event: &Rc<ConfigurationChangeEvent>| {
            if event.affects_configuration(SHOW_LABELS_SETTING, None)
                && let Some(this) = weak.upgrade()
            {
                this.pane.update_composite_bar(true);
            }
        }));

        this
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
        Dynamic::object([("type", Dynamic::from(Parts::PanelPart.as_str()))])
    }
}

impl IDisposable for PanelPart {
    fn dispose(&self) {
        self.base.dispose();
        self.pane.dispose();
    }
}
