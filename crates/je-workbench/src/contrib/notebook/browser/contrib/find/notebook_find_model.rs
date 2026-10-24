//! Porte de `src/vs/workbench/contrib/notebook/browser/contrib/find/findModel.ts`, o `FindModel`: liga o estado de busca
//! (`FindReplaceState<NotebookFindFilters>`) ao editor do notebook, calcula as ocorrências (com um atraso de 20 ms), guarda a ocorrência
//! atual e a navega. O `CellFindMatchModel` está em `find_model.rs`.
//!
//! Divergências:
//! - `INotebookEditor` é o trait [`INotebookFindEditor`], só com o que o `FindModel` lê, e o `NotebookViewModel` que o `getViewModel()` devolve é
//!   o concreto. As células são o `CellViewModel` (a identidade `===` é `ptr_eq`).
//! - `find(option: { previous: boolean } | { index: number })` é o enum [`FindOption`].
//! - `changeMatchInfo(currentMatch, total, undefined)` recebe a posição como `usize`: o `-1` (sem ocorrência atual) vai como `0`.
//! - `this._notebookEditor.find(val, options, token)` devolve [`CellFindMatchModel`] com a célula concreta.
//! - O `async`/`await` são `async_fn` e a [`Promise`]; `createCancelablePromise` é o do je-base. O construtor devolve `Rc<Self>` (os fechos
//!   guardam um `Weak`).
//! - `configurationService.inspect<string>('editor.wordSeparators').value` é o `value()` do `inspect` com os `overrides` padrão.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::arrays_find::find_first_idx_monotonous_or_arr_len;
use je_base::async_::{CancelablePromise, Delayer, async_fn, create_cancelable_promise};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::symbols::Delay;
use je_editor::find::find_state::{FindReplaceState, FindReplaceStateChangedEvent, INewFindReplaceState};
use je_editor::prefix_sum_computer::PrefixSumComputer;
use je_editor::range::Range;
use je_platform::configuration::ConfigurationOverrides;
use je_platform::configuration::service::IConfigurationService;

use super::find_filters::NotebookFindFilters;
use super::find_model::{CellFindMatchModel, CellMatch};
use super::notebook_find_match_decoration_model::{CurrentMatchDecorations, IFindDecorationsEditor, NotebookFindMatchDecorationModel};
use crate::contrib::notebook::browser::notebook_browser::CellEditState;
use crate::contrib::notebook::browser::notebook_view_events::NotebookCellStateChangedEvent;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::{CellViewModel, NotebookViewModel};
use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_common::{CellKind, ICell, INotebookFindOptions, NotebookCellsChangeType};
use crate::contrib::notebook::common::notebook_range::ICellRange;

/// `CellFindMatchWithIndex` com a célula concreta.
pub type CellMatches = CellFindMatchModel<CellViewModel>;

/// O que o `FindModel` lê do `INotebookEditor`.
pub trait INotebookFindEditor: IFindDecorationsEditor {
    fn get_id(&self) -> String;
    fn has_model(&self) -> bool;
    fn text_model(&self) -> Rc<NotebookTextModel>;
    fn on_did_change_model(&self) -> Event<Option<Rc<NotebookTextModel>>>;
    fn on_did_change_cell_state(&self) -> Event<NotebookCellStateChangedEvent>;
    fn get_view_model(&self) -> Option<Rc<NotebookViewModel>>;
    fn find(&self, query: &str, options: &INotebookFindOptions, token: CancellationToken) -> Promise<Vec<CellMatches>>;
    fn get_length(&self) -> usize;
    fn get_focus(&self) -> ICellRange;
    fn get_cell_index(&self, cell: &CellViewModel) -> i64;
    fn cell_at(&self, index: i64) -> Option<CellViewModel>;
    fn focus_element(&self, cell: &CellViewModel);
    fn reveal_cell_offset_in_center(&self, cell: &CellViewModel, offset: f64);
    fn set_cell_editor_selection(&self, cell: &CellViewModel, range: &Range);
    fn reveal_in_view(&self, cell: &CellViewModel) -> Promise<()>;
    fn reveal_range_in_center_if_outside_viewport_async(&self, cell: &CellViewModel, range: &Range) -> Promise<()>;
}

