//! Porte de `src/vs/workbench/contrib/notebook/browser/notebookCellLayoutManager.ts`: o `NotebookCellLayoutManager`, que aplica a altura
//! pedida por uma célula na lista, adiando a atualização para o próximo quadro quando a lista está numa transação de renderização.
//!
//! Divergências:
//! - `ICellViewModel` é o trait [`ILayoutCell`] (só o `handle`, o que o gerenciador lê), com a identidade do objeto (o endereço do `Rc`)
//!   no lugar da chave do `WeakMap`. `INotebookCellList` (`getViewIndex`, `elementHeight`, `inRenderingTransaction`, `scrollTop` e
//!   `updateElementHeight2`) é o [`INotebookCellLayoutList`], e o `NotebookEditorWidget` (`viewModel?.hasCell`, `viewModel?.getCellIndex`,
//!   `hasEditorFocus`, `visibleRanges`, `getAbsoluteTopOfElement` e `getLength`) é o [`INotebookCellLayoutWidget`].
//! - `DOM.scheduleAtNextAnimationFrame(DOM.getWindow(widget.getDomNode()), doLayout)` é o [`ScheduleAtNextAnimationFrame`] recebido
//!   pelo construtor (a janela é do host de UI).
//! - `WeakMap<ICellViewModel, IDisposable>` e `Set<IDisposable>` são mapas por id (o `Set` precisa da identidade do descartável).
//! - `try { ... } finally { ... }` é `errors::catch` seguido do bloco final e do relançamento do erro.
//! - `async layoutNotebookCell` devolve a promessa do `DeferredPromise`.
//! - O construtor devolve `Rc<Self>` (o fecho do quadro guarda um `Weak`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::promise::{DeferredPromise, Promise};

use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_logging_service::INotebookLoggingService;
use crate::contrib::notebook::common::notebook_range::ICellRange;

/// O que o gerenciador lê de `ICellViewModel`.
pub trait ILayoutCell {
    fn handle(&self) -> i64;
}

impl ILayoutCell for CellViewModel {
    fn handle(&self) -> i64 {
        CellViewModel::handle(self)
    }
}

/// `ICellViewModel`, por identidade.
pub type LayoutCell = Rc<dyn ILayoutCell>;

/// `Pick<INotebookCellList, 'getViewIndex' | 'elementHeight' | 'inRenderingTransaction' | 'scrollTop' | 'updateElementHeight2'>`.
pub trait INotebookCellLayoutList {
    /// `getViewIndex(cell)`: `None` quando a célula está escondida.
    fn get_view_index(&self, cell: &LayoutCell) -> Option<usize>;
    fn element_height(&self, cell: &LayoutCell) -> f64;
    fn in_rendering_transaction(&self) -> bool;
    fn scroll_top(&self) -> f64;
    /// `updateElementHeight2(cell, size, anchorElementIndex?)`.
    fn update_element_height2(&self, cell: &LayoutCell, size: f64, anchor_element_index: Option<usize>);
}

/// O que o gerenciador lê do `NotebookEditorWidget`.
pub trait INotebookCellLayoutWidget {
    /// `viewModel?.hasCell(cell)`: `None` quando não há view model.
    fn view_model_has_cell(&self, cell: &LayoutCell) -> Option<bool>;
    /// `viewModel?.getCellIndex(cell)`: `None` quando não há view model.
    fn view_model_get_cell_index(&self, cell: &LayoutCell) -> Option<i64>;
    fn has_editor_focus(&self) -> bool;
    fn get_absolute_top_of_element(&self, cell: &LayoutCell) -> f64;
    fn get_length(&self) -> usize;
    fn visible_ranges(&self) -> Vec<ICellRange>;
}

/// `DOM.scheduleAtNextAnimationFrame(window, callback)`.
pub type ScheduleAtNextAnimationFrame = Rc<dyn Fn(Box<dyn FnOnce()>) -> DisposableRef>;

fn cell_key(cell: &LayoutCell) -> usize {
    Rc::as_ptr(cell) as *const () as usize
}

/// `NotebookCellLayoutManager`.
pub struct NotebookCellLayoutManager {
    this: Weak<NotebookCellLayoutManager>,
    base: Rc<Disposable>,
    notebook_widget: Rc<dyn INotebookCellLayoutWidget>,
    list: Rc<dyn INotebookCellLayoutList>,
    logging_service: Rc<dyn INotebookLoggingService>,
    schedule_at_next_animation_frame: ScheduleAtNextAnimationFrame,
    /// `_pendingLayouts`: a chave da célula para o id do descartável.
    pending_layouts: RefCell<HashMap<usize, u64>>,
    /// `_layoutDisposables`.
    layout_disposables: RefCell<HashMap<u64, DisposableRef>>,
    next_id: Cell<u64>,
    layout_stack: RefCell<Vec<String>>,
    is_disposed: Cell<bool>,
}

