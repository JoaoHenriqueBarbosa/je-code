//! Porte de `src/vs/workbench/api/common/extHostNotebook.ts`: o controlador de notebooks do lado das extensões
//! (`ExtHostNotebookController`): documentos e editores de notebook, serializadores, provedores de itens da barra de
//! status das células, abrir, criar, mostrar e salvar notebooks.
//!
//! Divergências:
//! - `ExtHostSearch` e `$searchInNotebooks` NÃO estão portados: o método depende de `ExtHostSearch`
//!   (`doInternalFileSearchWithCustomCallback`), de `CellSearchModel` e de `searchNotebookHelpers`, que não existem no porte. O
//!   construtor perde o parâmetro `_extHostSearch` (só o método de busca o usava) e [`ExtHostNotebookShape`] não declara o método.
//! - `_registerApiCommands` (`vscode.executeDataToNotebook` e `vscode.executeNotebookToData`) NÃO está portado: os argumentos e
//!   o resultado desses comandos são `NotebookData`/`NotebookDataDto` como valor [`Dynamic`], e essa representação não existe no
//!   porte (os conversores trabalham sobre os tipos tipados).
//! - `IExtHostConsumerFileSystem` (`extHostFileSystemConsumer.ts`) é o trait [`IExtHostConsumerFileSystem`], só com o que o
//!   controlador usa; o indireto `.value` do original some (os métodos de `FileSystemExt` são os do próprio trait).
//!   `isWritableFileSystem` devolve `Option<bool>` (`boolean | undefined`: `None` conta como não gravável).
//! - Os objetos da API (`vscode.NotebookSerializer`, `vscode.NotebookCellStatusBarItemProvider`, `vscode.NotebookRegistrationData`,
//!   `vscode.NotebookDocumentShowOptions`) são os traits e structs deste arquivo. `ProviderResult<X | X[]>` é
//!   [`NotebookCellStatusBarItems`]; `NotebookData | Thenable<NotebookData>` é [`MaybePromise`].
//! - `registerNotebookSerializer` e `registerNotebookCellStatusBarItemProvider` devolvem o descartável (o primeiro lança com o
//!   `viewType` vazio: devolve [`Result`]). A contagem de handles do provedor da barra de status é estática ([`AtomicU32`]).
//! - `_notebookStatusBarItemProviderHandlePool` e `_handlePool` são contadores; o `Cache` guarda o `DisposableStore` do resultado.
//! - O `getNotebookDocument(uri, relaxed)` com duas sobrecargas é [`ExtHostNotebookController::get_notebook_document`] (lança) e
//!   [`ExtHostNotebookController::try_get_notebook_document`] (`relaxed`).
//! - O processador de argumentos devolve a célula e o notebook como objetos [`Dynamic`] com o objeto do host ligado
//!   ([`ObjectRef::with_host`]); quem os lê usa `host::<ExtHostCell>()` e `host::<ExtHostNotebookDocument>()`.
//! - `new Error(...)`/`throw` dos `$accept*` é pânico com [`Error`] ([`errors::throw`]). `NotebookSaveError` é um [`Error`] com
//!   `name = "NotebookSaveError"`. O `{ ...error, message: error.message }` do `FileOperationError` é
//!   [`NotebookSaveResult::Error`].
//! - O `localize` do original usa o texto em inglês (`nls::localize`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU32, Ordering};

use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::async_::{async_fn, await_maybe};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable, DisposableRef};
use je_base::map::{OrderedMap, ResourceMap};
use je_base::marshalling_ids::MarshalledId;
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::resources::ExtUri;
use je_base::strings::is_falsy_or_whitespace;
use je_base::types::assert_returns_defined;
use je_base::uri::{URI, UriComponents};
use je_extensions::scanner::ExtensionDescription;
use je_platform::files::files::{
    ETAG_DISABLED, FileOperationError, FileOperationResult, FilePermission, FileType, Stat, WriteFileOptions, etag,
};
use je_platform::log::LogServiceRef;
use je_platform::resources::fs_path;

use super::cache::Cache;
use super::ext_host_commands::{ArgumentProcessor, CommandsConverter, ExtHostCommands};
use super::ext_host_documents::ExtHostDocuments;
use super::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use super::ext_host_notebook_document::{ExtHostCell, ExtHostNotebookDocument, NotebookDocument};
use super::ext_host_notebook_editor::{ExtHostNotebookEditor, NotebookEditor};
use super::ext_host_protocol::{
    DocumentsAndEditorsDelta, ExtHostNotebookDocumentsAndEditorsShape, ExtHostNotebookShape, IMainContext, INotebookCellStatusBarListDto,
    INotebookDocumentShowOptions, INotebookDocumentsAndEditorsDelta, INotebookEditorAddData, INotebookPartialFileStatsWithMetadata, MainContext,
    MainThreadNotebookDocumentsShape, MainThreadNotebookEditorsShape, MainThreadNotebookShape, NotebookCreateOptions, NotebookDataDto,
    NotebookSaveResult, SerializableObjectWithBuffers,
};
use super::ext_host_type_converters as type_converters;
use super::ext_host_type_converters::notebook::{NotebookExclusiveDocumentPattern, NotebookFilenamePatternInput, NotebookStatusBarItem};
use super::ext_host_types as types;
use crate::contrib::notebook::common::notebook_common::{INotebookContributionData, NotebookExtensionDescription};
use crate::services::editor::editor_resolver_service::RegisteredEditorPriority;

