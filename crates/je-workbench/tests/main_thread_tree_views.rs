//! Porte de `src/vs/workbench/api/test/browser/mainThreadTreeViews.test.ts` (suíte `MainThreadHostTreeView`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o `setup` é [`setup`] (async) e o
//!   `teardown` (`ViewsRegistry.deregisterViews`) é [`teardown`].
//! - `workbenchInstantiationService` e o `ViewDescriptorService` não entram: o `CustomTreeView` do porte só usa os serviços de
//!   [`CustomTreeViewServices`], e o registro do contêiner e da view vai direto nos registros ([`views_registry`]).
//! - `mock<ExtHostTreeViewsShape>()` é o [`MockExtHostTreeViewsShape`]: o membro que o original não sobrescreve falha.
//! - `CustomTreeItem` (`customProp`) é o item com a propriedade extra em `custom_properties`.
//! - `Reflect.get(treeView, 'treeViewDnd')` é `CustomTreeView::tree_view_dnd`; `Reflect.get(dragService, '_dragOperations')` é
//!   `TreeViewsDnDService::drag_operations`.
//! - `new DragEvent('dragstart', { dataTransfer: new DataTransfer() })` é o [`DragEventData`] com o [`TestDataTransfer`].
//! - `assert.throws(() => ..., /Unknown tree/)` é `errors::catch` com a conferência da mensagem.
//! - O `for (const teardown of ['tree', 'customer', 'view'])` do original registra três casos: `releases_an_active_drag_on_tree_disposal`,
//!   `releases_an_active_drag_on_customer_disposal` e `releases_an_active_drag_on_view_disposal`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::cancellation::CancellationToken;
use je_base::data_transfer::VSDataTransfer;
use je_base::dynamic::Dynamic;
use je_base::errors;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable, none};
use je_base::mime::mimes;
use je_base::nls::LocalizedString;
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_editor::tree_views_dnd::TreeViewsDnDService;
use je_platform::label::label::{
    FormatterChangeEvent, ILabelService, LabelFormatter, LabelSeparator, ResourceLabelFormatter, UriLabelOptions, WorkspaceLabelOptions, WorkspaceLabelTarget,
};
use je_platform::log::NullLogService;
use je_platform::progress::progress::{IProgress, IProgressService, Progress, ProgressOptions, ProgressStep, ProgressTask};
use je_platform::telemetry::telemetry_utils::NullTelemetryServiceShape;
use je_workbench::api::browser::main_thread_tree_views::MainThreadTreeViews;
use je_workbench::api::common::ext_host_protocol::{
    CheckboxUpdate, DataTransferDto, DataTransferItemDto, ExtHostTreeViewsShape, IMainContext, TreeChildrenBatch, TreeViewRegistrationOptions, UriListPart,
};
use je_workbench::api::common::ext_host_protocol::MainThreadTreeViewsShape;
use je_workbench::browser::parts::views::tree_view::{
    CustomTreeView, CustomTreeViewServices, DragDataTransferItem, DragEventData, ElementsDragAndDropData, IDragDataTransfer,
};
use je_workbench::common::tree_view_contracts::ITreeView;
use je_workbench::common::views::{
    IViewContainerDescriptor, IViewDescriptor, ITreeItem, TreeItemCollapsibleState, ViewContainerLocation, ViewContainerRc, ViewDescriptorRc, view_containers_registry,
    views_registry,
};
use je_workbench::services::activity::activity::{IActivity, IActivityService};
use je_workbench::services::extensions::extensions::{ActivationKind, IExtensionDescription, IExtensionService};
use je_workbench::services::views::views_service::IViewsService;
use je_workbench::test::common::workbench_test_services::TestNotificationService;
use je_extensions::scanner::ExtensionDescription;

const TEST_TREE_VIEW_ID: &str = "testTreeView";
const CUSTOM_VALUE: &str = "customValue";

fn custom_tree_item(handle: &str, collapsible_state: TreeItemCollapsibleState) -> ITreeItem {
    ITreeItem { handle: handle.to_owned(), collapsible_state, custom_properties: vec![("customProp".to_owned(), Dynamic::string(CUSTOM_VALUE))], ..ITreeItem::default() }
}

/// `class MockExtHostTreeViewsShape extends mock<ExtHostTreeViewsShape>()`.
#[derive(Default)]
struct MockExtHostTreeViewsShape {
    released_drag_operations: RefCell<Vec<String>>,
    drag_tokens: RefCell<Vec<CancellationToken>>,
    drag_result: RefCell<Option<DataTransferDto>>,
    children: RefCell<Vec<ITreeItem>>,
}

