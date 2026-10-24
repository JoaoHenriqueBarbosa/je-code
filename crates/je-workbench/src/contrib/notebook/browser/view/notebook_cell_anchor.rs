//! Porte de `src/vs/workbench/contrib/notebook/browser/view/notebookCellAnchor.ts`: decide se uma célula que cresce ou
//! encolhe deve ancorar a rolagem na célula com foco, e acompanha a execução da célula para continuar ancorando enquanto o
//! usuário não rolar para cima.
//!
//! Divergências:
//! - `IListView<CellViewModel>` é o trait [`ICellListView`], só com os membros que `shouldAnchor` lê (`element(i).focusMode`,
//!   `elementTop`, `elementHeight`, `renderHeight`, `getScrollTop`).
//! - `executionWatcher` e `scrollWatcher` (`IDisposable | undefined`) são `MutableDisposable`; limpar é descartar e
//!   desreferenciar de uma vez (no original, o `scrollWatcher` descartado no `onDidStopExecution` continua referenciado,
//!   o que não é observável).
//! - O `Event<ScrollEvent>` do `onDidScroll` da lista é recebido pelo construtor, como no original.

use std::cell::Cell;
use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::{IDisposable, MutableDisposable};
use je_base::scrollable::ScrollEvent;
use je_platform::configuration::service::IConfigurationService;

use crate::contrib::notebook::browser::view_model::cell_view_model_types::CellFocusMode;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_common::{CellKind, NotebookCellExecutionState, NotebookSetting};
use crate::contrib::notebook::common::notebook_execution_state_service::INotebookExecutionStateService;

/// O pedaço de `IListView<CellViewModel>` que o `NotebookCellAnchor` consulta.
pub trait ICellListView {
    /// `element(index).focusMode`.
    fn element_focus_mode(&self, index: usize) -> CellFocusMode;
    fn element_top(&self, index: usize) -> f64;
    fn element_height(&self, index: usize) -> f64;
    fn render_height(&self) -> f64;
    /// `getScrollTop()`.
    fn get_scroll_top(&self) -> f64;
}

/// `NotebookCellAnchor`.
pub struct NotebookCellAnchor {
    notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    configuration_service: Rc<dyn IConfigurationService>,
    scroll_event: Event<ScrollEvent>,
    stop_anchoring: Cell<bool>,
    execution_watcher: Rc<MutableDisposable<dyn IDisposable>>,
    scroll_watcher: Rc<MutableDisposable<dyn IDisposable>>,
}

impl NotebookCellAnchor {
    pub fn new(
        notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
        configuration_service: Rc<dyn IConfigurationService>,
        scroll_event: Event<ScrollEvent>,
    ) -> Rc<Self> {
        Rc::new(Self {
            notebook_execution_state_service,
            configuration_service,
            scroll_event,
            stop_anchoring: Cell::new(false),
            execution_watcher: MutableDisposable::<dyn IDisposable>::new(),
            scroll_watcher: MutableDisposable::<dyn IDisposable>::new(),
        })
    }

    pub fn should_anchor(self: &Rc<Self>, cell_list_view: &dyn ICellListView, focused_index: usize, height_delta: f64, executing_cell_uri: &CellViewModel) -> bool {
        if cell_list_view.element_focus_mode(focused_index) == CellFocusMode::Editor {
            return true;
        }
        if self.stop_anchoring.get() {
            return false;
        }

        let new_focus_bottom = cell_list_view.element_top(focused_index) + cell_list_view.element_height(focused_index) + height_delta;
        let view_bottom = cell_list_view.render_height() + cell_list_view.get_scroll_top();
        let focus_still_visible = view_bottom > new_focus_bottom;
        let allow_scrolling = !self.scroll_to_reveal_cell_is_none();
        let growing = height_delta > 0.0;
        let auto_anchor = allow_scrolling && growing && !focus_still_visible;

        if auto_anchor {
            self.watch_achor_during_execution(executing_cell_uri);
            return true;
        }

        false
    }

    /// `this.configurationService.getValue(NotebookSetting.scrollToRevealCell) !== 'none'`, negado.
    fn scroll_to_reveal_cell_is_none(&self) -> bool {
        match self.configuration_service.get_value(Some(NotebookSetting::SCROLL_TO_REVEAL_CELL), None) {
            je_base::dynamic::Dynamic::String(value) => value.to_string_lossy() == "none",
            _ => false,
        }
    }

    /// `watchAchorDuringExecution` (o erro de grafia é do original).
    pub fn watch_achor_during_execution(self: &Rc<Self>, executing_cell: &CellViewModel) {
        // anchor while the cell is executing unless the user scrolls up.
        if self.execution_watcher.value().is_some() || executing_cell.cell_kind() != CellKind::Code {
            return;
        }
        let Some(execution_state) = self.notebook_execution_state_service.get_cell_execution(&executing_cell.uri()) else { return };
        if execution_state.state() != NotebookCellExecutionState::Executing {
            return;
        }
        let Some(code_cell) = executing_cell.as_code() else { return };

        let weak = Rc::downgrade(self);
        self.execution_watcher.set(Some(code_cell.on_did_stop_execution().listen(move |_| {
            let Some(this) = weak.upgrade() else { return };
            this.execution_watcher.clear();
            this.scroll_watcher.clear();
            this.stop_anchoring.set(false);
        })));

        let weak = Rc::downgrade(self);
        self.scroll_watcher.set(Some(self.scroll_event.listen(move |scroll_event| {
            if scroll_event.scroll_top < scroll_event.old_scroll_top {
                let Some(this) = weak.upgrade() else { return };
                this.stop_anchoring.set(true);
                this.scroll_watcher.clear();
            }
        })));
    }

    pub fn dispose(&self) {
        // `this.executionWatcher?.dispose(); this.scrollWatcher?.dispose();`: os contêineres saem junto com o valor.
        self.execution_watcher.dispose();
        self.scroll_watcher.dispose();
    }
}

impl IDisposable for NotebookCellAnchor {
    fn dispose(&self) {
        NotebookCellAnchor::dispose(self);
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookCellAnchor"
    }
}
