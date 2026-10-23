//! Porte de `src/vs/platform/opener/test/common/opener.test.ts` (14 casos, na ordem do original).

use je_base::JsString;
use je_base::uri::URI;
use je_platform::opener::editor_types::TextEditorSelection;
use je_platform::opener::{extract_selection, with_selection};

fn parse(text: &str) -> URI {
    URI::parse(text, false)
}

fn selection(
    start_line_number: f64,
    start_column: f64,
    end_line_number: Option<f64>,
    end_column: Option<f64>,
) -> TextEditorSelection {
    TextEditorSelection { start_line_number, start_column, end_line_number, end_column }
}

fn js(text: &str) -> JsString {
    JsString::from(text)
}

#[test]
fn extract_selection_with_only_start_line_number() {
    let uri = parse("file:///some/file.js#73");
    assert_eq!(extract_selection(&uri).selection, Some(selection(73.0, 1.0, None, None)));
}

#[test]
fn extract_selection_with_only_start_line_number_in_l_format() {
    let uri = parse("file:///some/file.js#L73");
    assert_eq!(extract_selection(&uri).selection, Some(selection(73.0, 1.0, None, None)));
}

#[test]
fn extract_selection_with_start_line_number_and_start_column() {
    let uri = parse("file:///some/file.js#73,84");
    assert_eq!(extract_selection(&uri).selection, Some(selection(73.0, 84.0, None, None)));
}

#[test]
fn extract_selection_with_start_line_number_and_start_column_in_l_format() {
    let uri = parse("file:///some/file.js#L73,84");
    assert_eq!(extract_selection(&uri).selection, Some(selection(73.0, 84.0, None, None)));
}

#[test]
fn extract_selection_with_range_and_no_column_number() {
    let uri = parse("file:///some/file.js#73-83");
    assert_eq!(extract_selection(&uri).selection, Some(selection(73.0, 1.0, Some(83.0), Some(1.0))));
}

#[test]
fn extract_selection_with_range_and_no_column_number_in_l_format() {
    let uri = parse("file:///some/file.js#L73-L83");
    assert_eq!(extract_selection(&uri).selection, Some(selection(73.0, 1.0, Some(83.0), Some(1.0))));
}

#[test]
fn extract_selection_with_range_and_no_column_number_in_l_format_only_for_start() {
    let uri = parse("file:///some/file.js#L73-83");
    assert_eq!(extract_selection(&uri).selection, Some(selection(73.0, 1.0, Some(83.0), Some(1.0))));
}

#[test]
fn extract_selection_with_range_and_no_column_number_in_l_format_only_for_end() {
    let uri = parse("file:///some/file.js#73-L83");
    assert_eq!(extract_selection(&uri).selection, Some(selection(73.0, 1.0, Some(83.0), Some(1.0))));
}

#[test]
fn extract_selection_with_complete_range() {
    let uri = parse("file:///some/file.js#73,84-83,52");
    assert_eq!(extract_selection(&uri).selection, Some(selection(73.0, 84.0, Some(83.0), Some(52.0))));
}

#[test]
fn extract_selection_with_complete_range_in_l_format() {
    let uri = parse("file:///some/file.js#L73,84-L83,52");
    assert_eq!(extract_selection(&uri).selection, Some(selection(73.0, 84.0, Some(83.0), Some(52.0))));
}

#[test]
fn with_selection_with_start_line_number_and_start_column() {
    assert_eq!(
        with_selection(&parse("file:///some/file.js"), &selection(73.0, 84.0, None, None)).to_string(false),
        js("file:///some/file.js#73%2C84")
    );
}

#[test]
fn with_selection_with_start_line_number_start_column_and_end_line_number() {
    assert_eq!(
        with_selection(&parse("file:///some/file.js"), &selection(73.0, 84.0, Some(83.0), None)).to_string(false),
        js("file:///some/file.js#73%2C84-83")
    );
}

#[test]
fn with_selection_with_start_line_number_start_column_and_end_line_number_end_column() {
    assert_eq!(
        with_selection(&parse("file:///some/file.js"), &selection(73.0, 84.0, Some(83.0), Some(52.0))).to_string(false),
        js("file:///some/file.js#73%2C84-83%2C52")
    );
}

#[test]
fn extract_selection_returns_original_with_selection_uri() {
    let mut uri = parse("file:///some/file.js");

    let uri_with_selection = with_selection(&parse("file:///some/file.js"), &selection(73.0, 84.0, Some(83.0), Some(52.0)));
    assert_eq!(uri.to_string(false), extract_selection(&uri_with_selection).uri.to_string(false));

    uri = parse("file:///some/file.js");
    assert_eq!(uri.to_string(false), extract_selection(&uri).uri.to_string(false));
}
