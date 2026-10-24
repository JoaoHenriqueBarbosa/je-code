//! Porte de `src/vs/workbench/contrib/search/browser/searchTreeModel/folderMatch.ts`: a pasta de uma árvore de
//! resultados de busca (`FolderMatchImpl` e as subclasses `FolderMatchWithResourceImpl`, `FolderMatchWorkspaceRootImpl` e
//! `FolderMatchNoRootImpl`).
//!
//! Divergências:
//! - **Herança.** As quatro classes têm as mesmas dependências injetadas e só diferem em três pontos (o recurso, o
//!   `createAndConfigureFileMatch` e o que a pasta é para os predicados de tipo), então o porte é um único
//!   [`FolderMatchImpl`] com o [`FolderMatchKind`] como parâmetro (a diferença virando parâmetro). Os argumentos de
//!   `createInstance` de cada classe são os construtores de [`FolderMatchArgs`]: [`FolderMatchArgs::with_resource`]
//!   (`FolderMatchWithResourceImpl`), [`FolderMatchArgs::workspace_root`] (`FolderMatchWorkspaceRootImpl`) e
//!   [`FolderMatchArgs::no_root`] (`FolderMatchNoRootImpl`).
//! - O pai é um `Weak` (a pasta ou o cabeçalho dono guarda os filhos); o `_searchResult` do original é lido pelo pai
//!   (`searchModel` sobe até o cabeçalho, que conhece o resultado), que dá o mesmo objeto.
//! - `FolderMatchImpl.parent()` devolve [`FolderMatchParent`]; `isInParentChain` anda pelos pais comparando `id()`.
//! - `ResourceMap` e `TernarySearchTree` guardam `Rc<FolderMatchImpl>` e `Rc<dyn ISearchTreeFileMatch>`; `normalizedResource`
//!   é um [`Lazy`].
//! - O ramo de `bindNotebookEditorWidget` e `unbindNotebookEditorWidget` que desce para as pastas filhas quando o arquivo
//!   é indefinido é código morto no original (só entra se `isNotebookFileMatch(fileMatch)`), e não existe aqui.
//! - `dispose` conta suas chamadas em [`FolderMatchImpl`] para o teste que espiona `dispose` no original.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lazy::Lazy;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::ResourceMap;
use je_base::promise::Promise;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::URI;
use je_editor::text_model::TextModel;
use je_instantiation::{Injectable, InstantiationService};
use je_platform::label::label::ILabelService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use super::r#match::text_search_result_to_matches;
use super::search_tree_common::{
    FileOrFolderMatch, FolderMatchParent, FOLDER_MATCH_PREFIX, IChangeEvent, ISearchModel, ISearchTreeFileMatch, ISearchTreeFolderMatch,
    create_folder_matches_map, get_file_matches,
};
use crate::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;
use crate::contrib::search::notebook_search::notebook_search_model::{NotebookCompatibleFileMatch, NotebookCompatibleFileMatchArgs};
use crate::contrib::search::notebook_search::search_notebook_helpers::{get_id_from_i_notebook_cell_match, is_i_notebook_file_match_with_model};
use crate::contrib::search::common::search_notebook_helpers::is_i_notebook_file_match_no_model;
use crate::contrib::search::replace::IReplaceService;
use crate::services::search::search::{IFileMatch, ITextQuery, ITextSearchQuery, ITextSearchResult, PatternInfo};

/// O que a pasta é para os predicados de tipo e para `createAndConfigureFileMatch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderMatchKind {
    /// `FolderMatchWithResourceImpl`: uma pasta intermediária.
    WithResource,
    /// `FolderMatchWorkspaceRootImpl`.
    WorkspaceRoot,
    /// `FolderMatchNoRootImpl`: "Other files".
    NoRoot,
}

/// Os argumentos estáticos do construtor (`resource`, `_id`, `_index`, `_query`, `_parent` e `_closestRoot`).
pub struct FolderMatchArgs {
    pub kind: FolderMatchKind,
    pub resource: Option<URI>,
    pub id: String,
    pub index: i32,
    pub query: ITextQuery,
    pub parent: FolderMatchParent,
    pub closest_root: Option<Rc<dyn ISearchTreeFolderMatch>>,
}

