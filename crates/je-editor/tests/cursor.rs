//! Porte de `src/vs/editor/test/browser/controller/cursor.test.ts` (suítes `Editor Controller - Cursor`, `Editor Controller`,
//! `Undo stops`, `Overtype Mode`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado em `run_cursor_test` e nos casos que não passam por
//!   ele. O carregamento do módulo `coreCommands` (que registra os comandos) acontece antes do guarda, como o `import` do
//!   original: `core_navigation_commands::instance()` cria e registra os comandos.
//! - `withTestCodeEditor(text, {}, (editor, viewModel) => ...)` é `with_test_code_editor` de
//!   `tests/support/test_code_editor.rs` (as opções `{}` são o `Default`).
//! - `CoreNavigationCommands.X.runCoreEditorCommand(viewModel, args)` é `instance().x.run_core_editor_command`, com a struct
//!   de argumentos do comando (`{}` é o `Default`).
//! - `assert.deepStrictEqual` de seleções compara o `toString()` das seleções, como o `assertCursor` do original.
//!
//! Divergências:
//! - `editor.runCommand(CoreNavigationCommands.CursorDown, null)` é `run_core_editor_command(view_model, &Default)`: o
//!   `CoreEditorCommand` do porte não implementa `ITestEditorCommand` e o `runEditorCommand` do original só repassa o
//!   `viewModel` ao `runCoreEditorCommand` com `args || {}`.
//! - `JSON.parse(JSON.stringify(viewModel.saveCursorState()))` não tem passagem por texto: o `SavedCursorState` é
//!   copiado (o estado é só números, a ida e volta por JSON não muda nada).
//! - Os textos de `grapheme breaking` usam escapes `\u{...}` para os caracteres combinantes e o seletor de variação.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable as _;
use je_editor::code_editor_widget::{EditorViewModel, ICodeEditor, SelectionOrRange};
use je_editor::core_commands::{
    BaseCommandOptions, ColumnSelectCommandOptions, CursorMoveCommandOptions, EndCommandOptions, MoveCommandOptions,
    core_navigation_commands,
};
use je_editor::cursor_events::{CursorChangeReason, ICursorPositionChangedEvent};
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::languages::{EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult};
use je_editor::model::{InjectedTextOptions, ModelDecorationOptionsInit, ModelDeltaDecoration};
use je_editor::null_tokenize::null_state;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::{ModelDecorationOptions, TextModel};
use je_editor::tokenization_registry::tokenization_registry;
use je_editor::view_model_event_dispatcher::OutgoingViewModelEvent;
use je_editor::view_model_types::IViewModel;
use support::test_code_editor::{ITestCodeEditor, TestCodeEditorInstantiationOptions, with_test_code_editor};
use support::test_text_model::{create_text_model, create_text_model_with};

// --------- utils

fn move_to(editor: &ITestCodeEditor, view_model: &EditorViewModel, line_number: i32, column: i32, in_selection_mode: bool) {
    let _ = editor;
    let options = MoveCommandOptions { position: Some(Position::new(line_number, column)), ..MoveCommandOptions::default() };
    let commands = core_navigation_commands::instance();
    if in_selection_mode {
        commands.move_to_select.run_core_editor_command(view_model, &options);
    } else {
        commands.move_to.run_core_editor_command(view_model, &options);
    }
}

/// O par `(comando, comando com seleção)` de um movimento com `CursorMoveCommandOptions` vazio, escolhido por
/// `in_selection_mode`.
fn run_cursor_move(
    view_model: &EditorViewModel,
    in_selection_mode: bool,
    pick: impl Fn(&core_navigation_commands::CoreNavigationCommands) -> (&Rc<je_editor::core_commands::CoreEditorCommand<CursorMoveCommandOptions>>, &Rc<je_editor::core_commands::CoreEditorCommand<CursorMoveCommandOptions>>),
) {
    let commands = core_navigation_commands::instance();
    let (plain, select) = pick(&commands);
    let command = if in_selection_mode { select } else { plain };
    command.run_core_editor_command(view_model, &CursorMoveCommandOptions::default());
}

fn move_left(editor: &ITestCodeEditor, view_model: &EditorViewModel, in_selection_mode: bool) {
    let _ = editor;
    run_cursor_move(view_model, in_selection_mode, |c| (&c.cursor_left, &c.cursor_left_select));
}

fn move_right(editor: &ITestCodeEditor, view_model: &EditorViewModel, in_selection_mode: bool) {
    let _ = editor;
    run_cursor_move(view_model, in_selection_mode, |c| (&c.cursor_right, &c.cursor_right_select));
}

fn move_down(editor: &ITestCodeEditor, view_model: &EditorViewModel, in_selection_mode: bool) {
    let _ = editor;
    run_cursor_move(view_model, in_selection_mode, |c| (&c.cursor_down, &c.cursor_down_select));
}

fn move_up(editor: &ITestCodeEditor, view_model: &EditorViewModel, in_selection_mode: bool) {
    let _ = editor;
    run_cursor_move(view_model, in_selection_mode, |c| (&c.cursor_up, &c.cursor_up_select));
}

/// O comando com `BaseCommandOptions` vazio, escolhido por `in_selection_mode`.
fn run_base_move(
    view_model: &EditorViewModel,
    in_selection_mode: bool,
    pick: impl Fn(&core_navigation_commands::CoreNavigationCommands) -> (&Rc<je_editor::core_commands::CoreEditorCommand<BaseCommandOptions>>, &Rc<je_editor::core_commands::CoreEditorCommand<BaseCommandOptions>>),
) {
    let commands = core_navigation_commands::instance();
    let (plain, select) = pick(&commands);
    let command = if in_selection_mode { select } else { plain };
    command.run_core_editor_command(view_model, &BaseCommandOptions::default());
}

fn move_to_beginning_of_line(editor: &ITestCodeEditor, view_model: &EditorViewModel, in_selection_mode: bool) {
    let _ = editor;
    run_base_move(view_model, in_selection_mode, |c| (&c.cursor_home, &c.cursor_home_select));
}

fn move_to_end_of_line(editor: &ITestCodeEditor, view_model: &EditorViewModel, in_selection_mode: bool) {
    let _ = editor;
    let commands = core_navigation_commands::instance();
    if in_selection_mode {
        commands.cursor_end_select.run_core_editor_command(view_model, &EndCommandOptions::default());
    } else {
        commands.cursor_end.run_core_editor_command(view_model, &EndCommandOptions::default());
    }
}

fn move_to_beginning_of_buffer(editor: &ITestCodeEditor, view_model: &EditorViewModel, in_selection_mode: bool) {
    let _ = editor;
    run_base_move(view_model, in_selection_mode, |c| (&c.cursor_top, &c.cursor_top_select));
}

fn move_to_end_of_buffer(editor: &ITestCodeEditor, view_model: &EditorViewModel, in_selection_mode: bool) {
    let _ = editor;
    run_base_move(view_model, in_selection_mode, |c| (&c.cursor_bottom, &c.cursor_bottom_select));
}

/// O `Position | Selection | Selection[]` do `assertCursor`.
pub enum CursorExpectation {
    Position(Position),
    Selections(Vec<Selection>),
}

impl From<Position> for CursorExpectation {
    fn from(position: Position) -> Self {
        CursorExpectation::Position(position)
    }
}

impl From<Selection> for CursorExpectation {
    fn from(selection: Selection) -> Self {
        CursorExpectation::Selections(vec![selection])
    }
}

impl From<Vec<Selection>> for CursorExpectation {
    fn from(selections: Vec<Selection>) -> Self {
        CursorExpectation::Selections(selections)
    }
}

impl From<&[Selection]> for CursorExpectation {
    fn from(selections: &[Selection]) -> Self {
        CursorExpectation::Selections(selections.to_vec())
    }
}

fn assert_cursor(view_model: &EditorViewModel, what: impl Into<CursorExpectation>) {
    let selections = match what.into() {
        CursorExpectation::Position(position) => {
            vec![Selection::new(position.line_number, position.column, position.line_number, position.column)]
        }
        CursorExpectation::Selections(selections) => selections,
    };
    let actual: Vec<String> = view_model.get_selections().iter().map(ToString::to_string).collect();
    let expected: Vec<String> = selections.iter().map(ToString::to_string).collect();

    assert_eq!(actual, expected);
}

/// `{}` como argumento de comando sem struct própria.
fn empty_args() -> Dynamic {
    Dynamic::object(std::iter::empty::<(&str, Dynamic)>())
}

// --------- suíte `Editor Controller - Cursor`

const LINE1: &str = "    \tMy First Line\t ";
const LINE2: &str = "\tMy Second Line";
const LINE3: &str = "    Third Line🐶";
const LINE4: &str = "";
const LINE5: &str = "1";

/// `TEXT`.
const TEXT: &str = "    \tMy First Line\t \r\n\tMy Second Line\n    Third Line🐶\n\r\n1";

/// `LINEn.length` (unidades UTF-16, como o `length` do original).
fn utf16_len(line: &str) -> i32 {
    line.encode_utf16().count() as i32
}

/// `runTest(callback)`.
fn run_cursor_test(callback: impl FnOnce(&ITestCodeEditor, &EditorViewModel)) {
    // o carregamento do módulo `coreCommands`, antes do guarda de vazamentos
    core_navigation_commands::instance();
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor(TEXT, TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| {
        callback(editor, view_model)
    });
}

/// `withTestCodeEditor(text, {}, callback)` com o guarda de vazamentos.
fn with_editor(text: impl Into<support::test_code_editor::TestEditorInput>, callback: impl FnOnce(&ITestCodeEditor, &EditorViewModel)) {
    core_navigation_commands::instance();
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor(text, TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| {
        callback(editor, view_model)
    });
}

/// `viewModel.setSelections('test', selections)`.
fn set_selections_test(view_model: &EditorViewModel, selections: &[Selection]) {
    view_model.set_selections(Some("test"), selections, CursorChangeReason::NotSet);
}

/// As opções `{ wrappingIndent: 'indent', wordWrap: 'wordWrapColumn', wordWrapColumn: 20 }`.
fn cursor_word_wrap_options() -> TestCodeEditorInstantiationOptions {
    TestCodeEditorInstantiationOptions::new(Dynamic::object([
        ("wrappingIndent", Dynamic::from("indent")),
        ("wordWrap", Dynamic::from("wordWrapColumn")),
        ("wordWrapColumn", Dynamic::Number(20.0)),
    ]))
}

/// O corpo comum de `issue #144041` e `issue #140195`: quatro `CursorDown`, quatro `CursorUp` e as posições de visão
/// medidas antes e depois de cada um. `with_injected_text` acrescenta a decoração com texto injetado do segundo caso.
fn check_cursor_up_down_positions(with_injected_text: bool, expected: &[&str]) {
    core_navigation_commands::instance();
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_text_model(&["Word1 Word2 Word3 Word4", "Word5 Word6 Word7 Word8"].join("\n"));

    with_test_code_editor(model.clone(), cursor_word_wrap_options(), |editor, view_model, _| {
        if with_injected_text {
            editor.change_decorations(|change_accessor| {
                change_accessor.delta_decorations(
                    &[],
                    &[ModelDeltaDecoration {
                        range: Range::new(1, 22, 1, 22),
                        options: ModelDecorationOptions::register(&ModelDecorationOptionsInit {
                            description: "test".to_owned(),
                            show_if_collapsed: true,
                            after: Some(InjectedTextOptions {
                                content: "some very very very very very very very very long text".to_owned(),
                                ..InjectedTextOptions::default()
                            }),
                            ..Default::default()
                        }),
                    }],
                );
            });
        }
        set_selections_test(view_model, &[Selection::new(1, 1, 1, 1)]);

        let mut cursor_positions: Vec<String> = Vec::new();
        let mut report_cursor_position = |view_model: &EditorViewModel| {
            cursor_positions.push(view_model.get_cursor_states()[0].view_state.position.to_string());
        };

        let commands = core_navigation_commands::instance();
        report_cursor_position(view_model);
        for _ in 0..4 {
            commands.cursor_down.run_core_editor_command(view_model, &CursorMoveCommandOptions::default());
            report_cursor_position(view_model);
        }
        for _ in 0..4 {
            commands.cursor_up.run_core_editor_command(view_model, &CursorMoveCommandOptions::default());
            report_cursor_position(view_model);
        }

        let expected: Vec<String> = expected.iter().map(|position| (*position).to_owned()).collect();
        assert_eq!(cursor_positions, expected);
    });

    model.dispose();
}

/// `Selection(line, start_column, line, end_column)` para cada linha `i + 1`, com os fins dados.
fn column_selections(start_column: i32, end_columns: &[i32]) -> Vec<Selection> {
    end_columns
        .iter()
        .enumerate()
        .map(|(index, end_column)| {
            let line_number = index as i32 + 1;
            Selection::new(line_number, start_column, line_number, *end_column)
        })
        .collect()
}

/// `n` vezes `CursorColumnSelectRight` (ou `Left`, `Up`, `Down`) escolhido por `pick`.
fn repeat_column_select(
    view_model: &EditorViewModel,
    times: usize,
    pick: impl Fn(&core_navigation_commands::CoreNavigationCommands) -> &Rc<je_editor::core_commands::CoreEditorCommand<BaseCommandOptions>>,
) {
    let commands = core_navigation_commands::instance();
    for _ in 0..times {
        pick(&commands).run_core_editor_command(view_model, &BaseCommandOptions::default());
    }
}

/// O `ColumnSelect` com `{ position, viewPosition, mouseColumn, doColumnSelect: true }`.
fn column_select(view_model: &EditorViewModel, position: Position, mouse_column: i32) {
    core_navigation_commands::instance().column_select.run_core_editor_command(
        view_model,
        &ColumnSelectCommandOptions {
            position: Some(position),
            view_position: Some(position),
            mouse_column: Some(mouse_column),
            do_column_select: true,
            ..ColumnSelectCommandOptions::default()
        },
    );
}

/// As dez linhas de `<property .../>` dos casos `issue #20087`.
const PROPERTY_LINES: &str = concat!(
    "<property id=\"SomeThing\" key=\"SomeKey\" value=\"000\"/>\n",
    "<property id=\"SomeThing\" key=\"SomeKey\" value=\"000\"/>\n",
    "<property id=\"SomeThing\" Key=\"SomeKey\" value=\"000\"/>\n",
    "<property id=\"SomeThing\" key=\"SomeKey\" value=\"000\"/>\n",
    "<property id=\"SomeThing\" key=\"SoMEKEy\" value=\"000\"/>\n",
    "<property id=\"SomeThing\" key=\"SomeKey\" value=\"000\"/>\n",
    "<property id=\"SomeThing\" key=\"SomeKey\" value=\"000\"/>\n",
    "<property id=\"SomeThing\" key=\"SomeKey\" valuE=\"000\"/>\n",
    "<property id=\"SomeThing\" key=\"SomeKey\" value=\"000\"/>\n",
    "<property id=\"SomeThing\" key=\"SomeKey\" value=\"00X\"/>"
);

/// As sete linhas de `var ... = require(...)` dos casos de coluna.
const REQUIRE_LINES: &str = concat!(
    "var gulp = require(\"gulp\");\n",
    "var path = require(\"path\");\n",
    "var rimraf = require(\"rimraf\");\n",
    "var isarray = require(\"isarray\");\n",
    "var merge = require(\"merge-stream\");\n",
    "var concat = require(\"gulp-concat\");\n",
    "var newer = require(\"gulp-newer\");"
);

/// O suporte de tokenização do caso `setSelection / setPosition with source`: tokens vazios.
struct CursorSuiteEmptyTokenizationSupport;

impl ITokenizationSupport for CursorSuiteEmptyTokenizationSupport {
    fn get_initial_state(&self) -> State {
        null_state()
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        unreachable!("`tokenize: undefined!` no original")
    }

    fn tokenize_encoded(&self, _line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        EncodedTokenizationResult { tokens: Vec::new(), font_info: Vec::new(), end_state: state.clone() }
    }
}

mod editor_controller_cursor {
    use super::*;

    #[test]
    fn cursor_cursor_initialized() {
        run_cursor_test(|_editor, view_model| {
            assert_cursor(view_model, Position::new(1, 1));
        });
    }

    // --------- absolute move

    #[test]
    fn cursor_no_move() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 1, false);
            assert_cursor(view_model, Position::new(1, 1));
        });
    }

    #[test]
    fn cursor_move() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 2, false);
            assert_cursor(view_model, Position::new(1, 2));
        });
    }

    #[test]
    fn cursor_move_in_selection_mode() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 2, true);
            assert_cursor(view_model, Selection::new(1, 1, 1, 2));
        });
    }

    #[test]
    fn cursor_move_beyond_line_end() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 25, false);
            assert_cursor(view_model, Position::new(1, utf16_len(LINE1) + 1));
        });
    }

    #[test]
    fn cursor_move_empty_line() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 4, 20, false);
            assert_cursor(view_model, Position::new(4, 1));
        });
    }

    #[test]
    fn cursor_move_one_char_line() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 5, 20, false);
            assert_cursor(view_model, Position::new(5, 2));
        });
    }

    #[test]
    fn cursor_selection_down() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 2, 1, true);
            assert_cursor(view_model, Selection::new(1, 1, 2, 1));
        });
    }

    #[test]
    fn cursor_move_and_then_select() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 2, 3, false);
            assert_cursor(view_model, Position::new(2, 3));

            move_to(editor, view_model, 2, 15, true);
            assert_cursor(view_model, Selection::new(2, 3, 2, 15));

            move_to(editor, view_model, 1, 2, true);
            assert_cursor(view_model, Selection::new(2, 3, 1, 2));
        });
    }

    // --------- move left

    #[test]
    fn cursor_move_left_on_top_left_position() {
        run_cursor_test(|editor, view_model| {
            move_left(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 1));
        });
    }

    #[test]
    fn cursor_move_left() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 3, false);
            assert_cursor(view_model, Position::new(1, 3));
            move_left(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 2));
        });
    }

    #[test]
    fn cursor_move_left_with_surrogate_pair() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 17, false);
            assert_cursor(view_model, Position::new(3, 17));
            move_left(editor, view_model, false);
            assert_cursor(view_model, Position::new(3, 15));
        });
    }

    #[test]
    fn cursor_move_left_goes_to_previous_row() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 2, 1, false);
            assert_cursor(view_model, Position::new(2, 1));
            move_left(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 21));
        });
    }

    #[test]
    fn cursor_move_left_selection() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 2, 1, false);
            assert_cursor(view_model, Position::new(2, 1));
            move_left(editor, view_model, true);
            assert_cursor(view_model, Selection::new(2, 1, 1, 21));
        });
    }

    // --------- move right

    #[test]
    fn cursor_move_right_on_bottom_right_position() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 5, 2, false);
            assert_cursor(view_model, Position::new(5, 2));
            move_right(editor, view_model, false);
            assert_cursor(view_model, Position::new(5, 2));
        });
    }

    #[test]
    fn cursor_move_right() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 3, false);
            assert_cursor(view_model, Position::new(1, 3));
            move_right(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 4));
        });
    }

    #[test]
    fn cursor_move_right_with_surrogate_pair() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 15, false);
            assert_cursor(view_model, Position::new(3, 15));
            move_right(editor, view_model, false);
            assert_cursor(view_model, Position::new(3, 17));
        });
    }

    #[test]
    fn cursor_move_right_goes_to_next_row() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 21, false);
            assert_cursor(view_model, Position::new(1, 21));
            move_right(editor, view_model, false);
            assert_cursor(view_model, Position::new(2, 1));
        });
    }

    #[test]
    fn cursor_move_right_selection() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 21, false);
            assert_cursor(view_model, Position::new(1, 21));
            move_right(editor, view_model, true);
            assert_cursor(view_model, Selection::new(1, 21, 2, 1));
        });
    }

    // --------- move down

    #[test]
    fn cursor_move_down() {
        run_cursor_test(|editor, view_model| {
            for expected in [(2, 1), (3, 1), (4, 1), (5, 1), (5, 2)] {
                move_down(editor, view_model, false);
                assert_cursor(view_model, Position::new(expected.0, expected.1));
            }
        });
    }

    #[test]
    fn cursor_move_down_with_selection() {
        run_cursor_test(|editor, view_model| {
            for expected in [(2, 1), (3, 1), (4, 1), (5, 1), (5, 2)] {
                move_down(editor, view_model, true);
                assert_cursor(view_model, Selection::new(1, 1, expected.0, expected.1));
            }
        });
    }

    #[test]
    fn cursor_move_down_with_tabs() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 5, false);
            assert_cursor(view_model, Position::new(1, 5));
            for expected in [(2, 2), (3, 5), (4, 1), (5, 2)] {
                move_down(editor, view_model, false);
                assert_cursor(view_model, Position::new(expected.0, expected.1));
            }
        });
    }

    // --------- move up

    #[test]
    fn cursor_move_up() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 5, false);
            assert_cursor(view_model, Position::new(3, 5));

            move_up(editor, view_model, false);
            assert_cursor(view_model, Position::new(2, 2));

            move_up(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 5));
        });
    }

    #[test]
    fn cursor_move_up_with_selection() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 5, false);
            assert_cursor(view_model, Position::new(3, 5));

            move_up(editor, view_model, true);
            assert_cursor(view_model, Selection::new(3, 5, 2, 2));

            move_up(editor, view_model, true);
            assert_cursor(view_model, Selection::new(3, 5, 1, 5));
        });
    }

    #[test]
    fn cursor_move_up_and_down_with_tabs() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 5, false);
            assert_cursor(view_model, Position::new(1, 5));
            for _ in 0..4 {
                move_down(editor, view_model, false);
            }
            assert_cursor(view_model, Position::new(5, 2));
            for expected in [(4, 1), (3, 5), (2, 2), (1, 5)] {
                move_up(editor, view_model, false);
                assert_cursor(view_model, Position::new(expected.0, expected.1));
            }
        });
    }

    #[test]
    fn cursor_move_up_and_down_with_end_of_lines_starting_from_a_long_one() {
        run_cursor_test(|editor, view_model| {
            move_to_end_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, utf16_len(LINE1) + 1));
            move_to_end_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, utf16_len(LINE1) + 1));
            for (line_number, line) in [(2, LINE2), (3, LINE3), (4, LINE4), (5, LINE5)] {
                move_down(editor, view_model, false);
                assert_cursor(view_model, Position::new(line_number, utf16_len(line) + 1));
            }
            for _ in 0..4 {
                move_up(editor, view_model, false);
            }
            assert_cursor(view_model, Position::new(1, utf16_len(LINE1) + 1));
        });
    }

    #[test]
    fn cursor_issue_44465_cursor_position_not_correct_when_move() {
        run_cursor_test(|editor, view_model| {
            set_selections_test(view_model, &[Selection::new(1, 5, 1, 5)]);
            // going once up on the first line remembers the offset visual columns
            move_up(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 1));
            move_down(editor, view_model, false);
            assert_cursor(view_model, Position::new(2, 2));
            move_up(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 5));

            // going twice up on the first line discards the offset visual columns
            move_up(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 1));
            move_up(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 1));
            move_down(editor, view_model, false);
            assert_cursor(view_model, Position::new(2, 1));
        });
    }

    #[test]
    fn cursor_issue_144041_cursor_up_down_works() {
        check_cursor_up_down_positions(
            false,
            &["(1,1)", "(2,5)", "(3,1)", "(4,5)", "(4,10)", "(3,1)", "(2,5)", "(1,1)", "(1,1)"],
        );
    }

    #[test]
    fn cursor_issue_140195_cursor_up_down_makes_progress() {
        check_cursor_up_down_positions(
            true,
            &["(1,1)", "(2,5)", "(5,19)", "(6,1)", "(7,5)", "(6,1)", "(2,8)", "(1,1)", "(1,1)"],
        );
    }

    // --------- move to beginning of line

    #[test]
    fn cursor_move_to_beginning_of_line() {
        run_cursor_test(|editor, view_model| {
            move_to_beginning_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 6));
            move_to_beginning_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 1));
        });
    }

    #[test]
    fn cursor_move_to_beginning_of_line_from_within_line() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 8, false);
            move_to_beginning_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 6));
            move_to_beginning_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 1));
        });
    }

    #[test]
    fn cursor_move_to_beginning_of_line_from_whitespace_at_beginning_of_line() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 2, false);
            move_to_beginning_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 6));
            move_to_beginning_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 1));
        });
    }

    #[test]
    fn cursor_move_to_beginning_of_line_from_within_line_selection() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 8, false);
            move_to_beginning_of_line(editor, view_model, true);
            assert_cursor(view_model, Selection::new(1, 8, 1, 6));
            move_to_beginning_of_line(editor, view_model, true);
            assert_cursor(view_model, Selection::new(1, 8, 1, 1));
        });
    }

    #[test]
    fn cursor_move_to_beginning_of_line_with_selection_multiline_forward() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 8, false);
            move_to(editor, view_model, 3, 9, true);
            move_to_beginning_of_line(editor, view_model, false);
            assert_cursor(view_model, Selection::new(3, 5, 3, 5));
        });
    }

    #[test]
    fn cursor_move_to_beginning_of_line_with_selection_multiline_backward() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 9, false);
            move_to(editor, view_model, 1, 8, true);
            move_to_beginning_of_line(editor, view_model, false);
            assert_cursor(view_model, Selection::new(1, 6, 1, 6));
        });
    }

    #[test]
    fn cursor_move_to_beginning_of_line_with_selection_single_line_forward() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 2, false);
            move_to(editor, view_model, 3, 9, true);
            move_to_beginning_of_line(editor, view_model, false);
            assert_cursor(view_model, Selection::new(3, 5, 3, 5));
        });
    }

    #[test]
    fn cursor_move_to_beginning_of_line_with_selection_single_line_backward() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 9, false);
            move_to(editor, view_model, 3, 2, true);
            move_to_beginning_of_line(editor, view_model, false);
            assert_cursor(view_model, Selection::new(3, 5, 3, 5));
        });
    }

    #[test]
    fn cursor_issue_15401_end_key_is_behaving_weird_when_text_is_selected_part_1() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 8, false);
            move_to(editor, view_model, 3, 9, true);
            move_to_beginning_of_line(editor, view_model, false);
            assert_cursor(view_model, Selection::new(3, 5, 3, 5));
        });
    }

    #[test]
    fn cursor_issue_17011_shift_home_end_now_go_to_the_end_of_the_selection_starts_line_not_the_selections_end() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 8, false);
            move_to(editor, view_model, 3, 9, true);
            move_to_beginning_of_line(editor, view_model, true);
            assert_cursor(view_model, Selection::new(1, 8, 3, 5));
        });
    }

    // --------- move to end of line

    #[test]
    fn cursor_move_to_end_of_line() {
        run_cursor_test(|editor, view_model| {
            move_to_end_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, utf16_len(LINE1) + 1));
            move_to_end_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, utf16_len(LINE1) + 1));
        });
    }

    #[test]
    fn cursor_move_to_end_of_line_from_within_line() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 6, false);
            move_to_end_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, utf16_len(LINE1) + 1));
            move_to_end_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, utf16_len(LINE1) + 1));
        });
    }

    #[test]
    fn cursor_move_to_end_of_line_from_whitespace_at_end_of_line() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 20, false);
            move_to_end_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, utf16_len(LINE1) + 1));
            move_to_end_of_line(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, utf16_len(LINE1) + 1));
        });
    }

    #[test]
    fn cursor_move_to_end_of_line_from_within_line_selection() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 6, false);
            move_to_end_of_line(editor, view_model, true);
            assert_cursor(view_model, Selection::new(1, 6, 1, utf16_len(LINE1) + 1));
            move_to_end_of_line(editor, view_model, true);
            assert_cursor(view_model, Selection::new(1, 6, 1, utf16_len(LINE1) + 1));
        });
    }

    #[test]
    fn cursor_move_to_end_of_line_with_selection_multiline_forward() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 1, false);
            move_to(editor, view_model, 3, 9, true);
            move_to_end_of_line(editor, view_model, false);
            assert_cursor(view_model, Selection::new(3, 17, 3, 17));
        });
    }

    #[test]
    fn cursor_move_to_end_of_line_with_selection_multiline_backward() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 9, false);
            move_to(editor, view_model, 1, 1, true);
            move_to_end_of_line(editor, view_model, false);
            assert_cursor(view_model, Selection::new(1, 21, 1, 21));
        });
    }

    #[test]
    fn cursor_move_to_end_of_line_with_selection_single_line_forward() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 1, false);
            move_to(editor, view_model, 3, 9, true);
            move_to_end_of_line(editor, view_model, false);
            assert_cursor(view_model, Selection::new(3, 17, 3, 17));
        });
    }

    #[test]
    fn cursor_move_to_end_of_line_with_selection_single_line_backward() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 9, false);
            move_to(editor, view_model, 3, 1, true);
            move_to_end_of_line(editor, view_model, false);
            assert_cursor(view_model, Selection::new(3, 17, 3, 17));
        });
    }

    #[test]
    fn cursor_issue_15401_end_key_is_behaving_weird_when_text_is_selected_part_2() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 1, false);
            move_to(editor, view_model, 3, 9, true);
            move_to_end_of_line(editor, view_model, false);
            assert_cursor(view_model, Selection::new(3, 17, 3, 17));
        });
    }

    // --------- move to beginning of buffer

    #[test]
    fn cursor_move_to_beginning_of_buffer() {
        run_cursor_test(|editor, view_model| {
            move_to_beginning_of_buffer(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 1));
        });
    }

    #[test]
    fn cursor_move_to_beginning_of_buffer_from_within_first_line() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 3, false);
            move_to_beginning_of_buffer(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 1));
        });
    }

    #[test]
    fn cursor_move_to_beginning_of_buffer_from_within_another_line() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 3, false);
            move_to_beginning_of_buffer(editor, view_model, false);
            assert_cursor(view_model, Position::new(1, 1));
        });
    }

    #[test]
    fn cursor_move_to_beginning_of_buffer_from_within_first_line_selection() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 1, 3, false);
            move_to_beginning_of_buffer(editor, view_model, true);
            assert_cursor(view_model, Selection::new(1, 3, 1, 1));
        });
    }

    #[test]
    fn cursor_move_to_beginning_of_buffer_from_within_another_line_selection() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 3, false);
            move_to_beginning_of_buffer(editor, view_model, true);
            assert_cursor(view_model, Selection::new(3, 3, 1, 1));
        });
    }

    // --------- move to end of buffer

    #[test]
    fn cursor_move_to_end_of_buffer() {
        run_cursor_test(|editor, view_model| {
            move_to_end_of_buffer(editor, view_model, false);
            assert_cursor(view_model, Position::new(5, utf16_len(LINE5) + 1));
        });
    }

    #[test]
    fn cursor_move_to_end_of_buffer_from_within_last_line() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 5, 1, false);
            move_to_end_of_buffer(editor, view_model, false);
            assert_cursor(view_model, Position::new(5, utf16_len(LINE5) + 1));
        });
    }

    #[test]
    fn cursor_move_to_end_of_buffer_from_within_another_line() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 3, false);
            move_to_end_of_buffer(editor, view_model, false);
            assert_cursor(view_model, Position::new(5, utf16_len(LINE5) + 1));
        });
    }

    #[test]
    fn cursor_move_to_end_of_buffer_from_within_last_line_selection() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 5, 1, false);
            move_to_end_of_buffer(editor, view_model, true);
            assert_cursor(view_model, Selection::new(5, 1, 5, utf16_len(LINE5) + 1));
        });
    }

    #[test]
    fn cursor_move_to_end_of_buffer_from_within_another_line_selection() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 3, 3, false);
            move_to_end_of_buffer(editor, view_model, true);
            assert_cursor(view_model, Selection::new(3, 3, 5, utf16_len(LINE5) + 1));
        });
    }

    // --------- misc

    #[test]
    fn cursor_select_all() {
        run_cursor_test(|_editor, view_model| {
            core_navigation_commands::instance().select_all.run_core_editor_command(view_model, &empty_args());
            assert_cursor(view_model, Selection::new(1, 1, 5, utf16_len(LINE5) + 1));
        });
    }

    // --------- eventing

    #[test]
    fn cursor_no_move_doesnt_trigger_event() {
        run_cursor_test(|editor, view_model| {
            let disposable = view_model.on_event.listen(|_e| {
                panic!("was not expecting event");
            });
            move_to(editor, view_model, 1, 1, false);
            disposable.dispose();
        });
    }

    /// O corpo comum de `move eventing` e `move in selection mode eventing`: um `CursorStateChanged` com as seleções
    /// esperadas.
    fn check_move_eventing(in_selection_mode: bool, expected: Selection) {
        run_cursor_test(|editor, view_model| {
            let events = Rc::new(Cell::new(0));
            let listener_events = events.clone();
            let disposable = view_model.on_event.listen(move |e| {
                if let OutgoingViewModelEvent::CursorStateChanged(e) = e {
                    listener_events.set(listener_events.get() + 1);
                    let actual: Vec<String> = e.selections.iter().map(ToString::to_string).collect();
                    assert_eq!(actual, vec![expected.to_string()]);
                }
            });
            move_to(editor, view_model, 1, 2, in_selection_mode);
            assert_eq!(events.get(), 1, "receives 1 event");
            disposable.dispose();
        });
    }

    #[test]
    fn cursor_move_eventing() {
        check_move_eventing(false, Selection::new(1, 2, 1, 2));
    }

    #[test]
    fn cursor_move_in_selection_mode_eventing() {
        check_move_eventing(true, Selection::new(1, 1, 1, 2));
    }

    // --------- state save & restore

    #[test]
    fn cursor_save_state_and_restore_state() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 2, 1, true);
            assert_cursor(view_model, Selection::new(1, 1, 2, 1));

            let saved_state = view_model.save_cursor_state();

            move_to(editor, view_model, 1, 1, false);
            assert_cursor(view_model, Position::new(1, 1));

            view_model.restore_cursor_state(&saved_state);
            assert_cursor(view_model, Selection::new(1, 1, 2, 1));
        });
    }

    // --------- updating cursor

    #[test]
    fn cursor_independent_model_edit_1() {
        run_cursor_test(|editor, view_model| {
            move_to(editor, view_model, 2, 16, true);

            let edits: Vec<SingleEditOperation> = vec![EditOperation::delete(Range::new(2, 1, 2, 2))];
            editor.get_model().expect("o editor tem o modelo").apply_edits(&edits);
            assert_cursor(view_model, Selection::new(1, 1, 2, 15));
        });
    }

    #[test]
    fn cursor_column_select_1() {
        with_editor(
            &[
                "\tprivate compute(a:number): boolean {",
                "\t\tif (a + 3 === 0 || a + 5 === 0) {",
                "\t\t\treturn false;",
                "\t\t}",
                "\t}",
            ][..],
            |editor, view_model| {
                move_to(editor, view_model, 1, 7, false);
                assert_cursor(view_model, Position::new(1, 7));

                column_select(view_model, Position::new(4, 4), 15);

                let expected_selections = vec![
                    Selection::new(1, 7, 1, 12),
                    Selection::new(2, 4, 2, 9),
                    Selection::new(3, 3, 3, 6),
                    Selection::new(4, 4, 4, 4),
                ];

                assert_cursor(view_model, expected_selections);
            },
        );
    }

    #[test]
    fn cursor_grapheme_breaking() {
        let combining_a = "a\u{303}".repeat(6);
        let variation_sequence = "\u{8FFB}\u{E0100}".repeat(3);
        with_editor(
            &["abcabc", combining_a.as_str(), variation_sequence.as_str(), "\u{0BAA}\u{0BC1}"][..],
            |editor, view_model| {
                set_selections_test(view_model, &[Selection::new(2, 1, 2, 1)]);
                move_right(editor, view_model, false);
                assert_cursor(view_model, Position::new(2, 3));
                move_left(editor, view_model, false);
                assert_cursor(view_model, Position::new(2, 1));

                set_selections_test(view_model, &[Selection::new(3, 1, 3, 1)]);
                move_right(editor, view_model, false);
                assert_cursor(view_model, Position::new(3, 4));
                move_left(editor, view_model, false);
                assert_cursor(view_model, Position::new(3, 1));

                set_selections_test(view_model, &[Selection::new(4, 1, 4, 1)]);
                move_right(editor, view_model, false);
                assert_cursor(view_model, Position::new(4, 3));
                move_left(editor, view_model, false);
                assert_cursor(view_model, Position::new(4, 1));

                set_selections_test(view_model, &[Selection::new(1, 3, 1, 3)]);
                move_down(editor, view_model, false);
                assert_cursor(view_model, Position::new(2, 5));
                move_down(editor, view_model, false);
                assert_cursor(view_model, Position::new(3, 4));
                move_up(editor, view_model, false);
                assert_cursor(view_model, Position::new(2, 5));
                move_up(editor, view_model, false);
                assert_cursor(view_model, Position::new(1, 3));
            },
        );
    }

    #[test]
    fn cursor_issue_4905_column_select_is_biased_to_the_right() {
        with_editor(REQUIRE_LINES, |editor, view_model| {
            move_to(editor, view_model, 1, 4, false);
            assert_cursor(view_model, Position::new(1, 4));

            column_select(view_model, Position::new(4, 1), 1);

            assert_cursor(view_model, column_selections(4, &[1, 1, 1, 1]));
        });
    }

    /// As dez seleções, da linha 10 à 1, de `issue #20087: column select with mouse`.
    fn property_column_selections() -> Vec<Selection> {
        (1..=10).rev().map(|line_number| Selection::new(line_number, 10, line_number, 1)).collect()
    }

    #[test]
    fn cursor_issue_20087_column_select_with_mouse() {
        with_editor(PROPERTY_LINES, |editor, view_model| {
            move_to(editor, view_model, 10, 10, false);
            assert_cursor(view_model, Position::new(10, 10));

            column_select(view_model, Position::new(1, 1), 1);
            assert_cursor(view_model, property_column_selections());

            column_select(view_model, Position::new(1, 1), 1);
            assert_cursor(view_model, property_column_selections());
        });
    }

    #[test]
    fn cursor_issue_20087_column_select_with_keyboard() {
        with_editor(PROPERTY_LINES, |editor, view_model| {
            move_to(editor, view_model, 10, 10, false);
            assert_cursor(view_model, Position::new(10, 10));

            repeat_column_select(view_model, 1, |c| &c.cursor_column_select_left);
            assert_cursor(view_model, vec![Selection::new(10, 10, 10, 9)]);

            repeat_column_select(view_model, 1, |c| &c.cursor_column_select_left);
            assert_cursor(view_model, vec![Selection::new(10, 10, 10, 8)]);

            repeat_column_select(view_model, 1, |c| &c.cursor_column_select_right);
            assert_cursor(view_model, vec![Selection::new(10, 10, 10, 9)]);

            repeat_column_select(view_model, 1, |c| &c.cursor_column_select_up);
            assert_cursor(view_model, vec![Selection::new(10, 10, 10, 9), Selection::new(9, 10, 9, 9)]);

            repeat_column_select(view_model, 1, |c| &c.cursor_column_select_down);
            assert_cursor(view_model, vec![Selection::new(10, 10, 10, 9)]);
        });
    }

    #[test]
    fn cursor_issue_118062_column_selection_cannot_select_first_position_of_a_line() {
        with_editor("hello world", |editor, view_model| {
            move_to(editor, view_model, 1, 2, false);
            assert_cursor(view_model, Position::new(1, 2));

            repeat_column_select(view_model, 1, |c| &c.cursor_column_select_left);
            assert_cursor(view_model, vec![Selection::new(1, 2, 1, 1)]);
        });
    }

    #[test]
    fn cursor_column_select_with_keyboard() {
        with_editor(REQUIRE_LINES, |editor, view_model| {
            move_to(editor, view_model, 1, 4, false);
            assert_cursor(view_model, Position::new(1, 4));

            repeat_column_select(view_model, 1, |c| &c.cursor_column_select_right);
            assert_cursor(view_model, column_selections(4, &[5]));

            repeat_column_select(view_model, 1, |c| &c.cursor_column_select_down);
            assert_cursor(view_model, column_selections(4, &[5, 5]));

            repeat_column_select(view_model, 1, |c| &c.cursor_column_select_down);
            assert_cursor(view_model, column_selections(4, &[5, 5, 5]));

            repeat_column_select(view_model, 4, |c| &c.cursor_column_select_down);
            assert_cursor(view_model, column_selections(4, &[5, 5, 5, 5, 5, 5, 5]));

            repeat_column_select(view_model, 1, |c| &c.cursor_column_select_right);
            assert_cursor(view_model, column_selections(4, &[6, 6, 6, 6, 6, 6, 6]));

            // 10 times
            repeat_column_select(view_model, 10, |c| &c.cursor_column_select_right);
            assert_cursor(view_model, column_selections(4, &[16, 16, 16, 16, 16, 16, 16]));

            // 10 times
            repeat_column_select(view_model, 10, |c| &c.cursor_column_select_right);
            assert_cursor(view_model, column_selections(4, &[26, 26, 26, 26, 26, 26, 26]));

            // 2 times => reaching the ending of lines 1 and 2
            repeat_column_select(view_model, 2, |c| &c.cursor_column_select_right);
            assert_cursor(view_model, column_selections(4, &[28, 28, 28, 28, 28, 28, 28]));

            // 4 times => reaching the ending of line 3
            repeat_column_select(view_model, 4, |c| &c.cursor_column_select_right);
            assert_cursor(view_model, column_selections(4, &[28, 28, 32, 32, 32, 32, 32]));

            // 2 times => reaching the ending of line 4
            repeat_column_select(view_model, 2, |c| &c.cursor_column_select_right);
            assert_cursor(view_model, column_selections(4, &[28, 28, 32, 34, 34, 34, 34]));

            // 1 time => reaching the ending of line 7
            repeat_column_select(view_model, 1, |c| &c.cursor_column_select_right);
            assert_cursor(view_model, column_selections(4, &[28, 28, 32, 34, 35, 35, 35]));

            // 3 times => reaching the ending of lines 5 & 6
            repeat_column_select(view_model, 3, |c| &c.cursor_column_select_right);
            assert_cursor(view_model, column_selections(4, &[28, 28, 32, 34, 37, 37, 35]));

            // cannot go anywhere anymore
            repeat_column_select(view_model, 1, |c| &c.cursor_column_select_right);
            assert_cursor(view_model, column_selections(4, &[28, 28, 32, 34, 37, 37, 35]));

            // cannot go anywhere anymore even if we insist
            repeat_column_select(view_model, 4, |c| &c.cursor_column_select_right);
            assert_cursor(view_model, column_selections(4, &[28, 28, 32, 34, 37, 37, 35]));

            // can easily go back
            repeat_column_select(view_model, 1, |c| &c.cursor_column_select_left);
            assert_cursor(view_model, column_selections(4, &[28, 28, 32, 34, 36, 36, 35]));
        });
    }

    #[test]
    fn cursor_set_selection_set_position_with_source() {
        const LANGUAGE_ID: &str = "modelModeTest1";
        core_navigation_commands::instance();
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let language_registration = tokenization_registry().register(LANGUAGE_ID, Rc::new(CursorSuiteEmptyTokenizationSupport));
        let model: Rc<TextModel> = create_text_model_with("Just text", Some(LANGUAGE_ID), Default::default(), None);

        with_test_code_editor(model.clone(), TestCodeEditorInstantiationOptions::default(), |editor1, _cursor1, _| {
            let event: Rc<RefCell<Option<ICursorPositionChangedEvent>>> = Rc::default();
            let listener_event = event.clone();
            let disposable = editor1.on_did_change_cursor_position().listen(move |e| {
                *listener_event.borrow_mut() = Some(e.clone());
            });

            editor1.set_selection(SelectionOrRange::from(Range::new(1, 2, 1, 3)), Some("navigation"));
            assert_eq!(event.borrow().as_ref().expect("o evento de posição").source, "navigation");

            *event.borrow_mut() = None;
            editor1.set_position(Position::new(1, 2), Some("navigation"));
            assert_eq!(event.borrow().as_ref().expect("o evento de posição").source, "navigation");
            disposable.dispose();
        });

        language_registration.dispose();
        model.dispose();
    }
}

