//! Porte de `src/vs/editor/test/browser/controller/columnSelectionPaste.test.ts` (suíte `Column selection paste`, com a
//! suíte aninhada `clipboard metadata`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso ([`leak_guard`]); o
//!   carregamento do módulo `coreCommands` (que registra os comandos) vem antes do guarda, como o `import` do original.
//! - `withTestCodeEditor(text, { multiCursorPaste: 'spread', ...options }, ...)` é `with_test_code_editor` de
//!   `tests/support/test_code_editor.rs`; as opções são o objeto `Dynamic` montado por [`Scenario::editor_options`]
//!   (a chave repetida de `options` vence, como o `...` do original).
//! - `editor.trigger('keyboard', Handler.Paste, payload)` é [`paste`]: o `payload` é o `PastePayload` do porte, e o
//!   `Partial<PastePayload>` de `overrides` é [`PasteOverrides`] (`isBlock: undefined` é `Some(false)`, e
//!   `multicursorText: null` é `Some(None)`).
//! - `new Selection(...)` é a abreviação `sel`; as seleções e os textos são comparados pelo `toString()` e por linhas.
//! - `DataTransfer`, `ClipboardEvent` e `File` do DOM são os dublês de `je_editor::edit_context::clipboard_utils`;
//!   `createClipboardCopyEvent(..., context, logService, isFirefox)` recebe o `viewModel` no lugar do `ViewContext`
//!   (o único membro que o teste e a produção leem) e não recebe o `ILogService` (só diagnóstico).
//! - `InMemoryClipboardMetadataManager.INSTANCE.get('')` do `teardown` é [`in_memory_clipboard_metadata_manager`].
//! - `InputMode.setInputMode('overtype')` com o `finally` que restaura o modo anterior é o [`InputModeScope`].
//! - `EditorOptions.multiCursorPaste.validate('invalid')` é a opção do `editor_options_registry()` validada pela
//!   tabela de registro, com o valor devolvido lido como `String`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::platform::Platform;
use je_base::strings::split_lines;
use je_editor::code_editor_widget::{EditorViewModel, Handler, ICodeEditor, PastePayload};
use je_editor::config::editor_options::registry::{MULTI_CURSOR_PASTE, editor_options_registry};
use je_editor::config::editor_options::{EditorOption, InputMode};
use je_editor::core_commands::{ColumnSelectCommandOptions, core_navigation_commands};
use je_editor::cursor_events::CursorChangeReason;
use je_editor::edit_context::clipboard_utils::{
    ClipboardEvent, DataTransfer, create_clipboard_copy_event, create_clipboard_paste_event,
    in_memory_clipboard_metadata_manager,
};
use je_editor::input_mode::{get_input_mode, set_input_mode};
use je_editor::model::{EndOfLineSequence, TextModelUpdateOptions};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use support::test_code_editor::{ITestCodeEditor, TestCodeEditorInstantiationOptions, with_test_code_editor};

const BLOCK: &str = "A\nBC\nD";

fn sel(start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) -> Selection {
    Selection::new(start_line_number, start_column, end_line_number, end_column)
}

/// O `ensureNoDisposablesAreLeakedInTestSuite()` e o `teardown` da suíte, depois do carregamento dos comandos.
struct SuiteGuard {
    _leaks: Box<dyn std::any::Any>,
}

impl Drop for SuiteGuard {
    fn drop(&mut self) {
        in_memory_clipboard_metadata_manager().get(&JsString::default());
    }
}

fn leak_guard() -> SuiteGuard {
    core_navigation_commands::instance();
    SuiteGuard { _leaks: Box::new(ensure_no_disposables_are_leaked_in_test_suite()) }
}

fn lines(values: &[&str]) -> Vec<JsString> {
    values.iter().map(|value| JsString::from(*value)).collect()
}

fn strings(values: &Option<Vec<JsString>>) -> Option<Vec<String>> {
    values.as_ref().map(|lines| lines.iter().map(JsString::to_string_lossy).collect())
}

fn selections_text(selections: &[Selection]) -> Vec<String> {
    selections.iter().map(ToString::to_string).collect()
}

/// `{ text: model.getLinesContent(), selections: editor.getSelections() }`.
fn state(editor: &ITestCodeEditor) -> (Vec<JsString>, Vec<String>) {
    let model = editor.get_model().expect("o editor tem o modelo");
    (model.get_lines_content(), selections_text(&editor.get_selections().expect("o editor tem seleções")))
}

/// O `Partial<PastePayload>` de `overrides`.
#[derive(Default)]
struct PasteOverrides {
    multicursor_text: Option<Option<Vec<JsString>>>,
    is_block: Option<bool>,
}

