//! Porte de `src/vs/workbench/services/extensionManagement/browser/extensionEnablementService.ts`: o
//! `ExtensionEnablementService` (o estado de habilitação de cada extensão: global, por workspace, por ambiente,
//! por confiança no workspace, por workspace virtual, por tipo de extensão, por dependência, por lista de permissão,
//! por extensão maliciosa e por unificação do chat) e o `ExtensionsManager` (a lista das extensões instaladas que ele
//! consulta).
//!
//! Divergências:
//! - O `extends Disposable` vira o campo `base` ([`Disposable`]); `this._store` é o campo `store`.
//! - As dependências por injeção (`@IStorageService`...) entram numa struct, [`ExtensionEnablementServiceParts`]
//!   (o `IInstantiationService` do original só criava o `ExtensionsManager`, e aqui o construtor o cria).
//!   `IExtensionManagementService` e `IWorkbenchExtensionManagementService` são o mesmo serviço (o do workbench, de
//!   que o da plataforma é supertrait). O `IAllowedExtensionsService` é o [`AllowedExtensionsService`] concreto
//!   (o único que existe); quem o monta o liga à configuração (`on_configuration_changed`).
//! - Os métodos do contrato [`IWorkbenchExtensionEnablementService`] ficam só no `impl` do trait (sem duplicar em
//!   métodos próprios). O que o original expõe como `protected` (`extensionsManager`, `globalExtensionEnablementService`,
//!   `_getWorkspaceEnabledExtensions`, `_getWorkspaceDisabledExtensions`) é público, para o dublê de teste que estende
//!   a classe.
//! - As funções `async` do original são [`async_fn`]; o `await` de uma promessa cede como no JavaScript. O que o
//!   original lança dentro delas é o `Err` que rejeita a promessa. Os métodos que lançam e que quem chama captura
//!   (`throwErrorIf...`) devolvem `Result<(), Error>`.
//! - O mapa `computedEnablementStates` (`Map<IExtension, EnablementState>`) é chaveado pela identidade do objeto (o
//!   endereço da [`IExtension`]), como o `Map` do JavaScript.
//! - `isAllowed(extension)` recebe a extensão como os campos que a lista `extensions.allowed` lê (`id`, `version`,
//!   `prerelease`, `publisherDisplayName`), e o publisher é o prefixo do identificador (no original, o do manifesto: são
//!   o mesmo em toda extensão válida).
//! - `isWeb` é [`Platform::detect`]. `registerSingleton(..., InstantiationType.Delayed)` não tem equivalente: quem monta o
//!   workbench cria o serviço com [`ExtensionEnablementService::new`].
//! - O `IChatEntitlementService` é o recorte de [`super::service_contracts`] e o `IHostService` é o de [`crate::services::host::host`], e o
//!   `IExtensionBisectService` o de [`super::extension_bisect`].
//! - `Event.filter` e as inscrições seguram o serviço por referência fraca: o serviço é descartado quando o último
//!   `Rc` some, e as promessas pendentes do laço da lista de maliciosas não o mantêm vivo.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::{Delayer, async_fn};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::product::BoolOrLimited;
use je_base::promise::{MaybePromise, Promise};
use je_base::symbols::Delay;
use je_extensions::allowed::{AllowedCandidate, AllowedExtensionsService};
use je_extensions::management::{
    DISABLED_EXTENSIONS_STORAGE_PATH, ENABLED_EXTENSIONS_STORAGE_PATH, ExtensionOrPublisher, InstallOperation, MaliciousExtensionInfo,
};
use je_extensions::manifest::{
    ExtensionIdentifierInfo, ExtensionKind, ExtensionManifest, ExtensionType, is_authentication_provider_extension, is_language_pack_extension,
    is_resolver_extension,
};
use je_extensions::util::{BETTER_MERGE_ID, are_same_extensions, get_extension_dependencies, is_malicious};
use je_platform::configuration::service::IConfigurationService;
use je_platform::environment::environment::{DisableExtensions, EnvironmentService};
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};
use je_platform::notification::notification_telemetry::{NotificationActionTelemetryId, NotificationTelemetry, NotificationTelemetryId};
use je_platform::notification::{
    BasePromptChoice, INotificationService, NotificationMessage, NotificationPriority, NotificationProperties, PromptChoice, PromptChoiceKind, PromptOptions, Severity,
};
use je_platform::product::IProductService;
use je_platform::remote::remote_hosts::get_remote_name;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::user_data_sync::user_data_sync::IUserDataSyncEnablementService;
use je_platform::user_data_sync::user_data_sync_account::IUserDataSyncAccountService;
use je_platform::virtual_workspace::is_virtual_workspace;
use je_platform::workspace::{IWorkspaceContextService, WorkbenchState};
use je_platform::workspace_trust::{IWorkspaceTrustManagementService, IWorkspaceTrustRequestService};
use je_storage::StorageValue;
use serde_json::Value;

use super::extension_bisect::IExtensionBisectService;
use super::extension_management::{
    EnablementState, ExtensionInstallLocation, ExtensionRef, IExtension, IExtensionManagementServer, IExtensionManagementServerService,
    IGlobalExtensionEnablementService, IWorkbenchExtensionEnablementService, IWorkbenchExtensionManagementService, WorkspaceTypeOverrides,
    local_extension_uri,
};
use super::global_extension_enablement_service::StorageManager;
use super::service_contracts::IChatEntitlementService;
use crate::services::host::host::{IHostService, ReloadOptions};
use crate::services::accounts::default_account::IDefaultAccountService;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;
use crate::services::extensions::extension_manifest_properties_service::IExtensionManifestPropertiesService;
use crate::services::lifecycle::lifecycle::{ILifecycleService, LifecyclePhase};

const SOURCE: &str = "IWorkbenchExtensionEnablementService";

const EXTENSION_UNIFICATION_SETTING: &str = "chat.extensionUnification.enabled";
const MALICIOUS_EXTENSIONS_STORAGE_KEY: &str = "extensionsEnablement/malicious";

/// `ChatAIDisabledSettingId` (`platform/chat/common/chatSettings.ts`).
pub const CHAT_AI_DISABLED_SETTING_ID: &str = "chat.disableAIFeatures";

/// `webWorkerExtHostConfig` (`workbench/services/extensions/common/extensions.ts`).
pub const WEB_WORKER_EXT_HOST_CONFIG: &str = "extensions.webWorker";

/// A chave (no escopo do perfil) que marca a migração da extensão de chat embutida como feita.
const BUILTIN_CHAT_EXTENSION_ENABLEMENT_MIGRATION_KEY: &str = "builtinChatExtensionEnablementMigration";

/// De quanto em quanto tempo a lista de extensões maliciosas é conferida: cinco minutos.
const MALICIOUS_EXTENSIONS_CHECK_INTERVAL_MS: u64 = 1000 * 60 * 5;

/// `WorkspaceType`: `{ virtual, trusted }`.
#[derive(Clone, Copy, Debug)]
struct WorkspaceType {
    is_virtual: bool,
    trusted: bool,
}

/// O `{ dependencies: boolean; pack: boolean }` de `getExtensionsToEnableRecursively`.
#[derive(Clone, Copy)]
struct EnableRecursivelyOptions {
    dependencies: bool,
    pack: bool,
}

/// O mapa `computedEnablementStates`, chaveado pelo endereço do objeto.
type ComputedEnablementStates = HashMap<usize, EnablementState>;

/// A identidade de uma extensão (o `===` de objetos do JavaScript).
fn identity_of(extension: &IExtension) -> usize {
    std::ptr::from_ref(extension) as usize
}

