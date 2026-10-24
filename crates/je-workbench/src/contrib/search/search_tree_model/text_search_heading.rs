//! Porte de `src/vs/workbench/contrib/search/browser/searchTreeModel/textSearchHeading.ts`: o cabeçalho de uma seção de
//! resultados (`TextSearchHeadingImpl`, abstrato) e o de texto simples (`PlainTextSearchHeadingImpl`).
//!
//! Divergências:
//! - **Herança.** `TextSearchHeadingImpl<QueryType>` é o [`TextSearchHeadingImpl`] (um `Rc` compartilhado) que cada
//!   subclasse embute em `base`; a consulta é guardada como [`ITextSearchQuery`] (a união `ITextQuery | IAITextQuery`).
//!   Os membros que não variam entre as subclasses são implementados pela macro [`forward_text_search_heading_methods`];
//!   `isAIContributed` e `hidden` iniciais são parâmetros da base.
//! - `groupFilesByFolder<FileMatch extends IFileMatch>` é genérico sobre o item e a função que lê o recurso dele; o
//!   `Map<URI, FileMatch[]>` é um `ResourceMap`.
//! - `disposePastResults` (um `async` sem `await`) é uma função síncrona; `Event.once(folderMatch.onDispose)(...)` é
//!   `on_dispose().once().listen(...)`.
//! - `_parent` é um `Weak` (o `SearchResultImpl` é dono dos cabeçalhos).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::ResourceMap;
use je_base::promise::Promise;
use je_base::ternary_search_tree::TernarySearchTree;
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationService};
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use super::folder_match::FolderMatchArgs;
use super::folder_match::FolderMatchImpl;
use super::range_decorations::{HighlightResource, RangeHighlightDecorations};
use super::search_tree_common::{
    FileOrFolderMatch, GroupedFiles, IChangeEvent, IPlainTextSearchHeading, ISearchResult, ISearchTreeFileMatch, ISearchTreeFolderMatch,
    ISearchTreeMatch, ITextSearchHeading, PLAIN_TEXT_SEARCH__RESULT_ID, TEXT_SEARCH_HEADING_PREFIX, create_folder_matches_map,
};
use crate::contrib::search::replace::IReplaceService;
use crate::services::search::search::{IFileMatch, ISearchComplete, ITextQuery, ITextSearchQuery};
use crate::services::textfile::collaborators::{IProgress, ProgressStep};

/// `TextSearchHeadingImpl`.
pub struct TextSearchHeadingImpl {
    me: Weak<TextSearchHeadingImpl>,
    base: Rc<Disposable>,
    on_change: Rc<Emitter<IChangeEvent>>,
    is_dirty: Cell<bool>,
    show_highlights: Cell<bool>,
    query: RefCell<Option<ITextSearchQuery>>,
    range_highlight_decorations: Rc<RangeHighlightDecorations>,
    dispose_past_results: RefCell<Rc<dyn Fn()>>,

    pub(crate) folder_matches: RefCell<Vec<Rc<dyn ISearchTreeFolderMatch>>>,
    pub(crate) other_files_match: RefCell<Option<Rc<dyn ISearchTreeFolderMatch>>>,
    pub(crate) folder_matches_map: RefCell<TernarySearchTree<URI, Rc<dyn ISearchTreeFolderMatch>>>,
    hidden: Cell<bool>,
    cached_search_complete: RefCell<Option<ISearchComplete>>,

    allow_other_results: bool,
    is_ai_contributed: bool,
    parent: Weak<dyn ISearchResult>,
    pub(crate) instantiation_service: Rc<InstantiationService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
}

