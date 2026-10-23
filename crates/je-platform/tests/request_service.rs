//! Porte de `src/vs/platform/request/test/common/requestService.test.ts` (suíte `AbstractRequestService`), caso a caso e na mesma ordem.
//!
//! Pendente: `src/vs/platform/request/test/node/requestService.test.ts` (323 linhas, `RequestService` do Node sobre um
//! servidor HTTP local, proxy e certificados): o je-code ainda não tem cliente HTTP (ver `src/request/mod.rs`).
//!
//! Divergência: o `TestLogService` do original junta `[message, ...args].join(' ')`; aqui o `join` do
//! JavaScript é refeito sobre os argumentos (`undefined` e `null` viram texto vazio).

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::*;
use je_base::buffer::{VSBuffer, buffer_to_stream, stream_to_buffer};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::parts::ipc::IChannel;
use je_base::marshalling::Marshalled;
use je_base::parts::ipc::IServerChannel;
use je_base::parts::request::{HeaderValue, IHeaders, IRequestContext, IRequestOptions, IRequestResponse, IRequestTimings};
use je_base::promise::Promise;
use je_platform::js::to_js_string;
use je_platform::log::{AbstractLogger, LogArg, LogLevel, LogMessage, Logger};
use je_platform::request::request_ipc::{RequestChannel, RequestChannelClient};
use je_platform::request::{AuthInfo, Credentials, IRequestService, NO_FETCH_TELEMETRY, RequestCompleteEvent, RequestLogger};

fn join(message: &str, args: &[LogArg]) -> String {
    let mut parts = vec![message.to_owned()];
    parts.extend(args.iter().map(|arg| match arg.to_dynamic() {
        Dynamic::Undefined | Dynamic::Null => String::new(),
        other => to_js_string(&other).to_string_lossy(),
    }));
    parts.join(" ")
}

#[derive(Default)]
struct TestLogService {
    base: AbstractLogger,
    traces: RefCell<Vec<String>>,
    errors: RefCell<Vec<String>>,
}

impl IDisposable for TestLogService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl Logger for TestLogService {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        self.base.on_did_change_log_level()
    }
    fn get_level(&self) -> LogLevel {
        self.base.get_level()
    }
    fn set_level(&self, level: LogLevel) {
        self.base.set_level(level);
    }
    fn trace_args(&self, message: &str, args: &[LogArg]) {
        self.traces.borrow_mut().push(join(message, args));
    }
    fn debug_args(&self, _message: &str, _args: &[LogArg]) {}
    fn info_args(&self, _message: &str, _args: &[LogArg]) {}
    fn warn_args(&self, _message: &LogMessage, _args: &[LogArg]) {}
    fn error_args(&self, error: &LogMessage, args: &[LogArg]) {
        let text = match error {
            LogMessage::Text(text) => text.clone(),
            LogMessage::Error(error) => error.message.clone(),
        };
        self.errors.borrow_mut().push(if args.is_empty() { text } else { join(&text, args) });
    }
    fn flush(&self) {}
}

type Handler = Rc<dyn Fn(&IRequestOptions) -> Promise<IRequestContext>>;

struct TestRequestService {
    logger: Rc<RequestLogger>,
    handler: Handler,
}

impl TestRequestService {
    fn new(handler: impl Fn(&IRequestOptions) -> Promise<IRequestContext> + 'static) -> Rc<Self> {
        Self::with_log(handler, Rc::new(TestLogService::default()))
    }

    fn with_log(handler: impl Fn(&IRequestOptions) -> Promise<IRequestContext> + 'static, log: Rc<TestLogService>) -> Rc<Self> {
        Rc::new(Self { logger: RequestLogger::new(log), handler: Rc::new(handler) })
    }
}