/// `a === b` entre dois servidores de gerenciamento (`null === null` vale).
fn is_same_server(a: &Option<Rc<IExtensionManagementServer>>, b: &Option<Rc<IExtensionManagementServer>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// O nome que as mensagens usam: `extension.manifest.displayName || extension.identifier.id`.
fn display_name_of(extension: &IExtension) -> String {
    extension.manifest.display_name.clone().filter(|name| !name.is_empty()).unwrap_or_else(|| extension.identifier.id.clone())
}

/// A verdade JavaScript de `manifest.main` e `manifest.browser`.
fn has_entry_point(entry_point: &Option<String>) -> bool {
    entry_point.as_deref().is_some_and(|path| !path.is_empty())
}

/// `areSameExtensions({ id }, identifier)`.
fn is_extension_id(id: &str, identifier: &ExtensionIdentifierInfo) -> bool {
    are_same_extensions(&ExtensionIdentifierInfo::new(id), identifier)
}

/// `extensionDependencies?.some(id => areSameExtensions({ id }, identifier))`.
fn lists_extension(ids: &Option<Vec<String>>, identifier: &ExtensionIdentifierInfo) -> bool {
    ids.as_ref().is_some_and(|ids| ids.iter().any(|id| is_extension_id(id, identifier)))
}

/// As dependências (instaladas) de uma extensão, como `getExtensionDependencies`.
fn dependencies_of(installed_extensions: &[ExtensionRef], manifest: &ExtensionManifest) -> Vec<ExtensionRef> {
    get_extension_dependencies(installed_extensions, manifest, |e| &e.manifest, |e| &e.identifier)
}

/// `this.environmentService.disableExtensions === true`: todas as extensões do usuário estão desabilitadas.
fn are_all_user_extensions_disabled(environment_service: &dyn IWorkbenchEnvironmentService) -> bool {
    matches!(environment_service.disable_extensions(), DisableExtensions::All(true))
}

/// O que o [`ExtensionsManager`] avisa quando a lista de extensões muda.
#[derive(Clone)]
pub struct ExtensionsChangeEvent {
    pub added: Vec<ExtensionRef>,
    pub removed: Vec<ExtensionRef>,
    pub is_profile_switch: bool,
}

/// As dependências por injeção do [`ExtensionEnablementService`].
pub struct ExtensionEnablementServiceParts {
    pub storage_service: Rc<dyn StorageService>,
    pub global_extension_enablement_service: Rc<dyn IGlobalExtensionEnablementService>,
    pub context_service: Rc<dyn IWorkspaceContextService>,
    pub environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    pub extension_management_service: Rc<dyn IWorkbenchExtensionManagementService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub extension_management_server_service: Rc<dyn IExtensionManagementServerService>,
    pub user_data_sync_enablement_service: Rc<dyn IUserDataSyncEnablementService>,
    pub default_account_service: Rc<dyn IDefaultAccountService>,
    pub user_data_sync_account_service: Rc<dyn IUserDataSyncAccountService>,
    pub lifecycle_service: Rc<dyn ILifecycleService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub host_service: Rc<dyn IHostService>,
    pub extension_bisect_service: Rc<dyn IExtensionBisectService>,
    pub allowed_extensions_service: Rc<AllowedExtensionsService>,
    pub workspace_trust_management_service: Rc<dyn IWorkspaceTrustManagementService>,
    pub workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService>,
    pub extension_manifest_properties_service: Rc<dyn IExtensionManifestPropertiesService>,
    pub chat_entitlement_service: Rc<dyn IChatEntitlementService>,
    pub log_service: LogServiceRef,
    pub product_service: Rc<dyn IProductService>,
}

/// `ExtensionEnablementService`.
pub struct ExtensionEnablementService {
    base: Rc<Disposable>,
    /// `this._store`.
    store: Rc<DisposableStore>,
    this: Weak<ExtensionEnablementService>,

    on_enablement_changed: Rc<Emitter<Vec<ExtensionRef>>>,

    /// `extensionsManager` (protegido no original).
    pub extensions_manager: Rc<ExtensionsManager>,
    storage_manager: Rc<StorageManager>,
    extensions_disabled_extensions: RefCell<Vec<ExtensionRef>>,
    delayer: Rc<Delayer<()>>,

    // Extension unification
    completions_extension_id: Option<String>,
    chat_extension_id: Option<String>,
    extension_unification_enabled: Cell<bool>,

    /// Sessions window allow-list (lowercased extension ids)
    sessions_window_allowed_extensions: HashSet<String>,

    malicious_extensions_cache: RefCell<Option<Vec<MaliciousExtensionInfo>>>,

    storage_service: Rc<dyn StorageService>,
    /// `globalExtensionEnablementService` (protegido no original).
    pub global_extension_enablement_service: Rc<dyn IGlobalExtensionEnablementService>,
    context_service: Rc<dyn IWorkspaceContextService>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    extension_management_service: Rc<dyn IWorkbenchExtensionManagementService>,
    configuration_service: Rc<dyn IConfigurationService>,
    extension_management_server_service: Rc<dyn IExtensionManagementServerService>,
    user_data_sync_enablement_service: Rc<dyn IUserDataSyncEnablementService>,
    default_account_service: Rc<dyn IDefaultAccountService>,
    user_data_sync_account_service: Rc<dyn IUserDataSyncAccountService>,
    extension_bisect_service: Rc<dyn IExtensionBisectService>,
    allowed_extensions_service: Rc<AllowedExtensionsService>,
    workspace_trust_management_service: Rc<dyn IWorkspaceTrustManagementService>,
    workspace_trust_request_service: Rc<dyn IWorkspaceTrustRequestService>,
    extension_manifest_properties_service: Rc<dyn IExtensionManifestPropertiesService>,
    chat_entitlement_service: Rc<dyn IChatEntitlementService>,
    log_service: LogServiceRef,
}

impl ExtensionEnablementService {
    pub fn new(parts: ExtensionEnablementServiceParts) -> Rc<Self> {
        let ExtensionEnablementServiceParts {
            storage_service,
            global_extension_enablement_service,
            context_service,
            environment_service,
            extension_management_service,
            configuration_service,
            extension_management_server_service,
            user_data_sync_enablement_service,
            default_account_service,
            user_data_sync_account_service,
            lifecycle_service,
            notification_service,
            host_service,
            extension_bisect_service,
            allowed_extensions_service,
            workspace_trust_management_service,
            workspace_trust_request_service,
            extension_manifest_properties_service,
            chat_entitlement_service,
            log_service,
            product_service,
        } = parts;

        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        let on_enablement_changed = Emitter::new();
        base.register(on_enablement_changed.clone());
        let storage_manager = StorageManager::new(storage_service.clone());
        base.register(storage_manager.clone());
        let delayer = Rc::new(Delayer::<()>::new(Delay::Millis(0)));
        base.register(delayer.clone());
        let extensions_manager = ExtensionsManager::new(extension_management_service.clone(), extension_management_server_service.clone(), log_service.clone());
        base.register(extensions_manager.clone());

        // Extension unification
        let default_chat_agent = product_service.default_chat_agent();
        let completions_extension_id = default_chat_agent.as_ref().map(|agent| agent.extension_id.to_lowercase());
        let chat_extension_id = default_chat_agent.as_ref().map(|agent| agent.chat_extension_id.to_lowercase());
        let sessions_window_allowed_extensions: HashSet<String> = product_service.sessions_window_allowed_extensions().iter().map(|id| id.to_lowercase()).collect();
        let unification_extensions: Vec<String> = [completions_extension_id.clone(), chat_extension_id.clone()].into_iter().flatten().collect();

        // Disabling extension unification should immediately disable the unified extension flow
        // Enabling extension unification will only take effect after restart
        // Extension Unification is disabled in web when there is no remote authority
        let extension_unification_enabled = if Platform::detect().is_web && environment_service.remote_authority().is_none() {
            false
        } else {
            matches!(configuration_service.get_value(Some(EXTENSION_UNIFICATION_SETTING), None), Dynamic::Bool(true))
        };

        let all_user_extensions_disabled = are_all_user_extensions_disabled(&*environment_service);

        let this = Rc::new_cyclic(|this| Self {
            base,
            store,
            this: this.clone(),
            on_enablement_changed,
            extensions_manager,
            storage_manager,
            extensions_disabled_extensions: RefCell::new(Vec::new()),
            delayer,
            completions_extension_id,
            chat_extension_id,
            extension_unification_enabled: Cell::new(extension_unification_enabled),
            sessions_window_allowed_extensions,
            malicious_extensions_cache: RefCell::new(None),
            storage_service,
            global_extension_enablement_service,
            context_service,
            environment_service,
            extension_management_service,
            configuration_service,
            extension_management_server_service,
            user_data_sync_enablement_service,
            default_account_service,
            user_data_sync_account_service,
            extension_bisect_service,
            allowed_extensions_service,
            workspace_trust_management_service,
            workspace_trust_request_service,
            extension_manifest_properties_service,
            chat_entitlement_service,
            log_service,
        });

        let weak = Rc::downgrade(&this);
        let uninstall_disposable: DisposableRef = this.base.register(
            this.extension_management_service
                .on_did_uninstall_extension()
                .filter(|e| e.error.is_none(), None)
                .listen(move |e| {
                    if let Some(this) = weak.upgrade() {
                        this.reset(&e.identifier);
                    }
                }),
        );
        let is_disposed = Rc::new(Cell::new(false));
        this.base.register(to_disposable({
            let is_disposed = is_disposed.clone();
            move || is_disposed.set(true)
        }));
        let weak = Rc::downgrade(&this);
        this.extensions_manager.when_initialized().then(move |()| {
            let Some(this) = weak.upgrade() else { return };
            if !is_disposed.get() {
                uninstall_disposable.dispose();
                this.on_did_change_extensions(Vec::new(), Vec::new(), false);
                let weak = this.this.clone();
                this.base.register(this.extensions_manager.on_did_change_extensions().listen(move |e| {
                    if let Some(this) = weak.upgrade() {
                        this.on_did_change_extensions(e.added.clone(), e.removed.clone(), e.is_profile_switch);
                    }
                }));
                this.loop_check_for_malicious_extensions();
            }
        });

        let weak = Rc::downgrade(&this);
        this.base.register(this.global_extension_enablement_service.on_did_change_enablement().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_globally_disabled_extensions(&e.extensions, e.source.as_deref());
            }
        }));
        let weak = Rc::downgrade(&this);
        this.base.register(this.allowed_extensions_service.on_did_change_allowed_extensions_config_value().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_extensions(Vec::new(), Vec::new(), false);
            }
        }));

        // Invalidate the cached malicious extensions list when the stored value changes.
        let weak = Rc::downgrade(&this);
        this.base.register(this.storage_service.on_did_change_value(StorageScope::Application, Some(MALICIOUS_EXTENSIONS_STORAGE_KEY), &this.store).listen(move |_| {
            if let Some(this) = weak.upgrade() {
                *this.malicious_extensions_cache.borrow_mut() = None;
            }
        }));

        let weak = Rc::downgrade(&this);
        this.base.register(this.configuration_service.on_did_change_configuration().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            if e.affects_configuration(EXTENSION_UNIFICATION_SETTING, None) {
                let extension_unification_enabled = matches!(this.configuration_service.get_value(Some(EXTENSION_UNIFICATION_SETTING), None), Dynamic::Bool(true));
                if !extension_unification_enabled {
                    this.extension_unification_enabled.set(false);
                    let unified: Vec<ExtensionRef> =
                        this.extensions_manager.extensions().into_iter().filter(|ext| unification_extensions.contains(&ext.identifier.id.to_lowercase())).collect();
                    this.on_enablement_changed.fire(unified);
                }
            }
        }));

        // delay notification for extensions disabled until workbench restored
        if all_user_extensions_disabled {
            lifecycle_service.when(LifecyclePhase::Eventually).then(move |()| {
                let reload_host_service = host_service.clone();
                notification_service.prompt(
                    Severity::Info,
                    NotificationMessage::from(localize("extensionsDisabled", "All installed extensions are temporarily disabled.", &[])),
                    vec![PromptChoiceKind::Plain(PromptChoice {
                        base: BasePromptChoice {
                            telemetry_id: Some(NotificationActionTelemetryId::Reload),
                            label: localize("Reload", "Reload and Enable Extensions", &[]),
                            keep_open: false,
                            run: Rc::new(move || {
                                let _ = reload_host_service.reload(Some(ReloadOptions { disable_extensions: Some(false) }));
                            }),
                        },
                        is_secondary: false,
                    })],
                    Some(PromptOptions {
                        properties: NotificationProperties {
                            telemetry: Some(NotificationTelemetry::Id(NotificationTelemetryId::ExtensionsDisabled)),
                            sticky: true,
                            priority: Some(NotificationPriority::Urgent),
                            ..NotificationProperties::default()
                        },
                        on_cancel: None,
                    }),
                );
            });
        }

        this.ensure_chat_extension_initial_disabled_state();

        this
    }

    fn ensure_chat_extension_initial_disabled_state(&self) {
        let Some(chat_extension_id) = &self.chat_extension_id else { return };
        if self.environment_service.is_sessions_window() || self.environment_service.skip_builtin_extensions().iter().any(|id| id.to_lowercase() == *chat_extension_id) {
            return;
        }

        let builtin_chat_extension_enablement_migration =
            self.storage_service.get_boolean(BUILTIN_CHAT_EXTENSION_ENABLEMENT_MIGRATION_KEY, StorageScope::Profile, None) == Some(true);
        if builtin_chat_extension_enablement_migration {
            return;
        }

        self.log_service.debug("Running builtin chat extension enablement migration");
        self.storage_service.store(BUILTIN_CHAT_EXTENSION_ENABLEMENT_MIGRATION_KEY, StorageValue::Bool(true), StorageScope::Profile, StorageTarget::Machine);
        if let Some(context) = self.chat_entitlement_service.context() {
            let chat_extension = ExtensionIdentifierInfo::new(chat_extension_id.clone());
            if context.state.completed {
                // User has used chat features before
                if self.is_disabled_globally_by_identifier(&chat_extension) {
                    // User had specifically disabled the chat extension to disable AI features
                    if !matches!(self.configuration_service.get_value(Some(CHAT_AI_DISABLED_SETTING_ID), None), Dynamic::Bool(true)) {
                        // Honor that choice by disabling AI features
                        self.log_service.debug("Disabling AI features because builtin chat extension is disabled");
                        let log_service = self.log_service.clone();
                        self.configuration_service.update_value(CHAT_AI_DISABLED_SETTING_ID, Dynamic::Bool(true), None, None).catch(move |error| {
                            log_service.error_args(
                                &LogMessage::from("Failed to update chat.disableAIFeatures setting during builtin chat extension enablement migration"),
                                &[LogArg::from(&error)],
                            );
                        });
                    }
                }
            } else {
                // User has not used chat features before so avoid activating the chat extension by disabling it
                self.log_service.debug("Disabling builtin chat extension as chat set up is not completed");
                self.disable_extension(&chat_extension);
            }
        }
    }

    fn has_workspace(&self) -> bool {
        self.context_service.get_workbench_state() != WorkbenchState::Empty
    }

    /// `isDefaultOrSettingsSyncAuthProviderExtension`.
    fn is_default_or_settings_sync_auth_provider_extension(&self, manifest: &ExtensionManifest) -> bool {
        if !is_authentication_provider_extension(manifest) {
            return false;
        }

        let authentication = manifest.contributes.as_ref().and_then(|contributes| contributes.authentication.as_ref()).map(Vec::as_slice).unwrap_or_default();
        let default_account_auth_provider = self.default_account_service.get_default_account_authentication_provider();
        if authentication.iter().any(|a| a.id.as_deref() == Some(default_account_auth_provider.id.as_str())) {
            return true;
        }

        if self.user_data_sync_enablement_service.is_enabled()
            && let Some(account) = self.user_data_sync_account_service.account()
            && authentication.iter().any(|a| a.id.as_deref() == Some(account.authentication_provider_id.as_str()))
        {
            return true;
        }

        false
    }

    fn throw_error_if_cannot_change_enablement(&self, extension: &IExtension, donot_check_dependencies: bool) -> Result<(), Error> {
        if is_language_pack_extension(&extension.manifest) {
            return Err(Error::new(localize(
                "cannot disable language pack extension",
                "Cannot change enablement of {0} extension because it contributes language packs.",
                &[&display_name_of(extension)],
            )));
        }

        if self.is_default_or_settings_sync_auth_provider_extension(&extension.manifest) {
            return Err(Error::new(localize(
                "cannot disable settings sync auth extension",
                "Cannot change enablement of {0} extension because Settings Sync depends on it.",
                &[&display_name_of(extension)],
            )));
        }

        if self.is_enabled_in_env(extension) {
            return Err(Error::new(localize(
                "cannot change enablement environment",
                "Cannot change enablement of {0} extension because it is enabled in environment",
                &[&display_name_of(extension)],
            )));
        }

        self.throw_error_if_enablement_state_cannot_be_changed(extension, self.get_enablement_state(extension), donot_check_dependencies)
    }

    fn throw_error_if_enablement_state_cannot_be_changed(&self, extension: &IExtension, enablement_state_of_extension: EnablementState, donot_check_dependencies: bool) -> Result<(), Error> {
        let name = display_name_of(extension);
        match enablement_state_of_extension {
            EnablementState::DisabledByEnvironment => Err(Error::new(localize(
                "cannot change disablement environment",
                "Cannot change enablement of {0} extension because it is disabled in environment",
                &[&name],
            ))),
            EnablementState::DisabledByMalicious => {
                Err(Error::new(localize("cannot change enablement malicious", "Cannot change enablement of {0} extension because it is malicious", &[&name])))
            }
            EnablementState::DisabledByVirtualWorkspace => Err(Error::new(localize(
                "cannot change enablement virtual workspace",
                "Cannot change enablement of {0} extension because it does not support virtual workspaces",
                &[&name],
            ))),
            EnablementState::DisabledByExtensionKind => Err(Error::new(localize(
                "cannot change enablement extension kind",
                "Cannot change enablement of {0} extension because of its extension kind",
                &[&name],
            ))),
            EnablementState::DisabledByAllowlist => {
                Err(Error::new(localize("cannot change disallowed extension enablement", "Cannot change enablement of {0} extension because it is disallowed", &[&name])))
            }
            EnablementState::DisabledByInvalidExtension => {
                Err(Error::new(localize("cannot change invalid extension enablement", "Cannot change enablement of {0} extension because of it is invalid", &[&name])))
            }
            EnablementState::DisabledByExtensionDependency => {
                if donot_check_dependencies {
                    return Ok(());
                }
                // Can be changed only when all its dependencies enablements can be changed
                for dependency in dependencies_of(&self.extensions_manager.extensions(), &extension.manifest) {
                    if self.is_enabled(&dependency) {
                        continue;
                    }
                    return Err(Error::new(localize(
                        "cannot change enablement dependency",
                        "Cannot enable '{0}' extension because it depends on '{1}' extension that cannot be enabled",
                        &[&name, &display_name_of(&dependency)],
                    )));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn throw_error_if_cannot_change_workspace_enablement(&self, extension: &IExtension) -> Result<(), Error> {
        if !self.has_workspace() {
            return Err(Error::new(localize("noWorkspace", "No workspace.", &[])));
        }

        if self.is_default_or_settings_sync_auth_provider_extension(&extension.manifest) {
            return Err(Error::new(localize(
                "cannot disable settings sync auth extension in workspace",
                "Cannot change enablement of {0} extension in workspace because Settings Sync depends on it.",
                &[&display_name_of(extension)],
            )));
        }

        Ok(())
    }

    fn get_extensions_to_enable_recursively(
        &self,
        extensions: &[ExtensionRef],
        all_extensions: &[ExtensionRef],
        enablement_state: EnablementState,
        options: EnableRecursivelyOptions,
        checked: &mut Vec<ExtensionRef>,
    ) -> Vec<ExtensionRef> {
        if !options.dependencies && !options.pack {
            return Vec::new();
        }

        let to_check: Vec<ExtensionRef> = extensions.iter().filter(|e| !checked.iter().any(|c| Rc::ptr_eq(c, e))).cloned().collect();
        if to_check.is_empty() {
            return Vec::new();
        }

        checked.extend(to_check);

        let mut extensions_to_enable: Vec<ExtensionRef> = Vec::new();
        for extension in all_extensions {
            // Extension is already checked
            if checked.iter().any(|e| are_same_extensions(&e.identifier, &extension.identifier)) {
                continue;
            }

            let enablement_state_of_extension = self.get_enablement_state(extension);
            // Extension is enabled
            if self.is_enabled_enablement_state(enablement_state_of_extension) {
                continue;
            }

            // Skip if dependency extension is disabled by extension kind
            if enablement_state_of_extension == EnablementState::DisabledByExtensionKind {
                continue;
            }

            // Check if the extension is a dependency or in extension pack
            if extensions.iter().any(|e| {
                (options.dependencies && lists_extension(&e.manifest.extension_dependencies, &extension.identifier))
                    || (options.pack && lists_extension(&e.manifest.extension_pack, &extension.identifier))
            }) {
                let index = extensions_to_enable.iter().position(|e| are_same_extensions(&e.identifier, &extension.identifier));

                match index {
                    // Extension is not added to the disablement list so add it
                    None => extensions_to_enable.push(extension.clone()),

                    // Extension is there already in the disablement list.
                    Some(index) => {
                        // Replace only if the enablement state can be changed
                        if self.throw_error_if_enablement_state_cannot_be_changed(extension, enablement_state_of_extension, true).is_ok() {
                            extensions_to_enable[index] = extension.clone();
                        }
                    }
                }
            }
        }

        if !extensions_to_enable.is_empty() {
            let more = self.get_extensions_to_enable_recursively(&extensions_to_enable, all_extensions, enablement_state, options, checked);
            extensions_to_enable.extend(more);
        }

        extensions_to_enable
    }

    fn set_user_enablement_state(&self, extension: &IExtension, new_state: EnablementState) -> Promise<bool> {
        let current_state = self.get_user_enablement_state(&extension.identifier);

        if current_state == new_state {
            return Promise::resolved(false);
        }

        match new_state {
            EnablementState::EnabledGlobally => {
                self.enable_extension(&extension.identifier);
            }
            EnablementState::DisabledGlobally => {
                self.disable_extension(&extension.identifier);
            }
            EnablementState::EnabledWorkspace => self.enable_extension_in_workspace(&extension.identifier),
            EnablementState::DisabledWorkspace => self.disable_extension_in_workspace(&extension.identifier),
            _ => {}
        }

        Promise::resolved(true)
    }

    fn compute_enablement_state(&self, extension: &IExtension, extensions: &[ExtensionRef], workspace_type: WorkspaceType, computed_enablement_states: &mut ComputedEnablementStates) -> EnablementState {
        if let Some(enablement_state) = computed_enablement_states.get(&identity_of(extension)) {
            return *enablement_state;
        }

        // Ensure the chat extension is disabled in fresh profiles where chat setup is not completed.
        // This is called here (in addition to the constructor) because on profile switch the
        // enablement service is not recreated, but the storage scope changes to the new profile.
        if self.chat_extension_id.as_deref() == Some(extension.identifier.id.to_lowercase().as_str()) {
            self.ensure_chat_extension_initial_disabled_state();
        }

        let mut enablement_state = self.get_user_enablement_state(&extension.identifier);
        let is_enabled = self.is_enabled_enablement_state(enablement_state);

        if is_malicious(&extension.identifier, &self.get_malicious_extensions_for_check()) {
            enablement_state = EnablementState::DisabledByMalicious;
        } else if is_enabled && extension.extension_type == ExtensionType::User && self.is_disallowed(extension) {
            enablement_state = EnablementState::DisabledByAllowlist;
        } else if is_enabled && !extension.is_valid {
            enablement_state = EnablementState::DisabledByInvalidExtension;
        } else if self.extension_bisect_service.is_disabled_by_bisect(extension) {
            enablement_state = EnablementState::DisabledByEnvironment;
        } else if self.is_disabled_in_env(extension) {
            enablement_state = EnablementState::DisabledByEnvironment;
        } else if self.is_disabled_by_virtual_workspace(extension, workspace_type) {
            enablement_state = EnablementState::DisabledByVirtualWorkspace;
        } else if is_enabled && self.is_disabled_by_workspace_trust(extension, workspace_type) {
            enablement_state = EnablementState::DisabledByTrustRequirement;
        } else if self.is_disabled_by_extension_kind(extension) {
            enablement_state = EnablementState::DisabledByExtensionKind;
        } else if self.is_disabled_by_sessions_window(extension) {
            enablement_state = EnablementState::DisabledByEnvironment;
        } else if is_enabled && self.is_disabled_by_extension_dependency(extension, extensions, workspace_type, computed_enablement_states) {
            enablement_state = EnablementState::DisabledByExtensionDependency;
        } else if self.is_disabled_by_unification(&extension.identifier) {
            enablement_state = EnablementState::DisabledByUnification;
        } else if !is_enabled && self.is_enabled_in_env(extension) {
            enablement_state = EnablementState::EnabledByEnvironment;
        }

        computed_enablement_states.insert(identity_of(extension), enablement_state);
        enablement_state
    }

    /// `this.allowedExtensionsService.isAllowed(extension) !== true`.
    fn is_disallowed(&self, extension: &IExtension) -> bool {
        self.allowed_extensions_service
            .is_allowed(&AllowedCandidate::Fields {
                id: &extension.identifier.id,
                publisher_display_name: extension.publisher_display_name.as_deref(),
                version: Some(extension.manifest.version.as_str()),
                prerelease: Some(extension.pre_release),
                target_platform: Some(extension.target_platform),
            })
            .is_err()
    }

    /// `getRemoteName(this.environmentService.remoteAuthority)`.
    fn remote_name(&self) -> Option<String> {
        let authority = self.environment_service.remote_authority()?;
        get_remote_name(Some(&JsString::from(authority.as_str()))).map(|name| name.to_string_lossy())
    }

    fn is_disabled_in_env(&self, extension: &IExtension) -> bool {
        if are_all_user_extensions_disabled(&*self.environment_service) {
            return !extension.is_builtin && !is_resolver_extension(&extension.manifest, self.remote_name().as_deref());
        }

        if let DisableExtensions::Some(disabled_extensions) = self.environment_service.disable_extensions() {
            return disabled_extensions.iter().any(|id| is_extension_id(id, &extension.identifier));
        }

        // Check if this is the better merge extension which was migrated to a built-in extension
        if is_extension_id(BETTER_MERGE_ID, &extension.identifier) {
            return true;
        }

        false
    }

    fn is_enabled_in_env(&self, extension: &IExtension) -> bool {
        self.environment_service.enable_extensions().is_some_and(|enabled_extensions| enabled_extensions.iter().any(|id| is_extension_id(id, &extension.identifier)))
    }

    fn is_disabled_by_virtual_workspace(&self, extension: &IExtension, workspace_type: WorkspaceType) -> bool {
        // Not a virtual workspace
        if !workspace_type.is_virtual {
            return false;
        }

        // Supports virtual workspace
        if !matches!(self.extension_manifest_properties_service.get_extension_virtual_workspace_support_type(&extension.manifest), BoolOrLimited::Bool(false)) {
            return false;
        }

        // Web extension from web extension management server
        if is_same_server(&self.extension_management_server_service.get_extension_management_server(extension), &self.extension_management_server_service.web_extension_management_server())
            && self.extension_manifest_properties_service.can_execute_on_web(&extension.manifest)
        {
            return false;
        }

        true
    }

    fn is_disabled_by_extension_kind(&self, extension: &IExtension) -> bool {
        if self.extension_management_server_service.remote_extension_management_server().is_some() || self.extension_management_server_service.web_extension_management_server().is_some() {
            let install_location = self.extension_management_server_service.get_extension_install_location(extension);
            for extension_kind in self.extension_manifest_properties_service.get_extension_kind(&extension.manifest) {
                if extension_kind == ExtensionKind::Ui && install_location == Some(ExtensionInstallLocation::Local) {
                    return false;
                }
                if extension_kind == ExtensionKind::Workspace && install_location == Some(ExtensionInstallLocation::Remote) {
                    return false;
                }
                if extension_kind == ExtensionKind::Web {
                    if self.extension_management_server_service.web_extension_management_server().is_some() {
                        /* web */
                        if install_location == Some(ExtensionInstallLocation::Web) || install_location == Some(ExtensionInstallLocation::Remote) {
                            return false;
                        }
                    } else if install_location == Some(ExtensionInstallLocation::Local) {
                        let enable_local_web_worker = self.configuration_service.get_value(Some(WEB_WORKER_EXT_HOST_CONFIG), None);
                        let is_auto = matches!(&enable_local_web_worker, Dynamic::String(text) if text.to_string_lossy() == "auto");
                        if matches!(enable_local_web_worker, Dynamic::Bool(true)) || is_auto {
                            // Web extensions are enabled on all configurations
                            return false;
                        }
                    }
                }
            }
            return true;
        }
        false
    }

    fn is_disabled_by_workspace_trust(&self, extension: &IExtension, workspace_type: WorkspaceType) -> bool {
        if workspace_type.trusted {
            return false;
        }

        if self.context_service.is_inside_workspace(&local_extension_uri(extension)) {
            return true;
        }

        matches!(self.extension_manifest_properties_service.get_extension_untrusted_workspace_support_type(&extension.manifest), BoolOrLimited::Bool(false))
    }

    fn is_disabled_by_extension_dependency(
        &self,
        extension: &IExtension,
        extensions: &[ExtensionRef],
        workspace_type: WorkspaceType,
        computed_enablement_states: &mut ComputedEnablementStates,
    ) -> bool {
        let Some(extension_dependencies) = &extension.manifest.extension_dependencies else { return false };

        // Find dependency that is from the same server or does not exports any API
        let server_of_extension = self.extension_management_server_service.get_extension_management_server(extension);
        let dependency_extensions: Vec<&ExtensionRef> = extensions
            .iter()
            .filter(|e| {
                extension_dependencies.iter().any(|id| {
                    is_extension_id(id, &e.identifier)
                        && (is_same_server(&self.extension_management_server_service.get_extension_management_server(e), &server_of_extension)
                            || ((has_entry_point(&e.manifest.main) || has_entry_point(&e.manifest.browser)) && e.manifest.api.as_deref() == Some("none")))
                })
            })
            .collect();

        if dependency_extensions.is_empty() {
            return false;
        }

        let key = identity_of(extension);
        let has_enablement_state = computed_enablement_states.contains_key(&key);
        if !has_enablement_state {
            // Placeholder to handle cyclic deps
            computed_enablement_states.insert(key, EnablementState::EnabledGlobally);
        }
        let mut disabled = false;
        for dependency_extension in dependency_extensions {
            let enablement_state = self.compute_enablement_state(dependency_extension, extensions, workspace_type, computed_enablement_states);
            if !self.is_enabled_enablement_state(enablement_state) && enablement_state != EnablementState::DisabledByExtensionKind {
                disabled = true;
                break;
            }
        }
        if !has_enablement_state {
            // remove the placeholder
            computed_enablement_states.remove(&key);
        }

        disabled
    }

    fn get_user_enablement_state(&self, identifier: &ExtensionIdentifierInfo) -> EnablementState {
        if self.has_workspace() {
            if self.get_workspace_enabled_extensions().iter().any(|e| are_same_extensions(e, identifier)) {
                return EnablementState::EnabledWorkspace;
            }

            if self.get_workspace_disabled_extensions().iter().any(|e| are_same_extensions(e, identifier)) {
                return EnablementState::DisabledWorkspace;
            }
        }
        if self.is_disabled_globally_by_identifier(identifier) {
            return EnablementState::DisabledGlobally;
        }
        EnablementState::EnabledGlobally
    }

    fn is_disabled_globally_by_identifier(&self, identifier: &ExtensionIdentifierInfo) -> bool {
        self.global_extension_enablement_service.get_disabled_extensions().iter().any(|e| are_same_extensions(e, identifier))
    }

    fn is_disabled_by_unification(&self, identifier: &ExtensionIdentifierInfo) -> bool {
        self.extension_unification_enabled.get() && self.completions_extension_id.as_deref() == Some(identifier.id.to_lowercase().as_str())
    }

    fn is_disabled_by_sessions_window(&self, extension: &IExtension) -> bool {
        if !self.environment_service.is_sessions_window() {
            return false;
        }

        let id = extension.identifier.id.to_lowercase();

        // Allow-listed extensions are always enabled in the sessions window.
        if self.sessions_window_allowed_extensions.contains(&id) {
            return false;
        }

        if extension.is_builtin && self.chat_extension_id.as_deref() == Some(id.as_str()) {
            return false;
        }

        !self.extension_manifest_properties_service.can_execute_on_sessions_window(&extension.manifest, extension.is_builtin)
    }

    fn enable_extension(&self, identifier: &ExtensionIdentifierInfo) -> Promise<bool> {
        self.remove_from_workspace_disabled_extensions(identifier);
        self.remove_from_workspace_enabled_extensions(identifier);
        self.global_extension_enablement_service.enable_extension(identifier, Some(SOURCE))
    }

    fn disable_extension(&self, identifier: &ExtensionIdentifierInfo) -> Promise<bool> {
        self.remove_from_workspace_disabled_extensions(identifier);
        self.remove_from_workspace_enabled_extensions(identifier);
        self.global_extension_enablement_service.disable_extension(identifier, Some(SOURCE))
    }

    fn enable_extension_in_workspace(&self, identifier: &ExtensionIdentifierInfo) {
        self.remove_from_workspace_disabled_extensions(identifier);
        self.add_to_workspace_enabled_extensions(identifier);
    }

    fn disable_extension_in_workspace(&self, identifier: &ExtensionIdentifierInfo) {
        self.add_to_workspace_disabled_extensions(identifier);
        self.remove_from_workspace_enabled_extensions(identifier);
    }

    fn add_to_workspace_disabled_extensions(&self, identifier: &ExtensionIdentifierInfo) -> bool {
        if !self.has_workspace() {
            return false;
        }
        let mut disabled_extensions = self.get_workspace_disabled_extensions();
        if disabled_extensions.iter().all(|e| !are_same_extensions(e, identifier)) {
            disabled_extensions.push(identifier.clone());
            self.set_disabled_extensions(&disabled_extensions);
            return true;
        }
        false
    }

    fn remove_from_workspace_disabled_extensions(&self, identifier: &ExtensionIdentifierInfo) -> bool {
        if !self.has_workspace() {
            return false;
        }
        let mut disabled_extensions = self.get_workspace_disabled_extensions();
        if let Some(index) = disabled_extensions.iter().position(|disabled_extension| are_same_extensions(disabled_extension, identifier)) {
            disabled_extensions.remove(index);
            self.set_disabled_extensions(&disabled_extensions);
            return true;
        }
        false
    }

    fn add_to_workspace_enabled_extensions(&self, identifier: &ExtensionIdentifierInfo) -> bool {
        if !self.has_workspace() {
            return false;
        }
        let mut enabled_extensions = self.get_workspace_enabled_extensions();
        if enabled_extensions.iter().all(|e| !are_same_extensions(e, identifier)) {
            enabled_extensions.push(identifier.clone());
            self.set_enabled_extensions(&enabled_extensions);
            return true;
        }
        false
    }

    fn remove_from_workspace_enabled_extensions(&self, identifier: &ExtensionIdentifierInfo) -> bool {
        if !self.has_workspace() {
            return false;
        }
        let mut enabled_extensions = self.get_workspace_enabled_extensions();
        if let Some(index) = enabled_extensions.iter().position(|enabled_extension| are_same_extensions(enabled_extension, identifier)) {
            enabled_extensions.remove(index);
            self.set_enabled_extensions(&enabled_extensions);
            return true;
        }
        false
    }

    /// `_getWorkspaceEnabledExtensions` (protegido no original).
    pub fn get_workspace_enabled_extensions(&self) -> Vec<ExtensionIdentifierInfo> {
        self.get_extensions(ENABLED_EXTENSIONS_STORAGE_PATH)
    }

    fn set_enabled_extensions(&self, enabled_extensions: &[ExtensionIdentifierInfo]) {
        self.set_extensions(ENABLED_EXTENSIONS_STORAGE_PATH, enabled_extensions);
    }

    /// `_getWorkspaceDisabledExtensions` (protegido no original).
    pub fn get_workspace_disabled_extensions(&self) -> Vec<ExtensionIdentifierInfo> {
        self.get_extensions(DISABLED_EXTENSIONS_STORAGE_PATH)
    }

    fn set_disabled_extensions(&self, disabled_extensions: &[ExtensionIdentifierInfo]) {
        self.set_extensions(DISABLED_EXTENSIONS_STORAGE_PATH, disabled_extensions);
    }

    fn get_extensions(&self, storage_id: &str) -> Vec<ExtensionIdentifierInfo> {
        if !self.has_workspace() {
            return Vec::new();
        }
        self.storage_manager.get(storage_id, StorageScope::Workspace)
    }

    fn set_extensions(&self, storage_id: &str, extensions: &[ExtensionIdentifierInfo]) {
        self.storage_manager.set(storage_id, extensions, StorageScope::Workspace);
    }

    fn on_did_change_globally_disabled_extensions(&self, extension_identifiers: &[ExtensionIdentifierInfo], source: Option<&str>) {
        if source != Some(SOURCE) {
            let weak = self.this.clone();
            let extension_identifiers = extension_identifiers.to_vec();
            self.extensions_manager.when_initialized().then(move |()| {
                let Some(this) = weak.upgrade() else { return };
                let extensions: Vec<ExtensionRef> = this
                    .extensions_manager
                    .extensions()
                    .into_iter()
                    .filter(|installed_extension| extension_identifiers.iter().any(|identifier| are_same_extensions(identifier, &installed_extension.identifier)))
                    .collect();
                this.on_enablement_changed.fire(extensions);
            });
        }
    }

    fn on_did_change_extensions(&self, added: Vec<ExtensionRef>, removed: Vec<ExtensionRef>, is_profile_switch: bool) {
        let mut changed_extensions: Vec<ExtensionRef> = added.into_iter().filter(|e| !self.is_enabled_enablement_state(self.get_enablement_state(e))).collect();
        let existing_disabled_extensions = self.extensions_disabled_extensions.borrow().clone();
        let now_disabled: Vec<ExtensionRef> = self
            .extensions_manager
            .extensions()
            .into_iter()
            .filter(|extension| {
                let enablement_state = self.get_enablement_state(extension);
                enablement_state == EnablementState::DisabledByExtensionDependency
                    || enablement_state == EnablementState::DisabledByAllowlist
                    || enablement_state == EnablementState::DisabledByMalicious
            })
            .collect();
        *self.extensions_disabled_extensions.borrow_mut() = now_disabled.clone();
        for extension in &existing_disabled_extensions {
            if now_disabled.iter().all(|e| !are_same_extensions(&e.identifier, &extension.identifier)) {
                changed_extensions.push(extension.clone());
            }
        }
        for extension in &now_disabled {
            if existing_disabled_extensions.iter().all(|e| !are_same_extensions(&e.identifier, &extension.identifier)) {
                changed_extensions.push(extension.clone());
            }
        }
        if !changed_extensions.is_empty() {
            self.on_enablement_changed.fire(changed_extensions);
        }
        if !is_profile_switch {
            for extension in &removed {
                self.reset(&extension.identifier);
            }
        }
    }

    fn get_workspace_type(&self) -> WorkspaceType {
        WorkspaceType { trusted: self.workspace_trust_management_service.is_workspace_trusted(), is_virtual: is_virtual_workspace(&*self.context_service.get_workspace()) }
    }

    fn reset(&self, extension: &ExtensionIdentifierInfo) {
        self.remove_from_workspace_disabled_extensions(extension);
        self.remove_from_workspace_enabled_extensions(extension);
        self.global_extension_enablement_service.enable_extension(extension, None);
    }

    fn loop_check_for_malicious_extensions(&self) {
        let Some(this) = self.this.upgrade() else { return };
        let weak = self.this.clone();
        let weak_next = self.this.clone();
        this.check_for_malicious_extensions()
            .then_promise(move |()| match weak.upgrade() {
                // every five minutes
                Some(this) => this.delayer.trigger(|| MaybePromise::Value(()), Some(Delay::Millis(MALICIOUS_EXTENSIONS_CHECK_INTERVAL_MS))),
                None => Promise::resolved(()),
            })
            .then(move |()| {
                if let Some(this) = weak_next.upgrade() {
                    this.loop_check_for_malicious_extensions();
                }
            });
    }

    fn check_for_malicious_extensions(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            match this.extension_management_service.get_extensions_control_manifest().await {
                Ok(extensions_control_manifest) => {
                    let malicious: Vec<ExtensionOrPublisher> = extensions_control_manifest.malicious.iter().map(|m| m.extension_or_publisher.clone()).collect();
                    let changed = this.store_malicious_extensions(&malicious);
                    if changed {
                        this.on_did_change_extensions(Vec::new(), Vec::new(), false);
                    }
                }
                Err(error) => this.log_service.error_args(&LogMessage::from(&error), &[]),
            }
            Ok(())
        })
    }

    /// `getMaliciousExtensions`: `getObject(key, APPLICATION, [])`, uma lista de identificadores e de publishers.
    fn get_malicious_extensions(&self) -> Vec<ExtensionOrPublisher> {
        let Some(stored) = self.storage_service.get(MALICIOUS_EXTENSIONS_STORAGE_KEY, StorageScope::Application, None) else { return Vec::new() };
        let Ok(Value::Array(items)) = serde_json::from_str::<Value>(&stored) else { return Vec::new() };
        items
            .into_iter()
            .filter_map(|item| match item {
                Value::String(publisher) => Some(ExtensionOrPublisher::Publisher(publisher)),
                Value::Object(_) => serde_json::from_value::<ExtensionIdentifierInfo>(item).ok().map(ExtensionOrPublisher::Extension),
                _ => None,
            })
            .collect()
    }

    fn get_malicious_extensions_for_check(&self) -> Vec<MaliciousExtensionInfo> {
        if self.malicious_extensions_cache.borrow().is_none() {
            let malicious = self.get_malicious_extensions().into_iter().map(|extension_or_publisher| MaliciousExtensionInfo { extension_or_publisher, learn_more_link: None }).collect();
            *self.malicious_extensions_cache.borrow_mut() = Some(malicious);
        }
        self.malicious_extensions_cache.borrow().clone().unwrap_or_default()
    }

    fn store_malicious_extensions(&self, extensions: &[ExtensionOrPublisher]) -> bool {
        let existing = self.get_malicious_extensions();
        let is_equal = existing.len() == extensions.len()
            && existing.iter().zip(extensions).all(|(a, b)| match (a, b) {
                (ExtensionOrPublisher::Extension(a), ExtensionOrPublisher::Extension(b)) => are_same_extensions(a, b),
                (ExtensionOrPublisher::Publisher(a), ExtensionOrPublisher::Publisher(b)) => a == b,
                _ => false,
            });
        if is_equal {
            return false;
        }
        *self.malicious_extensions_cache.borrow_mut() = None;
        let items: Vec<Value> = extensions
            .iter()
            .map(|extension| match extension {
                ExtensionOrPublisher::Publisher(publisher) => Value::String(publisher.clone()),
                ExtensionOrPublisher::Extension(identifier) => serde_json::to_value(identifier).unwrap_or(Value::Null),
            })
            .collect();
        self.storage_service.store(MALICIOUS_EXTENSIONS_STORAGE_KEY, StorageValue::String(Value::Array(items).to_string()), StorageScope::Application, StorageTarget::Machine);
        true
    }
}

impl IDisposable for ExtensionEnablementService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IWorkbenchExtensionEnablementService for ExtensionEnablementService {
    fn on_enablement_changed(&self) -> Event<Vec<ExtensionRef>> {
        self.on_enablement_changed.event()
    }

    fn get_enablement_state(&self, extension: &IExtension) -> EnablementState {
        self.compute_enablement_state(extension, &self.extensions_manager.extensions(), self.get_workspace_type(), &mut ComputedEnablementStates::new())
    }

    fn get_enablement_states(&self, extensions: &[ExtensionRef], workspace_type_overrides: WorkspaceTypeOverrides) -> Vec<EnablementState> {
        let mut extensions_enablements = ComputedEnablementStates::new();
        let mut workspace_type = self.get_workspace_type();
        if let Some(trusted) = workspace_type_overrides.trusted {
            workspace_type.trusted = trusted;
        }
        extensions.iter().map(|extension| self.compute_enablement_state(extension, extensions, workspace_type, &mut extensions_enablements)).collect()
    }

    fn get_dependencies_enablement_states(&self, extension: &IExtension) -> Vec<(ExtensionRef, EnablementState)> {
        dependencies_of(&self.extensions_manager.extensions(), &extension.manifest)
            .into_iter()
            .map(|e| {
                let enablement_state = self.get_enablement_state(&e);
                (e, enablement_state)
            })
            .collect()
    }

    fn can_change_enablement(&self, extension: &IExtension) -> bool {
        self.throw_error_if_cannot_change_enablement(extension, false).is_ok()
    }

    fn can_change_workspace_enablement(&self, extension: &IExtension) -> bool {
        if !self.can_change_enablement(extension) {
            return false;
        }

        self.throw_error_if_cannot_change_workspace_enablement(extension).is_ok()
    }

    fn is_enabled(&self, extension: &IExtension) -> bool {
        let enablement_state = self.get_enablement_state(extension);
        self.is_enabled_enablement_state(enablement_state)
    }

    fn is_enabled_enablement_state(&self, enablement_state: EnablementState) -> bool {
        enablement_state == EnablementState::EnabledByEnvironment || enablement_state == EnablementState::EnabledWorkspace || enablement_state == EnablementState::EnabledGlobally
    }

    fn is_disabled_globally(&self, extension: &IExtension) -> bool {
        self.is_disabled_globally_by_identifier(&extension.identifier)
    }

    fn set_enablement(&self, extensions: Vec<ExtensionRef>, new_state: EnablementState) -> Promise<Vec<bool>> {
        let Some(this) = self.this.upgrade() else { return Promise::rejected(Error::new("ExtensionEnablementService is disposed")) };
        let mut extensions = extensions;
        async_fn(async move {
            this.extensions_manager.when_initialized().await?;

            if new_state == EnablementState::EnabledGlobally || new_state == EnablementState::EnabledWorkspace {
                let installed = this.extensions_manager.extensions();
                let to_enable = this.get_extensions_to_enable_recursively(&extensions, &installed, new_state, EnableRecursivelyOptions { dependencies: true, pack: true }, &mut Vec::new());
                extensions.extend(to_enable);
            }

            let workspace = new_state == EnablementState::DisabledWorkspace || new_state == EnablementState::EnabledWorkspace;
            for extension in &extensions {
                if workspace {
                    this.throw_error_if_cannot_change_workspace_enablement(extension)?;
                } else {
                    this.throw_error_if_cannot_change_enablement(extension, false)?;
                }
            }

            let mut result: Vec<bool> = Vec::new();
            for extension in &extensions {
                let enablement_state = this.get_enablement_state(extension);
                if enablement_state == EnablementState::DisabledByTrustRequirement
                    /* All its disabled dependencies are disabled by Trust Requirement */
                    || (enablement_state == EnablementState::DisabledByExtensionDependency
                        && this
                            .get_dependencies_enablement_states(extension)
                            .iter()
                            .all(|(_, e)| this.is_enabled_enablement_state(*e) || *e == EnablementState::DisabledByTrustRequirement))
                {
                    let trust_state = this.workspace_trust_request_service.request_workspace_trust(None).await?;
                    result.push(trust_state.unwrap_or(false));
                } else {
                    result.push(this.set_user_enablement_state(extension, new_state).await?);
                }
            }

            let changed_extensions: Vec<ExtensionRef> = extensions.iter().zip(&result).filter(|(_, changed)| **changed).map(|(extension, _)| extension.clone()).collect();
            if !changed_extensions.is_empty() {
                this.on_enablement_changed.fire(changed_extensions);
            }
            Ok(result)
        })
    }

    fn update_extensions_enablements_when_workspace_trust_changes(&self) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::rejected(Error::new("ExtensionEnablementService is disposed")) };
        async_fn(async move {
            this.extensions_manager.when_initialized().await?;

            let compute_enablement_states = |workspace_type: WorkspaceType| -> Vec<(ExtensionRef, EnablementState)> {
                let mut extensions_enablements = ComputedEnablementStates::new();
                let extensions = this.extensions_manager.extensions();
                extensions
                    .iter()
                    .map(|extension| (extension.clone(), this.compute_enablement_state(extension, &extensions, workspace_type, &mut extensions_enablements)))
                    .collect()
            };

            let workspace_type = this.get_workspace_type();
            let enablement_states_with_trusted_workspace = compute_enablement_states(WorkspaceType { trusted: true, ..workspace_type });
            let enablement_states_with_untrusted_workspace = compute_enablement_states(WorkspaceType { trusted: false, ..workspace_type });
            let enablement_changed_extensions_because_of_trust: Vec<ExtensionRef> = enablement_states_with_trusted_workspace
                .iter()
                .enumerate()
                .filter(|(index, (_, enablement_state))| *enablement_state != enablement_states_with_untrusted_workspace[*index].1)
                .map(|(_, (extension, _))| extension.clone())
                .collect();

            if !enablement_changed_extensions_because_of_trust.is_empty() {
                this.on_enablement_changed.fire(enablement_changed_extensions_because_of_trust);
            }
            Ok(())
        })
    }
}

