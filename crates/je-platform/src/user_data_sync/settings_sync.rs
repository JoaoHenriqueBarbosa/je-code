//! Porte de `src/vs/platform/userDataSync/common/settingsSync.ts`: o sincronizador de `settings.json` (sobre o
//! `merge` de `settingsMerge`) e o inicializador dele.
//!
//! Decisões de tradução (as da base estão em [`super::abstract_synchronizer`] e
//! [`super::abstract_json_synchronizer`]):
//! - `ISettingsResourcePreview` é o [`IJsonResourcePreview`] da base JSON (`fileContent` e `previewResult`); o
//!   resultado de aceitar não acrescenta campos (`()`).
//! - `ISettingsSyncContent` (`{ settings }`) é [`SettingsSyncContent`]; `parseSettingsSyncContent` migra o conteúdo
//!   antigo (que era o próprio texto das settings) como o original.
//! - `IExtensionManagementService` só é lido por `getInstalled(type, location)` e pelos dois eventos de instalação
//!   e remoção, e `getInstalled` só serve para `getIgnoredSettingsForExtension(e.manifest)`, que pede o
//!   `IExtensionManifest` do `je-extensions` (que o `je-platform` não conhece). O trait
//!   [`IExtensionManagementService`] é esse recorte: devolve, para cada extensão instalada, o que
//!   `getIgnoredSettingsForExtension` dá, e os eventos já trazem o que os filtros do original leem
//!   (`!!local` de cada resultado de instalação; `!e.error` da remoção).
//! - `getIgnoredSettingForSystemExtensions` e `getIgnoredSettingForUserExtensions` diferem só no tipo e no local
//!   das extensões: são um método só ([`SettingsSynchroniser::get_ignored_setting_for_extensions`]).
//! - Os três `Promise<string[]> | undefined` do cache de settings ignoradas ficam num [`RefCell`]; o ouvinte de
//!   instalação e remoção que invalida o cache das extensões do usuário se desfaz depois do primeiro disparo,
//!   como no original.
//! - `configurationService.reloadConfiguration(ConfigurationTarget.USER_LOCAL)` é `reload_configuration()`: o
//!   serviço de configuração do porte recarrega a configuração do usuário local, sem alvo.
//! - `hasLocalData` tem um `catch` que só dispararia se `getLocalFileContent` lançasse; ele engole o erro de
//!   leitura, então o `catch` não existe aqui.
//! - `trim()` é o `trim` do JavaScript ([`js_trim`]).

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::arrays::distinct;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::{Event, any};
use je_base::json::{Object, Value};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

use super::abstract_json_synchronizer::{FileMergePreviewExtra, IJsonResourcePreview, PreviewResources};
use super::abstract_synchronizer::{
    AbstractInitializer, AbstractJsonFileSynchroniser, IAcceptResult, IMergeResult, IResourcePreview, InitializerImpl, SynchroniserImpl, SynchroniserServices, parse_json, stringify_json,
};
use super::keybindings_sync::js_trim;
use super::settings_merge::{get_ignored_settings, is_empty as is_settings_empty, merge, update_ignored_settings};
use super::user_data_sync::{
    Change, IRemoteUserData, IUserDataSyncConfiguration, IUserDataSyncLogService, IUserDataSyncResource, IUserDataSyncUtilService, RefOrUserData, SyncResource, USER_DATA_SYNC_CONFIGURATION_SCOPE,
    UserDataSyncError, UserDataSyncErrorCode,
};
use crate::configuration::models::ConfigurationModelParser;
use crate::environment::environment::EnvironmentService;
use crate::files::file_service::{FileContentSource, FileService};
use crate::files::files::{FileOperationError, FileOperationResult};
use crate::log::{LogMessage, LogServiceRef};
use crate::storage::StorageService;
use crate::user_data_profile::{IUserDataProfilesService, UserDataProfile};
use je_base::resources::ExtUri;

// region: conteúdo de sincronização

/// `ISettingsSyncContent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsSyncContent {
    pub settings: String,
}

