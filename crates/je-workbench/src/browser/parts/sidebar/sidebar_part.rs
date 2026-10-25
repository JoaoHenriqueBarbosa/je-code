//! Porte de `src/vs/workbench/browser/parts/sidebar/sidebarPart.ts` (e de `media/sidebarpart.css`): a [`SidebarPart`], a barra
//! lateral primária: os limites, o `preferredWidth`, os estilos, a barra de atividades (`ActivitybarPart`) ou a barra de composites
//! no topo ou no rodapé, a classe do sash interno e a ação de alternar a barra de atividades.
//!
//! Divergências:
//! - **Herança vira composição.** `SidebarPart extends AbstractPaneCompositePart` é a [`SidebarPart`], que contém a
//!   [`AbstractPaneCompositePart`] (`Deref`) e se apresenta a ela como o [`PaneCompositePartBehavior`] [`SidebarBehavior`].
//! - **`this.instantiationService.createInstance(ActivitybarPart, this.location, this)`** é o [`ActivitybarPart::new`], criado
//!   depois da parte base (precisa dela) e guardado no estado que o [`SidebarBehavior`] lê; o `createInstance(ActivityBarCompositeBar,
//!   ...)` do `createCompositeBar` é a [`CompositeBarFactory`] dos serviços (que escolhe a barra pela `part` do pedido).
//! - **`VisibleViewContainersTracker`** é o [`VisibleViewContainersTracker`] criado com o `IViewDescriptorService`.
//! - **`registerAction2(class extends Action2)`** é a [`ToggleActivityBarVisibilityAction`], que alcança a parte por uma referência
//!   fraca (o `that` do original).
//! - **`container.style.<propriedade>`** são as propriedades CSS do `ViewElement`, com o valor normalizado do CSSOM (`rgb(...)`);
//!   o valor vazio remove a propriedade.
//! - **`IBoundarySashes.right?.addClass(...)`** é o `Sash::add_class`, e o `MutableDisposable` guarda o descarte da classe.
//! - **`getRememberedActivityBarVisiblePosition`/`focusActivityBar`** (`async`) devolvem a [`Promise`] do original.

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::actions::{ActionRef, Separator};
use je_base::dynamic::Dynamic;
use je_base::layout::AnchorAlignment;
use je_base::lifecycle::{Disposable, IDisposable, MutableDisposable};
use je_base::promise::Promise;
use je_editor::services::theme_service::IColorTheme;
use je_instantiation::ServicesAccessor;
use je_platform::action::{CommandAction, CommandActionTitle, Title, localize2};
use je_platform::actions::actions::{Action2, Action2Options, register_action2};
use je_platform::actions::menu_ids;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::storage::{StorageScope, StorageTarget};
use je_storage::StorageValue;
use je_widgets::action_bar::ActionsOrientation;
use je_widgets::hover_api::HoverPosition;
use je_widgets::sash::BoundarySashes;

use crate::browser::pane_composite::extensions;
use crate::browser::part::PartOptions;
use crate::browser::parts::activitybar::activitybar_part::ActivitybarPart;
use crate::browser::parts::pane_composite_bar::{ActivityHoverOptions, CompositeBarColors, ContextMenuTarget, PaneCompositeBarOptions};
use crate::browser::parts::pane_composite_part::{
    AbstractPaneCompositePart, CompositeBarPosition, PaneCompositePartArgs, PaneCompositePartBehavior, PaneCompositePartServices,
};
use crate::browser::parts::part_support::set_style_or_remove;
use crate::browser::parts::visible_view_containers_tracker::{VisibleCountChange, VisibleViewContainersTracker};
use crate::common::contextkeys::{active_viewlet_context, sidebar_focus_context};
use crate::common::views::ViewContainerLocation;
use crate::services::host::host::IHostService;
use crate::services::layout::layout_service::{ActivityBarPosition, LayoutSettings, Parts, Position};

