//! Porte de `src/vs/editor/contrib/linesOperations/test/browser/linesOperations.test.ts` (a suíte
//! `Editor Contrib - Line Operations`), caso a caso e na mesma ordem. Cada `suite` do original vira um `mod`.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` e o registro dos comandos do núcleo são [`run_test`]; cada caso roda
//!   dentro dele.
//! - `new SortLinesAscendingAction()` e as demais são as funções e construtores de `je_editor::lines_operations`
//!   (`sort_lines_ascending_action()`, `DeleteDuplicateLinesAction::new()`...).
//! - `executeAction(action, editor)` é [`execute_action`]: `EditorAction::run(&action, None, editor.widget(), &Dynamic::Undefined)`
//!   (o `null!` do accessor é `None` e o `undefined` dos argumentos é `Dynamic::Undefined`).
//! - `withTestCodeEditor(lines, {}, cb)` é [`with_editor`] sobre `with_test_code_editor` de
//!   `tests/support/test_code_editor.rs` (as opções `{}` são o `Default`).
//! - `assert.deepStrictEqual(editor.getSelections(), [...])` compara a representação de texto de cada seleção
//!   (`[âncora -> posição]`), que carrega a direção, como em `tests/word_operations.rs`.
//! - `editor.runCommand(CoreEditingCommands.Undo/Redo, null)` usa o `ITestEditorCommand` de `UndoRedoCommand` e
//!   `CoreEditingCommands.Tab` o adaptador de `IEditorCommand`, como em `tests/word_operations.rs`.
//! - Em `InsertLineBeforeAction` e `InsertLineAfterAction` o `viewModel.getSelection()` é o
//!   `editor.getSelection()`, que devolve exatamente o do `ViewModel`.
//! - `createTextModel(text, undefined, { insertSpaces: false })` é `create_text_model_with`, e o `model.dispose()` do
//!   fim do caso é o do porte.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable as _;
use je_base::promise::MaybePromise;
use je_editor::code_editor_widget::{CodeEditorWidget, Handler, ICodeEditor, IEditorCommand};
use je_editor::core_commands::EditorOrNativeTextInputCommand;
use je_editor::core_commands::core_editing_commands::{self, UndoRedoCommand};
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::editor_extensions::EditorAction;
use je_editor::lines_operations::{
    DeleteDuplicateLinesAction, DeleteLinesAction, IndentLinesAction, JoinLinesAction, ReverseLinesAction,
    TransposeAction, camel_case_action, delete_all_left_action, delete_all_right_action, insert_line_after_action,
    insert_line_before_action, kebab_case_action, lower_case_action, pascal_case_action, snake_case_action,
    sort_lines_ascending_action, sort_lines_descending_action, title_case_action, upper_case_action,
};
use je_editor::model::RelaxedTextModelCreationOptions;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::text_model::TextModel;
use je_instantiation::ServicesAccessor;
use support::test_code_editor::{
    ITestEditorCommand, TestCodeEditor, TestCodeEditorInstantiationOptions, with_test_code_editor,
};
use support::test_text_model::{create_text_model, create_text_model_with};

/// O que o mocha faz em volta de cada `test(...)`: o `ensureNoDisposablesAreLeakedInTestSuite()` e o corpo.
fn run_test(body: impl FnOnce()) {
    je_editor::core_commands::register_core_commands();
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    body();
}

/// `ITestEditorCommand` de um `IEditorCommand` (o `Tab`): o `runEditorCommand(accessor, editor, null)` do original.
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

/// `CoreEditingCommands.Undo` e `Redo` como argumento de `editor.runCommand(command, args)`.
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

/// `withTestCodeEditor(lines, {}, (editor) => ...)`.
fn with_editor(lines: &[&str], callback: impl FnOnce(&TestCodeEditor)) {
    with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, _, _| callback(editor));
}

/// `executeAction(action, editor)`.
fn execute_action(action: &impl EditorAction, editor: &TestCodeEditor) {
    let _ = action.run(None, editor.widget(), &Dynamic::Undefined);
}

/// `editor.runCommand(CoreEditingCommands.Undo, null)`.
fn undo(editor: &TestCodeEditor) {
    let _ = editor.run_command(&*core_editing_commands::instance().undo, None);
}

/// `editor.runCommand(CoreEditingCommands.Redo, null)`.
fn redo(editor: &TestCodeEditor) {
    let _ = editor.run_command(&*core_editing_commands::instance().redo, None);
}

/// `editor.runCommand(CoreEditingCommands.Tab, null)`.
fn tab(editor: &TestCodeEditor) {
    let _ = editor.run_command(&EditorCommandAdapter(&*core_editing_commands::instance().tab), None);
}

/// `editor.trigger('keyboard', Handler.Type, { text })`.
fn type_text(editor: &TestCodeEditor, text: &str) {
    editor.trigger(Some("keyboard"), Handler::TYPE, Some(Dynamic::object(vec![("text", Dynamic::from(text))])));
}

/// `editor.setSelection(selection)`.
fn set_selection(editor: &TestCodeEditor, selection: Selection) {
    editor.set_selection(selection.into(), None);
}

/// `editor.setSelections(selections)`.
fn set_selections(editor: &TestCodeEditor, selections: &[Selection]) {
    editor.set_selections(selections, None, None);
}

/// `editor.getSelections()`.
fn selections_of(editor: &TestCodeEditor) -> Vec<Selection> {
    editor.get_selections().expect("o editor tem o modelo")
}

/// `editor.getSelection()`.
fn selection_of(editor: &TestCodeEditor) -> Selection {
    editor.get_selection().expect("o editor tem o modelo")
}

/// `editor.getModel()!`.
fn model_of(editor: &TestCodeEditor) -> Rc<TextModel> {
    editor.get_model().expect("o editor tem o modelo")
}

fn selection_strings(selections: &[Selection]) -> Vec<String> {
    selections.iter().map(ToString::to_string).collect()
}

/// `assert.deepStrictEqual(editor.getSelections(), expected)`.
fn assert_selections(editor: &TestCodeEditor, expected: &[Selection]) {
    assert_eq!(selection_strings(&selections_of(editor)), selection_strings(expected));
}

/// `assertSelection(editor, expected)` com uma seleção só.
fn assert_selection(editor: &TestCodeEditor, expected: Selection) {
    assert_selections(editor, &[expected]);
}

/// `[startLineNumber, startColumn, endLineNumber, endColumn]` de uma seleção.
fn bounds(selection: &Selection) -> [i32; 4] {
    [selection.start_line_number, selection.start_column, selection.end_line_number, selection.end_column]
}

/// `model.getLineContent(lineNumber)`.
fn line_content(model: &TextModel, line_number: i32) -> String {
    CursorSimpleModel::get_line_content(model, line_number).to_string_lossy()
}

/// `model.getLinesContent()`.
fn lines_content(model: &TextModel) -> Vec<String> {
    model.get_lines_content().iter().map(|line| line.to_string_lossy()).collect()
}

/// `expected` como o `string[]` do `assert.deepStrictEqual`.
fn strings(expected: &[&str]) -> Vec<String> {
    expected.iter().map(ToString::to_string).collect()
}

/// `model.getValueInRange(range)`.
fn value_in_range(model: &TextModel, range: Range) -> String {
    model.get_value_in_range(&range).to_string_lossy()
}

/// `editor.getValue()`.
fn value_of(editor: &TestCodeEditor) -> String {
    editor.get_value(None).to_string_lossy()
}

/// `editor.setSelection(selection)` seguido de `executeAction(action, editor)`: o par que a suíte de caixa repete.
fn run_on_selection(editor: &TestCodeEditor, action: &impl EditorAction, selection: Selection) {
    set_selection(editor, selection);
    execute_action(action, editor);
}

mod sort_lines_ascending_action {
    use super::*;

    #[test]
    fn should_sort_selected_lines_in_ascending_order() {
        run_test(|| {
            with_editor(&["omicron", "beta", "alpha"], |editor| {
                let model = model_of(editor);
                let sort_lines_ascending_action = sort_lines_ascending_action();

                set_selection(editor, Selection::new(1, 1, 3, 5));
                execute_action(&sort_lines_ascending_action, editor);
                assert_eq!(lines_content(&model), strings(&["alpha", "beta", "omicron"]));
                assert_selection(editor, Selection::new(1, 1, 3, 7));
            });
        });
    }

    #[test]
    fn should_sort_lines_in_ascending_order() {
        run_test(|| {
            with_editor(&["omicron", "beta", "alpha"], |editor| {
                let model = model_of(editor);
                let sort_lines_ascending_action = sort_lines_ascending_action();

                execute_action(&sort_lines_ascending_action, editor);
                assert_eq!(lines_content(&model), strings(&["alpha", "beta", "omicron"]));
            });
        });
    }

