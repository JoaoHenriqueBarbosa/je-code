//! Porte de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellToolbarStickyScroll.ts`: `registerCellToolbarStickyScroll`,
//! que mantém a barra de título da célula (e o rótulo de ordem de execução) visível enquanto o editor da célula rola.
//!
//! O arquivo não está na lista de fatias do `mod.rs`, mas `CellTitleToolbarPart` (`cellToolbars.ts`) o importa e o porte não o tinha.
//!
//! Divergências:
//! - `INotebookEditor` é o contrato estreito [`ICellToolbarStickyScrollEditor`], só com `getCellByHandle`, `scrollTop`,
//!   `getAbsoluteTopOfElement`, `onDidScroll` e `onDidChangeLayout`. Os dois eventos carregam `()` porque a conta só reage ao disparo.
//! - `element.style.top = ''` e `` `${top}px` `` são o [`StickyTopNode`]: a árvore do `je-ui` não tem folha de estilo, então o `top`
//!   que o CSS dá ao elemento é o `y0` que o nó tinha quando o [`StickyTopNode`] foi criado, e `''` volta a ele.
//! - `cell.handle` comparado por identidade com o resultado de `getCellByHandle` é [`CellViewModel::ptr_eq`].
//! - `IDisposable[]` mais `combinedDisposable` é [`combined_disposable`] sobre os dois ouvintes.

use std::rc::Rc;

use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, combined_disposable};
use je_base::numbers::clamp;
use je_render::kurbo::Rect;
use je_ui::{NodeId, Ui};

use crate::contrib::notebook::browser::view::cell_part::common_layout_info;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;

/// O que `registerCellToolbarStickyScroll` chama do `INotebookEditor`.
pub trait ICellToolbarStickyScrollEditor {
    /// `getCellByHandle(handle)`.
    fn get_cell_by_handle(&self, handle: i64) -> Option<CellViewModel>;
    /// `scrollTop`.
    fn scroll_top(&self) -> f64;
    /// `getAbsoluteTopOfElement(cell)`.
    fn get_absolute_top_of_element(&self, cell: &CellViewModel) -> f64;
    /// `onDidScroll`.
    fn on_did_scroll(&self) -> Event<()>;
    /// `onDidChangeLayout`.
    fn on_did_change_layout(&self) -> Event<()>;
}

/// O `HTMLElement` de `registerCellToolbarStickyScroll`, só com o `style.top` (ver as divergências do módulo).
pub struct StickyTopNode {
    ui: Ui,
    node: NodeId,
    /// O `top` que a folha de estilo dá ao elemento: o `y0` do nó quando o [`StickyTopNode`] foi criado.
    base_top: f64,
}

impl StickyTopNode {
    pub fn new(ui: &Ui, node: NodeId) -> Rc<Self> {
        let base_top = ui.tree().bounds(node).y0;
        Rc::new(Self { ui: ui.clone(), node, base_top })
    }

    /// O nó do elemento.
    pub fn node(&self) -> NodeId {
        self.node
    }

    /// `element.style.top = ...`: `None` é o `''` (volta ao `top` da folha de estilo) e `Some(n)` é `` `${n}px` ``.
    pub fn set_top(&self, top: Option<f64>) {
        let top = top.unwrap_or(self.base_top);
        let mut tree = self.ui.tree_mut();
        let bounds = tree.bounds(self.node);
        tree.set_bounds(self.node, Rect::new(bounds.x0, top, bounds.x1, top + bounds.height()));
    }
}

/// `opts?: { extraOffset?: number; min?: number }`.
#[derive(Clone, Copy, Debug, Default)]
pub struct StickyScrollOptions {
    pub extra_offset: Option<f64>,
    pub min: Option<f64>,
}

/// `registerCellToolbarStickyScroll(notebookEditor, cell, element, opts)`.
pub fn register_cell_toolbar_sticky_scroll(
    notebook_editor: Rc<dyn ICellToolbarStickyScrollEditor>,
    cell: CellViewModel,
    element: Rc<StickyTopNode>,
    opts: Option<StickyScrollOptions>,
) -> DisposableRef {
    let extra_offset = opts.and_then(|opts| opts.extra_offset).unwrap_or(0.0);
    let min = opts.and_then(|opts| opts.min).unwrap_or(0.0);

    let update_for_scroll: Rc<dyn Fn()> = {
        let notebook_editor = notebook_editor.clone();
        Rc::new(move || {
            // Re-resolve the captured cell against the editor's current view model. The scroll listener can outlive the cell's
            // membership (e.g. the cell was removed or the pooled editor widget was reattached to a different notebook), in which
            // case `getAbsoluteTopOfElement` below would throw an "Invalid index -1" ListError.
            if !notebook_editor.get_cell_by_handle(cell.handle()).is_some_and(|current| current.ptr_eq(&cell)) {
                return;
            }
            if cell.base().is_input_collapsed() {
                element.set_top(None);
            } else {
                let scroll_top = notebook_editor.scroll_top();
                let element_top = notebook_editor.get_absolute_top_of_element(&cell);
                let diff = scroll_top - element_top + extra_offset;
                let layout_info = common_layout_info(&cell);
                // subtract roughly the height of the execution order label plus padding
                let max_top = layout_info.editor_height + layout_info.status_bar_height - 45.0;
                // Don't move the run button if it can only move a very short distance
                let top = if max_top > 20.0 { clamp(min, diff, max_top) } else { min };
                element.set_top(Some(top));
            }
        })
    };

    update_for_scroll();
    let on_scroll = update_for_scroll.clone();
    let on_layout = update_for_scroll;
    combined_disposable(vec![notebook_editor.on_did_scroll().listen(move |_| on_scroll()), notebook_editor.on_did_change_layout().listen(move |_| on_layout())])
}
