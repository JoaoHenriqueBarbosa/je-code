//! Porte de `src/vs/workbench/contrib/terminal/test/browser/xterm/shellIntegrationAddon.test.ts` (suíte `ShellIntegrationAddon`), caso a caso e na mesma ordem.
//!
//! Casos de fora (23), que dependem do emulador de terminal (Fase 6: o `Terminal` do xterm com `parser.registerOscHandler`,
//! `writeP`) e das capacidades `CommandDetectionCapability`, `CwdDetectionCapability` e `BufferMarkCapability`, ainda não
//! portadas, e dos mocks do `sinon` sobre elas:
//! - `cwd detection`: `should activate capability on the cwd sequence`, `should pass cwd sequence to the capability as
//!   trusted when nonce matches`, `should treat cwd sequence as untrusted when nonce is missing`, `... when nonce does not
//!   match`, `detect ITerm sequence`, `detect SetCwd sequence` (`should accept well-formatted URLs` e `should ignore
//!   ill-formatted URLs`) e `detect SetWindowsFrindlyCwd sequence` (8 casos);
//! - `command tracking`: os 11 casos `should activate capability on ...`, `should pass ... to the capability`,
//!   `should not activate capability on the cwd sequence` e `should pass cwd sequence to the capability if it's
//!   initialized`;
//! - `BufferMarkCapability`: `SetMark`, `SetMark - ID`, `SetMark - hidden` e `SetMark - hidden & ID` (4 casos).
//!
//! Portados: `parseMarkSequence` (4), `deserializeMessage` (22), `serializeVSCodeOscMessage` (21) e
//! `parseKeyValueAssignment` (1), que só exercitam as funções puras de `shellIntegrationAddon.ts`.
//!
//! Divergências:
//! - **`cases.forEach(... test(title, ...))`.** Cada elemento do vetor de casos é um `#[test]` gerado pela macro
//!   `deserialize_case!`/`serialize_case!`, com o título no nome da função.
//! - **`Backslash`, `Newline` e `Semicolon`.** Os casos escrevem as barras invertidas diretamente em literais `r"..."` ou
//!   com `\\`, o que o original evita pelas constantes por clareza; os valores são os mesmos.
//! - **`deepEqual(parseMarkSequence(...), { id: undefined, hidden: false })`** é `assert_eq!` com [`MarkSequence`]
//!   (`id: None` é o `undefined`).

use je_platform::terminal::shell_integration_addon::{
    KeyValueAssignment, MarkSequence, deserialize_vscode_osc_message, parse_key_value_assignment, parse_mark_sequence, serialize_vscode_osc_message,
};

// ---------------------------------------------------------------------------------------------------------------------
// suite('BufferMarkCapability') > suite('parseMarkSequence')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn parse_mark_sequence_basic() {
    assert_eq!(parse_mark_sequence(&[Some(""), Some("")]), MarkSequence { id: None, hidden: false });
}

#[test]
fn parse_mark_sequence_id() {
    assert_eq!(parse_mark_sequence(&[Some("Id=3"), Some("")]), MarkSequence { id: Some("3".to_owned()), hidden: false });
}

#[test]
fn parse_mark_sequence_hidden() {
    assert_eq!(parse_mark_sequence(&[Some(""), Some("Hidden")]), MarkSequence { id: None, hidden: true });
}

#[test]
fn parse_mark_sequence_id_hidden() {
    assert_eq!(parse_mark_sequence(&[Some("Id=4555"), Some("Hidden")]), MarkSequence { id: Some("4555".to_owned()), hidden: true });
}

// ---------------------------------------------------------------------------------------------------------------------
// suite('deserializeMessage')
// ---------------------------------------------------------------------------------------------------------------------

/// `test(title, () => strictEqual(deserializeVSCodeOscMessage(input), expected))`.
macro_rules! deserialize_case {
    ($name:ident, $input:expr, $expected:expr) => {
        #[test]
        fn $name() {
            assert_eq!(deserialize_vscode_osc_message($input), $expected);
        }
    };
}

