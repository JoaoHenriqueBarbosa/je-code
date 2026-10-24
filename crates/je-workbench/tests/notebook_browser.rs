//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookBrowser.test.ts` (suíte `notebookBrowser`, subsuíte `getRanges`), caso a caso e na mesma ordem. Os 4 casos estão portados.
//!
//! Notas de tradução:
//! - `getRanges` é uma função do próprio arquivo de teste (não é produção): é portada aqui, sobre os `cellKind` das células
//!   (`{ cellKind: CellKind.Code }` é só o `CellKind`, o único membro que o predicado lê), com o predicado como fecho.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::notebook::common::notebook_common::CellKind;
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;

/// Return a set of ranges for the cells matching the given predicate.
fn get_ranges(cells: &[CellKind], included: impl Fn(CellKind) -> bool) -> Vec<ICellRange> {
    let mut ranges: Vec<ICellRange> = Vec::new();
    let mut current_range: Option<usize> = None;

    for (idx, cell) in cells.iter().enumerate() {
        let idx = idx as i64;
        if included(*cell) {
            match current_range {
                None => {
                    ranges.push(ICellRange { start: idx, end: idx + 1 });
                    current_range = Some(ranges.len() - 1);
                }
                Some(current) => ranges[current].end = idx + 1,
            }
        } else {
            current_range = None;
        }
    }

    ranges
}

fn predicate(cell: CellKind) -> bool {
    cell == CellKind::Code
}

#[test]
fn all_code() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let cells = [CellKind::Code, CellKind::Code];
    assert_eq!(get_ranges(&cells, predicate), vec![ICellRange { start: 0, end: 2 }]);
}

#[test]
fn none_code() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let cells = [CellKind::Markup, CellKind::Markup];
    assert_eq!(get_ranges(&cells, predicate), Vec::<ICellRange>::new());
}

#[test]
fn start_code() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let cells = [CellKind::Code, CellKind::Markup];
    assert_eq!(get_ranges(&cells, predicate), vec![ICellRange { start: 0, end: 1 }]);
}

#[test]
fn random() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let cells = [CellKind::Code, CellKind::Code, CellKind::Markup, CellKind::Code, CellKind::Markup, CellKind::Markup, CellKind::Code];
    assert_eq!(
        get_ranges(&cells, predicate),
        vec![ICellRange { start: 0, end: 2 }, ICellRange { start: 3, end: 4 }, ICellRange { start: 6, end: 7 }]
    );
}
