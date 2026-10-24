//! Porte de `src/vs/base/parts/ipc/common/ipc.ts`: canais (`IChannel`, `IServerChannel`), o servidor e o
//! cliente de canais sobre um protocolo de mensagens (`ChannelServer`, `ChannelClient`, `IPCServer`,
//! `IPCClient`), os canais atrasados, o roteamento e o `ProxyChannel`.
//!
//! A codificação em bytes é a do original (ver [`protocol`]). Decisões de tradução:
//!
//! - Os valores que atravessam o canal (`any`) são [`Marshalled`], o JSON com `undefined`, `URI`, `RegExp`,
//!   `Date`, `VSBuffer` e `Uint8Array`. `revive` roda sobre eles como no original.
//! - O `TContext` genérico é um [`Marshalled`]: o contexto de um cliente é o primeiro valor que ele envia, lido
//!   pelo `deserialize`, e nada mais estreito que isso é conhecido pelo servidor.
//! - Uma exceção do original é um [`Error`]. O `PromiseErrorObj` (um `reject` com um valor que não é `Error`)
//!   viaja como um [`Error`] com a propriedade [`ERROR_OBJ_PROP`] guardando o JSON do valor: [`error_obj`] cria
//!   esse erro e o servidor o envia como tipo 203.
//! - O `ProxyChannel` usa `Proxy` e acesso dinâmico a propriedades. Aqui o serviço implementa [`IProxyService`]
//!   (métodos e eventos por nome) e o cliente é um [`ServiceProxy`].
//! - O log do `IPCLogger` usa `console.log` com `%c`; aqui escreve texto simples em stdout.
//!
//! Os tamanhos e ids são `u32`/`i32` quando vêm de `number` no original; uma mensagem malformada (cabeçalho que
//! não é um array) é ignorada em vez de lançar `TypeError`.

pub mod mp;
pub mod net;
pub mod node_net;
pub mod protocol;
pub mod web_socket_framing;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;

use crate::arrays::get_random_element;
use crate::async_::{async_fn, create_cancelable_promise, timeout};
use crate::buffer::VSBuffer;
use crate::cancellation::{CancellationToken, CancellationTokenSource};
use crate::errors::{self, Error};
use crate::event::{Emitter, EmitterOptions, Event, EventMultiplexer, Relay};
use crate::lifecycle::{DisposableRef, DisposableStore, IDisposable, plain_disposable, to_disposable};
use crate::marshalling::{Marshalled, json_stringify, revive};
use crate::path::Platform;
use crate::promise::{CancelablePromise, MaybePromise, Promise};
use crate::runtime::{self, TimerId};
use crate::strings::is_upper_ascii_letter;

pub use protocol::{BufferReader, BufferWriter, IReader, IWriter, deserialize, serialize};

/// O nome da propriedade de [`Error`] que guarda o JSON de um valor lançado que não é um erro.
pub const ERROR_OBJ_PROP: &str = "ipcErrorObj";

/// Um erro que o servidor envia como `PromiseErrorObj`: `reject(data)` com um valor qualquer.
pub fn error_obj(data: &Marshalled) -> Error {
    let json = json_stringify(data).map(|j| j.to_string_lossy()).unwrap_or_default();
    Error::new(json.clone()).with_prop(ERROR_OBJ_PROP, json)
}

/// `IChannel`: uma coleção de comandos. Um `call` recebe no máximo um argumento e devolve uma promessa com no
/// máximo um valor.
pub trait IChannel {
    fn call(&self, command: &str, arg: Marshalled, cancellation_token: CancellationToken) -> Promise<Marshalled>;
    fn listen(&self, event: &str, arg: Marshalled) -> Event<Marshalled>;
}

/// `IServerChannel`: o lado do servidor de um [`IChannel`].
pub trait IServerChannel {
    fn call(&self, ctx: &Marshalled, command: &str, arg: Marshalled, cancellation_token: CancellationToken) -> Promise<Marshalled>;
    fn listen(&self, ctx: &Marshalled, event: &str, arg: Marshalled) -> Event<Marshalled>;
}

mod request_type {
    pub const PROMISE: i32 = 100;
    pub const PROMISE_CANCEL: i32 = 101;
    pub const EVENT_LISTEN: i32 = 102;
    pub const EVENT_DISPOSE: i32 = 103;
}

mod response_type {
    pub const INITIALIZE: i32 = 200;
    pub const PROMISE_SUCCESS: i32 = 201;
    pub const PROMISE_ERROR: i32 = 202;
    pub const PROMISE_ERROR_OBJ: i32 = 203;
    pub const EVENT_FIRE: i32 = 204;
}

fn request_type_to_str(type_: i32) -> &'static str {
    match type_ {
        request_type::PROMISE => "req",
        request_type::PROMISE_CANCEL => "cancel",
        request_type::EVENT_LISTEN => "subscribe",
        _ => "unsubscribe",
    }
}

fn response_type_to_str(type_: i32) -> &'static str {
    match type_ {
        response_type::INITIALIZE => "init",
        response_type::PROMISE_SUCCESS => "reply:",
        response_type::PROMISE_ERROR | response_type::PROMISE_ERROR_OBJ => "replyErr:",
        _ => "event:",
    }
}

/// `IRawRequest`.
#[derive(Clone, Debug)]
enum RawRequest {
    Promise { id: u32, channel_name: String, name: String, arg: Marshalled },
    PromiseCancel { id: u32 },
    EventListen { id: u32, channel_name: String, name: String, arg: Marshalled },
    EventDispose { id: u32 },
}

impl RawRequest {
    fn type_(&self) -> i32 {
        match self {
            RawRequest::Promise { .. } => request_type::PROMISE,
            RawRequest::PromiseCancel { .. } => request_type::PROMISE_CANCEL,
            RawRequest::EventListen { .. } => request_type::EVENT_LISTEN,
            RawRequest::EventDispose { .. } => request_type::EVENT_DISPOSE,
        }
    }
}

/// `IRawResponse`.
#[derive(Clone, Debug)]
enum RawResponse {
    Initialize,
    PromiseSuccess { id: u32, data: Marshalled },
    PromiseError { id: u32, data: Marshalled },
    PromiseErrorObj { id: u32, data: Marshalled },
    EventFire { id: u32, data: Marshalled },
}

impl RawResponse {
    fn type_(&self) -> i32 {
        match self {
            RawResponse::Initialize => response_type::INITIALIZE,
            RawResponse::PromiseSuccess { .. } => response_type::PROMISE_SUCCESS,
            RawResponse::PromiseError { .. } => response_type::PROMISE_ERROR,
            RawResponse::PromiseErrorObj { .. } => response_type::PROMISE_ERROR_OBJ,
            RawResponse::EventFire { .. } => response_type::EVENT_FIRE,
        }
    }
}

type Handler = Rc<dyn Fn(&RawResponse)>;

/// `IMessagePassingProtocol`.
pub trait IMessagePassingProtocol {
    fn send(&self, buffer: VSBuffer);
    fn on_message(&self) -> Event<VSBuffer>;
    /// Wait for the write buffer (if applicable) to become empty.
    fn drain(&self) -> Option<Promise<()>> {
        None
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Uninitialized,
    Idle,
}

/// `RequestInitiator`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestInitiator {
    LocalSide = 0,
    OtherSide = 1,
}

/// `IIPCLogger`.
pub trait IIPCLogger {
    fn log_incoming(&self, msg_length: usize, request_id: u32, initiator: RequestInitiator, str: &str, data: Option<&Marshalled>);
    fn log_outgoing(&self, msg_length: usize, request_id: u32, initiator: RequestInitiator, str: &str, data: Option<&Marshalled>);
}

