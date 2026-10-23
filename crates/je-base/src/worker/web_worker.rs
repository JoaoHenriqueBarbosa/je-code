//! O protocolo, o cliente e o servidor de `webWorker.ts`. Ver o topo de [`crate::worker`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use crate::errors::{self, Error};
use crate::event::{Emitter, EmitterOptions, Event};
use crate::lifecycle::{DisposableRef, IDisposable};
use crate::marshalling::Marshalled;
use crate::parts::ipc::{BufferReader, BufferWriter, deserialize, serialize};
use crate::buffer::VSBuffer;
use crate::promise::Promise;
use crate::strings::is_upper_ascii_letter;

pub const DEFAULT_CHANNEL: &str = "default";
pub const INITIALIZE: &str = "$initialize";

/// `Message`: a união de mensagens do protocolo (o campo `type` do original é a variante).
#[derive(Clone, Debug)]
pub enum Message {
    Request { vs_worker: i64, req: String, channel: String, method: String, args: Vec<Marshalled> },
    Reply { vs_worker: i64, seq: String, res: Marshalled, err: Marshalled },
    SubscribeEvent { vs_worker: i64, req: String, channel: String, event_name: String, arg: Marshalled },
    Event { vs_worker: i64, req: String, event: Marshalled },
    UnsubscribeEvent { vs_worker: i64, req: String },
}

/// `MessageType`.
mod message_type {
    pub const REQUEST: f64 = 0.0;
    pub const REPLY: f64 = 1.0;
    pub const SUBSCRIBE_EVENT: f64 = 2.0;
    pub const EVENT: f64 = 3.0;
    pub const UNSUBSCRIBE_EVENT: f64 = 4.0;
}

fn object(entries: Vec<(&str, Marshalled)>) -> Marshalled {
    Marshalled::Object(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

fn string(s: &str) -> Marshalled {
    Marshalled::String(s.into())
}

fn number(n: i64) -> Marshalled {
    Marshalled::Number(n as f64)
}

fn field<'a>(value: &'a Marshalled, name: &str) -> &'a Marshalled {
    value.key(name).unwrap_or(&Marshalled::Undefined)
}

fn text(value: &Marshalled) -> String {
    match value {
        Marshalled::String(s) => s.to_string_lossy(),
        _ => String::new(),
    }
}

fn integer(value: &Marshalled) -> i64 {
    match value {
        Marshalled::Number(n) => *n as i64,
        _ => 0,
    }
}

impl Message {
    pub fn vs_worker(&self) -> i64 {
        match self {
            Message::Request { vs_worker, .. } | Message::Reply { vs_worker, .. } | Message::SubscribeEvent { vs_worker, .. } | Message::Event { vs_worker, .. } | Message::UnsubscribeEvent { vs_worker, .. } => *vs_worker,
        }
    }

    /// A mensagem como objeto, com os campos do original (`type`, `vsWorker`, ...).
    pub fn to_marshalled(&self) -> Marshalled {
        match self {
            Message::Request { vs_worker, req, channel, method, args } => object(vec![
                ("type", Marshalled::Number(message_type::REQUEST)),
                ("vsWorker", number(*vs_worker)),
                ("req", string(req)),
                ("channel", string(channel)),
                ("method", string(method)),
                ("args", Marshalled::Array(args.clone())),
            ]),
            Message::Reply { vs_worker, seq, res, err } => object(vec![
                ("type", Marshalled::Number(message_type::REPLY)),
                ("vsWorker", number(*vs_worker)),
                ("seq", string(seq)),
                ("res", res.clone()),
                ("err", err.clone()),
            ]),
            Message::SubscribeEvent { vs_worker, req, channel, event_name, arg } => object(vec![
                ("type", Marshalled::Number(message_type::SUBSCRIBE_EVENT)),
                ("vsWorker", number(*vs_worker)),
                ("req", string(req)),
                ("channel", string(channel)),
                ("eventName", string(event_name)),
                ("arg", arg.clone()),
            ]),
            Message::Event { vs_worker, req, event } => object(vec![
                ("type", Marshalled::Number(message_type::EVENT)),
                ("vsWorker", number(*vs_worker)),
                ("req", string(req)),
                ("event", event.clone()),
            ]),
            Message::UnsubscribeEvent { vs_worker, req } => object(vec![
                ("type", Marshalled::Number(message_type::UNSUBSCRIBE_EVENT)),
                ("vsWorker", number(*vs_worker)),
                ("req", string(req)),
            ]),
        }
    }

