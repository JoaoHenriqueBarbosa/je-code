//! Porte de `src/vs/platform/userDataSync/common/userDataProfilesManifestSync.ts`: o sincronizador da lista de
//! perfis (o `profiles.json` do servidor), sobre o `merge` de `userDataProfilesManifestMerge`.
//!
//! Decisões de tradução (as da base estão em [`super::abstract_synchronizer`]):
//! - `IUserDataProfilesManifestResourcePreview` é um [`IResourcePreview`] com [`ManifestPreviewExtra`]
//!   (`previewResult` e `remoteProfiles`); o `IUserDataProfileManifestResourceMergeResult` é um
//!   [`IAcceptResult`] com [`ManifestMergeExtra`] (`local` e `remote`).
//! - `JSON.parse`/`JSON.stringify` de `ISyncUserDataProfile` passam por [`ISyncUserDataProfile::from_json`] e
//!   [`ISyncUserDataProfile::to_json`], que escrevem as chaves na ordem `id`, `name`, `collection`, `icon`,
//!   `useDefaultFlags` (a de quem cria o perfil no original); o que o servidor devolve em outra ordem sai
//!   normalizado nessa.
//! - `remoteProfiles.splice(remoteProfiles.findIndex(...), 1)` com perfil ausente (`findIndex` dá `-1`) tira o
//!   último elemento, como o `splice(-1, 1)` do JavaScript.
//! - `Promise.all(list.map(async ...))` inicia todas as operações já (cada `async_fn` roda até o primeiro
//!   `await`) e espera todas, com [`Promise::all`].
//! - `localeCompare` é o `locale_compare` do `je-base`.

use std::rc::Rc;

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::collator::locale_compare;
use je_base::errors::Error;
use je_base::json::{Object, Value};
use je_base::json_formatter::{FormattingOptions, to_formatted_string};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;

use super::abstract_synchronizer::{AbstractSynchroniser, IAcceptResult, IMergeResult, IResourcePreview, SynchroniserImpl, SynchroniserServices, parse_json, stringify_json, with_sync_authority};
use super::user_data_profiles_manifest_merge::{LocalResult, MergeResult, RemoteResult, merge};
use super::user_data_sync::{
    Change, IRemoteUserData, ISyncData, ISyncUserDataProfile, IUserDataSyncConfiguration, IUserDataSyncResource, RefOrUserData, SyncResource, UserDataSyncError, UserDataSyncErrorCode,
};
use crate::user_data_profile::{IUserDataProfilesService, UseDefaultProfileFlags, UserDataProfile, UserDataProfileOptions, UserDataProfileUpdateOptions};

/// O máximo de perfis que o servidor guarda.
const MAX_PROFILES: usize = 20;

/// `IUserDataProfileManifestResourceMergeResult` sem a parte de `IAcceptResult`: o resultado do `merge`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ManifestMergeExtra {
    pub local: LocalResult,
    pub remote: Option<RemoteResult>,
}

/// O que `IUserDataProfilesManifestResourcePreview` acrescenta a `IResourcePreview`.
#[derive(Clone, Debug, PartialEq)]
pub struct ManifestPreviewExtra {
    pub preview_result: IAcceptResult<ManifestMergeExtra>,
    pub remote_profiles: Option<Vec<ISyncUserDataProfile>>,
}

/// `IUserDataProfilesManifestResourcePreview`.
pub type IUserDataProfilesManifestResourcePreview = IResourcePreview<ManifestPreviewExtra>;

/// `IUserDataProfileManifestResourceMergeResult`.
pub type IUserDataProfileManifestResourceMergeResult = IAcceptResult<ManifestMergeExtra>;

// region: JSON dos perfis

