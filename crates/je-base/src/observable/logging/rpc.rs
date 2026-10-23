//! Porte de `observableInternal/logging/debugger/rpc.ts`: uma conexão RPC tipada e simples, com requisições
//! (que respondem) e notificações (que não respondem), sobre um canal que o chamador fornece.
//!
//! Os `Proxy` do original (`api.requests.nome(...)` e `api.notifications.nome(...)`, que viram uma mensagem
//! `[nome, args]` a partir do nome da propriedade) são [`RpcRequests::call`] e [`RpcNotifications::send`]. O
//! nome `then` não existe no proxy (para `await` não repassá-lo pelo canal): `call("then", ...)` rejeita e
//! `send("then", ...)` não envia nada. Os valores `unknown` são [`Marshalled`].

use std::collections::HashMap;
use std::rc::Rc;

use crate::errors::{self, Error};
use crate::marshalling::Marshalled;
use crate::promise::{MaybePromise, Promise};

/// `RpcRequestResult`.
#[derive(Clone, Debug)]
pub enum RpcRequestResult {
    Result(Marshalled),
    /// O erro de uma requisição; o `value` do original é qualquer valor, aqui o [`Error`] que o lado remoto lançou.
    Error(Error),
}

/// `IChannel`.
pub trait IChannel {
    fn send_notification(&self, data: Marshalled);
    fn send_request(&self, data: Marshalled) -> Promise<RpcRequestResult>;
}

/// `IChannelHandler`.
pub trait IChannelHandler {
    fn handle_notification(&self, notification_data: &Marshalled);
    fn handle_request(&self, request_data: &Marshalled) -> MaybePromise<RpcRequestResult>;
}

/// `ChannelFactory`.
pub type ChannelFactory = Box<dyn FnOnce(Rc<dyn IChannelHandler>) -> Rc<dyn IChannel>>;

type NotificationFn = Box<dyn Fn(Vec<Marshalled>)>;
type RequestFn = Box<dyn Fn(Vec<Marshalled>) -> MaybePromise<Marshalled>>;

/// `Side`: as notificações e as requisições que um lado atende.
#[derive(Default)]
pub struct Side {
    pub notifications: HashMap<String, NotificationFn>,
    pub requests: HashMap<String, RequestFn>,
}

/// `[method, args]`.
fn outgoing_message(method: &str, args: Vec<Marshalled>) -> Marshalled {
    Marshalled::Array(vec![Marshalled::String(method.into()), Marshalled::Array(args)])
}

fn method_and_args(data: &Marshalled) -> (String, Vec<Marshalled>) {
    match data {
        Marshalled::Array(items) => {
            let method = match items.first() {
                Some(Marshalled::String(s)) => s.to_string_lossy(),
                _ => String::new(),
            };
            let args = match items.get(1) {
                Some(Marshalled::Array(args)) => args.clone(),
                _ => Vec::new(),
            };
            (method, args)
        }
        _ => (String::new(), Vec::new()),
    }
}

struct Handler {
    get_handler: Box<dyn Fn() -> Rc<Side>>,
}

impl IChannelHandler for Handler {
    fn handle_notification(&self, notification_data: &Marshalled) {
        let (method, args) = method_and_args(notification_data);
        let side = (self.get_handler)();
        match side.notifications.get(&method) {
            Some(f) => f(args),
            None => errors::throw(Error::new(format!("Unknown notification \"{method}\"!"))),
        }
    }

    fn handle_request(&self, request_data: &Marshalled) -> MaybePromise<RpcRequestResult> {
        let (method, args) = method_and_args(request_data);
        let side = (self.get_handler)();
        let outcome = errors::catch(|| match side.requests.get(&method) {
            Some(f) => f(args),
            None => errors::throw(Error::type_error(format!("this._getHandler().requests.{method} is not a function"))),
        });
        match outcome {
            Ok(MaybePromise::Value(value)) => MaybePromise::Value(RpcRequestResult::Result(value)),
            Ok(MaybePromise::Promise(promise)) => MaybePromise::Promise(promise.then_result(|r| Ok(match r {
                Ok(value) => RpcRequestResult::Result(value),
                Err(e) => RpcRequestResult::Error(e),
            }))),
            Err(e) => MaybePromise::Value(RpcRequestResult::Error(e)),
        }
    }
}

/// `api.requests`.
pub struct RpcRequests {
    channel: Rc<dyn IChannel>,
}

impl RpcRequests {
    /// `await api.requests[name](...args)`: manda a requisição e devolve o resultado, ou rejeita com o erro.
    pub fn call(&self, name: &str, args: Vec<Marshalled>) -> Promise<Marshalled> {
        // Answering `then` makes this proxy a thenable, so `await` would send a bogus request and never settle.
        if name == "then" {
            return Promise::rejected(Error::type_error("api.requests.then is not a function"));
        }
        self.channel.send_request(outgoing_message(name, args)).then_result(|result| match result {
            Ok(RpcRequestResult::Error(e)) | Err(e) => Err(e),
            Ok(RpcRequestResult::Result(value)) => Ok(value),
        })
    }
}

/// `api.notifications`.
pub struct RpcNotifications {
    channel: Rc<dyn IChannel>,
}

impl RpcNotifications {
    /// `api.notifications[name](...args)`.
    pub fn send(&self, name: &str, args: Vec<Marshalled>) {
        // Answering `then` makes this proxy a thenable, so `await` would send a bogus notification and never settle.
        if name == "then" {
            return;
        }
        self.channel.send_notification(outgoing_message(name, args));
    }
}

/// `api`: `{ notifications, requests }`.
pub struct RpcApi {
    pub notifications: RpcNotifications,
    pub requests: RpcRequests,
}

/// `SimpleTypedRpcConnection`.
pub struct SimpleTypedRpcConnection {
    pub api: RpcApi,
}

impl SimpleTypedRpcConnection {
    /// `createHost`: o lado que atende o `host` e chama o `client`.
    pub fn create_host(channel_factory: ChannelFactory, get_handler: impl Fn() -> Rc<Side> + 'static) -> Self {
        Self::new(channel_factory, Box::new(get_handler))
    }

    /// `createClient`: o lado que atende o `client` e chama o `host`.
    pub fn create_client(channel_factory: ChannelFactory, get_handler: impl Fn() -> Rc<Side> + 'static) -> Self {
        Self::new(channel_factory, Box::new(get_handler))
    }

    fn new(channel_factory: ChannelFactory, get_handler: Box<dyn Fn() -> Rc<Side>>) -> Self {
        let channel = channel_factory(Rc::new(Handler { get_handler }));
        Self { api: RpcApi { notifications: RpcNotifications { channel: channel.clone() }, requests: RpcRequests { channel } } }
    }
}