    /// Lê um objeto como mensagem; `None` se não tem a forma de uma (o `handleMessage` do original ignora o que
    /// não tem `vsWorker`).
    pub fn from_marshalled(value: &Marshalled) -> Option<Message> {
        let vs_worker = integer(field(value, "vsWorker"));
        let t = match field(value, "type") {
            Marshalled::Number(n) => *n,
            _ => return None,
        };
        Some(match t {
            message_type::REQUEST => Message::Request {
                vs_worker,
                req: text(field(value, "req")),
                channel: text(field(value, "channel")),
                method: text(field(value, "method")),
                args: match field(value, "args") {
                    Marshalled::Array(args) => args.clone(),
                    _ => Vec::new(),
                },
            },
            message_type::REPLY => Message::Reply { vs_worker, seq: text(field(value, "seq")), res: field(value, "res").clone(), err: field(value, "err").clone() },
            message_type::SUBSCRIBE_EVENT => Message::SubscribeEvent {
                vs_worker,
                req: text(field(value, "req")),
                channel: text(field(value, "channel")),
                event_name: text(field(value, "eventName")),
                arg: field(value, "arg").clone(),
            },
            message_type::EVENT => Message::Event { vs_worker, req: text(field(value, "req")), event: field(value, "event").clone() },
            message_type::UNSUBSCRIBE_EVENT => Message::UnsubscribeEvent { vs_worker, req: text(field(value, "req")) },
            _ => return None,
        })
    }

    /// A mensagem como bytes, na codificação do IPC (`serialize`): é o que atravessa uma thread.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BufferWriter::new();
        serialize(&mut writer, &self.to_marshalled());
        writer.buffer().to_vec()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Message> {
        // Os objetos da mensagem voltam do JSON como `Marshalled` plano, sem reviver URI nem RegExp.
        let value = deserialize(&mut BufferReader::new(VSBuffer::from_byte_array(bytes)));
        Message::from_marshalled(&value)
    }
}

/// `SerializedError` / `transformErrorForSerialization`: o erro como dado, para atravessar o canal.
pub fn serialized_error(error: &Error) -> Marshalled {
    object(vec![
        ("$isError", Marshalled::Bool(true)),
        ("name", string(&error.name)),
        ("message", string(&error.message)),
        ("stack", error.stack.as_deref().map_or(Marshalled::Undefined, string)),
    ])
}

/// O ramo `$isError` do `_handleReplyMessage`: reconstrói o `Error`.
pub fn error_of_serialized(value: &Marshalled) -> Error {
    let mut error = Error::named(text(field(value, "name")), text(field(value, "message")));
    error.stack = match field(value, "stack") {
        Marshalled::String(s) => Some(s.to_string_lossy()),
        _ => None,
    };
    error
}

/// `!replyMessage.err`.
fn is_falsy(value: &Marshalled) -> bool {
    match value {
        Marshalled::Undefined | Marshalled::Null => true,
        Marshalled::Bool(b) => !b,
        Marshalled::Number(n) => *n == 0.0 || n.is_nan(),
        Marshalled::String(s) => s.is_empty(),
        _ => false,
    }
}

/// Assume a property is an event if it has a form of "onSomething".
pub fn property_is_event(name: &str) -> bool {
    let units: Vec<u16> = name.encode_utf16().collect();
    units.first() == Some(&u16::from(b'o')) && units.get(1) == Some(&u16::from(b'n')) && units.get(2).is_some_and(|&u| is_upper_ascii_letter(u))
}

/// Assume a property is a dynamic event (a method that returns an event) if it has a form of "onDynamicSomething".
pub fn property_is_dynamic_event(name: &str) -> bool {
    let units: Vec<u16> = name.encode_utf16().collect();
    name.starts_with("onDynamic") && units.get(9).is_some_and(|&u| is_upper_ascii_letter(u))
}