fn flags_to_json(flags: UseDefaultProfileFlags) -> Value {
    let mut object = Object::new();
    for (name, value) in [
        ("settings", flags.settings),
        ("keybindings", flags.keybindings),
        ("snippets", flags.snippets),
        ("prompts", flags.prompts),
        ("tasks", flags.tasks),
        ("extensions", flags.extensions),
        ("globalState", flags.global_state),
        ("mcp", flags.mcp),
        ("languageModels", flags.language_models),
    ] {
        if let Some(value) = value {
            object.insert(JsString::from(name), Value::Bool(value));
        }
    }
    Value::Object(object)
}

fn flags_from_json(value: &Value) -> Option<UseDefaultProfileFlags> {
    let Value::Object(object) = value else { return None };
    let flag = |name: &str| match object.get(name) {
        Some(Value::Bool(value)) => Some(*value),
        _ => None,
    };
    Some(UseDefaultProfileFlags {
        settings: flag("settings"),
        keybindings: flag("keybindings"),
        snippets: flag("snippets"),
        prompts: flag("prompts"),
        tasks: flag("tasks"),
        extensions: flag("extensions"),
        global_state: flag("globalState"),
        mcp: flag("mcp"),
        language_models: flag("languageModels"),
    })
}

impl ISyncUserDataProfile {
    /// `{ id, name, collection, icon?, useDefaultFlags? }`.
    pub fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert(JsString::from("id"), Value::String(JsString::from(self.id.as_str())));
        object.insert(JsString::from("name"), Value::String(JsString::from(self.name.as_str())));
        object.insert(JsString::from("collection"), Value::String(JsString::from(self.collection.as_str())));
        if let Some(icon) = &self.icon {
            object.insert(JsString::from("icon"), Value::String(JsString::from(icon.as_str())));
        }
        if let Some(flags) = self.use_default_flags {
            object.insert(JsString::from("useDefaultFlags"), flags_to_json(flags));
        }
        Value::Object(object)
    }

    /// O perfil que o valor descreve: `id`, `name` e `collection` são obrigatórios.
    pub fn from_json(value: &Value) -> Option<Self> {
        let Value::Object(object) = value else { return None };
        let text = |name: &str| match object.get(name) {
            Some(Value::String(text)) => Some(text.to_string_lossy()),
            _ => None,
        };
        Some(Self { id: text("id")?, collection: text("collection")?, name: text("name")?, icon: text("icon"), use_default_flags: object.get("useDefaultFlags").and_then(flags_from_json) })
    }
}

/// `JSON.parse(content)` de uma lista de perfis.
fn parse_profiles(content: &str) -> Result<Vec<ISyncUserDataProfile>, Error> {
    let Value::Array(items) = parse_json(content)? else { return Err(Error::type_error("profiles.forEach is not a function")) };
    items.iter().map(|item| ISyncUserDataProfile::from_json(item).ok_or_else(|| Error::type_error("Invalid profile in the user data profiles manifest"))).collect()
}

/// `parseUserDataProfilesManifest`.
pub fn parse_user_data_profiles_manifest(sync_data: &ISyncData) -> Result<Vec<ISyncUserDataProfile>, Error> {
    parse_profiles(&sync_data.content)
}

/// `stringifyLocalProfiles`: `{ id, name }` ordenados por nome, compactos ou formatados.
pub fn stringify_local_profiles(profiles: &[UserDataProfile], format: bool) -> String {
    let mut sorted: Vec<&UserDataProfile> = profiles.iter().collect();
    sorted.sort_by(|a, b| locale_compare(&a.name, &b.name));
    let result = Value::Array(
        sorted
            .into_iter()
            .map(|profile| {
                let mut object = Object::new();
                object.insert(JsString::from("id"), Value::String(JsString::from(profile.id.as_str())));
                object.insert(JsString::from("name"), Value::String(JsString::from(profile.name.as_str())));
                Value::Object(object)
            })
            .collect(),
    );
    if format { to_formatted_string(&result, &FormattingOptions::default()).to_string_lossy() } else { stringify_json(&result) }
}

