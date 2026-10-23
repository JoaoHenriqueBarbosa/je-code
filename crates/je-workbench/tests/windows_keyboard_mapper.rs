//! Porte de `src/vs/workbench/services/keybinding/test/node/windowsKeyboardMapper.test.ts` (33 casos), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - O literal `IResolvedKeybinding` do original vira [`expected`], e o `IKeyboardEvent` vira [`event`], para o
//!   corpo de cada caso mostrar só o que varia (o `_standardKeyboardEventBrand` não tem equivalente e `code: null!`
//!   é `None`).
//! - `suiteSetup` (um mapeador por suíte) vira [`create_keyboard_mapper`] em cada caso: montar o mapeador é barato.
//! - `readRawMapping` executa o arquivo `win_*.js` (um `define({...})` com um literal de objeto). Aqui o literal é
//!   lido pelo analisador mínimo de `support::js_literal`, compartilhado com os testes do Mac e do Linux.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::key_codes::{KeyCode, KeyMod, ScanCode, key_chord};
use je_base::keybindings::{Keybinding, SingleModifierChord, decode_keybinding};
use je_base::platform::OperatingSystem;
use je_platform::keybinding::keybinding::KeyboardEvent;
use je_platform::keyboard_layout::WindowsKeyMapping;
use je_workbench::services::keybinding::windows_keyboard_mapper::WindowsKeyboardMapper;
use support::js_literal::read_raw_entries;
use support::keyboard_mapper_test_utils::{
    ResolvedKeybindingSnapshot, assert_mapping, assert_resolve_keybinding, assert_resolve_keyboard_event, ctrl_chord, expected,
    key_code_chord, scan_code_chord,
};

const WRITE_FILE_IF_DIFFERENT: bool = false;

/// `readRawMapping<IWindowsKeyboardMapping>(file)`: o mapa na ordem em que o arquivo o declara.
fn read_raw_mapping(file: &str) -> Vec<(String, WindowsKeyMapping)> {
    read_raw_entries(file)
        .into_iter()
        .map(|entry| {
            let mapping = WindowsKeyMapping {
                vkey: entry.text("vkey"),
                value: entry.text("value"),
                with_shift: entry.text("withShift"),
                with_alt_gr: entry.text("withAltGr"),
                with_shift_alt_gr: entry.text("withShiftAltGr"),
            };
            (entry.code, mapping)
        })
        .collect()
}

fn new_mapper(is_us_standard: bool, raw_mappings: &[(String, WindowsKeyMapping)], map_alt_gr_to_ctrl_alt: bool) -> WindowsKeyboardMapper {
    WindowsKeyboardMapper::new(is_us_standard, raw_mappings.iter().map(|(code, mapping)| (code, mapping)), map_alt_gr_to_ctrl_alt)
}

/// `createKeyboardMapper`.
fn create_keyboard_mapper(is_us_standard: bool, file: &str, map_alt_gr_to_ctrl_alt: bool) -> WindowsKeyboardMapper {
    new_mapper(is_us_standard, &read_raw_mapping(file), map_alt_gr_to_ctrl_alt)
}

/// `_assertResolveKeybinding`.
fn assert_binary_resolve_keybinding(mapper: &WindowsKeyboardMapper, k: u32, expected: &[ResolvedKeybindingSnapshot]) {
    let keybinding = decode_keybinding(k, OperatingSystem::Windows).unwrap().unwrap();
    assert_resolve_keybinding(mapper, &keybinding, expected);
}

/// Um `IKeyboardEvent` na ordem `ctrl, shift, alt, meta, altGraph` do original.
fn event(ctrl: bool, shift: bool, alt: bool, meta: bool, alt_gr: bool, key_code: KeyCode) -> KeyboardEvent {
    KeyboardEvent { ctrl_key: ctrl, shift_key: shift, alt_key: alt, meta_key: meta, alt_graph_key: alt_gr, key_code, code: None }
}

mod de_ch {
    use super::*;

