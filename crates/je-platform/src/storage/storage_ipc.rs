//! Porte de `src/vs/platform/storage/electron-main/storageIpc.ts` (e dos tipos de requisição de
//! `common/storageIpc.ts` que ele lê): o `StorageDatabaseChannel`, o canal que expõe os armazéns do
//! [`StorageMainService`](super::StorageMainService) aos clientes (leitura, escrita, `compareAndSwap` e o evento de
//! mudança agrupado).
//!
//! Não há processo principal do Electron no je-code, mas o papel dele existe no processo único; por isso o canal é o
//! mesmo, sem a camada de IPC: o `IServerChannel` do original só existe para os clientes de outro processo chamarem este
//! objeto, então [`StorageDatabaseChannel::listen`] e [`StorageDatabaseChannel::call`] ficam como métodos diretos, sem o
//! contexto (`unknown`).
//!
//! Decisões de tradução:
//! - **`IBaseSerializableStorageRequest` e as requisições que a estendem** (`ISerializableUpdateRequest`,
//!   `ISerializableGetValueRequest`, `ISerializableCompareAndSwapRequest`) são a struct [`StorageRequest`], com os
//!   campos de todas (o `arg` do original é um objeto que cada comando lê como a requisição dele). O `profile` e o
//!   `workspace` chegam já como [`UserDataProfile`] e [`AnyWorkspaceIdentifier`]: o `revive<IUserDataProfile>` e o
//!   `reviveIdentifier` do original só desserializam o que veio pelo IPC.
//! - **`Item[]`** é `Vec<(String, String)>`; **`ISerializableItemsChangeEvent`** é [`SerializableItemsChangeEvent`].
//! - **O retorno de `call`** (`any` no original) é o [`StorageCallResult`], com uma variante por forma de retorno.
//! - **`Event.debounce(storage.onDidChangeStorage, merge, 100)`** é [`Event::debounce`] com 100 ms.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use indexmap::{IndexMap, IndexSet};
use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::event::{DebounceOptions, Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::symbols::Delay;
use je_storage::StorageValue;

use super::storage::CompareAndSwapResult;
use super::storage_main::{IStorageMain, StorageMainChangeEvent};
use super::storage_main_service::IStorageMainService;
use crate::log::{LogServiceRef, Logger};
use crate::user_data_profile::UserDataProfile;
use crate::workspace::AnyWorkspaceIdentifier;

/// `ISerializableItemsChangeEvent`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SerializableItemsChangeEvent {
    pub changed: Vec<(String, String)>,
    pub deleted: Vec<String>,
}

/// `IBaseSerializableStorageRequest` e as requisições que a estendem.
#[derive(Clone, Debug, Default)]
pub struct StorageRequest {
    /// The profile storage to target. Se ausente (e sem `workspace`), é o armazém da aplicação.
    pub profile: Option<UserDataProfile>,
    /// The workspace storage to target.
    pub workspace: Option<AnyWorkspaceIdentifier>,
    /// Quando verdadeiro (e sem `profile` nem `workspace`), o alvo é o armazém compartilhado da aplicação.
    pub application_shared: bool,
    /// Dados extras do comando (o `isUsed` leva o caminho).
    pub payload: Option<String>,
    /// `ISerializableUpdateRequest.insert`.
    pub insert: Option<Vec<(String, String)>>,
    /// `ISerializableUpdateRequest.delete`.
    pub delete: Option<Vec<String>>,
    /// `ISerializableGetValueRequest.key` (e a do `compareAndSwap`).
    pub key: String,
    /// `ISerializableCompareAndSwapRequest.expectedValue`.
    pub expected_value: Option<String>,
    /// `ISerializableCompareAndSwapRequest.newValue`.
    pub new_value: String,
}

/// O que o `call` devolve (`Promise<any>`): uma variante por forma de retorno dos comandos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageCallResult {
    /// `getItems` e `getFallbackApplicationStorageItems`.
    Items(Vec<(String, String)>),
    /// `getValue`.
    Value(Option<String>),
    /// `compareAndSwap`.
    CompareAndSwap(CompareAndSwapResult),
    /// `isUsed`.
    Used(bool),
    /// `updateItems` e `optimize` (sem retorno).
    Done,
}

