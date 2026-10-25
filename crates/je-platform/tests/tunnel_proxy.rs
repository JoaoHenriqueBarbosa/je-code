//! Porte de `src/vs/platform/tunnel/test/node/tunnelProxy.test.ts`.
//!
//! O teste sobe sockets locais como o original: um servidor HTTP de eco como destino e, no lugar do agente remoto, funções
//! de conexão que discam o destino local. O cliente do proxy é um cliente TLS do `rustls` sem verificação de
//! certificado (o `rejectUnauthorized: false` do original), com HTTP/1.1 escrito à mão.
//!
//! Adaptações ao Rust: o proxy usa threads e E/S bloqueante, então os eventos dos `net.Socket` do original viram consultas
//! (`NodeTcpSocket::destroyed`, `wait_closed`, `emit_error`) e o "aguardar o `close`" do cliente é ler até o fim do fluxo.
//! `ensureNoDisposablesAreLeakedInTestSuite` não tem equivalente: cada proxy é descartado ao sair do escopo (`Drop`).

#![cfg(not(target_arch = "wasm32"))]

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use je_base::errors::Error;
use je_base::parts::ipc::net::SocketCloseEvent;
use je_platform::tunnel::tunnel_proxy::{ITunnelConnectFn, ITunnelProtocol, NullTunnelProxyLog, TunnelProxy};
use je_platform::tunnel::tunnel_proxy_info::ITunnelProxyInfo;
use je_platform::tunnel::tunnel_socket::{CloseListener, DataListener, EndListener, IManagedSocket, NodeSocket, NodeTcpSocket, TunnelSocket, Unsubscribe};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, SignatureScheme};

/// O prazo de cada leitura do cliente: um teste que trava falha em vez de pendurar.
const IO_TIMEOUT: Duration = Duration::from_secs(20);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

// #region Servidores HTTP de destino

/// Um servidor HTTP em `127.0.0.1` (o `http.createServer` do original), que responde como o Node responde a um
/// `writeHead` seguido de `end(texto)`: `Transfer-Encoding: chunked` e keep-alive.
struct TargetServer {
    port: u16,
    stop: Arc<AtomicBool>,
}

struct TargetRequest {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
}

impl TargetServer {
    fn start(content_type: &'static str, respond: impl Fn(&TargetRequest) -> String + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind the target server");
        let port = listener.local_addr().expect("target address").port();
        let stop = Arc::new(AtomicBool::new(false));
        let respond = Arc::new(respond);
        thread::spawn({
            let stop = stop.clone();
            move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let respond = respond.clone();
                    thread::spawn(move || serve_target_connection(stream, content_type, &*respond));
                }
            }
        });
        Self { port, stop }
    }
}

impl Drop for TargetServer {
    /// `targetServer.close()`.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn serve_target_connection(mut stream: TcpStream, content_type: &str, respond: &(dyn Fn(&TargetRequest) -> String + Send + Sync)) {
    let mut buffer: Vec<u8> = Vec::new();
    loop {
        let head_end = loop {
            if let Some(position) = find(&buffer, b"\r\n\r\n") {
                break position + 4;
            }
            let mut chunk = [0u8; 4096];
            match stream.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(count) => buffer.extend_from_slice(&chunk[..count]),
            }
        };
        let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
        buffer.drain(..head_end);

        let mut lines = head.split("\r\n");
        let mut request_line = lines.next().unwrap_or("").split(' ');
        let method = request_line.next().unwrap_or("").to_owned();
        let url = request_line.next().unwrap_or("").to_owned();
        let headers: Vec<(String, String)> =
            lines.take_while(|line| !line.is_empty()).filter_map(|line| line.split_once(':')).map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned())).collect();
        let close = headers.iter().any(|(name, value)| name == "connection" && value.split(',').any(|token| token.trim().eq_ignore_ascii_case("close")));

        let body = respond(&TargetRequest { method, url, headers });
        let connection = if close { "close" } else { "keep-alive\r\nKeep-Alive: timeout=5" };
        let response = format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nConnection: {connection}\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n", body.len());
        if stream.write_all(response.as_bytes()).is_err() || close {
            let _ = stream.shutdown(std::net::Shutdown::Both);
            return;
        }
    }
}

fn escape_json(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `JSON.stringify(req.headers)`.
fn headers_json(headers: &[(String, String)]) -> String {
    let fields: Vec<String> = headers.iter().map(|(name, value)| format!("\"{}\":\"{}\"", escape_json(name), escape_json(value))).collect();
    format!("{{{}}}", fields.join(","))
}

/// `JSON.parse(body)[name]`, para um objeto plano de textos.
fn json_field(json: &str, name: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", escape_json(name));
    let start = json.find(&needle)? + needle.len();
    let mut value = String::new();
    let mut chars = json[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => value.push(chars.next()?),
            '"' => return Some(value),
            other => value.push(other),
        }
    }
    None
}

