//! Porte de `src/vs/workbench/api/node/loopbackServer.ts`: o servidor HTTP de loopback do fluxo OAuth.
//!
//! Notas de tradução:
//! - O `http.createServer` do Node vira um `TcpListener` de `std::net` com uma thread de aceite e uma thread por conexão
//!   (leitura com limite de tempo). O servidor só escuta em `127.0.0.1`.
//! - As promessas viram [`Deferred`]: um valor que o primeiro `complete` ou `error` resolve e que `wait` bloqueia até
//!   existir. `start` e `stop` são síncronos (o `bind` já devolve com a porta conhecida, então não há o
//!   `Timeout waiting for port` de 5 s do original); o `server.close` espera a thread de aceite terminar.
//! - O `EADDRINUSE` do `error` do servidor vira o `AddrInUse` do `bind`: registra o erro no `ILogger` e tenta a porta 0.
//! - O `writeHead` do Node lança `ERR_INVALID_CHAR` quando o valor de um cabeçalho tem caractere fora de
//!   `[\t\x20-\x7e\x80-\xff]` (o caso do `error_description` com `%0d%0a`); ali a exceção estoura no ouvinte e a requisição
//!   fica sem resposta. Aqui a conexão recebe `500` e a promessa do OAuth não é resolvida, como no original.
//! - A resposta sai com `Content-Length` e `Connection: close` (o Node usaria `chunked` e `keep-alive`); o conteúdo observável
//!   é o mesmo.
//! - `randomBytes(16).toString('base64')` é `fill_bytes` mais `encode_base64` com preenchimento.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use je_base::buffer::{VSBuffer, encode_base64};
use je_base::oauth::{DEFAULT_AUTH_FLOW_PORT, UrlSearchParams};
use je_base::random::fill_bytes;
use je_base::strings::{escape, html_attribute_encode_value};
use je_base::uri::URI;
use je_platform::log::LoggerRef;

/// O erro das promessas do servidor (o `Error` do original): só a mensagem.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopbackError {
    pub message: String,
}

impl LoopbackError {
    fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

impl fmt::Display for LoopbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LoopbackError {}

/// `IOAuthResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthResult {
    pub code: String,
    pub state: String,
}

/// `ILoopbackServer`.
pub trait LoopbackServer {
    /// The state parameter used in the OAuth flow.
    fn state(&self) -> &str;
    /// Starts the server. Falha se o servidor não sobe ou já está iniciado.
    fn start(&self) -> Result<(), LoopbackError>;
    /// Stops the server. Falha se o servidor não fecha.
    fn stop(&self) -> Result<(), LoopbackError>;
    /// Bloqueia até o resultado do fluxo OAuth (o `waitForOAuthResponse` devolve uma promessa).
    fn wait_for_oauth_response(&self) -> Result<OAuthResult, LoopbackError>;
}

/// O `DeferredPromise` das respostas: o primeiro `complete` ou `error` vence, `wait` bloqueia até haver valor.
struct Deferred<T: Clone> {
    slot: Mutex<Option<Result<T, LoopbackError>>>,
    condvar: Condvar,
}

impl<T: Clone> Deferred<T> {
    fn new() -> Self {
        Self { slot: Mutex::new(None), condvar: Condvar::new() }
    }

    fn settle(&self, value: Result<T, LoopbackError>) {
        let mut slot = self.slot.lock().expect("lock do Deferred");
        if slot.is_none() {
            *slot = Some(value);
            self.condvar.notify_all();
        }
    }

    fn complete(&self, value: T) {
        self.settle(Ok(value));
    }

    fn error(&self, error: LoopbackError) {
        self.settle(Err(error));
    }

    fn wait(&self) -> Result<T, LoopbackError> {
        let mut slot = self.slot.lock().expect("lock do Deferred");
        loop {
            if let Some(value) = slot.as_ref() {
                return value.clone();
            }
            slot = self.condvar.wait(slot).expect("espera do Deferred");
        }
    }
}

/// O servidor em execução: a thread de aceite e o sinal de parada.
struct Running {
    stopping: Arc<AtomicBool>,
    address: SocketAddr,
    accept_thread: JoinHandle<()>,
}

/// O que as threads do servidor compartilham.
struct Shared {
    state: String,
    page: String,
    result: Deferred<OAuthResult>,
}

/// `LoopbackAuthServer`.
pub struct LoopbackAuthServer {
    logger: LoggerRef,
    app_uri: URI,
    app_name: String,
    shared: Arc<Shared>,
    port: Cell<Option<u16>>,
    running: RefCell<Option<Running>>,
}

impl LoopbackAuthServer {
    pub fn new(logger: LoggerRef, app_uri: URI, app_name: &str) -> Self {
        let mut bytes = [0u8; 16];
        fill_bytes(&mut bytes);
        let state = encode_base64(&VSBuffer::wrap(bytes.to_vec()), true, false);
        let page = render_html(&app_uri, app_name);
        Self {
            logger,
            app_uri,
            app_name: app_name.to_owned(),
            shared: Arc::new(Shared { state, page, result: Deferred::new() }),
            port: Cell::new(None),
            running: RefCell::new(None),
        }
    }

    pub fn state(&self) -> &str {
        &self.shared.state
    }

    /// `redirectUri`: falha se o servidor ainda não foi iniciado.
    pub fn redirect_uri(&self) -> Result<String, LoopbackError> {
        match self.port.get() {
            Some(port) => Ok(format!("http://127.0.0.1:{port}/")),
            None => Err(LoopbackError::new("Server is not started yet")),
        }
    }

    pub fn app_uri(&self) -> &URI {
        &self.app_uri
    }

    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    /// `getHtml`: a página de `/done`.
    pub fn get_html(&self) -> String {
        render_html(&self.app_uri, &self.app_name)
    }

