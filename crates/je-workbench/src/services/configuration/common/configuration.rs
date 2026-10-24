//! Porte de `src/vs/workbench/services/configuration/common/configuration.ts`: as constantes de pasta e de esquema, os
//! escopos de cada camada, as configurações isoladas (`tasks`, `launch`, `mcp`), o cache e o contrato do serviço de
//! configuração do workbench.
//!
//! Decisões de tradução:
//! - `WORKSPACE_STANDALONE_CONFIGURATIONS` e `USER_STANDALONE_CONFIGURATIONS` (objetos sem protótipo cuja ordem de
//!   chaves importa) são listas de pares `(chave, caminho)` na ordem do original.
//! - `IWorkbenchConfigurationService` estende o [`IConfigurationService`] reduzido do porte da plataforma
//!   (`reloadConfiguration(target?)` e `updateValue` com opções ficam no `WorkspaceService`, que os expõe como métodos
//!   próprios).
//! - `RestrictedSettings.workspaceFolder` (um `ResourceMap`) fica num `Rc`, porque o evento entrega o valor a vários
//!   ouvintes.

use std::rc::Rc;

use je_base::JsString;
use je_base::event::Event;
use je_base::map::ResourceMap;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;
use je_platform::configuration::registry::ConfigurationScope;
use je_platform::configuration::service::IConfigurationService;
use je_platform::workspace::AnyWorkspaceIdentifier;

pub const FOLDER_CONFIG_FOLDER_NAME: &str = ".vscode";
pub const FOLDER_SETTINGS_NAME: &str = "settings";
pub const FOLDER_SETTINGS_PATH: &str = ".vscode/settings.json";

pub const DEFAULT_SETTINGS_SCHEMA_ID: &str = "vscode://schemas/settings/default";
pub const USER_SETTINGS_SCHEMA_ID: &str = "vscode://schemas/settings/user";
pub const PROFILE_SETTINGS_SCHEMA_ID: &str = "vscode://schemas/settings/profile";
pub const MACHINE_SETTINGS_SCHEMA_ID: &str = "vscode://schemas/settings/machine";
pub const WORKSPACE_SETTINGS_SCHEMA_ID: &str = "vscode://schemas/settings/workspace";
pub const FOLDER_SETTINGS_SCHEMA_ID: &str = "vscode://schemas/settings/folder";
pub const LAUNCH_SCHEMA_ID: &str = "vscode://schemas/launch";
pub const TASKS_SCHEMA_ID: &str = "vscode://schemas/tasks";
pub const MCP_SCHEMA_ID: &str = "vscode://schemas/mcp";

pub const APPLICATION_SCOPES: [ConfigurationScope; 2] = [ConfigurationScope::Application, ConfigurationScope::ApplicationMachine];
pub const PROFILE_SCOPES: [ConfigurationScope; 5] = [
    ConfigurationScope::Machine,
    ConfigurationScope::Window,
    ConfigurationScope::Resource,
    ConfigurationScope::LanguageOverridable,
    ConfigurationScope::MachineOverridable,
];
pub const LOCAL_MACHINE_PROFILE_SCOPES: [ConfigurationScope; 3] = [ConfigurationScope::Window, ConfigurationScope::Resource, ConfigurationScope::LanguageOverridable];
pub const LOCAL_MACHINE_SCOPES: [ConfigurationScope; 4] =
    [ConfigurationScope::Application, ConfigurationScope::Window, ConfigurationScope::Resource, ConfigurationScope::LanguageOverridable];
pub const REMOTE_MACHINE_SCOPES: [ConfigurationScope; 6] = [
    ConfigurationScope::Machine,
    ConfigurationScope::ApplicationMachine,
    ConfigurationScope::Window,
    ConfigurationScope::Resource,
    ConfigurationScope::LanguageOverridable,
    ConfigurationScope::MachineOverridable,
];
pub const WORKSPACE_SCOPES: [ConfigurationScope; 4] =
    [ConfigurationScope::Window, ConfigurationScope::Resource, ConfigurationScope::LanguageOverridable, ConfigurationScope::MachineOverridable];
