//! A ponte entre as threads de E/S do túnel e o laço de eventos onde o handshake com o agente remoto roda.
//!
//! O `NodeRemoteTunnel` do original vive no laço de eventos do Node e chama `connectRemoteAgentTunnel` direto. Aqui o
//! encaminhamento de cada conexão local roda numa thread própria (E/S bloqueante, ver `tunnel_service`), enquanto
//! [`connect_remote_agent_tunnel`] devolve promessas e o `PersistentProtocol` e os `ISocket` do `je-base` são `Rc`, presos
//! à thread do laço de eventos. Este módulo é o elo entre os dois mundos, e é o que liga o [`ITunnelConnectFn`] (`Send +
//! Sync`, bloqueante) ao fluxo real do original:
//!
//! - [`RemoteAgentBridge::start`] ocupa uma tarefa do laço de eventos (a "bomba") que recebe comandos das outras threads por
//!   uma fila com `Waker`. A thread do laço de eventos precisa processar as tarefas acordadas de outras threads (o
//!   `run_microtasks` do `je-base` já o faz) e acordar quando o `Waker` é chamado.
//! - Um comando `Connect` roda [`connect_remote_agent_tunnel`] na thread do laço de eventos e devolve à thread chamadora um
//!   [`BridgeSocket`], o `ISocket` do protocolo visto como [`IManagedSocket`] (`Send + Sync`), mais os bytes que o protocolo
//!   já tinha lido (`readEntireBuffer`). O protocolo é descartado ali mesmo, como o original faz logo depois de pegar o soquete.
//! - `write`, `end`, `dispose` e `drain` do [`BridgeSocket`] viram comandos para a bomba; `drain` espera a resposta. Os eventos
//!   `onData`, `onEnd` e `onClose` do `ISocket` voltam por [`SocketEvents`].
//!
//! Divergências do original:
//! - O Node entrega os eventos do soquete de forma assíncrona, de modo que quem registra os três ouvintes no mesmo tick
//!   não perde nada. Aqui os eventos que chegam antes de existir ouvinte de `data`, de `end` e de `close` ficam guardados e
//!   são entregues, na ordem, quando o último dos três tipos ganha ouvinte.
//! - O transporte sempre chega ao espelho como [`IManagedSocket`]: o ramo `instanceof NodeSocket` do original (que liga o
//!   `net.Socket` cru por `pipe`) não existe, porque o soquete do `PersistentProtocol` é um `ISocket` do laço de eventos.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use je_base::buffer::VSBuffer;
use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::parts::ipc::net::{ISocket, SocketCloseEvent};
use je_base::runtime;

use super::tunnel_proxy::{ITunnelConnectFn, ITunnelProtocol};
use super::tunnel_socket::{CloseListener, DataListener, EndListener, IManagedSocket, TunnelSocket, Unsubscribe, lock};
use crate::remote::remote_agent_connection::{IConnectionOptions, connect_remote_agent_tunnel};

/// O identificador de cada soquete que atravessa a ponte.
static NEXT_SOCKET_ID: AtomicU64 = AtomicU64::new(1);

fn closed_error() -> Error {
    Error::new("the connection to the event loop is closed")
}

type DataFn = dyn Fn(&[u8]) + Send + Sync;
type EndFn = dyn Fn() + Send + Sync;
type CloseFn = dyn Fn(Option<&SocketCloseEvent>) + Send + Sync;

enum SocketEvent {
    Data(Vec<u8>),
    End,
    Close(Option<SocketCloseEvent>),
}

#[derive(Default)]
struct Listeners {
    next: u64,
    data: Vec<(u64, Arc<DataFn>)>,
    end: Vec<(u64, Arc<EndFn>)>,
    close: Vec<(u64, Arc<CloseFn>)>,
    /// Os três tipos já tiveram ouvinte: a partir daqui os eventos vão direto, sem passar pela fila.
    armed: bool,
    backlog: VecDeque<SocketEvent>,
}