    pub fn start(&self) -> Result<(), LoopbackError> {
        if self.running.borrow().is_some() {
            return Err(LoopbackError::new("Server is already started"));
        }
        // Best effort to use a specific port, but fallback to a random one if it is in use
        let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, DEFAULT_AUTH_FLOW_PORT)) {
            Ok(listener) => listener,
            Err(error) if error.kind() == ErrorKind::AddrInUse => {
                self.logger.error("Address in use, retrying with a different port...");
                TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|error| LoopbackError::new(format!("Error listening to server: {error}")))?
            }
            Err(error) => return Err(LoopbackError::new(format!("Error listening to server: {error}"))),
        };
        let address = listener.local_addr().map_err(|_| LoopbackError::new("Unable to determine port"))?;
        self.port.set(Some(address.port()));

        let stopping = Arc::new(AtomicBool::new(false));
        let accept_thread = {
            let stopping = Arc::clone(&stopping);
            let shared = Arc::clone(&self.shared);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stopping.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        let shared = Arc::clone(&shared);
                        std::thread::spawn(move || handle_connection(stream, &shared));
                    }
                }
            })
        };
        *self.running.borrow_mut() = Some(Running { stopping, address, accept_thread });
        Ok(())
    }

    pub fn stop(&self) -> Result<(), LoopbackError> {
        let Some(running) = self.running.borrow_mut().take() else {
            return Ok(());
        };
        running.stopping.store(true, Ordering::SeqCst);
        // Acorda o `accept` bloqueado; a thread vê o sinal e sai, soltando o `TcpListener` (o `server.close`).
        let _ = TcpStream::connect_timeout(&running.address, Duration::from_secs(5));
        running.accept_thread.join().map_err(|_| LoopbackError::new("Timeout waiting for server to close"))
    }

    pub fn wait_for_oauth_response(&self) -> Result<OAuthResult, LoopbackError> {
        self.shared.result.wait()
    }
}

impl LoopbackServer for LoopbackAuthServer {
    fn state(&self) -> &str {
        LoopbackAuthServer::state(self)
    }

    fn start(&self) -> Result<(), LoopbackError> {
        LoopbackAuthServer::start(self)
    }

    fn stop(&self) -> Result<(), LoopbackError> {
        LoopbackAuthServer::stop(self)
    }

    fn wait_for_oauth_response(&self) -> Result<OAuthResult, LoopbackError> {
        LoopbackAuthServer::wait_for_oauth_response(self)
    }
}

/// Uma resposta HTTP: status, cabeçalhos e corpo.
struct Response {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Response {
    fn status(status: u16) -> Self {
        Self { status, headers: Vec::new(), body: Vec::new() }
    }

    fn redirect(location: String) -> Self {
        Self { status: 302, headers: vec![("location", location)], body: Vec::new() }
    }

    fn reason(&self) -> &'static str {
        match self.status {
            200 => "OK",
            302 => "Found",
            400 => "Bad Request",
            404 => "Not Found",
            _ => "Internal Server Error",
        }
    }

    /// O `checkInvalidHeaderChar` do Node: só `\t`, `\x20-\x7e` e `\x80-\xff` valem num valor de cabeçalho.
    fn has_valid_headers(&self) -> bool {
        self.headers.iter().all(|(_, value)| value.chars().all(|c| matches!(c, '\t' | '\x20'..='\x7e' | '\u{80}'..='\u{ff}')))
    }

    fn write_to(&self, stream: &mut TcpStream) {
        let mut head = format!("HTTP/1.1 {} {}\r\n", self.status, self.reason()).into_bytes();
        for (name, value) in &self.headers {
            head.extend_from_slice(name.as_bytes());
            head.extend_from_slice(b": ");
            // Latin-1, como o Node escreve os cabeçalhos.
            head.extend(value.chars().map(|c| c as u8));
            head.extend_from_slice(b"\r\n");
        }
        head.extend_from_slice(format!("Content-Length: {}\r\nConnection: close\r\n\r\n", self.body.len()).as_bytes());
        let _ = stream.write_all(&head);
        let _ = stream.write_all(&self.body);
        let _ = stream.flush();
        let _ = stream.shutdown(Shutdown::Both);
    }
}

/// Lê a linha de requisição (`req.url`), com o limite de cabeçalho do Node (16 KiB) e de tempo.
fn read_request_target(stream: &mut TcpStream) -> Option<String> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buffer.windows(4).any(|window| window == b"\r\n\r\n") && buffer.len() < 16 * 1024 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    }
    let text = String::from_utf8_lossy(&buffer);
    let line = text.lines().next()?;
    let mut parts = line.split(' ');
    let _method = parts.next()?;
    parts.next().map(str::to_owned)
}

/// O `new URL(req.url, base)`: o caminho e a query (sem o `#`, que o cliente não envia).
fn split_target(target: &str) -> (String, String) {
    let target = match target.find("://") {
        Some(scheme_end) => match target[scheme_end + 3..].find('/') {
            Some(slash) => &target[scheme_end + 3 + slash..],
            None => "/",
        },
        None => target,
    };
    let target = target.split('#').next().unwrap_or(target);
    match target.split_once('?') {
        Some((path, query)) => (path.to_owned(), query.to_owned()),
        None => (target.to_owned(), String::new()),
    }
}

fn handle_connection(mut stream: TcpStream, shared: &Shared) {
    let Some(target) = read_request_target(&mut stream) else {
        return;
    };
    let (pathname, query) = split_target(&target);
    let response = match pathname.as_str() {
        "/" => handle_callback(&UrlSearchParams::parse(&query), shared),
        // Serve the static files
        "/done" => {
            let mut response = Response::status(200);
            response.headers.push(("Content-Type", "text/html".to_owned()));
            response.body = shared.page.clone().into_bytes();
            response
        }
        _ => Response::status(404),
    };
    if !response.has_valid_headers() {
        // `writeHead` lança no Node; a promessa do OAuth fica como estava (ver o topo do módulo).
        Response::status(500).write_to(&mut stream);
        return;
    }
    // Ordem do original: o `writeHead` e o `end` vêm antes do `deferredPromise`.
    response.write_to(&mut stream);
    settle_callback(&UrlSearchParams::parse(&query), &pathname, shared);
}

