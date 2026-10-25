//! Porte de `src/vs/platform/request/node/requestService.ts`: o `RequestService` do lado Node (proxy, timeouts,
//! redirecionamentos, gzip, cancelamento, repetição de requisições idempotentes) e o `nodeRequest` que o executa.
//!
//! Decisões de tradução:
//! - `http.ClientRequest`, `http.IncomingMessage` e `http.request` do Node são [`ClientRequest`], [`IncomingMessage`] e
//!   [`RawRequestFunction`] (o `IRawRequestFunction`): o `nodeRequest` só fala com esse contrato, e é por ele que o
//!   `getRawRequest` injeta uma requisição de teste. Os eventos que o original escuta (`error`, `response`, `abort`)
//!   são registros de ouvintes ([`ClientRequest::on_error`] e companhia).
//! - O cliente de produção é o `ureq` com `rustls` e o provedor RustCrypto (Rust puro, o mesmo do cliente do
//!   marketplace em `je-extensions`; o provedor padrão do `rustls` compila C e `scripts/pure-rust-check.sh` o barra).
//!   O `ureq` é bloqueante: cada requisição roda numa thread e o laço de eventos consulta o resultado a cada
//!   [`POLL_INTERVAL_MS`] ms (a mesma técnica do `shell_env`). O corpo da resposta chega inteiro, num só pedaço de
//!   stream. `abort()` descarta o resultado da thread (o `ureq` não interrompe a E/S em curso).
//! - Sem redirecionamento no cliente (`max_redirects(0)`): quem segue o `Location` é o `nodeRequest`, como no original.
//!   Sem decodificação de `Content-Encoding` no cliente: o gzip é do `nodeRequest` (`createGunzip`). Se o `ureq` for
//!   compilado com a feature `gzip` por unificação de features do workspace (o `je-extensions` a liga) e já tiver
//!   descomprimido o corpo, o cabeçalho continua dizendo `gzip` e o corpo não começa com a assinatura `1f 8b`: nesse caso
//!   o corpo passa como está, em vez de falhar na segunda descompressão.
//! - O `req.setTimeout(ms)` do original, no Node, só emite o evento `timeout` no soquete ocioso (o pedido "deve ser
//!   destruído à mão", diz a documentação do Node) e o `nodeRequest` não escuta esse evento: então não aborta nada.
//!   [`ClientRequest::set_timeout`] do cliente `ureq` é, de propósito, sem efeito; o caminho com
//!   `isChromiumNetwork` (`setTimeout` do Node com `abort` e rejeição) é o do original.
//! - O código do erro de rede (`ECONNREFUSED`, `EPROTO`...) é a propriedade `code` do [`Error`], a que o
//!   `isTransientError` lê. O `ureq` não expõe o `errno`: o código sai do tipo do erro (e do `io::ErrorKind`).
//! - `lookupKerberosAuthorization` depende do módulo nativo `kerberos` (GSSAPI no Unix, SSPI no Windows), que exige C
//!   e `unsafe`; o porte calcula o SPN e registra o log como o original e rejeita dizendo que o módulo não está
//!   disponível, o que o `RequestService#lookupKerberosAuthorization` converte em `undefined`, como no original.
//! - `loadCertificates` (o `loadSystemCertificates` do `@vscode/proxy-agent`, que não está em `ref/`) lê o armazém de
//!   certificados do sistema: no Linux o `SSL_CERT_FILE`, `SSL_CERT_DIR` e os pacotes de CA dos locais usuais; no macOS o
//!   `security find-certificate -a -p`. No Windows devolve vazio (o armazém exige a API nativa). O `http.systemCertificatesNode`
//!   não muda nada aqui, porque só há uma via de leitura.

use std::cell::{Cell, RefCell};
use std::io::Read;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use je_base::async_::{async_fn, promises, timeout_with_token};
use je_base::buffer::{BytesOrString, VSBuffer, encode_base64, stream_to_buffer_readable_stream};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::parts::request::{HeaderValue, IHeaders, IRequestContext, IRequestOptions, IRequestResponse};
use je_base::platform::Platform;
use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;
use je_base::runtime::{self, TimerId};
use je_base::stream::{IStreamListener, ReadableStreamEvents, WriteableStream, listen_stream, new_writeable_stream};

use super::node_proxy::{Agent, IOptions, get_proxy_agent, parse_url};
use super::request::{AuthInfo, Credentials, IRequestService, RequestCompleteEvent, RequestLogger, read_header};
use crate::configuration::service::IConfigurationService;
use crate::configuration::ConfigurationOverrides;
use crate::environment::environment::NativeEnvironmentServiceApi;
use crate::log::{LogArg, LogMessage, LogServiceRef};
use crate::shell::shell_env::get_resolved_shell_env;

/// De quanto em quanto tempo o laço de eventos consulta o resultado da thread da requisição.
pub const POLL_INTERVAL_MS: u64 = 10;

