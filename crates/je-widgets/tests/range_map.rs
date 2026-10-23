//! Porte de `src/vs/base/test/browser/ui/list/rangeMap.test.ts` (suíte `RangeMap`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: `{ size: n }` é `IItem`, `one`, `two`... são constantes; `Range.intersect` é
//! `je_base::range::intersect`; `deepStrictEqual` é `assert_eq!` sobre os structs (comparação por valor).

mod support;

use je_base::range::{IRange, IRangedGroup};
use je_widgets::list::range_map::{self as rm, IItem, IRangeMap, RangeMap};
use support::ensure_no_disposables_are_leaked_in_test_suite;

mod range_map {
    use super::*;
    #[test]
    fn intersection() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(je_base::range::intersect(IRange { start: 0.0, end: 0.0 }, IRange { start: 0.0, end: 0.0 }), IRange { start: 0.0, end: 0.0 });
        assert_eq!(je_base::range::intersect(IRange { start: 0.0, end: 0.0 }, IRange { start: 5.0, end: 5.0 }), IRange { start: 0.0, end: 0.0 });
        assert_eq!(je_base::range::intersect(IRange { start: 0.0, end: 1.0 }, IRange { start: 5.0, end: 6.0 }), IRange { start: 0.0, end: 0.0 });
        assert_eq!(je_base::range::intersect(IRange { start: 5.0, end: 6.0 }, IRange { start: 0.0, end: 1.0 }), IRange { start: 0.0, end: 0.0 });
        assert_eq!(je_base::range::intersect(IRange { start: 0.0, end: 5.0 }, IRange { start: 2.0, end: 2.0 }), IRange { start: 0.0, end: 0.0 });
        assert_eq!(je_base::range::intersect(IRange { start: 0.0, end: 1.0 }, IRange { start: 0.0, end: 1.0 }), IRange { start: 0.0, end: 1.0 });
        assert_eq!(je_base::range::intersect(IRange { start: 0.0, end: 10.0 }, IRange { start: 0.0, end: 5.0 }), IRange { start: 0.0, end: 5.0 });
        assert_eq!(je_base::range::intersect(IRange { start: 0.0, end: 5.0 }, IRange { start: 0.0, end: 10.0 }), IRange { start: 0.0, end: 5.0 });
        assert_eq!(je_base::range::intersect(IRange { start: 0.0, end: 10.0 }, IRange { start: 5.0, end: 10.0 }), IRange { start: 5.0, end: 10.0 });
        assert_eq!(je_base::range::intersect(IRange { start: 5.0, end: 10.0 }, IRange { start: 0.0, end: 10.0 }), IRange { start: 5.0, end: 10.0 });
        assert_eq!(je_base::range::intersect(IRange { start: 0.0, end: 10.0 }, IRange { start: 2.0, end: 8.0 }), IRange { start: 2.0, end: 8.0 });
        assert_eq!(je_base::range::intersect(IRange { start: 2.0, end: 8.0 }, IRange { start: 0.0, end: 10.0 }), IRange { start: 2.0, end: 8.0 });
        assert_eq!(je_base::range::intersect(IRange { start: 0.0, end: 10.0 }, IRange { start: 5.0, end: 15.0 }), IRange { start: 5.0, end: 10.0 });
        assert_eq!(je_base::range::intersect(IRange { start: 5.0, end: 15.0 }, IRange { start: 0.0, end: 10.0 }), IRange { start: 5.0, end: 10.0 });
    }
    #[test]
    fn multi_intersect() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(rm::group_intersect(IRange { start: 0.0, end: 0.0 }, &[IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }]), vec![]);
        assert_eq!(rm::group_intersect(IRange { start: 10.0, end: 20.0 }, &[IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }]), vec![]);
        assert_eq!(rm::group_intersect(IRange { start: 2.0, end: 8.0 }, &[IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }]), vec![IRangedGroup { range: IRange { start: 2.0, end: 8.0 }, size: 1.0 }]);
        assert_eq!(rm::group_intersect(IRange { start: 2.0, end: 8.0 }, &[IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }, IRangedGroup { range: IRange { start: 10.0, end: 20.0 }, size: 5.0 }]), vec![IRangedGroup { range: IRange { start: 2.0, end: 8.0 }, size: 1.0 }]);
        assert_eq!(rm::group_intersect(IRange { start: 12.0, end: 18.0 }, &[IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }, IRangedGroup { range: IRange { start: 10.0, end: 20.0 }, size: 5.0 }]), vec![IRangedGroup { range: IRange { start: 12.0, end: 18.0 }, size: 5.0 }]);
        assert_eq!(rm::group_intersect(IRange { start: 2.0, end: 18.0 }, &[IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }, IRangedGroup { range: IRange { start: 10.0, end: 20.0 }, size: 5.0 }]), vec![IRangedGroup { range: IRange { start: 2.0, end: 10.0 }, size: 1.0 }, IRangedGroup { range: IRange { start: 10.0, end: 18.0 }, size: 5.0 }]);
        assert_eq!(rm::group_intersect(IRange { start: 2.0, end: 28.0 }, &[IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }, IRangedGroup { range: IRange { start: 10.0, end: 20.0 }, size: 5.0 }, IRangedGroup { range: IRange { start: 20.0, end: 30.0 }, size: 10.0 }]), vec![IRangedGroup { range: IRange { start: 2.0, end: 10.0 }, size: 1.0 }, IRangedGroup { range: IRange { start: 10.0, end: 20.0 }, size: 5.0 }, IRangedGroup { range: IRange { start: 20.0, end: 28.0 }, size: 10.0 }]);
    }
    #[test]
    fn consolidate() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_eq!(rm::consolidate(&[]), vec![]);
        assert_eq!(rm::consolidate(&[IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }]), vec![IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }]);
        assert_eq!(rm::consolidate(&[IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }, IRangedGroup { range: IRange { start: 10.0, end: 20.0 }, size: 1.0 }]), vec![IRangedGroup { range: IRange { start: 0.0, end: 20.0 }, size: 1.0 }]);
        assert_eq!(rm::consolidate(&[IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }, IRangedGroup { range: IRange { start: 10.0, end: 20.0 }, size: 1.0 }, IRangedGroup { range: IRange { start: 20.0, end: 100.0 }, size: 1.0 }]), vec![IRangedGroup { range: IRange { start: 0.0, end: 100.0 }, size: 1.0 }]);
        assert_eq!(rm::consolidate(&[IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }, IRangedGroup { range: IRange { start: 10.0, end: 20.0 }, size: 5.0 }, IRangedGroup { range: IRange { start: 20.0, end: 30.0 }, size: 10.0 }]), vec![IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }, IRangedGroup { range: IRange { start: 10.0, end: 20.0 }, size: 5.0 }, IRangedGroup { range: IRange { start: 20.0, end: 30.0 }, size: 10.0 }]);
        assert_eq!(rm::consolidate(&[IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }, IRangedGroup { range: IRange { start: 10.0, end: 20.0 }, size: 2.0 }, IRangedGroup { range: IRange { start: 20.0, end: 100.0 }, size: 2.0 }]), vec![IRangedGroup { range: IRange { start: 0.0, end: 10.0 }, size: 1.0 }, IRangedGroup { range: IRange { start: 10.0, end: 100.0 }, size: 2.0 }]);
    }
    #[test]
    fn empty() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let map = RangeMap::new(None);
        assert_eq!(map.size(), 0.0);
        assert_eq!(map.count(), 0);
    }
    const ONE: IItem = IItem { size: 1.0 };
    const TWO: IItem = IItem { size: 2.0 };
    const THREE: IItem = IItem { size: 3.0 };
    const FIVE: IItem = IItem { size: 5.0 };
    const TEN: IItem = IItem { size: 10.0 };
    #[test]
    fn length_count() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(None);
        map.splice(0, 0, &[ONE]);
        assert_eq!(map.size(), 1.0);
        assert_eq!(map.count(), 1);
    }
    #[test]
    fn length_count_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(None);
        map.splice(0, 0, &[ONE, ONE, ONE, ONE, ONE]);
        assert_eq!(map.size(), 5.0);
        assert_eq!(map.count(), 5);
    }
    #[test]
    fn length_count_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(None);
        map.splice(0, 0, &[FIVE]);
        assert_eq!(map.size(), 5.0);
        assert_eq!(map.count(), 1);
    }
    #[test]
    fn length_count_4() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(None);
        map.splice(0, 0, &[FIVE, FIVE, FIVE, FIVE, FIVE]);
        assert_eq!(map.size(), 25.0);
        assert_eq!(map.count(), 5);
    }
    #[test]
    fn insert() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(None);
        map.splice(0, 0, &[FIVE, FIVE, FIVE, FIVE, FIVE]);
        assert_eq!(map.size(), 25.0);
        assert_eq!(map.count(), 5);
        map.splice(0, 0, &[FIVE, FIVE, FIVE, FIVE, FIVE]);
        assert_eq!(map.size(), 50.0);
        assert_eq!(map.count(), 10);
        map.splice(5, 0, &[TEN, TEN]);
        assert_eq!(map.size(), 70.0);
        assert_eq!(map.count(), 12);
        map.splice(12, 0, &[IItem { size: 200.0 }]);
        assert_eq!(map.size(), 270.0);
        assert_eq!(map.count(), 13);
    }
    #[test]
    fn delete() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(None);
        map.splice(0, 0, &[FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE, FIVE]);
        assert_eq!(map.size(), 100.0);
        assert_eq!(map.count(), 20);
        map.splice(10, 5, &[]);
        assert_eq!(map.size(), 75.0);
        assert_eq!(map.count(), 15);
        map.splice(0, 1, &[]);
        assert_eq!(map.size(), 70.0);
        assert_eq!(map.count(), 14);
        map.splice(1, 13, &[]);
        assert_eq!(map.size(), 5.0);
        assert_eq!(map.count(), 1);
        map.splice(1, 1, &[]);
        assert_eq!(map.size(), 5.0);
        assert_eq!(map.count(), 1);
    }
    #[test]
    fn insert_delete() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(None);
        assert_eq!(map.size(), 0.0);
        assert_eq!(map.count(), 0);
        map.splice(0, 0, &[ONE]);
        assert_eq!(map.size(), 1.0);
        assert_eq!(map.count(), 1);
        map.splice(0, 1, &[]);
        assert_eq!(map.size(), 0.0);
        assert_eq!(map.count(), 0);
    }
    #[test]
    fn insert_delete_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(None);
        map.splice(0, 0, &[ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE]);
        map.splice(2, 6, &[]);
        assert_eq!(map.count(), 4);
        assert_eq!(map.size(), 4.0);
    }
    #[test]
    fn insert_delete_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(None);
        map.splice(0, 0, &[ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, TWO, TWO, TWO, TWO, TWO, TWO, TWO, TWO, TWO, TWO]);
        map.splice(8, 4, &[]);
        assert_eq!(map.count(), 16);
        assert_eq!(map.size(), 24.0);
    }
    #[test]
    fn insert_delete_4() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(None);
        map.splice(0, 0, &[ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, TWO, TWO, TWO, TWO, TWO, TWO, TWO, TWO, TWO, TWO]);
        map.splice(5, 0, &[THREE, THREE, THREE, THREE, THREE]);
        assert_eq!(map.count(), 25);
        assert_eq!(map.size(), 45.0);
        map.splice(4, 7, &[]);
        assert_eq!(map.count(), 18);
        assert_eq!(map.size(), 28.0);
    }
    mod index_at_position_at {
        use super::*;
        #[test]
        fn empty() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let map = RangeMap::new(None);
            assert_eq!(map.index_at(0.0), 0);
            assert_eq!(map.index_at(10.0), 0);
            assert_eq!(map.index_at(-1.0), -1);
            assert_eq!(map.position_at(0), -1.0);
            assert_eq!(map.position_at(10), -1.0);
            assert_eq!(map.position_at(-1), -1.0);
        }
        #[test]
        fn simple() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let mut map = RangeMap::new(None);
            map.splice(0, 0, &[ONE]);
            assert_eq!(map.index_at(0.0), 0);
            assert_eq!(map.index_at(1.0), 1);
            assert_eq!(map.position_at(0), 0.0);
            assert_eq!(map.position_at(1), -1.0);
        }
        #[test]
        fn simple_2() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let mut map = RangeMap::new(None);
            map.splice(0, 0, &[TEN]);
            assert_eq!(map.index_at(0.0), 0);
            assert_eq!(map.index_at(5.0), 0);
            assert_eq!(map.index_at(9.0), 0);
            assert_eq!(map.index_at(10.0), 1);
            assert_eq!(map.position_at(0), 0.0);
            assert_eq!(map.position_at(1), -1.0);
        }
        #[test]
        fn insert() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let mut map = RangeMap::new(None);
            map.splice(0, 0, &[ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE]);
            assert_eq!(map.index_at(0.0), 0);
            assert_eq!(map.index_at(1.0), 1);
            assert_eq!(map.index_at(5.0), 5);
            assert_eq!(map.index_at(9.0), 9);
            assert_eq!(map.index_at(10.0), 10);
            assert_eq!(map.index_at(11.0), 10);
            map.splice(10, 0, &[ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE]);
            assert_eq!(map.index_at(10.0), 10);
            assert_eq!(map.index_at(19.0), 19);
            assert_eq!(map.index_at(20.0), 20);
            assert_eq!(map.index_at(21.0), 20);
            assert_eq!(map.position_at(0), 0.0);
            assert_eq!(map.position_at(1), 1.0);
            assert_eq!(map.position_at(19), 19.0);
            assert_eq!(map.position_at(20), -1.0);
        }
        #[test]
        fn delete() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let mut map = RangeMap::new(None);
            map.splice(0, 0, &[ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE, ONE]);
            map.splice(2, 6, &[]);
            assert_eq!(map.index_at(0.0), 0);
            assert_eq!(map.index_at(1.0), 1);
            assert_eq!(map.index_at(3.0), 3);
            assert_eq!(map.index_at(4.0), 4);
            assert_eq!(map.index_at(5.0), 4);
            assert_eq!(map.position_at(0), 0.0);
            assert_eq!(map.position_at(1), 1.0);
            assert_eq!(map.position_at(3), 3.0);
            assert_eq!(map.position_at(4), -1.0);
        }
        #[test]
        fn delete_2() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let mut map = RangeMap::new(None);
            map.splice(0, 0, &[TEN, TEN, TEN, TEN, TEN, TEN, TEN, TEN, TEN, TEN]);
            map.splice(2, 6, &[]);
            assert_eq!(map.index_at(0.0), 0);
            assert_eq!(map.index_at(1.0), 0);
            assert_eq!(map.index_at(30.0), 3);
            assert_eq!(map.index_at(40.0), 4);
            assert_eq!(map.index_at(50.0), 4);
            assert_eq!(map.position_at(0), 0.0);
            assert_eq!(map.position_at(1), 10.0);
            assert_eq!(map.position_at(2), 20.0);
            assert_eq!(map.position_at(3), 30.0);
            assert_eq!(map.position_at(4), -1.0);
        }
    }
}
mod range_map_with_top_padding {
    use super::*;
    #[test]
    fn empty() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let map = RangeMap::new(Some(10.0));
        assert_eq!(map.size(), 10.0);
        assert_eq!(map.count(), 0);
    }
    const ONE: IItem = IItem { size: 1.0 };
    const FIVE: IItem = IItem { size: 5.0 };
    const TEN: IItem = IItem { size: 10.0 };
    #[test]
    fn length_count() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(Some(10.0));
        map.splice(0, 0, &[ONE]);
        assert_eq!(map.size(), 11.0);
        assert_eq!(map.count(), 1);
    }
    #[test]
    fn length_count_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(Some(10.0));
        map.splice(0, 0, &[ONE, ONE, ONE, ONE, ONE]);
        assert_eq!(map.size(), 15.0);
        assert_eq!(map.count(), 5);
    }
    #[test]
    fn length_count_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(Some(10.0));
        map.splice(0, 0, &[FIVE]);
        assert_eq!(map.size(), 15.0);
        assert_eq!(map.count(), 1);
    }
    #[test]
    fn length_count_4() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(Some(10.0));
        map.splice(0, 0, &[FIVE, FIVE, FIVE, FIVE, FIVE]);
        assert_eq!(map.size(), 35.0);
        assert_eq!(map.count(), 5);
    }
    #[test]
    fn insert() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mut map = RangeMap::new(Some(10.0));
        map.splice(0, 0, &[FIVE, FIVE, FIVE, FIVE, FIVE]);
        assert_eq!(map.size(), 35.0);
        assert_eq!(map.count(), 5);
        map.splice(0, 0, &[FIVE, FIVE, FIVE, FIVE, FIVE]);
        assert_eq!(map.size(), 60.0);
        assert_eq!(map.count(), 10);
        map.splice(5, 0, &[TEN, TEN]);
        assert_eq!(map.size(), 80.0);
        assert_eq!(map.count(), 12);
        map.splice(12, 0, &[IItem { size: 200.0 }]);
        assert_eq!(map.size(), 280.0);
        assert_eq!(map.count(), 13);
    }
    mod index_at_position_at {
        use super::*;
        #[test]
        fn empty() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let map = RangeMap::new(Some(10.0));
            assert_eq!(map.index_at(0.0), 0);
            assert_eq!(map.index_at(10.0), 0);
            assert_eq!(map.index_at(-1.0), -1);
            assert_eq!(map.position_at(0), -1.0);
            assert_eq!(map.position_at(10), -1.0);
            assert_eq!(map.position_at(-1), -1.0);
        }
        #[test]
        fn simple() {
            let _guard = ensure_no_disposables_are_leaked_in_test_suite();
            let mut map = RangeMap::new(Some(10.0));
            map.splice(0, 0, &[ONE]);
            assert_eq!(map.index_at(0.0), 0);
            assert_eq!(map.index_at(1.0), 0);
            assert_eq!(map.index_at(10.0), 0);
            assert_eq!(map.index_at(11.0), 1);
            assert_eq!(map.position_at(0), 10.0);
            assert_eq!(map.position_at(1), -1.0);
        }
    }
}
