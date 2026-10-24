//! Porte de `src/vs/workbench/contrib/issue/common/issue.ts`: os tipos de dados do relator de problemas
//! (tipo e origem do problema, dados das extensões, dados que o serviço entrega ao relator).
//!
//! Divergências:
//! - **Só os dados são portados.** `IIssueFormService`, `IWorkbenchIssueService` e `IIssueSubmissionHost`
//!   descrevem serviços que abrem janela, mostram diálogo e falam com o GitHub (`openReporter`,
//!   `showConfirmCloseDialog`, `submitIssue`, `searchGitHubIssues` com `AbortSignal`...), e dependem das camadas
//!   de janela e de diálogo do workbench, ainda não portadas. Entram junto com o `IssueFormService`. Os tipos que a
//!   interface usa ([`IssueReporterData`], [`SimilarIssue`]) estão aqui.
//! - `IssueType` é `const enum` no original (valores numéricos 0, 1 e 2): [`IssueType`] guarda os mesmos
//!   discriminantes. `IssueSource` é um `enum` de texto: [`IssueSource::as_str`] devolve o valor.
//! - `UriComponents` é [`UriComponents`] de `je_base::uri`; `Promise<void>` é [`Promise<()>`].
//! - Campos opcionais do TypeScript (`x?: T`) são `Option<T>`.

use je_base::promise::Promise;
use je_base::uri::UriComponents;

/// `WindowStyles`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowStyles {
    pub background_color: Option<String>,
    pub color: Option<String>,
}

/// `WindowData`.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowData {
    pub styles: WindowStyles,
    pub zoom_level: f64,
}

/// `IssueType` (`const enum`: `Bug` 0, `PerformanceIssue` 1, `FeatureRequest` 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueType {
    Bug = 0,
    PerformanceIssue = 1,
    FeatureRequest = 2,
}

/// `IssueSource`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSource {
    VSCode,
    AgentsWindow,
    Extension,
    Marketplace,
    Unknown,
}

impl IssueSource {
    /// O valor de texto do original (`'vscode'`, `'agentsWindow'`, `'extension'`, `'marketplace'`, `'unknown'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VSCode => "vscode",
            Self::AgentsWindow => "agentsWindow",
            Self::Extension => "extension",
            Self::Marketplace => "marketplace",
            Self::Unknown => "unknown",
        }
    }
}

/// `IssueReporterStyles extends WindowStyles`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IssueReporterStyles {
    pub window: WindowStyles,
    pub text_link_color: Option<String>,
    pub text_link_active_foreground: Option<String>,
    pub input_background: Option<String>,
    pub input_foreground: Option<String>,
    pub input_border: Option<String>,
    pub input_error_border: Option<String>,
    pub input_error_background: Option<String>,
    pub input_error_foreground: Option<String>,
    pub input_active_border: Option<String>,
    pub button_background: Option<String>,
    pub button_foreground: Option<String>,
    pub button_hover_background: Option<String>,
    pub slider_background_color: Option<String>,
    pub slider_hover_color: Option<String>,
    pub slider_active_color: Option<String>,
}

/// `IssueReporterExtensionData`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueReporterExtensionData {
    pub name: String,
    pub publisher: Option<String>,
    pub version: String,
    pub id: String,
    pub is_theme: bool,
    pub is_builtin: bool,
    pub display_name: Option<String>,
    pub repository_url: Option<String>,
    pub bugs_url: Option<String>,
    pub extension_data: Option<String>,
    pub extension_template: Option<String>,
    pub data: Option<String>,
    pub uri: Option<UriComponents>,
    pub private_uri: Option<UriComponents>,
}

/// `IssueReporterData extends WindowData`: o que o serviço entrega ao abrir o relator.
#[derive(Clone)]
pub struct IssueReporterData {
    pub window: WindowData,
    pub styles: IssueReporterStyles,
    pub enabled_extensions: Vec<IssueReporterExtensionData>,
    /// Resolve quando `enabled_extensions` foi preenchida (ou falhou ao preencher). Deixa o painel do
    /// assistente esperar a enumeração assíncrona de extensões do `NativeIssueService` antes de desenhar a
    /// seção de extensões.
    pub when_extensions_loaded: Option<Promise<()>>,
    /// Resolve quando todos os dados assíncronos (extensões, token, integridade, experimentos) foram
    /// preenchidos. Deixa o painel do assistente repassar valores que chegam tarde, como
    /// `is_installation_pure` e `github_access_token`, ao modelo da sobreposição.
    pub when_data_complete: Option<Promise<()>>,
    pub issue_type: Option<IssueType>,
    pub issue_source: Option<IssueSource>,
    pub extension_id: Option<String>,
    pub experiments: Option<String>,
    pub restricted_mode: bool,
    pub is_installation_pure: bool,
    pub is_sessions_window: bool,
    pub github_access_token: String,
    pub issue_title: Option<String>,
    pub issue_body: Option<String>,
    pub data: Option<String>,
    pub uri: Option<UriComponents>,
    pub private_uri: Option<UriComponents>,
}

/// `ISettingSearchResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingSearchResult {
    pub extension_id: String,
    pub key: String,
    pub score: f64,
}

/// `ISimilarIssue`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimilarIssue {
    pub html_url: String,
    pub title: String,
    pub state: Option<String>,
}
