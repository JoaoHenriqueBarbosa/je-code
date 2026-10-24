//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookCellList.test.ts` (suíte `NotebookCellList`), caso a caso e
//! na mesma ordem. Os 11 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso (`_store`); o `setup` (o
//!   `DisposableStore`, o `setupInstantiationService` e o `config = new TestConfigurationService()` registrado com
//!   `instantiationService.stub(IConfigurationService, config)`) é o `with_test_notebook` mais o `config` de [`run`], que a
//!   lista lê pelo `NotebookCellAnchor` e pelo `NotebookOptions` (`create_notebook_cell_list_with_configuration`).
//! - `restoreEditorViewState({ editingCells: [false, ...], editorViewStates: [null, ...], cellTotalHeights: [...], ... })` é
//!   [`restore_heights`]: `editingCells` e `collapsed*` com `false` e `editorViewStates` com `null` equivalem aos mapas
//!   vazios (a leitura por índice dá `false`/ausente), e `cellTotalHeights` vira o mapa índice para altura.
//! - `cellList.layout(210, 100)` é `layout(Some(210.0))` (a largura só serve ao `_rerender` das alturas dinâmicas, que a
//!   camada lógica não tem). `cellList.scrollTop = 5` é `set_scroll_top(5.0)`; `updateElementHeight(0, 60)` do `List` é
//!   `update_element_height_by_index`; `updateElementHeight2(cell, size)` é `update_element_height2(&cell, size, None)`;
//!   `cellList.setFocus([1])` é `set_focus(&[1], false)`.
//! - `config.setUserConfiguration(NotebookSetting.scrollToRevealCell, 'none')` é `set_user_configuration` com a string.
//! - Os callbacks `async` do `withTestNotebook` são síncronos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_workbench::contrib::notebook::browser::controller::IActiveNotebookEditor;
use je_workbench::contrib::notebook::browser::view::notebook_cell_list::{CellListOptionsUpdate, NotebookCellList};
use je_workbench::contrib::notebook::browser::view_model::cell_view_model_types::INotebookEditorViewState;
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::NotebookViewModel;
use je_workbench::contrib::notebook::common::notebook_common::{CellKind, NotebookSetting};
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use support::test_notebook_editor::{ActiveTestNotebookEditorDelegate, MockNotebookCell, create_notebook_cell_list_with_configuration, empty_metadata, with_test_notebook};

fn md(source: &'static str) -> MockNotebookCell {
    (source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))
}

fn js(source: &'static str) -> MockNotebookCell {
    (source, "javascript", CellKind::Code, Vec::new(), Some(empty_metadata()))
}

fn five_cells() -> Vec<MockNotebookCell> {
    vec![md("# header a"), js("var b = 1;"), md("# header b"), js("var b = 2;"), md("# header c")]
}

fn range(start: i64, end: i64) -> ICellRange {
    ICellRange { start, end }
}

/// `viewModel.restoreEditorViewState({ ..., cellTotalHeights })`.
fn restore_heights(view_model: &NotebookViewModel, heights: &[f64]) {
    view_model.restore_editor_view_state(Some(&INotebookEditorViewState {
        editing_cells: HashMap::new(),
        editor_view_states: HashMap::new(),
        cell_total_heights: Some(heights.iter().copied().enumerate().collect()),
        cell_line_number_states: HashMap::new(),
        collapsed_input_cells: HashMap::new(),
        collapsed_output_cells: HashMap::new(),
    }));
}

/// O corpo comum dos casos: o `config` (com `scrollToRevealCell = 'none'` se pedido), o notebook de teste, o
/// `createNotebookCellList(instantiationService, disposables)` (ainda sem o view model).
fn run(
    cells: Vec<MockNotebookCell>,
    scroll_to_reveal_cell_none: bool,
    body: impl FnOnce(&Rc<ActiveTestNotebookEditorDelegate>, &Rc<NotebookViewModel>, &Rc<NotebookCellList>),
) {
    let config = TestConfigurationService::new(None);
    if scroll_to_reveal_cell_none {
        config.set_user_configuration(NotebookSetting::SCROLL_TO_REVEAL_CELL, Dynamic::string("none"), None);
    }
    with_test_notebook(cells, |editor, view_model, ds, services| {
        ds.add_rc(config.on_did_change_configuration_emitter.clone());
        let configuration_service: Rc<dyn IConfigurationService> = config.clone();
        let cell_list = create_notebook_cell_list_with_configuration(services, ds, None, configuration_service);
        body(editor, view_model, &cell_list);
    });
}

