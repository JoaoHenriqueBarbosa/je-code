//! Porte de `src/vs/workbench/services/keybinding/test/node/macLinuxKeyboardMapper.test.ts` (85 casos), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - Cada `suite` do original vira um módulo. O `suiteSetup` (um mapeador por suíte) vira a função `mapper()` do
//!   módulo, chamada em cada caso: montar o mapeador é barato.
//! - O literal `IResolvedKeybinding` do original vira [`expected`] (e [`ctrl_chord`] para um acorde só), e o
//!   `IKeyboardEvent` vira [`event`], para o corpo de cada caso mostrar só o que varia (o `_standardKeyboardEventBrand`
//!   não tem equivalente). O `keyCode: -1` dos eventos por escancode vira [`code_event`].
//! - `readRawMapping` lê o `.js` do layout pelo `support::js_literal`, que aceita as chaves `valueIsDeadKey` e parentes
//!   dos layouts do Mac (o mapeador as ignora, como o original).
//! - Os mapas escritos em linha no original (`{ 'Backquote': {...} }` e `{}`) viram vetores de pares na ordem do literal.
//! - `assertKeybindingTranslation` do original compara os rótulos de configuração do usuário dos acordes por escancode;
//!   o rótulo do atalho de teclas só entra na mensagem da asserção, por isso não tem porte.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::key_codes::{KeyCode, KeyMod, ScanCode, ScanCodeUtils, key_chord};
use je_base::keybinding_labels::ModifierLabelProvider;
use je_base::keybindings::{Keybinding, SingleModifierChord, create_simple_keybinding, decode_keybinding};
use je_base::platform::OperatingSystem;
use je_platform::keybinding::keybinding::KeyboardEvent;
use je_platform::keyboard_layout::MacLinuxKeyMapping;
use je_workbench::services::keybinding::mac_linux_keyboard_mapper::MacLinuxKeyboardMapper;
use support::js_literal::read_raw_entries;
use support::keyboard_mapper_test_utils::{
    ResolvedKeybindingSnapshot, assert_mapping, assert_resolve_keybinding, assert_resolve_keyboard_event, ctrl_chord, expected,
    key_code_chord, scan_code_chord,
};

const WRITE_FILE_IF_DIFFERENT: bool = false;

/// `readRawMapping<IMacLinuxKeyboardMapping>(file)`: o mapa na ordem em que o arquivo o declara.
fn read_raw_mapping(file: &str) -> Vec<(String, MacLinuxKeyMapping)> {
    read_raw_entries(file)
        .into_iter()
        .map(|entry| {
            let mapping = MacLinuxKeyMapping::new(
                &entry.text("value"),
                &entry.text("withShift"),
                &entry.text("withAltGr"),
                &entry.text("withShiftAltGr"),
            );
            (entry.code, mapping)
        })
        .collect()
}

fn new_mapper(
    is_us_standard: bool,
    raw_mappings: &[(String, MacLinuxKeyMapping)],
    map_alt_gr_to_ctrl_alt: bool,
    os: OperatingSystem,
) -> MacLinuxKeyboardMapper {
    MacLinuxKeyboardMapper::new(is_us_standard, raw_mappings.iter().map(|(code, mapping)| (code, mapping)), map_alt_gr_to_ctrl_alt, os)
}

/// `createKeyboardMapper`.
fn create_keyboard_mapper(is_us_standard: bool, file: &str, map_alt_gr_to_ctrl_alt: bool, os: OperatingSystem) -> MacLinuxKeyboardMapper {
    new_mapper(is_us_standard, &read_raw_mapping(file), map_alt_gr_to_ctrl_alt, os)
}

/// `_assertKeybindingTranslation`.
fn assert_keybinding_translation(mapper: &MacLinuxKeyboardMapper, os: OperatingSystem, kb: u32, expected: &[&str]) {
    let runtime_keybinding = create_simple_keybinding(kb, os);
    let provider = ModifierLabelProvider::user_settings();
    let actual: Vec<String> = mapper
        .key_code_chord_to_scan_code_chord(&runtime_keybinding)
        .iter()
        .map(|chord| {
            provider
                .to_label(os, std::slice::from_ref(chord), |keybinding| Some(ScanCodeUtils::to_string(keybinding.scan_code).to_string()))
                .expect("o rótulo por escancode sempre existe")
        })
        .collect();
    assert_eq!(actual, expected);
}

/// `_assertResolveKeybinding`.
fn assert_binary_resolve_keybinding(mapper: &MacLinuxKeyboardMapper, os: OperatingSystem, k: u32, expected: &[ResolvedKeybindingSnapshot]) {
    let keybinding = decode_keybinding(k, os).unwrap().unwrap();
    assert_resolve_keybinding(mapper, &keybinding, expected);
}

