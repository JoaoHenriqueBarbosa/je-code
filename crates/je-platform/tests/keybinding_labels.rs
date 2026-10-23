//! Porte de `src/vs/platform/keybinding/test/common/keybindingLabels.test.ts` (suíte `KeybindingLabels`), caso a caso e na mesma ordem.

mod common;

use common::keybinding::create_us_layout_resolved_keybinding;
use common::*;
use je_base::key_codes::{KeyCode, KeyMod, key_chord};
use je_base::platform::OperatingSystem;

const CTRL_CMD: u32 = KeyMod::CTRL_CMD;
const SHIFT: u32 = KeyMod::SHIFT;
const ALT: u32 = KeyMod::ALT;
const WIN_CTRL: u32 = KeyMod::WIN_CTRL;
const KEY_A: u32 = KeyCode::KeyA.0 as u32;
const KEY_B: u32 = KeyCode::KeyB.0 as u32;

fn assert_us_label(os: OperatingSystem, keybinding: u32, expected: &str) {
    let us_resolved_keybinding = create_us_layout_resolved_keybinding(keybinding, os).unwrap();
    assert_eq!(us_resolved_keybinding.get_label().as_deref(), Some(expected));
}

#[test]
fn windows_us_label() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let w = OperatingSystem::Windows;
    // no modifier
    assert_us_label(w, KEY_A, "A");

    // one modifier
    assert_us_label(w, CTRL_CMD | KEY_A, "Ctrl+A");
    assert_us_label(w, SHIFT | KEY_A, "Shift+A");
    assert_us_label(w, ALT | KEY_A, "Alt+A");
    assert_us_label(w, WIN_CTRL | KEY_A, "Windows+A");

    // two modifiers
    assert_us_label(w, CTRL_CMD | SHIFT | KEY_A, "Ctrl+Shift+A");
    assert_us_label(w, CTRL_CMD | ALT | KEY_A, "Ctrl+Alt+A");
    assert_us_label(w, CTRL_CMD | WIN_CTRL | KEY_A, "Ctrl+Windows+A");
    assert_us_label(w, SHIFT | ALT | KEY_A, "Shift+Alt+A");
    assert_us_label(w, SHIFT | WIN_CTRL | KEY_A, "Shift+Windows+A");
    assert_us_label(w, ALT | WIN_CTRL | KEY_A, "Alt+Windows+A");

    // three modifiers
    assert_us_label(w, CTRL_CMD | SHIFT | ALT | KEY_A, "Ctrl+Shift+Alt+A");
    assert_us_label(w, CTRL_CMD | SHIFT | WIN_CTRL | KEY_A, "Ctrl+Shift+Windows+A");
    assert_us_label(w, CTRL_CMD | ALT | WIN_CTRL | KEY_A, "Ctrl+Alt+Windows+A");
    assert_us_label(w, SHIFT | ALT | WIN_CTRL | KEY_A, "Shift+Alt+Windows+A");

    // four modifiers
    assert_us_label(w, CTRL_CMD | SHIFT | ALT | WIN_CTRL | KEY_A, "Ctrl+Shift+Alt+Windows+A");

    // chord
    assert_us_label(w, key_chord(CTRL_CMD | KEY_A, CTRL_CMD | KEY_B), "Ctrl+A Ctrl+B");
}

