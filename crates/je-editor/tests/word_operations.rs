//! Porte de `src/vs/editor/contrib/wordOperations/test/browser/wordOperations.test.ts` (suíte `WordOperations`), caso a
//! caso e na mesma ordem, com os auxiliares de `wordTestUtils.ts` em `tests/support/word_test_utils.rs`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup` e o `teardown` da suíte são [`run_test`] (o `LeakGuard`,
//!   os serviços do editor de teste e o descarte, nessa ordem); cada caso roda dentro dele.
//! - `new CursorWordStartLeft()` e os demais comandos da suíte são as funções de `je_editor::word_operations`
//!   (`ops::cursor_word_start_left()`...), que criam o comando sem registrá-lo. Elas são criadas a cada chamada, o que
//!   não muda nada, porque o comando não guarda estado.
//! - `runEditorCommand(editor, command)` é `editor.run_command(...)` com o comando embrulhado em
//!   [`EditorCommandAdapter`], que faz o papel de `ITestEditorCommand` (o `runEditorCommand(accessor, editor, null)` do
//!   original; o `null` chega ao comando como `Dynamic::Null`). `editor.runCommand(CoreEditingCommands.Undo, null)` usa
//!   o `ITestEditorCommand` de `UndoRedoCommand` abaixo.
//! - O auxiliar `cursorWordAccessibilityLeft` do original escolhe os comandos ao contrário (com `inSelectionMode` roda
//!   o sem seleção e sem ele roda o de seleção; o `cursorWordAccessibilityRight` está certo): a inversão foi
//!   preservada, e o caso só lê a posição do cursor.
//! - `isFirefox` é falso no porte, então os casos `Recognize words` não são pulados. Eles dependem do `Intl.Segmenter`
//!   (`wordSegmenterLocales: 'ja'`), que ainda não existe no porte (ver `word_character_classifier`).
//! - `withTestCodeEditor(lines, {}, cb)` é `with_test_code_editor` de `tests/support/test_code_editor.rs` (as opções `{}`
//!   são o `Default`) e `{ wordSegmenterLocales: 'ja' }` é o objeto `Dynamic` das opções.
//! - `viewModel.type(char, 'keyboard')` é `view_model.r#type(&u(char), Some("keyboard"))` (o texto vira UTF-16).
//! - `_deleteInsideWord.run(null!, editor, args)` é `EditorAction::run(&action, None, editor.widget(), &args)`, com o
//!   `args` omitido valendo `Dynamic::Undefined`.
//! - O modelo do caso `issue #91855` usa os serviços de linguagem da suíte (o `instantiateTextModel(instantiationService,
//!   ...)` do original) e é descartado junto com ela.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableStore, IDisposable, to_disposable};
use je_base::promise::MaybePromise;
use je_editor::code_editor_widget::{CodeEditorWidget, ICodeEditor, IEditorCommand};
use je_editor::core_commands::core_editing_commands::{self, UndoRedoCommand};
use je_editor::core_commands::EditorOrNativeTextInputCommand;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::editor_extensions::EditorAction;
use je_editor::language_configuration::{AutoClosingPairConditional, LanguageConfiguration};
use je_editor::language_configuration_registry::ILanguageConfigurationService as _;
use je_editor::language_service::{ILanguageService, I_LANGUAGE_SERVICE};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::position::Position;
use je_editor::selection::Selection;
use je_editor::text_model::{TextModel, TextModelServices};
use je_editor::word_operations::{self as ops, DeleteInsideWord};
use je_instantiation::ServicesAccessor;
use support::test_code_editor::{
    CodeEditorServices, ITestEditorCommand, TestCodeEditor, TestCodeEditorInstantiationOptions,
    create_code_editor_services, instantiate_test_code_editor, with_test_code_editor,
};
use support::test_language_configuration_service::TestLanguageConfigurationService;
use support::test_text_model::resolve_options;
use support::u;
use support::word_test_utils::{
    deserialize_pipe_positions, serialize_pipe_positions, test_repeated_action_and_extract_positions,
};

/// Os `let` da suíte `WordOperations`.
struct Suite {
    disposables: Rc<DisposableStore>,
    instantiation_service: Rc<CodeEditorServices>,
    language_configuration_service: Rc<TestLanguageConfigurationService>,
    language_service: Rc<dyn ILanguageService>,
}

