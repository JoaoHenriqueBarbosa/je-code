//! Porte de `src/vs/editor/contrib/caretOperations/test/browser/moveCarretCommand.test.ts` (suíte `Editor Contrib - Move Caret Command`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `testCommand` recebe a seleção já ajustada e a `commandFactory` sem o `accessor` (ver
//!   `tests/support/test_command.rs`); `forceTokenization` fica `false`, como no original.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::move_caret_command::MoveCaretCommand;
use je_editor::selection::Selection;
use support::test_command::test_command;

fn test_move_caret_left_command(lines: &[&str], selection: Selection, expected_lines: &[&str], expected_selection: Selection) {
    test_command(
        lines,
        None,
        selection,
        |sel| Box::new(MoveCaretCommand::new(sel, true)),
        expected_lines,
        expected_selection,
        false,
    );
}

fn test_move_caret_right_command(lines: &[&str], selection: Selection, expected_lines: &[&str], expected_selection: Selection) {
    test_command(
        lines,
        None,
        selection,
        |sel| Box::new(MoveCaretCommand::new(sel, false)),
        expected_lines,
        expected_selection,
        false,
    );
}

#[test]
fn move_selection_to_left() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_move_caret_left_command(&["012345"], Selection::new(1, 3, 1, 5), &["023145"], Selection::new(1, 2, 1, 4));
}

#[test]
fn move_selection_to_right() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_move_caret_right_command(&["012345"], Selection::new(1, 3, 1, 5), &["014235"], Selection::new(1, 4, 1, 6));
}

#[test]
fn move_selection_to_left_from_first_column_no_change() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_move_caret_left_command(&["012345"], Selection::new(1, 1, 1, 1), &["012345"], Selection::new(1, 1, 1, 1));
}

#[test]
fn move_selection_to_right_from_last_column_no_change() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_move_caret_right_command(&["012345"], Selection::new(1, 5, 1, 7), &["012345"], Selection::new(1, 5, 1, 7));
}
