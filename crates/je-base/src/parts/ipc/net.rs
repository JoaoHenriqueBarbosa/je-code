//! Porte de `src/vs/base/parts/ipc/common/ipc.net.ts`: o protocolo de mensagens sobre um socket
//! (`Protocol`, `PersistentProtocol` com confirmações e reconexão), o `ChunkStream` e o `Client`.
//!
//! O formato de uma mensagem no fio, byte a byte como no original:
//!
//! ```text
//!     /-------------------------------|------\
//!     |             HEADER            |      |
//!     |-------------------------------| DATA |
//!     | TYPE | ID | ACK | DATA_LENGTH |      |
//!     \-------------------------------|------/
//! ```
//!
//! O cabeçalho tem 13 bytes: TYPE (1 byte, `ProtocolMessageType`), ID (4 bytes, big endian, o id da mensagem;
//! 0 para ser ignorado), ACK (4 bytes, big endian, o id confirmado; 0 para ser ignorado) e DATA_LENGTH (4 bytes,
//! big endian). Só as mensagens `Regular` são contadas e confirmadas.
//!
//! Decisões de tradução:
//!
//! - `ISocket` é um trait com eventos (`on_data`, `on_close`, `on_end`) no lugar de `onData(listener)`.
//! - `setTimeout`, `setInterval` e `Date.now()` são os do [`runtime`], que o teste troca por um relógio virtual.
//! - `SocketDiagnostics` (desligado por uma constante no original) não é portado; `trace_socket_event` existe
//!   no trait como gancho sem efeito.
//! - O `BufferedEmitter` entrega as mensagens guardadas numa microtarefa, como o original.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use crate::async_::{IntervalHandle, clear_interval, set_interval};
use crate::buffer::VSBuffer;
use crate::errors::{self, Error};
use crate::event::{Emitter, EmitterOptions, Event};
use crate::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use crate::marshalling::Marshalled;
use crate::parts::ipc::{IIPCLogger, IMessagePassingProtocol, IPCClient};
use crate::promise::Promise;
use crate::runtime::{self, TimerId};

/// `SocketDiagnosticsEventType`, os tipos de evento que um socket pode registrar (desligado no original).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketDiagnosticsEventType {
    Created,
    Read,
    Write,
    Open,
    Error,
    Close,
    BrowserWebSocketBlobReceived,
    NodeEndReceived,
    NodeEndSent,
    NodeDrainBegin,
    NodeDrainEnd,
    ZlibInflateError,
    ZlibInflateData,
    ZlibInflateInitialWrite,
    ZlibInflateInitialFlushFired,
    ZlibInflateWrite,
    ZlibInflateFlushFired,
    ZlibDeflateError,
    ZlibDeflateData,
    ZlibDeflateWrite,
    ZlibDeflateFlushFired,
    WebSocketNodeSocketWrite,
    WebSocketNodeSocketPeekedHeader,
    WebSocketNodeSocketReadHeader,
    WebSocketNodeSocketReadData,
    WebSocketNodeSocketUnmaskedData,
    WebSocketNodeSocketDrainBegin,
    WebSocketNodeSocketDrainEnd,
    ProtocolHeaderRead,
    ProtocolMessageRead,
    ProtocolHeaderWrite,
    ProtocolMessageWrite,
    ProtocolWrite,
}

/// `SocketCloseEvent` (o `undefined` é `None` no evento).
#[derive(Clone, Debug, PartialEq)]
pub enum SocketCloseEvent {
    /// `NodeSocketCloseEvent`.
    Node {
        /// `true` if the socket had a transmission error.
        had_error: bool,
        /// Underlying error.
        error: Option<Error>,
    },
    /// `WebSocketCloseEvent`.
    WebSocket {
        /// The WebSocket connection close code provided by the server.
        code: u16,
        /// The WebSocket connection close reason provided by the server.
        reason: String,
        /// True if the connection closed cleanly; false otherwise.
        was_clean: bool,
    },
}

/// `SocketTimeoutReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketTimeoutReason {
    UnacknowledgedMessage,
    KeepAlive,
}

/// `SocketTimeoutEvent`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SocketTimeoutEvent {
    pub reason: SocketTimeoutReason,
    pub unacknowledged_msg_count: usize,
    pub time_since_oldest_unacknowledged_msg: Option<f64>,
    pub time_since_last_received_some_data: f64,
}

/// `ISocket`.
pub trait ISocket: IDisposable {
    fn on_data(&self) -> Event<VSBuffer>;
    fn on_close(&self) -> Event<Option<SocketCloseEvent>>;
    fn on_end(&self) -> Event<()>;
    fn write(&self, buffer: VSBuffer);
    fn end(&self);
    fn drain(&self) -> Promise<()>;

    fn trace_socket_event(&self, _type: SocketDiagnosticsEventType, _data: Option<&VSBuffer>) {}
}

/// `ChunkStream`: uma fila de pedaços de bytes lida em tamanhos arbitrários.
#[derive(Default)]
pub struct ChunkStream {
    chunks: VecDeque<VSBuffer>,
    total_length: usize,
}

impl ChunkStream {
    pub fn new() -> Self {
        Self::default()
    }

    /// `byteLength`.
    pub fn byte_length(&self) -> usize {
        self.total_length
    }

    pub fn accept_chunk(&mut self, buff: VSBuffer) {
        self.total_length += buff.byte_length();
        self.chunks.push_back(buff);
    }

    pub fn read(&mut self, byte_count: usize) -> VSBuffer {
        self.read_impl(byte_count, true)
    }

    pub fn peek(&mut self, byte_count: usize) -> VSBuffer {
        self.read_impl(byte_count, false)
    }

