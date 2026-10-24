//! Porte de `src/vs/platform/userDataSync/common/userDataSyncService.ts`: o serviço que coordena a
//! sincronização (`UserDataSyncService`) e o sincronizador de um perfil (`ProfileSynchronizer`).
//!
//! Portado: `createSyncTask` e a tarefa que ele devolve, `sync` do perfil padrão e dos perfis remotos
//! (`syncRemoteProfiles`), `stop`, `resetLocal`, `resetRemote`, `reset`, `accept`, `hasLocalData`,
//! `hasPreviouslySynced`, `performAction`, `cleanUpStaleStorageData`, o status, os conflitos e o horário da
//! última sincronização.
//!
//! Fora do porte (e por quê):
//! - `createManualSyncTask` e `applyManualSync` (a sincronização manual, com `ProfileSynchronizer.apply`),
//!   `resolveContent`, `replace`, `cleanUpRemoteData` (pedem o `IUserDataSyncResourceProviderService` e
//!   `userDataSyncMachines.ts`), `saveRemoteActivityData` e `extractActivityData`: nenhum entra no caminho de
//!   `createSyncTask().run()`. Por isso [`UserDataSyncService`] não implementa o trait
//!   [`super::user_data_sync::IUserDataSyncService`] inteiro: expõe os métodos portados.
//! - Os sincronizadores de `settings`, `snippets`, `prompts`, `mcp`, `extensions` e `globalState` ainda não
//!   existem no porte: [`ProfileSynchronizer::create_synchronizer`] devolve `None` para eles e o
//!   `registerSynchronizer` os pula (o de extensões também pediria o `IExtensionGalleryService`). Existem os de
//!   perfis, `keybindings` e `tasks`. Com isso a leitura remota de `settingsSync` de `getUserDataSyncConfiguration`
//!   (que vem do `SettingsSynchroniser`) é só a configuração local.
//!
//! Decisões de tradução:
//! - `IUserDataManifest | IUserDataSyncLatestData | null` é `Option<`[`ManifestOrLatestData`]`>`: o
//!   `isUserDataManifest` do original é a variante.
//! - Os sincronizadores de um perfil são [`Synchroniser`], um `enum` com uma variante por sincronizador portado;
//!   o `as UserDataProfilesManifestSynchroniser` do original é [`Synchroniser::into_profiles_manifest`] (e
//!   [`Synchroniser::into_keybindings`], [`Synchroniser::into_tasks`] para os outros).
//! - `instantiationService.createInstance(X, ...)` é o construtor direto, com os serviços do
//!   [`SynchroniserServices`] e o `IUserDataSyncUtilService` que o `UserDataSyncService` recebe.
//! - Os `Promise` da tarefa de sincronização são `async_fn`; `finally` é o resultado guardado e o fim executado
//!   antes de devolvê-lo.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::arrays::equals;
use je_base::async_::{RunOnceScheduler, async_fn, create_cancelable_promise};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, number_to_js_string};
use je_base::error_message::to_error_message_of;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use je_base::promise::{CancelablePromise, Promise};
use je_base::runtime::date_now;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_storage::StorageValue;

use super::abstract_synchronizer::SynchroniserServices;
use super::keybindings_sync::KeybindingsSynchroniser;
use super::tasks_sync::TasksSynchroniser;
use super::user_data_profiles_manifest_sync::UserDataProfilesManifestSynchroniser;
use super::user_data_sync::{
    ALL_SYNC_RESOURCES, ApplyMode, IUserDataManifest, IUserDataSyncConfiguration, IUserDataSyncLatestData, IUserDataSyncResource, IUserDataSyncResourceConflicts, IUserDataSyncResourceError,
    IUserDataSyncStoreManagementService, IUserDataSyncTask, IUserDataSyncUtilService, IUserDataSynchroniser, ISyncUserDataProfile, RefOrUserData, SyncResource, SyncStatus,
    USER_DATA_SYNC_CONFIGURATION_SCOPE, UserDataSyncError, UserDataSyncErrorCode, UserDataSyncStoreError, create_sync_headers, to_user_data_sync_error,
};
use crate::log::{LogArg, LogMessage};
use crate::storage::{StorageScope, StorageTarget};
use crate::user_data_profile::{IUserDataProfilesService, UseDefaultProfileFlags, UserDataProfile};

const LAST_SYNC_TIME_KEY: &str = "sync.lastSyncTime";

// region: dados do servidor

/// `IUserDataManifest | IUserDataSyncLatestData`: o que `createSyncTask` e a sincronização manual recebem do
/// servidor.
#[derive(Clone, Debug, PartialEq)]
pub enum ManifestOrLatestData {
    Manifest(IUserDataManifest),
    Latest(IUserDataSyncLatestData),
}

/// `getRefOrUserData`: o `ref` (do manifesto) ou os dados (do `download/latest`) de um recurso.
pub fn get_ref_or_user_data(manifest_or_latest_data: Option<&ManifestOrLatestData>, collection: Option<&str>, resource: SyncResource) -> Option<RefOrUserData> {
    let collection = collection.filter(|collection| !collection.is_empty());
    match manifest_or_latest_data? {
        ManifestOrLatestData::Manifest(manifest) => {
            let latest = match collection {
                Some(collection) => manifest.collections.as_ref()?.get(collection)?.latest.as_ref()?,
                None => manifest.latest.as_ref()?,
            };
            latest.get(resource.as_str()).cloned().map(RefOrUserData::Ref)
        }
        ManifestOrLatestData::Latest(latest_data) => {
            let resources = match collection {
                Some(collection) => latest_data.collections.as_ref()?.get(collection)?.resources.as_ref()?,
                None => latest_data.resources.as_ref()?,
            };
            resources.get(resource.as_str()).cloned().map(RefOrUserData::UserData)
        }
    }
}

// endregion

// region: erros

/// `canBailout`: os erros que interrompem a sincronização em vez de seguir para o próximo recurso.
fn can_bailout(error: &Error) -> bool {
    UserDataSyncError::from_error(error).is_some_and(|sync_error| {
        matches!(
            sync_error.code,
            UserDataSyncErrorCode::MethodNotFound
                | UserDataSyncErrorCode::TooLarge
                | UserDataSyncErrorCode::TooManyRequests
                | UserDataSyncErrorCode::TooManyRequestsAndRetryAfter
                | UserDataSyncErrorCode::LocalTooManyRequests
                | UserDataSyncErrorCode::LocalTooManyProfiles
                | UserDataSyncErrorCode::Gone
                | UserDataSyncErrorCode::UpgradeRequired
                | UserDataSyncErrorCode::IncompatibleRemoteContent
                | UserDataSyncErrorCode::IncompatibleLocalContent
        )
    })
}