/// `IMessageHandler`.
pub trait IMessageHandler {
    fn send_message(&self, msg: &Message);
    fn handle_message(&self, channel: &str, method: &str, args: Vec<Marshalled>) -> Promise<Marshalled>;
    fn handle_event(&self, channel: &str, event_name: &str, arg: Marshalled) -> Result<Event<Marshalled>, Error>;
}

/// O objeto que atende um canal (o `requestHandler` do servidor ou um canal local): o `handler[name]` do
/// original, por nome.
pub trait IWorkerChannelHandler {
    /// `handler[method].apply(handler, args)`; `None` se não há o método.
    fn call(&self, method: &str, args: Vec<Marshalled>) -> Option<Promise<Marshalled>>;
    /// `handler[eventName]` (um evento `onSomething`); `None` se não há.
    fn event(&self, _event_name: &str) -> Option<Event<Marshalled>> {
        None
    }
    /// `handler[eventName].call(handler, arg)` (um evento dinâmico `onDynamicSomething`); `None` se não há.
    fn dynamic_event(&self, _event_name: &str, _arg: Marshalled) -> Option<Event<Marshalled>> {
        None
    }
}

/// `WebWorkerProtocol`.
pub struct WebWorkerProtocol {
    me: Weak<WebWorkerProtocol>,
    worker_id: Cell<i64>,
    last_sent_req: Cell<u64>,
    pending_replies: RefCell<HashMap<String, crate::promise::Resolver<Marshalled>>>,
    pending_emitters: RefCell<HashMap<String, Rc<Emitter<Marshalled>>>>,
    pending_events: RefCell<HashMap<String, DisposableRef>>,
    handler: Rc<dyn IMessageHandler>,
}