impl FolderMatchArgs {
    /// `createInstance(FolderMatchWithResourceImpl, resource, id, index, query, parent, searchResult, closestRoot)`.
    pub fn with_resource(
        resource: URI,
        id: String,
        index: i32,
        query: ITextQuery,
        parent: FolderMatchParent,
        closest_root: Rc<dyn ISearchTreeFolderMatch>,
    ) -> Self {
        Self { kind: FolderMatchKind::WithResource, resource: Some(resource), id, index, query, parent, closest_root: Some(closest_root) }
    }

    /// `createInstance(FolderMatchWorkspaceRootImpl, resource, id, index, query, parent)`.
    pub fn workspace_root(resource: URI, id: String, index: i32, query: ITextQuery, parent: Rc<dyn super::search_tree_common::ITextSearchHeading>) -> Self {
        Self {
            kind: FolderMatchKind::WorkspaceRoot,
            resource: Some(resource),
            id,
            index,
            query,
            parent: FolderMatchParent::Heading(parent),
            closest_root: None,
        }
    }

    /// `createInstance(FolderMatchNoRootImpl, id, index, query, parent)`.
    pub fn no_root(id: String, index: i32, query: ITextQuery, parent: Rc<dyn super::search_tree_common::ITextSearchHeading>) -> Self {
        Self { kind: FolderMatchKind::NoRoot, resource: None, id, index, query, parent: FolderMatchParent::Heading(parent), closest_root: None }
    }
}

/// O `_parent` guardado (`ITextSearchHeading | FolderMatchImpl`), como `Weak`.
enum ParentRef {
    Folder(Weak<dyn ISearchTreeFolderMatch>),
    Heading(Weak<dyn super::search_tree_common::ITextSearchHeading>),
}

/// `FolderMatchImpl`.
pub struct FolderMatchImpl {
    this: Weak<FolderMatchImpl>,
    base: Rc<Disposable>,
    kind: FolderMatchKind,

    on_change: Rc<Emitter<IChangeEvent>>,
    on_dispose: Rc<Emitter<()>>,

    file_matches: RefCell<ResourceMap<Rc<dyn ISearchTreeFileMatch>>>,
    folder_matches: RefCell<ResourceMap<Rc<FolderMatchImpl>>>,
    folder_matches_map: RefCell<TernarySearchTree<URI, Rc<FolderMatchImpl>>>,
    un_disposed_file_matches: RefCell<ResourceMap<Rc<dyn ISearchTreeFileMatch>>>,
    un_disposed_folder_matches: RefCell<ResourceMap<Rc<FolderMatchImpl>>>,
    replacing_all: Cell<bool>,
    name: Lazy<String>,
    id: String,
    resource: Option<URI>,
    index: i32,
    query: ITextQuery,
    parent: ParentRef,
    closest_root: Option<Weak<dyn ISearchTreeFolderMatch>>,
    normalized_resource: Option<Lazy<URI>>,

    replace_service: Rc<dyn IReplaceService>,
    instantiation_service: Rc<InstantiationService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    dispose_count: Cell<usize>,
}

impl Injectable for FolderMatchImpl {
    type Args = FolderMatchArgs;
    type Deps = (Rc<dyn IReplaceService>, Rc<InstantiationService>, Rc<dyn ILabelService>, Rc<dyn IUriIdentityService>);

    fn create(args: FolderMatchArgs, (replace_service, instantiation_service, label_service, uri_identity_service): Self::Deps) -> Rc<Self> {
        let FolderMatchArgs { kind, resource, id, index, query, parent, closest_root } = args;
        Rc::new_cyclic(|this: &Weak<FolderMatchImpl>| {
            let base = Disposable::new();
            let on_change = Emitter::<IChangeEvent>::new();
            base.register(on_change.clone());
            let on_dispose = Emitter::<()>::new();
            base.register(on_dispose.clone());

            let name_resource = resource.clone();
            let normalized_resource = (kind != FolderMatchKind::NoRoot && resource.is_some()).then(|| {
                let service = uri_identity_service.clone();
                let resource = resource.clone().expect("a pasta com recurso tem recurso");
                Lazy::new(move || {
                    let ext_uri = service.ext_uri();
                    ext_uri.remove_trailing_path_separator(&ext_uri.normalize_path(&resource), None)
                })
            });
            let parent_ref = match &parent {
                FolderMatchParent::FolderMatch(folder_match) => ParentRef::Folder(Rc::downgrade(folder_match)),
                FolderMatchParent::Heading(heading) => ParentRef::Heading(Rc::downgrade(heading)),
            };
            Self {
                this: this.clone(),
                base,
                kind,
                on_change,
                on_dispose,
                file_matches: RefCell::new(ResourceMap::new()),
                folder_matches: RefCell::new(ResourceMap::new()),
                folder_matches_map: RefCell::new(create_folder_matches_map(&uri_identity_service)),
                un_disposed_file_matches: RefCell::new(ResourceMap::new()),
                un_disposed_folder_matches: RefCell::new(ResourceMap::new()),
                replacing_all: Cell::new(false),
                name: Lazy::new(move || match &name_resource {
                    Some(resource) => label_service.get_uri_basename_label(resource),
                    None => String::new(),
                }),
                id: format!("{FOLDER_MATCH_PREFIX}{id}"),
                resource,
                index,
                query,
                parent: parent_ref,
                closest_root: closest_root.as_ref().map(Rc::downgrade),
                normalized_resource,
                replace_service,
                instantiation_service,
                uri_identity_service,
                dispose_count: Cell::new(0),
            }
        })
    }
}

