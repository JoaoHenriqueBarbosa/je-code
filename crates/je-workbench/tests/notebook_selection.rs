//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookSelection.test.ts` (suítes `NotebookSelection` e
//! `NotebookCellList focus/selection`), caso a caso e na mesma ordem. Os 15 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso (`store`).
//! - `setState(null, ...)` é `set_state(None, ...)`, e `assert.deepStrictEqual` dos `{ start, end }` compara os `ICellRange`.
//! - O `setup` da suíte `NotebookCellList focus/selection` (o `DisposableStore`, o `setupInstantiationService` e o
//!   `ILanguageService`) é o que `with_test_notebook` já entrega (`ds` e `services`); o original cria uma instanciação
//!   separada que só alimenta a `TestCell` e a `createNotebookCellList`, e o resultado observável é o mesmo.
//! - `cellList.setFocus([1], new KeyboardEvent('keydown'), undefined)` é `set_focus(&[1], false)`: o `browserEvent` não é
//!   lido pela lista lógica. `cellList.focusNext(1, false)` é `focus_next(1, false, None)`.
//! - `runDeleteAction(editor, cell)` recebe o `editor` como `&dyn IActiveNotebookEditor`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_workbench::contrib::notebook::browser::controller::cell_operations::run_delete_action;
use je_workbench::contrib::notebook::browser::view_model::cell_selection_collection::NotebookCellSelectionCollection;
use je_workbench::contrib::notebook::browser::view_model::folding_model::{FoldingModel, update_folding_state_at_index};
use je_workbench::contrib::notebook::common::notebook_common::{
    CellKind, ICellEditOperation, ICellReplaceEdit, IOutputDto, ISelectionIndexState, ISelectionState, NotebookCellMetadata,
};
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use support::test_notebook_editor::{TestCell, create_notebook_cell_list, empty_metadata, with_test_notebook};

fn range(start: i64, end: i64) -> ICellRange {
    ICellRange { start, end }
}

mod notebook_selection {
    use super::*;

    #[test]
    fn focus_is_never_empty() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let selection_collection = NotebookCellSelectionCollection::new();
        assert_eq!(selection_collection.focus(), range(0, 0));

        selection_collection.set_state(None, &[], true, "model");
        assert_eq!(selection_collection.focus(), range(0, 0));
        selection_collection.dispose();
    }

    #[test]
    fn selection_is_never_empty() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let selection_collection = NotebookCellSelectionCollection::new();
        assert_eq!(selection_collection.selections(), [range(0, 0)]);

        selection_collection.set_state(None, &[], true, "model");
        assert_eq!(selection_collection.selections(), [range(0, 0)]);
        selection_collection.dispose();
    }

    #[test]
    fn selections_does_not_change_when_setting_to_empty() {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let selection_collection = NotebookCellSelectionCollection::new();
        let changed = Rc::new(Cell::new(false));
        store.add({
            let changed = changed.clone();
            selection_collection.on_did_change_selection().listen(move |_| changed.set(true))
        });

        selection_collection.set_state(None, &[], false, "model");
        assert!(!changed.get());
        selection_collection.set_state(Some(range(0, 0)), &[], false, "model");
        assert!(!changed.get());
        selection_collection.set_state(Some(range(0, 0)), &[range(0, 0)], false, "model");
        assert!(!changed.get());
        selection_collection.set_state(None, &[], false, "model");
        assert!(!changed.get());
        selection_collection.dispose();
    }

    #[test]
    fn event_fires_when_selection_or_focus_changes() {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let selection_collection = NotebookCellSelectionCollection::new();
        let event_count = Rc::new(Cell::new(0));
        store.add({
            let event_count = event_count.clone();
            selection_collection.on_did_change_selection().listen(move |_| event_count.set(event_count.get() + 1))
        });

        // Change focus
        selection_collection.set_state(Some(range(1, 1)), &[range(1, 2)], false, "model");
        assert_eq!(event_count.get(), 1);

        // Change selections
        selection_collection.set_state(Some(range(1, 1)), &[range(1, 2), range(2, 3)], false, "model");
        assert_eq!(event_count.get(), 2);

        // no change
        selection_collection.set_state(Some(range(1, 1)), &[range(1, 2), range(2, 3)], false, "model");
        assert_eq!(event_count.get(), 2);

        // change to empty focus
        selection_collection.set_state(Some(range(0, 0)), &[range(4, 5)], false, "model");
        assert_eq!(event_count.get(), 3);

        // change to empty selections
        selection_collection.set_state(Some(range(0, 0)), &[], false, "model");
        assert_eq!(event_count.get(), 4);

        selection_collection.dispose();
    }
}

