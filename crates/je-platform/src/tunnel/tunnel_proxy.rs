//! Porte de `src/vs/platform/tunnel/node/tunnelProxy.ts`: o proxy HTTPS em `127.0.0.1` que leva as conexões TCP do
//! navegador embutido pelo túnel do agente remoto (`CONNECT` para HTTPS, requisições HTTP em forma absoluta para HTTP
//! simples), com TLS autoassinado, autenticação Basic aleatória e a impressão digital do certificado devolvida em
//! [`TunnelProxy::start`].
//!
//! Decisões de tradução:
//! - O original roda no laço de eventos do Node (`net`, `http`, `https`). Aqui a E/S é bloqueante com `std::net` e
//!   threads: uma thread aceita conexões, uma thread atende cada conexão do cliente e um `CONNECT` ganha uma segunda
//!   thread para o sentido túnel para cliente (`pipe` nos dois sentidos). O código de rede não existe no `wasm32`.
//! - O `https.createServer` é o `rustls` com o provedor RustCrypto (Rust puro) sobre [`TlsServerStream`]; o HTTP/1.1 que o
//!   `http.Server` e o `http.request` fazem por baixo está em `http_wire` (cabeçalhos, `Content-Length`, `chunked`,
//!   keep-alive e a serialização). O certificado vem de [`generate_self_signed_cert`].
//! - `findFreePortFaster(0, ...)` do original resolve com a porta `0` (o sistema escolhe a porta ao `listen`): aqui o
//!   `TcpListener` já nasce na porta `0` e fica com o endereço que o sistema deu, sem a janela entre achar e ocupar.
//! - O `http.Agent` com `keepAlive` que junta os soquetes do túnel por `host:porta` é [`TunnelAgent`]: só pede um túnel novo
//!   quando não há soquete livre para o destino, e um soquete livre que o outro lado fechou enquanto parado faz a
//!   requisição de corpo vazio tentar de novo uma vez com um túnel novo (o Node tira o soquete do pool pelo evento `close`,
//!   que aqui não há). `destroy()` do agente derruba todos os soquetes que ele criou, os livres e os em uso, como no Node.
//! - O `Limiter` do original (`async.ts`) é de laço de eventos e `Rc`: [`ConnectLimiter`] é o mesmo limite de 6 conexões
//!   simultâneas, com semáforo. Descartar o limitador acorda quem espera com erro (no original essas promessas nunca
//!   resolvem, e os soquetes de quem esperava são destruídos de qualquer forma).
//! - `ILogService` é `Rc` e as threads precisam de `Send + Sync`: o proxy fala com o [`ITunnelProxyLog`], com os mesmos
//!   níveis (`trace`, `info`, `warn`, `error`); o erro, que o original passa como argumento, vai no texto.
//! - `ITunnelConnectFn` devolve um [`ITunnelProtocol`] (o subconjunto de `PersistentProtocol` que o proxy usa). O
//!   `readEntireBuffer()` devolve `Vec<u8>` (o `VSBuffer` do porte é `Rc`, sem `Send`). Os dois transportes do original
//!   são [`TunnelSocket::Node`] (o `NodeSocket`: o proxy toma o soquete cru) e [`TunnelSocket::Generic`] (o `ISocket`
//!   adaptado por [`RemoteSocketStream`]).
//! - `start` chamado duas vezes devolve erro (no original a segunda chamada deixaria o primeiro servidor órfão).

use std::collections::HashMap;
use std::io;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use je_base::errors::Error;
use rand_core::{OsRng, RngCore};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use super::http_wire::{
    BodyEncoding, BodyFraming, BodyWriter, Headers, RequestHead, ResponseHead, WireReader, http_date, parse_request_head, request_framing, response_framing, serialize_request_head,
    serialize_response_head,
};
use super::self_signed_cert::{ISelfSignedCert, generate_self_signed_cert, pem_to_der};
use super::tls_stream::TlsServerStream;
use super::tunnel_proxy_info::{ITunnelProxyCredentials, ITunnelProxyInfo};
use super::tunnel_socket::{Duplex, RemoteSocketStream, TunnelSocket, lock};

/// Maximum number of tunnel connections we establish through the remote
/// agent at the same time. Each new tunnel dials the loopback forwarder,
/// which opens a fresh multiplexed channel to the remote (crypto +
/// round-trips) on a single event loop. An ad-heavy page fans out dozens
/// of simultaneous CONNECTs to distinct hosts; left unbounded, that
/// stampede overflows the forwarder's accept backlog and it starts
/// refusing (ECONNREFUSED) and resetting (ECONNRESET) connections. This
/// cap smooths the burst to a rate the forwarder can absorb; excess
/// requests queue rather than fail.
const MAX_CONCURRENT_TUNNEL_CONNECTS: usize = 6;

/// O `headersTimeout` do `http.Server` do Node: o prazo para o cabeçalho chegar.
const HEADERS_TIMEOUT: Duration = Duration::from_secs(60);
/// O `requestTimeout` do `http.Server` do Node: o prazo para a requisição inteira.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
/// O `keepAliveTimeout` do `http.Server` do Node (o `Keep-Alive: timeout=5` que ele anuncia).
const KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(5);
/// Quanto tempo uma conexão encerrada pelo proxy espera o cliente ler a resposta antes de ser derrubada.
const LINGER_TIMEOUT: Duration = Duration::from_secs(2);
/// O tamanho de cada leitura das pontes do `CONNECT`.
const PUMP_BUFFER: usize = 64 * 1024;

