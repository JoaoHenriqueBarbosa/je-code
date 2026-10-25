//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/collapsedCellInput.ts`: `CollapsedCellInput`, que expande a
//! entrada (ou a saída) de uma célula recolhida com o duplo clique, e a entrada com o clique no ícone `expandInputIcon`.
//!
//! Divergências:
//! - `INotebookEditor` é o contrato estreito [`ICollapsedCellInputEditor`], só com `hasModel()`.
//! - `element && element.classList && element.classList.contains('expandInputIcon')` é a classe do nó alvo do clique na árvore.

use std::rc::{Rc, Weak};

use je_ui::{EventKind, NodeId, Ui};

use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState};

/// O que `CollapsedCellInput` chama do `INotebookEditor`.
pub trait ICollapsedCellInputEditor {
    /// `hasModel()`.
    fn has_model(&self) -> bool;
}

/// `CollapsedCellInput`.
pub struct CollapsedCellInput {
    state: CellPartState,
    notebook_editor: Rc<dyn ICollapsedCellInputEditor>,
}

impl CollapsedCellInput {
    pub fn new(ui: &Ui, notebook_editor: Rc<dyn ICollapsedCellInputEditor>, cell_input_collapsed_container: NodeId) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| {
            let state = CellPartState::new();

            {
                let me = me.clone();
                state.register(ui.listen(cell_input_collapsed_container, EventKind::DoubleClick, false, move |_| {
                    let Some(part) = me.upgrade() else { return };
                    let Some(cell) = part.state.current_cell() else { return };
                    if !part.notebook_editor.has_model() {
                        return;
                    }

                    if cell.base().is_input_collapsed() {
                        cell.base().set_input_collapsed(false);
                    } else {
                        cell.base().set_output_collapsed(false);
                    }
                }));
            }

            {
                let (me, ui) = (me.clone(), ui.clone());
                state.register(ui.clone().listen(cell_input_collapsed_container, EventKind::Click, false, move |e| {
                    let Some(part) = me.upgrade() else { return };
                    let Some(cell) = part.state.current_cell() else { return };
                    if !part.notebook_editor.has_model() {
                        return;
                    }

                    if ui.tree().has_class(e.target, "expandInputIcon") {
                        // clicked on the expand icon
                        cell.base().set_input_collapsed(false);
                    }
                }));
            }

            Self { state, notebook_editor }
        })
    }
}

impl CellPart for CollapsedCellInput {
    fn state(&self) -> &CellPartState {
        &self.state
    }
}

impl_cell_content_part!(CollapsedCellInput, "CollapsedCellInput");