// As cores de `workbench/common/theme.ts` e de `colorRegistry.ts` que a parte lê.
pub const SIDE_BAR_TITLE_FOREGROUND: &str = "sideBarTitle.foreground";
pub const SIDE_BAR_TITLE_BORDER: &str = "sideBarTitle.border";
pub const SIDE_BAR_BACKGROUND: &str = "sideBar.background";
pub const SIDE_BAR_FOREGROUND: &str = "sideBar.foreground";
pub const SIDE_BAR_BORDER: &str = "sideBar.border";
pub const SIDE_BAR_DRAG_AND_DROP_BACKGROUND: &str = "sideBar.dropBackground";
pub const ACTIVITY_BAR_BADGE_BACKGROUND: &str = "activityBarBadge.background";
pub const ACTIVITY_BAR_BADGE_FOREGROUND: &str = "activityBarBadge.foreground";
pub const ACTIVITY_BAR_TOP_FOREGROUND: &str = "activityBarTop.foreground";
pub const ACTIVITY_BAR_TOP_ACTIVE_BORDER: &str = "activityBarTop.activeBorder";
pub const ACTIVITY_BAR_TOP_INACTIVE_FOREGROUND: &str = "activityBarTop.inactiveForeground";
pub const ACTIVITY_BAR_TOP_DRAG_AND_DROP_BORDER: &str = "activityBarTop.dropBorder";
pub const CONTRAST_BORDER: &str = "contrastBorder";

/// `ToggleActivityBarVisibilityActionId` (`actions/layoutActions.ts`).
pub const TOGGLE_ACTIVITY_BAR_VISIBILITY_ACTION_ID: &str = "workbench.action.toggleActivityBarVisibility";

const PRIMARY_SIDE_BAR_SASH_CLASS: &str = "primary-sidebar-sash";

/// `SidebarPart.activeViewletSettingsKey`.
pub const ACTIVE_VIEWLET_SETTINGS_KEY: &str = "workbench.sidebar.activeviewletid";

/// Os serviços que o construtor da `SidebarPart` injeta.
#[derive(Clone)]
pub struct SidebarPartServices {
    pub pane: PaneCompositePartServices,
    pub host_service: Rc<dyn IHostService>,
}

/// O estado da subclasse que a parte base cria depois: a barra de atividades e o rastreador de contêineres visíveis.
#[derive(Default)]
struct SidebarShared {
    activity_bar_part: RefCell<Option<Rc<ActivitybarPart>>>,
    visible_view_containers_tracker: RefCell<Option<Rc<VisibleViewContainersTracker>>>,
}

/// O [`PaneCompositePartBehavior`] da barra lateral.
pub struct SidebarBehavior {
    shared: Rc<SidebarShared>,
    primary_side_bar_sash_class_disposable: Rc<MutableDisposable<dyn IDisposable>>,
}

impl SidebarBehavior {
    fn activity_bar_part(&self) -> Option<Rc<ActivitybarPart>> {
        self.shared.activity_bar_part.borrow().clone()
    }
}

