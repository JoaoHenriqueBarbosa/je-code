//! Porte de `src/vs/editor/test/browser/controller/textAreaState.test.ts`, caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `createTextModel(text)` é `create_text_model` de `tests/support/test_text_model.rs`.
//! - O `MockTextAreaWrapper` (um `Disposable` no original) é uma struct sem recursos a descartar, então o
//!   `textArea.dispose()` não tem equivalente.
//! - `new TextAreaState(value, a, b, null, undefined)` é `TextAreaState::new(js(value), a, b, None, None)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::edit_context::screen_reader_utils::{IPagedScreenReaderStrategy, SimplePagedScreenReaderStrategy};
use je_editor::edit_context::text_area_state::{ITextAreaWrapper, ITypeData, TextAreaState};
use je_editor::range::Range;
use je_editor::selection::Selection;
use support::test_text_model::create_text_model;

fn js(text: &str) -> JsString {
    JsString::from(text)
}

struct MockTextAreaWrapper {
    value: JsString,
    selection_start: usize,
    selection_end: usize,
}

impl MockTextAreaWrapper {
    fn new() -> Self {
        MockTextAreaWrapper { value: JsString::new(), selection_start: 0, selection_end: 0 }
    }
}

impl ITextAreaWrapper for MockTextAreaWrapper {
    fn get_value(&self) -> JsString {
        self.value.clone()
    }

    fn set_value(&mut self, _reason: &str, value: &[u16]) {
        self.value = JsString::from(value);
        self.selection_start = self.value.len();
        self.selection_end = self.value.len();
    }

    fn get_selection_start(&self) -> usize {
        self.selection_start
    }

    fn get_selection_end(&self) -> usize {
        self.selection_end
    }

    fn set_selection_range(&mut self, _reason: &str, selection_start: usize, selection_end: usize) {
        // Os valores do original são limitados a [0, value.length]; `usize` já não passa de zero.
        self.selection_start = selection_start.min(self.value.len());
        self.selection_end = selection_end.min(self.value.len());
    }
}

fn equals_text_area_state(a: &TextAreaState, b: &TextAreaState) -> bool {
    a.value == b.value
        && a.selection_start == b.selection_start
        && a.selection_end == b.selection_end
        && a.selection == b.selection
        && a.newline_count_before_selection == b.newline_count_before_selection
}

fn assert_text_area_state(actual: &TextAreaState, value: &str, selection_start: usize, selection_end: usize) {
    let desired = TextAreaState::new(js(value), selection_start, selection_end, None, None);
    assert!(equals_text_area_state(&desired, actual), "{} == {}", desired, actual);
}

#[test]
fn from_text_area() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut text_area = MockTextAreaWrapper::new();
    text_area.value = js("Hello world!");
    text_area.selection_start = 1;
    text_area.selection_end = 12;
    let mut actual = TextAreaState::read_from_text_area(&text_area, None);

    assert_text_area_state(&actual, "Hello world!", 1, 12);
    assert_eq!(actual.value, "Hello world!");
    assert_eq!(actual.selection_start, 1);

    actual = actual.collapse_selection();
    assert_text_area_state(&actual, "Hello world!", 12, 12);
}

#[test]
fn apply_to_text_area() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut text_area = MockTextAreaWrapper::new();
    text_area.value = js("Hello world!");
    text_area.selection_start = 1;
    text_area.selection_end = 12;

    let mut state = TextAreaState::new(js("Hi world!"), 2, 2, None, None);
    state.write_to_text_area("test", &mut text_area, false);

    assert_eq!(text_area.value, "Hi world!");
    assert_eq!(text_area.selection_start, 9);
    assert_eq!(text_area.selection_end, 9);

    state = TextAreaState::new(js("Hi world!"), 3, 3, None, None);
    state.write_to_text_area("test", &mut text_area, false);

    assert_eq!(text_area.value, "Hi world!");
    assert_eq!(text_area.selection_start, 9);
    assert_eq!(text_area.selection_end, 9);

    state = TextAreaState::new(js("Hi world!"), 0, 2, None, None);
    state.write_to_text_area("test", &mut text_area, true);

    assert_eq!(text_area.value, "Hi world!");
    assert_eq!(text_area.selection_start, 0);
    assert_eq!(text_area.selection_end, 2);
}

fn test_deduce_input(
    prev_state: Option<TextAreaState>,
    value: &str,
    selection_start: usize,
    selection_end: usize,
    could_be_emoji_input: bool,
    expected: &str,
    expected_char_replace_cnt: usize,
) {
    let prev_state = prev_state.unwrap_or(TextAreaState::EMPTY);

    let mut text_area = MockTextAreaWrapper::new();
    text_area.value = js(value);
    text_area.selection_start = selection_start;
    text_area.selection_end = selection_end;

    let new_state = TextAreaState::read_from_text_area(&text_area, None);
    let actual = TextAreaState::deduce_input(&prev_state, &new_state, could_be_emoji_input);

    assert_eq!(
        actual,
        ITypeData {
            text: js(expected),
            replace_prev_char_cnt: expected_char_replace_cnt,
            replace_next_char_cnt: 0,
            position_delta: 0,
        }
    );
}

