//! Porte de `src/vs/workbench/api/browser/mainThreadUriOpeners.ts`: o lado do workbench do RPC `ExtHostUriOpeners`. Guarda os
//! openers externos de URI que as extensões registram, os entrega ao `ExternalUriOpenerService` (como
//! `IExternalOpenerProvider`) e avisa o usuário, com a ação de abrir pelo opener padrão, quando o opener de uma extensão
//! falha.
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadUriOpeners)` e o `IExtHostContext` do construtor: o registro de clientes não
//!   existe ainda; o construtor recebe o [`IMainContext`] e pede o proxy de `ExtHostUriOpeners`. Os decoradores
//!   (`@IStorageService`, `@IExternalUriOpenerService`, `@IExtensionService`, `@IOpenerService`, `@INotificationService`)
//!   são argumentos de [`MainThreadUriOpeners::new`].
//! - `async *getOpeners` (um `AsyncIterable`) é a `Promise` da lista completa dos openers ([`IExternalOpenerProvider::get_openers`],
//!   ver a nota do `ExternalUriOpenerService`): espera o `activateByEvent` e só então lê o mapa de registrados, como o
//!   gerador, que roda esse trecho na primeira chamada de `next()`.
//! - `createOpener` devolve o [`MainThreadUriOpener`], uma struct com o id, o rótulo e os serviços que o objeto literal do
//!   original captura de `this`. O `_registeredOpeners` (um `Map`) é um [`IndexMap`] compartilhado por `Rc`, porque o
//!   `getOpeners` o lê depois do `await`.
//! - `$registerUriOpener` e `$unregisterUriOpener` são `async` no original: o `throw` é uma promessa rejeitada.
//! - `e.toString()` de um erro é o `Display` de [`Error`] (`Nome: mensagem`); `nls.localize({ key, comment }, ...)` é
//!   `localize(key, ...)`; `openDefaultAction.tooltip = uri.toString()` é `set_tooltip`.
//! - `extends Disposable`: o [`Disposable`] base guarda o registro do provedor e o armazém.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::actions::{Action, ActionCallback, ActionRef, IAction};
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, is_cancellation_error};
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::promise::{MaybePromise, Promise};
use je_base::severity::Severity;
use je_base::uri::URI;
use je_editor::languages::ExternalUriOpenerPriority;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::notification::{INotificationService, Notification, NotificationActions};
use je_platform::opener::{AllowContributedOpeners, DEFAULT_EXTERNAL_URI_OPENER_ID, IOpenerService, OpenOptions, OpenResource};
use je_platform::storage::StorageService;

use crate::api::common::ext_host_protocol::{ExtHostContext, ExtHostUriOpenersShape, IMainContext, MainThreadUriOpenersShape, OpenUriContextDto};
use crate::contrib::external_uri_opener::common::contributed_openers::ContributedExternalUriOpenersStore;
use crate::contrib::external_uri_opener::common::external_uri_opener_service::{
    ExternalUriOpenerContext, IExternalOpenerProvider, IExternalUriOpener, IExternalUriOpenerService,
};
use crate::services::extensions::extensions::IExtensionService;

/// `RegisteredOpenerMetadata`.
struct RegisteredOpenerMetadata {
    schemes: HashSet<String>,
    #[allow(dead_code)] // o original guarda o `extensionId` junto do registro, mas só o armazém o lê (por `$registerUriOpener`)
    extension_id: ExtensionIdentifier,
    label: String,
}

type RegisteredOpeners = Rc<RefCell<IndexMap<String, Rc<RegisteredOpenerMetadata>>>>;

/// O objeto devolvido por `createOpener`.
struct MainThreadUriOpener {
    id: String,
    label: String,
    proxy: Rc<dyn ExtHostUriOpenersShape>,
    opener_service: Rc<dyn IOpenerService>,
    notification_service: Rc<dyn INotificationService>,
}

impl IExternalUriOpener for MainThreadUriOpener {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn can_open(&self, uri: &URI, token: &CancellationToken) -> Promise<ExternalUriOpenerPriority> {
        self.proxy.rpc_can_open_uri(&self.id, uri, token.clone())
    }

    fn open_external_uri(&self, uri: &URI, ctx: &ExternalUriOpenerContext, token: &CancellationToken) -> Promise<bool> {
        let (id, proxy, opener_service, notification_service) =
            (self.id.clone(), Rc::clone(&self.proxy), Rc::clone(&self.opener_service), Rc::clone(&self.notification_service));
        let (uri, ctx, token) = (uri.clone(), ctx.clone(), token.clone());
        async_fn(async move {
            let context = OpenUriContextDto { resolved_uri: uri.clone(), source_uri: ctx.source_uri, view_column: ctx.view_column };
            if let Err(error) = proxy.rpc_open_uri(&id, context, token).await
                && !is_cancellation_error(&error)
            {
                let open_default_action = Self::open_default_action(&opener_service, &uri);

                let error_text = error.to_string();
                let mut notification = Notification::new(Severity::Error, localize("openerFailedMessage", "Could not open uri with '{0}': {1}", &[id.as_str(), error_text.as_str()]));
                notification.actions = Some(NotificationActions { primary: vec![open_default_action], secondary: Vec::new() });
                notification_service.notify(notification);
            }
            Ok(true)
        })
    }
}

impl MainThreadUriOpener {
    /// `openDefaultAction`.
    fn open_default_action(opener_service: &Rc<dyn IOpenerService>, uri: &URI) -> ActionRef {
        let (opener_service, target) = (Rc::clone(opener_service), uri.clone());
        let callback: ActionCallback = Rc::new(move |_event: Dynamic| {
            let options = OpenOptions {
                allow_tunneling: Some(false),
                allow_contributed_openers: Some(AllowContributedOpeners::Id(JsString::from(DEFAULT_EXTERNAL_URI_OPENER_ID))),
                ..OpenOptions::default()
            };
            MaybePromise::Promise(opener_service.open(&OpenResource::Uri(target.clone()), Some(&options)).then(|_| Dynamic::Undefined))
        });
        let action = Action::new("default", &localize("openerFailedUseDefault", "Open using default opener", &[]), "", true, Some(callback));
        action.set_tooltip(uri.to_string(false).to_string_lossy());
        action
    }
}

/// `MainThreadUriOpeners`.
pub struct MainThreadUriOpeners {
    base: Rc<Disposable>,
    proxy: Rc<dyn ExtHostUriOpenersShape>,
    registered_openers: RegisteredOpeners,
    contributed_external_uri_openers_store: Rc<ContributedExternalUriOpenersStore>,
    extension_service: Rc<dyn IExtensionService>,
    opener_service: Rc<dyn IOpenerService>,
    notification_service: Rc<dyn INotificationService>,
}

impl MainThreadUriOpeners {
    pub fn new(
        context: &dyn IMainContext,
        storage_service: Rc<dyn StorageService>,
        external_uri_opener_service: Rc<dyn IExternalUriOpenerService>,
        extension_service: Rc<dyn IExtensionService>,
        opener_service: Rc<dyn IOpenerService>,
        notification_service: Rc<dyn INotificationService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let this = Rc::new(Self {
            base: Rc::clone(&base),
            proxy: context.get_proxy(&ExtHostContext::EXT_HOST_URI_OPENERS),
            registered_openers: Rc::default(),
            contributed_external_uri_openers_store: ContributedExternalUriOpenersStore::new(storage_service, Rc::clone(&extension_service)),
            extension_service,
            opener_service,
            notification_service,
        });

        base.register(external_uri_opener_service.register_external_opener_provider(this.clone()));
        base.register(Rc::clone(&this.contributed_external_uri_openers_store) as DisposableRef);

        this
    }
}

impl IExternalOpenerProvider for MainThreadUriOpeners {
    fn get_openers(&self, target_uri: &URI) -> Promise<Vec<Rc<dyn IExternalUriOpener>>> {
        // Currently we only allow openers for http and https urls
        if *target_uri.scheme() != schemas::HTTP && *target_uri.scheme() != schemas::HTTPS {
            return Promise::resolved(Vec::new());
        }

        let scheme = target_uri.scheme().to_string_lossy();
        let (extension_service, registered_openers) = (Rc::clone(&self.extension_service), Rc::clone(&self.registered_openers));
        let (proxy, opener_service, notification_service) = (Rc::clone(&self.proxy), Rc::clone(&self.opener_service), Rc::clone(&self.notification_service));
        async_fn(async move {
            extension_service.activate_by_event(&format!("onOpenExternalUri:{scheme}"), None).await?;

            let mut openers: Vec<Rc<dyn IExternalUriOpener>> = Vec::new();
            for (id, opener_metadata) in registered_openers.borrow().iter() {
                if opener_metadata.schemes.contains(&scheme) {
                    openers.push(Rc::new(MainThreadUriOpener {
                        id: id.clone(),
                        label: opener_metadata.label.clone(),
                        proxy: Rc::clone(&proxy),
                        opener_service: Rc::clone(&opener_service),
                        notification_service: Rc::clone(&notification_service),
                    }));
                }
            }
            Ok(openers)
        })
    }
}

impl MainThreadUriOpenersShape for MainThreadUriOpeners {
    fn rpc_register_uri_opener(&self, id: &str, schemes: &[String], extension_id: &ExtensionIdentifier, label: &str) -> Promise<()> {
        if self.registered_openers.borrow().contains_key(id) {
            return Promise::rejected(Error::new(format!("Opener with id '{id}' already registered")));
        }

        self.registered_openers.borrow_mut().insert(
            id.to_owned(),
            Rc::new(RegisteredOpenerMetadata { schemes: schemes.iter().cloned().collect(), label: label.to_owned(), extension_id: extension_id.clone() }),
        );

        self.contributed_external_uri_openers_store.did_register_opener(id, &extension_id.value);
        Promise::resolved(())
    }

    fn rpc_unregister_uri_opener(&self, id: &str) -> Promise<()> {
        self.registered_openers.borrow_mut().shift_remove(id);
        self.contributed_external_uri_openers_store.delete(id);
        Promise::resolved(())
    }
}

impl IDisposable for MainThreadUriOpeners {
    fn dispose(&self) {
        self.base.dispose();
        self.registered_openers.borrow_mut().clear();
    }
}
