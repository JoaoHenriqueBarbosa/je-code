//! Porte de `src/vs/platform/userDataSync/common/extensionsSync.ts`: o sincronizador de extensões
//! (`ExtensionsSynchroniser`), o provedor das extensões locais (`LocalExtensionsProvider`), o inicializador abstrato
//! (`AbstractExtensionsInitializer`) e as funções `parseExtensions` e `stringify`.
//!
//! Por que mora no `je-workbench` e não no `je-platform`: o sincronizador lê os serviços de gerência de extensões, o
//! de habilitação global, o de armazenamento de extensão e a galeria (todos daqui) e o `merge` e o `SyncExtension` são do
//! `je-extensions`; os dois dependem do `je-platform`, então o caminho inverso criaria um ciclo. O `je-platform` o conhece
//! só pela [`ExtensionsSynchroniserFactory`], que o `UserDataSyncService` recebe em
//! `SynchroniserExtraServices::extensions_synchroniser_factory`; a fábrica deste módulo é
//! [`WorkbenchExtensionsSynchroniserFactory`].
//!
//! Decisões de tradução (as da base estão em `je_platform::user_data_sync::abstract_synchronizer`):
//! - `IExtensionResourcePreview` é um [`IResourcePreview`] com [`ExtensionsPreviewExtra`] (`localExtensions`,
//!   `remoteExtensions`, `skippedExtensions`, `builtinExtensions` e `previewResult`); `IExtensionResourceMergeResult` é
//!   um [`IAcceptResult`] com [`ExtensionsMergeExtra`] (`local` e `remote`, os do `merge`).
//! - `ILastSyncUserData` (os dados remotos com `skippedExtensions` e `builtinExtensions`) é o `IRemoteUserData` com
//!   esses dois campos nos `extras`, lidos por [`skipped_extensions_of`] e [`builtin_extensions_of`] e gravados em
//!   `updateLastSyncUserData`.
//! - `JSON.parse` e `JSON.stringify` das extensões passam por `serde_json` sobre o [`SyncExtension`] de
//!   `je-extensions`; os `extras` (JSON do `je-base`) e o `toFormattedString` convertem por texto.
//! - `stringify(extensions, format)` ordena a lista NO PRÓPRIO vetor (o `extensions.sort` do original), então o preview
//!   guarda as listas local, remota e da última sincronização já ordenadas: [`stringify`] recebe `&mut [SyncExtension]`.
//! - `instantiationService.createChild(new ServiceCollection([IStorageService, storageService]))` do
//!   `withProfileScopedServices` é a construção direta do `GlobalExtensionEnablementService` e do
//!   `ExtensionStorageService` sobre o armazenamento do perfil, descartados no fim. O `withProfileScopedStorageService`
//!   mora na struct concreta `AbstractUserDataProfileStorageService` (o trait não o tem), por isso
//!   [`ExtensionsSyncServices`] guarda a struct.
//! - `Promises.settled` é `je_base::async_::promises::settled`; as listas que as tarefas paralelas preenchem
//!   (`extensionsToInstall`, `removeFromSkipped`, `addToSkipped`) vivem num `Rc<RefCell<_>>` e nenhum empréstimo
//!   atravessa um `await`.
//! - O `try`/`catch` de `getLocalExtensions` em volta da leitura do estado da extensão não existe: nenhuma das duas
//!   chamadas lança aqui (o serviço de armazenamento devolve valor ausente em vez de lançar).
//! - `context` do `InstallOptions` é o `IndexMap` que `je-extensions` ganhou para isso.
//! - `AbstractExtensionsInitializer` é uma struct com os métodos protegidos do original públicos; quem a estende liga o
//!   seu `InitializerImpl` ao `base` (o `AbstractInitializer`).

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::async_::{async_fn, promises};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::json::Value as JsonValue;
use je_base::json_formatter::{FormattingOptions, to_formatted_string};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::strings::compare;
use je_base::uri::URI;
use je_extensions::extensions_merge::{ExtensionState, LocalMergeResult, LocalSyncExtension, MergeResult as ExtensionMergeResult, RemoteMergeResult, SyncExtension, merge};
use je_extensions::management::{
    DISABLED_EXTENSIONS_STORAGE_PATH, EXTENSION_INSTALL_SKIP_PUBLISHER_TRUST_CONTEXT, EXTENSION_INSTALL_SKIP_WALKTHROUGH_CONTEXT, EXTENSION_INSTALL_SOURCE_CONTEXT, ExtensionInfo,
    ExtensionInstallSource, ExtensionManagementErrorCode, ExtensionQueryOptions, GalleryExtension, InstallOptions, UninstallOptions,
};
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionType, is_application_scoped_extension};
use je_extensions::util::are_same_extensions;
use je_instantiation::Injectable;
use je_platform::environment::environment::EnvironmentService;
use je_platform::files::i_file_service::IFileService;
use je_platform::log::{LogArg, LogMessage, LogServiceRef};
use je_platform::product::IProductService;
use je_platform::storage::{StorageService, StorageValueChangeEvent};
use je_platform::user_data_profile::user_data_profile_storage_service::{AbstractUserDataProfileStorageService, IUserDataProfileStorageService};
use je_platform::user_data_profile::{IUserDataProfilesService, UserDataProfile};
use je_platform::user_data_sync::abstract_json_synchronizer::PreviewResources;
use je_platform::user_data_sync::abstract_synchronizer::{
    AbstractInitializer, AbstractSynchroniser, IAcceptResult, IMergeResult, IResourcePreview, SynchroniserImpl, SynchroniserServices, get_sync_resource_log_label, parse_json, stringify_json,
};
use je_platform::user_data_sync::user_data_sync::{Change, IRemoteUserData, ISyncData, IUserDataSyncConfiguration, IUserDataSyncLogService, IUserDataSyncResource, SyncResource};
use je_platform::user_data_sync::user_data_sync_service::{ExtensionsSynchroniserFactory, ExtensionsSynchroniserHandle};

use crate::services::extension_management::extension_gallery_service::IExtensionGalleryService;
use crate::services::extension_management::extension_install_service::{IExtensionInstallService, InstallExtensionInfo};
use crate::services::extension_management::extension_management::{ExtensionRef, IGlobalExtensionEnablementService, InstallExtensionSource};
use crate::services::extension_management::extension_storage::{ExtensionIdWithVersion, ExtensionStorageService, ExtensionStorageTarget, IExtensionStorageService};
use crate::services::extension_management::global_extension_enablement_service::GlobalExtensionEnablementService;
use crate::services::user_data_sync::ignored_extensions::IIgnoredExtensionsManagementService;

// region: tipos do preview

/// O que `IExtensionResourceMergeResult` acrescenta a `IAcceptResult`: `local` e `remote`, os do `merge`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExtensionsMergeExtra {
    pub local: LocalMergeResult,
    pub remote: Option<RemoteMergeResult>,
}

impl ExtensionsMergeExtra {
    /// `local.added.length > 0 || local.removed.length > 0 || local.updated.length > 0 ? Change.Modified : Change.None`.
    fn local_change(&self) -> Change {
        if !self.local.added.is_empty() || !self.local.removed.is_empty() || !self.local.updated.is_empty() { Change::Modified } else { Change::None }
    }

