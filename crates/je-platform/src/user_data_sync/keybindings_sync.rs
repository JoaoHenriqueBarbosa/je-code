//! Porte de `src/vs/platform/userDataSync/common/keybindingsSync.ts`: o sincronizador de `keybindings.json`
//! (sobre o `merge` de `keybindingsMerge`) e o inicializador dele.
//!
//! Decisões de tradução (as da base estão em [`super::abstract_synchronizer`] e
//! [`super::abstract_json_synchronizer`]):
//! - `ISyncContent` (`{ mac?, linux?, windows?, all? }`) é um objeto JSON; `OS` é o sistema detectado na partida
//!   ([`Platform::detect`]).
//! - `ILastSyncUserData.platformSpecific` é a entrada `platformSpecific` do estado da última sincronização, que
//!   [`IRemoteUserData::extras`] carrega.
//! - `IKeybindingsResourcePreview` é o mesmo [`IJsonResourcePreview`] da base JSON (o original os declara com a
//!   mesma forma), e `getAcceptResult` e `resolveContent` são os de [`PreviewResources`].
//! - `hasLocalData` tem um `catch` que só dispararia se `parse` lançasse; o `parse` do porte não lança, e
//!   `getLocalFileContent` já engole o erro de leitura, então o `catch` não existe aqui.
//! - `KeybindingsInitializer.isEmpty` lê o `settingsResource` (e não o de keybindings), como o original; o
//!   conteúdo de settings é um objeto, nunca um array não vazio, então só um erro de leitura diferente de
//!   "não encontrado" faz dele não vazio.
//! - `trim()` é o `trim` do JavaScript (o U+FEFF conta como espaço, o U+0085 não).
//! - `JSON.parse(syncContent || '{}')` com um valor que não é objeto vale como objeto vazio (o original lançaria
//!   `TypeError` ao atribuir).

use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::arrays::is_non_empty_array;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Truthy;
use je_base::errors::Error;
use je_base::js_string;
use je_base::json::{self, Object, Value};
use je_base::lifecycle::IDisposable;
use je_base::nls::localize;
use je_base::platform::{OperatingSystem, Platform};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;

use super::abstract_json_synchronizer::{FileMergePreviewExtra, IJsonResourcePreview, PreviewResources, get_sync_content_property};
use super::abstract_synchronizer::{
    AbstractInitializer, AbstractJsonFileSynchroniser, IAcceptResult, IMergeResult, IResourcePreview, InitializerImpl, SynchroniserImpl, SynchroniserServices, parse_json, stringify_json,
};
use super::keybindings_merge::merge;
use super::user_data_sync::{
    CONFIG_SYNC_KEYBINDINGS_PER_PLATFORM, Change, IRemoteUserData, IUserDataSyncConfiguration, IUserDataSyncLogService, IUserDataSyncResource, IUserDataSyncUtilService, SyncResource,
    UserDataSyncError, UserDataSyncErrorCode,
};
use crate::environment::environment::EnvironmentService;
use crate::files::file_service::{FileContentSource, FileService};
use crate::files::files::{FileOperationError, FileOperationResult};
use crate::log::{LogMessage, LogServiceRef};
use crate::storage::StorageService;
use crate::user_data_profile::{IUserDataProfilesService, UserDataProfile};

/// A chave de `ISyncContent` do sistema operacional.
fn os_property(os: OperatingSystem) -> &'static str {
    match os {
        OperatingSystem::Macintosh => "mac",
        OperatingSystem::Linux => "linux",
        OperatingSystem::Windows => "windows",
    }
}

/// `getKeybindingsContentFromSyncContent`: o conteúdo de `all` (sem `platform_specific`) ou o do sistema atual.
pub fn get_keybindings_content_from_sync_content(sync_content: &str, platform_specific: bool, log_service: &IUserDataSyncLogService) -> Option<String> {
    let property = if platform_specific { os_property(Platform::detect().os) } else { "all" };
    get_sync_content_property(sync_content, property, log_service)
}

/// `str.trim()` do JavaScript.
fn js_trim(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    JsString::from(js_string::trim(&units)).to_string_lossy()
}

