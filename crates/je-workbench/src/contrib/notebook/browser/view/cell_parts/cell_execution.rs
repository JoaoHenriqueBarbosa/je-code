//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellExecution.ts`: `CellExecutionPart`, o rótulo `[n]` da ordem
//! de execução ao lado do editor da célula, que acompanha a rolagem e vira um ícone girando enquanto a célula executa fora da tela.
//!
//! Divergências:
//! - O `HTMLElement` do rótulo e o `div` interno são nós da árvore do `je-ui`. `innerText = '[1]'` é o rótulo do nó interno
//!   (`textContent` não existe na árvore), depois de remover os filhos (o `innerText` os substitui); `style.top = '...px'` é o `y0`
//!   do nó do rótulo, mantendo a altura; `DOM.hide` e `DOM.show` são `display: none`.
//! - `renderLabelWithIcons('$(icon)')` anexado ao `div` interno é o [`SimpleIconLabel`] sobre ele (ele limpa os filhos e monta o ícone).
//!   O estilo do texto (cor, fonte) chega como o argumento `label_styles`, montado por quem tem o tema.
//! - `executingStateIcon` (`notebookIcons.ts`, sem porte) é `registerIcon('notebook-state-executing', Codicon.sync)`: o ícone
//!   registrado resolve para o glifo padrão `sync`, que `code_cell_execution_icon.rs` expõe (uma troca do ícone pelo tema não é
//!   honrada).
//! - `INotebookEditorDelegate` é o contrato estreito [`ICellExecutionEditor`]. `activeKernel` é o `Rc<dyn INotebookKernel>` e
//!   `activeKernel.onDidChange` é o evento do kernel; `onDidScroll` carrega `()` porque a parte só reage ao disparo.
//! - `hasKey(e, { affectsCell: true })` é o `match` na variante de célula do [`ExecutionStateChange`].
//! - `this.currentCell!` em `updateExecutionOrder`: sem célula atual o original lançaria; aqui a chamada não faz nada.
//! - `executionOrderContent.classList.contains('sticky-spinner')` testa o `div` interno e o `add('sticky-spinner')` vai no rótulo
//!   externo, então a condição nunca é verdadeira e o ícone é recriado a cada posicionamento. O porte mantém o original.
//! - `querySelector('.codicon') !== null` é a busca de um descendente com a classe `codicon`.

use std::rc::{Rc, Weak};

use je_base::dynamic::number_to_js_string;
use je_base::async_::disposable_timeout;
use je_base::lifecycle::DisposableStore;
use je_base::numbers::clamp;
use je_base::event::Event;
use je_render::kurbo::Rect;
use je_ui::{NodeId, Role, Tree, Ui};
use je_widgets::dom::{clear_node, create_el, hide, show};
use je_widgets::highlighted_label::HighlightedLabelStyles;
use je_widgets::simple_icon_label::SimpleIconLabel;

use crate::contrib::notebook::browser::notebook_view_events::CellViewModelStateChangeEvent;
use crate::contrib::notebook::browser::view::cell_part::{CellPart, CellPartState, common_layout_info};
use crate::contrib::notebook::browser::view::cell_parts::code_cell_execution_icon::spinning_executing_state_icon;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_common::NotebookCellInternalMetadata;
use crate::contrib::notebook::common::notebook_execution_state_service::{ExecutionStateChange, INotebookExecutionStateService};
use crate::contrib::notebook::common::notebook_kernel_service::INotebookKernel;

const UPDATE_EXECUTION_ORDER_GRACE_PERIOD: u64 = 200;

/// O que `CellExecutionPart` chama do `INotebookEditorDelegate`.
pub trait ICellExecutionEditor {
    /// `onDidChangeActiveKernel`.
    fn on_did_change_active_kernel(&self) -> Event<()>;
    /// `activeKernel`.
    fn active_kernel(&self) -> Option<Rc<dyn INotebookKernel>>;
    /// `onDidScroll`.
    fn on_did_scroll(&self) -> Event<()>;
    /// `getAbsoluteTopOfElement(cell)`.
    fn get_absolute_top_of_element(&self, cell: &CellViewModel) -> f64;
    /// `scrollBottom`.
    fn scroll_bottom(&self) -> f64;
}

/// O texto do rótulo: `[n]` com a ordem, ou `[ ]` sem ela.
fn execution_order_text(execution_order: Option<f64>) -> String {
    match execution_order {
        Some(order) => format!("[{}]", number_to_js_string(order)),
        None => "[ ]".to_owned(),
    }
}

/// `querySelector('.codicon') !== null`.
fn has_descendant_with_class(tree: &Tree, node: NodeId, class_name: &str) -> bool {
    tree.children(node).iter().any(|child| tree.has_class(*child, class_name) || has_descendant_with_class(tree, *child, class_name))
}

