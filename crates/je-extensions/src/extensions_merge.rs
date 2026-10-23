//! Porte de `src/vs/platform/userDataSync/common/extensionsMerge.ts`: o merge de três vias (local, remoto e base)
//! das extensões na sincronização de dados do usuário.
//!
//! Por que este módulo mora no `je-extensions` e não no `je-platform`: o merge depende de `IExtensionIdentifier`
//! ([`ExtensionIdentifierInfo`]), que é daqui, e o `je-extensions` já depende do `je-platform`, então o caminho
//! inverso criaria um ciclo. Os tipos `ISyncExtension`, `ILocalSyncExtension` e `IRemoteSyncExtension` de
//! `userDataSync.ts` são portados aqui também, pelo mesmo motivo.
//!
//! Representação:
//! - `ISyncExtension` é a união `ILocalSyncExtension | IRemoteSyncExtension`. Os três tipos têm o mesmo formato
//!   quando todos os campos opcionais são lidos como `Option` (o local exige `pinned` e `preRelease`, o remoto os
//!   aceita ausentes), então há uma só struct, [`SyncExtension`], e os outros dois nomes são apelidos dela.
//! - Uma propriedade ausente do objeto do JavaScript é `None`. O `massageOutgoingExtension` apaga as chaves
//!   falsas com `delete`, o que aqui é voltar o campo para `None`.
//! - `IStringDictionary<any>` do `state` é [`ExtensionState`], um `IndexMap<String, serde_json::Value>`: a ordem
//!   de inserção do objeto decide a ordem das chaves do estado mesclado. `objects.equals` sobre dois valores é a
//!   igualdade estrutural de `Value` (a ordem das chaves de um objeto não conta, como no original).
//! - `Map<string, ISyncExtension>` é `IndexMap` (o `set` de uma chave existente mantém a posição e o `delete`
//!   preserva a ordem dos demais) e `Set<string>` é `IndexSet`, porque a ordem de inserção decide a ordem dos
//!   resultados.
//! - `assertReturnsDefined` lança no original quando o valor é indefinido; aqui entra em pânico, pelo mesmo
//!   motivo (invariante do algoritmo).
//! - `semver.gt` do original lança para versão inválida; o `je_base::semver::gt` devolve `false`.

use std::collections::{HashMap, HashSet};

use indexmap::{IndexMap, IndexSet};
use je_base::semver;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::manifest::ExtensionIdentifierInfo;

/// `IStringDictionary<any>` do `state` de uma extensão.
pub type ExtensionState = IndexMap<String, Value>;

/// `ISyncExtension`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncExtension {
    pub identifier: ExtensionIdentifierInfo,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_release: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_application_scoped: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ExtensionState>,
}

/// `ILocalSyncExtension` (a mesma forma de [`SyncExtension`], ver o doc do módulo).
pub type LocalSyncExtension = SyncExtension;

/// `IRemoteSyncExtension` (a mesma forma de [`SyncExtension`], ver o doc do módulo).
pub type RemoteSyncExtension = SyncExtension;

/// O lado local do resultado (`IMergeResult.local`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocalMergeResult {
    pub added: Vec<SyncExtension>,
    pub removed: Vec<ExtensionIdentifierInfo>,
    pub updated: Vec<SyncExtension>,
}

/// O lado remoto do resultado (`IMergeResult.remote`, quando não é `null`).
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteMergeResult {
    pub added: Vec<SyncExtension>,
    pub removed: Vec<SyncExtension>,
    pub updated: Vec<SyncExtension>,
    pub all: Vec<SyncExtension>,
}

/// `IMergeResult`.
#[derive(Clone, Debug, PartialEq)]
pub struct MergeResult {
    pub local: LocalMergeResult,
    pub remote: Option<RemoteMergeResult>,
}

type ExtensionMap = IndexMap<String, SyncExtension>;
type KeySet = IndexSet<String>;

