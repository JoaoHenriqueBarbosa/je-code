//! Porte de `src/vs/platform/userDataSync/common/abstractJsonSynchronizer.ts`: a base dos sincronizadores de um
//! recurso que é um arquivo JSON mesclado como texto (hoje o de `tasks`), e o que ela divide com o sincronizador
//! de `keybindings` ([`PreviewResources`], [`FileMergePreviewExtra`], [`get_sync_content_property`]).
//!
//! Decisões de tradução (as da base estão em [`super::abstract_synchronizer`]):
//! - **Métodos abstratos.** `getContentFromSyncContent` e `toSyncContent` são o trait [`JsonSyncKind`], que o
//!   recurso concreto implementa (funções associadas, sem estado); [`AbstractJsonSynchronizer`] é genérico nele e
//!   o sincronizador do recurso é um alias (`TasksSynchroniser`). O `object` de `toSyncContent` é um [`Value`].
//! - **Preview.** `IJsonResourcePreview` (e o `IKeybindingsResourcePreview`, idêntico no original) é um
//!   [`IResourcePreview`] com [`FileMergePreviewExtra`] (`fileContent` e `previewResult`); o resultado de aceitar
//!   não acrescenta campos (`()`).
//! - `getAcceptResult` e `resolveContent` são os mesmos nos dois sincronizadores de arquivo: vivem em
//!   [`PreviewResources`], que guarda as URIs virtuais (`base`, `local`, `remote`, `accepted`) e a do preview.
//! - `null` e `undefined` de `string | null` viram `None`; as comparações `!lastSyncContent` (vazio conta como
//!   ausente) são feitas sobre `Option<String>`.

use std::marker::PhantomData;
use std::ops::Deref;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::json::Value;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;

use super::abstract_synchronizer::{
    AbstractFileSynchroniser, AbstractSynchroniser, IAcceptResult, IMergeResult, IResourcePreview, SynchroniserImpl, SynchroniserServices, parse_json, stringify_json, with_sync_authority,
};
use super::user_data_sync::{Change, IRemoteUserData, IUserDataSyncConfiguration, IUserDataSyncLogService, IUserDataSyncResource};
use crate::files::files::FileContent;
use crate::log::LogMessage;

/// O que `IJsonResourcePreview` e `IKeybindingsResourcePreview` acrescentam a `IFileResourcePreview`:
/// `fileContent` (o arquivo lido) e `previewResult`.
#[derive(Clone, Debug)]
pub struct FileMergePreviewExtra {
    pub file_content: Option<FileContent>,
    pub preview_result: IMergeResult<()>,
}

/// `IJsonResourcePreview`.
pub type IJsonResourcePreview = IResourcePreview<FileMergePreviewExtra>;

/// A propriedade de texto do conteúdo de sincronização (`JSON.parse(syncContent)[property]`), ou `None` (o `null`
/// do original) se ela não existe, se o conteúdo não é JSON (o erro vai para o log) ou se é `null` (o acesso
/// lança `TypeError`, que o `catch` do original também registra).
pub fn get_sync_content_property(sync_content: &str, property: &str, log_service: &IUserDataSyncLogService) -> Option<String> {
    let parsed = match parse_json(sync_content) {
        Ok(Value::Null) => Err(Error::type_error(format!("Cannot read properties of null (reading '{property}')"))),
        other => other,
    };
    match parsed {
        Ok(Value::Object(object)) => match object.get(property) {
            Some(Value::String(text)) => Some(text.to_string_lossy()),
            _ => None,
        },
        Ok(_) => None,
        Err(error) => {
            log_service.error_args(&LogMessage::from(&error), &[]);
            None
        }
    }
}

/// As URIs de um recurso de arquivo no preview: a do próprio preview e as virtuais derivadas dela.
#[derive(Clone, Debug)]
pub struct PreviewResources {
    pub preview: URI,
    pub base: URI,
    pub local: URI,
    pub remote: URI,
    pub accepted: URI,
}

