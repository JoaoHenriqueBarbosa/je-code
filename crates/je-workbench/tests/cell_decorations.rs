//! Porte de `src/vs/workbench/contrib/notebook/test/browser/cellDecorations.test.ts` (suíte `CellDecorations`), caso a caso e na mesma ordem. Os 2 casos estão portados.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` de cada caso, e `await withTestNotebook(cells, async (editor, viewModel) => ...)`
//!   é o `with_test_notebook` com o corpo síncrono.
//! - `Event.once(cell.onCellDecorationsChanged)(listener)` é o `once()` do evento, e os `let added = false` capturados pelo ouvinte são
//!   `Cell<bool>` compartilhados.
//! - `cell.deltaCellDecorations(...)`, `cell.getCellDecorations()` e `cell.onCellDecorationsChanged` são os da base da célula (`base()`).
//! - `{ className: 'style1' }` é o [`INotebookCellDecorationOptions`] com os campos não citados ausentes.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::notebook::browser::view_model::base_cell_view_model::INotebookCellDecorationOptions;
use je_workbench::contrib::notebook::common::notebook_common::CellKind;
use support::test_notebook_editor::{empty_metadata, with_test_notebook};

fn decoration(class_name: Option<&str>, output_class_name: Option<&str>) -> INotebookCellDecorationOptions {
    INotebookCellDecorationOptions { class_name: class_name.map(str::to_string), output_class_name: output_class_name.map(str::to_string), ..Default::default() }
}

#[test]
fn add_and_remove_a_cell_decoration() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![("# header a", "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))], |_editor, view_model, _ds, _services| {
        let cell = view_model.cell_at(0).expect("cell 0");

        let added = Rc::new(Cell::new(false));
        let added_flag = added.clone();
        cell.base().on_cell_decorations_changed().once().listen(move |e| added_flag.set(e.added.iter().any(|decoration| decoration.class_name.as_deref() == Some("style1"))));

        let decoration_ids = cell.base().delta_cell_decorations(&[], vec![decoration(Some("style1"), None)]);
        assert!(cell.base().get_cell_decorations().iter().any(|dec| dec.class_name.as_deref() == Some("style1")));

        let removed = Rc::new(Cell::new(false));
        let removed_flag = removed.clone();
        cell.base().on_cell_decorations_changed().once().listen(move |e| removed_flag.set(e.removed.iter().any(|decoration| decoration.class_name.as_deref() == Some("style1"))));
        cell.base().delta_cell_decorations(&decoration_ids, Vec::new());

        assert!(!cell.base().get_cell_decorations().iter().any(|dec| dec.class_name.as_deref() == Some("style1")));
        assert!(added.get());
        assert!(removed.get());
    });
}

#[test]
fn removing_one_cell_decoration_should_not_remove_all() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_notebook(vec![("# header a", "markdown", CellKind::Markup, Vec::new(), Some(empty_metadata()))], |_editor, view_model, _ds, _services| {
        let cell = view_model.cell_at(0).expect("cell 0");

        let decoration_ids = cell.base().delta_cell_decorations(&[], vec![decoration(Some("style1"), Some("style1"))]);
        cell.base().delta_cell_decorations(&[], vec![decoration(Some("style1"), None)]);

        let style_removed = Rc::new(Cell::new(false));
        let output_style_removed = Rc::new(Cell::new(false));
        let (style_flag, output_flag) = (style_removed.clone(), output_style_removed.clone());
        cell.base().on_cell_decorations_changed().once().listen(move |e| {
            style_flag.set(e.removed.iter().any(|decoration| decoration.class_name.as_deref() == Some("style1")));
            output_flag.set(e.removed.iter().any(|decoration| decoration.output_class_name.as_deref() == Some("style1")));
        });
        // remove the first style added, which should only remove the output class
        cell.base().delta_cell_decorations(&decoration_ids, Vec::new());

        assert!(!cell.base().get_cell_decorations().iter().any(|dec| dec.output_class_name.as_deref() == Some("style1")));
        assert!(cell.base().get_cell_decorations().iter().any(|dec| dec.class_name.as_deref() == Some("style1")));
        assert!(!style_removed.get());
        assert!(output_style_removed.get());
    });
}