impl FolderMatchImpl {
    fn self_rc(&self) -> Rc<FolderMatchImpl> {
        self.this.upgrade().expect("a pasta está viva enquanto é usada")
    }

    fn parent_ref(&self) -> FolderMatchParent {
        match &self.parent {
            ParentRef::Folder(folder_match) => FolderMatchParent::FolderMatch(folder_match.upgrade().expect("a pasta pai está viva")),
            ParentRef::Heading(heading) => FolderMatchParent::Heading(heading.upgrade().expect("o cabeçalho pai está vivo")),
        }
    }

    /// `FolderMatchWithResourceImpl.resource` (não nulo).
    fn resource_uri(&self) -> URI {
        self.resource.clone().expect("a pasta com recurso tem recurso")
    }

    /// `FolderMatchWithResourceImpl.normalizedResource`.
    fn normalized_resource(&self) -> URI {
        self.normalized_resource.as_ref().expect("a pasta com recurso tem recurso normalizado").value()
    }

    pub fn create_intermediate_folder_match(
        &self,
        resource: URI,
        id: String,
        index: i32,
        query: ITextQuery,
        base_workspace_folder: &Rc<dyn ISearchTreeFolderMatch>,
    ) -> Rc<FolderMatchImpl> {
        let parent: Rc<dyn ISearchTreeFolderMatch> = self.self_rc();
        let folder_match = self.instantiation_service.create_instance::<FolderMatchImpl>(FolderMatchArgs::with_resource(
            resource,
            id,
            index,
            query,
            FolderMatchParent::FolderMatch(parent),
            base_workspace_folder.clone(),
        ));
        self.base.register(folder_match.clone());
        self.configure_intermediate_match(&folder_match);
        self.do_add_folder(folder_match.clone());
        folder_match
    }

    pub fn configure_intermediate_match(&self, folder_match: &Rc<FolderMatchImpl>) {
        let this = self.this.clone();
        let child = Rc::downgrade(folder_match);
        let disposable = folder_match.on_change.event().listen(move |event| {
            if let (Some(this), Some(child)) = (this.upgrade(), child.upgrade()) {
                this.on_folder_change(&child, event.clone());
            }
        });
        self.base.register(folder_match.on_dispose.event().listen(move |_| disposable.dispose()));
    }

    pub fn do_add_file(&self, file_match: Rc<dyn ISearchTreeFileMatch>) {
        let resource = file_match.resource();
        self.file_matches.borrow_mut().set(resource.clone(), file_match);
        self.un_disposed_file_matches.borrow_mut().delete(&resource);
    }

    fn uri_has_parent(&self, parent: &URI, child: &URI) -> bool {
        let ext_uri = self.uri_identity_service.ext_uri();
        ext_uri.is_equal_or_parent(child, parent, false, false) && !ext_uri.is_equal(Some(child), Some(parent), false)
    }

    fn is_in_parent_chain(&self, folder_match_id: &str) -> bool {
        if self.id == folder_match_id {
            return true;
        }
        let mut current = self.parent_ref();
        loop {
            match current {
                FolderMatchParent::FolderMatch(folder_match) => {
                    if folder_match.id() == folder_match_id {
                        return true;
                    }
                    current = folder_match.parent();
                }
                FolderMatchParent::Heading(_) => return false,
            }
        }
    }

