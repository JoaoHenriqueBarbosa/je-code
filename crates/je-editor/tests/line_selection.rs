//! Porte de `src/vs/editor/contrib/lineSelection/test/browser/lineSelection.test.ts` (suíte `LineSelection`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo do caso.
//! - `executeAction(action, editor)` é `action.run(null!, editor, undefined)`: o `run(None, editor.widget(), &Dynamic::Undefined)`
//!   do `EditorAction`, cuja promessa (já resolvida) é descartada.
//! - `withTestCodeEditor(TEXT, {}, (editor, viewModel) => ...)` é `with_test_code_editor` com as opções padrão (`{}`).
//! - `assert.deepStrictEqual(editor.getSelection(), new Selection(...))` é `assert_eq!` do `Option` que o `get_selection()` do
//!   porte devolve com o `Some` esperado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::editor_extensions::EditorAction;
use je_editor::line_selection::ExpandLineSelectionAction;
use je_editor::position::Position;
use je_editor::selection::Selection;
use support::test_code_editor::{TestCodeEditor, TestCodeEditorInstantiationOptions, with_test_code_editor};

/// `executeAction(action, editor)`.
fn execute_action(action: &dyn EditorAction, editor: &TestCodeEditor) {
    let _ = action.run(None, editor.widget(), &Dynamic::Undefined);
}

/// `assert.deepStrictEqual(editor.getSelection(), new Selection(...))`.
fn assert_selection(editor: &TestCodeEditor, expected: Selection) {
    assert_eq!(editor.get_selection(), Some(expected));
}

#[test]
fn unnamed() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();

    let line1 = "    \tMy First Line\t ";
    let line2 = "\tMy Second Line";
    let line3 = "    Third Line🐶";
    let line4 = "";
    let line5 = "1";

    let text = format!("{line1}\r\n{line2}\n{line3}\n{line4}\r\n{line5}");

    with_test_code_editor(text.as_str(), TestCodeEditorInstantiationOptions::default(), |editor, _view_model, _| {
        let action = ExpandLineSelectionAction::new();

        //              0          1         2
        //              01234 56789012345678 0
        // let LINE1 = '    \tMy First Line\t ';
        editor.set_position(Position::new(1, 1), None);
        execute_action(&action, editor);
        assert_selection(editor, Selection::new(1, 1, 2, 1));

        editor.set_position(Position::new(1, 2), None);
        execute_action(&action, editor);
        assert_selection(editor, Selection::new(1, 1, 2, 1));

        editor.set_position(Position::new(1, 5), None);
        execute_action(&action, editor);
        assert_selection(editor, Selection::new(1, 1, 2, 1));

        editor.set_position(Position::new(1, 19), None);
        execute_action(&action, editor);
        assert_selection(editor, Selection::new(1, 1, 2, 1));

        editor.set_position(Position::new(1, 20), None);
        execute_action(&action, editor);
        assert_selection(editor, Selection::new(1, 1, 2, 1));

        editor.set_position(Position::new(1, 21), None);
        execute_action(&action, editor);
        assert_selection(editor, Selection::new(1, 1, 2, 1));
        execute_action(&action, editor);
        assert_selection(editor, Selection::new(1, 1, 3, 1));
        execute_action(&action, editor);
        assert_selection(editor, Selection::new(1, 1, 4, 1));
        execute_action(&action, editor);
        assert_selection(editor, Selection::new(1, 1, 5, 1));
        execute_action(&action, editor);
        assert_selection(editor, Selection::new(1, 1, 5, line5.len() as i32 + 1));
        execute_action(&action, editor);
        assert_selection(editor, Selection::new(1, 1, 5, line5.len() as i32 + 1));
    });
}
