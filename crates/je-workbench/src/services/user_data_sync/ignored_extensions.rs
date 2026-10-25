//! Porte de `src/vs/platform/userDataSync/common/ignoredExtensions.ts`: `IIgnoredExtensionsManagementService`, que
//! decide quais extensões a sincronização ignora (`settingsSync.ignoredExtensions`), e o
//! `IgnoredExtensionsManagementService` que lê e grava essa configuração.
//!
//! Por que mora no `je-workbench`: `getIgnoredExtensions` recebe `ILocalExtension`, que é do `je-extensions`, e o
//! `je-extensions` depende do `je-platform` (o caminho inverso criaria um ciclo).
//!
//! Divergências:
//! - `ILocalExtension[]` é `&[ExtensionRef]` (a extensão compartilhada por `Rc`, o que o serviço de gerência devolve).
//! - `getValue<string[]>` devolve um [`Dynamic`]: só os itens que são texto entram na lista (o original lançaria
//!   num item que não é texto, no `toLowerCase`).

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_platform::configuration::ConfigurationTarget;
use je_platform::configuration::service::IConfigurationService;

use crate::services::extension_management::extension_management::ExtensionRef;

/// A chave de configuração das extensões ignoradas.
const IGNORED_EXTENSIONS_KEY: &str = "settingsSync.ignoredExtensions";

/// `IIgnoredExtensionsManagementService`.
pub trait IIgnoredExtensionsManagementService {
    fn get_ignored_extensions(&self, installed: &[ExtensionRef]) -> Vec<String>;

    fn has_to_never_sync_extension(&self, extension_id: &str) -> bool;
    fn has_to_always_sync_extension(&self, extension_id: &str) -> bool;
    fn update_ignored_extensions(&self, ignored_extension_id: &str, ignore: bool) -> Promise<()>;
    fn update_synchronized_extensions(&self, ignored_extension_id: &str, sync: bool) -> Promise<()>;
}

service_id!(pub I_IGNORED_EXTENSIONS_MANAGEMENT_SERVICE: dyn IIgnoredExtensionsManagementService = "IIgnoredExtensionsManagementService");

/// `IgnoredExtensionsManagementService`.
pub struct IgnoredExtensionsManagementService {
    configuration_service: Rc<dyn IConfigurationService>,
}

impl IgnoredExtensionsManagementService {
    pub fn new(configuration_service: Rc<dyn IConfigurationService>) -> Rc<Self> {
        Rc::new(Self { configuration_service })
    }

    /// `getValue<string[]>('settingsSync.ignoredExtensions')`, sem mudar a caixa.
    fn configured_value(&self) -> Vec<String> {
        match self.configuration_service.get_value(Some(IGNORED_EXTENSIONS_KEY), None) {
            Dynamic::Array(items) => items
                .to_vec()
                .into_iter()
                .filter_map(|item| match item {
                    Dynamic::String(text) => Some(text.to_string_lossy()),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `getConfiguredIgnoredExtensions`.
    fn get_configured_ignored_extensions(&self) -> Vec<String> {
        self.configured_value().into_iter().map(|id| id.to_lowercase()).collect()
    }

    /// Grava a lista nova (`undefined`, que remove a chave, se vazia).
    fn write(&self, current_value: Vec<String>) -> Promise<()> {
        let value = if current_value.is_empty() { Dynamic::Undefined } else { Dynamic::array(current_value.iter().map(|id| Dynamic::string(id))) };
        self.configuration_service.update_value(IGNORED_EXTENSIONS_KEY, value, None, Some(ConfigurationTarget::User))
    }

    /// A lista configurada, em minúsculas e sem o identificador (nem o `-identificador`).
    fn without(&self, extension_id: &str) -> Vec<String> {
        let negated = format!("-{extension_id}");
        self.configured_value().into_iter().map(|id| id.to_lowercase()).filter(|value| value != extension_id && *value != negated).collect()
    }
}

impl IIgnoredExtensionsManagementService for IgnoredExtensionsManagementService {
    fn has_to_never_sync_extension(&self, extension_id: &str) -> bool {
        let configured_ignored_extensions = self.get_configured_ignored_extensions();
        configured_ignored_extensions.contains(&extension_id.to_lowercase())
    }

    fn has_to_always_sync_extension(&self, extension_id: &str) -> bool {
        let configured_ignored_extensions = self.get_configured_ignored_extensions();
        configured_ignored_extensions.contains(&format!("-{}", extension_id.to_lowercase()))
    }

    fn update_ignored_extensions(&self, ignored_extension_id: &str, ignore: bool) -> Promise<()> {
        // first remove the extension completely from ignored extensions
        let mut current_value = self.without(ignored_extension_id);

        // Add only if ignored
        if ignore {
            current_value.push(ignored_extension_id.to_lowercase());
        }

        self.write(current_value)
    }

    fn update_synchronized_extensions(&self, extension_id: &str, sync: bool) -> Promise<()> {
        // first remove the extension completely from ignored extensions
        let mut current_value = self.without(extension_id);

        // Add only if synced
        if sync {
            current_value.push(format!("-{}", extension_id.to_lowercase()));
        }

        self.write(current_value)
    }

    fn get_ignored_extensions(&self, installed: &[ExtensionRef]) -> Vec<String> {
        let default_ignored_extensions = installed.iter().filter(|extension| extension.is_machine_scoped).map(|extension| extension.identifier.id.to_lowercase());
        let value = self.get_configured_ignored_extensions();
        let mut added: Vec<String> = Vec::new();
        let mut removed: Vec<String> = Vec::new();
        for key in value {
            match key.strip_prefix('-') {
                Some(removed_key) => removed.push(removed_key.to_string()),
                None => added.push(key),
            }
        }
        let mut distinct: Vec<String> = Vec::new();
        for setting in default_ignored_extensions.chain(added) {
            if !removed.contains(&setting) && !distinct.contains(&setting) {
                distinct.push(setting);
            }
        }
        distinct
    }
}
