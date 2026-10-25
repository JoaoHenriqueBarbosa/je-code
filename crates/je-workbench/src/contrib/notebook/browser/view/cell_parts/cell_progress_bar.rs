//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellProgressBar.ts`: `CellProgressBar`, as duas barras de
//! progresso infinito (a do editor e a da entrada recolhida) que aparecem enquanto a célula executa.
//!
//! Divergências:
//! - `defaultProgressBarStyles` (que lê o tema) chega como o argumento `styles` ([`IProgressBarStyles`]), montado por quem tem o tema
//!   com `default_progress_bar_styles`.
//! - `@INotebookExecutionStateService` é o argumento `notebook_execution_state_service`.
//! - `e?.changed ?? getCellExecution(uri)` mantém a semântica do `??`: um evento sem `changed` (execução terminada) também cai na
//!   consulta ao serviço.

use std::rc::Rc;

use je_ui::{NodeId, Ui};
use je_widgets::progress_bar::{IProgressBarOptions, IProgressBarStyles, ProgressBar};

use crate::contrib::notebook::browser::notebook_view_events::CellViewModelStateChangeEvent;
use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState};
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_common::NotebookCellExecutionState;
use crate::contrib::notebook::common::notebook_execution_state_service::{ICellExecutionStateChangedEvent, INotebookExecutionStateService};

/// `CellProgressBar`.
pub struct CellProgressBar {
    state: CellPartState,
    notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    progress_bar: Rc<ProgressBar>,
    collapsed_progress_bar: Rc<ProgressBar>,
}

impl CellProgressBar {
    pub fn new(
        ui: &Ui,
        editor_container: NodeId,
        collapsed_input_container: NodeId,
        styles: IProgressBarStyles,
        notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    ) -> Rc<Self> {
        let state = CellPartState::new();
        let options = IProgressBarOptions { styles, aria_label: None };

        let progress_bar = ProgressBar::new(ui, editor_container, Some(&options));
        state.register(progress_bar.clone());
        progress_bar.hide();

        let collapsed_progress_bar = ProgressBar::new(ui, collapsed_input_container, Some(&options));
        state.register(collapsed_progress_bar.clone());
        collapsed_progress_bar.hide();

        Rc::new(Self { state, notebook_execution_state_service, progress_bar, collapsed_progress_bar })
    }

    fn update_for_execution_state_of(&self, element: &CellViewModel, e: Option<&Rc<dyn ICellExecutionStateChangedEvent>>) {
        let exe_state = e.and_then(|e| e.changed()).or_else(|| self.notebook_execution_state_service.get_cell_execution(&element.uri()));
        let is_input_collapsed = element.base().is_input_collapsed();
        let progress_bar = if is_input_collapsed { &self.collapsed_progress_bar } else { &self.progress_bar };
        match exe_state {
            Some(exe_state) if exe_state.state() == NotebookCellExecutionState::Executing && (!exe_state.did_pause() || is_input_collapsed) => {
                show_progress_bar(progress_bar);
            }
            _ => progress_bar.hide(),
        }
    }
}

impl CellPart for CellProgressBar {
    fn state(&self) -> &CellPartState {
        &self.state
    }

    fn did_render_cell(&self, element: &CellViewModel) {
        self.update_for_execution_state_of(element, None);
    }

    fn update_for_execution_state(&self, element: &CellViewModel, e: &Rc<dyn ICellExecutionStateChangedEvent>) {
        self.update_for_execution_state_of(element, Some(e));
    }

    fn update_state(&self, element: &CellViewModel, e: &CellViewModelStateChangeEvent) {
        if e.metadata_changed || e.internal_metadata_changed {
            self.update_for_execution_state_of(element, None);
        }

        if e.input_collapsed_changed {
            let exe_state = self.notebook_execution_state_service.get_cell_execution(&element.uri());
            let is_executing = exe_state.is_some_and(|exe_state| exe_state.state() == NotebookCellExecutionState::Executing);
            if element.base().is_input_collapsed() {
                self.progress_bar.hide();
            } else {
                self.collapsed_progress_bar.hide();
            }
            if is_executing {
                self.update_for_execution_state_of(element, None);
            }
        }
    }
}

fn show_progress_bar(progress_bar: &ProgressBar) {
    progress_bar.infinite();
    progress_bar.show(Some(500));
}

impl_cell_content_part!(CellProgressBar, "CellProgressBar");
