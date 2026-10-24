//! Porte de `src/vs/workbench/api/common/extHostDecorations.ts`: `window.registerFileDecorationProvider` do lado das
//! extensões. O host guarda os provedores por `handle`, avisa a thread principal quando um provedor muda decorações (limitando
//! o tamanho do evento) e responde ao `$provideDecorations`, consultando todos os recursos do pedido em paralelo.
//!
//! Divergências:
//! - `vscode.FileDecorationProvider` é o trait [`FileDecorationProvider`]: `onDidChangeFileDecorations?: Event<undefined | Uri |
//!   Uri[]>` é [`FileDecorationProvider::on_did_change_file_decorations`] (lido uma vez, no registro; `None` quando o provedor não
//!   tem o evento) com o payload `Option<OneOrMany<URI>>`, e `ProviderResult<FileDecoration>` é `MaybePromise<Option<FileDecoration>>`.
//! - `_handlePool` (estático) é um contador por thread. `ExtHostDecorations` é um `Rc` ([`ExtHostDecorations::new`]): o
//!   `Disposable` do registro e o ouvinte seguram o objeto. `IExtHostRpcService` só serve para pedir o proxy
//!   (`MainThreadDecorations`) no construtor, como `IMainContext`.
//! - `Object.create(null)` do `DecorationReply` é o [`DecorationReply`] vazio; o `result[id] = [propagate, tooltip, badge, color]`
//!   é o [`DecorationData`] de mesmos campos.
//! - O `console.warn` de `this._logService.warn('[Decorations] CAPPING ...', id, length)` junta os argumentos ao texto, e o
//!   `${e}` do aviso de decoração inválida é o `Display` do erro.
//! - `FileDecoration.validate(data)` devolve `Result<bool, _>` aqui; só o erro importa.
//! - `data.badge && typeof data.badge !== 'string'` é o `badge` ser a variante [`FileDecorationBadge::ThemeIcon`].
//! - `dirname` é o do `path` `posix` do processo (`path.dirname` do `base/common/path.js` na plataforma de `uri.path`, que é
//!   sempre com `/`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::arrays::{OneOrMany, as_array, group_by};
use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::DisposableRef;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::{MaybePromise, Promise};
use je_base::strings::{compare, count};
use je_base::uri::URI;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogMessage, LogServiceRef, Logger};

use super::ext_host_protocol::{
    DecorationData, DecorationReply, DecorationRequest, ExtHostDecorationsShape, IMainContext, MainContext, MainThreadDecorationsShape,
};
use super::ext_host_types::{Disposable, FileDecoration, FileDecorationBadge};
use crate::services::extensions::extensions::check_proposed_api_enabled;

/// `vscode.FileDecorationProvider`.
pub trait FileDecorationProvider {
    /// `onDidChangeFileDecorations`: os recursos cujas decorações mudaram (`undefined` para todos); `None` se o provedor não tem o evento.
    fn on_did_change_file_decorations(&self) -> Option<Event<Option<OneOrMany<URI>>>> {
        None
    }

    /// `provideFileDecoration(uri, token)`.
    fn provide_file_decoration(&self, uri: &URI, token: CancellationToken) -> MaybePromise<Option<FileDecoration>>;
}

/// `ProviderData`.
struct ProviderData {
    provider: Rc<dyn FileDecorationProvider>,
    extension_description: Rc<ExtensionDescription>,
}

thread_local! {
    static HANDLE_POOL: Cell<u32> = const { Cell::new(0) };
}

/// `ExtHostDecorations._maxEventSize`.
const MAX_EVENT_SIZE: usize = 250;

/// `ExtHostDecorations`.
pub struct ExtHostDecorations {
    provider: RefCell<HashMap<u32, ProviderData>>,
    proxy: Rc<dyn MainThreadDecorationsShape>,
    log_service: LogServiceRef,
}

