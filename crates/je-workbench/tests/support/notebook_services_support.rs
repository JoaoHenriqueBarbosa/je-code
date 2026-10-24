//! Os mocks das suítes `NotebookExecutionStateService`, `NotebookKernelService` e `NotebookExecutionService`
//! (`notebookExecutionStateService.test.ts`, `notebookKernelService.test.ts`, `notebookExecutionService.test.ts`): o
//! `INotebookService`, o `IMenuService` e o `IMenu` mockados com `new class extends mock<...>() { ... }`, o
//! `INotebookKernelHistoryService`, e o `TestNotebookKernel` (que cada suíte declara no fim do arquivo, com pequenas
//! diferenças).
//!
//! Divergências:
//! - `mock<T>()` é um objeto sem membros: ler um que o caso não sobrescreveu lança `TypeError`. Aqui cada membro não
//!   sobrescrito é `unreachable!` com a mesma mensagem.
//! - `TestNotebookKernel` guarda as propriedades que os casos mudam depois de criar o objeto (`id`, `implementsInterrupt`,
//!   `executeNotebookCellsRequest` e `cancelNotebookCellExecution`) em células e `RefCell`s; o `class extends` que
//!   sobrescreve um método é a mesma struct com o fecho trocado.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::AsyncIterableProducer;
use je_base::buffer::VSBufferReadableStream;
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::path::Platform;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_action::platform::{AccessibilitySignal, IAccessibilitySignalService};
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::actions::actions::{IMenu, IMenuService, MenuActionGroup, MenuActionOptions, MenuChangeEvent, MenuCreateOptions, MenuId};
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::storage::StorageService;
use je_workbench::api::common::ext_host_protocol::{VariablesRequestKindDto, VariablesResult};
use je_workbench::contrib::notebook::browser::services::notebook_kernel_service_impl::NotebookKernelService;
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::{
    INotebookContributionData, INotebookRendererInfo, INotebookStaticPreloadInfo, NotebookExtensionDescription,
};
use je_workbench::contrib::notebook::common::notebook_kernel_service::{
    INotebookKernel, INotebookKernelChangeEvent, INotebookKernelHistory, INotebookKernelHistoryService, INotebookKernelService, INotebookTextModelLike,
};
use je_workbench::contrib::notebook::common::notebook_service::{INotebookSerializer, INotebookService, SimpleNotebookProviderInfo};
use je_workbench::test::common::workbench_test_services::TestStorageService;

fn absent(member: &str) -> ! {
    unreachable!("TypeError: {member} is not a function")
}

/// `instantiationService.stub(INotebookService, new class extends mock<INotebookService>() { ... })`: só
/// `onDidAddNotebookDocument`, `onWillRemoveNotebookDocument`, `getNotebookTextModels` e `getNotebookTextModel` respondem.
pub struct NotebookServiceMock {
    pub on_did_add_notebook_document: Event<Rc<NotebookTextModel>>,
    pub on_will_remove_notebook_document: Event<Rc<NotebookTextModel>>,
    /// `getNotebookTextModel(uri)`; `None` é o membro ausente do `mock`.
    pub get_notebook_text_model: Option<Box<dyn Fn(&URI) -> Option<Rc<NotebookTextModel>>>>,
}

