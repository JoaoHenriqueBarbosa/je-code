//! Porte de `src/vs/base/parts/ipc/test/node/ipc.net.test.ts` (suítes `IPC, Socket Protocol` e
//! `PersistentProtocol reconnection`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O `net.Socket` do Node é o `INodeStream`; o `EtherStream` e o `Ether` do original (dois soquetes ligados por
//!   um fio com latência) são reescritos sobre os emissores do porte, e o `EventEmitter` simples dos testes
//!   `issue #211462` e `dispose(false)` é o `SimpleStream`. `listenerCount` é `Emitter::has_listeners`.
//! - `runWithFakedTimers` é `run_with_faked_timers`; o `sinon.useFakeTimers()` do `#211462` é o relógio virtual
//!   do `runtime`, e `timers.tick(n)` é `advance_fake_time(n)`.
//! - `MessageStream` guarda as mensagens e entrega uma por `waitForOne()`.
//!
//! Pendentes:
//! - `IPC, create handle` (`createRandomIPCHandle` e `createStaticIPCHandle`): abrem um servidor `net` de verdade
//!   num socket de domínio Unix; o porte não tem sockets reais (ficam na camada de plataforma). As duas funções
//!   são conferidas em `ipc_net_extras.rs`.
//! - `WebSocketNodeSocket` (todos os casos, inclusive os de compressão): depende do `zlib` do Node (`DeflateRaw` e
//!   `InflateRaw` com `Z_SYNC_FLUSH`) e não foi portado.

mod common;

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::buffer::VSBuffer;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable, mark_as_singleton};
use je_base::parts::ipc::net::{ILoadEstimator, ISocket, PersistentProtocol, PersistentProtocolOptions, Protocol, SocketTimeoutReason, protocol_constants};
use je_base::parts::ipc::node_net::{INodeStream, NodeSocket};
use je_base::parts::ipc::{IMessagePassingProtocol, net::INetProtocol};
use je_base::promise::{Promise, Resolver};
use je_base::runtime;

/// `MessageStream`.
struct MessageStream {
    current_complete: RefCell<Option<Resolver<VSBuffer>>>,
    messages: RefCell<VecDeque<VSBuffer>>,
    listener: RefCell<Option<DisposableRef>>,
}

impl MessageStream {
    fn new(on_message: Event<VSBuffer>) -> Rc<Self> {
        let this = Rc::new(Self { current_complete: RefCell::default(), messages: RefCell::default(), listener: RefCell::default() });
        let weak = Rc::downgrade(&this);
        *this.listener.borrow_mut() = Some(on_message.listen(move |data| {
            if let Some(stream) = weak.upgrade() {
                stream.messages.borrow_mut().push_back(data.clone());
                stream.trigger();
            }
        }));
        this
    }

    fn trigger(&self) {
        if self.current_complete.borrow().is_none() {
            return;
        }
        let msg = self.messages.borrow_mut().pop_front();
        if let Some(msg) = msg {
            let complete = self.current_complete.borrow_mut().take().expect("conferido acima");
            complete.resolve(msg);
        }
    }

    fn wait_for_one(&self) -> Promise<VSBuffer> {
        Promise::with_executor(|complete| {
            *self.current_complete.borrow_mut() = Some(complete);
            self.trigger();
        })
    }

    fn dispose(&self) {
        if let Some(listener) = self.listener.borrow_mut().take() {
            listener.dispose();
        }
    }
}

/// Os ouvintes de um `EventEmitter` do Node não são `IDisposable` rastreados; o `Emitter` do porte cria um
/// disposable rastreado por ouvinte, então o evento do fluxo falso marca cada um como singleton (fora do
/// relatório de vazamento), como se o ouvinte fosse do `EventEmitter`.
fn untracked<T: Clone + 'static>(emitter: &Rc<Emitter<T>>) -> Event<T> {
    let emitter = emitter.clone();
    Event::new(move |listener| mark_as_singleton(emitter.event().listen_rc(listener)))
}