type Logger = Option<Rc<dyn IIPCLogger>>;

fn number(value: &Marshalled) -> Option<f64> {
    match value {
        Marshalled::Number(n) => Some(*n),
        _ => None,
    }
}

fn text(value: &Marshalled) -> Option<String> {
    match value {
        Marshalled::String(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}

fn object(entries: Vec<(&str, Marshalled)>) -> Marshalled {
    Marshalled::Object(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

fn string_value(s: &str) -> Marshalled {
    Marshalled::String(s.into())
}

fn send_buffer(protocol: &dyn IMessagePassingProtocol, message: VSBuffer) -> usize {
    // noop: o envio pode lançar (socket fechado) e o original ignora.
    let length = message.byte_length();
    match errors::catch(|| protocol.send(message)) {
        Ok(()) => length,
        Err(_) => 0,
    }
}

fn send(protocol: &dyn IMessagePassingProtocol, header: Marshalled, body: Marshalled) -> usize {
    let mut writer = BufferWriter::new();
    serialize(&mut writer, &header);
    serialize(&mut writer, &body);
    let length = send_buffer(protocol, writer.buffer());
    writer.dispose();
    length
}

struct PendingRequest {
    request: RawRequest,
    timeout_timer: TimerId,
}

/// `ChannelServer`: hospeda canais registrados por nome.
pub struct ChannelServer {
    me: Weak<ChannelServer>,
    protocol: Rc<dyn IMessagePassingProtocol>,
    ctx: Marshalled,
    logger: Logger,
    timeout_delay: u64,
    channels: RefCell<IndexMap<String, Rc<dyn IServerChannel>>>,
    active_requests: RefCell<IndexMap<u32, DisposableRef>>,
    protocol_listener: RefCell<Option<DisposableRef>>,
    // Requests might come in for channels which are not yet registered. They will timeout after `timeoutDelay`.
    pending_requests: RefCell<IndexMap<String, Vec<PendingRequest>>>,
}

impl ChannelServer {
    pub fn new(protocol: Rc<dyn IMessagePassingProtocol>, ctx: Marshalled, logger: Logger, timeout_delay: Option<u64>) -> Rc<Self> {
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            protocol: protocol.clone(),
            ctx,
            logger,
            timeout_delay: timeout_delay.unwrap_or(1000),
            channels: RefCell::default(),
            active_requests: RefCell::default(),
            protocol_listener: RefCell::default(),
            pending_requests: RefCell::default(),
        });
        let weak = this.me.clone();
        let listener = protocol.on_message().listen(move |msg| {
            if let Some(server) = weak.upgrade() {
                server.on_raw_message(msg);
            }
        });
        *this.protocol_listener.borrow_mut() = Some(listener);
        this.send_response(RawResponse::Initialize);
        this
    }

    pub fn register_channel(&self, channel_name: &str, channel: Rc<dyn IServerChannel>) {
        self.channels.borrow_mut().insert(channel_name.to_owned(), channel);

        // https://github.com/microsoft/vscode/issues/72531
        let weak = self.me.clone();
        let channel_name = channel_name.to_owned();
        runtime::set_timeout(
            move || {
                if let Some(server) = weak.upgrade() {
                    server.flush_pending_requests(&channel_name);
                }
            },
            0,
        );
    }

    fn send_response(&self, response: RawResponse) {
        let type_ = Marshalled::Number(f64::from(response.type_()));
        match &response {
            RawResponse::Initialize => {
                let length = send(&*self.protocol, Marshalled::Array(vec![type_]), Marshalled::Undefined);
                if let Some(logger) = &self.logger {
                    logger.log_outgoing(length, 0, RequestInitiator::OtherSide, response_type_to_str(response.type_()), None);
                }
            }
            RawResponse::PromiseSuccess { id, data } | RawResponse::PromiseError { id, data } | RawResponse::EventFire { id, data } | RawResponse::PromiseErrorObj { id, data } => {
                let length = send(&*self.protocol, Marshalled::Array(vec![type_, Marshalled::Number(f64::from(*id))]), data.clone());
                if let Some(logger) = &self.logger {
                    logger.log_outgoing(length, *id, RequestInitiator::OtherSide, response_type_to_str(response.type_()), Some(data));
                }
            }
        }
    }

    fn on_raw_message(&self, message: &VSBuffer) {
        let mut reader = BufferReader::new(message.clone());
        let header = deserialize(&mut reader);
        let body = deserialize(&mut reader);
        let Marshalled::Array(header) = header else { return };
        let Some(type_) = header.first().and_then(number).map(|n| n as i32) else { return };
        let id = header.get(1).and_then(number).map_or(0, |n| n as u32);
        let channel_name = header.get(2).and_then(text).unwrap_or_default();
        let name = header.get(3).and_then(text).unwrap_or_default();
        let describe = |prefix: &str| format!("{prefix}: {channel_name}.{name}");
        let length = message.byte_length();
        let log = |str: String, data: Option<&Marshalled>| {
            if let Some(logger) = &self.logger {
                logger.log_incoming(length, id, RequestInitiator::OtherSide, &str, data);
            }
        };
        match type_ {
            request_type::PROMISE => {
                log(describe(request_type_to_str(type_)), Some(&body));
                self.on_promise(RawRequest::Promise { id, channel_name, name, arg: body });
            }
            request_type::EVENT_LISTEN => {
                log(describe(request_type_to_str(type_)), Some(&body));
                self.on_event_listen(RawRequest::EventListen { id, channel_name, name, arg: body });
            }
            request_type::PROMISE_CANCEL | request_type::EVENT_DISPOSE => {
                log(request_type_to_str(type_).to_owned(), None);
                self.dispose_active_request(id);
            }
            _ => {}
        }
    }

    fn on_promise(&self, request: RawRequest) {
        let RawRequest::Promise { id, channel_name, name, arg } = &request else { return };
        let Some(channel) = self.channels.borrow().get(channel_name).cloned() else {
            self.collect_pending_request(request);
            return;
        };

        let cancellation_token_source = Rc::new(CancellationTokenSource::new(None));
        let ctx = self.ctx.clone();
        let (command, argument, token) = (name.clone(), arg.clone(), cancellation_token_source.token());
        let promise = match errors::catch(move || channel.call(&ctx, &command, argument, token)) {
            Ok(promise) => promise,
            Err(err) => Promise::rejected(err),
        };

        let id = *id;
        let weak = self.me.clone();
        let responder = weak.clone();
        let finisher = weak.clone();
        let disposable: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let finish_disposable = disposable.clone();
        promise
            .then_result(move |result| {
                if let Some(server) = responder.upgrade() {
                    match result {
                        Ok(data) => server.send_response(RawResponse::PromiseSuccess { id, data }),
                        Err(err) => server.send_error(id, &err),
                    }
                }
                Ok(())
            })
            .finally(move || {
                if let Some(disposable) = finish_disposable.borrow_mut().take() {
                    disposable.dispose();
                }
                if let Some(server) = finisher.upgrade() {
                    server.active_requests.borrow_mut().shift_remove(&id);
                }
            });

        let cancel_source = cancellation_token_source;
        let request_disposable = to_disposable(move || cancel_source.cancel());
        *disposable.borrow_mut() = Some(request_disposable.clone());
        self.active_requests.borrow_mut().insert(id, request_disposable);
    }

    /// O ramo `err instanceof Error` do `onPromise`: o erro com `message`, `name` e `stack` em linhas; um
    /// erro criado com [`error_obj`] vai como `PromiseErrorObj`.
    fn send_error(&self, id: u32, err: &Error) {
        if let Some(json) = err.prop(ERROR_OBJ_PROP) {
            let mut parse_errors = Vec::new();
            let data = crate::json::parse_str(json, &mut parse_errors).filter(|_| parse_errors.is_empty()).map_or(Marshalled::Undefined, |v| crate::marshalling::from_json(&v));
            self.send_response(RawResponse::PromiseErrorObj { id, data });
            return;
        }
        let stack = match &err.stack {
            Some(stack) if !stack.is_empty() => Marshalled::Array(stack.split('\n').map(string_value).collect()),
            _ => Marshalled::Undefined,
        };
        let data = object(vec![("message", string_value(&err.message)), ("name", string_value(&err.name)), ("stack", stack)]);
        self.send_response(RawResponse::PromiseError { id, data });
    }

    fn on_event_listen(&self, request: RawRequest) {
        let RawRequest::EventListen { id, channel_name, name, arg } = &request else { return };
        let Some(channel) = self.channels.borrow().get(channel_name).cloned() else {
            self.collect_pending_request(request);
            return;
        };

        let id = *id;
        let event = channel.listen(&self.ctx, name, arg.clone());
        let weak = self.me.clone();
        let disposable = event.listen(move |data| {
            if let Some(server) = weak.upgrade() {
                server.send_response(RawResponse::EventFire { id, data: data.clone() });
            }
        });

        self.active_requests.borrow_mut().insert(id, disposable);
    }

    fn dispose_active_request(&self, id: u32) {
        let disposable = self.active_requests.borrow_mut().shift_remove(&id);
        if let Some(disposable) = disposable {
            disposable.dispose();
        }
    }

    fn collect_pending_request(&self, request: RawRequest) {
        let (channel_name, id, is_promise) = match &request {
            RawRequest::Promise { channel_name, id, .. } => (channel_name.clone(), *id, true),
            RawRequest::EventListen { channel_name, id, .. } => (channel_name.clone(), *id, false),
            _ => return,
        };
        let weak = self.me.clone();
        let (name, delay) = (channel_name.clone(), self.timeout_delay);
        let timer = runtime::set_timeout(
            move || {
                eprintln!("Unknown channel: {name}");
                if is_promise && let Some(server) = weak.upgrade() {
                    let data = object(vec![
                        ("name", string_value("Unknown channel")),
                        ("message", string_value(&format!("Channel name '{name}' timed out after {delay}ms"))),
                        ("stack", Marshalled::Undefined),
                    ]);
                    server.send_response(RawResponse::PromiseError { id, data });
                }
            },
            self.timeout_delay,
        );
        self.pending_requests.borrow_mut().entry(channel_name).or_default().push(PendingRequest { request, timeout_timer: timer });
    }

    fn flush_pending_requests(&self, channel_name: &str) {
        let requests = self.pending_requests.borrow_mut().shift_remove(channel_name);
        for pending in requests.into_iter().flatten() {
            runtime::clear_timeout(pending.timeout_timer);
            match pending.request {
                request @ RawRequest::Promise { .. } => self.on_promise(request),
                request @ RawRequest::EventListen { .. } => self.on_event_listen(request),
                _ => {}
            }
        }
    }
}

impl IDisposable for ChannelServer {
    fn dispose(&self) {
        if let Some(listener) = self.protocol_listener.borrow_mut().take() {
            listener.dispose();
        }
        let active: Vec<DisposableRef> = self.active_requests.borrow_mut().drain(..).map(|(_, d)| d).collect();
        for disposable in active {
            disposable.dispose();
        }
    }
}

/// `ChannelClient`: acessa os canais de um [`ChannelServer`] do outro lado do protocolo.
pub struct ChannelClient {
    me: Weak<ChannelClient>,
    protocol: Rc<dyn IMessagePassingProtocol>,
    is_disposed: Cell<bool>,
    state: Cell<State>,
    active_requests: RefCell<Vec<DisposableRef>>,
    handlers: RefCell<IndexMap<u32, Handler>>,
    last_request_id: Cell<u32>,
    protocol_listener: RefCell<Option<DisposableRef>>,
    logger: Logger,
    on_did_initialize: Rc<Emitter<()>>,
    on_did_initialize_promise: RefCell<Option<Promise<()>>>,
}

struct ClientChannel {
    client: Rc<ChannelClient>,
    channel_name: String,
}

impl IChannel for ClientChannel {
    fn call(&self, command: &str, arg: Marshalled, cancellation_token: CancellationToken) -> Promise<Marshalled> {
        if self.client.is_disposed.get() {
            return Promise::rejected(Error::cancellation());
        }
        self.client.request_promise(&self.channel_name, command, arg, cancellation_token)
    }

    fn listen(&self, event: &str, arg: Marshalled) -> Event<Marshalled> {
        if self.client.is_disposed.get() {
            return Event::none();
        }
        self.client.request_event(&self.channel_name, event, arg)
    }
}

impl ChannelClient {
    pub fn new(protocol: Rc<dyn IMessagePassingProtocol>, logger: Logger) -> Rc<Self> {
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            protocol: protocol.clone(),
            is_disposed: Cell::new(false),
            state: Cell::new(State::Uninitialized),
            active_requests: RefCell::default(),
            handlers: RefCell::default(),
            last_request_id: Cell::new(0),
            protocol_listener: RefCell::default(),
            logger,
            on_did_initialize: Emitter::new(),
            on_did_initialize_promise: RefCell::default(),
        });
        let weak = this.me.clone();
        let listener = protocol.on_message().listen(move |msg| {
            if let Some(client) = weak.upgrade() {
                client.on_buffer(msg);
            }
        });
        *this.protocol_listener.borrow_mut() = Some(listener);
        this
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o cliente está vivo enquanto um método dele roda")
    }

    /// `onDidInitialize`.
    pub fn on_did_initialize(&self) -> Event<()> {
        self.on_did_initialize.event()
    }

    /// `getChannel`.
    pub fn get_channel(&self, channel_name: &str) -> Rc<dyn IChannel> {
        Rc::new(ClientChannel { client: self.this(), channel_name: channel_name.to_owned() })
    }

    fn request_promise(&self, channel_name: &str, name: &str, arg: Marshalled, cancellation_token: CancellationToken) -> Promise<Marshalled> {
        let id = self.last_request_id.get();
        self.last_request_id.set(id.wrapping_add(1));
        let request = RawRequest::Promise { id, channel_name: channel_name.to_owned(), name: name.to_owned(), arg };

        if cancellation_token.is_cancellation_requested() {
            return Promise::rejected(Error::cancellation());
        }

        let disposable: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let disposable_with_request_cancel: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let this = self.this();

        let result = {
            let (disposable, disposable_with_request_cancel, this, cancellation_token) =
                (disposable.clone(), disposable_with_request_cancel.clone(), this.clone(), cancellation_token.clone());
            Promise::with_executor(move |resolver| {
                if cancellation_token.is_cancellation_requested() {
                    resolver.reject(Error::cancellation());
                    return;
                }

                let do_request = {
                    let (this, resolver) = (this.clone(), resolver.clone());
                    move || {
                        let handler: Handler = {
                            let (weak, resolver) = (this.me.clone(), resolver.clone());
                            Rc::new(move |response: &RawResponse| {
                                let Some(client) = weak.upgrade() else { return };
                                match response {
                                    RawResponse::PromiseSuccess { data, .. } => {
                                        client.handlers.borrow_mut().shift_remove(&id);
                                        resolver.resolve(data.clone());
                                    }
                                    RawResponse::PromiseError { data, .. } => {
                                        client.handlers.borrow_mut().shift_remove(&id);
                                        resolver.reject(error_of_response(data));
                                    }
                                    RawResponse::PromiseErrorObj { data, .. } => {
                                        client.handlers.borrow_mut().shift_remove(&id);
                                        resolver.reject(error_obj(data));
                                    }
                                    _ => {}
                                }
                            })
                        };
                        this.handlers.borrow_mut().insert(id, handler);
                        this.send_request(&request);
                    }
                };

                let uninitialized_promise: Rc<RefCell<Option<CancelablePromise<()>>>> = Rc::default();
                if this.state.get() == State::Idle {
                    do_request();
                } else {
                    let weak = this.me.clone();
                    let promise = create_cancelable_promise(move |_| match weak.upgrade() {
                        Some(client) => client.when_initialized(),
                        None => Promise::resolved(()),
                    });
                    *uninitialized_promise.borrow_mut() = Some(promise.clone());
                    let slot = uninitialized_promise.clone();
                    promise.then_result(move |result| {
                        if result.is_ok() {
                            *slot.borrow_mut() = None;
                            do_request();
                        }
                        Ok(())
                    });
                }

                let cancel: Rc<dyn Fn()> = {
                    let (this, uninitialized_promise, resolver) = (this.clone(), uninitialized_promise.clone(), resolver.clone());
                    Rc::new(move || {
                        let pending = uninitialized_promise.borrow_mut().take();
                        match pending {
                            Some(promise) => promise.cancel(),
                            None => this.send_request(&RawRequest::PromiseCancel { id }),
                        }
                        resolver.reject(Error::cancellation());
                    })
                };

                let listener = cancellation_token.on_cancellation_requested().listen({
                    let cancel = cancel.clone();
                    move |_| cancel()
                });
                *disposable.borrow_mut() = Some(listener.clone());
                // Um objeto simples (`{ dispose }`) no original, que o rastreador de vazamentos não vê.
                let with_cancel = plain_disposable(move || {
                    cancel();
                    listener.dispose();
                });
                *disposable_with_request_cancel.borrow_mut() = Some(with_cancel.clone());
                this.active_requests.borrow_mut().push(with_cancel);
            })
        };

        result.finally(move || {
            // Seen as undefined in tests.
            if let Some(disposable) = disposable.borrow_mut().take() {
                disposable.dispose();
            }
            if let Some(entry) = disposable_with_request_cancel.borrow_mut().take() {
                this.active_requests.borrow_mut().retain(|d| !Rc::ptr_eq(d, &entry));
            }
        })
    }

    fn request_event(&self, channel_name: &str, name: &str, arg: Marshalled) -> Event<Marshalled> {
        let id = self.last_request_id.get();
        self.last_request_id.set(id.wrapping_add(1));
        let request = RawRequest::EventListen { id, channel_name: channel_name.to_owned(), name: name.to_owned(), arg };

        let uninitialized_promise: Rc<RefCell<Option<CancelablePromise<()>>>> = Rc::default();
        let emitter_slot: Rc<RefCell<Weak<Emitter<Marshalled>>>> = Rc::default();
        let this = self.this();

        let emitter = Emitter::with_options(EmitterOptions {
            on_will_add_first_listener: Some(Rc::new({
                let (this, uninitialized_promise, emitter_slot) = (this.clone(), uninitialized_promise.clone(), emitter_slot.clone());
                move |_: &Emitter<Marshalled>| {
                    let handler: Handler = {
                        let emitter_slot = emitter_slot.clone();
                        Rc::new(move |response: &RawResponse| {
                            let data = match response {
                                RawResponse::PromiseSuccess { data, .. } | RawResponse::PromiseError { data, .. } | RawResponse::PromiseErrorObj { data, .. } | RawResponse::EventFire { data, .. } => data.clone(),
                                RawResponse::Initialize => Marshalled::Undefined,
                            };
                            let emitter = emitter_slot.borrow().upgrade();
                            if let Some(emitter) = emitter {
                                emitter.fire(data);
                            }
                        })
                    };
                    this.handlers.borrow_mut().insert(id, handler);
                    let do_request = {
                        let (this, emitter_slot, request) = (this.clone(), emitter_slot.clone(), request.clone());
                        move || {
                            if let Some(emitter) = emitter_slot.borrow().upgrade() {
                                this.active_requests.borrow_mut().push(emitter);
                            }
                            this.send_request(&request);
                        }
                    };
                    if this.state.get() == State::Idle {
                        do_request();
                    } else {
                        let weak = this.me.clone();
                        let promise = create_cancelable_promise(move |_| match weak.upgrade() {
                            Some(client) => client.when_initialized(),
                            None => Promise::resolved(()),
                        });
                        *uninitialized_promise.borrow_mut() = Some(promise.clone());
                        let slot = uninitialized_promise.clone();
                        promise.then_result(move |result| {
                            if result.is_ok() {
                                *slot.borrow_mut() = None;
                                do_request();
                            }
                            Ok(())
                        });
                    }
                }
            })),
            on_did_remove_last_listener: Some(Rc::new({
                let (this, uninitialized_promise, emitter_slot) = (this.clone(), uninitialized_promise.clone(), emitter_slot.clone());
                move |_: &Emitter<Marshalled>| {
                    let pending = uninitialized_promise.borrow_mut().take();
                    match pending {
                        Some(promise) => promise.cancel(),
                        None => {
                            if let Some(emitter) = emitter_slot.borrow().upgrade() {
                                let entry: DisposableRef = emitter;
                                this.active_requests.borrow_mut().retain(|d| !Rc::ptr_eq(d, &entry));
                            }
                            this.send_request(&RawRequest::EventDispose { id });
                        }
                    }
                    this.handlers.borrow_mut().shift_remove(&id);
                }
            })),
            ..EmitterOptions::default()
        });
        *emitter_slot.borrow_mut() = Rc::downgrade(&emitter);

        // O `Event` guarda o emitter vivo.
        let keep_alive = emitter.clone();
        Event::new(move |listener| keep_alive.event().listen_rc(listener))
    }

    fn send_request(&self, request: &RawRequest) {
        match request {
            RawRequest::Promise { id, channel_name, name, arg } | RawRequest::EventListen { id, channel_name, name, arg } => {
                let header = Marshalled::Array(vec![
                    Marshalled::Number(f64::from(request.type_())),
                    Marshalled::Number(f64::from(*id)),
                    string_value(channel_name),
                    string_value(name),
                ]);
                let length = send(&*self.protocol, header, arg.clone());
                if let Some(logger) = &self.logger {
                    logger.log_outgoing(length, *id, RequestInitiator::LocalSide, &format!("{}: {channel_name}.{name}", request_type_to_str(request.type_())), Some(arg));
                }
            }
            RawRequest::PromiseCancel { id } | RawRequest::EventDispose { id } => {
                let header = Marshalled::Array(vec![Marshalled::Number(f64::from(request.type_())), Marshalled::Number(f64::from(*id))]);
                let length = send(&*self.protocol, header, Marshalled::Undefined);
                if let Some(logger) = &self.logger {
                    logger.log_outgoing(length, *id, RequestInitiator::LocalSide, request_type_to_str(request.type_()), None);
                }
            }
        }
    }

    fn on_buffer(&self, message: &VSBuffer) {
        let mut reader = BufferReader::new(message.clone());
        let header = deserialize(&mut reader);
        let body = deserialize(&mut reader);
        let Marshalled::Array(header) = header else { return };
        let Some(type_) = header.first().and_then(number).map(|n| n as i32) else { return };
        let id = header.get(1).and_then(number).map_or(0, |n| n as u32);
        let length = message.byte_length();
        let log = |data: Option<&Marshalled>, request_id: u32| {
            if let Some(logger) = &self.logger {
                logger.log_incoming(length, request_id, RequestInitiator::LocalSide, response_type_to_str(type_), data);
            }
        };
        let response = match type_ {
            response_type::INITIALIZE => {
                log(None, 0);
                RawResponse::Initialize
            }
            response_type::PROMISE_SUCCESS => {
                log(Some(&body), id);
                RawResponse::PromiseSuccess { id, data: body }
            }
            response_type::PROMISE_ERROR => {
                log(Some(&body), id);
                RawResponse::PromiseError { id, data: body }
            }
            response_type::EVENT_FIRE => {
                log(Some(&body), id);
                RawResponse::EventFire { id, data: body }
            }
            response_type::PROMISE_ERROR_OBJ => {
                log(Some(&body), id);
                RawResponse::PromiseErrorObj { id, data: body }
            }
            _ => return,
        };
        self.on_response(&response);
    }

    fn on_response(&self, response: &RawResponse) {
        let id = match response {
            RawResponse::Initialize => {
                self.state.set(State::Idle);
                self.on_did_initialize.fire(());
                return;
            }
            RawResponse::PromiseSuccess { id, .. } | RawResponse::PromiseError { id, .. } | RawResponse::PromiseErrorObj { id, .. } | RawResponse::EventFire { id, .. } => *id,
        };
        let handler = self.handlers.borrow().get(&id).cloned();
        if let Some(handler) = handler {
            handler(response);
        }
    }

    /// `onDidInitializePromise`: memoizada.
    fn on_did_initialize_promise(&self) -> Promise<()> {
        self.on_did_initialize_promise.borrow_mut().get_or_insert_with(|| self.on_did_initialize().to_promise(None).promise).clone()
    }

    fn when_initialized(&self) -> Promise<()> {
        if self.state.get() == State::Idle { Promise::resolved(()) } else { self.on_did_initialize_promise() }
    }
}

