//! Porte de `src/vs/editor/test/browser/controller/cursorMoveCommand.test.ts` (suítes `Cursor move command test`,
//! `Cursor move by blankline test` e `Cursor move command - foldedLine unit`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado em `execute_test`, o único caminho que
//!   cada caso percorre. O carregamento do módulo `coreCommands` (que registra os comandos) acontece antes do guarda,
//!   como o `import` do original: `core_navigation_commands::instance()` cria e registra os comandos.
//! - `withTestCodeEditor(TEXT, {}, (editor, viewModel) => ...)` é `with_test_code_editor` de
//!   `tests/support/test_code_editor.rs` (as opções `{}` são o `Default`).
//! - `CoreNavigationCommands.X.runCoreEditorCommand(viewModel, args)` é `instance().x.run_core_editor_command`. O `args`
//!   do `CursorMove` é o objeto literal do original, montado como [`Dynamic`] (só com os campos definidos) e lido por
//!   `CursorMoveArgs::from_dynamic`, como o `Partial<...>` que o comando recebe.
//! - `viewModel.getCompletelyVisibleViewRange = () => new Range(...)` (atribuição no objeto) é
//!   `ViewModel::override_completely_visible_view_range`.
//! - `viewModel.setHiddenAreas(ranges)` é `set_hidden_areas(ranges, None, false)`.
//! - `assert.deepStrictEqual` de posição e seleção é `assert_eq!`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_editor::code_editor_widget::EditorViewModel;
use je_editor::core_commands::{CoreCommandArgs, CursorMoveArgs, EndCommandOptions, MoveCommandOptions, core_navigation_commands};
use je_editor::cursor_move_commands::cursor_move::{raw_direction, raw_unit};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use support::test_code_editor::{TestCodeEditorInstantiationOptions, with_test_code_editor};

/// A suíte `Cursor move command test`: `TEXT`.
const TEXT: &str = "    \tMy First Line\t \n\tMy Second Line\n    Third Line🐶\n\n1";

/// A suíte `Cursor move by blankline test`: `TEXT`.
const BLANK_LINE_TEXT: &str = "    \tMy First Line\t \n\tMy Second Line\n    Third Line🐶\n\n1\n2\n3\n\n         \na\nb";

/// A suíte `Cursor move command - foldedLine unit`: o texto de `executeFoldTest`.
const FOLD_TEXT: &str = "line1\nline2\nline3\nline4\nline5";

/// `executeTest(callback)` e `executeFoldTest(callback)`: o editor de teste com o texto dado.
fn execute_test(text: &str, callback: impl FnOnce(&EditorViewModel)) {
    // o carregamento do módulo `coreCommands`, antes do guarda de vazamentos
    core_navigation_commands::instance();
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor(text, TestCodeEditorInstantiationOptions::default(), |_editor, view_model, _| callback(view_model));
}

// Move command

/// `move(viewModel, args)`.
fn run_move(view_model: &EditorViewModel, args: Dynamic) {
    core_navigation_commands::instance().cursor_move.run_core_editor_command(view_model, &CursorMoveArgs::from_dynamic(&args));
}

/// O objeto `{ to, by, value, select }` do original, só com os campos definidos.
fn move_with(view_model: &EditorViewModel, to: &str, by: Option<&str>, value: Option<i32>, select: Option<bool>) {
    let mut entries = vec![("to", Dynamic::from(to))];
    if let Some(by) = by {
        entries.push(("by", Dynamic::from(by)));
    }
    if let Some(value) = value {
        entries.push(("value", Dynamic::Number(f64::from(value))));
    }
    if let Some(select) = select {
        entries.push(("select", Dynamic::Bool(select)));
    }
    run_move(view_model, Dynamic::object(entries));
}

fn move_to_line_start(view_model: &EditorViewModel) {
    move_with(view_model, raw_direction::WRAPPED_LINE_START, None, None, None);
}

fn move_to_line_first_non_whitespace_character(view_model: &EditorViewModel) {
    move_with(view_model, raw_direction::WRAPPED_LINE_FIRST_NON_WHITESPACE_CHARACTER, None, None, None);
}

