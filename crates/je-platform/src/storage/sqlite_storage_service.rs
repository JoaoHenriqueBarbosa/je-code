//! O serviço de armazenamento sobre o `je-storage`: abre direto os bancos `state.vscdb` do VS Code
//! (aplicação, aplicação compartilhada, perfil e workspace). Junta o que, no VS Code, é o `RemoteStorageService`
//! (`storageService.ts`) com o dono das conexões (`storageMain.ts`), porque o je-code é um processo só.
//!
//! Caminhos (os mesmos do VS Code, para reusar o perfil do usuário):
//! - aplicação: `<User>/globalStorage/state.vscdb` (o `globalStorageHome` do perfil padrão);
//! - perfil: `<globalStorageHome do perfil>/state.vscdb`; o perfil padrão usa o mesmo armazém da aplicação;
//! - compartilhado: `<appSharedDataHome>/sharedStorage/state.vscdb`;
//! - workspace: `<User>/workspaceStorage/<id>/state.vscdb`.
//!
//! Divergências: o armazém compartilhado não migra chaves do armazém da aplicação (o `MigratingStorage` com
//! `setFallbackStorage` do `ApplicationSharedStorageMain`); as chaves de data de sessão do telemetry que o
//! `ApplicationStorageMain` grava a cada partida não são gravadas; o `IS_NEW_KEY` é mantido como no
//! `BaseStorageMain.init` (a primeira abertura grava `true`, a seguinte `false`).

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_storage::{IStorage, SqliteStorageDatabase, SqliteStorageDatabaseLoggingOptions, SqliteStorageDatabaseOptions, Storage, StorageHint, StorageOptions, StorageValue};

use super::storage::{
    AbstractStorageService, IS_NEW_KEY, StorageBackend, StorageProfile, StorageScope, StorageServiceCoreOptions, StorageSwitchTarget,
    StorageWorkspace, WillSaveStateReason, is_profile_using_default_storage,
};
use crate::log::LogServiceRef;
use crate::resources::fs_path;

const STORAGE_NAME: &str = "state.vscdb";

/// As localizações dos bancos do [`SqliteStorageService`].
#[derive(Clone, Debug)]
pub struct StorageServiceOptions {
    /// O perfil padrão (o armazém da aplicação fica no `globalStorageHome` dele).
    pub default_profile: StorageProfile,
    /// O perfil atual (o padrão, na maioria das vezes).
    pub current_profile: StorageProfile,
    /// `appSharedDataHome/sharedStorage`.
    pub application_shared_storage_folder: std::path::PathBuf,
    /// `workspaceStorageHome`: a pasta que tem uma subpasta por workspace.
    pub workspace_storage_home: std::path::PathBuf,
    /// O workspace inicial, se houver.
    pub initial_workspace: Option<StorageWorkspace>,
}

fn open_storage(path: &Path, log: &LogServiceRef) -> Rc<Storage> {
    let existed = path.exists();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let (log_error, log_trace) = (log.clone(), log.clone());
    let database = SqliteStorageDatabase::new(
        &path.to_string_lossy(),
        SqliteStorageDatabaseOptions {
            logging: Some(SqliteStorageDatabaseLoggingOptions {
                log_error: Some(Rc::new(move |message: &str| log_error.error(message))),
                log_trace: Some(Rc::new(move |message: &str| log_trace.trace(message))),
            }),
            ..Default::default()
        },
    );
    Storage::new(database, StorageOptions { hint: (!existed).then_some(StorageHint::StorageDoesNotExist) })
}

/// O `BaseStorageMain.init`: inicia e mantém o marcador de "armazém novo".
fn init_storage(storage: &Rc<Storage>) -> Promise<()> {
    let storage = storage.clone();
    async_fn(async move {
        storage.init().await?;
        match storage.get_boolean(IS_NEW_KEY, None) {
            None => {
                let _ = storage.set(IS_NEW_KEY, StorageValue::Bool(true), false);
            }
            Some(true) => {
                let _ = storage.set(IS_NEW_KEY, StorageValue::Bool(false), false);
            }
            Some(false) => {}
        }
        Ok(())
    })
}

struct State {
    application_storage: Rc<Storage>,
    application_storage_profile: StorageProfile,
    application_shared_storage: Rc<Storage>,
    profile_storage_profile: StorageProfile,
    profile_storage: Rc<Storage>,
    workspace_storage_id: Option<String>,
    workspace_storage: Option<Rc<Storage>>,
}

/// Liga o `onDidChangeStorage` de um `Storage` ao `emitDidChangeValue` do serviço, no escopo dado.
type StorageWiring = Rc<dyn Fn(StorageScope, &Rc<Storage>) -> je_base::lifecycle::DisposableRef>;

struct SqliteBackend {
    me: std::rc::Weak<SqliteBackend>,
    log: LogServiceRef,
    application_shared_storage_folder: std::path::PathBuf,
    workspace_storage_home: std::path::PathBuf,
    state: RefCell<State>,
    /// Descarta os `Storage` abertos (um por troca de perfil ou de workspace).
    storages: Rc<DisposableStore>,
    profile_storages: Rc<DisposableStore>,
    workspace_storages: Rc<DisposableStore>,
    wiring: RefCell<Option<StorageWiring>>,
}