/// O ramo `PromiseError` do `onResponse` do cliente: reconstrói o `Error` com `message`, `name` e `stack`.
fn error_of_response(data: &Marshalled) -> Error {
    let field = |name: &str| data.key(name);
    let message = field("message").and_then(text).unwrap_or_default();
    let name = field("name").and_then(text).unwrap_or_else(|| "Error".to_owned());
    let mut error = Error::named(name, message);
    error.stack = match field("stack") {
        Some(Marshalled::Array(lines)) => Some(lines.iter().map(|l| text(l).unwrap_or_default()).collect::<Vec<_>>().join("\n")),
        Some(Marshalled::String(s)) => Some(s.to_string_lossy()),
        _ => None,
    };
    error
}

impl IDisposable for ChannelClient {
    fn dispose(&self) {
        self.is_disposed.set(true);
        if let Some(listener) = self.protocol_listener.borrow_mut().take() {
            listener.dispose();
        }
        let active: Vec<DisposableRef> = std::mem::take(&mut *self.active_requests.borrow_mut());
        for disposable in active {
            disposable.dispose();
        }
        self.on_did_initialize.dispose();
    }
}

/// `ClientConnectionEvent`.
#[derive(Clone)]
pub struct ClientConnectionEvent {
    pub protocol: Rc<dyn IMessagePassingProtocol>,
    pub on_did_client_disconnect: Event<()>,
}

