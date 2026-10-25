//! O serviço de armazenamento sobre o `je-storage`, para o cliente (a janela): o `RemoteStorageService`
//! (`storageService.ts`), que lê e escreve os bancos `state.vscdb` do VS Code (aplicação, aplicação compartilhada,
//! perfil e workspace) pelo [`StorageMainService`](super::StorageMainService), o dono das conexões.
//!
//! O je-code é um processo só: onde o `RemoteStorageService` abre um `Storage` sobre um banco que chama o canal de IPC
//! (`ApplicationStorageDatabaseClient` e companhia), este serviço usa o armazém do [`StorageMainService`] direto
//! (`IStorageMain.storage`, que fica em memória até o `init` do armazém).
//!
//! Caminhos (os mesmos do VS Code, para reusar o perfil do usuário), todos resolvidos pelo [`StorageMainService`]:
//! - aplicação: `<User>/globalStorage/state.vscdb` (o `globalStorageHome` do perfil padrão);
//! - perfil: `<globalStorageHome do perfil>/state.vscdb`; o perfil padrão usa o mesmo armazém da aplicação;
//! - compartilhado: `<appSharedDataHome>/sharedStorage/state.vscdb`;
//! - workspace: `<User>/workspaceStorage/<id>/state.vscdb`.
//!
//! Divergências:
//! - o `close()` do original fecha os `Storage` do cliente (que só avisam o IPC); aqui os armazéns são do
//!   [`StorageMainService`], que os fecha no `onWillShutdown`. O `close()` avisa o `onWillSaveState` (`SHUTDOWN`) como o
//!   original e descarrega as escritas pendentes dos armazéns (`flush` com `SHUTDOWN`), para o que foi gravado estar no
//!   disco quando a promessa assentar.
//! - na troca de perfil o armazém antigo não é fechado (é do [`StorageMainService`] e outras janelas podem usá-lo).
//! - o destino de `switch` para um workspace traz só o `id` ([`StorageWorkspace`]): o armazém trocado é aberto pelo
//!   identificador de janela vazia daquele `id`, então o `workspace.json` de um workspace que ainda não tem pasta de
//!   armazém não é gravado nessa troca (o workspace inicial, que vem completo, grava).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::Error;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_storage::IStorage;

use super::storage::{
    AbstractStorageService, StorageBackend, StorageProfile, StorageScope, StorageService, StorageServiceCoreOptions, StorageSwitchTarget, StorageWorkspace, WillSaveStateReason,
};
use super::storage_main::{IStorageMain, file_fs_path};
use super::storage_main_service::IStorageMainService;
use crate::user_data_profile::user_data_profile_storage_service::storage_profile;
use crate::user_data_profile::{IUserDataProfilesService, UserDataProfile};
use crate::workspace::{AnyWorkspaceIdentifier, EmptyWorkspaceIdentifier};

/// De onde o [`SqliteStorageService`] tira os armazéns.
#[derive(Clone)]
pub struct StorageServiceOptions {
    /// O dono das conexões dos bancos.
    pub storage_main_service: Rc<dyn IStorageMainService>,
    /// Os perfis (o perfil padrão é o dono do armazém da aplicação; `switch` acha o perfil de destino por aqui).
    pub user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    /// O perfil atual (o padrão, na maioria das vezes).
    pub current_profile: UserDataProfile,
    /// O workspace inicial, se houver.
    pub initial_workspace: Option<AnyWorkspaceIdentifier>,
}

struct State {
    profile_storage_profile: UserDataProfile,
    profile_storage: Rc<dyn IStorageMain>,
    workspace_storage_id: Option<String>,
    workspace_storage: Option<Rc<dyn IStorageMain>>,
}

/// Liga o `onDidChangeStorage` de um armazém ao `emitDidChangeValue` do serviço, no escopo dado.
type StorageWiring = Rc<dyn Fn(StorageScope, &Rc<dyn IStorageMain>) -> DisposableRef>;

