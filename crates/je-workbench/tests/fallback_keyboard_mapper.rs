//! Porte de `src/vs/workbench/services/keybinding/test/node/fallbackKeyboardMapper.test.ts` (suítes
//! `keyboardMapper - MAC fallback` e `keyboardMapper - LINUX fallback`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - O literal `IResolvedKeybinding` do original vira [`expected`], e o `IKeyboardEvent` vira [`event`], para o
//!   corpo de cada caso mostrar só o que varia (o `_standardKeyboardEventBrand` não tem equivalente e `code: null!`
//!   é `None`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::key_codes::{KeyCode, KeyMod, ScanCode, key_chord};
use je_base::keybindings::{Chord, KeyCodeChord, Keybinding, ScanCodeChord, SingleModifierChord, decode_keybinding};
use je_base::platform::OperatingSystem;
use je_platform::keybinding::keybinding::KeyboardEvent;
use je_workbench::services::keybinding::fallback_keyboard_mapper::FallbackKeyboardMapper;
use support::keyboard_mapper_test_utils::{
    ResolvedKeybindingSnapshot, assert_resolve_keybinding, assert_resolve_keyboard_event,
};

fn s(text: &str) -> Option<String> {
    Some(text.to_string())
}

#[allow(clippy::too_many_arguments)]
fn expected(
    label: &str,
    aria_label: &str,
    electron_accelerator: Option<&str>,
    user_settings_label: &str,
    is_multi_chord: bool,
    dispatch_parts: &[Option<&str>],
    single_modifier_dispatch_parts: &[Option<SingleModifierChord>],
) -> ResolvedKeybindingSnapshot {
    ResolvedKeybindingSnapshot {
        label: s(label),
        aria_label: s(aria_label),
        electron_accelerator: electron_accelerator.map(str::to_string),
        user_settings_label: s(user_settings_label),
        is_wysiwyg: true,
        is_multi_chord,
        dispatch_parts: dispatch_parts.iter().map(|p| p.map(str::to_string)).collect(),
        single_modifier_dispatch_parts: single_modifier_dispatch_parts.to_vec(),
    }
}

/// Um `IKeyboardEvent` na ordem `ctrl, shift, alt, meta, altGraph` do original.
fn event(ctrl: bool, shift: bool, alt: bool, meta: bool, alt_gr: bool, key_code: KeyCode) -> KeyboardEvent {
    KeyboardEvent { ctrl_key: ctrl, shift_key: shift, alt_key: alt, meta_key: meta, alt_graph_key: alt_gr, key_code, code: None }
}

fn key_code_chord(ctrl: bool, shift: bool, alt: bool, meta: bool, key_code: KeyCode) -> Chord {
    Chord::KeyCode(KeyCodeChord::new(ctrl, shift, alt, meta, key_code))
}

fn scan_code_chord(ctrl: bool, shift: bool, alt: bool, meta: bool, scan_code: ScanCode) -> Chord {
    Chord::ScanCode(ScanCodeChord::new(ctrl, shift, alt, meta, scan_code))
}

mod mac_fallback {
    use super::*;

    const OS: OperatingSystem = OperatingSystem::Macintosh;

    fn mapper() -> FallbackKeyboardMapper {
        FallbackKeyboardMapper::new(false, OS)
    }

    fn assert_binary_resolve_keybinding(k: u32, expected: &[ResolvedKeybindingSnapshot]) {
        assert_resolve_keybinding(&mapper(), &decode_keybinding(k, OS).unwrap().unwrap(), expected);
    }

