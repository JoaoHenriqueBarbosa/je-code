//! Porte de `src/vs/workbench/contrib/notebook/test/browser/contrib/layoutActions.test.ts` (suíte `Notebook Layout Actions`), caso a caso e na mesma ordem. O caso está portado.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` do caso.
//! - `'right'` (texto) é o [`ToolbarPosition::Legacy`], e `{ default: 'right' }` é o [`ToolbarPosition::PerViewType`]; o objeto devolvido é
//!   a lista de pares na ordem de inserção.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_workbench::contrib::notebook::browser::contrib::layout::layout_actions::{ToggleCellToolbarPositionAction, ToolbarPosition};

fn legacy(value: &str) -> ToolbarPosition {
    ToolbarPosition::Legacy(value.to_string())
}

fn per_view_type(default: &str) -> ToolbarPosition {
    ToolbarPosition::PerViewType(vec![("default".to_string(), default.to_string())])
}

fn pairs(default: &str, view: &str) -> Vec<(String, String)> {
    vec![("default".to_string(), default.to_string()), ("test-nb".to_string(), view.to_string())]
}

#[test]
fn toggle_cell_toolbar_position() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let action = ToggleCellToolbarPositionAction;

    // "notebook.cellToolbarLocation": "right"
    assert_eq!(action.toggle_position("test-nb", &legacy("right")), pairs("right", "left"));

    // "notebook.cellToolbarLocation": "left"
    assert_eq!(action.toggle_position("test-nb", &legacy("left")), pairs("left", "right"));

    // "notebook.cellToolbarLocation": "hidden"
    assert_eq!(action.toggle_position("test-nb", &legacy("hidden")), pairs("hidden", "right"));

    // invalid
    assert_eq!(action.toggle_position("test-nb", &legacy("")), pairs("right", "left"));

    // no user config, default value
    assert_eq!(action.toggle_position("test-nb", &per_view_type("right")), pairs("right", "left"));

    // user config, default to left
    assert_eq!(action.toggle_position("test-nb", &per_view_type("left")), pairs("left", "right"));

    // user config, default to hidden
    assert_eq!(action.toggle_position("test-nb", &per_view_type("hidden")), pairs("hidden", "right"));
}
