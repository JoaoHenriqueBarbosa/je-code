//! Porte de `src/vs/workbench/api/common/extHostDocuments.ts`: os documentos de texto do lado das extensões
//! (`vscode.workspace.textDocuments`, `onDidOpenTextDocument`, `onDidChangeTextDocument`...) e as mudanças que a
//! thread principal manda sobre eles.
//!
//! Divergências:
//! - `setWordDefinitionFor(languageId, wordDefinition)` só repassa a `setWordDefinitionFor` de
//!   `extHostDocumentData.ts`: a regra do projeto proíbe a função de repasse, e quem o chamaria usa
//!   [`set_word_definition_for`](super::ext_host_document_data::set_word_definition_for) direto.
//! - `ExtHostDocuments` é um `Rc` ([`ExtHostDocuments::new`]), porque as promessas de `ensureDocumentData` guardam
//!   o objeto até assentarem. O `mainContext: IMainContext` só serve para pedir o proxy; o
//!   [`ExtHostDocumentsAndEditors`] é compartilhado por `Rc`.
//! - Os objetos de evento são structs ([`TextDocumentChangeEvent`] e [`TextDocumentContentChangeEvent`]); o
//!   `deepFreeze` do original não existe (o evento entrega `&T` aos ouvintes, e eles não o mudam). O
//!   `Omit<vscode.TextDocumentChangeEvent, 'detailedReason'>` do emissor `_onDidChangeDocument` é o mesmo struct
//!   com `detailed_reason` ausente (o original nem cria a propriedade). O `detailedReason` com `source` e
//!   `metadata` é [`TextDocumentChangeDetailedReason`]; o `source` é o texto da propriedade (vazio se ela não for
//!   um texto), e o `metadata` é o objeto inteiro. Os `contentChanges` dos dois eventos são os mesmos valores
//!   (o original os monta duas vezes).
//! - `ISerializedModelContentChangedEvent` entra no espelho do modelo como um `ModelContentChangedEvent` sem
//!   `detailedReasons` (o espelho só lê `changes`, `eol` e `versionId`).
//! - O `URI.revive(...)` do original já está feito nos protocolos (`URI` em vez de `UriComponents`), e o
//!   `.then(data => URI.revive(data))` de `createDocumentData` fica como `then` identidade, para manter a mesma
//!   contagem de microtarefas.
//! - `getDocument` lança no original; aqui devolve [`Result`]. Os `throw new Error('unknown document')` dos
//!   `$accept*` são pânico com [`Error`] (o transporte captura), num auxiliar único.
//! - `getDocumentData(resource)` não tem o `if (!resource) return undefined`: o tipo `&URI` garante o argumento.
//! - `ensureDocumentData` recebe o `options` por valor (a chamada recursiva o reaproveita) e `_documentLoader` é
//!   um `HashMap` por `uri.toString()`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::types::assert_returns_defined;
use je_base::uri::URI;
use je_editor::text_model_events::ModelContentChangedEvent;

use super::ext_host_document_data::{ExtHostDocumentData, TextDocument};
use super::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use super::ext_host_protocol::{
    CreateDocumentOptions, ExtHostDocumentsShape, IMainContext, MainContext, MainThreadDocumentsShape, OpenDocumentOptions,
    SerializedModelContentChangedEvent,
};
use super::ext_host_type_converters as type_converters;
use super::ext_host_types::{Range, TextDocumentChangeReason};

/// `vscode.TextDocumentContentChangeEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextDocumentContentChangeEvent {
    pub range: Range,
    pub range_offset: f64,
    pub range_length: f64,
    pub text: JsString,
}

/// O `detailedReason` de `vscode.TextDocumentChangeEvent`: `{ source: string, metadata: ... }`.
#[derive(Clone, Debug)]
pub struct TextDocumentChangeDetailedReason {
    pub source: String,
    pub metadata: Dynamic,
}

/// `vscode.TextDocumentChangeEvent`.
#[derive(Clone)]
pub struct TextDocumentChangeEvent {
    pub document: TextDocument,
    pub content_changes: Vec<TextDocumentContentChangeEvent>,
    pub reason: Option<TextDocumentChangeReason>,
    pub detailed_reason: Option<TextDocumentChangeDetailedReason>,
}

/// Um [`Emitter`] registrado em `store`, para ser descartado junto.
fn register_emitter<T: 'static>(store: &DisposableStore) -> Rc<Emitter<T>> {
    let emitter: Rc<Emitter<T>> = Emitter::new();
    store.add(emitter.clone());
    emitter
}

/// `ExtHostDocuments`.
pub struct ExtHostDocuments {
    to_dispose: Rc<DisposableStore>,

