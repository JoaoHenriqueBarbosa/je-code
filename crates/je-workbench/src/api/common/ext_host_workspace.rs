//! Porte de `src/vs/workbench/api/common/extHostWorkspace.ts`: o lado da extensão do workspace
//! (`vscode.workspace.workspaceFolders`, `updateWorkspaceFolders`, `findFiles`, `findTextInFiles`, os provedores de
//! identidade de sessão de edição e de URI canônico, a confiança e a codificação de texto).
//!
//! Divergências:
//! - `vscode.WorkspaceFolder` (um objeto mutável que o host mantém "vivo" entre as atualizações do workspace) é o
//!   [`Rc<WorkspaceFolder>`](WorkspaceFolder): `name` e `index` são células, e a identidade do original (`===`) é
//!   [`Rc::ptr_eq`]. `ExtHostWorkspaceImpl extends Workspace` vira composição: [`ExtHostWorkspaceImpl`] guarda o
//!   `Workspace` da plataforma (que `ExtHostConfigProvider` lê) e acrescenta `name`, `isUntitled` e a árvore de pastas.
//! - `IExtHostFileSystemInfo`, `IExtHostInitDataService` e `IURITransformerService` (de `extHostFileSystemInfo.ts`,
//!   `extHostInitDataService.ts` e `extHostUriTransformerService.ts`, ainda não portados) são os traits mínimos que este
//!   arquivo lê (`getCapabilities`, `workspace`, `transformOutgoingScheme`). O trait [`IExtHostWorkspaceProvider`] é
//!   implementado só por [`ExtHostWorkspace`], e é onde moram `getWorkspaceFolder2`, `resolveWorkspaceFolder`,
//!   `getWorkspaceFolders2`, `resolveProxy`, `lookupAuthorization`, `lookupKerberosAuthorization` e `loadCertificates`.
//! - `RelativePattern` (`extHostTypes.ts`) e `GlobPattern.from` (`extHostTypeConverters.ts`) ainda não existem no porte:
//!   [`RelativePattern`], [`GlobPattern`] e [`glob_pattern_from`] são o que este arquivo precisa deles e devem migrar
//!   para `ext_host_types` e `ext_host_type_converters` quando esses receberem o resto. Pelo mesmo motivo os tipos de opção
//!   da API `vscode` da busca (`FindFiles2Options`, `FindTextInFilesOptions`, `FindTextInFilesOptions2`,
//!   `TextSearchResult`...) são definidos aqui; `vscode.TextSearchQuery` é o [`PatternInfo`] (mesma forma) e
//!   `vscode.TextSearchQuery2` é o [`TextSearchQuery2`].
//! - `exclude: GlobPattern | null | undefined` de `findFiles` é [`FindFilesExclude`] (`undefined`, `null`, padrão).
//!   O `token = CancellationToken.None` padrão é um argumento explícito. A reembalagem de um token estrangeiro de
//!   `_findFilesBase` (que cobre um módulo `CancellationToken` de outro bundle) não existe: o token é sempre o nativo.
//! - `updateWorkspaceFolders(index, deleteCount)` recebe [`Option<f64>`]: `None` é um valor que não é `number`
//!   (o `null` do original). As verificações `Array.isArray`, `URI.isUri` e `typeof` dos demais argumentos são do tipo.
//! - `if (this._proxy)` de `updateWorkspaceFolders` vale sempre (o proxy nunca é ausente). O `error.toString()` da mensagem
//!   é o `Display` do [`Error`].
//! - Os provedores (`EditSessionIdentityProvider`, `CanonicalUriProvider`) são traits; o método opcional
//!   (`provideEditSessionIdentityMatch?.`, `provideCanonicalUri?.`) devolve `None` quando o provedor não o tem.
//! - `listener.extension` do `AsyncEmitter` (uma propriedade que o original pendura na função do listener) é uma tabela
//!   de listeners embrulhados em `will_create_listeners`, procurada por identidade ([`Rc::ptr_eq`]).
//! - O `revive` das URIs e dos resultados de busca recebidos não existe: o protocolo já entrega [`URI`] e
//!   [`ITextSearchResult`]. Os argumentos de log que o original passa como objeto (`workspaceFolder`) saem como o
//!   texto da URI.
//! - `$setConfigProvider` guarda o provedor como [`Weak`]: quem o possui é o `ExtHostConfiguration` (o original guarda
//!   uma referência forte, mas o ciclo só existe no Rust).
//! - Os métodos de RPC (`$initializeWorkspace`...) são o trait [`ExtHostWorkspaceShape`] (prefixo `rpc_`).

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::time::Instant;

use je_base::JsString;
use je_base::arrays::{self, Delta, OneOrMany};
use je_base::async_::{AsyncIterableProducer, Barrier, async_fn};
use je_base::buffer::{VSBuffer, VSBufferReadableStream, buffer_to_stream, readable_to_buffer};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::{AsyncEmitter, Emitter, Event, Listener, PromiseJoin, WaitUntilEvent};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::numbers::Counter;
use je_base::promise::{MaybePromise, Promise};
use je_base::resources::ExtUri;
use je_base::severity::Severity;
use je_base::stopwatch::StopWatch;
use je_base::stream::consume_stream;
use je_base::strings::compare;
use je_base::ternary_search_tree::{TernarySearchTree, UriPredicate};
use je_base::uri::{URI, UriComponents};
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::files::files::FileSystemProviderCapabilities;
use je_platform::log::{LogArg, LogMessage, LogServiceRef, Logger};
use je_platform::request::request::{AuthInfo, Credentials};
use je_platform::resources::{ext_uri, fs_path, host_environment, uri_file};
use je_platform::workspace::{Workspace, WorkspaceFolder as PlatformWorkspaceFolder};
use je_platform::workspace_trust::{ResourceTrustRequestOptions, WorkspaceTrustRequestOptions};

use super::ext_host_configuration::ExtHostConfigProvider;
use super::ext_host_protocol::{
    EditSessionIdentityMatch, EncodingOptionDto, ExtHostWorkspaceShape, IMainContext, IRawFileMatch2, IRelativePatternDto, ITextSearchComplete, IWorkspaceData,
    MainContext, MainThreadMessageOptions, MainThreadMessageServiceShape, MainThreadMessageSource, MainThreadTelemetryShape, MainThreadWorkspaceShape,
    SaveOptionsDto, WorkspaceFolderDataDto, WorkspaceFolderToAdd,
};
use super::ext_host_types::{Position as ExtPosition, Range as ExtRange};
use crate::services::search::query_builder::{
    ICommonQueryBuilderOptions, IFileQueryBuilderOptions, ITextQueryBuilderOptions, SearchPathPatternBuilder, SearchPatternBuilder,
};
use crate::services::search::search::{ITextSearchPreviewOptions, ITextSearchResult, PatternInfo};
use crate::services::search::search_ext_types::{
    ExcludeSettingOptions, Range as SearchExtRange, TextSearchComplete2, TextSearchContext2, TextSearchMatch2, TextSearchMatchRanges, TextSearchQuery2,
    TextSearchResult2,
};
use crate::services::textfile::encoding::{DecodeStreamOptions, EncodeOptions, UTF8, to_decode_stream, to_encode_readable};

// ---- serviços de que este arquivo depende ----------------------------------------------------------------

/// `IExtHostFileSystemInfo`, só o `getCapabilities`: as capacidades do provedor de um esquema, se houver.
pub trait IExtHostFileSystemInfo {
    fn get_capabilities(&self, scheme: &str) -> Option<FileSystemProviderCapabilities>;
}

/// `IExtHostInitDataService`, só o `workspace` (os dados do workspace do começo da sessão).
pub trait IExtHostInitDataService {
    fn workspace(&self) -> Option<IWorkspaceData>;
}

/// `IURITransformerService`, só o `transformOutgoingScheme`.
pub trait IURITransformerService {
    fn transform_outgoing_scheme(&self, scheme: &str) -> String;
}

/// `IExtHostWorkspaceProvider`.
pub trait IExtHostWorkspaceProvider {
    fn get_workspace_folder2(&self, uri: &URI, resolve_parent: Option<bool>) -> Promise<Option<Rc<WorkspaceFolder>>>;
    fn resolve_workspace_folder(&self, uri: &URI) -> Promise<Option<Rc<WorkspaceFolder>>>;
    fn get_workspace_folders2(&self) -> Promise<Option<Vec<Rc<WorkspaceFolder>>>>;
    fn resolve_proxy(&self, url: &str) -> Promise<Option<String>>;
    fn lookup_authorization(&self, auth_info: AuthInfo) -> Promise<Option<Credentials>>;
    fn lookup_kerberos_authorization(&self, url: &str) -> Promise<Option<String>>;
    fn load_certificates(&self) -> Promise<Vec<String>>;
}

// ---- RelativePattern e GlobPattern ----------------------------------------------------------------------

/// A base de um [`RelativePattern`]: `vscode.WorkspaceFolder | URI | string`.
#[derive(Clone)]
pub enum RelativePatternBase {
    Path(String),
    Uri(URI),
    Folder(Rc<WorkspaceFolder>),
}

impl From<&str> for RelativePatternBase {
    fn from(path: &str) -> Self {
        Self::Path(path.to_owned())
    }
}

impl From<URI> for RelativePatternBase {
    fn from(uri: URI) -> Self {
        Self::Uri(uri)
    }
}

impl From<Rc<WorkspaceFolder>> for RelativePatternBase {
    fn from(folder: Rc<WorkspaceFolder>) -> Self {
        Self::Folder(folder)
    }
}

/// `RelativePattern` (`extHostTypes.ts`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelativePattern {
    pub pattern: String,
    base: String,
    base_uri: URI,
}

impl RelativePattern {
    pub fn new(base: impl Into<RelativePatternBase>, pattern: &str) -> RelativePattern {
        let base_uri = match base.into() {
            RelativePatternBase::Path(path) => uri_file(&path),
            RelativePatternBase::Uri(uri) => uri,
            RelativePatternBase::Folder(folder) => folder.uri().clone(),
        };
        RelativePattern { pattern: pattern.to_owned(), base: fs_path(&base_uri), base_uri }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    pub fn base_uri(&self) -> &URI {
        &self.base_uri
    }

    /// `set baseUri`: muda também o `base`.
    pub fn set_base_uri(&mut self, base_uri: URI) {
        self.base = fs_path(&base_uri);
        self.base_uri = base_uri;
    }

    /// `set base`: muda também o `baseUri`.
    pub fn set_base(&mut self, base: &str) {
        self.base_uri = uri_file(base);
        self.base = base.to_owned();
    }

    /// `toJSON()`.
    pub fn to_json(&self) -> IRelativePatternDto {
        IRelativePatternDto { pattern: self.pattern.clone(), base: self.base.clone(), base_uri: self.base_uri.clone() }
    }
}

/// `vscode.GlobPattern`: `string | RelativePattern`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobPattern {
    Pattern(String),
    Relative(RelativePattern),
}

