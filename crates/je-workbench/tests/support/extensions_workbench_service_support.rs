//! Dublês de serviço e construtores de dados do `extensionsWorkbenchService.test.ts` (suíte `ExtensionsWorkbenchServiceTest`).
//!
//! Notas de tradução:
//! - `TestInstantiationService` (`stub`, `stubPromise`, `get`, `createInstance`) é o [`WorkbenchFixture`]: ele guarda o que o
//!   `setup` registra e que os casos trocam no meio (`stubPromise(IExtensionManagementService, 'getInstalled', [...])` é
//!   [`StubExtensionManagementService::stub_installed`], `stubPromise(IExtensionGalleryService, 'query', aPage(...))` é
//!   [`StubGalleryService::stub_query`], `stubConfiguration(...)` é [`WorkbenchFixture::stub_configuration`]...).
//!   `createInstance(ExtensionsWorkbenchService)` monta o [`ExtensionsWorkbenchServiceParts`] com o que estiver registrado
//!   naquele momento.
//! - **Um serviço só para `IExtensionManagementService` e `IWorkbenchExtensionManagementService`.** Os dois decoradores têm o
//!   mesmo identificador no original; o literal do `setup` é o [`StubExtensionManagementService`], que também é o serviço
//!   do servidor local (`instantiationService.get(IExtensionManagementService)`). Os servidores de
//!   `aMultiExtensionManagementServerService` têm o seu próprio (`createExtensionManagementService`).
//! - **Mutação no lugar.** O `updateMetadata` do original altera o objeto `ILocalExtension` que a lista e a `Extension` já
//!   compartilham; a [`LocalExtension`] do porte é imutável dentro do `Rc`. O `updateMetadata` padrão devolve a cópia
//!   alterada e a põe no lugar da antiga na lista (a lista é a mesma de antes, com o campo novo); o que o caso troca
//!   ([`StubExtensionManagementService::stub_update_metadata`], que faz `local.pinned = !!metadata.pinned`) também
//!   dispara `onDidUpdateExtensionMetadata`, que é como a `Extension` passa a ver a cópia (o literal do original tem
//!   `Event.None` porque a mutação já chega nela).
//! - **`undefined` vs `false`.** `ILocalExtension.pinned` é `undefined` nas extensões sem o campo; no porte é `bool`, e os
//!   `assert.strictEqual(x.local?.pinned, undefined)` viram `false`.
//! - **Serviços que o `setup` não registra** (`IDialogService`, `IUserDataProfilesService`, `IUriIdentityService`,
//!   `IIgnoredExtensionsManagementService`, `IUserDataAutoSyncService`, `IUserDataSyncEnablementService`, `IHostService`
//!   quando nenhum caso o troca) chegam `undefined` ao construtor, e nenhum caso chama os que o original chama de fato.
//!   Aqui são dublês inertes; o [`StubDialogService`] sem resposta rejeita a chamada com o `TypeError` que o original
//!   lança, e é o que faz os casos de desabilitar com dependentes falharem sem o `prompt` do `INotificationService`.
//! - **`IProgressService`.** O `stub(IProgressService, ProgressService)` do original é uma instância esvaziada, em que
//!   `withProgress` devolve `undefined` e a tarefa já foi iniciada por quem chamou. O porte inicia a tarefa dentro do
//!   callback de progresso, então o dublê ([`RunningProgressService`]) a executa e devolve a promessa dela.
//! - **`uninstallExtensions`** não existe no literal do `setup` (chamá-lo lança `TypeError` e rejeita a promessa solta de
//!   `uninstall`, que o mocha ignora). No porte uma rejeição solta é erro inesperado e reprova o caso, então o dublê o
//!   resolve sem fazer nada; os casos disparam os eventos de remoção à mão.
//! - **`stubConfiguration`.** O literal responde `affectsConfiguration(...)` com `true` para qualquer chave; o
//!   [`ConfigurationChangeEvent`] real responde pelas chaves afetadas, então o evento lista as quatro que a produção
//!   consulta (`extensions.autoUpdate`, `extensions.autoUpdateDelay`, `extensions.autoCheckUpdates` e
//!   `extensions.autoRestart`).
//! - **Laço de eventos.** `run` roda as microtarefas e só os timers já vencidos ([`runtime::turn`]), como o mocha que
//!   espera uma promessa: um timer de 0 ms (a verificação de atualizações do `initialize`) dispara depois que as
//!   microtarefas esvaziam, e os de horas (o intervalo de verificação) nunca vencem dentro de um caso.
//! - `aLocalExtension(name, manifest, properties)` põe `isBuiltin` ausente (falso, mesmo com `type: System`) e o
//!   `targetPlatform` ausente ([`TargetPlatform::Undefined`]); `location` é a URI `pub.<name>`, guardada como caminho (ou
//!   como URI completa quando o esquema não é `file`).

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::rc::Rc;

use je_base::JsString;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::map::ResourceMap;
use je_base::network::schemas;
use je_base::paging::{IPager, single_page_pager};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_extensions::management::{
    ExtensionInfo, ExtensionQueryOptions, ExtensionsControlManifest, GalleryExtension, GalleryExtensionAsset, GalleryExtensionAssets, GalleryExtensionProperties,
    GalleryExtensionVersion, HostPlatform, InstallOperation, InstallOptions, InstallSource, LocalExtension, Metadata, ProductVersion, QueryOptions, StatisticType,
    UninstallOptions, current_arch, get_target_platform,
};
use je_extensions::manifest::{ExtensionIdentifier, ExtensionIdentifierInfo, ExtensionManifest, ExtensionType, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_extensions::util::get_gallery_extension_id;
use je_platform::configuration::models::{Configuration, ConfigurationChangeEvent, ConfigurationKeys, ConfigurationModel};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationValue;
use je_platform::configuration::{
    ConfigurationChange, ConfigurationData, ConfigurationOverrides, ConfigurationTarget, ConfigurationUpdateOverrides, ConfigurationValue,
};
use je_platform::dialogs::dialogs::{Confirmation, ConfirmationResult, IDialogService, Input, InputResult, Prompt, PromptResult};
use je_platform::files::i_file_service::IFileService;
use je_platform::log::NullLogService;
use je_platform::metered_connection::IMeteredConnectionService;
use je_platform::product::{IProductService, ProductService};
use je_platform::progress::progress::{IProgress, IProgressService, Progress, ProgressOptions, ProgressStep, ProgressTask};
use je_platform::resources::{ext_uri, uri_file};
use je_platform::storage::{AbstractStorageService, InMemoryStorageService};
use je_platform::telemetry::telemetry_utils::NullTelemetryServiceShape;
use je_platform::update::update::{State, UpdateService};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::user_data_profile::{
    DidChangeProfilesEvent, IUserDataProfilesService, UserDataProfile, UserDataProfileOptions, UserDataProfileUpdateOptions, to_user_data_profile,
};
use je_platform::user_data_sync::user_data_sync::{IUserDataAutoSyncService, SyncOptions};
use je_platform::workspace::AnyWorkspaceIdentifier;
use je_workbench::contrib::extensions::extensions_service::{ExtensionsWorkbenchService, ExtensionsWorkbenchServiceParts, IIgnoredExtensionsManagementService};
use je_workbench::services::environment::environment_service::IWorkbenchEnvironmentService;
use je_workbench::services::extension_management::extension_install_service::{
    DidUpdateExtensionMetadata, IExtensionInstallService, IExtensionManagementParticipant, IWorkbenchExtensionInstallService, InstallExtensionEvent,
    InstallExtensionInfo, UninstallExtensionEvent, UninstallExtensionInfo,
};
use je_workbench::services::extension_management::extension_gallery_service::IExtensionGalleryService;
use je_workbench::services::extension_management::extension_management::{
    DidChangeProfileEvent, DidUninstallExtensionEvent, ExtensionRef, IExtensionManagementServer, IExtensionManagementServerService, IExtensionManagementService,
    IProfileAwareExtensionManagementService, IResourceExtension, IWorkbenchExtensionEnablementService, IWorkbenchExtensionManagementService,
    InstallExtensionResult,
};
use je_workbench::services::extensions::extensions::{ActivationKind, IExtensionDescription, IExtensionService};
use je_workbench::services::extensions::extension_manifest_properties_service::ExtensionManifestPropertiesService;
use je_workbench::services::host::host::IHostService;
use je_workbench::services::lifecycle::lifecycle::ILifecycleService;
use je_workbench::services::user_data_profile::user_data_profile::IUserDataProfileService;
use je_workbench::services::user_data_profile::user_data_profile_service::UserDataProfileService;
use je_workbench::test::browser::workbench_test_services::{TestEnvironmentService, TestHostService};
use je_workbench::test::common::workbench_test_services::{
    TestContextService, TestFileService, TestLifecycleService, TestNotificationService, TestProductService, TestWorkspaceTrustEnablementService,
    TestWorkspaceTrustManagementService,
};
use je_instantiation::Injectable;
use serde_json::{Map, Value, json};

use super::extension_enablement_support::common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use super::extension_enablement_support::{
    EnablementServices, StubDefaultAccountService, StubUserDataSyncAccountService, StubUserDataSyncEnablementService, TestExtensionEnablementService,
    an_extension_management_server_service, dynamic_of,
};
use super::extension_services_support::extension_services;

/// `new Error(...)` de uma chamada a algo que o objeto do original não tem (o `TypeError` de chamar `undefined`).
fn not_a_function<T: Clone + 'static>(name: &str) -> Promise<T> {
    Promise::rejected(Error::new(format!("{name} is not a function")))
}