#[allow(unused_imports)]
mod part_b {
    use super::*;
use je_editor::view_model_types::IViewModel as _;

    // imports do fragmento B:
    use std::cell::Cell;
    use std::rc::Rc;

    use je_base::JsString;
    use je_base::dynamic::Dynamic;
    use je_base::lifecycle::IDisposable as _;
    use je_editor::code_editor_widget::{ExecuteEditsSource, ICodeEditor};
    use je_editor::core_commands::core_editing_commands;
    use je_editor::core_commands::core_navigation_commands;
    use je_editor::cursor_common::CursorSimpleModel;
    use je_editor::edit_operation::{EditOperation, SingleEditOperation};
    use je_editor::language_configuration::{AutoClosingPair, IndentAction, LanguageConfiguration};
    use je_editor::languages::{EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult};
    use je_editor::model::{EndOfLineSequence, RelaxedTextModelCreationOptions, ValidEditOperation};
    use je_editor::null_tokenize::null_state;
    use je_editor::position::Position;
    use je_editor::range::Range;
    use je_editor::selection::Selection;
    use je_editor::tokenization_registry::tokenization_registry;
    use support::test_view_model::no_options;
    use support::u;

    use editor_controller::*;

    /// A suíte `Editor Controller`: o `setup`, o `teardown` e os auxiliares internos da suíte (`setupOnEnterLanguage`,
    /// `setupIndentRulesLanguage`, `setupAutoClosingLanguage`, `createTextModel`, `withTestCodeEditor`, `usingCursor`,
    /// `assertType`...), compartilhados com os outros fragmentos.
    ///
    /// Notas de tradução:
    /// - O `let disposables/instantiationService/languageConfigurationService/languageService` da suíte são os campos de
    ///   [`EditorControllerFixture`]; o `setup` é [`EditorControllerFixture::new`] e o `teardown` é
    ///   [`EditorControllerFixture::teardown`]. [`run_test`] é o que o mocha faz em volta de cada `test(...)`: o carregamento
    ///   do módulo `coreCommands`, o `ensureNoDisposablesAreLeakedInTestSuite()` (o `LeakGuard`), o `setup`, o corpo e o
    ///   `teardown`, nessa ordem.
    /// - Os auxiliares que leem os `let` da suíte são métodos da fixture. Os que não leem (`extractAutoClosingSpecialColumns`,
    ///   `assertType`) são funções livres do módulo.
    /// - Os parâmetros com valor padrão do original (`languageId = null`, `options = DEFAULT_CREATION_OPTIONS`, `uri = null`)
    ///   são `Option`: `None` é o padrão. `{}` de opções do editor é `&no_options()`.
    /// - `editor.runCommand(CoreEditingCommands.X, null)` é `editor.run_command(&*commands.x, None)`: o porte dos comandos de
    ///   edição vira `ITestEditorCommand` aqui (as duas implementações abaixo, uma para os `CoreEditingCommand` e outra para
    ///   `Undo`/`Redo`), porque o `runCommand` do editor de teste chama `command.runEditorCommand(accessor, this, args)`.
    /// - `viewModel.type(text, source)` é [`type_text`] (o texto vira UTF-16) e `viewModel.setSelections('test', ...)` é
    ///   [`set_test_selections`] (a fonte `test` e o motivo `NotSet`, o padrão do original).
    pub mod editor_controller {
        use std::any::Any;
        use std::rc::Rc;

        use je_base::dynamic::Dynamic;
        use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
        use je_base::promise::MaybePromise;
        use je_base::uri::URI;
        use je_base::JsString;
        use je_editor::code_editor_widget::{CodeEditorWidget, EditorViewModel, IEditorCommand};
        use je_editor::core_commands::core_editing_commands::{CoreEditingCommand, UndoRedoCommand};
        use je_editor::core_commands::{
            CoreEditorCommand, EditorOrNativeTextInputCommand, MoveCommandOptions, register_core_commands,
        };
        use je_editor::cursor_common::CursorSimpleModel;
        use je_editor::view_model_types::IViewModel as _;
        use je_editor::cursor_events::CursorChangeReason;
        use je_editor::encoded_token_attributes::{metadata_consts, standard_token_type};
        use je_editor::language_configuration::{
            AutoClosingPair, AutoClosingPairConditional, CommentRule, DocComment, ElectricCharacterSupportConfig, EnterAction,
            IndentAction, IndentationRule, LanguageConfiguration, OnEnterRule,
        };
        use je_editor::language_configuration_registry::ILanguageConfigurationService;
        use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE};
        use je_editor::languages::{EncodedTokenizationResult, IState, ITokenizationSupport, State, TokenizationResult};
        use je_editor::model::{EndOfLinePreference, RelaxedTextModelCreationOptions};
        use je_editor::modes_registry::LanguageExtensionPoint;
        use je_editor::position::Position;
        use je_editor::selection::Selection;
        use je_editor::text_model::{TextModel, TextModelServices, TextSource};
        use je_editor::tokenization_registry::tokenization_registry;
        use je_instantiation::ServicesAccessor;

        use super::move_to;
        use crate::common::ensure_no_disposables_are_leaked_in_test_suite;
        use crate::support::re;
        use crate::support::test_code_editor::{
            CodeEditorServices, ITestEditorCommand, TestCodeEditor, TestCodeEditorCreationOptions, TestEditorInput,
            create_code_editor_services, instantiate_test_code_editor,
        };
        use crate::support::test_language_configuration_service::TestLanguageConfigurationService;
        use crate::support::test_text_model::{PLAINTEXT_LANGUAGE_ID, resolve_options};
        use crate::support::u;

        pub const SURROUNDING_LANGUAGE_ID: &str = "surroundingLanguage";
        pub const INDENT_RULES_LANGUAGE_ID: &str = "indentRulesLanguage";
        pub const ELECTRIC_CHAR_LANGUAGE_ID: &str = "electricCharLanguage";
        pub const AUTO_CLOSING_LANGUAGE_ID: &str = "autoClosingLanguage";
        pub const EMPTY_CLOSING_SURROUND_LANGUAGE_ID: &str = "emptyClosingSurroundLanguage";

        /// O `[]` de linhas do `withTestCodeEditor` local (`string[]`): um vetor de tamanho fixo de linhas.
        impl<const N: usize> From<[String; N]> for TestEditorInput {
            fn from(lines: [String; N]) -> Self {
                TestEditorInput::Lines(lines.into_iter().collect())
            }
        }

        impl<const N: usize> From<[&str; N]> for TestEditorInput {
            fn from(lines: [&str; N]) -> Self {
                TestEditorInput::Lines(lines.iter().map(|line| (*line).to_owned()).collect())
            }
        }

        /// `CoreEditingCommands.X` como argumento de `editor.runCommand(command, args)`: o `runEditorCommand` do
        /// `EditorCommand`, com o `args` ausente (`null`) valendo `{}` dentro do comando.
        impl ITestEditorCommand for CoreEditingCommand {
            fn run_editor_command(
                &self,
                accessor: &ServicesAccessor,
                editor: &Rc<CodeEditorWidget>,
                args: Option<&Dynamic>,
            ) -> MaybePromise<()> {
                let args = args.cloned().unwrap_or(Dynamic::Null);
                MaybePromise::Promise(IEditorCommand::run_editor_command(self, Some(accessor), editor, &args))
            }
        }

        /// `CoreEditingCommands.Undo` e `CoreEditingCommands.Redo` como argumento de `editor.runCommand(command, args)`: o
        /// `runEditorCommand` do `EditorOrNativeTextInputCommand`.
        impl ITestEditorCommand for UndoRedoCommand {
            fn run_editor_command(
                &self,
                accessor: &ServicesAccessor,
                editor: &Rc<CodeEditorWidget>,
                args: Option<&Dynamic>,
            ) -> MaybePromise<()> {
                let args = args.cloned().unwrap_or(Dynamic::Null);
                EditorOrNativeTextInputCommand::run_editor_command(self, Some(accessor), editor, &args)
            }
        }

