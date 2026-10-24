//! Porte de `src/vs/editor/contrib/multicursor/test/browser/multicursor.test.ts` (suítes `Multicursor`,
//! `Multicursor selection` e as suas subsuítes `Selected text occurrence matching`, `Selected text occurrence matching
//! highlights` e `Find state disassociation`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado dentro de `test_multicursor` (e no começo dos três
//!   primeiros casos), de modo que cada `withTestCodeEditor` confere o vazamento ao terminar. O `disposables` da suíte
//!   `Multicursor selection` é o `LeakGuard` que o callback recebe.
//! - `withTestCodeEditor(text, { serviceCollection, ...options }, callback)` e o `testMulticursor` são
//!   `test_multicursor`. O `InMemoryStorageService` da `serviceCollection` serve ao `CommonFindController` real (histórico
//!   de busca e estado do widget); o dublê não o usa, então a coleção não existe aqui.
//! - O `CommonFindController` (`contrib/find`) ainda não foi portado: todo caso que o usa (todos os da suíte `Multicursor
//!   selection`, do 4º em diante) roda contra o dublê de `tests/multicursor_doubles`, que traz o `FindReplaceState`
//!   fiel e o `CommonFindController` reduzido ao `getState` e ao `highlightFindOptions` (ver o topo dele). Só os três
//!   primeiros casos (`Multicursor`) não dependem dele.
//! - `editor.registerAndInstantiateContribution(X.ID, X)` é `register_and_instantiate_contribution`, com o construtor da
//!   contribuição. `new Action().run(null!, editor, args)` é o `run(None, editor.widget(), &args)` do `EditorAction`
//!   (com `Dynamic::Undefined` quando o original chama sem `args`), cujo resultado (uma promessa já resolvida) é descartado.
//! - `${action.id}` nos nomes dos casos vira o nome da ação no nome da função: `add_selection_to_next`,
//!   `add_selection_to_previous`, `move_selection_to_next`, `move_selection_to_previous` e `select_highlights`.
//! - `setTextFocus(editor, hasFocus)` é `set_text_focus`: `editor.setHasTextFocus(hasFocus)` (o do editor de teste) e
//!   `viewModel.setHasFocus(hasFocus)`. O `_withTestCodeEditor` do original põe o foco do `ViewModel` em `true` mesmo com
//!   `hasTextFocus: false`, e o `hasTextFocus()` do editor de teste é virtual; aqui o widget (que o código do multicursor
//!   consulta) lê o foco do `ViewModel`, então `test_multicursor` o põe em `false` quando a opção `hasTextFocus` é
//!   `false`, antes do callback, e os dois ficam coerentes como no original.
//! - `editor.trigger('test', 'removeSecondaryCursors', null)` é `remove_secondary_cursors`: o registro de extensões do
//!   editor de teste é vazio, então o comando do núcleo roda como o `_triggerEditorCommand` o rodaria (carga com
//!   `source`, sem `accessor`).
//! - `sinon.spy(findController, 'highlightFindOptions')` e `callCount` são `spy_on_highlight_find_options` e
//!   `highlight_find_options_call_count` do dublê. `sinon.useFakeTimers()` com `clock.runAll()` é
//!   `runtime::with_faked_timers` com `runtime::run_until_idle()`.
//! - `new Selection(...)` e `fromRange` são `sel` e `from_range` (`[startLine, startColumn, endLine, endColumn]`);
//!   `deepStrictEqual` de objetos é `assert_eq!` de tuplas e vetores.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod multicursor_doubles;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::{IDisposable, to_disposable};
use je_base::runtime;
use je_editor::code_editor_widget::{Handler, ICodeEditor, IEditorCommand, PastePayload, TypePayload};
use je_editor::config::editor_options::registry::SELECTED_TEXT_MATCH_MODE;
use je_editor::core_commands::core_navigation_commands;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::editor_extensions::EditorAction;
use je_editor::model::EndOfLineSequence;
use je_editor::multicursor::{
    ICommonFindControllerSource, INewFindReplaceState, IFindReplaceState, MultiCursorSelectionController,
    MultiCursorSelectionControllerAction, SelectionHighlighter, add_selection_to_next_find_match_action,
    add_selection_to_previous_find_match_action, insert_cursor_above, insert_cursor_below,
    move_selection_to_next_find_match_action, move_selection_to_previous_find_match_action, select_highlights_action,
};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::selection::Selection;
use je_editor::view_model_types::IViewModel as _;
use multicursor_doubles::{NoDocumentHighlightProviders, TestFindController, TestFindControllerSource};
use support::test_code_editor::{TestCodeEditor, TestCodeEditorInstantiationOptions, with_test_code_editor};
use support::test_text_model::create_text_model;

fn sel(start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) -> Selection {
    Selection::new(start_line_number, start_column, end_line_number, end_column)
}

/// `fromRange(rng)`.
fn from_range(range: &Range) -> [i32; 4] {
    [range.start_line_number, range.start_column, range.end_line_number, range.end_column]
}

/// `editor.getSelections().map(fromRange)`.
fn selections_of(editor: &TestCodeEditor) -> Vec<[i32; 4]> {
    editor.get_selections().expect("o editor tem o modelo").iter().map(|selection| from_range(selection)).collect()
}

/// `expectedLines.map(line => [line, 1, line, 4])`.
fn lines_1_to_4(lines: &[i32]) -> Vec<[i32; 4]> {
    lines.iter().map(|&line| [line, 1, line, 4]).collect()
}

/// O `{}` de argumentos de uma ação.
fn no_args() -> Dynamic {
    Dynamic::object(std::iter::empty::<(&str, Dynamic)>())
}

/// `action.run(null!, editor, args)`.
fn run_with(action: &dyn EditorAction, editor: &TestCodeEditor, args: &Dynamic) {
    let _ = action.run(None, editor.widget(), args);
}

/// `action.run(null!, editor)`.
fn run(action: &dyn EditorAction, editor: &TestCodeEditor) {
    run_with(action, editor, &Dynamic::Undefined);
}

/// O `CommonFindController.get(editor)` e o `createInstance` que as ações recebem.
fn source() -> Rc<dyn ICommonFindControllerSource> {
    Rc::new(TestFindControllerSource)
}

/// `{ hasTextFocus, selectedTextMatchMode }`: cada campo `None` é `undefined` e não entra no objeto.
fn editor_options(has_text_focus: Option<bool>, selected_text_match_mode: Option<&str>) -> Dynamic {
    let mut entries: Vec<(&str, Dynamic)> = Vec::new();
    if let Some(has_text_focus) = has_text_focus {
        entries.push(("hasTextFocus", Dynamic::from(has_text_focus)));
    }
    if let Some(selected_text_match_mode) = selected_text_match_mode {
        entries.push(("selectedTextMatchMode", Dynamic::from(selected_text_match_mode)));
    }
    Dynamic::object(entries)
}

/// `{ selectedTextMatchMode }`.
fn mode_options(selected_text_match_mode: &str) -> Dynamic {
    editor_options(None, Some(selected_text_match_mode))
}

/// `editor.updateOptions({ selectedTextMatchMode })`.
fn update_selected_text_match_mode(editor: &TestCodeEditor, selected_text_match_mode: &str) {
    editor.update_options(Some(&mode_options(selected_text_match_mode)));
}