fn move_to_line_center(view_model: &EditorViewModel) {
    move_with(view_model, raw_direction::WRAPPED_LINE_COLUMN_CENTER, None, None, None);
}

fn move_to_line_end(view_model: &EditorViewModel) {
    move_with(view_model, raw_direction::WRAPPED_LINE_END, None, None, None);
}

fn move_to_line_last_non_whitespace_character(view_model: &EditorViewModel) {
    move_with(view_model, raw_direction::WRAPPED_LINE_LAST_NON_WHITESPACE_CHARACTER, None, None, None);
}

fn move_left(view_model: &EditorViewModel, value: Option<i32>, by: Option<&str>, select: Option<bool>) {
    move_with(view_model, raw_direction::LEFT, by, value, select);
}

fn move_right(view_model: &EditorViewModel, value: Option<i32>, by: Option<&str>, select: Option<bool>) {
    move_with(view_model, raw_direction::RIGHT, by, value, select);
}

fn move_up(view_model: &EditorViewModel, no_of_lines: i32, select: Option<bool>) {
    move_with(view_model, raw_direction::UP, Some(raw_unit::WRAPPED_LINE), Some(no_of_lines), select);
}

fn move_up_by_blank_line(view_model: &EditorViewModel, select: Option<bool>) {
    move_with(view_model, raw_direction::PREV_BLANK_LINE, Some(raw_unit::WRAPPED_LINE), None, select);
}

fn move_up_by_model_line(view_model: &EditorViewModel, no_of_lines: i32, select: Option<bool>) {
    move_with(view_model, raw_direction::UP, None, Some(no_of_lines), select);
}

fn move_down(view_model: &EditorViewModel, no_of_lines: i32, select: Option<bool>) {
    move_with(view_model, raw_direction::DOWN, Some(raw_unit::WRAPPED_LINE), Some(no_of_lines), select);
}

fn move_down_by_blank_line(view_model: &EditorViewModel, select: Option<bool>) {
    move_with(view_model, raw_direction::NEXT_BLANK_LINE, Some(raw_unit::WRAPPED_LINE), None, select);
}

fn move_down_by_model_line(view_model: &EditorViewModel, no_of_lines: i32, select: Option<bool>) {
    move_with(view_model, raw_direction::DOWN, None, Some(no_of_lines), select);
}

fn move_down_by_folded_line(view_model: &EditorViewModel, no_of_lines: i32, select: Option<bool>) {
    move_with(view_model, raw_direction::DOWN, Some(raw_unit::FOLDED_LINE), Some(no_of_lines), select);
}

fn move_up_by_folded_line(view_model: &EditorViewModel, no_of_lines: i32, select: Option<bool>) {
    move_with(view_model, raw_direction::UP, Some(raw_unit::FOLDED_LINE), Some(no_of_lines), select);
}

fn move_to_top(view_model: &EditorViewModel, no_of_lines: i32, select: Option<bool>) {
    move_with(view_model, raw_direction::VIEW_PORT_TOP, None, Some(no_of_lines), select);
}

fn move_to_center(view_model: &EditorViewModel, select: Option<bool>) {
    move_with(view_model, raw_direction::VIEW_PORT_CENTER, None, None, select);
}

fn move_to_bottom(view_model: &EditorViewModel, no_of_lines: i32, select: Option<bool>) {
    move_with(view_model, raw_direction::VIEW_PORT_BOTTOM, None, Some(no_of_lines), select);
}

/// `cursorEqual(viewModel, posLineNumber, posColumn, selLineNumber = posLineNumber, selColumn = posColumn)`.
fn cursor_equal_with_selection(
    view_model: &EditorViewModel,
    pos_line_number: i32,
    pos_column: i32,
    sel_line_number: i32,
    sel_column: i32,
) {
    position_equal(view_model.get_position(), pos_line_number, pos_column);
    selection_equal(view_model.get_selection(), pos_line_number, pos_column, sel_line_number, sel_column);
}

