//! Porte parcial de `src/vs/workbench/contrib/files/common/explorerModel.ts`: o `ExplorerItem`.
//!
//! O que entra é o que o recortar, copiar e colar do explorador lê e o que o próprio item mantém por si (o nome, o
//! recurso, o pai e os filhos). O `ExplorerModel`, o `NewExplorerItem`, o `ExplorerItem.create`, o `mergeLocalWithDisk`, o
//! `fetchChildren` (com o aninhamento de arquivos), o `find`, o `move` e o `rename` entram com o `ExplorerService` e a
//! vista do explorador, que são os que os chamam.
//!
//! Divergências:
//! - **Identidade e ciclos.** O item é um `Rc<ExplorerItem>`; o pai (`_parent`) é um `Weak`, porque o pai guarda os
//!   filhos no `children`. `this` (a identidade do `this === this.root`) é o `Weak` de `Rc::new_cyclic`.
//! - **Propriedades mutáveis.** `resource`, `_parent`, `_name`, `_isDirectoryResolved`, `isExcluded` e `nestedChildren`
//!   mudam depois da construção, então são `Cell` e `RefCell`.
//! - **`@memoize get children()`.** O `Map` nasce vazio junto com o item (o memoize só adia a criação).
//! - **`IMarkdownString`.** `isReadonly` devolve o [`Readonly`] do `IFilesConfigurationService`, o `boolean |
//!   IMarkdownString` do original.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::errors::Error;
use je_base::map::OrderedMap;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::{FileStat, FileSystemProviderCapabilities};
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::{ext_uri, join_path};

use crate::services::files_configuration::files_configuration_service::{IFilesConfigurationService, Readonly};

/// `ExplorerItem`.
pub struct ExplorerItem {
    this: Weak<ExplorerItem>,
    /// `resource` (público e mutável no original).
    resource: RefCell<URI>,
    file_service: Rc<dyn IFileService>,
    #[allow(dead_code)]
    config_service: Rc<dyn IConfigurationService>,
    files_config_service: Rc<dyn IFilesConfigurationService>,
    /// `_parent`.
    parent: RefCell<Option<Weak<ExplorerItem>>>,
    /// `_isDirectory`.
    is_directory: bool,
    /// `_isSymbolicLink`.
    is_symbolic_link: bool,
    /// `_readonly`.
    readonly: Option<bool>,
    /// `_locked`.
    locked: Option<bool>,
    /// `_name`.
    name: RefCell<String>,
    /// `_mtime`.
    mtime: Option<f64>,
    /// `_unknown`.
    unknown: bool,
    /// `_isDirectoryResolved` (usado nos testes).
    is_directory_resolved: Cell<bool>,
    /// `error`.
    error: RefCell<Option<Error>>,
    /// `_isExcluded`.
    is_excluded: Cell<bool>,
    /// `nestedParent`.
    nested_parent: RefCell<Option<Weak<ExplorerItem>>>,
    /// `nestedChildren`.
    nested_children: RefCell<Option<Vec<Rc<ExplorerItem>>>>,
    /// `children`.
    children: RefCell<OrderedMap<String, Rc<ExplorerItem>>>,
    /// `markedAsFindResult`.
    marked_as_find_result: Cell<bool>,
}