    /// `remote !== null ? Change.Modified : Change.None`.
    fn remote_change(&self) -> Change {
        if self.remote.is_some() { Change::Modified } else { Change::None }
    }
}

/// `IExtensionResourceMergeResult`.
pub type IExtensionResourceMergeResult = IAcceptResult<ExtensionsMergeExtra>;

/// O que `IExtensionResourcePreview` acrescenta a `IResourcePreview`.
#[derive(Clone, Debug)]
pub struct ExtensionsPreviewExtra {
    pub local_extensions: Vec<LocalSyncExtension>,
    pub remote_extensions: Option<Vec<SyncExtension>>,
    pub skipped_extensions: Vec<SyncExtension>,
    pub builtin_extensions: Option<Vec<ExtensionIdentifierInfo>>,
    pub preview_result: IExtensionResourceMergeResult,
}

/// `IExtensionResourcePreview`.
pub type IExtensionResourcePreview = IResourcePreview<ExtensionsPreviewExtra>;

/// O resultado do `merge` como `IExtensionResourceMergeResult`, com o conteúdo dado.
fn merge_to_accept_result(content: Option<String>, result: ExtensionMergeResult) -> IExtensionResourceMergeResult {
    let extra = ExtensionsMergeExtra { local: result.local, remote: result.remote };
    IAcceptResult { content, local_change: extra.local_change(), remote_change: extra.remote_change(), extra }
}

// endregion

// region: JSON

fn syntax_error(error: serde_json::Error) -> Error {
    Error::named("SyntaxError", error.to_string())
}

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// O texto JSON (de `serde_json`) como o JSON do `je-base`, o que os `extras` e o formatador usam.
fn text_to_json(text: &str) -> JsonValue {
    parse_json(text).expect("o texto que o serde_json escreveu é JSON válido")
}

fn sync_extensions_text(extensions: &[SyncExtension]) -> String {
    serde_json::to_string(extensions).expect("a serialização das extensões de sincronização não falha")
}

fn identifiers_text(identifiers: &[ExtensionIdentifierInfo]) -> String {
    serde_json::to_string(identifiers).expect("a serialização dos identificadores de extensão não falha")
}

/// `JSON.stringify(extensions.map(e => e.identifier.id))`.
fn ids_text<'a>(ids: impl Iterator<Item = &'a str>) -> String {
    serde_json::to_string(&ids.collect::<Vec<_>>()).expect("a serialização dos identificadores de extensão não falha")
}

/// `JSON.parse(content)` de uma lista de extensões.
fn parse_sync_extensions(content: &str) -> Result<Vec<SyncExtension>, Error> {
    serde_json::from_str(content).map_err(syntax_error)
}

/// `resourcePreview.remoteContent ? JSON.parse(resourcePreview.remoteContent) : null`.
fn parse_remote_content(content: Option<&str>) -> Result<Option<Vec<SyncExtension>>, Error> {
    content.filter(|content| !content.is_empty()).map(parse_sync_extensions).transpose()
}

/// `parseExtensions`.
pub fn parse_extensions(sync_data: &ISyncData) -> Result<Vec<SyncExtension>, Error> {
    parse_sync_extensions(&sync_data.content)
}

/// `stringify`: ordena a lista (as sem `uuid` primeiro, depois pelo `id`) e a escreve, compacta ou formatada.
pub fn stringify(extensions: &mut [SyncExtension], format: bool) -> String {
    extensions.sort_by(|e1, e2| {
        let (e1_uuid, e2_uuid) = (e1.identifier.uuid.as_deref().is_some_and(|uuid| !uuid.is_empty()), e2.identifier.uuid.as_deref().is_some_and(|uuid| !uuid.is_empty()));
        match (e1_uuid, e2_uuid) {
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
            _ => compare(&utf16(&e1.identifier.id), &utf16(&e2.identifier.id)).cmp(&0),
        }
    });
    let text = sync_extensions_text(extensions);
    if format { to_formatted_string(&text_to_json(&text), &FormattingOptions::default()).to_string_lossy() } else { text }
}

/// O valor dos `extras` do `ILastSyncUserData` que não é `null` nem `undefined`.
fn extra_value<'a>(last_sync_user_data: Option<&'a IRemoteUserData>, key: &str) -> Option<&'a JsonValue> {
    last_sync_user_data?.extras.iter().find(|(name, _)| name == key).map(|(_, value)| value).filter(|value| !matches!(value, JsonValue::Null))
}

/// `lastSyncUserData.skippedExtensions`.
fn skipped_extensions_of(last_sync_user_data: Option<&IRemoteUserData>) -> Result<Option<Vec<SyncExtension>>, Error> {
    extra_value(last_sync_user_data, "skippedExtensions").map(|value| serde_json::from_str::<Vec<SyncExtension>>(&stringify_json(value)).map_err(syntax_error)).transpose()
}

/// `lastSyncUserData.builtinExtensions`.
fn builtin_extensions_of(last_sync_user_data: Option<&IRemoteUserData>) -> Result<Option<Vec<ExtensionIdentifierInfo>>, Error> {
    extra_value(last_sync_user_data, "builtinExtensions").map(|value| serde_json::from_str::<Vec<ExtensionIdentifierInfo>>(&stringify_json(value)).map_err(syntax_error)).transpose()
}

/// `parseAndMigrateExtensions`: as migrações da versão 1 (`enabled` vira `disabled`) e da 2 (`installed` nas que não são
/// embutidas).
async fn parse_and_migrate_extensions(sync_data: ISyncData, extension_management_service: Rc<dyn IExtensionInstallService>) -> Result<Vec<SyncExtension>, Error> {
    let mut extensions: Vec<serde_json::Value> = serde_json::from_str(&sync_data.content).map_err(syntax_error)?;
    if sync_data.version == 1.0 || sync_data.version == 2.0 {
        let builtin_extensions: Vec<ExtensionRef> =
            extension_management_service.get_installed_filtered(Some(ExtensionType::System), None, None, None).await?.into_iter().filter(|extension| extension.is_builtin).collect();
        for extension in &mut extensions {
            let Some(object) = extension.as_object_mut() else { continue };
            // #region Migration from v1 (enabled -> disabled)
            if sync_data.version == 1.0 {
                if object.get("enabled") == Some(&serde_json::Value::Bool(false)) {
                    object.insert("disabled".to_owned(), serde_json::Value::Bool(true));
                }
                object.remove("enabled");
            }
            // #endregion

            // #region Migration from v2 (set installed property on extension)
            if sync_data.version == 2.0 {
                let identifier: ExtensionIdentifierInfo = serde_json::from_value(object.get("identifier").cloned().unwrap_or(serde_json::Value::Null)).map_err(syntax_error)?;
                if builtin_extensions.iter().all(|installed| !are_same_extensions(&installed.identifier, &identifier)) {
                    object.insert("installed".to_owned(), serde_json::Value::Bool(true));
                }
            }
            // #endregion
        }
    }
    serde_json::from_value(serde_json::Value::Array(extensions)).map_err(syntax_error)
}