/// `paste(editor, text, overrides)`.
fn paste(editor: &ITestCodeEditor, text: &str, overrides: &PasteOverrides) {
    let units: Vec<u16> = text.encode_utf16().collect();
    let payload = PastePayload {
        text: JsString::from(text),
        paste_on_new_line: false,
        multicursor_text: match &overrides.multicursor_text {
            Some(value) => value.clone(),
            None => Some(split_lines(&units).into_iter().map(JsString::from).collect()),
        },
        mode: None,
        is_block: overrides.is_block.unwrap_or(true),
    };
    editor.trigger(Some("keyboard"), Handler::PASTE, Some(payload.to_dynamic()));
}

/// O objeto do `assertPaste`.
#[derive(Default)]
struct Scenario<'a> {
    text: &'a [&'a str],
    selections: Vec<Selection>,
    expected: &'a [&'a str],
    pasted_text: Option<&'a str>,
    tab_size: Option<i32>,
    options: Vec<(&'a str, Dynamic)>,
    payload: PasteOverrides,
}

impl Scenario<'_> {
    /// `{ multiCursorPaste: 'spread', ...scenario.options }`.
    fn editor_options(&self) -> Dynamic {
        let mut entries: Vec<(String, Dynamic)> = vec![("multiCursorPaste".to_string(), Dynamic::from("spread"))];
        for (key, value) in &self.options {
            match entries.iter_mut().find(|(existing, _)| existing == key) {
                Some(entry) => entry.1 = value.clone(),
                None => entries.push((key.to_string(), value.clone())),
            }
        }
        Dynamic::object(entries)
    }
}

fn assert_paste(scenario: Scenario<'_>) {
    let _guard = leak_guard();
    let options = TestCodeEditorInstantiationOptions::new(scenario.editor_options());
    with_test_code_editor(scenario.text, options, |editor, _, _| {
        let model = editor.get_model().expect("o editor tem o modelo");
        model.update_options(TextModelUpdateOptions { tab_size: Some(scenario.tab_size.unwrap_or(4)), ..Default::default() });
        editor.set_selections(&scenario.selections, None, None);
        paste(editor, scenario.pasted_text.unwrap_or(BLOCK), &scenario.payload);
        assert_eq!(model.get_lines_content(), lines(scenario.expected));
    });
}

fn spread_options() -> TestCodeEditorInstantiationOptions {
    TestCodeEditorInstantiationOptions::new(Dynamic::object([("multiCursorPaste", Dynamic::from("spread"))]))
}

const LEFT_RIGHT: [&str; 3] = ["left--right", "left--right", "left--right"];
const XX_YY_ZZ: [&str; 3] = ["xx", "yy", "zz"];

#[test]
fn inserts_successive_rows_from_a_single_cursor() {
    assert_paste(Scenario {
        text: &LEFT_RIGHT,
        selections: vec![sel(1, 5, 1, 5)],
        expected: &["leftA --right", "leftBC--right", "leftD --right"],
        ..Scenario::default()
    });
}

#[test]
fn replaces_a_single_line_selection_with_the_first_row() {
    assert_paste(Scenario {
        text: &LEFT_RIGHT,
        selections: vec![sel(1, 5, 1, 7)],
        expected: &["leftA right", "leftBC--right", "leftD --right"],
        ..Scenario::default()
    });
}

#[test]
fn uses_the_start_of_a_reversed_selection() {
    assert_paste(Scenario {
        text: &LEFT_RIGHT,
        selections: vec![sel(1, 7, 1, 5)],
        expected: &["leftA right", "leftBC--right", "leftD --right"],
        ..Scenario::default()
    });
}

#[test]
fn replaces_an_equal_height_continuous_selection_with_the_first_row() {
    assert_paste(Scenario {
        text: &LEFT_RIGHT,
        selections: vec![sel(1, 5, 3, 7)],
        expected: &["leftA right", "    BC", "    D "],
        ..Scenario::default()
    });
}

#[test]
fn replaces_a_reversed_equal_height_continuous_selection() {
    assert_paste(Scenario {
        text: &LEFT_RIGHT,
        selections: vec![sel(3, 7, 1, 5)],
        expected: &["leftA right", "    BC", "    D "],
        ..Scenario::default()
    });
}

#[test]
fn replaces_a_full_line_selection_including_its_final_newline() {
    assert_paste(Scenario {
        text: &["first", "second", "third", "unselected"],
        selections: vec![sel(1, 1, 4, 1)],
        expected: &["A unselected", "BC", "D "],
        ..Scenario::default()
    });
}

#[test]
fn preserves_the_suffix_when_a_reversed_selection_ends_at_column_one() {
    assert_paste(Scenario {
        text: &["left--right", "second", "third", "unselected"],
        selections: vec![sel(4, 1, 1, 5)],
        expected: &["leftA unselected", "    BC", "    D "],
        ..Scenario::default()
    });
}

#[test]
fn replaces_a_shorter_continuous_selection_with_the_first_row() {
    assert_paste(Scenario {
        text: &["left--right", "left--right", "last"],
        selections: vec![sel(1, 5, 2, 7)],
        expected: &["leftA right", "lastBC", "    D "],
        ..Scenario::default()
    });
}