/// `parse(content)` é um array não vazio.
fn is_non_empty_keybindings(content: &str) -> bool {
    let mut errors = Vec::new();
    matches!(json::parse_str(content, &mut errors), Some(Value::Array(keybindings)) if is_non_empty_array(Some(keybindings.as_slice())))
}

/// `KeybindingsSynchroniser`. Dá `Deref` para o [`AbstractJsonFileSynchroniser`].
pub struct KeybindingsSynchroniser {
    json_file_synchroniser: AbstractJsonFileSynchroniser<KeybindingsSynchroniser>,
    resources: PreviewResources,
}

impl Deref for KeybindingsSynchroniser {
    type Target = AbstractJsonFileSynchroniser<KeybindingsSynchroniser>;

    fn deref(&self) -> &Self::Target {
        &self.json_file_synchroniser
    }
}

impl IDisposable for KeybindingsSynchroniser {
    fn dispose(&self) {
        self.json_file_synchroniser.dispose();
    }
}

impl KeybindingsSynchroniser {
    pub fn new(profile: UserDataProfile, collection: Option<String>, services: SynchroniserServices, user_data_sync_util_service: Rc<dyn IUserDataSyncUtilService>) -> Rc<Self> {
        let ext_uri = services.ext_uri.clone();
        let configuration_service = services.configuration_service.clone();
        let keybindings_resource = profile.keybindings_resource.clone();
        let json_file_synchroniser =
            AbstractJsonFileSynchroniser::new(keybindings_resource, IUserDataSyncResource { sync_resource: SyncResource::Keybindings, profile }, collection, services, user_data_sync_util_service);
        let preview_resource = ext_uri.join_path(&json_file_synchroniser.sync_preview_folder, &["keybindings.json"]);
        let this = Rc::new(Self { json_file_synchroniser, resources: PreviewResources::new(preview_resource) });
        this.bind(Rc::downgrade(&this));

        let weak = Rc::downgrade(&this.base());
        this.register(configuration_service.on_did_change_configuration().listen(move |event| {
            if event.affects_configuration(CONFIG_SYNC_KEYBINDINGS_PER_PLATFORM, None)
                && let Some(base) = weak.upgrade()
            {
                base.trigger_local_change();
            }
        }));
        this
    }

    /// `syncKeybindingsPerPlatform`.
    fn sync_keybindings_per_platform(&self) -> bool {
        self.services.configuration_service.get_value(Some(CONFIG_SYNC_KEYBINDINGS_PER_PLATFORM), None).is_truthy()
    }

    /// O erro de conteúdo inválido de `generateSyncPreview` e `applyResult`.
    fn invalid_content_error(&self) -> Error {
        UserDataSyncError::new(
            localize("errorInvalidSettings", "Unable to sync keybindings because the content in the file is not valid. Please open the file and correct it.", &[]),
            UserDataSyncErrorCode::LocalInvalidContent,
            Some(self.resource),
        )
        .into_error()
    }

    /// `getKeybindingsContentFromLastSyncUserData`.
    fn get_keybindings_content_from_last_sync_user_data(&self, last_sync_user_data: &IRemoteUserData) -> Option<String> {
        let sync_data = last_sync_user_data.sync_data.as_ref()?;
        let per_platform = self.sync_keybindings_per_platform();

        // Return null if there is a change in platform specific property from last time sync.
        let platform_specific = last_sync_user_data.extras.iter().find(|(key, _)| key == "platformSpecific").map(|(_, value)| value);
        if platform_specific.is_some_and(|platform_specific| !matches!(platform_specific, Value::Bool(value) if *value == per_platform)) {
            return None;
        }

        get_keybindings_content_from_sync_content(&sync_data.content, per_platform, &self.services.log_service)
    }

