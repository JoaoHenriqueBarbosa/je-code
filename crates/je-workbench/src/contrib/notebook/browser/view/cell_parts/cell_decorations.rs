//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellDecorations.ts`: `CellDecorations`, que traduz as decorações
//! da célula (`getCellDecorations`) em classes no container raiz da célula e em elementos de decoração no topo.
//!
//! Divergências:
//! - `rootContainer.classList` e `decorationContainer` são nós da árvore do `je-ui`: `/^nb\-.*$/` é `starts_with("nb-")`,
//!   `innerText = ''` é `clear_node`, e `append(DOM.$('.classe'))` é `create_el`.
//! - `INotebookEditorDelegate` é o contrato estreito [`ICellDecorationsEditor`], só com `deltaCellContainerClassNames`.
//! - `options.className` e `options.topClassName` são testados por veracidade onde o original usa `&&` e `find` (texto vazio conta como
//!   ausente); o filtro `topClassName !== undefined` do original é `is_some`.

use std::rc::{Rc, Weak};

use je_ui::{NodeId, Role, Ui};
use je_widgets::dom::{clear_node, create_el};

use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState};
use crate::contrib::notebook::browser::view_model::base_cell_view_model::INotebookCellDecorationOptions;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_common::CellKind;

/// O que `CellDecorations` chama do `INotebookEditorDelegate`.
pub trait ICellDecorationsEditor {
    /// `deltaCellContainerClassNames(cellId, added, removed, cellKind)`.
    fn delta_cell_container_class_names(&self, cell_id: &str, added: &[String], removed: &[String], cell_kind: CellKind);
}

/// Texto que o `&&` e o `find` do original tratam como presente.
fn truthy(text: &Option<String>) -> Option<&str> {
    text.as_deref().filter(|text| !text.is_empty())
}

fn has_top_class_name(options: &INotebookCellDecorationOptions) -> bool {
    truthy(&options.top_class_name).is_some()
}

/// `CellDecorations`.
pub struct CellDecorations {
    state: CellPartState,
    me: Weak<CellDecorations>,
    ui: Ui,
    notebook_editor: Rc<dyn ICellDecorationsEditor>,
    root_container: NodeId,
    decoration_container: NodeId,
}

impl CellDecorations {
    pub fn new(ui: &Ui, notebook_editor: Rc<dyn ICellDecorationsEditor>, root_container: NodeId, decoration_container: NodeId) -> Rc<Self> {
        Rc::new_cyclic(|me: &Weak<Self>| Self {
            state: CellPartState::new(),
            me: me.clone(),
            ui: ui.clone(),
            notebook_editor,
            root_container,
            decoration_container,
        })
    }

    fn register_decorations(&self) {
        let Some(cell) = self.state.current_cell() else {
            return;
        };

        {
            let me = self.me.clone();
            self.state.cell_disposables().add(cell.base().on_cell_decorations_changed().listen(move |e| {
                let Some(part) = me.upgrade() else { return };
                if part.state.current_cell().is_none() {
                    return;
                }

                let mut tree = part.ui.tree_mut();
                for options in &e.added {
                    if let Some(class_name) = truthy(&options.class_name) {
                        tree.add_class(part.root_container, class_name);
                    }
                }

                for options in &e.removed {
                    if let Some(class_name) = truthy(&options.class_name) {
                        tree.remove_class(part.root_container, class_name);
                    }
                }
            }));
        }

        for options in cell.base().get_cell_decorations() {
            if let Some(class_name) = truthy(&options.class_name) {
                self.ui.tree_mut().add_class(self.root_container, class_name);
                self.notebook_editor.delta_cell_container_class_names(&cell.id(), &[class_name.to_string()], &[], cell.cell_kind());
            }

            if let Some(output_class_name) = truthy(&options.output_class_name) {
                self.notebook_editor.delta_cell_container_class_names(&cell.id(), &[output_class_name.to_string()], &[], cell.cell_kind());
            }
        }
    }
}

impl CellPart for CellDecorations {
    fn state(&self) -> &CellPartState {
        &self.state
    }

    fn did_render_cell(&self, element: &CellViewModel) {
        {
            let mut tree = self.ui.tree_mut();
            let removed_class_names: Vec<String> = tree.classes(self.root_container).iter().filter(|class_name| class_name.starts_with("nb-")).cloned().collect();
            for class_name in &removed_class_names {
                tree.remove_class(self.root_container, class_name);
            }

            clear_node(&mut tree, self.decoration_container);
        }

        let generate_cell_top_decorations: Rc<dyn Fn()> = {
            let ui = self.ui.clone();
            let decoration_container = self.decoration_container;
            let element = element.clone();
            Rc::new(move || {
                let mut tree = ui.tree_mut();
                clear_node(&mut tree, decoration_container);

                for options in element.base().get_cell_decorations().iter().filter(|options| options.top_class_name.is_some()) {
                    if let Some(top_class_name) = options.top_class_name.as_deref() {
                        create_el(&mut tree, decoration_container, Role::Generic, top_class_name);
                    }
                }
            })
        };

        {
            let generate = generate_cell_top_decorations.clone();
            self.state.cell_disposables().add(element.base().on_cell_decorations_changed().listen(move |e| {
                let modified = e.added.iter().any(has_top_class_name) || e.removed.iter().any(has_top_class_name);

                if modified {
                    generate();
                }
            }));
        }

        generate_cell_top_decorations();
        self.register_decorations();
    }
}

impl_cell_content_part!(CellDecorations, "CellDecorations");