/// `vscode.NotebookSerializer`.
pub trait NotebookSerializer {
    fn deserialize_notebook(&self, content: &[u8], token: CancellationToken) -> MaybePromise<types::NotebookData>;
    fn serialize_notebook(&self, data: &types::NotebookData, token: CancellationToken) -> MaybePromise<Vec<u8>>;
}

/// `NotebookCellStatusBarItem | NotebookCellStatusBarItem[]`.
#[derive(Clone, Debug)]
pub enum NotebookCellStatusBarItems {
    One(types::NotebookCellStatusBarItem),
    Many(Vec<types::NotebookCellStatusBarItem>),
}

/// `vscode.NotebookCellStatusBarItemProvider`.
pub trait NotebookCellStatusBarItemProvider {
    /// `onDidChangeCellStatusBarItems` (opcional no original).
    fn on_did_change_cell_status_bar_items(&self) -> Option<Event<()>> {
        None
    }

    fn provide_cell_status_bar_items(
        &self,
        cell: &super::ext_host_notebook_document::NotebookCell,
        token: CancellationToken,
    ) -> MaybePromise<Option<NotebookCellStatusBarItems>>;
}

/// `vscode.NotebookRegistrationData`.
#[derive(Clone, Debug)]
pub struct NotebookRegistrationData {
    pub display_name: String,
    pub filename_pattern: Vec<NotebookFilenamePatternInput>,
    pub exclusive: Option<bool>,
}

/// `asRepl?: boolean | string | { label: string }` de `vscode.NotebookDocumentShowOptions`.
#[derive(Clone, Debug)]
pub enum NotebookAsRepl {
    Flag(bool),
    Label(String),
}

/// `vscode.NotebookDocumentShowOptions`.
#[derive(Clone, Debug, Default)]
pub struct NotebookDocumentShowOptions {
    pub view_column: Option<f64>,
    pub preserve_focus: Option<bool>,
    pub selections: Option<Vec<types::NotebookRange>>,
    pub preview: Option<bool>,
    pub as_repl: Option<NotebookAsRepl>,
}

/// O que o controlador usa de `IExtHostConsumerFileSystem` (e de `FileSystemExt`, a propriedade `value`).
pub trait IExtHostConsumerFileSystem {
    fn is_writable_file_system(&self, scheme: &str) -> Option<bool>;
    fn write_file(&self, uri: &URI, content: Vec<u8>) -> Promise<()>;
    fn stat(&self, uri: &URI) -> Promise<Stat>;
    fn get_file_system_provider_ext_uri(&self, scheme: &str) -> ExtUri;
}

/// O registro de um serializador: `{ viewType, serializer, options }`.
struct SerializerEntry {
    #[allow(dead_code)]
    view_type: String,
    serializer: Rc<dyn NotebookSerializer>,
    options: Option<types::NotebookDocumentContentOptions>,
}

/// `NotebookSaveError`.
pub fn notebook_save_error(message: impl Into<String>) -> Error {
    let mut error = Error::new(message);
    error.name = "NotebookSaveError".to_owned();
    error
}

/// `ExtHostNotebookController._notebookStatusBarItemProviderHandlePool`.
static STATUS_BAR_ITEM_PROVIDER_HANDLE_POOL: AtomicU32 = AtomicU32::new(0);

/// O `URI` de um valor `UriComponents` serializado (o `notebookUri` e o `uri` dos contextos de ação).
fn uri_of(value: &Dynamic) -> Option<URI> {
    if !matches!(value, Dynamic::Object(_)) {
        return None;
    }
    let component = |key: &str| match value.get_str(key) {
        Dynamic::String(text) if !text.is_empty() => Some(text),
        _ => None,
    };
    let scheme = component("scheme")?;
    Some(URI::from(&UriComponents { scheme: Some(scheme), authority: component("authority"), path: component("path"), query: component("query"), fragment: component("fragment") }, false))
}

/// O processador de argumentos do controlador (`Serialized INotebookCellActionContext`).
struct NotebookArgumentProcessor {
    controller: Weak<ExtHostNotebookController>,
}

