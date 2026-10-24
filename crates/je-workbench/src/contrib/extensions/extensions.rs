//! Porte de `src/vs/workbench/contrib/extensions/common/extensions.ts`: o contrato `IExtension` (a extensão como o
//! workbench a mostra: instalada, da galeria ou de um recurso), os estados que ele expõe e as chaves de configuração.
//!
//! Recortes (cada um entra inteiro quando o serviço que ele descreve for portado):
//! - **`IExtensionsWorkbenchService`, `InstallExtensionOptions`, `IExtensionsNotification`, `ExtensionContainers`,
//!   `IExtensionsViewState` e `IExtensionContainer`** dependem da interface de usuário (editor, views, contêineres de
//!   widgets) e do serviço do workbench, que ainda não existe no porte.
//! - **As chaves de contexto** (`RawContextKey`) e os grupos de menu (`MenuId`) não têm o serviço de contexto ligado a
//!   este módulo; as constantes de texto que as acompanham estão aqui.
//! - **`IExtension.local`, `gallery`** são os tipos do `je-extensions` ([`LocalExtension`] e [`GalleryExtension`]), a
//!   `ILocalExtension` e a `IGalleryExtension` do original, compartilhados por `Rc` porque o original passa o mesmo
//!   objeto de mão em mão. `telemetryData` é um [`Dynamic`], o objeto solto do original.
//! - **`IExtension.server`** é `Option<Rc<IExtensionManagementServer>>`, comparado por identidade (`Rc::ptr_eq`).

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::management::{DeprecationInfo, GalleryExtension, LocalExtension};
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionManifest, ExtensionType};

use crate::services::extension_management::extension_management::{EnablementState, IExtensionManagementServer, IResourceExtension};

pub const VIEWLET_ID: &str = "workbench.view.extensions";

/// `ExtensionState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionState {
    Installing,
    Installed,
    Uninstalling,
    Uninstalled,
}

/// `ExtensionRuntimeActionType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionRuntimeActionType {
    ReloadWindow,
    RestartExtensions,
    DownloadUpdate,
    ApplyUpdate,
    QuitAndInstall,
}

impl ExtensionRuntimeActionType {
    /// O valor de texto do enum do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReloadWindow => "reloadWindow",
            Self::RestartExtensions => "restartExtensions",
            Self::DownloadUpdate => "downloadUpdate",
            Self::ApplyUpdate => "applyUpdate",
            Self::QuitAndInstall => "quitAndInstall",
        }
    }
}

/// `ExtensionRuntimeState`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionRuntimeState {
    pub action: ExtensionRuntimeActionType,
    pub reason: String,
}

/// `IExtension`.
pub trait IExtension {
    fn extension_type(&self) -> ExtensionType;
    fn is_builtin(&self) -> bool;
    fn is_workspace_scoped(&self) -> bool;
    fn state(&self) -> ExtensionState;
    fn name(&self) -> String;
    fn display_name(&self) -> String;
    fn identifier(&self) -> ExtensionIdentifierInfo;
    fn publisher(&self) -> String;
    fn publisher_display_name(&self) -> String;
    fn publisher_url(&self) -> Option<URI>;
    /// `publisherDomain`: `{ link, verified }`.
    fn publisher_domain(&self) -> Option<(String, bool)>;
    fn publisher_sponsor_link(&self) -> Option<URI>;
    fn pinned(&self) -> bool;
    fn version(&self) -> String;
    /// `private`.
    fn is_private(&self) -> bool;
    fn latest_version(&self) -> String;
    fn pre_release(&self) -> bool;
    fn is_pre_release_version(&self) -> bool;
    fn has_pre_release_version(&self) -> bool;
    fn has_release_version(&self) -> bool;
    fn description(&self) -> String;
    fn url(&self) -> Option<String>;
    fn repository(&self) -> Option<String>;
    fn support_url(&self) -> Option<String>;
    fn icon_url(&self) -> Option<String>;
    fn icon_url_fallback(&self) -> Option<String>;
    fn license_url(&self) -> Option<String>;
    fn install_count(&self) -> Option<f64>;
    fn rating(&self) -> Option<f64>;
    fn rating_count(&self) -> Option<f64>;
    fn rating_url(&self) -> Option<String>;
    fn outdated(&self) -> bool;
    fn outdated_target_platform(&self) -> bool;
    fn runtime_state(&self) -> Option<ExtensionRuntimeState>;
    fn enablement_state(&self) -> EnablementState;
    fn tags(&self) -> Vec<String>;
    fn categories(&self) -> Vec<String>;
    fn dependencies(&self) -> Vec<String>;
    fn extension_pack(&self) -> Vec<String>;
    fn telemetry_data(&self) -> Dynamic;
    fn preview(&self) -> bool;
    fn get_manifest(&self, token: CancellationToken) -> Promise<Option<ExtensionManifest>>;
    fn has_readme(&self) -> bool;
    fn get_readme(&self, token: CancellationToken) -> Promise<String>;
    fn has_changelog(&self) -> bool;
    fn get_changelog(&self, token: CancellationToken) -> Promise<String>;
    fn server(&self) -> Option<Rc<IExtensionManagementServer>>;
    fn local(&self) -> Option<Rc<LocalExtension>>;
    fn gallery(&self) -> Option<Rc<GalleryExtension>>;
    /// `gallery = value`: troca a extensão da galeria (e descarta o que estava em cache dela).
    fn set_gallery(&self, gallery: Option<Rc<GalleryExtension>>);
    fn resource_extension(&self) -> Option<IResourceExtension>;
    fn is_malicious(&self) -> Option<bool>;
    fn malicious_info_link(&self) -> Option<String>;
    fn deprecation_info(&self) -> Option<DeprecationInfo>;
    fn missing_from_gallery(&self) -> bool;
}

pub const CONFIGURATION_KEY: &str = "extensions";
pub const AUTO_UPDATE_CONFIGURATION_KEY: &str = "extensions.autoUpdate";
pub const AUTO_UPDATE_DELAY_CONFIGURATION_KEY: &str = "extensions.autoUpdateDelay";
pub const AUTO_CHECK_UPDATES_CONFIGURATION_KEY: &str = "extensions.autoCheckUpdates";
pub const CLOSE_EXTENSION_DETAILS_ON_VIEW_CHANGE_KEY: &str = "extensions.closeExtensionDetailsOnViewChange";
pub const AUTO_RESTART_CONFIGURATION_KEY: &str = "extensions.autoRestart";

pub const WORKSPACE_RECOMMENDATIONS_VIEW_ID: &str = "workbench.views.extensions.workspaceRecommendations";
pub const OUTDATED_EXTENSIONS_VIEW_ID: &str = "workbench.views.extensions.searchOutdated";
pub const TOGGLE_IGNORE_EXTENSION_ACTION_ID: &str = "workbench.extensions.action.toggleIgnoreExtension";
pub const SELECT_INSTALL_VSIX_EXTENSION_COMMAND_ID: &str = "workbench.extensions.action.installVSIX";
pub const INSTALL_EXTENSION_FROM_VSIX_COMMAND_ID: &str = "workbench.extensions.command.installFromVSIX";
pub const LIST_WORKSPACE_UNSUPPORTED_EXTENSIONS_COMMAND_ID: &str = "workbench.extensions.action.listWorkspaceUnsupportedExtensions";

pub const THEME_ACTIONS_GROUP: &str = "_theme_";
pub const INSTALL_ACTIONS_GROUP: &str = "0_install";
pub const UPDATE_ACTIONS_GROUP: &str = "0_update";
