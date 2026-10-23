//! Porte de `src/vs/editor/test/common/core/lineRange.test.ts` (suíte `LineRange`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::line_range::{LineRange, LineRangeSet};

#[test]
fn contains() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let r = LineRange::new(2, 3);
    assert!(!r.contains(1));
    assert!(r.contains(2));
    assert!(!r.contains(3));
    assert!(!r.contains(4));
}

// Suíte `LineRangeSet`.

#[test]
fn add_range() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut set = LineRangeSet::default();
    set.add_range(LineRange::new(2, 3));
    set.add_range(LineRange::new(3, 4));
    set.add_range(LineRange::new(10, 20));
    assert_eq!(set.to_string(), "[2,4), [10,20)");

    set.add_range(LineRange::new(3, 21));
    assert_eq!(set.to_string(), "[2,21)");
}

#[test]
fn get_union() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let set1 = LineRangeSet::new(vec![LineRange::new(2, 3), LineRange::new(5, 7), LineRange::new(10, 20)]);
    let set2 = LineRangeSet::new(vec![LineRange::new(3, 4), LineRange::new(6, 8), LineRange::new(9, 11)]);

    let union = set1.get_union(&set2);
    assert_eq!(union.to_string(), "[2,4), [5,8), [9,20)");
}

#[test]
fn intersects() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let set1 = LineRangeSet::new(vec![LineRange::new(2, 3), LineRange::new(5, 7), LineRange::new(10, 20)]);

    assert!(!set1.intersects(&LineRange::new(1, 2)));
    assert!(set1.intersects(&LineRange::new(1, 3)));
    assert!(!set1.intersects(&LineRange::new(3, 5)));
}
