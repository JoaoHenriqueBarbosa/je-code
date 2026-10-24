//! Porte de `src/vs/editor/test/common/model/linesTextBuffer/linesTextBuffer.test.ts` (suítes
//! `PieceTreeTextBuffer._getInverseEdits` e `PieceTreeTextBuffer._toSingleEditOperation`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `text: string[] | null` vira `Option<&[&str]>`; `null` é `None`.
//! - `new Range(...)` é `Range::new(...)` e `deepStrictEqual` é `assert_eq!` (o `ValidatedEditOperation`
//!   deriva `PartialEq`).
//! - `createTextBufferFactory(...).create(DefaultEndOfLine.LF)` devolve o buffer; o `disposable` do original
//!   é `buffer.disposable()`.
//! - Os nomes dos casos que se repetem nas duas suítes (`many edits`) levam o prefixo da suíte.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_editor::model::DefaultEndOfLine;
use je_editor::piece_tree_text_buffer::{PieceTreeTextBuffer, ValidatedEditOperation};
use je_editor::range::Range;
use je_editor::text_model::create_text_buffer_factory;

/// `editOp` das duas suítes: a primeira não informa `rangeOffset` nem `rangeLength` (zeros).
fn edit_op_full(
    start_line_number: i32,
    start_column: i32,
    end_line_number: i32,
    end_column: i32,
    range_offset: usize,
    range_length: usize,
    text: Option<&[&str]>,
) -> ValidatedEditOperation {
    let joined = text.map(|lines| lines.join("\n")).unwrap_or_default();
    ValidatedEditOperation {
        sort_index: 0,
        identifier: None,
        range: Range::new(start_line_number, start_column, end_line_number, end_column),
        range_offset,
        range_length,
        text: JsString::from(joined.as_str()),
        eol_count: text.map(|lines| lines.len() - 1).unwrap_or(0),
        first_line_length: text.map(|lines| lines[0].encode_utf16().count()).unwrap_or(0),
        last_line_length: text.map(|lines| lines[lines.len() - 1].encode_utf16().count()).unwrap_or(0),
        force_move_markers: false,
        is_auto_whitespace_edit: false,
    }
}

// suite('PieceTreeTextBuffer._getInverseEdits')

fn edit_op(start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32, text: Option<&[&str]>) -> ValidatedEditOperation {
    edit_op_full(start_line_number, start_column, end_line_number, end_column, 0, 0, text)
}

fn inverse_edit_op(start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) -> Range {
    Range::new(start_line_number, start_column, end_line_number, end_column)
}

fn assert_inverse_edits(ops: Vec<ValidatedEditOperation>, expected: Vec<Range>) {
    let actual = PieceTreeTextBuffer::get_inverse_edit_ranges(&ops);
    assert_eq!(actual, expected);
}

#[test]
fn inverse_edits_single_insert() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(vec![edit_op(1, 1, 1, 1, Some(&["hello"]))], vec![inverse_edit_op(1, 1, 1, 6)]);
}

#[test]
fn inverse_edits_bug_19872_undo_is_funky() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(
        vec![edit_op(2, 1, 2, 2, Some(&[""])), edit_op(3, 1, 4, 2, Some(&[""]))],
        vec![inverse_edit_op(2, 1, 2, 1), inverse_edit_op(3, 1, 3, 1)],
    );
}

#[test]
fn inverse_edits_two_single_unrelated_inserts() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(
        vec![edit_op(1, 1, 1, 1, Some(&["hello"])), edit_op(2, 1, 2, 1, Some(&["world"]))],
        vec![inverse_edit_op(1, 1, 1, 6), inverse_edit_op(2, 1, 2, 6)],
    );
}

#[test]
fn inverse_edits_two_single_inserts_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(
        vec![edit_op(1, 1, 1, 1, Some(&["hello"])), edit_op(1, 2, 1, 2, Some(&["world"]))],
        vec![inverse_edit_op(1, 1, 1, 6), inverse_edit_op(1, 7, 1, 12)],
    );
}

#[test]
fn inverse_edits_two_single_inserts_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(
        vec![edit_op(1, 1, 1, 1, Some(&["hello"])), edit_op(1, 4, 1, 4, Some(&["world"]))],
        vec![inverse_edit_op(1, 1, 1, 6), inverse_edit_op(1, 9, 1, 14)],
    );
}

#[test]
fn inverse_edits_multiline_insert() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(vec![edit_op(1, 1, 1, 1, Some(&["hello", "world"]))], vec![inverse_edit_op(1, 1, 2, 6)]);
}

#[test]
fn inverse_edits_two_unrelated_multiline_inserts() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(
        vec![edit_op(1, 1, 1, 1, Some(&["hello", "world"])), edit_op(2, 1, 2, 1, Some(&["how", "are", "you?"]))],
        vec![inverse_edit_op(1, 1, 2, 6), inverse_edit_op(3, 1, 5, 5)],
    );
}

#[test]
fn inverse_edits_two_multiline_inserts_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(
        vec![edit_op(1, 1, 1, 1, Some(&["hello", "world"])), edit_op(1, 2, 1, 2, Some(&["how", "are", "you?"]))],
        vec![inverse_edit_op(1, 1, 2, 6), inverse_edit_op(2, 7, 4, 5)],
    );
}

