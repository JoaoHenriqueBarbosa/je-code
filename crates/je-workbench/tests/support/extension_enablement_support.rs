//! Dublês de serviço do `extensionEnablementService.test.ts` que outros testes reaproveitam: o `Fixture` (o
//! `TestInstantiationService` com o `setup` da suíte `ExtensionEnablementService Test`), o `TestExtensionEnablementService`,
//! `anExtensionManagementServerService`, `aLocalExtension2` e os dublês dos serviços que o construtor do
//! `ExtensionEnablementService` recebe.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado antes de qualquer serviço; o `disposableStore`
//!   do original é `fixture.guard`. O módulo [`common`] é o de `je-base/tests/common`, incluído aqui (e não no topo de
//!   cada teste) para que quem usa estes dublês compartilhe o mesmo `LeakGuard`.
//! - `TestInstantiationService` (`stub`, `get`, `createInstance`) é o [`Fixture`]: ele guarda os serviços que o `setup`
//!   registra e que os casos trocam no meio (`stub(IWorkbenchEnvironmentService, { ... })`, `stub(IWorkspaceTrustManagementService,
//!   { isWorkspaceTrusted() ... })`...). Como no original, `new TestExtensionEnablementService(instantiationService)` usa o
//!   que estiver registrado e, para o que falta, registra um serviço novo que os demais construtores passam a compartilhar
//!   (o armazenamento, a confiança no workspace, o ciclo de vida...).
//! - O construtor do original lê os serviços do `instantiationService`; aqui [`TestExtensionEnablementService::create`]
//!   recebe os serviços já resolvidos ([`EnablementServices`]), para que o `Fixture` desta suíte e o `setup` do
//!   `extensionsWorkbenchService.test.ts` (que tem os seus próprios serviços registrados) montem o mesmo objeto.
//! - `ExtensionManagementService` (o serviço real do workbench, que o original constrói por `createInstance`) é o
//!   [`TestExtensionManagementService`]: junta os eventos e as listas dos servidores de gerenciamento, como o real, sem
//!   a instalação, que o `ExtensionEnablementService` não usa. `getInstalled: () => Promise.resolve(installed)` devolve
//!   a lista viva, que o gerenciador só copia quando continua depois do `await`: o servidor local do dublê lê
//!   `installed` nesse momento, e não quando a promessa é pedida.
//! - `aLocalExtension2` devolve `Object.create({ manifest, ...properties })`: aqui é uma [`IExtension`] compartilhada
//!   (`Rc`), que o original compara por identidade. O manifesto passa por `ExtensionManifest::from_value`, que descarta o
//!   que tem o tipo errado: `aContributes('views' | 'viewsContainers' | 'menus')` põe `{}` nesses pontos (que são
//!   dicionários) e `[]` nos demais, e os dois valem como "o ponto existe".
//! - `UserDataSyncAccountService` passado como "serviço" (`stub(IUserDataSyncAccountService, UserDataSyncAccountService)`,
//!   a classe, sem `account`) é a conta vazia.

#![allow(dead_code)]

#[path = "../../../je-base/tests/common/mod.rs"]
pub mod common;

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::default_account::{ICopilotTokenInfo, IDefaultAccount, IDefaultAccountAuthenticationProvider, IPolicyData};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event, any};
use je_base::lifecycle::IDisposable;
use je_base::network::schemas;
use je_base::promise::Promise;
use je_base::uri::{Field, URI, UriChange};
use je_extensions::allowed::AllowedExtensionsService;
use je_extensions::management::{
    ALLOWED_EXTENSIONS_CONFIG_KEY, ExtensionOrPublisher, ExtensionsControlManifest, InstallSource, MaliciousExtensionInfo,
};
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionManifest, ExtensionType, TargetPlatform};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::NullLogService;
use je_platform::notification::INotificationService;
use je_platform::policy::managed_settings_freshness::IManagedSettingsFreshness;
use je_platform::resources::uri_file;
use je_platform::storage::{AbstractStorageService, InMemoryStorageService, StorageService};
use je_platform::user_data_sync::user_data_sync::{IUserDataSyncEnablementService, SyncResource};
use je_platform::user_data_sync::user_data_sync_account::{IUserDataSyncAccount, IUserDataSyncAccountService};
use je_platform::workspace::IWorkspaceContextService;
use je_platform::workspace_trust::IWorkspaceTrustManagementService;
use je_workbench::services::accounts::default_account::{
    IDefaultAccountProvider, IDefaultAccountRefreshOptions, IDefaultAccountService, ISignInOptions, ManagedSettingsFetchStatus,
};
use je_workbench::services::accounts::managed_settings::IManagedSettingsCompatibilityError;
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_workbench::services::extension_management::extension_bisect::IExtensionBisectService;
use je_workbench::services::extension_management::extension_enablement_service::{ExtensionEnablementService, ExtensionEnablementServiceParts};
use je_workbench::services::extension_management::extension_management::{
    DidChangeProfileEvent, DidUninstallExtensionEvent, EnablementState, ExtensionInstallLocation, ExtensionRef, IExtension, IExtensionManagementServer,
    IExtensionManagementServerService, IExtensionManagementService, IProfileAwareExtensionManagementService, IWorkbenchExtensionEnablementService,
    IWorkbenchExtensionManagementService, InstallExtensionResult, local_extension_uri,
};
use je_workbench::services::extension_management::global_extension_enablement_service::GlobalExtensionEnablementService;
use je_workbench::services::extension_management::service_contracts::IChatEntitlementService;
use je_workbench::services::extensions::extension_manifest_properties_service::ExtensionManifestPropertiesService;
use je_workbench::services::host::host::IHostService;
use je_workbench::services::lifecycle::lifecycle::ILifecycleService;
use je_workbench::test::browser::workbench_test_services::{EnvironmentOverrides, TestEnvironmentService, TestHostService, product_service};
use je_workbench::test::common::workbench_test_services::{
    TestChatEntitlementService, TestContextService, TestLifecycleService, TestNotificationService, TestProductService, TestWorkspaceTrustEnablementService,
    TestWorkspaceTrustManagementService, TestWorkspaceTrustRequestService,
};
use serde_json::{Map, Value, json};

