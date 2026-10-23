//! Porte de `src/vs/editor/test/common/diff/diffComputer.test.ts` (suíte `Editor Diff - DiffComputer`),
//! caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso;
//! - `createTextModel(text)` é `create_text_model` de `tests/support/test_text_model.rs`;
//! - os parâmetros com valor padrão de `assertDiff` (`true, false, false`) viram `assert_diff_defaults`;
//! - `assert.deepStrictEqual(actual, expected)` compara direto as listas de `LineChange` (a projeção em
//!   objetos literais do original só descarta o que a estrutura já não tem);
//! - `Constants.MAX_SAFE_SMALL_INTEGER` é `je_base::uint::constants::MAX_SAFE_SMALL_INTEGER`;
//! - as 27 mudanças de caractere idênticas do caso `issue #23636` (uma por linha, coluna 1 a 2) saem de um laço.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::uint::constants::MAX_SAFE_SMALL_INTEGER;
use je_editor::diff::legacy_lines_diff_computer::{CharChange, DiffComputer, DiffComputerOpts, LineChange};
use je_editor::edit_operation::SingleEditOperation;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use support::test_text_model::create_text_model;

fn to_js_lines(lines: &[&str]) -> Vec<JsString> {
    lines.iter().map(|line| JsString::from(*line)).collect()
}

fn assert_diff(
    original_lines: &[&str],
    modified_lines: &[&str],
    expected_changes: Vec<LineChange>,
    should_compute_char_changes: bool,
    should_post_process_char_changes: bool,
    should_ignore_trim_whitespace: bool,
) {
    let diff_computer = DiffComputer::new(
        to_js_lines(original_lines),
        to_js_lines(modified_lines),
        DiffComputerOpts {
            should_compute_char_changes,
            should_post_process_char_changes,
            should_ignore_trim_whitespace,
            should_make_pretty_diff: true,
            max_computation_time: 0.0,
        },
    );
    let changes = diff_computer.compute_diff().changes;

    assert_eq!(changes, expected_changes);

    if !should_ignore_trim_whitespace {
        // The diffs should describe how to apply edits to the original text model to get to the modified text model.

        let modified_text_model = create_text_model(&modified_lines.join("\n"));
        let expected_value = modified_text_model.get_value();

        {
            // Line changes:
            let original_text_model = create_text_model(&original_lines.join("\n"));
            let edits: Vec<SingleEditOperation> = changes.iter().map(|c| get_line_edit(c, &modified_text_model)).collect();
            original_text_model.apply_edits(&edits);
            assert_eq!(original_text_model.get_value(), expected_value);
            original_text_model.dispose();
        }

        if should_compute_char_changes {
            // Char changes:
            let original_text_model = create_text_model(&original_lines.join("\n"));
            let edits: Vec<SingleEditOperation> = changes.iter().flat_map(|c| get_char_edits(c, &modified_text_model)).collect();
            original_text_model.apply_edits(&edits);
            assert_eq!(original_text_model.get_value(), expected_value);
            original_text_model.dispose();
        }

        modified_text_model.dispose();
    }
}

fn assert_diff_defaults(original_lines: &[&str], modified_lines: &[&str], expected_changes: Vec<LineChange>) {
    assert_diff(original_lines, modified_lines, expected_changes, true, false, false);
}

fn get_char_edits(line_change: &LineChange, modified_text_model: &TextModel) -> Vec<SingleEditOperation> {
    let Some(char_changes) = &line_change.char_changes else {
        return vec![get_line_edit(line_change, modified_text_model)];
    };
    char_changes
        .iter()
        .map(|c| SingleEditOperation::new(c.original_range(), Some(modified_text_model.get_value_in_range(&c.modified_range())), false))
        .collect()
}

fn get_line_edit(line_change: &LineChange, modified_text_model: &TextModel) -> SingleEditOperation {
    let original_range = if line_change.original_end_line_number == 0 {
        // Insertion
        LineRange::new(line_change.original_start_line_number + 1, 0)
    } else {
        LineRange::new(line_change.original_start_line_number, line_change.original_end_line_number - line_change.original_start_line_number + 1)
    };

    let modified_range = if line_change.modified_end_line_number == 0 {
        // Deletion
        LineRange::new(line_change.modified_start_line_number + 1, 0)
    } else {
        LineRange::new(line_change.modified_start_line_number, line_change.modified_end_line_number - line_change.modified_start_line_number + 1)
    };

    let (r1, r2) = diff_from_line_ranges(original_range, modified_range);
    SingleEditOperation::new(r1, Some(modified_text_model.get_value_in_range(&r2)), false)
}

