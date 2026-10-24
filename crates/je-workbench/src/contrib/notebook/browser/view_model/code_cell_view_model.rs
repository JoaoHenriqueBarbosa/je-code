//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/codeCellViewModel.ts`: o view model da célula de código
//! (`CodeCellViewModel`), com o cálculo de altura (editor, saídas, barra de status) e as saídas.
//!
//! Divergências:
//! - `extends BaseCellViewModel` é a composição com `Deref` para o [`BaseCellViewModel`] (ver as divergências dele).
//! - `INotebookService`, `ITextModelService`, `ICodeEditorService` e `IInlineChatSessionService` (que o construtor original
//!   recebe) não existem na camada lógica; o [`CellOutputViewModel`] não precisa do `INotebookService` (ver a divergência dele).
//! - `set editorHeight`/`get editorHeight` (que lança) são [`CodeCellViewModel::set_editor_height`] e nenhum leitor (o `get`
//!   que lançava não existe); o mesmo vale para `chatHeight`, que lê.
//! - `executionErrorDiagnostic = observableValue(...)` é um `ISettableObservable<Option<Rc<ICellExecutionError>>>`
//!   ([`CodeCellViewModel::execution_error_diagnostic`]); o objeto do erro é um `Rc` para que o `===` do observável seja a identidade.
//! - `cell.layoutInfo` é uma cópia (`CodeCellLayoutInfo` é `Clone`); `layoutChange(state, source)` recebe o
//!   [`CellLayoutChangeEvent`] com `source` dentro do evento (o `{ ...state, source }` do original).
//! - `Uint32Array` do `PrefixSumComputer` é `Vec<u32>`: as alturas são truncadas para inteiro sem sinal.
//! - `startFind` devolve `Option<CellFindMatch>` com `cell` sendo o [`CellViewModel`] dono, que o chamador informa, porque o
//!   `this` do original só existe como `Rc<CodeCellViewModel>` aqui.

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use je_base::event::{Emitter, Event, PauseableEmitter};
use je_base::lifecycle::IDisposable;
use je_base::observable::{ISettableObservable, SettableExt, observable_value};
use je_base::uuid::generate_uuid;
use je_editor::prefix_sum_computer::PrefixSumComputer;
use je_editor::undo_redo_service::IUndoRedoService;
use je_platform::configuration::service::IConfigurationService;

use super::base_cell_view_model::BaseCellViewModel;
use super::cell_output_view_model::CellOutputViewModel;
use super::cell_view_model_types::{CellFindMatch, CellLayoutChangeEvent, CellLayoutState, CodeCellLayoutInfo};
use super::notebook_view_model_impl::CellViewModel;
use super::view_context::ViewContext;
use crate::contrib::notebook::browser::notebook_browser::CellEditState;
use crate::contrib::notebook::browser::notebook_options::NotebookOptionsChangeEvent;
use crate::contrib::notebook::browser::notebook_view_events::{CellViewModelStateChangeEvent, NotebookLayoutInfo};
use crate::contrib::notebook::common::model::notebook_cell_text_model::NotebookCellTextModel;
use crate::contrib::notebook::common::notebook_common::{CellKind, ICell, INotebookFindOptions, NotebookCellOutputsSplice};
use crate::contrib::notebook::common::notebook_execution_state_service::{ICellExecutionError, ICellExecutionStateChangedEvent};

pub const OUTPUT_DISPLAY_LIMIT: usize = 500;

/// `CodeCellViewModel`.
pub struct CodeCellViewModel {
    base: Rc<BaseCellViewModel>,
    pub view_context: Rc<ViewContext>,

    on_layout_info_read: Rc<Emitter<()>>,
    on_did_start_execution: Rc<Emitter<Rc<dyn ICellExecutionStateChangedEvent>>>,
    on_did_stop_execution: Rc<Emitter<Rc<dyn ICellExecutionStateChangedEvent>>>,
    on_did_change_outputs: Rc<Emitter<NotebookCellOutputsSplice>>,
    on_did_remove_outputs: Rc<Emitter<Vec<Rc<CellOutputViewModel>>>>,
    has_find_result: Rc<Emitter<bool>>,
    pauseable_emitter: Rc<PauseableEmitter<CellLayoutChangeEvent>>,