/// O `EventEmitter` mínimo que faz de `net.Socket` nos testes; `destroy` pode emitir `close`.
struct SimpleStream {
    data: Rc<Emitter<VSBuffer>>,
    close: Rc<Emitter<bool>>,
    end: Rc<Emitter<()>>,
    error: Rc<Emitter<Error>>,
    timeout: Rc<Emitter<()>>,
    drain: Rc<Emitter<()>>,
    destroyed: Cell<bool>,
    destroy_emits_close: bool,
}

impl SimpleStream {
    fn new(destroy_emits_close: bool) -> Rc<Self> {
        Rc::new(Self {
            data: Emitter::new(),
            close: Emitter::new(),
            end: Emitter::new(),
            error: Emitter::new(),
            timeout: Emitter::new(),
            drain: Emitter::new(),
            destroyed: Cell::new(false),
            destroy_emits_close,
        })
    }
}

impl INodeStream for SimpleStream {
    fn on_data(&self) -> Event<VSBuffer> {
        untracked(&self.data)
    }

    fn on_close(&self) -> Event<bool> {
        untracked(&self.close)
    }

    fn on_end(&self) -> Event<()> {
        untracked(&self.end)
    }

    fn on_error(&self) -> Event<Error> {
        untracked(&self.error)
    }

    fn on_timeout(&self) -> Event<()> {
        untracked(&self.timeout)
    }

    fn on_drain(&self) -> Event<()> {
        untracked(&self.drain)
    }

    fn write(&self, _buffer: VSBuffer) -> Result<(), Error> {
        Ok(())
    }

    fn end(&self) {}

    fn destroy(&self) {
        self.destroyed.set(true);
        if self.destroy_emits_close {
            self.close.fire(false);
        }
    }

    fn destroyed(&self) -> bool {
        self.destroyed.get()
    }

    fn buffer_size(&self) -> usize {
        0
    }
}

impl IDisposable for SimpleStream {
    fn dispose(&self) {
        self.data.dispose();
        self.close.dispose();
        self.end.dispose();
        self.error.dispose();
        self.timeout.dispose();
        self.drain.dispose();
    }
}

/// `Ether`: dois lados ligados por um fio.
struct Ether {
    wire_latency: u64,
    a: Rc<EtherStream>,
    b: Rc<EtherStream>,
    ab: RefCell<Vec<VSBuffer>>,
    ba: RefCell<Vec<VSBuffer>>,
}

/// `EtherStream`: um `net.Socket` falso preso a um lado do fio.
struct EtherStream {
    simple: Rc<SimpleStream>,
    ether: RefCell<std::rc::Weak<Ether>>,
    name: char,
}

impl INodeStream for EtherStream {
    fn on_data(&self) -> Event<VSBuffer> {
        self.simple.on_data()
    }

    fn on_close(&self) -> Event<bool> {
        self.simple.on_close()
    }

    fn on_end(&self) -> Event<()> {
        self.simple.on_end()
    }

    fn on_error(&self) -> Event<Error> {
        self.simple.on_error()
    }

    fn on_timeout(&self) -> Event<()> {
        self.simple.on_timeout()
    }

    fn on_drain(&self) -> Event<()> {
        self.simple.on_drain()
    }

    fn write(&self, data: VSBuffer) -> Result<(), Error> {
        let ether = self.ether.borrow().upgrade().expect("o fio está vivo");
        Ether::write(&ether, self.name, data);
        Ok(())
    }

    fn end(&self) {}

    fn destroy(&self) {}

    fn destroyed(&self) -> bool {
        false
    }

    fn buffer_size(&self) -> usize {
        0
    }
}

impl Ether {
    fn new(wire_latency: u64) -> Rc<Self> {
        let make = |name| Rc::new(EtherStream { simple: SimpleStream::new(false), ether: RefCell::default(), name });
        let ether = Rc::new(Self { wire_latency, a: make('a'), b: make('b'), ab: RefCell::default(), ba: RefCell::default() });
        *ether.a.ether.borrow_mut() = Rc::downgrade(&ether);
        *ether.b.ether.borrow_mut() = Rc::downgrade(&ether);
        ether
    }