fn diff_from_line_ranges(original_range: LineRange, modified_range: LineRange) -> (Range, Range) {
    if original_range.start_line_number == 1 || modified_range.start_line_number == 1 {
        if !original_range.is_empty() && !modified_range.is_empty() {
            return (
                Range::new(original_range.start_line_number, 1, original_range.end_line_number_exclusive() - 1, MAX_SAFE_SMALL_INTEGER),
                Range::new(modified_range.start_line_number, 1, modified_range.end_line_number_exclusive() - 1, MAX_SAFE_SMALL_INTEGER),
            );
        }

        // When one of them is one and one of them is empty, the other cannot be the last line of the document
        return (
            Range::new(original_range.start_line_number, 1, original_range.end_line_number_exclusive(), 1),
            Range::new(modified_range.start_line_number, 1, modified_range.end_line_number_exclusive(), 1),
        );
    }

    (
        Range::new(
            original_range.start_line_number - 1,
            MAX_SAFE_SMALL_INTEGER,
            original_range.end_line_number_exclusive() - 1,
            MAX_SAFE_SMALL_INTEGER,
        ),
        Range::new(
            modified_range.start_line_number - 1,
            MAX_SAFE_SMALL_INTEGER,
            modified_range.end_line_number_exclusive() - 1,
            MAX_SAFE_SMALL_INTEGER,
        ),
    )
}

/// O `LineRange` local do teste (início e quantidade de linhas), não o do editor.
#[derive(Clone, Copy)]
struct LineRange {
    start_line_number: i32,
    line_count: i32,
}

impl LineRange {
    fn new(start_line_number: i32, line_count: i32) -> Self {
        Self { start_line_number, line_count }
    }

    fn is_empty(&self) -> bool {
        self.line_count == 0
    }

    fn end_line_number_exclusive(&self) -> i32 {
        self.start_line_number + self.line_count
    }
}

fn create_line_deletion(start_line_number: i32, end_line_number: i32, modified_line_number: i32) -> LineChange {
    LineChange {
        original_start_line_number: start_line_number,
        original_end_line_number: end_line_number,
        modified_start_line_number: modified_line_number,
        modified_end_line_number: 0,
        char_changes: None,
    }
}

fn create_line_insertion(start_line_number: i32, end_line_number: i32, original_line_number: i32) -> LineChange {
    LineChange {
        original_start_line_number: original_line_number,
        original_end_line_number: 0,
        modified_start_line_number: start_line_number,
        modified_end_line_number: end_line_number,
        char_changes: None,
    }
}

fn create_line_change(
    original_start_line_number: i32,
    original_end_line_number: i32,
    modified_start_line_number: i32,
    modified_end_line_number: i32,
    char_changes: Option<Vec<CharChange>>,
) -> LineChange {
    LineChange { original_start_line_number, original_end_line_number, modified_start_line_number, modified_end_line_number, char_changes }
}

#[allow(clippy::too_many_arguments)]
fn create_char_change(
    original_start_line_number: i32,
    original_start_column: i32,
    original_end_line_number: i32,
    original_end_column: i32,
    modified_start_line_number: i32,
    modified_start_column: i32,
    modified_end_line_number: i32,
    modified_end_column: i32,
) -> CharChange {
    CharChange {
        original_start_line_number,
        original_start_column,
        original_end_line_number,
        original_end_column,
        modified_start_line_number,
        modified_start_column,
        modified_end_line_number,
        modified_end_column,
    }
}

// ---- insertions

