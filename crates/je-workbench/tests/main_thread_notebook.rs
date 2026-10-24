//! Porte de `src/vs/workbench/api/test/browser/mainThreadNotebook.test.ts` (suíte `MainThreadNotebooks`), caso a caso e na mesma
//! ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo do caso; o `store.add(service)` é o `dispose()` do
//!   serviço no corpo do caso (o original o descarta na própria suíte, e o serviço só tem o `DisposableStore` que o guarda vê).
//! - `mock<T>()` (`base/test/common/mock.ts`) não existe em Rust: cada dublê implementa o contrato inteiro, e o membro que o original
//!   não sobrescreve é `unreachable!` (o `TypeError` do original). `AnyCallRPCProtocol<ExtHostNotebookShape>()` é o contexto de
//!   [`TestMainContext`] com um dublê de `ExtHostNotebookShape` que devolve o proxy para qualquer identificador de notebook; o caso
//!   não chama o host, então nenhum método dele é lido.
//! - `{ dispose: () => registrationDisposals++ }` é o [`CountingDisposable`], que não é rastreado pelo guarda de vazamento (o literal
//!   do original também não é).
//! - O caso é `async` e aguarda as promessas de `$register...` e `$unregister...`: roda em `run_with_faked_timers`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::buffer::{VSBuffer, VSBufferReadableStream};
use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::log::NullLogService;
use je_workbench::api::browser::main_thread_notebook::MainThreadNotebooks;
use je_workbench::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostNotebookDocumentsAndEditorsShape, ExtHostNotebookShape, IMainContext, INotebookCellStatusBarListDto, INotebookDocumentsAndEditorsDelta,
    MainThreadNotebookShape, NotebookDataDto, NotebookSaveResult, SerializableObjectWithBuffers,
};
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_cell_status_bar_service::INotebookCellStatusBarService;
use je_workbench::contrib::notebook::common::notebook_common::{
    INotebookCellStatusBarItemList, INotebookCellStatusBarItemProvider, INotebookContributionData, INotebookRendererInfo, INotebookStaticPreloadInfo,
    NotebookExtensionDescription,
};
use je_workbench::contrib::notebook::common::notebook_service::{INotebookSerializer, INotebookService, SimpleNotebookProviderInfo};
use je_platform::files::files::WriteFileOptions;
use support::ext_host_workspace_support::TestMainContext;

/// `{ dispose: () => registrationDisposals++ }`.
struct CountingDisposable {
    disposals: Rc<Cell<u32>>,
}

impl IDisposable for CountingDisposable {
    fn dispose(&self) {
        self.disposals.set(self.disposals.get() + 1);
    }
}

/// `new class extends mock<INotebookCellStatusBarService>() { override registerCellStatusBarItemProvider(...) { ... } }`.
struct CountingCellStatusBarService {
    registration_disposals: Rc<Cell<u32>>,
}

impl INotebookCellStatusBarService for CountingCellStatusBarService {
    fn on_did_change_providers(&self) -> Event<()> {
        unreachable!("onDidChangeProviders não é definido no mock")
    }

    fn on_did_change_items(&self) -> Event<()> {
        unreachable!("onDidChangeItems não é definido no mock")
    }

    fn register_cell_status_bar_item_provider(&self, _provider: Rc<dyn INotebookCellStatusBarItemProvider>) -> DisposableRef {
        Rc::new(CountingDisposable { disposals: self.registration_disposals.clone() })
    }

    fn get_status_bar_items_for_cell(&self, _doc_uri: &URI, _cell_index: usize, _view_type: &str, _token: &CancellationToken) -> Promise<Vec<INotebookCellStatusBarItemList>> {
        unreachable!("getStatusBarItemsForCell não é definido no mock")
    }
}

/// `new class extends mock<INotebookService>() { }`.
struct UnusedNotebookService;

impl INotebookService for UnusedNotebookService {
    fn can_resolve(&self, _view_type: &str) -> Promise<bool> {
        unreachable!("canResolve não é definido no mock")
    }

    fn on_add_view_type(&self) -> Event<String> {
        unreachable!("onAddViewType não é definido no mock")
    }

    fn on_will_remove_view_type(&self) -> Event<String> {
        unreachable!("onWillRemoveViewType não é definido no mock")
    }

    fn on_did_change_output_renderers(&self) -> Event<()> {
        unreachable!("onDidChangeOutputRenderers não é definido no mock")
    }