#[test]
fn extract_new_text_no_previous_state_with_selection() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(None, "a", 0, 1, true, "a", 0);
}

#[test]
fn issue_2586_replacing_selected_end_of_line_with_newline_locks_up_the_document() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(Some(TextAreaState::new(js("]\n"), 1, 2, None, None)), "]\n", 2, 2, true, "\n", 0);
}

#[test]
fn extract_new_text_no_previous_state_without_selection() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(None, "a", 1, 1, true, "a", 0);
}

#[test]
fn extract_new_text_typing_does_not_cause_a_selection() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(Some(TextAreaState::EMPTY), "a", 0, 1, true, "a", 0);
}

#[test]
fn extract_new_text_had_the_textarea_empty() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(Some(TextAreaState::EMPTY), "a", 1, 1, true, "a", 0);
}

#[test]
fn extract_new_text_had_the_entire_line_selected() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(Some(TextAreaState::new(js("Hello world!"), 0, 12, None, None)), "H", 1, 1, true, "H", 0);
}

#[test]
fn extract_new_text_had_previous_text_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(
        Some(TextAreaState::new(js("Hello world!"), 12, 12, None, None)),
        "Hello world!a",
        13,
        13,
        true,
        "a",
        0,
    );
}

#[test]
fn extract_new_text_had_previous_text_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(
        Some(TextAreaState::new(js("Hello world!"), 0, 0, None, None)),
        "aHello world!",
        1,
        1,
        true,
        "a",
        0,
    );
}

#[test]
fn extract_new_text_had_previous_text_3() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(
        Some(TextAreaState::new(js("Hello world!"), 6, 11, None, None)),
        "Hello other!",
        11,
        11,
        true,
        "other",
        0,
    );
}

#[test]
fn extract_new_text_ime() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(Some(TextAreaState::EMPTY), "これは", 3, 3, true, "これは", 0);
}

#[test]
fn extract_new_text_is_in_overwrite_mode() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(
        Some(TextAreaState::new(js("Hello world!"), 0, 0, None, None)),
        "Aello world!",
        1,
        1,
        true,
        "A",
        0,
    );
}

#[test]
fn extract_mac_replaced_text_does_nothing_if_there_is_selection() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(
        Some(TextAreaState::new(js("Hello world!"), 5, 5, None, None)),
        "Hellö world!",
        4,
        5,
        true,
        "ö",
        0,
    );
}

#[test]
fn extract_mac_replaced_text_does_nothing_if_there_is_more_than_one_extra_char() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(
        Some(TextAreaState::new(js("Hello world!"), 5, 5, None, None)),
        "Hellöö world!",
        5,
        5,
        true,
        "öö",
        1,
    );
}

#[test]
fn extract_mac_replaced_text_does_nothing_if_there_is_more_than_one_changed_char() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(
        Some(TextAreaState::new(js("Hello world!"), 5, 5, None, None)),
        "Helöö world!",
        5,
        5,
        true,
        "öö",
        2,
    );
}

#[test]
fn extract_mac_replaced_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(
        Some(TextAreaState::new(js("Hello world!"), 5, 5, None, None)),
        "Hellö world!",
        5,
        5,
        true,
        "ö",
        1,
    );
}

#[test]
fn issue_25101_first_key_press_ignored() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(Some(TextAreaState::new(js("a"), 0, 1, None, None)), "a", 1, 1, true, "a", 0);
}

#[test]
fn issue_16520_cmd_d_of_single_character_followed_by_typing_same_character_as_has_no_effect() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_input(Some(TextAreaState::new(js("x x"), 0, 1, None, None)), "x x", 1, 1, true, "x", 0);
}

#[allow(clippy::too_many_arguments)]
fn test_deduce_android_composition_input(
    prev_state: Option<TextAreaState>,
    value: &str,
    selection_start: usize,
    selection_end: usize,
    expected: &str,
    expected_replace_prev_char_cnt: usize,
    expected_replace_next_char_cnt: usize,
    expected_position_delta: i64,
) {
    let prev_state = prev_state.unwrap_or(TextAreaState::EMPTY);

    let mut text_area = MockTextAreaWrapper::new();
    text_area.value = js(value);
    text_area.selection_start = selection_start;
    text_area.selection_end = selection_end;

    let new_state = TextAreaState::read_from_text_area(&text_area, None);
    let actual = TextAreaState::deduce_android_composition_input(&prev_state, &new_state);

    assert_eq!(
        actual,
        ITypeData {
            text: js(expected),
            replace_prev_char_cnt: expected_replace_prev_char_cnt,
            replace_next_char_cnt: expected_replace_next_char_cnt,
            position_delta: expected_position_delta,
        }
    );
}