/// A resposta de `/`: redireciona para `/done` (com o erro, se houver) ou devolve `400`.
fn handle_callback(params: &UrlSearchParams, shared: &Shared) -> Response {
    let code = params.get("code").filter(|value| !value.is_empty());
    let state = params.get("state").filter(|value| !value.is_empty());
    let error = params.get("error").filter(|value| !value.is_empty());
    if let Some(error) = error {
        let description = params.get("error_description").filter(|value| !value.is_empty()).unwrap_or(error);
        return Response::redirect(format!("/done?error={description}"));
    }
    if code.is_none() || state.is_none() {
        return Response::status(400);
    }
    if state != Some(shared.state.as_str()) {
        return Response::redirect(format!("/done?error={}", encode_uri_component("State does not match.")));
    }
    Response::redirect("/done".to_owned())
}

/// O efeito sobre a promessa do OAuth, que o original aplica logo depois de `res.end()`.
fn settle_callback(params: &UrlSearchParams, pathname: &str, shared: &Shared) {
    if pathname != "/" {
        return;
    }
    let code = params.get("code").filter(|value| !value.is_empty());
    let state = params.get("state").filter(|value| !value.is_empty());
    if let Some(error) = params.get("error").filter(|value| !value.is_empty()) {
        shared.result.error(LoopbackError::new(error));
        return;
    }
    let (Some(code), Some(state)) = (code, state) else {
        return;
    };
    if shared.state != state {
        shared.result.error(LoopbackError::new("State does not match."));
        return;
    }
    shared.result.complete(OAuthResult { code: code.to_owned(), state: state.to_owned() });
}

/// `encodeURIComponent`: tudo, menos letras, dígitos e `-_.!~*'()`, em UTF-8 com `%XX`.
fn encode_uri_component(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// A imagem de fundo do `.branding`, conforme o nome cru do app (`this._appName`, não o escapado).
fn background_image(app_name: &str) -> &'static str {
    match app_name {
        "Visual Studio Code" => BACKGROUND_VSCODE,
        "Visual Studio Code - Insiders" => BACKGROUND_VSCODE_INSIDERS,
        _ => BACKGROUND_DEFAULT,
    }
}

/// O `getHtml`: nenhum dos dois valores é estático. Cada um é codificado para o seu contexto e o URI fica fora do script.
fn render_html(app_uri: &URI, raw_app_name: &str) -> String {
    let app_uri = html_attribute_encode_value(app_uri.to_string(true).units()).to_string_lossy();
    let app_name_units: Vec<u16> = raw_app_name.encode_utf16().collect();
    let app_name = escape(&app_name_units).to_string_lossy();
    // Substituição numa passada só sobre o modelo: o que entra nunca é relido como marcador.
    let mut out = String::with_capacity(HTML_TEMPLATE.len() + 16 * 1024);
    let mut rest = HTML_TEMPLATE;
    while let Some(start) = rest.find("@@") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("@@").expect("marcador do modelo fechado");
        match &after[..end] {
            "BACKGROUND_IMAGE" => out.push_str(background_image(raw_app_name)),
            "APP_URI" => out.push_str(&app_uri),
            "APP_NAME" => out.push_str(&app_name),
            other => panic!("marcador desconhecido no modelo: {other}"),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

const HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="en">

<head>
	<meta charset="utf-8" />
	<title>GitHub Authentication - Sign In</title>
	<meta name="viewport" content="width=device-width, initial-scale=1">
	<style>
		html {
			height: 100%;
		}

		body {
			box-sizing: border-box;
			min-height: 100%;
			margin: 0;
			padding: 15px 30px;
			display: flex;
			flex-direction: column;
			color: white;
			font-family: "Segoe UI","Helvetica Neue","Helvetica",Arial,sans-serif;
			background-color: #2C2C32;
		}

		.branding {
			background-image: @@BACKGROUND_IMAGE@@;
			background-size: 24px;
			background-repeat: no-repeat;
			background-position: left center;
			padding-left: 36px;
			font-size: 20px;
			letter-spacing: -0.04rem;
			font-weight: 400;
			color: white;
			text-decoration: none;
		}

		.message-container {
			flex-grow: 1;
			display: flex;
			align-items: center;
			justify-content: center;
			margin: 0 30px;
		}

		.message {
			font-weight: 300;
			font-size: 1.4rem;
		}

		body.error .message {
			display: none;
		}

		body.error .error-message {
			display: block;
		}

		.error-message {
			display: none;
			font-weight: 300;
			font-size: 1.3rem;
		}

		.error-text {
			color: red;
			font-size: 1rem;
		}

		@font-face {
			font-family: 'Segoe UI';
			src: url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/light/latest.eot"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/light/latest.eot?#iefix") format("embedded-opentype");
			src: local("Segoe UI Light"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/light/latest.woff2") format("woff2"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/light/latest.woff") format("woff"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/light/latest.ttf") format("truetype"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/light/latest.svg#web") format("svg");
			font-weight: 200
		}

		@font-face {
			font-family: 'Segoe UI';
			src: url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/semilight/latest.eot"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/semilight/latest.eot?#iefix") format("embedded-opentype");
			src: local("Segoe UI Semilight"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/semilight/latest.woff2") format("woff2"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/semilight/latest.woff") format("woff"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/semilight/latest.ttf") format("truetype"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/semilight/latest.svg#web") format("svg");
			font-weight: 300
		}

		@font-face {
			font-family: 'Segoe UI';
			src: url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/normal/latest.eot"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/normal/latest.eot?#iefix") format("embedded-opentype");
			src: local("Segoe UI"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/normal/latest.woff2") format("woff"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/normal/latest.woff") format("woff"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/normal/latest.ttf") format("truetype"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/normal/latest.svg#web") format("svg");
			font-weight: 400
		}

		@font-face {
			font-family: 'Segoe UI';
			src: url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/semibold/latest.eot"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/semibold/latest.eot?#iefix") format("embedded-opentype");
			src: local("Segoe UI Semibold"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/semibold/latest.woff2") format("woff"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/semibold/latest.woff") format("woff"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/semibold/latest.ttf") format("truetype"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/semibold/latest.svg#web") format("svg");
			font-weight: 600
		}

		@font-face {
			font-family: 'Segoe UI';
			src: url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/bold/latest.eot"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/bold/latest.eot?#iefix") format("embedded-opentype");
			src: local("Segoe UI Bold"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/bold/latest.woff2") format("woff"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/bold/latest.woff") format("woff"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/bold/latest.ttf") format("truetype"),url("https://c.s-microsoft.com/static/fonts/segoe-ui/west-european/bold/latest.svg#web") format("svg");
			font-weight: 700
		}
	</style>
