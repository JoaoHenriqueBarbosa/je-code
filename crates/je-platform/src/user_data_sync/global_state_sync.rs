//! Porte de `src/vs/platform/userDataSync/common/globalStateSync.ts`: o sincronizador do estado global (o
//! armazenamento de escopo de usuário do perfil e o `locale` do `argv.json`), o provedor do estado global local, o
//! inicializador e o `UserDataSyncStoreTypeSynchronizer`.
//!
//! Decisões de tradução (as da base estão em [`super::abstract_synchronizer`]):
//! - `IGlobalStateResourcePreview` é um [`IResourcePreview`] com [`GlobalStatePreviewExtra`] (`localUserData`,
//!   `previewResult` e `storageKeys`); `IGlobalStateResourceMergeResult` é um [`IAcceptResult`] com
//!   [`GlobalStateMergeExtra`] (`local` e `remote`, os do `merge`).
//! - `JSON.parse(content)` de um `IGlobalState` é [`parse_global_state`] (que rejeita um valor de `storage` que não
//!   tem a forma de `IStorageValue`, onde o original seguiria adiante com o valor torto) e `JSON.stringify` é
//!   [`global_state_to_json`]: as chaves saem na ordem `version`, `value`, `scope`, a de quem as cria no original.
//!   `storage` ausente num dado remoto torto vale como vazio.
//! - `stringify(globalState, format)` ordena as chaves de `globalState.storage` NO PRÓPRIO objeto (a atribuição
//!   `globalState.storage = storage` do original): [`stringify`] recebe `&mut IGlobalState`, porque o preview guarda
//!   o estado local já ordenado.
//! - `IStorageValue.value` é `string` no original e é [`JsString`] aqui: um `locale` do `argv.json` que não é texto
//!   fica de fora do estado local (o tipo do original não o admite).
//! - `Event.any(argv mudou, perfil mudou)` são dois ouvintes que disparam o mesmo `triggerLocalChange`.
//! - A chave do `hasLocalData` é `${argvStoragePrefx}.locale`, com o ponto a mais do original (`globalState.argv..locale`):
//!   o porte a mantém, e com isso a chave nunca existe e `hasLocalData` vale `true` quando o estado local lê.
//! - `UserDataSyncStoreTypeSynchronizer.getSyncStoreType` devolve [`UserDataSyncStoreType`]: um texto que não é
//!   `insiders` nem `stable` (o `as` do original o deixaria passar) é `None`.
//! - `IStorageService.storeAll` de `GlobalStateInitializer` recebe os [`StorageEntry`] do porte.

use std::ops::Deref;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::error_message::to_error_message_of;
use je_base::errors::Error;
use je_base::json::{self, Object, Value};
use je_base::json_edit::EditSegment;
use je_base::json_formatter::{FormattingOptions, to_formatted_string};
use je_base::lifecycle::IDisposable;
use je_base::parts::request::IHeaders;
use je_base::platform::Platform;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_storage::StorageValue;

use super::abstract_json_synchronizer::PreviewResources;
use super::abstract_synchronizer::{
    AbstractInitializer, AbstractSynchroniser, IAcceptResult, IMergeResult, IResourcePreview, InitializerImpl, SynchroniserImpl, SynchroniserServices, get_sync_resource_log_label, parse_json,
    stringify_json,
};
use super::content::edit;
use super::global_state_merge::{LocalResult, MergeResult, RemoteResult, StorageDictionary, StorageKeys as MergeStorageKeys, merge};
use super::user_data_sync::{
    ALL_SYNC_RESOURCES, Change, IGlobalState, IRemoteUserData, IStorageValue, ISyncData, IUserData, IUserDataSyncConfiguration, IUserDataSyncLogService, IUserDataSyncResource,
    IUserDataSyncStoreService, SYNC_SERVICE_URL_TYPE, ServerResource, SyncResource, UserDataSyncError, UserDataSyncErrorCode, UserDataSyncStoreType, create_sync_headers, get_enablement_key,
};
use super::user_data_sync_store_service::UserDataSyncStoreClient;
use crate::environment::environment::EnvironmentService;
use crate::external_services::service_machine_id::get_service_machine_id;
use crate::files::file_service::{FileContentSource, FileService};
use crate::log::{LogArg, LogServiceRef};
use crate::storage::{StorageEntry, StorageScope, StorageService, StorageSwitchTarget, StorageTarget};
use crate::user_data_profile::user_data_profile_storage_service::{IUserDataProfileStorageService, ProfileStorageValue, storage_profile};
use crate::user_data_profile::{IUserDataProfilesService, UserDataProfile};

const ARGV_STORAGE_PREFIX: &str = "globalState.argv.";
const ARGV_PROPERTIES: [&str; 1] = ["locale"];
const GLOBAL_STATE_DATA_VERSION: f64 = 1.0;

// region: JSON do estado global

fn scope_to_json(scope: StorageScope) -> Value {
    Value::Number(f64::from(scope as i32))
}

fn scope_from_json(value: &Value) -> Option<StorageScope> {
    let Value::Number(number) = value else { return None };
    match *number as i64 {
        -2 => Some(StorageScope::ApplicationShared),
        -1 => Some(StorageScope::Application),
        0 => Some(StorageScope::Profile),
        1 => Some(StorageScope::Workspace),
        _ => None,
    }
}