impl ExtHostTreeViewsShape for MockExtHostTreeViewsShape {
    fn rpc_get_children(&self, _tree_view_id: &str, _tree_item_handles: Option<Vec<String>>) -> Promise<Option<Vec<TreeChildrenBatch>>> {
        Promise::resolved(Some(vec![TreeChildrenBatch { index: 0, children: self.children.borrow().clone() }]))
    }

    fn rpc_handle_drop(
        &self,
        _destination_view_id: &str,
        _request_id: f64,
        _tree_data_transfer: DataTransferDto,
        _target_handle: Option<String>,
        _token: CancellationToken,
        _operation_uuid: Option<String>,
        _source_view_id: Option<String>,
        _source_tree_item_handles: Option<Vec<String>>,
    ) -> Promise<()> {
        panic!("$handleDrop is not a function");
    }

    fn rpc_handle_drag(&self, _source_view_id: &str, _source_tree_item_handles: Vec<String>, _operation_uuid: &str, token: CancellationToken) -> Promise<Option<DataTransferDto>> {
        self.drag_tokens.borrow_mut().push(token);
        Promise::resolved(self.drag_result.borrow().clone())
    }

    fn rpc_release_drag_operation(&self, operation_uuid: &str) {
        self.released_drag_operations.borrow_mut().push(operation_uuid.to_owned());
    }

    fn rpc_set_expanded(&self, _tree_view_id: &str, _tree_item_handle: &str, _expanded: bool) {
        panic!("$setExpanded is not a function");
    }

    fn rpc_set_selection_and_focus(&self, _tree_view_id: &str, _selection_handles: Vec<String>, _focus_handle: &str) {
        panic!("$setSelectionAndFocus is not a function");
    }

    fn rpc_set_visible(&self, _tree_view_id: &str, _visible: bool) {}

    fn rpc_change_checkbox_state(&self, _tree_view_id: &str, _checkbox_updates: Vec<CheckboxUpdate>) {
        panic!("$changeCheckboxState is not a function");
    }

    fn rpc_has_resolve(&self, _tree_view_id: &str) -> Promise<bool> {
        Promise::resolved(false)
    }

    fn rpc_resolve(&self, _tree_view_id: &str, _tree_item_handle: &str, _token: CancellationToken) -> Promise<Option<ITreeItem>> {
        panic!("$resolve is not a function");
    }
}

/// `new class implements IExtHostContext { getProxy() { return extHostTreeViewsShape; } ... }`.
struct TestExtHostContext {
    proxy: Rc<MockExtHostTreeViewsShape>,
}

impl IMainContext for TestExtHostContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        assert_eq!(sid, "ExtHostTreeViews");
        let shape: Rc<dyn ExtHostTreeViewsShape> = self.proxy.clone();
        Rc::new(shape)
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

/// `TestExtensionService`: as extensões já estão registradas.
struct TestExtensionService;

impl IExtensionService for TestExtensionService {
    fn activate_by_event(&self, _activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        Promise::resolved(())
    }

    fn activation_event_is_done(&self, _activation_event: &str) -> bool {
        true
    }

    fn get_extension(&self, _id: &str) -> Promise<Option<IExtensionDescription>> {
        Promise::resolved(None)
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        Event::none()
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        Vec::new()
    }
}

/// `TestViewsService`: `openView` resolve com `null`.
struct TestViewsService;

impl IViewsService for TestViewsService {
    fn open_view(&self, _id: &str, _focus: bool) -> Promise<()> {
        Promise::resolved(())
    }
}

/// O `TestProgressService` do `workbenchInstantiationService`: roda a tarefa na hora.
struct TestProgressService;

impl IProgressService for TestProgressService {
    fn with_progress(&self, _options: ProgressOptions, task: ProgressTask, _on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        let progress: Rc<dyn IProgress<ProgressStep>> = Progress::<ProgressStep>::none();
        task(progress)
    }
}

/// O `TestActivityService`: o selo não aparece em lugar nenhum.
struct TestActivityService;

impl IActivityService for TestActivityService {
    fn show_view_activity(&self, _view_id: &str, _badge: IActivity) -> DisposableRef {
        none()
    }
}

/// `mock<ILabelService>()`: nenhum membro é usado pelos casos.
struct MockLabelService;

impl ILabelService for MockLabelService {
    fn get_uri_label(&self, _resource: &URI, _options: &UriLabelOptions) -> String {
        panic!("getUriLabel is not a function");
    }

    fn get_uri_basename_label(&self, _resource: &URI) -> String {
        panic!("getUriBasenameLabel is not a function");
    }