/// Um `IKeyboardEvent` na ordem `ctrl, shift, alt, meta, altGraph, keyCode, code` do original.
fn event(ctrl: bool, shift: bool, alt: bool, meta: bool, alt_gr: bool, key_code: KeyCode, code: &str) -> KeyboardEvent {
    KeyboardEvent {
        ctrl_key: ctrl,
        shift_key: shift,
        alt_key: alt,
        meta_key: meta,
        alt_graph_key: alt_gr,
        key_code,
        code: Some(code.to_string()),
    }
}

/// Um `IKeyboardEvent` com `keyCode: -1`, que o mapeador resolve só pelo `code`.
fn code_event(ctrl: bool, shift: bool, alt: bool, meta: bool, alt_gr: bool, code: &str) -> KeyboardEvent {
    event(ctrl, shift, alt, meta, alt_gr, KeyCode(-1), code)
}

/// O evento de uma tecla sem modificadores (`assertNumpadKeyboardEvent` e `assertKeyboardEvent` do original, que só
/// diferem no tipo do acelerador).
fn assert_plain_key_event(
    mapper: &MacLinuxKeyboardMapper,
    key_code: KeyCode,
    code: &str,
    label: &str,
    electron_accelerator: Option<&str>,
    user_settings_label: &str,
    dispatch: &str,
) {
    assert_resolve_keyboard_event(
        mapper,
        &event(false, false, false, false, false, key_code, code),
        &ctrl_chord(label, label, electron_accelerator, user_settings_label, true, dispatch),
    );
}

/// O `IResolvedKeybinding` de uma tecla modificadora sozinha.
fn single_modifier(label: &str, aria_label: &str, user_settings_label: &str, chord: SingleModifierChord) -> ResolvedKeybindingSnapshot {
    expected(label, aria_label, None, user_settings_label, true, false, &[None], &[Some(chord)])
}

mod mac_de_ch {
    use super::*;

    const OS: OperatingSystem = OperatingSystem::Macintosh;

    fn mapper() -> MacLinuxKeyboardMapper {
        create_keyboard_mapper(false, "mac_de_ch", false, OS)
    }

    fn translate(kb: u32, expected: &[&str]) {
        assert_keybinding_translation(&mapper(), OS, kb, expected);
    }