/// `{ previous: boolean } | { index: number }`.
#[derive(Clone, Copy, Debug)]
pub enum FindOption {
    Previous(bool),
    Index(i64),
}

/// O retorno de `getCurrentMatch()`.
pub struct CurrentMatchInfo {
    pub cell: CellViewModel,
    pub match_: CellMatch,
    pub is_model_match: bool,
}

/// `FindModel`.
pub struct FindModel {
    this: Weak<FindModel>,
    base: Rc<DisposableStore>,
    notebook_editor: Rc<dyn INotebookFindEditor>,
    state: Rc<FindReplaceState<NotebookFindFilters>>,
    configuration_service: Rc<dyn IConfigurationService>,

    find_matches: RefCell<Vec<CellMatches>>,
    find_matches_starts: RefCell<Option<PrefixSumComputer>>,
    current_match: Cell<i64>,

    throttled_delayer: Rc<Delayer<()>>,
    compute_promise: RefCell<Option<CancelablePromise<Option<Vec<CellMatches>>>>>,
    model_disposable: Rc<DisposableStore>,
    find_match_decoration_model: Rc<NotebookFindMatchDecorationModel>,
}

impl FindModel {
    pub fn new(
        notebook_editor: Rc<dyn INotebookFindEditor>,
        state: Rc<FindReplaceState<NotebookFindFilters>>,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Rc<Self> {
        let base = DisposableStore::new();
        let throttled_delayer = Rc::new(Delayer::<()>::new(Delay::Millis(20)));
        base.add_rc(throttled_delayer.clone());
        let model_disposable = DisposableStore::new();
        base.add(je_base::lifecycle::to_disposable({
            let model_disposable = model_disposable.clone();
            move || model_disposable.dispose()
        }));

        let decorations_editor: Rc<dyn IFindDecorationsEditor> = notebook_editor.clone();
        let find_match_decoration_model = NotebookFindMatchDecorationModel::new(decorations_editor, &notebook_editor.get_id());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            notebook_editor: notebook_editor.clone(),
            state: state.clone(),
            configuration_service,
            find_matches: RefCell::new(Vec::new()),
            find_matches_starts: RefCell::new(None),
            current_match: Cell::new(-1),
            throttled_delayer,
            compute_promise: RefCell::new(None),
            model_disposable,
            find_match_decoration_model,
        });

        let weak = this.this.clone();
        this.base.add(state.on_find_replace_state_change().listen(move |e| {
            let Some(this) = weak.upgrade() else { return };
            this.update_cell_states(e);

            if e.search_string
                || e.is_regex
                || e.match_case
                || e.search_scope
                || e.whole_word
                || (e.is_revealed && this.state.is_revealed())
                || e.filters
                || e.is_replace_revealed
            {
                let _ = this.research();
            }

            if e.is_revealed && !this.state.is_revealed() {
                this.clear();
            }
        }));

