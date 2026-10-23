//! Porte de `src/vs/base/test/common/observableInternal/logging/debugger/rpc.test.ts` (suíte
//! `SimpleTypedRpcConnection`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: o proxy do original responde `undefined` a `then`, para `await` não repassar a chamada
//! pelo canal. Em Rust `api.requests` e `api.notifications` não são `Future` e não há `await` implícito; o que o
//! caso protege é que o nome `then` nunca vira uma requisição ou notificação, e é isso que se confere.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::marshalling::Marshalled;
use je_base::observable::logging::rpc::{ChannelFactory, IChannel, IChannelHandler, RpcRequestResult, Side, SimpleTypedRpcConnection};
use je_base::promise::Promise;

struct CountingChannel {
    sent: Rc<Cell<usize>>,
}

impl IChannel for CountingChannel {
    fn send_notification(&self, _data: Marshalled) {
        self.sent.set(self.sent.get() + 1);
    }

    fn send_request(&self, _data: Marshalled) -> Promise<RpcRequestResult> {
        self.sent.set(self.sent.get() + 1);
        Promise::resolved(RpcRequestResult::Result(Marshalled::Undefined))
    }
}

#[test]
fn api_proxies_are_not_thenable() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    // A thenable proxy would forward `then` over the channel and never settle.
    let sent = Rc::new(Cell::new(0));
    let channel_sent = sent.clone();
    let factory: ChannelFactory = Box::new(move |_handler: Rc<dyn IChannelHandler>| Rc::new(CountingChannel { sent: channel_sent }) as Rc<dyn IChannel>);
    let connection = SimpleTypedRpcConnection::create_host(factory, || Rc::new(Side::default()));

    // `then` não é uma requisição nem uma notificação: nada vai pelo canal.
    assert!(connection.api.requests.call("then", vec![]).result().is_some_and(|r| r.is_err()));
    connection.api.notifications.send("then", vec![]);
    assert_eq!(sent.get(), 0);

    // Qualquer outro nome vai pelo canal.
    connection.api.notifications.send("log", vec![]);
    let _ = connection.api.requests.call("ping", vec![]);
    assert_eq!(sent.get(), 2);
}