/// `ExtensionsManager`: a lista das extensões instaladas (de todos os servidores e do workspace), mantida em dia pelos
/// eventos de instalação, remoção e troca de perfil.
pub struct ExtensionsManager {
    base: Rc<Disposable>,
    this: Weak<ExtensionsManager>,
    extensions: RefCell<Vec<ExtensionRef>>,
    on_did_change_extensions: Rc<Emitter<ExtensionsChangeEvent>>,
    initialize_promise: RefCell<Option<Promise<()>>>,
    disposed: Rc<Cell<bool>>,
    extension_management_service: Rc<dyn IWorkbenchExtensionManagementService>,
    extension_management_server_service: Rc<dyn IExtensionManagementServerService>,
    log_service: LogServiceRef,
}

impl ExtensionsManager {
    pub fn new(
        extension_management_service: Rc<dyn IWorkbenchExtensionManagementService>,
        extension_management_server_service: Rc<dyn IExtensionManagementServerService>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_extensions = Emitter::new();
        base.register(on_did_change_extensions.clone());
        let disposed = Rc::new(Cell::new(false));
        base.register(to_disposable({
            let disposed = disposed.clone();
            move || disposed.set(true)
        }));
        let this = Rc::new_cyclic(|this| Self {
            base,
            this: this.clone(),
            extensions: RefCell::new(Vec::new()),
            on_did_change_extensions,
            initialize_promise: RefCell::new(None),
            disposed,
            extension_management_service,
            extension_management_server_service,
            log_service,
        });
        let initialize_promise = this.initialize();
        *this.initialize_promise.borrow_mut() = Some(initialize_promise);
        this
    }