#[test]
fn one_inserted_line_below() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line"];
    let modified = ["line", "new line"];
    let expected = vec![create_line_insertion(2, 2, 1)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn two_inserted_lines_below() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line"];
    let modified = ["line", "new line", "another new line"];
    let expected = vec![create_line_insertion(2, 3, 1)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn one_inserted_line_above() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line"];
    let modified = ["new line", "line"];
    let expected = vec![create_line_insertion(1, 1, 0)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn two_inserted_lines_above() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line"];
    let modified = ["new line", "another new line", "line"];
    let expected = vec![create_line_insertion(1, 2, 0)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn one_inserted_line_in_middle() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line1", "line2", "line3", "line4"];
    let modified = ["line1", "line2", "new line", "line3", "line4"];
    let expected = vec![create_line_insertion(3, 3, 2)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn two_inserted_lines_in_middle() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line1", "line2", "line3", "line4"];
    let modified = ["line1", "line2", "new line", "another new line", "line3", "line4"];
    let expected = vec![create_line_insertion(3, 4, 2)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn two_inserted_lines_in_middle_interrupted() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line1", "line2", "line3", "line4"];
    let modified = ["line1", "line2", "new line", "line3", "another new line", "line4"];
    let expected = vec![create_line_insertion(3, 3, 2), create_line_insertion(5, 5, 3)];
    assert_diff_defaults(&original, &modified, expected);
}

// ---- deletions

#[test]
fn one_deleted_line_below() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line", "new line"];
    let modified = ["line"];
    let expected = vec![create_line_deletion(2, 2, 1)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn two_deleted_lines_below() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line", "new line", "another new line"];
    let modified = ["line"];
    let expected = vec![create_line_deletion(2, 3, 1)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn one_deleted_lines_above() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["new line", "line"];
    let modified = ["line"];
    let expected = vec![create_line_deletion(1, 1, 0)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn two_deleted_lines_above() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["new line", "another new line", "line"];
    let modified = ["line"];
    let expected = vec![create_line_deletion(1, 2, 0)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn one_deleted_line_in_middle() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line1", "line2", "new line", "line3", "line4"];
    let modified = ["line1", "line2", "line3", "line4"];
    let expected = vec![create_line_deletion(3, 3, 2)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn two_deleted_lines_in_middle() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line1", "line2", "new line", "another new line", "line3", "line4"];
    let modified = ["line1", "line2", "line3", "line4"];
    let expected = vec![create_line_deletion(3, 4, 2)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn two_deleted_lines_in_middle_interrupted() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line1", "line2", "new line", "line3", "another new line", "line4"];
    let modified = ["line1", "line2", "line3", "line4"];
    let expected = vec![create_line_deletion(3, 3, 2), create_line_deletion(5, 5, 3)];
    assert_diff_defaults(&original, &modified, expected);
}

// ---- changes

#[test]
fn one_line_changed_chars_inserted_at_the_end() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line"];
    let modified = ["line changed"];
    let expected = vec![create_line_change(1, 1, 1, 1, Some(vec![create_char_change(1, 5, 1, 5, 1, 5, 1, 13)]))];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn one_line_changed_chars_inserted_at_the_beginning() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["line"];
    let modified = ["my line"];
    let expected = vec![create_line_change(1, 1, 1, 1, Some(vec![create_char_change(1, 1, 1, 1, 1, 1, 1, 4)]))];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn one_line_changed_chars_inserted_in_the_middle() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["abba"];
    let modified = ["abzzba"];
    let expected = vec![create_line_change(1, 1, 1, 1, Some(vec![create_char_change(1, 3, 1, 3, 1, 3, 1, 5)]))];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn one_line_changed_chars_inserted_in_the_middle_two_spots() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["abba"];
    let modified = ["abzzbzza"];
    let expected = vec![create_line_change(
        1,
        1,
        1,
        1,
        Some(vec![create_char_change(1, 3, 1, 3, 1, 3, 1, 5), create_char_change(1, 4, 1, 4, 1, 6, 1, 8)]),
    )];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn one_line_changed_chars_deleted_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["abcdefg"];
    let modified = ["abcfg"];
    let expected = vec![create_line_change(1, 1, 1, 1, Some(vec![create_char_change(1, 4, 1, 6, 1, 4, 1, 4)]))];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn one_line_changed_chars_deleted_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["abcdefg"];
    let modified = ["acfg"];
    let expected = vec![create_line_change(
        1,
        1,
        1,
        1,
        Some(vec![create_char_change(1, 2, 1, 3, 1, 2, 1, 2), create_char_change(1, 4, 1, 6, 1, 3, 1, 3)]),
    )];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn two_lines_changed_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["abcd", "efgh"];
    let modified = ["abcz"];
    let expected = vec![create_line_change(1, 2, 1, 1, Some(vec![create_char_change(1, 4, 2, 5, 1, 4, 1, 5)]))];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn two_lines_changed_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["foo", "abcd", "efgh", "BAR"];
    let modified = ["foo", "abcz", "BAR"];
    let expected = vec![create_line_change(2, 3, 2, 2, Some(vec![create_char_change(2, 4, 3, 5, 2, 4, 2, 5)]))];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn two_lines_changed_3() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["foo", "abcd", "efgh", "BAR"];
    let modified = ["foo", "abcz", "zzzzefgh", "BAR"];
    let expected = vec![create_line_change(
        2,
        3,
        2,
        3,
        Some(vec![create_char_change(2, 4, 2, 5, 2, 4, 2, 5), create_char_change(3, 1, 3, 1, 3, 1, 3, 5)]),
    )];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn two_lines_changed_4() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["abc"];
    let modified = ["", "", "axc", ""];
    let expected = vec![create_line_change(
        1,
        1,
        1,
        4,
        Some(vec![
            create_char_change(1, 1, 1, 1, 1, 1, 3, 1),
            create_char_change(1, 2, 1, 3, 3, 2, 3, 3),
            create_char_change(1, 4, 1, 4, 3, 4, 4, 1),
        ]),
    )];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn empty_original_sequence_in_char_diff() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["abc", "", "xyz"];
    let modified = ["abc", "qwe", "rty", "xyz"];
    let expected = vec![create_line_change(2, 2, 2, 3, None)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn three_lines_changed() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["foo", "abcd", "efgh", "BAR"];
    let modified = ["foo", "zzzefgh", "xxx", "BAR"];
    let expected = vec![create_line_change(
        2,
        3,
        2,
        3,
        Some(vec![create_char_change(2, 1, 3, 1, 2, 1, 2, 4), create_char_change(3, 5, 3, 5, 2, 8, 3, 4)]),
    )];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn big_change_part_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["foo", "abcd", "efgh", "BAR"];
    let modified = ["hello", "foo", "zzzefgh", "xxx", "BAR"];
    let expected = vec![
        create_line_insertion(1, 1, 0),
        create_line_change(2, 3, 3, 4, Some(vec![create_char_change(2, 1, 3, 1, 3, 1, 3, 4), create_char_change(3, 5, 3, 5, 3, 8, 4, 4)])),
    ];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn big_change_part_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["foo", "abcd", "efgh", "BAR", "RAB"];
    let modified = ["hello", "foo", "zzzefgh", "xxx", "BAR"];
    let expected = vec![
        create_line_insertion(1, 1, 0),
        create_line_change(2, 3, 3, 4, Some(vec![create_char_change(2, 1, 3, 1, 3, 1, 3, 4), create_char_change(3, 5, 3, 5, 3, 8, 4, 4)])),
        create_line_deletion(5, 5, 5),
    ];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn char_change_postprocessing_merges() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["abba"];
    let modified = ["azzzbzzzbzzza"];
    let expected = vec![create_line_change(1, 1, 1, 1, Some(vec![create_char_change(1, 2, 1, 4, 1, 2, 1, 13)]))];
    assert_diff(&original, &modified, expected, true, true, false);
}

#[test]
fn ignore_trim_whitespace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["\t\t foo ", "abcd", "efgh", "\t\t BAR\t\t"];
    let modified = ["  hello\t", "\t foo   \t", "zzzefgh", "xxx", "   BAR   \t"];
    let expected = vec![
        create_line_insertion(1, 1, 0),
        create_line_change(2, 3, 3, 4, Some(vec![create_char_change(2, 1, 2, 5, 3, 1, 3, 4), create_char_change(3, 5, 3, 5, 4, 1, 4, 4)])),
    ];
    assert_diff(&original, &modified, expected, true, false, true);
}

#[test]
fn issue_12122_r_has_own_property_is_not_a_function() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["hasOwnProperty"];
    let modified = ["hasOwnProperty", "and another line"];
    let expected = vec![create_line_insertion(2, 2, 1)];
    assert_diff_defaults(&original, &modified, expected);
}

#[test]
fn empty_diff_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = [""];
    let modified = ["something"];
    let expected = vec![create_line_change(1, 1, 1, 1, None)];
    assert_diff(&original, &modified, expected, true, false, true);
}

#[test]
fn empty_diff_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = [""];
    let modified = ["something", "something else"];
    let expected = vec![create_line_change(1, 1, 1, 2, None)];
    assert_diff(&original, &modified, expected, true, false, true);
}

#[test]
fn empty_diff_3() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["something", "something else"];
    let modified = [""];
    let expected = vec![create_line_change(1, 2, 1, 1, None)];
    assert_diff(&original, &modified, expected, true, false, true);
}

#[test]
fn empty_diff_4() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["something"];
    let modified = [""];
    let expected = vec![create_line_change(1, 1, 1, 1, None)];
    assert_diff(&original, &modified, expected, true, false, true);
}

#[test]
fn empty_diff_5() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = [""];
    let modified = [""];
    let expected: Vec<LineChange> = Vec::new();
    assert_diff(&original, &modified, expected, true, false, true);
}

#[test]
fn pretty_diff_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = [
        "suite(function () {",
        "\ttest1() {",
        "\t\tassert.ok(true);",
        "\t}",
        "",
        "\ttest2() {",
        "\t\tassert.ok(true);",
        "\t}",
        "});",
        "",
    ];
    let modified = [
        "// An insertion",
        "suite(function () {",
        "\ttest1() {",
        "\t\tassert.ok(true);",
        "\t}",
        "",
        "\ttest2() {",
        "\t\tassert.ok(true);",
        "\t}",
        "",
        "\ttest3() {",
        "\t\tassert.ok(true);",
        "\t}",
        "});",
        "",
    ];
    let expected = vec![create_line_insertion(1, 1, 0), create_line_insertion(10, 13, 8)];
    assert_diff(&original, &modified, expected, true, false, true);
}

#[test]
fn pretty_diff_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = [
        "// Just a comment",
        "",
        "function compute(a, b, c, d) {",
        "\tif (a) {",
        "\t\tif (b) {",
        "\t\t\tif (c) {",
        "\t\t\t\treturn 5;",
        "\t\t\t}",
        "\t\t}",
        "\t\t// These next lines will be deleted",
        "\t\tif (d) {",
        "\t\t\treturn -1;",
        "\t\t}",
        "\t\treturn 0;",
        "\t}",
        "}",
    ];
    let modified = [
        "// Here is an inserted line",
        "// and another inserted line",
        "// and another one",
        "// Just a comment",
        "",
        "function compute(a, b, c, d) {",
        "\tif (a) {",
        "\t\tif (b) {",
        "\t\t\tif (c) {",
        "\t\t\t\treturn 5;",
        "\t\t\t}",
        "\t\t}",
        "\t\treturn 0;",
        "\t}",
        "}",
    ];
    let expected = vec![create_line_insertion(1, 3, 0), create_line_deletion(10, 13, 12)];
    assert_diff(&original, &modified, expected, true, false, true);
}

#[test]
fn pretty_diff_3() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = [
        "class A {",
        "\t/**",
        "\t * m1",
        "\t */",
        "\tmethod1() {}",
        "",
        "\t/**",
        "\t * m3",
        "\t */",
        "\tmethod3() {}",
        "}",
    ];
    let modified = [
        "class A {",
        "\t/**",
        "\t * m1",
        "\t */",
        "\tmethod1() {}",
        "",
        "\t/**",
        "\t * m2",
        "\t */",
        "\tmethod2() {}",
        "",
        "\t/**",
        "\t * m3",
        "\t */",
        "\tmethod3() {}",
        "}",
    ];
    let expected = vec![create_line_insertion(7, 11, 6)];
    assert_diff(&original, &modified, expected, true, false, true);
}

