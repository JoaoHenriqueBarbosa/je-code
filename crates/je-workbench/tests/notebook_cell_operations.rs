//! Porte de `src/vs/workbench/contrib/notebook/test/browser/cellOperations.test.ts` (suíte `CellOperations`), caso a caso e na mesma ordem.
//! Os 26 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso (`store`); os callbacks `async`
//!   do `withTestNotebook` e as operações `await`adas são síncronos (ver `cell_operations.rs`).
//! - `joinNotebookCells` devolve [`JoinEdit`] (o espelho local de `ResourceTextEdit | ResourceNotebookCellEdit`, ver
//!   `cell_operations.rs`); o `assert.deepStrictEqual(ret?.edits[n], new ResourceTextEdit(...))` compara cada campo do espelho
//!   (`resource`, `range`, `text`; e, no `ResourceNotebookCellEdit`, o `resource` do notebook, o `index`, o `count` e as `cells`
//!   vazias; o `editType: Replace` é a própria variante `ICellReplaceEdit`).
//! - `assert.strictEqual(cell, viewModel.cellAt(2))` (identidade) é `assert!(cell == other)` (o `PartialEq` do `CellViewModel` é
//!   por identidade).
//! - `computeCellLinesContents(cell, [{ lineNumber, column }])` recebe `Position`.
//! - O contexto `{ notebookEditor: editor, cell, ui }` é o `INotebookActionContext` montado com `new`, `with_cell` e `with_ui`.
//! - `insertCell(languageService, editor, ...)` recebe o `ILanguageService` de `services.language_service()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::model::{Eol, ValidAnnotatedEditOperation};
use je_editor::position::Position;
use je_editor::range::Range;
use je_workbench::contrib::notebook::browser::controller::cell_operations::{
    JoinEdit, change_cell_to_kind, compute_cell_lines_contents, copy_cell_range, insert_cell, join_notebook_cells, move_cell_range, run_delete_action,
};
use je_workbench::contrib::notebook::browser::controller::{IActiveNotebookEditor, INotebookActionContext};
use je_workbench::contrib::notebook::browser::view_model::folding_model::{FoldingModel, update_folding_state_at_index};
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, IOutputDto, ISelectionIndexState, ISelectionState, NotebookCellMetadata};
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use support::test_notebook_editor::{ActiveTestNotebookEditorDelegate, empty_metadata, with_test_notebook};

type Cell = (&'static str, &'static str, CellKind, Vec<IOutputDto>, Option<NotebookCellMetadata>);

fn md(source: &'static str) -> Cell {
    (source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))
}

fn js(source: &'static str) -> Cell {
    (source, "javascript", CellKind::Code, Vec::new(), Some(empty_metadata()))
}

fn range(start: i64, end: i64) -> ICellRange {
    ICellRange { start, end }
}

fn index_state(focus: ICellRange, selections: Vec<ICellRange>) -> ISelectionState {
    ISelectionState::Index(ISelectionIndexState { focus, selections })
}

fn five_cells() -> Vec<Cell> {
    vec![md("# header a"), js("var b = 1;"), md("# header b"), js("var b = 2;"), js("var c = 3;")]
}

fn context(editor: &Rc<ActiveTestNotebookEditorDelegate>) -> INotebookActionContext {
    let editor: Rc<dyn IActiveNotebookEditor> = editor.clone();
    INotebookActionContext::new(editor)
}

fn text_at(view_model: &je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::NotebookViewModel, index: i64) -> Option<String> {
    view_model.cell_at(index).map(|cell| cell.get_text())
}

fn eol_of(cell: &CellViewModel) -> &'static str {
    match cell.base().text_buffer().borrow().get_eol() {
        Eol::Lf => "\n",
        Eol::CrLf => "\r\n",
    }
}

