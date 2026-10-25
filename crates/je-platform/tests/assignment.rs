//! Porte de `src/vs/platform/assignment/test/common/assignment.test.ts` (suíte `Assignment`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: `assert.deepStrictEqual` é `assert_eq!` sobre uma tupla. `undefined` é `None`; `isStaff` é
//! `Option<bool>` e a lista de organizações é `Option<&[&str]>`. A guarda de vazamentos é a mesma do
//! `ensureNoDisposablesAreLeakedInTestSuite`.

mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::assignment::is_internal_account;

#[test]
fn an_account_is_internal_when_it_is_staff_or_belongs_to_an_internal_organisation() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        (
            is_internal_account(None, None),
            is_internal_account(Some(false), Some(&["contoso"])),
            is_internal_account(Some(true), Some(&["contoso"])),
            is_internal_account(Some(false), Some(&["microsoft"])),
            is_internal_account(None, Some(&["Visual-Studio-Code"])),
            is_internal_account(None, Some(&["github"])),
            is_internal_account(None, Some(&["ms-copilot"])),
        ),
        (false, false, true, true, true, true, true),
    );
}
