//! Porte de `src/vs/workbench/api/common/extHostUriOpener.ts`: `window.registerExternalUriOpener` do lado das extensões. O host
//! guarda os openers por id, avisa a thread principal do registro e responde ao `$canOpenUri` e ao `$openUri` dela.
//!
//! Divergências:
//! - `vscode.ExternalUriOpener` é o trait [`ApiExternalUriOpener`]; `ProviderResult<ExternalUriOpenerPriority>` é
//!   `MaybePromise<Option<ExternalUriOpenerPriority>>` (`null` e `undefined` são `None`) e `Thenable<void>` é
//!   `MaybePromise<()>`. `vscode.ExternalUriOpenerMetadata` é [`ApiExternalUriOpenerMetadata`] e o contexto
//!   `vscode.OpenExternalUriContext` (`{ sourceUri, viewColumn }`) é [`ApiOpenExternalUriContext`], com a coluna como o
//!   `number` da API (`ViewColumn.Beside` é `-2`).
//! - A prioridade que o opener devolve é a da API (`extHostTypes`); `$canOpenUri` a entrega como a do editor
//!   (`languages.ExternalUriOpenerPriority`), que tem os mesmos valores. Uma prioridade ausente (`undefined` na API de
//!   JavaScript) é entregue como `None` (o zero), que o serviço de openers descarta do mesmo jeito que o `undefined`.
//! - `registerExternalUriOpener` lança no original; aqui devolve [`Result`], e o `vscode.Disposable` é um [`DisposableRef`].
//! - `async $canOpenUri` e `async $openUri` são `async fn` ([`async_fn`]) e o `return opener.canOpenExternalUri(...)` /
//!   `return opener.openExternalUri(...)` é um `.await` (muda só o número de rodadas de microtarefa, não a ordem das
//!   chamadas). Os `UriComponents` são a [`URI`] e `URI.revive` é a identidade.
//! - `IMainContext` só serve para pedir o proxy (`MainThreadUriOpeners`) no construtor.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::network::schemas;
use je_base::promise::{MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::languages::ExternalUriOpenerPriority;
use je_extensions::manifest::ExtensionIdentifier;

use super::ext_host_protocol::{ExtHostUriOpenersShape, IMainContext, MainContext, MainThreadUriOpenersShape, OpenUriContextDto};
use super::ext_host_types::ExternalUriOpenerPriority as ApiExternalUriOpenerPriority;

/// `vscode.OpenExternalUriContext`: `{ sourceUri: Uri; viewColumn?: ViewColumn }`.
#[derive(Clone, Debug, PartialEq)]
pub struct ApiOpenExternalUriContext {
    pub source_uri: URI,
    pub view_column: Option<f64>,
}

/// `vscode.ExternalUriOpenerMetadata`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiExternalUriOpenerMetadata {
    /// `readonly schemes: readonly string[]`.
    pub schemes: Vec<String>,
    /// `readonly label: string`.
    pub label: String,
}

/// `vscode.ExternalUriOpener`.
pub trait ApiExternalUriOpener {
    /// `canOpenExternalUri(uri, token)`.
    fn can_open_external_uri(&self, uri: &URI, token: CancellationToken) -> MaybePromise<Option<ApiExternalUriOpenerPriority>>;

    /// `openExternalUri(resolvedUri, ctx, token)`.
    fn open_external_uri(&self, resolved_uri: &URI, ctx: &ApiOpenExternalUriContext, token: CancellationToken) -> MaybePromise<()>;
}

/// A prioridade da API como a do editor (os mesmos valores numéricos).
fn to_editor_priority(priority: Option<ApiExternalUriOpenerPriority>) -> ExternalUriOpenerPriority {
    priority.and_then(|priority| ExternalUriOpenerPriority::from_value(priority.value())).unwrap_or(ExternalUriOpenerPriority::None)
}

/// `ExtHostUriOpeners`.
pub struct ExtHostUriOpeners {
    proxy: Rc<dyn MainThreadUriOpenersShape>,
    openers: RefCell<HashMap<String, Rc<dyn ApiExternalUriOpener>>>,
}

impl ExtHostUriOpeners {
    /// `supportedSchemes`: `http` e `https`.
    const SUPPORTED_SCHEMES: [&'static str; 2] = [schemas::HTTP, schemas::HTTPS];

    pub fn new(main_context: &dyn IMainContext) -> Rc<Self> {
        Rc::new(Self { proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_URI_OPENERS), openers: RefCell::new(HashMap::new()) })
    }

    pub fn register_external_uri_opener(
        self: &Rc<Self>,
        extension_id: &ExtensionIdentifier,
        id: &str,
        opener: Rc<dyn ApiExternalUriOpener>,
        metadata: &ApiExternalUriOpenerMetadata,
    ) -> Result<DisposableRef, Error> {
        if self.openers.borrow().contains_key(id) {
            return Err(Error::new(format!("Opener with id '{id}' already registered")));
        }

        if let Some(invalid_scheme) = metadata.schemes.iter().find(|scheme| !Self::SUPPORTED_SCHEMES.contains(&scheme.as_str())) {
            return Err(Error::new(format!("Scheme '{invalid_scheme}' is not supported. Only http and https are currently supported.")));
        }

        self.openers.borrow_mut().insert(id.to_owned(), opener);
        let _ = self.proxy.rpc_register_uri_opener(id, &metadata.schemes, extension_id, &metadata.label);

        let (this, id) = (Rc::clone(self), id.to_owned());
        Ok(to_disposable(move || {
            this.openers.borrow_mut().remove(&id);
            let _ = this.proxy.rpc_unregister_uri_opener(&id);
        }))
    }
}

impl ExtHostUriOpenersShape for ExtHostUriOpeners {
    fn rpc_can_open_uri(&self, id: &str, uri: &URI, token: CancellationToken) -> Promise<ExternalUriOpenerPriority> {
        let opener = self.openers.borrow().get(id).cloned();
        let (id, uri) = (id.to_owned(), uri.clone());
        async_fn(async move {
            let Some(opener) = opener else {
                return Err(Error::new(format!("Unknown opener with id: {id}")));
            };

            Ok(to_editor_priority(await_maybe(opener.can_open_external_uri(&uri, token)).await?))
        })
    }

    fn rpc_open_uri(&self, id: &str, context: OpenUriContextDto, token: CancellationToken) -> Promise<()> {
        let opener = self.openers.borrow().get(id).cloned();
        let id = id.to_owned();
        async_fn(async move {
            let Some(opener) = opener else {
                return Err(Error::new(format!("Unknown opener id: '{id}'")));
            };

            let ctx = ApiOpenExternalUriContext { source_uri: context.source_uri, view_column: context.view_column.map(|column| f64::from(column as i32)) };
            await_maybe(opener.open_external_uri(&context.resolved_uri, &ctx, token)).await
        })
    }
}