/// `testMulticursor(text, callback, options)`.
fn test_multicursor(
    text: &[&str],
    options: Dynamic,
    callback: impl FnOnce(&Rc<TestCodeEditor>, &Rc<TestFindController>, &LeakGuard),
) {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let has_text_focus = !matches!(options.get_str("hasTextFocus"), Dynamic::Bool(false));
    with_test_code_editor(text, TestCodeEditorInstantiationOptions::new(options), |editor, view_model, _| {
        if !has_text_focus {
            view_model.set_has_focus(false);
        }
        let find_controller = editor.register_and_instantiate_contribution(TestFindController::ID, |_, _| TestFindController::new());
        let multi_cursor_select_controller = editor
            .register_and_instantiate_contribution(MultiCursorSelectionController::ID, |widget, _| {
                MultiCursorSelectionController::new(widget)
            });

        callback(editor, &find_controller, &ds);

        multi_cursor_select_controller.dispose();
        find_controller.dispose();
    });
}

/// `testAddSelectionToNextFindMatchAction(text, callback)`.
fn test_add_selection_to_next_find_match_action(
    text: &[&str],
    callback: impl FnOnce(&Rc<TestCodeEditor>, &MultiCursorSelectionControllerAction, &Rc<TestFindController>),
) {
    test_multicursor(text, editor_options(None, None), |editor, find_controller, _| {
        let action = add_selection_to_next_find_match_action(source());
        callback(editor, &action, find_controller);
    });
}

/// `setTextFocus(editor, hasFocus)`.
fn set_text_focus(editor: &TestCodeEditor, has_focus: bool) {
    editor.set_has_text_focus(has_focus);
    editor.get_view_model().expect("o editor tem o modelo").set_has_focus(has_focus);
}

/// `editor.trigger('test', 'removeSecondaryCursors', null)`.
fn remove_secondary_cursors(editor: &TestCodeEditor) {
    let payload = Dynamic::object([("source", Dynamic::from("test"))]);
    let _ = core_navigation_commands::instance().remove_secondary_cursors.run_editor_command(None, editor.widget(), &payload);
}

/// `editor.trigger('test', Handler.Type, { text })`.
fn type_text(editor: &TestCodeEditor, text: &str) {
    editor.trigger(Some("test"), Handler::TYPE, Some(TypePayload { text: JsString::from(text) }.to_dynamic()));
}

mod multicursor {
    use super::*;

    #[test]
    fn issue_26393_multiple_cursors_word_wrap() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let line = "a".repeat(20);
        let lines: &[&str] = &[line.as_str(), line.as_str()];
        let options = Dynamic::object([("wordWrap", Dynamic::from("wordWrapColumn")), ("wordWrapColumn", Dynamic::from(10))]);
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::new(options), |editor, view_model, _| {
            let add_cursor_down_action = insert_cursor_below();
            run_with(&add_cursor_down_action, editor, &no_args());

            assert_eq!(view_model.get_cursor_states().len(), 2);

            assert_eq!(view_model.get_cursor_states()[0].view_state.position.line_number, 1);
            assert_eq!(view_model.get_cursor_states()[1].view_state.position.line_number, 3);

            editor.set_position(Position::new(4, 1), None);
            let add_cursor_up_action = insert_cursor_above();
            run_with(&add_cursor_up_action, editor, &no_args());

            assert_eq!(view_model.get_cursor_states().len(), 2);

            assert_eq!(view_model.get_cursor_states()[0].view_state.position.line_number, 4);
            assert_eq!(view_model.get_cursor_states()[1].view_state.position.line_number, 2);
        });
    }

    #[test]
    fn issue_2205_multi_cursor_pastes_in_reverse_order() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let lines: &[&str] = &["abc", "def"];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| {
            let add_cursor_up_action = insert_cursor_above();

            editor.set_selection(sel(2, 1, 2, 1).into(), None);
            run_with(&add_cursor_up_action, editor, &no_args());
            assert_eq!(view_model.get_selections().len(), 2);

            editor.trigger(
                Some("test"),
                Handler::PASTE,
                Some(
                    PastePayload {
                        text: JsString::from("1\n2"),
                        multicursor_text: Some(vec![JsString::from("1"), JsString::from("2")]),
                        ..Default::default()
                    }
                    .to_dynamic(),
                ),
            );

            let model = editor.get_model().expect("o editor tem o modelo");
            assert_eq!(model.get_line_content(1), "1abc");
            assert_eq!(model.get_line_content(2), "2def");
        });
    }

    #[test]
    fn issue_1336_insert_cursor_below_on_last_line_adds_a_cursor_to_the_end_of_the_current_line() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let lines: &[&str] = &["abc"];
        with_test_code_editor(lines, TestCodeEditorInstantiationOptions::default(), |editor, view_model, _| {
            let add_cursor_down_action = insert_cursor_below();
            run_with(&add_cursor_down_action, editor, &no_args());
            assert_eq!(view_model.get_selections().len(), 1);
        });
    }
}

mod multicursor_selection {
    use super::*;

    #[test]
    fn issue_8817_cursor_position_changes_when_you_cancel_multicursor() {
        test_multicursor(
            &["var x = (3 * 5)", "var y = (3 * 5)", "var z = (3 * 5)"],
            editor_options(None, None),
            |editor, _, _| {
                let select_highlights_action = select_highlights_action(source());

                editor.set_selection(sel(2, 9, 2, 16).into(), None);

                run(&select_highlights_action, editor);
                assert_eq!(selections_of(editor), vec![[2, 9, 2, 16], [1, 9, 1, 16], [3, 9, 3, 16]]);

                remove_secondary_cursors(editor);

                assert_eq!(from_range(&editor.get_selection().expect("o editor tem o modelo")), [2, 9, 2, 16]);
            },
        );
    }

    #[test]
    fn issue_5400_select_all_occurrences_of_find_match_does_not_select_all_if_find_uses_regex() {
        test_multicursor(
            &["something", "someething", "someeething", "nothing"],
            editor_options(Some(false), None),
            |editor, find_controller, _| {
                let select_highlights_action = select_highlights_action(source());

                editor.set_selection(sel(1, 1, 1, 1).into(), None);
                find_controller.find_state().change(
                    &INewFindReplaceState {
                        search_string: Some(JsString::from("some+thing")),
                        is_regex: Some(true),
                        is_revealed: Some(true),
                        ..Default::default()
                    },
                    false,
                );

                run(&select_highlights_action, editor);
                assert_eq!(selections_of(editor), vec![[1, 1, 1, 10], [2, 1, 2, 11], [3, 1, 3, 12]]);

                assert_eq!(find_controller.find_state().search_string(), "some+thing");
            },
        );
    }

    #[test]
    fn add_selection_to_next_find_match_action_can_work_with_multiline() {
        test_multicursor(
            &["", "qwe", "rty", "", "qwe", "", "rty", "qwe", "rty"],
            editor_options(None, None),
            |editor, _, _| {
                let add_selection_to_next_find_match = add_selection_to_next_find_match_action(source());

                editor.set_selection(sel(2, 1, 3, 4).into(), None);

                run(&add_selection_to_next_find_match, editor);
                assert_eq!(selections_of(editor), vec![[2, 1, 3, 4], [8, 1, 9, 4]]);

                remove_secondary_cursors(editor);

                assert_eq!(from_range(&editor.get_selection().expect("o editor tem o modelo")), [2, 1, 3, 4]);
            },
        );
    }

