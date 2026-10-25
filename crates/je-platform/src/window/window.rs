//! Porte de `src/vs/platform/window/common/window.ts`, só o que o workbench já usa.
//!
//! Do arquivo original entram `DEFAULT_CUSTOM_TITLEBAR_HEIGHT` (o teste de posição das notificações
//! calcula o deslocamento do topo com ela) e o `IPath` reduzido ao `fileUri` (o que o serviço de confiança no
//! workspace lê dos arquivos de abertura; `workspace`, `label`, `type`, `openOnlyIfExists`, `exists`, `editorOptions`,
//! `selection`, `remoteAuthority`, `editorOverrideId`, `diffMode` e `mergeMode` entram junto com quem os consumir).
//! O `IWindowSettings` é o [`WindowSettings`], só com `restoreFullscreen` e `newWindowDimensions` (o que o
//! `WindowsStateHandler` lê; o resto das chaves entra junto com quem as consumir), e os três tamanhos padrão de janela
//! (`DEFAULT_EMPTY_WINDOW_SIZE`, `DEFAULT_WORKSPACE_WINDOW_SIZE` e `DEFAULT_AUX_WINDOW_SIZE`) também estão aqui.
//! Os demais tipos (`IOpenWindowOptions`...) entram junto com quem os consumir.

use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors;
use je_base::network::AGENTS_AUTHORITY;
use je_base::oauth::UrlSearchParams;
use je_base::platform::is_tahoe_or_newer;
use je_base::uri::URI;

/// `DEFAULT_CUSTOM_TITLEBAR_HEIGHT`: altura da barra de título customizada, em pixels, incluindo o espaço da
/// central de comandos.
pub const DEFAULT_CUSTOM_TITLEBAR_HEIGHT: u32 = 35;

/// Largura e altura em pixels: o tipo dos três tamanhos padrão de janela.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowSize {
    pub width: f64,
    pub height: f64,
}

/// `DEFAULT_EMPTY_WINDOW_SIZE`.
pub const DEFAULT_EMPTY_WINDOW_SIZE: WindowSize = WindowSize { width: 1200.0, height: 800.0 };

/// `DEFAULT_WORKSPACE_WINDOW_SIZE`.
pub const DEFAULT_WORKSPACE_WINDOW_SIZE: WindowSize = WindowSize { width: 1440.0, height: 900.0 };

/// `DEFAULT_AUX_WINDOW_SIZE`.
pub const DEFAULT_AUX_WINDOW_SIZE: WindowSize = WindowSize { width: 1024.0, height: 768.0 };

/// `IWindowSettings['newWindowDimensions']`. Um valor fora dos cinco conhecidos se comporta como `default` em todo
/// consumidor (nenhum teste do original o distingue), então vira [`NewWindowDimensions::Default`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewWindowDimensions {
    Default,
    Inherit,
    Offset,
    Maximized,
    Fullscreen,
}

/// `IWindowSettings`, só com as chaves que o `WindowsStateHandler` lê.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowSettings {
    pub restore_fullscreen: bool,
    /// `None` quando a chave não está definida (ou está vazia).
    pub new_window_dimensions: Option<NewWindowDimensions>,
}

impl WindowSettings {
    /// O valor de `configurationService.getValue<IWindowSettings | undefined>('window')`; `None` quando a seção não é um objeto.
    pub fn from_dynamic(value: &Dynamic) -> Option<Self> {
        if !matches!(value, Dynamic::Object(_)) {
            return None;
        }
        let new_window_dimensions = match value.get_str("newWindowDimensions") {
            Dynamic::String(text) => match text.to_string_lossy().as_str() {
                "" => None,
                "inherit" => Some(NewWindowDimensions::Inherit),
                "offset" => Some(NewWindowDimensions::Offset),
                "maximized" => Some(NewWindowDimensions::Maximized),
                "fullscreen" => Some(NewWindowDimensions::Fullscreen),
                _ => Some(NewWindowDimensions::Default),
            },
            _ => None,
        };
        Some(Self { restore_fullscreen: value.get_str("restoreFullscreen").is_truthy(), new_window_dimensions })
    }
}

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