/// Os cabeçalhos hop-by-hop que o proxy tira, além dos nomeados em `Connection`.
const HOP_BY_HOP_HEADERS: [&str; 7] = ["connection", "keep-alive", "proxy-authorization", "proxy-connection", "te", "transfer-encoding", "upgrade"];

/// `ITunnelConnectFn`: a função que um túnel TCP até `host:port` pelo agente remoto. Retorna só depois de o remoto
/// confirmar (pelo handshake do túnel) que o destino está acessível, e rejeita caso contrário.
pub type ITunnelConnectFn = Arc<dyn Fn(&str, u16) -> Result<Box<dyn ITunnelProtocol>, Error> + Send + Sync>;

/// O que o proxy usa do resultado da conexão: um subconjunto de `PersistentProtocol` (`getSocket()`,
/// `readEntireBuffer()` e `dispose()`).
pub trait ITunnelProtocol: Send {
    fn get_socket(&self) -> TunnelSocket;
    /// Os bytes que o protocolo já leu durante o handshake.
    fn read_entire_buffer(&self) -> Vec<u8>;
    fn dispose(&self);
}

/// O `ILogService` do proxy, com `Send + Sync` (ver o topo).
pub trait ITunnelProxyLog: Send + Sync {
    fn trace(&self, message: &str);
    fn info(&self, message: &str);
    fn warn(&self, message: &str);
    fn error(&self, message: &str);
}

/// O `NullLogService`.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullTunnelProxyLog;

impl ITunnelProxyLog for NullTunnelProxyLog {
    fn trace(&self, _message: &str) {}
    fn info(&self, _message: &str) {}
    fn warn(&self, _message: &str) {}
    fn error(&self, _message: &str) {}
}

fn io_error(error: io::Error) -> Error {
    Error::new(error.to_string())
}

/// Um conjunto de soquetes que o desligamento derruba. Os já destruídos saem do conjunto a cada inclusão (o `'close'` do
/// original remove cada um).
#[derive(Default)]
struct SocketSet {
    sockets: Mutex<Vec<Arc<dyn Duplex>>>,
}

impl SocketSet {
    fn add(&self, socket: Arc<dyn Duplex>) {
        let mut sockets = lock(&self.sockets);
        sockets.retain(|existing| !existing.is_destroyed());
        sockets.push(socket);
    }

    fn remove(&self, socket: &Arc<dyn Duplex>) {
        lock(&self.sockets).retain(|existing| !Arc::ptr_eq(existing, socket));
    }

    fn destroy_all(&self) {
        let drained = std::mem::take(&mut *lock(&self.sockets));
        for socket in drained {
            socket.destroy();
        }
    }
}

/// O limite de túneis sendo criados ao mesmo tempo. Gates the setup (connect + handshake) only; once a tunnel is
/// established the slot is released and data piping proceeds unthrottled.
struct ConnectLimiter {
    state: Mutex<LimiterState>,
    condvar: Condvar,
    max: usize,
}

#[derive(Default)]
struct LimiterState {
    running: usize,
    disposed: bool,
}

struct LimiterSlot<'a>(&'a ConnectLimiter);

impl Drop for LimiterSlot<'_> {
    fn drop(&mut self) {
        lock(&self.0.state).running -= 1;
        self.0.condvar.notify_one();
    }
}

impl ConnectLimiter {
    fn new(max: usize) -> Self {
        Self { state: Mutex::new(LimiterState::default()), condvar: Condvar::new(), max }
    }

    fn run<T>(&self, task: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
        {
            let mut state = lock(&self.state);
            while state.running >= self.max && !state.disposed {
                state = self.condvar.wait(state).unwrap_or_else(PoisonError::into_inner);
            }
            if state.disposed {
                return Err(Error::new("TunnelProxy disposed while waiting for a tunnel slot"));
            }
            state.running += 1;
        }
        let _slot = LimiterSlot(self);
        task()
    }

    fn dispose(&self) {
        lock(&self.state).disposed = true;
        self.condvar.notify_all();
    }
}

type AgentKey = (String, u16);

/// O `http.Agent` com `keepAlive`: pools tunnel sockets by target host:port.
#[derive(Default)]
struct TunnelAgent {
    free: Mutex<HashMap<AgentKey, Vec<Arc<dyn Duplex>>>>,
    sockets: SocketSet,
    destroyed: AtomicBool,
}

impl TunnelAgent {
    /// Um soquete livre e vivo para o destino (o mais recente primeiro, o `scheduling: 'lifo'` do Node).
    fn take_free(&self, key: &AgentKey) -> Option<Arc<dyn Duplex>> {
        let mut free = lock(&self.free);
        let list = free.get_mut(key)?;
        while let Some(socket) = list.pop() {
            if !socket.is_destroyed() {
                return Some(socket);
            }
        }
        None
    }

    /// Um soquete que o agente criou: `destroy()` o derruba.
    fn register(&self, socket: &Arc<dyn Duplex>) {
        self.sockets.add(socket.clone());
        if self.destroyed.load(Ordering::SeqCst) {
            socket.destroy();
        }
    }

    fn release(&self, key: AgentKey, socket: Arc<dyn Duplex>) {
        lock(&self.free).entry(key).or_default().push(socket.clone());
        if self.destroyed.load(Ordering::SeqCst) {
            socket.destroy();
        }
    }