/// O que `compare` devolve: as chaves adicionadas, removidas e atualizadas de `from` para `to`.
struct Comparison {
    added: KeySet,
    removed: KeySet,
    updated: KeySet,
}

/// Verdade do JavaScript para um `boolean | undefined`.
fn truthy(value: Option<bool>) -> bool {
    value.unwrap_or(false)
}

/// `assertReturnsDefined`.
fn defined<T>(value: Option<T>) -> T {
    value.expect("assertReturnsDefined: valor indefinido")
}

/// `uuid ? `uuid:${uuid}` : `id:${id.toLowerCase()}``.
fn uuid_key(uuid: Option<&str>, id: &str) -> String {
    match uuid.filter(|uuid| !uuid.is_empty()) {
        Some(uuid) => format!("uuid:{uuid}"),
        None => format!("id:{}", id.to_lowercase()),
    }
}

/// O `getKey` do original: o uuid do próprio identificador ou o conhecido para o mesmo id.
fn key_of(uuids: &HashMap<String, String>, identifier: &ExtensionIdentifierInfo) -> String {
    let own = identifier.uuid.as_deref().filter(|uuid| !uuid.is_empty());
    let uuid = own.or_else(|| uuids.get(&identifier.id.to_lowercase()).map(String::as_str));
    uuid_key(uuid, &identifier.id)
}

fn add_extension_to_map(uuids: &HashMap<String, String>, map: &mut ExtensionMap, extension: &SyncExtension) {
    map.insert(key_of(uuids, &extension.identifier), extension.clone());
}

fn build_map(uuids: &HashMap<String, String>, extensions: &[SyncExtension]) -> ExtensionMap {
    let mut map = ExtensionMap::new();
    for extension in extensions {
        add_extension_to_map(uuids, &mut map, extension);
    }
    map
}

