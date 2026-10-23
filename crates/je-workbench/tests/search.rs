//! Porte de `src/vs/workbench/services/search/test/common/search.test.ts` (suítes `TextSearchResult` e
//! `isFilePatternMatch`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `deepStrictEqual` vira `assert_eq!` sobre tipos que derivam `PartialEq`.
//! - O caso comentado do original (`all lines of multiline match`) continua de fora.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::js_string::{JsString, substring};
use je_workbench::services::search::search::{
    IRawFileMatch, ITextSearchPreviewOptions, OneLineRange, SearchRange, TextSearchMatch, is_file_pattern_match,
};

const PREVIEW_OPTIONS_1: ITextSearchPreviewOptions = ITextSearchPreviewOptions { match_lines: 1, chars_per_line: 100 };

fn assert_one_line_preview_range_text(text: &str, result: &TextSearchMatch) {
    assert_eq!(result.range_locations.len(), 1);
    let preview = result.range_locations[0].preview;
    assert_eq!(
        JsString::from(substring(result.preview_text.units(), preview.start_column, preview.end_column)),
        text
    );
}

fn get_first_source_from_result(result: &TextSearchMatch) -> SearchRange {
    result.range_locations.iter().map(|e| e.source).next().unwrap()
}

#[test]
fn empty_without_preview_options() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let range = OneLineRange::new(5, 0, 0);
    let result = TextSearchMatch::new("", &[range], None, None);
    assert_eq!(get_first_source_from_result(&result), range);
    assert_one_line_preview_range_text("", &result);
}

#[test]
fn empty_with_preview_options() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let range = OneLineRange::new(5, 0, 0);
    let result = TextSearchMatch::new("", &[range], Some(&PREVIEW_OPTIONS_1), None);
    assert_eq!(get_first_source_from_result(&result), range);
    assert_one_line_preview_range_text("", &result);
}

#[test]
fn short_without_preview_options() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let range = OneLineRange::new(5, 4, 7);
    let result = TextSearchMatch::new("foo bar", &[range], None, None);
    assert_eq!(get_first_source_from_result(&result), range);
    assert_one_line_preview_range_text("bar", &result);
}

#[test]
fn short_with_preview_options() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let range = OneLineRange::new(5, 4, 7);
    let result = TextSearchMatch::new("foo bar", &[range], Some(&PREVIEW_OPTIONS_1), None);
    assert_eq!(get_first_source_from_result(&result), range);
    assert_one_line_preview_range_text("bar", &result);
}

#[test]
fn leading() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let range = OneLineRange::new(5, 25, 28);
    let result = TextSearchMatch::new("long text very long text foo", &[range], Some(&PREVIEW_OPTIONS_1), None);
    assert_eq!(get_first_source_from_result(&result), range);
    assert_one_line_preview_range_text("foo", &result);
}

#[test]
fn trailing() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let range = OneLineRange::new(5, 0, 3);
    let result = TextSearchMatch::new(
        "foo long text very long text long text very long text long text very long text long text very long text long text very long text",
        &[range],
        Some(&PREVIEW_OPTIONS_1),
        None,
    );
    assert_eq!(get_first_source_from_result(&result), range);
    assert_one_line_preview_range_text("foo", &result);
}

#[test]
fn middle() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let range = OneLineRange::new(5, 30, 33);
    let result = TextSearchMatch::new(
        "long text very long text long foo text very long text long text very long text long text very long text long text very long text",
        &[range],
        Some(&PREVIEW_OPTIONS_1),
        None,
    );
    assert_eq!(get_first_source_from_result(&result), range);
    assert_one_line_preview_range_text("foo", &result);
}

#[test]
fn truncating_match() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let preview_options = ITextSearchPreviewOptions { match_lines: 1, chars_per_line: 1 };

    let range = OneLineRange::new(0, 4, 7);
    let result = TextSearchMatch::new("foo bar", &[range], Some(&preview_options), None);
    assert_eq!(get_first_source_from_result(&result), range);
    assert_one_line_preview_range_text("b", &result);
}

#[test]
fn one_line_of_multiline_match() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let preview_options = ITextSearchPreviewOptions { match_lines: 1, chars_per_line: 10000 };

    let range = SearchRange::new(5, 4, 6, 3);
    let result = TextSearchMatch::new("foo bar\nfoo bar", &[range], Some(&preview_options), None);
    assert_eq!(get_first_source_from_result(&result), range);
    assert_eq!(result.preview_text, "foo bar\nfoo bar");
    assert_eq!(result.range_locations.len(), 1);
    assert_eq!(result.range_locations[0].preview.start_line_number, 0);
    assert_eq!(result.range_locations[0].preview.start_column, 4);
    assert_eq!(result.range_locations[0].preview.end_line_number, 1);
    assert_eq!(result.range_locations[0].preview.end_column, 3);
}

#[test]
fn compacts_multiple_ranges_on_long_lines() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let preview_options = ITextSearchPreviewOptions { match_lines: 1, chars_per_line: 10 };

    let range1 = SearchRange::new(5, 4, 5, 7);
    let range2 = SearchRange::new(5, 133, 5, 136);
    let range3 = SearchRange::new(5, 141, 5, 144);
    let result = TextSearchMatch::new(
        "foo bar 123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890 foo bar baz bar",
        &[range1, range2, range3],
        Some(&preview_options),
        None,
    );
    assert_eq!(
        result.range_locations.iter().map(|e| e.preview).collect::<Vec<_>>(),
        vec![OneLineRange::new(0, 4, 7), OneLineRange::new(0, 42, 45), OneLineRange::new(0, 50, 53)]
    );
    assert_eq!(result.preview_text, "foo bar 123456⟪ 117 characters skipped ⟫o bar baz bar");
}

#[test]
fn trims_lines_endings() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let range = SearchRange::new(5, 3, 5, 5);
    let preview_options = ITextSearchPreviewOptions { match_lines: 1, chars_per_line: 10000 };

    assert_eq!(TextSearchMatch::new("foo bar\n", &[range], Some(&preview_options), None).preview_text, "foo bar");
    assert_eq!(TextSearchMatch::new("foo bar\r\n", &[range], Some(&preview_options), None).preview_text, "foo bar");
}

fn candidate(relative_path: &str) -> IRawFileMatch {
    IRawFileMatch { base: None, relative_path: relative_path.to_string(), search_path: None }
}

#[test]
fn glob_match_is_case_sensitive_by_default() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let candidate = candidate("src/Foo.ts");
    assert!(is_file_pattern_match(&candidate, "**/*.ts", false, false));
    assert!(!is_file_pattern_match(&candidate, "**/*.TS", false, false));
}

#[test]
fn glob_match_is_case_insensitive_when_ignore_case_is_true() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let candidate = candidate("src/Foo.ts");
    assert!(is_file_pattern_match(&candidate, "**/*.TS", false, true));
    assert!(is_file_pattern_match(&candidate, "**/*.Ts", false, true));
}

#[test]
fn glob_match_with_mixed_case_pattern() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let candidate = candidate("src/MyComponent.TSX");
    assert!(!is_file_pattern_match(&candidate, "**/*.tsx", false, false));
    assert!(is_file_pattern_match(&candidate, "**/*.tsx", false, true));
}