    output_collection: RefCell<Vec<u32>>,
    outputs_top: RefCell<Option<PrefixSumComputer>>,
    editor_height: Cell<f64>,
    chat_height: Cell<f64>,
    hovering_output: Cell<bool>,
    focus_on_output: Cell<bool>,
    focus_input_in_output: Cell<bool>,
    output_min_height: Cell<f64>,
    layout_info: RefCell<CodeCellLayoutInfo>,
    output_view_models: RefCell<Vec<Rc<CellOutputViewModel>>>,
    execution_error_diagnostic: ISettableObservable<Option<Rc<ICellExecutionError>>>,
}

impl Deref for CodeCellViewModel {
    type Target = BaseCellViewModel;

    fn deref(&self) -> &BaseCellViewModel {
        &self.base
    }
}

impl CodeCellViewModel {
    pub fn new(
        view_type: &str,
        model: Rc<NotebookCellTextModel>,
        initial_notebook_layout_info: Option<&NotebookLayoutInfo>,
        view_context: Rc<ViewContext>,
        configuration_service: Rc<dyn IConfigurationService>,
        undo_redo_service: Rc<dyn IUndoRedoService>,
    ) -> Rc<Self> {
        let id = generate_uuid();
        let base = BaseCellViewModel::new(view_type, model.clone(), id.clone(), view_context.clone(), configuration_service, undo_redo_service);
        let output_view_models: Vec<Rc<CellOutputViewModel>> = model.outputs().into_iter().map(|output| CellOutputViewModel::new(&id, output)).collect();

        let on_layout_info_read = Emitter::<()>::new();
        let on_did_start_execution = Emitter::new();
        let on_did_stop_execution = Emitter::new();
        let on_did_change_outputs = Emitter::new();
        let on_did_remove_outputs = Emitter::new();
        let has_find_result = Emitter::new();
        let pauseable_emitter = PauseableEmitter::<CellLayoutChangeEvent>::new();
        base.register(on_layout_info_read.clone());
        base.register(on_did_start_execution.clone());
        base.register(on_did_stop_execution.clone());
        base.register(on_did_change_outputs.clone());
        base.register(on_did_remove_outputs.clone());
        base.register(has_find_result.clone());
        base.register(pauseable_emitter.clone());

        let output_count = model.outputs().len();
        let layout_configuration = view_context.notebook_options.get_layout_configuration();

        let this = Rc::new(Self {
            base,
            view_context: view_context.clone(),
            on_layout_info_read,
            on_did_start_execution,
            on_did_stop_execution,
            on_did_change_outputs,
            on_did_remove_outputs,
            has_find_result,
            pauseable_emitter,
            output_collection: RefCell::new(vec![0; output_count]),
            outputs_top: RefCell::new(None),
            editor_height: Cell::new(0.0),
            chat_height: Cell::new(0.0),
            hovering_output: Cell::new(false),
            focus_on_output: Cell::new(false),
            focus_input_in_output: Cell::new(false),
            output_min_height: Cell::new(0.0),
            layout_info: RefCell::new(CodeCellLayoutInfo {
                font_info: initial_notebook_layout_info.map(|info| info.font_info.clone()),
                editor_height: 0.0,
                editor_width: initial_notebook_layout_info.map(|info| view_context.notebook_options.compute_code_cell_editor_width(info.width)).unwrap_or(0.0),
                chat_height: 0.0,
                status_bar_height: 0.0,
                comment_offset: 0.0,
                comment_height: 0.0,
                output_container_offset: 0.0,
                output_total_height: 0.0,
                output_show_more_container_height: 0.0,
                output_show_more_container_offset: 0.0,
                total_height: 0.0,
                code_indicator_height: 0.0,
                output_indicator_height: 0.0,
                bottom_toolbar_offset: 0.0,
                layout_state: CellLayoutState::Uninitialized,
                estimated_has_horizontal_scrolling: false,
                outline_width: 1.0,
                top_margin: layout_configuration.cell_top_margin,
                bottom_margin: layout_configuration.cell_bottom_margin,
            }),
            output_view_models: RefCell::new(output_view_models),
            execution_error_diagnostic: observable_value::<Option<Rc<ICellExecutionError>>, ()>("executionErrorDiagnostic", None),
        });
        this.layout_info.borrow_mut().total_height = this.compute_total_height(17.0, 0.0, 0.0, 0.0);

        {
            let weak = Rc::downgrade(&this);
            this.base.register(model.on_did_change_outputs().listen(move |splice| {
                let Some(this) = weak.upgrade() else { return };
                let mut removed_outputs: Vec<Rc<CellOutputViewModel>> = Vec::new();
                let mut output_layout_change = false;
                for i in splice.start..splice.start + splice.delete_count {
                    if this.output_collection.borrow().get(i).is_some_and(|height| *height != 0) {
                        output_layout_change = true;
                    }
                }

                {
                    let mut collection = this.output_collection.borrow_mut();
                    let start = splice.start.min(collection.len());
                    let end = (start + splice.delete_count).min(collection.len());
                    collection.splice(start..end, splice.new_outputs.iter().map(|_| 0));
                }
                {
                    let mut view_models = this.output_view_models.borrow_mut();
                    let start = splice.start.min(view_models.len());
                    let end = (start + splice.delete_count).min(view_models.len());
                    let new_view_models: Vec<Rc<CellOutputViewModel>> = splice.new_outputs.iter().map(|output| CellOutputViewModel::new(&this.base.id, output.clone())).collect();
                    removed_outputs.extend(view_models.splice(start..end, new_view_models));
                }

                *this.outputs_top.borrow_mut() = None;
                this.on_did_change_outputs.fire(splice.clone());
                this.on_did_remove_outputs.fire(removed_outputs.clone());
                if output_layout_change {
                    this.layout_change(CellLayoutChangeEvent { output_height: true, source: Some("CodeCellViewModel#model.onDidChangeOutputs".to_string()), ..Default::default() });
                }
                if this.output_collection.borrow().is_empty() {
                    this.execution_error_diagnostic.set(None, None);
                }
                for removed in &removed_outputs {
                    removed.dispose();
                }
            }));
        }

        {
            // O ouvinte de `onDidChangeInternalMetadata` da base que chama `this.layoutChange({})`.
            let weak = Rc::downgrade(&this);
            this.base.register(model.on_did_change_internal_metadata().listen(move |e| {
                let Some(this) = weak.upgrade() else { return };
                this.base.fire_did_change_state(CellViewModelStateChangeEvent { internal_metadata_changed: true, ..Default::default() });
                if e.last_run_success_changed == Some(true) {
                    // Statusbar visibility may change
                    this.layout_change(CellLayoutChangeEvent::default());
                }
            }));
        }

        this
    }