impl ArgumentProcessor for NotebookArgumentProcessor {
    fn process_argument(&self, arg: &Dynamic, _extension: Option<&ExtensionDescription>) -> Dynamic {
        let Some(controller) = self.controller.upgrade() else {
            return arg.clone();
        };
        if let Dynamic::Object(_) = arg {
            let mid = arg.get_str("$mid");
            if mid.strict_equals(&Dynamic::Number(f64::from(MarshalledId::NotebookCellActionContext as i32))) {
                let notebook_editor = arg.get_str("notebookEditor");
                let notebook_uri = if notebook_editor.is_nullish() { Dynamic::Undefined } else { notebook_editor.get_str("notebookUri") };
                let cell_handle = match arg.get_str("cell").get_str("handle") {
                    Dynamic::Number(handle) => Some(handle as i64),
                    _ => None,
                };
                let cell = uri_of(&notebook_uri)
                    .and_then(|uri| controller.try_get_notebook_document(&uri))
                    .zip(cell_handle)
                    .and_then(|(data, handle)| data.get_cell(handle));
                if let Some(cell) = cell {
                    return Dynamic::Object(ObjectRef::with_host(cell));
                }
            }
            if mid.strict_equals(&Dynamic::Number(f64::from(MarshalledId::NotebookActionContext as i32))) {
                if let Some(data) = uri_of(&arg.get_str("uri")).and_then(|uri| controller.try_get_notebook_document(&uri)) {
                    return Dynamic::Object(ObjectRef::with_host(data));
                }
            }
        }
        arg.clone()
    }
}

/// `ExtHostNotebookController`.
pub struct ExtHostNotebookController {
    this: Weak<ExtHostNotebookController>,

    notebook_proxy: Rc<dyn MainThreadNotebookShape>,
    notebook_documents_proxy: Rc<dyn MainThreadNotebookDocumentsShape>,
    notebook_editors_proxy: Rc<dyn MainThreadNotebookEditorsShape>,

    notebook_status_bar_item_providers: RefCell<HashMap<u32, Rc<dyn NotebookCellStatusBarItemProvider>>>,
    documents: RefCell<ResourceMap<Rc<ExtHostNotebookDocument>>>,
    editors: RefCell<OrderedMap<String, Rc<ExtHostNotebookEditor>>>,
    commands_converter: Rc<CommandsConverter>,

    on_did_change_active_notebook_editor: Rc<Emitter<Option<NotebookEditor>>>,
    active_notebook_editor: RefCell<Option<Rc<ExtHostNotebookEditor>>>,
    visible_notebook_editors: RefCell<Vec<Rc<ExtHostNotebookEditor>>>,

    on_did_open_notebook_document: Rc<Emitter<NotebookDocument>>,
    on_did_close_notebook_document: Rc<Emitter<NotebookDocument>>,
    on_did_change_visible_notebook_editors: Rc<Emitter<Vec<NotebookEditor>>>,

    status_bar_cache: Cache<Rc<DisposableStore>>,

    handle_pool: std::cell::Cell<u32>,
    notebook_serializer: RefCell<HashMap<u32, Rc<SerializerEntry>>>,

    text_documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
    text_documents: Rc<ExtHostDocuments>,
    ext_host_file_system: Rc<dyn IExtHostConsumerFileSystem>,
    log_service: LogServiceRef,
}