    fn a(&self) -> Rc<dyn INodeStream> {
        self.a.clone()
    }

    fn b(&self) -> Rc<dyn INodeStream> {
        self.b.clone()
    }

    fn write(this: &Rc<Self>, from: char, data: VSBuffer) {
        let ether = this.clone();
        runtime::set_timeout(
            move || {
                if from == 'a' {
                    ether.ab.borrow_mut().push(data);
                } else {
                    ether.ba.borrow_mut().push(data);
                }

                let ether = ether.clone();
                runtime::set_timeout(move || Ether::deliver(&ether), 0);
            },
            this.wire_latency,
        );
    }

    fn deliver(this: &Rc<Self>) {
        let next = |queue: &RefCell<Vec<VSBuffer>>| {
            let mut chunks = queue.borrow_mut();
            if chunks.is_empty() { None } else { Some(VSBuffer::concat(&std::mem::take(&mut *chunks), None)) }
        };
        if let Some(data) = next(&this.ab) {
            this.b.simple.data.fire(data);
            let ether = this.clone();
            runtime::set_timeout(move || Ether::deliver(&ether), 0);
            return;
        }

        if let Some(data) = next(&this.ba) {
            this.a.simple.data.fire(data);
            let ether = this.clone();
            runtime::set_timeout(move || Ether::deliver(&ether), 0);
        }
    }
}

fn s(text: &str) -> VSBuffer {
    VSBuffer::from_string(text)
}

fn string_of(buffer: &VSBuffer) -> String {
    String::from_utf8(buffer.to_vec()).unwrap()
}

mod socket_protocol {
    use super::*;

    #[test]
    fn read_write() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let ether = Ether::new(0);
            let a = Protocol::new(NodeSocket::new(ether.a(), ""));
            let b = Protocol::new(NodeSocket::new(ether.b(), ""));
            let b_messages = MessageStream::new(b.on_message());

            a.send(s("foobarfarboo"));
            let msg1 = b_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&msg1), "foobarfarboo");

            let buffer = VSBuffer::alloc(1);
            buffer.write_u_int8(123, 0);
            a.send(buffer);
            let msg2 = b_messages.wait_for_one().await.unwrap();
            assert_eq!(msg2.read_u_int8(0), 123);

            b_messages.dispose();
            INetProtocol::dispose(&*a);
            INetProtocol::dispose(&*b);
        });
    }

    #[test]
    fn read_write_object_data() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let ether = Ether::new(0);
            let a = Protocol::new(NodeSocket::new(ether.a(), ""));
            let b = Protocol::new(NodeSocket::new(ether.b(), ""));
            let b_messages = MessageStream::new(b.on_message());

            let data = r#"{"pi":3.141592653589793,"foo":"bar","more":true,"data":["H","e","l","l","o"," ","W","o","r","l","d"]}"#;

            a.send(s(data));
            let msg = b_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&msg), data);

            b_messages.dispose();
            INetProtocol::dispose(&*a);
            INetProtocol::dispose(&*b);
        });
    }

    #[test]
    fn issue_211462_destroy_socket_after_end_timeout() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        runtime::with_faked_timers(|| {
            let stream = SimpleStream::new(true);
            let node_socket = NodeSocket::new(stream.clone(), "");
            let protocol = Protocol::new(node_socket.clone());

            let disposed = Rc::new(Cell::new(false));
            let flag = disposed.clone();
            ds.add(protocol.on_did_dispose().listen(move |()| flag.set(true)));

            stream.end.fire(());
            assert!(!disposed.get());
            runtime::advance_fake_time(29_999);
            assert!(!disposed.get());
            runtime::advance_fake_time(1);
            assert!(disposed.get());

            INetProtocol::dispose(&*protocol);
            node_socket.dispose();
            stream.dispose();
        });
    }

    #[test]
    fn dispose_false_detaches_listeners_without_destroying_the_socket() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let stream = SimpleStream::new(false);

        let node_socket = NodeSocket::new(stream.clone(), "");
        node_socket.dispose_with(false);

        assert_eq!(
            (stream.destroyed(), stream.error.has_listeners(), stream.close.has_listeners(), stream.end.has_listeners()),
            (false, false, false, false)
        );
        stream.dispose();
    }
}

