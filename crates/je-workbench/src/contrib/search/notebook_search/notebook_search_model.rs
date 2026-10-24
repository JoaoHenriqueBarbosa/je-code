//! Porte de `src/vs/workbench/contrib/search/browser/notebookSearch/notebookSearchModel.ts`: o casamento dentro de uma
//! célula (`MatchInNotebook`), a célula (`CellMatch`) e o arquivo que sabe de células de notebook
//! (`NotebookCompatibleFileMatch`).
//!
//! Divergências:
//! - **Herança.** `MatchInNotebook extends MatchImpl` embute o [`MatchImpl`] em `base`; `NotebookCompatibleFileMatch extends
//!   FileMatchImpl` embute o [`FileMatchImpl`] em `base` e redefine `matches`, `removeMatch` e `setSelectedMatch` pelo
//!   [`FileMatchVirtual`] (os demais membros vêm da macro `forward_file_match_methods`).
//! - O pai de [`CellMatch`] e a célula de [`MatchInNotebook`] são `Weak` (o arquivo é dono das células, e a célula dos
//!   casamentos).
//! - `new FindMatchDecorationModel(widget, searchInstanceID)` é `widget.create_find_match_decoration_model(...)`.
//! - `this._cell instanceof CellSearchModel` é um `as_any().is::<CellSearchModel>()`: o `CellSearchModel` do porte não
//!   implementa `ICellViewModel` (o original o passa por conversão de tipo), então o teste só vale para quem o fizer.
//! - `catch (e) { // no op }` de `setNotebookFindMatchDecorationsUsingCellMatches` é [`errors::catch`] ignorando o erro.
//! - Os `async` de `updateMatchesForEditorWidget` e `showMatch` devolvem `Promise<()>`; os resultados de
//!   `revealRangeInCenterIfOutsideViewportAsync` e `highlightCurrentFindMatchDecoration` chamados sem `await` são
//!   descartados.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::async_::RunOnceScheduler;
use je_base::cancellation::CancellationToken;
use je_base::errors;
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, IDisposable, id_of};
use je_base::map::OrderedMap;
use je_base::promise::Promise;
use je_editor::model::FindMatch;
use je_editor::range::Range;
use je_editor::services_model::IModelService;
use je_instantiation::Injectable;
use je_platform::label::label::ILabelService;

use super::notebook_search_model_base::{ICellMatch, IMatchInNotebook, INotebookFileInstanceMatch};
use super::search_notebook_helpers::{
    INotebookCellMatch, content_matches_to_text_search_matches, is_i_notebook_file_match_with_model, webview_matches_to_text_search_matches,
};
use crate::contrib::notebook::browser::contrib::find::find_model::CellFindMatchModel;
use crate::contrib::notebook::browser::notebook_browser::{
    CellFindMatchWithIndex, CellWebviewFindMatch, ICellViewModel, INotebookFindOptions,
};
use crate::contrib::notebook::browser::notebook_editor_widget::NotebookEditorWidget;
use crate::contrib::notebook::browser::services::notebook_editor_service::INotebookEditorService;
use crate::contrib::notebook::common::notebook_common::NotebookCellsChangeType;
use crate::contrib::search::common::cell_search_model::CellSearchModel;
use crate::contrib::search::common::search_notebook_helpers::{RAW_CELL_PREFIX, is_i_notebook_file_match_no_model};
use crate::contrib::search::replace::IReplaceService;
use crate::contrib::search::search_tree_model::file_match::{FileMatchImpl, FileMatchVirtual, forward_file_match_methods};
use crate::contrib::search::search_tree_model::r#match::{MatchImpl, split_lines_lf, text_search_result_to_matches};
use crate::contrib::search::search_tree_model::search_tree_common::{
    FileMatchChangeEvent, ISearchTreeFileMatch, ISearchTreeFolderMatch, ISearchTreeMatch, MATCH_PREFIX, MatchPreview,
};
use crate::services::search::search::{
    IFileMatch, ISearchRange, ITextSearchPreviewOptions, ITextSearchResult, PatternInfo, TextSearchMatch,
};
use crate::services::search::search_helpers::get_text_search_match_with_model_context;