#[test]
fn android_composition_input_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_android_composition_input(
        Some(TextAreaState::new(js("Microsoft"), 4, 4, None, None)),
        "Microsoft",
        4,
        4,
        "",
        0,
        0,
        0,
    );
}

#[test]
fn android_composition_input_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_android_composition_input(
        Some(TextAreaState::new(js("Microsoft"), 4, 4, None, None)),
        "Microsoft",
        0,
        9,
        "",
        0,
        0,
        5,
    );
}

#[test]
fn android_composition_input_3() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_android_composition_input(
        Some(TextAreaState::new(js("Microsoft"), 0, 9, None, None)),
        "Microsoft's",
        11,
        11,
        "'s",
        0,
        0,
        0,
    );
}

#[test]
fn android_backspace() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_deduce_android_composition_input(
        Some(TextAreaState::new(js("undefinedVariable"), 2, 2, None, None)),
        "udefinedVariable",
        1,
        1,
        "",
        1,
        0,
        0,
    );
}

// suite('SimplePagedScreenReaderStrategy')

fn test_paged_screen_reader_strategy(lines: &[&str], selection: Selection, expected: TextAreaState) {
    let model = create_text_model(&lines.join("\n"));
    let screen_reader_strategy = SimplePagedScreenReaderStrategy;
    let screen_reader_content_state = screen_reader_strategy.from_editor_selection(&*model, &selection, 10, true);
    let text_area_state = TextAreaState::from_screen_reader_content_state(&screen_reader_content_state);
    assert!(equals_text_area_state(&text_area_state, &expected), "{} == {}", expected, text_area_state);
    model.dispose();
}

#[test]
fn simple() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_paged_screen_reader_strategy(
        &["Hello world!"],
        Selection::new(1, 13, 1, 13),
        TextAreaState::new(js("Hello world!"), 12, 12, Some(Range::new(1, 13, 1, 13)), Some(0)),
    );

    test_paged_screen_reader_strategy(
        &["Hello world!"],
        Selection::new(1, 1, 1, 1),
        TextAreaState::new(js("Hello world!"), 0, 0, Some(Range::new(1, 1, 1, 1)), Some(0)),
    );

    test_paged_screen_reader_strategy(
        &["Hello world!"],
        Selection::new(1, 1, 1, 6),
        TextAreaState::new(js("Hello world!"), 0, 5, Some(Range::new(1, 1, 1, 6)), Some(0)),
    );
}

#[test]
fn multiline() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_paged_screen_reader_strategy(
        &["Hello world!", "How are you?"],
        Selection::new(1, 1, 1, 1),
        TextAreaState::new(js("Hello world!\nHow are you?"), 0, 0, Some(Range::new(1, 1, 1, 1)), Some(0)),
    );

    test_paged_screen_reader_strategy(
        &["Hello world!", "How are you?"],
        Selection::new(2, 1, 2, 1),
        TextAreaState::new(js("Hello world!\nHow are you?"), 13, 13, Some(Range::new(2, 1, 2, 1)), Some(1)),
    );
}

#[test]
fn page() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    const LINES: &str = "L1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\nL9\nL10\nL11\nL12\nL13\nL14\nL15\nL16\nL17\nL18\nL19\nL20\nL21";

    test_paged_screen_reader_strategy(
        &[LINES],
        Selection::new(1, 1, 1, 1),
        TextAreaState::new(js("L1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\nL9\nL10\n"), 0, 0, Some(Range::new(1, 1, 1, 1)), Some(0)),
    );

    test_paged_screen_reader_strategy(
        &[LINES],
        Selection::new(11, 1, 11, 1),
        TextAreaState::new(js("L11\nL12\nL13\nL14\nL15\nL16\nL17\nL18\nL19\nL20\n"), 0, 0, Some(Range::new(11, 1, 11, 1)), Some(0)),
    );

    test_paged_screen_reader_strategy(
        &[LINES],
        Selection::new(12, 1, 12, 1),
        TextAreaState::new(js("L11\nL12\nL13\nL14\nL15\nL16\nL17\nL18\nL19\nL20\n"), 4, 4, Some(Range::new(12, 1, 12, 1)), Some(1)),
    );

    test_paged_screen_reader_strategy(
        &[LINES],
        Selection::new(21, 1, 21, 1),
        TextAreaState::new(js("L21"), 0, 0, Some(Range::new(21, 1, 21, 1)), Some(0)),
    );
}
