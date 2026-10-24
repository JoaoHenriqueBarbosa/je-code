//! Porte de `src/vs/platform/workspace/common/workspace.ts`: as pastas e o `Workspace` (que acha a pasta que
//! contém um recurso), os identificadores de workspace (de pasta única, multi-raiz e vazio), o
//! `IWorkspaceContextService` e as constantes e predicados do formato `.code-workspace`.
//!
//! Representação: `IWorkspaceIdentifier | ISingleFolderWorkspaceIdentifier | IEmptyWorkspaceIdentifier` é o enum
//! [`AnyWorkspaceIdentifier`] (os guardas `isWorkspaceIdentifier` e companhia são métodos dele); o `IWorkspace`
//! estrutural é o trait [`IWorkspace`], que o [`Workspace`] e o [`WorkspaceSnapshot`] implementam.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::ternary_search_tree::{TernarySearchTree, UriPredicate};
use je_base::uri::URI;

/// `IRawFileWorkspaceFolder | IRawUriWorkspaceFolder`: os metadados originais de uma pasta.
#[derive(Clone, Debug, PartialEq)]
pub enum RawWorkspaceFolder {
    Path { path: String, name: Option<String> },
    Uri { uri: String, name: Option<String> },
}

/// `WorkspaceFolder` (e `IWorkspaceFolderData`).
#[derive(Clone, Debug)]
pub struct WorkspaceFolder {
    pub uri: URI,
    pub name: String,
    pub index: usize,
    pub raw: Option<RawWorkspaceFolder>,
}

impl WorkspaceFolder {
    pub fn new(uri: URI, name: impl Into<String>, index: usize, raw: Option<RawWorkspaceFolder>) -> Self {
        Self { uri, name: name.into(), index, raw }
    }
}

/// `Workspace`.
pub struct Workspace {
    id: RefCell<String>,
    transient: bool,
    configuration: RefCell<Option<URI>>,
    name: Option<String>,
    ignore_path_casing: UriPredicate,
    folders: RefCell<Vec<WorkspaceFolder>>,
    folders_map: RefCell<TernarySearchTree<URI, WorkspaceFolder>>,
}

impl Workspace {
    pub fn new(
        id: impl Into<String>,
        folders: Vec<WorkspaceFolder>,
        transient: bool,
        configuration: Option<URI>,
        ignore_path_casing: UriPredicate,
        name: Option<String>,
    ) -> Rc<Self> {
        let workspace = Rc::new(Self {
            id: RefCell::new(id.into()),
            transient,
            configuration: RefCell::new(configuration),
            name,
            folders_map: RefCell::new(TernarySearchTree::for_uris(ignore_path_casing.clone(), Rc::new(|_: &URI| true))),
            ignore_path_casing,
            folders: RefCell::new(Vec::new()),
        });
        workspace.set_folders(folders);
        workspace
    }

    pub fn id(&self) -> String {
        self.id.borrow().clone()
    }

    pub fn transient(&self) -> bool {
        self.transient
    }

    pub fn configuration(&self) -> Option<URI> {
        self.configuration.borrow().clone()
    }

