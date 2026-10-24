//! Porte de `src/vs/editor/contrib/wordPartOperations/test/browser/wordPartOperations.test.ts` (suíte
//! `WordPartOperations`), caso a caso e na mesma ordem, com os auxiliares de `wordTestUtils.ts` em
//! `tests/support/word_test_utils.rs`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de `je-base/tests/common`, criado no começo de cada
//!   caso.
//! - `new DeleteWordPartLeft()` e os demais comandos da suíte são as funções de `je_editor::word_part_operations`
//!   (`ops::cursor_word_part_left()`...), que criam o comando sem registrá-lo; como o comando não guarda estado, criá-lo
//!   a cada chamada em vez de uma vez por suíte não muda nada.
//! - `runEditorCommand(editor, command)` é `run_editor_command` de `tests/support/test_code_editor.rs` (o
//!   `command.runEditorCommand(serviceAccessor, editor, null)` do original). O `StaticServiceAccessor` com o
//!   `TestLanguageConfigurationService` (`utils.ts`) é o próprio acessor do editor de teste, que já injeta o serviço de
//!   configuração de linguagem de teste.
//! - `withTestCodeEditor(text, options, cb)` (dentro de `testRepeatedActionAndExtractPositions`) é
//!   `with_test_code_editor`; `{ wordSeparators: '...' }` é o objeto `Dynamic` das opções.
//! - `ed.getValue().length` e `text.length` são o tamanho em unidades UTF-16 (o `length` do JavaScript).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::position::Position;
use je_editor::word_part_operations as ops;
use support::test_code_editor::{TestCodeEditor, TestCodeEditorInstantiationOptions, run_editor_command};
use support::word_test_utils::{
    deserialize_pipe_positions, serialize_pipe_positions, test_repeated_action_and_extract_positions,
};

/// As opções dos casos `issue #158667`: `{ wordSeparators: '...' }`, os separadores padrão sem `$-%~` e mais `·`.
const KEBAB_WORD_SEPARATORS: &str = "!\"#&'()*+,./:;<=>?@[\\]^`{|}·";

fn cursor_word_part_left(editor: &TestCodeEditor, in_selection_mode: bool) {
    run_editor_command(
        editor,
        &if in_selection_mode { ops::cursor_word_part_left_select() } else { ops::cursor_word_part_left() },
    );
}

fn cursor_word_part_right(editor: &TestCodeEditor, in_selection_mode: bool) {
    run_editor_command(
        editor,
        &if in_selection_mode { ops::cursor_word_part_right_select() } else { ops::cursor_word_part_right() },
    );
}

fn delete_word_part_left(editor: &TestCodeEditor) {
    run_editor_command(editor, &ops::delete_word_part_left());
}

fn delete_word_part_right(editor: &TestCodeEditor) {
    run_editor_command(editor, &ops::delete_word_part_right());
}

/// `ed.getPosition()!`.
fn position_of(editor: &TestCodeEditor) -> Position {
    editor.get_position().expect("o editor tem o modelo")
}

/// `ed.getValue().length`: o tamanho em unidades UTF-16.
fn value_length(editor: &TestCodeEditor) -> i32 {
    editor.get_value(None).units().len() as i32
}

/// `text.length`: o tamanho em unidades UTF-16.
fn text_length(text: &str) -> i32 {
    text.encode_utf16().count() as i32
}

/// `ed => ed.getPosition()!.equals(new Position(line, column))`.
fn is_at(line_number: i32, column: i32) -> impl Fn(&TestCodeEditor) -> bool {
    move |editor: &TestCodeEditor| position_of(editor) == Position::new(line_number, column)
}

/// `ed => ed.getValue().length === 0`.
fn is_empty(editor: &TestCodeEditor) -> bool {
    value_length(editor) == 0
}

/// `{ wordSeparators: KEBAB_WORD_SEPARATORS }`.
fn kebab_options() -> TestCodeEditorInstantiationOptions {
    TestCodeEditorInstantiationOptions::new(Dynamic::object(vec![("wordSeparators", Dynamic::from(KEBAB_WORD_SEPARATORS))]))
}