mod notebook_cell_list_focus_selection {
    use super::*;

    type Cell = (&'static str, &'static str, CellKind, Vec<IOutputDto>, Option<NotebookCellMetadata>);

    fn code(source: &'static str) -> Cell {
        (source, "javascript", CellKind::Code, Vec::new(), Some(empty_metadata()))
    }

    fn markup(source: &'static str) -> Cell {
        (source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))
    }

    #[test]
    fn notebook_cell_list_set_focus() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![code("var a = 1;"), code("var b = 2;")], |_editor, view_model, ds, services| {
            let cell_list = create_notebook_cell_list(services, ds, None);
            cell_list.attach_view_model(view_model.clone());

            assert_eq!(cell_list.length(), 2);
            cell_list.set_focus(&[0], false);
            assert_eq!(view_model.get_focus(), range(0, 1));

            cell_list.set_focus(&[1], false);
            assert_eq!(view_model.get_focus(), range(1, 2));
            cell_list.detach_view_model();
        });
    }

    #[test]
    fn notebook_cell_list_set_selections() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![code("var a = 1;"), code("var b = 2;")], |_editor, view_model, ds, services| {
            let cell_list = create_notebook_cell_list(services, ds, None);
            cell_list.attach_view_model(view_model.clone());

            assert_eq!(cell_list.length(), 2);
            cell_list.set_selection(&[0], false);
            // the only selection is also the focus
            assert_eq!(view_model.get_selections(), [range(0, 1)]);

            // set selection does not modify focus
            cell_list.set_selection(&[1], false);
            assert_eq!(view_model.get_selections(), [range(1, 2)]);
        });
    }

    #[test]
    fn notebook_cell_list_set_focus2() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![code("var a = 1;"), code("var b = 2;")], |_editor, view_model, ds, services| {
            let cell_list = create_notebook_cell_list(services, ds, None);
            cell_list.attach_view_model(view_model.clone());

            assert_eq!(cell_list.length(), 2);
            cell_list.set_focus(&[0], false);
            assert_eq!(view_model.get_focus(), range(0, 1));

            cell_list.set_focus(&[1], false);
            assert_eq!(view_model.get_focus(), range(1, 2));

            cell_list.set_selection(&[1], false);
            assert_eq!(view_model.get_selections(), [range(1, 2)]);
            cell_list.detach_view_model();
        });
    }

    #[test]
    fn notebook_cell_list_focus_selection_from_ui() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(
            vec![markup("# header a"), code("var b = 1;"), markup("# header b"), code("var b = 2;"), markup("# header c")],
            |_editor, view_model, ds, services| {
                let cell_list = create_notebook_cell_list(services, ds, None);
                cell_list.attach_view_model(view_model.clone());
                assert_eq!(view_model.get_focus(), range(0, 1));
                assert_eq!(view_model.get_selections(), [range(0, 1)]);

                // arrow down, move both focus and selections
                cell_list.set_focus(&[1], false);
                cell_list.set_selection(&[1], false);
                assert_eq!(view_model.get_focus(), range(1, 2));
                assert_eq!(view_model.get_selections(), [range(1, 2)]);

                // shift+arrow down, expands selection
                cell_list.set_focus(&[2], false);
                cell_list.set_selection(&[1, 2], false);
                assert_eq!(view_model.get_focus(), range(2, 3));
                assert_eq!(view_model.get_selections(), [range(1, 3)]);

                // arrow down, will move focus but not expand selection
                cell_list.set_focus(&[3], false);
                assert_eq!(view_model.get_focus(), range(3, 4));
                assert_eq!(view_model.get_selections(), [range(1, 3)]);
            },
        );
    }

    #[test]
    fn notebook_cell_list_focus_selection_with_folding_regions() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(
            vec![markup("# header a"), code("var b = 1;"), markup("# header b"), code("var b = 2;"), markup("# header c")],
            |_editor, view_model, ds, services| {
                let folding_model = ds.add_rc(FoldingModel::new());
                folding_model.attach_view_model(view_model.clone());

                let cell_list = create_notebook_cell_list(services, ds, None);
                cell_list.attach_view_model(view_model.clone());
                assert_eq!(cell_list.length(), 5);
                assert_eq!(view_model.get_focus(), range(0, 1));
                assert_eq!(view_model.get_selections(), [range(0, 1)]);
                cell_list.set_focus(&[0], false);

                update_folding_state_at_index(&folding_model, 0, true);
                update_folding_state_at_index(&folding_model, 2, true);
                view_model.update_folding_ranges(folding_model.regions());
                cell_list.set_hidden_areas(&view_model.get_hidden_ranges(), true);
                assert_eq!(cell_list.length(), 3);

                // currently, focus on a folded cell will only focus the cell itself, excluding its "inner" cells
                assert_eq!(view_model.get_focus(), range(0, 1));
                assert_eq!(view_model.get_selections(), [range(0, 1)]);

                cell_list.focus_next(1, false, None);
                // focus next should skip the folded items
                assert_eq!(view_model.get_focus(), range(2, 3));
                assert_eq!(view_model.get_selections(), [range(0, 1)]);

                // unfold
                update_folding_state_at_index(&folding_model, 2, false);
                view_model.update_folding_ranges(folding_model.regions());
                cell_list.set_hidden_areas(&view_model.get_hidden_ranges(), true);
                assert_eq!(cell_list.length(), 4);
                assert_eq!(view_model.get_focus(), range(2, 3));
            },
        );
    }

    #[test]
    fn notebook_cell_list_focus_selection_with_folding_regions_and_apply_edits() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(
            vec![
                markup("# header a"),
                code("var b = 1;"),
                markup("# header b"),
                code("var b = 2;"),
                ("var c = 3", "javascript", CellKind::Markup, Vec::new(), Some(empty_metadata())),
                markup("# header d"),
                code("var e = 4;"),
            ],
            |editor, view_model, ds, services| {
                let folding_model = ds.add_rc(FoldingModel::new());
                folding_model.attach_view_model(view_model.clone());

                let cell_list = create_notebook_cell_list(services, ds, None);
                cell_list.attach_view_model(view_model.clone());
                cell_list.set_focus(&[0], false);
                cell_list.set_selection(&[0], false);

                update_folding_state_at_index(&folding_model, 0, true);
                update_folding_state_at_index(&folding_model, 2, true);
                view_model.update_folding_ranges(folding_model.regions());
                cell_list.set_hidden_areas(&view_model.get_hidden_ranges(), true);
                assert_eq!(cell_list.get_model_index2(0), Some(0));
                assert_eq!(cell_list.get_model_index2(1), Some(2));

                editor.text_model.apply_edits(&[ICellEditOperation::Replace(ICellReplaceEdit { index: 0, count: 2, cells: Vec::new() })], true, None, &|| None, None, false);
                view_model.update_folding_ranges(folding_model.regions());
                cell_list.set_hidden_areas(&view_model.get_hidden_ranges(), true);

                assert_eq!(cell_list.get_model_index2(0), Some(0));
                assert_eq!(cell_list.get_model_index2(1), Some(3));

                // mimic undo
                let language_service = services.language_service();
                let cell_7 = ds.add_rc(Rc::new(TestCell::new(&view_model.view_type, 7, "# header f", "markdown", CellKind::Code, Vec::new(), language_service.clone())));
                let cell_8 = ds.add_rc(Rc::new(TestCell::new(&view_model.view_type, 8, "var g = 5;", "javascript", CellKind::Code, Vec::new(), language_service)));
                editor.text_model.apply_edits(
                    &[ICellEditOperation::Replace(ICellReplaceEdit { index: 0, count: 0, cells: vec![cell_7.cell_dto(), cell_8.cell_dto()] })],
                    true,
                    None,
                    &|| None,
                    None,
                    false,
                );
                view_model.update_folding_ranges(folding_model.regions());
                cell_list.set_hidden_areas(&view_model.get_hidden_ranges(), true);
                assert_eq!(cell_list.get_model_index2(0), Some(0));
                assert_eq!(cell_list.get_model_index2(1), Some(1));
                assert_eq!(cell_list.get_model_index2(2), Some(2));
            },
        );
    }

    #[test]
    fn notebook_cell_list_get_model_index() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(
            vec![markup("# header a"), code("var b = 1;"), markup("# header b"), code("var b = 2;"), markup("# header c")],
            |_editor, view_model, ds, services| {
                let folding_model = ds.add_rc(FoldingModel::new());
                folding_model.attach_view_model(view_model.clone());

                let cell_list = create_notebook_cell_list(services, ds, None);
                cell_list.attach_view_model(view_model.clone());

                update_folding_state_at_index(&folding_model, 0, true);
                update_folding_state_at_index(&folding_model, 2, true);
                view_model.update_folding_ranges(folding_model.regions());
                cell_list.set_hidden_areas(&view_model.get_hidden_ranges(), true);

                assert_eq!(cell_list.get_model_index2(-1), Some(0));
                assert_eq!(cell_list.get_model_index2(0), Some(0));
                assert_eq!(cell_list.get_model_index2(1), Some(2));
                assert_eq!(cell_list.get_model_index2(2), Some(4));
            },
        );
    }

    #[test]
    fn notebook_validate_range() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![markup("# header a"), code("var b = 1;")], |_editor, view_model, _ds, _services| {
            assert_eq!(view_model.validate_range(None), None);
            assert_eq!(view_model.validate_range(Some(range(0, 0))), Some(range(0, 0)));
            assert_eq!(view_model.validate_range(Some(range(0, 2))), Some(range(0, 2)));
            assert_eq!(view_model.validate_range(Some(range(0, 3))), Some(range(0, 2)));
            assert_eq!(view_model.validate_range(Some(range(-1, 3))), Some(range(0, 2)));
            assert_eq!(view_model.validate_range(Some(range(-1, 1))), Some(range(0, 1)));
            assert_eq!(view_model.validate_range(Some(range(2, 1))), Some(range(1, 2)));
            assert_eq!(view_model.validate_range(Some(range(2, -1))), Some(range(0, 2)));
        });
    }

    #[test]
    fn notebook_update_selection_state() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![markup("# header a"), code("var b = 1;")], |_editor, view_model, _ds, _services| {
            view_model.update_selections_state(
                &ISelectionState::Index(ISelectionIndexState { focus: range(1, 2), selections: vec![range(1, 2), range(-1, 0)] }),
                "model",
            );
            assert_eq!(view_model.get_selections(), [range(1, 2)]);
        });
    }

    #[test]
    fn notebook_cell_selection_with_cell_deletion() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![markup("# header a"), code("var b = 1;")], |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&ISelectionState::Index(ISelectionIndexState { focus: range(1, 2), selections: vec![range(1, 2)] }), "model");
            run_delete_action(editor.as_ref(), &view_model.cell_at(1).expect("cell 1"));
            // viewModel.deleteCell(1, true, false);
            assert_eq!(view_model.get_focus(), range(0, 1));
            assert_eq!(view_model.get_selections(), [range(0, 1)]);
            run_delete_action(editor.as_ref(), &view_model.cell_at(0).expect("cell 0"));
            assert_eq!(view_model.get_focus(), range(0, 0));
            assert_eq!(view_model.get_selections(), [range(0, 0)]);
        });
    }

    #[test]
    fn notebook_cell_selection_with_cell_deletion_from_apply_edits() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        with_test_notebook(vec![markup("# header a"), code("var b = 1;"), code("var c = 2;")], |editor, view_model, _ds, _services| {
            view_model.update_selections_state(&ISelectionState::Index(ISelectionIndexState { focus: range(1, 2), selections: vec![range(1, 2)] }), "model");
            editor.text_model.apply_edits(&[ICellEditOperation::Replace(ICellReplaceEdit { index: 1, count: 1, cells: Vec::new() })], true, None, &|| None, None, true);
            assert_eq!(view_model.get_focus(), range(1, 2));
            assert_eq!(view_model.get_selections(), [range(1, 2)]);
        });
    }
}
