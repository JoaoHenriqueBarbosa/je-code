//! Os soquetes do túnel: o que o `tunnelProxy.ts` pede de `net.Socket`, `NodeSocket`, `ISocket` e `Duplex`
//! (`src/vs/base/parts/ipc/node/ipc.net.ts`, `src/vs/base/parts/ipc/common/ipc.net.ts` e `stream`), em E/S bloqueante
//! com threads.
//!
//! Decisões de tradução:
//! - O `Duplex` do Node é [`Duplex`]: leitura bloqueante (`Ok(0)` é o `'end'`), `write_all`, `end` (`Duplex.end()`, fecha só
//!   a escrita), `destroy` (derruba na hora e acorda quem está lendo), `unshift` e o ouvinte de `'error'`.
//! - `net.Socket` é [`NodeTcpSocket`] (um `TcpStream` com o `destroyed`, o `'close'` e os ouvintes de erro); o `NodeSocket`
//!   que o embrulha é [`NodeSocket`], com o ouvinte de `'error'` que o original instala e o `dispose(destroySocket)`.
//! - O `ISocket` genérico (transporte gerenciado, sem `net.Socket`) é [`IManagedSocket`]: o mesmo contrato por eventos
//!   (`onData`, `onEnd`, `onClose`, `write`, `end`, `drain`, `dispose`), mas com ouvintes `Send + Sync` e `drain`
//!   bloqueante, porque cada ponte roda em threads. Os ouvintes são chamados sem trava interna do emissor.
//! - [`RemoteSocketStream`] é a classe de mesmo nome do original: adapta um [`IManagedSocket`] a um [`Duplex`], com a
//!   contrapressão do `drain` na escrita. Os `setKeepAlive`/`ref`/`unref` que ela expõe para o `http.Agent` não existem
//!   aqui, porque o agente do porte não chama nada de `net.Socket` nos soquetes.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use je_base::errors::Error;
use je_base::parts::ipc::net::SocketCloseEvent;

/// A trava sem envenenamento: um pânico em outra thread não pode derrubar o desligamento do proxy.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Cancela uma assinatura de evento.
pub type Unsubscribe = Box<dyn FnOnce() + Send>;
/// O ouvinte de `onData`.
pub type DataListener = Box<dyn Fn(&[u8]) + Send + Sync>;
/// O ouvinte de `onEnd`.
pub type EndListener = Box<dyn Fn() + Send + Sync>;
/// O ouvinte de `onClose`.
pub type CloseListener = Box<dyn Fn(Option<&SocketCloseEvent>) + Send + Sync>;
/// O ouvinte de `'error'`.
pub type ErrorListener = Arc<dyn Fn(&io::Error) + Send + Sync>;

/// O `Duplex` do Node, bloqueante.
pub trait Duplex: Send + Sync {
    /// Lê até `buf.len()` bytes; bloqueia até haver dados. `Ok(0)` é o fim do fluxo (`'end'`).
    fn read(&self, buf: &mut [u8]) -> io::Result<usize>;
    /// `write(data)`: escreve tudo (a contrapressão é o próprio bloqueio).
    fn write_all(&self, data: &[u8]) -> io::Result<()>;
    /// `end()`: fecha a escrita; a leitura continua até o outro lado terminar.
    fn end(&self);
    /// `destroy()`: derruba o soquete na hora e acorda as threads bloqueadas nele.
    fn destroy(&self);
    /// `destroyed`.
    fn is_destroyed(&self) -> bool;
    /// `unshift(chunk)`: devolve bytes ao começo da fila de leitura.
    fn unshift(&self, data: Vec<u8>);
    /// `on('error', listener)`. Os soquetes sem erros assíncronos próprios não guardam nada.
    fn on_error(&self, _listener: ErrorListener) {}
}

/// Os bytes devolvidos por `unshift` e os que sobraram de um pedaço lido a mais.
#[derive(Default)]
pub struct PendingBytes(Mutex<Vec<u8>>);

impl PendingBytes {
    pub fn unshift(&self, data: Vec<u8>) {
        let mut pending = lock(&self.0);
        let mut merged = data;
        merged.extend_from_slice(&pending);
        *pending = merged;
    }