impl ExtHostNotebookController {
    pub fn new(
        main_context: &dyn IMainContext,
        commands: &Rc<ExtHostCommands>,
        text_documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
        text_documents: Rc<ExtHostDocuments>,
        ext_host_file_system: Rc<dyn IExtHostConsumerFileSystem>,
        log_service: LogServiceRef,
    ) -> Rc<ExtHostNotebookController> {
        let controller = Rc::new_cyclic(|this| ExtHostNotebookController {
            this: this.clone(),
            notebook_proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_NOTEBOOK),
            notebook_documents_proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_NOTEBOOK_DOCUMENTS),
            notebook_editors_proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_NOTEBOOK_EDITORS),
            notebook_status_bar_item_providers: RefCell::new(HashMap::new()),
            documents: RefCell::new(ResourceMap::new()),
            editors: RefCell::new(OrderedMap::new()),
            commands_converter: commands.converter(),
            on_did_change_active_notebook_editor: Emitter::new(),
            active_notebook_editor: RefCell::new(None),
            visible_notebook_editors: RefCell::new(Vec::new()),
            on_did_open_notebook_document: Emitter::new(),
            on_did_close_notebook_document: Emitter::new(),
            on_did_change_visible_notebook_editors: Emitter::new(),
            status_bar_cache: Cache::new("NotebookCellStatusBarCache"),
            handle_pool: std::cell::Cell::new(0),
            notebook_serializer: RefCell::new(HashMap::new()),
            text_documents_and_editors,
            text_documents,
            ext_host_file_system,
            log_service,
        });

        commands.register_argument_processor(Rc::new(NotebookArgumentProcessor { controller: Rc::downgrade(&controller) }));
        controller
    }

    /// O `Rc` deste controlador, para as operações assíncronas que precisam dele depois do retorno.
    fn rc(&self) -> Rc<ExtHostNotebookController> {
        self.this.upgrade().expect("o controlador está vivo enquanto um método dele roda")
    }

    /// `onDidChangeActiveNotebookEditor`.
    pub fn on_did_change_active_notebook_editor(&self) -> Event<Option<NotebookEditor>> {
        self.on_did_change_active_notebook_editor.event()
    }

    /// `onDidOpenNotebookDocument`.
    pub fn on_did_open_notebook_document(&self) -> Event<NotebookDocument> {
        self.on_did_open_notebook_document.event()
    }

    /// `onDidCloseNotebookDocument`.
    pub fn on_did_close_notebook_document(&self) -> Event<NotebookDocument> {
        self.on_did_close_notebook_document.event()
    }

    /// `onDidChangeVisibleNotebookEditors`.
    pub fn on_did_change_visible_notebook_editors(&self) -> Event<Vec<NotebookEditor>> {
        self.on_did_change_visible_notebook_editors.event()
    }

    /// `activeNotebookEditor`.
    pub fn active_notebook_editor(&self) -> Option<NotebookEditor> {
        self.active_notebook_editor.borrow().as_ref().map(ExtHostNotebookEditor::api_editor)
    }

    /// `visibleNotebookEditors`.
    pub fn visible_notebook_editors(&self) -> Vec<NotebookEditor> {
        self.visible_notebook_editors.borrow().iter().map(ExtHostNotebookEditor::api_editor).collect()
    }

    /// `getEditorById(editorId)`: lança se o editor não existe.
    pub fn get_editor_by_id(&self, editor_id: &str) -> Rc<ExtHostNotebookEditor> {
        let editors = self.editors.borrow();
        match editors.get(&editor_id.to_owned()) {
            Some(editor) => editor.clone(),
            None => {
                let known: Vec<String> = editors.keys().cloned().collect();
                drop(editors);
                errors::throw(Error::new(format!("unknown text editor: {editor_id}. known editors: {} ", known.join(","))))
            }
        }
    }

    /// `getIdByEditor(editor)`.
    pub fn get_id_by_editor(&self, editor: &NotebookEditor) -> Option<String> {
        self.editors.borrow().iter().find(|(_, candidate)| Rc::ptr_eq(candidate, editor.ext_host())).map(|(id, _)| id.clone())
    }

    /// `notebookDocuments`.
    pub fn notebook_documents(&self) -> Vec<Rc<ExtHostNotebookDocument>> {
        self.documents.borrow().values().cloned().collect()
    }

    /// `getNotebookDocument(uri)`: lança se o documento não existe.
    pub fn get_notebook_document(&self, uri: &URI) -> Rc<ExtHostNotebookDocument> {
        self.try_get_notebook_document(uri).unwrap_or_else(|| errors::throw(Error::new(format!("NO notebook document for '{}'", uri.to_string(false)))))
    }

    /// `getNotebookDocument(uri, relaxed: true)`.
    pub fn try_get_notebook_document(&self, uri: &URI) -> Option<Rc<ExtHostNotebookDocument>> {
        self.documents.borrow().get(uri).cloned()
    }

    fn convert_notebook_registration_data(extension: &ExtensionDescription, registration: Option<&NotebookRegistrationData>) -> Option<INotebookContributionData> {
        let registration = registration?;
        let view_options_filename_pattern = registration.filename_pattern.iter().map(NotebookExclusiveDocumentPattern::from).collect();
        Some(INotebookContributionData {
            extension: Some(extension.identifier.clone()),
            provider_display_name: extension.manifest.display_name.clone().filter(|name| !name.is_empty()).unwrap_or_else(|| extension.manifest.name.clone()),
            display_name: registration.display_name.clone(),
            filename_pattern: view_options_filename_pattern,
            priority: registration.exclusive.unwrap_or(false).then_some(RegisteredEditorPriority::Exclusive),
        })
    }

    pub fn register_notebook_cell_status_bar_item_provider(
        &self,
        _extension: &ExtensionDescription,
        notebook_type: &str,
        provider: Rc<dyn NotebookCellStatusBarItemProvider>,
    ) -> types::Disposable {
        let handle = STATUS_BAR_ITEM_PROVIDER_HANDLE_POOL.fetch_add(1, Ordering::SeqCst);
        let on_did_change = provider.on_did_change_cell_status_bar_items();
        let event_handle = on_did_change.as_ref().map(|_| STATUS_BAR_ITEM_PROVIDER_HANDLE_POOL.fetch_add(1, Ordering::SeqCst));

        self.notebook_status_bar_item_providers.borrow_mut().insert(handle, provider);
        let _ = self.notebook_proxy.rpc_register_notebook_cell_status_bar_item_provider(handle, event_handle, notebook_type);

        let subscription: Option<DisposableRef> = match (on_did_change, event_handle) {
            (Some(event), Some(event_handle)) => {
                let proxy = self.notebook_proxy.clone();
                Some(event.listen(move |_| proxy.rpc_emit_cell_status_bar_event(event_handle)))
            }
            _ => None,
        };

        let this = self.this.clone();
        types::Disposable::new(move || {
            let Some(this) = this.upgrade() else {
                return;
            };
            this.notebook_status_bar_item_providers.borrow_mut().remove(&handle);
            let _ = this.notebook_proxy.rpc_unregister_notebook_cell_status_bar_item_provider(handle, event_handle);
            if let Some(subscription) = &subscription {
                subscription.dispose();
            }
        })
    }

    pub fn create_notebook_document(&self, view_type: &str, content: Option<&types::NotebookData>) -> Promise<URI> {
        self.notebook_documents_proxy.rpc_try_create_notebook(NotebookCreateOptions {
            view_type: view_type.to_owned(),
            content: content.map(type_converters::notebook::NotebookData::from),
        })
    }

    pub fn open_notebook_document(&self, uri: &URI) -> Promise<NotebookDocument> {
        if let Some(cached) = self.try_get_notebook_document(uri) {
            return Promise::resolved(cached.api_notebook());
        }
        let this = self.rc();
        self.notebook_documents_proxy.rpc_try_open_notebook(uri).then(move |canonical_uri| {
            let document = this.try_get_notebook_document(&canonical_uri);
            assert_returns_defined(document.map(|document| document.api_notebook()))
        })
    }

    pub fn show_notebook_document(&self, notebook: &NotebookDocument, options: Option<NotebookDocumentShowOptions>) -> Promise<NotebookEditor> {
        let resolved_options = match &options {
            Some(options) => INotebookDocumentShowOptions {
                position: Some(type_converters::ViewColumn::from(options.view_column)),
                preserve_focus: options.preserve_focus,
                selections: options.selections.as_ref().map(|selections| selections.iter().map(type_converters::NotebookRange::from).collect()),
                pinned: options.preview.map(|preview| !preview),
                label: match &options.as_repl {
                    Some(NotebookAsRepl::Label(label)) => Some(label.clone()),
                    _ => None,
                },
            },
            None => INotebookDocumentShowOptions { preserve_focus: Some(false), pinned: Some(true), ..INotebookDocumentShowOptions::default() },
        };

        let is_repl = match options.as_ref().and_then(|options| options.as_repl.as_ref()) {
            Some(NotebookAsRepl::Flag(flag)) => *flag,
            Some(NotebookAsRepl::Label(_)) => true,
            None => false,
        };
        let view_type = if is_repl { "repl".to_owned() } else { notebook.notebook_type().to_owned() };
        let uri = notebook.uri().clone();
        let this = self.rc();
        let proxy = self.notebook_editors_proxy.clone();
        async_fn(async move {
            let editor_id = proxy.rpc_try_show_notebook_document(&uri, &view_type, resolved_options).await?;
            let editor = if editor_id.is_empty() { None } else { this.editors.borrow().get(&editor_id).map(ExtHostNotebookEditor::api_editor) };

            if let Some(editor) = editor {
                return Ok(editor);
            }

            if !editor_id.is_empty() {
                Err(Error::new(format!("Could NOT open editor for \"{}\" because another editor opened in the meantime.", uri.to_string(false))))
            } else {
                Err(Error::new(format!("Could NOT open editor for \"{}\".", uri.to_string(false))))
            }
        })
    }

    // --- serialize/deserialize

    pub fn register_notebook_serializer(
        &self,
        extension: &ExtensionDescription,
        view_type: &str,
        serializer: Rc<dyn NotebookSerializer>,
        options: Option<types::NotebookDocumentContentOptions>,
        registration: Option<&NotebookRegistrationData>,
    ) -> Result<DisposableRef, Error> {
        let view_type_units: Vec<u16> = view_type.encode_utf16().collect();
        if is_falsy_or_whitespace(Some(view_type_units.as_slice())) {
            return Err(Error::new("viewType cannot be empty or just whitespace"));
        }
        let handle = self.handle_pool.get();
        self.handle_pool.set(handle + 1);
        let transient_options = type_converters::notebook::NotebookDocumentContentOptions::from(options.as_ref());
        self.notebook_serializer.borrow_mut().insert(handle, Rc::new(SerializerEntry { view_type: view_type.to_owned(), serializer, options }));
        self.notebook_proxy.rpc_register_notebook_serializer(
            handle,
            NotebookExtensionDescription {
                id: extension.identifier.clone(),
                location: Some(URI::parse(&extension.extension_location, false).to_json().components),
            },
            view_type,
            transient_options,
            Self::convert_notebook_registration_data(extension, registration),
        );
        let this = self.this.clone();
        Ok(to_disposable(move || {
            let Some(this) = this.upgrade() else {
                return;
            };
            this.notebook_serializer.borrow_mut().remove(&handle);
            this.notebook_proxy.rpc_unregister_notebook_serializer(handle);
        }))
    }

    fn validate_write_file(&self, uri: &URI, options: &WriteFileOptions) -> Promise<()> {
        let options = options.clone();
        self.ext_host_file_system.stat(uri).then_result(move |stat| {
            let stat = stat?;
            // Dirty write prevention
            if let (Some(mtime), Some(options_etag)) = (options.mtime, options.etag.as_deref())
                && options_etag != ETAG_DISABLED
                && mtime < stat.mtime
                && options_etag != etag(mtime /* not using stat.mtime for a reason, see above */, stat.size)
            {
                return Err(FileOperationError::new(localize("fileModifiedError", "File Modified Since", &[]), FileOperationResult::FileModifiedSince).into_error());
            }
            Ok(())
        })
    }

    fn resource_for_error(uri: &URI) -> String {
        if uri.scheme() == schemas::FILE {
            fs_path(uri)
        } else {
            uri.to_string(false).to_string_lossy()
        }
    }

    fn trace(&self, msg: &str) {
        self.log_service.trace(&format!("[Extension Host Notebook] {msg}"));
    }
}