fn stringify_remote_profiles(profiles: &[ISyncUserDataProfile]) -> String {
    let mut sorted: Vec<&ISyncUserDataProfile> = profiles.iter().collect();
    sorted.sort_by(|a, b| locale_compare(&a.name, &b.name));
    stringify_json(&Value::Array(sorted.into_iter().map(ISyncUserDataProfile::to_json).collect()))
}

/// `JSON.stringify(profiles.map(e => e.name))`.
fn names_json<'a>(names: impl Iterator<Item = &'a str>) -> String {
    stringify_json(&Value::Array(names.map(|name| Value::String(JsString::from(name))).collect()))
}

// endregion

fn local_change_of(local: &LocalResult) -> Change {
    if !local.added.is_empty() || !local.removed.is_empty() || !local.updated.is_empty() { Change::Modified } else { Change::None }
}

fn remote_change_of(remote: &Option<RemoteResult>) -> Change {
    if remote.is_some() { Change::Modified } else { Change::None }
}

/// O `IUserDataProfileManifestResourceMergeResult` de um `merge`, com o conteúdo dado.
fn merge_result(content: Option<String>, MergeResult { local, remote }: MergeResult) -> IUserDataProfileManifestResourceMergeResult {
    IAcceptResult { content, local_change: local_change_of(&local), remote_change: remote_change_of(&remote), extra: ManifestMergeExtra { local, remote } }
}

/// `UserDataProfilesManifestSynchroniser`. Dá `Deref` para a [`AbstractSynchroniser`].
pub struct UserDataProfilesManifestSynchroniser {
    pub base: Rc<AbstractSynchroniser<UserDataProfilesManifestSynchroniser>>,
    user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
    pub preview_resource: URI,
    pub base_resource: URI,
    pub local_resource: URI,
    pub remote_resource: URI,
    pub accepted_resource: URI,
}