    fn destroy(&self) {
        self.destroyed.store(true, Ordering::SeqCst);
        lock(&self.free).clear();
        self.sockets.destroy_all();
    }
}

/// O que as threads do proxy compartilham.
struct Shared {
    connect_tunnel: ITunnelConnectFn,
    log: Arc<dyn ITunnelProxyLog>,
    expected_auth_header: OnceLock<String>,
    agent: Mutex<Option<Arc<TunnelAgent>>>,
    /// Sockets we took over from the HTTPS server via CONNECT. Once the
    /// CONNECT handler runs the server no longer tracks them, so
    /// closing the server won't terminate them: we have to destroy them
    /// ourselves on dispose to release the listening port promptly.
    connect_sockets: SocketSet,
    /// The remote (tunnel) side of every active bridge: both CONNECT
    /// tunnels and pooled plain-HTTP sockets. We destroy these explicitly
    /// and synchronously on dispose rather than relying on the local
    /// socket's async close to propagate `end()`.
    remote_sockets: SocketSet,
    connect_limiter: ConnectLimiter,
    disposed: AtomicBool,
}

impl Shared {
    fn current_agent(&self) -> Option<Arc<TunnelAgent>> {
        lock(&self.agent).clone()
    }

    /// Abre um túnel (respeitando o limite) e toma o fluxo remoto, com os bytes que o handshake já leu.
    fn open_tunnel(&self, host: &str, port: u16) -> Result<(Arc<dyn Duplex>, Vec<u8>), Error> {
        // The connect function resolves only once the remote has confirmed the
        // target is reachable (via the tunnel handshake) and rejects otherwise.
        let protocol = self.connect_limiter.run(|| (self.connect_tunnel)(host, port))?;
        Ok(self.take_remote_stream(protocol.as_ref()))
    }

    /// Take ownership of a freshly-connected tunnel's transport as a [`Duplex`] stream, together with any bytes the
    /// protocol already buffered during the handshake (the caller routes that leftover to the appropriate side).
    ///
    /// Two transports occur in practice:
    /// - [`TunnelSocket::Node`] (classic/websocket server): unwrap the raw socket so we can rely on native stream
    ///   backpressure.
    /// - [`TunnelSocket::Generic`] (managed / exec-server connection): there is no raw socket underneath, so adapt the
    ///   message-passing socket to a [`Duplex`] ([`RemoteSocketStream`]).
    fn take_remote_stream(&self, protocol: &dyn ITunnelProtocol) -> (Arc<dyn Duplex>, Vec<u8>) {
        match protocol.get_socket() {
            TunnelSocket::Node(node_socket) => {
                // Take ownership of the raw socket, detaching NodeSocket's own listener. NodeSocket routes every non-EPIPE
                // error through the unexpected-error handler. When the upstream tunnel endpoint dies, every pooled/active
                // tunnel socket is reset at once: that reset is expected teardown here, not an unexpected error. We bridge
                // the raw socket ourselves (attaching our own error handlers), so NodeSocket's routing must be removed.
                let socket: Arc<dyn Duplex> = node_socket.socket();
                let leftover = protocol.read_entire_buffer();
                node_socket.dispose_with(false);
                protocol.dispose();
                (socket, leftover)
            }
            TunnelSocket::Generic(managed_socket) => {
                // Read the buffered leftover and detach the protocol's reader/writer before the adapter starts consuming
                // the socket, so subsequent messages reach the adapter exactly once.
                let leftover = protocol.read_entire_buffer();
                protocol.dispose();
                (RemoteSocketStream::new(managed_socket), leftover)
            }
        }
    }

    /// Track a remote tunnel socket so `dispose` can tear it down synchronously. The socket drops out of the set once
    /// destroyed.
    fn track_remote_socket(&self, socket: &Arc<dyn Duplex>) {
        self.remote_sockets.add(socket.clone());

        // Once we detach NodeSocket's listener (see `take_remote_stream`) the raw socket has no error handler of its
        // own, and a socket that reports an error nobody listens to is a genuine uncaught exception. Every socket we
        // own must have one: destroying on error tears the socket down quietly and lets the agent evict it from the pool.
        let weak = Arc::downgrade(socket);
        socket.on_error(Arc::new(move |_error: &io::Error| {
            if let Some(socket) = weak.upgrade() {
                socket.destroy();
            }
        }));
        if self.disposed.load(Ordering::SeqCst) {
            socket.destroy();
        }
    }

    /// `_createTunnelSocket`: um soquete do túnel para o `http.Agent`.
    fn create_tunnel_socket(&self, host: &str, port: u16) -> Result<Arc<dyn Duplex>, Error> {
        let (stream, leftover) = self.open_tunnel(host, port)?;
        self.track_remote_socket(&stream);
        if !leftover.is_empty() {
            stream.unshift(leftover);
        }
        Ok(stream)
    }
}

struct ServerHandle {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
    connections: Arc<SocketSet>,
}