/// `Connection<TContext>` (e `Client<TContext>`): o contexto do cliente e os dois lados do canal.
pub struct Connection {
    pub channel_server: Rc<ChannelServer>,
    pub channel_client: Rc<ChannelClient>,
    pub ctx: Marshalled,
}

/// `IConnectionHub`.
pub trait IConnectionHub {
    fn connections(&self) -> Vec<Rc<Connection>>;
    fn on_did_add_connection(&self) -> Event<Rc<Connection>>;
    fn on_did_remove_connection(&self) -> Event<Rc<Connection>>;
}

/// `IClientRouter`: escolhe o cliente de uma chamada quando há vários possíveis.
pub trait IClientRouter {
    fn route_call(&self, hub: Rc<dyn IConnectionHub>, command: &str, arg: &Marshalled, cancellation_token: &CancellationToken) -> Promise<Rc<Connection>>;
    fn route_event(&self, hub: Rc<dyn IConnectionHub>, event: &str, arg: &Marshalled) -> Promise<Rc<Connection>>;
}

/// Como [`IPCServer::get_channel`] escolhe o cliente: um roteador, ou um filtro (sem roteador, uma chamada vai
/// para um cliente qualquer que passe no filtro e um evento escuta todos).
#[derive(Clone)]
pub enum ClientSelector {
    Router(Rc<dyn IClientRouter>),
    Filter(Rc<dyn Fn(&Connection) -> bool>),
}