    pub fn cell_kind(&self) -> CellKind {
        CellKind::Code
    }

    pub fn on_layout_info_read(&self) -> Event<()> {
        self.on_layout_info_read.event()
    }

    pub fn on_did_start_execution(&self) -> Event<Rc<dyn ICellExecutionStateChangedEvent>> {
        self.on_did_start_execution.event()
    }

    pub fn on_did_stop_execution(&self) -> Event<Rc<dyn ICellExecutionStateChangedEvent>> {
        self.on_did_stop_execution.event()
    }

    pub fn on_did_change_outputs(&self) -> Event<NotebookCellOutputsSplice> {
        self.on_did_change_outputs.event()
    }

    pub fn on_did_remove_outputs(&self) -> Event<Vec<Rc<CellOutputViewModel>>> {
        self.on_did_remove_outputs.event()
    }

    pub fn on_did_change_layout(&self) -> Event<CellLayoutChangeEvent> {
        self.pauseable_emitter.event()
    }

    pub fn has_find_result(&self) -> Event<bool> {
        self.has_find_result.event()
    }

    pub fn set_editor_height(&self, height: f64) {
        if self.editor_height.get() == height {
            return;
        }

        self.editor_height.set(height);
        self.layout_change(CellLayoutChangeEvent { editor_height: true, source: Some("CodeCellViewModel#editorHeight".to_string()), ..Default::default() });
    }

    pub fn set_chat_height(&self, height: f64) {
        if self.chat_height.get() == height {
            return;
        }

        self.chat_height.set(height);
        self.layout_change(CellLayoutChangeEvent { chat_height: true, source: Some("CodeCellViewModel#chatHeight".to_string()), ..Default::default() });
    }

