//! Porte de `src/vs/platform/userDataSync/common/userDataProfilesManifestMerge.ts`: o merge de três vias
//! (local, remoto e base) da lista de perfis da sincronização de dados do usuário.
//!
//! Representação:
//! - `IUserDataProfile[] | null` do remoto e da base é `Option<&[...]>`; o `remote` nulo do resultado é
//!   `Option<RemoteResult>` (o `Required<...>` do original só tira a opcionalidade, não o `null`).
//! - `IUserDataProfileInfo`, o subconjunto que `compare` lê dos dois tipos de perfil, é o trait
//!   [`UserDataProfileInfo`].
//! - `objects.equals` sobre `UseDefaultProfileFlags | undefined` é a igualdade estrutural do tipo
//!   (`PartialEq` de `Option<UseDefaultProfileFlags>`): campo ausente só é igual a campo ausente.

use super::user_data_sync::ISyncUserDataProfile;
use crate::user_data_profile::{UseDefaultProfileFlags, UserDataProfile};

/// O lado local do resultado (`IMergeResult.local`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocalResult {
    pub added: Vec<ISyncUserDataProfile>,
    pub removed: Vec<UserDataProfile>,
    pub updated: Vec<ISyncUserDataProfile>,
}

/// O lado remoto do resultado (`IMergeResult.remote`, quando não é `null`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RemoteResult {
    pub added: Vec<UserDataProfile>,
    pub removed: Vec<ISyncUserDataProfile>,
    pub updated: Vec<UserDataProfile>,
}

impl RemoteResult {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.updated.is_empty()
    }
}

/// `IMergeResult`: `remote` é `None` quando não há nada a enviar.
#[derive(Clone, Debug, PartialEq)]
pub struct MergeResult {
    pub local: LocalResult,
    pub remote: Option<RemoteResult>,
}

/// `IUserDataProfileInfo`: o que `compare` lê de um perfil.
pub trait UserDataProfileInfo {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn icon(&self) -> Option<&str>;
    fn use_default_flags(&self) -> Option<UseDefaultProfileFlags>;
}

impl UserDataProfileInfo for UserDataProfile {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn icon(&self) -> Option<&str> {
        self.icon.as_deref()
    }
    fn use_default_flags(&self) -> Option<UseDefaultProfileFlags> {
        self.use_default_flags
    }
}

impl UserDataProfileInfo for ISyncUserDataProfile {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn icon(&self) -> Option<&str> {
        self.icon.as_deref()
    }
    fn use_default_flags(&self) -> Option<UseDefaultProfileFlags> {
        self.use_default_flags
    }
}

fn find_by_id<'a, T: UserDataProfileInfo>(profiles: &'a [T], id: &str) -> &'a T {
    profiles.iter().find(|profile| profile.id() == id).expect("o id vem de `compare` sobre esta mesma lista")
}

/// `merge`.
pub fn merge(
    local: &[UserDataProfile],
    remote: Option<&[ISyncUserDataProfile]>,
    last_sync: Option<&[ISyncUserDataProfile]>,
    ignored: &[String],
) -> MergeResult {
    let mut local_result = LocalResult::default();
    let mut remote_result = Some(RemoteResult::default());

    let Some(remote) = remote else {
        let added: Vec<UserDataProfile> = local.iter().filter(|profile| !ignored.contains(&profile.id)).cloned().collect();
        let remote_result = if added.is_empty() { None } else { Some(RemoteResult { added, ..RemoteResult::default() }) };
        return MergeResult { local: local_result, remote: remote_result };
    };
    let remote_result_ref = remote_result.as_mut().expect("acabou de ser criado");

    let local_to_remote = compare(Some(local), remote, ignored);
    if !local_to_remote.added.is_empty() || !local_to_remote.removed.is_empty() || !local_to_remote.updated.is_empty() {
        let base_to_local = compare(last_sync, local, ignored);
        let base_to_remote = compare(last_sync, remote, ignored);

        // Remotely removed profiles
        for id in &base_to_remote.removed {
            if let Some(e) = local.iter().find(|profile| &profile.id == id) {
                local_result.removed.push(e.clone());
            }
        }

        // Remotely added profiles
        for id in &base_to_remote.added {
            let remote_profile = find_by_id(remote, id);
            // Got added in local
            if base_to_local.added.contains(id) {
                // Is different from local to remote
                if local_to_remote.updated.contains(id) {
                    // Remote wins always
                    local_result.updated.push(remote_profile.clone());
                }
            } else {
                local_result.added.push(remote_profile.clone());
            }
        }

        // Remotely updated profiles
        for id in &base_to_remote.updated {
            // Remote wins always
            local_result.updated.push(find_by_id(remote, id).clone());
        }

        // Locally added profiles
        for id in &base_to_local.added {
            // Not there in remote
            if !base_to_remote.added.contains(id) {
                remote_result_ref.added.push(find_by_id(local, id).clone());
            }
        }

        // Locally updated profiles
        for id in &base_to_local.updated {
            // If removed in remote
            if base_to_remote.removed.contains(id) {
                continue;
            }

            // If not updated in remote
            if !base_to_remote.updated.contains(id) {
                remote_result_ref.updated.push(find_by_id(local, id).clone());
            }
        }

        // Locally removed profiles
        for id in &base_to_local.removed {
            if let Some(removed_profile) = remote.iter().find(|profile| &profile.id == id) {
                remote_result_ref.removed.push(removed_profile.clone());
            }
        }
    }

    if remote_result_ref.is_empty() {
        remote_result = None;
    }

    MergeResult { local: local_result, remote: remote_result }
}

/// O que `compare` devolve: os ids adicionados, removidos e atualizados de `from` para `to`.
struct Comparison {
    added: Vec<String>,
    removed: Vec<String>,
    updated: Vec<String>,
}

fn compare<F: UserDataProfileInfo, T: UserDataProfileInfo>(from: Option<&[F]>, to: &[T], ignored_profiles: &[String]) -> Comparison {
    let is_kept = |id: &str| !ignored_profiles.iter().any(|ignored| ignored == id);
    let from: Vec<&F> = from.map(|profiles| profiles.iter().filter(|profile| is_kept(profile.id())).collect()).unwrap_or_default();
    let to: Vec<&T> = to.iter().filter(|profile| is_kept(profile.id())).collect();
    let from_keys: Vec<String> = from.iter().map(|profile| profile.id().to_string()).collect();
    let to_keys: Vec<String> = to.iter().map(|profile| profile.id().to_string()).collect();
    let added: Vec<String> = to_keys.iter().filter(|key| !from_keys.contains(key)).cloned().collect();
    let removed: Vec<String> = from_keys.iter().filter(|key| !to_keys.contains(key)).cloned().collect();
    let mut updated: Vec<String> = Vec::new();

    for profile in &from {
        let id = profile.id();
        if removed.iter().any(|removed_id| removed_id == id) {
            continue;
        }
        let to_profile = to.iter().find(|p| p.id() == id);
        if to_profile.is_none_or(|to_profile| {
            to_profile.name() != profile.name()
                || to_profile.icon() != profile.icon()
                || to_profile.use_default_flags() != profile.use_default_flags()
        }) {
            updated.push(id.to_string());
        }
    }

    Comparison { added, removed, updated }
}