/// `MatchInNotebook`.
pub struct MatchInNotebook {
    this: Weak<MatchInNotebook>,
    base: MatchImpl,
    cell_parent: Weak<CellMatch>,
    webview_index: Option<usize>,
}

impl MatchInNotebook {
    pub fn new(
        cell_parent: &Rc<CellMatch>,
        full_preview_lines: Vec<JsString>,
        full_preview_range: ISearchRange,
        document_range: ISearchRange,
        webview_index: Option<usize>,
    ) -> Rc<MatchInNotebook> {
        let parent_file: Rc<dyn ISearchTreeFileMatch> = cell_parent.parent();
        let base = MatchImpl::new_base(&parent_file, full_preview_lines, full_preview_range, document_range, false);
        let webview_suffix = webview_index.filter(|index| *index != 0).map(|index| format!("_{index}")).unwrap_or_default();
        let match_type = if webview_index.is_some() { "webview" } else { "content" };
        base.set_id(format!(
            "{}{}>{}{}_{}{}{}",
            MATCH_PREFIX,
            parent_file.resource().to_string(false).to_string_lossy(),
            cell_parent.cell_index(),
            webview_suffix,
            match_type,
            base.range(),
            base.get_match_string()
        ));
        Rc::new_cyclic(|this| Self { this: this.clone(), base, cell_parent: Rc::downgrade(cell_parent), webview_index })
    }

    fn cell_match(&self) -> Rc<CellMatch> {
        self.cell_parent.upgrade().expect("a célula dona do casamento está viva")
    }
}

impl ISearchTreeMatch for MatchInNotebook {
    fn id(&self) -> String {
        self.base.id()
    }

    /// `parent()`: o pai visível na árvore de busca.
    fn parent(&self) -> Rc<dyn ISearchTreeFileMatch> {
        self.cell_match().parent()
    }

    fn text(&self) -> JsString {
        self.base.text()
    }

    fn range(&self) -> Range {
        self.base.range()
    }

    fn preview(&self) -> MatchPreview {
        self.base.preview()
    }

    fn replace_string(&self) -> JsString {
        self.base.replace_string()
    }

    fn full_match_text(&self, include_surrounding: bool) -> JsString {
        self.base.full_match_text(include_surrounding)
    }

    fn range_in_preview(&self) -> ISearchRange {
        self.base.range_in_preview()
    }

    fn full_preview_lines(&self) -> Vec<JsString> {
        self.base.full_preview_lines()
    }

    fn get_match_string(&self) -> JsString {
        self.base.get_match_string()
    }

    fn is_readonly(&self) -> bool {
        self.base.is_readonly() || !self.cell_match().has_cell_view_model() || self.is_webview_match()
    }

    fn as_match_in_notebook(&self) -> Option<Rc<dyn IMatchInNotebook>> {
        self.this.upgrade().map(|this| -> Rc<dyn IMatchInNotebook> { this })
    }
}

impl IMatchInNotebook for MatchInNotebook {
    fn notebook_parent(&self) -> Rc<dyn INotebookFileInstanceMatch> {
        self.cell_match().parent()
    }

    fn cell_parent(&self) -> Rc<dyn ICellMatch> {
        self.cell_match()
    }

    fn is_webview_match(&self) -> bool {
        self.webview_index.is_some()
    }

    fn cell_index(&self) -> usize {
        self.cell_match().cell_index()
    }

    fn webview_index(&self) -> Option<usize> {
        self.webview_index
    }

    fn cell(&self) -> Option<Rc<dyn ICellViewModel>> {
        self.cell_match().cell()
    }
}

/// `CellMatch`.
pub struct CellMatch {
    this: Weak<CellMatch>,
    parent: Weak<dyn INotebookFileInstanceMatch>,
    cell: RefCell<Option<Rc<dyn ICellViewModel>>>,
    cell_index: usize,
    content_matches: RefCell<OrderedMap<String, Rc<MatchInNotebook>>>,
    webview_matches: RefCell<OrderedMap<String, Rc<MatchInNotebook>>>,
    context: RefCell<OrderedMap<i64, JsString>>,
}

