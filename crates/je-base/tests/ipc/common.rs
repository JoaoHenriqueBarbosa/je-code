//! Os auxiliares do `ipc.test.ts` e dos testes de `ipc.net`: o `QueueProtocol`, o par de protocolos, o
//! `TestIPCClient`, o `TestIPCServer`, o `TestService` e seus canais.
#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::{Emitter, EmitterOptions, Event};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::marshalling::Marshalled;
use je_base::parts::ipc::proxy_channel::IProxyService;
use je_base::parts::ipc::{ClientConnectionEvent, IChannel, IMessagePassingProtocol, IPCClient, IPCServer, IServerChannel};
use je_base::promise::Promise;

pub fn string(s: &str) -> Marshalled {
    Marshalled::String(s.into())
}

pub fn as_string(value: &Marshalled) -> String {
    match value {
        Marshalled::String(s) => s.to_string_lossy(),
        other => panic!("esperava uma string, veio {other:?}"),
    }
}

/// `QueueProtocol`: guarda as mensagens enquanto ninguém escuta.
pub struct QueueProtocol {
    buffering: Cell<bool>,
    buffers: RefCell<Vec<VSBuffer>>,
    on_message: Rc<Emitter<VSBuffer>>,
    other: RefCell<Option<Rc<QueueProtocol>>>,
}

impl QueueProtocol {
    fn new() -> Rc<Self> {
        Rc::new_cyclic(|me: &std::rc::Weak<QueueProtocol>| {
            let (first, last) = (me.clone(), me.clone());
            QueueProtocol {
                buffering: Cell::new(true),
                buffers: RefCell::default(),
                on_message: Emitter::with_options(EmitterOptions {
                    on_did_add_first_listener: Some(Rc::new(move |emitter: &Emitter<VSBuffer>| {
                        if let Some(protocol) = first.upgrade() {
                            let buffers = std::mem::take(&mut *protocol.buffers.borrow_mut());
                            for buffer in buffers {
                                emitter.fire(buffer);
                            }
                            protocol.buffering.set(false);
                        }
                    })),
                    on_did_remove_last_listener: Some(Rc::new(move |_: &Emitter<VSBuffer>| {
                        if let Some(protocol) = last.upgrade() {
                            protocol.buffering.set(true);
                        }
                    })),
                    ..EmitterOptions::default()
                }),
                other: RefCell::default(),
            }
        })
    }

    fn receive(&self, buffer: VSBuffer) {
        if self.buffering.get() {
            self.buffers.borrow_mut().push(buffer);
        } else {
            self.on_message.fire(buffer);
        }
    }
}

impl IMessagePassingProtocol for QueueProtocol {
    fn send(&self, buffer: VSBuffer) {
        let other = self.other.borrow().clone().expect("o par está ligado");
        other.receive(buffer);
    }

    fn on_message(&self) -> Event<VSBuffer> {
        self.on_message.event()
    }
}

/// `createProtocolPair`.
pub fn create_protocol_pair() -> (Rc<QueueProtocol>, Rc<QueueProtocol>) {
    let one = QueueProtocol::new();
    let other = QueueProtocol::new();
    *one.other.borrow_mut() = Some(other.clone());
    *other.other.borrow_mut() = Some(one.clone());
    (one, other)
}

/// `TestIPCClient`: um `IPCClient` que avisa quando é descartado.
pub struct TestIPCClient {
    pub client: IPCClient,
    on_did_disconnect: Rc<Emitter<()>>,
}

impl TestIPCClient {
    fn new(protocol: Rc<dyn IMessagePassingProtocol>, id: &str) -> Rc<Self> {
        Rc::new(Self { client: IPCClient::new(protocol, string(id), None), on_did_disconnect: Emitter::new() })
    }

    pub fn on_did_disconnect(&self) -> Event<()> {
        self.on_did_disconnect.event()
    }

    pub fn get_channel(&self, name: &str) -> Rc<dyn IChannel> {
        self.client.get_channel(name)
    }

    pub fn register_channel(&self, name: &str, channel: Rc<dyn IServerChannel>) {
        self.client.register_channel(name, channel);
    }
}

impl IDisposable for TestIPCClient {
    fn dispose(&self) {
        self.on_did_disconnect.fire(());
        self.client.dispose();
        self.on_did_disconnect.dispose();
    }
}

/// `TestIPCServer`.
pub struct TestIPCServer {
    pub server: Rc<IPCServer>,
    on_did_client_connect: Rc<Emitter<ClientConnectionEvent>>,
}

impl TestIPCServer {
    pub fn new() -> Rc<Self> {
        let on_did_client_connect = Emitter::new();
        let server = IPCServer::new(on_did_client_connect.event(), None, None);
        Rc::new(Self { server, on_did_client_connect })
    }

    pub fn create_connection(&self, id: &str) -> Rc<TestIPCClient> {
        let (pc, ps) = create_protocol_pair();
        let client = TestIPCClient::new(pc, id);
        self.on_did_client_connect.fire(ClientConnectionEvent { protocol: ps, on_did_client_disconnect: client.on_did_disconnect() });
        client
    }
}

impl IDisposable for TestIPCServer {
    fn dispose(&self) {
        self.server.dispose();
        self.on_did_client_connect.dispose();
    }
}

/// `TestService`.
pub struct TestService {
    disposables: Rc<DisposableStore>,
    on_pong: Rc<Emitter<Marshalled>>,
}