</head>

<body>
	<a class="branding" href="https://code.visualstudio.com/">
		@@APP_NAME@@
	</a>
	<div class="message-container">
		<div class="message">
			Sign-in successful! Returning to @@APP_NAME@@...
			<br><br>
			If you're not redirected automatically, <a id="redirect-link" href="@@APP_URI@@" style="color: #85CEFF;">click here</a> or close this page.
		</div>
		<div class="error-message">
			An error occurred while signing in:
			<div class="error-text"></div>
		</div>
	</div>
	<script>
		const search = window.location.search;
		const error = (/[?&^]error=([^&]+)/.exec(search) || [])[1];
		if (error) {
			document.querySelector('.error-text')
				.textContent = decodeURIComponent(error);
			document.querySelector('body')
				.classList.add('error');
		} else {
			// Redirect to the app URI after a 1-second delay to allow page to load
			setTimeout(function() {
				window.location.href = document.getElementById('redirect-link').getAttribute('href');
			}, 1000);
		}
	</script>
</body>
</html>"#;

// As três imagens de fundo do `.branding`, byte a byte as do original (o original as deixa inline para trazer por mixin).
const BACKGROUND_DEFAULT: &str = "url('data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAABAAAAAQABAMAAACNMzawAAAABGdBTUEAALGPC/xhBQAAACBjSFJNAAB6JgAAgIQAAPoAAACA6AAAdTAAAOpgAAA6mAAAF3CculE8AAAAJ1BMVEUAAAD///9Qm8+ozed8tNsWer+Lvd9trNfF3u9Ck8slgsMzi8eZxeM/Qa6mAAAAAXRSTlMAQObYZgAAAAFiS0dEAf8CLd4AAAAHdElNRQfiCwYULRt0g+ZLAAAJRUlEQVR42u3SUY0CQRREUSy0hSZtBA+wfOwv4wAPYwAJSMAfAthkB6YD79HnKqikzmYjSZIkSZIkSZIkSZIkSZIkSZIkSZIkSZIkSZIkSZIkSZIkSZIkSZIkSZIkSZIkSZIkSZIk6cmKhg4AAASAABAAAkAACAABIAAEgAAQAAJAAAgAAaBvBVAjtV2wfFe1ogcA+0gdFgBoe60IAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAnAjAP/1MkWoAvBvAsUTqBAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAJAAgAASAABIAAEAACQAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAKA7gDlbFwC6AijZagAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQDM2boA0BXAqAEAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIATARAAAkAAvF7N1hWArgBKthoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAfBrAlK0bAF0BjBoAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA4EQABIAAEwOtN2boB0BVAyVYDAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAgE8DqNm6AtAVwKgBAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACAEwEQAAJAAPzd7xypMwDvBnAskToBAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAIAAkAACAABIAAEgAAQAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAAkAACAABIAAEgAAQAAJAAAgAASAABIAAEAACQAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAIBKBGarsAwK5qRQ8ANGYAACAABIAAEAACQAAIAAEgAASAABAAAkDfCUCSJEmSJEmSJEmSJEmSJEmSJEmSJEmSJEmSJEmSJEmSJEmSJEmSJEmSJEmSJEmSJEmSJEmSJC3uDtO80OSql+i8AAAAJXRFWHRkYXRlOmNyZWF0ZQAyMDE4LTExLTA2VDIwOjQ1OjI3KzAwOjAwEjLurQAAACV0RVh0ZGF0ZTptb2RpZnkAMjAxOC0xMS0wNlQyMDo0NToyNyswMDowMGNvVhEAAAAZdEVYdFNvZnR3YXJlAEFkb2JlIEltYWdlUmVhZHlxyWU8AAAAAElFTkSuQmCC')";