    pub fn get_folder_match(&self, resource: &URI) -> Option<Rc<FolderMatchImpl>> {
        self.folder_matches_map.borrow().find_substr(resource).cloned()
    }

    pub fn do_add_folder(&self, folder_match: Rc<FolderMatchImpl>) {
        let folder_resource = folder_match.resource_uri();
        if let Some(resource) = &self.resource
            && !self.uri_has_parent(resource, &folder_resource)
        {
            errors::throw(Error::new(format!(
                "{} does not belong as a child of {}",
                folder_resource.to_string(false).to_string_lossy(),
                resource.to_string(false).to_string_lossy()
            )));
        } else if self.is_in_parent_chain(&folder_match.id) {
            errors::throw(Error::new(format!(
                "{} is a parent of {}",
                folder_resource.to_string(false).to_string_lossy(),
                self.resource.as_ref().map_or_else(|| "null".to_string(), |resource| resource.to_string(false).to_string_lossy())
            )));
        }

        self.folder_matches.borrow_mut().set(folder_resource.clone(), folder_match.clone());
        self.folder_matches_map.borrow_mut().set(folder_resource.clone(), folder_match);
        self.un_disposed_folder_matches.borrow_mut().delete(&folder_resource);
    }

    fn file_count(&self) -> usize {
        self.file_matches.borrow().size()
    }

    fn folder_count(&self) -> usize {
        self.folder_matches.borrow().size()
    }

    fn batch_replace(&self, matches: &[FileOrFolderMatch]) -> Promise<()> {
        let all_matches = get_file_matches(matches);

        let this = self.this.clone();
        let removed_matches = all_matches.clone();
        self.replace_service.replace_files(&all_matches, None).then(move |_| {
            if let Some(this) = this.upgrade() {
                ISearchTreeFolderMatch::do_remove_file(&*this, &removed_matches, true, true, true);
            }
        })
    }

    pub fn on_file_change(&self, file_match: &Rc<dyn ISearchTreeFileMatch>, removed: bool) {
        let mut added = false;
        let mut removed = removed;
        let has_file = self.file_matches.borrow().has(&file_match.resource());
        if !has_file {
            self.do_add_file(file_match.clone());
            added = true;
        }
        if file_match.count() == 0 {
            ISearchTreeFolderMatch::do_remove_file(self, &[file_match.clone()], false, false, false);
            added = false;
            removed = true;
        }
        if !self.replacing_all.get() {
            self.on_change.fire(IChangeEvent { elements: vec![file_match.clone()], added: Some(added), removed: Some(removed), clearing_all: None });
        }
    }

    pub fn on_folder_change(&self, folder_match: &Rc<FolderMatchImpl>, event: IChangeEvent) {
        let folder_resource = folder_match.resource_uri();
        let has_folder = self.folder_matches.borrow().has(&folder_resource);
        if !has_folder {
            self.do_add_folder(folder_match.clone());
        }
        if ISearchTreeFolderMatch::is_empty(&**folder_match) {
            self.folder_matches.borrow_mut().delete(&folder_resource);
            folder_match.dispose();
        }

        self.on_change.fire(event);
    }

    /// `contentPatternToUse`: o padrão de conteúdo da consulta (`string` ou `IPatternInfo`; aqui sempre o segundo).
    fn content_pattern_to_use(&self) -> PatternInfo {
        self.query.content_pattern.clone()
    }

    fn create_file_match(
        &self,
        parent: &Rc<FolderMatchImpl>,
        raw_file_match: &IFileMatch,
        closest_root: &Rc<dyn ISearchTreeFolderMatch>,
        search_instance_id: &str,
    ) -> Rc<NotebookCompatibleFileMatch> {
        let parent_dyn: Rc<dyn ISearchTreeFolderMatch> = parent.clone();
        // TODO do original: can probably just create FileMatchImpl if we don't expect cell results from the file.
        let file_match = self.instantiation_service.create_instance::<NotebookCompatibleFileMatch>(NotebookCompatibleFileMatchArgs {
            query: self.content_pattern_to_use(),
            preview_options: self.query.preview_options,
            max_results: self.query.common.max_results,
            parent: parent_dyn,
            raw_match: raw_file_match.clone(),
            closest_root: Some(closest_root.clone()),
            search_instance_id: search_instance_id.to_string(),
        });
        file_match.create_matches();
        let file_match_dyn: Rc<dyn ISearchTreeFileMatch> = file_match.clone();
        parent.do_add_file(file_match_dyn);
        self.listen_file_match(parent, &file_match);
        file_match
    }

