//! Porte de `src/vs/workbench/contrib/extensions/browser/extensionsWorkbenchService.ts`, classes `Extensions` (as
//! extensões de um servidor de gerenciamento) e `ExtensionsWorkbenchService` (a lista unificada, a consulta local e da
//! galeria, instalação, remoção, habilitação, atualização automática e estado de execução). A classe `Extension` está em
//! [`super::extensions_workbench_service`].
//!
//! Notas de tradução:
//! - **Injeção de dependência.** Os serviços do construtor entram agrupados em [`ExtensionsWorkbenchServiceParts`]. O
//!   `IInstantiationService.createInstance(Extension, ...)` é [`Extensions::create_extension`], que monta a `Extension`
//!   com os provedores de estado e a fábrica de serviços `extension_services`.
//! - **`IExtension`.** O original devolve a interface; só existe a classe `Extension`, então a API devolve `Rc<Extension>`
//!   (comparada por identidade, `Rc::ptr_eq`, como o `===` do original) e o `instanceof Extension` de `canInstall` é sempre
//!   verdadeiro.
//! - **Servidor de instalação.** O `IExtensionManagementService` de cada servidor é o contrato reduzido; as chamadas de
//!   instalação (`installFromGallery`, `updateMetadata`, `getTargetPlatform`, `canInstall`...) vão pelo
//!   [`IProfileAwareExtensionManagementService::as_install_service`]. Um servidor que não o oferece só alimenta a lista
//!   local (sem eventos de instalação) e recusa instalar.
//! - **`Promise`.** As `async` do original são [`async_fn`]; `Promises.settled` e `Promise.allSettled` são
//!   `Promise::all_settled`. `CancelablePromise` de `withProgress` é o [`CancelablePromise`] de `je-base`.
//! - **`ExtensionGalleryService`.** É o contrato assíncrono [`IExtensionGalleryService`]; a `Extension` ainda lê o
//!   [`GalleryApi`] síncrono, que chega pela fábrica `extension_services`.
//! - **`isWeb`** é [`Platform::detect`]. **`ThrottledDelayer`** vem de `je_base::async_`.
//! - **Recortes (não portado):** `open`/`openSearch` (`ExtensionsInput`, `IExtensionsViewPaneContainer`), `downloadVSIX`
//!   (`IQuickInputService`, `IFileDialogService`, `fromNow`), `setLanguage`/`canSetLanguage` (`ILocaleService`,
//!   `getLocale` de `languagePacks`), o tratador de URL (`handleURL`), `getExtensionRuntimeStatus`
//!   (`getExtensionsStatus`), o aviso de extensões (`getExtensionsNotification`, `IExtensionsNotification`,
//!   `MarkdownString`), a chave de contexto `HasOutdatedExtensionsContext`, o relatório de telemetria
//!   `installedExtensions`, o relatório de progresso de outras fontes (`reportProgressFromOtherSources`) e a
//!   verificação de assinatura de repositório de `canInstall` (`shouldRequireRepositorySignatureFor` e o
//!   `IExtensionGalleryManifestService` ainda não têm porte). O aviso de "extensão não encontrada" de `install` sai sem o
//!   link de suporte do manifesto da galeria, pelo mesmo motivo.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::{ThrottledDelayer, async_fn};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, is_cancellation_error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::paging::{IPager, single_page_pager};
use je_base::platform::Platform;
use je_base::promise::{CancelablePromise, Promise};
use je_base::semver;
use je_base::uri::URI;
use je_extensions::allowed::AllowedExtensionsService;
use je_extensions::management::{
    ExtensionInfo, ExtensionManagementErrorCode, ExtensionQueryOptions, ExtensionsControlManifest, GalleryExtension, InstallOperation, InstallOptions,
    InstallSource, LocalExtension, Metadata, ProductVersion, QueryOptions, UninstallOptions, WEB_EXTENSION_TAG, is_extension_identifier,
    is_target_platform_compatible,
};
use je_extensions::manifest::{ExtensionIdentifier, ExtensionKind, ExtensionManifest, ExtensionType, TargetPlatform, is_application_scoped_extension};
use je_extensions::scanner::ExtensionDescription;
use je_extensions::util::{are_same_extensions, get_gallery_extension_id, group_by_extension};
use je_extensions::validator::is_engine_valid;
use je_platform::configuration::service::IConfigurationService;
use je_platform::dialogs::dialogs::{
    BaseDialogOptions, Confirmation, DialogDetail, DialogTypeOrSeverity, IDialogService, Prompt, PromptBaseButton, PromptButton, PromptCancel, PromptCancelButton,
};
use je_platform::log::LogServiceRef;
use je_platform::notification::{INotificationService, NotificationMessage, Severity};
use je_platform::product::IProductService;
use je_platform::progress::progress::{IProgressService, ProgressLocation, ProgressLocationId, ProgressOptions};
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::telemetry::ITelemetryService;
use je_platform::update::update::{State, StateType, UpdateService};
use je_platform::user_data_profile::user_data_profile::IUserDataProfilesService;
use je_platform::user_data_sync::user_data_sync::{IUserDataAutoSyncService, IUserDataSyncEnablementService, SyncResource};
use je_platform::workspace::IWorkspaceContextService;
use je_storage::StorageValue;
use serde_json::Value;

use super::extensions::{
    AUTO_CHECK_UPDATES_CONFIGURATION_KEY, AUTO_RESTART_CONFIGURATION_KEY, AUTO_UPDATE_CONFIGURATION_KEY, AUTO_UPDATE_DELAY_CONFIGURATION_KEY, ExtensionRuntimeActionType,
    ExtensionRuntimeState, ExtensionState, IExtension,
};
use super::extensions_workbench_service::{Extension, ExtensionServices, ExtensionStateProvider, ResourceExtensionInfo};
use crate::services::extension_management::extension_gallery_service::IExtensionGalleryService;
use crate::services::extension_management::extension_install_service::{
    IExtensionInstallService, InstallExtensionEvent, InstallExtensionInfo, UninstallExtensionInfo,
};
use crate::services::extension_management::extension_management::{
    DidUninstallExtensionEvent, EnablementState, ExtensionRef, IExtensionManagementServer, IExtensionManagementServerService, IResourceExtension,
    IWorkbenchExtensionEnablementService, InstallExtensionResult, InstallExtensionSource, local_extension_uri,
};
use crate::services::extension_management::extension_install_service::IWorkbenchExtensionInstallService;
use crate::services::extensions::extension_manifest_properties_service::IExtensionManifestPropertiesService;
use crate::services::extensions::extensions::{ExtensionHostUpdates, IExtensionService};
use crate::services::host::host::IHostService;
use crate::services::lifecycle::lifecycle::{ILifecycleService, LifecyclePhase};
use crate::services::user_data_profile::user_data_profile::IUserDataProfileService;

const EXTENSIONS_AUTO_UPDATE_KEY: &str = "extensions.autoUpdate";
const EXTENSIONS_DONOT_AUTO_UPDATE_KEY: &str = "extensions.donotAutoUpdate";

/// `UpdatesCheckInterval`: 12 horas.
const UPDATES_CHECK_INTERVAL_MS: u64 = 1000 * 60 * 60 * 12;

/// `AutoUpdateConfigurationValue`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoUpdateConfigurationValue {
    On,
    Off,
}

/// O `justification` de `InstallExtensionOptions`: `string | { reason: string; action: string }`.
#[derive(Clone, Debug)]
pub enum InstallJustification {
    Reason(String),
    ReasonWithAction { reason: String, action: String },
}

impl InstallJustification {
    fn reason(&self) -> &str {
        match self {
            Self::Reason(reason) | Self::ReasonWithAction { reason, .. } => reason,
        }
    }
}

/// `InstallExtensionOptions` (`contrib/extensions/common/extensions.ts`): as `InstallOptions` mais o que o workbench lê.
#[derive(Clone, Debug, Default)]
pub struct InstallExtensionOptions {
    pub base: InstallOptions,
    pub version: Option<String>,
    pub justification: Option<InstallJustification>,
    pub enable: bool,
    pub install_everywhere: bool,
}

/// O argumento de `install`: `string | URI | IExtension`.
#[derive(Clone)]
pub enum InstallArg {
    Id(String),
    Location(URI),
    Extension(Rc<Extension>),
}

/// O argumento de `isAutoUpdateEnabledFor` e `updateAutoUpdateEnablementFor`: `IExtension | string` (um publisher).
#[derive(Clone)]
pub enum ExtensionOrPublisher {
    Extension(Rc<Extension>),
    Publisher(String),
}

/// Os argumentos de `queryGallery`: `IQueryOptions` com o token.
pub type GalleryQueryOptions = QueryOptions;

/// O `{ extension; operation? }` que `Extensions.onChange` emite.
#[derive(Clone)]
pub struct ExtensionChange {
    pub extension: Rc<Extension>,
    pub operation: Option<InstallOperation>,
}

/// Os servidores da lista unificada: o `Extensions` de um servidor de gerenciamento.
type ServerExtensions = Rc<Extensions>;

fn identifiers_equal(a: &Rc<Extension>, b: &Rc<Extension>) -> bool {
    Rc::ptr_eq(a, b)
}

fn same_server(a: &Option<Rc<IExtensionManagementServer>>, b: &Option<Rc<IExtensionManagementServer>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

fn bool_enablement(state: EnablementState) -> bool {
    matches!(state, EnablementState::EnabledGlobally | EnablementState::EnabledWorkspace)
}

/// `toExtensionDescription(extension)` (`services/extensions/common/extensions.ts`).
fn to_extension_description(local: &LocalExtension) -> ExtensionDescription {
    let id = get_gallery_extension_id(Some(local.manifest.publisher.as_str()).filter(|publisher| !publisher.is_empty()), &local.manifest.name);
    ExtensionDescription {
        identifier: ExtensionIdentifier::new(id.clone()),
        id,
        is_builtin: local.extension_type == ExtensionType::System,
        is_user_builtin: local.extension_type == ExtensionType::User && local.is_builtin,
        is_under_development: false,
        extension_location: local.location.clone(),
        uuid: local.identifier.uuid.clone(),
        target_platform: local.target_platform,
        publisher_display_name: local.publisher_display_name.clone(),
        pre_release: local.pre_release,
        manifest: local.manifest.clone(),
    }
}

/// `toExtension(description)` (`services/extensions/common/extensions.ts`): a extensão local que a descrição em execução
/// representa, para achar o servidor dela.
fn to_extension(description: &ExtensionDescription) -> LocalExtension {
    LocalExtension {
        extension_type: if description.is_builtin { ExtensionType::System } else { ExtensionType::User },
        is_builtin: !description.is_under_development,
        identifier: je_extensions::manifest::ExtensionIdentifierInfo { id: description.identifier.value.clone(), uuid: description.uuid.clone() },
        manifest: description.manifest.clone(),
        location: description.extension_location.clone(),
        target_platform: description.target_platform,
        publisher_display_name: description.publisher_display_name.clone(),
        readme_url: None,
        changelog_url: None,
        is_valid: true,
        validations: Vec::new(),
        pre_release: description.pre_release,
        is_workspace_scoped: false,
        is_machine_scoped: false,
        is_application_scoped: false,
        publisher_id: None,
        installed_timestamp: None,
        is_pre_release_version: description.pre_release,
        has_pre_release_version: false,
        private: false,
        updated: false,
        pinned: false,
        force_auto_update: false,
        source: InstallSource::Vsix,
        size: 0.0,
    }
}

/// `groupByExtension(extensions, r => r.identifier)`: agrupa na ordem do primeiro aparecimento.
fn group_extensions(extensions: Vec<Rc<Extension>>) -> Vec<Vec<Rc<Extension>>> {
    let mut groups: Vec<Vec<Rc<Extension>>> = Vec::new();
    for extension in extensions {
        match groups.iter().position(|group| group.iter().any(|e| are_same_extensions(&e.identifier(), &extension.identifier()))) {
            Some(index) => groups[index].push(extension),
            None => groups.push(vec![extension]),
        }
    }
    groups
}

/// A data em ISO 8601 (`new Date(timestamp).toISOString()`).
fn iso_string(timestamp_ms: f64) -> String {
    let total_seconds = (timestamp_ms / 1000.0).floor() as i64;
    let millis = (timestamp_ms - (total_seconds as f64) * 1000.0) as i64;
    let days = total_seconds.div_euclid(86_400);
    let seconds_of_day = total_seconds.rem_euclid(86_400);
    // Dias desde 1970-01-01 para ano, mês e dia (algoritmo de Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 { month_part + 3 } else { month_part - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        seconds_of_day / 3600,
        (seconds_of_day % 3600) / 60,
        seconds_of_day % 60
    )
}

fn read_string_list(raw: Option<String>) -> Vec<String> {
    match raw.and_then(|raw| serde_json::from_str::<Value>(&raw).ok()) {
        Some(Value::Array(items)) => items.into_iter().filter_map(|item| item.as_str().map(str::to_owned)).collect(),
        _ => Vec::new(),
    }
}