    #[test]
    fn should_sort_multiple_selections_in_ascending_order() {
        run_test(|| {
            with_editor(&["omicron", "beta", "alpha", "", "omicron", "beta", "alpha"], |editor| {
                let model = model_of(editor);
                let sort_lines_ascending_action = sort_lines_ascending_action();

                set_selections(editor, &[Selection::new(1, 1, 3, 5), Selection::new(5, 1, 7, 5)]);
                execute_action(&sort_lines_ascending_action, editor);
                assert_eq!(
                    lines_content(&model),
                    strings(&["alpha", "beta", "omicron", "", "alpha", "beta", "omicron"])
                );
                let expected_selections = [Selection::new(1, 1, 3, 7), Selection::new(5, 1, 7, 7)];
                for (index, actual_selection) in selections_of(editor).iter().enumerate() {
                    assert_eq!(actual_selection.to_string(), expected_selections[index].to_string());
                }
            });
        });
    }

    #[test]
    fn applies_to_whole_document_when_selection_is_single_line() {
        run_test(|| {
            with_editor(&["omicron", "beta", "alpha"], |editor| {
                let model = model_of(editor);
                let sort_lines_ascending_action = sort_lines_ascending_action();

                set_selection(editor, Selection::new(2, 1, 2, 4));
                execute_action(&sort_lines_ascending_action, editor);
                assert_eq!(lines_content(&model), strings(&["alpha", "beta", "omicron"]));
            });
        });
    }
}

mod sort_lines_descending_action {
    use super::*;

    #[test]
    fn should_sort_selected_lines_in_descending_order() {
        run_test(|| {
            with_editor(&["alpha", "beta", "omicron"], |editor| {
                let model = model_of(editor);
                let sort_lines_descending_action = sort_lines_descending_action();

                set_selection(editor, Selection::new(1, 1, 3, 7));
                execute_action(&sort_lines_descending_action, editor);
                assert_eq!(lines_content(&model), strings(&["omicron", "beta", "alpha"]));
                assert_selection(editor, Selection::new(1, 1, 3, 5));
            });
        });
    }

    #[test]
    fn should_sort_multiple_selections_in_descending_order() {
        run_test(|| {
            with_editor(&["alpha", "beta", "omicron", "", "alpha", "beta", "omicron"], |editor| {
                let model = model_of(editor);
                let sort_lines_descending_action = sort_lines_descending_action();

                set_selections(editor, &[Selection::new(1, 1, 3, 7), Selection::new(5, 1, 7, 7)]);
                execute_action(&sort_lines_descending_action, editor);
                assert_eq!(
                    lines_content(&model),
                    strings(&["omicron", "beta", "alpha", "", "omicron", "beta", "alpha"])
                );
                let expected_selections = [Selection::new(1, 1, 3, 5), Selection::new(5, 1, 7, 5)];
                for (index, actual_selection) in selections_of(editor).iter().enumerate() {
                    assert_eq!(actual_selection.to_string(), expected_selections[index].to_string());
                }
            });
        });
    }
}

mod delete_duplicate_lines_action {
    use super::*;

    #[test]
    fn should_remove_duplicate_lines_within_selection() {
        run_test(|| {
            with_editor(&["alpha", "beta", "beta", "beta", "alpha", "omicron"], |editor| {
                let model = model_of(editor);
                let delete_duplicate_lines_action = DeleteDuplicateLinesAction::new();

                set_selection(editor, Selection::new(1, 3, 6, 4));
                execute_action(&delete_duplicate_lines_action, editor);
                assert_eq!(lines_content(&model), strings(&["alpha", "beta", "omicron"]));
                assert_selection(editor, Selection::new(1, 1, 3, 8));
            });
        });
    }

    #[test]
    fn should_remove_duplicate_lines() {
        run_test(|| {
            with_editor(&["alpha", "beta", "beta", "beta", "alpha", "omicron"], |editor| {
                let model = model_of(editor);
                let delete_duplicate_lines_action = DeleteDuplicateLinesAction::new();

                execute_action(&delete_duplicate_lines_action, editor);
                assert_eq!(lines_content(&model), strings(&["alpha", "beta", "omicron"]));
                assert!(selection_of(editor).is_empty());
            });
        });
    }

    #[test]
    fn should_remove_duplicate_lines_in_multiple_selections() {
        run_test(|| {
            with_editor(&["alpha", "beta", "beta", "omicron", "", "alpha", "alpha", "beta"], |editor| {
                let model = model_of(editor);
                let delete_duplicate_lines_action = DeleteDuplicateLinesAction::new();

                set_selections(editor, &[Selection::new(1, 2, 4, 3), Selection::new(6, 2, 8, 3)]);
                execute_action(&delete_duplicate_lines_action, editor);
                assert_eq!(lines_content(&model), strings(&["alpha", "beta", "omicron", "", "alpha", "beta"]));
                let expected_selections = [Selection::new(1, 1, 3, 8), Selection::new(5, 1, 6, 5)];
                for (index, actual_selection) in selections_of(editor).iter().enumerate() {
                    assert_eq!(actual_selection.to_string(), expected_selections[index].to_string());
                }
            });
        });
    }

    #[test]
    fn applies_to_whole_document_when_selection_is_single_line() {
        run_test(|| {
            with_editor(&["alpha", "beta", "alpha", "omicron"], |editor| {
                let model = model_of(editor);
                let delete_duplicate_lines_action = DeleteDuplicateLinesAction::new();

                set_selection(editor, Selection::new(2, 1, 2, 2));
                execute_action(&delete_duplicate_lines_action, editor);
                assert_eq!(lines_content(&model), strings(&["alpha", "beta", "omicron"]));
            });
        });
    }
}

mod delete_all_left_action {
    use super::*;

    #[test]
    fn should_delete_to_the_left_of_the_cursor() {
        run_test(|| {
            with_editor(&["one", "two", "three"], |editor| {
                let model = model_of(editor);
                let delete_all_left_action = delete_all_left_action();

                set_selection(editor, Selection::new(1, 2, 1, 2));
                execute_action(&delete_all_left_action, editor);
                assert_eq!(line_content(&model, 1), "ne");

                set_selections(editor, &[Selection::new(2, 2, 2, 2), Selection::new(3, 2, 3, 2)]);
                execute_action(&delete_all_left_action, editor);
                assert_eq!(line_content(&model, 2), "wo");
                assert_eq!(line_content(&model, 3), "hree");
            });
        });
    }

    #[test]
    fn should_jump_to_the_previous_line_when_on_first_column() {
        run_test(|| {
            with_editor(&["one", "two", "three"], |editor| {
                let model = model_of(editor);
                let delete_all_left_action = delete_all_left_action();

                set_selection(editor, Selection::new(2, 1, 2, 1));
                execute_action(&delete_all_left_action, editor);
                assert_eq!(line_content(&model, 1), "onetwo");

                set_selections(editor, &[Selection::new(1, 1, 1, 1), Selection::new(2, 1, 2, 1)]);
                execute_action(&delete_all_left_action, editor);
                assert_eq!(lines_content(&model)[0], "onetwothree");
                assert_eq!(lines_content(&model).len(), 1);

                set_selection(editor, Selection::new(1, 1, 1, 1));
                execute_action(&delete_all_left_action, editor);
                assert_eq!(lines_content(&model)[0], "onetwothree");
            });
        });
    }

    #[test]
    fn should_keep_deleting_lines_in_multi_cursor_mode() {
        run_test(|| {
            with_editor(
                &[
                    "hi my name is Carlos Matos",
                    "BCC",
                    "waso waso waso",
                    "my wife doesnt believe in me",
                    "nonononono",
                    "bitconneeeect",
                ],
                |editor| {
                    let model = model_of(editor);
                    let delete_all_left_action = delete_all_left_action();

                    let before_second_waso_selection = Selection::new(3, 5, 3, 5);
                    let end_of_bcc_selection = Selection::new(2, 4, 2, 4);
                    let end_of_nonono = Selection::new(5, 11, 5, 11);

                    set_selections(editor, &[before_second_waso_selection, end_of_bcc_selection, end_of_nonono]);

                    execute_action(&delete_all_left_action, editor);
                    let selections = selections_of(editor);

                    assert_eq!(line_content(&model, 2), "");
                    assert_eq!(line_content(&model, 3), " waso waso");
                    assert_eq!(line_content(&model, 5), "");

                    assert_eq!(bounds(&selections[0]), [3, 1, 3, 1]);
                    assert_eq!(bounds(&selections[1]), [2, 1, 2, 1]);
                    assert_eq!(bounds(&selections[2]), [5, 1, 5, 1]);

                    execute_action(&delete_all_left_action, editor);
                    let selections = selections_of(editor);

                    assert_eq!(line_content(&model, 1), "hi my name is Carlos Matos waso waso");
                    assert_eq!(selections.len(), 2);

                    assert_eq!(bounds(&selections[0]), [1, 27, 1, 27]);
                    assert_eq!(bounds(&selections[1]), [2, 29, 2, 29]);
                },
            );
        });
    }