/// `merge`.
pub fn merge(
    local_extensions: &[LocalSyncExtension],
    remote_extensions: Option<&[RemoteSyncExtension]>,
    last_sync_extensions: Option<&[RemoteSyncExtension]>,
    skipped_extensions: &[SyncExtension],
    ignored_extensions: &[String],
    last_sync_builtin_extensions: Option<&[ExtensionIdentifierInfo]>,
) -> MergeResult {
    let mut added: Vec<SyncExtension> = Vec::new();
    let mut removed: Vec<ExtensionIdentifierInfo> = Vec::new();
    let mut updated: Vec<SyncExtension> = Vec::new();

    let Some(remote_extensions) = remote_extensions else {
        let remote: Vec<SyncExtension> = local_extensions
            .iter()
            .filter(|extension| {
                ignored_extensions.iter().all(|id| id.to_lowercase() != extension.identifier.id.to_lowercase())
            })
            .cloned()
            .collect();
        return MergeResult {
            local: LocalMergeResult { added, removed, updated },
            remote: if remote.is_empty() {
                None
            } else {
                Some(RemoteMergeResult { added: remote.clone(), updated: Vec::new(), removed: Vec::new(), all: remote })
            },
        };
    };

    let local_extensions: Vec<SyncExtension> = local_extensions.iter().map(massage_incoming_extension).collect();
    let remote_extensions: Vec<SyncExtension> = remote_extensions.iter().map(massage_incoming_extension).collect();
    let last_sync_extensions: Option<Vec<SyncExtension>> =
        last_sync_extensions.map(|extensions| extensions.iter().map(massage_incoming_extension).collect());

    let mut uuids: HashMap<String, String> = HashMap::new();
    let mut add_uuid = |identifier: &ExtensionIdentifierInfo| {
        if let Some(uuid) = identifier.uuid.as_deref().filter(|uuid| !uuid.is_empty()) {
            uuids.insert(identifier.id.to_lowercase(), uuid.to_owned());
        }
    };
    local_extensions.iter().for_each(|extension| add_uuid(&extension.identifier));
    remote_extensions.iter().for_each(|extension| add_uuid(&extension.identifier));
    if let Some(extensions) = &last_sync_extensions {
        extensions.iter().for_each(|extension| add_uuid(&extension.identifier));
    }
    skipped_extensions.iter().for_each(|extension| add_uuid(&extension.identifier));
    if let Some(identifiers) = last_sync_builtin_extensions {
        identifiers.iter().for_each(&mut add_uuid);
    }

    let local_extensions_map = build_map(&uuids, &local_extensions);
    let remote_extensions_map = build_map(&uuids, &remote_extensions);
    // `deepClone` de cada extensão remota, na mesma ordem.
    let mut new_remote_extensions_map = remote_extensions_map.clone();
    let last_sync_extensions_map: Option<ExtensionMap> =
        last_sync_extensions.as_ref().map(|extensions| build_map(&uuids, extensions));
    let skipped_extensions_map = build_map(&uuids, skipped_extensions);
    let ignored_extensions_set: HashSet<String> = ignored_extensions
        .iter()
        .map(|id| uuid_key(uuids.get(&id.to_lowercase()).map(String::as_str), id))
        .collect();
    let last_sync_builtin_extensions_set: Option<HashSet<String>> = last_sync_builtin_extensions.map(|identifiers| {
        identifiers
            .iter()
            .map(|identifier| {
                let uuid = match identifier.uuid.as_deref() {
                    Some(uuid) => Some(uuid),
                    None => uuids.get(&identifier.id.to_lowercase()).map(String::as_str),
                };
                uuid_key(uuid, &identifier.id)
            })
            .collect()
    });
    let last_sync_map = last_sync_extensions_map.as_ref();

    let local_to_remote = compare(Some(&local_extensions_map), &remote_extensions_map, &ignored_extensions_set, false);
    if !local_to_remote.added.is_empty() || !local_to_remote.removed.is_empty() || !local_to_remote.updated.is_empty()
    {
        let base_to_local = compare(last_sync_map, &local_extensions_map, &ignored_extensions_set, false);
        let base_to_remote = compare(last_sync_map, &remote_extensions_map, &ignored_extensions_set, true);

        let merge_extension = |key: &str,
                               local_extension: &SyncExtension,
                               remote_extension: &SyncExtension,
                               preferred: &SyncExtension|
         -> SyncExtension {
            let mut pinned: Option<bool>;
            let mut version: Option<String> = None;
            let mut pre_release: Option<bool>;
            if truthy(local_extension.installed) {
                pinned = preferred.pinned;
                pre_release = preferred.pre_release;
                if truthy(pinned) {
                    version = Some(preferred.version.clone());
                }
            } else {
                pinned = remote_extension.pinned;
                pre_release = remote_extension.pre_release;
                if truthy(pinned) {
                    version = Some(remote_extension.version.clone());
                }
            }
            if pinned.is_none() {
                // from older client
                pinned = local_extension.pinned;
                if truthy(pinned) {
                    version = Some(local_extension.version.clone());
                }
            }
            if pre_release.is_none() {
                // from older client
                pre_release = local_extension.pre_release;
            }
            let version = version.unwrap_or_else(|| {
                if !remote_extension.version.is_empty()
                    && (!truthy(local_extension.installed)
                        || semver::gt(&remote_extension.version, &local_extension.version))
                {
                    remote_extension.version.clone()
                } else {
                    local_extension.version.clone()
                }
            });
            SyncExtension {
                installed: Some(truthy(local_extension.installed) || truthy(remote_extension.installed)),
                pinned,
                pre_release,
                version,
                state: merge_extension_state(local_extension, remote_extension, last_sync_map.and_then(|m| m.get(key))),
                ..preferred.clone()
            }
        };

        let was_an_installed_extension_during_last_sync = |key: &str, base_extension: &SyncExtension| -> bool {
            last_sync_builtin_extensions_set.as_ref().is_some_and(|set| !set.contains(key))
                && truthy(base_extension.installed)
        };

        // Remotely removed extension => exist in base and does not in remote
        for key in &base_to_remote.removed {
            let Some(local_extension) = local_extensions_map.get(key) else {
                continue;
            };

            let base_extension = defined(last_sync_map.and_then(|m| m.get(key)));
            if truthy(local_extension.installed) && was_an_installed_extension_during_last_sync(key, base_extension) {
                // Installed extension is removed from remote. Remove it from local.
                removed.push(local_extension.identifier.clone());
            } else {
                // Add to remote: It is a builtin extenision or got installed after last sync
                new_remote_extensions_map.insert(key.clone(), local_extension.clone());
            }
        }

        // Remotely added extension => does not exist in base and exist in remote
        for key in &base_to_remote.added {
            let remote_extension = defined(remote_extensions_map.get(key));
            let local_extension = local_extensions_map.get(key);

            if let Some(local_extension) = local_extension {
                // Also exist in local: is different from local to remote
                if local_to_remote.updated.contains(key) {
                    let merged_extension = merge_extension(key, local_extension, remote_extension, remote_extension);
                    // Update locally only when the extension has changes in properties other than installed poperty
                    if !are_same(local_extension, remote_extension, false, false) {
                        updated.push(massage_outgoing_extension(&merged_extension, key));
                    }
                    new_remote_extensions_map.insert(key.clone(), merged_extension);
                }
            } else if truthy(remote_extension.installed) {
                // Add only if the extension is an installed extension
                added.push(massage_outgoing_extension(remote_extension, key));
            }
        }

        // Remotely updated extension => exist in base and remote
        for key in &base_to_remote.updated {
            let remote_extension = defined(remote_extensions_map.get(key));
            let base_extension = defined(last_sync_map.and_then(|m| m.get(key)));
            let local_extension = local_extensions_map.get(key);

            if let Some(local_extension) = local_extension {
                // Also exist in local
                if was_an_installed_extension_during_last_sync(key, base_extension)
                    && truthy(local_extension.installed)
                    && !truthy(remote_extension.installed)
                {
                    // Remove it locally if it is installed locally and not remotely
                    removed.push(local_extension.identifier.clone());
                } else {
                    // Update in local always
                    let merged_extension = merge_extension(key, local_extension, remote_extension, remote_extension);
                    updated.push(massage_outgoing_extension(&merged_extension, key));
                    new_remote_extensions_map.insert(key.clone(), merged_extension);
                }
            } else if truthy(remote_extension.installed) {
                // Add it locally if does not exist locally and installed remotely
                added.push(massage_outgoing_extension(remote_extension, key));
            }
        }

        // Locally added extension => does not exist in base and exist in local
        for key in &base_to_local.added {
            // If added in remote (already handled)
            if base_to_remote.added.contains(key) {
                continue;
            }
            new_remote_extensions_map.insert(key.clone(), defined(local_extensions_map.get(key)).clone());
        }

        // Locally updated extension => exist in base and local
        for key in &base_to_local.updated {
            // If removed in remote (already handled)
            if base_to_remote.removed.contains(key) {
                continue;
            }
            // If updated in remote (already handled)
            if base_to_remote.updated.contains(key) {
                continue;
            }
            let local_extension = defined(local_extensions_map.get(key));
            let remote_extension = defined(remote_extensions_map.get(key));
            // Update remotely
            new_remote_extensions_map
                .insert(key.clone(), merge_extension(key, local_extension, remote_extension, local_extension));
        }

        // Locally removed extensions => exist in base and does not exist in local
        for key in &base_to_local.removed {
            // If updated in remote (already handled)
            if base_to_remote.updated.contains(key) {
                continue;
            }
            // If removed in remote (already handled)
            if base_to_remote.removed.contains(key) {
                continue;
            }
            // Skipped
            if skipped_extensions_map.contains_key(key) {
                continue;
            }
            // Skip if it is a builtin extension
            if !truthy(defined(remote_extensions_map.get(key)).installed) {
                continue;
            }
            // Skip if last sync builtin extensions set is not available
            let Some(last_sync_builtin_extensions_set) = &last_sync_builtin_extensions_set else {
                continue;
            };
            // Skip if it was a builtin extension during last sync
            if last_sync_builtin_extensions_set.contains(key)
                || !truthy(defined(last_sync_map.and_then(|m| m.get(key))).installed)
            {
                continue;
            }
            new_remote_extensions_map.shift_remove(key);
        }
    }

    let remote_changes = compare(Some(&remote_extensions_map), &new_remote_extensions_map, &HashSet::new(), true);
    let has_remote_changes =
        !remote_changes.added.is_empty() || !remote_changes.updated.is_empty() || !remote_changes.removed.is_empty();
    let remote = if has_remote_changes {
        let all: Vec<SyncExtension> = new_remote_extensions_map
            .iter()
            .map(|(key, value)| massage_outgoing_extension(value, key))
            .collect();
        Some(RemoteMergeResult {
            added: remote_changes.added.iter().map(|id| defined(new_remote_extensions_map.get(id)).clone()).collect(),
            updated: remote_changes
                .updated
                .iter()
                .map(|id| defined(new_remote_extensions_map.get(id)).clone())
                .collect(),
            removed: remote_changes.removed.iter().map(|id| defined(remote_extensions_map.get(id)).clone()).collect(),
            all,
        })
    } else {
        None
    };

    MergeResult { local: LocalMergeResult { added, removed, updated }, remote }
}