impl IRequestService for TestRequestService {
    fn on_did_complete_request(&self) -> Event<RequestCompleteEvent> {
        self.logger.on_did_complete_request()
    }
    fn request(&self, options: IRequestOptions, _token: CancellationToken) -> Promise<IRequestContext> {
        let handler = self.handler.clone();
        let for_handler = options.clone();
        self.logger.log_and_request(&options, move || handler(&for_handler))
    }
    fn resolve_proxy(&self, _url: &str) -> Promise<Option<String>> {
        Promise::resolved(None)
    }
    fn lookup_authorization(&self, _auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        Promise::resolved(None)
    }
    fn lookup_kerberos_authorization(&self, _url: &str) -> Promise<Option<String>> {
        Promise::resolved(None)
    }
    fn load_certificates(&self) -> Promise<Vec<String>> {
        Promise::resolved(Vec::new())
    }
}

impl IDisposable for TestRequestService {
    fn dispose(&self) {
        self.logger.dispose();
    }
}

fn make_response(status_code: u16) -> IRequestContext {
    IRequestContext {
        res: IRequestResponse { headers: IHeaders::new(), status_code: Some(status_code) },
        stream: buffer_to_stream(VSBuffer::from_string("")),
        timings: None,
    }
}

fn options(url: &str, call_site: &str) -> IRequestOptions {
    IRequestOptions { url: Some(url.to_owned()), call_site: call_site.to_owned(), ..IRequestOptions::default() }
}

fn collect_events(service: &TestRequestService) -> Rc<RefCell<Vec<RequestCompleteEvent>>> {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    service.on_did_complete_request().listen(move |e| sink.borrow_mut().push(e.clone()));
    events
}

#[test]
fn on_did_complete_request_fires_with_correct_data() {
    run_with_faked_timers(async {
        let service = TestRequestService::new(|_| Promise::resolved(make_response(200)));
        let events = collect_events(&service);

        service.request(options("http://test", "test.callSite"), CancellationToken::None).await.unwrap();

        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].call_site, "test.callSite");
        assert_eq!(events[0].status_code, Some(200));
        assert!(events[0].latency >= 0.0);
        service.dispose();
    });
}

#[test]
fn on_did_complete_request_reports_status_code_from_response() {
    run_with_faked_timers(async {
        let service = TestRequestService::new(|_| Promise::resolved(make_response(404)));
        let events = collect_events(&service);

        service.request(options("http://test", "test.notFound"), CancellationToken::None).await.unwrap();

        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status_code, Some(404));
        service.dispose();
    });
}

#[test]
fn on_did_complete_request_fires_for_no_fetch_telemetry() {
    run_with_faked_timers(async {
        let service = TestRequestService::new(|_| Promise::resolved(make_response(200)));
        let events = collect_events(&service);

        service.request(options("http://test", NO_FETCH_TELEMETRY), CancellationToken::None).await.unwrap();

        let events = events.borrow();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].call_site, NO_FETCH_TELEMETRY);
        service.dispose();
    });
}

#[test]
fn on_did_complete_request_does_not_fire_when_request_throws() {
    run_with_faked_timers(async {
        let service = TestRequestService::new(|_| Promise::rejected(Error::new("network error")));
        let events = collect_events(&service);

        assert!(service.request(options("http://test", "test.error"), CancellationToken::None).await.is_err());

        assert_eq!(events.borrow().len(), 0);
        service.dispose();
    });
}

#[test]
fn logs_cancellation_at_trace_level() {
    run_with_faked_timers(async {
        let log = Rc::new(TestLogService::default());
        let service = TestRequestService::with_log(|_| Promise::rejected(Error::cancellation()), log.clone());

        let Err(error) = service.request(options("http://test", "test.cancelled"), CancellationToken::None).await else { panic!("devia rejeitar") };
        assert!(errors::is_cancellation_error(&error));

        assert_eq!(log.traces.borrow().iter().filter(|m| m.contains(" - cancelled")).count(), 1);
        assert!(log.errors.borrow().is_empty());
        service.dispose();
    });
}