    fn get_workspace_label(&self, _workspace: &WorkspaceLabelTarget, _options: Option<&WorkspaceLabelOptions>) -> String {
        panic!("getWorkspaceLabel is not a function");
    }

    fn get_host_label(&self, _scheme: &str, _authority: Option<&str>) -> String {
        panic!("getHostLabel is not a function");
    }

    fn get_host_tooltip(&self, _scheme: &str, _authority: Option<&str>) -> Option<String> {
        panic!("getHostTooltip is not a function");
    }

    fn get_separator(&self, _scheme: &str, _authority: Option<&str>) -> LabelSeparator {
        panic!("getSeparator is not a function");
    }

    fn get_uri_home(&self, _resource: &URI) -> Option<URI> {
        panic!("getUriHome is not a function");
    }

    fn register_formatter(&self, _formatter: LabelFormatter) -> DisposableRef {
        panic!("registerFormatter is not a function");
    }

    fn on_did_change_formatters(&self) -> Event<FormatterChangeEvent> {
        Event::none()
    }

    fn register_cached_formatter(&self, _formatter: ResourceLabelFormatter) -> DisposableRef {
        panic!("registerCachedFormatter is not a function");
    }
}

/// O `DataTransfer` de um `DragEvent` que a suíte cria.
#[derive(Default)]
struct TestDataTransfer {
    data: RefCell<Vec<(String, String)>>,
}

impl IDragDataTransfer for TestDataTransfer {
    fn clear_data(&self, format: &str) {
        self.data.borrow_mut().retain(|(key, _)| key != format);
    }

    fn set_data(&self, format: &str, data: &str) {
        self.data.borrow_mut().push((format.to_owned(), data.to_owned()));
    }

    fn drop_effect(&self) -> String {
        "none".to_owned()
    }

    fn items(&self) -> Vec<DragDataTransferItem> {
        Vec::new()
    }

    fn to_external_vs_data_transfer(&self, _disable_file_entries: bool) -> VSDataTransfer {
        VSDataTransfer::new()
    }

    fn fill_editors_drag_data(&self, _resources: &[URI]) {}
}

struct Fixture {
    guard: LeakGuard,
    container: ViewContainerRc,
    view_descriptors: Vec<ViewDescriptorRc>,
    ext_host_tree_views_shape: Rc<MockExtHostTreeViewsShape>,
    main_thread_tree_views: Rc<MainThreadTreeViews>,
    tree_view: Rc<CustomTreeView>,
    drag_service: Rc<TreeViewsDnDService<Rc<VSDataTransfer>>>,
    services: CustomTreeViewServices,
    test_extension_service: Rc<TestExtensionService>,
}

fn new_tree_view(fixture_services: &CustomTreeViewServices, id: &str) -> Rc<CustomTreeView> {
    CustomTreeView::new(id, "Test Title", "extension.id", fixture_services.clone())
}

fn register_view(container: &ViewContainerRc, id: &str, tree_view: Rc<CustomTreeView>) -> ViewDescriptorRc {
    let tree_view: Rc<dyn ITreeView> = tree_view;
    let name = LocalizedString { original: "Test View 1".to_owned(), value: "Test View 1".to_owned() };
    let view_descriptor: ViewDescriptorRc = Rc::new(IViewDescriptor { tree_view: Some(tree_view), ..IViewDescriptor::new(id, name) });
    views_registry().register_views(vec![view_descriptor.clone()], container);
    view_descriptor
}

fn new_main_thread_tree_views(guard: &LeakGuard, proxy: &Rc<MockExtHostTreeViewsShape>, extension_service: Rc<TestExtensionService>) -> Rc<MainThreadTreeViews> {
    let context: Rc<dyn IMainContext> = Rc::new(TestExtHostContext { proxy: proxy.clone() });
    let main_thread_tree_views = MainThreadTreeViews::new(
        &context,
        Rc::new(TestViewsService),
        Rc::new(TestNotificationService),
        extension_service,
        NullLogService::shared(),
        Rc::new(NullTelemetryServiceShape),
    );
    guard.add(main_thread_tree_views.clone());
    main_thread_tree_views
}

fn default_registration_options() -> TreeViewRegistrationOptions {
    TreeViewRegistrationOptions {
        show_collapse_all: false,
        can_select_many: false,
        drop_mime_types: vec![],
        drag_mime_types: vec![],
        has_handle_drag: true,
        has_handle_drop: false,
        manually_manage_checkboxes: false,
    }
}