/// O que quase todos os casos repetem depois do `deserializePipePositions(EXPECTED)`:
/// `testRepeatedActionAndExtractPositions(...)`, `serializePipePositions(text, actualStops)` e o
/// `assert.deepStrictEqual(actual, EXPECTED)`.
fn assert_repeated_action_stops(
    text: &str,
    expected: &str,
    initial_position: Position,
    action: impl Fn(&TestCodeEditor),
    record: impl Fn(&TestCodeEditor) -> Position,
    stop_condition: impl Fn(&TestCodeEditor) -> bool,
    options: TestCodeEditorInstantiationOptions,
) {
    let actual_stops =
        test_repeated_action_and_extract_positions(text, initial_position, action, record, stop_condition, options);
    let actual = serialize_pipe_positions(text, actual_stops);
    assert_eq!(actual, expected);
}

#[test]
fn cursor_word_part_left_basic() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = [
        "|start| |line|",
        "|this|Is|A|Camel|Case|Var|  |this_|is_|a_|snake_|case_|var| |THIS_|IS_|CAPS_|SNAKE| |this_|IS|Mixed|Use|",
        "|end| |line",
    ]
    .join("\n");
    let (text, _) = deserialize_pipe_positions(&expected);
    assert_repeated_action_stops(
        &text,
        &expected,
        Position::new(1000, 1000),
        |ed| cursor_word_part_left(ed, false),
        position_of,
        is_at(1, 1),
        TestCodeEditorInstantiationOptions::default(),
    );
}

#[test]
fn cursor_word_part_left_issue_53899_whitespace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = "|myvar| |=| |'|demonstration|     |of| |selection| |with| |space|'";
    let (text, _) = deserialize_pipe_positions(expected);
    assert_repeated_action_stops(
        &text,
        expected,
        Position::new(1000, 1000),
        |ed| cursor_word_part_left(ed, false),
        position_of,
        is_at(1, 1),
        TestCodeEditorInstantiationOptions::default(),
    );
}

#[test]
fn cursor_word_part_left_issue_53899_underscores() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = "|myvar| |=| |'|demonstration_____|of| |selection| |with| |space|'";
    let (text, _) = deserialize_pipe_positions(expected);
    assert_repeated_action_stops(
        &text,
        expected,
        Position::new(1000, 1000),
        |ed| cursor_word_part_left(ed, false),
        position_of,
        is_at(1, 1),
        TestCodeEditorInstantiationOptions::default(),
    );
}

#[test]
fn cursor_word_part_right_basic() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = [
        "start| |line|",
        "|this|Is|A|Camel|Case|Var|  |this|_is|_a|_snake|_case|_var| |THIS|_IS|_CAPS|_SNAKE| |this|_IS|Mixed|Use|",
        "|end| |line|",
    ]
    .join("\n");
    let (text, _) = deserialize_pipe_positions(&expected);
    assert_repeated_action_stops(
        &text,
        &expected,
        Position::new(1, 1),
        |ed| cursor_word_part_right(ed, false),
        position_of,
        is_at(3, 9),
        TestCodeEditorInstantiationOptions::default(),
    );
}

#[test]
fn cursor_word_part_right_issue_53899_whitespace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = "myvar| |=| |'|demonstration|     |of| |selection| |with| |space|'|";
    let (text, _) = deserialize_pipe_positions(expected);
    assert_repeated_action_stops(
        &text,
        expected,
        Position::new(1, 1),
        |ed| cursor_word_part_right(ed, false),
        position_of,
        is_at(1, 52),
        TestCodeEditorInstantiationOptions::default(),
    );
}

#[test]
fn cursor_word_part_right_issue_53899_underscores() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = "myvar| |=| |'|demonstration|_____of| |selection| |with| |space|'|";
    let (text, _) = deserialize_pipe_positions(expected);
    assert_repeated_action_stops(
        &text,
        expected,
        Position::new(1, 1),
        |ed| cursor_word_part_right(ed, false),
        position_of,
        is_at(1, 52),
        TestCodeEditorInstantiationOptions::default(),
    );
}

#[test]
fn cursor_word_part_right_issue_53899_second_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = [";| |--| |1|", "|;|        |--| |2|", "|;|    |#|3|", "|;|   |#|4|"].join("\n");
    let (text, _) = deserialize_pipe_positions(&expected);
    assert_repeated_action_stops(
        &text,
        &expected,
        Position::new(1, 1),
        |ed| cursor_word_part_right(ed, false),
        position_of,
        is_at(4, 7),
        TestCodeEditorInstantiationOptions::default(),
    );
}

#[test]
fn issue_93239_cursor_word_part_right() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = ["foo|_bar|"].join("\n");
    let (text, _) = deserialize_pipe_positions(&expected);
    assert_repeated_action_stops(
        &text,
        &expected,
        Position::new(1, 1),
        |ed| cursor_word_part_right(ed, false),
        position_of,
        is_at(1, 8),
        TestCodeEditorInstantiationOptions::default(),
    );
}