// endregion

// region: serviços

/// Os serviços que o `ExtensionsSynchroniser` e o `LocalExtensionsProvider` recebem por decorador, além dos de
/// [`SynchroniserServices`].
#[derive(Clone)]
pub struct ExtensionsSyncServices {
    pub extension_management_service: Rc<dyn IExtensionInstallService>,
    pub ignored_extensions_management_service: Rc<dyn IIgnoredExtensionsManagementService>,
    pub extension_storage_service: Rc<dyn IExtensionStorageService>,
    pub user_data_profile_storage_service: Rc<AbstractUserDataProfileStorageService>,
    pub extension_gallery_service: Rc<dyn IExtensionGalleryService>,
    pub product_service: Rc<dyn IProductService>,
}

/// O que `getLocalExtensions` devolve: as extensões locais e os identificadores ignorados.
#[derive(Clone)]
pub struct LocalExtensionsResult {
    pub local_extensions: Vec<LocalSyncExtension>,
    pub ignored_extensions: Vec<String>,
}

/// O que as tarefas paralelas de `updateLocalExtensions` preenchem.
#[derive(Default)]
struct UpdatePlan {
    extensions_to_install: Vec<InstallExtensionInfo>,
    sync_extensions_to_install: IndexMap<String, SyncExtension>,
    remove_from_skipped: Vec<ExtensionIdentifierInfo>,
    add_to_skipped: Vec<SyncExtension>,
}

/// Os argumentos de log, textos.
fn log_args(args: &[&str]) -> Vec<LogArg> {
    args.iter().map(|arg| LogArg::from(*arg)).collect()
}

/// `extension.displayName || extension.identifier.id`.
fn display_name_or_id(display_name: &str, id: &str) -> String {
    if display_name.is_empty() { id.to_owned() } else { display_name.to_owned() }
}

/// `extensionEnablementService.getDisabledExtensions().some(disabledExtension => areSameExtensions(disabledExtension, identifier))`.
fn is_disabled(extension_enablement_service: &dyn IGlobalExtensionEnablementService, identifier: &ExtensionIdentifierInfo) -> bool {
    extension_enablement_service.get_disabled_extensions().iter().any(|disabled_extension| are_same_extensions(disabled_extension, identifier))
}

/// O caminho da URI do `extensions.json` do perfil, o `profileLocation` das opções.
fn profile_location_of(profile: &UserDataProfile) -> String {
    profile.extensions_resource.path().to_string_lossy()
}

// endregion

// region: LocalExtensionsProvider

/// `LocalExtensionsProvider`.
#[derive(Clone)]
pub struct LocalExtensionsProvider {
    services: ExtensionsSyncServices,
    log_service: IUserDataSyncLogService,
}

impl LocalExtensionsProvider {
    pub fn new(services: ExtensionsSyncServices, log_service: IUserDataSyncLogService) -> Self {
        Self { services, log_service }
    }

    /// `getInstalled(undefined, profile.extensionsResource)`.
    fn get_installed(&self, profile: &UserDataProfile) -> Promise<Vec<ExtensionRef>> {
        self.services.extension_management_service.get_installed_filtered(None, Some(&profile.extensions_resource), None, None)
    }

    /// `getLocalExtensions`.
    pub fn get_local_extensions(&self, profile: &UserDataProfile) -> Promise<LocalExtensionsResult> {
        let (this, profile) = (self.clone(), profile.clone());
        async_fn(async move {
            let installed_extensions = this.get_installed(&profile).await?;
            let ignored_extensions = this.services.ignored_extensions_management_service.get_ignored_extensions(&installed_extensions);
            let provider = this.clone();
            let local_extensions = this
                .with_profile_scoped_services(&profile, move |extension_enablement_service, extension_storage_service| {
                    let disabled_extensions = extension_enablement_service.get_disabled_extensions();
                    Promise::resolved(
                        installed_extensions
                            .iter()
                            .map(|extension| provider.to_local_sync_extension(extension, &disabled_extensions, extension_storage_service.as_ref()))
                            .collect::<Vec<_>>(),
                    )
                })
                .await?;
            Ok(LocalExtensionsResult { local_extensions, ignored_extensions })
        })
    }

    /// O corpo do `map` de `getLocalExtensions`: a extensão instalada como `ILocalSyncExtension`.
    fn to_local_sync_extension(&self, extension: &ExtensionRef, disabled_extensions: &[ExtensionIdentifierInfo], extension_storage_service: &dyn IExtensionStorageService) -> LocalSyncExtension {
        let identifier = &extension.identifier;
        let mut sync_extension = SyncExtension {
            identifier: identifier.clone(),
            version: extension.manifest.version.clone(),
            pinned: Some(extension.pinned),
            pre_release: Some(extension.pre_release),
            disabled: None,
            installed: None,
            is_application_scoped: None,
            state: None,
        };
        if extension.is_application_scoped && !is_application_scoped_extension(&extension.manifest) {
            sync_extension.is_application_scoped = Some(extension.is_application_scoped);
        }
        if self.services.product_service.built_in_extensions_enabled_with_auto_updates().iter().any(|id| id.to_lowercase() == identifier.id.to_lowercase()) {
            sync_extension.is_application_scoped = Some(true);
        }
        if disabled_extensions.iter().any(|disabled_extension| are_same_extensions(disabled_extension, identifier)) {
            sync_extension.disabled = Some(true);
        }
        if !extension.is_builtin {
            sync_extension.installed = Some(true);
        }
        if let Some(keys) = extension_storage_service.get_keys_for_sync(&ExtensionIdWithVersion { id: identifier.id.clone(), version: extension.manifest.version.clone() }) {
            let target = ExtensionStorageTarget::from(&extension.manifest);
            let extension_storage_state = match extension_storage_service.get_extension_state(&target, true) {
                Some(serde_json::Value::Object(state)) => state,
                _ => serde_json::Map::new(),
            };
            let mut state = ExtensionState::new();
            for (key, value) in extension_storage_state {
                if keys.contains(&key) {
                    state.insert(key, value);
                }
            }
            sync_extension.state = Some(state);
        }
        sync_extension
    }