#[test]
fn reveal_elements_in_view_reveal_fully_visible_cell_should_not_scroll() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    run(five_cells(), false, |_editor, view_model, cell_list| {
        restore_heights(view_model, &[50.0, 100.0, 50.0, 100.0, 50.0]);

        cell_list.attach_view_model(view_model.clone());

        // render height 210, it can render 3 full cells and 1 partial cell
        cell_list.layout(Some(210.0));
        // scroll a bit, scrollTop to bottom: 5, 215
        cell_list.set_scroll_top(5.0);

        // init scrollTop and scrollBottom
        assert_eq!(cell_list.scroll_top(), 5.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 215.0);

        // reveal cell 1, top 50, bottom 150, which is fully visible in the viewport
        cell_list.reveal_cells(range(1, 2));
        assert_eq!(cell_list.scroll_top(), 5.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 215.0);

        // reveal cell 2, top 150, bottom 200, which is fully visible in the viewport
        cell_list.reveal_cells(range(2, 3));
        assert_eq!(cell_list.scroll_top(), 5.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 215.0);

        // reveal cell 3, top 200, bottom 300, which is partially visible in the viewport
        cell_list.reveal_cells(range(3, 4));
        assert_eq!(cell_list.scroll_top(), 90.0);
    });
}

#[test]
fn reveal_elements_in_view_reveal_partially_visible_cell() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    run(five_cells(), false, |_editor, view_model, cell_list| {
        restore_heights(view_model, &[50.0, 100.0, 50.0, 100.0, 50.0]);

        cell_list.attach_view_model(view_model.clone());

        // render height 210, it can render 3 full cells and 1 partial cell
        cell_list.layout(Some(210.0));

        // init scrollTop and scrollBottom
        assert_eq!(cell_list.scroll_top(), 0.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 210.0);

        // reveal cell 3, top 200, bottom 300, which is partially visible in the viewport
        cell_list.reveal_cells(range(3, 4));
        assert_eq!(cell_list.scroll_top(), 90.0);

        // scroll to 5
        cell_list.set_scroll_top(5.0);
        assert_eq!(cell_list.scroll_top(), 5.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 215.0);

        // reveal cell 0, top 0, bottom 50
        cell_list.reveal_cells(range(0, 1));
        assert_eq!(cell_list.scroll_top(), 0.0);
    });
}

#[test]
fn reveal_elements_in_view_reveal_cell_out_of_viewport() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    run(five_cells(), false, |_editor, view_model, cell_list| {
        restore_heights(view_model, &[50.0, 100.0, 50.0, 100.0, 50.0]);

        // without paddingBottom, the last 20 px will always be hidden due to `topInsertToolbarHeight`
        cell_list.update_options(&CellListOptionsUpdate { padding_bottom: Some(100.0) });
        cell_list.attach_view_model(view_model.clone());

        // render height 210, it can render 3 full cells and 1 partial cell
        cell_list.layout(Some(210.0));

        // init scrollTop and scrollBottom
        assert_eq!(cell_list.scroll_top(), 0.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 210.0);

        cell_list.reveal_cells(range(4, 5));
        assert_eq!(cell_list.scroll_top(), 140.0);
        // assert.deepStrictEqual(cellList.getViewScrollBottom(), 330);
    });
}