impl From<&str> for GlobPattern {
    fn from(pattern: &str) -> Self {
        Self::Pattern(pattern.to_owned())
    }
}

impl From<RelativePattern> for GlobPattern {
    fn from(pattern: RelativePattern) -> Self {
        Self::Relative(pattern)
    }
}

/// `string | IRelativePatternDto`: o que `GlobPattern.from` devolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobPatternDto {
    Pattern(String),
    Relative(IRelativePatternDto),
}

/// `typeConverters.GlobPattern.from`.
pub fn glob_pattern_from(pattern: &GlobPattern) -> GlobPatternDto {
    match pattern {
        GlobPattern::Relative(relative) => GlobPatternDto::Relative(relative.to_json()),
        GlobPattern::Pattern(pattern) => GlobPatternDto::Pattern(pattern.clone()),
    }
}

// ---- opções da API de busca -----------------------------------------------------------------------------

/// `exclude: vscode.GlobPattern | null | undefined` de `findFiles`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindFilesExclude {
    /// `undefined`: vale o padrão (`files.exclude`).
    Undefined,
    /// `null`: nenhuma exclusão, nem a de `files.exclude`.
    Null,
    Pattern(GlobPattern),
}

/// `useIgnoreFiles` de `FindFiles2Options` e de `FindTextInFilesOptions2`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UseIgnoreFilesOptions {
    pub local: Option<bool>,
    pub parent: Option<bool>,
    pub global: Option<bool>,
}

/// `vscode.FindFiles2Options`.
#[derive(Clone, Debug, Default)]
pub struct FindFiles2Options {
    pub exclude: Option<Vec<GlobPattern>>,
    pub use_ignore_files: Option<UseIgnoreFilesOptions>,
    pub use_exclude_settings: Option<ExcludeSettingOptions>,
    pub follow_symlinks: Option<bool>,
    pub case_insensitive: Option<bool>,
    pub max_results: Option<i64>,
}

/// `vscode.FindTextInFilesOptions` mais o `useSearchExclude` que o host aceita.
#[derive(Clone, Debug, Default)]
pub struct FindTextInFilesOptions {
    pub include: Option<GlobPattern>,
    pub exclude: Option<GlobPattern>,
    pub max_results: Option<i64>,
    pub use_default_excludes: Option<bool>,
    pub use_ignore_files: Option<bool>,
    pub use_global_ignore_files: Option<bool>,
    pub use_parent_ignore_files: Option<bool>,
    pub follow_symlinks: Option<bool>,
    pub encoding: Option<String>,
    pub preview_options: Option<ITextSearchPreviewOptions>,
    pub before_context: Option<i64>,
    pub after_context: Option<i64>,
    pub use_search_exclude: Option<bool>,
}

/// `previewOptions` de `FindTextInFilesOptions2`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextSearchPreviewOptions2 {
    pub num_match_lines: Option<i64>,
    pub chars_per_line: Option<i64>,
}

/// `vscode.FindTextInFilesOptions2`.
#[derive(Clone, Debug, Default)]
pub struct FindTextInFilesOptions2 {
    pub include: Option<Vec<GlobPattern>>,
    pub exclude: Option<Vec<GlobPattern>>,
    pub use_ignore_files: Option<UseIgnoreFilesOptions>,
    pub use_exclude_settings: Option<ExcludeSettingOptions>,
    pub follow_symlinks: Option<bool>,
    pub encoding: Option<String>,
    pub max_results: Option<i64>,
    pub case_insensitive: Option<bool>,
    pub preview_options: Option<TextSearchPreviewOptions2>,
    pub surrounding_context: Option<i64>,
}

/// `vscode.TextSearchMatch.preview`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextSearchMatchPreview {
    pub text: JsString,
    pub matches: OneOrMany<ExtRange>,
}

/// `vscode.TextSearchMatch`.
#[derive(Clone, Debug, PartialEq)]
pub struct TextSearchMatchResult {
    pub uri: URI,
    pub ranges: OneOrMany<ExtRange>,
    pub preview: TextSearchMatchPreview,
}

/// `vscode.TextSearchContext`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSearchContextResult {
    pub uri: URI,
    pub text: JsString,
    pub line_number: i64,
}

/// `vscode.TextSearchResult`.
#[derive(Clone, Debug, PartialEq)]
pub enum TextSearchResult {
    Match(TextSearchMatchResult),
    Context(TextSearchContextResult),
}

/// `vscode.FindTextInFilesResponse`.
pub struct FindTextInFilesResponse {
    pub results: Rc<AsyncIterableProducer<TextSearchResult2>>,
    pub complete: Promise<TextSearchComplete2>,
}

// ---- provedores -----------------------------------------------------------------------------------------

/// `vscode.EditSessionIdentityProvider`.
pub trait EditSessionIdentityProvider {
    fn provide_edit_session_identity(&self, workspace_folder: &Rc<WorkspaceFolder>, token: &CancellationToken) -> Promise<Option<String>>;

    /// `provideEditSessionIdentityMatch?`: `None` quando o provedor não implementa.
    fn provide_edit_session_identity_match(
        &self,
        _identity1: &str,
        _identity2: &str,
        _token: &CancellationToken,
    ) -> Option<Promise<Option<EditSessionIdentityMatch>>> {
        None
    }
}

/// `vscode.CanonicalUriRequestOptions`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalUriRequestOptions {
    pub target_scheme: String,
}

/// `vscode.CanonicalUriProvider`.
pub trait CanonicalUriProvider {
    /// `provideCanonicalUri?`: `None` quando o provedor não implementa.
    fn provide_canonical_uri(&self, _uri: &URI, _options: &CanonicalUriRequestOptions, _token: &CancellationToken) -> Option<Promise<Option<URI>>> {
        None
    }
}

/// Os dados de `vscode.EditSessionIdentityWillCreateEvent` (o `waitUntil` é o de [`WaitUntilEvent`]).
#[derive(Clone)]
pub struct EditSessionIdentityWillCreateEventData {
    pub workspace_folder: Rc<WorkspaceFolder>,
}

/// `{ uri?: vscode.Uri; encoding?: string }` de `decode` e `encode`.
#[derive(Clone, Debug, Default)]
pub struct EncodeDecodeArgs {
    pub uri: Option<URI>,
    pub encoding: Option<String>,
}

/// O `string | vscode.Uri | undefined` de `getRelativePath`.
#[derive(Clone, Debug)]
pub enum RelativePathInput {
    Path(String),
    Uri(URI),
    Undefined,
}

impl From<&str> for RelativePathInput {
    fn from(path: &str) -> Self {
        Self::Path(path.to_owned())
    }
}

impl From<URI> for RelativePathInput {
    fn from(uri: URI) -> Self {
        Self::Uri(uri)
    }
}

// ---- pastas ---------------------------------------------------------------------------------------------

/// `vscode.WorkspaceFolder` (o `MutableWorkspaceFolder` do original): o host troca o `name` e o `index` das pastas que
/// continuam no workspace, para que o objeto que a extensão guardou siga vivo.
#[derive(Debug)]
pub struct WorkspaceFolder {
    uri: URI,
    name: RefCell<String>,
    index: Cell<usize>,
}

impl WorkspaceFolder {
    pub fn new(uri: URI, name: impl Into<String>, index: usize) -> Rc<WorkspaceFolder> {
        Rc::new(WorkspaceFolder { uri, name: RefCell::new(name.into()), index: Cell::new(index) })
    }

    pub fn uri(&self) -> &URI {
        &self.uri
    }

    pub fn name(&self) -> String {
        self.name.borrow().clone()
    }

    pub fn index(&self) -> usize {
        self.index.get()
    }

    fn to_platform(&self) -> PlatformWorkspaceFolder {
        PlatformWorkspaceFolder::new(self.uri.clone(), self.name(), self.index(), None)
    }
}

/// `IWorkspaceFoldersChangeEvent` do `vscode.workspace.onDidChangeWorkspaceFolders`.
#[derive(Clone, Debug)]
pub struct WorkspaceFoldersChangeEvent {
    pub added: Vec<Rc<WorkspaceFolder>>,
    pub removed: Vec<Rc<WorkspaceFolder>>,
}

type FileSystemInfo = Rc<dyn IExtHostFileSystemInfo>;

/// `ignorePathCasing(uri, extHostFileSystemInfo)`.
fn ignore_path_casing(uri: &URI, ext_host_file_system_info: &dyn IExtHostFileSystemInfo) -> bool {
    let capabilities = ext_host_file_system_info.get_capabilities(&uri.scheme().to_string_lossy());
    !capabilities.is_some_and(|capabilities| capabilities.intersects(FileSystemProviderCapabilities::PATH_CASE_SENSITIVE))
}

fn ignore_path_casing_predicate(ext_host_file_system_info: &FileSystemInfo) -> UriPredicate {
    let info = ext_host_file_system_info.clone();
    Rc::new(move |uri: &URI| ignore_path_casing(uri, info.as_ref()))
}

/// `new ExtUri(uri => ignorePathCasing(uri, extHostFileSystemInfo))`.
fn ext_uri_of(ext_host_file_system_info: &FileSystemInfo) -> ExtUri {
    let info = ext_host_file_system_info.clone();
    ExtUri::new(host_environment(), move |uri| ignore_path_casing(uri, info.as_ref()))
}

fn is_folder_equal(folder_a: &URI, folder_b: &URI, ext_host_file_system_info: &FileSystemInfo) -> bool {
    ext_uri_of(ext_host_file_system_info).is_equal(Some(folder_a), Some(folder_b), false)
}

fn compare_text(a: &str, b: &str) -> f64 {
    let (a, b): (Vec<u16>, Vec<u16>) = (a.encode_utf16().collect(), b.encode_utf16().collect());
    f64::from(compare(&a, &b))
}