impl CellMatch {
    pub fn new(parent: &Rc<dyn INotebookFileInstanceMatch>, cell: Option<Rc<dyn ICellViewModel>>, cell_index: usize) -> Rc<CellMatch> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            parent: Rc::downgrade(parent),
            cell: RefCell::new(cell),
            cell_index,
            content_matches: RefCell::new(OrderedMap::new()),
            webview_matches: RefCell::new(OrderedMap::new()),
            context: RefCell::new(OrderedMap::new()),
        })
    }

    fn self_rc(&self) -> Rc<CellMatch> {
        self.this.upgrade().expect("a célula está viva enquanto é usada")
    }

    fn collect_matches(matches: &RefCell<OrderedMap<String, Rc<MatchInNotebook>>>) -> Vec<Rc<dyn IMatchInNotebook>> {
        matches.borrow().values().map(|match_| -> Rc<dyn IMatchInNotebook> { match_.clone() }).collect()
    }
}

impl ICellMatch for CellMatch {
    fn has_cell_view_model(&self) -> bool {
        !self.cell.borrow().as_ref().is_some_and(|cell| cell.as_any().is::<CellSearchModel>())
    }

    fn context(&self) -> OrderedMap<i64, JsString> {
        self.context.borrow().clone()
    }

    fn matches(&self) -> Vec<Rc<dyn IMatchInNotebook>> {
        let mut matches = Self::collect_matches(&self.content_matches);
        matches.extend(Self::collect_matches(&self.webview_matches));
        matches
    }

    fn content_matches(&self) -> Vec<Rc<dyn IMatchInNotebook>> {
        Self::collect_matches(&self.content_matches)
    }

    fn webview_matches(&self) -> Vec<Rc<dyn IMatchInNotebook>> {
        Self::collect_matches(&self.webview_matches)
    }

    fn remove(&self, matches: &[Rc<dyn IMatchInNotebook>]) {
        for match_ in matches {
            self.content_matches.borrow_mut().remove(&match_.id());
            self.webview_matches.borrow_mut().remove(&match_.id());
        }
    }

    fn clear_all_matches(&self) {
        self.content_matches.borrow_mut().clear();
        self.webview_matches.borrow_mut().clear();
    }

    fn add_content_matches(&self, text_search_matches: &[TextSearchMatch]) {
        let content_matches = text_search_matches_to_notebook_matches(text_search_matches, &self.self_rc());
        for match_ in content_matches {
            self.content_matches.borrow_mut().insert(match_.id(), match_);
        }
        self.add_context(text_search_matches);
    }

    fn add_context(&self, text_search_matches: &[TextSearchMatch]) {
        let Some(cell) = self.cell() else {
            // todo: get closed notebook results in search editor
            return;
        };
        let this = self.this.clone();
        let text_search_matches = text_search_matches.to_vec();
        let _ = cell.resolve_text_model().then(move |text_model| {
            let Some(this) = this.upgrade() else {
                return;
            };
            let query = this.parent().parent().query().expect("a consulta existe enquanto há resultados");
            let text_results_with_context = get_text_search_match_with_model_context(&text_search_matches, text_model.as_ref(), &query);
            for result in text_results_with_context {
                if let ITextSearchResult::Context(context) = result {
                    this.context.borrow_mut().insert(context.line_number + 1, context.text);
                }
            }
        });
    }

    fn add_webview_matches(&self, text_search_matches: &[TextSearchMatch]) {
        let webview_matches = text_search_matches_to_notebook_matches(text_search_matches, &self.self_rc());
        for match_ in webview_matches {
            self.webview_matches.borrow_mut().insert(match_.id(), match_);
        }
        // TODO do original: add webview results to context
    }

    fn set_cell_model(&self, cell: Rc<dyn ICellViewModel>) {
        *self.cell.borrow_mut() = Some(cell);
    }

    fn parent(&self) -> Rc<dyn INotebookFileInstanceMatch> {
        self.parent.upgrade().expect("o arquivo dono da célula está vivo")
    }

    fn id(&self) -> String {
        match self.cell.borrow().as_ref() {
            Some(cell) => cell.id(),
            None => format!("{RAW_CELL_PREFIX}{}", self.cell_index),
        }
    }

    fn cell_index(&self) -> usize {
        self.cell_index
    }

    fn cell(&self) -> Option<Rc<dyn ICellViewModel>> {
        self.cell.borrow().clone()
    }
}

