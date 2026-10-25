//! Porte de `src/vs/platform/storage/electron-main/storageMainService.ts`: o `StorageMainService`, que deixa os
//! armazéns de aplicação, aplicação compartilhada, perfil e workspace acessíveis às janelas a partir do processo
//! principal, e o `ApplicationStorageMainService`, que usa o armazém da aplicação de dentro do processo principal.
//!
//! Não há processo principal do Electron no je-code, mas o papel dele existe no processo único; por isso o serviço é o
//! mesmo, e quem precisa dos armazéns (o [`SqliteStorageService`](super::SqliteStorageService), o canal de IPC) o chama
//! direto.
//!
//! Decisões de tradução:
//! - **`IStorageMainService` e `StorageMainService`**: a interface é o trait [`IStorageMainService`] (o `createDecorator`
//!   é [`I_STORAGE_MAIN_SERVICE`]) e a classe é a struct [`StorageMainService`]. As propriedades (`applicationStorage`,
//!   `applicationSharedStorage`) são métodos.
//! - **`getStorageOptions()`** é `protected` e o teste o sobrescreve para usar só memória. O Rust não tem herança: o
//!   valor entra como parâmetro em [`StorageMainService::with_storage_options`], e [`StorageMainService::new`] o
//!   calcula como o original (`!!environmentService.extensionTestsLocationURI`).
//! - **`INativeEnvironmentService`, `IUserDataProfilesMainService`, `ILifecycleMainService`, `IFileService` e
//!   `IUriIdentityService`** entram como `Rc` dos contratos e dos serviços portados (o `ILifecycleMainService` é o
//!   contrato mínimo de `user_data_profiles_handler.rs`: `onWillLoadWindow`, `onWillShutdown` e `when`).
//! - **`Map<string, IStorageMain>`** é um `IndexMap` (a ordem de inserção é a do `Map`).
//! - **`isProfileUsingDefaultStorage(profile)`** é o de `storage.rs` sobre o [`StorageProfile`] do perfil.
//! - **`ApplicationStorageMainService`**: o `AbstractStorageService` com um [`StorageBackend`] que só enxerga os escopos
//!   de aplicação; `hasScope`, `switchToProfile` e `switchToWorkspace` lançam, como no original. O estreitamento do
//!   escopo no tipo (`ApplicationStorageScope`) não existe: qualquer outro escopo não tem armazém (`getStorage` devolve
//!   `None`). O `switch()` sobrescrito é [`ApplicationStorageMainService::switch`].
//! - **`onWillShutdown`** fecha os armazéns que ainda estão no mapa do serviço (aplicação, compartilhado, perfis e
//!   workspaces), como o original. O cliente do processo único ([`SqliteStorageService`](super::SqliteStorageService))
//!   fecha por conta própria só o armazém do workspace; o `onDidCloseStorage` o tira do mapa, então ele não é fechado de
//!   novo aqui. O `onDidChangeStorage` e o `onDidChangeProfileStorage` carregam só a chave (`IStorageChangeEvent` de
//!   `storageMain.ts`); o `external` do cliente vem de [`IStorageMain::on_did_change_client_storage`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::async_fn;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_storage::IStorage;

use super::storage::{AbstractStorageService, StorageBackend, StorageProfile, StorageScope, StorageServiceCoreOptions, StorageSwitchTarget, StorageWorkspace, is_profile_using_default_storage};
use super::storage_main::{
    ApplicationSharedStorageMain, ApplicationStorageMain, IStorageMain, InMemoryStorageMain, ProfileStorageMain, StorageMainOptions, WorkspaceStorageMain, file_fs_path, join,
};
use crate::environment::environment::{EnvironmentService, NativeEnvironmentServiceApi};
use crate::files::i_file_service::IFileService;
use crate::log::{LogServiceRef, Logger};
use crate::resources::uri_file;
use crate::uri_identity::uri_identity::IUriIdentityService;
use crate::user_data_profile::user_data_profile_storage_service::storage_profile;
use crate::user_data_profile::user_data_profiles_handler::{LifecycleMainPhase, LifecycleMainService, ShutdownReason};
use crate::user_data_profile::{IUserDataProfilesService, UserDataProfile, UserDataProfilesMainService};
use crate::workspace::AnyWorkspaceIdentifier;

