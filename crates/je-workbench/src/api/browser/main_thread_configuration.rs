//! Porte de `src/vs/workbench/api/browser/mainThreadConfiguration.ts`: a ponte entre o `IConfigurationService` do
//! workbench e a configuração que o host de extensões enxerga (envio inicial, mudanças e escrita).
//!
//! Divergências:
//! - `@extHostNamedCustomer(MainContext.MainThreadConfiguration)` e o `IExtHostContext` do construtor: o registro de
//!   clientes não existe ainda; o construtor recebe o [`IMainContext`] e pede o proxy de `ExtHostConfiguration`.
//! - `getConfigurationData()!` é um `Option`: sem dados (o serviço de teste não tem camadas), o original espalharia
//!   `undefined` e ficaria só com `configurationScopes`; aqui os dados são os de uma configuração vazia.
//! - `{ donotNotifyError: true }` (`IConfigurationUpdateOptions`) não existe em `IConfigurationService::update_value`
//!   (ver `je_platform::configuration::service`), que não lê as opções.
//! - `URI.revive(overrides.resource)` não existe: o `UriComponents` já chega como [`URI`].
//! - `unknown | undefined` de `overriddenValue` é um [`Dynamic`] (`undefined` é [`Dynamic::Undefined`]); o `null` de
//!   `ConfigurationTarget | null` é `None`.
//! - O `Registry.as<IConfigurationRegistry>(...)` é o `configuration_registry()` global.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_platform::configuration::registry::{configuration_registry, get_scopes, ConfigurationScope};
use je_platform::configuration::service::{update_overrides_of, IConfigurationService};
use je_platform::configuration::{
    ConfigurationData, ConfigurationModelData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue,
};
use je_platform::environment::environment::EnvironmentService;
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState};

use crate::api::common::ext_host_protocol::{
    ExtHostConfigurationShape, ExtHostContext, IConfigurationInitData, IMainContext, MainThreadConfigurationShape,
};

/// `MainThreadConfiguration`.
pub struct MainThreadConfiguration {
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
    configuration_service: Rc<dyn IConfigurationService>,
    configuration_listener: DisposableRef,
}

/// Os dados de uma configuração vazia (o `IConfigurationModel` vazio de cada camada).
fn empty_configuration_data() -> ConfigurationData {
    let empty = || ConfigurationModelData { contents: Dynamic::object(Vec::<(&str, Dynamic)>::new()), keys: Vec::new(), overrides: Vec::new(), raw: None };
    ConfigurationData {
        defaults: empty(),
        policy: empty(),
        application: empty(),
        user_local: empty(),
        user_remote: empty(),
        workspace: empty(),
        folders: Vec::new(),
    }
}

/// `_getConfigurationData`.
fn get_configuration_data(configuration_service: &dyn IConfigurationService, environment_service: &dyn EnvironmentService) -> IConfigurationInitData {
    let data = configuration_service.get_configuration_data().unwrap_or_else(empty_configuration_data);
    let mut configuration_data = IConfigurationInitData { data, configuration_scopes: Vec::new() };
    // Send configurations scopes only in development mode.
    if !environment_service.is_built() || environment_service.is_extension_development() {
        configuration_data.configuration_scopes = get_scopes().into_iter().map(|(key, scope)| (key.to_string_lossy(), scope)).collect();
    }
    configuration_data
}

impl MainThreadConfiguration {
    pub fn new(
        ext_host_context: &dyn IMainContext,
        workspace_context_service: Rc<dyn IWorkspaceContextService>,
        configuration_service: Rc<dyn IConfigurationService>,
        environment_service: Rc<dyn EnvironmentService>,
    ) -> Rc<Self> {
        let proxy = ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_CONFIGURATION);

