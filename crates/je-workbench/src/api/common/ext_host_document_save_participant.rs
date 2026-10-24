//! Porte de `src/vs/workbench/api/common/extHostDocumentSaveParticipant.ts`: o `onWillSaveTextDocument` do lado das
//! extensões. A thread principal pede `$participateInSave`, o host entrega o evento a cada ouvinte registrado, espera
//! as promessas de `waitUntil` (com prazo) e aplica as edições devolvidas, se o documento não mudou no meio do caminho.
//!
//! Divergências:
//! - O `Event<vscode.TextDocumentWillSaveEvent>` que `getOnWillSaveTextDocumentEvent` devolve é o [`Event`] de
//!   `je-base`: o `thisArg` não existe (o fecho captura o que precisar) e o `disposables` é tratado por
//!   `listen_into`. A inscrição devolvida é o `{ dispose: remove }` do original, sem rastreio de vazamento.
//! - `vscode.TextDocumentWillSaveEvent` é o [`TextDocumentWillSaveEvent`]: o original o congela com `Object.freeze`
//!   (`event.document = x` lança); aqui o ouvinte só recebe `&TextDocumentWillSaveEvent`, e escrever nos campos é erro de
//!   compilação. `waitUntil` é [`TextDocumentWillSaveEvent::wait_until`], que devolve [`Result`] (o `throw` do
//!   original) e recebe `Promise<Dynamic>` (o `Promise<any | TextEdit[]>`, já uma promessa: o `Promise.resolve(p)` do
//!   original devolve a mesma promessa). O evento é `Clone` (os `Rc` internos são compartilhados), porque a extensão
//!   pode guardá-lo e chamar `waitUntil` depois do prazo, o que o original recusa com `illegalState`.
//! - Os valores de `waitUntil` cruzam como [`Dynamic`]: `Array.isArray(value) && value.every(e => e instanceof TextEdit)`
//!   é `Dynamic::Array` com todos os elementos de classe `TextEdit` (`is_instance_of`), lidos por
//!   [`TextEdit::from_dynamic`].
//! - `_badListeners` (`WeakMap<Function, number>`) é uma lista de `Weak` do ouvinte (`Rc<dyn Fn>`) com a contagem; as
//!   entradas de ouvintes já liberados saem a cada consulta. O `err instanceof Error` de `_deliverEventAsync...` é
//!   sempre verdadeiro (todo erro é um [`Error`]).
//! - `_deliverEventAsync` devolve `Promise<boolean | undefined>` no original, e o valor nunca é lido (só importa se
//!   assentou ou rejeitou): é `Promise<bool>`, com `undefined` (nenhuma edição) valendo `false`.
//! - `SerializableObjectWithBuffers` é o de [`ext_host_protocol`](super::ext_host_protocol), e o `IWorkspaceTextEditDto`
//!   é o `IWorkspaceTextEdit` do editor. A conversão `TextDocumentSaveReason.to(reason)` de `extHostTypeConverters.ts`
//!   é a de [`ext_host_type_converters::save`](super::ext_host_type_converters::save).
//! - `ExtHostDocumentSaveParticipant` guarda o estado num `Rc` interno: as promessas e os temporizadores seguram o
//!   participante até assentarem. `$participateInSave` é uma função `async` (`async_fn`), com o `try/finally` do
//!   temporizador de prazo geral escrito em torno do laço.
//! - Os `throw` de `getDocument` (documento desconhecido) rejeitam a promessa de `$participateInSave`, como no original.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Event, Listener};
use je_base::lifecycle::{IDisposable, plain_disposable};
use je_base::linked_list::LinkedList;
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::uri::URI;
use je_editor::code_action::languages::IWorkspaceTextEdit;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogMessage, LogServiceRef, Logger};

use super::ext_host_document_data::TextDocument;
use super::ext_host_documents::ExtHostDocuments;
use super::ext_host_protocol::{
    ExtHostDocumentSaveParticipantShape, IWorkspaceEditDto, MainThreadBulkEditsShape, SerializableObjectWithBuffers, WorkspaceEditEntryDto,
};
use super::ext_host_type_converters as type_converters;
use super::ext_host_types::{TextDocumentSaveReason, TextEdit};
use super::ext_host_values::{ApiValue, is_instance_of};
use crate::common::editor::SaveReason;

/// O `{ timeout: number; errors: number }` do construtor: o prazo em milissegundos (de cada ouvinte e do conjunto) e
/// quantos erros um ouvinte pode dar antes de ser ignorado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveParticipantThresholds {
    pub timeout: u64,
    pub errors: u32,
}

impl Default for SaveParticipantThresholds {
    fn default() -> Self {
        Self { timeout: 1500, errors: 3 }
    }
}

/// `vscode.TextDocumentWillSaveEvent`.
#[derive(Clone)]
pub struct TextDocumentWillSaveEvent {
    pub document: TextDocument,
    pub reason: TextDocumentSaveReason,
    promises: Rc<RefCell<Vec<Promise<Dynamic>>>>,
    promises_frozen: Rc<Cell<bool>>,
}

