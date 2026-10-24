//! Porte de `src/vs/workbench/api/browser/mainThreadShare.ts`: o lado do workbench do RPC `ExtHostShare`, que registra no
//! [`IShareService`] os provedores de compartilhamento das extensões.
//!
//! Divergências:
//! - **Injeção.** O construtor `(extHostContext, @IShareService)` é [`MainThreadShare::new`];
//!   `@extHostNamedCustomer(MainContext.MainThreadShare)` é o registro do `extHostCustomers.ts`, que não foi portado.
//! - **`selector: IDocumentFilterDto[]`** vira o [`LanguageSelector`] (o mesmo de `main_thread_language_features.rs`).
//! - **`URI.revive(result)`** é a identidade: o proxy já devolve a [`URI`] revivida (ver o protocolo).
//! - O provedor registrado no serviço é um struct próprio ([`MainThreadShareProvider`]) no lugar do literal
//!   `{ id, label, selector, priority, provideShare }`.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{DisposableMap, IDisposable};
use je_base::promise::Promise;
use je_editor::language_selector::LanguageSelector;

use super::main_thread_language_features::to_selector;
use crate::api::common::ext_host_protocol::{ExtHostContext, ExtHostShareShape, IDocumentFilterDto, IMainContext, MainThreadShareShape};
use crate::contrib::share::common::share::{IShareProvider, IShareService, IShareableItem, ShareResult};

/// O provedor que o `$registerShareProvider` registra no serviço.
struct MainThreadShareProvider {
    id: String,
    label: String,
    selector: LanguageSelector,
    priority: i32,
    handle: u32,
    proxy: Rc<dyn ExtHostShareShape>,
}

impl IShareProvider for MainThreadShareProvider {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn label(&self) -> String {
        self.label.clone()
    }

    fn priority(&self) -> i32 {
        self.priority
    }

    fn selector(&self) -> LanguageSelector {
        self.selector.clone()
    }

    fn provide_share(&self, item: &IShareableItem, _token: CancellationToken) -> Promise<Option<ShareResult>> {
        let (proxy, handle, item) = (self.proxy.clone(), self.handle, item.clone());
        async_fn(async move { proxy.rpc_provide_share(handle, item, CancellationToken::None).await })
    }
}

/// `MainThreadShare`.
pub struct MainThreadShare {
    proxy: Rc<dyn ExtHostShareShape>,
    provider_disposables: Rc<DisposableMap<u32, dyn IDisposable>>,
    share_service: Rc<dyn IShareService>,
}

impl MainThreadShare {
    pub fn new(ext_host_context: &dyn IMainContext, share_service: Rc<dyn IShareService>) -> Rc<Self> {
        Rc::new(Self {
            proxy: ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_SHARE),
            provider_disposables: DisposableMap::new(),
            share_service,
        })
    }
}

impl MainThreadShareShape for MainThreadShare {
    fn rpc_register_share_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, id: &str, label: &str, priority: i32) {
        let provider = MainThreadShareProvider {
            id: id.to_owned(),
            label: label.to_owned(),
            selector: to_selector(&selector),
            priority,
            handle,
            proxy: self.proxy.clone(),
        };
        let disposable = self.share_service.register_share_provider(Rc::new(provider));
        self.provider_disposables.set(handle, disposable, false);
    }

    fn rpc_unregister_share_provider(&self, handle: u32) {
        self.provider_disposables.delete_and_dispose(&handle);
    }
}

impl IDisposable for MainThreadShare {
    fn dispose(&self) {
        self.provider_disposables.dispose();
    }
}