struct Targets {
    data: Vec<Arc<DataFn>>,
    end: Vec<Arc<EndFn>>,
    close: Vec<Arc<CloseFn>>,
}

impl Listeners {
    fn targets(&self) -> Targets {
        Targets {
            data: self.data.iter().map(|(_, listener)| listener.clone()).collect(),
            end: self.end.iter().map(|(_, listener)| listener.clone()).collect(),
            close: self.close.iter().map(|(_, listener)| listener.clone()).collect(),
        }
    }
}

fn deliver(targets: &Targets, event: &SocketEvent) {
    match event {
        SocketEvent::Data(data) => targets.data.iter().for_each(|listener| listener(data)),
        SocketEvent::End => targets.end.iter().for_each(|listener| listener()),
        SocketEvent::Close(close) => targets.close.iter().for_each(|listener| listener(close.as_ref())),
    }
}

/// Os eventos de um soquete, da thread do laço de eventos para as threads de E/S.
#[derive(Default)]
struct SocketEvents {
    /// Mantém a ordem de entrega quando duas threads entregam ao mesmo tempo (a que registra o último ouvinte e a do laço).
    delivery: Mutex<()>,
    listeners: Mutex<Listeners>,
}

impl SocketEvents {
    fn push(&self, event: SocketEvent) {
        let _delivery = lock(&self.delivery);
        let targets = {
            let mut listeners = lock(&self.listeners);
            if !listeners.armed {
                listeners.backlog.push_back(event);
                return;
            }
            listeners.targets()
        };
        deliver(&targets, &event);
    }

    fn subscribe(self: &Arc<Self>, add: impl FnOnce(&mut Listeners, u64), remove: fn(&mut Listeners, u64)) -> Unsubscribe {
        let _delivery = lock(&self.delivery);
        let (id, replay) = {
            let mut listeners = lock(&self.listeners);
            let id = listeners.next;
            listeners.next += 1;
            add(&mut listeners, id);
            let ready = !listeners.armed && !listeners.data.is_empty() && !listeners.end.is_empty() && !listeners.close.is_empty();
            let replay = if ready {
                listeners.armed = true;
                Some((listeners.targets(), std::mem::take(&mut listeners.backlog)))
            } else {
                None
            };
            (id, replay)
        };
        if let Some((targets, backlog)) = replay {
            for event in &backlog {
                deliver(&targets, event);
            }
        }
        let events = self.clone();
        Box::new(move || remove(&mut lock(&events.listeners), id))
    }
}

/// O que a thread do laço de eventos executa em nome das threads de E/S.
enum Command {
    Connect { host: String, port: u16, reply: Sender<Result<ConnectedSocket, Error>> },
    Write(u64, Vec<u8>),
    End(u64),
    Dispose(u64),
    Drain(u64, Sender<Result<(), Error>>),
}

/// O resultado de um `Connect`.
struct ConnectedSocket {
    socket: Arc<BridgeSocket>,
    /// `protocol.readEntireBuffer()`.
    leftover: Vec<u8>,
}

struct QueueState {
    commands: VecDeque<Command>,
    waker: Option<Waker>,
    closed: bool,
}

/// A fila de comandos, com o `Waker` da bomba.
struct BridgeQueue {
    state: Mutex<QueueState>,
}