    /// `updateLocalExtensions`: devolve as extensões que seguem puladas.
    pub fn update_local_extensions(
        &self,
        added: Vec<SyncExtension>,
        removed: Vec<ExtensionIdentifierInfo>,
        updated: Vec<SyncExtension>,
        skipped_extensions: Vec<SyncExtension>,
        profile: UserDataProfile,
    ) -> Promise<Vec<SyncExtension>> {
        let this = self.clone();
        async_fn(async move {
            let sync_resource_log_label = get_sync_resource_log_label(SyncResource::Extensions, &profile);
            let plan: Rc<RefCell<UpdatePlan>> = Rc::default();
            let installed_extensions = this.get_installed(&profile).await?;

            // 1. Sync extensions state first so that the storage is flushed and updated in all opened windows
            if !added.is_empty() || !updated.is_empty() {
                let all: Vec<SyncExtension> = added.iter().chain(updated.iter()).cloned().collect();
                let (provider, installed, scoped_profile, scoped_plan) = (this.clone(), installed_extensions.clone(), profile.clone(), plan.clone());
                this.with_profile_scoped_services(&profile, move |extension_enablement_service, extension_storage_service| {
                    let tasks: Vec<Promise<()>> = all
                        .into_iter()
                        .map(|e| {
                            let (provider, installed, profile, plan) = (provider.clone(), installed.clone(), scoped_profile.clone(), scoped_plan.clone());
                            let (extension_enablement_service, extension_storage_service) = (extension_enablement_service.clone(), extension_storage_service.clone());
                            async_fn(async move { provider.prepare_extension(&e, &installed, &profile, extension_enablement_service.as_ref(), extension_storage_service.as_ref(), &plan).await })
                        })
                        .collect();
                    async_fn(async move {
                        promises::settled(tasks).await?;
                        Ok(())
                    })
                })
                .await?;
            }

            // 2. Next uninstall the removed extensions
            if !removed.is_empty() {
                let extensions_to_remove: Vec<ExtensionRef> =
                    installed_extensions.iter().filter(|extension| !extension.is_builtin && removed.iter().any(|r| are_same_extensions(&extension.identifier, r))).cloned().collect();
                let tasks: Vec<Promise<()>> = extensions_to_remove
                    .into_iter()
                    .map(|extension_to_remove| {
                        let (provider, profile, plan, label) = (this.clone(), profile.clone(), plan.clone(), sync_resource_log_label.clone());
                        async_fn(async move {
                            let id = extension_to_remove.identifier.id.as_str();
                            provider.log_service.trace_args(&format!("{label}: Uninstalling local extension..."), &log_args(&[id]));
                            let options = UninstallOptions { profile_location: Some(profile_location_of(&profile)), donot_include_pack: true, donot_check_dependents: true, ..UninstallOptions::default() };
                            provider.services.extension_management_service.uninstall(&extension_to_remove, options).await?;
                            provider.log_service.info_args(&format!("{label}: Uninstalled local extension."), &log_args(&[id]));
                            plan.borrow_mut().remove_from_skipped.push(extension_to_remove.identifier.clone());
                            Ok(())
                        })
                    })
                    .collect();
                promises::settled(tasks).await?;
            }

            // 3. Install extensions at the end
            let extensions_to_install = std::mem::take(&mut plan.borrow_mut().extensions_to_install);
            let results = this.services.extension_management_service.install_gallery_extensions(extensions_to_install).await?;
            for result in results {
                let gallery: Option<&GalleryExtension> = match &result.source {
                    Some(InstallExtensionSource::Gallery(gallery)) => Some(gallery),
                    _ => None,
                };
                let gallery_version = gallery.map_or("", |gallery| gallery.version.as_str());
                let gallery_name = gallery.map_or_else(|| result.identifier.id.clone(), |gallery| display_name_or_id(&gallery.display_name, &gallery.identifier.id));
                if result.local.is_some() {
                    this.log_service.info_args(&format!("{sync_resource_log_label}: Installed extension."), &log_args(&[&result.identifier.id, gallery_version]));
                    plan.borrow_mut().remove_from_skipped.push(result.identifier.clone());
                } else {
                    let synced = plan.borrow().sync_extensions_to_install.get(&result.identifier.id.to_lowercase()).cloned();
                    if let Some(e) = synced {
                        plan.borrow_mut().add_to_skipped.push(e);
                        this.log_service.info_args(&format!("{sync_resource_log_label}: Skipped synchronizing extension"), &log_args(&[&gallery_name]));
                    }
                    if let Some(error) = &result.error {
                        use ExtensionManagementErrorCode::{Incompatible, IncompatibleApi, IncompatibleTargetPlatform, NotAllowed};
                        if matches!(error.code, Incompatible | IncompatibleApi | IncompatibleTargetPlatform | NotAllowed) {
                            this.log_service.info_args(&format!("{sync_resource_log_label}: Skipped synchronizing extension because the compatible extension is not found."), &log_args(&[&gallery_name]));
                        } else {
                            this.log_service.error_args(&LogMessage::from(error.message.as_str()), &[]);
                        }
                    }
                }
            }

            let plan = plan.borrow();
            let mut new_skipped_extensions: Vec<SyncExtension> = Vec::new();
            for skipped_extension in &skipped_extensions {
                if !plan.remove_from_skipped.iter().any(|e| are_same_extensions(e, &skipped_extension.identifier)) {
                    new_skipped_extensions.push(skipped_extension.clone());
                }
            }
            for skipped_extension in &plan.add_to_skipped {
                if !new_skipped_extensions.iter().any(|e| are_same_extensions(&e.identifier, &skipped_extension.identifier)) {
                    new_skipped_extensions.push(skipped_extension.clone());
                }
            }
            Ok(new_skipped_extensions)
        })
    }