    #[test]
    fn should_work_in_multi_cursor_mode() {
        run_test(|| {
            with_editor(
                &["hello", "world", "hello world", "hello", "bonjour", "hola", "world", "hello world"],
                |editor| {
                    let model = model_of(editor);
                    let delete_all_left_action = delete_all_left_action();

                    set_selections(editor, &[Selection::new(1, 2, 1, 2), Selection::new(1, 4, 1, 4)]);
                    execute_action(&delete_all_left_action, editor);
                    assert_eq!(line_content(&model, 1), "lo");

                    set_selections(editor, &[Selection::new(2, 2, 2, 2), Selection::new(2, 4, 2, 5)]);
                    execute_action(&delete_all_left_action, editor);
                    assert_eq!(line_content(&model, 2), "d");

                    set_selections(editor, &[Selection::new(3, 2, 3, 5), Selection::new(3, 7, 3, 7)]);
                    execute_action(&delete_all_left_action, editor);
                    assert_eq!(line_content(&model, 3), "world");

                    set_selections(editor, &[Selection::new(4, 3, 4, 3), Selection::new(4, 5, 5, 4)]);
                    execute_action(&delete_all_left_action, editor);
                    assert_eq!(line_content(&model, 4), "jour");

                    set_selections(
                        editor,
                        &[Selection::new(5, 3, 6, 3), Selection::new(6, 5, 7, 5), Selection::new(7, 7, 7, 7)],
                    );
                    execute_action(&delete_all_left_action, editor);
                    assert_eq!(line_content(&model, 5), "world");
                },
            );
        });
    }

    #[test]
    fn issue_36234_should_push_undo_stop() {
        run_test(|| {
            with_editor(&["one", "two", "three"], |editor| {
                let model = model_of(editor);
                let delete_all_left_action = delete_all_left_action();

                set_selection(editor, Selection::new(1, 1, 1, 1));

                type_text(editor, "Typing some text here on line ");
                assert_eq!(line_content(&model, 1), "Typing some text here on line one");
                assert_eq!(selection_of(editor).to_string(), Selection::new(1, 31, 1, 31).to_string());

                execute_action(&delete_all_left_action, editor);
                assert_eq!(line_content(&model, 1), "one");
                assert_eq!(selection_of(editor).to_string(), Selection::new(1, 1, 1, 1).to_string());

                undo(editor);
                assert_eq!(line_content(&model, 1), "Typing some text here on line one");
                assert_eq!(selection_of(editor).to_string(), Selection::new(1, 31, 1, 31).to_string());
            });
        });
    }
}

mod join_lines_action {
    use super::*;

    #[test]
    fn should_join_lines_and_insert_space_if_necessary() {
        run_test(|| {
            with_editor(
                &["hello", "world", "hello ", "world", "hello\t\t", "\tworld", "hello   ", "\tworld", "", "", "hello world"],
                |editor| {
                    let model = model_of(editor);
                    let join_lines_action = JoinLinesAction::new();

                    set_selection(editor, Selection::new(1, 2, 1, 2));
                    execute_action(&join_lines_action, editor);
                    assert_eq!(line_content(&model, 1), "hello world");
                    assert_selection(editor, Selection::new(1, 6, 1, 6));

                    set_selection(editor, Selection::new(2, 2, 2, 2));
                    execute_action(&join_lines_action, editor);
                    assert_eq!(line_content(&model, 2), "hello world");
                    assert_selection(editor, Selection::new(2, 7, 2, 7));

                    set_selection(editor, Selection::new(3, 2, 3, 2));
                    execute_action(&join_lines_action, editor);
                    assert_eq!(line_content(&model, 3), "hello world");
                    assert_selection(editor, Selection::new(3, 7, 3, 7));

                    set_selection(editor, Selection::new(4, 2, 5, 3));
                    execute_action(&join_lines_action, editor);
                    assert_eq!(line_content(&model, 4), "hello world");
                    assert_selection(editor, Selection::new(4, 2, 4, 8));

                    set_selection(editor, Selection::new(5, 1, 7, 3));
                    execute_action(&join_lines_action, editor);
                    assert_eq!(line_content(&model, 5), "hello world");
                    assert_selection(editor, Selection::new(5, 1, 5, 3));
                },
            );
        });
    }

    #[test]
    fn issue_50471_join_lines_at_the_end_of_document() {
        run_test(|| {
            with_editor(&["hello", "world"], |editor| {
                let model = model_of(editor);
                let join_lines_action = JoinLinesAction::new();

                set_selection(editor, Selection::new(2, 1, 2, 1));
                execute_action(&join_lines_action, editor);
                assert_eq!(line_content(&model, 1), "hello");
                assert_eq!(line_content(&model, 2), "world");
                assert_selection(editor, Selection::new(2, 6, 2, 6));
            });
        });
    }

    #[test]
    fn should_work_in_multi_cursor_mode() {
        run_test(|| {
            with_editor(
                &["hello", "world", "hello ", "world", "hello\t\t", "\tworld", "hello   ", "\tworld", "", "", "hello world"],
                |editor| {
                    let model = model_of(editor);
                    let join_lines_action = JoinLinesAction::new();

                    set_selections(
                        editor,
                        &[
                            // primary cursor
                            Selection::new(5, 2, 5, 2),
                            Selection::new(1, 2, 1, 2),
                            Selection::new(3, 2, 4, 2),
                            Selection::new(5, 4, 6, 3),
                            Selection::new(7, 5, 8, 4),
                            Selection::new(10, 1, 10, 1),
                        ],
                    );

                    execute_action(&join_lines_action, editor);
                    assert_eq!(
                        lines_content(&model).join("\n"),
                        "hello world\nhello world\nhello world\nhello world\n\nhello world"
                    );
                    assert_selections(
                        editor,
                        &[
                            // primary cursor
                            Selection::new(3, 4, 3, 8),
                            Selection::new(1, 6, 1, 6),
                            Selection::new(2, 2, 2, 8),
                            Selection::new(4, 5, 4, 9),
                            Selection::new(6, 1, 6, 1),
                        ],
                    );
                },
            );
        });
    }

    #[test]
    fn should_push_undo_stop() {
        run_test(|| {
            with_editor(&["hello", "world"], |editor| {
                let model = model_of(editor);
                let join_lines_action = JoinLinesAction::new();

                set_selection(editor, Selection::new(1, 6, 1, 6));

                type_text(editor, " my dear");
                assert_eq!(line_content(&model, 1), "hello my dear");
                assert_eq!(selection_of(editor).to_string(), Selection::new(1, 14, 1, 14).to_string());

                execute_action(&join_lines_action, editor);
                assert_eq!(line_content(&model, 1), "hello my dear world");
                assert_eq!(selection_of(editor).to_string(), Selection::new(1, 14, 1, 14).to_string());

                undo(editor);
                assert_eq!(line_content(&model, 1), "hello my dear");
                assert_eq!(selection_of(editor).to_string(), Selection::new(1, 14, 1, 14).to_string());
            });
        });
    }
}

mod reverse_lines_action {
    use super::*;

    #[test]
    fn reverses_lines() {
        run_test(|| {
            with_editor(&["alice", "bob", "charlie"], |editor| {
                let model = model_of(editor);
                let reverse_lines_action = ReverseLinesAction::new();

                execute_action(&reverse_lines_action, editor);
                assert_eq!(lines_content(&model), strings(&["charlie", "bob", "alice"]));
            });
        });
    }

    #[test]
    fn excludes_empty_last_line() {
        run_test(|| {
            with_editor(&["alice", "bob", "charlie", ""], |editor| {
                let model = model_of(editor);
                let reverse_lines_action = ReverseLinesAction::new();

                execute_action(&reverse_lines_action, editor);
                assert_eq!(lines_content(&model), strings(&["charlie", "bob", "alice", ""]));
            });
        });
    }

    #[test]
    fn updates_cursor() {
        run_test(|| {
            with_editor(&["alice", "bob", "charlie"], |editor| {
                let reverse_lines_action = ReverseLinesAction::new();
                // cursor at third column of third line 'charlie'
                editor.set_position(Position::new(3, 3), None);

                execute_action(&reverse_lines_action, editor);
                // cursor at third column of *first* line 'charlie'
                assert!(editor.get_position() == Some(Position::new(1, 3)));
            });
        });
    }

