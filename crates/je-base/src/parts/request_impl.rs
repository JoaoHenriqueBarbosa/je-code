//! Porte de `src/vs/base/parts/request/common/requestImpl.ts`: uma requisição HTTP sobre um `fetch`.
//!
//! O `fetch` do navegador é injetado, como no original (o parâmetro `fetcher`): [`Fetcher`] recebe a URL, as
//! opções, e um [`CancellationToken`] no lugar do `AbortSignal` (cancelado, o `fetch` rejeita com um erro de
//! nome `AbortError`). O timeout (`AbortSignal.timeout`) cancela o mesmo token e faz [`request`] devolver
//! `Fetch timeout: <n>ms`. O corpo é lido por [`FetchResponse::array_buffer`], que roda depois dos cabeçalhos
//! (a fase `body`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::async_::async_fn;
use crate::buffer::{VSBuffer, buffer_to_stream};
use crate::cancellation::{CancellationToken, CancellationTokenSource};
use crate::errors::Error;
use crate::lifecycle::IDisposable;
use crate::parts::request::{HeaderValue, IHeaders, IRequestContext, IRequestOptions, IRequestResponse, IRequestTimings, offline_error};
use crate::performance::{clear_marks, mark};
use crate::promise::Promise;
use crate::runtime;
use crate::stopwatch::StopWatch;

/// `RequestInit`, o que [`request`] usa do `fetch`.
#[derive(Clone, Debug, Default)]
pub struct FetchInit {
    pub method: String,
    /// `None` é o `headers: undefined` do original; um cabeçalho pode se repetir (`append`).
    pub headers: Option<Vec<(String, String)>>,
    pub body: Option<String>,
    /// `cache: 'no-store'`.
    pub no_store: bool,
    /// `redirect: 'manual'`.
    pub manual_redirect: bool,
}

/// `Response`, o que [`request`] usa do `fetch`.
#[derive(Clone)]
pub struct FetchResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// `res.arrayBuffer()`.
    pub array_buffer: Rc<dyn Fn() -> Promise<Vec<u8>>>,
}

/// `typeof fetch`.
pub trait Fetcher {
    fn fetch(&self, url: &str, init: &FetchInit, signal: &CancellationToken) -> Promise<FetchResponse>;
}