    /// O corpo do `map` da etapa 1 de `updateLocalExtensions`: sincroniza o estado e a habilitação da extensão e, se for
    /// o caso, a põe na lista de instalação.
    async fn prepare_extension(
        &self,
        e: &SyncExtension,
        installed_extensions: &[ExtensionRef],
        profile: &UserDataProfile,
        extension_enablement_service: &dyn IGlobalExtensionEnablementService,
        extension_storage_service: &dyn IExtensionStorageService,
        plan: &RefCell<UpdatePlan>,
    ) -> Result<(), Error> {
        let label = get_sync_resource_log_label(SyncResource::Extensions, profile);
        let installed_extension = installed_extensions.iter().find(|installed| are_same_extensions(&installed.identifier, &e.identifier));
        let wants_disabled = e.disabled == Some(true);

        // Builtin Extension Sync: Enablement & State
        if let Some(installed_extension) = installed_extension.filter(|installed| installed.is_builtin) {
            if let Some(state) = &e.state
                && installed_extension.manifest.version == e.version
            {
                update_extension_state(
                    state,
                    &installed_extension.identifier.id,
                    &ExtensionStorageTarget::from(&installed_extension.manifest),
                    Some(&installed_extension.manifest.version),
                    extension_storage_service,
                );
            }
            if is_disabled(extension_enablement_service, &e.identifier) != wants_disabled {
                if wants_disabled {
                    self.log_service.trace_args(&format!("{label}: Disabling extension..."), &log_args(&[&e.identifier.id]));
                    extension_enablement_service.disable_extension(&e.identifier, None).await?;
                    self.log_service.info_args(&format!("{label}: Disabled extension"), &log_args(&[&e.identifier.id]));
                } else {
                    self.log_service.trace_args(&format!("{label}: Enabling extension..."), &log_args(&[&e.identifier.id]));
                    extension_enablement_service.enable_extension(&e.identifier, None).await?;
                    self.log_service.info_args(&format!("{label}: Enabled extension"), &log_args(&[&e.identifier.id]));
                }
            }
            plan.borrow_mut().remove_from_skipped.push(e.identifier.clone());
            return Ok(());
        }

        // User Extension Sync: Install/Update, Enablement & State
        let version: Option<String> = (e.pinned == Some(true)).then(|| e.version.clone()).filter(|version| !version.is_empty());
        let info = ExtensionInfo {
            id: e.identifier.id.clone(),
            uuid: e.identifier.uuid.clone(),
            version: version.clone(),
            pre_release: if version.is_some() { None } else { e.pre_release },
            has_pre_release: None,
            current_version: None,
        };
        let extension: Option<GalleryExtension> = self.services.extension_gallery_service.get_extensions(vec![info], ExtensionQueryOptions::default(), CancellationToken::None).await?.into_iter().next();

        /* Update extension state only if
         *	extension is installed and version is same as synced version or
         *	extension is not installed and installable
         */
        if let Some(state) = &e.state {
            let applicable = match installed_extension {
                Some(installed) => installed.manifest.version == e.version, /* Installed and remote has same version */
                None => extension.is_some(),                                /* Installable */
            };
            if applicable {
                match (installed_extension, &extension) {
                    (Some(installed), _) => update_extension_state(
                        state,
                        &installed.identifier.id,
                        &ExtensionStorageTarget::from(&installed.manifest),
                        Some(&installed.manifest.version),
                        extension_storage_service,
                    ),
                    (None, Some(gallery)) => update_extension_state(
                        state,
                        &gallery.identifier.id,
                        &ExtensionStorageTarget::Parts { publisher: gallery.publisher.clone(), name: gallery.name.clone() },
                        None,
                        extension_storage_service,
                    ),
                    (None, None) => {}
                }
            }
        }

        let Some(extension) = extension else {
            plan.borrow_mut().add_to_skipped.push(e.clone());
            self.log_service.info_args(&format!("{label}: Skipped synchronizing extension because the extension is not found."), &log_args(&[&e.identifier.id]));
            return Ok(());
        };

        let outcome: Result<(), Error> = async {
            if is_disabled(extension_enablement_service, &e.identifier) != wants_disabled {
                if wants_disabled {
                    self.log_service.trace_args(&format!("{label}: Disabling extension..."), &log_args(&[&e.identifier.id, &extension.version]));
                    extension_enablement_service.disable_extension(&extension.identifier, None).await?;
                    self.log_service.info_args(&format!("{label}: Disabled extension"), &log_args(&[&e.identifier.id, &extension.version]));
                } else {
                    self.log_service.trace_args(&format!("{label}: Enabling extension..."), &log_args(&[&e.identifier.id, &extension.version]));
                    extension_enablement_service.enable_extension(&extension.identifier, None).await?;
                    self.log_service.info_args(&format!("{label}: Enabled extension"), &log_args(&[&e.identifier.id, &extension.version]));
                }
            }

            let must_install = installed_extension.is_none_or(|installed| {
                Some(installed.pre_release) != e.pre_release // Install if the extension pre-release preference has changed
                    || Some(installed.pinned) != e.pinned // Install if the extension pinned preference has changed
                    || version.as_deref().is_some_and(|version| installed.manifest.version != version) // Install if the extension version has changed
            }); // Install if the extension does not exist
            if must_install {
                if self.services.extension_management_service.can_install(&extension).await?.is_ok() {
                    let context = IndexMap::from([
                        (EXTENSION_INSTALL_SKIP_WALKTHROUGH_CONTEXT.to_owned(), serde_json::Value::Bool(true)),
                        (EXTENSION_INSTALL_SOURCE_CONTEXT.to_owned(), serde_json::Value::String(ExtensionInstallSource::SettingsSync.as_str().to_owned())),
                        (EXTENSION_INSTALL_SKIP_PUBLISHER_TRUST_CONTEXT.to_owned(), serde_json::Value::Bool(true)),
                    ]);
                    let options = InstallOptions {
                        is_machine_scoped: Some(false), /* set isMachineScoped value to prevent install and sync dialog in web */
                        donot_include_pack_and_dependencies: true,
                        install_given_version: e.pinned == Some(true) && !e.version.is_empty(),
                        pinned: e.pinned,
                        install_pre_release_version: e.pre_release == Some(true),
                        pre_release: e.pre_release,
                        profile_location: Some(profile_location_of(profile)),
                        is_application_scoped: e.is_application_scoped,
                        context,
                        ..InstallOptions::default()
                    };
                    let mut plan = plan.borrow_mut();
                    plan.extensions_to_install.push(InstallExtensionInfo { extension: extension.clone(), options });
                    plan.sync_extensions_to_install.insert(extension.identifier.id.to_lowercase(), e.clone());
                } else {
                    self.log_service.info_args(
                        &format!("{label}: Skipped synchronizing extension because it cannot be installed."),
                        &log_args(&[&display_name_or_id(&extension.display_name, &extension.identifier.id)]),
                    );
                    plan.borrow_mut().add_to_skipped.push(e.clone());
                }
            }
            Ok(())
        }
        .await;
        if let Err(error) = outcome {
            plan.borrow_mut().add_to_skipped.push(e.clone());
            self.log_service.error_args(&LogMessage::from(&error), &[]);
            self.log_service.info_args(&format!("{label}: Skipped synchronizing extension"), &log_args(&[&display_name_or_id(&extension.display_name, &extension.identifier.id)]));
        }
        Ok(())
    }

    /// `withProfileScopedServices`: chama `f` com o serviço de habilitação global e o de armazenamento de extensão sobre o
    /// armazenamento do perfil, e os descarta no fim.
    fn with_profile_scoped_services<T: Clone + 'static>(
        &self,
        profile: &UserDataProfile,
        f: impl FnOnce(Rc<dyn IGlobalExtensionEnablementService>, Rc<dyn IExtensionStorageService>) -> Promise<T> + 'static,
    ) -> Promise<T> {
        let (services, log_service) = (self.services.clone(), self.log_service.clone());
        self.services.user_data_profile_storage_service.with_profile_scoped_storage_service(profile, move |storage_service: Rc<dyn StorageService>| {
            async_fn(async move {
                let disposables = DisposableStore::new();
                let extension_enablement_service = disposables.add_rc(GlobalExtensionEnablementService::new(storage_service.clone(), services.extension_management_service.clone()));
                let extension_storage_service = disposables.add_rc(ExtensionStorageService::create((), (storage_service, services.product_service.clone(), log_service)));
                let result = f(extension_enablement_service, extension_storage_service).await;
                disposables.dispose();
                result
            })
        })
    }
}

/// `updateExtensionState`: copia para o estado guardado da extensão as chaves a sincronizar (todas, se a versão não é
/// conhecida).
fn update_extension_state(state: &ExtensionState, extension_id: &str, target: &ExtensionStorageTarget, version: Option<&str>, extension_storage_service: &dyn IExtensionStorageService) {
    let mut extension_state = match extension_storage_service.get_extension_state(target, true) {
        Some(serde_json::Value::Object(extension_state)) => extension_state,
        _ => serde_json::Map::new(),
    };
    let keys = version.and_then(|version| extension_storage_service.get_keys_for_sync(&ExtensionIdWithVersion { id: extension_id.to_owned(), version: version.to_owned() }));
    match keys {
        Some(keys) => {
            for key in keys {
                match state.get(&key) {
                    Some(value) => {
                        extension_state.insert(key, value.clone());
                    }
                    None => {
                        extension_state.remove(&key);
                    }
                }
            }
        }
        None => {
            for (key, value) in state {
                extension_state.insert(key.clone(), value.clone());
            }
        }
    }
    extension_storage_service.set_extension_state(target, Some(&serde_json::Value::Object(extension_state)), true);
}