    #[test]
    fn preserves_cursor_on_empty_last_line() {
        run_test(|| {
            with_editor(&["alice", "bob", "charlie", ""], |editor| {
                let reverse_lines_action = ReverseLinesAction::new();
                editor.set_position(Position::new(4, 1), None);

                execute_action(&reverse_lines_action, editor);
                assert!(editor.get_position() == Some(Position::new(4, 1)));
            });
        });
    }

    #[test]
    fn preserves_selected_text_when_selections_do_not_span_lines() {
        run_test(|| {
            with_editor(&["alice", "bob", "charlie", ""], |editor| {
                let model = model_of(editor);
                let reverse_lines_action = ReverseLinesAction::new();
                set_selections(editor, &[Selection::new(1, 1, 1, 3), Selection::new(2, 1, 2, 4), Selection::new(3, 1, 3, 5)]);
                let expected_selected_text = strings(&["al", "bob", "char"]);
                let selected_text = |editor: &TestCodeEditor| -> Vec<String> {
                    selections_of(editor).iter().map(|selection| value_in_range(&model, **selection)).collect()
                };
                assert_eq!(selected_text(editor), expected_selected_text);

                execute_action(&reverse_lines_action, editor);
                assert_eq!(selected_text(editor), expected_selected_text);
            });
        });
    }

    #[test]
    fn reverses_lines_within_selection() {
        run_test(|| {
            with_editor(&["line1", "line2", "line3", "line4", "line5"], |editor| {
                let model = model_of(editor);
                let reverse_lines_action = ReverseLinesAction::new();

                // Select lines 2-4
                set_selection(editor, Selection::new(2, 1, 4, 6));
                execute_action(&reverse_lines_action, editor);
                assert_eq!(lines_content(&model), strings(&["line1", "line4", "line3", "line2", "line5"]));
            });
        });
    }

    #[test]
    fn reverses_lines_within_partial_selection() {
        run_test(|| {
            with_editor(&["line1", "line2", "line3", "line4", "line5"], |editor| {
                let model = model_of(editor);
                let reverse_lines_action = ReverseLinesAction::new();

                // Select partial lines 2-4 (from middle of line2 to middle of line4)
                set_selection(editor, Selection::new(2, 3, 4, 3));
                execute_action(&reverse_lines_action, editor);
                assert_eq!(lines_content(&model), strings(&["line1", "line4", "line3", "line2", "line5"]));
            });
        });
    }

    #[test]
    fn reverses_lines_with_multiple_selections() {
        run_test(|| {
            with_editor(&["line1", "line2", "line3", "line4", "line5", "line6"], |editor| {
                let model = model_of(editor);
                let reverse_lines_action = ReverseLinesAction::new();

                // Select lines 1-2 and lines 4-5
                set_selections(editor, &[Selection::new(1, 1, 2, 6), Selection::new(4, 1, 5, 6)]);
                execute_action(&reverse_lines_action, editor);
                assert_eq!(lines_content(&model), strings(&["line2", "line1", "line3", "line5", "line4", "line6"]));
            });
        });
    }

    #[test]
    fn updates_selection_positions_after_reversal() {
        run_test(|| {
            with_editor(&["line1", "line2", "line3", "line4"], |editor| {
                let reverse_lines_action = ReverseLinesAction::new();

                // Select lines 1-3
                set_selection(editor, Selection::new(1, 2, 3, 3));
                execute_action(&reverse_lines_action, editor);

                // After reversal, selection should be updated to maintain relative position
                // Originally line 1 col 2 -> line 3 col 3, so after reversal should be line 3 col 2 -> line 1 col 3
                let selection = selection_of(editor);
                // The selection should cover the same logical text after reversal
                // Range normalization ensures startLineNumber <= endLineNumber
                assert_eq!(selection.start_line_number, 1);
                assert_eq!(selection.start_column, 3);
                assert_eq!(selection.end_line_number, 3);
                assert_eq!(selection.end_column, 2);
            });
        });
    }

    #[test]
    fn applies_to_whole_document_when_selection_is_single_line() {
        run_test(|| {
            with_editor(&["line1", "line2", "line3"], |editor| {
                let model = model_of(editor);
                let reverse_lines_action = ReverseLinesAction::new();

                // Select only line 2
                set_selection(editor, Selection::new(2, 1, 2, 6));
                execute_action(&reverse_lines_action, editor);
                assert_eq!(lines_content(&model), strings(&["line3", "line2", "line1"]));
            });
        });
    }

    #[test]
    fn excludes_end_line_when_selection_ends_at_column_1() {
        run_test(|| {
            with_editor(&["line1", "line2", "line3", "line4", "line5"], |editor| {
                let model = model_of(editor);
                let reverse_lines_action = ReverseLinesAction::new();

                // Select from line 2 to line 4 column 1 (should exclude line 4)
                set_selection(editor, Selection::new(2, 1, 4, 1));
                execute_action(&reverse_lines_action, editor);
                assert_eq!(lines_content(&model), strings(&["line1", "line3", "line2", "line4", "line5"]));
            });
        });
    }

    #[test]
    fn applies_to_whole_document_when_selection_is_single_line_2() {
        run_test(|| {
            with_editor(&["omicron", "beta", "alpha"], |editor| {
                let model = model_of(editor);
                let reverse_lines_action = ReverseLinesAction::new();

                set_selection(editor, Selection::new(2, 1, 2, 4));
                execute_action(&reverse_lines_action, editor);
                assert_eq!(lines_content(&model), strings(&["alpha", "beta", "omicron"]));
            });
        });
    }
}

#[test]
fn transpose() {
    run_test(|| {
        with_editor(&["hello world", "", "", "   "], |editor| {
            let model = model_of(editor);
            let transpose_action = TransposeAction::new();

            set_selection(editor, Selection::new(1, 1, 1, 1));
            execute_action(&transpose_action, editor);
            assert_eq!(line_content(&model, 1), "hello world");
            assert_selection(editor, Selection::new(1, 2, 1, 2));

            set_selection(editor, Selection::new(1, 6, 1, 6));
            execute_action(&transpose_action, editor);
            assert_eq!(line_content(&model, 1), "hell oworld");
            assert_selection(editor, Selection::new(1, 7, 1, 7));

            set_selection(editor, Selection::new(1, 12, 1, 12));
            execute_action(&transpose_action, editor);
            assert_eq!(line_content(&model, 1), "hell oworl");
            assert_selection(editor, Selection::new(2, 2, 2, 2));

            set_selection(editor, Selection::new(3, 1, 3, 1));
            execute_action(&transpose_action, editor);
            assert_eq!(line_content(&model, 3), "");
            assert_selection(editor, Selection::new(4, 1, 4, 1));

            set_selection(editor, Selection::new(4, 2, 4, 2));
            execute_action(&transpose_action, editor);
            assert_eq!(line_content(&model, 4), "   ");
            assert_selection(editor, Selection::new(4, 3, 4, 3));
        });

        // fix #16633
        with_editor(&["", "", "hello", "world", "", "hello world", "", "hello world"], |editor| {
            let model = model_of(editor);
            let transpose_action = TransposeAction::new();

            set_selection(editor, Selection::new(1, 1, 1, 1));
            execute_action(&transpose_action, editor);
            assert_eq!(line_content(&model, 2), "");
            assert_selection(editor, Selection::new(2, 1, 2, 1));

            set_selection(editor, Selection::new(3, 6, 3, 6));
            execute_action(&transpose_action, editor);
            assert_eq!(line_content(&model, 4), "oworld");
            assert_selection(editor, Selection::new(4, 2, 4, 2));

            set_selection(editor, Selection::new(6, 12, 6, 12));
            execute_action(&transpose_action, editor);
            assert_eq!(line_content(&model, 7), "d");
            assert_selection(editor, Selection::new(7, 2, 7, 2));

            set_selection(editor, Selection::new(8, 12, 8, 12));
            execute_action(&transpose_action, editor);
            assert_eq!(line_content(&model, 8), "hello world");
            assert_selection(editor, Selection::new(8, 12, 8, 12));
        });
    });
}

