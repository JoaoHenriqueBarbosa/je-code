//! Porte do `TestRPCProtocol` de `src/vs/workbench/api/test/common/testRPCProtocol.ts` (o `SingleProxyRPCProtocol` está em
//! `test_rpc_protocol.rs`). O arquivo não está em `support/mod.rs`: os testes que o usam o incluem com `#[path]`.
//!
//! Divergências:
//! - No original `getProxy` devolve um `Proxy` que, para qualquer método `$nome`, faz a chamada remota (adiada por um
//!   `setTimeout(0)`, rodada no objeto que `set` registrou e contada para o `sync()`). Em Rust não há reflexão: o teste
//!   escreve, para cada forma (`MainThreadXShape`, `ExtHostXShape`), um proxy que encaminha cada método por
//!   [`TestRpcProtocol::remote_call`] e o registra com [`TestRpcProtocol::register_proxy`]; o proxy procura o objeto local com
//!   [`TestRpcProtocol::local`] só quando a chamada roda, como o original (os dois lados se pedem o proxy no construtor antes
//!   de o outro existir).
//! - `simulateWireTransfer` (o `JSON.parse(JSON.stringify(...))` dos argumentos e do resultado) não tem equivalente: os valores
//!   do porte já são imutáveis ou clonados, e o que sobreviveria à serialização é o mesmo.
//! - `drain` e `assertRegistered` (que o original deixa sem uso nos testes) não são portados.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::promise::{Promise, Resolver};
use je_base::runtime;
use je_workbench::api::common::ext_host_protocol::{IMainContext, ProxyIdentifier};

/// `new Promise(c => setTimeout(c, 0))`.
fn macrotask() -> Promise<()> {
    Promise::with_executor(|resolver| {
        runtime::set_timeout(move || resolver.resolve(()), 0);
    })
}

/// `TestRPCProtocol`.
#[derive(Default)]
pub struct TestRpcProtocol {
    /// `_locals`: o objeto que `set` registrou para cada identificador (um `Rc<T>` guardado como `Rc<dyn Any>`).
    locals: RefCell<HashMap<&'static str, Rc<dyn Any>>>,
    /// `_proxies`: o proxy que `getProxy` devolve para cada identificador (idem).
    proxies: RefCell<HashMap<&'static str, Rc<dyn Any>>>,
    /// `_callCountValue`.
    call_count: Cell<usize>,
    /// `_idle` e `_completeIdle`.
    idle: RefCell<Option<(Promise<()>, Resolver<()>)>>,
}

impl TestRpcProtocol {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// `set(identifier, value)`: registra o objeto local e o devolve.
    pub fn set<T: ?Sized + 'static>(&self, identifier: &ProxyIdentifier<T>, value: Rc<T>) -> Rc<T> {
        self.locals.borrow_mut().insert(identifier.sid, Rc::new(value.clone()));
        value
    }

    /// `this._locals[proxyId]`, lido quando a chamada roda.
    pub fn local<T: ?Sized + 'static>(&self, identifier: &ProxyIdentifier<T>) -> Rc<T> {
        self.locals
            .borrow()
            .get(identifier.sid)
            .and_then(|local| local.downcast_ref::<Rc<T>>())
            .cloned()
            .unwrap_or_else(|| panic!("nenhum objeto registrado para '{}'", identifier.sid))
    }

    /// O proxy que `getProxy(identifier)` devolve (ver as notas do módulo).
    pub fn register_proxy<T: ?Sized + 'static>(&self, identifier: &ProxyIdentifier<T>, proxy: Rc<T>) {
        self.proxies.borrow_mut().insert(identifier.sid, Rc::new(proxy));
    }

    /// `_remoteCall(proxyId, path, args)`: conta a chamada, espera uma macrotarefa, roda `call` no objeto local e só
    /// desconta a chamada quando ela termina (resolvida ou rejeitada).
    pub fn remote_call<T: Clone + 'static>(self: &Rc<Self>, call: impl FnOnce() -> Promise<T> + 'static) -> Promise<T> {
        self.call_count.set(self.call_count.get() + 1);

        let this = Rc::clone(self);
        macrotask().then_promise(move |_| {
            call().then_result_promise(move |outcome| {
                this.finish_call();
                match outcome {
                    Ok(result) => Promise::resolved(result),
                    Err(error) => Promise::rejected(error),
                }
            })
        })
    }

    /// O `set _callCount` do original: ao chegar a zero, completa o `_idle` pendente.
    fn finish_call(&self) {
        self.call_count.set(self.call_count.get() - 1);
        if self.call_count.get() == 0 {
            let idle = self.idle.borrow_mut().take();
            if let Some((_, complete_idle)) = idle {
                complete_idle.resolve(());
            }
        }
    }

    /// `sync()`: espera uma macrotarefa e depois as chamadas remotas em voo.
    pub fn sync(self: &Rc<Self>) -> Promise<()> {
        let this = Rc::clone(self);
        macrotask().then_promise(move |_| {
            if this.call_count.get() == 0 {
                return Promise::resolved(());
            }
            this.idle.borrow_mut().get_or_insert_with(Promise::new).0.clone()
        })
    }
}

impl IMainContext for TestRpcProtocol {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        self.proxies.borrow().get(sid).cloned().unwrap_or_else(|| panic!("nenhum proxy registrado para '{sid}'"))
    }

    fn set_erased(&self, sid: &'static str, value: Rc<dyn Any>) {
        self.locals.borrow_mut().insert(sid, value);
    }
}
