//! Porte de `src/vs/workbench/services/keybinding/test/browser/keybindingIO.test.ts` (suíte `keybindingIO`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `createUSLayoutResolvedKeybinding` (do `keybindingsTestUtils.ts`) vira a função de mesmo nome aqui.
//! - `JSON.parse(...)` vira [`parse_json`], que devolve o [`Dynamic`] que o `readUserKeybindingItem` recebe.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

#[path = "../../je-platform/tests/common/keybinding.rs"]
#[allow(dead_code)]
mod platform_keybinding;

use platform_keybinding::create_us_layout_resolved_keybinding;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::json;
use je_base::key_codes::{KeyCode, KeyMod, ScanCode, key_chord};
use je_base::keybinding_parser::KeybindingParser;
use je_base::keybindings::{Chord, KeyCodeChord, Keybinding, ScanCodeChord, decode_keybinding};
use je_base::platform::OperatingSystem;
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use je_workbench::services::keybinding::keybinding_io::{KeybindingIO, OutputBuilder};


/// `JSON.parse`.
fn parse_json(text: &str) -> Dynamic {
    let mut errors = Vec::new();
    let value = json::parse_str(text, &mut errors).expect("JSON válido");
    assert!(errors.is_empty(), "JSON inválido: {text}");
    Dynamic::from_json(&value)
}

