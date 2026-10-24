//! Porte de `src/vs/workbench/contrib/search/browser/searchTreeModel/searchResult.ts`: o resultado de uma busca
//! (`SearchResultImpl`), a raiz da árvore com os cabeçalhos de texto simples e de IA.
//!
//! Divergências:
//! - `modelService.getModels()` e `modelService.onModelAdded` são os membros de [`IModelService`] que este porte
//!   acrescentou ao trait para o resultado os ler.
//! - `widget instanceof NotebookEditorWidget` é sempre verdadeiro para quem implementa o trait
//!   [`NotebookEditorWidget`]. Os ouvintes do widget guardam um `Weak` dele.
//! - `searchModel` é um `Weak` (o `SearchModelImpl` é dono do resultado). `batchReplace` (um `async`) devolve
//!   `Promise<()>`: as substituições começam em ordem e o emissor é retomado quando todas terminam.
//! - `override async dispose()` roda por inteiro de forma síncrona (não tem `await` antes do fim do corpo).
//! - `setAIQueryUsingTextQuery(query?)` recebe `Option<Option<ITextQuery>>` (`undefined` e `null` são `None`/`Some(None)`).

use std::rc::{Rc, Weak};

use je_base::event::{EmitterOptions, PauseableEmitter};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, MutableDisposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_editor::services_model::IModelService;
use je_editor::text_model::TextModel;
use je_instantiation::{Injectable, InstantiationService};

use super::range_decorations::RangeHighlightDecorations;
use super::search_tree_common::{
    FileOrFolderMatch, FolderMatchParent, IChangeEvent, IPlainTextSearchHeading, ISearchModel, ISearchResult, ISearchTreeFileMatch,
    ISearchTreeFolderMatch, ITextSearchHeading, RenderableMatch, SEARCH_RESULT_PREFIX, array_contains_element_or_parent,
    is_search_tree_folder_match_no_root, is_search_tree_folder_match_with_resource, merge_search_result_events,
};
use super::text_search_heading::PlainTextSearchHeadingImpl;
use crate::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;
use crate::contrib::notebook::browser::services::notebook_editor_service::INotebookEditorService;
use crate::contrib::search::ai_search::ai_search_model::AITextSearchHeadingImpl;
use crate::services::search::search::{IAITextQuery, IFileMatch, ISearchComplete, ITextQuery, ITextSearchQuery};
use crate::services::textfile::collaborators::{IProgress, ProgressStep};

/// `SearchResultImpl`.
pub struct SearchResultImpl {
    this: Weak<SearchResultImpl>,
    base: Rc<Disposable>,
    search_model: Weak<dyn ISearchModel>,
    on_change: Rc<PauseableEmitter<IChangeEvent>>,
    on_will_change_model_listener: Rc<MutableDisposable<dyn IDisposable>>,
    on_did_change_model_listener: Rc<MutableDisposable<dyn IDisposable>>,
    plain_text_search_result: Rc<PlainTextSearchHeadingImpl>,
    ai_text_search_result: Rc<AITextSearchHeadingImpl>,
    id: String,
}

impl Injectable for SearchResultImpl {
    type Args = Weak<dyn ISearchModel>;
    type Deps = (Rc<InstantiationService>, Rc<dyn IModelService>, Rc<dyn INotebookEditorService>);

