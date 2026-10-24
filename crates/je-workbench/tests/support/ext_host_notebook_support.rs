//! Dublês e auxiliares dos testes do notebook no host de extensões (`extHostNotebook.test.ts`,
//! `extHostNotebookSerializer.test.ts` e `extHostNotebookStatusCommands.test.ts`).
//!
//! `new TestRPCProtocol()` é o [`TestMainContext`]; o `mock<T>()` de cada `rpcProtocol.set(...)` é um dublê com os métodos de
//! que o caso precisa (o que o original não sobrescreve falha com `panic!`, como o método ausente do `mock` lançaria
//! `TypeError`). `ExtHostConsumerFileSystem` e `ExtHostSearch` não existem no porte: o controlador recebe o dublê
//! [`UnusedConsumerFileSystem`] (nenhum caso grava ou lê arquivo) e não recebe a busca (ver `ext_host_notebook.rs`).

use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::files::files::Stat;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_commands::ExtHostCommands;
use je_workbench::api::common::ext_host_document_data::IExtHostDocumentSaveDelegate;
use je_workbench::api::common::ext_host_documents::ExtHostDocuments;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_notebook::{ExtHostNotebookController, IExtHostConsumerFileSystem, NotebookSerializer};
use je_workbench::api::common::ext_host_notebook_document::ExtHostNotebookDocument;
use je_workbench::api::common::ext_host_notebook_documents::ExtHostNotebookDocuments;
use je_workbench::api::common::ext_host_protocol::{
    CommandArgs, CreateDocumentOptions, ExtHostNotebookDocumentsAndEditorsShape, INotebookDocumentShowOptions,
    INotebookDocumentsAndEditorsDelta, INotebookEditorAddData, INotebookModelAddedData, IExtHostTelemetry, MainContext, MainThreadCommandsShape,
    MainThreadDocumentsShape, MainThreadNotebookDocumentsShape, MainThreadNotebookEditorsShape, MainThreadNotebookShape, NotebookCellDto,
    NotebookCreateOptions, NotebookOutputDto, NotebookOutputItemDto, OpenDocumentOptions, SerializableObjectWithBuffers,
};
use je_workbench::api::common::ext_host_type_converters::CellRange;
use je_workbench::api::common::ext_host_types as types;
use je_workbench::contrib::notebook::common::notebook_common::{
    CellKind, INotebookContributionData, NotebookExtensionDescription, TransientOptions,
};
use je_workbench::services::extensions::extensions::null_extension_description;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::resources::ExtUri;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::log::LogServiceRef;

use super::ext_host_workspace_support::TestMainContext;

#[path = "../../../je-base/tests/common/mod.rs"]
pub mod common;

pub use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};

/// `new class extends mock<MainThreadCommandsShape>() { override $registerCommand() { } }`.
pub struct MockMainThreadCommands;

impl IDisposable for MockMainThreadCommands {
    fn dispose(&self) {}
}

impl MainThreadCommandsShape for MockMainThreadCommands {
    fn rpc_register_command(&self, _id: &str) {}

    fn rpc_unregister_command(&self, _id: &str) {
        panic!("$unregisterCommand is not a function")
    }

    fn rpc_fire_command_activation_event(&self, _id: &str) {
        panic!("$fireCommandActivationEvent is not a function")
    }

    fn rpc_execute_command(&self, _id: &str, _args: CommandArgs, _retry: bool) -> Promise<Dynamic> {
        panic!("$executeCommand is not a function")
    }

    fn rpc_get_commands(&self) -> Promise<Vec<String>> {
        panic!("$getCommands is not a function")
    }
}

/// O `MainThreadDocuments` do contexto de teste do original (`TestRPCProtocol` sem objeto registrado): nenhum caso o chama.
pub struct UnusedMainThreadDocuments;

impl IDisposable for UnusedMainThreadDocuments {
    fn dispose(&self) {}
}

impl IExtHostDocumentSaveDelegate for UnusedMainThreadDocuments {
    fn rpc_try_save_document(&self, _uri: &URI) -> Promise<bool> {
        panic!("$trySaveDocument is not a function")
    }
}