    #[test]
    fn issue_6661_add_selection_to_next_find_match_action_can_work_with_touching_ranges() {
        test_multicursor(&["abcabc", "abc", "abcabc"], editor_options(None, None), |editor, _, _| {
            let add_selection_to_next_find_match = add_selection_to_next_find_match_action(source());

            editor.set_selection(sel(1, 1, 1, 4).into(), None);

            run(&add_selection_to_next_find_match, editor);
            assert_eq!(selections_of(editor), vec![[1, 1, 1, 4], [1, 4, 1, 7]]);

            run(&add_selection_to_next_find_match, editor);
            run(&add_selection_to_next_find_match, editor);
            run(&add_selection_to_next_find_match, editor);
            assert_eq!(
                selections_of(editor),
                vec![[1, 1, 1, 4], [1, 4, 1, 7], [2, 1, 2, 4], [3, 1, 3, 4], [3, 4, 3, 7]]
            );

            type_text(editor, "z");
            assert_eq!(selections_of(editor), vec![[1, 2, 1, 2], [1, 3, 1, 3], [2, 2, 2, 2], [3, 2, 3, 2], [3, 3, 3, 3]]);
            assert_eq!(editor.get_value(None), JsString::from("zz\nz\nzz"));
        });
    }

    #[test]
    fn issue_23541_multiline_ctrl_d_does_not_work_in_crlf_files() {
        test_multicursor(
            &["", "qwe", "rty", "", "qwe", "", "rty", "qwe", "rty"],
            editor_options(None, None),
            |editor, _, _| {
                editor.get_model().expect("o editor tem o modelo").set_eol(EndOfLineSequence::CrLf);

                let add_selection_to_next_find_match = add_selection_to_next_find_match_action(source());

                editor.set_selection(sel(2, 1, 3, 4).into(), None);

                run(&add_selection_to_next_find_match, editor);
                assert_eq!(selections_of(editor), vec![[2, 1, 3, 4], [8, 1, 9, 4]]);

                remove_secondary_cursors(editor);

                assert_eq!(from_range(&editor.get_selection().expect("o editor tem o modelo")), [2, 1, 3, 4]);
            },
        );
    }

