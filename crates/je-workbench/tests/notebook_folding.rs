//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookFolding.test.ts` (suíte `Notebook Folding`), caso a caso e na
//! mesma ordem. Os 7 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso (`_store`); o `setup` da suíte (o
//!   `DisposableStore`, o `setupInstantiationService` e o `spy(IUndoRedoService, 'pushElement')`, que nenhum caso lê) é o que o
//!   `with_test_notebook` já entrega.
//! - Os casos com vários `await withTestNotebook(...)` (`Folding`, `Folding Memento`, `View Index`) rodam um `with_test_notebook`
//!   após o outro dentro do mesmo `#[test]`.
//! - `ds.add(new FoldingModel())` é `ds.add_rc(FoldingModel::new())`; `regions.findRange(line)` e `getEndLineNumber(index)` são os
//!   métodos de mesmo nome do `FoldingRegions`, e os `{ start, end }` dos `deepStrictEqual` são `ICellRange`.
//! - Cada lista de células é montada por [`markdown_cells`] (`['texto', 'markdown', CellKind.Markup, [], {}]`) e, onde há código,
//!   por [`python_cell`].
//! - O trecho comentado do original em `Nested Folding` (`viewModel.insertCell`) continua comentado lá e não é portado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::notebook::browser::view_model::folding_model::{FoldingModel, update_folding_state_at_index};
use je_workbench::contrib::notebook::common::notebook_common::CellKind;
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;
use support::test_notebook_editor::{MockNotebookCell, empty_metadata, with_test_notebook};

fn range(start: i64, end: i64) -> ICellRange {
    ICellRange { start, end }
}

fn markdown_cells(sources: &[&'static str]) -> Vec<MockNotebookCell> {
    sources.iter().map(|source| (*source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))).collect()
}

fn python_cell(source: &'static str) -> MockNotebookCell {
    (source, "python", CellKind::Code, Vec::new(), Some(empty_metadata()))
}

fn markdown_cell(source: &'static str) -> MockNotebookCell {
    (source, "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))
}

/// As 12 células dos casos `Folding Memento` e `View Index`.
fn twelve_cells() -> Vec<MockNotebookCell> {
    markdown_cells(&[
        "# header 1",
        "body",
        "# header 2.1\n",
        "body 2",
        "body 3",
        "## header 2.2",
        "var e = 7;",
        "# header 2.1\n",
        "body 2",
        "body 3",
        "## header 2.2",
        "var e = 7;",
    ])
}

#[test]
fn folding_based_on_markdown_cells() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        markdown_cells(&["# header 1", "body", "## header 2.1", "body 2", "body 3", "## header 2.2", "var e = 7;"]),
        |_editor, view_model, ds, _services| {
            let folding_controller = ds.add_rc(FoldingModel::new());
            folding_controller.attach_view_model(view_model.clone());

            assert_eq!(folding_controller.regions().find_range(1), 0);
            assert_eq!(folding_controller.regions().find_range(2), 0);
            assert_eq!(folding_controller.regions().find_range(3), 1);
            assert_eq!(folding_controller.regions().find_range(4), 1);
            assert_eq!(folding_controller.regions().find_range(5), 1);
            assert_eq!(folding_controller.regions().find_range(6), 2);
            assert_eq!(folding_controller.regions().find_range(7), 2);
        },
    );
}

#[test]
fn folding_not_based_on_code_cells() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        vec![
            markdown_cell("# header 1"),
            markdown_cell("body"),
            python_cell("# comment 1"),
            markdown_cell("body 2"),
            markdown_cell("body 3\n```\n## comment 2\n```"),
            markdown_cell("body 4"),
            markdown_cell("## header 2.1"),
            python_cell("var e = 7;"),
        ],
        |_editor, view_model, ds, _services| {
            let folding_controller = ds.add_rc(FoldingModel::new());
            folding_controller.attach_view_model(view_model.clone());

            assert_eq!(folding_controller.regions().find_range(1), 0);
            assert_eq!(folding_controller.regions().find_range(2), 0);
            assert_eq!(folding_controller.regions().find_range(3), 0);
            assert_eq!(folding_controller.regions().find_range(4), 0);
            assert_eq!(folding_controller.regions().find_range(5), 0);
            assert_eq!(folding_controller.regions().find_range(6), 0);
            assert_eq!(folding_controller.regions().find_range(7), 1);
            assert_eq!(folding_controller.regions().find_range(8), 1);
        },
    );
}