    fn read_impl(&mut self, byte_count: usize, advance: bool) -> VSBuffer {
        if byte_count == 0 {
            return VSBuffer::alloc(0);
        }

        if byte_count > self.total_length {
            errors::throw(Error::new("Cannot read so many bytes!"));
        }

        let first_length = self.chunks[0].byte_length();
        if first_length == byte_count {
            // super fast path, precisely first chunk must be returned
            let result = self.chunks[0].clone();
            if advance {
                self.chunks.pop_front();
                self.total_length -= byte_count;
            }
            return result;
        }

        if first_length > byte_count {
            // fast path, the reading is entirely within the first chunk
            let result = self.chunks[0].slice(Some(0), Some(byte_count as i64));
            if advance {
                self.chunks[0] = self.chunks[0].slice(Some(byte_count as i64), None);
                self.total_length -= byte_count;
            }
            return result;
        }

        let result = VSBuffer::alloc(byte_count);
        let mut remaining = byte_count;
        let mut result_offset = 0;
        let mut chunk_index = 0;
        while remaining > 0 {
            let chunk = self.chunks[chunk_index].clone();
            if chunk.byte_length() > remaining {
                // this chunk will survive
                result.set(&chunk.slice(Some(0), Some(remaining as i64)), result_offset);

                if advance {
                    self.chunks[chunk_index] = chunk.slice(Some(remaining as i64), None);
                    self.total_length -= remaining;
                }

                remaining = 0;
            } else {
                // this chunk will be entirely read
                result.set(&chunk, result_offset);
                result_offset += chunk.byte_length();

                if advance {
                    self.chunks.pop_front();
                    self.total_length -= chunk.byte_length();
                } else {
                    chunk_index += 1;
                }

                remaining -= chunk.byte_length();
            }
        }
        result
    }
}

/// `ProtocolMessageType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolMessageType {
    None = 0,
    Regular = 1,
    Control = 2,
    Ack = 3,
    Disconnect = 5,
    ReplayRequest = 6,
    Pause = 7,
    Resume = 8,
    KeepAlive = 9,
}

impl ProtocolMessageType {
    fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::None,
            1 => Self::Regular,
            2 => Self::Control,
            3 => Self::Ack,
            5 => Self::Disconnect,
            6 => Self::ReplayRequest,
            7 => Self::Pause,
            8 => Self::Resume,
            9 => Self::KeepAlive,
            _ => return Option::None,
        })
    }
}

/// `ProtocolConstants`.
pub mod protocol_constants {
    pub const HEADER_LENGTH: usize = 13;
    /// Send an Acknowledge message at most 2 seconds later...
    pub const ACKNOWLEDGE_TIME: f64 = 2000.0;
    /// If there is a sent message that has been unacknowledged for 20 seconds, and we didn't see any incoming
    /// server data in the past 20 seconds, then consider the connection has timed out.
    pub const TIMEOUT_TIME: f64 = 20000.0;
    /// If there is no reconnection within this time-frame, consider the connection permanently closed...
    pub const RECONNECTION_GRACE_TIME: f64 = 3.0 * 60.0 * 60.0 * 1000.0;
    /// Maximal grace time between the first and the last reconnection...
    pub const RECONNECTION_SHORT_GRACE_TIME: f64 = 5.0 * 60.0 * 1000.0;
    /// Send a message every 5 seconds to avoid that the connection is closed by the OS.
    pub const KEEP_ALIVE_SEND_TIME: u64 = 5000;
}

fn now() -> f64 {
    runtime::date_now() as f64
}

/// `ProtocolMessage`.
pub struct ProtocolMessage {
    pub r#type: ProtocolMessageType,
    pub id: u32,
    pub ack: u32,
    pub data: VSBuffer,
    pub written_time: Cell<f64>,
}

impl ProtocolMessage {
    pub fn new(r#type: ProtocolMessageType, id: u32, ack: u32, data: VSBuffer) -> Rc<Self> {
        Rc::new(Self { r#type, id, ack, data, written_time: Cell::new(0.0) })
    }

    pub fn size(&self) -> usize {
        self.data.byte_length()
    }
}

struct ReaderState {
    read_head: bool,
    read_len: usize,
    message_type: Option<ProtocolMessageType>,
    id: u32,
    ack: u32,
}

/// `ProtocolReader`.
struct ProtocolReader {
    me: Weak<ProtocolReader>,
    is_disposed: Cell<bool>,
    incoming_data: RefCell<ChunkStream>,
    last_read_time: Cell<f64>,
    on_message: Rc<Emitter<Rc<ProtocolMessage>>>,
    state: RefCell<ReaderState>,
    socket_listener: RefCell<Option<DisposableRef>>,
}

impl ProtocolReader {
    fn new(socket: &Rc<dyn ISocket>) -> Rc<Self> {
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            is_disposed: Cell::new(false),
            incoming_data: RefCell::default(),
            last_read_time: Cell::new(now()),
            on_message: Emitter::new(),
            state: RefCell::new(ReaderState { read_head: true, read_len: protocol_constants::HEADER_LENGTH, message_type: Some(ProtocolMessageType::None), id: 0, ack: 0 }),
            socket_listener: RefCell::default(),
        });
        let weak = this.me.clone();
        let listener = socket.on_data().listen(move |data| {
            if let Some(reader) = weak.upgrade() {
                reader.accept_chunk(Some(data.clone()));
            }
        });
        *this.socket_listener.borrow_mut() = Some(listener);
        this
    }

    fn on_message(&self) -> Event<Rc<ProtocolMessage>> {
        self.on_message.event()
    }

    fn last_read_time(&self) -> f64 {
        self.last_read_time.get()
    }