/// As inscrições por conexão do evento multiplexado.
type ConnectionSubscriptions = Rc<RefCell<Vec<(Rc<Connection>, DisposableRef)>>>;

/// Quem trata uma conexão que entra.
type ConnectionCallback = Rc<dyn Fn(&Rc<Connection>)>;

/// `IPCServer`: um servidor de canais e um cliente de canais com roteamento.
pub struct IPCServer {
    me: Weak<IPCServer>,
    channels: RefCell<IndexMap<String, Rc<dyn IServerChannel>>>,
    connections: RefCell<Vec<Rc<Connection>>>,
    on_did_add_connection: Rc<Emitter<Rc<Connection>>>,
    on_did_remove_connection: Rc<Emitter<Rc<Connection>>>,
    disposables: Rc<DisposableStore>,
}

impl IPCServer {
    pub fn new(on_did_client_connect: Event<ClientConnectionEvent>, ipc_logger: Logger, timeout_delay: Option<u64>) -> Rc<Self> {
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            channels: RefCell::default(),
            connections: RefCell::default(),
            on_did_add_connection: Emitter::new(),
            on_did_remove_connection: Emitter::new(),
            disposables: DisposableStore::new(),
        });
        let weak = this.me.clone();
        let listener = on_did_client_connect.listen(move |connect| {
            let Some(server) = weak.upgrade() else { return };
            let ClientConnectionEvent { protocol, on_did_client_disconnect } = connect.clone();
            let on_first_message = protocol.on_message().once();
            let connection_disposables = DisposableStore::new();

            let first_message_disposable = on_first_message.listen({
                let (weak, connection_disposables, protocol, ipc_logger) = (weak.clone(), connection_disposables.clone(), protocol.clone(), ipc_logger.clone());
                move |msg| {
                    let Some(server) = weak.upgrade() else { return };
                    let mut reader = BufferReader::new(msg.clone());
                    let ctx = deserialize(&mut reader);

                    let channel_server = ChannelServer::new(protocol.clone(), ctx.clone(), ipc_logger.clone(), timeout_delay);
                    let channel_client = ChannelClient::new(protocol.clone(), ipc_logger.clone());

                    let channels: Vec<(String, Rc<dyn IServerChannel>)> = server.channels.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    for (name, channel) in channels {
                        channel_server.register_channel(&name, channel);
                    }

                    let connection = Rc::new(Connection { channel_server, channel_client, ctx });
                    server.connections.borrow_mut().push(connection.clone());
                    server.on_did_add_connection.fire(connection.clone());

                    let removal = on_did_client_disconnect.listen({
                        let (weak, connection, connection_disposables) = (weak.clone(), connection, connection_disposables.clone());
                        move |()| {
                            connection.channel_server.dispose();
                            connection.channel_client.dispose();
                            let Some(server) = weak.upgrade() else { return };
                            server.connections.borrow_mut().retain(|c| !Rc::ptr_eq(c, &connection));
                            server.on_did_remove_connection.fire(connection.clone());
                            let as_ref: DisposableRef = connection_disposables.clone();
                            server.disposables.delete(&as_ref);
                            connection_disposables.dispose();
                        }
                    });
                    connection_disposables.add(removal);
                }
            });

            connection_disposables.add(first_message_disposable);
            let as_ref: DisposableRef = connection_disposables;
            server.disposables.add(as_ref);
        });
        this.disposables.add(listener);
        this
    }

    fn this(&self) -> Rc<Self> {
        self.me.upgrade().expect("o servidor está vivo enquanto um método dele roda")
    }

    /// `registerChannel`.
    pub fn register_channel(&self, channel_name: &str, channel: Rc<dyn IServerChannel>) {
        self.channels.borrow_mut().insert(channel_name.to_owned(), channel.clone());
        let connections = self.connections.borrow().clone();
        for connection in connections {
            connection.channel_server.register_channel(channel_name, channel.clone());
        }
    }

    /// `getChannel`: um canal de um cliente remoto. Com um roteador, ele escolhe o cliente da chamada e do
    /// evento; com um filtro, uma chamada vai para um cliente qualquer que passe nele e um evento escuta todos.
    pub fn get_channel(&self, channel_name: &str, selector: ClientSelector) -> Rc<dyn IChannel> {
        Rc::new(RoutedChannel { server: self.this(), channel_name: channel_name.to_owned(), selector })
    }

    fn get_multicast_event(&self, channel_name: &str, client_filter: Rc<dyn Fn(&Connection) -> bool>, event_name: &str, arg: Marshalled) -> Event<Marshalled> {
        let that = self.this();
        let disposables: Rc<RefCell<Option<Rc<DisposableStore>>>> = Rc::default();
        let emitter_slot: Rc<RefCell<Weak<Emitter<Marshalled>>>> = Rc::default();
        let (channel_name, event_name) = (channel_name.to_owned(), event_name.to_owned());

        // Create an emitter which hooks up to all clients as soon as first listener is added. It also disconnects
        // from all clients as soon as the last listener is removed.
        let emitter = Emitter::with_options(EmitterOptions {
            on_will_add_first_listener: Some(Rc::new({
                let (that, disposables, emitter_slot, client_filter) = (that.clone(), disposables.clone(), emitter_slot.clone(), client_filter.clone());
                move |_: &Emitter<Marshalled>| {
                    let store = DisposableStore::new();
                    *disposables.borrow_mut() = Some(store.clone());

                    // The event multiplexer is useful since the active client list is dynamic. We need to hook up
                    // and disconnection to/from clients as they come and go.
                    let event_multiplexer = EventMultiplexer::<Marshalled>::new();
                    let map: ConnectionSubscriptions = Rc::default();

                    let on_did_add_connection: ConnectionCallback = Rc::new({
                        let (event_multiplexer, map, channel_name, event_name, arg) = (event_multiplexer.clone(), map.clone(), channel_name.clone(), event_name.clone(), arg.clone());
                        move |connection: &Rc<Connection>| {
                            let channel = connection.channel_client.get_channel(&channel_name);
                            let event = channel.listen(&event_name, arg.clone());
                            let disposable = event_multiplexer.add(event);
                            map.borrow_mut().push((connection.clone(), disposable));
                        }
                    });

                    let on_did_remove_connection = {
                        let map = map.clone();
                        move |connection: &Rc<Connection>| {
                            let found = map.borrow().iter().position(|(c, _)| Rc::ptr_eq(c, connection));
                            if let Some(index) = found {
                                let (_, disposable) = map.borrow_mut().remove(index);
                                disposable.dispose();
                            }
                        }
                    };

                    let current: Vec<Rc<Connection>> = that.connections.borrow().iter().filter(|c| client_filter(c)).cloned().collect();
                    for connection in &current {
                        on_did_add_connection(connection);
                    }
                    let filter = client_filter.clone();
                    store.add(that.on_did_add_connection.event().filter(move |c| filter(c), None).listen({
                        let on_did_add_connection = on_did_add_connection.clone();
                        move |c| on_did_add_connection(c)
                    }));
                    store.add(that.on_did_remove_connection.event().listen(on_did_remove_connection));
                    store.add(event_multiplexer.event().listen({
                        let emitter_slot = emitter_slot.clone();
                        move |value| {
                            let emitter = emitter_slot.borrow().upgrade();
                            if let Some(emitter) = emitter {
                                emitter.fire(value.clone());
                            }
                        }
                    }));
                    store.add(event_multiplexer);
                }
            })),
            on_did_remove_last_listener: Some(Rc::new({
                let disposables = disposables.clone();
                move |_: &Emitter<Marshalled>| {
                    if let Some(store) = disposables.borrow_mut().take() {
                        store.dispose();
                    }
                }
            })),
            ..EmitterOptions::default()
        });
        *emitter_slot.borrow_mut() = Rc::downgrade(&emitter);
        self.disposables.add(emitter.clone());
        Event::new(move |listener| emitter.event().listen_rc(listener))
    }
}

