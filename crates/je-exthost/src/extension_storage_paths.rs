//! `IExtensionStoragePaths` do host em processo (`workbench/api/common/extHostStoragePaths.ts`, `ExtensionStoragePaths`): onde cada
//! extensão guarda o estado do workspace (`storageUri`) e o global (`globalStorageUri`), e onde ficam os logs dela (`logUri`).
//!
//! Divergências:
//! - O original recebe `initData.environment.workspaceStorageHome`, `initData.environment.globalStorageHome`, `initData.workspace.id`
//!   e `initData.logsLocation`; o host em processo não tem `initData`, então o dono do host entrega os três diretórios já resolvidos em
//!   [`ExtensionStorageEnvironment`] (`workspace_storage` é o `workspaceStorageHome/<workspace.id>`).
//! - `_getOrCreateWorkspaceStoragePath` (que cria o diretório e o `meta.json` pelo `IExtHostConsumerFileSystem`) não existe aqui: o
//!   host não tem o `ExtHostConsumerFileSystem` de um processo próprio, e o diretório é criado por quem entrega o ambiente.
//! - `whenReady` não existe: não há criação assíncrona de diretório a esperar.

use je_base::uri::URI;
use je_extensions::scanner::ExtensionDescription;
use je_workbench::api::common::ext_host_custom_editors::IExtensionStoragePaths;

/// Os diretórios que o `initData` entrega ao `ExtensionStoragePaths` e ao `_loadExtensionContext`.
#[derive(Clone)]
pub struct ExtensionStorageEnvironment {
    /// `initData.logsLocation`.
    pub logs_location: URI,
    /// `initData.environment.globalStorageHome`.
    pub global_storage_home: URI,
    /// `initData.environment.workspaceStorageHome` unido ao `initData.workspace.id`; `None` sem workspace (`storageUri` é `undefined`).
    pub workspace_storage: Option<URI>,
}

/// `ExtensionStoragePaths`.
pub struct ExtensionStoragePaths {
    environment: ExtensionStorageEnvironment,
}

impl ExtensionStoragePaths {
    pub fn new(environment: ExtensionStorageEnvironment) -> Self {
        Self { environment }
    }

    /// `URI.joinPath(initData.logsLocation, extensionDescription.identifier.value)`: o `logUri` da extensão.
    pub fn log_uri(&self, extension: &ExtensionDescription) -> URI {
        je_platform::resources::join_path(&self.environment.logs_location, &[extension.identifier.value.as_str()])
    }
}

impl IExtensionStoragePaths for ExtensionStoragePaths {
    fn workspace_value(&self, extension: &ExtensionDescription) -> Option<URI> {
        self.environment.workspace_storage.as_ref().map(|value| je_platform::resources::join_path(value, &[extension.identifier.value.as_str()]))
    }

    fn global_value(&self, extension: &ExtensionDescription) -> URI {
        je_platform::resources::join_path(&self.environment.global_storage_home, &[extension.identifier.value.to_lowercase().as_str()])
    }
}