impl TextSearchHeadingImpl {
    pub fn new(
        allow_other_results: bool,
        parent: Weak<dyn ISearchResult>,
        instantiation_service: Rc<InstantiationService>,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        is_ai_contributed: bool,
        hidden: bool,
    ) -> Rc<TextSearchHeadingImpl> {
        let range_highlight_decorations = instantiation_service.create_instance::<RangeHighlightDecorations>(());
        Rc::new_cyclic(|me: &Weak<TextSearchHeadingImpl>| {
            let base = Disposable::new();
            let on_change = Emitter::<IChangeEvent>::new();
            base.register(on_change.clone());

            let dirty_target = me.clone();
            base.register(on_change.event().listen(move |event| {
                if event.removed == Some(true)
                    && let Some(heading) = dirty_target.upgrade()
                {
                    heading.is_dirty.set(!heading.is_empty());
                }
            }));

            Self {
                me: me.clone(),
                base,
                on_change,
                is_dirty: Cell::new(false),
                show_highlights: Cell::new(false),
                query: RefCell::new(None),
                range_highlight_decorations,
                dispose_past_results: RefCell::new(Rc::new(|| {})),
                folder_matches: RefCell::new(Vec::new()),
                other_files_match: RefCell::new(None),
                folder_matches_map: RefCell::new(create_folder_matches_map(&uri_identity_service)),
                hidden: Cell::new(hidden),
                cached_search_complete: RefCell::new(None),
                allow_other_results,
                is_ai_contributed,
                parent,
                instantiation_service,
                uri_identity_service,
            }
        })
    }

    pub fn on_change(&self) -> Event<IChangeEvent> {
        self.on_change.event()
    }

    /// `this._onChange.fire(event)` (o emissor é `protected`).
    pub(crate) fn fire_change(&self, event: IChangeEvent) {
        self.on_change.fire(event);
    }

    /// `this._onChange`, para os ouvintes das pastas.
    pub(crate) fn change_emitter(&self) -> Rc<Emitter<IChangeEvent>> {
        self.on_change.clone()
    }

    /// `this._register(...)`.
    pub(crate) fn register(&self, disposable: je_base::lifecycle::DisposableRef) {
        self.base.register(disposable);
    }

    pub fn hidden(&self) -> bool {
        self.hidden.get()
    }

    pub fn set_hidden(&self, hidden: bool) {
        self.hidden.set(hidden);
    }

    pub fn cached_search_complete(&self) -> Option<ISearchComplete> {
        self.cached_search_complete.borrow().clone()
    }

    pub fn set_cached_search_complete(&self, cached_search_complete: Option<ISearchComplete>) {
        *self.cached_search_complete.borrow_mut() = cached_search_complete;
    }

    pub fn hide(&self) {
        self.hidden.set(true);
        self.clear(true);
    }

    pub fn parent(&self) -> Rc<dyn ISearchResult> {
        self.parent.upgrade().expect("o resultado dono do cabeçalho está vivo")
    }

    pub fn has_children(&self) -> bool {
        !self.folder_matches.borrow().is_empty()
    }

    pub fn is_dirty(&self) -> bool {
        self.is_dirty.get()
    }

    pub fn get_folder_match(&self, resource: &URI) -> Option<Rc<dyn ISearchTreeFolderMatch>> {
        let folder_match = self.folder_matches_map.borrow().find_substr(resource).cloned();

        if folder_match.is_none() && self.allow_other_results {
            let other_files_match = self.other_files_match.borrow().clone();
            if other_files_match.is_some() {
                return other_files_match;
            }
        }
        folder_match
    }

    pub fn add(&self, all_raw: &[IFileMatch], search_instance_id: &str, silent: bool) {
        // Split up raw into a list per folder so we can do a batch add per folder.

        let GroupedFiles { by_folder, other } = self.group_files_by_folder(all_raw, |raw| raw.resource.clone());
        for raw in by_folder.values() {
            if raw.is_empty() {
                continue;
            }

            // ai results go into the respective folder
            let folder_match = self.get_folder_match(&raw[0].resource);
            if let Some(folder_match) = folder_match {
                folder_match.add_file_match(raw, silent, search_instance_id);
            }
        }

        if !self.is_ai_contributed {
            let other_files_match = self.other_files_match.borrow().clone();
            if let Some(other_files_match) = other_files_match {
                other_files_match.add_file_match(&other, silent, search_instance_id);
            }
        }
        let dispose_past_results = self.dispose_past_results.borrow().clone();
        dispose_past_results();
    }