const BACKGROUND_VSCODE: &str = "url('data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjU2IiBoZWlnaHQ9IjI1NiIgdmlld0JveD0iMCAwIDI1NiAyNTYiIGZpbGw9Im5vbmUiIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyI+CjxtYXNrIGlkPSJtYXNrMCIgbWFzay10eXBlPSJhbHBoYSIgbWFza1VuaXRzPSJ1c2VyU3BhY2VPblVzZSIgeD0iMCIgeT0iMCIgd2lkdGg9IjI1NiIgaGVpZ2h0PSIyNTYiPgo8cGF0aCBmaWxsLXJ1bGU9ImV2ZW5vZGQiIGNsaXAtcnVsZT0iZXZlbm9kZCIgZD0iTTE4MS41MzQgMjU0LjI1MkMxODUuNTY2IDI1NS44MjMgMTkwLjE2NCAyNTUuNzIyIDE5NC4yMzQgMjUzLjc2NEwyNDYuOTQgMjI4LjQwM0MyNTIuNDc4IDIyNS43MzggMjU2IDIyMC4xMzIgMjU2IDIxMy45ODNWNDIuMDE4MUMyNTYgMzUuODY4OSAyNTIuNDc4IDMwLjI2MzggMjQ2Ljk0IDI3LjU5ODhMMTk0LjIzNCAyLjIzNjgxQzE4OC44OTMgLTAuMzMzMTMyIDE4Mi42NDIgMC4yOTYzNDQgMTc3Ljk1NSAzLjcwNDE4QzE3Ny4yODUgNC4xOTEgMTc2LjY0NyA0LjczNDU0IDE3Ni4wNDkgNS4zMzM1NEw3NS4xNDkgOTcuMzg2MkwzMS4xOTkyIDY0LjAyNDdDMjcuMTA3OSA2MC45MTkxIDIxLjM4NTMgNjEuMTczNSAxNy41ODU1IDY0LjYzTDMuNDg5MzYgNzcuNDUyNUMtMS4xNTg1MyA4MS42ODA1IC0xLjE2Mzg2IDg4Ljk5MjYgMy40Nzc4NSA5My4yMjc0TDQxLjU5MjYgMTI4TDMuNDc3ODUgMTYyLjc3M0MtMS4xNjM4NiAxNjcuMDA4IC0xLjE1ODUzIDE3NC4zMiAzLjQ4OTM2IDE3OC41NDhMMTcuNTg1NSAxOTEuMzdDMjEuMzg1MyAxOTQuODI3IDI3LjEwNzkgMTk1LjA4MSAzMS4xOTkyIDE5MS45NzZMNzUuMTQ5IDE1OC42MTRMMTc2LjA0OSAyNTAuNjY3QzE3Ny42NDUgMjUyLjI2NCAxNzkuNTE5IDI1My40NjcgMTgxLjUzNCAyNTQuMjUyWk0xOTIuMDM5IDY5Ljg4NTNMMTE1LjQ3OSAxMjhMMTkyLjAzOSAxODYuMTE1VjY5Ljg4NTNaIiBmaWxsPSJ3aGl0ZSIvPgo8L21hc2s+CjxnIG1hc2s9InVybCgjbWFzazApIj4KPHBhdGggZD0iTTI0Ni45NCAyNy42MzgzTDE5NC4xOTMgMi4yNDEzOEMxODguMDg4IC0wLjY5ODMwMiAxODAuNzkxIDAuNTQxNzIxIDE3NS45OTkgNS4zMzMzMkwzLjMyMzcxIDE2Mi43NzNDLTEuMzIwODIgMTY3LjAwOCAtMS4zMTU0OCAxNzQuMzIgMy4zMzUyMyAxNzguNTQ4TDE3LjQzOTkgMTkxLjM3QzIxLjI0MjEgMTk0LjgyNyAyNi45NjgyIDE5NS4wODEgMzEuMDYxOSAxOTEuOTc2TDIzOS4wMDMgMzQuMjI2OUMyNDUuOTc5IDI4LjkzNDcgMjU1Ljk5OSAzMy45MTAzIDI1NS45OTkgNDIuNjY2N1Y0Mi4wNTQzQzI1NS45OTkgMzUuOTA3OCAyNTIuNDc4IDMwLjMwNDcgMjQ2Ljk0IDI3LjYzODNaIiBmaWxsPSIjMDA2NUE5Ii8+CjxnIGZpbHRlcj0idXJsKCNmaWx0ZXIwX2QpIj4KPHBhdGggZD0iTTI0Ni45NCAyMjguMzYyTDE5NC4xOTMgMjUzLjc1OUMxODguMDg4IDI1Ni42OTggMTgwLjc5MSAyNTUuNDU4IDE3NS45OTkgMjUwLjY2N0wzLjMyMzcxIDkzLjIyNzJDLTEuMzIwODIgODguOTkyNSAtMS4zMTU0OCA4MS42ODAyIDMuMzM1MjMgNzcuNDUyM0wxNy40Mzk5IDY0LjYyOThDMjEuMjQyMSA2MS4xNzMzIDI2Ljk2ODIgNjAuOTE4OCAzMS4wNjE5IDY0LjAyNDVMMjM5LjAwMyAyMjEuNzczQzI0NS45NzkgMjI3LjA2NSAyNTUuOTk5IDIyMi4wOSAyNTUuOTk5IDIxMy4zMzNWMjEzLjk0NkMyNTUuOTk5IDIyMC4wOTIgMjUyLjQ3OCAyMjUuNjk1IDI0Ni45NCAyMjguMzYyWiIgZmlsbD0iIzAwN0FDQyIvPgo8L2c+CjxnIGZpbHRlcj0idXJsKCNmaWx0ZXIxX2QpIj4KPHBhdGggZD0iTTE5NC4xOTYgMjUzLjc2M0MxODguMDg5IDI1Ni43IDE4MC43OTIgMjU1LjQ1OSAxNzYgMjUwLjY2N0MxODEuOTA0IDI1Ni41NzEgMTkyIDI1Mi4zODkgMTkyIDI0NC4wMzlWMTEuOTYwNkMxOTIgMy42MTA1NyAxODEuOTA0IC0wLjU3MTE3NSAxNzYgNS4zMzMyMUMxODAuNzkyIDAuNTQxMTY2IDE4OC4wODkgLTAuNzAwNjA3IDE5NC4xOTYgMi4yMzY0OEwyNDYuOTM0IDI3LjU5ODVDMjUyLjQ3NiAzMC4yNjM1IDI1NiAzNS44Njg2IDI1NiA0Mi4wMTc4VjIxMy45ODNDMjU2IDIyMC4xMzIgMjUyLjQ3NiAyMjUuNzM3IDI0Ni45MzQgMjI4LjQwMkwxOTQuMTk2IDI1My43NjNaIiBmaWxsPSIjMUY5Q0YwIi8+CjwvZz4KPGcgc3R5bGU9Im1peC1ibGVuZC1tb2RlOm92ZXJsYXkiIG9wYWNpdHk9IjAuMjUiPgo8cGF0aCBmaWxsLXJ1bGU9ImV2ZW5vZGQiIGNsaXAtcnVsZT0iZXZlbm9kZCIgZD0iTTE4MS4zNzggMjU0LjI1MkMxODUuNDEgMjU1LjgyMiAxOTAuMDA4IDI1NS43MjIgMTk0LjA3NyAyNTMuNzY0TDI0Ni43ODMgMjI4LjQwMkMyNTIuMzIyIDIyNS43MzcgMjU1Ljg0NCAyMjAuMTMyIDI1NS44NDQgMjEzLjk4M1Y0Mi4wMTc5QzI1NS44NDQgMzUuODY4NyAyNTIuMzIyIDMwLjI2MzYgMjQ2Ljc4NCAyNy41OTg2TDE5NC4wNzcgMi4yMzY2NUMxODguNzM3IC0wLjMzMzI5OSAxODIuNDg2IDAuMjk2MTc3IDE3Ny43OTggMy43MDQwMUMxNzcuMTI5IDQuMTkwODMgMTc2LjQ5MSA0LjczNDM3IDE3NS44OTIgNS4zMzMzN0w3NC45OTI3IDk3LjM4NkwzMS4wNDI5IDY0LjAyNDVDMjYuOTUxNyA2MC45MTg5IDIxLjIyOSA2MS4xNzM0IDE3LjQyOTIgNjQuNjI5OEwzLjMzMzExIDc3LjQ1MjNDLTEuMzE0NzggODEuNjgwMyAtMS4zMjAxMSA4OC45OTI1IDMuMzIxNiA5My4yMjczTDQxLjQzNjQgMTI4TDMuMzIxNiAxNjIuNzczQy0xLjMyMDExIDE2Ny4wMDggLTEuMzE0NzggMTc0LjMyIDMuMzMzMTEgMTc4LjU0OEwxNy40MjkyIDE5MS4zN0MyMS4yMjkgMTk0LjgyNyAyNi45NTE3IDE5NS4wODEgMzEuMDQyOSAxOTEuOTc2TDc0Ljk5MjcgMTU4LjYxNEwxNzUuODkyIDI1MC42NjdDMTc3LjQ4OCAyNTIuMjY0IDE3OS4zNjMgMjUzLjQ2NyAxODEuMzc4IDI1NC4yNTJaTTE5MS44ODMgNjkuODg1MUwxMTUuMzIzIDEyOEwxOTEuODgzIDE4Ni4xMTVWNjkuODg1MVoiIGZpbGw9InVybCgjcGFpbnQwX2xpbmVhcikiLz4KPC9nPgo8L2c+CjxkZWZzPgo8ZmlsdGVyIGlkPSJmaWx0ZXIwX2QiIHg9Ii0yMS40ODk2IiB5PSI0MC41MjI1IiB3aWR0aD0iMjk4LjgyMiIgaGVpZ2h0PSIyMzYuMTQ5IiBmaWx0ZXJVbml0cz0idXNlclNwYWNlT25Vc2UiIGNvbG9yLWludGVycG9sYXRpb24tZmlsdGVycz0ic1JHQiI+CjxmZUZsb29kIGZsb29kLW9wYWNpdHk9IjAiIHJlc3VsdD0iQmFja2dyb3VuZEltYWdlRml4Ii8+CjxmZUNvbG9yTWF0cml4IGluPSJTb3VyY2VBbHBoYSIgdHlwZT0ibWF0cml4IiB2YWx1ZXM9IjAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDEyNyAwIi8+CjxmZU9mZnNldC8+CjxmZUdhdXNzaWFuQmx1ciBzdGREZXZpYXRpb249IjEwLjY2NjciLz4KPGZlQ29sb3JNYXRyaXggdHlwZT0ibWF0cml4IiB2YWx1ZXM9IjAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAuMjUgMCIvPgo8ZmVCbGVuZCBtb2RlPSJvdmVybGF5IiBpbjI9IkJhY2tncm91bmRJbWFnZUZpeCIgcmVzdWx0PSJlZmZlY3QxX2Ryb3BTaGFkb3ciLz4KPGZlQmxlbmQgbW9kZT0ibm9ybWFsIiBpbj0iU291cmNlR3JhcGhpYyIgaW4yPSJlZmZlY3QxX2Ryb3BTaGFkb3ciIHJlc3VsdD0ic2hhcGUiLz4KPC9maWx0ZXI+CjxmaWx0ZXIgaWQ9ImZpbHRlcjFfZCIgeD0iMTU0LjY2NyIgeT0iLTIwLjY3MzUiIHdpZHRoPSIxMjIuNjY3IiBoZWlnaHQ9IjI5Ny4zNDciIGZpbHRlclVuaXRzPSJ1c2VyU3BhY2VPblVzZSIgY29sb3ItaW50ZXJwb2xhdGlvbi1maWx0ZXJzPSJzUkdCIj4KPGZlRmxvb2QgZmxvb2Qtb3BhY2l0eT0iMCIgcmVzdWx0PSJCYWNrZ3JvdW5kSW1hZ2VGaXgiLz4KPGZlQ29sb3JNYXRyaXggaW49IlNvdXJjZUFscGhhIiB0eXBlPSJtYXRyaXgiIHZhbHVlcz0iMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMTI3IDAiLz4KPGZlT2Zmc2V0Lz4KPGZlR2F1c3NpYW5CbHVyIHN0ZERldmlhdGlvbj0iMTAuNjY2NyIvPgo8ZmVDb2xvck1hdHJpeCB0eXBlPSJtYXRyaXgiIHZhbHVlcz0iMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMC4yNSAwIi8+CjxmZUJsZW5kIG1vZGU9Im92ZXJsYXkiIGluMj0iQmFja2dyb3VuZEltYWdlRml4IiByZXN1bHQ9ImVmZmVjdDFfZHJvcFNoYWRvdyIvPgo8ZmVCbGVuZCBtb2RlPSJub3JtYWwiIGluPSJTb3VyY2VHcmFwaGljIiBpbjI9ImVmZmVjdDFfZHJvcFNoYWRvdyIgcmVzdWx0PSJzaGFwZSIvPgo8L2ZpbHRlcj4KPGxpbmVhckdyYWRpZW50IGlkPSJwYWludDBfbGluZWFyIiB4MT0iMTI3Ljg0NCIgeTE9IjAuNjU5OTg4IiB4Mj0iMTI3Ljg0NCIgeTI9IjI1NS4zNCIgZ3JhZGllbnRVbml0cz0idXNlclNwYWNlT25Vc2UiPgo8c3RvcCBzdG9wLWNvbG9yPSJ3aGl0ZSIvPgo8c3RvcCBvZmZzZXQ9IjEiIHN0b3AtY29sb3I9IndoaXRlIiBzdG9wLW9wYWNpdHk9IjAiLz4KPC9saW5lYXJHcmFkaWVudD4KPC9kZWZzPgo8L3N2Zz4K')";