fn compare_uri_text(a: &URI, b: &URI) -> f64 {
    f64::from(compare(a.to_string(false).units(), b.to_string(false).units()))
}

type FolderCompare = fn(&Rc<WorkspaceFolder>, &Rc<WorkspaceFolder>, &FileSystemInfo) -> f64;

fn compare_workspace_folder_by_uri(a: &Rc<WorkspaceFolder>, b: &Rc<WorkspaceFolder>, ext_host_file_system_info: &FileSystemInfo) -> f64 {
    if is_folder_equal(a.uri(), b.uri(), ext_host_file_system_info) { 0.0 } else { compare_uri_text(a.uri(), b.uri()) }
}

fn compare_workspace_folder_by_uri_and_name_and_index(a: &Rc<WorkspaceFolder>, b: &Rc<WorkspaceFolder>, ext_host_file_system_info: &FileSystemInfo) -> f64 {
    if a.index() != b.index() {
        return if a.index() < b.index() { -1.0 } else { 1.0 };
    }

    if is_folder_equal(a.uri(), b.uri(), ext_host_file_system_info) { compare_text(&a.name(), &b.name()) } else { compare_uri_text(a.uri(), b.uri()) }
}

fn delta(
    old_folders: &[Rc<WorkspaceFolder>],
    new_folders: &[Rc<WorkspaceFolder>],
    compare: FolderCompare,
    ext_host_file_system_info: &FileSystemInfo,
) -> Delta<Rc<WorkspaceFolder>> {
    let sorted = |folders: &[Rc<WorkspaceFolder>]| {
        let mut sorted = folders.to_vec();
        sorted.sort_by(|a, b| compare(a, b, ext_host_file_system_info).partial_cmp(&0.0).unwrap_or(Ordering::Equal));
        sorted
    };
    arrays::delta(&sorted(old_folders), &sorted(new_folders), |a, b| compare(a, b, ext_host_file_system_info))
}

/// O resultado de [`ExtHostWorkspaceImpl::to_ext_host_workspace`].
pub struct ExtHostWorkspaceUpdate {
    pub workspace: Option<Rc<ExtHostWorkspaceImpl>>,
    pub added: Vec<Rc<WorkspaceFolder>>,
    pub removed: Vec<Rc<WorkspaceFolder>>,
}

/// `ExtHostWorkspaceImpl`.
pub struct ExtHostWorkspaceImpl {
    base: Rc<Workspace>,
    name: String,
    is_untitled: bool,
    workspace_folders: Vec<Rc<WorkspaceFolder>>,
    structure: TernarySearchTree<URI, Rc<WorkspaceFolder>>,
}

impl ExtHostWorkspaceImpl {
    /// `ExtHostWorkspaceImpl.toExtHostWorkspace`.
    pub fn to_ext_host_workspace(
        data: Option<&IWorkspaceData>,
        previous_confirmed_workspace: Option<&Rc<ExtHostWorkspaceImpl>>,
        previous_unconfirmed_workspace: Option<&Rc<ExtHostWorkspaceImpl>>,
        ext_host_file_system_info: &FileSystemInfo,
    ) -> ExtHostWorkspaceUpdate {
        let Some(data) = data else {
            return ExtHostWorkspaceUpdate { workspace: None, added: Vec::new(), removed: Vec::new() };
        };

        let mut new_workspace_folders: Vec<Rc<WorkspaceFolder>> = Vec::new();

        // If we have an existing workspace, we try to find the folders that match our
        // data and update their properties. It could be that an extension stored them
        // for later use and we want to keep them "live" if they are still present.
        let old_workspace = previous_confirmed_workspace;
        if let Some(previous_confirmed_workspace) = previous_confirmed_workspace {
            for (index, folder_data) in data.folders.iter().enumerate() {
                let existing_folder = Self::find_folder(
                    previous_unconfirmed_workspace.unwrap_or(previous_confirmed_workspace),
                    &folder_data.uri,
                    ext_host_file_system_info,
                );

                match existing_folder {
                    Some(existing_folder) => {
                        *existing_folder.name.borrow_mut() = folder_data.name.clone();
                        existing_folder.index.set(folder_data.index);

                        new_workspace_folders.push(existing_folder);
                    }
                    None => new_workspace_folders.push(WorkspaceFolder::new(folder_data.uri.clone(), folder_data.name.clone(), index)),
                }
            }
        } else {
            new_workspace_folders.extend(data.folders.iter().map(|folder| WorkspaceFolder::new(folder.uri.clone(), folder.name.clone(), folder.index)));
        }

        // make sure to restore sort order based on index
        new_workspace_folders.sort_by_key(|folder| folder.index());

        let workspace = ExtHostWorkspaceImpl::new(
            &data.id,
            &data.name,
            new_workspace_folders,
            data.transient.unwrap_or(false),
            data.configuration.clone(),
            data.is_untitled.unwrap_or(false),
            ignore_path_casing_predicate(ext_host_file_system_info),
        );
        let old_folders = old_workspace.map(|workspace| workspace.workspace_folders()).unwrap_or_default();
        let Delta { removed, added } = delta(&old_folders, &workspace.workspace_folders(), compare_workspace_folder_by_uri, ext_host_file_system_info);

        ExtHostWorkspaceUpdate { workspace: Some(workspace), added, removed }
    }

    fn find_folder(workspace: &ExtHostWorkspaceImpl, folder_uri_to_find: &URI, ext_host_file_system_info: &FileSystemInfo) -> Option<Rc<WorkspaceFolder>> {
        workspace.workspace_folders.iter().find(|folder| is_folder_equal(folder.uri(), folder_uri_to_find, ext_host_file_system_info)).cloned()
    }

    pub fn new(
        id: &str,
        name: &str,
        folders: Vec<Rc<WorkspaceFolder>>,
        transient: bool,
        configuration: Option<URI>,
        is_untitled: bool,
        ignore_path_casing: UriPredicate,
    ) -> Rc<ExtHostWorkspaceImpl> {
        let base = Workspace::new(id, folders.iter().map(|folder| folder.to_platform()).collect(), transient, configuration, ignore_path_casing.clone(), None);
        let mut structure = TernarySearchTree::for_uris(ignore_path_casing, Rc::new(|_: &URI| true));

        // setup the workspace folder data structure
        for folder in &folders {
            structure.set(folder.uri().clone(), folder.clone());
        }

        Rc::new(ExtHostWorkspaceImpl { base, name: name.to_owned(), is_untitled, workspace_folders: folders, structure })
    }

    /// O `Workspace` base (as pastas como a plataforma as vê), que o `ExtHostConfigProvider` lê.
    pub fn workspace(&self) -> &Rc<Workspace> {
        &self.base
    }

    pub fn id(&self) -> String {
        self.base.id()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn configuration(&self) -> Option<URI> {
        self.base.configuration()
    }

    pub fn transient(&self) -> bool {
        self.base.transient()
    }

    /// `folders` do `Workspace` base: cópias das pastas, na plataforma.
    pub fn folders(&self) -> Vec<PlatformWorkspaceFolder> {
        self.base.folders()
    }

    pub fn is_untitled(&self) -> bool {
        self.is_untitled
    }

    pub fn workspace_folders(&self) -> Vec<Rc<WorkspaceFolder>> {
        self.workspace_folders.clone()
    }

    pub fn get_workspace_folder(&self, uri: &URI, resolve_parent: Option<bool>) -> Option<Rc<WorkspaceFolder>> {
        let mut uri = uri.clone();
        if resolve_parent.unwrap_or(false) && self.structure.get(&uri).is_some() {
            // `uri` is a workspace folder so we check for its parent
            uri = ext_uri().dirname(&uri);
        }
        self.structure.find_substr(&uri).cloned()
    }

    pub fn resolve_workspace_folder(&self, uri: &URI) -> Option<Rc<WorkspaceFolder>> {
        self.structure.get(uri).cloned()
    }
}

// ---- a busca ---------------------------------------------------------------------------------------------

/// `QueryOptions<T>`.
#[derive(Clone, Debug)]
pub struct QueryOptions<T> {
    pub options: T,
    pub folder: Option<URI>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FindFilesApiKind {
    FindFiles,
    FindFiles2,
}

impl FindFilesApiKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::FindFiles => "findFiles",
            Self::FindFiles2 => "findFiles2",
        }
    }
}

/// `FindFilesCallIntent`.
struct FindFilesCallIntent {
    /// O que a extensão passou para `useIgnoreFiles.local` (`findFiles2`); `None` se não passou ou se não se aplica.
    use_ignore_files_local: Option<bool>,
    /// Se a extensão passou `null` como `exclude` de `findFiles`. Sempre `false` em `findFiles2`.
    exclude_was_null: bool,
}

/// `{ type: 'include', value } | { type: 'filePatterns', value }`.
enum FindFilesQuery {
    Include(Option<GlobPattern>),
    FilePatterns(Vec<GlobPattern>),
}

/// As medidas que `_findFilesImpl` reporta na telemetria.
struct FindFilesStats {
    query_count: usize,
    respected_ignore_files: bool,
    result_count: usize,
    cancelled: bool,
    errored: bool,
}

/// `parseSearchExcludeInclude`'s result.
struct ParsedInclude {
    pattern: String,
    folder: Option<URI>,
}

fn parse_search_exclude_include(include: Option<&GlobPatternDto>) -> Option<ParsedInclude> {
    match include? {
        GlobPatternDto::Pattern(pattern) if pattern.is_empty() => None,
        GlobPatternDto::Pattern(pattern) => Some(ParsedInclude { pattern: pattern.clone(), folder: None }),
        GlobPatternDto::Relative(relative) => Some(ParsedInclude { pattern: relative.pattern.clone(), folder: Some(relative.base_uri.clone()) }),
    }
}

fn globs_to_search_pattern_builder(excludes: Option<&[GlobPattern]>) -> Vec<SearchPatternBuilder> {
    excludes
        .unwrap_or_default()
        .iter()
        .filter_map(|exclude| match exclude {
            GlobPattern::Pattern(pattern) => {
                if pattern.is_empty() {
                    return None;
                }
                Some(SearchPatternBuilder { pattern: SearchPathPatternBuilder::Single(pattern.clone()), uri: None })
            }
            GlobPattern::Relative(_) => {
                let parsed_exclude = parse_search_exclude_include(Some(&glob_pattern_from(exclude)))?;
                Some(SearchPatternBuilder { pattern: SearchPathPatternBuilder::Single(parsed_exclude.pattern), uri: parsed_exclude.folder })
            }
        })
        .collect()
}