#[test]
fn update_element_height() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    run(five_cells(), false, |_editor, view_model, cell_list| {
        restore_heights(view_model, &[50.0, 100.0, 50.0, 100.0, 50.0]);

        cell_list.attach_view_model(view_model.clone());

        // render height 210, it can render 3 full cells and 1 partial cell
        cell_list.layout(Some(210.0));

        // init scrollTop and scrollBottom
        assert_eq!(cell_list.scroll_top(), 0.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 210.0);

        cell_list.update_element_height_by_index(0, 60.0);
        assert_eq!(cell_list.scroll_top(), 0.0);

        // scroll to 5
        cell_list.set_scroll_top(5.0);
        assert_eq!(cell_list.scroll_top(), 5.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 215.0);

        cell_list.update_element_height_by_index(0, 80.0);
        assert_eq!(cell_list.scroll_top(), 5.0);
    });
}

#[test]
fn update_element_height_with_anchor() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    run(five_cells(), false, |_editor, view_model, cell_list| {
        restore_heights(view_model, &[50.0, 100.0, 50.0, 100.0, 50.0]);

        cell_list.attach_view_model(view_model.clone());

        // render height 210, it can render 3 full cells and 1 partial cell
        cell_list.layout(Some(210.0));

        // init scrollTop and scrollBottom
        assert_eq!(cell_list.scroll_top(), 0.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 210.0);

        // scroll to 5
        cell_list.update_element_height2(&view_model.cell_at(0).expect("cell 0"), 50.0, None);
        cell_list.set_scroll_top(5.0);
        assert_eq!(cell_list.scroll_top(), 5.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 215.0);

        cell_list.set_focus(&[1], false);
        cell_list.update_element_height2(&view_model.cell_at(0).expect("cell 0"), 100.0, None);
        assert_eq!(cell_list.get_scroll_height(), 400.0);

        // the first cell grows, and the focused cell will remain fully visible, so we don't scroll
        assert_eq!(cell_list.scroll_top(), 5.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 215.0);

        cell_list.update_element_height2(&view_model.cell_at(0).expect("cell 0"), 150.0, None);
        // the first cell grows, and the focused cell will be pushed out of view, so we scroll down
        assert_eq!(cell_list.scroll_top(), 55.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 265.0);

        // We don't anchor to the focused cell when cells shrink
        cell_list.update_element_height2(&view_model.cell_at(0).expect("cell 0"), 50.0, None);
        assert_eq!(cell_list.scroll_top(), 55.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 265.0);

        // focus won't be visible after cell 0 grow to 250, so let's try to keep the focused cell visible
        cell_list.update_element_height2(&view_model.cell_at(0).expect("cell 0"), 250.0, None);
        assert_eq!(cell_list.scroll_top(), 250.0 + 100.0 - cell_list.get_render_height());
        assert_eq!(cell_list.get_view_scroll_bottom(), 250.0 + 100.0 - cell_list.get_render_height() + 210.0);
    });
}

#[test]
fn update_element_height_with_no_scrolling() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    run(five_cells(), true, |_editor, view_model, cell_list| {
        restore_heights(view_model, &[50.0, 100.0, 50.0, 100.0, 50.0]);

        cell_list.attach_view_model(view_model.clone());

        // render height 210, it can render 3 full cells and 1 partial cell
        cell_list.layout(Some(210.0));

        // init scrollTop and scrollBottom
        assert_eq!(cell_list.scroll_top(), 0.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 210.0);

        // scroll to 5
        cell_list.update_element_height2(&view_model.cell_at(0).expect("cell 0"), 50.0, None);
        cell_list.set_scroll_top(5.0);
        assert_eq!(cell_list.scroll_top(), 5.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 215.0);

        cell_list.set_focus(&[1], false);
        cell_list.update_element_height2(&view_model.cell_at(0).expect("cell 0"), 100.0, None);
        assert_eq!(cell_list.get_scroll_height(), 400.0);

        // Any change in cell size should not affect the scroll height with scrollToReveal set to none
        assert_eq!(cell_list.scroll_top(), 5.0);

        cell_list.update_element_height2(&view_model.cell_at(0).expect("cell 0"), 50.0, None);
        assert_eq!(cell_list.scroll_top(), 5.0);

        cell_list.update_element_height2(&view_model.cell_at(0).expect("cell 0"), 250.0, None);
        assert_eq!(cell_list.scroll_top(), 5.0);
    });
}

