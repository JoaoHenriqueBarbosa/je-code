//! Porte de `src/vs/workbench/test/browser/parts/activitybar/activitybarPart.test.ts` (suíte `ActivitybarPart`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o [`LeakGuard`] criado no começo de cada caso; `disposables.add` vira
//!   `guard.add_rc` / `guard.add`. O `setup`/`teardown` do fixture no `body` não existe: o elemento da parte é um
//!   [`PartElement`] nativo, sem árvore para anexar nem remover.
//! - **DOM vira estado.** `el.style.getPropertyValue(...)`, `el.classList.contains(...)`, `el.style.backgroundColor` e
//!   `el.style.borderColor` são os leitores do [`PartElement`]; `content.style.height` é a altura do `ContentElement`.
//! - **`TestLayoutService` e a subclasse `TestFloatingPanelsLayoutService`** são um dublê só, com `Cell` nos campos que o
//!   original reatribui (`floatingPanelsEnabled`, `modernUICompact`, `sideBarPosition` e `isVisible`).
//! - **`TestHostService`** é o dublê local `TestHostService` (`setFocus`, `setActiveWindow`); `mainWindow.vscodeWindowId` é
//!   `MAIN_WINDOW.0`.
//! - **`stubInstantiationService`** é a [`CompositeBarFactory`]: a padrão falha com `not expected`, como o `createInstance`
//!   que lança no original.
//! - **`Reflect.get(ActivityBarCompositeBar.prototype, 'layout').call(harness, ...)`**: o `layout` real da
//!   [`ActivityBarCompositeBar`] roda sobre um `PaneCompositeBar` de base que guarda a dimensão recebida (o `compositeBar.layout`
//!   do harness) e sobre uma barra global cujo `getHeight` é a fórmula do original e cujo `clientWidth` falha, para provar que o
//!   layout não mede o DOM.
//! - **`deepStrictEqual` de objetos literais** é `assert_eq!` de tuplas, com os mesmos campos, na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod configuration_event_support;
#[path = "../../je-editor/tests/support/test_color_theme_service.rs"]
mod test_color_theme_service;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use configuration_event_support::fire_configuration_change;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable, none};
use je_base::promise::Promise;
use je_editor::core_2d::dimension::Dimension;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_widgets::action_bar::ActionsOrientation;
use je_widgets::grid::Direction;
use je_widgets::gridview::ViewDimensions;
use je_workbench::browser::parts::activitybar::activitybar_part::{
    ACTIVITY_BAR_BACKGROUND, ACTIVITY_BAR_BORDER, ActivityBarCompositeBar, ActivitybarPart, CompositeBarFactory, CompositeBarRequest,
    MODERN_ACTIVITY_BAR_BACKGROUND, MODERN_ACTIVITY_BAR_INACTIVE_BACKGROUND, MenuBarContainer, PartElement,
};
use je_workbench::browser::parts::pane_composite_bar::{
    ContentElement, GlobalActivities, IGlobalCompositeBar, IPaneCompositeBar, IPaneCompositePart, PaneCompositeBarOptions, global_composite_bar_height,
};
use je_workbench::common::views::ViewContainerLocation;
use je_workbench::services::host::host::{IHostService, ReloadOptions};
use je_workbench::services::layout::layout_service::{
    AddedContainer, COMPACT_FLOATING_PANEL_OUTER_MARGIN, ContainerLayout, ILayoutService, IWorkbenchLayoutService, LayoutContainerId, LayoutOffsetInfo, LayoutPart,
    LayoutSettings, MAIN_WINDOW, ModernUiDensity, PanelAlignment, PartVisibilityChangeEvent, Parts, Position, WindowId, WindowMaximizedChange,
};
use test_color_theme_service::{TestColorTheme, TestThemeService};

const BODY: LayoutContainerId = LayoutContainerId(1);

/// `TestLayoutService` + `TestFloatingPanelsLayoutService`.
struct TestFloatingPanelsLayoutService {
    floating_panels_enabled: Cell<bool>,
    modern_ui_compact: Cell<bool>,
    side_bar_position: Cell<Position>,
    is_visible_fn: RefCell<Box<dyn Fn(Parts) -> bool>>,
}

impl TestFloatingPanelsLayoutService {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            floating_panels_enabled: Cell::new(false),
            modern_ui_compact: Cell::new(false),
            side_bar_position: Cell::new(Position::Left),
            is_visible_fn: RefCell::new(Box::new(|_| true)),
        })
    }

    /// `layoutService.isVisible = (partId) => visible.has(partId)`.
    fn set_visible_parts(&self, visible: &[Parts]) {
        let visible: Vec<Parts> = visible.to_vec();
        *self.is_visible_fn.borrow_mut() = Box::new(move |part| visible.contains(&part));
    }
}