fn to_search_ext_range(range: &crate::services::search::search::SearchRange) -> SearchExtRange {
    SearchExtRange::new(range.start_line_number, range.start_column, range.end_line_number, range.end_column)
}

fn to_ext_range(range: &crate::services::search::search::SearchRange) -> ExtRange {
    let to_position = |line: i64, character: i64| ExtPosition::new(line as f64, character as f64).unwrap_or_else(|error| errors::throw(error));
    ExtRange::new(to_position(range.start_line_number, range.start_column), to_position(range.end_line_number, range.end_column))
}

/// `toEncodeDecodeParameters`: o `uri` e o `{ encoding }` que vão ao proxy.
fn to_encode_decode_parameters(opts: Option<EncodeDecodeArgs>) -> (Option<URI>, Option<EncodingOptionDto>) {
    let Some(opts) = opts else {
        return (None, None);
    };
    (opts.uri, opts.encoding.filter(|encoding| !encoding.is_empty()).map(|encoding| EncodingOptionDto { encoding }))
}

type SearchCallback = Rc<dyn Fn(&ITextSearchResult, &URI)>;
type WillCreateListener = Listener<WaitUntilEvent<EditSessionIdentityWillCreateEventData>>;

fn dynamic_bool(value: Dynamic, default_value: bool) -> bool {
    if value.is_nullish() { default_value } else { value.is_truthy() }
}

/// `ExtHostWorkspace`.
pub struct ExtHostWorkspace {
    this: Weak<ExtHostWorkspace>,
    on_did_change_workspace: Rc<Emitter<WorkspaceFoldersChangeEvent>>,
    on_did_grant_workspace_trust: Rc<Emitter<()>>,
    on_did_change_workspace_trusted_folders: Rc<Emitter<()>>,
    log_service: LogServiceRef,
    request_id_provider: RefCell<Counter>,
    barrier: Barrier,
    confirmed_workspace: RefCell<Option<Rc<ExtHostWorkspaceImpl>>>,
    unconfirmed_workspace: RefCell<Option<Rc<ExtHostWorkspaceImpl>>>,
    proxy: Rc<dyn MainThreadWorkspaceShape>,
    message_service: Rc<dyn MainThreadMessageServiceShape>,
    telemetry_proxy: Rc<dyn MainThreadTelemetryShape>,
    ext_host_file_system_info: FileSystemInfo,
    uri_transformer_service: Rc<dyn IURITransformerService>,
    active_search_callbacks: RefCell<HashMap<u64, Rc<dyn Fn(&IRawFileMatch2)>>>,
    trusted: Cell<bool>,
    edit_session_identity_providers: RefCell<HashMap<String, Rc<dyn EditSessionIdentityProvider>>>,
    /// Pushed in by ExtHostConfiguration after init (see `$setConfigProvider`).
    config_provider: RefCell<Weak<ExtHostConfigProvider>>,
    provider_handle_pool: Cell<u64>,
    on_will_create_edit_session_identity_event: Rc<AsyncEmitter<EditSessionIdentityWillCreateEventData>>,
    will_create_listeners: RefCell<Vec<(WillCreateListener, Rc<ExtensionDescription>)>>,
    canonical_uri_providers: RefCell<HashMap<String, Rc<dyn CanonicalUriProvider>>>,
}