        proxy.rpc_initialize_configuration(get_configuration_data(configuration_service.as_ref(), environment_service.as_ref()));
        let listener_proxy = proxy.clone();
        let listener_configuration_service = configuration_service.clone();
        let configuration_listener = configuration_service.on_did_change_configuration().listen(move |event| {
            listener_proxy.rpc_accept_configuration_changed(
                get_configuration_data(listener_configuration_service.as_ref(), environment_service.as_ref()),
                event.change.clone(),
            );
        });
        Rc::new(Self { workspace_context_service, configuration_service, configuration_listener })
    }

    /// `writeConfiguration`.
    fn write_configuration(
        &self,
        target: Option<ConfigurationTarget>,
        key: &str,
        value: Dynamic,
        overrides: ConfigurationOverrides,
        scope_to_language: Option<bool>,
    ) -> Promise<()> {
        let target = target.unwrap_or_else(|| self.derive_configuration_target(key, &overrides));
        let configuration_value = self.configuration_service.inspect(key, &overrides);
        let overridden_value = |layer: Option<je_platform::configuration::models::InspectValue>| layer.map_or(Dynamic::Undefined, |layer| layer.override_value());
        let overridden = match target {
            ConfigurationTarget::Memory => overridden_value(configuration_value.memory()),
            ConfigurationTarget::WorkspaceFolder => overridden_value(configuration_value.workspace_folder()),
            ConfigurationTarget::Workspace => overridden_value(configuration_value.workspace()),
            ConfigurationTarget::UserRemote => overridden_value(configuration_value.user_remote()),
            _ => overridden_value(configuration_value.user_local()),
        };
        self.update_value(key, value, target, overridden, overrides, scope_to_language)
    }

    /// `_updateValue`.
    fn update_value(
        &self,
        key: &str,
        value: Dynamic,
        configuration_target: ConfigurationTarget,
        overridden_value: Dynamic,
        overrides: ConfigurationOverrides,
        scope_to_language: Option<bool>,
    ) -> Promise<()> {
        let has_override_identifier = overrides.override_identifier.as_ref().is_some_and(|identifier| !identifier.is_empty());
        let overrides = match scope_to_language {
            Some(true) => overrides,
            Some(false) => ConfigurationOverrides { override_identifier: None, resource: overrides.resource },
            None if has_override_identifier && !overridden_value.is_undefined() => overrides,
            None => ConfigurationOverrides { override_identifier: None, resource: overrides.resource },
        };
        let update_overrides: ConfigurationUpdateOverrides = update_overrides_of(&overrides);
        self.configuration_service.update_value(key, value, Some(update_overrides), Some(configuration_target))
    }

    /// `deriveConfigurationTarget`.
    fn derive_configuration_target(&self, key: &str, overrides: &ConfigurationOverrides) -> ConfigurationTarget {
        if overrides.resource.is_some() && self.workspace_context_service.get_workbench_state() == WorkbenchState::Workspace {
            let configuration_properties = configuration_registry().get_configuration_properties();
            if let Some(property) = configuration_properties.get(key) {
                if matches!(property.scope(), Some(ConfigurationScope::Resource) | Some(ConfigurationScope::LanguageOverridable)) {
                    return ConfigurationTarget::WorkspaceFolder;
                }
            }
        }
        ConfigurationTarget::Workspace
    }
}

impl IDisposable for MainThreadConfiguration {
    fn dispose(&self) {
        self.configuration_listener.dispose();
    }
}

impl MainThreadConfigurationShape for MainThreadConfiguration {
    fn rpc_update_configuration_option(
        &self,
        target: Option<ConfigurationTarget>,
        key: &str,
        value: Dynamic,
        overrides: Option<ConfigurationOverrides>,
        scope_to_language: Option<bool>,
    ) -> Promise<()> {
        self.write_configuration(target, key, value, overrides.unwrap_or_default(), scope_to_language)
    }

    fn rpc_remove_configuration_option(
        &self,
        target: Option<ConfigurationTarget>,
        key: &str,
        overrides: Option<ConfigurationOverrides>,
        scope_to_language: Option<bool>,
    ) -> Promise<()> {
        self.write_configuration(target, key, Dynamic::Undefined, overrides.unwrap_or_default(), scope_to_language)
    }
}
