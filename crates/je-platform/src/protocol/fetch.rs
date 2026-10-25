//! `GlobalRequest` e `GlobalResponse` (o `Request` e o `Response` do `fetch` do Node e do Electron) e `net.fetch` do Electron,
//! que os protocolos de `src/vs/platform/protocol/electron-main` usam.
//!
//! Decisões de tradução:
//! - **`Request`** é [`GlobalRequest`]: a URL é validada e serializada pelo parser WHATWG (`new Request(url)` lança
//!   `TypeError` numa URL inválida), então `request.url` já vem normalizado.
//! - **`Response`** é [`GlobalResponse`] com o corpo inteiro em memória. O construtor segue o `fetch`: o `status` fora de
//!   200 a 599 e um corpo em resposta sem corpo (204, 205 e 304) são erros; um corpo de texto ganha o
//!   `Content-Type: text/plain;charset=UTF-8` se os cabeçalhos não trouxerem um. `Response.error()` é [`GlobalResponse::error`]
//!   (status 0, tipo `error`).
//! - **`Headers`** é [`GlobalHeaders`]: os nomes são minúsculos e a busca não distingue caixa, como no original.
//! - **`net.fetch(url, { bypassCustomProtocolHandlers: true })`** é [`net_fetch`]: o `ureq` (Rust puro, bloqueante) numa thread, com
//!   o laço de eventos consultando o resultado a cada [`POLL_INTERVAL_MS`] ms (a mesma técnica do `RequestService`). Os
//!   redirecionamentos são seguidos, como no `fetch`. O esquema é só o `http:` (o `https:` precisa do provedor de TLS que o
//!   `RequestService` configura; a URL que o protocolo remoto monta é sempre `http:`).

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use je_base::errors::Error;
use je_base::promise::{Promise, Resolver};
use je_base::runtime;

/// De quanto em quanto tempo o laço de eventos consulta o resultado da thread de [`net_fetch`].
pub const POLL_INTERVAL_MS: u64 = 10;

/// O tratador de um protocolo: `(request: GlobalRequest) => Promise<GlobalResponse>`.
pub type ProtocolHandler = Rc<dyn Fn(GlobalRequest) -> Promise<GlobalResponse>>;

/// `Headers`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlobalHeaders {
    entries: Vec<(String, String)>,
}

impl GlobalHeaders {
    pub fn new() -> Self {
        Self::default()
    }

    /// `new Headers(init)` a partir de pares nome/valor (um nome repetido troca o valor, como o `Record` do original).
    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut headers = Self::new();
        for (name, value) in pairs {
            headers.set(name, value);
        }
        headers
    }

    /// `headers.get(name)`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.entries.iter().find(|(entry_name, _)| entry_name.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }

    /// `headers.set(name, value)`.
    pub fn set(&mut self, name: &str, value: &str) {
        let name = name.to_ascii_lowercase();
        match self.entries.iter_mut().find(|(entry_name, _)| *entry_name == name) {
            Some((_, current)) => *current = value.to_owned(),
            None => self.entries.push((name, value.to_owned())),
        }
    }

    /// Os pares na ordem de inserção, com o nome em minúsculas.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

/// `Request`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalRequest {
    url: String,
    method: String,
    headers: GlobalHeaders,
    body: Option<Vec<u8>>,
}

impl GlobalRequest {
    /// `new Request(url)`.
    pub fn new(url: &str) -> Result<Self, Error> {
        let parsed = ::url::Url::parse(url).map_err(|error| Error::new(format!("Failed to parse URL from {url}: {error}")))?;

        Ok(Self { url: parsed.as_str().to_owned(), method: "GET".to_owned(), headers: GlobalHeaders::new(), body: None })
    }

    /// O `method` do `RequestInit`: os métodos conhecidos são normalizados para maiúsculas, como no `fetch`.
    pub fn with_method(mut self, method: &str) -> Self {
        let upper = method.to_ascii_uppercase();
        self.method = if ["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT"].contains(&upper.as_str()) { upper } else { method.to_owned() };
        self
    }

    /// O `headers` do `RequestInit`.
    pub fn with_headers(mut self, headers: GlobalHeaders) -> Self {
        self.headers = headers;
        self
    }

    /// O `body` do `RequestInit`.
    pub fn with_body(mut self, body: Option<Vec<u8>>) -> Self {
        self.body = body;
        self
    }

    /// `request.url`.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// `request.method`.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// `request.headers`.
    pub fn headers(&self) -> &GlobalHeaders {
        &self.headers
    }

    /// O corpo do pedido, quando há.
    pub fn body(&self) -> Option<&[u8]> {
        self.body.as_deref()
    }
}

