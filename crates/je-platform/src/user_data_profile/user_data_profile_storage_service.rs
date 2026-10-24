//! Porte de `src/vs/platform/userDataProfile/common/userDataProfileStorageService.ts`: a leitura e a escrita do
//! armazenamento de estado global de um perfil (`IUserDataProfileStorageService`).
//!
//! Decisões de tradução:
//! - `AbstractUserDataProfileStorageService` tem dois membros abstratos, `onDidChange` e `createStorageDatabase`:
//!   são o trait [`UserDataProfileStorageBackend`], que a classe concreta implementa; a base é a struct
//!   [`AbstractUserDataProfileStorageService`], que recebe o backend e implementa
//!   [`IUserDataProfileStorageService`]. (O `TestUserDataProfileStorageService` do teste é um backend.)
//! - O `StorageService` privado do original (um `AbstractStorageService` só com o armazém de perfil, sobre um
//!   `IStorageDatabase`) é o [`ProfileStorageBackend`], o [`StorageBackend`] do porte.
//! - `withProfileScopedStorageService<T>` é genérico no resultado e por isso não cabe num `dyn` trait: é um método
//!   da struct base, e o trait só tem `readStorageData` e `updateStorageData`.
//! - `Map<string, string | undefined | null>` (os dados a gravar) é `IndexMap<String, Option<String>>`; `None` remove
//!   a chave, como o `undefined` e o `null` do original (o `storeAll` remove as chaves sem valor).
//! - `IUserDataProfile` do armazenamento é o [`StorageProfile`] mínimo: [`storage_profile`] faz a conversão.
//! - O `DisposableMap` dos serviços persistidos é um `IndexMap` que o `dispose` esvazia e descarta.
//!
//! Fora do porte: `RemoteUserDataProfileStorageService` (pede o `IRemoteService` e os canais de IPC, que o je-code
//! não tem: é um processo só).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_storage::{IStorage, Storage, StorageDatabase, StorageOptions, StorageValue};

use super::UserDataProfile;
use crate::storage::{
    AbstractStorageService, StorageBackend, StorageEntry, StorageProfile, StorageScope, StorageService, StorageServiceCoreOptions, StorageSwitchTarget, StorageTarget, StorageValueChangeEvent,
    StorageWorkspace, WillSaveStateReason,
};

/// `IProfileStorageValueChanges`.
#[derive(Clone, Debug)]
pub struct ProfileStorageValueChanges {
    pub profile: UserDataProfile,
    pub changes: Vec<StorageValueChangeEvent>,
}

/// `IProfileStorageChanges`.
#[derive(Clone, Debug)]
pub struct ProfileStorageChanges {
    pub target_changes: Vec<UserDataProfile>,
    pub value_changes: Vec<ProfileStorageValueChanges>,
}

/// `IStorageValue` (o deste módulo, que não é o de `userDataSync.ts`): `value` ausente é `undefined`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileStorageValue {
    pub value: Option<String>,
    pub target: StorageTarget,
    pub scope: Option<StorageScope>,
}

/// `IUserDataProfileStorageService`.
pub trait IUserDataProfileStorageService {
    /// Emitted whenever data is updated or deleted in a profile storage or target of a profile storage entry changes.
    fn on_did_change(&self) -> Event<ProfileStorageChanges>;

    /// Return the requested profile storage data. `profile` is the profile from which the data has to be read.
    fn read_storage_data(&self, profile: &UserDataProfile) -> Promise<IndexMap<String, ProfileStorageValue>>;

    /// Update the given profile storage data in the profile storage. `scope` ausente é `PROFILE`.
    fn update_storage_data(&self, profile: &UserDataProfile, data: IndexMap<String, Option<String>>, target: StorageTarget, scope: Option<StorageScope>) -> Promise<()>;
}

service_id!(pub I_USER_DATA_PROFILE_STORAGE_SERVICE: dyn IUserDataProfileStorageService = "IUserDataProfileStorageService");

/// O [`StorageProfile`] (o que o armazenamento lê) de um perfil.
pub fn storage_profile(profile: &UserDataProfile) -> StorageProfile {
    StorageProfile {
        id: profile.id.clone(),
        is_default: profile.is_default,
        use_default_global_state: profile.use_default_flags.and_then(|flags| flags.global_state).unwrap_or(false),
        global_storage_home: profile.global_storage_home.clone(),
    }
}