    /// `fileMatch.onChange(({ didRemove }) => parent.onFileChange(fileMatch, didRemove))` e o descarte da inscrição junto com o
    /// arquivo (`this._register(fileMatch.onDispose(() => disposable.dispose()))`).
    fn listen_file_match(&self, parent: &Rc<FolderMatchImpl>, file_match: &Rc<NotebookCompatibleFileMatch>) {
        let parent = Rc::downgrade(parent);
        let target: Weak<NotebookCompatibleFileMatch> = Rc::downgrade(file_match);
        let disposable = file_match.on_change().listen(move |event| {
            if let (Some(parent), Some(target)) = (parent.upgrade(), target.upgrade()) {
                let target: Rc<dyn ISearchTreeFileMatch> = target;
                parent.on_file_change(&target, event.did_remove.unwrap_or(false));
            }
        });
        self.base.register(file_match.on_dispose().listen(move |_| disposable.dispose()));
    }

    fn normalized_uri_parent(&self, uri: &URI) -> URI {
        let ext_uri = self.uri_identity_service.ext_uri();
        ext_uri.normalize_path(&ext_uri.dirname(uri))
    }

    fn uri_equals(&self, uri1: &URI, uri2: &URI) -> bool {
        self.uri_identity_service.ext_uri().is_equal(Some(uri1), Some(uri2), false)
    }

    /// `FolderMatchWorkspaceRootImpl.createAndConfigureFileMatch`.
    fn create_and_configure_file_match_in_root(&self, raw_file_match: &IFileMatch, search_instance_id: &str) -> Rc<dyn ISearchTreeFileMatch> {
        let resource = self.resource_uri();
        if !self.uri_has_parent(&resource, &raw_file_match.resource) {
            errors::throw(Error::new(format!(
                "{} is not a descendant of {}",
                raw_file_match.resource.to_string(false).to_string_lossy(),
                resource.to_string(false).to_string_lossy()
            )));
        }

        let mut file_match_parent_parts: Vec<URI> = Vec::new();
        let mut uri = self.normalized_uri_parent(&raw_file_match.resource);

        while !self.uri_equals(&self.normalized_resource(), &uri) {
            file_match_parent_parts.insert(0, uri.clone());
            let prev_uri = uri.clone();
            uri = self.uri_identity_service.ext_uri().remove_trailing_path_separator(&self.normalized_uri_parent(&uri), None);
            if self.uri_equals(&prev_uri, &uri) {
                errors::throw(Error::new(format!(
                    "{} is not correctly configured as a child of {}",
                    raw_file_match.resource.to_string(false).to_string_lossy(),
                    self.normalized_resource().to_string(false).to_string_lossy()
                )));
            }
        }

        let this_dyn: Rc<dyn ISearchTreeFolderMatch> = self.self_rc();
        let root: Rc<dyn ISearchTreeFolderMatch> = self.closest_root().unwrap_or(this_dyn);
        let mut parent: Rc<FolderMatchImpl> = self.self_rc();
        for part in &file_match_parent_parts {
            let folder_match = match parent.get_folder_match(part) {
                Some(folder_match) => folder_match,
                None => parent.create_intermediate_folder_match(part.clone(), part.to_string(false).to_string_lossy(), -1, self.query.clone(), &root),
            };
            parent = folder_match;
        }
        let file_match = self.create_file_match(&parent, raw_file_match, &root, search_instance_id);
        file_match
    }

    /// `FolderMatchNoRootImpl.createAndConfigureFileMatch`.
    fn create_and_configure_file_match_without_root(&self, raw_file_match: &IFileMatch, search_instance_id: &str) -> Rc<dyn ISearchTreeFileMatch> {
        let parent: Rc<dyn ISearchTreeFolderMatch> = self.self_rc();
        // TODO do original: can probably just create FileMatchImpl if we don't expect cell results from the file.
        let file_match = self.instantiation_service.create_instance::<NotebookCompatibleFileMatch>(NotebookCompatibleFileMatchArgs {
            query: self.content_pattern_to_use(),
            preview_options: self.query.preview_options,
            max_results: self.query.common.max_results,
            parent,
            raw_match: raw_file_match.clone(),
            closest_root: None,
            search_instance_id: search_instance_id.to_string(),
        });
        self.base.register(file_match.clone());
        file_match.create_matches();
        let file_match_dyn: Rc<dyn ISearchTreeFileMatch> = file_match.clone();
        self.do_add_file(file_match_dyn);
        self.listen_file_match(&self.self_rc(), &file_match);
        file_match
    }