    fn resolve(k: u32, expected: &[ResolvedKeybindingSnapshot]) {
        assert_binary_resolve_keybinding(&mapper(), OS, k, expected);
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "mac_de_ch.txt");
    }

    #[test]
    fn kb_to_hw() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // unchanged
        translate(KeyMod::CTRL_CMD | KeyCode::Digit1.bits(), &["cmd+Digit1"]);
        translate(KeyMod::CTRL_CMD | KeyCode::KeyB.bits(), &["cmd+KeyB"]);
        translate(KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyCode::KeyB.bits(), &["shift+cmd+KeyB"]);
        translate(KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyMod::ALT | KeyMod::WIN_CTRL | KeyCode::KeyB.bits(), &["ctrl+shift+alt+cmd+KeyB"]);

        // flips Y and Z
        translate(KeyMod::CTRL_CMD | KeyCode::KeyZ.bits(), &["cmd+KeyY"]);
        translate(KeyMod::CTRL_CMD | KeyCode::KeyY.bits(), &["cmd+KeyZ"]);

        // Ctrl+/
        translate(KeyMod::CTRL_CMD | KeyCode::Slash.bits(), &["shift+cmd+Digit7"]);
    }

    #[test]
    fn resolve_keybinding_cmd_a() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(KeyMod::CTRL_CMD | KeyCode::KeyA.bits(), &[ctrl_chord("⌘A", "Command+A", Some("Cmd+A"), "cmd+a", true, "meta+[KeyA]")]);
    }

    #[test]
    fn resolve_keybinding_cmd_b() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(KeyMod::CTRL_CMD | KeyCode::KeyB.bits(), &[ctrl_chord("⌘B", "Command+B", Some("Cmd+B"), "cmd+b", true, "meta+[KeyB]")]);
    }

    #[test]
    fn resolve_keybinding_cmd_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(KeyMod::CTRL_CMD | KeyCode::KeyZ.bits(), &[ctrl_chord("⌘Z", "Command+Z", Some("Cmd+Z"), "cmd+z", true, "meta+[KeyY]")]);
    }

    #[test]
    fn resolve_keyboard_event_cmd_key_y() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(false, false, false, true, false, "KeyY"),
            &ctrl_chord("⌘Z", "Command+Z", Some("Cmd+Z"), "cmd+z", true, "meta+[KeyY]"),
        );
    }

    #[test]
    fn resolve_keybinding_cmd_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::BracketRight.bits(),
            &[ctrl_chord("⌃⌥⌘6", "Control+Option+Command+6", Some("Ctrl+Alt+Cmd+6"), "ctrl+alt+cmd+6", true, "ctrl+alt+meta+[Digit6]")],
        );
    }

    #[test]
    fn resolve_keyboard_event_cmd_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(false, false, false, true, false, "BracketRight"),
            &ctrl_chord("⌘¨", "Command+¨", None, "cmd+[BracketRight]", false, "meta+[BracketRight]"),
        );
    }

    #[test]
    fn resolve_keybinding_shift_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::SHIFT | KeyCode::BracketRight.bits(),
            &[ctrl_chord("⌃⌥9", "Control+Option+9", Some("Ctrl+Alt+9"), "ctrl+alt+9", true, "ctrl+alt+[Digit9]")],
        );
    }

    #[test]
    fn resolve_keybinding_cmd_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::Slash.bits(),
            &[ctrl_chord("⇧⌘7", "Shift+Command+7", Some("Shift+Cmd+7"), "shift+cmd+7", true, "shift+meta+[Digit7]")],
        );
    }

    #[test]
    fn resolve_keybinding_cmd_shift_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyCode::Slash.bits(),
            &[ctrl_chord("⇧⌘'", "Shift+Command+'", None, "shift+cmd+[Minus]", false, "shift+meta+[Minus]")],
        );
    }

    #[test]
    fn resolve_keybinding_cmd_k_cmd_backslash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Backslash.bits()),
            &[expected(
                "⌘K ⌃⇧⌥⌘7",
                "Command+K Control+Shift+Option+Command+7",
                None,
                "cmd+k ctrl+shift+alt+cmd+7",
                true,
                true,
                &[Some("meta+[KeyK]"), Some("ctrl+shift+alt+meta+[Digit7]")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keybinding_cmd_k_cmd_equal() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Equal.bits()),
            &[expected(
                "⌘K ⇧⌘0",
                "Command+K Shift+Command+0",
                None,
                "cmd+k shift+cmd+0",
                true,
                true,
                &[Some("meta+[KeyK]"), Some("shift+meta+[Digit0]")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keybinding_cmd_down_arrow() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::DownArrow.bits(),
            &[ctrl_chord("⌘↓", "Command+DownArrow", Some("Cmd+Down"), "cmd+down", true, "meta+[ArrowDown]")],
        );
    }

    #[test]
    fn resolve_keybinding_cmd_numpad_0() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::Numpad0.bits(),
            &[ctrl_chord("⌘NumPad0", "Command+NumPad0", None, "cmd+numpad0", true, "meta+[Numpad0]")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_home() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::Home.bits(),
            &[ctrl_chord("⌘Home", "Command+Home", Some("Cmd+Home"), "cmd+home", true, "meta+[Home]")],
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_home() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(false, false, false, true, false, "Home"),
            &ctrl_chord("⌘Home", "Command+Home", Some("Cmd+Home"), "cmd+home", true, "meta+[Home]"),
        );
    }

    #[test]
    fn resolve_user_binding_cmd_comma_cmd_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keybinding(
            &mapper(),
            &Keybinding::new(vec![
                scan_code_chord(false, false, false, true, ScanCode::Comma),
                key_code_chord(false, false, false, true, KeyCode::Slash),
            ])
            .unwrap(),
            &[expected(
                "⌘, ⇧⌘7",
                "Command+, Shift+Command+7",
                None,
                "cmd+[Comma] shift+cmd+7",
                false,
                true,
                &[Some("meta+[Comma]"), Some("shift+meta+[Digit7]")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_meta_left() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(false, false, false, true, false, "MetaLeft"),
            &single_modifier("⌘", "Command", "cmd", SingleModifierChord::Meta),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_meta_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(false, false, false, true, false, "MetaRight"),
            &single_modifier("⌘", "Command", "cmd", SingleModifierChord::Meta),
        );
    }
}

mod mac_en_us {
    use super::*;

    const OS: OperatingSystem = OperatingSystem::Macintosh;

    fn mapper() -> MacLinuxKeyboardMapper {
        create_keyboard_mapper(true, "mac_en_us", false, OS)
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "mac_en_us.txt");
    }

    #[test]
    fn resolve_user_binding_cmd_comma_cmd_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keybinding(
            &mapper(),
            &Keybinding::new(vec![
                scan_code_chord(false, false, false, true, ScanCode::Comma),
                key_code_chord(false, false, false, true, KeyCode::Slash),
            ])
            .unwrap(),
            &[expected(
                "⌘, ⌘/",
                "Command+, Command+/",
                None,
                "cmd+, cmd+/",
                true,
                true,
                &[Some("meta+[Comma]"), Some("meta+[Slash]")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_meta_left() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(false, false, false, true, false, "MetaLeft"),
            &single_modifier("⌘", "Command", "cmd", SingleModifierChord::Meta),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_meta_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(false, false, false, true, false, "MetaRight"),
            &single_modifier("⌘", "Command", "cmd", SingleModifierChord::Meta),
        );
    }

    #[test]
    fn resolve_keyboard_event_map_alt_gr_to_ctrl_alt_alt_gr_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mapper = create_keyboard_mapper(true, "mac_en_us", true, OS);

        assert_resolve_keyboard_event(
            &mapper,
            &code_event(false, false, false, false, true, "KeyZ"),
            &ctrl_chord("⌃⌥Z", "Control+Option+Z", Some("Ctrl+Alt+Z"), "ctrl+alt+z", true, "ctrl+alt+[KeyZ]"),
        );
    }
}

mod linux_de_ch {
    use super::*;

    const OS: OperatingSystem = OperatingSystem::Linux;

    fn mapper() -> MacLinuxKeyboardMapper {
        create_keyboard_mapper(false, "linux_de_ch", false, OS)
    }

    fn translate(kb: u32, expected: &[&str]) {
        assert_keybinding_translation(&mapper(), OS, kb, expected);
    }

    fn resolve(k: u32, expected: &[ResolvedKeybindingSnapshot]) {
        assert_binary_resolve_keybinding(&mapper(), OS, k, expected);
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "linux_de_ch.txt");
    }

    #[test]
    fn kb_to_hw() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // unchanged
        translate(KeyMod::CTRL_CMD | KeyCode::Digit1.bits(), &["ctrl+Digit1"]);
        translate(KeyMod::CTRL_CMD | KeyCode::KeyB.bits(), &["ctrl+KeyB"]);
        translate(KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyCode::KeyB.bits(), &["ctrl+shift+KeyB"]);
        translate(KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyMod::ALT | KeyMod::WIN_CTRL | KeyCode::KeyB.bits(), &["ctrl+shift+alt+meta+KeyB"]);

        // flips Y and Z
        translate(KeyMod::CTRL_CMD | KeyCode::KeyZ.bits(), &["ctrl+KeyY"]);
        translate(KeyMod::CTRL_CMD | KeyCode::KeyY.bits(), &["ctrl+KeyZ"]);

        // Ctrl+/
        translate(KeyMod::CTRL_CMD | KeyCode::Slash.bits(), &["ctrl+shift+Digit7"]);
    }

    #[test]
    fn resolve_keybinding_ctrl_a() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(KeyMod::CTRL_CMD | KeyCode::KeyA.bits(), &[ctrl_chord("Ctrl+A", "Control+A", Some("Ctrl+A"), "ctrl+a", true, "ctrl+[KeyA]")]);
    }

    #[test]
    fn resolve_keybinding_ctrl_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(KeyMod::CTRL_CMD | KeyCode::KeyZ.bits(), &[ctrl_chord("Ctrl+Z", "Control+Z", Some("Ctrl+Z"), "ctrl+z", true, "ctrl+[KeyY]")]);
    }

    #[test]
    fn resolve_keyboard_event_ctrl_key_y() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, false, false, false, false, "KeyY"),
            &ctrl_chord("Ctrl+Z", "Control+Z", Some("Ctrl+Z"), "ctrl+z", true, "ctrl+[KeyY]"),
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(KeyMod::CTRL_CMD | KeyCode::BracketRight.bits(), &[]);
    }

    #[test]
    fn resolve_keyboard_event_ctrl_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, false, false, false, false, "BracketRight"),
            &ctrl_chord("Ctrl+¨", "Control+¨", None, "ctrl+[BracketRight]", false, "ctrl+[BracketRight]"),
        );
    }

    #[test]
    fn resolve_keybinding_shift_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::SHIFT | KeyCode::BracketRight.bits(),
            &[
                ctrl_chord("Ctrl+Alt+0", "Control+Alt+0", Some("Ctrl+Alt+0"), "ctrl+alt+0", true, "ctrl+alt+[Digit0]"),
                ctrl_chord("Ctrl+Alt+$", "Control+Alt+$", None, "ctrl+alt+[Backslash]", false, "ctrl+alt+[Backslash]"),
            ],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::Slash.bits(),
            &[ctrl_chord("Ctrl+Shift+7", "Control+Shift+7", Some("Ctrl+Shift+7"), "ctrl+shift+7", true, "ctrl+shift+[Digit7]")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_shift_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyCode::Slash.bits(),
            &[ctrl_chord("Ctrl+Shift+'", "Control+Shift+'", None, "ctrl+shift+[Minus]", false, "ctrl+shift+[Minus]")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_k_ctrl_backslash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Backslash.bits()), &[]);
    }

    #[test]
    fn resolve_keybinding_ctrl_k_ctrl_equal() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Equal.bits()),
            &[expected(
                "Ctrl+K Ctrl+Shift+0",
                "Control+K Control+Shift+0",
                None,
                "ctrl+k ctrl+shift+0",
                true,
                true,
                &[Some("ctrl+[KeyK]"), Some("ctrl+shift+[Digit0]")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_down_arrow() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::DownArrow.bits(),
            &[ctrl_chord("Ctrl+DownArrow", "Control+DownArrow", Some("Ctrl+Down"), "ctrl+down", true, "ctrl+[ArrowDown]")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_numpad_0() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::Numpad0.bits(),
            &[ctrl_chord("Ctrl+NumPad0", "Control+NumPad0", None, "ctrl+numpad0", true, "ctrl+[Numpad0]")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_home() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::Home.bits(),
            &[ctrl_chord("Ctrl+Home", "Control+Home", Some("Ctrl+Home"), "ctrl+home", true, "ctrl+[Home]")],
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_home() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, false, false, false, false, "Home"),
            &ctrl_chord("Ctrl+Home", "Control+Home", Some("Ctrl+Home"), "ctrl+home", true, "ctrl+[Home]"),
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_key_x() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, false, false, false, false, "KeyX"),
            &ctrl_chord("Ctrl+X", "Control+X", Some("Ctrl+X"), "ctrl+x", true, "ctrl+[KeyX]"),
        );
    }

    #[test]
    fn resolve_user_binding_ctrl_comma_ctrl_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keybinding(
            &mapper(),
            &Keybinding::new(vec![
                scan_code_chord(true, false, false, false, ScanCode::Comma),
                key_code_chord(true, false, false, false, KeyCode::Slash),
            ])
            .unwrap(),
            &[expected(
                "Ctrl+, Ctrl+Shift+7",
                "Control+, Control+Shift+7",
                None,
                "ctrl+[Comma] ctrl+shift+7",
                false,
                true,
                &[Some("ctrl+[Comma]"), Some("ctrl+shift+[Digit7]")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_control_left() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, false, false, false, false, "ControlLeft"),
            &single_modifier("Ctrl", "Control", "ctrl", SingleModifierChord::Ctrl),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_control_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, false, false, false, false, "ControlRight"),
            &single_modifier("Ctrl", "Control", "ctrl", SingleModifierChord::Ctrl),
        );
    }
}