struct NoLoad;

impl ILoadEstimator for NoLoad {
    fn has_high_load(&self) -> bool {
        false
    }
}

fn load_estimator() -> Option<Rc<dyn ILoadEstimator>> {
    Some(Rc::new(NoLoad))
}

fn persistent(socket: Rc<dyn ISocket>, load: Option<Rc<dyn ILoadEstimator>>, send_keep_alive: Option<bool>) -> Rc<PersistentProtocol> {
    PersistentProtocol::new(PersistentProtocolOptions { socket, initial_chunk: None, load_estimator: load, send_keep_alive })
}

mod persistent_protocol_reconnection {
    use super::*;

    fn dispose_all(streams: &[&Rc<MessageStream>], protocols: &[&Rc<PersistentProtocol>]) {
        for stream in streams {
            stream.dispose();
        }
        for protocol in protocols {
            protocol.dispose();
        }
    }

    #[test]
    fn acks_get_piggybacked_with_messages() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let ether = Ether::new(0);
            let a = persistent(NodeSocket::new(ether.a(), ""), load_estimator(), None);
            let a_messages = MessageStream::new(a.on_message());
            let b = persistent(NodeSocket::new(ether.b(), ""), load_estimator(), None);
            let b_messages = MessageStream::new(b.on_message());