/// `cursorEqual(viewModel, posLineNumber, posColumn)`: a seleção é vazia na posição.
fn cursor_equal(view_model: &EditorViewModel, pos_line_number: i32, pos_column: i32) {
    cursor_equal_with_selection(view_model, pos_line_number, pos_column, pos_line_number, pos_column);
}

fn position_equal(position: Position, line_number: i32, column: i32) {
    assert_eq!(position, Position::new(line_number, column), "position equal");
}

fn selection_equal(selection: Selection, pos_line_number: i32, pos_column: i32, sel_line_number: i32, sel_column: i32) {
    assert_eq!(
        (
            selection.selection_start_line_number,
            selection.selection_start_column,
            selection.position_line_number,
            selection.position_column
        ),
        (sel_line_number, sel_column, pos_line_number, pos_column),
        "selection equal"
    );
}

/// `moveTo(viewModel, lineNumber, column, inSelectionMode = false)`.
fn move_to(view_model: &EditorViewModel, line_number: i32, column: i32, in_selection_mode: bool) {
    let options = MoveCommandOptions { position: Some(Position::new(line_number, column)), ..MoveCommandOptions::default() };
    let commands = core_navigation_commands::instance();
    if in_selection_mode {
        commands.move_to_select.run_core_editor_command(view_model, &options);
    } else {
        commands.move_to.run_core_editor_command(view_model, &options);
    }
}

/// `moveToEndOfLine(viewModel, inSelectionMode = false)`: o argumento `{}`.
fn move_to_end_of_line(view_model: &EditorViewModel, in_selection_mode: bool) {
    let commands = core_navigation_commands::instance();
    if in_selection_mode {
        commands.cursor_end_select.run_core_editor_command(view_model, &EndCommandOptions::default());
    } else {
        commands.cursor_end.run_core_editor_command(view_model, &EndCommandOptions::default());
    }
}

mod cursor_move_command_test {
    use super::*;

