//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookCellLayoutManager.test.ts` (suíte `NotebookCellLayoutManager`), caso a caso e na mesma ordem. Os 4 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada caso, e os casos `async` rodam com `run_with_real_timers`.
//! - `{ handle: 'cell1' } as ICellViewModel` é o [`MockCell`] (só o `handle`, numérico no porte); `MockList`, `MockLoggingService` e
//!   `MockNotebookWidget` implementam os traits que o gerenciador lê. As células são distintas pela identidade do `Rc`.
//! - `DOM.scheduleAtNextAnimationFrame` é o [`AnimationFrames`]: como o `AnimationFrameQueueItem` do DOM, todos os pedidos de um quadro
//!   rodam juntos num único `setTimeout`, e o `dispose` do pedido o tira da fila.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::runtime;
use je_workbench::contrib::notebook::browser::notebook_cell_layout_manager::{
    ILayoutCell, INotebookCellLayoutList, INotebookCellLayoutWidget, LayoutCell, NotebookCellLayoutManager, ScheduleAtNextAnimationFrame,
};
use je_workbench::contrib::notebook::common::notebook_logging_service::INotebookLoggingService;
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;

/// `{ handle: 'cell1' } as unknown as ICellViewModel`.
struct MockCell {
    handle: i64,
}

impl ILayoutCell for MockCell {
    fn handle(&self) -> i64 {
        self.handle
    }
}

fn mock_cell_view_model() -> LayoutCell {
    Rc::new(MockCell { handle: 1 })
}

fn key(cell: &LayoutCell) -> usize {
    Rc::as_ptr(cell) as *const () as usize
}

/// `class MockList`.
#[derive(Default)]
struct MockList {
    height: RefCell<HashMap<usize, f64>>,
    in_rendering_transaction: Cell<bool>,
    cells: RefCell<Vec<LayoutCell>>,
}

impl INotebookCellLayoutList for MockList {
    fn get_view_index(&self, cell: &LayoutCell) -> Option<usize> {
        self.cells.borrow().iter().position(|candidate| key(candidate) == key(cell))
    }

    fn element_height(&self, cell: &LayoutCell) -> f64 {
        self.height.borrow().get(&key(cell)).copied().unwrap_or(100.0)
    }

    fn in_rendering_transaction(&self) -> bool {
        self.in_rendering_transaction.get()
    }

    fn scroll_top(&self) -> f64 {
        0.0
    }

    fn update_element_height2(&self, cell: &LayoutCell, height: f64, _anchor_element_index: Option<usize>) {
        self.height.borrow_mut().insert(key(cell), height);
    }
}

/// `class MockLoggingService`.
struct MockLoggingService;

impl INotebookLoggingService for MockLoggingService {
    fn info(&self, _category: &str, _output: &str) {}
    fn warn(&self, _category: &str, _output: &str) {}
    fn error(&self, _category: &str, _output: &str) {}
    fn debug(&self, _category: &str, _output: &str) {}
    fn trace(&self, _category: &str, _output: &str) {}
}

/// `class MockNotebookWidget`.
struct MockNotebookWidget {
    visible_ranges: Vec<ICellRange>,
}

impl MockNotebookWidget {
    fn new() -> Self {
        Self { visible_ranges: vec![ICellRange { start: 0, end: 0 }] }
    }
}

impl INotebookCellLayoutWidget for MockNotebookWidget {
    fn view_model_has_cell(&self, _cell: &LayoutCell) -> Option<bool> {
        Some(true)
    }

    fn view_model_get_cell_index(&self, _cell: &LayoutCell) -> Option<i64> {
        Some(0)
    }

    fn has_editor_focus(&self) -> bool {
        true
    }

    fn get_absolute_top_of_element(&self, _cell: &LayoutCell) -> f64 {
        0.0
    }

    fn get_length(&self) -> usize {
        1
    }

    fn visible_ranges(&self) -> Vec<ICellRange> {
        self.visible_ranges.clone()
    }
}