fn dynamic_text(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Extensions: as extensões de um servidor
// ---------------------------------------------------------------------------------------------------------------------

/// Os serviços de que `Extensions` precisa.
struct ExtensionsParts {
    server: Rc<IExtensionManagementServer>,
    state_provider: ExtensionStateProvider<ExtensionState>,
    runtime_state_provider: ExtensionStateProvider<Option<ExtensionRuntimeState>>,
    is_workspace_server: bool,
    gallery_service: Rc<dyn IExtensionGalleryService>,
    extension_enablement_service: Rc<dyn IWorkbenchExtensionEnablementService>,
    workbench_extension_management_service: Rc<dyn IWorkbenchExtensionInstallService>,
    extension_services: Rc<dyn Fn() -> ExtensionServices>,
    telemetry_service: Rc<dyn ITelemetryService>,
}

/// `Extensions`.
pub struct Extensions {
    this: Weak<Extensions>,
    base: Rc<Disposable>,
    pub server: Rc<IExtensionManagementServer>,
    state_provider: ExtensionStateProvider<ExtensionState>,
    runtime_state_provider: ExtensionStateProvider<Option<ExtensionRuntimeState>>,
    is_workspace_server: bool,
    on_change: Rc<Emitter<Option<ExtensionChange>>>,
    on_reset: Rc<Emitter<()>>,
    installing: RefCell<Vec<Rc<Extension>>>,
    uninstalling: RefCell<Vec<Rc<Extension>>>,
    installed: RefCell<Vec<Rc<Extension>>>,
    local_cache: RefCell<Option<Vec<Rc<Extension>>>>,
    gallery_service: Rc<dyn IExtensionGalleryService>,
    extension_enablement_service: Rc<dyn IWorkbenchExtensionEnablementService>,
    workbench_extension_management_service: Rc<dyn IWorkbenchExtensionInstallService>,
    extension_services: Rc<dyn Fn() -> ExtensionServices>,
    telemetry_service: Rc<dyn ITelemetryService>,
}

impl Extensions {
    fn new(parts: ExtensionsParts) -> Rc<Self> {
        let base = Disposable::new();
        let on_change = Emitter::<Option<ExtensionChange>>::new();
        let on_reset = Emitter::<()>::new();
        base.register(on_change.clone());
        base.register(on_reset.clone());
        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            server: parts.server,
            state_provider: parts.state_provider,
            runtime_state_provider: parts.runtime_state_provider,
            is_workspace_server: parts.is_workspace_server,
            on_change,
            on_reset,
            installing: RefCell::new(Vec::new()),
            uninstalling: RefCell::new(Vec::new()),
            installed: RefCell::new(Vec::new()),
            local_cache: RefCell::new(None),
            gallery_service: parts.gallery_service,
            extension_enablement_service: parts.extension_enablement_service,
            workbench_extension_management_service: parts.workbench_extension_management_service,
            extension_services: parts.extension_services,
            telemetry_service: parts.telemetry_service,
        });
        this.subscribe();
        this
    }

    pub fn on_change(&self) -> Event<Option<ExtensionChange>> {
        self.on_change.event()
    }

    pub fn on_reset(&self) -> Event<()> {
        self.on_reset.event()
    }

    fn install_service(&self) -> Option<Rc<dyn IExtensionInstallService>> {
        self.server.extension_management_service.clone().as_install_service()
    }

    /// Liga os eventos do servidor (o construtor do original).
    fn subscribe(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let service = &self.server.extension_management_service;
        if let Some(install) = self.install_service() {
            let w = weak.clone();
            self.base.register(install.on_install_extension().listen(move |e| {
                if let Some(this) = w.upgrade() {
                    this.on_install_extension(e);
                }
            }));
            let w = weak.clone();
            self.base.register(service.on_did_install_extensions().listen(move |e| {
                if let Some(this) = w.upgrade() {
                    this.on_did_install_extensions(e.clone());
                }
            }));
            let w = weak.clone();
            self.base.register(install.on_uninstall_extension().listen(move |e| {
                if let Some(this) = w.upgrade() {
                    this.on_uninstall_extension(&e.identifier);
                }
            }));
            let w = weak.clone();
            self.base.register(service.on_did_uninstall_extension().listen(move |e| {
                if let Some(this) = w.upgrade() {
                    this.on_did_uninstall_extension(e);
                }
            }));
            let w = weak.clone();
            self.base.register(install.on_did_update_extension_metadata().listen(move |e| {
                if let Some(this) = w.upgrade() {
                    this.on_did_update_extension_metadata(e.local.clone());
                }
            }));
        } else {
            let w = weak.clone();
            self.base.register(service.on_did_install_extensions().listen(move |e| {
                if let Some(this) = w.upgrade() {
                    this.on_did_install_extensions(e.clone());
                }
            }));
            let w = weak.clone();
            self.base.register(service.on_did_uninstall_extension().listen(move |e| {
                if let Some(this) = w.upgrade() {
                    this.on_did_uninstall_extension(e);
                }
            }));
        }
        let w = weak.clone();
        self.base.register(service.on_did_change_profile().listen(move |_| {
            if let Some(this) = w.upgrade() {
                this.reset();
            }
        }));
        let w = weak.clone();
        self.base.register(self.extension_enablement_service.on_enablement_changed().listen(move |e| {
            if let Some(this) = w.upgrade() {
                this.on_enablement_changed(e);
            }
        }));
        let w = weak.clone();
        self.base.register(self.on_change.event().listen(move |_| {
            if let Some(this) = w.upgrade() {
                *this.local_cache.borrow_mut() = None;
            }
        }));
        let w = weak.clone();
        self.base.register(self.on_reset.event().listen(move |_| {
            if let Some(this) = w.upgrade() {
                *this.local_cache.borrow_mut() = None;
            }
        }));
        if self.is_workspace_server {
            let workbench = self.workbench_extension_management_service.clone();
            let w = weak.clone();
            self.base.register(workbench.on_install_extension().listen(move |e| {
                if let Some(this) = w.upgrade() {
                    if e.workspace_scoped == Some(true) {
                        this.on_install_extension(e);
                    }
                }
            }));
            let w = weak.clone();
            self.base.register(workbench.on_did_install_extensions().listen(move |e| {
                if let Some(this) = w.upgrade() {
                    let result: Vec<InstallExtensionResult> = e.iter().filter(|e| e.workspace_scoped == Some(true)).cloned().collect();
                    if !result.is_empty() {
                        this.on_did_install_extensions(result);
                    }
                }
            }));
            let w = weak.clone();
            self.base.register(workbench.on_uninstall_extension().listen(move |e| {
                if let Some(this) = w.upgrade() {
                    if e.workspace_scoped == Some(true) {
                        this.on_uninstall_extension(&e.identifier);
                    }
                }
            }));
            let w = weak;
            self.base.register(workbench.on_did_uninstall_extension().listen(move |e| {
                if let Some(this) = w.upgrade() {
                    if e.workspace_scoped == Some(true) {
                        this.on_did_uninstall_extension(e);
                    }
                }
            }));
        }
    }

    fn create_extension(&self, local: Option<Rc<LocalExtension>>, gallery: Option<Rc<GalleryExtension>>) -> Rc<Extension> {
        Rc::new(Extension::new(
            self.state_provider.clone(),
            self.runtime_state_provider.clone(),
            Some(self.server.clone()),
            local,
            gallery,
            None,
            (self.extension_services)(),
        ))
    }

    fn fire_change(&self, extension: Option<Rc<Extension>>) {
        self.on_change.fire(extension.map(|extension| ExtensionChange { extension, operation: None }));
    }

    /// `local`.
    pub fn local(&self) -> Vec<Rc<Extension>> {
        if let Some(cache) = self.local_cache.borrow().clone() {
            return cache;
        }
        let installed = self.installed.borrow().clone();
        let mut local: Vec<Rc<Extension>> = installed.clone();
        for extension in self.installing.borrow().iter() {
            if !installed.iter().any(|installed| are_same_extensions(&installed.identifier(), &extension.identifier())) {
                local.push(extension.clone());
            }
        }
        *self.local_cache.borrow_mut() = Some(local.clone());
        local
    }

    /// `queryInstalled(productVersion)`.
    pub fn query_installed(&self, product_version: ProductVersion) -> Promise<Vec<Rc<Extension>>> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(Vec::new()) };
        async_fn(async move {
            this.fetch_installed_extensions(Some(product_version)).await?;
            this.on_change.fire(None);
            Ok(this.local())
        })
    }

    /// `syncInstalledExtensionsWithGallery(galleryExtensions, productVersion, flagExtensionsMissingFromGallery)`.
    pub fn sync_installed_extensions_with_gallery(
        &self,
        gallery_extensions: Vec<GalleryExtension>,
        product_version: ProductVersion,
        flag_extensions_missing_from_gallery: Option<Vec<ExtensionInfo>>,
    ) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            let extensions = this.map_installed_extension_with_compatible_gallery_extension(gallery_extensions, product_version).await?;
            for (extension, gallery) in extensions {
                // update metadata of the extension if it does not exist
                if let Some(local) = extension.local() {
                    if local.extension_type != ExtensionType::System && local.identifier.uuid.is_none() {
                        let updated = this.update_metadata(&local, &gallery).await?;
                        extension.set_local(Some(updated));
                    }
                }
                let needs_update = match extension.gallery() {
                    None => true,
                    Some(current) => current.version != gallery.version || current.properties.target_platform != gallery.properties.target_platform,
                };
                if needs_update {
                    extension.set_gallery(Some(Rc::new(gallery)));
                    this.on_change.fire(Some(ExtensionChange { extension, operation: None }));
                }
            }
            // Detect extensions that do not have a corresponding gallery entry.
            if let Some(flagged) = flag_extensions_missing_from_gallery {
                let mut extensions_to_query: Vec<Rc<Extension>> = Vec::new();
                for extension in this.local() {
                    // Extension is already paired with a gallery object
                    if extension.gallery().is_some() {
                        continue;
                    }
                    // Already flagged as missing from gallery
                    if extension.missing_from_gallery() {
                        continue;
                    }
                    // A UUID indicates extension originated from gallery
                    if extension.identifier().uuid.is_none() {
                        continue;
                    }
                    // Extension is not present in the set we are concerned about
                    if !flagged.iter().any(|f| are_same_extensions(&je_extensions::manifest::ExtensionIdentifierInfo { id: f.id.clone(), uuid: f.uuid.clone() }, &extension.identifier())) {
                        continue;
                    }
                    extensions_to_query.push(extension);
                }
                if !extensions_to_query.is_empty() {
                    let infos = extensions_to_query
                        .iter()
                        .map(|e| ExtensionInfo { id: e.identifier().id, uuid: e.identifier().uuid, version: Some(e.version()), ..ExtensionInfo::default() })
                        .collect();
                    let query_result = this.gallery_service.get_extensions(infos, ExtensionQueryOptions::default(), CancellationToken::None).await?;
                    let mut queried_ids: Vec<String> = Vec::new();
                    let mut missing_ids: Vec<String> = Vec::new();
                    for extension in extensions_to_query {
                        queried_ids.push(extension.identifier().id);
                        match query_result.iter().find(|g| are_same_extensions(&g.identifier, &extension.identifier())) {
                            Some(gallery) => extension.set_gallery(Some(Rc::new(gallery.clone()))),
                            None => {
                                extension.set_missing_from_gallery(true);
                                missing_ids.push(extension.identifier().id);
                            }
                        }
                        this.on_change.fire(Some(ExtensionChange { extension, operation: None }));
                    }
                    this.telemetry_service.public_log2(
                        "extensions:missingFromGallery",
                        Some(Dynamic::object([
                            ("queriedIds", Dynamic::string(&queried_ids.join(";"))),
                            ("missingIds", Dynamic::string(&missing_ids.join(";"))),
                        ])),
                    );
                }
            }
            Ok(())
        })
    }

    fn map_installed_extension_with_compatible_gallery_extension(
        &self,
        gallery_extensions: Vec<GalleryExtension>,
        product_version: ProductVersion,
    ) -> Promise<Vec<(Rc<Extension>, GalleryExtension)>> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(Vec::new()) };
        async_fn(async move {
            let mapped_extensions = this.map_installed_extension_with_gallery_extension(&gallery_extensions);
            let target_platform = match this.install_service() {
                Some(install) => install.get_target_platform().await?,
                None => TargetPlatform::Undefined,
            };
            let mut compatible_gallery_extensions: Vec<GalleryExtension> = Vec::new();
            let mut compatible_gallery_extensions_to_fetch: Vec<ExtensionInfo> = Vec::new();
            for (extension, gallery) in &mapped_extensions {
                if let Some(local) = extension.local() {
                    let compatible = this.gallery_service.is_extension_compatible(gallery, local.pre_release, target_platform, Some(product_version.clone())).await;
                    match compatible {
                        Ok(true) => compatible_gallery_extensions.push(gallery.clone()),
                        _ => compatible_gallery_extensions_to_fetch.push(ExtensionInfo {
                            id: local.identifier.id.clone(),
                            uuid: local.identifier.uuid.clone(),
                            pre_release: Some(local.pre_release),
                            ..ExtensionInfo::default()
                        }),
                    }
                }
            }
            if !compatible_gallery_extensions_to_fetch.is_empty() {
                let options = ExtensionQueryOptions {
                    target_platform: Some(target_platform),
                    compatible: true,
                    query_all_versions: true,
                    product_version: Some(product_version),
                    ..ExtensionQueryOptions::default()
                };
                let result = this.gallery_service.get_extensions(compatible_gallery_extensions_to_fetch, options, CancellationToken::None).await?;
                compatible_gallery_extensions.extend(result);
            }
            Ok(this.map_installed_extension_with_gallery_extension(&compatible_gallery_extensions))
        })
    }

    fn map_installed_extension_with_gallery_extension(&self, gallery_extensions: &[GalleryExtension]) -> Vec<(Rc<Extension>, GalleryExtension)> {
        let mut mapped_extensions: Vec<(Rc<Extension>, GalleryExtension)> = Vec::new();
        let mut by_uuid: HashMap<String, &GalleryExtension> = HashMap::new();
        let mut by_id: HashMap<String, &GalleryExtension> = HashMap::new();
        for gallery in gallery_extensions {
            if let Some(uuid) = &gallery.identifier.uuid {
                by_uuid.insert(uuid.clone(), gallery);
            }
            by_id.insert(gallery.identifier.id.to_lowercase(), gallery);
        }
        for installed in self.installed.borrow().iter() {
            if let Some(uuid) = installed.uuid() {
                if let Some(gallery) = by_uuid.get(&uuid) {
                    mapped_extensions.push((installed.clone(), (*gallery).clone()));
                    continue;
                }
            }
            if installed.local().is_none_or(|local| local.source != InstallSource::Resource) {
                if let Some(gallery) = by_id.get(&installed.identifier().id.to_lowercase()) {
                    mapped_extensions.push((installed.clone(), (*gallery).clone()));
                }
            }
        }
        mapped_extensions
    }

    fn update_metadata(&self, local_extension: &Rc<LocalExtension>, gallery: &GalleryExtension) -> Promise<Rc<LocalExtension>> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(local_extension.clone()) };
        let local_extension = local_extension.clone();
        let gallery = gallery.clone();
        async_fn(async move {
            let mut is_pre_release_version = false;
            if local_extension.manifest.version != gallery.version {
                this.telemetry_service.public_log2("galleryService:updateMetadata", None);
                let info = ExtensionInfo {
                    id: local_extension.identifier.id.clone(),
                    uuid: local_extension.identifier.uuid.clone(),
                    version: Some(local_extension.manifest.version.clone()),
                    ..ExtensionInfo::default()
                };
                let found = this.gallery_service.get_extensions(vec![info], ExtensionQueryOptions::default(), CancellationToken::None).await?;
                is_pre_release_version = found.first().is_some_and(|g| g.properties.is_pre_release_version);
            }
            let metadata = Metadata {
                id: gallery.identifier.uuid.clone(),
                publisher_display_name: Some(gallery.publisher_display_name.clone()),
                publisher_id: Some(gallery.publisher_id.clone()),
                is_pre_release_version: Some(is_pre_release_version),
                ..Metadata::default()
            };
            this.workbench_extension_management_service.update_metadata(&local_extension, metadata, None).await
        })
    }

    pub fn can_install(&self, gallery_extension: &GalleryExtension) -> Promise<Result<(), String>> {
        match self.install_service() {
            Some(install) => install.can_install(gallery_extension),
            None => Promise::resolved(Err(localize("cannot be installed", "The extension server cannot install extensions.", &[]))),
        }
    }

    fn on_install_extension(&self, event: &InstallExtensionEvent) {
        if let InstallExtensionSource::Gallery(source) = &event.source {
            let existing = self.installed.borrow().iter().find(|e| are_same_extensions(&e.identifier(), &source.identifier)).cloned();
            let extension = existing.unwrap_or_else(|| self.create_extension_from_gallery_source(source));
            self.installing.borrow_mut().push(extension.clone());
            self.on_change.fire(Some(ExtensionChange { extension, operation: None }));
        }
    }

    /// `createInstance(Extension, ..., undefined, source, undefined)`: o argumento `local` é `undefined` e o `gallery`, a fonte.
    fn create_extension_from_gallery_source(&self, source: &GalleryExtension) -> Rc<Extension> {
        self.create_extension(None, Some(Rc::new(source.clone())))
    }

    fn server_installed(&self, product_version: Option<ProductVersion>) -> Promise<Vec<ExtensionRef>> {
        match self.install_service() {
            Some(install) => install.get_installed_filtered(None, None, product_version, None),
            None => self.server.extension_management_service.get_installed(),
        }
    }

    fn fetch_installed_extensions(&self, product_version: Option<ProductVersion>) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            let extensions_control_manifest = this.server.extension_management_service.get_extensions_control_manifest().await?;
            let mut all = this.server_installed(product_version).await?;
            if this.is_workspace_server {
                all.extend(this.workbench_extension_management_service.get_installed_workspace_extensions(true).await?);
            }

            // dedup workspace, user and system extensions by giving priority to workspace first and then to user extension.
            let mut installed: Vec<ExtensionRef> = Vec::new();
            for extensions in group_by_extension(all, |e| &e.identifier) {
                if extensions.len() == 1 {
                    installed.extend(extensions);
                    continue;
                }
                let (mut workspace_extension, mut user_extension, mut system_extension) = (None, None, None);
                for extension in extensions {
                    if extension.is_workspace_scoped {
                        workspace_extension = Some(extension);
                    } else if extension.extension_type == ExtensionType::User {
                        user_extension = Some(extension);
                    } else {
                        system_extension = Some(extension);
                    }
                }
                if let Some(extension) = workspace_extension.or(user_extension).or(system_extension) {
                    installed.push(extension);
                }
            }

            let by_id: HashMap<String, Rc<Extension>> = this
                .installed
                .borrow()
                .iter()
                .map(|e| (e.local().map_or_else(|| e.identifier().id, |local| local.identifier.id.clone()), e.clone()))
                .collect();
            let next: Vec<Rc<Extension>> = installed
                .into_iter()
                .map(|local| {
                    let extension = by_id.get(&local.identifier.id).cloned().unwrap_or_else(|| this.create_extension(Some(local.clone()), None));
                    extension.set_local(Some(local.clone()));
                    extension.set_enablement_state(this.extension_enablement_service.get_enablement_state(&local));
                    extension.set_extensions_control_manifest(&extensions_control_manifest);
                    extension
                })
                .collect();
            *this.installed.borrow_mut() = next;
            Ok(())
        })
    }

    fn reset(&self) {
        let Some(this) = self.this.upgrade() else { return };
        this.installed.borrow_mut().clear();
        this.installing.borrow_mut().clear();
        this.uninstalling.borrow_mut().clear();
        let fetched = this.fetch_installed_extensions(None);
        fetched.then(move |()| this.on_reset.fire(()));
    }

    fn on_did_install_extensions(&self, results: Vec<InstallExtensionResult>) {
        let Some(this) = self.this.upgrade() else { return };
        let mut extensions: Vec<Rc<Extension>> = Vec::new();
        for event in &results {
            let local = event.local.clone();
            let gallery = match &event.source {
                Some(InstallExtensionSource::Gallery(gallery)) => Some(gallery.clone()),
                _ => None,
            };
            let is_location = matches!(event.source, Some(InstallExtensionSource::Location(_)));
            let installing_extension = gallery.as_ref().and_then(|gallery| this.installing.borrow().iter().find(|e| are_same_extensions(&e.identifier(), &gallery.identifier)).cloned());
            if let Some(installing) = &installing_extension {
                this.installing.borrow_mut().retain(|e| !identifiers_equal(e, installing));
            }

            let mut extension: Option<Rc<Extension>> = match installing_extension {
                Some(installing) => Some(installing),
                None if is_location || local.is_some() => Some(this.create_extension(local.clone(), None)),
                None => None,
            };
            if let Some(current) = extension.clone() {
                if let Some(local) = &local {
                    let installed = this.installed.borrow().iter().find(|e| are_same_extensions(&e.identifier(), &current.identifier())).cloned();
                    let target = match installed {
                        Some(installed) => installed,
                        None => {
                            this.installed.borrow_mut().push(current.clone());
                            current
                        }
                    };
                    target.set_local(Some(local.clone()));
                    if target.gallery().is_none() {
                        target.set_gallery(gallery.map(Rc::new));
                    }
                    target.set_enablement_state(this.extension_enablement_service.get_enablement_state(local));
                    extension = Some(target);
                }
                if let Some(extension) = &extension {
                    extensions.push(extension.clone());
                }
            }
            this.on_change.fire(match (&local, &extension) {
                (Some(_), Some(extension)) => Some(ExtensionChange { extension: extension.clone(), operation: Some(event.operation) }),
                _ => None,
            });
        }

        if !extensions.is_empty() {
            let manifest_promise = this.server.extension_management_service.get_extensions_control_manifest();
            let this = this.clone();
            async_fn(async move {
                let manifest = manifest_promise.await?;
                for extension in &extensions {
                    extension.set_extensions_control_manifest(&manifest);
                }
                this.match_installed_extensions_with_gallery(extensions).await
            })
            .then_result(|result| {
                // O original não trata a rejeição da promessa solta.
                let _ = result;
                Ok(())
            });
        }
    }

    fn on_did_update_extension_metadata(&self, local: ExtensionRef) {
        let extension = self.installed.borrow().iter().find(|e| are_same_extensions(&e.identifier(), &local.identifier)).cloned();
        if let Some(extension) = extension {
            if extension.local().is_some() {
                extension.set_local(Some(local));
                self.on_change.fire(Some(ExtensionChange { extension, operation: None }));
            }
        }
    }

    fn match_installed_extensions_with_gallery(&self, extensions: Vec<Rc<Extension>>) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            let to_match: Vec<Rc<Extension>> = extensions
                .iter()
                .filter(|e| e.local().is_some_and(|local| e.gallery().is_none() && local.source != InstallSource::Resource))
                .cloned()
                .collect();
            if to_match.is_empty() {
                return Ok(());
            }
            if !this.gallery_service.is_enabled() {
                return Ok(());
            }
            let target_platform = match this.install_service() {
                Some(install) => install.get_target_platform().await?,
                None => TargetPlatform::Undefined,
            };
            let infos = to_match
                .iter()
                .map(|e| ExtensionInfo {
                    id: e.identifier().id,
                    uuid: e.identifier().uuid,
                    pre_release: e.local().map(|local| local.pre_release),
                    ..ExtensionInfo::default()
                })
                .collect();
            let options = ExtensionQueryOptions { compatible: true, target_platform: Some(target_platform), ..ExtensionQueryOptions::default() };
            let gallery_extensions = this.gallery_service.get_extensions(infos, options, CancellationToken::None).await?;
            for extension in extensions {
                if let Some(compatible) = gallery_extensions.iter().find(|e| are_same_extensions(&e.identifier, &extension.identifier())) {
                    extension.set_gallery(Some(Rc::new(compatible.clone())));
                    this.on_change.fire(Some(ExtensionChange { extension, operation: None }));
                }
            }
            Ok(())
        })
    }

    fn on_uninstall_extension(&self, identifier: &je_extensions::manifest::ExtensionIdentifierInfo) {
        let extension = self.installed.borrow().iter().find(|e| are_same_extensions(&e.identifier(), identifier)).cloned();
        if let Some(extension) = extension {
            let uninstalling = self.uninstalling.borrow().iter().find(|e| are_same_extensions(&e.identifier(), identifier)).cloned().unwrap_or(extension);
            {
                let mut list = self.uninstalling.borrow_mut();
                list.retain(|e| !are_same_extensions(&e.identifier(), identifier));
                list.insert(0, uninstalling.clone());
            }
            self.on_change.fire(Some(ExtensionChange { extension: uninstalling, operation: None }));
        }
    }

    fn on_did_uninstall_extension(&self, event: &DidUninstallExtensionEvent) {
        let identifier = &event.identifier;
        let uninstalled = self
            .uninstalling
            .borrow()
            .iter()
            .find(|e| are_same_extensions(&e.identifier(), identifier))
            .cloned()
            .or_else(|| self.installed.borrow().iter().find(|e| are_same_extensions(&e.identifier(), identifier)).cloned());
        self.uninstalling.borrow_mut().retain(|e| !are_same_extensions(&e.identifier(), identifier));
        if event.error.is_none() {
            self.installed.borrow_mut().retain(|e| !are_same_extensions(&e.identifier(), identifier));
        }
        if let Some(extension) = uninstalled {
            self.on_change.fire(Some(ExtensionChange { extension, operation: None }));
        }
    }

    fn on_enablement_changed(&self, platform_extensions: &[ExtensionRef]) {
        let extensions: Vec<Rc<Extension>> =
            self.local().into_iter().filter(|e| platform_extensions.iter().any(|p| are_same_extensions(&e.identifier(), &p.identifier))).collect();
        for extension in extensions {
            if let Some(local) = extension.local() {
                let enablement_state = self.extension_enablement_service.get_enablement_state(&local);
                if enablement_state != extension.enablement_state() {
                    extension.set_enablement_state(enablement_state);
                    self.on_change.fire(Some(ExtensionChange { extension, operation: None }));
                }
            }
        }
    }

    /// `getExtensionState(extension)`.
    pub fn get_extension_state(&self, extension: &Extension) -> ExtensionState {
        if let Some(gallery) = extension.gallery() {
            if self.installing.borrow().iter().any(|e| e.gallery().is_some_and(|g| are_same_extensions(&g.identifier, &gallery.identifier))) {
                return ExtensionState::Installing;
            }
        }
        if self.uninstalling.borrow().iter().any(|e| are_same_extensions(&e.identifier(), &extension.identifier())) {
            return ExtensionState::Uninstalling;
        }
        let found = self.installed.borrow().iter().any(|e| {
            std::ptr::eq(Rc::as_ptr(e), extension)
                || match (e.gallery(), extension.gallery()) {
                    (Some(a), Some(b)) => are_same_extensions(&a.identifier, &b.identifier),
                    _ => false,
                }
        });
        if found { ExtensionState::Installed } else { ExtensionState::Uninstalled }
    }
}

