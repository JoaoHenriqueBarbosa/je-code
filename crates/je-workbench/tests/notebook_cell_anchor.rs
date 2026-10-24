//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookCellAnchor.test.ts` (suíte `NotebookCellAnchor`), caso a caso e na mesma ordem. Os 3 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada caso, e o `setup` (o `TestConfigurationService`, o emissor de
//!   rolagem, o `executionService` mockado e o `NotebookCellAnchor`) é [`Suite::new`], que roda dentro do `with_test_notebook` porque
//!   o `resizingCell` é uma célula de código de verdade (o original usa um objeto com `cellKind` e `onDidStopExecution`).
//! - `onDidStopExecution.fire()` é o `update_execution_state` da célula com um evento de execução terminada (`changed` indefinido),
//!   que dispara o `onDidStopExecution` da célula.
//! - `focusedCell.focusMode` e o `MockListView` são o [`MockListView`] (`ICellListView`), em que o `focusMode` é um `Cell` compartilhado
//!   com o caso, e `focusedCellTop` é o `Cell` do `top` da célula com foco.
//! - O `executionService` (`{ getCellExecution: () => ({ state: Executing }) }`) é o [`ExecutionServiceMock`], em que só
//!   `getCellExecution` responde e todo outro membro lança `TypeError`.
//! - `{ oldScrollTop, scrollTop } as ScrollEvent` é o [`scroll`], que preenche os outros campos com zero.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::IDisposable;
use je_base::scrollable::ScrollEvent;
use je_base::uri::URI;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::contrib::notebook::browser::view::notebook_cell_anchor::{ICellListView, NotebookCellAnchor};
use je_workbench::contrib::notebook::browser::view_model::cell_view_model_types::CellFocusMode;
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, NotebookCellExecutionState, NotebookSetting};
use je_workbench::contrib::notebook::common::notebook_execution_state_service::{
    ExecutionStateChange, ICellExecuteUpdate, ICellExecutionComplete, ICellExecutionStateChangedEvent, INotebookCellExecution, INotebookExecution,
    INotebookExecutionStateService, INotebookFailStateChangedEvent,
};
use support::test_notebook_editor::{MockNotebookCell, empty_metadata, with_test_notebook};

