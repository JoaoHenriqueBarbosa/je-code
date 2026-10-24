//! Porte de `src/vs/workbench/contrib/notebook/test/browser/contrib/executionStatusBarItem.test.ts` (suíte `notebookBrowser`), caso a caso e na mesma ordem. O caso está portado.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` do caso.
//! - `formatCellDuration(x)` (o `showMilliseconds` omitido vale `true`) é `format_cell_duration(x, true)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::notebook::browser::contrib::cell_status_bar::execution_status_bar_item_controller::format_cell_duration;

#[test]
fn format_cell_duration_cases() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(format_cell_duration(0.0, false), "0.0s");
    assert_eq!(format_cell_duration(0.0, true), "0ms");
    assert_eq!(format_cell_duration(10.0, false), "0.0s");
    assert_eq!(format_cell_duration(10.0, true), "10ms");
    assert_eq!(format_cell_duration(100.0, false), "0.1s");
    assert_eq!(format_cell_duration(100.0, true), "100ms");
    assert_eq!(format_cell_duration(200.0, false), "0.2s");
    assert_eq!(format_cell_duration(200.0, true), "200ms");
    assert_eq!(format_cell_duration(3300.0, true), "3.3s");
    assert_eq!(format_cell_duration(180000.0, true), "3m 0.0s");
    assert_eq!(format_cell_duration(189412.0, true), "3m 9.4s");
}