//#region Storage Main Service (intent: make application, profile and workspace storage accessible to windows from main process)

/// `IProfileStorageChangeEvent`: a mudança de uma chave no armazém de um perfil.
#[derive(Clone)]
pub struct ProfileStorageChangeEvent {
    /// `key` (do `IStorageChangeEvent`): a chave do item que mudou ou foi removido.
    pub key: String,
    pub storage: Rc<dyn IStorageMain>,
    pub profile: UserDataProfile,
}

/// `IStorageMainService`.
pub trait IStorageMainService {
    /// `applicationStorage`: o armazém da aplicação, compartilhado entre todas as janelas e todos os perfis.
    ///
    /// Note: DO NOT use this for reading/writing from the main process! Rather use [`ApplicationStorageMainService`]
    /// for that purpose.
    fn application_storage(&self) -> Rc<dyn IStorageMain>;

    /// `applicationSharedStorage`: o armazém compartilhado entre o VS Code e o app de agentes.
    fn application_shared_storage(&self) -> Rc<dyn IStorageMain>;

    /// `onDidChangeProfileStorage`: emitido quando um dado do armazém de escopo de perfil é atualizado ou removido.
    fn on_did_change_profile_storage(&self) -> Event<ProfileStorageChangeEvent>;

    /// `profileStorage(profile)`: o armazém do perfil, compartilhado entre todas as janelas.
    ///
    /// Note: DO NOT use this for reading/writing from the main process! This is currently not supported.
    fn profile_storage(&self, profile: &UserDataProfile) -> Rc<dyn IStorageMain>;

    /// `workspaceStorage(workspace)`: o armazém do workspace, específico de uma janela.
    ///
    /// Note: DO NOT use this for reading/writing from the main process! This is currently not supported.
    fn workspace_storage(&self, workspace: &AnyWorkspaceIdentifier) -> Rc<dyn IStorageMain>;

    /// `isUsed(path)`: se o caminho (do arquivo ou da pasta que o contém) está em uso por algum banco.
    fn is_used(&self, path: &str) -> bool;
}

service_id!(pub I_STORAGE_MAIN_SERVICE: dyn IStorageMainService = "storageMainService");

/// `StorageMainService`.
pub struct StorageMainService {
    me: Weak<StorageMainService>,
    base: Rc<Disposable>,
    log_service: LogServiceRef,
    file_service: Rc<dyn IFileService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    environment_service: Rc<dyn NativeEnvironmentServiceApi>,
    storage_options: StorageMainOptions,
    shutdown_reason: Cell<Option<ShutdownReason>>,
    on_did_change_profile_storage: Rc<Emitter<ProfileStorageChangeEvent>>,
    application_storage: Rc<dyn IStorageMain>,
    application_shared_storage: Rc<dyn IStorageMain>,
    map_profile_to_storage: RefCell<IndexMap<String /* profile ID */, Rc<dyn IStorageMain>>>,
    map_workspace_to_storage: RefCell<IndexMap<String /* workspace ID */, Rc<dyn IStorageMain>>>,
}

impl StorageMainService {
    /// O construtor do original: as opções são as de `getStorageOptions()`, que usa só memória durante os testes de
    /// extensão (`no storage during extension tests!`).
    pub fn new(
        log_service: LogServiceRef,
        environment_service: Rc<dyn NativeEnvironmentServiceApi>,
        user_data_profiles_service: Rc<UserDataProfilesMainService>,
        lifecycle_main_service: Rc<dyn LifecycleMainService>,
        file_service: Rc<dyn IFileService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
    ) -> Rc<Self> {
        let storage_options = StorageMainOptions { use_in_memory_storage: environment_service.extension_tests_location_uri().is_some() };
        Self::with_storage_options(storage_options, log_service, environment_service, user_data_profiles_service, lifecycle_main_service, file_service, uri_identity_service)
    }

