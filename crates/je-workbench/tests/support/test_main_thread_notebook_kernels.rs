//! Porte de `src/vs/workbench/api/test/browser/TestMainThreadNotebookKernels.ts`: o `MainThreadNotebookKernels` com os serviços
//! mockados que a suíte de variáveis usa.
//!
//! Divergências:
//! - `TestInstantiationService` com `stub(ILanguageService, ...)` e companhia vira a injeção direta dos cinco dublês no construtor
//!   do [`MainThreadNotebookKernels`]. `mock<T>()` não existe em Rust: cada dublê implementa o contrato inteiro, e o membro que o
//!   original não sobrescreve é `unreachable!` (o `TypeError` do original).
//! - O `INotebookKernelService` sobrescreve `registerKernel` (guarda o kernel pelo id e devolve `Disposable.None`),
//!   `onDidChangeSelectedNotebooks = Event.None` e `getMatchingKernel` (`{ selected: undefined, suggestions: [], all: [], hidden: [] }`).
//!   Os membros que o `MainThreadNotebookKernels` chama e o original deixa sem definição falham.
//! - `INotebookExecutionStateService` devolve, em `createCellExecution` e `createExecution`, execuções em que qualquer membro falha.
//! - `INotebookEditorService` sobrescreve `listNotebookEditors() { return []; }`, `onDidAddNotebookEditor` e `onDidRemoveNotebookEditor`.
//! - O `extensionLocation: { scheme: 'test', path: '/test' }` é o `URI` desses componentes.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::buffer::VSBufferReadableStream;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable, none};
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_editor::language_service::{ILanguageSelection, ILanguageService};
use je_editor::languages::ILanguageIdCodec;
use je_editor::languages_registry::LanguageNameIdPair;
use je_editor::modes_registry::{LanguageExtensionPoint, LanguageIcon};
use je_extensions::manifest::ExtensionIdentifier;
use je_workbench::api::browser::main_thread_notebook_kernels::MainThreadNotebookKernels;
use je_workbench::api::common::ext_host_protocol::{IMainContext, INotebookKernelDto2, MainThreadNotebookKernelsShape};
use je_workbench::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;
use je_workbench::contrib::notebook::browser::services::notebook_editor_service::{IBorrowValue, INotebookEditorService};
use je_workbench::contrib::notebook::common::notebook_common::{
    INotebookContributionData, INotebookRendererInfo, INotebookStaticPreloadInfo, INotebookTextModel, NotebookCellExecutionState, NotebookExecutionState, NotebookExtensionDescription,
};
use je_workbench::contrib::notebook::common::notebook_execution_state_service::{
    ExecutionStateChange, ICellExecuteUpdate, ICellExecutionComplete, INotebookCellExecution, INotebookExecution, INotebookExecutionStateService, INotebookFailStateChangedEvent,
};
use je_workbench::contrib::notebook::common::notebook_kernel_service::{
    INotebookKernel, INotebookKernelDetectionTask, INotebookKernelMatchResult, INotebookKernelService, INotebookTextModelLike, IKernelSourceActionProvider,
    ISelectedNotebooksChangeEvent,
};
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_service::{INotebookSerializer, INotebookService, SimpleNotebookProviderInfo};

fn absent(member: &str) -> ! {
    unreachable!("TypeError: {member} is not a function")
}

// ---- ILanguageService

/// `getRegisteredLanguageIds() { return ['typescript', 'javascript', 'python']; }`.
struct StubLanguageService;

