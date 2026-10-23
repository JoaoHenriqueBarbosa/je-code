//! Porte de `src/vs/editor/contrib/linesOperations/test/browser/copyLinesCommand.test.ts` (suíte `Editor Contrib - Copy Lines Command`), caso a caso e na mesma ordem.
//!
//! Pendente: a suíte `Editor Contrib - Duplicate Selection` (casos `empty selection` e `with selection`)
//! usa `withTestCodeEditor` e `DuplicateSelectionAction` (`linesOperations.ts`), que dependem do editor de
//! código de teste, do view model e do cursor com várias seleções, ainda não portados. Não há aproximação.
//!
//! Notas de tradução:
//! - `testCommand` recebe a seleção já ajustada e a `commandFactory` sem o `accessor` (ver
//!   `tests/support/test_command.rs`); `forceTokenization` fica `false`, como no original.
//! - `new CopyLinesCommand(sel, true)` deixa o `noop` ausente, que vira `None`.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::copy_lines_command::CopyLinesCommand;
use je_editor::selection::Selection;
use support::test_command::test_command;

fn test_copy_lines_down_command(lines: &[&str], selection: Selection, expected_lines: &[&str], expected_selection: Selection) {
    test_command(
        lines,
        None,
        selection,
        |sel| Box::new(CopyLinesCommand::new(sel, true, None)),
        expected_lines,
        expected_selection,
        false,
    );
}

fn test_copy_lines_up_command(lines: &[&str], selection: Selection, expected_lines: &[&str], expected_selection: Selection) {
    test_command(
        lines,
        None,
        selection,
        |sel| Box::new(CopyLinesCommand::new(sel, false, None)),
        expected_lines,
        expected_selection,
        false,
    );
}

#[test]
fn copy_first_line_down() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_copy_lines_down_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(1, 3, 1, 1),
        &["first", "first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(2, 3, 2, 1),
    );
}

#[test]
fn copy_first_line_up() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_copy_lines_up_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(1, 3, 1, 1),
        &["first", "first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(1, 3, 1, 1),
    );
}

#[test]
fn copy_last_line_down() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_copy_lines_down_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(5, 3, 5, 1),
        &["first", "second line", "third line", "fourth line", "fifth", "fifth"],
        Selection::new(6, 3, 6, 1),
    );
}

#[test]
fn copy_last_line_up() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_copy_lines_up_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(5, 3, 5, 1),
        &["first", "second line", "third line", "fourth line", "fifth", "fifth"],
        Selection::new(5, 3, 5, 1),
    );
}

#[test]
fn issue_1322_copy_line_up() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_copy_lines_up_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(3, 11, 3, 11),
        &["first", "second line", "third line", "third line", "fourth line", "fifth"],
        Selection::new(3, 11, 3, 11),
    );
}

#[test]
fn issue_1322_copy_last_line_up() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_copy_lines_up_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(5, 6, 5, 6),
        &["first", "second line", "third line", "fourth line", "fifth", "fifth"],
        Selection::new(5, 6, 5, 6),
    );
}

#[test]
fn copy_many_lines_up() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_copy_lines_up_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(4, 3, 2, 1),
        &["first", "second line", "third line", "fourth line", "second line", "third line", "fourth line", "fifth"],
        Selection::new(4, 3, 2, 1),
    );
}

#[test]
fn ignore_empty_selection() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_copy_lines_up_command(
        &["first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(2, 1, 1, 1),
        &["first", "first", "second line", "third line", "fourth line", "fifth"],
        Selection::new(2, 1, 1, 1),
    );
}
