//! Porte de `src/vs/workbench/contrib/search/browser/searchTreeModel/fileMatch.ts`: o arquivo com casamentos de uma
//! busca de texto (`FileMatchImpl`), as decorações dos casamentos no modelo aberto e a substituição serializada.
//!
//! Divergências:
//! - **Herança.** `FileMatchImpl` é o estado compartilhado (um `Rc`) que cada subclasse (`NotebookCompatibleFileMatch`,
//!   `AIFileMatch`) embute no campo `base` e que implementa o [`ISearchTreeFileMatch`] repassando os métodos pela macro
//!   [`forward_file_match_methods`]; nenhuma instância pura de `FileMatchImpl` existe no produto (as pastas criam sempre o
//!   `NotebookCompatibleFileMatch`). Os membros que as subclasses redefinem e que a própria base chama (`matches`,
//!   `removeMatch`, `setSelectedMatch`) são despachados por [`FileMatchVirtual`]; `this` das subclasses é o `outer`.
//! - `protected` é `pub(crate)`. `updatesMatchesForLineAfterReplace` (um `async` sem `await`) é síncrono.
//! - O pai (`_parent`) e a raiz mais próxima (`_closestRoot`) são `Weak` (a pasta é dona do arquivo).
//! - As opções de decoração estáticas (`ModelDecorationOptions.register`) são `thread_local`; as cores
//!   (`themeColorFromId(...)`) são [`DecorationColor::Theme`] com os ids de `overviewRulerFindMatchForeground`
//!   (`editorOverviewRuler.findMatchForeground`) e `minimapFindMatch` (`minimap.findMatchHighlight`).
//! - `Lazy<string>` do nome é o [`Lazy`] do je-base; o `Map<number, string>` do contexto é um [`OrderedMap`].
//! - `FindMatchDecorationModel` é o contrato [`FindMatchDecorationModel`] (criado pelo widget do notebook).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use je_base::async_::RunOnceScheduler;
use je_base::js_string::JsString;
use je_base::lazy::Lazy;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::map::OrderedMap;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::model::{
    DecorationColor, FindMatch, MinimapPosition, ModelDecorationMinimapInit, ModelDecorationOptionsInit, ModelDecorationOverviewRulerInit,
    ModelDeltaDecoration, OverviewRulerLane, TrackedRangeStickiness,
};
use je_editor::services_model::IModelService;
use je_editor::text_model::{FindScope, ModelDecorationOptions, TextModel};
use je_platform::files::files::FileStatWithMetadata;
use je_platform::files::i_file_service::IFileService;
use je_platform::label::label::ILabelService;
use je_base::event::{Emitter, Event};

use super::r#match::text_search_result_to_matches;
use super::search_tree_common::{FILE_MATCH_PREFIX, FileMatchChangeEvent, ISearchTreeFileMatch, ISearchTreeFolderMatch, ISearchTreeMatch};
use crate::contrib::notebook::browser::contrib::find::find_match_decoration_model::FindMatchDecorationModel;
use crate::contrib::search::replace::IReplaceService;
use crate::services::search::search::{DEFAULT_MAX_SEARCH_RESULTS, IFileMatch, ITextSearchPreviewOptions, ITextSearchResult, PatternInfo};
use crate::services::search::search_helpers::{editor_matches_to_text_search_results, get_text_search_match_with_model_context};

/// `themeColorFromId(overviewRulerFindMatchForeground)`.
const OVERVIEW_RULER_FIND_MATCH_FOREGROUND: &str = "editorOverviewRuler.findMatchForeground";
/// `themeColorFromId(minimapFindMatch)`.
const MINIMAP_FIND_MATCH: &str = "minimap.findMatchHighlight";

fn overview_ruler_find_match() -> ModelDecorationOverviewRulerInit {
    ModelDecorationOverviewRulerInit {
        color: Some(DecorationColor::Theme(OVERVIEW_RULER_FIND_MATCH_FOREGROUND.to_owned())),
        dark_color: None,
        position: Some(OverviewRulerLane::Center),
    }
}

fn minimap_find_match() -> ModelDecorationMinimapInit {
    ModelDecorationMinimapInit {
        color: Some(DecorationColor::Theme(MINIMAP_FIND_MATCH.to_owned())),
        dark_color: None,
        position: MinimapPosition::Inline,
        section_header_style: None,
        section_header_text: None,
    }
}