impl MainThreadDocumentsShape for UnusedMainThreadDocuments {
    fn rpc_try_create_document(&self, _options: Option<CreateDocumentOptions>) -> Promise<URI> {
        panic!("$tryCreateDocument is not a function")
    }

    fn rpc_try_open_document(&self, _uri: &URI, _options: Option<OpenDocumentOptions>) -> Promise<URI> {
        panic!("$tryOpenDocument is not a function")
    }
}

/// `new class extends mock<MainThreadNotebookShape>() { override async $registerNotebookSerializer() { } ... }`. Guarda os
/// handles que `$registerNotebookSerializer` recebeu e os que `$registerNotebookCellStatusBarItemProvider` recebeu.
#[derive(Default)]
pub struct MockMainThreadNotebook {
    pub serializer_handles: std::cell::RefCell<Vec<u32>>,
    pub status_bar_handles: std::cell::RefCell<Vec<u32>>,
}

impl IDisposable for MockMainThreadNotebook {
    fn dispose(&self) {}
}

impl MainThreadNotebookShape for MockMainThreadNotebook {
    fn rpc_register_notebook_serializer(
        &self,
        handle: u32,
        _extension: NotebookExtensionDescription,
        _view_type: &str,
        _options: TransientOptions,
        _registration: Option<INotebookContributionData>,
    ) {
        self.serializer_handles.borrow_mut().push(handle);
    }

    fn rpc_unregister_notebook_serializer(&self, _handle: u32) {}

    fn rpc_register_notebook_cell_status_bar_item_provider(&self, handle: u32, _event_handle: Option<u32>, _view_type: &str) -> Promise<()> {
        self.status_bar_handles.borrow_mut().push(handle);
        Promise::resolved(())
    }

    fn rpc_unregister_notebook_cell_status_bar_item_provider(&self, _handle: u32, _event_handle: Option<u32>) -> Promise<()> {
        Promise::resolved(())
    }

    fn rpc_emit_cell_status_bar_event(&self, _event_handle: u32) {
        panic!("$emitCellStatusBarEvent is not a function")
    }
}

/// `new class extends mock<MainThreadNotebookDocumentsShape>() { }`.
pub struct MockMainThreadNotebookDocuments;

impl IDisposable for MockMainThreadNotebookDocuments {
    fn dispose(&self) {}
}

impl MainThreadNotebookDocumentsShape for MockMainThreadNotebookDocuments {
    fn rpc_try_create_notebook(&self, _options: NotebookCreateOptions) -> Promise<URI> {
        panic!("$tryCreateNotebook is not a function")
    }

    fn rpc_try_open_notebook(&self, _uri: &URI) -> Promise<URI> {
        panic!("$tryOpenNotebook is not a function")
    }

    fn rpc_try_save_notebook(&self, _uri: &URI) -> Promise<bool> {
        panic!("$trySaveNotebook is not a function")
    }
}

/// `MainThreadNotebookEditors` que falha se chamado.
pub struct MockMainThreadNotebookEditors;

impl IDisposable for MockMainThreadNotebookEditors {
    fn dispose(&self) {}
}

impl MainThreadNotebookEditorsShape for MockMainThreadNotebookEditors {
    fn rpc_try_show_notebook_document(&self, _uri: &URI, _view_type: &str, _options: INotebookDocumentShowOptions) -> Promise<String> {
        panic!("$tryShowNotebookDocument is not a function")
    }

    fn rpc_try_reveal_range(&self, _id: &str, _range: CellRange, _reveal_type: types::NotebookEditorRevealType) -> Promise<()> {
        panic!("$tryRevealRange is not a function")
    }

    fn rpc_try_set_selections(&self, _id: &str, _range: Vec<CellRange>) -> Promise<()> {
        panic!("$trySetSelections is not a function")
    }
}

/// `new class extends mock<IExtHostTelemetry>() { override onExtensionError(): boolean { return true; } }`.
pub struct TestExtHostTelemetry;

impl IExtHostTelemetry for TestExtHostTelemetry {
    fn on_extension_error(&self, _extension: &ExtensionIdentifier, _error: &Error) -> bool {
        true
    }
}

/// O `ExtHostConsumerFileSystem` do original: nenhum caso grava, lê ou consulta arquivo.
pub struct UnusedConsumerFileSystem;

