//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookViewZones.test.ts` (suítes `NotebookRangeMap`, `NotebookRangeMap with top padding` e `NotebookRangeMap with whitesspaces`), caso a caso e na mesma ordem.
//!
//! Casos de fora (6 dos 33), todos da suíte `NotebookRangeMap with whitesspaces` e todos dependentes do widget de lista de
//! células (`createNotebookCellList` com o `NotebookCellList` de DOM, `changeViewZones`, `scrollHeight`, `getElementTop`,
//! `restoreEditorViewState` do view model e o `FoldingModel`), que são a Fase 6: `Whitespace with editing`, `Multiple
//! Whitespaces`, `Multiple Whitespaces 2`, `Multiple Whitespaces 3`, `Whitespace with folding support` e `Whitespace with
//! multiple viewzones at same position`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso. O `setup` da suíte de
//!   espaços (o `DisposableStore`, `setupInstantiationService` e o `TestConfigurationService`) só serve aos casos de fora
//!   (`createNotebookCellList`), e os dois casos portados não o leem; não é portado.
//! - `{ size: n }` é `IItem { size }` (`ONE`, `TWO`, ...); `rangeMap.splice(index, count)` sem itens é `splice(index, count, &[])`.
//! - Os `assert.strictEqual` de números comparam `f64` e `i64` exatos (as medidas são inteiras).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_widgets::list::range_map::{IItem, IRangeMap};
use je_workbench::contrib::notebook::browser::view::notebook_cell_list_view::NotebookCellsLayout;

const ONE: IItem = IItem { size: 1.0 };
const TWO: IItem = IItem { size: 2.0 };
const THREE: IItem = IItem { size: 3.0 };
const FIVE: IItem = IItem { size: 5.0 };
const TEN: IItem = IItem { size: 10.0 };

mod notebook_range_map {
    use super::*;