    #[test]
    fn add_selection_to_next_find_match_action_starting_with_single_collapsed_selection() {
        let text = ["abc pizza", "abc house", "abc bar"];
        test_add_selection_to_next_find_match_action(&text, |editor, action, _| {
            editor.set_selections(&[sel(1, 2, 1, 2)], None, None);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4)]);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(2, 1, 2, 4)]);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(2, 1, 2, 4), sel(3, 1, 3, 4)]);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(2, 1, 2, 4), sel(3, 1, 3, 4)]);
        });
    }

    #[test]
    fn add_selection_to_next_find_match_action_starting_with_two_selections_one_being_collapsed_1() {
        let text = ["abc pizza", "abc house", "abc bar"];
        test_add_selection_to_next_find_match_action(&text, |editor, action, _| {
            editor.set_selections(&[sel(1, 1, 1, 4), sel(2, 2, 2, 2)], None, None);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(2, 1, 2, 4)]);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(2, 1, 2, 4), sel(3, 1, 3, 4)]);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(2, 1, 2, 4), sel(3, 1, 3, 4)]);
        });
    }

    #[test]
    fn add_selection_to_next_find_match_action_starting_with_two_selections_one_being_collapsed_2() {
        let text = ["abc pizza", "abc house", "abc bar"];
        test_add_selection_to_next_find_match_action(&text, |editor, action, _| {
            editor.set_selections(&[sel(1, 2, 1, 2), sel(2, 1, 2, 4)], None, None);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(2, 1, 2, 4)]);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(2, 1, 2, 4), sel(3, 1, 3, 4)]);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(2, 1, 2, 4), sel(3, 1, 3, 4)]);
        });
    }

    #[test]
    fn add_selection_to_next_find_match_action_starting_with_all_collapsed_selections() {
        let text = ["abc pizza", "abc house", "abc bar"];
        test_add_selection_to_next_find_match_action(&text, |editor, action, _| {
            editor.set_selections(&[sel(1, 2, 1, 2), sel(2, 2, 2, 2), sel(3, 1, 3, 1)], None, None);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(2, 1, 2, 4), sel(3, 1, 3, 4)]);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(2, 1, 2, 4), sel(3, 1, 3, 4)]);
        });
    }

    #[test]
    fn add_selection_to_next_find_match_action_starting_with_all_collapsed_selections_on_different_words() {
        let text = ["abc pizza", "abc house", "abc bar"];
        test_add_selection_to_next_find_match_action(&text, |editor, action, _| {
            editor.set_selections(&[sel(1, 6, 1, 6), sel(2, 6, 2, 6), sel(3, 6, 3, 6)], None, None);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 5, 1, 10), sel(2, 5, 2, 10), sel(3, 5, 3, 8)]);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 5, 1, 10), sel(2, 5, 2, 10), sel(3, 5, 3, 8)]);
        });
    }

    #[test]
    fn issue_20651_add_selection_to_next_find_match_action_case_insensitive() {
        let text = ["test", "testte", "Test", "testte", "test"];
        test_add_selection_to_next_find_match_action(&text, |editor, action, _| {
            editor.set_selections(&[sel(1, 1, 1, 5)], None, None);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 5), sel(2, 1, 2, 5)]);

            run(action, editor);
            assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 5), sel(2, 1, 2, 5), sel(3, 1, 3, 5)]);

            run(action, editor);
            assert_eq!(
                editor.get_selections().unwrap(),
                vec![sel(1, 1, 1, 5), sel(2, 1, 2, 5), sel(3, 1, 3, 5), sel(4, 1, 4, 5)]
            );

            run(action, editor);
            assert_eq!(
                editor.get_selections().unwrap(),
                vec![sel(1, 1, 1, 5), sel(2, 1, 2, 5), sel(3, 1, 3, 5), sel(4, 1, 4, 5), sel(5, 1, 5, 5)]
            );

            run(action, editor);
            assert_eq!(
                editor.get_selections().unwrap(),
                vec![sel(1, 1, 1, 5), sel(2, 1, 2, 5), sel(3, 1, 3, 5), sel(4, 1, 4, 5), sel(5, 1, 5, 5)]
            );
        });
    }

    mod selected_text_occurrence_matching {
        use super::*;

        const TEXT: [&str; 5] = ["foo", "FOObar", "foobar", "FOO", "foo"];

        /// As cinco ações do escopo da suíte.
        struct Actions {
            add_selection_to_next: MultiCursorSelectionControllerAction,
            add_selection_to_previous: MultiCursorSelectionControllerAction,
            move_selection_to_next: MultiCursorSelectionControllerAction,
            move_selection_to_previous: MultiCursorSelectionControllerAction,
            select_highlights: MultiCursorSelectionControllerAction,
        }

        fn actions() -> Actions {
            Actions {
                add_selection_to_next: add_selection_to_next_find_match_action(source()),
                add_selection_to_previous: add_selection_to_previous_find_match_action(source()),
                move_selection_to_next: move_selection_to_next_find_match_action(source()),
                move_selection_to_previous: move_selection_to_previous_find_match_action(source()),
                select_highlights: select_highlights_action(source()),
            }
        }

        fn assert_selected_text(
            run_action: impl FnOnce(&TestCodeEditor),
            start_line: i32,
            expected_lines: &[i32],
            selected_text_match_mode: &str,
            is_revealed: bool,
        ) {
            test_multicursor(&TEXT, mode_options(selected_text_match_mode), |editor, find_controller, _| {
                let state = find_controller.find_state();
                let match_case = selected_text_match_mode == "caseSensitive";
                state.change(
                    &INewFindReplaceState {
                        search_string: Some(JsString::from("FOO.*")),
                        is_revealed: Some(is_revealed),
                        match_case: Some(!match_case),
                        whole_word: Some(true),
                        is_regex: Some(true),
                        ..Default::default()
                    },
                    false,
                );
                editor.set_selection(sel(start_line, 1, start_line, 4).into(), None);

                run_action(editor);

                assert_eq!(
                    (selections_of(editor), [state.match_case(), state.whole_word(), state.is_regex()]),
                    (lines_1_to_4(expected_lines), [!match_case, true, true])
                );
            });
        }

        fn assert_empty_selection(
            run_action: impl Fn(&TestCodeEditor),
            start_line: i32,
            expected_lines: &[i32],
            selected_text_match_mode: &str,
        ) {
            test_multicursor(&TEXT, mode_options(selected_text_match_mode), |editor, find_controller, _| {
                let state = find_controller.find_state();
                state.change(
                    &INewFindReplaceState {
                        match_case: Some(false),
                        whole_word: Some(false),
                        is_regex: Some(true),
                        ..Default::default()
                    },
                    false,
                );
                editor.set_selection(sel(start_line, 2, start_line, 2).into(), None);

                run_action(editor);
                run_action(editor);

                assert_eq!(
                    (selections_of(editor), [state.match_case(), state.whole_word(), state.is_regex()]),
                    (lines_1_to_4(expected_lines), [true, true, false])
                );
            });
        }

        fn assert_find_options_feedback(
            run_action: impl FnOnce(&TestCodeEditor),
            start_line: i32,
            expected_lines: &[i32],
            has_text_focus: bool,
        ) {
            let mut actual: Option<(Vec<[i32; 4]>, [bool; 3], usize)> = None;
            test_multicursor(&TEXT, editor_options(Some(has_text_focus), Some("caseInsensitive")), |editor, find_controller, _| {
                let state = find_controller.find_state();
                state.change(
                    &INewFindReplaceState {
                        search_string: Some(JsString::from("foo")),
                        is_revealed: Some(!has_text_focus),
                        match_case: Some(true),
                        whole_word: Some(true),
                        is_regex: Some(true),
                        ..Default::default()
                    },
                    false,
                );
                editor.set_selection(sel(start_line, 1, start_line, 4).into(), None);
                find_controller.spy_on_highlight_find_options();

                run_action(editor);

                actual = Some((
                    selections_of(editor),
                    [state.match_case(), state.whole_word(), state.is_regex()],
                    find_controller.highlight_find_options_call_count(),
                ));
            });

            assert_eq!(
                actual,
                Some((lines_1_to_4(expected_lines), [true, true, true], if has_text_focus { 0 } else { 1 }))
            );
        }

        #[test]
        fn defaults_to_find_and_reports_live_option_changes() {
            test_multicursor(&TEXT, editor_options(None, None), |editor, _, ds| {
                let mut values = vec![editor.get_option(SELECTED_TEXT_MATCH_MODE).clone()];
                let changes: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
                let recorded = changes.clone();
                ds.add(editor.on_did_change_configuration().listen(move |e| {
                    recorded.borrow_mut().push(e.has_changed(SELECTED_TEXT_MATCH_MODE.id()));
                }));
                for selected_text_match_mode in ["caseSensitive", "caseInsensitive"] {
                    update_selected_text_match_mode(editor, selected_text_match_mode);
                    values.push(editor.get_option(SELECTED_TEXT_MATCH_MODE).clone());
                }
                assert_eq!(
                    (values, changes.borrow().clone()),
                    (vec!["findOptions".to_owned(), "caseSensitive".to_owned(), "caseInsensitive".to_owned()], vec![true, true])
                );
            });
        }

        #[test]
        fn default_selected_text_matching_uses_find_match_case_and_whole_word_options() {
            let add_selection_to_next = add_selection_to_next_find_match_action(source());
            let mut actual: Vec<Vec<[i32; 4]>> = Vec::new();
            for (match_case, whole_word) in [(false, false), (false, true), (true, false), (true, true)] {
                test_multicursor(&TEXT, editor_options(None, None), |editor, find_controller, _| {
                    find_controller.find_state().change(
                        &INewFindReplaceState { match_case: Some(match_case), whole_word: Some(whole_word), ..Default::default() },
                        false,
                    );
                    editor.set_selection(sel(1, 1, 1, 4).into(), None);
                    run(&add_selection_to_next, editor);
                    actual.push(selections_of(editor));
                });
            }

            assert_eq!(
                actual,
                vec![
                    vec![[1, 1, 1, 4], [2, 1, 2, 4]],
                    vec![[1, 1, 1, 4], [4, 1, 4, 4]],
                    vec![[1, 1, 1, 4], [3, 1, 3, 4]],
                    vec![[1, 1, 1, 4], [5, 1, 5, 4]],
                ]
            );
        }

        #[test]
        fn add_selection_to_next_selected_text_match_case_false_find_visible_false() {
            let a = actions();
            assert_selected_text(|editor| run(&a.add_selection_to_next, editor), 1, &[1, 2], "caseInsensitive", false);
        }

        #[test]
        fn add_selection_to_next_selected_text_match_case_false_find_visible_true() {
            let a = actions();
            assert_selected_text(|editor| run(&a.add_selection_to_next, editor), 1, &[1, 2], "caseInsensitive", true);
        }

        #[test]
        fn add_selection_to_next_empty_selection_always_starts_whole_word_and_case_sensitive_case_insensitive_setting() {
            let a = actions();
            assert_empty_selection(|editor| run(&a.add_selection_to_next, editor), 1, &[1, 5], "caseInsensitive");
        }

        #[test]
        fn add_selection_to_next_selected_text_match_case_true_find_visible_false() {
            let a = actions();
            assert_selected_text(|editor| run(&a.add_selection_to_next, editor), 1, &[1, 3], "caseSensitive", false);
        }

        #[test]
        fn add_selection_to_next_selected_text_match_case_true_find_visible_true() {
            let a = actions();
            assert_selected_text(|editor| run(&a.add_selection_to_next, editor), 1, &[1, 3], "caseSensitive", true);
        }

        #[test]
        fn add_selection_to_next_empty_selection_always_starts_whole_word_and_case_sensitive_case_sensitive_setting() {
            let a = actions();
            assert_empty_selection(|editor| run(&a.add_selection_to_next, editor), 1, &[1, 5], "caseSensitive");
        }

        #[test]
        fn add_selection_to_previous_selected_text_match_case_false_find_visible_false() {
            let a = actions();
            assert_selected_text(|editor| run(&a.add_selection_to_previous, editor), 5, &[5, 4], "caseInsensitive", false);
        }

        #[test]
        fn add_selection_to_previous_selected_text_match_case_false_find_visible_true() {
            let a = actions();
            assert_selected_text(|editor| run(&a.add_selection_to_previous, editor), 5, &[5, 4], "caseInsensitive", true);
        }

        #[test]
        fn add_selection_to_previous_empty_selection_always_starts_whole_word_and_case_sensitive_case_insensitive_setting() {
            let a = actions();
            assert_empty_selection(|editor| run(&a.add_selection_to_previous, editor), 5, &[5, 1], "caseInsensitive");
        }

        #[test]
        fn add_selection_to_previous_selected_text_match_case_true_find_visible_false() {
            let a = actions();
            assert_selected_text(|editor| run(&a.add_selection_to_previous, editor), 5, &[5, 3], "caseSensitive", false);
        }

        #[test]
        fn add_selection_to_previous_selected_text_match_case_true_find_visible_true() {
            let a = actions();
            assert_selected_text(|editor| run(&a.add_selection_to_previous, editor), 5, &[5, 3], "caseSensitive", true);
        }

        #[test]
        fn add_selection_to_previous_empty_selection_always_starts_whole_word_and_case_sensitive_case_sensitive_setting() {
            let a = actions();
            assert_empty_selection(|editor| run(&a.add_selection_to_previous, editor), 5, &[5, 1], "caseSensitive");
        }

        #[test]
        fn move_selection_to_next_selected_text_match_case_false_find_visible_false() {
            let a = actions();
            assert_selected_text(|editor| run(&a.move_selection_to_next, editor), 1, &[2], "caseInsensitive", false);
        }

        #[test]
        fn move_selection_to_next_selected_text_match_case_false_find_visible_true() {
            let a = actions();
            assert_selected_text(|editor| run(&a.move_selection_to_next, editor), 1, &[2], "caseInsensitive", true);
        }

        #[test]
        fn move_selection_to_next_empty_selection_always_starts_whole_word_and_case_sensitive_case_insensitive_setting() {
            let a = actions();
            assert_empty_selection(|editor| run(&a.move_selection_to_next, editor), 1, &[5], "caseInsensitive");
        }

        #[test]
        fn move_selection_to_next_selected_text_match_case_true_find_visible_false() {
            let a = actions();
            assert_selected_text(|editor| run(&a.move_selection_to_next, editor), 1, &[3], "caseSensitive", false);
        }

        #[test]
        fn move_selection_to_next_selected_text_match_case_true_find_visible_true() {
            let a = actions();
            assert_selected_text(|editor| run(&a.move_selection_to_next, editor), 1, &[3], "caseSensitive", true);
        }

        #[test]
        fn move_selection_to_next_empty_selection_always_starts_whole_word_and_case_sensitive_case_sensitive_setting() {
            let a = actions();
            assert_empty_selection(|editor| run(&a.move_selection_to_next, editor), 1, &[5], "caseSensitive");
        }

        #[test]
        fn move_selection_to_previous_selected_text_match_case_false_find_visible_false() {
            let a = actions();
            assert_selected_text(|editor| run(&a.move_selection_to_previous, editor), 5, &[4], "caseInsensitive", false);
        }

        #[test]
        fn move_selection_to_previous_selected_text_match_case_false_find_visible_true() {
            let a = actions();
            assert_selected_text(|editor| run(&a.move_selection_to_previous, editor), 5, &[4], "caseInsensitive", true);
        }

        #[test]
        fn move_selection_to_previous_empty_selection_always_starts_whole_word_and_case_sensitive_case_insensitive_setting() {
            let a = actions();
            assert_empty_selection(|editor| run(&a.move_selection_to_previous, editor), 5, &[1], "caseInsensitive");
        }

        #[test]
        fn move_selection_to_previous_selected_text_match_case_true_find_visible_false() {
            let a = actions();
            assert_selected_text(|editor| run(&a.move_selection_to_previous, editor), 5, &[3], "caseSensitive", false);
        }

        #[test]
        fn move_selection_to_previous_selected_text_match_case_true_find_visible_true() {
            let a = actions();
            assert_selected_text(|editor| run(&a.move_selection_to_previous, editor), 5, &[3], "caseSensitive", true);
        }

        #[test]
        fn move_selection_to_previous_empty_selection_always_starts_whole_word_and_case_sensitive_case_sensitive_setting() {
            let a = actions();
            assert_empty_selection(|editor| run(&a.move_selection_to_previous, editor), 5, &[1], "caseSensitive");
        }

        #[test]
        fn select_highlights_selected_text_match_case_false_find_visible_false() {
            let a = actions();
            assert_selected_text(|editor| run(&a.select_highlights, editor), 1, &[1, 2, 3, 4, 5], "caseInsensitive", false);
        }

        #[test]
        fn select_highlights_selected_text_match_case_false_find_visible_true() {
            let a = actions();
            assert_selected_text(|editor| run(&a.select_highlights, editor), 1, &[1, 2, 3, 4, 5], "caseInsensitive", true);
        }

        #[test]
        fn select_highlights_empty_selection_always_starts_whole_word_and_case_sensitive_case_insensitive_setting() {
            let a = actions();
            assert_empty_selection(|editor| run(&a.select_highlights, editor), 1, &[1, 5], "caseInsensitive");
        }

        #[test]
        fn select_highlights_selected_text_match_case_true_find_visible_false() {
            let a = actions();
            assert_selected_text(|editor| run(&a.select_highlights, editor), 1, &[1, 3, 5], "caseSensitive", false);
        }

        #[test]
        fn select_highlights_selected_text_match_case_true_find_visible_true() {
            let a = actions();
            assert_selected_text(|editor| run(&a.select_highlights, editor), 1, &[1, 3, 5], "caseSensitive", true);
        }

        #[test]
        fn select_highlights_empty_selection_always_starts_whole_word_and_case_sensitive_case_sensitive_setting() {
            let a = actions();
            assert_empty_selection(|editor| run(&a.select_highlights, editor), 1, &[1, 5], "caseSensitive");
        }

        #[test]
        fn issue_18239_replacing_selected_prefixes_preserves_differently_cased_identifiers() {
            test_multicursor(
                &["accountQryConn *grpc.ClientConn", "accountQryClient proto.AccountQueryClient"],
                mode_options("caseSensitive"),
                |editor, find_controller, _| {
                    find_controller.find_state().change(
                        &INewFindReplaceState { match_case: Some(false), whole_word: Some(true), ..Default::default() },
                        false,
                    );
                    editor.set_selection(sel(1, 1, 1, 8).into(), None);
                    run(&add_selection_to_next_find_match_action(source()), editor);
                    type_text(editor, "voucher");

                    assert_eq!(
                        editor.get_value(None),
                        JsString::from("voucherQryConn *grpc.ClientConn\nvoucherQryClient proto.AccountQueryClient")
                    );
                },
            );
        }

        #[test]
        fn select_all_replaces_only_selected_text_occurrences_while_regex_find_is_visible() {
            let select_highlights = select_highlights_action(source());
            let mut actual: Option<JsString> = None;
            test_multicursor(&TEXT, mode_options("caseSensitive"), |editor, find_controller, _| {
                find_controller.find_state().change(
                    &INewFindReplaceState {
                        search_string: Some(JsString::from("FOO.*")),
                        is_revealed: Some(true),
                        match_case: Some(false),
                        whole_word: Some(true),
                        is_regex: Some(true),
                        ..Default::default()
                    },
                    false,
                );
                editor.set_selection(sel(1, 1, 1, 4).into(), None);

                run(&select_highlights, editor);
                type_text(editor, "baz");

                actual = Some(editor.get_value(None));
            });

            assert_eq!(actual, Some(JsString::from("baz\nFOObar\nbazbar\nFOO\nbaz")));
        }

        #[test]
        fn changing_find_options_ends_caret_started_sessions_before_applying_the_selected_text_matching_setting() {
            for (selected_text_match_mode, expected_line) in [("caseSensitive", 3), ("caseInsensitive", 2)] {
                test_multicursor(&TEXT, mode_options(selected_text_match_mode), |editor, find_controller, _| {
                    editor.set_selection(sel(1, 2, 1, 2).into(), None);
                    let action = add_selection_to_next_find_match_action(source());
                    run(&action, editor);
                    find_controller.find_state().change(
                        &INewFindReplaceState { match_case: Some(true), whole_word: Some(true), ..Default::default() },
                        false,
                    );
                    find_controller.find_state().change(
                        &INewFindReplaceState { match_case: Some(false), whole_word: Some(false), ..Default::default() },
                        false,
                    );
                    run(&action, editor);

                    assert_eq!(selections_of(editor), vec![[1, 1, 1, 4], [expected_line, 1, expected_line, 4]]);
                });
            }
        }

        #[test]
        fn mixed_case_initial_selections_are_compared_using_the_selection_setting_not_find() {
            test_multicursor(&["foo", "FOO", "fooBar"], mode_options("caseInsensitive"), |editor, find_controller, _| {
                find_controller.find_state().change(&INewFindReplaceState { match_case: Some(true), ..Default::default() }, false);
                editor.set_selections(&[sel(1, 1, 1, 4), sel(2, 1, 2, 4)], None, None);
                run(&add_selection_to_next_find_match_action(source()), editor);

                assert_eq!(selections_of(editor), vec![[1, 1, 1, 4], [2, 1, 2, 4], [3, 1, 3, 4]]);
            });
        }

        #[test]
        fn find_focused_mixed_case_initial_selections_respect_find_match_case_false() {
            test_multicursor(
                &["foo", "FOO", "fooBar"],
                editor_options(Some(false), Some("caseSensitive")),
                |editor, find_controller, _| {
                    find_controller.find_state().change(
                        &INewFindReplaceState {
                            search_string: Some(JsString::from("foo")),
                            is_revealed: Some(true),
                            match_case: Some(false),
                            ..Default::default()
                        },
                        false,
                    );
                    editor.set_selections(&[sel(1, 1, 1, 4), sel(2, 1, 2, 4)], None, None);
                    run(&add_selection_to_next_find_match_action(source()), editor);

                    assert_eq!(selections_of(editor), vec![[1, 1, 1, 4], [2, 1, 2, 4], [3, 1, 3, 4]]);
                },
            );
        }

        #[test]
        fn find_focused_mixed_case_initial_selections_respect_find_match_case_true() {
            test_multicursor(
                &["foo", "FOO", "fooBar"],
                editor_options(Some(false), Some("caseInsensitive")),
                |editor, find_controller, _| {
                    find_controller.find_state().change(
                        &INewFindReplaceState {
                            search_string: Some(JsString::from("foo")),
                            is_revealed: Some(true),
                            match_case: Some(true),
                            ..Default::default()
                        },
                        false,
                    );
                    editor.set_selections(&[sel(1, 1, 1, 4), sel(2, 1, 2, 4)], None, None);
                    run(&add_selection_to_next_find_match_action(source()), editor);

                    assert_eq!(selections_of(editor), vec![[1, 1, 1, 4], [2, 1, 2, 4]]);
                },
            );
        }

        #[test]
        fn switching_focus_transfers_matching_rules_between_selection_and_find_sessions() {
            test_multicursor(
                &["foo", "FOO", "fooqux", "foobar", "bar", "BAR", "barista"],
                mode_options("caseSensitive"),
                |editor, find_controller, _| {
                    let action = move_selection_to_next_find_match_action(source());
                    editor.set_selection(sel(1, 1, 1, 4).into(), None);
                    run(&action, editor);
                    let mut actual = vec![selections_of(editor)];

                    find_controller.find_state().change(
                        &INewFindReplaceState {
                            search_string: Some(JsString::from("bar")),
                            is_revealed: Some(true),
                            match_case: Some(false),
                            whole_word: Some(true),
                            ..Default::default()
                        },
                        false,
                    );
                    set_text_focus(editor, false);
                    run(&action, editor);
                    actual.push(selections_of(editor));

                    set_text_focus(editor, true);
                    run(&action, editor);
                    actual.push(selections_of(editor));

                    assert_eq!(actual, vec![vec![[3, 1, 3, 4]], vec![[5, 1, 5, 4]], vec![[7, 1, 7, 4]]]);
                },
            );
        }

        #[test]
        fn the_default_find_session_survives_returning_focus_to_the_editor() {
            test_multicursor(&["foo", "bar", "bar"], editor_options(None, None), |editor, find_controller, _| {
                let add_selection_to_next = add_selection_to_next_find_match_action(source());
                set_text_focus(editor, false);
                editor.set_selection(sel(1, 1, 1, 4).into(), None);
                find_controller.find_state().change(
                    &INewFindReplaceState { search_string: Some(JsString::from("bar")), is_revealed: Some(true), ..Default::default() },
                    false,
                );
                run(&add_selection_to_next, editor);

                set_text_focus(editor, true);
                run(&add_selection_to_next, editor);

                assert_eq!(selections_of(editor), vec![[1, 1, 1, 4], [2, 1, 2, 4], [3, 1, 3, 4]]);
            });
        }

        #[test]
        fn a_selection_session_does_not_survive_replacing_the_editor_model() {
            test_multicursor(&TEXT, editor_options(None, None), |editor, _, ds| {
                let action = add_selection_to_next_find_match_action(source());
                editor.set_selection(sel(1, 1, 1, 4).into(), None);
                run(&action, editor);
                let replacement = create_text_model("bar\nBAR\nbarista");
                let disposable_replacement = replacement.clone();
                ds.add(to_disposable(move || disposable_replacement.dispose()));
                editor.set_model(Some(replacement));
                editor.set_selection(sel(1, 1, 1, 4).into(), None);
                update_selected_text_match_mode(editor, "caseSensitive");
                run(&action, editor);

                assert_eq!(selections_of(editor), vec![[1, 1, 1, 4], [3, 1, 3, 4]]);
            });
        }

        #[test]
        fn multiline_crlf_selections_match_literal_substrings_with_match_case_false() {
            test_multicursor(
                &["foo.", "bar", "FOO.", "BAR", "xfoo.", "bar"],
                mode_options("caseInsensitive"),
                |editor, find_controller, _| {
                    editor.get_model().expect("o editor tem o modelo").set_eol(EndOfLineSequence::CrLf);
                    find_controller.find_state().change(
                        &INewFindReplaceState {
                            search_string: Some(JsString::from("unrelated.*")),
                            is_revealed: Some(true),
                            match_case: Some(true),
                            whole_word: Some(true),
                            is_regex: Some(true),
                            ..Default::default()
                        },
                        false,
                    );
                    editor.set_selection(sel(1, 1, 2, 4).into(), None);
                    run(&select_highlights_action(source()), editor);

                    assert_eq!(selections_of(editor), vec![[1, 1, 2, 4], [3, 1, 4, 4], [5, 2, 6, 4]]);
                },
            );
        }

        #[test]
        fn multiline_crlf_selections_match_literal_substrings_with_match_case_true() {
            test_multicursor(
                &["foo.", "bar", "FOO.", "BAR", "xfoo.", "bar"],
                mode_options("caseSensitive"),
                |editor, find_controller, _| {
                    editor.get_model().expect("o editor tem o modelo").set_eol(EndOfLineSequence::CrLf);
                    find_controller.find_state().change(
                        &INewFindReplaceState {
                            search_string: Some(JsString::from("unrelated.*")),
                            is_revealed: Some(true),
                            match_case: Some(false),
                            whole_word: Some(true),
                            is_regex: Some(true),
                            ..Default::default()
                        },
                        false,
                    );
                    editor.set_selection(sel(1, 1, 2, 4).into(), None);
                    run(&select_highlights_action(source()), editor);

                    assert_eq!(selections_of(editor), vec![[1, 1, 2, 4], [5, 2, 6, 4]]);
                },
            );
        }

        /// O estado de busca de `bar` (sem caixa, palavra inteira) dos três casos `Find-focused commands retain Find
        /// matching rules`.
        fn find_bar_whole_word_case_insensitive(find_controller: &TestFindController) {
            find_controller.find_state().change(
                &INewFindReplaceState {
                    search_string: Some(JsString::from("bar")),
                    is_revealed: Some(true),
                    match_case: Some(false),
                    whole_word: Some(true),
                    ..Default::default()
                },
                false,
            );
        }

        #[test]
        fn add_selection_to_next_find_focused_commands_retain_find_matching_rules() {
            test_multicursor(
                &["foo", "BAR", "bar", "barista", "foo"],
                editor_options(Some(false), Some("caseSensitive")),
                |editor, find_controller, _| {
                    editor.set_selection(sel(1, 1, 1, 4).into(), None);
                    find_bar_whole_word_case_insensitive(find_controller);
                    update_selected_text_match_mode(editor, "caseInsensitive");
                    update_selected_text_match_mode(editor, "caseSensitive");
                    run(&actions().add_selection_to_next, editor);

                    assert_eq!(selections_of(editor), vec![[1, 1, 1, 4], [2, 1, 2, 4]]);
                },
            );
        }

        #[test]
        fn add_selection_to_previous_find_focused_commands_retain_find_matching_rules() {
            test_multicursor(
                &["foo", "BAR", "bar", "barista", "foo"],
                editor_options(Some(false), Some("caseSensitive")),
                |editor, find_controller, _| {
                    editor.set_selection(sel(5, 1, 5, 4).into(), None);
                    find_bar_whole_word_case_insensitive(find_controller);
                    update_selected_text_match_mode(editor, "caseInsensitive");
                    update_selected_text_match_mode(editor, "caseSensitive");
                    run(&actions().add_selection_to_previous, editor);

                    assert_eq!(selections_of(editor), vec![[5, 1, 5, 4], [3, 1, 3, 4]]);
                },
            );
        }

        #[test]
        fn select_highlights_find_focused_commands_retain_find_matching_rules() {
            test_multicursor(
                &["foo", "BAR", "bar", "barista", "foo"],
                editor_options(Some(false), Some("caseSensitive")),
                |editor, find_controller, _| {
                    editor.set_selection(sel(1, 1, 1, 4).into(), None);
                    find_bar_whole_word_case_insensitive(find_controller);
                    update_selected_text_match_mode(editor, "caseInsensitive");
                    update_selected_text_match_mode(editor, "caseSensitive");
                    run(&actions().select_highlights, editor);

                    assert_eq!(selections_of(editor), vec![[2, 1, 2, 4], [3, 1, 3, 4]]);
                },
            );
        }

        #[test]
        fn find_focused_regex_select_all_follows_find_options_in_both_independent_matching_modes() {
            for (selected_text_match_mode, match_case, expected_lines) in
                [("caseSensitive", false, vec![1, 4, 5]), ("caseInsensitive", true, vec![1, 5])]
            {
                test_multicursor(
                    &TEXT,
                    editor_options(Some(false), Some(selected_text_match_mode)),
                    |editor, find_controller, _| {
                        find_controller.find_state().change(
                            &INewFindReplaceState {
                                search_string: Some(JsString::from("^foo$")),
                                is_revealed: Some(true),
                                match_case: Some(match_case),
                                is_regex: Some(true),
                                ..Default::default()
                            },
                            false,
                        );
                        editor.set_selection(sel(1, 1, 1, 4).into(), None);

                        run(&actions().select_highlights, editor);

                        assert_eq!(selections_of(editor), lines_1_to_4(&expected_lines));
                    },
                );
            }
        }

        #[test]
        fn add_next_occurrence_does_not_highlight_unrelated_find_options() {
            let a = actions();
            assert_find_options_feedback(|editor| run(&a.add_selection_to_next, editor), 1, &[1, 2], true);
        }

        #[test]
        fn add_previous_occurrence_does_not_highlight_unrelated_find_options() {
            let a = actions();
            assert_find_options_feedback(|editor| run(&a.add_selection_to_previous, editor), 5, &[5, 4], true);
        }

        #[test]
        fn select_all_occurrences_does_not_highlight_unrelated_find_options() {
            let a = actions();
            assert_find_options_feedback(|editor| run(&a.select_highlights, editor), 1, &[1, 2, 3, 4, 5], true);
        }

        #[test]
        fn find_focused_occurrence_selection_still_highlights_find_options() {
            let a = actions();
            assert_find_options_feedback(|editor| run(&a.add_selection_to_next, editor), 1, &[1, 5], false);
        }
    }

    mod selected_text_occurrence_matching_highlights {
        use super::*;

        const TEXT: [&str; 4] = ["foo", "FOO", "fooBar", "FOOBar"];

        /// `editor.registerAndInstantiateContribution(SelectionHighlighter.ID, SelectionHighlighter)`.
        fn register_selection_highlighter(editor: &TestCodeEditor) {
            editor.register_and_instantiate_contribution(SelectionHighlighter::ID, |widget, _| {
                SelectionHighlighter::new(widget, Rc::new(NoDocumentHighlightProviders), source())
            });
        }

        /// `highlights(editor)`.
        fn highlights(editor: &TestCodeEditor) -> Vec<[i32; 4]> {
            let model = editor.get_model().expect("o editor tem o modelo");
            let mut ranges: Vec<Range> = model
                .get_all_decorations(0, false, false)
                .into_iter()
                .filter(|decoration| decoration.options.class_name.as_deref() == Some("selectionHighlight"))
                .map(|decoration| decoration.range)
                .collect();
            ranges.sort_by(|a, b| Range::compare_ranges_using_starts(Some(a), Some(b)).cmp(&0));
            ranges.iter().map(from_range).collect()
        }

        fn assert_selection_highlighting(selected_text_match_mode: &str, has_text_focus: bool, expected_lines: &[i32]) {
            test_multicursor(
                &TEXT,
                editor_options(Some(has_text_focus), Some(selected_text_match_mode)),
                |editor, find_controller, _| {
                    register_selection_highlighter(editor);
                    find_controller.find_state().change(
                        &INewFindReplaceState {
                            search_string: Some(JsString::from("f.*")),
                            is_revealed: Some(true),
                            match_case: Some(selected_text_match_mode != "caseSensitive"),
                            whole_word: Some(true),
                            is_regex: Some(true),
                            ..Default::default()
                        },
                        false,
                    );
                    editor.set_selection(sel(1, 1, 1, 4).into(), None);

                    assert_eq!(highlights(editor), lines_1_to_4(expected_lines));
                },
            );
        }

        fn assert_duplicate_find_highlights(selected_text_match_mode: &str, expected_different_case_lines: &[i32]) {
            test_multicursor(&TEXT, mode_options(selected_text_match_mode), |editor, find_controller, _| {
                register_selection_highlighter(editor);
                editor.set_selection(sel(1, 1, 1, 4).into(), None);
                let match_case = selected_text_match_mode == "caseSensitive";
                find_controller.find_state().change(
                    &INewFindReplaceState {
                        search_string: Some(JsString::from("foo")),
                        is_revealed: Some(true),
                        match_case: Some(match_case),
                        whole_word: Some(false),
                        ..Default::default()
                    },
                    false,
                );
                let same_options = highlights(editor);

                find_controller.find_state().change(&INewFindReplaceState { match_case: Some(!match_case), ..Default::default() }, false);
                assert_eq!(
                    (same_options, highlights(editor)),
                    (Vec::<[i32; 4]>::new(), lines_1_to_4(expected_different_case_lines))
                );
            });
        }

        #[test]
        fn selection_highlighting_uses_match_case_false_editor_focused_true() {
            assert_selection_highlighting("caseInsensitive", true, &[2, 3, 4]);
        }

        #[test]
        fn only_suppress_duplicate_find_highlights_with_the_same_matching_rules_match_case_false() {
            assert_duplicate_find_highlights("caseInsensitive", &[2, 3, 4]);
        }

        #[test]
        fn selection_highlighting_uses_match_case_true_editor_focused_true() {
            assert_selection_highlighting("caseSensitive", true, &[3]);
        }

        #[test]
        fn only_suppress_duplicate_find_highlights_with_the_same_matching_rules_match_case_true() {
            assert_duplicate_find_highlights("caseSensitive", &[3]);
        }

        #[test]
        fn selection_highlighting_refreshes_on_case_only_configuration_changes_without_cursor_movement() {
            let mut actual: Vec<Vec<[i32; 4]>> = Vec::new();
            runtime::with_faked_timers(|| {
                test_multicursor(&TEXT, editor_options(None, None), |editor, _, _| {
                    register_selection_highlighter(editor);
                    editor.set_selection(sel(1, 1, 1, 4).into(), None);
                    // Drain the initial scheduled update so it cannot mask missing configuration invalidation.
                    runtime::run_until_idle();
                    actual.push(highlights(editor));

                    update_selected_text_match_mode(editor, "caseSensitive");
                    runtime::run_until_idle();
                    actual.push(highlights(editor));

                    update_selected_text_match_mode(editor, "caseInsensitive");
                    runtime::run_until_idle();
                    actual.push(highlights(editor));
                });
            });

            assert_eq!(
                actual,
                vec![lines_1_to_4(&[2, 3, 4]), lines_1_to_4(&[3]), lines_1_to_4(&[2, 3, 4])]
            );
        }

        #[test]
        fn find_regex_select_all_discards_an_existing_selection_driven_session() {
            test_multicursor(
                &["foo", "fooBar", "bar", "barista"],
                mode_options("caseInsensitive"),
                |editor, find_controller, _| {
                    register_selection_highlighter(editor);
                    editor.set_selection(sel(1, 1, 1, 4).into(), None);
                    let action = add_selection_to_next_find_match_action(source());
                    run(&action, editor);

                    find_controller.find_state().change(
                        &INewFindReplaceState {
                            search_string: Some(JsString::from("bar$")),
                            is_revealed: Some(true),
                            is_regex: Some(true),
                            match_case: Some(true),
                            ..Default::default()
                        },
                        false,
                    );
                    set_text_focus(editor, false);
                    run(&select_highlights_action(source()), editor);
                    find_controller.find_state().change(&INewFindReplaceState { is_revealed: Some(false), ..Default::default() }, false);
                    let after_find = highlights(editor);

                    set_text_focus(editor, true);
                    run(&action, editor);

                    assert_eq!(
                        (after_find, selections_of(editor)),
                        (vec![[2, 4, 2, 7], [4, 1, 4, 4]], vec![[3, 1, 3, 4], [4, 1, 4, 4]])
                    );
                },
            );
        }
    }

    mod find_state_disassociation {
        use super::*;

        const TEXT: [&str; 6] = ["app", "apples", "whatsapp", "app", "App", " app"];

        #[test]
        fn caret_started_sessions_temporarily_override_and_restore_find_options_in_every_matching_mode() {
            for selected_text_match_mode in [None, Some("findOptions"), Some("caseSensitive"), Some("caseInsensitive")] {
                for end_session in ["selection", "blur", "dispose", "configuration"] {
                    test_multicursor(&TEXT, editor_options(None, selected_text_match_mode), |editor, find_controller, _| {
                        let state = find_controller.find_state().clone();
                        state.change(
                            &INewFindReplaceState {
                                match_case: Some(false),
                                whole_word: Some(false),
                                is_regex: Some(true),
                                ..Default::default()
                            },
                            false,
                        );
                        let find_options = || {
                            (
                                [state.match_case(), state.whole_word(), state.is_regex()],
                                [state.actual_match_case(), state.actual_whole_word(), state.actual_is_regex()],
                            )
                        };
                        find_controller.spy_on_highlight_find_options();
                        editor.set_selection(sel(1, 2, 1, 2).into(), None);
                        let action = add_selection_to_next_find_match_action(source());
                        run(&action, editor);
                        run(&action, editor);

                        let during_session = (
                            state.search_string().to_string_lossy(),
                            find_options(),
                            selections_of(editor),
                            find_controller.highlight_find_options_call_count(),
                        );

                        match end_session {
                            "selection" => editor.set_selection(sel(2, 1, 2, 4).into(), None),
                            "blur" => set_text_focus(editor, false),
                            "dispose" => {
                                MultiCursorSelectionController::get(editor).expect("o controlador está registrado").dispose();
                            }
                            "configuration" => {
                                let next = if editor.get_option(SELECTED_TEXT_MATCH_MODE) == "findOptions" {
                                    "caseInsensitive"
                                } else {
                                    "findOptions"
                                };
                                update_selected_text_match_mode(editor, next);
                            }
                            other => unreachable!("fim de sessão desconhecido: {other}"),
                        }

                        assert_eq!(
                            (during_session, find_options()),
                            (
                                (
                                    "app".to_owned(),
                                    ([true, true, false], [false, false, true]),
                                    vec![[1, 1, 1, 4], [4, 1, 4, 4]],
                                    1usize,
                                ),
                                ([false, false, true], [false, false, true]),
                            ),
                            "{}: {}",
                            selected_text_match_mode.unwrap_or("default"),
                            end_session
                        );
                    });
                }
            }
        }

        #[test]
        fn changing_find_options_ends_a_default_caret_started_session_without_reverting_user_preferences() {
            test_add_selection_to_next_find_match_action(&TEXT, |editor, action, find_controller| {
                let state = find_controller.find_state();
                state.change(
                    &INewFindReplaceState { match_case: Some(false), whole_word: Some(false), is_regex: Some(true), ..Default::default() },
                    false,
                );
                editor.set_selection(sel(1, 2, 1, 2).into(), None);
                run(action, editor);

                state.change(&INewFindReplaceState { match_case: Some(false), whole_word: Some(true), ..Default::default() }, false);
                run(action, editor);
                run(action, editor);

                assert_eq!(
                    (
                        selections_of(editor),
                        [state.match_case(), state.whole_word(), state.is_regex()],
                        [state.actual_match_case(), state.actual_whole_word(), state.actual_is_regex()],
                    ),
                    (
                        vec![[1, 1, 1, 4], [4, 1, 4, 4], [5, 1, 5, 4]],
                        [false, true, true],
                        [false, true, true],
                    )
                );
            });
        }

        #[test]
        fn enters_mode() {
            test_add_selection_to_next_find_match_action(&TEXT, |editor, action, _| {
                editor.set_selections(&[sel(1, 2, 1, 2)], None, None);

                run(action, editor);
                assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4)]);

                run(action, editor);
                assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(4, 1, 4, 4)]);

                run(action, editor);
                assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(4, 1, 4, 4), sel(6, 2, 6, 5)]);
            });
        }

        #[test]
        fn leaves_mode_when_selection_changes() {
            test_add_selection_to_next_find_match_action(&TEXT, |editor, action, _| {
                editor.set_selections(&[sel(1, 2, 1, 2)], None, None);

                run(action, editor);
                assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4)]);

                run(action, editor);
                assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(4, 1, 4, 4)]);

                // change selection
                editor.set_selections(&[sel(1, 1, 1, 4)], None, None);

                run(action, editor);
                assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(2, 1, 2, 4)]);
            });
        }

        #[test]
        fn select_highlights_respects_mode() {
            test_multicursor(&TEXT, editor_options(None, None), |editor, _, _| {
                let action = select_highlights_action(source());
                editor.set_selections(&[sel(1, 2, 1, 2)], None, None);

                run(&action, editor);
                assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(4, 1, 4, 4), sel(6, 2, 6, 5)]);

                run(&action, editor);
                assert_eq!(editor.get_selections().unwrap(), vec![sel(1, 1, 1, 4), sel(4, 1, 4, 4), sel(6, 2, 6, 5)]);
            });
        }
    }
}