/// `getTargetPlatform(platform, arch)`.
pub fn host_target_platform() -> TargetPlatform {
    get_target_platform(HostPlatform::current(), Some(current_arch()))
}

// ---------------------------------------------------------------------------------------------------------------------
// Extensões locais e da galeria.
// ---------------------------------------------------------------------------------------------------------------------

/// O `properties` de `aLocalExtension(name, manifest, properties)`: o que os casos trocam.
#[derive(Default)]
pub struct LocalProperties {
    pub extension_type: Option<ExtensionType>,
    pub location: Option<URI>,
    pub identifier: Option<ExtensionIdentifierInfo>,
    pub pinned: bool,
    pub readme_url: Option<&'static str>,
    pub changelog_url: Option<&'static str>,
}

/// `aLocalExtension(name, manifest, properties)`.
pub fn a_local_extension_with(name: &str, manifest: Value, properties: LocalProperties) -> ExtensionRef {
    let Value::Object(patch) = manifest else { panic!("o manifesto é um objeto") };
    let mut manifest = Map::new();
    manifest.insert("name".to_owned(), json!(name));
    manifest.insert("publisher".to_owned(), json!("pub"));
    manifest.insert("version".to_owned(), json!("1.0.0"));
    manifest.extend(patch);
    let publisher = manifest.get("publisher").and_then(Value::as_str).unwrap_or_default().to_owned();
    let manifest_name = manifest.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
    let identifier = properties.identifier.unwrap_or_else(|| ExtensionIdentifierInfo::new(get_gallery_extension_id(Some(&publisher), &manifest_name)));
    let location = properties.location.unwrap_or_else(|| uri_file(&format!("pub.{name}")));
    let location = if location.scheme().to_string_lossy() == schemas::FILE { location.path().to_string_lossy() } else { location.to_string(true).to_string_lossy() };
    Rc::new(LocalExtension {
        extension_type: properties.extension_type.unwrap_or(ExtensionType::User),
        is_builtin: false,
        identifier,
        manifest: ExtensionManifest::from_value(Value::Object(manifest)),
        location,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        readme_url: properties.readme_url.map(str::to_owned),
        changelog_url: properties.changelog_url.map(str::to_owned),
        is_valid: true,
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
        pinned: properties.pinned,
        force_auto_update: false,
        source: InstallSource::Vsix,
        size: 0.0,
    })
}

/// `aLocalExtension(name)`.
pub fn a_local_extension(name: &str) -> ExtensionRef {
    a_local_extension_with(name, json!({}), LocalProperties::default())
}

/// O `properties` de `aGalleryExtension(name, properties, ...)`: o que os casos trocam.
#[derive(Default)]
pub struct GalleryProperties<'a> {
    pub display_name: Option<&'a str>,
    pub version: Option<&'a str>,
    pub publisher_id: Option<&'a str>,
    pub publisher: Option<&'a str>,
    pub publisher_display_name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub install_count: Option<f64>,
    pub rating: Option<f64>,
    pub rating_count: Option<f64>,
    pub last_updated: Option<f64>,
    pub is_signed: Option<bool>,
}

/// `{ uri, fallbackUri }`.
pub fn asset(uri: &str, fallback_uri: &str) -> GalleryExtensionAsset {
    GalleryExtensionAsset { uri: uri.to_owned(), fallback_uri: fallback_uri.to_owned() }
}

/// `noAssets`.
pub fn no_assets() -> GalleryExtensionAssets {
    GalleryExtensionAssets {
        manifest: None,
        readme: None,
        changelog: None,
        license: None,
        repository: None,
        download: asset("", ""),
        icon: None,
        signature: None,
        core_translations: Vec::new(),
    }
}

/// `aGalleryExtension(name, properties, { dependencies }, assets)`: `dependencies` é `None` para o padrão (`[]`).
pub fn a_gallery_extension_with(name: &str, properties: GalleryProperties, dependencies: Option<&[&str]>, assets: Option<GalleryExtensionAssets>) -> GalleryExtension {
    let target_platform = host_target_platform();
    let publisher = properties.publisher.unwrap_or("pub");
    GalleryExtension {
        name: name.to_owned(),
        identifier: ExtensionIdentifierInfo { id: get_gallery_extension_id(Some(publisher), name), uuid: Some(generate_uuid()) },
        version: properties.version.unwrap_or("1.0.0").to_owned(),
        display_name: properties.display_name.unwrap_or_default().to_owned(),
        publisher_id: properties.publisher_id.unwrap_or_default().to_owned(),
        publisher: publisher.to_owned(),
        publisher_display_name: properties.publisher_display_name.unwrap_or_default().to_owned(),
        publisher_domain: None,
        publisher_link: None,
        publisher_sponsor_link: None,
        description: properties.description.unwrap_or_default().to_owned(),
        install_count: properties.install_count.unwrap_or(0.0),
        rating: properties.rating.unwrap_or(0.0),
        rating_count: properties.rating_count.unwrap_or(0.0),
        categories: Vec::new(),
        tags: Vec::new(),
        release_date: 0.0,
        last_updated: properties.last_updated.unwrap_or(0.0),
        preview: false,
        private: false,
        blocking_info: None,
        has_pre_release_version: false,
        has_release_version: false,
        is_signed: properties.is_signed.unwrap_or(true),
        all_target_platforms: vec![target_platform],
        assets: assets.unwrap_or_else(no_assets),
        properties: GalleryExtensionProperties {
            dependencies: Some(dependencies.map_or_else(Vec::new, |list| list.iter().map(|id| (*id).to_owned()).collect())),
            extension_pack: None,
            engine: None,
            enabled_api_proposals: None,
            localized_languages: None,
            target_platform,
            is_pre_release_version: false,
            executes_code: None,
        },
        details_link: None,
        rating_link: None,
        support_link: None,
    }
}