    fn create(search_model: Self::Args, (instantiation_service, model_service, notebook_editor_service): Self::Deps) -> Rc<Self> {
        Rc::new_cyclic(|this: &Weak<SearchResultImpl>| {
            let base = Disposable::new();
            let on_change = PauseableEmitter::<IChangeEvent>::with_options(EmitterOptions::default(), Some(Rc::new(merge_search_result_events)));
            base.register(on_change.clone());
            let on_will_change_model_listener = MutableDisposable::<dyn IDisposable>::new();
            base.register(on_will_change_model_listener.clone());
            let on_did_change_model_listener = MutableDisposable::<dyn IDisposable>::new();
            base.register(on_did_change_model_listener.clone());

            let parent: Weak<dyn ISearchResult> = this.clone();
            let plain_text_search_result = instantiation_service.create_instance::<PlainTextSearchHeadingImpl>(parent.clone());
            base.register(plain_text_search_result.clone());
            let ai_text_search_result = instantiation_service.create_instance::<AITextSearchHeadingImpl>(parent);
            base.register(ai_text_search_result.clone());

            let emitter = on_change.clone();
            base.register(plain_text_search_result.on_change().listen(move |event| emitter.fire(event.clone())));
            let emitter = on_change.clone();
            base.register(ai_text_search_result.on_change().listen(move |event| emitter.fire(event.clone())));

            for model in model_service.get_models() {
                on_model_added(&plain_text_search_result, &model);
            }
            let plain = plain_text_search_result.clone();
            base.register(model_service.on_model_added().listen(move |model| on_model_added(&plain, model)));

            let plain = plain_text_search_result.clone();
            let will_change_listener = on_will_change_model_listener.clone();
            let did_change_listener = on_did_change_model_listener.clone();
            base.register(notebook_editor_service.on_did_add_notebook_editor().listen(move |widget| {
                on_did_add_notebook_editor_widget(&plain, &will_change_listener, &did_change_listener, widget);
            }));

            Self {
                this: this.clone(),
                base,
                search_model,
                on_change,
                on_will_change_model_listener,
                on_did_change_model_listener,
                plain_text_search_result,
                ai_text_search_result,
                id: format!("{SEARCH_RESULT_PREFIX}{}", runtime::date_now()),
            }
        })
    }
}

/// `onModelAdded(model)`.
fn on_model_added(plain_text_search_result: &Rc<PlainTextSearchHeadingImpl>, model: &Rc<TextModel>) {
    let folder_match = plain_text_search_result.find_folder_substr(&model.uri());
    if let Some(folder_match) = folder_match {
        folder_match.bind_model(model);
    }
}

/// `onDidAddNotebookEditorWidget(widget)`.
fn on_did_add_notebook_editor_widget(
    plain_text_search_result: &Rc<PlainTextSearchHeadingImpl>,
    on_will_change_model_listener: &Rc<MutableDisposable<dyn IDisposable>>,
    on_did_change_model_listener: &Rc<MutableDisposable<dyn IDisposable>>,
    widget: &Rc<dyn NotebookEditorWidget>,
) {
    let plain = plain_text_search_result.clone();
    let weak_widget = Rc::downgrade(widget);
    let will_change_listener: DisposableRef = widget.on_will_change_model().listen(move |model| {
        if let (Some(model), Some(widget)) = (model, weak_widget.upgrade()) {
            // onNotebookEditorWidgetRemoved
            if let Some(folder_match) = plain.find_folder_substr(&model.uri()) {
                folder_match.unbind_notebook_editor_widget(&widget, &model.uri());
            }
        }
    });
    on_will_change_model_listener.set(Some(will_change_listener));

    // listen to view model change as we are searching on both inputs and outputs
    let plain = plain_text_search_result.clone();
    let weak_widget = Rc::downgrade(widget);
    let did_change_listener: DisposableRef = widget.on_did_attach_view_model().listen(move |_| {
        let Some(widget) = weak_widget.upgrade() else {
            return;
        };
        if widget.has_model()
            && let Some(text_model) = widget.text_model()
        {
            // onNotebookEditorWidgetAdded
            let resource = text_model.uri();
            if let Some(folder_match) = plain.find_folder_substr(&resource) {
                let _ = folder_match.bind_notebook_editor_widget(&widget, &resource);
            }
        }
    });
    on_did_change_model_listener.set(Some(did_change_listener));
}

/// `aiTextQueryFromTextQuery`.
fn ai_text_query_from_text_query(query: Option<ITextQuery>) -> Option<IAITextQuery> {
    query.map(|query| IAITextQuery {
        common: query.common,
        content_pattern: query.content_pattern.pattern,
        preview_options: query.preview_options,
        max_file_size: query.max_file_size,
        surrounding_context: query.surrounding_context,
        user_disabled_excludes_and_ignore_files: query.user_disabled_excludes_and_ignore_files,
    })
}

impl SearchResultImpl {
    fn text_search_results(&self) -> Vec<Rc<dyn ITextSearchHeading>> {
        let plain: Rc<dyn ITextSearchHeading> = self.plain_text_search_result.clone();
        let ai: Rc<dyn ITextSearchHeading> = self.ai_text_search_result.clone();
        vec![plain, ai]
    }
}

impl IDisposable for SearchResultImpl {
    fn dispose(&self) {
        self.ai_text_search_result.dispose();
        self.plain_text_search_result.dispose();
        self.base.dispose();
    }
}

impl ISearchResult for SearchResultImpl {
    fn on_change(&self) -> je_base::event::Event<IChangeEvent> {
        self.on_change.event()
    }