impl IExtHostConsumerFileSystem for UnusedConsumerFileSystem {
    fn is_writable_file_system(&self, _scheme: &str) -> Option<bool> {
        panic!("isWritableFileSystem is not a function")
    }

    fn write_file(&self, _uri: &URI, _content: Vec<u8>) -> Promise<()> {
        panic!("writeFile is not a function")
    }

    fn stat(&self, _uri: &URI) -> Promise<Stat> {
        panic!("stat is not a function")
    }

    fn get_file_system_provider_ext_uri(&self, _scheme: &str) -> ExtUri {
        panic!("getFileSystemProviderExtUri is not a function")
    }
}

/// `new class extends mock<vscode.NotebookSerializer>() { }`: nenhum caso o chama.
pub struct UnusedNotebookSerializer;

impl NotebookSerializer for UnusedNotebookSerializer {
    fn deserialize_notebook(&self, _content: &[u8], _token: CancellationToken) -> je_base::promise::MaybePromise<types::NotebookData> {
        panic!("deserializeNotebook is not a function")
    }

    fn serialize_notebook(&self, _data: &types::NotebookData, _token: CancellationToken) -> je_base::promise::MaybePromise<Vec<u8>> {
        panic!("serializeNotebook is not a function")
    }
}

/// O `TestRPCProtocol` com os proxies que o host de notebook pede, e o `NullLogService`.
pub fn new_notebook_rpc() -> (Rc<TestMainContext>, Rc<MockMainThreadNotebook>) {
    let rpc = Rc::new(TestMainContext::new());
    rpc.set(&MainContext::MAIN_THREAD_COMMANDS, Rc::new(MockMainThreadCommands) as Rc<dyn MainThreadCommandsShape>);
    rpc.set(&MainContext::MAIN_THREAD_DOCUMENTS, Rc::new(UnusedMainThreadDocuments) as Rc<dyn MainThreadDocumentsShape>);
    let notebook = Rc::new(MockMainThreadNotebook::default());
    rpc.set(&MainContext::MAIN_THREAD_NOTEBOOK, notebook.clone() as Rc<dyn MainThreadNotebookShape>);
    rpc.set(&MainContext::MAIN_THREAD_NOTEBOOK_DOCUMENTS, Rc::new(MockMainThreadNotebookDocuments) as Rc<dyn MainThreadNotebookDocumentsShape>);
    rpc.set(&MainContext::MAIN_THREAD_NOTEBOOK_EDITORS, Rc::new(MockMainThreadNotebookEditors) as Rc<dyn MainThreadNotebookEditorsShape>);
    (rpc, notebook)
}

/// O que `extHostNotebook.test.ts` e `extHostNotebookSerializer.test.ts` montam: documentos e editores de texto, documentos,
/// comandos e o controlador.
pub struct NotebookHost {
    pub rpc: Rc<TestMainContext>,
    pub main_thread_notebook: Rc<MockMainThreadNotebook>,
    pub documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
    pub documents: Rc<ExtHostDocuments>,
    pub commands: Rc<ExtHostCommands>,
    pub notebooks: Rc<ExtHostNotebookController>,
}

pub fn new_notebook_host() -> NotebookHost {
    let (rpc, main_thread_notebook) = new_notebook_rpc();
    let log_service: LogServiceRef = NullLogService::shared();
    let documents_and_editors = Rc::new(ExtHostDocumentsAndEditors::new(rpc.clone(), log_service.clone()));
    let documents = ExtHostDocuments::new(rpc.as_ref(), documents_and_editors.clone());
    let commands = ExtHostCommands::new(rpc.as_ref(), log_service.clone(), Rc::new(TestExtHostTelemetry));
    let notebooks = ExtHostNotebookController::new(
        rpc.as_ref(),
        &commands,
        documents_and_editors.clone(),
        documents.clone(),
        Rc::new(UnusedConsumerFileSystem),
        log_service,
    );
    NotebookHost { rpc, main_thread_notebook, documents_and_editors, documents, commands, notebooks }
}