#[test]
fn toggle_case() {
    run_test(|| {
        with_editor(
            &[
                "hello world",
                "öçşğü",
                "parseHTMLString",
                "getElementById",
                "insertHTML",
                "PascalCase",
                "CSSSelectorsList",
                "iD",
                "tEST",
                "öçşÖÇŞğüĞÜ",
                "audioConverter.convertM4AToMP3();",
                "snake_case",
                "Capital_Snake_Case",
                // O `.replace(/^\s+/gm, '')` do original tira a indentação das quatro linhas do template.
                "function helloWorld() {",
                "return someGlobalObject.printHelloWorld(\"en\", \"utf-8\");",
                "}",
                "helloWorld();",
                "'JavaScript'",
                "parseHTML4String",
                "_accessor: ServicesAccessor",
            ],
            |editor| {
                let model = model_of(editor);
                let uppercase_action = upper_case_action();
                let lowercase_action = lower_case_action();
                let titlecase_action = title_case_action();
                let snakecase_action = snake_case_action();

                run_on_selection(editor, &uppercase_action, Selection::new(1, 1, 1, 12));
                assert_eq!(line_content(&model, 1), "HELLO WORLD");
                assert_selection(editor, Selection::new(1, 1, 1, 12));

                run_on_selection(editor, &lowercase_action, Selection::new(1, 1, 1, 12));
                assert_eq!(line_content(&model, 1), "hello world");
                assert_selection(editor, Selection::new(1, 1, 1, 12));

                run_on_selection(editor, &uppercase_action, Selection::new(1, 3, 1, 3));
                assert_eq!(line_content(&model, 1), "HELLO world");
                assert_selection(editor, Selection::new(1, 3, 1, 3));

                run_on_selection(editor, &lowercase_action, Selection::new(1, 4, 1, 4));
                assert_eq!(line_content(&model, 1), "hello world");
                assert_selection(editor, Selection::new(1, 4, 1, 4));

                run_on_selection(editor, &titlecase_action, Selection::new(1, 1, 1, 12));
                assert_eq!(line_content(&model, 1), "Hello World");
                assert_selection(editor, Selection::new(1, 1, 1, 12));

                run_on_selection(editor, &uppercase_action, Selection::new(2, 1, 2, 6));
                assert_eq!(line_content(&model, 2), "ÖÇŞĞÜ");
                assert_selection(editor, Selection::new(2, 1, 2, 6));

                run_on_selection(editor, &lowercase_action, Selection::new(2, 1, 2, 6));
                assert_eq!(line_content(&model, 2), "öçşğü");
                assert_selection(editor, Selection::new(2, 1, 2, 6));

                run_on_selection(editor, &titlecase_action, Selection::new(2, 1, 2, 6));
                assert_eq!(line_content(&model, 2), "Öçşğü");
                assert_selection(editor, Selection::new(2, 1, 2, 6));

                run_on_selection(editor, &snakecase_action, Selection::new(3, 1, 3, 16));
                assert_eq!(line_content(&model, 3), "parse_html_string");
                assert_selection(editor, Selection::new(3, 1, 3, 18));

                run_on_selection(editor, &snakecase_action, Selection::new(4, 1, 4, 15));
                assert_eq!(line_content(&model, 4), "get_element_by_id");
                assert_selection(editor, Selection::new(4, 1, 4, 18));

                run_on_selection(editor, &snakecase_action, Selection::new(5, 1, 5, 11));
                assert_eq!(line_content(&model, 5), "insert_html");
                assert_selection(editor, Selection::new(5, 1, 5, 12));

                run_on_selection(editor, &snakecase_action, Selection::new(6, 1, 6, 11));
                assert_eq!(line_content(&model, 6), "pascal_case");
                assert_selection(editor, Selection::new(6, 1, 6, 12));

                run_on_selection(editor, &snakecase_action, Selection::new(7, 1, 7, 17));
                assert_eq!(line_content(&model, 7), "css_selectors_list");
                assert_selection(editor, Selection::new(7, 1, 7, 19));

                run_on_selection(editor, &snakecase_action, Selection::new(8, 1, 8, 3));
                assert_eq!(line_content(&model, 8), "i_d");
                assert_selection(editor, Selection::new(8, 1, 8, 4));

                run_on_selection(editor, &snakecase_action, Selection::new(9, 1, 9, 5));
                assert_eq!(line_content(&model, 9), "t_est");
                assert_selection(editor, Selection::new(9, 1, 9, 6));

                run_on_selection(editor, &snakecase_action, Selection::new(10, 1, 10, 11));
                assert_eq!(line_content(&model, 10), "öçş_öç_şğü_ğü");
                assert_selection(editor, Selection::new(10, 1, 10, 14));

                run_on_selection(editor, &snakecase_action, Selection::new(11, 1, 11, 34));
                assert_eq!(line_content(&model, 11), "audio_converter.convert_m4a_to_mp3();");
                assert_selection(editor, Selection::new(11, 1, 11, 38));

                run_on_selection(editor, &snakecase_action, Selection::new(12, 1, 12, 11));
                assert_eq!(line_content(&model, 12), "snake_case");
                assert_selection(editor, Selection::new(12, 1, 12, 11));

                run_on_selection(editor, &snakecase_action, Selection::new(13, 1, 13, 19));
                assert_eq!(line_content(&model, 13), "capital_snake_case");
                assert_selection(editor, Selection::new(13, 1, 13, 19));

                run_on_selection(editor, &snakecase_action, Selection::new(14, 1, 17, 14));
                assert_eq!(
                    value_in_range(&model, Range::new(14, 1, 17, 15)),
                    "function hello_world() {\nreturn some_global_object.print_hello_world(\"en\", \"utf-8\");\n}\nhello_world();"
                );
                assert_selection(editor, Selection::new(14, 1, 17, 15));

                run_on_selection(editor, &snakecase_action, Selection::new(18, 1, 18, 13));
                assert_eq!(line_content(&model, 18), "'java_script'");
                assert_selection(editor, Selection::new(18, 1, 18, 14));

                run_on_selection(editor, &snakecase_action, Selection::new(19, 1, 19, 17));
                assert_eq!(line_content(&model, 19), "parse_html4_string");
                assert_selection(editor, Selection::new(19, 1, 19, 19));

                run_on_selection(editor, &snakecase_action, Selection::new(20, 1, 20, 28));
                assert_eq!(line_content(&model, 20), "_accessor: services_accessor");
                assert_selection(editor, Selection::new(20, 1, 20, 29));
            },
        );

        with_editor(
            &[
                "foO baR BaZ",
                "foO'baR'BaZ",
                "foO[baR]BaZ",
                "foO`baR~BaZ",
                "foO^baR%BaZ",
                "foO$baR!BaZ",
                "'physician's assistant'",
            ],
            |editor| {
                let model = model_of(editor);
                let titlecase_action = title_case_action();

                run_on_selection(editor, &titlecase_action, Selection::new(1, 1, 1, 12));
                assert_eq!(line_content(&model, 1), "Foo Bar Baz");

                run_on_selection(editor, &titlecase_action, Selection::new(2, 1, 2, 12));
                assert_eq!(line_content(&model, 2), "Foo'bar'baz");

                run_on_selection(editor, &titlecase_action, Selection::new(3, 1, 3, 12));
                assert_eq!(line_content(&model, 3), "Foo[Bar]Baz");

                run_on_selection(editor, &titlecase_action, Selection::new(4, 1, 4, 12));
                assert_eq!(line_content(&model, 4), "Foo`Bar~Baz");

                run_on_selection(editor, &titlecase_action, Selection::new(5, 1, 5, 12));
                assert_eq!(line_content(&model, 5), "Foo^Bar%Baz");

                run_on_selection(editor, &titlecase_action, Selection::new(6, 1, 6, 12));
                assert_eq!(line_content(&model, 6), "Foo$Bar!Baz");

                run_on_selection(editor, &titlecase_action, Selection::new(7, 1, 7, 23));
                assert_eq!(line_content(&model, 7), "'Physician's Assistant'");
            },
        );

        with_editor(
            &[
                "camel from words",
                "from_snake_case",
                "from-kebab-case",
                "alreadyCamel",
                "ReTain_some_CAPitalization",
                "my_var.test_function()",
                "öçş_öç_şğü_ğü",
                "XMLHttpRequest",
                "\tfunction hello_world() {",
                "\t\treturn some_global_object;",
                "\t}",
            ],
            |editor| {
                let model = model_of(editor);
                let camelcase_action = camel_case_action();

                run_on_selection(editor, &camelcase_action, Selection::new(1, 1, 1, 18));
                assert_eq!(line_content(&model, 1), "camelFromWords");

                run_on_selection(editor, &camelcase_action, Selection::new(2, 1, 2, 15));
                assert_eq!(line_content(&model, 2), "fromSnakeCase");

                run_on_selection(editor, &camelcase_action, Selection::new(3, 1, 3, 15));
                assert_eq!(line_content(&model, 3), "fromKebabCase");

                run_on_selection(editor, &camelcase_action, Selection::new(4, 1, 4, 12));
                assert_eq!(line_content(&model, 4), "alreadyCamel");

                run_on_selection(editor, &camelcase_action, Selection::new(5, 1, 5, 26));
                assert_eq!(line_content(&model, 5), "reTainSomeCAPitalization");

                run_on_selection(editor, &camelcase_action, Selection::new(6, 1, 6, 23));
                assert_eq!(line_content(&model, 6), "myVar.testFunction()");

                run_on_selection(editor, &camelcase_action, Selection::new(7, 1, 7, 14));
                assert_eq!(line_content(&model, 7), "öçşÖçŞğüĞü");

                run_on_selection(editor, &camelcase_action, Selection::new(8, 1, 8, 14));
                assert_eq!(line_content(&model, 8), "XMLHttpRequest");

                run_on_selection(editor, &camelcase_action, Selection::new(9, 1, 11, 2));
                assert_eq!(
                    value_in_range(&model, Range::new(9, 1, 11, 3)),
                    "\tfunction helloWorld() {\n\t\treturn someGlobalObject;\n\t}"
                );
            },
        );

        with_editor(&["", "   "], |editor| {
            let model = model_of(editor);
            let uppercase_action = upper_case_action();
            let lowercase_action = lower_case_action();

            run_on_selection(editor, &uppercase_action, Selection::new(1, 1, 1, 1));
            assert_eq!(line_content(&model, 1), "");
            assert_selection(editor, Selection::new(1, 1, 1, 1));

            run_on_selection(editor, &lowercase_action, Selection::new(1, 1, 1, 1));
            assert_eq!(line_content(&model, 1), "");
            assert_selection(editor, Selection::new(1, 1, 1, 1));

            run_on_selection(editor, &uppercase_action, Selection::new(2, 2, 2, 2));
            assert_eq!(line_content(&model, 2), "   ");
            assert_selection(editor, Selection::new(2, 2, 2, 2));

            run_on_selection(editor, &lowercase_action, Selection::new(2, 2, 2, 2));
            assert_eq!(line_content(&model, 2), "   ");
            assert_selection(editor, Selection::new(2, 2, 2, 2));
        });

        with_editor(
            &[
                "hello world",
                "öçşğü",
                "parseHTMLString",
                "getElementById",
                "PascalCase",
                "öçşÖÇŞğüĞÜ",
                "audioConverter.convertM4AToMP3();",
                "Capital_Snake_Case",
                "parseHTML4String",
                "_accessor: ServicesAccessor",
                "Kebab-Case",
            ],
            |editor| {
                let model = model_of(editor);
                let kebab_case_action = kebab_case_action();

                run_on_selection(editor, &kebab_case_action, Selection::new(1, 1, 1, 12));
                assert_eq!(line_content(&model, 1), "hello world");
                assert_selection(editor, Selection::new(1, 1, 1, 12));

                run_on_selection(editor, &kebab_case_action, Selection::new(2, 1, 2, 6));
                assert_eq!(line_content(&model, 2), "öçşğü");
                assert_selection(editor, Selection::new(2, 1, 2, 6));

                run_on_selection(editor, &kebab_case_action, Selection::new(3, 1, 3, 16));
                assert_eq!(line_content(&model, 3), "parse-html-string");
                assert_selection(editor, Selection::new(3, 1, 3, 18));

                run_on_selection(editor, &kebab_case_action, Selection::new(4, 1, 4, 15));
                assert_eq!(line_content(&model, 4), "get-element-by-id");
                assert_selection(editor, Selection::new(4, 1, 4, 18));

                run_on_selection(editor, &kebab_case_action, Selection::new(5, 1, 5, 11));
                assert_eq!(line_content(&model, 5), "pascal-case");
                assert_selection(editor, Selection::new(5, 1, 5, 12));

                run_on_selection(editor, &kebab_case_action, Selection::new(6, 1, 6, 11));
                assert_eq!(line_content(&model, 6), "öçş-öç-şğü-ğü");
                assert_selection(editor, Selection::new(6, 1, 6, 14));

                run_on_selection(editor, &kebab_case_action, Selection::new(7, 1, 7, 34));
                assert_eq!(line_content(&model, 7), "audio-converter.convert-m4a-to-mp3();");
                assert_selection(editor, Selection::new(7, 1, 7, 38));

                run_on_selection(editor, &kebab_case_action, Selection::new(8, 1, 8, 19));
                assert_eq!(line_content(&model, 8), "capital-snake-case");
                assert_selection(editor, Selection::new(8, 1, 8, 19));

                run_on_selection(editor, &kebab_case_action, Selection::new(9, 1, 9, 17));
                assert_eq!(line_content(&model, 9), "parse-html4-string");
                assert_selection(editor, Selection::new(9, 1, 9, 19));

                run_on_selection(editor, &kebab_case_action, Selection::new(10, 1, 10, 28));
                assert_eq!(line_content(&model, 10), "_accessor: services-accessor");
                assert_selection(editor, Selection::new(10, 1, 10, 29));

                run_on_selection(editor, &kebab_case_action, Selection::new(11, 1, 11, 11));
                assert_eq!(line_content(&model, 11), "kebab-case");
                assert_selection(editor, Selection::new(11, 1, 11, 11));
            },
        );

        with_editor(
            &[
                "hello world",
                "öçşğü",
                "parseHTMLString",
                "getElementById",
                "PascalCase",
                "öçşÖÇŞğüĞÜ",
                "audioConverter.convertM4AToMP3();",
                "Capital_Snake_Case",
                "parseHTML4String",
                "Kebab-Case",
                "FOO_BAR",
                "FOO BAR A",
                "xML_HTTP-reQUEsT",
                "ÉCOLE",
                "ΩMEGA_CASE",
                "ДОМ_ТЕСТ",
            ],
            |editor| {
                let model = model_of(editor);
                let pascal_case_action = pascal_case_action();

                run_on_selection(editor, &pascal_case_action, Selection::new(1, 1, 1, 12));
                assert_eq!(line_content(&model, 1), "HelloWorld");
                assert_selection(editor, Selection::new(1, 1, 1, 11));

                run_on_selection(editor, &pascal_case_action, Selection::new(2, 1, 2, 6));
                assert_eq!(line_content(&model, 2), "Öçşğü");
                assert_selection(editor, Selection::new(2, 1, 2, 6));

                run_on_selection(editor, &pascal_case_action, Selection::new(3, 1, 3, 16));
                assert_eq!(line_content(&model, 3), "ParseHTMLString");
                assert_selection(editor, Selection::new(3, 1, 3, 16));

                run_on_selection(editor, &pascal_case_action, Selection::new(4, 1, 4, 15));
                assert_eq!(line_content(&model, 4), "GetElementById");
                assert_selection(editor, Selection::new(4, 1, 4, 15));

                run_on_selection(editor, &pascal_case_action, Selection::new(5, 1, 5, 11));
                assert_eq!(line_content(&model, 5), "PascalCase");
                assert_selection(editor, Selection::new(5, 1, 5, 11));

                run_on_selection(editor, &pascal_case_action, Selection::new(6, 1, 6, 11));
                assert_eq!(line_content(&model, 6), "ÖçşÖÇŞğüĞÜ");
                assert_selection(editor, Selection::new(6, 1, 6, 11));

                run_on_selection(editor, &pascal_case_action, Selection::new(7, 1, 7, 34));
                assert_eq!(line_content(&model, 7), "AudioConverter.ConvertM4AToMP3();");
                assert_selection(editor, Selection::new(7, 1, 7, 34));

                run_on_selection(editor, &pascal_case_action, Selection::new(8, 1, 8, 19));
                assert_eq!(line_content(&model, 8), "CapitalSnakeCase");
                assert_selection(editor, Selection::new(8, 1, 8, 17));

                run_on_selection(editor, &pascal_case_action, Selection::new(9, 1, 9, 17));
                assert_eq!(line_content(&model, 9), "ParseHTML4String");
                assert_selection(editor, Selection::new(9, 1, 9, 17));

                run_on_selection(editor, &pascal_case_action, Selection::new(10, 1, 10, 11));
                assert_eq!(line_content(&model, 10), "KebabCase");
                assert_selection(editor, Selection::new(10, 1, 10, 10));

                run_on_selection(editor, &pascal_case_action, Selection::new(9, 1, 10, 11));
                assert_eq!(value_in_range(&model, Range::new(9, 1, 10, 11)), "ParseHTML4String\nKebabCase");
                assert_selection(editor, Selection::new(9, 1, 10, 10));

                run_on_selection(editor, &pascal_case_action, Selection::new(11, 1, 11, 8));
                assert_eq!(line_content(&model, 11), "FooBar");
                assert_selection(editor, Selection::new(11, 1, 11, 7));

                run_on_selection(editor, &pascal_case_action, Selection::new(12, 1, 12, 10));
                assert_eq!(line_content(&model, 12), "FooBarA");
                assert_selection(editor, Selection::new(12, 1, 12, 8));

                run_on_selection(editor, &pascal_case_action, Selection::new(13, 1, 13, 17));
                assert_eq!(line_content(&model, 13), "XmlHttpReQUEsT");
                assert_selection(editor, Selection::new(13, 1, 13, 15));

                run_on_selection(editor, &pascal_case_action, Selection::new(14, 1, 14, 6));
                assert_eq!(line_content(&model, 14), "École");
                assert_selection(editor, Selection::new(14, 1, 14, 6));

                run_on_selection(editor, &pascal_case_action, Selection::new(15, 1, 15, 11));
                assert_eq!(line_content(&model, 15), "ΩmegaCase");
                assert_selection(editor, Selection::new(15, 1, 15, 10));

                run_on_selection(editor, &pascal_case_action, Selection::new(16, 1, 16, 9));
                assert_eq!(line_content(&model, 16), "ДомТест");
                assert_selection(editor, Selection::new(16, 1, 16, 8));
            },
        );
    });
}