impl ILanguageService for StubLanguageService {
    fn language_id_codec(&self) -> Rc<dyn ILanguageIdCodec> {
        absent("languageIdCodec")
    }
    fn on_did_request_basic_language_features(&self) -> Event<String> {
        absent("onDidRequestBasicLanguageFeatures")
    }
    fn on_did_request_rich_language_features(&self) -> Event<String> {
        absent("onDidRequestRichLanguageFeatures")
    }
    fn on_did_change(&self) -> Event<()> {
        absent("onDidChange")
    }
    fn register_language(&self, _def: LanguageExtensionPoint) -> DisposableRef {
        absent("registerLanguage")
    }
    fn is_registered_language_id(&self, _language_id: Option<&str>) -> bool {
        absent("isRegisteredLanguageId")
    }
    fn get_registered_language_ids(&self) -> Vec<String> {
        vec!["typescript".to_owned(), "javascript".to_owned(), "python".to_owned()]
    }
    fn get_sorted_registered_language_names(&self) -> Vec<LanguageNameIdPair> {
        absent("getSortedRegisteredLanguageNames")
    }
    fn get_language_name(&self, _language_id: &str) -> Option<String> {
        absent("getLanguageName")
    }
    fn get_mime_type(&self, _language_id: &str) -> Option<String> {
        absent("getMimeType")
    }
    fn get_icon(&self, _language_id: &str) -> Option<LanguageIcon> {
        absent("getIcon")
    }
    fn get_extensions(&self, _language_id: &str) -> Vec<String> {
        absent("getExtensions")
    }
    fn get_filenames(&self, _language_id: &str) -> Vec<String> {
        absent("getFilenames")
    }
    fn get_configuration_files(&self, _language_id: &str) -> Vec<URI> {
        absent("getConfigurationFiles")
    }
    fn get_language_id_by_language_name(&self, _language_name: &str) -> Option<String> {
        absent("getLanguageIdByLanguageName")
    }
    fn get_language_id_by_mime_type(&self, _mime_type: Option<&str>) -> Option<String> {
        absent("getLanguageIdByMimeType")
    }
    fn guess_language_id_by_filepath_or_first_line(&self, _resource: Option<&URI>, _first_line: Option<&JsString>) -> Option<String> {
        absent("guessLanguageIdByFilepathOrFirstLine")
    }
    fn create_by_id(&self, _language_id: Option<&str>) -> Rc<dyn ILanguageSelection> {
        absent("createById")
    }
    fn create_by_mime_type(&self, _mime_type: Option<&str>) -> Rc<dyn ILanguageSelection> {
        absent("createByMimeType")
    }
    fn create_by_filepath_or_first_line(&self, _resource: Option<&URI>, _first_line: Option<&JsString>) -> Rc<dyn ILanguageSelection> {
        absent("createByFilepathOrFirstLine")
    }
    fn request_basic_language_features(&self, _language_id: &str) {
        absent("requestBasicLanguageFeatures")
    }
    fn request_rich_language_features(&self, _language_id: &str) {
        absent("requestRichLanguageFeatures")
    }
}

// ---- INotebookKernelService

type RegisteredKernels = Rc<RefCell<HashMap<String, Rc<dyn INotebookKernel>>>>;

/// O `INotebookKernelService` mockado.
struct StubKernelService {
    registered_kernels: RegisteredKernels,
}

impl INotebookKernelService for StubKernelService {
    fn on_did_change_selected_notebooks(&self) -> Event<ISelectedNotebooksChangeEvent> {
        Event::none()
    }

    fn register_kernel(&self, kernel: Rc<dyn INotebookKernel>) -> DisposableRef {
        self.registered_kernels.borrow_mut().insert(kernel.id(), kernel);
        none()
    }

    fn get_matching_kernel(&self, _notebook: &dyn INotebookTextModelLike) -> INotebookKernelMatchResult {
        INotebookKernelMatchResult { selected: None, suggestions: Vec::new(), all: Vec::new(), hidden: Vec::new() }
    }

    fn update_kernel_notebook_affinity(&self, _kernel: &Rc<dyn INotebookKernel>, _notebook: &URI, _preference: Option<i32>) {
        absent("updateKernelNotebookAffinity")
    }

    fn register_notebook_kernel_detection_task(&self, _task: Rc<dyn INotebookKernelDetectionTask>) -> DisposableRef {
        absent("registerNotebookKernelDetectionTask")
    }

    fn register_kernel_source_action_provider(&self, _view_type: &str, _provider: Rc<IKernelSourceActionProvider>) -> DisposableRef {
        absent("registerKernelSourceActionProvider")
    }

    fn notify_variables_change(&self, _notebook_uri: &URI) {
        absent("notifyVariablesChange")
    }
}

// ---- INotebookExecutionStateService

/// `new class extends mock<INotebookCellExecution>() { }`.
struct UnusedCellExecution;

impl INotebookCellExecution for UnusedCellExecution {
    fn notebook(&self) -> URI {
        absent("notebook")
    }
    fn cell_handle(&self) -> i64 {
        absent("cellHandle")
    }
    fn state(&self) -> NotebookCellExecutionState {
        absent("state")
    }
    fn did_pause(&self) -> bool {
        absent("didPause")
    }
    fn is_paused(&self) -> bool {
        absent("isPaused")
    }
    fn confirm(&self) {
        absent("confirm")
    }
    fn update(&self, _updates: Vec<ICellExecuteUpdate>) {
        absent("update")
    }
    fn complete(&self, _complete: ICellExecutionComplete) {
        absent("complete")
    }
}