struct SqliteBackend {
    storage_main_service: Rc<dyn IStorageMainService>,
    user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    state: RefCell<State>,
    /// Descarta as ligações do armazém de perfil e do de workspace (uma por troca).
    profile_storages: Rc<DisposableStore>,
    workspace_storages: Rc<DisposableStore>,
    wiring: RefCell<Option<StorageWiring>>,
}

impl SqliteBackend {
    fn wire(&self, scope: StorageScope, storage: &Rc<dyn IStorageMain>) -> DisposableRef {
        let wiring = self.wiring.borrow().clone().expect("ligado na construção");
        wiring(scope, storage)
    }

    fn create_profile_storage(&self, profile: &UserDataProfile) -> Rc<dyn IStorageMain> {
        // First clear any previously associated disposables
        self.profile_storages.clear();

        // If we are using default profile storage, the profile storage is actually the same as application storage: the
        // storage main service hands out the application storage for it.
        let profile_storage = self.storage_main_service.profile_storage(profile);
        self.profile_storages.add(self.wire(StorageScope::Profile, &profile_storage));

        self.state.borrow_mut().profile_storage_profile = profile.clone();
        profile_storage
    }

    fn create_workspace_storage(&self, workspace: Option<&AnyWorkspaceIdentifier>) -> Option<Rc<dyn IStorageMain>> {
        // First clear any previously associated disposables
        self.workspace_storages.clear();

        self.state.borrow_mut().workspace_storage_id = workspace.map(|w| w.id().to_string());
        let workspace_storage = self.storage_main_service.workspace_storage(workspace?);
        self.workspace_storages.add(self.wire(StorageScope::Workspace, &workspace_storage));
        Some(workspace_storage)
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
        let (profile, workspace) = {
            let state = self.state.borrow();
            (state.profile_storage.clone(), state.workspace_storage.clone())
        };
        let application = self.storage_main_service.application_storage();

        // Init all storage locations
        let mut promises = vec![application.init(), self.storage_main_service.application_shared_storage().init()];
        if !Rc::ptr_eq(&profile, &application) {
            promises.push(profile.init());
        }
        if let Some(workspace) = workspace {
            promises.push(workspace.init());
        }
        Promise::all_settled(promises).then(|_| ())
    }

    fn get_storage(&self, scope: StorageScope) -> Option<Rc<dyn IStorage>> {
        match scope {
            StorageScope::ApplicationShared => Some(self.storage_main_service.application_shared_storage().storage()),
            StorageScope::Application => Some(self.storage_main_service.application_storage().storage()),
            StorageScope::Profile => Some(self.state.borrow().profile_storage.storage()),
            StorageScope::Workspace => self.state.borrow().workspace_storage.as_ref().map(|storage| storage.storage()),
        }
    }

    fn get_log_details(&self, scope: StorageScope) -> Option<String> {
        match scope {
            StorageScope::ApplicationShared => self.storage_main_service.application_shared_storage().path(),
            StorageScope::Application => Some(file_fs_path(&self.user_data_profiles_service.default_profile().global_storage_home)),
            StorageScope::Profile => Some(file_fs_path(&self.state.borrow().profile_storage_profile.global_storage_home)),
            StorageScope::Workspace => self.state.borrow().workspace_storage.as_ref().and_then(|storage| storage.path()),
        }
    }

    fn switch_to_profile(&self, service: &Rc<AbstractStorageService>, to_profile: &StorageProfile, _preserve_data: bool) -> Promise<()> {
        let (old_profile, old_profile_storage) = {
            let state = self.state.borrow();
            (state.profile_storage_profile.clone(), state.profile_storage.clone())
        };
        if !service.can_switch_profile(&storage_profile(&old_profile), to_profile) {
            return Promise::resolved(());
        }
        let Some(profile) = self.user_data_profiles_service.profiles().into_iter().find(|profile| profile.id == to_profile.id) else {
            return Promise::rejected(Error::new(format!("Unable to switch storage to the unknown profile '{}'", to_profile.id)));
        };

        let old_items = old_profile_storage.items().snapshot();

        // Create new profile storage & init (the old one belongs to the storage main service and stays open)
        let new_storage = self.create_profile_storage(&profile);
        self.state.borrow_mut().profile_storage = new_storage.clone();

        // Handle data switch and eventing
        let service = service.clone();
        new_storage.init().then(move |()| service.switch_data(&old_items, new_storage.storage().as_ref(), StorageScope::Profile))
    }