impl TestService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { disposables: DisposableStore::new(), on_pong: Emitter::new() })
    }

    pub fn on_pong(&self) -> Event<Marshalled> {
        self.on_pong.event()
    }

    pub fn has_pong_listeners(&self) -> bool {
        self.on_pong.has_listeners()
    }

    pub fn marco(&self) -> Promise<Marshalled> {
        Promise::resolved(string("polo"))
    }

    pub fn error(&self, message: &str) -> Promise<Marshalled> {
        Promise::rejected(Error::new(message))
    }

    pub fn never_complete(&self) -> Promise<Marshalled> {
        Promise::with_executor(|_| {})
    }

    pub fn never_complete_ct(&self, cancellation_token: CancellationToken) -> Promise<Marshalled> {
        if cancellation_token.is_cancellation_requested() {
            return Promise::rejected(Error::cancellation());
        }
        let disposables = self.disposables.clone();
        Promise::with_executor(move |resolver| {
            disposables.add(cancellation_token.on_cancellation_requested().listen(move |_| resolver.reject(Error::cancellation())));
        })
    }

    pub fn buffers_length(&self, buffers: &Marshalled) -> Promise<Marshalled> {
        let Marshalled::Array(items) = buffers else { panic!("esperava um array de buffers") };
        let total: usize = items
            .iter()
            .map(|b| match b {
                Marshalled::Buffer(buffer) => buffer.byte_length(),
                other => panic!("esperava um VSBuffer, veio {other:?}"),
            })
            .sum();
        Promise::resolved(Marshalled::Number(total as f64))
    }

    pub fn ping(&self, msg: &str) {
        self.on_pong.fire(string(msg));
    }

    pub fn marshall(&self, uri: Marshalled) -> Promise<Marshalled> {
        Promise::resolved(uri)
    }

    pub fn context(&self, context: Option<Marshalled>) -> Promise<Marshalled> {
        Promise::resolved(context.unwrap_or(Marshalled::Undefined))
    }
}

impl IDisposable for TestService {
    fn dispose(&self) {
        self.disposables.dispose();
        self.on_pong.dispose();
    }
}

/// `TestChannel`.
pub struct TestChannel {
    pub service: Rc<TestService>,
}

impl IServerChannel for TestChannel {
    fn call(&self, _ctx: &Marshalled, command: &str, arg: Marshalled, cancellation_token: CancellationToken) -> Promise<Marshalled> {
        match command {
            "marco" => self.service.marco(),
            "error" => self.service.error(&as_string(&arg)),
            "neverComplete" => self.service.never_complete(),
            "neverCompleteCT" => self.service.never_complete_ct(cancellation_token),
            "buffersLength" => self.service.buffers_length(&arg),
            _ => Promise::rejected(Error::new("not implemented")),
        }
    }

    fn listen(&self, _ctx: &Marshalled, event: &str, _arg: Marshalled) -> Event<Marshalled> {
        match event {
            "onPong" => self.service.on_pong(),
            _ => je_base::errors::throw(Error::new("not implemented")),
        }
    }
}

/// `TestChannelClient`.
pub struct TestChannelClient {
    pub channel: Rc<dyn IChannel>,
}

impl TestChannelClient {
    pub fn on_pong(&self) -> Event<Marshalled> {
        self.channel.listen("onPong", Marshalled::Undefined)
    }

    pub fn marco(&self) -> Promise<Marshalled> {
        self.channel.call("marco", Marshalled::Undefined, CancellationToken::None)
    }

    pub fn error(&self, message: &str) -> Promise<Marshalled> {
        self.channel.call("error", string(message), CancellationToken::None)
    }

    pub fn never_complete_ct(&self, cancellation_token: CancellationToken) -> Promise<Marshalled> {
        self.channel.call("neverCompleteCT", Marshalled::Undefined, cancellation_token)
    }

    pub fn buffers_length(&self, buffers: Vec<VSBuffer>) -> Promise<Marshalled> {
        self.channel.call("buffersLength", Marshalled::Array(buffers.into_iter().map(Marshalled::Buffer).collect()), CancellationToken::None)
    }
}

/// O `TestService` como serviço de `ProxyChannel`.
pub struct ProxyTestService(pub Rc<TestService>);

impl IProxyService for ProxyTestService {
    fn event_names(&self) -> Vec<String> {
        vec!["onPong".to_owned()]
    }

    fn event(&self, name: &str) -> Option<Event<Marshalled>> {
        (name == "onPong").then(|| self.0.on_pong())
    }

    fn dynamic_event(&self, _name: &str, _arg: Marshalled) -> Option<Event<Marshalled>> {
        None
    }

    fn call(&self, name: &str, args: Vec<Marshalled>) -> Option<Promise<Marshalled>> {
        let first = args.first().cloned();
        match name {
            "marco" => Some(self.0.marco()),
            "error" => Some(self.0.error(&as_string(&first.unwrap_or(Marshalled::Undefined)))),
            "neverComplete" => Some(self.0.never_complete()),
            "buffersLength" => {
                let buffers = first.unwrap_or(Marshalled::Undefined);
                Some(self.0.buffers_length(&buffers))
            }
            "marshall" => Some(self.0.marshall(first.unwrap_or(Marshalled::Undefined))),
            "context" => Some(self.0.context(first)),
            _ => None,
        }
    }
}

pub fn add_disposable(store: &DisposableStore, d: DisposableRef) {
    store.add(d);
}
