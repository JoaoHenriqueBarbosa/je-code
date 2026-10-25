//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/collapsedCellOutput.ts`: `CollapsedCellOutput`, o aviso
//! "Outputs are collapsed" com o ícone de reticências, que expande a saída com o clique no ícone ou o duplo clique no container.
//!
//! Divergências:
//! - `EXPAND_CELL_OUTPUT_COMMAND_ID` (de `notebookBrowser.ts`, ainda sem porte) é a constante deste arquivo.
//! - `INotebookEditor` é o contrato estreito [`ICollapsedCellOutputEditor`], só com `textModel`.
//! - O texto do `span.expandOutputPlaceholder` é o rótulo do nó (`textContent` não existe na árvore); a fonte do output, o itálico, a
//!   opacidade 0,7 e a cor dependem do tema e são aplicados pelo `cellRenderer.ts` que monta o template. O `title` é o atributo
//!   `title` do nó.
//! - `ThemeIcon.asClassNameArray(Codicon.more)` é o par de classes `codicon` e `codicon-more`.
//! - O segundo `if (!this.currentCell) return;` de `expand` é repetição inócua do primeiro e não é portado.
//! - `localize('cellExpandOutputButtonLabel', "Expand Cell Output (${0})", ...)` mantém o `$` antes de `{0}` do texto original.

use std::rc::{Rc, Weak};

use je_base::nls::localize;
use je_platform::keybinding::keybinding::IKeybindingService;
use je_ui::{EventKind, NodeId, Role, Ui};
use je_widgets::dom::{create_el, hide};

use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState};
use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;

/// `EXPAND_CELL_OUTPUT_COMMAND_ID`.
pub const EXPAND_CELL_OUTPUT_COMMAND_ID: &str = "notebook.cell.expandCellOutput";

/// O que `CollapsedCellOutput` chama do `INotebookEditor`.
pub trait ICollapsedCellOutputEditor {
    /// `textModel!`.
    fn text_model(&self) -> Rc<NotebookTextModel>;
}

/// `CollapsedCellOutput`.
pub struct CollapsedCellOutput {
    state: CellPartState,
    notebook_editor: Rc<dyn ICollapsedCellOutputEditor>,
}

impl CollapsedCellOutput {
    pub fn new(
        ui: &Ui,
        notebook_editor: Rc<dyn ICollapsedCellOutputEditor>,
        cell_output_collapse_container: NodeId,
        keybinding_service: &dyn IKeybindingService,
    ) -> Rc<Self> {
        let expand_icon = {
            let mut tree = ui.tree_mut();
            let placeholder = create_el(&mut tree, cell_output_collapse_container, Role::Generic, "expandOutputPlaceholder");
            tree.set_label(placeholder, localize("cellOutputsCollapsedMsg", "Outputs are collapsed", &[]));
            let expand_icon = create_el(&mut tree, cell_output_collapse_container, Role::Generic, "expandOutputIcon");
            tree.add_class(expand_icon, "codicon");
            tree.add_class(expand_icon, "codicon-more");

            if let Some(label) = keybinding_service.lookup_keybinding(EXPAND_CELL_OUTPUT_COMMAND_ID, None, false).and_then(|keybinding| keybinding.get_label()) {
                tree.set_attribute(placeholder, "title", localize("cellExpandOutputButtonLabelWithDoubleClick", "Double-click to expand cell output ({0})", &[label.as_str()]));
                tree.set_attribute(cell_output_collapse_container, "title", localize("cellExpandOutputButtonLabel", "Expand Cell Output (${0})", &[label.as_str()]));
            }

            hide(&mut tree, cell_output_collapse_container);
            expand_icon
        };

        Rc::new_cyclic(|me: &Weak<Self>| {
            let state = CellPartState::new();

            for (node, kind) in [(expand_icon, EventKind::Click), (cell_output_collapse_container, EventKind::DoubleClick)] {
                let me = me.clone();
                state.register(ui.listen(node, kind, false, move |_| {
                    if let Some(part) = me.upgrade() {
                        part.expand();
                    }
                }));
            }

            Self { state, notebook_editor }
        })
    }

    fn expand(&self) {
        let Some(cell) = self.state.current_cell() else {
            return;
        };

        let text_model = self.notebook_editor.text_model();
        let model = cell.model();
        if !text_model.cells().iter().any(|candidate| Rc::ptr_eq(candidate, &model)) {
            return;
        }

        cell.base().set_output_collapsed(!cell.base().is_output_collapsed());
    }
}

impl CellPart for CollapsedCellOutput {
    fn state(&self) -> &CellPartState {
        &self.state
    }
}

impl_cell_content_part!(CollapsedCellOutput, "CollapsedCellOutput");
