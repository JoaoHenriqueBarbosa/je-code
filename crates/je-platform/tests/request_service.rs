//! Porte de `src/vs/platform/request/test/node/requestService.test.ts`.
//!
//! Os casos saem um a um e na mesma ordem do original, mais três casos de rede real que o original não tem (no fim,
//! com o prefixo `extra_`).
//!
//! Divergências:
//! - O original não sobe servidor: injeta, por `getRawRequest`, um `http.ClientRequest` falso que dispara o `error`
//!   (`setTimeout(..., 0)`) ou o `callback` da resposta. Aqui o falso é o [`MockClientRequest`], com os mesmos
//!   disparos em `runtime::set_timeout(..., 0)`; os cinco casos que só variam o método, a mensagem e o código do erro
//!   dividem os auxiliares [`succeeds_on_third_attempt`] e [`always_fails`].
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso e o laço de eventos é o
//!   real (`run_with_real_timers`): a espera entre as tentativas (100 ms e 200 ms) é de verdade, como no mocha.
//! - O `assert.fail('Should have thrown an error')` dentro do `try` do original é capturado pelo próprio `catch`
//!   (`AssertionError` é um `Error`), então esses casos nunca reprovavam por falta de erro; aqui a ausência do erro
//!   reprova, e o erro tem o `code` que o falso lançou.
//! - O caso `Kerberos lookup` é `(isWindows ? test : test.skip)`: fora do Windows é `#[ignore]`. O módulo nativo
//!   `kerberos` não existe no porte (ver o topo de `node_request_service.rs`), então no Windows o caso reprova.
//! - Os `extra_*` sobem um servidor HTTP em `127.0.0.1` com `std::net::TcpListener` e exercitam o cliente de produção.

mod common;

use std::cell::Cell;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;
use std::thread::JoinHandle;

use common::{ensure_no_disposables_are_leaked_in_test_suite, null_log, run_with_real_timers};
use je_base::buffer::BytesOrString;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{Error, is_cancellation_error};
use je_base::parts::request::{HeaderValue, IHeaders, IRequestOptions};
use je_base::runtime;
use je_base::stream::new_writeable_stream;
use je_platform::request::as_text;
use je_platform::request::node_request_service::{
    ClientRequest, IncomingMessage, NodeRequestOptions, RawRequestFunction, RawRequestOptions, ResponseCallback, lookup_kerberos_authorization, node_request,
};

/// O `mockReq` dos casos: `on('error', handler)` e `end()` disparam o que o caso quiser; o resto do `ClientRequest` não
/// faz nada (`on` de outros eventos, `abort`, `setTimeout`).
struct MockClientRequest {
    on_error: Box<dyn Fn(Rc<dyn Fn(Error)>)>,
    on_end: Box<dyn Fn()>,
}

impl ClientRequest for MockClientRequest {
    fn on_error(&self, handler: Rc<dyn Fn(Error)>) {
        (self.on_error)(handler);
    }

    fn on_response(&self, _handler: Rc<dyn Fn()>) {}

    fn on_abort(&self, _handler: Rc<dyn Fn()>) {}

    fn set_timeout(&self, _ms: u64) {}

    fn remove_header(&self, _name: &str) {}

    fn write(&self, _data: &str) {}

    fn end(&self) {
        (self.on_end)();
    }

    fn abort(&self) {}
}

/// `getRawRequest: () => mockRawRequest`.
fn get_raw_request(mock_raw_request: impl Fn(ResponseCallback) -> MockClientRequest + 'static) -> Rc<dyn Fn(&IRequestOptions) -> RawRequestFunction> {
    let raw_request: RawRequestFunction = Rc::new(move |_options: &RawRequestOptions, callback: ResponseCallback| -> Rc<dyn ClientRequest> { Rc::new(mock_raw_request(callback)) });
    Rc::new(move |_| raw_request.clone())
}

/// `const err = new Error(message) as NodeJS.ErrnoException; err.code = code;`.
fn errno_error(message: &str, code: &str) -> Error {
    Error::new(message).with_prop("code", code)
}

