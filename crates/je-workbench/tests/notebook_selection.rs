//! Porte de `src/vs/workbench/contrib/notebook/test/browser/notebookSelection.test.ts` (suíte `NotebookSelection`), caso a caso e na mesma ordem.
//!
//! Casos de fora (11 dos 15), todos da suíte `NotebookCellList focus/selection`, que dependem do widget de lista de células
//! (`createNotebookCellList` e `NotebookCellList.setFocus`/`setSelections`), do `NotebookViewModel` com o `FoldingModel`,
//! da seleção por UI e de `runDeleteAction` (`cellOperations.ts`), tudo Fase 6: `notebook cell list setFocus`, `notebook
//! cell list setSelections`, `notebook cell list setFocus2`, `notebook cell list focus/selection from UI`, `notebook cell
//! list focus/selection with folding regions`, `notebook cell list focus/selection with folding regions and applyEdits`,
//! `notebook cell list getModelIndex`, `notebook validate range`, `notebook updateSelectionState`, `notebook cell
//! selection w/ cell deletion` e `notebook cell selection w/ cell deletion from applyEdits`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso (`store`).
//! - `setState(null, ...)` é `set_state(None, ...)`, e `assert.deepStrictEqual` dos `{ start, end }` compara os `ICellRange`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_workbench::contrib::notebook::browser::view_model::cell_selection_collection::NotebookCellSelectionCollection;
use je_workbench::contrib::notebook::common::notebook_range::ICellRange;

fn range(start: i64, end: i64) -> ICellRange {
    ICellRange { start, end }
}

mod notebook_selection {
    use super::*;

    #[test]
    fn focus_is_never_empty() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let selection_collection = NotebookCellSelectionCollection::new();
        assert_eq!(selection_collection.focus(), range(0, 0));

        selection_collection.set_state(None, &[], true, "model");
        assert_eq!(selection_collection.focus(), range(0, 0));
        selection_collection.dispose();
    }

    #[test]
    fn selection_is_never_empty() {
        let _store = ensure_no_disposables_are_leaked_in_test_suite();
        let selection_collection = NotebookCellSelectionCollection::new();
        assert_eq!(selection_collection.selections(), [range(0, 0)]);

        selection_collection.set_state(None, &[], true, "model");
        assert_eq!(selection_collection.selections(), [range(0, 0)]);
        selection_collection.dispose();
    }

    #[test]
    fn selections_does_not_change_when_setting_to_empty() {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let selection_collection = NotebookCellSelectionCollection::new();
        let changed = Rc::new(Cell::new(false));
        store.add({
            let changed = changed.clone();
            selection_collection.on_did_change_selection().listen(move |_| changed.set(true))
        });

        selection_collection.set_state(None, &[], false, "model");
        assert!(!changed.get());
        selection_collection.set_state(Some(range(0, 0)), &[], false, "model");
        assert!(!changed.get());
        selection_collection.set_state(Some(range(0, 0)), &[range(0, 0)], false, "model");
        assert!(!changed.get());
        selection_collection.set_state(None, &[], false, "model");
        assert!(!changed.get());
        selection_collection.dispose();
    }

    #[test]
    fn event_fires_when_selection_or_focus_changes() {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let selection_collection = NotebookCellSelectionCollection::new();
        let event_count = Rc::new(Cell::new(0));
        store.add({
            let event_count = event_count.clone();
            selection_collection.on_did_change_selection().listen(move |_| event_count.set(event_count.get() + 1))
        });

        // Change focus
        selection_collection.set_state(Some(range(1, 1)), &[range(1, 2)], false, "model");
        assert_eq!(event_count.get(), 1);

        // Change selections
        selection_collection.set_state(Some(range(1, 1)), &[range(1, 2), range(2, 3)], false, "model");
        assert_eq!(event_count.get(), 2);

        // no change
        selection_collection.set_state(Some(range(1, 1)), &[range(1, 2), range(2, 3)], false, "model");
        assert_eq!(event_count.get(), 2);

        // change to empty focus
        selection_collection.set_state(Some(range(0, 0)), &[range(4, 5)], false, "model");
        assert_eq!(event_count.get(), 3);

        // change to empty selections
        selection_collection.set_state(Some(range(0, 0)), &[], false, "model");
        assert_eq!(event_count.get(), 4);

        selection_collection.dispose();
    }
}