#[test]
fn issue_23636() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = [
        "if(!TextDrawLoad[playerid])",
        "{",
        "",
        "\tTextDrawHideForPlayer(playerid,TD_AppleJob[3]);",
        "\tTextDrawHideForPlayer(playerid,TD_AppleJob[4]);",
        "\tif(!AppleJobTreesType[AppleJobTreesPlayerNum[playerid]])",
        "\t{",
        "\t\tfor(new i=0;i<10;i++) if(StatusTD_AppleJobApples[playerid][i]) TextDrawHideForPlayer(playerid,TD_AppleJob[5+i]);",
        "\t}",
        "\telse",
        "\t{",
        "\t\tfor(new i=0;i<10;i++) if(StatusTD_AppleJobApples[playerid][i]) TextDrawHideForPlayer(playerid,TD_AppleJob[15+i]);",
        "\t}",
        "}",
        "else",
        "{",
        "\tTextDrawHideForPlayer(playerid,TD_AppleJob[3]);",
        "\tTextDrawHideForPlayer(playerid,TD_AppleJob[27]);",
        "\tif(!AppleJobTreesType[AppleJobTreesPlayerNum[playerid]])",
        "\t{",
        "\t\tfor(new i=0;i<10;i++) if(StatusTD_AppleJobApples[playerid][i]) TextDrawHideForPlayer(playerid,TD_AppleJob[28+i]);",
        "\t}",
        "\telse",
        "\t{",
        "\t\tfor(new i=0;i<10;i++) if(StatusTD_AppleJobApples[playerid][i]) TextDrawHideForPlayer(playerid,TD_AppleJob[38+i]);",
        "\t}",
        "}",
    ];
    let modified = [
        "\tif(!TextDrawLoad[playerid])",
        "\t{",
        "\t",
        "\t\tTextDrawHideForPlayer(playerid,TD_AppleJob[3]);",
        "\t\tTextDrawHideForPlayer(playerid,TD_AppleJob[4]);",
        "\t\tif(!AppleJobTreesType[AppleJobTreesPlayerNum[playerid]])",
        "\t\t{",
        "\t\t\tfor(new i=0;i<10;i++) if(StatusTD_AppleJobApples[playerid][i]) TextDrawHideForPlayer(playerid,TD_AppleJob[5+i]);",
        "\t\t}",
        "\t\telse",
        "\t\t{",
        "\t\t\tfor(new i=0;i<10;i++) if(StatusTD_AppleJobApples[playerid][i]) TextDrawHideForPlayer(playerid,TD_AppleJob[15+i]);",
        "\t\t}",
        "\t}",
        "\telse",
        "\t{",
        "\t\tTextDrawHideForPlayer(playerid,TD_AppleJob[3]);",
        "\t\tTextDrawHideForPlayer(playerid,TD_AppleJob[27]);",
        "\t\tif(!AppleJobTreesType[AppleJobTreesPlayerNum[playerid]])",
        "\t\t{",
        "\t\t\tfor(new i=0;i<10;i++) if(StatusTD_AppleJobApples[playerid][i]) TextDrawHideForPlayer(playerid,TD_AppleJob[28+i]);",
        "\t\t}",
        "\t\telse",
        "\t\t{",
        "\t\t\tfor(new i=0;i<10;i++) if(StatusTD_AppleJobApples[playerid][i]) TextDrawHideForPlayer(playerid,TD_AppleJob[38+i]);",
        "\t\t}",
        "\t}",
    ];
    let char_changes: Vec<CharChange> = (1..=27).map(|line| create_char_change(line, 1, line, 1, line, 1, line, 2)).collect();
    let expected = vec![create_line_change(1, 27, 1, 27, Some(char_changes))];
    assert_diff(&original, &modified, expected, true, true, false);
}

