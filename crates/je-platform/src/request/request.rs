//! Porte de `src/vs/platform/request/common/request.ts`.
//!
//! Decisões de tradução:
//! - `AbstractRequestService` (classe abstrata) é o [`RequestLogger`]: o serviço concreto o guarda e chama
//!   [`RequestLogger::log_and_request`] (o `logAndRequest` protegido do original). O evento
//!   `onDidCompleteRequest` é o do [`RequestLogger`].
//! - O `IRequestService` é o trait [`IRequestService`]; as promessas são [`Promise`].
//! - `asJson` lê o texto com o parser JSON do `je-base` em modo estrito (sem comentários nem vírgula final), e a
//!   mensagem de um JSON inválido é a desse parser (a do V8 não é reproduzida); o sufixo `:\n<texto>` é o do
//!   original.

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, stream_to_buffer};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::error_message::to_error_message_of;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::json::{self, ParseOptions};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::parts::request::{HeaderValue, IHeaders, IRequestContext, IRequestOptions};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::stream::{IStreamListener, ReadableStream, ReadableStreamEvents, listen_stream};
use je_instantiation::service_id;

use crate::js::from_json;
use crate::log::{LogArg, LogMessage, LogServiceRef};

/// `NO_FETCH_TELEMETRY`: o `callSite` que impede a telemetria da requisição (para o envio de telemetria não
/// entrar em ciclo).
pub const NO_FETCH_TELEMETRY: &str = "NO_FETCH_TELEMETRY";

/// `IRequestCompleteEvent`.
#[derive(Clone, Debug, PartialEq)]
pub struct RequestCompleteEvent {
    pub call_site: String,
    pub latency: f64,
    pub status_code: Option<u16>,
}

/// `AuthInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthInfo {
    pub is_proxy: bool,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub realm: String,
    pub attempt: u32,
}

/// `Credentials`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// `IRequestService`.
pub trait IRequestService {
    /// Dispara quando uma requisição termina (com sucesso ou com uma resposta de erro).
    fn on_did_complete_request(&self) -> Event<RequestCompleteEvent>;
    fn request(&self, options: IRequestOptions, token: CancellationToken) -> Promise<IRequestContext>;
    fn resolve_proxy(&self, url: &str) -> Promise<Option<String>>;
    fn lookup_authorization(&self, auth_info: AuthInfo) -> Promise<Option<Credentials>>;
    fn lookup_kerberos_authorization(&self, url: &str) -> Promise<Option<String>>;
    fn load_certificates(&self) -> Promise<Vec<String>>;
}

service_id!(pub I_REQUEST_SERVICE: dyn IRequestService = "requestService");

/// `LoggableHeaders`: os cabeçalhos com `Authorization` e `Proxy-Authorization` mascarados.
fn loggable_headers(original: &IHeaders) -> Dynamic {
    Dynamic::object(original.iter().map(|(key, value)| {
        let lower = key.to_lowercase();
        let value = if lower == "authorization" || lower == "proxy-authorization" { Dynamic::string("*****") } else { header_value_to_dynamic(value) };
        (key.as_str(), value)
    }))
}

fn header_value_to_dynamic(value: &HeaderValue) -> Dynamic {
    match value {
        HeaderValue::One(text) => Dynamic::string(text),
        HeaderValue::Many(items) => Dynamic::array(items.iter().map(|item| Dynamic::string(item))),
    }
}

/// O código (`ECONNRESET`) ou, na falta dele, o nome de um valor lançado: nunca a mensagem, que pode repetir a URL.
fn loggable_error_code(error: &Error) -> String {
    let allowed = |text: &str| !text.is_empty() && text.len() <= 64 && text.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if let Some(code) = error.prop("code").filter(|code| allowed(code)) {
        return code.to_owned();
    }
    if allowed(&error.name) { error.name.clone() } else { "error".to_owned() }
}