/// `isSettingsSyncContent`: um objeto só com `settings`, um texto não vazio. Devolve o texto.
fn settings_of_sync_content(thing: &Value) -> Option<String> {
    match thing {
        Value::Object(object) if object.len() == 1 => match object.get("settings") {
            Some(Value::String(settings)) if !settings.is_empty() => Some(settings.to_string_lossy()),
            _ => None,
        },
        _ => None,
    }
}

/// `parseSettingsSyncContent`: um conteúdo que não tem a forma de [`SettingsSyncContent`] é o formato antigo, em
/// que o conteúdo era o texto das settings (migra).
pub fn parse_settings_sync_content(sync_content: &str) -> Result<SettingsSyncContent, Error> {
    let parsed = parse_json(sync_content)?;
    Ok(SettingsSyncContent { settings: settings_of_sync_content(&parsed).unwrap_or_else(|| sync_content.to_string()) })
}

/// `parseSettingsSyncContent` dos dois chamadores (o sincronizador e o inicializador): o erro vai para o log.
fn parse_settings_sync_content_or_log(sync_content: &str, log_service: &IUserDataSyncLogService) -> Option<SettingsSyncContent> {
    match parse_settings_sync_content(sync_content) {
        Ok(content) => Some(content),
        Err(error) => {
            log_service.error_args(&LogMessage::from(&error), &[]);
            None
        }
    }
}

/// `JSON.stringify(toSettingsSyncContent(settings))`: `{ "settings": ... }`.
fn settings_sync_content_text(settings: &str) -> String {
    let mut content = Object::new();
    content.insert(JsString::from("settings"), Value::String(JsString::from(settings)));
    stringify_json(&Value::Object(content))
}

// endregion

// region: extensões

/// `ExtensionType`: as extensões do sistema e as do usuário.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionType {
    System,
    User,
}

/// O recorte de `IExtensionManagementService` que o `SettingsSynchroniser` lê (ver a nota do módulo).
pub trait IExtensionManagementService {
    /// `getInstalled(type, extensionsProfileLocation)` mapeado por `getIgnoredSettingsForExtension(e.manifest)`:
    /// as settings ignoradas de cada extensão instalada.
    fn get_installed_ignored_settings(&self, extension_type: ExtensionType, extensions_profile_location: Option<&URI>) -> Promise<Vec<Vec<JsString>>>;

    /// `onDidInstallExtensions`: para cada resultado, se a extensão instalada é local (`!!local`).
    fn on_did_install_extensions(&self) -> Event<Vec<bool>>;

    /// `onDidUninstallExtension`: se a remoção falhou (`!!e.error`).
    fn on_did_uninstall_extension(&self) -> Event<bool>;
}

service_id!(pub I_EXTENSION_MANAGEMENT_SERVICE: dyn IExtensionManagementService = "extensionManagementService");

/// O cache das settings ignoradas: `coreIgnoredSettings`, `systemExtensionsIgnoredSettings` e
/// `userExtensionsIgnoredSettings`.
#[derive(Default)]
struct IgnoredSettingsCache {
    core: Option<Promise<Vec<JsString>>>,
    system_extensions: Option<Promise<Vec<JsString>>>,
    user_extensions: Option<Promise<Vec<JsString>>>,
}

// endregion

/// `SettingsSynchroniser`. Dá `Deref` para o [`AbstractJsonFileSynchroniser`].
pub struct SettingsSynchroniser {
    json_file_synchroniser: AbstractJsonFileSynchroniser<SettingsSynchroniser>,
    profile: UserDataProfile,
    extension_management_service: Rc<dyn IExtensionManagementService>,
    /// `previewResource`, `baseResource`, `localResource`, `remoteResource` e `acceptedResource`.
    pub resources: PreviewResources,
    ignored_settings: RefCell<IgnoredSettingsCache>,
}

impl Deref for SettingsSynchroniser {
    type Target = AbstractJsonFileSynchroniser<SettingsSynchroniser>;

    fn deref(&self) -> &Self::Target {
        &self.json_file_synchroniser
    }
}

