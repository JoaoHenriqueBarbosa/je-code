//! Porte de `src/vs/base/parts/request/common/request.ts`: as opções, os cabeçalhos e o contexto de uma
//! requisição HTTP, e o erro de estar offline.

use std::rc::Rc;

use indexmap::IndexMap;

use crate::buffer::VSBuffer;
use crate::errors::Error;
use crate::stream::WriteableStreamImpl;

const OFFLINE_NAME: &str = "Offline";

/// `isOfflineError`: checks if the given error is offline error.
pub fn is_offline_error(error: &Error) -> bool {
    error.name == OFFLINE_NAME && error.message == OFFLINE_NAME
}

/// `new OfflineError()`.
pub fn offline_error() -> Error {
    Error::named(OFFLINE_NAME, OFFLINE_NAME)
}

/// O valor de um cabeçalho: `string | string[]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HeaderValue {
    One(String),
    Many(Vec<String>),
}

/// `IHeaders`: os cabeçalhos, na ordem de inserção. As chaves que o original declara (`Proxy-Authorization`,
/// `x-operation-id`, `retry-after`, `etag`, `Content-Length`, `activityid`, `X-Market-User-Id`) são chaves
/// comuns daqui.
pub type IHeaders = IndexMap<String, HeaderValue>;

/// `IRequestOptions`.
#[derive(Clone, Debug, Default)]
pub struct IRequestOptions {
    pub r#type: Option<String>,
    pub url: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub headers: Option<IHeaders>,
    pub timeout: Option<u64>,
    pub data: Option<String>,
    pub follow_redirects: Option<u32>,
    pub proxy_authorization: Option<String>,
    /// A signal to not cache the response. This may not be supported in all implementations.
    pub disable_cache: Option<bool>,
    /// `false` logs only this request's host, status and error name or code, never its URL, headers or error
    /// message (for signed URLs).
    pub log_url: Option<bool>,
    /// Identifies the call site making this request, used for telemetry. Use "NO_FETCH_TELEMETRY" to opt out of
    /// request telemetry.
    pub call_site: String,
    /// Opts into local timing diagnostics and User Timing marks; never sent to the server.
    pub diagnostic_id: Option<String>,
}

/// `IRequestTimings`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IRequestTimings {
    /// Client wait until response headers arrive, including connection setup and redirects.
    pub response_headers_ms: f64,
    /// Time spent consuming the response after receiving headers.
    pub response_body_ms: f64,
    /// Decompressed response size, not compressed bytes transferred on the wire.
    pub decoded_body_bytes: usize,
}

/// `IRequestContext.res`.
#[derive(Clone, Debug)]
pub struct IRequestResponse {
    pub headers: IHeaders,
    pub status_code: Option<u16>,
}

/// `IRequestContext`.
#[derive(Clone)]
pub struct IRequestContext {
    pub res: IRequestResponse,
    pub stream: Rc<WriteableStreamImpl<VSBuffer>>,
    /// Available for diagnostic requests using the fetch-backed implementation.
    pub timings: Option<IRequestTimings>,
}