/// `new class extends mock<INotebookExecution>() { }`.
struct UnusedExecution;

impl INotebookExecution for UnusedExecution {
    fn notebook(&self) -> URI {
        absent("notebook")
    }
    fn state(&self) -> NotebookExecutionState {
        absent("state")
    }
    fn confirm(&self) {
        absent("confirm")
    }
    fn begin(&self) {
        absent("begin")
    }
    fn complete(&self) {
        absent("complete")
    }
}

/// O `INotebookExecutionStateService` mockado.
struct StubExecutionStateService;

impl INotebookExecutionStateService for StubExecutionStateService {
    fn on_did_change_execution(&self) -> Event<ExecutionStateChange> {
        absent("onDidChangeExecution")
    }
    fn on_did_change_last_run_fail_state(&self) -> Event<INotebookFailStateChangedEvent> {
        absent("onDidChangeLastRunFailState")
    }
    fn force_cancel_notebook_executions(&self, _notebook_uri: &URI) {
        absent("forceCancelNotebookExecutions")
    }
    fn get_cell_executions_for_notebook(&self, _notebook: &URI) -> Vec<Rc<dyn INotebookCellExecution>> {
        absent("getCellExecutionsForNotebook")
    }
    fn get_cell_executions_by_handle_for_notebook(&self, _notebook: &URI) -> Option<HashMap<i64, Rc<dyn INotebookCellExecution>>> {
        absent("getCellExecutionsByHandleForNotebook")
    }
    fn get_cell_execution(&self, _cell_uri: &URI) -> Option<Rc<dyn INotebookCellExecution>> {
        absent("getCellExecution")
    }
    fn create_cell_execution(&self, _notebook: &URI, _cell_handle: i64) -> Rc<dyn INotebookCellExecution> {
        Rc::new(UnusedCellExecution)
    }
    fn get_execution(&self, _notebook: &URI) -> Option<Rc<dyn INotebookExecution>> {
        absent("getExecution")
    }
    fn create_execution(&self, _notebook: &URI) -> Rc<dyn INotebookExecution> {
        Rc::new(UnusedExecution)
    }
    fn get_last_failed_cell_for_notebook(&self, _notebook: &URI) -> Option<i64> {
        absent("getLastFailedCellForNotebook")
    }
    fn get_last_completed_cell_for_notebook(&self, _notebook: &URI) -> Option<i64> {
        absent("getLastCompletedCellForNotebook")
    }
}

// ---- INotebookService e INotebookEditorService

/// `getNotebookTextModel() { return undefined; }`.
struct StubNotebookService;

impl INotebookService for StubNotebookService {
    fn can_resolve(&self, _view_type: &str) -> Promise<bool> {
        absent("canResolve")
    }
    fn on_add_view_type(&self) -> Event<String> {
        absent("onAddViewType")
    }
    fn on_will_remove_view_type(&self) -> Event<String> {
        absent("onWillRemoveViewType")
    }
    fn on_did_change_output_renderers(&self) -> Event<()> {
        absent("onDidChangeOutputRenderers")
    }
    fn on_will_add_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        absent("onWillAddNotebookDocument")
    }
    fn on_did_add_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        absent("onDidAddNotebookDocument")
    }
    fn on_will_remove_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        absent("onWillRemoveNotebookDocument")
    }
    fn on_did_remove_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        absent("onDidRemoveNotebookDocument")
    }
    fn register_notebook_serializer(&self, _view_type: &str, _extension_data: NotebookExtensionDescription, _serializer: Rc<dyn INotebookSerializer>) -> DisposableRef {
        absent("registerNotebookSerializer")
    }
    fn with_notebook_data_provider(&self, _view_type: &str) -> Promise<Rc<SimpleNotebookProviderInfo>> {
        absent("withNotebookDataProvider")
    }
    fn try_get_data_provider_sync(&self, _view_type: &str) -> Option<Rc<SimpleNotebookProviderInfo>> {
        absent("tryGetDataProviderSync")
    }
    fn get_view_type_provider(&self, _view_type: &str) -> Option<String> {
        absent("getViewTypeProvider")
    }
    fn get_renderer_info(&self, _id: &str) -> Option<Rc<dyn INotebookRendererInfo>> {
        absent("getRendererInfo")
    }
    fn get_renderers(&self) -> Vec<Rc<dyn INotebookRendererInfo>> {
        absent("getRenderers")
    }
    fn get_static_preloads(&self, _view_type: &str) -> Vec<INotebookStaticPreloadInfo> {
        absent("getStaticPreloads")
    }
    fn create_notebook_text_model(&self, _view_type: &str, _uri: &URI, _stream: Option<VSBufferReadableStream>) -> Promise<Rc<NotebookTextModel>> {
        absent("createNotebookTextModel")
    }
    fn get_notebook_text_model(&self, _uri: &URI) -> Option<Rc<NotebookTextModel>> {
        None
    }
    fn get_notebook_text_models(&self) -> Vec<Rc<NotebookTextModel>> {
        absent("getNotebookTextModels")
    }
    fn list_notebook_documents(&self) -> Vec<Rc<NotebookTextModel>> {
        absent("listNotebookDocuments")
    }
    fn register_contributed_notebook_type(&self, _view_type: &str, _data: INotebookContributionData) -> DisposableRef {
        absent("registerContributedNotebookType")
    }
    fn has_supported_notebooks(&self, _resource: &URI) -> bool {
        absent("hasSupportedNotebooks")
    }
    fn get_notebook_provider_resource_roots(&self) -> Vec<URI> {
        absent("getNotebookProviderResourceRoots")
    }
    fn clear_editor_cache(&self) {
        absent("clearEditorCache")
    }
}