#[test]
fn issue_93239_cursor_word_part_left() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = ["|foo_|bar"].join("\n");
    let (text, _) = deserialize_pipe_positions(&expected);
    assert_repeated_action_stops(
        &text,
        &expected,
        Position::new(1, 8),
        |ed| cursor_word_part_left(ed, false),
        position_of,
        is_at(1, 1),
        TestCodeEditorInstantiationOptions::default(),
    );
}

#[test]
fn delete_word_part_left_basic() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = "|   |/*| |Just| |some| |text| |a|+=| |3| |+|5|-|3| |*/|  |this|Is|A|Camel|Case|Var|  |this_|is_|a_|snake_|case_|var| |THIS_|IS_|CAPS_|SNAKE| |this_|IS|Mixed|Use";
    let (text, _) = deserialize_pipe_positions(expected);
    assert_repeated_action_stops(
        &text,
        expected,
        Position::new(1, 1000),
        delete_word_part_left,
        position_of,
        is_empty,
        TestCodeEditorInstantiationOptions::default(),
    );
}

#[test]
fn delete_word_part_right_basic() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = "   |/*| |Just| |some| |text| |a|+=| |3| |+|5|-|3| |*/|  |this|Is|A|Camel|Case|Var|  |this|_is|_a|_snake|_case|_var| |THIS|_IS|_CAPS|_SNAKE| |this|_IS|Mixed|Use|";
    let (text, _) = deserialize_pipe_positions(expected);
    let original_length = text_length(&text);
    assert_repeated_action_stops(
        &text,
        expected,
        Position::new(1, 1),
        delete_word_part_right,
        |ed| Position::new(1, original_length - value_length(ed) + 1),
        is_empty,
        TestCodeEditorInstantiationOptions::default(),
    );
}

#[test]
fn issue_158667_cursor_word_part_left_stops_at_dash_even_when_dash_is_not_in_word_separators() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = ["|this-|is-|a-|kebab-|case-|var| |THIS-|IS-|CAPS-|KEBAB| |this-|IS|Mixed|Use"].join("\n");
    let (text, _) = deserialize_pipe_positions(&expected);
    assert_repeated_action_stops(
        &text,
        &expected,
        Position::new(1000, 1000),
        |ed| cursor_word_part_left(ed, false),
        position_of,
        is_at(1, 1),
        kebab_options(), // default characters sans '$-%~' plus '·'
    );
}

#[test]
fn issue_158667_cursor_word_part_right_stops_at_dash_even_when_dash_is_not_in_word_separators() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = ["this|-is|-a|-kebab|-case|-var| |THIS|-IS|-CAPS|-KEBAB| |this|-IS|Mixed|Use|"].join("\n");
    let (text, _) = deserialize_pipe_positions(&expected);
    assert_repeated_action_stops(
        &text,
        &expected,
        Position::new(1, 1),
        |ed| cursor_word_part_right(ed, false),
        position_of,
        is_at(1, 60),
        kebab_options(), // default characters sans '$-%~' plus '·'
    );
}

#[test]
fn issue_158667_delete_word_part_left_stops_at_dash_even_when_dash_is_not_in_word_separators() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = ["|this-|is-|a-|kebab-|case-|var| |THIS-|IS-|CAPS-|KEBAB| |this-|IS|Mixed|Use"].join(" ");
    let (text, _) = deserialize_pipe_positions(&expected);
    assert_repeated_action_stops(
        &text,
        &expected,
        Position::new(1000, 1000),
        delete_word_part_left,
        position_of,
        is_empty,
        kebab_options(), // default characters sans '$-%~' plus '·'
    );
}

#[test]
fn issue_158667_delete_word_part_right_stops_at_dash_even_when_dash_is_not_in_word_separators() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let expected = ["this|-is|-a|-kebab|-case|-var| |THIS|-IS|-CAPS|-KEBAB| |this|-IS|Mixed|Use|"].join(" ");
    let (text, _) = deserialize_pipe_positions(&expected);
    let original_length = text_length(&text);
    assert_repeated_action_stops(
        &text,
        &expected,
        Position::new(1, 1),
        delete_word_part_right,
        |ed| Position::new(1, original_length - value_length(ed) + 1),
        is_empty,
        kebab_options(), // default characters sans '$-%~' plus '·'
    );
}
