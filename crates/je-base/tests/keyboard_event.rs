//! Porte de `src/vs/base/test/browser/keyboardEvent.test.ts`.
//!
//! Notas de tradução:
//! - `new KeyboardEvent('keydown', init)` com o `keyCode` redefinido por `Object.defineProperty` é o [`KeyboardEvent`] da
//!   crate com o `key_code` atribuído direto.
//! - `KeyCode.KEY_IN_COMPOSITION` é `KeyCode::KeyInComposition`; `KeyMod.CtrlCmd` é `KeyMod::CTRL_CMD`, e a codificação
//!   `KeyMod | KeyCode` vira `u32` com `KeyCode::bits`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::browser::keyboard_event::{KeyboardEvent, StandardKeyboardEvent};
use je_base::key_codes::{KeyCode, KeyMod};

fn keydown(init: KeyboardEvent) -> StandardKeyboardEvent {
    // `keyCode` is legacy but is what `StandardKeyboardEvent` reads, so it has to be set explicitly.
    StandardKeyboardEvent::new(init)
}

#[test]
fn reports_the_pressed_key_when_no_composition_is_in_progress() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let event = keydown(KeyboardEvent { key_code: 13, is_composing: false, ..Default::default() });
    assert_eq!([event.key_code == KeyCode::Enter, event.equals(KeyCode::Enter.bits())], [true, true]);
}

#[test]
fn normalizes_the_key_code_to_key_in_composition_while_composing() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Some platform/IME combinations report the real key code (rather than 229) for the Enter
    // that commits a composition. Normalizing means both `equals()` callers and the many
    // handlers that compare `keyCode` directly stop seeing a key the user never aimed at them.
    let event = keydown(KeyboardEvent { key_code: 13, is_composing: true, ..Default::default() });
    assert_eq!([event.key_code == KeyCode::KeyInComposition, event.equals(KeyCode::Enter.bits())], [true, false]);
}

#[test]
fn normalizes_modified_keybindings_while_composing_too() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let event = keydown(KeyboardEvent { key_code: 13, ctrl_key: true, is_composing: true, ..Default::default() });
    assert!(!event.equals(KeyMod::CTRL_CMD | KeyCode::Enter.bits()));
}

#[test]
fn keeps_matching_key_in_composition_while_composing() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Composition-aware callers ask about KEY_IN_COMPOSITION explicitly; the editor relies on
    // this to detect IME input, so it has to keep working for both key code shapes.
    for key_code in [229, 13] {
        let event = keydown(KeyboardEvent { key_code, is_composing: true, ..Default::default() });
        assert_eq!((key_code, event.equals(KeyCode::KeyInComposition.bits())), (key_code, true));
    }
}

#[test]
fn resolves_to_a_chord_that_matches_no_keybinding_while_composing() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Keybinding resolution goes through `toKeyCodeChord()` rather than `equals()`, so it needs
    // the same normalization to avoid running commands mid-composition.
    let event = keydown(KeyboardEvent { key_code: 13, is_composing: true, ..Default::default() });
    assert_eq!(event.to_key_code_chord().key_code, KeyCode::KeyInComposition);
}