impl IDisposable for SettingsSynchroniser {
    fn dispose(&self) {
        self.json_file_synchroniser.dispose();
    }
}

impl SettingsSynchroniser {
    pub fn new(
        profile: UserDataProfile,
        collection: Option<String>,
        services: SynchroniserServices,
        user_data_sync_util_service: Rc<dyn IUserDataSyncUtilService>,
        extension_management_service: Rc<dyn IExtensionManagementService>,
    ) -> Rc<Self> {
        let ext_uri = services.ext_uri.clone();
        let json_file_synchroniser = AbstractJsonFileSynchroniser::new(
            profile.settings_resource.clone(),
            IUserDataSyncResource { sync_resource: SyncResource::Settings, profile: profile.clone() },
            collection,
            services,
            user_data_sync_util_service,
        );
        let preview_resource = ext_uri.join_path(&json_file_synchroniser.sync_preview_folder, &["settings.json"]);
        let this = Rc::new(Self {
            json_file_synchroniser,
            profile,
            extension_management_service,
            resources: PreviewResources::new(preview_resource),
            ignored_settings: RefCell::new(IgnoredSettingsCache::default()),
        });
        this.bind(Rc::downgrade(&this));
        this
    }

    /// `getRemoteUserDataSyncConfiguration`: a seção `settingsSync` das settings que estão no servidor.
    pub fn get_remote_user_data_sync_configuration(self: &Rc<Self>, ref_or_latest_data: Option<RefOrUserData>) -> Promise<IUserDataSyncConfiguration> {
        let this = self.clone();
        async_fn(async move {
            let last_sync_user_data = this.get_last_sync_user_data().await?;
            let remote_user_data = this.get_latest_remote_user_data(ref_or_latest_data, last_sync_user_data).await?;
            let remote_settings_sync_content = this.get_settings_sync_content(&remote_user_data);
            let mut parser = ConfigurationModelParser::new(USER_DATA_SYNC_CONFIGURATION_SCOPE, this.services.log_service.clone());
            if let Some(remote_settings_sync_content) = remote_settings_sync_content.filter(|content| !content.settings.is_empty()) {
                parser.parse(Some(remote_settings_sync_content.settings.as_str()), None);
            }
            Ok(IUserDataSyncConfiguration::from_dynamic(&parser.configuration_model().get_value(USER_DATA_SYNC_CONFIGURATION_SCOPE)))
        })
    }

    /// `getSettingsSyncContent`.
    fn get_settings_sync_content(&self, remote_user_data: &IRemoteUserData) -> Option<SettingsSyncContent> {
        remote_user_data.sync_data.as_ref().and_then(|sync_data| parse_settings_sync_content_or_log(&sync_data.content, &self.services.log_service))
    }

    /// `validateContent`.
    fn validate_content(&self, content: &str) -> Result<(), Error> {
        if self.has_errors(content, false) {
            return Err(UserDataSyncError::new(
                localize("errorInvalidSettings", "Unable to sync settings as there are errors/warning in settings file.", &[]),
                UserDataSyncErrorCode::LocalInvalidContent,
                Some(self.resource),
            )
            .into_error());
        }
        Ok(())
    }

    // region: settings ignoradas

    /// `getIgnoredSettings(content?)`: as settings que não se sincronizam (as do núcleo, as das extensões, as das
    /// máquinas e as que o usuário ignora).
    fn get_ignored_settings(self: &Rc<Self>, content: Option<String>) -> Promise<Vec<JsString>> {
        let this = self.clone();
        async_fn(async move {
            let (core, system_extensions, user_extensions) = this.default_ignored_settings_promises();
            let default_ignored_settings: Vec<JsString> = Promise::all(vec![core, system_extensions, user_extensions]).await?.into_iter().flatten().collect();
            Ok(get_ignored_settings(&default_ignored_settings, this.services.configuration_service.as_ref(), content.as_deref()))
        })
    }