impl PaneCompositePartBehavior for SidebarBehavior {
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
        Dynamic::object([("type", Dynamic::from(Parts::SidebarPart.as_str()))])
    }

    fn snap(&self, _part: &AbstractPaneCompositePart) -> bool {
        true
    }

    fn update_styles(&self, part: &AbstractPaneCompositePart) {
        part.update_styles_base();

        let Some(container) = part.get_container() else { return };

        set_style_or_remove(&container, "background-color", part.get_color_css(SIDE_BAR_BACKGROUND).as_deref());
        set_style_or_remove(&container, "color", part.get_color_css(SIDE_BAR_FOREGROUND).as_deref());

        let border_color = part.get_color_css(SIDE_BAR_BORDER).or_else(|| part.get_color_css(CONTRAST_BORDER));
        let is_position_left = part.layout_service().get_side_bar_position() == Position::Left;
        let has_border = border_color.is_some();
        set_style_or_remove(&container, "border-right-width", Some("1px").filter(|_| has_border && is_position_left));
        set_style_or_remove(&container, "border-right-style", Some("solid").filter(|_| has_border && is_position_left));
        set_style_or_remove(&container, "border-right-color", border_color.as_deref().filter(|_| is_position_left));
        set_style_or_remove(&container, "border-left-width", Some("1px").filter(|_| has_border && !is_position_left));
        set_style_or_remove(&container, "border-left-style", Some("solid").filter(|_| has_border && !is_position_left));
        set_style_or_remove(&container, "border-left-color", border_color.as_deref().filter(|_| !is_position_left));
        set_style_or_remove(&container, "outline-color", part.get_color_css(SIDE_BAR_DRAG_AND_DROP_BACKGROUND).as_deref());
    }

    fn layout(&self, part: &AbstractPaneCompositePart, width: f64, height: f64, top: f64, left: f64) {
        if !part.layout_service().is_visible(Parts::SidebarPart, None) {
            return;
        }

        part.layout_base(width, height, top, left);
    }

    fn set_boundary_sashes(&self, part: &AbstractPaneCompositePart, sashes: &BoundarySashes) {
        part.set_boundary_sashes_base(sashes);

        self.primary_side_bar_sash_class_disposable.clear();
        let primary_side_bar_sash = if part.layout_service().get_side_bar_position() == Position::Left { sashes.right.clone() } else { sashes.left.clone() };
        self.primary_side_bar_sash_class_disposable.set(primary_side_bar_sash.map(|sash| sash.add_class(PRIMARY_SIDE_BAR_SASH_CLASS)));
    }

    fn get_title_area_drop_down_anchor_alignment(&self, part: &AbstractPaneCompositePart) -> AnchorAlignment {
        if part.layout_service().get_side_bar_position() == Position::Left { AnchorAlignment::Left } else { AnchorAlignment::Right }
    }

    fn get_pinned_pane_composite_ids(&self, part: &AbstractPaneCompositePart) -> Vec<String> {
        match (part.should_show_composite_bar(), self.activity_bar_part()) {
            (false, Some(activity_bar_part)) => activity_bar_part.get_pinned_pane_composite_ids(),
            _ => part.get_pinned_pane_composite_ids_base(),
        }
    }

    fn get_visible_pane_composite_ids(&self, part: &AbstractPaneCompositePart) -> Vec<String> {
        match (part.should_show_composite_bar(), self.activity_bar_part()) {
            (false, Some(activity_bar_part)) => activity_bar_part.get_visible_pane_composite_ids(),
            _ => part.get_visible_pane_composite_ids_base(),
        }
    }

    fn get_pane_composite_ids(&self, part: &AbstractPaneCompositePart) -> Vec<String> {
        match (part.should_show_composite_bar(), self.activity_bar_part()) {
            (false, Some(activity_bar_part)) => activity_bar_part.get_pane_composite_ids(),
            _ => part.get_pane_composite_ids_base(),
        }
    }

    fn get_composite_bar_options(&self, part: &AbstractPaneCompositePart) -> PaneCompositeBarOptions {
        let weak = part.weak();
        let position_weak = weak.clone();
        let fill_weak = weak.clone();
        PaneCompositeBarOptions {
            part_container_class: "sidebar".to_owned(),
            pinned_view_containers_key: ActivitybarPart::PINNED_VIEW_CONTAINERS_KEY.to_owned(),
            placeholder_view_containers_key: ActivitybarPart::PLACEHOLDER_VIEW_CONTAINERS_KEY.to_owned(),
            view_containers_workspace_state_key: ActivitybarPart::VIEW_CONTAINERS_WORKSPACE_STATE_KEY.to_owned(),
            icon: true,
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
                let Some(part) = fill_weak.upgrade() else { return };
                if part.get_composite_bar_position() == CompositeBarPosition::Title
                    && let Some(views_submenu_action) = part.get_views_submenu_action()
                {
                    actions.push(Separator::new());
                    actions.push(views_submenu_action);
                }
            }),
            composite_size: 0.0,
            overflow_action_size: 30.0,
            colors: Rc::new(|theme: &dyn IColorTheme| CompositeBarColors {
                active_background_color: theme.get_color(SIDE_BAR_BACKGROUND, None),
                inactive_background_color: theme.get_color(SIDE_BAR_BACKGROUND, None),
                active_border_bottom_color: theme.get_color(ACTIVITY_BAR_TOP_ACTIVE_BORDER, None),
                active_foreground_color: theme.get_color(ACTIVITY_BAR_TOP_FOREGROUND, None),
                inactive_foreground_color: theme.get_color(ACTIVITY_BAR_TOP_INACTIVE_FOREGROUND, None),
                badge_background: theme.get_color(ACTIVITY_BAR_BADGE_BACKGROUND, None),
                badge_foreground: theme.get_color(ACTIVITY_BAR_BADGE_FOREGROUND, None),
                drag_and_drop_border: theme.get_color(ACTIVITY_BAR_TOP_DRAG_AND_DROP_BORDER, None),
                active_border_color: None,
                active_background: None,
            }),
            compact: true,
        }
    }

    fn should_show_composite_bar(&self, part: &AbstractPaneCompositePart) -> bool {
        let configuration_service = part.configuration_service();
        let activity_bar_position = configuration_service.get_value(Some(LayoutSettings::ActivityBarLocation.as_str()), None);
        let is_top_or_bottom = activity_bar_position.strict_equals(&Dynamic::from(ActivityBarPosition::Top.as_str()))
            || activity_bar_position.strict_equals(&Dynamic::from(ActivityBarPosition::Bottom.as_str()));
        if !is_top_or_bottom {
            return false;
        }

        // Check if auto-hide is enabled and there's only one visible view container
        let auto_hide = matches!(configuration_service.get_value(Some(LayoutSettings::ActivityBarAutoHide.as_str()), None), Dynamic::Bool(true));
        if auto_hide {
            // Use visible composite count from the composite bar if available (considers pinned state), otherwise fall back to the
            // tracker's count (based on active view descriptors). Note: We access paneCompositeBar directly to avoid circular
            // calls with getVisiblePaneCompositeIds()
            let visible_count = self.shared.visible_view_containers_tracker.borrow().as_ref().map_or(0, |tracker| tracker.visible_count());
            if visible_count <= 1 {
                return false;
            }
        }

        true
    }

    fn get_composite_bar_position(&self, part: &AbstractPaneCompositePart) -> CompositeBarPosition {
        let activity_bar_position = part.configuration_service().get_value(Some(LayoutSettings::ActivityBarLocation.as_str()), None);
        if activity_bar_position.strict_equals(&Dynamic::from(ActivityBarPosition::Top.as_str())) {
            CompositeBarPosition::Top
        } else if activity_bar_position.strict_equals(&Dynamic::from(ActivityBarPosition::Bottom.as_str())) {
            CompositeBarPosition::Bottom
        } else {
            CompositeBarPosition::Title
        }
    }
}

