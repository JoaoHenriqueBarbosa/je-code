//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookStickyScroll.test.ts` (suíte `NotebookEditorStickyScroll`), caso
//! a caso e na mesma ordem. Os 8 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso; o `setup`/`teardown` (o
//!   `DisposableStore` e o `setupInstantiationService`) é o `with_test_notebook`.
//! - `getOutline(editor)` cria um `NotebookCellOutline` (alvo `QuickPick`) e o teste só lê `outline.entries`, que é o `entries` da
//!   fonte de dados do outline. `NotebookCellOutline` (widget, `IEditorPane`) não tem porte, então o caso cria o
//!   [`NotebookCellOutlineDataSource`] sobre o editor de teste, com a mesma fábrica de entradas, o serviço de marcadores e a
//!   configuração. Os símbolos do alvo `QuickPick` (`computeFullSymbols`) são assíncronos e o teste lê `entries` na hora, antes
//!   deles, então não participam do resultado.
//! - `ILanguageFeaturesService`/`LanguageFeaturesService` não é lido por nenhum caso (só o `NotebookCellOutline` o pedia).
//! - `createNotebookCellList(instantiationService, disposables)` + `attachViewModel` + `layout(400, 100)` é
//!   `create_notebook_cell_list` + `attach_view_model` + `layout(Some(400.0))` (a largura não é lida pela camada lógica).
//! - `editor.setScrollTop(n)` é `cell_list.set_scroll_top(n)` do editor de teste (o `cellList.scrollTop` do original) e
//!   `editor.visibleRanges = [...]` é `set_visible_ranges`; `getLayoutInfo().scrollHeight` é `get_scroll_height()` da lista.
//! - `nbStickyTestHelper` + `createStickyTestElement` devolvem o `innerText` (o `header_text`) de cada linha desenhada, a última
//!   primeiro (o `unshift`). O `assertSnapshot` compara com o conteúdo dos arquivos `__snapshots__/NotebookEditorStickyScroll_*.snap`,
//!   copiado literalmente em cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::collections::HashMap;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_platform::markers::marker_service::MarkerService;
use je_platform::markers::markers::IMarkerService;
use je_workbench::contrib::notebook::browser::controller::IActiveNotebookEditor;
use je_workbench::contrib::notebook::browser::view_model::cell_view_model_types::INotebookEditorViewState;
use je_workbench::contrib::notebook::browser::view_model::notebook_outline_data_source::{INotebookCellOutlineDataSource, NotebookCellOutlineDataSource};
use je_workbench::contrib::notebook::browser::view_model::notebook_outline_entry_factory::{INotebookOutlineEntryFactory, NotebookOutlineEntryFactory};
use je_workbench::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use je_workbench::contrib::notebook::browser::view_model::outline_entry::OutlineEntry;
use je_workbench::contrib::notebook::browser::view_parts::notebook_editor_sticky_scroll::{IStickyScrollCellList, IStickyScrollEditor, compute_content};
use je_workbench::contrib::notebook::common::notebook_common::CellKind;
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use support::notebook_outline_support::{UnusedOutlineModelService, UnusedTextModelService};
use support::test_notebook_editor::{ActiveTestNotebookEditorDelegate, MockNotebookCell, create_notebook_cell_list, empty_metadata, with_test_notebook};

fn md(source: &'static str) -> MockNotebookCell {
    (source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))
}

fn js(source: &'static str) -> MockNotebookCell {
    (source, "javascript", CellKind::Code, Vec::new(), Some(empty_metadata()))
}

impl IStickyScrollEditor for ActiveTestNotebookEditorDelegate {
    fn scroll_top(&self) -> f64 {
        self.cell_list.scroll_top()
    }

    fn visible_ranges(&self) -> Vec<ICellRange> {
        ActiveTestNotebookEditorDelegate::visible_ranges(self)
    }

    fn cell_at(&self, index: i64) -> Option<CellViewModel> {
        IActiveNotebookEditor::cell_at(self, index)
    }

    fn layout_scroll_height(&self) -> f64 {
        self.cell_list.get_scroll_height()
    }
}

/// O corpo comum dos casos: o notebook de teste com as células e a altura 50 em cada uma, a lista de células com `layout`, a posição
/// de rolagem e a faixa visível; devolve o que `nbStickyTestHelper` devolve.
fn render_sticky(cells: Vec<MockNotebookCell>, scroll_top: f64, visible_range: ICellRange) -> Vec<String> {
    let count = cells.len();
    with_test_notebook(cells, |editor, view_model, ds, services| {
        view_model.restore_editor_view_state(Some(&INotebookEditorViewState {
            editing_cells: HashMap::new(),
            editor_view_states: HashMap::new(),
            cell_total_heights: Some((0..count).map(|index| (index, 50.0)).collect()),
            cell_line_number_states: HashMap::new(),
            collapsed_input_cells: HashMap::new(),
            collapsed_output_cells: HashMap::new(),
        }));

        let cell_list = create_notebook_cell_list(services, ds, None);
        cell_list.attach_view_model(view_model.clone());
        cell_list.layout(Some(400.0));

        editor.cell_list.set_scroll_top(scroll_top);
        editor.set_visible_ranges(vec![visible_range]);

        // getOutline(editor)
        let marker_service = MarkerService::new();
        let marker_service_ref: Rc<dyn IMarkerService> = marker_service.clone();
        let entry_factory: Rc<dyn INotebookOutlineEntryFactory> =
            Rc::new(NotebookOutlineEntryFactory::new(services.notebook_execution_state_service(), Rc::new(UnusedOutlineModelService), Rc::new(UnusedTextModelService)));
        let active_editor: Rc<dyn IActiveNotebookEditor> = editor.clone();
        let outline = NotebookCellOutlineDataSource::new(active_editor, marker_service_ref, services.configuration_service.clone(), entry_factory);
        let notebook_outline_entries: Vec<Rc<OutlineEntry>> = outline.entries();

        // nbStickyTestHelper + createStickyTestElement
        let output = compute_content(&**editor, &*cell_list, &notebook_outline_entries, 0.0);
        let mut output_elements: Vec<String> = Vec::new();
        for sticky_line in output.values() {
            if sticky_line.rendered {
                output_elements.insert(0, sticky_line.line.header_text.clone());
            }
        }

        outline.dispose();
        marker_service.dispose();
        output_elements
    })
}