impl WebWorkerProtocol {
    pub fn new(handler: Rc<dyn IMessageHandler>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            worker_id: Cell::new(-1),
            last_sent_req: Cell::new(0),
            pending_replies: RefCell::default(),
            pending_emitters: RefCell::default(),
            pending_events: RefCell::default(),
            handler,
        })
    }

    pub fn set_worker_id(&self, worker_id: i64) {
        self.worker_id.set(worker_id);
    }

    fn next_req(&self) -> String {
        let req = self.last_sent_req.get() + 1;
        self.last_sent_req.set(req);
        req.to_string()
    }

    pub fn send_message(&self, channel: &str, method: &str, args: Vec<Marshalled>) -> Promise<Marshalled> {
        let req = self.next_req();
        let (promise, resolver) = Promise::new();
        self.pending_replies.borrow_mut().insert(req.clone(), resolver);
        self.send(&Message::Request { vs_worker: self.worker_id.get(), req, channel: channel.to_owned(), method: method.to_owned(), args });
        promise
    }

    pub fn listen(&self, channel: &str, event_name: &str, arg: Marshalled) -> Event<Marshalled> {
        let req: Rc<RefCell<Option<String>>> = Rc::default();
        let slot: Rc<RefCell<Weak<Emitter<Marshalled>>>> = Rc::default();
        let (channel, event_name) = (channel.to_owned(), event_name.to_owned());
        let emitter = Emitter::with_options(EmitterOptions {
            on_will_add_first_listener: Some(Rc::new({
                let (me, req, slot, arg) = (self.me.clone(), req.clone(), slot.clone(), arg);
                move |_: &Emitter<Marshalled>| {
                    let Some(protocol) = me.upgrade() else { return };
                    let id = protocol.next_req();
                    *req.borrow_mut() = Some(id.clone());
                    if let Some(emitter) = slot.borrow().upgrade() {
                        protocol.pending_emitters.borrow_mut().insert(id.clone(), emitter);
                    }
                    protocol.send(&Message::SubscribeEvent { vs_worker: protocol.worker_id.get(), req: id, channel: channel.clone(), event_name: event_name.clone(), arg: arg.clone() });
                }
            })),
            on_did_remove_last_listener: Some(Rc::new({
                let (me, req) = (self.me.clone(), req.clone());
                move |_: &Emitter<Marshalled>| {
                    let Some(protocol) = me.upgrade() else { return };
                    let id = req.borrow_mut().take();
                    if let Some(id) = id {
                        protocol.pending_emitters.borrow_mut().remove(&id);
                        protocol.send(&Message::UnsubscribeEvent { vs_worker: protocol.worker_id.get(), req: id });
                    }
                }
            })),
            ..EmitterOptions::default()
        });
        *slot.borrow_mut() = Rc::downgrade(&emitter);
        Event::new(move |listener| emitter.event().listen_rc(listener))
    }

    pub fn handle_message(&self, message: &Message) {
        if message.vs_worker() == 0 {
            return;
        }
        if self.worker_id.get() != -1 && message.vs_worker() != self.worker_id.get() {
            return;
        }
        self.handle_message_inner(message);
    }

    /// `createProxyToRemoteChannel`.
    pub fn create_proxy_to_remote_channel(&self, channel: &str, send_message_barrier: Option<Rc<dyn Fn() -> Promise<()>>>) -> Rc<RemoteChannel> {
        Rc::new(RemoteChannel { protocol: self.me.upgrade().expect("o protocolo está vivo"), channel: channel.to_owned(), barrier: send_message_barrier, events: RefCell::default() })
    }

    fn handle_message_inner(&self, msg: &Message) {
        match msg {
            Message::Reply { seq, res, err, .. } => self.handle_reply_message(seq, res, err),
            Message::Request { req, channel, method, args, .. } => self.handle_request_message(req, channel, method, args),
            Message::SubscribeEvent { req, channel, event_name, arg, .. } => self.handle_subscribe_event_message(req, channel, event_name, arg),
            Message::Event { req, event, .. } => self.handle_event_message(req, event),
            Message::UnsubscribeEvent { req, .. } => self.handle_unsubscribe_event_message(req),
        }
    }

    fn handle_reply_message(&self, seq: &str, res: &Marshalled, err: &Marshalled) {
        let Some(reply) = self.pending_replies.borrow_mut().remove(seq) else {
            eprintln!("Got reply to unknown seq");
            return;
        };

        if !is_falsy(err) {
            let error = match field(err, "$isError") {
                Marshalled::Bool(true) => error_of_serialized(err),
                _ => crate::parts::ipc::error_obj(err),
            };
            reply.reject(error);
            return;
        }

        reply.resolve(res.clone());
    }

    fn handle_request_message(&self, req: &str, channel: &str, method: &str, args: &[Marshalled]) {
        let result = self.handler.handle_message(channel, method, args.to_vec());
        let (ok, fail) = (self.me.clone(), self.me.clone());
        let (req_ok, req_err) = (req.to_owned(), req.to_owned());
        result.then_result(move |outcome| {
            match outcome {
                Ok(r) => {
                    if let Some(p) = ok.upgrade() {
                        p.send(&Message::Reply { vs_worker: p.worker_id.get(), seq: req_ok, res: r, err: Marshalled::Undefined });
                    }
                }
                Err(e) => {
                    if let Some(p) = fail.upgrade() {
                        p.send(&Message::Reply { vs_worker: p.worker_id.get(), seq: req_err, res: Marshalled::Undefined, err: serialized_error(&e) });
                    }
                }
            }
            Ok(())
        });
    }

    fn handle_subscribe_event_message(&self, req: &str, channel: &str, event_name: &str, arg: &Marshalled) {
        let event = match self.handler.handle_event(channel, event_name, arg.clone()) {
            Ok(event) => event,
            Err(error) => errors::throw(error),
        };
        let (me, id) = (self.me.clone(), req.to_owned());
        let sent_id = id.clone();
        let disposable = event.listen(move |event| {
            if let Some(p) = me.upgrade() {
                p.send(&Message::Event { vs_worker: p.worker_id.get(), req: sent_id.clone(), event: event.clone() });
            }
        });
        self.pending_events.borrow_mut().insert(id, disposable);
    }

    fn handle_event_message(&self, req: &str, event: &Marshalled) {
        let emitter = self.pending_emitters.borrow().get(req).cloned();
        match emitter {
            Some(emitter) => emitter.fire(event.clone()),
            None => eprintln!("Got event for unknown req"),
        }
    }

    fn handle_unsubscribe_event_message(&self, req: &str) {
        let event = self.pending_events.borrow_mut().remove(req);
        match event {
            Some(event) => event.dispose(),
            None => eprintln!("Got unsubscribe for unknown req"),
        }
    }

    fn send(&self, msg: &Message) {
        self.handler.send_message(msg);
    }
}

