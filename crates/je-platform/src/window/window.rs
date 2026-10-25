//! Porte de `src/vs/platform/window/common/window.ts`, só o que o workbench já usa.
//!
//! Do arquivo original entram `DEFAULT_CUSTOM_TITLEBAR_HEIGHT` (o teste de posição das notificações
//! calcula o deslocamento do topo com ela) e o `IPath` reduzido ao `fileUri` (o que o serviço de confiança no
//! workspace lê dos arquivos de abertura; `workspace`, `label`, `type`, `openOnlyIfExists`, `exists`, `editorOptions`,
//! `selection`, `remoteAuthority`, `editorOverrideId`, `diffMode` e `mergeMode` entram junto com quem os consumir).
//! Os demais tipos (`IWindowSettings`, `IOpenWindowOptions`...) entram junto com quem os consumir.

use je_base::errors;
use je_base::network::AGENTS_AUTHORITY;
use je_base::oauth::UrlSearchParams;
use je_base::platform::is_tahoe_or_newer;
use je_base::uri::URI;

/// `DEFAULT_CUSTOM_TITLEBAR_HEIGHT`: altura da barra de título customizada, em pixels, incluindo o espaço da
/// central de comandos.
pub const DEFAULT_CUSTOM_TITLEBAR_HEIGHT: u32 = 35;

/// `IPoint`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IPoint {
    pub x: f64,
    pub y: f64,
}

/// `getMacOSWindowControlsPosition`: centraliza os semáforos do macOS na vertical, mantendo opcionalmente o
/// recuo horizontal independente da altura. `None` é o `null` do original.
pub fn get_macos_window_controls_position(height: f64, os_version: &str, horizontal_inset: Option<f64>) -> Option<IPoint> {
    let button_height = if is_tahoe_or_newer(os_version) { 14.0 } else { 16.0 };
    let offset = ((height - button_height) / 2.0).floor();
    if offset == 0.0 && horizontal_inset.is_none() {
        None
    } else {
        Some(IPoint { x: horizontal_inset.unwrap_or(offset + 1.0), y: offset })
    }
}

/// `IAgentsWindowDraft`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentsWindowDraft {
    pub input_text: String,
    /// URI-aware serialized chat attachments, including exported image data.
    pub attachments: String,
}

/// `IAgentsWindowNewSessionLink`.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentsWindowNewSessionLink {
    pub workspace_uri: Option<URI>,
    pub draft: AgentsWindowDraft,
}

/// O argumento `uri: URI | string` de `parseExternalAgentsWindowNewSessionLinkUri`.
pub enum ExternalLinkSource {
    Uri(URI),
    Text(String),
}

impl From<URI> for ExternalLinkSource {
    fn from(uri: URI) -> Self {
        Self::Uri(uri)
    }
}

impl From<&URI> for ExternalLinkSource {
    fn from(uri: &URI) -> Self {
        Self::Uri(uri.clone())
    }
}

impl From<&str> for ExternalLinkSource {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

/// `parseExternalAgentsWindowNewSessionLinkUri`.
pub fn parse_external_agents_window_new_session_link_uri(uri: impl Into<ExternalLinkSource>, product_url_protocol: &str) -> Option<AgentsWindowNewSessionLink> {
    let parsed = match uri.into() {
        ExternalLinkSource::Uri(uri) => uri,
        ExternalLinkSource::Text(text) => URI::parse(text.as_str(), false),
    };
    if parsed.scheme() != product_url_protocol || parsed.authority() != AGENTS_AUTHORITY || parsed.path() != "/new" {
        return None;
    }

    let params = UrlSearchParams::parse(&parsed.query().to_string_lossy());
    let workspace = params.get("workspace");
    let prompt = params.get("prompt");
    let prompt = match prompt {
        Some(prompt) if !prompt.is_empty() => prompt,
        _ => return None,
    };
    if workspace == Some("") {
        return None;
    }

    // `URI.parse(workspace, true)` lança com um texto sem esquema.
    let workspace_uri = match workspace {
        None => None,
        Some(workspace) => Some(errors::catch(|| URI::parse(workspace, true)).ok()?),
    };
    Some(AgentsWindowNewSessionLink { workspace_uri, draft: AgentsWindowDraft { input_text: prompt.to_owned(), attachments: "[]".to_owned() } })
}

/// `IPath` (só o `fileUri`): o arquivo a abrir, `None` quando o caminho é de uma pasta ou de um workspace.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IPath {
    pub file_uri: Option<URI>,
}