/// `StorageDatabaseChannel`.
pub struct StorageDatabaseChannel {
    base: Rc<Disposable>,
    log_service: LogServiceRef,
    storage_main_service: Rc<dyn IStorageMainService>,
    on_did_change_application_storage_emitter: Rc<Emitter<SerializableItemsChangeEvent>>,
    on_did_change_application_shared_storage_emitter: Rc<Emitter<SerializableItemsChangeEvent>>,
    map_profile_to_on_did_change_profile_storage_emitter: RefCell<HashMap<String /* profile ID */, Rc<Emitter<SerializableItemsChangeEvent>>>>,
    me: Weak<StorageDatabaseChannel>,
}

impl StorageDatabaseChannel {
    const STORAGE_CHANGE_DEBOUNCE_TIME: u64 = 100;

    pub fn new(log_service: LogServiceRef, storage_main_service: Rc<dyn IStorageMainService>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_application_storage_emitter = Emitter::new();
        base.register(on_did_change_application_storage_emitter.clone());
        let on_did_change_application_shared_storage_emitter = Emitter::new();
        base.register(on_did_change_application_shared_storage_emitter.clone());

        Self::register_storage_change_listeners(&base, storage_main_service.application_storage(), on_did_change_application_storage_emitter.clone());
        Self::register_storage_change_listeners(&base, storage_main_service.application_shared_storage(), on_did_change_application_shared_storage_emitter.clone());

        Rc::new_cyclic(|me| Self {
            base,
            log_service,
            storage_main_service,
            on_did_change_application_storage_emitter,
            on_did_change_application_shared_storage_emitter,
            map_profile_to_on_did_change_profile_storage_emitter: RefCell::new(HashMap::new()),
            me: me.clone(),
        })
    }

    //#region Storage Change Events

    fn register_storage_change_listeners(base: &Disposable, storage: Rc<dyn IStorageMain>, emitter: Rc<Emitter<SerializableItemsChangeEvent>>) {
        // Listen for changes in provided storage to send to listeners that are listening. Use a debouncer to reduce IPC
        // traffic.
        let debounced = storage.on_did_change_storage().debounce(
            |prev: Option<Vec<StorageMainChangeEvent>>, cur: &StorageMainChangeEvent| {
                let mut events = prev.unwrap_or_default();
                events.push(cur.clone());
                events
            },
            DebounceOptions { delay: Delay::Millis(Self::STORAGE_CHANGE_DEBOUNCE_TIME), ..DebounceOptions::default() },
            None,
        );
        base.register(debounced.listen(move |events| {
            if !events.is_empty() {
                emitter.fire(Self::serialize_storage_change_events(events, storage.as_ref()));
            }
        }));
    }

    fn serialize_storage_change_events(events: &[StorageMainChangeEvent], storage: &dyn IStorageMain) -> SerializableItemsChangeEvent {
        let mut changed: IndexMap<String, String> = IndexMap::new();
        let mut deleted: IndexSet<String> = IndexSet::new();
        for event in events {
            match storage.get(&event.key, None) {
                Some(existing) => {
                    changed.insert(event.key.clone(), existing);
                }
                None => {
                    deleted.insert(event.key.clone());
                }
            }
        }

        SerializableItemsChangeEvent { changed: changed.into_iter().collect(), deleted: deleted.into_iter().collect() }
    }

