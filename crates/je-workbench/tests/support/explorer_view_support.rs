//! O `setup` da suíte `context menu` de `explorerView.test.ts`, montando o [`ExplorerView`] com a árvore de teste do original.
//!
//! Divergências do `setup` do original:
//! - **`workbenchInstantiationService`.** Os serviços são os de teste do porte (`TestContextService`, `TestFileService`,
//!   `LabelService` real, `TestConfigurationService` com `{ explorer: { autoReveal: false, compactFolders: false } }`...) e os
//!   dublês dos que o original troca por `stub` (`IViewDescriptorService`, `IExplorerService`, `IContextMenuService`,
//!   `IEditorResolverService`). O que o original não define lança `TypeError: <membro> is not a function` ([`not_defined`]).
//! - **A árvore de teste e o `stubInstance`.** O original cria a sua `WorkbenchCompressibleAsyncDataTree` (renderer simples, fonte de
//!   dados com o `refreshGate`, `compressionEnabled: false`, `identityProvider` pelo recurso) e a injeta com `stubInstance`, junto
//!   com os dublês de `ResourceLabels`, `FilesFilter`, `ExplorerFindProvider`, `ExplorerDataSource`, `FileSorter`,
//!   `FileDragAndDrop` e `FilesRenderer`. Aqui o `stubInstance` é o [`ExplorerTreeFactory`] de [`ExplorerViewServices::tree_factory`]:
//!   `create_components` entrega os dublês e `create_tree` devolve a árvore de teste. O `accessibilityProvider` não existe nas
//!   opções da árvore do `je-widgets` e não entra.
//! - **O renderer e a fonte de dados.** O `FilesRenderer` do porte é também o renderer dos nós da árvore (contrato
//!   [`IExplorerRenderer`]), então o [`TestRenderer`] reúne o renderer `test` do original e o `stubInstance(FilesRenderer, {
//!   getCompressedNavigationController, dispose })`. O `resolveTopLevel` da vista chama o `getChildren` da fonte de dados que o
//!   `createTree` recebeu, e o original chama o da fonte de dados da árvore de teste no `setInput` da pasta; por isso o dublê de
//!   `ExplorerDataSource` é a própria fonte de dados de teste da árvore, com o `refreshGate`.
//! - **A entrada da árvore.** A entrada do porte é a lista dos filhos da pasta (ver `ExplorerTreeInput` em `explorer_view.rs`), então o
//!   `tree.setInput(root)` do `setup` é `set_input(root.children())`.
//! - **`renderForTesting(container)`.** `renderBody` é o [`ViewPaneDelegate::render_body`] da vista, chamado direto no contêiner.
//! - **`getContextMenuTarget` e `dispatchEvent`.** O evento de menu de contexto que a árvore emite é o
//!   [`ExplorerContextMenuEvent`] entregue a [`ExplorerView::on_context_menu`]: o item (ou nenhum), o `BrowserEvent` (botão
//!   secundário, ou o `F10` com `shift` do teclado) e o nó alvo.
//! - **Descarte da árvore.** A vista registra a árvore em `createTree` (`_register(this.tree)`) e a descarta; o `ds.add(tree)` do
//!   original só repete esse registro e não é repetido aqui.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{ById, IDisposable, none};
use je_base::nls::LocalizedString;
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::bulk_edit_service::ResourceFileEdit;
use je_editor::language_service::LanguageService;
use je_editor::model_service::ModelService;
use je_editor::undo_redo_service::UndoRedoService;
use je_platform::actions::menu_service::MenuService;
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::contextkey::service::ContextKeyService;
use je_platform::contextview::context_view::{ContextMenuRequest, IContextMenuMenuDelegate, IContextMenuService};
use je_platform::keybinding::keybinding::IKeybindingService;
use je_platform::list::list_service::ListService;
use je_platform::progress::progress::{IProgress, IProgressService, Progress, ProgressOptions, ProgressStep, ProgressTask};
use je_platform::resources::uri_file;
use je_platform::storage::storage::InMemoryStorageService;
use je_platform::storage::StorageService;
use je_platform::workspace::WorkspaceIdentifier;
use je_platform::workspaces::workspaces::WorkspaceFolderCreationData;
use je_render::Rgba;
use je_ui::{Modifiers, NodeId, PointerButton, Role, Ui};
use je_widgets::count_badge::ICountBadgeStyles;
use je_widgets::dom::create_el;
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::icon_label::IconLabelStyles;
use je_widgets::input_box::InputBoxStyles;
use je_widgets::hover_api::get_base_layer_hover_delegate;
use je_widgets::list::list_styles::IListStyles;
use je_widgets::list::list_types::{BrowserEvent, IIdentityProvider, IListVirtualDelegate, TemplateData};
use je_widgets::progress_bar::IProgressBarOptions;
use je_widgets::tree::async_data_tree_node::{AsyncDataTreeNodeWrapper, IAsyncDataTreeRenderer};
use je_widgets::tree::compressible_async_data_tree::{
    CompressibleAsyncDataTreeNodeWrapper, CompressibleAsyncDataTreeOptions, ICompressibleAsyncDataTreeRenderer, ITreeCompressionDelegate,
};
use je_widgets::tree::tree_types::{
    IAsyncDataSource, ITreeFilter, ITreeSorter, InputOrElement, TreeElementRenderDetails, TreeFilterResult, TreeMouseEventTarget, TreeNodeApi, TreeVisibility,
};
use je_workbench::api::common::ext_host_values::ApiValue;
use je_workbench::browser::labels::ResourceLabelsServices;
use je_workbench::browser::parts::views::view_pane::{IViewPaneOptions, IViewPaneToolbar, IViewWelcomeView, ViewPaneServices, ViewPaneToolbarArgs};
use je_workbench::browser::workbench_tree::{IWorkbenchTreeOptions, WorkbenchTreeServices};
use je_workbench::common::views::{
    IViewContainerModel, IViewContentDescriptor, IViewDescriptor, IViewDescriptorService, ViewContainerLocation, ViewContainerLocationChange, ViewContainerRc, ViewContainersChange,
    ViewDescriptorRc, ViewVisibilityState, ViewsContainerChange, ViewsLocationChange,
};
use je_workbench::contrib::files::browser::views::explorer_view::{
    CompressedNavigationControllerProvider, ExplorerComponents, ExplorerComponentsArgs, ExplorerContextMenuEvent, ExplorerFilter, ExplorerRendererServices, ExplorerTree,
    ExplorerTreeArgs, ExplorerTreeFactory, ExplorerView, ExplorerViewServices, IExplorerRenderer, IExplorerResourceContextKey, IExplorerViewContainerDelegate,
    IExplorerViewPaneOptions,
};
use je_workbench::contrib::files::browser::views::explorer_viewer::ICompressedNavigationController;
use je_workbench::contrib::files::browser::views::explorer_viewer_data::ExplorerInput;
use je_workbench::contrib::files::browser::views::explorer_viewer_renderer::{FileIconThemeInfo, InlineEditServices};
use je_workbench::contrib::files::browser_files::{EditableItem, ExplorerBulkEditOptions, ExplorerReveal, IExplorerService, IExplorerView};
use je_workbench::contrib::files::common_files::SortOrderConfiguration;
use je_workbench::contrib::files::explorer_model::ExplorerItem;
use je_workbench::services::editor::editor_groups_service::PreferredGroup;
use je_workbench::services::editor::editor_resolver_service::{
    EditorAssociations, EditorInputFactoryObject, EditorMatches, EditorResolverServiceGetAllEditorsOptions, EditorResolverServiceGetEditorMatchesOptions,
    EditorResolverServiceGetEditorsOptions, GlobPattern, IEditorResolverService, RegisteredEditorInfo, RegisteredEditorOptions, RegisteredEditorRegistrationInfo, ResolvedEditor,
};
use je_workbench::services::notebook::common::notebook_document_service::{INotebookDocument, INotebookDocumentService};
use je_workbench::services::workspaces::workspace_editing::{DidEnterWorkspaceEvent, IWorkspaceEditingService};
use je_workbench::test::browser::workbench_test_services::{TestEditorService, TestEnvironmentService};
use je_workbench::test::common::test_notification_telemetry::TestNotificationTelemetryService;
use je_workbench::test::common::workbench_test_services::TestLifecycleService;
use je_workbench::common::editor::UntypedEditorInput;