    /// `toSyncContent`.
    fn to_sync_content(&self, keybindings_content: &str, sync_content: Option<&str>) -> String {
        let parsed = match parse_json(sync_content.filter(|sync_content| !sync_content.is_empty()).unwrap_or("{}")) {
            Ok(Value::Object(parsed)) => parsed,
            Ok(_) => Object::new(),
            Err(error) => {
                self.services.log_service.error_args(&LogMessage::from(&error), &[]);
                Object::new()
            }
        };
        let mut parsed: Object = if self.sync_keybindings_per_platform() {
            parsed.iter().filter(|(key, _)| key.to_string_lossy() != "all").map(|(key, value)| (key.clone(), value.clone())).collect()
        } else {
            let mut parsed = parsed;
            parsed.insert(JsString::from("all"), Value::from(keybindings_content));
            parsed
        };
        parsed.insert(JsString::from(os_property(Platform::detect().os)), Value::from(keybindings_content));
        stringify_json(&Value::Object(parsed))
    }
}

impl SynchroniserImpl for KeybindingsSynchroniser {
    type Preview = FileMergePreviewExtra;
    type Accept = ();

    /* Version 2: Change settings from `sync.${setting}` to `settingsSync.{setting}` */
    fn version(&self) -> f64 {
        2.0
    }

    fn generate_sync_preview(
        self: Rc<Self>,
        remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        is_remote_data_from_current_machine: bool,
        user_data_sync_configuration: IUserDataSyncConfiguration,
        _token: CancellationToken,
    ) -> Promise<Vec<IJsonResourcePreview>> {
        async_fn(async move {
            let per_platform = user_data_sync_configuration.keybindings_per_platform.unwrap_or_else(|| self.sync_keybindings_per_platform());
            let remote_content =
                remote_user_data.sync_data.as_ref().and_then(|sync_data| get_keybindings_content_from_sync_content(&sync_data.content, per_platform, &self.services.log_service));

            // Use remote data as last sync data if last sync data does not exist and remote data is from same machine
            let last_sync_user_data = if last_sync_user_data.is_none() && is_remote_data_from_current_machine { Some(remote_user_data.clone()) } else { last_sync_user_data };
            let last_sync_content: Option<String> = last_sync_user_data.as_ref().and_then(|last_sync_user_data| self.get_keybindings_content_from_last_sync_user_data(last_sync_user_data));

            // Get file content last to get the latest
            let file_content = self.get_local_file_content().await?;
            let formatting_options = self.get_formatting_options().await?;

            let mut merged_content: Option<String> = None;
            let mut has_local_changed = false;
            let mut has_remote_changed = false;
            let mut has_conflicts = false;

            if let Some(remote_content) = remote_content.as_deref().filter(|remote_content| !remote_content.is_empty()) {
                let local_content = file_content.as_ref().map(|file_content| file_content.value.to_string()).filter(|local_content| !local_content.is_empty()).unwrap_or_else(|| "[]".to_string());
                if self.has_errors(&local_content, true) {
                    return Err(self.invalid_content_error());
                }

                if last_sync_content.as_deref().is_none_or(str::is_empty) // First time sync
                    || last_sync_content.as_deref() != Some(local_content.as_str()) // Local has forwarded
                    || last_sync_content.as_deref() != Some(remote_content)
                // Remote has forwarded
                {
                    self.services.log_service.trace(&format!("{}: Merging remote keybindings with local keybindings...", self.sync_resource_log_label));
                    let result = merge(&local_content, remote_content, last_sync_content.as_deref(), &formatting_options, &*self.user_data_sync_util_service).await?;
                    // Sync only if there are changes
                    if result.has_changes {
                        has_conflicts = result.has_conflicts;
                        has_local_changed = has_conflicts || result.merge_content != local_content;
                        has_remote_changed = has_conflicts || result.merge_content != remote_content;
                        merged_content = Some(result.merge_content);
                    }
                }
            }
            // First time syncing to remote
            else if let Some(file_content) = &file_content {
                self.services.log_service.trace(&format!("{}: Remote keybindings does not exist. Synchronizing keybindings for the first time.", self.sync_resource_log_label));
                merged_content = Some(file_content.value.to_string());
                has_remote_changed = true;
            }

            let preview_result = IMergeResult {
                result: IAcceptResult {
                    content: if has_conflicts { last_sync_content.clone() } else { merged_content },
                    local_change: if has_local_changed {
                        if file_content.is_some() { Change::Modified } else { Change::Added }
                    } else {
                        Change::None
                    },
                    remote_change: if has_remote_changed { Change::Modified } else { Change::None },
                    extra: (),
                },
                has_conflicts,
            };

            let local_content = file_content.as_ref().map(|file_content| file_content.value.to_string());
            Ok(vec![IResourcePreview {
                base_resource: self.resources.base.clone(),
                base_content: last_sync_content,

                local_resource: self.resources.local.clone(),
                local_content,
                local_change: preview_result.local_change,

                remote_resource: self.resources.remote.clone(),
                remote_content,
                remote_change: preview_result.remote_change,

                preview_resource: self.resources.preview.clone(),
                accepted_resource: self.resources.accepted.clone(),
                extra: FileMergePreviewExtra { file_content, preview_result },
            }])
        })
    }

