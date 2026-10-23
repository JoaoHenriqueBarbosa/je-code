//! Porte de `src/vs/editor/test/common/core/range.test.ts` (suíte `Editor Core - Range`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::position::Position;
use je_editor::range::Range;

#[test]
fn empty_range() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let s = Range::new(1, 1, 1, 1);
    assert_eq!(s.start_line_number, 1);
    assert_eq!(s.start_column, 1);
    assert_eq!(s.end_line_number, 1);
    assert_eq!(s.end_column, 1);
    assert!(s.is_empty());
}

#[test]
fn swap_start_and_stop_same_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let s = Range::new(1, 2, 1, 1);
    assert_eq!(s.start_line_number, 1);
    assert_eq!(s.start_column, 1);
    assert_eq!(s.end_line_number, 1);
    assert_eq!(s.end_column, 2);
    assert!(!s.is_empty());
}

#[test]
fn swap_start_and_stop() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let s = Range::new(2, 1, 1, 2);
    assert_eq!(s.start_line_number, 1);
    assert_eq!(s.start_column, 2);
    assert_eq!(s.end_line_number, 2);
    assert_eq!(s.end_column, 1);
    assert!(!s.is_empty());
}

#[test]
fn no_swap_same_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let s = Range::new(1, 1, 1, 2);
    assert_eq!(s.start_line_number, 1);
    assert_eq!(s.start_column, 1);
    assert_eq!(s.end_line_number, 1);
    assert_eq!(s.end_column, 2);
    assert!(!s.is_empty());
}

#[test]
fn no_swap() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let s = Range::new(1, 1, 2, 1);
    assert_eq!(s.start_line_number, 1);
    assert_eq!(s.start_column, 1);
    assert_eq!(s.end_line_number, 2);
    assert_eq!(s.end_column, 1);
    assert!(!s.is_empty());
}

#[test]
fn compare_ranges_using_ends() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut a: Range;
    let mut b: Range;

    a = Range::new(1, 1, 1, 3);
    b = Range::new(1, 2, 1, 4);
    assert!(Range::compare_ranges_using_ends(&a, &b) < 0, "a.start < b.start, a.end < b.end");

    a = Range::new(1, 1, 1, 3);
    b = Range::new(1, 1, 1, 4);
    assert!(Range::compare_ranges_using_ends(&a, &b) < 0, "a.start = b.start, a.end < b.end");

    a = Range::new(1, 2, 1, 3);
    b = Range::new(1, 1, 1, 4);
    assert!(Range::compare_ranges_using_ends(&a, &b) < 0, "a.start > b.start, a.end < b.end");

    a = Range::new(1, 1, 1, 4);
    b = Range::new(1, 2, 1, 4);
    assert!(Range::compare_ranges_using_ends(&a, &b) < 0, "a.start < b.start, a.end = b.end");

    a = Range::new(1, 1, 1, 4);
    b = Range::new(1, 1, 1, 4);
    assert!(Range::compare_ranges_using_ends(&a, &b) == 0, "a.start = b.start, a.end = b.end");

    a = Range::new(1, 2, 1, 4);
    b = Range::new(1, 1, 1, 4);
    assert!(Range::compare_ranges_using_ends(&a, &b) > 0, "a.start > b.start, a.end = b.end");

    a = Range::new(1, 1, 1, 5);
    b = Range::new(1, 2, 1, 4);
    assert!(Range::compare_ranges_using_ends(&a, &b) > 0, "a.start < b.start, a.end > b.end");

    a = Range::new(1, 1, 2, 4);
    b = Range::new(1, 1, 1, 4);
    assert!(Range::compare_ranges_using_ends(&a, &b) > 0, "a.start = b.start, a.end > b.end");

    a = Range::new(1, 2, 5, 1);
    b = Range::new(1, 1, 1, 4);
    assert!(Range::compare_ranges_using_ends(&a, &b) > 0, "a.start > b.start, a.end > b.end");
}

#[test]
fn contains_position() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let r = Range::new(2, 2, 5, 10);
    assert!(!r.contains_position(&Position::new(1, 3)));
    assert!(!r.contains_position(&Position::new(2, 1)));
    assert!(r.contains_position(&Position::new(2, 2)));
    assert!(r.contains_position(&Position::new(2, 3)));
    assert!(r.contains_position(&Position::new(3, 1)));
    assert!(r.contains_position(&Position::new(5, 9)));
    assert!(r.contains_position(&Position::new(5, 10)));
    assert!(!r.contains_position(&Position::new(5, 11)));
    assert!(!r.contains_position(&Position::new(6, 1)));
}

#[test]
fn contains_range() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let r = Range::new(2, 2, 5, 10);
    assert!(!r.contains_range(&Range::new(1, 3, 2, 2)));
    assert!(!r.contains_range(&Range::new(2, 1, 2, 2)));
    assert!(!r.contains_range(&Range::new(2, 2, 5, 11)));
    assert!(!r.contains_range(&Range::new(2, 2, 6, 1)));
    assert!(!r.contains_range(&Range::new(5, 9, 6, 1)));
    assert!(!r.contains_range(&Range::new(5, 10, 6, 1)));
    assert!(r.contains_range(&Range::new(2, 2, 5, 10)));
    assert!(r.contains_range(&Range::new(2, 3, 5, 9)));
    assert!(r.contains_range(&Range::new(3, 100, 4, 100)));
}

#[test]
fn are_intersecting() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!Range::are_intersecting(&Range::new(2, 2, 3, 2), &Range::new(4, 2, 5, 2)));
    assert!(!Range::are_intersecting(&Range::new(4, 2, 5, 2), &Range::new(2, 2, 3, 2)));
    assert!(!Range::are_intersecting(&Range::new(4, 2, 5, 2), &Range::new(5, 2, 6, 2)));
    assert!(!Range::are_intersecting(&Range::new(5, 2, 6, 2), &Range::new(4, 2, 5, 2)));
    assert!(Range::are_intersecting(&Range::new(2, 2, 2, 7), &Range::new(2, 4, 2, 6)));
    assert!(Range::are_intersecting(&Range::new(2, 2, 2, 7), &Range::new(2, 4, 2, 9)));
    assert!(Range::are_intersecting(&Range::new(2, 4, 2, 9), &Range::new(2, 2, 2, 7)));
}