thread_local! {
    /// `_CURRENT_FIND_MATCH`.
    static CURRENT_FIND_MATCH: Rc<ModelDecorationOptions> = ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: "search-current-find-match".to_owned(),
        stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
        z_index: 13,
        class_name: Some("currentFindMatch".to_owned()),
        inline_class_name: Some("currentFindMatchInline".to_owned()),
        overview_ruler: Some(overview_ruler_find_match()),
        minimap: Some(minimap_find_match()),
        ..ModelDecorationOptionsInit::default()
    });

    /// `_FIND_MATCH`.
    static FIND_MATCH: Rc<ModelDecorationOptions> = ModelDecorationOptions::register(&ModelDecorationOptionsInit {
        description: "search-find-match".to_owned(),
        stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
        class_name: Some("findMatch".to_owned()),
        inline_class_name: Some("findMatchInline".to_owned()),
        overview_ruler: Some(overview_ruler_find_match()),
        minimap: Some(minimap_find_match()),
        ..ModelDecorationOptionsInit::default()
    });
}

/// `getDecorationOption(selected)`.
fn get_decoration_option(selected: bool) -> Rc<ModelDecorationOptions> {
    if selected { CURRENT_FIND_MATCH.with(Rc::clone) } else { FIND_MATCH.with(Rc::clone) }
}

/// Os membros de `FileMatchImpl` que as subclasses redefinem e que a base chama.
pub trait FileMatchVirtual {
    fn matches(&self) -> Vec<Rc<dyn ISearchTreeMatch>>;
    fn remove_match(&self, match_: &Rc<dyn ISearchTreeMatch>);
    fn set_selected_match(&self, match_: Option<Rc<dyn ISearchTreeMatch>>);
}

/// `FileMatchImpl`.
pub struct FileMatchImpl {
    me: Weak<FileMatchImpl>,
    base: Rc<Disposable>,
    /// O objeto mais externo (`this` da subclasse).
    outer: Weak<dyn ISearchTreeFileMatch>,
    virtual_members: Weak<dyn FileMatchVirtual>,

    pub(crate) find_match_decoration_model: RefCell<Option<Rc<dyn FindMatchDecorationModel>>>,

    on_change: Rc<Emitter<FileMatchChangeEvent>>,
    on_dispose: Rc<Emitter<()>>,

    pub(crate) query: PatternInfo,
    preview_options: Option<ITextSearchPreviewOptions>,
    max_results: Option<i64>,
    parent: Weak<dyn ISearchTreeFolderMatch>,
    pub(crate) raw_match: IFileMatch,
    closest_root: Option<Weak<dyn ISearchTreeFolderMatch>>,
    pub(crate) model_service: Rc<dyn IModelService>,
    replace_service: Rc<dyn IReplaceService>,

    pub(crate) resource: URI,
    file_stat: RefCell<Option<FileStatWithMetadata>>,
    model: RefCell<Option<Rc<TextModel>>>,
    model_listener: RefCell<Option<Rc<DisposableStore>>>,
    pub(crate) text_matches: RefCell<OrderedMap<String, Rc<dyn ISearchTreeMatch>>>,

    removed_text_matches: RefCell<HashSet<String>>,
    pub(crate) selected_match: RefCell<Option<Rc<dyn ISearchTreeMatch>>>,
    name: Lazy<String>,

    update_scheduler: Rc<RunOnceScheduler>,
    model_decorations: RefCell<Vec<String>>,

    context: RefCell<OrderedMap<i64, JsString>>,
    replace_q: RefCell<Promise<()>>,
}