impl IConnectionHub for IPCServer {
    fn connections(&self) -> Vec<Rc<Connection>> {
        self.connections.borrow().clone()
    }

    fn on_did_add_connection(&self) -> Event<Rc<Connection>> {
        self.on_did_add_connection.event()
    }

    fn on_did_remove_connection(&self) -> Event<Rc<Connection>> {
        self.on_did_remove_connection.event()
    }
}

impl IDisposable for IPCServer {
    fn dispose(&self) {
        self.disposables.dispose();

        let connections = std::mem::take(&mut *self.connections.borrow_mut());
        for connection in connections {
            connection.channel_client.dispose();
            connection.channel_server.dispose();
        }

        self.channels.borrow_mut().clear();
        self.on_did_add_connection.dispose();
        self.on_did_remove_connection.dispose();
    }
}

struct RoutedChannel {
    server: Rc<IPCServer>,
    channel_name: String,
    selector: ClientSelector,
}

impl IChannel for RoutedChannel {
    fn call(&self, command: &str, arg: Marshalled, cancellation_token: CancellationToken) -> Promise<Marshalled> {
        let that = &self.server;
        let connection_promise: Promise<Rc<Connection>> = match &self.selector {
            ClientSelector::Filter(filter) => {
                // when no router is provided, we go random client picking
                let candidates: Vec<Rc<Connection>> = that.connections.borrow().iter().filter(|c| filter(c)).cloned().collect();
                match get_random_element(&candidates) {
                    // if we found a client, let's call on it
                    Some(connection) => Promise::resolved(connection.clone()),
                    // else, let's wait for a client to come along
                    None => {
                        let filter = filter.clone();
                        that.on_did_add_connection.event().filter(move |c| filter(c), None).to_promise(None).promise
                    }
                }
            }
            ClientSelector::Router(router) => router.route_call(that.clone(), command, &arg, &CancellationToken::None),
        };

        let channel_name = self.channel_name.clone();
        let channel_promise = connection_promise.then(move |connection| connection.channel_client.get_channel(&channel_name));
        get_delayed_channel(channel_promise).call(command, arg, cancellation_token)
    }

    fn listen(&self, event: &str, arg: Marshalled) -> Event<Marshalled> {
        match &self.selector {
            ClientSelector::Filter(filter) => self.server.get_multicast_event(&self.channel_name, filter.clone(), event, arg),
            ClientSelector::Router(router) => {
                let channel_name = self.channel_name.clone();
                let channel_promise = router.route_event(self.server.clone(), event, &arg).then(move |connection| connection.channel_client.get_channel(&channel_name));
                get_delayed_channel(channel_promise).listen(event, arg)
            }
        }
    }
}