#[test]
fn inverse_edits_single_delete() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(vec![edit_op(1, 1, 1, 6, None)], vec![inverse_edit_op(1, 1, 1, 1)]);
}

#[test]
fn inverse_edits_two_single_unrelated_deletes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(
        vec![edit_op(1, 1, 1, 6, None), edit_op(2, 1, 2, 6, None)],
        vec![inverse_edit_op(1, 1, 1, 1), inverse_edit_op(2, 1, 2, 1)],
    );
}

#[test]
fn inverse_edits_two_single_deletes_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(
        vec![edit_op(1, 1, 1, 6, None), edit_op(1, 7, 1, 12, None)],
        vec![inverse_edit_op(1, 1, 1, 1), inverse_edit_op(1, 2, 1, 2)],
    );
}

#[test]
fn inverse_edits_two_single_deletes_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(
        vec![edit_op(1, 1, 1, 6, None), edit_op(1, 9, 1, 14, None)],
        vec![inverse_edit_op(1, 1, 1, 1), inverse_edit_op(1, 4, 1, 4)],
    );
}

#[test]
fn inverse_edits_multiline_delete() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(vec![edit_op(1, 1, 2, 6, None)], vec![inverse_edit_op(1, 1, 1, 1)]);
}

#[test]
fn inverse_edits_two_unrelated_multiline_deletes() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(
        vec![edit_op(1, 1, 2, 6, None), edit_op(3, 1, 5, 5, None)],
        vec![inverse_edit_op(1, 1, 1, 1), inverse_edit_op(2, 1, 2, 1)],
    );
}

#[test]
fn inverse_edits_two_multiline_deletes_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(
        vec![edit_op(1, 1, 2, 6, None), edit_op(2, 7, 4, 5, None)],
        vec![inverse_edit_op(1, 1, 1, 1), inverse_edit_op(1, 2, 1, 2)],
    );
}

#[test]
fn inverse_edits_single_replace() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(vec![edit_op(1, 1, 1, 6, Some(&["Hello world"]))], vec![inverse_edit_op(1, 1, 1, 12)]);
}

#[test]
fn inverse_edits_two_replaces() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(
        vec![edit_op(1, 1, 1, 6, Some(&["Hello world"])), edit_op(1, 7, 1, 8, Some(&["How are you?"]))],
        vec![inverse_edit_op(1, 1, 1, 12), inverse_edit_op(1, 13, 1, 25)],
    );
}

#[test]
fn inverse_edits_many_edits() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert_inverse_edits(
        vec![edit_op(1, 2, 1, 2, Some(&["", "  "])), edit_op(1, 5, 1, 6, Some(&[""])), edit_op(1, 9, 1, 9, Some(&["", ""]))],
        vec![inverse_edit_op(1, 2, 2, 3), inverse_edit_op(2, 6, 2, 6), inverse_edit_op(2, 9, 3, 1)],
    );
}

// suite('PieceTreeTextBuffer._toSingleEditOperation')

fn test_to_single_edit_operation(original: &[&str], edits: Vec<ValidatedEditOperation>, expected: ValidatedEditOperation) {
    let buffer = create_text_buffer_factory(JsString::from(original.join("\n").as_str()).units()).create(DefaultEndOfLine::Lf);

    let actual = buffer.to_single_edit_operation(&edits);
    assert_eq!(actual, expected);
    buffer.disposable().dispose();
}

#[test]
fn to_single_edit_operation_one_edit_op_is_unchanged() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_to_single_edit_operation(
        &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
        vec![edit_op_full(1, 3, 1, 3, 2, 0, Some(&[" new line", "No longer"]))],
        edit_op_full(1, 3, 1, 3, 2, 0, Some(&[" new line", "No longer"])),
    );
}

#[test]
fn to_single_edit_operation_two_edits_on_one_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_to_single_edit_operation(
        &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
        vec![
            edit_op_full(1, 1, 1, 3, 0, 2, Some(&["Your"])),
            edit_op_full(1, 4, 1, 4, 3, 0, Some(&["Interesting "])),
            edit_op_full(2, 3, 2, 6, 16, 3, None),
        ],
        edit_op_full(1, 1, 2, 6, 0, 19, Some(&["Your Interesting First Line", "\t\t"])),
    );
}

#[test]
fn to_single_edit_operation_insert_multiple_newlines() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_to_single_edit_operation(
        &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
        vec![
            edit_op_full(1, 3, 1, 3, 2, 0, Some(&["", "", "", "", ""])),
            edit_op_full(3, 15, 3, 15, 45, 0, Some(&["a", "b"])),
        ],
        edit_op_full(
            1,
            3,
            3,
            15,
            2,
            43,
            Some(&["", "", "", "", " First Line", "\t\tMy Second Line", "    Third Linea", "b"]),
        ),
    );
}

#[test]
fn to_single_edit_operation_delete_empty_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_to_single_edit_operation(
        &["My First Line", "\t\tMy Second Line", "    Third Line", "", "1"],
        vec![edit_op_full(1, 1, 1, 1, 0, 0, Some(&[""]))],
        edit_op_full(1, 1, 1, 1, 0, 0, Some(&[""])),
    );
}