impl ILayoutService for TestFloatingPanelsLayoutService {
    fn on_did_layout_main_container(&self) -> Event<Dimension> {
        Event::none()
    }
    fn on_did_layout_container(&self) -> Event<ContainerLayout> {
        Event::none()
    }
    fn on_did_layout_active_container(&self) -> Event<Dimension> {
        Event::none()
    }
    fn on_did_add_container(&self) -> Event<AddedContainer> {
        Event::none()
    }
    fn on_did_change_active_container(&self) -> Event<()> {
        Event::none()
    }
    fn main_container_dimension(&self) -> Dimension {
        Dimension { width: 800.0, height: 600.0 }
    }
    fn active_container_dimension(&self) -> Dimension {
        Dimension { width: 800.0, height: 600.0 }
    }
    fn main_container(&self) -> LayoutContainerId {
        BODY
    }
    fn active_container(&self) -> LayoutContainerId {
        BODY
    }
    fn containers(&self) -> Vec<LayoutContainerId> {
        vec![BODY]
    }
    fn get_container(&self, _window: WindowId) -> LayoutContainerId {
        BODY
    }
    fn when_container_styles_loaded(&self, _window: WindowId) -> Option<Promise<()>> {
        None
    }
    fn main_container_offset(&self) -> LayoutOffsetInfo {
        LayoutOffsetInfo { top: 0.0, quick_pick_top: 0.0 }
    }
    fn active_container_offset(&self) -> LayoutOffsetInfo {
        LayoutOffsetInfo { top: 0.0, quick_pick_top: 0.0 }
    }
    fn focus(&self) {}
}

impl IWorkbenchLayoutService for TestFloatingPanelsLayoutService {
    fn on_did_change_zen_mode(&self) -> Event<bool> {
        Event::none()
    }
    fn on_did_change_window_maximized(&self) -> Event<WindowMaximizedChange> {
        Event::none()
    }
    fn on_did_change_main_editor_centered_layout(&self) -> Event<bool> {
        Event::none()
    }
    fn on_did_change_panel_position(&self) -> Event<String> {
        Event::none()
    }
    fn on_did_change_panel_alignment(&self) -> Event<PanelAlignment> {
        Event::none()
    }
    fn on_did_change_part_visibility(&self) -> Event<PartVisibilityChangeEvent> {
        Event::none()
    }
    fn on_did_change_notifications_visibility(&self) -> Event<bool> {
        Event::none()
    }
    fn on_did_change_auxiliary_bar_maximized(&self) -> Event<()> {
        Event::none()
    }
    fn opened_default_editors(&self) -> bool {
        false
    }
    fn layout(&self) {}
    fn is_restored(&self) -> bool {
        true
    }
    fn when_restored(&self) -> Promise<()> {
        Promise::resolved(())
    }
    fn has_focus(&self, _part: Parts) -> bool {
        false
    }
    fn is_floating_panels_enabled(&self) -> bool {
        self.floating_panels_enabled.get()
    }
    fn is_modern_ui_compact(&self) -> bool {
        self.modern_ui_compact.get()
    }
    fn focus_part(&self, _part: Parts, _target_window: Option<WindowId>) {}
    fn get_container_of_part(&self, _target_window: WindowId, _part: Option<Parts>) -> Option<LayoutContainerId> {
        Some(BODY)
    }
    fn is_visible(&self, part: Parts, _target_window: Option<WindowId>) -> bool {
        (self.is_visible_fn.borrow())(part)
    }
    fn set_part_hidden(&self, _hidden: bool, _part: Parts) {}
    fn is_secondary_side_bar_visible(&self) -> bool {
        false
    }
    fn toggle_secondary_side_bar(&self) {}
    fn toggle_maximized_panel(&self) {}
    fn is_panel_maximized(&self) -> bool {
        false
    }
    fn toggle_maximized_auxiliary_bar(&self) {}
    fn set_auxiliary_bar_maximized(&self, _maximized: bool) -> bool {
        false
    }
    fn is_auxiliary_bar_maximized(&self) -> bool {
        false
    }
    fn has_main_window_border(&self) -> bool {
        false
    }
    fn get_main_window_border_radius(&self) -> Option<String> {
        None
    }
    fn get_side_bar_position(&self) -> Position {
        self.side_bar_position.get()
    }
    fn toggle_menu_bar(&self) {}
    fn get_panel_position(&self) -> Position {
        Position::Left
    }
    fn set_panel_position(&self, _position: Position) {}
    fn get_panel_alignment(&self) -> PanelAlignment {
        PanelAlignment::Center
    }
    fn set_panel_alignment(&self, _alignment: PanelAlignment) {}
    fn get_maximum_editor_dimensions(&self, _container: LayoutContainerId) -> Dimension {
        panic!("not implemented")
    }
    fn toggle_zen_mode(&self) {}
    fn is_main_editor_layout_centered(&self) -> bool {
        false
    }
    fn center_main_editor_layout(&self, _active: bool) {}
    fn get_size(&self, _part: Parts) -> ViewDimensions {
        panic!("Method not implemented.")
    }
    fn set_size(&self, _part: Parts, _size: ViewDimensions) {
        panic!("Method not implemented.")
    }
    fn resize_part(&self, _part: Parts, _size_change_width: f64, _size_change_height: f64) {}
    fn register_part(&self, _part: Rc<dyn LayoutPart>) -> DisposableRef {
        none()
    }
    fn is_window_maximized(&self, _target_window: WindowId) -> bool {
        false
    }
    fn update_window_maximized_state(&self, _target_window: WindowId, _maximized: bool) {}
    fn get_visible_neighbor_part(&self, _part: Parts, _direction: Direction) -> Option<Parts> {
        None
    }
}

/// `TestHostService`.
struct TestHostService {
    has_focus: Cell<bool>,
    on_did_change_focus: Rc<Emitter<bool>>,
    on_did_change_window: Rc<Emitter<u32>>,
}

impl TestHostService {
    fn new() -> Rc<Self> {
        Rc::new(Self { has_focus: Cell::new(true), on_did_change_focus: Emitter::new(), on_did_change_window: Emitter::new() })
    }

    /// `setFocus(focus)`.
    fn set_focus(&self, focus: bool) {
        self.has_focus.set(focus);
        self.on_did_change_focus.fire(focus);
    }