    /// Copia os bytes pendentes para `buf`; `None` se não há nenhum.
    pub fn take(&self, buf: &mut [u8]) -> Option<usize> {
        let mut pending = lock(&self.0);
        if pending.is_empty() || buf.is_empty() {
            return None;
        }
        let count = pending.len().min(buf.len());
        buf[..count].copy_from_slice(&pending[..count]);
        pending.drain(..count);
        Some(count)
    }
}

/// O evento `'close'`: acontece uma vez e quem espera é acordado.
#[derive(Default)]
struct CloseSignal {
    closed: Mutex<bool>,
    condvar: Condvar,
}

impl CloseSignal {
    fn close(&self) {
        *lock(&self.closed) = true;
        self.condvar.notify_all();
    }

    fn wait(&self, timeout: Duration) -> bool {
        let closed = lock(&self.closed);
        let (closed, _) = self.condvar.wait_timeout_while(closed, timeout, |closed| !*closed).unwrap_or_else(PoisonError::into_inner);
        *closed
    }
}

fn is_interrupted(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::Interrupted
}

/// `net.Socket`: um `TcpStream` com o `destroyed`, o `'close'` e os ouvintes de `'error'`.
pub struct NodeTcpSocket {
    stream: TcpStream,
    destroyed: AtomicBool,
    pending: PendingBytes,
    closed: CloseSignal,
    error_listeners: Mutex<Vec<(u64, ErrorListener)>>,
    next_listener: AtomicU64,
}

impl NodeTcpSocket {
    pub fn new(stream: TcpStream) -> Arc<Self> {
        Arc::new(Self {
            stream,
            destroyed: AtomicBool::new(false),
            pending: PendingBytes::default(),
            closed: CloseSignal::default(),
            error_listeners: Mutex::new(Vec::new()),
            next_listener: AtomicU64::new(0),
        })
    }

    /// `destroyed`.
    pub fn destroyed(&self) -> bool {
        self.destroyed.load(Ordering::SeqCst)
    }

    /// `localAddress`: o endereço IP local da conexão (sem a porta); `None` com o soquete já fechado.
    pub fn local_address(&self) -> Option<String> {
        self.stream.local_addr().ok().map(|address| address.ip().to_string())
    }

    /// `socket.on('error', listener)`: devolve o id para o `off_error`.
    pub fn add_error_listener(&self, listener: ErrorListener) -> u64 {
        let id = self.next_listener.fetch_add(1, Ordering::SeqCst);
        lock(&self.error_listeners).push((id, listener));
        id
    }

    /// `socket.off('error', listener)`.
    pub fn remove_error_listener(&self, id: u64) {
        lock(&self.error_listeners).retain(|(listener_id, _)| *listener_id != id);
    }

    /// `socket.emit('error', error)`. No Node um `'error'` sem ouvinte é lançado como exceção não capturada: devolve
    /// `true` quando algum ouvinte o recebeu (o que o Node trataria sem lançar) e `false` quando ele seria lançado.
    pub fn emit_error(&self, error: &io::Error) -> bool {
        let listeners: Vec<ErrorListener> = lock(&self.error_listeners).iter().map(|(_, listener)| listener.clone()).collect();
        for listener in &listeners {
            listener(error);
        }
        !listeners.is_empty()
    }

    /// Espera o `'close'` por até `timeout`; `true` se ele aconteceu.
    pub fn wait_closed(&self, timeout: Duration) -> bool {
        self.closed.wait(timeout)
    }
}

impl Duplex for NodeTcpSocket {
    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(count) = self.pending.take(buf) {
            return Ok(count);
        }
        loop {
            if self.destroyed() {
                return Ok(0);
            }
            match (&self.stream).read(buf) {
                Ok(count) => {
                    if count == 0 {
                        self.closed.close();
                    }
                    return Ok(count);
                }
                Err(error) if is_interrupted(&error) => {}
                Err(error) => {
                    self.closed.close();
                    return Err(error);
                }
            }
        }
    }

    fn write_all(&self, data: &[u8]) -> io::Result<()> {
        if self.destroyed() {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        (&self.stream).write_all(data)
    }

    fn end(&self) {
        let _ = self.stream.shutdown(Shutdown::Write);
    }

    fn destroy(&self) {
        self.destroyed.store(true, Ordering::SeqCst);
        let _ = self.stream.shutdown(Shutdown::Both);
        self.closed.close();
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed()
    }

    fn unshift(&self, data: Vec<u8>) {
        self.pending.unshift(data);
    }

    fn on_error(&self, listener: ErrorListener) {
        self.add_error_listener(listener);
    }
}