/// `TRANSIENT_ERROR_CODES`.
const TRANSIENT_ERROR_CODES: [&str; 7] = [
    "EAI_AGAIN",    // DNS lookup timed out
    "ECONNREFUSED", // Connection refused by server
    "EHOSTDOWN",    // Host is down
    "EHOSTUNREACH", // No route to host
    "ENETDOWN",     // Network is down
    "ENETUNREACH",  // Network is unreachable
    "EPROTO",       // Protocol error (TLS/SSL handshake failure)
];

/// `IDEMPOTENT_HTTP_METHODS_REGEX` (`/^(GET|HEAD|OPTIONS)$/i`) aplicado a `options.type || 'GET'`.
fn is_idempotent_method(method: Option<&str>) -> bool {
    let method = method.filter(|method| !method.is_empty()).unwrap_or("GET");
    ["GET", "HEAD", "OPTIONS"].iter().any(|idempotent| method.eq_ignore_ascii_case(idempotent))
}

/// `isTransientError`.
fn is_transient_error(error: &Error) -> bool {
    error.prop("code").is_some_and(|code| TRANSIENT_ERROR_CODES.contains(&code))
}

/// `http.RequestOptions` com o que o `nodeRequestAttempt` preenche.
#[derive(Clone, Debug)]
pub struct RawRequestOptions {
    pub hostname: Option<String>,
    pub port: u16,
    /// `http:` ou `https:`.
    pub protocol: Option<String>,
    pub path: Option<String>,
    pub method: String,
    pub headers: Option<IHeaders>,
    pub agent: Option<Agent>,
    pub reject_unauthorized: bool,
    /// `usuario:senha`.
    pub auth: Option<String>,
    /// `cache: 'no-store'`.
    pub cache_no_store: bool,
}

/// `http.IncomingMessage`: o status, os cabeçalhos (nomes em minúsculas, como no Node) e o corpo.
#[derive(Clone)]
pub struct IncomingMessage {
    pub status_code: Option<u16>,
    pub headers: IHeaders,
    pub stream: Rc<dyn ReadableStreamEvents<BytesOrString>>,
}

/// O `callback` de `http.request(options, callback)`.
pub type ResponseCallback = Rc<dyn Fn(IncomingMessage)>;

/// `http.ClientRequest`, o que o `nodeRequest` usa dela.
pub trait ClientRequest {
    /// `req.on('error', handler)`.
    fn on_error(&self, handler: Rc<dyn Fn(Error)>);
    /// `req.on('response', handler)`: roda depois do `callback` da requisição.
    fn on_response(&self, handler: Rc<dyn Fn()>);
    /// `req.on('abort', handler)`.
    fn on_abort(&self, handler: Rc<dyn Fn()>);
    /// `req.setTimeout(ms)`.
    fn set_timeout(&self, ms: u64);
    /// `req.removeHeader(name)`.
    fn remove_header(&self, name: &str);
    /// `req.write(data)`.
    fn write(&self, data: &str);
    /// `req.end()`.
    fn end(&self);
    /// `req.abort()`.
    fn abort(&self);
}

/// `IRawRequestFunction`.
pub type RawRequestFunction = Rc<dyn Fn(&RawRequestOptions, ResponseCallback) -> Rc<dyn ClientRequest>>;

/// `NodeRequestOptions`.
#[derive(Clone, Default)]
pub struct NodeRequestOptions {
    /// O `IRequestOptions` que o `NodeRequestOptions` estende.
    pub base: IRequestOptions,
    pub agent: Option<Agent>,
    pub strict_ssl: Option<bool>,
    pub is_chromium_network: bool,
    pub get_raw_request: Option<Rc<dyn Fn(&IRequestOptions) -> RawRequestFunction>>,
}

impl From<IRequestOptions> for NodeRequestOptions {
    fn from(base: IRequestOptions) -> Self {
        Self { base, ..Self::default() }
    }
}

/// O `machine: 'local' | 'remote'` do construtor: de qual camada de configuração o serviço lê as `http.*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestMachine {
    Local,
    Remote,
}

/// O que `configure()` lê das configurações `http.*`.
#[derive(Clone, Debug, Default)]
struct HttpSettings {
    proxy_url: Option<String>,
    strict_ssl: bool,
    authorization: Option<String>,
}

/// `getConfigValue` e `configure` do `RequestService`.
struct HttpConfiguration {
    machine: RequestMachine,
    configuration_service: Rc<dyn IConfigurationService>,
}

fn as_string(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

impl HttpConfiguration {
    /// `getConfigValue<T>(key, fallback)`.
    fn get_config_value(&self, key: &str, fallback: Dynamic) -> Dynamic {
        if self.machine == RequestMachine::Remote {
            return self.configuration_service.get_value(Some(key), None);
        }
        let values = self.configuration_service.inspect(key, &ConfigurationOverrides::default());
        // `values.userLocalValue ?? values.defaultValue ?? fallback`
        [values.user_local_value(), values.default_value()].into_iter().find(|value| !value.is_nullish()).unwrap_or(fallback)
    }

    /// `configure()`.
    fn read_settings(&self) -> HttpSettings {
        HttpSettings {
            proxy_url: as_string(&self.get_config_value("http.proxy", Dynamic::Undefined)),
            strict_ssl: self.get_config_value("http.proxyStrictSSL", Dynamic::Undefined).is_truthy(),
            authorization: as_string(&self.get_config_value("http.proxyAuthorization", Dynamic::Undefined)),
        }
    }
}

/// O ambiente do processo (`process.env`).
fn process_environment() -> ProcessEnvironment {
    std::env::vars_os().filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?))).collect()
}