    fn on_will_add_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        unreachable!("onWillAddNotebookDocument não é definido no mock")
    }

    fn on_did_add_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        unreachable!("onDidAddNotebookDocument não é definido no mock")
    }

    fn on_will_remove_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        unreachable!("onWillRemoveNotebookDocument não é definido no mock")
    }

    fn on_did_remove_notebook_document(&self) -> Event<Rc<NotebookTextModel>> {
        unreachable!("onDidRemoveNotebookDocument não é definido no mock")
    }

    fn register_notebook_serializer(&self, _view_type: &str, _extension_data: NotebookExtensionDescription, _serializer: Rc<dyn INotebookSerializer>) -> DisposableRef {
        unreachable!("registerNotebookSerializer não é definido no mock")
    }

    fn with_notebook_data_provider(&self, _view_type: &str) -> Promise<Rc<SimpleNotebookProviderInfo>> {
        unreachable!("withNotebookDataProvider não é definido no mock")
    }

    fn try_get_data_provider_sync(&self, _view_type: &str) -> Option<Rc<SimpleNotebookProviderInfo>> {
        unreachable!("tryGetDataProviderSync não é definido no mock")
    }

    fn get_view_type_provider(&self, _view_type: &str) -> Option<String> {
        unreachable!("getViewTypeProvider não é definido no mock")
    }

    fn get_renderer_info(&self, _id: &str) -> Option<Rc<dyn INotebookRendererInfo>> {
        unreachable!("getRendererInfo não é definido no mock")
    }

    fn get_renderers(&self) -> Vec<Rc<dyn INotebookRendererInfo>> {
        unreachable!("getRenderers não é definido no mock")
    }

    fn get_static_preloads(&self, _view_type: &str) -> Vec<INotebookStaticPreloadInfo> {
        unreachable!("getStaticPreloads não é definido no mock")
    }

    fn create_notebook_text_model(&self, _view_type: &str, _uri: &URI, _stream: Option<VSBufferReadableStream>) -> Promise<Rc<NotebookTextModel>> {
        unreachable!("createNotebookTextModel não é definido no mock")
    }

    fn get_notebook_text_model(&self, _uri: &URI) -> Option<Rc<NotebookTextModel>> {
        unreachable!("getNotebookTextModel não é definido no mock")
    }

    fn get_notebook_text_models(&self) -> Vec<Rc<NotebookTextModel>> {
        unreachable!("getNotebookTextModels não é definido no mock")
    }

    fn list_notebook_documents(&self) -> Vec<Rc<NotebookTextModel>> {
        unreachable!("listNotebookDocuments não é definido no mock")
    }

    fn register_contributed_notebook_type(&self, _view_type: &str, _data: INotebookContributionData) -> DisposableRef {
        unreachable!("registerContributedNotebookType não é definido no mock")
    }

    fn has_supported_notebooks(&self, _resource: &URI) -> bool {
        unreachable!("hasSupportedNotebooks não é definido no mock")
    }

    fn get_notebook_provider_resource_roots(&self) -> Vec<URI> {
        unreachable!("getNotebookProviderResourceRoots não é definido no mock")
    }

    fn clear_editor_cache(&self) {
        unreachable!("clearEditorCache não é definido no mock")
    }
}

/// O proxy de `AnyCallRPCProtocol<ExtHostNotebookShape>()`: o caso não o chama.
struct AnyCallExtHostNotebook;

impl ExtHostNotebookDocumentsAndEditorsShape for AnyCallExtHostNotebook {
    fn rpc_accept_document_and_editors_delta(&self, _delta: SerializableObjectWithBuffers<INotebookDocumentsAndEditorsDelta>) {}
}

impl ExtHostNotebookShape for AnyCallExtHostNotebook {
    fn rpc_provide_notebook_cell_status_bar_items(&self, _handle: u32, _uri: &URI, _index: usize, _token: CancellationToken) -> Promise<Option<INotebookCellStatusBarListDto>> {
        unreachable!("$provideNotebookCellStatusBarItems não é chamado no caso")
    }

    fn rpc_release_notebook_cell_status_bar_items(&self, _id: u32) {}

    fn rpc_data_to_notebook(&self, _handle: u32, _data: VSBuffer, _token: CancellationToken) -> Promise<SerializableObjectWithBuffers<NotebookDataDto>> {
        unreachable!("$dataToNotebook não é chamado no caso")
    }

    fn rpc_notebook_to_data(&self, _handle: u32, _data: SerializableObjectWithBuffers<NotebookDataDto>, _token: CancellationToken) -> Promise<VSBuffer> {
        unreachable!("$notebookToData não é chamado no caso")
    }

    fn rpc_save_notebook(&self, _handle: u32, _uri: &URI, _version_id: i64, _options: WriteFileOptions, _token: CancellationToken) -> Promise<NotebookSaveResult> {
        unreachable!("$saveNotebook não é chamado no caso")
    }
}

#[test]
fn disposes_and_clears_cell_status_bar_provider_registrations() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async {
        let registration_disposals = Rc::new(Cell::new(0));
        let cell_status_bar_service: Rc<dyn INotebookCellStatusBarService> = Rc::new(CountingCellStatusBarService { registration_disposals: registration_disposals.clone() });

        let rpc = Rc::new(TestMainContext::new());
        rpc.set(&ExtHostContext::EXT_HOST_NOTEBOOK, Rc::new(AnyCallExtHostNotebook) as Rc<dyn ExtHostNotebookShape>);
        let context: Rc<dyn IMainContext> = rpc;
        let service = MainThreadNotebooks::new(&context, Rc::new(UnusedNotebookService), cell_status_bar_service, NullLogService::shared());

        service.rpc_register_notebook_cell_status_bar_item_provider(1, None, "*").await.expect("o registro resolve");
        service.dispose();
        service.rpc_unregister_notebook_cell_status_bar_item_provider(1, None).await.expect("o cancelamento resolve");

        assert_eq!(registration_disposals.get(), 1);
    });
}
