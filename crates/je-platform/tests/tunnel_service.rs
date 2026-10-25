//! Porte de `src/vs/platform/tunnel/test/node/tunnelService.test.ts`.
//!
//! O teste isola a ponte do soquete genérico (`_mirrorGenericSocket`) do handshake do agente remoto: um servidor TCP em
//! `127.0.0.1`, o soquete aceito como soquete local e um `ISocket` simulado como remoto.
//!
//! Adaptações ao Rust: os emissores do `ISocket` simulado são listas de ouvintes chamadas sem trava; o `client.pause()` é não
//! ler o `TcpStream` do cliente até o `client.resume()`, e `await once(client, 'end')` é ler até o fim do fluxo.
//! `localSocket.writableLength` é `Mirror::writable_length` e `localSocket.destroyed` é `NodeTcpSocket::destroyed`.
//! `ensureNoDisposablesAreLeakedInTestSuite` não tem equivalente: os soquetes são descartados ao sair do escopo.

#![cfg(not(target_arch = "wasm32"))]

use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use je_base::errors::Error;
use je_base::parts::ipc::net::SocketCloseEvent;
use je_platform::tunnel::tunnel_service::{Mirror, mirror_generic_socket};
use je_platform::tunnel::tunnel_socket::{CloseListener, DataListener, EndListener, IManagedSocket, NodeTcpSocket, Unsubscribe};

/// O prazo de cada leitura do cliente: um teste que trava falha em vez de pendurar.
const IO_TIMEOUT: Duration = Duration::from_secs(60);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Um `Emitter` do original: guarda os ouvintes e chama cada um no `fire`.
struct Emitter<F: ?Sized> {
    listeners: Mutex<Vec<Arc<F>>>,
}

impl<F: ?Sized> Default for Emitter<F> {
    fn default() -> Self {
        Self { listeners: Mutex::new(Vec::new()) }
    }
}

impl<F: ?Sized> Emitter<F> {
    fn add(&self, listener: Arc<F>) -> Unsubscribe {
        lock(&self.listeners).push(listener);
        // O `store.add(...)` do original solta o ouvinte só ao fim da suíte; o emissor morre com o soquete.
        Box::new(|| {})
    }

    fn snapshot(&self) -> Vec<Arc<F>> {
        lock(&self.listeners).clone()
    }
}

/// O `remoteSocket: ISocket` do teste: `write`, `end` e `dispose` não fazem nada, `drain` resolve na hora.
#[derive(Default)]
struct MockRemoteSocket {
    on_data: Emitter<dyn Fn(&[u8]) + Send + Sync>,
    on_end: Emitter<dyn Fn() + Send + Sync>,
    on_close: Emitter<dyn Fn(Option<&SocketCloseEvent>) + Send + Sync>,
}

impl MockRemoteSocket {
    /// `onData.fire(...)`.
    fn fire_data(&self, data: &[u8]) {
        for listener in self.on_data.snapshot() {
            listener(data);
        }
    }

    /// `onEnd.fire()`.
    fn fire_end(&self) {
        for listener in self.on_end.snapshot() {
            listener();
        }
    }

    /// `onClose.fire(event)`.
    fn fire_close(&self, event: Option<SocketCloseEvent>) {
        for listener in self.on_close.snapshot() {
            listener(event.as_ref());
        }
    }
}

impl IManagedSocket for MockRemoteSocket {
    fn on_data(&self, listener: DataListener) -> Unsubscribe {
        self.on_data.add(Arc::from(listener))
    }

    fn on_end(&self, listener: EndListener) -> Unsubscribe {
        self.on_end.add(Arc::from(listener))
    }

    fn on_close(&self, listener: CloseListener) -> Unsubscribe {
        self.on_close.add(Arc::from(listener))
    }

    fn write(&self, _buffer: &[u8]) {}

    fn end(&self) {}

    fn drain(&self) -> Result<(), Error> {
        Ok(())
    }

    fn dispose(&self) {}
}

struct BufferedMirror {
    remote_socket: Arc<MockRemoteSocket>,
    client: TcpStream,
    received: Vec<u8>,
    local_socket: Arc<NodeTcpSocket>,
    response: Vec<u8>,
    /// Mantém o servidor e a ponte vivos até o fim do teste (`store.add(toDisposable(() => server.close()))`).
    _server: TcpListener,
    _mirror: Arc<Mirror>,
}

/// `mirrorGenericSocketWithBufferedResponse`
fn mirror_generic_socket_with_buffered_response() -> BufferedMirror {
    let remote_socket = Arc::new(MockRemoteSocket::default());

    let server = TcpListener::bind(("127.0.0.1", 0)).expect("listen");
    let port = server.local_addr().expect("server address").port();

    // `client.pause()`: o cliente só lê quando o teste manda (`resume`).
    let client = TcpStream::connect(("127.0.0.1", port)).expect("connect the client");
    client.set_read_timeout(Some(IO_TIMEOUT)).expect("set the client timeout");
    let (accepted, _) = server.accept().expect("accept the connection");
    let local_socket = NodeTcpSocket::new(accepted);

    let remote: Arc<dyn IManagedSocket> = remote_socket.clone();
    let mirror = mirror_generic_socket(&local_socket, &remote);

    // Buffer.alloc(16 * 1024 * 1024, 'response')
    let body: Vec<u8> = b"response".iter().cycle().take(16 * 1024 * 1024).copied().collect();
    let mut response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    response.extend_from_slice(&body);
    // Deliver in message-sized chunks; some platforms (Windows) hand a single large write straight to the kernel without buffering.
    let chunk_size = 1024 * 1024;
    for chunk in response.chunks(chunk_size) {
        remote_socket.fire_data(chunk);
    }
    assert!(mirror.writable_length() > 0, "The response must still have buffered data when the remote socket terminates");

    BufferedMirror { remote_socket, client, received: Vec::new(), local_socket, response, _server: server, _mirror: mirror }
}

fn flushes_a_large_response_on(termination: &str) {
    let mut fixture = mirror_generic_socket_with_buffered_response();

    if termination != "close" {
        fixture.remote_socket.fire_end();
    }
    if termination != "end" {
        fixture.remote_socket.fire_close(None);
    }

    // client.resume(); await ended;
    fixture.client.read_to_end(&mut fixture.received).expect("read the response until the end");
    assert!(fixture.received == fixture.response, "Received {} of {} response bytes", fixture.received.len(), fixture.response.len());
}

#[test]
fn generic_socket_flushes_a_large_response_on_end() {
    flushes_a_large_response_on("end");
}

#[test]
fn generic_socket_flushes_a_large_response_on_close() {
    flushes_a_large_response_on("close");
}

#[test]
fn generic_socket_flushes_a_large_response_on_end_then_close() {
    flushes_a_large_response_on("end then close");
}

#[test]
fn generic_socket_aborts_the_local_socket_when_the_remote_socket_closes_with_an_error() {
    let fixture = mirror_generic_socket_with_buffered_response();

    fixture.remote_socket.fire_close(Some(SocketCloseEvent::Node { had_error: true, error: Some(Error::new("remote failure")) }));

    assert!(fixture.local_socket.destroyed());
}