#[test]
fn top_level_header_in_a_cell_wins() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        markdown_cells(&["# header 1", "body", "## header 2.1\n# header3", "body 2", "body 3", "## header 2.2", "var e = 7;"]),
        |_editor, view_model, ds, _services| {
            let folding_controller = ds.add_rc(FoldingModel::new());
            folding_controller.attach_view_model(view_model.clone());

            assert_eq!(folding_controller.regions().find_range(1), 0);
            assert_eq!(folding_controller.regions().find_range(2), 0);
            assert_eq!(folding_controller.regions().get_end_line_number(0), 2);

            assert_eq!(folding_controller.regions().find_range(3), 1);
            assert_eq!(folding_controller.regions().find_range(4), 1);
            assert_eq!(folding_controller.regions().find_range(5), 1);
            assert_eq!(folding_controller.regions().get_end_line_number(1), 7);

            assert_eq!(folding_controller.regions().find_range(6), 2);
            assert_eq!(folding_controller.regions().find_range(7), 2);
            assert_eq!(folding_controller.regions().get_end_line_number(2), 7);
        },
    );
}

#[test]
fn folding() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        markdown_cells(&["# header 1", "body", "## header 2.1", "body 2", "body 3", "## header 2.2", "var e = 7;"]),
        |_editor, view_model, ds, _services| {
            let folding_model = ds.add_rc(FoldingModel::new());
            folding_model.attach_view_model(view_model.clone());
            update_folding_state_at_index(&folding_model, 0, true);
            view_model.update_folding_ranges(folding_model.regions());
            assert_eq!(view_model.get_hidden_ranges(), vec![range(1, 6)]);
        },
    );

    with_test_notebook(
        markdown_cells(&["# header 1", "body", "## header 2.1\n", "body 2", "body 3", "## header 2.2", "var e = 7;"]),
        |_editor, view_model, ds, _services| {
            let folding_model = ds.add_rc(FoldingModel::new());
            folding_model.attach_view_model(view_model.clone());
            update_folding_state_at_index(&folding_model, 2, true);
            view_model.update_folding_ranges(folding_model.regions());

            assert_eq!(view_model.get_hidden_ranges(), vec![range(3, 4)]);
        },
    );

    with_test_notebook(
        markdown_cells(&["# header 1", "body", "# header 2.1\n", "body 2", "body 3", "## header 2.2", "var e = 7;"]),
        |_editor, view_model, ds, _services| {
            let folding_model = ds.add_rc(FoldingModel::new());
            folding_model.attach_view_model(view_model.clone());
            update_folding_state_at_index(&folding_model, 2, true);
            view_model.update_folding_ranges(folding_model.regions());

            assert_eq!(view_model.get_hidden_ranges(), vec![range(3, 6)]);
        },
    );
}

#[test]
fn nested_folding() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(
        markdown_cells(&["# header 1", "body", "# header 2.1\n", "body 2", "body 3", "## header 2.2", "var e = 7;"]),
        |_editor, view_model, ds, _services| {
            let folding_model = ds.add_rc(FoldingModel::new());
            folding_model.attach_view_model(view_model.clone());
            update_folding_state_at_index(&folding_model, 0, true);
            view_model.update_folding_ranges(folding_model.regions());

            assert_eq!(view_model.get_hidden_ranges(), vec![range(1, 1)]);

            update_folding_state_at_index(&folding_model, 5, true);
            update_folding_state_at_index(&folding_model, 2, true);
            view_model.update_folding_ranges(folding_model.regions());

            assert_eq!(view_model.get_hidden_ranges(), vec![range(1, 1), range(3, 6)]);

            update_folding_state_at_index(&folding_model, 2, false);
            view_model.update_folding_ranges(folding_model.regions());
            assert_eq!(view_model.get_hidden_ranges(), vec![range(1, 1), range(6, 6)]);
        },
    );
}

