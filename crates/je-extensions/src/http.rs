//! Cliente HTTP do marketplace: o `IRequestService` do VS Code, reduzido ao que o gerenciamento de extensões usa
//! (GET e POST com cabeçalhos, corpo e tempo limite). A interface é um trait para os testes injetarem respostas.
//!
//! A implementação de produção ([`UreqHttpClient`]) usa o `ureq` com `rustls` e o provedor de criptografia RustCrypto,
//! em Rust puro: o provedor padrão do `rustls` (`ring` ou `aws-lc`) compila C e é proibido pelo
//! `scripts/pure-rust-check.sh`. As raízes de certificado são as do `webpki-roots`.
//!
//! TODO(integração): proxy do sistema, `http.proxy`, certificados extras do sistema e autenticação (o
//! `IRequestService` do VS Code resolve tudo isso; o produto ainda não tem esse serviço).

use std::time::Duration;

/// O método HTTP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
}

/// `IRequestOptions`.
#[derive(Clone, Debug)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Option<Duration>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self { method: HttpMethod::Get, url: url.into(), headers: Vec::new(), body: None, timeout: None }
    }

    pub fn post(url: impl Into<String>, body: Vec<u8>) -> Self {
        Self { method: HttpMethod::Post, url: url.into(), headers: Vec::new(), body: Some(body), timeout: None }
    }

    pub fn with_headers(mut self, headers: &[(String, String)]) -> Self {
        self.headers.extend(headers.iter().cloned());
        self
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    pub fn with_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.timeout = timeout;
        self
    }
}

/// `IRequestContext`: a resposta inteira (o marketplace responde JSON e VSIX de tamanho limitado).
#[derive(Clone, Debug)]
pub struct HttpResponse {
    pub status: u16,
    /// Nomes em minúscula.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }

    /// `isSuccess`: 2xx.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status)
    }

    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status)
    }

    /// `asText`: o corpo como texto (UTF-8, com troca dos bytes inválidos).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// `asJson`: `None` com o corpo vazio.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<Option<T>, serde_json::Error> {
        if self.body.is_empty() { Ok(None) } else { serde_json::from_slice(&self.body).map(Some) }
    }
}

/// Por que a requisição falhou antes de haver uma resposta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpErrorKind {
    /// Sem rede (`isOfflineError`).
    Offline,
    /// Estourou o tempo limite.
    Timeout,
    Other,
}

#[derive(Clone, Debug)]
pub struct HttpError {
    pub kind: HttpErrorKind,
    pub message: String,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HttpError {}

/// `IRequestService#request`. Uma resposta com status de erro (4xx, 5xx) é uma resposta, não um erro.
pub trait HttpClient {
    fn request(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError>;
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use native::UreqHttpClient;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod native {
    use std::sync::Arc;

    use super::{HttpClient, HttpError, HttpErrorKind, HttpMethod, HttpRequest, HttpResponse};

    /// `ureq` com `rustls` + RustCrypto.
    pub struct UreqHttpClient {
        agent: ureq::Agent,
    }

    impl Default for UreqHttpClient {
        fn default() -> Self {
            Self::new()
        }
    }

    impl UreqHttpClient {
        pub fn new() -> Self {
            let provider = Arc::new(rustls_rustcrypto::provider());
            let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::Rustls).unversioned_rustls_crypto_provider(provider).build();
            let agent: ureq::Agent = ureq::Agent::config_builder().tls_config(tls).http_status_as_error(false).build().into();
            Self { agent }
        }
    }

    fn classify(error: &ureq::Error) -> HttpErrorKind {
        match error {
            ureq::Error::Timeout(_) => HttpErrorKind::Timeout,
            ureq::Error::HostNotFound | ureq::Error::ConnectionFailed | ureq::Error::Io(_) => HttpErrorKind::Offline,
            _ => HttpErrorKind::Other,
        }
    }

    impl HttpClient for UreqHttpClient {
        fn request(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
            let fail = |error: ureq::Error| HttpError { kind: classify(&error), message: error.to_string() };
            let response = match request.method {
                HttpMethod::Get => {
                    let mut builder = self.agent.get(&request.url);
                    for (name, value) in &request.headers {
                        builder = builder.header(name, value);
                    }
                    if let Some(timeout) = request.timeout {
                        builder = builder.config().timeout_global(Some(timeout)).build();
                    }
                    builder.call()
                }
                HttpMethod::Post => {
                    let mut builder = self.agent.post(&request.url);
                    for (name, value) in &request.headers {
                        builder = builder.header(name, value);
                    }
                    if let Some(timeout) = request.timeout {
                        builder = builder.config().timeout_global(Some(timeout)).build();
                    }
                    builder.send(request.body.as_deref().unwrap_or_default())
                }
            };
            let mut response = response.map_err(fail)?;
            let status = response.status().as_u16();
            let headers = response.headers().iter().map(|(name, value)| (name.as_str().to_ascii_lowercase(), String::from_utf8_lossy(value.as_bytes()).into_owned())).collect();
            let body = response.body_mut().with_config().limit(1 << 30).read_to_vec().map_err(fail)?;
            Ok(HttpResponse { status, headers, body })
        }
    }
}