/// `SidebarPart`.
pub struct SidebarPart {
    pane: Rc<AbstractPaneCompositePart>,
    base: Rc<Disposable>,
    weak_self: Weak<SidebarPart>,
    shared: Rc<SidebarShared>,
    services: SidebarPartServices,
}

impl Deref for SidebarPart {
    type Target = AbstractPaneCompositePart;

    fn deref(&self) -> &AbstractPaneCompositePart {
        &self.pane
    }
}

impl SidebarPart {
    pub const ACTIVE_VIEWLET_SETTINGS_KEY: &'static str = ACTIVE_VIEWLET_SETTINGS_KEY;

    pub fn new(services: SidebarPartServices) -> Rc<Self> {
        let pane_services = services.pane.clone();
        let context_key_service = pane_services.context_key_service.clone();
        let configuration_service = pane_services.configuration_service.clone();

        let shared = Rc::new(SidebarShared::default());
        let slot: Rc<RefCell<Weak<AbstractPaneCompositePart>>> = Rc::new(RefCell::new(Weak::new()));
        let base = Disposable::new();
        let primary_side_bar_sash_class_disposable = MutableDisposable::<dyn IDisposable>::new();
        base.register(primary_side_bar_sash_class_disposable.clone());

        let border_slot = slot.clone();
        let behavior = Rc::new(SidebarBehavior { shared: shared.clone(), primary_side_bar_sash_class_disposable });
        let pane = AbstractPaneCompositePart::new(
            PaneCompositePartArgs {
                part_id: Parts::SidebarPart,
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
                trailing_separator: false,
                active_pane_composite_settings_key: ACTIVE_VIEWLET_SETTINGS_KEY.to_owned(),
                active_pane_context_key: active_viewlet_context().bind_to(context_key_service.as_ref()),
                pane_focus_context_key: sidebar_focus_context().bind_to(context_key_service.as_ref()),
                name_for_telemetry: "sideBar".to_owned(),
                composite_css_class: "viewlet".to_owned(),
                title_foreground_color: Some(SIDE_BAR_TITLE_FOREGROUND.to_owned()),
                title_border_color: Some(SIDE_BAR_TITLE_BORDER.to_owned()),
                location: ViewContainerLocation::Sidebar,
                registry_id: extensions::VIEWLETS.to_owned(),
                global_actions_menu_id: menu_ids::sidebar_title(),
            },
            pane_services.clone(),
            behavior,
        );
        *slot.borrow_mut() = Rc::downgrade(&pane);

        let activity_bar_part = ActivitybarPart::new(
            ViewContainerLocation::Sidebar,
            pane.as_pane_composite_part(),
            pane_services.composite_bar_factory.clone(),
            pane_services.composite.layout_service.clone(),
            pane_services.composite.theme_service.clone(),
            configuration_service.clone(),
            services.host_service.clone(),
        );
        base.register(activity_bar_part.clone());
        *shared.activity_bar_part.borrow_mut() = Some(activity_bar_part);

        // Track visible view containers for auto-hide
        let tracker = VisibleViewContainersTracker::new(ViewContainerLocation::Sidebar, pane_services.view_descriptor_service.clone());
        base.register(tracker.clone());
        *shared.visible_view_containers_tracker.borrow_mut() = Some(tracker.clone());

        let this = Rc::new_cyclic(|weak| SidebarPart { pane, base, weak_self: weak.clone(), shared, services });

        let weak = this.weak_self.clone();
        this.base.register(tracker.on_did_change().listen(move |change: &VisibleCountChange| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_auto_hide_view_containers(change);
            }
        }));

        this.remember_activity_bar_visible_position();
        let weak = this.weak_self.clone();
        this.base.register(configuration_service.on_did_change_configuration().listen(move |event: &Rc<ConfigurationChangeEvent>| {
            let Some(this) = weak.upgrade() else { return };
            if event.affects_configuration(LayoutSettings::ActivityBarLocation.as_str(), None) {
                this.on_did_change_activity_bar_location();
            }
            if event.affects_configuration(LayoutSettings::ActivityBarAutoHide.as_str(), None) {
                this.on_did_change_activity_bar_location();
            }
        }));

        this.register_actions();
        this
    }

    fn activity_bar_part(&self) -> Rc<ActivitybarPart> {
        crate::browser::part::assert_returns_defined(self.shared.activity_bar_part.borrow().clone())
    }

    /// `preferredWidth`.
    pub fn preferred_width(&self) -> Option<f64> {
        let viewlet = self.pane.get_active_pane_composite()?;

        let width = viewlet.get_optimal_width()?;
        Some(width.max(300.0))
    }

    fn on_did_change_auto_hide_view_containers(&self, change: &VisibleCountChange) {
        // Only update if auto-hide is enabled and composite bar position is top/bottom
        let configuration_service = self.pane.configuration_service();
        let activity_bar_position = configuration_service.get_value(Some(LayoutSettings::ActivityBarLocation.as_str()), None);
        let auto_hide = matches!(configuration_service.get_value(Some(LayoutSettings::ActivityBarAutoHide.as_str()), None), Dynamic::Bool(true));
        if auto_hide
            && (activity_bar_position.strict_equals(&Dynamic::from(ActivityBarPosition::Top.as_str()))
                || activity_bar_position.strict_equals(&Dynamic::from(ActivityBarPosition::Bottom.as_str())))
        {
            let visible_before = change.before > 1;
            let visible_after = change.after > 1;
            if visible_before != visible_after {
                self.on_did_change_activity_bar_location();
            }
        }
    }

    fn on_did_change_activity_bar_location(&self) {
        self.activity_bar_part().hide();

        self.pane.update_composite_bar(false);

        if let Some(id) = self.pane.get_active_composite_id() {
            self.pane.on_title_area_update(&id);
        }

        if self.should_show_activity_bar() {
            self.activity_bar_part().show(false);
        }

        self.remember_activity_bar_visible_position();
    }

    fn should_show_activity_bar(&self) -> bool {
        if self.pane.should_show_composite_bar() {
            return false;
        }

        !self.pane.configuration_service().get_value(Some(LayoutSettings::ActivityBarLocation.as_str()), None).strict_equals(&Dynamic::from(ActivityBarPosition::Hidden.as_str()))
    }

    fn remember_activity_bar_visible_position(&self) {
        let activity_bar_position = self.pane.configuration_service().get_value(Some(LayoutSettings::ActivityBarLocation.as_str()), None);
        if !activity_bar_position.strict_equals(&Dynamic::from(ActivityBarPosition::Hidden.as_str())) {
            // O valor guardado é o texto da configuração (o `''` quando ela não é um texto).
            let value = activity_bar_position_of(&activity_bar_position).map_or("", ActivityBarPosition::as_str);
            self.pane.composite_part().storage_service().store(LayoutSettings::ActivityBarLocation.as_str(), StorageValue::String(value.to_owned()), StorageScope::Profile, StorageTarget::User);
        }
    }

    fn get_remembered_activity_bar_visible_position(&self) -> ActivityBarPosition {
        let activity_bar_position = self.pane.composite_part().storage_service().get(LayoutSettings::ActivityBarLocation.as_str(), StorageScope::Profile, None);
        match activity_bar_position.as_deref() {
            Some(value) if value == ActivityBarPosition::Top.as_str() => ActivityBarPosition::Top,
            Some(value) if value == ActivityBarPosition::Bottom.as_str() => ActivityBarPosition::Bottom,
            _ => ActivityBarPosition::Default,
        }
    }

    /// `focusActivityBar()`.
    pub fn focus_activity_bar(&self) -> Promise<()> {
        let hidden = self.pane.configuration_service().get_value(Some(LayoutSettings::ActivityBarLocation.as_str()), None).strict_equals(&Dynamic::from(ActivityBarPosition::Hidden.as_str()));
        let weak = self.weak_self.clone();
        let then_focus = move || {
            let Some(this) = weak.upgrade() else { return };
            if this.pane.should_show_composite_bar() {
                this.pane.focus_composite_bar();
            } else {
                if !this.pane.layout_service().is_visible(Parts::ActivitybarPart, None) {
                    this.pane.layout_service().set_part_hidden(false, Parts::ActivitybarPart);
                }

                this.activity_bar_part().show(true);
            }
        };

        if hidden {
            let remembered = self.get_remembered_activity_bar_visible_position();
            let weak = self.weak_self.clone();
            self.pane
                .configuration_service()
                .update_value(LayoutSettings::ActivityBarLocation.as_str(), Dynamic::from(remembered.as_str()), None, None)
                .then(move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.on_did_change_activity_bar_location();
                    }
                    then_focus();
                })
        } else {
            then_focus();
            Promise::resolved(())
        }
    }

    fn register_actions(&self) {
        let action = ToggleActivityBarVisibilityAction { part: self.weak_self.clone() };
        self.base.register(register_action2(Rc::new(action)));
    }

    /// `getPinnedPaneCompositeIds()` da barra de atividades.
    pub fn services(&self) -> &SidebarPartServices {
        &self.services
    }

    /// `toJSON()`.
    pub fn to_json(&self) -> Dynamic {
        Dynamic::object([("type", Dynamic::from(Parts::SidebarPart.as_str()))])
    }
}