    pub fn remove(&self, matches: &[FileOrFolderMatch], _ai: bool) {
        for m in matches {
            if let FileOrFolderMatch::Folder(folder_match) = m {
                folder_match.clear(false);
            }
        }

        let file_matches: Vec<Rc<dyn ISearchTreeFileMatch>> = matches
            .iter()
            .filter_map(|m| match m {
                FileOrFolderMatch::File(file_match) => Some(file_match.clone()),
                FileOrFolderMatch::Folder(_) => None,
            })
            .collect();

        let GroupedFiles { by_folder, other } = self.group_files_by_folder(&file_matches, |file_match| file_match.resource());
        for matches in by_folder.values() {
            if matches.is_empty() {
                continue;
            }

            if let Some(folder_match) = self.get_folder_match(&matches[0].resource()) {
                let files: Vec<FileOrFolderMatch> = matches.iter().cloned().map(FileOrFolderMatch::File).collect();
                folder_match.remove(&files);
            }
        }

        if !other.is_empty()
            && let Some(folder_match) = self.get_folder_match(&other[0].resource())
        {
            let files: Vec<FileOrFolderMatch> = other.iter().cloned().map(FileOrFolderMatch::File).collect();
            folder_match.remove(&files);
        }
    }

    pub fn group_files_by_folder<T: Clone>(&self, file_matches: &[T], resource_of: impl Fn(&T) -> URI) -> GroupedFiles<T> {
        let mut groups: Vec<(URI, Vec<T>)> = Vec::new();
        let mut index_by_key: HashMap<String, usize> = HashMap::new();
        let mut other_file_matches: Vec<T> = Vec::new();
        for folder_match in self.folder_matches.borrow().iter() {
            let resource = folder_match.resource().expect("a raiz do workspace tem recurso");
            index_by_key.insert(resource.to_string(false).to_string_lossy(), groups.len());
            groups.push((resource, Vec::new()));
        }

        for raw_file_match in file_matches {
            let Some(folder_match) = self.get_folder_match(&resource_of(raw_file_match)) else {
                // foldermatch was previously removed by user or disposed for some reason
                continue;
            };

            match folder_match.resource() {
                Some(resource) => {
                    let index = *index_by_key
                        .get(&resource.to_string(false).to_string_lossy())
                        .expect("a pasta do arquivo é uma raiz do workspace");
                    groups[index].1.push(raw_file_match.clone());
                }
                None => other_file_matches.push(raw_file_match.clone()),
            }
        }

        GroupedFiles { by_folder: ResourceMap::from_entries(groups, None), other: other_file_matches }
    }

    pub fn is_empty(&self) -> bool {
        self.folder_matches().iter().all(|folder_match| folder_match.is_empty())
    }

    pub fn find_folder_substr(&self, resource: &URI) -> Option<Rc<dyn ISearchTreeFolderMatch>> {
        self.folder_matches_map.borrow().find_substr(resource).cloned()
    }

    pub fn query(&self) -> Option<ITextSearchQuery> {
        self.query.borrow().clone()
    }

    pub(crate) fn set_query_value(&self, query: Option<ITextSearchQuery>) {
        *self.query.borrow_mut() = query;
    }

    pub(crate) fn clear_query(&self) {
        // When updating the query we could change the roots, so keep a reference to them to clean up when we trigger `disposePastResults`
        let old_folder_matches = self.folder_matches();
        let heading = self.me.clone();
        *self.dispose_past_results.borrow_mut() = Rc::new(move || {
            for folder_match in &old_folder_matches {
                folder_match.clear(false);
            }
            for folder_match in &old_folder_matches {
                folder_match.dispose();
            }
            if let Some(heading) = heading.upgrade() {
                heading.is_dirty.set(false);
            }
        });

        *self.cached_search_complete.borrow_mut() = None;

        self.range_highlight_decorations.remove_highlight_range();
        *self.folder_matches_map.borrow_mut() = create_folder_matches_map(&self.uri_identity_service);
    }

