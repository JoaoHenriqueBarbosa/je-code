//! Porte de `src/vs/workbench/contrib/files/common/explorerModel.ts`: o `ExplorerModel`, o `ExplorerItem` e o
//! `NewExplorerItem`.
//!
//! Divergências:
//! - **Identidade e ciclos.** O item é um `Rc<ExplorerItem>`; o pai (`_parent`) e o `nestedParent` são `Weak`, porque
//!   o pai guarda os filhos no `children`. `this` (a identidade do `this === this.root`) é o `Weak` de `Rc::new_cyclic`.
//! - **Propriedades mutáveis.** `resource`, `_parent`, `_name`, `_isDirectory`, `_isSymbolicLink`, `_mtime`,
//!   `_isDirectoryResolved`, `isExcluded`, `error`, `nestedParent` e `nestedChildren` mudam depois da construção, então
//!   são `Cell` e `RefCell`, com um setter cada onde o original escreve a propriedade de fora.
//! - **`@memoize get children()`.** O `Map` nasce vazio junto com o item (o memoize só adia a criação). O `Map` é um
//!   [`OrderedMap`] (ordem de inserção); `children.size` e `children.keys()` são [`ExplorerItem::children_len`] e
//!   [`ExplorerItem::children_keys`].
//! - **`class NewExplorerItem extends ExplorerItem`.** Um [`ExplorerItem`] com a marca `is_new`, que o
//!   `oldChild instanceof NewExplorerItem` do `mergeLocalWithDisk` lê; [`NewExplorerItem::new`] é o construtor.
//! - **`move`.** `move` é palavra reservada: [`ExplorerItem::move_to`].
//! - **`rename(renamedStat: { name; mtime? })`.** Recebe o nome e o `mtime`: [`ExplorerItem::rename`].
//! - **`fetchChildren`.** Devolve `ExplorerItem[] | Promise<ExplorerItem[]>` como [`MaybePromise`]; a leitura do
//!   `explorer.fileNesting` é a do `IConfigurationService` com o recurso da raiz como `overrides`.
//! - **`IMarkdownString`.** `isReadonly` devolve o [`Readonly`] do `IFilesConfigurationService`, o `boolean |
//!   IMarkdownString` do original.
//! - **Texto.** As comparações de caminho do `find` rodam sobre as unidades UTF-16 do `URI.path`, como o `string` do
//!   JavaScript; só o nome do filho, que é a chave do `Map`, passa por `String`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::extpath;
use je_base::js_string::{self, index_of, starts_with_at};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::map::{OrderedMap, ResourceMap};
use je_base::promise::MaybePromise;
use je_base::strings;
use je_base::uri::URI;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::{FileStat, FileSystemProviderCapabilities, ResolveFileOptions};
use je_platform::files::i_file_service::IFileService;
use je_platform::resources::{ext_uri, join_path, uri_file};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::workspace::IWorkspaceContextService;

use super::common_files::SortOrder;
use super::explorer_file_nesting_trie::ExplorerFileNestingTrie;
use crate::services::files_configuration::files_configuration_service::{IFilesConfigurationService, Readonly};

/// `posix.sep` em unidades UTF-16.
const SEP: u16 = b'/' as u16;

/// `ExplorerModel`.
pub struct ExplorerModel {
    /// `_roots`.
    roots: RefCell<Vec<Rc<ExplorerItem>>>,
    /// `_listener`.
    listener: RefCell<Option<DisposableRef>>,
    /// `_onDidChangeRoots`.
    on_did_change_roots: Rc<Emitter<()>>,
    context_service: Rc<dyn IWorkspaceContextService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
}