impl TextDocumentWillSaveEvent {
    /// `waitUntil(p)`: lança `Illegal state: waitUntil can not be called async` depois que o ouvinte retornou.
    pub fn wait_until(&self, p: Promise<Dynamic>) -> Result<(), Error> {
        if self.promises_frozen.get() {
            return Err(Error::new("Illegal state: waitUntil can not be called async"));
        }
        self.promises.borrow_mut().push(p);
        Ok(())
    }
}

type SaveListener = Listener<TextDocumentWillSaveEvent>;

/// `Listener = [Function, unknown, IExtensionDescription]` (sem o `thisArg`).
#[derive(Clone)]
struct ListenerEntry {
    listener: SaveListener,
    extension: ExtensionDescription,
}

struct Inner {
    callbacks: LinkedList<ListenerEntry>,
    bad_listeners: RefCell<Vec<(Weak<dyn Fn(&TextDocumentWillSaveEvent)>, u32)>>,
    log_service: LogServiceRef,
    documents: Rc<ExtHostDocuments>,
    main_thread_bulk_edits: Rc<dyn MainThreadBulkEditsShape>,
    thresholds: SaveParticipantThresholds,
}

/// `ExtHostDocumentSaveParticipant`.
pub struct ExtHostDocumentSaveParticipant {
    inner: Rc<Inner>,
}

impl ExtHostDocumentSaveParticipant {
    /// O construtor com os limites padrão (`{ timeout: 1500, errors: 3 }`).
    pub fn new(log_service: LogServiceRef, documents: Rc<ExtHostDocuments>, main_thread_bulk_edits: Rc<dyn MainThreadBulkEditsShape>) -> Self {
        Self::with_thresholds(log_service, documents, main_thread_bulk_edits, SaveParticipantThresholds::default())
    }

    pub fn with_thresholds(
        log_service: LogServiceRef,
        documents: Rc<ExtHostDocuments>,
        main_thread_bulk_edits: Rc<dyn MainThreadBulkEditsShape>,
        thresholds: SaveParticipantThresholds,
    ) -> Self {
        Self {
            inner: Rc::new(Inner {
                callbacks: LinkedList::new(),
                bad_listeners: RefCell::new(Vec::new()),
                log_service,
                documents,
                main_thread_bulk_edits,
                thresholds,
            }),
        }
    }

    /// `getOnWillSaveTextDocumentEvent(extension)`.
    pub fn get_on_will_save_text_document_event(&self, extension: &ExtensionDescription) -> Event<TextDocumentWillSaveEvent> {
        let inner = self.inner.clone();
        let extension = extension.clone();
        Event::new(move |listener: SaveListener| {
            let remove = inner.callbacks.push(ListenerEntry { listener, extension: extension.clone() });
            plain_disposable(move || remove.remove())
        })
    }
}

impl IDisposable for ExtHostDocumentSaveParticipant {
    fn dispose(&self) {
        self.inner.callbacks.clear();
    }
}

impl ExtHostDocumentSaveParticipantShape for ExtHostDocumentSaveParticipant {
    fn rpc_participate_in_save(&self, resource: &URI, reason: SaveReason) -> Promise<Vec<bool>> {
        let inner = self.inner.clone();
        let resource = resource.clone();
        async_fn(async move {
            let did_timeout = Rc::new(Cell::new(false));
            let did_timeout_handle = runtime::set_timeout(
                {
                    let did_timeout = did_timeout.clone();
                    move || did_timeout.set(true)
                },
                inner.thresholds.timeout,
            );

            let mut results: Vec<bool> = Vec::new();
            let outcome: Result<(), Error> = async {
                // copy to prevent concurrent modifications
                for listener in inner.callbacks.iter().collect::<Vec<_>>() {
                    if did_timeout.get() {
                        // timeout - no more listeners
                        break;
                    }
                    let document = inner.documents.get_document(&resource)?;

                    let success = inner.deliver_event_async_and_blame_bad_listeners(&listener, document, type_converters::TextDocumentSaveReason::to(reason)).await?;
                    results.push(success);
                }
                Ok(())
            }
            .await;
            runtime::clear_timeout(did_timeout_handle);
            outcome?;
            Ok(results)
        })
    }
}

impl Inner {
    /// `this._badListeners.get(listener)`, descartando as entradas de ouvintes que já não existem.
    fn bad_listener_errors(&self, listener: &SaveListener) -> Option<u32> {
        let key = Rc::downgrade(listener);
        let mut bad_listeners = self.bad_listeners.borrow_mut();
        bad_listeners.retain(|(weak, _)| weak.strong_count() > 0);
        bad_listeners.iter().find(|(weak, _)| weak.ptr_eq(&key)).map(|(_, errors)| *errors)
    }