mod linux_en_us {
    use super::*;

    const OS: OperatingSystem = OperatingSystem::Linux;

    fn mapper() -> MacLinuxKeyboardMapper {
        create_keyboard_mapper(true, "linux_en_us", false, OS)
    }

    fn resolve(k: u32, expected: &[ResolvedKeybindingSnapshot]) {
        assert_binary_resolve_keybinding(&mapper(), OS, k, expected);
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "linux_en_us.txt");
    }

    #[test]
    fn resolve_keybinding_ctrl_a() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(KeyMod::CTRL_CMD | KeyCode::KeyA.bits(), &[ctrl_chord("Ctrl+A", "Control+A", Some("Ctrl+A"), "ctrl+a", true, "ctrl+[KeyA]")]);
    }

    #[test]
    fn resolve_keybinding_ctrl_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(KeyMod::CTRL_CMD | KeyCode::KeyZ.bits(), &[ctrl_chord("Ctrl+Z", "Control+Z", Some("Ctrl+Z"), "ctrl+z", true, "ctrl+[KeyZ]")]);
    }

    #[test]
    fn resolve_keyboard_event_ctrl_key_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, false, false, false, false, "KeyZ"),
            &ctrl_chord("Ctrl+Z", "Control+Z", Some("Ctrl+Z"), "ctrl+z", true, "ctrl+[KeyZ]"),
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::BracketRight.bits(),
            &[ctrl_chord("Ctrl+]", "Control+]", Some("Ctrl+]"), "ctrl+]", true, "ctrl+[BracketRight]")],
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, false, false, false, false, "BracketRight"),
            &ctrl_chord("Ctrl+]", "Control+]", Some("Ctrl+]"), "ctrl+]", true, "ctrl+[BracketRight]"),
        );
    }

    #[test]
    fn resolve_keybinding_shift_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::SHIFT | KeyCode::BracketRight.bits(),
            &[ctrl_chord("Shift+]", "Shift+]", Some("Shift+]"), "shift+]", true, "shift+[BracketRight]")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(KeyMod::CTRL_CMD | KeyCode::Slash.bits(), &[ctrl_chord("Ctrl+/", "Control+/", Some("Ctrl+/"), "ctrl+/", true, "ctrl+[Slash]")]);
    }

    #[test]
    fn resolve_keybinding_ctrl_shift_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyCode::Slash.bits(),
            &[ctrl_chord("Ctrl+Shift+/", "Control+Shift+/", Some("Ctrl+Shift+/"), "ctrl+shift+/", true, "ctrl+shift+[Slash]")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_k_ctrl_backslash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Backslash.bits()),
            &[expected(
                "Ctrl+K Ctrl+\\",
                "Control+K Control+\\",
                None,
                "ctrl+k ctrl+\\",
                true,
                true,
                &[Some("ctrl+[KeyK]"), Some("ctrl+[Backslash]")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_k_ctrl_equal() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Equal.bits()),
            &[expected(
                "Ctrl+K Ctrl+=",
                "Control+K Control+=",
                None,
                "ctrl+k ctrl+=",
                true,
                true,
                &[Some("ctrl+[KeyK]"), Some("ctrl+[Equal]")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_down_arrow() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::DownArrow.bits(),
            &[ctrl_chord("Ctrl+DownArrow", "Control+DownArrow", Some("Ctrl+Down"), "ctrl+down", true, "ctrl+[ArrowDown]")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_numpad_0() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::Numpad0.bits(),
            &[ctrl_chord("Ctrl+NumPad0", "Control+NumPad0", None, "ctrl+numpad0", true, "ctrl+[Numpad0]")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_home() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::Home.bits(),
            &[ctrl_chord("Ctrl+Home", "Control+Home", Some("Ctrl+Home"), "ctrl+home", true, "ctrl+[Home]")],
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_home() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, false, false, false, false, "Home"),
            &ctrl_chord("Ctrl+Home", "Control+Home", Some("Ctrl+Home"), "ctrl+home", true, "ctrl+[Home]"),
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_shift_comma() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyCode::Comma.bits(),
            &[
                ctrl_chord("Ctrl+Shift+,", "Control+Shift+,", Some("Ctrl+Shift+,"), "ctrl+shift+,", true, "ctrl+shift+[Comma]"),
                ctrl_chord("Ctrl+<", "Control+<", None, "ctrl+[IntlBackslash]", false, "ctrl+[IntlBackslash]"),
            ],
        );
    }

    #[test]
    fn issue_23393_resolve_keybinding_ctrl_enter() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        resolve(
            KeyMod::CTRL_CMD | KeyCode::Enter.bits(),
            &[ctrl_chord("Ctrl+Enter", "Control+Enter", Some("Ctrl+Enter"), "ctrl+enter", true, "ctrl+[Enter]")],
        );
    }

    #[test]
    fn issue_23393_resolve_keyboard_event_ctrl_numpad_enter() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, false, false, false, false, "NumpadEnter"),
            &ctrl_chord("Ctrl+Enter", "Control+Enter", Some("Ctrl+Enter"), "ctrl+enter", true, "ctrl+[Enter]"),
        );
    }

    #[test]
    fn resolve_user_binding_ctrl_comma_ctrl_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keybinding(
            &mapper(),
            &Keybinding::new(vec![
                scan_code_chord(true, false, false, false, ScanCode::Comma),
                key_code_chord(true, false, false, false, KeyCode::Slash),
            ])
            .unwrap(),
            &[expected(
                "Ctrl+, Ctrl+/",
                "Control+, Control+/",
                None,
                "ctrl+, ctrl+/",
                true,
                true,
                &[Some("ctrl+[Comma]"), Some("ctrl+[Slash]")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_user_binding_ctrl_comma() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keybinding(
            &mapper(),
            &Keybinding::new(vec![scan_code_chord(true, false, false, false, ScanCode::Comma)]).unwrap(),
            &[ctrl_chord("Ctrl+,", "Control+,", Some("Ctrl+,"), "ctrl+,", true, "ctrl+[Comma]")],
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_control_left() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, false, false, false, false, "ControlLeft"),
            &single_modifier("Ctrl", "Control", "ctrl", SingleModifierChord::Ctrl),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_control_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, false, false, false, false, "ControlRight"),
            &single_modifier("Ctrl", "Control", "ctrl", SingleModifierChord::Ctrl),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_shift_left() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(false, true, false, false, false, "ShiftLeft"),
            &single_modifier("Shift", "Shift", "shift", SingleModifierChord::Shift),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_shift_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(false, true, false, false, false, "ShiftRight"),
            &single_modifier("Shift", "Shift", "shift", SingleModifierChord::Shift),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_alt_left() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(false, false, true, false, false, "AltLeft"),
            &single_modifier("Alt", "Alt", "alt", SingleModifierChord::Alt),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_alt_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(false, false, true, false, false, "AltRight"),
            &single_modifier("Alt", "Alt", "alt", SingleModifierChord::Alt),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_meta_left() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(false, false, false, true, false, "MetaLeft"),
            &single_modifier("Super", "Super", "meta", SingleModifierChord::Meta),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_meta_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(false, false, false, true, false, "MetaRight"),
            &single_modifier("Super", "Super", "meta", SingleModifierChord::Meta),
        );
    }

    #[test]
    fn resolve_keyboard_event_only_modifiers_ctrl_shift() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, true, false, false, false, "ShiftLeft"),
            &expected("Ctrl+Shift", "Control+Shift", None, "ctrl+shift", true, false, &[None], &[None]),
        );
    }

    #[test]
    fn resolve_keyboard_event_map_alt_gr_to_ctrl_alt_alt_gr_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mapper = create_keyboard_mapper(true, "linux_en_us", true, OS);

        assert_resolve_keyboard_event(
            &mapper,
            &code_event(false, false, false, false, true, "KeyZ"),
            &ctrl_chord("Ctrl+Alt+Z", "Control+Alt+Z", Some("Ctrl+Alt+Z"), "ctrl+alt+z", true, "ctrl+alt+[KeyZ]"),
        );
    }
}