#[test]
fn on_did_complete_request_fires_for_each_request() {
    run_with_faked_timers(async {
        let service = TestRequestService::new(|_| Promise::resolved(make_response(200)));
        let events = collect_events(&service);

        service.request(options("http://test/1", "first"), CancellationToken::None).await.unwrap();
        service.request(options("http://test/2", "second"), CancellationToken::None).await.unwrap();

        assert_eq!(events.borrow().iter().map(|e| e.call_site.clone()).collect::<Vec<_>>(), ["first", "second"]);
        service.dispose();
    });
}

/// O canal de IPC do teste: `channel.call(undefined, command, args, token)` direto.
struct DirectChannel(RequestChannel);

impl IChannel for DirectChannel {
    fn call(&self, command: &str, arg: Marshalled, token: CancellationToken) -> Promise<Marshalled> {
        self.0.call(&Marshalled::Undefined, command, arg, token)
    }
    fn listen(&self, _event: &str, _arg: Marshalled) -> Event<Marshalled> {
        Event::none()
    }
}

#[test]
fn request_ipc_preserves_optional_diagnostics() {
    // `for (const timings of [undefined, { responseHeadersMs: 30, ... }])`: um teste por valor, no original.
    for timings in [None, Some(IRequestTimings { response_headers_ms: 30.0, response_body_ms: 70.0, decoded_body_bytes: 0 })] {
        run_with_faked_timers(async move {
            let diagnostic_id: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
            let seen = diagnostic_id.clone();
            let service = TestRequestService::new(move |options| {
                *seen.borrow_mut() = options.diagnostic_id.clone();
                Promise::resolved(IRequestContext { timings, ..make_response(200) })
            });
            let channel = RequestChannel::new(service.clone());
            let client = RequestChannelClient::new(Rc::new(DirectChannel(channel)));
            let mut request_options = options("https://example.test", "test.ipc");
            request_options.diagnostic_id = Some("local-test".to_owned());
            let result = client.request(request_options, CancellationToken::None).await.unwrap();

            assert_eq!(diagnostic_id.borrow().as_deref(), Some("local-test"));
            assert_eq!(result.timings, timings);
            assert_eq!(stream_to_buffer(&*result.stream).await.unwrap().to_string(), "");
            service.dispose();
        });
    }
}

#[test]
fn log_url_false_logs_only_the_host_the_status_and_the_error_code_never_the_url_headers_or_error_message() {
    run_with_faked_timers(async {
        let log = Rc::new(TestLogService::default());
        let signed = "https://acct.blob.core.windows.net/models/m.gguf?sv=2024&sig=SECRET";
        let failure = Error::new(format!("connect failed for {signed}")).with_prop("code", "ECONNRESET");
        let fail = Rc::new(std::cell::Cell::new(false));
        let should_fail = fail.clone();
        let service = TestRequestService::with_log(
            move |_| {
                if should_fail.get() {
                    Promise::rejected(failure.clone())
                } else {
                    let mut headers = IHeaders::new();
                    headers.insert("location".to_owned(), HeaderValue::One(signed.to_owned()));
                    Promise::resolved(IRequestContext { res: IRequestResponse { status_code: Some(302), headers }, ..make_response(302) })
                }
            },
            log.clone(),
        );

        let mut first = options(signed, "test.noUrl");
        first.r#type = Some("GET".to_owned());
        first.log_url = Some(false);
        let mut headers = IHeaders::new();
        headers.insert("Authorization".to_owned(), HeaderValue::One("Bearer SECRET".to_owned()));
        first.headers = Some(headers);
        service.request(first, CancellationToken::None).await.unwrap();
        fail.set(true);
        let mut second = options(signed, "test.noUrl");
        second.r#type = Some("GET".to_owned());
        second.log_url = Some(false);
        assert!(service.request(second, CancellationToken::None).await.is_err());

        assert_eq!(
            *log.traces.borrow(),
            ["#1: acct.blob.core.windows.net - begin GET", "#1: acct.blob.core.windows.net - end GET 302", "#2: acct.blob.core.windows.net - begin GET"]
        );
        assert_eq!(*log.errors.borrow(), ["#2: acct.blob.core.windows.net - error GET ECONNRESET"]);
        service.dispose();
    });
}
