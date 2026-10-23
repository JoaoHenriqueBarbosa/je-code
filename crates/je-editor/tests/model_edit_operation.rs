//! Porte de `src/vs/editor/test/common/model/modelEditOperation.test.ts` (suíte `Editor Model - Model Edit Operation`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O `setup`/`teardown` do original vira `with_model`, que cria o modelo, roda o caso e o descarta.
//! - `applyEdits(ops, true)` é `apply_edits_computing_undo`. As operações inversas que ele devolve
//!   (`IValidEditOperation`) voltam ao modelo como `SingleEditOperation`; o original as passa direto, e o
//!   `forceMoveMarkers` ausente delas é o `false` que o `simplifyEdit` do original já assume.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::model::ValidEditOperation;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use support::test_text_model::*;

const LINE1: &str = "My First Line";
const LINE2: &str = "\t\tMy Second Line";
const LINE3: &str = "    Third Line";
const LINE4: &str = "";
const LINE5: &str = "1";

fn with_model(test: impl FnOnce(&TextModel)) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let text = format!("{LINE1}\r\n{LINE2}\n{LINE3}\n{LINE4}\r\n{LINE5}");
    let model = create_text_model(&text);
    test(&model);
    model.dispose();
}

/// `createSingleEditOp(text, positionLineNumber, positionColumn, selectionLineNumber = positionLineNumber, selectionColumn = positionColumn)`.
fn create_single_edit_op(
    text: &str,
    position_line_number: i32,
    position_column: i32,
    selection: Option<(i32, i32)>,
) -> SingleEditOperation {
    let (selection_line_number, selection_column) = selection.unwrap_or((position_line_number, position_column));
    let range = Range::new(selection_line_number, selection_column, position_line_number, position_column);

    SingleEditOperation::new(range, Some(JsString::from(text)), false)
}

/// O `IValidEditOperation` passado de volta a `applyEdits`.
fn to_single_edit_operation(edit: &ValidEditOperation) -> SingleEditOperation {
    let mut operation = SingleEditOperation::new(edit.range, Some(edit.text.clone()), false);
    operation.identifier = edit.identifier;
    operation
}

fn assert_single_edit_op(model: &TextModel, single_edit_op: SingleEditOperation, edited_lines: &[&str]) {
    let edit_op = [single_edit_op];

    let inverse_edit_op = model.apply_edits_computing_undo(&edit_op);

    assert_eq!(model.get_line_count() as usize, edited_lines.len());
    for (i, edited_line) in edited_lines.iter().enumerate() {
        assert_eq!(model.get_line_content(i as i32 + 1), *edited_line);
    }

    let inverse_edit_op: Vec<SingleEditOperation> = inverse_edit_op.iter().map(to_single_edit_operation).collect();
    let original_op = model.apply_edits_computing_undo(&inverse_edit_op);

    assert_eq!(model.get_line_count(), 5);
    assert_eq!(model.get_line_content(1), LINE1);
    assert_eq!(model.get_line_content(2), LINE2);
    assert_eq!(model.get_line_content(3), LINE3);
    assert_eq!(model.get_line_content(4), LINE4);
    assert_eq!(model.get_line_content(5), LINE5);

    // simplifyEdit: { range, text, forceMoveMarkers || false }
    let original_op: Vec<(Range, Option<JsString>, bool)> =
        original_op.iter().map(|edit| (edit.range, Some(edit.text.clone()), false)).collect();
    let edit_op: Vec<(Range, Option<JsString>, bool)> =
        edit_op.iter().map(|edit| (edit.range, edit.text.clone(), edit.force_move_markers)).collect();
    assert_eq!(original_op, edit_op);
}

#[test]
fn insert_inline() {
    with_model(|model| {
        assert_single_edit_op(model, create_single_edit_op("a", 1, 1, None), &["aMy First Line", LINE2, LINE3, LINE4, LINE5]);
    });
}

#[test]
/// `Replace inline/inline 1`
fn replace_inline_inline_1() {
    with_model(|model| {
        assert_single_edit_op(
            model,
            create_single_edit_op(" incredibly awesome", 1, 3, None),
            &["My incredibly awesome First Line", LINE2, LINE3, LINE4, LINE5],
        );
    });
}

#[test]
/// `Replace inline/inline 2`
fn replace_inline_inline_2() {
    with_model(|model| {
        assert_single_edit_op(
            model,
            create_single_edit_op(" with text at the end.", 1, 14, None),
            &["My First Line with text at the end.", LINE2, LINE3, LINE4, LINE5],
        );
    });
}

#[test]
/// `Replace inline/inline 3`
fn replace_inline_inline_3() {
    with_model(|model| {
        assert_single_edit_op(
            model,
            create_single_edit_op("My new First Line.", 1, 1, Some((1, 14))),
            &["My new First Line.", LINE2, LINE3, LINE4, LINE5],
        );
    });
}

#[test]
/// `Replace inline/multi line 1`
fn replace_inline_multi_line_1() {
    with_model(|model| {
        assert_single_edit_op(
            model,
            create_single_edit_op("My new First Line.", 1, 1, Some((3, 15))),
            &["My new First Line.", LINE4, LINE5],
        );
    });
}

#[test]
/// `Replace inline/multi line 2`
fn replace_inline_multi_line_2() {
    with_model(|model| {
        assert_single_edit_op(
            model,
            create_single_edit_op("My new First Line.", 1, 2, Some((3, 15))),
            &["MMy new First Line.", LINE4, LINE5],
        );
    });
}

#[test]
/// `Replace inline/multi line 3`
fn replace_inline_multi_line_3() {
    with_model(|model| {
        assert_single_edit_op(
            model,
            create_single_edit_op("My new First Line.", 1, 2, Some((3, 2))),
            &["MMy new First Line.   Third Line", LINE4, LINE5],
        );
    });
}

#[test]
/// `Replace muli line/multi line`
fn replace_muli_line_multi_line() {
    with_model(|model| {
        assert_single_edit_op(
            model,
            create_single_edit_op("1\n2\n3\n4\n", 1, 1, None),
            &["1", "2", "3", "4", LINE1, LINE2, LINE3, LINE4, LINE5],
        );
    });
}