/// `{ ...source }` sem as chaves marcadas como transitórias: o `filter(obj, key => !transient[key])` do original.
fn filter_transient(source: &Dynamic, transient: Option<&HashMap<String, bool>>) -> Dynamic {
    Dynamic::object(source.own_entries().into_iter().filter(|(key, _)| !transient.and_then(|transient| transient.get(&key.to_string_lossy())).copied().unwrap_or(false)))
}

impl ExtHostNotebookDocumentsAndEditorsShape for ExtHostNotebookController {
    fn rpc_accept_document_and_editors_delta(&self, delta: SerializableObjectWithBuffers<INotebookDocumentsAndEditorsDelta>) {
        let delta = delta.value;

        if let Some(removed_documents) = delta.removed_documents {
            for uri in removed_documents {
                let document = self.documents.borrow().get(&uri).cloned();

                if let Some(document) = document {
                    document.dispose();
                    self.documents.borrow_mut().delete(&uri);
                    let api_notebook = document.api_notebook();
                    self.text_documents_and_editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
                        removed_documents: Some(api_notebook.get_cells(None).iter().map(|cell| cell.document().uri().clone()).collect()),
                        ..DocumentsAndEditorsDelta::default()
                    });
                    self.on_did_close_notebook_document.fire(api_notebook);
                }

                let removed_editor_ids: Vec<String> = self
                    .editors
                    .borrow()
                    .values()
                    .filter(|editor| editor.notebook_data.uri.to_string(false) == uri.to_string(false))
                    .map(|editor| editor.id.clone())
                    .collect();
                for id in removed_editor_ids {
                    self.editors.borrow_mut().remove(&id);
                }
            }
        }

        if let Some(added_documents) = delta.added_documents {
            let mut added_cell_documents = Vec::new();

            for model_data in added_documents {
                let uri = model_data.uri.clone();

                if self.documents.borrow().has(&uri) {
                    errors::throw(Error::new(format!("adding EXISTING notebook {} ", uri.to_string(false))));
                }

                let document = ExtHostNotebookDocument::new(
                    self.notebook_documents_proxy.clone(),
                    self.text_documents_and_editors.clone(),
                    self.text_documents.clone(),
                    uri.clone(),
                    model_data.clone(),
                );

                // add cell document as vscode.TextDocument
                added_cell_documents.extend(model_data.cells.iter().map(ExtHostCell::as_model_add_data));

                let existing = self.documents.borrow().get(&uri).cloned();
                if let Some(existing) = existing {
                    existing.dispose();
                }
                self.documents.borrow_mut().set(uri, document.clone());
                self.text_documents_and_editors.rpc_accept_documents_and_editors_delta(DocumentsAndEditorsDelta {
                    added_documents: Some(added_cell_documents.clone()),
                    ..DocumentsAndEditorsDelta::default()
                });

                self.on_did_open_notebook_document.fire(document.api_notebook());
            }
        }

        if let Some(added_editors) = delta.added_editors {
            for editor_model_data in added_editors {
                if self.editors.borrow().contains_key(&editor_model_data.id) {
                    return;
                }

                let document = self.try_get_notebook_document(&editor_model_data.document_uri);

                if let Some(document) = document {
                    self.create_ext_host_editor(document, &editor_model_data.id, &editor_model_data);
                }
            }
        }

        let mut removed_editors: Vec<Rc<ExtHostNotebookEditor>> = Vec::new();

        if let Some(removed_editor_ids) = delta.removed_editors {
            for editor_id in removed_editor_ids {
                let editor = self.editors.borrow_mut().remove(&editor_id);

                if let Some(editor) = editor {
                    let is_active = self.active_notebook_editor.borrow().as_ref().is_some_and(|active| active.id == editor.id);
                    if is_active {
                        *self.active_notebook_editor.borrow_mut() = None;
                    }

                    removed_editors.push(editor);
                }
            }
        }
        // `removedEditors` só é usado no original para juntar os editores removidos: nenhum evento os usa.
        drop(removed_editors);

        if let Some(visible_editors) = delta.visible_editors {
            let visible: Vec<Rc<ExtHostNotebookEditor>> = visible_editors.iter().filter_map(|id| self.editors.borrow().get(id).cloned()).collect();
            let visible_editors_set: Vec<String> = visible.iter().map(|editor| editor.id.clone()).collect();
            *self.visible_notebook_editors.borrow_mut() = visible;

            for editor in self.editors.borrow().values() {
                let new_value = visible_editors_set.contains(&editor.id);
                editor.accept_visibility(new_value);
            }

            let visible_now: Vec<Rc<ExtHostNotebookEditor>> = self.editors.borrow().values().filter(|editor| editor.visible()).cloned().collect();
            *self.visible_notebook_editors.borrow_mut() = visible_now;
            self.on_did_change_visible_notebook_editors.fire(self.visible_notebook_editors());
        }

        match &delta.new_active_editor {
            Some(None) => {
                // clear active notebook as current active editor is non-notebook editor
                *self.active_notebook_editor.borrow_mut() = None;
            }
            Some(Some(new_active_editor)) => {
                let active_editor = self.editors.borrow().get(new_active_editor).cloned();
                if active_editor.is_none() {
                    eprintln!("FAILED to find active notebook editor {new_active_editor}");
                }
                *self.active_notebook_editor.borrow_mut() = active_editor;
            }
            None => {}
        }
        if delta.new_active_editor.is_some() {
            self.on_did_change_active_notebook_editor.fire(self.active_notebook_editor());
        }
    }
}