    /// `setActiveWindow(windowId)`.
    fn set_active_window(&self, window_id: u32) {
        self.on_did_change_window.fire(window_id);
    }
}

impl IHostService for TestHostService {
    fn reload(&self, _options: Option<ReloadOptions>) -> Promise<()> {
        Promise::resolved(())
    }
    fn restart(&self) -> Promise<()> {
        Promise::resolved(())
    }
    fn has_focus(&self) -> bool {
        self.has_focus.get()
    }
    fn on_did_change_focus(&self) -> Event<bool> {
        self.on_did_change_focus.event()
    }
    fn on_did_change_active_window(&self) -> Event<u32> {
        self.on_did_change_window.event()
    }
}

impl IDisposable for TestHostService {
    fn dispose(&self) {
        self.on_did_change_focus.dispose();
        self.on_did_change_window.dispose();
    }
}

/// `StubPaneCompositePart`.
struct StubPaneCompositePart;

impl IPaneCompositePart for StubPaneCompositePart {
    fn part_id(&self) -> Parts {
        Parts::SidebarPart
    }
    fn get_pinned_pane_composite_ids(&self) -> Vec<String> {
        Vec::new()
    }
    fn get_visible_pane_composite_ids(&self) -> Vec<String> {
        Vec::new()
    }
    fn get_pane_composite_ids(&self) -> Vec<String> {
        Vec::new()
    }
}

/// `stubCompositeBar = { create, layout, dispose }`, com o gancho de `layout`.
struct StubCompositeBar {
    on_layout: Box<dyn Fn(f64, f64)>,
}

impl StubCompositeBar {
    fn new(on_layout: impl Fn(f64, f64) + 'static) -> Rc<dyn IPaneCompositeBar> {
        Rc::new(Self { on_layout: Box::new(on_layout) })
    }
}

impl IDisposable for StubCompositeBar {
    fn dispose(&self) {}
}

impl IPaneCompositeBar for StubCompositeBar {
    fn create(&self, _parent: &Rc<ContentElement>) {}
    fn layout(&self, width: f64, height: f64) {
        (self.on_layout)(width, height);
    }
    fn focus(&self, _index: Option<usize>) {}
    fn get_pinned_pane_composite_ids(&self) -> Vec<String> {
        Vec::new()
    }
    fn get_visible_pane_composite_ids(&self) -> Vec<String> {
        Vec::new()
    }
    fn get_pane_composite_ids(&self) -> Vec<String> {
        Vec::new()
    }
}

struct Setup {
    compact: bool,
    floating_panels_enabled: bool,
    side_bar_position: Position,
    colors: Vec<(&'static str, &'static str)>,
    modern_ui_compact: bool,
    factory: Option<CompositeBarFactory>,
}

impl Default for Setup {
    fn default() -> Self {
        Self { compact: false, floating_panels_enabled: false, side_bar_position: Position::Left, colors: Vec::new(), modern_ui_compact: false, factory: None }
    }
}

struct Fixture {
    part: Rc<ActivitybarPart>,
    config_service: Rc<TestConfigurationService>,
    layout_service: Rc<TestFloatingPanelsLayoutService>,
    host_service: Rc<TestHostService>,
}

fn create_activitybar_part(guard: &LeakGuard, setup: Setup) -> Fixture {
    let config_service = TestConfigurationService::new(Some(Dynamic::object(vec![
        (LayoutSettings::ActivityBarCompact.as_str(), Dynamic::from(setup.compact)),
        (LayoutSettings::ModernUi.as_str(), Dynamic::from(setup.floating_panels_enabled)),
        (
            LayoutSettings::ModernUiDensity.as_str(),
            Dynamic::string(if setup.modern_ui_compact { ModernUiDensity::Compact.as_str() } else { ModernUiDensity::Default.as_str() }),
        ),
    ])));
    guard.add_rc(config_service.on_did_change_configuration_emitter.clone());
    let theme_service = guard.add_rc(TestThemeService::new(TestColorTheme::new(&setup.colors)));
    let layout_service = TestFloatingPanelsLayoutService::new();
    let host_service = guard.add_rc(TestHostService::new());
    layout_service.floating_panels_enabled.set(setup.floating_panels_enabled);
    layout_service.modern_ui_compact.set(setup.modern_ui_compact);
    layout_service.side_bar_position.set(setup.side_bar_position);

    // Override isVisible to return false so that create() does not call show() and attempt to instantiate the composite bar
    // (which requires a full DI setup).
    layout_service.set_visible_parts(&[]);

    // Stub instantiation service: createCompositeBar is only called in show(), which we skip in unit tests focused on
    // dimensions / style behaviour.
    let factory: CompositeBarFactory =
        setup.factory.unwrap_or_else(|| Rc::new(|_request: CompositeBarRequest| -> Rc<dyn IPaneCompositeBar> { panic!("not expected") }));

    let part = guard.add_rc(ActivitybarPart::new(
        ViewContainerLocation::Sidebar,
        Rc::new(StubPaneCompositePart),
        factory,
        layout_service.clone(),
        theme_service,
        config_service.clone(),
        host_service.clone(),
    ));

    Fixture { part, config_service, layout_service, host_service }
}

fn set_user_configuration(config_service: &TestConfigurationService, key: &str, value: impl Into<Dynamic>) {
    let _ = config_service.set_user_configuration(key, value.into(), None);
}

fn collect_events(guard: &LeakGuard, part: &ActivitybarPart) -> Rc<RefCell<Vec<bool>>> {
    // Cada `IViewSize | undefined` vira `is_some()`: `false` é o `undefined` que a parte dispara.
    let events: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    guard.add(part.on_did_change().listen(move |event| sink.borrow_mut().push(event.is_some())));
    events
}

