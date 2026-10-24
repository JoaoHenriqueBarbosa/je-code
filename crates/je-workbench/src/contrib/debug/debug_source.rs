//! Porte de `src/vs/workbench/contrib/debug/common/debugSource.ts`: a `Source` de depuração e a URI que a representa.
//!
//! Debug URI format
//!
//! a debug URI represents a Source object and the debug session where the Source comes from.
//!
//! ```txt
//!       debug:arbitrary_path?session=123e4567-e89b-12d3-a456-426655440000&ref=1016
//!       \___/ \____________/ \__________________________________________/ \______/
//!         |          |                             |                          |
//!      scheme   source.path                    session id            source.reference
//! ```
//!
//! Notas de tradução:
//! - `DebugProtocol.Source` é o [`ProtocolSource`](super::debug_protocol::Source). O `raw_` opcional do construtor é
//!   `Option<ProtocolSource>`.
//! - `new Source(raw, sessionId, uriIdentityService, logService)` recebe os serviços por referência
//!   (`&dyn IUriIdentityService` e `&dyn LogService`).
//! - `||` do original (texto vazio cai no próximo valor) é o `filter(|s| !s.is_empty())`.
//! - `normalize`, `isAbsolute` e `fsPath` usam o `path` e a plataforma do processo hospedeiro, como os imports de nível
//!   de módulo do original; `resources.basenameOrAuthority` é o `ext_uri()` do hospedeiro.
//! - `parseInt(pair[1])` devolvendo `NaN` (texto sem dígitos) vira `None` em `source_reference`: o `NaN` do original
//!   e o `undefined` só se distinguem por `===`, que nenhum chamador faz.
//! - `getUriFromSource` captura a exceção das URIs inválidas (`URI.from` lança) com `errors::catch`.
//! - `openInEditor` NÃO foi portado: ele passa `SIDE_GROUP` ou `ACTIVE_GROUP` ao `IEditorService.openEditor`, e o
//!   `open_editor` do porte ainda não aceita o grupo preferido (o trait tem três implementações de teste a ajustar).

use je_base::JsString;
use je_base::errors;
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::uri::{URI, UriComponents};
use je_platform::log::{LogService, Logger};
use je_platform::resources::{ext_uri, host_platform};
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use super::debug::DEBUG_SCHEME;
use super::debug_protocol::{Source as ProtocolSource, SourcePresentationHint};
use super::debug_utils::is_uri_string;

/// `UNKNOWN_SOURCE_LABEL`.
pub fn unknown_source_label() -> String {
    localize("unknownSource", "Unknown Source", &[])
}

/// O que `getEncodedDebugData` devolve: `{ name, path, sessionId?, sourceReference? }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedDebugData {
    pub name: String,
    pub path: String,
    pub session_id: Option<String>,
    pub source_reference: Option<i64>,
}

/// `Source`.
pub struct Source {
    pub uri: URI,
    pub available: bool,
    pub raw: ProtocolSource,
}

impl Source {
    pub fn new(raw: Option<ProtocolSource>, session_id: &str, uri_identity_service: &dyn IUriIdentityService, log_service: &dyn LogService) -> Self {
        let (raw, path, available) = match raw {
            Some(raw) => {
                let path = raw.path.clone().filter(|p| !p.is_empty()).or_else(|| raw.name.clone().filter(|n| !n.is_empty())).unwrap_or_default();
                (raw, path, true)
            }
            None => {
                let label = unknown_source_label();
                let raw = ProtocolSource { name: Some(label.clone()), ..ProtocolSource::default() };
                (raw, format!("{DEBUG_SCHEME}:{label}"), false)
            }
        };

        let uri = get_uri_from_source(&raw, Some(&path), session_id, uri_identity_service, log_service);
        Self { uri, available, raw }
    }