impl IDisposable for SidebarPart {
    fn dispose(&self) {
        self.base.dispose();
        self.pane.dispose();
    }
}

/// O `Action2` de `registerActions`: alterna a visibilidade da barra de atividades.
struct ToggleActivityBarVisibilityAction {
    part: Weak<SidebarPart>,
}

impl Action2 for ToggleActivityBarVisibilityAction {
    fn desc(&self) -> Action2Options {
        let title = localize2("toggleActivityBar", "Toggle Activity Bar Visibility");
        Action2Options {
            command: CommandAction::new(TOGGLE_ACTIVITY_BAR_VISIBILITY_ACTION_ID, Title::Localized(CommandActionTitle { value: title.value, original: title.original, mnemonic_title: None })),
            menu: Vec::new(),
            keybinding: Vec::new(),
            f1: false,
        }
    }

    fn run(&self, _accessor: &ServicesAccessor, _args: &[Dynamic]) {
        let Some(that) = self.part.upgrade() else { return };
        let configuration_service = that.pane.configuration_service();
        let hidden = configuration_service.get_value(Some(LayoutSettings::ActivityBarLocation.as_str()), None).strict_equals(&Dynamic::from(ActivityBarPosition::Hidden.as_str()));
        let value = if hidden { that.get_remembered_activity_bar_visible_position() } else { ActivityBarPosition::Hidden };
        let _ = configuration_service.update_value(LayoutSettings::ActivityBarLocation.as_str(), Dynamic::from(value.as_str()), None, None);
    }
}

/// A posição da barra de atividades que o valor de configuração é, se for uma delas.
fn activity_bar_position_of(value: &Dynamic) -> Option<ActivityBarPosition> {
    [ActivityBarPosition::Default, ActivityBarPosition::Top, ActivityBarPosition::Bottom, ActivityBarPosition::Hidden]
        .into_iter()
        .find(|position| value.strict_equals(&Dynamic::from(position.as_str())))
}