    /// `listen(_, event, arg)`: o único evento é `onDidChangeStorage`.
    pub fn listen(&self, event: &str, arg: &StorageRequest) -> Result<Event<SerializableItemsChangeEvent>, Error> {
        if event != "onDidChangeStorage" {
            return Err(Error::new(format!("Event not found: {event}")));
        }

        // Without profile: application or application-shared scope
        let Some(profile) = &arg.profile else {
            if arg.application_shared {
                return Ok(self.on_did_change_application_shared_storage_emitter.event());
            }

            return Ok(self.on_did_change_application_storage_emitter.event());
        };

        // With profile: profile scope for the profile
        let existing = self.map_profile_to_on_did_change_profile_storage_emitter.borrow().get(&profile.id).cloned();
        let profile_storage_change_emitter = match existing {
            Some(emitter) => emitter,
            None => {
                let emitter = Emitter::new();
                self.base.register(emitter.clone());
                Self::register_storage_change_listeners(&self.base, self.storage_main_service.profile_storage(profile), emitter.clone());
                self.map_profile_to_on_did_change_profile_storage_emitter.borrow_mut().insert(profile.id.clone(), emitter.clone());
                emitter
            }
        };

        Ok(profile_storage_change_emitter.event())
    }

    //#endregion

    /// `call(_, command, arg)`.
    pub fn call(&self, command: &str, arg: StorageRequest) -> Promise<StorageCallResult> {
        let this = self.me.upgrade().expect("o canal está vivo enquanto um método dele roda");
        let command = command.to_string();
        async_fn(async move {
            // Get storage to be ready
            let storage = this.with_storage_initialized(&arg).await?;

            // handle call
            match command.as_str() {
                "getItems" => Ok(StorageCallResult::Items(storage.items().snapshot().iter().map(|(key, value)| (key.clone(), value.clone())).collect())),

                "getValue" => Ok(StorageCallResult::Value(storage.get(&arg.key, None))),

                "getFallbackApplicationStorageItems" => {
                    Ok(StorageCallResult::Items(storage.application_storage_items().map_or_else(Vec::new, |items| items.snapshot().iter().map(|(key, value)| (key.clone(), value.clone())).collect())))
                }

                "updateItems" => {
                    if let Some(insert) = &arg.insert {
                        for (key, value) in insert {
                            let _ = storage.set(key, StorageValue::String(value.clone()));
                        }
                    }

                    for key in arg.delete.iter().flatten() {
                        let _ = storage.delete(key);
                    }

                    Ok(StorageCallResult::Done)
                }

                "compareAndSwap" => {
                    let current_value = storage.get(&arg.key, None);
                    if current_value != arg.expected_value {
                        return Ok(StorageCallResult::CompareAndSwap(CompareAndSwapResult { swapped: false, current_value }));
                    }

                    let _ = storage.set(&arg.key, StorageValue::String(arg.new_value.clone()));
                    Ok(StorageCallResult::CompareAndSwap(CompareAndSwapResult { swapped: true, current_value: Some(arg.new_value.clone()) }))
                }

                "optimize" => {
                    storage.optimize().await?;
                    Ok(StorageCallResult::Done)
                }

                "isUsed" => Ok(StorageCallResult::Used(arg.payload.as_deref().is_some_and(|path| this.storage_main_service.is_used(path)))),

                _ => Err(Error::new(format!("Call not found: {command}"))),
            }
        })
    }

    /// `withStorageInitialized(profile, workspace, applicationShared)`.
    fn with_storage_initialized(&self, request: &StorageRequest) -> Promise<Rc<dyn IStorageMain>> {
        let (storage, scope): (Rc<dyn IStorageMain>, &'static str) = if let Some(workspace) = &request.workspace {
            (self.storage_main_service.workspace_storage(workspace), "workspace")
        } else if let Some(profile) = &request.profile {
            (self.storage_main_service.profile_storage(profile), "profile")
        } else if request.application_shared {
            (self.storage_main_service.application_shared_storage(), "application-shared")
        } else {
            (self.storage_main_service.application_storage(), "application")
        };

        let log_service = self.log_service.clone();
        async_fn(async move {
            if let Err(error) = storage.init().await {
                log_service.error(&format!("StorageIPC#init: Unable to init {scope} storage due to {error}"));
            }

            Ok(storage)
        })
    }
}

impl IDisposable for StorageDatabaseChannel {
    fn dispose(&self) {
        self.base.dispose();
    }
}
