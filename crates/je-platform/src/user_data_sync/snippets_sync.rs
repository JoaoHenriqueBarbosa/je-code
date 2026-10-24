//! Porte de `src/vs/platform/userDataSync/common/snippetsSync.ts`: o sincronizador de snippets (os arquivos
//! `.json` e `.code-snippets` da pasta de snippets do perfil) e o inicializador dele.
//!
//! Decisões de tradução (as da base estão em [`super::abstract_synchronizer`]):
//! - `SnippetsSynchroniser extends AbstractSynchroniser` é o [`SnippetsSynchroniser`], que guarda o
//!   `Rc<AbstractSynchroniser<Self>>`, o liga com `bind` e dá `Deref` para ele. Cada snippet é um recurso do
//!   preview, então não há uma URI de preview fixa como nos sincronizadores de arquivo único.
//! - `ISnippetsResourcePreview` (um `IFileResourcePreview` com `previewResult`) é um [`IResourcePreview`] com
//!   [`SnippetsPreviewExtra`]; o resultado de aceitar não acrescenta campos (`()`).
//! - `IStringDictionary<string>` é o [`Dictionary`] do `snippets_merge` (a ordem de inserção do objeto decide a
//!   ordem das chaves) e `IStringDictionary<IFileContent>` é um `OrderedMap` de [`FileContent`].
//! - `null` e `undefined` do original se distinguem em duas comparações de `getAcceptResult`
//!   (`fileContent !== null` e `remoteContent !== null`), que tratam `undefined` como "existe". Isso acontece
//!   quando um snippet some dos dois lados: o preview do lado local-removido lê `localFileContent[key]` sem
//!   `|| null` e o do lado remoto-removido lê `remoteSnippets[key]` do mesmo jeito. Os campos
//!   `file_content_is_undefined` e `remote_content_is_undefined` guardam esse `undefined`.
//! - `JSON.parse(syncData.content)` é [`parse_snippets`]: o conteúdo precisa ser um objeto cujos valores são
//!   texto (o original deixaria seguir um valor torto e o `merge` o trataria como texto); outra forma é um erro.
//! - O `remoteResource` de `updateLocalSnippets` e o `localResource` de `updateRemoteSnippets` nunca são
//!   `undefined` neste sincronizador, então o `?:` que escolhe entre os dois lê sempre o primeiro.
//! - `acceptResult.content!` é o conteúdo exigido ([`accepted_content`]): se faltar, é um erro em vez de um
//!   `TypeError` na criação do buffer.
//! - `fileService.watch(...)` e o filtro `Event.filter(onDidFilesChange, e => e.affects(snippetsFolder))` são o
//!   ouvinte registrado no construtor.

use std::ops::Deref;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::json::{Object, Value};
use je_base::lifecycle::IDisposable;
use je_base::map::OrderedMap;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;

use super::abstract_synchronizer::{
    AbstractInitializer, AbstractSynchroniser, IAcceptResult, IMergeResult, IResourcePreview, InitializerImpl, SynchroniserImpl, SynchroniserServices, parse_json, stringify_json, with_sync_authority,
};
use super::snippets_merge::{Dictionary, MergeResult, are_same, merge};
use super::user_data_sync::{Change, IRemoteUserData, ISyncData, IUserDataSyncConfiguration, IUserDataSyncResource, SyncResource};
use crate::environment::environment::EnvironmentService;
use crate::files::file_service::{FileContentSource, FileService};
use crate::files::files::{CreateFileOptions, FileContent, FileOperationError, FileOperationResult, WriteFileOptions};
use crate::log::{LogArg, LogServiceRef};
use crate::storage::StorageService;
use crate::user_data_profile::{IUserDataProfilesService, UserDataProfile};

const SNIPPETS_DATA_VERSION: f64 = 1.0;

/// `IStringDictionary<IFileContent>`: o arquivo lido de cada snippet local, pelo nome.
type LocalFiles = OrderedMap<JsString, FileContent>;

// region: JSON dos snippets

/// `parseSnippets(syncData)`: `JSON.parse(syncData.content)`.
pub fn parse_snippets(sync_data: &ISyncData) -> Result<Dictionary, Error> {
    parse_snippets_content(&sync_data.content)
}

