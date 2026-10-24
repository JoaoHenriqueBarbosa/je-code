//! Porte de `src/vs/workbench/contrib/terminalContrib/zoom/test/browser/terminal.zoom.test.ts` (suíte `Terminal Mouse Wheel
//! Zoom`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - O `number` do TypeScript é `f64`; as mensagens do `strictEqual` viram as mensagens do `assert_eq!`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::terminal_contrib::zoom::browser::terminal_zoom_contribution::clamp_terminal_font_size;

#[test]
fn clamps_font_size_to_minimum_value_when_below_bounds() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = clamp_terminal_font_size(3.0 + (-2.0)); // 3 - 2 = 1, clamped to 6
    assert_eq!(result, 6.0, "Font size should be clamped to minimum value of 6");
}

#[test]
fn clamps_font_size_to_maximum_value_when_above_bounds() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = clamp_terminal_font_size(99.0 + 5.0); // 99 + 5 = 104, clamped to 100
    assert_eq!(result, 100.0, "Font size should be clamped to maximum value of 100");
}

#[test]
fn preserves_font_size_when_within_bounds() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = clamp_terminal_font_size(12.0 + 3.0); // 12 + 3 = 15, within bounds
    assert_eq!(result, 15.0, "Font size should remain unchanged when within bounds");
}

#[test]
fn clamps_font_size_when_going_below_minimum() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = clamp_terminal_font_size(6.0 + (-1.0)); // 6 - 1 = 5, clamped to 6
    assert_eq!(result, 6.0, "Font size should be clamped when going below minimum");
}

#[test]
fn clamps_font_size_when_going_above_maximum() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = clamp_terminal_font_size(100.0 + 1.0); // 100 + 1 = 101, clamped to 100
    assert_eq!(result, 100.0, "Font size should be clamped when going above maximum");
}
