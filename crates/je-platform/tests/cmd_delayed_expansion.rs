//! Porte de `src/vs/platform/terminal/test/common/autoApprove/cmdDelayedExpansion.test.ts` (suíte `containsCmdDelayedExpansion`), caso a caso e na mesma ordem.
//!
//! Divergências: nenhuma além das do módulo (o original usa regex, aqui há uma varredura equivalente).

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::terminal::auto_approve::cmd_delayed_expansion::contains_cmd_delayed_expansion as contains;

#[test]
fn detects_balanced_delayed_environment_variable_expansion() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        vec![contains("!APPDATA!\\file.txt"), contains("!TARGET!"), contains("!VAR:~0,1!\\file.txt"), contains("!ROOT!\\!NAME!")],
        vec![true, true, true, true]
    );
}

#[test]
fn ignores_literal_or_unmatched_exclamation_marks() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        vec![contains("file.txt"), contains("important!.txt"), contains("!APPDATA"), contains("APPDATA!"), contains("!!")],
        vec![false, false, false, false, false]
    );
}