    fn mapper() -> WindowsKeyboardMapper {
        create_keyboard_mapper(false, "win_de_ch", false)
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "win_de_ch.txt");
    }

    #[test]
    fn resolve_keybinding_ctrl_a() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::KeyA.bits(),
            &[ctrl_chord("Ctrl+A", "Control+A", Some("Ctrl+A"), "ctrl+a", true, "ctrl+A")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::KeyZ.bits(),
            &[ctrl_chord("Ctrl+Z", "Control+Z", Some("Ctrl+Z"), "ctrl+z", true, "ctrl+Z")],
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::KeyZ),
            &ctrl_chord("Ctrl+Z", "Control+Z", Some("Ctrl+Z"), "ctrl+z", true, "ctrl+Z"),
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::BracketRight.bits(),
            &[ctrl_chord("Ctrl+^", "Control+^", Some("Ctrl+]"), "ctrl+oem_6", false, "ctrl+]")],
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::BracketRight),
            &ctrl_chord("Ctrl+^", "Control+^", Some("Ctrl+]"), "ctrl+oem_6", false, "ctrl+]"),
        );
    }

    #[test]
    fn resolve_keybinding_shift_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::SHIFT | KeyCode::BracketRight.bits(),
            &[ctrl_chord("Shift+^", "Shift+^", Some("Shift+]"), "shift+oem_6", false, "shift+]")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::Slash.bits(),
            &[ctrl_chord("Ctrl+§", "Control+§", Some("Ctrl+/"), "ctrl+oem_2", false, "ctrl+/")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_shift_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyCode::Slash.bits(),
            &[ctrl_chord("Ctrl+Shift+§", "Control+Shift+§", Some("Ctrl+Shift+/"), "ctrl+shift+oem_2", false, "ctrl+shift+/")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_k_ctrl_backslash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Backslash.bits()),
            &[expected(
                "Ctrl+K Ctrl+ä",
                "Control+K Control+ä",
                None,
                "ctrl+k ctrl+oem_5",
                false,
                true,
                &[Some("ctrl+K"), Some("ctrl+\\")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_k_ctrl_equal() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Equal.bits()),
            &[],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_down_arrow() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::DownArrow.bits(),
            &[ctrl_chord("Ctrl+DownArrow", "Control+DownArrow", Some("Ctrl+Down"), "ctrl+down", true, "ctrl+DownArrow")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_numpad_0() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::Numpad0.bits(),
            &[ctrl_chord("Ctrl+NumPad0", "Control+NumPad0", None, "ctrl+numpad0", true, "ctrl+NumPad0")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_home() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::Home.bits(),
            &[ctrl_chord("Ctrl+Home", "Control+Home", Some("Ctrl+Home"), "ctrl+home", true, "ctrl+Home")],
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_home() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::Home),
            &ctrl_chord("Ctrl+Home", "Control+Home", Some("Ctrl+Home"), "ctrl+home", true, "ctrl+Home"),
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
                "Ctrl+, Ctrl+§",
                "Control+, Control+§",
                None,
                "ctrl+oem_comma ctrl+oem_2",
                false,
                true,
                &[Some("ctrl+,"), Some("ctrl+/")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_ctrl() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::Ctrl),
            &expected("Ctrl", "Control", None, "ctrl", true, false, &[None], &[Some(SingleModifierChord::Ctrl)]),
        );
    }
}

mod en_us {
    use super::*;

    fn mapper() -> WindowsKeyboardMapper {
        create_keyboard_mapper(true, "win_en_us", false)
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "win_en_us.txt");
    }

    #[test]
    fn resolve_keybinding_ctrl_k_ctrl_backslash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Backslash.bits()),
            &[expected(
                "Ctrl+K Ctrl+\\",
                "Control+K Control+\\",
                None,
                "ctrl+k ctrl+\\",
                true,
                true,
                &[Some("ctrl+K"), Some("ctrl+\\")],
                &[None, None],
            )],
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
                &[Some("ctrl+,"), Some("ctrl+/")],
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
            &[ctrl_chord("Ctrl+,", "Control+,", Some("Ctrl+,"), "ctrl+,", true, "ctrl+,")],
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_ctrl() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::Ctrl),
            &expected("Ctrl", "Control", None, "ctrl", true, false, &[None], &[Some(SingleModifierChord::Ctrl)]),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_shift() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(false, true, false, false, false, KeyCode::Shift),
            &expected("Shift", "Shift", None, "shift", true, false, &[None], &[Some(SingleModifierChord::Shift)]),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_alt() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(false, false, true, false, false, KeyCode::Alt),
            &expected("Alt", "Alt", None, "alt", true, false, &[None], &[Some(SingleModifierChord::Alt)]),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_meta() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(false, false, false, true, false, KeyCode::Meta),
            &expected("Windows", "Windows", None, "win", true, false, &[None], &[Some(SingleModifierChord::Meta)]),
        );
    }

    #[test]
    fn resolve_keyboard_event_only_modifiers_ctrl_shift() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, true, false, false, false, KeyCode::Shift),
            &expected("Ctrl+Shift", "Control+Shift", None, "ctrl+shift", true, false, &[None], &[None]),
        );
    }

    #[test]
    fn resolve_keyboard_event_map_alt_gr_to_ctrl_alt_alt_gr_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mapper = create_keyboard_mapper(true, "win_en_us", true);

        assert_resolve_keyboard_event(
            &mapper,
            &event(false, false, false, false, true, KeyCode::KeyZ),
            &ctrl_chord("Ctrl+Alt+Z", "Control+Alt+Z", Some("Ctrl+Alt+Z"), "ctrl+alt+z", true, "ctrl+alt+Z"),
        );
    }
}

