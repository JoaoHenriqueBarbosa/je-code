//! Porte de `src/vs/workbench/api/common/extHostNotebookDocumentSaveParticipant.ts`: o `onWillSaveNotebookDocument` do lado das
//! extensões. A thread principal pede `$participateInSave`, o host entrega o evento a cada ouvinte registrado, espera as
//! promessas de `waitUntil` e aplica as edições de workspace devolvidas.
//!
//! Divergências:
//! - **Entrega.** O original usa o `AsyncEmitter` de `base/common/event.ts`, cujo `waitUntil` guarda o valor da promessa (que
//!   `promiseJoin` lê). O `AsyncEmitter` do `je-base` recebe `Promise<()>` e perde o valor, então a entrega é própria e segue o
//!   mesmo algoritmo de `fireAsync`: os ouvintes são copiados para uma fila; enquanto houver ouvinte e o `token` não tiver sido
//!   cancelado, o próximo é chamado de forma síncrona (um erro dele vai para `onUnexpectedError` e a fila segue), a coleção de
//!   `waitUntil` é congelada, e as promessas (cada uma passada por `promiseJoin`) são esperadas com `allSettled` (uma rejeitada
//!   é ignorada).
//! - **`vscode.NotebookDocumentWillSaveEvent`** é a [`NotebookDocumentWillSaveEvent`]. O `waitUntil` do original aceita qualquer
//!   valor e só usa o `WorkspaceEdit`; o daqui é [`NotebookDocumentWillSaveEvent::wait_until`], que recebe
//!   `Promise<Option<WorkspaceEdit>>`: `None` é o valor falso (ignorado), e o ramo `ignored due to invalid data` (valor
//!   verdadeiro que não é `WorkspaceEdit`) não existe, porque o tipo o impede. Escrever depois que o ouvinte retornou devolve o
//!   `Err` com `waitUntil can NOT be called asynchronous`.
//! - **`getOnWillSaveNotebookDocumentEvent(extension)`** devolve o [`Event`] de `je-base`: o `thisArg` não existe (o fecho captura
//!   o que precisar) e o `wrappedListener.extension` é a [`ListenerEntry`] (o ouvinte e a extensão que o registrou).
//! - `Date.now()` do prazo de cada ouvinte é o [`StopWatch`] com o relógio de `Date.now`. O `thresholds.errors` do construtor não
//!   é lido (o original também não o lê).
//! - `$participateInSave` é uma função `async` ([`async_fn`]); o `throw new Error('Unable to resolve notebook document')`
//!   rejeita a promessa.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::{Event, Listener};
use je_base::lifecycle::{IDisposable, plain_disposable};
use je_base::linked_list::LinkedList;
use je_base::promise::Promise;
use je_base::stopwatch::StopWatch;
use je_base::uri::URI;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};

use super::ext_host_document_save_participant::SaveParticipantThresholds;
use super::ext_host_notebook::ExtHostNotebookController;
use super::ext_host_notebook_document::NotebookDocument;
use super::ext_host_protocol::{ExtHostNotebookDocumentSaveParticipantShape, IWorkspaceEditDto, MainThreadBulkEditsShape, SerializableObjectWithBuffers};
use super::ext_host_type_converters as type_converters;
use super::ext_host_types as types;
use crate::common::editor::SaveReason;

/// `vscode.NotebookDocumentWillSaveEvent`.
#[derive(Clone)]
pub struct NotebookDocumentWillSaveEvent {
    pub notebook: NotebookDocument,
    pub reason: types::TextDocumentSaveReason,
    /// O `token` com que `fireAsync` foi chamado.
    pub token: CancellationToken,
    thenables: Rc<RefCell<Vec<Promise<Option<types::WorkspaceEdit>>>>>,
    thenables_frozen: Rc<Cell<bool>>,
}

impl NotebookDocumentWillSaveEvent {
    /// `waitUntil(thenable)`: só pode ser chamado de forma síncrona, durante o ouvinte.
    pub fn wait_until(&self, thenable: Promise<Option<types::WorkspaceEdit>>) -> Result<(), Error> {
        if self.thenables_frozen.get() {
            return Err(Error::new("waitUntil can NOT be called asynchronous"));
        }
        self.thenables.borrow_mut().push(thenable);
        Ok(())
    }
}

type SaveListener = Listener<NotebookDocumentWillSaveEvent>;

/// `IExtensionListener<E>`: o ouvinte e a extensão que o registrou.
#[derive(Clone)]
pub struct ListenerEntry {
    pub listener: SaveListener,
    pub extension: ExtensionDescription,
}

struct Inner {
    callbacks: LinkedList<ListenerEntry>,
    log_service: LogServiceRef,
    notebooks_and_editors: Rc<ExtHostNotebookController>,
    main_thread_bulk_edits: Rc<dyn MainThreadBulkEditsShape>,
    thresholds: SaveParticipantThresholds,
}

/// `ExtHostNotebookDocumentSaveParticipant`.
pub struct ExtHostNotebookDocumentSaveParticipant {
    inner: Rc<Inner>,
}

