//! Porte de `src/vs/workbench/services/configurationResolver/browser/configurationResolverService.ts`: o serviço do
//! navegador, que é o `BaseConfigurationResolverService` com `appRoot` e `execPath` indefinidos e o ambiente do processo
//! vazio.
//!
//! Notas de tradução:
//! - **Herança.** A subclasse só fixa os argumentos do construtor da base, então o serviço é a própria base: o alias
//!   [`ConfigurationResolverService`]. A instanciação (`Injectable`) é a do navegador; a do Electron
//!   (`electron-browser/configurationResolverService.ts`) lê o `appRoot`, o `execPath` e o ambiente do shell de serviços que
//!   ainda não têm porte.
//! - **`registerSingleton(IConfigurationResolverService, ConfigurationResolverService, InstantiationType.Delayed)`** é
//!   [`register`].

use std::rc::Rc;

use je_base::process::ProcessEnvironment;
use je_base::promise::Promise;
use je_instantiation::{Injectable, InstantiationType, register_singleton, service_impl};
use je_platform::commands::commands::ICommandService;
use je_platform::configuration::service::IConfigurationService;
use je_platform::label::label::ILabelService;
use je_platform::path::path_service::IPathService;
use je_platform::quickinput::quick_input::IQuickInputService;
use je_platform::storage::StorageService;
use je_platform::workspace::IWorkspaceContextService;

use super::base_configuration_resolver_service::{BaseConfigurationResolverService, ResolverAppContext};
use crate::services::configuration_resolver::common::configuration_resolver::{I_CONFIGURATION_RESOLVER_SERVICE, IConfigurationResolverService};
use crate::services::editor::editor_service::IEditorService;
use crate::services::extensions::extensions::IExtensionService;

/// `ConfigurationResolverService`.
pub type ConfigurationResolverService = BaseConfigurationResolverService;

impl Injectable for BaseConfigurationResolverService {
    type Args = ();
    type Deps = (
        Rc<dyn IEditorService>,
        Rc<dyn IConfigurationService>,
        Rc<dyn ICommandService>,
        Rc<dyn IWorkspaceContextService>,
        Rc<dyn IQuickInputService>,
        Rc<dyn ILabelService>,
        Rc<dyn IPathService>,
        Rc<dyn IExtensionService>,
        Rc<dyn StorageService>,
    );

    fn create(
        _: (),
        (editor_service, configuration_service, command_service, workspace_context_service, quick_input_service, label_service, path_service, extension_service, storage_service): Self::Deps,
    ) -> Rc<Self> {
        Rc::new(BaseConfigurationResolverService::new(
            ResolverAppContext { get_app_root: Rc::new(|| None), get_exec_path: Rc::new(|| None) },
            Promise::resolved(ProcessEnvironment::new()),
            editor_service,
            configuration_service,
            command_service,
            workspace_context_service,
            quick_input_service,
            label_service,
            path_service,
            extension_service,
            storage_service,
        ))
    }
}

service_impl!(ConfigurationResolverService => dyn IConfigurationResolverService);

/// `registerSingleton(IConfigurationResolverService, ConfigurationResolverService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<ConfigurationResolverService>(I_CONFIGURATION_RESOLVER_SERVICE, InstantiationType::Delayed);
}