fn absent(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

/// `{ state: NotebookCellExecutionState.Executing }`.
struct ExecutingCellExecution;

impl INotebookCellExecution for ExecutingCellExecution {
    fn notebook(&self) -> URI {
        absent("notebook")
    }

    fn cell_handle(&self) -> i64 {
        absent("cellHandle")
    }

    fn state(&self) -> NotebookCellExecutionState {
        NotebookCellExecutionState::Executing
    }

    fn did_pause(&self) -> bool {
        absent("didPause")
    }

    fn is_paused(&self) -> bool {
        absent("isPaused")
    }

    fn confirm(&self) {
        absent("confirm")
    }

    fn update(&self, _updates: Vec<ICellExecuteUpdate>) {
        absent("update")
    }

    fn complete(&self, _complete: ICellExecutionComplete) {
        absent("complete")
    }
}

/// `{ getCellExecution: () => { return { state: NotebookCellExecutionState.Executing }; } } as unknown as INotebookExecutionStateService`.
struct ExecutionServiceMock;

impl INotebookExecutionStateService for ExecutionServiceMock {
    fn on_did_change_execution(&self) -> Event<ExecutionStateChange> {
        absent("onDidChangeExecution")
    }

    fn on_did_change_last_run_fail_state(&self) -> Event<INotebookFailStateChangedEvent> {
        absent("onDidChangeLastRunFailState")
    }

    fn force_cancel_notebook_executions(&self, _notebook_uri: &URI) {
        absent("forceCancelNotebookExecutions")
    }

    fn get_cell_executions_for_notebook(&self, _notebook: &URI) -> Vec<Rc<dyn INotebookCellExecution>> {
        absent("getCellExecutionsForNotebook")
    }

    fn get_cell_executions_by_handle_for_notebook(&self, _notebook: &URI) -> Option<HashMap<i64, Rc<dyn INotebookCellExecution>>> {
        absent("getCellExecutionsByHandleForNotebook")
    }

    fn get_cell_execution(&self, _cell_uri: &URI) -> Option<Rc<dyn INotebookCellExecution>> {
        Some(Rc::new(ExecutingCellExecution))
    }

    fn create_cell_execution(&self, _notebook: &URI, _cell_handle: i64) -> Rc<dyn INotebookCellExecution> {
        absent("createCellExecution")
    }

    fn get_execution(&self, _notebook: &URI) -> Option<Rc<dyn INotebookExecution>> {
        absent("getExecution")
    }

    fn create_execution(&self, _notebook: &URI) -> Rc<dyn INotebookExecution> {
        absent("createExecution")
    }

    fn get_last_failed_cell_for_notebook(&self, _notebook: &URI) -> Option<i64> {
        absent("getLastFailedCellForNotebook")
    }

    fn get_last_completed_cell_for_notebook(&self, _notebook: &URI) -> Option<i64> {
        absent("getLastCompletedCellForNotebook")
    }
}

/// O evento de uma execução terminada (`changed` indefinido): o que dispara o `onDidStopExecution` da célula.
struct StopExecutionEvent;

impl ICellExecutionStateChangedEvent for StopExecutionEvent {
    fn notebook(&self) -> URI {
        absent("notebook")
    }

    fn cell_handle(&self) -> i64 {
        absent("cellHandle")
    }

    fn changed(&self) -> Option<Rc<dyn INotebookCellExecution>> {
        None
    }

    fn affects_cell(&self, _cell: &URI) -> bool {
        absent("affectsCell")
    }

    fn affects_notebook(&self, _notebook: &URI) -> bool {
        absent("affectsNotebook")
    }
}

/// `class MockListView`.
struct MockListView {
    focused_cell_top: Cell<f64>,
    focused_cell_height: f64,
    render_top: f64,
    render_height: f64,
    /// `focusedCell.focusMode`.
    focus_mode: Rc<Cell<CellFocusMode>>,
}

impl MockListView {
    fn new(focus_mode: &Rc<Cell<CellFocusMode>>) -> Self {
        Self { focused_cell_top: Cell::new(100.0), focused_cell_height: 50.0, render_top: 0.0, render_height: 150.0, focus_mode: focus_mode.clone() }
    }
}

impl ICellListView for MockListView {
    fn element_focus_mode(&self, _index: usize) -> CellFocusMode {
        self.focus_mode.get()
    }

    fn element_top(&self, _index: usize) -> f64 {
        self.focused_cell_top.get()
    }

    fn element_height(&self, _index: usize) -> f64 {
        self.focused_cell_height
    }

    fn render_height(&self) -> f64 {
        self.render_height
    }

    fn get_scroll_top(&self) -> f64 {
        self.render_top
    }
}

/// `{ oldScrollTop, scrollTop } as ScrollEvent`.
fn scroll(old_scroll_top: f64, scroll_top: f64) -> ScrollEvent {
    ScrollEvent {
        in_smooth_scrolling: false,
        old_width: 0.0,
        old_scroll_width: 0.0,
        old_scroll_left: 0.0,
        width: 0.0,
        scroll_width: 0.0,
        scroll_left: 0.0,
        old_height: 0.0,
        old_scroll_height: 0.0,
        old_scroll_top,
        height: 0.0,
        scroll_height: 0.0,
        scroll_top,
        width_changed: false,
        scroll_width_changed: false,
        scroll_left_changed: false,
        height_changed: false,
        scroll_height_changed: false,
        scroll_top_changed: false,
    }
}

/// O `setup` da suíte, com o `focusedCell` (o `focus_mode`), o `config`, o `scrollEvent`, o `resizingCell` e o `cellAnchor`.
struct Suite {
    config: Rc<TestConfigurationService>,
    scroll_event: Rc<Emitter<ScrollEvent>>,
    focus_mode: Rc<Cell<CellFocusMode>>,
    resizing_cell: CellViewModel,
    cell_anchor: Rc<NotebookCellAnchor>,
    _store: LeakGuard,
}

impl Suite {
    fn new(resizing_cell: CellViewModel) -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let config = TestConfigurationService::new(None);
        store.add_rc(config.on_did_change_configuration_emitter.clone());
        let scroll_event = store.add_rc(Emitter::<ScrollEvent>::new());
        // `focusedCell = { focusMode: CellFocusMode.Container }`
        let focus_mode = Rc::new(Cell::new(CellFocusMode::Container));

        let configuration_service: Rc<dyn IConfigurationService> = config.clone();
        let cell_anchor = store.add_rc(NotebookCellAnchor::new(Rc::new(ExecutionServiceMock), configuration_service, scroll_event.event()));
        Self { config, scroll_event, focus_mode, resizing_cell, cell_anchor, _store: store }
    }

    /// `config.setUserConfiguration(NotebookSetting.scrollToRevealCell, value)`.
    fn set_scroll_to_reveal_cell(&self, value: &str) {
        let _ = self.config.set_user_configuration(NotebookSetting::SCROLL_TO_REVEAL_CELL, Dynamic::string(value), None);
    }

    /// `cellAnchor.shouldAnchor(listView, 1, delta, resizingCell)`.
    fn should_anchor(&self, list_view: &MockListView, height_delta: f64) -> bool {
        self.cell_anchor.should_anchor(list_view, 1, height_delta, &self.resizing_cell)
    }

    /// `onDidStopExecution.fire()`.
    fn fire_did_stop_execution(&self) {
        let event: Rc<dyn ICellExecutionStateChangedEvent> = Rc::new(StopExecutionEvent);
        self.resizing_cell.as_code().expect("o resizingCell é de código").update_execution_state(&event);
    }
}