impl std::ops::Deref for UserDataProfilesManifestSynchroniser {
    type Target = AbstractSynchroniser<UserDataProfilesManifestSynchroniser>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl IDisposable for UserDataProfilesManifestSynchroniser {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl UserDataProfilesManifestSynchroniser {
    pub fn new(profile: UserDataProfile, collection: Option<String>, user_data_profiles_service: Rc<dyn IUserDataProfilesService>, services: SynchroniserServices) -> Rc<Self> {
        let base = AbstractSynchroniser::<Self>::new(IUserDataSyncResource { sync_resource: SyncResource::Profiles, profile }, collection, services);
        let preview_resource = base.services.ext_uri.join_path(&base.sync_preview_folder, &["profiles.json"]);
        let this = Rc::new(Self {
            base: base.clone(),
            user_data_profiles_service,
            base_resource: with_sync_authority(&preview_resource, "base"),
            local_resource: with_sync_authority(&preview_resource, "local"),
            remote_resource: with_sync_authority(&preview_resource, "remote"),
            accepted_resource: with_sync_authority(&preview_resource, "accepted"),
            preview_resource,
        });
        base.bind(Rc::downgrade(&this));
        let weak = Rc::downgrade(&base);
        base.register(this.user_data_profiles_service.on_did_change_profiles().listen(move |_| {
            if let Some(base) = weak.upgrade() {
                base.trigger_local_change();
            }
        }));
        this
    }

    /// `getLastSyncedProfiles`.
    pub fn get_last_synced_profiles(&self) -> Promise<Option<Vec<ISyncUserDataProfile>>> {
        let last_sync_user_data = self.base.get_last_sync_user_data();
        async_fn(async move {
            match last_sync_user_data.await?.and_then(|last_sync_user_data| last_sync_user_data.sync_data) {
                Some(sync_data) => Ok(Some(parse_user_data_profiles_manifest(&sync_data)?)),
                None => Ok(None),
            }
        })
    }

    /// `getRemoteSyncedProfiles`.
    pub fn get_remote_synced_profiles(&self, ref_or_latest_data: Option<RefOrUserData>) -> Promise<Option<Vec<ISyncUserDataProfile>>> {
        let base = self.base.clone();
        async_fn(async move {
            let last_sync_user_data = base.get_last_sync_user_data().await?;
            let remote_user_data = base.get_latest_remote_user_data(ref_or_latest_data, last_sync_user_data).await?;
            match remote_user_data.sync_data {
                Some(sync_data) => Ok(Some(parse_user_data_profiles_manifest(&sync_data)?)),
                None => Ok(None),
            }
        })
    }

    /// `updateRemoteProfiles`.
    pub fn update_remote_profiles(&self, profiles: &[ISyncUserDataProfile], r#ref: Option<String>) -> Promise<IRemoteUserData> {
        self.base.update_remote_user_data(stringify_remote_profiles(profiles), r#ref)
    }

    /// `getLocalUserDataProfiles`: os perfis que se sincronizam (nem o padrão nem os transitórios).
    fn get_local_user_data_profiles(&self) -> Vec<UserDataProfile> {
        self.user_data_profiles_service.profiles().into_iter().filter(|profile| !profile.is_default && !profile.is_transient.unwrap_or(false)).collect()
    }

    fn accept_local(&self, resource_preview: &IUserDataProfilesManifestResourcePreview) -> IUserDataProfileManifestResourceMergeResult {
        let local_profiles = self.get_local_user_data_profiles();
        merge_result(resource_preview.local_content.clone(), merge(&local_profiles, None, None, &[]))
    }

    fn accept_remote(&self, resource_preview: &IUserDataProfilesManifestResourcePreview) -> Result<IUserDataProfileManifestResourceMergeResult, Error> {
        let remote_profiles = match resource_preview.remote_content.as_deref().filter(|content| !content.is_empty()) {
            Some(content) => Some(parse_profiles(content)?),
            None => None,
        };
        let mut last_sync_profiles: Vec<ISyncUserDataProfile> = Vec::new();
        let mut local_profiles: Vec<UserDataProfile> = Vec::new();
        for profile in self.get_local_user_data_profiles() {
            let remote_profile = remote_profiles.as_ref().and_then(|remote_profiles| remote_profiles.iter().find(|remote_profile| remote_profile.id == profile.id));
            if let Some(remote_profile) = remote_profile {
                last_sync_profiles.push(ISyncUserDataProfile { id: profile.id.clone(), name: profile.name.clone(), collection: remote_profile.collection.clone(), icon: None, use_default_flags: None });
                local_profiles.push(profile);
            }
        }
        Ok(match remote_profiles {
            Some(remote_profiles) => merge_result(resource_preview.remote_content.clone(), merge(&local_profiles, Some(&remote_profiles), Some(&last_sync_profiles), &[])),
            None => IAcceptResult { content: resource_preview.remote_content.clone(), local_change: Change::None, remote_change: Change::None, extra: ManifestMergeExtra::default() },
        })
    }
}

impl SynchroniserImpl for UserDataProfilesManifestSynchroniser {
    type Preview = ManifestPreviewExtra;
    type Accept = ManifestMergeExtra;

    fn version(&self) -> f64 {
        2.0
    }

    fn generate_sync_preview(
        self: Rc<Self>,
        remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        _is_remote_data_from_current_machine: bool,
        _user_data_sync_configuration: IUserDataSyncConfiguration,
        _token: CancellationToken,
    ) -> Promise<Vec<IUserDataProfilesManifestResourcePreview>> {
        async_fn(async move {
            let remote_profiles = match &remote_user_data.sync_data {
                Some(sync_data) => Some(parse_user_data_profiles_manifest(sync_data)?),
                None => None,
            };
            let last_sync_profiles = match last_sync_user_data.as_ref().and_then(|last_sync_user_data| last_sync_user_data.sync_data.as_ref()) {
                Some(sync_data) => Some(parse_user_data_profiles_manifest(sync_data)?),
                None => None,
            };
            let local_profiles = self.get_local_user_data_profiles();

            let preview_result = merge_result(
                last_sync_profiles.as_deref().map(stringify_remote_profiles),
                merge(&local_profiles, remote_profiles.as_deref(), last_sync_profiles.as_deref(), &[]),
            );

            let local_content = stringify_local_profiles(&local_profiles, false);
            Ok(vec![IResourcePreview {
                base_resource: self.base_resource.clone(),
                base_content: last_sync_profiles.as_deref().map(stringify_remote_profiles),
                local_resource: self.local_resource.clone(),
                local_content: Some(local_content),
                remote_resource: self.remote_resource.clone(),
                remote_content: remote_profiles.as_deref().map(stringify_remote_profiles),
                preview_resource: self.preview_resource.clone(),
                local_change: preview_result.local_change,
                remote_change: preview_result.remote_change,
                accepted_resource: self.accepted_resource.clone(),
                extra: ManifestPreviewExtra { preview_result, remote_profiles },
            }])
        })
    }

    fn has_remote_changed(self: Rc<Self>, last_sync_user_data: IRemoteUserData) -> Promise<bool> {
        async_fn(async move {
            let last_sync_profiles = match &last_sync_user_data.sync_data {
                Some(sync_data) => Some(parse_user_data_profiles_manifest(sync_data)?),
                None => None,
            };
            let local_profiles = self.get_local_user_data_profiles();
            let MergeResult { remote, .. } = merge(&local_profiles, last_sync_profiles.as_deref(), last_sync_profiles.as_deref(), &[]);
            Ok(remote.is_some_and(|remote| !remote.added.is_empty() || !remote.removed.is_empty() || !remote.updated.is_empty()))
        })
    }

    fn get_merge_result(self: Rc<Self>, resource_preview: IUserDataProfilesManifestResourcePreview, _token: CancellationToken) -> Promise<IMergeResult<ManifestMergeExtra>> {
        Promise::resolved(IMergeResult { result: resource_preview.extra.preview_result, has_conflicts: false })
    }

    fn get_accept_result(
        self: Rc<Self>,
        resource_preview: IUserDataProfilesManifestResourcePreview,
        resource: URI,
        _content: Option<Option<String>>,
        _token: CancellationToken,
    ) -> Promise<IUserDataProfileManifestResourceMergeResult> {
        async_fn(async move {
            let is_same = |other: &URI| self.services.ext_uri.is_equal(Some(&resource), Some(other), false);

            /* Accept local resource */
            if is_same(&self.local_resource) {
                return Ok(self.accept_local(&resource_preview));
            }

            /* Accept remote resource */
            if is_same(&self.remote_resource) {
                return self.accept_remote(&resource_preview);
            }

            /* Accept preview resource */
            if is_same(&self.preview_resource) {
                return Ok(resource_preview.extra.preview_result);
            }

            Err(Error::new(format!("Invalid Resource: {}", resource.to_string(false).to_string_lossy())))
        })
    }

    fn apply_result(
        self: Rc<Self>,
        mut remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        resource_previews: Vec<(IUserDataProfilesManifestResourcePreview, IUserDataProfileManifestResourceMergeResult)>,
        force: bool,
    ) -> Promise<()> {
        async_fn(async move {
            let (resource_preview, accept_result) = &resource_previews[0];
            let ManifestMergeExtra { local, remote } = &accept_result.extra;
            let (local_change, remote_change) = (accept_result.local_change, accept_result.remote_change);
            let label = &self.sync_resource_log_label;
            if local_change == Change::None && remote_change == Change::None {
                self.services.log_service.info(&format!("{label}: No changes found during synchronizing profiles."));
            }

            let mut remote_profiles = resource_preview.extra.remote_profiles.clone().unwrap_or_default();
            let remote_added = remote.as_ref().map_or(0, |remote| remote.added.len());
            let remote_removed = remote.as_ref().map_or(0, |remote| remote.removed.len());
            if remote_profiles.len() + remote_added > MAX_PROFILES + remote_removed {
                return Err(UserDataSyncError::new("Too many profiles to sync. Please remove some profiles and try again.", UserDataSyncErrorCode::LocalTooManyProfiles, None).into_error());
            }

            if local_change != Change::None {
                self.backup_local(&stringify_local_profiles(&self.get_local_user_data_profiles(), false)).await?;

                let removals: Vec<Promise<()>> = local
                    .removed
                    .iter()
                    .map(|profile| {
                        let (this, profile) = (self.clone(), profile.clone());
                        async_fn(async move {
                            this.services.log_service.trace(&format!("{}: Removing '{}' profile...", this.sync_resource_log_label, profile.name));
                            this.user_data_profiles_service.remove_profile(&profile).await?;
                            this.services.log_service.info(&format!("{}: Removed profile '{}'.", this.sync_resource_log_label, profile.name));
                            Ok(())
                        })
                    })
                    .collect();
                Promise::all(removals).await?;

                let creations: Vec<Promise<()>> = local
                    .added
                    .iter()
                    .map(|profile| {
                        let (this, profile) = (self.clone(), profile.clone());
                        async_fn(async move {
                            this.services.log_service.trace(&format!("{}: Creating '{}' profile...", this.sync_resource_log_label, profile.name));
                            let options = UserDataProfileOptions { icon: profile.icon.clone(), use_default_flags: profile.use_default_flags, ..UserDataProfileOptions::default() };
                            this.user_data_profiles_service.create_profile(&profile.id, &profile.name, Some(options), None).await?;
                            this.services.log_service.info(&format!("{}: Created profile '{}'.", this.sync_resource_log_label, profile.name));
                            Ok(())
                        })
                    })
                    .collect();
                Promise::all(creations).await?;

                let updates: Vec<Promise<()>> = local
                    .updated
                    .iter()
                    .map(|profile| {
                        let (this, profile) = (self.clone(), profile.clone());
                        async_fn(async move {
                            let local_profile = this.user_data_profiles_service.profiles().into_iter().find(|candidate| candidate.id == profile.id);
                            match local_profile {
                                Some(local_profile) => {
                                    this.services.log_service.trace(&format!("{}: Updating '{}' profile...", this.sync_resource_log_label, profile.name));
                                    // `icon` ausente é `undefined`: não muda o ícone
                                    let options = UserDataProfileUpdateOptions {
                                        name: Some(profile.name.clone()),
                                        icon: profile.icon.clone().map(Some),
                                        use_default_flags: profile.use_default_flags,
                                        ..UserDataProfileUpdateOptions::default()
                                    };
                                    this.user_data_profiles_service.update_profile(&local_profile, options).await?;
                                    this.services.log_service.info(&format!("{}: Updated profile '{}'.", this.sync_resource_log_label, profile.name));
                                }
                                None => this.services.log_service.info(&format!("{}: Could not find profile with id '{}' to update.", this.sync_resource_log_label, profile.id)),
                            }
                            Ok(())
                        })
                    })
                    .collect();
                Promise::all(updates).await?;
            }

            if remote_change != Change::None {
                self.services.log_service.trace(&format!("{label}: Updating remote profiles..."));
                let mut added_collections: Vec<String> = Vec::new();
                let can_add_remote_profiles = remote_profiles.len() + remote_added <= MAX_PROFILES;
                if can_add_remote_profiles {
                    for profile in remote.iter().flat_map(|remote| remote.added.iter()) {
                        let collection = self.services.user_data_sync_store_service.create_collection(Some(self.sync_headers())).await?;
                        self.services.log_service.trace(&format!("{label}: Created collection \"{collection}\" for \"{}\".", profile.name));
                        added_collections.push(collection.clone());
                        remote_profiles.push(ISyncUserDataProfile { id: profile.id.clone(), name: profile.name.clone(), collection, icon: profile.icon.clone(), use_default_flags: profile.use_default_flags });
                    }
                } else {
                    self.services.log_service.info(&format!("{label}: Could not create remote profiles as there are too many profiles."));
                }
                for profile in remote.iter().flat_map(|remote| remote.removed.iter()) {
                    // `splice(findIndex(...), 1)`: sem o perfil, o índice é -1 e sai o último
                    match remote_profiles.iter().position(|candidate| candidate.id == profile.id) {
                        Some(index) => {
                            remote_profiles.remove(index);
                        }
                        None => {
                            remote_profiles.pop();
                        }
                    }
                }
                for profile in remote.iter().flat_map(|remote| remote.updated.iter()) {
                    if let Some(profile_to_be_updated) = remote_profiles.iter_mut().find(|candidate| candidate.id == profile.id) {
                        *profile_to_be_updated = ISyncUserDataProfile {
                            id: profile.id.clone(),
                            name: profile.name.clone(),
                            icon: profile.icon.clone(),
                            use_default_flags: profile.use_default_flags,
                            ..profile_to_be_updated.clone()
                        };
                    }
                }

                match self.update_remote_profiles(&remote_profiles, if force { None } else { Some(remote_user_data.r#ref.clone()) }).await {
                    Ok(updated) => {
                        remote_user_data = updated;
                        let added = if can_add_remote_profiles && remote_added > 0 { format!(" Added: {}.", names_json(remote.iter().flat_map(|remote| remote.added.iter()).map(|profile| profile.name.as_str()))) } else { String::new() };
                        let updated = match remote.as_ref().filter(|remote| !remote.updated.is_empty()) {
                            Some(remote) => format!(" Updated: {}.", names_json(remote.updated.iter().map(|profile| profile.name.as_str()))),
                            None => String::new(),
                        };
                        let removed = match remote.as_ref().filter(|remote| !remote.removed.is_empty()) {
                            Some(remote) => format!(" Removed: {}.", names_json(remote.removed.iter().map(|profile| profile.name.as_str()))),
                            None => String::new(),
                        };
                        self.services.log_service.info(&format!("{label}: Updated remote profiles.{added}{updated}{removed}"));
                    }
                    Err(error) => {
                        if !added_collections.is_empty() {
                            self.services.log_service.info(&format!("{label}: Failed to update remote profiles. Cleaning up added collections..."));
                            for collection in &added_collections {
                                self.services.user_data_sync_store_service.delete_collection(Some(collection), Some(self.sync_headers())).await?;
                            }
                        }
                        return Err(error);
                    }
                }

                for profile in remote.iter().flat_map(|remote| remote.removed.iter()) {
                    self.services.user_data_sync_store_service.delete_collection(Some(&profile.collection), Some(self.sync_headers())).await?;
                }
            }

            if last_sync_user_data.as_ref().map(|last_sync_user_data| &last_sync_user_data.r#ref) != Some(&remote_user_data.r#ref) {
                // update last sync
                self.services.log_service.trace(&format!("{label}: Updating last synchronized profiles..."));
                self.update_last_sync_user_data(remote_user_data, Vec::new()).await?;
                self.services.log_service.info(&format!("{label}: Updated last synchronized profiles."));
            }
            Ok(())
        })
    }

    fn has_local_data(self: Rc<Self>) -> Promise<bool> {
        Promise::resolved(!self.get_local_user_data_profiles().is_empty())
    }

    fn resolve_content(self: Rc<Self>, uri: URI) -> Promise<Option<String>> {
        async_fn(async move {
            let virtual_resources = [&self.remote_resource, &self.base_resource, &self.local_resource, &self.accepted_resource];
            if virtual_resources.into_iter().any(|resource| self.services.ext_uri.is_equal(Some(resource), Some(&uri), false)) {
                let content = self.resolve_preview_content(&uri).await?;
                return match &content {
                    Some(text) if !text.is_empty() => Ok(Some(to_formatted_string(&parse_json(text)?, &FormattingOptions::default()).to_string_lossy())),
                    _ => Ok(content),
                };
            }
            Ok(None)
        })
    }
}