/// `DOM.scheduleAtNextAnimationFrame`: a fila de um quadro, esvaziada por um único `setTimeout`.
#[derive(Default)]
struct AnimationFrames {
    queue: RefCell<Vec<(u64, Box<dyn FnOnce()>)>>,
    timer_pending: Cell<bool>,
    next_id: Cell<u64>,
}

impl AnimationFrames {
    fn scheduler(self: &Rc<Self>) -> ScheduleAtNextAnimationFrame {
        let frames = self.clone();
        Rc::new(move |callback: Box<dyn FnOnce()>| -> DisposableRef {
            let id = frames.next_id.get();
            frames.next_id.set(id + 1);
            frames.queue.borrow_mut().push((id, callback));
            if !frames.timer_pending.get() {
                frames.timer_pending.set(true);
                let runner = frames.clone();
                runtime::set_timeout(
                    move || {
                        runner.timer_pending.set(false);
                        let batch = std::mem::take(&mut *runner.queue.borrow_mut());
                        for (_, callback) in batch {
                            callback();
                        }
                    },
                    0,
                );
            }
            let frames = frames.clone();
            to_disposable(move || frames.queue.borrow_mut().retain(|(queued, _)| *queued != id))
        })
    }
}

/// O `store.add(new NotebookCellLayoutManager(widget, list, new MockLoggingService()))`.
fn create_manager(store: &LeakGuard, list: &Rc<MockList>) -> Rc<NotebookCellLayoutManager> {
    let frames = Rc::new(AnimationFrames::default());
    let list: Rc<dyn INotebookCellLayoutList> = list.clone();
    store.add_rc(NotebookCellLayoutManager::new(Rc::new(MockNotebookWidget::new()), list, Rc::new(MockLoggingService), frames.scheduler()))
}

#[test]
fn should_update_cell_height() {
    run_with_real_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let cell = mock_cell_view_model();
        let cell2 = mock_cell_view_model();
        let list = Rc::new(MockList::default());
        list.cells.borrow_mut().push(cell.clone());
        list.cells.borrow_mut().push(cell2.clone());
        let mgr = create_manager(&store, &list);
        let _ = mgr.layout_notebook_cell(&cell, 200.0);
        let _ = mgr.layout_notebook_cell(&cell2, 200.0);
        assert_eq!(list.element_height(&cell), 200.0);
        assert_eq!(list.element_height(&cell2), 200.0);
    });
}

#[test]
fn should_schedule_updates_if_already_in_a_rendering_transaction() {
    run_with_real_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let cell = mock_cell_view_model();
        let cell2 = mock_cell_view_model();
        let list = Rc::new(MockList::default());
        list.in_rendering_transaction.set(true);
        list.cells.borrow_mut().push(cell.clone());
        list.cells.borrow_mut().push(cell2.clone());
        let mgr = create_manager(&store, &list);

        let promise = mgr.layout_notebook_cell(&cell, 200.0);
        let _ = mgr.layout_notebook_cell(&cell2, 200.0);
        assert_eq!(list.element_height(&cell), 100.0);
        assert_eq!(list.element_height(&cell2), 100.0);
        list.in_rendering_transaction.set(false);

        promise.await.unwrap();

        assert_eq!(list.element_height(&cell), 200.0);
        assert_eq!(list.element_height(&cell2), 200.0);
    });
}

#[test]
fn should_not_update_if_cell_is_hidden() {
    run_with_real_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let cell = mock_cell_view_model();
        let list = Rc::new(MockList::default());
        let mgr = create_manager(&store, &list);
        mgr.layout_notebook_cell(&cell, 200.0).await.unwrap();
        assert_eq!(list.element_height(&cell), 100.0);
    });
}

#[test]
fn should_not_update_if_height_is_unchanged() {
    run_with_real_timers(async move {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let cell = mock_cell_view_model();
        let list = Rc::new(MockList::default());
        list.cells.borrow_mut().push(cell.clone());
        let mgr = create_manager(&store, &list);
        mgr.layout_notebook_cell(&cell, 100.0).await.unwrap();
        assert_eq!(list.element_height(&cell), 100.0);
    });
}