impl ExtHostNotebookDocumentSaveParticipant {
    /// O construtor com os limites padrão (`{ timeout: 1500, errors: 3 }`).
    pub fn new(log_service: LogServiceRef, notebooks_and_editors: Rc<ExtHostNotebookController>, main_thread_bulk_edits: Rc<dyn MainThreadBulkEditsShape>) -> Self {
        Self::with_thresholds(log_service, notebooks_and_editors, main_thread_bulk_edits, SaveParticipantThresholds::default())
    }

    pub fn with_thresholds(
        log_service: LogServiceRef,
        notebooks_and_editors: Rc<ExtHostNotebookController>,
        main_thread_bulk_edits: Rc<dyn MainThreadBulkEditsShape>,
        thresholds: SaveParticipantThresholds,
    ) -> Self {
        Self { inner: Rc::new(Inner { callbacks: LinkedList::new(), log_service, notebooks_and_editors, main_thread_bulk_edits, thresholds }) }
    }

    /// `getOnWillSaveNotebookDocumentEvent(extension)`.
    pub fn get_on_will_save_notebook_document_event(&self, extension: &ExtensionDescription) -> Event<NotebookDocumentWillSaveEvent> {
        let inner = self.inner.clone();
        let extension = extension.clone();
        Event::new(move |listener: SaveListener| {
            let remove = inner.callbacks.push(ListenerEntry { listener, extension: extension.clone() });
            plain_disposable(move || remove.remove())
        })
    }
}

impl IDisposable for ExtHostNotebookDocumentSaveParticipant {
    fn dispose(&self) {}
}

impl Inner {
    /// O `promiseJoin` de `fireAsync`: espera a promessa do ouvinte, avisa quando passou do prazo e guarda o `WorkspaceEdit`.
    fn join(self: &Rc<Self>, thenable: Promise<Option<types::WorkspaceEdit>>, extension: &ExtensionDescription, token: CancellationToken, edits: Rc<RefCell<Vec<types::WorkspaceEdit>>>) -> Promise<()> {
        let (inner, extension) = (self.clone(), extension.clone());
        async_fn(async move {
            let sw = StopWatch::new(Some(false));
            let data = thenable.await?;
            if sw.elapsed() > inner.thresholds.timeout as f64 {
                inner.log_service.warn_args(&LogMessage::from("onWillSaveNotebookDocument-listener from extension"), &[LogArg::from(extension.identifier.value.as_str())]);
            }

            if token.is_cancellation_requested() {
                return Ok(());
            }

            if let Some(edit) = data {
                edits.borrow_mut().push(edit);
            }

            Ok(())
        })
    }
}

impl ExtHostNotebookDocumentSaveParticipantShape for ExtHostNotebookDocumentSaveParticipant {
    fn rpc_participate_in_save(&self, resource: &URI, reason: SaveReason, token: CancellationToken) -> Promise<bool> {
        let inner = self.inner.clone();
        let resource = resource.clone();
        async_fn(async move {
            let Some(document) = inner.notebooks_and_editors.try_get_notebook_document(&resource) else {
                return Err(Error::new("Unable to resolve notebook document"));
            };

            let edits: Rc<RefCell<Vec<types::WorkspaceEdit>>> = Rc::default();

            // `this._onWillSaveNotebookDocumentEvent.fireAsync(...)`
            let mut queue: std::collections::VecDeque<ListenerEntry> = inner.callbacks.iter().collect();
            while !queue.is_empty() && !token.is_cancellation_requested() {
                let Some(entry) = queue.pop_front() else {
                    break;
                };
                let event = NotebookDocumentWillSaveEvent {
                    notebook: document.api_notebook(),
                    reason: type_converters::TextDocumentSaveReason::to(reason),
                    token: token.clone(),
                    thenables: Rc::default(),
                    thenables_frozen: Rc::new(Cell::new(false)),
                };
                if let Err(error) = errors::catch(|| (entry.listener)(&event)) {
                    errors::on_unexpected_error(&error);
                    continue;
                }

                // freeze thenables-collection to enforce sync-calls to wait until and then wait for all thenables to resolve
                event.thenables_frozen.set(true);
                let thenables: Vec<_> = event.thenables.borrow_mut().drain(..).collect();
                let joined: Vec<Promise<()>> = thenables.into_iter().map(|thenable| inner.join(thenable, &entry.extension, token.clone(), edits.clone())).collect();
                Promise::<()>::all_settled(joined).await?;
            }

            if token.is_cancellation_requested() {
                return Ok(false);
            }

            let edits = edits.borrow().clone();
            if edits.is_empty() {
                return Ok(true);
            }

            let mut dto = IWorkspaceEditDto { edits: Vec::new() };
            for edit in &edits {
                dto.edits.extend(type_converters::WorkspaceEdit::from(edit, None).edits);
            }

            inner.main_thread_bulk_edits.rpc_try_apply_workspace_edit(SerializableObjectWithBuffers::new(dto), None, None).await
        })
    }
}