    pub fn chat_height(&self) -> f64 {
        self.chat_height.get()
    }

    /// O setter de `commentHeight` da base, que chama `layoutChange({ commentHeight: true }, ...)`.
    pub fn set_comment_height(&self, height: f64) {
        if self.base.comment_height.get() == height {
            return;
        }
        self.base.comment_height.set(height);
        self.layout_change(CellLayoutChangeEvent { comment_height: true, source: Some("BaseCellViewModel#commentHeight".to_string()), ..Default::default() });
    }

    pub fn output_is_hovered(&self) -> bool {
        self.hovering_output.get()
    }

    pub fn set_output_is_hovered(&self, v: bool) {
        self.hovering_output.set(v);
        self.base.fire_did_change_state(CellViewModelStateChangeEvent { output_is_hovered_changed: true, ..Default::default() });
    }

    pub fn output_is_focused(&self) -> bool {
        self.focus_on_output.get()
    }

    pub fn set_output_is_focused(&self, v: bool) {
        self.focus_on_output.set(v);
        self.base.fire_did_change_state(CellViewModelStateChangeEvent { output_is_focused_changed: true, ..Default::default() });
    }

    pub fn input_in_output_is_focused(&self) -> bool {
        self.focus_input_in_output.get()
    }

    pub fn set_input_in_output_is_focused(&self, v: bool) {
        self.focus_input_in_output.set(v);
    }

    /// The minimum height of the output region. It's only set to non-zero temporarily when replacing an output with a new one.
    /// It's reset to 0 when the new output is rendered, or in one second.
    pub fn output_min_height(&self) -> f64 {
        self.output_min_height.get()
    }

    pub fn layout_info(&self) -> CodeCellLayoutInfo {
        self.layout_info.borrow().clone()
    }

    pub fn outputs_view_models(&self) -> Vec<Rc<CellOutputViewModel>> {
        self.output_view_models.borrow().clone()
    }

    /// `executionErrorDiagnostic`: o observável, que quem lê (`.get()`, `.read(reader)`) e escreve (`.set(valor, None)`) compartilha.
    pub fn execution_error_diagnostic(&self) -> ISettableObservable<Option<Rc<ICellExecutionError>>> {
        self.execution_error_diagnostic.clone()
    }

    pub fn update_execution_state(&self, e: &Rc<dyn ICellExecutionStateChangedEvent>) {
        if e.changed().is_some() {
            self.execution_error_diagnostic.set(None, None);
            self.on_did_start_execution.fire(e.clone());
        } else {
            self.on_did_stop_execution.fire(e.clone());
        }
    }

    pub fn update_options(&self, e: &NotebookOptionsChangeEvent) {
        self.base.update_options(e);
        if e.cell_status_bar_visibility || e.insert_toolbar_position || e.cell_toolbar_location {
            self.layout_change(CellLayoutChangeEvent::default());
        }
    }

    pub fn pause_layout(&self) {
        self.pauseable_emitter.pause();
    }

    pub fn resume_layout(&self) {
        self.pauseable_emitter.resume();
    }