impl ExplorerItem {
    /// `constructor(resource, fileService, configService, filesConfigService, _parent, _isDirectory?, _isSymbolicLink?,
    /// _readonly?, _locked?, _name = basenameOrAuthority(resource), _mtime?, _unknown = false)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        resource: URI,
        file_service: Rc<dyn IFileService>,
        config_service: Rc<dyn IConfigurationService>,
        files_config_service: Rc<dyn IFilesConfigurationService>,
        parent: Option<&Rc<ExplorerItem>>,
        is_directory: Option<bool>,
        is_symbolic_link: Option<bool>,
        readonly: Option<bool>,
        locked: Option<bool>,
        name: Option<String>,
        mtime: Option<f64>,
        unknown: bool,
    ) -> Rc<Self> {
        let name = name.unwrap_or_else(|| ext_uri().basename_or_authority(&resource));
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            resource: RefCell::new(resource),
            file_service,
            config_service,
            files_config_service,
            parent: RefCell::new(parent.map(Rc::downgrade)),
            is_directory: is_directory.unwrap_or(false),
            is_symbolic_link: is_symbolic_link.unwrap_or(false),
            readonly,
            locked,
            name: RefCell::new(name),
            mtime,
            unknown,
            is_directory_resolved: Cell::new(false),
            error: RefCell::new(None),
            is_excluded: Cell::new(false),
            nested_parent: RefCell::new(None),
            nested_children: RefCell::new(None),
            children: RefCell::new(OrderedMap::new()),
            marked_as_find_result: Cell::new(false),
        })
    }

    /// A própria instância (o `this` do original).
    fn this(&self) -> Rc<ExplorerItem> {
        self.this.upgrade().expect("o item do explorador já foi liberado")
    }

    /// `resource`.
    pub fn resource(&self) -> URI {
        self.resource.borrow().clone()
    }

    /// `isExcluded`: o próprio ou o de algum ancestral.
    pub fn is_excluded(&self) -> bool {
        if self.is_excluded.get() {
            return true;
        }
        match self.parent() {
            None => false,
            Some(parent) => parent.is_excluded(),
        }
    }

    /// `isExcluded = value`.
    pub fn set_excluded(&self, value: bool) {
        self.is_excluded.set(value);
    }

    /// `hasChildren(filter)`.
    pub fn has_children(&self, filter: impl Fn(&Rc<ExplorerItem>) -> bool) -> bool {
        if self.has_nests() {
            self.nested_children.borrow().as_ref().is_some_and(|children| children.iter().any(&filter))
        } else {
            self.is_directory()
        }
    }

    /// `hasNests`.
    pub fn has_nests(&self) -> bool {
        self.nested_children.borrow().as_ref().is_some_and(|children| !children.is_empty())
    }

    /// `isDirectoryResolved`.
    pub fn is_directory_resolved(&self) -> bool {
        self.is_directory_resolved.get()
    }

    /// `_isDirectoryResolved = value`.
    pub fn set_directory_resolved(&self, value: bool) {
        self.is_directory_resolved.set(value);
    }

    /// `isSymbolicLink`.
    pub fn is_symbolic_link(&self) -> bool {
        self.is_symbolic_link
    }

    /// `isDirectory`.
    pub fn is_directory(&self) -> bool {
        self.is_directory
    }

    /// `isReadonly`: `filesConfigService.isReadonly(resource, { resource, name, readonly, locked })`.
    pub fn is_readonly(&self) -> Readonly {
        let resource = self.resource();
        let stat = FileStat {
            resource: resource.clone(),
            name: self.name(),
            size: None,
            mtime: None,
            ctime: None,
            etag: None,
            readonly: self.readonly,
            locked: self.locked,
            executable: None,
            is_file: false,
            is_directory: false,
            is_symbolic_link: false,
            children: None,
        };
        self.files_config_service.is_readonly(&resource, Some(&stat))
    }

    /// `mtime`.
    pub fn mtime(&self) -> Option<f64> {
        self.mtime
    }

    /// `name`.
    pub fn name(&self) -> String {
        self.name.borrow().clone()
    }

    /// `isUnknown`.
    pub fn is_unknown(&self) -> bool {
        self.unknown
    }

    /// `parent`.
    pub fn parent(&self) -> Option<Rc<ExplorerItem>> {
        self.parent.borrow().as_ref().and_then(Weak::upgrade)
    }

    /// `root`.
    pub fn root(&self) -> Rc<ExplorerItem> {
        match self.parent() {
            None => self.this(),
            Some(parent) => parent.root(),
        }
    }

    /// `error`.
    pub fn error(&self) -> Option<Error> {
        self.error.borrow().clone()
    }

    /// `nestedParent`.
    pub fn nested_parent(&self) -> Option<Rc<ExplorerItem>> {
        self.nested_parent.borrow().as_ref().and_then(Weak::upgrade)
    }

    /// `nestedChildren`.
    pub fn nested_children(&self) -> Option<Vec<Rc<ExplorerItem>>> {
        self.nested_children.borrow().clone()
    }

    /// `children`: os filhos pelo nome (na caixa do sistema de arquivos), na ordem de inserção.
    pub fn children(&self) -> Vec<Rc<ExplorerItem>> {
        self.children.borrow().values().cloned().collect()
    }

    /// `getId()`.
    pub fn get_id(&self) -> String {
        let mut id = format!("{}::{}", self.root().resource().to_string(false).to_string_lossy(), self.resource().to_string(false).to_string_lossy());

        if self.is_marked_as_filtered() {
            id.push_str("::findFilterResult");
        }

        id
    }

    /// `toString()`.
    pub fn to_display_string(&self) -> String {
        format!("ExplorerItem: {}", self.name())
    }

    /// `isRoot`: `this === this.root`.
    pub fn is_root(&self) -> bool {
        Rc::ptr_eq(&self.this(), &self.root())
    }

    /// `addChild(child)`: adds a child element to this folder.
    pub fn add_child(&self, child: &Rc<ExplorerItem>) {
        // Inherit some parent properties to child
        *child.parent.borrow_mut() = Some(self.this.clone());
        child.update_resource(false);
        let key = self.get_platform_aware_name(&child.name());
        self.children.borrow_mut().insert(key, child.clone());
    }

    /// `getChild(name)`.
    pub fn get_child(&self, name: &str) -> Option<Rc<ExplorerItem>> {
        self.children.borrow().get(&self.get_platform_aware_name(name)).cloned()
    }

    /// `removeChild(child)`: removes a child element from this folder.
    pub fn remove_child(&self, child: &Rc<ExplorerItem>) {
        *self.nested_children.borrow_mut() = None;
        self.children.borrow_mut().remove(&self.get_platform_aware_name(&child.name()));
    }

    /// `getPlatformAwareName(name)`.
    fn get_platform_aware_name(&self, name: &str) -> String {
        if self.file_service.has_capability(&self.resource(), FileSystemProviderCapabilities::PATH_CASE_SENSITIVE) {
            name.to_string()
        } else {
            JsString::from(name).to_lower_case().to_string_lossy()
        }
    }

    /// `updateResource(recursive)`.
    fn update_resource(&self, recursive: bool) {
        if let Some(parent) = self.parent() {
            let resource = join_path(&parent.resource(), &[self.name().as_str()]);
            *self.resource.borrow_mut() = resource;
        }

        if recursive && self.is_directory() {
            for child in self.children() {
                child.update_resource(true);
            }
        }
    }

    /// `isMarkedAsFiltered()`.
    pub fn is_marked_as_filtered(&self) -> bool {
        self.marked_as_find_result.get()
    }

    /// `markItemAndParentsAsFiltered()`.
    pub fn mark_item_and_parents_as_filtered(&self) {
        self.marked_as_find_result.set(true);
        if let Some(parent) = self.parent() {
            parent.mark_item_and_parents_as_filtered();
        }
    }

    /// `unmarkItemAndChildren()`.
    pub fn unmark_item_and_children(&self) {
        self.marked_as_find_result.set(false);
        for child in self.children() {
            child.unmark_item_and_children();
        }
    }
}