/// `NodeSocket`: o embrulho de um `net.Socket` que instala o ouvinte de `'error'` do original (todo erro que não é
/// `EPIPE` é um erro inesperado) e que o proxy desfaz com `dispose_with(false)` ao tomar o soquete cru.
pub struct NodeSocket {
    socket: Arc<NodeTcpSocket>,
    error_listener: Mutex<Option<u64>>,
    unexpected_errors: Arc<AtomicUsize>,
}

impl NodeSocket {
    pub fn new(socket: Arc<NodeTcpSocket>) -> Arc<Self> {
        let unexpected_errors = Arc::new(AtomicUsize::new(0));
        let counter = unexpected_errors.clone();
        // `onUnexpectedError` do original: o `EPIPE` num momento errado é ignorado, o resto é contado.
        let listener = socket.add_error_listener(Arc::new(move |error: &io::Error| {
            if error.kind() != io::ErrorKind::BrokenPipe {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));
        Arc::new(Self { socket, error_listener: Mutex::new(Some(listener)), unexpected_errors })
    }

    /// `socket`: o `net.Socket` que ele embrulha.
    pub fn socket(&self) -> Arc<NodeTcpSocket> {
        self.socket.clone()
    }

    /// Quantos erros chegaram ao `onUnexpectedError` do original.
    pub fn unexpected_error_count(&self) -> usize {
        self.unexpected_errors.load(Ordering::SeqCst)
    }

    /// `dispose(destroySocket)`.
    pub fn dispose_with(&self, destroy_socket: bool) {
        if let Some(id) = lock(&self.error_listener).take() {
            self.socket.remove_error_listener(id);
        }
        if destroy_socket {
            self.socket.destroy();
        }
    }

    /// `dispose()`.
    pub fn dispose(&self) {
        self.dispose_with(true);
    }
}

/// `ISocket` de um transporte sem `net.Socket` (uma conexão gerenciada ou de exec-server).
pub trait IManagedSocket: Send + Sync {
    fn on_data(&self, listener: DataListener) -> Unsubscribe;
    fn on_end(&self, listener: EndListener) -> Unsubscribe;
    fn on_close(&self, listener: CloseListener) -> Unsubscribe;
    fn write(&self, buffer: &[u8]);
    fn end(&self);
    /// `drain()`: espera o transporte esvaziar o que tem para escrever.
    fn drain(&self) -> Result<(), Error>;
    fn dispose(&self);
}

/// O soquete que um `ITunnelProtocol` entrega: o `NodeSocket` (servidor clássico/websocket) ou um `ISocket` genérico.
#[derive(Clone)]
pub enum TunnelSocket {
    Node(Arc<NodeSocket>),
    Generic(Arc<dyn IManagedSocket>),
}

enum RemoteEvent {
    Data(Vec<u8>),
    End,
    Closed,
}

/// `RemoteSocketStream`: adapts a generic [`IManagedSocket`] (such as a managed / exec-server connection, which has no
/// underlying `net.Socket`) to a [`Duplex`], so the proxy can pipe and pool it exactly like the raw socket it extracts
/// from a [`NodeSocket`].
pub struct RemoteSocketStream {
    socket: Arc<dyn IManagedSocket>,
    events: Mutex<Receiver<RemoteEvent>>,
    sender: Mutex<Sender<RemoteEvent>>,
    subscriptions: Mutex<Vec<Unsubscribe>>,
    pending: PendingBytes,
    ended: AtomicBool,
    destroyed: AtomicBool,
    error: Mutex<Option<String>>,
}

impl RemoteSocketStream {
    pub fn new(socket: Arc<dyn IManagedSocket>) -> Arc<Self> {
        let (sender, events) = mpsc::channel();
        let stream = Arc::new(Self {
            socket,
            events: Mutex::new(events),
            sender: Mutex::new(sender),
            subscriptions: Mutex::new(Vec::new()),
            pending: PendingBytes::default(),
            ended: AtomicBool::new(false),
            destroyed: AtomicBool::new(false),
            error: Mutex::new(None),
        });
        let weak: Weak<Self> = Arc::downgrade(&stream);
        let on_data = stream.socket.on_data(Box::new({
            let weak = weak.clone();
            move |data: &[u8]| {
                if let Some(stream) = weak.upgrade() {
                    stream.push(RemoteEvent::Data(data.to_vec()));
                }
            }
        }));
        let on_end = stream.socket.on_end(Box::new({
            let weak = weak.clone();
            move || {
                if let Some(stream) = weak.upgrade() {
                    stream.push(RemoteEvent::End);
                }
            }
        }));
        let on_close = stream.socket.on_close(Box::new(move |event: Option<&SocketCloseEvent>| {
            // The transport is fully closed, so tear the stream down: this closes it (so the proxy drops it from its
            // socket set and the agent evicts it from the pool) and disposes the underlying socket. A clean close
            // carries no error.
            let error = match event {
                Some(SocketCloseEvent::Node { error, .. }) => error.clone(),
                _ => None,
            };
            if let Some(stream) = weak.upgrade() {
                stream.destroy_with(error);
            }
        }));
        lock(&stream.subscriptions).extend([on_data, on_end, on_close]);
        stream
    }

    fn push(&self, event: RemoteEvent) {
        let _ = lock(&self.sender).send(event);
    }

    /// `destroy(error)`: `_destroy` do original (solta os ouvintes e descarta o `ISocket`) mais o erro que o `'close'`
    /// carregava, que a próxima leitura devolve.
    fn destroy_with(&self, error: Option<Error>) {
        if self.destroyed.swap(true, Ordering::SeqCst) {
            return;
        }
        if let Some(error) = error {
            *lock(&self.error) = Some(error.message);
        }
        let subscriptions: Vec<Unsubscribe> = lock(&self.subscriptions).drain(..).collect();
        for unsubscribe in subscriptions {
            unsubscribe();
        }
        self.socket.dispose();
        self.push(RemoteEvent::Closed);
    }

    fn finish_read(&self) -> io::Result<usize> {
        match lock(&self.error).clone() {
            Some(message) => Err(io::Error::other(message)),
            None => Ok(0),
        }
    }
}

impl Duplex for RemoteSocketStream {
    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        if let Some(count) = self.pending.take(buf) {
            return Ok(count);
        }
        let events = lock(&self.events);
        loop {
            if self.ended.load(Ordering::SeqCst) {
                return self.finish_read();
            }
            // Um `'close'` do transporte destrói o adaptador logo depois do último `'data'`; os pedaços que já estão na fila
            // são entregues antes do fim do fluxo (no Node o `'data'` enfileirado chega antes do `'end'`), senão a resposta
            // do destino se perde conforme a ordem das threads. Destruído, a fila não se espera: só se esvazia.
            let next = if self.destroyed.load(Ordering::SeqCst) { events.try_recv().ok() } else { events.recv().ok() };
            match next {
                Some(RemoteEvent::Data(data)) => {
                    if data.is_empty() {
                        continue;
                    }
                    let count = data.len().min(buf.len());
                    buf[..count].copy_from_slice(&data[..count]);
                    if count < data.len() {
                        self.pending.unshift(data[count..].to_vec());
                    }
                    return Ok(count);
                }
                Some(RemoteEvent::End | RemoteEvent::Closed) | None => {
                    self.ended.store(true, Ordering::SeqCst);
                    return self.finish_read();
                }
            }
        }
    }

    /// `_write`: respect backpressure: complete only after the socket has drained its buffer so a fast producer cannot
    /// queue unbounded data on a slow managed / exec-server transport.
    fn write_all(&self, data: &[u8]) -> io::Result<()> {
        if self.destroyed.load(Ordering::SeqCst) {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        self.socket.write(data);
        self.socket.drain().map_err(|error| io::Error::other(error.message))
    }

    /// `_final`.
    fn end(&self) {
        self.socket.end();
    }

    fn destroy(&self) {
        self.destroy_with(None);
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed.load(Ordering::SeqCst)
    }

    fn unshift(&self, data: Vec<u8>) {
        self.pending.unshift(data);
    }
}
