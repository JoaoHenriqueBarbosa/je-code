//! Porte de `src/vs/editor/test/common/model/model.modes.test.ts` (suítes `Editor Model - Model Modes 1` e `Editor Model - Model Modes 2`), caso a caso e na mesma ordem.
//!
//! Cada `suite` do original vira um `mod`.
//!
//! Notas de tradução:
//! - O `setup`/`teardown` do original vira `with_model`, que registra o suporte de tokenização no
//!   `TokenizationRegistry` global (um por thread, então os casos não se misturam), cria o modelo, roda o caso
//!   e descarta o modelo e o registro, nessa ordem.
//! - O `calledFor` do original é uma variável do módulo; aqui é uma lista por caso, compartilhada com o
//!   suporte, e `getAndClear` é o fecho que o caso recebe.
//! - `tokenize: undefined!` vira um pânico: o modelo só chama `tokenizeEncoded`.
//! - O `ModelState2` mutável do original (o `tokenizeEncoded` escreve em `state.prevLineContent`) usa
//!   `RefCell` para a mutação por trás do `Rc<dyn IState>`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use je_editor::cursor_common::CursorSimpleModel;
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::edit_operation::EditOperation;
use je_editor::languages::{
    EncodedTokenizationResult, ITokenizationSupport, IState, State, TokenizationResult,
};
use je_editor::null_tokenize::null_state;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_editor::tokenization_registry::tokenization_registry;
use support::test_text_model::*;

type CalledFor = Rc<RefCell<Vec<String>>>;

/// `getAndClear()`.
fn get_and_clear(called_for: &CalledFor) -> Vec<String> {
    std::mem::take(&mut *called_for.borrow_mut())
}

fn assert_called(actual: Vec<String>, expected: &[&str]) {
    assert_eq!(actual, expected.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
}

/// `setup` + `teardown`: registra o suporte, cria o modelo, roda o caso e descarta o modelo e o registro.
fn with_model(
    text: &str,
    language_id: &str,
    support: impl FnOnce(CalledFor) -> Rc<dyn ITokenizationSupport>,
    test: impl FnOnce(&TextModel, &dyn Fn() -> Vec<String>),
) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let called_for: CalledFor = Rc::default();
    let language_registration = tokenization_registry().register(language_id, support(called_for.clone()));
    let this_model = create_text_model_with(text, Some(language_id), Default::default(), None);
    test(&this_model, &|| get_and_clear(&called_for));
    this_model.dispose();
    language_registration.dispose();
}

// --------- utilitários

mod editor_model_model_modes_1 {
    use super::*;

    struct TokenizationSupport1 {
        called_for: CalledFor,
    }

    impl ITokenizationSupport for TokenizationSupport1 {
        fn get_initial_state(&self) -> State {
            null_state()
        }

        fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
            unreachable!("`tokenize: undefined!` no original")
        }

        fn tokenize_encoded(&self, line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
            // `line.charAt(0)`
            let first = line.units().first().map(|u| String::from_utf16_lossy(&[*u])).unwrap_or_default();
            self.called_for.borrow_mut().push(first);
            EncodedTokenizationResult { tokens: Vec::new(), font_info: Vec::new(), end_state: state.clone() }
        }
    }

    fn with_model(test: impl FnOnce(&TextModel, &dyn Fn() -> Vec<String>)) {
        let text = "1\r\n2\n3\n4\r\n5";
        super::with_model(
            text,
            "modelModeTest1",
            |called_for| Rc::new(TokenizationSupport1 { called_for }),
            test,
        );
    }

    #[test]
    fn model_calls_syntax_highlighter_1() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(1);
            assert_called(get_and_clear(), &["1"]);
        });
    }

    #[test]
    fn model_calls_syntax_highlighter_2() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(2);
            assert_called(get_and_clear(), &["1", "2"]);

            this_model.tokenization().force_tokenization(2);
            assert_called(get_and_clear(), &[]);
        });
    }

    #[test]
    fn model_caches_states() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(1);
            assert_called(get_and_clear(), &["1"]);

            this_model.tokenization().force_tokenization(2);
            assert_called(get_and_clear(), &["2"]);

            this_model.tokenization().force_tokenization(3);
            assert_called(get_and_clear(), &["3"]);

            this_model.tokenization().force_tokenization(4);
            assert_called(get_and_clear(), &["4"]);

            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["5"]);

            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &[]);
        });
    }

    #[test]
    fn model_invalidates_states_for_one_line_insert() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["1", "2", "3", "4", "5"]);

            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 1), "-")]);
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["-"]);

            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &[]);
        });
    }

    #[test]
    fn model_invalidates_states_for_many_lines_insert() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["1", "2", "3", "4", "5"]);

            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 1), "0\n-\n+")]);
            assert_eq!(this_model.get_line_count(), 7);
            this_model.tokenization().force_tokenization(7);
            assert_called(get_and_clear(), &["0", "-", "+"]);

            this_model.tokenization().force_tokenization(7);
            assert_called(get_and_clear(), &[]);
        });
    }

    #[test]
    fn model_invalidates_states_for_one_new_line() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["1", "2", "3", "4", "5"]);

            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 2), "\n")]);
            this_model.apply_edits(&[EditOperation::insert(Position::new(2, 1), "a")]);
            this_model.tokenization().force_tokenization(6);
            assert_called(get_and_clear(), &["1", "a"]);
        });
    }

    #[test]
    fn model_invalidates_states_for_one_line_delete() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["1", "2", "3", "4", "5"]);

            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 2), "-")]);
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["1"]);

            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 1, 2))]);
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["-"]);

            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &[]);
        });
    }

    #[test]
    fn model_invalidates_states_for_many_lines_delete() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["1", "2", "3", "4", "5"]);

            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 3, 1))]);
            this_model.tokenization().force_tokenization(3);
            assert_called(get_and_clear(), &["3"]);

            this_model.tokenization().force_tokenization(3);
            assert_called(get_and_clear(), &[]);
        });
    }
}