    /// `extensions`.
    pub fn extensions(&self) -> Vec<ExtensionRef> {
        self.extensions.borrow().clone()
    }

    /// `onDidChangeExtensions`.
    pub fn on_did_change_extensions(&self) -> Event<ExtensionsChangeEvent> {
        self.on_did_change_extensions.event()
    }

    /// `whenInitialized()`.
    pub fn when_initialized(&self) -> Promise<()> {
        self.initialize_promise.borrow().clone().unwrap_or_else(|| Promise::resolved(()))
    }

    fn initialize(&self) -> Promise<()> {
        let Some(this) = self.this.upgrade() else { return Promise::resolved(()) };
        async_fn(async move {
            // `return` quando o gerenciador foi descartado durante a leitura (sem registrar os ouvintes).
            let outcome: Result<bool, Error> = async {
                let mut extensions = this.extension_management_service.get_installed().await?;
                extensions.extend(this.extension_management_service.get_installed_workspace_extensions(true).await?);
                *this.extensions.borrow_mut() = extensions;
                if this.disposed.get() {
                    return Ok(true);
                }
                this.on_did_change_extensions.fire(ExtensionsChangeEvent { added: this.extensions(), removed: Vec::new(), is_profile_switch: false });
                Ok(false)
            }
            .await;
            match outcome {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(error) => this.log_service.error_args(&LogMessage::from(&error), &[]),
            }

            let weak = this.this.clone();
            this.base.register(this.extension_management_service.on_did_install_extensions().listen(move |results| {
                let Some(this) = weak.upgrade() else { return };
                let added: Vec<ExtensionRef> = results
                    .iter()
                    .filter_map(|result| match &result.local {
                        Some(local) if result.operation != InstallOperation::Migrate => Some(local.clone()),
                        _ => None,
                    })
                    .collect();
                this.update_extensions(added, &[], &None, false);
            }));
            let weak = this.this.clone();
            this.base.register(this.extension_management_service.on_did_uninstall_extension().filter(|e| e.error.is_none(), None).listen(move |e| {
                if let Some(this) = weak.upgrade() {
                    this.update_extensions(Vec::new(), std::slice::from_ref(&e.identifier), &e.server, false);
                }
            }));
            let weak = this.this.clone();
            this.base.register(this.extension_management_service.on_did_change_profile().listen(move |e| {
                if let Some(this) = weak.upgrade() {
                    let removed: Vec<ExtensionIdentifierInfo> = e.removed.iter().map(|extension| extension.identifier.clone()).collect();
                    this.update_extensions(e.added.clone(), &removed, &e.server, true);
                }
            }));
            Ok(())
        })
    }