    /// Os três `Promise<string[]>` guardados de `getIgnoredSettings`, criados na primeira chamada.
    fn default_ignored_settings_promises(self: &Rc<Self>) -> (Promise<Vec<JsString>>, Promise<Vec<JsString>>, Promise<Vec<JsString>>) {
        let core = self.ignored_settings.borrow_mut().core.get_or_insert_with(|| self.user_data_sync_util_service.resolve_default_core_ignored_settings()).clone();
        let system_extensions = self
            .ignored_settings
            .borrow_mut()
            .system_extensions
            .get_or_insert_with(|| self.get_ignored_setting_for_extensions(ExtensionType::System, None))
            .clone();
        let existing_user_extensions = self.ignored_settings.borrow().user_extensions.clone();
        let user_extensions = match existing_user_extensions {
            Some(user_extensions) => user_extensions,
            None => {
                let user_extensions = self.get_ignored_setting_for_extensions(ExtensionType::User, Some(self.profile.extensions_resource.clone()));
                self.ignored_settings.borrow_mut().user_extensions = Some(user_extensions.clone());
                self.invalidate_user_extensions_on_change();
                user_extensions
            }
        };
        (core, system_extensions, user_extensions)
    }

    /// Descarta o cache das extensões do usuário na próxima instalação ou remoção (uma vez só).
    fn invalidate_user_extensions_on_change(self: &Rc<Self>) {
        let service = &self.extension_management_service;
        let installed = service.on_did_install_extensions().filter(|results| results.iter().any(|has_local| *has_local), None).signal();
        let uninstalled = service.on_did_uninstall_extension().filter(|has_error| !*has_error, None).signal();
        let listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let (weak, slot): (Weak<Self>, _) = (Rc::downgrade(self), listener.clone());
        let disposable = any(vec![installed, uninstalled]).listen(move |_| {
            if let Some(disposable) = slot.borrow_mut().take() {
                disposable.dispose();
            }
            if let Some(this) = weak.upgrade() {
                this.ignored_settings.borrow_mut().user_extensions = None;
            }
        });
        *listener.borrow_mut() = Some(disposable.clone());
        self.register(disposable);
    }

    /// `getIgnoredSettingForSystemExtensions` e `getIgnoredSettingForUserExtensions`.
    fn get_ignored_setting_for_extensions(&self, extension_type: ExtensionType, extensions_profile_location: Option<URI>) -> Promise<Vec<JsString>> {
        let extension_management_service = self.extension_management_service.clone();
        async_fn(async move {
            let ignored_settings_of_extensions = extension_management_service.get_installed_ignored_settings(extension_type, extensions_profile_location.as_ref()).await?;
            let ignored_settings: Vec<JsString> = ignored_settings_of_extensions.into_iter().flatten().collect();
            Ok(distinct(&ignored_settings, |setting| setting.clone()))
        })
    }

    // endregion

    /// `resolvePreviewContent` do sincronizador: o conteúdo do preview sem as settings ignoradas.
    fn resolve_settings_preview_content(self: &Rc<Self>, resource: &URI) -> Promise<Option<String>> {
        let (this, resource) = (self.clone(), resource.clone());
        async_fn(async move {
            let content = this.resolve_preview_content(&resource).await?;
            match content {
                Some(text) if !text.is_empty() => {
                    let formatting_options = this.get_formatting_options().await?;
                    // remove ignored settings from the preview content
                    let ignored_settings = this.get_ignored_settings(None).await?;
                    Ok(Some(update_ignored_settings(&text, "{}", &ignored_settings, &formatting_options)))
                }
                other => Ok(other),
            }
        })
    }
}