impl ExtHostNotebookController {
    fn create_ext_host_editor(&self, document: Rc<ExtHostNotebookDocument>, editor_id: &str, data: &INotebookEditorAddData) {
        if self.editors.borrow().contains_key(&editor_id.to_owned()) {
            errors::throw(Error::new(format!("editor with id ALREADY EXSIST: {editor_id}")));
        }

        let to_range = |range: &type_converters::CellRange| type_converters::NotebookRange::to(range).unwrap_or_else(|error| errors::throw(error));
        let editor = ExtHostNotebookEditor::new(
            editor_id,
            self.notebook_editors_proxy.clone(),
            document,
            data.visible_ranges.iter().map(to_range).collect(),
            data.selections.iter().map(to_range).collect(),
            data.view_column.map(type_converters::ViewColumn::to),
            &data.view_type,
        );

        self.editors.borrow_mut().insert(editor_id.to_owned(), Rc::new(editor));
    }
}

impl ExtHostNotebookShape for ExtHostNotebookController {
    fn rpc_provide_notebook_cell_status_bar_items(&self, handle: u32, uri: &URI, index: usize, token: CancellationToken) -> Promise<Option<INotebookCellStatusBarListDto>> {
        let this = self.rc();
        let provider = self.notebook_status_bar_item_providers.borrow().get(&handle).cloned();
        let document = self.try_get_notebook_document(uri);
        async_fn(async move {
            let (Some(document), Some(provider)) = (document, provider) else {
                return Ok(None);
            };

            let Some(cell) = document.get_cell_from_index(index) else {
                return Ok(None);
            };

            let result = await_maybe(provider.provide_cell_status_bar_items(&cell.api_cell(), token)).await?;
            let Some(result) = result else {
                return Ok(None);
            };

            let disposables = DisposableStore::new();
            let cache_id = this.status_bar_cache.add(vec![disposables.clone()]);
            let result_arr = match result {
                NotebookCellStatusBarItems::One(item) => vec![item],
                NotebookCellStatusBarItems::Many(items) => items,
            };
            let items = result_arr
                .iter()
                .map(|item| NotebookStatusBarItem::from(item, this.commands_converter.as_ref(), &disposables))
                .collect::<Result<Vec<_>, Error>>()?;
            Ok(Some(INotebookCellStatusBarListDto { cache_id, items }))
        })
    }