/// O servidor de eco da suíte (`suiteSetup`): responde `ECHO <método> <url>`.
fn target_port() -> u16 {
    static TARGET: OnceLock<TargetServer> = OnceLock::new();
    TARGET.get_or_init(|| TargetServer::start("text/plain", |request| format!("ECHO {} {}", request.method, request.url))).port
}

// #endregion

// #region Túnel simulado

fn rejected(error: io::Error) -> Error {
    Error::new(error.to_string())
}

/// `mockTunnelProtocol` e o protocolo gerenciado: o formato que o `TunnelProxy` espera, emulando o agente remoto. O
/// handshake do túnel (o remoto confirmando que o destino está acessível) acontece dentro da função de conexão real, que
/// os testes do proxy substituem; chegar até aqui representa um túnel estabelecido. Um túnel que falha é simulado por uma
/// função de conexão que rejeita.
struct MockProtocol {
    socket: TunnelSocket,
}

impl ITunnelProtocol for MockProtocol {
    fn get_socket(&self) -> TunnelSocket {
        self.socket.clone()
    }

    fn read_entire_buffer(&self) -> Vec<u8> {
        Vec::new()
    }

    fn dispose(&self) {
        // NodeSocket owns the underlying socket
    }
}

fn connect_to_target(target_port: u16) -> Result<Arc<NodeTcpSocket>, Error> {
    TcpStream::connect(("127.0.0.1", target_port)).map(NodeTcpSocket::new).map_err(rejected)
}

fn mock_tunnel_protocol(socket: Arc<NodeTcpSocket>) -> Box<dyn ITunnelProtocol> {
    Box::new(MockProtocol { socket: TunnelSocket::Node(NodeSocket::new(socket)) })
}

/// `createMockConnectFn`: conecta a um servidor TCP local em vez de passar pelo agente remoto.
fn create_mock_connect_fn(target_port: u16) -> ITunnelConnectFn {
    Arc::new(move |_host: &str, _port: u16| -> Result<Box<dyn ITunnelProtocol>, Error> { Ok(mock_tunnel_protocol(connect_to_target(target_port)?)) })
}

/// Como `create_mock_connect_fn`, guardando cada soquete remoto entregue ao proxy.
fn capturing_connect_fn(target_port: u16, remote_sockets: Arc<Mutex<Vec<Arc<NodeTcpSocket>>>>) -> ITunnelConnectFn {
    Arc::new(move |_host: &str, _port: u16| -> Result<Box<dyn ITunnelProtocol>, Error> {
        let socket = connect_to_target(target_port)?;
        lock(&remote_sockets).push(socket.clone());
        Ok(mock_tunnel_protocol(socket))
    })
}

/// Uma lista de ouvintes que chama sem trava (os `Emitter` do original).
struct ListenerList<F: ?Sized> {
    entries: Mutex<Vec<(u64, Arc<F>)>>,
    next_id: AtomicU64,
}

impl<F: ?Sized + Send + Sync + 'static> ListenerList<F> {
    fn new() -> Arc<Self> {
        Arc::new(Self { entries: Mutex::new(Vec::new()), next_id: AtomicU64::new(0) })
    }

    fn add(self: &Arc<Self>, listener: Arc<F>) -> Unsubscribe {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        lock(&self.entries).push((id, listener));
        let list = self.clone();
        Box::new(move || lock(&list.entries).retain(|(entry_id, _)| *entry_id != id))
    }

    fn snapshot(&self) -> Vec<Arc<F>> {
        lock(&self.entries).iter().map(|(_, listener)| listener.clone()).collect()
    }
}

/// `ManagedTestSocket`: um `ISocket` genérico que **não** é um `NodeSocket`, sobre um `TcpStream`, para emular um transporte
/// gerenciado / de exec-server. Chegar ao `TunnelProxy` por ele exercita o adaptador `RemoteSocketStream` (o proxy nunca
/// vê um soquete TCP) em vez do caminho rápido do soquete cru.
struct ManagedTestSocket {
    stream: TcpStream,
    disposed: AtomicBool,
    on_data: Arc<ListenerList<dyn Fn(&[u8]) + Send + Sync>>,
    on_close: Arc<ListenerList<dyn Fn(Option<&SocketCloseEvent>) + Send + Sync>>,
    on_end: Arc<ListenerList<dyn Fn() + Send + Sync>>,
}

impl ManagedTestSocket {
    fn new(stream: TcpStream) -> Arc<Self> {
        let reader = stream.try_clone().expect("clone the target stream");
        let socket = Arc::new(Self { stream, disposed: AtomicBool::new(false), on_data: ListenerList::new(), on_close: ListenerList::new(), on_end: ListenerList::new() });
        thread::spawn({
            let socket = socket.clone();
            move || socket.read_loop(reader)
        });
        socket
    }