    fn accept_chunk(&self, data: Option<VSBuffer>) {
        let Some(data) = data.filter(|d| d.byte_length() != 0) else { return };

        self.last_read_time.set(now());
        self.incoming_data.borrow_mut().accept_chunk(data);

        loop {
            let read_len = self.state.borrow().read_len;
            if self.incoming_data.borrow().byte_length() < read_len {
                break;
            }

            let buff = self.incoming_data.borrow_mut().read(read_len);

            let read_head = self.state.borrow().read_head;
            if read_head {
                // buff is the header

                // save new state => next time will read the body
                let mut state = self.state.borrow_mut();
                state.read_head = false;
                state.read_len = buff.read_u_int32_be(9) as usize;
                state.message_type = ProtocolMessageType::from_u8(buff.read_u_int8(0));
                state.id = buff.read_u_int32_be(1);
                state.ack = buff.read_u_int32_be(5);
            } else {
                // buff is the body
                let (message_type, id, ack) = {
                    let mut state = self.state.borrow_mut();
                    let current = (state.message_type, state.id, state.ack);

                    // save new state => next time will read the header
                    state.read_head = true;
                    state.read_len = protocol_constants::HEADER_LENGTH;
                    state.message_type = Some(ProtocolMessageType::None);
                    state.id = 0;
                    state.ack = 0;
                    current
                };

                // Um tipo desconhecido não tem equivalente em `ProtocolMessageType`; o `switch` do original
                // ignora o que não reconhece, então a mensagem é descartada.
                if let Some(message_type) = message_type {
                    self.on_message.fire(ProtocolMessage::new(message_type, id, ack, buff));
                }

                if self.is_disposed.get() {
                    // check if an event listener lead to our disposal
                    break;
                }
            }
        }
    }

    fn read_entire_buffer(&self) -> VSBuffer {
        let length = self.incoming_data.borrow().byte_length();
        self.incoming_data.borrow_mut().read(length)
    }
}

impl IDisposable for ProtocolReader {
    fn dispose(&self) {
        self.is_disposed.set(true);
        if let Some(listener) = self.socket_listener.borrow_mut().take() {
            listener.dispose();
        }
        self.on_message.dispose();
    }
}

/// `ProtocolWriter`.
struct ProtocolWriter {
    me: Weak<ProtocolWriter>,
    is_disposed: Cell<bool>,
    is_paused: Cell<bool>,
    socket: Rc<dyn ISocket>,
    data: RefCell<Vec<VSBuffer>>,
    total_length: Cell<usize>,
    last_write_time: Cell<f64>,
    write_now_timeout: Cell<Option<TimerId>>,
}

impl ProtocolWriter {
    fn new(socket: Rc<dyn ISocket>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            is_disposed: Cell::new(false),
            is_paused: Cell::new(false),
            socket,
            data: RefCell::default(),
            total_length: Cell::new(0),
            last_write_time: Cell::new(0.0),
            write_now_timeout: Cell::new(None),
        })
    }

    fn drain(&self) -> Promise<()> {
        self.flush();
        self.socket.drain()
    }

    fn flush(&self) {
        // flush
        self.write_now();
    }

    fn pause(&self) {
        self.is_paused.set(true);
    }

    fn resume(&self) {
        self.is_paused.set(false);
        self.schedule_writing();
    }

    fn write(&self, msg: &ProtocolMessage) {
        if self.is_disposed.get() {
            // ignore: there could be left-over promises which complete and then decide to write a response, etc...
            return;
        }
        msg.written_time.set(now());
        self.last_write_time.set(now());
        let header = VSBuffer::alloc(protocol_constants::HEADER_LENGTH);
        header.write_u_int8(msg.r#type as u8, 0);
        header.write_u_int32_be(msg.id, 1);
        header.write_u_int32_be(msg.ack, 5);
        header.write_u_int32_be(msg.data.byte_length() as u32, 9);

        self.write_soon(header, msg.data.clone());
    }

    fn buffer_add(&self, head: VSBuffer, body: VSBuffer) -> bool {
        let was_empty = self.total_length.get() == 0;
        self.total_length.set(self.total_length.get() + head.byte_length() + body.byte_length());
        let mut data = self.data.borrow_mut();
        data.push(head);
        data.push(body);
        was_empty
    }

    fn buffer_take(&self) -> VSBuffer {
        let ret = VSBuffer::concat(&self.data.borrow(), Some(self.total_length.get()));
        self.data.borrow_mut().clear();
        self.total_length.set(0);
        ret
    }

    fn write_soon(&self, header: VSBuffer, data: VSBuffer) {
        if self.buffer_add(header, data) {
            self.schedule_writing();
        }
    }

    fn schedule_writing(&self) {
        if self.write_now_timeout.get().is_some() {
            return;
        }
        let weak = self.me.clone();
        self.write_now_timeout.set(Some(runtime::set_timeout(
            move || {
                if let Some(writer) = weak.upgrade() {
                    writer.write_now_timeout.set(None);
                    writer.write_now();
                }
            },
            0,
        )));
    }

    fn write_now(&self) {
        if self.total_length.get() == 0 {
            return;
        }
        if self.is_paused.get() {
            return;
        }
        let data = self.buffer_take();
        self.socket.write(data);
    }
}

impl IDisposable for ProtocolWriter {
    fn dispose(&self) {
        // ignore error, since the socket could be already closed
        let _ = errors::catch(|| self.flush());
        self.is_disposed.set(true);
    }
}

/// O que `Protocol` e `PersistentProtocol` têm em comum para o [`Client`].
pub trait INetProtocol: IMessagePassingProtocol {
    fn get_socket(&self) -> Rc<dyn ISocket>;
    fn send_disconnect(&self);
    fn dispose(&self);
    fn on_did_dispose(&self) -> Event<()>;
}

/// `Protocol`: o protocolo de mensagens sobre um socket, sem confirmações.
///
/// Só as mensagens `Regular` são contadas.
pub struct Protocol {
    socket: Rc<dyn ISocket>,
    socket_writer: Rc<ProtocolWriter>,
    socket_reader: Rc<ProtocolReader>,
    on_message: Rc<Emitter<VSBuffer>>,
    on_did_dispose: Rc<Emitter<()>>,
    disposables: Rc<DisposableStore>,
}

