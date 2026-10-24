//! Porte de `src/vs/workbench/contrib/notebook/browser/contrib/cellStatusBar/contributedStatusBarItemController.ts`: o controlador que
//! pede aos provedores de itens da barra de status de célula os itens das células visíveis e os mantém atualizados
//! (`ContributedStatusBarItemController` e o `CellStatusBarHelper` de cada célula).
//!
//! Divergências:
//! - `INotebookEditor` é o [`IVisibleCellsEditor`] do observador de células visíveis; o `getViewModel()` vem dele.
//! - `registerNotebookContribution(id, ctor)` e `INotebookEditorContribution` (o registro de contribuições do widget) não têm porte: o
//!   controlador é criado por quem monta o editor, e o `id` é [`ContributedStatusBarItemController::ID`].
//! - `setTimeout(() => ..., 0)` do `_updateSoon` é o `runtime::set_timeout`; `async _update` é o `async_fn`.
//! - `this._activeToken?.dispose(true)` é `dispose_with(true)`; `itemList.dispose && itemList.dispose()` é o `dispose` opcional da lista.
//! - `this._cell.model.onDidChange*` são os eventos do modelo da célula.
//! - Os construtores devolvem `Rc<Self>` (os fechos guardam um `Weak`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::async_::{Throttler, async_fn};
use je_base::cancellation::CancellationTokenSource;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::runtime;

use super::notebook_visible_cell_observer::{ICellVisibilityChangeEvent, IVisibleCellsEditor, NotebookVisibleCellObserver};
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::{CellViewModel, INotebookDeltaCellStatusBarItems, NotebookViewModel};
use crate::contrib::notebook::common::notebook_cell_status_bar_service::INotebookCellStatusBarService;
use crate::contrib::notebook::common::notebook_common::{ICell, INotebookCellStatusBarItemList};

/// `ContributedStatusBarItemController`.
pub struct ContributedStatusBarItemController {
    this: Weak<ContributedStatusBarItemController>,
    base: Rc<DisposableStore>,
    notebook_editor: Rc<dyn IVisibleCellsEditor>,
    notebook_cell_status_bar_service: Rc<dyn INotebookCellStatusBarService>,
    visible_cells: RefCell<HashMap<i64, Rc<CellStatusBarHelper>>>,
    observer: Rc<NotebookVisibleCellObserver>,
}

impl ContributedStatusBarItemController {
    pub const ID: &'static str = "workbench.notebook.statusBar.contributed";

    pub fn new(notebook_editor: Rc<dyn IVisibleCellsEditor>, notebook_cell_status_bar_service: Rc<dyn INotebookCellStatusBarService>) -> Rc<Self> {
        let base = DisposableStore::new();
        let observer = base.add_rc(NotebookVisibleCellObserver::new(notebook_editor.clone()));
        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            notebook_editor,
            notebook_cell_status_bar_service: notebook_cell_status_bar_service.clone(),
            visible_cells: RefCell::new(HashMap::new()),
            observer: observer.clone(),
        });

        let weak = this.this.clone();
        this.base.add(observer.on_did_change_visible_cells().listen(move |e| {
            if let Some(this) = weak.upgrade() {
                this.update_visible_cells(e.added.clone(), e.removed.iter().map(CellViewModel::handle).collect());
            }
        }));

        this.update_everything();
        let weak = this.this.clone();
        this.base.add(notebook_cell_status_bar_service.on_did_change_providers().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_everything();
            }
        }));
        let weak = this.this.clone();
        this.base.add(notebook_cell_status_bar_service.on_did_change_items().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_everything();
            }
        }));
        this
    }

    fn update_everything(&self) {
        let visible = self.observer.visible_cells();
        let new_cells: Vec<CellViewModel> = visible.iter().filter(|cell| !self.visible_cells.borrow().contains_key(&cell.handle())).cloned().collect();
        let visible_cell_handles: Vec<i64> = visible.iter().map(CellViewModel::handle).collect();
        let current_cell_handles: Vec<i64> = self.visible_cells.borrow().keys().copied().collect();
        let removed_cells: Vec<i64> = current_cell_handles.iter().filter(|handle| !visible_cell_handles.contains(handle)).copied().collect();
        let items_to_update: Vec<i64> = current_cell_handles.iter().filter(|handle| visible_cell_handles.contains(handle)).copied().collect();

        self.update_visible_cells(new_cells, removed_cells);
        for handle in items_to_update {
            let helper = self.visible_cells.borrow().get(&handle).cloned();
            if let Some(helper) = helper {
                helper.update();
            }
        }
    }

    fn update_visible_cells(&self, added: Vec<CellViewModel>, removed: Vec<i64>) {
        let Some(vm) = self.notebook_editor.get_view_model() else {
            return;
        };

        for new_cell in added {
            let helper = CellStatusBarHelper::new(vm.clone(), new_cell.clone(), self.notebook_cell_status_bar_service.clone());
            self.visible_cells.borrow_mut().insert(new_cell.handle(), helper);
        }

        for old_cell in removed {
            let helper = self.visible_cells.borrow_mut().remove(&old_cell);
            if let Some(helper) = helper {
                helper.dispose();
            }
        }
    }
}

impl IDisposable for ContributedStatusBarItemController {
    fn dispose(&self) {
        self.base.dispose();

        let helpers: Vec<Rc<CellStatusBarHelper>> = self.visible_cells.borrow_mut().drain().map(|(_, helper)| helper).collect();
        for helper in helpers {
            helper.dispose();
        }
    }