#[test]
fn linux_us_label() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let l = OperatingSystem::Linux;
    // no modifier
    assert_us_label(l, KEY_A, "A");

    // one modifier
    assert_us_label(l, CTRL_CMD | KEY_A, "Ctrl+A");
    assert_us_label(l, SHIFT | KEY_A, "Shift+A");
    assert_us_label(l, ALT | KEY_A, "Alt+A");
    assert_us_label(l, WIN_CTRL | KEY_A, "Super+A");

    // two modifiers
    assert_us_label(l, CTRL_CMD | SHIFT | KEY_A, "Ctrl+Shift+A");
    assert_us_label(l, CTRL_CMD | ALT | KEY_A, "Ctrl+Alt+A");
    assert_us_label(l, CTRL_CMD | WIN_CTRL | KEY_A, "Ctrl+Super+A");
    assert_us_label(l, SHIFT | ALT | KEY_A, "Shift+Alt+A");
    assert_us_label(l, SHIFT | WIN_CTRL | KEY_A, "Shift+Super+A");
    assert_us_label(l, ALT | WIN_CTRL | KEY_A, "Alt+Super+A");

    // three modifiers
    assert_us_label(l, CTRL_CMD | SHIFT | ALT | KEY_A, "Ctrl+Shift+Alt+A");
    assert_us_label(l, CTRL_CMD | SHIFT | WIN_CTRL | KEY_A, "Ctrl+Shift+Super+A");
    assert_us_label(l, CTRL_CMD | ALT | WIN_CTRL | KEY_A, "Ctrl+Alt+Super+A");
    assert_us_label(l, SHIFT | ALT | WIN_CTRL | KEY_A, "Shift+Alt+Super+A");

    // four modifiers
    assert_us_label(l, CTRL_CMD | SHIFT | ALT | WIN_CTRL | KEY_A, "Ctrl+Shift+Alt+Super+A");

    // chord
    assert_us_label(l, key_chord(CTRL_CMD | KEY_A, CTRL_CMD | KEY_B), "Ctrl+A Ctrl+B");
}

#[test]
fn mac_us_label() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let m = OperatingSystem::Macintosh;
    // no modifier
    assert_us_label(m, KEY_A, "A");

    // one modifier
    assert_us_label(m, CTRL_CMD | KEY_A, "⌘A");
    assert_us_label(m, SHIFT | KEY_A, "⇧A");
    assert_us_label(m, ALT | KEY_A, "⌥A");
    assert_us_label(m, WIN_CTRL | KEY_A, "⌃A");

    // two modifiers
    assert_us_label(m, CTRL_CMD | SHIFT | KEY_A, "⇧⌘A");
    assert_us_label(m, CTRL_CMD | ALT | KEY_A, "⌥⌘A");
    assert_us_label(m, CTRL_CMD | WIN_CTRL | KEY_A, "⌃⌘A");
    assert_us_label(m, SHIFT | ALT | KEY_A, "⇧⌥A");
    assert_us_label(m, SHIFT | WIN_CTRL | KEY_A, "⌃⇧A");
    assert_us_label(m, ALT | WIN_CTRL | KEY_A, "⌃⌥A");

    // three modifiers
    assert_us_label(m, CTRL_CMD | SHIFT | ALT | KEY_A, "⇧⌥⌘A");
    assert_us_label(m, CTRL_CMD | SHIFT | WIN_CTRL | KEY_A, "⌃⇧⌘A");
    assert_us_label(m, CTRL_CMD | ALT | WIN_CTRL | KEY_A, "⌃⌥⌘A");
    assert_us_label(m, SHIFT | ALT | WIN_CTRL | KEY_A, "⌃⇧⌥A");

    // four modifiers
    assert_us_label(m, CTRL_CMD | SHIFT | ALT | WIN_CTRL | KEY_A, "⌃⇧⌥⌘A");

    // chord
    assert_us_label(m, key_chord(CTRL_CMD | KEY_A, CTRL_CMD | KEY_B), "⌘A ⌘B");

    // special keys
    assert_us_label(m, KeyCode::LeftArrow.0 as u32, "←");
    assert_us_label(m, KeyCode::UpArrow.0 as u32, "↑");
    assert_us_label(m, KeyCode::RightArrow.0 as u32, "→");
    assert_us_label(m, KeyCode::DownArrow.0 as u32, "↓");
}

