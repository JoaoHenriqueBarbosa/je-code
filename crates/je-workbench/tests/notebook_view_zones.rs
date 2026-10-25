//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookViewZones.test.ts` (suítes `NotebookRangeMap`, `NotebookRangeMap with top padding` e `NotebookRangeMap with whitesspaces`), caso a caso e na mesma ordem. Os 33 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso. O `setup` da suíte de
//!   espaços (o `DisposableStore`, `setupInstantiationService` e o `TestConfigurationService`) é o `with_test_notebook`, que
//!   monta os serviços de teste e a configuração que a lista lê (`createNotebookCellList`); os dois casos sem lista não o
//!   usam.
//! - `{ size: n }` é `IItem { size }` (`ONE`, `TWO`, ...); `rangeMap.splice(index, count)` sem itens é `splice(index, count, &[])`.
//! - Os `assert.strictEqual` de números comparam `f64` e `i64` exatos (as medidas são inteiras).
//! - `viewModel.restoreEditorViewState({ ..., cellTotalHeights })` é [`restore_heights`] (os mapas vazios equivalem a
//!   `editingCells: [false, ...]` e `editorViewStates: [null, ...]`); `cellList.layout(210, 100)` é `layout(Some(210.0))`;
//!   `cellList.scrollHeight` é `get_scroll_height()`, `getElementTop(i)` é `element_top(i)` e
//!   `cellList._getView().getWhitespacePosition(id)` é `get_whitespace_position(id)`.
//! - O `domNode: document.createElement('div')` das zonas não existe na camada lógica (`INotebookViewZone` guarda só
//!   `afterModelPosition` e `heightInPx`). O caso comentado `Multiple Whitespaces 4` do original não é um caso.
//! - Os callbacks `async` do `withTestNotebook` são síncronos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::DisposableStore;
use je_widgets::list::range_map::{IItem, IRangeMap};
use je_workbench::contrib::notebook::browser::view::notebook_cell_list::NotebookCellList;
use je_workbench::contrib::notebook::browser::view::notebook_cell_list_view::NotebookCellsLayout;
use je_workbench::contrib::notebook::browser::view_model::cell_view_model_types::INotebookEditorViewState;
use je_workbench::contrib::notebook::browser::view_model::folding_model::FoldingModel;
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::NotebookViewModel;
use je_workbench::contrib::notebook::browser::view_parts::notebook_view_zones::INotebookViewZone;
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, ICellEditOperation, ICellReplaceEdit};
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use support::test_notebook_editor::{ActiveTestNotebookEditorDelegate, MockNotebookCell, create_notebook_cell_list, empty_metadata, with_test_notebook};

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

    fn md(source: &'static str) -> MockNotebookCell {
        (source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))
    }

    fn js(source: &'static str) -> MockNotebookCell {
        (source, "javascript", CellKind::Code, Vec::new(), Some(empty_metadata()))
    }

    fn range(start: i64, end: i64) -> ICellRange {
        ICellRange { start, end }
    }

    fn zone(after_model_position: i64) -> INotebookViewZone {
        INotebookViewZone { after_model_position, height_in_px: 20.0 }
    }

    /// O corpo comum dos casos de lista: o notebook de cinco células, `restoreEditorViewState` com as alturas,
    /// `createNotebookCellList`, `attachViewModel` e `layout(210, 100)` (a altura 210 renderiza 3 células inteiras e 1
    /// parcial), com a conferência do `scrollHeight` 350.
    fn run(body: impl FnOnce(&Rc<ActiveTestNotebookEditorDelegate>, &Rc<NotebookViewModel>, &Rc<DisposableStore>, &Rc<NotebookCellList>)) {
        let cells = vec![md("# header a"), js("var b = 1;"), md("# header b"), js("var b = 2;"), md("# header c")];
        with_test_notebook(cells, |editor, view_model, disposables, services| {
            view_model.restore_editor_view_state(Some(&INotebookEditorViewState {
                editing_cells: HashMap::new(),
                editor_view_states: HashMap::new(),
                cell_total_heights: Some([50.0, 100.0, 50.0, 100.0, 50.0].iter().copied().enumerate().collect()),
                cell_line_number_states: HashMap::new(),
                collapsed_input_cells: HashMap::new(),
                collapsed_output_cells: HashMap::new(),
            }));

            let cell_list = create_notebook_cell_list(services, disposables, None);
            cell_list.attach_view_model(view_model.clone());

            // render height 210, it can render 3 full cells and 1 partial cell
            cell_list.layout(Some(210.0));
            assert_eq!(cell_list.get_scroll_height(), 350.0);

            body(editor, view_model, disposables, &cell_list);
        });
    }

    /// `foldingModel.attachViewModel(viewModel); foldingModel.applyMemento([...]); viewModel.updateFoldingRanges(...)`.
    fn fold(disposables: &DisposableStore, view_model: &Rc<NotebookViewModel>, start: i64, end: i64) {
        let folding_model = disposables.add_rc(FoldingModel::new());
        folding_model.attach_view_model(view_model.clone());
        folding_model.apply_memento(&[range(start, end)]);
        view_model.update_folding_ranges(folding_model.regions());
    }

    #[test]
    fn whitespace_with_editing() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        run(|editor, _view_model, _disposables, cell_list| {
            cell_list.change_view_zones(|accessor| {
                let id = accessor.add_zone(zone(1));

                accessor.layout_zone(&id);
                assert_eq!(cell_list.get_scroll_height(), 370.0);

                assert_eq!(cell_list.element_top(0), 0.0);
                assert_eq!(cell_list.element_top(1), 70.0);
                assert_eq!(cell_list.element_top(2), 170.0);

                editor.text_model.apply_edits(&[ICellEditOperation::Replace(ICellReplaceEdit { index: 0, count: 1, cells: Vec::new() })], true, None, &|| None, None, true);

                assert_eq!(cell_list.element_top(0), 20.0);
                assert_eq!(cell_list.element_top(1), 120.0);
                assert_eq!(cell_list.element_top(2), 170.0);

                accessor.remove_zone(&id);
            });
        });
    }

    #[test]
    fn multiple_whitespaces() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        run(|_editor, _view_model, _disposables, cell_list| {
            cell_list.change_view_zones(|accessor| {
                let first = accessor.add_zone(zone(0));
                accessor.layout_zone(&first);

                let second = accessor.add_zone(zone(3));
                accessor.layout_zone(&second);

                assert_eq!(cell_list.get_scroll_height(), 390.0);

                assert_eq!(cell_list.element_top(0), 20.0);
                assert_eq!(cell_list.element_top(1), 70.0);
                assert_eq!(cell_list.element_top(2), 170.0);
                assert_eq!(cell_list.element_top(3), 240.0);

                accessor.remove_zone(&first);

                assert_eq!(cell_list.get_scroll_height(), 370.0);
                assert_eq!(cell_list.element_top(0), 0.0);
                assert_eq!(cell_list.element_top(1), 50.0);
                assert_eq!(cell_list.element_top(2), 150.0);
                assert_eq!(cell_list.element_top(3), 220.0);

                accessor.remove_zone(&second);

                assert_eq!(cell_list.get_scroll_height(), 350.0);
                assert_eq!(cell_list.element_top(3), 200.0);
            });
        });
    }

    #[test]
    fn multiple_whitespaces_2() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        run(|_editor, _view_model, _disposables, cell_list| {
            cell_list.change_view_zones(|accessor| {
                let first = accessor.add_zone(zone(0));
                accessor.layout_zone(&first);

                let second = accessor.add_zone(zone(1));
                accessor.layout_zone(&second);

                assert_eq!(cell_list.get_scroll_height(), 390.0);
                assert_eq!(cell_list.get_whitespace_position(&first), 0.0);
                assert_eq!(cell_list.get_whitespace_position(&second), 70.0);

                accessor.remove_zone(&first);
                accessor.remove_zone(&second);
            });
        });
    }

    #[test]
    fn multiple_whitespaces_3() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        run(|_editor, _view_model, _disposables, cell_list| {
            cell_list.change_view_zones(|accessor| {
                let first = accessor.add_zone(zone(1));
                accessor.layout_zone(&first);

                let second = accessor.add_zone(zone(2));
                accessor.layout_zone(&second);

                assert_eq!(cell_list.get_scroll_height(), 390.0);
                assert_eq!(cell_list.get_whitespace_position(&first), 50.0);
                assert_eq!(cell_list.get_whitespace_position(&second), 170.0);

                accessor.remove_zone(&first);
                accessor.remove_zone(&second);
            });
        });
    }

    #[test]
    fn whitespace_with_folding_support() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        run(|_editor, view_model, disposables, cell_list| {
            cell_list.change_view_zones(|accessor| {
                let id = accessor.add_zone(zone(0));

                accessor.layout_zone(&id);
                assert_eq!(cell_list.get_scroll_height(), 370.0);

                assert_eq!(cell_list.element_top(0), 20.0);
                assert_eq!(cell_list.element_top(1), 70.0);
                assert_eq!(cell_list.element_top(2), 170.0);
                assert_eq!(cell_list.element_top(3), 220.0);
                assert_eq!(cell_list.element_top(4), 320.0);

                accessor.remove_zone(&id);
                assert_eq!(cell_list.get_scroll_height(), 350.0);
            });

            cell_list.change_view_zones(|accessor| {
                let id = accessor.add_zone(zone(1));

                accessor.layout_zone(&id);
                assert_eq!(cell_list.get_scroll_height(), 370.0);

                assert_eq!(cell_list.element_top(0), 0.0);
                assert_eq!(cell_list.element_top(1), 70.0);
                assert_eq!(cell_list.element_top(2), 170.0);
                assert_eq!(cell_list.element_top(3), 220.0);
                assert_eq!(cell_list.element_top(4), 320.0);

                accessor.remove_zone(&id);
                assert_eq!(cell_list.get_scroll_height(), 350.0);
            });

            // Whitespace should be hidden if it's after the header in a folding region
            cell_list.change_view_zones(|accessor| {
                let id = accessor.add_zone(zone(3));

                accessor.layout_zone(&id);
                assert_eq!(cell_list.get_scroll_height(), 370.0);

                fold(disposables, view_model, 2, 3);
                assert_eq!(view_model.get_hidden_ranges(), vec![range(3, 3)]);
                cell_list.set_hidden_areas(&view_model.get_hidden_ranges(), true);
                assert_eq!(cell_list.get_scroll_height(), 250.0);

                assert_eq!(cell_list.element_top(0), 0.0);
                assert_eq!(cell_list.element_top(1), 50.0);
                assert_eq!(cell_list.element_top(2), 150.0);
                assert_eq!(cell_list.element_top(3), 200.0);

                cell_list.set_hidden_areas(&[], true);
                assert_eq!(cell_list.get_scroll_height(), 370.0);
                accessor.remove_zone(&id);
                assert_eq!(cell_list.get_scroll_height(), 350.0);
            });

            // Whitespace should not be hidden if it's after the last cell in a folding region
            cell_list.change_view_zones(|accessor| {
                let id = accessor.add_zone(zone(4));

                accessor.layout_zone(&id);
                assert_eq!(cell_list.get_scroll_height(), 370.0);

                fold(disposables, view_model, 2, 3);
                assert_eq!(view_model.get_hidden_ranges(), vec![range(3, 3)]);
                cell_list.set_hidden_areas(&view_model.get_hidden_ranges(), true);
                assert_eq!(cell_list.get_scroll_height(), 270.0);

                assert_eq!(cell_list.element_top(0), 0.0);
                assert_eq!(cell_list.element_top(1), 50.0);
                assert_eq!(cell_list.element_top(2), 150.0);
                assert_eq!(cell_list.element_top(3), 220.0);

                cell_list.set_hidden_areas(&[], true);
                assert_eq!(cell_list.get_scroll_height(), 370.0);
                accessor.remove_zone(&id);
                assert_eq!(cell_list.get_scroll_height(), 350.0);
            });

            // Whitespace move when previous folding regions fold
            cell_list.change_view_zones(|accessor| {
                let id = accessor.add_zone(zone(4));

                accessor.layout_zone(&id);
                assert_eq!(cell_list.get_scroll_height(), 370.0);

                fold(disposables, view_model, 0, 1);
                assert_eq!(view_model.get_hidden_ranges(), vec![range(1, 1)]);
                cell_list.set_hidden_areas(&view_model.get_hidden_ranges(), true);
                assert_eq!(cell_list.get_scroll_height(), 270.0);

                assert_eq!(cell_list.element_top(0), 0.0);
                assert_eq!(cell_list.element_top(1), 50.0);
                assert_eq!(cell_list.element_top(2), 100.0);
                assert_eq!(cell_list.element_top(3), 220.0);

                cell_list.set_hidden_areas(&[], true);
                assert_eq!(cell_list.get_scroll_height(), 370.0);
                accessor.remove_zone(&id);
                assert_eq!(cell_list.get_scroll_height(), 350.0);
            });
        });
    }

    #[test]
    fn whitespace_with_multiple_viewzones_at_same_position() {
        let _disposables = ensure_no_disposables_are_leaked_in_test_suite();
        run(|_editor, _view_model, _disposables, cell_list| {
            cell_list.change_view_zones(|accessor| {
                let first = accessor.add_zone(zone(0));

                accessor.layout_zone(&first);
                assert_eq!(cell_list.get_scroll_height(), 370.0);

                let second = accessor.add_zone(zone(0));
                accessor.layout_zone(&second);
                assert_eq!(cell_list.get_scroll_height(), 390.0);

                assert_eq!(cell_list.element_top(0), 40.0);
                assert_eq!(cell_list.element_top(1), 90.0);
                assert_eq!(cell_list.element_top(2), 190.0);
                assert_eq!(cell_list.element_top(3), 240.0);
                assert_eq!(cell_list.element_top(4), 340.0);

                accessor.remove_zone(&first);
                assert_eq!(cell_list.get_scroll_height(), 370.0);
                accessor.remove_zone(&second);
                assert_eq!(cell_list.get_scroll_height(), 350.0);
            });
        });
    }
}