impl SqliteBackend {
    fn profile_storage_path(profile: &StorageProfile) -> std::path::PathBuf {
        Path::new(&fs_path(&profile.global_storage_home)).join(STORAGE_NAME)
    }

    fn wire(&self, scope: StorageScope, storage: &Rc<Storage>) -> je_base::lifecycle::DisposableRef {
        let wiring = self.wiring.borrow().clone().expect("ligado na construção");
        wiring(scope, storage)
    }

    fn create_profile_storage(&self, profile: &StorageProfile) -> Rc<Storage> {
        // First clear any previously associated disposables
        self.profile_storages.clear();

        let application_storage = self.state.borrow().application_storage.clone();
        let profile_storage = if is_profile_using_default_storage(profile) {
            // If we are using default profile storage, the profile storage is actually the same as
            // application storage. As such we avoid creating the storage library a second time on the same DB.
            application_storage
        } else {
            let storage = open_storage(&Self::profile_storage_path(profile), &self.log);
            self.profile_storages.add_rc(storage.clone());
            storage
        };
        self.profile_storages.add(self.wire(StorageScope::Profile, &profile_storage));

        let mut state = self.state.borrow_mut();
        state.profile_storage_profile = profile.clone();
        profile_storage
    }

    fn create_workspace_storage(&self, workspace: Option<&StorageWorkspace>) -> Option<Rc<Storage>> {
        // First clear any previously associated disposables
        self.workspace_storages.clear();

        self.state.borrow_mut().workspace_storage_id = workspace.map(|w| w.id.clone());
        let workspace = workspace?;
        let storage = open_storage(&self.workspace_storage_home.join(&workspace.id).join(STORAGE_NAME), &self.log);
        self.workspace_storages.add_rc(storage.clone());
        self.workspace_storages.add(self.wire(StorageScope::Workspace, &storage));
        Some(storage)
    }
}

impl StorageBackend for SqliteBackend {
    fn has_scope(&self, scope: &StorageSwitchTarget) -> bool {
        match scope {
            StorageSwitchTarget::Profile(profile) => self.state.borrow().profile_storage_profile.id == profile.id,
            StorageSwitchTarget::Workspace(workspace) => self.state.borrow().workspace_storage_id.as_deref() == Some(workspace.id.as_str()),
        }
    }

    fn do_initialize(&self) -> Promise<()> {
        let (application, shared, profile, workspace) = {
            let state = self.state.borrow();
            (state.application_storage.clone(), state.application_shared_storage.clone(), state.profile_storage.clone(), state.workspace_storage.clone())
        };
        // Init all storage locations
        let mut promises = vec![init_storage(&application), init_storage(&shared)];
        if !Rc::ptr_eq(&profile, &application) {
            promises.push(init_storage(&profile));
        }
        if let Some(workspace) = workspace {
            promises.push(init_storage(&workspace));
        }
        Promise::all_settled(promises).then(|_| ())
    }

    fn get_storage(&self, scope: StorageScope) -> Option<Rc<dyn IStorage>> {
        let state = self.state.borrow();
        match scope {
            StorageScope::ApplicationShared => Some(state.application_shared_storage.clone()),
            StorageScope::Application => Some(state.application_storage.clone()),
            StorageScope::Profile => Some(state.profile_storage.clone()),
            StorageScope::Workspace => state.workspace_storage.clone().map(|s| s as Rc<dyn IStorage>),
        }
    }

    fn get_log_details(&self, scope: StorageScope) -> Option<String> {
        let state = self.state.borrow();
        match scope {
            StorageScope::ApplicationShared => Some(self.application_shared_storage_folder.to_string_lossy().into_owned()),
            StorageScope::Application => Some(fs_path(&state.application_storage_profile.global_storage_home)),
            StorageScope::Profile => Some(fs_path(&state.profile_storage_profile.global_storage_home)),
            StorageScope::Workspace => state.workspace_storage_id.as_ref().map(|id| self.workspace_storage_home.join(id).join(STORAGE_NAME).to_string_lossy().into_owned()),
        }
    }

    fn switch_to_profile(&self, service: &Rc<AbstractStorageService>, to_profile: &StorageProfile, _preserve_data: bool) -> Promise<()> {
        let (old_profile, old_profile_storage, application_storage) = {
            let state = self.state.borrow();
            (state.profile_storage_profile.clone(), state.profile_storage.clone(), state.application_storage.clone())
        };
        if !service.can_switch_profile(&old_profile, to_profile) {
            return Promise::resolved(());
        }

        let old_items = old_profile_storage.items().snapshot();
        let (to_profile, service, this) = (to_profile.clone(), service.clone(), self.me.upgrade().expect("o backend está vivo"));

        // Close old profile storage but only if this is different from application storage!
        let close = if Rc::ptr_eq(&old_profile_storage, &application_storage) { Promise::resolved(()) } else { old_profile_storage.close() };
        close.then_promise(move |()| {
            // Create new profile storage & init
            let new_storage = this.create_profile_storage(&to_profile);
            this.state.borrow_mut().profile_storage = new_storage.clone();
            // Handle data switch and eventing
            init_storage(&new_storage).then(move |()| service.switch_data(&old_items, new_storage.as_ref(), StorageScope::Profile))
        })
    }