impl BridgeQueue {
    /// Enfileira e acorda a bomba; fechada a fila, devolve o comando.
    fn push(&self, command: Command) -> Result<(), Command> {
        let waker = {
            let mut state = lock(&self.state);
            if state.closed {
                return Err(command);
            }
            state.commands.push_back(command);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
        Ok(())
    }

    /// Fecha a fila: a bomba processa o que falta e termina.
    fn close(&self) {
        let waker = {
            let mut state = lock(&self.state);
            state.closed = true;
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

/// O próximo comando da fila, ou `None` com a fila fechada e vazia.
struct NextCommand(Arc<BridgeQueue>);

impl Future for NextCommand {
    type Output = Option<Command>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = lock(&self.0.state);
        if let Some(command) = state.commands.pop_front() {
            return Poll::Ready(Some(command));
        }
        if state.closed {
            return Poll::Ready(None);
        }
        state.waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

/// O `ISocket` do protocolo, visto das threads de E/S.
pub struct BridgeSocket {
    id: u64,
    queue: Arc<BridgeQueue>,
    events: Arc<SocketEvents>,
}

impl IManagedSocket for BridgeSocket {
    fn on_data(&self, listener: DataListener) -> Unsubscribe {
        let listener: Arc<DataFn> = Arc::from(listener);
        self.events.subscribe(move |listeners, id| listeners.data.push((id, listener)), |listeners, id| listeners.data.retain(|(listener_id, _)| *listener_id != id))
    }

    fn on_end(&self, listener: EndListener) -> Unsubscribe {
        let listener: Arc<EndFn> = Arc::from(listener);
        self.events.subscribe(move |listeners, id| listeners.end.push((id, listener)), |listeners, id| listeners.end.retain(|(listener_id, _)| *listener_id != id))
    }

    fn on_close(&self, listener: CloseListener) -> Unsubscribe {
        let listener: Arc<CloseFn> = Arc::from(listener);
        self.events.subscribe(move |listeners, id| listeners.close.push((id, listener)), |listeners, id| listeners.close.retain(|(listener_id, _)| *listener_id != id))
    }

    fn write(&self, buffer: &[u8]) {
        let _ = self.queue.push(Command::Write(self.id, buffer.to_vec()));
    }

    fn end(&self) {
        let _ = self.queue.push(Command::End(self.id));
    }

    fn drain(&self) -> Result<(), Error> {
        let (reply, receive) = mpsc::channel();
        self.queue.push(Command::Drain(self.id, reply)).map_err(|_| closed_error())?;
        receive.recv().map_err(|_| closed_error())?
    }

    fn dispose(&self) {
        let _ = self.queue.push(Command::Dispose(self.id));
    }
}

/// O `PersistentProtocol` já descartado, o que o [`ITunnelProtocol`] expõe dele.
struct BridgeProtocol {
    socket: Arc<BridgeSocket>,
    leftover: Mutex<Vec<u8>>,
}

impl ITunnelProtocol for BridgeProtocol {
    fn get_socket(&self) -> TunnelSocket {
        TunnelSocket::Generic(self.socket.clone())
    }

    fn read_entire_buffer(&self) -> Vec<u8> {
        std::mem::take(&mut *lock(&self.leftover))
    }

    fn dispose(&self) {}
}

/// Um soquete registrado na bomba, com as assinaturas dos seus eventos.
struct Registered {
    socket: Rc<dyn ISocket>,
    subscriptions: Vec<DisposableRef>,
}

type Registry = Rc<RefCell<HashMap<u64, Registered>>>;

fn socket_of(registry: &Registry, id: u64) -> Option<Rc<dyn ISocket>> {
    registry.borrow().get(&id).map(|registered| registered.socket.clone())
}

/// Registra o soquete do protocolo na bomba e liga os eventos dele à ponte.
fn register(registry: &Registry, queue: &Arc<BridgeQueue>, socket: Rc<dyn ISocket>, leftover: Vec<u8>) -> ConnectedSocket {
    let id = NEXT_SOCKET_ID.fetch_add(1, Ordering::SeqCst);
    let events = Arc::new(SocketEvents::default());
    let on_data = socket.on_data().listen({
        let events = events.clone();
        move |buffer: &VSBuffer| events.push(SocketEvent::Data(buffer.to_vec()))
    });
    let on_end = socket.on_end().listen({
        let events = events.clone();
        move |_: &()| events.push(SocketEvent::End)
    });
    let on_close = socket.on_close().listen({
        let (events, registry) = (events.clone(), registry.clone());
        move |event: &Option<SocketCloseEvent>| {
            events.push(SocketEvent::Close(event.clone()));
            // Fechado o transporte, não há mais o que escrever nele.
            registry.borrow_mut().remove(&id);
        }
    });
    registry.borrow_mut().insert(id, Registered { socket, subscriptions: vec![on_data, on_end, on_close] });
    ConnectedSocket { socket: Arc::new(BridgeSocket { id, queue: queue.clone(), events }), leftover }
}

/// A bomba: roda os comandos das threads de E/S no laço de eventos.
async fn pump(queue: Arc<BridgeQueue>, options: IConnectionOptions) {
    let registry: Registry = Rc::default();
    while let Some(command) = NextCommand(queue.clone()).await {
        match command {
            Command::Connect { host, port, reply } => {
                let (registry, queue) = (registry.clone(), queue.clone());
                connect_remote_agent_tunnel(&options, &host, u32::from(port)).then_result(move |outcome| {
                    let connected = outcome.map(|protocol| {
                        // O mesmo que o original faz: o soquete, o que o protocolo já leu, e o protocolo se despede.
                        let socket = protocol.get_socket();
                        let leftover = protocol.read_entire_buffer().to_vec();
                        protocol.dispose();
                        register(&registry, &queue, socket, leftover)
                    });
                    let _ = reply.send(connected);
                    Ok::<(), Error>(())
                });
            }
            Command::Write(id, data) => {
                if let Some(socket) = socket_of(&registry, id) {
                    let _ = errors::catch(|| socket.write(VSBuffer::wrap(data)));
                }
            }
            Command::End(id) => {
                if let Some(socket) = socket_of(&registry, id) {
                    let _ = errors::catch(|| socket.end());
                }
            }
            Command::Dispose(id) => {
                let registered = registry.borrow_mut().remove(&id);
                if let Some(registered) = registered {
                    registered.subscriptions.iter().for_each(|subscription| subscription.dispose());
                    registered.socket.dispose();
                }
            }
            Command::Drain(id, reply) => match socket_of(&registry, id) {
                Some(socket) => {
                    socket.drain().then_result(move |outcome| {
                        let _ = reply.send(outcome);
                        Ok::<(), Error>(())
                    });
                }
                None => {
                    let _ = reply.send(Ok(()));
                }
            },
        }
    }
}

/// A ponte de um conjunto de túneis que compartilham as mesmas opções de conexão.
pub struct RemoteAgentBridge {
    queue: Arc<BridgeQueue>,
}

impl RemoteAgentBridge {
    /// Ocupa uma tarefa do laço de eventos desta thread (que precisa ser a do serviço de túneis) e devolve a ponte com o
    /// [`ITunnelConnectFn`] que conversa com ela: o `connectRemoteAgentTunnel(options, host, port)` do original.
    pub fn start(options: IConnectionOptions) -> (Self, ITunnelConnectFn) {
        let queue = Arc::new(BridgeQueue { state: Mutex::new(QueueState { commands: VecDeque::new(), waker: None, closed: false }) });
        runtime::spawn_local(pump(queue.clone(), options));
        let connect: ITunnelConnectFn = Arc::new({
            let queue = queue.clone();
            move |host: &str, port: u16| -> Result<Box<dyn ITunnelProtocol>, Error> {
                let (reply, receive) = mpsc::channel();
                queue.push(Command::Connect { host: host.to_owned(), port, reply }).map_err(|_| closed_error())?;
                let connected = receive.recv().map_err(|_| closed_error())??;
                Ok(Box::new(BridgeProtocol { socket: connected.socket, leftover: Mutex::new(connected.leftover) }))
            }
        });
        (Self { queue }, connect)
    }

    /// Fecha a ponte: a bomba termina o que está na fila e sai; as conexões novas passam a falhar.
    pub fn close(&self) {
        self.queue.close();
    }
}