        /// `ICursorOpts`.
        #[derive(Default)]
        pub struct CursorOpts<'a> {
            pub text: Vec<&'a str>,
            pub language_id: Option<&'a str>,
            pub model_opts: Option<RelaxedTextModelCreationOptions>,
            pub editor_opts: Option<Dynamic>,
        }

        /// `AutoClosingColumnType`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum AutoClosingColumnType {
            Normal = 0,
            Special1 = 1,
            Special2 = 2,
        }

        /// Os `let` da suíte `Editor Controller`.
        pub struct EditorControllerFixture {
            pub disposables: Rc<DisposableStore>,
            pub instantiation_service: Rc<CodeEditorServices>,
            pub language_configuration_service: Rc<TestLanguageConfigurationService>,
            pub language_service: Rc<dyn ILanguageService>,
        }

        /// As regras de indentação da `indentRulesLanguage`.
        fn indent_rules_language_indentation_rules() -> IndentationRule {
            IndentationRule {
                decrease_indent_pattern: re(
                    r##"^\s*((?!\S.*\/[*]).*[*]\/\s*)?[})\]]|^\s*(case\b.*|default):\s*(\/\/.*|\/[*].*[*]\/\s*)?$"##,
                    "",
                ),
                increase_indent_pattern: re(
                    r##"^((?!\/\/).)*(\{[^}"'`]*|\([^)"']*|\[[^\]"']*|^\s*(\{\}|\(\)|\[\]|(case\b.*|default):))\s*(\/\/.*|\/[*].*[*]\/\s*)?$"##,
                    "",
                ),
                indent_next_line_pattern: Some(re(
                    r##"^\s*(for|while|if|else)\b(?!.*[;{}]\s*(\/\/.*|\/[*].*[*]\/\s*)?$)"##,
                    "",
                )),
                un_indented_line_pattern: Some(re(
                    r##"^(?!.*([;{}]|\S:)\s*(\/\/.*|\/[*].*[*]\/\s*)?$)(?!.*(\{[^}"']*|\([^)"']*|\[[^\]"']*|^\s*(\{\}|\(\)|\[\]|(case\b.*|default):))\s*(\/\/.*|\/[*].*[*]\/\s*)?$)(?!^\s*((?!\S.*\/[*]).*[*]\/\s*)?[})\]]|^\s*(case\b.*|default):\s*(\/\/.*|\/[*].*[*]\/\s*)?$)(?!^\s*(for|while|if|else)\b(?!.*[;{}]\s*(\/\/.*|\/[*].*[*]\/\s*)?$))"##,
                    "",
                )),
            }
        }

        /// O `__electricCharacterSupport: { docComment: { open: '/**', close: ' */' } }` das linguagens `electricCharLanguage` e
        /// `autoClosingLanguage`.
        fn doc_comment_electric_character_support() -> Option<ElectricCharacterSupportConfig> {
            Some(ElectricCharacterSupportConfig {
                doc_comment: Some(DocComment { open: "/**".to_owned(), close: Some(" */".to_owned()) }),
            })
        }

        /// Os pares de fechamento automático que `setupAutoClosingLanguage` e `setAutoClosingLanguageEnabledSet` têm em comum
        /// (tudo menos o par `begin`/`end`).
        fn common_auto_closing_pairs() -> Vec<AutoClosingPairConditional> {
            vec![
                AutoClosingPairConditional::new("{", "}"),
                AutoClosingPairConditional::new("[", "]"),
                AutoClosingPairConditional::new("(", ")"),
                AutoClosingPairConditional::new("'", "'").not_in(&["string", "comment"]),
                AutoClosingPairConditional::new("\"", "\"").not_in(&["string"]),
                AutoClosingPairConditional::new("`", "`").not_in(&["string", "comment"]),
                AutoClosingPairConditional::new("/**", " */").not_in(&["string"]),
            ]
        }

        impl EditorControllerFixture {
            /// O `setup` da suíte.
            pub fn new() -> Self {
                let disposables = DisposableStore::new();
                let instantiation_service = create_code_editor_services(&disposables, None);
                let language_configuration_service = instantiation_service.language_configuration_service.clone();
                let language_service = instantiation_service.get(I_LANGUAGE_SERVICE);
                let fixture = Self { disposables, instantiation_service, language_configuration_service, language_service };

                fixture.register_language(
                    SURROUNDING_LANGUAGE_ID,
                    LanguageConfiguration {
                        auto_closing_pairs: Some(vec![AutoClosingPairConditional::new("(", ")")]),
                        ..LanguageConfiguration::default()
                    },
                );

                fixture.register_language(
                    EMPTY_CLOSING_SURROUND_LANGUAGE_ID,
                    LanguageConfiguration {
                        surrounding_pairs: Some(vec![AutoClosingPair::new("<", "")]),
                        ..LanguageConfiguration::default()
                    },
                );

                fixture.setup_indent_rules_language(INDENT_RULES_LANGUAGE_ID, indent_rules_language_indentation_rules());

                fixture.register_language(
                    ELECTRIC_CHAR_LANGUAGE_ID,
                    LanguageConfiguration {
                        electric_character_support: doc_comment_electric_character_support(),
                        brackets: Some(vec![
                            ("{".to_owned(), "}".to_owned()),
                            ("[".to_owned(), "]".to_owned()),
                            ("(".to_owned(), ")".to_owned()),
                        ]),
                        ..LanguageConfiguration::default()
                    },
                );

                fixture.setup_auto_closing_language();
                fixture
            }

            /// O `teardown` da suíte.
            pub fn teardown(&self) {
                self.disposables.dispose();
            }

            /// O par `disposables.add(languageService.registerLanguage({ id }))` e
            /// `disposables.add(languageConfigurationService.register(id, configuration))` que o `setup` e os auxiliares repetem.
            pub fn register_language(&self, language_id: &str, configuration: LanguageConfiguration) {
                self.disposables.add(self.language_service.register_language(LanguageExtensionPoint::new(language_id)));
                self.disposables.add(self.language_configuration_service.register(language_id, configuration, None));
            }

            /// `setupOnEnterLanguage(indentAction)`: devolve o id da linguagem.
            pub fn setup_on_enter_language(&self, indent_action: IndentAction) -> String {
                let on_enter_language_id = "onEnterMode";

                self.register_language(
                    on_enter_language_id,
                    LanguageConfiguration {
                        on_enter_rules: Some(vec![OnEnterRule {
                            before_text: re(".*", ""),
                            after_text: None,
                            previous_line_text: None,
                            action: EnterAction::new(indent_action),
                        }]),
                        ..LanguageConfiguration::default()
                    },
                );
                on_enter_language_id.to_owned()
            }

            /// `setupIndentRulesLanguage(languageId, indentationRules)`: devolve o id da linguagem.
            pub fn setup_indent_rules_language(&self, language_id: &str, indentation_rules: IndentationRule) -> String {
                self.register_language(
                    language_id,
                    LanguageConfiguration { indentation_rules: Some(indentation_rules), ..LanguageConfiguration::default() },
                );
                language_id.to_owned()
            }

            /// `setupAutoClosingLanguage()`.
            pub fn setup_auto_closing_language(&self) {
                let mut auto_closing_pairs = common_auto_closing_pairs();
                auto_closing_pairs.push(AutoClosingPairConditional::new("begin", "end").not_in(&["string"]));

                self.register_language(
                    AUTO_CLOSING_LANGUAGE_ID,
                    LanguageConfiguration {
                        comments: Some(CommentRule { line_comment: None, block_comment: Some(("/*".to_owned(), "*/".to_owned())) }),
                        auto_closing_pairs: Some(auto_closing_pairs),
                        electric_character_support: doc_comment_electric_character_support(),
                        ..LanguageConfiguration::default()
                    },
                );
            }

            /// `setupAutoClosingLanguageTokenization()`: registra o tokenizador de teste da `autoClosingLanguage`.
            pub fn setup_auto_closing_language_tokenization(&self) {
                let encoded_language_id = self.language_service.language_id_codec().encode_language_id(AUTO_CLOSING_LANGUAGE_ID);
                self.disposables.add(
                    tokenization_registry().register(AUTO_CLOSING_LANGUAGE_ID, Rc::new(AutoClosingTokenizationSupport { encoded_language_id })),
                );
            }

            /// `setAutoClosingLanguageEnabledSet(chars)`.
            pub fn set_auto_closing_language_enabled_set(&self, chars: &str) {
                self.disposables.add(self.language_configuration_service.register(
                    AUTO_CLOSING_LANGUAGE_ID,
                    LanguageConfiguration {
                        auto_close_before: Some(chars.to_owned()),
                        auto_closing_pairs: Some(common_auto_closing_pairs()),
                        ..LanguageConfiguration::default()
                    },
                    None,
                ));
            }

            /// `createTextModel(text, languageId = null, options = TextModel.DEFAULT_CREATION_OPTIONS, uri = null)`: o modelo usa
            /// os serviços de linguagem da suíte (o `instantiateTextModel(instantiationService, ...)` do original) e é
            /// descartado junto com a fixture.
            pub fn create_text_model(
                &self,
                text: impl Into<TextSource>,
                language_id: Option<&str>,
                options: RelaxedTextModelCreationOptions,
                uri: Option<URI>,
            ) -> Rc<TextModel> {
                let model = TextModel::new_with_language_service(
                    text.into(),
                    language_id.unwrap_or(PLAINTEXT_LANGUAGE_ID),
                    resolve_options(&options),
                    uri,
                    self.instantiation_service.undo_redo_service.clone(),
                    Some(TextModelServices {
                        language_service: self.language_service.clone(),
                        language_configuration_service: self.language_configuration_service.clone(),
                    }),
                );
                let owned = model.clone();
                self.disposables.add(to_disposable(move || owned.dispose()));
                model
            }

            /// `withTestCodeEditor(text, options, callback)` local da suíte: o editor (e o modelo, quando `text` não é um)
            /// é descartado junto com a fixture, não ao fim do callback.
            pub fn with_test_code_editor(
                &self,
                text: impl Into<TestEditorInput>,
                options: &TestCodeEditorCreationOptions,
                callback: impl FnOnce(&Rc<TestCodeEditor>, &Rc<EditorViewModel>),
            ) {
                let model = match text.into() {
                    TestEditorInput::Model(model) => model,
                    TestEditorInput::Text(text) => self.create_text_model(text, None, RelaxedTextModelCreationOptions::default(), None),
                    TestEditorInput::Lines(lines) => {
                        self.create_text_model(lines.join("\n"), None, RelaxedTextModelCreationOptions::default(), None)
                    }
                    TestEditorInput::Factory(factory) => {
                        self.create_text_model(factory, None, RelaxedTextModelCreationOptions::default(), None)
                    }
                };
                let editor = self.disposables.add_rc(instantiate_test_code_editor(&self.instantiation_service, Some(model), options));
                let view_model = editor.get_view_model().expect("o editor de teste tem o modelo ligado");
                view_model.set_has_focus(true);
                callback(&editor, &view_model);
            }

            /// `usingCursor(opts, callback)`.
            pub fn using_cursor(
                &self,
                opts: CursorOpts<'_>,
                callback: impl FnOnce(&Rc<TestCodeEditor>, &Rc<TextModel>, &Rc<EditorViewModel>),
            ) {
                let model = self.create_text_model(
                    opts.text.join("\n"),
                    opts.language_id,
                    opts.model_opts.unwrap_or_default(),
                    None,
                );
                let editor_options = opts.editor_opts.unwrap_or_else(super::no_options);
                self.with_test_code_editor(model.clone(), &editor_options, |editor, view_model| {
                    callback(editor, &model, view_model);
                });
            }
        }

        impl Default for EditorControllerFixture {
            fn default() -> Self {
                Self::new()
            }
        }

        /// O que o mocha faz em volta de cada `test(...)` da suíte `Editor Controller`: carrega o módulo `coreCommands`
        /// (que registra os comandos), instala o guarda de vazamentos, roda o `setup`, o corpo e o `teardown`.
        pub fn run_test(body: impl FnOnce(&EditorControllerFixture)) {
            register_core_commands();
            let _ds = ensure_no_disposables_are_leaked_in_test_suite();
            let fixture = EditorControllerFixture::new();
            body(&fixture);
            fixture.teardown();
        }

        /// `viewModel.type(text, source)`.
        pub fn type_text(view_model: &EditorViewModel, text: &str, source: Option<&str>) {
            view_model.r#type(&u(text), source);
        }

        /// `viewModel.setSelections('test', selections)`: o motivo omitido do original vale `NotSet`.
        pub fn set_test_selections(view_model: &EditorViewModel, selections: &[Selection]) {
            view_model.set_selections(Some("test"), selections, CursorChangeReason::NotSet);
        }

        /// `model.getValue()`.
        pub fn get_value(model: &TextModel) -> String {
            model.get_value().to_string_lossy()
        }

        /// `model.getValue(EndOfLinePreference.LF)`.
        pub fn get_value_lf(model: &TextModel) -> String {
            model.get_value_with(EndOfLinePreference::Lf, false).to_string_lossy()
        }

        /// `model.getLineContent(lineNumber)`.
        pub fn get_line_content(model: &TextModel, line_number: i32) -> String {
            CursorSimpleModel::get_line_content(model, line_number).to_string_lossy()
        }

        /// `CoreNavigationCommands.X.runCoreEditorCommand(viewModel, { position: new Position(lineNumber, column) })`, o
        /// argumento dos comandos `WordSelect` e `WordSelectDrag`.
        pub fn run_move_command(
            command: &CoreEditorCommand<MoveCommandOptions>,
            view_model: &EditorViewModel,
            line_number: i32,
            column: i32,
        ) {
            command.run_core_editor_command(
                view_model,
                &MoveCommandOptions { position: Some(Position::new(line_number, column)), ..MoveCommandOptions::default() },
            );
        }

        /// As opções `{ wordWrap: 'wordWrapColumn', wordWrapColumn: column }` do editor.
        pub fn word_wrap_options(column: i32) -> Dynamic {
            Dynamic::object([("wordWrap", Dynamic::from("wordWrapColumn")), ("wordWrapColumn", Dynamic::from(column))])
        }

        /// `extractAutoClosingSpecialColumns(maxColumn, annotatedLine)`: o índice do vetor é a coluna (a posição 0 não é
        /// usada); uma coluna além de `maxColumn` estende o vetor, com `Normal` nas posições intermediárias.
        pub fn extract_auto_closing_special_columns(max_column: i32, annotated_line: &str) -> Vec<AutoClosingColumnType> {
            let mut result = vec![AutoClosingColumnType::Normal; max_column.max(0) as usize + 1];
            let mut column = 1usize;
            for unit in annotated_line.encode_utf16() {
                let special = match unit {
                    unit if unit == u16::from(b'|') => Some(AutoClosingColumnType::Special1),
                    unit if unit == u16::from(b'!') => Some(AutoClosingColumnType::Special2),
                    _ => None,
                };
                match special {
                    Some(special) => {
                        if column >= result.len() {
                            result.resize(column + 1, AutoClosingColumnType::Normal);
                        }
                        result[column] = special;
                    }
                    None => column += 1,
                }
            }
            result
        }

        /// `assertType(editor, model, viewModel, lineNumber, column, chr, expectedInsert, message)`.
        #[allow(clippy::too_many_arguments)]
        pub fn assert_type(
            editor: &TestCodeEditor,
            model: &TextModel,
            view_model: &EditorViewModel,
            line_number: i32,
            column: i32,
            chr: &str,
            expected_insert: &str,
            message: &str,
        ) {
            let line_content = CursorSimpleModel::get_line_content(model, line_number);
            let units = line_content.units();
            let split = ((column - 1).max(0) as usize).min(units.len());
            let mut expected: Vec<u16> = units[..split].to_vec();
            expected.extend(expected_insert.encode_utf16());
            expected.extend_from_slice(&units[split..]);
            move_to(editor, view_model, line_number, column, false);
            type_text(view_model, chr, Some("keyboard"));
            assert_eq!(get_line_content(model, line_number), String::from_utf16_lossy(&expected), "{message}");
            model.undo();
        }

        // #region tokenização da autoClosingLanguage

        /// `BaseState`, `StringState` e `BlockCommentState` de `setupAutoClosingLanguageTokenization`. O `parent` de um
        /// `BaseState` e o `parentState` dos outros dois guardam o estado de fora.
        #[derive(Clone)]
        enum AutoClosingState {
            Base { parent: Option<Rc<AutoClosingState>> },
            Str { char: u16, parent_state: Rc<AutoClosingState> },
            BlockComment { parent_state: Rc<AutoClosingState> },
        }

        impl AutoClosingState {
            /// O `equals` de cada classe do original, com o `instanceof` do `BlockCommentState.equals` (que confere
            /// `StringState`, não `BlockCommentState`) preservado como está.
            fn state_equals(&self, other: &AutoClosingState) -> bool {
                match self {
                    AutoClosingState::Base { parent } => match other {
                        AutoClosingState::Base { parent: other_parent } => match (parent, other_parent) {
                            (None, None) => true,
                            (Some(parent), Some(other_parent)) => parent.state_equals(other_parent),
                            _ => false,
                        },
                        _ => false,
                    },
                    AutoClosingState::Str { char, parent_state } => match other {
                        AutoClosingState::Str { char: other_char, parent_state: other_parent_state } => {
                            char == other_char && parent_state.state_equals(other_parent_state)
                        }
                        _ => false,
                    },
                    AutoClosingState::BlockComment { parent_state } => match other {
                        AutoClosingState::Str { parent_state: other_parent_state, .. } => parent_state.state_equals(other_parent_state),
                        _ => false,
                    },
                }
            }
        }

        impl IState for AutoClosingState {
            fn clone_state(&self) -> State {
                Rc::new(self.clone())
            }

            fn equals(&self, other: &dyn IState) -> bool {
                other.as_any().downcast_ref::<AutoClosingState>().is_some_and(|other| self.state_equals(other))
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        /// O `{ length, type }` de `tokens` no `tokenizeEncoded`.
        struct TokenSpan {
            length: usize,
            token_type: u32,
        }

        /// O estado local de um `tokenizeEncoded`: a linha que falta consumir, o estado e os tokens até aqui.
        struct LineTokenizer {
            line: Vec<u16>,
            state: AutoClosingState,
            tokens: Vec<TokenSpan>,
        }

        impl LineTokenizer {
            /// `generateToken(length, type, newState?)`.
            fn generate_token(&mut self, length: usize, token_type: u32, new_state: Option<AutoClosingState>) {
                match self.tokens.last_mut() {
                    Some(last) if last.token_type == token_type => {
                        // grow last tokens
                        last.length += length;
                    }
                    _ => self.tokens.push(TokenSpan { length, token_type }),
                }
                self.line.drain(..length.min(self.line.len()));
                if let Some(new_state) = new_state {
                    self.state = new_state;
                }
            }

            /// O tamanho do trecho inicial da linha cujos caracteres satisfazem `accepts` (o `^[...]+` do original).
            fn leading_run(&self, accepts: impl Fn(u16) -> bool) -> usize {
                self.line.iter().take_while(|unit| accepts(**unit)).count()
            }

            /// `advance()`.
            fn advance(&mut self) {
                const SINGLE_QUOTE: u16 = b'\'' as u16;
                const DOUBLE_QUOTE: u16 = b'"' as u16;
                const BACKTICK: u16 = b'`' as u16;
                const OPEN_BRACE: u16 = b'{' as u16;
                const CLOSE_BRACE: u16 = b'}' as u16;
                const SLASH: u16 = b'/' as u16;
                const STAR: u16 = b'*' as u16;
                const BACKSLASH: u16 = b'\\' as u16;
                const DOLLAR: u16 = b'$' as u16;

                let state = self.state.clone();
                let first = self.line[0];
                match &state {
                    AutoClosingState::Base { parent } => {
                        let run = self.leading_run(|unit| !matches!(unit, SINGLE_QUOTE | DOUBLE_QUOTE | BACKTICK | OPEN_BRACE | CLOSE_BRACE | SLASH));
                        if run > 0 {
                            return self.generate_token(run, standard_token_type::OTHER, None);
                        }
                        if matches!(first, SINGLE_QUOTE | DOUBLE_QUOTE | BACKTICK) {
                            let new_state = AutoClosingState::Str { char: first, parent_state: Rc::new(state.clone()) };
                            return self.generate_token(1, standard_token_type::STRING, Some(new_state));
                        }
                        if first == OPEN_BRACE {
                            let new_state = AutoClosingState::Base { parent: Some(Rc::new(state.clone())) };
                            return self.generate_token(1, standard_token_type::OTHER, Some(new_state));
                        }
                        if first == CLOSE_BRACE {
                            let new_state = match parent {
                                Some(parent) => (**parent).clone(),
                                None => AutoClosingState::Base { parent: None },
                            };
                            return self.generate_token(1, standard_token_type::OTHER, Some(new_state));
                        }
                        if self.line.starts_with(&[SLASH, SLASH]) {
                            return self.generate_token(self.line.len(), standard_token_type::COMMENT, None);
                        }
                        if self.line.starts_with(&[SLASH, STAR]) {
                            let new_state = AutoClosingState::BlockComment { parent_state: Rc::new(state.clone()) };
                            return self.generate_token(2, standard_token_type::COMMENT, Some(new_state));
                        }
                        self.generate_token(1, standard_token_type::OTHER, None)
                    }
                    AutoClosingState::Str { char, parent_state } => {
                        let run = self.leading_run(|unit| !matches!(unit, BACKSLASH | SINGLE_QUOTE | DOUBLE_QUOTE | BACKTICK | DOLLAR));
                        if run > 0 {
                            return self.generate_token(run, standard_token_type::STRING, None);
                        }
                        if first == BACKSLASH {
                            return self.generate_token(2, standard_token_type::STRING, None);
                        }
                        if first == *char {
                            return self.generate_token(1, standard_token_type::STRING, Some((**parent_state).clone()));
                        }
                        if self.line.starts_with(&[DOLLAR, OPEN_BRACE]) {
                            let new_state = AutoClosingState::Base { parent: Some(Rc::new(state.clone())) };
                            return self.generate_token(2, standard_token_type::OTHER, Some(new_state));
                        }
                        self.generate_token(1, standard_token_type::OTHER, None)
                    }
                    AutoClosingState::BlockComment { parent_state } => {
                        let run = self.leading_run(|unit| unit != STAR);
                        if run > 0 {
                            return self.generate_token(run, standard_token_type::STRING, None);
                        }
                        if self.line.starts_with(&[STAR, SLASH]) {
                            return self.generate_token(2, standard_token_type::COMMENT, Some((**parent_state).clone()));
                        }
                        self.generate_token(1, standard_token_type::OTHER, None)
                    }
                }
            }
        }

        /// O objeto `ITokenizationSupport` de `setupAutoClosingLanguageTokenization`.
        struct AutoClosingTokenizationSupport {
            encoded_language_id: u32,
        }

        impl ITokenizationSupport for AutoClosingTokenizationSupport {
            fn get_initial_state(&self) -> State {
                Rc::new(AutoClosingState::Base { parent: None })
            }

            fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
                unreachable!("`tokenize: undefined!` no original")
            }

            fn tokenize_encoded(&self, line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
                let state = state.as_any().downcast_ref::<AutoClosingState>().expect("o estado da autoClosingLanguage").clone();
                let mut tokenizer = LineTokenizer { line: line.units().to_vec(), state, tokens: Vec::new() };
                while !tokenizer.line.is_empty() {
                    tokenizer.advance();
                }
                let mut result: Vec<u32> = Vec::with_capacity(tokenizer.tokens.len() * 2);
                let mut start_index = 0u32;
                for token in &tokenizer.tokens {
                    result.push(start_index);
                    result.push(
                        (self.encoded_language_id << metadata_consts::LANGUAGEID_OFFSET)
                            | (token.token_type << metadata_consts::TOKEN_TYPE_OFFSET),
                    );
                    start_index += token.length as u32;
                }
                let end_state: State = Rc::new(tokenizer.state);
                EncodedTokenizationResult { tokens: result, font_info: Vec::new(), end_state }
            }
        }

        // #endregion
    }

    /// O `tokenizationSupport` do caso `issue #46314`: tokens vazios e o estado nulo.
    struct EmptyTokenizationSupport;

    impl ITokenizationSupport for EmptyTokenizationSupport {
        fn get_initial_state(&self) -> State {
            null_state()
        }

        fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
            unreachable!("`tokenize: undefined!` no original")
        }

        fn tokenize_encoded(&self, _line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
            EncodedTokenizationResult { tokens: Vec::new(), font_info: Vec::new(), end_state: state.clone() }
        }
    }

    // Casos da suíte `Editor Controller`

    #[test]
    fn editor_controller_issue_microsoft_monaco_editor_443_indentation_of_a_single_row_deletes_selected_text_in_some_cases() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(
                ["Hello world!", "another line"].join("\n"),
                None,
                RelaxedTextModelCreationOptions { insert_spaces: Some(false), ..Default::default() },
                None,
            );
            f.with_test_code_editor(model, &no_options(), |editor, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 1, 1, 13)]);

                // Check that indenting maintains the selection start at column 1
                editor.run_command(&*editing.tab, None);
                assert_eq!(view_model.get_selection(), Selection::new(1, 1, 1, 14));
            });
        });
    }

    #[test]
    fn editor_controller_bug_9121_auto_indent_undo_redo_is_funky() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(
                [""].join("\n"),
                None,
                RelaxedTextModelCreationOptions { insert_spaces: Some(false), trim_auto_whitespace: Some(false), ..Default::default() },
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_value_lf(&model), "\n", "assert1");

                editor.run_command(&*editing.tab, None);
                assert_eq!(get_value_lf(&model), "\n\t", "assert2");

                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_value_lf(&model), "\n\t\n\t", "assert3");

                type_text(view_model, "x", None);
                assert_eq!(get_value_lf(&model), "\n\t\n\tx", "assert4");

                move_left(editor, view_model, false);
                assert_eq!(get_value_lf(&model), "\n\t\n\tx", "assert5");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "\n\t\nx", "assert6");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "\n\tx", "assert7");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "\nx", "assert8");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "x", "assert9");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value_lf(&model), "\nx", "assert10");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value_lf(&model), "\n\t\nx", "assert11");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value_lf(&model), "\n\t\n\tx", "assert12");

                editor.run_command(&*editing.redo, None);
                assert_eq!(get_value_lf(&model), "\n\t\nx", "assert13");

                editor.run_command(&*editing.redo, None);
                assert_eq!(get_value_lf(&model), "\nx", "assert14");

                editor.run_command(&*editing.redo, None);
                assert_eq!(get_value_lf(&model), "x", "assert15");
            });
        });
    }

    #[test]
    fn editor_controller_issue_23539_setting_model_eol_isnt_undoable() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            f.with_test_code_editor(["Hello", "world"], &no_options(), |editor, view_model| {
                let model = editor.get_model().expect("o editor tem o modelo");

                assert_cursor(view_model, Position::new(1, 1));
                model.set_eol(EndOfLineSequence::Lf);
                assert_eq!(get_value(&model), "Hello\nworld");

                model.push_eol(EndOfLineSequence::CrLf);
                assert_eq!(get_value(&model), "Hello\r\nworld");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value(&model), "Hello\nworld");
            });
        });
    }

    #[test]
    fn editor_controller_issue_47733_undo_mangles_unicode_characters() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let language_id = "myMode";

            f.register_language(
                language_id,
                LanguageConfiguration {
                    surrounding_pairs: Some(vec![AutoClosingPair::new("%", "%")]),
                    ..LanguageConfiguration::default()
                },
            );

            let model = f.create_text_model("'👁'", Some(language_id), Default::default(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                editor.set_selection(Selection::new(1, 1, 1, 2).into(), None);

                type_text(view_model, "%", Some("keyboard"));
                assert_eq!(get_value_lf(&model), "%'%👁'", "assert1");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value_lf(&model), "'👁'", "assert2");
            });
        });
    }

    #[test]
    fn editor_controller_issue_46208_allow_empty_selections_in_the_undo_redo_stack() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model("", None, Default::default(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                type_text(view_model, "Hello", Some("keyboard"));
                type_text(view_model, " ", Some("keyboard"));
                type_text(view_model, "world", Some("keyboard"));
                type_text(view_model, " ", Some("keyboard"));
                assert_eq!(get_line_content(&model, 1), "Hello world ");
                assert_cursor(view_model, Position::new(1, 13));

                move_left(editor, view_model, false);
                move_right(editor, view_model, false);

                let mut cursor_state_computer = |_: &[ValidEditOperation]| -> Option<Vec<Selection>> { Some(Vec::new()) };
                model.push_edit_operations(
                    Some(Vec::new()),
                    &[EditOperation::replace_move(Range::new(1, 12, 1, 13), Some(JsString::from("")))],
                    Some(&mut cursor_state_computer),
                    None,
                    None,
                );
                assert_eq!(get_line_content(&model, 1), "Hello world");
                assert_cursor(view_model, Position::new(1, 12));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(&model, 1), "Hello world ");
                assert_cursor(view_model, Selection::new(1, 13, 1, 13));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(&model, 1), "Hello world");
                assert_cursor(view_model, Position::new(1, 12));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(&model, 1), "Hello");
                assert_cursor(view_model, Position::new(1, 6));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(&model, 1), "");
                assert_cursor(view_model, Position::new(1, 1));

                editor.run_command(&*editing.redo, None);
                assert_eq!(get_line_content(&model, 1), "Hello");
                assert_cursor(view_model, Position::new(1, 6));

                editor.run_command(&*editing.redo, None);
                assert_eq!(get_line_content(&model, 1), "Hello world");
                assert_cursor(view_model, Position::new(1, 12));

                editor.run_command(&*editing.redo, None);
                assert_eq!(get_line_content(&model, 1), "Hello world ");
                assert_cursor(view_model, Position::new(1, 13));

                editor.run_command(&*editing.redo, None);
                assert_eq!(get_line_content(&model, 1), "Hello world");
                assert_cursor(view_model, Position::new(1, 12));

                editor.run_command(&*editing.redo, None);
                assert_eq!(get_line_content(&model, 1), "Hello world");
                assert_cursor(view_model, Position::new(1, 12));
            });
        });
    }

    #[test]
    fn editor_controller_bug_16815_shift_tab_doesnt_go_back_to_tabstop() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let language_id = f.setup_on_enter_language(IndentAction::IndentOutdent);
            let model = f.create_text_model(["     function baz() {"].join("\n"), Some(language_id.as_str()), Default::default(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 1, 6, false);
                assert_cursor(view_model, Selection::new(1, 6, 1, 6));

                editor.run_command(&*editing.outdent, None);
                assert_eq!(get_line_content(&model, 1), "    function baz() {");
                assert_cursor(view_model, Selection::new(1, 5, 1, 5));
            });
        });
    }

    #[test]
    fn editor_controller_bug_18293_regression_editor_cant_outdent_whitespace_line() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(["      "].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 1, 7, false);
                assert_cursor(view_model, Selection::new(1, 7, 1, 7));

                editor.run_command(&*editing.outdent, None);
                assert_eq!(get_line_content(&model, 1), "    ");
                assert_cursor(view_model, Selection::new(1, 5, 1, 5));
            });
        });
    }

    #[test]
    fn editor_controller_issue_95591_unindenting_moves_cursor_to_beginning_of_line() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(["        "].join("\n"), None, Default::default(), None);

            let options = Dynamic::object([("useTabStops", Dynamic::from(false))]);
            f.with_test_code_editor(model.clone(), &options, |editor, view_model| {
                move_to(editor, view_model, 1, 9, false);
                assert_cursor(view_model, Selection::new(1, 9, 1, 9));

                editor.run_command(&*editing.outdent, None);
                assert_eq!(get_line_content(&model, 1), "    ");
                assert_cursor(view_model, Selection::new(1, 5, 1, 5));
            });
        });
    }

    #[test]
    fn editor_controller_bug_16657_editor_tab_on_empty_line_of_zero_indentation_moves_cursor_to_position_1_1() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(
                [
                    "function baz() {",
                    "\tfunction hello() { // something here",
                    "\t",
                    "",
                    "\t}",
                    "}",
                    "",
                ]
                .join("\n"),
                None,
                RelaxedTextModelCreationOptions { insert_spaces: Some(false), ..Default::default() },
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 7, 1, false);
                assert_cursor(view_model, Selection::new(7, 1, 7, 1));

                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 7), "\t");
                assert_cursor(view_model, Selection::new(7, 2, 7, 2));
            });
        });
    }

    #[test]
    fn editor_controller_bug_16740_editor_cut_line_doesnt_quite_cut_the_last_line() {
        run_test(|f| {
            // Part 1 => there is text on the last line
            f.with_test_code_editor(["asdasd", "qwerty"], &no_options(), |editor, view_model| {
                let model = editor.get_model().expect("o editor tem o modelo");

                move_to(editor, view_model, 2, 1, false);
                assert_cursor(view_model, Selection::new(2, 1, 2, 1));

                view_model.cut(Some("keyboard"));
                assert_eq!(model.get_line_count(), 1);
                assert_eq!(get_line_content(&model, 1), "asdasd");
            });

            // Part 2 => there is no text on the last line
            f.with_test_code_editor(["asdasd", ""], &no_options(), |editor, view_model| {
                let model = editor.get_model().expect("o editor tem o modelo");

                move_to(editor, view_model, 2, 1, false);
                assert_cursor(view_model, Selection::new(2, 1, 2, 1));

                view_model.cut(Some("keyboard"));
                assert_eq!(model.get_line_count(), 1);
                assert_eq!(get_line_content(&model, 1), "asdasd");

                view_model.cut(Some("keyboard"));
                assert_eq!(model.get_line_count(), 1);
                assert_eq!(get_line_content(&model, 1), "");
            });
        });
    }

    #[test]
    fn editor_controller_issue_128602_when_cutting_multiple_lines_ctrl_x_the_last_line_will_not_be_erased() {
        run_test(|f| {
            f.with_test_code_editor(["a1", "a2", "a3"], &no_options(), |editor, view_model| {
                let model = editor.get_model().expect("o editor tem o modelo");

                set_test_selections(
                    view_model,
                    &[Selection::new(1, 1, 1, 1), Selection::new(2, 1, 2, 1), Selection::new(3, 1, 3, 1)],
                );

                view_model.cut(Some("keyboard"));
                assert_eq!(model.get_line_count(), 1);
                assert_eq!(get_line_content(&model, 1), "");
            });
        });
    }

    #[test]
    fn editor_controller_bug_11476_double_bracket_surrounding_undo_is_broken() {
        run_test(|f| {
            f.using_cursor(
                CursorOpts { text: vec!["hello"], language_id: Some(SURROUNDING_LANGUAGE_ID), ..Default::default() },
                |editor, _model, view_model| {
                    move_to(editor, view_model, 1, 3, false);
                    move_to(editor, view_model, 1, 5, true);
                    assert_cursor(view_model, Selection::new(1, 3, 1, 5));

                    type_text(view_model, "(", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(1, 4, 1, 6));

                    type_text(view_model, "(", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(1, 5, 1, 7));
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_206774_surround_selection_command_with_empty_char_after_selection_should_not_throw() {
        // This test reproduces the issue where SurroundSelectionCommand throws when charAfterSelection is empty
        // The problem is that addTrackedEditOperation ignores empty strings, causing computeCursorState to fail
        // when trying to access inverseEditOperations[1].range (which is undefined)
        run_test(|f| {
            f.using_cursor(
                CursorOpts {
                    text: vec!["hello world"],
                    language_id: Some(EMPTY_CLOSING_SURROUND_LANGUAGE_ID),
                    ..Default::default()
                },
                |editor, model, view_model| {
                    // Select "hello"
                    move_to(editor, view_model, 1, 1, false);
                    move_to(editor, view_model, 1, 6, true);
                    assert_cursor(view_model, Selection::new(1, 1, 1, 6));

                    // Type < which should surround with '<' and empty string
                    // This reproduces the crash where charAfterSelection is empty
                    type_text(view_model, "<", Some("keyboard"));

                    // Test passes if we don't crash - the exact cursor position depends on the fix
                    // The main issue is that computeCursorState fails when charAfterSelection is empty
                    assert_eq!(get_value(model), "<hello world");
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_1140_backspace_stops_prematurely() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(["function baz() {", "  return 1;", "};"].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 3, 2, false);
                move_to(editor, view_model, 1, 14, true);
                assert_cursor(view_model, Selection::new(3, 2, 1, 14));

                editor.run_command(&*editing.delete_left, None);
                assert_cursor(view_model, Selection::new(1, 14, 1, 14));
                assert_eq!(model.get_line_count(), 1);
                assert_eq!(get_line_content(&model, 1), "function baz(;");
            });
        });
    }

    #[test]
    fn editor_controller_issue_10212_pasting_entire_line_does_not_replace_selection() {
        run_test(|f| {
            f.using_cursor(CursorOpts { text: vec!["line1", "line2"], ..Default::default() }, |editor, model, view_model| {
                move_to(editor, view_model, 2, 1, false);
                move_to(editor, view_model, 2, 6, true);

                view_model.paste(&u("line1\n"), true, None, None, false);

                assert_eq!(get_line_content(model, 1), "line1");
                assert_eq!(get_line_content(model, 2), "line1");
                assert_eq!(get_line_content(model, 3), "");
            });
        });
    }

    #[test]
    fn editor_controller_issue_74722_pasting_whole_line_does_not_replace_selection() {
        run_test(|f| {
            f.using_cursor(CursorOpts { text: vec!["line1", "line sel 2", "line3"], ..Default::default() }, |_editor, model, view_model| {
                set_test_selections(view_model, &[Selection::new(2, 6, 2, 9)]);

                view_model.paste(&u("line1\n"), true, None, None, false);

                assert_eq!(get_line_content(model, 1), "line1");
                assert_eq!(get_line_content(model, 2), "line line1");
                assert_eq!(get_line_content(model, 3), " 2");
                assert_eq!(get_line_content(model, 4), "line3");
            });
        });
    }

    #[test]
    fn editor_controller_issue_4996_multiple_cursor_paste_pastes_contents_of_all_cursors() {
        run_test(|f| {
            f.using_cursor(CursorOpts { text: vec!["line1", "line2", "line3"], ..Default::default() }, |_editor, model, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 1, 1, 1), Selection::new(2, 1, 2, 1)]);

                view_model.paste(
                    &u("a\nb\nc\nd"),
                    false,
                    Some(&[JsString::from("a\nb"), JsString::from("c\nd")][..]),
                    None,
                    false,
                );

                assert_eq!(get_value(model), ["a", "bline1", "c", "dline2", "line3"].join("\n"));
            });
        });
    }

    #[test]
    fn editor_controller_issue_16155_paste_into_multiple_cursors_has_edge_case_when_number_of_lines_equals_number_of_cursors_minus_1() {
        run_test(|f| {
            f.using_cursor(CursorOpts { text: vec!["test", "test", "test", "test"], ..Default::default() }, |_editor, model, view_model| {
                set_test_selections(
                    view_model,
                    &[Selection::new(1, 1, 1, 5), Selection::new(2, 1, 2, 5), Selection::new(3, 1, 3, 5), Selection::new(4, 1, 4, 5)],
                );

                view_model.paste(&u("aaa\nbbb\nccc\n"), false, None, None, false);

                assert_eq!(
                    get_value(model),
                    [
                        "aaa", "bbb", "ccc", "", "aaa", "bbb", "ccc", "", "aaa", "bbb", "ccc", "", "aaa", "bbb", "ccc", "",
                    ]
                    .join("\n")
                );
            });
        });
    }

    #[test]
    fn editor_controller_issue_43722_multiline_paste_doesnt_work_anymore() {
        run_test(|f| {
            f.using_cursor(CursorOpts { text: vec!["test", "test", "test", "test"], ..Default::default() }, |_editor, model, view_model| {
                set_test_selections(
                    view_model,
                    &[Selection::new(1, 1, 1, 5), Selection::new(2, 1, 2, 5), Selection::new(3, 1, 3, 5), Selection::new(4, 1, 4, 5)],
                );

                view_model.paste(&u("aaa\r\nbbb\r\nccc\r\nddd\r\n"), false, None, None, false);

                assert_eq!(get_value(model), ["aaa", "bbb", "ccc", "ddd"].join("\n"));
            });
        });
    }

    #[test]
    fn editor_controller_issue_46440_1_pasting_a_multi_line_selection_pastes_entire_selection_into_every_insertion_point() {
        run_test(|f| {
            f.using_cursor(CursorOpts { text: vec!["line1", "line2", "line3"], ..Default::default() }, |_editor, model, view_model| {
                set_test_selections(
                    view_model,
                    &[Selection::new(1, 1, 1, 1), Selection::new(2, 1, 2, 1), Selection::new(3, 1, 3, 1)],
                );

                view_model.paste(&u("a\nb\nc"), false, None, None, false);

                assert_eq!(get_value(model), ["aline1", "bline2", "cline3"].join("\n"));
            });
        });
    }

    #[test]
    fn editor_controller_issue_46440_2_pasting_a_multi_line_selection_pastes_entire_selection_into_every_insertion_point() {
        run_test(|f| {
            f.using_cursor(CursorOpts { text: vec!["line1", "line2", "line3"], ..Default::default() }, |_editor, model, view_model| {
                set_test_selections(
                    view_model,
                    &[Selection::new(1, 1, 1, 1), Selection::new(2, 1, 2, 1), Selection::new(3, 1, 3, 1)],
                );

                view_model.paste(&u("a\nb\nc\n"), false, None, None, false);

                assert_eq!(get_value(model), ["aline1", "bline2", "cline3"].join("\n"));
            });
        });
    }

    #[test]
    fn editor_controller_issue_256039_paste_from_multiple_cursors_with_empty_selections_and_multi_cursor_paste_full() {
        run_test(|f| {
            f.using_cursor(
                CursorOpts {
                    text: vec!["line1", "line2", "line3"],
                    editor_opts: Some(Dynamic::object([("multiCursorPaste", Dynamic::from("full"))])),
                    ..Default::default()
                },
                |_editor, model, view_model| {
                    // 2 cursors on lines 1 and 2
                    set_test_selections(view_model, &[Selection::new(1, 1, 1, 1), Selection::new(2, 1, 2, 1)]);

                    view_model.paste(
                        &u("line1\nline2\n"),
                        true,
                        Some(&[JsString::from("line1\n"), JsString::from("line2\n")][..]),
                        None,
                        false,
                    );

                    // Each cursor gets its respective line
                    assert_eq!(get_value(model), ["line1", "line1", "line2", "line2", "line3"].join("\n"));
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_3071_investigate_why_undo_stack_gets_corrupted() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(["some lines", "and more lines", "just some text"].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 1, 1, false);
                move_to(editor, view_model, 3, 4, true);

                let is_first = Rc::new(Cell::new(true));
                let listener_view_model = view_model.clone();
                let listener_is_first = is_first.clone();
                let disposable = model.on_did_change_content().listen(move |_| {
                    if listener_is_first.get() {
                        listener_is_first.set(false);
                        type_text(&listener_view_model, "\t", Some("keyboard"));
                    }
                });

                editor.run_command(&*editing.tab, None);
                assert_eq!(get_value(&model), ["\t just some text"].join("\n"), "001");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value(&model), ["    some lines", "    and more lines", "    just some text"].join("\n"), "002");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value(&model), ["some lines", "and more lines", "just some text"].join("\n"), "003");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value(&model), ["some lines", "and more lines", "just some text"].join("\n"), "004");

                disposable.dispose();
            });
        });
    }

    #[test]
    fn editor_controller_issue_12950_cannot_double_click_to_insert_emoji_using_osx_emoji_panel() {
        run_test(|f| {
            f.using_cursor(
                CursorOpts { text: vec!["some lines", "and more lines", "just some text"], language_id: None, ..Default::default() },
                |editor, model, view_model| {
                    move_to(editor, view_model, 3, 1, false);

                    type_text(view_model, "😍", Some("keyboard"));

                    assert_eq!(get_value(model), ["some lines", "and more lines", "😍just some text"].join("\n"));
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_3463_pressing_tab_adds_spaces_but_not_as_many_as_for_a_tab() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(
                ["function a() {", "\tvar a = {", "\t\tx: 3", "\t};", "}"].join("\n"),
                None,
                Default::default(),
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 3, 2, false);
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 3), "\t    \tx: 3");
            });
        });
    }

    #[test]
    fn editor_controller_issue_4312_trying_to_type_a_tab_character_over_a_sequence_of_spaces_results_in_unexpected_behaviour() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(
                ["var foo = 123;       // this is a comment", "var bar = 4;       // another comment"].join("\n"),
                None,
                RelaxedTextModelCreationOptions { insert_spaces: Some(false), ..Default::default() },
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 1, 15, false);
                move_to(editor, view_model, 1, 22, true);
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 1), "var foo = 123;\t// this is a comment");
            });
        });
    }

    #[test]
    fn editor_controller_issue_832_word_right() {
        run_test(|f| {
            let navigation = core_navigation_commands::instance();
            f.using_cursor(
                CursorOpts { text: vec!["   /* Just some   more   text a+= 3 +5-3 + 7 */  "], ..Default::default() },
                |editor, _model, view_model| {
                    move_to(editor, view_model, 1, 1, false);

                    // `expected` é o texto que o original mede com `.length`: a coluna esperada é o comprimento mais um.
                    let assert_word_right = |col: i32, expected: &str| {
                        if col == 1 {
                            run_move_command(&navigation.word_select, view_model, 1, col);
                        } else {
                            run_move_command(&navigation.word_select_drag, view_model, 1, col);
                        }

                        let expected_col = expected.encode_utf16().count() as i32 + 1;
                        assert_eq!(view_model.get_selection().start_column, 1, "TEST FOR {col}");
                        assert_eq!(view_model.get_selection().end_column, expected_col, "TEST FOR {col}");
                    };

                    assert_word_right(1, "   ");
                    assert_word_right(2, "   ");
                    assert_word_right(3, "   ");
                    assert_word_right(4, "   ");
                    assert_word_right(5, "   /");
                    assert_word_right(6, "   /*");
                    assert_word_right(7, "   /* ");
                    assert_word_right(8, "   /* Just");
                    assert_word_right(9, "   /* Just");
                    assert_word_right(10, "   /* Just");
                    assert_word_right(11, "   /* Just");
                    assert_word_right(12, "   /* Just ");
                    assert_word_right(13, "   /* Just some");
                    assert_word_right(14, "   /* Just some");
                    assert_word_right(15, "   /* Just some");
                    assert_word_right(16, "   /* Just some");
                    assert_word_right(17, "   /* Just some ");
                    assert_word_right(18, "   /* Just some  ");
                    assert_word_right(19, "   /* Just some   ");
                    assert_word_right(20, "   /* Just some   more");
                    assert_word_right(21, "   /* Just some   more");
                    assert_word_right(22, "   /* Just some   more");
                    assert_word_right(23, "   /* Just some   more");
                    assert_word_right(24, "   /* Just some   more ");
                    assert_word_right(25, "   /* Just some   more  ");
                    assert_word_right(26, "   /* Just some   more   ");
                    assert_word_right(27, "   /* Just some   more   text");
                    assert_word_right(28, "   /* Just some   more   text");
                    assert_word_right(29, "   /* Just some   more   text");
                    assert_word_right(30, "   /* Just some   more   text");
                    assert_word_right(31, "   /* Just some   more   text ");
                    assert_word_right(32, "   /* Just some   more   text a");
                    assert_word_right(33, "   /* Just some   more   text a+");
                    assert_word_right(34, "   /* Just some   more   text a+=");
                    assert_word_right(35, "   /* Just some   more   text a+= ");
                    assert_word_right(36, "   /* Just some   more   text a+= 3");
                    assert_word_right(37, "   /* Just some   more   text a+= 3 ");
                    assert_word_right(38, "   /* Just some   more   text a+= 3 +");
                    assert_word_right(39, "   /* Just some   more   text a+= 3 +5");
                    assert_word_right(40, "   /* Just some   more   text a+= 3 +5-");
                    assert_word_right(41, "   /* Just some   more   text a+= 3 +5-3");
                    assert_word_right(42, "   /* Just some   more   text a+= 3 +5-3 ");
                    assert_word_right(43, "   /* Just some   more   text a+= 3 +5-3 +");
                    assert_word_right(44, "   /* Just some   more   text a+= 3 +5-3 + ");
                    assert_word_right(45, "   /* Just some   more   text a+= 3 +5-3 + 7");
                    assert_word_right(46, "   /* Just some   more   text a+= 3 +5-3 + 7 ");
                    assert_word_right(47, "   /* Just some   more   text a+= 3 +5-3 + 7 *");
                    assert_word_right(48, "   /* Just some   more   text a+= 3 +5-3 + 7 */");
                    assert_word_right(49, "   /* Just some   more   text a+= 3 +5-3 + 7 */ ");
                    assert_word_right(50, "   /* Just some   more   text a+= 3 +5-3 + 7 */  ");
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_33788_wrong_cursor_position_when_double_click_to_select_a_word() {
        run_test(|f| {
            let navigation = core_navigation_commands::instance();
            let model = f.create_text_model(["Just some text"].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model, &no_options(), |_editor, view_model| {
                run_move_command(&navigation.word_select, view_model, 1, 8);
                assert_eq!(view_model.get_selection(), Selection::new(1, 6, 1, 10));

                run_move_command(&navigation.word_select_drag, view_model, 1, 8);
                assert_eq!(view_model.get_selection(), Selection::new(1, 6, 1, 10));
            });
        });
    }

    #[test]
    fn editor_controller_issue_12887_double_click_highlighting_separating_white_space() {
        run_test(|f| {
            let navigation = core_navigation_commands::instance();
            let model = f.create_text_model(["abc def"].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model, &no_options(), |_editor, view_model| {
                run_move_command(&navigation.word_select, view_model, 1, 5);
                assert_eq!(view_model.get_selection(), Selection::new(1, 5, 1, 8));
            });
        });
    }

    #[test]
    fn editor_controller_double_click_on_punctuation_should_select_the_character_not_adjacent_space() {
        run_test(|f| {
            let navigation = core_navigation_commands::instance();
            let model = f.create_text_model(
                ["// a b c 1 2 3 ~ ! @ # $ % ^ & * ( ) _ + \\ /"].join("\n"),
                None,
                Default::default(),
                None,
            );

            f.with_test_code_editor(model, &no_options(), |_editor, view_model| {
                // Test double-click on '@' at position 20
                run_move_command(&navigation.word_select, view_model, 1, 20);
                assert_eq!(view_model.get_selection(), Selection::new(1, 20, 1, 21), "Should select @ character");

                // Test double-click on '#' at position 22
                run_move_command(&navigation.word_select, view_model, 1, 22);
                assert_eq!(view_model.get_selection(), Selection::new(1, 22, 1, 23), "Should select # character");

                // Test double-click on '!' at position 18
                run_move_command(&navigation.word_select, view_model, 1, 18);
                assert_eq!(view_model.get_selection(), Selection::new(1, 18, 1, 19), "Should select ! character");

                // Test double-click on first '/' in '//' at position 1
                run_move_command(&navigation.word_select, view_model, 1, 1);
                assert_eq!(view_model.get_selection(), Selection::new(1, 1, 1, 3), "Should select // token");

                // Test double-click on second '/' in '//' at position 2
                run_move_command(&navigation.word_select, view_model, 1, 2);
                assert_eq!(view_model.get_selection(), Selection::new(1, 1, 1, 3), "Should select // token");

                // Test double-click on '\' at position 42
                run_move_command(&navigation.word_select, view_model, 1, 42);
                assert_eq!(view_model.get_selection(), Selection::new(1, 42, 1, 43), "Should select \\ character");
            });
        });
    }

    #[test]
    fn editor_controller_issue_9675_undo_redo_adds_a_stop_in_between_chn_characters() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            f.with_test_code_editor([""; 0], &no_options(), |editor, view_model| {
                let model = editor.get_model().expect("o editor tem o modelo");
                assert_cursor(view_model, Position::new(1, 1));

                // Typing sennsei in Japanese - Hiragana
                type_text(view_model, "ｓ", Some("keyboard"));
                view_model.composition_type(&u("せ"), 1, 0, 0, None);
                view_model.composition_type(&u("せｎ"), 1, 0, 0, None);
                view_model.composition_type(&u("せん"), 2, 0, 0, None);
                view_model.composition_type(&u("せんｓ"), 2, 0, 0, None);
                view_model.composition_type(&u("せんせ"), 3, 0, 0, None);
                view_model.composition_type(&u("せんせ"), 3, 0, 0, None);
                view_model.composition_type(&u("せんせい"), 3, 0, 0, None);
                view_model.composition_type(&u("せんせい"), 4, 0, 0, None);
                view_model.composition_type(&u("せんせい"), 4, 0, 0, None);
                view_model.composition_type(&u("せんせい"), 4, 0, 0, None);

                assert_eq!(get_line_content(&model, 1), "せんせい");
                assert_cursor(view_model, Position::new(1, 5));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(&model, 1), "");
                assert_cursor(view_model, Position::new(1, 1));
            });
        });
    }

    #[test]
    fn editor_controller_issue_23983_calling_model_set_eol_does_not_reset_cursor_position() {
        run_test(|f| {
            f.using_cursor(CursorOpts { text: vec!["first line", "second line"], ..Default::default() }, |_editor, model, view_model| {
                model.set_eol(EndOfLineSequence::CrLf);

                set_test_selections(view_model, &[Selection::new(2, 2, 2, 2)]);
                model.set_eol(EndOfLineSequence::Lf);

                assert_cursor(view_model, Selection::new(2, 2, 2, 2));
            });
        });
    }

    #[test]
    fn editor_controller_issue_23983_calling_model_set_value_resets_cursor_position() {
        run_test(|f| {
            f.using_cursor(CursorOpts { text: vec!["first line", "second line"], ..Default::default() }, |_editor, model, view_model| {
                model.set_eol(EndOfLineSequence::CrLf);

                set_test_selections(view_model, &[Selection::new(2, 2, 2, 2)]);
                model.set_value(["different first line", "different second line", "new third line"].join("\n").into());

                assert_cursor(view_model, Selection::new(1, 1, 1, 1));
            });
        });
    }

    #[test]
    fn editor_controller_issue_36740_wordwrap_creates_an_extra_step_character_at_the_wrapping_point() {
        run_test(|f| {
            // a single model line => 4 view lines
            f.with_test_code_editor(
                [["Lorem ipsum ", "dolor sit amet ", "consectetur ", "adipiscing elit"].join("")],
                &word_wrap_options(16),
                |editor, view_model| {
                    set_test_selections(view_model, &[Selection::new(1, 7, 1, 7)]);

                    move_right(editor, view_model, false);
                    assert_cursor(view_model, Selection::new(1, 8, 1, 8));

                    move_right(editor, view_model, false);
                    assert_cursor(view_model, Selection::new(1, 9, 1, 9));

                    move_right(editor, view_model, false);
                    assert_cursor(view_model, Selection::new(1, 10, 1, 10));

                    move_right(editor, view_model, false);
                    assert_cursor(view_model, Selection::new(1, 11, 1, 11));

                    move_right(editor, view_model, false);
                    assert_cursor(view_model, Selection::new(1, 12, 1, 12));

                    move_right(editor, view_model, false);
                    assert_cursor(view_model, Selection::new(1, 13, 1, 13));

                    // moving to view line 2
                    move_right(editor, view_model, false);
                    assert_cursor(view_model, Selection::new(1, 14, 1, 14));

                    move_left(editor, view_model, false);
                    assert_cursor(view_model, Selection::new(1, 13, 1, 13));

                    // moving back to view line 1
                    move_left(editor, view_model, false);
                    assert_cursor(view_model, Selection::new(1, 12, 1, 12));
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_110376_multiple_selections_with_wordwrap_behave_differently() {
        run_test(|f| {
            // a single model line => 4 view lines
            f.with_test_code_editor(
                [["just a sentence. just a ", "sentence. just a sentence."].join("")],
                &word_wrap_options(25),
                |editor, view_model| {
                    set_test_selections(
                        view_model,
                        &[Selection::new(1, 1, 1, 16), Selection::new(1, 18, 1, 33), Selection::new(1, 35, 1, 50)],
                    );

                    move_left(editor, view_model, false);
                    assert_cursor(
                        view_model,
                        vec![Selection::new(1, 1, 1, 1), Selection::new(1, 18, 1, 18), Selection::new(1, 35, 1, 35)],
                    );

                    set_test_selections(
                        view_model,
                        &[Selection::new(1, 1, 1, 16), Selection::new(1, 18, 1, 33), Selection::new(1, 35, 1, 50)],
                    );

                    move_right(editor, view_model, false);
                    assert_cursor(
                        view_model,
                        vec![Selection::new(1, 16, 1, 16), Selection::new(1, 33, 1, 33), Selection::new(1, 50, 1, 50)],
                    );
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_98320_multi_cursor_wrap_lines_and_cursor_select_right_cursors_out_of_sync() {
        run_test(|f| {
            // a single model line => 4 view lines
            f.with_test_code_editor(
                [[
                    "lorem_ipsum-1993x11x13",
                    "dolor_sit_amet-1998x04x27",
                    "consectetur-2007x10x08",
                    "adipiscing-2012x07x27",
                    "elit-2015x02x27",
                ]
                .join("\n")],
                &word_wrap_options(16),
                |editor, view_model| {
                    set_test_selections(
                        view_model,
                        &[
                            Selection::new(1, 13, 1, 13),
                            Selection::new(2, 16, 2, 16),
                            Selection::new(3, 13, 3, 13),
                            Selection::new(4, 12, 4, 12),
                            Selection::new(5, 6, 5, 6),
                        ],
                    );
                    assert_cursor(
                        view_model,
                        vec![
                            Selection::new(1, 13, 1, 13),
                            Selection::new(2, 16, 2, 16),
                            Selection::new(3, 13, 3, 13),
                            Selection::new(4, 12, 4, 12),
                            Selection::new(5, 6, 5, 6),
                        ],
                    );

                    move_right(editor, view_model, true);
                    assert_cursor(
                        view_model,
                        vec![
                            Selection::new(1, 13, 1, 14),
                            Selection::new(2, 16, 2, 17),
                            Selection::new(3, 13, 3, 14),
                            Selection::new(4, 12, 4, 13),
                            Selection::new(5, 6, 5, 7),
                        ],
                    );

                    move_right(editor, view_model, true);
                    assert_cursor(
                        view_model,
                        vec![
                            Selection::new(1, 13, 1, 15),
                            Selection::new(2, 16, 2, 18),
                            Selection::new(3, 13, 3, 15),
                            Selection::new(4, 12, 4, 14),
                            Selection::new(5, 6, 5, 8),
                        ],
                    );

                    move_right(editor, view_model, true);
                    assert_cursor(
                        view_model,
                        vec![
                            Selection::new(1, 13, 1, 16),
                            Selection::new(2, 16, 2, 19),
                            Selection::new(3, 13, 3, 16),
                            Selection::new(4, 12, 4, 15),
                            Selection::new(5, 6, 5, 9),
                        ],
                    );

                    move_right(editor, view_model, true);
                    assert_cursor(
                        view_model,
                        vec![
                            Selection::new(1, 13, 1, 17),
                            Selection::new(2, 16, 2, 20),
                            Selection::new(3, 13, 3, 17),
                            Selection::new(4, 12, 4, 16),
                            Selection::new(5, 6, 5, 10),
                        ],
                    );
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_41573_delete_across_multiple_lines_does_not_shrink_the_selection_when_word_wraps() {
        run_test(|f| {
            f.with_test_code_editor(
                ["Authorization: 'Bearer pHKRfCTFSnGxs6akKlb9ddIXcca0sIUSZJutPHYqz7vEeHdMTMh0SGN0IGU3a0n59DXjTLRsj5EJ2u33qLNIFi9fk5XF8pK39PndLYUZhPt4QvHGLScgSkK0L4gwzkzMloTQPpKhqiikiIOvyNNSpd2o8j29NnOmdTUOKi9DVt74PD2ohKxyOrWZ6oZprTkb3eKajcpnS0LABKfaw2rmv4',"].join("\n"),
                &word_wrap_options(100),
                |editor, view_model| {
                    move_to(editor, view_model, 1, 43, false);
                    move_to(editor, view_model, 1, 147, true);
                    assert_cursor(view_model, Selection::new(1, 43, 1, 147));

                    editor
                        .get_model()
                        .expect("o editor tem o modelo")
                        .apply_edits(&[EditOperation::replace(Range::new(1, 1, 1, 43), Some(JsString::from("")))]);

                    assert_cursor(view_model, Selection::new(1, 1, 1, 105));
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_22717_moving_text_cursor_cause_an_incorrect_position_in_chinese() {
        run_test(|f| {
            // a single model line => 4 view lines
            f.with_test_code_editor(["一二三四五六七八九十\n12345678901234567890"], &no_options(), |editor, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 5, 1, 5)]);

                move_down(editor, view_model, false);
                assert_cursor(view_model, Selection::new(2, 9, 2, 9));

                move_right(editor, view_model, false);
                assert_cursor(view_model, Selection::new(2, 10, 2, 10));

                move_right(editor, view_model, false);
                assert_cursor(view_model, Selection::new(2, 11, 2, 11));

                move_up(editor, view_model, false);
                assert_cursor(view_model, Selection::new(1, 6, 1, 6));
            });
        });
    }

    #[test]
    fn editor_controller_issue_112301_new_sticky_tab_stops_feature_interferes_with_word_wrap() {
        run_test(|f| {
            let options = Dynamic::object([
                ("wordWrap", Dynamic::from("wordWrapColumn")),
                ("wordWrapColumn", Dynamic::from(32)),
                ("stickyTabStops", Dynamic::from(true)),
            ]);
            f.with_test_code_editor(
                [["function hello() {", "        console.log(`this is a long console message`)", "}"].join("\n")],
                &options,
                |editor, view_model| {
                    set_test_selections(view_model, &[Selection::new(2, 31, 2, 31)]);
                    move_right(editor, view_model, false);
                    assert_cursor(view_model, Position::new(2, 32));

                    move_right(editor, view_model, false);
                    assert_cursor(view_model, Position::new(2, 33));

                    move_right(editor, view_model, false);
                    assert_cursor(view_model, Position::new(2, 34));

                    move_left(editor, view_model, false);
                    assert_cursor(view_model, Position::new(2, 33));

                    move_left(editor, view_model, false);
                    assert_cursor(view_model, Position::new(2, 32));

                    move_left(editor, view_model, false);
                    assert_cursor(view_model, Position::new(2, 31));
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_44805_should_not_be_able_to_undo_in_readonly_editor() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model([""].join("\n"), None, Default::default(), None);

            let options = Dynamic::object([("readOnly", Dynamic::from(true))]);
            f.with_test_code_editor(model.clone(), &options, |editor, _view_model| {
                let mut cursor_state_computer = |_: &[ValidEditOperation]| -> Option<Vec<Selection>> { Some(vec![Selection::new(1, 1, 1, 1)]) };
                model.push_edit_operations(
                    Some(vec![Selection::new(1, 1, 1, 1)]),
                    &[EditOperation::replace(Range::new(1, 1, 1, 1), Some(JsString::from("Hello world!")))],
                    Some(&mut cursor_state_computer),
                    None,
                    None,
                );
                assert_eq!(get_value_lf(&model), "Hello world!");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value_lf(&model), "Hello world!");
            });
        });
    }

    #[test]
    fn editor_controller_issue_46314_view_model_is_out_of_sync_with_model() {
        run_test(|f| {
            let language_id = "modelModeTest1";
            let language_registration = tokenization_registry().register(language_id, Rc::new(EmptyTokenizationSupport));
            let model = f.create_text_model("Just text", Some(language_id), Default::default(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor1, _cursor1| {
                f.with_test_code_editor(model.clone(), &no_options(), |_editor2, _cursor2| {
                    let listener_model = model.clone();
                    let disposable = editor1.on_did_change_cursor_position().listen(move |_| {
                        listener_model.tokenization().tokenize_if_cheap(1);
                    });

                    model.apply_edits(&[EditOperation::replace(Range::new(1, 1, 1, 1), Some(JsString::from("-")))]);

                    disposable.dispose();
                });
            });

            language_registration.dispose();
            model.dispose();
        });
    }

    #[test]
    fn editor_controller_issue_37967_problem_replacing_consecutive_characters() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(["const a = \"foo\";", "const b = \"\""].join("\n"), None, Default::default(), None);

            let options = Dynamic::object([("multiCursorMergeOverlapping", Dynamic::from(false))]);
            f.with_test_code_editor(model.clone(), &options, |editor, view_model| {
                editor.set_selections(
                    &[Selection::new(1, 12, 1, 12), Selection::new(1, 16, 1, 16), Selection::new(2, 12, 2, 12), Selection::new(2, 13, 2, 13)],
                    None,
                    None,
                );

                editor.run_command(&*editing.delete_left, None);

                assert_cursor(
                    view_model,
                    vec![
                        Selection::new(1, 11, 1, 11),
                        Selection::new(1, 14, 1, 14),
                        Selection::new(2, 11, 2, 11),
                        Selection::new(2, 11, 2, 11),
                    ],
                );

                type_text(view_model, "'", Some("keyboard"));

                assert_eq!(get_line_content(&model, 1), "const a = 'foo';");
                assert_eq!(get_line_content(&model, 2), "const b = ''");
            });
        });
    }

    #[test]
    fn editor_controller_issue_15761_cursor_doesnt_move_in_a_redo_operation() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(["hello"].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model, &no_options(), |editor, view_model| {
                editor.set_selections(&[Selection::new(1, 4, 1, 4)], None, None);

                editor.execute_edits(
                    ExecuteEditsSource::Name(Some("test")),
                    vec![SingleEditOperation::new(Range::new(1, 1, 1, 1), Some(JsString::from("*")), true)],
                    None,
                );
                assert_cursor(view_model, vec![Selection::new(1, 5, 1, 5)]);

                editor.run_command(&*editing.undo, None);
                assert_cursor(view_model, vec![Selection::new(1, 4, 1, 4)]);

                editor.run_command(&*editing.redo, None);
                assert_cursor(view_model, vec![Selection::new(1, 5, 1, 5)]);
            });
        });
    }

    #[test]
    fn editor_controller_issue_42783_api_calls_with_undo_leave_cursor_in_wrong_position() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(["ab"].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model, &no_options(), |editor, view_model| {
                editor.set_selections(&[Selection::new(1, 1, 1, 1)], None, None);

                editor.execute_edits(
                    ExecuteEditsSource::Name(Some("test")),
                    vec![SingleEditOperation::new(Range::new(1, 1, 1, 3), Some(JsString::from("")), false)],
                    None,
                );
                assert_cursor(view_model, vec![Selection::new(1, 1, 1, 1)]);

                editor.run_command(&*editing.undo, None);
                assert_cursor(view_model, vec![Selection::new(1, 1, 1, 1)]);

                editor.execute_edits(
                    ExecuteEditsSource::Name(Some("test")),
                    vec![SingleEditOperation::new(Range::new(1, 1, 1, 2), Some(JsString::from("")), false)],
                    None,
                );
                assert_cursor(view_model, vec![Selection::new(1, 1, 1, 1)]);
            });
        });
    }

    #[test]
    fn editor_controller_issue_85712_paste_line_moves_cursor_to_start_of_current_line_rather_than_start_of_next_line() {
        run_test(|f| {
            let model = f.create_text_model(["abc123", ""].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                editor.set_selections(&[Selection::new(2, 1, 2, 1)], None, None);
                view_model.paste(&u("something\n"), true, None, None, false);
                assert_eq!(get_value(&model), ["abc123", "something", ""].join("\n"));
                assert_cursor(view_model, Position::new(3, 1));
            });
        });
    }

    #[test]
    fn editor_controller_issue_84897_left_delete_behavior_in_some_languages_is_changed() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(["สวัสดี"].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor, _view_model| {
                editor.set_selections(&[Selection::new(1, 7, 1, 7)], None, None);

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "สวัสด");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "สวัส");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "สวั");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "สว");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "ส");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "");
            });
        });
    }

}

