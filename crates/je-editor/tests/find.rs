//! Porte de `src/vs/editor/contrib/find/test/browser/find.test.ts` (suíte `Find`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o guarda criado no começo de cada caso.
//! - `withTestCodeEditor(lines, {}, cb)` é `with_test_code_editor` de `tests/support/test_code_editor.rs` (as opções `{}`
//!   são o `Default`).
//! - `getSelectionSearchString(editor)` é `get_selection_search_string(editor, None, None)` (os dois parâmetros opcionais
//!   do original com o valor padrão); o `string | null` é `Option<JsString>`.
//! - `editor.setPosition(...)` e `editor.setSelection(...)` são `set_position` e `set_selection` (o `Range` vira
//!   `SelectionOrRange` por `into`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::find::find_controller::get_selection_search_string;
use je_editor::position::Position;
use je_editor::range::Range;
use support::test_code_editor::{TestCodeEditorInstantiationOptions, with_test_code_editor};

fn search_string(text: &str) -> Option<JsString> {
    Some(JsString::from(text))
}

#[test]
fn search_string_at_position() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor(&["ABC DEF", "0123 456"][..], TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
        // The cursor is at the very top, of the file, at the first ABC
        let search_string_at_top = get_selection_search_string(editor.widget(), None, None);
        assert_eq!(search_string_at_top, search_string("ABC"));

        // Move cursor to the end of ABC
        editor.set_position(Position::new(1, 3), None);
        let search_string_after_abc = get_selection_search_string(editor.widget(), None, None);
        assert_eq!(search_string_after_abc, search_string("ABC"));

        // Move cursor to DEF
        editor.set_position(Position::new(1, 5), None);
        let search_string_inside_def = get_selection_search_string(editor.widget(), None, None);
        assert_eq!(search_string_inside_def, search_string("DEF"));
    });
}

#[test]
fn search_string_with_selection() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor(&["ABC DEF", "0123 456"][..], TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
        // Select A of ABC
        editor.set_selection(Range::new(1, 1, 1, 2).into(), None);
        let search_string_selection_a = get_selection_search_string(editor.widget(), None, None);
        assert_eq!(search_string_selection_a, search_string("A"));

        // Select BC of ABC
        editor.set_selection(Range::new(1, 2, 1, 4).into(), None);
        let search_string_selection_bc = get_selection_search_string(editor.widget(), None, None);
        assert_eq!(search_string_selection_bc, search_string("BC"));

        // Select BC DE
        editor.set_selection(Range::new(1, 2, 1, 7).into(), None);
        let search_string_selection_bcde = get_selection_search_string(editor.widget(), None, None);
        assert_eq!(search_string_selection_bcde, search_string("BC DE"));
    });
}

#[test]
fn search_string_with_multiline_selection() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    with_test_code_editor(&["ABC DEF", "0123 456"][..], TestCodeEditorInstantiationOptions::default(), |editor, _, _| {
        // Select first line and newline
        editor.set_selection(Range::new(1, 1, 2, 1).into(), None);
        let search_string_selection_whole_line = get_selection_search_string(editor.widget(), None, None);
        assert_eq!(search_string_selection_whole_line, None);

        // Select first line and chunk of second
        editor.set_selection(Range::new(1, 1, 2, 4).into(), None);
        let search_string_selection_two_lines = get_selection_search_string(editor.widget(), None, None);
        assert_eq!(search_string_selection_two_lines, None);

        // Select end of first line newline and chunk of second
        editor.set_selection(Range::new(1, 7, 2, 4).into(), None);
        let search_string_selection_span_lines = get_selection_search_string(editor.widget(), None, None);
        assert_eq!(search_string_selection_span_lines, None);
    });
}