#[test]
fn to_single_edit_operation_two_unrelated_edits() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_to_single_edit_operation(
        &["My First Line", "\t\tMy Second Line", "    Third Line", "", "123"],
        vec![edit_op_full(2, 1, 2, 3, 14, 2, Some(&["\t"])), edit_op_full(3, 1, 3, 5, 31, 4, Some(&[""]))],
        edit_op_full(2, 1, 3, 5, 14, 21, Some(&["\tMy Second Line", ""])),
    );
}

#[test]
fn to_single_edit_operation_many_edits() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_to_single_edit_operation(
        &["{\"x\" : 1}"],
        vec![
            edit_op_full(1, 2, 1, 2, 1, 0, Some(&["\n  "])),
            edit_op_full(1, 5, 1, 6, 4, 1, Some(&[""])),
            edit_op_full(1, 9, 1, 9, 8, 0, Some(&["\n"])),
        ],
        edit_op_full(1, 2, 1, 9, 1, 7, Some(&["", "  \"x\": 1", ""])),
    );
}

#[test]
fn to_single_edit_operation_many_edits_reversed() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_to_single_edit_operation(
        &["{", "  \"x\": 1", "}"],
        vec![
            edit_op_full(1, 2, 2, 3, 1, 3, Some(&[""])),
            edit_op_full(2, 6, 2, 6, 7, 0, Some(&[" "])),
            edit_op_full(2, 9, 3, 1, 10, 1, Some(&[""])),
        ],
        edit_op_full(1, 2, 3, 1, 1, 10, Some(&["\"x\" : 1"])),
    );
}

#[test]
fn to_single_edit_operation_replacing_newlines_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_to_single_edit_operation(
        &["{", "\"a\": true,", "", "\"b\": true", "}"],
        vec![
            edit_op_full(1, 2, 2, 1, 1, 1, Some(&["", "\t"])),
            edit_op_full(2, 11, 4, 1, 12, 2, Some(&["", "\t"])),
        ],
        edit_op_full(1, 2, 4, 1, 1, 13, Some(&["", "\t\"a\": true,", "\t"])),
    );
}

#[test]
fn to_single_edit_operation_replacing_newlines_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_to_single_edit_operation(
        &["some text", "some more text", "now comes an empty line", "", "after empty line", "and the last line"],
        vec![
            edit_op_full(1, 5, 3, 1, 4, 21, Some(&[" text", "some more text", "some more text"])),
            edit_op_full(3, 2, 4, 1, 26, 23, Some(&["o more lines", "asd", "asd", "asd"])),
            edit_op_full(5, 1, 5, 6, 50, 5, Some(&["zzzzzzzz"])),
            edit_op_full(5, 11, 6, 16, 60, 22, Some(&["1", "2", "3", "4"])),
        ],
        edit_op_full(
            1,
            5,
            6,
            16,
            4,
            78,
            Some(&[
                " text",
                "some more text",
                "some more textno more lines",
                "asd",
                "asd",
                "asd",
                "zzzzzzzz empt1",
                "2",
                "3",
                "4",
            ]),
        ),
    );
}

#[test]
fn to_single_edit_operation_advanced() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_to_single_edit_operation(
        &[
            " {       \"d\": [",
            "             null",
            "        ] /*comment*/",
            "        ,\"e\": /*comment*/ [null] }",
        ],
        vec![
            edit_op_full(1, 1, 1, 2, 0, 1, Some(&[""])),
            edit_op_full(1, 3, 1, 10, 2, 7, Some(&["", "  "])),
            edit_op_full(1, 16, 2, 14, 15, 14, Some(&["", "    "])),
            edit_op_full(2, 18, 3, 9, 33, 9, Some(&["", "  "])),
            edit_op_full(3, 22, 4, 9, 55, 9, Some(&[""])),
            edit_op_full(4, 10, 4, 10, 65, 0, Some(&["", "  "])),
            edit_op_full(4, 28, 4, 28, 83, 0, Some(&["", "    "])),
            edit_op_full(4, 32, 4, 32, 87, 0, Some(&["", "  "])),
            edit_op_full(4, 33, 4, 34, 88, 1, Some(&["", ""])),
        ],
        edit_op_full(
            1,
            1,
            4,
            34,
            0,
            89,
            Some(&["{", "  \"d\": [", "    null", "  ] /*comment*/,", "  \"e\": /*comment*/ [", "    null", "  ]", ""]),
        ),
    );
}

#[test]
fn to_single_edit_operation_advanced_simplified() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_to_single_edit_operation(
        &["   abc", " ,def"],
        vec![
            edit_op_full(1, 1, 1, 4, 0, 3, Some(&[""])),
            edit_op_full(1, 7, 2, 2, 6, 2, Some(&[""])),
            edit_op_full(2, 3, 2, 3, 9, 0, Some(&["", ""])),
        ],
        edit_op_full(1, 1, 2, 3, 0, 9, Some(&["abc,", ""])),
    );
}
