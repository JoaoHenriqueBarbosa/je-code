//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/markupCellViewModel.ts`: o view model da célula de markdown
//! (`MarkupCellViewModel`).
//!
//! Divergências:
//! - `extends BaseCellViewModel` é a composição com `Deref` para o [`BaseCellViewModel`] (ver as divergências dele).
//! - `foldingDelegate: EditorFoldingStateDelegate` é um `RefCell<Option<Rc<dyn EditorFoldingStateDelegate>>>` (o `dispose` do
//!   original o anula). Como o delegate recebe a célula (`getCellIndex(this)`), o método [`MarkupCellViewModel::folding_state`]
//!   recebe o [`CellViewModel`] que envolve esta célula.
//! - `set renderedHtml`/`get renderedHtml` são [`MarkupCellViewModel::set_rendered_html`] e `rendered_html`.
//! - `startFind` recebe o [`CellViewModel`] dono (ver o `CodeCellViewModel`).
//! - Os serviços do editor de código não existem na camada lógica (ver a base).

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::uuid::generate_uuid;
use je_editor::undo_redo_service::IUndoRedoService;
use je_platform::configuration::service::IConfigurationService;

use super::base_cell_view_model::BaseCellViewModel;
use super::cell_view_model_types::{CellFindMatch, CellFoldingState, CellLayoutChangeEvent, CellLayoutContext, CellLayoutState, EditorFoldingStateDelegate, MarkupCellLayoutInfo};
use super::notebook_view_model_impl::CellViewModel;
use super::view_context::ViewContext;
use crate::contrib::notebook::browser::notebook_browser::CellEditState;
use crate::contrib::notebook::browser::notebook_options::NotebookOptionsChangeEvent;
use crate::contrib::notebook::browser::notebook_view_events::{CellViewModelStateChangeEvent, NotebookCellStateChangedEvent, NotebookLayoutInfo, NotebookViewEvent};
use crate::contrib::notebook::common::model::notebook_cell_text_model::NotebookCellTextModel;
use crate::contrib::notebook::common::notebook_common::{CellKind, ICell, INotebookFindOptions};

/// `MarkupCellViewModel`.
pub struct MarkupCellViewModel {
    base: Rc<BaseCellViewModel>,
    pub view_context: Rc<ViewContext>,
    folding_delegate: RefCell<Option<Rc<dyn EditorFoldingStateDelegate>>>,
    layout_info: RefCell<MarkupCellLayoutInfo>,
    rendered_html: RefCell<Option<String>>,
    preview_height: Cell<f64>,
    chat_height: Cell<f64>,
    editor_height: Cell<f64>,
    status_bar_height: Cell<f64>,
    on_did_change_layout: Rc<Emitter<CellLayoutChangeEvent>>,
    has_find_result: Rc<Emitter<bool>>,
    hovering_output: Cell<bool>,
    focus_on_output: Cell<bool>,
    hovering_cell: Cell<bool>,
}

impl Deref for MarkupCellViewModel {
    type Target = BaseCellViewModel;

    fn deref(&self) -> &BaseCellViewModel {
        &self.base
    }
}

