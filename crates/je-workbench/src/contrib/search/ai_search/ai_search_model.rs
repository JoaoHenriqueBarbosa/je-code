//! Porte de `src/vs/workbench/contrib/search/browser/AISearch/aiSearchModel.ts`: o cabeçalho (`AITextSearchHeadingImpl`),
//! a pasta de raiz (`AIFolderMatchWorkspaceRootImpl`) e o arquivo (`AIFileMatch`) dos resultados da busca com IA.
//!
//! Divergências:
//! - **Herança.** `AITextSearchHeadingImpl extends TextSearchHeadingImpl` e `AIFileMatch extends FileMatchImpl` embutem a
//!   base em `base` (ver `text_search_heading.rs` e `file_match.rs`); a base do arquivo despacha `matches`, `removeMatch` e
//!   `setSelectedMatch` pelo [`FileMatchVirtual`], que aqui usa os da base.
//! - `AIFileMatch.name()` reproduz a precedência do original: `super.name() + range ? ' ' + range : ''` é
//!   `(super.name() + range) ? ' ' + range : ''`, que é sempre verdadeiro (sem intervalo, `range` é o texto `undefined`).
//! - `Date.now()` é `runtime::date_now()`; `new ResourceSet()` é o [`ResourceSet`] do je-base.
//! - O pai da pasta é um `Weak` do cabeçalho; `closestRoot` não existe nesta pasta (o `ISearchTreeFolderMatch` do porte o
//!   expõe como `None`).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lazy::Lazy;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::{OrderedMap, ResourceSet};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::range::Range;
use je_editor::services_model::IModelService;
use je_editor::text_model::TextModel;
use je_instantiation::{Injectable, InstantiationService};
use je_platform::label::label::ILabelService;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use super::ai_search_model_base::ISearchTreeAIFileMatch;
use crate::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;
use crate::contrib::search::replace::IReplaceService;
use crate::contrib::search::search_tree_model::file_match::{FileMatchImpl, FileMatchVirtual, forward_file_match_methods};
use crate::contrib::search::search_tree_model::r#match::text_search_result_to_matches;
use crate::contrib::search::search_tree_model::search_tree_common::{
    AI_TEXT_SEARCH_RESULT_ID, FILE_MATCH_PREFIX, FOLDER_MATCH_PREFIX, FileOrFolderMatch, FolderMatchParent, IChangeEvent, ISearchModel,
    ISearchResult, ISearchTreeFileMatch, ISearchTreeFolderMatch, ISearchTreeMatch, ITextSearchHeading, TEXT_SEARCH_HEADING_PREFIX,
    get_file_matches,
};
use crate::contrib::search::search_tree_model::text_search_heading::{TextSearchHeadingImpl, forward_text_search_heading_methods};
use crate::services::search::search::{IAITextQuery, IFileMatch, ITextSearchPreviewOptions, ITextSearchQuery, ITextSearchResult, PatternInfo};

/// `AITextSearchHeadingImpl`.
pub struct AITextSearchHeadingImpl {
    this: Weak<AITextSearchHeadingImpl>,
    base: Rc<TextSearchHeadingImpl>,
}

impl Injectable for AITextSearchHeadingImpl {
    type Args = Weak<dyn ISearchResult>;
    type Deps = (Rc<InstantiationService>, Rc<dyn IUriIdentityService>);

    fn create(parent: Self::Args, (instantiation_service, uri_identity_service): Self::Deps) -> Rc<Self> {
        let base = TextSearchHeadingImpl::new(false, parent, instantiation_service, uri_identity_service, true, true);
        Rc::new_cyclic(|this| Self { this: this.clone(), base })
    }
}

impl AITextSearchHeadingImpl {
    fn set_query_impl(&self, query: Option<IAITextQuery>) {
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
            .map(|(index, resource)| self.create_base_folder_match(resource, resource.to_string(false).to_string_lossy(), index as i32, &query))
            .collect();
        *self.base.folder_matches.borrow_mut() = folder_matches.clone();

        for folder_match in &folder_matches {
            let resource = folder_match.resource().expect("a raiz do workspace tem recurso");
            self.base.folder_matches_map.borrow_mut().set(resource, folder_match.clone());
        }

        self.base.set_query_value(Some(ITextSearchQuery::AiText(query)));
    }