    /// `FolderMatchWorkspaceRootImpl.createAndConfigureFileMatch` ou `FolderMatchNoRootImpl.createAndConfigureFileMatch`.
    pub fn create_and_configure_file_match(&self, raw_file_match: &IFileMatch, search_instance_id: &str) -> Rc<dyn ISearchTreeFileMatch> {
        match self.kind {
            FolderMatchKind::WorkspaceRoot => self.create_and_configure_file_match_in_root(raw_file_match, search_instance_id),
            FolderMatchKind::NoRoot => self.create_and_configure_file_match_without_root(raw_file_match, search_instance_id),
            FolderMatchKind::WithResource => errors::throw(Error::new("createAndConfigureFileMatch is not available on an intermediate folder match")),
        }
    }

    fn dispose_matches(&self) {
        let file_matches: Vec<Rc<dyn ISearchTreeFileMatch>> = self.file_matches.borrow().values().cloned().collect();
        for file_match in &file_matches {
            file_match.dispose();
        }
        let folder_matches: Vec<Rc<FolderMatchImpl>> = self.folder_matches.borrow().values().cloned().collect();
        for folder_match in &folder_matches {
            folder_match.dispose_matches();
        }
        let un_disposed_file_matches: Vec<Rc<dyn ISearchTreeFileMatch>> = self.un_disposed_file_matches.borrow().values().cloned().collect();
        for file_match in &un_disposed_file_matches {
            file_match.dispose();
        }
        let un_disposed_folder_matches: Vec<Rc<FolderMatchImpl>> = self.un_disposed_folder_matches.borrow().values().cloned().collect();
        for folder_match in &un_disposed_folder_matches {
            folder_match.dispose_matches();
        }
        self.file_matches.borrow_mut().clear();
        self.folder_matches.borrow_mut().clear();
        self.un_disposed_file_matches.borrow_mut().clear();
        self.un_disposed_folder_matches.borrow_mut().clear();
    }
}

impl IDisposable for FolderMatchImpl {
    fn dispose(&self) {
        self.dispose_count.set(self.dispose_count.get() + 1);
        self.dispose_matches();
        self.on_dispose.fire(());
        self.base.dispose();
    }
}

impl ISearchTreeFolderMatch for FolderMatchImpl {
    fn on_change(&self) -> Event<IChangeEvent> {
        self.on_change.event()
    }

    fn on_dispose(&self) -> Event<()> {
        self.on_dispose.event()
    }

    fn id(&self) -> String {
        self.id.clone()
    }

    fn resource(&self) -> Option<URI> {
        self.resource.clone()
    }

    fn index(&self) -> i32 {
        self.index
    }

    fn name(&self) -> String {
        self.name.value()
    }

    fn count(&self) -> usize {
        self.file_count() + self.folder_count()
    }

    fn has_children(&self) -> bool {
        self.file_matches.borrow().size() > 0 || self.folder_matches.borrow().size() > 0
    }

    fn parent(&self) -> FolderMatchParent {
        self.parent_ref()
    }

    fn matches(&self) -> Vec<FileOrFolderMatch> {
        let mut matches: Vec<FileOrFolderMatch> = self.file_matches_iterator().into_iter().map(FileOrFolderMatch::File).collect();
        matches.extend(self.folder_matches_iterator().into_iter().map(FileOrFolderMatch::Folder));
        matches
    }

    fn all_downstream_file_matches(&self) -> Vec<Rc<dyn ISearchTreeFileMatch>> {
        let mut recursive_children: Vec<Rc<dyn ISearchTreeFileMatch>> = Vec::new();
        for folder_match in self.folder_matches_iterator() {
            recursive_children.extend(folder_match.all_downstream_file_matches());
        }

        let mut result = self.file_matches_iterator();
        result.extend(recursive_children);
        result
    }