    fn has_remote_changed(self: Rc<Self>, last_sync_user_data: IRemoteUserData) -> Promise<bool> {
        async_fn(async move {
            let Some(last_sync_content) = self.get_keybindings_content_from_last_sync_user_data(&last_sync_user_data) else { return Ok(true) };

            let file_content = self.get_local_file_content().await?;
            let local_content = file_content.map(|file_content| file_content.value.to_string()).unwrap_or_default();
            let formatting_options = self.get_formatting_options().await?;
            let local_content = if local_content.is_empty() { "[]" } else { local_content.as_str() };
            let result = merge(local_content, &last_sync_content, Some(&last_sync_content), &formatting_options, &*self.user_data_sync_util_service).await?;
            Ok(result.has_conflicts || result.merge_content != last_sync_content)
        })
    }

    fn get_merge_result(self: Rc<Self>, resource_preview: IJsonResourcePreview, _token: CancellationToken) -> Promise<IMergeResult<()>> {
        Promise::resolved(resource_preview.extra.preview_result)
    }

    fn get_accept_result(self: Rc<Self>, resource_preview: IJsonResourcePreview, resource: URI, content: Option<Option<String>>, _token: CancellationToken) -> Promise<IAcceptResult<()>> {
        async_fn(async move { self.resources.accept_result(&self.services.ext_uri, &resource_preview, &resource, content) })
    }