/// `UserDataSyncError.toUserDataSyncError(error)` como o erro que se lança: um erro que já é de sincronização
/// segue como está (com as propriedades do `UserDataSyncStoreError`).
fn to_sync_error(error: &Error) -> Error {
    if UserDataSyncError::from_error(error).is_some() { error.clone() } else { to_user_data_sync_error(error).into_error() }
}

/// `reportUserDataSyncError`.
fn report_user_data_sync_error(
    error: &Error,
    execution_id: &str,
    user_data_sync_store_management_service: &dyn IUserDataSyncStoreManagementService,
    telemetry_service: &dyn crate::telemetry::ITelemetryService,
) {
    let sync_error = to_user_data_sync_error(error);
    let store_error = UserDataSyncStoreError::from_error(error);
    let service = user_data_sync_store_management_service.user_data_sync_store().map(|store| store.url.to_string(false).to_string_lossy()).unwrap_or_default();
    telemetry_service.public_log2(
        "sync/error",
        Some(Dynamic::object([
            ("code", Dynamic::string(sync_error.code.as_str())),
            ("service", Dynamic::string(&service)),
            ("serverCode", store_error.as_ref().map_or(Dynamic::Undefined, |store_error| Dynamic::string(&store_error.server_code.map_or("undefined".to_string(), number_to_js_string)))),
            ("url", store_error.as_ref().map_or(Dynamic::Undefined, |store_error| Dynamic::string(&store_error.url))),
            ("resource", sync_error.resource.map_or(Dynamic::Undefined, |resource| Dynamic::string(resource.as_str()))),
            ("executionId", Dynamic::string(execution_id)),
        ])),
    );
}

// endregion

// region: sincronizadores

/// O sincronizador de um recurso que o `ProfileSynchronizer` guarda. Uma variante por sincronizador portado.
#[derive(Clone)]
pub enum Synchroniser {
    Profiles(Rc<UserDataProfilesManifestSynchroniser>),
    Keybindings(Rc<KeybindingsSynchroniser>),
    Tasks(Rc<TasksSynchroniser>),
}

impl Synchroniser {
    pub fn resource(&self) -> SyncResource {
        match self {
            Self::Profiles(_) => SyncResource::Profiles,
            Self::Keybindings(_) => SyncResource::Keybindings,
            Self::Tasks(_) => SyncResource::Tasks,
        }
    }

    /// O sincronizador como o `IUserDataSynchroniser` que o serviço usa.
    pub fn as_dyn(&self) -> Rc<dyn IUserDataSynchroniser> {
        match self {
            Self::Profiles(synchroniser) => synchroniser.base.clone(),
            Self::Keybindings(synchroniser) => synchroniser.base(),
            Self::Tasks(synchroniser) => synchroniser.base(),
        }
    }

    /// `as UserDataProfilesManifestSynchroniser`.
    pub fn into_profiles_manifest(self) -> Option<Rc<UserDataProfilesManifestSynchroniser>> {
        match self {
            Self::Profiles(synchroniser) => Some(synchroniser),
            Self::Keybindings(_) | Self::Tasks(_) => None,
        }
    }

    /// `as KeybindingsSynchroniser`.
    pub fn into_keybindings(self) -> Option<Rc<KeybindingsSynchroniser>> {
        match self {
            Self::Keybindings(synchroniser) => Some(synchroniser),
            Self::Profiles(_) | Self::Tasks(_) => None,
        }
    }

    /// `as TasksSynchroniser`.
    pub fn into_tasks(self) -> Option<Rc<TasksSynchroniser>> {
        match self {
            Self::Tasks(synchroniser) => Some(synchroniser),
            Self::Profiles(_) | Self::Keybindings(_) => None,
        }
    }

    /// `disposables.add(synchroniser)`.
    fn add_to(&self, store: &DisposableStore) {
        match self {
            Self::Profiles(synchroniser) => {
                store.add_rc(synchroniser.clone());
            }
            Self::Keybindings(synchroniser) => {
                store.add_rc(synchroniser.clone());
            }
            Self::Tasks(synchroniser) => {
                store.add_rc(synchroniser.clone());
            }
        }
    }
}

/// `this.profile.useDefaultFlags?.[syncResource]`.
fn uses_default_profile(flags: Option<UseDefaultProfileFlags>, resource: SyncResource) -> bool {
    let Some(flags) = flags else { return false };
    match resource {
        SyncResource::Settings => flags.settings,
        SyncResource::Keybindings => flags.keybindings,
        SyncResource::Snippets => flags.snippets,
        SyncResource::Prompts => flags.prompts,
        SyncResource::Tasks => flags.tasks,
        SyncResource::Mcp => flags.mcp,
        SyncResource::Extensions => flags.extensions,
        SyncResource::GlobalState => flags.global_state,
        SyncResource::Profiles | SyncResource::WorkspaceState => None,
    }
    .unwrap_or(false)
}

/// `ProfileSynchronizer.getOrder`.
fn get_order(resource: SyncResource) -> u8 {
    match resource {
        SyncResource::Settings => 0,
        SyncResource::Keybindings => 1,
        SyncResource::Snippets => 2,
        SyncResource::Tasks => 3,
        SyncResource::Mcp => 4,
        SyncResource::GlobalState => 5,
        SyncResource::Extensions => 6,
        SyncResource::Prompts => 7,
        SyncResource::Profiles => 8,
        SyncResource::WorkspaceState => 9,
    }
}

struct EnabledSynchroniser {
    synchroniser: Synchroniser,
    order: u8,
    disposables: Rc<DisposableStore>,
}

struct ProfileState {
    status: SyncStatus,
    conflicts: Vec<IUserDataSyncResourceConflicts>,
}

/// `ProfileSynchronizer`: os sincronizadores de um perfil.
pub struct ProfileSynchronizer {
    me: Weak<Self>,
    base: Rc<Disposable>,
    pub profile: UserDataProfile,
    pub collection: Option<String>,
    services: SynchroniserServices,
    user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    user_data_sync_store_management_service: Rc<dyn IUserDataSyncStoreManagementService>,
    user_data_sync_util_service: Rc<dyn IUserDataSyncUtilService>,
    enabled: RefCell<Vec<EnabledSynchroniser>>,
    state: RefCell<ProfileState>,
    on_did_change_status: Rc<Emitter<SyncStatus>>,
    on_did_change_local: Rc<Emitter<SyncResource>>,
    on_did_change_conflicts: Rc<Emitter<Vec<IUserDataSyncResourceConflicts>>>,
}