impl NotebookCellLayoutManager {
    pub fn new(
        notebook_widget: Rc<dyn INotebookCellLayoutWidget>,
        list: Rc<dyn INotebookCellLayoutList>,
        logging_service: Rc<dyn INotebookLoggingService>,
        schedule_at_next_animation_frame: ScheduleAtNextAnimationFrame,
    ) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base: Disposable::new(),
            notebook_widget,
            list,
            logging_service,
            schedule_at_next_animation_frame,
            pending_layouts: RefCell::new(HashMap::new()),
            layout_disposables: RefCell::new(HashMap::new()),
            next_id: Cell::new(0),
            layout_stack: RefCell::new(Vec::new()),
            is_disposed: Cell::new(false),
        })
    }

    fn check_stack_depth(&self) {
        if self.layout_stack.borrow().len() > 30 {
            let layout_trace = self.layout_stack.borrow().join(" -> ");
            errors::throw(Error::new(format!("NotebookCellLayoutManager: layout stack is too deep: {layout_trace}")));
        }
    }

    /// `layoutNotebookCell(cell, height)`.
    pub fn layout_notebook_cell(&self, cell: &LayoutCell, height: f64) -> Promise<()> {
        let layout_tag = format!("cell:{}, height:{}", cell.handle(), height);
        self.logging_service.debug("cell layout", &layout_tag);
        if self.list.get_view_index(cell).is_none() {
            // the cell is hidden
            return Promise::resolved(());
        }

        let key = cell_key(cell);
        let old_pending_layout = self.pending_layouts.borrow().get(&key).copied();
        if let Some(old_pending_layout) = old_pending_layout {
            let disposable = self.layout_disposables.borrow_mut().remove(&old_pending_layout);
            if let Some(disposable) = disposable {
                disposable.dispose();
            }
        }

        let deferred = Rc::new(DeferredPromise::<()>::new());
        if self.list.in_rendering_transaction() {
            let (weak, cell_for_frame, deferred_for_frame, tag) = (self.this.clone(), cell.clone(), deferred.clone(), layout_tag);
            let layout_disposable = (self.schedule_at_next_animation_frame)(Box::new(move || {
                if let Some(this) = weak.upgrade() {
                    this.do_layout(&cell_for_frame, height, &tag, &deferred_for_frame);
                }
            }));

            let deferred_for_dispose = deferred.clone();
            let disposable = to_disposable(move || {
                layout_disposable.dispose();
                let _ = deferred_for_dispose.complete(());
            });
            let id = self.next_id.get();
            self.next_id.set(id + 1);
            self.pending_layouts.borrow_mut().insert(key, id);
            self.layout_disposables.borrow_mut().insert(id, disposable);
        } else {
            self.do_layout(cell, height, &layout_tag, &deferred);
        }

        deferred.promise.clone()
    }

    /// `doLayout`.
    fn do_layout(&self, cell: &LayoutCell, height: f64, layout_tag: &str, deferred: &Rc<DeferredPromise<()>>) {
        let key = cell_key(cell);
        let pending_layout = self.pending_layouts.borrow_mut().remove(&key);

        self.layout_stack.borrow_mut().push(layout_tag.to_string());
        let outcome = errors::catch(|| self.layout_cell_height(cell, height));
        self.layout_stack.borrow_mut().pop();
        let _ = deferred.complete(());
        if let Some(pending_layout) = pending_layout {
            let disposable = self.layout_disposables.borrow_mut().remove(&pending_layout);
            if let Some(disposable) = disposable {
                disposable.dispose();
            }
        }
        if let Err(error) = outcome {
            errors::throw(error);
        }
    }

    /// O corpo do `try` de `doLayout`.
    fn layout_cell_height(&self, cell: &LayoutCell, height: f64) {
        if self.is_disposed.get() {
            return;
        }

        if self.notebook_widget.view_model_has_cell(cell) != Some(true) {
            // Cell removed in the meantime?
            return;
        }

        if self.list.get_view_index(cell).is_none() {
            // Cell can be hidden
            return;
        }

        if self.list.element_height(cell) == height {
            return;
        }

        self.check_stack_depth();

        if !self.notebook_widget.has_editor_focus() {
            // Do not scroll inactive notebook
            // https://github.com/microsoft/vscode/issues/145340
            let cell_index = self.notebook_widget.view_model_get_cell_index(cell);
            let visible_ranges = self.notebook_widget.visible_ranges();
            if let Some(cell_index) = cell_index
                && visible_ranges.first().is_some_and(|range| range.start == cell_index)
                // cell is partially visible
                && self.list.scroll_top() > self.notebook_widget.get_absolute_top_of_element(cell)
            {
                let anchor = (cell_index + 1).min(self.notebook_widget.get_length() as i64 - 1);
                return self.list.update_element_height2(cell, height, Some(anchor.max(0) as usize));
            }
        }

        self.list.update_element_height2(cell, height, None);
    }
}

impl IDisposable for NotebookCellLayoutManager {
    fn dispose(&self) {
        self.base.dispose();
        self.is_disposed.set(true);
        let disposables: Vec<DisposableRef> = self.layout_disposables.borrow_mut().drain().map(|(_, disposable)| disposable).collect();
        for disposable in disposables {
            disposable.dispose();
        }
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookCellLayoutManager"
    }
}