mod editor_model_model_modes_2 {
    use super::*;

    /// `ModelState2`.
    struct ModelState2 {
        prev_line_content: RefCell<String>,
    }

    impl ModelState2 {
        fn new_state(prev_line_content: &str) -> State {
            Rc::new(ModelState2 { prev_line_content: RefCell::new(prev_line_content.to_owned()) })
        }
    }

    impl IState for ModelState2 {
        fn clone_state(&self) -> State {
            ModelState2::new_state(&self.prev_line_content.borrow())
        }

        fn equals(&self, other: &dyn IState) -> bool {
            other
                .as_any()
                .downcast_ref::<ModelState2>()
                .is_some_and(|other| *other.prev_line_content.borrow() == *self.prev_line_content.borrow())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    struct TokenizationSupport2 {
        called_for: CalledFor,
    }

    impl ITokenizationSupport for TokenizationSupport2 {
        fn get_initial_state(&self) -> State {
            ModelState2::new_state("")
        }

        fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
            unreachable!("`tokenize: undefined!` no original")
        }

        fn tokenize_encoded(&self, line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
            let line = line.to_string_lossy();
            self.called_for.borrow_mut().push(line.clone());
            let model_state = state.as_any().downcast_ref::<ModelState2>().expect("ModelState2");
            *model_state.prev_line_content.borrow_mut() = line;
            EncodedTokenizationResult { tokens: Vec::new(), font_info: Vec::new(), end_state: state.clone() }
        }
    }

    fn with_model(test: impl FnOnce(&TextModel, &dyn Fn() -> Vec<String>)) {
        let text = "Line1\r\nLine2\nLine3\nLine4\r\nLine5";
        super::with_model(
            text,
            "modelModeTest2",
            |called_for| Rc::new(TokenizationSupport2 { called_for }),
            test,
        );
    }

    #[test]
    fn get_tokens_for_invalid_lines_one_text_insert() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["Line1", "Line2", "Line3", "Line4", "Line5"]);
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 6), "-")]);
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["Line1-", "Line2"]);
        });
    }

    #[test]
    fn get_tokens_for_invalid_lines_two_text_insert() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["Line1", "Line2", "Line3", "Line4", "Line5"]);
            this_model.apply_edits(&[
                EditOperation::insert(Position::new(1, 6), "-"),
                EditOperation::insert(Position::new(3, 6), "-"),
            ]);

            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["Line1-", "Line2", "Line3-", "Line4"]);
        });
    }

    #[test]
    fn get_tokens_for_invalid_lines_one_multi_line_text_insert_one_small_text_insert() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["Line1", "Line2", "Line3", "Line4", "Line5"]);
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 6), "\nNew line\nAnother new line")]);
            this_model.apply_edits(&[EditOperation::insert(Position::new(5, 6), "-")]);
            this_model.tokenization().force_tokenization(7);
            assert_called(get_and_clear(), &["Line1", "New line", "Another new line", "Line2", "Line3-", "Line4"]);
        });
    }

    #[test]
    fn get_tokens_for_invalid_lines_one_delete_text() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["Line1", "Line2", "Line3", "Line4", "Line5"]);
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 1, 5))]);
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["1", "Line2"]);
        });
    }

    #[test]
    fn get_tokens_for_invalid_lines_one_line_delete_text() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["Line1", "Line2", "Line3", "Line4", "Line5"]);
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 2, 1))]);
            this_model.tokenization().force_tokenization(4);
            assert_called(get_and_clear(), &["Line2"]);
        });
    }

    #[test]
    fn get_tokens_for_invalid_lines_multiple_lines_delete_text() {
        with_model(|this_model, get_and_clear| {
            this_model.tokenization().force_tokenization(5);
            assert_called(get_and_clear(), &["Line1", "Line2", "Line3", "Line4", "Line5"]);
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 3, 3))]);
            this_model.tokenization().force_tokenization(3);
            assert_called(get_and_clear(), &["ne3", "Line4"]);
        });
    }
}