/// `IPCClient`: um cliente de canais e um servidor de canais.
pub struct IPCClient {
    channel_client: Rc<ChannelClient>,
    channel_server: Rc<ChannelServer>,
}

impl IPCClient {
    pub fn new(protocol: Rc<dyn IMessagePassingProtocol>, ctx: Marshalled, ipc_logger: Logger) -> Self {
        let mut writer = BufferWriter::new();
        serialize(&mut writer, &ctx);
        protocol.send(writer.buffer());
        writer.dispose();

        let channel_client = ChannelClient::new(protocol.clone(), ipc_logger.clone());
        let channel_server = ChannelServer::new(protocol, ctx, ipc_logger, None);
        Self { channel_client, channel_server }
    }

    pub fn get_channel(&self, channel_name: &str) -> Rc<dyn IChannel> {
        self.channel_client.get_channel(channel_name)
    }

    pub fn register_channel(&self, channel_name: &str, channel: Rc<dyn IServerChannel>) {
        self.channel_server.register_channel(channel_name, channel);
    }
}

impl IDisposable for IPCClient {
    fn dispose(&self) {
        self.channel_client.dispose();
        self.channel_server.dispose();
    }
}

struct DelayedChannel {
    promise: Promise<Rc<dyn IChannel>>,
}

impl IChannel for DelayedChannel {
    fn call(&self, command: &str, arg: Marshalled, cancellation_token: CancellationToken) -> Promise<Marshalled> {
        let command = command.to_owned();
        self.promise.then_promise(move |channel| channel.call(&command, arg, cancellation_token))
    }

    fn listen(&self, event: &str, arg: Marshalled) -> Event<Marshalled> {
        let relay = Relay::<Marshalled>::new();
        let event = event.to_owned();
        let (on_ok, on_err) = (relay.clone(), relay.clone());
        self.promise.then_result(move |result| {
            match result {
                Ok(channel) => on_ok.set_input(channel.listen(&event, arg)),
                Err(_) => on_err.dispose(),
            }
            Ok(())
        });
        let emitter_relay = relay;
        Event::new(move |listener| emitter_relay.event().listen_rc(listener))
    }
}

/// `getDelayedChannel`: um canal que espera a promessa do canal verdadeiro.
pub fn get_delayed_channel(promise: Promise<Rc<dyn IChannel>>) -> Rc<dyn IChannel> {
    Rc::new(DelayedChannel { promise })
}

struct NextTickChannel {
    channel: Rc<dyn IChannel>,
    did_tick: Rc<Cell<bool>>,
}

impl IChannel for NextTickChannel {
    fn call(&self, command: &str, arg: Marshalled, cancellation_token: CancellationToken) -> Promise<Marshalled> {
        if self.did_tick.get() {
            return self.channel.call(command, arg, cancellation_token);
        }

        let (channel, did_tick, command) = (self.channel.clone(), self.did_tick.clone(), command.to_owned());
        timeout(0).promise.then(move |()| did_tick.set(true)).then_promise(move |()| channel.call(&command, arg, cancellation_token))
    }

    fn listen(&self, event: &str, arg: Marshalled) -> Event<Marshalled> {
        if self.did_tick.get() {
            return self.channel.listen(event, arg);
        }

        let relay = Relay::<Marshalled>::new();
        let (channel, did_tick, event, input) = (self.channel.clone(), self.did_tick.clone(), event.to_owned(), relay.clone());
        timeout(0).promise.then(move |()| did_tick.set(true)).then(move |()| input.set_input(channel.listen(&event, arg)));
        Event::new(move |listener| relay.event().listen_rc(listener))
    }
}

/// `getNextTickChannel`: adia a primeira chamada ou escuta para o próximo tick do laço de eventos.
pub fn get_next_tick_channel(channel: Rc<dyn IChannel>) -> Rc<dyn IChannel> {
    Rc::new(NextTickChannel { channel, did_tick: Rc::new(Cell::new(false)) })
}

/// `StaticRouter`: o primeiro cliente cujo contexto passa na função; se nenhum passa, espera outro conectar.
pub struct StaticRouter {
    f: RouterFilter,
}

/// `(ctx: TContext) => boolean | Promise<boolean>`.
type RouterFilter = Rc<dyn Fn(&Marshalled) -> MaybePromise<bool>>;

impl StaticRouter {
    pub fn new(f: impl Fn(&Marshalled) -> MaybePromise<bool> + 'static) -> Self {
        Self { f: Rc::new(f) }
    }

    fn route(hub: Rc<dyn IConnectionHub>, f: RouterFilter) -> Promise<Rc<Connection>> {
        async_fn(async move {
            loop {
                for connection in hub.connections() {
                    let matched = match f(&connection.ctx) {
                        MaybePromise::Value(value) => Promise::resolved(value).await?,
                        MaybePromise::Promise(promise) => promise.await?,
                    };
                    if matched {
                        return Promise::resolved(connection).await;
                    }
                }
                hub.on_did_add_connection().to_promise(None).await?;
            }
        })
    }
}

impl IClientRouter for StaticRouter {
    fn route_call(&self, hub: Rc<dyn IConnectionHub>, _command: &str, _arg: &Marshalled, _token: &CancellationToken) -> Promise<Rc<Connection>> {
        Self::route(hub, self.f.clone())
    }

    fn route_event(&self, hub: Rc<dyn IConnectionHub>, _event: &str, _arg: &Marshalled) -> Promise<Rc<Connection>> {
        Self::route(hub, self.f.clone())
    }
}

/// `ProxyChannel`: serviços como canais, sem escrever cada método e evento à mão.
///
/// - Com marshalling ligado, só `URI` e `RegExp` são convertidos sozinhos (`revive`).
/// - Eventos seguem a convenção `onUpperCase`; eventos dinâmicos, `onDynamicUpperCase`.
/// - `CancellationToken` não é suportado.
pub mod proxy_channel {
    use super::*;

    /// `IProxyOptions`.
    #[derive(Clone, Copy, Default)]
    pub struct IProxyOptions {
        /// Disables automatic marshalling of `URI`. If marshalling is disabled, `UriComponents` must be used.
        pub disable_marshalling: bool,
    }

    /// `ICreateServiceChannelOptions`.
    #[derive(Clone, Default)]
    pub struct ICreateServiceChannelOptions {
        pub proxy: IProxyOptions,
        /// Events that should subscribe lazily and not replay emissions before the first IPC listener.
        pub unbuffered_events: Vec<String>,
    }

    /// O serviço que um canal expõe: o `handler[key]` do original, por nome.
    pub trait IProxyService {
        /// As chaves do serviço com forma de evento (`onSomething`), para o `for (const key in handler)`.
        fn event_names(&self) -> Vec<String>;
        /// `handler[name]` como evento, se existe.
        fn event(&self, name: &str) -> Option<Event<Marshalled>>;
        /// `handler[name].call(handler, arg)` de um evento dinâmico, se existe.
        fn dynamic_event(&self, name: &str, arg: Marshalled) -> Option<Event<Marshalled>>;
        /// `handler[name].apply(handler, args)` de um método, se existe.
        fn call(&self, name: &str, args: Vec<Marshalled>) -> Option<Promise<Marshalled>>;
    }