/// Os argumentos estáticos de `NotebookCompatibleFileMatch` (`_query`, `_previewOptions`, `_maxResults`, `_parent`,
/// `rawMatch`, `_closestRoot` e `searchInstanceID`).
pub struct NotebookCompatibleFileMatchArgs {
    pub query: PatternInfo,
    pub preview_options: Option<ITextSearchPreviewOptions>,
    pub max_results: Option<i64>,
    pub parent: Rc<dyn ISearchTreeFolderMatch>,
    pub raw_match: IFileMatch,
    pub closest_root: Option<Rc<dyn ISearchTreeFolderMatch>>,
    pub search_instance_id: String,
}

/// `NotebookCompatibleFileMatch`.
pub struct NotebookCompatibleFileMatch {
    this: Weak<NotebookCompatibleFileMatch>,
    base: Rc<FileMatchImpl>,
    search_instance_id: String,
    notebook_editor_service: Rc<dyn INotebookEditorService>,

    notebook_editor_widget: RefCell<Option<Rc<dyn NotebookEditorWidget>>>,
    editor_widget_listener: RefCell<Option<DisposableRef>>,
    notebook_update_scheduler: Rc<RunOnceScheduler>,
    last_editor_widget_id_for_update: RefCell<Option<String>>,
    cell_matches: RefCell<OrderedMap<String, Rc<CellMatch>>>,
}

impl Injectable for NotebookCompatibleFileMatch {
    type Args = NotebookCompatibleFileMatchArgs;
    type Deps = (Rc<dyn IModelService>, Rc<dyn IReplaceService>, Rc<dyn ILabelService>, Rc<dyn INotebookEditorService>);

    fn create(
        args: NotebookCompatibleFileMatchArgs,
        (model_service, replace_service, label_service, notebook_editor_service): Self::Deps,
    ) -> Rc<Self> {
        let NotebookCompatibleFileMatchArgs { query, preview_options, max_results, parent, raw_match, closest_root, search_instance_id } = args;
        Rc::new_cyclic(|this: &Weak<NotebookCompatibleFileMatch>| {
            let outer: Weak<dyn ISearchTreeFileMatch> = this.clone();
            let virtual_members: Weak<dyn FileMatchVirtual> = this.clone();
            let base = FileMatchImpl::new(
                query,
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

            let scheduler_target = this.clone();
            let notebook_update_scheduler = RunOnceScheduler::new(
                move || {
                    if let Some(file_match) = scheduler_target.upgrade() {
                        let _ = file_match.update_matches_for_editor_widget();
                    }
                },
                250,
            );
            base.register(notebook_update_scheduler.clone());

            Self {
                this: this.clone(),
                base,
                search_instance_id,
                notebook_editor_service,
                notebook_editor_widget: RefCell::new(None),
                editor_widget_listener: RefCell::new(None),
                notebook_update_scheduler,
                last_editor_widget_id_for_update: RefCell::new(None),
                cell_matches: RefCell::new(OrderedMap::new()),
            }
        })
    }
}

impl NotebookCompatibleFileMatch {
    fn self_rc(&self) -> Rc<NotebookCompatibleFileMatch> {
        self.this.upgrade().expect("o arquivo está vivo enquanto é usado")
    }

    fn selected_notebook_match(&self) -> Option<Rc<dyn IMatchInNotebook>> {
        let selected_match = self.base.selected_match.borrow().clone();
        selected_match.and_then(|selected_match| selected_match.as_match_in_notebook())
    }

    pub fn cell_context(&self) -> OrderedMap<String, OrderedMap<i64, JsString>> {
        let mut cell_context = OrderedMap::new();
        for cell_match in self.cell_matches.borrow().values() {
            cell_context.insert(cell_match.id(), cell_match.context());
        }
        cell_context
    }

    pub fn add_webview_matches_to_cell(&self, cell_id: &str, webview_matches: &[TextSearchMatch]) {
        if let Some(cell_match) = INotebookFileInstanceMatch::get_cell_match(self, cell_id) {
            cell_match.add_webview_matches(webview_matches);
        }
    }

    pub fn add_content_matches_to_cell(&self, cell_id: &str, content_matches: &[TextSearchMatch]) {
        if let Some(cell_match) = INotebookFileInstanceMatch::get_cell_match(self, cell_id) {
            cell_match.add_content_matches(content_matches);
        }
    }