fn created(part: &ActivitybarPart) -> Rc<PartElement> {
    let element = PartElement::new();
    part.create(&element);
    element
}

// --- Static constants ---------------------------------------------------

#[test]
fn default_constants_match_expected_dimensions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        (ActivitybarPart::ACTIVITYBAR_WIDTH, ActivitybarPart::ACTION_HEIGHT, ActivitybarPart::ICON_SIZE),
        (48.0, 48.0, 24.0)
    );
}

#[test]
fn compact_constants_match_reduced_dimensions() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        (ActivitybarPart::COMPACT_ACTIVITYBAR_WIDTH, ActivitybarPart::COMPACT_ACTION_HEIGHT, ActivitybarPart::COMPACT_ICON_SIZE),
        (36.0, 28.0, 16.0)
    );
}

#[test]
fn floating_constants_are_narrower_than_default() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        (ActivitybarPart::FLOATING_ACTIVITYBAR_WIDTH, ActivitybarPart::FLOATING_ACTION_HEIGHT, ActivitybarPart::FLOATING_COMPACT_ACTIVITYBAR_WIDTH),
        (36.0, 36.0, 28.0)
    );
}

// --- Dimension getters --------------------------------------------------

#[test]
fn default_mode_returns_default_width_constraints() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, .. } = create_activitybar_part(&guard, Setup::default());
    assert_eq!((part.minimum_width(), part.maximum_width()), (ActivitybarPart::ACTIVITYBAR_WIDTH, ActivitybarPart::ACTIVITYBAR_WIDTH));
}

#[test]
fn compact_mode_returns_compact_width_constraints() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, .. } = create_activitybar_part(&guard, Setup { compact: true, ..Setup::default() });
    assert_eq!(
        (part.minimum_width(), part.maximum_width()),
        (ActivitybarPart::COMPACT_ACTIVITYBAR_WIDTH, ActivitybarPart::COMPACT_ACTIVITYBAR_WIDTH)
    );
}

#[test]
fn height_constraints_are_unbounded() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, .. } = create_activitybar_part(&guard, Setup::default());
    assert_eq!(part.minimum_height(), 0.0);
    assert_eq!(part.maximum_height(), f64::INFINITY);
}

#[test]
fn floating_panels_reserves_the_cluster_perimeter_plus_a_leading_gap_only_for_a_standalone_right_hand_rail() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let base = ActivitybarPart::FLOATING_ACTIVITYBAR_WIDTH + ActivitybarPart::FLOATING_MARGIN + ActivitybarPart::FLOATING_LANE;
    let with_leading_gap = base + ActivitybarPart::FLOATING_MARGIN;

    let width_of = |side_bar_position: Position, side_bar_visible: bool, modern_ui_compact: bool| {
        let Fixture { part, layout_service, .. } = create_activitybar_part(
            &guard,
            Setup { floating_panels_enabled: true, side_bar_position, modern_ui_compact, ..Setup::default() },
        );
        layout_service.set_visible_parts(if side_bar_visible { &[Parts::SidebarPart] } else { &[] });
        part.minimum_width()
    };

    assert_eq!(
        (
            width_of(Position::Left, true, false),
            width_of(Position::Left, false, false),
            width_of(Position::Right, true, false),
            width_of(Position::Right, false, false),
            width_of(Position::Right, false, true),
        ),
        (
            base,
            base,
            base,
            // Only here does the rail follow another card and have to supply the gap itself.
            with_leading_gap,
            // Compact keeps its cards joined edge to edge, so no gap is ever needed.
            ActivitybarPart::FLOATING_ACTIVITYBAR_WIDTH + COMPACT_FLOATING_PANEL_OUTER_MARGIN + ActivitybarPart::FLOATING_COMPACT_LANE,
        )
    );
}

#[test]
fn compact_modern_ui_density_reserves_only_the_rail_padding() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, .. } = create_activitybar_part(&guard, Setup { floating_panels_enabled: true, modern_ui_compact: true, ..Setup::default() });

    let expected = ActivitybarPart::FLOATING_ACTIVITYBAR_WIDTH + COMPACT_FLOATING_PANEL_OUTER_MARGIN + ActivitybarPart::FLOATING_COMPACT_LANE;
    assert_eq!((part.minimum_width(), part.maximum_width()), (expected, expected));
}

// --- Configuration change: dimension update ----------------------------

#[test]
fn toggling_compact_via_config_changes_width_constraints() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, config_service, .. } = create_activitybar_part(&guard, Setup::default());

    // Initially default
    assert_eq!(part.minimum_width(), ActivitybarPart::ACTIVITYBAR_WIDTH);

    // Switch to compact
    set_user_configuration(&config_service, LayoutSettings::ActivityBarCompact.as_str(), true);
    fire_configuration_change(&config_service, &[LayoutSettings::ActivityBarCompact.as_str()]);

    assert_eq!(
        (part.minimum_width(), part.maximum_width()),
        (ActivitybarPart::COMPACT_ACTIVITYBAR_WIDTH, ActivitybarPart::COMPACT_ACTIVITYBAR_WIDTH)
    );

    // Switch back to default
    set_user_configuration(&config_service, LayoutSettings::ActivityBarCompact.as_str(), false);
    fire_configuration_change(&config_service, &[LayoutSettings::ActivityBarCompact.as_str()]);

    assert_eq!((part.minimum_width(), part.maximum_width()), (ActivitybarPart::ACTIVITYBAR_WIDTH, ActivitybarPart::ACTIVITYBAR_WIDTH));
}