fn compare(
    from: Option<&ExtensionMap>,
    to: &ExtensionMap,
    ignored_extensions: &HashSet<String>,
    check_version_property: bool,
) -> Comparison {
    let from_keys: KeySet = match from {
        Some(from) => from.keys().filter(|key| !ignored_extensions.contains(*key)).cloned().collect(),
        None => KeySet::new(),
    };
    let to_keys: KeySet = to.keys().filter(|key| !ignored_extensions.contains(*key)).cloned().collect();
    let added: KeySet = to_keys.iter().filter(|key| !from_keys.contains(*key)).cloned().collect();
    let removed: KeySet = from_keys.iter().filter(|key| !to_keys.contains(*key)).cloned().collect();
    let mut updated = KeySet::new();

    if let Some(from) = from {
        for key in &from_keys {
            if removed.contains(key) {
                continue;
            }
            let from_extension = defined(from.get(key));
            let to_extension = to.get(key);
            if to_extension.is_none_or(|to_extension| !are_same(from_extension, to_extension, check_version_property, true))
            {
                updated.insert(key.clone());
            }
        }
    }

    Comparison { added, removed, updated }
}

fn are_same(
    from_extension: &SyncExtension,
    to_extension: &SyncExtension,
    check_version_property: bool,
    check_installed_property: bool,
) -> bool {
    if from_extension.disabled != to_extension.disabled {
        /* extension enablement changed */
        return false;
    }

    if truthy(from_extension.is_application_scoped) != truthy(to_extension.is_application_scoped) {
        /* extension application scope has changed */
        return false;
    }

    if check_installed_property && from_extension.installed != to_extension.installed {
        /* extension installed property changed */
        return false;
    }

    if truthy(from_extension.installed) && truthy(to_extension.installed) {
        if from_extension.pre_release != to_extension.pre_release {
            /* installed extension's pre-release version changed */
            return false;
        }

        if from_extension.pinned != to_extension.pinned {
            /* installed extension's pinning changed */
            return false;
        }

        if truthy(to_extension.pinned) && from_extension.version != to_extension.version {
            /* installed extension's pinned version changed */
            return false;
        }
    }

    if !is_same_extension_state(from_extension.state.as_ref(), to_extension.state.as_ref()) {
        /* extension state changed */
        return false;
    }

    if check_version_property && from_extension.version != to_extension.version {
        /* extension version changed */
        return false;
    }

    true
}