    fn search_model(&self) -> Rc<dyn ISearchModel> {
        self.search_model.upgrade().expect("o modelo de busca dono do resultado está vivo")
    }

    fn plain_text_search_result(&self) -> Rc<dyn IPlainTextSearchHeading> {
        self.plain_text_search_result.clone()
    }

    fn ai_text_search_result(&self) -> Rc<dyn ITextSearchHeading> {
        self.ai_text_search_result.clone()
    }

    fn children(&self) -> Vec<Rc<dyn ITextSearchHeading>> {
        self.text_search_results()
    }

    fn has_children(&self) -> bool {
        // should always have a Text Search Result for plain results.
        true
    }

    fn is_dirty(&self) -> bool {
        ITextSearchHeading::is_dirty(&*self.ai_text_search_result) || ITextSearchHeading::is_dirty(&*self.plain_text_search_result)
    }

    fn query(&self) -> Option<ITextQuery> {
        match ITextSearchHeading::query(&*self.plain_text_search_result) {
            Some(ITextSearchQuery::Text(query)) => Some(query),
            _ => None,
        }
    }

    fn set_query(&self, query: Option<ITextQuery>) {
        ITextSearchHeading::set_query(&*self.plain_text_search_result, query.map(ITextSearchQuery::Text));
    }

    fn batch_replace(&self, elements_to_replace: Vec<RenderableMatch>) -> Promise<()> {
        self.on_change.pause();
        let mut replacements: Vec<Promise<()>> = Vec::new();
        for elem in &elements_to_replace {
            if let Some(parent) = elem.parent()
                && matches!(parent, RenderableMatch::FolderMatch(_) | RenderableMatch::FileMatch(_))
                && array_contains_element_or_parent(&parent, &elements_to_replace)
            {
                // skip any children who have parents in the array
                continue;
            }

            match elem {
                RenderableMatch::FileMatch(file_match) => replacements.push(file_match.parent().replace(file_match)),
                RenderableMatch::Match(match_) => replacements.push(match_.parent().replace(match_)),
                RenderableMatch::FolderMatch(folder_match) => replacements.push(folder_match.replace_all()),
                RenderableMatch::Heading(_) => {}
            }
        }

        let this = self.this.clone();
        Promise::all(replacements).then_result(move |result| {
            if let Some(this) = this.upgrade() {
                this.on_change.resume();
            }
            result.map(|_| ())
        })
    }

    fn batch_remove(&self, elements_to_remove: &[RenderableMatch]) {
        // need to check that we aren't trying to remove elements twice
        let mut removed_elems: Vec<RenderableMatch> = Vec::new();

        self.on_change.pause();
        for current_element in elements_to_remove {
            if array_contains_element_or_parent(current_element, &removed_elems) {
                continue;
            }
            match current_element {
                RenderableMatch::Heading(heading) => heading.hide(),
                RenderableMatch::FolderMatch(folder_match)
                    if !(is_search_tree_folder_match_with_resource(folder_match.as_ref()) || is_search_tree_folder_match_no_root(folder_match.as_ref())) => {}
                RenderableMatch::FileMatch(file_match) => {
                    file_match.parent().remove(&[FileOrFolderMatch::File(file_match.clone())]);
                    removed_elems.push(current_element.clone());
                }
                RenderableMatch::Match(match_) => {
                    match_.parent().remove(&[match_.clone()]);
                    removed_elems.push(current_element.clone());
                }
                RenderableMatch::FolderMatch(folder_match) => {
                    let elements = [FileOrFolderMatch::Folder(folder_match.clone())];
                    if is_search_tree_folder_match_with_resource(folder_match.as_ref()) {
                        match folder_match.parent() {
                            FolderMatchParent::FolderMatch(parent) => parent.remove(&elements),
                            FolderMatchParent::Heading(parent) => parent.remove(&elements, false),
                        }
                    } else if let FolderMatchParent::Heading(parent) = folder_match.parent() {
                        parent.remove(&elements, false);
                    }
                    removed_elems.push(current_element.clone());
                }
            }
        }
        self.on_change.resume();
    }

    fn folder_matches(&self, ai: bool) -> Vec<Rc<dyn ISearchTreeFolderMatch>> {
        if ai {
            return ITextSearchHeading::folder_matches(&*self.ai_text_search_result);
        }
        ITextSearchHeading::folder_matches(&*self.plain_text_search_result)
    }

