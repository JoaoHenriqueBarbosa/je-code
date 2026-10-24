//! Porte parcial de `src/vs/platform/agentHost/common/agentHostUri.ts`: o esquema `vscode-agent-host` e o embrulho de
//! um URI remoto nele ([`to_agent_host_uri`] e [`to_agent_host_content_uri`]), que o serviço de confiança no workspace
//! usa para não tratar pastas do Agent Host como recursos virtuais confiáveis.
//!
//! Fora do porte (entram junto de quem os usar): `IAgentHostResourceUriMapper`, a leitura do metadado
//! (`fromAgentHostUri`), `agentHostAuthority`, `normalizeRemoteAgentHostAddress` e o `AGENT_HOST_LABEL_FORMATTER`.
//!
//! Divergências:
//! - `new URLSearchParams()` com um único `set` de um valor base64 url-safe (só `A-Za-z0-9-_`, que a serialização
//!   `application/x-www-form-urlencoded` não altera) é `_ah=<valor>`.
//! - O metadado é um objeto JSON montado na ordem do original (`scheme`, `authority`, `path`, `query`, `contentRef`,
//!   `fileUri`) e serializado por [`json_stringify`].

use je_base::JsString;
use je_base::buffer::{VSBuffer, encode_base64};
use je_base::json::{Object, Value};
use je_base::json_formatter::json_stringify;
use je_base::network::schemas;
use je_base::uri::{URI, UriComponents};

/// `AGENT_HOST_SCHEME`: o esquema de um URI do Agent Host embrulhado.
pub const AGENT_HOST_SCHEME: &str = "vscode-agent-host";

/// `LOCAL_AGENT_HOST_AUTHORITY`: o Agent Host em processo, que roda sempre na mesma máquina do cliente.
pub const LOCAL_AGENT_HOST_AUTHORITY: &str = "local";

/// `AGENT_HOST_META_PARAM`: o parâmetro da query que carrega o metadado.
const AGENT_HOST_META_PARAM: &str = "_ah";

/// `toAgentHostUri(originalUri, connectionAuthority)`.
pub fn to_agent_host_uri(original_uri: &URI, connection_authority: &str) -> URI {
    wrap_agent_host_uri(original_uri, connection_authority, false, None)
}

/// `toAgentHostContentUri(originalUri, connectionAuthority, fileUri?)`.
pub fn to_agent_host_content_uri(original_uri: &URI, connection_authority: &str, file_uri: Option<&URI>) -> URI {
    wrap_agent_host_uri(original_uri, connection_authority, true, file_uri)
}

fn wrap_agent_host_uri(original_uri: &URI, connection_authority: &str, content_ref: bool, file_uri: Option<&URI>) -> URI {
    if connection_authority == LOCAL_AGENT_HOST_AUTHORITY && (original_uri.scheme() == schemas::FILE || original_uri.scheme() == schemas::VSCODE_REMOTE) {
        return original_uri.clone();
    }

    let display_path = file_uri.map_or_else(|| original_uri.path().clone(), |uri| uri.path().clone());
    let path = if !display_path.is_empty() && display_path.units()[0] != u16::from(b'/') { JsString::from(format!("/{display_path}")) } else { display_path };

    let mut meta = Object::new();
    meta.insert(JsString::from("scheme"), Value::String(original_uri.scheme().clone()));
    if !original_uri.authority().is_empty() {
        meta.insert(JsString::from("authority"), Value::String(original_uri.authority().clone()));
    }
    if path != *original_uri.path() {
        meta.insert(JsString::from("path"), Value::String(original_uri.path().clone()));
    }
    if !original_uri.query().is_empty() {
        meta.insert(JsString::from("query"), Value::String(original_uri.query().clone()));
    }
    if content_ref {
        meta.insert(JsString::from("contentRef"), Value::Bool(true));
    }
    if let Some(file_uri) = file_uri {
        meta.insert(JsString::from("fileUri"), Value::String(file_uri.to_string(false)));
    }

    let encoded = encode_base64(&VSBuffer::from_string(&json_stringify(&Value::Object(meta), None)), false, true);
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(AGENT_HOST_SCHEME)),
            authority: Some(JsString::from(connection_authority)),
            path: Some(if path.is_empty() { JsString::from("/") } else { path }),
            query: Some(JsString::from(format!("{AGENT_HOST_META_PARAM}={encoded}"))),
            fragment: Some(original_uri.fragment().clone()),
        },
        false,
    )
}
