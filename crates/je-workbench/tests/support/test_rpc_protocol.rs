//! Porte parcial de `src/vs/workbench/api/test/common/testRPCProtocol.ts`.
//!
//! Divergências: só `SingleProxyRPCProtocol` é portado, que é o que `extHostBrowsers.test.ts` usa;
//! `AnyCallRPCProtocol` e `TestRPCProtocol` entram junto do primeiro teste que os pedir. O contexto
//! devolvido é um [`IMainContext`] (as partes de `IExtHostContext` e `IExtHostRpcService` que o original
//! deixa em `undefined!` ou fixas não existem no trait parcial). `mock<T>()` de `base/test/common/mock.ts`
//! cria um objeto vazio que o TypeScript trata como `T`; isso não existe em Rust, então cada teste
//! escreve o seu dublê (ver `ext_host_browsers.rs`), e o `mock.rs` não é portado.

use std::any::Any;
use std::rc::Rc;

use je_workbench::api::common::ext_host_protocol::IMainContext;

/// O contexto de `SingleProxyRPCProtocol(thing)`: `getProxy` devolve sempre `thing`, qualquer que seja o
/// identificador, e `set` não registra nada.
struct SingleProxyRpcProtocol {
    thing: Rc<dyn Any>,
}

impl IMainContext for SingleProxyRpcProtocol {
    fn get_proxy_erased(&self, _sid: &'static str) -> Rc<dyn Any> {
        self.thing.clone()
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

/// `SingleProxyRPCProtocol(thing)`. O `T` é a forma do proxy (por exemplo `dyn MainThreadBrowsersShape`).
pub fn single_proxy_rpc_protocol<T: ?Sized + 'static>(thing: Rc<T>) -> Rc<dyn IMainContext> {
    Rc::new(SingleProxyRpcProtocol { thing: Rc::new(thing) })
}
