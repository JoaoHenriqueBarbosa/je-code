//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookViewModel.test.ts` (suítes `NotebookViewModel`,
//! `NotebookViewModel Decorations` e `NotebookViewModel API`), caso a caso e na mesma ordem. Estão portados 9 dos 10 casos.
//!
//! Casos de fora:
//! - `deleted cells are removed from the disposable store`: o caso troca `cell.dispose` por uma função que conta as chamadas e
//!   confere que `runDeleteAction` chama `dispose` exatamente uma vez e que o descarte do notebook não chama de novo. Em Rust
//!   o `dispose` da célula não é substituível, e o `Disposable` interno é idempotente, então nenhum efeito observável
//!   distingue uma chamada de duas (o caso passaria sem provar nada). Por isso não vira `#[test]`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso (`_store`).
//! - `suiteSetup` (o `setupInstantiationService`) do caso `ctor` é `setup_notebook_services`; o `NotebookEditorTestModel` só
//!   guarda o notebook e não é lido por nenhum caso (ver `support/test_notebook_editor.rs`), então `model.dispose()` não existe.
//! - `insertCellAtIndex(..., {}, [], true, true)` é `insert_cell_at_index(..., Some(empty_metadata()), vec![], true, true)`.
//! - `setTrackedRange('test', ...)` é `set_tracked_range(Some("test"), Some(...), ...)`.
//! - `viewModel.getCellsInRange()` sem argumento é `get_cells_in_range(None)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::sequence::Splice;
use je_base::uri::URI;
use je_editor::model::TrackedRangeStickiness;
use je_editor::undo_redo_service::IUndoRedoService;
use je_workbench::contrib::notebook::browser::controller::cell_operations::{insert_cell_at_index, run_delete_action};
use je_workbench::contrib::notebook::browser::view_model::event_dispatcher::NotebookEventDispatcher;
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::{NotebookViewModel, NotebookViewModelOptions};
use je_workbench::contrib::notebook::browser::view_model::view_context::{IBaseCellEditorOptions, ViewContext};
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, TransientOptions, diff};
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use support::test_notebook_editor::{
    MockBaseCellEditorOptions, MockNotebookCell, create_notebook_options, empty_metadata, setup_notebook_services, with_test_notebook,
};

fn range(start: i64, end: i64) -> ICellRange {
    ICellRange { start, end }
}

fn js(source: &'static str) -> MockNotebookCell {
    (source, "javascript", CellKind::Code, Vec::new(), Some(empty_metadata()))
}

fn md(source: &'static str) -> MockNotebookCell {
    (source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))
}

fn get_visible_cells<T: Clone>(cells: &[T], hidden_ranges: &[ICellRange]) -> Vec<T> {
    if hidden_ranges.is_empty() {
        return cells.to_vec();
    }

    let mut start: i64 = 0;
    let mut hidden_range_index = 0;
    let mut result: Vec<T> = Vec::new();

    while (start as usize) < cells.len() && hidden_range_index < hidden_ranges.len() {
        if start < hidden_ranges[hidden_range_index].start {
            result.extend_from_slice(&cells[start as usize..hidden_ranges[hidden_range_index].start as usize]);
        }

        start = hidden_ranges[hidden_range_index].end + 1;
        hidden_range_index += 1;
    }

    if (start as usize) < cells.len() {
        result.extend_from_slice(&cells[start as usize..]);
    }

    result
}

mod notebook_view_model {
    use super::*;

