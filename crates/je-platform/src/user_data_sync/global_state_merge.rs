//! Porte de `src/vs/platform/userDataSync/common/globalStateMerge.ts`: o merge de três vias do estado
//! global (local, remoto e base) da sincronização de dados do usuário.
//!
//! Representação:
//! - `IStringDictionary<IStorageValue>` é `OrderedMap<JsString, IStorageValue>`: a ordem de inserção do
//!   objeto do JavaScript decide a ordem de `local.removed` e de `remote.added`/`updated`/`removed`, então
//!   um `HashMap` não serve.
//! - `Set<string>` também vira `OrderedMap<JsString, ()>` (ver [`KeySet`]), pelo mesmo motivo.
//! - `remoteStorage`, `baseStorage` e `remote.all` nulos do original são `Option`.
//! - `objects.equals(value1, value2)` sobre dois `IStorageValue` é a igualdade estrutural do tipo
//!   (`PartialEq`), que compara `version`, `value` e `scope` (ausente igual a ausente).
//! - `ILogService` é o [`LogService`](crate::log::LogService), recebido por referência.

use je_base::JsString;
use je_base::map::OrderedMap;

use super::user_data_sync::{IStorageValue, SYNC_SERVICE_URL_TYPE};
use crate::log::LogService;

/// `IStringDictionary<IStorageValue>`.
pub type StorageDictionary = OrderedMap<JsString, IStorageValue>;

/// `Set<string>` com a ordem de inserção.
type KeySet = OrderedMap<JsString, ()>;

/// O lado local do resultado (`IMergeResult.local`).
#[derive(Clone, Debug, Default)]
pub struct LocalResult {
    pub added: StorageDictionary,
    pub removed: Vec<JsString>,
    pub updated: StorageDictionary,
}

/// O lado remoto do resultado (`IMergeResult.remote`).
#[derive(Clone, Debug, Default)]
pub struct RemoteResult {
    pub added: Vec<JsString>,
    pub removed: Vec<JsString>,
    pub updated: Vec<JsString>,
    pub all: Option<StorageDictionary>,
}

/// `IMergeResult`.
#[derive(Clone, Debug, Default)]
pub struct MergeResult {
    pub local: LocalResult,
    pub remote: RemoteResult,
}

/// O `{ machine: ReadonlyArray<string>; unregistered: ReadonlyArray<string> }` do parâmetro `storageKeys`.
#[derive(Clone, Copy, Debug)]
pub struct StorageKeys<'a> {
    pub machine: &'a [JsString],
    pub unregistered: &'a [JsString],
}

/// O que `compare` devolve: as chaves adicionadas, removidas e atualizadas de `from` para `to`.
struct Comparison {
    added: KeySet,
    removed: KeySet,
    updated: KeySet,
}

impl Comparison {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.updated.is_empty()
    }
}

fn keys_of(set: &KeySet) -> Vec<JsString> {
    set.keys().cloned().collect()
}

/// O `Object.keys(storage).reduce((r, k) => { r.add(k); return r; }, new Set<string>())` do original.
fn key_set_of(storage: &StorageDictionary) -> KeySet {
    let mut set = KeySet::new();
    for key in storage.keys() {
        set.insert(key.clone(), ());
    }
    set
}