/// `request`.
pub fn request(options: IRequestOptions, token: CancellationToken, is_online: Option<Rc<dyn Fn() -> bool>>, fetcher: Rc<dyn Fetcher>) -> Promise<IRequestContext> {
    if token.is_cancellation_requested() {
        return Promise::rejected(Error::cancellation());
    }

    async_fn(async move {
        let cancellation = Rc::new(CancellationTokenSource::new(None));
        let cancel = cancellation.clone();
        let disposable = token.on_cancellation_requested().listen(move |_| cancel.cancel());
        let timed_out = Rc::new(Cell::new(false));
        let timer = options.timeout.filter(|&t| t > 0).map(|timeout| {
            let (cancel, timed_out) = (cancellation.clone(), timed_out.clone());
            runtime::set_timeout(
                move || {
                    timed_out.set(true);
                    cancel.cancel();
                },
                timeout,
            )
        });
        let signal = cancellation.token();
        let watch = options.diagnostic_id.as_ref().map(|_| StopWatch::create(Some(false)));
        let marks: Rc<RefCell<Vec<String>>> = Rc::default();
        let trace = {
            let (marks, diagnostic_id) = (marks.clone(), options.diagnostic_id.clone());
            move |event: &str| {
                if let Some(id) = &diagnostic_id {
                    let name = format!("code/request/{id}/{event}");
                    marks.borrow_mut().push(name.clone());
                    mark(&name, None);
                }
            }
        };

        let mut phase = "headers";
        let outcome = async {
            let init = FetchInit {
                method: options.r#type.clone().filter(|t| !t.is_empty()).unwrap_or_else(|| "GET".to_owned()),
                headers: get_request_headers(&options),
                body: options.data.clone(),
                no_store: options.disable_cache == Some(true),
                manual_redirect: options.follow_redirects == Some(0),
            };
            trace("start");
            let res = fetcher.fetch(options.url.as_deref().unwrap_or(""), &init, &signal).await?;
            let response_headers_ms = watch.as_ref().map(StopWatch::elapsed);
            trace("headersReceived");
            phase = "body";
            let body = (res.array_buffer)().await?;
            let response_body_ms = watch.as_ref().zip(response_headers_ms).map(|(w, headers)| w.elapsed() - headers);
            trace("bodyReceived");
            let timings = response_headers_ms.zip(response_body_ms).map(|(response_headers_ms, response_body_ms)| IRequestTimings { response_headers_ms, response_body_ms, decoded_body_bytes: body.len() });
            Ok::<_, Error>(IRequestContext {
                res: IRequestResponse { status_code: Some(res.status), headers: get_response_headers(&res.headers) },
                stream: buffer_to_stream(VSBuffer::wrap(body)),
                timings,
            })
        }
        .await;

        let result = match outcome {
            Ok(context) => Ok(context),
            Err(err) => {
                let aborted = err.name == "AbortError";
                trace(&format!("{phase}/{}", if token.is_cancellation_requested() || aborted { "cancelled" } else { "failed" }));
                if is_online.as_ref().is_some_and(|f| !f()) {
                    Err(offline_error())
                } else if aborted && timed_out.get() {
                    Err(Error::new(format!("Fetch timeout: {}ms", options.timeout.unwrap_or(0))))
                } else if aborted {
                    Err(Error::cancellation())
                } else {
                    Err(err)
                }
            }
        };

        disposable.dispose();
        if let Some(timer) = timer {
            runtime::clear_timeout(timer);
        }
        cancellation.dispose();
        for name in marks.borrow().iter() {
            clear_marks(Some(name));
        }
        result
    })
}

fn get_request_headers(options: &IRequestOptions) -> Option<Vec<(String, String)>> {
    let has_user = options.user.as_deref().is_some_and(|u| !u.is_empty()) || options.password.as_deref().is_some_and(|p| !p.is_empty());
    if options.headers.is_some() || has_user || options.proxy_authorization.as_deref().is_some_and(|p| !p.is_empty()) {
        let mut headers: Vec<(String, String)> = Vec::new();
        let set = |headers: &mut Vec<(String, String)>, name: &str, value: &str| {
            headers.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
            headers.push((name.to_owned(), value.to_owned()));
        };
        for (k, header) in options.headers.iter().flatten() {
            match k.to_lowercase().as_str() {
                // unsafe headers
                "user-agent" | "accept-encoding" | "content-length" => continue,
                _ => {}
            }
            match header {
                HeaderValue::One(value) => set(&mut headers, k, value),
                HeaderValue::Many(values) => headers.extend(values.iter().map(|v| (k.clone(), v.clone()))),
            }
        }
        if has_user {
            let credentials = format!("{}:{}", options.user.as_deref().unwrap_or(""), options.password.as_deref().unwrap_or(""));
            set(&mut headers, "Authorization", &format!("Basic {}", crate::buffer::encode_base64(&VSBuffer::from_string(&credentials), true, false)));
        }
        if let Some(proxy) = options.proxy_authorization.as_deref().filter(|p| !p.is_empty()) {
            set(&mut headers, "Proxy-Authorization", proxy);
        }
        return Some(headers);
    }
    None
}

fn get_response_headers(res_headers: &[(String, String)]) -> IHeaders {
    let mut headers = IHeaders::new();
    for (key, value) in res_headers {
        match headers.get_mut(key) {
            Some(HeaderValue::Many(values)) => values.push(value.clone()),
            Some(existing) => {
                let first = match existing {
                    HeaderValue::One(first) => first.clone(),
                    HeaderValue::Many(_) => unreachable!("tratado acima"),
                };
                *existing = HeaderValue::Many(vec![first, value.clone()]);
            }
            None => {
                headers.insert(key.clone(), HeaderValue::One(value.clone()));
            }
        }
    }
    headers
}