impl INotebookService for NotebookServiceMock {
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
        self.on_did_add_notebook_document.clone()
    }
    fn on_will_remove_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        self.on_will_remove_notebook_document.clone()
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
    fn get_notebook_text_model(&self, uri: &URI) -> Option<Rc<NotebookTextModel>> {
        match &self.get_notebook_text_model {
            Some(get_notebook_text_model) => get_notebook_text_model(uri),
            None => absent("getNotebookTextModel"),
        }
    }
    fn get_notebook_text_models(&self) -> Vec<Rc<NotebookTextModel>> {
        Vec::new()
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

/// `new class extends mock<IMenu>() { onDidChange = Event.None; getActions() { return []; } dispose() { } }`.
struct MenuMock;

impl IDisposable for MenuMock {
    fn dispose(&self) {}
}

impl IMenu for MenuMock {
    fn on_did_change(&self) -> Event<MenuChangeEvent> {
        Event::none()
    }

    fn get_actions(&self, _options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup> {
        Vec::new()
    }
}

/// `new class extends mock<IMenuService>() { createMenu() { return menu } }`.
pub struct MenuServiceMock;

impl IMenuService for MenuServiceMock {
    fn create_menu(&self, _id: &MenuId, _context_key_service: Rc<dyn IContextKeyService>, _options: Option<MenuCreateOptions>) -> Rc<dyn IMenu> {
        Rc::new(MenuMock)
    }

    fn get_menu_actions(&self, _id: &MenuId, _context_key_service: &dyn IContextKeyService, _options: Option<&MenuActionOptions>) -> Vec<MenuActionGroup> {
        absent("getMenuActions")
    }

    fn get_menu_contexts(&self, _id: &MenuId) -> std::collections::HashSet<String> {
        absent("getMenuContexts")
    }

    fn reset_hidden_states(&self, _menu_ids: Option<&[MenuId]>) {
        absent("resetHiddenStates")
    }
}

/// `IAccessibilitySignalService`: o serviço real toca um som; o mock não faz nada.
pub struct AccessibilitySignalServiceMock;

impl IAccessibilitySignalService for AccessibilitySignalServiceMock {
    fn play_signal(&self, _signal: AccessibilitySignal) {}
}

/// `instantiationService.stub(INotebookKernelHistoryService, new class extends mock<...>() { getKernels(notebook) { return
/// kernelService.getMatchingKernel(notebook); } addMostRecentKernel(kernel) { } })`.
pub struct KernelHistoryServiceMock {
    pub kernel_service: Rc<dyn INotebookKernelService>,
}

impl INotebookKernelHistoryService for KernelHistoryServiceMock {
    fn get_kernels(&self, notebook: &dyn INotebookTextModelLike) -> INotebookKernelHistory {
        let info = self.kernel_service.get_matching_kernel(notebook);
        INotebookKernelHistory { selected: info.selected, all: info.all }
    }

    fn add_most_recent_kernel(&self, _kernel: &Rc<dyn INotebookKernel>) {}
}

/// `disposables.add(instantiationService.createInstance(NotebookKernelService))` com o armazenamento em memória do
/// `setupInstantiationService` (`TestStorageService`), o `IMenuService` mockado e o `ContextKeyService` de teste.
pub fn create_kernel_service(disposables: &DisposableStore, notebook_service: Rc<dyn INotebookService>) -> Rc<NotebookKernelService> {
    let storage = TestStorageService::new();
    disposables.add_rc(storage.clone());
    let storage_service: Rc<dyn StorageService> = Rc::new(storage);
    let menu_service: Rc<dyn IMenuService> = Rc::new(MenuServiceMock);
    let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
    disposables.add_rc(NotebookKernelService::new(notebook_service, storage_service, menu_service, context_key_service))
}

/// `class TestNotebookKernel implements INotebookKernel`.
pub struct TestNotebookKernel {
    pub id: RefCell<String>,
    pub label: String,
    pub view_type: String,
    pub supported_languages: Vec<String>,
    pub implements_interrupt: Cell<Option<bool>>,
    pub execute_notebook_cells_request: RefCell<Box<dyn Fn(&URI, Vec<i64>) -> Promise<()>>>,
    pub cancel_notebook_cell_execution: RefCell<Box<dyn Fn(&URI, Vec<i64>) -> Promise<()>>>,
}

/// `opts` do construtor: `{ languages?, id?, label?, viewType? }`.
#[derive(Default)]
pub struct TestNotebookKernelOptions {
    pub languages: Option<Vec<&'static str>>,
    pub id: Option<String>,
    pub label: Option<&'static str>,
    pub view_type: Option<&'static str>,
}

impl TestNotebookKernel {
    /// `id = 'test'`, `label = ''`, `viewType = '*'` e `supportedLanguages = [PLAINTEXT_LANGUAGE_ID]` por padrão.
    pub fn new(opts: TestNotebookKernelOptions) -> Rc<Self> {
        Rc::new(Self {
            id: RefCell::new(opts.id.unwrap_or_else(|| "test".to_string())),
            label: opts.label.unwrap_or("").to_string(),
            view_type: opts.view_type.unwrap_or("*").to_string(),
            supported_languages: opts.languages.map(|languages| languages.into_iter().map(str::to_string).collect()).unwrap_or_else(|| vec!["plaintext".to_string()]),
            implements_interrupt: Cell::new(None),
            execute_notebook_cells_request: RefCell::new(Box::new(|_, _| Promise::resolved(()))),
            cancel_notebook_cell_execution: RefCell::new(Box::new(|_, _| Promise::resolved(()))),
        })
    }
}

impl INotebookKernel for TestNotebookKernel {
    fn id(&self) -> String {
        self.id.borrow().clone()
    }

    fn view_type(&self) -> String {
        self.view_type.clone()
    }

    fn on_did_change(&self) -> Event<INotebookKernelChangeEvent> {
        Event::none()
    }

    fn extension(&self) -> ExtensionIdentifier {
        ExtensionIdentifier::new("test")
    }

    fn local_resource_root(&self) -> URI {
        URI::file("/test", Platform::Posix)
    }

    fn preload_uris(&self) -> Vec<URI> {
        Vec::new()
    }

    fn preload_provides(&self) -> Vec<String> {
        Vec::new()
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn description(&self) -> Option<String> {
        None
    }

    fn detail(&self) -> Option<String> {
        None
    }

    fn supported_languages(&self) -> Vec<String> {
        self.supported_languages.clone()
    }

    fn implements_interrupt(&self) -> Option<bool> {
        self.implements_interrupt.get()
    }

    fn implements_execution_order(&self) -> Option<bool> {
        None
    }

    fn has_variable_provider(&self) -> Option<bool> {
        None
    }

    fn execute_notebook_cells_request(&self, uri: &URI, cell_handles: Vec<i64>) -> Promise<()> {
        (self.execute_notebook_cells_request.borrow())(uri, cell_handles)
    }

    fn cancel_notebook_cell_execution(&self, uri: &URI, cell_handles: Vec<i64>) -> Promise<()> {
        (self.cancel_notebook_cell_execution.borrow())(uri, cell_handles)
    }

    fn provide_variables(
        &self,
        _notebook_uri: &URI,
        _parent_id: Option<u32>,
        _kind: VariablesRequestKindDto,
        _start: usize,
        _token: CancellationToken,
    ) -> Rc<AsyncIterableProducer<VariablesResult>> {
        AsyncIterableProducer::empty()
    }
}