    fn update_extensions(&self, added: Vec<ExtensionRef>, identifiers: &[ExtensionIdentifierInfo], server: &Option<Rc<IExtensionManagementServer>>, is_profile_switch: bool) {
        if !added.is_empty() {
            for extension in &added {
                let extension_server = self.extension_management_server_service.get_extension_management_server(extension);
                let index = self.extensions.borrow().iter().position(|e| {
                    are_same_extensions(&e.identifier, &extension.identifier) && is_same_server(&self.extension_management_server_service.get_extension_management_server(e), &extension_server)
                });
                if let Some(index) = index {
                    self.extensions.borrow_mut().remove(index);
                }
            }
            self.extensions.borrow_mut().extend(added.iter().cloned());
        }
        let mut removed: Vec<ExtensionRef> = Vec::new();
        for identifier in identifiers {
            let index = self.extensions.borrow().iter().position(|e| {
                are_same_extensions(&e.identifier, identifier)
                    && server.as_ref().is_some_and(|server| {
                        self.extension_management_server_service.get_extension_management_server(e).is_some_and(|extension_server| Rc::ptr_eq(&extension_server, server))
                    })
            });
            if let Some(index) = index {
                removed.push(self.extensions.borrow_mut().remove(index));
            }
        }
        if !added.is_empty() || !removed.is_empty() {
            self.on_did_change_extensions.fire(ExtensionsChangeEvent { added, removed, is_profile_switch });
        }
    }
}

impl IDisposable for ExtensionsManager {
    fn dispose(&self) {
        self.base.dispose();
    }
}