    fn reveal_cell_range(&self, match_: &Rc<dyn IMatchInNotebook>, output_offset: Option<f64>) {
        let widget = self.notebook_editor_widget.borrow().clone();
        let (Some(widget), Some(cell)) = (widget, match_.cell()) else {
            // match cell should never be a CellSearchModel if the notebook is open
            return;
        };
        if match_.webview_index().is_some() {
            let index = widget.get_cell_index(&cell);
            if index.is_some() {
                widget.reveal_cell_offset_in_center(&cell, output_offset.unwrap_or(0.0));
            }
        } else {
            cell.update_edit_state(cell.get_edit_state(), "focusNotebookCell");
            widget.set_cell_editor_selection(&cell, &match_.range());
            let _ = widget.reveal_range_in_center_if_outside_viewport_async(&cell, &match_.range());
        }
    }

    fn add_notebook_highlights(&self) {
        let Some(widget) = self.notebook_editor_widget.borrow().clone() else {
            return;
        };
        self.remove_notebook_highlights();
        *self.base.find_match_decoration_model.borrow_mut() = Some(widget.create_find_match_decoration_model(&self.search_instance_id));
        if let Some(selected_match) = self.selected_notebook_match() {
            let _ = self.highlight_current_find_match_decoration(&selected_match);
        }
    }

    fn remove_notebook_highlights(&self) {
        let decoration_model = self.base.find_match_decoration_model.borrow_mut().take();
        if let Some(decoration_model) = decoration_model {
            decoration_model.stop_webview_find();
            decoration_model.dispose();
        }
    }

    fn update_notebook_matches(&self, matches: &[CellFindMatchWithIndex], model_change: bool) {
        let Some(widget) = self.notebook_editor_widget.borrow().clone() else {
            return;
        };

        let old_cell_matches = self.cell_matches.borrow().clone();
        let widget_id = widget.get_id();
        if self.last_editor_widget_id_for_update.borrow().as_deref() != Some(widget_id.as_str()) {
            self.cell_matches.borrow_mut().clear();
            *self.last_editor_widget_id_for_update.borrow_mut() = Some(widget_id);
        }
        for match_ in matches {
            let mut existing_cell: Option<Rc<CellMatch>> = self.cell_matches.borrow().get(&match_.cell.id()).cloned();
            if existing_cell.is_none() {
                let index = widget.get_cell_index(&match_.cell);
                let existing_raw_cell = index.and_then(|index| old_cell_matches.get(&format!("{RAW_CELL_PREFIX}{index}")).cloned());
                if let Some(existing_raw_cell) = existing_raw_cell {
                    existing_raw_cell.set_cell_model(match_.cell.clone());
                    existing_raw_cell.clear_all_matches();
                    existing_cell = Some(existing_raw_cell);
                }
            }
            if let Some(existing_cell) = &existing_cell {
                existing_cell.clear_all_matches();
            }
            let cell = existing_cell.unwrap_or_else(|| {
                let parent: Rc<dyn INotebookFileInstanceMatch> = self.self_rc();
                CellMatch::new(&parent, Some(match_.cell.clone()), match_.index)
            });
            cell.add_content_matches(&content_matches_to_text_search_matches(&match_.content_matches, &match_.cell));
            cell.add_webview_matches(&webview_matches_to_text_search_matches(&match_.webview_matches));
            self.cell_matches.borrow_mut().insert(cell.id(), cell);
        }

        let decoration_model = self.base.find_match_decoration_model.borrow().clone();
        if let Some(decoration_model) = decoration_model {
            decoration_model.set_all_find_matches_decorations(matches);
        }
        if let Some(selected_match) = self.selected_notebook_match() {
            let _ = self.highlight_current_find_match_decoration(&selected_match);
        }
        self.base.fire_change(FileMatchChangeEvent { did_remove: None, force_update_model: Some(model_change) });
    }