impl MarkupCellViewModel {
    pub fn new(
        view_type: &str,
        model: Rc<NotebookCellTextModel>,
        initial_notebook_layout_info: Option<&NotebookLayoutInfo>,
        folding_delegate: Rc<dyn EditorFoldingStateDelegate>,
        view_context: Rc<ViewContext>,
        configuration_service: Rc<dyn IConfigurationService>,
        undo_redo_service: Rc<dyn IUndoRedoService>,
    ) -> Rc<Self> {
        let base = BaseCellViewModel::new(view_type, model.clone(), generate_uuid(), view_context.clone(), configuration_service, undo_redo_service);
        let on_did_change_layout = Emitter::<CellLayoutChangeEvent>::new();
        let has_find_result = Emitter::<bool>::new();
        base.register(on_did_change_layout.clone());
        base.register(has_find_result.clone());

        let bottom_toolbar_gap = view_context.notebook_options.compute_bottom_toolbar_dimensions(Some(view_type)).bottom_toolbar_gap;
        let layout_configuration = view_context.notebook_options.get_layout_configuration();
        let layout_info = MarkupCellLayoutInfo {
            chat_height: 0.0,
            editor_height: 0.0,
            preview_height: 0.0,
            font_info: initial_notebook_layout_info.map(|info| info.font_info.clone()),
            editor_width: match initial_notebook_layout_info {
                Some(info) if info.width != 0.0 => view_context.notebook_options.compute_markdown_cell_editor_width(info.width),
                _ => 0.0,
            },
            comment_offset: 0.0,
            comment_height: 0.0,
            bottom_toolbar_offset: bottom_toolbar_gap,
            total_height: 100.0,
            layout_state: CellLayoutState::Uninitialized,
            fold_hint_height: 0.0,
            status_bar_height: 0.0,
            outline_width: 1.0,
            bottom_margin: layout_configuration.markdown_cell_bottom_margin,
            top_margin: layout_configuration.markdown_cell_top_margin,
        };

        let this = Rc::new(Self {
            base,
            view_context: view_context.clone(),
            folding_delegate: RefCell::new(Some(folding_delegate)),
            layout_info: RefCell::new(layout_info),
            rendered_html: RefCell::new(None),
            preview_height: Cell::new(0.0),
            chat_height: Cell::new(0.0),
            editor_height: Cell::new(0.0),
            status_bar_height: Cell::new(0.0),
            on_did_change_layout,
            has_find_result,
            hovering_output: Cell::new(false),
            focus_on_output: Cell::new(false),
            hovering_cell: Cell::new(false),
        });

        {
            let weak = Rc::downgrade(&this);
            let model = model.clone();
            this.base.register(this.base.on_did_change_state().listen(move |e| {
                let Some(this) = weak.upgrade() else { return };
                this.view_context.event_dispatcher.emit(vec![NotebookViewEvent::CellStateChanged(NotebookCellStateChangedEvent { source: *e, cell: model.clone() })]);

                if e.folding_state_changed {
                    let total_height = this.compute_total_height();
                    this.update_total_height(total_height, Some(CellLayoutContext::Fold));
                }
            }));
        }

        {
            // O ouvinte de `onDidChangeInternalMetadata` da base: `layoutChange({})` quando `lastRunSuccessChanged`.
            let weak = Rc::downgrade(&this);
            this.base.register(model.on_did_change_internal_metadata().listen(move |e| {
                let Some(this) = weak.upgrade() else { return };
                this.base.fire_did_change_state(CellViewModelStateChangeEvent { internal_metadata_changed: true, ..Default::default() });
                if e.last_run_success_changed == Some(true) {
                    this.layout_change(CellLayoutChangeEvent::default());
                }
            }));
        }

        this
    }

    pub fn cell_kind(&self) -> CellKind {
        CellKind::Markup
    }

    pub fn rendered_html(&self) -> Option<String> {
        self.rendered_html.borrow().clone()
    }

    pub fn set_rendered_html(&self, value: Option<String>) {
        if *self.rendered_html.borrow() != value {
            *self.rendered_html.borrow_mut() = value;
            self.base.fire_did_change_state(CellViewModelStateChangeEvent { content_changed: true, ..Default::default() });
        }
    }

    pub fn layout_info(&self) -> MarkupCellLayoutInfo {
        self.layout_info.borrow().clone()
    }

    pub fn set_rendered_markdown_height(&self, new_height: f64) {
        self.preview_height.set(new_height);
        let total_height = self.compute_total_height();
        self.update_total_height(total_height, None);
    }

    pub fn set_chat_height(&self, new_height: f64) {
        self.chat_height.set(new_height);
        let total_height = self.compute_total_height();
        self.update_total_height(total_height, None);
    }

    pub fn chat_height(&self) -> f64 {
        self.chat_height.get()
    }

    pub fn set_editor_height(&self, new_height: f64) {
        self.editor_height.set(new_height);
        self.status_bar_height.set(self.view_context.notebook_options.compute_status_bar_height());
        let total_height = self.compute_total_height();
        self.update_total_height(total_height, None);
    }

    /// O setter de `commentHeight` da base, que chama `layoutChange({ commentHeight: true }, ...)`.
    pub fn set_comment_height(&self, height: f64) {
        if self.base.comment_height.get() == height {
            return;
        }
        self.base.comment_height.set(height);
        self.layout_change(CellLayoutChangeEvent { comment_height: true, source: Some("BaseCellViewModel#commentHeight".to_string()), ..Default::default() });
    }