fn parse_snippets_content(content: &str) -> Result<Dictionary, Error> {
    match parse_json(content)? {
        Value::Object(object) => {
            let mut snippets = Dictionary::new();
            for (key, value) in object.iter() {
                match value {
                    Value::String(text) => {
                        snippets.insert(key.clone(), text.clone());
                    }
                    _ => return Err(Error::type_error(format!("Invalid snippet content: {}", key.to_string_lossy()))),
                }
            }
            Ok(snippets)
        }
        Value::Null => Err(Error::type_error("Cannot convert undefined or null to object")),
        _ => Err(Error::type_error("Invalid snippets content: not an object")),
    }
}

/// `JSON.stringify(snippets)` como valor.
fn dictionary_to_json(snippets: &Dictionary) -> Value {
    let mut object = Object::new();
    for (key, value) in snippets.iter() {
        object.insert(key.clone(), Value::String(value.clone()));
    }
    Value::Object(object)
}

/// `toSnippetsContents`: o texto de cada arquivo.
fn to_snippets_contents(snippets_file_contents: &LocalFiles) -> Dictionary {
    let mut snippets = Dictionary::new();
    for (key, file_content) in snippets_file_contents.iter() {
        snippets.insert(key.clone(), JsString::from(file_content.value.to_string()));
    }
    snippets
}

/// `acceptResult.content!`.
fn accepted_content(accept_result: &IAcceptResult<()>) -> Result<String, Error> {
    accept_result.content.clone().ok_or_else(|| Error::type_error("The accepted snippet has no content"))
}

// endregion

// region: preview

/// O que `ISnippetsResourcePreview` acrescenta a `IResourcePreview`: `fileContent` (o arquivo lido) e
/// `previewResult`, mais os dois `undefined` que o original distingue de `null` (ver o cabeçalho do módulo).
#[derive(Clone, Debug)]
pub struct SnippetsPreviewExtra {
    pub file_content: Option<FileContent>,
    pub file_content_is_undefined: bool,
    pub remote_content_is_undefined: bool,
    pub preview_result: IMergeResult<()>,
}

/// `ISnippetsResourcePreview`.
pub type ISnippetsResourcePreview = IResourcePreview<SnippetsPreviewExtra>;

/// O `IMergeResult` sem conflitos que `getResourcePreviews` monta para cada caso do `merge`.
fn merge_result(content: Option<String>, has_conflicts: bool, local_change: Change, remote_change: Change) -> IMergeResult<()> {
    IMergeResult { result: IAcceptResult { content, local_change, remote_change, extra: () }, has_conflicts }
}

/// Como o original lê um campo: `null` escrito no literal, o valor cru do dicionário (`undefined` se falta) ou o
/// valor com `|| null` (vazio e ausente viram `null`).
#[derive(Clone, Copy)]
enum Read {
    Null,
    Raw,
    Truthy,
}

/// O que o preview de um snippet lê dos dicionários.
struct PreviewSources<'a> {
    local_files: &'a LocalFiles,
    remote: &'a Dictionary,
    base: &'a Dictionary,
}

/// Os campos de um preview que dependem do dicionário em que o snippet é lido.
struct PreviewFields {
    base_content: Option<String>,
    local_content: Option<String>,
    remote_content: Option<String>,
    file_content: Option<FileContent>,
    file_content_is_undefined: bool,
    remote_content_is_undefined: bool,
}

impl PreviewSources<'_> {
    /// `baseContent: baseSnippets[key] ?? null`, `fileContent`, `localContent` e `remoteContent` de `key`.
    fn fields(&self, key: &JsString, file: Read, remote: Read) -> PreviewFields {
        let file_content = if matches!(file, Read::Null) { None } else { self.local_files.get(key).cloned() };
        let remote_text = self.remote.get(key).map(JsString::to_string_lossy);
        let (remote_content, remote_content_is_undefined) = match remote {
            Read::Null => (None, false),
            Read::Raw => (remote_text, !self.remote.contains_key(key)),
            Read::Truthy => (remote_text.filter(|text| !text.is_empty()), false),
        };
        PreviewFields {
            base_content: self.base.get(key).map(JsString::to_string_lossy),
            local_content: file_content.as_ref().map(|file_content| file_content.value.to_string()),
            file_content_is_undefined: matches!(file, Read::Raw) && file_content.is_none(),
            file_content,
            remote_content,
            remote_content_is_undefined,
        }
    }
}

// endregion

