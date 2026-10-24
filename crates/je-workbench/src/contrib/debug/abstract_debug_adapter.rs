//! Porte de `src/vs/workbench/contrib/debug/common/abstractDebugAdapter.ts`: a implementação abstrata da API de baixo
//! nível de um adaptador de depuração. Falta só como essa API se comunica com o adaptador.
//!
//! Notas de tradução:
//! - A classe abstrata é o trait [`AbstractDebugAdapter`]: os métodos abstratos (`startSession`, `stopSession`,
//!   `sendMessage`) são os obrigatórios e o estado (`sequence`, os pedidos pendentes, os callbacks, a fila e os dois
//!   emissores) é o [`DebugAdapterState`], entregue por `state()`. O `dispose` das subclasses chama o
//!   [`DebugAdapterState::dispose`], que é o `super.dispose()`.
//! - `processQueue` e `cancelPendingRequests` são `async` no original: aqui são tarefas locais do runtime
//!   (`spawn_local`), cada `await` cede para uma microtarefa como no JS. `acceptMessage` recebe `Rc<Self>` porque a
//!   tarefa de `processQueue` mantém o adaptador vivo enquanto a fila não esvazia.
//! - Os callbacks (`DebugProtocol.Response`, `Request`, `Event`) recebem a mensagem por referência. Os pedidos pendentes
//!   ficam num `IndexMap` porque o `Map` do JS itera na ordem de inserção.
//! - `sendRequest(command, args: any, ...)` recebe `args` como `Option<&Value>`; `args && Object.keys(args).length > 0`
//!   vale para objeto, lista e texto não vazios (os demais valores não têm chaves).
//! - `sendResponse(response)` e o `internalSend` mutam a mensagem do chamador (`seq`) no original; o `sendResponse` aqui
//!   recebe `&mut Response` e o `internalSend` recebe `&mut ProtocolMessage`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use indexmap::IndexMap;
use je_base::async_::timeout;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::runtime::{self, TimerId};
use serde_json::Value;

use super::debug_protocol::{Event as ProtocolEvent, ProtocolMessage, Request, Response};

/// `(e: DebugProtocol.Response) => void`.
pub type ResponseCallback = Rc<dyn Fn(&Response)>;

/// O estado de `AbstractDebugAdapter` (os campos privados e `protected` da classe).
pub struct DebugAdapterState {
    sequence: Cell<i64>,
    pending_requests: RefCell<IndexMap<i64, ResponseCallback>>,
    pending_request_timers: RefCell<HashMap<i64, TimerId>>,
    request_callback: RefCell<Option<Rc<dyn Fn(&Request)>>>,
    event_callback: RefCell<Option<Rc<dyn Fn(&ProtocolEvent)>>>,
    message_callback: RefCell<Option<Rc<dyn Fn(&ProtocolMessage)>>>,
    queue: RefCell<VecDeque<ProtocolMessage>>,
    /// `_onError`.
    pub on_error_emitter: Rc<Emitter<Error>>,
    /// `_onExit`.
    pub on_exit_emitter: Rc<Emitter<Option<i32>>>,
}

impl DebugAdapterState {
    /// O construtor: `this.sequence = 1`.
    pub fn new() -> Rc<Self> {
        Rc::new(Self {
            sequence: Cell::new(1),
            pending_requests: RefCell::new(IndexMap::new()),
            pending_request_timers: RefCell::new(HashMap::new()),
            request_callback: RefCell::new(None),
            event_callback: RefCell::new(None),
            message_callback: RefCell::new(None),
            queue: RefCell::new(VecDeque::new()),
            on_error_emitter: Emitter::new(),
            on_exit_emitter: Emitter::new(),
        })
    }

    /// `clearPendingRequestTimer(requestSeq)`.
    fn clear_pending_request_timer(&self, request_seq: i64) {
        let timer = self.pending_request_timers.borrow_mut().remove(&request_seq);
        if let Some(timer) = timer {
            runtime::clear_timeout(timer);
        }
    }