#[test]
fn replaces_a_taller_continuous_selection_with_the_first_row() {
    assert_paste(Scenario {
        text: &["left--right", "middle", "middle", "left--right"],
        selections: vec![sel(1, 5, 4, 7)],
        expected: &["leftA right", "    BC", "    D "],
        ..Scenario::default()
    });
}

#[test]
fn pads_short_and_empty_lines_to_the_block_column() {
    assert_paste(Scenario {
        text: &["left--right", "x", ""],
        selections: vec![sel(1, 5, 1, 5)],
        expected: &["leftA --right", "x   BC", "    D "],
        ..Scenario::default()
    });
}

#[test]
fn appends_missing_rows_without_moving_the_existing_suffix() {
    assert_paste(Scenario {
        text: &["left--right"],
        selections: vec![sel(1, 5, 1, 5)],
        expected: &["leftA --right", "    BC", "    D "],
        ..Scenario::default()
    });
}

#[test]
fn appends_rows_when_the_cursor_is_at_the_end_of_the_file() {
    assert_paste(Scenario {
        text: &["left"],
        selections: vec![sel(1, 5, 1, 5)],
        expected: &["leftA ", "    BC", "    D "],
        ..Scenario::default()
    });
}

#[test]
fn pastes_into_an_empty_document() {
    assert_paste(Scenario {
        text: &[""],
        selections: vec![sel(1, 1, 1, 1)],
        expected: &["A ", "BC", "D "],
        ..Scenario::default()
    });
}

#[test]
fn preserves_empty_and_trailing_copied_rows() {
    assert_paste(Scenario {
        text: &["left--right", "left--right", "left--right", "left--right"],
        selections: vec![sel(1, 5, 1, 5)],
        pasted_text: Some("A\n\nD\n"),
        expected: &["leftA--right", "left --right", "leftD--right", "left --right"],
        ..Scenario::default()
    });
}

#[test]
fn pads_short_lines_for_empty_and_trailing_copied_rows() {
    assert_paste(Scenario {
        text: &["left", "x", "", "y"],
        selections: vec![sel(1, 5, 1, 5)],
        pasted_text: Some("A\n\nD\n"),
        expected: &["leftA", "x    ", "    D", "y    "],
        ..Scenario::default()
    });
}

#[test]
fn appends_empty_and_trailing_copied_rows() {
    assert_paste(Scenario {
        text: &["left"],
        selections: vec![sel(1, 5, 1, 5)],
        pasted_text: Some("A\n\nD\n"),
        expected: &["leftA", "     ", "    D", "     "],
        ..Scenario::default()
    });
}

#[test]
fn aligns_using_visible_columns_across_tabs_and_spaces() {
    assert_paste(Scenario {
        text: &["\tleft", "    left", "  \tleft"],
        selections: vec![sel(1, 2, 1, 2)],
        expected: &["\tA left", "    BCleft", "  \tD left"],
        ..Scenario::default()
    });
}

#[test]
fn pads_short_lines_using_the_configured_tab_size() {
    assert_paste(Scenario {
        text: &["\t--left", "\t", ""],
        selections: vec![sel(1, 4, 1, 4)],
        tab_size: Some(8),
        expected: &["\t--A left", "\t  BC", "          D "],
        ..Scenario::default()
    });
}

#[test]
fn pads_using_visible_widths_of_graphemes_and_full_width_characters() {
    assert_paste(Scenario {
        text: &["left--right", "e\u{0301}", "\u{4E2D}", "\u{1F600}"],
        selections: vec![sel(1, 5, 1, 5)],
        pasted_text: Some("A\nB\nC\nD"),
        expected: &["leftA--right", "e\u{0301}   B", "\u{4E2D}  C", "\u{1F600}  D"],
        ..Scenario::default()
    });
}

#[test]
fn uses_grapheme_boundaries_and_full_width_character_columns() {
    assert_paste(Scenario {
        text: &["\u{1F600}left", "e\u{0301}xleft", "\u{4E2D}left"],
        selections: vec![sel(1, 3, 1, 3)],
        expected: &["\u{1F600}A left", "e\u{0301}xBCleft", "\u{4E2D}D left"],
        ..Scenario::default()
    });
}

#[test]
fn pads_copied_rows_to_the_same_visible_width() {
    assert_paste(Scenario {
        text: &["left--right", "left--right", "left--right", "left--right"],
        selections: vec![sel(1, 5, 1, 5)],
        pasted_text: Some("\tX\n\u{4E2D}\n\u{1F600}\ne\u{0301}"),
        expected: &["left\tX--right", "left\u{4E2D}   --right", "left\u{1F600}   --right", "lefte\u{0301}    --right"],
        ..Scenario::default()
    });
}

#[test]
fn uses_successive_model_lines_when_word_wrap_is_enabled() {
    assert_paste(Scenario {
        text: &LEFT_RIGHT,
        selections: vec![sel(1, 5, 1, 5)],
        options: vec![("wordWrap", Dynamic::from("wordWrapColumn")), ("wordWrapColumn", Dynamic::from(6_i32))],
        expected: &["leftA --right", "leftBC--right", "leftD --right"],
        ..Scenario::default()
    });
}