/// An HTTPS proxy server that routes TCP connections through the remote agent tunnel.
///
/// Handles:
/// - **CONNECT** requests (used by Chromium for HTTPS): establishes a raw TCP tunnel through the remote agent.
/// - **Plain HTTP** requests (GET, POST, etc. with absolute URLs): establishes a tunnel and forwards the request.
///
/// The server binds exclusively to `127.0.0.1` and is never exposed to the network: this is the primary security
/// boundary. The additional layers below are defence-in-depth:
///
/// - **TLS** with a self-signed certificate (generated in-memory) prevents other local processes from passively
///   sniffing traffic.
/// - **Basic proxy authentication** with randomly generated credentials prevents other local processes from actively
///   using the proxy.
/// - The certificate **fingerprint** is returned from [`TunnelProxy::start`] so the consumer's Electron session can pin
///   it.
///
/// If certificate generation or server startup fails the proxy simply does not start: the worst outcome is that the
/// browser view falls back to not having remote network access.
pub struct TunnelProxy {
    shared: Arc<Shared>,
    server: Mutex<Option<ServerHandle>>,
    local_port: AtomicU16,
}

impl TunnelProxy {
    pub fn new(connect_tunnel: ITunnelConnectFn, log: Arc<dyn ITunnelProxyLog>) -> Self {
        Self {
            shared: Arc::new(Shared {
                connect_tunnel,
                log,
                expected_auth_header: OnceLock::new(),
                agent: Mutex::new(None),
                connect_sockets: SocketSet::default(),
                remote_sockets: SocketSet::default(),
                connect_limiter: ConnectLimiter::new(MAX_CONCURRENT_TUNNEL_CONNECTS),
                disposed: AtomicBool::new(false),
            }),
            server: Mutex::new(None),
            local_port: AtomicU16::new(0),
        }
    }

    /// `localPort`.
    pub fn local_port(&self) -> u16 {
        self.local_port.load(Ordering::SeqCst)
    }

    pub fn start(&self) -> Result<ITunnelProxyInfo, Error> {
        // Generate random credentials
        let username = random_hex(16);
        let password = random_hex(32);
        let expected = format!("Basic {}", STANDARD.encode(format!("{username}:{password}")));
        if self.shared.expected_auth_header.set(expected).is_err() {
            return Err(Error::new("TunnelProxy already started"));
        }

        // Generate a self-signed certificate in memory
        let cert = generate_self_signed_cert()?;
        let tls_config = build_tls_config(&cert)?;

        // Create an agent that pools tunnel sockets by host:port.
        *lock(&self.shared.agent) = Some(Arc::new(TunnelAgent::default()));

        // HTTPS server: handles plain HTTP requests (absolute-form URLs from Chromium when configured as a proxy) and
        // CONNECT tunnels for HTTPS.
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(io_error)?;
        let port = listener.local_addr().map_err(io_error)?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let connections = Arc::new(SocketSet::default());
        let thread = std::thread::Builder::new()
            .name("je tunnel proxy".to_owned())
            .spawn({
                let (shared, stop, connections) = (self.shared.clone(), stop.clone(), connections.clone());
                move || accept_loop(&listener, &shared, &tls_config, &stop, &connections)
            })
            .map_err(io_error)?;
        *lock(&self.server) = Some(ServerHandle { port, stop, thread, connections });
        self.local_port.store(port, Ordering::SeqCst);
        self.shared.log.info(&format!("[TunnelProxy] Listening on https://127.0.0.1:{port}"));

        Ok(ITunnelProxyInfo {
            url: format!("https://127.0.0.1:{port}"),
            host: "127.0.0.1".to_owned(),
            port,
            credentials: ITunnelProxyCredentials { username, password },
            cert_fingerprint: cert.fingerprint,
        })
    }

    /// `dispose()`. Any tunnels still queued behind the limiter are abandoned here: they fail with an error and their
    /// sockets are destroyed below, so nothing is left waiting on a tunnel that will never arrive.
    pub fn dispose(&self) {
        let shared = &self.shared;
        if shared.disposed.swap(true, Ordering::SeqCst) {
            return;
        }
        shared.connect_sockets.destroy_all();
        shared.remote_sockets.destroy_all();
        let agent = lock(&shared.agent).take();
        if let Some(agent) = agent {
            agent.destroy();
        }
        let server = lock(&self.server).take();
        if let Some(server) = server {
            // Acorda o `accept` com uma conexão (que o laço descarta ao ver o sinal de parada) e espera a thread
            // soltar o `TcpListener`: ao voltar daqui a porta já recusa conexões.
            server.stop.store(true, Ordering::SeqCst);
            let _ = TcpStream::connect(("127.0.0.1", server.port));
            server.connections.destroy_all();
            let _ = server.thread.join();
        }
        shared.connect_limiter.dispose();
    }

    /// Drop every pooled keep-alive tunnel socket by recreating the agent. Called when the upstream tunnel endpoint
    /// changes: the pooled sockets all dial the now-stale endpoint, so they would be reset en masse once it goes away.
    /// Recreating the agent drops the pool and forces subsequent requests to dial the new endpoint.
    pub fn drain_connection_pool(&self) {
        let old_agent = {
            let mut agent = lock(&self.shared.agent);
            let Some(old_agent) = agent.take() else {
                return; // not started yet; nothing pooled
            };
            *agent = Some(Arc::new(TunnelAgent::default()));
            old_agent
        };
        old_agent.destroy();
        self.shared.log.trace("[TunnelProxy] Upstream endpoint changed; drained pooled tunnel sockets");
    }
}

impl Drop for TunnelProxy {
    fn drop(&mut self) {
        self.dispose();
    }
}

