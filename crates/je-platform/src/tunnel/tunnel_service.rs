//! Porte de `src/vs/platform/tunnel/node/tunnelService.ts`: o `NodeRemoteTunnel`, que escuta uma porta local e leva cada
//! conexão que chega pelo túnel do agente remoto (`createRemoteTunnel` com as três tentativas contra as portas que o
//! navegador restringe).
//!
//! Decisões de tradução:
//! - O original roda no laço de eventos do Node (`net.Server`, `net.Socket`, `pipe`). Aqui a E/S é bloqueante com `std::net`
//!   e threads, como no `TunnelProxy`: uma thread aceita as conexões, uma thread atende cada uma e cada ponte tem a sua
//!   thread de leitura. `net.Socket` é [`NodeTcpSocket`], o `NodeSocket` é [`NodeSocket`] e o `ISocket` genérico é
//!   [`IManagedSocket`].
//! - `connectRemoteAgentTunnel(options, host, port)` é a [`ITunnelConnectFn`] injetada (a mesma do `TunnelProxy`): o
//!   `remoteAgentConnection` ainda não foi portado, então quem cria o túnel entrega a função que já carrega as opções.
//! - O `write` do `net.Socket` do Node guarda o que não coube no kernel e o `end()` só fecha depois de esvaziar esse buffer;
//!   [`LocalSocketWriter`] é essa fila (com o `writableLength`). A ponte genérica escreve por ela; a ponte de `NodeSocket`
//!   escreve direto (o `pipe` do Node respeita a contrapressão, aqui o próprio bloqueio da escrita).
//! - Os eventos `'end'`, `'close'` e `'error'` do soquete local viram o desfecho da thread que o lê ([`LocalReadOutcome`]),
//!   tratado ao final de `on_connection`.
//! - `localSocket.localAddress` é só o IP local (sem a porta), então todas as conexões de um túnel caem na mesma chave de
//!   `_socketsDispose`, e o `dispose` do original só encerra a última: o porte mantém o comportamento.
//! - Uma conexão cujo túnel remoto falha (`await connectRemoteAgentTunnel` rejeita) deixa, no original, o soquete local
//!   pausado e esquecido; aqui ele é destruído, que é o que o cliente enxergaria quando o soquete é coletado.
//! - `findFreePortFaster` (`base/node/ports.ts`) é [`find_free_port_faster`] e `BROWSER_RESTRICTED_PORTS` é
//!   [`is_browser_restricted_port`]; o prazo (`timeout`) do original não existe, porque o `bind` não espera.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use je_base::errors::Error;
use je_base::parts::ipc::net::SocketCloseEvent;

use super::tunnel_proxy::ITunnelConnectFn;
use super::tunnel_socket::{Duplex, IManagedSocket, NodeTcpSocket, TunnelSocket, lock};
use super::{TunnelPrivacyId, is_all_interfaces, is_localhost};

/// O tamanho de cada leitura do soquete local.
const PUMP_BUFFER: usize = 64 * 1024;

/// Quantas vezes `createRemoteTunnel` tenta obter uma porta local que o navegador não restrinja.
const CREATE_ATTEMPTS: usize = 3;

/// Reference: <https://chromium.googlesource.com/chromium/src.git/+/refs/heads/main/net/base/port_util.cc#56>
const BROWSER_RESTRICTED_PORTS: &[u16] = &[
    1,     // tcpmux
    7,     // echo
    9,     // discard
    11,    // systat
    13,    // daytime
    15,    // netstat
    17,    // qotd
    19,    // chargen
    20,    // ftp data
    21,    // ftp access
    22,    // ssh
    23,    // telnet
    25,    // smtp
    37,    // time
    42,    // name
    43,    // nicname
    53,    // domain
    69,    // tftp
    77,    // priv-rjs
    79,    // finger
    87,    // ttylink
    95,    // supdup
    101,   // hostriame
    102,   // iso-tsap
    103,   // gppitnp
    104,   // acr-nema
    109,   // pop2
    110,   // pop3
    111,   // sunrpc
    113,   // auth
    115,   // sftp
    117,   // uucp-path
    119,   // nntp
    123,   // NTP
    135,   // loc-srv /epmap
    137,   // netbios
    139,   // netbios
    143,   // imap2
    161,   // snmp
    179,   // BGP
    389,   // ldap
    427,   // SLP (Also used by Apple Filing Protocol)
    465,   // smtp+ssl
    512,   // print / exec
    513,   // login
    514,   // shell
    515,   // printer
    526,   // tempo
    530,   // courier
    531,   // chat
    532,   // netnews
    540,   // uucp
    548,   // AFP (Apple Filing Protocol)
    554,   // rtsp
    556,   // remotefs
    563,   // nntp+ssl
    587,   // smtp (rfc6409)
    601,   // syslog-conn (rfc3195)
    636,   // ldap+ssl
    989,   // ftps-data
    990,   // ftps
    993,   // ldap+ssl
    995,   // pop3+ssl
    1719,  // h323gatestat
    1720,  // h323hostcall
    1723,  // pptp
    2049,  // nfs
    3659,  // apple-sasl / PasswordServer
    4045,  // lockd
    5060,  // sip
    5061,  // sips
    6000,  // X11
    6566,  // sane-port
    6665,  // Alternate IRC [Apple addition]
    6666,  // Alternate IRC [Apple addition]
    6667,  // Standard IRC [Apple addition]
    6668,  // Alternate IRC [Apple addition]
    6669,  // Alternate IRC [Apple addition]
    6697,  // IRC + TLS
    10080, // Amanda
];