#[allow(unused_imports)]
mod part_c {
    use super::*;
use je_editor::view_model_types::IViewModel as _;
use std::cell::Cell;
use std::rc::Rc;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable as _;
use je_editor::code_editor_widget::{ExecuteEditsSource, ICodeEditor};
use je_editor::core_commands::core_editing_commands;
use je_editor::core_commands::core_navigation_commands;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::language_configuration::{AutoClosingPair, IndentAction, LanguageConfiguration};
use je_editor::languages::{EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult};
use je_editor::model::{EndOfLineSequence, RelaxedTextModelCreationOptions, ValidEditOperation};
use je_editor::null_tokenize::null_state;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::tokenization_registry::tokenization_registry;
use support::test_view_model::no_options;
use support::u;
use super::part_b::editor_controller::*;

    // imports do fragmento C:
    use je_base::lifecycle::IDisposable as _;
    use je_editor::core_commands::{MoveCommandOptions};
    use je_editor::editor_common::{Command, CursorStateComputerData, EditOperationBuilder};
    use je_editor::language_configuration::{EnterAction, IndentationRule, OnEnterRule};
    use je_editor::model::{EndOfLinePreference, IndentSize, TextModelUpdateOptions};
    use je_editor::modes_registry::LanguageExtensionPoint;
    use je_editor::text_model::TextModel;
    use support::re;


    // Casos da suíte `Editor Controller`, linhas 3007 a 4644 do original.
    //
    // Notas de tradução deste fragmento:
    // - `model.getLineMaxColumn(n)` é [`line_max_column`], que também vale para `model.getLineContent(n).length + 1` (o
    //   máximo de coluna de uma linha é o comprimento dela mais um).
    // - `model.tokenization.forceTokenization(model.getLineCount())` é [`force_tokenization_of_all_lines`].
    // - `model.getValue(EndOfLinePreference.CRLF)` é [`get_value_crlf`].
    // - O `TestCommand` do caso `issue #6862` lê `viewModel.getSelection()` dentro do `getEditOperations` no original; aqui
    //   a seleção é lida antes de entregar o comando ao `execute_command`, porque o `EditorViewModel` está emprestado
    //   durante a execução do comando. O valor é o mesmo: nada muda a seleção entre as duas leituras.

    /// `model.getLineMaxColumn(lineNumber)`.
    fn line_max_column(model: &TextModel, line_number: i32) -> i32 {
        CursorSimpleModel::get_line_max_column(model, line_number)
    }

    /// `model.tokenization.forceTokenization(model.getLineCount())`.
    fn force_tokenization_of_all_lines(model: &TextModel) {
        model.tokenization().force_tokenization(CursorSimpleModel::get_line_count(model));
    }

    /// `model.getValue(EndOfLinePreference.CRLF)`.
    fn get_value_crlf(model: &TextModel) -> String {
        model.get_value_with(EndOfLinePreference::CrLf, false).to_string_lossy()
    }

    /// As opções de modelo `{ insertSpaces: false }`.
    fn tabs_model_options() -> RelaxedTextModelCreationOptions {
        RelaxedTextModelCreationOptions { insert_spaces: Some(false), ..Default::default() }
    }

    /// As opções de editor `{ autoIndent: mode }`.
    fn auto_indent_options(mode: &str) -> Dynamic {
        Dynamic::object([("autoIndent", Dynamic::from(mode))])
    }

    /// As opções de editor `{ useTabStops: enabled }`.
    fn use_tab_stops_options(enabled: bool) -> Dynamic {
        Dynamic::object([("useTabStops", Dynamic::from(enabled))])
    }

    /// As opções de editor `{ wordWrap: 'wordWrapColumn', wordWrapColumn: column }` com `wrappingIndent` e `stickyTabStops`
    /// opcionais.
    fn wrapped_options(column: i32, wrapping_indent: Option<&str>, sticky_tab_stops: bool) -> Dynamic {
        let mut entries = vec![("wordWrap", Dynamic::from("wordWrapColumn")), ("wordWrapColumn", Dynamic::from(column))];
        if let Some(wrapping_indent) = wrapping_indent {
            entries.push(("wrappingIndent", Dynamic::from(wrapping_indent)));
        }
        if sticky_tab_stops {
            entries.push(("stickyTabStops", Dynamic::from(true)));
        }
        Dynamic::object(entries)
    }

    /// `IndentationRule` só com `decreaseIndentPattern` e `increaseIndentPattern`, como os literais dos casos.
    fn decrease_increase_rules(decrease_pattern: &str, increase_pattern: &str, flags: &str) -> IndentationRule {
        IndentationRule {
            decrease_indent_pattern: re(decrease_pattern, flags),
            increase_indent_pattern: re(increase_pattern, flags),
            indent_next_line_pattern: None,
            un_indented_line_pattern: None,
        }
    }