    pub fn folder_matches(&self) -> Vec<Rc<dyn ISearchTreeFolderMatch>> {
        let mut folder_matches = self.folder_matches.borrow().clone();
        if self.allow_other_results
            && let Some(other_files_match) = self.other_files_match.borrow().clone()
        {
            folder_matches.push(other_files_match);
        }
        folder_matches
    }

    fn dispose_matches(&self) {
        for folder_match in self.folder_matches() {
            folder_match.dispose();
        }

        self.folder_matches.borrow_mut().clear();

        *self.folder_matches_map.borrow_mut() = create_folder_matches_map(&self.uri_identity_service);

        self.range_highlight_decorations.remove_highlight_range();
    }

    pub fn matches(&self) -> Vec<Rc<dyn ISearchTreeFileMatch>> {
        self.folder_matches().iter().flat_map(|folder_match| folder_match.all_downstream_file_matches()).collect()
    }

    pub fn show_highlights(&self) -> bool {
        self.show_highlights.get()
    }

    pub fn toggle_highlights(&self, value: bool) {
        if self.show_highlights.get() == value {
            return;
        }
        self.show_highlights.set(value);
        let mut selected_match: Option<Rc<dyn ISearchTreeMatch>> = None;
        for file_match in self.matches() {
            file_match.update_highlights();
            if let Some(notebook_file_match) = file_match.as_notebook_file_match() {
                notebook_file_match.update_notebook_highlights();
            }
            if selected_match.is_none() {
                selected_match = file_match.get_selected_match();
            }
        }
        match selected_match {
            Some(selected_match) if self.show_highlights.get() => {
                self.range_highlight_decorations.highlight_range(HighlightResource::Uri(selected_match.parent().resource()), &selected_match.range());
            }
            _ => self.range_highlight_decorations.remove_highlight_range(),
        }
    }

    pub fn range_highlight_decorations(&self) -> Rc<RangeHighlightDecorations> {
        self.range_highlight_decorations.clone()
    }

    pub fn file_count(&self) -> usize {
        self.folder_matches().iter().map(|folder_match| folder_match.recursive_file_count()).sum()
    }

    pub fn count(&self) -> usize {
        self.matches().iter().map(|file_match| file_match.count()).sum()
    }

    pub fn clear(&self, clear_all: bool) {
        *self.cached_search_complete.borrow_mut() = None;
        for folder_match in self.folder_matches() {
            folder_match.clear(clear_all);
        }
        self.dispose_matches();
        self.folder_matches.borrow_mut().clear();
        *self.other_files_match.borrow_mut() = None;
    }

    pub fn is_ai_contributed(&self) -> bool {
        self.is_ai_contributed
    }
}

impl IDisposable for TextSearchHeadingImpl {
    fn dispose(&self) {
        self.range_highlight_decorations.dispose();
        self.dispose_matches();
        self.base.dispose();
        let dispose_past_results = self.dispose_past_results.borrow().clone();
        dispose_past_results();
    }
}