    #[test]
    fn empty() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let range_map = NotebookCellsLayout::new(None);
        assert_eq!(range_map.size(), 0.0);
        assert_eq!(range_map.count(), 0);
    }

    #[test]
    fn length_and_count() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(None);
        range_map.splice(0, 0, &[ONE]);
        assert_eq!(range_map.size(), 1.0);
        assert_eq!(range_map.count(), 1);
    }

    #[test]
    fn length_and_count_2() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(None);
        range_map.splice(0, 0, &[ONE, ONE, ONE, ONE, ONE]);
        assert_eq!(range_map.size(), 5.0);
        assert_eq!(range_map.count(), 5);
    }

    #[test]
    fn length_and_count_3() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(None);
        range_map.splice(0, 0, &[FIVE]);
        assert_eq!(range_map.size(), 5.0);
        assert_eq!(range_map.count(), 1);
    }

    #[test]
    fn length_and_count_4() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(None);
        range_map.splice(0, 0, &[FIVE, FIVE, FIVE, FIVE, FIVE]);
        assert_eq!(range_map.size(), 25.0);
        assert_eq!(range_map.count(), 5);
    }

    #[test]
    fn insert() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(None);
        range_map.splice(0, 0, &[FIVE, FIVE, FIVE, FIVE, FIVE]);
        assert_eq!(range_map.size(), 25.0);
        assert_eq!(range_map.count(), 5);

        range_map.splice(0, 0, &[FIVE, FIVE, FIVE, FIVE, FIVE]);
        assert_eq!(range_map.size(), 50.0);
        assert_eq!(range_map.count(), 10);

        range_map.splice(5, 0, &[TEN, TEN]);
        assert_eq!(range_map.size(), 70.0);
        assert_eq!(range_map.count(), 12);

        range_map.splice(12, 0, &[IItem { size: 200.0 }]);
        assert_eq!(range_map.size(), 270.0);
        assert_eq!(range_map.count(), 13);
    }

    #[test]
    fn delete() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(None);
        range_map.splice(0, 0, &[FIVE; 20]);
        assert_eq!(range_map.size(), 100.0);
        assert_eq!(range_map.count(), 20);

        range_map.splice(10, 5, &[]);
        assert_eq!(range_map.size(), 75.0);
        assert_eq!(range_map.count(), 15);

        range_map.splice(0, 1, &[]);
        assert_eq!(range_map.size(), 70.0);
        assert_eq!(range_map.count(), 14);

        range_map.splice(1, 13, &[]);
        assert_eq!(range_map.size(), 5.0);
        assert_eq!(range_map.count(), 1);

        range_map.splice(1, 1, &[]);
        assert_eq!(range_map.size(), 5.0);
        assert_eq!(range_map.count(), 1);
    }

    #[test]
    fn insert_and_delete() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(None);
        assert_eq!(range_map.size(), 0.0);
        assert_eq!(range_map.count(), 0);

        range_map.splice(0, 0, &[ONE]);
        assert_eq!(range_map.size(), 1.0);
        assert_eq!(range_map.count(), 1);

        range_map.splice(0, 1, &[]);
        assert_eq!(range_map.size(), 0.0);
        assert_eq!(range_map.count(), 0);
    }

    #[test]
    fn insert_and_delete_2() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(None);
        range_map.splice(0, 0, &[ONE; 10]);
        range_map.splice(2, 6, &[]);
        assert_eq!(range_map.count(), 4);
        assert_eq!(range_map.size(), 4.0);
    }

    /// Os vinte itens `[one x 10, two x 10]` dos casos `insert & delete #3` e `#4`.
    fn ones_and_twos() -> Vec<IItem> {
        let mut items = vec![ONE; 10];
        items.extend([TWO; 10]);
        items
    }

    #[test]
    fn insert_and_delete_3() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(None);
        range_map.splice(0, 0, &ones_and_twos());
        range_map.splice(8, 4, &[]);
        assert_eq!(range_map.count(), 16);
        assert_eq!(range_map.size(), 24.0);
    }

    #[test]
    fn insert_and_delete_4() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(None);
        range_map.splice(0, 0, &ones_and_twos());
        range_map.splice(5, 0, &[THREE, THREE, THREE, THREE, THREE]);
        assert_eq!(range_map.count(), 25);
        assert_eq!(range_map.size(), 45.0);

        range_map.splice(4, 7, &[]);
        assert_eq!(range_map.count(), 18);
        assert_eq!(range_map.size(), 28.0);
    }

    mod index_at_position_at {
        use super::*;

        #[test]
        fn empty() {
            let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
            let range_map = NotebookCellsLayout::new(None);
            assert_eq!(range_map.index_at(0.0), 0);
            assert_eq!(range_map.index_at(10.0), 0);
            assert_eq!(range_map.index_at(-1.0), -1);
            assert_eq!(range_map.position_at(0), -1.0);
            assert_eq!(range_map.position_at(10), -1.0);
            assert_eq!(range_map.position_at(-1), -1.0);
        }

        #[test]
        fn simple() {
            let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
            let mut range_map = NotebookCellsLayout::new(None);
            range_map.splice(0, 0, &[ONE]);
            assert_eq!(range_map.index_at(0.0), 0);
            assert_eq!(range_map.index_at(1.0), 1);
            assert_eq!(range_map.position_at(0), 0.0);
            assert_eq!(range_map.position_at(1), -1.0);
        }

        #[test]
        fn simple_2() {
            let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
            let mut range_map = NotebookCellsLayout::new(None);
            range_map.splice(0, 0, &[TEN]);
            assert_eq!(range_map.index_at(0.0), 0);
            assert_eq!(range_map.index_at(5.0), 0);
            assert_eq!(range_map.index_at(9.0), 0);
            assert_eq!(range_map.index_at(10.0), 1);
            assert_eq!(range_map.position_at(0), 0.0);
            assert_eq!(range_map.position_at(1), -1.0);
        }

        #[test]
        fn insert() {
            let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
            let mut range_map = NotebookCellsLayout::new(None);
            range_map.splice(0, 0, &[ONE; 10]);
            assert_eq!(range_map.index_at(0.0), 0);
            assert_eq!(range_map.index_at(1.0), 1);
            assert_eq!(range_map.index_at(5.0), 5);
            assert_eq!(range_map.index_at(9.0), 9);
            assert_eq!(range_map.index_at(10.0), 10);
            assert_eq!(range_map.index_at(11.0), 10);

            range_map.splice(10, 0, &[ONE; 10]);
            assert_eq!(range_map.index_at(10.0), 10);
            assert_eq!(range_map.index_at(19.0), 19);
            assert_eq!(range_map.index_at(20.0), 20);
            assert_eq!(range_map.index_at(21.0), 20);
            assert_eq!(range_map.position_at(0), 0.0);
            assert_eq!(range_map.position_at(1), 1.0);
            assert_eq!(range_map.position_at(19), 19.0);
            assert_eq!(range_map.position_at(20), -1.0);
        }

        #[test]
        fn delete() {
            let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
            let mut range_map = NotebookCellsLayout::new(None);
            range_map.splice(0, 0, &[ONE; 10]);
            range_map.splice(2, 6, &[]);

            assert_eq!(range_map.index_at(0.0), 0);
            assert_eq!(range_map.index_at(1.0), 1);
            assert_eq!(range_map.index_at(3.0), 3);
            assert_eq!(range_map.index_at(4.0), 4);
            assert_eq!(range_map.index_at(5.0), 4);
            assert_eq!(range_map.position_at(0), 0.0);
            assert_eq!(range_map.position_at(1), 1.0);
            assert_eq!(range_map.position_at(3), 3.0);
            assert_eq!(range_map.position_at(4), -1.0);
        }

        #[test]
        fn delete_2() {
            let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
            let mut range_map = NotebookCellsLayout::new(None);
            range_map.splice(0, 0, &[TEN; 10]);
            range_map.splice(2, 6, &[]);

            assert_eq!(range_map.index_at(0.0), 0);
            assert_eq!(range_map.index_at(1.0), 0);
            assert_eq!(range_map.index_at(30.0), 3);
            assert_eq!(range_map.index_at(40.0), 4);
            assert_eq!(range_map.index_at(50.0), 4);
            assert_eq!(range_map.position_at(0), 0.0);
            assert_eq!(range_map.position_at(1), 10.0);
            assert_eq!(range_map.position_at(2), 20.0);
            assert_eq!(range_map.position_at(3), 30.0);
            assert_eq!(range_map.position_at(4), -1.0);
        }
    }
}

