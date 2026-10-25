//! Testes da fachada `observable_code_editor` (porte de `src/vs/editor/browser/observableCodeEditor.ts`). O original
//! não tem suíte própria; os casos conferem os observáveis básicos sobre o editor de teste (`withTestCodeEditor`).
//!
//! Os casos moram aqui, e não num `mod tests` do módulo, porque o editor de teste (`TestCodeEditor`,
//! `TestConfiguration`) é código dos testes de integração (`tests/support`) e não é visível de dentro da crate.

#![allow(dead_code)]

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_base::observable::base::Observable;
use je_base::observable::{SettableExt, autorun};
use je_editor::code_editor_widget::{Handler, ICodeEditor, TypePayload};
use je_editor::observable_code_editor::observable_code_editor;
use je_editor::position::Position;
use je_editor::selection::Selection;
use support::test_code_editor::{TestCodeEditorInstantiationOptions, with_test_code_editor};

#[test]
fn value_follows_the_model_and_writes_back() {
    with_test_code_editor("hello", TestCodeEditorInstantiationOptions::default(), |editor, _view_model, _services| {
        let oce = observable_code_editor(editor.widget());
        assert_eq!(oce.value.get().to_string_lossy(), "hello");
        assert!(!oce.value_is_empty.get());

        editor.set_value(JsString::from("abc"));
        assert_eq!(oce.value.get().to_string_lossy(), "abc");

        // `value` é um observável com setter: escrever nele altera o modelo.
        oce.value.set(JsString::from("xyz"), None);
        assert_eq!(editor.get_value(None).to_string_lossy(), "xyz");

        editor.set_value(JsString::from(""));
        assert!(oce.value_is_empty.get());
    });
}

#[test]
fn selections_and_cursor_follow_the_editor() {
    with_test_code_editor("hello world", TestCodeEditorInstantiationOptions::default(), |editor, _view_model, _services| {
        let oce = observable_code_editor(editor.widget());
        assert_eq!(oce.selections.get().expect("o editor tem modelo").len(), 1);

        editor.set_selection(Selection::new(1, 1, 1, 3).into(), None);

        assert_eq!(*oce.selections.get().expect("o editor tem modelo"), vec![Selection::new(1, 1, 1, 3)]);
        assert_eq!(oce.cursor_selection.get(), Some(Selection::new(1, 1, 1, 3)));
        assert_eq!(oce.cursor_position.get(), Some(Position::new(1, 3)));
        assert_eq!(oce.cursor_line_number.get(), Some(1));
        assert_eq!(oce.positions.get().map(|positions| positions.to_vec()), Some(vec![Position::new(1, 1)]));
    });
}

#[test]
fn version_id_follows_the_model() {
    with_test_code_editor("hello", TestCodeEditorInstantiationOptions::default(), |editor, _view_model, _services| {
        let oce = observable_code_editor(editor.widget());
        let before = oce.version_id.get().expect("o editor tem modelo");

        editor.set_value(JsString::from("changed"));

        let after = oce.version_id.get().expect("o editor tem modelo");
        assert!(after > before);
        assert_eq!(Some(after), editor.get_model().map(|model| model.get_version_id()));
    });
}

#[test]
fn detaching_the_model_clears_the_state() {
    with_test_code_editor("hello", TestCodeEditorInstantiationOptions::default(), |editor, _view_model, _services| {
        let oce = observable_code_editor(editor.widget());
        assert!(oce.model.get().is_some());

        editor.set_model(None);

        assert!(oce.model.get().is_none());
        assert_eq!(oce.version_id.get(), None);
        assert!(oce.selections.get().is_none());
        assert_eq!(oce.value.get().to_string_lossy(), "");
        assert_eq!(oce.cursor_line_number.get(), None);
    });
}

#[test]
fn transaction_batches_the_editor_updates_and_on_did_type_fires() {
    with_test_code_editor("hello", TestCodeEditorInstantiationOptions::default(), |editor, _view_model, _services| {
        let oce = observable_code_editor(editor.widget());

        let runs = Rc::new(Cell::new(0));
        let reaction = {
            let (oce, runs) = (Rc::clone(&oce), Rc::clone(&runs));
            autorun(move |reader| {
                oce.value.read(reader);
                oce.selections.read(reader);
                runs.set(runs.get() + 1);
            })
        };
        assert_eq!(runs.get(), 1);

        // Duas mudanças do editor dentro da transação reagem uma vez só, no fim dela.
        oce.transaction(|_tx| {
            editor.set_value(JsString::from("abc"));
            editor.set_selection(Selection::new(1, 2, 1, 2).into(), None);
        });
        assert_eq!(runs.get(), 2);
        reaction.dispose();

        let typed = Rc::new(Cell::new(0));
        let type_reaction = {
            let (oce, typed) = (Rc::clone(&oce), Rc::clone(&typed));
            autorun(move |reader| {
                oce.on_did_type.read(reader);
                typed.set(typed.get() + 1);
            })
        };
        assert_eq!(typed.get(), 1);
        editor.trigger(Some("keyboard"), Handler::TYPE, Some(TypePayload { text: JsString::from("a") }.to_dynamic()));
        assert_eq!(typed.get(), 2);
        type_reaction.dispose();
    });
}