    pub fn on_did_change_layout(&self) -> Event<CellLayoutChangeEvent> {
        self.on_did_change_layout.event()
    }

    pub fn has_find_result(&self) -> Event<bool> {
        self.has_find_result.event()
    }

    /// `get foldingState()`; `this_cell` é o [`CellViewModel`] que envolve esta célula.
    pub fn folding_state(&self, this_cell: &CellViewModel) -> CellFoldingState {
        let delegate = self.folding_delegate.borrow().clone().expect("foldingDelegate");
        delegate.get_folding_state(delegate.get_cell_index(this_cell))
    }

    pub fn output_is_hovered(&self) -> bool {
        self.hovering_output.get()
    }

    pub fn set_output_is_hovered(&self, v: bool) {
        self.hovering_output.set(v);
    }

    pub fn output_is_focused(&self) -> bool {
        self.focus_on_output.get()
    }

    pub fn set_output_is_focused(&self, v: bool) {
        self.focus_on_output.set(v);
    }

    pub fn input_in_output_is_focused(&self) -> bool {
        false
    }

    pub fn set_input_in_output_is_focused(&self, _v: bool) {}

    pub fn cell_is_hovered(&self) -> bool {
        self.hovering_cell.get()
    }

    pub fn set_cell_is_hovered(&self, v: bool) {
        self.hovering_cell.set(v);
        self.base.fire_did_change_state(CellViewModelStateChangeEvent { cell_is_hovered_changed: true, ..Default::default() });
    }

    fn compute_total_height(&self) -> f64 {
        let options = &self.view_context.notebook_options;
        let layout_configuration = options.get_layout_configuration();
        let bottom_toolbar_gap = options.compute_bottom_toolbar_dimensions(Some(&self.base.view_type)).bottom_toolbar_gap;
        let fold_hint_height = self.compute_fold_hint_height_for_current();

        if self.base.get_edit_state() == CellEditState::Editing {
            self.editor_height.get()
                + layout_configuration.markdown_cell_top_margin
                + layout_configuration.markdown_cell_bottom_margin
                + bottom_toolbar_gap
                + self.status_bar_height.get()
                + self.base.comment_height.get()
        } else {
            // @rebornix
            // On file open, the previewHeight + bottomToolbarGap for a cell out of viewport can be 0
            // When it's 0, the list view will never try to render it anymore even if we scroll the cell into view.
            // Thus we make sure it's greater than 0
            (self.preview_height.get() + bottom_toolbar_gap + fold_hint_height + self.base.comment_height.get()).max(1.0)
        }
    }

    /// `_computeFoldHintHeight()`. O `foldingState` do original lê o delegate com `this`; a célula envolvente é recuperada
    /// pelo delegate por identidade do `NotebookCellTextModel` (o [`CellViewModel`] que tem este `base`).
    fn compute_fold_hint_height_for_current(&self) -> f64 {
        let delegate = self.folding_delegate.borrow().clone();
        let collapsed = match delegate {
            Some(delegate) => delegate.get_folding_state_by_handle(self.base.handle()) == CellFoldingState::Collapsed,
            None => false,
        };
        if self.base.get_edit_state() == CellEditState::Editing || !collapsed {
            0.0
        } else {
            self.view_context.notebook_options.get_layout_configuration().markdown_fold_hint_height
        }
    }

    pub fn update_options(&self, e: &NotebookOptionsChangeEvent) {
        self.base.update_options(e);
        if e.cell_status_bar_visibility || e.insert_toolbar_position || e.cell_toolbar_location {
            let total_height = self.compute_total_height();
            self.update_total_height(total_height, None);
        }
    }

    pub fn trigger_folding_state_change(&self) {
        self.base.fire_did_change_state(CellViewModelStateChangeEvent { folding_state_changed: true, ..Default::default() });
    }

    fn update_total_height(&self, new_height: f64, context: Option<CellLayoutContext>) {
        if new_height != self.layout_info.borrow().total_height {
            self.layout_change(CellLayoutChangeEvent { total_height: Some(new_height), context, ..Default::default() });
        }
    }