/// O `setup` do `suite`.
async fn setup() -> Fixture {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let drag_service: Rc<TreeViewsDnDService<Rc<VSDataTransfer>>> = Rc::new(TreeViewsDnDService::new());
    let test_extension_service = Rc::new(TestExtensionService);
    let services = CustomTreeViewServices {
        log_service: NullLogService::shared(),
        telemetry_service: Rc::new(NullTelemetryServiceShape),
        extension_service: test_extension_service.clone(),
        progress_service: Rc::new(TestProgressService),
        activity_service: Rc::new(TestActivityService),
        label_service: Rc::new(MockLabelService),
        tree_views_dnd_service: drag_service.clone(),
    };
    let container = view_containers_registry().register_view_container(
        IViewContainerDescriptor::new("testContainer", LocalizedString { original: "test".to_owned(), value: "test".to_owned() }),
        ViewContainerLocation::Sidebar,
        None,
    );
    let tree_view = new_tree_view(&services, "testTree");
    guard.add(tree_view.clone());
    let view_descriptor = register_view(&container, TEST_TREE_VIEW_ID, tree_view.clone());

    let ext_host_tree_views_shape = Rc::new(MockExtHostTreeViewsShape::default());
    *ext_host_tree_views_shape.children.borrow_mut() = vec![custom_tree_item("testItem1", TreeItemCollapsibleState::Expanded)];
    let main_thread_tree_views = new_main_thread_tree_views(&guard, &ext_host_tree_views_shape, test_extension_service.clone());
    main_thread_tree_views.rpc_register_tree_view_data_provider(TEST_TREE_VIEW_ID, default_registration_options());
    test_extension_service.when_installed_extensions_registered().await.unwrap();

    Fixture {
        guard,
        container,
        view_descriptors: vec![view_descriptor],
        ext_host_tree_views_shape,
        main_thread_tree_views,
        tree_view,
        drag_service,
        services,
        test_extension_service,
    }
}

/// O `teardown`: `ViewsRegistry.deregisterViews(ViewsRegistry.getViews(container), container)`.
fn teardown(fixture: &Fixture) {
    let registry = views_registry();
    registry.deregister_views(&registry.get_views(&fixture.container), &fixture.container);
}

#[test]
fn get_children_keeps_custom_properties() {
    run_with_real_timers(async {
        let fixture = setup().await;
        let tree_view: Rc<dyn ITreeView> = views_registry().get_view(TEST_TREE_VIEW_ID).unwrap().tree_view.clone().unwrap();
        let children = tree_view.data_provider().unwrap().get_children(Some(&ITreeItem { handle: "root".to_owned(), collapsible_state: TreeItemCollapsibleState::Expanded, ..ITreeItem::default() })).await.unwrap();
        let children = children.unwrap();
        assert!(children.len() == 1, "Exactly one child should be returned");
        assert!(
            children[0].custom_properties.iter().any(|(key, value)| key == "customProp" && matches!(value, Dynamic::String(text) if text.to_string_lossy() == CUSTOM_VALUE)),
            "Tree Items should keep custom properties"
        );
        teardown(&fixture);
    });
}

#[test]
fn forwards_canceled_drag_release_to_the_extension_host() {
    run_with_real_timers(async {
        let fixture = setup().await;
        let tree_view: Rc<dyn ITreeView> = views_registry().get_view(TEST_TREE_VIEW_ID).unwrap().tree_view.clone().unwrap();
        tree_view.drag_and_drop_controller().unwrap().handle_drag_end("canceled-drag");
        assert_eq!(*fixture.ext_host_tree_views_shape.released_drag_operations.borrow(), vec!["canceled-drag".to_owned()]);
        teardown(&fixture);
    });
}