/// O proxy de um canal remoto (`createProxyToRemoteChannel`): os métodos `$nome`, os eventos `onNome` e os
/// eventos dinâmicos `onDynamicNome`.
pub struct RemoteChannel {
    protocol: Rc<WebWorkerProtocol>,
    channel: String,
    barrier: Option<Rc<dyn Fn() -> Promise<()>>>,
    events: RefCell<HashMap<String, Event<Marshalled>>>,
}

impl RemoteChannel {
    /// `proxy.$method(...args)`: espera a barreira (o carregamento do worker), manda a requisição e devolve a
    /// resposta. O nome precisa começar com `$`; qualquer outro nome não existe no proxy.
    pub fn call(&self, method: &str, args: Vec<Marshalled>) -> Promise<Marshalled> {
        if !method.starts_with('$') {
            return Promise::rejected(Error::new(format!("{method} is not a function")));
        }
        let (protocol, channel, method) = (self.protocol.clone(), self.channel.clone(), method.to_owned());
        match &self.barrier {
            Some(barrier) => barrier().then_promise(move |()| protocol.send_message(&channel, &method, args)),
            None => protocol.send_message(&channel, &method, args),
        }
    }

    /// `proxy.onSomething`: o mesmo evento a cada leitura (o original guarda no alvo do proxy).
    pub fn event(&self, name: &str) -> Event<Marshalled> {
        self.events.borrow_mut().entry(name.to_owned()).or_insert_with(|| self.protocol.listen(&self.channel, name, Marshalled::Undefined)).clone()
    }

    /// `proxy.onDynamicSomething(arg)`.
    pub fn dynamic_event(&self, name: &str, arg: Marshalled) -> Event<Marshalled> {
        self.protocol.listen(&self.channel, name, arg)
    }
}

/// `IWebWorker`: o transporte de mensagens de um worker.
pub trait IWebWorker: IDisposable {
    fn get_id(&self) -> i64;
    fn on_message(&self) -> Event<Message>;
    fn on_error(&self) -> Event<Error>;
    fn post_message(&self, message: &Message);
}

fn handle_local_message(channels: &RefCell<HashMap<String, Rc<dyn IWorkerChannelHandler>>>, channel_name: &str, method: &str, args: Vec<Marshalled>, side: &str) -> Promise<Marshalled> {
    let Some(channel) = channels.borrow().get(channel_name).cloned() else {
        return Promise::rejected(Error::new(format!("Missing channel {channel_name} on {side}")));
    };
    match errors::catch(|| channel.call(method, args)) {
        Ok(Some(promise)) => promise,
        Ok(None) => Promise::rejected(Error::new(format!("Missing method {method} on {side} channel {channel_name}"))),
        Err(e) => Promise::rejected(e),
    }
}

fn handle_local_event(channel: Option<Rc<dyn IWorkerChannelHandler>>, channel_name: &str, event_name: &str, arg: Marshalled, side: &str) -> Result<Event<Marshalled>, Error> {
    let Some(channel) = channel else {
        return Err(Error::new(format!("Missing channel {channel_name} on {side}")));
    };
    if property_is_dynamic_event(event_name) {
        return channel.dynamic_event(event_name, arg).ok_or_else(|| Error::new(format!("Missing dynamic event {event_name} on {side} channel {channel_name}.")));
    }
    if property_is_event(event_name) {
        return channel.event(event_name).ok_or_else(|| Error::new(format!("Missing event {event_name} on {side} channel {channel_name}.")));
    }
    Err(Error::new(format!("Malformed event name {event_name}")))
}

