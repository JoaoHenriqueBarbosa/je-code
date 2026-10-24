//! Porte de `src/vs/workbench/contrib/preferences/test/browser/keybindingsEditorContribution.test.ts` (suíte
//! `KeybindingsEditorContribution`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`KeybindingEditorDecorationsRenderer._userSettingsFuzzyEquals`.** É a função livre `user_settings_fuzzy_equals`; o
//!   resto da classe (decorações do modelo de texto) depende do editor em DOM e não entra.
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda criado no começo do caso (no original ele vem no fim da
//!   suíte, o que vale para o caso único).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::preferences::keybindings_editor_contribution::user_settings_fuzzy_equals;

fn assert_user_settings_fuzzy_equals(a: &str, b: &str, expected: bool) {
    let actual = user_settings_fuzzy_equals(a, b);
    let message = if expected { format!("{a} == {b}") } else { format!("{a} != {b}") };
    assert_eq!(actual, expected, "fuzzy: {message}");
}

fn assert_equal(a: &str, b: &str) {
    assert_user_settings_fuzzy_equals(a, b, true);
}

fn assert_different(a: &str, b: &str) {
    assert_user_settings_fuzzy_equals(a, b, false);
}

#[test]
fn user_settings_fuzzy_equals_cases() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_equal("a", "a");
    assert_equal("a", "A");
    assert_equal("ctrl+a", "CTRL+A");
    assert_equal("ctrl+a", " CTRL+A ");

    assert_equal("ctrl+shift+a", "shift+ctrl+a");
    assert_equal("ctrl+shift+a ctrl+alt+b", "shift+ctrl+a alt+ctrl+b");

    assert_different("ctrl+[KeyA]", "ctrl+a");

    // issue #23335
    assert_equal("cmd+shift+p", "shift+cmd+p");
    assert_equal("cmd+shift+p", "shift-cmd-p");
}