fn type_arg(options: &IRequestOptions) -> LogArg {
    LogArg::Value(options.r#type.as_deref().map_or(Dynamic::Undefined, Dynamic::string))
}

/// O estado de `AbstractRequestService`: o contador de requisições, o evento de fim e o log.
pub struct RequestLogger {
    base: Rc<Disposable>,
    log_service: LogServiceRef,
    counter: Cell<u32>,
    on_did_complete_request: Rc<Emitter<RequestCompleteEvent>>,
}

impl RequestLogger {
    pub fn new(log_service: LogServiceRef) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_complete_request = Emitter::new();
        base.register(on_did_complete_request.clone());
        Rc::new(Self { base, log_service, counter: Cell::new(0), on_did_complete_request })
    }

    /// `onDidCompleteRequest`.
    pub fn on_did_complete_request(&self) -> Event<RequestCompleteEvent> {
        self.on_did_complete_request.event()
    }

    fn next_prefix(&self, subject: &str) -> String {
        self.counter.set(self.counter.get() + 1);
        format!("#{}: {subject}", self.counter.get())
    }

    /// `logAndRequest`: faz a requisição registrando o início, o fim ou o erro. Com `logUrl: false` registra só
    /// o host, o status e o nome ou código do erro.
    pub fn log_and_request(self: &Rc<Self>, options: &IRequestOptions, request: impl FnOnce() -> Promise<IRequestContext> + 'static) -> Promise<IRequestContext> {
        let this = self.clone();
        let options = options.clone();
        let log_url = options.log_url != Some(false);
        let prefix = if log_url {
            self.next_prefix(options.url.as_deref().unwrap_or("undefined"))
        } else {
            // `new URL(options.url ?? '').host`; se não der, a URL é registrada como inválida.
            let host = options
                .url
                .as_deref()
                .and_then(|url| crate::url::whatwg::try_new_url(&url.into()))
                .map_or_else(|| "<invalid url>".to_owned(), |url| crate::url::whatwg::host(&url).to_string_lossy());
            self.next_prefix(&host)
        };
        if log_url {
            let headers = loggable_headers(&options.headers.clone().unwrap_or_default());
            self.log_service.trace_args(&format!("{prefix} - begin"), &[type_arg(&options), LogArg::Value(headers)]);
        } else {
            self.log_service.trace_args(&format!("{prefix} - begin"), &[type_arg(&options)]);
        }
        let start_time = runtime::date_now();
        async_fn(async move {
            match request().await {
                Ok(result) => {
                    let status = result.res.status_code.map_or(Dynamic::Undefined, |code| Dynamic::Number(f64::from(code)));
                    if log_url {
                        let headers = Dynamic::object(result.res.headers.iter().map(|(k, v)| (k.as_str(), header_value_to_dynamic(v))));
                        this.log_service.trace_args(&format!("{prefix} - end"), &[type_arg(&options), LogArg::Value(status), LogArg::Value(headers)]);
                    } else {
                        this.log_service.trace_args(&format!("{prefix} - end"), &[type_arg(&options), LogArg::Value(status)]);
                    }
                    this.on_did_complete_request.fire(RequestCompleteEvent {
                        call_site: options.call_site.clone(),
                        latency: runtime::date_now().saturating_sub(start_time) as f64,
                        status_code: result.res.status_code,
                    });
                    Ok(result)
                }
                Err(error) => {
                    if errors::is_cancellation_error(&error) {
                        this.log_service.trace_args(&format!("{prefix} - cancelled"), &[type_arg(&options)]);
                    } else {
                        let detail = if log_url { to_error_message_of(&error, false) } else { loggable_error_code(&error) };
                        this.log_service.error_args(&LogMessage::from(format!("{prefix} - error").as_str()), &[type_arg(&options), LogArg::Value(Dynamic::string(&detail))]);
                    }
                    Err(error)
                }
            }
        })
    }
}

