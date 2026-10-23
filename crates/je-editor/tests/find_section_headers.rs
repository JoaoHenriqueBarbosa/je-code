//! Porte de `src/vs/editor/test/common/services/findSectionHeaders.test.ts` (suíte `FindSectionHeaders`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::find_section_headers::{FindSectionHeaderOptions, ISectionHeaderFinderTarget, find_section_headers};

struct TestSectionHeaderFinderTarget {
    lines: Vec<JsString>,
}

impl TestSectionHeaderFinderTarget {
    fn new<S: AsRef<str>>(lines: &[S]) -> Self {
        Self {
            lines: lines.iter().map(|line| JsString::from(line.as_ref())).collect(),
        }
    }
}

impl ISectionHeaderFinderTarget for TestSectionHeaderFinderTarget {
    fn get_line_count(&self) -> i32 {
        self.lines.len() as i32
    }

    fn get_line_content(&self, line_number: i32) -> JsString {
        self.lines[(line_number - 1) as usize].clone()
    }
}

/// As opções de todos os casos: só a busca por `MARK`, com a regex dada.
fn mark_options(mark_section_header_regex: &str) -> FindSectionHeaderOptions {
    FindSectionHeaderOptions {
        folding_rules: None,
        find_region_section_headers: false,
        find_mark_section_headers: true,
        mark_section_header_regex: JsString::from(mark_section_header_regex),
    }
}

const BANNER_REGEX: &str = r"^// =+\n^// (?<label>[^\n]+?)\n^// =+$";
const BANNER_DASH_EQUALS_REGEX: &str = r"^// [-=]+\n^// (?<label>[^\n]+?)\n^// [-=]+$";

#[test]
fn finds_simple_section_headers() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestSectionHeaderFinderTarget::new(&[
        "regular line",
        "MARK: My Section",
        "another line",
        "MARK: Another Section",
        "last line",
    ]);

    let options = mark_options(r"MARK:\s*(?<label>.*)$");

    let headers = find_section_headers(&model, &options);
    assert_eq!(headers.len(), 2);

    assert_eq!(headers[0].text, JsString::from("My Section"));
    assert_eq!(headers[0].range.start_line_number, 2);
    assert_eq!(headers[0].range.end_line_number, 2);

    assert_eq!(headers[1].text, JsString::from("Another Section"));
    assert_eq!(headers[1].range.start_line_number, 4);
    assert_eq!(headers[1].range.end_line_number, 4);
}

#[test]
fn finds_section_headers_with_separators() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestSectionHeaderFinderTarget::new(&[
        "regular line",
        "MARK: -My Section",
        "another line",
        "MARK: - Another Section",
        "last line",
    ]);

    let options = mark_options(r"MARK:\s*(?<separator>-?)\s*(?<label>.*)$");

    let headers = find_section_headers(&model, &options);
    assert_eq!(headers.len(), 2);

    assert_eq!(headers[0].text, JsString::from("My Section"));
    assert!(headers[0].has_separator_line);

    assert_eq!(headers[1].text, JsString::from("Another Section"));
    assert!(headers[1].has_separator_line);
}

#[test]
fn finds_multi_line_section_headers_with_separators() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestSectionHeaderFinderTarget::new(&[
        "regular line",
        "// ==========",
        "// My Section",
        "// ==========",
        "code...",
        "// ==========",
        "// Another Section",
        "// ==========",
        "more code...",
    ]);

    let options = mark_options(BANNER_REGEX);

    let headers = find_section_headers(&model, &options);
    assert_eq!(headers.len(), 2);

    assert_eq!(headers[0].text, JsString::from("My Section"));
    assert_eq!(headers[0].range.start_line_number, 2);
    assert_eq!(headers[0].range.end_line_number, 4);

    assert_eq!(headers[1].text, JsString::from("Another Section"));
    assert_eq!(headers[1].range.start_line_number, 6);
    assert_eq!(headers[1].range.end_line_number, 8);
}

#[test]
fn handles_overlapping_multi_line_section_headers_correctly() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestSectionHeaderFinderTarget::new(&[
        "// ==========",
        "// Section 1",
        "// ==========",
        "// ==========", // This line starts another header
        "// Section 2",
        "// ==========",
    ]);

    let options = mark_options(BANNER_REGEX);

    let headers = find_section_headers(&model, &options);
    assert_eq!(headers.len(), 2);

    assert_eq!(headers[0].text, JsString::from("Section 1"));
    assert_eq!(headers[0].range.start_line_number, 1);
    assert_eq!(headers[0].range.end_line_number, 3);

    assert_eq!(headers[1].text, JsString::from("Section 2"));
    assert_eq!(headers[1].range.start_line_number, 4);
    assert_eq!(headers[1].range.end_line_number, 6);
}