fn storage_value_to_json(value: &IStorageValue) -> Value {
    let mut object = Object::new();
    object.insert(JsString::from("version"), Value::Number(value.version));
    object.insert(JsString::from("value"), Value::String(value.value.clone()));
    if let Some(scope) = value.scope {
        object.insert(JsString::from("scope"), scope_to_json(scope));
    }
    Value::Object(object)
}

fn storage_value_from_json(value: &Value) -> Option<IStorageValue> {
    let Value::Object(object) = value else { return None };
    let Some(Value::Number(version)) = object.get("version") else { return None };
    let Some(Value::String(text)) = object.get("value") else { return None };
    Some(IStorageValue { version: *version, value: text.clone(), scope: object.get("scope").and_then(scope_from_json) })
}

/// `{ storage }`: `storage` nulo é o `null` de `remote.all`.
fn storage_wrapper_json(storage: Option<&StorageDictionary>) -> Value {
    let storage = storage.map_or(Value::Null, |storage| {
        let mut object = Object::new();
        for (key, value) in storage.iter() {
            object.insert(key.clone(), storage_value_to_json(value));
        }
        Value::Object(object)
    });
    let mut wrapper = Object::new();
    wrapper.insert(JsString::from("storage"), storage);
    Value::Object(wrapper)
}

/// `JSON.stringify(globalState)` como valor.
pub fn global_state_to_json(global_state: &IGlobalState) -> Value {
    storage_wrapper_json(Some(&global_state.storage))
}

/// `JSON.parse(content)` de um [`IGlobalState`].
pub fn parse_global_state(content: &str) -> Result<IGlobalState, Error> {
    let parsed = parse_json(content)?;
    let mut storage = StorageDictionary::new();
    match &parsed {
        Value::Null => return Err(Error::type_error("Cannot read properties of null (reading 'storage')")),
        Value::Object(object) => {
            if let Some(Value::Object(entries)) = object.get("storage") {
                for (key, entry) in entries.iter() {
                    let value = storage_value_from_json(entry).ok_or_else(|| Error::type_error(format!("Invalid global state storage value: {}", key.to_string_lossy())))?;
                    storage.insert(key.clone(), value);
                }
            }
        }
        _ => {}
    }
    Ok(IGlobalState { storage })
}

/// `stringify(globalState, format)`: ordena as chaves do `storage` (no próprio estado) e o escreve, compacto ou
/// formatado.
pub fn stringify(global_state: &mut IGlobalState, format: bool) -> String {
    let mut keys: Vec<JsString> = global_state.storage.keys().cloned().collect();
    keys.sort_by(|a, b| a.units().cmp(b.units()));
    let mut storage = StorageDictionary::new();
    for key in keys {
        if let Some(value) = global_state.storage.get(&key) {
            let value = value.clone();
            storage.insert(key, value);
        }
    }
    global_state.storage = storage;
    let value = global_state_to_json(global_state);
    if format { to_formatted_string(&value, &FormattingOptions::default()).to_string_lossy() } else { stringify_json(&value) }
}

/// `Object.keys(...)` de uma lista de chaves como o `${array}` do JavaScript (separadas por vírgula).
fn join_keys(keys: &[JsString]) -> String {
    keys.iter().map(JsString::to_string_lossy).collect::<Vec<_>>().join(",")
}

fn keys_dynamic<'a>(keys: impl Iterator<Item = &'a String>) -> Dynamic {
    Dynamic::array(keys.map(|key| Dynamic::string(key)))
}

// endregion

// region: tipos do preview

/// `StorageKeys`: as chaves de armazenamento por alvo e as não registradas.
#[derive(Clone, Debug)]
pub struct StorageKeys {
    pub machine: Vec<JsString>,
    pub user: Vec<JsString>,
    pub unregistered: Vec<JsString>,
}

impl StorageKeys {
    /// O recorte que o `merge` lê.
    fn for_merge(&self) -> MergeStorageKeys<'_> {
        MergeStorageKeys { machine: &self.machine, unregistered: &self.unregistered }
    }
}

/// O que `IGlobalStateResourceMergeResult` acrescenta a `IAcceptResult`: `local` e `remote`.
#[derive(Clone, Debug, Default)]
pub struct GlobalStateMergeExtra {
    pub local: LocalResult,
    pub remote: RemoteResult,
}

/// `IGlobalStateResourceMergeResult`.
pub type IGlobalStateResourceMergeResult = IAcceptResult<GlobalStateMergeExtra>;

/// O que `IGlobalStateResourcePreview` acrescenta a `IResourcePreview`.
#[derive(Clone, Debug)]
pub struct GlobalStatePreviewExtra {
    pub local_user_data: IGlobalState,
    pub preview_result: IGlobalStateResourceMergeResult,
    pub storage_keys: StorageKeys,
}

/// `IGlobalStateResourcePreview`.
pub type IGlobalStateResourcePreview = IResourcePreview<GlobalStatePreviewExtra>;

/// `Object.keys(local.added).length > 0 || Object.keys(local.updated).length > 0 || local.removed.length > 0 ? Change.Modified : Change.None`.
fn local_change_of(local: &LocalResult) -> Change {
    if !local.added.is_empty() || !local.updated.is_empty() || !local.removed.is_empty() { Change::Modified } else { Change::None }
}

/// `remote.all !== null ? Change.Modified : Change.None`.
fn remote_change_of(remote: &RemoteResult) -> Change {
    if remote.all.is_some() { Change::Modified } else { Change::None }
}

