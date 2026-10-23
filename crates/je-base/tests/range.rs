//! Testes próprios de `src/vs/base/common/range.ts` (o VS Code não tem `range.test.ts`).

use je_base::range::{IRange, intersect, intersects, is_empty, relative_complement};

fn r(start: f64, end: f64) -> IRange {
    IRange { start, end }
}

#[test]
fn intersect_returns_the_overlap_or_the_zero_range() {
    assert_eq!(intersect(r(0.0, 10.0), r(5.0, 15.0)), r(5.0, 10.0));
    assert_eq!(intersect(r(0.0, 10.0), r(10.0, 20.0)), r(0.0, 0.0));
    assert_eq!(intersect(r(5.0, 6.0), r(0.0, 100.0)), r(5.0, 6.0));
    assert_eq!(intersect(r(20.0, 30.0), r(0.0, 10.0)), r(0.0, 0.0));
}

#[test]
fn is_empty_and_intersects() {
    assert!(is_empty(r(3.0, 3.0)));
    assert!(is_empty(r(5.0, 3.0)));
    assert!(!is_empty(r(3.0, 4.0)));
    assert!(intersects(r(0.0, 10.0), r(9.0, 12.0)));
    assert!(!intersects(r(0.0, 10.0), r(10.0, 12.0)));
}

#[test]
fn relative_complement_returns_what_is_left_of_one() {
    assert_eq!(relative_complement(r(0.0, 10.0), r(3.0, 6.0)), vec![r(0.0, 3.0), r(6.0, 10.0)]);
    assert_eq!(relative_complement(r(0.0, 10.0), r(0.0, 6.0)), vec![r(6.0, 10.0)]);
    assert_eq!(relative_complement(r(0.0, 10.0), r(3.0, 20.0)), vec![r(0.0, 3.0)]);
    assert_eq!(relative_complement(r(0.0, 10.0), r(0.0, 10.0)), Vec::<IRange>::new());
    assert_eq!(relative_complement(r(0.0, 10.0), r(20.0, 30.0)), vec![r(0.0, 10.0)]);
}