    fn rpc_release_notebook_cell_status_bar_items(&self, cache_id: u32) {
        if let Some(disposables) = self.status_bar_cache.get(cache_id, 0) {
            disposables.dispose();
        }
        self.status_bar_cache.delete(cache_id);
    }

    fn rpc_data_to_notebook(&self, handle: u32, bytes: VSBuffer, token: CancellationToken) -> Promise<SerializableObjectWithBuffers<NotebookDataDto>> {
        let serializer = self.notebook_serializer.borrow().get(&handle).cloned();
        async_fn(async move {
            let Some(serializer) = serializer else {
                return Err(Error::new("NO serializer found"));
            };
            let data = await_maybe(serializer.serializer.deserialize_notebook(&bytes.to_vec(), token)).await?;
            Ok(SerializableObjectWithBuffers::new(type_converters::notebook::NotebookData::from(&data)))
        })
    }

    fn rpc_notebook_to_data(&self, handle: u32, data: SerializableObjectWithBuffers<NotebookDataDto>, token: CancellationToken) -> Promise<VSBuffer> {
        let serializer = self.notebook_serializer.borrow().get(&handle).cloned();
        async_fn(async move {
            let Some(serializer) = serializer else {
                return Err(Error::new("NO serializer found"));
            };
            let notebook_data = type_converters::notebook::NotebookData::to(&data.value)?;
            let bytes = await_maybe(serializer.serializer.serialize_notebook(&notebook_data, token)).await?;
            Ok(VSBuffer::wrap(bytes))
        })
    }