impl Protocol {
    pub fn new(socket: Rc<dyn ISocket>) -> Rc<Self> {
        let disposables = DisposableStore::new();
        let socket_writer = ProtocolWriter::new(socket.clone());
        disposables.add_rc(socket_writer.clone());
        let socket_reader = ProtocolReader::new(&socket);
        disposables.add_rc(socket_reader.clone());
        let on_message = Emitter::new();
        let on_did_dispose = Emitter::new();
        disposables.add_rc(on_message.clone());
        disposables.add_rc(on_did_dispose.clone());

        let sink = on_message.clone();
        disposables.add(socket_reader.on_message().listen(move |msg| {
            if msg.r#type == ProtocolMessageType::Regular {
                sink.fire(msg.data.clone());
            }
        }));

        let sink = on_did_dispose.clone();
        disposables.add(socket.on_close().listen(move |_| sink.fire(())));

        Rc::new(Self { socket, socket_writer, socket_reader, on_message, on_did_dispose, disposables })
    }

    pub fn drain(&self) -> Promise<()> {
        self.socket_writer.drain()
    }

    /// `readEntireBuffer`.
    pub fn read_entire_buffer(&self) -> VSBuffer {
        self.socket_reader.read_entire_buffer()
    }
}

impl IMessagePassingProtocol for Protocol {
    fn send(&self, buffer: VSBuffer) {
        self.socket_writer.write(&ProtocolMessage::new(ProtocolMessageType::Regular, 0, 0, buffer));
    }

    fn on_message(&self) -> Event<VSBuffer> {
        self.on_message.event()
    }

    fn drain(&self) -> Option<Promise<()>> {
        Some(self.socket_writer.drain())
    }
}

impl INetProtocol for Protocol {
    fn get_socket(&self) -> Rc<dyn ISocket> {
        self.socket.clone()
    }

    fn send_disconnect(&self) {
        // Nothing to do...
    }

    fn dispose(&self) {
        self.disposables.dispose();
    }

    fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose.event()
    }
}

/// `Client<TContext>`: um [`IPCClient`] sobre um socket.
pub struct Client {
    ipc_client: IPCClient,
    protocol: Rc<dyn INetProtocol>,
}

impl Client {
    /// `Client.fromSocket`.
    pub fn from_socket(socket: Rc<dyn ISocket>, id: Marshalled) -> Self {
        Self::new(Protocol::new(socket), id, None)
    }

    pub fn new(protocol: Rc<dyn INetProtocol>, id: Marshalled, ipc_logger: Option<Rc<dyn IIPCLogger>>) -> Self {
        let message_protocol: Rc<dyn IMessagePassingProtocol> = Rc::new(ProtocolHandle(protocol.clone()));
        Self { ipc_client: IPCClient::new(message_protocol, id, ipc_logger), protocol }
    }

    pub fn on_did_dispose(&self) -> Event<()> {
        self.protocol.on_did_dispose()
    }

    pub fn get_channel(&self, channel_name: &str) -> Rc<dyn crate::parts::ipc::IChannel> {
        self.ipc_client.get_channel(channel_name)
    }

    pub fn register_channel(&self, channel_name: &str, channel: Rc<dyn crate::parts::ipc::IServerChannel>) {
        self.ipc_client.register_channel(channel_name, channel);
    }
}

impl IDisposable for Client {
    fn dispose(&self) {
        self.ipc_client.dispose();
        let socket = self.protocol.get_socket();
        // should be sent gracefully with a .flush(), but try to send it out as a last resort here if nothing else:
        self.protocol.send_disconnect();
        self.protocol.dispose();
        socket.end();
    }
}

/// Um [`INetProtocol`] visto como [`IMessagePassingProtocol`].
struct ProtocolHandle(Rc<dyn INetProtocol>);

impl IMessagePassingProtocol for ProtocolHandle {
    fn send(&self, buffer: VSBuffer) {
        self.0.send(buffer);
    }

    fn on_message(&self) -> Event<VSBuffer> {
        self.0.on_message()
    }

    fn drain(&self) -> Option<Promise<()>> {
        self.0.drain()
    }
}

struct BufferedState<T: 'static> {
    emitter: RefCell<Weak<Emitter<T>>>,
    has_listeners: Cell<bool>,
    is_delivering_messages: Cell<bool>,
    buffered_messages: RefCell<VecDeque<T>>,
}

/// `BufferedEmitter<T>`: garante que nenhuma mensagem se perde se não há ouvintes.
pub struct BufferedEmitter<T: Clone + 'static> {
    emitter: Rc<Emitter<T>>,
    state: Rc<BufferedState<T>>,
}

impl<T: Clone + 'static> BufferedEmitter<T> {
    pub fn new() -> Self {
        let state = Rc::new(BufferedState { emitter: RefCell::default(), has_listeners: Cell::new(false), is_delivering_messages: Cell::new(false), buffered_messages: RefCell::default() });
        let emitter = Emitter::with_options(EmitterOptions {
            on_will_add_first_listener: Some(Rc::new({
                let state = state.clone();
                move |_: &Emitter<T>| {
                    state.has_listeners.set(true);
                    // it is important to deliver these messages after this call, but before other messages have a
                    // chance to be received (to guarantee in order delivery) that's why we're using here
                    // queueMicrotask and not other types of timeouts
                    let state = state.clone();
                    runtime::queue_microtask(move || Self::deliver_messages(&state));
                }
            })),
            on_did_remove_last_listener: Some(Rc::new({
                let state = state.clone();
                move |_: &Emitter<T>| state.has_listeners.set(false)
            })),
            ..EmitterOptions::default()
        });
        *state.emitter.borrow_mut() = Rc::downgrade(&emitter);
        Self { emitter, state }
    }

    fn deliver_messages(state: &BufferedState<T>) {
        if state.is_delivering_messages.get() {
            return;
        }
        state.is_delivering_messages.set(true);
        while state.has_listeners.get() {
            let next = state.buffered_messages.borrow_mut().pop_front();
            let emitter = state.emitter.borrow().upgrade();
            match (next, emitter) {
                (Some(message), Some(emitter)) => emitter.fire(message),
                _ => break,
            }
        }
        state.is_delivering_messages.set(false);
    }

    pub fn event(&self) -> Event<T> {
        self.emitter.event()
    }

    pub fn fire(&self, event: T) {
        if self.state.has_listeners.get() {
            if !self.state.buffered_messages.borrow().is_empty() {
                self.state.buffered_messages.borrow_mut().push_back(event);
            } else {
                self.emitter.fire(event);
            }
        } else {
            self.state.buffered_messages.borrow_mut().push_back(event);
        }
    }

    pub fn flush_buffer(&self) {
        self.state.buffered_messages.borrow_mut().clear();
    }
}