    fn set_notebook_find_match_decorations_using_cell_matches(&self, cells: &[Rc<dyn ICellMatch>]) {
        let Some(decoration_model) = self.base.find_match_decoration_model.borrow().clone() else {
            return;
        };
        let cell_find_match: Vec<CellFindMatchWithIndex> = cells
            .iter()
            .filter_map(|cell| {
                let webview_matches: Vec<CellWebviewFindMatch> = cell
                    .webview_matches()
                    .iter()
                    .filter_map(|match_| {
                        match_.webview_index().filter(|index| *index != 0).map(|index| CellWebviewFindMatch { index, search_preview_info: None })
                    })
                    .collect();
                let cell_view_model = cell.cell()?;
                let find_matches: Vec<FindMatch> =
                    cell.content_matches().iter().map(|match_| FindMatch { range: match_.range(), matches: Some(vec![Some(match_.text())]) }).collect();
                Some(CellFindMatchModel::new(cell_view_model, cell.cell_index(), find_matches, webview_matches).to_find_match_with_index())
            })
            .collect();
        // no op, might happen due to bugs related to cell output regex search
        let _ = errors::catch(|| decoration_model.set_all_find_matches_decorations(&cell_find_match));
    }

    pub fn update_matches_for_editor_widget_impl(&self) -> Promise<()> {
        let Some(widget) = self.notebook_editor_widget.borrow().clone() else {
            return Promise::resolved(());
        };

        *self.base.text_matches.borrow_mut() = OrderedMap::new();

        let query = &self.base.query;
        let word_separators = if query.is_word_match.unwrap_or(false) {
            query.word_separators.as_ref().filter(|separators| !separators.is_empty()).map(JsString::to_string_lossy)
        } else {
            None
        };
        let notebook_info = query.notebook_info;
        let options = INotebookFindOptions {
            regex: query.is_reg_exp,
            whole_word: query.is_word_match,
            case_sensitive: query.is_case_sensitive,
            word_separators,
            include_markup_input: notebook_info.and_then(|info| info.is_in_notebook_markdown_input),
            include_markup_preview: notebook_info.and_then(|info| info.is_in_notebook_markdown_preview),
            include_code_input: notebook_info.and_then(|info| info.is_in_notebook_cell_input),
            include_output: notebook_info.and_then(|info| info.is_in_notebook_cell_output),
        };
        let this = self.this.clone();
        widget.find(&query.pattern.to_string_lossy(), &options, CancellationToken::None, false, true, &self.search_instance_id).then(
            move |all_matches| {
                if let Some(this) = this.upgrade() {
                    this.update_notebook_matches(&all_matches, true);
                }
            },
        )
    }

    fn highlight_current_find_match_decoration(&self, match_: &Rc<dyn IMatchInNotebook>) -> Promise<Option<f64>> {
        let decoration_model = self.base.find_match_decoration_model.borrow().clone();
        let (Some(decoration_model), Some(cell)) = (decoration_model, match_.cell()) else {
            // match cell should never be a CellSearchModel if the notebook is open
            return Promise::resolved(None);
        };
        match match_.webview_index() {
            None => decoration_model.highlight_current_find_match_decoration_in_cell(&cell, &match_.range()),
            Some(webview_index) => decoration_model.highlight_current_find_match_decoration_in_webview(&cell, webview_index),
        }
    }

    /// `createMatches()`.
    pub fn create_matches(&self) {
        let model = self.base.model_service.get_model(&self.base.resource);
        if let Some(model) = model {
            // todo: handle better when ai contributed results has model, currently, createMatches does not work for this
            self.base.bind_model(&model);
            self.base.update_matches_for_model();
        } else {
            let notebook_editor_widget_borrow = self.notebook_editor_service.retrieve_existing_widget_from_uri(&self.base.resource);

            if let Some(widget) = notebook_editor_widget_borrow.and_then(|borrow| borrow.value) {
                INotebookFileInstanceMatch::bind_notebook_editor_widget(self, widget);
            }
            if let Some(results) = &self.base.raw_match.results {
                let outer = self.base.outer();
                for result in results {
                    if let ITextSearchResult::Match(raw_match) = result {
                        for match_ in text_search_result_to_matches(raw_match, &outer, false) {
                            self.base.add(match_, false);
                        }
                    }
                }
            }

            if is_i_notebook_file_match_with_model(&self.base.raw_match) || is_i_notebook_file_match_no_model(&self.base.raw_match) {
                for cell in self.base.raw_match.cell_results.iter().flatten() {
                    INotebookFileInstanceMatch::add_cell_match(self, cell);
                }
                self.set_notebook_find_match_decorations_using_cell_matches(&INotebookFileInstanceMatch::cell_matches(self));
                self.base.fire_change(FileMatchChangeEvent { did_remove: None, force_update_model: Some(true) });
            }
            self.base.add_context(self.base.raw_match.results.as_deref());
        }
    }
}

impl FileMatchVirtual for NotebookCompatibleFileMatch {
    fn matches(&self) -> Vec<Rc<dyn ISearchTreeMatch>> {
        let mut matches: Vec<Rc<dyn ISearchTreeMatch>> = self.base.super_matches();
        let cell_matches: Vec<Rc<CellMatch>> = self.cell_matches.borrow().values().cloned().collect();
        for cell_match in &cell_matches {
            matches.extend(cell_match.matches().into_iter().map(|match_| -> Rc<dyn ISearchTreeMatch> { match_ }));
        }
        matches
    }