    pub fn layout_change(&self, state: CellLayoutChangeEvent) {
        // recompute
        self.ensure_outputs_top();
        let options = &self.view_context.notebook_options;
        let notebook_layout_configuration = options.get_layout_configuration();
        let bottom_toolbar_dimensions = options.compute_bottom_toolbar_dimensions(Some(&self.base.view_type));
        let current = self.layout_info.borrow().clone();
        let output_show_more_container_height = match state.output_show_more_container_height {
            Some(height) if height != 0.0 => height,
            _ => current.output_show_more_container_height,
        };
        let output_total_height = self.output_min_height.get().max(if self.base.is_output_collapsed() {
            notebook_layout_configuration.collapsed_indicator_height
        } else {
            self.outputs_top.borrow().as_ref().expect("outputsTop").get_total_sum() as f64
        });
        let comment_height = if state.comment_height { self.base.comment_height.get() } else { current.comment_height };

        let original_layout = current.clone();
        let new_layout;
        if !self.base.is_input_collapsed() {
            let new_state;
            let editor_height;
            let total_height;
            let mut has_horizontal_scrolling = false;
            let chat_height = if state.chat_height { self.chat_height.get() } else { current.chat_height };
            if !state.editor_height && current.layout_state == CellLayoutState::FromCache && !state.output_height {
                // No new editorHeight info - keep cached totalHeight and estimate editorHeight
                let estimate = self.estimate_editor_height(state.font.as_ref().map(|font| font.line_height).or(current.font_info.as_ref().map(|font| font.line_height)));
                editor_height = estimate.0;
                has_horizontal_scrolling = estimate.1;
                total_height = current.total_height;
                new_state = CellLayoutState::FromCache;
            } else if state.editor_height || current.layout_state == CellLayoutState::Measured {
                // Editor has been measured
                editor_height = self.editor_height.get();
                total_height = self.compute_total_height(self.editor_height.get(), output_total_height, output_show_more_container_height, chat_height);
                new_state = CellLayoutState::Measured;
                has_horizontal_scrolling = current.estimated_has_horizontal_scrolling;
            } else {
                let estimate = self.estimate_editor_height(state.font.as_ref().map(|font| font.line_height).or(current.font_info.as_ref().map(|font| font.line_height)));
                editor_height = estimate.0;
                has_horizontal_scrolling = estimate.1;
                total_height = self.compute_total_height(editor_height, output_total_height, output_show_more_container_height, chat_height);
                new_state = CellLayoutState::Estimated;
            }

            let status_bar_height = options.compute_editor_statusbar_height(&self.base.internal_metadata(), &self.base.uri());
            let code_indicator_height = editor_height + status_bar_height;
            let output_indicator_height = output_total_height + output_show_more_container_height;
            let output_container_offset = notebook_layout_configuration.editor_toolbar_height + notebook_layout_configuration.cell_top_margin + chat_height + editor_height + status_bar_height;
            let output_show_more_container_offset =
                total_height - bottom_toolbar_dimensions.bottom_toolbar_gap - bottom_toolbar_dimensions.bottom_toolbar_height / 2.0 - output_show_more_container_height;
            let bottom_toolbar_offset = options.compute_bottom_toolbar_offset(total_height, &self.base.view_type);
            let editor_width = match state.outer_width {
                Some(outer_width) => options.compute_code_cell_editor_width(outer_width),
                None => current.editor_width,
            };

            new_layout = CodeCellLayoutInfo {
                font_info: state.font.clone().or(current.font_info.clone()),
                chat_height,
                editor_height,
                editor_width,
                status_bar_height,
                output_container_offset,
                output_total_height,
                output_show_more_container_height,
                output_show_more_container_offset,
                comment_offset: output_container_offset + output_total_height,
                comment_height,
                total_height,
                code_indicator_height,
                output_indicator_height,
                bottom_toolbar_offset,
                layout_state: new_state,
                estimated_has_horizontal_scrolling: has_horizontal_scrolling,
                top_margin: notebook_layout_configuration.cell_top_margin,
                bottom_margin: notebook_layout_configuration.cell_bottom_margin,
                outline_width: 1.0,
            };
        } else {
            let code_indicator_height = notebook_layout_configuration.collapsed_indicator_height;
            let output_indicator_height = output_total_height + output_show_more_container_height;
            let chat_height = if state.chat_height { self.chat_height.get() } else { current.chat_height };

            let output_container_offset = notebook_layout_configuration.cell_top_margin + notebook_layout_configuration.collapsed_indicator_height;
            let total_height = notebook_layout_configuration.cell_top_margin
                + notebook_layout_configuration.collapsed_indicator_height
                + notebook_layout_configuration.cell_bottom_margin
                + bottom_toolbar_dimensions.bottom_toolbar_gap
                + chat_height
                + comment_height
                + output_total_height
                + output_show_more_container_height;
            let output_show_more_container_offset =
                total_height - bottom_toolbar_dimensions.bottom_toolbar_gap - bottom_toolbar_dimensions.bottom_toolbar_height / 2.0 - output_show_more_container_height;
            let bottom_toolbar_offset = options.compute_bottom_toolbar_offset(total_height, &self.base.view_type);
            let editor_width = match state.outer_width {
                Some(outer_width) => options.compute_code_cell_editor_width(outer_width),
                None => current.editor_width,
            };

            new_layout = CodeCellLayoutInfo {
                font_info: state.font.clone().or(current.font_info.clone()),
                editor_height: current.editor_height,
                editor_width,
                chat_height,
                status_bar_height: 0.0,
                output_container_offset,
                output_total_height,
                output_show_more_container_height,
                output_show_more_container_offset,
                comment_offset: output_container_offset + output_total_height,
                comment_height,
                total_height,
                code_indicator_height,
                output_indicator_height,
                bottom_toolbar_offset,
                layout_state: current.layout_state,
                estimated_has_horizontal_scrolling: false,
                outline_width: 1.0,
                top_margin: notebook_layout_configuration.cell_top_margin,
                bottom_margin: notebook_layout_configuration.cell_bottom_margin,
            };
        }

        let total_height_changed = new_layout.total_height != original_layout.total_height;
        *self.layout_info.borrow_mut() = new_layout;

        let mut event = state;
        event.total_height_changed = total_height_changed;
        self.pauseable_emitter.fire(event);
    }

