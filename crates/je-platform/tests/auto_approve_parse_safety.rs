//! Porte de `src/vs/platform/terminal/test/common/autoApprove/autoApproveParseSafety.test.ts` (suíte `shouldRequireConfirmationForAutoApproveParse`), caso a caso e na mesma ordem.
//!
//! Divergências: a linguagem `'bash' | 'powershell'` é o enum `AutoApproveParseLanguage`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::terminal::auto_approve::auto_approve_parse_safety::{
    AutoApproveParseLanguage::{Bash, Powershell},
    should_require_confirmation_for_auto_approve_parse as should_require,
};

#[test]
fn requires_confirmation_for_power_shell_parse_errors() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        vec![should_require(Powershell, false), should_require(Powershell, true), should_require(Bash, false), should_require(Bash, true)],
        vec![false, true, false, false]
    );
}