impl<T: Clone + 'static> Default for BufferedEmitter<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `LoadEstimator`: estima a carga do laço de eventos pelos tiques de um intervalo de 1 s.
pub struct LoadEstimator {
    last_runs: Rc<RefCell<Vec<f64>>>,
    interval: IntervalHandle,
}

const HISTORY_LENGTH: usize = 10;

thread_local! {
    static LOAD_ESTIMATOR: RefCell<Option<Rc<LoadEstimator>>> = const { RefCell::new(None) };
}

impl LoadEstimator {
    /// `getInstance`: o estimador do thread.
    pub fn get_instance() -> Rc<LoadEstimator> {
        LOAD_ESTIMATOR.with(|slot| slot.borrow_mut().get_or_insert_with(|| Rc::new(LoadEstimator::new())).clone())
    }

    pub fn new() -> Self {
        let start = now();
        let last_runs: Vec<f64> = (0..HISTORY_LENGTH).map(|i| start - 1000.0 * i as f64).collect();
        let last_runs = Rc::new(RefCell::new(last_runs));
        let ticking = last_runs.clone();
        let interval = set_interval(
            move || {
                let mut runs = ticking.borrow_mut();
                // O original desloca até o índice `HISTORY_LENGTH` (um a mais que o histórico lido).
                runs.push(0.0);
                for i in (1..=HISTORY_LENGTH).rev() {
                    runs[i] = runs[i - 1];
                }
                runs[0] = now();
                runs.truncate(HISTORY_LENGTH + 1);
            },
            1000,
        );
        Self { last_runs, interval }
    }

    /// Returns an estimative number, from 0 (low load) to 1 (high load).
    fn load(&self) -> f64 {
        let now = now();
        let history_limit = (1 + HISTORY_LENGTH) as f64 * 1000.0;
        let runs = self.last_runs.borrow();
        let score = runs.iter().take(HISTORY_LENGTH).filter(|&&run| now - run <= history_limit).count();
        1.0 - score as f64 / HISTORY_LENGTH as f64
    }
}

impl ILoadEstimator for LoadEstimator {
    fn has_high_load(&self) -> bool {
        self.load() >= 0.5
    }
}

impl Default for LoadEstimator {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for LoadEstimator {
    fn drop(&mut self) {
        clear_interval(&self.interval);
    }
}

/// `ILoadEstimator`.
pub trait ILoadEstimator {
    fn has_high_load(&self) -> bool;
}

/// `PersistentProtocolOptions`.
pub struct PersistentProtocolOptions {
    /// The socket to use.
    pub socket: Rc<dyn ISocket>,
    /// The initial chunk of data that has already been received from the socket.
    pub initial_chunk: Option<VSBuffer>,
    /// The CPU load estimator to use.
    pub load_estimator: Option<Rc<dyn ILoadEstimator>>,
    /// Whether to send keep alive messages. Defaults to true.
    pub send_keep_alive: Option<bool>,
}

struct Connection {
    socket: Rc<dyn ISocket>,
    writer: Rc<ProtocolWriter>,
    reader: Rc<ProtocolReader>,
    disposables: Rc<DisposableStore>,
}

struct PersistentState {
    is_reconnecting: bool,
    did_send_disconnect: bool,

    outgoing_unack_msg: VecDeque<Rc<ProtocolMessage>>,
    outgoing_msg_id: u32,
    outgoing_ack_id: u32,
    outgoing_ack_timeout: Option<TimerId>,

    incoming_msg_id: u32,
    incoming_ack_id: u32,
    incoming_msg_last_time: f64,
    incoming_ack_timeout: Option<TimerId>,

    keep_alive_interval: Option<IntervalHandle>,