/// `aGalleryExtension(name)`.
pub fn a_gallery_extension(name: &str) -> GalleryExtension {
    a_gallery_extension_with(name, GalleryProperties::default(), None, None)
}

/// `aGalleryExtension(name, properties)`.
pub fn a_gallery_extension_props(name: &str, properties: GalleryProperties) -> GalleryExtension {
    a_gallery_extension_with(name, properties, None, None)
}

/// O `runningExtension` dos casos de aviso de reinício: a extensão em execução que corresponde a `local`.
pub fn a_running_extension(local: &LocalExtension, is_under_development: bool) -> ExtensionDescription {
    let manifest = ExtensionManifest::from_value(json!({
        "name": "my_extension",
        "publisher": "my_publisher",
        "version": "1.0.0",
        "engines": { "vscode": "*" },
    }));
    ExtensionDescription {
        id: "my_publisher.my_extension".to_owned(),
        identifier: ExtensionIdentifier::new("my_publisher.my_extension"),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development,
        extension_location: local.location.clone(),
        uuid: None,
        target_platform: host_target_platform(),
        publisher_display_name: None,
        pre_release: false,
        manifest,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// IExtensionManagementService e IWorkbenchExtensionManagementService.
// ---------------------------------------------------------------------------------------------------------------------

/// O que um caso troca em `updateMetadata`: recebe a extensão e o `metadata`, devolve a extensão alterada.
pub type MetadataUpdater = Rc<dyn Fn(&LocalExtension, &Metadata) -> LocalExtension>;

/// O literal do `IWorkbenchExtensionManagementService` do `setup` e o `createExtensionManagementService(installed)` dos
/// servidores de `aMultiExtensionManagementServerService`.
pub struct StubExtensionManagementService {
    /// `installEvent`.
    pub install_event: Rc<Emitter<InstallExtensionEvent>>,
    /// `didInstallEvent`.
    pub did_install_event: Rc<Emitter<Vec<InstallExtensionResult>>>,
    /// `uninstallEvent`.
    pub uninstall_event: Rc<Emitter<UninstallExtensionEvent>>,
    /// `didUninstallEvent`.
    pub did_uninstall_event: Rc<Emitter<DidUninstallExtensionEvent>>,
    did_update_metadata_event: Rc<Emitter<DidUpdateExtensionMetadata>>,
    installed: RefCell<Vec<ExtensionRef>>,
    update_metadata: RefCell<Option<MetadataUpdater>>,
}

impl StubExtensionManagementService {
    /// `createExtensionManagementService(installed)`: os emissores entram no `disposableStore`.
    pub fn new(guard: &LeakGuard, installed: Vec<ExtensionRef>) -> Rc<Self> {
        Rc::new(Self {
            install_event: guard.add_rc(Emitter::new()),
            did_install_event: guard.add_rc(Emitter::new()),
            uninstall_event: guard.add_rc(Emitter::new()),
            did_uninstall_event: guard.add_rc(Emitter::new()),
            did_update_metadata_event: guard.add_rc(Emitter::new()),
            installed: RefCell::new(installed),
            update_metadata: RefCell::new(None),
        })
    }

    /// `instantiationService.stubPromise(IExtensionManagementService, 'getInstalled', installed)`.
    pub fn stub_installed(&self, installed: Vec<ExtensionRef>) {
        *self.installed.borrow_mut() = installed;
    }

    /// `instantiationService.stub(IExtensionManagementService, 'updateMetadata', fn)`.
    pub fn stub_update_metadata(&self, updater: impl Fn(&LocalExtension, &Metadata) -> LocalExtension + 'static) {
        *self.update_metadata.borrow_mut() = Some(Rc::new(updater));
    }
}

impl IExtensionManagementService for StubExtensionManagementService {
    fn on_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>> {
        self.did_install_event.event()
    }

    fn on_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent> {
        self.did_uninstall_event.event()
    }

    fn get_installed(&self) -> Promise<Vec<ExtensionRef>> {
        Promise::resolved(self.installed.borrow().clone())
    }

    fn get_extensions_control_manifest(&self) -> Promise<ExtensionsControlManifest> {
        Promise::resolved(ExtensionsControlManifest::default())
    }
}

impl IProfileAwareExtensionManagementService for StubExtensionManagementService {
    fn on_profile_aware_did_install_extensions(&self) -> Event<Vec<InstallExtensionResult>> {
        Event::none()
    }

    fn on_profile_aware_did_uninstall_extension(&self) -> Event<DidUninstallExtensionEvent> {
        Event::none()
    }

    fn on_did_change_profile(&self) -> Event<DidChangeProfileEvent> {
        Event::none()
    }

    fn as_install_service(self: Rc<Self>) -> Option<Rc<dyn IExtensionInstallService>> {
        Some(self)
    }
}

impl IWorkbenchExtensionManagementService for StubExtensionManagementService {
    fn get_installed_workspace_extensions(&self, _include_invalid: bool) -> Promise<Vec<ExtensionRef>> {
        Promise::resolved(Vec::new())
    }
}

impl IExtensionInstallService for StubExtensionManagementService {
    fn prefer_pre_releases(&self) -> bool {
        false
    }

    fn on_install_extension(&self) -> Event<InstallExtensionEvent> {
        self.install_event.event()
    }

    fn on_uninstall_extension(&self) -> Event<UninstallExtensionEvent> {
        self.uninstall_event.event()
    }

    fn on_did_update_extension_metadata(&self) -> Event<DidUpdateExtensionMetadata> {
        self.did_update_metadata_event.event()
    }

    fn zip(&self, _extension: &LocalExtension) -> Promise<URI> {
        not_a_function("zip")
    }

    fn get_manifest(&self, _vsix: &URI) -> Promise<Value> {
        not_a_function("getManifest")
    }

    fn install(&self, _vsix: &URI, _options: InstallOptions) -> Promise<ExtensionRef> {
        not_a_function("install")
    }

    /// `async canInstall() { return true; }`.
    fn can_install(&self, _extension: &GalleryExtension) -> Promise<Result<(), String>> {
        Promise::resolved(Ok(()))
    }

    /// `installFromGallery: (extension) => Promise.reject(new Error('not supported'))`.
    fn install_from_gallery(&self, _extension: &GalleryExtension, _options: InstallOptions) -> Promise<ExtensionRef> {
        Promise::rejected(Error::new("not supported"))
    }

    fn install_gallery_extensions(&self, _extensions: Vec<InstallExtensionInfo>) -> Promise<Vec<InstallExtensionResult>> {
        not_a_function("installGalleryExtensions")
    }

    fn install_from_location(&self, _location: &URI, _profile_location: &URI) -> Promise<ExtensionRef> {
        not_a_function("installFromLocation")
    }

    fn install_extensions_from_profile(&self, _extensions: Vec<ExtensionIdentifierInfo>, _from: &URI, _to: &URI) -> Promise<Vec<ExtensionRef>> {
        not_a_function("installExtensionsFromProfile")
    }

    fn uninstall(&self, _extension: &LocalExtension, _options: UninstallOptions) -> Promise<()> {
        not_a_function("uninstall")
    }

    /// Resolve sem fazer nada (ver a nota do módulo).
    fn uninstall_extensions(&self, _extensions: Vec<UninstallExtensionInfo>) -> Promise<()> {
        Promise::resolved(())
    }

    fn toggle_application_scope(&self, _extension: &LocalExtension, _from_profile_location: &URI) -> Promise<ExtensionRef> {
        not_a_function("toggleApplicationScope")
    }

    fn get_installed_filtered(
        &self,
        _extension_type: Option<ExtensionType>,
        _profile_location: Option<&URI>,
        _product_version: Option<ProductVersion>,
        _language: Option<&str>,
    ) -> Promise<Vec<ExtensionRef>> {
        self.get_installed()
    }

    fn copy_extensions(&self, _from_profile_location: &URI, _to_profile_location: &URI) -> Promise<()> {
        not_a_function("copyExtensions")
    }

    /// O `updateMetadata` do `setup`: `local.identifier.uuid = metadata.id`, `local.publisherDisplayName` e
    /// `local.publisherId`, ou o que o caso trocou.
    fn update_metadata(&self, local: &LocalExtension, metadata: Metadata, _profile_location: Option<&URI>) -> Promise<ExtensionRef> {
        let overridden = self.update_metadata.borrow().clone();
        let (updated, notify) = match overridden {
            Some(updater) => (Rc::new(updater(local, &metadata)), true),
            None => {
                let mut updated = local.clone();
                updated.identifier.uuid = metadata.id.clone();
                updated.publisher_display_name = metadata.publisher_display_name.clone();
                updated.publisher_id = metadata.publisher_id.clone();
                (Rc::new(updated), false)
            }
        };
        if let Some(entry) = self.installed.borrow_mut().iter_mut().find(|entry| std::ptr::eq(Rc::as_ptr(entry), local)) {
            *entry = updated.clone();
        }
        if notify {
            self.did_update_metadata_event.fire(DidUpdateExtensionMetadata { profile_location: uri_file("profile"), local: updated.clone() });
        }
        Promise::resolved(updated)
    }

    /// `async resetPinnedStateForAllUserExtensions(pinned) { }`.
    fn reset_pinned_state_for_all_user_extensions(&self, _pinned: bool) -> Promise<()> {
        Promise::resolved(())
    }

    fn download(&self, _extension: &GalleryExtension, _operation: InstallOperation, _donot_verify_signature: bool) -> Promise<URI> {
        not_a_function("download")
    }

    fn register_participant(&self, _participant: Rc<dyn IExtensionManagementParticipant>) {}

    /// `getTargetPlatform: async () => getTargetPlatform(platform, arch)`.
    fn get_target_platform(&self) -> Promise<TargetPlatform> {
        Promise::resolved(host_target_platform())
    }

    fn clean_up(&self) -> Promise<()> {
        not_a_function("cleanUp")
    }
}

impl IWorkbenchExtensionInstallService for StubExtensionManagementService {
    fn get_installable_servers(&self, _extension: &GalleryExtension) -> Promise<Vec<Rc<IExtensionManagementServer>>> {
        not_a_function("getInstallableServers")
    }

    fn get_resource_extensions(&self, _locations: Vec<URI>) -> Promise<Vec<IResourceExtension>> {
        not_a_function("getExtensions")
    }

    fn can_install_resource(&self, _extension: &IResourceExtension) -> Promise<Result<(), String>> {
        not_a_function("canInstall")
    }

    fn install_resource_extension(&self, _extension: &IResourceExtension, _options: InstallOptions) -> Promise<ExtensionRef> {
        not_a_function("installResourceExtension")
    }

    fn install_vsix(&self, _vsix: &URI, _manifest: ExtensionManifest, _options: InstallOptions) -> Promise<ExtensionRef> {
        not_a_function("installVSIX")
    }

    fn update_from_gallery(&self, _gallery: &GalleryExtension, _existing: &LocalExtension, _options: InstallOptions) -> Promise<ExtensionRef> {
        not_a_function("updateFromGallery")
    }

    fn install_from_gallery_in_servers(
        &self,
        _gallery: &GalleryExtension,
        _options: InstallOptions,
        _servers: Option<Vec<Rc<IExtensionManagementServer>>>,
    ) -> Promise<ExtensionRef> {
        not_a_function("installFromGallery")
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// IExtensionGalleryService.
// ---------------------------------------------------------------------------------------------------------------------

/// A instância esvaziada de `ExtensionGalleryService` do `setup`, com `isEnabled` verdadeiro, `query` devolvendo a página
/// de `stubPromise`, `getExtensions` a lista e `getCompatibleExtension` a extensão (o que ninguém trocou devolve
/// `undefined`: `isExtensionCompatible` falso, `getCompatibleExtension` sem extensão).
pub struct StubGalleryService {
    page: RefCell<Vec<GalleryExtension>>,
    extensions: RefCell<Vec<GalleryExtension>>,
    compatible: RefCell<Option<GalleryExtension>>,
}

impl StubGalleryService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { page: RefCell::new(Vec::new()), extensions: RefCell::new(Vec::new()), compatible: RefCell::new(None) })
    }

    /// `stubPromise(IExtensionGalleryService, 'query', aPage(...extensions))`.
    pub fn stub_query(&self, extensions: Vec<GalleryExtension>) {
        *self.page.borrow_mut() = extensions;
    }

    /// `stubPromise(IExtensionGalleryService, 'getExtensions', extensions)`.
    pub fn stub_get_extensions(&self, extensions: Vec<GalleryExtension>) {
        *self.extensions.borrow_mut() = extensions;
    }

    /// `stubPromise(IExtensionGalleryService, 'getCompatibleExtension', extension)`.
    pub fn stub_get_compatible_extension(&self, extension: GalleryExtension) {
        *self.compatible.borrow_mut() = Some(extension);
    }
}

impl IExtensionGalleryService for StubGalleryService {
    fn is_enabled(&self) -> bool {
        true
    }

    fn query(&self, _options: QueryOptions, _token: CancellationToken) -> Promise<Rc<dyn IPager<GalleryExtension>>> {
        Promise::resolved(single_page_pager(self.page.borrow().clone()))
    }

    fn get_extensions(&self, _extension_infos: Vec<ExtensionInfo>, _options: ExtensionQueryOptions, _token: CancellationToken) -> Promise<Vec<GalleryExtension>> {
        Promise::resolved(self.extensions.borrow().clone())
    }

    fn is_extension_compatible(
        &self,
        _extension: &GalleryExtension,
        _include_pre_release: bool,
        _target_platform: TargetPlatform,
        _product_version: Option<ProductVersion>,
    ) -> Promise<bool> {
        Promise::resolved(false)
    }

    fn get_compatible_extension(
        &self,
        _extension: &GalleryExtension,
        _include_pre_release: bool,
        _target_platform: TargetPlatform,
        _product_version: Option<ProductVersion>,
    ) -> Promise<Option<GalleryExtension>> {
        Promise::resolved(self.compatible.borrow().clone())
    }

    fn get_all_compatible_versions(&self, _extension_identifier: &ExtensionIdentifierInfo, _include_pre_release: bool, _target_platform: TargetPlatform) -> Promise<Vec<GalleryExtensionVersion>> {
        Promise::resolved(Vec::new())
    }

    fn get_all_versions(&self, _extension_identifier: &ExtensionIdentifierInfo) -> Promise<Vec<GalleryExtensionVersion>> {
        Promise::resolved(Vec::new())
    }

    fn download(&self, _extension: &GalleryExtension, _location: &URI, _operation: InstallOperation) -> Promise<()> {
        Promise::resolved(())
    }

    fn download_signature_archive(&self, _extension: &GalleryExtension, _location: &URI) -> Promise<()> {
        Promise::resolved(())
    }

    fn report_statistic(&self, _publisher: &str, _name: &str, _version: &str, _statistic_type: StatisticType) -> Promise<()> {
        Promise::resolved(())
    }

    fn get_readme(&self, _extension: &GalleryExtension, _token: CancellationToken) -> Promise<String> {
        Promise::resolved(String::new())
    }

    fn get_manifest(&self, _extension: &GalleryExtension, _token: CancellationToken) -> Promise<Option<Value>> {
        Promise::resolved(None)
    }

    fn get_changelog(&self, _extension: &GalleryExtension, _token: CancellationToken) -> Promise<String> {
        Promise::resolved(String::new())
    }

    fn get_core_translation(&self, _extension: &GalleryExtension, _language_id: &str) -> Promise<Option<Value>> {
        Promise::resolved(None)
    }

    fn get_extensions_control_manifest(&self) -> Promise<ExtensionsControlManifest> {
        Promise::resolved(ExtensionsControlManifest::default())
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// IConfigurationService (`stubConfiguration`).
// ---------------------------------------------------------------------------------------------------------------------

/// `Configuration` sem nenhuma camada preenchida.
fn empty_configuration() -> Configuration {
    let model = || ConfigurationModel::create_empty_model(NullLogService::shared());
    Configuration::new(model(), model(), model(), model(), model(), model(), ResourceMap::new(), model(), ResourceMap::new(), NullLogService::shared())
}

/// O literal de `stubConfiguration(autoUpdateValue?)`: `getValue(key)` devolve `values[key]`, `updateValue` grava e dispara
/// `onDidChangeConfiguration`, e `inspect` devolve `{}`.
pub struct StubConfigurationService {
    values: RefCell<HashMap<String, Dynamic>>,
    emitter: Rc<Emitter<Rc<ConfigurationChangeEvent>>>,
}

const AUTO_UPDATE_KEY: &str = "extensions.autoUpdate";
const AUTO_UPDATE_DELAY_KEY: &str = "extensions.autoUpdateDelay";
const AUTO_CHECK_UPDATES_KEY: &str = "extensions.autoCheckUpdates";
const AUTO_RESTART_KEY: &str = "extensions.autoRestart";

impl StubConfigurationService {
    /// `stubConfiguration(autoUpdateValue)`: `autoUpdateValue ?? true`, o atraso `2` e `autoCheckUpdates` verdadeiro.
    pub fn new(guard: &LeakGuard, auto_update_value: Option<Dynamic>) -> Rc<Self> {
        let mut values = HashMap::new();
        values.insert(AUTO_UPDATE_KEY.to_owned(), auto_update_value.unwrap_or(Dynamic::Bool(true)));
        values.insert(AUTO_UPDATE_DELAY_KEY.to_owned(), Dynamic::Number(2.0));
        values.insert(AUTO_CHECK_UPDATES_KEY.to_owned(), Dynamic::Bool(true));
        Rc::new(Self { values: RefCell::new(values), emitter: guard.add_rc(Emitter::new()) })
    }

    fn fire_change(&self, key: &str) {
        let keys: Vec<JsString> = [AUTO_UPDATE_KEY, AUTO_UPDATE_DELAY_KEY, AUTO_CHECK_UPDATES_KEY, AUTO_RESTART_KEY, key].iter().map(|key| JsString::from(*key)).collect();
        let change = ConfigurationChange { keys, overrides: Vec::new() };
        let event = ConfigurationChangeEvent::new(change, None, Rc::new(empty_configuration()), None, NullLogService::shared());
        event.source.set(Some(ConfigurationTarget::User));
        self.emitter.fire(Rc::new(event));
    }
}

impl IConfigurationService for StubConfigurationService {
    fn on_did_change_configuration(&self) -> Event<Rc<ConfigurationChangeEvent>> {
        self.emitter.event()
    }

    fn get_configuration_data(&self) -> Option<ConfigurationData> {
        None
    }

    /// `getValue: (key?) => key ? values[key] : undefined`.
    fn get_value(&self, section: Option<&str>, _overrides: Option<&ConfigurationOverrides>) -> Dynamic {
        match section.filter(|section| !section.is_empty()) {
            Some(key) => self.values.borrow().get(key).cloned().unwrap_or(Dynamic::Undefined),
            None => Dynamic::Undefined,
        }
    }

    fn update_value(&self, key: &str, value: Dynamic, _overrides: Option<ConfigurationUpdateOverrides>, _target: Option<ConfigurationTarget>) -> Promise<()> {
        self.values.borrow_mut().insert(key.to_owned(), value);
        self.fire_change(key);
        Promise::resolved(())
    }

    /// `inspect: (key) => ({})`.
    fn inspect(&self, _key: &str, _overrides: &ConfigurationOverrides) -> Rc<dyn ConfigurationValue> {
        Rc::new(TestConfigurationValue::default())
    }

    fn reload_configuration(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn keys(&self) -> ConfigurationKeys {
        ConfigurationKeys { default: Vec::new(), policy: Vec::new(), user: Vec::new(), workspace: Vec::new(), workspace_folder: Vec::new() }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Os demais serviços.
// ---------------------------------------------------------------------------------------------------------------------

/// `IDialogService`: o literal que o caso registra (`{ confirm }` ou `{ prompt }`), ou nenhum (o serviço `undefined`, cujas
/// chamadas rejeitam com o `TypeError` do original).
pub struct StubDialogService {
    confirm_result: Option<ConfirmationResult>,
    prompt_result: Option<PromptResult>,
}

impl StubDialogService {
    /// O `IDialogService` que o `setup` não registra.
    pub fn unregistered() -> Rc<Self> {
        Rc::new(Self { confirm_result: None, prompt_result: None })
    }

    /// `{ confirm: () => Promise.resolve({ confirmed }) }`.
    pub fn confirming(confirmed: bool) -> Rc<Self> {
        Rc::new(Self { confirm_result: Some(ConfirmationResult { checkbox_checked: None, confirmed }), prompt_result: None })
    }

    /// `{ prompt() { return Promise.resolve({ result }); } }`.
    pub fn prompting(result: Dynamic) -> Rc<Self> {
        Rc::new(Self { confirm_result: None, prompt_result: Some(PromptResult { checkbox_checked: None, result: Some(result) }) })
    }
}

impl IDialogService for StubDialogService {
    fn on_will_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn on_did_show_dialog(&self) -> Event<()> {
        Event::none()
    }

    fn confirm(&self, _confirmation: Confirmation) -> Promise<ConfirmationResult> {
        match self.confirm_result {
            Some(result) => Promise::resolved(result),
            None => Promise::rejected(Error::new("Cannot read properties of undefined (reading 'confirm')")),
        }
    }

    fn prompt(&self, _prompt: Prompt) -> Promise<PromptResult> {
        match &self.prompt_result {
            Some(result) => Promise::resolved(result.clone()),
            None => Promise::rejected(Error::new("Cannot read properties of undefined (reading 'prompt')")),
        }
    }

    fn input(&self, _input: Input) -> Promise<InputResult> {
        Promise::rejected(Error::new("Cannot read properties of undefined (reading 'input')"))
    }

    fn info(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::rejected(Error::new("Cannot read properties of undefined (reading 'info')"))
    }

    fn warn(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::rejected(Error::new("Cannot read properties of undefined (reading 'warn')"))
    }

    fn error(&self, _message: &str, _detail: Option<&str>) -> Promise<()> {
        Promise::rejected(Error::new("Cannot read properties of undefined (reading 'error')"))
    }

    fn about(&self) -> Promise<()> {
        Promise::rejected(Error::new("Cannot read properties of undefined (reading 'about')"))
    }
}

/// `IProgressService` que executa a tarefa (ver a nota do módulo).
pub struct RunningProgressService;

impl IProgressService for RunningProgressService {
    fn with_progress(&self, _options: ProgressOptions, task: ProgressTask, _on_did_cancel: Option<Rc<dyn Fn(Option<i32>)>>) -> Promise<()> {
        let progress: Rc<dyn IProgress<ProgressStep>> = Progress::none();
        task(progress)
    }
}

/// `{ onStateChange: Event.None, state: State.Uninitialized }`.
pub struct StubUpdateService;

impl UpdateService for StubUpdateService {
    fn on_state_change(&self) -> Event<State> {
        Event::none()
    }

    fn state(&self) -> State {
        State::Uninitialized
    }

    fn check_for_updates(&self, _explicit: bool) -> Promise<()> {
        not_a_function("checkForUpdates")
    }

    fn download_update(&self, _explicit: bool) -> Promise<()> {
        not_a_function("downloadUpdate")
    }

    fn apply_update(&self) -> Promise<()> {
        not_a_function("applyUpdate")
    }

    fn quit_and_install(&self) -> Promise<()> {
        not_a_function("quitAndInstall")
    }

    fn is_latest_version(&self) -> Promise<Option<bool>> {
        not_a_function("isLatestVersion")
    }

    fn apply_specific_update(&self, _package_path: &str) -> Promise<()> {
        not_a_function("_applySpecificUpdate")
    }

    fn set_internal_org(&self, _internal_org: Option<&str>) -> Promise<()> {
        not_a_function("setInternalOrg")
    }
}

/// `{ isConnectionMetered: false, onDidChangeIsConnectionMetered: Event.None }`.
pub struct StubMeteredConnectionService;

impl IMeteredConnectionService for StubMeteredConnectionService {
    fn is_connection_metered(&self) -> bool {
        false
    }

    fn on_did_change_is_connection_metered(&self) -> Event<bool> {
        Event::none()
    }
}

/// `{ onDidChangeExtensions: Event.None, extensions, canAddExtension: false, canRemoveExtension: false,
/// whenInstalledExtensionsRegistered: true }`.
pub struct StubExtensionService {
    extensions: Vec<ExtensionDescription>,
}

impl StubExtensionService {
    /// `extensions: []`.
    pub fn new() -> Rc<Self> {
        Self::with_extensions(Vec::new())
    }

    /// `extensions: [runningExtension]`.
    pub fn with_extensions(extensions: Vec<ExtensionDescription>) -> Rc<Self> {
        Rc::new(Self { extensions })
    }
}

impl IExtensionService for StubExtensionService {
    fn activate_by_event(&self, _activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        not_a_function("activateByEvent")
    }

    fn activation_event_is_done(&self, _activation_event: &str) -> bool {
        errors::throw(Error::new("activationEventIsDone is not a function"))
    }

    fn get_extension(&self, _id: &str) -> Promise<Option<IExtensionDescription>> {
        not_a_function("getExtension")
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        Event::none()
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        self.extensions.clone()
    }
}

/// O `IIgnoredExtensionsManagementService` que o `setup` não registra.
pub struct NoIgnoredExtensionsService;

impl IIgnoredExtensionsManagementService for NoIgnoredExtensionsService {
    fn has_to_never_sync_extension(&self, _extension_id: &str) -> bool {
        false
    }

    fn has_to_always_sync_extension(&self, _extension_id: &str) -> bool {
        false
    }

    fn update_ignored_extensions(&self, _ignored_extension_id: &str, _ignore: bool) -> Promise<()> {
        Promise::resolved(())
    }
}

/// O `IUserDataAutoSyncService` que o `setup` não registra.
pub struct NoUserDataAutoSyncService;

impl IUserDataAutoSyncService for NoUserDataAutoSyncService {
    fn on_error(&self) -> Event<Error> {
        Event::none()
    }

    fn turn_on(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn turn_off(&self, _everywhere: bool) -> Promise<()> {
        Promise::resolved(())
    }

    fn trigger_sync(&self, _sources: &[String], _options: Option<SyncOptions>) -> Promise<()> {
        Promise::resolved(())
    }
}

/// O `IUriIdentityService` que o `setup` não registra.
pub struct NoUriIdentityService;

impl IUriIdentityService for NoUriIdentityService {
    fn ext_uri(&self) -> je_base::resources::ExtUri {
        ext_uri()
    }

    fn as_canonical_uri(&self, uri: &URI) -> URI {
        uri.clone()
    }
}

/// O `IUserDataProfilesService` que o `setup` não registra: `profiles` vazio (o original só o lê quando a extensão é de
/// escopo de aplicação, o que nenhum caso tem) e o perfil padrão dado.
pub struct NoUserDataProfilesService {
    default_profile: UserDataProfile,
}

fn not_implemented() -> ! {
    errors::throw(Error::new("Cannot read properties of undefined"))
}

impl IUserDataProfilesService for NoUserDataProfilesService {
    fn profiles_home(&self) -> URI {
        not_implemented()
    }

    fn default_profile(&self) -> UserDataProfile {
        self.default_profile.clone()
    }

    fn on_did_change_profiles(&self) -> Event<DidChangeProfilesEvent> {
        Event::none()
    }

    fn profiles(&self) -> Vec<UserDataProfile> {
        Vec::new()
    }

    fn on_did_reset_workspaces(&self) -> Event<()> {
        Event::none()
    }

    fn create_named_profile(&self, _name: &str, _options: Option<UserDataProfileOptions>, _workspace_identifier: Option<AnyWorkspaceIdentifier>) -> Promise<UserDataProfile> {
        not_a_function("createNamedProfile")
    }

    fn create_transient_profile(&self, _workspace_identifier: Option<AnyWorkspaceIdentifier>) -> Promise<UserDataProfile> {
        not_a_function("createTransientProfile")
    }

    fn create_profile(&self, _id: &str, _name: &str, _options: Option<UserDataProfileOptions>, _workspace_identifier: Option<AnyWorkspaceIdentifier>) -> Promise<UserDataProfile> {
        not_a_function("createProfile")
    }

    fn update_profile(&self, _profile: &UserDataProfile, _options: UserDataProfileUpdateOptions) -> Promise<UserDataProfile> {
        not_a_function("updateProfile")
    }

    fn remove_profile(&self, _profile: &UserDataProfile) -> Promise<()> {
        not_a_function("removeProfile")
    }

    fn set_profile_for_workspace(&self, _workspace_identifier: &AnyWorkspaceIdentifier, _profile: &UserDataProfile) -> Promise<()> {
        not_a_function("setProfileForWorkspace")
    }

    fn reset_workspaces(&self) -> Promise<()> {
        not_a_function("resetWorkspaces")
    }

    fn clean_up(&self) -> Promise<()> {
        not_a_function("cleanUp")
    }

    fn clean_up_transient_profiles(&self) -> Promise<()> {
        not_a_function("cleanUpTransientProfiles")
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// O `setup`.
// ---------------------------------------------------------------------------------------------------------------------

/// `sinon.spy()` passado a um evento: conta as chamadas.
pub struct Spy {
    count: Rc<Cell<usize>>,
}

impl Spy {
    pub fn new() -> Self {
        Self { count: Rc::new(Cell::new(0)) }
    }

    /// `disposableStore.add(event(target))`.
    pub fn attach<T: Clone + 'static>(&self, guard: &LeakGuard, event: Event<T>) {
        let count = self.count.clone();
        guard.add(event.listen(move |_| count.set(count.get() + 1)));
    }

    /// `target.called`.
    pub fn called(&self) -> bool {
        self.count.get() > 0
    }

    /// `target.calledOnce`.
    pub fn called_once(&self) -> bool {
        self.count.get() == 1
    }
}

/// O `TestInstantiationService` do `setup` da suíte `ExtensionsWorkbenchServiceTest`.
pub struct WorkbenchFixture {
    /// `disposableStore`.
    pub guard: LeakGuard,
    /// O serviço de gerenciamento do `setup` (`IExtensionManagementService` e `IWorkbenchExtensionManagementService`); os
    /// emissores `installEvent`, `didInstallEvent`, `uninstallEvent` e `didUninstallEvent` são os dele.
    pub management: Rc<StubExtensionManagementService>,
    pub gallery: Rc<StubGalleryService>,
    pub context: Rc<TestContextService>,
    pub storage: Rc<AbstractStorageService>,
    lifecycle: Rc<TestLifecycleService>,
    workspace_trust: Rc<TestWorkspaceTrustManagementService>,
    manifest_properties: Rc<ExtensionManifestPropertiesService>,
    configuration: RefCell<Rc<StubConfigurationService>>,
    server_service: RefCell<Rc<dyn IExtensionManagementServerService>>,
    enablement: RefCell<Rc<TestExtensionEnablementService>>,
    environment: Rc<dyn IWorkbenchEnvironmentService>,
    user_data_sync_enablement: Rc<StubUserDataSyncEnablementService>,
    product: Rc<RefCell<ProductService>>,
    extension_service: RefCell<Rc<StubExtensionService>>,
    dialog: RefCell<Rc<StubDialogService>>,
    file_service: Rc<dyn IFileService>,
    user_data_profile_service: Rc<UserDataProfileService>,
    user_data_profiles_service: Rc<NoUserDataProfilesService>,
}

impl WorkbenchFixture {
    /// O `setup`.
    pub fn new() -> Rc<Self> {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let file_service_concrete = guard.add_rc(TestFileService::new());
        let file_service: Rc<dyn IFileService> = file_service_concrete;
        let gallery = StubGalleryService::new();
        let context = TestContextService::new(None, None);
        let configuration = StubConfigurationService::new(&guard, None);
        let profile = to_user_data_profile("test", "test", &uri_file("foo"), &uri_file("cache"), None, None, &ext_uri());
        let user_data_profile_service = guard.add_rc(UserDataProfileService::create(profile.clone(), ()));
        let user_data_profiles_service = Rc::new(NoUserDataProfilesService { default_profile: profile });
        let management = StubExtensionManagementService::new(&guard, Vec::new());
        let local_server = Rc::new(IExtensionManagementServer { id: "local".to_owned(), label: "local".to_owned(), extension_management_service: management.clone() });
        let server_service = an_extension_management_server_service(Some(local_server), None, None);
        let storage = guard.add_rc(InMemoryStorageService::new());
        let lifecycle = guard.add_rc(TestLifecycleService::new());
        let workspace_trust = guard.add_rc(TestWorkspaceTrustManagementService::new(None, None));
        let manifest_properties = guard.add_rc(ExtensionManifestPropertiesService::new(
            Rc::new(TestProductService::default()),
            configuration.clone(),
            TestWorkspaceTrustEnablementService::new(true),
            NullLogService::shared(),
        ));
        let environment: Rc<dyn IWorkbenchEnvironmentService> = TestEnvironmentService::new();
        let user_data_sync_enablement = Rc::new(StubUserDataSyncEnablementService { enabled: false });
        let enablement = Self::new_enablement(
            &guard,
            &storage,
            &context,
            &configuration,
            &server_service,
            &management,
            &lifecycle,
            &workspace_trust,
            &manifest_properties,
            &environment,
            &user_data_sync_enablement,
        );
        enablement.reset();
        Rc::new(Self {
            guard,
            management,
            gallery,
            context,
            storage,
            lifecycle,
            workspace_trust,
            manifest_properties,
            configuration: RefCell::new(configuration),
            server_service: RefCell::new(server_service),
            enablement: RefCell::new(enablement),
            environment,
            user_data_sync_enablement,
            product: Rc::new(RefCell::new(TestProductService::default())),
            extension_service: RefCell::new(StubExtensionService::new()),
            dialog: RefCell::new(StubDialogService::unregistered()),
            file_service,
            user_data_profile_service,
            user_data_profiles_service,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn new_enablement(
        guard: &LeakGuard,
        storage: &Rc<AbstractStorageService>,
        context: &Rc<TestContextService>,
        configuration: &Rc<StubConfigurationService>,
        server_service: &Rc<dyn IExtensionManagementServerService>,
        management: &Rc<StubExtensionManagementService>,
        lifecycle: &Rc<TestLifecycleService>,
        workspace_trust: &Rc<TestWorkspaceTrustManagementService>,
        manifest_properties: &Rc<ExtensionManifestPropertiesService>,
        environment: &Rc<dyn IWorkbenchEnvironmentService>,
        user_data_sync_enablement: &Rc<StubUserDataSyncEnablementService>,
    ) -> Rc<TestExtensionEnablementService> {
        TestExtensionEnablementService::create(
            guard,
            EnablementServices {
                storage: storage.clone(),
                context: context.clone(),
                environment: environment.clone(),
                extension_management_service: management.clone(),
                configuration: configuration.clone(),
                server_service: server_service.clone(),
                user_data_sync_enablement_service: user_data_sync_enablement.clone(),
                default_account_service: Rc::new(StubDefaultAccountService),
                user_data_sync_account_service: Rc::new(StubUserDataSyncAccountService { account: None }),
                lifecycle_service: lifecycle.clone(),
                notification_service: Rc::new(TestNotificationService),
                host_service: Rc::new(TestHostService),
                workspace_trust_management_service: workspace_trust.clone(),
                extension_manifest_properties_service: manifest_properties.clone(),
                chat_entitlement_service: je_workbench::test::common::workbench_test_services::TestChatEntitlementService::new(),
            },
        )
    }

    /// `instantiationService.get(IWorkbenchExtensionEnablementService)`.
    pub fn enablement_service(&self) -> Rc<dyn IWorkbenchExtensionEnablementService> {
        self.enablement.borrow().service()
    }

    /// `instantiationService.stub(IExtensionManagementServerService, service)`.
    pub fn stub_server_service(&self, service: Rc<dyn IExtensionManagementServerService>) {
        *self.server_service.borrow_mut() = service;
    }

    /// `instantiationService.stub(IWorkbenchExtensionEnablementService, disposableStore.add(new TestExtensionEnablementService(instantiationService)))`.
    pub fn restub_enablement_service(&self) {
        let enablement = Self::new_enablement(
            &self.guard,
            &self.storage,
            &self.context,
            &self.configuration.borrow(),
            &self.server_service.borrow(),
            &self.management,
            &self.lifecycle,
            &self.workspace_trust,
            &self.manifest_properties,
            &self.environment,
            &self.user_data_sync_enablement,
        );
        *self.enablement.borrow_mut() = enablement;
    }

    /// `stubConfiguration(autoUpdateValue)`: o valor do `extensions.autoUpdate` (`None` é `undefined`, que vale `true`).
    pub fn stub_configuration(&self, auto_update_value: Option<Value>) {
        *self.configuration.borrow_mut() = StubConfigurationService::new(&self.guard, auto_update_value.as_ref().map(dynamic_of));
    }

    /// `instantiationService.stub(IProductService, product)`.
    pub fn stub_product_service(&self, product: ProductService) {
        *self.product.borrow_mut() = product;
    }

    /// `instantiationService.stub(IExtensionService, { extensions, ... })`.
    pub fn stub_extension_service(&self, extensions: Vec<ExtensionDescription>) {
        *self.extension_service.borrow_mut() = StubExtensionService::with_extensions(extensions);
    }

    /// `instantiationService.stub(IDialogService, ...)`.
    pub fn stub_dialog_service(&self, service: Rc<StubDialogService>) {
        *self.dialog.borrow_mut() = service;
    }

    /// `aMultiExtensionManagementServerService(instantiationService, localExtensionManagementService, remoteExtensionManagementService)`.
    pub fn a_multi_extension_management_server_service(&self, local: Vec<ExtensionRef>, remote: Vec<ExtensionRef>) -> Rc<dyn IExtensionManagementServerService> {
        let server = |id: &str, label: &str, installed: Vec<ExtensionRef>| {
            Rc::new(IExtensionManagementServer { id: id.to_owned(), label: label.to_owned(), extension_management_service: StubExtensionManagementService::new(&self.guard, installed) })
        };
        an_extension_management_server_service(Some(server("vscode-local", "local", local)), Some(server("vscode-remote", "remote", remote)), None)
    }

    /// `instantiationService.createInstance(ExtensionsWorkbenchService)`, já no `disposableStore`.
    pub fn create_workbench_service(&self) -> Rc<ExtensionsWorkbenchService> {
        let enablement = self.enablement.borrow().clone();
        let product = self.product.clone();
        let file_service = self.file_service.clone();
        let parts = ExtensionsWorkbenchServiceParts {
            extension_management_service: self.management.clone(),
            gallery_service: self.gallery.clone(),
            extension_services: Rc::new(move || extension_services(file_service.clone(), product.borrow().clone())),
            configuration_service: self.configuration.borrow().clone(),
            telemetry_service: Rc::new(NullTelemetryServiceShape),
            notification_service: Rc::new(TestNotificationService),
            extension_enablement_service: enablement.service(),
            host_service: Rc::new(TestHostService),
            progress_service: Rc::new(RunningProgressService),
            extension_management_server_service: self.server_service.borrow().clone(),
            extensions_sync_management_service: Rc::new(NoIgnoredExtensionsService),
            user_data_auto_sync_service: Rc::new(NoUserDataAutoSyncService),
            product_service: Rc::new(self.product.borrow().clone()),
            extension_manifest_properties_service: self.manifest_properties.clone(),
            log_service: NullLogService::shared(),
            extension_service: self.extension_service.borrow().clone(),
            lifecycle_service: self.lifecycle.clone(),
            user_data_profile_service: self.user_data_profile_service.clone(),
            user_data_profiles_service: self.user_data_profiles_service.clone(),
            storage_service: Rc::new(self.storage.clone()),
            dialog_service: self.dialog.borrow().clone(),
            user_data_sync_enablement_service: self.user_data_sync_enablement.clone(),
            update_service: Rc::new(StubUpdateService),
            workspace_context_service: self.context.clone(),
            allowed_extensions_service: enablement.allowed_extensions_service.clone(),
            metered_connection_service: Rc::new(StubMeteredConnectionService),
            uri_identity_service: Rc::new(NoUriIdentityService),
        };
        self.guard.add_rc(ExtensionsWorkbenchService::new(parts))
    }

    /// `aWorkbenchService()`.
    pub async fn a_workbench_service(&self) -> Rc<ExtensionsWorkbenchService> {
        let service = self.create_workbench_service();
        service.query_local(None).await.expect("queryLocal resolve");
        service
    }

    /// `anOutdatedExtensionWorkbenchService(lastUpdated, isSigned)`.
    pub async fn an_outdated_extension_workbench_service(&self, last_updated: Option<f64>, is_signed: bool) -> Rc<ExtensionsWorkbenchService> {
        let local = a_local_extension_with("a", json!({ "version": "1.0.1" }), LocalProperties::default());
        let gallery = a_gallery_extension_props(
            &local.manifest.name,
            GalleryProperties { version: Some("1.0.2"), last_updated, is_signed: Some(is_signed), ..GalleryProperties::default() },
        );
        self.management.stub_installed(vec![local]);
        self.gallery.stub_query(vec![gallery.clone()]);
        self.gallery.stub_get_compatible_extension(gallery.clone());
        self.gallery.stub_get_extensions(vec![gallery]);
        let service = self.a_workbench_service().await;
        service.check_for_updates(None, false).await.expect("checkForUpdates resolve");
        service
    }
}

/// O número de voltas do laço de eventos que um caso tem para terminar (cada volta esvazia as microtarefas e dispara os
/// timers vencidos).
const MAX_TURNS: usize = 100_000;

/// Roda o caso: o `setup`, o corpo (`async`), e o `teardown` (descartar o `disposableStore` e conferir os vazamentos).
pub fn run<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<WorkbenchFixture>) -> F) {
    let fixture = WorkbenchFixture::new();
    let done = Rc::new(Cell::new(false));
    let finished = done.clone();
    let future = body(fixture.clone());
    runtime::spawn_local(async move {
        future.await;
        finished.set(true);
    });
    for _ in 0..MAX_TURNS {
        runtime::run_microtasks();
        if done.get() {
            break;
        }
        runtime::turn();
    }
    assert!(done.get(), "o teste assíncrono não terminou");
}