/// `merge`.
pub fn merge(
    local_storage: &StorageDictionary,
    remote_storage: Option<&StorageDictionary>,
    base_storage: Option<&StorageDictionary>,
    storage_keys: StorageKeys<'_>,
    log_service: &dyn LogService,
) -> MergeResult {
    let Some(remote_storage) = remote_storage else {
        return MergeResult {
            remote: RemoteResult {
                added: keys_of(&key_set_of(local_storage)),
                removed: Vec::new(),
                updated: Vec::new(),
                all: if local_storage.is_empty() { None } else { Some(local_storage.clone()) },
            },
            local: LocalResult::default(),
        };
    };

    let local_to_remote = compare(local_storage, remote_storage);
    if local_to_remote.is_empty() {
        // No changes found between local and remote.
        return MergeResult::default();
    }

    let base_to_remote = match base_storage {
        Some(base) => compare(base, remote_storage),
        None => Comparison { added: key_set_of(remote_storage), removed: KeySet::new(), updated: KeySet::new() },
    };
    let base_to_local = match base_storage {
        Some(base) => compare(base, local_storage),
        None => Comparison { added: key_set_of(local_storage), removed: KeySet::new(), updated: KeySet::new() },
    };

    let mut local = LocalResult::default();
    let mut remote: StorageDictionary = remote_storage.clone();

    let is_first_time_sync = base_storage.is_none();
    let sync_service_url_type = JsString::from(SYNC_SERVICE_URL_TYPE);

    // Added in local
    for key in base_to_local.added.keys() {
        // If syncing for first time remote value gets precedence always,
        // except for sync service type key - local value takes precedence for this key
        if *key != sync_service_url_type && is_first_time_sync && base_to_remote.added.contains_key(key) {
            continue;
        }

        if let Some(value) = local_storage.get(key) {
            remote.insert(key.clone(), value.clone());
        }
    }

    // Updated in local
    for key in base_to_local.updated.keys() {
        if let Some(value) = local_storage.get(key) {
            remote.insert(key.clone(), value.clone());
        }
    }

    // Removed in local
    for key in base_to_local.removed.keys() {
        // Do not remove from remote if key is not registered.
        if storage_keys.unregistered.contains(key) {
            continue;
        }
        remote.remove(key);
    }

    // Added in remote
    for key in base_to_remote.added.keys() {
        let Some(remote_value) = remote_storage.get(key) else {
            continue;
        };
        if storage_keys.machine.contains(key) {
            log_service.info(&format!(
                "GlobalState: Skipped adding {} in local storage because it is declared as machine scoped.",
                key.to_string_lossy()
            ));
            continue;
        }
        // Skip if the value is also added in local from the time it is last synced
        if base_storage.is_some() && base_to_local.added.contains_key(key) {
            continue;
        }
        let local_value = local_storage.get(key);
        if local_value.is_some_and(|local_value| local_value.value == remote_value.value) {
            continue;
        }

        // Local sync service type value takes precedence if syncing for first time
        if *key == sync_service_url_type && is_first_time_sync && base_to_local.added.contains_key(key) {
            continue;
        }

        if local_value.is_some() {
            local.updated.insert(key.clone(), remote_value.clone());
        } else {
            local.added.insert(key.clone(), remote_value.clone());
        }
    }

    // Updated in Remote
    for key in base_to_remote.updated.keys() {
        let Some(remote_value) = remote_storage.get(key) else {
            continue;
        };
        if storage_keys.machine.contains(key) {
            log_service.info(&format!(
                "GlobalState: Skipped updating {} in local storage because it is declared as machine scoped.",
                key.to_string_lossy()
            ));
            continue;
        }
        // Skip if the value is also updated or removed in local
        if base_to_local.updated.contains_key(key) || base_to_local.removed.contains_key(key) {
            continue;
        }
        let local_value = local_storage.get(key);
        if local_value.is_some_and(|local_value| local_value.value == remote_value.value) {
            continue;
        }
        local.updated.insert(key.clone(), remote_value.clone());
    }

    // Removed in remote
    for key in base_to_remote.removed.keys() {
        if storage_keys.machine.contains(key) {
            log_service.trace(&format!(
                "GlobalState: Skipped removing {} in local storage because it is declared as machine scoped.",
                key.to_string_lossy()
            ));
            continue;
        }
        // Skip if the value is also updated or removed in local
        if base_to_local.updated.contains_key(key) || base_to_local.removed.contains_key(key) {
            continue;
        }
        local.removed.push(key.clone());
    }

    let result = compare(remote_storage, &remote);
    let unchanged = result.is_empty();
    MergeResult {
        local,
        remote: RemoteResult {
            added: keys_of(&result.added),
            updated: keys_of(&result.updated),
            removed: keys_of(&result.removed),
            all: if unchanged { None } else { Some(remote) },
        },
    }
}

fn compare<V: PartialEq>(from: &OrderedMap<JsString, V>, to: &OrderedMap<JsString, V>) -> Comparison {
    let mut added = KeySet::new();
    let mut removed = KeySet::new();
    let mut updated = KeySet::new();

    for key in to.keys() {
        if !from.contains_key(key) {
            added.insert(key.clone(), ());
        }
    }
    for key in from.keys() {
        if !to.contains_key(key) {
            removed.insert(key.clone(), ());
        }
    }

    for (key, value1) in from.iter() {
        if removed.contains_key(key) {
            continue;
        }
        if to.get(key).is_some_and(|value2| value1 != value2) {
            updated.insert(key.clone(), ());
        }
    }

    Comparison { added, removed, updated }
}