#[test]
fn serialize_deserialize() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    fn test_one_serialization(keybinding: u32, expected: &str, msg: &str, os: OperatingSystem) {
        let us_layout_resolved_keybinding = create_us_layout_resolved_keybinding(keybinding, os).unwrap();
        let actual_serialized = us_layout_resolved_keybinding.get_user_settings_label();
        assert_eq!(actual_serialized.as_deref(), Some(expected), "{expected} - {msg}");
    }
    fn test_serialization(keybinding: u32, expected_win: &str, expected_mac: &str, expected_linux: &str) {
        test_one_serialization(keybinding, expected_win, "win", OperatingSystem::Windows);
        test_one_serialization(keybinding, expected_mac, "mac", OperatingSystem::Macintosh);
        test_one_serialization(keybinding, expected_linux, "linux", OperatingSystem::Linux);
    }

    fn test_one_deserialization(keybinding: &str, expected: u32, msg: &str, os: OperatingSystem) {
        let actual_deserialized = KeybindingParser::parse_keybinding(keybinding);
        let expected = decode_keybinding(expected, os).unwrap();
        assert_eq!(actual_deserialized, expected, "{keybinding} - {msg}");
    }
    fn test_deserialization(in_win: &str, in_mac: &str, in_linux: &str, expected: u32) {
        test_one_deserialization(in_win, expected, "win", OperatingSystem::Windows);
        test_one_deserialization(in_mac, expected, "mac", OperatingSystem::Macintosh);
        test_one_deserialization(in_linux, expected, "linux", OperatingSystem::Linux);
    }

    fn test_roundtrip(keybinding: u32, expected_win: &str, expected_mac: &str, expected_linux: &str) {
        test_serialization(keybinding, expected_win, expected_mac, expected_linux);
        test_deserialization(expected_win, expected_mac, expected_linux, keybinding);
    }

    let (ctrl_cmd, shift, alt, win_ctrl) = (KeyMod::CTRL_CMD, KeyMod::SHIFT, KeyMod::ALT, KeyMod::WIN_CTRL);
    let key_a = KeyCode::KeyA.bits();

    test_roundtrip(KeyCode::Digit0.bits(), "0", "0", "0");
    test_roundtrip(key_a, "a", "a", "a");
    test_roundtrip(KeyCode::UpArrow.bits(), "up", "up", "up");
    test_roundtrip(KeyCode::RightArrow.bits(), "right", "right", "right");
    test_roundtrip(KeyCode::DownArrow.bits(), "down", "down", "down");
    test_roundtrip(KeyCode::LeftArrow.bits(), "left", "left", "left");

    // one modifier
    test_roundtrip(alt | key_a, "alt+a", "alt+a", "alt+a");
    test_roundtrip(ctrl_cmd | key_a, "ctrl+a", "cmd+a", "ctrl+a");
    test_roundtrip(shift | key_a, "shift+a", "shift+a", "shift+a");
    test_roundtrip(win_ctrl | key_a, "win+a", "ctrl+a", "meta+a");

    // two modifiers
    test_roundtrip(ctrl_cmd | alt | key_a, "ctrl+alt+a", "alt+cmd+a", "ctrl+alt+a");
    test_roundtrip(ctrl_cmd | shift | key_a, "ctrl+shift+a", "shift+cmd+a", "ctrl+shift+a");
    test_roundtrip(ctrl_cmd | win_ctrl | key_a, "ctrl+win+a", "ctrl+cmd+a", "ctrl+meta+a");
    test_roundtrip(shift | alt | key_a, "shift+alt+a", "shift+alt+a", "shift+alt+a");
    test_roundtrip(shift | win_ctrl | key_a, "shift+win+a", "ctrl+shift+a", "shift+meta+a");
    test_roundtrip(alt | win_ctrl | key_a, "alt+win+a", "ctrl+alt+a", "alt+meta+a");

    // three modifiers
    test_roundtrip(ctrl_cmd | shift | alt | key_a, "ctrl+shift+alt+a", "shift+alt+cmd+a", "ctrl+shift+alt+a");
    test_roundtrip(ctrl_cmd | shift | win_ctrl | key_a, "ctrl+shift+win+a", "ctrl+shift+cmd+a", "ctrl+shift+meta+a");
    test_roundtrip(shift | alt | win_ctrl | key_a, "shift+alt+win+a", "ctrl+shift+alt+a", "shift+alt+meta+a");

    // all modifiers
    test_roundtrip(
        ctrl_cmd | shift | alt | win_ctrl | key_a,
        "ctrl+shift+alt+win+a",
        "ctrl+shift+alt+cmd+a",
        "ctrl+shift+alt+meta+a",
    );

    // chords
    test_roundtrip(
        key_chord(ctrl_cmd | key_a, ctrl_cmd | key_a),
        "ctrl+a ctrl+a",
        "cmd+a cmd+a",
        "ctrl+a ctrl+a",
    );
    test_roundtrip(
        key_chord(ctrl_cmd | KeyCode::UpArrow.bits(), ctrl_cmd | KeyCode::UpArrow.bits()),
        "ctrl+up ctrl+up",
        "cmd+up cmd+up",
        "ctrl+up ctrl+up",
    );

    // OEM keys
    test_roundtrip(KeyCode::Semicolon.bits(), ";", ";", ";");
    test_roundtrip(KeyCode::Equal.bits(), "=", "=", "=");
    test_roundtrip(KeyCode::Comma.bits(), ",", ",", ",");
    test_roundtrip(KeyCode::Minus.bits(), "-", "-", "-");
    test_roundtrip(KeyCode::Period.bits(), ".", ".", ".");
    test_roundtrip(KeyCode::Slash.bits(), "/", "/", "/");
    test_roundtrip(KeyCode::Backquote.bits(), "`", "`", "`");
    test_roundtrip(KeyCode::AbntC1.bits(), "abnt_c1", "abnt_c1", "abnt_c1");
    test_roundtrip(KeyCode::AbntC2.bits(), "abnt_c2", "abnt_c2", "abnt_c2");
    test_roundtrip(KeyCode::BracketLeft.bits(), "[", "[", "[");
    test_roundtrip(KeyCode::Backslash.bits(), "\\", "\\", "\\");
    test_roundtrip(KeyCode::BracketRight.bits(), "]", "]", "]");
    test_roundtrip(KeyCode::Quote.bits(), "'", "'", "'");
    test_roundtrip(KeyCode::Oem8.bits(), "oem_8", "oem_8", "oem_8");
    test_roundtrip(KeyCode::IntlBackslash.bits(), "oem_102", "oem_102", "oem_102");

    // OEM aliases
    test_deserialization("OEM_1", "OEM_1", "OEM_1", KeyCode::Semicolon.bits());
    test_deserialization("OEM_PLUS", "OEM_PLUS", "OEM_PLUS", KeyCode::Equal.bits());
    test_deserialization("OEM_COMMA", "OEM_COMMA", "OEM_COMMA", KeyCode::Comma.bits());
    test_deserialization("OEM_MINUS", "OEM_MINUS", "OEM_MINUS", KeyCode::Minus.bits());
    test_deserialization("OEM_PERIOD", "OEM_PERIOD", "OEM_PERIOD", KeyCode::Period.bits());
    test_deserialization("OEM_2", "OEM_2", "OEM_2", KeyCode::Slash.bits());
    test_deserialization("OEM_3", "OEM_3", "OEM_3", KeyCode::Backquote.bits());
    test_deserialization("ABNT_C1", "ABNT_C1", "ABNT_C1", KeyCode::AbntC1.bits());
    test_deserialization("ABNT_C2", "ABNT_C2", "ABNT_C2", KeyCode::AbntC2.bits());
    test_deserialization("OEM_4", "OEM_4", "OEM_4", KeyCode::BracketLeft.bits());
    test_deserialization("OEM_5", "OEM_5", "OEM_5", KeyCode::Backslash.bits());
    test_deserialization("OEM_6", "OEM_6", "OEM_6", KeyCode::BracketRight.bits());
    test_deserialization("OEM_7", "OEM_7", "OEM_7", KeyCode::Quote.bits());
    test_deserialization("OEM_8", "OEM_8", "OEM_8", KeyCode::Oem8.bits());
    test_deserialization("OEM_102", "OEM_102", "OEM_102", KeyCode::IntlBackslash.bits());

    // accepts '-' as separator
    test_deserialization(
        "ctrl-shift-alt-win-a",
        "ctrl-shift-alt-cmd-a",
        "ctrl-shift-alt-meta-a",
        ctrl_cmd | shift | alt | win_ctrl | key_a,
    );

    // various input mistakes
    test_deserialization(
        " ctrl-shift-alt-win-A ",
        " shift-alt-cmd-Ctrl-A ",
        " ctrl-shift-alt-META-A ",
        ctrl_cmd | shift | alt | win_ctrl | key_a,
    );
}