#[test]
fn does_not_create_cursors_or_truncate_at_the_multi_cursor_limit() {
    assert_paste(Scenario {
        text: &LEFT_RIGHT,
        selections: vec![sel(1, 5, 1, 5)],
        options: vec![("multiCursorLimit", Dynamic::from(1_i32))],
        expected: &["leftA --right", "leftBC--right", "leftD --right"],
        ..Scenario::default()
    });
}

#[test]
fn distributes_matching_cursors_in_document_order() {
    assert_paste(Scenario {
        text: &XX_YY_ZZ,
        selections: vec![sel(3, 2, 3, 2), sel(1, 2, 1, 2), sel(2, 2, 2, 2)],
        expected: &["xAx", "yBCy", "zDz"],
        ..Scenario::default()
    });
}

#[test]
fn distributes_matching_non_empty_selections() {
    assert_paste(Scenario {
        text: &["[one]", "[two]", "[three]"],
        selections: vec![sel(1, 2, 1, 5), sel(2, 2, 2, 5), sel(3, 2, 3, 7)],
        expected: &["[A]", "[BC]", "[D]"],
        ..Scenario::default()
    });
}

#[test]
fn repeats_the_whole_text_when_multiple_cursor_counts_differ() {
    assert_paste(Scenario {
        text: &XX_YY_ZZ,
        selections: vec![sel(1, 2, 1, 2), sel(3, 2, 3, 2)],
        expected: &["xA", "BC", "Dx", "yy", "zA", "BC", "Dz"],
        ..Scenario::default()
    });
}

#[test]
fn uses_normal_replacement_for_multiple_destinations_including_a_multiline_selection() {
    assert_paste(Scenario {
        text: &["xx", "yy", "zz", "[one]", "[two]"],
        selections: vec![sel(1, 2, 1, 2), sel(4, 2, 5, 5)],
        expected: &["xA", "BC", "Dx", "yy", "zz", "[A", "BC", "D]"],
        ..Scenario::default()
    });
}

#[test]
fn retains_ordinary_trailing_newline_handling_when_spreading_to_multiple_cursors() {
    assert_paste(Scenario {
        text: &["xx", "yy"],
        selections: vec![sel(1, 2, 1, 2), sel(2, 2, 2, 2)],
        pasted_text: Some("A\nB\n"),
        expected: &["xAx", "yBy"],
        ..Scenario::default()
    });
}

#[test]
fn block_mode_respects_multi_cursor_paste_full_for_multiple_destinations_without_row_metadata() {
    assert_paste(Scenario {
        text: &XX_YY_ZZ,
        selections: vec![sel(1, 2, 1, 2), sel(2, 2, 2, 2), sel(3, 2, 3, 2)],
        options: vec![("multiCursorPaste", Dynamic::from("full"))],
        payload: PasteOverrides { multicursor_text: Some(None), ..PasteOverrides::default() },
        expected: &["xA", "BC", "Dx", "yA", "BC", "Dy", "zA", "BC", "Dz"],
        ..Scenario::default()
    });
}

#[test]
fn full_mode_retains_single_cursor_multiline_pasting() {
    assert_paste(Scenario {
        text: &["left--right", "last"],
        selections: vec![sel(1, 5, 1, 5)],
        options: vec![("multiCursorPaste", Dynamic::from("full"))],
        expected: &["leftA", "BC", "D--right", "last"],
        ..Scenario::default()
    });
}

#[test]
fn full_mode_retains_existing_multi_cursor_metadata_distribution() {
    assert_paste(Scenario {
        text: &XX_YY_ZZ,
        selections: vec![sel(1, 2, 1, 2), sel(2, 2, 2, 2), sel(3, 2, 3, 2)],
        options: vec![("multiCursorPaste", Dynamic::from("full"))],
        expected: &["xAx", "yBCy", "zDz"],
        ..Scenario::default()
    });
}

#[test]
fn ordinary_multiline_copies_spread_into_a_rectangle_at_a_single_cursor() {
    assert_paste(Scenario {
        text: &["left--right", "last"],
        selections: vec![sel(1, 5, 1, 5)],
        payload: PasteOverrides { is_block: Some(true), ..PasteOverrides::default() },
        expected: &["leftA --right", "lastBC", "    D "],
        ..Scenario::default()
    });
}

#[test]
fn missing_block_metadata_still_spreads_multiline_text_at_a_single_cursor() {
    assert_paste(Scenario {
        text: &["left--right", "last"],
        selections: vec![sel(1, 5, 1, 5)],
        payload: PasteOverrides { is_block: Some(true), multicursor_text: Some(None) },
        expected: &["leftA --right", "lastBC", "    D "],
        ..Scenario::default()
    });
}