impl ExtHostWorkspace {
    pub fn new(
        ext_host_rpc: &dyn IMainContext,
        init_data: &dyn IExtHostInitDataService,
        ext_host_file_system_info: Rc<dyn IExtHostFileSystemInfo>,
        log_service: LogServiceRef,
        uri_transformer_service: Rc<dyn IURITransformerService>,
    ) -> Rc<ExtHostWorkspace> {
        let data = init_data.workspace();
        let confirmed_workspace = data.as_ref().map(|data| {
            ExtHostWorkspaceImpl::new(
                &data.id,
                &data.name,
                Vec::new(),
                data.transient.unwrap_or(false),
                data.configuration.clone(),
                data.is_untitled.unwrap_or(false),
                ignore_path_casing_predicate(&ext_host_file_system_info),
            )
        });

        Rc::new_cyclic(|this| ExtHostWorkspace {
            this: this.clone(),
            on_did_change_workspace: Emitter::new(),
            on_did_grant_workspace_trust: Emitter::new(),
            on_did_change_workspace_trusted_folders: Emitter::new(),
            log_service,
            request_id_provider: RefCell::new(Counter::new()),
            barrier: Barrier::new(),
            confirmed_workspace: RefCell::new(confirmed_workspace),
            unconfirmed_workspace: RefCell::new(None),
            proxy: ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_WORKSPACE),
            message_service: ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_MESSAGE_SERVICE),
            telemetry_proxy: ext_host_rpc.get_proxy(&MainContext::MAIN_THREAD_TELEMETRY),
            ext_host_file_system_info,
            uri_transformer_service,
            active_search_callbacks: RefCell::new(HashMap::new()),
            trusted: Cell::new(false),
            edit_session_identity_providers: RefCell::new(HashMap::new()),
            config_provider: RefCell::new(Weak::new()),
            provider_handle_pool: Cell::new(0),
            on_will_create_edit_session_identity_event: AsyncEmitter::new(),
            will_create_listeners: RefCell::new(Vec::new()),
            canonical_uri_providers: RefCell::new(HashMap::new()),
        })
    }

    /// O `Rc` deste host, para as operações assíncronas que precisam dele depois do retorno.
    fn rc(&self) -> Rc<ExtHostWorkspace> {
        self.this.upgrade().expect("o ExtHostWorkspace está vivo enquanto um método dele roda")
    }

    /// `onDidChangeWorkspace`.
    pub fn on_did_change_workspace(&self) -> Event<WorkspaceFoldersChangeEvent> {
        self.on_did_change_workspace.event()
    }

    /// `onDidGrantWorkspaceTrust`.
    pub fn on_did_grant_workspace_trust(&self) -> Event<()> {
        self.on_did_grant_workspace_trust.event()
    }

    /// `onDidChangeWorkspaceTrustedFolders`.
    pub fn on_did_change_workspace_trusted_folders(&self) -> Event<()> {
        self.on_did_change_workspace_trusted_folders.event()
    }

    /// Receives the configuration provider from ExtHostConfiguration after init. We cannot inject
    /// IExtHostConfiguration directly because it creates a DI cycle (ExtHostConfiguration already
    /// depends on IExtHostWorkspace). Once set, settings reads in findFiles become synchronous.
    pub fn set_config_provider(&self, provider: &Rc<ExtHostConfigProvider>) {
        *self.config_provider.borrow_mut() = Rc::downgrade(provider);
    }

    fn search_setting(&self, key: &str, default_value: bool) -> bool {
        let provider = self.config_provider.borrow().upgrade();
        match provider {
            Some(provider) => dynamic_bool(provider.get_configuration(Some("search"), None, None).get(key, None), default_value),
            None => default_value,
        }
    }

    fn use_ignore_files_in_find_files(&self) -> bool {
        self.search_setting("experimental.useIgnoreFilesInFindFiles", false)
    }

    fn user_ignore_files_setting(&self) -> bool {
        // Default in `search.useIgnoreFiles` is `true`; mirror that here so telemetry computed against
        // an unset config still reflects the fallback the query builder will apply.
        self.search_setting("useIgnoreFiles", true)
    }

    pub fn wait_for_initialize_call(&self) -> Promise<bool> {
        self.barrier.wait()
    }

    // --- workspace ---

    /// `_actualWorkspace`.
    fn actual_workspace(&self) -> Option<Rc<ExtHostWorkspaceImpl>> {
        let unconfirmed = self.unconfirmed_workspace.borrow().clone();
        unconfirmed.or_else(|| self.confirmed_workspace.borrow().clone())
    }

    /// `workspace`.
    pub fn workspace(&self) -> Option<Rc<ExtHostWorkspaceImpl>> {
        self.actual_workspace()
    }

    pub fn name(&self) -> Option<String> {
        self.actual_workspace().map(|workspace| workspace.name().to_owned())
    }

    pub fn workspace_file(&self) -> Option<URI> {
        let workspace = self.actual_workspace()?;
        let configuration = workspace.configuration()?;
        if workspace.is_untitled() {
            // Untitled Workspace: return untitled URI
            let ext_uri = ext_uri();
            return Some(URI::from(
                &UriComponents {
                    scheme: Some(JsString::from(schemas::UNTITLED)),
                    path: Some(JsString::from(ext_uri.basename(&ext_uri.dirname(&configuration), None))),
                    ..UriComponents::default()
                },
                false,
            ));
        }

        // Workspace: return the configuration location
        Some(configuration)
    }

    pub fn get_workspace_folders(&self) -> Option<Vec<Rc<WorkspaceFolder>>> {
        self.actual_workspace().map(|workspace| workspace.workspace_folders())
    }

    /// `updateWorkspaceFolders(extension, index, deleteCount, ...workspaceFoldersToAdd)`.
    pub fn update_workspace_folders(
        &self,
        extension: &ExtensionDescription,
        index: Option<f64>,
        delete_count: Option<f64>,
        workspace_folders_to_add: Vec<WorkspaceFolderToAdd>,
    ) -> bool {
        let info = &self.ext_host_file_system_info;
        let mut validated_distinct_workspace_folders_to_add: Vec<WorkspaceFolderToAdd> = Vec::new();
        for folder_to_add in &workspace_folders_to_add {
            if !validated_distinct_workspace_folders_to_add.iter().any(|folder| is_folder_equal(&folder.uri, &folder_to_add.uri, info)) {
                let name = folder_to_add.name.clone().filter(|name| !name.is_empty()).unwrap_or_else(|| ext_uri().basename_or_authority(&folder_to_add.uri));
                validated_distinct_workspace_folders_to_add.push(WorkspaceFolderToAdd { uri: folder_to_add.uri.clone(), name: Some(name) });
            }
        }

        if self.unconfirmed_workspace.borrow().is_some() {
            return false; // prevent accumulated calls without a confirmed workspace
        }

        let (Some(index), Some(delete_count)) = (index, delete_count) else {
            return false; // validate numbers
        };
        if index < 0.0 || delete_count < 0.0 {
            return false; // validate numbers
        }

        if delete_count == 0.0 && validated_distinct_workspace_folders_to_add.is_empty() {
            return false; // nothing to delete or add
        }

        let current_workspace_folders = self.actual_workspace().map(|workspace| workspace.workspace_folders()).unwrap_or_default();
        if index + delete_count > current_workspace_folders.len() as f64 {
            return false; // cannot delete more than we have
        }

        // Simulate the updateWorkspaceFolders method on our data to do more validation
        let mut new_workspace_folders = current_workspace_folders.clone();
        let (start, end) = (index as usize, (index + delete_count) as usize);
        new_workspace_folders.splice(
            start..end,
            validated_distinct_workspace_folders_to_add
                .iter()
                .map(|folder| WorkspaceFolder::new(folder.uri.clone(), folder.name.clone().unwrap_or_default(), 0 /* fixed later */)),
        );

        for (i, folder) in new_workspace_folders.iter().enumerate() {
            if new_workspace_folders.iter().enumerate().any(|(other_index, other_folder)| other_index != i && is_folder_equal(folder.uri(), other_folder.uri(), info)) {
                return false; // cannot add the same folder multiple times
            }
        }

        for (index, folder) in new_workspace_folders.iter().enumerate() {
            folder.index.set(index); // fix index
        }
        let Delta { removed, added } =
            delta(&current_workspace_folders, &new_workspace_folders, compare_workspace_folder_by_uri_and_name_and_index, info);
        if added.is_empty() && removed.is_empty() {
            return false; // nothing actually changed
        }

        // Trigger on main side
        let ext_name = extension.manifest.display_name.clone().filter(|name| !name.is_empty()).unwrap_or_else(|| extension.manifest.name.clone());
        let this = self.rc();
        let (error_extension_name, identifier) = (ext_name.clone(), extension.identifier.clone());
        let update = self.proxy.rpc_update_workspace_folders(&ext_name, index, delete_count, validated_distinct_workspace_folders_to_add);
        drop(update.then_result(move |result| {
            if let Err(error) = result {
                // in case of an error, make sure to clear out the unconfirmed workspace
                // because we cannot expect the acknowledgement from the main side for this
                *this.unconfirmed_workspace.borrow_mut() = None;

                // show error to user
                let options = MainThreadMessageOptions {
                    source: Some(MainThreadMessageSource { identifier, label: error_extension_name.clone() }),
                    ..MainThreadMessageOptions::default()
                };
                drop(this.message_service.rpc_show_message(
                    Severity::Error,
                    &localize(
                        "updateerror",
                        "Extension '{0}' failed to update workspace folders: {1}",
                        &[error_extension_name.as_str(), error.to_string().as_str()],
                    ),
                    options,
                    Vec::new(),
                ));
            }
            Ok(())
        }));

        // Try to accept directly
        self.try_set_workspace_folders(&new_workspace_folders);

        true
    }

    pub fn get_workspace_folder(&self, uri: &URI, resolve_parent: Option<bool>) -> Option<Rc<WorkspaceFolder>> {
        self.actual_workspace()?.get_workspace_folder(uri, resolve_parent)
    }

    pub fn get_path(&self) -> Option<String> {
        // this is legacy from the days before having
        // multi-root and we keep it only alive if there
        // is just one workspace folder.
        let workspace = self.actual_workspace()?;
        let folders = workspace.folders();
        // #54483 @Joh Why are we still using fsPath?
        folders.first().map(|folder| fs_path(&folder.uri))
    }

    /// `getRelativePath(pathOrUri, includeWorkspace?)`.
    pub fn get_relative_path(&self, path_or_uri: RelativePathInput, include_workspace: Option<bool>) -> String {
        let (resource, path) = match path_or_uri {
            RelativePathInput::Path(path) => (Some(uri_file(&path)), path),
            RelativePathInput::Uri(uri) => {
                let path = fs_path(&uri);
                (Some(uri), path)
            }
            RelativePathInput::Undefined => (None, String::new()),
        };

        let Some(resource) = resource else {
            return path;
        };

        let Some(folder) = self.get_workspace_folder(&resource, Some(true)) else {
            return path;
        };

        let include_workspace = include_workspace.or_else(|| self.actual_workspace().map(|workspace| workspace.folders().len() > 1));

        let mut result = ext_uri().relative_path(folder.uri(), &resource).unwrap_or_default();
        let name = folder.name();
        if include_workspace.unwrap_or(false) && !name.is_empty() {
            result = format!("{name}/{result}");
        }
        result
    }

    fn try_set_workspace_folders(&self, folders: &[Rc<WorkspaceFolder>]) {
        // Update directly here. The workspace is unconfirmed as long as we did not get an
        // acknowledgement from the main side (via $acceptWorkspaceData)
        let Some(actual_workspace) = self.actual_workspace() else {
            return;
        };
        let data = IWorkspaceData {
            id: actual_workspace.id(),
            name: actual_workspace.name().to_owned(),
            transient: None,
            configuration: actual_workspace.configuration(),
            is_untitled: Some(actual_workspace.is_untitled()),
            folders: folders.iter().map(|folder| WorkspaceFolderDataDto { uri: folder.uri().clone(), name: folder.name(), index: folder.index() }).collect(),
        };
        let update = ExtHostWorkspaceImpl::to_ext_host_workspace(Some(&data), Some(&actual_workspace), None, &self.ext_host_file_system_info);
        *self.unconfirmed_workspace.borrow_mut() = update.workspace;
    }

    // --- search ---

    /// Note, null/undefined have different and important meanings for "exclude".
    pub fn find_files(
        &self,
        include: Option<GlobPattern>,
        exclude: FindFilesExclude,
        max_results: Option<i64>,
        extension_id: &ExtensionIdentifier,
        token: CancellationToken,
    ) -> Promise<Vec<URI>> {
        self.log_service.trace(&format!("extHostWorkspace#findFiles: fileSearch, extension: {}, entryPoint: findFiles", extension_id.value));

        let mut exclude_string = String::new();
        let mut use_file_excludes = true;
        match &exclude {
            FindFilesExclude::Null => use_file_excludes = false,
            FindFilesExclude::Undefined => {}
            FindFilesExclude::Pattern(GlobPattern::Pattern(pattern)) => exclude_string = pattern.clone(),
            FindFilesExclude::Pattern(GlobPattern::Relative(pattern)) => exclude_string = pattern.pattern.clone(),
        }
        let exclude_was_null = exclude == FindFilesExclude::Null;

        let use_ignore_files_opt_in = self.use_ignore_files_in_find_files();
        // `useIgnoreFiles.local` semantics: `false` means "do not respect local .gitignore" (--no-ignore to rg).
        // Default (PR #204845): hardcoded `false` for every legacy findFiles caller, regardless of `search.useIgnoreFiles`.
        // Opt-in (`search.experimental.useIgnoreFilesInFindFiles: true`): honor the user's `search.useIgnoreFiles`,
        // while keeping `exclude === null` as the documented escape hatch (no excludes => bypass .gitignore).
        let local_ignore_files = if use_ignore_files_opt_in && !exclude_was_null { None } else { Some(false) };

        // todo: consider exclude baseURI if available
        self.find_files_impl(
            FindFilesQuery::Include(include),
            FindFiles2Options {
                exclude: Some(vec![GlobPattern::Pattern(exclude_string)]),
                max_results,
                use_exclude_settings: Some(if use_file_excludes { ExcludeSettingOptions::FilesExclude } else { ExcludeSettingOptions::None }),
                use_ignore_files: Some(UseIgnoreFilesOptions { local: local_ignore_files, ..UseIgnoreFilesOptions::default() }),
                ..FindFiles2Options::default()
            },
            extension_id,
            FindFilesApiKind::FindFiles,
            FindFilesCallIntent { use_ignore_files_local: None, exclude_was_null },
            token,
        )
    }

    pub fn find_files2(&self, file_patterns: Vec<GlobPattern>, options: FindFiles2Options, extension_id: &ExtensionIdentifier, token: CancellationToken) -> Promise<Vec<URI>> {
        self.log_service.trace(&format!("extHostWorkspace#findFiles2New: fileSearch, extension: {}, entryPoint: findFiles2New", extension_id.value));
        let use_ignore_files_local = options.use_ignore_files.and_then(|use_ignore_files| use_ignore_files.local);
        self.find_files_impl(
            FindFilesQuery::FilePatterns(file_patterns),
            options,
            extension_id,
            FindFilesApiKind::FindFiles2,
            FindFilesCallIntent { use_ignore_files_local, exclude_was_null: false },
            token,
        )
    }

    /// `_findFilesImpl`: o `include` de `findFiles` consulta por `includePattern`, e o `filePatterns` de `findFiles2` por
    /// `filePattern`, que é o jeito certo porque pesa menos que os arquivos de ignore.
    fn find_files_impl(
        &self,
        query: FindFilesQuery,
        options: FindFiles2Options,
        extension_id: &ExtensionIdentifier,
        api_kind: FindFilesApiKind,
        intent: FindFilesCallIntent,
        token: CancellationToken,
    ) -> Promise<Vec<URI>> {
        let this = self.rc();
        let extension_id = extension_id.value.clone();
        async_fn(async move {
            let use_ignore_files_local_requested = match intent.use_ignore_files_local {
                Some(true) => "true",
                Some(false) => "false",
                None => "unspecified",
            };
            let sw = StopWatch::new(Some(true));
            let mut stats = FindFilesStats {
                query_count: 0,
                respected_ignore_files: this.user_ignore_files_setting(),
                result_count: 0,
                cancelled: false,
                errored: false,
            };

            let outcome: Result<Vec<URI>, Error> = async {
                if token.is_cancellation_requested() {
                    stats.cancelled = true;
                    return Ok(Vec::new());
                }

                let file_patterns_to_use: Vec<Option<&GlobPattern>> = match &query {
                    FindFilesQuery::Include(include) => vec![include.as_ref()],
                    FindFilesQuery::FilePatterns(patterns) => patterns.iter().map(Some).collect(),
                };
                let is_include_query = matches!(query, FindFilesQuery::Include(_));

                let query_options: Vec<QueryOptions<IFileQueryBuilderOptions>> = file_patterns_to_use
                    .into_iter()
                    .map(|file_pattern| {
                        let exclude_patterns = globs_to_search_pattern_builder(options.exclude.as_deref());

                        let parse_include = parse_search_exclude_include(file_pattern.map(glob_pattern_from).as_ref());
                        let mut file_queries = IFileQueryBuilderOptions {
                            common: ICommonQueryBuilderOptions {
                                ignore_symlinks: options.follow_symlinks.map(|follow| !follow),
                                disregard_ignore_files: options.use_ignore_files.and_then(|use_ignore_files| use_ignore_files.local).map(|local| !local),
                                disregard_global_ignore_files: options.use_ignore_files.and_then(|use_ignore_files| use_ignore_files.global).map(|global| !global),
                                disregard_parent_ignore_files: options.use_ignore_files.and_then(|use_ignore_files| use_ignore_files.parent).map(|parent| !parent),
                                disregard_exclude_settings: Some(options.use_exclude_settings == Some(ExcludeSettingOptions::None)),
                                disregard_search_exclude_settings: Some(
                                    options.use_exclude_settings.is_some_and(|use_exclude_settings| use_exclude_settings != ExcludeSettingOptions::SearchAndFilesExclude),
                                ),
                                max_results: options.max_results,
                                exclude_pattern: (!exclude_patterns.is_empty()).then_some(exclude_patterns),
                                ignore_glob_case: options.case_insensitive,
                                reason: Some("startFileSearch".to_owned()),
                                ..ICommonQueryBuilderOptions::default()
                            },
                            should_glob_search: if is_include_query { None } else { Some(true) },
                            ..IFileQueryBuilderOptions::default()
                        };

                        let folder_to_use = parse_include.as_ref().and_then(|parsed| parsed.folder.clone());
                        let pattern = parse_include.map(|parsed| parsed.pattern);
                        if is_include_query {
                            file_queries.common.include_pattern = pattern.map(SearchPathPatternBuilder::Single);
                        } else {
                            file_queries.file_pattern = pattern;
                        }

                        QueryOptions { folder: folder_to_use, options: file_queries }
                    })
                    .collect();

                stats.query_count = query_options.len();
                // Effective ignore-file behavior across all sub-queries: a call respected `.gitignore` only when every
                // sub-query is either explicitly honoring it or falls back to a user setting that honors it. When
                // `disregardIgnoreFiles` is `undefined` the query builder uses `search.useIgnoreFiles`, which we mirror here.
                let user_honors_ignore = this.user_ignore_files_setting();
                stats.respected_ignore_files = query_options.iter().all(|query| match query.options.common.disregard_ignore_files {
                    Some(true) => false,
                    Some(false) => true,
                    None => user_honors_ignore,
                });

                let result = this.find_files_base(query_options, &token).await?;
                stats.result_count = result.len();
                stats.cancelled = token.is_cancellation_requested();
                Ok(result)
            }
            .await;

            if outcome.is_err() {
                stats.errored = true;
                stats.cancelled = token.is_cancellation_requested();
            }
            this.report_find_files_telemetry(
                &extension_id,
                api_kind,
                &stats,
                use_ignore_files_local_requested,
                intent.exclude_was_null,
                sw.elapsed(),
            );
            outcome
        })
    }

    fn find_files_base(&self, query_options: Vec<QueryOptions<IFileQueryBuilderOptions>>, token: &CancellationToken) -> Promise<Vec<URI>> {
        let searches: Vec<Promise<Vec<URI>>> = query_options
            .into_iter()
            .map(|option| self.proxy.rpc_start_file_search(option.folder, option.options, token.clone()).then(|data| data.unwrap_or_default()))
            .collect();
        let ext_host_file_system_info = self.ext_host_file_system_info.clone();

        Promise::all(searches).then(move |result| {
            let flat_result: Vec<URI> = result.into_iter().flatten().collect();

            // Dedupe entries in a flat array
            let ext_uri = ext_uri_of(&ext_host_file_system_info);
            let mut seen: HashSet<JsString> = HashSet::new();
            let mut deduped = Vec::new();

            for uri in flat_result {
                if seen.insert(ext_uri.get_comparison_key(&uri, false)) {
                    deduped.push(uri);
                }
            }

            deduped
        })
    }

    fn report_find_files_telemetry(
        &self,
        extension_id: &str,
        api_kind: FindFilesApiKind,
        stats: &FindFilesStats,
        use_ignore_files_local_requested: &str,
        exclude_was_null: bool,
        duration_ms: f64,
    ) {
        self.telemetry_proxy.rpc_public_log2(
            "extHostFindFiles",
            Some(Dynamic::object([
                ("extensionId", Dynamic::string(extension_id)),
                ("apiKind", Dynamic::string(api_kind.as_str())),
                ("respectedIgnoreFiles", Dynamic::Bool(stats.respected_ignore_files)),
                ("useIgnoreFilesLocalRequested", Dynamic::string(use_ignore_files_local_requested)),
                ("excludeWasNull", Dynamic::Bool(exclude_was_null)),
                ("resultCount", Dynamic::Number(stats.result_count as f64)),
                ("durationMs", Dynamic::Number(duration_ms)),
                ("queryCount", Dynamic::Number(stats.query_count as f64)),
                ("cancelled", Dynamic::Bool(stats.cancelled)),
                ("errored", Dynamic::Bool(stats.errored)),
            ])),
        );
    }

    pub fn find_text_in_files2(
        &self,
        query: TextSearchQuery2,
        options: Option<FindTextInFilesOptions2>,
        extension_id: &ExtensionIdentifier,
        token: CancellationToken,
    ) -> FindTextInFilesResponse {
        self.log_service.trace(&format!("extHostWorkspace#findTextInFiles2: textSearch, extension: {}, entryPoint: findTextInFiles2", extension_id.value));

        let get_options = |include: Option<&GlobPattern>| -> QueryOptions<ITextQueryBuilderOptions> {
            let Some(options) = &options else {
                return QueryOptions { folder: None, options: ITextQueryBuilderOptions::default() };
            };
            let parsed_include = include.and_then(|include| parse_search_exclude_include(Some(&glob_pattern_from(include))));

            let exclude_patterns = options.exclude.as_deref().map(|exclude| globs_to_search_pattern_builder(Some(exclude)));

            QueryOptions {
                options: ITextQueryBuilderOptions {
                    common: ICommonQueryBuilderOptions {
                        ignore_symlinks: options.follow_symlinks.map(|follow| !follow),
                        disregard_ignore_files: options.use_ignore_files.and_then(|use_ignore_files| use_ignore_files.local).map(|local| !local),
                        disregard_global_ignore_files: options.use_ignore_files.and_then(|use_ignore_files| use_ignore_files.global).map(|global| !global),
                        disregard_parent_ignore_files: options.use_ignore_files.and_then(|use_ignore_files| use_ignore_files.parent).map(|parent| !parent),
                        disregard_exclude_settings: Some(options.use_exclude_settings == Some(ExcludeSettingOptions::None)),
                        disregard_search_exclude_settings: Some(
                            options.use_exclude_settings.is_some_and(|use_exclude_settings| use_exclude_settings != ExcludeSettingOptions::SearchAndFilesExclude),
                        ),
                        max_results: options.max_results,
                        ignore_glob_case: options.case_insensitive,
                        include_pattern: parsed_include.as_ref().map(|parsed| SearchPathPatternBuilder::Single(parsed.pattern.clone())),
                        exclude_pattern: exclude_patterns,
                        ..ICommonQueryBuilderOptions::default()
                    },
                    file_encoding: options.encoding.clone(),
                    preview_options: options.preview_options.map(|preview_options| ITextSearchPreviewOptions {
                        match_lines: preview_options.num_match_lines.unwrap_or(100),
                        chars_per_line: preview_options.chars_per_line.unwrap_or(10000),
                    }),
                    surrounding_context: options.surrounding_context,
                    ..ITextQueryBuilderOptions::default()
                },
                folder: parsed_include.and_then(|parsed| parsed.folder),
            }
        };

        let query_options: Vec<QueryOptions<ITextQueryBuilderOptions>> = match options.as_ref().and_then(|options| options.include.as_ref()) {
            Some(include) => include.iter().map(|include| get_options(Some(include))).collect(),
            None => vec![get_options(None)],
        };

        let disposables = DisposableStore::new();
        let progress_emitter: Rc<Emitter<(ITextSearchResult, URI)>> = Emitter::new();
        let emitter = progress_emitter.clone();
        let progress: SearchCallback = Rc::new(move |result: &ITextSearchResult, uri: &URI| emitter.fire((result.clone(), uri.clone())));
        let pattern_info = PatternInfo {
            pattern: query.pattern,
            is_reg_exp: query.is_reg_exp,
            is_word_match: query.is_word_match,
            is_multiline: query.is_multiline,
            is_case_sensitive: query.is_case_sensitive,
            ..PatternInfo::default()
        };
        let complete = self.find_text_in_files_base(pattern_info, query_options, progress, token);

        let iteration = complete.clone();
        let subscriptions = disposables.clone();
        let results = AsyncIterableProducer::new(
            move |emitter| {
                subscriptions.add(progress_emitter.event().listen(move |(result, uri)| match result {
                    ITextSearchResult::Match(result) => emitter.emit_one(TextSearchResult2::Match(TextSearchMatch2::new(
                        uri.clone(),
                        result
                            .range_locations
                            .iter()
                            .map(|range| TextSearchMatchRanges { preview_range: to_search_ext_range(&range.preview), source_range: to_search_ext_range(&range.source) })
                            .collect(),
                        result.preview_text.clone(),
                    ))),
                    ITextSearchResult::Context(result) => {
                        emitter.emit_one(TextSearchResult2::Context(TextSearchContext2::new(uri.clone(), result.text.clone(), result.line_number)))
                    }
                }));
                MaybePromise::Promise(iteration.then_result(|result| result.map(|_| ())))
            },
            None,
        );

        FindTextInFilesResponse {
            results,
            complete: complete.then(move |complete| {
                disposables.dispose();
                TextSearchComplete2 { limit_hit: Some(complete.limit_hit.unwrap_or(false)), message: None }
            }),
        }
    }

    pub fn find_text_in_files_base(
        &self,
        query: PatternInfo,
        query_options: Vec<QueryOptions<ITextQueryBuilderOptions>>,
        callback: SearchCallback,
        token: CancellationToken,
    ) -> Promise<ITextSearchComplete> {
        let request_id = self.request_id_provider.borrow_mut().get_next();

        let is_canceled = Rc::new(Cell::new(false));
        let canceled = is_canceled.clone();
        drop(token.on_cancellation_requested().listen(move |_| canceled.set(true)));

        self.active_search_callbacks.borrow_mut().insert(
            request_id,
            Rc::new(move |p: &IRawFileMatch2| {
                if is_canceled.get() {
                    return;
                }

                let uri = p.resource.clone();
                for result in p.results.iter().flatten() {
                    callback(result, &uri);
                }
            }),
        );

        if token.is_cancellation_requested() {
            return Promise::resolved(ITextSearchComplete::default());
        }

        let searches: Vec<Promise<Option<ITextSearchComplete>>> = query_options
            .into_iter()
            .map(|option| self.proxy.rpc_start_text_search(query.clone(), option.folder, option.options, request_id, token.clone()))
            .collect();
        let this = self.rc();
        async_fn(async move {
            let result = Promise::all(searches).await;
            this.active_search_callbacks.borrow_mut().remove(&request_id);
            Ok(result?.into_iter().fold(ITextSearchComplete::default(), |acc, val| {
                let val = val.unwrap_or_default();
                ITextSearchComplete {
                    limit_hit: Some(acc.limit_hit.unwrap_or(false) || val.limit_hit.unwrap_or(false)),
                    message: Some(acc.message.into_iter().flatten().chain(val.message.into_iter().flatten()).collect()),
                }
            }))
        })
    }

    pub fn find_text_in_files(
        &self,
        query: PatternInfo,
        options: FindTextInFilesOptions,
        callback: Rc<dyn Fn(TextSearchResult)>,
        extension_id: &ExtensionIdentifier,
        token: CancellationToken,
    ) -> Promise<ITextSearchComplete> {
        self.log_service.trace(&format!("extHostWorkspace#findTextInFiles: textSearch, extension: {}, entryPoint: findTextInFiles", extension_id.value));

        let preview_options = options.preview_options.unwrap_or(ITextSearchPreviewOptions { match_lines: 100, chars_per_line: 10000 });

        let parsed_include = parse_search_exclude_include(options.include.as_ref().map(glob_pattern_from).as_ref());

        let exclude_pattern = match &options.exclude {
            Some(GlobPattern::Pattern(pattern)) => Some(pattern.clone()),
            Some(GlobPattern::Relative(pattern)) => Some(pattern.pattern.clone()),
            None => None,
        }
        .filter(|pattern| !pattern.is_empty());
        let query_options = ITextQueryBuilderOptions {
            common: ICommonQueryBuilderOptions {
                ignore_symlinks: options.follow_symlinks.map(|follow| !follow),
                disregard_ignore_files: options.use_ignore_files.map(|use_ignore_files| !use_ignore_files),
                disregard_global_ignore_files: options.use_global_ignore_files.map(|use_global_ignore_files| !use_global_ignore_files),
                disregard_parent_ignore_files: options.use_parent_ignore_files.map(|use_parent_ignore_files| !use_parent_ignore_files),
                disregard_exclude_settings: Some(options.use_default_excludes.map_or(true, |use_default_excludes| !use_default_excludes)),
                disregard_search_exclude_settings: Some(options.use_search_exclude.map_or(true, |use_search_exclude| !use_search_exclude)),
                max_results: options.max_results,
                include_pattern: parsed_include.as_ref().map(|parsed| SearchPathPatternBuilder::Single(parsed.pattern.clone())),
                exclude_pattern: exclude_pattern.map(|pattern| vec![SearchPatternBuilder { pattern: SearchPathPatternBuilder::Single(pattern), uri: None }]),
                ..ICommonQueryBuilderOptions::default()
            },
            file_encoding: options.encoding.clone(),
            preview_options: Some(preview_options),
            surrounding_context: options.after_context, // TODO: remove ability to have before/after context separately
            ..ITextQueryBuilderOptions::default()
        };

        let progress: SearchCallback = Rc::new(move |result: &ITextSearchResult, uri: &URI| match result {
            ITextSearchResult::Match(result) => {
                let preview_matches = arrays::map_array_or_not(OneOrMany::Many(result.range_locations.clone()), |location| to_ext_range(&location.preview));
                let ranges = arrays::map_array_or_not(OneOrMany::Many(result.range_locations.clone()), |location| to_ext_range(&location.source));
                callback(TextSearchResult::Match(TextSearchMatchResult {
                    uri: uri.clone(),
                    preview: TextSearchMatchPreview { text: result.preview_text.clone(), matches: preview_matches },
                    ranges,
                }));
            }
            ITextSearchResult::Context(result) => {
                callback(TextSearchResult::Context(TextSearchContextResult { uri: uri.clone(), text: result.text.clone(), line_number: result.line_number }));
            }
        });

        self.find_text_in_files_base(query, vec![QueryOptions { options: query_options, folder: parsed_include.and_then(|parsed| parsed.folder) }], progress, token)
    }

    pub fn save(&self, uri: &URI) -> Promise<Option<URI>> {
        self.proxy.rpc_save(uri, SaveOptionsDto { save_as: false })
    }

    pub fn save_as(&self, uri: &URI) -> Promise<Option<URI>> {
        self.proxy.rpc_save(uri, SaveOptionsDto { save_as: true })
    }

    pub fn save_all(&self, include_untitled: Option<bool>) -> Promise<bool> {
        self.proxy.rpc_save_all(include_untitled)
    }

    // --- trust ---

    pub fn trusted(&self) -> bool {
        self.trusted.get()
    }

    pub fn request_resource_trust(&self, options: ResourceTrustRequestOptions) -> Promise<Option<bool>> {
        self.proxy.rpc_request_resource_trust(options)
    }

    pub fn request_workspace_trust(&self, options: Option<WorkspaceTrustRequestOptions>) -> Promise<Option<bool>> {
        self.proxy.rpc_request_workspace_trust(options)
    }

    pub fn is_resource_trusted(&self, resource: &URI) -> Promise<bool> {
        self.proxy.rpc_is_resource_trusted(resource)
    }

    // --- edit sessions ---

    /// `nextHandle`: o `_providerHandlePool++`, compartilhado pelos provedores de sessão de edição e de URI canônico.
    fn next_provider_handle(&self) -> u64 {
        let handle = self.provider_handle_pool.get();
        self.provider_handle_pool.set(handle + 1);
        handle
    }

    // called by ext host
    pub fn register_edit_session_identity_provider(&self, scheme: &str, provider: Rc<dyn EditSessionIdentityProvider>) -> Result<DisposableRef, Error> {
        if self.edit_session_identity_providers.borrow().contains_key(scheme) {
            return Err(Error::new(format!("A provider has already been registered for scheme {scheme}")));
        }

        self.edit_session_identity_providers.borrow_mut().insert(scheme.to_owned(), provider);
        let outgoing_scheme = self.uri_transformer_service.transform_outgoing_scheme(scheme);
        let handle = self.next_provider_handle();
        self.proxy.rpc_register_edit_session_identity_provider(handle, &outgoing_scheme);

        let this = self.rc();
        let scheme = scheme.to_owned();
        Ok(to_disposable(move || {
            this.edit_session_identity_providers.borrow_mut().remove(&scheme);
            this.proxy.rpc_unregister_edit_session_identity_provider(handle);
        }))
    }

    pub fn get_on_will_create_edit_session_identity_event(&self, extension: &ExtensionDescription) -> Event<WaitUntilEvent<EditSessionIdentityWillCreateEventData>> {
        let this = self.this.clone();
        let extension = Rc::new(extension.clone());
        Event::new(move |listener: WillCreateListener| {
            let Some(this) = this.upgrade() else {
                return to_disposable(|| {});
            };
            let wrapped: WillCreateListener = Rc::new(move |e: &WaitUntilEvent<EditSessionIdentityWillCreateEventData>| listener(e));
            this.will_create_listeners.borrow_mut().push((wrapped.clone(), extension.clone()));
            let subscription = this.on_will_create_edit_session_identity_event.event().listen_rc(wrapped.clone());
            to_disposable(move || {
                subscription.dispose();
                this.will_create_listeners.borrow_mut().retain(|(candidate, _)| !Rc::ptr_eq(candidate, &wrapped));
            })
        })
    }

    // --- canonical uri identity ---

    // called by ext host
    pub fn register_canonical_uri_provider(&self, scheme: &str, provider: Rc<dyn CanonicalUriProvider>) -> Result<DisposableRef, Error> {
        if self.canonical_uri_providers.borrow().contains_key(scheme) {
            return Err(Error::new(format!("A provider has already been registered for scheme {scheme}")));
        }

        self.canonical_uri_providers.borrow_mut().insert(scheme.to_owned(), provider);
        let outgoing_scheme = self.uri_transformer_service.transform_outgoing_scheme(scheme);
        let handle = self.next_provider_handle();
        self.proxy.rpc_register_canonical_uri_provider(handle, &outgoing_scheme);

        let this = self.rc();
        let scheme = scheme.to_owned();
        Ok(to_disposable(move || {
            this.canonical_uri_providers.borrow_mut().remove(&scheme);
            this.proxy.rpc_unregister_canonical_uri_provider(handle);
        }))
    }

    pub fn provide_canonical_uri(&self, uri: &URI, options: &CanonicalUriRequestOptions, cancellation_token: &CancellationToken) -> Promise<Option<URI>> {
        let provider = self.canonical_uri_providers.borrow().get(&uri.scheme().to_string_lossy()).cloned();
        let Some(provider) = provider else {
            return Promise::resolved(None);
        };

        match provider.provide_canonical_uri(uri, options, cancellation_token) {
            Some(result) => result,
            None => Promise::resolved(None),
        }
    }

    // --- encodings ---

    pub fn decode(&self, content: &[u8], args: Option<EncodeDecodeArgs>) -> Promise<String> {
        let this = self.rc();
        let content = content.to_vec();
        async_fn(async move {
            let (uri, opts) = to_encode_decode_parameters(args);
            let options = this.proxy.rpc_resolve_decoding(uri.clone(), opts.clone()).await?;

            let preferred_encoding = options.preferred_encoding.clone();
            let validating = this.clone();
            let overwrite_encoding: Rc<dyn Fn(Option<String>) -> Promise<String>> = Rc::new(move |detected_encoding: Option<String>| match detected_encoding {
                // Prevent another roundtrip to the main thread
                // if the detected encoding is null or the same
                // as the preferred encoding
                None => Promise::resolved(preferred_encoding.clone()),
                Some(detected_encoding) if detected_encoding == preferred_encoding => Promise::resolved(preferred_encoding.clone()),
                Some(detected_encoding) => validating.proxy.rpc_validate_detected_encoding(uri.clone(), &detected_encoding, opts.clone()),
            });

            let source: VSBufferReadableStream = buffer_to_stream(VSBuffer::wrap(content));
            let decoded = to_decode_stream(
                source,
                DecodeStreamOptions {
                    accept_text_only: true,
                    guess_encoding: options.guess_encoding,
                    candidate_guess_encodings: options.candidate_guess_encodings,
                    min_bytes_required_for_detection: None,
                    overwrite_encoding,
                },
            )
            .await?;

            consume_stream(&*decoded.stream, Rc::new(|chunks: Vec<String>| chunks.concat())).await
        })
    }

    pub fn encode(&self, content: &str, args: Option<EncodeDecodeArgs>) -> Promise<Vec<u8>> {
        let this = self.rc();
        let content = content.to_owned();
        async_fn(async move {
            let (uri, options) = to_encode_decode_parameters(args);
            let resolved = this.proxy.rpc_resolve_encoding(uri, options).await?;

            // when encoding is standard skip encoding step
            if resolved.encoding == UTF8 && !resolved.add_bom {
                return Ok(VSBuffer::from_string(&content).to_vec());
            }

            // otherwise create encoded readable
            let mut pending = Some(content);
            let mut encoded = to_encode_readable(move || pending.take(), &resolved.encoding, Some(EncodeOptions { add_bom: resolved.add_bom }))?;
            Ok(readable_to_buffer(&mut encoded).to_vec())
        })
    }
}