#[test]
fn section_headers_must_be_in_comments_when_specified() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestSectionHeaderFinderTarget::new(&[
        "// ==========",
        "// Section 1", // This one is in a comment
        "// ==========",
        "==========", // This one isn't
        "Section 2",
        "==========",
    ]);

    let options = mark_options(r"^(?:// )?=+\n^(?:// )?(?<label>[^\n]+?)\n^(?:// )?=+$");

    // Both patterns match, but the second one should be filtered out by the token check
    let headers = find_section_headers(&model, &options);
    assert!(headers[0].should_be_in_comments);
}

#[test]
fn handles_section_headers_at_chunk_boundaries() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Create enough lines to ensure we cross chunk boundaries
    let mut lines: Vec<String> = Vec::new();
    for i in 0..150 {
        lines.push(format!("line {i}"));
    }

    // Add headers near the chunk boundary (chunk size is 100)
    lines[97] = "// ==========".to_string();
    lines[98] = "// Section 1".to_string();
    lines[99] = "// ==========".to_string();
    lines[100] = "// ==========".to_string();
    lines[101] = "// Section 2".to_string();
    lines[102] = "// ==========".to_string();

    let model = TestSectionHeaderFinderTarget::new(&lines);

    let options = mark_options(BANNER_REGEX);

    let headers = find_section_headers(&model, &options);
    assert_eq!(headers.len(), 2);

    assert_eq!(headers[0].text, JsString::from("Section 1"));
    assert_eq!(headers[0].range.start_line_number, 98);
    assert_eq!(headers[0].range.end_line_number, 100);

    assert_eq!(headers[1].text, JsString::from("Section 2"));
    assert_eq!(headers[1].range.start_line_number, 101);
    assert_eq!(headers[1].range.end_line_number, 103);
}

#[test]
fn handles_empty_regex_gracefully_without_infinite_loop() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestSectionHeaderFinderTarget::new(&["line 1", "line 2", "line 3"]);

    // Empty string that would cause infinite loop
    let options = mark_options("");

    let headers = find_section_headers(&model, &options);
    assert_eq!(headers.len(), 0, "Should return no headers for empty regex");
}

#[test]
fn handles_whitespace_only_regex_gracefully_without_infinite_loop() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestSectionHeaderFinderTarget::new(&["line 1", "line 2", "line 3"]);

    // Whitespace that would cause infinite loop
    let options = mark_options("   ");

    let headers = find_section_headers(&model, &options);
    assert_eq!(headers.len(), 0, "Should return no headers for whitespace-only regex");
}

#[test]
fn correctly_advances_past_matches_without_infinite_loop() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestSectionHeaderFinderTarget::new(&[
        "// ==========",
        "// Section 1",
        "// ==========",
        "some code",
        "// ==========",
        "// Section 2",
        "// ==========",
        "more code",
        "// ==========",
        "// Section 3",
        "// ==========",
    ]);

    let options = mark_options(BANNER_REGEX);

    let headers = find_section_headers(&model, &options);
    assert_eq!(headers.len(), 3, "Should find all three section headers");
    assert_eq!(headers[0].text, JsString::from("Section 1"));
    assert_eq!(headers[1].text, JsString::from("Section 2"));
    assert_eq!(headers[2].text, JsString::from("Section 3"));
}

#[test]
fn handles_consecutive_section_headers_correctly() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestSectionHeaderFinderTarget::new(&[
        "// ==========",
        "// Section 1",
        "// ==========",
        "// ==========", // This line is both the end of Section 1 and start of Section 2
        "// Section 2",
        "// ==========",
    ]);

    let options = mark_options(BANNER_REGEX);

    let headers = find_section_headers(&model, &options);
    assert_eq!(headers.len(), 2, "Should find both section headers");
    assert_eq!(headers[0].text, JsString::from("Section 1"));
    assert_eq!(headers[1].text, JsString::from("Section 2"));
}