// ---------------------------------------------------------------------------------------------------------------------
// Extensões, servidores de gerenciamento e os dublês dos serviços.
// ---------------------------------------------------------------------------------------------------------------------

/// O valor JSON como valor dinâmico (o que o `TestConfigurationService` guarda).
pub fn dynamic_of(value: &Value) -> Dynamic {
    match value {
        Value::Null => Dynamic::Null,
        Value::Bool(boolean) => Dynamic::Bool(*boolean),
        Value::Number(number) => Dynamic::Number(number.as_f64().expect("número finito")),
        Value::String(text) => Dynamic::string(text),
        Value::Array(items) => Dynamic::array(items.iter().map(dynamic_of)),
        Value::Object(entries) => Dynamic::object(entries.iter().map(|(key, entry)| (key.as_str(), dynamic_of(entry)))),
    }
}

/// `URI.file(path).with({ scheme })`.
pub fn uri_with_scheme(path: &str, scheme: &str) -> URI {
    uri_file(path).with(&UriChange { scheme: Field::from(scheme), ..UriChange::default() })
}

/// `{ id }`.
pub fn identifier(id: &str) -> ExtensionIdentifierInfo {
    ExtensionIdentifierInfo::new(id)
}

/// As propriedades de `aLocalExtension2` que os casos trocam: `location`, `type` e `isValid`.
#[derive(Default)]
pub struct Properties {
    pub location: Option<URI>,
    pub extension_type: Option<ExtensionType>,
    pub is_valid: Option<bool>,
}

impl Properties {
    pub fn at(location: URI) -> Self {
        Self { location: Some(location), ..Self::default() }
    }

    pub fn of_type(extension_type: ExtensionType) -> Self {
        Self { extension_type: Some(extension_type), ..Self::default() }
    }

    pub fn invalid() -> Self {
        Self { is_valid: Some(false), ..Self::default() }
    }
}

/// `aLocalExtension(id, contributes?, type?)`.
pub fn a_local_extension_with(id: &str, contributes: Option<Value>, extension_type: Option<ExtensionType>) -> ExtensionRef {
    a_local_extension2(
        id,
        contributes.map_or_else(|| json!({}), |contributes| json!({ "contributes": contributes })),
        extension_type.map_or_else(Properties::default, Properties::of_type),
    )
}

/// `aLocalExtension(id)`.
pub fn a_local_extension(id: &str) -> ExtensionRef {
    a_local_extension_with(id, None, None)
}

/// `aContributes(...points)`.
pub fn a_contributes(points: &[&str]) -> Value {
    let mut result = Map::new();
    for point in points {
        // Os pontos que o manifesto tipa como dicionário recebem `{}` (ver a nota do módulo).
        let is_dictionary = matches!(*point, "views" | "viewsContainers" | "menus");
        result.insert((*point).to_owned(), if is_dictionary { json!({}) } else { json!([]) });
    }
    Value::Object(result)
}