impl IExtHostWorkspaceProvider for ExtHostWorkspace {
    fn get_workspace_folder2(&self, uri: &URI, resolve_parent: Option<bool>) -> Promise<Option<Rc<WorkspaceFolder>>> {
        let (this, uri) = (self.rc(), uri.clone());
        async_fn(async move {
            this.barrier.wait().await?;
            Ok(this.actual_workspace().and_then(|workspace| workspace.get_workspace_folder(&uri, resolve_parent)))
        })
    }

    fn resolve_workspace_folder(&self, uri: &URI) -> Promise<Option<Rc<WorkspaceFolder>>> {
        let (this, uri) = (self.rc(), uri.clone());
        async_fn(async move {
            this.barrier.wait().await?;
            Ok(this.actual_workspace().and_then(|workspace| workspace.resolve_workspace_folder(&uri)))
        })
    }

    fn get_workspace_folders2(&self) -> Promise<Option<Vec<Rc<WorkspaceFolder>>>> {
        let this = self.rc();
        async_fn(async move {
            this.barrier.wait().await?;
            Ok(this.actual_workspace().map(|workspace| workspace.workspace_folders()))
        })
    }

    fn resolve_proxy(&self, url: &str) -> Promise<Option<String>> {
        self.proxy.rpc_resolve_proxy(url)
    }