    /// O construtor com o `getStorageOptions()` sobrescrito (o `TestStorageMainService` do teste).
    pub fn with_storage_options(
        storage_options: StorageMainOptions,
        log_service: LogServiceRef,
        environment_service: Rc<dyn NativeEnvironmentServiceApi>,
        user_data_profiles_service: Rc<UserDataProfilesMainService>,
        lifecycle_main_service: Rc<dyn LifecycleMainService>,
        file_service: Rc<dyn IFileService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_profile_storage = Emitter::new();
        base.register(on_did_change_profile_storage.clone());

        let application_storage = Self::create_application_storage(&base, storage_options, &log_service, &user_data_profiles_service, &file_service);
        let registered: DisposableRef = application_storage.clone();
        base.register(registered);

        let application_shared_storage = Self::create_application_shared_storage(&base, storage_options, &log_service, environment_service.as_ref(), &application_storage, &file_service);
        let registered: DisposableRef = application_shared_storage.clone();
        base.register(registered);

        let this = Rc::new_cyclic(|me| Self {
            me: me.clone(),
            base,
            log_service,
            file_service,
            uri_identity_service,
            environment_service,
            storage_options,
            shutdown_reason: Cell::new(None),
            on_did_change_profile_storage,
            application_storage,
            application_shared_storage,
            map_profile_to_storage: RefCell::new(IndexMap::new()),
            map_workspace_to_storage: RefCell::new(IndexMap::new()),
        });

        this.register_listeners(lifecycle_main_service.as_ref(), &user_data_profiles_service);
        this
    }

    fn register_listeners(self: &Rc<Self>, lifecycle_main_service: &dyn LifecycleMainService, user_data_profiles_service: &UserDataProfilesMainService) {
        // Application Storage: Warmup when any window opens
        let when_after_window_open = lifecycle_main_service.when(LifecycleMainPhase::AfterWindowOpen);
        let this = self.clone();
        drop(async_fn(async move {
            when_after_window_open.await?;

            let _ = this.application_storage.init();
            let _ = this.application_shared_storage.init();
            Ok(())
        }));

        let me = self.me.clone();
        self.base.register(lifecycle_main_service.on_will_load_window().listen(move |e| {
            let Some(this) = me.upgrade() else {
                return;
            };

            // Profile Storage: Warmup when related window with profile loads
            if let Some(profile) = e.window.profile() {
                let _ = this.profile_storage(&profile).init();
            }

            // Workspace Storage: Warmup when related window with workspace loads
            if let Some(workspace) = &e.workspace {
                let _ = this.workspace_storage(workspace).init();
            }
        }));

        // All Storage: Close when shutting down
        let me = self.me.clone();
        self.base.register(lifecycle_main_service.on_will_shutdown().listen(move |e| {
            let Some(this) = me.upgrade() else {
                return;
            };
            this.log_service.trace("storageMainService#onWillShutdown()");

            // Remember shutdown reason
            this.shutdown_reason.set(Some(e.reason));

            // Application Storage
            e.join("applicationStorage", this.application_storage.close());

            // Application Shared Storage
            e.join("applicationSharedStorage", this.application_shared_storage.close());

            // Profile Storage(s)
            let profile_storages: Vec<_> = this.map_profile_to_storage.borrow().values().cloned().collect();
            for profile_storage in profile_storages {
                e.join("profileStorage", profile_storage.close());
            }

            // Workspace Storage(s)
            let workspace_storages: Vec<_> = this.map_workspace_to_storage.borrow().values().cloned().collect();
            for workspace_storage in workspace_storages {
                e.join("workspaceStorage", workspace_storage.close());
            }
        }));

        // Prepare storage location as needed
        let file_service = self.file_service.clone();
        self.base.register(user_data_profiles_service.on_will_create_profile().listen(move |e| {
            let (file_service, global_storage_home) = (file_service.clone(), e.profile.global_storage_home.clone());
            e.join(async_fn(async move {
                if !file_service.exists(&global_storage_home).await? {
                    file_service.create_folder(&global_storage_home).await?;
                }
                Ok(())
            }));
        }));

        // Close the storage of the profile that is being removed
        let me = self.me.clone();
        self.base.register(user_data_profiles_service.on_will_remove_profile().listen(move |e| {
            let storage = me.upgrade().and_then(|this| this.map_profile_to_storage.borrow().get(&e.profile.id).cloned());
            if let Some(storage) = storage {
                e.join(storage.close());
            }
        }));
    }