            a.send(s("a1"));
            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 0);

            a.send(s("a2"));
            assert_eq!(a.unacknowledged_count(), 2);
            assert_eq!(b.unacknowledged_count(), 0);

            a.send(s("a3"));
            assert_eq!(a.unacknowledged_count(), 3);
            assert_eq!(b.unacknowledged_count(), 0);

            let a1 = b_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&a1), "a1");
            assert_eq!(a.unacknowledged_count(), 3);
            assert_eq!(b.unacknowledged_count(), 0);

            let a2 = b_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&a2), "a2");
            assert_eq!(a.unacknowledged_count(), 3);
            assert_eq!(b.unacknowledged_count(), 0);

            let a3 = b_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&a3), "a3");
            assert_eq!(a.unacknowledged_count(), 3);
            assert_eq!(b.unacknowledged_count(), 0);

            b.send(s("b1"));
            assert_eq!(a.unacknowledged_count(), 3);
            assert_eq!(b.unacknowledged_count(), 1);

            let b1 = a_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&b1), "b1");
            assert_eq!(a.unacknowledged_count(), 0);
            assert_eq!(b.unacknowledged_count(), 1);

            a.send(s("a4"));
            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 1);

            let b2 = b_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&b2), "a4");
            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 0);

            dispose_all(&[&a_messages, &b_messages], &[&a, &b]);
        });
    }

    #[test]
    fn ack_gets_sent_after_a_while() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let ether = Ether::new(0);
            let a = persistent(NodeSocket::new(ether.a(), ""), load_estimator(), None);
            let a_messages = MessageStream::new(a.on_message());
            let b = persistent(NodeSocket::new(ether.b(), ""), load_estimator(), None);
            let b_messages = MessageStream::new(b.on_message());

            // send one message A -> B
            a.send(s("a1"));
            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 0);
            let a1 = b_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&a1), "a1");
            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 0);

            // wait for ack to arrive B -> A
            timeout(2 * protocol_constants::ACKNOWLEDGE_TIME as u64).await.unwrap();
            assert_eq!(a.unacknowledged_count(), 0);
            assert_eq!(b.unacknowledged_count(), 0);

            dispose_all(&[&a_messages, &b_messages], &[&a, &b]);
        });
    }

    #[test]
    fn messages_that_are_never_written_to_a_socket_should_not_cause_an_ack_timeout() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            // Date.now() in fake timers starts at 0, which is very inconvenient since we want to test exactly that a
            // certain field is not initialized with Date.now(). As a workaround we wait such that Date.now() starts
            // producing more realistic values
            timeout(60 * 60 * 1000).await.unwrap();

            let ether = Ether::new(0);
            let a_socket = NodeSocket::new(ether.a(), "");
            let a = persistent(a_socket.clone(), load_estimator(), Some(false));
            let a_messages = MessageStream::new(a.on_message());
            let b_socket = NodeSocket::new(ether.b(), "");
            let b = persistent(b_socket, load_estimator(), Some(false));
            let b_messages = MessageStream::new(b.on_message());

            // send message a1 before reconnection to get _recvAckCheck() scheduled
            a.send(s("a1"));
            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 0);

            // read message a1 at B
            let a1 = b_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&a1), "a1");
            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 0);

            // send message b1 to send the ack for a1
            b.send(s("b1"));
            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 1);

            // read message b1 at A to receive the ack for a1
            let b1 = a_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&b1), "b1");
            assert_eq!(a.unacknowledged_count(), 0);
            assert_eq!(b.unacknowledged_count(), 1);

            // begin reconnection
            a_socket.dispose();
            let a_socket2 = NodeSocket::new(ether.a(), "");
            a.begin_accept_reconnection(a_socket2, None);

            let timeout_listener_called = Rc::new(Cell::new(false));
            let flag = timeout_listener_called.clone();
            let socket_timeout_listener = a.on_socket_timeout().listen(move |_| flag.set(true));

            // send message 2 during reconnection
            a.send(s("a2"));
            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 1);

            // wait for scheduled _recvAckCheck() to execute
            timeout(2 * protocol_constants::TIMEOUT_TIME as u64).await.unwrap();

            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 1);
            assert!(!timeout_listener_called.get());

            a.end_accept_reconnection();
            assert!(!timeout_listener_called.get());

            timeout(2 * protocol_constants::TIMEOUT_TIME as u64).await.unwrap();
            assert_eq!(a.unacknowledged_count(), 0);
            assert_eq!(b.unacknowledged_count(), 0);
            assert!(!timeout_listener_called.get());

            socket_timeout_listener.dispose();
            dispose_all(&[&a_messages, &b_messages], &[&a, &b]);
        });
    }

    #[test]
    fn acks_are_always_sent_after_a_reconnection() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let wire_latency = 1000;
            let ether = Ether::new(wire_latency);
            let a_socket = NodeSocket::new(ether.a(), "");
            let a = persistent(a_socket.clone(), load_estimator(), None);
            let a_messages = MessageStream::new(a.on_message());
            let b_socket = NodeSocket::new(ether.b(), "");
            let b = persistent(b_socket.clone(), load_estimator(), None);
            let b_messages = MessageStream::new(b.on_message());

            // send message a1 to have something unacknowledged
            a.send(s("a1"));
            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 0);

            // read message a1 at B
            let a1 = b_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&a1), "a1");
            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 0);

            // wait for B to send an ACK message, but resume before A receives it
            timeout(protocol_constants::ACKNOWLEDGE_TIME as u64 + wire_latency / 2).await.unwrap();
            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 0);

            // simulate complete reconnection
            a_socket.dispose();
            b_socket.dispose();
            let ether2 = Ether::new(wire_latency);
            let a_socket2 = NodeSocket::new(ether2.a(), "");
            let b_socket2 = NodeSocket::new(ether2.b(), "");
            b.begin_accept_reconnection(b_socket2, None);
            b.end_accept_reconnection();
            a.begin_accept_reconnection(a_socket2, None);
            a.end_accept_reconnection();

            // wait for quite some time
            timeout(2 * protocol_constants::ACKNOWLEDGE_TIME as u64 + wire_latency).await.unwrap();
            assert_eq!(a.unacknowledged_count(), 0);
            assert_eq!(b.unacknowledged_count(), 0);

            dispose_all(&[&a_messages, &b_messages], &[&a, &b]);
        });
    }

    #[test]
    fn on_socket_timeout_is_emitted_at_most_once_every_20s() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let ether = Ether::new(0);
            let a = persistent(NodeSocket::new(ether.a(), ""), load_estimator(), None);
            let a_messages = MessageStream::new(a.on_message());
            let b = persistent(NodeSocket::new(ether.b(), ""), load_estimator(), None);
            let b_messages = MessageStream::new(b.on_message());

            // never receive acks
            b.pause_socket_writing();

            // send message a1 to have something unacknowledged
            a.send(s("a1"));

            // wait for the first timeout to fire
            a.on_socket_timeout().to_promise(None).await.unwrap();

            let timeout_fired_again = Rc::new(Cell::new(false));
            let flag = timeout_fired_again.clone();
            let timeout_listener = a.on_socket_timeout().listen(move |_| flag.set(true));

            // send more messages
            a.send(s("a2"));
            a.send(s("a3"));

            // wait for 10s
            timeout(protocol_constants::TIMEOUT_TIME as u64 / 2).await.unwrap();

            assert!(!timeout_fired_again.get());

            timeout_listener.dispose();
            dispose_all(&[&a_messages, &b_messages], &[&a, &b]);
        });
    }

    #[test]
    fn keepalive_detects_dead_connection_when_no_regular_messages_are_pending() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let ether = Ether::new(0);
            let a = persistent(NodeSocket::new(ether.a(), ""), load_estimator(), None);
            let a_messages = MessageStream::new(a.on_message());
            let b = persistent(NodeSocket::new(ether.b(), ""), load_estimator(), None);
            let b_messages = MessageStream::new(b.on_message());

            // exchange a message so both sides are in a good state
            a.send(s("a1"));
            let a1 = b_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&a1), "a1");

            // wait for ack to arrive
            timeout(protocol_constants::ACKNOWLEDGE_TIME as u64 * 2).await.unwrap();

            // confirm no unacknowledged messages
            assert_eq!(a.unacknowledged_count(), 0);

            // now kill b's ability to send anything (simulates a dead connection where the remote side's keepalives
            // stop arriving)
            b.pause_socket_writing();

            // wait for timeout to be detected via keepalive
            let socket_timeout_event = a.on_socket_timeout().to_promise(None).await.unwrap();

            assert_eq!(socket_timeout_event.reason, SocketTimeoutReason::KeepAlive);
            assert!(socket_timeout_event.time_since_last_received_some_data >= protocol_constants::TIMEOUT_TIME);
            // no regular messages were pending
            assert_eq!(socket_timeout_event.unacknowledged_msg_count, 0);
            assert_eq!(socket_timeout_event.time_since_oldest_unacknowledged_msg, None);

            dispose_all(&[&a_messages, &b_messages], &[&a, &b]);
        });
    }

    #[test]
    fn writing_can_be_paused() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        run_with_faked_timers(async move {
            let ether = Ether::new(0);
            let a = persistent(NodeSocket::new(ether.a(), ""), load_estimator(), None);
            let a_messages = MessageStream::new(a.on_message());
            let b = persistent(NodeSocket::new(ether.b(), ""), load_estimator(), None);
            let b_messages = MessageStream::new(b.on_message());

            // send one message A -> B
            a.send(s("a1"));
            let a1 = b_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&a1), "a1");

            // ask A to pause writing
            b.send_pause();

            // send a message B -> A
            b.send(s("b1"));
            let b1 = a_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&b1), "b1");

            // send a message A -> B (this should be blocked at A)
            a.send(s("a2"));

            // wait a long time and check that not even acks are written
            timeout(2 * protocol_constants::ACKNOWLEDGE_TIME as u64).await.unwrap();
            assert_eq!(a.unacknowledged_count(), 1);
            assert_eq!(b.unacknowledged_count(), 1);

            // ask A to resume writing
            b.send_resume();

            // check that B receives message
            let a2 = b_messages.wait_for_one().await.unwrap();
            assert_eq!(string_of(&a2), "a2");

            // wait a long time and check that acks are written
            timeout(2 * protocol_constants::ACKNOWLEDGE_TIME as u64).await.unwrap();
            assert_eq!(a.unacknowledged_count(), 0);
            assert_eq!(b.unacknowledged_count(), 0);

            dispose_all(&[&a_messages, &b_messages], &[&a, &b]);
        });
    }
}