    fn lookup_authorization(&self, auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        self.proxy.rpc_lookup_authorization(auth_info)
    }

    fn lookup_kerberos_authorization(&self, url: &str) -> Promise<Option<String>> {
        self.proxy.rpc_lookup_kerberos_authorization(url)
    }

    fn load_certificates(&self) -> Promise<Vec<String>> {
        self.proxy.rpc_load_certificates()
    }
}

impl ExtHostWorkspaceShape for ExtHostWorkspace {
    fn rpc_initialize_workspace(&self, data: Option<IWorkspaceData>, trusted: bool) {
        self.trusted.set(trusted);
        self.rpc_accept_workspace_data(data);
        self.barrier.open();
    }

    fn rpc_accept_workspace_data(&self, data: Option<IWorkspaceData>) {
        let confirmed = self.confirmed_workspace.borrow().clone();
        let unconfirmed = self.unconfirmed_workspace.borrow().clone();
        let ExtHostWorkspaceUpdate { workspace, added, removed } =
            ExtHostWorkspaceImpl::to_ext_host_workspace(data.as_ref(), confirmed.as_ref(), unconfirmed.as_ref(), &self.ext_host_file_system_info);

        // Update our workspace object. We have a confirmed workspace, so we drop our
        // unconfirmed workspace.
        *self.confirmed_workspace.borrow_mut() = workspace;
        *self.unconfirmed_workspace.borrow_mut() = None;

        // Events
        self.on_did_change_workspace.fire(WorkspaceFoldersChangeEvent { added, removed });
    }