/// `{ statusCode: 200, headers: {}, on: () => { }, pipe: () => ({ on: () => { } }) }`: uma resposta cujo corpo nunca emite.
fn ok_response() -> IncomingMessage {
    IncomingMessage { status_code: Some(200), headers: IHeaders::new(), stream: new_writeable_stream::<BytesOrString>(None, None) }
}

/// O `mockRawRequest` dos casos que repetem: as duas primeiras tentativas disparam o erro (`setTimeout(() => handler(err), 0)`) e
/// a terceira responde 200 no `end()`.
fn succeeds_on_third_attempt(attempt_count: Rc<Cell<u32>>, message: &'static str, code: &'static str) -> impl Fn(ResponseCallback) -> MockClientRequest {
    move |callback| {
        attempt_count.set(attempt_count.get() + 1);
        let current_attempt = attempt_count.get();
        MockClientRequest {
            on_error: Box::new(move |handler| {
                if current_attempt < 3 {
                    let err = errno_error(message, code);
                    runtime::set_timeout(move || handler(err), 0);
                }
            }),
            on_end: Box::new(move || {
                if current_attempt >= 3 {
                    // Succeed on third attempt by calling the response callback
                    let callback = callback.clone();
                    runtime::set_timeout(move || callback(ok_response()), 0);
                }
            }),
        }
    }
}

/// O `mockRawRequest` dos casos que não repetem: toda tentativa dispara `ECONNREFUSED`.
fn always_fails(attempt_count: Rc<Cell<u32>>) -> impl Fn(ResponseCallback) -> MockClientRequest {
    move |_callback| {
        attempt_count.set(attempt_count.get() + 1);
        MockClientRequest {
            on_error: Box::new(|handler| {
                let err = errno_error("Connection refused", "ECONNREFUSED");
                runtime::set_timeout(move || handler(err), 0);
            }),
            on_end: Box::new(|| {}),
        }
    }
}

fn mock_options(method: &str, call_site: &str, get_raw_request: Rc<dyn Fn(&IRequestOptions) -> RawRequestFunction>) -> NodeRequestOptions {
    NodeRequestOptions {
        base: IRequestOptions { url: Some("http://example.com".to_owned()), r#type: Some(method.to_owned()), call_site: call_site.to_owned(), ..IRequestOptions::default() },
        get_raw_request: Some(get_raw_request),
        ..NodeRequestOptions::default()
    }
}

/// Um caso "should retry <method> requests on transient errors".
fn assert_retries(method: &str, call_site: &str, message: &'static str, code: &'static str) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let method = method.to_owned();
    let call_site = call_site.to_owned();
    run_with_real_timers(async move {
        let attempt_count = Rc::new(Cell::new(0));
        let options = mock_options(&method, &call_site, get_raw_request(succeeds_on_third_attempt(attempt_count.clone(), message, code)));

        // Expected to eventually succeed or fail after retries
        let _ = node_request(options, CancellationToken::None).await;

        assert!(attempt_count.get() > 1, "{method} request should have been retried");
    });
}

/// Um caso "should NOT retry <method> requests".
fn assert_does_not_retry(method: &str, call_site: &str) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let method = method.to_owned();
    let call_site = call_site.to_owned();
    run_with_real_timers(async move {
        let attempt_count = Rc::new(Cell::new(0));
        let options = mock_options(&method, &call_site, get_raw_request(always_fails(attempt_count.clone())));

        match node_request(options, CancellationToken::None).await {
            Ok(_) => panic!("Should have thrown an error"),
            Err(err) => assert_eq!(err.prop("code"), Some("ECONNREFUSED")),
        }

        assert_eq!(attempt_count.get(), 1, "{method} request should not have been retried");
    });
}

