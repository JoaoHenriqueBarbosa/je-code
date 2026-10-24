//! Porte de `src/vs/workbench/contrib/notebook/test/browser/cellDnd.test.ts` (suíte `cellDND`), caso a caso e na mesma ordem.
//! Os 9 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso.
//! - `testCellDnd(beginning, dragAction, end)` é [`test_cell_dnd`]; as três interfaces (`IBeginningState`, `IDragAction`,
//!   `IEndState`) são as structs de mesmo nome. O callback `async` do `withTestNotebook` é síncrono.
//! - `performCellDropEdits(editor, ...)` é `perform_cell_drop_edits` (`CellDropDirection::{Above, Below}` no lugar de
//!   `'above' | 'below'`); o `for (const i in end.endOrder)` confere o texto de cada célula na ordem final.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::notebook::browser::controller::IActiveNotebookEditor;
use je_workbench::contrib::notebook::browser::view::cell_parts::cell_dnd::{CellDropDirection, perform_cell_drop_edits};
use je_workbench::contrib::notebook::common::notebook_common::CellKind;
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use support::test_notebook_editor::{MockNotebookCell, with_test_notebook};

struct IBeginningState {
    start_order: Vec<&'static str>,
    selections: Vec<ICellRange>,
    focus: i64,
}

struct IDragAction {
    drag_idx: i64,
    drag_over_idx: i64,
    direction: CellDropDirection,
}

struct IEndState {
    end_order: Vec<&'static str>,
    selection: ICellRange,
    focus: i64,
}

fn range(start: i64, end: i64) -> ICellRange {
    ICellRange { start, end }
}

fn test_cell_dnd(beginning: IBeginningState, drag_action: IDragAction, end: IEndState) {
    let cells: Vec<MockNotebookCell> = beginning.start_order.iter().map(|text| (*text, "plaintext", CellKind::Code, Vec::new(), None)).collect();
    with_test_notebook(cells, |editor, view_model, _ds, _services| {
        editor.set_selections(beginning.selections.clone());
        editor.set_focus(range(beginning.focus, beginning.focus + 1));
        let dragged_cell = view_model.cell_at(drag_action.drag_idx).expect("célula arrastada");
        let dragged_over_cell = view_model.cell_at(drag_action.drag_over_idx).expect("célula alvo");
        perform_cell_drop_edits(&**editor, &dragged_cell, drag_action.direction, &dragged_over_cell);

        let view_cells = view_model.view_cells();
        for (i, expected) in end.end_order.iter().enumerate() {
            assert_eq!(view_cells[i].get_text(), *expected);
        }

        let selections = IActiveNotebookEditor::get_selections(&**editor);
        assert_eq!(selections.len(), 1);
        assert_eq!(selections[0], end.selection);
        assert_eq!(IActiveNotebookEditor::get_focus(&**editor), range(end.focus, end.focus + 1));
    });
}

#[test]
fn drag_1_cell() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    test_cell_dnd(
        IBeginningState { start_order: vec!["0", "1", "2", "3"], selections: vec![range(0, 1)], focus: 0 },
        IDragAction { drag_idx: 0, drag_over_idx: 1, direction: CellDropDirection::Below },
        IEndState { end_order: vec!["1", "0", "2", "3"], selection: range(1, 2), focus: 1 },
    );
}

#[test]
fn drag_multiple_contiguous_cells_down() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    test_cell_dnd(
        IBeginningState { start_order: vec!["0", "1", "2", "3"], selections: vec![range(1, 3)], focus: 1 },
        IDragAction { drag_idx: 1, drag_over_idx: 3, direction: CellDropDirection::Below },
        IEndState { end_order: vec!["0", "3", "1", "2"], selection: range(2, 4), focus: 2 },
    );
}

#[test]
fn drag_multiple_contiguous_cells_up() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    test_cell_dnd(
        IBeginningState { start_order: vec!["0", "1", "2", "3"], selections: vec![range(2, 4)], focus: 2 },
        IDragAction { drag_idx: 3, drag_over_idx: 0, direction: CellDropDirection::Above },
        IEndState { end_order: vec!["2", "3", "0", "1"], selection: range(0, 2), focus: 0 },
    );
}

#[test]
fn drag_ranges_down() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    test_cell_dnd(
        IBeginningState { start_order: vec!["0", "1", "2", "3"], selections: vec![range(0, 1), range(2, 3)], focus: 0 },
        IDragAction { drag_idx: 0, drag_over_idx: 3, direction: CellDropDirection::Below },
        IEndState { end_order: vec!["1", "3", "0", "2"], selection: range(2, 4), focus: 2 },
    );
}

#[test]
fn drag_ranges_up() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    test_cell_dnd(
        IBeginningState { start_order: vec!["0", "1", "2", "3"], selections: vec![range(1, 2), range(3, 4)], focus: 1 },
        IDragAction { drag_idx: 1, drag_over_idx: 0, direction: CellDropDirection::Above },
        IEndState { end_order: vec!["1", "3", "0", "2"], selection: range(0, 2), focus: 0 },
    );
}

#[test]
fn drag_ranges_between_ranges() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    test_cell_dnd(
        IBeginningState { start_order: vec!["0", "1", "2", "3"], selections: vec![range(0, 1), range(3, 4)], focus: 0 },
        IDragAction { drag_idx: 0, drag_over_idx: 1, direction: CellDropDirection::Below },
        IEndState { end_order: vec!["1", "0", "3", "2"], selection: range(1, 3), focus: 1 },
    );
}

#[test]
fn drag_ranges_just_above_a_range() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    test_cell_dnd(
        IBeginningState { start_order: vec!["0", "1", "2", "3"], selections: vec![range(1, 2), range(3, 4)], focus: 1 },
        IDragAction { drag_idx: 1, drag_over_idx: 1, direction: CellDropDirection::Above },
        IEndState { end_order: vec!["0", "1", "3", "2"], selection: range(1, 3), focus: 1 },
    );
}

#[test]
fn drag_ranges_inside_a_range() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    test_cell_dnd(
        IBeginningState { start_order: vec!["0", "1", "2", "3"], selections: vec![range(0, 2), range(3, 4)], focus: 0 },
        IDragAction { drag_idx: 0, drag_over_idx: 0, direction: CellDropDirection::Below },
        IEndState { end_order: vec!["0", "1", "3", "2"], selection: range(0, 3), focus: 0 },
    );
}

#[test]
fn dragged_cell_is_not_focused_or_selected() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    test_cell_dnd(
        IBeginningState { start_order: vec!["0", "1", "2", "3"], selections: vec![range(1, 2)], focus: 1 },
        IDragAction { drag_idx: 2, drag_over_idx: 3, direction: CellDropDirection::Below },
        IEndState { end_order: vec!["0", "1", "3", "2"], selection: range(3, 4), focus: 3 },
    );
}
