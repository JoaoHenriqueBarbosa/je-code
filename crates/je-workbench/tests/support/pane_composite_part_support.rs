//! Dublês comuns dos testes das partes de painéis (`paneCompositePart.test.ts`, `sidebarPart.test.ts` e
//! `auxiliaryBarPart.test.ts`): o `TestLayoutService` com `TestPaneCompositeLayoutService`/`TestSidebarLayoutService`, o
//! `TestViewDescriptorService`, a vista sem árvore nativa dos casos que só leem classes e estilo e o conjunto de serviços do
//! `workbenchInstantiationService`.
#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable, none};
use je_base::promise::Promise;
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::theme_service::IThemeService;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::actions::IMenuService;
use je_platform::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::IContextKeyService;
use je_platform::contextview::context_view::{AnchorTarget, IContextMenuService};
use je_platform::contextview::view_element::ViewElement;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::notification::INotificationService;
use je_platform::storage::StorageService;
use je_widgets::grid::Direction;
use je_widgets::gridview::ViewDimensions;
use je_widgets::action_view_items::{IActionViewItem, IActionViewItemOptions};
use je_workbench::browser::dnd::{DragAndDropHost, DragEventHandler, DragEventKind};
use je_workbench::browser::parts::activitybar::activitybar_part::CompositeBarRequest;
use je_workbench::browser::parts::composite_part::{
    CompositePartServices, CompositeToolBarArgs, ICompositePartView, ICompositeProgressBar, ICompositeToolBar, ITitleLabelView, IToolBarItems,
};
use je_workbench::browser::parts::pane_composite_bar::{ContentElement, IPaneCompositeBar};
use je_workbench::browser::parts::pane_composite_part::{GlobalToolBarArgs, IPaneCompositePartView, PaneCompositeLayout, PaneCompositePartServices};
use je_workbench::common::views::{
    IViewContainerModel, IViewDescriptorService, ViewContainerLocation, ViewContainerLocationChange, ViewContainerRc, ViewContainersChange, ViewDescriptorRc,
    ViewVisibilityState, ViewsContainerChange, ViewsLocationChange,
};
use je_workbench::services::extensions::extensions::IExtensionService;
use je_workbench::services::layout::layout_service::{
    AddedContainer, ContainerLayout, ILayoutService, IWorkbenchLayoutService, LayoutContainerId, LayoutOffsetInfo, LayoutPart, PanelAlignment, PartVisibilityChangeEvent,
    Parts, Position, WindowId, WindowMaximizedChange,
};
use je_workbench::services::progress::progress_indicator::IProgressBar;
use je_workbench::test::common::workbench_test_services::TestStorageService;

pub const BODY: LayoutContainerId = LayoutContainerId(1);

/// `TestLayoutService` e as subclasses `TestPaneCompositeLayoutService` e `TestSidebarLayoutService`: o que cada caso reatribui
/// é uma `Cell`, e as partes visíveis são o conjunto `visibleParts`.
pub struct PaneTestLayoutService {
    pub floating_panels_enabled: Cell<bool>,
    pub modern_ui_compact: Cell<bool>,
    pub panel_position: Cell<Position>,
    pub side_bar_position: Cell<Position>,
    visible_parts: RefCell<HashSet<Parts>>,
}

impl PaneTestLayoutService {
    pub fn new(visible_parts: &[Parts]) -> Rc<Self> {
        Rc::new(Self {
            floating_panels_enabled: Cell::new(false),
            modern_ui_compact: Cell::new(false),
            panel_position: Cell::new(Position::Bottom),
            side_bar_position: Cell::new(Position::Left),
            visible_parts: RefCell::new(visible_parts.iter().copied().collect()),
        })
    }
}

impl ILayoutService for PaneTestLayoutService {
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

impl IWorkbenchLayoutService for PaneTestLayoutService {
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
        self.visible_parts.borrow().contains(&part)
    }
    fn set_part_hidden(&self, hidden: bool, part: Parts) {
        if hidden {
            self.visible_parts.borrow_mut().remove(&part);
        } else {
            self.visible_parts.borrow_mut().insert(part);
        }
    }
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
        self.panel_position.get()
    }
    fn set_panel_position(&self, position: Position) {
        self.panel_position.set(position);
    }
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