    fn add(&self, all_raw: &[IFileMatch], search_instance_id: &str, ai: bool, silent: bool) {
        ITextSearchHeading::set_hidden(&*self.plain_text_search_result, false);

        if ai {
            ITextSearchHeading::add(&*self.ai_text_search_result, all_raw, search_instance_id, silent);
        } else {
            ITextSearchHeading::add(&*self.plain_text_search_result, all_raw, search_instance_id, silent);
        }
    }

    fn clear(&self) {
        ITextSearchHeading::clear(&*self.plain_text_search_result, true);
        ITextSearchHeading::clear(&*self.ai_text_search_result, true);
    }

    fn remove(&self, matches: &[FileOrFolderMatch], ai: bool) {
        if ai {
            ITextSearchHeading::remove(&*self.ai_text_search_result, matches, ai);
        }
        ITextSearchHeading::remove(&*self.plain_text_search_result, matches, ai);
    }

    fn replace(&self, match_: &Rc<dyn ISearchTreeFileMatch>) -> Promise<()> {
        IPlainTextSearchHeading::replace(&*self.plain_text_search_result, match_)
    }

    fn matches(&self, ai: Option<bool>) -> Vec<Rc<dyn ISearchTreeFileMatch>> {
        match ai {
            None => {
                let mut matches = ITextSearchHeading::matches(&*self.plain_text_search_result);
                matches.extend(ITextSearchHeading::matches(&*self.ai_text_search_result));
                matches
            }
            Some(true) => ITextSearchHeading::matches(&*self.ai_text_search_result),
            Some(false) => ITextSearchHeading::matches(&*self.plain_text_search_result),
        }
    }

    fn is_empty(&self) -> bool {
        ITextSearchHeading::is_empty(&*self.plain_text_search_result) && ITextSearchHeading::is_empty(&*self.ai_text_search_result)
    }

    fn file_count(&self, ignore_semantic_search_results: bool) -> usize {
        if ignore_semantic_search_results {
            return ITextSearchHeading::file_count(&*self.plain_text_search_result);
        }
        ITextSearchHeading::file_count(&*self.plain_text_search_result) + ITextSearchHeading::file_count(&*self.ai_text_search_result)
    }

    fn count(&self, ignore_semantic_search_results: bool) -> usize {
        if ignore_semantic_search_results {
            return ITextSearchHeading::count(&*self.plain_text_search_result);
        }
        ITextSearchHeading::count(&*self.plain_text_search_result) + ITextSearchHeading::count(&*self.ai_text_search_result)
    }

    fn id(&self) -> String {
        self.id.clone()
    }

    fn set_cached_search_complete(&self, cached_search_complete: Option<ISearchComplete>, ai: bool) {
        if ai {
            ITextSearchHeading::set_cached_search_complete(&*self.ai_text_search_result, cached_search_complete);
        } else {
            ITextSearchHeading::set_cached_search_complete(&*self.plain_text_search_result, cached_search_complete);
        }
    }

    fn get_cached_search_complete(&self, ai: bool) -> Option<ISearchComplete> {
        if ai {
            return ITextSearchHeading::cached_search_complete(&*self.ai_text_search_result);
        }
        ITextSearchHeading::cached_search_complete(&*self.plain_text_search_result)
    }

    fn toggle_highlights(&self, value: bool, ai: bool) {
        if ai {
            ITextSearchHeading::toggle_highlights(&*self.ai_text_search_result, value);
        } else {
            ITextSearchHeading::toggle_highlights(&*self.plain_text_search_result, value);
        }
    }

    fn get_range_highlight_decorations(&self, ai: bool) -> Rc<RangeHighlightDecorations> {
        if ai {
            return ITextSearchHeading::range_highlight_decorations(&*self.ai_text_search_result);
        }
        ITextSearchHeading::range_highlight_decorations(&*self.plain_text_search_result)
    }

    fn replace_all(&self, progress: Option<&dyn IProgress<ProgressStep>>) -> Promise<()> {
        IPlainTextSearchHeading::replace_all(&*self.plain_text_search_result, progress)
    }

    fn set_ai_query_using_text_query(&self, query: Option<Option<ITextQuery>>) {
        let query = query.flatten().or_else(|| ISearchResult::query(self));
        ITextSearchHeading::set_query(&*self.ai_text_search_result, ai_text_query_from_text_query(query).map(ITextSearchQuery::AiText));
    }
}