    /// `CursorOpts` de uma linguagem de regras de indentação (`indentRulesLanguageId`) com as opções dadas.
    fn indent_rules_cursor_opts<'a>(
        text: Vec<&'a str>,
        model_opts: Option<RelaxedTextModelCreationOptions>,
        editor_opts: Option<Dynamic>,
    ) -> CursorOpts<'a> {
        CursorOpts { text, language_id: Some(INDENT_RULES_LANGUAGE_ID), model_opts, editor_opts }
    }

    /// O `TestCommand` do caso `issue #6862`.
    struct AutoFormatTestCommand {
        selection: Selection,
        selection_id: Option<String>,
    }

    impl Command for AutoFormatTestCommand {
        fn get_edit_operations(&mut self, _model: &TextModel, builder: &mut dyn EditOperationBuilder) {
            builder.add_edit_operation(Range::new(1, 13, 1, 14), Some(JsString::from("")), false);
            self.selection_id = Some(builder.track_selection(self.selection, None));
        }

        fn compute_cursor_state(&mut self, _model: &TextModel, helper: &dyn CursorStateComputerData) -> Selection {
            helper.get_tracked_selection(self.selection_id.as_deref().expect("getEditOperations roda antes de computeCursorState"))
        }
    }

    #[test]
    fn editor_controller_issue_122914_left_delete_behavior_in_some_languages_is_changed_use_tab_stops_false() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(["สวัสดี"].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model.clone(), &use_tab_stops_options(false), |editor, _view_model| {
                editor.set_selections(&[Selection::new(1, 7, 1, 7)], None, None);

                for expected in ["สวัสด", "สวัส", "สวั", "สว", "ส", ""] {
                    editor.run_command(&*editing.delete_left, None);
                    assert_eq!(get_value_lf(&model), expected);
                }
            });
        });
    }

    #[test]
    fn editor_controller_issue_99629_emoji_modifiers_in_text_treated_separately_when_using_backspace() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(["\u{1F476}\u{1F3FE}"].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model.clone(), &use_tab_stops_options(false), |editor, _view_model| {
                let len = model.get_value_length() as i32;
                editor.set_selections(&[Selection::new(1, 1 + len, 1, 1 + len)], None, None);

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "");
            });
        });
    }

    #[test]
    fn editor_controller_issue_99629_emoji_modifiers_in_text_treated_separately_when_using_backspace_zwj_sequence() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(
                ["\u{1F468}\u{200D}\u{1F469}\u{1F3FD}\u{200D}\u{1F467}\u{200D}\u{1F466}"].join("\n"),
                None,
                Default::default(),
                None,
            );

            f.with_test_code_editor(model.clone(), &use_tab_stops_options(false), |editor, _view_model| {
                let len = model.get_value_length() as i32;
                editor.set_selections(&[Selection::new(1, 1 + len, 1, 1 + len)], None, None);

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "\u{1F468}\u{200D}\u{1F469}\u{1F3FD}\u{200D}\u{1F467}");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "\u{1F468}\u{200D}\u{1F469}\u{1F3FD}");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "\u{1F468}");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "");
            });
        });
    }

    #[test]
    fn editor_controller_issue_105730_move_left_behaves_differently_for_multiple_cursors() {
        run_test(|f| {
            let model = f.create_text_model("asdfghjkl, asdfghjkl, asdfghjkl, ", None, Default::default(), None);

            f.with_test_code_editor(model, &wrapped_options(24, None, false), |editor, view_model| {
                set_test_selections(
                    view_model,
                    &[Selection::new(1, 10, 1, 12), Selection::new(1, 21, 1, 23), Selection::new(1, 32, 1, 34)],
                );
                move_left(editor, view_model, false);
                assert_cursor(
                    view_model,
                    vec![Selection::new(1, 10, 1, 10), Selection::new(1, 21, 1, 21), Selection::new(1, 32, 1, 32)],
                );

                set_test_selections(
                    view_model,
                    &[Selection::new(1, 10, 1, 12), Selection::new(1, 21, 1, 23), Selection::new(1, 32, 1, 34)],
                );
                move_left(editor, view_model, true);
                assert_cursor(
                    view_model,
                    vec![Selection::new(1, 10, 1, 11), Selection::new(1, 21, 1, 22), Selection::new(1, 32, 1, 33)],
                );
            });
        });
    }

    #[test]
    fn editor_controller_issue_105730_move_right_should_always_skip_wrap_point() {
        run_test(|f| {
            let model = f.create_text_model("asdfghjkl, asdfghjkl, asdfghjkl, \nasdfghjkl,", None, Default::default(), None);

            f.with_test_code_editor(model, &wrapped_options(24, None, false), |editor, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 22, 1, 22)]);
                move_right(editor, view_model, false);
                move_right(editor, view_model, false);
                assert_cursor(view_model, vec![Selection::new(1, 24, 1, 24)]);

                set_test_selections(view_model, &[Selection::new(1, 22, 1, 22)]);
                move_right(editor, view_model, true);
                move_right(editor, view_model, true);
                assert_cursor(view_model, vec![Selection::new(1, 22, 1, 24)]);
            });
        });
    }

    #[test]
    fn editor_controller_move_right_normalizes_the_cursor_to_the_next_wrapped_line() {
        run_test(|f| {
            f.with_test_code_editor("text edit", &wrapped_options(5, Some("none"), false), |editor, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 5, 1, 5)]);
                move_right(editor, view_model, false);

                assert_eq!(view_model.get_cursor_states()[0].view_state.position, Position::new(2, 1));
            });
        });
    }

    #[test]
    fn editor_controller_issue_123178_sticky_tab_in_consecutive_wrapped_lines() {
        run_test(|f| {
            let model = f.create_text_model(
                "    aaaa        aaaa",
                None,
                RelaxedTextModelCreationOptions { tab_size: Some(4), ..Default::default() },
                None,
            );

            f.with_test_code_editor(model, &wrapped_options(8, None, true), |editor, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 9, 1, 9)]);
                move_right(editor, view_model, false);
                assert_cursor(view_model, vec![Selection::new(1, 10, 1, 10)]);

                move_left(editor, view_model, false);
                assert_cursor(view_model, vec![Selection::new(1, 9, 1, 9)]);
            });
        });
    }

    #[test]
    fn editor_controller_cursor_honors_insert_spaces_configuration_on_new_line() {
        run_test(|f| {
            let navigation = core_navigation_commands::instance();
            f.using_cursor(
                CursorOpts {
                    text: vec!["    \tMy First Line\t ", "\tMy Second Line", "    Third Line", "", "1"],
                    ..Default::default()
                },
                |_editor, model, view_model| {
                    navigation.move_to.run_core_editor_command(
                        view_model,
                        &MoveCommandOptions {
                            position: Some(Position::new(1, 21)),
                            source: Some("keyboard".to_owned()),
                            ..MoveCommandOptions::default()
                        },
                    );
                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "    \tMy First Line\t ");
                    assert_eq!(get_line_content(model, 2), "        ");
                },
            );
        });
    }

    #[test]
    fn editor_controller_cursor_honors_insert_spaces_configuration_on_tab() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(
                ["    \tMy First Line\t ", "My Second Line123", "    Third Line", "", "1"].join("\n"),
                None,
                RelaxedTextModelCreationOptions { tab_size: Some(13), indent_size: Some(IndentSize::Size(13)), ..Default::default() },
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                // Tab on column 1
                move_to(editor, view_model, 2, 1, false);
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 2), "             My Second Line123");
                editor.run_command(&*editing.undo, None);

                // Tab on column 2
                assert_eq!(get_line_content(&model, 2), "My Second Line123");
                move_to(editor, view_model, 2, 2, false);
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 2), "M            y Second Line123");
                editor.run_command(&*editing.undo, None);

                // Tab on column 3
                assert_eq!(get_line_content(&model, 2), "My Second Line123");
                move_to(editor, view_model, 2, 3, false);
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 2), "My            Second Line123");
                editor.run_command(&*editing.undo, None);

                // Tab on column 4
                assert_eq!(get_line_content(&model, 2), "My Second Line123");
                move_to(editor, view_model, 2, 4, false);
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 2), "My           Second Line123");
                editor.run_command(&*editing.undo, None);

                // Tab on column 5
                assert_eq!(get_line_content(&model, 2), "My Second Line123");
                move_to(editor, view_model, 2, 5, false);
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 2), "My S         econd Line123");
                editor.run_command(&*editing.undo, None);

                // Tab on column 5
                assert_eq!(get_line_content(&model, 2), "My Second Line123");
                move_to(editor, view_model, 2, 5, false);
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 2), "My S         econd Line123");
                editor.run_command(&*editing.undo, None);

                // Tab on column 13
                assert_eq!(get_line_content(&model, 2), "My Second Line123");
                move_to(editor, view_model, 2, 13, false);
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 2), "My Second Li ne123");
                editor.run_command(&*editing.undo, None);

                // Tab on column 14
                assert_eq!(get_line_content(&model, 2), "My Second Line123");
                move_to(editor, view_model, 2, 14, false);
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 2), "My Second Lin             e123");
            });
        });
    }

    #[test]
    fn editor_controller_enter_auto_indents_with_insert_spaces_setting_1() {
        run_test(|f| {
            let language_id = f.setup_on_enter_language(IndentAction::Indent);
            f.using_cursor(
                CursorOpts { text: vec!["\thello"], language_id: Some(language_id.as_str()), ..Default::default() },
                |editor, model, view_model| {
                    move_to(editor, view_model, 1, 7, false);
                    assert_cursor(view_model, Selection::new(1, 7, 1, 7));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_value_crlf(model), "\thello\r\n        ");
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_auto_indents_with_insert_spaces_setting_2() {
        run_test(|f| {
            let language_id = f.setup_on_enter_language(IndentAction::None);
            f.using_cursor(
                CursorOpts { text: vec!["\thello"], language_id: Some(language_id.as_str()), ..Default::default() },
                |editor, model, view_model| {
                    move_to(editor, view_model, 1, 7, false);
                    assert_cursor(view_model, Selection::new(1, 7, 1, 7));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_value_crlf(model), "\thello\r\n    ");
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_auto_indents_with_insert_spaces_setting_3() {
        run_test(|f| {
            let language_id = f.setup_on_enter_language(IndentAction::IndentOutdent);
            f.using_cursor(
                CursorOpts { text: vec!["\thell()"], language_id: Some(language_id.as_str()), ..Default::default() },
                |editor, model, view_model| {
                    move_to(editor, view_model, 1, 7, false);
                    assert_cursor(view_model, Selection::new(1, 7, 1, 7));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_value_crlf(model), "\thell(\r\n        \r\n    )");
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_148256_pressing_enter_creates_line_with_bad_indent_with_insert_spaces_true() {
        run_test(|f| {
            f.using_cursor(CursorOpts { text: vec!["  \t"], ..Default::default() }, |editor, model, view_model| {
                move_to(editor, view_model, 1, 4, false);
                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_value(model), "  \t\n    ");
            });
        });
    }

    #[test]
    fn editor_controller_issue_148256_pressing_enter_creates_line_with_bad_indent_with_insert_spaces_false() {
        run_test(|f| {
            f.using_cursor(CursorOpts { text: vec!["  \t"], ..Default::default() }, |editor, model, view_model| {
                model.update_options(TextModelUpdateOptions { insert_spaces: Some(false), ..Default::default() });
                move_to(editor, view_model, 1, 4, false);
                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_value(model), "  \t\n\t");
            });
        });
    }

    #[test]
    fn editor_controller_remove_auto_whitespace_off() {
        run_test(|f| {
            f.using_cursor(
                CursorOpts {
                    text: vec!["    some  line abc  "],
                    model_opts: Some(RelaxedTextModelCreationOptions { trim_auto_whitespace: Some(false), ..Default::default() }),
                    ..Default::default()
                },
                |editor, model, view_model| {
                    // Move cursor to the end, verify that we do not trim whitespaces if line has values
                    move_to(editor, view_model, 1, line_max_column(model, 1), false);
                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "    some  line abc  ");
                    assert_eq!(get_line_content(model, 2), "    ");

                    // Try to enter again, we should trimmed previous line
                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "    some  line abc  ");
                    assert_eq!(get_line_content(model, 2), "    ");
                    assert_eq!(get_line_content(model, 3), "    ");
                },
            );
        });
    }

    #[test]
    fn editor_controller_remove_auto_whitespace_on_removes_only_whitespace_the_cursor_added_1() {
        run_test(|f| {
            f.using_cursor(CursorOpts { text: vec!["    "], ..Default::default() }, |editor, model, view_model| {
                move_to(editor, view_model, 1, line_max_column(model, 1), false);
                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "    ");
                assert_eq!(get_line_content(model, 2), "    ");

                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "    ");
                assert_eq!(get_line_content(model, 2), "");
                assert_eq!(get_line_content(model, 3), "    ");
            });
        });
    }

    #[test]
    fn editor_controller_issue_115033_indent_and_append_text() {
        run_test(|f| {
            let language_id = "onEnterMode";

            f.register_language(
                language_id,
                LanguageConfiguration {
                    on_enter_rules: Some(vec![OnEnterRule {
                        before_text: re(".*", ""),
                        after_text: None,
                        previous_line_text: None,
                        action: EnterAction {
                            indent_action: IndentAction::Indent,
                            append_text: Some("x".to_owned()),
                            remove_text: None,
                        },
                    }]),
                    ..LanguageConfiguration::default()
                },
            );
            f.using_cursor(
                CursorOpts { text: vec!["text"], language_id: Some(language_id), ..Default::default() },
                |editor, model, view_model| {
                    move_to(editor, view_model, 1, 5, false);
                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "text");
                    assert_eq!(get_line_content(model, 2), "    x");
                    assert_cursor(view_model, Position::new(2, 6));
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_6862_editor_removes_auto_inserted_indentation_when_formatting_on_type() {
        run_test(|f| {
            let language_id = f.setup_on_enter_language(IndentAction::IndentOutdent);
            f.using_cursor(
                CursorOpts {
                    text: vec!["function foo (params: string) {}"],
                    language_id: Some(language_id.as_str()),
                    ..Default::default()
                },
                |editor, model, view_model| {
                    move_to(editor, view_model, 1, 32, false);
                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "function foo (params: string) {");
                    assert_eq!(get_line_content(model, 2), "    ");
                    assert_eq!(get_line_content(model, 3), "}");

                    view_model.execute_command(
                        Box::new(AutoFormatTestCommand { selection: view_model.get_selection(), selection_id: None }),
                        Some("autoFormat"),
                    );
                    assert_eq!(get_line_content(model, 1), "function foo(params: string) {");
                    assert_eq!(get_line_content(model, 2), "    ");
                    assert_eq!(get_line_content(model, 3), "}");
                },
            );
        });
    }

    #[test]
    fn editor_controller_remove_auto_whitespace_on_removes_only_whitespace_the_cursor_added_2() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let language_id = "testLang";
            let registration = f.language_service.register_language(LanguageExtensionPoint::new(language_id));
            let model = f.create_text_model(
                ["    if (a) {", "        ", "", "", "    }"].join("\n"),
                Some(language_id),
                Default::default(),
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 3, 1, false);
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 1), "    if (a) {");
                assert_eq!(get_line_content(&model, 2), "        ");
                assert_eq!(get_line_content(&model, 3), "    ");
                assert_eq!(get_line_content(&model, 4), "");
                assert_eq!(get_line_content(&model, 5), "    }");

                move_to(editor, view_model, 4, 1, false);
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 1), "    if (a) {");
                assert_eq!(get_line_content(&model, 2), "        ");
                assert_eq!(get_line_content(&model, 3), "");
                assert_eq!(get_line_content(&model, 4), "    ");
                assert_eq!(get_line_content(&model, 5), "    }");

                move_to(editor, view_model, 5, line_max_column(&model, 5), false);
                type_text(view_model, "something", Some("keyboard"));
                assert_eq!(get_line_content(&model, 1), "    if (a) {");
                assert_eq!(get_line_content(&model, 2), "        ");
                assert_eq!(get_line_content(&model, 3), "");
                assert_eq!(get_line_content(&model, 4), "");
                assert_eq!(get_line_content(&model, 5), "    }something");
            });

            registration.dispose();
        });
    }

    #[test]
    fn editor_controller_remove_auto_whitespace_on_test_1() {
        run_test(|f| {
            let model = f.create_text_model(["    some  line abc  "].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                let editing = core_editing_commands::instance();

                // Move cursor to the end, verify that we do not trim whitespaces if line has values
                move_to(editor, view_model, 1, line_max_column(&model, 1), false);
                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_line_content(&model, 1), "    some  line abc  ");
                assert_eq!(get_line_content(&model, 2), "    ");

                // Try to enter again, we should trimmed previous line
                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_line_content(&model, 1), "    some  line abc  ");
                assert_eq!(get_line_content(&model, 2), "");
                assert_eq!(get_line_content(&model, 3), "    ");

                // More whitespaces
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 1), "    some  line abc  ");
                assert_eq!(get_line_content(&model, 2), "");
                assert_eq!(get_line_content(&model, 3), "        ");

                // Enter and verify that trimmed again
                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_line_content(&model, 1), "    some  line abc  ");
                assert_eq!(get_line_content(&model, 2), "");
                assert_eq!(get_line_content(&model, 3), "");
                assert_eq!(get_line_content(&model, 4), "        ");

                // Trimmed if we will keep only text
                move_to(editor, view_model, 1, 5, false);
                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_line_content(&model, 1), "    ");
                assert_eq!(get_line_content(&model, 2), "    some  line abc  ");
                assert_eq!(get_line_content(&model, 3), "");
                assert_eq!(get_line_content(&model, 4), "");
                assert_eq!(get_line_content(&model, 5), "");

                // Trimmed if we will keep only text by selection
                move_to(editor, view_model, 2, 5, false);
                move_to(editor, view_model, 3, 1, true);
                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_line_content(&model, 1), "    ");
                assert_eq!(get_line_content(&model, 2), "    ");
                assert_eq!(get_line_content(&model, 3), "    ");
                assert_eq!(get_line_content(&model, 4), "");
                assert_eq!(get_line_content(&model, 5), "");
            });
        });
    }

    #[test]
    fn editor_controller_issue_15118_remove_auto_whitespace_when_pasting_entire_line() {
        run_test(|f| {
            let model = f.create_text_model(
                ["    function f() {", "        // I'm gonna copy this line", "        return 3;", "    }"].join("\n"),
                None,
                Default::default(),
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 3, line_max_column(&model, 3), false);
                type_text(view_model, "\n", Some("keyboard"));

                assert_eq!(
                    get_value(&model),
                    [
                        "    function f() {",
                        "        // I'm gonna copy this line",
                        "        return 3;",
                        "        ",
                        "    }",
                    ]
                    .join("\n")
                );
                assert_cursor(view_model, Position::new(4, line_max_column(&model, 4)));

                view_model.paste(&u("        // I'm gonna copy this line\n"), true, None, None, false);
                assert_eq!(
                    get_value(&model),
                    [
                        "    function f() {",
                        "        // I'm gonna copy this line",
                        "        return 3;",
                        "        // I'm gonna copy this line",
                        "",
                        "    }",
                    ]
                    .join("\n")
                );
                assert_cursor(view_model, Position::new(5, 1));
            });
        });
    }

    #[test]
    fn editor_controller_issue_40695_maintain_cursor_position_when_copying_lines_using_ctrl_c_ctrl_v() {
        run_test(|f| {
            let model = f.create_text_model(
                [
                    "    function f() {",
                    "        // I'm gonna copy this line",
                    "        // Another line",
                    "        return 3;",
                    "    }",
                ]
                .join("\n"),
                None,
                Default::default(),
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                editor.set_selections(&[Selection::new(4, 10, 4, 10)], None, None);
                view_model.paste(&u("        // I'm gonna copy this line\n"), true, None, None, false);

                assert_eq!(
                    get_value(&model),
                    [
                        "    function f() {",
                        "        // I'm gonna copy this line",
                        "        // Another line",
                        "        // I'm gonna copy this line",
                        "        return 3;",
                        "    }",
                    ]
                    .join("\n")
                );
                assert_cursor(view_model, Position::new(5, 10));
            });
        });
    }

    #[test]
    fn editor_controller_use_tab_stops_is_off() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(["    x", "        a    ", "    "].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model.clone(), &use_tab_stops_options(false), |editor, view_model| {
                // DeleteLeft removes just one whitespace
                move_to(editor, view_model, 2, 9, false);
                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(&model, 2), "       a    ");
            });
        });
    }

    #[test]
    fn editor_controller_backspace_removes_whitespaces_with_tab_size() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model([" \t \t     x", "        a    ", "    "].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model.clone(), &use_tab_stops_options(true), |editor, view_model| {
                // DeleteLeft does not remove tab size, because some text exists before
                move_to(editor, view_model, 2, line_max_column(&model, 2), false);
                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(&model, 2), "        a   ");

                // DeleteLeft removes tab size = 4
                move_to(editor, view_model, 2, 9, false);
                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(&model, 2), "    a   ");

                // DeleteLeft removes tab size = 4
                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(&model, 2), "a   ");

                // Undo DeleteLeft - get us back to original indentation
                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(&model, 2), "        a   ");

                // Nothing is broken when cursor is in (1,1)
                move_to(editor, view_model, 1, 1, false);
                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(&model, 1), " \t \t     x");

                // DeleteLeft stops at tab stops even in mixed whitespace case
                move_to(editor, view_model, 1, 10, false);
                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(&model, 1), " \t \t    x");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(&model, 1), " \t \tx");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(&model, 1), " \tx");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(&model, 1), "x");

                // DeleteLeft on last line
                move_to(editor, view_model, 3, line_max_column(&model, 3), false);
                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(&model, 3), "");

                // DeleteLeft with removing new line symbol
                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "x\n        a   ");

                // In case of selection DeleteLeft only deletes selected text
                move_to(editor, view_model, 2, 3, false);
                move_to(editor, view_model, 2, 4, true);
                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(&model, 2), "       a   ");
            });
        });
    }

    #[test]
    fn editor_controller_pr_5423_auto_indent_undo_redo_is_funky() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model([""].join("\n"), None, tabs_model_options(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_value_lf(&model), "\n", "assert1");

                editor.run_command(&*editing.tab, None);
                assert_eq!(get_value_lf(&model), "\n\t", "assert2");

                type_text(view_model, "y", Some("keyboard"));
                assert_eq!(get_value_lf(&model), "\n\ty", "assert2");

                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_value_lf(&model), "\n\ty\n\t", "assert3");

                type_text(view_model, "x", None);
                assert_eq!(get_value_lf(&model), "\n\ty\n\tx", "assert4");

                move_left(editor, view_model, false);
                assert_eq!(get_value_lf(&model), "\n\ty\n\tx", "assert5");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "\n\ty\nx", "assert6");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "\n\tyx", "assert7");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "\n\tx", "assert8");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "\nx", "assert9");

                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_value_lf(&model), "x", "assert10");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value_lf(&model), "\nx", "assert11");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value_lf(&model), "\n\ty\nx", "assert12");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value_lf(&model), "\n\ty\n\tx", "assert13");

                editor.run_command(&*editing.redo, None);
                assert_eq!(get_value_lf(&model), "\n\ty\nx", "assert14");

                editor.run_command(&*editing.redo, None);
                assert_eq!(get_value_lf(&model), "\nx", "assert15");

                editor.run_command(&*editing.redo, None);
                assert_eq!(get_value_lf(&model), "x", "assert16");
            });
        });
    }

    #[test]
    fn editor_controller_issue_90973_undo_brings_back_model_alternative_version() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model([""].join("\n"), None, tabs_model_options(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                let before_version = model.get_version_id();
                let before_alt_version = model.get_alternative_version_id();
                type_text(view_model, "Hello", Some("keyboard"));
                editor.run_command(&*editing.undo, None);
                let after_version = model.get_version_id();
                let after_alt_version = model.get_alternative_version_id();

                assert_ne!(before_version, after_version);
                assert_eq!(before_alt_version, after_alt_version);
            });
        });
    }

    #[test]
    fn editor_controller_enter_honors_increase_indent_pattern() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["if (true) {", "\tif (true) {"],
                    Some(tabs_model_options()),
                    Some(auto_indent_options("full")),
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 1, 12, false);
                    assert_cursor(view_model, Selection::new(1, 12, 1, 12));

                    type_text(view_model, "\n", Some("keyboard"));
                    force_tokenization_of_all_lines(model);
                    assert_cursor(view_model, Selection::new(2, 2, 2, 2));

                    move_to(editor, view_model, 3, 13, false);
                    assert_cursor(view_model, Selection::new(3, 13, 3, 13));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(4, 3, 4, 3));
                },
            );
        });
    }

    #[test]
    fn editor_controller_type_honors_decrease_indent_pattern() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(vec!["if (true) {", "\t"], None, Some(auto_indent_options("full"))),
                |editor, model, view_model| {
                    move_to(editor, view_model, 2, 2, false);
                    assert_cursor(view_model, Selection::new(2, 2, 2, 2));

                    type_text(view_model, "}", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(2, 2, 2, 2));
                    assert_eq!(get_line_content(model, 2), "}", "001");
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_honors_un_indented_line_pattern() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["if (true) {", "\t\t\treturn true"],
                    Some(tabs_model_options()),
                    Some(auto_indent_options("full")),
                ),
                |editor, _model, view_model| {
                    move_to(editor, view_model, 2, 15, false);
                    assert_cursor(view_model, Selection::new(2, 15, 2, 15));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(3, 2, 3, 2));
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_honors_indent_next_line_pattern() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["if (true)", "\treturn true;", "if (true)", "\t\t\t\treturn true"],
                    Some(tabs_model_options()),
                    Some(auto_indent_options("full")),
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 2, 14, false);
                    assert_cursor(view_model, Selection::new(2, 14, 2, 14));

                    type_text(view_model, "\n", Some("keyboard"));
                    force_tokenization_of_all_lines(model);
                    assert_cursor(view_model, Selection::new(3, 1, 3, 1));

                    move_to(editor, view_model, 5, 16, false);
                    assert_cursor(view_model, Selection::new(5, 16, 5, 16));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(6, 2, 6, 2));
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_honors_indent_next_line_pattern_2() {
        run_test(|f| {
            let model = f.create_text_model(
                ["if (true)", "\tif (true)"].join("\n"),
                Some(INDENT_RULES_LANGUAGE_ID),
                tabs_model_options(),
                None,
            );

            f.with_test_code_editor(model.clone(), &auto_indent_options("full"), |editor, view_model| {
                move_to(editor, view_model, 2, 11, false);
                assert_cursor(view_model, Selection::new(2, 11, 2, 11));

                type_text(view_model, "\n", Some("keyboard"));
                force_tokenization_of_all_lines(&model);
                assert_cursor(view_model, Selection::new(3, 3, 3, 3));

                type_text(view_model, "console.log();", Some("keyboard"));
                type_text(view_model, "\n", Some("keyboard"));
                assert_cursor(view_model, Selection::new(4, 1, 4, 1));
            });
        });
    }

    #[test]
    fn editor_controller_enter_honors_intential_indent() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["if (true) {", "\tif (true) {", "return true;", "}}"],
                    None,
                    Some(auto_indent_options("full")),
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 3, 13, false);
                    assert_cursor(view_model, Selection::new(3, 13, 3, 13));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(4, 1, 4, 1));
                    assert_eq!(get_line_content(model, 3), "return true;", "001");
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_supports_selection_1() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["if (true) {", "\tif (true) {", "\t\treturn true;", "\t}a}"],
                    Some(tabs_model_options()),
                    None,
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 4, 3, false);
                    move_to(editor, view_model, 4, 4, true);
                    assert_cursor(view_model, Selection::new(4, 3, 4, 4));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(5, 1, 5, 1));
                    assert_eq!(get_line_content(model, 4), "\t}", "001");
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_supports_selection_2() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(vec!["if (true) {", "\tif (true) {"], Some(tabs_model_options()), None),
                |editor, _model, view_model| {
                    move_to(editor, view_model, 2, 12, false);
                    move_to(editor, view_model, 2, 13, true);
                    assert_cursor(view_model, Selection::new(2, 12, 2, 13));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(3, 3, 3, 3));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(4, 3, 4, 3));
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_honors_tab_size_and_insert_spaces_1() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(vec!["if (true) {", "\tif (true) {"], None, None),
                |editor, model, view_model| {
                    move_to(editor, view_model, 1, 12, false);
                    assert_cursor(view_model, Selection::new(1, 12, 1, 12));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(2, 5, 2, 5));

                    force_tokenization_of_all_lines(model);

                    move_to(editor, view_model, 3, 13, false);
                    assert_cursor(view_model, Selection::new(3, 13, 3, 13));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(4, 9, 4, 9));
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_honors_tab_size_and_insert_spaces_2() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(vec!["if (true) {", "    if (true) {"], None, None),
                |editor, model, view_model| {
                    move_to(editor, view_model, 1, 12, false);
                    assert_cursor(view_model, Selection::new(1, 12, 1, 12));

                    type_text(view_model, "\n", Some("keyboard"));
                    force_tokenization_of_all_lines(model);
                    assert_cursor(view_model, Selection::new(2, 5, 2, 5));

                    move_to(editor, view_model, 3, 16, false);
                    assert_cursor(view_model, Selection::new(3, 16, 3, 16));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_line_content(model, 3), "    if (true) {");
                    assert_cursor(view_model, Selection::new(4, 9, 4, 9));
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_honors_tab_size_and_insert_spaces_3() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(vec!["if (true) {", "    if (true) {"], Some(tabs_model_options()), None),
                |editor, model, view_model| {
                    move_to(editor, view_model, 1, 12, false);
                    assert_cursor(view_model, Selection::new(1, 12, 1, 12));

                    type_text(view_model, "\n", Some("keyboard"));
                    force_tokenization_of_all_lines(model);
                    assert_cursor(view_model, Selection::new(2, 2, 2, 2));

                    move_to(editor, view_model, 3, 16, false);
                    assert_cursor(view_model, Selection::new(3, 16, 3, 16));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_line_content(model, 3), "    if (true) {");
                    assert_cursor(view_model, Selection::new(4, 3, 4, 3));
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_supports_intentional_indentation() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["\tif (true) {", "\t\tswitch(true) {", "\t\t\tcase true:", "\t\t\t\tbreak;", "\t\t}", "\t}"],
                    Some(tabs_model_options()),
                    Some(auto_indent_options("full")),
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 5, 4, false);
                    assert_cursor(view_model, Selection::new(5, 4, 5, 4));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_line_content(model, 5), "\t\t}");
                    assert_cursor(view_model, Selection::new(6, 3, 6, 3));
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_should_not_adjust_cursor_position_when_press_enter_in_the_middle_of_a_line_1() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["if (true) {", "\tif (true) {", "\t\treturn true;", "\t}a}"],
                    Some(tabs_model_options()),
                    None,
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 3, 9, false);
                    assert_cursor(view_model, Selection::new(3, 9, 3, 9));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(4, 3, 4, 3));
                    assert_eq!(get_line_content(model, 4), "\t\t true;", "001");
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_should_not_adjust_cursor_position_when_press_enter_in_the_middle_of_a_line_2() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["if (true) {", "\tif (true) {", "\t\treturn true;", "\t}a}"],
                    Some(tabs_model_options()),
                    None,
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 3, 3, false);
                    assert_cursor(view_model, Selection::new(3, 3, 3, 3));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(4, 3, 4, 3));
                    assert_eq!(get_line_content(model, 4), "\t\treturn true;", "001");
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_should_not_adjust_cursor_position_when_press_enter_in_the_middle_of_a_line_3() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(vec!["if (true) {", "  if (true) {", "    return true;", "  }a}"], None, None),
                |editor, model, view_model| {
                    move_to(editor, view_model, 3, 11, false);
                    assert_cursor(view_model, Selection::new(3, 11, 3, 11));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(4, 5, 4, 5));
                    assert_eq!(get_line_content(model, 4), "     true;", "001");
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_should_adjust_cursor_position_when_press_enter_in_the_middle_of_leading_whitespaces_1() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["if (true) {", "\tif (true) {", "\t\treturn true;", "\t}a}"],
                    Some(tabs_model_options()),
                    None,
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 3, 2, false);
                    assert_cursor(view_model, Selection::new(3, 2, 3, 2));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(4, 2, 4, 2));
                    assert_eq!(get_line_content(model, 4), "\t\treturn true;", "001");

                    move_to(editor, view_model, 4, 1, false);
                    assert_cursor(view_model, Selection::new(4, 1, 4, 1));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(5, 1, 5, 1));
                    assert_eq!(get_line_content(model, 5), "\t\treturn true;", "002");
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_should_adjust_cursor_position_when_press_enter_in_the_middle_of_leading_whitespaces_2() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["\tif (true) {", "\t\tif (true) {", "\t    \treturn true;", "\t\t}a}"],
                    Some(tabs_model_options()),
                    None,
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 3, 4, false);
                    assert_cursor(view_model, Selection::new(3, 4, 3, 4));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(4, 3, 4, 3));
                    assert_eq!(get_line_content(model, 4), "\t\t\treturn true;", "001");

                    move_to(editor, view_model, 4, 1, false);
                    assert_cursor(view_model, Selection::new(4, 1, 4, 1));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(5, 1, 5, 1));
                    assert_eq!(get_line_content(model, 5), "\t\t\treturn true;", "002");
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_should_adjust_cursor_position_when_press_enter_in_the_middle_of_leading_whitespaces_3() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(vec!["if (true) {", "  if (true) {", "    return true;", "}a}"], None, None),
                |editor, model, view_model| {
                    move_to(editor, view_model, 3, 2, false);
                    assert_cursor(view_model, Selection::new(3, 2, 3, 2));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(4, 2, 4, 2));
                    assert_eq!(get_line_content(model, 4), "    return true;", "001");

                    move_to(editor, view_model, 4, 3, false);
                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(5, 3, 5, 3));
                    assert_eq!(get_line_content(model, 5), "    return true;", "002");
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_should_adjust_cursor_position_when_press_enter_in_the_middle_of_leading_whitespaces_4() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec![
                        "if (true) {",
                        "  if (true) {",
                        "\t  return true;",
                        "}a}",
                        "",
                        "if (true) {",
                        "  if (true) {",
                        "\t  return true;",
                        "}a}",
                    ],
                    Some(RelaxedTextModelCreationOptions {
                        tab_size: Some(2),
                        indent_size: Some(IndentSize::Size(2)),
                        ..Default::default()
                    }),
                    None,
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 3, 3, false);
                    assert_cursor(view_model, Selection::new(3, 3, 3, 3));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(4, 4, 4, 4));
                    assert_eq!(get_line_content(model, 4), "    return true;", "001");

                    move_to(editor, view_model, 9, 4, false);
                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(10, 5, 10, 5));
                    assert_eq!(get_line_content(model, 10), "    return true;", "001");
                },
            );
        });
    }

    #[test]
    fn editor_controller_enter_should_adjust_cursor_position_when_press_enter_in_the_middle_of_leading_whitespaces_5() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["if (true) {", "  if (true) {", "    return true;", "    return true;", ""],
                    Some(RelaxedTextModelCreationOptions { tab_size: Some(2), ..Default::default() }),
                    None,
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 3, 5, false);
                    move_to(editor, view_model, 4, 3, true);
                    assert_cursor(view_model, Selection::new(3, 5, 4, 3));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(4, 3, 4, 3));
                    assert_eq!(get_line_content(model, 4), "    return true;", "001");
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_microsoft_monaco_editor_108_part_1_2_auto_indentation_on_enter_with_selection_is_half_broken() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["function baz() {", "\tvar x = 1;", "\t\t\t\t\t\t\treturn x;", "}"],
                    Some(tabs_model_options()),
                    None,
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 3, 8, false);
                    move_to(editor, view_model, 2, 12, true);
                    assert_cursor(view_model, Selection::new(3, 8, 2, 12));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_line_content(model, 3), "\treturn x;");
                    assert_cursor(view_model, Position::new(3, 2));
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_microsoft_monaco_editor_108_part_2_2_auto_indentation_on_enter_with_selection_is_half_broken() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["function baz() {", "\tvar x = 1;", "\t\t\t\t\t\t\treturn x;", "}"],
                    Some(tabs_model_options()),
                    None,
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 2, 12, false);
                    move_to(editor, view_model, 3, 8, true);
                    assert_cursor(view_model, Selection::new(2, 12, 3, 8));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_line_content(model, 3), "\treturn x;");
                    assert_cursor(view_model, Position::new(3, 2));
                },
            );
        });
    }

    #[test]
    fn editor_controller_on_enter_works_if_there_are_no_indentation_rules() {
        run_test(|f| {
            f.using_cursor(
                CursorOpts {
                    text: vec!["<?", "\tif (true) {", "\t\techo $hi;", "\t\techo $bye;", "\t}", "?>"],
                    model_opts: Some(tabs_model_options()),
                    ..Default::default()
                },
                |editor, model, view_model| {
                    move_to(editor, view_model, 5, 3, false);
                    assert_cursor(view_model, Selection::new(5, 3, 5, 3));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_eq!(get_line_content(model, 6), "\t");
                    assert_cursor(view_model, Selection::new(6, 2, 6, 2));
                    assert_eq!(get_line_content(model, 5), "\t}");
                },
            );
        });
    }

    #[test]
    fn editor_controller_on_enter_works_if_there_are_no_indentation_rules_2() {
        run_test(|f| {
            f.using_cursor(
                CursorOpts {
                    text: vec!["\tif (5)", "\t\treturn 5;", "\t"],
                    model_opts: Some(tabs_model_options()),
                    ..Default::default()
                },
                |editor, model, view_model| {
                    move_to(editor, view_model, 3, 2, false);
                    assert_cursor(view_model, Selection::new(3, 2, 3, 2));

                    type_text(view_model, "\n", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(4, 2, 4, 2));
                    assert_eq!(get_line_content(model, 4), "\t");
                },
            );
        });
    }

    #[test]
    fn editor_controller_bug_16543_tab_should_indent_to_correct_indentation_spot_immediately() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(
                ["function baz() {", "\tfunction hello() { // something here", "\t", "", "\t}", "}"].join("\n"),
                Some(INDENT_RULES_LANGUAGE_ID),
                tabs_model_options(),
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 4, 1, false);
                assert_cursor(view_model, Selection::new(4, 1, 4, 1));

                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 4), "\t\t");
            });
        });
    }

    #[test]
    fn editor_controller_bug_2938_1_when_pressing_tab_on_white_space_only_lines_indent_straight_to_the_right_spot() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(
                ["\tfunction baz() {", "\t\tfunction hello() { // something here", "\t\t", "\t", "\t\t}", "\t}"].join("\n"),
                Some(INDENT_RULES_LANGUAGE_ID),
                tabs_model_options(),
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 4, 2, false);
                assert_cursor(view_model, Selection::new(4, 2, 4, 2));

                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 4), "\t\t\t");
            });
        });
    }

    #[test]
    fn editor_controller_bug_2938_2_when_pressing_tab_on_white_space_only_lines_indent_straight_to_the_right_spot() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(
                ["\tfunction baz() {", "\t\tfunction hello() { // something here", "\t\t", "    ", "\t\t}", "\t}"].join("\n"),
                Some(INDENT_RULES_LANGUAGE_ID),
                tabs_model_options(),
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 4, 1, false);
                assert_cursor(view_model, Selection::new(4, 1, 4, 1));

                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 4), "\t\t\t");
            });
        });
    }

    #[test]
    fn editor_controller_bug_2938_3_when_pressing_tab_on_white_space_only_lines_indent_straight_to_the_right_spot() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(
                ["\tfunction baz() {", "\t\tfunction hello() { // something here", "\t\t", "\t\t\t", "\t\t}", "\t}"].join("\n"),
                Some(INDENT_RULES_LANGUAGE_ID),
                tabs_model_options(),
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 4, 3, false);
                assert_cursor(view_model, Selection::new(4, 3, 4, 3));

                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 4), "\t\t\t\t");
            });
        });
    }

    #[test]
    fn editor_controller_bug_2938_4_when_pressing_tab_on_white_space_only_lines_indent_straight_to_the_right_spot() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(
                ["\tfunction baz() {", "\t\tfunction hello() { // something here", "\t\t", "\t\t\t\t", "\t\t}", "\t}"].join("\n"),
                Some(INDENT_RULES_LANGUAGE_ID),
                tabs_model_options(),
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 4, 4, false);
                assert_cursor(view_model, Selection::new(4, 4, 4, 4));

                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 4), "\t\t\t\t\t");
            });
        });
    }

    #[test]
    fn editor_controller_bug_31015_when_pressing_tab_on_lines_and_enter_rules_are_avail_indent_straight_to_the_right_spot_tab() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let on_enter_language_id = f.setup_on_enter_language(IndentAction::Indent);
            let model = f.create_text_model(
                ["    if (a) {", "        ", "", "", "    }"].join("\n"),
                Some(on_enter_language_id.as_str()),
                Default::default(),
                None,
            );

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                move_to(editor, view_model, 3, 1, false);
                editor.run_command(&*editing.tab, None);
                assert_eq!(get_line_content(&model, 1), "    if (a) {");
                assert_eq!(get_line_content(&model, 2), "        ");
                assert_eq!(get_line_content(&model, 3), "        ");
                assert_eq!(get_line_content(&model, 4), "");
                assert_eq!(get_line_content(&model, 5), "    }");
            });
        });
    }

    #[test]
    fn editor_controller_type_honors_indentation_rules_ruby_keywords() {
        run_test(|f| {
            let ruby_language_id = f.setup_indent_rules_language(
                "ruby",
                decrease_increase_rules(
                    r##"^\s*([}\]]([,)]?\s*(#|$)|\.[a-zA-Z_]\w*\b)|(end|rescue|ensure|else|elsif|when)\b)"##,
                    r##"^\s*((begin|class|def|else|elsif|ensure|for|if|module|rescue|unless|until|when|while)|(.*\sdo\b))\b[^\{;]*$"##,
                    "",
                ),
            );
            let model = f.create_text_model(
                ["class Greeter", "  def initialize(name)", "    @name = name", "    en"].join("\n"),
                Some(ruby_language_id.as_str()),
                Default::default(),
                None,
            );

            f.with_test_code_editor(model.clone(), &auto_indent_options("full"), |editor, view_model| {
                move_to(editor, view_model, 4, 7, false);
                assert_cursor(view_model, Selection::new(4, 7, 4, 7));

                type_text(view_model, "d", Some("keyboard"));
                assert_eq!(get_line_content(&model, 4), "  end");
            });
        });
    }

    #[test]
    fn editor_controller_auto_indent_on_type_increase_indent_pattern_has_higher_priority_than_decrease_indent_when_inheriting() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(
                    vec!["\tif (true) {", "\t\tconsole.log();", "\t} else if {", "\t\tconsole.log()", "\t}"],
                    None,
                    None,
                ),
                |editor, model, view_model| {
                    move_to(editor, view_model, 5, 3, false);
                    assert_cursor(view_model, Selection::new(5, 3, 5, 3));

                    type_text(view_model, "e", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(5, 4, 5, 4));
                    assert_eq!(get_line_content(model, 5), "\t}e", "This line should not decrease indent");
                },
            );
        });
    }

    #[test]
    fn editor_controller_type_honors_users_indentation_adjustment() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(vec!["\tif (true ||", "\t ) {", "\t}", "if (true ||", ") {", "}"], None, None),
                |editor, model, view_model| {
                    move_to(editor, view_model, 2, 3, false);
                    assert_cursor(view_model, Selection::new(2, 3, 2, 3));

                    type_text(view_model, " ", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(2, 4, 2, 4));
                    assert_eq!(get_line_content(model, 2), "\t  ) {", "This line should not decrease indent");
                },
            );
        });
    }

    #[test]
    fn editor_controller_bug_29972_if_a_line_is_line_comment_open_bracket_should_not_indent_next_line() {
        run_test(|f| {
            f.using_cursor(
                indent_rules_cursor_opts(vec!["if (true) {", "\t// {", "\t\t"], None, Some(auto_indent_options("full"))),
                |editor, model, view_model| {
                    move_to(editor, view_model, 3, 3, false);
                    assert_cursor(view_model, Selection::new(3, 3, 3, 3));

                    type_text(view_model, "}", Some("keyboard"));
                    assert_cursor(view_model, Selection::new(3, 2, 3, 2));
                    assert_eq!(get_line_content(model, 3), "}");
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_38261_tab_key_results_in_bizarre_indentation_in_cpp_mode() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let language_id = "indentRulesMode";

            f.register_language(
                language_id,
                LanguageConfiguration {
                    brackets: Some(vec![
                        ("{".to_owned(), "}".to_owned()),
                        ("[".to_owned(), "]".to_owned()),
                        ("(".to_owned(), ")".to_owned()),
                    ]),
                    indentation_rules: Some(decrease_increase_rules(r"^\s*\}", r"(^.*\{[^}]*$)", "")),
                    ..LanguageConfiguration::default()
                },
            );

            let model = f.create_text_model(
                [
                    "int main() {",
                    "  return 0;",
                    "}",
                    "",
                    "bool Foo::bar(const string &a,",
                    "              const string &b) {",
                    "  foo();",
                    "",
                    ")",
                ]
                .join("\n"),
                Some(language_id),
                RelaxedTextModelCreationOptions { tab_size: Some(2), indent_size: Some(IndentSize::Size(2)), ..Default::default() },
                None,
            );

            f.with_test_code_editor(model.clone(), &auto_indent_options("advanced"), |editor, view_model| {
                move_to(editor, view_model, 8, 1, false);
                assert_cursor(view_model, Selection::new(8, 1, 8, 1));

                editor.run_command(&*editing.tab, None);
                assert_eq!(
                    get_value(&model),
                    [
                        "int main() {",
                        "  return 0;",
                        "}",
                        "",
                        "bool Foo::bar(const string &a,",
                        "              const string &b) {",
                        "  foo();",
                        "  ",
                        ")",
                    ]
                    .join("\n")
                );
                assert_eq!(view_model.get_selection(), Selection::new(8, 3, 8, 3));
            });
        });
    }

}

#[allow(unused_imports)]
mod part_d {
    use super::*;
use je_editor::view_model_types::IViewModel as _;
use std::cell::Cell;
use std::rc::Rc;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable as _;
use je_editor::code_editor_widget::{ExecuteEditsSource, ICodeEditor};
use je_editor::core_commands::core_editing_commands;
use je_editor::core_commands::core_navigation_commands;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::language_configuration::{AutoClosingPair, IndentAction, LanguageConfiguration};
use je_editor::languages::{EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult};
use je_editor::model::{EndOfLineSequence, RelaxedTextModelCreationOptions, ValidEditOperation};
use je_editor::null_tokenize::null_state;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::tokenization_registry::tokenization_registry;
use support::test_view_model::no_options;
use support::u;
use super::part_b::editor_controller::*;

    // imports do fragmento D:
    use je_editor::language_configuration_registry::ILanguageConfigurationService as _;
    use std::cell::{RefCell};

    use je_base::lifecycle::IDisposable as _;
    use je_editor::code_editor_widget::{EditorViewModel, EndCursorState};
    use je_editor::language_configuration::{AutoClosingPairConditional, IndentationRule};
    use je_editor::model::{IndentSize};
    use je_editor::text_model::TextModel;
    use je_editor::text_model_edit_source::EditSources;
    use support::re;
    use support::test_code_editor::TestCodeEditor;

    // Casos da suíte `Editor Controller` (terceiro trecho: das regras de indentação aos auto-fechamentos)

    /// As opções do editor cujos valores são todos texto (`{ autoIndent: 'full' }` e parecidos).
    fn string_options(options: &[(&str, &str)]) -> Dynamic {
        Dynamic::object(options.iter().map(|(key, value)| (*key, Dynamic::from(*value))))
    }

    /// `new IndentationRule` com os dois padrões obrigatórios e a mesma `flags` nos dois.
    fn indentation_rules(decrease_indent_pattern: &str, increase_indent_pattern: &str, flags: &str) -> IndentationRule {
        IndentationRule {
            decrease_indent_pattern: re(decrease_indent_pattern, flags),
            increase_indent_pattern: re(increase_indent_pattern, flags),
            indent_next_line_pattern: None,
            un_indented_line_pattern: None,
        }
    }

    /// O par de colchetes `['{', '}']` do `brackets` de uma linguagem.
    fn bracket_pair(open: &str, close: &str) -> (String, String) {
        (open.to_owned(), close.to_owned())
    }

    /// O corpo comum dos casos `ElectricCharacter` simples: o cursor vai para `(line_number, column)`, digita `typed` e a
    /// linha `line_number` passa a ser `expected`.
    fn assert_electric_typing(
        f: &EditorControllerFixture,
        text: Vec<&str>,
        line_number: i32,
        column: i32,
        typed: &str,
        expected: &str,
    ) {
        f.using_cursor(
            CursorOpts { text, language_id: Some(ELECTRIC_CHAR_LANGUAGE_ID), ..Default::default() },
            |editor, model, view_model| {
                move_to(editor, view_model, line_number, column, false);
                type_text(view_model, typed, Some("keyboard"));
                assert_eq!(get_line_content(model, line_number), expected);
            },
        );
    }

    /// O `{ text, languageId: autoClosingLanguageId }` que a maioria dos casos de auto-fechamento usa.
    fn auto_closing_opts(text: Vec<&str>) -> CursorOpts<'_> {
        CursorOpts { text, language_id: Some(AUTO_CLOSING_LANGUAGE_ID), ..Default::default() }
    }

    /// As oito linhas de amostra dos casos `autoClosingPairs - ...`.
    const AUTO_CLOSING_SAMPLE_LINES: [&str; 8] = [
        "var a = [];",
        "var b = `asd`;",
        "var c = 'asd';",
        "var d = \"asd\";",
        "var e = /*3*/\t3;",
        "var f = /** 3 */3;",
        "var g = (3+5);",
        "var h = { a: 'value' };",
    ];

    /// O laço comum dos casos `autoClosingPairs`: para cada linha anotada e cada coluna, força a tokenização da linha e entrega
    /// `(linha, coluna, tipo da coluna)` a `check`.
    fn for_each_auto_closing_column(
        model: &TextModel,
        annotated_lines: &[&str],
        mut check: impl FnMut(i32, i32, AutoClosingColumnType),
    ) {
        for (index, annotated_line) in annotated_lines.iter().enumerate() {
            let line_number = index as i32 + 1;
            let auto_close_columns =
                extract_auto_closing_special_columns(CursorSimpleModel::get_line_max_column(model, line_number), annotated_line);

            for (column, column_type) in auto_close_columns.iter().enumerate().skip(1) {
                model.tokenization().force_tokenization(line_number);
                check(line_number, column as i32, *column_type);
            }
        }
    }

    /// O `if (Special1) assertType(..., closed, 'auto closes ...') else assertType(..., chr, 'does not auto close ...')` dos
    /// casos `autoClosingPairs`.
    #[allow(clippy::too_many_arguments)]
    fn assert_auto_close(
        editor: &TestCodeEditor,
        model: &TextModel,
        view_model: &EditorViewModel,
        line_number: i32,
        column: i32,
        chr: &str,
        closes: bool,
        closed: &str,
    ) {
        if closes {
            assert_type(editor, model, view_model, line_number, column, chr, closed, &format!("auto closes @ ({line_number}, {column})"));
        } else {
            assert_type(editor, model, view_model, line_number, column, chr, chr, &format!("does not auto close @ ({line_number}, {column})"));
        }
    }

    /// `typeCharacters(viewModel, chars)`: um `type` por caractere.
    fn type_characters(view_model: &EditorViewModel, chars: &str) {
        for chr in chars.chars() {
            type_text(view_model, &chr.to_string(), Some("keyboard"));
        }
    }

    /// `model.setValue(text)` seguido de `viewModel.setSelections('test', [selection])`, o preparo que vários casos repetem.
    fn reset_model_with_selection(model: &TextModel, view_model: &EditorViewModel, text: &str, selection: Selection) {
        model.set_value(text.into());
        set_test_selections(view_model, &[selection]);
    }

    /// `[model.getLineContent(lineNumber), viewModel.getSelections().map(s => s.toString())]`.
    fn line_and_selections(model: &TextModel, view_model: &EditorViewModel, line_number: i32) -> (String, Vec<String>) {
        (get_line_content(model, line_number), view_model.get_selections().iter().map(ToString::to_string).collect())
    }

    /// O vetor esperado de `line_and_selections`.
    fn expected_states(states: &[(&str, &[&str])]) -> Vec<(String, Vec<String>)> {
        states
            .iter()
            .map(|(line, selections)| ((*line).to_owned(), selections.iter().map(|selection| (*selection).to_owned()).collect()))
            .collect()
    }

    /// `viewModel.startComposition()`, `type(typed)`, um `compositionType(text, replacePrevCharCnt, 0, 0)` para cada par de
    /// `compositions` e `endComposition()`, todos com a fonte `keyboard`.
    fn type_with_composition(view_model: &EditorViewModel, typed: &str, compositions: &[(&str, i32)]) {
        view_model.start_composition();
        type_text(view_model, typed, Some("keyboard"));
        for (text, replace_prev_char_count) in compositions {
            view_model.composition_type(&u(text), *replace_prev_char_count, 0, 0, Some("keyboard"));
        }
        view_model.end_composition(Some("keyboard"));
    }

    /// `editor.trigger('keyboard', 'type', { text })`.
    fn trigger_type(editor: &TestCodeEditor, text: &str) {
        editor.trigger(Some("keyboard"), "type", Some(Dynamic::object([("text", Dynamic::from(text))])));
    }

    /// A mesma lista de pares de aspas com prefixo (`r"`, `R"`, `u"`...) que a linguagem Python do caso `issue #85983` repete
    /// para `"` e para `'`.
    fn python_prefixed_quote_pairs(quote: &str) -> Vec<AutoClosingPairConditional> {
        ["r", "R", "u", "U", "f", "F", "b", "B"]
            .iter()
            .map(|prefix| AutoClosingPairConditional::new(&format!("{prefix}{quote}"), quote).not_in(&["string", "comment"]))
            .collect()
    }

    #[test]
    fn editor_controller_issue_57197_indent_rules_regex_should_be_stateless() {
        run_test(|f| {
            // Os marcadores `<EN>` e `<EM>` do padrão viram os traços médio (U+2013) e longo (U+2014) do original, montados
            // por código para o fonte não carregar esses caracteres.
            let en_dash = char::from_u32(0x2013).expect("U+2013 é um caractere válido").to_string();
            let em_dash = char::from_u32(0x2014).expect("U+2014 é um caractere válido").to_string();
            let increase_indent_pattern = r"^(?![^\S\n]*(?!--|<EN><EN>|<EM><EM>)(?:[-❍❑■⬜□☐▪▫<EN><EM>≡→›✘xX✔✓☑+]|\[[ xX+-]?\])\s[^\n]*)[^\S\n]*(.+:)[^\S\n]*(?:(?=@[^\s*~(]+(?::\/\/[^\s*~(:]+)?(?:\([^)]*\))?)|$)"
                .replace("<EN>", &en_dash)
                .replace("<EM>", &em_dash);
            let language_id =
                f.setup_indent_rules_language("lang", indentation_rules(r"^\s*}$", &increase_indent_pattern, "gm"));
            f.using_cursor(
                CursorOpts {
                    text: vec!["Project:"],
                    language_id: Some(language_id.as_str()),
                    model_opts: Some(RelaxedTextModelCreationOptions { insert_spaces: Some(false), ..Default::default() }),
                    editor_opts: Some(string_options(&[("autoIndent", "full")])),
                },
                |editor, model, view_model| {
                    move_to(editor, view_model, 1, 9, false);
                    assert_cursor(view_model, Selection::new(1, 9, 1, 9));

                    type_text(view_model, "\n", Some("keyboard"));
                    model.tokenization().force_tokenization(model.get_line_count());
                    assert_cursor(view_model, Selection::new(2, 2, 2, 2));

                    move_to(editor, view_model, 1, 9, false);
                    assert_cursor(view_model, Selection::new(1, 9, 1, 9));
                    type_text(view_model, "\n", Some("keyboard"));
                    model.tokenization().force_tokenization(model.get_line_count());
                    assert_cursor(view_model, Selection::new(2, 2, 2, 2));
                },
            );
        });
    }

    #[test]
    fn editor_controller_typing_in_json() {
        run_test(|f| {
            let language_id = "indentRulesMode";

            f.register_language(
                language_id,
                LanguageConfiguration {
                    brackets: Some(vec![bracket_pair("{", "}"), bracket_pair("[", "]"), bracket_pair("(", ")")]),
                    indentation_rules: Some(IndentationRule {
                        increase_indent_pattern: re(r##"({+(?=([^"]*"[^"]*")*[^"}]*$))|(\[+(?=([^"]*"[^"]*")*[^"\]]*$))"##, ""),
                        decrease_indent_pattern: re(r"^\s*[}\]],?\s*$", ""),
                        indent_next_line_pattern: None,
                        un_indented_line_pattern: None,
                    }),
                    ..LanguageConfiguration::default()
                },
            );

            let model = f.create_text_model(
                [
                    "{",
                    "  \"scripts: {\"",
                    "    \"watch\": \"a {\"",
                    "    \"build{\": \"b\"",
                    "    \"tasks\": []",
                    "    \"tasks\": [\"a\"]",
                    "  \"}\"",
                    "\"}\"",
                ]
                .join("\n"),
                Some(language_id),
                RelaxedTextModelCreationOptions { tab_size: Some(2), indent_size: Some(IndentSize::Size(2)), ..Default::default() },
                None,
            );

            f.with_test_code_editor(model.clone(), &string_options(&[("autoIndent", "full")]), |editor, view_model| {
                move_to(editor, view_model, 3, 19, false);
                assert_cursor(view_model, Selection::new(3, 19, 3, 19));

                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_line_content(&model, 4), "    ");

                move_to(editor, view_model, 5, 18, false);
                assert_cursor(view_model, Selection::new(5, 18, 5, 18));

                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_line_content(&model, 6), "    ");

                move_to(editor, view_model, 7, 15, false);
                assert_cursor(view_model, Selection::new(7, 15, 7, 15));

                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_line_content(&model, 8), "      ");
                assert_eq!(get_line_content(&model, 9), "    ]");

                move_to(editor, view_model, 10, 18, false);
                assert_cursor(view_model, Selection::new(10, 18, 10, 18));

                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_line_content(&model, 11), "    ]");
            });
        });
    }

    #[test]
    fn editor_controller_issue_111128_multicursor_enter_issue_with_indentation() {
        run_test(|f| {
            let model = f.create_text_model(
                "    let a, b, c;",
                Some(INDENT_RULES_LANGUAGE_ID),
                RelaxedTextModelCreationOptions {
                    detect_indentation: Some(false),
                    insert_spaces: Some(false),
                    tab_size: Some(4),
                    ..Default::default()
                },
                None,
            );
            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                editor.set_selections(&[Selection::new(1, 11, 1, 11), Selection::new(1, 14, 1, 14)], None, None);
                type_text(view_model, "\n", Some("keyboard"));
                assert_eq!(get_value(&model), "    let a,\n\t b,\n\t c;");
            });
        });
    }

    #[test]
    fn editor_controller_issue_122714_tab_size_1_prevent_typing_a_string_matching_decrease_indent_pattern_in_an_empty_file() {
        run_test(|f| {
            let latex_language_id = f.setup_indent_rules_language(
                "latex",
                indentation_rules(r"^\s*\\end{(?!document)", r"\\begin{(?!document)([^}]*)}(?!.*\\end{\1})", ""),
            );
            let model = f.create_text_model(
                "\\end",
                Some(latex_language_id.as_str()),
                RelaxedTextModelCreationOptions { tab_size: Some(1), ..Default::default() },
                None,
            );

            f.with_test_code_editor(model.clone(), &string_options(&[("autoIndent", "full")]), |editor, view_model| {
                move_to(editor, view_model, 1, 5, false);
                assert_cursor(view_model, Selection::new(1, 5, 1, 5));

                type_text(view_model, "{", Some("keyboard"));
                assert_eq!(get_line_content(&model, 1), "\\end{}");
            });
        });
    }

    #[test]
    fn editor_controller_electric_character_does_nothing_if_no_electric_char() {
        run_test(|f| assert_electric_typing(f, vec!["  if (a) {", ""], 2, 1, "*", "*"));
    }

    #[test]
    fn editor_controller_electric_character_indents_in_order_to_match_bracket() {
        run_test(|f| assert_electric_typing(f, vec!["  if (a) {", ""], 2, 1, "}", "  }"));
    }

    #[test]
    fn editor_controller_electric_character_unindents_in_order_to_match_bracket() {
        run_test(|f| assert_electric_typing(f, vec!["  if (a) {", "    "], 2, 5, "}", "  }"));
    }

    #[test]
    fn editor_controller_electric_character_matches_with_correct_bracket() {
        run_test(|f| assert_electric_typing(f, vec!["  if (a) {", "    if (b) {", "    }", "    "], 4, 1, "}", "  }    "));
    }

    #[test]
    fn editor_controller_electric_character_does_nothing_if_bracket_does_not_match() {
        run_test(|f| assert_electric_typing(f, vec!["  if (a) {", "    if (b) {", "    }", "  }  "], 4, 6, "}", "  }  }"));
    }

    #[test]
    fn editor_controller_electric_character_matches_bracket_even_in_line_with_content() {
        run_test(|f| assert_electric_typing(f, vec!["  if (a) {", "// hello"], 2, 1, "}", "  }// hello"));
    }

    #[test]
    fn editor_controller_electric_character_is_no_op_if_bracket_is_lined_up() {
        run_test(|f| assert_electric_typing(f, vec!["  if (a) {", "  "], 2, 3, "}", "  }"));
    }

    #[test]
    fn editor_controller_electric_character_is_no_op_if_there_is_non_whitespace_text_before() {
        run_test(|f| assert_electric_typing(f, vec!["  if (a) {", "a"], 2, 2, "}", "a}"));
    }

    #[test]
    fn editor_controller_electric_character_is_no_op_if_pairs_are_all_matched_before() {
        run_test(|f| assert_electric_typing(f, vec!["foo(() => {", "  ( 1 + 2 ) ", "})"], 2, 13, "*", "  ( 1 + 2 ) *"));
    }

    #[test]
    fn editor_controller_electric_character_is_no_op_if_matching_bracket_is_on_the_same_line() {
        run_test(|f| {
            f.using_cursor(
                CursorOpts { text: vec!["(div"], language_id: Some(ELECTRIC_CHAR_LANGUAGE_ID), ..Default::default() },
                |editor, model, view_model| {
                    move_to(editor, view_model, 1, 5, false);
                    let change_text: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
                    let listener_change_text = change_text.clone();
                    let disposable = model.on_did_change_content().listen(move |e| {
                        *listener_change_text.borrow_mut() = Some(e.changes[0].text.to_string_lossy());
                    });
                    type_text(view_model, ")", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "(div)");
                    assert_eq!(*change_text.borrow(), Some(")".to_owned()));
                    disposable.dispose();
                },
            );
        });
    }

    #[test]
    fn editor_controller_electric_character_is_no_op_if_the_line_has_other_content() {
        run_test(|f| assert_electric_typing(f, vec!["Math.max(", "\t2", "\t3"], 3, 3, ")", "\t3)"));
    }

    #[test]
    fn editor_controller_electric_character_appends_text() {
        run_test(|f| assert_electric_typing(f, vec!["  if (a) {", "/*"], 2, 3, "*", "/** */"));
    }

    #[test]
    fn editor_controller_electric_character_appends_text_2() {
        run_test(|f| assert_electric_typing(f, vec!["  if (a) {", "  /*"], 2, 5, "*", "  /** */"));
    }

    #[test]
    fn editor_controller_electric_character_issue_23711_replacing_selected_text_with_closing_bracket_fails_to_delete_old_text_with_backwards_dragged_selection() {
        run_test(|f| {
            f.using_cursor(
                CursorOpts { text: vec!["{", "word"], language_id: Some(ELECTRIC_CHAR_LANGUAGE_ID), ..Default::default() },
                |editor, model, view_model| {
                    move_to(editor, view_model, 2, 5, false);
                    move_to(editor, view_model, 2, 1, true);
                    type_text(view_model, "}", Some("keyboard"));
                    assert_eq!(get_line_content(model, 2), "}");
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_61070_backtick_should_auto_close_after_a_word_character() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["const markup = highlight"]), |editor, model, view_model| {
                model.tokenization().force_tokenization(1);
                assert_type(editor, model, view_model, 1, 25, "`", "``", "auto closes ` @ (1, 25)");
            });
        });
    }

    #[test]
    fn editor_controller_issue_132912_quotes_should_not_auto_close_if_they_are_closing_a_string() {
        run_test(|f| {
            f.setup_auto_closing_language_tokenization();
            let model = f.create_text_model(
                "const t2 = `something ${t1}",
                Some(AUTO_CLOSING_LANGUAGE_ID),
                Default::default(),
                None,
            );
            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                model.tokenization().force_tokenization(1);
                assert_type(editor, &model, view_model, 1, 28, "`", "`", "does not auto close ` @ (1, 28)");
            });
        });
    }

    #[test]
    fn editor_controller_auto_closing_pairs_open_parens_default() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(AUTO_CLOSING_SAMPLE_LINES.to_vec()), |editor, model, view_model| {
                let auto_close_positions = [
                    "var| a| |=| [|]|;|",
                    "var| b| |=| |`asd|`|;|",
                    "var| c| |=| |'asd|'|;|",
                    "var| d| |=| |\"asd|\"|;|",
                    "var| e| |=| /*3*/|\t3|;|",
                    "var| f| |=| /**| 3| */3|;|",
                    "var| g| |=| (3+5|)|;|",
                    "var| h| |=| {| a|:| |'value|'| |}|;|",
                ];
                for_each_auto_closing_column(model, &auto_close_positions, |line_number, column, column_type| {
                    assert_auto_close(
                        editor,
                        model,
                        view_model,
                        line_number,
                        column,
                        "(",
                        column_type == AutoClosingColumnType::Special1,
                        "()",
                    );
                });
            });
        });
    }

    #[test]
    fn editor_controller_auto_closing_pairs_open_parens_whitespace() {
        run_test(|f| {
            f.using_cursor(
                CursorOpts {
                    editor_opts: Some(string_options(&[("autoClosingBrackets", "beforeWhitespace")])),
                    ..auto_closing_opts(AUTO_CLOSING_SAMPLE_LINES.to_vec())
                },
                |editor, model, view_model| {
                    let auto_close_positions = [
                        "var| a| =| [|];|",
                        "var| b| =| `asd`;|",
                        "var| c| =| 'asd';|",
                        "var| d| =| \"asd\";|",
                        "var| e| =| /*3*/|\t3;|",
                        "var| f| =| /**| 3| */3;|",
                        "var| g| =| (3+5|);|",
                        "var| h| =| {| a:| 'value'| |};|",
                    ];
                    for_each_auto_closing_column(model, &auto_close_positions, |line_number, column, column_type| {
                        assert_auto_close(
                            editor,
                            model,
                            view_model,
                            line_number,
                            column,
                            "(",
                            column_type == AutoClosingColumnType::Special1,
                            "()",
                        );
                    });
                },
            );
        });
    }

    #[test]
    fn editor_controller_auto_closing_pairs_open_parens_disabled_enabled_open_quotes_enabled_disabled() {
        run_test(|f| {
            f.using_cursor(
                CursorOpts {
                    editor_opts: Some(string_options(&[("autoClosingBrackets", "beforeWhitespace"), ("autoClosingQuotes", "never")])),
                    ..auto_closing_opts(vec!["var a = [];"])
                },
                |editor, model, view_model| {
                    for_each_auto_closing_column(model, &["var| a| =| [|];|"], |line_number, column, column_type| {
                        assert_auto_close(
                            editor,
                            model,
                            view_model,
                            line_number,
                            column,
                            "(",
                            column_type == AutoClosingColumnType::Special1,
                            "()",
                        );
                        assert_type(
                            editor,
                            model,
                            view_model,
                            line_number,
                            column,
                            "'",
                            "'",
                            &format!("does not auto close @ ({line_number}, {column})"),
                        );
                    });
                },
            );

            f.using_cursor(
                CursorOpts {
                    editor_opts: Some(string_options(&[("autoClosingBrackets", "never"), ("autoClosingQuotes", "beforeWhitespace")])),
                    ..auto_closing_opts(vec!["var b = [];"])
                },
                |editor, model, view_model| {
                    for_each_auto_closing_column(model, &["var b =| [|];|"], |line_number, column, column_type| {
                        assert_auto_close(
                            editor,
                            model,
                            view_model,
                            line_number,
                            column,
                            "'",
                            column_type == AutoClosingColumnType::Special1,
                            "''",
                        );
                        assert_type(
                            editor,
                            model,
                            view_model,
                            line_number,
                            column,
                            "(",
                            "(",
                            &format!("does not auto close @ ({line_number}, {column})"),
                        );
                    });
                },
            );
        });
    }

    #[test]
    fn editor_controller_auto_closing_pairs_configurable_open_parens() {
        run_test(|f| {
            f.set_auto_closing_language_enabled_set("abc");
            f.using_cursor(
                CursorOpts {
                    editor_opts: Some(string_options(&[("autoClosingBrackets", "languageDefined")])),
                    ..auto_closing_opts(AUTO_CLOSING_SAMPLE_LINES.to_vec())
                },
                |editor, model, view_model| {
                    let auto_close_positions = [
                        "v|ar |a = [|];|",
                        "v|ar |b = `|asd`;|",
                        "v|ar |c = '|asd';|",
                        "v|ar d = \"|asd\";|",
                        "v|ar e = /*3*/\t3;|",
                        "v|ar f = /** 3| */3;|",
                        "v|ar g = (3+5|);|",
                        "v|ar h = { |a: 'v|alue' |};|",
                    ];
                    for_each_auto_closing_column(model, &auto_close_positions, |line_number, column, column_type| {
                        assert_auto_close(
                            editor,
                            model,
                            view_model,
                            line_number,
                            column,
                            "(",
                            column_type == AutoClosingColumnType::Special1,
                            "()",
                        );
                    });
                },
            );
        });
    }

    #[test]
    fn editor_controller_auto_closing_pairs_auto_pairing_can_be_disabled() {
        run_test(|f| {
            f.using_cursor(
                CursorOpts {
                    editor_opts: Some(string_options(&[("autoClosingBrackets", "never"), ("autoClosingQuotes", "never")])),
                    ..auto_closing_opts(AUTO_CLOSING_SAMPLE_LINES.to_vec())
                },
                |editor, model, view_model| {
                    for_each_auto_closing_column(model, &AUTO_CLOSING_SAMPLE_LINES, |line_number, column, column_type| {
                        let closes = column_type == AutoClosingColumnType::Special1;
                        assert_auto_close(editor, model, view_model, line_number, column, "(", closes, "()");
                        assert_auto_close(editor, model, view_model, line_number, column, "\"", closes, "\"\"");
                    });
                },
            );
        });
    }

    #[test]
    fn editor_controller_auto_closing_pairs_auto_wrapping_is_configurable() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["var a = asd"]), |_editor, model, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 1, 1, 4), Selection::new(1, 9, 1, 12)]);

                // type a `
                type_text(view_model, "`", Some("keyboard"));

                assert_eq!(get_value(model), "`var` a = `asd`");

                // type a (
                type_text(view_model, "(", Some("keyboard"));

                assert_eq!(get_value(model), "`(var)` a = `(asd)`");
            });

            f.using_cursor(
                CursorOpts {
                    editor_opts: Some(string_options(&[("autoSurround", "never")])),
                    ..auto_closing_opts(vec!["var a = asd"])
                },
                |_editor, model, view_model| {
                    set_test_selections(view_model, &[Selection::new(1, 1, 1, 4)]);

                    // type a `
                    type_text(view_model, "`", Some("keyboard"));

                    assert_eq!(get_value(model), "` a = asd");
                },
            );

            f.using_cursor(
                CursorOpts {
                    editor_opts: Some(string_options(&[("autoSurround", "quotes")])),
                    ..auto_closing_opts(vec!["var a = asd"])
                },
                |_editor, model, view_model| {
                    set_test_selections(view_model, &[Selection::new(1, 1, 1, 4)]);

                    // type a `
                    type_text(view_model, "`", Some("keyboard"));
                    assert_eq!(get_value(model), "`var` a = asd");

                    // type a (
                    type_text(view_model, "(", Some("keyboard"));
                    assert_eq!(get_value(model), "`(` a = asd");
                },
            );

            f.using_cursor(
                CursorOpts {
                    editor_opts: Some(string_options(&[("autoSurround", "brackets")])),
                    ..auto_closing_opts(vec!["var a = asd"])
                },
                |_editor, model, view_model| {
                    set_test_selections(view_model, &[Selection::new(1, 1, 1, 4)]);

                    // type a (
                    type_text(view_model, "(", Some("keyboard"));
                    assert_eq!(get_value(model), "(var) a = asd");

                    // type a `
                    type_text(view_model, "`", Some("keyboard"));
                    assert_eq!(get_value(model), "(`) a = asd");
                },
            );
        });
    }

    #[test]
    fn editor_controller_auto_closing_pairs_quote() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(AUTO_CLOSING_SAMPLE_LINES.to_vec()), |editor, model, view_model| {
                let auto_close_positions = [
                    "var a |=| [|]|;|",
                    "var b |=| `asd`|;|",
                    "var c |=| 'asd'|;|",
                    "var d |=| \"asd\"|;|",
                    "var e |=| /*3*/|\t3;|",
                    "var f |=| /**| 3 */3;|",
                    "var g |=| (3+5)|;|",
                    "var h |=| {| a:| 'value'| |}|;|",
                ];
                for_each_auto_closing_column(model, &auto_close_positions, |line_number, column, column_type| match column_type {
                    AutoClosingColumnType::Special1 => {
                        assert_type(editor, model, view_model, line_number, column, "'", "''", &format!("auto closes @ ({line_number}, {column})"));
                    }
                    AutoClosingColumnType::Special2 => {
                        assert_type(editor, model, view_model, line_number, column, "'", "", &format!("over types @ ({line_number}, {column})"));
                    }
                    AutoClosingColumnType::Normal => {
                        assert_type(editor, model, view_model, line_number, column, "'", "'", &format!("does not auto close @ ({line_number}, {column})"));
                    }
                });
            });
        });
    }

    #[test]
    fn editor_controller_auto_closing_pairs_multi_character_autoclose() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec![""]), |_editor, model, view_model| {
                reset_model_with_selection(model, view_model, "begi", Selection::new(1, 5, 1, 5));
                type_text(view_model, "n", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "beginend");

                reset_model_with_selection(model, view_model, "/*", Selection::new(1, 3, 1, 3));
                type_text(view_model, "*", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "/** */");
            });
        });
    }

    #[test]
    fn editor_controller_issue_338693_undo_after_multi_character_autoclose_first_removes_only_the_auto_closed_characters() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            f.using_cursor(auto_closing_opts(vec!["begi begi"]), |editor, model, view_model| {
                let mut states: Vec<(String, Vec<String>)> = Vec::new();

                set_test_selections(view_model, &[Selection::new(1, 5, 1, 5), Selection::new(1, 10, 1, 10)]);
                type_text(view_model, "n", Some("keyboard"));
                states.push(line_and_selections(model, view_model, 1));
                editor.run_command(&*editing.undo, None);
                states.push(line_and_selections(model, view_model, 1));
                editor.run_command(&*editing.undo, None);
                states.push(line_and_selections(model, view_model, 1));

                assert_eq!(
                    states,
                    expected_states(&[
                        ("beginend beginend", &["[1,6 -> 1,6]", "[1,15 -> 1,15]"]),
                        ("begin begin", &["[1,6 -> 1,6]", "[1,12 -> 1,12]"]),
                        ("begi begi", &["[1,5 -> 1,5]", "[1,10 -> 1,10]"]),
                    ])
                );
            });
        });
    }

    #[test]
    fn editor_controller_issue_338693_multi_character_autoclose_does_not_insert_an_orphan_close_when_a_listener_reentrantly_removes_the_typed_character() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["begi"]), |_editor, model, view_model| {
                let is_first = Rc::new(Cell::new(true));
                let listener_model = model.clone();
                f.disposables.add(model.on_did_change_content().listen(move |_| {
                    if is_first.get() {
                        is_first.set(false);
                        listener_model.apply_edits(&[EditOperation::replace(Range::new(1, 5, 1, 6), Some(JsString::from("")))]);
                    }
                }));

                set_test_selections(view_model, &[Selection::new(1, 5, 1, 5)]);
                type_text(view_model, "n", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "begi");
            });
        });
    }

    #[test]
    fn editor_controller_issue_338693_multi_character_autoclose_does_not_use_stale_selections_after_a_reentrant_edit() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["begi"]), |_editor, model, view_model| {
                let is_first = Rc::new(Cell::new(true));
                let listener_model = model.clone();
                f.disposables.add(model.on_did_change_content().listen(move |_| {
                    if is_first.get() {
                        is_first.set(false);
                        listener_model.apply_edits(&[EditOperation::replace(Range::new(1, 1, 1, 1), Some(JsString::from("begin\n")))]);
                    }
                }));

                set_test_selections(view_model, &[Selection::new(1, 5, 1, 5)]);
                type_text(view_model, "n", Some("keyboard"));
                assert_eq!(get_value(model), "begin\nbegin");
            });
        });
    }

    #[test]
    fn editor_controller_issue_338693_undo_after_multi_character_autoclose_with_auto_indentation_first_removes_only_the_auto_closed_characters() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let language_id = "autoClosingWithIndentRulesLanguage";
            f.register_language(
                language_id,
                LanguageConfiguration {
                    auto_closing_pairs: Some(vec![AutoClosingPairConditional::new("begin", "end")]),
                    indentation_rules: Some(indentation_rules(r"^\s*begin\b", r"^\s*procedure\b", "")),
                    ..LanguageConfiguration::default()
                },
            );
            f.using_cursor(
                CursorOpts {
                    text: vec!["\tfoo", "\tbegi"],
                    language_id: Some(language_id),
                    editor_opts: Some(string_options(&[("autoIndent", "full")])),
                    ..Default::default()
                },
                |editor, model, view_model| {
                    let mut states: Vec<(String, Vec<String>)> = Vec::new();

                    set_test_selections(view_model, &[Selection::new(2, 6, 2, 6)]);
                    type_text(view_model, "n", Some("keyboard"));
                    states.push(line_and_selections(model, view_model, 2));
                    editor.run_command(&*editing.undo, None);
                    states.push(line_and_selections(model, view_model, 2));
                    editor.run_command(&*editing.undo, None);
                    states.push(line_and_selections(model, view_model, 2));

                    assert_eq!(
                        states,
                        expected_states(&[
                            ("beginend", &["[2,6 -> 2,6]"]),
                            ("begin", &["[2,6 -> 2,6]"]),
                            ("\tbegi", &["[2,6 -> 2,6]"]),
                        ])
                    );
                },
            );
        });
    }

    #[test]
    fn editor_controller_auto_closing_pairs_before_text() {
        run_test(|f| {
            let language_id = "autoClosingBeforeText";

            f.register_language(
                language_id,
                LanguageConfiguration {
                    auto_closing_pairs: Some(vec![AutoClosingPairConditional {
                        before_text: Some(re(r"(?:^|\s)begin$", "g")),
                        ..AutoClosingPairConditional::new("begin", "end")
                    }]),
                    ..LanguageConfiguration::default()
                },
            );

            f.using_cursor(
                CursorOpts { text: vec!["begi", "begi"], language_id: Some(language_id), ..Default::default() },
                |_editor, model, view_model| {
                    let mut results: Vec<String> = Vec::new();

                    set_test_selections(view_model, &[Selection::new(1, 5, 1, 5), Selection::new(2, 5, 2, 5)]);
                    type_text(view_model, "n", Some("keyboard"));
                    results.push(get_value(model));

                    reset_model_with_selection(model, view_model, "prefixbegi", Selection::new(1, 11, 1, 11));
                    type_text(view_model, "n", Some("keyboard"));
                    results.push(get_value(model));

                    reset_model_with_selection(model, view_model, "prefix begi", Selection::new(1, 12, 1, 12));
                    type_text(view_model, "n", Some("keyboard"));
                    results.push(get_value(model));

                    assert_eq!(results, ["beginend\nbeginend", "prefixbegin", "prefix beginend"]);
                },
            );
        });
    }

    /// A linguagem dos dois casos `beforeText falls back to a shorter pair`: o par `begin`/`end` condicionado e o par curto
    /// `n`/`]`.
    fn register_before_text_fallback_language(f: &EditorControllerFixture, language_id: &str) {
        f.register_language(
            language_id,
            LanguageConfiguration {
                auto_closing_pairs: Some(vec![
                    AutoClosingPairConditional {
                        before_text: Some(re(r"(?:^|\s)begin$", "")),
                        ..AutoClosingPairConditional::new("begin", "end")
                    },
                    AutoClosingPairConditional::new("n", "]"),
                ]),
                ..LanguageConfiguration::default()
            },
        );
    }

    #[test]
    fn editor_controller_auto_closing_pairs_before_text_falls_back_to_a_shorter_pair() {
        run_test(|f| {
            let language_id = "autoClosingBeforeTextFallback";
            register_before_text_fallback_language(f, language_id);

            f.using_cursor(
                CursorOpts { text: vec!["prefixbegi"], language_id: Some(language_id), ..Default::default() },
                |_editor, model, view_model| {
                    set_test_selections(view_model, &[Selection::new(1, 11, 1, 11)]);
                    type_text(view_model, "n", Some("keyboard"));

                    assert_eq!(get_value(model), "prefixbegin]");
                },
            );
        });
    }

    #[test]
    fn editor_controller_auto_closing_pairs_before_text_falls_back_to_a_shorter_pair_after_composition() {
        run_test(|f| {
            let language_id = "autoClosingBeforeTextCompositionFallback";
            register_before_text_fallback_language(f, language_id);

            f.using_cursor(
                CursorOpts { text: vec!["prefixbegi"], language_id: Some(language_id), ..Default::default() },
                |_editor, model, view_model| {
                    set_test_selections(view_model, &[Selection::new(1, 11, 1, 11)]);
                    type_with_composition(view_model, "n", &[("n", 1)]);

                    assert_eq!(get_value(model), "prefixbegin]");
                },
            );
        });
    }

    #[test]
    fn editor_controller_auto_closing_pairs_doc_comments_can_be_turned_off() {
        run_test(|f| {
            f.using_cursor(
                CursorOpts {
                    editor_opts: Some(string_options(&[("autoClosingComments", "never")])),
                    ..auto_closing_opts(vec![""])
                },
                |_editor, model, view_model| {
                    reset_model_with_selection(model, view_model, "/*", Selection::new(1, 3, 1, 3));
                    type_text(view_model, "*", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "/**");
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_72177_multi_character_autoclose_with_conflicting_patterns() {
        run_test(|f| {
            let language_id = "autoClosingModeMultiChar";

            f.register_language(
                language_id,
                LanguageConfiguration {
                    auto_closing_pairs: Some(vec![
                        AutoClosingPairConditional::new("(", ")"),
                        AutoClosingPairConditional::new("(*", "*)"),
                        AutoClosingPairConditional::new("<@", "@>"),
                        AutoClosingPairConditional::new("<@@", "@@>"),
                    ]),
                    ..LanguageConfiguration::default()
                },
            );

            f.using_cursor(
                CursorOpts { text: vec![""], language_id: Some(language_id), ..Default::default() },
                |_editor, model, view_model| {
                    type_text(view_model, "(", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "()");
                    type_text(view_model, "*", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "(**)", "doesn't add entire close when already closed substring is there");

                    reset_model_with_selection(model, view_model, "(", Selection::new(1, 2, 1, 2));
                    type_text(view_model, "*", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "(**)", "does add entire close if not already there");

                    model.set_value("".into());
                    type_text(view_model, "<@", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "<@@>");
                    type_text(view_model, "@", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "<@@@@>", "autocloses when before multi-character closing brace");
                    type_text(view_model, "(", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "<@@()@@>", "autocloses when before multi-character closing brace");
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_55314_do_not_auto_close_when_ending_with_open() {
        run_test(|f| {
            let language_id = "myElectricMode";

            f.register_language(
                language_id,
                LanguageConfiguration {
                    auto_closing_pairs: Some(vec![
                        AutoClosingPairConditional::new("{", "}"),
                        AutoClosingPairConditional::new("[", "]"),
                        AutoClosingPairConditional::new("(", ")"),
                        AutoClosingPairConditional::new("'", "'").not_in(&["string", "comment"]),
                        AutoClosingPairConditional::new("\"", "\"").not_in(&["string"]),
                        AutoClosingPairConditional::new("B\"", "\"").not_in(&["string", "comment"]),
                        AutoClosingPairConditional::new("`", "`").not_in(&["string", "comment"]),
                        AutoClosingPairConditional::new("/**", " */").not_in(&["string"]),
                    ]),
                    ..LanguageConfiguration::default()
                },
            );

            f.using_cursor(
                CursorOpts {
                    text: vec!["little goat", "little LAMB", "little sheep", "Big LAMB"],
                    language_id: Some(language_id),
                    ..Default::default()
                },
                |editor, model, view_model| {
                    for (line_number, column) in [(1, 4), (2, 4), (3, 4), (4, 2), (4, 3)] {
                        model.tokenization().force_tokenization(model.get_line_count());
                        assert_type(editor, model, view_model, line_number, column, "\"", "\"", "does not double quote when ending with open");
                    }
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_27937_trying_to_add_an_item_to_the_front_of_a_list_is_cumbersome() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["var arr = [\"b\", \"c\"];"]), |editor, model, view_model| {
                assert_type(editor, model, view_model, 1, 12, "\"", "\"", "does not over type and will not auto close");
            });
        });
    }

    #[test]
    fn editor_controller_issue_6841_auto_closing_brackets_should_balance_brackets() {
        run_test(|f| {
            let language_id = "balancedAutoClosingLanguage";
            f.register_language(
                language_id,
                LanguageConfiguration {
                    brackets: Some(vec![bracket_pair("{", "}"), bracket_pair("[", "]"), bracket_pair("(", ")")]),
                    auto_closing_pairs: Some(vec![
                        AutoClosingPairConditional::new("{", "}"),
                        AutoClosingPairConditional::new("[", "]"),
                        AutoClosingPairConditional::new("(", ")"),
                    ]),
                    ..LanguageConfiguration::default()
                },
            );

            f.using_cursor(
                CursorOpts { text: vec![""], language_id: Some(language_id), ..Default::default() },
                |_editor, model, view_model| {
                    model.bracket_pairs().get_brackets_in_range(model.get_full_model_range(), false).to_array();
                    // (texto, seleção, o que se digita, o esperado)
                    let test_cases = [
                        ("\n}", Selection::new(1, 1, 1, 1), "{", "{\n}"),
                        ("", Selection::new(1, 1, 1, 1), "{", "{}"),
                        ("{}", Selection::new(1, 2, 1, 2), "{", "{{}}"),
                        ("{\n", Selection::new(2, 1, 2, 1), "{", "{\n{}"),
                        ("\n]", Selection::new(1, 1, 1, 1), "{", "{}\n]"),
                        (")\n", Selection::new(2, 1, 2, 1), "(", ")\n()"),
                        ("function foo() {\n\n}", Selection::new(2, 1, 2, 1), "{", "function foo() {\n{}\n}"),
                        ("someFunction);", Selection::new(1, 13, 1, 13), "(", "someFunction();"),
                        ("someFunction;", Selection::new(1, 13, 1, 13), "(", "someFunction();"),
                        ("someFunctionsomeParam);", Selection::new(1, 13, 1, 13), "(", "someFunction(someParam);"),
                    ];
                    let actual: Vec<String> = test_cases
                        .iter()
                        .map(|(text, selection, typed, _)| {
                            reset_model_with_selection(model, view_model, text, *selection);
                            type_text(view_model, typed, Some("keyboard"));
                            get_value(model)
                        })
                        .collect();
                    let expected: Vec<String> = test_cases.iter().map(|(_, _, _, expected)| (*expected).to_owned()).collect();
                    assert_eq!(actual, expected);
                },
            );

            f.using_cursor(
                CursorOpts { text: vec!["", "}", ""], language_id: Some(language_id), ..Default::default() },
                |_editor, model, view_model| {
                    model.bracket_pairs().get_brackets_in_range(model.get_full_model_range(), false).to_array();
                    set_test_selections(view_model, &[Selection::new(1, 1, 1, 1), Selection::new(3, 1, 3, 1)]);
                    type_text(view_model, "{", Some("keyboard"));
                    assert_eq!(get_value(model), "{\n}\n{");
                },
            );

            f.using_cursor(
                CursorOpts {
                    text: vec!["", ")"],
                    language_id: Some(language_id),
                    editor_opts: Some(string_options(&[("autoClosingBrackets", "always")])),
                    ..Default::default()
                },
                |_editor, model, view_model| {
                    type_text(view_model, "(", Some("keyboard"));
                    assert_eq!(get_value(model), "()\n)");
                },
            );

            f.disposables.add(f.language_configuration_service.register(
                AUTO_CLOSING_LANGUAGE_ID,
                LanguageConfiguration { brackets: Some(vec![bracket_pair("{", "}")]), ..LanguageConfiguration::default() },
                None,
            ));
            f.setup_auto_closing_language_tokenization();
            f.using_cursor(auto_closing_opts(vec!["", "\"}\""]), |_editor, model, view_model| {
                type_text(view_model, "{", Some("keyboard"));
                assert_eq!(get_value(model), "{}\n\"}\"");
            });

            f.using_cursor(auto_closing_opts(vec!["", "\"}\"", "// }"]), |_editor, model, view_model| {
                model.tokenization().force_tokenization(model.get_line_count());
                model.bracket_pairs().get_brackets_in_range(model.get_full_model_range(), false).to_array();
                type_text(view_model, "{", Some("keyboard"));
                assert_eq!(get_value(model), "{}\n\"}\"\n// }");
            });
        });
    }

    #[test]
    fn editor_controller_issue_25658_do_not_auto_close_single_double_quotes_after_word_characters() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec![""]), |_editor, model, view_model| {
                // First gif
                model.tokenization().force_tokenization(model.get_line_count());
                type_characters(view_model, "teste1 = teste' ok");
                assert_eq!(get_line_content(model, 1), "teste1 = teste' ok");

                // (o que se digita, a linha esperada) das linhas 2 a 8, cada uma começada por um Enter no fim da anterior
                let following_lines = [
                    ("teste2 = teste 'ok", "teste2 = teste 'ok'"),
                    ("teste3 = teste\" ok", "teste3 = teste\" ok"),
                    ("teste4 = teste \"ok", "teste4 = teste \"ok\""),
                    // Second gif
                    ("teste '", "teste ''"),
                    ("teste \"", "teste \"\""),
                    ("teste'", "teste'"),
                    ("teste\"", "teste\""),
                ];
                for (index, (typed, expected)) in following_lines.iter().enumerate() {
                    let previous_line_number = index as i32 + 1;
                    set_test_selections(view_model, &[Selection::new(previous_line_number, 1000, previous_line_number, 1000)]);
                    type_characters(view_model, "\n");
                    model.tokenization().force_tokenization(model.get_line_count());
                    type_characters(view_model, typed);
                    assert_eq!(get_line_content(model, previous_line_number + 1), *expected);
                }
            });
        });
    }

    #[test]
    fn editor_controller_issue_37315_overtypes_only_those_characters_that_it_inserted() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["", "y=();"]), |_editor, model, view_model| {
                assert_cursor(view_model, Position::new(1, 1));

                type_text(view_model, "x=(", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=()");

                type_text(view_model, "asd", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=(asd)");

                // overtype!
                type_text(view_model, ")", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=(asd)");

                // do not overtype!
                set_test_selections(view_model, &[Selection::new(2, 4, 2, 4)]);
                type_text(view_model, ")", Some("keyboard"));
                assert_eq!(get_line_content(model, 2), "y=());");
            });
        });
    }

    #[test]
    fn editor_controller_issue_37315_stops_overtyping_once_cursor_leaves_area() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["", "y=();"]), |_editor, model, view_model| {
                assert_cursor(view_model, Position::new(1, 1));

                type_text(view_model, "x=(", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=()");

                set_test_selections(view_model, &[Selection::new(1, 5, 1, 5)]);
                type_text(view_model, ")", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=())");
            });
        });
    }

    #[test]
    fn editor_controller_issue_37315_it_overtypes_only_once() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["", "y=();"]), |_editor, model, view_model| {
                assert_cursor(view_model, Position::new(1, 1));

                type_text(view_model, "x=(", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=()");

                type_text(view_model, ")", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=()");

                set_test_selections(view_model, &[Selection::new(1, 4, 1, 4)]);
                type_text(view_model, ")", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=())");
            });
        });
    }

    #[test]
    fn editor_controller_issue_37315_it_can_remember_multiple_auto_closed_instances() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["", "y=();"]), |_editor, model, view_model| {
                assert_cursor(view_model, Position::new(1, 1));

                type_text(view_model, "x=(", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=()");

                type_text(view_model, "(", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=(())");

                type_text(view_model, ")", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=(())");

                type_text(view_model, ")", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=(())");
            });
        });
    }

    #[test]
    fn editor_controller_issue_205598_retains_auto_closed_actions_created_by_a_reentrant_decorations_listener() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec![""]), |editor, model, _view_model| {
                let mut results: Vec<(String, usize)> = Vec::new();
                for iteration in 0..3 {
                    editor.execute_edits(
                        ExecuteEditsSource::Name(Some("test")),
                        vec![SingleEditOperation::new(model.get_full_model_range(), Some(JsString::from("")), false)],
                        Some(EndCursorState::Selections(vec![Selection::new(1, 1, 1, 1)])),
                    );
                    for _pair in 0..=iteration {
                        trigger_type(editor, "(");
                    }
                    let armed = Rc::new(Cell::new(true));
                    let listener_editor = editor.clone();
                    let listener = f.disposables.add(model.on_did_change_decorations().listen(move |_| {
                        if armed.get() {
                            armed.set(false);
                            trigger_type(&listener_editor, "(");
                        }
                    }));
                    editor.set_position(Position::new(1, CursorSimpleModel::get_line_max_column(&**model, 1)), None);
                    listener.dispose();
                    trigger_type(editor, ")");
                    editor.set_position(Position::new(1, 1), None);
                    results.push((
                        get_value(model),
                        model
                            .get_all_decorations(0, false, false)
                            .iter()
                            .filter(|decoration| decoration.options.description.starts_with("auto-closed-"))
                            .count(),
                    ));
                }
                assert_eq!(
                    results,
                    vec![("()()".to_owned(), 0), ("(())()".to_owned(), 0), ("((()))()".to_owned(), 0)]
                );
            });
        });
    }

    #[test]
    fn editor_controller_issue_118270_auto_closing_deletes_only_those_characters_that_it_inserted() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            f.using_cursor(auto_closing_opts(vec!["", "y=();"]), |editor, model, view_model| {
                assert_cursor(view_model, Position::new(1, 1));

                type_text(view_model, "x=(", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=()");

                type_text(view_model, "asd", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "x=(asd)");

                editor.run_command(&*editing.delete_left, None);
                editor.run_command(&*editing.delete_left, None);
                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(model, 1), "x=()");

                // delete closing char!
                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(model, 1), "x=");

                // do not delete closing char!
                set_test_selections(view_model, &[Selection::new(2, 4, 2, 4)]);
                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(model, 2), "y=);");
            });
        });
    }

    #[test]
    fn editor_controller_issue_78527_does_not_close_quote_on_odd_count() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["std::cout << '\"' << entryMap"]), |_editor, model, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 29, 1, 29)]);

                type_text(view_model, "[", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "std::cout << '\"' << entryMap[]");

                type_text(view_model, "\"", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "std::cout << '\"' << entryMap[\"\"]");

                type_text(view_model, "a", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "std::cout << '\"' << entryMap[\"a\"]");

                type_text(view_model, "\"", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "std::cout << '\"' << entryMap[\"a\"]");

                type_text(view_model, "]", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "std::cout << '\"' << entryMap[\"a\"]");
            });
        });
    }

    #[test]
    fn editor_controller_issue_85983_editor_auto_closing_brackets_before_whitespace_is_incorrect_for_python() {
        run_test(|f| {
            let language_id = "pythonMode";

            let mut auto_closing_pairs = vec![
                AutoClosingPairConditional::new("{", "}"),
                AutoClosingPairConditional::new("[", "]"),
                AutoClosingPairConditional::new("(", ")"),
                AutoClosingPairConditional::new("\"", "\"").not_in(&["string"]),
            ];
            auto_closing_pairs.extend(python_prefixed_quote_pairs("\""));
            auto_closing_pairs.push(AutoClosingPairConditional::new("'", "'").not_in(&["string", "comment"]));
            auto_closing_pairs.extend(python_prefixed_quote_pairs("'"));
            auto_closing_pairs.push(AutoClosingPairConditional::new("`", "`").not_in(&["string"]));

            f.register_language(
                language_id,
                LanguageConfiguration { auto_closing_pairs: Some(auto_closing_pairs), ..LanguageConfiguration::default() },
            );

            f.using_cursor(
                CursorOpts {
                    text: vec!["foo'hello'"],
                    editor_opts: Some(string_options(&[("autoClosingBrackets", "beforeWhitespace")])),
                    language_id: Some(language_id),
                    ..Default::default()
                },
                |editor, model, view_model| {
                    assert_type(editor, model, view_model, 1, 4, "(", "(", "does not auto close @ (1, 4)");
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_78975_parentheses_swallowing_does_not_work_when_parentheses_are_inserted_by_autocomplete() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["<div id"]), |_editor, model, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 8, 1, 8)]);

                let mut cursor_state_computer =
                    |_: &[ValidEditOperation]| -> Option<Vec<Selection>> { Some(vec![Selection::new(1, 10, 1, 10)]) };
                view_model.execute_edits(
                    Some("snippet"),
                    vec![SingleEditOperation::new(Range::new(1, 6, 1, 8), Some(JsString::from("id=\"\"")), false)],
                    &mut cursor_state_computer,
                    EditSources::unknown(None),
                );
                assert_eq!(get_line_content(model, 1), "<div id=\"\"");

                type_text(view_model, "a", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "<div id=\"a\"");

                type_text(view_model, "\"", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "<div id=\"a\"");
            });
        });
    }

    #[test]
    fn editor_controller_issue_78833_add_config_to_use_old_brackets_quotes_overtyping() {
        run_test(|f| {
            f.using_cursor(
                CursorOpts {
                    editor_opts: Some(string_options(&[("autoClosingOvertype", "always")])),
                    ..auto_closing_opts(vec!["", "y=();"])
                },
                |_editor, model, view_model| {
                    assert_cursor(view_model, Position::new(1, 1));

                    type_text(view_model, "x=(", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "x=()");

                    type_text(view_model, ")", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "x=()");

                    set_test_selections(view_model, &[Selection::new(1, 4, 1, 4)]);
                    type_text(view_model, ")", Some("keyboard"));
                    assert_eq!(get_line_content(model, 1), "x=()");

                    set_test_selections(view_model, &[Selection::new(2, 4, 2, 4)]);
                    type_text(view_model, ")", Some("keyboard"));
                    assert_eq!(get_line_content(model, 2), "y=();");
                },
            );
        });
    }

    #[test]
    fn editor_controller_issue_15825_accents_on_mac_us_intl_keyboard() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec![]), |_editor, model, view_model| {
                assert_cursor(view_model, Position::new(1, 1));

                // Typing ` + e on the mac US intl kb layout
                type_with_composition(view_model, "`", &[("è", 1)]);

                assert_eq!(get_value(model), "è");
            });
        });
    }

    #[test]
    fn editor_controller_issue_90016_allow_accents_on_mac_us_intl_keyboard_to_surround_selection() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["test"]), |_editor, model, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 1, 1, 5)]);

                // Typing ` + e on the mac US intl kb layout
                type_with_composition(view_model, "'", &[("'", 1), ("'", 1)]);

                assert_eq!(get_value(model), "'test'");
            });
        });
    }

    #[test]
    fn editor_controller_issue_53357_over_typing_ignores_characters_after_backslash() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["console.log();"]), |_editor, model, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 13, 1, 13)]);

                type_text(view_model, "'", Some("keyboard"));
                assert_eq!(get_value(model), "console.log('');");

                type_text(view_model, "it", Some("keyboard"));
                assert_eq!(get_value(model), "console.log('it');");

                type_text(view_model, "\\", Some("keyboard"));
                assert_eq!(get_value(model), "console.log('it\\');");

                type_text(view_model, "'", Some("keyboard"));
                assert_eq!(get_value(model), "console.log('it\\'');");
            });
        });
    }

    #[test]
    fn editor_controller_issue_84998_overtyping_brackets_doesnt_work_after_backslash() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec![""]), |_editor, model, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 1, 1, 1)]);

                type_text(view_model, "\\", Some("keyboard"));
                assert_eq!(get_value(model), "\\");

                type_text(view_model, "(", Some("keyboard"));
                assert_eq!(get_value(model), "\\()");

                type_text(view_model, "abc", Some("keyboard"));
                assert_eq!(get_value(model), "\\(abc)");

                type_text(view_model, "\\", Some("keyboard"));
                assert_eq!(get_value(model), "\\(abc\\)");

                type_text(view_model, ")", Some("keyboard"));
                assert_eq!(get_value(model), "\\(abc\\)");
            });
        });
    }

    #[test]
    fn editor_controller_issue_2773_accents_are_inserted_in_the_wrong_position_mac() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["hello", "world"]), |editor, model, view_model| {
                assert_cursor(view_model, Position::new(1, 1));

                // Typing ` and pressing shift+down on the mac US intl kb layout
                // Here we're just replaying what the cursor gets
                view_model.start_composition();
                type_text(view_model, "`", Some("keyboard"));
                move_down(editor, view_model, true);
                view_model.composition_type(&u("`"), 1, 0, 0, Some("keyboard"));
                view_model.composition_type(&u("`"), 1, 0, 0, Some("keyboard"));
                view_model.end_composition(Some("keyboard"));

                assert_eq!(get_value(model), "`hello\nworld");
                assert_cursor(view_model, Selection::new(1, 2, 2, 2));
            });
        });
    }

    #[test]
    fn editor_controller_issue_26820_auto_close_quotes_when_not_used_as_accents() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec![""]), |_editor, model, view_model| {
                assert_cursor(view_model, Position::new(1, 1));

                // on the mac US intl kb layout

                // Typing ' + space
                type_with_composition(view_model, "'", &[("'", 1)]);
                assert_eq!(get_value(model), "''");

                // Typing one more ' + space
                type_with_composition(view_model, "'", &[("'", 1)]);
                assert_eq!(get_value(model), "''");

                // Typing ' as a closing tag
                reset_model_with_selection(model, view_model, "'abc", Selection::new(1, 5, 1, 5));
                type_with_composition(view_model, "'", &[("'", 1)]);

                assert_eq!(get_value(model), "'abc'");

                // quotes before the newly added character are all paired.
                reset_model_with_selection(model, view_model, "'abc'def ", Selection::new(1, 10, 1, 10));
                type_with_composition(view_model, "'", &[("'", 1)]);

                assert_eq!(get_value(model), "'abc'def ''");

                // No auto closing if there is non-whitespace character after the cursor
                reset_model_with_selection(model, view_model, "abc", Selection::new(1, 1, 1, 1));
                type_with_composition(view_model, "'", &[("'", 1)]);

                // No auto closing if it's after a word.
                reset_model_with_selection(model, view_model, "abc", Selection::new(1, 4, 1, 4));
                type_with_composition(view_model, "'", &[("'", 1)]);

                assert_eq!(get_value(model), "abc'");
            });
        });
    }

    #[test]
    fn editor_controller_issue_144690_quotes_do_not_overtype_when_using_us_intl_pc_keyboard_layout() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec![""]), |_editor, model, view_model| {
                assert_cursor(view_model, Position::new(1, 1));

                // Pressing ' + ' + ;

                type_with_composition(view_model, "'", &[("'", 1), ("'", 1)]);
                type_with_composition(view_model, "'", &[("';", 1), ("';", 2)]);

                assert_eq!(get_value(model), "'';");
            });
        });
    }

    #[test]
    fn editor_controller_issue_144693_typing_a_quote_using_us_intl_pc_keyboard_layout_always_surrounds_words() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["const hello = 3;"]), |_editor, model, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 7, 1, 12)]);

                // Pressing ' + e

                type_with_composition(view_model, "'", &[("é", 1), ("é", 1)]);

                assert_eq!(get_value(model), "const é = 3;");
            });
        });
    }

    #[test]
    fn editor_controller_issue_82701_auto_close_does_not_execute_when_ime_is_canceled_via_backspace() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["{}"]), |_editor, model, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 2, 1, 2)]);

                // Typing a + backspace
                type_with_composition(view_model, "a", &[("", 1)]);
                assert_eq!(get_value(model), "{}");
            });
        });
    }

    #[test]
    fn editor_controller_issue_20891_all_cursors_should_do_the_same_thing() {
        run_test(|f| {
            f.using_cursor(auto_closing_opts(vec!["var a = asd"]), |_editor, model, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 9, 1, 9), Selection::new(1, 12, 1, 12)]);

                // type a `
                type_text(view_model, "`", Some("keyboard"));

                assert_eq!(get_value(model), "var a = `asd`");
            });
        });
    }

    #[test]
    fn editor_controller_issue_41825_special_handling_of_quotes_in_surrounding_pairs() {
        run_test(|f| {
            let language_id = "myMode";

            f.register_language(
                language_id,
                LanguageConfiguration {
                    surrounding_pairs: Some(vec![AutoClosingPair::new("\"", "\""), AutoClosingPair::new("'", "'")]),
                    ..LanguageConfiguration::default()
                },
            );

            let model = f.create_text_model("var x = 'hi';", Some(language_id), Default::default(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                editor.set_selections(&[Selection::new(1, 9, 1, 10), Selection::new(1, 12, 1, 13)], None, None);
                type_text(view_model, "\"", Some("keyboard"));
                assert_eq!(get_value_lf(&model), "var x = \"hi\";", "assert1");

                editor.set_selections(&[Selection::new(1, 9, 1, 10), Selection::new(1, 12, 1, 13)], None, None);
                type_text(view_model, "'", Some("keyboard"));
                assert_eq!(get_value_lf(&model), "var x = 'hi';", "assert2");
            });
        });
    }

    #[test]
    fn editor_controller_all_cursors_should_do_the_same_thing_when_deleting_left() {
        run_test(|f| {
            let editing = core_editing_commands::instance();
            let model = f.create_text_model(["var a = ()"].join("\n"), Some(AUTO_CLOSING_LANGUAGE_ID), Default::default(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor, view_model| {
                set_test_selections(view_model, &[Selection::new(1, 4, 1, 4), Selection::new(1, 10, 1, 10)]);

                // delete left
                editor.run_command(&*editing.delete_left, None);

                assert_eq!(get_value(&model), "va a = )");
            });
        });
    }

    #[test]
    fn editor_controller_issue_7100_mouse_word_selection_is_strange_when_non_word_character_is_at_the_end_of_line() {
        run_test(|f| {
            let navigation = core_navigation_commands::instance();
            let model = f.create_text_model(
                ["before.a", "before", "hello:", "there:", "this is strange:", "here", "it", "is"].join("\n"),
                None,
                Default::default(),
                None,
            );

            f.with_test_code_editor(model, &no_options(), |_editor, view_model| {
                run_move_command(&navigation.word_select, view_model, 3, 7);
                assert_cursor(view_model, Selection::new(3, 7, 3, 7));

                run_move_command(&navigation.word_select_drag, view_model, 4, 7);
                assert_cursor(view_model, Selection::new(3, 7, 4, 7));
            });
        });
    }

    #[test]
    fn editor_controller_issue_112039_shift_continuing_a_double_triple_click_and_drag_selection_does_not_remember_its_starting_mode() {
        run_test(|f| {
            let navigation = core_navigation_commands::instance();
            let model = f.create_text_model(
                ["just some text", "and another line", "and another one"].join("\n"),
                None,
                Default::default(),
                None,
            );

            f.with_test_code_editor(model, &no_options(), |_editor, view_model| {
                run_move_command(&navigation.word_select, view_model, 2, 6);
                run_move_command(&navigation.move_to_select, view_model, 1, 8);
                assert_cursor(view_model, Selection::new(2, 12, 1, 6));
            });
        });
    }

    #[test]
    fn editor_controller_issue_158236_shift_click_selection_does_not_work_on_line_number_indicator() {
        run_test(|f| {
            let navigation = core_navigation_commands::instance();
            let model = f.create_text_model(
                ["just some text", "and another line", "and another one"].join("\n"),
                None,
                Default::default(),
                None,
            );

            f.with_test_code_editor(model, &no_options(), |_editor, view_model| {
                run_move_command(&navigation.move_to, view_model, 3, 5);
                run_move_command(&navigation.line_select_drag, view_model, 2, 1);
                assert_cursor(view_model, Selection::new(3, 5, 2, 1));
            });
        });
    }

    #[test]
    fn editor_controller_issue_111513_text_gets_automatically_selected_when_typing_at_the_same_location_in_another_editor() {
        run_test(|f| {
            let model = f.create_text_model(["just", "", "some text"].join("\n"), None, Default::default(), None);

            f.with_test_code_editor(model.clone(), &no_options(), |editor1, view_model1| {
                editor1.set_selections(&[Selection::new(2, 1, 2, 1)], None, None);
                f.with_test_code_editor(model.clone(), &no_options(), |editor2, view_model2| {
                    editor2.set_selections(&[Selection::new(2, 1, 2, 1)], None, None);
                    type_text(view_model2, "e", Some("keyboard"));
                    assert_cursor(view_model2, Position::new(2, 2));
                    assert_cursor(view_model1, Position::new(2, 2));
                });
            });
        });
    }

}