    #[test]
    fn ctor() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let disposables = DisposableStore::new();
        let services = setup_notebook_services(&disposables);
        let notebook = NotebookTextModel::new(
            "notebook",
            URI::parse("test", false),
            Vec::new(),
            empty_metadata(),
            TransientOptions::default(),
            services.undo_redo_service.clone(),
            services.model_service.clone(),
            services.language_service(),
            services.language_detection_service(),
            services.notebook_execution_state_service(),
            services.notebook_logging_service(),
        );
        let options = create_notebook_options(&services, &disposables);
        let event_dispatcher = NotebookEventDispatcher::new();
        let base_cell_editor_options: Rc<dyn IBaseCellEditorOptions> = Rc::new(MockBaseCellEditorOptions);
        let view_context = ViewContext::new(options.clone(), event_dispatcher.clone(), move |_language| base_cell_editor_options.clone());
        let undo_redo_service: Rc<dyn IUndoRedoService> = services.undo_redo_service.clone();
        let view_model = NotebookViewModel::new(
            "notebook",
            notebook.clone(),
            view_context,
            None,
            NotebookViewModelOptions { is_read_only: false },
            services.configuration_service.clone(),
            undo_redo_service,
            services.notebook_execution_state_service(),
        );
        assert_eq!(view_model.view_type, "notebook");
        notebook.dispose();
        options.dispose();
        event_dispatcher.dispose();
        view_model.dispose();
        disposables.dispose();
    }

    #[test]
    fn insert_delete() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![js("var a = 1;"), js("var b = 2;")], |editor, view_model, _ds, _services| {
            let cell = insert_cell_at_index(view_model, 1, "var c = 3", "javascript", CellKind::Code, Some(empty_metadata()), Vec::new(), true, true);
            assert_eq!(view_model.length(), 3);
            assert_eq!(view_model.notebook_document().cells().len(), 3);
            assert_eq!(view_model.get_cell_index(&cell), 1);

            run_delete_action(editor.as_ref(), &view_model.cell_at(1).expect("cell 1"));
            assert_eq!(view_model.length(), 2);
            assert_eq!(view_model.notebook_document().cells().len(), 2);
            assert_eq!(view_model.get_cell_index(&cell), -1);

            cell.dispose();
            cell.model().dispose();
        });
    }

    #[test]
    fn index() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![js("var a = 1;"), js("var b = 2;")], |editor, view_model, _ds, _services| {
            let first_view_cell = view_model.cell_at(0).expect("cell 0");
            let last_view_cell = view_model.cell_at(view_model.length() as i64 - 1).expect("last cell");

            let insert_index = view_model.get_cell_index(&first_view_cell) + 1;
            let cell = insert_cell_at_index(view_model, insert_index, "var c = 3;", "javascript", CellKind::Code, Some(empty_metadata()), Vec::new(), true, true);

            let added_cell_index = view_model.get_cell_index(&cell);
            run_delete_action(editor.as_ref(), &view_model.cell_at(added_cell_index).expect("added cell"));

            let second_insert_index = view_model.get_cell_index(&last_view_cell) + 1;
            let cell2 = insert_cell_at_index(view_model, second_insert_index, "var d = 4;", "javascript", CellKind::Code, Some(empty_metadata()), Vec::new(), true, true);

            assert_eq!(view_model.length(), 3);
            assert_eq!(view_model.notebook_document().cells().len(), 3);
            assert_eq!(view_model.get_cell_index(&cell2), 2);

            cell.dispose();
            cell.model().dispose();
            cell2.dispose();
            cell2.model().dispose();
        });
    }
}

mod notebook_view_model_decorations {
    use super::*;

    #[test]
    fn tracking_range() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;"), js("var d = 4;"), js("var e = 5;")], |editor, view_model, _ds, _services| {
            let tracked_id = view_model
                .set_tracked_range(Some("test"), Some(range(1, 2)), TrackedRangeStickiness::GrowsOnlyWhenTypingAfter)
                .expect("tracked id");
            assert_eq!(view_model.get_tracked_range(&tracked_id), Some(range(1, 2)));

            let cell1 = insert_cell_at_index(view_model, 0, "var d = 6;", "javascript", CellKind::Code, Some(empty_metadata()), Vec::new(), true, true);
            assert_eq!(view_model.get_tracked_range(&tracked_id), Some(range(2, 3)));

            run_delete_action(editor.as_ref(), &view_model.cell_at(0).expect("cell 0"));
            assert_eq!(view_model.get_tracked_range(&tracked_id), Some(range(1, 2)));

            let cell2 = insert_cell_at_index(view_model, 3, "var d = 7;", "javascript", CellKind::Code, Some(empty_metadata()), Vec::new(), true, true);
            assert_eq!(view_model.get_tracked_range(&tracked_id), Some(range(1, 3)));

            run_delete_action(editor.as_ref(), &view_model.cell_at(3).expect("cell 3"));
            assert_eq!(view_model.get_tracked_range(&tracked_id), Some(range(1, 2)));

            run_delete_action(editor.as_ref(), &view_model.cell_at(1).expect("cell 1"));
            assert_eq!(view_model.get_tracked_range(&tracked_id), Some(range(0, 1)));