    struct ServiceChannel {
        service: Rc<dyn IProxyService>,
        disable_marshalling: bool,
        unbuffered_events: Vec<String>,
        disposables: Rc<DisposableStore>,
        map_event_name_to_event: RefCell<IndexMap<String, Event<Marshalled>>>,
        platform: Platform,
    }

    impl ServiceChannel {
        fn buffered(&self, name: &str, event: &Event<Marshalled>) -> Event<Marshalled> {
            event.buffer(name, true, Vec::new(), Some(&self.disposables))
        }
    }

    impl IServerChannel for ServiceChannel {
        fn listen(&self, _ctx: &Marshalled, event: &str, arg: Marshalled) -> Event<Marshalled> {
            if let Some(event_impl) = self.map_event_name_to_event.borrow().get(event) {
                return event_impl.clone();
            }

            if property_is_dynamic_event(event) {
                if let Some(dynamic) = self.service.dynamic_event(event, arg) {
                    return dynamic;
                }
            } else if let Some(target) = self.service.event(event)
                && property_is_event(event)
            {
                if self.unbuffered_events.iter().any(|e| e == event) {
                    return target;
                }
                let buffered = self.buffered(event, &target);
                self.map_event_name_to_event.borrow_mut().insert(event.to_owned(), buffered.clone());
                return buffered;
            }

            errors::throw(Error::named("ErrorNoTelemetry", format!("Event not found: {event}")))
        }

        fn call(&self, _ctx: &Marshalled, command: &str, args: Marshalled, _cancellation_token: CancellationToken) -> Promise<Marshalled> {
            let mut args = match args {
                Marshalled::Array(args) => args,
                Marshalled::Undefined => Vec::new(),
                other => vec![other],
            };
            // Revive unless marshalling disabled
            if !self.disable_marshalling {
                args = args.into_iter().map(|arg| revive(arg, 0, self.platform)).collect();
            }
            match self.service.call(command, args) {
                Some(promise) => promise,
                None => errors::throw(Error::named("ErrorNoTelemetry", format!("Method not found: {command}"))),
            }
        }
    }

    /// `fromService`.
    pub fn from_service(service: Rc<dyn IProxyService>, disposables: &Rc<DisposableStore>, options: Option<ICreateServiceChannelOptions>) -> Rc<dyn IServerChannel> {
        let options = options.unwrap_or_default();

        // Buffer any event that should be supported by iterating over all property keys and finding them.
        let channel = ServiceChannel {
            service: service.clone(),
            disable_marshalling: options.proxy.disable_marshalling,
            unbuffered_events: options.unbuffered_events,
            disposables: disposables.clone(),
            map_event_name_to_event: RefCell::default(),
            platform: Platform::Posix,
        };
        for key in service.event_names() {
            if property_is_event(&key)
                && !channel.unbuffered_events.contains(&key)
                && let Some(event) = service.event(&key)
            {
                let buffered = channel.buffered(&key, &event);
                channel.map_event_name_to_event.borrow_mut().insert(key, buffered);
            }
        }
        Rc::new(channel)
    }

    /// `ICreateProxyServiceOptions`.
    #[derive(Clone, Default)]
    pub struct ICreateProxyServiceOptions {
        pub proxy: IProxyOptions,
        /// If provided, will add the value of `context` to each method call to the target.
        pub context: Option<Marshalled>,
        /// If provided, will not proxy any of the properties that are part of the map but rather return that value.
        pub properties: IndexMap<String, Marshalled>,
    }

    /// O proxy de um serviço do outro lado do canal (`toService`).
    pub struct ServiceProxy {
        channel: Rc<dyn IChannel>,
        options: ICreateProxyServiceOptions,
        platform: Platform,
    }

    impl ServiceProxy {
        /// Uma propriedade predefinida (`options.properties`).
        pub fn property(&self, name: &str) -> Option<Marshalled> {
            self.options.properties.get(name).cloned()
        }

        /// `proxy.onSomething`: o evento.
        pub fn event(&self, name: &str) -> Event<Marshalled> {
            self.channel.listen(name, Marshalled::Undefined)
        }

        /// `proxy.onDynamicSomething(arg)`.
        pub fn dynamic_event(&self, name: &str, arg: Marshalled) -> Event<Marshalled> {
            self.channel.listen(name, arg)
        }

        /// `await proxy.method(...args)`.
        pub fn call(&self, name: &str, args: Vec<Marshalled>) -> Promise<Marshalled> {
            // Add context if any
            let method_args = match &self.options.context {
                Some(context) if !matches!(context, Marshalled::Null | Marshalled::Undefined) => std::iter::once(context.clone()).chain(args).collect(),
                _ => args,
            };

            let disable_marshalling = self.options.proxy.disable_marshalling;
            let platform = self.platform;
            self.channel.call(name, Marshalled::Array(method_args), CancellationToken::None).then(move |result| {
                // Revive unless marshalling disabled
                if disable_marshalling { result } else { revive(result, 0, platform) }
            })
        }
    }

    /// `toService`.
    pub fn to_service(channel: Rc<dyn IChannel>, options: Option<ICreateProxyServiceOptions>) -> ServiceProxy {
        ServiceProxy { channel, options: options.unwrap_or_default(), platform: Platform::Posix }
    }

    /// Assume a property is an event if it has a form of "onSomething".
    pub fn property_is_event(name: &str) -> bool {
        let units: Vec<u16> = name.encode_utf16().collect();
        units.first() == Some(&u16::from(b'o')) && units.get(1) == Some(&u16::from(b'n')) && units.get(2).is_some_and(|&u| is_upper_ascii_letter(u))
    }

    /// Assume a property is a dynamic event (a method that returns an event) if it has a form of
    /// "onDynamicSomething".
    pub fn property_is_dynamic_event(name: &str) -> bool {
        let units: Vec<u16> = name.encode_utf16().collect();
        name.starts_with("onDynamic") && units.get(9).is_some_and(|&u| is_upper_ascii_letter(u))
    }
}

pub use proxy_channel as ProxyChannel;

/// `IPCLogger`: escreve cada mensagem com o prefixo da direção, o total acumulado e o tamanho.
pub struct IPCLogger {
    total_incoming: Cell<usize>,
    total_outgoing: Cell<usize>,
    outgoing_prefix: String,
    incoming_prefix: String,
}

impl IPCLogger {
    pub fn new(outgoing_prefix: &str, incoming_prefix: &str) -> Self {
        Self { total_incoming: Cell::new(0), total_outgoing: Cell::new(0), outgoing_prefix: outgoing_prefix.to_owned(), incoming_prefix: incoming_prefix.to_owned() }
    }

    fn log(direction: &str, total_length: usize, msg_length: usize, req: u32, str: &str, data: Option<&Marshalled>) {
        let data = data.and_then(json_stringify).map(|j| j.to_string_lossy()).unwrap_or_default();
        println!("[{direction}][{total_length:>7}][len: {msg_length:>5}]{req:>5} - {str} {data}");
    }
}

impl IIPCLogger for IPCLogger {
    fn log_outgoing(&self, msg_length: usize, request_id: u32, _initiator: RequestInitiator, str: &str, data: Option<&Marshalled>) {
        self.total_outgoing.set(self.total_outgoing.get() + msg_length);
        Self::log(&self.outgoing_prefix, self.total_outgoing.get(), msg_length, request_id, str, data);
    }

    fn log_incoming(&self, msg_length: usize, request_id: u32, _initiator: RequestInitiator, str: &str, data: Option<&Marshalled>) {
        self.total_incoming.set(self.total_incoming.get() + msg_length);
        Self::log(&self.incoming_prefix, self.total_incoming.get(), msg_length, request_id, str, data);
    }
}