/// `SnippetsSynchroniser`. Dá `Deref` para a [`AbstractSynchroniser`].
///
/// O `PromptsSynchronizer` de `promptsSync/promptsSync.ts` ("Adopted from SnippetsSynchroniser") é este mesmo
/// sincronizador com outro [`SnippetsKind`]: o recurso `prompts`, a pasta `promptsHome`, o filtro de extensões dos
/// arquivos e as palavras do log mudam; o merge, o preview e a aplicação são os mesmos (o `promptsMerge.ts` é o
/// `snippetsMerge.ts` com os nomes trocados).
pub struct SnippetsSynchroniser {
    pub base: Rc<AbstractSynchroniser<SnippetsSynchroniser>>,
    kind: SnippetsKind,
    /// A pasta de snippets (ou de prompts) do perfil.
    snippets_folder: URI,
}

/// O que distingue o sincronizador de snippets do de prompts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnippetsKind {
    Snippets,
    Prompts,
}

impl SnippetsKind {
    fn sync_resource(self) -> SyncResource {
        match self {
            Self::Snippets => SyncResource::Snippets,
            Self::Prompts => SyncResource::Prompts,
        }
    }

    /// `snippet` ou `prompt`, como o original escreve nas mensagens de log.
    fn singular(self) -> &'static str {
        match self {
            Self::Snippets => "snippet",
            Self::Prompts => "prompt",
        }
    }

    fn plural(self) -> &'static str {
        match self {
            Self::Snippets => "snippets",
            Self::Prompts => "prompts",
        }
    }

    /// `profile.snippetsHome` ou `profile.promptsHome`.
    fn folder(self, profile: &UserDataProfile) -> URI {
        match self {
            Self::Snippets => profile.snippets_home.clone(),
            Self::Prompts => profile.prompts_home.clone(),
        }
    }

    /// Se o arquivo entra na sincronização: `.json` e `.code-snippets` (extensão do nome) para snippets;
    /// `.prompt.md`, `.instructions.md`, `.chatmode.md` e `.agent.md` (fim do caminho) para prompts.
    fn includes(self, ext_uri: &ExtUri, resource: &URI) -> bool {
        match self {
            Self::Snippets => {
                let extension = ext_uri.extname(resource);
                extension == ".json" || extension == ".code-snippets"
            }
            Self::Prompts => {
                let path = resource.path().to_string_lossy();
                [".prompt.md", ".instructions.md", ".chatmode.md", ".agent.md"].iter().any(|extension| path.ends_with(extension))
            }
        }
    }
}