    /// `set configuration(...)`: troca o arquivo de configuração do workspace.
    pub fn set_configuration(&self, configuration: Option<URI>) {
        *self.configuration.borrow_mut() = configuration;
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn folders(&self) -> Vec<WorkspaceFolder> {
        self.folders.borrow().clone()
    }

    /// `set folders(...)`: troca as pastas e refaz o índice por URI.
    pub fn set_folders(&self, folders: Vec<WorkspaceFolder>) {
        let mut map = TernarySearchTree::for_uris(self.ignore_path_casing.clone(), Rc::new(|_: &URI| true));
        for folder in &folders {
            map.set(folder.uri.clone(), folder.clone());
        }
        *self.folders.borrow_mut() = folders;
        *self.folders_map.borrow_mut() = map;
    }

    /// `getFolder(resource)`: a pasta cujo URI é o do recurso ou um prefixo dele.
    pub fn get_folder(&self, resource: &URI) -> Option<WorkspaceFolder> {
        self.folders_map.borrow().find_substr(resource).cloned()
    }
}

/// `toWorkspaceFolder`, com o nome já calculado por quem chama (o original usa `basenameOrAuthority`).
pub fn to_workspace_folder(resource: &URI, name: impl Into<String>) -> WorkspaceFolder {
    let raw = RawWorkspaceFolder::Uri { uri: resource.to_string(false).to_string_lossy(), name: None };
    WorkspaceFolder::new(resource.clone(), name, 0, Some(raw))
}

// ---------------------------------------------------------------------------------------------
// O resto do `workspace.ts`.
// ---------------------------------------------------------------------------------------------

use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::path::PathModule;
use je_base::uri::{URI as Uri, UriComponents};
use je_instantiation::service_id;

use crate::resources::{ext_uri_biased_ignore_path_case, host_environment};

/// `IRawFileWorkspaceFolder`.
pub type RawFileWorkspaceFolder = RawWorkspaceFolder;

impl RawWorkspaceFolder {
    /// O nome que o arquivo de workspace deu à pasta (`name?`).
    pub fn name(&self) -> Option<&str> {
        match self {
            RawWorkspaceFolder::Path { name, .. } | RawWorkspaceFolder::Uri { name, .. } => name.as_deref(),
        }
    }
}

impl WorkspaceFolder {
    /// `toResource(relativePath)`: o recurso com o caminho absoluto da pasta mais o relativo.
    pub fn to_resource(&self, relative_path: &str) -> URI {
        crate::resources::join_path(&self.uri, &[relative_path])
    }
}

/// `EXTENSION_DEVELOPMENT_EMPTY_WINDOW_WORKSPACE` e `UNKNOWN_EMPTY_WINDOW_WORKSPACE`.
pub const EXTENSION_DEVELOPMENT_EMPTY_WINDOW_WORKSPACE_ID: &str = "ext-dev";
pub const UNKNOWN_EMPTY_WINDOW_WORKSPACE_ID: &str = "empty-window";

/// `IWorkspaceIdentifier`: o caminho do arquivo de workspace mais o id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceIdentifier {
    pub id: String,
    pub config_path: URI,
}

/// `ISingleFolderWorkspaceIdentifier`: o caminho da pasta mais o id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SingleFolderWorkspaceIdentifier {
    pub id: String,
    pub uri: URI,
}

/// `IEmptyWorkspaceIdentifier`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmptyWorkspaceIdentifier {
    pub id: String,
}

/// `IAnyWorkspaceIdentifier`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnyWorkspaceIdentifier {
    Workspace(WorkspaceIdentifier),
    SingleFolder(SingleFolderWorkspaceIdentifier),
    Empty(EmptyWorkspaceIdentifier),
}

impl AnyWorkspaceIdentifier {
    /// `IBaseWorkspaceIdentifier.id`: único para cada workspace; não dá para abrir o mesmo `id` em duas janelas.
    pub fn id(&self) -> &str {
        match self {
            AnyWorkspaceIdentifier::Workspace(w) => &w.id,
            AnyWorkspaceIdentifier::SingleFolder(f) => &f.id,
            AnyWorkspaceIdentifier::Empty(e) => &e.id,
        }
    }

    /// `isSingleFolderWorkspaceIdentifier`.
    pub fn is_single_folder_workspace_identifier(&self) -> bool {
        matches!(self, AnyWorkspaceIdentifier::SingleFolder(_))
    }

    /// `isWorkspaceIdentifier`.
    pub fn is_workspace_identifier(&self) -> bool {
        matches!(self, AnyWorkspaceIdentifier::Workspace(_))
    }

    /// `isEmptyWorkspaceIdentifier`.
    pub fn is_empty_workspace_identifier(&self) -> bool {
        matches!(self, AnyWorkspaceIdentifier::Empty(_))
    }
}

/// `toWorkspaceIdentifier(backupPath, isExtensionDevelopment)`: o identificador de uma janela vazia.
pub fn to_empty_workspace_identifier(backup_path: Option<&str>, is_extension_development: bool) -> EmptyWorkspaceIdentifier {
    // With a backupPath, the basename is the empty workspace identifier
    if let Some(backup_path) = backup_path {
        let basename = PathModule::native(host_environment().as_ref()).basename(backup_path, None);
        return EmptyWorkspaceIdentifier { id: basename };
    }
    // Extension development empty windows have backups disabled so we return a constant workspace identifier for
    // extension authors to allow to restore their workspace state even then.
    if is_extension_development {
        return EmptyWorkspaceIdentifier { id: EXTENSION_DEVELOPMENT_EMPTY_WINDOW_WORKSPACE_ID.to_string() };
    }
    EmptyWorkspaceIdentifier { id: UNKNOWN_EMPTY_WINDOW_WORKSPACE_ID.to_string() }
}