#[test]
fn deserialize_scan_codes() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        KeybindingParser::parse_keybinding("ctrl+shift+[comma] ctrl+/"),
        Some(
            Keybinding::new(vec![
                Chord::ScanCode(ScanCodeChord::new(true, true, false, false, ScanCode::Comma)),
                Chord::KeyCode(KeyCodeChord::new(true, false, false, false, KeyCode::Slash)),
            ])
            .unwrap()
        )
    );
}

#[test]
fn issue_10452_invalid_command() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let str_json = r#"[{ "key": "ctrl+k ctrl+f", "command": ["firstcommand", "seccondcommand"] }]"#;
    let user_keybinding = parse_json(str_json).get_str("0");
    let keybinding_item = KeybindingIO::read_user_keybinding_item(&user_keybinding);
    assert_eq!(keybinding_item.command, None);
}

#[test]
fn issue_10452_invalid_when() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let str_json = r#"[{ "key": "ctrl+k ctrl+f", "command": "firstcommand", "when": [] }]"#;
    let user_keybinding = parse_json(str_json).get_str("0");
    let keybinding_item = KeybindingIO::read_user_keybinding_item(&user_keybinding);
    assert!(keybinding_item.when.is_none());
}

#[test]
fn issue_10452_invalid_key() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let str_json = r#"[{ "key": [], "command": "firstcommand" }]"#;
    let user_keybinding = parse_json(str_json).get_str("0");
    let keybinding_item = KeybindingIO::read_user_keybinding_item(&user_keybinding);
    assert_eq!(keybinding_item.keybinding, None);
}

#[test]
fn issue_10452_invalid_key_2() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let str_json = r#"[{ "key": "", "command": "firstcommand" }]"#;
    let user_keybinding = parse_json(str_json).get_str("0");
    let keybinding_item = KeybindingIO::read_user_keybinding_item(&user_keybinding);
    assert_eq!(keybinding_item.keybinding, None);
}

#[test]
fn test_commands_args() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let str_json = r#"[{ "key": "ctrl+k ctrl+f", "command": "firstcommand", "when": [], "args": { "text": "theText" } }]"#;
    let user_keybinding = parse_json(str_json).get_str("0");
    let keybinding_item = KeybindingIO::read_user_keybinding_item(&user_keybinding);
    assert_eq!(keybinding_item.command_args.get_str("text"), Dynamic::string("theText"));
}

#[test]
fn system_wide_read_defaults_to_false_when_absent_or_invalid() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let absent = KeybindingIO::read_user_keybinding_item(&parse_json(r#"{ "key": "ctrl+cmd+a", "command": "firstcommand" }"#));
    assert!(!absent.system_wide);

    let invalid = KeybindingIO::read_user_keybinding_item(&parse_json(
        r#"{ "key": "ctrl+cmd+a", "command": "firstcommand", "systemWide": "yes" }"#,
    ));
    assert!(!invalid.system_wide);
}

#[test]
fn system_wide_read_parses_boolean_value() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let item = KeybindingIO::read_user_keybinding_item(&parse_json(
        r#"{ "key": "ctrl+cmd+a", "command": "firstcommand", "systemWide": true }"#,
    ));
    assert!(item.system_wide);
}

#[test]
fn system_wide_write_export_preserves_the_flag_roundtrip() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let resolved_keybinding =
        create_us_layout_resolved_keybinding(KeyMod::CTRL_CMD | KeyMod::WIN_CTRL | KeyCode::KeyA.bits(), OperatingSystem::Macintosh)
            .unwrap();
    let item = ResolvedKeybindingItem::new(
        Some(resolved_keybinding),
        Some("workbench.action.openAgentsWindow"),
        Dynamic::Undefined,
        None,
        false,
        None,
        false,
        /* system_wide */ true,
    );

    let mut out = OutputBuilder::new();
    KeybindingIO::write_keybinding_item(&mut out, &item);
    let serialized = out.to_string();
    assert!(serialized.contains("\"systemWide\": true"), "expected serialized keybinding to include systemWide, got: {serialized}");

    let read_back = KeybindingIO::read_user_keybinding_item(&parse_json(&serialized));
    assert!(read_back.system_wide);
    assert_eq!(read_back.command.as_deref(), Some("workbench.action.openAgentsWindow"));
}