mod keyboard_mapper {
    use super::*;

    #[test]
    fn issue_23706_linux_uk_layout_ctrl_apostrophe_also_toggles_terminal() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mapper = new_mapper(
            false,
            &[("Backquote".to_string(), MacLinuxKeyMapping::new("`", "¬", "|", "|"))],
            false,
            OperatingSystem::Linux,
        );

        assert_resolve_keyboard_event(
            &mapper,
            &code_event(true, false, false, false, false, "Backquote"),
            &ctrl_chord("Ctrl+`", "Control+`", None, "ctrl+`", true, "ctrl+[Backquote]"),
        );
    }

    #[test]
    fn issue_24064_numlock_numpad_keys_stopped_working_in_1_11_on_linux() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mapper = new_mapper(false, &[], false, OperatingSystem::Linux);

        assert_plain_key_event(&mapper, KeyCode::End, "Numpad1", "End", Some("End"), "end", "[End]");
        assert_plain_key_event(&mapper, KeyCode::DownArrow, "Numpad2", "DownArrow", Some("Down"), "down", "[ArrowDown]");
        assert_plain_key_event(&mapper, KeyCode::PageDown, "Numpad3", "PageDown", Some("PageDown"), "pagedown", "[PageDown]");
        assert_plain_key_event(&mapper, KeyCode::LeftArrow, "Numpad4", "LeftArrow", Some("Left"), "left", "[ArrowLeft]");
        assert_plain_key_event(&mapper, KeyCode::Unknown, "Numpad5", "NumPad5", None, "numpad5", "[Numpad5]");
        assert_plain_key_event(&mapper, KeyCode::RightArrow, "Numpad6", "RightArrow", Some("Right"), "right", "[ArrowRight]");
        assert_plain_key_event(&mapper, KeyCode::Home, "Numpad7", "Home", Some("Home"), "home", "[Home]");
        assert_plain_key_event(&mapper, KeyCode::UpArrow, "Numpad8", "UpArrow", Some("Up"), "up", "[ArrowUp]");
        assert_plain_key_event(&mapper, KeyCode::PageUp, "Numpad9", "PageUp", Some("PageUp"), "pageup", "[PageUp]");
        assert_plain_key_event(&mapper, KeyCode::Insert, "Numpad0", "Insert", Some("Insert"), "insert", "[Insert]");
        assert_plain_key_event(&mapper, KeyCode::Delete, "NumpadDecimal", "Del", Some("Delete"), "delete", "[Delete]");
    }

    #[test]
    fn issue_24107_delete_insert_home_end_pgup_pgdn_and_arrow_keys_no_longer_work_editor_in_1_11() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mapper = new_mapper(false, &[], false, OperatingSystem::Linux);

        // https://github.com/microsoft/vscode/issues/24107#issuecomment-292318497
        assert_plain_key_event(&mapper, KeyCode::UpArrow, "Lang3", "UpArrow", Some("Up"), "up", "[ArrowUp]");
        assert_plain_key_event(&mapper, KeyCode::DownArrow, "NumpadEnter", "DownArrow", Some("Down"), "down", "[ArrowDown]");
        assert_plain_key_event(&mapper, KeyCode::LeftArrow, "Convert", "LeftArrow", Some("Left"), "left", "[ArrowLeft]");
        assert_plain_key_event(&mapper, KeyCode::RightArrow, "NonConvert", "RightArrow", Some("Right"), "right", "[ArrowRight]");
        assert_plain_key_event(&mapper, KeyCode::Delete, "PrintScreen", "Del", Some("Delete"), "delete", "[Delete]");
        assert_plain_key_event(&mapper, KeyCode::Insert, "NumpadDivide", "Insert", Some("Insert"), "insert", "[Insert]");
        assert_plain_key_event(&mapper, KeyCode::End, "Unknown", "End", Some("End"), "end", "[End]");
        assert_plain_key_event(&mapper, KeyCode::Home, "IntlRo", "Home", Some("Home"), "home", "[Home]");
        assert_plain_key_event(&mapper, KeyCode::PageDown, "ControlRight", "PageDown", Some("PageDown"), "pagedown", "[PageDown]");
        assert_plain_key_event(&mapper, KeyCode::PageUp, "Lang4", "PageUp", Some("PageUp"), "pageup", "[PageUp]");

        // https://github.com/microsoft/vscode/issues/24107#issuecomment-292323924
        assert_plain_key_event(&mapper, KeyCode::PageDown, "ControlRight", "PageDown", Some("PageDown"), "pagedown", "[PageDown]");
        assert_plain_key_event(&mapper, KeyCode::PageUp, "Lang4", "PageUp", Some("PageUp"), "pageup", "[PageUp]");
        assert_plain_key_event(&mapper, KeyCode::End, "", "End", Some("End"), "end", "[End]");
        assert_plain_key_event(&mapper, KeyCode::Home, "IntlRo", "Home", Some("Home"), "home", "[Home]");
        assert_plain_key_event(&mapper, KeyCode::Delete, "PrintScreen", "Del", Some("Delete"), "delete", "[Delete]");
        assert_plain_key_event(&mapper, KeyCode::Insert, "NumpadDivide", "Insert", Some("Insert"), "insert", "[Insert]");
        assert_plain_key_event(&mapper, KeyCode::RightArrow, "NonConvert", "RightArrow", Some("Right"), "right", "[ArrowRight]");
        assert_plain_key_event(&mapper, KeyCode::LeftArrow, "Convert", "LeftArrow", Some("Left"), "left", "[ArrowLeft]");
        assert_plain_key_event(&mapper, KeyCode::DownArrow, "NumpadEnter", "DownArrow", Some("Down"), "down", "[ArrowDown]");
        assert_plain_key_event(&mapper, KeyCode::UpArrow, "Lang3", "UpArrow", Some("Up"), "up", "[ArrowUp]");
    }
}