// --- onDidChange fires for grid ----------------------------------------

#[test]
fn fires_on_did_change_undefined_when_compact_setting_changes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, config_service, .. } = create_activitybar_part(&guard, Setup::default());
    let events = collect_events(&guard, &part);

    // Toggle to compact
    set_user_configuration(&config_service, LayoutSettings::ActivityBarCompact.as_str(), true);
    fire_configuration_change(&config_service, &[LayoutSettings::ActivityBarCompact.as_str()]);

    assert_eq!(events.borrow().len(), 1);
    assert!(!events.borrow()[0], "should fire undefined to signal constraint change");

    // Toggle back
    set_user_configuration(&config_service, LayoutSettings::ActivityBarCompact.as_str(), false);
    fire_configuration_change(&config_service, &[LayoutSettings::ActivityBarCompact.as_str()]);

    assert_eq!(events.borrow().len(), 2);
    assert!(!events.borrow()[1]);
}

#[test]
fn does_not_fire_on_did_change_for_unrelated_config_changes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, config_service, .. } = create_activitybar_part(&guard, Setup::default());
    let events = collect_events(&guard, &part);

    fire_configuration_change(&config_service, &["editor.fontSize"]);

    assert_eq!(events.borrow().len(), 0);
}

#[test]
fn fires_on_did_change_undefined_when_floating_panels_setting_changes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, config_service, layout_service, .. } = create_activitybar_part(&guard, Setup::default());
    let events = collect_events(&guard, &part);

    layout_service.floating_panels_enabled.set(true);
    set_user_configuration(&config_service, LayoutSettings::ModernUi.as_str(), true);
    fire_configuration_change(&config_service, &[LayoutSettings::ModernUi.as_str()]);

    assert_eq!(*events.borrow(), vec![false]);
    assert_eq!(part.minimum_width(), ActivitybarPart::FLOATING_ACTIVITYBAR_WIDTH + ActivitybarPart::FLOATING_MARGIN + ActivitybarPart::FLOATING_LANE);
}

#[test]
fn fires_on_did_change_undefined_when_modern_ui_density_changes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, config_service, layout_service, .. } = create_activitybar_part(&guard, Setup { floating_panels_enabled: true, ..Setup::default() });
    let events = collect_events(&guard, &part);

    layout_service.modern_ui_compact.set(true);
    set_user_configuration(&config_service, LayoutSettings::ModernUiDensity.as_str(), ModernUiDensity::Compact.as_str());
    fire_configuration_change(&config_service, &[LayoutSettings::ModernUiDensity.as_str()]);

    assert_eq!(
        (events.borrow().clone(), part.minimum_width()),
        (
            vec![false],
            ActivitybarPart::FLOATING_ACTIVITYBAR_WIDTH + COMPACT_FLOATING_PANEL_OUTER_MARGIN + ActivitybarPart::FLOATING_COMPACT_LANE
        )
    );
}

// --- CSS custom properties on element -----------------------------------

#[test]
fn update_compact_style_sets_correct_css_custom_properties_in_default_mode() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, .. } = create_activitybar_part(&guard, Setup::default());

    let el = created(&part);

    assert_eq!(el.get_property_value("--activity-bar-width"), format!("{}px", ActivitybarPart::ACTIVITYBAR_WIDTH));
    assert_eq!(el.get_property_value("--activity-bar-action-height"), format!("{}px", ActivitybarPart::ACTION_HEIGHT));
    assert_eq!(el.get_property_value("--activity-bar-action-gap"), "0px");
    assert_eq!(el.get_property_value("--activity-bar-icon-size"), format!("{}px", ActivitybarPart::ICON_SIZE));
    assert!(!el.has_class("compact"));
}

#[test]
fn update_compact_style_sets_correct_css_custom_properties_in_compact_mode() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, .. } = create_activitybar_part(&guard, Setup { compact: true, ..Setup::default() });

    let el = created(&part);

    assert_eq!(el.get_property_value("--activity-bar-width"), format!("{}px", ActivitybarPart::COMPACT_ACTIVITYBAR_WIDTH));
    assert_eq!(el.get_property_value("--activity-bar-action-height"), format!("{}px", ActivitybarPart::COMPACT_ACTION_HEIGHT));
    assert_eq!(el.get_property_value("--activity-bar-action-gap"), "0px");
    assert_eq!(el.get_property_value("--activity-bar-icon-size"), format!("{}px", ActivitybarPart::COMPACT_ICON_SIZE));
    assert!(el.has_class("compact"));
}

#[test]
fn update_compact_style_sets_correct_css_custom_properties_in_floating_mode() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, .. } = create_activitybar_part(&guard, Setup { floating_panels_enabled: true, ..Setup::default() });

    let el = created(&part);

    assert_eq!(el.get_property_value("--activity-bar-width"), format!("{}px", ActivitybarPart::FLOATING_ACTIVITYBAR_WIDTH));
    assert_eq!(el.get_property_value("--activity-bar-action-height"), format!("{}px", ActivitybarPart::FLOATING_ACTION_HEIGHT));
    assert_eq!(el.get_property_value("--activity-bar-action-gap"), format!("{}px", ActivitybarPart::FLOATING_ACTION_GAP));
    assert_eq!(el.get_property_value("--activity-bar-icon-size"), format!("{}px", ActivitybarPart::ICON_SIZE));
    assert!(!el.has_class("compact"));
}