/// `IWorkspace`: o que o `toWorkspaceIdentifier` e o `isWorkspace` leem de um workspace.
pub trait IWorkspace {
    fn id(&self) -> String;
    fn folders(&self) -> Vec<WorkspaceFolder>;
    fn transient(&self) -> bool;
    fn configuration(&self) -> Option<URI>;
    fn name(&self) -> Option<String>;
}

impl IWorkspace for Workspace {
    fn id(&self) -> String {
        Workspace::id(self)
    }
    fn folders(&self) -> Vec<WorkspaceFolder> {
        Workspace::folders(self)
    }
    fn transient(&self) -> bool {
        Workspace::transient(self)
    }
    fn configuration(&self) -> Option<URI> {
        Workspace::configuration(self)
    }
    fn name(&self) -> Option<String> {
        Workspace::name(self).map(str::to_string)
    }
}

/// Um `IWorkspace` literal (um objeto simples no original).
#[derive(Clone, Debug)]
pub struct WorkspaceSnapshot {
    pub id: String,
    pub folders: Vec<WorkspaceFolder>,
    pub transient: bool,
    pub configuration: Option<URI>,
    pub name: Option<String>,
}

impl IWorkspace for WorkspaceSnapshot {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn folders(&self) -> Vec<WorkspaceFolder> {
        self.folders.clone()
    }
    fn transient(&self) -> bool {
        self.transient
    }
    fn configuration(&self) -> Option<URI> {
        self.configuration.clone()
    }
    fn name(&self) -> Option<String> {
        self.name.clone()
    }
}

/// `toWorkspaceIdentifier(workspace)`.
pub fn to_workspace_identifier(workspace: &dyn IWorkspace) -> AnyWorkspaceIdentifier {
    // Multi root
    if let Some(configuration) = workspace.configuration() {
        return AnyWorkspaceIdentifier::Workspace(WorkspaceIdentifier { id: workspace.id(), config_path: configuration });
    }
    // Single folder
    let folders = workspace.folders();
    if folders.len() == 1 {
        return AnyWorkspaceIdentifier::SingleFolder(SingleFolderWorkspaceIdentifier { id: workspace.id(), uri: folders[0].uri.clone() });
    }
    // Empty window
    AnyWorkspaceIdentifier::Empty(EmptyWorkspaceIdentifier { id: workspace.id() })
}

fn uri_from_marshalled(value: &Dynamic) -> Option<URI> {
    if !matches!(value, Dynamic::Object(_)) {
        return None;
    }
    let part = |name: &str| match value.get_str(name) {
        Dynamic::String(s) => Some(s),
        _ => None,
    };
    Some(Uri::from(
        &UriComponents { scheme: part("scheme"), authority: part("authority"), path: part("path"), query: part("query"), fragment: part("fragment") },
        false,
    ))
}

/// `reviveIdentifier`: o identificador serializado (com `UriComponents`) de volta a um identificador de verdade.
pub fn revive_identifier(identifier: &Dynamic) -> Option<AnyWorkspaceIdentifier> {
    if !matches!(identifier, Dynamic::Object(_)) {
        return None;
    }
    let id = |value: &Dynamic| match value.get_str("id") {
        Dynamic::String(s) => Some(s.to_string_lossy()),
        _ => None,
    };
    // Single Folder
    if let (Some(id), Some(uri)) = (id(identifier), uri_from_marshalled(&identifier.get_str("uri"))) {
        return Some(AnyWorkspaceIdentifier::SingleFolder(SingleFolderWorkspaceIdentifier { id, uri }));
    }
    // Multi folder
    if let (Some(id), Some(config_path)) = (id(identifier), uri_from_marshalled(&identifier.get_str("configPath"))) {
        return Some(AnyWorkspaceIdentifier::Workspace(WorkspaceIdentifier { id, config_path }));
    }
    // Empty
    id(identifier).filter(|id| !id.is_empty()).map(|id| AnyWorkspaceIdentifier::Empty(EmptyWorkspaceIdentifier { id }))
}

/// `WorkbenchState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkbenchState {
    Empty = 1,
    Folder,
    Workspace,
}

/// `IWorkspaceFoldersChangeEvent`.
#[derive(Clone, Debug, Default)]
pub struct WorkspaceFoldersChangeEvent {
    pub added: Vec<WorkspaceFolder>,
    pub removed: Vec<WorkspaceFolder>,
    pub changed: Vec<WorkspaceFolder>,
}

/// `IWorkspaceFoldersWillChangeEvent`: `join` guarda as promessas que o emissor espera.
#[derive(Clone)]
pub struct WorkspaceFoldersWillChangeEvent {
    pub changes: WorkspaceFoldersChangeEvent,
    pub from_cache: bool,
    joiners: Rc<RefCell<Vec<je_base::promise::Promise<()>>>>,
}