    pub fn restore_editor_view_state(&self, editor_view_states: Option<je_base::dynamic::Dynamic>, total_height: Option<f64>) {
        self.base.restore_editor_view_state(editor_view_states);
        let mut layout_info = self.layout_info.borrow_mut();
        if let Some(total_height) = total_height
            && layout_info.layout_state != CellLayoutState::Measured
        {
            layout_info.total_height = total_height;
            layout_info.layout_state = CellLayoutState::FromCache;
        }
    }

    pub fn get_dynamic_height(&self) -> f64 {
        self.on_layout_info_read.fire(());
        self.layout_info.borrow().total_height
    }

    pub fn get_height(&self, line_height: f64) -> f64 {
        if self.layout_info.borrow().layout_state == CellLayoutState::Uninitialized {
            let estimate = self.estimate_editor_height(Some(line_height));
            self.compute_total_height(estimate.0, 0.0, 0.0, 0.0)
        } else {
            self.layout_info.borrow().total_height
        }
    }

    /// `estimateEditorHeight(lineHeight = 20)`: `(editorHeight, hasHorizontalScrolling)`.
    fn estimate_editor_height(&self, line_height: Option<f64>) -> (f64, bool) {
        let line_height = line_height.unwrap_or(20.0);
        let mut has_horizontal_scrolling = false;
        let cell_editor_options = self.view_context.get_base_cell_editor_options(&self.base.language());
        let layout_info = self.layout_info.borrow().clone();
        if let Some(font_info) = &layout_info.font_info
            && cell_editor_options.word_wrap() == "off"
        {
            for i in 0..self.base.line_count() {
                let max = self.base.text_buffer().borrow().get_line_last_non_whitespace_column(i as i32 + 1) as f64;
                let estimated_width = max * (font_info.typical_halfwidth_character_width + font_info.letter_spacing);
                if estimated_width > layout_info.editor_width {
                    has_horizontal_scrolling = true;
                    break;
                }
            }
        }

        let vertical_scrollbar_height = if has_horizontal_scrolling { 12.0 } else { 0.0 }; // take zoom level into account
        let editor_padding = self.view_context.notebook_options.compute_editor_padding(&self.base.internal_metadata(), &self.base.uri());
        let editor_height = self.base.line_count() as f64 * line_height + editor_padding.top + editor_padding.bottom + vertical_scrollbar_height;
        (editor_height, has_horizontal_scrolling)
    }

    fn compute_total_height(&self, editor_height: f64, outputs_total_height: f64, output_show_more_container_height: f64, chat_height: f64) -> f64 {
        let options = &self.view_context.notebook_options;
        let layout_configuration = options.get_layout_configuration();
        let bottom_toolbar_gap = options.compute_bottom_toolbar_dimensions(Some(&self.base.view_type)).bottom_toolbar_gap;
        layout_configuration.editor_toolbar_height
            + layout_configuration.cell_top_margin
            + chat_height
            + editor_height
            + options.compute_editor_statusbar_height(&self.base.internal_metadata(), &self.base.uri())
            + self.base.comment_height.get()
            + outputs_total_height
            + output_show_more_container_height
            + bottom_toolbar_gap
            + layout_configuration.cell_bottom_margin
    }