/// `BROWSER_RESTRICTED_PORTS[port]`.
pub fn is_browser_restricted_port(port: u16) -> bool {
    BROWSER_RESTRICTED_PORTS.contains(&port)
}

/// `findFreePortFaster`: uses listen instead of connect. Is faster, but if there is another listener on 0.0.0.0 then this
/// will take 127.0.0.1 from that listener. Tenta `start_port` e as `give_up_after - 1` portas seguintes; devolve `0` quando
/// nenhuma abre.
pub fn find_free_port_faster(start_port: u16, give_up_after: u32, hostname: &str) -> u16 {
    let mut port = start_port;
    let mut count_tried = 1;
    loop {
        match TcpListener::bind((hostname, port)) {
            Ok(_) => return port,
            Err(error) if matches!(error.kind(), io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied) && count_tried < give_up_after => {
                let Some(next) = port.checked_add(1) else { return 0 };
                port = next;
                count_tried += 1;
            }
            Err(_) => return 0,
        }
    }
}

/// A fila de escrita do soquete local: o `write`/`end`/`destroy` do `net.Socket` com o buffer em memória do Node. O que não
/// cabe no kernel espera na fila (o `writableLength`); `end` fecha a escrita só depois de esvaziá-la, e `destroy` aborta.
pub struct LocalSocketWriter {
    socket: Arc<NodeTcpSocket>,
    sender: Mutex<Option<Sender<WriterCommand>>>,
    queued: Arc<AtomicUsize>,
}

enum WriterCommand {
    Data(Vec<u8>),
    End,
}

impl LocalSocketWriter {
    fn new(socket: Arc<NodeTcpSocket>) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel();
        let queued = Arc::new(AtomicUsize::new(0));
        thread::spawn({
            let socket = socket.clone();
            let queued = queued.clone();
            move || drain_writer_queue(&socket, &receiver, &queued)
        });
        Arc::new(Self { socket, sender: Mutex::new(Some(sender)), queued })
    }

    /// `write(data)`: enfileira; depois de `end` ou `destroy` o dado é descartado.
    pub fn write(&self, data: &[u8]) {
        if let Some(sender) = lock(&self.sender).as_ref() {
            self.queued.fetch_add(data.len(), Ordering::SeqCst);
            if sender.send(WriterCommand::Data(data.to_vec())).is_err() {
                self.queued.fetch_sub(data.len(), Ordering::SeqCst);
            }
        }
    }

    /// `end()`: fecha a escrita depois de enviar tudo o que está na fila. Chamado de novo, não faz nada.
    pub fn end(&self) {
        if let Some(sender) = lock(&self.sender).take() {
            let _ = sender.send(WriterCommand::End);
        }
    }

    /// `destroy()`: descarta o que falta e derruba o soquete na hora.
    pub fn destroy(&self) {
        lock(&self.sender).take();
        self.socket.destroy();
    }

    /// `writableLength`: os bytes que ainda não chegaram ao kernel.
    pub fn writable_length(&self) -> usize {
        self.queued.load(Ordering::SeqCst)
    }
}

fn drain_writer_queue(socket: &NodeTcpSocket, receiver: &Receiver<WriterCommand>, queued: &AtomicUsize) {
    while let Ok(command) = receiver.recv() {
        match command {
            WriterCommand::Data(data) => {
                let written = socket.write_all(&data);
                queued.fetch_sub(data.len(), Ordering::SeqCst);
                if written.is_err() {
                    return;
                }
            }
            WriterCommand::End => {
                socket.end();
                return;
            }
        }
    }
}