#[test]
fn folding_memento() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(twelve_cells(), |_editor, view_model, ds, _services| {
        let folding_model = ds.add_rc(FoldingModel::new());
        folding_model.attach_view_model(view_model.clone());
        folding_model.apply_memento(&[range(2, 6)]);
        view_model.update_folding_ranges(folding_model.regions());

        // Note that hidden ranges !== folding ranges
        assert_eq!(view_model.get_hidden_ranges(), vec![range(3, 6)]);
    });

    with_test_notebook(twelve_cells(), |_editor, view_model, ds, _services| {
        let folding_model = ds.add_rc(FoldingModel::new());
        folding_model.attach_view_model(view_model.clone());
        folding_model.apply_memento(&[range(5, 6), range(10, 11)]);
        view_model.update_folding_ranges(folding_model.regions());

        // Note that hidden ranges !== folding ranges
        assert_eq!(view_model.get_hidden_ranges(), vec![range(6, 6), range(11, 11)]);
    });

    with_test_notebook(twelve_cells(), |_editor, view_model, ds, _services| {
        let folding_model = ds.add_rc(FoldingModel::new());
        folding_model.attach_view_model(view_model.clone());
        folding_model.apply_memento(&[range(5, 6), range(7, 11)]);
        view_model.update_folding_ranges(folding_model.regions());

        // Note that hidden ranges !== folding ranges
        assert_eq!(view_model.get_hidden_ranges(), vec![range(6, 6), range(8, 11)]);
    });
}

#[test]
fn view_index() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(twelve_cells(), |_editor, view_model, ds, _services| {
        let folding_model = ds.add_rc(FoldingModel::new());
        folding_model.attach_view_model(view_model.clone());
        folding_model.apply_memento(&[range(2, 6)]);
        view_model.update_folding_ranges(folding_model.regions());

        // Note that hidden ranges !== folding ranges
        assert_eq!(view_model.get_hidden_ranges(), vec![range(3, 6)]);

        assert_eq!(view_model.get_next_visible_cell_index(1), 2);
        assert_eq!(view_model.get_next_visible_cell_index(2), 7);
        assert_eq!(view_model.get_next_visible_cell_index(3), 7);
        assert_eq!(view_model.get_next_visible_cell_index(4), 7);
        assert_eq!(view_model.get_next_visible_cell_index(5), 7);
        assert_eq!(view_model.get_next_visible_cell_index(6), 7);
        assert_eq!(view_model.get_next_visible_cell_index(7), 8);
    });

    with_test_notebook(twelve_cells(), |_editor, view_model, ds, _services| {
        let folding_model = ds.add_rc(FoldingModel::new());
        folding_model.attach_view_model(view_model.clone());
        folding_model.apply_memento(&[range(5, 6), range(10, 11)]);

        view_model.update_folding_ranges(folding_model.regions());

        // Note that hidden ranges !== folding ranges
        assert_eq!(view_model.get_hidden_ranges(), vec![range(6, 6), range(11, 11)]);

        // folding ranges
        // [5, 6]
        // [10, 11]
        assert_eq!(view_model.get_next_visible_cell_index(4), 5);
        assert_eq!(view_model.get_next_visible_cell_index(5), 7);
        assert_eq!(view_model.get_next_visible_cell_index(6), 7);

        assert_eq!(view_model.get_next_visible_cell_index(9), 10);
        assert_eq!(view_model.get_next_visible_cell_index(10), 12);
        assert_eq!(view_model.get_next_visible_cell_index(11), 12);
    });
}