#[test]
fn aria_label() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let assert_aria_label = |os: OperatingSystem, keybinding: u32, expected: &str| {
        let us_resolved_keybinding = create_us_layout_resolved_keybinding(keybinding, os).unwrap();
        assert_eq!(us_resolved_keybinding.get_aria_label().as_deref(), Some(expected));
    };

    let all = CTRL_CMD | SHIFT | ALT | WIN_CTRL | KEY_A;
    assert_aria_label(OperatingSystem::Windows, all, "Control+Shift+Alt+Windows+A");
    assert_aria_label(OperatingSystem::Linux, all, "Control+Shift+Alt+Super+A");
    assert_aria_label(OperatingSystem::Macintosh, all, "Control+Shift+Option+Command+A");
}

#[test]
fn electron_accelerator_label() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let assert_electron_accelerator_label = |os: OperatingSystem, keybinding: u32, expected: Option<&str>| {
        let us_resolved_keybinding = create_us_layout_resolved_keybinding(keybinding, os).unwrap();
        assert_eq!(us_resolved_keybinding.get_electron_accelerator().as_deref(), expected);
    };

    let all = CTRL_CMD | SHIFT | ALT | WIN_CTRL | KEY_A;
    assert_electron_accelerator_label(OperatingSystem::Windows, all, Some("Ctrl+Shift+Alt+Super+A"));
    assert_electron_accelerator_label(OperatingSystem::Linux, all, Some("Ctrl+Shift+Alt+Super+A"));
    assert_electron_accelerator_label(OperatingSystem::Macintosh, all, Some("Ctrl+Shift+Alt+Cmd+A"));

    // electron cannot handle chords
    let chord = key_chord(CTRL_CMD | KEY_A, CTRL_CMD | KEY_B);
    assert_electron_accelerator_label(OperatingSystem::Windows, chord, None);
    assert_electron_accelerator_label(OperatingSystem::Linux, chord, None);
    assert_electron_accelerator_label(OperatingSystem::Macintosh, chord, None);

    // electron cannot handle numpad keys
    let numpad1 = KeyCode::Numpad1.0 as u32;
    assert_electron_accelerator_label(OperatingSystem::Windows, numpad1, None);
    assert_electron_accelerator_label(OperatingSystem::Linux, numpad1, None);
    assert_electron_accelerator_label(OperatingSystem::Macintosh, numpad1, None);

    // special
    assert_electron_accelerator_label(OperatingSystem::Macintosh, KeyCode::LeftArrow.0 as u32, Some("Left"));
    assert_electron_accelerator_label(OperatingSystem::Macintosh, KeyCode::UpArrow.0 as u32, Some("Up"));
    assert_electron_accelerator_label(OperatingSystem::Macintosh, KeyCode::RightArrow.0 as u32, Some("Right"));
    assert_electron_accelerator_label(OperatingSystem::Macintosh, KeyCode::DownArrow.0 as u32, Some("Down"));
}

#[test]
fn user_settings_label() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let assert_user_settings_label = |os: OperatingSystem, keybinding: u32, expected: &str| {
        let us_resolved_keybinding = create_us_layout_resolved_keybinding(keybinding, os).unwrap();
        assert_eq!(us_resolved_keybinding.get_user_settings_label().as_deref(), Some(expected));
    };

    let all = CTRL_CMD | SHIFT | ALT | WIN_CTRL | KEY_A;
    assert_user_settings_label(OperatingSystem::Windows, all, "ctrl+shift+alt+win+a");
    assert_user_settings_label(OperatingSystem::Linux, all, "ctrl+shift+alt+meta+a");
    assert_user_settings_label(OperatingSystem::Macintosh, all, "ctrl+shift+alt+cmd+a");

    // electron cannot handle chords
    let chord = key_chord(CTRL_CMD | KEY_A, CTRL_CMD | KEY_B);
    assert_user_settings_label(OperatingSystem::Windows, chord, "ctrl+a ctrl+b");
    assert_user_settings_label(OperatingSystem::Linux, chord, "ctrl+a ctrl+b");
    assert_user_settings_label(OperatingSystem::Macintosh, chord, "cmd+a cmd+b");
}

#[test]
fn issue_91235_do_not_end_with_a_plus() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_us_label(OperatingSystem::Windows, CTRL_CMD | ALT | KeyCode::Alt.0 as u32, "Ctrl+Alt");
}