impl Deref for SnippetsSynchroniser {
    type Target = AbstractSynchroniser<SnippetsSynchroniser>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl IDisposable for SnippetsSynchroniser {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl SnippetsSynchroniser {
    pub fn new(profile: UserDataProfile, collection: Option<String>, services: SynchroniserServices) -> Rc<Self> {
        Self::for_kind(SnippetsKind::Snippets, profile, collection, services)
    }

    /// `new PromptsSynchronizer(profile, collection)`.
    pub fn new_prompts(profile: UserDataProfile, collection: Option<String>, services: SynchroniserServices) -> Rc<Self> {
        Self::for_kind(SnippetsKind::Prompts, profile, collection, services)
    }

    fn for_kind(kind: SnippetsKind, profile: UserDataProfile, collection: Option<String>, services: SynchroniserServices) -> Rc<Self> {
        let snippets_folder = kind.folder(&profile);
        let base = AbstractSynchroniser::<Self>::new(IUserDataSyncResource { sync_resource: kind.sync_resource(), profile }, collection, services);
        let this = Rc::new(Self { base: base.clone(), kind, snippets_folder: snippets_folder.clone() });
        base.bind(Rc::downgrade(&this));

        let file_service = base.services.file_service.clone();
        base.register(file_service.watch(&base.services.environment_service.user_roaming_data_home(), None));
        base.register(file_service.watch(&snippets_folder, None));
        let weak: Weak<AbstractSynchroniser<Self>> = Rc::downgrade(&base);
        base.register(file_service.on_did_files_change().listen(move |event| {
            if event.affects(&snippets_folder, &[])
                && let Some(base) = weak.upgrade()
            {
                base.trigger_local_change();
            }
        }));
        this
    }

    fn ext_uri(&self) -> &ExtUri {
        &self.services.ext_uri
    }

    fn basename(&self, resource: &URI) -> String {
        self.ext_uri().basename(resource, None)
    }

    /// `this.extUri.isEqualOrParent(resource, parent)`.
    fn is_equal_or_parent(&self, resource: &URI, parent: &URI) -> bool {
        self.ext_uri().is_equal_or_parent(resource, parent, false, false)
    }

    /// `this.syncPreviewFolder.with({ scheme: USER_DATA_SYNC_SCHEME, authority })`.
    fn virtual_preview_folder(&self, authority: &str) -> URI {
        with_sync_authority(&self.sync_preview_folder, authority)
    }

    /// `getAcceptResult`.
    fn accept_result(&self, resource_preview: &ISnippetsResourcePreview, resource: &URI, content: Option<Option<String>>) -> Result<IAcceptResult<()>, Error> {
        let extra = &resource_preview.extra;
        let has_file_content = extra.file_content.is_some();
        // `fileContent !== null` e `remoteContent !== null`: `undefined` conta como "existe"
        let file_content_is_not_null = has_file_content || extra.file_content_is_undefined;
        let remote_content_is_not_null = resource_preview.remote_content.is_some() || extra.remote_content_is_undefined;

        /* Accept local resource */
        if self.is_equal_or_parent(resource, &self.virtual_preview_folder("local")) {
            return Ok(IAcceptResult {
                content: extra.file_content.as_ref().map(|file_content| file_content.value.to_string()),
                local_change: Change::None,
                remote_change: if has_file_content {
                    if remote_content_is_not_null { Change::Modified } else { Change::Added }
                } else {
                    Change::Deleted
                },
                extra: (),
            });
        }

        /* Accept remote resource */
        if self.is_equal_or_parent(resource, &self.virtual_preview_folder("remote")) {
            return Ok(IAcceptResult {
                content: resource_preview.remote_content.clone(),
                local_change: if remote_content_is_not_null {
                    if has_file_content { Change::Modified } else { Change::Added }
                } else {
                    Change::Deleted
                },
                remote_change: Change::None,
                extra: (),
            });
        }

        /* Accept preview resource */
        if self.is_equal_or_parent(resource, &self.sync_preview_folder) {
            return Ok(match content {
                None => {
                    let preview_result = &extra.preview_result;
                    IAcceptResult { content: preview_result.content.clone(), local_change: preview_result.local_change, remote_change: preview_result.remote_change, extra: () }
                }
                Some(content) => {
                    let is_null = content.is_none();
                    IAcceptResult {
                        content,
                        local_change: if is_null {
                            if file_content_is_not_null { Change::Deleted } else { Change::None }
                        } else {
                            Change::Modified
                        },
                        remote_change: if is_null {
                            if remote_content_is_not_null { Change::Deleted } else { Change::None }
                        } else {
                            Change::Modified
                        },
                        extra: (),
                    }
                }
            });
        }

        Err(Error::new(format!("Invalid Resource: {}", resource.to_string(false).to_string_lossy())))
    }

    /// O preview de um snippet: as URIs virtuais (`base`, `local`, `remote`, `accepted`) e a do próprio preview são
    /// derivadas da pasta de preview e do nome.
    fn resource_preview(&self, key: &JsString, preview_result: IMergeResult<()>, fields: PreviewFields) -> ISnippetsResourcePreview {
        let name = key.to_string_lossy();
        let preview_resource = self.ext_uri().join_path(&self.sync_preview_folder, &[name.as_str()]);
        IResourcePreview {
            base_resource: with_sync_authority(&preview_resource, "base"),
            base_content: fields.base_content,
            local_resource: with_sync_authority(&preview_resource, "local"),
            local_content: fields.local_content,
            remote_resource: with_sync_authority(&preview_resource, "remote"),
            remote_content: fields.remote_content,
            local_change: preview_result.local_change,
            remote_change: preview_result.remote_change,
            accepted_resource: with_sync_authority(&preview_resource, "accepted"),
            preview_resource,
            extra: SnippetsPreviewExtra {
                file_content: fields.file_content,
                file_content_is_undefined: fields.file_content_is_undefined,
                remote_content_is_undefined: fields.remote_content_is_undefined,
                preview_result,
            },
        }
    }

    fn insert_preview(&self, previews: &mut OrderedMap<JsString, ISnippetsResourcePreview>, key: &JsString, preview_result: IMergeResult<()>, fields: PreviewFields) {
        previews.insert(key.clone(), self.resource_preview(key, preview_result, fields));
    }

    /// `getResourcePreviews`.
    fn get_resource_previews(&self, snippets_merge_result: &MergeResult, local_file_content: &LocalFiles, remote_snippets: &Dictionary, base_snippets: &Dictionary) -> Vec<ISnippetsResourcePreview> {
        let mut resource_previews: OrderedMap<JsString, ISnippetsResourcePreview> = OrderedMap::new();
        let sources = PreviewSources { local_files: local_file_content, remote: remote_snippets, base: base_snippets };
        let text = |value: &JsString| value.to_string_lossy();

        /* Snippets added remotely -> add locally */
        for (key, value) in snippets_merge_result.local.added.iter() {
            let preview_result = merge_result(Some(text(value)), false, Change::Added, Change::None);
            let fields = PreviewFields { base_content: None, ..sources.fields(key, Read::Null, Read::Raw) };
            self.insert_preview(&mut resource_previews, key, preview_result, fields);
        }

        /* Snippets updated remotely -> update locally */
        for (key, value) in snippets_merge_result.local.updated.iter() {
            let preview_result = merge_result(Some(text(value)), false, Change::Modified, Change::None);
            self.insert_preview(&mut resource_previews, key, preview_result, sources.fields(key, Read::Raw, Read::Raw));
        }

        /* Snippets removed remotely -> remove locally */
        for key in &snippets_merge_result.local.removed {
            let preview_result = merge_result(None, false, Change::Deleted, Change::None);
            self.insert_preview(&mut resource_previews, key, preview_result, sources.fields(key, Read::Raw, Read::Null));
        }

        /* Snippets added locally -> add remotely */
        for (key, value) in snippets_merge_result.remote.added.iter() {
            let preview_result = merge_result(Some(text(value)), false, Change::None, Change::Added);
            self.insert_preview(&mut resource_previews, key, preview_result, sources.fields(key, Read::Raw, Read::Null));
        }

        /* Snippets updated locally -> update remotely */
        for (key, value) in snippets_merge_result.remote.updated.iter() {
            let preview_result = merge_result(Some(text(value)), false, Change::None, Change::Modified);
            self.insert_preview(&mut resource_previews, key, preview_result, sources.fields(key, Read::Raw, Read::Raw));
        }

        /* Snippets removed locally -> remove remotely */
        for key in &snippets_merge_result.remote.removed {
            let preview_result = merge_result(None, false, Change::None, Change::Deleted);
            self.insert_preview(&mut resource_previews, key, preview_result, sources.fields(key, Read::Null, Read::Raw));
        }

        /* Snippets with conflicts */
        for key in &snippets_merge_result.conflicts {
            let preview_result = merge_result(
                base_snippets.get(key).map(text),
                true,
                if local_file_content.contains_key(key) { Change::Modified } else { Change::Added },
                if remote_snippets.get(key).is_some_and(|value| !value.is_empty()) { Change::Modified } else { Change::Added },
            );
            self.insert_preview(&mut resource_previews, key, preview_result, sources.fields(key, Read::Truthy, Read::Truthy));
        }

        /* Unmodified Snippets */
        for (key, file_content) in local_file_content.iter() {
            if !resource_previews.contains_key(key) {
                let preview_result = merge_result(Some(file_content.value.to_string()), false, Change::None, Change::None);
                self.insert_preview(&mut resource_previews, key, preview_result, sources.fields(key, Read::Truthy, Read::Truthy));
            }
        }

        resource_previews.iter().map(|(_, resource_preview)| resource_preview.clone()).collect()
    }

    /// `updateLocalBackup`.
    async fn update_local_backup(&self, resource_previews: &[(ISnippetsResourcePreview, IAcceptResult<()>)]) -> Result<(), Error> {
        let mut local = LocalFiles::new();
        for (resource_preview, _) in resource_previews {
            if let Some(file_content) = &resource_preview.extra.file_content {
                local.insert(JsString::from(self.basename(&resource_preview.local_resource)), file_content.clone());
            }
        }
        self.backup_local(&stringify_json(&dictionary_to_json(&to_snippets_contents(&local)))).await
    }

    /// `updateLocalSnippets`.
    async fn update_local_snippets(&self, resource_previews: &[(ISnippetsResourcePreview, IAcceptResult<()>)], force: bool) -> Result<(), Error> {
        let (label, file_service) = (&self.sync_resource_log_label, &self.services.file_service);
        let log_service = &self.services.log_service;
        let noun = self.kind.singular();
        for (resource_preview, accept_result) in resource_previews {
            let local_change = resource_preview.local_change;
            if local_change == Change::None {
                continue;
            }
            let key = self.basename(&resource_preview.remote_resource);
            let resource = self.ext_uri().join_path(&self.snippets_folder, &[key.as_str()]);
            let name = self.basename(&resource);
            let args = [LogArg::from(name.as_str())];

            // Removed
            if local_change == Change::Deleted {
                log_service.trace_args(&format!("{label}: Deleting {noun}..."), &args);
                file_service.del(&resource, None).await?;
                log_service.info_args(&format!("{label}: Deleted {noun}"), &args);
            }
            // Added
            else if local_change == Change::Added {
                log_service.trace_args(&format!("{label}: Creating {noun}..."), &args);
                let content = accepted_content(accept_result)?;
                file_service.create_file(&resource, Some(FileContentSource::from(content.as_str())), Some(CreateFileOptions { overwrite: force })).await?;
                log_service.info_args(&format!("{label}: Created {noun}"), &args);
            }
            // Updated
            else {
                log_service.trace_args(&format!("{label}: Updating {noun}..."), &args);
                let content = accepted_content(accept_result)?;
                let options = if force {
                    None
                } else {
                    resource_preview.extra.file_content.as_ref().map(|file_content| WriteFileOptions { mtime: Some(file_content.mtime()), etag: Some(file_content.etag().to_string()), ..WriteFileOptions::default() })
                };
                file_service.write_file(&resource, FileContentSource::from(content.as_str()), options).await?;
                log_service.info_args(&format!("{label}: Updated {noun}"), &args);
            }
        }
        Ok(())
    }

    /// `updateRemoteSnippets`.
    async fn update_remote_snippets(&self, resource_previews: &[(ISnippetsResourcePreview, IAcceptResult<()>)], mut remote_user_data: IRemoteUserData, force_push: bool) -> Result<IRemoteUserData, Error> {
        let current_snippets = match &remote_user_data.sync_data {
            Some(sync_data) => parse_snippets(sync_data)?,
            None => Dictionary::new(),
        };
        let mut new_snippets = current_snippets.clone();

        for (resource_preview, accept_result) in resource_previews {
            let remote_change = resource_preview.remote_change;
            if remote_change != Change::None {
                let key = JsString::from(self.basename(&resource_preview.local_resource));
                if remote_change == Change::Deleted {
                    new_snippets.remove(&key);
                } else {
                    new_snippets.insert(key, JsString::from(accepted_content(accept_result)?));
                }
            }
        }

        if !are_same(&current_snippets, &new_snippets) {
            // update remote
            self.services.log_service.trace(&format!("{}: Updating remote {}...", self.sync_resource_log_label, self.kind.plural()));
            remote_user_data = self.update_remote_user_data(stringify_json(&dictionary_to_json(&new_snippets)), if force_push { None } else { Some(remote_user_data.r#ref.clone()) }).await?;
            self.services.log_service.info(&format!("{}: Updated remote {}", self.sync_resource_log_label, self.kind.plural()));
        }
        Ok(remote_user_data)
    }

    /// `getSnippetsFileContents`: os arquivos `.json` e `.code-snippets` da pasta de snippets, pelo nome relativo.
    async fn get_snippets_file_contents(&self) -> Result<LocalFiles, Error> {
        let mut snippets = LocalFiles::new();
        let stat = match self.services.file_service.resolve(&self.snippets_folder, None).await {
            Ok(stat) => stat,
            Err(error) => {
                // No snippets
                if FileOperationError::from_error(&error).is_some_and(|file_error| file_error.file_operation_result == FileOperationResult::FileNotFound) {
                    return Ok(snippets);
                }
                return Err(error);
            }
        };
        for entry in stat.children.unwrap_or_default() {
            let resource = entry.resource;
            if self.kind.includes(self.ext_uri(), &resource) {
                let key = self.ext_uri().relative_path(&self.snippets_folder, &resource).expect("o snippet está dentro da pasta de snippets");
                let content = self.services.file_service.read_file(&resource, None, None).await?;
                snippets.insert(JsString::from(key), content);
            }
        }
        Ok(snippets)
    }
}

impl SynchroniserImpl for SnippetsSynchroniser {
    type Preview = SnippetsPreviewExtra;
    type Accept = ();

    fn version(&self) -> f64 {
        SNIPPETS_DATA_VERSION
    }

    fn generate_sync_preview(
        self: Rc<Self>,
        remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        is_remote_data_from_current_machine: bool,
        _user_data_sync_configuration: IUserDataSyncConfiguration,
        _token: CancellationToken,
    ) -> Promise<Vec<ISnippetsResourcePreview>> {
        async_fn(async move {
            let local = self.get_snippets_file_contents().await?;
            let local_snippets = to_snippets_contents(&local);
            let remote_snippets = match &remote_user_data.sync_data {
                Some(sync_data) => Some(parse_snippets(sync_data)?),
                None => None,
            };

            // Use remote data as last sync data if last sync data does not exist and remote data is from same machine
            let last_sync_user_data = if last_sync_user_data.is_none() && is_remote_data_from_current_machine { Some(remote_user_data.clone()) } else { last_sync_user_data };
            let last_sync_snippets = match last_sync_user_data.as_ref().and_then(|last_sync_user_data| last_sync_user_data.sync_data.as_ref()) {
                Some(sync_data) => Some(parse_snippets(sync_data)?),
                None => None,
            };

            if remote_snippets.is_some() {
                self.services.log_service.trace(&format!("{}: Merging remote {1} with local {1}...", self.sync_resource_log_label, self.kind.plural()));
            } else {
                self.services.log_service.trace(&format!("{}: Remote {1} does not exist. Synchronizing {1} for the first time.", self.sync_resource_log_label, self.kind.plural()));
            }

            let merge_result = merge(&local_snippets, remote_snippets.as_ref(), last_sync_snippets.as_ref());
            Ok(self.get_resource_previews(&merge_result, &local, &remote_snippets.unwrap_or_default(), &last_sync_snippets.unwrap_or_default()))
        })
    }

    fn has_remote_changed(self: Rc<Self>, last_sync_user_data: IRemoteUserData) -> Promise<bool> {
        async_fn(async move {
            let Some(sync_data) = &last_sync_user_data.sync_data else { return Ok(true) };
            let last_sync_snippets = parse_snippets(sync_data)?;
            let local = self.get_snippets_file_contents().await?;
            let local_snippets = to_snippets_contents(&local);
            let merge_result = merge(&local_snippets, Some(&last_sync_snippets), Some(&last_sync_snippets));
            Ok(!merge_result.remote.added.is_empty() || !merge_result.remote.updated.is_empty() || !merge_result.remote.removed.is_empty() || !merge_result.conflicts.is_empty())
        })
    }

    fn get_merge_result(self: Rc<Self>, resource_preview: ISnippetsResourcePreview, _token: CancellationToken) -> Promise<IMergeResult<()>> {
        Promise::resolved(resource_preview.extra.preview_result)
    }

    fn get_accept_result(self: Rc<Self>, resource_preview: ISnippetsResourcePreview, resource: URI, content: Option<Option<String>>, _token: CancellationToken) -> Promise<IAcceptResult<()>> {
        async_fn(async move { self.accept_result(&resource_preview, &resource, content) })
    }

    fn apply_result(
        self: Rc<Self>,
        remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        resource_previews: Vec<(ISnippetsResourcePreview, IAcceptResult<()>)>,
        force: bool,
    ) -> Promise<()> {
        async_fn(async move {
            let mut remote_user_data = remote_user_data;
            let label = &self.sync_resource_log_label;
            let plural = self.kind.plural();
            if resource_previews.iter().all(|(resource_preview, _)| resource_preview.local_change == Change::None && resource_preview.remote_change == Change::None) {
                self.services.log_service.info(&format!("{label}: No changes found during synchronizing {plural}."));
            }

            if resource_previews.iter().any(|(resource_preview, _)| resource_preview.local_change != Change::None) {
                // back up all snippets
                self.update_local_backup(&resource_previews).await?;
                self.update_local_snippets(&resource_previews, force).await?;
            }

            if resource_previews.iter().any(|(resource_preview, _)| resource_preview.remote_change != Change::None) {
                remote_user_data = self.update_remote_snippets(&resource_previews, remote_user_data, force).await?;
            }

            if last_sync_user_data.as_ref().map(|last_sync_user_data| &last_sync_user_data.r#ref) != Some(&remote_user_data.r#ref) {
                // update last sync
                self.services.log_service.trace(&format!("{label}: Updating last synchronized {plural}..."));
                self.update_last_sync_user_data(remote_user_data, Vec::new()).await?;
                self.services.log_service.info(&format!("{label}: Updated last synchronized {plural}"));
            }

            for (resource_preview, _) in &resource_previews {
                // Delete the preview
                let _ = self.services.file_service.del(&resource_preview.preview_resource, None).await;
            }
            Ok(())
        })
    }

    fn has_local_data(self: Rc<Self>) -> Promise<bool> {
        async_fn(async move {
            // um erro de leitura vale como "não há dados locais"
            Ok(self.get_snippets_file_contents().await.is_ok_and(|local_snippets| !local_snippets.is_empty()))
        })
    }

    fn resolve_content(self: Rc<Self>, uri: URI) -> Promise<Option<String>> {
        let is_virtual = ["remote", "local", "base", "accepted"].into_iter().any(|authority| self.is_equal_or_parent(&uri, &self.virtual_preview_folder(authority)));
        if is_virtual {
            return self.resolve_preview_content(&uri);
        }
        Promise::resolved(None)
    }
}

/// `SnippetsInitializer`. Dá `Deref` para o [`AbstractInitializer`].
pub struct SnippetsInitializer {
    base: Rc<AbstractInitializer>,
}

impl Deref for SnippetsInitializer {
    type Target = AbstractInitializer;

    fn deref(&self) -> &AbstractInitializer {
        &self.base
    }
}

impl SnippetsInitializer {
    pub fn new(
        file_service: Rc<FileService>,
        user_data_profiles_service: Rc<dyn IUserDataProfilesService>,
        environment_service: Rc<dyn EnvironmentService>,
        log_service: LogServiceRef,
        storage_service: Rc<dyn StorageService>,
        ext_uri: ExtUri,
    ) -> Rc<Self> {
        let base = AbstractInitializer::new(SyncResource::Snippets, user_data_profiles_service, environment_service, log_service, file_service, storage_service, ext_uri);
        let this = Rc::new(Self { base });
        let weak: Weak<dyn InitializerImpl> = Rc::downgrade(&this) as Weak<dyn InitializerImpl>;
        this.base.bind(weak);
        this
    }

    /// `isEmpty`: a pasta de snippets do perfil padrão não existe ou não tem filhos.
    async fn is_empty(base: &AbstractInitializer) -> Result<bool, Error> {
        match base.file_service.resolve(&base.user_data_profiles_service.default_profile().snippets_home, None).await {
            Ok(stat) => Ok(stat.children.is_none_or(|children| children.is_empty())),
            Err(error) => Ok(FileOperationError::from_error(&error).is_some_and(|file_error| file_error.file_operation_result == FileOperationResult::FileNotFound)),
        }
    }
}

impl InitializerImpl for SnippetsInitializer {
    fn do_initialize(&self, remote_user_data: IRemoteUserData) -> Promise<()> {
        let base = self.base.clone();
        async_fn(async move {
            let remote_snippets = match &remote_user_data.sync_data {
                Some(sync_data) => Some(parse_snippets_content(&sync_data.content)?),
                None => None,
            };
            let Some(remote_snippets) = remote_snippets else {
                base.log_service.info("Skipping initializing snippets because remote snippets does not exist.");
                return Ok(());
            };

            let is_empty = Self::is_empty(&base).await?;
            if !is_empty {
                base.log_service.info("Skipping initializing snippets because local snippets exist.");
                return Ok(());
            }

            for (key, content) in remote_snippets.iter() {
                if !content.is_empty() {
                    let resource = base.ext_uri.join_path(&base.user_data_profiles_service.default_profile().snippets_home, &[key.to_string_lossy().as_str()]);
                    base.file_service.create_file(&resource, Some(FileContentSource::from(content.to_string_lossy().as_str())), None).await?;
                    base.log_service.info_args("Created snippet", &[LogArg::from(base.ext_uri.basename(&resource, None).as_str())]);
                }
            }

            base.update_last_sync_user_data(remote_user_data, Vec::new()).await
        })
    }
}