    //#region Application Storage

    fn create_application_storage(
        base: &Disposable,
        storage_options: StorageMainOptions,
        log_service: &LogServiceRef,
        user_data_profiles_service: &UserDataProfilesMainService,
        file_service: &Rc<dyn IFileService>,
    ) -> Rc<dyn IStorageMain> {
        log_service.trace("StorageMainService: creating application storage");

        let application_storage = ApplicationStorageMain::new(storage_options, user_data_profiles_service, log_service.clone(), file_service.clone());

        let log_service = log_service.clone();
        base.register(application_storage.on_did_close_storage().once().listen(move |_| {
            log_service.trace("StorageMainService: closed application storage");
        }));

        application_storage
    }

    //#endregion

    //#region Application Shared Storage

    fn create_application_shared_storage(
        base: &Disposable,
        storage_options: StorageMainOptions,
        log_service: &LogServiceRef,
        environment_service: &dyn EnvironmentService,
        application_storage: &Rc<dyn IStorageMain>,
        file_service: &Rc<dyn IFileService>,
    ) -> Rc<dyn IStorageMain> {
        log_service.info("StorageMainService: creating application shared storage");

        let shared_storage_folder_path = join(&[&file_fs_path(&environment_service.app_shared_data_home()), "sharedStorage"]);

        // Use the local application storage as fallback for transparent migration of keys from APPLICATION to
        // APPLICATION_SHARED scope. The agents window is now part of the same VS Code app, so there is no separate
        // "host" app DB to fall back to.
        let application_shared_storage =
            ApplicationSharedStorageMain::new(storage_options, shared_storage_folder_path, application_storage.clone(), log_service.clone(), file_service.clone());

        let log_service = log_service.clone();
        base.register(application_shared_storage.on_did_close_storage().once().listen(move |_| {
            log_service.trace("StorageMainService: closed application shared storage");
        }));

        application_shared_storage
    }

    //#endregion

    //#region Profile Storage

    fn create_profile_storage(&self, profile: &UserDataProfile) -> Rc<dyn IStorageMain> {
        if self.shutdown_reason.get() == Some(ShutdownReason::Kill) {
            // Workaround for native crashes that we see when SQLite DBs are being created even after shutdown
            // https://github.com/microsoft/vscode/issues/143186
            return InMemoryStorageMain::new(self.log_service.clone(), self.file_service.clone());
        }

        ProfileStorageMain::new(profile, self.storage_options, self.log_service.clone(), self.file_service.clone())
    }

    //#endregion

    //#region Workspace Storage

    fn create_workspace_storage(&self, workspace: &AnyWorkspaceIdentifier) -> Rc<dyn IStorageMain> {
        if self.shutdown_reason.get() == Some(ShutdownReason::Kill) {
            // Workaround for native crashes that we see when SQLite DBs are being created even after shutdown
            // https://github.com/microsoft/vscode/issues/143186
            return InMemoryStorageMain::new(self.log_service.clone(), self.file_service.clone());
        }

        WorkspaceStorageMain::new(workspace.clone(), self.storage_options, self.log_service.clone(), self.environment_service.as_ref(), self.file_service.clone())
    }

    //#endregion
}

impl IStorageMainService for StorageMainService {
    fn application_storage(&self) -> Rc<dyn IStorageMain> {
        self.application_storage.clone()
    }

    fn application_shared_storage(&self) -> Rc<dyn IStorageMain> {
        self.application_shared_storage.clone()
    }

    fn on_did_change_profile_storage(&self) -> Event<ProfileStorageChangeEvent> {
        self.on_did_change_profile_storage.event()
    }

