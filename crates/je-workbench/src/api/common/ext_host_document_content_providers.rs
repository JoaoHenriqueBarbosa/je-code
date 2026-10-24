//! Porte de `src/vs/workbench/api/common/extHostDocumentContentProviders.ts`: `workspace.registerTextDocumentContentProvider`
//! do lado das extensões. O host guarda os provedores por `handle`, responde ao `$provideTextDocumentContent` da thread
//! principal e, quando o provedor dispara `onDidChange` para um documento aberto, busca o conteúdo novo e avisa a thread
//! principal se as linhas mudaram (serializando as buscas, para não perder eventos rápidos: #179711).
//!
//! Divergências:
//! - `vscode.TextDocumentContentProvider` é o trait [`TextDocumentContentProvider`]: `onDidChange?: Event<Uri>` é
//!   [`TextDocumentContentProvider::on_did_change`] (lido uma vez, no registro, como o `typeof provider.onDidChange ===
//!   'function'` do original; `None` quando o provedor não tem o evento) e `ProviderResult<string>` é
//!   `MaybePromise<Option<JsString>>` (`null` e `undefined` são `None`). O texto é [`JsString`], porque o conteúdo de um
//!   provedor é UTF-16 e `splitLines` do original opera nas unidades.
//! - O provedor é guardado e chamado por `Rc<dyn TextDocumentContentProvider>`; o `CancellationToken.None` do
//!   `$provideTextDocumentContent` é o `CancellationToken::None`.
//! - `registerTextDocumentContentProvider` lança no original (esquema já registrado); aqui devolve [`Result`]. O
//!   `vscode.Disposable` é o [`Disposable`] de `ext_host_types`.
//! - `Object.keys(Schemas).indexOf(scheme) >= 0` compara o esquema com os NOMES das constantes do namespace `Schemas`
//!   (`inMemory`, `vscode`, `file`, `untitled`...), não com os valores (`inmemory`...): é o comportamento do original,
//!   mantido ([`SCHEMAS_KEYS`], copiados de `network.ts`, porque o `je-base` só guarda os valores).
//! - `_handlePool` (estático) é um contador por thread. `ExtHostDocumentContentProvider` é um `Rc`
//!   ([`ExtHostDocumentContentProvider::new`]): o ouvinte de `onDidChange` e o `Disposable` seguram o objeto.
//! - O ouvinte `async uri => { ... }` é uma função `async` ([`async_fn`]) cuja promessa o evento ignora, como o
//!   original; o `then(async value => ...)` interno é [`async_fn_maybe`] (o `return this._proxy.$onVirtualDocumentChange(...)`
//!   devolve uma promessa, adotada com as rodadas de microtarefa do JavaScript). `thisEvent`, referenciado dentro do
//!   próprio `finally`, é um `Rc<RefCell<Option<_>>>` preenchido depois da cadeia; `lastEvent === thisEvent` é
//!   [`Promise::ptr_eq`].
//! - `IMainContext` só serve para pedir o proxy (`MainThreadDocumentContentProviders`) no construtor. `ILogService` é o
//!   [`LogServiceRef`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{async_fn, async_fn_maybe};
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::strings::split_lines;
use je_base::uri::URI;
use je_platform::log::{LogServiceRef, Logger};

use super::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use super::ext_host_protocol::{ExtHostDocumentContentProvidersShape, IMainContext, MainContext, MainThreadDocumentContentProvidersShape};
use super::ext_host_types::Disposable;

/// `Object.keys(Schemas)`: os nomes das constantes do namespace `Schemas` de `base/common/network.ts`.
pub const SCHEMAS_KEYS: &[&str] = &[
    "inMemory",
    "vscode",
    "internal",
    "walkThrough",
    "walkThroughSnippet",
    "vscodeOnboardingSample",
    "http",
    "https",
    "file",
    "mailto",
    "untitled",
    "data",
    "command",
    "vscodeRemote",
    "vscodeRemoteResource",
    "vscodeManagedRemoteResource",
    "vscodeUserData",
    "vscodeCustomEditor",
    "vscodeNotebookCell",
    "vscodeNotebookCellMetadata",
    "vscodeNotebookCellMetadataDiff",
    "vscodeNotebookCellOutput",
    "vscodeNotebookCellOutputDiff",
    "vscodeNotebookMetadata",
    "vscodeInteractiveInput",
    "vscodeSettings",
    "vscodeWorkspaceTrust",
    "vscodeTerminal",
    "vscodeImageCarousel",
    "vscodeChatCodeBlock",
    "vscodeChatCodeCompareBlock",
    "vscodeChatEditor",
    "vscodeChatInput",
    "sessionsChatInput",
    "vscodeLocalChatSession",
    "vscodeChatResponseResource",
    "agentHostTerminal",
    "vscodeChatTerminalOutput",
    "webviewPanel",
    "vscodeWebview",
    "vscodeBrowser",
    "extension",
    "vscodeFileResource",
    "tmp",
    "vsls",
    "vscodeSourceControl",
    "commentsInput",
    "codeSetting",
    "outputChannel",
    "accessibleView",
    "chatEditingSnapshotScheme",
    "chatEditingModel",
    "copilotPr",
];

/// `vscode.TextDocumentContentProvider`.
pub trait TextDocumentContentProvider {
    /// `onDidChange`: o evento que avisa que o conteúdo de um recurso mudou; `None` se o provedor não o tem.
    fn on_did_change(&self) -> Option<Event<URI>> {
        None
    }