// endregion

// region: LocalGlobalStateProvider

/// O que `handleUpdatedStorage` junta: o `argv` a editar e os dois mapas de armazenamento a gravar (o do perfil e,
/// no perfil padrão, o compartilhado). `None` é o `undefined` do original (remover a chave).
#[derive(Default)]
struct StorageUpdates {
    argv: IndexMap<String, Option<String>>,
    profile: IndexMap<String, Option<String>>,
    shared: Option<IndexMap<String, Option<String>>>,
}

impl StorageUpdates {
    /// O mapa do escopo: o compartilhado só existe no perfil padrão.
    fn target_map(&mut self, is_shared_scope: bool) -> &mut IndexMap<String, Option<String>> {
        match (&mut self.shared, is_shared_scope) {
            (Some(shared), true) => shared,
            _ => &mut self.profile,
        }
    }

    /// `handleUpdatedStorage(keys, storage?)`.
    fn handle<'a>(&mut self, keys: impl Iterator<Item = &'a JsString>, storage: Option<&StorageDictionary>, storage_data: &IndexMap<String, ProfileStorageValue>) {
        for key in keys {
            let key_text = key.to_string_lossy();
            if let Some(argv_property) = key_text.strip_prefix(ARGV_STORAGE_PREFIX) {
                self.argv.insert(argv_property.to_string(), storage.and_then(|storage| storage.get(key)).map(|value| value.value.to_string_lossy()));
                continue;
            }
            match storage {
                Some(storage) => {
                    let Some(storage_value) = storage.get(key) else { continue };
                    let new_value = storage_value.value.to_string_lossy();
                    if storage_data.get(&key_text).and_then(|data| data.value.as_deref()) != Some(new_value.as_str()) {
                        let is_shared_scope = storage_value.scope == Some(StorageScope::ApplicationShared);
                        self.target_map(is_shared_scope).insert(key_text, Some(new_value));
                    }
                }
                None => {
                    if let Some(data) = storage_data.get(&key_text) {
                        let is_shared_scope = data.scope == Some(StorageScope::ApplicationShared);
                        self.target_map(is_shared_scope).insert(key_text, None);
                    }
                }
            }
        }
    }
}

/// `LocalGlobalStateProvider`.
#[derive(Clone)]
pub struct LocalGlobalStateProvider {
    file_service: Rc<FileService>,
    environment_service: Rc<dyn EnvironmentService>,
    user_data_profile_storage_service: Rc<dyn IUserDataProfileStorageService>,
    log_service: IUserDataSyncLogService,
}

impl LocalGlobalStateProvider {
    pub fn new(
        file_service: Rc<FileService>,
        environment_service: Rc<dyn EnvironmentService>,
        user_data_profile_storage_service: Rc<dyn IUserDataProfileStorageService>,
        log_service: IUserDataSyncLogService,
    ) -> Self {
        Self { file_service, environment_service, user_data_profile_storage_service, log_service }
    }

    /// `getLocalGlobalState`.
    pub fn get_local_global_state(&self, profile: &UserDataProfile) -> Promise<IGlobalState> {
        let (this, profile) = (self.clone(), profile.clone());
        async_fn(async move {
            let mut storage = StorageDictionary::new();
            if profile.is_default {
                let argv_content = this.get_local_argv_content().await?;
                let mut errors = Vec::new();
                let argv_value = json::parse_str(&argv_content, &mut errors);
                for argv_property in ARGV_PROPERTIES {
                    let property = match &argv_value {
                        Some(Value::Object(object)) => object.get(argv_property),
                        None | Some(Value::Null) => {
                            let of = if argv_value.is_none() { "undefined" } else { "null" };
                            return Err(Error::type_error(format!("Cannot read properties of {of} (reading '{argv_property}')")));
                        }
                        Some(_) => None,
                    };
                    if let Some(Value::String(text)) = property {
                        storage.insert(JsString::from(format!("{ARGV_STORAGE_PREFIX}{argv_property}")), IStorageValue { version: 1.0, value: text.clone(), scope: None });
                    }
                }
            }
            let storage_data = this.user_data_profile_storage_service.read_storage_data(&profile).await?;
            for (key, value) in &storage_data {
                if value.target != StorageTarget::User {
                    continue;
                }
                if let Some(text) = value.value.as_ref().filter(|text| !text.is_empty()) {
                    storage.insert(JsString::from(key.as_str()), IStorageValue { version: 1.0, value: JsString::from(text.as_str()), scope: value.scope });
                }
            }
            Ok(IGlobalState { storage })
        })
    }

    /// `getLocalArgvContent`: `{}` se o `argv.json` não pode ser lido.
    fn get_local_argv_content(&self) -> Promise<String> {
        let this = self.clone();
        async_fn(async move {
            let argv_resource = this.environment_service.argv_resource();
            let argv_text = argv_resource.to_string(false).to_string_lossy();
            this.log_service.debug_args("GlobalStateSync#getLocalArgvContent", &[LogArg::from(Dynamic::string(&argv_text))]);
            match this.file_service.read_file(&argv_resource, None, None).await {
                Ok(content) => {
                    this.log_service.debug_args("GlobalStateSync#getLocalArgvContent - Resolved", &[LogArg::from(Dynamic::string(&argv_text))]);
                    return Ok(content.value.to_string());
                }
                Err(error) => this.log_service.debug(&to_error_message_of(&error, false).to_string()),
            }
            Ok("{}".to_string())
        })
    }