    fn constructor_name(&self) -> &'static str {
        "ContributedStatusBarItemController"
    }
}

/// `CellStatusBarHelper`.
struct CellStatusBarHelper {
    this: Weak<CellStatusBarHelper>,
    base: Rc<DisposableStore>,
    notebook_view_model: Rc<NotebookViewModel>,
    cell: CellViewModel,
    notebook_cell_status_bar_service: Rc<dyn INotebookCellStatusBarService>,
    current_item_ids: RefCell<Vec<String>>,
    current_item_lists: RefCell<Vec<INotebookCellStatusBarItemList>>,
    active_token: RefCell<Option<Rc<CancellationTokenSource>>>,
    is_disposed: Cell<bool>,
    update_throttler: Rc<Throttler<()>>,
}

impl CellStatusBarHelper {
    fn new(notebook_view_model: Rc<NotebookViewModel>, cell: CellViewModel, notebook_cell_status_bar_service: Rc<dyn INotebookCellStatusBarService>) -> Rc<Self> {
        let base = DisposableStore::new();
        let update_throttler = Rc::new(Throttler::<()>::new());
        base.add_rc(update_throttler.clone());
        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            notebook_view_model,
            cell: cell.clone(),
            notebook_cell_status_bar_service,
            current_item_ids: RefCell::new(Vec::new()),
            current_item_lists: RefCell::new(Vec::new()),
            active_token: RefCell::new(None),
            is_disposed: Cell::new(false),
            update_throttler,
        });

        let weak = this.this.clone();
        this.base.add(to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                this.dispose_active_token();
            }
        }));
        this.update_soon();
        let model = cell.model();
        let soon = |weak: Weak<CellStatusBarHelper>| {
            move || {
                if let Some(this) = weak.upgrade() {
                    this.update_soon();
                }
            }
        };
        let (on_content, on_language, on_metadata, on_internal_metadata, on_outputs) = (soon(this.this.clone()), soon(this.this.clone()), soon(this.this.clone()), soon(this.this.clone()), soon(this.this.clone()));
        this.base.add(model.on_did_change_content().listen(move |_| on_content()));
        this.base.add(model.on_did_change_language().listen(move |_| on_language()));
        this.base.add(model.on_did_change_metadata().listen(move |_| on_metadata()));
        this.base.add(model.on_did_change_internal_metadata().listen(move |_| on_internal_metadata()));
        this.base.add(model.on_did_change_outputs().listen(move |_| on_outputs()));
        this
    }

    fn dispose_active_token(&self) {
        let token = self.active_token.borrow().clone();
        if let Some(token) = token {
            token.dispose_with(true);
        }
    }

    fn update(&self) {
        self.update_soon();
    }

    fn update_soon(&self) {
        // Wait a tick to make sure that the event is fired to the EH before triggering status bar providers
        let weak = self.this.clone();
        runtime::set_timeout(
            move || {
                let Some(this) = weak.upgrade() else { return };
                if !this.is_disposed.get() {
                    let weak = this.this.clone();
                    let _ = this.update_throttler.queue(move |_| match weak.upgrade() {
                        Some(this) => this.update_impl(),
                        None => Promise::resolved(()),
                    });
                }
            },
            0,
        );
    }

    fn update_impl(self: &Rc<Self>) -> Promise<()> {
        let this = self.clone();
        async_fn(async move {
            let cell_index = this.notebook_view_model.get_cell_index(&this.cell);
            let document = this.notebook_view_model.notebook_document();
            let doc_uri = document.uri();
            let view_type = document.notebook_type();

            this.dispose_active_token();
            let token_source = Rc::new(CancellationTokenSource::default());
            *this.active_token.borrow_mut() = Some(token_source.clone());
            let token = token_source.token();
            let item_lists = this.notebook_cell_status_bar_service.get_status_bar_items_for_cell(&doc_uri, cell_index.max(0) as usize, &view_type, &token).await?;
            if token.is_cancellation_requested() {
                for item_list in &item_lists {
                    if let Some(dispose) = &item_list.dispose {
                        dispose();
                    }
                }
                return Ok(());
            }

            let items = item_lists.iter().flat_map(|item_list| item_list.items.clone()).collect();
            let old_ids = this.current_item_ids.borrow().clone();
            let new_ids = this.notebook_view_model.delta_cell_status_bar_items(&old_ids, vec![INotebookDeltaCellStatusBarItems { handle: this.cell.handle(), items }]);

            let previous = std::mem::replace(&mut *this.current_item_lists.borrow_mut(), item_lists);
            for item_list in &previous {
                if let Some(dispose) = &item_list.dispose {
                    dispose();
                }
            }
            *this.current_item_ids.borrow_mut() = new_ids;
            Ok(())
        })
    }
}

impl IDisposable for CellStatusBarHelper {
    fn dispose(&self) {
        self.base.dispose();
        self.is_disposed.set(true);
        self.dispose_active_token();

        let old_ids = self.current_item_ids.borrow().clone();
        self.notebook_view_model.delta_cell_status_bar_items(&old_ids, vec![INotebookDeltaCellStatusBarItems { handle: self.cell.handle(), items: Vec::new() }]);
        for item_list in self.current_item_lists.borrow().iter() {
            if let Some(dispose) = &item_list.dispose {
                dispose();
            }
        }
    }

    fn constructor_name(&self) -> &'static str {
        "CellStatusBarHelper"
    }
}

/// O tipo do evento do observador, reexportado para quem monta o controlador.
pub type VisibleCellsChange = ICellVisibilityChangeEvent;