impl PreviewResources {
    pub fn new(preview: URI) -> Self {
        Self {
            base: with_sync_authority(&preview, "base"),
            local: with_sync_authority(&preview, "local"),
            remote: with_sync_authority(&preview, "remote"),
            accepted: with_sync_authority(&preview, "accepted"),
            preview,
        }
    }

    /// `getAcceptResult`: aceitar o local, o remoto ou o preview (este com o conteúdo editado, se houver).
    pub fn accept_result(&self, ext_uri: &ExtUri, resource_preview: &IJsonResourcePreview, resource: &URI, content: Option<Option<String>>) -> Result<IAcceptResult<()>, Error> {
        let is_same = |other: &URI| ext_uri.is_equal(Some(resource), Some(other), false);

        /* Accept local resource */
        if is_same(&self.local) {
            return Ok(IAcceptResult {
                content: resource_preview.extra.file_content.as_ref().map(|file_content| file_content.value.to_string()),
                local_change: Change::None,
                remote_change: Change::Modified,
                extra: (),
            });
        }

        /* Accept remote resource */
        if is_same(&self.remote) {
            return Ok(IAcceptResult { content: resource_preview.remote_content.clone(), local_change: Change::Modified, remote_change: Change::None, extra: () });
        }

        /* Accept preview resource */
        if is_same(&self.preview) {
            return Ok(match content {
                None => {
                    let preview_result = &resource_preview.extra.preview_result;
                    IAcceptResult { content: preview_result.content.clone(), local_change: preview_result.local_change, remote_change: preview_result.remote_change, extra: () }
                }
                Some(content) => IAcceptResult { content, local_change: Change::Modified, remote_change: Change::Modified, extra: () },
            });
        }

        Err(Error::new(format!("Invalid Resource: {}", resource.to_string(false).to_string_lossy())))
    }

    /// `resolveContent`: o conteúdo das URIs virtuais vem do preview; as outras não têm conteúdo.
    pub fn resolve_content<I: SynchroniserImpl>(&self, synchroniser: &AbstractSynchroniser<I>, uri: &URI) -> Promise<Option<String>> {
        if self.is_virtual_resource(&synchroniser.services.ext_uri, uri) {
            return synchroniser.resolve_preview_content(uri);
        }
        Promise::resolved(None)
    }

    /// `uri` é uma das URIs virtuais (`remote`, `base`, `local`, `accepted`), as que o `resolveContent` resolve.
    pub fn is_virtual_resource(&self, ext_uri: &ExtUri, uri: &URI) -> bool {
        [&self.remote, &self.base, &self.local, &self.accepted].into_iter().any(|resource| ext_uri.is_equal(Some(resource), Some(uri), false))
    }
}

/// O que `AbstractJsonSynchronizer` deixa abstrato, para um recurso: `getContentFromSyncContent` e `toSyncContent`.
pub trait JsonSyncKind: 'static {
    /// `getContentFromSyncContent`.
    fn get_content_from_sync_content(sync_content: &str, log_service: &IUserDataSyncLogService) -> Option<String>;

    /// `toSyncContent`: o objeto que se grava no servidor (`None` é o `null`, o recurso não existe).
    fn to_sync_content(content: Option<&str>) -> Value;
}

/// O resultado do `merge` privado do original.
struct JsonMergeResult {
    content: Option<String>,
    has_local_changed: bool,
    has_remote_changed: bool,
    has_conflicts: bool,
}

impl JsonMergeResult {
    fn unchanged() -> Self {
        Self { content: None, has_local_changed: false, has_remote_changed: false, has_conflicts: false }
    }
}