/// O que `AbstractUserDataProfileStorageService` deixa abstrato: `onDidChange` e `createStorageDatabase`.
pub trait UserDataProfileStorageBackend {
    fn on_did_change(&self) -> Event<ProfileStorageChanges>;

    fn create_storage_database(&self, profile: &UserDataProfile) -> Promise<Rc<dyn StorageDatabase>>;
}

/// `StorageService` (privado no original): um serviço de armazenamento só com o escopo de perfil, sobre um banco.
struct ProfileStorageBackend {
    database: Promise<Rc<dyn StorageDatabase>>,
    storage: Rc<RefCell<Option<Rc<Storage>>>>,
    service: RefCell<Weak<AbstractStorageService>>,
}

impl ProfileStorageBackend {
    /// O serviço de armazenamento do banco, já ligado a este backend.
    fn create_service(database: Promise<Rc<dyn StorageDatabase>>) -> Rc<AbstractStorageService> {
        let backend = Rc::new(Self { database, storage: Rc::default(), service: RefCell::new(Weak::new()) });
        let service = AbstractStorageService::new(backend.clone(), StorageServiceCoreOptions { flush_interval: 100 });
        *backend.service.borrow_mut() = Rc::downgrade(&service);
        service
    }
}

impl StorageBackend for ProfileStorageBackend {
    fn has_scope(&self, _scope: &StorageSwitchTarget) -> bool {
        false
    }

    fn do_initialize(&self) -> Promise<()> {
        let (database, slot, service) = (self.database.clone(), self.storage.clone(), self.service.borrow().clone());
        async_fn(async move {
            let database = database.await?;
            let storage = Storage::new(database, StorageOptions { hint: None });
            if let Some(service) = service.upgrade() {
                let weak = Rc::downgrade(&service);
                service.register(storage.on_did_change_storage().listen(move |event| {
                    if let Some(service) = weak.upgrade() {
                        service.emit_did_change_value(StorageScope::Profile, event);
                    }
                }));
                let to_close = storage.clone();
                service.register(to_disposable(move || {
                    let _ = to_close.close();
                    to_close.dispose();
                }));
            }
            *slot.borrow_mut() = Some(storage.clone());
            storage.init().await
        })
    }

    fn get_storage(&self, scope: StorageScope) -> Option<Rc<dyn IStorage>> {
        if scope != StorageScope::Profile {
            return None;
        }
        self.storage.borrow().clone().map(|storage| storage as Rc<dyn IStorage>)
    }

    fn get_log_details(&self, _scope: StorageScope) -> Option<String> {
        None
    }

    fn switch_to_profile(&self, _service: &Rc<AbstractStorageService>, _to_profile: &StorageProfile, _preserve_data: bool) -> Promise<()> {
        Promise::resolved(())
    }

    fn switch_to_workspace(&self, _service: &Rc<AbstractStorageService>, _to_workspace: &StorageWorkspace, _preserve_data: bool) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `getItems`: os itens do perfil (`PROFILE`) e, no perfil padrão, os compartilhados (`APPLICATION_SHARED`).
fn get_items(storage_service: &dyn StorageService, is_default: bool) -> IndexMap<String, ProfileStorageValue> {
    let mut result = IndexMap::new();
    let mut populate = |scope: StorageScope, target: StorageTarget| {
        for key in storage_service.keys(scope, target) {
            let value = storage_service.get(&key, scope, None);
            result.insert(key, ProfileStorageValue { value, target, scope: Some(scope) });
        }
    };
    populate(StorageScope::Profile, StorageTarget::User);
    populate(StorageScope::Profile, StorageTarget::Machine);
    if is_default {
        populate(StorageScope::ApplicationShared, StorageTarget::User);
        populate(StorageScope::ApplicationShared, StorageTarget::Machine);
    }
    result
}

/// `writeItems`.
fn write_items(storage_service: &dyn StorageService, items: &IndexMap<String, Option<String>>, target: StorageTarget, scope: StorageScope) {
    let entries: Vec<StorageEntry> = items
        .iter()
        .map(|(key, value)| StorageEntry {
            key: key.clone(),
            value: value.as_ref().map_or(StorageValue::Undefined, |text| StorageValue::String(text.clone())),
            scope,
            target,
        })
        .collect();
    storage_service.store_all(&entries, true);
}

/// `AbstractUserDataProfileStorageService`.
pub struct AbstractUserDataProfileStorageService {
    me: Weak<Self>,
    base: Rc<Disposable>,
    storage_service: Rc<dyn StorageService>,
    backend: Rc<dyn UserDataProfileStorageBackend>,
    storage_services_map: Option<Rc<RefCell<IndexMap<String, Rc<AbstractStorageService>>>>>,
}

impl AbstractUserDataProfileStorageService {
    /// `persistStorages` guarda o serviço de armazenamento de cada perfil entre as chamadas.
    pub fn new(persist_storages: bool, storage_service: Rc<dyn StorageService>, backend: Rc<dyn UserDataProfileStorageBackend>) -> Rc<Self> {
        let base = Disposable::new();
        let storage_services_map: Option<Rc<RefCell<IndexMap<String, Rc<AbstractStorageService>>>>> = persist_storages.then(Rc::default);
        if let Some(map) = &storage_services_map {
            let map = map.clone();
            base.register(to_disposable(move || {
                let services: Vec<Rc<AbstractStorageService>> = map.borrow_mut().drain(..).map(|(_, service)| service).collect();
                for service in services {
                    service.dispose();
                }
            }));
        }
        Rc::new_cyclic(|me| Self { me: me.clone(), base, storage_service, backend, storage_services_map })
    }