mod linux_ru {
    use super::*;

    fn mapper() -> MacLinuxKeyboardMapper {
        create_keyboard_mapper(false, "linux_ru", false, OperatingSystem::Linux)
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "linux_ru.txt");
    }

    #[test]
    fn resolve_keybinding_ctrl_s() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            OperatingSystem::Linux,
            KeyMod::CTRL_CMD | KeyCode::KeyS.bits(),
            &[ctrl_chord("Ctrl+S", "Control+S", Some("Ctrl+S"), "ctrl+s", true, "ctrl+[KeyS]")],
        );
    }
}

mod linux_en_uk {
    use super::*;

    fn mapper() -> MacLinuxKeyboardMapper {
        create_keyboard_mapper(false, "linux_en_uk", false, OperatingSystem::Linux)
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "linux_en_uk.txt");
    }

    #[test]
    fn issue_24522_resolve_keyboard_event_ctrl_alt_minus() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &code_event(true, false, true, false, false, "Minus"),
            &ctrl_chord("Ctrl+Alt+-", "Control+Alt+-", None, "ctrl+alt+[Minus]", false, "ctrl+alt+[Minus]"),
        );
    }
}

mod mac_zh_hant {
    use super::*;

    fn mapper() -> MacLinuxKeyboardMapper {
        create_keyboard_mapper(false, "mac_zh_hant", false, OperatingSystem::Macintosh)
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "mac_zh_hant.txt");
    }

    #[test]
    fn issue_28237_resolve_keybinding_cmd_c() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            OperatingSystem::Macintosh,
            KeyMod::CTRL_CMD | KeyCode::KeyC.bits(),
            &[ctrl_chord("⌘C", "Command+C", Some("Cmd+C"), "cmd+c", true, "meta+[KeyC]")],
        );
    }
}

mod mac_zh_hant2 {
    use super::*;

    fn mapper() -> MacLinuxKeyboardMapper {
        create_keyboard_mapper(false, "mac_zh_hant2", false, OperatingSystem::Macintosh)
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "mac_zh_hant2.txt");
    }
}