/// Como a leitura do soquete local terminou: o `'end'` (`Ended`) ou o `'error'` (`Failed`).
#[derive(Debug)]
pub enum LocalReadOutcome {
    Ended,
    Failed(io::Error),
}

/// `localSocket.on('data', ...)` mais `resume()`: lê o soquete local até o fim, entregando cada pedaço a `forward`.
fn spawn_local_reader(local: Arc<NodeTcpSocket>, mut forward: impl FnMut(&[u8]) -> io::Result<()> + Send + 'static) -> JoinHandle<LocalReadOutcome> {
    thread::spawn(move || {
        let mut buffer = vec![0u8; PUMP_BUFFER];
        loop {
            match local.read(&mut buffer) {
                Ok(0) => return LocalReadOutcome::Ended,
                Ok(count) => {
                    if let Err(error) = forward(&buffer[..count]) {
                        return LocalReadOutcome::Failed(error);
                    }
                }
                Err(error) => return LocalReadOutcome::Failed(error),
            }
        }
    })
}

/// A ponte entre um soquete local e o soquete remoto, o que `_mirrorGenericSocket` e `_mirrorNodeSocket` deixam ligado.
pub struct Mirror {
    local: Arc<NodeTcpSocket>,
    writer: Option<Arc<LocalSocketWriter>>,
    local_read: Mutex<Option<JoinHandle<LocalReadOutcome>>>,
}

impl Mirror {
    /// `localSocket.writableLength` (sempre `0` na ponte de `NodeSocket`, que escreve direto).
    pub fn writable_length(&self) -> usize {
        self.writer.as_ref().map_or(0, |writer| writer.writable_length())
    }

    /// `localSocket.end()`.
    pub fn end_local(&self) {
        match &self.writer {
            Some(writer) => writer.end(),
            None => self.local.end(),
        }
    }

    /// Espera o fim da leitura do soquete local (o `'end'` ou o `'error'`).
    pub fn wait_local_read(&self) -> LocalReadOutcome {
        match lock(&self.local_read).take() {
            Some(handle) => handle.join().unwrap_or_else(|_| LocalReadOutcome::Failed(io::Error::other("the local socket reader panicked"))),
            None => LocalReadOutcome::Ended,
        }
    }
}

/// `_mirrorGenericSocket`: liga o soquete local a um `ISocket` genérico.
///
/// Na ponte pelo `ISocket` genérico, o fechamento do remoto aborta o soquete local quando veio com erro (para o cliente
/// local não tomar um fluxo truncado por completo); fora disso, o que está no buffer é enviado antes do `end`.
pub fn mirror_generic_socket(local: &Arc<NodeTcpSocket>, remote: &Arc<dyn IManagedSocket>) -> Arc<Mirror> {
    let writer = LocalSocketWriter::new(local.clone());
    let _ = remote.on_close(Box::new({
        let writer = writer.clone();
        move |event: Option<&SocketCloseEvent>| {
            // Abort on error so the local client can't mistake a truncated stream for a complete one; otherwise flush buffered data.
            if matches!(event, Some(SocketCloseEvent::Node { had_error: true, .. })) {
                writer.destroy();
            } else {
                writer.end();
            }
        }
    }));
    let _ = remote.on_end(Box::new({
        let writer = writer.clone();
        move || writer.end()
    }));
    let _ = remote.on_data(Box::new({
        let writer = writer.clone();
        move |data: &[u8]| writer.write(data)
    }));
    let local_read = spawn_local_reader(local.clone(), {
        let remote = remote.clone();
        move |data| {
            remote.write(data);
            Ok(())
        }
    });
    Arc::new(Mirror { local: local.clone(), writer: Some(writer), local_read: Mutex::new(Some(local_read)) })
}

/// `_mirrorNodeSocket`: liga o soquete local ao soquete cru do `NodeSocket` (o `pipe` nos dois sentidos).
fn mirror_node_socket(local: &Arc<NodeTcpSocket>, remote: Arc<NodeTcpSocket>) -> Arc<Mirror> {
    // remoteSocket.pipe(localSocket), com `'end'`/`'close'` do remoto encerrando o local e `'error'` derrubando-o.
    thread::spawn({
        let local = local.clone();
        let remote = remote.clone();
        move || {
            let mut buffer = vec![0u8; PUMP_BUFFER];
            loop {
                match remote.read(&mut buffer) {
                    Ok(0) => return local.end(),
                    Ok(count) => {
                        if local.write_all(&buffer[..count]).is_err() {
                            return;
                        }
                    }
                    Err(_) => return local.destroy(),
                }
            }
        }
    });
    // localSocket.pipe(remoteSocket)
    let local_read = spawn_local_reader(local.clone(), {
        let local = local.clone();
        move |data| {
            remote.write_all(data).inspect_err(|_| local.destroy())
        }
    });
    Arc::new(Mirror { local: local.clone(), writer: None, local_read: Mutex::new(Some(local_read)) })
}