const BACKGROUND_VSCODE_INSIDERS: &str = "url('data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjU2IiBoZWlnaHQ9IjI1NiIgdmlld0JveD0iMCAwIDI1NiAyNTYiIGZpbGw9Im5vbmUiIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyI+CjxtYXNrIGlkPSJtYXNrMCIgbWFzay10eXBlPSJhbHBoYSIgbWFza1VuaXRzPSJ1c2VyU3BhY2VPblVzZSIgeD0iMCIgeT0iMCIgd2lkdGg9IjI1NiIgaGVpZ2h0PSIyNTYiPgo8cGF0aCBkPSJNMTc2LjA0OSAyNTAuNjY5QzE4MC44MzggMjU1LjQ1OSAxODguMTMgMjU2LjcgMTk0LjIzNCAyNTMuNzY0TDI0Ni45NCAyMjguNDE5QzI1Mi40NzggMjI1Ljc1NSAyNTYgMjIwLjE1NCAyNTYgMjE0LjAwOFY0Mi4xNDc5QzI1NiAzNi4wMDI1IDI1Mi40NzggMzAuNDAwOCAyNDYuOTQgMjcuNzM3NEwxOTQuMjM0IDIuMzkwODlDMTg4LjEzIC0wLjU0NDQxNiAxODAuODM4IDAuNjk2NjA3IDE3Ni4wNDkgNS40ODU3MkMxODEuOTUgLTAuNDE1MDYgMTkyLjAzOSAzLjc2NDEzIDE5Mi4wMzkgMTIuMTA5MVYyNDQuMDQ2QzE5Mi4wMzkgMjUyLjM5MSAxODEuOTUgMjU2LjU3IDE3Ni4wNDkgMjUwLjY2OVoiIGZpbGw9IndoaXRlIi8+CjxwYXRoIGQ9Ik0xODEuMzc5IDE4MC42NDZMMTE0LjMzIDEyOC42MzNMMTgxLjM3OSA3NS41MTE0VjE3Ljc5NEMxODEuMzc5IDEwLjg0NzcgMTczLjEyOCA3LjIwNjczIDE2Ny45OTYgMTEuODg2Mkw3NC42NTE0IDk3Ljg1MThMMzEuMTk5NCA2NC4xNDM4QzI3LjEwODEgNjEuMDM5IDIxLjM4NTEgNjEuMjk0IDE3LjU4NTMgNjQuNzQ3NkwzLjQ4OTc0IDc3LjU2MjdDLTEuMTU4NDcgODEuNzg5MyAtMS4xNjM2NyA4OS4wOTQ4IDMuNDc2NzIgOTMuMzI5MkwxNjcuOTggMjQ0LjE4NUMxNzMuMTA3IDI0OC44ODcgMTgxLjM3OSAyNDUuMjQ5IDE4MS4zNzkgMjM4LjI5MlYxODAuNjQ2WiIgZmlsbD0id2hpdGUiLz4KPHBhdGggZD0iTTM2LjY5MzcgMTM0LjE5NUwzLjQ3NjcyIDE2Mi44MjhDLTEuMTYzNjcgMTY3LjA2MiAtMS4xNTg0NyAxNzQuMzcgMy40ODk3NCAxNzguNTk0TDE3LjU4NTMgMTkxLjQwOUMyMS4zODUxIDE5NC44NjMgMjcuMTA4MSAxOTUuMTE4IDMxLjE5OTQgMTkyLjAxM0w2OS40NDcyIDE2NC4wNTdMMzYuNjkzNyAxMzQuMTk1WiIgZmlsbD0id2hpdGUiLz4KPC9tYXNrPgo8ZyBtYXNrPSJ1cmwoI21hc2swKSI+CjxwYXRoIGQ9Ik0xNjcuOTk2IDExLjg4NTdDMTczLjEyOCA3LjIwNjI3IDE4MS4zNzkgMTAuODQ3MyAxODEuMzc5IDE3Ljc5MzZWNzUuNTEwOUwxMDQuOTM4IDEzNi4wNzNMNjUuNTc0MiAxMDYuMjExTDE2Ny45OTYgMTEuODg1N1oiIGZpbGw9IiMwMDlBN0MiLz4KPHBhdGggZD0iTTM2LjY5MzcgMTM0LjE5NEwzLjQ3NjcyIDE2Mi44MjdDLTEuMTYzNjcgMTY3LjA2MiAtMS4xNTg0NyAxNzQuMzcgMy40ODk3NCAxNzguNTk0TDE3LjU4NTMgMTkxLjQwOUMyMS4zODUxIDE5NC44NjMgMjcuMTA4MSAxOTUuMTE4IDMxLjE5OTQgMTkyLjAxM0w2OS40NDcyIDE2NC4wNTZMMzYuNjkzNyAxMzQuMTk0WiIgZmlsbD0iIzAwOUE3QyIvPgo8ZyBmaWx0ZXI9InVybCgjZmlsdGVyMF9kKSI+CjxwYXRoIGQ9Ik0xODEuMzc5IDE4MC42NDVMMzEuMTk5NCA2NC4xNDI3QzI3LjEwODEgNjEuMDM3OSAyMS4zODUxIDYxLjI5MjkgMTcuNTg1MyA2NC43NDY1TDMuNDg5NzQgNzcuNTYxNkMtMS4xNTg0NyA4MS43ODgyIC0xLjE2MzY3IDg5LjA5MzcgMy40NzY3MiA5My4zMjgxTDE2Ny45NzIgMjQ0LjE3NkMxNzMuMTAyIDI0OC44ODEgMTgxLjM3OSAyNDUuMjQxIDE4MS4zNzkgMjM4LjI4VjE4MC42NDVaIiBmaWxsPSIjMDBCMjk0Ii8+CjwvZz4KPGcgZmlsdGVyPSJ1cmwoI2ZpbHRlcjFfZCkiPgo8cGF0aCBkPSJNMTk0LjIzMyAyNTMuNzY2QzE4OC4xMyAyNTYuNzAxIDE4MC44MzcgMjU1LjQ2IDE3Ni4wNDggMjUwLjY3MUMxODEuOTQ5IDI1Ni41NzEgMTkyLjAzOSAyNTIuMzkyIDE5Mi4wMzkgMjQ0LjA0N1YxMi4xMTAzQzE5Mi4wMzkgMy43NjUzNSAxODEuOTQ5IC0wLjQxMzgzOSAxNzYuMDQ4IDUuNDg2OTRDMTgwLjgzNyAwLjY5NzgyNCAxODguMTI5IC0wLjU0MzE5MSAxOTQuMjMzIDIuMzkyMUwyNDYuOTM5IDI3LjczODZDMjUyLjQ3OCAzMC40MDIgMjU2IDM2LjAwMzcgMjU2IDQyLjE0OTFWMjE0LjAwOUMyNTYgMjIwLjE1NSAyNTIuNDc4IDIyNS43NTcgMjQ2LjkzOSAyMjguNDJMMTk0LjIzMyAyNTMuNzY2WiIgZmlsbD0iIzI0QkZBNSIvPgo8L2c+CjwvZz4KPGRlZnM+CjxmaWx0ZXIgaWQ9ImZpbHRlcjBfZCIgeD0iLTIxLjMzMzMiIHk9IjQwLjY0MTMiIHdpZHRoPSIyMjQuMDQ1IiBoZWlnaHQ9IjIyNi45ODgiIGZpbHRlclVuaXRzPSJ1c2VyU3BhY2VPblVzZSIgY29sb3ItaW50ZXJwb2xhdGlvbi1maWx0ZXJzPSJzUkdCIj4KPGZlRmxvb2QgZmxvb2Qtb3BhY2l0eT0iMCIgcmVzdWx0PSJCYWNrZ3JvdW5kSW1hZ2VGaXgiLz4KPGZlQ29sb3JNYXRyaXggaW49IlNvdXJjZUFscGhhIiB0eXBlPSJtYXRyaXgiIHZhbHVlcz0iMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMTI3IDAiLz4KPGZlT2Zmc2V0Lz4KPGZlR2F1c3NpYW5CbHVyIHN0ZERldmlhdGlvbj0iMTAuNjY2NyIvPgo8ZmVDb2xvck1hdHJpeCB0eXBlPSJtYXRyaXgiIHZhbHVlcz0iMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMC4xNSAwIi8+CjxmZUJsZW5kIG1vZGU9Im5vcm1hbCIgaW4yPSJCYWNrZ3JvdW5kSW1hZ2VGaXgiIHJlc3VsdD0iZWZmZWN0MV9kcm9wU2hhZG93Ii8+CjxmZUJsZW5kIG1vZGU9Im5vcm1hbCIgaW49IlNvdXJjZUdyYXBoaWMiIGluMj0iZWZmZWN0MV9kcm9wU2hhZG93IiByZXN1bHQ9InNoYXBlIi8+CjwvZmlsdGVyPgo8ZmlsdGVyIGlkPSJmaWx0ZXIxX2QiIHg9IjE1NC43MTUiIHk9Ii0yMC41MTY5IiB3aWR0aD0iMTIyLjYxOCIgaGVpZ2h0PSIyOTcuMTkxIiBmaWx0ZXJVbml0cz0idXNlclNwYWNlT25Vc2UiIGNvbG9yLWludGVycG9sYXRpb24tZmlsdGVycz0ic1JHQiI+CjxmZUZsb29kIGZsb29kLW9wYWNpdHk9IjAiIHJlc3VsdD0iQmFja2dyb3VuZEltYWdlRml4Ii8+CjxmZUNvbG9yTWF0cml4IGluPSJTb3VyY2VBbHBoYSIgdHlwZT0ibWF0cml4IiB2YWx1ZXM9IjAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDEyNyAwIi8+CjxmZU9mZnNldC8+CjxmZUdhdXNzaWFuQmx1ciBzdGREZXZpYXRpb249IjEwLjY2NjciLz4KPGZlQ29sb3JNYXRyaXggdHlwZT0ibWF0cml4IiB2YWx1ZXM9IjAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAgMCAwIDAuMjUgMCIvPgo8ZmVCbGVuZCBtb2RlPSJvdmVybGF5IiBpbjI9IkJhY2tncm91bmRJbWFnZUZpeCIgcmVzdWx0PSJlZmZlY3QxX2Ryb3BTaGFkb3ciLz4KPGZlQmxlbmQgbW9kZT0ibm9ybWFsIiBpbj0iU291cmNlR3JhcGhpYyIgaW4yPSJlZmZlY3QxX2Ryb3BTaGFkb3ciIHJlc3VsdD0ic2hhcGUiLz4KPC9maWx0ZXI+CjwvZGVmcz4KPC9zdmc+Cg==')";