impl ProfileSynchronizer {
    pub fn new(
        profile: UserDataProfile,
        collection: Option<String>,
        services: SynchroniserServices,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        user_data_sync_store_management_service: Rc<dyn IUserDataSyncStoreManagementService>,
        user_data_sync_util_service: Rc<dyn IUserDataSyncUtilService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_status = Emitter::new();
        base.register(on_did_change_status.clone());
        let on_did_change_local = Emitter::new();
        base.register(on_did_change_local.clone());
        let on_did_change_conflicts = Emitter::new();
        base.register(on_did_change_conflicts.clone());

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            profile,
            collection,
            services,
            user_data_profiles_service,
            user_data_sync_store_management_service,
            user_data_sync_util_service,
            enabled: RefCell::new(Vec::new()),
            state: RefCell::new(ProfileState { status: SyncStatus::Idle, conflicts: Vec::new() }),
            on_did_change_status,
            on_did_change_local,
            on_did_change_conflicts,
        });

        let weak = Rc::downgrade(&this);
        this.base.register(this.services.user_data_sync_enablement_service.on_did_change_resource_enablement().listen(move |event| {
            let (sync_resource, enablement) = *event;
            if let Some(this) = weak.upgrade() {
                this.on_did_change_resource_enablement(sync_resource, enablement);
            }
        }));
        let weak = Rc::downgrade(&this);
        this.base.register(to_disposable(move || {
            let enabled: Vec<EnabledSynchroniser> = match weak.upgrade() {
                Some(this) => this.enabled.borrow_mut().drain(..).collect(),
                None => Vec::new(),
            };
            for entry in enabled {
                entry.disposables.dispose();
            }
        }));
        for sync_resource in ALL_SYNC_RESOURCES {
            if this.services.user_data_sync_enablement_service.is_resource_enabled(sync_resource, None) {
                this.register_synchronizer(sync_resource);
            }
        }
        this
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o sincronizador do perfil está vivo enquanto é usado")
    }

    /// `enabled`: ordenados pela ordem de sincronização.
    pub fn enabled(&self) -> Vec<Synchroniser> {
        let mut enabled = self.enabled.borrow_mut();
        enabled.sort_by_key(|entry| entry.order);
        enabled.iter().map(|entry| entry.synchroniser.clone()).collect()
    }

    /// `disabled`.
    pub fn disabled(&self) -> Vec<SyncResource> {
        ALL_SYNC_RESOURCES.into_iter().filter(|sync_resource| !self.services.user_data_sync_enablement_service.is_resource_enabled(*sync_resource, None)).collect()
    }

    pub fn status(&self) -> SyncStatus {
        self.state.borrow().status
    }

    pub fn on_did_change_status(&self) -> Event<SyncStatus> {
        self.on_did_change_status.event()
    }

    pub fn on_did_change_local(&self) -> Event<SyncResource> {
        self.on_did_change_local.event()
    }

    pub fn conflicts(&self) -> Vec<IUserDataSyncResourceConflicts> {
        self.state.borrow().conflicts.clone()
    }

    pub fn on_did_change_conflicts(&self) -> Event<Vec<IUserDataSyncResourceConflicts>> {
        self.on_did_change_conflicts.event()
    }

    fn on_did_change_resource_enablement(&self, sync_resource: SyncResource, enabled: bool) {
        if enabled {
            self.register_synchronizer(sync_resource);
        } else {
            self.de_register_synchronizer(sync_resource);
        }
    }

    fn register_synchronizer(&self, sync_resource: SyncResource) {
        if self.enabled.borrow().iter().any(|entry| entry.synchroniser.resource() == sync_resource) {
            return;
        }
        if sync_resource == SyncResource::Profiles && !self.profile.is_default {
            return;
        }
        if sync_resource == SyncResource::WorkspaceState {
            return;
        }
        if sync_resource != SyncResource::Profiles && uses_default_profile(self.profile.use_default_flags, sync_resource) {
            self.services.log_service.debug(&format!("Skipping syncing {} in {} because it is already synced by default profile", sync_resource.as_str(), self.profile.name));
            return;
        }
        let Some(synchronizer) = self.create_synchronizer(sync_resource) else { return };
        let disposables = DisposableStore::new();
        synchronizer.add_to(&disposables);
        let dyn_synchronizer = synchronizer.as_dyn();
        let weak = self.me.clone();
        disposables.add(dyn_synchronizer.on_did_change_status().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_status();
                }
            }
        }));
        disposables.add(dyn_synchronizer.on_did_change_conflicts().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_conflicts();
                }
            }
        }));
        disposables.add(dyn_synchronizer.on_did_change_local().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_local.fire(sync_resource);
            }
        }));
        let order = get_order(sync_resource);
        self.enabled.borrow_mut().push(EnabledSynchroniser { synchroniser: synchronizer, order, disposables });
    }

    fn de_register_synchronizer(&self, sync_resource: SyncResource) {
        let index = self.enabled.borrow().iter().position(|entry| entry.synchroniser.resource() == sync_resource);
        if let Some(index) = index {
            let entry = self.enabled.borrow_mut().remove(index);
            entry.disposables.dispose();
            self.update_status();
            let log_service = self.services.log_service.clone();
            let _ = entry.synchroniser.as_dyn().stop().then_result(move |result| {
                if let Err(error) = result {
                    log_service.error_args(&LogMessage::from(&error), &[]);
                }
                Ok(())
            });
        }
    }

    /// `createSynchronizer`: `None` para os recursos cujo sincronizador ainda não foi portado.
    pub fn create_synchronizer(&self, sync_resource: SyncResource) -> Option<Synchroniser> {
        match sync_resource {
            SyncResource::Profiles => Some(Synchroniser::Profiles(UserDataProfilesManifestSynchroniser::new(
                self.profile.clone(),
                self.collection.clone(),
                self.user_data_profiles_service.clone(),
                self.services.clone(),
            ))),
            SyncResource::Keybindings => Some(Synchroniser::Keybindings(KeybindingsSynchroniser::new(
                self.profile.clone(),
                self.collection.clone(),
                self.services.clone(),
                self.user_data_sync_util_service.clone(),
            ))),
            SyncResource::Tasks => Some(Synchroniser::Tasks(TasksSynchroniser::for_profile(self.profile.clone(), self.collection.clone(), self.services.clone()))),
            _ => None,
        }
    }

    /// `sync`: os erros de cada recurso que falhou sem interromper os outros.
    pub fn sync(&self, manifest_or_latest_data: Option<ManifestOrLatestData>, preview: bool, execution_id: String, token: CancellationToken) -> Promise<Vec<(SyncResource, UserDataSyncError)>> {
        let this = self.rc();
        async_fn(async move {
            // Return if cancellation is requested
            if token.is_cancellation_requested() {
                return Ok(Vec::new());
            }

            let synchronizers = this.enabled();
            if synchronizers.is_empty() {
                return Ok(Vec::new());
            }

            let result = this.sync_synchronizers(synchronizers, manifest_or_latest_data, preview, &execution_id, token).await;
            this.update_status();
            result
        })
    }

    async fn sync_synchronizers(
        self: &Rc<Self>,
        synchronizers: Vec<Synchroniser>,
        manifest_or_latest_data: Option<ManifestOrLatestData>,
        preview: bool,
        execution_id: &str,
        token: CancellationToken,
    ) -> Result<Vec<(SyncResource, UserDataSyncError)>, Error> {
        let mut sync_errors = Vec::new();
        let sync_headers = create_sync_headers(execution_id);
        let user_data_sync_configuration = if preview { self.get_user_data_sync_configuration() } else { self.get_local_user_data_sync_configuration() };
        for synchroniser in synchronizers {
            // Return if cancellation is requested
            if token.is_cancellation_requested() {
                return Ok(Vec::new());
            }

            let resource = synchroniser.resource();
            // Return if resource is not enabled
            if !self.services.user_data_sync_enablement_service.is_resource_enabled(resource, None) {
                return Ok(Vec::new());
            }

            let ref_or_user_data = get_ref_or_user_data(manifest_or_latest_data.as_ref(), self.collection.as_deref(), resource);
            if let Err(error) = synchroniser.as_dyn().sync(ref_or_user_data, preview, user_data_sync_configuration.clone(), sync_headers.clone()).await {
                report_user_data_sync_error(&error, execution_id, self.user_data_sync_store_management_service.as_ref(), self.services.telemetry_service.as_ref());
                if can_bailout(&error) {
                    return Err(to_sync_error(&error));
                }

                // Log and and continue
                self.services.log_service.error_args(&LogMessage::from(&error), &[]);
                self.services.log_service.error(&format!("{}: {}", resource.as_str(), to_error_message_of(&error, false)));
                sync_errors.push((resource, to_user_data_sync_error(&error)));
            }
        }

        Ok(sync_errors)
    }

    /// `stop`.
    pub fn stop(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            for synchroniser in this.enabled() {
                let synchroniser = synchroniser.as_dyn();
                if synchroniser.status() != SyncStatus::Idle
                    && let Err(error) = synchroniser.stop().await
                {
                    this.services.log_service.error_args(&LogMessage::from(&error), &[]);
                }
            }
            Ok(())
        })
    }

    /// `resetLocal`.
    pub fn reset_local(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            for synchroniser in this.enabled() {
                let resource = synchroniser.resource();
                if let Err(error) = synchroniser.as_dyn().reset_local().await {
                    this.services.log_service.error(&format!("{}: {}", resource.as_str(), to_error_message_of(&error, false)));
                    this.services.log_service.error_args(&LogMessage::from(&error), &[]);
                }
            }
            Ok(())
        })
    }

    /// `getUserDataSyncConfiguration`. A leitura remota de `settingsSync` pede o `SettingsSynchroniser`, que
    /// não foi portado: sobra a configuração local.
    fn get_user_data_sync_configuration(&self) -> IUserDataSyncConfiguration {
        if !self.profile.is_default {
            return IUserDataSyncConfiguration::default();
        }
        self.get_local_user_data_sync_configuration()
    }

    fn get_local_user_data_sync_configuration(&self) -> IUserDataSyncConfiguration {
        IUserDataSyncConfiguration::from_dynamic(&self.services.configuration_service.get_value(Some(USER_DATA_SYNC_CONFIGURATION_SCOPE), None))
    }

    fn set_status(&self, status: SyncStatus) {
        let changed = {
            let mut state = self.state.borrow_mut();
            let changed = state.status != status;
            state.status = status;
            changed
        };
        if changed {
            self.on_did_change_status.fire(status);
        }
    }

    fn update_status(&self) {
        self.update_conflicts();
        let statuses: Vec<SyncStatus> = self.enabled().iter().map(|synchroniser| synchroniser.as_dyn().status()).collect();
        if statuses.contains(&SyncStatus::HasConflicts) {
            return self.set_status(SyncStatus::HasConflicts);
        }
        if statuses.contains(&SyncStatus::Syncing) {
            return self.set_status(SyncStatus::Syncing);
        }
        self.set_status(SyncStatus::Idle)
    }

    fn update_conflicts(&self) {
        let conflicts: Vec<IUserDataSyncResourceConflicts> = self
            .enabled()
            .iter()
            .map(Synchroniser::as_dyn)
            .filter(|synchroniser| synchroniser.status() == SyncStatus::HasConflicts)
            .map(|synchroniser| synchroniser.conflicts())
            .filter(|conflicts| !conflicts.conflicts.is_empty())
            .collect();
        let ext_uri = &self.services.ext_uri;
        let changed = !equals(Some(self.state.borrow().conflicts.as_slice()), Some(conflicts.as_slice()), |a, b| {
            a.sync_resource == b.sync_resource
                && equals(Some(a.conflicts.as_slice()), Some(b.conflicts.as_slice()), |a, b| ext_uri.is_equal(Some(&a.preview_resource), Some(&b.preview_resource), false))
        });
        if changed {
            self.state.borrow_mut().conflicts = conflicts.clone();
            self.on_did_change_conflicts.fire(conflicts);
        }
    }
}

