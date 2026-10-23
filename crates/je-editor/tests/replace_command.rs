//! Testes do porte de `src/vs/editor/common/commands/replaceCommand.ts`. O original não tem suíte própria
//! (os comandos são exercitados pelos testes do cursor), então cada caso aplica um comando num modelo com o
//! `test_command` e confere o texto e a seleção que o código do original produz.
//!
//! Notas de tradução:
//! - `testCommand` recebe a seleção já ajustada e a `commandFactory` sem o `accessor` (ver
//!   `tests/support/test_command.rs`); `forceTokenization` fica `false`.
//! - A seleção inicial é irrelevante para estes comandos, que recebem o intervalo no construtor.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::range::Range;
use je_editor::replace_command::{
    ReplaceCommand, ReplaceCommandThatPreservesSelection, ReplaceCommandThatSelectsText,
    ReplaceCommandWithOffsetCursorState, ReplaceCommandWithoutChangingPosition, ReplaceOvertypeCommand,
    ReplaceOvertypeCommandOnCompositionEnd,
};
use je_editor::selection::Selection;
use support::test_command::test_command;

const LINES: [&str; 3] = ["first", "second line", "third"];

fn text(value: &str) -> JsString {
    JsString::from(value)
}

#[test]
fn replace_command_puts_the_cursor_at_the_end_of_the_inserted_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_command(
        &LINES,
        None,
        Selection::new(1, 1, 1, 1),
        |_| Box::new(ReplaceCommand::new(Range::new(1, 2, 1, 4), text("XY"), None)),
        &["fXYst", "second line", "third"],
        Selection::new(1, 4, 1, 4),
        false,
    );
}

#[test]
fn replace_command_that_selects_text_selects_the_inserted_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_command(
        &LINES,
        None,
        Selection::new(1, 1, 1, 1),
        |_| Box::new(ReplaceCommandThatSelectsText::new(Range::new(1, 2, 1, 4), text("XY"))),
        &["fXYst", "second line", "third"],
        Selection::new(1, 2, 1, 4),
        false,
    );
}

#[test]
fn replace_command_without_changing_position_puts_the_cursor_at_the_start() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_command(
        &LINES,
        None,
        Selection::new(1, 1, 1, 1),
        |_| Box::new(ReplaceCommandWithoutChangingPosition::new(Range::new(1, 2, 1, 4), text("XY"), None)),
        &["fXYst", "second line", "third"],
        Selection::new(1, 2, 1, 2),
        false,
    );
}

#[test]
fn replace_command_with_offset_cursor_state_moves_the_end_by_the_deltas() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // O fim do texto inserido é (1,4); com 1 linha e 3 colunas de deslocamento vira (2,7).
    test_command(
        &LINES,
        None,
        Selection::new(1, 1, 1, 1),
        |_| Box::new(ReplaceCommandWithOffsetCursorState::new(Range::new(1, 2, 1, 4), text("XY"), 1, 3, None)),
        &["fXYst", "second line", "third"],
        Selection::new(2, 7, 2, 7),
        false,
    );
}

#[test]
fn replace_overtype_command_with_empty_range_overwrites_as_many_characters_as_the_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    test_command(
        &LINES,
        None,
        Selection::new(1, 1, 1, 1),
        |_| Box::new(ReplaceOvertypeCommand::new(Range::new(1, 2, 1, 2), text("XY"), None)),
        &["fXYst", "second line", "third"],
        Selection::new(1, 4, 1, 4),
        false,
    );
}

#[test]
fn replace_overtype_command_with_range_overwrites_one_character_less_than_the_text() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // `offsetDelta = 1 + (-1) = 0`: o intervalo fica como veio.
    test_command(
        &LINES,
        None,
        Selection::new(1, 1, 1, 1),
        |_| Box::new(ReplaceOvertypeCommand::new(Range::new(1, 2, 1, 3), text("X"), None)),
        &["fXrst", "second line", "third"],
        Selection::new(1, 3, 1, 3),
        false,
    );
}

#[test]
fn replace_overtype_command_does_not_overwrite_past_the_end_of_the_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Faltam só 2 colunas na linha: o fim é limitado ao fim da linha 1.
    test_command(
        &LINES,
        None,
        Selection::new(1, 1, 1, 1),
        |_| Box::new(ReplaceOvertypeCommand::new(Range::new(1, 5, 1, 5), text("XYZ"), None)),
        &["firsXYZ", "second line", "third"],
        Selection::new(1, 8, 1, 8),
        false,
    );
}

#[test]
fn replace_overtype_command_on_composition_end_removes_the_text_after_the_composition() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // A composição "ir" (colunas 2 a 4) apaga 2 caracteres depois dela: "st".
    test_command(
        &LINES,
        None,
        Selection::new(1, 1, 1, 1),
        |_| Box::new(ReplaceOvertypeCommandOnCompositionEnd::new(Range::new(1, 2, 1, 4))),
        &["fir", "second line", "third"],
        Selection::new(1, 4, 1, 4),
        false,
    );
}

#[test]
fn replace_command_that_preserves_selection_tracks_the_initial_selection() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Inserir "A\nB" no começo empurra a seleção rastreada da linha 2 para a linha 3.
    test_command(
        &LINES,
        None,
        Selection::new(2, 1, 2, 5),
        |sel| Box::new(ReplaceCommandThatPreservesSelection::new(Range::new(1, 1, 1, 1), text("A\nB"), sel, None)),
        &["A", "Bfirst", "second line", "third"],
        Selection::new(3, 1, 3, 5),
        false,
    );
}