/// This service exposes the `request` API, while using the global or configured proxy settings.
pub struct RequestService {
    base: Rc<Disposable>,
    logger: Rc<RequestLogger>,
    log_service: LogServiceRef,
    http: Rc<HttpConfiguration>,
    configuration_service: Rc<dyn IConfigurationService>,
    environment_service: Rc<dyn NativeEnvironmentServiceApi>,
    settings: Rc<RefCell<HttpSettings>>,
    shell_env_error_logged: Rc<Cell<bool>>,
}

impl RequestService {
    pub fn new(
        machine: RequestMachine,
        configuration_service: Rc<dyn IConfigurationService>,
        environment_service: Rc<dyn NativeEnvironmentServiceApi>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let http = Rc::new(HttpConfiguration { machine, configuration_service: configuration_service.clone() });
        let settings = Rc::new(RefCell::new(http.read_settings()));
        let listener = configuration_service.on_did_change_configuration().listen({
            let (http, settings) = (http.clone(), settings.clone());
            move |event| {
                if event.affects_configuration("http", None) {
                    *settings.borrow_mut() = http.read_settings();
                }
            }
        });
        base.register(listener);
        Rc::new(Self {
            base,
            logger: RequestLogger::new(log_service.clone()),
            log_service,
            http,
            configuration_service,
            environment_service,
            settings,
            shell_env_error_logged: Rc::new(Cell::new(false)),
        })
    }

    /// `request(options: NodeRequestOptions, token)`: o `IRequestService#request` com as opções do Node (o agent, o
    /// `isChromiumNetwork` e o `getRawRequest`).
    pub fn request_with(&self, mut options: NodeRequestOptions, token: CancellationToken) -> Promise<IRequestContext> {
        let (settings, logger, log_service) = (self.settings.clone(), self.logger.clone(), self.log_service.clone());
        let (configuration_service, environment_service, logged) = (self.configuration_service.clone(), self.environment_service.clone(), self.shell_env_error_logged.clone());
        async_fn(async move {
            let (proxy_url, strict_ssl) = {
                let settings = settings.borrow();
                (settings.proxy_url.clone(), settings.strict_ssl)
            };

            let shell_env = match get_resolved_shell_env(&configuration_service, &log_service, &environment_service.args(), &process_environment()).await {
                Ok(shell_env) => Some(shell_env),
                Err(error) => {
                    if !logged.replace(true) {
                        log_service.error_args(&LogMessage::from("resolving shell environment failed"), &[LogArg::from(error.message.as_str())]);
                    }
                    None
                }
            };

            let mut env = process_environment();
            env.extend(shell_env.unwrap_or_default());
            let agent = match options.agent.clone() {
                Some(agent) => Some(agent),
                None => get_proxy_agent(options.base.url.as_deref().unwrap_or(""), &env, &IOptions { proxy_url, strict_ssl: Some(strict_ssl) }).await?,
            };

            options.agent = agent;
            options.strict_ssl = Some(strict_ssl);

            let authorization = settings.borrow().authorization.clone().filter(|authorization| !authorization.is_empty());
            if let Some(authorization) = authorization {
                let mut headers = options.base.headers.take().unwrap_or_default();
                headers.insert("Proxy-Authorization".to_owned(), HeaderValue::One(authorization));
                options.base.headers = Some(headers);
            }

            let logged_options = options.base.clone();
            logger.log_and_request(&logged_options, move || node_request(options, token)).await
        })
    }
}

impl IDisposable for RequestService {
    fn dispose(&self) {
        self.logger.dispose();
        self.base.dispose();
    }
}

impl IRequestService for RequestService {
    fn on_did_complete_request(&self) -> Event<RequestCompleteEvent> {
        self.logger.on_did_complete_request()
    }

    fn request(&self, options: IRequestOptions, token: CancellationToken) -> Promise<IRequestContext> {
        self.request_with(NodeRequestOptions::from(options), token)
    }

    fn resolve_proxy(&self, _url: &str) -> Promise<Option<String>> {
        Promise::resolved(None) // currently not implemented in node
    }