// Kerberos module fails to load on local macOS and Linux CI.
#[test]
#[cfg_attr(not(windows), ignore = "o original só roda no Windows (isWindows ? test : test.skip)")]
fn kerberos_lookup() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_real_timers(async move {
        let log_service = null_log();
        match lookup_kerberos_authorization("http://localhost:9999", None, &log_service, "requestService.test.ts").await {
            Ok(response) => assert!(!response.is_empty()),
            Err(err) => {
                let message = err.message;
                assert!(
                    message.contains("No authority could be contacted for authentication")
                        || message.contains("No Kerberos credentials available")
                        || message.contains("No credentials are available in the security package")
                        || message.contains("no credential for"),
                    "Unexpected error: {message}"
                );
            }
        }
    });
}

#[test]
fn request_cancellation_during_retry_backoff() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let cts = guard.add_rc(Rc::new(CancellationTokenSource::new(None)));
    run_with_real_timers(async move {
        let attempt_count = Rc::new(Cell::new(0));
        let mock_raw_request = {
            let (attempt_count, cts) = (attempt_count.clone(), cts.clone());
            move |_callback: ResponseCallback| {
                attempt_count.set(attempt_count.get() + 1);
                let cts = cts.clone();
                MockClientRequest {
                    on_error: Box::new(move |handler| {
                        let err = errno_error("Connection refused", "ECONNREFUSED");
                        let cts = cts.clone();
                        // Fail the first attempt with a transient error, then cancel while the
                        // retry backoff is pending so cancellation is observed during the backoff.
                        runtime::set_timeout(
                            move || {
                                handler(err);
                                cts.cancel();
                            },
                            0,
                        );
                    }),
                    on_end: Box::new(|| {}),
                }
            }
        };

        match node_request(mock_options("GET", "requestService.test.cancellation", get_raw_request(mock_raw_request)), cts.token()).await {
            Ok(_) => panic!("Request should have been cancelled"),
            Err(err) => assert!(is_cancellation_error(&err), "Error should be a CancellationError"),
        }

        assert_eq!(attempt_count.get(), 1, "Request should be cancelled during backoff without further retries");
    });
}

#[test]
fn should_retry_get_requests_on_transient_errors() {
    assert_retries("GET", "requestService.test.retryGET", "Connection refused", "ECONNREFUSED");
}

#[test]
fn should_not_retry_post_requests() {
    assert_does_not_retry("POST", "requestService.test.noRetryPOST");
}

#[test]
fn should_retry_head_requests_on_transient_errors() {
    assert_retries("HEAD", "requestService.test.retryHEAD", "Host unreachable", "EHOSTUNREACH");
}

#[test]
fn should_retry_options_requests_on_transient_errors() {
    assert_retries("OPTIONS", "requestService.test.retryOPTIONS", "Network unreachable", "ENETUNREACH");
}

#[test]
fn should_not_retry_delete_requests() {
    assert_does_not_retry("DELETE", "requestService.test.noRetryDELETE");
}

#[test]
fn should_not_retry_put_requests() {
    assert_does_not_retry("PUT", "requestService.test.noRetryPUT");
}

#[test]
fn should_not_retry_patch_requests() {
    assert_does_not_retry("PATCH", "requestService.test.noRetryPATCH");
}

// --- Casos de rede real (sem equivalente no original)

/// Atende uma conexão por resposta (na ordem de `responses`) e devolve as requisições recebidas, cabeçalho e corpo.
fn serve(listener: TcpListener, responses: Vec<Vec<u8>>) -> JoinHandle<Vec<String>> {
    std::thread::spawn(move || {
        let mut requests = Vec::new();
        for response in responses {
            let (mut stream, _) = listener.accept().expect("aceitar a conexão");
            requests.push(read_request(&mut stream));
            stream.write_all(&response).expect("responder");
        }
        requests
    })
}

