//! Porte de `src/vs/workbench/api/test/browser/mainThreadTelemetry.test.ts` (suíte `MainThreadTelemetry - CAPI assignment context`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - `'b'.repeat(9 * 1024)` é `"b".repeat(9 * 1024)`; `assert.strictEqual(x, true)` e `false` são `assert!` e `assert!(!...)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::api::browser::main_thread_telemetry::is_valid_capi_assignment_context;

#[test]
fn accepts_well_formed_assignment_contexts() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert!(is_valid_capi_assignment_context("e4hcf520:1109203"));
    assert!(is_valid_capi_assignment_context("e4hcf520:1109203;61623843:1255491"));
    // a single trailing separator is tolerated (CAPI emits `a:b;c:d;`)
    assert!(is_valid_capi_assignment_context("e4hcf520:1109203;61623843:1255491;"));
}

#[test]
fn rejects_malformed_empty_or_oversized_input() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert!(!is_valid_capi_assignment_context(""));
    assert!(!is_valid_capi_assignment_context("noColonHere"));
    assert!(!is_valid_capi_assignment_context("a:b;;c:d")); // empty inner entry
    assert!(!is_valid_capi_assignment_context("a b:c")); // whitespace
    assert!(!is_valid_capi_assignment_context("a:b\nc:d")); // control character
    assert!(!is_valid_capi_assignment_context(&format!("a:{}", "b".repeat(9 * 1024)))); // over 8KB
}