    last_replay_request_time: f64,
    last_socket_timeout_time: f64,
}

/// `PersistentProtocol`: igual ao [`Protocol`], mas rastreia as mensagens e as confirmações, e garante que
/// nenhuma mensagem se perde enquanto não há ouvintes.
pub struct PersistentProtocol {
    me: Weak<PersistentProtocol>,
    state: RefCell<PersistentState>,
    connection: RefCell<Connection>,
    load_estimator: Rc<dyn ILoadEstimator>,
    on_control_message: BufferedEmitter<VSBuffer>,
    on_message: BufferedEmitter<VSBuffer>,
    on_did_dispose: BufferedEmitter<()>,
    on_socket_close: BufferedEmitter<Option<SocketCloseEvent>>,
    on_socket_timeout: BufferedEmitter<SocketTimeoutEvent>,
}

impl PersistentProtocol {
    pub fn new(opts: PersistentProtocolOptions) -> Rc<Self> {
        let load_estimator: Rc<dyn ILoadEstimator> = opts.load_estimator.unwrap_or_else(|| LoadEstimator::get_instance());
        let should_send_keep_alive = opts.send_keep_alive.unwrap_or(true);
        let this = Rc::new_cyclic(|me: &Weak<PersistentProtocol>| {
            let connection = Self::open_connection(me, opts.socket.clone());
            Self {
                me: me.clone(),
                state: RefCell::new(PersistentState {
                    is_reconnecting: false,
                    did_send_disconnect: false,
                    outgoing_unack_msg: VecDeque::new(),
                    outgoing_msg_id: 0,
                    outgoing_ack_id: 0,
                    outgoing_ack_timeout: None,
                    incoming_msg_id: 0,
                    incoming_ack_id: 0,
                    incoming_msg_last_time: 0.0,
                    incoming_ack_timeout: None,
                    keep_alive_interval: None,
                    last_replay_request_time: 0.0,
                    last_socket_timeout_time: now(),
                }),
                connection: RefCell::new(connection),
                load_estimator,
                on_control_message: BufferedEmitter::new(),
                on_message: BufferedEmitter::new(),
                on_did_dispose: BufferedEmitter::new(),
                on_socket_close: BufferedEmitter::new(),
                on_socket_timeout: BufferedEmitter::new(),
            }
        });

        if let Some(initial_chunk) = opts.initial_chunk {
            let reader = this.connection.borrow().reader.clone();
            reader.accept_chunk(Some(initial_chunk));
        }

        if should_send_keep_alive {
            let weak = this.me.clone();
            let interval = set_interval(
                move || {
                    if let Some(protocol) = weak.upgrade() {
                        protocol.send_keep_alive();
                    }
                },
                protocol_constants::KEEP_ALIVE_SEND_TIME,
            );
            this.state.borrow_mut().keep_alive_interval = Some(interval);
        }
        this
    }

    /// Liga o escritor e o leitor a um socket e registra os ouvintes que o original guarda em
    /// `_socketDisposables`.
    fn open_connection(me: &Weak<PersistentProtocol>, socket: Rc<dyn ISocket>) -> Connection {
        let disposables = DisposableStore::new();
        let writer = ProtocolWriter::new(socket.clone());
        disposables.add_rc(writer.clone());
        let reader = ProtocolReader::new(&socket);
        disposables.add_rc(reader.clone());
        let weak = me.clone();
        disposables.add(reader.on_message().listen(move |msg| {
            if let Some(protocol) = weak.upgrade() {
                protocol.receive_message(msg);
            }
        }));
        let weak = me.clone();
        disposables.add(socket.on_close().listen(move |e| {
            if let Some(protocol) = weak.upgrade() {
                protocol.on_socket_close.fire(e.clone());
            }
        }));
        Connection { socket, writer, reader, disposables }
    }

    pub fn on_control_message(&self) -> Event<VSBuffer> {
        self.on_control_message.event()
    }

    pub fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose.event()
    }

    pub fn on_socket_close(&self) -> Event<Option<SocketCloseEvent>> {
        self.on_socket_close.event()
    }

    pub fn on_socket_timeout(&self) -> Event<SocketTimeoutEvent> {
        self.on_socket_timeout.event()
    }

    /// `unacknowledgedCount`.
    pub fn unacknowledged_count(&self) -> u32 {
        let state = self.state.borrow();
        state.outgoing_msg_id - state.outgoing_ack_id
    }

    fn writer(&self) -> Rc<ProtocolWriter> {
        self.connection.borrow().writer.clone()
    }

    fn reader(&self) -> Rc<ProtocolReader> {
        self.connection.borrow().reader.clone()
    }

    pub fn dispose(&self) {
        let mut state = self.state.borrow_mut();
        if let Some(timer) = state.outgoing_ack_timeout.take() {
            runtime::clear_timeout(timer);
        }
        if let Some(timer) = state.incoming_ack_timeout.take() {
            runtime::clear_timeout(timer);
        }
        let interval = state.keep_alive_interval.take();
        drop(state);
        if let Some(interval) = interval {
            clear_interval(&interval);
        }
        let disposables = self.connection.borrow().disposables.clone();
        disposables.dispose();
    }

    pub fn drain(&self) -> Promise<()> {
        self.writer().drain()
    }

    pub fn send_disconnect(&self) {
        if !self.state.borrow().did_send_disconnect {
            self.state.borrow_mut().did_send_disconnect = true;
            let msg = ProtocolMessage::new(ProtocolMessageType::Disconnect, 0, 0, VSBuffer::alloc(0));
            let writer = self.writer();
            writer.write(&msg);
            writer.flush();
        }
    }

    pub fn send_pause(&self) {
        let msg = ProtocolMessage::new(ProtocolMessageType::Pause, 0, 0, VSBuffer::alloc(0));
        self.writer().write(&msg);
    }

    pub fn send_resume(&self) {
        let msg = ProtocolMessage::new(ProtocolMessageType::Resume, 0, 0, VSBuffer::alloc(0));
        self.writer().write(&msg);
    }

    pub fn pause_socket_writing(&self) {
        self.writer().pause();
    }

    pub fn get_socket(&self) -> Rc<dyn ISocket> {
        self.connection.borrow().socket.clone()
    }

    pub fn get_millis_since_last_incoming_data(&self) -> f64 {
        now() - self.reader().last_read_time()
    }

    pub fn begin_accept_reconnection(&self, socket: Rc<dyn ISocket>, initial_data_chunk: Option<VSBuffer>) {
        self.state.borrow_mut().is_reconnecting = true;

        let old = self.connection.borrow().disposables.clone();
        old.dispose();
        self.on_control_message.flush_buffer();
        self.on_socket_close.flush_buffer();
        self.on_socket_timeout.flush_buffer();
        let old_socket = self.connection.borrow().socket.clone();
        old_socket.dispose();

        {
            let mut state = self.state.borrow_mut();
            state.last_replay_request_time = 0.0;
            state.last_socket_timeout_time = now();
        }

        let connection = Self::open_connection(&self.me, socket);
        let reader = connection.reader.clone();
        *self.connection.borrow_mut() = connection;

        reader.accept_chunk(initial_data_chunk);
    }