/// Lê até o fim do cabeçalho e, se houver `Content-Length`, o corpo.
fn read_request(stream: &mut TcpStream) -> String {
    let mut received = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let count = stream.read(&mut chunk).expect("ler a requisição");
        received.extend_from_slice(&chunk[..count]);
        let text = String::from_utf8_lossy(&received).into_owned();
        if let Some(head_end) = text.find("\r\n\r\n") {
            let length = text[..head_end].lines().find_map(|line| line.to_lowercase().strip_prefix("content-length:").and_then(|value| value.trim().parse::<usize>().ok())).unwrap_or(0);
            if received.len() >= head_end + 4 + length {
                return text;
            }
        }
        if count == 0 {
            return text;
        }
    }
}

fn http_response(status_line: &str, headers: &[String], body: &[u8]) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 {status_line}\r\n");
    for header in headers {
        response.push_str(header);
        response.push_str("\r\n");
    }
    response.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", body.len()));
    let mut bytes = response.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn local_options(url: String, method: &str) -> NodeRequestOptions {
    NodeRequestOptions::from(IRequestOptions { url: Some(url), r#type: Some(method.to_owned()), call_site: "requestService.test.extra".to_owned(), ..IRequestOptions::default() })
}

#[test]
fn extra_follows_redirects_and_decompresses_gzip() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let listener = TcpListener::bind("127.0.0.1:0").expect("abrir a porta");
    let port = listener.local_addr().expect("endereço").port();
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(b"hello gzip").expect("comprimir");
    let server = serve(
        listener,
        vec![
            http_response("302 Found", &[format!("Location: http://127.0.0.1:{port}/final")], b""),
            http_response("200 OK", &["Content-Encoding: gzip".to_owned()], &encoder.finish().expect("fechar o gzip")),
        ],
    );
    run_with_real_timers(async move {
        let context = node_request(local_options(format!("http://127.0.0.1:{port}/start"), "GET"), CancellationToken::None).await.expect("requisição");
        assert_eq!(context.res.status_code, Some(200));
        assert_eq!(as_text(&context).await.expect("corpo"), Some("hello gzip".to_owned()));
    });
    let requests = server.join().expect("servidor");
    assert!(requests[0].starts_with("GET /start HTTP/1.1"), "{}", requests[0]);
    assert!(requests[1].starts_with("GET /final HTTP/1.1"), "{}", requests[1]);
}

#[test]
fn extra_does_not_follow_redirects_when_follow_redirects_is_zero() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let listener = TcpListener::bind("127.0.0.1:0").expect("abrir a porta");
    let port = listener.local_addr().expect("endereço").port();
    let server = serve(listener, vec![http_response("302 Found", &["Location: http://127.0.0.1:1/never".to_owned()], b"")]);
    run_with_real_timers(async move {
        let mut options = local_options(format!("http://127.0.0.1:{port}/start"), "GET");
        options.base.follow_redirects = Some(0);
        let context = node_request(options, CancellationToken::None).await.expect("requisição");
        assert_eq!(context.res.status_code, Some(302));
        assert_eq!(context.res.headers.get("location"), Some(&HeaderValue::One("http://127.0.0.1:1/never".to_owned())));
    });
    assert_eq!(server.join().expect("servidor").len(), 1);
}

#[test]
fn extra_post_sends_the_body_and_basic_credentials() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let listener = TcpListener::bind("127.0.0.1:0").expect("abrir a porta");
    let port = listener.local_addr().expect("endereço").port();
    let server = serve(listener, vec![http_response("201 Created", &[], b"done")]);
    run_with_real_timers(async move {
        let mut options = local_options(format!("http://127.0.0.1:{port}/submit"), "POST");
        options.base.data = Some("payload".to_owned());
        options.base.user = Some("user".to_owned());
        options.base.password = Some("pass".to_owned());
        let context = node_request(options, CancellationToken::None).await.expect("requisição");
        assert_eq!(context.res.status_code, Some(201));
        assert_eq!(as_text(&context).await.expect("corpo"), Some("done".to_owned()));
    });
    let request = server.join().expect("servidor").remove(0);
    assert!(request.starts_with("POST /submit HTTP/1.1"), "{request}");
    assert!(request.to_lowercase().contains("authorization: basic dxnlcjpwyxnz"), "{request}");
    assert!(request.ends_with("payload"), "{request}");
}