#[test]
fn update_compact_style_sets_compact_modern_ui_density_properties() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, .. } = create_activitybar_part(&guard, Setup { floating_panels_enabled: true, modern_ui_compact: true, ..Setup::default() });
    let element = created(&part);

    assert_eq!(
        (
            element.get_property_value("--activity-bar-width"),
            element.get_property_value("--activity-bar-action-height"),
            element.get_property_value("--activity-bar-icon-size"),
        ),
        (
            format!("{}px", ActivitybarPart::FLOATING_ACTIVITYBAR_WIDTH),
            format!("{}px", ActivitybarPart::FLOATING_ACTION_HEIGHT),
            format!("{}px", ActivitybarPart::ICON_SIZE),
        )
    );
}

#[test]
fn toggling_compact_updates_css_custom_properties_on_element() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, config_service, .. } = create_activitybar_part(&guard, Setup::default());

    let el = created(&part);

    // Default state
    assert_eq!(el.get_property_value("--activity-bar-width"), format!("{}px", ActivitybarPart::ACTIVITYBAR_WIDTH));
    assert!(!el.has_class("compact"));

    // Switch to compact
    set_user_configuration(&config_service, LayoutSettings::ActivityBarCompact.as_str(), true);
    fire_configuration_change(&config_service, &[LayoutSettings::ActivityBarCompact.as_str()]);

    assert_eq!(el.get_property_value("--activity-bar-width"), format!("{}px", ActivitybarPart::COMPACT_ACTIVITYBAR_WIDTH));
    assert_eq!(el.get_property_value("--activity-bar-action-height"), format!("{}px", ActivitybarPart::COMPACT_ACTION_HEIGHT));
    assert_eq!(el.get_property_value("--activity-bar-icon-size"), format!("{}px", ActivitybarPart::COMPACT_ICON_SIZE));
    assert!(el.has_class("compact"));

    // Switch back
    set_user_configuration(&config_service, LayoutSettings::ActivityBarCompact.as_str(), false);
    fire_configuration_change(&config_service, &[LayoutSettings::ActivityBarCompact.as_str()]);

    assert_eq!(el.get_property_value("--activity-bar-width"), format!("{}px", ActivitybarPart::ACTIVITYBAR_WIDTH));
    assert_eq!(el.get_property_value("--activity-bar-action-height"), format!("{}px", ActivitybarPart::ACTION_HEIGHT));
    assert_eq!(el.get_property_value("--activity-bar-icon-size"), format!("{}px", ActivitybarPart::ICON_SIZE));
    assert!(!el.has_class("compact"));
}

#[test]
fn leaves_the_card_border_to_the_stylesheet_in_modern_ui_keeping_the_legacy_inline_border_otherwise() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let render = |floating_panels_enabled: bool| {
        let Fixture { part, .. } = create_activitybar_part(
            &guard,
            Setup { floating_panels_enabled, colors: vec![(ACTIVITY_BAR_BORDER, "#123456")], ..Setup::default() },
        );
        let el = created(&part);
        (el.border_color(), el.has_class("bordered"))
    };

    assert_eq!(
        (render(true), render(false)),
        (
            // The rail is a floating card, so `modernActivityBar.border` drives it from CSS and no inline colour is written,
            // which is what lets the seam be styled per edge.
            (String::new(), false),
            ("rgb(18, 52, 86)".to_owned(), true),
        )
    );
}

#[test]
fn uses_the_inactive_background_only_for_inactive_modern_ui_windows() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, config_service, host_service, .. } = create_activitybar_part(
        &guard,
        Setup {
            floating_panels_enabled: true,
            colors: vec![
                (ACTIVITY_BAR_BACKGROUND, "#123456"),
                (MODERN_ACTIVITY_BAR_BACKGROUND, "#abcdef"),
                (MODERN_ACTIVITY_BAR_INACTIVE_BACKGROUND, "#654321"),
            ],
            ..Setup::default()
        },
    );
    let el = PartElement::new();
    host_service.set_active_window(MAIN_WINDOW.0);
    part.create(&el);

    let active_modern_background = el.background_color();
    host_service.set_active_window(MAIN_WINDOW.0 + 1);
    let inactive_auxiliary_modern_background = el.background_color();
    host_service.set_focus(false);
    host_service.set_focus(true);
    let refocused_auxiliary_modern_background = el.background_color();
    host_service.set_active_window(MAIN_WINDOW.0);
    host_service.set_focus(false);
    let inactive_modern_background = el.background_color();
    set_user_configuration(&config_service, LayoutSettings::ModernUi.as_str(), false);
    fire_configuration_change(&config_service, &[LayoutSettings::ModernUi.as_str()]);

    assert_eq!(
        (
            active_modern_background,
            inactive_auxiliary_modern_background,
            refocused_auxiliary_modern_background,
            inactive_modern_background,
            el.background_color(),
        ),
        (
            "rgb(171, 205, 239)".to_owned(),
            "rgb(101, 67, 33)".to_owned(),
            "rgb(101, 67, 33)".to_owned(),
            "rgb(101, 67, 33)".to_owned(),
            "rgb(18, 52, 86)".to_owned(),
        )
    );
}

// --- toJSON ------------------------------------------------------------

#[test]
fn to_json_returns_correct_part_type() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let Fixture { part, .. } = create_activitybar_part(&guard, Setup::default());
    assert_eq!(part.to_json().part_type, Parts::ActivitybarPart);
}

// --- layout: floating panels gutter reservation -------------------------