    /// `writeLocalGlobalState`.
    pub fn write_local_global_state(&self, local: &LocalResult, profile: &UserDataProfile) -> Promise<()> {
        let (this, profile, LocalResult { added, removed, updated }) = (self.clone(), profile.clone(), local.clone());
        async_fn(async move {
            let sync_resource_log_label = get_sync_resource_log_label(SyncResource::GlobalState, &profile);
            let storage_data = this.user_data_profile_storage_service.read_storage_data(&profile).await?;
            let mut updates = StorageUpdates { shared: profile.is_default.then(IndexMap::new), ..StorageUpdates::default() };
            updates.handle(added.keys(), Some(&added), &storage_data);
            updates.handle(updated.keys(), Some(&updated), &storage_data);
            updates.handle(removed.iter(), None, &storage_data);
            let StorageUpdates { argv, profile: updated_profile_storage, shared: updated_shared_storage } = updates;

            if !argv.is_empty() {
                this.log_service.trace(&format!("{sync_resource_log_label}: Updating locale..."));
                let argv_content = this.get_local_argv_content().await?;
                let mut content = argv_content.clone();
                for (argv_property, value) in &argv {
                    let value = value.as_ref().map(|value| Value::String(JsString::from(value.as_str())));
                    content = edit(&content, &[EditSegment::Property(JsString::from(argv_property.as_str()))], value, &FormattingOptions::default());
                }
                if argv_content != content {
                    this.log_service.trace(&format!("{sync_resource_log_label}: Updating locale..."));
                    this.file_service.write_file(&this.environment_service.argv_resource(), FileContentSource::from(content.as_str()), None).await?;
                    this.log_service.info(&format!("{sync_resource_log_label}: Updated locale."));
                }
                this.log_service.info(&format!("{sync_resource_log_label}: Updated locale"));
            }

            if !updated_profile_storage.is_empty() {
                this.log_service.trace(&format!("{sync_resource_log_label}: Updating global state..."));
                let keys = keys_dynamic(updated_profile_storage.keys());
                this.user_data_profile_storage_service.update_storage_data(&profile, updated_profile_storage, StorageTarget::User, None).await?;
                this.log_service.info_args(&format!("{sync_resource_log_label}: Updated global state"), &[LogArg::from(keys)]);
            }

            if let Some(updated_shared_storage) = updated_shared_storage.filter(|storage| !storage.is_empty()) {
                this.log_service.trace(&format!("{sync_resource_log_label}: Updating application shared state..."));
                let keys = keys_dynamic(updated_shared_storage.keys());
                this.user_data_profile_storage_service.update_storage_data(&profile, updated_shared_storage, StorageTarget::User, Some(StorageScope::ApplicationShared)).await?;
                this.log_service.info_args(&format!("{sync_resource_log_label}: Updated application shared state"), &[LogArg::from(keys)]);
            }
            Ok(())
        })
    }
}

// endregion

/// `GlobalStateSynchroniser`: sincroniza o armazenamento de escopo de usuário do perfil e o `locale` do `argv.json`.
///
/// O estado global é sincronizado sem checar a versão, como os outros recursos (settings, keybindings). Se o formato
/// do valor de uma chave muda e pede migração, quem a possui remove a chave do escopo de usuário e a troca por uma
/// chave nova de escopo de usuário. Dá `Deref` para a [`AbstractSynchroniser`].
pub struct GlobalStateSynchroniser {
    pub base: Rc<AbstractSynchroniser<GlobalStateSynchroniser>>,
    user_data_profile_storage_service: Rc<dyn IUserDataProfileStorageService>,
    local_global_state_provider: LocalGlobalStateProvider,
    /// `previewResource`, `baseResource`, `localResource`, `remoteResource` e `acceptedResource`.
    pub resources: PreviewResources,
}