    /// `dispose()` da classe abstrata.
    pub fn dispose(&self) {
        let timers: Vec<TimerId> = self.pending_request_timers.borrow().values().copied().collect();
        for timer in timers {
            runtime::clear_timeout(timer);
        }
        self.pending_request_timers.borrow_mut().clear();
        self.on_error_emitter.dispose();
        self.on_exit_emitter.dispose();
        self.queue.borrow_mut().clear();
    }

    /// `this._onError.fire(new Error(message))`.
    fn fire_error(&self, message: String) {
        self.on_error_emitter.fire(Error::new(message));
    }
}

/// `AbstractDebugAdapter`: o `IDebugAdapter` com tudo o que não depende de como a mensagem chega ao adaptador.
pub trait AbstractDebugAdapter: IDisposable + 'static {
    /// O estado da classe abstrata.
    fn state(&self) -> &Rc<DebugAdapterState>;

    fn start_session(&self) -> Promise<()>;

    fn stop_session(&self) -> Promise<()>;

    fn send_message(&self, message: &ProtocolMessage);

    /// `onError`.
    fn on_error(&self) -> Event<Error> {
        self.state().on_error_emitter.event()
    }

    /// `onExit`.
    fn on_exit(&self) -> Event<Option<i32>> {
        self.state().on_exit_emitter.event()
    }

    fn on_message(&self, callback: Box<dyn Fn(&ProtocolMessage)>) {
        let state = self.state();
        if state.message_callback.borrow().is_some() {
            state.fire_error("attempt to set more than one 'Message' callback".to_owned());
        }
        *state.message_callback.borrow_mut() = Some(Rc::from(callback));
    }

    fn on_event(&self, callback: Box<dyn Fn(&ProtocolEvent)>) {
        let state = self.state();
        if state.event_callback.borrow().is_some() {
            state.fire_error("attempt to set more than one 'Event' callback".to_owned());
        }
        *state.event_callback.borrow_mut() = Some(Rc::from(callback));
    }

    fn on_request(&self, callback: Box<dyn Fn(&Request)>) {
        let state = self.state();
        if state.request_callback.borrow().is_some() {
            state.fire_error("attempt to set more than one 'Request' callback".to_owned());
        }
        *state.request_callback.borrow_mut() = Some(Rc::from(callback));
    }

    fn send_response(&self, response: &mut Response) {
        if response.seq > 0 {
            self.state().fire_error(format!("attempt to send more than one response for command {}", response.command));
        } else {
            let mut message = ProtocolMessage::Response(std::mem::take(response));
            internal_send(self, &mut message);
            if let ProtocolMessage::Response(sent) = message {
                *response = sent;
            }
        }
    }

    /// `sendRequest(command, args, clb, timeout?)`: devolve o `seq` da requisição.
    fn send_request(&self, command: &str, args: Option<&Value>, clb: ResponseCallback, timeout_ms: Option<u64>) -> i64 {
        let has_args = match args {
            Some(Value::Object(map)) => !map.is_empty(),
            Some(Value::Array(items)) => !items.is_empty(),
            Some(Value::String(text)) => !text.is_empty(),
            _ => false,
        };
        let mut message = ProtocolMessage::Request(Request {
            seq: 0,
            command: command.to_owned(),
            arguments: if has_args { args.cloned() } else { None },
        });
        internal_send(self, &mut message);
        let seq = message.seq();

        let state = self.state();
        if let Some(timeout_ms) = timeout_ms {
            let timer_state = state.clone();
            let command = command.to_owned();
            let timer = runtime::set_timeout(
                move || {
                    timer_state.pending_request_timers.borrow_mut().remove(&seq);
                    let clb = timer_state.pending_requests.borrow_mut().shift_remove(&seq);
                    if let Some(clb) = clb {
                        let err = Response {
                            seq: 0,
                            request_seq: seq,
                            success: false,
                            command: command.clone(),
                            message: Some(localize("timeout", "Timeout after {0} ms for '{1}'", &[&timeout_ms.to_string(), &command])),
                            body: None,
                        };
                        clb(&err);
                    }
                },
                timeout_ms,
            );
            state.pending_request_timers.borrow_mut().insert(seq, timer);
        }
        // store callback for this request
        state.pending_requests.borrow_mut().insert(seq, clb);

        seq
    }

    fn accept_message(self: Rc<Self>, message: ProtocolMessage) {
        let message_callback = self.state().message_callback.borrow().clone();
        if let Some(callback) = message_callback {
            callback(&message);
        } else {
            let first = {
                let mut queue = self.state().queue.borrow_mut();
                queue.push_back(message);
                queue.len() == 1
            };
            if first {
                // first item = need to start processing loop
                process_queue(self);
            }
        }
    }

    /// Returns whether we should insert a timeout between processing messageA and messageB. Artificially queueing
    /// protocol messages guarantees that any microtasks for previous message finish before next message is processed.
    /// This is essential ordering when using promises anywhere along the call path.
    ///
    /// Because an event is dispatched synchronously, it may fire before the value the caller awaits is assigned if
    /// they're processed in the same task. Inserting a task boundary avoids this issue.
    fn needs_task_boundary_between(&self, message_a: &ProtocolMessage, message_b: &ProtocolMessage) -> bool {
        !matches!(message_a, ProtocolMessage::Event(_)) || !matches!(message_b, ProtocolMessage::Event(_))
    }

    /// `cancelPendingRequests()` (protected).
    fn cancel_pending_requests(&self) -> Promise<()> {
        let state = self.state().clone();
        if state.pending_requests.borrow().is_empty() {
            return Promise::resolved(());
        }

        let pending: Vec<(i64, ResponseCallback)> = state.pending_requests.borrow().iter().map(|(key, value)| (*key, value.clone())).collect();
        let (promise, resolver) = Promise::new();
        runtime::spawn_local(async move {
            let _ = timeout(500).await;
            for (request_seq, callback) in pending {
                let err = Response {
                    seq: 0,
                    request_seq,
                    success: false,
                    command: "canceled".to_owned(),
                    message: Some("canceled".to_owned()),
                    body: None,
                };
                callback(&err);
                state.pending_requests.borrow_mut().shift_remove(&request_seq);
                state.clear_pending_request_timer(request_seq);
            }
            resolver.resolve(());
        });
        promise
    }

    /// `getPendingRequestIds()`.
    fn get_pending_request_ids(&self) -> Vec<i64> {
        self.state().pending_requests.borrow().keys().copied().collect()
    }
}