impl SynchroniserImpl for SettingsSynchroniser {
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
        _user_data_sync_configuration: IUserDataSyncConfiguration,
        _token: CancellationToken,
    ) -> Promise<Vec<IJsonResourcePreview>> {
        async_fn(async move {
            let file_content = self.get_local_file_content().await?;
            let formatting_options = self.get_formatting_options().await?;
            let remote_settings_sync_content = self.get_settings_sync_content(&remote_user_data);

            // Use remote data as last sync data if last sync data does not exist and remote data is from same machine
            let last_sync_user_data = if last_sync_user_data.is_none() && is_remote_data_from_current_machine { Some(remote_user_data.clone()) } else { last_sync_user_data };
            let last_settings_sync_content = last_sync_user_data.as_ref().and_then(|last_sync_user_data| self.get_settings_sync_content(last_sync_user_data));
            let ignored_settings = self.get_ignored_settings(None).await?;

            let mut merged_content: Option<String> = None;
            let mut has_local_changed = false;
            let mut has_remote_changed = false;
            let mut has_conflicts = false;

            if let Some(remote_settings_sync_content) = &remote_settings_sync_content {
                let local_content = match &file_content {
                    Some(file_content) => js_trim(&file_content.value.to_string()),
                    None => "{}".to_string(),
                };
                let local_content = if local_content.is_empty() { "{}".to_string() } else { local_content };
                self.validate_content(&local_content)?;
                self.services.log_service.trace(&format!("{}: Merging remote settings with local settings...", self.sync_resource_log_label));
                let result = merge(
                    &local_content,
                    &remote_settings_sync_content.settings,
                    last_settings_sync_content.as_ref().map(|content| content.settings.as_str()),
                    &ignored_settings,
                    &[],
                    &formatting_options,
                );
                merged_content = result.local_content.clone().filter(|content| !content.is_empty()).or_else(|| result.remote_content.clone());
                has_local_changed = result.local_content.is_some();
                has_remote_changed = result.remote_content.is_some();
                has_conflicts = result.has_conflicts;
            }
            // First time syncing to remote
            else if let Some(file_content) = &file_content {
                self.services.log_service.trace(&format!("{}: Remote settings does not exist. Synchronizing settings for the first time.", self.sync_resource_log_label));
                let content = js_trim(&file_content.value.to_string());
                let content = if content.is_empty() { "{}".to_string() } else { content };
                self.validate_content(&content)?;
                merged_content = Some(content);
                has_remote_changed = true;
            }

            let local_content = file_content.as_ref().map(|file_content| file_content.value.to_string());
            let base_content = last_settings_sync_content.map(|content| content.settings);

            let preview_result = IMergeResult {
                result: IAcceptResult {
                    content: if has_conflicts { base_content.clone() } else { merged_content },
                    local_change: if has_local_changed { Change::Modified } else { Change::None },
                    remote_change: if has_remote_changed { Change::Modified } else { Change::None },
                    extra: (),
                },
                has_conflicts,
            };

            Ok(vec![IResourcePreview {
                base_resource: self.resources.base.clone(),
                base_content,

                local_resource: self.resources.local.clone(),
                local_content,
                local_change: preview_result.local_change,

                remote_resource: self.resources.remote.clone(),
                remote_content: remote_settings_sync_content.map(|content| content.settings),
                remote_change: preview_result.remote_change,

                preview_resource: self.resources.preview.clone(),
                accepted_resource: self.resources.accepted.clone(),
                extra: FileMergePreviewExtra { file_content, preview_result },
            }])
        })
    }

    fn has_remote_changed(self: Rc<Self>, last_sync_user_data: IRemoteUserData) -> Promise<bool> {
        async_fn(async move {
            let Some(last_settings_sync_content) = self.get_settings_sync_content(&last_sync_user_data) else { return Ok(true) };

            let file_content = self.get_local_file_content().await?;
            let local_content = file_content.map(|file_content| js_trim(&file_content.value.to_string())).unwrap_or_default();
            let ignored_settings = self.get_ignored_settings(None).await?;
            let formatting_options = self.get_formatting_options().await?;
            let local_content = if local_content.is_empty() { "{}" } else { local_content.as_str() };
            let result = merge(local_content, &last_settings_sync_content.settings, Some(&last_settings_sync_content.settings), &ignored_settings, &[], &formatting_options);
            Ok(result.remote_content.is_some())
        })
    }

    fn get_merge_result(self: Rc<Self>, resource_preview: IJsonResourcePreview, _token: CancellationToken) -> Promise<IMergeResult<()>> {
        async_fn(async move {
            let format_utils = self.get_formatting_options().await?;
            let ignored_settings = self.get_ignored_settings(None).await?;
            let preview_result = resource_preview.extra.preview_result;
            Ok(IMergeResult {
                result: IAcceptResult {
                    // remove ignored settings from the preview content
                    content: preview_result.content.as_ref().filter(|content| !content.is_empty()).map(|content| update_ignored_settings(content, "{}", &ignored_settings, &format_utils)),
                    local_change: preview_result.local_change,
                    remote_change: preview_result.remote_change,
                    extra: (),
                },
                has_conflicts: preview_result.has_conflicts,
            })
        })
    }

    fn get_accept_result(self: Rc<Self>, resource_preview: IJsonResourcePreview, resource: URI, content: Option<Option<String>>, _token: CancellationToken) -> Promise<IAcceptResult<()>> {
        async_fn(async move {
            let formatting_options = self.get_formatting_options().await?;
            let ignored_settings = self.get_ignored_settings(None).await?;
            let is_same = |other: &URI| self.services.ext_uri.is_equal(Some(&resource), Some(other), false);
            // `fileContent ? fileContent.value.toString() : '{}'`
            let local_or_empty = || resource_preview.extra.file_content.as_ref().map_or_else(|| "{}".to_string(), |file_content| file_content.value.to_string());

            /* Accept local resource */
            if is_same(&self.resources.local) {
                return Ok(IAcceptResult {
                    /* Remove ignored settings */
                    content: resource_preview
                        .extra
                        .file_content
                        .as_ref()
                        .map(|file_content| update_ignored_settings(&file_content.value.to_string(), "{}", &ignored_settings, &formatting_options)),
                    local_change: Change::None,
                    remote_change: Change::Modified,
                    extra: (),
                });
            }

            /* Accept remote resource */
            if is_same(&self.resources.remote) {
                return Ok(IAcceptResult {
                    /* Update ignored settings from local file content */
                    content: resource_preview.remote_content.as_ref().map(|remote_content| update_ignored_settings(remote_content, &local_or_empty(), &ignored_settings, &formatting_options)),
                    local_change: Change::Modified,
                    remote_change: Change::None,
                    extra: (),
                });
            }

            /* Accept preview resource */
            if is_same(&self.resources.preview) {
                return Ok(match content {
                    None => {
                        let preview_result = &resource_preview.extra.preview_result;
                        IAcceptResult { content: preview_result.content.clone(), local_change: preview_result.local_change, remote_change: preview_result.remote_change, extra: () }
                    }
                    Some(content) => IAcceptResult {
                        /* Add ignored settings from local file content */
                        content: content.map(|content| update_ignored_settings(&content, &local_or_empty(), &ignored_settings, &formatting_options)),
                        local_change: Change::Modified,
                        remote_change: Change::Modified,
                        extra: (),
                    },
                });
            }

            Err(Error::new(format!("Invalid Resource: {}", resource.to_string(false).to_string_lossy())))
        })
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
            let (local_change, remote_change) = (accept_result.local_change, accept_result.remote_change);
            let label = &self.sync_resource_log_label;

            if local_change == Change::None && remote_change == Change::None {
                self.services.log_service.info(&format!("{label}: No changes found during synchronizing settings."));
            }

            // `content = content ? content.trim() : '{}'; content = content || '{}';`
            let mut content = accept_result.content.as_deref().map(js_trim).filter(|content| !content.is_empty()).unwrap_or_else(|| "{}".to_string());
            self.validate_content(&content)?;

            if local_change != Change::None {
                self.services.log_service.trace(&format!("{label}: Updating local settings..."));
                if let Some(file_content) = &file_content {
                    self.backup_local(&settings_sync_content_text(&file_content.value.to_string())).await?;
                }
                self.update_local_file_content(&content, file_content.clone(), force).await?;
                self.services.configuration_service.reload_configuration().await?;
                self.services.log_service.info(&format!("{label}: Updated local settings"));
            }

            if remote_change != Change::None {
                let format_utils = self.get_formatting_options().await?;
                // Update ignored settings from remote
                let remote_settings_sync_content = self.get_settings_sync_content(&remote_user_data);
                let ignored_settings = self.get_ignored_settings(Some(content.clone())).await?;
                content = update_ignored_settings(
                    &content,
                    remote_settings_sync_content.as_ref().map_or("{}", |remote_settings_sync_content| remote_settings_sync_content.settings.as_str()),
                    &ignored_settings,
                    &format_utils,
                );
                self.services.log_service.trace(&format!("{label}: Updating remote settings..."));
                remote_user_data = self.update_remote_user_data(settings_sync_content_text(&content), if force { None } else { Some(remote_user_data.r#ref.clone()) }).await?;
                self.services.log_service.info(&format!("{label}: Updated remote settings"));
            }

            // Delete the preview
            let _ = self.services.file_service.del(&self.resources.preview, None).await;

            if last_sync_user_data.as_ref().map(|last_sync_user_data| &last_sync_user_data.r#ref) != Some(&remote_user_data.r#ref) {
                self.services.log_service.trace(&format!("{label}: Updating last synchronized settings..."));
                self.update_last_sync_user_data(remote_user_data, Vec::new()).await?;
                self.services.log_service.info(&format!("{label}: Updated last synchronized settings"));
            }
            Ok(())
        })
    }

    fn has_local_data(self: Rc<Self>) -> Promise<bool> {
        async_fn(async move {
            let local_file_content = self.get_local_file_content().await?;
            Ok(local_file_content.is_some_and(|local_file_content| !is_settings_empty(&local_file_content.value.to_string())))
        })
    }

    fn resolve_content(self: Rc<Self>, uri: URI) -> Promise<Option<String>> {
        if self.resources.is_virtual_resource(&self.services.ext_uri, &uri) {
            return self.resolve_settings_preview_content(&uri);
        }
        Promise::resolved(None)
    }
}

