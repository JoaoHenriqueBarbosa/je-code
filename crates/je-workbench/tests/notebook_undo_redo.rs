//! Porte de `src/vs/workbench/contrib/notebook/test/browser/contrib/notebookUndoRedo.test.ts` (suíte `Notebook Undo/Redo`), caso a caso e
//! na mesma ordem. Os 6 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso.
//! - Os callbacks `async` do `withTestNotebook` são síncronos, e o `await viewModel.undo()`/`redo()` é a chamada direta (o
//!   `UndoRedoService` do porte é síncrono).
//! - `editor.textModel.applyEdits(edits, true, undefined, () => undefined, undefined, true)` é [`apply_edits`].
//! - `{ source, language: 'markdown', cellKind: CellKind.Markup, outputs: [], mime: undefined }` é [`markup_dto`]: os campos
//!   que o original não escreve (`metadata`, `internalMetadata`, `collapseState`) ficam ausentes.
//! - `assert.doesNotThrow(() => ...)` é `errors::catch(...).is_ok()`.
//! - `createNotebookCellList(accessor, disposables)` é `create_notebook_cell_list(services, disposables, None)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use common::{assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::errors;
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::{
    CellKind, ICellDto2, ICellEditOperation, ICellMetadataEdit, ICellReplaceEdit, ISelectionIndexState, ISelectionState,
};
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use support::test_notebook_editor::{MockNotebookCell, create_notebook_cell_list, empty_metadata, with_test_notebook};

fn md(source: &'static str) -> MockNotebookCell {
    (source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))
}

fn two_cells() -> Vec<MockNotebookCell> {
    vec![md("# header 1"), md("body")]
}

fn range(start: i64, end: i64) -> ICellRange {
    ICellRange { start, end }
}

/// `{ source, language: 'markdown', cellKind: CellKind.Markup, outputs: [], mime: undefined }`.
fn markup_dto(source: &str) -> ICellDto2 {
    ICellDto2 {
        source: JsString::from(source),
        mime: None,
        language: "markdown".to_string(),
        cell_kind: CellKind::Markup,
        outputs: Vec::new(),
        metadata: None,
        internal_metadata: None,
        collapse_state: None,
    }
}

/// `{ editType: CellEditType.Replace, index, count, cells }`.
fn replace_edit(index: usize, count: usize, cells: Vec<ICellDto2>) -> ICellEditOperation {
    ICellEditOperation::Replace(ICellReplaceEdit { index, count, cells })
}

/// `textModel.applyEdits(edits, true, undefined, () => undefined, undefined, true)`.
fn apply_edits(text_model: &NotebookTextModel, edits: &[ICellEditOperation]) -> bool {
    text_model.apply_edits(edits, true, None, &|| None, None, true)
}

/// `{ focus: { start, end }, selections: [{ start, end }], kind: SelectionStateType.Index }`.
fn index_state(start: i64, end: i64) -> ISelectionState {
    ISelectionState::Index(ISelectionIndexState { focus: range(start, end), selections: vec![range(start, end)] })
}

#[test]
fn basics() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(two_cells(), |editor, view_model, _ds, _services| {
        assert_eq!(view_model.length(), 2);
        assert_eq!(view_model.get_version_id(), 0);
        assert_eq!(view_model.get_alternative_id(), "0_0,1;1,1");

        apply_edits(&editor.text_model, &[replace_edit(0, 2, Vec::new())]);
        assert_eq!(view_model.length(), 0);
        assert_eq!(view_model.get_version_id(), 1);
        assert_eq!(view_model.get_alternative_id(), "1_");

        view_model.undo();
        assert_eq!(view_model.length(), 2);
        assert_eq!(view_model.get_version_id(), 2);
        assert_eq!(view_model.get_alternative_id(), "0_0,1;1,1");

        view_model.redo();
        assert_eq!(view_model.length(), 0);
        assert_eq!(view_model.get_version_id(), 3);
        assert_eq!(view_model.get_alternative_id(), "1_");

        apply_edits(&editor.text_model, &[replace_edit(0, 0, vec![markup_dto("# header 3")])]);
        assert_eq!(view_model.get_version_id(), 4);
        assert_eq!(view_model.get_alternative_id(), "4_2,1");

        view_model.undo();
        assert_eq!(view_model.get_version_id(), 5);
        assert_eq!(view_model.get_alternative_id(), "1_");
    });
}