/// `TestViewDescriptorService extends mock<IViewDescriptorService>()`: os eventos são `Event.None`, `canMoveViews` é o valor dado, o
/// contêiner padrão é `undefined` e não há contêineres em nenhuma localização. Os outros membros não existem no `mock`.
pub struct TestViewDescriptorService {
    pub can_move_views: bool,
}

impl IViewDescriptorService for TestViewDescriptorService {
    fn view_containers(&self) -> Vec<ViewContainerRc> {
        panic!("viewContainers is not a function")
    }
    fn on_did_change_view_containers(&self) -> Event<ViewContainersChange> {
        Event::none()
    }
    fn get_default_view_container(&self, _location: ViewContainerLocation) -> Option<ViewContainerRc> {
        None
    }
    fn get_view_container_by_id(&self, _id: &str) -> Option<ViewContainerRc> {
        panic!("getViewContainerById is not a function")
    }
    fn is_view_container_removed_permanently(&self, _id: &str) -> bool {
        panic!("isViewContainerRemovedPermanently is not a function")
    }
    fn get_default_view_container_location(&self, _view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        panic!("getDefaultViewContainerLocation is not a function")
    }
    fn get_view_container_location(&self, _view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        panic!("getViewContainerLocation is not a function")
    }
    fn get_view_containers_by_location(&self, _location: ViewContainerLocation) -> Vec<ViewContainerRc> {
        Vec::new()
    }
    fn get_view_container_model(&self, _view_container: &ViewContainerRc) -> Rc<dyn IViewContainerModel> {
        panic!("getViewContainerModel is not a function")
    }
    fn on_did_change_container_location(&self) -> Event<ViewContainerLocationChange> {
        Event::none()
    }
    fn move_view_container_to_location(&self, _view_container: &ViewContainerRc, _location: ViewContainerLocation, _requested_index: Option<usize>, _reason: Option<&str>) {
        panic!("moveViewContainerToLocation is not a function")
    }
    fn get_view_container_badge_enablement_state(&self, _id: &str) -> bool {
        panic!("getViewContainerBadgeEnablementState is not a function")
    }
    fn set_view_container_badge_enablement_state(&self, _id: &str, _badges_enabled: bool) {
        panic!("setViewContainerBadgeEnablementState is not a function")
    }
    fn get_view_descriptor_by_id(&self, _id: &str) -> Option<ViewDescriptorRc> {
        panic!("getViewDescriptorById is not a function")
    }
    fn get_view_container_by_view_id(&self, _id: &str) -> Option<ViewContainerRc> {
        panic!("getViewContainerByViewId is not a function")
    }
    fn get_default_container_by_id(&self, _id: &str) -> Option<ViewContainerRc> {
        panic!("getDefaultContainerById is not a function")
    }
    fn get_view_location_by_id(&self, _id: &str) -> Option<ViewContainerLocation> {
        panic!("getViewLocationById is not a function")
    }
    fn can_move_views(&self) -> bool {
        self.can_move_views
    }
    fn on_did_change_container(&self) -> Event<ViewsContainerChange> {
        panic!("onDidChangeContainer is not defined")
    }
    fn move_views_to_container(&self, _views: &[ViewDescriptorRc], _view_container: &ViewContainerRc, _visibility_state: Option<ViewVisibilityState>, _reason: Option<&str>) {
        panic!("moveViewsToContainer is not a function")
    }
    fn on_did_change_location(&self) -> Event<ViewsLocationChange> {
        panic!("onDidChangeLocation is not defined")
    }
    fn move_view_to_location(&self, _view: &ViewDescriptorRc, _location: ViewContainerLocation, _reason: Option<&str>) {
        panic!("moveViewToLocation is not a function")
    }
    fn reset(&self) {
        panic!("reset is not a function")
    }
}

/// O `DragAndDropHost` dos casos: nenhum arrasto acontece.
pub struct NoDragAndDropHost;

impl DragAndDropHost for NoDragAndDropHost {
    fn add_listener(&self, _element: &Rc<ViewElement>, _kind: DragEventKind, _handler: DragEventHandler) -> DisposableRef {
        none()
    }