        let weak = this.this.clone();
        this.base.add(notebook_editor.on_did_change_model().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.register_model_listener(e.clone());
            }
        }));

        let weak = this.this.clone();
        this.base.add(notebook_editor.on_did_change_cell_state().listen(move |e| {
            if let Some(this) = weak.upgrade()
                && e.cell.cell_kind() == CellKind::Markup
                && e.source.edit_state_changed
            {
                // research when markdown cell is switching between markdown preview and editing mode.
                let _ = this.research();
            }
        }));

        if notebook_editor.has_model() {
            this.register_model_listener(Some(notebook_editor.text_model()));
        }

        this
    }

    pub fn find_matches(&self) -> Vec<CellMatches> {
        self.find_matches.borrow().clone()
    }

    pub fn current_match(&self) -> i64 {
        self.current_match.get()
    }

    fn word_separators(&self) -> Option<String> {
        match self.configuration_service.inspect("editor.wordSeparators", &ConfigurationOverrides::default()).value() {
            Dynamic::String(value) => Some(value.to_string_lossy()),
            _ => None,
        }
    }

    fn update_cell_states(&self, e: FindReplaceStateChangedEvent) {
        let filters = self.state.filters();
        let (Some(markup_input), Some(markup_preview), Some(find_scope)) = (
            filters.as_ref().map(NotebookFindFilters::markup_input).filter(|value| *value),
            filters.as_ref().map(NotebookFindFilters::markup_preview).filter(|value| *value),
            filters.as_ref().map(NotebookFindFilters::find_scope),
        ) else {
            return;
        };
        let _ = (markup_input, markup_preview);

        // we only update cell state if users are using the hybrid mode (both input and preview are enabled)
        let update_editing_state = || {
            let Some(view_model) = self.notebook_editor.get_view_model() else {
                return;
            };
            // search markup sources first to decide if a markup cell should be in editing mode
            let word_separators = self.word_separators();
            let options = INotebookFindOptions {
                regex: Some(self.state.is_regex()),
                whole_word: Some(self.state.whole_word()),
                case_sensitive: Some(self.state.match_case()),
                word_separators,
                include_markup_input: Some(true),
                include_code_input: Some(false),
                include_markup_preview: Some(false),
                include_output: Some(false),
                find_scope: Some(find_scope.clone()),
            };

            let content_matches = view_model.find(&self.state.search_string().to_string_lossy(), &options);
            for i in 0..view_model.length() {
                let Some(cell) = view_model.cell_at(i as i64) else { continue };
                if cell.cell_kind() == CellKind::Markup {
                    let found_content_match = content_matches.iter().find(|m| m.cell.handle() == cell.handle() && !m.content_matches.is_empty());
                    let target_state = if found_content_match.is_some() { CellEditState::Editing } else { CellEditState::Preview };
                    let current_editing_state = cell.base().get_edit_state();

                    if current_editing_state == CellEditState::Editing && cell.base().edit_state_source() != "find" {
                        // it's already in editing mode, we should not update
                        continue;
                    }
                    if current_editing_state != target_state {
                        cell.base().update_edit_state(target_state, "find");
                    }
                }
            }
        };

        if e.is_replace_revealed && !self.state.is_replace_revealed() {
            // replace is hidden, we need to switch all markdown cells to preview mode
            let Some(view_model) = self.notebook_editor.get_view_model() else {
                return;
            };

            for i in 0..view_model.length() {
                let Some(cell) = view_model.cell_at(i as i64) else { continue };
                if cell.cell_kind() == CellKind::Markup && cell.base().get_edit_state() == CellEditState::Editing && cell.base().edit_state_source() == "find" {
                    cell.base().update_edit_state(CellEditState::Preview, "find");
                }
            }

            return;
        }

        if e.is_replace_revealed {
            update_editing_state();
        } else if (e.filters || e.is_revealed || e.search_string || e.replace_string) && self.state.is_revealed() && self.state.is_replace_revealed() {
            update_editing_state();
        }
    }

    pub fn ensure_find_matches(&self) {
        if self.find_matches_starts.borrow().is_none() {
            let matches = self.find_matches.borrow().clone();
            self.set(matches, true);
        }
    }

    pub fn get_current_match(&self) -> CurrentMatchInfo {
        let next_index = self.find_matches_starts.borrow().as_ref().expect("ensureFindMatches antes").get_index_of(self.current_match.get());
        let matches = self.find_matches.borrow();
        let cell_matches = &matches[next_index.index];
        CurrentMatchInfo {
            cell: cell_matches.cell.clone(),
            match_: cell_matches.get_match(next_index.remainder as usize),
            is_model_match: (next_index.remainder as usize) < cell_matches.content_matches.len(),
        }
    }

    fn total_matches(&self) -> usize {
        self.find_matches.borrow().iter().map(CellMatches::length).sum()
    }

    fn change_match_info(&self) {
        self.state.change_match_info(self.current_match.get().max(0) as usize, self.total_matches(), None);
    }

    /// `refreshCurrentMatch(focus)`.
    pub fn refresh_current_match(&self, focus: (&CellViewModel, &Range)) {
        let (focus_cell, focus_range) = focus;
        let find_match_index = self.find_matches.borrow().iter().position(|m| m.cell == *focus_cell);
        let Some(find_match_index) = find_match_index else {
            return;
        };

        let index = self.find_matches.borrow()[find_match_index].content_matches.iter().position(|m| m.range.intersect_ranges(focus_range).is_some());
        let index = index.map_or(-1, |index| index as i64);

        let matches_before =
            if find_match_index == 0 { 0 } else { self.find_matches_starts.borrow().as_ref().map_or(0, |starts| starts.get_prefix_sum(find_match_index as i64 - 1) as i64) };
        self.current_match.set(matches_before + index);

        let this = self.this.upgrade().expect("o modelo está vivo");
        let highlighted = self.highlight_current_find_match_decoration(find_match_index, index.max(0) as usize);
        let _ = highlighted.then_promise(move |offset| {
            async_fn(async move {
                this.reveal_cell_range(find_match_index, index.max(0) as usize, offset).await?;
                this.change_match_info();
                Ok(())
            })
        });
    }

    /// `find(option)`.
    pub fn find(&self, option: FindOption) {
        if self.find_matches.borrow().is_empty() {
            return;
        }

        // let currCell;
        if self.find_matches_starts.borrow().is_none() {
            let matches = self.find_matches.borrow().clone();
            self.set(matches, true);
            if let FindOption::Index(index) = option {
                self.current_match.set(index);
            }
        } else {
            let total_val = i64::from(self.find_matches_starts.borrow().as_ref().expect("os inícios existem").get_total_sum());
            if let FindOption::Index(index) = option {
                self.current_match.set(index);
            } else if self.current_match.get() == -1 {
                let previous = matches!(option, FindOption::Previous(true));
                self.current_match.set(if previous { total_val - 1 } else { 0 });
            } else {
                let previous = matches!(option, FindOption::Previous(true));
                let next_val = (self.current_match.get() + if previous { -1 } else { 1 } + total_val) % total_val;
                self.current_match.set(next_val);
            }
        }

        let next_index = self.find_matches_starts.borrow().as_ref().expect("os inícios existem").get_index_of(self.current_match.get());
        let this = self.this.upgrade().expect("o modelo está vivo");
        let highlighted = self.highlight_current_find_match_decoration(next_index.index, next_index.remainder as usize);
        let _ = highlighted.then_promise(move |offset| {
            async_fn(async move {
                this.reveal_cell_range(next_index.index, next_index.remainder as usize, offset).await?;
                this.change_match_info();
                Ok(())
            })
        });
    }

    fn reveal_cell_range(self: &Rc<Self>, cell_index: usize, match_index: usize, output_offset: Option<f64>) -> Promise<()> {
        let (cell, content_len, match_) = {
            let matches = self.find_matches.borrow();
            let find_match = &matches[cell_index];
            (find_match.cell.clone(), find_match.content_matches.len(), if match_index < find_match.length() { Some(find_match.get_match(match_index)) } else { None })
        };
        if match_index >= content_len {
            // reveal output range
            self.notebook_editor.focus_element(&cell);
            let index = self.notebook_editor.get_cell_index(&cell);
            if index >= 0 {
                self.notebook_editor.reveal_cell_offset_in_center(&cell, output_offset.unwrap_or(0.0));
            }
            Promise::resolved(())
        } else {
            let Some(CellMatch::Content(content_match)) = match_ else {
                return Promise::resolved(());
            };
            if cell.base().get_edit_state() != CellEditState::Editing {
                cell.base().update_edit_state(CellEditState::Editing, "find");
            }
            cell.base().set_input_collapsed(false);
            self.notebook_editor.focus_element(&cell);
            self.notebook_editor.set_cell_editor_selection(&cell, &content_match.range);
            // First ensure the cell is visible in the notebook viewport
            let this = self.clone();
            self.notebook_editor.reveal_in_view(&cell).then_promise(move |()| {
                // Then reveal the specific range within the cell editor
                let _ = this.notebook_editor.reveal_range_in_center_if_outside_viewport_async(&cell, &content_match.range);
                Promise::resolved(())
            })
        }
    }

    fn register_model_listener(&self, notebook_text_model: Option<Rc<NotebookTextModel>>) {
        self.model_disposable.clear();

        if let Some(notebook_text_model) = notebook_text_model {
            let weak = self.this.clone();
            self.model_disposable.add(notebook_text_model.on_did_change_content().listen(move |e| {
                if !e.raw_events.iter().any(|event| matches!(event.kind(), NotebookCellsChangeType::ChangeCellContent | NotebookCellsChangeType::ModelChange)) {
                    return;
                }

                if let Some(this) = weak.upgrade() {
                    let _ = this.research();
                }
            }));
        }

        let _ = self.research();
    }

    /// `research()`.
    pub fn research(&self) -> Promise<()> {
        let weak = self.this.clone();
        self.throttled_delayer.trigger(
            move || {
                MaybePromise::Promise(async_fn(async move {
                    let Some(this) = weak.upgrade() else { return Ok(()) };
                    this.state.change(INewFindReplaceState { is_searching: Some(true), ..Default::default() }, false, None);
                    this.research_inner().await?;
                    this.state.change(INewFindReplaceState { is_searching: Some(false), ..Default::default() }, false, None);
                    Ok(())
                }))
            },
            None,
        )
    }

    /// `_research()`.
    pub fn research_inner(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let previous = this.compute_promise.borrow().clone();
            if let Some(previous) = previous {
                previous.cancel();
            }

            if !this.state.is_revealed() || !this.notebook_editor.has_model() {
                this.set(Vec::new(), false);
                return Ok(());
            }

            let compute = this.clone();
            let compute_promise = create_cancelable_promise(move |token| compute.compute(token));
            *this.compute_promise.borrow_mut() = Some(compute_promise.clone());

            let find_matches = compute_promise.await?;
            let Some(find_matches) = find_matches else {
                this.set(Vec::new(), false);
                return Ok(());
            };

            if find_matches.is_empty() {
                this.set(Vec::new(), false);
                return Ok(());
            }

            this.restore_current_match(find_matches);
            Ok(())
        })
    }

    /// O resto de `_research()`, depois que as ocorrências chegaram.
    fn restore_current_match(&self, find_matches: Vec<CellMatches>) {
        let find_first_match_after_cell_index = |cell_index: i64| {
            let indexes: Vec<usize> = find_matches.iter().map(|m| m.index).collect();
            let match_after_selection = find_first_idx_monotonous_or_arr_len(&indexes, |index| *index as i64 >= cell_index, 0, indexes.len());
            self.update_current_match(&find_matches, self.matches_count_before_index(&find_matches, match_after_selection));
        };

        if self.current_match.get() == -1 {
            // no active current match
            if self.notebook_editor.get_length() == 0 {
                self.set(find_matches, false);
                return;
            } else {
                let focus = self.notebook_editor.get_focus().start;
                find_first_match_after_cell_index(focus);
                self.set(find_matches.clone(), false);
                return;
            }
        }

        let old_curr_index = self.find_matches_starts.borrow().as_ref().expect("os inícios existem").get_index_of(self.current_match.get());
        let old_curr_cell = self.find_matches.borrow()[old_curr_index.index].cell.clone();
        let old_curr_match_cell_index = self.notebook_editor.get_cell_index(&old_curr_cell);

        if old_curr_match_cell_index < 0 {
            // the cell containing the active match is deleted
            if self.notebook_editor.get_length() == 0 {
                self.set(find_matches, false);
                return;
            }

            find_first_match_after_cell_index(old_curr_match_cell_index);
            return;
        }

        // the cell still exist
        let cell = self.notebook_editor.cell_at(old_curr_match_cell_index).expect("a célula ainda existe");
        // we will try restore the active find match in this cell, if it contains any find match

        if cell.cell_kind() == CellKind::Markup && cell.base().get_edit_state() == CellEditState::Preview {
            // find first match in this cell or below
            find_first_match_after_cell_index(old_curr_match_cell_index);
            return;
        }

        // the cell is a markup cell in editing mode or a code cell, both should have monaco editor rendered

        let Some(current_match_decorations) = self.find_match_decoration_model.current_match_decorations() else {
            // no current highlight decoration
            find_first_match_after_cell_index(old_curr_match_cell_index);
            return;
        };

        // check if there is monaco editor selection and find the first match, otherwise find the first match above current cell
        match current_match_decorations {
            CurrentMatchDecorations::Input { decorations } => {
                let current_match_decoration_id = decorations.iter().find(|decoration| decoration.owner_id == cell.handle()).cloned();

                let Some(current_match_decoration_id) = current_match_decoration_id else {
                    // current match decoration is no longer valid
                    find_first_match_after_cell_index(old_curr_match_cell_index);
                    return;
                };

                let match_after_selection =
                    find_first_idx_monotonous_or_arr_len(&find_matches, |m| m.index as i64 >= old_curr_match_cell_index, 0, find_matches.len()) % find_matches.len();
                if find_matches[match_after_selection].index as i64 > old_curr_match_cell_index {
                    // there is no search result in curr cell anymore, find the nearest one (from top to bottom)
                    self.update_current_match(&find_matches, self.matches_count_before_index(&find_matches, match_after_selection));
                } else {
                    // there are still some search results in current cell
                    let mut curr_match_range_in_editor: Option<Range> = match current_match_decoration_id.decorations.first() {
                        Some(decoration) if cell.base().editor_attached() => cell.base().get_cell_decoration_range(decoration),
                        _ => None,
                    };

                    let old_cell_matches = self.find_matches.borrow()[old_curr_index.index].content_matches.len() as i64;
                    if curr_match_range_in_editor.is_none() && old_curr_index.remainder < old_cell_matches {
                        let matches = self.find_matches.borrow();
                        if let CellMatch::Content(content_match) = matches[old_curr_index.index].get_match(old_curr_index.remainder as usize) {
                            curr_match_range_in_editor = Some(content_match.range);
                        }
                    }

                    if let Some(range_in_editor) = curr_match_range_in_editor {
                        // we find a range for the previous current match, let's find the nearest one after it (can overlap)
                        let cell_match = &find_matches[match_after_selection];
                        let match_after_old_selection = find_first_idx_monotonous_or_arr_len(
                            &cell_match.content_matches,
                            |m| Range::compare_ranges_using_starts(Some(&m.range), Some(&range_in_editor)) >= 0,
                            0,
                            cell_match.content_matches.len(),
                        );
                        self.update_current_match(&find_matches, self.matches_count_before_index(&find_matches, match_after_selection) + match_after_old_selection);
                    } else {
                        // no range found, let's fall back to finding the nearest match
                        self.update_current_match(&find_matches, self.matches_count_before_index(&find_matches, match_after_selection));
                    }
                }
            }
            CurrentMatchDecorations::Output { .. } => {
                // output now has the highlight
                let indexes: Vec<usize> = find_matches.iter().map(|m| m.index).collect();
                let match_after_selection = find_first_idx_monotonous_or_arr_len(&indexes, |index| *index as i64 >= old_curr_match_cell_index, 0, indexes.len()) % find_matches.len();
                self.update_current_match(&find_matches, self.matches_count_before_index(&find_matches, match_after_selection));
            }
        }
    }

    fn set(&self, cell_find_matches: Vec<CellMatches>, auto_start: bool) {
        if cell_find_matches.is_empty() {
            self.find_matches.borrow_mut().clear();
            self.find_match_decoration_model.set_all_find_matches_decorations(&[]);

            self.construct_find_matches_starts();
            self.current_match.set(-1);
            self.find_match_decoration_model.clear_current_find_match_decoration();

            self.change_match_info();
            return;
        }

        // all matches
        *self.find_matches.borrow_mut() = cell_find_matches.clone();
        self.find_match_decoration_model.set_all_find_matches_decorations(&cell_find_matches);

        // current match
        self.construct_find_matches_starts();

        if auto_start {
            self.current_match.set(0);
            let _ = self.highlight_current_find_match_decoration(0, 0);
        }

        self.change_match_info();
    }

    fn compute(&self, token: CancellationToken) -> Promise<Option<Vec<CellMatches>>> {
        if !self.notebook_editor.has_model() {
            return Promise::resolved(None);
        }
        let val = self.state.search_string().to_string_lossy();
        let word_separators = self.word_separators();
        let filters = self.state.filters();

        let options = INotebookFindOptions {
            regex: Some(self.state.is_regex()),
            whole_word: Some(self.state.whole_word()),
            case_sensitive: Some(self.state.match_case()),
            word_separators,
            include_markup_input: Some(filters.as_ref().map(NotebookFindFilters::markup_input).unwrap_or(true)),
            include_code_input: Some(filters.as_ref().map(NotebookFindFilters::code_input).unwrap_or(true)),
            include_markup_preview: Some(filters.as_ref().is_some_and(NotebookFindFilters::markup_preview)),
            include_output: Some(filters.as_ref().is_some_and(NotebookFindFilters::code_output)),
            find_scope: filters.as_ref().map(NotebookFindFilters::find_scope),
        };

        self.notebook_editor.find(&val, &options, token.clone()).then(move |ret| if token.is_cancellation_requested() { None } else { Some(ret) })
    }

    fn update_current_match(&self, find_matches: &[CellMatches], current_matches_position: usize) {
        self.current_match.set((current_matches_position % find_matches.len()) as i64);
        self.set(find_matches.to_vec(), false);
        let next_index = self.find_matches_starts.borrow().as_ref().expect("os inícios existem").get_index_of(self.current_match.get());
        let _ = self.highlight_current_find_match_decoration(next_index.index, next_index.remainder as usize);

        self.change_match_info();
    }

    fn matches_count_before_index(&self, find_matches: &[CellMatches], index: usize) -> usize {
        find_matches.iter().take(index).map(CellMatches::length).sum()
    }

    fn construct_find_matches_starts(&self) {
        let matches = self.find_matches.borrow();
        if !matches.is_empty() {
            let values: Vec<u32> = matches.iter().map(|m| m.length() as u32).collect();
            *self.find_matches_starts.borrow_mut() = Some(PrefixSumComputer::new(values));
        } else {
            *self.find_matches_starts.borrow_mut() = None;
        }
    }

    fn highlight_current_find_match_decoration(&self, cell_index: usize, match_index: usize) -> Promise<Option<f64>> {
        let (cell, content_len, match_) = {
            let matches = self.find_matches.borrow();
            let cell_matches = &matches[cell_index];
            (cell_matches.cell.clone(), cell_matches.content_matches.len(), cell_matches.get_match(match_index))
        };

        match match_ {
            CellMatch::Content(content_match) if match_index < content_len => self.find_match_decoration_model.highlight_current_find_match_decoration_in_cell(&cell, &content_match.range),
            CellMatch::Content(_) => Promise::resolved(None),
            CellMatch::Webview(webview_match) => self.find_match_decoration_model.highlight_current_find_match_decoration_in_webview(&cell, webview_match.index),
        }
    }

    pub fn clear(&self) {
        let compute_promise = self.compute_promise.borrow().clone();
        if let Some(compute_promise) = compute_promise {
            compute_promise.cancel();
        }
        self.throttled_delayer.cancel();
        self.set(Vec::new(), false);
    }
}

impl IDisposable for FindModel {
    fn dispose(&self) {
        self.find_match_decoration_model.dispose();
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "FindModel"
    }
}