    on_did_add_document: Rc<Emitter<TextDocument>>,
    on_did_remove_document: Rc<Emitter<TextDocument>>,
    on_did_change_document: Rc<Emitter<TextDocumentChangeEvent>>,
    on_did_change_document_with_reason: Rc<Emitter<TextDocumentChangeEvent>>,
    on_did_save_document: Rc<Emitter<TextDocument>>,

    proxy: Rc<dyn MainThreadDocumentsShape>,
    documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
    document_loader: RefCell<HashMap<JsString, Promise<Rc<ExtHostDocumentData>>>>,
}

impl ExtHostDocuments {
    pub fn new(main_context: &dyn IMainContext, documents_and_editors: Rc<ExtHostDocumentsAndEditors>) -> Rc<Self> {
        let to_dispose = DisposableStore::new();
        let on_did_add_document = register_emitter(&to_dispose);
        let on_did_remove_document = register_emitter(&to_dispose);

        documents_and_editors.on_did_remove_documents().listen_in(
            {
                let emitter = on_did_remove_document.clone();
                move |documents: &Vec<Rc<ExtHostDocumentData>>| {
                    for data in documents {
                        emitter.fire(data.document());
                    }
                }
            },
            &to_dispose,
        );
        documents_and_editors.on_did_add_documents().listen_in(
            {
                let emitter = on_did_add_document.clone();
                move |documents: &Vec<Rc<ExtHostDocumentData>>| {
                    for data in documents {
                        emitter.fire(data.document());
                    }
                }
            },
            &to_dispose,
        );

        Rc::new(Self {
            on_did_change_document: register_emitter(&to_dispose),
            on_did_change_document_with_reason: register_emitter(&to_dispose),
            on_did_save_document: register_emitter(&to_dispose),
            on_did_add_document,
            on_did_remove_document,
            to_dispose,
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_DOCUMENTS),
            documents_and_editors,
            document_loader: RefCell::new(HashMap::new()),
        })
    }

    /// `onDidAddDocument`.
    pub fn on_did_add_document(&self) -> Event<TextDocument> {
        self.on_did_add_document.event()
    }

    /// `onDidRemoveDocument`.
    pub fn on_did_remove_document(&self) -> Event<TextDocument> {
        self.on_did_remove_document.event()
    }

    /// `onDidChangeDocument` (o evento sem o `detailedReason`).
    pub fn on_did_change_document(&self) -> Event<TextDocumentChangeEvent> {
        self.on_did_change_document.event()
    }

    /// `onDidChangeDocumentWithReason`.
    pub fn on_did_change_document_with_reason(&self) -> Event<TextDocumentChangeEvent> {
        self.on_did_change_document_with_reason.event()
    }

    /// `onDidSaveDocument`.
    pub fn on_did_save_document(&self) -> Event<TextDocument> {
        self.on_did_save_document.event()
    }

    pub fn get_all_document_data(&self) -> Vec<Rc<ExtHostDocumentData>> {
        self.documents_and_editors.all_documents()
    }

    pub fn get_document_data(&self, resource: &URI) -> Option<Rc<ExtHostDocumentData>> {
        self.documents_and_editors.get_document(resource)
    }

    pub fn get_document(&self, resource: &URI) -> Result<TextDocument, Error> {
        match self.get_document_data(resource) {
            Some(data) => Ok(data.document()),
            None => Err(Error::new(format!("Unable to retrieve document from URI '{}'", resource.to_string(false)))),
        }
    }

    pub fn ensure_document_data(self: &Rc<Self>, uri: &URI, options: Option<OpenDocumentOptions>) -> Promise<Rc<ExtHostDocumentData>> {
        let encoding = options.as_ref().and_then(|options| options.encoding.clone()).filter(|encoding| !encoding.is_empty());

        let cached = self.documents_and_editors.get_document(uri);
        if let Some(cached) = cached
            && encoding.as_ref().is_none_or(|encoding| cached.document().encoding() == *encoding)
        {
            return Promise::resolved(cached);
        }

        let key = uri.to_string(false);
        let existing = self.document_loader.borrow().get(&key).cloned();
        match existing {
            None => {
                let this = self.clone();
                let loader_key = key.clone();
                let promise = self.proxy.rpc_try_open_document(uri, options).then_result(move |result| {
                    this.document_loader.borrow_mut().remove(&loader_key);
                    let canonical_uri = result?;
                    Ok(assert_returns_defined(this.documents_and_editors.get_document(&canonical_uri)))
                });
                self.document_loader.borrow_mut().insert(key, promise.clone());
                promise
            }
            Some(promise) => match encoding {
                Some(encoding) => {
                    let this = self.clone();
                    let uri = uri.clone();
                    promise.then_maybe(move |data| {
                        if data.document().encoding() != encoding {
                            return MaybePromise::Promise(this.ensure_document_data(&uri, options));
                        }
                        MaybePromise::Value(data)
                    })
                }
                None => promise,
            },
        }
    }

    pub fn create_document_data(&self, options: Option<CreateDocumentOptions>) -> Promise<URI> {
        // o `URI.revive` do original já foi feito pelo protocolo: o `then` fica pela mesma contagem de microtarefas
        self.proxy.rpc_try_create_document(options).then(|uri| uri)
    }

    /// O documento de `uri`; lança `unknown document` se não existe.
    fn known_document_data(&self, uri: &URI) -> Rc<ExtHostDocumentData> {
        self.documents_and_editors.get_document(uri).unwrap_or_else(|| errors::throw(Error::new("unknown document")))
    }

    /// Os dois eventos de mudança sem conteúdo (estado sujo e codificação).
    fn fire_change_without_content(&self, document: TextDocument) {
        self.on_did_change_document.fire(TextDocumentChangeEvent {
            document: document.clone(),
            content_changes: Vec::new(),
            reason: None,
            detailed_reason: None,
        });
        self.on_did_change_document_with_reason.fire(TextDocumentChangeEvent {
            document,
            content_changes: Vec::new(),
            reason: None,
            detailed_reason: None,
        });
    }
}