/// `processQueue()`: reads and dispatches items from the queue until it is empty.
fn process_queue<T: AbstractDebugAdapter + ?Sized>(adapter: Rc<T>) {
    runtime::spawn_local(async move {
        let state = adapter.state().clone();
        let mut message: Option<ProtocolMessage> = None;
        loop {
            let needs_boundary = {
                let queue = state.queue.borrow();
                let Some(next) = queue.front() else { break };
                match &message {
                    None => true,
                    Some(previous) => adapter.needs_task_boundary_between(next, previous),
                }
            };
            if needs_boundary {
                let _ = timeout(0).await;
            }

            message = state.queue.borrow_mut().pop_front();
            let Some(current) = &message else {
                return; // may have been disposed of
            };

            match current {
                ProtocolMessage::Event(event) => {
                    let callback = state.event_callback.borrow().clone();
                    if let Some(callback) = callback {
                        callback(event);
                    }
                }
                ProtocolMessage::Request(request) => {
                    let callback = state.request_callback.borrow().clone();
                    if let Some(callback) = callback {
                        callback(request);
                    }
                }
                ProtocolMessage::Response(response) => {
                    let clb = state.pending_requests.borrow_mut().shift_remove(&response.request_seq);
                    if let Some(clb) = clb {
                        state.clear_pending_request_timer(response.request_seq);
                        clb(response);
                    }
                }
                ProtocolMessage::Other(_) => {}
            }
        }
    });
}

/// `internalSend(typ, message)`: o `typ` é a variante da mensagem; atribui o `seq` e entrega ao `sendMessage`.
fn internal_send<T: AbstractDebugAdapter + ?Sized>(adapter: &T, message: &mut ProtocolMessage) {
    let state = adapter.state();
    message.set_seq(state.sequence.get());
    state.sequence.set(state.sequence.get() + 1);
    adapter.send_message(message);
}