#[test]
fn update_element_height_with_no_scroll_setting_and_cell_editor_focused() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    run(five_cells(), true, |editor, view_model, cell_list| {
        restore_heights(view_model, &[50.0, 100.0, 50.0, 100.0, 50.0]);

        cell_list.attach_view_model(view_model.clone());

        // render height 210, it can render 3 full cells and 1 partial cell
        cell_list.layout(Some(210.0));

        // init scrollTop and scrollBottom
        assert_eq!(cell_list.scroll_top(), 0.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 210.0);

        cell_list.set_focus(&[1], false);

        editor.focus_notebook_cell(&cell_list.view_model().expect("view model").cell_at(1).expect("cell 1"), "editor");
        cell_list.update_element_height2(&view_model.cell_at(0).expect("cell 0"), 100.0, None);
        assert_eq!(cell_list.get_scroll_height(), 400.0);

        // We have the cell editor focused, so we should anchor to that cell
        assert_eq!(cell_list.scroll_top(), 50.0);

        cell_list.update_element_height2(&view_model.cell_at(0).expect("cell 0"), 50.0, None);
        assert_eq!(cell_list.scroll_top(), 0.0);

        cell_list.update_element_height2(&view_model.cell_at(0).expect("cell 0"), 250.0, None);
        assert_eq!(cell_list.scroll_top(), 250.0 + 100.0 - cell_list.get_render_height());
    });
}

#[test]
fn update_element_height_with_focused_element_out_of_viewport() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    run(five_cells(), false, |_editor, view_model, cell_list| {
        restore_heights(view_model, &[50.0, 100.0, 50.0, 100.0, 50.0]);

        cell_list.attach_view_model(view_model.clone());

        // render height 210, it can render 3 full cells and 1 partial cell
        cell_list.layout(Some(210.0));

        // init scrollTop and scrollBottom
        assert_eq!(cell_list.scroll_top(), 0.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 210.0);

        cell_list.set_focus(&[4], false);
        cell_list.update_element_height2(&view_model.cell_at(1).expect("cell 1"), 130.0, None);
        // the focus cell is not in the viewport, the scrolltop should not change at all
        assert_eq!(cell_list.scroll_top(), 0.0);
    });
}

#[test]
fn update_element_height_of_cells_out_of_viewport_should_not_trigger_scroll_121140() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    run(five_cells(), false, |_editor, view_model, cell_list| {
        restore_heights(view_model, &[50.0, 100.0, 50.0, 100.0, 50.0]);

        cell_list.attach_view_model(view_model.clone());

        // render height 210, it can render 3 full cells and 1 partial cell
        cell_list.layout(Some(210.0));

        // init scrollTop and scrollBottom
        assert_eq!(cell_list.scroll_top(), 0.0);
        assert_eq!(cell_list.get_view_scroll_bottom(), 210.0);

        cell_list.set_focus(&[1], false);
        cell_list.set_scroll_top(80.0);
        assert_eq!(cell_list.scroll_top(), 80.0);

        cell_list.update_element_height2(&view_model.cell_at(0).expect("cell 0"), 30.0, None);
        assert_eq!(cell_list.scroll_top(), 60.0);
    });
}

#[test]
fn visible_ranges_should_be_exclusive_of_end() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    run(Vec::new(), false, |_editor, view_model, cell_list| {
        cell_list.attach_view_model(view_model.clone());

        // render height 210, it can render 3 full cells and 1 partial cell
        cell_list.layout(Some(100.0));

        assert_eq!(cell_list.visible_ranges(), Vec::<ICellRange>::new());
    });
}

#[test]
fn visible_ranges_should_be_exclusive_of_end_2() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    run(vec![md("# header a")], false, |_editor, view_model, cell_list| {
        restore_heights(view_model, &[50.0]);

        cell_list.attach_view_model(view_model.clone());

        // render height 210, it can render 3 full cells and 1 partial cell
        cell_list.layout(Some(100.0));

        assert_eq!(cell_list.visible_ranges(), vec![range(0, 1)]);
    });
}