    /// `this._badListeners.set(listener, errors)`.
    fn set_bad_listener_errors(&self, listener: &SaveListener, errors: u32) {
        let key = Rc::downgrade(listener);
        let mut bad_listeners = self.bad_listeners.borrow_mut();
        match bad_listeners.iter_mut().find(|(weak, _)| weak.ptr_eq(&key)) {
            Some((_, current)) => *current = errors,
            None => bad_listeners.push((key, errors)),
        }
    }

    fn deliver_event_async_and_blame_bad_listeners(self: &Rc<Self>, entry: &ListenerEntry, document: TextDocument, reason: TextDocumentSaveReason) -> Promise<bool> {
        let errors = self.bad_listener_errors(&entry.listener);
        if let Some(errors) = errors
            && errors > self.thresholds.errors
        {
            // bad listener - ignore
            return Promise::resolved(false);
        }

        let this = self.clone();
        let entry_for_errors = entry.clone();
        self.deliver_event_async(&entry.extension, &entry.listener, document, reason).then_result(move |result| match result {
            Ok(_) => {
                // don't send result across the wire
                Ok(true)
            }
            Err(err) => {
                let extension_id = &entry_for_errors.extension.identifier.value;
                this.log_service.error(&format!("onWillSaveTextDocument-listener from extension '{extension_id}' threw ERROR"));
                this.log_service.error_args(&LogMessage::from(&err), &[]);

                if err.message != "concurrent_edits" {
                    let errors = this.bad_listener_errors(&entry_for_errors.listener);
                    this.set_bad_listener_errors(&entry_for_errors.listener, errors.map_or(1, |errors| errors + 1));

                    if let Some(errors) = errors
                        && errors > this.thresholds.errors
                    {
                        this.log_service
                            .info(&format!("onWillSaveTextDocument-listener from extension '{extension_id}' will now be IGNORED because of timeouts and/or errors"));
                    }
                }
                Ok(false)
            }
        })
    }

    fn deliver_event_async(self: &Rc<Self>, extension: &ExtensionDescription, listener: &SaveListener, document: TextDocument, reason: TextDocumentSaveReason) -> Promise<bool> {
        let promises: Rc<RefCell<Vec<Promise<Dynamic>>>> = Rc::new(RefCell::new(Vec::new()));
        let promises_frozen = Rc::new(Cell::new(false));

        let t1 = runtime::date_now();
        let version = document.version();

        let event = TextDocumentWillSaveEvent { document: document.clone(), reason, promises: promises.clone(), promises_frozen: promises_frozen.clone() };

        // fire event
        if let Err(err) = errors::catch(|| listener(&event)) {
            return Promise::rejected(err);
        }

        // freeze promises after event call
        promises_frozen.set(true);
        let pending: Vec<Promise<Dynamic>> = promises.borrow().clone();

        // join on all listener promises, reject after timeout
        let (joined, resolver) = Promise::<Vec<Dynamic>>::new();
        let handle = runtime::set_timeout(
            {
                let resolver = resolver.clone();
                move || resolver.reject(Error::new("timeout"))
            },
            self.thresholds.timeout,
        );
        let log_service = self.log_service.clone();
        let extension_id = extension.identifier.value.clone();
        Promise::all(pending)
            .then({
                let resolver = resolver.clone();
                move |edits| {
                    log_service.debug(&format!(
                        "onWillSaveTextDocument-listener from extension '{extension_id}' finished after {}ms",
                        runtime::date_now().saturating_sub(t1)
                    ));
                    runtime::clear_timeout(handle);
                    resolver.resolve(edits);
                }
            })
            .catch(move |err| {
                runtime::clear_timeout(handle);
                resolver.reject(err);
            });

        let this = self.clone();
        joined.then_maybe(move |values| {
            let mut dto = IWorkspaceEditDto::default();
            for value in values {
                let Dynamic::Array(items) = &value else {
                    continue;
                };
                let items = items.to_vec();
                if !items.iter().all(|item| is_instance_of(item, "TextEdit")) {
                    continue;
                }
                for item in &items {
                    let edit = TextEdit::from_dynamic(item).unwrap_or_else(|error| errors::throw(error));
                    dto.edits.push(WorkspaceEditEntryDto::Text(IWorkspaceTextEdit {
                        resource: document.uri().clone(),
                        text_edit: type_converters::TextEdit::from(&edit),
                        version_id: None,
                        metadata: None,
                    }));
                }
            }

            // apply edits if any and if document
            // didn't change somehow in the meantime
            if dto.edits.is_empty() {
                return MaybePromise::Value(false);
            }

            if version == document.version() {
                return MaybePromise::Promise(this.main_thread_bulk_edits.rpc_try_apply_workspace_edit(SerializableObjectWithBuffers::new(dto), None, None));
            }

            MaybePromise::Promise(Promise::rejected(Error::new("concurrent_edits")))
        })
    }
}