/// `aLocalExtension2(id, manifest, properties)`.
pub fn a_local_extension2(id: &str, manifest: Value, properties: Properties) -> ExtensionRef {
    let mut parts = id.split('.');
    let publisher = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    let Value::Object(manifest) = manifest else { panic!("o manifesto é um objeto") };
    let mut manifest_properties = Map::new();
    manifest_properties.insert("name".to_owned(), json!(name));
    manifest_properties.insert("publisher".to_owned(), json!(publisher));
    manifest_properties.extend(manifest);
    let extension_type = properties.extension_type.unwrap_or(ExtensionType::User);
    let location = properties.location.unwrap_or_else(|| uri_file(&format!("pub.{name}")));
    let location = if location.scheme().to_string_lossy() == schemas::FILE { location.path().to_string_lossy() } else { location.to_string(true).to_string_lossy() };
    Rc::new(IExtension {
        extension_type,
        is_builtin: extension_type == ExtensionType::System,
        identifier: identifier(id),
        manifest: ExtensionManifest::from_value(Value::Object(manifest_properties)),
        location,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        readme_url: None,
        changelog_url: None,
        is_valid: properties.is_valid.unwrap_or(true),
        validations: Vec::new(),
        pre_release: false,
        is_workspace_scoped: false,
        is_machine_scoped: false,
        is_application_scoped: false,
        publisher_id: None,
        installed_timestamp: None,
        is_pre_release_version: false,
        has_pre_release_version: false,
        private: false,
        updated: false,
        pinned: false,
        force_auto_update: false,
        source: InstallSource::Vsix,
        size: 0.0,
    })
}