pub const FOLDER_SCOPES: [ConfigurationScope; 3] = [ConfigurationScope::Resource, ConfigurationScope::LanguageOverridable, ConfigurationScope::MachineOverridable];

pub const TASKS_CONFIGURATION_KEY: &str = "tasks";
pub const LAUNCH_CONFIGURATION_KEY: &str = "launch";
pub const MCP_CONFIGURATION_KEY: &str = "mcp";

/// `WORKSPACE_STANDALONE_CONFIGURATIONS`: a chave e o caminho relativo à raiz do workspace, na ordem do original.
pub const WORKSPACE_STANDALONE_CONFIGURATIONS: [(&str, &str); 3] =
    [(TASKS_CONFIGURATION_KEY, ".vscode/tasks.json"), (LAUNCH_CONFIGURATION_KEY, ".vscode/launch.json"), (MCP_CONFIGURATION_KEY, ".vscode/mcp.json")];

/// `USER_STANDALONE_CONFIGURATIONS`: a chave e o caminho relativo à pasta do perfil, na ordem do original.
pub const USER_STANDALONE_CONFIGURATIONS: [(&str, &str); 2] = [(TASKS_CONFIGURATION_KEY, "tasks.json"), (MCP_CONFIGURATION_KEY, "mcp.json")];

/// `ConfigurationKey['type']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigurationKeyType {
    Defaults,
    User,
    Workspaces,
    Folder,
}

impl ConfigurationKeyType {
    /// O texto do original (`'defaults' | 'user' | 'workspaces' | 'folder'`).
    pub fn as_str(self) -> &'static str {
        match self {
            ConfigurationKeyType::Defaults => "defaults",
            ConfigurationKeyType::User => "user",
            ConfigurationKeyType::Workspaces => "workspaces",
            ConfigurationKeyType::Folder => "folder",
        }
    }
}

/// `ConfigurationKey`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigurationKey {
    pub type_: ConfigurationKeyType,
    pub key: String,
}

/// `IConfigurationCache`.
pub trait IConfigurationCache {
    fn needs_caching(&self, resource: &URI) -> bool;
    fn read(&self, key: &ConfigurationKey) -> Promise<String>;
    fn write(&self, key: &ConfigurationKey, content: &str) -> Promise<()>;
    fn remove(&self, key: &ConfigurationKey) -> Promise<()>;
}

/// `RestrictedSettings`.
#[derive(Clone, Default)]
pub struct RestrictedSettings {
    pub default: Vec<JsString>,
    pub application: Option<Vec<JsString>>,
    pub user_local: Option<Vec<JsString>>,
    pub user_remote: Option<Vec<JsString>>,
    pub workspace: Option<Vec<JsString>>,
    pub workspace_folder: Option<Rc<ResourceMap<Vec<JsString>>>>,
}

/// `IWorkbenchConfigurationService`.
pub trait IWorkbenchConfigurationService: IConfigurationService {
    /// Restricted settings defined in each configuration target.
    fn restricted_settings(&self) -> RestrictedSettings;

    /// Event that triggers when the restricted settings changes.
    fn on_did_change_restricted_settings(&self) -> Event<RestrictedSettings>;

    /// A promise that resolves when the remote configuration is loaded in a remote window. The promise is resolved
    /// immediately if the window is not remote.
    fn when_remote_configuration_loaded(&self) -> Promise<()>;

    /// Initialize configuration service for the given workspace.
    fn initialize(&self, arg: &AnyWorkspaceIdentifier) -> Promise<()>;

    /// Returns true if the setting can be applied for all profiles otherwise false.
    fn is_setting_applied_for_all_profiles(&self, setting: &str) -> bool;
}

service_id!(pub I_WORKBENCH_CONFIGURATION_SERVICE: dyn IWorkbenchConfigurationService = "configurationService");

pub const TASKS_DEFAULT: &str = "{\n\t\"version\": \"2.0.0\",\n\t\"tasks\": []\n}";

pub const APPLY_ALL_PROFILES_SETTING: &str = "workbench.settings.applyToAllProfiles";