/// `WebWorkerClient`: o lado da thread principal.
pub struct WebWorkerClient {
    worker: Rc<dyn IWebWorker>,
    on_module_loaded: Promise<()>,
    protocol: Rc<WebWorkerProtocol>,
    /// `proxy`: o canal `default` do worker.
    pub proxy: Rc<RemoteChannel>,
    local_channels: Rc<RefCell<HashMap<String, Rc<dyn IWorkerChannelHandler>>>>,
    remote_channels: RefCell<HashMap<String, Rc<RemoteChannel>>>,
    listeners: RefCell<Vec<DisposableRef>>,
}

struct ClientHandler {
    worker: Rc<dyn IWebWorker>,
    local_channels: Rc<RefCell<HashMap<String, Rc<dyn IWorkerChannelHandler>>>>,
}

impl IMessageHandler for ClientHandler {
    fn send_message(&self, msg: &Message) {
        self.worker.post_message(msg);
    }

    fn handle_message(&self, channel: &str, method: &str, args: Vec<Marshalled>) -> Promise<Marshalled> {
        handle_local_message(&self.local_channels, channel, method, args, "main thread")
    }

    fn handle_event(&self, channel: &str, event_name: &str, arg: Marshalled) -> Result<Event<Marshalled>, Error> {
        let local = self.local_channels.borrow().get(channel).cloned();
        handle_local_event(local, channel, event_name, arg, "main thread")
    }
}

impl WebWorkerClient {
    pub fn new(worker: Rc<dyn IWebWorker>) -> Rc<Self> {
        let local_channels: Rc<RefCell<HashMap<String, Rc<dyn IWorkerChannelHandler>>>> = Rc::default();
        let protocol = WebWorkerProtocol::new(Rc::new(ClientHandler { worker: worker.clone(), local_channels: local_channels.clone() }));
        protocol.set_worker_id(worker.get_id());

        let mut listeners: Vec<DisposableRef> = Vec::new();
        let target = Rc::downgrade(&protocol);
        listeners.push(worker.on_message().listen(move |msg| {
            if let Some(protocol) = target.upgrade() {
                protocol.handle_message(msg);
            }
        }));
        listeners.push(worker.on_error().listen(errors::on_unexpected_error));

        // Send initialize message
        let on_module_loaded = protocol.send_message(DEFAULT_CHANNEL, INITIALIZE, vec![number(worker.get_id())]).then(|_| ());

        let barrier: Rc<dyn Fn() -> Promise<()>> = {
            let loaded = on_module_loaded.clone();
            Rc::new(move || loaded.clone())
        };
        let proxy = protocol.create_proxy_to_remote_channel(DEFAULT_CHANNEL, Some(barrier));
        on_module_loaded.catch(|e| {
            eprintln!("Worker failed to load ");
            eprintln!("{e}");
        });

        Rc::new(Self { worker, on_module_loaded, protocol, proxy, local_channels, remote_channels: RefCell::default(), listeners: RefCell::new(listeners) })
    }

    /// `setChannel`.
    pub fn set_channel(&self, channel: &str, handler: Rc<dyn IWorkerChannelHandler>) {
        self.local_channels.borrow_mut().insert(channel.to_owned(), handler);
    }

    /// `getChannel`.
    pub fn get_channel(&self, channel: &str) -> Rc<RemoteChannel> {
        if let Some(existing) = self.remote_channels.borrow().get(channel) {
            return existing.clone();
        }
        let loaded = self.on_module_loaded.clone();
        let barrier: Rc<dyn Fn() -> Promise<()>> = Rc::new(move || loaded.clone());
        let inst = self.protocol.create_proxy_to_remote_channel(channel, Some(barrier));
        self.remote_channels.borrow_mut().insert(channel.to_owned(), inst.clone());
        inst
    }

    /// A promessa de o worker ter respondido ao `$initialize`.
    pub fn on_module_loaded(&self) -> Promise<()> {
        self.on_module_loaded.clone()
    }
}

impl IDisposable for WebWorkerClient {
    fn dispose(&self) {
        for listener in self.listeners.borrow_mut().drain(..) {
            listener.dispose();
        }
        self.worker.dispose();
    }
}