#[test]
fn issue_43922() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = [
        " * `yarn [install]` -- Install project NPM dependencies. This is automatically done when you first create the project. You should only need to run this if you add dependencies in `package.json`.",
    ];
    let modified = [" * `yarn` -- Install project NPM dependencies. You should only need to run this if you add dependencies in `package.json`."];
    let expected = vec![create_line_change(
        1,
        1,
        1,
        1,
        Some(vec![create_char_change(1, 9, 1, 19, 1, 9, 1, 9), create_char_change(1, 58, 1, 120, 1, 48, 1, 48)]),
    )];
    assert_diff(&original, &modified, expected, true, true, false);
}

#[test]
fn issue_42751() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["    1", "  2"];
    let modified = ["    1", "   3"];
    let expected = vec![create_line_change(2, 2, 2, 2, Some(vec![create_char_change(2, 3, 2, 4, 2, 3, 2, 5)]))];
    assert_diff(&original, &modified, expected, true, true, false);
}

#[test]
fn does_not_give_character_changes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["    1", "  2", "A"];
    let modified = ["    1", "   3", " A"];
    let expected = vec![create_line_change(2, 3, 2, 3, None)];
    assert_diff(&original, &modified, expected, false, false, false);
}

#[test]
fn issue_44422_less_than_ideal_diff_results() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = [
        "export class C {",
        "",
        "\tpublic m1(): void {",
        "\t\t{",
        "\t\t//2",
        "\t\t//3",
        "\t\t//4",
        "\t\t//5",
        "\t\t//6",
        "\t\t//7",
        "\t\t//8",
        "\t\t//9",
        "\t\t//10",
        "\t\t//11",
        "\t\t//12",
        "\t\t//13",
        "\t\t//14",
        "\t\t//15",
        "\t\t//16",
        "\t\t//17",
        "\t\t//18",
        "\t\t}",
        "\t}",
        "",
        "\tpublic m2(): void {",
        "\t\tif (a) {",
        "\t\t\tif (b) {",
        "\t\t\t\t//A1",
        "\t\t\t\t//A2",
        "\t\t\t\t//A3",
        "\t\t\t\t//A4",
        "\t\t\t\t//A5",
        "\t\t\t\t//A6",
        "\t\t\t\t//A7",
        "\t\t\t\t//A8",
        "\t\t\t}",
        "\t\t}",
        "",
        "\t\t//A9",
        "\t\t//A10",
        "\t\t//A11",
        "\t\t//A12",
        "\t\t//A13",
        "\t\t//A14",
        "\t\t//A15",
        "\t}",
        "",
        "\tpublic m3(): void {",
        "\t\tif (a) {",
        "\t\t\t//B1",
        "\t\t}",
        "\t\t//B2",
        "\t\t//B3",
        "\t}",
        "",
        "\tpublic m4(): boolean {",
        "\t\t//1",
        "\t\t//2",
        "\t\t//3",
        "\t\t//4",
        "\t}",
        "",
        "}",
    ];
    let modified = [
        "export class C {",
        "",
        "\tconstructor() {",
        "",
        "",
        "",
        "",
        "\t}",
        "",
        "\tpublic m1(): void {",
        "\t\t{",
        "\t\t//2",
        "\t\t//3",
        "\t\t//4",
        "\t\t//5",
        "\t\t//6",
        "\t\t//7",
        "\t\t//8",
        "\t\t//9",
        "\t\t//10",
        "\t\t//11",
        "\t\t//12",
        "\t\t//13",
        "\t\t//14",
        "\t\t//15",
        "\t\t//16",
        "\t\t//17",
        "\t\t//18",
        "\t\t}",
        "\t}",
        "",
        "\tpublic m4(): boolean {",
        "\t\t//1",
        "\t\t//2",
        "\t\t//3",
        "\t\t//4",
        "\t}",
        "",
        "}",
    ];
    let expected = vec![create_line_change(2, 0, 3, 9, None), create_line_change(25, 55, 31, 0, None)];
    assert_diff(&original, &modified, expected, false, false, false);
}