mod delete_all_right_action {
    use super::*;

    #[test]
    fn should_be_noop_on_empty() {
        run_test(|| {
            with_editor(&[""], |editor| {
                let model = model_of(editor);
                let action = delete_all_right_action();

                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&[""]));
                assert_selections(editor, &[Selection::new(1, 1, 1, 1)]);

                set_selection(editor, Selection::new(1, 1, 1, 1));
                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&[""]));
                assert_selections(editor, &[Selection::new(1, 1, 1, 1)]);

                set_selections(editor, &[Selection::new(1, 1, 1, 1), Selection::new(1, 1, 1, 1), Selection::new(1, 1, 1, 1)]);
                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&[""]));
                assert_selections(editor, &[Selection::new(1, 1, 1, 1)]);
            });
        });
    }

    #[test]
    fn should_delete_selected_range() {
        run_test(|| {
            with_editor(&["hello", "world"], |editor| {
                let model = model_of(editor);
                let action = delete_all_right_action();

                set_selection(editor, Selection::new(1, 2, 1, 5));
                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&["ho", "world"]));
                assert_selections(editor, &[Selection::new(1, 2, 1, 2)]);

                set_selection(editor, Selection::new(1, 1, 2, 4));
                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&["ld"]));
                assert_selections(editor, &[Selection::new(1, 1, 1, 1)]);

                set_selection(editor, Selection::new(1, 1, 1, 3));
                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&[""]));
                assert_selections(editor, &[Selection::new(1, 1, 1, 1)]);
            });
        });
    }

    #[test]
    fn should_delete_to_the_right_of_the_cursor() {
        run_test(|| {
            with_editor(&["hello", "world"], |editor| {
                let model = model_of(editor);
                let action = delete_all_right_action();

                set_selection(editor, Selection::new(1, 3, 1, 3));
                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&["he", "world"]));
                assert_selections(editor, &[Selection::new(1, 3, 1, 3)]);

                set_selection(editor, Selection::new(2, 1, 2, 1));
                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&["he", ""]));
                assert_selections(editor, &[Selection::new(2, 1, 2, 1)]);
            });
        });
    }

    #[test]
    fn should_join_two_lines_if_at_the_end_of_the_line() {
        run_test(|| {
            with_editor(&["hello", "world"], |editor| {
                let model = model_of(editor);
                let action = delete_all_right_action();

                set_selection(editor, Selection::new(1, 6, 1, 6));
                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&["helloworld"]));
                assert_selections(editor, &[Selection::new(1, 6, 1, 6)]);

                set_selection(editor, Selection::new(1, 6, 1, 6));
                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&["hello"]));
                assert_selections(editor, &[Selection::new(1, 6, 1, 6)]);

                set_selection(editor, Selection::new(1, 6, 1, 6));
                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&["hello"]));
                assert_selections(editor, &[Selection::new(1, 6, 1, 6)]);
            });
        });
    }

    #[test]
    fn should_work_with_multiple_cursors() {
        run_test(|| {
            with_editor(&["hello", "there", "world"], |editor| {
                let model = model_of(editor);
                let action = delete_all_right_action();

                set_selections(editor, &[Selection::new(1, 3, 1, 3), Selection::new(1, 6, 1, 6), Selection::new(3, 4, 3, 4)]);
                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&["hethere", "wor"]));
                assert_selections(editor, &[Selection::new(1, 3, 1, 3), Selection::new(2, 4, 2, 4)]);

                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&["he", "wor"]));
                assert_selections(editor, &[Selection::new(1, 3, 1, 3), Selection::new(2, 4, 2, 4)]);

                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&["hewor"]));
                assert_selections(editor, &[Selection::new(1, 3, 1, 3), Selection::new(1, 6, 1, 6)]);

                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&["he"]));
                assert_selections(editor, &[Selection::new(1, 3, 1, 3)]);

                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&["he"]));
                assert_selections(editor, &[Selection::new(1, 3, 1, 3)]);
            });
        });
    }

    #[test]
    fn should_work_with_undo_redo() {
        run_test(|| {
            with_editor(&["hello", "there", "world"], |editor| {
                let model = model_of(editor);
                let action = delete_all_right_action();

                set_selections(editor, &[Selection::new(1, 3, 1, 3), Selection::new(1, 6, 1, 6), Selection::new(3, 4, 3, 4)]);
                execute_action(&action, editor);
                assert_eq!(lines_content(&model), strings(&["hethere", "wor"]));
                assert_selections(editor, &[Selection::new(1, 3, 1, 3), Selection::new(2, 4, 2, 4)]);

                undo(editor);
                assert_selections(
                    editor,
                    &[Selection::new(1, 3, 1, 3), Selection::new(1, 6, 1, 6), Selection::new(3, 4, 3, 4)],
                );
                redo(editor);
                assert_selections(editor, &[Selection::new(1, 3, 1, 3), Selection::new(2, 4, 2, 4)]);
            });
        });
    }
}