    fn profile_storage(&self, profile: &UserDataProfile) -> Rc<dyn IStorageMain> {
        if is_profile_using_default_storage(&storage_profile(profile)) {
            return self.application_storage.clone(); // for profiles using default storage, use application storage
        }

        let existing = self.map_profile_to_storage.borrow().get(&profile.id).cloned();
        if let Some(profile_storage) = existing {
            return profile_storage;
        }

        self.log_service.trace(&format!("StorageMainService: creating profile storage ({})", profile.name));

        let profile_storage = self.create_profile_storage(profile);
        let registered: DisposableRef = profile_storage.clone();
        self.base.register(registered);
        self.map_profile_to_storage.borrow_mut().insert(profile.id.clone(), profile_storage.clone());

        // Don't use this._register() for listeners that are disposed early as it causes entries to accumulate in
        // _store when storage is closed/reopened
        let (me, weak_storage, changed_profile) = (self.me.clone(), Rc::downgrade(&profile_storage), profile.clone());
        let listener = profile_storage.on_did_change_storage().listen(move |e| {
            if let (Some(this), Some(storage)) = (me.upgrade(), weak_storage.upgrade()) {
                this.on_did_change_profile_storage.fire(ProfileStorageChangeEvent { key: e.key.clone(), storage, profile: changed_profile.clone() });
            }
        });

        let (me, profile_id, profile_name) = (self.me.clone(), profile.id.clone(), profile.name.clone());
        let _ = profile_storage.on_did_close_storage().once().listen(move |_| {
            if let Some(this) = me.upgrade() {
                this.log_service.trace(&format!("StorageMainService: closed profile storage ({profile_name})"));

                this.map_profile_to_storage.borrow_mut().shift_remove(&profile_id);
            }
            listener.dispose();
        });

        profile_storage
    }

    fn workspace_storage(&self, workspace: &AnyWorkspaceIdentifier) -> Rc<dyn IStorageMain> {
        let existing = self.map_workspace_to_storage.borrow().get(workspace.id()).cloned();
        if let Some(workspace_storage) = existing {
            return workspace_storage;
        }

        self.log_service.trace(&format!("StorageMainService: creating workspace storage ({})", workspace.id()));

        let workspace_storage = self.create_workspace_storage(workspace);
        let registered: DisposableRef = workspace_storage.clone();
        self.base.register(registered);
        self.map_workspace_to_storage.borrow_mut().insert(workspace.id().to_string(), workspace_storage.clone());

        // Don't use this._register() for Event.once as it auto-disposes
        let (me, workspace_id) = (self.me.clone(), workspace.id().to_string());
        let _ = workspace_storage.on_did_close_storage().once().listen(move |_| {
            if let Some(this) = me.upgrade() {
                this.log_service.trace(&format!("StorageMainService: closed workspace storage ({workspace_id})"));

                this.map_workspace_to_storage.borrow_mut().shift_remove(&workspace_id);
            }
        });

        workspace_storage
    }

    fn is_used(&self, path: &str) -> bool {
        let path_uri = uri_file(path);
        let ext_uri = self.uri_identity_service.ext_uri();

        let mut storages = vec![self.application_storage.clone(), self.application_shared_storage.clone()];
        storages.extend(self.map_profile_to_storage.borrow().values().cloned());
        storages.extend(self.map_workspace_to_storage.borrow().values().cloned());

        storages.iter().filter_map(|storage| storage.path()).any(|storage_path| ext_uri.is_equal_or_parent(&uri_file(&storage_path), &path_uri, false, false))
    }
}

impl IDisposable for StorageMainService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

//#endregion

//#region Application Main Storage Service (intent: use application storage from main process)

/// `Promise.all([applicationStorage.whenInit, applicationSharedStorage.whenInit]).then(() => undefined)`.
fn when_application_storages_init(storage_main_service: &dyn IStorageMainService) -> Promise<()> {
    Promise::all(vec![storage_main_service.application_storage().when_init(), storage_main_service.application_shared_storage().when_init()]).then(|_| ())
}

/// Os métodos abstratos do `ApplicationStorageMainService`: só enxerga os escopos de aplicação.
struct ApplicationStorageMainBackend {
    user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    storage_main_service: Rc<dyn IStorageMainService>,
}