#[test]
fn gives_preference_to_matching_longer_lines() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["A", "A", "BB", "C"];
    let modified = ["A", "BB", "A", "D", "E", "A", "C"];
    let expected = vec![create_line_change(2, 2, 1, 0, None), create_line_change(3, 0, 3, 6, None)];
    assert_diff(&original, &modified, expected, false, false, false);
}

#[test]
fn issue_119051_gives_preference_to_fewer_diff_hunks() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["1", "", "", "2", ""];
    let modified = ["1", "", "1.5", "", "", "2", "", "3", ""];
    let expected = vec![create_line_change(2, 0, 3, 4, None), create_line_change(5, 0, 8, 9, None)];
    assert_diff(&original, &modified, expected, false, false, false);
}

#[test]
fn issue_121436_diff_chunk_contains_an_unchanged_line_part_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["if (cond) {", "    cmd", "}"];
    let modified = ["if (cond) {", "    if (other_cond) {", "        cmd", "    }", "}"];
    let expected = vec![create_line_change(1, 0, 2, 2, None), create_line_change(2, 0, 4, 4, None)];
    assert_diff(&original, &modified, expected, false, false, true);
}

#[test]
fn issue_121436_diff_chunk_contains_an_unchanged_line_part_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["if (cond) {", "    cmd", "}"];
    let modified = ["if (cond) {", "    if (other_cond) {", "        cmd", "    }", "}"];
    let expected = vec![create_line_change(1, 0, 2, 2, None), create_line_change(2, 2, 3, 3, None), create_line_change(2, 0, 4, 4, None)];
    assert_diff(&original, &modified, expected, false, false, false);
}

#[test]
fn issue_169552_assertion_error_when_having_both_leading_and_trailing_whitespace_diffs() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let original = ["if True:", "    print(2)"];
    let modified = ["if True:", "\tprint(2) "];
    let expected = vec![create_line_change(
        2,
        2,
        2,
        2,
        Some(vec![create_char_change(2, 1, 2, 5, 2, 1, 2, 2), create_char_change(2, 13, 2, 13, 2, 10, 2, 11)]),
    )];
    assert_diff(&original, &modified, expected, true, false, false);
}