    fn read_loop(&self, mut reader: TcpStream) {
        let mut buffer = vec![0u8; 16 * 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    self.fire_end();
                    self.fire_close(false);
                    return;
                }
                Ok(count) => self.fire_data(&buffer[..count]),
                // Swallow transport errors; they surface to the proxy as a close event.
                Err(_) => {
                    self.fire_close(true);
                    return;
                }
            }
        }
    }

    fn fire_data(&self, data: &[u8]) {
        if !self.is_disposed() {
            for listener in self.on_data.snapshot() {
                listener(data);
            }
        }
    }

    fn fire_end(&self) {
        if !self.is_disposed() {
            for listener in self.on_end.snapshot() {
                listener();
            }
        }
    }

    fn fire_close(&self, had_error: bool) {
        if !self.is_disposed() {
            let event = SocketCloseEvent::Node { had_error, error: None };
            for listener in self.on_close.snapshot() {
                listener(Some(&event));
            }
        }
    }

    fn is_disposed(&self) -> bool {
        self.disposed.load(Ordering::SeqCst)
    }
}

impl IManagedSocket for ManagedTestSocket {
    fn on_data(&self, listener: DataListener) -> Unsubscribe {
        self.on_data.add(Arc::from(listener))
    }

    fn on_end(&self, listener: EndListener) -> Unsubscribe {
        self.on_end.add(Arc::from(listener))
    }

    fn on_close(&self, listener: CloseListener) -> Unsubscribe {
        self.on_close.add(Arc::from(listener))
    }

    fn write(&self, buffer: &[u8]) {
        let _ = (&self.stream).write_all(buffer);
    }

    fn end(&self) {
        let _ = self.stream.shutdown(std::net::Shutdown::Write);
    }

    fn drain(&self) -> Result<(), Error> {
        Ok(())
    }

    fn dispose(&self) {
        self.disposed.store(true, Ordering::SeqCst);
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }
}

/// `createManagedConnectFn`: como `create_mock_connect_fn`, mas apresenta o túnel como um `ISocket` genérico (transporte
/// gerenciado / de exec-server) em vez de um `NodeSocket`, então o proxy o liga pelo adaptador `RemoteSocketStream`. O
/// `dispose` do protocolo é um no-op porque o adaptador é dono do soquete gerenciado e o descarta ao ser destruído.
fn create_managed_connect_fn(target_port: u16, captured: Option<Arc<Mutex<Option<Arc<ManagedTestSocket>>>>>) -> ITunnelConnectFn {
    Arc::new(move |_host: &str, _port: u16| -> Result<Box<dyn ITunnelProtocol>, Error> {
        let stream = TcpStream::connect(("127.0.0.1", target_port)).map_err(rejected)?;
        let managed = ManagedTestSocket::new(stream);
        if let Some(captured) = &captured {
            *lock(captured) = Some(managed.clone());
        }
        Ok(Box::new(MockProtocol { socket: TunnelSocket::Generic(managed) }))
    })
}

// #endregion

// #region Cliente do proxy

/// `rejectUnauthorized: false`: aceita o certificado autoassinado do proxy.
#[derive(Debug)]
struct AcceptAnyCertificate;

impl ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(&self, _end_entity: &CertificateDer<'_>, _intermediates: &[CertificateDer<'_>], _server_name: &ServerName<'_>, _ocsp_response: &[u8], _now: UnixTime) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(&self, _message: &[u8], _cert: &CertificateDer<'_>, _dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(&self, _message: &[u8], _cert: &CertificateDer<'_>, _dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ECDSA_NISTP256_SHA256, SignatureScheme::ECDSA_NISTP384_SHA384, SignatureScheme::ED25519, SignatureScheme::RSA_PSS_SHA256, SignatureScheme::RSA_PKCS1_SHA256]
    }
}

type TlsClient = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

/// `tls.connect({ host: '127.0.0.1', port, rejectUnauthorized: false })`, com o handshake já feito.
fn tls_connect(port: u16) -> io::Result<TlsClient> {
    let tcp = TcpStream::connect(("127.0.0.1", port))?;
    tcp.set_read_timeout(Some(IO_TIMEOUT))?;
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls_rustcrypto::provider()))
        .with_safe_default_protocol_versions()
        .map_err(io::Error::other)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyCertificate))
        .with_no_client_auth();
    let server_name = ServerName::try_from("127.0.0.1").map_err(io::Error::other)?;
    let connection = rustls::ClientConnection::new(Arc::new(config), server_name).map_err(io::Error::other)?;
    let mut client = rustls::StreamOwned::new(connection, tcp);
    while client.conn.is_handshaking() {
        client.conn.complete_io(&mut client.sock)?;
    }
    Ok(client)
}

fn auth_header(info: &ITunnelProxyInfo) -> String {
    format!("Basic {}", STANDARD.encode(format!("{}:{}", info.credentials.username, info.credentials.password)))
}

#[derive(Debug)]
struct ProxyResponse {
    status_code: u16,
    #[allow(dead_code)]
    headers: Vec<(String, String)>,
    body: String,
}