impl ExplorerModel {
    /// `constructor(contextService, uriIdentityService, fileService, configService, filesConfigService)`.
    pub fn new(
        context_service: Rc<dyn IWorkspaceContextService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        file_service: Rc<dyn IFileService>,
        config_service: Rc<dyn IConfigurationService>,
        files_config_service: Rc<dyn IFilesConfigurationService>,
    ) -> Rc<Self> {
        let set_roots = {
            let context_service = context_service.clone();
            Rc::new(move || -> Vec<Rc<ExplorerItem>> {
                context_service
                    .get_workspace()
                    .folders()
                    .into_iter()
                    .map(|folder| {
                        ExplorerItem::new(
                            folder.uri,
                            file_service.clone(),
                            config_service.clone(),
                            files_config_service.clone(),
                            None,
                            Some(true),
                            Some(false),
                            Some(false),
                            Some(false),
                            Some(folder.name),
                            None,
                            false,
                        )
                    })
                    .collect()
            })
        };

        let this = Rc::new(Self {
            roots: RefCell::new(set_roots()),
            listener: RefCell::new(None),
            on_did_change_roots: Emitter::new(),
            context_service: context_service.clone(),
            uri_identity_service,
        });

        let weak = Rc::downgrade(&this);
        let listener = context_service.on_did_change_workspace_folders().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                *this.roots.borrow_mut() = set_roots();
                this.on_did_change_roots.fire(());
            }
        });
        *this.listener.borrow_mut() = Some(listener);

        this
    }

    /// `roots`.
    pub fn roots(&self) -> Vec<Rc<ExplorerItem>> {
        self.roots.borrow().clone()
    }

    /// O array `roots` do modelo, para quem o altera de fora (`service.roots.push(...)`).
    pub fn roots_mut(&self) -> std::cell::RefMut<'_, Vec<Rc<ExplorerItem>>> {
        self.roots.borrow_mut()
    }

    /// `onDidChangeRoots`.
    pub fn on_did_change_roots(&self) -> Event<()> {
        self.on_did_change_roots.event()
    }

    /// Returns an array of child stat from this stat that matches with the provided path.
    /// Starts matching from the first root.
    /// Will return empty array in case the FileStat does not exist.
    pub fn find_all(&self, resource: &URI) -> Vec<Rc<ExplorerItem>> {
        self.roots().iter().filter_map(|root| root.find(resource)).collect()
    }

    /// Returns a FileStat that matches the passed resource.
    /// In case multiple FileStat are matching the resource (same folder opened multiple times) returns the FileStat that has the closest root.
    /// Will return `None` in case the FileStat does not exist.
    pub fn find_closest(&self, resource: &URI) -> Option<Rc<ExplorerItem>> {
        let folder = self.context_service.get_workspace_folder(resource)?;
        let ext_uri = self.uri_identity_service.ext_uri();
        let root = self.roots().into_iter().find(|root| ext_uri.is_equal(Some(&root.resource()), Some(&folder.uri), false))?;
        root.find(resource)
    }
}

impl IDisposable for ExplorerModel {
    fn dispose(&self) {
        self.on_did_change_roots.dispose();
        if let Some(listener) = self.listener.borrow_mut().take() {
            listener.dispose();
        }
    }
}

/// Um `ExplorerItem` é comparado por identidade, como o objeto do original (a árvore do explorador e os mapas por item
/// o usam como chave): dois itens são iguais só quando são o mesmo item.
impl PartialEq for ExplorerItem {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for ExplorerItem {}

impl std::hash::Hash for ExplorerItem {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::ptr::hash(self, state);
    }
}