    /// `onDidChangeTextModelContent()`.
    pub fn on_did_change_text_model_content(&self) {
        if self.base.get_edit_state() != CellEditState::Editing {
            self.base.update_edit_state(CellEditState::Editing, "onDidChangeTextModelContent");
            self.base.fire_did_change_state(CellViewModelStateChangeEvent { content_changed: true, ..Default::default() });
        }
    }

    pub fn on_deselect(&self) {
        self.base.update_edit_state(CellEditState::Preview, "onDeselect");
    }

    pub fn update_output_show_more_container_height(&self, height: f64) {
        self.layout_change(CellLayoutChangeEvent {
            output_show_more_container_height: Some(height),
            source: Some("CodeCellViewModel#updateOutputShowMoreContainerHeight".to_string()),
            ..Default::default()
        });
    }

    pub fn update_output_min_height(&self, height: f64) {
        self.output_min_height.set(height);
    }

    pub fn unlock_output_height(&self) {
        self.output_min_height.set(0.0);
        self.layout_change(CellLayoutChangeEvent { output_height: true, ..Default::default() });
    }

    pub fn update_output_height(&self, index: usize, height: f64, source: Option<&str>) {
        if index >= self.output_collection.borrow().len() {
            errors_throw("Output index out of range!");
        }

        self.ensure_outputs_top();

        let output_view_model = self.output_view_models.borrow()[index].clone();
        if index == 0 || height > 0.0 {
            output_view_model.set_visible(true, false);
        } else if height == 0.0 {
            output_view_model.set_visible(false, false);
        }

        let mut height = height;
        if output_view_model.visible() && height < 28.0 {
            height = 28.0;
        }

        self.output_collection.borrow_mut()[index] = height as u32;
        let changed = self.outputs_top.borrow_mut().as_mut().expect("outputsTop").set_value(index, height as u32);
        if changed {
            self.layout_change(CellLayoutChangeEvent { output_height: true, source: source.map(str::to_string), ..Default::default() });
        }
    }

    pub fn get_output_offset_in_container(&self, index: usize) -> f64 {
        self.ensure_outputs_top();

        if index >= self.output_collection.borrow().len() {
            errors_throw("Output index out of range!");
        }

        self.outputs_top.borrow().as_ref().expect("outputsTop").get_prefix_sum(index as i64 - 1) as f64
    }

    pub fn get_output_offset(&self, index: usize) -> f64 {
        self.layout_info.borrow().output_container_offset + self.get_output_offset_in_container(index)
    }

    pub fn splice_output_heights(&self, start: usize, delete_cnt: usize, heights: &[f64]) {
        self.ensure_outputs_top();

        {
            let mut outputs_top = self.outputs_top.borrow_mut();
            let outputs_top = outputs_top.as_mut().expect("outputsTop");
            outputs_top.remove_values(start, delete_cnt);
            if !heights.is_empty() {
                let values: Vec<u32> = heights.iter().map(|height| *height as u32).collect();
                outputs_top.insert_values(start, &values);
            }
        }

        self.layout_change(CellLayoutChangeEvent { output_height: true, source: Some("CodeCellViewModel#spliceOutputs".to_string()), ..Default::default() });
    }

    fn ensure_outputs_top(&self) {
        if self.outputs_top.borrow().is_none() {
            *self.outputs_top.borrow_mut() = Some(PrefixSumComputer::new(self.output_collection.borrow().clone()));
        }
    }

    /// `startFind(value, options)`; `this_cell` é o [`CellViewModel`] que envolve esta célula.
    pub fn start_find(&self, this_cell: &CellViewModel, value: &str, options: &INotebookFindOptions) -> Option<CellFindMatch> {
        let matches = self.base.cell_start_find(value, options)?;
        Some(CellFindMatch { cell: this_cell.clone(), content_matches: matches })
    }
}

fn errors_throw(message: &str) -> ! {
    je_base::errors::throw(je_base::errors::Error::new(message))
}

impl IDisposable for CodeCellViewModel {
    fn dispose(&self) {
        self.base.dispose();

        self.output_collection.borrow_mut().clear();
        *self.outputs_top.borrow_mut() = None;
        for output in self.output_view_models.borrow().iter() {
            output.dispose();
        }
    }

    fn constructor_name(&self) -> &'static str {
        "CodeCellViewModel"
    }
}