// endregion

// region: ExtensionsSynchroniser

/// `ExtensionsSynchroniser`: sincroniza as extensões instaladas, com a habilitação e o estado de cada uma. Dá `Deref`
/// para a [`AbstractSynchroniser`].
pub struct ExtensionsSynchroniser {
    pub base: Rc<AbstractSynchroniser<ExtensionsSynchroniser>>,
    extension_services: ExtensionsSyncServices,
    local_extensions_provider: LocalExtensionsProvider,
    /// `previewResource`, `baseResource`, `localResource`, `remoteResource` e `acceptedResource`.
    pub resources: PreviewResources,
}

impl Deref for ExtensionsSynchroniser {
    type Target = AbstractSynchroniser<ExtensionsSynchroniser>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl IDisposable for ExtensionsSynchroniser {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `getPreviewContent`.
fn get_preview_content(local_extensions: &[SyncExtension], added: &[SyncExtension], updated: &[SyncExtension], removed: &[ExtensionIdentifierInfo]) -> String {
    let mut preview: Vec<SyncExtension> = added.iter().chain(updated.iter()).cloned().collect();

    let mut ids_or_uuids: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut add_identifier = |identifier: &ExtensionIdentifierInfo| {
        ids_or_uuids.insert(identifier.id.to_lowercase());
        if let Some(uuid) = identifier.uuid.as_deref().filter(|uuid| !uuid.is_empty()) {
            ids_or_uuids.insert(uuid.to_owned());
        }
    };
    preview.iter().for_each(|extension| add_identifier(&extension.identifier));
    removed.iter().for_each(&mut add_identifier);

    for local_extension in local_extensions {
        let uuid = local_extension.identifier.uuid.as_deref().filter(|uuid| !uuid.is_empty());
        if ids_or_uuids.contains(&local_extension.identifier.id.to_lowercase()) || uuid.is_some_and(|uuid| ids_or_uuids.contains(uuid)) {
            // skip
            continue;
        }
        preview.push(local_extension.clone());
    }

    stringify(&mut preview, false)
}

/// `computeBuiltinExtensions`.
fn compute_builtin_extensions(local_extensions: &[LocalSyncExtension], previous_builtin_extensions: Option<&[ExtensionIdentifierInfo]>) -> Vec<ExtensionIdentifierInfo> {
    let mut local_extensions_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut builtin_extensions: Vec<ExtensionIdentifierInfo> = Vec::new();
    for local_extension in local_extensions {
        local_extensions_set.insert(local_extension.identifier.id.to_lowercase());
        if local_extension.installed != Some(true) {
            builtin_extensions.push(local_extension.identifier.clone());
        }
    }
    if let Some(previous_builtin_extensions) = previous_builtin_extensions {
        for builtin_extension in previous_builtin_extensions {
            // Add previous builtin extension if it does not exist in local extensions
            if !local_extensions_set.contains(&builtin_extension.id.to_lowercase()) {
                builtin_extensions.push(builtin_extension.clone());
            }
        }
    }
    builtin_extensions
}

impl ExtensionsSynchroniser {
    /// O construtor do original (`profile` muda para o perfil padrão).
    pub fn new(profile: UserDataProfile, collection: Option<String>, services: SynchroniserServices, extension_services: ExtensionsSyncServices) -> Rc<Self> {
        let base = AbstractSynchroniser::<Self>::new(IUserDataSyncResource { sync_resource: SyncResource::Extensions, profile }, collection, services);
        let preview_resource = base.services.ext_uri.join_path(&base.sync_preview_folder, &["extensions.json"]);
        let local_extensions_provider = LocalExtensionsProvider::new(extension_services.clone(), base.services.log_service.clone());
        let this = Rc::new(Self { base: base.clone(), extension_services: extension_services.clone(), local_extensions_provider, resources: PreviewResources::new(preview_resource) });
        base.bind(Rc::downgrade(&this));

        let weak: Weak<AbstractSynchroniser<Self>> = Rc::downgrade(&base);
        base.register(extension_services.extension_management_service.on_did_install_extensions().listen({
            let weak = weak.clone();
            move |results| {
                if results.iter().any(|result| result.local.is_some())
                    && let Some(base) = weak.upgrade()
                {
                    base.trigger_local_change();
                }
            }
        }));
        base.register(extension_services.extension_management_service.on_did_uninstall_extension().listen({
            let weak = weak.clone();
            move |event| {
                if event.error.is_none()
                    && let Some(base) = weak.upgrade()
                {
                    base.trigger_local_change();
                }
            }
        }));
        let profile_id = base.sync_resource.profile.id.clone();
        base.register(extension_services.user_data_profile_storage_service.on_did_change().listen({
            let weak = weak.clone();
            move |changes| {
                let disabled_changed = changes
                    .value_changes
                    .iter()
                    .any(|value_changes| value_changes.profile.id == profile_id && value_changes.changes.iter().any(|change: &StorageValueChangeEvent| change.key == DISABLED_EXTENSIONS_STORAGE_PATH));
                if disabled_changed && let Some(base) = weak.upgrade() {
                    base.trigger_local_change();
                }
            }
        }));
        base.register(extension_services.extension_storage_service.on_did_change_extension_storage_to_sync().listen(move |_| {
            if let Some(base) = weak.upgrade() {
                base.trigger_local_change();
            }
        }));
        this
    }

    /// `acceptLocal`.
    async fn accept_local(&self, resource_preview: &IExtensionResourcePreview) -> Result<IExtensionResourceMergeResult, Error> {
        let installed_extensions = self.extension_services.extension_management_service.get_installed_filtered(None, Some(&self.sync_resource.profile.extensions_resource), None, None).await?;
        let ignored_extensions = self.extension_services.ignored_extensions_management_service.get_ignored_extensions(&installed_extensions);
        let remote_extensions = parse_remote_content(resource_preview.remote_content.as_deref())?;
        let extra = &resource_preview.extra;
        let merge_result = merge(
            &extra.local_extensions,
            remote_extensions.as_deref(),
            remote_extensions.as_deref(),
            &extra.skipped_extensions,
            &ignored_extensions,
            extra.builtin_extensions.as_deref(),
        );
        Ok(merge_to_accept_result(resource_preview.local_content.clone(), merge_result))
    }

    /// `acceptRemote`.
    async fn accept_remote(&self, resource_preview: &IExtensionResourcePreview) -> Result<IExtensionResourceMergeResult, Error> {
        let installed_extensions = self.extension_services.extension_management_service.get_installed_filtered(None, Some(&self.sync_resource.profile.extensions_resource), None, None).await?;
        let ignored_extensions = self.extension_services.ignored_extensions_management_service.get_ignored_extensions(&installed_extensions);
        let remote_extensions = parse_remote_content(resource_preview.remote_content.as_deref())?;
        let extra = &resource_preview.extra;
        match remote_extensions {
            Some(remote_extensions) => {
                let merge_result = merge(&extra.local_extensions, Some(&remote_extensions), Some(&extra.local_extensions), &[], &ignored_extensions, extra.builtin_extensions.as_deref());
                Ok(merge_to_accept_result(resource_preview.remote_content.clone(), merge_result))
            }
            None => Ok(IAcceptResult { content: resource_preview.remote_content.clone(), local_change: Change::None, remote_change: Change::None, extra: ExtensionsMergeExtra::default() }),
        }
    }
}

impl SynchroniserImpl for ExtensionsSynchroniser {
    type Preview = ExtensionsPreviewExtra;
    type Accept = ExtensionsMergeExtra;

    /*
        Version 3 - Introduce installed property to skip installing built in extensions
        protected readonly version: number = 3;
    */
    /* Version 4: Change settings from `sync.${setting}` to `settingsSync.{setting}` */
    /* Version 5: Introduce extension state */
    /* Version 6: Added isApplicationScoped property */
    fn version(&self) -> f64 {
        6.0
    }

    fn generate_sync_preview(
        self: Rc<Self>,
        remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        _is_remote_data_from_current_machine: bool,
        _user_data_sync_configuration: IUserDataSyncConfiguration,
        _token: CancellationToken,
    ) -> Promise<Vec<IExtensionResourcePreview>> {
        async_fn(async move {
            let extension_management_service = self.extension_services.extension_management_service.clone();
            let mut remote_extensions = match &remote_user_data.sync_data {
                Some(sync_data) => Some(parse_and_migrate_extensions(sync_data.clone(), extension_management_service.clone()).await?),
                None => None,
            };
            let skipped_extensions = skipped_extensions_of(last_sync_user_data.as_ref())?.unwrap_or_default();
            let builtin_extensions = builtin_extensions_of(last_sync_user_data.as_ref())?;
            let mut last_sync_extensions = match last_sync_user_data.as_ref().and_then(|last_sync_user_data| last_sync_user_data.sync_data.as_ref()) {
                Some(sync_data) => Some(parse_and_migrate_extensions(sync_data.clone(), extension_management_service.clone()).await?),
                None => None,
            };

            let LocalExtensionsResult { mut local_extensions, ignored_extensions } = self.local_extensions_provider.get_local_extensions(&self.sync_resource.profile).await?;

            if remote_extensions.is_some() {
                self.services.log_service.trace(&format!("{}: Merging remote extensions with local extensions...", self.sync_resource_log_label));
            } else {
                self.services.log_service.trace(&format!("{}: Remote extensions does not exist. Synchronizing extensions for the first time.", self.sync_resource_log_label));
            }

            let merge_result = merge(&local_extensions, remote_extensions.as_deref(), last_sync_extensions.as_deref(), &skipped_extensions, &ignored_extensions, builtin_extensions.as_deref());
            let content = get_preview_content(&local_extensions, &merge_result.local.added, &merge_result.local.updated, &merge_result.local.removed);
            let preview_result = merge_to_accept_result(Some(content), merge_result);

            let local_content = stringify(&mut local_extensions, false);
            let base_content = match last_sync_extensions.as_mut() {
                Some(last_sync_extensions) => stringify(last_sync_extensions, false),
                None => local_content.clone(),
            };
            let remote_content = remote_extensions.as_mut().map(|remote_extensions| stringify(remote_extensions, false));
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
                extra: ExtensionsPreviewExtra { local_extensions, remote_extensions, skipped_extensions, builtin_extensions, preview_result },
            }])
        })
    }

    fn has_remote_changed(self: Rc<Self>, last_sync_user_data: IRemoteUserData) -> Promise<bool> {
        async_fn(async move {
            let last_sync_extensions = match &last_sync_user_data.sync_data {
                Some(sync_data) => Some(parse_and_migrate_extensions(sync_data.clone(), self.extension_services.extension_management_service.clone()).await?),
                None => None,
            };
            let LocalExtensionsResult { local_extensions, ignored_extensions } = self.local_extensions_provider.get_local_extensions(&self.sync_resource.profile).await?;
            let skipped_extensions = skipped_extensions_of(Some(&last_sync_user_data))?.unwrap_or_default();
            let builtin_extensions = builtin_extensions_of(Some(&last_sync_user_data))?.unwrap_or_default();
            let result = merge(&local_extensions, last_sync_extensions.as_deref(), last_sync_extensions.as_deref(), &skipped_extensions, &ignored_extensions, Some(&builtin_extensions));
            Ok(result.remote.is_some())
        })
    }

    fn get_merge_result(self: Rc<Self>, resource_preview: IExtensionResourcePreview, _token: CancellationToken) -> Promise<IMergeResult<ExtensionsMergeExtra>> {
        Promise::resolved(IMergeResult { result: resource_preview.extra.preview_result, has_conflicts: false })
    }

    fn get_accept_result(self: Rc<Self>, resource_preview: IExtensionResourcePreview, resource: URI, _content: Option<Option<String>>, _token: CancellationToken) -> Promise<IExtensionResourceMergeResult> {
        async_fn(async move {
            let is_same = |other: &URI| self.services.ext_uri.is_equal(Some(&resource), Some(other), false);

            /* Accept local resource */
            if is_same(&self.resources.local) {
                return self.accept_local(&resource_preview).await;
            }

            /* Accept remote resource */
            if is_same(&self.resources.remote) {
                return self.accept_remote(&resource_preview).await;
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
        resource_previews: Vec<(IExtensionResourcePreview, IExtensionResourceMergeResult)>,
        force: bool,
    ) -> Promise<()> {
        async_fn(async move {
            let (resource_preview, accept_result) = &resource_previews[0];
            let ExtensionsPreviewExtra { local_extensions, .. } = &resource_preview.extra;
            let mut skipped_extensions = resource_preview.extra.skipped_extensions.clone();
            let builtin_extensions = resource_preview.extra.builtin_extensions.clone();
            let ExtensionsMergeExtra { local, remote } = &accept_result.extra;
            let (local_change, remote_change) = (accept_result.local_change, accept_result.remote_change);
            let label = &self.sync_resource_log_label;

            if local_change == Change::None && remote_change == Change::None {
                self.services.log_service.info(&format!("{label}: No changes found during synchronizing extensions."));
            }

            if local_change != Change::None {
                self.backup_local(&sync_extensions_text(local_extensions)).await?;
                skipped_extensions = self.local_extensions_provider.update_local_extensions(local.added.clone(), local.removed.clone(), local.updated.clone(), skipped_extensions, self.sync_resource.profile.clone()).await?;
            }

            if let Some(remote) = remote {
                // update remote
                self.services.log_service.trace(&format!("{label}: Updating remote extensions..."));
                let content = sync_extensions_text(&remote.all);
                remote_user_data = self.update_remote_user_data(content, if force { None } else { Some(remote_user_data.r#ref.clone()) }).await?;
                let listed = |name: &str, extensions: &[SyncExtension]| {
                    if extensions.is_empty() { String::new() } else { format!(" {name}: {}.", ids_text(extensions.iter().map(|extension| extension.identifier.id.as_str()))) }
                };
                self.services.log_service.info(&format!("{label}: Updated remote extensions.{}{}{}", listed("Added", &remote.added), listed("Updated", &remote.updated), listed("Removed", &remote.removed)));
            }

            if last_sync_user_data.as_ref().map(|last_sync_user_data| &last_sync_user_data.r#ref) != Some(&remote_user_data.r#ref) {
                // update last sync
                self.services.log_service.trace(&format!("{label}: Updating last synchronized extensions..."));
                let builtin_extensions = compute_builtin_extensions(local_extensions, builtin_extensions.as_deref());
                let additional_props = vec![
                    ("skippedExtensions".to_owned(), text_to_json(&sync_extensions_text(&skipped_extensions))),
                    ("builtinExtensions".to_owned(), text_to_json(&identifiers_text(&builtin_extensions))),
                ];
                self.update_last_sync_user_data(remote_user_data, additional_props).await?;
                let skipped = if skipped_extensions.is_empty() { String::new() } else { format!(" Skipped: {}.", ids_text(skipped_extensions.iter().map(|extension| extension.identifier.id.as_str()))) };
                self.services.log_service.info(&format!("{label}: Updated last synchronized extensions.{skipped}"));
            }
            Ok(())
        })
    }

    fn has_local_data(self: Rc<Self>) -> Promise<bool> {
        async_fn(async move {
            match self.local_extensions_provider.get_local_extensions(&self.sync_resource.profile).await {
                Ok(LocalExtensionsResult { local_extensions, .. }) => Ok(local_extensions.iter().any(|extension| extension.installed == Some(true) || extension.disabled == Some(true))),
                Err(_) => Ok(false), /* ignore error */
            }
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
                    let mut extensions = parse_sync_extensions(&content)?;
                    Ok(Some(stringify(&mut extensions, true)))
                }
                other => Ok(other),
            }
        })
    }
}

// endregion

// region: fábrica

/// A fábrica do [`ExtensionsSynchroniser`] que o `UserDataSyncService` recebe (`createInstance(ExtensionsSynchroniser,
/// profile, collection)` do `ProfileSynchronizer.createSynchronizer`).
pub struct WorkbenchExtensionsSynchroniserFactory {
    services: ExtensionsSyncServices,
}

impl WorkbenchExtensionsSynchroniserFactory {
    pub fn new(services: ExtensionsSyncServices) -> Rc<Self> {
        Rc::new(Self { services })
    }
}

impl ExtensionsSynchroniserFactory for WorkbenchExtensionsSynchroniserFactory {
    fn is_gallery_enabled(&self) -> bool {
        self.services.extension_gallery_service.is_enabled()
    }

    fn create(&self, profile: UserDataProfile, collection: Option<String>, services: SynchroniserServices) -> Rc<ExtensionsSynchroniserHandle> {
        let synchroniser = ExtensionsSynchroniser::new(profile, collection, services, self.services.clone());
        Rc::new(ExtensionsSynchroniserHandle { synchroniser: synchroniser.base.clone(), disposable: synchroniser.base.clone(), implementation: synchroniser })
    }
}

// endregion

// region: AbstractExtensionsInitializer

/// `IExtensionsInitializerPreviewResult.newExtensions`: `IExtensionIdentifier & { preRelease: boolean }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewExtension {
    pub identifier: ExtensionIdentifierInfo,
    pub pre_release: bool,
}

/// `IExtensionsInitializerPreviewResult`.
#[derive(Clone)]
pub struct ExtensionsInitializerPreviewResult {
    pub installed_extensions: Vec<ExtensionRef>,
    pub disabled_extensions: Vec<ExtensionIdentifierInfo>,
    pub new_extensions: Vec<NewExtension>,
    pub remote_extensions: Vec<SyncExtension>,
}

/// `AbstractExtensionsInitializer`. Dá `Deref` para o [`AbstractInitializer`]; quem a estende liga o seu
/// `InitializerImpl` por `base.bind`.
pub struct AbstractExtensionsInitializer {
    pub base: Rc<AbstractInitializer>,
    pub extension_management_service: Rc<dyn IExtensionInstallService>,
    ignored_extensions_management_service: Rc<dyn IIgnoredExtensionsManagementService>,
}

impl Deref for AbstractExtensionsInitializer {
    type Target = AbstractInitializer;

    fn deref(&self) -> &AbstractInitializer {
        &self.base
    }
}

impl AbstractExtensionsInitializer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        extension_management_service: Rc<dyn IExtensionInstallService>,
        ignored_extensions_management_service: Rc<dyn IIgnoredExtensionsManagementService>,
        file_service: Rc<dyn IFileService>,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        environment_service: Rc<dyn EnvironmentService>,
        log_service: LogServiceRef,
        storage_service: Rc<dyn StorageService>,
        ext_uri: ExtUri,
    ) -> Rc<Self> {
        let base = AbstractInitializer::new(SyncResource::Extensions, user_data_profiles_service, environment_service, log_service, file_service, storage_service, ext_uri);
        Rc::new(Self { base, extension_management_service, ignored_extensions_management_service })
    }

    /// `parseExtensions`: `null` se o dado remoto não existe.
    pub fn parse_extensions(&self, remote_user_data: &IRemoteUserData) -> Promise<Option<Vec<SyncExtension>>> {
        match &remote_user_data.sync_data {
            Some(sync_data) => {
                let (sync_data, extension_management_service) = (sync_data.clone(), self.extension_management_service.clone());
                async_fn(async move { Ok(Some(parse_and_migrate_extensions(sync_data, extension_management_service).await?)) })
            }
            None => Promise::resolved(None),
        }
    }

    /// `generatePreview`.
    pub fn generate_preview(&self, remote_extensions: Vec<SyncExtension>, local_extensions: &[ExtensionRef]) -> ExtensionsInitializerPreviewResult {
        let mut installed_extensions: Vec<ExtensionRef> = Vec::new();
        let mut new_extensions: Vec<NewExtension> = Vec::new();
        let mut disabled_extensions: Vec<ExtensionIdentifierInfo> = Vec::new();
        for extension in &remote_extensions {
            if self.ignored_extensions_management_service.has_to_never_sync_extension(&extension.identifier.id) {
                // Skip extension ignored to sync
                continue;
            }

            let installed_extension = local_extensions.iter().find(|installed| are_same_extensions(&installed.identifier, &extension.identifier));
            if let Some(installed_extension) = installed_extension {
                installed_extensions.push(installed_extension.clone());
                if extension.disabled == Some(true) {
                    disabled_extensions.push(extension.identifier.clone());
                }
            } else if extension.installed == Some(true) {
                new_extensions.push(NewExtension { identifier: extension.identifier.clone(), pre_release: extension.pre_release == Some(true) });
                if extension.disabled == Some(true) {
                    disabled_extensions.push(extension.identifier.clone());
                }
            }
        }
        ExtensionsInitializerPreviewResult { installed_extensions, new_extensions, disabled_extensions, remote_extensions }
    }
}

// endregion