use crate::anything_quick_access_support::{RecordingNotificationService, TestOpenerService, TestSearchService};
use crate::common::LeakGuard;
use crate::editor_input_services::{TestAccessibilityService, WorkbenchServices, uri_identity_service};
use crate::extension_service_doubles::TestDialogService;
use crate::main_thread_support::StubTextFileService;
use crate::notification_doubles::{EmptyThemeService, NoCommandService, NoKeybindingService, TestLayoutService};
use crate::test_clipboard_service::TestClipboardService;
use crate::support::text_file_support::TestDecorationsService;

/// O membro que o objeto parcial do original não define: chamá-lo é um `TypeError`.
fn not_defined(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `TestProgressService` do `workbenchInstantiationService`: roda a tarefa na hora, com `Progress.None`.
struct TestProgressService;

impl IProgressService for TestProgressService {
    fn with_progress(&self, _options: ProgressOptions, task: ProgressTask, _on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        let progress: Rc<dyn IProgress<ProgressStep>> = Progress::<ProgressStep>::none();
        task(progress)
    }
}

/// `instantiationService.stub(IContextMenuService, { showContextMenu })`: guarda o delegado do último menu mostrado.
pub struct RecordingContextMenuService {
    menu: RefCell<Option<IContextMenuMenuDelegate>>,
}

impl RecordingContextMenuService {
    fn new() -> Rc<Self> {
        Rc::new(Self { menu: RefCell::new(None) })
    }
}

impl IContextMenuService for RecordingContextMenuService {
    fn on_did_show_context_menu(&self) -> Event<()> {
        not_defined("onDidShowContextMenu")
    }

    fn on_did_hide_context_menu(&self) -> Event<()> {
        not_defined("onDidHideContextMenu")
    }

    fn show_context_menu(&self, request: ContextMenuRequest) {
        match request {
            // assert.ok(hasKey(delegate, { menuActionOptions: true }))
            ContextMenuRequest::Menu(delegate) => {
                assert!(delegate.menu_action_options.is_some());
                *self.menu.borrow_mut() = Some(delegate);
            }
            ContextMenuRequest::Delegate(_) => panic!("o menu de contexto do explorador tem as opções do menu"),
        }
    }
}

/// `instantiationService.stub(IExplorerService, { roots, registerView, findClosest, findClosestRoot, isEditable, getEditable })`.
struct StubExplorerService {
    root: Rc<ExplorerItem>,
}

impl IExplorerService for StubExplorerService {
    fn roots(&self) -> Vec<Rc<ExplorerItem>> {
        vec![self.root.clone()]
    }

    fn sort_order_configuration(&self) -> SortOrderConfiguration {
        not_defined("sortOrderConfiguration")
    }

    fn get_context(&self, _respect_multi_selection: bool, _ignore_nested_children: Option<bool>) -> Vec<Rc<ExplorerItem>> {
        not_defined("getContext")
    }

    fn get_editable(&self) -> Option<EditableItem> {
        None
    }

    fn is_editable(&self, _stat: Option<&Rc<ExplorerItem>>) -> bool {
        false
    }

    fn find_closest(&self, _resource: &URI) -> Option<Rc<ExplorerItem>> {
        Some(self.root.clone())
    }

    fn find_closest_root(&self, _resource: &URI) -> Option<Rc<ExplorerItem>> {
        Some(self.root.clone())
    }

    fn set_to_copy(&self, _stats: Vec<Rc<ExplorerItem>>, _cut: bool) -> Promise<()> {
        not_defined("setToCopy")
    }

    fn apply_bulk_edit(&self, _edit: Vec<ResourceFileEdit>, _options: ExplorerBulkEditOptions) -> Promise<()> {
        not_defined("applyBulkEdit")
    }

    fn select(&self, _resource: &URI, _reveal: Option<ExplorerReveal>) -> Promise<()> {
        not_defined("select")
    }

    fn register_view(&self, _context_and_refresh_provider: Rc<dyn IExplorerView>) {}
}

/// `instantiationService.stub(IEditorResolverService, { getEditors: () => [] })`.
struct NoEditors;

impl IEditorResolverService for NoEditors {
    fn get_associations_for_resource(&self, _resource: &URI) -> EditorAssociations {
        not_defined("getAssociationsForResource")
    }

    fn get_editor_matches(&self, _resource: &URI, _options: Option<&EditorResolverServiceGetEditorMatchesOptions>) -> EditorMatches {
        not_defined("getEditorMatches")
    }

    fn set_default_editor(&self, _resource: &URI, _editor_id: &str, _for_diff_editor: Option<bool>) {
        not_defined("setDefaultEditor")
    }

    fn remove_diff_editor_association(&self, _resource: &URI, _filename_pattern: &str) {
        not_defined("removeDiffEditorAssociation")
    }

    fn on_did_change_editor_registrations(&self) -> Event<()> {
        Event::none()
    }

    fn buffer_change_events(&self, _callback: &dyn Fn()) {
        not_defined("bufferChangeEvents")
    }

    fn register_editor(
        &self,
        _glob_pattern: GlobPattern,
        _editor_info: RegisteredEditorRegistrationInfo,
        _options: RegisteredEditorOptions,
        _editor_factory_object: EditorInputFactoryObject,
    ) -> je_base::lifecycle::DisposableRef {
        not_defined("registerEditor")
    }

    fn resolve_editor(&self, _editor: &UntypedEditorInput, _preferred_group: Option<&PreferredGroup>) -> Promise<ResolvedEditor> {
        not_defined("resolveEditor")
    }

    fn get_editors(&self, _resource: &URI, _options: Option<&EditorResolverServiceGetEditorsOptions>) -> Vec<RegisteredEditorInfo> {
        Vec::new()
    }

    fn get_all_editors(&self, _options: Option<&EditorResolverServiceGetAllEditorsOptions>) -> Vec<RegisteredEditorInfo> {
        not_defined("getAllEditors")
    }

    fn get_binary_diff_fallback_editor(&self, _resource: &URI) -> Option<String> {
        not_defined("getBinaryDiffFallbackEditor")
    }

    fn get_all_user_associations(&self) -> EditorAssociations {
        not_defined("getAllUserAssociations")
    }
}

/// O `IWorkspaceEditingService` que o `workbenchInstantiationService` entrega: nenhum membro é usado pelo caso.
struct UnusedWorkspaceEditingService;

impl IWorkspaceEditingService for UnusedWorkspaceEditingService {
    fn on_did_enter_workspace(&self) -> Event<DidEnterWorkspaceEvent> {
        Event::none()
    }

    fn add_folders(&self, _folders: Vec<WorkspaceFolderCreationData>, _donot_notify_error: Option<bool>) -> Promise<()> {
        not_defined("addFolders")
    }

    fn remove_folders(&self, _folders: Vec<URI>, _donot_notify_error: Option<bool>) -> Promise<()> {
        not_defined("removeFolders")
    }

    fn update_folders(&self, _index: f64, _delete_count: Option<f64>, _folders_to_add: Option<Vec<WorkspaceFolderCreationData>>, _donot_notify_error: Option<bool>) -> Promise<()> {
        not_defined("updateFolders")
    }

    fn enter_workspace(&self, _path: &URI) -> Promise<()> {
        not_defined("enterWorkspace")
    }

    fn create_and_enter_workspace(&self, _folders: Vec<WorkspaceFolderCreationData>, _path: Option<URI>) -> Promise<()> {
        not_defined("createAndEnterWorkspace")
    }

    fn save_and_enter_workspace(&self, _path: &URI) -> Promise<()> {
        not_defined("saveAndEnterWorkspace")
    }

    fn copy_workspace_settings(&self, _to_workspace: &WorkspaceIdentifier) -> Promise<()> {
        not_defined("copyWorkspaceSettings")
    }

    fn pick_new_workspace_path(&self) -> Promise<Option<URI>> {
        not_defined("pickNewWorkspacePath")
    }
}

/// O `INotebookDocumentService` que os rótulos consultam: nenhum caderno aberto.
struct NoNotebookDocuments;

impl INotebookDocumentService for NoNotebookDocuments {
    fn get_notebook(&self, _uri: &URI) -> Option<Rc<dyn INotebookDocument>> {
        None
    }

    fn add_notebook_document(&self, _document: Rc<dyn INotebookDocument>) {}

    fn remove_notebook_document(&self, _document: Rc<dyn INotebookDocument>) {}
}

/// `upcastPartial<IViewDescriptorService>({ onDidChangeLocation: Event.None, getViewLocationById: () => ViewContainerLocation.Sidebar })`.
struct StubViewDescriptorService;

impl IViewDescriptorService for StubViewDescriptorService {
    fn view_containers(&self) -> Vec<ViewContainerRc> {
        not_defined("viewContainers")
    }

    fn on_did_change_view_containers(&self) -> Event<ViewContainersChange> {
        Event::none()
    }

    fn get_default_view_container(&self, _location: ViewContainerLocation) -> Option<ViewContainerRc> {
        not_defined("getDefaultViewContainer")
    }

    fn get_view_container_by_id(&self, _id: &str) -> Option<ViewContainerRc> {
        not_defined("getViewContainerById")
    }

    fn is_view_container_removed_permanently(&self, _id: &str) -> bool {
        not_defined("isViewContainerRemovedPermanently")
    }

    fn get_default_view_container_location(&self, _view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        not_defined("getDefaultViewContainerLocation")
    }

    fn get_view_container_location(&self, _view_container: &ViewContainerRc) -> Option<ViewContainerLocation> {
        not_defined("getViewContainerLocation")
    }

    fn get_view_containers_by_location(&self, _location: ViewContainerLocation) -> Vec<ViewContainerRc> {
        not_defined("getViewContainersByLocation")
    }

    fn get_view_container_model(&self, _view_container: &ViewContainerRc) -> Rc<dyn IViewContainerModel> {
        not_defined("getViewContainerModel")
    }

    fn on_did_change_container_location(&self) -> Event<ViewContainerLocationChange> {
        not_defined("onDidChangeContainerLocation")
    }

    fn move_view_container_to_location(&self, _view_container: &ViewContainerRc, _location: ViewContainerLocation, _requested_index: Option<usize>, _reason: Option<&str>) {
        not_defined("moveViewContainerToLocation")
    }

    fn get_view_container_badge_enablement_state(&self, _id: &str) -> bool {
        not_defined("getViewContainerBadgeEnablementState")
    }

    fn set_view_container_badge_enablement_state(&self, _id: &str, _badges_enabled: bool) {
        not_defined("setViewContainerBadgeEnablementState")
    }

    fn get_view_descriptor_by_id(&self, id: &str) -> Option<ViewDescriptorRc> {
        let name = LocalizedString { original: "Explorer".to_string(), value: "Explorer".to_string() };
        Some(Rc::new(IViewDescriptor::new(id, name)))
    }

    fn get_view_container_by_view_id(&self, _id: &str) -> Option<ViewContainerRc> {
        not_defined("getViewContainerByViewId")
    }

    fn get_default_container_by_id(&self, _id: &str) -> Option<ViewContainerRc> {
        not_defined("getDefaultContainerById")
    }

    fn get_view_location_by_id(&self, _id: &str) -> Option<ViewContainerLocation> {
        Some(ViewContainerLocation::Sidebar)
    }

    fn can_move_views(&self) -> bool {
        not_defined("canMoveViews")
    }

    fn on_did_change_container(&self) -> Event<ViewsContainerChange> {
        Event::none()
    }

    fn move_views_to_container(&self, _views: &[ViewDescriptorRc], _view_container: &ViewContainerRc, _visibility_state: Option<ViewVisibilityState>, _reason: Option<&str>) {
        not_defined("moveViewsToContainer")
    }

    fn on_did_change_location(&self) -> Event<ViewsLocationChange> {
        Event::none()
    }

    fn move_view_to_location(&self, _view: &ViewDescriptorRc, _location: ViewContainerLocation, _reason: Option<&str>) {
        not_defined("moveViewToLocation")
    }

    fn reset(&self) {
        not_defined("reset")
    }
}

/// A barra de ferramentas do cabeçalho: a vista de teste não desenha o cabeçalho.
struct NoToolbar;

impl IDisposable for NoToolbar {
    fn dispose(&self) {}
}

impl IViewPaneToolbar for NoToolbar {
    fn set_actions(&self, _primary_actions: &[ActionRef], _secondary_actions: &[ActionRef]) {}

    fn set_context(&self, _context: Option<Dynamic>) {}
}

/// A mensagem de boas-vindas: o `ExplorerView` de teste nunca a mostra.
struct NoWelcomeView;

impl IDisposable for NoWelcomeView {
    fn dispose(&self) {}
}

impl IViewWelcomeView for NoWelcomeView {
    fn render(&self, _contents: &[Rc<IViewContentDescriptor>]) {}

    fn layout(&self, _height: f64, _width: f64, _wide: bool) {}

    fn focus(&self) {}
}

/// O `ResourceContextKey` do `createInstance(ResourceContextKey)`.
struct NoResourceContext;

impl IDisposable for NoResourceContext {
    fn dispose(&self) {}
}

impl IExplorerResourceContextKey for NoResourceContext {
    fn set(&self, _resource: Option<&URI>) {}
}

/// `delegate: { willOpenElement: () => { }, didOpenElement: () => { } }`.
struct NoopContainerDelegate;

impl IExplorerViewContainerDelegate for NoopContainerDelegate {
    fn will_open_element(&self, _event: Option<&BrowserEvent>) {}

    fn did_open_element(&self, _event: Option<&BrowserEvent>) {}
}

/// `controllers: Map<ExplorerItem, ICompressedNavigationController[]>`.
pub type Controllers = Rc<RefCell<HashMap<ById<ExplorerItem>, Vec<Rc<dyn ICompressedNavigationController>>>>>;

/// `refreshGate: DeferredPromise<void> | undefined`.
pub type RefreshGate = Rc<RefCell<Option<Rc<DeferredPromise<()>>>>>;

/// `new class extends mock<ICompressedNavigationController>() { current; items; index; count }`: o resto lança.
pub struct MockCompressedNavigationController {
    current: Rc<ExplorerItem>,
    items: Vec<Rc<ExplorerItem>>,
    index: usize,
    count: usize,
}

impl MockCompressedNavigationController {
    pub fn new(current: Rc<ExplorerItem>, items: Vec<Rc<ExplorerItem>>, index: usize, count: usize) -> Rc<dyn ICompressedNavigationController> {
        Rc::new(Self { current, items, index, count })
    }
}

impl ICompressedNavigationController for MockCompressedNavigationController {
    fn current(&self) -> Rc<ExplorerItem> {
        self.current.clone()
    }

    fn current_id(&self) -> String {
        not_defined("currentId")
    }

    fn items(&self) -> Vec<Rc<ExplorerItem>> {
        self.items.clone()
    }

    fn index(&self) -> usize {
        self.index
    }

    fn count(&self) -> usize {
        self.count
    }

    fn on_did_change(&self) -> Event<()> {
        not_defined("onDidChange")
    }

    fn previous(&self) {
        not_defined("previous")
    }

    fn next(&self) {
        not_defined("next")
    }

    fn first(&self) {
        not_defined("first")
    }

    fn last(&self) {
        not_defined("last")
    }

    fn set_index(&self, _index: i64) {
        not_defined("setIndex")
    }

    fn update_collapsed(&self, _collapsed: bool) {
        not_defined("updateCollapsed")
    }
}

/// `{ getHeight: () => 20, getTemplateId: () => 'test' }`.
struct TestDelegate;

impl IListVirtualDelegate<Rc<ExplorerItem>> for TestDelegate {
    fn get_height(&self, _element: &Rc<ExplorerItem>) -> f64 {
        20.0
    }

    fn get_template_id(&self, _element: &Rc<ExplorerItem>) -> String {
        "test".to_string()
    }
}

/// `{ isIncompressible: item => !item.isDirectory }`.
struct TestCompressionDelegate;

impl ITreeCompressionDelegate<Rc<ExplorerItem>> for TestCompressionDelegate {
    fn is_incompressible(&self, item: &Rc<ExplorerItem>) -> bool {
        !item.is_directory()
    }
}

/// `identityProvider: { getId: item => item.resource.toString() }`.
struct ResourceIdentityProvider;

impl IIdentityProvider<Rc<ExplorerItem>> for ResourceIdentityProvider {
    fn get_id(&self, item: &Rc<ExplorerItem>) -> String {
        item.resource().to_string(false).to_string_lossy()
    }
}

/// O renderer `test` da árvore de teste (o nome do item, ou os nomes do nó comprimido separados por `/`) e o
/// `stubInstance(FilesRenderer, { getCompressedNavigationController: item => controllers.get(item), dispose: () => { } })`.
struct TestRenderer {
    ui: Ui,
    controllers: Controllers,
}

impl TestRenderer {
    /// `container.textContent = text`.
    fn set_text(&self, template_data: &TemplateData, text: String) {
        let container = *template_data.downcast_ref::<NodeId>().expect("o template do renderer de teste");
        self.ui.tree_mut().set_label(container, text);
    }
}

impl IAsyncDataTreeRenderer<ExplorerInput, Rc<ExplorerItem>, ()> for TestRenderer {
    fn template_id(&self) -> String {
        "test".to_string()
    }

    // renderTemplate: container => container
    fn render_template(&self, container: NodeId) -> TemplateData {
        Rc::new(container)
    }

    fn render_element(&self, node: &Rc<AsyncDataTreeNodeWrapper<ExplorerInput, Rc<ExplorerItem>, ()>>, _index: usize, template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {
        if let InputOrElement::Element(item) = node.element() {
            self.set_text(template_data, item.name());
        }
    }

    fn dispose_element(&self, _node: &Rc<AsyncDataTreeNodeWrapper<ExplorerInput, Rc<ExplorerItem>, ()>>, _index: i64, _template_data: &TemplateData, _details: Option<&TreeElementRenderDetails>) {}

    fn dispose_template(&self, _template_data: &TemplateData) {}
}

impl ICompressibleAsyncDataTreeRenderer<ExplorerInput, Rc<ExplorerItem>, ()> for TestRenderer {
    fn render_compressed_elements(
        &self,
        node: &Rc<CompressibleAsyncDataTreeNodeWrapper<ExplorerInput, Rc<ExplorerItem>, ()>>,
        _index: usize,
        template_data: &TemplateData,
        _details: Option<&TreeElementRenderDetails>,
    ) {
        let names: Vec<String> = node.element().elements.iter().filter_map(|element| element.as_element()).map(|item| item.name()).collect();
        self.set_text(template_data, names.join("/"));
    }
}

impl CompressedNavigationControllerProvider for TestRenderer {
    fn get_compressed_navigation_controller(&self, stat: &Rc<ExplorerItem>) -> Option<Vec<Rc<dyn ICompressedNavigationController>>> {
        self.controllers.borrow().get(&ById(stat.clone())).cloned()
    }
}

impl IDisposable for TestRenderer {
    fn dispose(&self) {}
}

impl IExplorerRenderer for TestRenderer {}

/// `hasChildren` e `getChildren` da fonte de dados da árvore de teste: `await refreshGate?.p` antes de devolver os filhos.
struct TestDataSource {
    refresh_gate: RefreshGate,
}

impl IAsyncDataSource<ExplorerInput, Rc<ExplorerItem>> for TestDataSource {
    // hasChildren: item => Array.isArray(item) || item.children.size > 0
    fn has_children(&self, element: &InputOrElement<ExplorerInput, Rc<ExplorerItem>>) -> bool {
        match element {
            InputOrElement::Input(_) => true,
            InputOrElement::Element(item) => !item.children().is_empty(),
        }
    }

    fn get_children(&self, element: &InputOrElement<ExplorerInput, Rc<ExplorerItem>>) -> MaybePromise<Vec<Rc<ExplorerItem>>> {
        let gate = self.refresh_gate.borrow().as_ref().map(|gate| gate.promise.clone());
        let element = element.clone();
        MaybePromise::Promise(async_fn(async move {
            // `await undefined` também cede uma vez
            gate.unwrap_or_else(|| Promise::resolved(())).await?;
            Ok(match element {
                InputOrElement::Input(items) => items,
                InputOrElement::Element(item) => item.children(),
            })
        }))
    }
}

/// `instantiationService.stubInstance(FilesFilter, { onDidChange: Event.None, dispose })`: o `filter` fica indefinido.
struct UndefinedFilter;

impl ITreeFilter<Rc<ExplorerItem>, ()> for UndefinedFilter {
    fn filter(&self, _element: &Rc<ExplorerItem>, _parent_visibility: TreeVisibility) -> TreeFilterResult<()> {
        not_defined("filter")
    }
}

/// `instantiationService.stubInstance(FileSorter, {})`.
struct UndefinedSorter;

impl ITreeSorter<Rc<ExplorerItem>> for UndefinedSorter {
    fn compare(&self, _element: &Rc<ExplorerItem>, _other_element: &Rc<ExplorerItem>) -> i32 {
        not_defined("compare")
    }
}

/// O `instantiationService.stubInstance(...)` do `setup`: os dublês dos componentes e a árvore de teste.
struct TestTreeFactory {
    tree: Rc<ExplorerTree>,
    renderer: Rc<TestRenderer>,
    data_source: Rc<TestDataSource>,
}

impl ExplorerTreeFactory for TestTreeFactory {
    fn create_components(&self, _services: &ExplorerViewServices, _args: ExplorerComponentsArgs) -> ExplorerComponents {
        ExplorerComponents {
            filter: ExplorerFilter { tree_filter: Rc::new(UndefinedFilter), on_did_change: Event::none(), disposable: none() },
            // stubInstance(ResourceLabels, { dispose: () => { } })
            labels: none(),
            // stubInstance(ExplorerFindProvider, { isShowingFilterResults: () => false })
            find_provider: Rc::new(|| false),
            renderer: self.renderer.clone(),
            // stubInstance(FileDragAndDrop, {})
            drag_and_drop: none(),
            data_source: self.data_source.clone(),
            sorter: Rc::new(UndefinedSorter),
        }
    }

    fn create_tree(&self, _services: &ExplorerViewServices, _args: ExplorerTreeArgs) -> Rc<ExplorerTree> {
        self.tree.clone()
    }
}

/// O estilo do texto do cabeçalho e dos rótulos.
fn label_styles() -> HighlightedLabelStyles {
    HighlightedLabelStyles {
        foreground: Rgba::opaque(0xcc, 0xcc, 0xcc),
        highlight_foreground: None,
        highlight_weight: 700,
        font_size: 13.0,
        font_weight: 400,
        line_height: Some(22.0),
        italic: false,
        strikethrough: false,
    }
}

/// O `setup` da suíte `context menu`: a raiz `/workspace` com `folder` (e `first.txt`, `second.txt`) e `target.txt`.
pub struct ExplorerViewHarness {
    pub view: Rc<ExplorerView>,
    pub ui: Ui,
    pub container: NodeId,
    pub root: Rc<ExplorerItem>,
    pub folder: Rc<ExplorerItem>,
    pub first: Rc<ExplorerItem>,
    pub second: Rc<ExplorerItem>,
    pub target: Rc<ExplorerItem>,
    menu_service: Rc<RecordingContextMenuService>,
    workbench: WorkbenchServices,
    config: Rc<TestConfigurationService>,
    controllers: Controllers,
    refresh_gate: RefreshGate,
}

impl ExplorerViewHarness {
    /// `createItem(path, isDirectory)`: `new ExplorerItem(URI.file(path), fileService, configService, NullFilesConfigurationService,
    /// undefined, isDirectory)`.
    pub fn create_item(&self, path: &str, is_directory: bool, parent: Option<&Rc<ExplorerItem>>) -> Rc<ExplorerItem> {
        create_item(&self.workbench, &self.config, path, is_directory, parent)
    }

    pub fn new(guard: &LeakGuard) -> Self {
        let config = TestConfigurationService::new(Some(Dynamic::object([(
            "explorer",
            Dynamic::object([("autoReveal", Dynamic::Bool(false)), ("compactFolders", Dynamic::Bool(false))]),
        )])));
        let config_service: Rc<dyn IConfigurationService> = config.clone();
        let workbench = WorkbenchServices::new(guard, &config);

        let root = create_item(&workbench, &config, "/workspace", true, None);
        let folder = create_item(&workbench, &config, "/workspace/folder", true, None);
        let first = create_item(&workbench, &config, "/workspace/folder/first.txt", false, None);
        let second = create_item(&workbench, &config, "/workspace/folder/second.txt", false, None);
        let target = create_item(&workbench, &config, "/workspace/target.txt", false, None);
        root.add_child(&folder);
        root.add_child(&target);
        folder.add_child(&first);
        folder.add_child(&second);

        let ui = Ui::new();
        let context_keys = guard.add_rc(ContextKeyService::new(config_service.clone()));
        let context_keys: Rc<dyn IContextKeyService> = context_keys;
        let storage: Rc<dyn StorageService> = Rc::new(guard.add_rc(InMemoryStorageService::new()));
        let command_service: Rc<dyn ICommandService> = Rc::new(NoCommandService);
        let keybinding_service: Rc<dyn IKeybindingService> = Rc::new(NoKeybindingService);
        let menu_service = RecordingContextMenuService::new();
        let lifecycle_service = guard.add_rc(TestLifecycleService::new());

        let pane = ViewPaneServices {
            ui: ui.clone(),
            keybinding_service: keybinding_service.clone(),
            context_menu_service: menu_service.clone(),
            configuration_service: config_service.clone(),
            context_key_service: context_keys.clone(),
            view_descriptor_service: Rc::new(StubViewDescriptorService),
            menu_service: MenuService::new(command_service.clone(), keybinding_service, storage.clone()),
            opener_service: TestOpenerService::new(),
            hover_service: get_base_layer_hover_delegate(),
            lifecycle_service,
            accessible_view_information_service: None,
            header_styles: label_styles(),
            description_foreground: Rgba::opaque(0x99, 0x99, 0x99),
            progress_bar_options: IProgressBarOptions::default(),
            create_toolbar: Rc::new(|_args: ViewPaneToolbarArgs| -> Rc<dyn IViewPaneToolbar> { Rc::new(NoToolbar) }),
            create_action_view_item: Rc::new(|_, _, _| None),
            create_welcome_view: Rc::new(|_| -> Rc<dyn IViewWelcomeView> { Rc::new(NoWelcomeView) }),
        };

        let language_service = guard.add_rc(LanguageService::new(false));
        let model_service = guard.add_rc(ModelService::new(
            config_service.clone(),
            je_workbench::test::common::workbench_test_services::TestTextResourcePropertiesService::new(config_service.clone()),
            UndoRedoService::for_tests(),
        ));
        let resource_labels = ResourceLabelsServices {
            ui: ui.clone(),
            configuration_service: config_service.clone(),
            model_service,
            workspace_service: workbench.context_service.clone(),
            language_service,
            decorations_service: Rc::new(TestDecorationsService),
            theme_service: Rc::new(EmptyThemeService),
            label_service: workbench.label_service.clone(),
            text_file_service: Rc::new(StubTextFileService),
            notebook_document_service: Rc::new(NoNotebookDocuments),
            icon_label_styles: Rc::new(|| IconLabelStyles { text: label_styles(), disabled_foreground: None }),
        };

        let tree_services = WorkbenchTreeServices {
            context_key_service: context_keys,
            list_service: ListService::new(),
            configuration_service: config_service,
            default_list_styles: Rc::new(IListStyles::default),
        };

        let root_node = ui.root();
        let container = create_el(&mut ui.tree_mut(), root_node, Role::Generic, "explorer-context-menu-test");
        let controllers = Controllers::default();
        let refresh_gate = RefreshGate::default();
        let renderer = Rc::new(TestRenderer { ui: ui.clone(), controllers: controllers.clone() });
        let data_source = Rc::new(TestDataSource { refresh_gate: refresh_gate.clone() });
        let mut tree_options = CompressibleAsyncDataTreeOptions::<Rc<ExplorerItem>, ()>::default();
        tree_options.compression_enabled = Some(false);
        tree_options.base.collapse_by_default = Some(Rc::new(|_: &Rc<ExplorerItem>| false));
        tree_options.base.identity_provider = Some(Rc::new(ResourceIdentityProvider));
        let tree_renderers: Vec<Rc<dyn ICompressibleAsyncDataTreeRenderer<ExplorerInput, Rc<ExplorerItem>, ()>>> = vec![renderer.clone()];
        let tree = ExplorerTree::new(
            "ExplorerContextMenuTest",
            &ui,
            container,
            Rc::new(TestDelegate),
            Rc::new(TestCompressionDelegate),
            tree_renderers,
            data_source.clone(),
            tree_options,
            IWorkbenchTreeOptions::default(),
            &tree_services,
        );
        // tree.layout(300, 400)
        tree.layout(Some(300.0), Some(400.0));

        let services = ExplorerViewServices {
            pane,
            context_service: workbench.context_service.clone(),
            progress_service: Rc::new(TestProgressService),
            notification_service: RecordingNotificationService::new(),
            editor_service: Rc::new(TestEditorService),
            editor_resolver_service: Rc::new(NoEditors),
            layout_service: Rc::new(TestLayoutService),
            label_service: workbench.label_service.clone(),
            telemetry_service: Rc::new(TestNotificationTelemetryService::new()),
            explorer_service: Rc::new(StubExplorerService { root: root.clone() }),
            storage_service: storage,
            // instantiationService.stub(IClipboardService, new TestClipboardService())
            clipboard_service: Rc::new(TestClipboardService::new()),
            file_service: workbench.file_service.clone(),
            uri_identity_service: uri_identity_service(),
            command_service,
            accessibility_service: Rc::new(TestAccessibilityService),
            environment_service: TestEnvironmentService::new(),
            files_config_service: workbench.files_configuration_service.clone(),
            search_service: Rc::new(TestSearchService),
            dialog_service: Rc::new(TestDialogService),
            workspace_editing_service: Rc::new(UnusedWorkspaceEditingService),
            tree: tree_services,
            resource_labels: Rc::new(resource_labels),
            renderer: ExplorerRendererServices {
                inline_edit: InlineEditServices { context_view_provider: None, context_menu_service: menu_service.clone(), input_box_styles: Rc::new(InputBoxStyles::default) },
                file_icon_theme: Rc::new(FileIconThemeInfo::default),
                badge_styles: ICountBadgeStyles::default(),
            },
            resource_context: Rc::new(NoResourceContext),
            on_did_color_theme_change: Event::none(),
            bind_file_icon_theme_scope: Rc::new(|_| none()),
            list_override_styles: Rc::new(|_| IListStyles::default()),
            external_file_drop: Rc::new(|_| Promise::resolved(())),
            // instantiationService.stubInstance(...)
            tree_factory: Some(Rc::new(TestTreeFactory { tree, renderer, data_source })),
        };

        let view = guard.add_rc(ExplorerView::new(
            IExplorerViewPaneOptions {
                pane: IViewPaneOptions {
                    id: "testExplorerContextMenu".to_string(),
                    pane: je_widgets::paneview::PaneOptions { title: "Explorer".to_string(), ..Default::default() },
                    ..IViewPaneOptions::default()
                },
                delegate: Some(Rc::new(NoopContainerDelegate)),
            },
            services,
        ));

        // view.renderForTesting(container)
        je_workbench::browser::parts::views::view_pane::ViewPaneDelegate::render_body(&*view, container);

        Self { view, ui, container, root, folder, first, second, target, menu_service, workbench, config, controllers, refresh_gate }
    }

    /// `await tree.setInput(root); await tree.expand(folder)` do `setup`, na árvore de teste.
    pub async fn setup(&self) {
        let tree = self.view.tree();
        tree.widget().set_input(self.root.children(), None).await.unwrap();
        tree.widget().expand(&self.folder, false).await.unwrap();
    }

    /// `controllers.set(item, controllers)`.
    pub fn set_compressed_navigation_controllers(&self, item: &Rc<ExplorerItem>, controllers: Vec<Rc<dyn ICompressedNavigationController>>) {
        self.controllers.borrow_mut().insert(ById(item.clone()), controllers);
    }

    /// `refreshGate = new DeferredPromise<void>()`: o `getChildren` seguinte espera até o portão ser completado.
    pub fn close_refresh_gate(&self) -> Rc<DeferredPromise<()>> {
        let gate = Rc::new(DeferredPromise::new());
        *self.refresh_gate.borrow_mut() = Some(gate.clone());
        gate
    }

    /// `menu`: o último delegado entregue ao `showContextMenu`.
    pub fn menu(&self) -> Option<IContextMenuMenuDelegate> {
        self.menu_service.menu.borrow().clone()
    }

    /// `menu?.menuActionOptions?.arg` quando é um `URI`.
    pub fn menu_target(&self) -> Option<URI> {
        let arg = self.menu()?.menu_action_options?.arg?;
        <URI as ApiValue>::from_dynamic(&arg).ok()
    }

    /// `menu?.getActionsContext?.()`: a lista de recursos.
    pub fn menu_selection(&self) -> Option<Vec<URI>> {
        let context = (self.menu()?.get_actions_context?)(None)?;
        let Dynamic::Array(items) = context else {
            panic!("o contexto das ações do menu é uma lista de recursos");
        };
        Some(items.to_vec().iter().map(|item| <URI as ApiValue>::from_dynamic(item).expect("um recurso")).collect())
    }

    /// O `contextmenu` do ponteiro (botão secundário) ou do teclado (`F10` com `shift`) sobre `target` (`None` é a própria árvore).
    pub fn context_menu_event(&self, item: Option<&Rc<ExplorerItem>>, keyboard: bool, target: Option<NodeId>) -> ExplorerContextMenuEvent {
        let browser_event = BrowserEvent {
            modifiers: Modifiers { shift: keyboard, ..Modifiers::default() },
            button: if keyboard { PointerButton::Primary } else { PointerButton::Secondary },
            target,
            click_count: if keyboard { 0 } else { 1 },
            is_keyboard: keyboard,
            force_event: false,
        };
        ExplorerContextMenuEvent { element: item.cloned(), browser_event, target: TreeMouseEventTarget::Element }
    }

    /// `showContextMenu(item, browserEvent)`: `tree.setFocus(item ? [item] : [])` e o `contextmenu` no alvo; resolve quando o
    /// menu foi mostrado.
    pub fn show_context_menu(&self, item: Option<&Rc<ExplorerItem>>, keyboard: bool) -> Promise<()> {
        self.view.tree().set_focus(&item.cloned().into_iter().collect::<Vec<_>>(), None);
        self.view.on_context_menu(&self.context_menu_event(item, keyboard, None))
    }

    /// `dom.append(getContextMenuTarget(target), $('input'))`: um campo de texto dentro da árvore.
    pub fn append_input(&self) -> NodeId {
        let tree_element = self.view.tree().get_html_element();
        create_el(&mut self.ui.tree_mut(), tree_element, Role::TextInput, "input")
    }
}

/// `createItem` do `setup`.
fn create_item(workbench: &WorkbenchServices, config: &Rc<TestConfigurationService>, path: &str, is_directory: bool, parent: Option<&Rc<ExplorerItem>>) -> Rc<ExplorerItem> {
    ExplorerItem::new(
        uri_file(path),
        workbench.file_service.clone(),
        config.clone(),
        workbench.files_configuration_service.clone(),
        parent,
        Some(is_directory),
        None,
        None,
        None,
        None,
        None,
        false,
    )
}