    fn create_base_folder_match(&self, resource: &URI, id: String, index: i32, query: &IAITextQuery) -> Rc<dyn ISearchTreeFolderMatch> {
        let parent: Rc<dyn ITextSearchHeading> = self.this.upgrade().expect("o cabeçalho está vivo enquanto é usado");
        let folder_match = self.base.instantiation_service.create_instance::<AIFolderMatchWorkspaceRootImpl>(AIFolderMatchArgs {
            resource: resource.clone(),
            id,
            index,
            query: query.clone(),
            parent,
        });
        self.base.register(folder_match.clone());
        let emitter = self.base.change_emitter();
        let disposable = folder_match.on_change().listen(move |event| emitter.fire(event.clone()));
        self.base.register(folder_match.on_dispose().listen(move |_| disposable.dispose()));
        folder_match
    }
}

impl IDisposable for AITextSearchHeadingImpl {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ITextSearchHeading for AITextSearchHeadingImpl {
    forward_text_search_heading_methods!();

    fn is_ai_contributed(&self) -> bool {
        true
    }

    fn id(&self) -> String {
        format!("{TEXT_SEARCH_HEADING_PREFIX}{AI_TEXT_SEARCH_RESULT_ID}")
    }

    fn name(&self) -> String {
        "AI".to_string()
    }

    fn file_count(&self) -> usize {
        let mut unique_file_uris = ResourceSet::new(None);
        for folder_match in self.base.folder_matches() {
            if folder_match.is_empty() {
                continue;
            }
            for file_match in folder_match.all_downstream_file_matches() {
                unique_file_uris.add(file_match.resource());
            }
        }

        unique_file_uris.size()
    }

    fn query(&self) -> Option<ITextSearchQuery> {
        self.base.query()
    }

    fn set_query(&self, query: Option<ITextSearchQuery>) {
        match query {
            None => self.set_query_impl(None),
            Some(ITextSearchQuery::AiText(query)) => self.set_query_impl(Some(query)),
            Some(ITextSearchQuery::Text(_)) => errors::throw(Error::new("AITextSearchHeadingImpl only accepts an IAITextQuery")),
        }
    }
}

/// Os argumentos estáticos de `AIFolderMatchWorkspaceRootImpl` (`_resource`, `_id`, `_index`, `_query` e `_parent`).
pub struct AIFolderMatchArgs {
    pub resource: URI,
    pub id: String,
    pub index: i32,
    pub query: IAITextQuery,
    pub parent: Rc<dyn ITextSearchHeading>,
}

/// `AIFolderMatchWorkspaceRootImpl`.
pub struct AIFolderMatchWorkspaceRootImpl {
    this: Weak<AIFolderMatchWorkspaceRootImpl>,
    base: Rc<Disposable>,
    on_change: Rc<Emitter<IChangeEvent>>,
    on_dispose: Rc<Emitter<()>>,
    id: String,
    name: Lazy<String>,
    /// id to fileMatch
    un_disposed_file_matches: RefCell<OrderedMap<String, Rc<dyn ISearchTreeFileMatch>>>,
    /// id to fileMatch
    file_matches: RefCell<OrderedMap<String, Rc<dyn ISearchTreeFileMatch>>>,
    resource: URI,
    index: i32,
    query: IAITextQuery,
    parent: Weak<dyn ITextSearchHeading>,
    instantiation_service: Rc<InstantiationService>,
    latest_rank: Cell<i64>,
    replacing_all: Cell<bool>,
    dispose_count: Cell<usize>,
}

impl Injectable for AIFolderMatchWorkspaceRootImpl {
    type Args = AIFolderMatchArgs;
    type Deps = (Rc<InstantiationService>, Rc<dyn ILabelService>);

    fn create(args: AIFolderMatchArgs, (instantiation_service, label_service): Self::Deps) -> Rc<Self> {
        let AIFolderMatchArgs { resource, id, index, query, parent } = args;
        Rc::new_cyclic(|this: &Weak<AIFolderMatchWorkspaceRootImpl>| {
            let base = Disposable::new();
            let on_change = Emitter::<IChangeEvent>::new();
            base.register(on_change.clone());
            let on_dispose = Emitter::<()>::new();
            base.register(on_dispose.clone());
            let name_resource = resource.clone();
            Self {
                this: this.clone(),
                base,
                on_change,
                on_dispose,
                id: format!("{FOLDER_MATCH_PREFIX}{id}"),
                name: Lazy::new(move || label_service.get_uri_basename_label(&name_resource)),
                un_disposed_file_matches: RefCell::new(OrderedMap::new()),
                file_matches: RefCell::new(OrderedMap::new()),
                resource,
                index,
                query,
                parent: Rc::downgrade(&parent),
                instantiation_service,
                latest_rank: Cell::new(0),
                replacing_all: Cell::new(false),
                dispose_count: Cell::new(0),
            }
        })
    }
}

impl AIFolderMatchWorkspaceRootImpl {
    fn self_rc(&self) -> Rc<AIFolderMatchWorkspaceRootImpl> {
        self.this.upgrade().expect("a pasta está viva enquanto é usada")
    }