/// `remoteSocket.end()`.
fn end_remote(remote: &TunnelSocket) {
    match remote {
        TunnelSocket::Node(socket) => socket.socket().end(),
        TunnelSocket::Generic(socket) => socket.end(),
    }
}

/// O que as threads de `NodeRemoteTunnel` compartilham.
struct Shared {
    tunnel_remote_host: String,
    tunnel_remote_port: u16,
    connect: ITunnelConnectFn,
    stopped: AtomicBool,
    /// `_socketsDispose`: por `localSocket.localAddress`, o que `dispose` faz com a conexão.
    sockets_dispose: Mutex<HashMap<String, Box<dyn FnOnce() + Send>>>,
}

impl Shared {
    /// `_onConnection`.
    fn on_connection(&self, local: Arc<NodeTcpSocket>) {
        // pause reading on the socket until we have a chance to forward its data (a leitura só começa na ponte)
        let tunnel_remote_host = if is_localhost(&self.tunnel_remote_host) || is_all_interfaces(&self.tunnel_remote_host) { "localhost" } else { self.tunnel_remote_host.as_str() };
        let protocol = match (self.connect)(tunnel_remote_host, self.tunnel_remote_port) {
            Ok(protocol) => protocol,
            Err(_) => return local.destroy(),
        };
        let remote = protocol.get_socket();
        let data_chunk = protocol.read_entire_buffer();
        protocol.dispose();

        if !data_chunk.is_empty() {
            let _ = local.write_all(&data_chunk);
        }

        let mirror = match &remote {
            TunnelSocket::Node(socket) => mirror_node_socket(&local, socket.socket()),
            TunnelSocket::Generic(socket) => mirror_generic_socket(&local, socket),
        };

        let key = local.local_address();
        if let Some(key) = &key {
            let mirror = mirror.clone();
            let remote = remote.clone();
            lock(&self.sockets_dispose).insert(
                key.clone(),
                Box::new(move || {
                    // Need to end instead of unpipe, otherwise whatever is connected locally could end up "stuck" with whatever state it had until manually exited.
                    mirror.end_local();
                    end_remote(&remote);
                }),
            );
        }

        let outcome = mirror.wait_local_read();
        if let Some(key) = &key {
            lock(&self.sockets_dispose).remove(key);
        }
        match (outcome, &remote) {
            // localSocket.on('end')
            (LocalReadOutcome::Ended, remote) => end_remote(remote),
            // localSocket.on('error')
            (LocalReadOutcome::Failed(_), TunnelSocket::Node(socket)) => socket.socket().destroy(),
            (LocalReadOutcome::Failed(_), TunnelSocket::Generic(socket)) => socket.end(),
        }
        // localSocket.on('close')
        end_remote(&remote);
    }
}

/// `NodeRemoteTunnel`: o servidor local de uma porta encaminhada (`RemoteTunnel`).
pub struct NodeRemoteTunnel {
    tunnel_remote_port: u16,
    tunnel_remote_host: String,
    default_tunnel_host: String,
    suggested_local_port: Option<u16>,
    shared: Arc<Shared>,
    ready: Mutex<Option<ReadyState>>,
}

/// O que `waitForReady` descobre depois de o servidor escutar.
struct ReadyState {
    tunnel_local_port: u16,
    local_address: String,
    listen_address: SocketAddr,
}

impl NodeRemoteTunnel {
    pub fn new(connect: ITunnelConnectFn, default_tunnel_host: &str, tunnel_remote_host: &str, tunnel_remote_port: u16, suggested_local_port: Option<u16>) -> Self {
        Self {
            tunnel_remote_port,
            tunnel_remote_host: tunnel_remote_host.to_owned(),
            default_tunnel_host: default_tunnel_host.to_owned(),
            suggested_local_port,
            shared: Arc::new(Shared {
                tunnel_remote_host: tunnel_remote_host.to_owned(),
                tunnel_remote_port,
                connect,
                stopped: AtomicBool::new(false),
                sockets_dispose: Mutex::new(HashMap::new()),
            }),
            ready: Mutex::new(None),
        }
    }