impl IDisposable for ProfileSynchronizer {
    fn dispose(&self) {
        self.base.dispose();
    }
}

// endregion

// region: serviço

struct ServiceState {
    status: SyncStatus,
    conflicts: Vec<IUserDataSyncResourceConflicts>,
    sync_errors: Vec<IUserDataSyncResourceError>,
    last_sync_time: Option<f64>,
}

type ActiveProfileSynchronizers = IndexMap<String, (Rc<ProfileSynchronizer>, Rc<DisposableStore>)>;

/// `UserDataSyncService`.
pub struct UserDataSyncService {
    me: Weak<Self>,
    base: Rc<Disposable>,
    services: SynchroniserServices,
    user_data_sync_store_management_service: Rc<dyn IUserDataSyncStoreManagementService>,
    user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    user_data_sync_util_service: Rc<dyn IUserDataSyncUtilService>,
    state: RefCell<ServiceState>,
    on_did_change_status: Rc<Emitter<SyncStatus>>,
    on_did_change_local: Rc<Emitter<SyncResource>>,
    on_did_change_conflicts: Rc<Emitter<Vec<IUserDataSyncResourceConflicts>>>,
    on_sync_errors: Rc<Emitter<Vec<IUserDataSyncResourceError>>>,
    on_did_change_last_sync_time: Rc<Emitter<f64>>,
    on_did_reset_local: Rc<Emitter<()>>,
    on_did_reset_remote: Rc<Emitter<()>>,
    active_profile_synchronizers: RefCell<ActiveProfileSynchronizers>,
}