/// `CellExecutionPart`.
pub struct CellExecutionPart {
    state: CellPartState,
    me: Weak<CellExecutionPart>,
    ui: Ui,
    notebook_editor: Rc<dyn ICellExecutionEditor>,
    execution_order_label: NodeId,
    execution_order_content: NodeId,
    icon_label: SimpleIconLabel,
    notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    kernel_disposables: Rc<DisposableStore>,
}

impl CellExecutionPart {
    pub fn new(
        ui: &Ui,
        notebook_editor: Rc<dyn ICellExecutionEditor>,
        execution_order_label: NodeId,
        label_styles: HighlightedLabelStyles,
        notebook_execution_state_service: Rc<dyn INotebookExecutionStateService>,
    ) -> Rc<Self> {
        let state = CellPartState::new();
        let kernel_disposables = DisposableStore::new();
        state.register(kernel_disposables.clone());

        // Add class to the outer container for styling
        // Create nested div for content
        let execution_order_content = {
            let mut tree = ui.tree_mut();
            tree.add_class(execution_order_label, "cell-execution-order");
            create_el(&mut tree, execution_order_label, Role::Generic, "")
        };

        Rc::new_cyclic(|me: &Weak<Self>| {
            // Add a method to watch for cell execution state changes
            {
                let me = me.clone();
                state.register(notebook_execution_state_service.on_did_change_execution().listen(move |e| {
                    let Some(part) = me.upgrade() else { return };
                    let ExecutionStateChange::Cell(e) = e else { return };
                    let affects_current_cell = part.state.current_cell().is_some_and(|cell| e.affects_cell(&cell.uri()));
                    if affects_current_cell {
                        part.update_position();
                    }
                }));
            }

            {
                let me = me.clone();
                state.register(notebook_editor.on_did_change_active_kernel().listen(move |_| {
                    let Some(part) = me.upgrade() else { return };
                    let Some(cell) = part.state.current_cell() else { return };
                    part.kernel_disposables.clear();

                    if let Some(kernel) = part.notebook_editor.active_kernel() {
                        let me = part.me.clone();
                        part.kernel_disposables.add(kernel.on_did_change().listen(move |_| {
                            let Some(part) = me.upgrade() else { return };
                            if let Some(cell) = part.state.current_cell() {
                                part.update_execution_order(&cell.base().internal_metadata(), false);
                            }
                        }));
                    }

                    part.update_execution_order(&cell.base().internal_metadata(), false);
                }));
            }

            {
                let me = me.clone();
                state.register(notebook_editor.on_did_scroll().listen(move |_| {
                    if let Some(part) = me.upgrade() {
                        part.update_position();
                    }
                }));
            }

            Self {
                state,
                me: me.clone(),
                ui: ui.clone(),
                notebook_editor,
                execution_order_label,
                execution_order_content,
                icon_label: SimpleIconLabel::new(ui, execution_order_content, label_styles),
                notebook_execution_state_service,
                kernel_disposables,
            }
        })
    }

    /// `this._executionOrderContent.innerText = text`.
    fn set_inner_text(&self, text: &str) {
        let mut tree = self.ui.tree_mut();
        clear_node(&mut tree, self.execution_order_content);
        if text.is_empty() {
            tree.clear_label(self.execution_order_content);
        } else {
            tree.set_label(self.execution_order_content, text);
        }
    }

    fn update_execution_order(&self, internal_metadata: &NotebookCellInternalMetadata, force_clear: bool) {
        let active_kernel = self.notebook_editor.active_kernel();
        let kernel_implements_execution_order = active_kernel.as_ref().is_some_and(|kernel| kernel.implements_execution_order() == Some(true));
        if kernel_implements_execution_order || (active_kernel.is_none() && internal_metadata.execution_order.is_some()) {
            // If the executionOrder was just cleared, and the cell is executing, wait just a bit before clearing the view to avoid flashing
            if internal_metadata.execution_order.is_none() && !force_clear {
                let Some(rendering_cell) = self.state.current_cell() else {
                    return;
                };
                if self.notebook_execution_state_service.get_cell_execution(&rendering_cell.uri()).is_some() {
                    let me = self.me.clone();
                    disposable_timeout(
                        move || {
                            let Some(part) = me.upgrade() else { return };
                            let Some(current_cell) = part.state.current_cell() else { return };
                            if current_cell.ptr_eq(&rendering_cell) {
                                part.update_execution_order(&current_cell.base().internal_metadata(), true);
                                part.update_position();
                            }
                        },
                        UPDATE_EXECUTION_ORDER_GRACE_PERIOD,
                        Some(self.state.cell_disposables().clone()),
                    );
                    return;
                }
            }

            self.set_inner_text(&execution_order_text(internal_metadata.execution_order));

            // Call _updatePosition to refresh sticky status
            self.update_position();
        } else {
            self.set_inner_text("");
        }
    }