fn fill(client: &mut TlsClient, buffer: &mut Vec<u8>) -> io::Result<()> {
    let mut chunk = [0u8; 8192];
    let count = client.read(&mut chunk)?;
    if count == 0 {
        return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
    }
    buffer.extend_from_slice(&chunk[..count]);
    Ok(())
}

/// Lê até `needle` e devolve o índice logo depois dele.
fn read_until(client: &mut TlsClient, buffer: &mut Vec<u8>, needle: &[u8]) -> io::Result<usize> {
    loop {
        if let Some(position) = find(buffer, needle) {
            return Ok(position + needle.len());
        }
        fill(client, buffer)?;
    }
}

fn take(client: &mut TlsClient, buffer: &mut Vec<u8>, count: usize) -> io::Result<Vec<u8>> {
    while buffer.len() < count {
        fill(client, buffer)?;
    }
    Ok(buffer.drain(..count).collect())
}

fn read_chunked_body(client: &mut TlsClient, buffer: &mut Vec<u8>) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let line_end = read_until(client, buffer, b"\r\n")?;
        let line: Vec<u8> = buffer.drain(..line_end).collect();
        let size = usize::from_str_radix(String::from_utf8_lossy(&line).trim(), 16).map_err(io::Error::other)?;
        if size == 0 {
            take(client, buffer, 2)?;
            return Ok(body);
        }
        body.extend(take(client, buffer, size)?);
        take(client, buffer, 2)?;
    }
}

/// A resposta do proxy: o status, os cabeçalhos (em minúsculas) e o corpo, delimitado pelo `Content-Length`, por `chunked` ou
/// pelo fim da conexão.
fn read_response(client: &mut TlsClient) -> io::Result<ProxyResponse> {
    let mut buffer = Vec::new();
    let head_end = read_until(client, &mut buffer, b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    buffer.drain(..head_end);

    let mut lines = head.split("\r\n");
    let status_code: u16 = lines.next().and_then(|line| line.split(' ').nth(1)).and_then(|status| status.parse().ok()).ok_or_else(|| io::Error::other("malformed status line"))?;
    let headers: Vec<(String, String)> = lines.take_while(|line| !line.is_empty()).filter_map(|line| line.split_once(':')).map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned())).collect();
    let header = |name: &str| headers.iter().find(|(existing, _)| existing == name).map(|(_, value)| value.clone());

    let body = if header("transfer-encoding").is_some_and(|value| value.contains("chunked")) {
        read_chunked_body(client, &mut buffer)?
    } else if let Some(length) = header("content-length").and_then(|value| value.parse::<usize>().ok()) {
        take(client, &mut buffer, length)?
    } else {
        let mut rest = buffer;
        loop {
            match fill(client, &mut rest) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break rest,
                Err(error) => return Err(error),
            }
        }
    };
    Ok(ProxyResponse { status_code, headers, body: String::from_utf8_lossy(&body).into_owned() })
}

fn send_request(client: &mut TlsClient, info: &ITunnelProxyInfo, method: &str, path: &str, auth: bool, headers: &[(&str, &str)]) -> io::Result<()> {
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n", info.port);
    if method != "GET" {
        request.push_str("Content-Length: 0\r\n");
    }
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if auth {
        request.push_str(&format!("Proxy-Authorization: {}\r\n", auth_header(info)));
    }
    request.push_str("\r\n");
    client.write_all(request.as_bytes())?;
    client.flush()
}

/// `proxyRequest`: make an HTTPS request to the proxy, skipping cert verification (self-signed). Returns the response
/// status code and body.
fn proxy_request(info: &ITunnelProxyInfo, method: Option<&str>, path: &str, auth: bool, headers: &[(&str, &str)]) -> io::Result<ProxyResponse> {
    let mut client = tls_connect(info.port)?;
    send_request(&mut client, info, method.unwrap_or("GET"), path, auth, headers)?;
    read_response(&mut client)
}

/// `proxyConnect`: open a TLS connection to the proxy, send a raw CONNECT request, and return the response status code
/// and the TLS socket for further I/O.
fn proxy_connect(info: &ITunnelProxyInfo, target: &str, auth: bool) -> io::Result<(u16, TlsClient)> {
    let mut client = tls_connect(info.port)?;
    let mut request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
    if auth {
        request.push_str(&format!("Proxy-Authorization: {}\r\n", auth_header(info)));
    }
    request.push_str("\r\n");
    client.write_all(request.as_bytes())?;
    client.flush()?;

    let mut buffer = Vec::new();
    let head_end = read_until(&mut client, &mut buffer, b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let status_code = head.split("\r\n").next().and_then(|line| line.split(' ').nth(1)).and_then(|status| status.parse().ok()).ok_or_else(|| io::Error::other("malformed status line"))?;
    Ok((status_code, client))
}

/// Lê tudo o que o túnel devolve até o `'end'` (um erro rejeita, como o `socket.on('error', reject)`).
fn read_tunnel_to_end(client: &mut TlsClient) -> io::Result<String> {
    let mut body = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match client.read(&mut chunk)? {
            0 => return Ok(String::from_utf8_lossy(&body).into_owned()),
            count => body.extend_from_slice(&chunk[..count]),
        }
    }
}