/// `listNotebookEditors() { return []; }`, `onDidAddNotebookEditor = Event.None` e `onDidRemoveNotebookEditor = Event.None`.
struct StubNotebookEditorService;

impl INotebookEditorService for StubNotebookEditorService {
    fn on_did_add_notebook_editor(&self) -> Event<Rc<dyn NotebookEditorWidget>> {
        Event::none()
    }

    fn retrieve_existing_widget_from_uri(&self, _resource: &URI) -> Option<IBorrowValue<dyn NotebookEditorWidget>> {
        absent("retrieveExistingWidgetFromUri")
    }

    fn on_did_remove_notebook_editor(&self) -> Event<Rc<dyn NotebookEditorWidget>> {
        Event::none()
    }

    fn list_notebook_editors(&self) -> Vec<Rc<dyn NotebookEditorWidget>> {
        Vec::new()
    }
}

// ---- TestMainThreadNotebookKernels

/// `TestMainThreadNotebookKernels`.
pub struct TestMainThreadNotebookKernels {
    registered_kernels: RegisteredKernels,
    main_thread_notebook_kernels: Rc<MainThreadNotebookKernels>,
    kernel_handle: Cell<u32>,
}

impl TestMainThreadNotebookKernels {
    pub fn new(ext_host_context: &dyn IMainContext) -> Rc<TestMainThreadNotebookKernels> {
        let registered_kernels: RegisteredKernels = Rc::default();
        let main_thread_notebook_kernels = MainThreadNotebookKernels::new(
            ext_host_context,
            Rc::new(StubLanguageService),
            Rc::new(StubKernelService { registered_kernels: registered_kernels.clone() }),
            Rc::new(StubExecutionStateService),
            Rc::new(StubNotebookService),
            Rc::new(StubNotebookEditorService),
        );
        Rc::new(TestMainThreadNotebookKernels { registered_kernels, main_thread_notebook_kernels, kernel_handle: Cell::new(0) })
    }

    /// `get instance()`.
    pub fn instance(&self) -> Rc<MainThreadNotebookKernels> {
        self.main_thread_notebook_kernels.clone()
    }

    /// `addKernel(id)`.
    pub fn add_kernel(&self, id: &str) -> Promise<()> {
        let handle = self.kernel_handle.get();
        self.kernel_handle.set(handle + 1);
        self.instance().rpc_add_kernel(
            handle,
            INotebookKernelDto2 {
                id: id.to_owned(),
                notebook_type: "test-notebook".to_owned(),
                extension_id: ExtensionIdentifier::new("test.extension"),
                extension_location: URI::from(&UriComponents { scheme: Some(JsString::from("test")), path: Some(JsString::from("/test")), ..UriComponents::default() }, false),
                label: "Test Kernel".to_owned(),
                description: Some("A test kernel".to_owned()),
                detail: None,
                supported_languages: None,
                supports_interrupt: None,
                supports_execution_order: None,
                preloads: None,
                has_variable_provider: Some(true),
            },
        )
    }

    /// `getKernel(id)`.
    pub fn get_kernel(&self, id: &str) -> Option<Rc<dyn INotebookKernel>> {
        self.registered_kernels.borrow().get(id).cloned()
    }
}

impl IDisposable for TestMainThreadNotebookKernels {
    fn dispose(&self) {
        self.main_thread_notebook_kernels.dispose();
    }
}