    fn set_draggable(&self, _element: &Rc<ViewElement>, _draggable: bool) {}
}

/// A `ProgressBar` dos casos sem árvore nativa: nada é pintado.
struct StubProgressBar;

impl IProgressBar for StubProgressBar {
    fn infinite(&self) {}
    fn total(&self, _value: f64) {}
    fn has_total(&self) -> bool {
        false
    }
    fn worked(&self, _value: f64) {}
    fn stop(&self) {}
    fn show(&self, _delay: Option<u64>) {}
    fn hide(&self) {}
}

impl IDisposable for StubProgressBar {
    fn dispose(&self) {}
}

/// O rótulo do título dos casos sem árvore nativa.
struct StubTitleLabel;

impl ITitleLabelView for StubTitleLabel {
    fn set_text(&self, _text: &str) {}
    fn update_hover(&self, _content: &str) {}
    fn set_color(&self, _color: Option<&je_theme::color::Color>) {}
}

impl IDisposable for StubTitleLabel {
    fn dispose(&self) {}
}

/// A barra de ferramentas dos casos sem árvore nativa: nenhum item.
struct StubToolBar;

impl IDisposable for StubToolBar {
    fn dispose(&self) {}
}

impl IToolBarItems for StubToolBar {
    fn get_items_width(&self) -> f64 {
        0.0
    }
    fn get_items_length(&self) -> usize {
        0
    }
}

impl ICompositeToolBar for StubToolBar {
    fn set_actions(&self, _primary_actions: &[ActionRef], _secondary_actions: &[ActionRef], _menu_ids: Option<&[je_platform::actions::MenuId]>) {}
    fn set_context(&self, _context: Option<je_base::dynamic::Dynamic>) {}
    fn set_action_runner(&self, _action_runner: Rc<dyn je_base::actions::IActionRunner>) {}
    fn action_runner(&self) -> Rc<dyn je_base::actions::IActionRunner> {
        je_base::actions::ActionRunner::new()
    }
    fn set_aria_label(&self, _label: &str) {}
    fn get_element(&self) -> je_ui::NodeId {
        panic!("a barra de ferramentas do caso não tem nó")
    }
    fn get_item_action(&self, _index: usize) -> Option<ActionRef> {
        None
    }
    fn get_item_width(&self, _index: usize) -> f64 {
        0.0
    }
    fn get_item_element(&self, _index: usize) -> Option<je_ui::NodeId> {
        None
    }
    fn relayout(&self) {}
}

/// A vista dos casos que só leem classes e estilo: elementos sem nó nativo e barras vazias.
pub struct StubPaneView {
    pub device_pixel_ratio: Cell<f64>,
}

impl StubPaneView {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { device_pixel_ratio: Cell::new(1.0) })
    }
}

impl ICompositePartView for StubPaneView {
    fn create_element(&self, parent: Option<&Rc<ViewElement>>, _tag: &str, class_name: &str) -> Rc<ViewElement> {
        let element = ViewElement::new();
        element.set_class_name(class_name);
        if let Some(parent) = parent {
            element.set_parent(Some(parent.clone()));
        }
        element
    }
    fn prepend_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>) {
        child.set_parent(Some(parent.clone()));
    }
    fn append_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>) {
        child.set_parent(Some(parent.clone()));
    }
    fn remove_child(&self, child: &Rc<ViewElement>) {
        child.set_parent(None);
    }
    fn create_title_label_view(&self, _title_label: &Rc<ViewElement>) -> Rc<dyn ITitleLabelView> {
        Rc::new(StubTitleLabel)
    }
    fn create_progress_bar(&self, _container: &Rc<ViewElement>) -> Rc<dyn ICompositeProgressBar> {
        Rc::new(StubProgressBar)
    }
    fn create_tool_bar(&self, _args: CompositeToolBarArgs) -> Rc<dyn ICompositeToolBar> {
        Rc::new(StubToolBar)
    }
    fn create_action_view_item(&self, _action: &ActionRef, _options: &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>> {
        None
    }
}