impl ExtHostDecorations {
    pub fn new(main_context: &dyn IMainContext, log_service: LogServiceRef) -> Rc<Self> {
        Rc::new(Self { provider: RefCell::new(HashMap::new()), proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_DECORATIONS), log_service })
    }

    pub fn register_file_decoration_provider(self: &Rc<Self>, provider: Rc<dyn FileDecorationProvider>, extension_description: &ExtensionDescription) -> Disposable {
        let handle = HANDLE_POOL.with(|pool| {
            let handle = pool.get();
            pool.set(handle + 1);
            handle
        });
        self.provider.borrow_mut().insert(handle, ProviderData { provider: provider.clone(), extension_description: Rc::new(extension_description.clone()) });
        self.proxy.rpc_register_decoration_provider(handle, &extension_description.identifier.value);

        let listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        if let Some(on_did_change_file_decorations) = provider.on_did_change_file_decorations() {
            let (proxy, log_service, extension_id) = (self.proxy.clone(), self.log_service.clone(), extension_description.identifier.value.clone());
            *listener.borrow_mut() = Some(on_did_change_file_decorations.listen(move |e| {
                let Some(e) = e else {
                    proxy.rpc_on_did_change(handle, None);
                    return;
                };
                let array = as_array(e.clone());
                if array.len() <= MAX_EVENT_SIZE {
                    proxy.rpc_on_did_change(handle, Some(array));
                    return;
                }

                // too many resources per event. pick one resource per folder, starting
                // with parent folders
                log_service.warn(&format!("[Decorations] CAPPING events from decorations provider {extension_id} {}", array.len()));
                let mapped: Vec<(URI, usize)> = array
                    .into_iter()
                    .map(|uri| {
                        let rank = count(uri.path().units(), &[u16::from(b'/')]);
                        (uri, rank)
                    })
                    .collect();
                let groups = group_by(&mapped, |a, b| {
                    let rank = a.1 as f64 - b.1 as f64;
                    if rank != 0.0 { rank } else { f64::from(compare(a.0.path().units(), b.0.path().units())) }
                });
                let posix = PathModule::posix(&ProcessEnvironment);
                let mut picked: Vec<URI> = Vec::new();
                'outer: for uris in groups {
                    let mut last_dirname: Option<String> = None;
                    for (uri, _) in uris {
                        let my_dirname = posix.dirname(&uri.path().to_string_lossy());
                        if last_dirname.as_deref() != Some(my_dirname.as_str()) {
                            last_dirname = Some(my_dirname);
                            picked.push(uri);
                            if picked.len() >= MAX_EVENT_SIZE {
                                break 'outer;
                            }
                        }
                    }
                }
                proxy.rpc_on_did_change(handle, Some(picked));
            }));
        }

        let this = self.clone();
        Disposable::new(move || {
            if let Some(listener) = listener.borrow().as_ref() {
                listener.dispose();
            }
            this.proxy.rpc_unregister_decoration_provider(handle);
            this.provider.borrow_mut().remove(&handle);
        })
    }
}

impl ExtHostDecorationsShape for ExtHostDecorations {
    fn rpc_provide_decorations(&self, handle: u32, requests: Vec<DecorationRequest>, token: CancellationToken) -> Promise<DecorationReply> {
        let data = self.provider.borrow().get(&handle).map(|data| (data.provider.clone(), data.extension_description.clone()));
        let Some((provider, extension)) = data else {
            // might have been unregistered in the meantime
            return Promise::resolved(DecorationReply::new());
        };

        let result: Rc<RefCell<DecorationReply>> = Rc::default();
        let tasks: Vec<Promise<()>> = requests
            .into_iter()
            .map(|request| {
                let (provider, extension, token, result, log_service) = (provider.clone(), extension.clone(), token.clone(), result.clone(), self.log_service.clone());
                async_fn(async move {
                    let DecorationRequest { uri, id } = request;
                    let data = match await_maybe(provider.provide_file_decoration(&uri, token)).await {
                        Ok(data) => data,
                        Err(err) => {
                            log_service.error_args(&LogMessage::Error(err), &[]);
                            return Ok(());
                        }
                    };
                    let Some(data) = data else {
                        return Ok(());
                    };
                    let validated: Result<(), Error> = FileDecoration::validate(&data).and_then(|_| match &data.badge {
                        Some(FileDecorationBadge::ThemeIcon(_)) => check_proposed_api_enabled(&extension, "codiconDecoration"),
                        _ => Ok(()),
                    });
                    match validated {
                        Ok(()) => {
                            result.borrow_mut().insert(id, DecorationData { propagate: data.propagate, tooltip: data.tooltip, badge: data.badge, color: data.color });
                        }
                        Err(e) => log_service.warn(&format!("INVALID decoration from extension '{}': {e}", extension.identifier.value)),
                    }
                    Ok(())
                })
            })
            .collect();

        async_fn(async move {
            Promise::all(tasks).await?;
            Ok(result.take())
        })
    }
}