impl FileMatchImpl {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        query: PatternInfo,
        preview_options: Option<ITextSearchPreviewOptions>,
        max_results: Option<i64>,
        parent: &Rc<dyn ISearchTreeFolderMatch>,
        raw_match: IFileMatch,
        closest_root: Option<&Rc<dyn ISearchTreeFolderMatch>>,
        model_service: Rc<dyn IModelService>,
        replace_service: Rc<dyn IReplaceService>,
        label_service: Rc<dyn ILabelService>,
        outer: Weak<dyn ISearchTreeFileMatch>,
        virtual_members: Weak<dyn FileMatchVirtual>,
    ) -> Rc<FileMatchImpl> {
        Rc::new_cyclic(|me: &Weak<FileMatchImpl>| {
            let base = Disposable::new();
            let on_change = Emitter::<FileMatchChangeEvent>::new();
            base.register(on_change.clone());
            let on_dispose = Emitter::<()>::new();
            base.register(on_dispose.clone());

            let scheduler_target = me.clone();
            let update_scheduler = RunOnceScheduler::new(
                move || {
                    if let Some(file_match) = scheduler_target.upgrade() {
                        file_match.update_matches_for_model();
                    }
                },
                250,
            );
            base.register(update_scheduler.clone());

            let resource = raw_match.resource.clone();
            let label_resource = resource.clone();
            Self {
                me: me.clone(),
                base,
                outer,
                virtual_members,
                find_match_decoration_model: RefCell::new(None),
                on_change,
                on_dispose,
                query,
                preview_options,
                max_results,
                parent: Rc::downgrade(parent),
                raw_match,
                closest_root: closest_root.map(Rc::downgrade),
                model_service,
                replace_service,
                resource,
                file_stat: RefCell::new(None),
                model: RefCell::new(None),
                model_listener: RefCell::new(None),
                text_matches: RefCell::new(OrderedMap::new()),
                removed_text_matches: RefCell::new(HashSet::new()),
                selected_match: RefCell::new(None),
                name: Lazy::new(move || label_service.get_uri_basename_label(&label_resource)),
                update_scheduler,
                model_decorations: RefCell::new(Vec::new()),
                context: RefCell::new(OrderedMap::new()),
                replace_q: RefCell::new(Promise::resolved(())),
            }
        })
    }

    /// `this`, como a subclasse (o objeto que a árvore enxerga).
    pub(crate) fn outer(&self) -> Rc<dyn ISearchTreeFileMatch> {
        self.outer.upgrade().expect("o arquivo está vivo enquanto a base é usada")
    }

    pub fn on_change(&self) -> Event<FileMatchChangeEvent> {
        self.on_change.event()
    }

    pub(crate) fn fire_change(&self, event: FileMatchChangeEvent) {
        self.on_change.fire(event);
    }

    /// `this._register(...)`.
    pub(crate) fn register(&self, disposable: je_base::lifecycle::DisposableRef) {
        self.base.register(disposable);
    }

    pub fn on_dispose(&self) -> Event<()> {
        self.on_dispose.event()
    }

    pub fn context(&self) -> OrderedMap<i64, JsString> {
        self.context.borrow().clone()
    }

    pub fn closest_root(&self) -> Option<Rc<dyn ISearchTreeFolderMatch>> {
        self.closest_root.as_ref().and_then(Weak::upgrade)
    }

    pub fn has_readonly_matches(&self) -> bool {
        self.matches().iter().any(|m| m.is_readonly())
    }

    pub fn create_matches(&self) {
        let model = self.model_service.get_model(&self.resource);
        if let Some(model) = model {
            // todo: handle better when ai contributed results has model, currently, createMatches does not work for this
            self.bind_model(&model);
            self.update_matches_for_model();
        } else if let Some(results) = &self.raw_match.results {
            let outer = self.outer();
            for result in results {
                if let ITextSearchResult::Match(raw_match) = result {
                    for m in text_search_result_to_matches(raw_match, &outer, false) {
                        self.add(m, false);
                    }
                }
            }
        }
    }

    pub fn bind_model(&self, model: &Rc<TextModel>) {
        *self.model.borrow_mut() = Some(model.clone());
        let listener = DisposableStore::new();
        let on_content_change_target = self.me.clone();
        listener.add(model.on_did_change_content().listen(move |_| {
            if let Some(file_match) = on_content_change_target.upgrade() {
                file_match.update_scheduler.schedule(None);
            }
        }));
        let on_will_dispose_target = self.me.clone();
        listener.add(model.on_will_dispose().listen(move |_| {
            if let Some(file_match) = on_will_dispose_target.upgrade() {
                file_match.on_model_will_dispose();
            }
        }));
        *self.model_listener.borrow_mut() = Some(listener);
        self.update_highlights();
    }

    fn on_model_will_dispose(&self) {
        // Update matches because model might have some dirty changes
        self.update_matches_for_model();
        self.unbind_model();
    }

    fn unbind_model(&self) {
        let model = self.model.borrow().clone();
        if let Some(model) = model {
            self.update_scheduler.cancel();
            let old_decorations = self.model_decorations.borrow().clone();
            if let Some(decorations) = model.change_decorations(|accessor| accessor.delta_decorations(&old_decorations, &[]), 0) {
                *self.model_decorations.borrow_mut() = decorations;
            }
            *self.model.borrow_mut() = None;
            let listener = self.model_listener.borrow().clone();
            if let Some(listener) = listener {
                listener.dispose();
            }
        }
    }

    /// `wordSeparators` da consulta: só vale com `isWordMatch`.
    fn word_separators(&self) -> Option<String> {
        if self.query.is_word_match.unwrap_or(false) {
            self.query.word_separators.as_ref().filter(|separators| !separators.is_empty()).map(JsString::to_string_lossy)
        } else {
            None
        }
    }

    fn max_results_limit(&self) -> usize {
        self.max_results.and_then(|max_results| usize::try_from(max_results).ok()).unwrap_or(DEFAULT_MAX_SEARCH_RESULTS)
    }

    pub(crate) fn update_matches_for_model(&self) {
        // this is called from a timeout and might fire
        // after the model has been disposed
        let Some(model) = self.model.borrow().clone() else {
            return;
        };
        *self.text_matches.borrow_mut() = OrderedMap::new();

        let word_separators = self.word_separators();
        let matches = model.find_matches(
            self.query.pattern.clone(),
            FindScope::Ranges(vec![model.get_full_model_range()]),
            self.query.is_reg_exp.unwrap_or(false),
            self.query.is_case_sensitive.unwrap_or(false),
            word_separators.as_deref(),
            false,
            self.max_results_limit(),
        );

        self.update_matches(&matches, true, &model, false);
    }

    pub(crate) fn updates_matches_for_line_after_replace(&self, line_number: i32, model_change: bool) {
        let Some(model) = self.model.borrow().clone() else {
            return;
        };
        let range = je_editor::range::Range::new(
            line_number,
            CursorSimpleModel::get_line_min_column(model.as_ref(), line_number),
            line_number,
            CursorSimpleModel::get_line_max_column(model.as_ref(), line_number),
        );
        let old_matches: Vec<Rc<dyn ISearchTreeMatch>> =
            self.text_matches.borrow().values().filter(|m| m.range().start_line_number == line_number).cloned().collect();
        for old_match in &old_matches {
            self.text_matches.borrow_mut().remove(&old_match.id());
        }

        let word_separators = self.word_separators();
        let matches = model.find_matches(
            self.query.pattern.clone(),
            FindScope::Ranges(vec![range]),
            self.query.is_reg_exp.unwrap_or(false),
            self.query.is_case_sensitive.unwrap_or(false),
            word_separators.as_deref(),
            false,
            self.max_results_limit(),
        );
        self.update_matches(&matches, model_change, &model, false);
    }

    fn update_matches(&self, matches: &[FindMatch], model_change: bool, model: &Rc<TextModel>, is_ai_contributed: bool) {
        let text_search_results = editor_matches_to_text_search_results(matches, model.as_ref(), self.preview_options.as_ref());
        let outer = self.outer();
        for text_search_result in &text_search_results {
            for m in text_search_result_to_matches(text_search_result, &outer, is_ai_contributed) {
                let was_removed = self.removed_text_matches.borrow().contains(&m.id());
                if !was_removed {
                    self.add(m.clone(), false);
                    if self.is_match_selected(&m) {
                        *self.selected_match.borrow_mut() = Some(m);
                    }
                }
            }
        }

        let query = self.parent().parent().query().expect("a consulta existe enquanto há resultados");
        let context = get_text_search_match_with_model_context(&text_search_results, model.as_ref(), &query);
        self.add_context(Some(&context));

        self.on_change.fire(FileMatchChangeEvent { did_remove: None, force_update_model: Some(model_change) });
        self.update_highlights();
    }

    pub fn update_highlights(&self) {
        let Some(model) = self.model.borrow().clone() else {
            return;
        };

        let show_highlights = self.parent().show_highlights();
        let new_decorations: Vec<ModelDeltaDecoration> = if show_highlights {
            self.matches()
                .iter()
                .map(|m| ModelDeltaDecoration { range: m.range(), options: get_decoration_option(self.is_match_selected(m)) })
                .collect()
        } else {
            Vec::new()
        };
        let old_decorations = self.model_decorations.borrow().clone();
        if let Some(decorations) = model.change_decorations(|accessor| accessor.delta_decorations(&old_decorations, &new_decorations), 0) {
            *self.model_decorations.borrow_mut() = decorations;
        }
    }

    pub fn id(&self) -> String {
        format!("{}{}", FILE_MATCH_PREFIX, self.resource.to_string(false).to_string_lossy())
    }

    pub fn parent(&self) -> Rc<dyn ISearchTreeFolderMatch> {
        self.parent.upgrade().expect("a pasta dona do arquivo está viva")
    }

    pub fn has_children(&self) -> bool {
        !self.text_matches.borrow().is_empty()
    }

    /// `super.matches()`.
    pub fn super_matches(&self) -> Vec<Rc<dyn ISearchTreeMatch>> {
        self.text_matches.borrow().values().cloned().collect()
    }

    /// `this.matches()` (despachado para a subclasse).
    pub fn matches(&self) -> Vec<Rc<dyn ISearchTreeMatch>> {
        match self.virtual_members.upgrade() {
            Some(members) => members.matches(),
            None => self.super_matches(),
        }
    }

    pub fn text_matches(&self) -> Vec<Rc<dyn ISearchTreeMatch>> {
        self.text_matches.borrow().values().cloned().collect()
    }

    pub fn remove(&self, matches: &[Rc<dyn ISearchTreeMatch>]) {
        for m in matches {
            self.remove_match_virtual(m);
            self.removed_text_matches.borrow_mut().insert(m.id());
        }

        self.on_change.fire(FileMatchChangeEvent { did_remove: Some(true), force_update_model: None });
    }

    fn remove_match_virtual(&self, match_: &Rc<dyn ISearchTreeMatch>) {
        match self.virtual_members.upgrade() {
            Some(members) => members.remove_match(match_),
            None => self.super_remove_match(match_),
        }
    }

    /// `super.removeMatch(match)`.
    pub fn super_remove_match(&self, match_: &Rc<dyn ISearchTreeMatch>) {
        self.text_matches.borrow_mut().remove(&match_.id());
        if self.is_match_selected(match_) {
            self.set_selected_match_virtual(None);
            let decoration_model = self.find_match_decoration_model.borrow().clone();
            if let Some(decoration_model) = decoration_model {
                decoration_model.clear_current_find_match_decoration();
            }
        } else {
            self.update_highlights();
        }
    }

    pub fn replace(&self, to_replace: &Rc<dyn ISearchTreeMatch>) -> Promise<()> {
        let queue = self.replace_q.borrow().clone();
        let replace_service = self.replace_service.clone();
        let file_match = self.me.clone();
        let to_replace = to_replace.clone();
        let line_number = to_replace.range().start_line_number;
        let next = queue.finally_with(move || {
            replace_service.replace_match(&to_replace).then(move |_| {
                if let Some(file_match) = file_match.upgrade() {
                    file_match.updates_matches_for_line_after_replace(line_number, false);
                }
            })
        });
        *self.replace_q.borrow_mut() = next.clone();
        next
    }

    fn set_selected_match_virtual(&self, match_: Option<Rc<dyn ISearchTreeMatch>>) {
        match self.virtual_members.upgrade() {
            Some(members) => members.set_selected_match(match_),
            None => self.super_set_selected_match(match_),
        }
    }

    /// `super.setSelectedMatch(match)`.
    pub fn super_set_selected_match(&self, match_: Option<Rc<dyn ISearchTreeMatch>>) {
        if let Some(match_) = &match_ {
            if !self.text_matches.borrow().contains_key(&match_.id()) {
                return;
            }
            if self.is_match_selected(match_) {
                return;
            }
        }

        *self.selected_match.borrow_mut() = match_;
        self.update_highlights();
    }

    pub fn get_selected_match(&self) -> Option<Rc<dyn ISearchTreeMatch>> {
        self.selected_match.borrow().clone()
    }

    pub fn is_match_selected(&self, match_: &Rc<dyn ISearchTreeMatch>) -> bool {
        self.selected_match.borrow().as_ref().is_some_and(|selected| selected.id() == match_.id())
    }

    pub fn count(&self) -> usize {
        self.matches().len()
    }

    pub fn resource(&self) -> URI {
        self.resource.clone()
    }

    pub fn name(&self) -> String {
        self.name.value()
    }

    pub fn add_context(&self, results: Option<&[ITextSearchResult]>) {
        let Some(results) = results else {
            return;
        };

        let mut context = self.context.borrow_mut();
        for result in results {
            if let ITextSearchResult::Context(line) = result {
                context.insert(line.line_number, line.text.clone());
            }
        }
    }

    pub fn add(&self, match_: Rc<dyn ISearchTreeMatch>, trigger: bool) {
        self.text_matches.borrow_mut().insert(match_.id(), match_);
        if trigger {
            self.on_change.fire(FileMatchChangeEvent { did_remove: None, force_update_model: Some(true) });
        }
    }

    pub fn resolve_file_stat(&self, file_service: &Rc<dyn IFileService>) -> Promise<()> {
        let file_match = self.me.clone();
        file_service.stat(&self.resource).then_result(move |result| {
            if let Some(file_match) = file_match.upgrade() {
                *file_match.file_stat.borrow_mut() = result.ok();
            }
            Ok(())
        })
    }

    pub fn file_stat(&self) -> Option<FileStatWithMetadata> {
        self.file_stat.borrow().clone()
    }

    pub fn set_file_stat(&self, stat: Option<FileStatWithMetadata>) {
        *self.file_stat.borrow_mut() = stat;
    }

    pub fn has_only_read_only_matches(&self) -> bool {
        self.matches().iter().all(|m| m.is_readonly())
    }
}