/// O que o mocha faz em volta de cada `test(...)` da suíte: o `ensureNoDisposablesAreLeakedInTestSuite()`, o `setup`, o
/// corpo e o `teardown`, nessa ordem.
fn run_test(body: impl FnOnce(&Suite)) {
    je_editor::core_commands::register_core_commands();
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let disposables = DisposableStore::new();
    let instantiation_service = create_code_editor_services(&disposables, None);
    let language_configuration_service = instantiation_service.language_configuration_service.clone();
    let language_service = instantiation_service.get(I_LANGUAGE_SERVICE);
    let suite = Suite { disposables, instantiation_service, language_configuration_service, language_service };
    body(&suite);
    suite.disposables.dispose();
}

/// `ITestEditorCommand` de um `IEditorCommand` (os comandos de palavra): o `command.runEditorCommand(accessor, editor,
/// null)` do `runEditorCommand` da suíte.
struct EditorCommandAdapter<'a, C: IEditorCommand>(&'a C);

impl<C: IEditorCommand> ITestEditorCommand for EditorCommandAdapter<'_, C> {
    fn run_editor_command(
        &self,
        accessor: &ServicesAccessor,
        editor: &Rc<CodeEditorWidget>,
        args: Option<&Dynamic>,
    ) -> MaybePromise<()> {
        let args = args.cloned().unwrap_or(Dynamic::Null);
        MaybePromise::Promise(IEditorCommand::run_editor_command(self.0, Some(accessor), editor, &args))
    }
}

/// `CoreEditingCommands.Undo` como argumento de `editor.runCommand(command, args)`: o `runEditorCommand` do
/// `EditorOrNativeTextInputCommand`.
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

/// `runEditorCommand(editor, command)`.
fn run_editor_command(editor: &TestCodeEditor, command: &impl IEditorCommand) {
    editor.run_command(&EditorCommandAdapter(command), None);
}

fn cursor_word_left(editor: &TestCodeEditor, in_selection_mode: bool) {
    run_editor_command(editor, &if in_selection_mode { ops::cursor_word_left_select() } else { ops::cursor_word_left() });
}

/// O original escolhe aqui o comando ao contrário (ver as notas de tradução).
fn cursor_word_accessibility_left(editor: &TestCodeEditor, in_selection_mode: bool) {
    run_editor_command(
        editor,
        &if in_selection_mode { ops::cursor_word_accessibility_left() } else { ops::cursor_word_accessibility_left_select() },
    );
}

fn cursor_word_accessibility_right(editor: &TestCodeEditor, in_selection_mode: bool) {
    run_editor_command(
        editor,
        &if in_selection_mode { ops::cursor_word_accessibility_right_select() } else { ops::cursor_word_accessibility_right() },
    );
}

fn cursor_word_start_left(editor: &TestCodeEditor, in_selection_mode: bool) {
    run_editor_command(
        editor,
        &if in_selection_mode { ops::cursor_word_start_left_select() } else { ops::cursor_word_start_left() },
    );
}

fn cursor_word_end_left(editor: &TestCodeEditor, in_selection_mode: bool) {
    run_editor_command(
        editor,
        &if in_selection_mode { ops::cursor_word_end_left_select() } else { ops::cursor_word_end_left() },
    );
}

fn cursor_word_right(editor: &TestCodeEditor, in_selection_mode: bool) {
    run_editor_command(editor, &if in_selection_mode { ops::cursor_word_right_select() } else { ops::cursor_word_right() });
}

fn move_word_end_right(editor: &TestCodeEditor, in_selection_mode: bool) {
    run_editor_command(
        editor,
        &if in_selection_mode { ops::cursor_word_end_right_select() } else { ops::cursor_word_end_right() },
    );
}

fn move_word_start_right(editor: &TestCodeEditor, in_selection_mode: bool) {
    run_editor_command(
        editor,
        &if in_selection_mode { ops::cursor_word_start_right_select() } else { ops::cursor_word_start_right() },
    );
}

fn delete_word_left(editor: &TestCodeEditor) {
    run_editor_command(editor, &ops::delete_word_left());
}

fn delete_word_start_left(editor: &TestCodeEditor) {
    run_editor_command(editor, &ops::delete_word_start_left());
}

fn delete_word_end_left(editor: &TestCodeEditor) {
    run_editor_command(editor, &ops::delete_word_end_left());
}

fn delete_word_right(editor: &TestCodeEditor) {
    run_editor_command(editor, &ops::delete_word_right());
}