#[test]
fn insert_line_before_action_test() {
    run_test(|| {
        /// `testInsertLineBefore(lineNumber, column, callback)`.
        fn test_insert_line_before(line_number: i32, column: i32, callback: impl FnOnce(&TestCodeEditor, &TextModel)) {
            with_editor(&["First line", "Second line", "Third line"], |editor| {
                editor.set_position(Position::new(line_number, column), None);
                let insert_line_before_action = insert_line_before_action();

                execute_action(&insert_line_before_action, editor);
                callback(editor, &model_of(editor));
            });
        }

        test_insert_line_before(1, 3, |editor, model| {
            assert_eq!(selection_of(editor).to_string(), Selection::new(1, 1, 1, 1).to_string());
            assert_eq!(line_content(model, 1), "");
            assert_eq!(line_content(model, 2), "First line");
            assert_eq!(line_content(model, 3), "Second line");
            assert_eq!(line_content(model, 4), "Third line");
        });

        test_insert_line_before(2, 3, |editor, model| {
            assert_eq!(selection_of(editor).to_string(), Selection::new(2, 1, 2, 1).to_string());
            assert_eq!(line_content(model, 1), "First line");
            assert_eq!(line_content(model, 2), "");
            assert_eq!(line_content(model, 3), "Second line");
            assert_eq!(line_content(model, 4), "Third line");
        });

        test_insert_line_before(3, 3, |editor, model| {
            assert_eq!(selection_of(editor).to_string(), Selection::new(3, 1, 3, 1).to_string());
            assert_eq!(line_content(model, 1), "First line");
            assert_eq!(line_content(model, 2), "Second line");
            assert_eq!(line_content(model, 3), "");
            assert_eq!(line_content(model, 4), "Third line");
        });
    });
}