/// The part has no title, header or footer, so the content area ends up exactly the height `layout()` reserved.
fn layout_content_height(guard: &LeakGuard, visible_parts: &[Parts], floating_panels_enabled: bool, modern_ui_compact: bool) -> f64 {
    let Fixture { part, layout_service, .. } = create_activitybar_part(guard, Setup { floating_panels_enabled, modern_ui_compact, ..Setup::default() });
    created(&part);

    layout_service.set_visible_parts(visible_parts);
    part.layout(100.0, 300.0);

    part.content().expect("o conteúdo foi criado").height()
}

#[test]
fn reserves_the_perimeter_gutter_on_each_window_edge_the_activity_bar_faces() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let margin = ActivitybarPart::FLOATING_MARGIN;
    // At the default density the window-edge perimeter matches the inter-card gap.
    let outer_margin = ActivitybarPart::FLOATING_MARGIN;
    let borders = ActivitybarPart::FLOATING_BORDER * 2.0;
    let actual = (
        // Windowed default: a title bar above and a status bar below, so neither is a window edge.
        layout_content_height(&guard, &[Parts::TitlebarPart, Parts::StatusbarPart], true, false),
        // Native fullscreen: nothing above the middle section, so the top is a window edge.
        layout_content_height(&guard, &[Parts::StatusbarPart], true, false),
        // A visible banner still occupies the row above, so the top is not a window edge.
        layout_content_height(&guard, &[Parts::BannerPart, Parts::StatusbarPart], true, false),
        // Hidden status bar: the activity bar now reaches the window bottom edge.
        layout_content_height(&guard, &[Parts::TitlebarPart], true, false),
        // Both edges at once.
        layout_content_height(&guard, &[], true, false),
        // Experiment disabled: the activity bar is not a floating card, so no gutters at all.
        layout_content_height(&guard, &[], false, false),
    );

    assert_eq!(
        actual,
        (
            300.0 - margin - borders,
            300.0 - outer_margin - margin - borders,
            300.0 - margin - borders,
            300.0 - outer_margin - borders,
            300.0 - outer_margin * 2.0 - borders,
            300.0,
        )
    );
}

#[test]
fn compact_density_keeps_the_activity_bar_flush_with_window_chrome() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let outer_margin = COMPACT_FLOATING_PANEL_OUTER_MARGIN;
    let borders = ActivitybarPart::FLOATING_BORDER * 2.0;
    assert_eq!(
        (
            layout_content_height(&guard, &[Parts::TitlebarPart, Parts::StatusbarPart], true, true),
            layout_content_height(&guard, &[Parts::StatusbarPart], true, true),
            layout_content_height(&guard, &[Parts::TitlebarPart], true, true),
            layout_content_height(&guard, &[], true, true),
        ),
        (300.0 - outer_margin - borders, 300.0 - outer_margin * 2.0 - borders, 300.0 - outer_margin - borders, 300.0 - outer_margin * 2.0 - borders)
    );
}

// --- composite bar item sizing -------------------------------------------

/// The composite bar decides how many activity icons fit before collapsing the rest into the overflow ("Additional Views")
/// menu, so the size it is handed has to match the vertical space an item actually occupies in the current mode.
fn captured_composite_bar_options(guard: &LeakGuard, compact: bool, floating_panels_enabled: bool, modern_ui_compact: bool) -> (PaneCompositeBarOptions, GlobalActivities) {
    let captured: Rc<RefCell<Option<(PaneCompositeBarOptions, Option<GlobalActivities>)>>> = Rc::new(RefCell::new(None));
    let sink = captured.clone();
    let factory: CompositeBarFactory = Rc::new(move |request: CompositeBarRequest| {
        *sink.borrow_mut() = Some((request.options, request.global_activities));
        StubCompositeBar::new(|_, _| {})
    });
    let Fixture { part, .. } = create_activitybar_part(guard, Setup { compact, floating_panels_enabled, modern_ui_compact, factory: Some(factory), ..Setup::default() });

    created(&part);
    part.show(false);

    let (options, global_activities) = captured.borrow_mut().take().expect("a fábrica foi chamada");
    (options, global_activities.expect("globalActivities"))
}

#[test]
fn composite_bar_item_size_tracks_the_rendered_item_stride_in_every_mode() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let sizes_for = |compact: bool, floating_panels_enabled: bool, modern_ui_compact: bool| {
        let (options, global_activities) = captured_composite_bar_options(&guard, compact, floating_panels_enabled, modern_ui_compact);
        (options.composite_size, options.overflow_action_size, global_activities)
    };

    assert_eq!(
        (
            sizes_for(false, false, false),
            sizes_for(true, false, false),
            sizes_for(false, true, false),
            sizes_for(true, true, false),
            sizes_for(false, true, true),
        ),
        (
            // Items stack flush against each other, so the stride is just the action height.
            (48.0, 48.0, GlobalActivities { action_height: 48.0, action_gap: 0.0 }),
            (28.0, 28.0, GlobalActivities { action_height: 28.0, action_gap: 0.0 }),
            // Modern UI separates items with an 8px gap, but only at the default size.
            (44.0, 44.0, GlobalActivities { action_height: 36.0, action_gap: 8.0 }),
            (28.0, 28.0, GlobalActivities { action_height: 28.0, action_gap: 0.0 }),
            // The compact density tightens that gap to 4px, and the stride has to follow it so items do not collapse into the
            // overflow menu early.
            (40.0, 40.0, GlobalActivities { action_height: 36.0, action_gap: 4.0 }),
        )
    );
}

