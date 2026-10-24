//! Porte de `src/vs/platform/extensionRecommendations/common/extensionRecommendations.ts`.
//!
//! O `Array<string | URI>` de `promptWorkspaceRecommendations` é `Vec<RecommendedExtension>`.

use je_base::promise::Promise;
use je_base::uri::URI;

/// `RecommendationSource` (a ordem numérica decide a prioridade da fila de notificações).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecommendationSource {
    File = 1,
    Workspace = 2,
    Exe = 3,
}

/// `IExtensionRecommendations`, com as extensões como `Array<string | URI>` (o `Omit<..., 'extensions'>` que o serviço do
/// workbench usa).
#[derive(Clone, Debug)]
pub struct ExtensionRecommendationsRequest {
    pub source: RecommendationSource,
    pub extensions: Vec<RecommendedExtension>,
    pub name: String,
    pub search_value: Option<String>,
}

/// O `string | URI` de uma recomendação: um identificador da galeria ou uma extensão de recurso (pasta do workspace).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecommendedExtension {
    Gallery(String),
    Resource(URI),
}

impl RecommendedExtension {
    /// `isString(extension) ? extension : undefined`.
    pub fn as_gallery(&self) -> Option<&str> {
        match self {
            Self::Gallery(id) => Some(id),
            Self::Resource(_) => None,
        }
    }
}

/// `RecommendationSourceToString`.
pub fn recommendation_source_to_string(source: RecommendationSource) -> &'static str {
    match source {
        RecommendationSource::File => "file",
        RecommendationSource::Workspace => "workspace",
        RecommendationSource::Exe => "exe",
    }
}

/// `RecommendationsNotificationResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecommendationsNotificationResult {
    Ignored,
    Cancelled,
    TooMany,
    IncompatibleWindow,
    /// `'reacted'`.
    Accepted,
}

/// `IExtensionRecommendationNotificationService`.
pub trait IExtensionRecommendationNotificationService {
    fn ignored_recommendations(&self) -> Vec<String>;
    fn has_to_ignore_recommendation_notifications(&self) -> bool;
    fn prompt_important_extensions_install_notification(&self, recommendations: ExtensionRecommendationsRequest) -> Promise<RecommendationsNotificationResult>;
    fn prompt_workspace_recommendations(&self, recommendations: Vec<RecommendedExtension>) -> Promise<()>;
}