fn merge_extension_state(
    local_extension: &SyncExtension,
    remote_extension: &SyncExtension,
    last_sync_extension: Option<&SyncExtension>,
) -> Option<ExtensionState> {
    let local_state = local_extension.state.as_ref();
    let remote_state = remote_extension.state.as_ref();
    let base_state = last_sync_extension.and_then(|extension| extension.state.as_ref());

    // If remote extension has no version, use local state
    if remote_extension.version.is_empty() {
        return local_state.cloned();
    }

    // If local state exists and local extension is latest then use local state
    if local_state.is_some() && semver::gt(&local_extension.version, &remote_extension.version) {
        return local_state.cloned();
    }
    // If remote state exists and remote extension is latest, use remote state
    if remote_state.is_some() && semver::gt(&remote_extension.version, &local_extension.version) {
        return remote_state.cloned();
    }

    /* Remote and local are on same version */

    // If local state is not yet set, use remote state
    let Some(local_state) = local_state else {
        return remote_state.cloned();
    };
    // If remote state is not yet set, use local state
    let Some(remote_state) = remote_state else {
        return Some(local_state.clone());
    };

    let mut merged_state: ExtensionState = local_state.clone();
    let base_to_remote = match base_state {
        Some(base_state) => compare_extension_state(base_state, remote_state),
        None => Comparison {
            added: remote_state.keys().cloned().collect(),
            removed: KeySet::new(),
            updated: KeySet::new(),
        },
    };
    let base_to_local = match base_state {
        Some(base_state) => compare_extension_state(base_state, local_state),
        None => Comparison {
            added: local_state.keys().cloned().collect(),
            removed: KeySet::new(),
            updated: KeySet::new(),
        },
    };
    // Added/Updated in remote
    for key in base_to_remote.added.iter().chain(base_to_remote.updated.iter()) {
        if let Some(value) = remote_state.get(key) {
            merged_state.insert(key.clone(), value.clone());
        }
    }
    // Removed in remote
    for key in &base_to_remote.removed {
        // Not updated in local
        if !base_to_local.updated.contains(key) {
            merged_state.shift_remove(key);
        }
    }
    Some(merged_state)
}