    fn switch_to_workspace(&self, service: &Rc<AbstractStorageService>, to_workspace: &StorageWorkspace, _preserve_data: bool) -> Promise<()> {
        let old_workspace_storage = self.state.borrow().workspace_storage.clone();
        let old_items = old_workspace_storage.as_ref().map(|s| s.items().snapshot()).unwrap_or_default();
        let (to_workspace, service, this) = (to_workspace.clone(), service.clone(), self.me.upgrade().expect("o backend está vivo"));

        // Close old workspace storage
        let close = old_workspace_storage.map_or_else(|| Promise::resolved(()), |s| s.close());
        close.then_promise(move |()| {
            // Create new workspace storage & init
            let new_storage = this.create_workspace_storage(Some(&to_workspace)).expect("há workspace");
            this.state.borrow_mut().workspace_storage = Some(new_storage.clone());
            // Handle data switch and eventing
            init_storage(&new_storage).then(move |()| service.switch_data(&old_items, new_storage.as_ref(), StorageScope::Workspace))
        })
    }
}

/// O serviço de armazenamento sobre os bancos `state.vscdb`.
pub struct SqliteStorageService {
    base: Rc<Disposable>,
    service: Rc<AbstractStorageService>,
    backend: Rc<SqliteBackend>,
}

impl SqliteStorageService {
    pub fn new(options: StorageServiceOptions, log: LogServiceRef) -> Rc<Self> {
        let application_storage = open_storage(&SqliteBackend::profile_storage_path(&options.default_profile), &log);
        let application_shared_storage = open_storage(&options.application_shared_storage_folder.join(STORAGE_NAME), &log);

        let storages = DisposableStore::new();
        storages.add_rc(application_storage.clone());
        storages.add_rc(application_shared_storage.clone());

        let backend = Rc::new_cyclic(|me| SqliteBackend {
            me: me.clone(),
            log,
            application_shared_storage_folder: options.application_shared_storage_folder.clone(),
            workspace_storage_home: options.workspace_storage_home.clone(),
            state: RefCell::new(State {
                application_storage: application_storage.clone(),
                application_storage_profile: options.default_profile.clone(),
                application_shared_storage: application_shared_storage.clone(),
                profile_storage_profile: options.current_profile.clone(),
                profile_storage: application_storage.clone(),
                workspace_storage_id: None,
                workspace_storage: None,
            }),
            storages,
            profile_storages: DisposableStore::new(),
            workspace_storages: DisposableStore::new(),
            wiring: RefCell::new(None),
        });
        let service = AbstractStorageService::new(backend.clone(), StorageServiceCoreOptions::default());

        let weak = Rc::downgrade(&service);
        *backend.wiring.borrow_mut() = Some(Rc::new(move |scope, storage: &Rc<Storage>| {
            let weak = weak.clone();
            storage.on_did_change_storage().listen(move |e| {
                if let Some(service) = weak.upgrade() {
                    service.emit_did_change_value(scope, e);
                }
            })
        }));

        let base = Disposable::new();
        base.register(backend.wire(StorageScope::Application, &application_storage));
        base.register(backend.wire(StorageScope::ApplicationShared, &application_shared_storage));
        let profile_storage = backend.create_profile_storage(&options.current_profile);
        backend.state.borrow_mut().profile_storage = profile_storage;
        let workspace_storage = backend.create_workspace_storage(options.initial_workspace.as_ref());
        backend.state.borrow_mut().workspace_storage = workspace_storage;
        base.register(backend.storages.clone());
        base.register(backend.profile_storages.clone());
        base.register(backend.workspace_storages.clone());
        base.register(service.clone());

        Rc::new(Self { base, service, backend })
    }

    /// O serviço de armazenamento (`IStorageService`).
    pub fn service(&self) -> Rc<AbstractStorageService> {
        self.service.clone()
    }

    /// `initialize()`.
    pub fn initialize(&self) -> Promise<()> {
        self.service.initialize()
    }

    /// `close()`: avisa o `onWillSaveState` (`SHUTDOWN`) e fecha todos os bancos.
    pub fn close(&self) -> Promise<()> {
        // Stop periodic scheduler and idle runner as we now collect state normally
        self.service.stop_flush_when_idle();

        // Signal as event so that clients can still store data
        self.service.emit_will_save_state(WillSaveStateReason::Shutdown);

        // Do it
        let (application, shared, profile, workspace) = {
            let state = self.backend.state.borrow();
            (state.application_storage.clone(), state.application_shared_storage.clone(), state.profile_storage.clone(), state.workspace_storage.clone())
        };
        let mut promises = vec![application.close(), shared.close()];
        if !Rc::ptr_eq(&profile, &application) {
            promises.push(profile.close());
        }
        if let Some(workspace) = workspace {
            promises.push(workspace.close());
        }
        Promise::all_settled(promises).then(|_| ())
    }
}

impl IDisposable for SqliteStorageService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