    fn remove_match(&self, match_: &Rc<dyn ISearchTreeMatch>) {
        if let Some(notebook_match) = match_.as_match_in_notebook() {
            let cell_parent = notebook_match.cell_parent();
            cell_parent.remove(&[notebook_match.clone()]);
            if cell_parent.matches().is_empty() {
                self.cell_matches.borrow_mut().remove(&cell_parent.id());
            }

            if self.base.is_match_selected(match_) {
                ISearchTreeFileMatch::set_selected_match(self, None);
                let decoration_model = self.base.find_match_decoration_model.borrow().clone();
                if let Some(decoration_model) = decoration_model {
                    decoration_model.clear_current_find_match_decoration();
                }
            } else {
                self.base.update_highlights();
            }

            self.set_notebook_find_match_decorations_using_cell_matches(&INotebookFileInstanceMatch::cell_matches(self));
        } else {
            self.base.super_remove_match(match_);
        }
    }

    fn set_selected_match(&self, match_: Option<Rc<dyn ISearchTreeMatch>>) {
        if let Some(selected) = &match_ {
            if !self.base.is_match_selected(selected) && selected.as_match_in_notebook().is_some() {
                *self.base.selected_match.borrow_mut() = match_;
                return;
            }

            if !self.base.text_matches.borrow().contains_key(&selected.id()) {
                return;
            }
            if self.base.is_match_selected(selected) {
                return;
            }
        }

        *self.base.selected_match.borrow_mut() = match_;
        self.base.update_highlights();
    }
}

impl INotebookFileInstanceMatch for NotebookCompatibleFileMatch {
    fn bind_notebook_editor_widget(&self, widget: Rc<dyn NotebookEditorWidget>) {
        let is_same_widget = self.notebook_editor_widget.borrow().as_ref().is_some_and(|current| id_of(current) == id_of(&widget));
        if is_same_widget {
            return;
        }

        *self.notebook_editor_widget.borrow_mut() = Some(widget.clone());

        let this = self.this.clone();
        let listener = widget.text_model().map(|text_model| {
            text_model.on_did_change_content().listen(move |event| {
                if !event
                    .raw_events
                    .iter()
                    .any(|raw_event| matches!(raw_event.event.kind(), NotebookCellsChangeType::ChangeCellContent | NotebookCellsChangeType::ModelChange))
                {
                    return;
                }
                if let Some(this) = this.upgrade() {
                    this.notebook_update_scheduler.schedule(None);
                }
            })
        });
        *self.editor_widget_listener.borrow_mut() = listener;
        self.add_notebook_highlights();
    }

    fn update_matches_for_editor_widget(&self) -> Promise<()> {
        self.update_matches_for_editor_widget_impl()
    }

    fn unbind_notebook_editor_widget(&self, widget: Option<&Rc<dyn NotebookEditorWidget>>) {
        if let Some(widget) = widget {
            let is_same_widget = self.notebook_editor_widget.borrow().as_ref().is_some_and(|current| id_of(current) == id_of(widget));
            if !is_same_widget {
                return;
            }
        }

        if self.notebook_editor_widget.borrow().is_some() {
            self.notebook_update_scheduler.cancel();
            let listener = self.editor_widget_listener.borrow().clone();
            if let Some(listener) = listener {
                listener.dispose();
            }
        }
        self.remove_notebook_highlights();
        *self.notebook_editor_widget.borrow_mut() = None;
    }

