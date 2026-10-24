//! Porte de `IExtensionTipsService`, `IConfigBasedExtensionTip` e `IExecutableBasedExtensionTip` de
//! `src/vs/platform/extensionManagement/common/extensionManagement.ts`: o contrato das dicas de extensão por
//! configuração do workspace e por executável instalado. A implementação (`ExtensionTipsService`) usa o
//! `IExtensionManagementService` do workbench e fica em `je-workbench`.

use je_base::promise::Promise;
use je_base::uri::URI;

/// `IConfigBasedExtensionTip`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigBasedExtensionTip {
    pub extension_id: String,
    pub extension_name: String,
    pub is_extension_pack: bool,
    pub config_name: String,
    pub important: bool,
    pub when_not_installed: Option<Vec<String>>,
}

/// `IExecutableBasedExtensionTip`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutableBasedExtensionTip {
    pub extension_id: String,
    pub extension_name: String,
    pub is_extension_pack: bool,
    pub exe_name: String,
    pub exe_friendly_name: String,
    pub windows_path: Option<String>,
    pub when_not_installed: Option<Vec<String>>,
}

/// `IExtensionTipsService`.
pub trait IExtensionTipsService {
    fn get_config_based_tips(&self, folder: &URI) -> Promise<Vec<ConfigBasedExtensionTip>>;
    fn get_important_executable_based_tips(&self) -> Promise<Vec<ExecutableBasedExtensionTip>>;
    fn get_other_executable_based_tips(&self) -> Promise<Vec<ExecutableBasedExtensionTip>>;
}
