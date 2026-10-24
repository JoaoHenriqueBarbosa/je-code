//! Porte de `src/vs/workbench/contrib/notebook/browser/contrib/cellStatusBar/notebookVisibleCellObserver.ts`: o observador das células
//! visíveis do editor (`NotebookVisibleCellObserver`), que avisa quais células entraram e saíram da área visível.
//!
//! Divergências:
//! - `INotebookEditor` é o trait [`IVisibleCellsEditor`], só com o que o observador lê; `onDidChangeViewCells` é um `Event<()>` (o
//!   observador só reage ao disparo, sem ler as emendas). As células são o `CellViewModel`.
//! - `diffSets` é o de `je_base::collections` sobre as alças (`handle`).
//! - O construtor devolve `Rc<Self>` (os ouvintes guardam um `Weak`).

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::collections::diff_sets;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};

use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::{CellViewModel, NotebookViewModel};
use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_range::{ICellRange, cell_ranges_to_indexes};

/// O que o observador lê do `INotebookEditor`.
pub trait IVisibleCellsEditor {
    fn on_did_change_visible_ranges(&self) -> Event<()>;
    fn on_did_change_model(&self) -> Event<Option<Rc<NotebookTextModel>>>;
    fn on_did_change_view_cells(&self) -> Event<()>;
    fn has_model(&self) -> bool;
    fn visible_ranges(&self) -> Vec<ICellRange>;
    fn cell_at(&self, index: i64) -> Option<CellViewModel>;
    fn get_cell_by_handle(&self, handle: i64) -> Option<CellViewModel>;
    fn get_view_model(&self) -> Option<Rc<NotebookViewModel>>;
}

/// `ICellVisibilityChangeEvent`.
#[derive(Clone)]
pub struct ICellVisibilityChangeEvent {
    pub added: Vec<CellViewModel>,
    pub removed: Vec<CellViewModel>,
}

/// `NotebookVisibleCellObserver`.
pub struct NotebookVisibleCellObserver {
    this: Weak<NotebookVisibleCellObserver>,
    base: Rc<DisposableStore>,
    on_did_change_visible_cells: Rc<Emitter<ICellVisibilityChangeEvent>>,
    view_model_disposables: Rc<DisposableStore>,
    notebook_editor: Rc<dyn IVisibleCellsEditor>,
    visible_cells: RefCell<Vec<CellViewModel>>,
}

impl NotebookVisibleCellObserver {
    pub fn new(notebook_editor: Rc<dyn IVisibleCellsEditor>) -> Rc<Self> {
        let base = DisposableStore::new();
        let on_did_change_visible_cells = Emitter::<ICellVisibilityChangeEvent>::new();
        base.add_rc(on_did_change_visible_cells.clone());
        let view_model_disposables = DisposableStore::new();
        base.add_rc(view_model_disposables.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_change_visible_cells,
            view_model_disposables,
            notebook_editor: notebook_editor.clone(),
            visible_cells: RefCell::new(Vec::new()),
        });

        let weak = this.this.clone();
        this.base.add(notebook_editor.on_did_change_visible_ranges().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_visible_cells();
            }
        }));
        let weak = this.this.clone();
        this.base.add(notebook_editor.on_did_change_model().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.on_model_change();
            }
        }));
        this.update_visible_cells();
        this
    }

    pub fn on_did_change_visible_cells(&self) -> Event<ICellVisibilityChangeEvent> {
        self.on_did_change_visible_cells.event()
    }

    pub fn visible_cells(&self) -> Vec<CellViewModel> {
        self.visible_cells.borrow().clone()
    }

    fn on_model_change(&self) {
        self.view_model_disposables.clear();
        if self.notebook_editor.has_model() {
            let weak = self.this.clone();
            self.view_model_disposables.add(self.notebook_editor.on_did_change_view_cells().listen(move |_| {
                if let Some(this) = weak.upgrade() {
                    this.update_everything();
                }
            }));
        }

        self.update_everything();
    }

    fn update_everything(&self) {
        let removed = self.visible_cells.borrow().clone();
        self.on_did_change_visible_cells.fire(ICellVisibilityChangeEvent { added: Vec::new(), removed });
        self.visible_cells.borrow_mut().clear();
        self.update_visible_cells();
    }

    fn update_visible_cells(&self) {
        if !self.notebook_editor.has_model() {
            return;
        }

        let new_visible_cells: Vec<CellViewModel> =
            cell_ranges_to_indexes(&self.notebook_editor.visible_ranges()).into_iter().filter_map(|index| self.notebook_editor.cell_at(index)).collect();
        let new_visible_handles: Vec<i64> = new_visible_cells.iter().map(CellViewModel::handle).collect();
        let old_visible_handles: Vec<i64> = self.visible_cells.borrow().iter().map(CellViewModel::handle).collect();
        let diff = diff_sets(&old_visible_handles, &new_visible_handles);

        let added: Vec<CellViewModel> = diff.added.iter().filter_map(|handle| self.notebook_editor.get_cell_by_handle(*handle)).collect();
        let removed: Vec<CellViewModel> = diff.removed.iter().filter_map(|handle| self.notebook_editor.get_cell_by_handle(*handle)).collect();

        *self.visible_cells.borrow_mut() = new_visible_cells;
        self.on_did_change_visible_cells.fire(ICellVisibilityChangeEvent { added, removed });
    }
}

impl IDisposable for NotebookVisibleCellObserver {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "NotebookVisibleCellObserver"
    }
}