impl WorkspaceFoldersWillChangeEvent {
    pub fn new(changes: WorkspaceFoldersChangeEvent, from_cache: bool) -> Self {
        Self { changes, from_cache, joiners: Rc::new(RefCell::new(Vec::new())) }
    }

    pub fn join(&self, promise: je_base::promise::Promise<()>) {
        self.joiners.borrow_mut().push(promise);
    }

    /// As promessas entregues a `join` até agora.
    pub fn joined(&self) -> Vec<je_base::promise::Promise<()>> {
        self.joiners.borrow().clone()
    }
}

/// `IWorkspaceContextService`.
pub trait IWorkspaceContextService {
    /// Dispara nas mudanças de estado do workbench.
    fn on_did_change_workbench_state(&self) -> Event<WorkbenchState>;
    /// Dispara nas mudanças de nome do workspace.
    fn on_did_change_workspace_name(&self) -> Event<()>;
    /// Dispara antes de as pastas do workspace mudarem.
    fn on_will_change_workspace_folders(&self) -> Event<WorkspaceFoldersWillChangeEvent>;
    /// Dispara quando as pastas do workspace mudam.
    fn on_did_change_workspace_folders(&self) -> Event<WorkspaceFoldersChangeEvent>;
    /// O objeto completo do workspace.
    fn get_complete_workspace(&self) -> je_base::promise::Promise<Rc<Workspace>>;
    /// O workspace com que a janela roda (use `get_complete_workspace` para o completo).
    fn get_workspace(&self) -> Rc<Workspace>;
    /// O estado do workbench: `Empty` (janela vazia ou arquivo), `Folder` ou `Workspace`.
    fn get_workbench_state(&self) -> WorkbenchState;
    /// A pasta do workspace que contém o recurso; `None` se não houver workspace ou o recurso estiver fora.
    fn get_workspace_folder(&self, resource: &URI) -> Option<WorkspaceFolder>;
    /// Se o workspace atual tem o identificador (ou a pasta raiz) informado.
    fn is_current_workspace(&self, workspace_id_or_folder: &CurrentWorkspaceQuery) -> bool;
    /// Se o recurso está dentro do workspace.
    fn is_inside_workspace(&self, resource: &URI) -> bool;
    /// Se o workspace atual tem dados (pastas ou configuração) que podem ir ao extension host.
    fn has_workspace_data(&self) -> bool;
}

service_id!(pub I_WORKSPACE_CONTEXT_SERVICE: dyn IWorkspaceContextService = "contextService");

/// `IWorkspaceIdentifier | ISingleFolderWorkspaceIdentifier | URI` de `isCurrentWorkspace`.
#[derive(Clone, Debug)]
pub enum CurrentWorkspaceQuery {
    Workspace(WorkspaceIdentifier),
    SingleFolder(SingleFolderWorkspaceIdentifier),
    Uri(URI),
}

/// `IResolvedWorkspace`.
#[derive(Clone, Debug)]
pub struct ResolvedWorkspace {
    pub identifier: WorkspaceIdentifier,
    pub remote_authority: Option<String>,
    pub transient: Option<bool>,
    pub folders: Vec<WorkspaceFolder>,
}

/// `isWorkspace`: um valor com `id` de texto e `folders` em lista.
pub fn is_workspace(thing: &Dynamic) -> bool {
    matches!(thing, Dynamic::Object(_)) && matches!(thing.get_str("id"), Dynamic::String(_)) && thing.get_str("folders").is_array()
}

/// `isWorkspaceFolder`: `uri`, `name` e `toResource`.
pub fn is_workspace_folder(thing: &Dynamic) -> bool {
    matches!(thing, Dynamic::Object(_))
        && matches!(thing.get_str("uri"), Dynamic::Object(_))
        && matches!(thing.get_str("name"), Dynamic::String(_))
        && matches!(thing.get_str("toResource"), Dynamic::Function(_))
}

pub const WORKSPACE_EXTENSION: &str = "code-workspace";
pub const WORKSPACE_SUFFIX: &str = ".code-workspace";
pub const UNTITLED_WORKSPACE_NAME: &str = "workspace.json";
pub const STANDALONE_EDITOR_WORKSPACE_ID: &str = "4064f6ec-cb38-4ad0-af64-ee6467e63c82";