/// Implementa, no `impl ITextSearchHeading for X` de uma subclasse que guarda a base em `self.base`, os membros que
/// `TextSearchHeadingImpl` define e as subclasses não redefinem (`id`, `name`, `is_ai_contributed`, `query`, `set_query` e
/// `file_count` ficam com a subclasse).
macro_rules! forward_text_search_heading_methods {
    () => {
        fn on_change(&self) -> ::je_base::event::Event<$crate::contrib::search::search_tree_model::search_tree_common::IChangeEvent> {
            self.base.on_change()
        }
        fn resource(&self) -> Option<::je_base::uri::URI> {
            None
        }
        fn hidden(&self) -> bool {
            self.base.hidden()
        }
        fn set_hidden(&self, hidden: bool) {
            self.base.set_hidden(hidden)
        }
        fn cached_search_complete(&self) -> Option<$crate::services::search::search::ISearchComplete> {
            self.base.cached_search_complete()
        }
        fn set_cached_search_complete(&self, cached_search_complete: Option<$crate::services::search::search::ISearchComplete>) {
            self.base.set_cached_search_complete(cached_search_complete)
        }
        fn hide(&self) {
            self.base.hide()
        }
        fn parent(&self) -> ::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchResult> {
            self.base.parent()
        }
        fn has_children(&self) -> bool {
            self.base.has_children()
        }
        fn is_dirty(&self) -> bool {
            self.base.is_dirty()
        }
        fn get_folder_match(
            &self,
            resource: &::je_base::uri::URI,
        ) -> Option<::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeFolderMatch>> {
            self.base.get_folder_match(resource)
        }
        fn add(&self, all_raw: &[$crate::services::search::search::IFileMatch], search_instance_id: &str, silent: bool) {
            self.base.add(all_raw, search_instance_id, silent)
        }
        fn remove(&self, matches: &[$crate::contrib::search::search_tree_model::search_tree_common::FileOrFolderMatch], ai: bool) {
            self.base.remove(matches, ai)
        }
        fn group_files_by_folder(
            &self,
            file_matches: &[::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeFileMatch>],
        ) -> $crate::contrib::search::search_tree_model::search_tree_common::GroupedFiles<
            ::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeFileMatch>,
        > {
            self.base.group_files_by_folder(file_matches, |file_match| file_match.resource())
        }
        fn is_empty(&self) -> bool {
            self.base.is_empty()
        }
        fn find_folder_substr(
            &self,
            resource: &::je_base::uri::URI,
        ) -> Option<::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeFolderMatch>> {
            self.base.find_folder_substr(resource)
        }
        fn folder_matches(&self) -> Vec<::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeFolderMatch>> {
            self.base.folder_matches()
        }
        fn matches(&self) -> Vec<::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeFileMatch>> {
            self.base.matches()
        }
        fn show_highlights(&self) -> bool {
            self.base.show_highlights()
        }
        fn toggle_highlights(&self, value: bool) {
            self.base.toggle_highlights(value)
        }
        fn range_highlight_decorations(&self) -> ::std::rc::Rc<$crate::contrib::search::search_tree_model::range_decorations::RangeHighlightDecorations> {
            self.base.range_highlight_decorations()
        }
        fn count(&self) -> usize {
            self.base.count()
        }
        fn clear(&self, clear_all: bool) {
            self.base.clear(clear_all)
        }
    };
}

pub(crate) use forward_text_search_heading_methods;

/// `PlainTextSearchHeadingImpl`.
pub struct PlainTextSearchHeadingImpl {
    this: Weak<PlainTextSearchHeadingImpl>,
    base: Rc<TextSearchHeadingImpl>,
    replace_service: Rc<dyn IReplaceService>,
}

impl Injectable for PlainTextSearchHeadingImpl {
    type Args = Weak<dyn ISearchResult>;
    type Deps = (Rc<InstantiationService>, Rc<dyn IUriIdentityService>, Rc<dyn IReplaceService>);

    fn create(parent: Self::Args, (instantiation_service, uri_identity_service, replace_service): Self::Deps) -> Rc<Self> {
        let base = TextSearchHeadingImpl::new(true, parent, instantiation_service, uri_identity_service, false, false);
        Rc::new_cyclic(|this| Self { this: this.clone(), base, replace_service })
    }
}

impl PlainTextSearchHeadingImpl {
    fn self_rc(&self) -> Rc<PlainTextSearchHeadingImpl> {
        self.this.upgrade().expect("o cabeçalho está vivo enquanto é usado")
    }

    fn set_replacing_all(&self, running: bool) {
        for folder_match in self.base.folder_matches() {
            folder_match.set_replacing_all(running);
        }
    }