/// `merge`: três vias sobre o texto inteiro (sem mesclar por dentro).
fn merge(original_local_content: Option<String>, original_remote_content: Option<String>, base_content: Option<String>) -> JsonMergeResult {
    /* no changes */
    if original_local_content.is_none() && original_remote_content.is_none() && base_content.is_none() {
        return JsonMergeResult::unchanged();
    }

    // Normalize nulls to empty strings for easier comparison
    let original_remote_content = original_remote_content.unwrap_or_default();
    let original_local_content = original_local_content.unwrap_or_default();
    let base_content = base_content.unwrap_or_default();

    /* no changes */
    if original_local_content == original_remote_content {
        return JsonMergeResult::unchanged();
    }

    let local_forwarded = base_content != original_local_content;
    let remote_forwarded = base_content != original_remote_content;

    /* no changes */
    if !local_forwarded && !remote_forwarded {
        return JsonMergeResult::unchanged();
    }

    /* local has changed and remote has not */
    if local_forwarded && !remote_forwarded {
        return JsonMergeResult { content: Some(original_local_content), has_remote_changed: true, has_local_changed: false, has_conflicts: false };
    }

    /* remote has changed and local has not */
    if remote_forwarded && !local_forwarded {
        return JsonMergeResult { content: Some(original_remote_content), has_local_changed: true, has_remote_changed: false, has_conflicts: false };
    }

    JsonMergeResult { content: Some(original_local_content), has_local_changed: true, has_remote_changed: true, has_conflicts: true }
}

/// `AbstractJsonSynchronizer`. Dá `Deref` para o [`AbstractFileSynchroniser`].
pub struct AbstractJsonSynchronizer<K: JsonSyncKind> {
    file_synchroniser: AbstractFileSynchroniser<Self>,
    pub resources: PreviewResources,
    kind: PhantomData<K>,
}

impl<K: JsonSyncKind> Deref for AbstractJsonSynchronizer<K> {
    type Target = AbstractFileSynchroniser<Self>;

    fn deref(&self) -> &Self::Target {
        &self.file_synchroniser
    }
}

impl<K: JsonSyncKind> IDisposable for AbstractJsonSynchronizer<K> {
    fn dispose(&self) {
        self.file_synchroniser.dispose();
    }
}

impl<K: JsonSyncKind> AbstractJsonSynchronizer<K> {
    pub fn new(file_resource: URI, sync_resource_metadata: IUserDataSyncResource, collection: Option<String>, preview_file_name: &str, services: SynchroniserServices) -> Rc<Self> {
        let ext_uri = services.ext_uri.clone();
        let file_synchroniser = AbstractFileSynchroniser::new(file_resource, sync_resource_metadata, collection, services);
        let preview_resource = ext_uri.join_path(&file_synchroniser.sync_preview_folder, &[preview_file_name]);
        let this = Rc::new(Self { file_synchroniser, resources: PreviewResources::new(preview_resource), kind: PhantomData });
        this.bind(Rc::downgrade(&this));
        this
    }

    fn content_from_sync_data(&self, remote_user_data: &IRemoteUserData) -> Option<String> {
        remote_user_data.sync_data.as_ref().and_then(|sync_data| K::get_content_from_sync_content(&sync_data.content, &self.services.log_service))
    }

    fn label(&self) -> String {
        self.resource.as_str().to_string()
    }
}

impl<K: JsonSyncKind> SynchroniserImpl for AbstractJsonSynchronizer<K> {
    type Preview = FileMergePreviewExtra;
    type Accept = ();