    fn rc(&self) -> Rc<Self> {
        self.me.upgrade().expect("o serviço está vivo enquanto é usado")
    }

    /// `withProfileScopedStorageService`: chama `f` com um serviço de armazenamento do perfil.
    pub fn with_profile_scoped_storage_service<T: Clone + 'static>(
        self: &Rc<Self>,
        profile: &UserDataProfile,
        f: impl FnOnce(Rc<dyn StorageService>) -> Promise<T> + 'static,
    ) -> Promise<T> {
        let this = self.clone();
        let profile = profile.clone();
        async_fn(async move {
            if this.storage_service.has_scope(&StorageSwitchTarget::Profile(storage_profile(&profile))) {
                return f(this.storage_service.clone()).await;
            }

            let persisted = this.storage_services_map.as_ref().and_then(|map| map.borrow().get(&profile.id).cloned());
            let storage_service = match persisted {
                Some(storage_service) => storage_service,
                None => {
                    let storage_service = ProfileStorageBackend::create_service(this.backend.create_storage_database(&profile));
                    if let Some(map) = &this.storage_services_map {
                        map.borrow_mut().insert(profile.id.clone(), storage_service.clone());
                    }

                    if let Err(error) = storage_service.initialize().await {
                        let removed = this.storage_services_map.as_ref().and_then(|map| map.borrow_mut().shift_remove(&profile.id));
                        match removed {
                            Some(removed) => removed.dispose(),
                            None => storage_service.dispose(),
                        }
                        return Err(error);
                    }
                    storage_service
                }
            };

            let scoped: Rc<dyn StorageService> = Rc::new(storage_service.clone());
            let outcome: Result<T, Error> = async {
                let result = f(scoped).await?;
                StorageService::flush(&storage_service, WillSaveStateReason::None).await?;
                Ok(result)
            }
            .await;

            // finally
            let is_persisted = this.storage_services_map.as_ref().is_some_and(|map| map.borrow().contains_key(&profile.id));
            if !is_persisted {
                storage_service.dispose();
            }
            outcome
        })
    }
}

impl IDisposable for AbstractUserDataProfileStorageService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IUserDataProfileStorageService for AbstractUserDataProfileStorageService {
    fn on_did_change(&self) -> Event<ProfileStorageChanges> {
        self.backend.on_did_change()
    }

    fn read_storage_data(&self, profile: &UserDataProfile) -> Promise<IndexMap<String, ProfileStorageValue>> {
        let is_default = profile.is_default;
        self.rc().with_profile_scoped_storage_service(profile, move |storage_service| Promise::resolved(get_items(storage_service.as_ref(), is_default)))
    }

    fn update_storage_data(&self, profile: &UserDataProfile, data: IndexMap<String, Option<String>>, target: StorageTarget, scope: Option<StorageScope>) -> Promise<()> {
        let scope = scope.unwrap_or(StorageScope::Profile);
        self.rc().with_profile_scoped_storage_service(profile, move |storage_service| {
            write_items(storage_service.as_ref(), &data, target, scope);
            Promise::resolved(())
        })
    }
}