    fn remove(&self, matches: &[FileOrFolderMatch]) {
        let all_matches = get_file_matches(matches);
        self.do_remove_file(&all_matches, true, true, false);
    }

    fn add_file_match(&self, raw: &[IFileMatch], silent: bool, search_instance_id: &str) {
        // when adding a fileMatch that has intermediate directories
        let mut added: Vec<Rc<dyn ISearchTreeFileMatch>> = Vec::new();
        let mut updated: Vec<Rc<dyn ISearchTreeFileMatch>> = Vec::new();

        for raw_file_match in raw {
            let existing_file_match = self.get_downstream_file_match(&raw_file_match.resource);
            if let Some(existing_file_match) = existing_file_match {
                if let Some(results) = &raw_file_match.results {
                    for result in results {
                        if let ITextSearchResult::Match(m) = result {
                            for text_match in text_search_result_to_matches(m, &existing_file_match, false) {
                                existing_file_match.add(text_match, false);
                            }
                        }
                    }
                }

                // add cell matches
                if is_i_notebook_file_match_with_model(raw_file_match) || is_i_notebook_file_match_no_model(raw_file_match) {
                    for raw_cell_match in raw_file_match.cell_results.iter().flatten() {
                        if let Some(notebook_file_match) = existing_file_match.as_notebook_file_match() {
                            let existing_cell_match = notebook_file_match.get_cell_match(&get_id_from_i_notebook_cell_match(raw_cell_match));
                            if let Some(existing_cell_match) = existing_cell_match {
                                existing_cell_match.add_content_matches(raw_cell_match.content_results());
                                existing_cell_match.add_webview_matches(raw_cell_match.webview_results());
                            } else {
                                notebook_file_match.add_cell_match(raw_cell_match);
                            }
                        }
                    }
                }

                updated.push(existing_file_match.clone());

                if raw_file_match.results.as_ref().is_some_and(|results| !results.is_empty()) {
                    existing_file_match.add_context(raw_file_match.results.as_deref());
                }
            } else if self.has_create_and_configure_file_match() {
                added.push(self.create_and_configure_file_match(raw_file_match, search_instance_id));
            }
        }

        let added_any = !added.is_empty();
        let mut elements = added;
        elements.extend(updated);
        if !silent && !elements.is_empty() {
            self.on_change.fire(IChangeEvent { elements, added: Some(added_any), removed: None, clearing_all: None });
        }
    }

    fn is_empty(&self) -> bool {
        self.file_count() + self.folder_count() == 0
    }

    fn clear(&self, clearing_all: bool) {
        let changed = self.all_downstream_file_matches();
        self.dispose_matches();
        self.on_change.fire(IChangeEvent { elements: changed, removed: Some(true), added: Some(false), clearing_all: Some(clearing_all) });
    }

    fn show_highlights(&self) -> bool {
        self.parent_ref().show_highlights()
    }

    fn search_model(&self) -> Rc<dyn ISearchModel> {
        match self.parent_ref() {
            FolderMatchParent::FolderMatch(folder_match) => folder_match.search_model(),
            FolderMatchParent::Heading(heading) => heading.parent().search_model(),
        }
    }

    fn query(&self) -> Option<ITextSearchQuery> {
        Some(ITextSearchQuery::Text(self.query.clone()))
    }

    fn replace(&self, match_: &Rc<dyn ISearchTreeFileMatch>) -> Promise<()> {
        let this = self.this.clone();
        let replaced = match_.clone();
        self.replace_service.replace_files(&[match_.clone()], None).then(move |_| {
            if let Some(this) = this.upgrade() {
                ISearchTreeFolderMatch::do_remove_file(&*this, &[replaced], true, true, true);
            }
        })
    }

    fn set_replacing_all(&self, replacing_all: bool) {
        self.replacing_all.set(replacing_all);
    }

    fn bind_model(&self, model: &Rc<TextModel>) {
        let model_uri = model.uri();
        let file_match = self.file_matches.borrow().get(&model_uri).cloned();

        if let Some(file_match) = file_match {
            file_match.bind_model(model);
        } else {
            let folder_match = self.get_folder_match(&model_uri);
            let match_ = folder_match.and_then(|folder_match| folder_match.get_downstream_file_match(&model_uri));
            if let Some(match_) = match_ {
                match_.bind_model(model);
            }
        }
    }