fn delete_word_start_right(editor: &TestCodeEditor) {
    run_editor_command(editor, &ops::delete_word_start_right());
}

fn delete_word_end_right(editor: &TestCodeEditor) {
    run_editor_command(editor, &ops::delete_word_end_right());
}

fn delete_inside_word(editor: &TestCodeEditor, args: Option<&Dynamic>) {
    let _ = EditorAction::run(&DeleteInsideWord::new(), None, editor.widget(), args.unwrap_or(&Dynamic::Undefined));
}

/// `ed.getPosition()!`.
fn position_of(editor: &TestCodeEditor) -> Position {
    editor.get_position().expect("o editor tem o modelo")
}

/// `ed.getValue().length`: o tamanho em unidades UTF-16.
fn value_length(editor: &TestCodeEditor) -> i32 {
    editor.get_value(None).units().len() as i32
}

/// `text.length`: o tamanho em unidades UTF-16.
fn text_length(text: &str) -> i32 {
    text.encode_utf16().count() as i32
}

/// `model.getLineContent(lineNumber)`.
fn line_content(model: &TextModel, line_number: i32) -> String {
    CursorSimpleModel::get_line_content(model, line_number).to_string_lossy()
}

/// `model.getValue()`.
fn value_of(model: &TextModel) -> String {
    model.get_value().to_string_lossy()
}

/// `assert.deepStrictEqual(editor.getSelection(), selection)`.
fn assert_selection(editor: &TestCodeEditor, expected: Selection) {
    assert_eq!(editor.get_selection().expect("o editor tem o modelo").to_string(), expected.to_string());
}

/// O corpo que quase todos os casos de movimento repetem: `deserializePipePositions(EXPECTED)`,
/// `testRepeatedActionAndExtractPositions(...)`, `serializePipePositions(text, actualStops)` e o
/// `assert.deepStrictEqual(actual, EXPECTED)`.
fn assert_repeated_action_stops(
    expected: &str,
    initial_position: Position,
    action: impl Fn(&TestCodeEditor),
    record: impl Fn(&TestCodeEditor) -> Position,
    stop_condition: impl Fn(&TestCodeEditor) -> bool,
    options: TestCodeEditorInstantiationOptions,
) {
    let (text, _) = deserialize_pipe_positions(expected);
    let actual_stops =
        test_repeated_action_and_extract_positions(&text, initial_position, action, record, stop_condition, options);
    let actual = serialize_pipe_positions(&text, actual_stops);
    assert_eq!(actual, expected);
}

/// `ed => ed.getPosition()!.equals(new Position(line, column))`.
fn is_at(line_number: i32, column: i32) -> impl Fn(&TestCodeEditor) -> bool {
    move |editor: &TestCodeEditor| position_of(editor) == Position::new(line_number, column)
}

/// `{ wordSegmenterLocales: locales }`.
fn word_segmenter_locales(locales: &str) -> TestCodeEditorInstantiationOptions {
    TestCodeEditorInstantiationOptions::new(Dynamic::object(vec![("wordSegmenterLocales", Dynamic::from(locales))]))
}

/// As linhas que a maioria dos casos usa.
const SAMPLE_LINES: &[&str] = &["    \tMy First Line\t ", "\tMy Second Line", "    Third Line🐶", "", "1"];

