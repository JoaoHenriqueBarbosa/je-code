//! Porte de `src/vs/base/parts/ipc/common/ipc.mp.ts` (o `Protocol` sobre uma `MessagePort`), mais o `MessageChannel` em memória
//! que o navegador oferece e o `ipc.mp.ts` pressupõe.
//!
//! Divergências:
//! - **`MessagePort` do DOM** é o trait [`MessagePort`] (`addEventListener`/`removeEventListener` vêm do [`DomEventEmitter`], que o
//!   `Event.fromDOMEventEmitter` já usa). O `MessageEvent.data` é `Uint8Array | undefined` e vira `Option<Vec<u8>>`.
//! - **`MessageChannel`**: o par de pontas ([`MessageChannel::new`]) é a implementação em memória do `new MessageChannel()` do
//!   navegador. Como no DOM, uma ponta só entrega mensagens depois de `start()` (as que chegam antes ficam na fila) e a entrega é
//!   assíncrona (uma microtarefa, nunca de dentro do `postMessage`). `close()` descarta a fila e deixa de entregar nos dois sentidos.
//! - `Protocol.disconnect()` fecha a porta.
//! - O `IPCClient` sobre `MessagePort` (`Client`) não foi portado: nada o lê.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use crate::buffer::VSBuffer;
use crate::event::{DomEventEmitter, Event, Listener, from_dom_event_emitter};
use crate::parts::ipc::IMessagePassingProtocol;
use crate::runtime;

/// `MessageEvent`: `data` é o `Uint8Array` do original (pode faltar, por exemplo no desmonte da conexão).
#[derive(Clone, Debug, Default)]
pub struct MessageEvent {
    pub data: Option<Vec<u8>>,
}

/// `MessagePort`.
pub trait MessagePort: DomEventEmitter<MessageEvent> {
    fn post_message(&self, message: Vec<u8>);
    fn start(&self);
    fn close(&self);
}

/// Uma ponta de um [`MessageChannel`] em memória.
pub struct InMemoryMessagePort {
    me: Weak<InMemoryMessagePort>,
    peer: RefCell<Weak<InMemoryMessagePort>>,
    listeners: RefCell<Vec<Listener<MessageEvent>>>,
    inbox: RefCell<VecDeque<MessageEvent>>,
    started: Cell<bool>,
    closed: Cell<bool>,
    flush_scheduled: Cell<bool>,
}

impl InMemoryMessagePort {
    fn new() -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            peer: RefCell::new(Weak::new()),
            listeners: RefCell::new(Vec::new()),
            inbox: RefCell::new(VecDeque::new()),
            started: Cell::new(false),
            closed: Cell::new(false),
            flush_scheduled: Cell::new(false),
        })
    }

    fn receive(&self, event: MessageEvent) {
        if self.closed.get() {
            return;
        }
        self.inbox.borrow_mut().push_back(event);
        self.schedule_flush();
    }

    fn schedule_flush(&self) {
        if !self.started.get() || self.flush_scheduled.get() || self.inbox.borrow().is_empty() {
            return;
        }
        self.flush_scheduled.set(true);
        let me = self.me.clone();
        runtime::queue_microtask(move || {
            if let Some(port) = me.upgrade() {
                port.flush();
            }
        });
    }

    fn flush(&self) {
        self.flush_scheduled.set(false);
        loop {
            if self.closed.get() || !self.started.get() {
                return;
            }
            let Some(event) = self.inbox.borrow_mut().pop_front() else { return };
            let listeners: Vec<Listener<MessageEvent>> = self.listeners.borrow().clone();
            for listener in listeners {
                listener(&event);
            }
        }
    }
}

impl DomEventEmitter<MessageEvent> for InMemoryMessagePort {
    fn add_event_listener(&self, event: &str, listener: Listener<MessageEvent>) {
        if event == "message" {
            self.listeners.borrow_mut().push(listener);
        }
    }

    fn remove_event_listener(&self, event: &str, listener: &Listener<MessageEvent>) {
        if event == "message" {
            self.listeners.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, listener));
        }
    }
}

impl MessagePort for InMemoryMessagePort {
    fn post_message(&self, message: Vec<u8>) {
        if self.closed.get() {
            return;
        }
        if let Some(peer) = self.peer.borrow().upgrade() {
            peer.receive(MessageEvent { data: Some(message) });
        }
    }

    fn start(&self) {
        self.started.set(true);
        self.schedule_flush();
    }

    fn close(&self) {
        self.closed.set(true);
        self.inbox.borrow_mut().clear();
        if let Some(peer) = self.peer.borrow().upgrade() {
            peer.closed.set(true);
            peer.inbox.borrow_mut().clear();
        }
    }
}

/// `new MessageChannel()`: duas pontas ligadas uma à outra.
pub struct MessageChannel {
    pub port1: Rc<InMemoryMessagePort>,
    pub port2: Rc<InMemoryMessagePort>,
}

impl MessageChannel {
    pub fn new() -> Self {
        let port1 = InMemoryMessagePort::new();
        let port2 = InMemoryMessagePort::new();
        *port1.peer.borrow_mut() = Rc::downgrade(&port2);
        *port2.peer.borrow_mut() = Rc::downgrade(&port1);
        Self { port1, port2 }
    }
}

impl Default for MessageChannel {
    fn default() -> Self {
        Self::new()
    }
}

/// O `Protocol` do `ipc.mp.ts`: o `IMessagePassingProtocol` sobre uma `MessagePort`.
pub struct Protocol {
    port: Rc<dyn MessagePort>,
    on_message: Event<VSBuffer>,
}

impl Protocol {
    pub fn new(port: Rc<dyn MessagePort>) -> Rc<Self> {
        // A `message` event may carry no (or empty) data (e.g. during connection teardown). An empty frame is never a valid
        // protocol message, so it must not be forwarded: filter these out at the transport boundary.
        let emitter: Rc<dyn DomEventEmitter<MessageEvent>> = Rc::new(PortEmitter(port.clone()));
        let on_message = from_dom_event_emitter(emitter, "message", |event: &MessageEvent| match &event.data {
            Some(data) => VSBuffer::wrap(data.clone()),
            None => VSBuffer::alloc(0),
        })
        .filter(|data| data.byte_length() > 0, None);
        // we must call start() to ensure messages are flowing
        port.start();
        Rc::new(Self { port, on_message })
    }

    pub fn disconnect(&self) {
        self.port.close();
    }
}

impl IMessagePassingProtocol for Protocol {
    fn send(&self, message: VSBuffer) {
        self.port.post_message(message.to_vec());
    }

    fn on_message(&self) -> Event<VSBuffer> {
        self.on_message.clone()
    }
}

/// Um `Rc<dyn MessagePort>` visto como [`DomEventEmitter`] (o upcast de trait object não cobre o supertrait genérico aqui).
struct PortEmitter(Rc<dyn MessagePort>);

impl DomEventEmitter<MessageEvent> for PortEmitter {
    fn add_event_listener(&self, event: &str, listener: Listener<MessageEvent>) {
        self.0.add_event_listener(event, listener);
    }

    fn remove_event_listener(&self, event: &str, listener: &Listener<MessageEvent>) {
        self.0.remove_event_listener(event, listener);
    }
}