    fn get_downstream_file_match(&self, uri: &URI) -> Option<Rc<dyn ISearchTreeFileMatch>> {
        let direct_child_file_match = self.file_matches.borrow().get(uri).cloned();
        if direct_child_file_match.is_some() {
            return direct_child_file_match;
        }

        let folder_match = self.get_folder_match(uri);
        folder_match.and_then(|folder_match| folder_match.get_downstream_file_match(uri))
    }

    fn replace_all(&self) -> Promise<()> {
        let matches = self.matches();
        self.batch_replace(&matches)
    }

    fn recursive_file_count(&self) -> usize {
        self.all_downstream_file_matches().len()
    }

    fn do_remove_file(&self, file_matches: &[Rc<dyn ISearchTreeFileMatch>], dispose: bool, trigger: bool, keep_readonly: bool) {
        let mut removed: Vec<Rc<dyn ISearchTreeFileMatch>> = Vec::new();
        for match_ in file_matches {
            let resource = match_.resource();
            let is_direct_child = self.file_matches.borrow().get(&resource).is_some();
            if is_direct_child {
                if keep_readonly && match_.has_readonly_matches() {
                    continue;
                }
                self.file_matches.borrow_mut().delete(&resource);
                if dispose {
                    match_.dispose();
                } else {
                    self.un_disposed_file_matches.borrow_mut().set(resource, match_.clone());
                }
                removed.push(match_.clone());
            } else if let Some(folder) = self.get_folder_match(&resource) {
                ISearchTreeFolderMatch::do_remove_file(&*folder, &[match_.clone()], dispose, trigger, false);
            } else {
                errors::throw(Error::new(format!(
                    "FileMatch {} is not located within FolderMatch {}",
                    resource.to_string(false).to_string_lossy(),
                    self.resource.as_ref().map_or_else(|| "null".to_string(), |resource| resource.to_string(false).to_string_lossy())
                )));
            }
        }

        if trigger {
            self.on_change.fire(IChangeEvent { elements: removed, removed: Some(true), added: None, clearing_all: None });
        }
    }

    fn unbind_notebook_editor_widget(&self, editor: &Rc<dyn NotebookEditorWidget>, resource: &URI) {
        let file_match = self.file_matches.borrow().get(resource).cloned();
        if let Some(notebook_file_match) = file_match.and_then(|file_match| file_match.as_notebook_file_match()) {
            notebook_file_match.unbind_notebook_editor_widget(Some(editor));
        }
    }

    fn bind_notebook_editor_widget(&self, editor: &Rc<dyn NotebookEditorWidget>, resource: &URI) -> Promise<()> {
        let file_match = self.file_matches.borrow().get(resource).cloned();
        if let Some(notebook_file_match) = file_match.and_then(|file_match| file_match.as_notebook_file_match()) {
            notebook_file_match.bind_notebook_editor_widget(editor.clone());
            return notebook_file_match.update_matches_for_editor_widget();
        }
        Promise::resolved(())
    }

    fn has_only_read_only_matches(&self) -> bool {
        let file_matches: Vec<Rc<dyn ISearchTreeFileMatch>> = self.file_matches.borrow().values().cloned().collect();
        file_matches.iter().all(|file_match| file_match.has_only_read_only_matches())
    }

    fn file_matches_iterator(&self) -> Vec<Rc<dyn ISearchTreeFileMatch>> {
        self.file_matches.borrow().values().cloned().collect()
    }

    fn folder_matches_iterator(&self) -> Vec<Rc<dyn ISearchTreeFolderMatch>> {
        self.folder_matches
            .borrow()
            .values()
            .map(|folder_match| -> Rc<dyn ISearchTreeFolderMatch> { folder_match.clone() })
            .collect()
    }

    fn recursive_match_count(&self) -> usize {
        self.all_downstream_file_matches().iter().map(|match_| match_.count()).sum()
    }

    fn is_ai_contributed(&self) -> bool {
        false
    }

    fn closest_root(&self) -> Option<Rc<dyn ISearchTreeFolderMatch>> {
        self.closest_root.as_ref().and_then(Weak::upgrade)
    }

    fn has_create_and_configure_file_match(&self) -> bool {
        matches!(self.kind, FolderMatchKind::WorkspaceRoot | FolderMatchKind::NoRoot)
    }

    fn dispose_count_for_testing(&self) -> usize {
        self.dispose_count.get()
    }
}