impl UserDataSyncService {
    pub fn new(
        services: SynchroniserServices,
        user_data_sync_store_management_service: Rc<dyn IUserDataSyncStoreManagementService>,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        user_data_sync_util_service: Rc<dyn IUserDataSyncUtilService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_status = Emitter::new();
        base.register(on_did_change_status.clone());
        let on_did_change_local = Emitter::new();
        base.register(on_did_change_local.clone());
        let on_did_change_conflicts = Emitter::new();
        base.register(on_did_change_conflicts.clone());
        let on_sync_errors = Emitter::new();
        base.register(on_sync_errors.clone());
        let on_did_change_last_sync_time = Emitter::new();
        base.register(on_did_change_last_sync_time.clone());
        let on_did_reset_local = Emitter::new();
        base.register(on_did_reset_local.clone());
        let on_did_reset_remote = Emitter::new();
        base.register(on_did_reset_remote.clone());

        let status = if user_data_sync_store_management_service.user_data_sync_store().is_some() { SyncStatus::Idle } else { SyncStatus::Uninitialized };
        let last_sync_time = services.storage_service.get_number(LAST_SYNC_TIME_KEY, StorageScope::Application, None);
        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            services,
            user_data_sync_store_management_service,
            user_data_profiles_service,
            user_data_sync_util_service,
            state: RefCell::new(ServiceState { status, conflicts: Vec::new(), sync_errors: Vec::new(), last_sync_time }),
            on_did_change_status,
            on_did_change_local,
            on_did_change_conflicts,
            on_sync_errors,
            on_did_change_last_sync_time,
            on_did_reset_local,
            on_did_reset_remote,
            active_profile_synchronizers: RefCell::new(IndexMap::new()),
        });

        let weak = Rc::downgrade(&this);
        this.base.register(to_disposable({
            let weak = weak.clone();
            move || {
                if let Some(this) = weak.upgrade() {
                    this.clear_active_profile_synchronizers();
                }
            }
        }));

        let scheduler = RunOnceScheduler::new(
            move || {
                if let Some(this) = weak.upgrade() {
                    let _ = this.clean_up_stale_storage_data();
                }
            },
            5 * 1000, /* after 5s */
        );
        this.base.register(scheduler.clone());
        scheduler.schedule(None);
        this
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    pub fn status(&self) -> SyncStatus {
        self.state.borrow().status
    }

    pub fn on_did_change_status(&self) -> Event<SyncStatus> {
        self.on_did_change_status.event()
    }

    pub fn on_did_change_local(&self) -> Event<SyncResource> {
        self.on_did_change_local.event()
    }

    pub fn conflicts(&self) -> Vec<IUserDataSyncResourceConflicts> {
        self.state.borrow().conflicts.clone()
    }

    pub fn on_did_change_conflicts(&self) -> Event<Vec<IUserDataSyncResourceConflicts>> {
        self.on_did_change_conflicts.event()
    }

    pub fn on_sync_errors(&self) -> Event<Vec<IUserDataSyncResourceError>> {
        self.on_sync_errors.event()
    }

    pub fn last_sync_time(&self) -> Option<f64> {
        self.state.borrow().last_sync_time
    }

    pub fn on_did_change_last_sync_time(&self) -> Event<f64> {
        self.on_did_change_last_sync_time.event()
    }

    pub fn on_did_reset_local(&self) -> Event<()> {
        self.on_did_reset_local.event()
    }

    pub fn on_did_reset_remote(&self) -> Event<()> {
        self.on_did_reset_remote.event()
    }

    fn check_enablement(&self) -> Result<(), Error> {
        if self.user_data_sync_store_management_service.user_data_sync_store().is_none() {
            return Err(Error::new("Not enabled"));
        }
        Ok(())
    }

    /// `createSyncTask`.
    pub fn create_sync_task(&self, manifest: Option<IUserDataManifest>, disable_cache: Option<bool>) -> Promise<Rc<dyn IUserDataSyncTask>> {
        let this = self.rc();
        async_fn(async move {
            this.check_enablement()?;

            this.services.log_service.info("Sync started.");
            let start_time = date_now();
            let execution_id = generate_uuid();
            let mut sync_headers = create_sync_headers(&execution_id);
            if disable_cache == Some(true) {
                sync_headers.insert("Cache-Control".to_string(), je_base::parts::request::HeaderValue::One("no-cache".to_string()));
            }
            let manifest = match this.services.user_data_sync_store_service.manifest(manifest, Some(sync_headers)).await {
                Ok(manifest) => manifest,
                Err(error) => {
                    report_user_data_sync_error(&error, &execution_id, this.user_data_sync_store_management_service.as_ref(), this.services.telemetry_service.as_ref());
                    return Err(to_sync_error(&error));
                }
            };

            let task: Rc<dyn IUserDataSyncTask> = Rc::new(UserDataSyncTask { service: this.clone(), manifest, execution_id, start_time, cancellable: Rc::default() });
            Ok(task)
        })
    }

    fn sync(&self, manifest_or_latest_data: Option<ManifestOrLatestData>, preview: bool, execution_id: String, token: CancellationToken) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            this.state.borrow_mut().sync_errors.clear();
            let result = this.sync_profiles(manifest_or_latest_data, preview, &execution_id, token).await;
            if this.status() != SyncStatus::HasConflicts {
                this.set_status(SyncStatus::Idle);
            }
            let sync_errors = this.state.borrow().sync_errors.clone();
            this.on_sync_errors.fire(sync_errors);
            result
        })
    }

    async fn sync_profiles(self: &Rc<Self>, manifest_or_latest_data: Option<ManifestOrLatestData>, preview: bool, execution_id: &str, token: CancellationToken) -> Result<(), Error> {
        if self.status() != SyncStatus::HasConflicts {
            self.set_status(SyncStatus::Syncing);
        }

        // Sync Default Profile First
        let default_profile_synchronizer = self.get_or_create_active_profile_synchronizer(&self.user_data_profiles_service.default_profile(), None);
        let sync_errors = self.sync_profile(&default_profile_synchronizer, manifest_or_latest_data.clone(), preview, execution_id, token.clone()).await?;
        self.state.borrow_mut().sync_errors.extend(sync_errors);

        // Sync other profiles
        let user_data_profile_manifest_synchronizer = default_profile_synchronizer
            .enabled()
            .into_iter()
            .find(|synchroniser| synchroniser.resource() == SyncResource::Profiles)
            .and_then(Synchroniser::into_profiles_manifest);
        if let Some(user_data_profile_manifest_synchronizer) = user_data_profile_manifest_synchronizer {
            let sync_profiles = user_data_profile_manifest_synchronizer.get_last_synced_profiles().await?.unwrap_or_default();
            if token.is_cancellation_requested() {
                return Ok(());
            }
            self.sync_remote_profiles(sync_profiles, manifest_or_latest_data, preview, execution_id, token).await?;
        }
        Ok(())
    }

    async fn sync_remote_profiles(
        self: &Rc<Self>,
        remote_profiles: Vec<ISyncUserDataProfile>,
        manifest: Option<ManifestOrLatestData>,
        preview: bool,
        execution_id: &str,
        token: CancellationToken,
    ) -> Result<(), Error> {
        for sync_profile in remote_profiles {
            if token.is_cancellation_requested() {
                return Ok(());
            }
            let profile = self.user_data_profiles_service.profiles().into_iter().find(|profile| profile.id == sync_profile.id);
            let Some(profile) = profile else {
                self.services.log_service.error(&format!("Profile with id:{} and name: {} does not exist locally to sync.", sync_profile.id, sync_profile.name));
                continue;
            };
            self.services.log_service.info_args("Syncing profile.", &[LogArg::from(sync_profile.name.as_str())]);
            let profile_synchronizer = self.get_or_create_active_profile_synchronizer(&profile, Some(&sync_profile));
            let sync_errors = self.sync_profile(&profile_synchronizer, manifest.clone(), preview, execution_id, token.clone()).await?;
            self.state.borrow_mut().sync_errors.extend(sync_errors);
        }

        // Dispose & Delete profile synchronizers which do not exist anymore
        let entries: Vec<(String, Rc<ProfileSynchronizer>, Rc<DisposableStore>)> =
            self.active_profile_synchronizers.borrow().iter().map(|(key, (synchronizer, disposables))| (key.clone(), synchronizer.clone(), disposables.clone())).collect();
        for (key, profile_synchronizer, disposables) in entries {
            if self.user_data_profiles_service.profiles().iter().any(|profile| profile.id == profile_synchronizer.profile.id) {
                continue;
            }
            profile_synchronizer.reset_local().await?;
            disposables.dispose();
            self.active_profile_synchronizers.borrow_mut().shift_remove(&key);
        }
        Ok(())
    }

    async fn sync_profile(
        self: &Rc<Self>,
        profile_synchronizer: &Rc<ProfileSynchronizer>,
        manifest_or_latest_data: Option<ManifestOrLatestData>,
        preview: bool,
        execution_id: &str,
        token: CancellationToken,
    ) -> Result<Vec<IUserDataSyncResourceError>, Error> {
        let errors = profile_synchronizer.sync(manifest_or_latest_data, preview, execution_id.to_string(), token).await?;
        Ok(errors.into_iter().map(|(sync_resource, error)| IUserDataSyncResourceError { profile: profile_synchronizer.profile.clone(), sync_resource, error }).collect())
    }

    /// `stop`.
    fn stop(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            if this.status() != SyncStatus::Idle {
                let stops: Vec<Promise<()>> = this.get_active_profile_synchronizers().iter().map(|profile_synchronizer| profile_synchronizer.stop()).collect();
                let _ = Promise::all_settled(stops).await;
            }
            Ok(())
        })
    }

    /// `accept`.
    pub fn accept(&self, sync_resource: &IUserDataSyncResource, resource: &URI, content: Option<Option<String>>, apply: ApplyMode) -> Promise<()> {
        if let Err(error) = self.check_enablement() {
            return Promise::rejected(error);
        }

        let (sync_resource, resource) = (sync_resource.clone(), resource.clone());
        let performed = self.perform_action(sync_resource.profile.clone(), move |synchronizer| {
            if sync_resource.sync_resource != synchronizer.resource() {
                return Promise::resolved(None);
            }
            let (resource, content) = (resource.clone(), content.clone());
            async_fn(async move {
                synchronizer.accept(&resource, content).await?;
                let (should_apply, force) = match apply {
                    ApplyMode::Apply(apply) => (apply, false),
                    ApplyMode::Force { force } => (true, force),
                };
                if should_apply {
                    synchronizer.apply(force, create_sync_headers(&generate_uuid())).await?;
                }
                Ok(Some(true))
            })
        });
        async_fn(async move {
            performed.await?;
            Ok(())
        })
    }

    /// `hasLocalData`: ignora o estado global.
    pub fn has_local_data(&self) -> Promise<bool> {
        let result = self.perform_action(self.user_data_profiles_service.default_profile(), |synchronizer| {
            async_fn(async move {
                // skip global state synchronizer
                if synchronizer.resource() != SyncResource::GlobalState && synchronizer.has_local_data().await? {
                    return Ok(Some(true));
                }
                Ok(None)
            })
        });
        async_fn(async move { Ok(result.await?.is_some()) })
    }

    /// `hasPreviouslySynced`.
    pub fn has_previously_synced(&self) -> Promise<bool> {
        let result = self.perform_action(self.user_data_profiles_service.default_profile(), |synchronizer| {
            async_fn(async move {
                if synchronizer.has_previously_synced().await? {
                    return Ok(Some(true));
                }
                Ok(None)
            })
        });
        async_fn(async move { Ok(result.await?.is_some()) })
    }

    /// `reset`.
    pub fn reset(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            this.check_enablement()?;
            this.reset_remote().await?;
            this.reset_local().await
        })
    }

    /// `resetRemote`.
    pub fn reset_remote(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            this.check_enablement()?;
            match this.services.user_data_sync_store_service.clear().await {
                Ok(()) => this.services.log_service.info("Cleared data on server"),
                Err(error) => this.services.log_service.error_args(&LogMessage::from(&error), &[]),
            }
            this.on_did_reset_remote.fire(());
            Ok(())
        })
    }

    /// `resetLocal`.
    pub fn reset_local(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            this.check_enablement()?;
            this.state.borrow_mut().last_sync_time = None;
            this.services.storage_service.remove(LAST_SYNC_TIME_KEY, StorageScope::Application);
            for synchronizer in this.get_active_profile_synchronizers() {
                if let Err(error) = synchronizer.reset_local().await {
                    this.services.log_service.error_args(&LogMessage::from(&error), &[]);
                }
            }
            this.clear_active_profile_synchronizers();
            this.on_did_reset_local.fire(());
            this.services.log_service.info("Did reset the local sync state.");
            Ok(())
        })
    }

    /// `cleanUpStaleStorageData`: o estado da última sincronização dos perfis que já não existem no servidor.
    fn clean_up_stale_storage_data(&self) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let all_keys = this.services.storage_service.keys(StorageScope::Application, StorageTarget::Machine);
            let mut last_sync_profile_keys: Vec<(String, String)> = Vec::new();
            for key in all_keys {
                if !key.ends_with(".lastSyncUserData") {
                    continue;
                }
                let segments: Vec<&str> = key.split('.').collect();
                if segments.len() == 3 {
                    let collection = segments[0].to_string();
                    last_sync_profile_keys.push((key.clone(), collection));
                }
            }
            if last_sync_profile_keys.is_empty() {
                return Ok(());
            }

            let disposables = DisposableStore::new();
            let result = this.clean_up_stale_keys(last_sync_profile_keys, &disposables).await;
            disposables.dispose();
            result
        })
    }

    async fn clean_up_stale_keys(self: &Rc<Self>, last_sync_profile_keys: Vec<(String, String)>, disposables: &DisposableStore) -> Result<(), Error> {
        let default_profile = self.user_data_profiles_service.default_profile();
        let active = self.active_profile_synchronizers.borrow().get(&default_profile.id).map(|(synchronizer, _)| synchronizer.clone());
        let default_profile_synchronizer = match active {
            Some(synchronizer) => synchronizer,
            None => disposables.add_rc(self.create_profile_synchronizer(default_profile, None)),
        };
        let Some(user_data_profile_manifest_synchronizer) = default_profile_synchronizer
            .enabled()
            .into_iter()
            .find(|synchroniser| synchroniser.resource() == SyncResource::Profiles)
            .and_then(Synchroniser::into_profiles_manifest)
        else {
            return Ok(());
        };
        let last_synced_profiles = user_data_profile_manifest_synchronizer.get_last_synced_profiles().await?;
        let last_synced_collections: Vec<String> = last_synced_profiles.unwrap_or_default().into_iter().map(|profile| profile.collection).collect();
        for (key, collection) in last_sync_profile_keys {
            if !last_synced_collections.contains(&collection) {
                self.services.log_service.info(&format!("Removing last sync state for stale profile: {collection}"));
                self.services.storage_service.remove(&key, StorageScope::Application);
            }
        }
        Ok(())
    }

    /// `performAction`: roda a ação em cada sincronizador do perfil até uma devolver um valor.
    fn perform_action<T: Clone + 'static>(&self, profile: UserDataProfile, action: impl Fn(Rc<dyn IUserDataSynchroniser>) -> Promise<Option<T>> + 'static) -> Promise<Option<T>> {
        let this = self.rc();
        async_fn(async move {
            let disposables = DisposableStore::new();
            let result = this.perform_action_in(&profile, &action, &disposables).await;
            disposables.dispose();
            result
        })
    }

    async fn perform_action_in<T: Clone + 'static>(
        self: &Rc<Self>,
        profile: &UserDataProfile,
        action: &dyn Fn(Rc<dyn IUserDataSynchroniser>) -> Promise<Option<T>>,
        disposables: &DisposableStore,
    ) -> Result<Option<T>, Error> {
        let active = self.active_profile_synchronizers.borrow().get(&profile.id).map(|(synchronizer, _)| synchronizer.clone());
        if let Some(active) = active {
            return Self::perform_action_with_profile_synchronizer(&active, action, disposables).await;
        }

        if profile.is_default {
            let default_profile_synchronizer = disposables.add_rc(self.create_profile_synchronizer(profile.clone(), None));
            return Self::perform_action_with_profile_synchronizer(&default_profile_synchronizer, action, disposables).await;
        }

        let user_data_profile_manifest_synchronizer =
            disposables.add_rc(UserDataProfilesManifestSynchroniser::new(profile.clone(), None, self.user_data_profiles_service.clone(), self.services.clone()));
        let manifest = self.services.user_data_sync_store_service.manifest(None, None).await?;
        let latest_profiles_ref = manifest.and_then(|manifest| manifest.latest).and_then(|latest| latest.get(SyncResource::Profiles.as_str()).cloned());
        let sync_profiles = user_data_profile_manifest_synchronizer.get_remote_synced_profiles(latest_profiles_ref.map(RefOrUserData::Ref)).await?.unwrap_or_default();
        if let Some(sync_profile) = sync_profiles.into_iter().find(|sync_profile| sync_profile.id == profile.id) {
            let profile_synchronizer = disposables.add_rc(self.create_profile_synchronizer(profile.clone(), Some(sync_profile.collection)));
            return Self::perform_action_with_profile_synchronizer(&profile_synchronizer, action, disposables).await;
        }

        Ok(None)
    }

    async fn perform_action_with_profile_synchronizer<T: Clone + 'static>(
        profile_synchronizer: &Rc<ProfileSynchronizer>,
        action: &dyn Fn(Rc<dyn IUserDataSynchroniser>) -> Promise<Option<T>>,
        disposables: &DisposableStore,
    ) -> Result<Option<T>, Error> {
        let mut all_synchronizers: Vec<Rc<dyn IUserDataSynchroniser>> = profile_synchronizer.enabled().iter().map(Synchroniser::as_dyn).collect();
        for sync_resource in profile_synchronizer.disabled() {
            if sync_resource != SyncResource::WorkspaceState
                && let Some(synchronizer) = profile_synchronizer.create_synchronizer(sync_resource)
            {
                synchronizer.add_to(disposables);
                all_synchronizers.push(synchronizer.as_dyn());
            }
        }
        for synchronizer in all_synchronizers {
            let result = action(synchronizer).await?;
            if result.is_some() {
                return Ok(result);
            }
        }
        Ok(None)
    }

    fn set_status(&self, status: SyncStatus) {
        let old_status = self.status();
        if old_status != status {
            self.state.borrow_mut().status = status;
            self.on_did_change_status.fire(status);
            if old_status == SyncStatus::HasConflicts {
                self.update_last_sync_time();
            }
        }
    }

    fn update_conflicts(&self) {
        let conflicts: Vec<IUserDataSyncResourceConflicts> = self.get_active_profile_synchronizers().iter().flat_map(|synchronizer| synchronizer.conflicts()).collect();
        let ext_uri = &self.services.ext_uri;
        let changed = !equals(Some(self.state.borrow().conflicts.as_slice()), Some(conflicts.as_slice()), |a, b| {
            a.profile.id == b.profile.id
                && a.sync_resource == b.sync_resource
                && equals(Some(a.conflicts.as_slice()), Some(b.conflicts.as_slice()), |a, b| ext_uri.is_equal(Some(&a.preview_resource), Some(&b.preview_resource), false))
        });
        if changed {
            self.state.borrow_mut().conflicts = conflicts.clone();
            self.on_did_change_conflicts.fire(conflicts);
        }
    }

    fn update_last_sync_time(&self) {
        if self.status() == SyncStatus::Idle {
            let last_sync_time = date_now() as f64;
            self.state.borrow_mut().last_sync_time = Some(last_sync_time);
            self.services.storage_service.store(LAST_SYNC_TIME_KEY, StorageValue::from(last_sync_time), StorageScope::Application, StorageTarget::Machine);
            self.on_did_change_last_sync_time.fire(last_sync_time);
        }
    }

    fn create_profile_synchronizer(&self, profile: UserDataProfile, collection: Option<String>) -> Rc<ProfileSynchronizer> {
        ProfileSynchronizer::new(
            profile,
            collection,
            self.services.clone(),
            self.user_data_profiles_service.clone(),
            self.user_data_sync_store_management_service.clone(),
            self.user_data_sync_util_service.clone(),
        )
    }

    /// `getOrCreateActiveProfileSynchronizer`.
    pub fn get_or_create_active_profile_synchronizer(&self, profile: &UserDataProfile, sync_profile: Option<&ISyncUserDataProfile>) -> Rc<ProfileSynchronizer> {
        let collection = sync_profile.map(|sync_profile| sync_profile.collection.clone());
        let mut active_profile_synchronizer = self.active_profile_synchronizers.borrow().get(&profile.id).cloned();
        if let Some((synchronizer, disposables)) = &active_profile_synchronizer
            && synchronizer.collection != collection
        {
            self.services.log_service.error("Profile synchronizer collection does not match with the remote sync profile collection");
            disposables.dispose();
            active_profile_synchronizer = None;
            self.active_profile_synchronizers.borrow_mut().shift_remove(&profile.id);
        }
        if let Some((synchronizer, _)) = active_profile_synchronizer {
            return synchronizer;
        }

        let disposables = DisposableStore::new();
        let profile_synchronizer = disposables.add_rc(self.create_profile_synchronizer(profile.clone(), collection));
        let weak = self.me.clone();
        disposables.add(profile_synchronizer.on_did_change_status().listen({
            let weak = weak.clone();
            move |status| {
                if let Some(this) = weak.upgrade() {
                    this.set_status(*status);
                }
            }
        }));
        disposables.add(profile_synchronizer.on_did_change_conflicts().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_conflicts();
                }
            }
        }));
        disposables.add(profile_synchronizer.on_did_change_local().listen(move |resource| {
            if let Some(this) = weak.upgrade() {
                this.on_did_change_local.fire(*resource);
            }
        }));
        self.active_profile_synchronizers.borrow_mut().insert(profile.id.clone(), (profile_synchronizer.clone(), disposables));
        profile_synchronizer
    }

    fn get_active_profile_synchronizers(&self) -> Vec<Rc<ProfileSynchronizer>> {
        self.active_profile_synchronizers.borrow().values().map(|(profile_synchronizer, _)| profile_synchronizer.clone()).collect()
    }

    fn clear_active_profile_synchronizers(&self) {
        let active: Vec<(Rc<ProfileSynchronizer>, Rc<DisposableStore>)> = self.active_profile_synchronizers.borrow_mut().drain(..).map(|(_, entry)| entry).collect();
        for (_, disposables) in active {
            disposables.dispose();
        }
    }
}