    fn version(&self) -> f64 {
        1.0
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
            let remote_content = self.content_from_sync_data(&remote_user_data);

            // Use remote data as last sync data if last sync data does not exist and remote data is from same machine
            let last_sync_user_data = if last_sync_user_data.is_none() && is_remote_data_from_current_machine { Some(remote_user_data.clone()) } else { last_sync_user_data };
            let last_sync_content = last_sync_user_data.as_ref().and_then(|last_sync_user_data| self.content_from_sync_data(last_sync_user_data));

            // Get file content last to get the latest
            let file_content = self.get_local_file_content().await?;
            let local_content = file_content.as_ref().map(|file_content| file_content.value.to_string());

            let mut content: Option<String> = None;
            let mut has_local_changed = false;
            let mut has_remote_changed = false;
            let mut has_conflicts = false;

            if remote_user_data.sync_data.is_some() {
                if last_sync_content.as_deref().is_none_or(str::is_empty) // First time sync
                    || last_sync_content != local_content // Local has forwarded
                    || last_sync_content != remote_content
                // Remote has forwarded
                {
                    let label = self.label();
                    self.services.log_service.trace(&format!("{}: Merging remote {label} with local {label}...", self.sync_resource_log_label));
                    let result = merge(local_content.clone(), remote_content.clone(), last_sync_content.clone());
                    content = result.content;
                    has_conflicts = result.has_conflicts;
                    has_local_changed = result.has_local_changed;
                    has_remote_changed = result.has_remote_changed;
                }
            }
            // First time syncing to remote
            else if let Some(local_content) = &local_content {
                let label = self.label();
                self.services.log_service.trace(&format!("{}: Remote {label} does not exist. Synchronizing {label} for the first time.", self.sync_resource_log_label));
                content = Some(local_content.clone());
                has_remote_changed = true;
            }

            let preview_result = IMergeResult {
                result: IAcceptResult {
                    content: if has_conflicts { last_sync_content.clone() } else { content },
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
            let last_sync_content = self.content_from_sync_data(&last_sync_user_data);
            if last_sync_content.is_none() {
                return Ok(true);
            }

            let file_content = self.get_local_file_content().await?;
            let local_content = file_content.map(|file_content| file_content.value.to_string());
            let result = merge(local_content, last_sync_content.clone(), last_sync_content);
            Ok(result.has_local_changed || result.has_remote_changed)
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
            let (content, local_change, remote_change) = (accept_result.content.clone(), accept_result.local_change, accept_result.remote_change);
            let (label, resource) = (&self.sync_resource_log_label, self.label());

            if local_change == Change::None && remote_change == Change::None {
                self.services.log_service.info(&format!("{label}: No changes found during synchronizing {resource}."));
            }

            if local_change != Change::None {
                self.services.log_service.trace(&format!("{label}: Updating local {resource}..."));
                if let Some(file_content) = &file_content {
                    self.backup_local(&stringify_json(&K::to_sync_content(Some(&file_content.value.to_string())))).await?;
                }
                match content.as_deref().filter(|content| !content.is_empty()) {
                    Some(content) => self.update_local_file_content(content, file_content.clone(), force).await?,
                    None => self.delete_local_file().await?,
                }
                self.services.log_service.info(&format!("{label}: Updated local {resource}"));
            }

            if remote_change != Change::None {
                self.services.log_service.trace(&format!("{label}: Updating remote {resource}..."));
                let remote_contents = stringify_json(&K::to_sync_content(content.as_deref()));
                remote_user_data = self.update_remote_user_data(remote_contents, if force { None } else { Some(remote_user_data.r#ref.clone()) }).await?;
                self.services.log_service.info(&format!("{label}: Updated remote {resource}"));
            }

            // Delete the preview
            let _ = self.services.file_service.del(&self.resources.preview, None).await;

            if last_sync_user_data.as_ref().map(|last_sync_user_data| &last_sync_user_data.r#ref) != Some(&remote_user_data.r#ref) {
                self.services.log_service.trace(&format!("{label}: Updating last synchronized {resource}..."));
                self.update_last_sync_user_data(remote_user_data, Vec::new()).await?;
                self.services.log_service.info(&format!("{label}: Updated last synchronized {resource}"));
            }
            Ok(())
        })
    }

    fn has_local_data(self: Rc<Self>) -> Promise<bool> {
        self.services.file_service.exists(&self.file)
    }

    fn resolve_content(self: Rc<Self>, uri: URI) -> Promise<Option<String>> {
        self.resources.resolve_content::<Self>(&self, &uri)
    }
}