impl IPaneCompositePartView for StubPaneView {
    fn set_text_content(&self, _element: &Rc<ViewElement>, _text: &str) {}
    fn create_global_tool_bar(&self, _args: GlobalToolBarArgs) -> Rc<dyn IToolBarItems> {
        Rc::new(StubToolBar)
    }
    fn on_context_menu(&self, _element: &Rc<ViewElement>, _handler: Rc<dyn Fn(AnchorTarget)>) -> DisposableRef {
        none()
    }
    fn schedule_at_next_animation_frame(&self, _runner: Box<dyn FnOnce()>) -> DisposableRef {
        none()
    }
    fn toggle_container_class(&self, _container: LayoutContainerId, _class_name: &str, _force: bool) {}
    fn get_floating_border_width(&self) -> f64 {
        let device_pixel_ratio = self.device_pixel_ratio.get();
        (device_pixel_ratio * 1.0).floor().max(1.0) / device_pixel_ratio
    }
    fn content_element_for(&self, _element: &Rc<ViewElement>) -> Rc<ContentElement> {
        Rc::new(ContentElement::default())
    }
    fn layout_part(&self, _layout: &PaneCompositeLayout) {}
}

/// Os serviços que o `workbenchInstantiationService(undefined, store)` entrega às partes de painéis.
pub struct PaneTestDeps {
    pub notification_service: Rc<dyn INotificationService>,
    pub theme_service: Rc<dyn IThemeService>,
    pub keybinding_service: Rc<dyn IKeybindingService>,
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub menu_service: Rc<dyn IMenuService>,
    pub context_menu_service: Rc<dyn IContextMenuService>,
    pub command_service: Rc<dyn ICommandService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub view_descriptor_service: Rc<dyn IViewDescriptorService>,
    pub extension_service: Rc<dyn IExtensionService>,
    pub storage_service: Rc<dyn StorageService>,
}

/// O `TestStorageService` como `IStorageService`.
pub fn test_storage_service() -> Rc<dyn StorageService> {
    Rc::new(TestStorageService::new())
}

/// A barra de composites dos casos: o `createInstance(PaneCompositeBar, ...)` do original cria uma barra real, que aqui não tem
/// itens (nenhum caso registra painéis).
pub struct StubPaneCompositeBar;

impl IDisposable for StubPaneCompositeBar {
    fn dispose(&self) {}
}

impl IPaneCompositeBar for StubPaneCompositeBar {
    fn create(&self, _parent: &Rc<ContentElement>) {}
    fn layout(&self, _width: f64, _height: f64) {}
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

/// Os serviços da `AbstractPaneCompositePart` sobre a `view` dada.
pub fn pane_part_services<V>(deps: &PaneTestDeps, layout_service: Rc<PaneTestLayoutService>, view: Rc<V>, instantiation_service: Rc<InstantiationService>) -> PaneCompositePartServices
where
    V: ICompositePartView + IPaneCompositePartView + 'static,
{
    PaneCompositePartServices {
        composite: CompositePartServices {
            notification_service: deps.notification_service.clone(),
            storage_service: deps.storage_service.clone(),
            keybinding_service: deps.keybinding_service.clone(),
            instantiation_service,
            theme_service: deps.theme_service.clone(),
            layout_service,
            view: view.clone(),
        },
        pane_view: view,
        context_menu_service: deps.context_menu_service.clone(),
        view_descriptor_service: deps.view_descriptor_service.clone(),
        context_key_service: deps.context_key_service.clone(),
        extension_service: deps.extension_service.clone(),
        menu_service: deps.menu_service.clone(),
        configuration_service: deps.configuration_service.clone(),
        composite_bar_factory: Rc::new(|_request: CompositeBarRequest| -> Rc<dyn IPaneCompositeBar> { Rc::new(StubPaneCompositeBar) }),
        drag_and_drop_host: Rc::new(NoDragAndDropHost),
        focus_tracker_factory: None,
    }
}

/// `new InstantiationService()` sem serviços (as partes só o usam para os composites, que os casos não instanciam).
pub fn empty_instantiation_service() -> Rc<InstantiationService> {
    InstantiationService::new(ServiceCollection::new())
}