impl IDisposable for Extensions {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// ExtensionsWorkbenchService
// ---------------------------------------------------------------------------------------------------------------------

/// Os serviços que o construtor do original recebe por decorador.
pub struct ExtensionsWorkbenchServiceParts {
    pub extension_management_service: Rc<dyn IWorkbenchExtensionInstallService>,
    pub gallery_service: Rc<dyn IExtensionGalleryService>,
    /// A fábrica dos serviços de cada `Extension` (o `createInstance(Extension, ...)` do original).
    pub extension_services: Rc<dyn Fn() -> ExtensionServices>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub telemetry_service: Rc<dyn ITelemetryService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub extension_enablement_service: Rc<dyn IWorkbenchExtensionEnablementService>,
    pub host_service: Rc<dyn IHostService>,
    pub progress_service: Rc<dyn IProgressService>,
    pub extension_management_server_service: Rc<dyn IExtensionManagementServerService>,
    pub extensions_sync_management_service: Rc<dyn IIgnoredExtensionsManagementService>,
    pub user_data_auto_sync_service: Rc<dyn IUserDataAutoSyncService>,
    pub product_service: Rc<dyn IProductService>,
    pub extension_manifest_properties_service: Rc<dyn IExtensionManifestPropertiesService>,
    pub log_service: LogServiceRef,
    pub extension_service: Rc<dyn IExtensionService>,
    pub lifecycle_service: Rc<dyn ILifecycleService>,
    pub user_data_profile_service: Rc<dyn IUserDataProfileService>,
    pub user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    pub storage_service: Rc<dyn StorageService>,
    pub dialog_service: Rc<dyn IDialogService>,
    pub user_data_sync_enablement_service: Rc<dyn IUserDataSyncEnablementService>,
    pub update_service: Rc<dyn UpdateService>,
    pub workspace_context_service: Rc<dyn IWorkspaceContextService>,
    pub allowed_extensions_service: Rc<AllowedExtensionsService>,
    pub metered_connection_service: Rc<dyn je_platform::metered_connection::IMeteredConnectionService>,
    pub uri_identity_service: Rc<dyn je_platform::uri_identity::uri_identity::IUriIdentityService>,
}

/// `IIgnoredExtensionsManagementService` (`platform/userDataSync/common/ignoredExtensions.ts`), recorte do que o
/// `ExtensionsWorkbenchService` chama: o serviço ainda não foi portado.
pub trait IIgnoredExtensionsManagementService {
    fn has_to_never_sync_extension(&self, extension_id: &str) -> bool;
    fn has_to_always_sync_extension(&self, extension_id: &str) -> bool;
    fn update_ignored_extensions(&self, ignored_extension_id: &str, ignore: bool) -> Promise<()>;
}

/// `ExtensionsWorkbenchService`.
pub struct ExtensionsWorkbenchService {
    this: Weak<ExtensionsWorkbenchService>,
    base: Rc<Disposable>,
    store: Rc<DisposableStore>,

    local_extensions: Option<ServerExtensions>,
    remote_extensions: Option<ServerExtensions>,
    web_extensions: Option<ServerExtensions>,
    extensions_servers: Vec<ServerExtensions>,

    updates_check_delayer: Rc<ThrottledDelayer<()>>,
    auto_update_delayer: Rc<ThrottledDelayer<()>>,

    on_change: Rc<Emitter<Option<Rc<Extension>>>>,
    on_reset: Rc<Emitter<()>>,

    installing: RefCell<Vec<Rc<Extension>>>,
    tasks_in_progress: RefCell<Vec<CancelablePromise<()>>>,
    installed_cache: RefCell<Option<Vec<Rc<Extension>>>>,
    local_cache: RefCell<Option<Vec<Rc<Extension>>>>,
    enabled_auto_update_extensions_value: RefCell<Option<String>>,
    disabled_auto_update_extensions_value: RefCell<Option<String>>,
    auto_restart_listener: RefCell<Option<DisposableRef>>,
    disposed: Cell<bool>,

    pub when_initialized: RefCell<Option<Promise<()>>>,

    p: ExtensionsWorkbenchServiceParts,
}

fn extension_error(message: String, code: ExtensionManagementErrorCode) -> Error {
    Error::named(format!("{code:?}"), message).with_prop("code", format!("{code:?}"))
}

impl ExtensionsWorkbenchService {
    /// `constructor(...)`: monta os `Extensions` de cada servidor e começa `initialize`.
    pub fn new(parts: ExtensionsWorkbenchServiceParts) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        let on_change = Emitter::<Option<Rc<Extension>>>::new();
        let on_reset = Emitter::<()>::new();
        base.register(on_change.clone());
        base.register(on_reset.clone());
        let updates_check_delayer = Rc::new(ThrottledDelayer::<()>::new(UPDATES_CHECK_INTERVAL_MS));
        let auto_update_delayer = Rc::new(ThrottledDelayer::<()>::new(1000));
        base.register(updates_check_delayer.clone());
        base.register(auto_update_delayer.clone());

        let server_service = parts.extension_management_server_service.clone();
        let local_server = server_service.local_extension_management_server();
        let remote_server = server_service.remote_extension_management_server();
        let web_server = server_service.web_extension_management_server();

        let this = Rc::new_cyclic(|weak: &Weak<ExtensionsWorkbenchService>| {
            let make = |server: Rc<IExtensionManagementServer>, is_workspace_server: bool| -> ServerExtensions {
                let state_weak = weak.clone();
                let runtime_weak = weak.clone();
                let extensions = Extensions::new(ExtensionsParts {
                    server,
                    state_provider: Rc::new(move |ext: &Extension| state_weak.upgrade().map_or(ExtensionState::Uninstalled, |service| service.get_extension_state(ext))),
                    runtime_state_provider: Rc::new(move |ext: &Extension| runtime_weak.upgrade().and_then(|service| service.get_runtime_state(ext))),
                    is_workspace_server,
                    gallery_service: parts.gallery_service.clone(),
                    extension_enablement_service: parts.extension_enablement_service.clone(),
                    workbench_extension_management_service: parts.extension_management_service.clone(),
                    extension_services: parts.extension_services.clone(),
                    telemetry_service: parts.telemetry_service.clone(),
                });
                base.register(extensions.clone());
                extensions
            };
            let local_extensions = local_server.clone().map(|server| make(server, remote_server.is_none()));
            let remote_extensions = remote_server.clone().map(|server| make(server, true));
            let web_extensions = web_server.clone().map(|server| make(server, remote_server.is_none() && local_server.is_none()));
            let extensions_servers: Vec<ServerExtensions> =
                [local_extensions.clone(), remote_extensions.clone(), web_extensions.clone()].into_iter().flatten().collect();
            Self {
                this: weak.clone(),
                base: base.clone(),
                store: store.clone(),
                local_extensions,
                remote_extensions,
                web_extensions,
                extensions_servers,
                updates_check_delayer: updates_check_delayer.clone(),
                auto_update_delayer: auto_update_delayer.clone(),
                on_change: on_change.clone(),
                on_reset: on_reset.clone(),
                installing: RefCell::new(Vec::new()),
                tasks_in_progress: RefCell::new(Vec::new()),
                installed_cache: RefCell::new(None),
                local_cache: RefCell::new(None),
                enabled_auto_update_extensions_value: RefCell::new(None),
                disabled_auto_update_extensions_value: RefCell::new(None),
                auto_restart_listener: RefCell::new(None),
                disposed: Cell::new(false),
                when_initialized: RefCell::new(None),
                p: parts,
            }
        });