/// `IWebWorkerServer`.
pub trait IWebWorkerServer {
    fn set_channel(&self, channel: &str, handler: Rc<dyn IWorkerChannelHandler>);
    fn get_channel(&self, channel: &str) -> Rc<RemoteChannel>;
}

/// `WebWorkerServer`: o lado do worker.
pub struct WebWorkerServer {
    pub request_handler: RefCell<Option<Rc<dyn IWorkerChannelHandler>>>,
    protocol: Rc<WebWorkerProtocol>,
    local_channels: Rc<RefCell<HashMap<String, Rc<dyn IWorkerChannelHandler>>>>,
    remote_channels: RefCell<HashMap<String, Rc<RemoteChannel>>>,
}

struct ServerHandler {
    server: RefCell<Weak<WebWorkerServer>>,
    post_message: Rc<dyn Fn(&Message)>,
}

impl ServerHandler {
    fn channel(&self, name: &str) -> Option<Rc<dyn IWorkerChannelHandler>> {
        let server = self.server.borrow().upgrade()?;
        if name == DEFAULT_CHANNEL { server.request_handler.borrow().clone() } else { server.local_channels.borrow().get(name).cloned() }
    }
}

impl IMessageHandler for ServerHandler {
    fn send_message(&self, msg: &Message) {
        (self.post_message)(msg);
    }

    fn handle_message(&self, channel: &str, method: &str, args: Vec<Marshalled>) -> Promise<Marshalled> {
        if channel == DEFAULT_CHANNEL && method == INITIALIZE {
            if let Some(server) = self.server.borrow().upgrade() {
                server.protocol.set_worker_id(args.first().map_or(0, integer));
            }
            return Promise::resolved(Marshalled::Undefined);
        }
        let Some(handler) = self.channel(channel) else {
            return Promise::rejected(Error::new(format!("Missing channel {channel} on worker thread")));
        };
        match errors::catch(|| handler.call(method, args)) {
            Ok(Some(promise)) => promise,
            Ok(None) => Promise::rejected(Error::new(format!("Missing method {method} on worker thread channel {channel}"))),
            Err(e) => Promise::rejected(e),
        }
    }

    fn handle_event(&self, channel: &str, event_name: &str, arg: Marshalled) -> Result<Event<Marshalled>, Error> {
        handle_local_event(self.channel(channel), channel, event_name, arg, "worker thread")
    }
}

impl WebWorkerServer {
    /// `new WebWorkerServer(postMessage, requestHandlerFactory)`.
    pub fn new(post_message: impl Fn(&Message) + 'static, request_handler_factory: impl FnOnce(&Rc<WebWorkerServer>) -> Rc<dyn IWorkerChannelHandler>) -> Rc<Self> {
        let handler = Rc::new(ServerHandler { server: RefCell::default(), post_message: Rc::new(post_message) });
        let protocol = WebWorkerProtocol::new(handler.clone());
        let server = Rc::new(Self { request_handler: RefCell::default(), protocol, local_channels: Rc::default(), remote_channels: RefCell::default() });
        *handler.server.borrow_mut() = Rc::downgrade(&server);
        let request_handler = request_handler_factory(&server);
        *server.request_handler.borrow_mut() = Some(request_handler);
        server
    }

    /// `onmessage`.
    pub fn onmessage(&self, msg: &Message) {
        self.protocol.handle_message(msg);
    }
}

impl IWebWorkerServer for WebWorkerServer {
    fn set_channel(&self, channel: &str, handler: Rc<dyn IWorkerChannelHandler>) {
        self.local_channels.borrow_mut().insert(channel.to_owned(), handler);
    }

    fn get_channel(&self, channel: &str) -> Rc<RemoteChannel> {
        if let Some(existing) = self.remote_channels.borrow().get(channel) {
            return existing.clone();
        }
        let inst = self.protocol.create_proxy_to_remote_channel(channel, None);
        self.remote_channels.borrow_mut().insert(channel.to_owned(), inst.clone());
        inst
    }
}