    fn set_query_impl(&self, query: Option<ITextQuery>) {
        self.base.clear_query();

        let Some(query) = query else {
            return;
        };

        let folder_matches: Vec<Rc<dyn ISearchTreeFolderMatch>> = query
            .common
            .folder_queries
            .iter()
            .map(|folder_query| &folder_query.folder)
            .enumerate()
            .map(|(index, resource)| self.create_base_folder_match(Some(resource), resource.to_string(false).to_string_lossy(), index as i32, &query))
            .collect();
        *self.base.folder_matches.borrow_mut() = folder_matches.clone();

        for folder_match in &folder_matches {
            let resource = folder_match.resource().expect("a raiz do workspace tem recurso");
            self.base.folder_matches_map.borrow_mut().set(resource, folder_match.clone());
        }

        let other_files_match = self.create_base_folder_match(None, "otherFiles".to_string(), folder_matches.len() as i32 + 1, &query);
        *self.base.other_files_match.borrow_mut() = Some(other_files_match);

        self.base.set_query_value(Some(ITextSearchQuery::Text(query)));
    }

    fn create_base_folder_match(&self, resource: Option<&URI>, id: String, index: i32, query: &ITextQuery) -> Rc<dyn ISearchTreeFolderMatch> {
        let parent: Rc<dyn ITextSearchHeading> = self.self_rc();
        let args = match resource {
            Some(resource) => FolderMatchArgs::workspace_root(resource.clone(), id, index, query.clone(), parent),
            None => FolderMatchArgs::no_root(id, index, query.clone(), parent),
        };
        let folder_match: Rc<dyn ISearchTreeFolderMatch> = self.base.instantiation_service.create_instance::<FolderMatchImpl>(args);
        let emitter = self.base.change_emitter();
        let disposable = folder_match.on_change().listen(move |event| emitter.fire(event.clone()));
        folder_match.on_dispose().once().listen(move |_| disposable.dispose());
        folder_match
    }
}

impl IDisposable for PlainTextSearchHeadingImpl {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ITextSearchHeading for PlainTextSearchHeadingImpl {
    forward_text_search_heading_methods!();

    fn is_ai_contributed(&self) -> bool {
        false
    }

    fn id(&self) -> String {
        format!("{TEXT_SEARCH_HEADING_PREFIX}{PLAIN_TEXT_SEARCH__RESULT_ID}")
    }

    fn name(&self) -> String {
        "Text".to_string()
    }

    fn file_count(&self) -> usize {
        self.base.file_count()
    }

    fn query(&self) -> Option<ITextSearchQuery> {
        self.base.query()
    }

    fn set_query(&self, query: Option<ITextSearchQuery>) {
        match query {
            None => self.set_query_impl(None),
            Some(ITextSearchQuery::Text(query)) => self.set_query_impl(Some(query)),
            Some(ITextSearchQuery::AiText(_)) => {
                je_base::errors::throw(je_base::errors::Error::new("PlainTextSearchHeadingImpl only accepts an ITextQuery"))
            }
        }
    }
}

impl IPlainTextSearchHeading for PlainTextSearchHeadingImpl {
    fn replace(&self, match_: &Rc<dyn ISearchTreeFileMatch>) -> Promise<()> {
        match self.base.get_folder_match(&match_.resource()) {
            Some(folder_match) => folder_match.replace(match_),
            None => Promise::resolved(()),
        }
    }

    fn replace_all(&self, progress: Option<&dyn IProgress<ProgressStep>>) -> Promise<()> {
        self.set_replacing_all(true);

        let promise = self.replace_service.replace_files(&self.base.matches(), progress);

        let heading = self.this.clone();
        promise.then_result(move |result| {
            if let Some(heading) = heading.upgrade() {
                heading.set_replacing_all(false);
                if result.is_ok() {
                    heading.base.clear(true);
                }
            }
            Ok(())
        })
    }
}