    pub fn end_accept_reconnection(&self) {
        let (ack_id, to_send) = {
            let mut state = self.state.borrow_mut();
            state.is_reconnecting = false;

            // After a reconnection, let the other party know (again) which messages have been received. (perhaps
            // the other party didn't receive a previous ACK)
            state.incoming_ack_id = state.incoming_msg_id;
            (state.incoming_ack_id, state.outgoing_unack_msg.iter().cloned().collect::<Vec<_>>())
        };
        let writer = self.writer();
        writer.write(&ProtocolMessage::new(ProtocolMessageType::Ack, 0, ack_id, VSBuffer::alloc(0)));

        // Send again all unacknowledged messages
        for msg in &to_send {
            writer.write(msg);
        }
        self.recv_ack_check();
    }

    pub fn accept_disconnect(&self) {
        self.on_did_dispose.fire(());
    }

    fn receive_message(&self, msg: &Rc<ProtocolMessage>) {
        {
            let mut state = self.state.borrow_mut();
            if msg.ack > state.outgoing_ack_id {
                state.outgoing_ack_id = msg.ack;
                while let Some(first) = state.outgoing_unack_msg.front() {
                    if first.id <= msg.ack {
                        // this message has been confirmed, remove it
                        state.outgoing_unack_msg.pop_front();
                    } else {
                        break;
                    }
                }
            }
        }

        match msg.r#type {
            ProtocolMessageType::None => {
                // N/A
            }
            ProtocolMessageType::Regular => {
                let incoming_msg_id = self.state.borrow().incoming_msg_id;
                if msg.id > incoming_msg_id {
                    if msg.id != incoming_msg_id + 1 {
                        // in case we missed some messages we ask the other party to resend them
                        let now = now();
                        if now - self.state.borrow().last_replay_request_time > 10000.0 {
                            // send a replay request at most once every 10s
                            self.state.borrow_mut().last_replay_request_time = now;
                            self.writer().write(&ProtocolMessage::new(ProtocolMessageType::ReplayRequest, 0, 0, VSBuffer::alloc(0)));
                        }
                    } else {
                        {
                            let mut state = self.state.borrow_mut();
                            state.incoming_msg_id = msg.id;
                            state.incoming_msg_last_time = now();
                        }
                        self.send_ack_check();
                        self.on_message.fire(msg.data.clone());
                    }
                }
            }
            ProtocolMessageType::Control => self.on_control_message.fire(msg.data.clone()),
            ProtocolMessageType::Ack => {
                // nothing to do, .ack is handled above already
            }
            ProtocolMessageType::Disconnect => self.on_did_dispose.fire(()),
            ProtocolMessageType::ReplayRequest => {
                // Send again all unacknowledged messages
                let to_send: Vec<Rc<ProtocolMessage>> = self.state.borrow().outgoing_unack_msg.iter().cloned().collect();
                let writer = self.writer();
                for msg in &to_send {
                    writer.write(msg);
                }
                self.recv_ack_check();
            }
            ProtocolMessageType::Pause => self.writer().pause(),
            ProtocolMessageType::Resume => self.writer().resume(),
            ProtocolMessageType::KeepAlive => {
                // nothing to do
            }
        }
    }

    pub fn read_entire_buffer(&self) -> VSBuffer {
        self.reader().read_entire_buffer()
    }

    pub fn flush(&self) {
        self.writer().flush();
    }

    /// Send a message which will not be part of the regular acknowledge flow. Use this for early control messages
    /// which are repeated in case of reconnection.
    pub fn send_control(&self, buffer: VSBuffer) {
        let msg = ProtocolMessage::new(ProtocolMessageType::Control, 0, 0, buffer);
        self.writer().write(&msg);
    }

    fn send_ack_check(&self) {
        let (incoming_msg_id, incoming_ack_id, has_timeout, last_time) = {
            let state = self.state.borrow();
            (state.incoming_msg_id, state.incoming_ack_id, state.incoming_ack_timeout.is_some(), state.incoming_msg_last_time)
        };
        if incoming_msg_id <= incoming_ack_id {
            // nothink to acknowledge
            return;
        }

        if has_timeout {
            // there will be a check in the near future
            return;
        }

        let time_since_last_incoming_msg = now() - last_time;
        if time_since_last_incoming_msg >= protocol_constants::ACKNOWLEDGE_TIME {
            // sufficient time has passed since this message has been received, and no message from our side needed
            // to be sent in the meantime, so we will send a message containing only an ack.
            self.send_ack();
            return;
        }

        let weak = self.me.clone();
        let delay = (protocol_constants::ACKNOWLEDGE_TIME - time_since_last_incoming_msg + 5.0).max(0.0) as u64;
        let timer = runtime::set_timeout(
            move || {
                if let Some(protocol) = weak.upgrade() {
                    protocol.state.borrow_mut().incoming_ack_timeout = None;
                    protocol.send_ack_check();
                }
            },
            delay,
        );
        self.state.borrow_mut().incoming_ack_timeout = Some(timer);
    }

    fn recv_ack_check(&self) {
        let (outgoing_msg_id, outgoing_ack_id, has_timeout, is_reconnecting) = {
            let state = self.state.borrow();
            (state.outgoing_msg_id, state.outgoing_ack_id, state.outgoing_ack_timeout.is_some(), state.is_reconnecting)
        };
        if outgoing_msg_id <= outgoing_ack_id {
            // everything has been acknowledged
            return;
        }

        if has_timeout {
            // there will be a check in the near future
            return;
        }

        if is_reconnecting {
            // do not cause a timeout during reconnection, because messages will not be actually written until
            // `endAcceptReconnection`
            return;
        }

        let oldest_unacknowledged_msg = self.state.borrow().outgoing_unack_msg.front().cloned().expect("há mensagem sem confirmação");
        let now = now();
        let time_since_oldest_unacknowledged_msg = now - oldest_unacknowledged_msg.written_time.get();
        let time_since_last_received_some_data = now - self.reader().last_read_time();
        let time_since_last_timeout = now - self.state.borrow().last_socket_timeout_time;

        if time_since_oldest_unacknowledged_msg >= protocol_constants::TIMEOUT_TIME
            && time_since_last_received_some_data >= protocol_constants::TIMEOUT_TIME
            && time_since_last_timeout >= protocol_constants::TIMEOUT_TIME
        {
            // It's been a long time since our sent message was acknowledged and a long time since we received some
            // data

            // But this might be caused by the event loop being busy and failing to read messages
            if !self.load_estimator.has_high_load() {
                // Trash the socket
                let unacknowledged_msg_count = self.state.borrow().outgoing_unack_msg.len();
                self.state.borrow_mut().last_socket_timeout_time = now;
                self.on_socket_timeout.fire(SocketTimeoutEvent {
                    reason: SocketTimeoutReason::UnacknowledgedMessage,
                    unacknowledged_msg_count,
                    time_since_oldest_unacknowledged_msg: Some(time_since_oldest_unacknowledged_msg),
                    time_since_last_received_some_data,
                });
                return;
            }
        }

        let minimum_time_until_timeout = (protocol_constants::TIMEOUT_TIME - time_since_oldest_unacknowledged_msg)
            .max(protocol_constants::TIMEOUT_TIME - time_since_last_received_some_data)
            .max(protocol_constants::TIMEOUT_TIME - time_since_last_timeout)
            .max(500.0);

        let weak = self.me.clone();
        let timer = runtime::set_timeout(
            move || {
                if let Some(protocol) = weak.upgrade() {
                    protocol.state.borrow_mut().outgoing_ack_timeout = None;
                    protocol.recv_ack_check();
                }
            },
            minimum_time_until_timeout as u64,
        );
        self.state.borrow_mut().outgoing_ack_timeout = Some(timer);
    }

    /// Called after sending a keepalive. Both sides of this protocol send keepalives every KeepAliveSendTime (5s),
    /// so receiving no data for TimeoutTime (20s) means the connection is dead. This catches silent connection
    /// deaths that `recv_ack_check` cannot detect because there are no unacknowledged regular messages.
    fn keep_alive_timeout_check(&self) {
        if self.state.borrow().is_reconnecting {
            return;
        }

        let now = now();
        let time_since_last_received_some_data = now - self.reader().last_read_time();
        let time_since_last_timeout = now - self.state.borrow().last_socket_timeout_time;

        if time_since_last_received_some_data >= protocol_constants::TIMEOUT_TIME && time_since_last_timeout >= protocol_constants::TIMEOUT_TIME {
            // But this might be caused by the event loop being busy and failing to read messages
            if !self.load_estimator.has_high_load() {
                let (unacknowledged_msg_count, oldest) = {
                    let mut state = self.state.borrow_mut();
                    state.last_socket_timeout_time = now;
                    (state.outgoing_unack_msg.len(), state.outgoing_unack_msg.front().cloned())
                };
                self.on_socket_timeout.fire(SocketTimeoutEvent {
                    reason: SocketTimeoutReason::KeepAlive,
                    unacknowledged_msg_count,
                    time_since_oldest_unacknowledged_msg: oldest.map(|msg| now - msg.written_time.get()),
                    time_since_last_received_some_data,
                });
            }
        }
    }

    fn send_ack(&self) {
        let ack_id = {
            let mut state = self.state.borrow_mut();
            if state.incoming_msg_id <= state.incoming_ack_id {
                // nothink to acknowledge
                return;
            }
            state.incoming_ack_id = state.incoming_msg_id;
            state.incoming_ack_id
        };
        self.writer().write(&ProtocolMessage::new(ProtocolMessageType::Ack, 0, ack_id, VSBuffer::alloc(0)));
    }

    fn send_keep_alive(&self) {
        let ack_id = {
            let mut state = self.state.borrow_mut();
            state.incoming_ack_id = state.incoming_msg_id;
            state.incoming_ack_id
        };
        self.writer().write(&ProtocolMessage::new(ProtocolMessageType::KeepAlive, 0, ack_id, VSBuffer::alloc(0)));
        self.keep_alive_timeout_check();
    }
}

