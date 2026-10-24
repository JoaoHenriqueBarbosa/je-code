//! Porte de `src/vs/workbench/services/extensionRecommendations/common/extensionRecommendations.ts`: o motivo de uma
//! recomendação e os contratos do serviço de recomendações e do serviço de recomendações ignoradas.

use je_base::event::Event;
use je_base::product::IStringDictionary;
use je_base::promise::Promise;

/// `ExtensionRecommendationReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionRecommendationReason {
    Workspace,
    File,
    Executable,
    WorkspaceConfig,
    DynamicWorkspace,
    Experimental,
    Application,
}

/// `IExtensionRecommendationReason`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IExtensionRecommendationReason {
    pub reason_id: ExtensionRecommendationReason,
    pub reason_text: String,
}

/// O `Array<string | URI>` de `getWorkspaceRecommendations`.
pub use je_platform::extension_recommendations::extension_recommendations::RecommendedExtension;

/// As dicas de executáveis do `getExeBasedRecommendations`: `{ important: string[]; others: string[] }`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportantAndOthers {
    pub important: Vec<String>,
    pub others: Vec<String>,
}

/// `IExtensionRecommendationsService`.
pub trait IExtensionRecommendationsService {
    fn on_did_change_recommendations(&self) -> Event<()>;
    fn get_all_recommendations_with_reason(&self) -> IStringDictionary<IExtensionRecommendationReason>;

    fn get_important_recommendations(&self) -> Promise<Vec<String>>;
    fn get_other_recommendations(&self) -> Promise<Vec<String>>;
    fn get_file_based_recommendations(&self) -> Vec<String>;
    fn get_exe_based_recommendations(&self, exe: Option<&str>) -> Promise<ImportantAndOthers>;
    fn get_config_based_recommendations(&self) -> Promise<ImportantAndOthers>;
    fn get_workspace_recommendations(&self) -> Promise<Vec<RecommendedExtension>>;
    fn get_keymap_recommendations(&self) -> Vec<String>;
    fn get_language_recommendations(&self) -> Vec<String>;
    fn get_remote_recommendations(&self) -> Vec<String>;
}

/// `IgnoredRecommendationChangeNotification`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IgnoredRecommendationChangeNotification {
    pub extension_id: String,
    pub is_recommended: bool,
}

/// `IExtensionIgnoredRecommendationsService`.
pub trait IExtensionIgnoredRecommendationsService {
    fn on_did_change_ignored_recommendations(&self) -> Event<()>;
    fn ignored_recommendations(&self) -> Vec<String>;

    fn on_did_change_global_ignored_recommendation(&self) -> Event<IgnoredRecommendationChangeNotification>;
    fn global_ignored_recommendations(&self) -> Vec<String>;
    fn toggle_global_ignored_recommendation(&self, extension_id: &str, ignore: bool);
}