#[test]
fn cursor_word_left_simple() {
    run_test(|_| {
        let expected = ["|    \t|My |First |Line\t ", "|\t|My |Second |Line", "|    |Third |Line🐶", "|", "|1"].join("\n");
        assert_repeated_action_stops(
            &expected,
            Position::new(1000, 1000),
            |ed| cursor_word_left(ed, false),
            position_of,
            is_at(1, 1),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn cursor_word_left_with_selection() {
    run_test(|_| {
        with_test_code_editor(SAMPLE_LINES, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            editor.set_position(Position::new(5, 2), None);
            cursor_word_left(editor, true);
            assert_selection(editor, Selection::new(5, 2, 5, 1));
        });
    });
}

#[test]
fn cursor_word_left_issue_832() {
    run_test(|_| {
        let expected = "|   |/* |Just |some   |more   |text |a|+= |3 |+|5-|3 |+ |7 |*/  ";
        assert_repeated_action_stops(
            expected,
            Position::new(1000, 1000),
            |ed| cursor_word_left(ed, false),
            position_of,
            is_at(1, 1),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn cursor_word_left_issue_48046_word_selection_doesnt_work_as_usual() {
    run_test(|_| {
        let expected = "|deep.|object.|property";
        assert_repeated_action_stops(
            expected,
            Position::new(1, 21),
            |ed| cursor_word_left(ed, false),
            position_of,
            is_at(1, 1),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn cursor_word_left_recognize_words() {
    run_test(|_| {
        let expected = "|/* |これ|は|テスト|です |/*";
        assert_repeated_action_stops(
            expected,
            Position::new(1000, 1000),
            |ed| cursor_word_left(ed, true),
            position_of,
            is_at(1, 1),
            word_segmenter_locales("ja"),
        );
    });
}

#[test]
fn cursor_word_left_does_not_recognize_words() {
    run_test(|_| {
        let expected = "|/* |これはテストです |/*";
        assert_repeated_action_stops(
            expected,
            Position::new(1000, 1000),
            |ed| cursor_word_left(ed, true),
            position_of,
            is_at(1, 1),
            word_segmenter_locales(""),
        );
    });
}

#[test]
fn cursor_word_left_issue_169904_cursors_out_of_sync() {
    run_test(|_| {
        let text = [
            ".grid1 {",
            "  display: grid;",
            "  grid-template-columns:",
            "    [full-start] minmax(1em, 1fr)",
            "    [main-start] minmax(0, 40em) [main-end]",
            "    minmax(1em, 1fr) [full-end];",
            "}",
            ".grid2 {",
            "  display: grid;",
            "  grid-template-columns:",
            "    [full-start] minmax(1em, 1fr)",
            "    [main-start] minmax(0, 40em) [main-end] minmax(1em, 1fr) [full-end];",
            "}",
        ];
        with_test_code_editor(&text[..], TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            editor.set_selections(
                &[
                    Selection::new(5, 44, 5, 44),
                    Selection::new(6, 32, 6, 32),
                    Selection::new(12, 44, 12, 44),
                    Selection::new(12, 72, 12, 72),
                ],
                None,
                None,
            );
            cursor_word_left(editor, false);
            let actual: Vec<String> =
                editor.get_selections().expect("o editor tem o modelo").iter().map(ToString::to_string).collect();
            let expected: Vec<String> = [
                Selection::new(5, 43, 5, 43),
                Selection::new(6, 31, 6, 31),
                Selection::new(12, 43, 12, 43),
                Selection::new(12, 71, 12, 71),
            ]
            .iter()
            .map(ToString::to_string)
            .collect();
            assert_eq!(actual, expected);
        });
    });
}

#[test]
fn cursor_word_left_select_issue_74369_cursor_word_left_and_cursor_word_left_select_do_not_behave_consistently() {
    run_test(|_| {
        let expected = "|this.|is.|a.|test";
        assert_repeated_action_stops(
            expected,
            Position::new(1, 15),
            |ed| cursor_word_left(ed, true),
            position_of,
            is_at(1, 1),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn cursor_word_start_left_stops() {
    run_test(|_| {
        // This is the behaviour observed in Visual Studio, please do not touch test
        let expected = "|   |/* |Just |some   |more   |text |a|+= |3 |+|5|-|3 |+ |7 |*/  ";
        assert_repeated_action_stops(
            expected,
            Position::new(1000, 1000),
            |ed| cursor_word_start_left(ed, false),
            position_of,
            is_at(1, 1),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn cursor_word_start_left_issue_51119_regression_makes_vs_compatibility_impossible() {
    run_test(|_| {
        // This is the behaviour observed in Visual Studio, please do not touch test
        let expected = "|this|.|is|.|a|.|test";
        assert_repeated_action_stops(
            expected,
            Position::new(1000, 1000),
            |ed| cursor_word_start_left(ed, false),
            position_of,
            is_at(1, 1),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn issue_51275_cursor_word_start_left_does_not_push_undo_redo_stack_element() {
    run_test(|_| {
        with_test_code_editor("", TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| {
            // `function type(viewModel, text)`
            let type_text = |text: &str| {
                for character in text.chars() {
                    view_model.r#type(&u(&character.to_string()), Some("keyboard"));
                }
            };

            type_text("foo bar baz");
            assert_eq!(editor.get_value(None).to_string_lossy(), "foo bar baz");

            cursor_word_start_left(editor, false);
            cursor_word_start_left(editor, false);
            type_text("q");

            assert_eq!(editor.get_value(None).to_string_lossy(), "foo qbar baz");

            editor.run_command(&*core_editing_commands::instance().undo, None);
            assert_eq!(editor.get_value(None).to_string_lossy(), "foo bar baz");
        });
    });
}

#[test]
fn cursor_word_end_left_stops() {
    run_test(|_| {
        let expected = "|   /*| Just| some|   more|   text| a|+=| 3| +|5|-|3| +| 7| */|  ";
        assert_repeated_action_stops(
            expected,
            Position::new(1000, 1000),
            |ed| cursor_word_end_left(ed, false),
            position_of,
            is_at(1, 1),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn cursor_word_right_simple() {
    run_test(|_| {
        let expected = ["    \tMy| First| Line|\t |", "\tMy| Second| Line|", "    Third| Line🐶|", "|", "1|"].join("\n");
        assert_repeated_action_stops(
            &expected,
            Position::new(1, 1),
            |ed| cursor_word_right(ed, false),
            position_of,
            is_at(5, 2),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn cursor_word_right_selection() {
    run_test(|_| {
        with_test_code_editor(SAMPLE_LINES, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            editor.set_position(Position::new(1, 1), None);
            cursor_word_right(editor, true);
            assert_selection(editor, Selection::new(1, 1, 1, 8));
        });
    });
}

#[test]
fn cursor_word_right_issue_832() {
    run_test(|_| {
        let expected = "   /*| Just| some|   more|   text| a|+=| 3| +5|-3| +| 7| */|  |";
        assert_repeated_action_stops(
            expected,
            Position::new(1, 1),
            |ed| cursor_word_right(ed, false),
            position_of,
            is_at(1, 50),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn cursor_word_right_issue_41199() {
    run_test(|_| {
        let expected = "console|.log|(err|)|";
        assert_repeated_action_stops(
            expected,
            Position::new(1, 1),
            |ed| cursor_word_right(ed, false),
            position_of,
            is_at(1, 17),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn cursor_word_right_recognize_words() {
    run_test(|_| {
        let expected = "/*| これ|は|テスト|です|/*|";
        assert_repeated_action_stops(
            expected,
            Position::new(1, 1),
            |ed| cursor_word_right(ed, false),
            position_of,
            is_at(1, 14),
            word_segmenter_locales("ja"),
        );
    });
}

#[test]
fn cursor_word_right_does_not_recognize_words() {
    run_test(|_| {
        let expected = "/*| これはテストです|/*|";
        assert_repeated_action_stops(
            expected,
            Position::new(1, 1),
            |ed| cursor_word_right(ed, false),
            position_of,
            is_at(1, 14),
            word_segmenter_locales(""),
        );
    });
}

#[test]
fn move_word_end_right_test() {
    run_test(|_| {
        let expected = "   /*| Just| some|   more|   text| a|+=| 3| +5|-3| +| 7| */|  |";
        assert_repeated_action_stops(
            expected,
            Position::new(1, 1),
            |ed| move_word_end_right(ed, false),
            position_of,
            is_at(1, 50),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn move_word_start_right_test() {
    run_test(|_| {
        // This is the behaviour observed in Visual Studio, please do not touch test
        let expected = "   |/* |Just |some   |more   |text |a|+= |3 |+|5|-|3 |+ |7 |*/  |";
        assert_repeated_action_stops(
            expected,
            Position::new(1, 1),
            |ed| move_word_start_right(ed, false),
            position_of,
            is_at(1, 50),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn issue_51119_cursor_word_start_right_regression_makes_vs_compatibility_impossible() {
    run_test(|_| {
        // This is the behaviour observed in Visual Studio, please do not touch test
        let expected = "this|.|is|.|a|.|test|";
        assert_repeated_action_stops(
            expected,
            Position::new(1, 1),
            |ed| move_word_start_right(ed, false),
            position_of,
            is_at(1, 15),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn issue_64810_cursor_word_start_right_skips_first_word_after_newline() {
    run_test(|_| {
        // This is the behaviour observed in Visual Studio, please do not touch test
        let expected = ["Hello |World|", "|Hei |mailman|"].join("\n");
        assert_repeated_action_stops(
            &expected,
            Position::new(1, 1),
            |ed| move_word_start_right(ed, false),
            position_of,
            is_at(2, 12),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn cursor_word_accessibility_left_test() {
    run_test(|_| {
        let expected = "|   /* |Just |some   |more   |text |a+= |3 +|5-|3 + |7 */  ";
        assert_repeated_action_stops(
            expected,
            Position::new(1000, 1000),
            |ed| cursor_word_accessibility_left(ed, false),
            position_of,
            is_at(1, 1),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn cursor_word_accessibility_right_test() {
    run_test(|_| {
        let expected = "   /* |Just |some   |more   |text |a+= |3 +|5-|3 + |7 */  |";
        assert_repeated_action_stops(
            expected,
            Position::new(1, 1),
            |ed| cursor_word_accessibility_right(ed, false),
            position_of,
            is_at(1, 50),
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn delete_word_left_for_non_empty_selection() {
    run_test(|_| {
        with_test_code_editor(SAMPLE_LINES, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_selection(Selection::new(3, 7, 3, 9).into(), None);
            delete_word_left(editor);
            assert_eq!(line_content(&model, 3), "    Thd Line🐶");
            assert_eq!(editor.get_position(), Some(Position::new(3, 7)));
        });
    });
}

#[test]
fn delete_word_left_for_cursor_at_beginning_of_document() {
    run_test(|_| {
        with_test_code_editor(SAMPLE_LINES, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 1), None);
            delete_word_left(editor);
            assert_eq!(line_content(&model, 1), "    \tMy First Line\t ");
            assert_eq!(editor.get_position(), Some(Position::new(1, 1)));
        });
    });
}

#[test]
fn delete_word_left_for_cursor_at_end_of_whitespace() {
    run_test(|_| {
        with_test_code_editor(SAMPLE_LINES, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(3, 11), None);
            delete_word_left(editor);
            assert_eq!(line_content(&model, 3), "    Line🐶");
            assert_eq!(editor.get_position(), Some(Position::new(3, 5)));
        });
    });
}

#[test]
fn delete_word_left_for_cursor_just_behind_a_word() {
    run_test(|_| {
        with_test_code_editor(SAMPLE_LINES, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(2, 11), None);
            delete_word_left(editor);
            assert_eq!(line_content(&model, 2), "\tMy  Line");
            assert_eq!(editor.get_position(), Some(Position::new(2, 5)));
        });
    });
}

#[test]
fn delete_word_left_for_cursor_inside_of_a_word() {
    run_test(|_| {
        with_test_code_editor(SAMPLE_LINES, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 12), None);
            delete_word_left(editor);
            assert_eq!(line_content(&model, 1), "    \tMy st Line\t ");
            assert_eq!(editor.get_position(), Some(Position::new(1, 9)));
        });
    });
}

#[test]
fn delete_word_right_for_non_empty_selection() {
    run_test(|_| {
        with_test_code_editor(SAMPLE_LINES, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_selection(Selection::new(3, 7, 3, 9).into(), None);
            delete_word_right(editor);
            assert_eq!(line_content(&model, 3), "    Thd Line🐶");
            assert_eq!(editor.get_position(), Some(Position::new(3, 7)));
        });
    });
}

#[test]
fn delete_word_right_for_cursor_at_end_of_document() {
    run_test(|_| {
        with_test_code_editor(SAMPLE_LINES, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(5, 3), None);
            delete_word_right(editor);
            assert_eq!(line_content(&model, 5), "1");
            assert_eq!(editor.get_position(), Some(Position::new(5, 2)));
        });
    });
}

#[test]
fn delete_word_right_for_cursor_at_beggining_of_whitespace() {
    run_test(|_| {
        with_test_code_editor(SAMPLE_LINES, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(3, 1), None);
            delete_word_right(editor);
            assert_eq!(line_content(&model, 3), "Third Line🐶");
            assert_eq!(editor.get_position(), Some(Position::new(3, 1)));
        });
    });
}

#[test]
fn delete_word_right_for_cursor_just_before_a_word() {
    run_test(|_| {
        with_test_code_editor(SAMPLE_LINES, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(2, 5), None);
            delete_word_right(editor);
            assert_eq!(line_content(&model, 2), "\tMy  Line");
            assert_eq!(editor.get_position(), Some(Position::new(2, 5)));
        });
    });
}

#[test]
fn delete_word_right_for_cursor_inside_of_a_word() {
    run_test(|_| {
        with_test_code_editor(SAMPLE_LINES, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 11), None);
            delete_word_right(editor);
            assert_eq!(line_content(&model, 1), "    \tMy Fi Line\t ");
            assert_eq!(editor.get_position(), Some(Position::new(1, 11)));
        });
    });
}

#[test]
fn delete_word_left_issue_832() {
    run_test(|_| {
        let expected = "|   |/* |Just |some |text |a|+= |3 |+|5 |*/|  ";
        assert_repeated_action_stops(
            expected,
            Position::new(1000, 10000),
            |ed| delete_word_left(ed),
            position_of,
            |ed| value_length(ed) == 0,
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn delete_word_start_left_test() {
    run_test(|_| {
        let expected = "|   |/* |Just |some |text |a|+= |3 |+|5 |*/  ";
        assert_repeated_action_stops(
            expected,
            Position::new(1000, 10000),
            |ed| delete_word_start_left(ed),
            position_of,
            |ed| value_length(ed) == 0,
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn delete_word_end_left_test() {
    run_test(|_| {
        let expected = "|   /*| Just| some| text| a|+=| 3| +|5| */|  ";
        assert_repeated_action_stops(
            expected,
            Position::new(1000, 10000),
            |ed| delete_word_end_left(ed),
            position_of,
            |ed| value_length(ed) == 0,
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn delete_word_left_issue_24947() {
    run_test(|_| {
        let lines: &[&str] = &["{", "}"];

        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(2, 1), None);
            delete_word_left(editor);
            assert_eq!(line_content(&model, 1), "{}");
        });

        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(2, 1), None);
            delete_word_start_left(editor);
            assert_eq!(line_content(&model, 1), "{}");
        });

        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(2, 1), None);
            delete_word_end_left(editor);
            assert_eq!(line_content(&model, 1), "{}");
        });
    });
}

/// `ed => new Position(1, text.length - ed.getValue().length + 1)`: o `record` dos casos que apagam para a direita.
fn deleted_length_position(text: String) -> impl Fn(&TestCodeEditor) -> Position {
    move |editor: &TestCodeEditor| Position::new(1, text_length(&text) - value_length(editor) + 1)
}

#[test]
fn delete_word_right_issue_832() {
    run_test(|_| {
        let expected = "   |/*| |Just| |some| |text| |a|+=| |3| |+|5|-|3| |*/|  |";
        let (text, _) = deserialize_pipe_positions(expected);
        assert_repeated_action_stops(
            expected,
            Position::new(1, 1),
            |ed| delete_word_right(ed),
            deleted_length_position(text),
            |ed| value_length(ed) == 0,
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn delete_word_right_issue_3882() {
    run_test(|_| {
        let lines: &[&str] = &["public void Add( int x,", "                 int y )"];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 24), None);
            delete_word_right(editor);
            assert_eq!(line_content(&model, 1), "public void Add( int x,int y )", "001");
        });
    });
}

#[test]
fn delete_word_start_right_issue_3882() {
    run_test(|_| {
        let lines: &[&str] = &["public void Add( int x,", "                 int y )"];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 24), None);
            delete_word_start_right(editor);
            assert_eq!(line_content(&model, 1), "public void Add( int x,int y )", "001");
        });
    });
}

#[test]
fn delete_word_end_right_issue_3882() {
    run_test(|_| {
        let lines: &[&str] = &["public void Add( int x,", "                 int y )"];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 24), None);
            delete_word_end_right(editor);
            assert_eq!(line_content(&model, 1), "public void Add( int x,int y )", "001");
        });
    });
}

#[test]
fn delete_word_start_right_test() {
    run_test(|_| {
        let expected = "   |/* |Just |some |text |a|+= |3 |+|5|-|3 |*/  |";
        let (text, _) = deserialize_pipe_positions(expected);
        assert_repeated_action_stops(
            expected,
            Position::new(1, 1),
            |ed| delete_word_start_right(ed),
            deleted_length_position(text),
            |ed| value_length(ed) == 0,
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn delete_word_end_right_test() {
    run_test(|_| {
        let expected = "   /*| Just| some| text| a|+=| 3| +|5|-|3| */|  |";
        let (text, _) = deserialize_pipe_positions(expected);
        assert_repeated_action_stops(
            expected,
            Position::new(1, 1),
            |ed| delete_word_end_right(ed),
            deleted_length_position(text),
            |ed| value_length(ed) == 0,
            TestCodeEditorInstantiationOptions::default(),
        );
    });
}

#[test]
fn delete_word_right_issue_3882_1_ctrl_delete_removing_entire_line_when_used_at_the_end_of_line() {
    run_test(|_| {
        let lines: &[&str] = &["A line with text.", "   And another one"];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 18), None);
            delete_word_right(editor);
            assert_eq!(line_content(&model, 1), "A line with text.And another one", "001");
        });
    });
}

#[test]
fn delete_word_left_issue_3882_2_ctrl_delete_removing_entire_line_when_used_at_the_end_of_line() {
    run_test(|_| {
        let lines: &[&str] = &["A line with text.", "   And another one"];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(2, 1), None);
            delete_word_left(editor);
            assert_eq!(line_content(&model, 1), "A line with text.   And another one", "001");
        });
    });
}

#[test]
fn delete_word_left_issue_91855_matching_quote_bracket_paren_doesnt_get_deleted_when_hitting_ctrl_backspace() {
    run_test(|suite| {
        let language_id = "myTestMode";

        suite.disposables.add(suite.language_service.register_language(LanguageExtensionPoint::new(language_id)));
        suite.disposables.add(suite.language_configuration_service.register(
            language_id,
            LanguageConfiguration {
                auto_closing_pairs: Some(vec![AutoClosingPairConditional::new("\"", "\"")]),
                ..LanguageConfiguration::default()
            },
            None,
        ));

        let model = TextModel::new_with_language_service(
            "a \"\"".to_owned().into(),
            language_id,
            resolve_options(&RelaxedTextModelCreationOptions::default()),
            None,
            suite.instantiation_service.undo_redo_service.clone(),
            Some(TextModelServices {
                language_service: suite.language_service.clone(),
                language_configuration_service: suite.language_configuration_service.clone(),
            }),
        );
        let owned = model.clone();
        suite.disposables.add(to_disposable(move || owned.dispose()));
        let options = Dynamic::object(vec![("autoClosingDelete", Dynamic::from("always"))]);
        let editor = suite
            .disposables
            .add_rc(instantiate_test_code_editor(&suite.instantiation_service, Some(model.clone()), &options));

        editor.set_position(Position::new(1, 4), None);
        delete_word_left(&editor);
        assert_eq!(line_content(&model, 1), "a ");
    });
}

#[test]
fn delete_inside_word_empty_line() {
    run_test(|_| {
        let lines: &[&str] = &["Line1", "", "Line2"];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(2, 1), None);
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "Line1\nLine2");
        });
    });
}

#[test]
fn delete_inside_word_in_whitespace_1() {
    run_test(|_| {
        let lines: &[&str] = &["Just  some text."];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 6), None);
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "Justsome text.");
        });
    });
}

#[test]
fn delete_inside_word_in_whitespace_2() {
    run_test(|_| {
        let lines: &[&str] = &["Just     some text."];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 6), None);
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "Justsome text.");
        });
    });
}

#[test]
fn delete_inside_word_in_whitespace_3() {
    run_test(|_| {
        let lines: &[&str] = &["Just     \"some text."];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 6), None);
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "Just\"some text.");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "\"some text.");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "some text.");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "text.");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), ".");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "");
        });
    });
}

#[test]
fn delete_inside_word_in_non_words() {
    run_test(|_| {
        let lines: &[&str] = &["x=3+4+5+6"];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 7), None);
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "x=3+45+6");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "x=3++6");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "x=36");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "x=");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "x");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "");
        });
    });
}