impl StorageBackend for ApplicationStorageMainBackend {
    fn has_scope(&self, _scope: &StorageSwitchTarget) -> bool {
        errors::throw(Error::new("Main process is never profile or workspace scoped"))
    }

    fn do_initialize(&self) -> Promise<()> {
        // application storage is being initialized as part of the first window opening, so we do not trigger it here but
        // can join it
        when_application_storages_init(self.storage_main_service.as_ref())
    }

    fn get_storage(&self, scope: StorageScope) -> Option<Rc<dyn IStorage>> {
        match scope {
            StorageScope::Application => Some(self.storage_main_service.application_storage().storage()),
            StorageScope::ApplicationShared => Some(self.storage_main_service.application_shared_storage().storage()),
            _ => None, // any other scope is unsupported from main process
        }
    }

    fn get_log_details(&self, scope: StorageScope) -> Option<String> {
        match scope {
            StorageScope::Application => Some(file_fs_path(&self.user_data_profiles_service.default_profile().global_storage_home)),
            StorageScope::ApplicationShared => self.storage_main_service.application_shared_storage().path(),
            _ => None, // any other scope is unsupported from main process
        }
    }

    fn switch_to_profile(&self, _service: &Rc<AbstractStorageService>, _to_profile: &StorageProfile, _preserve_data: bool) -> Promise<()> {
        errors::throw(Error::new("Switching storage profile is unsupported from main process"))
    }

    fn switch_to_workspace(&self, _service: &Rc<AbstractStorageService>, _to_workspace: &StorageWorkspace, _preserve_data: bool) -> Promise<()> {
        errors::throw(Error::new("Switching storage workspace is unsupported from main process"))
    }

    fn should_flush_when_idle(&self) -> bool {
        false // not needed here, will be triggered from any window that is opened
    }
}

/// `ApplicationStorageMainService` (`IApplicationStorageMainService`): um serviço de armazenamento (o
/// [`AbstractStorageService`], de [`service`](Self::service)) restrito aos escopos de aplicação.
///
/// Important: unlike other storage services in the renderer, the main process does not await the storage to be ready,
/// rather storage is being initialized while a window opens to reduce pressure on startup. As such, any client wanting to
/// access application storage from the main process needs to wait for [`when_ready`](Self::when_ready), otherwise there is
/// a chance that the service operates on an in-memory store that is not backed by any persistent DB.
pub struct ApplicationStorageMainService {
    service: Rc<AbstractStorageService>,
    when_ready: Promise<()>,
}

impl ApplicationStorageMainService {
    pub fn new(user_data_profiles_service: Rc<dyn IUserDataProfilesService>, storage_main_service: Rc<dyn IStorageMainService>) -> Self {
        let when_ready = when_application_storages_init(storage_main_service.as_ref());
        let service = AbstractStorageService::new(Rc::new(ApplicationStorageMainBackend { user_data_profiles_service, storage_main_service }), StorageServiceCoreOptions::default());
        Self::from_service(service, when_ready)
    }

    /// O `IApplicationStorageMainService` sobre um serviço de armazenamento já montado e o `whenReady` dele: para quem
    /// não tem os serviços principais (`StorageMainService` e perfis) por trás, como os testes que usam o
    /// `InMemoryStorageService`.
    pub fn from_service(service: Rc<AbstractStorageService>, when_ready: Promise<()>) -> Self {
        Self { service, when_ready }
    }

    /// O serviço de armazenamento (`IStorageService`).
    pub fn service(&self) -> Rc<AbstractStorageService> {
        self.service.clone()
    }

    /// `whenReady`.
    pub fn when_ready(&self) -> Promise<()> {
        self.when_ready.clone()
    }

    /// `switch()`: sobrescrito para lançar.
    pub fn switch(&self) -> ! {
        errors::throw(Error::new("Migrating storage is unsupported from main process"))
    }
}

impl IDisposable for ApplicationStorageMainService {
    fn dispose(&self) {
        self.service.dispose();
    }
}

//#endregion