/// `a === b` entre servidores (`null === null` vale).
pub fn is_same_server(a: &Option<Rc<IExtensionManagementServer>>, b: &Option<Rc<IExtensionManagementServer>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// O que `anExtensionManagementServerService` devolve.
pub struct TestExtensionManagementServerService {
    pub local: Option<Rc<IExtensionManagementServer>>,
    pub remote: Option<Rc<IExtensionManagementServer>>,
    pub web: Option<Rc<IExtensionManagementServer>>,
}

impl IExtensionManagementServerService for TestExtensionManagementServerService {
    fn local_extension_management_server(&self) -> Option<Rc<IExtensionManagementServer>> {
        self.local.clone()
    }

    fn remote_extension_management_server(&self) -> Option<Rc<IExtensionManagementServer>> {
        self.remote.clone()
    }

    fn web_extension_management_server(&self) -> Option<Rc<IExtensionManagementServer>> {
        self.web.clone()
    }

    fn get_extension_management_server(&self, extension: &IExtension) -> Option<Rc<IExtensionManagementServer>> {
        let scheme = local_extension_uri(extension).scheme().to_string_lossy();
        if scheme == schemas::FILE {
            return self.local.clone();
        }
        if scheme == schemas::VSCODE_REMOTE {
            return self.remote.clone();
        }
        self.web.clone()
    }

    fn get_extension_install_location(&self, extension: &IExtension) -> Option<ExtensionInstallLocation> {
        let server = self.get_extension_management_server(extension);
        Some(if is_same_server(&server, &self.remote) {
            ExtensionInstallLocation::Remote
        } else if is_same_server(&server, &self.web) {
            ExtensionInstallLocation::Web
        } else {
            ExtensionInstallLocation::Local
        })
    }
}

/// `anExtensionManagementServerService(local, remote, web)`.
pub fn an_extension_management_server_service(
    local: Option<Rc<IExtensionManagementServer>>,
    remote: Option<Rc<IExtensionManagementServer>>,
    web: Option<Rc<IExtensionManagementServer>>,
) -> Rc<dyn IExtensionManagementServerService> {
    Rc::new(TestExtensionManagementServerService { local, remote, web })
}

/// O serviço de gerenciamento do servidor local do `setup`: `onDidInstallExtensions`, `onDidUninstallExtension`,
/// `onDidChangeProfile`, `getInstalled` e `getExtensionsControlManifest`, com as listas `installed` e `malicious` do caso.
pub struct LocalServerExtensionManagementService {
    pub did_install: Rc<Emitter<Vec<InstallExtensionResult>>>,
    pub did_uninstall: Rc<Emitter<DidUninstallExtensionEvent>>,
    pub did_change_profile: Rc<Emitter<DidChangeProfileEvent>>,
    pub installed: Rc<RefCell<Vec<ExtensionRef>>>,
    pub malicious: Rc<RefCell<Vec<ExtensionIdentifierInfo>>>,
}

impl IExtensionManagementService for LocalServerExtensionManagementService {
    fn on_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>> {
        self.did_install.event()
    }

    fn on_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent> {
        self.did_uninstall.event()
    }

    fn get_installed(&self) -> Promise<Vec<ExtensionRef>> {
        let installed = self.installed.clone();
        // `getInstalled: () => Promise.resolve(installed)`: a lista é lida quando quem espera continua.
        Promise::resolved(()).then(move |()| installed.borrow().clone())
    }

    fn get_extensions_control_manifest(&self) -> Promise<ExtensionsControlManifest> {
        Promise::resolved(ExtensionsControlManifest {
            malicious: self
                .malicious
                .borrow()
                .iter()
                .map(|extension| MaliciousExtensionInfo { extension_or_publisher: ExtensionOrPublisher::Extension(extension.clone()), learn_more_link: None })
                .collect(),
            ..ExtensionsControlManifest::default()
        })
    }
}

impl IProfileAwareExtensionManagementService for LocalServerExtensionManagementService {
    fn on_profile_aware_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>> {
        Event::none()
    }

    fn on_profile_aware_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent> {
        Event::none()
    }

    fn on_did_change_profile(&self) -> Event<DidChangeProfileEvent> {
        self.did_change_profile.event()
    }
}

/// `ExtensionManagementService` (o do workbench): junta os eventos e as listas dos servidores, e põe o `server` em cada
/// evento de remoção e de troca de perfil, como o real.
pub struct TestExtensionManagementService {
    servers: Vec<Rc<IExtensionManagementServer>>,
    server_service: Rc<dyn IExtensionManagementServerService>,
}

impl TestExtensionManagementService {
    pub fn new(server_service: Rc<dyn IExtensionManagementServerService>) -> Rc<Self> {
        let servers = [server_service.local_extension_management_server(), server_service.remote_extension_management_server(), server_service.web_extension_management_server()]
            .into_iter()
            .flatten()
            .collect();
        Rc::new(Self { servers, server_service })
    }
}

impl IExtensionManagementService for TestExtensionManagementService {
    fn on_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>> {
        any(self.servers.iter().map(|server| server.extension_management_service.on_did_install_extensions()).collect())
    }

    fn on_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent> {
        any(self
            .servers
            .iter()
            .map(|server| {
                let stamped = server.clone();
                server.extension_management_service.on_did_uninstall_extension().map(move |e| DidUninstallExtensionEvent { server: Some(stamped.clone()), ..e.clone() }, None)
            })
            .collect())
    }

    fn get_installed(&self) -> Promise<Vec<ExtensionRef>> {
        Promise::all(self.servers.iter().map(|server| server.extension_management_service.get_installed()).collect())
            .then(|installed_by_server| installed_by_server.into_iter().flatten().collect::<Vec<ExtensionRef>>())
    }

    fn get_extensions_control_manifest(&self) -> Promise<ExtensionsControlManifest> {
        let server = self
            .server_service
            .local_extension_management_server()
            .or_else(|| self.server_service.remote_extension_management_server())
            .or_else(|| self.server_service.web_extension_management_server());
        match server {
            Some(server) => server.extension_management_service.get_extensions_control_manifest(),
            None => Promise::resolved(ExtensionsControlManifest::default()),
        }
    }
}

impl IProfileAwareExtensionManagementService for TestExtensionManagementService {
    fn on_profile_aware_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>> {
        any(self.servers.iter().map(|server| server.extension_management_service.on_profile_aware_did_install_extensions()).collect())
    }

    fn on_profile_aware_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent> {
        any(self
            .servers
            .iter()
            .map(|server| {
                let stamped = server.clone();
                server
                    .extension_management_service
                    .on_profile_aware_did_uninstall_extension()
                    .map(move |e| DidUninstallExtensionEvent { server: Some(stamped.clone()), ..e.clone() }, None)
            })
            .collect())
    }

    fn on_did_change_profile(&self) -> Event<DidChangeProfileEvent> {
        any(self
            .servers
            .iter()
            .map(|server| {
                let stamped = server.clone();
                server.extension_management_service.on_did_change_profile().map(move |e| DidChangeProfileEvent { server: Some(stamped.clone()), ..e.clone() }, None)
            })
            .collect())
    }
}

impl IWorkbenchExtensionManagementService for TestExtensionManagementService {
    fn get_installed_workspace_extensions(&self, _include_invalid: bool) -> Promise<Vec<ExtensionRef>> {
        Promise::resolved(Vec::new())
    }
}

/// O `{ isEnabled() { return enabled; } }` do `IUserDataSyncEnablementService`.
pub struct StubUserDataSyncEnablementService {
    pub enabled: bool,
}

impl IUserDataSyncEnablementService for StubUserDataSyncEnablementService {
    fn on_did_change_enablement(&self) -> Event<bool> {
        Event::none()
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn can_toggle_enablement(&self) -> bool {
        false
    }

    fn set_enablement(&self, _enabled: bool) {}

    fn on_did_change_resource_enablement(&self) -> Event<(SyncResource, bool)> {
        Event::none()
    }

    fn is_resource_enabled(&self, _resource: SyncResource, _default_value: Option<bool>) -> bool {
        false
    }

    fn set_resource_enablement(&self, _resource: SyncResource, _enabled: bool) {}

    fn get_resource_sync_state_version(&self, _resource: SyncResource) -> Option<String> {
        None
    }

    fn is_resource_enablement_configured(&self, _resource: SyncResource) -> bool {
        false
    }
}

/// O `{ account: { authenticationProviderId } }` do `IUserDataSyncAccountService`.
pub struct StubUserDataSyncAccountService {
    pub account: Option<IUserDataSyncAccount>,
}

impl StubUserDataSyncAccountService {
    pub fn with_provider(authentication_provider_id: &str) -> Rc<Self> {
        Rc::new(Self { account: Some(IUserDataSyncAccount { authentication_provider_id: authentication_provider_id.to_owned(), token: String::new() }) })
    }
}

impl IUserDataSyncAccountService for StubUserDataSyncAccountService {
    fn on_token_failed(&self) -> Event<bool> {
        Event::none()
    }

    fn account(&self) -> Option<IUserDataSyncAccount> {
        self.account.clone()
    }

    fn on_did_change_account(&self) -> Event<Option<IUserDataSyncAccount>> {
        Event::none()
    }

    fn update_account(&self, _account: Option<IUserDataSyncAccount>) -> Promise<()> {
        Promise::resolved(())
    }
}

/// O `IDefaultAccountService` do `TestExtensionEnablementService`: o provedor `default-auth-provider`. Os demais membros
/// não existem no objeto do original, e chamá-los lança.
pub struct StubDefaultAccountService;

fn not_implemented() -> ! {
    errors::throw(Error::new("not implemented"))
}

impl IDefaultAccountService for StubDefaultAccountService {
    fn get_default_account_authentication_provider(&self) -> IDefaultAccountAuthenticationProvider {
        IDefaultAccountAuthenticationProvider { id: "default-auth-provider".to_owned(), name: "default-auth-provider".to_owned(), enterprise: false, extra: Default::default() }
    }

    fn on_did_change_default_account(&self) -> Event<Option<IDefaultAccount>> {
        not_implemented()
    }

    fn on_did_change_policy_data(&self) -> Event<Option<IPolicyData>> {
        not_implemented()
    }

    fn policy_data(&self) -> Option<IPolicyData> {
        not_implemented()
    }

    fn current_default_account(&self) -> Option<IDefaultAccount> {
        not_implemented()
    }

    fn copilot_token_info(&self) -> Option<ICopilotTokenInfo> {
        not_implemented()
    }

    fn on_did_change_copilot_token_info(&self) -> Event<Option<ICopilotTokenInfo>> {
        not_implemented()
    }

    fn managed_settings_fetch_status(&self) -> Option<ManagedSettingsFetchStatus> {
        not_implemented()
    }

    fn managed_settings_fetched_at(&self) -> Option<f64> {
        not_implemented()
    }

    fn managed_settings_raw_response(&self) -> Dynamic {
        not_implemented()
    }

    fn managed_settings_compatibility_error(&self) -> Option<IManagedSettingsCompatibilityError> {
        not_implemented()
    }

    fn on_did_change_managed_settings_compatibility_error(&self) -> Event<Option<IManagedSettingsCompatibilityError>> {
        not_implemented()
    }

    fn managed_settings_freshness(&self) -> IManagedSettingsFreshness {
        not_implemented()
    }

    fn on_did_change_managed_settings_freshness(&self) -> Event<IManagedSettingsFreshness> {
        not_implemented()
    }

    fn get_default_account(&self) -> Promise<Option<IDefaultAccount>> {
        not_implemented()
    }

    fn set_default_account_provider(&self, _provider: Rc<dyn IDefaultAccountProvider>) -> Result<(), Error> {
        not_implemented()
    }

    fn refresh(&self, _options: Option<IDefaultAccountRefreshOptions>) -> Promise<Option<IDefaultAccount>> {
        not_implemented()
    }

    fn sign_in(&self, _options: Option<ISignInOptions>) -> Promise<Option<IDefaultAccount>> {
        not_implemented()
    }

    fn sign_out(&self) -> Promise<()> {
        not_implemented()
    }

    fn resolve_git_hub_url(&self, _path: &str) -> Option<String> {
        not_implemented()
    }
}

/// O `IExtensionBisectService` com `isDisabledByBisect() { return false; }`.
pub struct StubExtensionBisectService;

impl IExtensionBisectService for StubExtensionBisectService {
    fn is_disabled_by_bisect(&self, _extension: &IExtension) -> bool {
        false
    }
}

/// O `get(...) || stub(...)` do `TestInstantiationService`: o serviço registrado, ou um novo que passa a ficar registrado.
pub fn get_or_stub<T: ?Sized>(slot: &RefCell<Option<Rc<T>>>, create: impl FnOnce() -> Rc<T>) -> Rc<T> {
    let existing = slot.borrow().clone();
    if let Some(existing) = existing {
        return existing;
    }
    let created = create();
    *slot.borrow_mut() = Some(created.clone());
    created
}

/// Os serviços que o construtor de `TestExtensionEnablementService` lê do `instantiationService`, já resolvidos (os que
/// o original lê com `get(...) || stub(...)` chegam aqui no estado em que o registro os deixou).
pub struct EnablementServices {
    pub storage: Rc<AbstractStorageService>,
    pub context: Rc<dyn IWorkspaceContextService>,
    pub environment: Rc<dyn IWorkbenchEnvironmentService>,
    /// O `IWorkbenchExtensionManagementService` registrado.
    pub extension_management_service: Rc<dyn IWorkbenchExtensionManagementService>,
    pub configuration: Rc<dyn IConfigurationService>,
    pub server_service: Rc<dyn IExtensionManagementServerService>,
    pub user_data_sync_enablement_service: Rc<dyn IUserDataSyncEnablementService>,
    pub default_account_service: Rc<dyn IDefaultAccountService>,
    pub user_data_sync_account_service: Rc<dyn IUserDataSyncAccountService>,
    pub lifecycle_service: Rc<dyn ILifecycleService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub host_service: Rc<dyn IHostService>,
    pub workspace_trust_management_service: Rc<dyn IWorkspaceTrustManagementService>,
    pub extension_manifest_properties_service: Rc<ExtensionManifestPropertiesService>,
    pub chat_entitlement_service: Rc<dyn IChatEntitlementService>,
}

/// `TestInstantiationService` com o `setup` da suíte: os serviços registrados, as listas `installed` e `malicious`, e os
/// emissores `didInstallEvent`, `didUninstallEvent` e `didChangeProfileExtensionsEvent`.
pub struct Fixture {
    /// `disposableStore`.
    pub guard: LeakGuard,
    pub did_install_event: Rc<Emitter<Vec<InstallExtensionResult>>>,
    pub did_uninstall_event: Rc<Emitter<DidUninstallExtensionEvent>>,
    pub did_change_profile_extensions_event: Rc<Emitter<DidChangeProfileEvent>>,
    pub installed: Rc<RefCell<Vec<ExtensionRef>>>,
    pub malicious: Rc<RefCell<Vec<ExtensionIdentifierInfo>>>,
    pub configuration: Rc<TestConfigurationService>,
    pub context: Rc<TestContextService>,
    pub environment: RefCell<Rc<dyn IWorkbenchEnvironmentService>>,
    pub server_service: RefCell<Rc<dyn IExtensionManagementServerService>>,
    /// `stub(IWorkbenchExtensionManagementService, createInstance(ExtensionManagementService))`.
    pub workbench_extension_management_service: Rc<TestExtensionManagementService>,
    pub storage: RefCell<Option<Rc<AbstractStorageService>>>,
    pub workspace_trust_management_service: RefCell<Option<Rc<dyn IWorkspaceTrustManagementService>>>,
    pub user_data_sync_enablement_service: RefCell<Option<Rc<dyn IUserDataSyncEnablementService>>>,
    pub user_data_sync_account_service: RefCell<Option<Rc<dyn IUserDataSyncAccountService>>>,
    pub default_account_service: RefCell<Option<Rc<dyn IDefaultAccountService>>>,
    pub lifecycle_service: RefCell<Option<Rc<dyn ILifecycleService>>>,
    pub manifest_properties_service: RefCell<Option<Rc<ExtensionManifestPropertiesService>>>,
}

impl Fixture {
    /// O `setup`, até antes de criar o `testObject`.
    pub fn new() -> Rc<Self> {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration = TestConfigurationService::new(None);
        configuration.set_user_configuration(ALLOWED_EXTENSIONS_CONFIG_KEY, dynamic_of(&json!({ "*": true, "unallowed": false })), None);
        let did_install_event = Emitter::new();
        let did_uninstall_event = Emitter::new();
        let did_change_profile_extensions_event = Emitter::new();
        let installed = Rc::new(RefCell::new(Vec::new()));
        let malicious = Rc::new(RefCell::new(Vec::new()));
        let local_server = Rc::new(IExtensionManagementServer {
            id: "local".to_owned(),
            label: "local".to_owned(),
            extension_management_service: Rc::new(LocalServerExtensionManagementService {
                did_install: did_install_event.clone(),
                did_uninstall: did_uninstall_event.clone(),
                did_change_profile: did_change_profile_extensions_event.clone(),
                installed: installed.clone(),
                malicious: malicious.clone(),
            }),
        });
        let server_service = an_extension_management_server_service(Some(local_server), None, None);
        let workbench_extension_management_service = TestExtensionManagementService::new(server_service.clone());
        Rc::new(Self {
            guard,
            did_install_event,
            did_uninstall_event,
            did_change_profile_extensions_event,
            installed,
            malicious,
            configuration,
            context: TestContextService::new(None, None),
            environment: RefCell::new(TestEnvironmentService::new()),
            server_service: RefCell::new(server_service),
            workbench_extension_management_service,
            storage: RefCell::new(None),
            workspace_trust_management_service: RefCell::new(None),
            user_data_sync_enablement_service: RefCell::new(None),
            user_data_sync_account_service: RefCell::new(None),
            default_account_service: RefCell::new(None),
            lifecycle_service: RefCell::new(None),
            manifest_properties_service: RefCell::new(None),
        })
    }

    /// `anExtensionManagementServer(authority, instantiationService)`: o serviço do servidor é o
    /// `IExtensionManagementService` registrado (o do workbench do `setup`).
    pub fn an_extension_management_server(&self, authority: &str) -> Rc<IExtensionManagementServer> {
        Rc::new(IExtensionManagementServer { id: authority.to_owned(), label: authority.to_owned(), extension_management_service: self.workbench_extension_management_service.clone() })
    }

    /// `aMultiExtensionManagementServerService(instantiationService)`.
    pub fn a_multi_extension_management_server_service(&self) -> Rc<dyn IExtensionManagementServerService> {
        an_extension_management_server_service(Some(self.an_extension_management_server("vscode-local")), Some(self.an_extension_management_server("vscode-remote")), None)
    }

    /// `instantiationService.stub(IExtensionManagementServerService, service)`.
    pub fn stub_server_service(&self, service: Rc<dyn IExtensionManagementServerService>) {
        *self.server_service.borrow_mut() = service;
    }

    /// `instantiationService.stub(IWorkbenchEnvironmentService, { ... })`.
    pub fn stub_environment(&self, overrides: EnvironmentOverrides) {
        *self.environment.borrow_mut() = TestEnvironmentService::with_overrides(overrides);
    }

    /// `instantiationService.stub(IWorkspaceTrustManagementService, { isWorkspaceTrusted() { return trusted; } })`.
    pub fn stub_workspace_trusted(&self, trusted: bool) {
        let service = self.guard.add_rc(TestWorkspaceTrustManagementService::new(Some(trusted), None));
        *self.workspace_trust_management_service.borrow_mut() = Some(service);
    }

    /// `instantiationService.stub(IUserDataSyncEnablementService, { isEnabled() { return enabled; } })`.
    pub fn stub_user_data_sync_enabled(&self, enabled: bool) {
        *self.user_data_sync_enablement_service.borrow_mut() = Some(Rc::new(StubUserDataSyncEnablementService { enabled }));
    }

    /// `instantiationService.stub(IUserDataSyncAccountService, { account: { authenticationProviderId } })`.
    pub fn stub_user_data_sync_account(&self, authentication_provider_id: &str) {
        *self.user_data_sync_account_service.borrow_mut() = Some(StubUserDataSyncAccountService::with_provider(authentication_provider_id));
    }

    /// `createStorageService(instantiationService, disposables)`: o armazenamento registrado, ou um novo em memória.
    pub fn storage(&self) -> Rc<AbstractStorageService> {
        get_or_stub(&self.storage, || self.guard.add_rc(InMemoryStorageService::new()))
    }

    /// `new TestExtensionEnablementService(instantiationService, chatEntitlementService?)`, já no `disposableStore`.
    pub fn new_test_object(&self, chat_entitlement_service: Option<Rc<dyn IChatEntitlementService>>) -> Rc<TestExtensionEnablementService> {
        let storage = self.storage();
        let server_service = self.server_service.borrow().clone();
        let workspace_trust_management_service = get_or_stub(&self.workspace_trust_management_service, || {
            let service: Rc<TestWorkspaceTrustManagementService> = self.guard.add_rc(TestWorkspaceTrustManagementService::new(None, None));
            service as Rc<dyn IWorkspaceTrustManagementService>
        });
        let manifest_properties_service = get_or_stub(&self.manifest_properties_service, || {
            self.guard.add_rc(ExtensionManifestPropertiesService::new(
                Rc::new(TestProductService::default()),
                self.configuration.clone(),
                TestWorkspaceTrustEnablementService::new(true),
                NullLogService::shared(),
            ))
        });
        let chat_entitlement_service = chat_entitlement_service.unwrap_or_else(|| {
            let service: Rc<dyn IChatEntitlementService> = TestChatEntitlementService::new();
            service
        });
        let services = EnablementServices {
            storage,
            context: self.context.clone(),
            environment: self.environment.borrow().clone(),
            extension_management_service: self.workbench_extension_management_service.clone(),
            configuration: self.configuration.clone(),
            server_service,
            user_data_sync_enablement_service: get_or_stub(&self.user_data_sync_enablement_service, || Rc::new(StubUserDataSyncEnablementService { enabled: false })),
            default_account_service: get_or_stub(&self.default_account_service, || Rc::new(StubDefaultAccountService)),
            user_data_sync_account_service: get_or_stub(&self.user_data_sync_account_service, || Rc::new(StubUserDataSyncAccountService { account: None })),
            lifecycle_service: get_or_stub(&self.lifecycle_service, || -> Rc<dyn ILifecycleService> {
                let service: Rc<TestLifecycleService> = self.guard.add_rc(TestLifecycleService::new());
                service
            }),
            notification_service: Rc::new(TestNotificationService),
            host_service: Rc::new(TestHostService),
            workspace_trust_management_service,
            extension_manifest_properties_service: manifest_properties_service,
            chat_entitlement_service,
        };
        TestExtensionEnablementService::create(&self.guard, services)
    }
}

/// `TestExtensionEnablementService`: o serviço com `waitUntilInitialized` e `reset`.
pub struct TestExtensionEnablementService {
    pub inner: Rc<ExtensionEnablementService>,
    /// O `AllowedExtensionsService` que o construtor do original registra no `instantiationService`.
    pub allowed_extensions_service: Rc<AllowedExtensionsService>,
}

impl Deref for TestExtensionEnablementService {
    type Target = ExtensionEnablementService;

    fn deref(&self) -> &ExtensionEnablementService {
        &self.inner
    }
}

impl IDisposable for TestExtensionEnablementService {
    fn dispose(&self) {
        self.inner.dispose();
    }
}

impl TestExtensionEnablementService {
    /// `new TestExtensionEnablementService(instantiationService, chatEntitlementService?)`, já no `disposableStore`.
    pub fn create(guard: &LeakGuard, services: EnablementServices) -> Rc<Self> {
        let storage: Rc<dyn StorageService> = Rc::new(services.storage.clone());
        // `createInstance(ExtensionManagementService)`, com o servidor registrado agora.
        let extension_management_service = TestExtensionManagementService::new(services.server_service.clone());
        let global_extension_enablement_service = guard.add_rc(GlobalExtensionEnablementService::new(storage.clone(), extension_management_service));
        let configuration = services.configuration.clone();
        let allowed_extensions_service = Rc::new(AllowedExtensionsService::new(&[], {
            let configuration = configuration.clone();
            move || json_stringify(&configuration.get_value(Some(ALLOWED_EXTENSIONS_CONFIG_KEY), None)).and_then(|text| serde_json::from_str::<Value>(&text).ok())
        }));
        let inner = ExtensionEnablementService::new(ExtensionEnablementServiceParts {
            storage_service: storage,
            global_extension_enablement_service,
            context_service: services.context,
            environment_service: services.environment,
            extension_management_service: services.extension_management_service,
            configuration_service: configuration,
            extension_management_server_service: services.server_service,
            user_data_sync_enablement_service: services.user_data_sync_enablement_service,
            default_account_service: services.default_account_service,
            user_data_sync_account_service: services.user_data_sync_account_service,
            lifecycle_service: services.lifecycle_service,
            notification_service: services.notification_service,
            host_service: services.host_service,
            extension_bisect_service: Rc::new(StubExtensionBisectService),
            allowed_extensions_service: allowed_extensions_service.clone(),
            workspace_trust_management_service: services.workspace_trust_management_service,
            workspace_trust_request_service: {
                let service: Rc<TestWorkspaceTrustRequestService> = guard.add_rc(TestWorkspaceTrustRequestService::new(true));
                service
            },
            extension_manifest_properties_service: services.extension_manifest_properties_service,
            chat_entitlement_service: services.chat_entitlement_service,
            log_service: NullLogService::shared(),
            product_service: Rc::new(product_service()),
        });
        guard.add_rc(Rc::new(Self { inner, allowed_extensions_service }))
    }

    /// O serviço como o contrato `IWorkbenchExtensionEnablementService` (o que o `instantiationService` entrega).
    pub fn service(&self) -> Rc<dyn IWorkbenchExtensionEnablementService> {
        self.inner.clone()
    }

    /// `async waitUntilInitialized()`.
    pub fn wait_until_initialized(&self) -> Promise<()> {
        let initialized = self.extensions_manager.when_initialized();
        je_base::async_::async_fn(async move {
            initialized.await?;
            Ok(())
        })
    }

    /// `reset()`: habilita de volta o que está desabilitado.
    pub fn reset(&self) -> Vec<Promise<Vec<bool>>> {
        let mut extensions = self.global_extension_enablement_service.get_disabled_extensions();
        for e in self.get_workspace_disabled_extensions() {
            if !extensions.iter().any(|r| je_extensions::util::are_same_extensions(r, &e)) {
                extensions.push(e);
            }
        }
        let workspace_enabled_extensions = self.get_workspace_enabled_extensions();
        if !workspace_enabled_extensions.is_empty() {
            extensions.retain(|r| !workspace_enabled_extensions.iter().any(|e| je_extensions::util::are_same_extensions(e, r)));
        }
        extensions.iter().map(|d| self.set_enablement(vec![a_local_extension(&d.id)], EnablementState::EnabledGlobally)).collect()
    }
}