#[test]
fn preserves_ordinary_spread_pasting_without_metadata() {
    assert_paste(Scenario {
        text: &XX_YY_ZZ,
        selections: vec![sel(1, 2, 1, 2), sel(2, 2, 2, 2), sel(3, 2, 3, 2)],
        payload: PasteOverrides { is_block: Some(false), multicursor_text: Some(None) },
        expected: &["xAx", "yBCy", "zDz"],
        ..Scenario::default()
    });
}

#[test]
fn preserves_ordinary_full_pasting_without_metadata() {
    assert_paste(Scenario {
        text: &["xx", "yy"],
        selections: vec![sel(1, 2, 1, 2), sel(2, 2, 2, 2)],
        pasted_text: Some("A\nB"),
        options: vec![("multiCursorPaste", Dynamic::from("full"))],
        payload: PasteOverrides { is_block: Some(false), multicursor_text: Some(None) },
        expected: &["xA", "Bx", "yA", "By"],
        ..Scenario::default()
    });
}

#[test]
fn does_not_edit_a_read_only_document() {
    assert_paste(Scenario {
        text: &["left--right", "last"],
        selections: vec![sel(1, 5, 1, 5)],
        options: vec![("readOnly", Dynamic::from(true))],
        expected: &["left--right", "last"],
        ..Scenario::default()
    });
}

#[test]
fn preserves_multi_cursor_distribution_through_undo_and_redo_in_block_mode() {
    let _guard = leak_guard();
    let text = ["xx", "yy", "zz"];
    with_test_code_editor(&text[..], spread_options(), |editor, _, _| {
        let model = editor.get_model().expect("o editor tem o modelo");
        let selections = [sel(1, 2, 1, 2), sel(2, 2, 2, 2), sel(3, 2, 3, 2)];
        editor.set_selections(&selections, None, None);
        paste(editor, BLOCK, &PasteOverrides::default());
        let after = state(editor);
        model.undo();
        let undone = state(editor);
        model.redo();
        let expected = (lines(&["xAx", "yBCy", "zDz"]), selections_text(&[sel(1, 3, 1, 3), sel(2, 4, 2, 4), sel(3, 3, 3, 3)]));
        assert_eq!(after, expected);
        assert_eq!(undone, (lines(&text), selections_text(&selections)));
        assert_eq!(state(editor), expected);
    });
}

#[test]
fn replaces_a_multiline_selection_and_aligns_later_rows_through_undo_and_redo() {
    let _guard = leak_guard();
    let text = ["\tfirst", "middle", "last--suffix", "x", ""];
    with_test_code_editor(&text[..], spread_options(), |editor, _, _| {
        let model = editor.get_model().expect("o editor tem o modelo");
        model.update_options(TextModelUpdateOptions { tab_size: Some(4), ..Default::default() });
        let selection = sel(3, 7, 1, 2);
        editor.set_selection(selection.into(), None);
        paste(editor, "\nBC\n", &PasteOverrides::default());
        let after = state(editor);
        model.undo();
        let undone = state(editor);
        model.redo();
        let expected = (lines(&["\t  suffix", "x   BC", "      "]), selections_text(&[sel(3, 7, 3, 7)]));
        assert_eq!(after, expected);
        assert_eq!(undone, (lines(&text), selections_text(&[selection])));
        assert_eq!(state(editor), expected);
    });
}

#[test]
fn preserves_crlf_line_endings() {
    let _guard = leak_guard();
    with_test_code_editor(&["left", "x"][..], spread_options(), |editor, _, _| {
        let model = editor.get_model().expect("o editor tem o modelo");
        model.set_eol(EndOfLineSequence::CrLf);
        editor.set_position(Position::new(1, 5), None);
        paste(editor, "A\r\nBC\r\nD", &PasteOverrides::default());
        assert_eq!(model.get_value(), JsString::from("leftA \r\nx   BC\r\n    D "));
    });
}

#[test]
fn leaves_the_cursor_after_rectangular_padding_on_an_empty_trailing_pasted_row() {
    let _guard = leak_guard();
    with_test_code_editor(&["left", "left", "left"][..], spread_options(), |editor, _, _| {
        editor.set_position(Position::new(1, 5), None);
        paste(editor, "A\nBC\n", &PasteOverrides::default());
        assert_eq!(state(editor), (lines(&["leftA ", "leftBC", "left  "]), selections_text(&[sel(3, 7, 3, 7)])));
    });
}

#[test]
fn leaves_the_cursor_after_padding_on_an_empty_trailing_pasted_row() {
    let _guard = leak_guard();
    with_test_code_editor(&["left", "x", ""][..], spread_options(), |editor, _, _| {
        editor.set_position(Position::new(1, 5), None);
        paste(editor, "A\nBC\n", &PasteOverrides::default());
        assert_eq!(state(editor), (lines(&["leftA ", "x   BC", "      "]), selections_text(&[sel(3, 7, 3, 7)])));
    });
}