#[test]
fn handles_nested_separators_correctly() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestSectionHeaderFinderTarget::new(&[
        "// ==============",
        "// Major Section",
        "// ==============",
        "",
        "// ----------",
        "// Subsection",
        "// ----------",
    ]);

    let options = mark_options(BANNER_DASH_EQUALS_REGEX);

    let headers = find_section_headers(&model, &options);
    assert_eq!(headers.len(), 2, "Should find both section headers");
    assert_eq!(headers[0].text, JsString::from("Major Section"));
    assert_eq!(headers[1].text, JsString::from("Subsection"));
}

#[test]
fn handles_section_headers_at_chunk_boundaries_correctly() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut lines: Vec<String> = Vec::new();
    // Fill up to near the chunk boundary (chunk size is 100)
    for i in 0..97 {
        lines.push(format!("line {i}"));
    }

    // Add a section header that would cross the chunk boundary
    lines.push("// ==========".to_string()); // line 97
    lines.push("// Section 1".to_string()); // line 98
    lines.push("// ==========".to_string()); // line 99
    lines.push("// ==========".to_string()); // line 100 (chunk boundary)
    lines.push("// Section 2".to_string()); // line 101
    lines.push("// ==========".to_string()); // line 102

    // Add more content after
    for i in 103..150 {
        lines.push(format!("line {i}"));
    }

    let model = TestSectionHeaderFinderTarget::new(&lines);

    let options = mark_options(BANNER_REGEX);

    let headers = find_section_headers(&model, &options);
    assert_eq!(headers.len(), 2, "Should find both section headers across chunk boundary");

    assert_eq!(headers[0].text, JsString::from("Section 1"));
    assert_eq!(headers[0].range.start_line_number, 98);
    assert_eq!(headers[0].range.end_line_number, 100);

    assert_eq!(headers[1].text, JsString::from("Section 2"));
    assert_eq!(headers[1].range.start_line_number, 101);
    assert_eq!(headers[1].range.end_line_number, 103);
}

#[test]
fn handles_overlapping_section_headers_without_duplicates() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestSectionHeaderFinderTarget::new(&[
        "// ==========",  // Line 1
        "// Section 1",   // Line 2 - This is part of first header
        "// ==========",  // Line 3 - This is the end of first
        "// Section 2",   // Line 4 - This is not a header
        "// ==========",  // Line 5
        "// ==========",  // Line 6 - Start of second header
        "// Section 3",   // Line 7
        "// ===========", // Line 8
    ]);

    let options = mark_options(BANNER_REGEX);

    let headers = find_section_headers(&model, &options);
    assert_eq!(headers.len(), 2);

    assert_eq!(headers[0].text, JsString::from("Section 1"));
    assert_eq!(headers[0].range.start_line_number, 1);
    assert_eq!(headers[0].range.end_line_number, 3);

    // assert.strictEqual(headers[1].text, 'Section 2');
    // assert.strictEqual(headers[1].range.startLineNumber, 3);
    // assert.strictEqual(headers[1].range.endLineNumber, 5);

    assert_eq!(headers[1].text, JsString::from("Section 3"));
    assert_eq!(headers[1].range.start_line_number, 6);
    assert_eq!(headers[1].range.end_line_number, 8);
}

#[test]
fn handles_partially_overlapping_multiline_section_headers_correctly() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = TestSectionHeaderFinderTarget::new(&[
        "// ================", // Line 1
        "// Major Section 1",  // Line 2
        "// ================", // Line 3
        "// --------",         // Line 4 - Start of subsection that overlaps with end of major section
        "// Subsection 1.1",   // Line 5
        "// --------",         // Line 6
        "// ================", // Line 7
        "// Major Section 2",  // Line 8
        "// ================", // Line 9
    ]);

    let options = mark_options(BANNER_DASH_EQUALS_REGEX);

    let headers = find_section_headers(&model, &options);
    assert_eq!(headers.len(), 3);

    assert_eq!(headers[0].text, JsString::from("Major Section 1"));
    assert_eq!(headers[0].range.start_line_number, 1);
    assert_eq!(headers[0].range.end_line_number, 3);

    assert_eq!(headers[1].text, JsString::from("Subsection 1.1"));
    assert_eq!(headers[1].range.start_line_number, 4);
    assert_eq!(headers[1].range.end_line_number, 6);

    assert_eq!(headers[2].text, JsString::from("Major Section 2"));
    assert_eq!(headers[2].range.start_line_number, 7);
    assert_eq!(headers[2].range.end_line_number, 9);
}
