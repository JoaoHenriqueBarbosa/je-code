//! Porte de `src/vs/platform/userDataProfile/electron-main/userDataProfileStorageIpc.ts`: o
//! `ProfileStorageChangesListenerChannel`, que observa as mudanças no armazenamento de aplicação e no de cada perfil
//! (no processo principal) e as junta num único evento `onDidChange` com os perfis cujo alvo mudou e os valores que
//! mudaram.
//!
//! Não há processo principal do Electron no je-code, mas o papel dele existe no processo único; por isso o canal é o
//! mesmo, sem a camada de IPC: o `IServerChannel` do original só existe para o `RemoteUserDataProfileStorageService`
//! (do renderer) chamar este objeto por outro processo, então [`ProfileStorageChangesListenerChannel::listen`] e
//! [`ProfileStorageChangesListenerChannel::call`] ficam como métodos diretos, sem o contexto (`unknown`) nem o
//! argumento serializável (`IBaseSerializableStorageRequest`), que o original ignora.
//!
//! Decisões de tradução:
//! - **`IStorageMainService`, `IStorageMain` e `IProfileStorageChangeEvent`** (de `storageMainService.ts` e
//!   `storageMain.ts`) são [`IStorageMainService`], [`IStorageMain`] e [`ProfileStorageChangeEvent`], do módulo
//!   `crate::storage`; o canal lê só `applicationStorage`, `onDidChangeProfileStorage`, e de cada `IStorageMain` o
//!   `storage` e o `onDidChangeStorage`.
//! - **`Event.debounce(event, merge, 100)`** é [`Event::debounce`] com as opções padrão (100 ms): o `merge` do primeiro
//!   evento recebe `undefined` (aqui `None`) e devolve a lista de chaves, ou o mapa por perfil (um `IndexMap`, que
//!   guarda a ordem de inserção como o `Map`).
//! - **`MutableDisposable<IDisposable>`** é o [`MutableDisposable`] sobre `dyn IDisposable`; `disposable.value = x` é
//!   `set(Some(x))` e `= undefined` é `set(None)`.
//! - **`keyTargets[key]`** é o [`StorageTarget`] da chave, ou `None` quando a chave não tem alvo (o `undefined` do
//!   original).

use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::errors::Error;
use je_base::event::{DebounceOptions, Emitter, EmitterOptions, Event};
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, MutableDisposable};
use je_base::promise::Promise;
use je_storage::StorageChangeEvent;

use super::user_data_profile_storage_service::{ProfileStorageChanges, ProfileStorageValueChanges};
use super::{IUserDataProfilesService, UserDataProfile};
use crate::log::{LogServiceRef, Logger};
use crate::storage::{IStorageMain, IStorageMainService, ProfileStorageChangeEvent, StorageScope, StorageTarget, StorageValueChangeEvent, TARGET_KEY, load_key_targets};

/// O valor acumulado por perfil durante o debounce: `{ profile, keys, storage }`.
struct PendingProfileChanges {
    profile: UserDataProfile,
    keys: Vec<String>,
    storage: Rc<dyn IStorageMain>,
}

/// `Map<string, { profile; keys; storage }>`, pelo id do perfil.
type PendingChanges = IndexMap<String, PendingProfileChanges>;

/// `ProfileStorageChangesListenerChannel`.
pub struct ProfileStorageChangesListenerChannel {
    me: Weak<ProfileStorageChangesListenerChannel>,
    base: Rc<Disposable>,
    storage_main_service: Rc<dyn IStorageMainService>,
    user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    log_service: LogServiceRef,
    disposable: Rc<MutableDisposable<dyn IDisposable>>,
    on_did_change: Rc<Emitter<ProfileStorageChanges>>,
}