impl IDisposable for RequestLogger {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `isSuccess`.
pub fn is_success(context: &IRequestContext) -> bool {
    matches!(context.res.status_code, Some(code) if (200..300).contains(&code) || code == 1223)
}

/// `isClientError`.
pub fn is_client_error(context: &IRequestContext) -> bool {
    matches!(context.res.status_code, Some(code) if (400..500).contains(&code))
}

/// `isServerError`.
pub fn is_server_error(context: &IRequestContext) -> bool {
    matches!(context.res.status_code, Some(code) if (500..600).contains(&code))
}

/// `readHeader`: lê um cabeçalho de um mapa, tolerando valores em lista e a busca sem distinção de caixa
/// (só pela forma original e a minúscula, como o original).
pub fn read_header(headers: Option<&IHeaders>, name: &str) -> Option<String> {
    let headers = headers?;
    let value = headers.get(name).or_else(|| headers.get(&name.to_lowercase()))?;
    match value {
        HeaderValue::One(text) => Some(text.clone()),
        HeaderValue::Many(items) => items.first().cloned(),
    }
}

/// `retryAfterFromHeaders`: o `Retry-After` em segundos, ou `None` se ausente ou não for um número positivo.
/// A forma de data HTTP não é lida.
pub fn retry_after_from_headers(headers: Option<&IHeaders>) -> Option<i64> {
    let value = read_header(headers, "retry-after").filter(|v| !v.is_empty())?;
    // `parseInt(value, 10)`: sinal opcional e dígitos do começo; o resto é ignorado.
    let trimmed = value.trim_start();
    let (negative, digits) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    let parsed: i64 = digits[..end].parse().ok()?;
    let parsed = if negative { -parsed } else { parsed };
    (parsed > 0).then_some(parsed)
}

/// `hasNoContent`.
pub fn has_no_content(context: &IRequestContext) -> bool {
    context.res.status_code == Some(204)
}

/// `asText`: o corpo como texto, ou `None` se a resposta não tem conteúdo.
pub fn as_text(context: &IRequestContext) -> Promise<Option<String>> {
    if has_no_content(context) {
        return Promise::resolved(None);
    }
    stream_to_buffer(&*context.stream).then(|buffer| Some(buffer.to_string()))
}

/// `readBoundedResponse`: lê a resposta sem guardar mais bytes que o máximo do chamador.
pub fn read_bounded_response(context: &IRequestContext, max_bytes: usize, on_limit_exceeded: impl Fn() -> Error + 'static) -> Promise<String> {
    let (promise, resolver) = Promise::new();
    let chunks: Rc<RefCell<Vec<VSBuffer>>> = Rc::new(RefCell::new(Vec::new()));
    let bytes = Rc::new(Cell::new(0usize));
    let stream = context.stream.clone();
    let on_data = {
        let (chunks, bytes, resolver, stream) = (chunks.clone(), bytes.clone(), resolver.clone(), stream.clone());
        Rc::new(move |chunk: &VSBuffer| {
            bytes.set(bytes.get() + chunk.byte_length());
            if bytes.get() > max_bytes {
                resolver.reject(on_limit_exceeded());
                stream.destroy();
            } else {
                chunks.borrow_mut().push(chunk.clone());
            }
        })
    };
    let on_error = {
        let resolver = resolver.clone();
        Rc::new(move |error: &Error| resolver.reject(error.clone()))
    };
    let on_end = Rc::new(move || resolver.resolve(VSBuffer::concat(&chunks.borrow(), None).to_string()));
    listen_stream(&*context.stream, IStreamListener { on_data, on_error, on_end }, None);
    promise
}

fn ensure_success(context: &IRequestContext) -> Result<(), Error> {
    if is_success(context) {
        Ok(())
    } else {
        Err(Error::new(format!("Server returned {}", context.res.status_code.map_or("undefined".to_owned(), |c| c.to_string()))))
    }
}

/// `asTextOrError`: o corpo como texto; uma resposta que não é de sucesso rejeita com `Server returned <status>`.
pub fn as_text_or_error(context: &IRequestContext) -> Promise<Option<String>> {
    match ensure_success(context) {
        Ok(()) => as_text(context),
        Err(error) => Promise::rejected(error),
    }
}

/// `asJson`: o corpo lido como JSON, ou `None` se a resposta não tem conteúdo.
pub fn as_json(context: &IRequestContext) -> Promise<Option<Dynamic>> {
    if let Err(error) = ensure_success(context) {
        return Promise::rejected(error);
    }
    if has_no_content(context) {
        return Promise::resolved(None);
    }
    stream_to_buffer(&*context.stream).then_result(|result| {
        let text = result?.to_string();
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut parse_errors = Vec::new();
        let options = ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false };
        match json::parse(&units, &mut parse_errors, options) {
            Some(value) if parse_errors.is_empty() => Ok(Some(from_json(&value))),
            _ => Err(Error::new(format!("Unexpected token in JSON:\n{text}"))),
        }
    })
}

/// O tipo do `IRequestContext.stream` de leitura, para quem só lê.
pub type ResponseStream = dyn ReadableStreamEvents<VSBuffer>;