#[test]
fn leaves_the_cursor_after_padding_on_an_appended_empty_row() {
    let _guard = leak_guard();
    with_test_code_editor(&["left"][..], spread_options(), |editor, _, _| {
        editor.set_position(Position::new(1, 5), None);
        paste(editor, "A\nBC\n", &PasteOverrides::default());
        assert_eq!(state(editor), (lines(&["leftA ", "    BC", "      "]), selections_text(&[sel(3, 7, 3, 7)])));
    });
}

#[test]
fn undo_and_redo_preserve_text_and_the_original_single_selection() {
    let _guard = leak_guard();
    with_test_code_editor(&["left--right", "x"][..], spread_options(), |editor, _, _| {
        let model = editor.get_model().expect("o editor tem o modelo");
        let selection = sel(1, 7, 1, 5);
        editor.set_selection(selection.into(), None);
        paste(editor, BLOCK, &PasteOverrides::default());
        let after = state(editor);
        model.undo();
        let undone = state(editor);
        model.redo();
        let redone = state(editor);
        let expected_after = (lines(&["leftA right", "x   BC", "    D "]), selections_text(&[sel(3, 7, 3, 7)]));
        assert_eq!(after, expected_after);
        assert_eq!(undone, (lines(&["left--right", "x"]), selections_text(&[selection])));
        assert_eq!(redone, expected_after);
    });
}

#[test]
fn changes_only_the_row_ranges_in_one_content_change() {
    let _guard = leak_guard();
    with_test_code_editor(&["left--right", "x", ""][..], spread_options(), |editor, _, _| {
        let changes: Rc<RefCell<Vec<Vec<(Range, String)>>>> = Rc::new(RefCell::new(Vec::new()));
        let recorded = changes.clone();
        editor.register_disposable(editor.on_did_change_model_content().listen(move |event| {
            recorded.borrow_mut().push(event.changes.iter().map(|change| (change.range, change.text.to_string_lossy())).collect());
        }));
        editor.set_position(Position::new(1, 5), None);
        paste(editor, BLOCK, &PasteOverrides::default());
        assert_eq!(
            *changes.borrow(),
            vec![vec![
                (Range::new(3, 1, 3, 1), "    D ".to_string()),
                (Range::new(2, 2, 2, 2), "   BC".to_string()),
                (Range::new(1, 5, 1, 5), "A ".to_string()),
            ]]
        );
    });
}

#[test]
fn reports_pasted_ranges_for_cursors_and_multiline_destination_selections() {
    let _guard = leak_guard();
    with_test_code_editor(&LEFT_RIGHT[..], spread_options(), |editor, _, _| {
        let events: Rc<RefCell<Vec<Range>>> = Rc::new(RefCell::new(Vec::new()));
        let recorded = events.clone();
        editor.register_disposable(editor.on_did_paste().listen(move |event| recorded.borrow_mut().push(event.range)));
        editor.set_selection(sel(1, 5, 1, 5).into(), None);
        paste(editor, BLOCK, &PasteOverrides::default());
        editor.get_model().expect("o editor tem o modelo").undo();
        editor.set_selection(sel(1, 5, 2, 7).into(), None);
        paste(editor, BLOCK, &PasteOverrides::default());
        assert_eq!(*events.borrow(), vec![Range::new(1, 5, 3, 7), Range::new(1, 5, 3, 7)]);
    });
}

#[test]
fn responds_to_spread_full_and_spread_configuration_transitions() {
    let _guard = leak_guard();
    let options = TestCodeEditorInstantiationOptions::default();
    with_test_code_editor(&["left", "x", ""][..], options, |editor, _, _| {
        let model = editor.get_model().expect("o editor tem o modelo");
        let mut results: Vec<Vec<JsString>> = Vec::new();
        let default_value = editor.get_option(MULTI_CURSOR_PASTE).clone();
        for multi_cursor_paste in ["spread", "full", "spread"] {
            editor.update_options(Some(&Dynamic::object([("multiCursorPaste", Dynamic::from(multi_cursor_paste))])));
            editor.set_position(Position::new(1, 5), None);
            paste(editor, BLOCK, &PasteOverrides::default());
            results.push(model.get_lines_content());
            model.undo();
        }
        let registry = editor_options_registry();
        let validated = registry[EditorOption::MultiCursorPaste as usize].validate(&Dynamic::from("invalid"));
        let invalid_value = validated.downcast_ref::<String>().expect("a opção é uma string").clone();
        assert_eq!(default_value, "spread");
        assert_eq!(invalid_value, "spread");
        assert_eq!(
            results,
            vec![
                lines(&["leftA ", "x   BC", "    D "]),
                lines(&["leftA", "BC", "D", "x", ""]),
                lines(&["leftA ", "x   BC", "    D "]),
            ]
        );
    });
}

/// O `try`/`finally` do caso `respects overtype and pads short rows`: restaura o modo de digitação anterior.
struct InputModeScope {
    previous: InputMode,
}

impl InputModeScope {
    fn set(input_mode: InputMode) -> Self {
        let previous = get_input_mode();
        set_input_mode(input_mode);
        Self { previous }
    }
}