    fn update_notebook_highlights(&self) {
        if self.base.parent().show_highlights() {
            self.add_notebook_highlights();
            self.set_notebook_find_match_decorations_using_cell_matches(&INotebookFileInstanceMatch::cell_matches(self));
        } else {
            self.remove_notebook_highlights();
        }
    }

    fn get_cell_match(&self, cell_id: &str) -> Option<Rc<dyn ICellMatch>> {
        self.cell_matches.borrow().get(&cell_id.to_string()).map(|cell_match| -> Rc<dyn ICellMatch> { cell_match.clone() })
    }

    fn add_cell_match(&self, raw_cell: &INotebookCellMatch) {
        let parent: Rc<dyn INotebookFileInstanceMatch> = self.self_rc();
        let cell = match raw_cell {
            INotebookCellMatch::WithModel(cell_match) => Some(cell_match.cell.clone()),
            INotebookCellMatch::NoModel(_) => None,
        };
        let cell_match = CellMatch::new(&parent, cell, raw_cell.index());
        let cell_id = cell_match.id();
        self.cell_matches.borrow_mut().insert(cell_id.clone(), cell_match);
        self.add_webview_matches_to_cell(&cell_id, raw_cell.webview_results());
        self.add_content_matches_to_cell(&cell_id, raw_cell.content_results());
    }

    fn show_match(&self, match_: &Rc<dyn IMatchInNotebook>) -> Promise<()> {
        let this = self.this.clone();
        let shown_match = match_.clone();
        self.highlight_current_find_match_decoration(match_).then(move |offset| {
            if let Some(this) = this.upgrade() {
                let selected: Rc<dyn ISearchTreeMatch> = shown_match.clone();
                ISearchTreeFileMatch::set_selected_match(&*this, Some(selected));
                this.reveal_cell_range(&shown_match, offset);
            }
        })
    }

    fn cell_matches(&self) -> Vec<Rc<dyn ICellMatch>> {
        self.cell_matches.borrow().values().map(|cell_match| -> Rc<dyn ICellMatch> { cell_match.clone() }).collect()
    }
}

impl IDisposable for NotebookCompatibleFileMatch {
    fn dispose(&self) {
        INotebookFileInstanceMatch::unbind_notebook_editor_widget(self, None);
        self.base.dispose();
    }
}

impl ISearchTreeFileMatch for NotebookCompatibleFileMatch {
    forward_file_match_methods!();

    fn id(&self) -> String {
        self.base.id()
    }

    fn name(&self) -> String {
        self.base.name()
    }

    fn has_children(&self) -> bool {
        self.base.has_children() || !self.cell_matches.borrow().is_empty()
    }

    fn matches(&self) -> Vec<Rc<dyn ISearchTreeMatch>> {
        FileMatchVirtual::matches(self)
    }

    fn set_selected_match(&self, match_: Option<Rc<dyn ISearchTreeMatch>>) {
        FileMatchVirtual::set_selected_match(self, match_)
    }

    fn as_notebook_file_match(&self) -> Option<Rc<dyn INotebookFileInstanceMatch>> {
        self.this.upgrade().map(|this| -> Rc<dyn INotebookFileInstanceMatch> { this })
    }
}

/// `textSearchMatchesToNotebookMatches`: de resultados de busca de texto para casamentos dentro de uma célula.
pub fn text_search_matches_to_notebook_matches(text_search_matches: &[TextSearchMatch], cell: &Rc<CellMatch>) -> Vec<Rc<MatchInNotebook>> {
    let mut notebook_matches: Vec<Rc<MatchInNotebook>> = Vec::new();
    for text_search_match in text_search_matches {
        let preview_lines = split_lines_lf(&text_search_match.preview_text);
        for range_location in &text_search_match.range_locations {
            let preview_range: ISearchRange = range_location.preview;
            notebook_matches.push(MatchInNotebook::new(cell, preview_lines.clone(), preview_range, range_location.source, text_search_match.webview_index));
        }
    }
    notebook_matches
}