#[test]
fn insert_line_after_action_test() {
    run_test(|| {
        /// `testInsertLineAfter(lineNumber, column, callback)`.
        fn test_insert_line_after(line_number: i32, column: i32, callback: impl FnOnce(&TestCodeEditor, &TextModel)) {
            with_editor(&["First line", "Second line", "Third line"], |editor| {
                editor.set_position(Position::new(line_number, column), None);
                let insert_line_after_action = insert_line_after_action();

                execute_action(&insert_line_after_action, editor);
                callback(editor, &model_of(editor));
            });
        }

        test_insert_line_after(1, 3, |editor, model| {
            assert_eq!(selection_of(editor).to_string(), Selection::new(2, 1, 2, 1).to_string());
            assert_eq!(line_content(model, 1), "First line");
            assert_eq!(line_content(model, 2), "");
            assert_eq!(line_content(model, 3), "Second line");
            assert_eq!(line_content(model, 4), "Third line");
        });

        test_insert_line_after(2, 3, |editor, model| {
            assert_eq!(selection_of(editor).to_string(), Selection::new(3, 1, 3, 1).to_string());
            assert_eq!(line_content(model, 1), "First line");
            assert_eq!(line_content(model, 2), "Second line");
            assert_eq!(line_content(model, 3), "");
            assert_eq!(line_content(model, 4), "Third line");
        });

        test_insert_line_after(3, 3, |editor, model| {
            assert_eq!(selection_of(editor).to_string(), Selection::new(4, 1, 4, 1).to_string());
            assert_eq!(line_content(model, 1), "First line");
            assert_eq!(line_content(model, 2), "Second line");
            assert_eq!(line_content(model, 3), "Third line");
            assert_eq!(line_content(model, 4), "");
        });
    });
}

#[test]
fn bug_18276_editor_indentation_broken_when_selection_is_empty() {
    run_test(|| {
        let model = create_text_model_with(
            String::from("function baz() {"),
            None,
            RelaxedTextModelCreationOptions { insert_spaces: Some(false), ..Default::default() },
            None,
        );

        with_test_code_editor(model.clone(), TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let indent_lines_action = IndentLinesAction::new();
            editor.set_position(Position::new(1, 2), None);

            execute_action(&indent_lines_action, editor);
            assert_eq!(line_content(&model, 1), "\tfunction baz() {");
            assert_eq!(selection_of(editor).to_string(), Selection::new(1, 3, 1, 3).to_string());

            tab(editor);
            assert_eq!(line_content(&model, 1), "\tf\tunction baz() {");
        });

        model.dispose();
    });
}

#[test]
fn issue_80736_indenting_while_the_cursor_is_at_the_start_of_a_line_of_text_causes_the_added_spaces_or_tab_to_be_selected() {
    run_test(|| {
        let model = create_text_model_with(
            String::from("Some text"),
            None,
            RelaxedTextModelCreationOptions { insert_spaces: Some(false), ..Default::default() },
            None,
        );

        with_test_code_editor(model.clone(), TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
            let indent_lines_action = IndentLinesAction::new();
            editor.set_position(Position::new(1, 1), None);

            execute_action(&indent_lines_action, editor);
            assert_eq!(line_content(&model, 1), "\tSome text");
            assert_eq!(selection_of(editor).to_string(), Selection::new(1, 2, 1, 2).to_string());
        });

        model.dispose();
    });
}

#[test]
fn indenting_on_empty_line_should_move_cursor() {
    run_test(|| {
        let model = create_text_model("");

        with_test_code_editor(
            model.clone(),
            TestCodeEditorInstantiationOptions::new(Dynamic::object(vec![("useTabStops", Dynamic::from(false))])),
            |editor, _, _| {
                let indent_lines_action = IndentLinesAction::new();
                editor.set_position(Position::new(1, 1), None);

                execute_action(&indent_lines_action, editor);
                assert_eq!(line_content(&model, 1), "    ");
                assert_eq!(selection_of(editor).to_string(), Selection::new(1, 5, 1, 5).to_string());
            },
        );

        model.dispose();
    });
}

#[test]
fn issue_62112_delete_line_does_not_work_properly_when_multiple_cursors_are_on_line() {
    run_test(|| {
        with_editor(&["a", "foo boo", "too", "c"], |editor| {
            set_selections(editor, &[Selection::new(2, 4, 2, 4), Selection::new(2, 8, 2, 8), Selection::new(3, 4, 3, 4)]);
            let delete_lines_action = DeleteLinesAction::new();
            execute_action(&delete_lines_action, editor);

            assert_eq!(value_of(editor), "a\nc");
        });
    });
}

/// `testDeleteLinesCommand(initialText, initialSelections, resultingText, resultingSelections)`.
fn test_delete_lines_command(
    initial_text: &[&str],
    initial_selections: &[Selection],
    resulting_text: &[&str],
    resulting_selections: &[Selection],
) {
    with_editor(initial_text, |editor| {
        set_selections(editor, initial_selections);
        let delete_lines_action = DeleteLinesAction::new();
        execute_action(&delete_lines_action, editor);

        assert_eq!(value_of(editor), resulting_text.join("\n"));
        assert_selections(editor, resulting_selections);
    });
}

/// As cinco linhas que os casos de `DeleteLinesAction` usam.
const FIVE_LINES: &[&str] = &["first", "second line", "third line", "fourth line", "fifth"];

#[test]
fn empty_selection_in_middle_of_lines() {
    run_test(|| {
        test_delete_lines_command(
            FIVE_LINES,
            &[Selection::new(2, 3, 2, 3)],
            &["first", "third line", "fourth line", "fifth"],
            &[Selection::new(2, 3, 2, 3)],
        );
    });
}

#[test]
fn empty_selection_at_top_of_lines() {
    run_test(|| {
        test_delete_lines_command(
            FIVE_LINES,
            &[Selection::new(1, 5, 1, 5)],
            &["second line", "third line", "fourth line", "fifth"],
            &[Selection::new(1, 5, 1, 5)],
        );
    });
}

#[test]
fn empty_selection_at_end_of_lines() {
    run_test(|| {
        test_delete_lines_command(
            FIVE_LINES,
            &[Selection::new(5, 2, 5, 2)],
            &["first", "second line", "third line", "fourth line"],
            &[Selection::new(4, 2, 4, 2)],
        );
    });
}

#[test]
fn with_selection_in_middle_of_lines() {
    run_test(|| {
        test_delete_lines_command(
            FIVE_LINES,
            &[Selection::new(3, 3, 2, 2)],
            &["first", "fourth line", "fifth"],
            &[Selection::new(2, 2, 2, 2)],
        );
    });
}

#[test]
fn with_selection_at_top_of_lines() {
    run_test(|| {
        test_delete_lines_command(
            FIVE_LINES,
            &[Selection::new(1, 4, 1, 5)],
            &["second line", "third line", "fourth line", "fifth"],
            &[Selection::new(1, 5, 1, 5)],
        );
    });
}

#[test]
fn with_selection_at_end_of_lines() {
    run_test(|| {
        test_delete_lines_command(
            FIVE_LINES,
            &[Selection::new(5, 1, 5, 2)],
            &["first", "second line", "third line", "fourth line"],
            &[Selection::new(4, 2, 4, 2)],
        );
    });
}

#[test]
fn with_full_line_selection_in_middle_of_lines() {
    run_test(|| {
        test_delete_lines_command(
            FIVE_LINES,
            &[Selection::new(4, 1, 2, 1)],
            &["first", "fourth line", "fifth"],
            &[Selection::new(2, 1, 2, 1)],
        );
    });
}

#[test]
fn with_full_line_selection_at_top_of_lines() {
    run_test(|| {
        test_delete_lines_command(
            FIVE_LINES,
            &[Selection::new(2, 1, 1, 5)],
            &["second line", "third line", "fourth line", "fifth"],
            &[Selection::new(1, 5, 1, 5)],
        );
    });
}

#[test]
fn with_full_line_selection_at_end_of_lines() {
    run_test(|| {
        test_delete_lines_command(
            FIVE_LINES,
            &[Selection::new(4, 1, 5, 2)],
            &["first", "second line", "third line"],
            &[Selection::new(3, 2, 3, 2)],
        );
    });
}

#[test]
fn multicursor_1() {
    run_test(|| {
        test_delete_lines_command(
            &[
                "class P {",
                "",
                "    getA() {",
                "        if (true) {",
                "            return \"a\";",
                "        }",
                "    }",
                "",
                "    getB() {",
                "        if (true) {",
                "            return \"b\";",
                "        }",
                "    }",
                "",
                "    getC() {",
                "        if (true) {",
                "            return \"c\";",
                "        }",
                "    }",
                "}",
            ],
            &[Selection::new(4, 1, 5, 1), Selection::new(10, 1, 11, 1), Selection::new(16, 1, 17, 1)],
            &[
                "class P {",
                "",
                "    getA() {",
                "            return \"a\";",
                "        }",
                "    }",
                "",
                "    getB() {",
                "            return \"b\";",
                "        }",
                "    }",
                "",
                "    getC() {",
                "            return \"c\";",
                "        }",
                "    }",
                "}",
            ],
            &[Selection::new(4, 1, 4, 1), Selection::new(9, 1, 9, 1), Selection::new(14, 1, 14, 1)],
        );
    });
}
