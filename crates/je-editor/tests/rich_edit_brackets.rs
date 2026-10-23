//! Porte de `src/vs/editor/test/common/modes/supports/richEditBrackets.test.ts` (suíte `richEditBrackets`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite` vem do `common` do `je-base`, incluído por `#[path]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::js_regexp::JsRegExp;
use je_editor::range::Range;
use je_editor::rich_edit_brackets::BracketsUtils;
use support::u;

fn regex(pattern: &str) -> JsRegExp {
    JsRegExp::from_str(pattern, "i").expect("regex válida")
}

fn find_prev_bracket_in_range(
    reversed_bracket_regex: &JsRegExp,
    line_text: &str,
    current_token_start: i32,
    current_token_end: i32,
) -> Option<Range> {
    BracketsUtils::find_prev_bracket_in_range(
        reversed_bracket_regex,
        1,
        &u(line_text),
        current_token_start,
        current_token_end,
    )
}

fn find_next_bracket_in_range(
    forward_bracket_regex: &JsRegExp,
    line_text: &str,
    current_token_start: i32,
    current_token_end: i32,
) -> Option<Range> {
    BracketsUtils::find_next_bracket_in_range(
        forward_bracket_regex,
        1,
        &u(line_text),
        current_token_start,
        current_token_end,
    )
}

#[test]
fn find_prev_bracket_in_token_one_char_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = find_prev_bracket_in_range(&regex(r"(\{)|(\})"), "{", 0, 1).unwrap();
    assert_eq!(result.start_column, 1);
    assert_eq!(result.end_column, 2);
}

#[test]
fn find_prev_bracket_in_token_one_char_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = find_prev_bracket_in_range(&regex(r"(\{)|(\})"), "{{", 0, 1).unwrap();
    assert_eq!(result.start_column, 1);
    assert_eq!(result.end_column, 2);
}

#[test]
fn find_prev_bracket_in_token_one_char_3() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = find_prev_bracket_in_range(&regex(r"(\{)|(\})"), "{hello world!", 0, 13).unwrap();
    assert_eq!(result.start_column, 1);
    assert_eq!(result.end_column, 2);
}

#[test]
fn find_prev_bracket_in_token_more_chars_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = find_prev_bracket_in_range(&regex(r"(olleh)"), "hello world!", 0, 12).unwrap();
    assert_eq!(result.start_column, 1);
    assert_eq!(result.end_column, 6);
}

#[test]
fn find_prev_bracket_in_token_more_chars_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = find_prev_bracket_in_range(&regex(r"(olleh)"), "hello world!", 0, 5).unwrap();
    assert_eq!(result.start_column, 1);
    assert_eq!(result.end_column, 6);
}

#[test]
fn find_prev_bracket_in_token_more_chars_3() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = find_prev_bracket_in_range(&regex(r"(olleh)"), " hello world!", 0, 6).unwrap();
    assert_eq!(result.start_column, 2);
    assert_eq!(result.end_column, 7);
}

#[test]
fn find_next_bracket_in_token_one_char() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = find_next_bracket_in_range(&regex(r"(\{)|(\})"), "{", 0, 1).unwrap();
    assert_eq!(result.start_column, 1);
    assert_eq!(result.end_column, 2);
}

#[test]
fn find_next_bracket_in_token_more_chars() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = find_next_bracket_in_range(&regex(r"(world)"), "hello world!", 0, 12).unwrap();
    assert_eq!(result.start_column, 7);
    assert_eq!(result.end_column, 12);
}

#[test]
fn find_next_bracket_in_token_with_emoty_result() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = find_next_bracket_in_range(&regex(r"(\{)|(\})"), "", 0, 0);
    assert_eq!(result, None);
}

#[test]
fn issue_3894_handlebars_curly_braces_edit_issues() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = find_prev_bracket_in_range(&regex(r"(\-\-!<)|(>\-\-)|(\{\{)|(\}\})"), "{{asd}}", 0, 2).unwrap();
    assert_eq!(result.start_column, 1);
    assert_eq!(result.end_column, 3);
}