/// `WORKSPACE_FILTER`: `{ name, extensions }`, com o nome localizado.
pub fn workspace_filter() -> Vec<(String, Vec<String>)> {
    vec![(je_base::nls::localize("codeWorkspace", "Code Workspace", &[]), vec![WORKSPACE_EXTENSION.to_string()])]
}

/// `isUntitledWorkspace(path, environmentService)`: `untitled_workspaces_home` é o `untitledWorkspacesHome` do ambiente.
pub fn is_untitled_workspace(path: &URI, untitled_workspaces_home: &URI) -> bool {
    ext_uri_biased_ignore_path_case().is_equal_or_parent(path, untitled_workspaces_home, false, false)
}

/// `isTemporaryWorkspace(path)` e `isTemporaryWorkspace(workspace)`: o esquema do arquivo de configuração é `vscode-tmp`.
pub fn is_temporary_workspace(path: Option<&URI>) -> bool {
    path.is_some_and(|p| p.scheme() == je_base::network::schemas::TMP)
}

/// `isStandaloneEditorWorkspace`.
pub fn is_standalone_editor_workspace(workspace: &dyn IWorkspace) -> bool {
    workspace.id() == STANDALONE_EDITOR_WORKSPACE_ID
}

/// `isSavedWorkspace`.
pub fn is_saved_workspace(path: &URI, untitled_workspaces_home: &URI) -> bool {
    !is_untitled_workspace(path, untitled_workspaces_home) && !is_temporary_workspace(Some(path))
}

/// `hasWorkspaceFileExtension(path)`: a extensão do caminho de texto é `.code-workspace`.
pub fn has_workspace_file_extension(path: &str) -> bool {
    PathModule::native(host_environment().as_ref()).extname(path) == WORKSPACE_SUFFIX
}

/// `hasWorkspaceFileExtension(uri)`.
pub fn has_workspace_file_extension_uri(path: &URI) -> bool {
    ext_uri_biased_ignore_path_case().extname(path) == WORKSPACE_SUFFIX
}

// ---------------------------------------------------------------------------------------------
// Porte de `src/vs/platform/workspace/test/common/testWorkspace.ts`: o workspace usado pelos testes. É infraestrutura
// de teste compartilhada por todas as crates acima, por isso fica público e junto do tipo que ele constrói.
//
// Divergências: a classe `Workspace` que estende a base (só fixa `transient` em falso e dá padrões aos parâmetros)
// é a função [`new_test_workspace`], porque o Rust não tem herança e os parâmetros opcionais são `Option`; o
// `TestWorkspace` (uma constante do módulo, compartilhada) é [`default_test_workspace`], que devolve sempre o mesmo `Rc`.
// ---------------------------------------------------------------------------------------------

use je_base::platform::Platform;
use je_base::resources::ExtUri;

/// `new Workspace(id, folders, configuration, ignorePathCasing)` de `testWorkspace.ts`. `ignore_path_casing` ausente é
/// o padrão do original (`() => !isLinux`).
pub fn new_test_workspace(id: &str, folders: Vec<WorkspaceFolder>, configuration: Option<URI>, ignore_path_casing: Option<UriPredicate>) -> Rc<Workspace> {
    let ignore_path_casing = ignore_path_casing.unwrap_or_else(|| -> UriPredicate {
        let is_linux = Platform::detect().is_linux;
        Rc::new(move |_: &URI| !is_linux)
    });
    Workspace::new(id, folders, false, configuration, ignore_path_casing, None)
}

/// `testWorkspace(...resource)`: o workspace `test-workspace` com uma pasta por recurso.
pub fn test_workspace(resources: &[URI]) -> Rc<Workspace> {
    let ext_uri = ExtUri::unbiased(host_environment());
    let folders = resources.iter().map(|resource| to_workspace_folder(resource, ext_uri.basename_or_authority(resource))).collect();
    new_test_workspace("test-workspace", folders, None, None)
}

thread_local! {
    static TEST_WORKSPACE: Rc<Workspace> = {
        let ws_uri = crate::resources::uri_file(if Platform::detect().is_windows { "C:\\testWorkspace" } else { "/testWorkspace" });
        test_workspace(&[ws_uri])
    };
}

/// `TestWorkspace`: o workspace padrão dos testes, com a pasta `/testWorkspace` (`C:\testWorkspace` no Windows).
/// Cada thread (cada teste) tem o seu, como cada processo de teste do original tem o seu módulo.
pub fn default_test_workspace() -> Rc<Workspace> {
    TEST_WORKSPACE.with(Rc::clone)
}