/// O `await closed` de `socket.once('close')`: lê até o fluxo acabar ou cair, e falha se nada acontecer no prazo.
fn wait_for_close(client: &mut TlsClient) {
    let mut chunk = [0u8; 1024];
    loop {
        match client.read(&mut chunk) {
            Ok(0) => return,
            Ok(_) => {}
            Err(error) => {
                assert!(!matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut), "the connection was not closed in time: {error}");
                return;
            }
        }
    }
}

fn read_tlv(input: &[u8]) -> (&[u8], &[u8]) {
    let (length, header) = if input[1] < 0x80 {
        (usize::from(input[1]), 2)
    } else {
        let count = usize::from(input[1] & 0x7f);
        (input[2..2 + count].iter().fold(0usize, |length, byte| (length << 8) | usize::from(*byte)), 2 + count)
    };
    (&input[header..header + length], &input[header + length..])
}

/// `cert.subject.CN`: o `CN` do assunto de um certificado X.509 em DER.
fn subject_common_name(certificate: &[u8]) -> Option<String> {
    let (certificate, _) = read_tlv(certificate);
    let (tbs, _) = read_tlv(certificate);
    // [0] version, serial, signature algorithm, issuer, validity, subject
    let mut rest = tbs;
    for _ in 0..5 {
        rest = read_tlv(rest).1;
    }
    let (subject, _) = read_tlv(rest);
    let (relative_name, _) = read_tlv(subject);
    let (attribute, _) = read_tlv(relative_name);
    let (oid, value) = read_tlv(attribute);
    let (value, _) = read_tlv(value);
    (oid == [0x55, 0x04, 0x03]).then(|| String::from_utf8_lossy(value).into_owned())
}

// #endregion

fn new_proxy(connect: ITunnelConnectFn) -> TunnelProxy {
    TunnelProxy::new(connect, Arc::new(NullTunnelProxyLog))
}

/// O `setup` da suíte: um proxy novo, ligado ao servidor de eco, e o que `start` devolveu. O proxy é descartado (o
/// `teardown`) ao sair do escopo.
struct Fixture {
    _proxy: TunnelProxy,
    info: ITunnelProxyInfo,
}

impl Fixture {
    fn new() -> Self {
        let proxy = new_proxy(create_mock_connect_fn(target_port()));
        let info = proxy.start().expect("start the proxy");
        Self { _proxy: proxy, info }
    }
}

fn target_url(path: &str) -> String {
    format!("http://127.0.0.1:{}{path}", target_port())
}

// --- ITunnelProxyInfo shape ---

#[test]
fn start_returns_a_valid_i_tunnel_proxy_info() {
    let fixture = Fixture::new();
    let info = &fixture.info;
    assert_eq!(info.host, "127.0.0.1");
    assert!(info.port > 0 && u32::from(info.port) < 65536);
    assert_eq!(info.url, format!("https://127.0.0.1:{}", info.port));
    assert!(!info.credentials.username.is_empty());
    assert!(!info.credentials.password.is_empty());
    assert!(info.cert_fingerprint.starts_with("sha256/"));
}

// --- TLS ---

#[test]
fn server_uses_tls() {
    let fixture = Fixture::new();
    let client = tls_connect(fixture.info.port).expect("TLS connection");
    assert!(!client.conn.is_handshaking() && client.conn.protocol_version().is_some());
    let certificates = client.conn.peer_certificates().expect("the peer certificate");
    assert_eq!(subject_common_name(certificates[0].as_ref()).as_deref(), Some("TunnelProxy"));
}

// --- Authentication ---

#[test]
fn rejects_plain_http_request_without_credentials_407() {
    let fixture = Fixture::new();
    let res = proxy_request(&fixture.info, None, &target_url("/hello"), false, &[]).expect("response");
    assert_eq!(res.status_code, 407);
}

#[test]
fn rejects_connect_without_credentials_407() {
    let fixture = Fixture::new();
    let (status_code, _socket) = proxy_connect(&fixture.info, &format!("127.0.0.1:{}", target_port()), false).expect("CONNECT");
    assert_eq!(status_code, 407);
}

// --- Plain HTTP forwarding ---

#[test]
fn forwards_authenticated_http_get_to_target() {
    let fixture = Fixture::new();
    let res = proxy_request(&fixture.info, None, &target_url("/some/path"), true, &[]).expect("response");
    assert_eq!(res.status_code, 200);
    assert_eq!(res.body, "ECHO GET /some/path");
}