    /// Repõe o texto `[n]` da ordem de execução da célula atual (as três voltas de `sticky` para o texto normal do original).
    fn restore_execution_order_text(&self, cell: &CellViewModel) {
        self.set_inner_text(&execution_order_text(cell.base().internal_metadata().execution_order));
    }

    fn update_position(&self) {
        let Some(cell) = self.state.current_cell() else {
            return;
        };

        if cell.base().is_input_collapsed() {
            hide(&mut self.ui.tree_mut(), self.execution_order_label);
            return;
        }

        // Only show the execution order label when the cell is running
        let cell_is_running = self.notebook_execution_state_service.get_cell_execution(&cell.uri()).is_some();

        // Store sticky state before potentially removing the class
        let was_sticky = self.ui.tree().has_class(self.execution_order_label, "sticky");

        if !cell_is_running {
            // Keep showing the execution order label but remove sticky class
            self.ui.tree_mut().remove_class(self.execution_order_label, "sticky");

            // If we were sticky and cell stopped running, restore the proper content
            if was_sticky {
                self.restore_execution_order_text(&cell);
            }
        }

        show(&mut self.ui.tree_mut(), self.execution_order_label);
        let layout_info = common_layout_info(&cell);
        let mut top = layout_info.editor_height - 22.0 + layout_info.status_bar_height;

        if let Some(code_cell) = cell.as_code() {
            let code_layout_info = code_cell.layout_info();
            let element_top = self.notebook_editor.get_absolute_top_of_element(&cell);
            let editor_bottom = element_top + code_layout_info.output_container_offset;
            let scroll_bottom = self.notebook_editor.scroll_bottom();
            let line_height = 22.0;

            let status_bar_visible = code_layout_info.status_bar_height > 0.0;

            // Sticky mode: cell is running and editor is not fully visible
            let offset = editor_bottom - scroll_bottom;
            top -= offset;
            top = clamp(
                top,
                line_height + 12.0, // line height + padding for single line
                code_layout_info.editor_height - line_height + code_layout_info.status_bar_height,
            );

            if scroll_bottom <= editor_bottom && cell_is_running {
                let is_already_icon = self.ui.tree().has_class(self.execution_order_content, "sticky-spinner");
                // Add a class when it's in sticky mode for special styling
                if !is_already_icon {
                    self.ui.tree_mut().add_class(self.execution_order_label, "sticky-spinner");
                    // Only recreate the content if we're newly becoming sticky
                    clear_node(&mut self.ui.tree_mut(), self.execution_order_content);
                    self.ui.tree_mut().clear_label(self.execution_order_content);
                    let icon = spinning_executing_state_icon();
                    self.icon_label.set_text(&format!("$({})", icon.id));
                }
                // When already sticky, we don't need to recreate the content
            } else if !status_bar_visible && cell_is_running {
                // Status bar is hidden but cell is running: show execution order label at the bottom of the editor area
                let was_sticky_here = self.ui.tree().has_class(self.execution_order_label, "sticky");
                self.ui.tree_mut().remove_class(self.execution_order_label, "sticky");
                top = layout_info.editor_height - line_height; // Place at the bottom of the editor
                // Only update content if we were previously sticky or content is not correct
                let icon_is_present = has_descendant_with_class(&self.ui.tree(), self.execution_order_content, "codicon");
                if was_sticky_here || icon_is_present {
                    self.restore_execution_order_text(&cell);
                }
            } else {
                // Only update if the current state is sticky
                let currently_sticky = self.ui.tree().has_class(self.execution_order_label, "sticky");
                self.ui.tree_mut().remove_class(self.execution_order_label, "sticky");

                // When transitioning from sticky to non-sticky, restore the proper content
                if currently_sticky {
                    self.restore_execution_order_text(&cell);
                }
            }
        }

        let mut tree = self.ui.tree_mut();
        let bounds = tree.bounds(self.execution_order_label);
        tree.set_bounds(self.execution_order_label, Rect::new(bounds.x0, top, bounds.x1, top + bounds.height()));
    }
}

impl CellPart for CellExecutionPart {
    fn state(&self) -> &CellPartState {
        &self.state
    }

    fn did_render_cell(&self, element: &CellViewModel) {
        self.update_execution_order(&element.base().internal_metadata(), true);
    }

    fn update_state(&self, element: &CellViewModel, e: &CellViewModelStateChangeEvent) {
        if e.internal_metadata_changed {
            self.update_execution_order(&element.base().internal_metadata(), false);
        }
    }

    fn update_internal_layout_now(&self, _element: &CellViewModel) {
        self.update_position();
    }
}

impl_cell_content_part!(CellExecutionPart, "CellExecutionPart");