    pub fn layout_change(&self, mut state: CellLayoutChangeEvent) {
        let options = &self.view_context.notebook_options;
        let current = self.layout_info.borrow().clone();
        let total_height;
        let fold_hint_height;
        if !self.base.is_input_collapsed() {
            total_height = match state.total_height {
                None => {
                    if current.layout_state == CellLayoutState::Uninitialized {
                        100.0
                    } else {
                        current.total_height
                    }
                }
                Some(height) => height,
            };
            // recompute
            fold_hint_height = self.compute_fold_hint_height_for_current();
        } else {
            total_height = options.compute_collapsed_markdown_cell_height(&self.base.view_type);
            state.total_height = Some(total_height);

            fold_hint_height = 0.0;
        }
        let notebook_layout_configuration = options.get_layout_configuration();
        let comment_offset = if self.base.get_edit_state() == CellEditState::Editing {
            notebook_layout_configuration.editor_toolbar_height
                + notebook_layout_configuration.cell_top_margin
                + self.chat_height.get()
                + self.editor_height.get()
                + self.status_bar_height.get()
        } else {
            self.preview_height.get()
        };

        *self.layout_info.borrow_mut() = MarkupCellLayoutInfo {
            font_info: state.font.clone().or(current.font_info.clone()),
            editor_width: match state.outer_width {
                Some(outer_width) => options.compute_markdown_cell_editor_width(outer_width),
                None => current.editor_width,
            },
            chat_height: self.chat_height.get(),
            editor_height: self.editor_height.get(),
            status_bar_height: self.status_bar_height.get(),
            preview_height: self.preview_height.get(),
            bottom_toolbar_offset: options.compute_bottom_toolbar_offset(total_height, &self.base.view_type),
            total_height,
            layout_state: CellLayoutState::Measured,
            fold_hint_height,
            comment_offset,
            comment_height: if state.comment_height { self.base.comment_height.get() } else { current.comment_height },
            outline_width: 1.0,
            bottom_margin: notebook_layout_configuration.markdown_cell_bottom_margin,
            top_margin: notebook_layout_configuration.markdown_cell_top_margin,
        };

        self.on_did_change_layout.fire(state);
    }

    pub fn restore_editor_view_state(&self, editor_view_states: Option<je_base::dynamic::Dynamic>, total_height: Option<f64>) {
        self.base.restore_editor_view_state(editor_view_states);
        // we might already warmup the viewport so the cell has a total height computed
        let restore = total_height.filter(|_| self.layout_info.borrow().layout_state == CellLayoutState::Uninitialized);
        if let Some(total_height) = restore {
            {
                let mut layout_info = self.layout_info.borrow_mut();
                layout_info.total_height = total_height;
                layout_info.chat_height = self.chat_height.get();
                layout_info.editor_height = self.editor_height.get();
                layout_info.status_bar_height = self.status_bar_height.get();
                layout_info.layout_state = CellLayoutState::FromCache;
            }
            self.layout_change(CellLayoutChangeEvent::default());
        }
    }

    pub fn get_dynamic_height(&self) -> Option<f64> {
        None
    }

    pub fn get_height(&self, _line_height: f64) -> f64 {
        if self.layout_info.borrow().layout_state == CellLayoutState::Uninitialized {
            100.0
        } else {
            self.layout_info.borrow().total_height
        }
    }

    /// `onDidChangeTextModelContent()`.
    pub fn on_did_change_text_model_content(&self) {
        self.base.fire_did_change_state(CellViewModelStateChangeEvent { content_changed: true, ..Default::default() });
    }

    pub fn on_deselect(&self) {}

    pub fn start_find(&self, this_cell: &CellViewModel, value: &str, options: &INotebookFindOptions) -> Option<CellFindMatch> {
        let matches = self.base.cell_start_find(value, options)?;
        Some(CellFindMatch { cell: this_cell.clone(), content_matches: matches })
    }
}

impl IDisposable for MarkupCellViewModel {
    fn dispose(&self) {
        self.base.dispose();
        *self.folding_delegate.borrow_mut() = None;
    }

    fn constructor_name(&self) -> &'static str {
        "MarkupCellViewModel"
    }
}
