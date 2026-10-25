//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellFocus.ts`: `CellFocusPart`, que leva o foco do container
//! da célula (e do "ralo" de foco das saídas) para o editor de notebook.
//!
//! Divergências:
//! - `INotebookEditor` é o contrato estreito [`ICellFocusEditor`], só com `focusElement` e `focusNotebookCell`.
//! - `(this.currentCell as CodeCellViewModel).outputsViewModels.length` lança `TypeError` numa célula de markup no original (o campo
//!   não existe); o porte lê só em células de código e ignora o foco no ralo de uma célula de markup.

use std::rc::{Rc, Weak};

use je_ui::{EventKind, NodeId, Ui};

use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState};
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;

/// O que `CellFocusPart` chama do `INotebookEditor`.
pub trait ICellFocusEditor {
    /// `focusElement(cell)`.
    fn focus_element(&self, cell: &CellViewModel);
    /// `focusNotebookCell(cell, focusItem)` com `focusItem` em `'editor' | 'container' | 'output'`.
    fn focus_notebook_cell(&self, cell: &CellViewModel, focus_item: &str);
}

/// `CellFocusPart`.
pub struct CellFocusPart {
    state: CellPartState,
}

impl CellFocusPart {
    pub fn new(ui: &Ui, container_element: NodeId, focus_sink_element: Option<NodeId>, notebook_editor: Rc<dyn ICellFocusEditor>) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| {
            let state = CellPartState::new();

            {
                let (me, notebook_editor) = (me.clone(), notebook_editor.clone());
                state.register(ui.listen(container_element, EventKind::Focus, true, move |_| {
                    if let Some(cell) = me.upgrade().and_then(|part| part.state.current_cell()) {
                        notebook_editor.focus_element(&cell);
                    }
                }));
            }

            if let Some(focus_sink_element) = focus_sink_element {
                let (me, notebook_editor) = (me.clone(), notebook_editor.clone());
                state.register(ui.listen(focus_sink_element, EventKind::Focus, false, move |_| {
                    let Some(cell) = me.upgrade().and_then(|part| part.state.current_cell()) else {
                        return;
                    };
                    if cell.as_code().is_some_and(|code| !code.outputs_view_models().is_empty()) {
                        notebook_editor.focus_notebook_cell(&cell, "output");
                    }
                }));
            }

            Self { state }
        })
    }
}

impl CellPart for CellFocusPart {
    fn state(&self) -> &CellPartState {
        &self.state
    }
}

impl_cell_content_part!(CellFocusPart, "CellFocusPart");