    fn parent_heading(&self) -> Rc<dyn ITextSearchHeading> {
        self.parent.upgrade().expect("o cabeçalho pai está vivo")
    }

    fn do_add_file(&self, file_match: Rc<dyn ISearchTreeFileMatch>) {
        self.file_matches.borrow_mut().insert(file_match.id(), file_match);
    }

    pub fn create_and_configure_file_match(&self, raw_file_match: &IFileMatch, _search_instance_id: &str) -> Rc<AIFileMatch> {
        let rank = self.latest_rank.get();
        self.latest_rank.set(rank + 1);
        let this_dyn: Rc<dyn ISearchTreeFolderMatch> = self.self_rc();
        let file_match = self.instantiation_service.create_instance::<AIFileMatch>(AIFileMatchArgs {
            query: self.query.content_pattern.to_string_lossy(),
            preview_options: self.query.preview_options,
            max_results: self.query.common.max_results,
            parent: this_dyn.clone(),
            raw_match: raw_file_match.clone(),
            closest_root: Some(this_dyn),
            id: format!("{}_{}", raw_file_match.resource.to_string(false).to_string_lossy(), runtime::date_now()),
            rank,
        });
        file_match.create_matches();
        let file_match_dyn: Rc<dyn ISearchTreeFileMatch> = file_match.clone();
        self.do_add_file(file_match_dyn);
        let this = self.this.clone();
        let target: Weak<AIFileMatch> = Rc::downgrade(&file_match);
        let disposable = file_match.on_change().listen(move |event| {
            if let (Some(this), Some(target)) = (this.upgrade(), target.upgrade()) {
                let target: Rc<dyn ISearchTreeFileMatch> = target;
                this.on_file_change(&target, event.did_remove.unwrap_or(false));
            }
        });
        self.base.register(file_match.on_dispose().listen(move |_| disposable.dispose()));
        file_match
    }

    fn on_file_change(&self, file_match: &Rc<dyn ISearchTreeFileMatch>, removed: bool) {
        let mut added = false;
        let mut removed = removed;
        let has_file = self.file_matches.borrow().contains_key(&file_match.id());
        if !has_file {
            self.do_add_file(file_match.clone());
            added = true;
        }
        if file_match.count() == 0 {
            self.do_remove_file(&[file_match.clone()], false, false, false);
            added = false;
            removed = true;
        }
        self.on_change.fire(IChangeEvent { elements: vec![file_match.clone()], added: Some(added), removed: Some(removed), clearing_all: None });
    }

    fn dispose_matches(&self) {
        let file_matches: Vec<Rc<dyn ISearchTreeFileMatch>> = self.file_matches.borrow().values().cloned().collect();
        for file_match in &file_matches {
            file_match.dispose();
        }
        let un_disposed_file_matches: Vec<Rc<dyn ISearchTreeFileMatch>> = self.un_disposed_file_matches.borrow().values().cloned().collect();
        for file_match in &un_disposed_file_matches {
            file_match.dispose();
        }
        self.file_matches.borrow_mut().clear();
    }
}

impl IDisposable for AIFolderMatchWorkspaceRootImpl {
    fn dispose(&self) {
        self.dispose_count.set(self.dispose_count.get() + 1);
        self.dispose_matches();
        self.on_dispose.fire(());
        self.base.dispose();
    }
}

impl ISearchTreeFolderMatch for AIFolderMatchWorkspaceRootImpl {
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
        Some(self.resource.clone())
    }

    fn index(&self) -> i32 {
        self.index
    }

    fn name(&self) -> String {
        self.name.value()
    }

    fn count(&self) -> usize {
        self.file_matches.borrow().len()
    }

    fn has_children(&self) -> bool {
        !self.file_matches.borrow().is_empty()
    }

    fn parent(&self) -> FolderMatchParent {
        FolderMatchParent::Heading(self.parent_heading())
    }

    fn matches(&self) -> Vec<FileOrFolderMatch> {
        self.file_matches.borrow().values().cloned().map(FileOrFolderMatch::File).collect()
    }

    fn all_downstream_file_matches(&self) -> Vec<Rc<dyn ISearchTreeFileMatch>> {
        self.file_matches.borrow().values().cloned().collect()
    }