/// `[source, lang, kind, output?, metadata?]` do `resizingCell`.
fn code_cell() -> MockNotebookCell {
    ("var a = 1;", "javascript", CellKind::Code, Vec::new(), Some(empty_metadata()))
}

#[test]
fn basic_anchoring() {
    with_test_notebook(vec![code_cell()], |_editor, view_model, _ds, _services| {
        let suite = Suite::new(view_model.cell_at(0).expect("cell 0"));

        suite.focus_mode.set(CellFocusMode::Editor);
        let list_view = MockListView::new(&suite.focus_mode);
        assert!(suite.should_anchor(&list_view, -10.0), "should anchor if cell editor is focused");
        assert!(suite.should_anchor(&list_view, 10.0), "should anchor if cell editor is focused");
        suite.set_scroll_to_reveal_cell("none");
        assert!(suite.should_anchor(&list_view, 10.0), "should anchor if cell editor is focused");

        suite.set_scroll_to_reveal_cell("fullCell");
        suite.focus_mode.set(CellFocusMode::Container);
        assert!(suite.should_anchor(&list_view, 10.0), "should anchor if cell is growing");
        suite.focus_mode.set(CellFocusMode::Output);
        assert!(suite.should_anchor(&list_view, 10.0), "should anchor if cell is growing");

        assert!(!suite.should_anchor(&list_view, -10.0), "should not anchor if not growing and editor not focused");

        suite.set_scroll_to_reveal_cell("none");
        assert!(!suite.should_anchor(&list_view, 10.0), "should not anchor if scroll on execute is disabled");

        suite.cell_anchor.dispose();
    });
}

#[test]
fn anchor_during_execution_until_user_scrolls_up() {
    with_test_notebook(vec![code_cell()], |_editor, view_model, _ds, _services| {
        let suite = Suite::new(view_model.cell_at(0).expect("cell 0"));
        let list_view = MockListView::new(&suite.focus_mode);
        let scroll_down = scroll(100.0, 150.0);
        let scroll_up = scroll(200.0, 150.0);

        assert!(suite.should_anchor(&list_view, 10.0));

        suite.scroll_event.fire(scroll_down);
        assert!(suite.should_anchor(&list_view, 10.0), "cell should still be anchored after scrolling down");

        suite.scroll_event.fire(scroll_up);
        assert!(!suite.should_anchor(&list_view, 10.0), "cell should not be anchored after scrolling up");
        suite.focus_mode.set(CellFocusMode::Editor);
        assert!(suite.should_anchor(&list_view, 10.0), "cell should anchor again if the editor is focused");
        suite.focus_mode.set(CellFocusMode::Container);

        suite.fire_did_stop_execution();
        assert!(suite.should_anchor(&list_view, 10.0), "cell should anchor for new execution");

        suite.cell_anchor.dispose();
    });
}

#[test]
fn only_anchor_during_when_the_focused_cell_will_be_pushed_out_of_view() {
    with_test_notebook(vec![code_cell()], |_editor, view_model, _ds, _services| {
        let suite = Suite::new(view_model.cell_at(0).expect("cell 0"));
        let mock_list_view = MockListView::new(&suite.focus_mode);
        mock_list_view.focused_cell_top.set(50.0);

        assert!(!suite.should_anchor(&mock_list_view, 10.0), "should not anchor if focused cell will still be fully visible after resize");
        suite.focus_mode.set(CellFocusMode::Editor);
        assert!(suite.should_anchor(&mock_list_view, 10.0), "cell should always anchor if the editor is focused");

        // fully visible focused cell would be pushed partially out of view
        assert!(suite.should_anchor(&mock_list_view, 150.0), "cell should be anchored if focused cell will be pushed out of view");
        mock_list_view.focused_cell_top.set(110.0);
        // partially visible focused cell would be pushed further out of view
        assert!(suite.should_anchor(&mock_list_view, 10.0), "cell should be anchored if focused cell will be pushed out of view");

        suite.cell_anchor.dispose();
    });
}
