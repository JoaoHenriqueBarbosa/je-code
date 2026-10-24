//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookEditor.test.ts` (suíte `ListViewInfoAccessor`), caso a caso e na mesma ordem. O caso `basics` está portado.
//!
//! Casos de fora:
//! - Suíte `NotebookEditor focus` (`includes detached cell popups without including unrelated elements`): cria o `NotebookEditor` (o painel
//!   do workbench), anexa elementos DOM reais ao `document.body` e confere `document.activeElement` e `pane.hasFocus()` sobre eles. É DOM
//!   puro e a UI nativa não tem esse contrato.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` do caso, e `await withTestNotebook(cells, (editor, viewModel, ds) => ...)`
//!   é o `with_test_notebook` com o corpo síncrono.
//! - `ds.add(createNotebookCellList(...))` é o `create_notebook_cell_list` de `support/test_notebook_editor.rs`, e
//!   `new class extends mock<INotebookEditor>() { getViewIndexByModelIndex; getCellRangeFromViewRange; cellAt }` é o
//!   [`MockNotebookEditor`] (`IActiveNotebookEditor`), em que só esses três membros respondem e todo outro lança `TypeError`.
//! - `assert.deepStrictEqual(a, b)` entre listas de células é `a == b` (a identidade do `Rc` de cada célula).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::errors::{self, Error};
use je_workbench::contrib::notebook::browser::controller::{IActiveNotebookEditor, expand_cell_ranges_with_hidden_cells};
use je_workbench::contrib::notebook::browser::view::notebook_cell_list::ListViewInfoAccessor;
use je_workbench::contrib::notebook::browser::view_model::folding_model::{FoldingModel, update_folding_state_at_index};
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::{CellViewModel, NotebookViewModel};
use je_workbench::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use je_workbench::contrib::notebook::common::notebook_common::CellKind;
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use support::test_notebook_editor::{MockNotebookCell, create_notebook_cell_list, empty_metadata, with_test_notebook};

fn absent(member: &str) -> ! {
    errors::throw(Error::type_error(format!("{member} is not a function")))
}

fn range(start: i64, end: i64) -> ICellRange {
    ICellRange { start, end }
}

fn md(source: &'static str) -> MockNotebookCell {
    (source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))
}

fn js(source: &'static str) -> MockNotebookCell {
    (source, "javascript", CellKind::Code, Vec::new(), Some(empty_metadata()))
}

/// `new class extends mock<INotebookEditor>() { ... }`.
struct MockNotebookEditor {
    view_model: Rc<NotebookViewModel>,
    list_view_info_accessor: Rc<ListViewInfoAccessor>,
}

impl IActiveNotebookEditor for MockNotebookEditor {
    fn get_id(&self) -> String {
        absent("getId")
    }

    fn text_model(&self) -> Rc<NotebookTextModel> {
        absent("textModel")
    }

    fn is_read_only(&self) -> bool {
        absent("isReadOnly")
    }

    fn get_selections(&self) -> Vec<ICellRange> {
        absent("getSelections")
    }

    fn get_focus(&self) -> ICellRange {
        absent("getFocus")
    }

    fn get_length(&self) -> usize {
        absent("getLength")
    }

    fn cell_at(&self, index: i64) -> Option<CellViewModel> {
        self.view_model.cell_at(index)
    }

    fn get_cell_index(&self, _cell: &CellViewModel) -> i64 {
        absent("getCellIndex")
    }

    fn get_cells_in_range(&self, _range: Option<ICellRange>) -> Vec<CellViewModel> {
        absent("getCellsInRange")
    }

    fn get_view_model(&self) -> Rc<NotebookViewModel> {
        absent("getViewModel")
    }

    fn get_view_index_by_model_index(&self, index: i64) -> i64 {
        self.list_view_info_accessor.get_view_index(&self.view_model.view_cells()[index as usize])
    }

    fn get_cell_range_from_view_range(&self, start_index: i64, end_index: i64) -> Option<ICellRange> {
        self.list_view_info_accessor.get_cell_range_from_view_range(start_index, end_index)
    }

    fn reveal_cell_range_in_view(&self, _range: ICellRange) {
        absent("revealCellRangeInView")
    }

    fn focus_notebook_cell(&self, _cell: &CellViewModel, _focus_item: &str) {
        absent("focusNotebookCell")
    }

    fn active_kernel_supported_languages(&self) -> Option<Vec<String>> {
        absent("activeKernelSupportedLanguages")
    }
}

#[test]
fn basics() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![md("# header a"), js("var b = 1;"), md("# header b"), js("var b = 2;"), js("var c = 3;")],
        |_editor, view_model, ds, services| {
            let folding_model = ds.add_rc(FoldingModel::new());
            folding_model.attach_view_model(view_model.clone());

            let cell_list = create_notebook_cell_list(services, ds, None);
            cell_list.attach_view_model(view_model.clone());
            let list_view_info_accessor = Rc::new(ListViewInfoAccessor::new(cell_list.clone()));
            let cell = |index: i64| view_model.cell_at(index).expect("cell");

            assert_eq!(list_view_info_accessor.get_view_index(&cell(0)), 0);
            assert_eq!(list_view_info_accessor.get_view_index(&cell(1)), 1);
            assert_eq!(list_view_info_accessor.get_view_index(&cell(2)), 2);
            assert_eq!(list_view_info_accessor.get_view_index(&cell(3)), 3);
            assert_eq!(list_view_info_accessor.get_view_index(&cell(4)), 4);
            assert_eq!(list_view_info_accessor.get_cell_range_from_view_range(0, 1), Some(range(0, 1)));
            assert_eq!(list_view_info_accessor.get_cell_range_from_view_range(1, 2), Some(range(1, 2)));

            update_folding_state_at_index(&folding_model, 0, true);
            update_folding_state_at_index(&folding_model, 2, true);
            view_model.update_folding_ranges(folding_model.regions());
            cell_list.set_hidden_areas(&view_model.get_hidden_ranges(), true);

            assert_eq!(list_view_info_accessor.get_view_index(&cell(0)), 0);
            assert_eq!(list_view_info_accessor.get_view_index(&cell(1)), -1);
            assert_eq!(list_view_info_accessor.get_view_index(&cell(2)), 1);
            assert_eq!(list_view_info_accessor.get_view_index(&cell(3)), -1);
            assert_eq!(list_view_info_accessor.get_view_index(&cell(4)), -1);

            assert_eq!(list_view_info_accessor.get_cell_range_from_view_range(0, 1), Some(range(0, 2)));
            assert_eq!(list_view_info_accessor.get_cell_range_from_view_range(1, 2), Some(range(2, 5)));
            assert!(list_view_info_accessor.get_cells_from_view_range(0, 1) == view_model.get_cells_in_range(Some(range(0, 2))));
            assert!(list_view_info_accessor.get_cells_from_view_range(1, 2) == view_model.get_cells_in_range(Some(range(2, 5))));

            let notebook_editor = MockNotebookEditor { view_model: view_model.clone(), list_view_info_accessor: list_view_info_accessor.clone() };

            assert_eq!(expand_cell_ranges_with_hidden_cells(&notebook_editor, &[range(0, 1)]), vec![range(0, 2)]);
            assert_eq!(expand_cell_ranges_with_hidden_cells(&notebook_editor, &[range(2, 3)]), vec![range(2, 5)]);
            assert_eq!(expand_cell_ranges_with_hidden_cells(&notebook_editor, &[range(0, 1), range(2, 3)]), vec![range(0, 5)]);
        },
    );
}