impl Deref for GlobalStateSynchroniser {
    type Target = AbstractSynchroniser<GlobalStateSynchroniser>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl IDisposable for GlobalStateSynchroniser {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl GlobalStateSynchroniser {
    pub fn new(profile: UserDataProfile, collection: Option<String>, services: SynchroniserServices, user_data_profile_storage_service: Rc<dyn IUserDataProfileStorageService>) -> Rc<Self> {
        let base = AbstractSynchroniser::<Self>::new(IUserDataSyncResource { sync_resource: SyncResource::GlobalState, profile }, collection, services);
        let preview_resource = base.services.ext_uri.join_path(&base.sync_preview_folder, &["globalState.json"]);
        let local_global_state_provider = LocalGlobalStateProvider::new(
            base.services.file_service.clone(),
            base.services.environment_service.clone(),
            user_data_profile_storage_service.clone(),
            base.services.log_service.clone(),
        );
        let this = Rc::new(Self { base: base.clone(), user_data_profile_storage_service: user_data_profile_storage_service.clone(), local_global_state_provider, resources: PreviewResources::new(preview_resource) });
        base.bind(Rc::downgrade(&this));

        let argv_resource = base.services.environment_service.argv_resource();
        base.register(base.services.file_service.watch(&base.services.ext_uri.dirname(&argv_resource), None));

        /* Locale change */
        let weak: Weak<AbstractSynchroniser<Self>> = Rc::downgrade(&base);
        base.register(base.services.file_service.on_did_files_change().listen({
            let weak = weak.clone();
            move |event| {
                if event.contains(&argv_resource, &[])
                    && let Some(base) = weak.upgrade()
                {
                    base.trigger_local_change();
                }
            }
        }));

        let profile_id = base.sync_resource.profile.id.clone();
        base.register(user_data_profile_storage_service.on_did_change().listen(move |changes| {
            /* StorageTarget has changed in profile storage */
            let target_changed = changes.target_changes.iter().any(|profile| profile.id == profile_id);
            /* User storage data has changed in profile storage */
            let user_data_changed = changes.value_changes.iter().any(|value_changes| value_changes.profile.id == profile_id && value_changes.changes.iter().any(|change| change.target == Some(StorageTarget::User)));
            if (target_changed || user_data_changed)
                && let Some(base) = weak.upgrade()
            {
                base.trigger_local_change();
            }
        }));
        this
    }

    /// `getStorageKeys`.
    fn get_storage_keys(&self, last_sync_global_state: Option<&IGlobalState>) -> Promise<StorageKeys> {
        let (service, profile) = (self.user_data_profile_storage_service.clone(), self.sync_resource.profile.clone());
        let last_sync_keys: Vec<JsString> = last_sync_global_state.map(|state| state.storage.keys().cloned().collect()).unwrap_or_default();
        async_fn(async move {
            let storage_data = service.read_storage_data(&profile).await?;
            let (mut user, mut machine): (Vec<JsString>, Vec<JsString>) = (Vec::new(), Vec::new());
            for (key, value) in &storage_data {
                match value.target {
                    StorageTarget::User => user.push(JsString::from(key.as_str())),
                    StorageTarget::Machine => machine.push(JsString::from(key.as_str())),
                }
            }
            let registered: Vec<JsString> = user.iter().chain(machine.iter()).cloned().collect();
            let mut unregistered: Vec<JsString> = last_sync_keys
                .into_iter()
                .filter(|key| {
                    let key_text = key.to_string_lossy();
                    !key_text.starts_with(ARGV_STORAGE_PREFIX) && !registered.contains(key) && storage_data.contains_key(&key_text)
                })
                .collect();

            if !Platform::detect().is_web {
                // Following keys are synced only in web. Do not sync these keys in other platforms
                let keys_synced_only_in_web: Vec<JsString> =
                    ALL_SYNC_RESOURCES.iter().map(|resource| JsString::from(get_enablement_key(*resource))).chain(std::iter::once(JsString::from(SYNC_SERVICE_URL_TYPE))).collect();
                unregistered.extend(keys_synced_only_in_web.iter().cloned());
                machine.extend(keys_synced_only_in_web);
            }

            Ok(StorageKeys { user, machine, unregistered })
        })
    }

    /// `acceptLocal`.
    fn accept_local(&self, resource_preview: &IGlobalStateResourcePreview) -> Result<IGlobalStateResourceMergeResult, Error> {
        let local_user_data = &resource_preview.extra.local_user_data;
        match &resource_preview.remote_content {
            Some(remote_content) => {
                let remote_global_state = parse_global_state(remote_content)?;
                let MergeResult { local, remote } = merge(
                    &local_user_data.storage,
                    Some(&remote_global_state.storage),
                    Some(&remote_global_state.storage),
                    resource_preview.extra.storage_keys.for_merge(),
                    &*self.services.log_service,
                );
                Ok(IAcceptResult { content: Some(remote_content.clone()), local_change: Change::None, remote_change: remote_change_of(&remote), extra: GlobalStateMergeExtra { local, remote } })
            }
            None => {
                let added: Vec<JsString> = local_user_data.storage.keys().cloned().collect();
                Ok(IAcceptResult {
                    content: resource_preview.local_content.clone(),
                    local_change: Change::None,
                    remote_change: Change::Modified,
                    extra: GlobalStateMergeExtra {
                        local: LocalResult::default(),
                        remote: RemoteResult { added, removed: Vec::new(), updated: Vec::new(), all: Some(local_user_data.storage.clone()) },
                    },
                })
            }
        }
    }

    /// `acceptRemote`.
    fn accept_remote(&self, resource_preview: &IGlobalStateResourcePreview) -> Result<IGlobalStateResourceMergeResult, Error> {
        let local_user_data = &resource_preview.extra.local_user_data;
        match &resource_preview.remote_content {
            Some(remote_content) => {
                let remote_global_state = parse_global_state(remote_content)?;
                let MergeResult { local, remote } = merge(
                    &local_user_data.storage,
                    Some(&remote_global_state.storage),
                    Some(&local_user_data.storage),
                    resource_preview.extra.storage_keys.for_merge(),
                    &*self.services.log_service,
                );
                Ok(IAcceptResult { content: Some(remote_content.clone()), local_change: local_change_of(&local), remote_change: Change::None, extra: GlobalStateMergeExtra { local, remote } })
            }
            None => Ok(IAcceptResult { content: None, local_change: Change::None, remote_change: Change::None, extra: GlobalStateMergeExtra::default() }),
        }
    }
}

impl SynchroniserImpl for GlobalStateSynchroniser {
    type Preview = GlobalStatePreviewExtra;
    type Accept = GlobalStateMergeExtra;

    fn version(&self) -> f64 {
        GLOBAL_STATE_DATA_VERSION
    }

    fn generate_sync_preview(
        self: Rc<Self>,
        remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        is_remote_data_from_current_machine: bool,
        _user_data_sync_configuration: IUserDataSyncConfiguration,
        _token: CancellationToken,
    ) -> Promise<Vec<IGlobalStateResourcePreview>> {
        async_fn(async move {
            let remote_global_state = match &remote_user_data.sync_data {
                Some(sync_data) => Some(parse_global_state(&sync_data.content)?),
                None => None,
            };

            // Use remote data as last sync data if last sync data does not exist and remote data is from same machine
            let last_sync_user_data = if last_sync_user_data.is_none() && is_remote_data_from_current_machine { Some(remote_user_data.clone()) } else { last_sync_user_data };
            let last_sync_global_state = match last_sync_user_data.as_ref().and_then(|last_sync_user_data| last_sync_user_data.sync_data.as_ref()) {
                Some(sync_data) => Some(parse_global_state(&sync_data.content)?),
                None => None,
            };

            let mut local_global_state = self.local_global_state_provider.get_local_global_state(&self.sync_resource.profile).await?;

            if remote_global_state.is_some() {
                self.services.log_service.trace(&format!("{}: Merging remote ui state with local ui state...", self.sync_resource_log_label));
            } else {
                self.services.log_service.trace(&format!("{}: Remote ui state does not exist. Synchronizing ui state for the first time.", self.sync_resource_log_label));
            }

            let storage_keys = self.get_storage_keys(last_sync_global_state.as_ref()).await?;
            let MergeResult { local, remote } = merge(
                &local_global_state.storage,
                remote_global_state.as_ref().map(|state| &state.storage),
                last_sync_global_state.as_ref().map(|state| &state.storage),
                storage_keys.for_merge(),
                &*self.services.log_service,
            );
            let preview_result = IAcceptResult { content: None, local_change: local_change_of(&local), remote_change: remote_change_of(&remote), extra: GlobalStateMergeExtra { local, remote } };

            let local_content = stringify(&mut local_global_state, false);
            let base_content = match last_sync_global_state {
                Some(mut last_sync_global_state) => stringify(&mut last_sync_global_state, false),
                None => local_content.clone(),
            };
            let remote_content = remote_global_state.map(|mut remote_global_state| stringify(&mut remote_global_state, false));
            Ok(vec![IResourcePreview {
                base_resource: self.resources.base.clone(),
                base_content: Some(base_content),
                local_resource: self.resources.local.clone(),
                local_content: Some(local_content),
                remote_resource: self.resources.remote.clone(),
                remote_content,
                preview_resource: self.resources.preview.clone(),
                local_change: preview_result.local_change,
                remote_change: preview_result.remote_change,
                accepted_resource: self.resources.accepted.clone(),
                extra: GlobalStatePreviewExtra { local_user_data: local_global_state, preview_result, storage_keys },
            }])
        })
    }

    fn has_remote_changed(self: Rc<Self>, last_sync_user_data: IRemoteUserData) -> Promise<bool> {
        async_fn(async move {
            let Some(sync_data) = &last_sync_user_data.sync_data else { return Ok(true) };
            let last_sync_global_state = parse_global_state(&sync_data.content)?;
            let local_global_state = self.local_global_state_provider.get_local_global_state(&self.sync_resource.profile).await?;
            let storage_keys = self.get_storage_keys(Some(&last_sync_global_state)).await?;
            let MergeResult { remote, .. } = merge(
                &local_global_state.storage,
                Some(&last_sync_global_state.storage),
                Some(&last_sync_global_state.storage),
                storage_keys.for_merge(),
                &*self.services.log_service,
            );
            Ok(remote.all.is_some())
        })
    }

    fn get_merge_result(self: Rc<Self>, resource_preview: IGlobalStateResourcePreview, _token: CancellationToken) -> Promise<IMergeResult<GlobalStateMergeExtra>> {
        Promise::resolved(IMergeResult { result: resource_preview.extra.preview_result, has_conflicts: false })
    }

    fn get_accept_result(self: Rc<Self>, resource_preview: IGlobalStateResourcePreview, resource: URI, _content: Option<Option<String>>, _token: CancellationToken) -> Promise<IGlobalStateResourceMergeResult> {
        async_fn(async move {
            let is_same = |other: &URI| self.services.ext_uri.is_equal(Some(&resource), Some(other), false);

            /* Accept local resource */
            if is_same(&self.resources.local) {
                return self.accept_local(&resource_preview);
            }

            /* Accept remote resource */
            if is_same(&self.resources.remote) {
                return self.accept_remote(&resource_preview);
            }

            /* Accept preview resource */
            if is_same(&self.resources.preview) {
                return Ok(resource_preview.extra.preview_result.clone());
            }

            Err(Error::new(format!("Invalid Resource: {}", resource.to_string(false).to_string_lossy())))
        })
    }

    fn apply_result(
        self: Rc<Self>,
        mut remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        resource_previews: Vec<(IGlobalStateResourcePreview, IGlobalStateResourceMergeResult)>,
        force: bool,
    ) -> Promise<()> {
        async_fn(async move {
            let (resource_preview, accept_result) = &resource_previews[0];
            let local_user_data = &resource_preview.extra.local_user_data;
            let (local_change, remote_change) = (accept_result.local_change, accept_result.remote_change);
            let GlobalStateMergeExtra { local, remote } = &accept_result.extra;
            let label = &self.sync_resource_log_label;

            if local_change == Change::None && remote_change == Change::None {
                self.services.log_service.info(&format!("{label}: No changes found during synchronizing ui state."));
            }

            if local_change != Change::None {
                // update local
                self.services.log_service.trace(&format!("{label}: Updating local ui state..."));
                self.backup_local(&stringify_json(&global_state_to_json(local_user_data))).await?;
                self.local_global_state_provider.write_local_global_state(local, &self.sync_resource.profile).await?;
                self.services.log_service.info(&format!("{label}: Updated local ui state"));
            }

            if remote_change != Change::None {
                // update remote
                self.services.log_service.trace(&format!("{label}: Updating remote ui state..."));
                let content = stringify_json(&storage_wrapper_json(remote.all.as_ref()));
                remote_user_data = self.update_remote_user_data(content, if force { None } else { Some(remote_user_data.r#ref.clone()) }).await?;
                let listed = |name: &str, keys: &[JsString]| if keys.is_empty() { String::new() } else { format!(" {name}: {}.", join_keys(keys)) };
                self.services.log_service.info(&format!(
                    "{label}: Updated remote ui state.{}{}{}",
                    listed("Added", &remote.added),
                    listed("Updated", &remote.updated),
                    listed("Removed", &remote.removed)
                ));
            }

            if last_sync_user_data.as_ref().map(|last_sync_user_data| &last_sync_user_data.r#ref) != Some(&remote_user_data.r#ref) {
                // update last sync
                self.services.log_service.trace(&format!("{label}: Updating last synchronized ui state..."));
                self.update_last_sync_user_data(remote_user_data, Vec::new()).await?;
                self.services.log_service.info(&format!("{label}: Updated last synchronized ui state"));
            }
            Ok(())
        })
    }

    fn has_local_data(self: Rc<Self>) -> Promise<bool> {
        async_fn(async move {
            if let Ok(global_state) = self.local_global_state_provider.get_local_global_state(&self.sync_resource.profile).await {
                let locale_key = JsString::from(format!("{ARGV_STORAGE_PREFIX}.locale"));
                let locale = global_state.storage.get(&locale_key).map(|value| value.value.to_string_lossy());
                if global_state.storage.len() > 1 || locale.as_deref() != Some("en") {
                    return Ok(true);
                }
            }
            Ok(false)
        })
    }

    fn resolve_content(self: Rc<Self>, uri: URI) -> Promise<Option<String>> {
        if !self.resources.is_virtual_resource(&self.services.ext_uri, &uri) {
            return Promise::resolved(None);
        }
        let preview_content = self.resolve_preview_content(&uri);
        async_fn(async move {
            match preview_content.await? {
                Some(content) if !content.is_empty() => {
                    let mut global_state = parse_global_state(&content)?;
                    Ok(Some(stringify(&mut global_state, true)))
                }
                other => Ok(other),
            }
        })
    }
}

/// `GlobalStateInitializer`. Dá `Deref` para o [`AbstractInitializer`].
pub struct GlobalStateInitializer {
    base: Rc<AbstractInitializer>,
}

impl Deref for GlobalStateInitializer {
    type Target = AbstractInitializer;

    fn deref(&self) -> &AbstractInitializer {
        &self.base
    }
}

impl GlobalStateInitializer {
    pub fn new(
        storage_service: Rc<dyn StorageService>,
        file_service: Rc<FileService>,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        environment_service: Rc<dyn EnvironmentService>,
        log_service: LogServiceRef,
        ext_uri: ExtUri,
    ) -> Rc<Self> {
        let base = AbstractInitializer::new(SyncResource::GlobalState, user_data_profiles_service, environment_service, log_service, file_service, storage_service, ext_uri);
        let this = Rc::new(Self { base });
        let weak: Weak<dyn InitializerImpl> = Rc::downgrade(&this) as Weak<dyn InitializerImpl>;
        this.base.bind(weak);
        this
    }
}

impl InitializerImpl for GlobalStateInitializer {
    fn do_initialize(&self, remote_user_data: IRemoteUserData) -> Promise<()> {
        let base = self.base.clone();
        async_fn(async move {
            let remote_global_state = match &remote_user_data.sync_data {
                Some(sync_data) => Some(parse_global_state(&sync_data.content)?),
                None => None,
            };
            let Some(remote_global_state) = remote_global_state else {
                base.log_service.info("Skipping initializing global state because remote global state does not exist.");
                return Ok(());
            };

            let mut argv: IndexMap<String, JsString> = IndexMap::new();
            let is_default_profile = base.storage_service.has_scope(&StorageSwitchTarget::Profile(storage_profile(&base.user_data_profiles_service.default_profile())));
            let mut storage: Vec<(String, JsString, StorageScope)> = Vec::new();
            for (key, remote_value) in remote_global_state.storage.iter() {
                let key_text = key.to_string_lossy();
                if let Some(argv_property) = key_text.strip_prefix(ARGV_STORAGE_PREFIX) {
                    argv.insert(argv_property.to_string(), remote_value.value.clone());
                } else {
                    let is_shared_scope = remote_value.scope == Some(StorageScope::ApplicationShared);
                    if is_shared_scope && !is_default_profile {
                        continue; // Skip APPLICATION_SHARED keys for non-default profiles
                    }
                    let scope = if is_shared_scope { StorageScope::ApplicationShared } else { StorageScope::Profile };
                    if base.storage_service.get(&key_text, scope, None).is_none() {
                        storage.push((key_text, remote_value.value.clone(), scope));
                    }
                }
            }

            if !argv.is_empty() {
                let argv_resource = base.environment_service.argv_resource();
                let mut content = "{}".to_string();
                if let Ok(file_content) = base.file_service.read_file(&argv_resource, None, None).await {
                    content = file_content.value.to_string();
                }
                for (argv_property, value) in &argv {
                    content = edit(&content, &[EditSegment::Property(JsString::from(argv_property.as_str()))], Some(Value::String(value.clone())), &FormattingOptions::default());
                }
                base.file_service.write_file(&argv_resource, FileContentSource::from(content.as_str()), None).await?;
            }

            if !storage.is_empty() {
                let storage_entries: Vec<StorageEntry> = storage
                    .into_iter()
                    .map(|(key, value, scope)| StorageEntry { key, value: StorageValue::String(value.to_string_lossy()), scope, target: StorageTarget::User })
                    .collect();
                base.storage_service.store_all(&storage_entries, true);
            }
            Ok(())
        })
    }
}

/// `UserDataSyncStoreTypeSynchronizer`: grava no servidor o tipo do serviço de sincronização (`insiders` ou
/// `stable`) como uma chave do estado global.
#[derive(Clone)]
pub struct UserDataSyncStoreTypeSynchronizer {
    user_data_sync_store_client: Rc<UserDataSyncStoreClient>,
    storage_service: Rc<dyn StorageService>,
    environment_service: Rc<dyn EnvironmentService>,
    file_service: Rc<FileService>,
    log_service: LogServiceRef,
}

impl UserDataSyncStoreTypeSynchronizer {
    pub fn new(
        user_data_sync_store_client: Rc<UserDataSyncStoreClient>,
        storage_service: Rc<dyn StorageService>,
        environment_service: Rc<dyn EnvironmentService>,
        file_service: Rc<FileService>,
        log_service: LogServiceRef,
    ) -> Self {
        Self { user_data_sync_store_client, storage_service, environment_service, file_service, log_service }
    }

    /// `getSyncStoreType`.
    pub fn get_sync_store_type(&self, user_data: &IUserData) -> Result<Option<UserDataSyncStoreType>, Error> {
        let remote_global_state = Self::parse_global_state(user_data)?;
        let store_type = remote_global_state.and_then(|state| state.storage.get(&JsString::from(SYNC_SERVICE_URL_TYPE)).map(|value| value.value.to_string_lossy()));
        Ok(match store_type.as_deref() {
            Some("insiders") => Some(UserDataSyncStoreType::Insiders),
            Some("stable") => Some(UserDataSyncStoreType::Stable),
            _ => None,
        })
    }

    /// `sync`: repete uma vez se o servidor recusou por haver uma versão remota nova.
    pub fn sync(&self, user_data_sync_store_type: UserDataSyncStoreType) -> Promise<()> {
        let (this, sync_headers) = (self.clone(), create_sync_headers(&generate_uuid()));
        async_fn(async move {
            match this.do_sync(user_data_sync_store_type, sync_headers.clone()).await {
                Err(error) => {
                    if UserDataSyncError::from_error(&error).is_some_and(|sync_error| sync_error.code == UserDataSyncErrorCode::PreconditionFailed) {
                        this.log_service.info("Failed to synchronize UserDataSyncStoreType as there is a new remote version available. Synchronizing again...");
                        return this.do_sync(user_data_sync_store_type, sync_headers).await;
                    }
                    Err(error)
                }
                ok => ok,
            }
        })
    }

    fn do_sync(&self, user_data_sync_store_type: UserDataSyncStoreType, sync_headers: IHeaders) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            // Read the global state from remote
            let global_state_user_data = this.user_data_sync_store_client.read_resource(ServerResource::Sync(SyncResource::GlobalState), None, None, Some(sync_headers.clone())).await?;
            let mut remote_global_state = match Self::parse_global_state(&global_state_user_data)? {
                Some(remote_global_state) => remote_global_state,
                None => IGlobalState { storage: StorageDictionary::new() },
            };

            // Update the sync store type
            remote_global_state.storage.insert(
                JsString::from(SYNC_SERVICE_URL_TYPE),
                IStorageValue { version: GLOBAL_STATE_DATA_VERSION, value: JsString::from(user_data_sync_store_type.as_str()), scope: None },
            );

            // Write the global state to remote
            let machine_id = get_service_machine_id(this.environment_service.as_ref(), &this.file_service, Some(this.storage_service.clone())).await?;
            let sync_data_to_update = ISyncData { version: GLOBAL_STATE_DATA_VERSION, machine_id: Some(machine_id), content: stringify(&mut remote_global_state, false) };
            this.user_data_sync_store_client
                .write_resource(ServerResource::Sync(SyncResource::GlobalState), &stringify_json(&sync_data_to_update.to_json()), Some(&global_state_user_data.r#ref), None, Some(sync_headers))
                .await?;
            Ok(())
        })
    }

    /// `parseGlobalState`.
    fn parse_global_state(user_data: &IUserData) -> Result<Option<IGlobalState>, Error> {
        let Some(content) = user_data.content.as_deref().filter(|content| !content.is_empty()) else { return Ok(None) };
        let sync_data = parse_json(content)?;
        match ISyncData::from_json(&sync_data) {
            Some(sync_data) => Ok(Some(parse_global_state(&sync_data.content)?)),
            None => Err(Error::new("Invalid remote data")),
        }
    }
}