    #[test]
    fn move_left_should_move_to_left_character() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 8, false);
            move_left(view_model, None, None, None);
            cursor_equal(view_model, 1, 7);
        });
    }

    #[test]
    fn move_left_should_move_to_left_by_n_characters() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 8, false);
            move_left(view_model, Some(3), None, None);
            cursor_equal(view_model, 1, 5);
        });
    }

    #[test]
    fn move_left_should_move_to_left_by_half_line() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 8, false);
            move_left(view_model, Some(1), Some(raw_unit::HALF_LINE), None);
            cursor_equal(view_model, 1, 1);
        });
    }

    #[test]
    fn move_left_moves_to_previous_line() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 2, 3, false);
            move_left(view_model, Some(10), None, None);
            cursor_equal(view_model, 1, 21);
        });
    }

    #[test]
    fn move_right_should_move_to_right_character() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 5, false);
            move_right(view_model, None, None, None);
            cursor_equal(view_model, 1, 6);
        });
    }

    #[test]
    fn move_right_should_move_to_right_by_n_characters() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 2, false);
            move_right(view_model, Some(6), None, None);
            cursor_equal(view_model, 1, 8);
        });
    }

    #[test]
    fn move_right_should_move_to_right_by_half_line() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 4, false);
            move_right(view_model, Some(1), Some(raw_unit::HALF_LINE), None);
            cursor_equal(view_model, 1, 14);
        });
    }

    #[test]
    fn move_right_moves_to_next_line() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 8, false);
            move_right(view_model, Some(100), None, None);
            cursor_equal(view_model, 2, 1);
        });
    }

    #[test]
    fn move_to_first_character_of_line_from_middle() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 8, false);
            move_to_line_start(view_model);
            cursor_equal(view_model, 1, 1);
        });
    }

    #[test]
    fn move_to_first_character_of_line_from_first_non_white_space_character() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 6, false);
            move_to_line_start(view_model);
            cursor_equal(view_model, 1, 1);
        });
    }

    #[test]
    fn move_to_first_character_of_line_from_first_character() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 1, false);
            move_to_line_start(view_model);
            cursor_equal(view_model, 1, 1);
        });
    }

    #[test]
    fn move_to_first_non_white_space_character_of_line_from_middle() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 8, false);
            move_to_line_first_non_whitespace_character(view_model);
            cursor_equal(view_model, 1, 6);
        });
    }

    #[test]
    fn move_to_first_non_white_space_character_of_line_from_first_non_white_space_character() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 6, false);
            move_to_line_first_non_whitespace_character(view_model);
            cursor_equal(view_model, 1, 6);
        });
    }

    #[test]
    fn move_to_first_non_white_space_character_of_line_from_first_character() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 1, false);
            move_to_line_first_non_whitespace_character(view_model);
            cursor_equal(view_model, 1, 6);
        });
    }

    #[test]
    fn move_to_end_of_line_from_middle() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 8, false);
            move_to_line_end(view_model);
            cursor_equal(view_model, 1, 21);
        });
    }

    #[test]
    fn move_to_end_of_line_from_last_non_white_space_character() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 19, false);
            move_to_line_end(view_model);
            cursor_equal(view_model, 1, 21);
        });
    }

    #[test]
    fn move_to_end_of_line_from_line_end() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 21, false);
            move_to_line_end(view_model);
            cursor_equal(view_model, 1, 21);
        });
    }

    #[test]
    fn move_to_last_non_white_space_character_from_middle() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 8, false);
            move_to_line_last_non_whitespace_character(view_model);
            cursor_equal(view_model, 1, 19);
        });
    }

    #[test]
    fn move_to_last_non_white_space_character_from_last_non_white_space_character() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 19, false);
            move_to_line_last_non_whitespace_character(view_model);
            cursor_equal(view_model, 1, 19);
        });
    }

    #[test]
    fn move_to_last_non_white_space_character_from_line_end() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 21, false);
            move_to_line_last_non_whitespace_character(view_model);
            cursor_equal(view_model, 1, 19);
        });
    }

    #[test]
    fn move_to_center_of_line_not_from_center() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 8, false);
            move_to_line_center(view_model);
            cursor_equal(view_model, 1, 11);
        });
    }

    #[test]
    fn move_to_center_of_line_from_center() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 11, false);
            move_to_line_center(view_model);
            cursor_equal(view_model, 1, 11);
        });
    }

    #[test]
    fn move_to_center_of_line_from_start() {
        execute_test(TEXT, |view_model| {
            move_to_line_start(view_model);
            move_to_line_center(view_model);
            cursor_equal(view_model, 1, 11);
        });
    }

    #[test]
    fn move_to_center_of_line_from_end() {
        execute_test(TEXT, |view_model| {
            move_to_line_end(view_model);
            move_to_line_center(view_model);
            cursor_equal(view_model, 1, 11);
        });
    }

    #[test]
    fn move_up_by_cursor_move_command() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 3, 5, false);
            cursor_equal(view_model, 3, 5);

            move_up(view_model, 2, None);
            cursor_equal(view_model, 1, 5);

            move_up(view_model, 1, None);
            cursor_equal(view_model, 1, 1);
        });
    }

    #[test]
    fn move_up_by_model_line_cursor_move_command() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 3, 5, false);
            cursor_equal(view_model, 3, 5);

            move_up_by_model_line(view_model, 2, None);
            cursor_equal(view_model, 1, 5);

            move_up_by_model_line(view_model, 1, None);
            cursor_equal(view_model, 1, 1);
        });
    }

    #[test]
    fn move_down_by_model_line_cursor_move_command() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 3, 5, false);
            cursor_equal(view_model, 3, 5);

            move_down_by_model_line(view_model, 2, None);
            cursor_equal(view_model, 5, 2);

            move_down_by_model_line(view_model, 1, None);
            cursor_equal(view_model, 5, 2);
        });
    }

    #[test]
    fn move_up_with_selection_by_cursor_move_command() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 3, 5, false);
            cursor_equal(view_model, 3, 5);

            move_up(view_model, 1, Some(true));
            cursor_equal_with_selection(view_model, 2, 2, 3, 5);

            move_up(view_model, 1, Some(true));
            cursor_equal_with_selection(view_model, 1, 5, 3, 5);
        });
    }

    #[test]
    fn move_up_and_down_with_tabs_by_cursor_move_command() {
        execute_test(TEXT, |view_model| {
            move_to(view_model, 1, 5, false);
            cursor_equal(view_model, 1, 5);

            move_down(view_model, 4, None);
            cursor_equal(view_model, 5, 2);

            move_up(view_model, 1, None);
            cursor_equal(view_model, 4, 1);

            move_up(view_model, 1, None);
            cursor_equal(view_model, 3, 5);

            move_up(view_model, 1, None);
            cursor_equal(view_model, 2, 2);

            move_up(view_model, 1, None);
            cursor_equal(view_model, 1, 5);
        });
    }

    #[test]
    fn move_up_and_down_with_end_of_lines_starting_from_a_long_one_by_cursor_move_command() {
        execute_test(TEXT, |view_model| {
            move_to_end_of_line(view_model, false);
            cursor_equal(view_model, 1, 21);

            move_to_end_of_line(view_model, false);
            cursor_equal(view_model, 1, 21);

            move_down(view_model, 2, None);
            cursor_equal(view_model, 3, 17);

            move_down(view_model, 1, None);
            cursor_equal(view_model, 4, 1);

            move_down(view_model, 1, None);
            cursor_equal(view_model, 5, 2);

            move_up(view_model, 4, None);
            cursor_equal(view_model, 1, 21);
        });
    }

    #[test]
    fn move_to_view_top_line_moves_to_first_visible_line_if_it_is_first_line() {
        execute_test(TEXT, |view_model| {
            view_model.override_completely_visible_view_range(Some(Range::new(1, 1, 10, 1)));

            move_to(view_model, 2, 2, false);
            move_to_top(view_model, 1, None);

            cursor_equal(view_model, 1, 6);
        });
    }

    #[test]
    fn move_to_view_top_line_moves_to_top_visible_line_when_first_line_is_not_visible() {
        execute_test(TEXT, |view_model| {
            view_model.override_completely_visible_view_range(Some(Range::new(2, 1, 10, 1)));

            move_to(view_model, 4, 1, false);
            move_to_top(view_model, 1, None);

            cursor_equal(view_model, 2, 2);
        });
    }

    #[test]
    fn move_to_view_top_line_moves_to_nth_line_from_top() {
        execute_test(TEXT, |view_model| {
            view_model.override_completely_visible_view_range(Some(Range::new(1, 1, 10, 1)));

            move_to(view_model, 4, 1, false);
            move_to_top(view_model, 3, None);

            cursor_equal(view_model, 3, 5);
        });
    }

    #[test]
    fn move_to_view_top_line_moves_to_last_line_if_n_is_greater_than_last_visible_line_number() {
        execute_test(TEXT, |view_model| {
            view_model.override_completely_visible_view_range(Some(Range::new(1, 1, 3, 1)));

            move_to(view_model, 2, 2, false);
            move_to_top(view_model, 4, None);

            cursor_equal(view_model, 3, 5);
        });
    }

    #[test]
    fn move_to_view_center_line_moves_to_the_center_line() {
        execute_test(TEXT, |view_model| {
            view_model.override_completely_visible_view_range(Some(Range::new(3, 1, 3, 1)));

            move_to(view_model, 2, 2, false);
            move_to_center(view_model, None);

            cursor_equal(view_model, 3, 5);
        });
    }

    #[test]
    fn move_to_view_bottom_line_moves_to_last_visible_line_if_it_is_last_line() {
        execute_test(TEXT, |view_model| {
            view_model.override_completely_visible_view_range(Some(Range::new(1, 1, 5, 1)));

            move_to(view_model, 2, 2, false);
            move_to_bottom(view_model, 1, None);

            cursor_equal(view_model, 5, 1);
        });
    }

    #[test]
    fn move_to_view_bottom_line_moves_to_last_visible_line_when_last_line_is_not_visible() {
        execute_test(TEXT, |view_model| {
            view_model.override_completely_visible_view_range(Some(Range::new(2, 1, 3, 1)));

            move_to(view_model, 2, 2, false);
            move_to_bottom(view_model, 1, None);

            cursor_equal(view_model, 3, 5);
        });
    }

    #[test]
    fn move_to_view_bottom_line_moves_to_nth_line_from_bottom() {
        execute_test(TEXT, |view_model| {
            view_model.override_completely_visible_view_range(Some(Range::new(1, 1, 5, 1)));

            move_to(view_model, 4, 1, false);
            move_to_bottom(view_model, 3, None);

            cursor_equal(view_model, 3, 5);
        });
    }

    #[test]
    fn move_to_view_bottom_line_moves_to_first_line_if_n_is_lesser_than_first_visible_line_number() {
        execute_test(TEXT, |view_model| {
            view_model.override_completely_visible_view_range(Some(Range::new(2, 1, 5, 1)));

            move_to(view_model, 4, 1, false);
            move_to_bottom(view_model, 5, None);

            cursor_equal(view_model, 2, 2);
        });
    }
}

