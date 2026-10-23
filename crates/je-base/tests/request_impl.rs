//! Porte de `src/vs/base/parts/request/test/common/requestImpl.test.ts` (suíte `Fetch request diagnostics`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `fetcher` é um `Fetcher` de teste; o `Response` com `arrayBuffer` sobrescrito é um `FetchResponse` cujo
//!   `array_buffer` roda o código do teste. O `AbortError` é um `Error` de nome `AbortError`.
//! - Os `for (const x of [...]) test(...)` do original geram um teste por valor; aqui cada laço é um teste
//!   que percorre os mesmos valores (`measures_headers_and_body_independently_only_when_opted_in`,
//!   `preserves_cancellation_and_clears_diagnostic_marks_while_awaiting_phase`).
//! - O `Headers` do `fetch` não existe: o cabeçalho enviado (`sentHeader`) é a ausência de cabeçalhos
//!   (`init.headers == None`), que é o que o original confere (`null`).

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::buffer::{VSBuffer, stream_to_buffer};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{Error, is_cancellation_error};
use je_base::parts::request::{IRequestOptions, IRequestTimings};
use je_base::parts::request_impl::{FetchInit, FetchResponse, Fetcher, request};
use je_base::performance::get_marks;
use je_base::promise::Promise;

fn marks(id: &str) -> Vec<String> {
    get_marks().into_iter().filter(|m| m.name.starts_with(&format!("code/request/{id}/"))).map(|m| m.name).collect()
}

type FetchFn = dyn Fn(&str, &FetchInit, &CancellationToken) -> Promise<FetchResponse>;

struct FnFetcher(Box<FetchFn>);

impl Fetcher for FnFetcher {
    fn fetch(&self, url: &str, init: &FetchInit, signal: &CancellationToken) -> Promise<FetchResponse> {
        (self.0)(url, init, signal)
    }
}

fn fetcher(f: impl Fn(&str, &FetchInit, &CancellationToken) -> Promise<FetchResponse> + 'static) -> Rc<dyn Fetcher> {
    Rc::new(FnFetcher(Box::new(f)))
}

fn abort_error() -> Error {
    Error::named("AbortError", "Aborted")
}

fn response(status: u16, array_buffer: impl Fn() -> Promise<Vec<u8>> + 'static) -> FetchResponse {
    FetchResponse { status, headers: Vec::new(), array_buffer: Rc::new(array_buffer) }
}

#[test]
fn measures_headers_and_body_independently_only_when_opted_in() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    for diagnostic_id in [None, Some("test-history")] {
        run_with_faked_timers(async move {
            let header_marks: Rc<RefCell<Vec<String>>> = Rc::default();
            type SentHeaders = Option<Vec<(String, String)>>;
            let sent_header: Rc<RefCell<Option<SentHeaders>>> = Rc::default();
            let bytes = VSBuffer::from_string("response \u{e9}");
            let (marks_seen, header_seen, body_bytes) = (header_marks.clone(), sent_header.clone(), bytes.to_vec());
            let response_context = request(
                IRequestOptions { url: Some("https://example.test/private-path".to_owned()), call_site: "request.test.diagnostics".to_owned(), diagnostic_id: diagnostic_id.map(str::to_owned), ..Default::default() },
                CancellationToken::None,
                None,
                fetcher(move |_url, init, _signal| {
                    *header_seen.borrow_mut() = Some(init.headers.clone());
                    let (marks_seen, body_bytes) = (marks_seen.clone(), body_bytes.clone());
                    timeout(30).then_promise(move |()| {
                        Promise::resolved(response(200, move || {
                            *marks_seen.borrow_mut() = marks("test-history");
                            let body_bytes = body_bytes.clone();
                            timeout(70).then(move |()| body_bytes.clone())
                        }))
                    })
                }),
            )
            .await
            .unwrap();
            let body = stream_to_buffer(&*response_context.stream).await.unwrap();

            let expected_timings = diagnostic_id.map(|_| IRequestTimings { response_headers_ms: 30.0, response_body_ms: 70.0, decoded_body_bytes: bytes.byte_length() });
            assert_eq!(response_context.timings, expected_timings);
            assert_eq!(String::from_utf8(body.to_vec()).unwrap(), "response \u{e9}");
            let expected_marks: Vec<String> = if diagnostic_id.is_some() { vec!["code/request/test-history/start".to_owned(), "code/request/test-history/headersReceived".to_owned()] } else { vec![] };
            assert_eq!(*header_marks.borrow(), expected_marks);
            assert_eq!(*sent_header.borrow(), Some(None));
            assert_eq!(marks("test-history"), Vec::<String>::new());
        });
    }
}

#[test]
fn preserves_cancellation_and_clears_diagnostic_marks_while_awaiting_phase() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    for phase in ["headers", "body"] {
        let source = Rc::new(store.add_rc(Rc::new(CancellationTokenSource::new(None))));
        let reached_body = Rc::new(Cell::new(false));
        let (abort_source, body_flag) = (source.clone(), reached_body.clone());
        let source_for_fetch = source.clone();
        let token = source.token();
        run_with_faked_timers(async move {
            let abort = move || {
                let source = abort_source.clone();
                timeout(25).then_promise(move |()| {
                    source.cancel();
                    Promise::<FetchResponse>::rejected(abort_error())
                })
            };
            let abort_body = {
                let source = source_for_fetch.clone();
                move || {
                    let source = source.clone();
                    timeout(25).then_promise(move |()| {
                        source.cancel();
                        Promise::<Vec<u8>>::rejected(abort_error())
                    })
                }
            };
            let result = request(
                IRequestOptions { url: Some("https://example.test/private-path".to_owned()), call_site: "request.test.cancelDiagnostics".to_owned(), diagnostic_id: Some("cancel-test".to_owned()), ..Default::default() },
                token,
                None,
                fetcher(move |_url, _init, _signal| {
                    if phase == "headers" {
                        return abort();
                    }
                    let (body_flag, abort_body) = (body_flag.clone(), abort_body.clone());
                    Promise::resolved(response(200, move || {
                        body_flag.set(true);
                        abort_body()
                    }))
                }),
            );
            let error = match result.await {
                Err(error) => error,
                Ok(_) => panic!("devia rejeitar"),
            };
            assert!(is_cancellation_error(&error), "{error}");
        });
        assert_eq!((reached_body.get(), marks("cancel-test")), (phase == "body", Vec::<String>::new()));
    }
}

#[test]
fn preserves_http_errors_and_response_bytes_without_treating_them_as_transport_failures() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let context = request(
            IRequestOptions { url: Some("https://example.test/private-path".to_owned()), call_site: "request.test.httpDiagnostics".to_owned(), diagnostic_id: Some("http-test".to_owned()), ..Default::default() },
            CancellationToken::None,
            None,
            fetcher(|_url, _init, _signal| Promise::resolved(response(503, || Promise::resolved(b"private-body".to_vec())))),
        )
        .await
        .unwrap();

        let body = stream_to_buffer(&*context.stream).await.unwrap();
        assert_eq!(context.res.status_code, Some(503));
        assert_eq!(context.timings.map(|t| t.decoded_body_bytes), Some(12));
        assert_eq!(String::from_utf8(body.to_vec()).unwrap(), "private-body");
        assert_eq!(marks("http-test"), Vec::<String>::new());
    });
}