impl ProfileStorageChangesListenerChannel {
    pub fn new(storage_main_service: Rc<dyn IStorageMainService>, user_data_profiles_service: Rc<dyn IUserDataProfilesService>, log_service: LogServiceRef) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| {
            let base = Disposable::new();
            let disposable = MutableDisposable::<dyn IDisposable>::new();
            base.register(disposable.clone());
            let on_did_change = Emitter::with_options(EmitterOptions {
                // Start listening to profile storage changes only when someone is listening
                on_will_add_first_listener: Some(Rc::new({
                    let me = me.clone();
                    move |_: &Emitter<ProfileStorageChanges>| {
                        if let Some(this) = me.upgrade() {
                            this.disposable.set(Some(this.register_storage_change_listeners()));
                        }
                    }
                })),
                // Stop listening to profile storage changes when no one is listening
                on_did_remove_last_listener: Some(Rc::new({
                    let me = me.clone();
                    move |_: &Emitter<ProfileStorageChanges>| {
                        if let Some(this) = me.upgrade() {
                            this.disposable.set(None);
                        }
                    }
                })),
                ..EmitterOptions::default()
            });
            base.register(on_did_change.clone());
            Self { me: me.clone(), base, storage_main_service, user_data_profiles_service, log_service, disposable, on_did_change }
        })
    }

    fn register_storage_change_listeners(&self) -> DisposableRef {
        self.log_service.debug("ProfileStorageChangesListenerChannel#registerStorageChangeListeners");
        let disposables = DisposableStore::new();

        let on_did_change_application_storage = self.storage_main_service.application_storage().on_did_change_storage().debounce(
            |keys: Option<Vec<String>>, e: &StorageChangeEvent| {
                let mut keys = keys.unwrap_or_default();
                keys.push(e.key.clone());
                keys
            },
            DebounceOptions::default(),
            None,
        );
        let me = self.me.clone();
        disposables.add(on_did_change_application_storage.listen(move |keys| {
            if let Some(this) = me.upgrade() {
                this.on_did_change_application_storage(keys);
            }
        }));

        let on_did_change_profile_storage = self.storage_main_service.on_did_change_profile_storage().debounce(
            |changes: Option<PendingChanges>, e: &ProfileStorageChangeEvent| {
                let mut changes = changes.unwrap_or_default();
                changes
                    .entry(e.profile.id.clone())
                    .or_insert_with(|| PendingProfileChanges { profile: e.profile.clone(), keys: Vec::new(), storage: e.storage.clone() })
                    .keys
                    .push(e.key.clone());
                changes
            },
            DebounceOptions::default(),
            None,
        );
        let me = self.me.clone();
        disposables.add(on_did_change_profile_storage.listen(move |changes| {
            if let Some(this) = me.upgrade() {
                this.on_did_change_profile_storage(changes);
            }
        }));

        disposables
    }

    fn on_did_change_application_storage(&self, keys: &[String]) {
        let default_profile = self.user_data_profiles_service.default_profile();
        let target_changed_profiles: Vec<UserDataProfile> = if keys.iter().any(|key| key == TARGET_KEY) { vec![default_profile.clone()] } else { Vec::new() };
        let mut profile_storage_value_changes: Vec<ProfileStorageValueChanges> = Vec::new();
        let keys: Vec<&String> = keys.iter().filter(|key| *key != TARGET_KEY).collect();
        if !keys.is_empty() {
            let key_targets = load_key_targets(self.storage_main_service.application_storage().storage().as_ref());
            profile_storage_value_changes.push(ProfileStorageValueChanges { profile: default_profile, changes: Self::value_changes(&keys, &key_targets) });
        }
        self.trigger_events(target_changed_profiles, profile_storage_value_changes);
    }

    fn on_did_change_profile_storage(&self, changes: &PendingChanges) {
        let mut target_changed_profiles: Vec<UserDataProfile> = Vec::new();
        let mut profile_storage_value_changes: IndexMap<String, ProfileStorageValueChanges> = IndexMap::new();
        for (profile_id, profile_changes) in changes {
            if profile_changes.keys.iter().any(|key| key == TARGET_KEY) {
                target_changed_profiles.push(profile_changes.profile.clone());
            }
            let keys: Vec<&String> = profile_changes.keys.iter().filter(|key| *key != TARGET_KEY).collect();
            if !keys.is_empty() {
                let key_targets = load_key_targets(profile_changes.storage.storage().as_ref());
                profile_storage_value_changes.insert(profile_id.clone(), ProfileStorageValueChanges { profile: profile_changes.profile.clone(), changes: Self::value_changes(&keys, &key_targets) });
            }
        }
        self.trigger_events(target_changed_profiles, profile_storage_value_changes.into_values().collect());
    }

    /// `keys.map(key => ({ key, scope: StorageScope.PROFILE, target: keyTargets[key] }))`.
    fn value_changes(keys: &[&String], key_targets: &je_base::map::OrderedMap<String, StorageTarget>) -> Vec<StorageValueChangeEvent> {
        keys.iter()
            .map(|key| StorageValueChangeEvent { scope: StorageScope::Profile, key: (*key).clone(), target: key_targets.get(*key).copied(), external: false })
            .collect()
    }

    fn trigger_events(&self, target_changes: Vec<UserDataProfile>, value_changes: Vec<ProfileStorageValueChanges>) {
        if !target_changes.is_empty() || !value_changes.is_empty() {
            self.on_did_change.fire(ProfileStorageChanges { value_changes, target_changes });
        }
    }

    /// `listen(_, event, arg)`: o único evento é `onDidChange`.
    pub fn listen(&self, event: &str) -> Result<Event<ProfileStorageChanges>, Error> {
        match event {
            "onDidChange" => Ok(self.on_did_change.event()),
            _ => Err(Error::new(format!("[ProfileStorageChangesListenerChannel] Event not found: {event}"))),
        }
    }

    /// `call(_, command)`: o canal não tem comandos.
    pub fn call(&self, command: &str) -> Promise<()> {
        Promise::rejected(Error::new(format!("Call not found: {command}")))
    }
}

impl IDisposable for ProfileStorageChangesListenerChannel {
    fn dispose(&self) {
        self.base.dispose();
    }
}