#[test]
fn forwards_authenticated_http_post_to_target() {
    let fixture = Fixture::new();
    let res = proxy_request(&fixture.info, Some("POST"), &target_url("/post"), true, &[]).expect("response");
    assert_eq!(res.status_code, 200);
    assert_eq!(res.body, "ECHO POST /post");
}

#[test]
fn strips_hop_by_hop_headers_from_forwarded_request() {
    // Use a target that echoes all received headers as JSON
    let header_server = TargetServer::start("application/json", |request| headers_json(&request.headers));
    let header_port = header_server.port;

    let proxy2 = new_proxy(create_mock_connect_fn(header_port));
    let info2 = proxy2.start().expect("start the proxy");

    let res = proxy_request(
        &info2,
        None,
        &format!("http://127.0.0.1:{header_port}/"),
        true,
        &[
            ("Connection", "keep-alive, X-Custom-Hop"),
            ("Keep-Alive", "timeout=5"),
            ("Proxy-Connection", "keep-alive"),
            ("TE", "trailers"),
            ("Upgrade", "websocket"),
            ("X-Custom-Hop", "should-be-removed"),
            ("X-End-To-End", "should-survive"),
        ],
    )
    .expect("response");
    assert_eq!(res.status_code, 200);
    let forwarded = |name: &str| json_field(&res.body, name);
    // All hop-by-hop headers MUST/SHOULD be removed
    assert_eq!(forwarded("proxy-authorization"), None);
    assert_eq!(forwarded("proxy-connection"), None);
    assert_eq!(forwarded("keep-alive"), None);
    assert_eq!(forwarded("te"), None);
    assert_eq!(forwarded("upgrade"), None);
    // Headers named in Connection must also be removed
    assert_eq!(forwarded("x-custom-hop"), None);
    // Note: connection itself is replaced by the agent with its own value (e.g. "keep-alive"), which is correct per
    // RFC 9110: the proxy replaces it with its own connection options.
    // End-to-end headers must survive
    assert_eq!(forwarded("x-end-to-end").as_deref(), Some("should-survive"));
    proxy2.dispose();
}

#[test]
fn returns_400_for_malformed_url() {
    let fixture = Fixture::new();
    let res = proxy_request(&fixture.info, None, "not-a-valid-url", true, &[]).expect("response");
    assert_eq!(res.status_code, 400);
}

// --- Agent connection pooling ---

#[test]
fn reuses_tunnel_socket_for_multiple_requests_to_the_same_host() {
    let connect_count = Arc::new(AtomicUsize::new(0));
    let counting_connect: ITunnelConnectFn = Arc::new({
        let connect_count = connect_count.clone();
        move |_host: &str, _port: u16| -> Result<Box<dyn ITunnelProtocol>, Error> {
            connect_count.fetch_add(1, Ordering::SeqCst);
            Ok(mock_tunnel_protocol(connect_to_target(target_port())?))
        }
    });
    let pool_proxy = new_proxy(counting_connect);
    let pool_info = pool_proxy.start().expect("start the proxy");

    // Send three sequential requests to the same host:port
    for i in 0..3 {
        let res = proxy_request(&pool_info, None, &target_url(&format!("/req{i}")), true, &[]).expect("response");
        assert_eq!(res.status_code, 200);
        assert_eq!(res.body, format!("ECHO GET /req{i}"));
    }

    // The agent should have opened only one tunnel connection
    let count = connect_count.load(Ordering::SeqCst);
    assert_eq!(count, 1, "Expected 1 tunnel connection, got {count}");
    pool_proxy.dispose();
}

#[test]
fn drain_connection_pool_destroys_pooled_tunnel_sockets() {
    // Capture the upstream socket the agent pools so we can assert it is dropped when the upstream endpoint changes.
    let remote_sockets = Arc::new(Mutex::new(Vec::new()));
    let p = new_proxy(capturing_connect_fn(target_port(), remote_sockets.clone()));
    let info = p.start().expect("start the proxy");

    // One request pools one keep-alive tunnel socket.
    let res = proxy_request(&info, None, &target_url("/"), true, &[]).expect("response");
    assert_eq!(res.status_code, 200);
    let sockets = lock(&remote_sockets).clone();
    assert_eq!(sockets.len(), 1);
    assert!(!sockets[0].destroyed());

    // Simulating an upstream endpoint change must drop the now-stale pooled socket so it isn't reset later by the dead
    // endpoint.
    p.drain_connection_pool();
    assert!(sockets[0].wait_closed(IO_TIMEOUT));
    assert!(sockets[0].destroyed());

    p.dispose();
}