    fn apply_result(
        self: Rc<Self>,
        mut remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        resource_previews: Vec<(IJsonResourcePreview, IAcceptResult<()>)>,
        force: bool,
    ) -> Promise<()> {
        async_fn(async move {
            let (resource_preview, accept_result) = &resource_previews[0];
            let file_content = resource_preview.extra.file_content.clone();
            let (mut content, local_change, remote_change) = (accept_result.content.clone(), accept_result.local_change, accept_result.remote_change);
            let label = &self.sync_resource_log_label;

            if local_change == Change::None && remote_change == Change::None {
                self.services.log_service.info(&format!("{label}: No changes found during synchronizing keybindings."));
            }

            if let Some(text) = &content {
                let trimmed = js_trim(text);
                let trimmed = if trimmed.is_empty() { "[]".to_string() } else { trimmed };
                if self.has_errors(&trimmed, true) {
                    return Err(self.invalid_content_error());
                }
                content = Some(trimmed);
            }
            // `content || '[]'`
            let content_or_empty_array = content.as_deref().filter(|content| !content.is_empty()).unwrap_or("[]").to_string();

            if local_change != Change::None {
                self.services.log_service.trace(&format!("{label}: Updating local keybindings..."));
                if let Some(file_content) = &file_content {
                    self.backup_local(&self.to_sync_content(&file_content.value.to_string(), None)).await?;
                }
                self.update_local_file_content(&content_or_empty_array, file_content.clone(), force).await?;
                self.services.log_service.info(&format!("{label}: Updated local keybindings"));
            }

            if remote_change != Change::None {
                self.services.log_service.trace(&format!("{label}: Updating remote keybindings..."));
                let remote_contents = self.to_sync_content(&content_or_empty_array, remote_user_data.sync_data.as_ref().map(|sync_data| sync_data.content.as_str()));
                remote_user_data = self.update_remote_user_data(remote_contents, if force { None } else { Some(remote_user_data.r#ref.clone()) }).await?;
                self.services.log_service.info(&format!("{label}: Updated remote keybindings"));
            }

            // Delete the preview
            let _ = self.services.file_service.del(&self.resources.preview, None).await;

            if last_sync_user_data.as_ref().map(|last_sync_user_data| &last_sync_user_data.r#ref) != Some(&remote_user_data.r#ref) {
                self.services.log_service.trace(&format!("{label}: Updating last synchronized keybindings..."));
                let platform_specific = vec![("platformSpecific".to_string(), Value::Bool(self.sync_keybindings_per_platform()))];
                self.update_last_sync_user_data(remote_user_data, platform_specific).await?;
                self.services.log_service.info(&format!("{label}: Updated last synchronized keybindings"));
            }
            Ok(())
        })
    }

    fn has_local_data(self: Rc<Self>) -> Promise<bool> {
        async_fn(async move {
            let local_file_content = self.get_local_file_content().await?;
            Ok(local_file_content.is_some_and(|local_file_content| is_non_empty_keybindings(&local_file_content.value.to_string())))
        })
    }

    fn resolve_content(self: Rc<Self>, uri: URI) -> Promise<Option<String>> {
        self.resources.resolve_content::<Self>(&self, &uri)
    }
}

/// `KeybindingsInitializer`. Dá `Deref` para o [`AbstractInitializer`].
pub struct KeybindingsInitializer {
    base: Rc<AbstractInitializer>,
}

impl Deref for KeybindingsInitializer {
    type Target = AbstractInitializer;

    fn deref(&self) -> &AbstractInitializer {
        &self.base
    }
}

impl KeybindingsInitializer {
    pub fn new(
        file_service: Rc<FileService>,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        environment_service: Rc<dyn EnvironmentService>,
        log_service: LogServiceRef,
        storage_service: Rc<dyn StorageService>,
        ext_uri: ExtUri,
    ) -> Rc<Self> {
        let base = AbstractInitializer::new(SyncResource::Keybindings, user_data_profiles_service, environment_service, log_service, file_service, storage_service, ext_uri);
        let this = Rc::new(Self { base });
        let weak: Weak<dyn InitializerImpl> = Rc::downgrade(&this) as Weak<dyn InitializerImpl>;
        this.base.bind(weak);
        this
    }

    /// `isEmpty`.
    fn is_empty(base: &AbstractInitializer) -> Promise<bool> {
        let (file_service, settings_resource) = (base.file_service.clone(), base.user_data_profiles_service.default_profile().settings_resource);
        async_fn(async move {
            match file_service.read_file(&settings_resource, None, None).await {
                Ok(file_content) => Ok(!is_non_empty_keybindings(&file_content.value.to_string())),
                Err(error) => Ok(FileOperationError::from_error(&error).is_some_and(|file_error| file_error.file_operation_result == FileOperationResult::FileNotFound)),
            }
        })
    }
}

impl InitializerImpl for KeybindingsInitializer {
    fn do_initialize(&self, remote_user_data: IRemoteUserData) -> Promise<()> {
        let base = self.base.clone();
        async_fn(async move {
            let keybindings_content = remote_user_data.sync_data.as_ref().and_then(|sync_data| get_keybindings_content_from_sync_content(&sync_data.content, true, &base.log_service));
            let Some(keybindings_content) = keybindings_content.filter(|keybindings_content| !keybindings_content.is_empty()) else {
                base.log_service.info("Skipping initializing keybindings because remote keybindings does not exist.");
                return Ok(());
            };

            let is_empty = KeybindingsInitializer::is_empty(&base).await?;
            if !is_empty {
                base.log_service.info("Skipping initializing keybindings because local keybindings exist.");
                return Ok(());
            }

            let keybindings_resource = base.user_data_profiles_service.default_profile().keybindings_resource;
            base.file_service.write_file(&keybindings_resource, FileContentSource::from(keybindings_content.as_str()), None).await?;

            base.update_last_sync_user_data(remote_user_data, Vec::new()).await
        })
    }
}