    #[test]
    fn resolve_keybinding_cmd_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            KeyMod::CTRL_CMD | KeyCode::KeyZ.bits(),
            &[expected("⌘Z", "Command+Z", Some("Cmd+Z"), "cmd+z", false, &[Some("meta+Z")], &[None])],
        );
    }

    #[test]
    fn resolve_keybinding_cmd_k_cmd_equal() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Equal.bits()),
            &[expected(
                "⌘K ⌘=",
                "Command+K Command+=",
                None,
                "cmd+k cmd+=",
                true,
                &[Some("meta+K"), Some("meta+=")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keyboard_event_cmd_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(false, false, false, true, false, KeyCode::KeyZ),
            &expected("⌘Z", "Command+Z", Some("Cmd+Z"), "cmd+z", false, &[Some("meta+Z")], &[None]),
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
            &[expected("⌘, ⌘/", "Command+, Command+/", None, "cmd+, cmd+/", true, &[Some("meta+,"), Some("meta+/")], &[None, None])],
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_meta() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(false, false, false, true, false, KeyCode::Meta),
            &expected("⌘", "Command", None, "cmd", false, &[None], &[Some(SingleModifierChord::Meta)]),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_shift() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(false, true, false, false, false, KeyCode::Shift),
            &expected("⇧", "Shift", None, "shift", false, &[None], &[Some(SingleModifierChord::Shift)]),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_alt() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(false, false, true, false, false, KeyCode::Alt),
            &expected("⌥", "Option", None, "alt", false, &[None], &[Some(SingleModifierChord::Alt)]),
        );
    }

    #[test]
    fn resolve_keyboard_event_only_modifiers_ctrl_shift() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, true, false, false, false, KeyCode::Shift),
            &expected("⌃⇧", "Control+Shift", None, "ctrl+shift", false, &[None], &[None]),
        );
    }

    #[test]
    fn resolve_keyboard_event_map_alt_gr_to_ctrl_alt_alt_gr_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mapper = FallbackKeyboardMapper::new(true, OS);

        assert_resolve_keyboard_event(
            &mapper,
            &event(false, false, false, false, true, KeyCode::KeyZ),
            &expected("⌃⌥Z", "Control+Option+Z", Some("Ctrl+Alt+Z"), "ctrl+alt+z", false, &[Some("ctrl+alt+Z")], &[None]),
        );
    }
}

mod linux_fallback {
    use super::*;

    const OS: OperatingSystem = OperatingSystem::Linux;

    fn mapper() -> FallbackKeyboardMapper {
        FallbackKeyboardMapper::new(false, OS)
    }

    fn assert_binary_resolve_keybinding(k: u32, expected: &[ResolvedKeybindingSnapshot]) {
        assert_resolve_keybinding(&mapper(), &decode_keybinding(k, OS).unwrap().unwrap(), expected);
    }

    #[test]
    fn resolve_keybinding_ctrl_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            KeyMod::CTRL_CMD | KeyCode::KeyZ.bits(),
            &[expected("Ctrl+Z", "Control+Z", Some("Ctrl+Z"), "ctrl+z", false, &[Some("ctrl+Z")], &[None])],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_k_ctrl_equal() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Equal.bits()),
            &[expected(
                "Ctrl+K Ctrl+=",
                "Control+K Control+=",
                None,
                "ctrl+k ctrl+=",
                true,
                &[Some("ctrl+K"), Some("ctrl+=")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::KeyZ),
            &expected("Ctrl+Z", "Control+Z", Some("Ctrl+Z"), "ctrl+z", false, &[Some("ctrl+Z")], &[None]),
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
            &[expected("Ctrl+, Ctrl+/", "Control+, Control+/", None, "ctrl+, ctrl+/", true, &[Some("ctrl+,"), Some("ctrl+/")], &[None, None])],
        );
    }

    #[test]
    fn resolve_user_binding_ctrl_comma() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keybinding(
            &mapper(),
            &Keybinding::new(vec![scan_code_chord(true, false, false, false, ScanCode::Comma)]).unwrap(),
            &[expected("Ctrl+,", "Control+,", Some("Ctrl+,"), "ctrl+,", false, &[Some("ctrl+,")], &[None])],
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_ctrl() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::Ctrl),
            &expected("Ctrl", "Control", None, "ctrl", false, &[None], &[Some(SingleModifierChord::Ctrl)]),
        );
    }

    #[test]
    fn resolve_keyboard_event_map_alt_gr_to_ctrl_alt_alt_gr_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mapper = FallbackKeyboardMapper::new(true, OS);

        assert_resolve_keyboard_event(
            &mapper,
            &event(false, false, false, false, true, KeyCode::KeyZ),
            &expected("Ctrl+Alt+Z", "Control+Alt+Z", Some("Ctrl+Alt+Z"), "ctrl+alt+z", false, &[Some("ctrl+alt+Z")], &[None]),
        );
    }
}