#[test]
fn a_reset_on_a_pooled_tunnel_socket_does_not_escalate_to_an_uncaught_exception() {
    // Capture the pooled upstream socket so we can simulate the upstream endpoint resetting it.
    let remote_sockets = Arc::new(Mutex::new(Vec::new()));
    let p = new_proxy(capturing_connect_fn(target_port(), remote_sockets.clone()));
    let info = p.start().expect("start the proxy");

    let res = proxy_request(&info, None, &target_url("/"), true, &[]).expect("response");
    assert_eq!(res.status_code, 200);
    let sockets = lock(&remote_sockets).clone();
    assert_eq!(sockets.len(), 1);

    // When the upstream endpoint dies, the pooled socket is reset. The proxy takes ownership of the raw socket (detaching
    // NodeSocket's listener, which would otherwise route the error through the unexpected-error handler) and attaches its
    // own error handler, so the reset is contained rather than thrown or reported as an unexpected error. `emit_error`
    // returns `false` exactly when the error would have been thrown (no listener).
    assert!(sockets[0].emit_error(&io::Error::other("simulated upstream reset")));

    p.dispose();
}

// --- CONNECT tunneling ---

#[test]
fn connect_establishes_a_tunnel_to_the_target() {
    let fixture = Fixture::new();
    let (status_code, mut socket) = proxy_connect(&fixture.info, &format!("127.0.0.1:{}", target_port()), true).expect("CONNECT");
    assert_eq!(status_code, 200);

    // Send a raw HTTP request through the tunnel
    socket.write_all(format!("GET /tunneled HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", target_port()).as_bytes()).expect("write through the tunnel");
    socket.flush().expect("flush");
    let body = read_tunnel_to_end(&mut socket).expect("tunnel response");
    assert!(body.contains("ECHO GET /tunneled"), "Expected tunneled echo, got: {body}");
}

#[test]
fn connect_rejects_invalid_port_0() {
    let fixture = Fixture::new();
    let (status_code, _socket) = proxy_connect(&fixture.info, "127.0.0.1:0", true).expect("CONNECT");
    assert_eq!(status_code, 400);
}

#[test]
fn connect_rejects_port_greater_than_65535() {
    let fixture = Fixture::new();
    let (status_code, _socket) = proxy_connect(&fixture.info, "127.0.0.1:99999", true).expect("CONNECT");
    assert_eq!(status_code, 400);
}

// --- Error handling ---

#[test]
fn fails_the_request_when_the_tunnel_connection_fails() {
    // A failed tunnel - whether the remote agent itself is unreachable or the remote reports (via the handshake) that the
    // target host:port is unreachable - surfaces here as a rejected connect function.
    let failing_connect: ITunnelConnectFn = Arc::new(|_host: &str, _port: u16| -> Result<Box<dyn ITunnelProtocol>, Error> { Err(Error::new("connect ECONNREFUSED 127.0.0.1:9999")) });
    let fail_proxy = new_proxy(failing_connect);
    let fail_info = fail_proxy.start().expect("start the proxy");

    // Plain HTTP request: the client connection is reset (no HTTP response) so the browser shows its native error page.
    assert!(proxy_request(&fail_info, None, "http://unreachable.example.com/path", true, &[]).is_err());

    // CONNECT should fail with a 502 (which the browser surfaces as a native tunnel error page).
    let (status_code, _socket) = proxy_connect(&fail_info, "unreachable.example.com:443", true).expect("CONNECT");
    assert_eq!(status_code, 502);

    fail_proxy.dispose();
}

// --- Lifecycle ---

#[test]
fn dispose_shuts_down_the_server() {
    let p = new_proxy(create_mock_connect_fn(target_port()));
    let info = p.start().expect("start the proxy");
    p.dispose();

    // Connection should be refused after dispose
    let error = proxy_request(&info, None, &target_url("/"), true, &[]).expect_err("the connection must be refused");
    assert_eq!(error.kind(), io::ErrorKind::ConnectionRefused, "{error}");
}

#[test]
fn dispose_terminates_active_connect_tunnels() {
    let p = new_proxy(create_mock_connect_fn(target_port()));
    let info = p.start().expect("start the proxy");

    // Open a CONNECT tunnel and keep it open (no end/destroy).
    let (status_code, mut socket) = proxy_connect(&info, &format!("127.0.0.1:{}", target_port()), true).expect("CONNECT");
    assert_eq!(status_code, 200);

    p.dispose();

    // The previously-active CONNECT socket must be force-closed by dispose; without explicit teardown of these sockets,
    // closing the server alone would leave the port bound indefinitely.
    wait_for_close(&mut socket);
}

#[test]
fn dispose_synchronously_destroys_the_remote_tunnel_socket() {
    // Capture the remote (upstream) socket handed out by the tunnel so we can assert dispose tears it down directly,
    // rather than relying on the local socket's close to propagate.
    let remote_sockets = Arc::new(Mutex::new(Vec::new()));
    let p = new_proxy(capturing_connect_fn(target_port(), remote_sockets.clone()));
    let info = p.start().expect("start the proxy");

    let (status_code, _socket) = proxy_connect(&info, &format!("127.0.0.1:{}", target_port()), true).expect("CONNECT");
    assert_eq!(status_code, 200);
    let sockets = lock(&remote_sockets).clone();
    assert_eq!(sockets.len(), 1);

    p.dispose();

    // The remote socket must be destroyed by the time dispose returns: no extra event-loop turn required.
    assert!(sockets[0].destroyed());
}