    /// `provideTextDocumentContent(uri, token)`.
    fn provide_text_document_content(&self, uri: &URI, token: CancellationToken) -> MaybePromise<Option<JsString>>;
}

thread_local! {
    static HANDLE_POOL: Cell<u64> = const { Cell::new(0) };
}

/// `ExtHostDocumentContentProvider`.
pub struct ExtHostDocumentContentProvider {
    document_content_providers: RefCell<HashMap<u64, Rc<dyn TextDocumentContentProvider>>>,
    proxy: Rc<dyn MainThreadDocumentContentProvidersShape>,
    documents_and_editors: Rc<ExtHostDocumentsAndEditors>,
    log_service: LogServiceRef,
}

impl ExtHostDocumentContentProvider {
    pub fn new(main_context: &dyn IMainContext, documents_and_editors: Rc<ExtHostDocumentsAndEditors>, log_service: LogServiceRef) -> Rc<Self> {
        Rc::new(Self {
            document_content_providers: RefCell::new(HashMap::new()),
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_DOCUMENT_CONTENT_PROVIDERS),
            documents_and_editors,
            log_service,
        })
    }

    pub fn register_text_document_content_provider(self: &Rc<Self>, scheme: &str, provider: Rc<dyn TextDocumentContentProvider>) -> Result<Disposable, Error> {
        // todo@remote
        // check with scheme from fs-providers!
        if SCHEMAS_KEYS.contains(&scheme) {
            return Err(Error::new(format!("scheme '{scheme}' already registered")));
        }

        let handle = HANDLE_POOL.with(|pool| {
            let handle = pool.get();
            pool.set(handle + 1);
            handle
        });

        self.document_content_providers.borrow_mut().insert(handle, provider.clone());
        self.proxy.rpc_register_text_content_provider(handle, scheme);

        let subscription: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        if let Some(on_did_change) = provider.on_did_change() {
            let last_event: Rc<RefCell<Option<Promise<()>>>> = Rc::default();
            let this = self.clone();
            let scheme = scheme.to_owned();

            let listener_subscription = on_did_change.listen(move |uri| {
                let (this, last_event, uri, scheme) = (this.clone(), last_event.clone(), uri.clone(), scheme.clone());
                async_fn(async move {
                    if uri.scheme().to_string_lossy() != scheme {
                        this.log_service.warn(&format!("Provider for scheme '{scheme}' is firing event for schema '{}' which will be IGNORED", uri.scheme()));
                        return Ok(());
                    }
                    if this.documents_and_editors.get_document(&uri).is_none() {
                        // ignore event if document isn't open
                        return Ok(());
                    }

                    let previous = last_event.borrow().clone();
                    if let Some(previous) = previous {
                        previous.await?;
                    }

                    let this_event_slot: Rc<RefCell<Option<Promise<()>>>> = Rc::default();
                    let this_event = this
                        .rpc_provide_text_document_content(handle, &uri)
                        .then_promise({
                            let (this, uri) = (this.clone(), uri.clone());
                            move |value| {
                                async_fn_maybe(async move {
                                    let Some(value) = value else {
                                        return Ok(MaybePromise::Value(()));
                                    };

                                    let Some(document) = this.documents_and_editors.get_document(&uri) else {
                                        // disposed in the meantime
                                        return Ok(MaybePromise::Value(()));
                                    };

                                    // create lines and compare
                                    let lines: Vec<JsString> = split_lines(value.units()).into_iter().map(JsString::from).collect();

                                    // broadcast event when content changed
                                    if !document.equal_lines(&lines) {
                                        return Ok(MaybePromise::Promise(this.proxy.rpc_on_virtual_document_change(&uri, &value)));
                                    }
                                    Ok(MaybePromise::Value(()))
                                })
                            }
                        })
                        .catch(|error| errors::on_unexpected_error(&error))
                        .finally({
                            let (last_event, this_event_slot) = (last_event.clone(), this_event_slot.clone());
                            move || {
                                let is_last = match (last_event.borrow().as_ref(), this_event_slot.borrow().as_ref()) {
                                    (Some(last), Some(current)) => last.ptr_eq(current),
                                    _ => false,
                                };
                                if is_last {
                                    *last_event.borrow_mut() = None;
                                }
                            }
                        });

                    *this_event_slot.borrow_mut() = Some(this_event.clone());
                    *last_event.borrow_mut() = Some(this_event);
                    Ok(())
                });
            });
            *subscription.borrow_mut() = Some(listener_subscription);
        }

        let this = self.clone();
        Ok(Disposable::new(move || {
            if this.document_content_providers.borrow_mut().remove(&handle).is_some() {
                this.proxy.rpc_unregister_text_content_provider(handle);
            }
            let taken = subscription.borrow_mut().take();
            if let Some(subscription) = taken {
                subscription.dispose();
            }
        }))
    }
}

impl ExtHostDocumentContentProvidersShape for ExtHostDocumentContentProvider {
    fn rpc_provide_text_document_content(&self, handle: u64, uri: &URI) -> Promise<Option<JsString>> {
        let provider = self.document_content_providers.borrow().get(&handle).cloned();
        let Some(provider) = provider else {
            return Promise::rejected(Error::new(format!("unsupported uri-scheme: {}", uri.scheme())));
        };
        match provider.provide_text_document_content(uri, CancellationToken::None) {
            MaybePromise::Value(value) => Promise::resolved(value),
            MaybePromise::Promise(promise) => promise,
        }
    }
}