    fn switch_to_workspace(&self, service: &Rc<AbstractStorageService>, to_workspace: &StorageWorkspace, _preserve_data: bool) -> Promise<()> {
        let old_items = self.state.borrow().workspace_storage.as_ref().map(|storage| storage.items().snapshot()).unwrap_or_default();

        // Create new workspace storage & init
        let workspace = AnyWorkspaceIdentifier::Empty(EmptyWorkspaceIdentifier { id: to_workspace.id.clone() });
        let new_storage = self.create_workspace_storage(Some(&workspace)).expect("há workspace");
        self.state.borrow_mut().workspace_storage = Some(new_storage.clone());

        // Handle data switch and eventing
        let service = service.clone();
        new_storage.init().then(move |()| service.switch_data(&old_items, new_storage.storage().as_ref(), StorageScope::Workspace))
    }
}

/// O serviço de armazenamento sobre os bancos `state.vscdb`.
pub struct SqliteStorageService {
    base: Rc<Disposable>,
    service: Rc<AbstractStorageService>,
}

impl SqliteStorageService {
    pub fn new(options: StorageServiceOptions) -> Rc<Self> {
        let StorageServiceOptions { storage_main_service, user_data_profiles_service, current_profile, initial_workspace } = options;
        let application_storage = storage_main_service.application_storage();
        let application_shared_storage = storage_main_service.application_shared_storage();

        let backend = Rc::new(SqliteBackend {
            storage_main_service,
            user_data_profiles_service,
            state: RefCell::new(State { profile_storage_profile: current_profile.clone(), profile_storage: application_storage.clone(), workspace_storage_id: None, workspace_storage: None }),
            profile_storages: DisposableStore::new(),
            workspace_storages: DisposableStore::new(),
            wiring: RefCell::new(None),
        });
        let service = AbstractStorageService::new(backend.clone(), StorageServiceCoreOptions::default());

        let weak = Rc::downgrade(&service);
        *backend.wiring.borrow_mut() = Some(Rc::new(move |scope, storage: &Rc<dyn IStorageMain>| {
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
        let profile_storage = backend.create_profile_storage(&current_profile);
        backend.state.borrow_mut().profile_storage = profile_storage;
        let workspace_storage = backend.create_workspace_storage(initial_workspace.as_ref());
        backend.state.borrow_mut().workspace_storage = workspace_storage;
        base.register(backend.profile_storages.clone());
        base.register(backend.workspace_storages.clone());
        base.register(service.clone());

        Rc::new(Self { base, service })
    }

    /// O serviço de armazenamento (`IStorageService`).
    pub fn service(&self) -> Rc<AbstractStorageService> {
        self.service.clone()
    }

    /// `initialize()`.
    pub fn initialize(&self) -> Promise<()> {
        self.service.initialize()
    }

    /// `close()`: avisa o `onWillSaveState` (`SHUTDOWN`) e descarrega as escritas pendentes. Os bancos são fechados pelo
    /// [`StorageMainService`](super::StorageMainService), no `onWillShutdown`.
    pub fn close(&self) -> Promise<()> {
        // Stop periodic scheduler and idle runner as we now collect state normally
        self.service.stop_flush_when_idle();

        // Signal as event so that clients can still store data, and flush what is pending
        StorageService::flush(&self.service, WillSaveStateReason::Shutdown)
    }
}

impl IDisposable for SqliteStorageService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