/// O corpo do `for (const teardown of ['tree', 'customer', 'view'])`.
fn releases_an_active_drag(teardown_kind: &'static str) {
    run_with_real_timers(async move {
        let fixture = setup().await;
        let tree_view = fixture.tree_view.clone();
        tree_view.set_visibility(false);
        let drag = tree_view.tree_view_dnd().unwrap();
        let event = DragEventData { data_transfer: Some(Rc::new(TestDataTransfer::default())) };
        drag.on_drag_start(&ElementsDragAndDropData::new(vec![ITreeItem { handle: "testItem1".to_owned(), collapsible_state: TreeItemCollapsibleState::None, ..ITreeItem::default() }]), &event);
        let operations = fixture.drag_service.drag_operations();
        let uuid = operations[0].0.clone();
        assert_eq!(operations.len(), 1);
        for (_, operation) in operations {
            operation.await.unwrap();
        }
        match teardown_kind {
            "tree" => {
                fixture.main_thread_tree_views.rpc_dispose_tree(TEST_TREE_VIEW_ID).await.unwrap();
            }
            "customer" => fixture.main_thread_tree_views.dispose(),
            _ => tree_view.dispose(),
        }
        assert_eq!(fixture.drag_service.drag_operations().len(), 0);
        assert!(fixture.ext_host_tree_views_shape.drag_tokens.borrow()[0].is_cancellation_requested());
        assert_eq!(*fixture.ext_host_tree_views_shape.released_drag_operations.borrow(), vec![uuid]);
        if teardown_kind != "view" {
            assert!(tree_view.drag_and_drop_controller().is_none());
            let error = errors::catch(|| fixture.main_thread_tree_views.rpc_resolve_drop_file_data(TEST_TREE_VIEW_ID, 1.0, "file")).err().expect("Unknown tree");
            assert!(error.message.contains("Unknown tree"));
        }
        teardown(&fixture);
    });
}

#[test]
fn releases_an_active_drag_on_tree_disposal() {
    releases_an_active_drag("tree");
}

#[test]
fn releases_an_active_drag_on_customer_disposal() {
    releases_an_active_drag("customer");
}

#[test]
fn releases_an_active_drag_on_view_disposal() {
    releases_an_active_drag("view");
}

#[test]
fn handle_drag_reconstructs_uri_list_from_uri_list_data() {
    run_with_real_timers(async {
        let fixture = setup().await;
        let test_tree_view_id_with_drag = "testTreeViewWithDrag";

        // Create a mock that returns URI list data
        let mock_ext_host_with_drag = Rc::new(MockExtHostTreeViewsShape::default());
        *mock_ext_host_with_drag.children.borrow_mut() = vec![ITreeItem { handle: "item1".to_owned(), collapsible_state: TreeItemCollapsibleState::None, ..ITreeItem::default() }];
        // Return a DataTransferDTO with text/uri-list containing uriListData
        // This simulates what the extension host sends after URI transformation
        let transformed = URI::from(
            &UriComponents {
                scheme: Some("file".into()),
                authority: Some("".into()),
                path: Some("/transformed/correct/path.txt".into()),
                query: Some("".into()),
                fragment: Some("".into()),
            },
            false,
        );
        *mock_ext_host_with_drag.drag_result.borrow_mut() = Some(DataTransferDto {
            items: vec![(
                mimes::URI_LIST.to_owned(),
                DataTransferItemDto {
                    id: "test-id".to_owned(),
                    // This is the original (untransformed) string - should NOT be used
                    as_string: "file:///original/untransformed/path.txt".to_owned(),
                    file_data: None,
                    // This is the transformed URI data - should be used
                    uri_list_data: Some(vec![UriListPart::Uri(transformed.clone())]),
                },
            )],
        });

        // Register a view with drag support
        let drag_test_tree_view = new_tree_view(&fixture.services, "testTree2");
        fixture.guard.add(drag_test_tree_view.clone());
        let view_descriptor_with_drag = register_view(&fixture.container, test_tree_view_id_with_drag, drag_test_tree_view.clone());

        let drag_test_main_thread_tree_views = new_main_thread_tree_views(&fixture.guard, &mock_ext_host_with_drag, fixture.test_extension_service.clone());
        drag_test_main_thread_tree_views.rpc_register_tree_view_data_provider(
            test_tree_view_id_with_drag,
            TreeViewRegistrationOptions {
                drag_mime_types: vec![mimes::URI_LIST.to_owned()],
                ..default_registration_options()
            },
        );
        fixture.test_extension_service.when_installed_extensions_registered().await.unwrap();

        // Get the tree view and its drag controller
        let drag_controller = drag_test_tree_view.drag_and_drop_controller().expect("Drag controller should exist");

        // Call handleDrag
        let result = drag_controller.handle_drag(vec!["item1".to_owned()], "test-operation-uuid", CancellationToken::None).await.unwrap().expect("Result should not be undefined");

        // Verify that the URI list was reconstructed from uriListData, not asString
        let uri_list_item = result.get(mimes::URI_LIST).expect("URI list item should exist");

        let uri_list_value = uri_list_item.as_string().await.unwrap();
        // The value should be the transformed URI, not the original untransformed one
        assert_eq!(uri_list_value, transformed.to_string(false).to_string_lossy());

        drag_test_main_thread_tree_views.dispose();
        views_registry().deregister_views(&[view_descriptor_with_drag], &fixture.container);
        teardown(&fixture);
        drop(fixture.view_descriptors);
    });
}