    fn rpc_save_notebook(&self, handle: u32, uri: &URI, version_id: i64, options: WriteFileOptions, token: CancellationToken) -> Promise<NotebookSaveResult> {
        let this = self.rc();
        let uri = uri.clone();
        let serializer = self.notebook_serializer.borrow().get(&handle).cloned();
        self.trace(&format!("enter saveNotebook(versionId: {version_id}, {})", uri.to_string(false)));

        async_fn(async move {
            let result: Result<NotebookSaveResult, Error> = async {
                let Some(serializer) = serializer else {
                    return Err(notebook_save_error("NO serializer found"));
                };

                let Some(document) = this.try_get_notebook_document(&uri) else {
                    return Err(notebook_save_error("Document NOT found"));
                };

                if document.version_id() != version_id {
                    return Err(notebook_save_error(format!("Document version mismatch, expected: {version_id}, actual: {}", document.version_id())));
                }

                if this.ext_host_file_system.is_writable_file_system(&uri.scheme().to_string_lossy()) != Some(true) {
                    let resource = Self::resource_for_error(&uri);
                    return Err(FileOperationError::new(
                        localize("err.readonly", "Unable to modify read-only file '{0}'", &[resource.as_str()]),
                        FileOperationResult::FilePermissionDenied,
                    )
                    .into_error());
                }

                let serializer_options = serializer.options.as_ref();
                let api_notebook = document.api_notebook();
                let mut cells: Vec<types::NotebookCellData> = Vec::new();

                // this data must be retrieved before any async calls to ensure the data is for the correct version
                for cell in api_notebook.get_cells(None) {
                    let transient_outputs = serializer_options.and_then(|options| options.transient_outputs).unwrap_or(false);
                    let mut cell_data = types::NotebookCellData::new(
                        cell.kind(),
                        cell.document().get_text(None).to_string_lossy(),
                        cell.document().language_id(),
                        cell.mime(),
                        Some(if transient_outputs { Vec::new() } else { cell.outputs() }),
                        Some(cell.metadata()),
                        Some(cell.execution_summary()),
                    );

                    cell_data.metadata = Some(filter_transient(&cell.metadata(), serializer_options.and_then(|options| options.transient_cell_metadata.as_ref())));
                    cells.push(cell_data);
                }
                let data = types::NotebookData {
                    metadata: Some(filter_transient(&api_notebook.metadata(), serializer_options.and_then(|options| options.transient_document_metadata.as_ref()))),
                    cells,
                };

                // validate write
                this.validate_write_file(&uri, &options).await?;

                if token.is_cancellation_requested() {
                    return Err(Error::cancellation());
                }
                let bytes = await_maybe(serializer.serializer.serialize_notebook(&data, token.clone())).await?;
                if token.is_cancellation_requested() {
                    return Err(Error::cancellation());
                }

                // Don't accept any cancellation beyond this point, we need to report the result of the file write
                this.trace(&format!("serialized versionId: {version_id} {}", uri.to_string(false)));
                this.ext_host_file_system.write_file(&uri, bytes).await?;
                this.trace(&format!("Finished write versionId: {version_id} {}", uri.to_string(false)));
                let provider_ext_uri = this.ext_host_file_system.get_file_system_provider_ext_uri(&uri.scheme().to_string_lossy());
                let stat = this.ext_host_file_system.stat(&uri).await?;
                let permissions = stat.permissions.unwrap_or(FilePermission::NONE);

                let file_stats = INotebookPartialFileStatsWithMetadata {
                    name: provider_ext_uri.basename(&uri, None),
                    is_file: stat.file_type.intersects(FileType::FILE),
                    is_directory: stat.file_type.intersects(FileType::DIRECTORY),
                    is_symbolic_link: stat.file_type.intersects(FileType::SYMBOLIC_LINK),
                    mtime: stat.mtime,
                    ctime: stat.ctime,
                    size: stat.size,
                    readonly: permissions.intersects(FilePermission::READONLY)
                        || this.ext_host_file_system.is_writable_file_system(&uri.scheme().to_string_lossy()) != Some(true),
                    locked: permissions.intersects(FilePermission::LOCKED),
                    executable: permissions.intersects(FilePermission::EXECUTABLE),
                    etag: etag(stat.mtime, stat.size),
                };

                this.trace(&format!("exit saveNotebook(versionId: {version_id}, {})", uri.to_string(false)));
                Ok(NotebookSaveResult::Stats(file_stats))
            }
            .await;

            // return fileOperationsErrors to keep the whole object across serialization, these errors are handled specially by the WCS
            match result {
                Err(error) => match FileOperationError::from_error(&error) {
                    Some(file_operation_error) => Ok(NotebookSaveResult::Error(file_operation_error)),
                    None => Err(error),
                },
                ok => ok,
            }
        })
    }
}