impl Drop for InputModeScope {
    fn drop(&mut self) {
        set_input_mode(self.previous);
    }
}

#[test]
fn respects_overtype_and_pads_short_rows() {
    let _scope = InputModeScope::set(InputMode::Overtype);
    assert_paste(Scenario {
        text: &["left--right", "left--right", "x", ""],
        selections: vec![sel(1, 5, 1, 5)],
        pasted_text: Some("A\nBC\nD\nE"),
        options: vec![("overtypeOnPaste", Dynamic::from(true))],
        expected: &["leftA right", "leftBCright", "x   D ", "    E "],
        ..Scenario::default()
    });
}

mod clipboard_metadata {
    use super::*;

    fn select_column(view_model: &EditorViewModel, from: Position, to: Position) {
        view_model.set_selections(Some("test"), &[Selection::from_positions(from, from)], CursorChangeReason::NotSet);
        core_navigation_commands::instance().column_select.run_core_editor_command(
            view_model,
            &ColumnSelectCommandOptions {
                position: Some(to),
                view_position: Some(to),
                mouse_column: Some(to.column),
                do_column_select: true,
                ..ColumnSelectCommandOptions::default()
            },
        );
    }

    fn copy(view_model: &EditorViewModel, is_cut: bool) -> Rc<DataTransfer> {
        let clipboard_data = Rc::new(DataTransfer::new());
        let browser_event = ClipboardEvent::new(if is_cut { "cut" } else { "copy" }, Some(clipboard_data.clone()));
        let event = create_clipboard_copy_event(browser_event, is_cut, view_model, false);
        event.ensure_clipboard_gets_editor_data();
        if is_cut {
            view_model.cut(Some("keyboard"));
        }
        clipboard_data
    }

    fn paste_event(clipboard_data: &Rc<DataTransfer>) -> je_editor::edit_context::clipboard_utils::ClipboardPasteEvent {
        create_clipboard_paste_event(ClipboardEvent::new("paste", Some(clipboard_data.clone())))
    }

    fn assert_rectangular_clipboard_identity(is_cut: bool) {
        let _guard = leak_guard();
        let source = ["<AA>", "<BB>", "<CC>"];
        with_test_code_editor(&source[..], TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| {
            select_column(view_model, Position::new(1, 2), Position::new(3, 4));
            let clipboard_data = copy(view_model, is_cut);
            let event = paste_event(&clipboard_data);
            let metadata = event.metadata.as_ref();
            let document: Vec<JsString> = editor.get_model().expect("o editor tem o modelo").get_lines_content();
            assert_eq!(event.text.to_string_lossy(), "AA\nBB\nCC");
            assert_eq!(metadata.and_then(|metadata| metadata.is_block), Some(true));
            assert_eq!(
                metadata.and_then(|metadata| strings(&metadata.multicursor_text)),
                Some(vec!["AA".to_string(), "BB".to_string(), "CC".to_string()])
            );
            assert_eq!(metadata.and_then(|metadata| metadata.is_from_empty_selection), Some(false));
            assert_eq!(document, if is_cut { lines(&["<>", "<>", "<>"]) } else { lines(&["<AA>", "<BB>", "<CC>"]) });
        });
    }

    #[test]
    fn copy_retains_rectangular_identity_through_clipboard_serialization() {
        assert_rectangular_clipboard_identity(false);
    }

    #[test]
    fn cut_retains_rectangular_identity_through_clipboard_serialization() {
        assert_rectangular_clipboard_identity(true);
    }

    #[test]
    fn copies_reversed_rectangles_in_document_order() {
        let _guard = leak_guard();
        with_test_code_editor(&["<AA>", "<BB>", "<CC>"][..], TestCodeEditorInstantiationOptions::default(), |_, view_model, _| {
            select_column(view_model, Position::new(3, 4), Position::new(1, 2));
            let event = paste_event(&copy(view_model, false));
            assert_eq!(event.text.to_string_lossy(), "AA\nBB\nCC");
            assert_eq!(event.metadata.as_ref().and_then(|metadata| metadata.is_block), Some(true));
        });
    }

    #[test]
    fn single_row_column_copies_preserve_the_single_selection_clipboard_shape() {
        let _guard = leak_guard();
        with_test_code_editor(&["<AA>"][..], TestCodeEditorInstantiationOptions::default(), |_, view_model, _| {
            select_column(view_model, Position::new(1, 2), Position::new(1, 4));
            let event = paste_event(&copy(view_model, false));
            let metadata = event.metadata.as_ref();
            assert_eq!(event.text.to_string_lossy(), "AA");
            assert_eq!(metadata.and_then(|metadata| strings(&metadata.multicursor_text)), None);
            assert_eq!(metadata.and_then(|metadata| metadata.is_block), Some(true));
        });
    }