fn random_hex(byte_count: usize) -> String {
    let mut bytes = vec![0u8; byte_count];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn build_tls_config(cert: &ISelfSignedCert) -> Result<Arc<rustls::ServerConfig>, Error> {
    let cert_der = pem_to_der(&cert.cert)?;
    let key_der = pem_to_der(&cert.key)?;
    let provider = Arc::new(rustls_rustcrypto::provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| Error::new(format!("TLS setup failed: {error}")))?
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(cert_der)], PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_der)))
        .map_err(|error| Error::new(format!("TLS setup failed: {error}")))?;
    Ok(Arc::new(config))
}

fn accept_loop(listener: &TcpListener, shared: &Arc<Shared>, tls_config: &Arc<rustls::ServerConfig>, stop: &Arc<AtomicBool>, connections: &Arc<SocketSet>) {
    loop {
        let accepted = listener.accept();
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match accepted {
            Ok((tcp, _)) => {
                let (connection_shared, connection_config, connection_stop, connection_set) = (shared.clone(), tls_config.clone(), stop.clone(), connections.clone());
                let spawned = std::thread::Builder::new()
                    .name("je tunnel proxy connection".to_owned())
                    .spawn(move || serve_connection(tcp, &connection_shared, connection_config, &connection_set, &connection_stop));
                if let Err(error) = spawned {
                    shared.log.error(&format!("[TunnelProxy] Server error: {error}"));
                }
            }
            Err(error) => {
                shared.log.error(&format!("[TunnelProxy] Server error: {error}"));
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn serve_connection(tcp: TcpStream, shared: &Shared, tls_config: Arc<rustls::ServerConfig>, connections: &SocketSet, stop: &AtomicBool) {
    let _ = tcp.set_nodelay(true);
    let tls = match TlsServerStream::new(tcp, tls_config) {
        Ok(tls) => tls,
        Err(error) => {
            shared.log.error(&format!("[TunnelProxy] Server error: {error}"));
            return;
        }
    };
    let local: Arc<dyn Duplex> = tls.clone();
    connections.add(local.clone());
    if stop.load(Ordering::SeqCst) {
        tls.destroy();
        return;
    }
    let graceful = serve_requests(shared, &tls, &local, connections);
    connections.remove(&local);
    if graceful && !tls.is_destroyed() {
        // Quem encerrou a conexão foi o proxy: dá ao cliente um tempo para ler a resposta antes de derrubar.
        let _ = tls.set_read_timeout(Some(LINGER_TIMEOUT));
        let mut sink = [0u8; 1024];
        while matches!(tls.read(&mut sink), Ok(count) if count > 0) {}
    }
    tls.destroy();
}

/// Atende as requisições de uma conexão do cliente. Devolve `true` se a conexão terminou de forma ordenada (o proxy a
/// encerrou) e `false` se deve ser derrubada na hora.
fn serve_requests(shared: &Shared, tls: &Arc<TlsServerStream>, local: &Arc<dyn Duplex>, connections: &SocketSet) -> bool {
    let mut reader = WireReader::new(local.as_ref());
    let mut first = true;
    loop {
        let _ = tls.set_read_timeout(Some(if first { HEADERS_TIMEOUT } else { KEEP_ALIVE_TIMEOUT }));
        first = false;
        let head_bytes = match reader.read_head(&mut || {
            let _ = tls.set_read_timeout(Some(HEADERS_TIMEOUT));
        }) {
            Ok(Some(bytes)) => bytes,
            _ => return false,
        };
        let _ = tls.set_read_timeout(Some(REQUEST_TIMEOUT));
        let Some(request) = parse_request_head(&head_bytes) else {
            let _ = write_simple_response(local.as_ref(), 400, &[], false);
            return true;
        };
        if request.method == "CONNECT" {
            let _ = tls.set_read_timeout(None);
            let head = reader.into_leftover();
            // The server no longer tracks the socket once the CONNECT handler runs.
            connections.remove(local);
            on_connect(shared, &request, local, head);
            return true;
        }
        match on_request(shared, &request, &mut reader, local.as_ref()) {
            Ok(true) => {}
            Ok(false) => return true,
            Err(_) => return false,
        }
    }
}

/// Verify the `Proxy-Authorization` header against our credentials. Returns `true` if the request is authorized.
fn check_auth(shared: &Shared, auth_header: Option<&str>) -> bool {
    match (auth_header, shared.expected_auth_header.get()) {
        (Some(header), Some(expected)) => constant_time_eq(header.as_bytes(), expected.as_bytes()),
        _ => false,
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && left.iter().zip(right).fold(0u8, |difference, (a, b)| difference | (a ^ b)) == 0
}

/// Uma resposta sem corpo (`res.writeHead(status, extra); res.end()`).
fn write_simple_response(local: &dyn Duplex, status: u16, extra: &[(&str, &str)], keep_alive: bool) -> io::Result<()> {
    let mut headers = Headers::default();
    for (name, value) in extra {
        headers.push(name, *value);
    }
    push_connection_headers(&mut headers, keep_alive);
    headers.push("Content-Length", "0");
    local.write_all(&serialize_response_head(status, &headers))
}

/// O `Date`, o `Connection` e o `Keep-Alive` que o `http.Server` do Node acrescenta quando a resposta não os traz.
fn push_connection_headers(headers: &mut Headers, keep_alive: bool) {
    if !headers.contains("date") {
        headers.push("Date", http_date());
    }
    if !headers.contains("connection") {
        headers.push("Connection", if keep_alive { "keep-alive" } else { "close" });
        if keep_alive && !headers.contains("keep-alive") {
            headers.push("Keep-Alive", format!("timeout={}", KEEP_ALIVE_TIMEOUT.as_secs()));
        }
    }
}

/// Handle HTTP CONNECT requests (used for HTTPS tunneling). Parses `host:port` from the request URL, establishes a
/// tunnel through the remote agent, and pipes the sockets together.
fn on_connect(shared: &Shared, request: &RequestHead, local: &Arc<dyn Duplex>, head: Vec<u8>) {
    // Track the socket from the moment the CONNECT is received so dispose can tear it down even before the upstream
    // tunnel returns (or if auth/host validation fails).
    shared.connect_sockets.add(local.clone());
    if shared.disposed.load(Ordering::SeqCst) {
        local.destroy();
        return;
    }
    handle_connect(shared, request, local, &head);
    shared.connect_sockets.remove(local);
}

fn handle_connect(shared: &Shared, request: &RequestHead, local: &Arc<dyn Duplex>, head: &[u8]) {
    let finish = |response: &str| {
        let _ = local.write_all(response.as_bytes());
        local.end();
    };

    if !check_auth(shared, request.headers.get("proxy-authorization")) {
        finish("HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"TunnelProxy\"\r\n\r\n");
        return;
    }

    let (host, port) = parse_host_port(&request.target, 443);
    if host.is_empty() {
        finish("HTTP/1.1 400 Bad Request\r\n\r\n");
        return;
    }

    shared.log.trace(&format!("[TunnelProxy] CONNECT {host}:{port}"));

    let remote = match shared.open_tunnel(&host, port) {
        Err(error) => Err(error.message),
        Ok((remote, leftover)) => {
            let handshake = (|| -> io::Result<()> {
                local.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
                if !leftover.is_empty() {
                    local.write_all(&leftover)?;
                }
                if !head.is_empty() {
                    remote.write_all(head)?;
                }
                Ok(())
            })();
            match handshake {
                Ok(()) => Ok(remote),
                Err(error) => {
                    remote.destroy();
                    Err(error.to_string())
                }
            }
        }
    };
    match remote {
        Ok(remote) => bridge_sockets(shared, local, &remote),
        Err(message) => {
            shared.log.error(&format!("[TunnelProxy] Failed to tunnel to {host}:{port}: {message}"));
            finish("HTTP/1.1 502 Bad Gateway\r\n\r\n");
        }
    }
}

/// Liga os dois soquetes (`pipe` nos dois sentidos) e só volta quando os dois sentidos terminaram.
fn bridge_sockets(shared: &Shared, local: &Arc<dyn Duplex>, remote: &Arc<dyn Duplex>) {
    shared.track_remote_socket(remote);

    let (from, to) = (remote.clone(), local.clone());
    let downstream = std::thread::Builder::new().name("je tunnel proxy bridge".to_owned()).spawn(move || pump(from.as_ref(), to.as_ref()));
    match downstream {
        Ok(downstream) => {
            pump(local.as_ref(), remote.as_ref());
            let _ = downstream.join();
        }
        Err(error) => shared.log.error(&format!("[TunnelProxy] Failed to bridge sockets: {error}")),
    }
    local.destroy();
    remote.destroy();
}

/// `from.pipe(to)` com o fim e o erro do `_bridgeSockets`: o fim do fluxo encerra a escrita do outro lado; um erro (de
/// leitura em `from` ou de escrita em `to`) derruba o outro lado.
fn pump(from: &dyn Duplex, to: &dyn Duplex) {
    let mut buffer = vec![0u8; PUMP_BUFFER];
    loop {
        match from.read(&mut buffer) {
            Ok(0) => {
                to.end();
                return;
            }
            Ok(count) => {
                if to.write_all(&buffer[..count]).is_err() {
                    from.destroy();
                    return;
                }
            }
            Err(_) => {
                to.destroy();
                return;
            }
        }
    }
}

/// Parse a `host:port` string. Falls back to `default_port` when the port component is missing. Returns an empty host
/// when the address is empty or the port is outside the valid TCP range (1-65535), per RFC 9110 section 9.3.6 ("A
/// server MUST reject a CONNECT request that targets an empty or invalid port number").
fn parse_host_port(address: &str, default_port: u16) -> (String, u16) {
    let default = i64::from(default_port);
    // Handle IPv6 bracket notation [::1]:port
    let (host, port) = parse_bracketed(address, default).unwrap_or_else(|| match address.rfind(':') {
        None => (address.to_owned(), default),
        Some(last_colon) => match js_parse_int(&address[last_colon + 1..]) {
            // Likely an IPv6 address without brackets
            None => (address.to_owned(), default),
            Some(port) => (address[..last_colon].to_owned(), port),
        },
    });

    // Validate port range
    match u16::try_from(port) {
        Ok(port) if port >= 1 => (host, port),
        _ => (String::new(), 0),
    }
}

/// `/^\[(?<host>[^\]]+)\]:(?<port>\d+)$/` e `/^\[(?<host>[^\]]+)\]$/`.
fn parse_bracketed(address: &str, default: i64) -> Option<(String, i64)> {
    let rest = address.strip_prefix('[')?;
    let close = rest.find(']')?;
    let host = &rest[..close];
    if host.is_empty() {
        return None;
    }
    let tail = &rest[close + 1..];
    if tail.is_empty() {
        return Some((host.to_owned(), default));
    }
    let digits = tail.strip_prefix(':')?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((host.to_owned(), digits.parse::<i64>().unwrap_or(i64::MAX)))
}

/// `parseInt(text, 10)`: os dígitos do começo (com sinal e espaços à esquerda); `None` é o `NaN`.
fn js_parse_int(text: &str) -> Option<i64> {
    let trimmed = text.trim_start();
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    let value = digits[..end].parse::<i64>().unwrap_or(i64::MAX);
    Some(if negative { -value } else { value })
}

/// Handle plain HTTP requests (GET, POST, etc. with absolute URLs).
///
/// Chromium sends proxied HTTP requests with absolute-form URLs (e.g. `GET http://example.com/page HTTP/1.1`) and
/// reuses keep-alive connections to the proxy for requests to **different** hosts.
///
/// Each request is forwarded over a tunnel socket pooled by host:port by the [`TunnelAgent`], which creates a tunnel only
/// when no pooled socket is available; otherwise it reuses an existing tunnel connection.
///
/// Devolve se a conexão do cliente continua aberta; um erro derruba a conexão do cliente.
fn on_request(shared: &Shared, request: &RequestHead, reader: &mut WireReader<'_>, local: &dyn Duplex) -> io::Result<bool> {
    let framing = match request_framing(&request.headers) {
        Ok(framing) => framing,
        Err(()) => {
            write_simple_response(local, 400, &[], false)?;
            return Ok(false);
        }
    };
    // Uma resposta sem encaminhar não pode deixar um corpo não lido na conexão.
    let can_keep_alive = request.keep_alive() && framing == BodyFraming::None;
    let reject = |status: u16, extra: &[(&str, &str)]| -> io::Result<bool> {
        write_simple_response(local, status, extra, can_keep_alive)?;
        Ok(can_keep_alive)
    };

    if !check_auth(shared, request.headers.get("proxy-authorization")) {
        return reject(407, &[("Proxy-Authenticate", "Basic realm=\"TunnelProxy\"")]);
    }

    let Ok(parsed) = ::url::Url::parse(&request.target) else {
        return reject(400, &[]);
    };

    // Plain HTTP forwarding only: HTTPS goes through CONNECT. In practice every HTTP/1.1 client (browsers included) uses
    // CONNECT for HTTPS via a proxy, so an absolute-form `https:` URL here should never happen. Reject loudly rather than
    // silently misforward it as plaintext (a request to either the URL's port or default 80 would produce confusing
    // failures or wrong content).
    if parsed.scheme() != "http" {
        shared.log.warn(&format!("[TunnelProxy] Rejecting non-HTTP forwarded request: {} {}", request.method, request.target));
        return reject(400, &[]);
    }

    let host = match parsed.host_str() {
        Some(host) if !host.is_empty() => host.to_owned(),
        _ => return reject(400, &[]),
    };
    // `parseInt(parsed.port, 10) || 80`
    let port = match parsed.port_or_known_default() {
        Some(0) | None => 80,
        Some(port) => port,
    };

    shared.log.trace(&format!("[TunnelProxy] {} {host}:{port}{}", request.method, parsed.path()));

    let mut path = parsed.path().to_owned();
    if let Some(query) = parsed.query().filter(|query| !query.is_empty()) {
        path.push('?');
        path.push_str(query);
    }
    let (headers, encoding) = forwarded_headers(request, &host, port, framing);
    let forward = Forward { request, headers: &headers, path: &path, framing, encoding };

    let Some(agent) = shared.current_agent() else {
        return Err(io::Error::other("TunnelProxy is not running"));
    };
    let key: AgentKey = (host.clone(), port);
    let retryable = framing == BodyFraming::None;
    let mut reuse_allowed = true;
    loop {
        let free_socket = if reuse_allowed { agent.take_free(&key) } else { None };
        let reused = free_socket.is_some();
        let upstream = match free_socket {
            Some(socket) => socket,
            None => match shared.create_tunnel_socket(&host, port) {
                Ok(socket) => {
                    agent.register(&socket);
                    socket
                }
                Err(error) => {
                    // A rejection here fails the request (the client connection is reset) rather than hanging or silently
                    // returning nothing. Chromium renders a 502 body as a page, whereas a transport reset triggers
                    // `did-fail-load`, so the browser shows its native "failed to load" error page (consistent with the
                    // HTTPS/CONNECT path).
                    shared.log.error(&format!("[TunnelProxy] Failed to tunnel to {host}:{port}: {}", error.message));
                    return Err(io::Error::other(error.message));
                }
            },
        };

        match forward_once(&forward, reader, local, upstream.as_ref()) {
            Ok(outcome) => {
                if outcome.upstream_reusable {
                    agent.release(key, upstream);
                } else {
                    upstream.destroy();
                }
                return Ok(outcome.client_keep_alive);
            }
            Err(ForwardError::Stale(_)) if reused && retryable => {
                upstream.destroy();
                reuse_allowed = false;
            }
            Err(ForwardError::Stale(error) | ForwardError::Failed(error)) => {
                upstream.destroy();
                shared.log.error(&format!("[TunnelProxy] Proxy request error for {host}:{port}: {error}"));
                // Reset the client connection instead of returning a 502 body.
                return Err(error);
            }
        }
    }
}

/// Os cabeçalhos da requisição encaminhada e como o corpo sai.
fn forwarded_headers(request: &RequestHead, host: &str, port: u16, framing: BodyFraming) -> (Headers, BodyEncoding) {
    let mut headers = request.headers.clone();

    // Strip hop-by-hop headers per RFC 9110 Section 7.6.1.
    // An intermediary MUST parse the Connection header and remove any
    // fields named in it, then remove Connection itself. It SHOULD
    // also remove other known hop-by-hop headers.
    for token in headers.tokens("connection") {
        headers.remove(&token);
    }
    for name in HOP_BY_HOP_HEADERS {
        headers.remove(name);
    }

    // O que o `http.request` do Node acrescenta: o `Host` (se faltar), o `Connection` do agente com keep-alive e o
    // enquadramento do corpo.
    if !headers.contains("host") {
        headers.push("host", if port == 80 { host.to_owned() } else { format!("{host}:{port}") });
    }
    headers.push("connection", "keep-alive");
    let encoding = match framing {
        BodyFraming::Length(length) => {
            headers.set("content-length", length.to_string());
            BodyEncoding::Identity
        }
        BodyFraming::Chunked => {
            headers.remove("content-length");
            headers.push("transfer-encoding", "chunked");
            BodyEncoding::Chunked
        }
        BodyFraming::None | BodyFraming::UntilClose => {
            if !["GET", "HEAD", "DELETE", "OPTIONS", "TRACE", "CONNECT"].contains(&request.method.as_str()) {
                headers.set("content-length", "0");
            }
            BodyEncoding::Identity
        }
    };
    (headers, encoding)
}

/// A requisição que `on_request` encaminha.
struct Forward<'a> {
    request: &'a RequestHead,
    headers: &'a Headers,
    path: &'a str,
    framing: BodyFraming,
    encoding: BodyEncoding,
}

enum ForwardError {
    /// Falhou antes de qualquer byte da resposta chegar (a escrita do cabeçalho ou a leitura do cabeçalho da resposta).
    Stale(io::Error),
    Failed(io::Error),
}

struct Outcome {
    client_keep_alive: bool,
    upstream_reusable: bool,
}

/// Falhas de um soquete que o outro lado já tinha fechado.
fn is_stale(error: &io::Error) -> bool {
    use io::ErrorKind;
    matches!(error.kind(), ErrorKind::UnexpectedEof | ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted | ErrorKind::BrokenPipe | ErrorKind::NotConnected)
}

fn forward_once(forward: &Forward<'_>, reader: &mut WireReader<'_>, local: &dyn Duplex, upstream: &dyn Duplex) -> Result<Outcome, ForwardError> {
    upstream.write_all(&serialize_request_head(&forward.request.method, forward.path, forward.headers)).map_err(ForwardError::Stale)?;

    if forward.framing != BodyFraming::None {
        let mut writer = BodyWriter::new(upstream, forward.encoding);
        reader.copy_body(forward.framing, &mut |chunk| writer.write(chunk)).map_err(ForwardError::Failed)?;
        writer.finish().map_err(ForwardError::Failed)?;
    }

    let mut upstream_reader = WireReader::new(upstream);
    let response = upstream_reader.read_response_head().map_err(|error| if is_stale(&error) { ForwardError::Stale(error) } else { ForwardError::Failed(error) })?;
    relay_response(forward.request, &response, &mut upstream_reader, local).map_err(ForwardError::Failed)
}

/// `res.writeHead(proxyRes.statusCode, proxyRes.headers); proxyRes.pipe(res)`: a resposta do destino volta ao cliente com
/// o status e os cabeçalhos dele, e o corpo é repassado conforme o enquadramento.
fn relay_response(request: &RequestHead, response: &ResponseHead, upstream: &mut WireReader<'_>, local: &dyn Duplex) -> io::Result<Outcome> {
    let framing = response_framing(&request.method, response.status, &response.headers);
    let upstream_wants_close = response.headers.tokens("connection").iter().any(|token| token == "close");
    let upstream_alive = if response.minor_version >= 1 { !upstream_wants_close } else { response.headers.tokens("connection").iter().any(|token| token == "keep-alive") };

    let mut headers = response.headers.clone();
    let mut client_keep_alive = request.keep_alive() && !upstream_wants_close;
    let encoding = match framing {
        BodyFraming::Chunked => {
            headers.remove("content-length");
            BodyEncoding::Chunked
        }
        // Sem enquadramento da origem o `http.Server` do Node usa `chunked` (HTTP/1.1) ou fecha a conexão (HTTP/1.0).
        BodyFraming::UntilClose if request.minor_version >= 1 => {
            headers.set("transfer-encoding", "chunked");
            BodyEncoding::Chunked
        }
        BodyFraming::UntilClose => {
            client_keep_alive = false;
            BodyEncoding::Identity
        }
        BodyFraming::None | BodyFraming::Length(_) => BodyEncoding::Identity,
    };
    push_connection_headers(&mut headers, client_keep_alive);

    local.write_all(&serialize_response_head(response.status, &headers))?;
    let mut writer = BodyWriter::new(local, encoding);
    upstream.copy_body(framing, &mut |chunk| writer.write(chunk))?;
    writer.finish()?;

    Ok(Outcome { client_keep_alive, upstream_reusable: upstream_alive && framing != BodyFraming::UntilClose && upstream.is_drained() })
}
