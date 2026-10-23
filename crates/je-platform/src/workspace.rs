//! O mínimo de `src/vs/platform/workspace/common/workspace.ts` de que a configuração precisa: a pasta
//! do workspace e o `Workspace` que acha a pasta que contém um recurso.
//!
//! TODO(integração): trocar pelo porte completo (`IWorkspaceContextService`, `WorkbenchState`,
//! workspace sem título, `toWorkspaceFolder` com `basenameOrAuthority`).

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