mod por_ptb {
    use super::*;

    fn mapper() -> WindowsKeyboardMapper {
        create_keyboard_mapper(false, "win_por_ptb", false)
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "win_por_ptb.txt");
    }

    #[test]
    fn resolve_keyboard_event_ctrl_intl_ro() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::AbntC1),
            &ctrl_chord("Ctrl+/", "Control+/", Some("Ctrl+ABNT_C1"), "ctrl+abnt_c1", false, "ctrl+ABNT_C1"),
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_numpad_comma() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::AbntC2),
            &ctrl_chord("Ctrl+.", "Control+.", Some("Ctrl+ABNT_C2"), "ctrl+abnt_c2", false, "ctrl+ABNT_C2"),
        );
    }
}

mod ru {
    use super::*;

    fn mapper() -> WindowsKeyboardMapper {
        create_keyboard_mapper(false, "win_ru", false)
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "win_ru.txt");
    }

    #[test]
    fn issue_24361_resolve_keybinding_ctrl_k_ctrl_k() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::KeyK.bits()),
            &[expected(
                "Ctrl+K Ctrl+K",
                "Control+K Control+K",
                None,
                "ctrl+k ctrl+k",
                true,
                true,
                &[Some("ctrl+K"), Some("ctrl+K")],
                &[None, None],
            )],
        );
    }
}

mod misc {
    use super::*;

    fn key_mapping(vkey: &str, value: &str, with_shift: &str) -> WindowsKeyMapping {
        WindowsKeyMapping {
            vkey: vkey.to_string(),
            value: value.to_string(),
            with_shift: with_shift.to_string(),
            with_alt_gr: String::new(),
            with_shift_alt_gr: String::new(),
        }
    }

    #[test]
    fn issue_23513_toggle_sidebar_visibility_and_go_to_line_display_same_key_mapping_in_arabic_keyboard() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mapper = new_mapper(
            false,
            &[
                ("KeyB".to_string(), key_mapping("VK_B", "لا", "لآ")),
                ("KeyG".to_string(), key_mapping("VK_G", "ل", "لأ")),
            ],
            false,
        );

        assert_binary_resolve_keybinding(
            &mapper,
            KeyMod::CTRL_CMD | KeyCode::KeyB.bits(),
            &[ctrl_chord("Ctrl+B", "Control+B", Some("Ctrl+B"), "ctrl+b", true, "ctrl+B")],
        );
    }
}