    fn lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        Promise::resolved(None) // currently not implemented in node
    }

    fn lookup_kerberos_authorization(&self, url: &str) -> Promise<Option<String>> {
        let spn_config = as_string(&self.http.get_config_value("http.proxyKerberosServicePrincipal", Dynamic::Undefined));
        let log_service = self.log_service.clone();
        lookup_kerberos_authorization(url, spn_config.as_deref(), &self.log_service, "RequestService#lookupKerberosAuthorization").then_result(move |result| match result {
            Ok(response) => Ok(Some(format!("Negotiate {response}"))),
            Err(error) => {
                log_service.debug_args("RequestService#lookupKerberosAuthorization Kerberos authentication failed", &[LogArg::from(&error)]);
                Ok(None)
            }
        })
    }

    fn load_certificates(&self) -> Promise<Vec<String>> {
        Promise::resolved(load_system_certificates(&self.log_service))
    }
}

/// `lookupKerberosAuthorization`: o SPN (`spnConfig` ou `HTTP/<host>` no Windows e `HTTP@<host>` nos demais) e o log do
/// original; a troca de tokens exige o módulo nativo `kerberos` (ver o topo) e a promessa é rejeitada.
pub fn lookup_kerberos_authorization(url_str: &str, spn_config: Option<&str>, log_service: &LogServiceRef, log_prefix: &str) -> Promise<String> {
    let Some(url) = crate::url::whatwg::try_new_url(&url_str.into()) else {
        return Promise::rejected(Error::named("TypeError", "Invalid URL").with_prop("code", "ERR_INVALID_URL"));
    };
    let hostname = crate::url::whatwg::hostname(&url).to_string_lossy();
    let spn = match spn_config.filter(|spn| !spn.is_empty()) {
        Some(spn) => spn.to_owned(),
        None if Platform::detect().is_windows => format!("HTTP/{hostname}"),
        None => format!("HTTP@{hostname}"),
    };
    let proxy_url = format!("proxyURL:{}", crate::url::whatwg::href(&url).to_string_lossy());
    let spn_arg = format!("spn:{spn}");
    log_service.debug_args(&format!("{log_prefix} Kerberos authentication lookup"), &[LogArg::from(proxy_url.as_str()), LogArg::from(spn_arg.as_str())]);
    Promise::rejected(Error::new("Kerberos authentication is not available in this build: the native kerberos module (GSSAPI/SSPI) is not ported").with_prop("code", "MODULE_NOT_FOUND"))
}

/// `loadSystemCertificates`: os certificados (PEM) do armazém do sistema, sem repetição, na ordem em que aparecem.
fn load_system_certificates(log_service: &LogServiceRef) -> Vec<String> {
    let mut certificates: Vec<String> = Vec::new();
    for text in read_system_certificate_texts(log_service) {
        for certificate in split_pem_certificates(&text) {
            if !certificates.contains(&certificate) {
                certificates.push(certificate);
            }
        }
    }
    certificates
}

fn split_pem_certificates(text: &str) -> Vec<String> {
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";
    let mut certificates = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(BEGIN) {
        let Some(length) = rest[start..].find(END) else { break };
        let end = start + length + END.len();
        certificates.push(rest[start..end].to_owned());
        rest = &rest[end..];
    }
    certificates
}

#[cfg(target_os = "macos")]
fn read_system_certificate_texts(log_service: &LogServiceRef) -> Vec<String> {
    ["/System/Library/Keychains/SystemRootCertificates.keychain", "/Library/Keychains/System.keychain"]
        .iter()
        .filter_map(|keychain| match std::process::Command::new("/usr/bin/security").args(["find-certificate", "-a", "-p", keychain]).output() {
            Ok(output) if output.status.success() => Some(String::from_utf8_lossy(&output.stdout).into_owned()),
            Ok(_) => None,
            Err(error) => {
                log_service.debug(&format!("loadSystemCertificates: security find-certificate failed: {error}"));
                None
            }
        })
        .collect()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn read_system_certificate_texts(_log_service: &LogServiceRef) -> Vec<String> {
    let mut texts = Vec::new();
    let mut files: Vec<std::path::PathBuf> = std::env::var_os("SSL_CERT_FILE").map(std::path::PathBuf::from).into_iter().collect();
    // O primeiro pacote de CA que existir (cada distribuição usa um lugar).
    files.extend(
        [
            "/etc/ssl/certs/ca-certificates.crt",
            "/etc/pki/tls/certs/ca-bundle.crt",
            "/etc/ssl/ca-bundle.pem",
            "/etc/pki/tls/cacert.pem",
            "/etc/ssl/cert.pem",
            "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
        ]
        .iter()
        .map(std::path::PathBuf::from)
        .find(|path| path.is_file()),
    );
    if let Some(dir) = std::env::var_os("SSL_CERT_DIR") {
        for dir in std::env::split_paths(&dir) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                files.extend(entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|ext| ext == "pem" || ext == "crt")));
            }
        }
    }
    for file in files {
        if let Ok(text) = std::fs::read_to_string(&file) {
            texts.push(text);
        }
    }
    texts
}

#[cfg(not(unix))]
fn read_system_certificate_texts(log_service: &LogServiceRef) -> Vec<String> {
    log_service.debug("loadSystemCertificates: the system certificate store is not read on this platform");
    Vec::new()
}