fn range(start: i64, end: i64) -> ICellRange {
    ICellRange { start, end }
}

fn assert_snapshot(actual: Vec<String>, expected: &[&str]) {
    assert_eq!(actual, expected);
}

#[test]
fn test0_should_render_empty_scroll_top_at_0() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_sticky(
        vec![
            md("# header a"),
            md("## header aa"),
            js("var b = 1;"),
            js("var b = 1;"),
            js("var b = 1;"),
            js("var b = 1;"),
            md("# header b"),
            js("var c = 2;"),
        ],
        0.0,
        range(0, 8),
    );
    assert_snapshot(result, &[]);
}

#[test]
fn test1_should_render_0_1_visible_range_3_8() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_sticky(
        vec![
            md("# header a"),    // 0
            md("## header aa"),  // 50
            js("var b = 1;"),    // 100
            js("var b = 1;"),    // 150
            js("var b = 1;"),    // 200
            js("var b = 1;"),    // 250
            md("# header b"),    // 300
            js("var c = 2;"),    // 350
        ],
        175.0,
        range(3, 8),
    );
    assert_snapshot(result, &["header a", "header aa"]);
}

#[test]
fn test2_should_render_0_visible_range_6_9_so_collapsing_next_2_against_following_section() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_sticky(
        vec![
            md("# header a"),      // 0
            md("## header aa"),    // 50
            md("### header aaa"),  // 100
            js("var b = 1;"),      // 150
            js("var b = 1;"),      // 200
            js("var b = 1;"),      // 250
            js("var b = 1;"),      // 300
            md("# header b"),      // 350
            js("var c = 2;"),      // 400
        ],
        325.0, // room for a single header
        range(6, 9),
    );
    assert_snapshot(result, &["header a"]);
}

#[test]
fn test3_should_render_0_2_collapsing_against_equivalent_level_header() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_sticky(
        vec![
            md("# header a"),      // 0
            md("## header aa"),    // 50
            md("### header aaa"),  // 100
            js("var b = 1;"),      // 150
            md("### header aab"),  // 200
            js("var b = 1;"),      // 250
            js("var b = 1;"),      // 300
            js("var b = 1;"),      // 350
            md("# header b"),      // 400
            js("var c = 2;"),      // 450
        ],
        175.0, // room for a single header
        range(3, 10),
    );
    assert_snapshot(result, &["header a", "header aa", "header aab"]);
}

// outdated/improper behavior
#[test]
fn test4_should_render_0_scrolltop_halfway_through_cell_0() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_sticky(
        vec![
            md("# header a"),
            md("## header aa"),
            js("var b = 1;"),
            js("var b = 1;"),
            js("var b = 1;"),
            js("var b = 1;"),
            md("# header b"),
            js("var c = 2;"),
        ],
        50.0,
        range(0, 8),
    );
    assert_snapshot(result, &["header a"]);
}

#[test]
fn test5_should_render_0_2_scrolltop_halfway_through_cell_2() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_sticky(
        vec![
            md("# header a"),
            md("## header aa"),
            md("### header aaa"),
            md("#### header aaaa"),
            js("var b = 1;"),
            js("var b = 1;"),
            js("var b = 1;"),
            js("var b = 1;"),
            md("# header b"),
            js("var c = 2;"),
        ],
        125.0,
        range(2, 10),
    );
    assert_snapshot(result, &["header a", "header aa", "header aaa"]);
}

#[test]
fn test6_should_render_6_7_scrolltop_halfway_through_cell_7() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_sticky(
        vec![
            md("# header a"),
            md("## header aa"),
            js("var b = 1;"),
            js("var b = 1;"),
            js("var b = 1;"),
            js("var b = 1;"),
            md("# header b"),
            md("## header bb"),
            md("### header bbb"),
            js("var c = 2;"),
        ],
        375.0,
        range(7, 10),
    );
    assert_snapshot(result, &["header b", "header bb"]);
}

#[test]
fn test7_should_render_0_1_collapsing_against_next_section() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let result = render_sticky(
        vec![
            md("# header a"),       // 0
            md("## header aa"),     // 50
            md("### header aaa"),   // 100
            md("#### header aaaa"), // 150
            js("var b = 1;"),       // 200
            js("var b = 1;"),       // 250
            js("var b = 1;"),       // 300
            js("var b = 1;"),       // 350
            md("# header b"),       // 400
            md("## header bb"),     // 450
            md("### header bbb"),
            js("var c = 2;"),
        ],
        350.0,
        range(7, 12),
    );
    assert_snapshot(result, &["header a", "header aa"]);
}