            cell1.dispose();
            cell1.model().dispose();
            cell2.dispose();
            cell2.model().dispose();
        });
    }

    #[test]
    fn tracking_range_2() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(
            vec![js("var a = 1;"), js("var b = 2;"), js("var c = 3;"), js("var d = 4;"), js("var e = 5;"), js("var e = 6;"), js("var e = 7;")],
            |_editor, view_model, _ds, _services| {
                let tracked_id = view_model
                    .set_tracked_range(Some("test"), Some(range(1, 3)), TrackedRangeStickiness::GrowsOnlyWhenTypingAfter)
                    .expect("tracked id");
                assert_eq!(view_model.get_tracked_range(&tracked_id), Some(range(1, 3)));

                insert_cell_at_index(view_model, 5, "var d = 9;", "javascript", CellKind::Code, Some(empty_metadata()), Vec::new(), true, true);
                assert_eq!(view_model.get_tracked_range(&tracked_id), Some(range(1, 3)));

                insert_cell_at_index(view_model, 4, "var d = 10;", "javascript", CellKind::Code, Some(empty_metadata()), Vec::new(), true, true);
                assert_eq!(view_model.get_tracked_range(&tracked_id), Some(range(1, 4)));
            },
        );
    }

    #[test]
    fn diff_hidden_ranges() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(get_visible_cells::<i64>(&[1, 2, 3, 4, 5], &[]), vec![1, 2, 3, 4, 5]);

        assert_eq!(get_visible_cells::<i64>(&[1, 2, 3, 4, 5], &[range(1, 2)]), vec![1, 4, 5]);

        assert_eq!(get_visible_cells::<i64>(&[1, 2, 3, 4, 5, 6, 7, 8, 9], &[range(1, 2), range(4, 5)]), vec![1, 4, 7, 8, 9]);

        let original = get_visible_cells::<i64>(&[1, 2, 3, 4, 5, 6, 7, 8, 9], &[range(1, 2), range(4, 5)]);

        let modified = get_visible_cells::<i64>(&[1, 2, 3, 4, 5, 6, 7, 8, 9], &[range(2, 4)]);

        assert_eq!(diff::<i64>(&original, &modified, |a| original.contains(a), None), vec![Splice { start: 1, delete_count: 1, to_insert: vec![2, 6] }]);
    }
}

mod notebook_view_model_api {
    use super::*;

    #[test]
    fn issue_115432_get_nearest_code_cell() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(
            vec![
                md("# header a"),
                js("var b = 1;"),
                md("# header b"),
                ("b = 2;", "python", CellKind::Code, Vec::new(), Some(empty_metadata())),
                js("var c = 3"),
                md("# header d"),
                ("var e = 4;", "TypeScript", CellKind::Code, Vec::new(), Some(empty_metadata())),
                md("# header f"),
            ],
            |_editor, view_model, _ds, _services| {
                assert_eq!(view_model.nearest_code_cell_index(0), 1);
                // find the nearest code cell from above
                assert_eq!(view_model.nearest_code_cell_index(2), 1);
                assert_eq!(view_model.nearest_code_cell_index(4), 3);
                assert_eq!(view_model.nearest_code_cell_index(5), 4);
                assert_eq!(view_model.nearest_code_cell_index(6), 4);
            },
        );
    }

    #[test]
    fn issue_108464_get_nearest_code_cell() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![md("# header a"), js("var b = 1;"), md("# header b")], |_editor, view_model, _ds, _services| {
            assert_eq!(view_model.nearest_code_cell_index(2), 1);
        });
    }

    #[test]
    fn get_cells() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![md("# header a"), js("var b = 1;"), md("# header b")], |_editor, view_model, _ds, _services| {
            let texts = |range: Option<ICellRange>| view_model.get_cells_in_range(range).iter().map(|cell| cell.get_text()).collect::<Vec<_>>();

            assert_eq!(view_model.get_cells_in_range(None).len(), 3);
            assert_eq!(texts(Some(range(0, 1))), ["# header a"]);
            assert_eq!(texts(Some(range(0, 2))), ["# header a", "var b = 1;"]);
            assert_eq!(texts(Some(range(0, 3))), ["# header a", "var b = 1;", "# header b"]);
            assert_eq!(texts(Some(range(0, 4))), ["# header a", "var b = 1;", "# header b"]);
            assert_eq!(texts(Some(range(1, 4))), ["var b = 1;", "# header b"]);
            assert_eq!(texts(Some(range(2, 4))), ["# header b"]);
            assert_eq!(texts(Some(range(3, 4))), Vec::<String>::new());

            // no one should use an invalid range but `getCells` should be able to handle that.
            assert_eq!(texts(Some(range(-1, 1))), ["# header a"]);
            assert_eq!(texts(Some(range(3, 0))), ["# header a", "var b = 1;", "# header b"]);
        });
    }
}