#[test]
fn delete_inside_word_in_words_1() {
    run_test(|_| {
        let lines: &[&str] = &["This is interesting"];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 7), None);
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "This interesting");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "This");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "");
        });
    });
}

#[test]
fn delete_inside_word_in_words_2() {
    run_test(|_| {
        let lines: &[&str] = &["This  is  interesting"];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 7), None);
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "This  interesting");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "This");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "");
            delete_inside_word(editor, None);
            assert_eq!(value_of(&model), "");
        });
    });
}

#[test]
fn delete_inside_word_only_word_does_not_delete_whitespace_before_last_word() {
    run_test(|_| {
        let lines: &[&str] = &["hello world"];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 9), None);
            delete_inside_word(editor, Some(&Dynamic::object(vec![("onlyWord", Dynamic::from(true))])));
            assert_eq!(value_of(&model), "hello ");
        });
    });
}

#[test]
fn delete_inside_word_only_word_deletes_just_the_word_leaves_double_spaces() {
    run_test(|_| {
        let lines: &[&str] = &["This is interesting"];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let model = editor.get_model().expect("o editor tem o modelo");
            editor.set_position(Position::new(1, 7), None);
            delete_inside_word(editor, Some(&Dynamic::object(vec![("onlyWord", Dynamic::from(true))])));
            assert_eq!(value_of(&model), "This  interesting");
        });
    });
}