    fn remove(&self, matches: &[FileOrFolderMatch]) {
        let all_matches = get_file_matches(matches);
        self.do_remove_file(&all_matches, true, true, false);
    }

    fn add_file_match(&self, raw: &[IFileMatch], silent: bool, search_instance_id: &str) {
        // when adding a fileMatch that has intermediate directories
        let mut added: Vec<Rc<dyn ISearchTreeFileMatch>> = Vec::new();
        let updated: Vec<Rc<dyn ISearchTreeFileMatch>> = Vec::new();

        for raw_file_match in raw {
            let file_match: Rc<dyn ISearchTreeFileMatch> = self.create_and_configure_file_match(raw_file_match, search_instance_id);
            added.push(file_match);
        }

        let added_any = !added.is_empty();
        let mut elements = added;
        elements.extend(updated);
        if !silent && !elements.is_empty() {
            self.on_change.fire(IChangeEvent { elements, added: Some(added_any), removed: None, clearing_all: None });
        }
    }

    fn is_empty(&self) -> bool {
        self.recursive_file_count() == 0
    }

    fn clear(&self, clearing_all: bool) {
        let changed = self.all_downstream_file_matches();
        if !changed.is_empty() {
            self.dispose_matches();
            self.on_change.fire(IChangeEvent { elements: changed, removed: Some(true), added: Some(false), clearing_all: Some(clearing_all) });
        }
    }

    fn show_highlights(&self) -> bool {
        self.parent_heading().show_highlights()
    }

    fn search_model(&self) -> Rc<dyn ISearchModel> {
        self.parent_heading().parent().search_model()
    }

    fn query(&self) -> Option<ITextSearchQuery> {
        Some(ITextSearchQuery::AiText(self.query.clone()))
    }

    fn replace(&self, _match: &Rc<dyn ISearchTreeFileMatch>) -> Promise<()> {
        errors::throw(Error::new("Cannot replace in AI search"))
    }

    fn set_replacing_all(&self, replacing_all: bool) {
        self.replacing_all.set(replacing_all);
    }

    fn bind_model(&self, _model: &Rc<TextModel>) {
        // no op
    }

    fn get_downstream_file_match(&self, uri: &URI) -> Option<Rc<dyn ISearchTreeFileMatch>> {
        let uri_text = uri.to_string(false);
        self.file_matches.borrow().values().find(|file_match| file_match.resource().to_string(false) == uri_text).cloned()
    }

    fn replace_all(&self) -> Promise<()> {
        errors::throw(Error::new("Cannot replace in AI search"))
    }

    fn recursive_file_count(&self) -> usize {
        self.file_matches.borrow().len()
    }

    fn do_remove_file(&self, file_matches: &[Rc<dyn ISearchTreeFileMatch>], dispose: bool, trigger: bool, keep_readonly: bool) {
        let mut removed: Vec<Rc<dyn ISearchTreeFileMatch>> = Vec::new();
        for match_ in file_matches {
            let is_known = self.file_matches.borrow().contains_key(&match_.id());
            if is_known {
                if keep_readonly && match_.has_readonly_matches() {
                    continue;
                }
                self.file_matches.borrow_mut().remove(&match_.id());
                if dispose {
                    match_.dispose();
                } else {
                    self.un_disposed_file_matches.borrow_mut().insert(match_.id(), match_.clone());
                }
                removed.push(match_.clone());
            }
        }

        if trigger {
            self.on_change.fire(IChangeEvent { elements: removed, removed: Some(true), added: None, clearing_all: None });
        }
    }

    fn unbind_notebook_editor_widget(&self, _editor: &Rc<dyn NotebookEditorWidget>, _resource: &URI) {
        // no op
    }

    fn bind_notebook_editor_widget(&self, _editor: &Rc<dyn NotebookEditorWidget>, _resource: &URI) -> Promise<()> {
        // no op
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
        Vec::new()
    }

    fn recursive_match_count(&self) -> usize {
        self.file_matches.borrow().len()
    }

    fn is_ai_contributed(&self) -> bool {
        true
    }

    fn closest_root(&self) -> Option<Rc<dyn ISearchTreeFolderMatch>> {
        None
    }

    fn has_create_and_configure_file_match(&self) -> bool {
        true
    }

    fn dispose_count_for_testing(&self) -> usize {
        self.dispose_count.get()
    }
}

