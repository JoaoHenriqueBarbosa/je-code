//! Testes próprios de `observable/logging/rpc.rs`, além do caso do `rpc.test.ts`: duas conexões ligadas por um
//! canal de memória (requisições com resposta, assíncrona e com erro, notificações e nome desconhecido). Não é
//! porte de teste do VS Code.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::run_with_faked_timers;
use je_base::errors::Error;
use je_base::marshalling::Marshalled;
use je_base::observable::logging::rpc::{ChannelFactory, IChannel, IChannelHandler, RpcRequestResult, Side, SimpleTypedRpcConnection};
use je_base::promise::{MaybePromise, Promise};

type Slot = Rc<RefCell<Option<Rc<dyn IChannelHandler>>>>;

/// Um lado do canal: o que ele envia chama o `IChannelHandler` do outro lado.
struct Link {
    peer: Slot,
}

impl Link {
    fn peer(&self) -> Rc<dyn IChannelHandler> {
        self.peer.borrow().clone().expect("o outro lado existe")
    }
}

impl IChannel for Link {
    fn send_notification(&self, data: Marshalled) {
        self.peer().handle_notification(&data);
    }

    fn send_request(&self, data: Marshalled) -> Promise<RpcRequestResult> {
        match self.peer().handle_request(&data) {
            MaybePromise::Value(value) => Promise::resolved(value),
            MaybePromise::Promise(promise) => promise,
        }
    }
}

fn factory(own: Slot, peer: Slot) -> ChannelFactory {
    Box::new(move |handler: Rc<dyn IChannelHandler>| {
        *own.borrow_mut() = Some(handler);
        Rc::new(Link { peer }) as Rc<dyn IChannel>
    })
}

fn num(n: f64) -> Marshalled {
    Marshalled::Number(n)
}

#[test]
fn requests_notifications_and_errors_cross_the_connection() {
    run_with_faked_timers(async move {
        let seen: Rc<RefCell<Vec<String>>> = Rc::default();

        let mut host_side = Side::default();
        let log = seen.clone();
        host_side.notifications.insert("log".into(), Box::new(move |args| log.borrow_mut().push(format!("{args:?}"))));
        host_side.requests.insert("add".into(), Box::new(|args| MaybePromise::Value(num(args.iter().map(|a| if let Marshalled::Number(n) = a { *n } else { 0.0 }).sum()))));
        host_side.requests.insert("later".into(), Box::new(|_| MaybePromise::Promise(Promise::resolved(Marshalled::String("depois".into())))));
        host_side.requests.insert("boom".into(), Box::new(|_| je_base::errors::throw(Error::new("falhou"))));
        host_side.requests.insert("rejects".into(), Box::new(|_| MaybePromise::Promise(Promise::rejected(Error::new("rejeitou")))));
        let host_side = Rc::new(host_side);

        let (host_slot, client_slot): (Slot, Slot) = (Rc::default(), Rc::default());
        let side = host_side.clone();
        let _host = SimpleTypedRpcConnection::create_host(factory(host_slot.clone(), client_slot.clone()), move || side.clone());
        let client = SimpleTypedRpcConnection::create_client(factory(client_slot, host_slot), || Rc::new(Side::default()));

        let sum = client.api.requests.call("add", vec![num(2.0), num(5.0)]).await.unwrap();
        assert!(matches!(sum, Marshalled::Number(n) if n == 7.0));

        let later = client.api.requests.call("later", vec![]).await.unwrap();
        assert!(matches!(&later, Marshalled::String(s) if s.to_string_lossy() == "depois"));

        assert_eq!(client.api.requests.call("boom", vec![]).await.expect_err("devia rejeitar").message, "falhou");
        assert_eq!(client.api.requests.call("rejects", vec![]).await.expect_err("devia rejeitar").message, "rejeitou");
        assert!(client.api.requests.call("unknown", vec![]).await.is_err());

        client.api.notifications.send("log", vec![num(1.0), Marshalled::String("x".into())]);
        assert_eq!(seen.borrow().len(), 1);
    });
}