    fn rpc_handle_text_search_result(&self, result: IRawFileMatch2, request_id: u64) {
        let callback = self.active_search_callbacks.borrow().get(&request_id).cloned();
        if let Some(callback) = callback {
            callback(&result);
        }
    }

    fn rpc_on_did_grant_workspace_trust(&self) {
        if !self.trusted.get() {
            self.trusted.set(true);
            self.on_did_grant_workspace_trust.fire(());
        }
    }

    fn rpc_on_did_change_workspace_trusted_folders(&self) {
        self.on_did_change_workspace_trusted_folders.fire(());
    }

    fn rpc_get_edit_session_identifier(&self, workspace_folder: URI, cancellation_token: CancellationToken) -> Promise<Option<String>> {
        let this = self.rc();
        async_fn(async move {
            this.log_service.info_args("Getting edit session identifier for workspaceFolder", &[LogArg::from(workspace_folder.to_string(false).to_string_lossy().as_str())]);
            let Some(folder) = this.resolve_workspace_folder(&workspace_folder).await? else {
                this.log_service.warn("Unable to resolve workspace folder");
                return Ok(None);
            };

            this.log_service.info_args("Invoking #provideEditSessionIdentity for workspaceFolder", &[LogArg::from(folder.uri().to_string(false).to_string_lossy().as_str())]);

            let provider = this.edit_session_identity_providers.borrow().get(&folder.uri().scheme().to_string_lossy()).cloned();
            this.log_service.info_args(
                &format!("Provider for scheme {} is defined: ", folder.uri().scheme()),
                &[LogArg::Value(Dynamic::Bool(provider.is_some()))],
            );
            let Some(provider) = provider else {
                return Ok(None);
            };

            let result = provider.provide_edit_session_identity(&folder, &cancellation_token).await?;
            this.log_service.info_args(
                "Provider returned edit session identifier: ",
                &[LogArg::Value(result.as_deref().map_or(Dynamic::Undefined, Dynamic::string))],
            );
            Ok(result.filter(|result| !result.is_empty()))
        })
    }

    fn rpc_provide_edit_session_identity_match(
        &self,
        workspace_folder: URI,
        identity1: &str,
        identity2: &str,
        cancellation_token: CancellationToken,
    ) -> Promise<Option<EditSessionIdentityMatch>> {
        let this = self.rc();
        let (identity1, identity2) = (identity1.to_owned(), identity2.to_owned());
        async_fn(async move {
            this.log_service.info_args("Getting edit session identifier for workspaceFolder", &[LogArg::from(workspace_folder.to_string(false).to_string_lossy().as_str())]);
            let Some(folder) = this.resolve_workspace_folder(&workspace_folder).await? else {
                this.log_service.warn("Unable to resolve workspace folder");
                return Ok(None);
            };

            this.log_service.info_args("Invoking #provideEditSessionIdentity for workspaceFolder", &[LogArg::from(folder.uri().to_string(false).to_string_lossy().as_str())]);

            let provider = this.edit_session_identity_providers.borrow().get(&folder.uri().scheme().to_string_lossy()).cloned();
            this.log_service.info_args(
                &format!("Provider for scheme {} is defined: ", folder.uri().scheme()),
                &[LogArg::Value(Dynamic::Bool(provider.is_some()))],
            );
            let Some(provider) = provider else {
                return Ok(None);
            };

            let result = match provider.provide_edit_session_identity_match(&identity1, &identity2, &cancellation_token) {
                Some(result) => result.await?,
                None => None,
            };
            this.log_service.info_args(
                "Provider returned edit session identifier match result: ",
                &[LogArg::Value(result.map_or(Dynamic::Undefined, |result| Dynamic::from(result as i32)))],
            );
            // `EditSessionIdentityMatch.None` is `0`, which is falsy
            Ok(result.filter(|result| *result != EditSessionIdentityMatch::None))
        })
    }

    // main thread calls this to trigger participants
    fn rpc_on_will_create_edit_session_identity(&self, workspace_folder: URI, token: CancellationToken, timeout: f64) -> Promise<()> {
        let this = self.rc();
        async_fn(async move {
            let Some(folder) = this.resolve_workspace_folder(&workspace_folder).await? else {
                return Err(Error::new("Unable to resolve workspace folder"));
            };

            let weak = this.this.clone();
            let promise_join: PromiseJoin<EditSessionIdentityWillCreateEventData> = Rc::new(move |thenable: Promise<()>, listener: &WillCreateListener| {
                let (weak, listener) = (weak.clone(), listener.clone());
                async_fn(async move {
                    let now = Instant::now();
                    let result = thenable.await;
                    if now.elapsed().as_secs_f64() * 1000.0 > timeout
                        && let Some(this) = weak.upgrade()
                    {
                        let extension = this.will_create_listeners.borrow().iter().find(|(candidate, _)| Rc::ptr_eq(candidate, &listener)).map(|(_, extension)| extension.clone());
                        if let Some(extension) = extension {
                            this.log_service.warn_args(
                                &LogMessage::from("SLOW edit session create-participant"),
                                &[LogArg::from(extension.identifier.value.as_str())],
                            );
                        }
                    }
                    result
                })
            });
            this.on_will_create_edit_session_identity_event
                .fire_async(EditSessionIdentityWillCreateEventData { workspace_folder: folder }, token.clone(), Some(promise_join))
                .await?;

            // `if (token.isCancellationRequested) return undefined;` não muda o retorno `void`
            Ok(())
        })
    }

    // called by main thread
    fn rpc_provide_canonical_uri(&self, uri: URI, target_scheme: &str, cancellation_token: CancellationToken) -> Promise<Option<URI>> {
        self.provide_canonical_uri(&uri, &CanonicalUriRequestOptions { target_scheme: target_scheme.to_owned() }, &cancellation_token)
    }
}