/// `SettingsInitializer`. Dá `Deref` para o [`AbstractInitializer`].
pub struct SettingsInitializer {
    base: Rc<AbstractInitializer>,
}

impl Deref for SettingsInitializer {
    type Target = AbstractInitializer;

    fn deref(&self) -> &AbstractInitializer {
        &self.base
    }
}

impl SettingsInitializer {
    pub fn new(
        file_service: Rc<FileService>,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        environment_service: Rc<dyn EnvironmentService>,
        log_service: LogServiceRef,
        storage_service: Rc<dyn StorageService>,
        ext_uri: ExtUri,
    ) -> Rc<Self> {
        let base = AbstractInitializer::new(SyncResource::Settings, user_data_profiles_service, environment_service, log_service, file_service, storage_service, ext_uri);
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
                Ok(file_content) => Ok(is_settings_empty(&js_trim(&file_content.value.to_string()))),
                Err(error) => Ok(FileOperationError::from_error(&error).is_some_and(|file_error| file_error.file_operation_result == FileOperationResult::FileNotFound)),
            }
        })
    }
}

impl InitializerImpl for SettingsInitializer {
    fn do_initialize(&self, remote_user_data: IRemoteUserData) -> Promise<()> {
        let base = self.base.clone();
        async_fn(async move {
            let settings_sync_content = remote_user_data.sync_data.as_ref().and_then(|sync_data| parse_settings_sync_content_or_log(&sync_data.content, &base.log_service));
            let Some(settings_sync_content) = settings_sync_content else {
                base.log_service.info("Skipping initializing settings because remote settings does not exist.");
                return Ok(());
            };

            let is_empty = SettingsInitializer::is_empty(&base).await?;
            if !is_empty {
                base.log_service.info("Skipping initializing settings because local settings exist.");
                return Ok(());
            }

            let settings_resource = base.user_data_profiles_service.default_profile().settings_resource;
            base.file_service.write_file(&settings_resource, FileContentSource::from(settings_sync_content.settings.as_str()), None).await?;

            base.update_last_sync_user_data(remote_user_data, Vec::new()).await
        })
    }
}