#[allow(unused_imports)]
mod part_e {
    use super::*;
use je_editor::view_model_types::IViewModel as _;
use std::cell::Cell;
use std::rc::Rc;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable as _;
use je_editor::code_editor_widget::{ExecuteEditsSource, ICodeEditor};
use je_editor::core_commands::core_editing_commands;
use je_editor::core_commands::core_navigation_commands;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::language_configuration::{AutoClosingPair, IndentAction, LanguageConfiguration};
use je_editor::languages::{EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult};
use je_editor::model::{EndOfLineSequence, RelaxedTextModelCreationOptions, ValidEditOperation};
use je_editor::null_tokenize::null_state;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::tokenization_registry::tokenization_registry;
use support::test_view_model::no_options;
use support::u;
use super::part_b::editor_controller::*;

    // imports do fragmento E:
    use je_editor::config::editor_options::InputMode;
    use je_editor::input_mode::set_input_mode;

    // --------- suítes `Undo stops` e `Overtype Mode`

    /// O corpo comum dos casos de `Undo stops` e `Overtype Mode`: o `ensureNoDisposablesAreLeakedInTestSuite()`, o
    /// `createTextModel(lines.join('\n'), undefined, { insertSpaces })`, o `withTestCodeEditor(model, {}, callback)` e o
    /// `model.dispose()` final.
    fn run_with_model(
        lines: &[&str],
        insert_spaces: Option<bool>,
        callback: impl FnOnce(&ITestCodeEditor, &EditorViewModel, &Rc<TextModel>),
    ) {
        je_editor::core_commands::register_core_commands();
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let model: Rc<TextModel> = create_text_model_with(
            lines.join("\n").as_str(),
            None,
            RelaxedTextModelCreationOptions { insert_spaces, ..Default::default() },
            None,
        );

        with_test_code_editor(model.clone(), TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| {
            callback(editor, view_model, &model);
        });

        model.dispose();
    }