#[test]
fn dispose_terminates_connect_sockets_stuck_waiting_for_the_upstream_tunnel() {
    // Mock connect that never resolves: simulates a slow/hung upstream tunnel. The CONNECT socket sits in limbo between
    // the request arriving and the upstream returning, and must still be torn down by dispose. Dropping `release` at the
    // end lets the hung call return so its thread does not outlive the test.
    let (release, held) = mpsc::channel::<()>();
    let held = Mutex::new(held);
    let (connect_called, connect_called_signal) = mpsc::channel::<()>();
    let hanging_connect: ITunnelConnectFn = Arc::new(move |_host: &str, _port: u16| -> Result<Box<dyn ITunnelProtocol>, Error> {
        let _ = connect_called.send(());
        let _ = lock(&held).recv();
        Err(Error::new("the hung tunnel was released"))
    });
    let p = new_proxy(hanging_connect);
    let info = p.start().expect("start the proxy");

    let mut client_socket = tls_connect(info.port).expect("TLS connection");
    let request = format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nProxy-Authorization: {}\r\n\r\n", auth_header(&info), port = target_port());
    client_socket.write_all(request.as_bytes()).expect("write the CONNECT");
    client_socket.flush().expect("flush");

    // Wait until the proxy has entered the hanging upstream call so the socket is registered as a CONNECT socket.
    connect_called_signal.recv_timeout(IO_TIMEOUT).expect("the connect function was never called");

    p.dispose();
    wait_for_close(&mut client_socket);
    drop(release);
}

#[test]
fn dispose_terminates_idle_https_keep_alive_connections() {
    let p = new_proxy(create_mock_connect_fn(target_port()));
    let info = p.start().expect("start the proxy");

    // Send one request with keep-alive so the client/server pair holds the TLS connection open after the response.
    // Without closing every server connection on dispose, this socket would linger until either side timed out.
    let mut client = tls_connect(info.port).expect("TLS connection");
    send_request(&mut client, &info, "GET", &target_url("/keepalive"), true, &[]).expect("request");
    let res = read_response(&mut client).expect("response");
    assert_eq!(res.status_code, 200);

    p.dispose();

    wait_for_close(&mut client);
}

// --- Managed (non-NodeSocket) transport ---
//
// Exec-server / managed connections yield a generic ISocket with no underlying raw socket, so the proxy bridges them
// through its RemoteSocketStream Duplex adapter instead of the raw-socket fast path.

/// O `setup` da suíte `managed (non-NodeSocket) transport`.
fn managed_fixture() -> (TunnelProxy, ITunnelProxyInfo) {
    let managed_proxy = new_proxy(create_managed_connect_fn(target_port(), None));
    let managed_info = managed_proxy.start().expect("start the proxy");
    (managed_proxy, managed_info)
}

#[test]
fn managed_forwards_an_authenticated_http_get_through_a_managed_socket() {
    let (_proxy, managed_info) = managed_fixture();
    let res = proxy_request(&managed_info, None, &target_url("/managed/path"), true, &[]).expect("response");
    assert_eq!(res.status_code, 200);
    assert_eq!(res.body, "ECHO GET /managed/path");
}

#[test]
fn managed_connect_tunnels_bidirectional_data_through_a_managed_socket() {
    let (_proxy, managed_info) = managed_fixture();
    let (status_code, mut socket) = proxy_connect(&managed_info, &format!("127.0.0.1:{}", target_port()), true).expect("CONNECT");
    assert_eq!(status_code, 200);

    // Write a request up the tunnel and read the echoed response back down it, proving the adapter bridges both
    // directions.
    socket.write_all(format!("GET /managed-tunnel HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", target_port()).as_bytes()).expect("write through the tunnel");
    socket.flush().expect("flush");
    let body = read_tunnel_to_end(&mut socket).expect("tunnel response");
    assert!(body.contains("ECHO GET /managed-tunnel"), "Expected tunneled echo, got: {body}");
}

#[test]
fn managed_dispose_disposes_the_managed_remote_socket_via_the_adapter() {
    let captured: Arc<Mutex<Option<Arc<ManagedTestSocket>>>> = Arc::new(Mutex::new(None));
    let p = new_proxy(create_managed_connect_fn(target_port(), Some(captured.clone())));
    let info = p.start().expect("start the proxy");

    let (status_code, _socket) = proxy_connect(&info, &format!("127.0.0.1:{}", target_port()), true).expect("CONNECT");
    assert_eq!(status_code, 200);
    let managed = lock(&captured).clone().expect("the managed socket was captured");

    p.dispose();

    // Destroying the adapter (RemoteSocketStream) on dispose must dispose the underlying managed socket, mirroring how the
    // NodeSocket path destroys the raw socket.
    assert!(managed.is_disposed());
}