    pub fn tunnel_remote_port(&self) -> u16 {
        self.tunnel_remote_port
    }

    pub fn tunnel_remote_host(&self) -> &str {
        &self.tunnel_remote_host
    }

    /// `tunnelLocalPort`; `None` antes de `wait_for_ready`.
    pub fn tunnel_local_port(&self) -> Option<u16> {
        lock(&self.ready).as_ref().map(|state| state.tunnel_local_port)
    }

    /// `localAddress`; `None` antes de `wait_for_ready`.
    pub fn local_address(&self) -> Option<String> {
        lock(&self.ready).as_ref().map(|state| state.local_address.clone())
    }

    /// `privacy`.
    pub fn privacy(&self) -> TunnelPrivacyId {
        TunnelPrivacyId::Private
    }

    /// `dispose`: para de aceitar conexões e encerra as ativas.
    pub fn dispose(&self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        // `server.close()`: acorda a thread que aceita, que sai e solta a porta.
        if let Some(state) = lock(&self.ready).as_ref() {
            let mut address = state.listen_address;
            if address.ip().is_unspecified() {
                address.set_ip(IpAddr::from([127, 0, 0, 1]));
            }
            let _ = TcpStream::connect_timeout(&address, Duration::from_secs(1));
        }
        let disposers: Vec<Box<dyn FnOnce() + Send>> = lock(&self.shared.sockets_dispose).drain().map(|(_, disposer)| disposer).collect();
        for disposer in disposers {
            disposer();
        }
    }

    /// `waitForReady`: escolhe a porta local e começa a escutar.
    pub fn wait_for_ready(&self) -> Result<(), Error> {
        let start_port = self.suggested_local_port.unwrap_or(self.tunnel_remote_port);
        let hostname = if is_all_interfaces(&self.default_tunnel_host) { "0.0.0.0" } else { "127.0.0.1" };
        // try to get the same port number as the remote port number...
        let local_port = find_free_port_faster(start_port, 2, hostname);

        // if that fails, the method above returns 0, which works out fine below; it is also possible for it to return a
        // port that there is already a server listening on, which makes the listen call fail: ask the system for one.
        let host = self.default_tunnel_host.as_str();
        let listener = TcpListener::bind((host, local_port))
            .or_else(|_| TcpListener::bind((host, 0)))
            .map_err(|error| Error::new(format!("failed to listen on {host}: {error}")))?;
        let address = listener.local_addr().map_err(|error| Error::new(format!("failed to read the listening address: {error}")))?;

        *lock(&self.ready) = Some(ReadyState {
            tunnel_local_port: address.port(),
            local_address: format!("{}:{}", if self.tunnel_remote_host == "127.0.0.1" { "127.0.0.1" } else { "localhost" }, address.port()),
            listen_address: address,
        });

        let shared = self.shared.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                if shared.stopped.load(Ordering::SeqCst) {
                    break;
                }
                // If there is no error listener and there is an error it will crash the whole window: swallow it.
                let Ok(stream) = stream else {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                };
                let shared = shared.clone();
                thread::spawn(move || shared.on_connection(NodeTcpSocket::new(stream)));
            }
        });
        Ok(())
    }
}

/// `createRemoteTunnel`: até três tentativas de obter uma porta local que o navegador não restrinja (a menos que a porta
/// local pedida já seja uma delas).
pub fn create_remote_tunnel(connect: &ITunnelConnectFn, default_tunnel_host: &str, tunnel_remote_host: &str, tunnel_remote_port: u16, tunnel_local_port: Option<u16>) -> Result<NodeRemoteTunnel, Error> {
    let mut ready_tunnel: Option<NodeRemoteTunnel> = None;
    for _ in 0..CREATE_ATTEMPTS {
        if let Some(previous) = ready_tunnel.take() {
            previous.dispose();
        }
        let tunnel = NodeRemoteTunnel::new(connect.clone(), default_tunnel_host, tunnel_remote_host, tunnel_remote_port, tunnel_local_port);
        tunnel.wait_for_ready()?;
        let requested_is_restricted = tunnel_local_port.is_some_and(|port| port != 0 && is_browser_restricted_port(port));
        let ready_is_restricted = tunnel.tunnel_local_port().is_some_and(is_browser_restricted_port);
        ready_tunnel = Some(tunnel);
        if requested_is_restricted || !ready_is_restricted {
            break;
        }
    }
    ready_tunnel.ok_or_else(|| Error::new("no tunnel was created"))
}