deserialize_case!(deserialize_message_empty, "", "");
deserialize_case!(deserialize_message_basic, "value", "value");
deserialize_case!(deserialize_message_space, "some thing", "some thing");
deserialize_case!(deserialize_message_escaped_backslash, r"\\", r"\");
deserialize_case!(deserialize_message_non_initial_escaped_backslash, r"foo\\", r"foo\");
deserialize_case!(deserialize_message_two_escaped_backslashes, r"\\\\", r"\\");
deserialize_case!(deserialize_message_escaped_backslash_amidst_text, r"Hello\\there", r"Hello\there");
deserialize_case!(deserialize_message_backslash_escaped_literally_and_as_hex, r"\\ is same as \x5c", r"\ is same as \");
deserialize_case!(deserialize_message_escaped_semicolon, r"\x3b", ";");
deserialize_case!(deserialize_message_non_initial_escaped_semicolon, r"foo\x3b", "foo;");
deserialize_case!(deserialize_message_escaped_semicolon_upper_hex, r"\x3B", ";");
deserialize_case!(deserialize_message_escaped_backslash_followed_by_literal_x3b_is_not_a_semicolon, r"\\x3b", r"\x3b");
deserialize_case!(deserialize_message_non_initial_escaped_backslash_followed_by_literal_x3b_is_not_a_semicolon, r"foo\\x3b", r"foo\x3b");
deserialize_case!(deserialize_message_escaped_backslash_followed_by_escaped_semicolon, r"\\\x3b", r"\;");
deserialize_case!(deserialize_message_escaped_semicolon_amidst_text, r"some\x3bthing", "some;thing");
deserialize_case!(deserialize_message_escaped_newline, r"\x0a", "\n");
deserialize_case!(deserialize_message_non_initial_escaped_newline, r"foo\x0a", "foo\n");
deserialize_case!(deserialize_message_escaped_newline_upper_hex, r"\x0A", "\n");
deserialize_case!(deserialize_message_escaped_backslash_followed_by_literal_x0a_is_not_a_newline, r"\\x0a", r"\x0a");
deserialize_case!(deserialize_message_non_initial_escaped_backslash_followed_by_literal_x0a_is_not_a_newline, r"foo\\x0a", r"foo\x0a");
deserialize_case!(deserialize_message_ps1_simple, r"[\u@\h \W]\$", r"[\u@\h \W]\$");
deserialize_case!(
    deserialize_message_ps1_vsc_si,
    r"\x1b]633;A\x07\[\x1b]0;\u@\h:\w\a\]\x1b]633;B\x07",
    "\x1b]633;A\x07\\[\x1b]0;\\u@\\h:\\w\\a\\]\x1b]633;B\x07"
);

// ---------------------------------------------------------------------------------------------------------------------
// suite('serializeVSCodeOscMessage')
// ---------------------------------------------------------------------------------------------------------------------

/// `test(title, () => strictEqual(serializeVSCodeOscMessage(input), expected))`.
macro_rules! serialize_case {
    ($name:ident, $input:expr, $expected:expr) => {
        #[test]
        fn $name() {
            assert_eq!(serialize_vscode_osc_message($input), $expected);
        }
    };
}

serialize_case!(serialize_vscode_osc_message_empty, "", "");
serialize_case!(serialize_vscode_osc_message_basic, "value", "value");
serialize_case!(serialize_vscode_osc_message_space, "some thing", r"some\x20thing");
serialize_case!(serialize_vscode_osc_message_backslash, r"\", r"\\");
serialize_case!(serialize_vscode_osc_message_non_initial_backslash, r"foo\", r"foo\\");
serialize_case!(serialize_vscode_osc_message_two_backslashes, r"\\", r"\\\\");
serialize_case!(serialize_vscode_osc_message_backslash_amidst_text, r"Hello\there", r"Hello\\there");
serialize_case!(serialize_vscode_osc_message_semicolon, ";", r"\x3b");
serialize_case!(serialize_vscode_osc_message_non_initial_semicolon, "foo;", r"foo\x3b");
serialize_case!(serialize_vscode_osc_message_semicolon_amidst_text, "some;thing", r"some\x3bthing");
serialize_case!(serialize_vscode_osc_message_newline, "\n", r"\x0a");
serialize_case!(serialize_vscode_osc_message_non_initial_newline, "foo\n", r"foo\x0a");
serialize_case!(serialize_vscode_osc_message_newline_amidst_text, "some\nthing", r"some\x0athing");
serialize_case!(serialize_vscode_osc_message_tab_character, "\t", r"\x09");
serialize_case!(serialize_vscode_osc_message_carriage_return, "\r", r"\x0d");
serialize_case!(serialize_vscode_osc_message_null_character, "\x00", r"\x00");
serialize_case!(serialize_vscode_osc_message_space_character_0x20, " ", r"\x20");
serialize_case!(serialize_vscode_osc_message_character_above_0x20, "!", "!");
serialize_case!(serialize_vscode_osc_message_multiple_special_chars, "hello\nworld;test\\end", r"hello\x0aworld\x3btest\\end");
serialize_case!(
    serialize_vscode_osc_message_ps1_with_escape_sequences,
    "\x1b]633;A\x07\\[\x1b]0;\\u@\\h:\\w\\a\\]\x1b]633;B\x07",
    r"\x1b]633\x3bA\x07\\[\x1b]0\x3b\\u@\\h:\\w\\a\\]\x1b]633\x3bB\x07"
);

// ---------------------------------------------------------------------------------------------------------------------
// test('parseKeyValueAssignment')
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn parse_key_value_assignment_cases() {
    // [title, input, [key, value]]
    let cases: [(&str, &str, (&str, Option<&str>)); 9] = [
        ("empty", "", ("", None)),
        ("no \"=\" sign", "some-text", ("some-text", None)),
        ("empty value", "key=", ("key", Some(""))),
        ("empty key", "=value", ("", Some("value"))),
        ("normal", "key=value", ("key", Some("value"))),
        ("multiple \"=\" signs (1)", "key==value", ("key", Some("=value"))),
        ("multiple \"=\" signs (2)", "key=value===true", ("key", Some("value===true"))),
        ("just a \"=\"", "=", ("", Some(""))),
        ("just a \"==\"", "==", ("", Some("="))),
    ];

    for (title, input, (key, value)) in cases {
        assert_eq!(parse_key_value_assignment(input), KeyValueAssignment { key: key.to_owned(), value: value.map(str::to_owned) }, "{title}");
    }
}