/// Os argumentos estáticos de `AIFileMatch` (`_query`, `_previewOptions`, `_maxResults`, `_parent`, `rawMatch`,
/// `_closestRoot`, `_id` e `rank`).
pub struct AIFileMatchArgs {
    pub query: String,
    pub preview_options: Option<ITextSearchPreviewOptions>,
    pub max_results: Option<i64>,
    pub parent: Rc<dyn ISearchTreeFolderMatch>,
    pub raw_match: IFileMatch,
    pub closest_root: Option<Rc<dyn ISearchTreeFolderMatch>>,
    pub id: String,
    pub rank: i64,
}

/// `AIFileMatch`.
pub struct AIFileMatch {
    this: Weak<AIFileMatch>,
    base: Rc<FileMatchImpl>,
    id: String,
    rank: i64,
}

impl Injectable for AIFileMatch {
    type Args = AIFileMatchArgs;
    type Deps = (Rc<dyn IModelService>, Rc<dyn IReplaceService>, Rc<dyn ILabelService>);

    fn create(args: AIFileMatchArgs, (model_service, replace_service, label_service): Self::Deps) -> Rc<Self> {
        let AIFileMatchArgs { query, preview_options, max_results, parent, raw_match, closest_root, id, rank } = args;
        Rc::new_cyclic(|this: &Weak<AIFileMatch>| {
            let outer: Weak<dyn ISearchTreeFileMatch> = this.clone();
            let virtual_members: Weak<dyn FileMatchVirtual> = this.clone();
            let base = FileMatchImpl::new(
                PatternInfo::new(query),
                preview_options,
                max_results,
                &parent,
                raw_match,
                closest_root.as_ref(),
                model_service,
                replace_service,
                label_service,
                outer,
                virtual_members,
            );
            Self { this: this.clone(), base, id, rank }
        })
    }
}

impl AIFileMatch {
    fn range_as_string(&self) -> Option<String> {
        let range = ISearchTreeAIFileMatch::get_full_range(self)?;
        Some(format!("{}:{}-{}:{}", range.start_line_number, range.start_column, range.end_line_number, range.end_column))
    }

    pub fn create_matches(&self) {
        if let Some(results) = &self.base.raw_match.results {
            let outer = self.base.outer();
            for result in results {
                if let ITextSearchResult::Match(raw_match) = result {
                    for match_ in text_search_result_to_matches(raw_match, &outer, true) {
                        self.base.add(match_, false);
                    }
                }
            }
        }
    }
}

impl FileMatchVirtual for AIFileMatch {
    fn matches(&self) -> Vec<Rc<dyn ISearchTreeMatch>> {
        self.base.super_matches()
    }

    fn remove_match(&self, match_: &Rc<dyn ISearchTreeMatch>) {
        self.base.super_remove_match(match_)
    }

    fn set_selected_match(&self, match_: Option<Rc<dyn ISearchTreeMatch>>) {
        self.base.super_set_selected_match(match_)
    }
}

impl IDisposable for AIFileMatch {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ISearchTreeFileMatch for AIFileMatch {
    forward_file_match_methods!();

    fn id(&self) -> String {
        format!("{FILE_MATCH_PREFIX}{}", self.id)
    }

    fn name(&self) -> String {
        // `super.name() + range ? ' ' + range : ''`: ver as divergências do módulo.
        let range = self.range_as_string();
        format!(" {}", range.as_deref().unwrap_or("undefined"))
    }

    fn has_children(&self) -> bool {
        self.base.has_children()
    }

    fn matches(&self) -> Vec<Rc<dyn ISearchTreeMatch>> {
        self.base.matches()
    }

    fn set_selected_match(&self, match_: Option<Rc<dyn ISearchTreeMatch>>) {
        FileMatchVirtual::set_selected_match(self, match_)
    }
}

impl ISearchTreeAIFileMatch for AIFileMatch {
    fn get_full_range(&self) -> Option<Range> {
        let mut earliest_start: Option<(i32, i32)> = None;
        let mut latest_end: Option<(i32, i32)> = None;

        for match_ in self.base.matches() {
            let range = match_.range();
            let match_start = (range.start_line_number, range.start_column);
            let match_end = (range.end_line_number, range.end_column);
            match earliest_start {
                None => earliest_start = Some(match_start),
                Some(earliest) if match_start < earliest => earliest_start = Some(match_start),
                Some(_) => {}
            }

            match latest_end {
                None => latest_end = Some(match_end),
                Some(latest) if match_end >= latest => latest_end = Some(match_end),
                Some(_) => {}
            }
        }

        let (earliest_start, latest_end) = (earliest_start?, latest_end?);
        Some(Range::new(earliest_start.0, earliest_start.1, latest_end.0, latest_end.1))
    }

    fn rank(&self) -> i64 {
        self.rank
    }
}