    pub fn name(&self) -> String {
        self.raw.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| ext_uri().basename_or_authority(&self.uri))
    }

    pub fn origin(&self) -> Option<&str> {
        self.raw.origin.as_deref()
    }

    pub fn presentation_hint(&self) -> Option<SourcePresentationHint> {
        self.raw.presentation_hint
    }

    pub fn reference(&self) -> Option<i64> {
        self.raw.source_reference
    }

    pub fn in_memory(&self) -> bool {
        *self.uri.scheme() == DEBUG_SCHEME
    }

    pub fn get_encoded_debug_data(model_uri: &URI) -> EncodedDebugData {
        let mut source_reference: Option<i64> = None;
        let mut session_id: Option<String> = None;

        let path = if *model_uri.scheme() == schemas::FILE {
            PathModule::native(&ProcessEnvironment).normalize(&model_uri.fs_path(host_platform()).to_string_lossy())
        } else if *model_uri.scheme() == DEBUG_SCHEME {
            let query = model_uri.query().to_string_lossy();
            if !query.is_empty() {
                for keyvalue in query.split('&') {
                    let pair: Vec<&str> = keyvalue.split('=').collect();
                    if pair.len() == 2 {
                        match pair[0] {
                            "session" => session_id = Some(pair[1].to_owned()),
                            "ref" => source_reference = parse_int(pair[1]),
                            _ => {}
                        }
                    }
                }
            }
            model_uri.path().to_string_lossy()
        } else {
            model_uri.to_string(false).to_string_lossy()
        };

        EncodedDebugData { name: ext_uri().basename_or_authority(model_uri), path, session_id, source_reference }
    }
}

/// `parseInt(text)` na base 10: espaços iniciais, sinal opcional e os dígitos que seguem (o resto é ignorado); sem
/// dígitos é `NaN`, que aqui é `None`.
fn parse_int(text: &str) -> Option<i64> {
    let text = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let digits: String = digits.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let value = digits.parse::<i64>().unwrap_or(i64::MAX);
    Some(if negative { -value } else { value })
}

/// `path?.replace(/^\/+/g, '/')` (#174054).
fn collapse_leading_slashes(path: &str) -> String {
    if path.starts_with('/') { format!("/{}", path.trim_start_matches('/')) } else { path.to_owned() }
}

/// `_getUriFromSource(path)` de `getUriFromSource`.
fn uri_from_path(raw: &ProtocolSource, path: Option<&str>, session_id: &str, uri_identity_service: &dyn IUriIdentityService) -> URI {
    if let Some(reference) = raw.source_reference.filter(|reference| *reference > 0) {
        return URI::from(
            &UriComponents {
                scheme: Some(JsString::from(DEBUG_SCHEME)),
                path: path.map(|p| JsString::from(collapse_leading_slashes(p))),
                query: Some(JsString::from(format!("session={session_id}&ref={reference}"))),
                ..UriComponents::default()
            },
            false,
        );
    }

    let non_empty_path = path.filter(|p| !p.is_empty());
    if let Some(path) = non_empty_path
        && is_uri_string(path)
    {
        // path looks like a uri
        return uri_identity_service.as_canonical_uri(&URI::parse(path, false));
    }
    // assume a filesystem path
    if let Some(path) = non_empty_path
        && PathModule::native(&ProcessEnvironment).is_absolute(path)
    {
        return uri_identity_service.as_canonical_uri(&URI::file(path, host_platform()));
    }
    // path is relative: since VS Code cannot deal with this by itself
    // create a debug url that will result in a DAP 'source' request when the url is resolved.
    uri_identity_service.as_canonical_uri(&URI::from(
        &UriComponents {
            scheme: Some(JsString::from(DEBUG_SCHEME)),
            path: path.map(JsString::from),
            query: Some(JsString::from(format!("session={session_id}"))),
            ..UriComponents::default()
        },
        false,
    ))
}

/// `getUriFromSource(raw, path, sessionId, uriIdentityService, logService)`.
pub fn get_uri_from_source(
    raw: &ProtocolSource,
    path: Option<&str>,
    session_id: &str,
    uri_identity_service: &dyn IUriIdentityService,
    log_service: &dyn LogService,
) -> URI {
    match errors::catch(|| uri_from_path(raw, path, session_id, uri_identity_service)) {
        Ok(uri) => uri,
        Err(_) => {
            log_service.error(&format!("Invalid path from debug adapter: {}", path.unwrap_or("undefined")));
            uri_from_path(raw, Some("/invalidDebugSource"), session_id, uri_identity_service)
        }
    }
}