impl IDisposable for ExtHostDocuments {
    fn dispose(&self) {
        self.to_dispose.dispose();
    }
}

impl ExtHostDocumentsShape for ExtHostDocuments {
    fn rpc_accept_model_language_changed(&self, uri: &URI, new_language_id: &str) {
        let data = self.known_document_data(uri);
        // Treat a language change as a remove + add

        self.on_did_remove_document.fire(data.document());
        data.accept_language_id(new_language_id);
        self.on_did_add_document.fire(data.document());
    }

    fn rpc_accept_model_saved(&self, uri: &URI) {
        let data = self.known_document_data(uri);
        self.rpc_accept_dirty_state_changed(uri, false);
        self.on_did_save_document.fire(data.document());
    }

    fn rpc_accept_dirty_state_changed(&self, uri: &URI, is_dirty: bool) {
        let data = self.known_document_data(uri);
        data.accept_is_dirty(is_dirty);
        self.fire_change_without_content(data.document());
    }

    fn rpc_accept_encoding_changed(&self, uri: &URI, encoding: &str) {
        let data = self.known_document_data(uri);
        data.accept_encoding(encoding);
        self.fire_change_without_content(data.document());
    }

    fn rpc_accept_model_changed(&self, uri: &URI, e: SerializedModelContentChangedEvent, is_dirty: bool) {
        let data = self.known_document_data(uri);
        data.accept_is_dirty(is_dirty);
        data.mirror_mut().on_events(&ModelContentChangedEvent {
            changes: e.changes.clone(),
            eol: e.eol,
            version_id: e.version_id,
            is_undoing: e.is_undoing,
            is_redoing: e.is_redoing,
            is_flush: e.is_flush,
            is_eol_change: e.is_eol_change,
            detailed_reasons: Vec::new(),
            detailed_reasons_change_lengths: Vec::new(),
        });

        let reason = if e.is_undoing {
            Some(TextDocumentChangeReason::Undo)
        } else if e.is_redoing {
            Some(TextDocumentChangeReason::Redo)
        } else {
            None
        };

        let content_changes: Vec<TextDocumentContentChangeEvent> = e
            .changes
            .iter()
            .map(|change| TextDocumentContentChangeEvent {
                range: type_converters::Range::to(&change.range).unwrap_or_else(|error| errors::throw(error)),
                range_offset: change.range_offset as f64,
                range_length: change.range_length as f64,
                text: change.text.clone(),
            })
            .collect();

        let document = data.document();
        self.on_did_change_document.fire(TextDocumentChangeEvent {
            document: document.clone(),
            content_changes: content_changes.clone(),
            reason,
            detailed_reason: None,
        });
        self.on_did_change_document_with_reason.fire(TextDocumentChangeEvent {
            document,
            content_changes,
            reason,
            detailed_reason: e.detailed_reason.filter(Dynamic::is_truthy).map(|detailed_reason| TextDocumentChangeDetailedReason {
                source: match detailed_reason.get_str("source") {
                    Dynamic::String(source) => source.to_string_lossy(),
                    _ => String::new(),
                },
                metadata: detailed_reason,
            }),
        });
    }
}