/// `nodeRequest`: a requisição com até três tentativas para os métodos idempotentes que falham por erro transitório
/// (espera de 100 ms vezes o número da tentativa, cancelável pelo token).
pub fn node_request(options: NodeRequestOptions, token: CancellationToken) -> Promise<IRequestContext> {
    async_fn(async move {
        let max_retries: u64 = 3;
        let mut last_error: Option<Error> = None;
        let is_idempotent = is_idempotent_method(options.base.r#type.as_deref());

        for attempt in 1..=max_retries {
            match node_request_attempt(options.clone(), token.clone()).await {
                Ok(context) => return Ok(context),
                Err(error) => {
                    last_error = Some(error.clone());
                    if errors::is_cancellation_error(&error) {
                        return Err(error);
                    }

                    if !is_idempotent || !is_transient_error(&error) || attempt == max_retries {
                        return Err(error);
                    }

                    timeout_with_token(100 * attempt, &token).await?;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| Error::new("undefined")))
    })
}

fn dispose_slot(slot: &RefCell<Option<DisposableRef>>) {
    let disposable = slot.borrow_mut().take();
    if let Some(disposable) = disposable {
        disposable.dispose();
    }
}

fn node_request_attempt(options: NodeRequestOptions, token: CancellationToken) -> Promise<IRequestContext> {
    promises::with_async_body(move |resolver| {
        async_fn(async move {
            let endpoint = parse_url(options.base.url.as_deref().unwrap_or(""));
            let raw_request = match &options.get_raw_request {
                Some(get_raw_request) => get_raw_request(&options.base),
                None => default_raw_request(),
            };
            let is_https = endpoint.protocol.as_deref() == Some("https:");

            let method = options.base.r#type.clone().filter(|method| !method.is_empty()).unwrap_or_else(|| "GET".to_owned());
            let has_credentials = options.base.user.as_deref().is_some_and(|user| !user.is_empty()) && options.base.password.as_deref().is_some_and(|password| !password.is_empty());
            let opts = RawRequestOptions {
                hostname: endpoint.hostname,
                port: endpoint.port.as_deref().and_then(|port| port.parse().ok()).unwrap_or(if is_https { 443 } else { 80 }),
                protocol: endpoint.protocol,
                path: endpoint.path,
                method,
                headers: options.base.headers.clone(),
                agent: options.agent.clone(),
                reject_unauthorized: options.strict_ssl.unwrap_or(true),
                auth: has_credentials.then(|| format!("{}:{}", options.base.user.as_deref().unwrap_or(""), options.base.password.as_deref().unwrap_or(""))),
                cache_no_store: options.base.disable_cache == Some(true),
            };

            let on_response: ResponseCallback = {
                let (options, token, resolver) = (options.clone(), token.clone(), resolver.clone());
                Rc::new(move |res: IncomingMessage| {
                    let follow_redirects = options.base.follow_redirects.unwrap_or(3);
                    let location = read_header(Some(&res.headers), "location").filter(|location| !location.is_empty());
                    match (res.status_code, location) {
                        (Some(status), Some(location)) if (300..400).contains(&status) && follow_redirects > 0 => {
                            let redirected = NodeRequestOptions { base: IRequestOptions { url: Some(location), follow_redirects: Some(follow_redirects - 1), ..options.base.clone() }, ..options.clone() };
                            node_request(redirected, token.clone()).pipe_to(&resolver);
                        }
                        _ => {
                            let mut stream = res.stream.clone();

                            // Responses from Electron net module should be treated as response
                            // from browser, which will apply gzip filter and decompress the response
                            // using zlib before passing the result to us. Following step can be bypassed
                            // in this case and proceed further.
                            // Refs https://source.chromium.org/chromium/chromium/src/+/main:net/url_request/url_request_http_job.cc;l=1266-1318
                            if !options.is_chromium_network && read_header(Some(&res.headers), "content-encoding").as_deref() == Some("gzip") {
                                stream = gunzip_stream(stream);
                            }

                            resolver.resolve(IRequestContext { res: IRequestResponse { headers: res.headers.clone(), status_code: res.status_code }, stream: stream_to_buffer_readable_stream(&*stream), timings: None });
                        }
                    }
                })
            };
            let req = raw_request(&opts, on_response);

            req.on_error({
                let resolver = resolver.clone();
                Rc::new(move |error: Error| resolver.reject(error))
            });

            // Handle timeout
            if let Some(timeout_ms) = options.base.timeout.filter(|timeout| *timeout != 0) {
                // Chromium network requests do not support the `timeout` option
                if options.is_chromium_network {
                    // Use Node's setTimeout for Chromium network requests
                    let timer = runtime::set_timeout(
                        {
                            let (req, resolver) = (req.clone(), resolver.clone());
                            move || {
                                req.abort();
                                resolver.reject(Error::new(format!("Request timeout after {timeout_ms}ms")));
                            }
                        },
                        timeout_ms,
                    );

                    // Clear timeout when request completes
                    req.on_response(Rc::new(move || runtime::clear_timeout(timer)));
                    req.on_error(Rc::new(move |_: Error| runtime::clear_timeout(timer)));
                    req.on_abort(Rc::new(move || runtime::clear_timeout(timer)));
                } else {
                    req.set_timeout(timeout_ms);
                }
            }

            // Chromium will abort the request if forbidden headers are set.
            // Ref https://source.chromium.org/chromium/chromium/src/+/main:services/network/public/cpp/header_util.cc;l=14-48;
            // for additional context.
            if options.is_chromium_network {
                req.remove_header("Content-Length");
            }

            if let Some(data) = options.base.data.as_deref().filter(|data| !data.is_empty()) {
                req.write(data);
            }

            req.end();

            let cancellation_listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
            let listener = token.on_cancellation_requested().listen({
                let (cancellation_listener, req, resolver) = (cancellation_listener.clone(), req.clone(), resolver.clone());
                move |_| {
                    dispose_slot(&cancellation_listener);
                    req.abort();

                    resolver.reject(Error::cancellation());
                }
            });
            *cancellation_listener.borrow_mut() = Some(listener);

            req.on_response({
                let cancellation_listener = cancellation_listener.clone();
                Rc::new(move || dispose_slot(&cancellation_listener))
            });
            req.on_error(Rc::new(move |_: Error| dispose_slot(&cancellation_listener)));
            Ok::<(), Error>(())
        })
    })
}

/// `res.pipe(createGunzip())`: lê o corpo até o fim e entrega o descomprimido. Um corpo que não começa com a assinatura
/// gzip (o cliente já o descomprimiu, ver o topo) passa como está; um gzip inválido vira um erro de stream
/// (`Z_DATA_ERROR`).
fn gunzip_stream(source: Rc<dyn ReadableStreamEvents<BytesOrString>>) -> Rc<dyn ReadableStreamEvents<BytesOrString>> {
    let output = new_writeable_stream::<BytesOrString>(None, None);
    let chunks: Rc<RefCell<Vec<u8>>> = Rc::default();
    let on_data = {
        let chunks = chunks.clone();
        Rc::new(move |chunk: &BytesOrString| match chunk {
            BytesOrString::Bytes(bytes) => chunks.borrow_mut().extend_from_slice(bytes),
            BytesOrString::Text(text) => chunks.borrow_mut().extend_from_slice(text.as_bytes()),
        })
    };
    let on_error = {
        let output = output.clone();
        Rc::new(move |error: &Error| output.error(error.clone()))
    };
    let on_end = {
        let output = output.clone();
        Rc::new(move || {
            let compressed = std::mem::take(&mut *chunks.borrow_mut());
            if compressed.starts_with(&[0x1f, 0x8b]) {
                let mut decoded = Vec::new();
                match flate2::read::GzDecoder::new(compressed.as_slice()).read_to_end(&mut decoded) {
                    Ok(_) => output.end(Some(BytesOrString::Bytes(decoded))),
                    Err(error) => {
                        output.error(Error::new(error.to_string()).with_prop("code", "Z_DATA_ERROR"));
                        output.end(None);
                    }
                }
            } else {
                output.end(Some(BytesOrString::Bytes(compressed)));
            }
        })
    };
    listen_stream(&*source, IStreamListener { on_data, on_error, on_end }, None);
    output
}

/// O `http.request`/`https.request` de produção (ver o topo): o `ureq` escolhe o protocolo pela URL.
fn default_raw_request() -> RawRequestFunction {
    Rc::new(|options: &RawRequestOptions, callback: ResponseCallback| -> Rc<dyn ClientRequest> { Rc::new(UreqRequest::new(options.clone(), callback)) })
}

#[derive(Default)]
struct UreqHandlers {
    error: Vec<Rc<dyn Fn(Error)>>,
    response: Vec<Rc<dyn Fn()>>,
    abort: Vec<Rc<dyn Fn()>>,
}

struct UreqState {
    options: RawRequestOptions,
    callback: ResponseCallback,
    handlers: RefCell<UreqHandlers>,
    removed_headers: RefCell<Vec<String>>,
    body: RefCell<Vec<u8>>,
    ended: Cell<bool>,
    aborted: Cell<bool>,
    cancelled: Arc<AtomicBool>,
    outcome: Arc<Mutex<Option<TransportOutcome>>>,
    poll_timer: Cell<Option<TimerId>>,
}

/// A `ClientRequest` sobre o `ureq`.
struct UreqRequest {
    state: Rc<UreqState>,
}

impl UreqRequest {
    fn new(options: RawRequestOptions, callback: ResponseCallback) -> Self {
        Self {
            state: Rc::new(UreqState {
                options,
                callback,
                handlers: RefCell::default(),
                removed_headers: RefCell::default(),
                body: RefCell::default(),
                ended: Cell::new(false),
                aborted: Cell::new(false),
                cancelled: Arc::new(AtomicBool::new(false)),
                outcome: Arc::new(Mutex::new(None)),
                poll_timer: Cell::new(None),
            }),
        }
    }
}

impl UreqState {
    fn fail(&self, error: Error) {
        let handlers = self.handlers.borrow().error.clone();
        for handler in handlers {
            handler(error.clone());
        }
    }

    fn transport_request(&self) -> TransportRequest {
        let options = &self.options;
        let protocol = options.protocol.as_deref().unwrap_or("http:");
        let host = options.hostname.as_deref().filter(|host| !host.is_empty()).unwrap_or("localhost");
        let host = if host.contains(':') { format!("[{host}]") } else { host.to_owned() };
        let path = options.path.as_deref().filter(|path| !path.is_empty()).unwrap_or("/");
        let path = if path.starts_with('/') { path.to_owned() } else { format!("/{path}") };

        let removed = self.removed_headers.borrow();
        let mut headers: Vec<(String, String)> = Vec::new();
        for (name, value) in options.headers.iter().flatten() {
            // O `ureq` calcula o `Content-Length` do corpo que envia.
            if name.eq_ignore_ascii_case("content-length") || removed.iter().any(|removed| removed.eq_ignore_ascii_case(name)) {
                continue;
            }
            match value {
                HeaderValue::One(value) => headers.push((name.clone(), value.clone())),
                HeaderValue::Many(values) => headers.extend(values.iter().map(|value| (name.clone(), value.clone()))),
            }
        }
        if let Some(auth) = &options.auth
            && !headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
        {
            headers.push(("Authorization".to_owned(), format!("Basic {}", encode_base64(&VSBuffer::from_string(auth), true, false))));
        }

        TransportRequest {
            method: options.method.clone(),
            url: format!("{protocol}//{host}:{}{path}", options.port),
            headers,
            body: self.body.borrow().clone(),
            proxy_url: options.agent.as_ref().map(proxy_url_of),
            reject_unauthorized: options.reject_unauthorized,
        }
    }

    /// A resposta chegou: o `callback` primeiro e depois os ouvintes de `response`, nessa ordem, como no Node.
    fn deliver(&self, response: TransportResponse) {
        let message = IncomingMessage { status_code: Some(response.status), headers: response_headers(&response.headers), stream: body_stream(response.body) };
        (self.callback)(message);
        let handlers = self.handlers.borrow().response.clone();
        for handler in handlers {
            handler();
        }
    }
}

/// A URL do proxy com a autenticação (`usuario:senha`, com escape) embutida.
fn proxy_url_of(agent: &Agent) -> String {
    let Ok(mut url) = ::url::Url::parse(&agent.proxy_url) else {
        return agent.proxy_url.clone();
    };
    if let Some((user, password)) = agent.auth.as_deref().map(|auth| auth.split_once(':').unwrap_or((auth, ""))) {
        let _ = url.set_username(user);
        let _ = url.set_password(Some(password).filter(|password| !password.is_empty()));
    }
    url.to_string()
}

/// Os cabeçalhos como o Node entrega: nomes em minúsculas, `set-cookie` em lista e os demais repetidos unidos por `, `.
fn response_headers(raw: &[(String, String)]) -> IHeaders {
    let mut headers = IHeaders::new();
    for (name, value) in raw {
        let name = name.to_ascii_lowercase();
        match headers.get_mut(&name) {
            Some(HeaderValue::Many(values)) => values.push(value.clone()),
            Some(HeaderValue::One(existing)) => {
                existing.push_str(", ");
                existing.push_str(value);
            }
            None if name == "set-cookie" => {
                headers.insert(name, HeaderValue::Many(vec![value.clone()]));
            }
            None => {
                headers.insert(name, HeaderValue::One(value.clone()));
            }
        }
    }
    headers
}

fn body_stream(body: Vec<u8>) -> Rc<dyn ReadableStreamEvents<BytesOrString>> {
    let stream = new_writeable_stream::<BytesOrString>(None, None);
    if !body.is_empty() {
        let _ = stream.write(BytesOrString::Bytes(body));
    }
    stream.end(None);
    stream
}

/// Consulta o resultado da thread e, se ainda não chegou, agenda a próxima consulta.
fn poll_outcome(state: Rc<UreqState>) {
    if state.aborted.get() {
        return;
    }
    let outcome = state.outcome.lock().ok().and_then(|mut outcome| outcome.take());
    match outcome {
        None => {
            let next = state.clone();
            state.poll_timer.set(Some(runtime::set_timeout(move || poll_outcome(next), POLL_INTERVAL_MS)));
        }
        Some(Ok(response)) => state.deliver(response),
        Some(Err(failure)) => {
            let error = Error::new(failure.message);
            state.fail(match failure.code {
                Some(code) => error.with_prop("code", code),
                None => error,
            });
        }
    }
}

impl ClientRequest for UreqRequest {
    fn on_error(&self, handler: Rc<dyn Fn(Error)>) {
        self.state.handlers.borrow_mut().error.push(handler);
    }

    fn on_response(&self, handler: Rc<dyn Fn()>) {
        self.state.handlers.borrow_mut().response.push(handler);
    }

    fn on_abort(&self, handler: Rc<dyn Fn()>) {
        self.state.handlers.borrow_mut().abort.push(handler);
    }

    /// O Node só emite `timeout` no soquete ocioso, e ninguém escuta (ver o topo): sem efeito.
    fn set_timeout(&self, _ms: u64) {}

    fn remove_header(&self, name: &str) {
        self.state.removed_headers.borrow_mut().push(name.to_owned());
    }

    fn write(&self, data: &str) {
        self.state.body.borrow_mut().extend_from_slice(data.as_bytes());
    }

    fn end(&self) {
        let state = &self.state;
        if state.ended.replace(true) || state.aborted.get() {
            return;
        }
        let request = state.transport_request();
        let (cancelled, outcome) = (state.cancelled.clone(), state.outcome.clone());
        let spawned = std::thread::Builder::new().name("je request".to_owned()).spawn(move || {
            let result = if cancelled.load(Ordering::SeqCst) { Err(TransportError { code: None, message: "aborted".to_owned() }) } else { perform_request(request) };
            *outcome.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
        });
        match spawned {
            Ok(_) => poll_outcome(state.clone()),
            Err(error) => state.fail(Error::new(error.to_string())),
        }
    }

    fn abort(&self) {
        let state = &self.state;
        if state.aborted.replace(true) {
            return;
        }
        state.cancelled.store(true, Ordering::SeqCst);
        if let Some(timer) = state.poll_timer.take() {
            runtime::clear_timeout(timer);
        }
        let handlers = state.handlers.borrow().abort.clone();
        for handler in handlers {
            handler();
        }
    }
}

struct TransportRequest {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    proxy_url: Option<String>,
    reject_unauthorized: bool,
}

struct TransportResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct TransportError {
    /// O `code` do erro do Node (`ECONNREFUSED`...), quando se sabe.
    code: Option<&'static str>,
    message: String,
}

type TransportOutcome = Result<TransportResponse, TransportError>;

fn io_error_code(kind: std::io::ErrorKind) -> Option<&'static str> {
    use std::io::ErrorKind;
    match kind {
        ErrorKind::ConnectionRefused => Some("ECONNREFUSED"),
        ErrorKind::ConnectionReset => Some("ECONNRESET"),
        ErrorKind::ConnectionAborted => Some("ECONNABORTED"),
        ErrorKind::TimedOut => Some("ETIMEDOUT"),
        ErrorKind::HostUnreachable => Some("EHOSTUNREACH"),
        ErrorKind::NetworkUnreachable => Some("ENETUNREACH"),
        ErrorKind::NetworkDown => Some("ENETDOWN"),
        _ => None,
    }
}

fn transport_error(error: ureq::Error) -> TransportError {
    let code = match &error {
        ureq::Error::HostNotFound => Some("ENOTFOUND"),
        ureq::Error::ConnectionFailed => Some("ECONNREFUSED"),
        ureq::Error::Timeout(_) => Some("ETIMEDOUT"),
        ureq::Error::Io(io_error) => io_error_code(io_error.kind()),
        other => {
            let text = other.to_string().to_lowercase();
            ["tls", "rustls", "certificate", "handshake"].iter().any(|marker| text.contains(marker)).then_some("EPROTO")
        }
    };
    TransportError { code, message: error.to_string() }
}

/// Faz a requisição, bloqueando a thread (a do `std::thread::spawn` de `end`).
fn perform_request(request: TransportRequest) -> TransportOutcome {
    let provider = Arc::new(rustls_rustcrypto::provider());
    let tls = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .unversioned_rustls_crypto_provider(provider)
        .disable_verification(!request.reject_unauthorized)
        .build();
    // Sem proxy do `ureq` pelo ambiente: o proxy é o que o `get_proxy_agent` escolheu (ou nenhum). Sem redirecionamento:
    // o `nodeRequest` segue o `Location`.
    let proxy = match &request.proxy_url {
        Some(url) => Some(ureq::Proxy::new(url).map_err(transport_error)?),
        None => None,
    };
    let agent: ureq::Agent = ureq::Agent::config_builder().tls_config(tls).proxy(proxy).http_status_as_error(false).max_redirects(0).max_redirects_will_error(false).build().into();

    let mut builder = ureq::http::Request::builder().method(request.method.as_str()).uri(request.url.as_str());
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let invalid = |error: ureq::http::Error| TransportError { code: None, message: error.to_string() };
    let response = if request.body.is_empty() {
        agent.run(builder.body(()).map_err(invalid)?)
    } else {
        agent.run(builder.body(request.body).map_err(invalid)?)
    };
    let mut response = response.map_err(transport_error)?;

    let status = response.status().as_u16();
    let headers = response.headers().iter().map(|(name, value)| (name.as_str().to_owned(), String::from_utf8_lossy(value.as_bytes()).into_owned())).collect();
    let body = response.body_mut().with_config().limit(1 << 30).read_to_vec().map_err(transport_error)?;
    Ok(TransportResponse { status, headers, body })
}