mod cursor_move_by_blankline_test {
    use super::*;

    #[test]
    fn move_down_should_move_to_start_of_next_blank_line() {
        execute_test(BLANK_LINE_TEXT, |view_model| {
            move_down_by_blank_line(view_model, Some(false));
            cursor_equal(view_model, 4, 1);
        });
    }

    #[test]
    fn move_up_should_move_to_start_of_previous_blank_line() {
        execute_test(BLANK_LINE_TEXT, |view_model| {
            move_to(view_model, 7, 1, false);
            move_up_by_blank_line(view_model, Some(false));
            cursor_equal(view_model, 4, 1);
        });
    }

    #[test]
    fn move_down_should_skip_over_whitespace_if_already_on_blank_line() {
        execute_test(BLANK_LINE_TEXT, |view_model| {
            move_to(view_model, 8, 1, false);
            move_down_by_blank_line(view_model, Some(false));
            cursor_equal(view_model, 11, 1);
        });
    }

    #[test]
    fn move_up_should_skip_over_whitespace_if_already_on_blank_line() {
        execute_test(BLANK_LINE_TEXT, |view_model| {
            move_to(view_model, 9, 1, false);
            move_up_by_blank_line(view_model, Some(false));
            cursor_equal(view_model, 4, 1);
        });
    }