/// `{ handle, uri: CellUri.generate(notebookUri, handle), source, eol: '\n', language, cellKind, outputs: [] }`.
pub fn cell_dto(notebook_uri: &URI, handle: i64, source: &[&str], language: &str, cell_kind: CellKind) -> NotebookCellDto {
    NotebookCellDto {
        handle,
        uri: je_workbench::contrib::notebook::common::notebook_common::CellUri::generate(notebook_uri, handle),
        eol: "\n".to_owned(),
        source: source.iter().map(|line| (*line).to_owned()).collect(),
        language: language.to_owned(),
        mime: None,
        cell_kind,
        outputs: Vec::new(),
        metadata: None,
        internal_metadata: None,
    }
}

/// O `$acceptDocumentAndEditorsDelta` de `setup`: o notebook de `notebookUri` (um cabeçalho markdown e um código com duas
/// linhas), o editor `_notebook_editor_0` e esse editor como ativo.
pub fn add_notebook_and_editor(notebooks: &ExtHostNotebookController, notebook_uri: &URI) {
    notebooks.rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(INotebookDocumentsAndEditorsDelta {
        added_documents: Some(vec![INotebookModelAddedData {
            uri: notebook_uri.clone(),
            view_type: "test".to_owned(),
            version_id: 0,
            cells: vec![
                cell_dto(notebook_uri, 0, &["### Heading"], "markdown", CellKind::Markup),
                cell_dto(notebook_uri, 1, &["console.log(\"aaa\")", "console.log(\"bbb\")"], "javascript", CellKind::Code),
            ],
            metadata: None,
        }]),
        added_editors: Some(vec![INotebookEditorAddData {
            document_uri: notebook_uri.clone(),
            id: "_notebook_editor_0".to_owned(),
            selections: vec![CellRange { start: 0.0, end: 1.0 }],
            visible_ranges: Vec::new(),
            view_column: None,
            view_type: "test".to_owned(),
        }]),
        ..INotebookDocumentsAndEditorsDelta::default()
    }));
    notebooks.rpc_accept_document_and_editors_delta(SerializableObjectWithBuffers::new(INotebookDocumentsAndEditorsDelta {
        new_active_editor: Some(Some("_notebook_editor_0".to_owned())),
        ..INotebookDocumentsAndEditorsDelta::default()
    }));
}

/// O ambiente de `NotebookCell#Document` (`setup` de `extHostNotebook.test.ts`).
pub struct NotebookEnv {
    pub host: NotebookHost,
    pub notebook_documents: Rc<ExtHostNotebookDocuments>,
    pub notebook: Rc<ExtHostNotebookDocument>,
    pub notebook_uri: URI,
    pub guard: LeakGuard,
}

impl NotebookEnv {
    /// `setup(async function () { ... })` com o `ensureNoDisposablesAreLeakedInTestSuite()` já ativo.
    pub fn new() -> NotebookEnv {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let notebook_uri = URI::parse("test:///notebook.file", false);
        let host = new_notebook_host();
        let notebook_documents = ExtHostNotebookDocuments::new(host.notebooks.clone());

        let reg = host.notebooks.register_notebook_serializer(&null_extension_description(), "test", Rc::new(UnusedNotebookSerializer), None, None).expect("registerNotebookSerializer");
        add_notebook_and_editor(&host.notebooks, &notebook_uri);

        let notebook = host.notebooks.notebook_documents()[0].clone();

        guard.add(reg);
        guard.add_rc(notebook.clone());
        guard.add_rc(host.documents.clone());
        NotebookEnv { host, notebook_documents, notebook, notebook_uri, guard }
    }
}

/// `{ mime, valueBytes: VSBuffer.fromString(text) }`.
pub fn output_item(mime: &str, text: &str) -> NotebookOutputItemDto {
    NotebookOutputItemDto { mime: mime.to_owned(), value_bytes: VSBuffer::from_string(text) }
}

/// `{ outputId, items }`.
pub fn output_dto(output_id: &str, items: Vec<NotebookOutputItemDto>) -> NotebookOutputDto {
    NotebookOutputDto { output_id: output_id.to_owned(), items, metadata: None }
}

/// `model.getValue().split(eol)`.
pub fn lines_of(text: &str) -> Vec<JsString> {
    text.split('\n').map(JsString::from).collect()
}