/// `ExplorerItem`.
pub struct ExplorerItem {
    this: Weak<ExplorerItem>,
    /// `resource` (público e mutável no original).
    resource: RefCell<URI>,
    file_service: Rc<dyn IFileService>,
    config_service: Rc<dyn IConfigurationService>,
    files_config_service: Rc<dyn IFilesConfigurationService>,
    /// `_parent`.
    parent: RefCell<Option<Weak<ExplorerItem>>>,
    /// `_isDirectory`.
    is_directory: Cell<bool>,
    /// `_isSymbolicLink`.
    is_symbolic_link: Cell<bool>,
    /// `_readonly`.
    readonly: Option<bool>,
    /// `_locked`.
    locked: Option<bool>,
    /// `_name`.
    name: RefCell<String>,
    /// `_mtime`.
    mtime: Cell<Option<f64>>,
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
    /// `_fileNester` (só a raiz o preenche).
    file_nester: RefCell<Option<Rc<ExplorerFileNestingTrie>>>,
    /// O `instanceof NewExplorerItem`.
    is_new: bool,
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
        Self::construct(resource, file_service, config_service, files_config_service, parent, is_directory, is_symbolic_link, readonly, locked, name, mtime, unknown, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn construct(
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
        is_new: bool,
    ) -> Rc<Self> {
        let name = name.unwrap_or_else(|| ext_uri().basename_or_authority(&resource));
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            resource: RefCell::new(resource),
            file_service,
            config_service,
            files_config_service,
            parent: RefCell::new(parent.map(Rc::downgrade)),
            is_directory: Cell::new(is_directory.unwrap_or(false)),
            is_symbolic_link: Cell::new(is_symbolic_link.unwrap_or(false)),
            readonly,
            locked,
            name: RefCell::new(name),
            mtime: Cell::new(mtime),
            unknown,
            is_directory_resolved: Cell::new(false),
            error: RefCell::new(None),
            is_excluded: Cell::new(false),
            nested_parent: RefCell::new(None),
            nested_children: RefCell::new(None),
            children: RefCell::new(OrderedMap::new()),
            marked_as_find_result: Cell::new(false),
            file_nester: RefCell::new(None),
            is_new,
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

    /// `resource = value`.
    pub fn set_resource(&self, value: URI) {
        *self.resource.borrow_mut() = value;
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
        self.is_symbolic_link.get()
    }

    /// `isDirectory`.
    pub fn is_directory(&self) -> bool {
        self.is_directory.get()
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
        self.mtime.get()
    }

    /// `name`.
    pub fn name(&self) -> String {
        self.name.borrow().clone()
    }

    /// `isUnknown`.
    pub fn is_unknown(&self) -> bool {
        self.unknown
    }

    /// O `instanceof NewExplorerItem`.
    pub fn is_new_explorer_item(&self) -> bool {
        self.is_new
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

    /// `error = value`.
    pub fn set_error(&self, value: Option<Error>) {
        *self.error.borrow_mut() = value;
    }

    /// `nestedParent`.
    pub fn nested_parent(&self) -> Option<Rc<ExplorerItem>> {
        self.nested_parent.borrow().as_ref().and_then(Weak::upgrade)
    }

    /// `nestedParent = value`.
    pub fn set_nested_parent(&self, value: Option<&Rc<ExplorerItem>>) {
        *self.nested_parent.borrow_mut() = value.map(Rc::downgrade);
    }

    /// `nestedChildren`.
    pub fn nested_children(&self) -> Option<Vec<Rc<ExplorerItem>>> {
        self.nested_children.borrow().clone()
    }

    /// `nestedChildren = value`.
    pub fn set_nested_children(&self, value: Option<Vec<Rc<ExplorerItem>>>) {
        *self.nested_children.borrow_mut() = value;
    }

    /// `children.values()`: os filhos pelo nome (na caixa do sistema de arquivos), na ordem de inserção.
    pub fn children(&self) -> Vec<Rc<ExplorerItem>> {
        self.children.borrow().values().cloned().collect()
    }

    /// `children.size`.
    pub fn children_len(&self) -> usize {
        self.children.borrow().len()
    }

    /// `children.keys()`: os nomes na caixa que o `Map` usa (minúsculos nos provedores sem caixa).
    pub fn children_keys(&self) -> Vec<String> {
        self.children.borrow().keys().cloned().collect()
    }

    /// `children.entries()`.
    fn children_entries(&self) -> Vec<(String, Rc<ExplorerItem>)> {
        self.children.borrow().iter().map(|(name, child)| (name.clone(), child.clone())).collect()
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

    /// `updateName(value)`.
    fn update_name(&self, value: &str) {
        let this = self.this();
        // Re-add to parent since the parent has a name map to children and the name might have changed
        if let Some(parent) = self.parent() {
            parent.remove_child(&this);
        }
        *self.name.borrow_mut() = value.to_string();
        if let Some(parent) = self.parent() {
            parent.add_child(&this);
        }
    }

    /// `ExplorerItem.create(fileService, configService, filesConfigService, raw, parent, resolveTo?)`.
    pub fn create(
        file_service: &Rc<dyn IFileService>,
        config_service: &Rc<dyn IConfigurationService>,
        files_config_service: &Rc<dyn IFilesConfigurationService>,
        raw: &FileStat,
        parent: Option<&Rc<ExplorerItem>>,
        resolve_to: Option<&[URI]>,
    ) -> Rc<ExplorerItem> {
        let stat = ExplorerItem::new(
            raw.resource.clone(),
            file_service.clone(),
            config_service.clone(),
            files_config_service.clone(),
            parent,
            Some(raw.is_directory),
            Some(raw.is_symbolic_link),
            raw.readonly,
            raw.locked,
            Some(raw.name.clone()),
            raw.mtime.map(|mtime| mtime as f64),
            !raw.is_file && !raw.is_directory,
        );

        // Recursively add children if present
        if stat.is_directory() {
            // isDirectoryResolved is a very important indicator in the stat model that tells if the folder was fully resolved
            // the folder is fully resolved if either it has a list of children or the client requested this by using the resolveTo
            // array of resource path to resolve.
            let extension = ext_uri();
            let resolved = raw.children.is_some()
                || resolve_to.is_some_and(|resolve_to| resolve_to.iter().any(|r| extension.is_equal_or_parent(r, &stat.resource(), false, false)));
            stat.is_directory_resolved.set(resolved);

            // Recurse into children
            if let Some(children) = &raw.children {
                for raw_child in children {
                    let child = ExplorerItem::create(file_service, config_service, files_config_service, raw_child, Some(&stat), resolve_to);
                    stat.add_child(&child);
                }
            }
        }

        stat
    }

    /// Merges the stat which was resolved from the disk with the local stat by copying over properties
    /// and children. The merge will only consider resolved stat elements to avoid overwriting data which
    /// exists locally.
    pub fn merge_local_with_disk(disk: &Rc<ExplorerItem>, local: &Rc<ExplorerItem>) {
        if disk.resource().to_string(false) != local.resource().to_string(false) {
            return; // Merging only supported for stats with the same resource
        }

        // Stop merging when a folder is not resolved to avoid loosing local data
        let merging_directories = disk.is_directory() || local.is_directory();
        if merging_directories && local.is_directory_resolved() && !disk.is_directory_resolved() {
            return;
        }

        // Properties
        local.set_resource(disk.resource());
        if !local.is_root() {
            local.update_name(&disk.name());
        }
        local.is_directory.set(disk.is_directory());
        local.mtime.set(disk.mtime());
        local.is_directory_resolved.set(disk.is_directory_resolved());
        local.is_symbolic_link.set(disk.is_symbolic_link());
        local.set_error(disk.error());

        // Merge Children if resolved
        if merging_directories && disk.is_directory_resolved() {
            // Map resource => stat
            let mut old_local_children: ResourceMap<Rc<ExplorerItem>> = ResourceMap::new();
            for child in local.children() {
                old_local_children.set(child.resource(), child);
            }

            // Clear current children
            local.children.borrow_mut().clear();

            // Merge received children
            for disk_child in disk.children() {
                let former_local_child = old_local_children.get(&disk_child.resource()).cloned();
                match former_local_child {
                    // Existing child: merge
                    Some(former_local_child) => {
                        ExplorerItem::merge_local_with_disk(&disk_child, &former_local_child);
                        local.add_child(&former_local_child);
                        old_local_children.delete(&disk_child.resource());
                    }

                    // New child: add
                    None => local.add_child(&disk_child),
                }
            }

            let remaining: Vec<Rc<ExplorerItem>> = old_local_children.values().cloned().collect();
            for old_child in remaining {
                if old_child.is_new_explorer_item() {
                    local.add_child(&old_child);
                }
            }
        }
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

    /// `fetchChildren(sortOrder)`.
    pub fn fetch_children(&self, sort_order: SortOrder) -> MaybePromise<Vec<Rc<ExplorerItem>>> {
        let nesting_config = self.nesting_config();

        // fast path when the children can be resolved sync
        if nesting_config.get_str("enabled").is_truthy()
            && let Some(nested_children) = self.nested_children()
        {
            return MaybePromise::Value(nested_children);
        }

        let this = self.this();
        MaybePromise::Promise(async_fn(async move {
            if !this.is_directory_resolved() {
                // Resolve metadata only when the mtime is needed since this can be expensive
                // Mtime is only used when the sort order is 'modified'
                let resolve_metadata = sort_order == SortOrder::Modified;
                this.set_error(None);
                let options = ResolveFileOptions { resolve_to: Vec::new(), resolve_single_child_descendants: true, resolve_metadata };
                match this.file_service.resolve(&this.resource(), Some(options)).await {
                    Ok(stat) => {
                        let resolved =
                            ExplorerItem::create(&this.file_service, &this.config_service, &this.files_config_service, &stat, Some(&this), None);
                        ExplorerItem::merge_local_with_disk(&resolved, &this);
                    }
                    Err(error) => {
                        this.set_error(Some(error.clone()));
                        return Err(error);
                    }
                }
                this.is_directory_resolved.set(true);
            }

            let mut items: Vec<Rc<ExplorerItem>> = Vec::new();
            if nesting_config.get_str("enabled").is_truthy() {
                let mut file_children: Vec<(String, Rc<ExplorerItem>)> = Vec::new();
                let mut dir_children: Vec<(String, Rc<ExplorerItem>)> = Vec::new();
                for entry in this.children_entries() {
                    entry.1.set_nested_parent(None);
                    if entry.1.is_directory() {
                        dir_children.push(entry);
                    } else {
                        file_children.push(entry);
                    }
                }

                let file_names: Vec<&str> = file_children.iter().map(|(name, _)| name.as_str()).collect();
                let nested = this.file_nester().nest(&file_names, &this.get_platform_aware_name(&this.name()));

                for (file_entry_name, file_entry_item) in &file_children {
                    match nested.get(file_entry_name) {
                        Some(nested_items) => {
                            let mut nested_children = Vec::new();
                            for name in nested_items.keys() {
                                let child = this.children.borrow().get(name).cloned().unwrap_or_else(|| {
                                    errors::throw(Error::new("Assertion Failed: argument is undefined or null"))
                                });
                                child.set_nested_parent(Some(file_entry_item));
                                nested_children.push(child);
                            }
                            file_entry_item.set_nested_children(Some(nested_children));
                            items.push(file_entry_item.clone());
                        }
                        None => file_entry_item.set_nested_children(None),
                    }
                }

                for (_, dir_entry_item) in dir_children {
                    items.push(dir_entry_item);
                }
            } else {
                for (_, child) in this.children_entries() {
                    items.push(child);
                }
            }
            Ok(items)
        }))
    }

    /// `this.configService.getValue<IFilesConfiguration>({ resource: this.root.resource }).explorer.fileNesting`.
    fn nesting_config(&self) -> Dynamic {
        let overrides = ConfigurationOverrides::with_resource(self.root().resource());
        self.config_service.get_value(None, Some(&overrides)).get_str("explorer").get_str("fileNesting")
    }

    /// `fileNester`: a trie da raiz, criada na primeira leitura.
    fn file_nester(&self) -> Rc<ExplorerFileNestingTrie> {
        let root = self.root();
        if let Some(nester) = root.file_nester.borrow().clone() {
            return nester;
        }

        let patterns_config = self.nesting_config().get_str("patterns");
        let mut patterns: Vec<(String, Vec<String>)> = Vec::new();
        for (parent_pattern, children_patterns) in patterns_config.own_entries() {
            let (Dynamic::String(children_patterns), parent_pattern) = (children_patterns, parent_pattern) else {
                continue;
            };
            if parent_pattern.is_empty() || children_patterns.is_empty() {
                continue;
            }
            let parent = self.get_platform_aware_name(&js_trim(&parent_pattern.to_string_lossy()));
            let children = children_patterns
                .to_string_lossy()
                .split(',')
                .map(|pattern| self.get_platform_aware_name(&js_trim(&js_trim(pattern).replace('\u{200b}', ""))))
                .filter(|pattern| !pattern.is_empty())
                .collect();
            patterns.push((parent, children));
        }

        let children_refs: Vec<Vec<&str>> = patterns.iter().map(|(_, children)| children.iter().map(String::as_str).collect()).collect();
        let config: Vec<(&str, &[&str])> = patterns.iter().zip(&children_refs).map(|((parent, _), children)| (parent.as_str(), children.as_slice())).collect();
        let nester = Rc::new(ExplorerFileNestingTrie::new(&config));
        *root.file_nester.borrow_mut() = Some(nester.clone());
        nester
    }

    /// `removeChild(child)`: removes a child element from this folder.
    pub fn remove_child(&self, child: &Rc<ExplorerItem>) {
        *self.nested_children.borrow_mut() = None;
        self.children.borrow_mut().remove(&self.get_platform_aware_name(&child.name()));
    }

    /// `forgetChildren()`.
    pub fn forget_children(&self) {
        self.children.borrow_mut().clear();
        *self.nested_children.borrow_mut() = None;
        self.is_directory_resolved.set(false);
        *self.file_nester.borrow_mut() = None;
    }

    /// `getPlatformAwareName(name)`.
    fn get_platform_aware_name(&self, name: &str) -> String {
        if self.file_service.has_capability(&self.resource(), FileSystemProviderCapabilities::PATH_CASE_SENSITIVE) {
            name.to_string()
        } else {
            JsString::from(name).to_lower_case().to_string_lossy()
        }
    }

    /// `move(newParent)`: moves this element under a new parent element.
    pub fn move_to(&self, new_parent: &Rc<ExplorerItem>) {
        let this = self.this();
        if let Some(nested_parent) = self.nested_parent() {
            nested_parent.remove_child(&this);
        }
        if let Some(parent) = self.parent() {
            parent.remove_child(&this);
        }
        new_parent.remove_child(&this); // make sure to remove any previous version of the file if any
        new_parent.add_child(&this);
        self.update_resource(true);
    }

    /// `updateResource(recursive)`.
    fn update_resource(&self, recursive: bool) {
        if let Some(parent) = self.parent() {
            let resource = join_path(&parent.resource(), &[self.name().as_str()]);
            self.set_resource(resource);
        }

        if recursive && self.is_directory() {
            for child in self.children() {
                child.update_resource(true);
            }
        }
    }

    /// `rename(renamedStat)`: tells this stat that it was renamed. This requires changes to all children of this stat
    /// (if any) so that the path property can be updated properly.
    pub fn rename(&self, name: &str, mtime: Option<f64>) {
        // Merge a subset of Properties that can change on rename
        self.update_name(name);
        self.mtime.set(mtime);

        // Update Paths including children
        self.update_resource(true);
    }

    /// `find(resource)`: returns a child stat from this stat that matches with the provided path.
    /// Will return `None` in case the child does not exist.
    pub fn find(&self, resource: &URI) -> Option<Rc<ExplorerItem>> {
        // Return if path found
        // For performance reasons try to do the comparison as fast as possible
        let ignore_case = !self.file_service.has_capability(resource, FileSystemProviderCapabilities::PATH_CASE_SENSITIVE);
        let own = self.resource();
        let (own_path, path) = (own.path().units(), resource.path().units());
        if own.scheme() == resource.scheme()
            && strings::equals_ignore_case(own.authority().units(), resource.authority().units())
            && (if ignore_case { strings::starts_with_ignore_case(path, own_path) } else { starts_with_at(path, own_path, 0) })
        {
            return self.find_by_path(strings::rtrim(path, &[SEP]), own_path.len(), ignore_case);
        }

        None //Unable to find
    }

    /// `findByPath(path, index, ignoreCase)`.
    fn find_by_path(&self, path: &[u16], index: usize, ignore_case: bool) -> Option<Rc<ExplorerItem>> {
        let own = self.resource();
        let own_path = String::from_utf16_lossy(strings::rtrim(own.path().units(), &[SEP]));
        if extpath::is_equal(&own_path, &String::from_utf16_lossy(path), ignore_case) {
            return Some(self.this());
        }

        if self.is_directory() {
            // Ignore separtor to more easily deduct the next name to search
            let mut index = index;
            while index < path.len() && path[index] == SEP {
                index += 1;
            }

            // If there is no separator take the remainder of the path
            let index_of_next_sep = index_of(path, &[SEP], index).unwrap_or(path.len());
            // The name to search is between two separators
            let name = String::from_utf16_lossy(&path[index..index_of_next_sep]);

            let child = self.children.borrow().get(&self.get_platform_aware_name(&name)).cloned();

            if let Some(child) = child {
                // We found a child with the given name, search inside it
                return child.find_by_path(path, index_of_next_sep, ignore_case);
            }
        }

        None
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

/// `String.prototype.trim` (o `\s` da ECMAScript, não o `char::is_whitespace` do Rust).
fn js_trim(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(js_string::trim(&units))
}

/// `NewExplorerItem`: o item provisório que a vista cria enquanto o usuário digita o nome do novo arquivo ou pasta.
pub struct NewExplorerItem;

impl NewExplorerItem {
    /// `constructor(fileService, configService, filesConfigService, parent, isDirectory)`.
    pub fn new(
        file_service: Rc<dyn IFileService>,
        config_service: Rc<dyn IConfigurationService>,
        files_config_service: Rc<dyn IFilesConfigurationService>,
        parent: &Rc<ExplorerItem>,
        is_directory: bool,
    ) -> Rc<ExplorerItem> {
        let item = ExplorerItem::construct(
            uri_file(""),
            file_service,
            config_service,
            files_config_service,
            Some(parent),
            Some(is_directory),
            None,
            None,
            None,
            None,
            None,
            false,
            true,
        );
        item.is_directory_resolved.set(true);
        item
    }
}