    #[test]
    fn move_up_should_go_to_first_column_of_first_line_if_not_empty() {
        execute_test(BLANK_LINE_TEXT, |view_model| {
            move_to(view_model, 2, 1, false);
            move_up_by_blank_line(view_model, Some(false));
            cursor_equal(view_model, 1, 1);
        });
    }

    #[test]
    fn move_down_should_go_to_first_column_of_last_line_if_not_empty() {
        execute_test(BLANK_LINE_TEXT, |view_model| {
            move_to(view_model, 10, 1, false);
            move_down_by_blank_line(view_model, Some(false));
            cursor_equal(view_model, 11, 1);
        });
    }

    #[test]
    fn select_down_should_select_to_start_of_next_blank_line() {
        execute_test(BLANK_LINE_TEXT, |view_model| {
            move_down_by_blank_line(view_model, Some(true));
            selection_equal(view_model.get_selection(), 4, 1, 1, 1);
        });
    }

    #[test]
    fn select_up_should_select_to_start_of_previous_blank_line() {
        execute_test(BLANK_LINE_TEXT, |view_model| {
            move_to(view_model, 7, 1, false);
            move_up_by_blank_line(view_model, Some(true));
            selection_equal(view_model.get_selection(), 4, 1, 7, 1);
        });
    }
}

// Tests for 'foldedLine' unit: moves by model lines but treats each fold as a single step.
// This is the semantics required by vim's j/k: move through visible lines, skip hidden ones.

mod cursor_move_command_folded_line_unit {
    use super::*;