mod notebook_range_map_with_top_padding {
    use super::*;

    #[test]
    fn empty() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let range_map = NotebookCellsLayout::new(Some(10.0));
        assert_eq!(range_map.size(), 10.0);
        assert_eq!(range_map.count(), 0);
    }

    #[test]
    fn length_and_count() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(Some(10.0));
        range_map.splice(0, 0, &[ONE]);
        assert_eq!(range_map.size(), 11.0);
        assert_eq!(range_map.count(), 1);
    }

    #[test]
    fn length_and_count_2() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(Some(10.0));
        range_map.splice(0, 0, &[ONE, ONE, ONE, ONE, ONE]);
        assert_eq!(range_map.size(), 15.0);
        assert_eq!(range_map.count(), 5);
    }

    #[test]
    fn length_and_count_3() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(Some(10.0));
        range_map.splice(0, 0, &[FIVE]);
        assert_eq!(range_map.size(), 15.0);
        assert_eq!(range_map.count(), 1);
    }

    #[test]
    fn length_and_count_4() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(Some(10.0));
        range_map.splice(0, 0, &[FIVE, FIVE, FIVE, FIVE, FIVE]);
        assert_eq!(range_map.size(), 35.0);
        assert_eq!(range_map.count(), 5);
    }

    #[test]
    fn insert() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(Some(10.0));
        range_map.splice(0, 0, &[FIVE, FIVE, FIVE, FIVE, FIVE]);
        assert_eq!(range_map.size(), 35.0);
        assert_eq!(range_map.count(), 5);

        range_map.splice(0, 0, &[FIVE, FIVE, FIVE, FIVE, FIVE]);
        assert_eq!(range_map.size(), 60.0);
        assert_eq!(range_map.count(), 10);

        range_map.splice(5, 0, &[TEN, TEN]);
        assert_eq!(range_map.size(), 80.0);
        assert_eq!(range_map.count(), 12);

        range_map.splice(12, 0, &[IItem { size: 200.0 }]);
        assert_eq!(range_map.size(), 280.0);
        assert_eq!(range_map.count(), 13);
    }

    mod index_at_position_at {
        use super::*;

        #[test]
        fn empty() {
            let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
            let range_map = NotebookCellsLayout::new(Some(10.0));
            assert_eq!(range_map.index_at(0.0), 0);
            assert_eq!(range_map.index_at(10.0), 0);
            assert_eq!(range_map.index_at(-1.0), -1);
            assert_eq!(range_map.position_at(0), -1.0);
            assert_eq!(range_map.position_at(10), -1.0);
            assert_eq!(range_map.position_at(-1), -1.0);
        }

        #[test]
        fn simple() {
            let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
            let mut range_map = NotebookCellsLayout::new(Some(10.0));
            range_map.splice(0, 0, &[ONE]);
            assert_eq!(range_map.index_at(0.0), 0);
            assert_eq!(range_map.index_at(1.0), 0);
            assert_eq!(range_map.index_at(10.0), 0);
            assert_eq!(range_map.index_at(11.0), 1);
            assert_eq!(range_map.position_at(0), 10.0);
            assert_eq!(range_map.position_at(1), -1.0);
        }
    }
}