#[test]
fn invalid_replace_count_should_not_throw() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(two_cells(), |editor, _view_model, _ds, _services| {
        apply_edits(&editor.text_model, &[replace_edit(0, 2, Vec::new())]);

        assert!(errors::catch(|| apply_edits(&editor.text_model, &[replace_edit(0, 2, vec![markup_dto("# header 2")])])).is_ok());
    });
}

#[test]
fn replace_beyond_length() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(two_cells(), |editor, view_model, _ds, _services| {
        apply_edits(&editor.text_model, &[replace_edit(1, 2, Vec::new())]);

        assert_eq!(view_model.length(), 1);
        view_model.undo();
        assert_eq!(view_model.length(), 2);
    });
}

#[test]
fn invalid_replace_count_should_not_affect_undo_redo() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(two_cells(), |editor, view_model, _ds, _services| {
        apply_edits(&editor.text_model, &[replace_edit(0, 2, Vec::new())]);

        apply_edits(&editor.text_model, &[replace_edit(0, 2, vec![markup_dto("# header 2")])]);

        assert_eq!(view_model.length(), 1);

        view_model.undo();
        view_model.undo();

        assert_eq!(view_model.length(), 2);
        apply_edits(&editor.text_model, &[replace_edit(1, 2, Vec::new())]);
        assert_eq!(view_model.length(), 1);
    });
}

#[test]
fn focus_selection_update() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(two_cells(), |editor, view_model, ds, services| {
        let cell_list = create_notebook_cell_list(services, ds, None);
        cell_list.attach_view_model(view_model.clone());
        cell_list.set_focus(&[1], false);

        editor.text_model.apply_edits(
            &[replace_edit(2, 0, vec![markup_dto("# header 2")])],
            true,
            Some(index_state(1, 2)),
            &|| Some(index_state(2, 3)),
            None,
            true,
        );
        assert_eq!(view_model.length(), 3);
        assert_eq!(view_model.get_version_id(), 1);
        assert_eq!(cell_list.get_focus(), vec![2]);
        assert_eq!(cell_list.get_selection(), vec![2]);

        view_model.undo();
        assert_eq!(view_model.length(), 2);
        assert_eq!(view_model.get_version_id(), 2);
        assert_eq!(cell_list.get_focus(), vec![1]);
        assert_eq!(cell_list.get_selection(), vec![1]);

        view_model.redo();
        assert_eq!(view_model.length(), 3);
        assert_eq!(view_model.get_version_id(), 3);
        assert_eq!(cell_list.get_focus(), vec![2]);
        assert_eq!(cell_list.get_selection(), vec![2]);
    });
}

#[test]
fn batch_edits() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(two_cells(), |editor, view_model, _ds, _services| {
        let input_collapsed_false = Dynamic::object([("inputCollapsed", Dynamic::Bool(false))]);
        apply_edits(
            &editor.text_model,
            &[
                replace_edit(2, 0, vec![markup_dto("# header 2")]),
                ICellEditOperation::Metadata(ICellMetadataEdit { index: 0, metadata: input_collapsed_false.clone() }),
            ],
        );
        assert_eq!(view_model.get_version_id(), 1);
        assert_deep_strict_equal(&view_model.cell_at(0).expect("célula 0").base().metadata(), &input_collapsed_false);

        view_model.undo();
        assert_eq!(view_model.length(), 2);
        assert_eq!(view_model.get_version_id(), 2);
        assert_deep_strict_equal(&view_model.cell_at(0).expect("célula 0").base().metadata(), &empty_metadata());

        view_model.redo();
        assert_eq!(view_model.length(), 3);
        assert_eq!(view_model.get_version_id(), 3);
        assert_deep_strict_equal(&view_model.cell_at(0).expect("célula 0").base().metadata(), &input_collapsed_false);
    });
}