    #[test]
    fn move_down_by_folded_line_skips_a_fold_below_the_cursor() {
        execute_test(FOLD_TEXT, |view_model| {
            // Line 4 is hidden (folded under line 3 as header)
            view_model.set_hidden_areas(vec![Range::new(4, 1, 4, 1)], None, false);
            move_to(view_model, 2, 1, false);
            // j from line 2 → line 3 (visible fold header)
            move_down_by_folded_line(view_model, 1, None);
            cursor_equal(view_model, 3, 1);
            // j from line 3 (fold header) → line 4 is hidden, lands on line 5
            move_down_by_folded_line(view_model, 1, None);
            cursor_equal(view_model, 5, 1);
        });
    }

    #[test]
    fn move_up_by_folded_line_skips_a_fold_above_the_cursor() {
        execute_test(FOLD_TEXT, |view_model| {
            // Line 3 is hidden (folded under line 2 as header)
            view_model.set_hidden_areas(vec![Range::new(3, 1, 3, 1)], None, false);
            move_to(view_model, 4, 1, false);
            // k from line 4: line 3 is hidden, lands on line 2 (fold header)
            move_up_by_folded_line(view_model, 1, None);
            cursor_equal(view_model, 2, 1);
            // k from line 2 → line 1
            move_up_by_folded_line(view_model, 1, None);
            cursor_equal(view_model, 1, 1);
        });
    }

    #[test]
    fn move_down_by_folded_line_with_count_treats_each_fold_as_one_step() {
        execute_test(FOLD_TEXT, |view_model| {
            // Line 3 is hidden
            view_model.set_hidden_areas(vec![Range::new(3, 1, 3, 1)], None, false);
            move_to(view_model, 1, 1, false);
            // 3j from line 1: step1→2, step2→3(hidden)→4, step3→5
            move_down_by_folded_line(view_model, 3, None);
            cursor_equal(view_model, 5, 1);
        });
    }

    #[test]
    fn move_down_by_folded_line_skips_a_multi_line_fold_as_one_step() {
        execute_test(FOLD_TEXT, |view_model| {
            // Lines 2-4 are hidden (folded under line 1 as header)
            view_model.set_hidden_areas(vec![Range::new(2, 1, 4, 1)], None, false);
            move_to(view_model, 1, 1, false);
            // j from line 1: lines 2-4 are all hidden, lands directly on line 5
            move_down_by_folded_line(view_model, 1, None);
            cursor_equal(view_model, 5, 1);
        });
    }

    #[test]
    fn move_down_by_folded_line_at_last_line_stays_at_last_line() {
        execute_test(FOLD_TEXT, |view_model| {
            move_to(view_model, 5, 1, false);
            move_down_by_folded_line(view_model, 1, None);
            cursor_equal(view_model, 5, 1);
        });
    }

    #[test]
    fn move_up_by_folded_line_at_first_line_stays_at_first_line() {
        execute_test(FOLD_TEXT, |view_model| {
            move_to(view_model, 1, 1, false);
            move_up_by_folded_line(view_model, 1, None);
            cursor_equal(view_model, 1, 1);
        });
    }

    #[test]
    fn move_down_by_folded_line_with_count_clamps_to_last_visible_line_after_fold() {
        execute_test(FOLD_TEXT, |view_model| {
            // Lines 2-4 are hidden. Visible lines are 1 and 5.
            view_model.set_hidden_areas(vec![Range::new(2, 1, 4, 1)], None, false);
            move_to(view_model, 1, 1, false);
            // 2j should land on line 5 and clamp there.
            move_down_by_folded_line(view_model, 2, None);
            cursor_equal(view_model, 5, 1);
        });
    }

    #[test]
    fn move_up_by_folded_line_with_count_clamps_to_first_visible_line_before_fold() {
        execute_test(FOLD_TEXT, |view_model| {
            // Lines 2-4 are hidden. Visible lines are 1 and 5.
            view_model.set_hidden_areas(vec![Range::new(2, 1, 4, 1)], None, false);
            move_to(view_model, 5, 1, false);
            // 2k should land on line 1 and clamp there.
            move_up_by_folded_line(view_model, 2, None);
            cursor_equal(view_model, 1, 1);
        });
    }
}