/// The gap is rendered *between* items, so N items occupy `N * height + (N - 1) * gap`. `compositeSize` bakes a trailing gap
/// into every item, which over-counts by exactly one gap, so `layout()` hands that gap back to the composite bar. Without it
/// the last item is pushed into the overflow menu a gap earlier than it needs to be.
fn composite_bar_layout_height(guard: &LeakGuard, compact: bool, floating_panels_enabled: bool) -> f64 {
    let layout_height = Rc::new(Cell::new(-1.0));
    let sink = layout_height.clone();
    let factory: CompositeBarFactory = Rc::new(move |_request: CompositeBarRequest| {
        let sink = sink.clone();
        StubCompositeBar::new(move |_width, height| sink.set(height))
    });
    let Fixture { part, layout_service, .. } = create_activitybar_part(guard, Setup { compact, floating_panels_enabled, factory: Some(factory), ..Setup::default() });

    created(&part);
    part.show(false);

    // A visible title and status bar means neither edge is a window edge.
    layout_service.set_visible_parts(&[Parts::TitlebarPart, Parts::StatusbarPart]);
    part.layout(100.0, 300.0);

    layout_height.get()
}

#[test]
fn composite_bar_is_given_back_the_leading_item_gap_it_does_not_render() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let margin = ActivitybarPart::FLOATING_MARGIN;
    let borders = ActivitybarPart::FLOATING_BORDER * 2.0;

    assert_eq!(
        (
            composite_bar_layout_height(&guard, false, false),
            composite_bar_layout_height(&guard, true, false),
            composite_bar_layout_height(&guard, false, true),
            composite_bar_layout_height(&guard, true, true),
        ),
        (
            // No floating gutters and no gap between items.
            300.0,
            300.0,
            // Floating reserves a bottom gutter and the card border; the 8px gap is then handed back.
            300.0 - margin - borders + ActivitybarPart::FLOATING_ACTION_GAP,
            300.0 - margin - borders,
        )
    );
}

// --- global activity icons reservation -----------------------------------

/// O `globalCompositeBar` do harness: `globalActivityActionBar.length()` é a contagem, o `getHeight` é o do original e o
/// `clientWidth` falha, porque o layout não pode medir as atividades globais.
struct HarnessGlobalCompositeBar {
    global_action_count: usize,
}

impl IDisposable for HarnessGlobalCompositeBar {
    fn dispose(&self) {}
}

impl IGlobalCompositeBar for HarnessGlobalCompositeBar {
    fn get_height(&self, action_height: f64, action_gap: f64) -> f64 {
        global_composite_bar_height(self.global_action_count, action_height, action_gap)
    }
    fn client_width(&self) -> f64 {
        panic!("Layout must not measure global activities");
    }
    fn focus(&self) {}
    fn on_did_change(&self) -> Event<()> {
        Event::none()
    }
}

/// O `layout` real da `ActivityBarCompositeBar` sobre o harness: devolve a dimensão que chegou ao `compositeBar.layout`.
fn laid_out_by_activity_bar_composite_bar(
    guard: &LeakGuard,
    orientation: ActionsOrientation,
    menu_bar_container: Option<Rc<MenuBarContainer>>,
    global: Option<(Rc<dyn IGlobalCompositeBar>, GlobalActivities)>,
    width: f64,
    height: f64,
) -> Dimension {
    let laid_out: Rc<Cell<Option<Dimension>>> = Rc::new(Cell::new(None));
    let sink = laid_out.clone();
    let base = StubCompositeBar::new(move |width, height| sink.set(Some(Dimension { width, height })));
    let (global_composite_bar, global_activities) = match global {
        Some((bar, activities)) => (Some(bar), Some(activities)),
        None => (None, None),
    };
    let bar = guard.add_rc(ActivityBarCompositeBar::new(base, orientation, global_composite_bar, global_activities));
    bar.set_menu_bar_container(menu_bar_container);
    bar.layout(width, height);
    laid_out.get().expect("o compositeBar.layout foi chamado")
}

fn height_left_for_composite_bar(guard: &LeakGuard, global_action_count: usize, item_height: f64, gap: f64) -> f64 {
    let global: Rc<dyn IGlobalCompositeBar> = Rc::new(HarnessGlobalCompositeBar { global_action_count });
    laid_out_by_activity_bar_composite_bar(
        guard,
        ActionsOrientation::Vertical,
        None,
        Some((global, GlobalActivities { action_height: item_height, action_gap: gap })),
        ActivitybarPart::FLOATING_ACTIVITYBAR_WIDTH,
        300.0,
    )
    .height
}

#[test]
fn reserves_global_activity_sizes_without_synchronous_dom_measurement() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        (
            height_left_for_composite_bar(&guard, 2, 48.0, 0.0),
            height_left_for_composite_bar(&guard, 2, 28.0, 0.0),
            height_left_for_composite_bar(&guard, 2, 36.0, 8.0),
            height_left_for_composite_bar(&guard, 2, 28.0, 0.0),
            height_left_for_composite_bar(&guard, 2, 36.0, 4.0),
            height_left_for_composite_bar(&guard, 1, 36.0, 8.0),
        ),
        (204.0, 244.0, 220.0, 244.0, 224.0, 264.0)
    );
}

#[test]
fn horizontal_composite_bars_preserve_compact_menu_space_without_global_activities() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let menu_bar_container = Rc::new(MenuBarContainer::default());
    menu_bar_container.client_width.set(35.0);
    let laid_out = laid_out_by_activity_bar_composite_bar(&guard, ActionsOrientation::Horizontal, Some(menu_bar_container), None, 300.0, 35.0);

    assert_eq!(laid_out, Dimension { width: 265.0, height: 35.0 });
}