/// `assert.deepStrictEqual(ret?.edits[0], new ResourceTextEdit(...))` e `edits[1]` (`ResourceNotebookCellEdit` de remoção).
fn assert_join_edits(
    edits: &[JoinEdit],
    expected_resource: &je_base::uri::URI,
    expected_text: &str,
    notebook_uri: &je_base::uri::URI,
    expected_index: usize,
    expected_count: usize,
) {
    assert_eq!(edits.len(), 2);
    match &edits[0] {
        JoinEdit::Text { resource, range, text } => {
            assert_eq!(resource, expected_resource);
            assert_eq!(*range, Range::new(1, 11, 1, 11));
            assert_eq!(text, expected_text);
        }
        other => panic!("esperava ResourceTextEdit, veio {other:?}"),
    }
    match &edits[1] {
        JoinEdit::NotebookCell { resource, cell_edit } => {
            assert_eq!(resource, notebook_uri);
            assert_eq!(cell_edit.index, expected_index);
            assert_eq!(cell_edit.count, expected_count);
            assert!(cell_edit.cells.is_empty());
        }
        other => panic!("esperava ResourceNotebookCellEdit, veio {other:?}"),
    }
}

mod cell_operations {
    use super::*;

    #[test]
    fn move_cells_single_cell() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(five_cells(), |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&index_state(range(1, 2), vec![range(1, 2)]), "model");
            let cell = view_model.cell_at(1).expect("cell");
            move_cell_range(&context(editor).with_cell(cell.clone()), "down");
            assert_eq!(text_at(view_model, 2), Some("var b = 1;".to_string()));
            assert!(cell == view_model.cell_at(2).expect("cell 2"));
        });
    }

    #[test]
    fn move_cells_multiple_cells_in_a_selection() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(five_cells(), |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&index_state(range(1, 2), vec![range(0, 2)]), "model");
            move_cell_range(&context(editor), "down");
            assert_eq!(text_at(view_model, 0), Some("# header b".to_string()));
            assert_eq!(text_at(view_model, 1), Some("# header a".to_string()));
            assert_eq!(text_at(view_model, 2), Some("var b = 1;".to_string()));
        });
    }

    #[test]
    fn move_cells_move_with_folding_ranges() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(five_cells(), |editor, view_model, ds, _services| {
            let folding_model = ds.add_rc(FoldingModel::new());
            folding_model.attach_view_model(view_model.clone());
            update_folding_state_at_index(&folding_model, 0, true);
            update_folding_state_at_index(&folding_model, 1, true);
            view_model.update_folding_ranges(folding_model.regions());
            editor.set_hidden_areas(&[range(1, 2)]);
            editor.set_hidden_areas(&view_model.get_hidden_ranges());

            view_model.update_selections_state(&index_state(range(0, 1), vec![range(0, 1)]), "model");
            move_cell_range(&context(editor), "down");
            assert_eq!(text_at(view_model, 0), Some("# header b".to_string()));
            assert_eq!(text_at(view_model, 1), Some("# header a".to_string()));
            assert_eq!(text_at(view_model, 2), Some("var b = 1;".to_string()));
        });
    }

    #[test]
    fn copy_duplicate_cells_single_cell() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(five_cells(), |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&index_state(range(1, 2), vec![range(1, 2)]), "model");
            copy_cell_range(&context(editor).with_cell(view_model.cell_at(1).expect("cell")), "down");
            assert_eq!(view_model.length(), 6);
            assert_eq!(text_at(view_model, 1), Some("var b = 1;".to_string()));
            assert_eq!(text_at(view_model, 2), Some("var b = 1;".to_string()));
        });
    }

    #[test]
    fn copy_duplicate_cells_target_and_selection_are_different_119769() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(five_cells(), |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&index_state(range(0, 1), vec![range(0, 1)]), "model");
            copy_cell_range(&context(editor).with_cell(view_model.cell_at(1).expect("cell")).with_ui(true), "down");
            assert_eq!(view_model.length(), 6);
            assert_eq!(text_at(view_model, 1), Some("var b = 1;".to_string()));
            assert_eq!(text_at(view_model, 2), Some("var b = 1;".to_string()));
        });
    }

    #[test]
    fn copy_duplicate_cells_multiple_cells_in_a_selection() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(five_cells(), |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&index_state(range(1, 2), vec![range(0, 2)]), "model");
            copy_cell_range(&context(editor).with_cell(view_model.cell_at(1).expect("cell")), "down");
            assert_eq!(view_model.length(), 7);
            assert_eq!(text_at(view_model, 0), Some("# header a".to_string()));
            assert_eq!(text_at(view_model, 1), Some("var b = 1;".to_string()));
            assert_eq!(text_at(view_model, 2), Some("# header a".to_string()));
            assert_eq!(text_at(view_model, 3), Some("var b = 1;".to_string()));
        });
    }

    #[test]
    fn copy_duplicate_cells_move_with_folding_ranges() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(five_cells(), |editor, view_model, ds, _services| {
            let folding_model = ds.add_rc(FoldingModel::new());
            folding_model.attach_view_model(view_model.clone());
            update_folding_state_at_index(&folding_model, 0, true);
            update_folding_state_at_index(&folding_model, 1, true);
            view_model.update_folding_ranges(folding_model.regions());
            editor.set_hidden_areas(&[range(1, 2)]);
            editor.set_hidden_areas(&view_model.get_hidden_ranges());

            view_model.update_selections_state(&index_state(range(0, 1), vec![range(0, 1)]), "model");
            copy_cell_range(&context(editor).with_cell(view_model.cell_at(1).expect("cell")), "down");
            assert_eq!(view_model.length(), 7);
            assert_eq!(text_at(view_model, 0), Some("# header a".to_string()));
            assert_eq!(text_at(view_model, 1), Some("var b = 1;".to_string()));
            assert_eq!(text_at(view_model, 2), Some("# header a".to_string()));
            assert_eq!(text_at(view_model, 3), Some("var b = 1;".to_string()));
        });
    }

    #[test]
    fn copy_duplicate_cells_should_not_share_the_same_text_buffer_102423() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![md("# header a"), js("var b = 1;")], |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&index_state(range(1, 2), vec![range(1, 2)]), "model");
            copy_cell_range(&context(editor).with_cell(view_model.cell_at(1).expect("cell")), "down");
            assert_eq!(view_model.length(), 3);
            let cell1 = view_model.cell_at(1).expect("cell 1");
            let cell2 = view_model.cell_at(2).expect("cell 2");
            assert_eq!(cell1.get_text(), "var b = 1;");
            assert_eq!(text_at(view_model, 2), Some("var b = 1;".to_string()));

            cell1.base().text_buffer().borrow_mut().apply_edits(
                &[ValidAnnotatedEditOperation {
                    identifier: None,
                    range: Range::new(1, 1, 1, 4),
                    text: Some("".into()),
                    force_move_markers: false,
                    is_auto_whitespace_edit: false,
                    is_tracked: false,
                }],
                false,
                true,
            );
            assert_ne!(cell1.get_text(), cell2.get_text());
        });
    }

    #[test]
    fn join_cell_with_below_single_cell() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(five_cells(), |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&index_state(range(3, 4), vec![range(3, 4)]), "model");
            let ret = join_notebook_cells(editor.as_ref(), range(3, 4), "below", None).expect("ret");
            let text = format!("{}var c = 3;", eol_of(&view_model.cell_at(4).expect("cell 4")));
            assert_join_edits(&ret.edits, &view_model.cell_at(3).expect("cell 3").uri(), &text, &editor.text_model.uri(), 4, 1);
        });
    }

    #[test]
    fn join_cell_with_above_single_cell() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(five_cells(), |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&index_state(range(3, 4), vec![range(3, 4)]), "model");
            let ret = join_notebook_cells(editor.as_ref(), range(4, 5), "above", None).expect("ret");
            let text = format!("{}var c = 3;", eol_of(&view_model.cell_at(4).expect("cell 4")));
            assert_join_edits(&ret.edits, &view_model.cell_at(3).expect("cell 3").uri(), &text, &editor.text_model.uri(), 4, 1);
        });
    }

    #[test]
    fn join_cell_with_below_multiple_cells() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;")], |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&index_state(range(1, 2), vec![range(0, 2)]), "model");
            let ret = join_notebook_cells(editor.as_ref(), range(0, 2), "below", None).expect("ret");
            let text = format!("{}var b = 2;{}var c = 3;", eol_of(&view_model.cell_at(1).expect("cell 1")), eol_of(&view_model.cell_at(2).expect("cell 2")));
            assert_join_edits(&ret.edits, &view_model.cell_at(0).expect("cell 0").uri(), &text, &editor.text_model.uri(), 1, 2);
        });
    }

    #[test]
    fn join_cell_with_above_multiple_cells() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;")], |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&index_state(range(2, 3), vec![range(1, 3)]), "model");
            let ret = join_notebook_cells(editor.as_ref(), range(1, 3), "above", None).expect("ret");
            let text = format!("{}var b = 2;{}var c = 3;", eol_of(&view_model.cell_at(1).expect("cell 1")), eol_of(&view_model.cell_at(2).expect("cell 2")));
            assert_join_edits(&ret.edits, &view_model.cell_at(0).expect("cell 0").uri(), &text, &editor.text_model.uri(), 1, 2);
        });
    }

    #[test]
    fn delete_focus_cell() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;")], |editor, view_model, _ds, _services| {
            editor.set_focus(range(0, 1));
            editor.set_selections(vec![range(0, 1)]);
            run_delete_action(editor.as_ref(), &view_model.cell_at(0).expect("cell"));
            assert_eq!(view_model.length(), 2);
        });
    }

    #[test]
    fn delete_selected_cells() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;")], |editor, view_model, _ds, _services| {
            editor.set_focus(range(0, 1));
            editor.set_selections(vec![range(0, 2)]);
            run_delete_action(editor.as_ref(), &view_model.cell_at(0).expect("cell"));
            assert_eq!(view_model.length(), 1);
        });
    }

    #[test]
    fn delete_focus_cell_out_of_a_selection() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;"), js("var d = 4;")], |editor, view_model, _ds, _services| {
            editor.set_focus(range(0, 1));
            editor.set_selections(vec![range(2, 4)]);
            run_delete_action(editor.as_ref(), &view_model.cell_at(0).expect("cell"));
            assert_eq!(view_model.length(), 3);
        });
    }

    #[test]
    fn delete_ui_target() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;")], |editor, view_model, _ds, _services| {
            editor.set_focus(range(0, 1));
            editor.set_selections(vec![range(0, 1)]);
            run_delete_action(editor.as_ref(), &view_model.cell_at(2).expect("cell"));
            assert_eq!(view_model.length(), 2);
            assert_eq!(text_at(view_model, 0), Some("var a = 1;".to_string()));
            assert_eq!(text_at(view_model, 1), Some("var b = 2;".to_string()));
        });
    }

    #[test]
    fn delete_ui_target_2() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(
            vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;"), js("var d = 4;"), js("var e = 5;")],
            |editor, view_model, _ds, _services| {
                editor.set_focus(range(0, 1));
                editor.set_selections(vec![range(0, 1), range(3, 5)]);
                run_delete_action(editor.as_ref(), &view_model.cell_at(1).expect("cell"));
                assert_eq!(view_model.length(), 4);
                assert_eq!(editor.get_focus(), range(0, 1));
                assert_eq!(view_model.get_selections(), [range(0, 1), range(2, 4)]);
            },
        );
    }

    #[test]
    fn delete_ui_target_3() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(
            vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;"), js("var d = 4;"), js("var e = 5;")],
            |editor, view_model, _ds, _services| {
                editor.set_focus(range(0, 1));
                editor.set_selections(vec![range(2, 3)]);
                run_delete_action(editor.as_ref(), &view_model.cell_at(0).expect("cell"));
                assert_eq!(view_model.length(), 4);
                assert_eq!(editor.get_focus(), range(0, 1));
                assert_eq!(view_model.get_selections(), [range(1, 2)]);
            },
        );
    }

    #[test]
    fn delete_ui_target_4() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(
            vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;"), js("var d = 4;"), js("var e = 5;")],
            |editor, view_model, _ds, _services| {
                editor.set_focus(range(2, 3));
                editor.set_selections(vec![range(3, 5)]);
                run_delete_action(editor.as_ref(), &view_model.cell_at(0).expect("cell"));
                assert_eq!(view_model.length(), 4);
                assert_eq!(editor.get_focus(), range(1, 2));
                assert_eq!(view_model.get_selections(), [range(2, 4)]);
            },
        );
    }

    #[test]
    fn delete_last_cell_sets_selection_correctly() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;")], |editor, view_model, _ds, _services| {
            editor.set_focus(range(2, 3));
            editor.set_selections(vec![range(2, 3)]);
            run_delete_action(editor.as_ref(), &view_model.cell_at(2).expect("cell"));
            assert_eq!(view_model.length(), 2);
            assert_eq!(editor.get_focus(), range(1, 2));
        });
    }

    #[test]
    fn issue_120187_delete_should_work_on_multiple_distinct_selection() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;"), js("var d = 4;")], |editor, view_model, _ds, _services| {
            editor.set_focus(range(0, 1));
            editor.set_selections(vec![range(0, 1), range(3, 4)]);
            run_delete_action(editor.as_ref(), &view_model.cell_at(0).expect("cell"));
            assert_eq!(view_model.length(), 2);
            assert_eq!(editor.get_focus(), range(0, 1));
        });
    }

    #[test]
    fn issue_120187_delete_should_work_on_multiple_distinct_selection_2() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(
            vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;"), js("var d = 4;"), js("var e = 5;")],
            |editor, view_model, _ds, _services| {
                editor.set_focus(range(1, 2));
                editor.set_selections(vec![range(1, 2), range(3, 5)]);
                run_delete_action(editor.as_ref(), &view_model.cell_at(1).expect("cell"));
                assert_eq!(view_model.length(), 2);
                assert_eq!(editor.get_focus(), range(1, 2));
            },
        );
    }

    #[test]
    fn change_cell_kind_single_cell() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(five_cells(), |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&index_state(range(1, 2), vec![range(1, 2)]), "model");
            change_cell_to_kind(CellKind::Markup, &context(editor).with_cell(view_model.cell_at(1).expect("cell")).with_ui(true), None, None);
            assert_eq!(view_model.cell_at(1).expect("cell 1").cell_kind(), CellKind::Markup);
        });
    }

    #[test]
    fn change_cell_kind_multi_cells() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(five_cells(), |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&index_state(range(1, 2), vec![range(1, 2)]), "model");
            let mut ctx = context(editor).with_ui(false);
            ctx.selected_cells = Some(vec![view_model.cell_at(3).expect("cell 3"), view_model.cell_at(4).expect("cell 4")]);
            change_cell_to_kind(CellKind::Markup, &ctx, None, None);
            assert_eq!(view_model.cell_at(3).expect("cell 3").cell_kind(), CellKind::Markup);
            assert_eq!(view_model.cell_at(4).expect("cell 4").cell_kind(), CellKind::Markup);
        });
    }

    #[test]
    fn split_cell() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![js("var b = 1;")], |_editor, view_model, _ds, _services| {
            let cell = view_model.cell_at(0).expect("cell");
            assert_eq!(compute_cell_lines_contents(&cell, &[Position::new(1, 4)]), Some(vec!["var".to_string(), " b = 1;".to_string()]));
            assert_eq!(
                compute_cell_lines_contents(&cell, &[Position::new(1, 4), Position::new(1, 6)]),
                Some(vec!["var".to_string(), " b".to_string(), " = 1;".to_string()])
            );
            assert_eq!(compute_cell_lines_contents(&cell, &[Position::new(1, 1)]), Some(vec!["".to_string(), "var b = 1;".to_string()]));
            assert_eq!(compute_cell_lines_contents(&cell, &[Position::new(1, 11)]), Some(vec!["var b = 1;".to_string(), "".to_string()]));
        });
    }

    #[test]
    fn insert_cell_case() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(five_cells(), |editor, view_model, _ds, services| {
            let language_service = services.language_service();
            let inserted_cell_above = insert_cell(language_service.as_ref(), editor.as_ref(), 4, CellKind::Code, "above", "var a = 0;", false).expect("above");
            assert_eq!(view_model.length(), 6);
            assert!(view_model.cell_at(4).expect("cell 4") == inserted_cell_above);

            let inserted_cell_below = insert_cell(language_service.as_ref(), editor.as_ref(), 1, CellKind::Code, "below", "var a = 0;", false).expect("below");
            assert_eq!(view_model.length(), 7);
            assert!(view_model.cell_at(2).expect("cell 2") == inserted_cell_below);
        });
    }
}