impl IDisposable for FileMatchImpl {
    fn dispose(&self) {
        self.set_selected_match_virtual(None);
        self.unbind_model();
        self.on_dispose.fire(());
        self.base.dispose();
    }
}

/// Implementa, no `impl ISearchTreeFileMatch for X` de uma subclasse que guarda a base em `self.base`, os membros que o
/// `FileMatchImpl` define e a subclasse não redefine (`id`, `name`, `has_children`, `matches` e `set_selected_match` ficam
/// com a subclasse).
macro_rules! forward_file_match_methods {
    () => {
        fn resource(&self) -> ::je_base::uri::URI {
            self.base.resource()
        }
        fn on_change(&self) -> ::je_base::event::Event<$crate::contrib::search::search_tree_model::search_tree_common::FileMatchChangeEvent> {
            self.base.on_change()
        }
        fn on_dispose(&self) -> ::je_base::event::Event<()> {
            self.base.on_dispose()
        }
        fn count(&self) -> usize {
            self.base.count()
        }
        fn has_only_read_only_matches(&self) -> bool {
            self.base.has_only_read_only_matches()
        }
        fn update_highlights(&self) {
            self.base.update_highlights()
        }
        fn get_selected_match(
            &self,
        ) -> Option<::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeMatch>> {
            self.base.get_selected_match()
        }
        fn parent(&self) -> ::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeFolderMatch> {
            self.base.parent()
        }
        fn bind_model(&self, model: &::std::rc::Rc<::je_editor::text_model::TextModel>) {
            self.base.bind_model(model)
        }
        fn has_readonly_matches(&self) -> bool {
            self.base.has_readonly_matches()
        }
        fn add_context(&self, results: Option<&[$crate::services::search::search::ITextSearchResult]>) {
            self.base.add_context(results)
        }
        fn add(
            &self,
            match_: ::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeMatch>,
            trigger: bool,
        ) {
            self.base.add(match_, trigger)
        }
        fn replace(
            &self,
            to_replace: &::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeMatch>,
        ) -> ::je_base::promise::Promise<()> {
            self.base.replace(to_replace)
        }
        fn remove(&self, matches: &[::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeMatch>]) {
            self.base.remove(matches)
        }
        fn file_stat(&self) -> Option<::je_platform::files::files::FileStatWithMetadata> {
            self.base.file_stat()
        }
        fn set_file_stat(&self, stat: Option<::je_platform::files::files::FileStatWithMetadata>) {
            self.base.set_file_stat(stat)
        }
        fn resolve_file_stat(&self, file_service: &::std::rc::Rc<dyn ::je_platform::files::i_file_service::IFileService>) -> ::je_base::promise::Promise<()> {
            self.base.resolve_file_stat(file_service)
        }
        fn text_matches(&self) -> Vec<::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeMatch>> {
            self.base.text_matches()
        }
        fn context(&self) -> ::je_base::map::OrderedMap<i64, ::je_base::js_string::JsString> {
            self.base.context()
        }
        fn closest_root(&self) -> Option<::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeFolderMatch>> {
            self.base.closest_root()
        }
        fn is_match_selected(
            &self,
            match_: &::std::rc::Rc<dyn $crate::contrib::search::search_tree_model::search_tree_common::ISearchTreeMatch>,
        ) -> bool {
            self.base.is_match_selected(match_)
        }
    };
}

pub(crate) use forward_file_match_methods;