mod notebook_range_map_with_whitesspaces {
    use super::*;

    #[test]
    fn simple() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let mut range_map = NotebookCellsLayout::new(Some(0.0));
        range_map.splice(
            0,
            0,
            &[
                IItem { size: 479.0 },
                IItem { size: 163.0 },
                IItem { size: 182.0 },
                IItem { size: 106.0 },
                IItem { size: 106.0 },
                IItem { size: 106.0 },
                IItem { size: 87.0 },
            ],
        );

        let start = range_map.index_at(650.0);
        let end = range_map.index_after(650.0 + 890.0 - 1.0);
        assert_eq!(start, 2);
        assert_eq!(end, 7);

        range_map.insert_whitespace("1", 0, 18.0);
        assert_eq!(range_map.index_at(650.0), 1);
    }

    #[test]
    fn whitespace_crud() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        let twenty = IItem { size: 20.0 };

        let mut range_map = NotebookCellsLayout::new(Some(0.0));
        range_map.splice(0, 0, &[twenty, twenty, twenty]);
        range_map.insert_whitespace("0", 0, 5.0);
        range_map.insert_whitespace("1", 0, 5.0);
        assert_eq!(range_map.index_at(0.0), 0);
        assert_eq!(range_map.index_at(1.0), 0);
        assert_eq!(range_map.index_at(10.0), 0);
        assert_eq!(range_map.index_at(11.0), 0);
        assert_eq!(range_map.index_at(21.0), 0);
        assert_eq!(range_map.index_at(31.0), 1);
        assert_eq!(range_map.position_at(0), 10.0);

        assert_eq!(range_map.get_whitespace_position("0"), 0.0);
        assert_eq!(range_map.get_whitespace_position("1"), 5.0);

        assert_eq!(range_map.position_at(0), 10.0);
        assert_eq!(range_map.position_at(1), 30.0);

        range_map.change_one_whitespace("0", 0, 10.0);
        assert_eq!(range_map.get_whitespace_position("0"), 0.0);
        assert_eq!(range_map.get_whitespace_position("1"), 10.0);

        assert_eq!(range_map.position_at(0), 15.0);
        assert_eq!(range_map.position_at(1), 35.0);

        range_map.remove_whitespace("1");
        assert_eq!(range_map.get_whitespace_position("0"), 0.0);

        assert_eq!(range_map.position_at(0), 10.0);
        assert_eq!(range_map.position_at(1), 30.0);
    }
}