    /// O `setup` e o `teardown` da suíte `Overtype Mode`: o modo de digitação vira `overtype` e volta a `insert` ao fim,
    /// mesmo quando o caso entra em pânico.
    struct OvertypeModeScope;

    impl OvertypeModeScope {
        fn new() -> Self {
            set_input_mode(InputMode::Overtype);
            OvertypeModeScope
        }
    }

    impl Drop for OvertypeModeScope {
        fn drop(&mut self) {
            set_input_mode(InputMode::Insert);
        }
    }

    /// `run_with_model` dentro do `setup`/`teardown` da suíte `Overtype Mode`.
    fn run_overtype(
        lines: &[&str],
        callback: impl FnOnce(&ITestCodeEditor, &EditorViewModel, &Rc<TextModel>),
    ) {
        let _scope = OvertypeModeScope::new();
        run_with_model(lines, Some(false), callback);
    }

    /// O texto de duas linhas `A  line` e `Another line` da suíte `Undo stops`.
    const UNDO_STOPS_LINES: [&str; 2] = ["A  line", "Another line"];

    mod undo_stops {

        use super::*;
        use super::*;

        #[test]
        fn undo_stops_there_is_an_undo_stop_between_typing_and_deleting_left() {
            run_with_model(&UNDO_STOPS_LINES, None, |editor, view_model, model| {
                let editing = core_editing_commands::instance();
                set_test_selections(view_model, &[Selection::new(1, 3, 1, 3)]);
                type_text(view_model, "first", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "A first line");
                assert_cursor(view_model, Selection::new(1, 8, 1, 8));

                editor.run_command(&*editing.delete_left, None);
                editor.run_command(&*editing.delete_left, None);
                assert_eq!(get_line_content(model, 1), "A fir line");
                assert_cursor(view_model, Selection::new(1, 6, 1, 6));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 1), "A first line");
                assert_cursor(view_model, Selection::new(1, 8, 1, 8));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 1), "A  line");
                assert_cursor(view_model, Selection::new(1, 3, 1, 3));
            });
        }

        #[test]
        fn undo_stops_there_is_an_undo_stop_between_typing_and_deleting_right() {
            run_with_model(&UNDO_STOPS_LINES, None, |editor, view_model, model| {
                let editing = core_editing_commands::instance();
                set_test_selections(view_model, &[Selection::new(1, 3, 1, 3)]);
                type_text(view_model, "first", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "A first line");
                assert_cursor(view_model, Selection::new(1, 8, 1, 8));

                editor.run_command(&*editing.delete_right, None);
                editor.run_command(&*editing.delete_right, None);
                assert_eq!(get_line_content(model, 1), "A firstine");
                assert_cursor(view_model, Selection::new(1, 8, 1, 8));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 1), "A first line");
                assert_cursor(view_model, Selection::new(1, 8, 1, 8));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 1), "A  line");
                assert_cursor(view_model, Selection::new(1, 3, 1, 3));
            });
        }

        #[test]
        fn undo_stops_there_is_an_undo_stop_between_deleting_left_and_typing() {
            run_with_model(&UNDO_STOPS_LINES, None, |editor, view_model, model| {
                let editing = core_editing_commands::instance();
                set_test_selections(view_model, &[Selection::new(2, 8, 2, 8)]);
                for _ in 0..7 {
                    editor.run_command(&*editing.delete_left, None);
                }
                assert_eq!(get_line_content(model, 2), " line");
                assert_cursor(view_model, Selection::new(2, 1, 2, 1));

                type_text(view_model, "Second", Some("keyboard"));
                assert_eq!(get_line_content(model, 2), "Second line");
                assert_cursor(view_model, Selection::new(2, 7, 2, 7));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 2), " line");
                assert_cursor(view_model, Selection::new(2, 1, 2, 1));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 2), "Another line");
                assert_cursor(view_model, Selection::new(2, 8, 2, 8));
            });
        }

        #[test]
        fn undo_stops_there_is_an_undo_stop_between_deleting_left_and_deleting_right() {
            run_with_model(&UNDO_STOPS_LINES, None, |editor, view_model, model| {
                let editing = core_editing_commands::instance();
                set_test_selections(view_model, &[Selection::new(2, 8, 2, 8)]);
                for _ in 0..7 {
                    editor.run_command(&*editing.delete_left, None);
                }
                assert_eq!(get_line_content(model, 2), " line");
                assert_cursor(view_model, Selection::new(2, 1, 2, 1));

                for _ in 0..5 {
                    editor.run_command(&*editing.delete_right, None);
                }
                assert_eq!(get_line_content(model, 2), "");
                assert_cursor(view_model, Selection::new(2, 1, 2, 1));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 2), " line");
                assert_cursor(view_model, Selection::new(2, 1, 2, 1));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 2), "Another line");
                assert_cursor(view_model, Selection::new(2, 8, 2, 8));
            });
        }

        #[test]
        fn undo_stops_there_is_an_undo_stop_between_deleting_right_and_typing() {
            run_with_model(&UNDO_STOPS_LINES, None, |editor, view_model, model| {
                let editing = core_editing_commands::instance();
                set_test_selections(view_model, &[Selection::new(2, 9, 2, 9)]);
                for _ in 0..4 {
                    editor.run_command(&*editing.delete_right, None);
                }
                assert_eq!(get_line_content(model, 2), "Another ");
                assert_cursor(view_model, Selection::new(2, 9, 2, 9));

                type_text(view_model, "text", Some("keyboard"));
                assert_eq!(get_line_content(model, 2), "Another text");
                assert_cursor(view_model, Selection::new(2, 13, 2, 13));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 2), "Another ");
                assert_cursor(view_model, Selection::new(2, 9, 2, 9));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 2), "Another line");
                assert_cursor(view_model, Selection::new(2, 9, 2, 9));
            });
        }

        #[test]
        fn undo_stops_there_is_an_undo_stop_between_deleting_right_and_deleting_left() {
            run_with_model(&UNDO_STOPS_LINES, None, |editor, view_model, model| {
                let editing = core_editing_commands::instance();
                set_test_selections(view_model, &[Selection::new(2, 9, 2, 9)]);
                for _ in 0..4 {
                    editor.run_command(&*editing.delete_right, None);
                }
                assert_eq!(get_line_content(model, 2), "Another ");
                assert_cursor(view_model, Selection::new(2, 9, 2, 9));

                for _ in 0..6 {
                    editor.run_command(&*editing.delete_left, None);
                }
                assert_eq!(get_line_content(model, 2), "An");
                assert_cursor(view_model, Selection::new(2, 3, 2, 3));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 2), "Another ");
                assert_cursor(view_model, Selection::new(2, 9, 2, 9));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 2), "Another line");
                assert_cursor(view_model, Selection::new(2, 9, 2, 9));
            });
        }

        #[test]
        fn undo_stops_inserts_undo_stop_when_typing_space() {
            run_with_model(&UNDO_STOPS_LINES, None, |editor, view_model, model| {
                let editing = core_editing_commands::instance();
                set_test_selections(view_model, &[Selection::new(1, 3, 1, 3)]);
                type_text(view_model, "first and interesting", Some("keyboard"));
                assert_eq!(get_line_content(model, 1), "A first and interesting line");
                assert_cursor(view_model, Selection::new(1, 24, 1, 24));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 1), "A first and line");
                assert_cursor(view_model, Selection::new(1, 12, 1, 12));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 1), "A first line");
                assert_cursor(view_model, Selection::new(1, 8, 1, 8));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_line_content(model, 1), "A  line");
                assert_cursor(view_model, Selection::new(1, 3, 1, 3));
            });
        }

        #[test]
        fn undo_stops_can_undo_typing_and_eol_change_in_one_undo_stop() {
            run_with_model(&UNDO_STOPS_LINES, None, |editor, view_model, model| {
                let editing = core_editing_commands::instance();
                set_test_selections(view_model, &[Selection::new(1, 3, 1, 3)]);
                type_text(view_model, "first", Some("keyboard"));
                assert_eq!(get_value(model), "A first line\nAnother line");
                assert_cursor(view_model, Selection::new(1, 8, 1, 8));

                model.push_eol(EndOfLineSequence::CrLf);
                assert_eq!(get_value(model), "A first line\r\nAnother line");
                assert_cursor(view_model, Selection::new(1, 8, 1, 8));

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value(model), "A  line\nAnother line");
                assert_cursor(view_model, Selection::new(1, 3, 1, 3));
            });
        }

        #[test]
        fn undo_stops_issue_93585_undo_multi_cursor_edit_corrupts_document() {
            run_with_model(&["hello world", "hello world"], None, |editor, view_model, model| {
                let editing = core_editing_commands::instance();
                set_test_selections(view_model, &[Selection::new(2, 7, 2, 12), Selection::new(1, 7, 1, 12)]);
                type_text(view_model, "no", Some("keyboard"));
                assert_eq!(get_value(model), "hello no\nhello no");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value(model), "hello world\nhello world");
            });
        }

        #[test]
        fn undo_stops_there_is_a_single_undo_stop_for_consecutive_whitespaces() {
            run_with_model(&[""], Some(false), |editor, view_model, model| {
                let editing = core_editing_commands::instance();
                for chr in ["a", "b", " ", " ", "c", "d"] {
                    type_text(view_model, chr, Some("keyboard"));
                }

                assert_eq!(get_value_lf(model), "ab  cd", "assert1");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value_lf(model), "ab  ", "assert2");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value_lf(model), "ab", "assert3");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value_lf(model), "", "assert4");
            });
        }

        #[test]
        fn undo_stops_there_is_no_undo_stop_after_a_single_whitespace() {
            run_with_model(&[""], Some(false), |editor, view_model, model| {
                let editing = core_editing_commands::instance();
                for chr in ["a", "b", " ", "c", "d"] {
                    type_text(view_model, chr, Some("keyboard"));
                }

                assert_eq!(get_value_lf(model), "ab cd", "assert1");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value_lf(model), "ab", "assert3");

                editor.run_command(&*editing.undo, None);
                assert_eq!(get_value_lf(model), "", "assert4");
            });
        }
    }

    mod overtype_mode {

        use super::*;
        use super::*;

        #[test]
        fn overtype_mode_simple_type() {
            run_overtype(&["123456789", "123456789"], |_editor, view_model, model| {
                set_test_selections(view_model, &[Selection::new(1, 3, 1, 3)]);
                type_text(view_model, "a", Some("keyboard"));
                assert_eq!(get_value_lf(model), ["12a456789", "123456789"].join("\n"), "assert1");

                set_test_selections(view_model, &[Selection::new(1, 9, 1, 9)]);
                type_text(view_model, "bbb", Some("keyboard"));
                assert_eq!(get_value_lf(model), ["12a45678bbb", "123456789"].join("\n"), "assert2");
            });
        }

        #[test]
        fn overtype_mode_multi_line_selection_type() {
            run_overtype(&["123456789", "123456789"], |_editor, view_model, model| {
                set_test_selections(view_model, &[Selection::new(1, 5, 2, 3)]);
                type_text(view_model, "cc", Some("keyboard"));
                assert_eq!(get_value_lf(model), ["1234cc456789"].join("\n"), "assert1");
            });
        }

        #[test]
        fn overtype_mode_simple_paste() {
            run_overtype(&["123456789", "123456789"], |_editor, view_model, model| {
                set_test_selections(view_model, &[Selection::new(1, 5, 1, 5)]);
                view_model.paste(&u("cc"), false, None, None, false);
                assert_eq!(get_value_lf(model), ["1234cc789", "123456789"].join("\n"), "assert1");

                set_test_selections(view_model, &[Selection::new(1, 5, 1, 5)]);
                view_model.paste(&u("dddddddd"), false, None, None, false);
                assert_eq!(get_value_lf(model), ["1234dddddddd", "123456789"].join("\n"), "assert2");
            });
        }

        #[test]
        fn overtype_mode_multi_line_selection_paste() {
            run_overtype(&["123456789", "123456789"], |_editor, view_model, model| {
                set_test_selections(view_model, &[Selection::new(1, 5, 2, 3)]);
                view_model.paste(&u("cc"), false, None, None, false);
                assert_eq!(get_value_lf(model), ["1234cc456789"].join("\n"), "assert1");
            });
        }

        #[test]
        fn overtype_mode_paste_multi_line_text() {
            run_overtype(&["123456789", "123456789"], |_editor, view_model, model| {
                set_test_selections(view_model, &[Selection::new(1, 5, 1, 5)]);
                view_model.paste(&u(&["aaaaaaa", "bbbbbbb"].join("\n")), false, None, None, false);
                assert_eq!(get_value_lf(model), ["1234aaaaaaa", "bbbbbbb", "123456789"].join("\n"), "assert1");
            });
        }

        #[test]
        fn overtype_mode_composition_type() {
            run_overtype(&["123456789", "123456789"], |_editor, view_model, model| {
                set_test_selections(view_model, &[Selection::new(1, 5, 1, 5)]);
                view_model.start_composition();
                view_model.composition_type(&u("セ"), 0, 0, 0, Some("keyboard"));
                assert_eq!(get_value_lf(model), ["1234セ56789", "123456789"].join("\n"), "assert1");

                view_model.end_composition(Some("keyboard"));
                assert_eq!(get_value_lf(model), ["1234セ6789", "123456789"].join("\n"), "assert1");
            });
        }
    }

}