fn compare_extension_state(from: &ExtensionState, to: &ExtensionState) -> Comparison {
    let added: KeySet = to.keys().filter(|key| !from.contains_key(*key)).cloned().collect();
    let removed: KeySet = from.keys().filter(|key| !to.contains_key(*key)).cloned().collect();
    let mut updated = KeySet::new();

    for (key, value1) in from {
        if removed.contains(key) {
            continue;
        }
        if to.get(key).is_some_and(|value2| value1 != value2) {
            updated.insert(key.clone());
        }
    }

    Comparison { added, removed, updated }
}

/// `isSameExtensionState(a = {}, b = {})`.
fn is_same_extension_state(a: Option<&ExtensionState>, b: Option<&ExtensionState>) -> bool {
    let empty = ExtensionState::new();
    let Comparison { added, removed, updated } = compare_extension_state(a.unwrap_or(&empty), b.unwrap_or(&empty));
    added.is_empty() && removed.is_empty() && updated.is_empty()
}

/// massage incoming extension - add optional properties
fn massage_incoming_extension(extension: &SyncExtension) -> SyncExtension {
    SyncExtension {
        disabled: Some(truthy(extension.disabled)),
        installed: Some(truthy(extension.installed)),
        ..extension.clone()
    }
}

/// massage outgoing extension - remove optional properties
fn massage_outgoing_extension(extension: &SyncExtension, key: &str) -> SyncExtension {
    SyncExtension {
        identifier: ExtensionIdentifierInfo {
            id: extension.identifier.id.clone(),
            uuid: key.strip_prefix("uuid:").map(str::to_owned),
        },
        /* set following always so that to differentiate with older clients */
        pre_release: Some(truthy(extension.pre_release)),
        pinned: Some(truthy(extension.pinned)),
        disabled: if truthy(extension.disabled) { extension.disabled } else { None },
        installed: if truthy(extension.installed) { extension.installed } else { None },
        state: extension.state.clone(),
        is_application_scoped: if truthy(extension.is_application_scoped) { extension.is_application_scoped } else { None },
        version: extension.version.clone(),
    }
}