/// O `BodyInit` de `new Response(body)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResponseBody {
    /// `null`.
    Null,
    /// Um `string`.
    Text(String),
    /// Um `Buffer` ou `Uint8Array`.
    Bytes(Vec<u8>),
}

/// `response.type`, reduzido ao que o porte distingue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResponseKind {
    Default,
    Error,
}

/// `Response`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalResponse {
    status: u16,
    headers: GlobalHeaders,
    body: Vec<u8>,
    kind: ResponseKind,
}

impl GlobalResponse {
    /// `new Response(body, { status, headers })`.
    pub fn new(body: ResponseBody, status: u16, headers: Option<GlobalHeaders>) -> Result<Self, Error> {
        if !(200..=599).contains(&status) {
            return Err(Error::new(format!("init[\"status\"] must be in the range of 200 to 599, inclusive. Received {status}")));
        }

        let mut headers = headers.unwrap_or_default();
        let bytes = match body {
            ResponseBody::Null => Vec::new(),
            ResponseBody::Text(text) => {
                if headers.get("content-type").is_none() {
                    headers.set("content-type", "text/plain;charset=UTF-8");
                }
                text.into_bytes()
            }
            ResponseBody::Bytes(bytes) => bytes,
        };

        if matches!(status, 204 | 205 | 304) && !bytes.is_empty() {
            return Err(Error::new(format!("Response constructor: Invalid response status code {status}")));
        }

        Ok(Self { status, headers, body: bytes, kind: ResponseKind::Default })
    }

    /// `Response.error()`.
    pub fn error() -> Self {
        Self { status: 0, headers: GlobalHeaders::new(), body: Vec::new(), kind: ResponseKind::Error }
    }

    /// `response.status`.
    pub fn status(&self) -> u16 {
        self.status
    }

    /// `response.type`.
    pub fn kind(&self) -> ResponseKind {
        self.kind
    }

    /// `response.headers`.
    pub fn headers(&self) -> &GlobalHeaders {
        &self.headers
    }

    /// `response.headers.set(name, value)`.
    pub fn set_header(&mut self, name: &str, value: &str) {
        self.headers.set(name, value);
    }

    /// O corpo inteiro (`await response.arrayBuffer()`).
    pub fn bytes(&self) -> &[u8] {
        &self.body
    }

    /// `await response.text()`: o corpo lido como UTF-8.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

type FetchOutcome = Option<Result<GlobalResponse, String>>;

/// `net.fetch(request)`.
pub fn net_fetch(request: GlobalRequest) -> Promise<GlobalResponse> {
    let outcome: Arc<Mutex<FetchOutcome>> = Arc::new(Mutex::new(None));
    let worker_outcome = outcome.clone();
    std::thread::spawn(move || {
        let result = perform_fetch(&request);
        *worker_outcome.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
    });

    Promise::with_executor(move |resolver| poll_fetch(outcome, resolver))
}

fn poll_fetch(outcome: Arc<Mutex<FetchOutcome>>, resolver: Resolver<GlobalResponse>) {
    let finished = outcome.lock().ok().and_then(|mut outcome| outcome.take());
    match finished {
        Some(Ok(response)) => resolver.resolve(response),
        Some(Err(message)) => resolver.reject(Error::new(format!("fetch failed: {message}"))),
        None => {
            runtime::set_timeout(move || poll_fetch(outcome, resolver), POLL_INTERVAL_MS);
        }
    }
}

/// Faz o pedido bloqueando a thread atual (a que `net_fetch` cria).
fn perform_fetch(request: &GlobalRequest) -> Result<GlobalResponse, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder().http_status_as_error(false).build().into();

    let mut builder = ureq::http::Request::builder().method(request.method()).uri(request.url());
    for (name, value) in request.headers().iter() {
        builder = builder.header(name, value);
    }

    let response = match request.body() {
        Some(body) => agent.run(builder.body(body.to_vec()).map_err(|error| error.to_string())?),
        None => agent.run(builder.body(()).map_err(|error| error.to_string())?),
    };
    let mut response = response.map_err(|error| error.to_string())?;

    let status = response.status().as_u16();
    let mut headers = GlobalHeaders::new();
    for (name, value) in response.headers() {
        headers.set(name.as_str(), &String::from_utf8_lossy(value.as_bytes()));
    }
    let body = response.body_mut().with_config().limit(1 << 30).read_to_vec().map_err(|error| error.to_string())?;

    GlobalResponse::new(ResponseBody::Bytes(body), status, Some(headers)).map_err(|error| error.to_string())
}