        for extensions in &this.extensions_servers {
            let weak = Rc::downgrade(&this);
            this.base.register(extensions.on_change().listen(move |e| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_extensions(e.as_ref().map(|e| e.extension.clone()));
                }
            }));
            let weak = Rc::downgrade(&this);
            this.base.register(extensions.on_reset().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.reset();
                }
            }));
        }

        let initialized = this.initialize();
        *this.when_initialized.borrow_mut() = Some(initialized);
        this
    }

    pub fn on_change(&self) -> Event<Option<Rc<Extension>>> {
        self.on_change.event()
    }

    pub fn on_reset(&self) -> Event<()> {
        self.on_reset.event()
    }

    fn weak(&self) -> Weak<ExtensionsWorkbenchService> {
        self.this.clone()
    }

    fn is_disposed(&self) -> bool {
        self.disposed.get()
    }

    /// `dispose()`.
    pub fn dispose(&self) {
        self.disposed.set(true);
        self.store.dispose();
        self.base.dispose();
    }

    fn initialize(&self) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            // initialize local extensions
            let queried = this.query_local(None);
            let registered = this.p.extension_service.when_installed_extensions_registered();
            queried.await?;
            registered.await?;
            if this.is_disposed() {
                return Ok(());
            }
            let running = this.p.extension_service.extensions();
            this.on_did_change_running_extensions(running, Vec::new());
            let weak = this.weak();
            this.base.register(this.p.extension_service.on_did_change_extensions().listen(move |e| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_running_extensions(e.added.clone(), e.removed.clone());
                }
            }));

            this.p.lifecycle_service.when(LifecyclePhase::Eventually).await?;
            if this.is_disposed() {
                return Ok(());
            }
            this.initialize_auto_update();
            Ok(())
        })
    }

    fn initialize_auto_update(&self) {
        let Some(this) = self.this.upgrade() else { return };
        // Register listeners for auto updates
        let weak = self.weak();
        self.base.register(self.p.configuration_service.on_did_change_configuration().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            if e.affects_configuration(AUTO_UPDATE_CONFIGURATION_KEY, None) {
                if this.is_auto_update_enabled() {
                    this.eventually_auto_update_extensions();
                }
                // The auto update value affects whether an extension is shown as delayed
                this.on_change.fire(None);
            }
            if e.affects_configuration(AUTO_UPDATE_DELAY_CONFIGURATION_KEY, None) {
                if this.is_auto_update_enabled() {
                    this.eventually_auto_update_extensions();
                }
                // The delay affects whether an extension is shown as delayed
                this.on_change.fire(None);
            }
            if e.affects_configuration(AUTO_CHECK_UPDATES_CONFIGURATION_KEY, None) && this.is_auto_check_updates_enabled() {
                this.check_for_updates(Some("Enabled auto check updates".to_owned()), false);
            }
        }));
        let weak = self.weak();
        self.base.register(self.p.extension_enablement_service.on_enablement_changed().listen(move |platform_extensions| {
            let Some(this) = weak.upgrade() else { return };
            if this.is_auto_check_updates_enabled()
                && this.get_auto_update_value() == AutoUpdateConfigurationValue::On
                && platform_extensions.iter().any(|e| this.p.extension_enablement_service.is_enabled(e))
            {
                this.check_for_updates(Some("Extension enablement changed".to_owned()), false);
            }
        }));
        let weak = self.weak();
        self.base.register(self.p.update_service.on_state_change().listen(move |state| {
            let Some(this) = weak.upgrade() else { return };
            let triggers = match state {
                State::CheckingForUpdates(checking) => checking.explicit,
                State::AvailableForDownload(_) | State::Downloaded(_) => true,
                _ => false,
            };
            if triggers {
                this.p.telemetry_service.public_log2("extensions:updatecheckonproductupdate", None);
                if this.is_auto_check_updates_enabled() {
                    this.check_for_updates(Some("Product update".to_owned()), false);
                }
            }
        }));
        let weak = self.weak();
        self.base.register(self.p.allowed_extensions_service.on_did_change_allowed_extensions_config_value().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                if this.is_auto_check_updates_enabled() {
                    this.check_for_updates(Some("Allowed extensions changed".to_owned()), false);
                }
            }
        }));
        let weak = self.weak();
        self.base.register(self.p.metered_connection_service.on_did_change_is_connection_metered().listen(move |_| {
            let Some(this) = weak.upgrade() else { return };
            if this.is_auto_check_updates_enabled() {
                this.check_for_updates(Some("Connection is no longer metered".to_owned()), false);
            }
            if Platform::detect().is_web && !this.is_auto_update_enabled() {
                this.auto_update_builtin_extensions();
            }
        }));

        // Check for updates
        this.eventually_check_for_updates(true);

        if Platform::detect().is_web {
            this.sync_pinned_builtin_extensions();
            // Always auto update builtin extensions in web
            if !this.is_auto_update_enabled() {
                this.auto_update_builtin_extensions();
            }
        }

        this.register_auto_restart_listener();
        let weak = self.weak();
        self.base.register(self.p.configuration_service.on_did_change_configuration().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                if e.affects_configuration(AUTO_RESTART_CONFIGURATION_KEY, None) {
                    this.register_auto_restart_listener();
                }
            }
        }));
    }

    fn is_auto_update_enabled(&self) -> bool {
        if self.p.metered_connection_service.is_connection_metered() {
            return false;
        }
        self.get_auto_update_value() != AutoUpdateConfigurationValue::Off
    }

    /// `getAutoUpdateValue()`.
    pub fn get_auto_update_value(&self) -> AutoUpdateConfigurationValue {
        let auto_update = self.p.configuration_service.get_value(Some(AUTO_UPDATE_CONFIGURATION_KEY), None);
        let off = match &auto_update {
            Dynamic::Bool(false) => true,
            other => matches!(dynamic_text(other).as_deref(), Some("off" | "onlySelectedExtensions")),
        };
        if off { AutoUpdateConfigurationValue::Off } else { AutoUpdateConfigurationValue::On }
    }

    /// `isAutoUpdateDelayed(extension)`.
    pub fn is_auto_update_delayed(&self, extension: &Rc<Extension>) -> bool {
        if !extension.outdated() {
            return false;
        }
        if !self.should_auto_update_extension(extension) {
            return false;
        }
        self.get_auto_update_delay_remaining(extension) > 0.0
    }

    /// `getAutoUpdateDelayRemaining(extension)`, em milissegundos.
    pub fn get_auto_update_delay_remaining(&self, extension: &Rc<Extension>) -> f64 {
        // Extensions from publishers trusted by the product are auto updated without delay.
        if self.is_from_trusted_publisher(extension) {
            return 0.0;
        }
        let last_updated = extension.gallery().map_or(0.0, |gallery| gallery.last_updated);
        if !last_updated.is_finite() || last_updated == 0.0 {
            return 0.0;
        }
        let elapsed = je_base::runtime::date_now() as f64 - last_updated;
        if elapsed < 0.0 {
            // Future timestamp (clock skew) - treat as not delayed
            return 0.0;
        }
        (self.get_auto_update_delay() - elapsed).max(0.0)
    }

    /// `getAutoUpdateDelay()`, em milissegundos.
    pub fn get_auto_update_delay(&self) -> f64 {
        let delay_hours = match self.p.configuration_service.get_value(Some(AUTO_UPDATE_DELAY_CONFIGURATION_KEY), None) {
            Dynamic::Number(hours) => hours,
            _ => 12.0,
        };
        delay_hours * 60.0 * 60.0 * 1000.0
    }

    fn is_from_trusted_publisher(&self, extension: &Rc<Extension>) -> bool {
        let trusted = self.p.product_service.trusted_extension_publishers();
        if trusted.is_empty() {
            return false;
        }
        trusted.contains(&extension.publisher().to_lowercase()) || trusted.contains(&extension.publisher_display_name().to_lowercase())
    }

    /// `updateAutoUpdateForAllExtensions(isAutoUpdateEnabled)`.
    pub fn update_auto_update_for_all_extensions(&self, is_auto_update_enabled: bool) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            let was_auto_update_enabled = this.is_auto_update_enabled();
            if was_auto_update_enabled == is_auto_update_enabled {
                return Ok(());
            }
            let mut base = BaseDialogOptions::new(if is_auto_update_enabled {
                localize("confirmEnableAutoUpdate", "Do you want to enable auto update for extensions?", &[])
            } else {
                localize("confirmDisableAutoUpdate", "Do you want to disable auto update for extensions?", &[])
            });
            base.title = Some(localize("confirmEnableDisableAutoUpdate", "Auto Update Extensions", &[]));
            base.detail = Some(DialogDetail::Text(localize(
                "confirmEnableDisableAutoUpdateDetail",
                "This will reset any auto update settings you have set for individual extensions.",
                &[],
            )));
            let result = this.p.dialog_service.confirm(Confirmation { base, primary_button: None, cancel_button: None }).await?;
            if !result.confirmed {
                return Ok(());
            }

            // Reset extensions enabled for auto update first to prevent them from being updated
            this.set_enabled_auto_update_extensions(&[]);

            this.p
                .configuration_service
                .update_value(AUTO_UPDATE_CONFIGURATION_KEY, Dynamic::string(if is_auto_update_enabled { "on" } else { "off" }), None, None)
                .await?;

            this.set_disabled_auto_update_extensions(&[]);
            this.update_extensions_pinned_state(!is_auto_update_enabled).await?;
            this.on_change.fire(None);
            Ok(())
        })
    }

    fn register_auto_restart_listener(&self) {
        if let Some(previous) = self.auto_restart_listener.borrow_mut().take() {
            previous.dispose();
        }
        if matches!(self.p.configuration_service.get_value(Some(AUTO_RESTART_CONFIGURATION_KEY), None), Dynamic::Bool(true)) {
            let weak = self.weak();
            let listener = self.p.host_service.on_did_change_focus().listen(move |focus| {
                let Some(this) = weak.upgrade() else { return };
                if !focus && matches!(this.p.configuration_service.get_value(Some(AUTO_RESTART_CONFIGURATION_KEY), None), Dynamic::Bool(true)) {
                    this.update_running_extensions(None, true);
                }
            });
            *self.auto_restart_listener.borrow_mut() = Some(listener);
        }
    }

    fn on_did_change_running_extensions(&self, added: Vec<ExtensionDescription>, removed: Vec<ExtensionDescription>) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            let mut changed_extensions: Vec<Rc<Extension>> = Vec::new();
            let mut extensions_to_fetch: Vec<ExtensionDescription> = Vec::new();
            for desc in added {
                let id = je_extensions::manifest::ExtensionIdentifierInfo { id: desc.identifier.value.clone(), uuid: desc.uuid.clone() };
                match this.installed().into_iter().find(|e| are_same_extensions(&id, &e.identifier())) {
                    Some(extension) => changed_extensions.push(extension),
                    None => extensions_to_fetch.push(desc),
                }
            }
            let mut workspace_extensions: Vec<ExtensionDescription> = Vec::new();
            for desc in removed {
                if this.p.workspace_context_service.is_inside_workspace(&URI::parse(desc.extension_location.as_str(), false)) {
                    workspace_extensions.push(desc);
                } else {
                    extensions_to_fetch.push(desc);
                }
            }
            if !extensions_to_fetch.is_empty() {
                let infos = extensions_to_fetch
                    .iter()
                    .map(|e| ExtensionInfo { id: e.identifier.value.clone(), uuid: e.uuid.clone(), ..ExtensionInfo::default() })
                    .collect();
                let extensions = this.get_extensions(infos, ExtensionQueryOptions::default(), CancellationToken::None).await?;
                changed_extensions.extend(extensions);
            }
            if !workspace_extensions.is_empty() {
                let locations = workspace_extensions.iter().map(|e| URI::parse(e.extension_location.as_str(), false)).collect();
                let extensions = this.get_resource_extensions(locations, true).await?;
                changed_extensions.extend(extensions);
            }
            for changed_extension in changed_extensions {
                this.on_change.fire(Some(changed_extension));
            }
            Ok(())
        })
    }

    fn update_extensions_pinned_state(&self, pinned: bool) -> Promise<()> {
        let service = self.p.extension_management_service.clone();
        self.with_progress_options(
            ProgressLocationId::Location(ProgressLocation::Extensions),
            Some(localize("updatingExtensions", "Updating Extensions Auto Update State", &[])),
            move || service.reset_pinned_state_for_all_user_extensions(pinned),
        )
    }

    fn reset(&self) {
        for task in self.tasks_in_progress.borrow_mut().drain(..) {
            task.cancel();
        }
        self.installing.borrow_mut().clear();
        self.on_did_change_extensions(None);
        self.on_reset.fire(());
    }

    fn on_did_change_extensions(&self, extension: Option<Rc<Extension>>) {
        *self.installed_cache.borrow_mut() = None;
        *self.local_cache.borrow_mut() = None;
        self.on_change.fire(extension);
    }

    /// `local`.
    pub fn local(&self) -> Vec<Rc<Extension>> {
        if let Some(cache) = self.local_cache.borrow().clone() {
            return cache;
        }
        let local = if self.extensions_servers.len() == 1 {
            self.installed()
        } else {
            let mut local = Vec::new();
            for extensions in group_extensions(self.installed()) {
                local.push(self.get_primary_extension(extensions));
            }
            local
        };
        *self.local_cache.borrow_mut() = Some(local.clone());
        local
    }

    /// `installed`.
    pub fn installed(&self) -> Vec<Rc<Extension>> {
        if let Some(cache) = self.installed_cache.borrow().clone() {
            return cache;
        }
        let mut installed = Vec::new();
        for extensions in &self.extensions_servers {
            installed.extend(extensions.local());
        }
        *self.installed_cache.borrow_mut() = Some(installed.clone());
        installed
    }

    /// `outdated`.
    pub fn outdated(&self) -> Vec<Rc<Extension>> {
        self.installed().into_iter().filter(|e| e.outdated() && e.local().is_some() && e.state() == ExtensionState::Installed).collect()
    }

    /// `queryLocal(server?)`.
    pub fn query_local(&self, server: Option<Rc<IExtensionManagementServer>>) -> Promise<Vec<Rc<Extension>>> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(Vec::new()) };
        async_fn(async move {
            if let Some(server) = &server {
                let candidates = [
                    (&this.local_extensions, this.p.extension_management_server_service.local_extension_management_server()),
                    (&this.remote_extensions, this.p.extension_management_server_service.remote_extension_management_server()),
                    (&this.web_extensions, this.p.extension_management_server_service.web_extension_management_server()),
                ];
                for (extensions, candidate) in candidates {
                    if let (Some(extensions), Some(candidate)) = (extensions, candidate) {
                        if Rc::ptr_eq(&candidate, server) {
                            return extensions.query_installed(this.get_product_version()).await;
                        }
                    }
                }
            }
            for extensions in [&this.local_extensions, &this.remote_extensions, &this.web_extensions].into_iter().flatten() {
                if let Err(error) = extensions.query_installed(this.get_product_version()).await {
                    this.p.log_service.error(error.message.as_str());
                }
            }
            Ok(this.local())
        })
    }

    /// `queryGallery(options, token)` (o `queryGallery(token)` é o mesmo com opções vazias).
    pub fn query_gallery(&self, options: GalleryQueryOptions, token: CancellationToken) -> Promise<Rc<dyn IPager<Rc<Extension>>>> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(single_page_pager(Vec::new())) };
        async_fn(async move {
            if !this.p.gallery_service.is_enabled() {
                return Ok(single_page_pager(Vec::new()));
            }
            let mut options = options;
            options.text = options.text.map(|text| this.resolve_query_text(&text));
            if !options.include_pre_release {
                options.include_pre_release = options.include_pre_release || this.p.extension_management_service.prefer_pre_releases();
            }

            let extensions_control_manifest = this.p.extension_management_service.get_extensions_control_manifest().await?;
            let pager = this.p.gallery_service.query(options, token).await?;
            this.sync_installed_extensions_with_gallery(pager.first_page(), None);
            let first_page = pager.first_page().iter().map(|gallery| this.from_gallery(gallery, &extensions_control_manifest)).collect();
            let result: Rc<dyn IPager<Rc<Extension>>> = Rc::new(WorkbenchPager {
                service: this.clone(),
                pager,
                control_manifest: extensions_control_manifest,
                first_page,
            });
            Ok(result)
        })
    }

    /// `getExtensions(extensionInfos, options, token)`.
    pub fn get_extensions(&self, extension_infos: Vec<ExtensionInfo>, options: ExtensionQueryOptions, token: CancellationToken) -> Promise<Vec<Rc<Extension>>> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(Vec::new()) };
        async_fn(async move {
            if !this.p.gallery_service.is_enabled() {
                return Ok(Vec::new());
            }
            let prefer = this.p.extension_management_service.prefer_pre_releases();
            let infos: Vec<ExtensionInfo> = extension_infos
                .into_iter()
                .map(|mut info| {
                    info.pre_release = Some(info.pre_release.unwrap_or(prefer));
                    info
                })
                .collect();
            let extensions_control_manifest = this.p.extension_management_service.get_extensions_control_manifest().await?;
            let gallery_extensions = this.p.gallery_service.get_extensions(infos, options, token).await?;
            this.sync_installed_extensions_with_gallery(gallery_extensions.clone(), None);
            Ok(gallery_extensions.iter().map(|gallery| this.from_gallery(gallery, &extensions_control_manifest)).collect())
        })
    }

    /// `getResourceExtensions(locations, isWorkspaceScoped)`.
    pub fn get_resource_extensions(&self, locations: Vec<URI>, is_workspace_scoped: bool) -> Promise<Vec<Rc<Extension>>> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(Vec::new()) };
        async_fn(async move {
            let resource_extensions = this.p.extension_management_service.get_resource_extensions(locations).await?;
            Ok(resource_extensions
                .into_iter()
                .map(|resource_extension| {
                    this.get_installed_extension_matching_location(&resource_extension.location).unwrap_or_else(|| {
                        let weak_state = this.weak();
                        let weak_runtime = this.weak();
                        Rc::new(Extension::new(
                            Rc::new(move |ext: &Extension| weak_state.upgrade().map_or(ExtensionState::Uninstalled, |s| s.get_extension_state(ext))),
                            Rc::new(move |ext: &Extension| weak_runtime.upgrade().and_then(|s| s.get_runtime_state(ext))),
                            None,
                            None,
                            None,
                            Some(ResourceExtensionInfo { resource_extension, is_workspace_scoped }),
                            (this.p.extension_services)(),
                        ))
                    })
                })
                .collect())
        })
    }

    fn resolve_query_text(&self, text: &str) -> String {
        let mut text = text.replace("@web", &format!("tag:\"{WEB_EXTENSION_TAG}\""));
        // `\bext:([^\s]+)\b`
        let mut result = String::new();
        let mut rest = text.as_str();
        while let Some(position) = rest.find("ext:") {
            let before_is_word = rest[..position].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_');
            let after = &rest[position + 4..];
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            let mut ext = &after[..end];
            // O `\b` final: o texto casado termina numa letra, dígito ou `_`.
            while !ext.is_empty() && !ext.chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                ext = &ext[..ext.len() - ext.chars().next_back().map_or(0, char::len_utf8)];
            }
            if before_is_word || ext.is_empty() {
                result.push_str(&rest[..position + 4]);
                rest = &rest[position + 4..];
                continue;
            }
            result.push_str(&rest[..position]);
            let keywords = self.p.product_service.extension_keywords().and_then(|lookup| lookup.get(ext).cloned()).unwrap_or_default();
            let keyword_tags = keywords.iter().map(|tag| format!("tag:\"{tag}\"")).collect::<Vec<_>>().join(" ");
            result.push_str(&format!("tag:\"__ext_{ext}\" tag:\"__ext_.{ext}\" {keyword_tags} tag:\"{ext}\""));
            rest = &after[ext.len()..];
        }
        result.push_str(rest);
        text = result;
        text.chars().take(350).collect()
    }

    fn from_gallery(&self, gallery: &GalleryExtension, extensions_control_manifest: &ExtensionsControlManifest) -> Rc<Extension> {
        if let Some(extension) = self.get_installed_extension_matching_gallery(gallery) {
            return extension;
        }
        let weak_state = self.weak();
        let weak_runtime = self.weak();
        let extension = Rc::new(Extension::new(
            Rc::new(move |ext: &Extension| weak_state.upgrade().map_or(ExtensionState::Uninstalled, |s| s.get_extension_state(ext))),
            Rc::new(move |ext: &Extension| weak_runtime.upgrade().and_then(|s| s.get_runtime_state(ext))),
            None,
            None,
            Some(Rc::new(gallery.clone())),
            None,
            (self.p.extension_services)(),
        ));
        extension.set_extensions_control_manifest(extensions_control_manifest);
        extension
    }

    fn get_installed_extension_matching_gallery(&self, gallery: &GalleryExtension) -> Option<Rc<Extension>> {
        for installed in self.local() {
            let identifier = installed.identifier();
            if identifier.uuid.is_some() {
                // Installed from Gallery
                if identifier.uuid == gallery.identifier.uuid {
                    return Some(installed);
                }
            } else if installed.local().is_none_or(|local| local.source != InstallSource::Resource) && are_same_extensions(&identifier, &gallery.identifier) {
                // Installed from other sources
                return Some(installed);
            }
        }
        None
    }

    fn get_installed_extension_matching_location(&self, location: &URI) -> Option<Rc<Extension>> {
        self.local().into_iter().find(|e| {
            e.local().is_some_and(|local| {
                let extension_location = local_extension_uri(&local);
                self.p.uri_identity_service.ext_uri().is_equal_or_parent(location, &extension_location, false, false)
            })
        })
    }

    /// `updateRunningExtensions(message?, auto?)`.
    pub fn update_running_extensions(&self, message: Option<String>, auto: bool) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            let message = message.unwrap_or_else(|| localize("restart", "Changing extension enablement", &[]));
            let mut to_add: Vec<LocalExtension> = Vec::new();
            let mut to_remove: Vec<String> = Vec::new();

            let extensions_to_check = this.local();
            for extension in &extensions_to_check {
                let Some(runtime_state) = extension.runtime_state() else { continue };
                if runtime_state.action != ExtensionRuntimeActionType::RestartExtensions {
                    continue;
                }
                if extension.state() == ExtensionState::Uninstalled {
                    to_remove.push(extension.identifier().id);
                    continue;
                }
                let Some(local) = extension.local() else { continue };
                let is_enabled = this.p.extension_enablement_service.is_enabled(&local);
                if is_enabled {
                    let running = this.p.extension_service.extensions().into_iter().find(|e| {
                        are_same_extensions(&je_extensions::manifest::ExtensionIdentifierInfo { id: e.identifier.value.clone(), uuid: e.uuid.clone() }, &extension.identifier())
                    });
                    if let Some(running) = running {
                        to_remove.push(running.identifier.value);
                    }
                    to_add.push((*local).clone());
                } else {
                    to_remove.push(extension.identifier().id);
                }
            }

            for extension in this.p.extension_service.extensions() {
                if extension.is_under_development {
                    continue;
                }
                let id = je_extensions::manifest::ExtensionIdentifierInfo { id: extension.identifier.value.clone(), uuid: extension.uuid.clone() };
                if extensions_to_check.iter().any(|e| are_same_extensions(&id, &e.local().map_or_else(|| e.identifier(), |local| local.identifier.clone()))) {
                    continue;
                }
                // Extension is running but doesn't exist locally. Remove it from running extensions.
                to_remove.push(extension.identifier.value);
            }

            if !to_add.is_empty() || !to_remove.is_empty() {
                let count = to_add.len() + to_remove.len();
                if this.p.extension_service.stop_extension_hosts(&message, auto).await? {
                    this.p.extension_service.start_extension_hosts(Some(ExtensionHostUpdates { to_add, to_remove })).await?;
                    if auto {
                        this.p.notification_service.info(vec![NotificationMessage::Text(localize(
                            "extensionsAutoRestart",
                            "Extensions were auto restarted to enable updates.",
                            &[],
                        ))]);
                    }
                    this.p.telemetry_service.public_log2(
                        "extensions:autorestart",
                        Some(Dynamic::object([("count", Dynamic::Number(count as f64)), ("auto", Dynamic::Bool(auto))])),
                    );
                }
            }
            Ok(())
        })
    }

    /// `getRuntimeState(extension)`.
    fn get_runtime_state(&self, extension: &Extension) -> Option<ExtensionRuntimeState> {
        let is_uninstalled = extension.state() == ExtensionState::Uninstalled;
        let running_extension = self.p.extension_service.extensions().into_iter().find(|e| e.identifier.value.eq_ignore_ascii_case(&extension.identifier().id));
        let server_service = &self.p.extension_management_server_service;
        let reload_action = if server_service.remote_extension_management_server().is_some() {
            ExtensionRuntimeActionType::ReloadWindow
        } else {
            ExtensionRuntimeActionType::RestartExtensions
        };
        let reload_action_label = if reload_action == ExtensionRuntimeActionType::ReloadWindow {
            localize("reload", "reload window", &[])
        } else {
            localize("restart extensions", "restart extensions", &[])
        };
        let server_of = |description: &ExtensionDescription| server_service.get_extension_management_server(&to_extension(description));
        let local_server = server_service.local_extension_management_server();
        let remote_server = server_service.remote_extension_management_server();
        let web_server = server_service.web_extension_management_server();
        let _ = web_server;

        if is_uninstalled {
            let can_remove_running_extension = running_extension.as_ref().is_some_and(|running| self.p.extension_service.can_remove_extension(running));
            let is_same_extension_running = running_extension.as_ref().is_some_and(|running| {
                (extension.server().is_none() || same_server(&extension.server(), &server_of(running)))
                    && extension.resource_extension().is_none_or(|resource| {
                        self.p.uri_identity_service.ext_uri().is_equal(Some(&resource.location), Some(&URI::parse(running.extension_location.as_str(), false)), false)
                    })
            });
            if !can_remove_running_extension && is_same_extension_running && !running_extension.as_ref().is_some_and(|running| running.is_under_development) {
                return Some(ExtensionRuntimeState {
                    action: reload_action,
                    reason: localize("postUninstallTooltip", "Please {0} to complete the uninstallation of this extension.", &[reload_action_label.as_str()]),
                });
            }
            return None;
        }
        let local = extension.local()?;
        let is_same_extension_running = running_extension.as_ref().is_some_and(|running| same_server(&extension.server(), &server_of(running)));
        let is_enabled = self.p.extension_enablement_service.is_enabled(&local);
        let reason = |key: &str, text: &str| localize(key, text, &[reload_action_label.as_str()]);

        // Extension is running
        if let Some(running) = &running_extension {
            if is_enabled {
                // No Reload is required if extension can run without reload
                if self.p.extension_service.can_add_extension(&to_extension_description(&local)) {
                    return None;
                }
                let running_extension_server = server_of(running);

                if is_same_extension_running {
                    // Different version or target platform of same extension is running. Requires reload to run the current version
                    if !running.is_under_development && (extension.version() != running.manifest.version || local.target_platform != running.target_platform) {
                        let product_current_version = self.get_product_current_version();
                        let product_update_version = self.get_product_update_version();
                        if let Some(update_version) = product_update_version {
                            let engine = local.manifest.engines.vscode.as_deref().unwrap_or_default();
                            if !is_engine_valid(engine, &product_current_version.version, product_current_version.date.as_deref())
                                && is_engine_valid(engine, &update_version.version, update_version.date.as_deref())
                            {
                                let name = self.p.product_service.name_long().to_owned();
                                return match self.p.update_service.state() {
                                    State::AvailableForDownload(_) => Some(ExtensionRuntimeState {
                                        action: ExtensionRuntimeActionType::DownloadUpdate,
                                        reason: localize("postUpdateDownloadTooltip", "Please update {0} to enable the updated extension.", &[name.as_str()]),
                                    }),
                                    State::Downloaded(_) => Some(ExtensionRuntimeState {
                                        action: ExtensionRuntimeActionType::ApplyUpdate,
                                        reason: localize("postUpdateUpdateTooltip", "Please update {0} to enable the updated extension.", &[name.as_str()]),
                                    }),
                                    State::Ready(_) => Some(ExtensionRuntimeState {
                                        action: ExtensionRuntimeActionType::QuitAndInstall,
                                        reason: localize("postUpdateRestartTooltip", "Please restart {0} to enable the updated extension.", &[name.as_str()]),
                                    }),
                                    _ => None,
                                };
                            }
                        }
                        return Some(ExtensionRuntimeState { action: reload_action, reason: reason("postUpdateTooltip", "Please {0} to enable the updated extension.") });
                    }

                    if self.extensions_servers.len() > 1 {
                        let in_other_server = self.installed().into_iter().find(|e| are_same_extensions(&e.identifier(), &extension.identifier()) && !same_server(&e.server(), &extension.server()));
                        if let Some(in_other_server) = in_other_server {
                            // This extension prefers to run on UI/Local side but is running in remote
                            if same_server(&running_extension_server, &remote_server)
                                && self.p.extension_manifest_properties_service.prefers_execute_on_ui(&local.manifest)
                                && same_server(&in_other_server.server(), &local_server)
                            {
                                return Some(ExtensionRuntimeState { action: reload_action, reason: reason("enable locally", "Please {0} to enable this extension locally.") });
                            }

                            // This extension prefers to run on Workspace/Remote side but is running in local
                            if same_server(&running_extension_server, &local_server)
                                && self.p.extension_manifest_properties_service.prefers_execute_on_workspace(&local.manifest)
                                && same_server(&in_other_server.server(), &remote_server)
                            {
                                let label = remote_server.as_ref().map(|server| server.label.clone()).unwrap_or_default();
                                return Some(ExtensionRuntimeState {
                                    action: reload_action,
                                    reason: localize("enable remote", "Please {0} to enable this extension in {1}.", &[reload_action_label.as_str(), label.as_str()]),
                                });
                            }
                        }
                    }
                } else {
                    if same_server(&extension.server(), &local_server)
                        && same_server(&running_extension_server, &remote_server)
                        && self.p.extension_manifest_properties_service.prefers_execute_on_ui(&local.manifest)
                    {
                        // This extension prefers to run on UI/Local side but is running in remote
                        return Some(ExtensionRuntimeState { action: reload_action, reason: reason("postEnableTooltip", "Please {0} to enable this extension.") });
                    }
                    if same_server(&extension.server(), &remote_server)
                        && same_server(&running_extension_server, &local_server)
                        && self.p.extension_manifest_properties_service.prefers_execute_on_workspace(&local.manifest)
                    {
                        // This extension prefers to run on Workspace/Remote side but is running in local
                        return Some(ExtensionRuntimeState { action: reload_action, reason: reason("postEnableTooltip", "Please {0} to enable this extension.") });
                    }
                }
                return None;
            }
            if is_same_extension_running && !running.is_under_development {
                return Some(ExtensionRuntimeState { action: reload_action, reason: reason("postDisableTooltip", "Please {0} to disable this extension.") });
            }
            return None;
        }

        // Extension is not running
        if is_enabled && !self.p.extension_service.can_add_extension(&to_extension_description(&local)) {
            return Some(ExtensionRuntimeState { action: reload_action, reason: reason("postEnableTooltip", "Please {0} to enable this extension.") });
        }

        let other_server = extension.server().map(|server| if same_server(&Some(server), &local_server) { remote_server.clone() } else { local_server.clone() });
        if let Some(other_server) = other_server {
            if other_server.is_some() && extension.enablement_state() == EnablementState::DisabledByExtensionKind {
                let in_other_server = self.local().into_iter().find(|e| are_same_extensions(&e.identifier(), &extension.identifier()) && same_server(&e.server(), &other_server));
                // Same extension in other server exists and
                if let Some(in_other_server) = in_other_server {
                    if in_other_server.local().is_some_and(|local| self.p.extension_enablement_service.is_enabled(&local)) {
                        return Some(ExtensionRuntimeState { action: reload_action, reason: reason("postEnableTooltip", "Please {0} to enable this extension.") });
                    }
                }
            }
        }
        None
    }

    fn get_primary_extension(&self, extensions: Vec<Rc<Extension>>) -> Rc<Extension> {
        if extensions.len() == 1 {
            return extensions[0].clone();
        }
        let enablement = &self.p.extension_enablement_service;
        let enabled_extensions: Vec<Rc<Extension>> = extensions.iter().filter(|e| e.local().is_some_and(|local| enablement.is_enabled(&local))).cloned().collect();
        if enabled_extensions.len() == 1 {
            return enabled_extensions[0].clone();
        }

        let extensions_to_choose = if enabled_extensions.is_empty() { extensions.clone() } else { enabled_extensions };
        let Some(manifest) = extensions_to_choose.iter().find_map(|e| e.local().map(|local| local.manifest.clone())) else {
            // Manifest is not found which should not happen. In which case return the first extension.
            return extensions_to_choose[0].clone();
        };

        let extension_kinds = self.p.extension_manifest_properties_service.get_extension_kind(&manifest);
        let service = &self.p.extension_management_server_service;
        let (local, remote, web) = (
            service.local_extension_management_server(),
            service.remote_extension_management_server(),
            service.web_extension_management_server(),
        );

        // O `find` de cada passo: a primeira extensão cujo primeiro tipo de extensão (na ordem) decide por um servidor.
        // Um `case` que casa o servidor devolve `true`, um que não casa devolve `false` (e encerra o laço dos tipos).
        let choose = |decide: &dyn Fn(ExtensionKind, &Rc<Extension>) -> Option<bool>| -> Option<Rc<Extension>> {
            extensions_to_choose
                .iter()
                .find(|extension| {
                    for kind in &extension_kinds {
                        if let Some(result) = decide(*kind, extension) {
                            return result;
                        }
                    }
                    false
                })
                .cloned()
        };

        let mut extension = choose(&|kind, extension| match kind {
            // UI extension is chosen only if it is installed locally
            ExtensionKind::Ui => Some(same_server(&extension.server(), &local)),
            // Choose remote workspace extension if exists
            ExtensionKind::Workspace => Some(same_server(&extension.server(), &remote)),
            // Choose web extension if exists
            ExtensionKind::Web => Some(same_server(&extension.server(), &web)),
        });

        if extension.is_none() && local.is_some() {
            extension = choose(&|kind, extension| match kind {
                // Choose local workspace extension if exists
                ExtensionKind::Workspace => Some(same_server(&extension.server(), &local)),
                // Choose local web extension if exists
                ExtensionKind::Web => Some(same_server(&extension.server(), &local)),
                ExtensionKind::Ui => None,
            });
        }

        if extension.is_none() && web.is_some() {
            extension = choose(&|kind, extension| match kind {
                // Choose web extension if exists
                ExtensionKind::Web => Some(same_server(&extension.server(), &web)),
                _ => None,
            });
        }

        if extension.is_none() && remote.is_some() {
            extension = choose(&|kind, extension| match kind {
                // Choose remote web extension if exists
                ExtensionKind::Web => Some(same_server(&extension.server(), &remote)),
                _ => None,
            });
        }

        extension.unwrap_or_else(|| extensions[0].clone())
    }

    /// `getExtensionState(extension)`.
    fn get_extension_state(&self, extension: &Extension) -> ExtensionState {
        if self.installing.borrow().iter().any(|i| are_same_extensions(&i.identifier(), &extension.identifier()) && (extension.server().is_none() || same_server(&i.server(), &extension.server()))) {
            return ExtensionState::Installing;
        }
        for extensions in [&self.remote_extensions, &self.web_extensions].into_iter().flatten() {
            let state = extensions.get_extension_state(extension);
            if state != ExtensionState::Uninstalled {
                return state;
            }
        }
        if let Some(local) = &self.local_extensions {
            return local.get_extension_state(extension);
        }
        ExtensionState::Uninstalled
    }

    /// `checkForUpdates(reason?, onlyBuiltin?)`.
    pub fn check_for_updates(&self, reason: Option<String>, only_builtin: bool) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            match &reason {
                Some(reason) => this.p.log_service.trace(&format!("[Extensions]: Checking for updates. Reason: {reason}")),
                None => this.p.log_service.trace("[Extensions]: Checking for updates"),
            }
            if !this.p.gallery_service.is_enabled() {
                return Ok(());
            }
            let servers: Vec<ServerExtensions> = [&this.local_extensions, &this.remote_extensions, &this.web_extensions].into_iter().flatten().cloned().collect();
            let Some(first) = servers.first() else { return Ok(()) };
            let mut infos: Vec<ExtensionInfo> = Vec::new();
            for installed in this.local() {
                if only_builtin && !installed.is_builtin() {
                    // Skip if check updates only for builtin extensions and current extension is not builtin.
                    continue;
                }
                let local = installed.local();
                let force_auto_update = local.as_ref().is_some_and(|l| l.force_auto_update);
                let pinned = local.as_ref().is_some_and(|l| l.pinned);
                let has_uuid = local.as_ref().is_some_and(|l| l.identifier.uuid.is_some());
                if !force_auto_update && installed.is_builtin() && !pinned && (installed.extension_type() == ExtensionType::System || !has_uuid) {
                    // Skip checking updates for a builtin extension if it is a system extension or if it does not have a Marketplace identifier
                    continue;
                }
                if local.as_ref().is_some_and(|l| l.source == InstallSource::Resource) {
                    continue;
                }
                infos.push(ExtensionInfo {
                    id: installed.identifier().id,
                    uuid: installed.identifier().uuid,
                    pre_release: Some(local.as_ref().is_some_and(|l| l.pre_release)),
                    current_version: if installed.is_builtin() { Some(installed.version()) } else { None },
                    ..ExtensionInfo::default()
                });
            }
            if !infos.is_empty() {
                let target_platform = match first.server.extension_management_service.clone().as_install_service() {
                    Some(install) => install.get_target_platform().await?,
                    None => TargetPlatform::Undefined,
                };
                this.p.telemetry_service.public_log2("galleryService:checkingForUpdates", Some(Dynamic::object([("count", Dynamic::Number(infos.len() as f64))])));
                let ids = infos.iter().map(|e| e.id.clone()).collect::<Vec<_>>().join(", ");
                this.p.log_service.trace(&format!("Checking updates for extensions {ids}"));
                let options = ExtensionQueryOptions { target_platform: Some(target_platform), compatible: true, product_version: Some(this.get_product_version()), ..ExtensionQueryOptions::default() };
                let gallery_extensions = this.p.gallery_service.get_extensions(infos.clone(), options, CancellationToken::None).await?;
                if !gallery_extensions.is_empty() {
                    this.sync_installed_extensions_with_gallery(gallery_extensions, Some(infos)).await?;
                }
            }
            Ok(())
        })
    }

    /// `updateAll()`.
    pub fn update_all(&self) -> Promise<Vec<InstallExtensionResult>> {
        let mut to_update: Vec<InstallExtensionInfo> = Vec::new();
        for extension in self.outdated() {
            if let Some(gallery) = extension.gallery() {
                let local = extension.local();
                to_update.push(InstallExtensionInfo {
                    extension: (*gallery).clone(),
                    options: InstallOptions {
                        operation: Some(InstallOperation::Update),
                        install_pre_release_version: local.as_ref().is_some_and(|l| l.is_pre_release_version),
                        profile_location: Some(self.p.user_data_profile_service.current_profile().extensions_resource.path().to_string_lossy()),
                        is_application_scoped: local.as_ref().map(|l| l.is_application_scoped),
                        ..InstallOptions::default()
                    },
                });
            }
        }
        self.p.extension_management_service.install_gallery_extensions(to_update)
    }

    fn sync_installed_extensions_with_gallery(&self, gallery: Vec<GalleryExtension>, flag_extensions_missing_from_gallery: Option<Vec<ExtensionInfo>>) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            let servers: Vec<ServerExtensions> = [&this.local_extensions, &this.remote_extensions, &this.web_extensions].into_iter().flatten().cloned().collect();
            if servers.is_empty() {
                return Ok(());
            }
            let pending = servers
                .iter()
                .map(|extensions| extensions.sync_installed_extensions_with_gallery(gallery.clone(), this.get_product_version(), flag_extensions_missing_from_gallery.clone()))
                .collect();
            Promise::all_settled(pending).await?;
            let outdated = this.outdated();
            if !outdated.is_empty() {
                let ids = outdated.iter().map(|e| e.identifier().id).collect::<Vec<_>>().join(", ");
                this.p.log_service.info(&format!("Auto updating outdated extensions. {ids}"));
                this.eventually_auto_update_extensions();
            }
            Ok(())
        })
    }

    fn is_auto_check_updates_enabled(&self) -> bool {
        if self.p.metered_connection_service.is_connection_metered() {
            return false;
        }
        matches!(self.p.configuration_service.get_value(Some(AUTO_CHECK_UPDATES_CONFIGURATION_KEY), None), Dynamic::Bool(true))
    }

    fn eventually_check_for_updates(&self, immediate: bool) {
        self.updates_check_delayer.cancel();
        let weak = self.weak();
        let delay = if immediate { 0 } else { self.get_updates_check_interval() };
        self.updates_check_delayer
            .trigger(
                move |_| {
                    let Some(this) = weak.upgrade() else { return Promise::resolved(()) };
                    async_fn(async move {
                        if this.is_auto_check_updates_enabled() {
                            this.check_for_updates(None, false).await?;
                        }
                        this.eventually_check_for_updates(false);
                        Ok(())
                    })
                },
                Some(delay),
            )
            .catch(|_| ());
    }

    fn get_updates_check_interval(&self) -> u64 {
        if self.p.product_service.quality() == Some("insider") && self.get_product_update_version().is_some() {
            return 1000 * 60 * 60; // 1 hour
        }
        UPDATES_CHECK_INTERVAL_MS
    }

    fn eventually_auto_update_extensions(&self) {
        let weak = self.weak();
        self.auto_update_delayer
            .trigger(
                move |_| match weak.upgrade() {
                    Some(this) => this.auto_update_extensions(),
                    None => Promise::resolved(()),
                },
                None,
            )
            .catch(|_| ());
    }

    fn auto_update_builtin_extensions(&self) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            if this.p.metered_connection_service.is_connection_metered() {
                return Ok(());
            }
            this.check_for_updates(None, true).await?;
            let to_update: Vec<Rc<Extension>> = this.outdated().into_iter().filter(|e| e.is_builtin()).collect();
            let installs = to_update
                .into_iter()
                .map(|e| {
                    let options = if e.local().is_some_and(|l| l.pre_release) {
                        InstallExtensionOptions { base: InstallOptions { install_pre_release_version: true, ..InstallOptions::default() }, ..InstallExtensionOptions::default() }
                    } else {
                        InstallExtensionOptions::default()
                    };
                    this.install(InstallArg::Extension(e), options, None).then(|_| ())
                })
                .collect();
            Promise::all_settled(installs).await?;
            Ok(())
        })
    }

    fn sync_pinned_builtin_extensions(&self) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            let mut infos: Vec<ExtensionInfo> = Vec::new();
            for installed in this.local() {
                if installed.is_builtin() && installed.local().is_some_and(|l| l.pinned && l.identifier.uuid.is_some()) {
                    infos.push(ExtensionInfo { id: installed.identifier().id, uuid: installed.identifier().uuid, version: Some(installed.version()), ..ExtensionInfo::default() });
                }
            }
            if !infos.is_empty() {
                let gallery_extensions = this.p.gallery_service.get_extensions(infos, ExtensionQueryOptions::default(), CancellationToken::None).await?;
                if !gallery_extensions.is_empty() {
                    this.sync_installed_extensions_with_gallery(gallery_extensions, None).await?;
                }
            }
            Ok(())
        })
    }

    fn auto_update_extensions(&self) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            if this.p.metered_connection_service.is_connection_metered() {
                this.p.log_service.trace("[Extensions]: Skipping auto-update because connection is metered");
                return Ok(());
            }

            let mut to_update: Vec<Rc<Extension>> = Vec::new();
            let mut disabled_auto_update: Vec<String> = Vec::new();
            let mut consent_required: Vec<String> = Vec::new();
            for extension in this.outdated() {
                if !this.should_auto_update_extension(&extension) {
                    disabled_auto_update.push(extension.identifier().id);
                    continue;
                }
                // New versions are auto updated only after the delay window has passed since they were published.
                if !extension.local().is_some_and(|l| l.force_auto_update) && this.get_auto_update_delay_remaining(&extension) > 0.0 {
                    this.p.log_service.trace(&format!("Auto update delayed for extension {}", extension.identifier().id));
                    continue;
                }
                if this.should_require_consent_to_update(&extension).await?.is_some() {
                    consent_required.push(extension.identifier().id);
                    continue;
                }
                to_update.push(extension);
            }

            if !disabled_auto_update.is_empty() {
                this.p.log_service.trace(&format!("Auto update disabled for extensions {}", disabled_auto_update.join(", ")));
            }
            if !consent_required.is_empty() {
                this.p.log_service.info(&format!("Auto update consent required for extensions {}", consent_required.join(", ")));
            }
            if to_update.is_empty() {
                return Ok(());
            }

            let product_version = this.get_product_version();
            let installs = to_update
                .into_iter()
                .map(|e| {
                    let pre_release = e.local().is_some_and(|l| l.pre_release);
                    let options = InstallExtensionOptions {
                        base: InstallOptions { install_pre_release_version: pre_release, product_version: Some(product_version.clone()), ..InstallOptions::default() },
                        ..InstallExtensionOptions::default()
                    };
                    this.install(InstallArg::Extension(e), options, None).then(|_| ())
                })
                .collect();
            Promise::all_settled(installs).await?;
            Ok(())
        })
    }

    fn get_product_version(&self) -> ProductVersion {
        self.get_product_update_version().unwrap_or_else(|| self.get_product_current_version())
    }

    fn get_product_current_version(&self) -> ProductVersion {
        ProductVersion { version: self.p.product_service.version().to_owned(), date: self.p.product_service.date().map(str::to_owned) }
    }

    fn get_product_update_version(&self) -> Option<ProductVersion> {
        let update = match self.p.update_service.state() {
            State::AvailableForDownload(state) => state.update,
            State::Downloaded(state) => state.update,
            State::Updating(state) => state.update,
            State::Ready(state) => state.update,
            _ => return None,
        };
        let version = update.product_version?;
        if semver::valid(&version).is_some() {
            return Some(ProductVersion { version, date: update.timestamp.map(iso_string) });
        }
        None
    }

    fn should_auto_update_extension(&self, extension: &Rc<Extension>) -> bool {
        if extension.deprecation_info().and_then(|info| info.disallow_install) == Some(true) {
            return false;
        }

        if extension.local().is_some_and(|l| l.force_auto_update) {
            // Extensions marked for auto-update are always auto-updated
            return true;
        }

        let auto_update_value = self.get_auto_update_value();

        if auto_update_value == AutoUpdateConfigurationValue::Off {
            let extensions_to_auto_update = self.get_enabled_auto_update_extensions();
            let extension_id = extension.identifier().id.to_lowercase();
            if extensions_to_auto_update.contains(&extension_id) {
                return true;
            }
            if self.is_auto_update_enabled_for_publisher(&extension.publisher()) && !extensions_to_auto_update.contains(&format!("-{extension_id}")) {
                return true;
            }
            return false;
        }

        if extension.pinned() {
            return false;
        }

        if self.get_disabled_auto_update_extensions().contains(&extension.identifier().id.to_lowercase()) {
            return false;
        }

        // Auto-update is on; only update enabled extensions.
        !matches!(extension.enablement_state(), EnablementState::DisabledGlobally | EnablementState::DisabledWorkspace)
    }

    /// `shouldRequireConsentToUpdate(extension)`: o motivo, quando o consentimento é necessário.
    pub fn should_require_consent_to_update(&self, extension: &Rc<Extension>) -> Promise<Option<String>> {
        if !extension.outdated() {
            return Promise::resolved(None);
        }
        let (Some(gallery), Some(local)) = (extension.gallery(), extension.local()) else { return Promise::resolved(None) };

        if let Some(uuid) = &local.identifier.uuid {
            if Some(uuid) != gallery.identifier.uuid.as_ref() {
                return Promise::resolved(Some(localize(
                    "consentRequiredToUpdateRepublishedExtension",
                    "The marketplace metadata of this extension changed, likely due to a re-publish.",
                    &[],
                )));
            }
        }

        if local.manifest.engines.vscode.as_deref().is_none_or(str::is_empty) || local.manifest.main.is_some() || local.manifest.browser.is_some() {
            return Promise::resolved(None);
        }

        let display_name = extension.display_name();
        let message = localize("consentRequiredToUpdate", "The update for {0} extension introduces executable code, which is not present in the currently installed version.", &[display_name.as_str()]);
        match gallery.properties.executes_code {
            Some(false) => Promise::resolved(None),
            Some(true) => Promise::resolved(Some(message)),
            None => {
                let manifest = extension.get_gallery_manifest(CancellationToken::None);
                manifest.then(move |manifest| match manifest {
                    Some(manifest) if manifest.main.is_some() || manifest.browser.is_some() => Some(message),
                    _ => None,
                })
            }
        }
    }

    /// `isAutoUpdateEnabledFor(extensionOrPublisher)`.
    pub fn is_auto_update_enabled_for(&self, extension_or_publisher: &ExtensionOrPublisher) -> Result<bool, Error> {
        match extension_or_publisher {
            ExtensionOrPublisher::Publisher(publisher) => {
                if is_extension_identifier(publisher) {
                    return Err(Error::new("Expected publisher string, found extension identifier"));
                }
                if self.is_auto_update_enabled() {
                    return Ok(true);
                }
                Ok(self.is_auto_update_enabled_for_publisher(publisher))
            }
            ExtensionOrPublisher::Extension(extension) => Ok(self.should_auto_update_extension(extension)),
        }
    }

    fn is_auto_update_enabled_for_publisher(&self, publisher: &str) -> bool {
        self.get_publishers_to_auto_update().contains(&publisher.to_lowercase())
    }

    /// `updateAutoUpdateEnablementFor(extensionOrPublisher, enable)`.
    pub fn update_auto_update_enablement_for(&self, extension_or_publisher: ExtensionOrPublisher, enable: bool) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            if this.is_auto_update_enabled() {
                let ExtensionOrPublisher::Extension(extension) = &extension_or_publisher else {
                    return Err(Error::new("Expected extension, found publisher string"));
                };
                let mut disabled_auto_update_extensions = this.get_disabled_auto_update_extensions();
                let extension_id = extension.identifier().id.to_lowercase();
                let extension_index = disabled_auto_update_extensions.iter().position(|id| *id == extension_id);
                if enable {
                    if let Some(index) = extension_index {
                        disabled_auto_update_extensions.remove(index);
                    }
                } else if extension_index.is_none() {
                    disabled_auto_update_extensions.push(extension_id);
                }
                this.set_disabled_auto_update_extensions(&disabled_auto_update_extensions);
                if enable && extension.pinned() {
                    if let Some(local) = extension.local() {
                        this.p.extension_management_service.update_metadata(&local, Metadata { pinned: Some(false), ..Metadata::default() }, None).await?;
                    }
                }
                this.on_change.fire(Some(extension.clone()));
            } else {
                let mut enabled_auto_update_extensions = this.get_enabled_auto_update_extensions();
                match &extension_or_publisher {
                    ExtensionOrPublisher::Publisher(publisher) => {
                        if is_extension_identifier(publisher) {
                            return Err(Error::new("Expected publisher string, found extension identifier"));
                        }
                        let publisher = publisher.to_lowercase();
                        let currently = this.is_auto_update_enabled_for(&ExtensionOrPublisher::Publisher(publisher.clone()))?;
                        if currently != enable {
                            if enable {
                                enabled_auto_update_extensions.push(publisher.clone());
                            } else if let Some(index) = enabled_auto_update_extensions.iter().position(|id| *id == publisher) {
                                enabled_auto_update_extensions.remove(index);
                            }
                        }
                        this.set_enabled_auto_update_extensions(&enabled_auto_update_extensions);
                        for e in this.installed() {
                            if e.publisher().to_lowercase() == publisher {
                                this.on_change.fire(Some(e));
                            }
                        }
                    }
                    ExtensionOrPublisher::Extension(extension) => {
                        let extension_id = extension.identifier().id.to_lowercase();
                        let negative_id = format!("-{extension_id}");
                        let enable_auto_updates_for_publisher = this.is_auto_update_enabled_for(&ExtensionOrPublisher::Publisher(extension.publisher().to_lowercase()))?;
                        let enable_auto_updates_for_extension = enabled_auto_update_extensions.contains(&extension_id);
                        let disable_auto_updates_for_extension = enabled_auto_update_extensions.contains(&negative_id);
                        let remove = |list: &mut Vec<String>, id: &str| {
                            if let Some(index) = list.iter().position(|e| e == id) {
                                list.remove(index);
                            }
                        };

                        if enable {
                            if disable_auto_updates_for_extension {
                                remove(&mut enabled_auto_update_extensions, &negative_id);
                            }
                            if enable_auto_updates_for_publisher {
                                if enable_auto_updates_for_extension {
                                    remove(&mut enabled_auto_update_extensions, &extension_id);
                                }
                            } else if !enable_auto_updates_for_extension {
                                enabled_auto_update_extensions.push(extension_id);
                            }
                        }
                        // Disable Auto Updates
                        else {
                            if enable_auto_updates_for_extension {
                                remove(&mut enabled_auto_update_extensions, &extension_id);
                            }
                            if enable_auto_updates_for_publisher {
                                if !disable_auto_updates_for_extension {
                                    enabled_auto_update_extensions.push(negative_id);
                                }
                            } else if disable_auto_updates_for_extension {
                                remove(&mut enabled_auto_update_extensions, &negative_id);
                            }
                        }
                        this.set_enabled_auto_update_extensions(&enabled_auto_update_extensions);
                        this.on_change.fire(Some(extension.clone()));
                    }
                }
            }

            if enable {
                this.auto_update_extensions();
            }
            Ok(())
        })
    }

    /// `canInstall(extension)`: `Ok(())` é o `true` do original, `Err(mensagem)` o `IMarkdownString`.
    pub fn can_install(&self, extension: &Rc<Extension>) -> Promise<Result<(), String>> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(Ok(())) };
        let extension = extension.clone();
        async_fn(async move {
            if extension.is_malicious() == Some(true) {
                return Ok(Err(localize("malicious", "This extension is reported to be problematic.", &[])));
            }
            if extension.deprecation_info().and_then(|info| info.disallow_install) == Some(true) {
                return Ok(Err(localize("disallowed", "This extension is disallowed to be installed.", &[])));
            }
            let display_name = extension.display_name();
            let cannot_be_installed = localize("cannot be installed", "Cannot install the '{0}' extension because it is not available in this setup.", &[display_name.as_str()]);
            if let Some(gallery) = extension.gallery() {
                let mut results: Vec<Result<(), String>> = Vec::new();
                for extensions in [&this.local_extensions, &this.remote_extensions, &this.web_extensions].into_iter().flatten() {
                    let result = extensions.can_install(&gallery).await?;
                    if result.is_ok() {
                        return Ok(Ok(()));
                    }
                    results.push(result);
                }
                return Ok(results.into_iter().next().unwrap_or(Err(cannot_be_installed)));
            }
            if let Some(resource_extension) = extension.resource_extension() {
                if this.p.extension_management_service.can_install_resource(&resource_extension).await?.is_ok() {
                    return Ok(Ok(()));
                }
            }
            Ok(Err(cannot_be_installed))
        })
    }

    /// `install(arg, installOptions, progressLocation?)`.
    pub fn install(&self, arg: InstallArg, install_options: InstallExtensionOptions, progress_location: Option<ProgressLocationId>) -> Promise<Rc<Extension>> {
        let Some(this) = self.this.upgrade() else { return Promise::rejected(Error::new("ExtensionsWorkbenchService is disposed")) };
        async_fn(async move {
            let mut install_options = install_options;
            let extension = this.install_inner(arg, &mut install_options, progress_location).await?;
            let Some(extension) = extension else {
                return Err(Error::new(localize("unknown", "Unable to install extension", &[])));
            };

            if install_options.enable {
                if matches!(extension.enablement_state(), EnablementState::DisabledWorkspace | EnablementState::DisabledGlobally) {
                    if let Some(justification) = &install_options.justification {
                        let display_name = extension.display_name();
                        let primary_button = match justification {
                            InstallJustification::Reason(_) => localize("enableButtonLabel", "&&Enable Extension", &[]),
                            InstallJustification::ReasonWithAction { action, .. } => localize("enableButtonLabelWithAction", "&&Enable Extension and {0}", &[action.as_str()]),
                        };
                        let mut base = BaseDialogOptions::new(localize("enableExtensionMessage", "Would you like to enable '{0}' extension?", &[display_name.as_str()]));
                        base.title = Some(localize("enableExtensionTitle", "Enable Extension", &[]));
                        base.detail = Some(DialogDetail::Text(justification.reason().to_owned()));
                        let result = this.p.dialog_service.confirm(Confirmation { base, primary_button: Some(primary_button), cancel_button: None }).await?;
                        if !result.confirmed {
                            return Err(Error::cancellation());
                        }
                    }
                    let target = if extension.enablement_state() == EnablementState::DisabledWorkspace { EnablementState::EnabledWorkspace } else { EnablementState::EnabledGlobally };
                    this.set_enablement(vec![extension.clone()], target).await?;
                }
                this.wait_until_extension_is_enabled(&extension).await?;
            }

            Ok(extension)
        })
    }

    fn install_inner(&self, arg: InstallArg, install_options: &mut InstallExtensionOptions, progress_location: Option<ProgressLocationId>) -> Promise<Option<Rc<Extension>>> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(None) };
        let mut install_options = install_options.clone();
        async_fn(async move {
            enum Installable {
                Location(URI),
                Gallery(GalleryExtension),
                Resource(IResourceExtension),
            }
            let mut installable: Option<Installable> = None;
            let mut extension: Option<Rc<Extension>> = None;
            let mut servers: Option<Vec<Rc<IExtensionManagementServer>>> = None;

            match &arg {
                InstallArg::Location(location) => installable = Some(Installable::Location(location.clone())),
                _ => {
                    let mut installable_info: Option<ExtensionInfo> = None;
                    let mut gallery: Option<GalleryExtension> = None;

                    match &arg {
                        // Install by id
                        InstallArg::Id(id) => {
                            extension = this.local().into_iter().find(|e| are_same_extensions(&e.identifier(), &je_extensions::manifest::ExtensionIdentifierInfo::new(id.clone())));
                            if extension.as_ref().is_some_and(|e| e.is_builtin()) {
                                if this.p.product_service.built_in_extensions_enabled_with_auto_updates().iter().any(|e| e.eq_ignore_ascii_case(id)) {
                                    return Ok(extension);
                                }
                            } else {
                                installable_info = Some(ExtensionInfo {
                                    id: id.clone(),
                                    version: install_options.version.clone(),
                                    pre_release: Some(if install_options.base.install_pre_release_version { true } else { this.p.extension_management_service.prefer_pre_releases() }),
                                    ..ExtensionInfo::default()
                                });
                            }
                        }
                        InstallArg::Extension(arg_extension) => {
                            // Install by gallery
                            if let Some(arg_gallery) = arg_extension.gallery() {
                                extension = Some(arg_extension.clone());
                                gallery = Some((*arg_gallery).clone());
                                if let Some(version) = &install_options.version {
                                    if *version != arg_gallery.version {
                                        installable_info = Some(ExtensionInfo { id: arg_extension.identifier().id, version: Some(version.clone()), ..ExtensionInfo::default() });
                                    }
                                }
                            }
                            // Install by resource
                            else if let Some(resource) = arg_extension.resource_extension() {
                                extension = Some(arg_extension.clone());
                                installable = Some(Installable::Resource(resource));
                            }
                        }
                        InstallArg::Location(_) => {}
                    }

                    if let Some(info) = installable_info {
                        let target_platform = match extension.as_ref().and_then(|e| e.server()) {
                            Some(server) => match server.extension_management_service.clone().as_install_service() {
                                Some(install) => Some(install.get_target_platform().await?),
                                None => None,
                            },
                            None => None,
                        };
                        let options = ExtensionQueryOptions { target_platform, ..ExtensionQueryOptions::default() };
                        gallery = this.p.gallery_service.get_extensions(vec![info], options, CancellationToken::None).await?.into_iter().next();
                    }

                    if extension.is_none() {
                        if let Some(gallery) = &gallery {
                            let created = this.from_gallery_uncached(gallery);
                            created.set_extensions_control_manifest(&this.p.extension_management_service.get_extensions_control_manifest().await?);
                            extension = Some(created);
                        }
                    }

                    if extension.as_ref().is_some_and(|e| e.is_malicious() == Some(true)) {
                        return Err(Error::new(localize("malicious", "This extension is reported to be problematic.", &[])));
                    }

                    if let Some(gallery) = &gallery {
                        // If requested to install everywhere
                        // then install the extension in all the servers where it is not installed
                        if install_options.install_everywhere {
                            let mut selected = Vec::new();
                            let installable_servers = this.p.extension_management_service.get_installable_servers(gallery).await?;
                            for extensions_server in &this.extensions_servers {
                                if installable_servers.iter().any(|s| Rc::ptr_eq(s, &extensions_server.server))
                                    && !extensions_server.local().iter().any(|e| are_same_extensions(&e.identifier(), &gallery.identifier))
                                {
                                    selected.push(extensions_server.server.clone());
                                }
                            }
                            servers = Some(selected);
                        }
                        // If requested to enable and extension is already installed
                        // Check if the extension is disabled because of extension kind
                        // If so, install the extension in the server that is compatible.
                        else if install_options.enable && extension.as_ref().is_some_and(|e| e.local().is_some()) {
                            let mut selected = Vec::new();
                            if extension.as_ref().is_some_and(|e| e.enablement_state() == EnablementState::DisabledByExtensionKind) {
                                if let Some(installable_server) = this.p.extension_management_service.get_installable_servers(gallery).await?.into_iter().next() {
                                    selected.push(installable_server);
                                }
                            }
                            servers = Some(selected);
                        }
                    }

                    if servers.as_ref().is_none_or(|servers| !servers.is_empty()) {
                        if installable.is_none() {
                            let Some(gallery) = gallery.clone() else {
                                let id = match &arg {
                                    InstallArg::Id(id) => id.clone(),
                                    InstallArg::Extension(e) => e.identifier().id,
                                    InstallArg::Location(_) => String::new(),
                                };
                                return Err(match &install_options.version {
                                    Some(version) => extension_error(
                                        localize("not found version", "The extension '{0}' cannot be installed because the requested version '{1}' was not found.", &[id.as_str(), version.as_str()]),
                                        ExtensionManagementErrorCode::NotFound,
                                    ),
                                    None => extension_error(
                                        localize("not found", "The extension '{0}' cannot be installed because it was not found.", &[id.as_str()]),
                                        ExtensionManagementErrorCode::NotFound,
                                    ),
                                });
                            };
                            installable = Some(Installable::Gallery(gallery));
                        }
                        if install_options.version.is_some() {
                            install_options.base.install_given_version = true;
                        }
                        if extension.as_ref().is_some_and(|e| e.is_workspace_scoped()) {
                            install_options.base.is_workspace_scoped = Some(true);
                        }
                    }
                }
            }

            if let Some(installable) = installable {
                if let Some(justification) = &install_options.justification {
                    let sync_check = install_options.base.is_machine_scoped.is_none()
                        && this.p.user_data_sync_enablement_service.is_enabled()
                        && this.p.user_data_sync_enablement_service.is_resource_enabled(SyncResource::Extensions, None);
                    let label = match justification {
                        InstallJustification::ReasonWithAction { action, .. } if !action.is_empty() => {
                            localize("installButtonLabelWithAction", "&&Install Extension and {0}", &[action.as_str()])
                        }
                        _ => localize("installButtonLabel", "&&Install Extension", &[]),
                    };
                    let mut buttons = vec![PromptButton { label, base: PromptBaseButton { run: Rc::new(|_| je_base::promise::MaybePromise::from(Dynamic::Bool(true))) } }];
                    if extension.is_none() {
                        buttons.push(PromptButton {
                            label: localize("open", "Open Extension", &[]),
                            base: PromptBaseButton { run: Rc::new(|_| je_base::promise::MaybePromise::from(Dynamic::Bool(false))) },
                        });
                    }
                    let message = match &extension {
                        Some(e) => {
                            let (name, publisher) = (e.display_name(), e.publisher_display_name());
                            localize("installExtensionMessage", "Would you like to install '{0}' extension from '{1}'?", &[name.as_str(), publisher.as_str()])
                        }
                        None => localize("installVSIXMessage", "Would you like to install the extension?", &[]),
                    };
                    let mut base = BaseDialogOptions::new(message);
                    base.title = Some(localize("installExtensionTitle", "Install Extension", &[]));
                    base.detail = Some(DialogDetail::Text(justification.reason().to_owned()));
                    if sync_check {
                        base.checkbox = Some(je_platform::dialogs::dialogs::Checkbox { label: localize("sync extension", "Sync this extension", &[]), checked: Some(true) });
                    }
                    let result = this
                        .p
                        .dialog_service
                        .prompt(Prompt {
                            base,
                            buttons: Some(buttons),
                            cancel_button: Some(PromptCancel::Button(PromptCancelButton {
                                label: None,
                                base: PromptBaseButton { run: Rc::new(|_| je_base::promise::MaybePromise::from(Dynamic::Bool(false))) },
                            })),
                        })
                        .await?;
                    if !matches!(result.result, Some(Dynamic::Bool(true))) {
                        return Err(Error::cancellation());
                    }
                    if sync_check {
                        install_options.base.is_machine_scoped = Some(!result.checkbox_checked.unwrap_or(false));
                    }
                }
                match installable {
                    Installable::Location(location) => {
                        let task_this = this.clone();
                        let options = install_options.clone();
                        extension = Some(this.do_install(None, move || task_this.install_from_vsix(location, options.base), progress_location).await?);
                    }
                    Installable::Resource(resource) => {
                        if let Some(current) = extension.clone() {
                            let service = this.p.extension_management_service.clone();
                            let options = install_options.base.clone();
                            extension = Some(this.do_install(Some(current), move || service.install_resource_extension(&resource, options), progress_location).await?);
                        }
                    }
                    Installable::Gallery(gallery) => {
                        if let Some(current) = extension.clone() {
                            let task_this = this.clone();
                            let task_extension = current.clone();
                            let options = install_options.clone();
                            extension = Some(
                                this.do_install(Some(current), move || task_this.install_from_gallery(&task_extension, &gallery, options, servers), progress_location).await?,
                            );
                        }
                    }
                }
            }
            Ok(extension)
        })
    }

    fn from_gallery_uncached(&self, gallery: &GalleryExtension) -> Rc<Extension> {
        let weak_state = self.weak();
        let weak_runtime = self.weak();
        Rc::new(Extension::new(
            Rc::new(move |ext: &Extension| weak_state.upgrade().map_or(ExtensionState::Uninstalled, |s| s.get_extension_state(ext))),
            Rc::new(move |ext: &Extension| weak_runtime.upgrade().and_then(|s| s.get_runtime_state(ext))),
            None,
            None,
            Some(Rc::new(gallery.clone())),
            None,
            (self.p.extension_services)(),
        ))
    }

    fn do_install(&self, extension: Option<Rc<Extension>>, install_task: impl FnOnce() -> Promise<ExtensionRef> + 'static, progress_location: Option<ProgressLocationId>) -> Promise<Rc<Extension>> {
        let Some(this) = self.this.upgrade() else { return Promise::rejected(Error::new("ExtensionsWorkbenchService is disposed")) };
        let title = match &extension {
            Some(e) => {
                let name = e.display_name();
                localize("installing named extension", "Installing '{0}' extension...", &[name.as_str()])
            }
            None => localize("installing extension", "Installing extension...", &[]),
        };
        let location = progress_location.unwrap_or(ProgressLocationId::Location(ProgressLocation::Extensions));
        let task_this = this.clone();
        let task_extension = extension.clone();
        this.with_progress_options_result(location, Some(title), move || {
            let this = task_this;
            let extension = task_extension;
            async_fn(async move {
                let result = async {
                    if let Some(extension) = &extension {
                        this.installing.borrow_mut().push(extension.clone());
                        this.on_change.fire(Some(extension.clone()));
                    }
                    let local = install_task().await?;
                    this.wait_and_get_installed_extension(&local.identifier).await
                }
                .await;
                if let Some(extension) = &extension {
                    this.installing.borrow_mut().retain(|e| !identifiers_equal(e, extension));
                    // Trigger the change without passing the extension because it is replaced by a new instance.
                    this.on_change.fire(None);
                }
                result
            })
        })
    }

    fn install_from_vsix(&self, vsix: URI, install_options: InstallOptions) -> Promise<ExtensionRef> {
        let Some(this) = self.this.upgrade() else { return Promise::rejected(Error::new("ExtensionsWorkbenchService is disposed")) };
        async_fn(async move {
            let mut install_options = install_options;
            let manifest_value = this.p.extension_management_service.get_manifest(&vsix).await?;
            let manifest = ExtensionManifest::from_value(manifest_value);
            let id = get_gallery_extension_id(Some(manifest.publisher.as_str()).filter(|publisher| !publisher.is_empty()), &manifest.name);
            let existing = this.local().into_iter().find(|local| are_same_extensions(&local.identifier(), &je_extensions::manifest::ExtensionIdentifierInfo::new(id.clone())));
            if let Some(existing) = existing {
                if existing.latest_version() == manifest.version {
                    install_options.pinned = Some(install_options.pinned.unwrap_or(existing.local().is_some_and(|l| l.pinned) || !this.should_auto_update_extension(&existing)));
                } else {
                    install_options.install_given_version = true;
                }
            }
            this.p.extension_management_service.install_vsix(&vsix, manifest, install_options).await
        })
    }

    fn install_from_gallery(&self, extension: &Rc<Extension>, gallery: &GalleryExtension, install_options: InstallExtensionOptions, servers: Option<Vec<Rc<IExtensionManagementServer>>>) -> Promise<ExtensionRef> {
        let mut options = install_options.base;
        options.pinned = Some(options.pinned.unwrap_or(extension.local().is_some_and(|l| l.pinned) || !self.should_auto_update_extension(extension)));
        match (extension.local(), &servers) {
            (Some(local), None) => {
                options.product_version = Some(self.get_product_version());
                options.operation = Some(InstallOperation::Update);
                self.p.extension_management_service.update_from_gallery(gallery, &local, options)
            }
            _ => self.p.extension_management_service.install_from_gallery_in_servers(gallery, options, servers),
        }
    }

    fn wait_and_get_installed_extension(&self, identifier: &je_extensions::manifest::ExtensionIdentifierInfo) -> Promise<Rc<Extension>> {
        let Some(this) = self.this.upgrade() else { return Promise::rejected(Error::new("ExtensionsWorkbenchService is disposed")) };
        let identifier = identifier.clone();
        async_fn(async move {
            let find = |this: &Rc<ExtensionsWorkbenchService>| this.local().into_iter().find(|local| are_same_extensions(&local.identifier(), &identifier));
            if find(&this).is_none() {
                let service = this.clone();
                let wanted = identifier.clone();
                let event = this.on_change.event().filter(
                    move |e| e.is_some() && service.local().iter().any(|local| are_same_extensions(&local.identifier(), &wanted)),
                    None,
                );
                event.to_promise(None).await?;
            }
            find(&this).ok_or_else(|| Error::new("Extension should have been installed"))
        })
    }

    fn wait_until_extension_is_enabled(&self, extension: &Rc<Extension>) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        let extension = extension.clone();
        async_fn(async move {
            let id = extension.identifier().id;
            let running = |this: &Rc<ExtensionsWorkbenchService>| this.p.extension_service.extensions().iter().any(|e| e.identifier.value.eq_ignore_ascii_case(&id));
            if running(&this) {
                return Ok(());
            }
            let Some(local) = extension.local() else { return Ok(()) };
            if !this.p.extension_service.can_add_extension(&to_extension_description(&local)) {
                return Ok(());
            }
            let service = this.clone();
            let id_for_event = extension.identifier().id;
            let event = this.p.extension_service.on_did_change_extensions().filter(
                move |_| service.p.extension_service.extensions().iter().any(|e| e.identifier.value.eq_ignore_ascii_case(&id_for_event)),
                None,
            );
            event.to_promise(None).await?;
            Ok(())
        })
    }

    /// `installInServer(extension, server, installOptions?)`.
    pub fn install_in_server(&self, extension: Rc<Extension>, server: Rc<IExtensionManagementServer>, install_options: Option<InstallOptions>) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            let task_this = this.clone();
            let task_extension = extension.clone();
            this.do_install(Some(extension), move || {
                async_fn(async move {
                    let mut extension = task_extension;
                    let this = task_this;
                    let Some(local) = extension.local() else { return Err(Error::new("Extension not found")) };
                    if extension.gallery().is_none() {
                        let info = ExtensionInfo { id: extension.identifier().id, uuid: extension.identifier().uuid, pre_release: Some(local.pre_release), ..ExtensionInfo::default() };
                        if let Some(found) = this.get_extensions(vec![info], ExtensionQueryOptions::default(), CancellationToken::None).await?.into_iter().next() {
                            extension = found;
                        }
                    }
                    let Some(install) = server.extension_management_service.clone().as_install_service() else {
                        return Err(Error::new("The extension management server cannot install extensions"));
                    };
                    if let Some(gallery) = extension.gallery() {
                        let mut options = install_options.unwrap_or_default();
                        options.install_pre_release_version = options.install_pre_release_version || local.pre_release;
                        return install.install_from_gallery(&gallery, options).await;
                    }

                    let target_platform = install.get_target_platform().await?;
                    if !is_target_platform_compatible(local.target_platform, &[local.target_platform], target_platform) {
                        let id = extension.identifier().id;
                        return Err(Error::new(localize("incompatible", "Can't install '{0}' extension because it is not compatible.", &[id.as_str()])));
                    }

                    let vsix = this.p.extension_management_service.zip(&local).await?;
                    install.install(&vsix, InstallOptions::default()).await
                })
            }, None)
            .await?;
            Ok(())
        })
    }

    /// `setEnablement(extensions, enablementState)`.
    pub fn set_enablement(&self, extensions: Vec<Rc<Extension>>, enablement_state: EnablementState) -> Promise<()> {
        self.prompt_and_set_enablement(extensions, enablement_state)
    }

    /// `uninstall(e)`.
    pub fn uninstall(&self, e: &Rc<Extension>) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        let e = e.clone();
        async_fn(async move {
            let extension = if e.local().is_some() { Some(e.clone()) } else { this.local().into_iter().find(|local| are_same_extensions(&local.identifier(), &e.identifier())) };
            let Some((extension, local)) = extension.and_then(|extension| extension.local().map(|local| (extension, local))) else {
                return Err(Error::new("Missing local"));
            };

            let profiles = this.p.user_data_profiles_service.profiles();
            if local.is_application_scoped && profiles.len() > 1 {
                let name = extension.display_name();
                let mut base = BaseDialogOptions::new(localize("uninstallApplicationScopedMessage", "Would you like to Uninstall {0} from all profiles?", &[name.as_str()]));
                base.title = Some(localize("uninstallApplicationScoped", "Uninstall Extension", &[]));
                base.dialog_type = Some(DialogTypeOrSeverity::Severity(Severity::Info));
                let result = this
                    .p
                    .dialog_service
                    .confirm(Confirmation { base, primary_button: Some(localize("uninstallAllProfiles", "Uninstall (All Profiles)", &[])), cancel_button: None })
                    .await?;
                if !result.confirmed {
                    return Err(Error::cancellation());
                }
            }

            let mut extensions_to_uninstall: Vec<UninstallExtensionInfo> = vec![UninstallExtensionInfo { extension: (*local).clone(), options: None }];
            let chat_agent_id = this.p.product_service.default_chat_agent().map(|agent| agent.extension_id);
            let is_chat_agent = chat_agent_id.is_some_and(|id| are_same_extensions(&extension.identifier(), &je_extensions::manifest::ExtensionIdentifierInfo::new(id)));
            if !is_chat_agent {
                for pack_extension in this.get_all_packed_extensions(&extension, &this.local(), &mut Vec::new()) {
                    if let Some(pack_local) = pack_extension.local() {
                        if !extensions_to_uninstall.iter().any(|e| are_same_extensions(&e.extension.identifier, &pack_extension.identifier())) {
                            extensions_to_uninstall.push(UninstallExtensionInfo { extension: (*pack_local).clone(), options: None });
                        }
                    }
                }
            }

            let mut dependents: Vec<Rc<LocalExtension>> = Vec::new();
            let mut extensions_from_all_profiles: Option<Vec<(ExtensionRef, URI)>> = None;
            let to_check: Vec<LocalExtension> = extensions_to_uninstall.iter().map(|info| info.extension.clone()).collect();
            for current in to_check {
                let mut installed_extensions: Vec<(ExtensionRef, Option<URI>)> = Vec::new();
                if current.is_application_scoped && profiles.len() > 1 {
                    if extensions_from_all_profiles.is_none() {
                        let mut collected = Vec::new();
                        let pending: Vec<Promise<Vec<(ExtensionRef, URI)>>> = profiles
                            .iter()
                            .map(|profile| {
                                let resource = profile.extensions_resource.clone();
                                this.p.extension_management_service.get_installed_filtered(Some(ExtensionType::User), Some(&resource), None, None).then(move |installed| {
                                    installed.into_iter().map(|local| (local, resource.clone())).collect()
                                })
                            })
                            .collect();
                        for settled in Promise::all_settled(pending).await? {
                            if let Ok(list) = settled {
                                collected.extend(list);
                            }
                        }
                        extensions_from_all_profiles = Some(collected);
                    }
                    installed_extensions.extend(extensions_from_all_profiles.clone().unwrap_or_default().into_iter().map(|(local, uri)| (local, Some(uri))));
                } else {
                    for local_extension in this.local() {
                        if let Some(local) = local_extension.local() {
                            installed_extensions.push((local, None));
                        }
                    }
                }
                for (local, profile_location) in installed_extensions {
                    if are_same_extensions(&local.identifier, &current.identifier) {
                        continue;
                    }
                    let Some(dependencies) = local.manifest.extension_dependencies.as_ref().filter(|d| !d.is_empty()) else { continue };
                    let in_pack = |pack: &Option<Vec<String>>| pack.as_ref().is_some_and(|pack| pack.iter().any(|id| are_same_extensions(&je_extensions::manifest::ExtensionIdentifierInfo::new(id.clone()), &local.identifier)));
                    if in_pack(&current.manifest.extension_pack) {
                        continue;
                    }
                    if dependents.iter().any(|d| in_pack(&d.manifest.extension_pack)) {
                        continue;
                    }
                    if dependencies.iter().any(|dep| are_same_extensions(&current.identifier, &je_extensions::manifest::ExtensionIdentifierInfo::new(dep.clone()))) {
                        dependents.push(local.clone());
                        extensions_to_uninstall.push(UninstallExtensionInfo {
                            extension: (*local).clone(),
                            options: Some(UninstallOptions { profile_location: profile_location.map(|uri| uri.path().to_string_lossy()), ..UninstallOptions::default() }),
                        });
                    }
                }
            }

            if !dependents.is_empty() {
                let mut base = BaseDialogOptions::new(this.get_error_message_for_uninstalling_an_extension_with_dependents(&extension, &dependents));
                base.title = Some(localize("uninstallDependents", "Uninstall Extension with Dependents", &[]));
                base.dialog_type = Some(DialogTypeOrSeverity::Severity(Severity::Warning));
                let result = this
                    .p
                    .dialog_service
                    .prompt(Prompt {
                        base,
                        buttons: Some(vec![PromptButton {
                            label: localize("uninstallAll", "Uninstall All", &[]),
                            base: PromptBaseButton { run: Rc::new(|_| je_base::promise::MaybePromise::from(Dynamic::Bool(true))) },
                        }]),
                        cancel_button: Some(PromptCancel::Button(PromptCancelButton {
                            label: None,
                            base: PromptBaseButton { run: Rc::new(|_| je_base::promise::MaybePromise::from(Dynamic::Bool(false))) },
                        })),
                    })
                    .await?;
                if !matches!(result.result, Some(Dynamic::Bool(true))) {
                    return Err(Error::cancellation());
                }
            }

            let service = this.p.extension_management_service.clone();
            let id = extension.identifier().id;
            let mut options = ProgressOptions::new(ProgressLocation::Extensions);
            options.title = Some(localize("uninstallingExtension", "Uninstalling extension...", &[]).into());
            options.source = Some(je_platform::notification::NotificationSourceOrText::Text(id));
            this.with_progress(options, move || service.uninstall_extensions(extensions_to_uninstall)).await
        })
    }

    fn get_all_packed_extensions(&self, extension: &Rc<Extension>, installed: &[Rc<Extension>], checked: &mut Vec<Rc<Extension>>) -> Vec<Rc<Extension>> {
        if checked.iter().any(|e| are_same_extensions(&e.identifier(), &extension.identifier())) {
            return Vec::new();
        }
        checked.push(extension.clone());
        let extensions_pack = extension.extension_pack();
        if extensions_pack.is_empty() {
            return Vec::new();
        }
        let packed_extensions: Vec<Rc<Extension>> = installed
            .iter()
            .filter(|i| !i.is_builtin() && extensions_pack.iter().any(|id| are_same_extensions(&je_extensions::manifest::ExtensionIdentifierInfo::new(id.clone()), &i.identifier())))
            .cloned()
            .collect();
        let mut pack_of_packed_extensions: Vec<Rc<Extension>> = Vec::new();
        for packed_extension in &packed_extensions {
            pack_of_packed_extensions.extend(self.get_all_packed_extensions(packed_extension, installed, checked));
        }
        let mut result = packed_extensions;
        result.extend(pack_of_packed_extensions);
        result
    }

    fn get_error_message_for_uninstalling_an_extension_with_dependents(&self, extension: &Rc<Extension>, dependents: &[Rc<LocalExtension>]) -> String {
        let name = extension.display_name();
        let dependent_name = |index: usize| dependents[index].manifest.display_name.clone().unwrap_or_default();
        if dependents.len() == 1 {
            return localize(
                "singleDependentUninstallError",
                "Cannot uninstall '{0}' extension alone. '{1}' extension depends on this. Do you want to uninstall all these extensions?",
                &[name.as_str(), dependent_name(0).as_str()],
            );
        }
        if dependents.len() == 2 {
            return localize(
                "twoDependentsUninstallError",
                "Cannot uninstall '{0}' extension alone. '{1}' and '{2}' extensions depend on this. Do you want to uninstall all these extensions?",
                &[name.as_str(), dependent_name(0).as_str(), dependent_name(1).as_str()],
            );
        }
        localize(
            "multipleDependentsUninstallError",
            "Cannot uninstall '{0}' extension alone. '{1}', '{2}' and other extensions depend on this. Do you want to uninstall all these extensions?",
            &[name.as_str(), dependent_name(0).as_str(), dependent_name(1).as_str()],
        )
    }

    /// `isExtensionIgnoredToSync(extension)`.
    pub fn is_extension_ignored_to_sync(&self, extension: &Rc<Extension>) -> bool {
        match extension.local() {
            Some(local) => !self.is_installed_extension_synced(&local),
            None => self.p.extensions_sync_management_service.has_to_never_sync_extension(&extension.identifier().id),
        }
    }

    /// `togglePreRelease(extension)`.
    pub fn toggle_pre_release(&self, extension: &Rc<Extension>) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        let extension = extension.clone();
        async_fn(async move {
            let Some(local) = extension.local() else { return Ok(()) };
            if extension.pre_release() != extension.is_pre_release_version() {
                this.p.extension_management_service.update_metadata(&local, Metadata { pre_release: Some(!extension.pre_release()), ..Metadata::default() }, None).await?;
                return Ok(());
            }
            let options = InstallExtensionOptions {
                base: InstallOptions { install_pre_release_version: !extension.pre_release(), pre_release: Some(!extension.pre_release()), ..InstallOptions::default() },
                ..InstallExtensionOptions::default()
            };
            this.install(InstallArg::Extension(extension), options, None).await?;
            Ok(())
        })
    }

    /// `toggleExtensionIgnoredToSync(extension)`.
    pub fn toggle_extension_ignored_to_sync(&self, extension: &Rc<Extension>) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        let extension = extension.clone();
        async_fn(async move {
            let mut including_packed = vec![extension.clone()];
            including_packed.extend(this.get_all_packed_extensions(&extension, &this.local(), &mut Vec::new()));
            // Updated in sync to prevent race conditions
            for e in including_packed {
                let is_ignored = this.is_extension_ignored_to_sync(&e);
                match e.local() {
                    Some(local) if is_ignored && local.is_machine_scoped => {
                        this.p.extension_management_service.update_metadata(&local, Metadata { is_machine_scoped: Some(false), ..Metadata::default() }, None).await?;
                    }
                    _ => this.p.extensions_sync_management_service.update_ignored_extensions(&e.identifier().id, !is_ignored).await?,
                }
            }
            this.p.user_data_auto_sync_service.trigger_sync(&["IgnoredExtensionsUpdated".to_owned()], None).await?;
            Ok(())
        })
    }

    /// `toggleApplyExtensionToAllProfiles(extension)`.
    pub fn toggle_apply_extension_to_all_profiles(&self, extension: &Rc<Extension>) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        let extension = extension.clone();
        async_fn(async move {
            let mut including_packed = vec![extension.clone()];
            including_packed.extend(this.get_all_packed_extensions(&extension, &this.local(), &mut Vec::new()));
            let all_servers: Vec<ServerExtensions> = [&this.local_extensions, &this.remote_extensions, &this.web_extensions].into_iter().flatten().cloned().collect();
            let pending: Vec<Promise<()>> = including_packed
                .into_iter()
                .map(|e| {
                    let this = this.clone();
                    let all_servers = all_servers.clone();
                    async_fn(async move {
                        let Some(local) = e.local() else { return Ok(()) };
                        if is_application_scoped_extension(&local.manifest) || e.is_builtin() {
                            return Ok(());
                        }
                        let is_application_scoped = local.is_application_scoped;
                        let inner: Vec<Promise<()>> = all_servers
                            .iter()
                            .map(|extension_server| {
                                let this = this.clone();
                                let found = extension_server.local().into_iter().find(|l| are_same_extensions(&e.identifier(), &l.identifier())).and_then(|l| l.local());
                                async_fn(async move {
                                    if let Some(local) = found {
                                        if local.is_application_scoped == is_application_scoped {
                                            let resource = this.p.user_data_profile_service.current_profile().extensions_resource;
                                            this.p.extension_management_service.toggle_application_scope(&local, &resource).await?;
                                        }
                                    }
                                    Ok(())
                                })
                            })
                            .collect();
                        Promise::all(inner).await?;
                        Ok(())
                    })
                })
                .collect();
            Promise::all_settled(pending).await?;
            Ok(())
        })
    }

    fn is_installed_extension_synced(&self, extension: &LocalExtension) -> bool {
        if extension.is_machine_scoped {
            return false;
        }
        if self.p.extensions_sync_management_service.has_to_always_sync_extension(&extension.identifier.id) {
            return true;
        }
        !self.p.extensions_sync_management_service.has_to_never_sync_extension(&extension.identifier.id)
    }

    fn prompt_and_set_enablement(&self, extensions: Vec<Rc<Extension>>, enablement_state: EnablementState) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            let enable = bool_enablement(enablement_state);
            if enable {
                let all = this.get_extensions_recursively(&extensions, &this.local(), enablement_state, true, true, &mut Vec::new());
                this.check_and_set_enablement(extensions, all, enablement_state).await
            } else {
                let packed = this.get_extensions_recursively(&extensions, &this.local(), enablement_state, false, true, &mut Vec::new());
                this.check_and_set_enablement(extensions, packed, enablement_state).await
            }
        })
    }

    fn check_and_set_enablement(&self, extensions: Vec<Rc<Extension>>, other_extensions: Vec<Rc<Extension>>, enablement_state: EnablementState) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            let mut all_extensions = extensions.clone();
            all_extensions.extend(other_extensions);
            let enable = bool_enablement(enablement_state);
            if !enable {
                for extension in &extensions {
                    let dependents = this.get_dependents_after_disablement(extension, &all_extensions, &this.local());
                    if !dependents.is_empty() {
                        let mut base = BaseDialogOptions::new(this.get_dependents_error_message_for_disablement(extension, &all_extensions, &dependents));
                        base.title = Some(localize("disableDependents", "Disable Extension with Dependents", &[]));
                        base.dialog_type = Some(DialogTypeOrSeverity::Severity(Severity::Warning));
                        let result = this
                            .p
                            .dialog_service
                            .prompt(Prompt {
                                base,
                                buttons: Some(vec![PromptButton {
                                    label: localize("disable all", "Disable All", &[]),
                                    base: PromptBaseButton { run: Rc::new(|_| je_base::promise::MaybePromise::from(Dynamic::Bool(true))) },
                                }]),
                                cancel_button: Some(PromptCancel::Button(PromptCancelButton {
                                    label: None,
                                    base: PromptBaseButton { run: Rc::new(|_| je_base::promise::MaybePromise::from(Dynamic::Bool(false))) },
                                })),
                            })
                            .await?;
                        if !matches!(result.result, Some(Dynamic::Bool(true))) {
                            return Err(Error::cancellation());
                        }
                        this.check_and_set_enablement(dependents, vec![extension.clone()], enablement_state).await?;
                    }
                }
            }
            this.do_set_enablement(&all_extensions, enablement_state).await?;
            Ok(())
        })
    }

    fn get_extensions_recursively(
        &self,
        extensions: &[Rc<Extension>],
        installed: &[Rc<Extension>],
        enablement_state: EnablementState,
        dependencies: bool,
        pack: bool,
        checked: &mut Vec<Rc<Extension>>,
    ) -> Vec<Rc<Extension>> {
        let to_check: Vec<Rc<Extension>> = extensions.iter().filter(|e| !checked.iter().any(|c| identifiers_equal(c, e))).cloned().collect();
        if to_check.is_empty() {
            return Vec::new();
        }
        for extension in to_check {
            checked.push(extension);
        }
        let enable = bool_enablement(enablement_state);
        let mut to_enable_or_disable: Vec<Rc<Extension>> = installed
            .iter()
            .filter(|i| {
                if checked.iter().any(|c| identifiers_equal(c, i)) {
                    return false;
                }
                let is_extension_enabled = bool_enablement(i.enablement_state());
                if enable == is_extension_enabled {
                    return false;
                }
                // Include all Extensions for enablement and only non builtin extensions for disablement
                (enable || !i.is_builtin())
                    && (dependencies || pack)
                    && extensions.iter().any(|extension| {
                        (dependencies && extension.dependencies().iter().any(|id| are_same_extensions(&je_extensions::manifest::ExtensionIdentifierInfo::new(id.clone()), &i.identifier())))
                            || (pack && extension.extension_pack().iter().any(|id| are_same_extensions(&je_extensions::manifest::ExtensionIdentifierInfo::new(id.clone()), &i.identifier())))
                    })
            })
            .cloned()
            .collect();
        if !to_enable_or_disable.is_empty() {
            let more = self.get_extensions_recursively(&to_enable_or_disable.clone(), installed, enablement_state, dependencies, pack, checked);
            to_enable_or_disable.extend(more);
        }
        to_enable_or_disable
    }

    fn get_dependents_after_disablement(&self, extension: &Rc<Extension>, extensions_to_disable: &[Rc<Extension>], installed: &[Rc<Extension>]) -> Vec<Rc<Extension>> {
        installed
            .iter()
            .filter(|i| {
                if i.dependencies().is_empty() {
                    return false;
                }
                if identifiers_equal(i, extension) {
                    return false;
                }
                if !self.p.extension_enablement_service.is_enabled_enablement_state(i.enablement_state()) {
                    return false;
                }
                if extensions_to_disable.iter().any(|e| identifiers_equal(e, i)) {
                    return false;
                }
                i.dependencies().iter().any(|dep| {
                    std::iter::once(extension).chain(extensions_to_disable.iter()).any(|d| are_same_extensions(&d.identifier(), &je_extensions::manifest::ExtensionIdentifierInfo::new(dep.clone())))
                })
            })
            .cloned()
            .collect()
    }

    fn get_dependents_error_message_for_disablement(&self, extension: &Rc<Extension>, all_disabled_extensions: &[Rc<Extension>], dependents: &[Rc<Extension>]) -> String {
        for e in std::iter::once(extension).chain(all_disabled_extensions.iter()) {
            let dependents_of_the_extension: Vec<Rc<Extension>> = dependents
                .iter()
                .filter(|d| d.dependencies().iter().any(|id| are_same_extensions(&je_extensions::manifest::ExtensionIdentifierInfo::new(id.clone()), &e.identifier())))
                .cloned()
                .collect();
            if !dependents_of_the_extension.is_empty() {
                return self.get_error_message_for_disabling_an_extension_with_dependents(e, &dependents_of_the_extension);
            }
        }
        String::new()
    }

    fn get_error_message_for_disabling_an_extension_with_dependents(&self, extension: &Rc<Extension>, dependents: &[Rc<Extension>]) -> String {
        let name = extension.display_name();
        if dependents.len() == 1 {
            let first = dependents[0].display_name();
            return localize(
                "singleDependentError",
                "Cannot disable '{0}' extension alone. '{1}' extension depends on this. Do you want to disable all these extensions?",
                &[name.as_str(), first.as_str()],
            );
        }
        let (first, second) = (dependents[0].display_name(), dependents[1].display_name());
        if dependents.len() == 2 {
            return localize(
                "twoDependentsError",
                "Cannot disable '{0}' extension alone. '{1}' and '{2}' extensions depend on this. Do you want to disable all these extensions?",
                &[name.as_str(), first.as_str(), second.as_str()],
            );
        }
        localize(
            "multipleDependentsError",
            "Cannot disable '{0}' extension alone. '{1}', '{2}' and other extensions depend on this. Do you want to disable all these extensions?",
            &[name.as_str(), first.as_str(), second.as_str()],
        )
    }

    fn do_set_enablement(&self, extensions: &[Rc<Extension>], enablement_state: EnablementState) -> Promise<Vec<bool>> {
        let locals: Vec<ExtensionRef> = extensions.iter().filter_map(|e| e.local()).collect();
        self.p.extension_enablement_service.set_enablement(locals, enablement_state)
    }

    /// `withProgress(options, task)`: o `createCancelablePromise` da tarefa fica em `tasksInProgress`.
    fn with_progress(&self, options: ProgressOptions, task: impl FnOnce() -> Promise<()> + 'static) -> Promise<()> {
        self.with_progress_result(options, move || task())
    }

    fn with_progress_options(&self, location: ProgressLocationId, title: Option<String>, task: impl FnOnce() -> Promise<()> + 'static) -> Promise<()> {
        let mut options = ProgressOptions::new(location);
        options.title = title.map(Into::into);
        self.with_progress(options, task)
    }

    fn with_progress_options_result<T: Clone + 'static>(&self, location: ProgressLocationId, title: Option<String>, task: impl FnOnce() -> Promise<T> + 'static) -> Promise<T> {
        let mut options = ProgressOptions::new(location);
        options.title = title.map(Into::into);
        self.with_progress_result(options, task)
    }

    fn with_progress_result<T: Clone + 'static>(&self, options: ProgressOptions, task: impl FnOnce() -> Promise<T> + 'static) -> Promise<T> {
        let Some(this) = self.this.upgrade() else { return Promise::rejected(Error::new("ExtensionsWorkbenchService is disposed")) };
        let (result, resolver) = Promise::<T>::new();
        let outer = self.p.progress_service.with_progress(
            options,
            Box::new(move |_progress| {
                let task_promise = task();
                let cancelable = CancelablePromise::new(task_promise.then(|_| ()), || {});
                // `tasksInProgress`
                let tracked = cancelable.clone();
                this.tasks_in_progress.borrow_mut().push(tracked);
                let this_for_cleanup = this.clone();
                let finished = task_promise.clone();
                finished.then_result(move |outcome| {
                    this_for_cleanup.tasks_in_progress.borrow_mut().retain(|t| !t.ptr_eq(&cancelable));
                    match outcome {
                        Ok(value) => resolver.resolve(value),
                        Err(error) => resolver.reject(error),
                    }
                    Ok(())
                });
                task_promise.then(|_| ()).catch(|_| ())
            }),
            None,
        );
        let _ = outer;
        result
    }

    /// `getPublishersToAutoUpdate()`.
    fn get_publishers_to_auto_update(&self) -> Vec<String> {
        self.get_enabled_auto_update_extensions().into_iter().filter(|id| !is_extension_identifier(id)).collect()
    }

    /// `getEnabledAutoUpdateExtensions()`.
    pub fn get_enabled_auto_update_extensions(&self) -> Vec<String> {
        read_string_list(Some(self.enabled_auto_update_extensions_value()))
    }

    fn set_enabled_auto_update_extensions(&self, values: &[String]) {
        let value = serde_json::to_string(values).unwrap_or_else(|_| "[]".to_owned());
        if self.enabled_auto_update_extensions_value() != value {
            *self.enabled_auto_update_extensions_value.borrow_mut() = Some(value.clone());
            self.p.storage_service.store(EXTENSIONS_AUTO_UPDATE_KEY, StorageValue::from(value.as_str()), StorageScope::Application, StorageTarget::User);
        }
    }

    fn enabled_auto_update_extensions_value(&self) -> String {
        if let Some(value) = self.enabled_auto_update_extensions_value.borrow().clone() {
            return value;
        }
        let value = self.p.storage_service.get(EXTENSIONS_AUTO_UPDATE_KEY, StorageScope::Application, Some("[]")).unwrap_or_else(|| "[]".to_owned());
        *self.enabled_auto_update_extensions_value.borrow_mut() = Some(value.clone());
        value
    }

    /// `getDisabledAutoUpdateExtensions()`.
    pub fn get_disabled_auto_update_extensions(&self) -> Vec<String> {
        read_string_list(Some(self.disabled_auto_update_extensions_value()))
    }

    fn set_disabled_auto_update_extensions(&self, values: &[String]) {
        let value = serde_json::to_string(values).unwrap_or_else(|_| "[]".to_owned());
        if self.disabled_auto_update_extensions_value() != value {
            *self.disabled_auto_update_extensions_value.borrow_mut() = Some(value.clone());
            self.p.storage_service.store(EXTENSIONS_DONOT_AUTO_UPDATE_KEY, StorageValue::from(value.as_str()), StorageScope::Application, StorageTarget::User);
        }
    }

    fn disabled_auto_update_extensions_value(&self) -> String {
        if let Some(value) = self.disabled_auto_update_extensions_value.borrow().clone() {
            return value;
        }
        let value = self.p.storage_service.get(EXTENSIONS_DONOT_AUTO_UPDATE_KEY, StorageScope::Application, Some("[]")).unwrap_or_else(|| "[]".to_owned());
        *self.disabled_auto_update_extensions_value.borrow_mut() = Some(value.clone());
        value
    }

    /// `onError(err)`.
    pub fn on_error(&self, err: &Error) {
        if is_cancellation_error(err) {
            return;
        }
        let message = err.message.as_str();
        if ["getaddrinfo ENOTFOUND", "getaddrinfo ENOENT", "connect EACCES", "connect ECONNREFUSED"].iter().any(|pattern| message.contains(pattern)) {
            return;
        }
        self.p.notification_service.error(vec![NotificationMessage::Error(err.clone())]);
    }
}

impl IDisposable for ExtensionsWorkbenchService {
    fn dispose(&self) {
        ExtensionsWorkbenchService::dispose(self);
    }
}

/// O `IPager<IExtension>` de `queryGallery`: a primeira página convertida e as outras sincronizadas sob demanda.
struct WorkbenchPager {
    service: Rc<ExtensionsWorkbenchService>,
    pager: Rc<dyn IPager<GalleryExtension>>,
    control_manifest: ExtensionsControlManifest,
    first_page: Vec<Rc<Extension>>,
}

impl IPager<Rc<Extension>> for WorkbenchPager {
    fn first_page(&self) -> Vec<Rc<Extension>> {
        self.first_page.clone()
    }

    fn total(&self) -> usize {
        self.pager.total()
    }

    fn page_size(&self) -> usize {
        self.pager.page_size()
    }

    fn get_page(&self, page_index: usize, token: CancellationToken) -> Promise<Vec<Rc<Extension>>> {
        let service = self.service.clone();
        let manifest = self.control_manifest.clone();
        self.pager.get_page(page_index, token).then(move |page| {
            service.sync_installed_extensions_with_gallery(page.clone(), None);
            page.iter().map(|gallery| service.from_gallery(gallery, &manifest)).collect()
        })
    }
}
