//! Porte de `src/vs/workbench/api/test/browser/mainThreadShare.test.ts` (suíte `MainThreadShare`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo do caso (`store`).
//! - `mock<IShareService>()` com `registerShareProvider` e `mock<IExtHostContext>()` com `getProxy(): any { return {}; }` são o
//!   [`TestShareService`] (só `registerShareProvider` tem comportamento) e o `single_proxy_rpc_protocol` sobre um
//!   [`EmptyExtHostShare`]: o proxy `{}` do original nunca é chamado neste caso. `remoteAuthority` e `extensionHostKind` do
//!   `IExtHostContext` não existem no [`IMainContext`] parcial e o `MainThreadShare` não os lê.
//! - `service.$registerShareProvider(...)` e `$unregisterShareProvider(...)` são `rpc_register_share_provider` e
//!   `rpc_unregister_share_provider`; `selector: []` é o vetor vazio.
//! - `getShareActions` e `provideShare` do `IShareService` não são usados pelo caso: o dublê os devolve vazios.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{to_disposable, DisposableRef};
use je_base::promise::Promise;
use je_platform::actions::actions::SubmenuItem;
use je_workbench::api::browser::main_thread_share::MainThreadShare;
use je_workbench::api::common::ext_host_protocol::{ExtHostShareShape, MainThreadShareShape};
use je_workbench::contrib::share::common::share::{IShareProvider, IShareService, IShareableItem, ShareResult};
use support::test_rpc_protocol::single_proxy_rpc_protocol;

/// `new class extends mock<IShareService>() { override registerShareProvider(_provider) { return toDisposable(() => registrationDisposed = true); } }`.
struct TestShareService {
    registration_disposed: Rc<Cell<bool>>,
}

impl IShareService for TestShareService {
    fn register_share_provider(&self, _provider: Rc<dyn IShareProvider>) -> DisposableRef {
        let registration_disposed = self.registration_disposed.clone();
        to_disposable(move || registration_disposed.set(true))
    }

    fn get_share_actions(&self) -> Vec<SubmenuItem> {
        Vec::new()
    }

    fn provide_share(&self, _item: &IShareableItem, _token: CancellationToken) -> Promise<Option<ShareResult>> {
        Promise::resolved(None)
    }
}

/// O `{}` que o `getProxy()` do original devolve.
struct EmptyExtHostShare;

impl ExtHostShareShape for EmptyExtHostShare {
    fn rpc_provide_share(&self, _handle: u32, _item: IShareableItem, _token: CancellationToken) -> Promise<Option<ShareResult>> {
        Promise::resolved(None)
    }
}

#[test]
fn disposes_share_provider_registrations_when_they_are_unregistered() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let registration_disposed = Rc::new(Cell::new(false));
    let share_service: Rc<dyn IShareService> = Rc::new(TestShareService { registration_disposed: registration_disposed.clone() });
    let proxy: Rc<dyn ExtHostShareShape> = Rc::new(EmptyExtHostShare);
    let ext_host_context = single_proxy_rpc_protocol(proxy);
    let service = store.add_rc(MainThreadShare::new(&*ext_host_context, share_service));

    service.rpc_register_share_provider(1, Vec::new(), "test", "Test", 1);
    service.rpc_unregister_share_provider(1);

    assert!(registration_disposed.get());
}