impl IMessagePassingProtocol for PersistentProtocol {
    fn send(&self, buffer: VSBuffer) {
        let (msg, is_reconnecting) = {
            let mut state = self.state.borrow_mut();
            state.outgoing_msg_id += 1;
            let my_id = state.outgoing_msg_id;
            state.incoming_ack_id = state.incoming_msg_id;
            let msg = ProtocolMessage::new(ProtocolMessageType::Regular, my_id, state.incoming_ack_id, buffer);
            state.outgoing_unack_msg.push_back(msg.clone());
            (msg, state.is_reconnecting)
        };
        if !is_reconnecting {
            self.writer().write(&msg);
            self.recv_ack_check();
        }
    }

    fn on_message(&self) -> Event<VSBuffer> {
        self.on_message.event()
    }

    fn drain(&self) -> Option<Promise<()>> {
        Some(self.writer().drain())
    }
}

impl INetProtocol for PersistentProtocol {
    fn get_socket(&self) -> Rc<dyn ISocket> {
        PersistentProtocol::get_socket(self)
    }

    fn send_disconnect(&self) {
        PersistentProtocol::send_disconnect(self);
    }

    fn dispose(&self) {
        PersistentProtocol::dispose(self);
    }

    fn on_did_dispose(&self) -> Event<()> {
        PersistentProtocol::on_did_dispose(self)
    }
}