impl IDisposable for UserDataSyncService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// A tarefa que `createSyncTask` devolve.
struct UserDataSyncTask {
    service: Rc<UserDataSyncService>,
    manifest: Option<IUserDataManifest>,
    execution_id: String,
    start_time: u64,
    cancellable: Rc<RefCell<Option<CancelablePromise<()>>>>,
}

impl IUserDataSyncTask for UserDataSyncTask {
    fn manifest(&self) -> Option<IUserDataManifest> {
        self.manifest.clone()
    }

    /// O `if (executed) throw new Error('Can run a task only once')` do original nunca dispara (`executed` é
    /// uma constante falsa), então não existe aqui.
    fn run(&self) -> Promise<()> {
        let service = self.service.clone();
        let (manifest, execution_id) = (self.manifest.clone().map(ManifestOrLatestData::Manifest), self.execution_id.clone());
        let sync_service = service.clone();
        let cancellable = create_cancelable_promise(move |token| sync_service.sync(manifest, false, execution_id, token));
        *self.cancellable.borrow_mut() = Some(cancellable.clone());
        let (slot, start_time) = (self.cancellable.clone(), self.start_time);
        async_fn(async move {
            let result = cancellable.await;
            slot.borrow_mut().take();
            result?;
            service.services.log_service.info(&format!("Sync done. Took {}ms", date_now() - start_time));
            service.update_last_sync_time();
            Ok(())
        })
    }

    fn stop(&self) -> Promise<()> {
        let cancellable = self.cancellable.borrow().clone();
        if let Some(cancellable) = cancellable {
            cancellable.cancel();
        }
        self.service.stop()
    }
}

// endregion
