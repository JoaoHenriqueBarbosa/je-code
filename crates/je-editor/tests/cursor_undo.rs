//! Porte de `src/vs/editor/contrib/cursorUndo/test/browser/cursorUndo.test.ts` (suíte `FindController`, o nome que o
//! original deu), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo do caso. O módulo `coreCommands` (que
//!   o original carrega por `import`) é `register_core_commands()`.
//! - `withTestCodeEditor('', {}, (editor) => ...)` é `with_test_code_editor` com as opções padrão. O
//!   `registerAndInstantiateContribution(CursorUndoRedoController.ID, CursorUndoRedoController)` é
//!   `register_and_instantiate_contribution` com o construtor da contribuição, que o caso descarta no fim (o editor de teste
//!   do porte só a guarda).
//! - `editor.trigger('test', Handler.Type, { text })` é `type_text`.
//! - `editor.runCommand(CoreNavigationCommands.CursorLeft, {})` é `run_core_editor_command(view_model, &opções vazias)` (o
//!   `runEditorCommand` de um `CoreEditorCommand` roda o `runCoreEditorCommand` com o `ViewModel` do editor) e
//!   `editor.runCommand(CoreEditingCommands.DeleteRight, {})` é `run_command` com o adaptador `EditorCommandAdapter`.
//! - `cursorUndoAction.run(null!, editor, {})` é `run(None, editor.widget(), &{})`.
//! - `assert.deepStrictEqual(editor.getSelections(), [new Selection(...)])` é `assert_eq!` do `Option<Vec<_>>` com o `Some`
//!   esperado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_editor::code_editor_widget::{Handler, ICodeEditor, TypePayload};
use je_editor::core_commands::{CursorMoveCommandOptions, core_editing_commands, core_navigation_commands, register_core_commands};
use je_editor::cursor_undo::{CursorUndoRedoController, CursorUndoRedoAction, cursor_undo};
use je_editor::editor_extensions::EditorAction;
use je_editor::selection::Selection;
use support::test_code_editor::{EditorCommandAdapter, TestCodeEditor, TestCodeEditorInstantiationOptions, with_test_code_editor};

/// `editor.trigger('test', Handler.Type, { text })`.
fn type_text(editor: &TestCodeEditor, text: &str) {
    editor.trigger(Some("test"), Handler::TYPE, Some(TypePayload { text: JsString::from(text) }.to_dynamic()));
}

/// `editor.runCommand(CoreNavigationCommands.CursorLeft, {})`.
fn cursor_left(view_model: &je_editor::code_editor_widget::EditorViewModel) {
    core_navigation_commands::instance().cursor_left.run_core_editor_command(view_model, &CursorMoveCommandOptions::default());
}

/// `cursorUndoAction.run(null!, editor, {})`.
fn run_cursor_undo(action: &CursorUndoRedoAction, editor: &TestCodeEditor) {
    let args = Dynamic::object(std::iter::empty::<(&str, Dynamic)>());
    let _ = action.run(None, editor.widget(), &args);
}

/// `assert.deepStrictEqual(editor.getSelections(), [new Selection(...)])`.
fn assert_selections(editor: &TestCodeEditor, expected: Vec<Selection>) {
    assert_eq!(editor.get_selections(), Some(expected));
}

#[test]
fn issue_82535_edge_case_with_cursor_undo() {
    register_core_commands();
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let cursor_undo_action = cursor_undo();

    with_test_code_editor("", TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| {
        let controller = editor
            .register_and_instantiate_contribution(CursorUndoRedoController::ID, |widget, _| CursorUndoRedoController::new(widget));

        // type hello
        type_text(editor, "hello");

        // press left
        cursor_left(view_model);

        // press Delete
        let editing = core_editing_commands::instance();
        editor.run_command(&EditorCommandAdapter(&*editing.delete_right), None);
        assert_eq!(editor.get_value(None).to_string_lossy(), "hell");
        assert_selections(editor, vec![Selection::new(1, 5, 1, 5)]);

        // press left
        cursor_left(view_model);
        assert_selections(editor, vec![Selection::new(1, 4, 1, 4)]);

        // press Ctrl+U
        run_cursor_undo(&cursor_undo_action, editor);
        assert_selections(editor, vec![Selection::new(1, 5, 1, 5)]);

        controller.dispose();
    });
}

#[test]
fn issue_82535_edge_case_with_cursor_undo_reverse() {
    register_core_commands();
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let cursor_undo_action = cursor_undo();

    with_test_code_editor("", TestCodeEditorInstantiationOptions::default(), |editor, _view_model, _| {
        let controller = editor
            .register_and_instantiate_contribution(CursorUndoRedoController::ID, |widget, _| CursorUndoRedoController::new(widget));

        // type hello
        type_text(editor, "hell");
        type_text(editor, "o");
        assert_eq!(editor.get_value(None).to_string_lossy(), "hello");
        assert_selections(editor, vec![Selection::new(1, 6, 1, 6)]);

        // press Ctrl+U
        run_cursor_undo(&cursor_undo_action, editor);
        assert_selections(editor, vec![Selection::new(1, 6, 1, 6)]);

        controller.dispose();
    });
}