    #[test]
    fn aligned_ordinary_multi_selections_are_not_rectangular_copies() {
        let _guard = leak_guard();
        with_test_code_editor(&["<AA>", "<BB>", "<CC>"][..], TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| {
            editor.set_selections(&[sel(1, 2, 1, 4), sel(2, 2, 2, 4), sel(3, 2, 3, 4)], None, None);
            let event = paste_event(&copy(view_model, false));
            assert_eq!(event.text.to_string_lossy(), "AA\nBB\nCC");
            assert_eq!(event.metadata.as_ref().and_then(|metadata| metadata.is_block), Some(false));
        });
    }

    #[test]
    fn zero_width_column_selections_retain_ordinary_whole_line_copying() {
        let _guard = leak_guard();
        let options = TestCodeEditorInstantiationOptions::new(Dynamic::object([("emptySelectionClipboard", Dynamic::from(true))]));
        with_test_code_editor(&["AA", "BB", "CC"][..], options, |_, view_model, _| {
            select_column(view_model, Position::new(1, 2), Position::new(3, 2));
            let event = paste_event(&copy(view_model, false));
            let metadata = event.metadata.as_ref();
            let new_line = if Platform::detect().is_windows { "\r\n" } else { "\n" };
            let expected: Vec<String> = ["AA", "BB", "CC"].iter().map(|line| format!("{line}{new_line}")).collect();
            assert_eq!(metadata.and_then(|metadata| strings(&metadata.multicursor_text)), Some(expected));
            assert_eq!(metadata.and_then(|metadata| metadata.is_block), Some(true));
        });
    }

    #[test]
    fn changing_the_selection_clears_rectangular_identity() {
        let _guard = leak_guard();
        with_test_code_editor(&["<AA>", "<BB>", "<CC>"][..], TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| {
            select_column(view_model, Position::new(1, 2), Position::new(3, 4));
            editor.set_selection(sel(1, 2, 3, 4).into(), None);
            let event = paste_event(&copy(view_model, false));
            assert_eq!(event.metadata.as_ref().and_then(|metadata| metadata.is_block), Some(false));
        });
    }

    #[test]
    fn in_memory_clipboard_fallback_retains_rectangular_identity_only_for_matching_text() {
        let _guard = leak_guard();
        with_test_code_editor(&["<AA>", "<BB>", "<CC>"][..], TestCodeEditorInstantiationOptions::default(), |_, view_model, _| {
            select_column(view_model, Position::new(1, 2), Position::new(3, 4));
            let clipboard_data = copy(view_model, false);
            clipboard_data.clear_data("vscode-editor-data");
            let matching = paste_event(&clipboard_data);
            clipboard_data.set_data("text/plain", "external text");
            let changed = paste_event(&clipboard_data);
            assert_eq!(matching.metadata.as_ref().and_then(|metadata| metadata.is_block), Some(true));
            assert!(changed.metadata.is_none());
        });
    }

    #[test]
    fn pastes_a_copied_rectangle_into_another_editor_using_serialized_metadata() {
        let _guard = leak_guard();
        let mut actual: Vec<JsString> = Vec::new();
        with_test_code_editor(&["<AA>", "<BB>", "<CC>"][..], TestCodeEditorInstantiationOptions::default(), |_, view_model, _| {
            select_column(view_model, Position::new(1, 2), Position::new(3, 4));
            let clipboard_data = copy(view_model, false);
            in_memory_clipboard_metadata_manager().get(&JsString::default());
            with_test_code_editor(&LEFT_RIGHT[..], spread_options(), |target, _, _| {
                target.set_position(Position::new(1, 5), None);
                let event = paste_event(&clipboard_data);
                let metadata = event.metadata.as_ref();
                paste(
                    target,
                    &event.text.to_string_lossy(),
                    &PasteOverrides {
                        is_block: Some(metadata.and_then(|metadata| metadata.is_block) == Some(true)),
                        multicursor_text: Some(metadata.and_then(|metadata| metadata.multicursor_text.clone())),
                    },
                );
                actual = target.get_model().expect("o editor tem o modelo").get_lines_content();
            });
        });
        assert_eq!(actual, lines(&["leftAA--right", "leftBB--right", "leftCC--right"]));
    }

    #[test]
    fn older_clipboard_metadata_without_a_block_flag_uses_normal_paste() {
        let _guard = leak_guard();
        let clipboard_data = Rc::new(DataTransfer::new());
        clipboard_data.set_data("text/plain", BLOCK);
        clipboard_data.set_data(
            "vscode-editor-data",
            r#"{"version":1,"isFromEmptySelection":false,"multicursorText":["A","BC","D"],"mode":null}"#,
        );
        with_test_code_editor(&["left--right"][..], TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            editor.set_position(Position::new(1, 5), None);
            let event = paste_event(&clipboard_data);
            let is_block = event.metadata.as_ref().and_then(|metadata| metadata.is_block) == Some(true);
            paste(editor, &event.text.to_string_lossy(), &PasteOverrides { is_block: Some(is_block), ..PasteOverrides::default() });
            assert_eq!(editor.get_model().expect("o editor tem o modelo").get_lines_content(), lines(&["leftA", "BC", "D--right"]));
        });
    }
}
