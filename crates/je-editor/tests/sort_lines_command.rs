//! Porte de `src/vs/editor/contrib/linesOperations/test/browser/sortLinesCommand.test.ts` (suíte `Editor Contrib - Sort Lines Command`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `testCommand` recebe a seleção já ajustada e a `commandFactory` sem o `accessor` (ver
//!   `tests/support/test_command.rs`); `forceTokenization` fica `false`, como no original.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::selection::Selection;
use je_editor::sort_lines_command::SortLinesCommand;
use support::test_command::test_command;

fn test_sort_lines_ascending_command(lines: &[&str], selection: Selection, expected_lines: &[&str], expected_selection: Selection) {
    test_command(
        lines,
        None,
        selection,
        |sel| Box::new(SortLinesCommand::new(sel, false)),
        expected_lines,
        expected_selection,
        false,
    );
}

fn test_sort_lines_descending_command(lines: &[&str], selection: Selection, expected_lines: &[&str], expected_selection: Selection) {
    test_command(
        lines,
        None,
        selection,
        |sel| Box::new(SortLinesCommand::new(sel, true)),
        expected_lines,
        expected_selection,
        false,
    );
}

#[test]
fn no_op_unless_at_least_two_lines_selected_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_sort_lines_ascending_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(1, 3, 1, 1),
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(1, 3, 1, 1),
    );
}

#[test]
fn no_op_unless_at_least_two_lines_selected_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_sort_lines_ascending_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(1, 3, 2, 1),
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(1, 3, 2, 1),
    );
}

#[test]
fn sorting_two_lines_ascending() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_sort_lines_ascending_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(3, 3, 4, 2),
        &["first", "second line", "fourth line", "third line", "fifth"],
        Selection::new(3, 3, 4, 1),
    );
}

#[test]
fn sorting_first_4_lines_ascending() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_sort_lines_ascending_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(1, 1, 5, 1),
        &["first", "fourth line", "second line", "third line", "fifth"],
        Selection::new(1, 1, 5, 1),
    );
}

#[test]
fn sorting_all_lines_ascending() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_sort_lines_ascending_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(1, 1, 5, 6),
        &["fifth", "first", "fourth line", "second line", "third line"],
        Selection::new(1, 1, 5, 11),
    );
}

#[test]
fn sorting_first_4_lines_descending() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_sort_lines_descending_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(1, 1, 5, 1),
        &["third line", "second line", "fourth line", "first", "fifth"],
        Selection::new(1, 1, 5, 1),
    );
}

#[test]
fn sorting_all_lines_descending() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_sort_lines_descending_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(1, 1, 5, 6),
        &["third line", "second line", "fourth line", "first", "fifth"],
        Selection::new(1, 1, 5, 6),
    );
}
