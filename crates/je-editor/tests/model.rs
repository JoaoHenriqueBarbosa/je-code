//! Porte de `src/vs/editor/test/common/model/model.test.ts` (suítes `Editor Model - Model`, `Editor Model - Model Line Separators` e `Editor Model - Words`), caso a caso e na mesma ordem.
//!
//! Cada `suite` do original vira um `mod`.
//!
//! Notas de tradução:
//! - O `setup`/`teardown` do original vira `with_model`, que cria o modelo, roda o caso e o descarta.
//! - O view model espião do `withEventCapturing` implementa o trait `ViewModel` do `TextModel` (o `mock` do
//!   original só sobrescreve os dois métodos que o modelo chama). O `assert.fail` dentro do espião vira
//!   pânico, que derruba o caso do mesmo jeito.
//! - `applyEdits(ops, true)` é `apply_edits_computing_undo`.
//! - `createModelServices(disposables)` com os serviços de linguagem é `create_language_model_services`
//!   (`tests/support/test_text_model.rs`); `OuterMode` e `InnerMode` do original são as funções
//!   `register_outer_mode` e `register_inner_mode`, que devolvem o `Disposable` com os registros.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use je_editor::cursor_common::CursorSimpleModel;
use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, plain_disposable};
use je_editor::edit_operation::{EditOperation, SingleEditOperation};
use je_editor::encoded_token_attributes::metadata_consts;
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::ILanguageService;
use je_editor::languages::{EncodedTokenizationResult, ILanguageIdCodec, ITokenizationSupport, State, TokenizationResult};
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::null_tokenize::null_state;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::{ContentOrInjectedTextEvent, TextModel, ViewModel};
use je_editor::text_model_events::{
    ModelRawChange, ModelRawContentChangedEvent, ModelRawLineChanged, ModelRawLinesDeleted, ModelRawLinesInserted,
};
use je_editor::tokenization_registry::tokenization_registry;
use je_editor::word_helper::WordAtPosition;
use support::re;
use support::test_text_model::*;

// --------- utilitários

const LINE1: &str = "My First Line";
const LINE2: &str = "\t\tMy Second Line";
const LINE3: &str = "    Third Line";
const LINE4: &str = "";
const LINE5: &str = "1";

/// `new ModelRawLineChanged(lineNumber, lineNumberPostEdit)`.
fn line_changed(line_number: i32, line_number_post_edit: i32) -> ModelRawChange {
    ModelRawChange::LineChanged(ModelRawLineChanged { line_number, line_number_post_edit })
}

/// `new ModelRawLinesDeleted(fromLineNumber, toLineNumber, lastUntouchedLinePostEdit)`.
fn lines_deleted(from_line_number: i32, to_line_number: i32, last_untouched_line_post_edit: i32) -> ModelRawChange {
    ModelRawChange::LinesDeleted(ModelRawLinesDeleted { from_line_number, to_line_number, last_untouched_line_post_edit })
}

/// `new ModelRawLinesInserted(fromLineNumber, fromLineNumberPostEdit, count)`.
fn lines_inserted(from_line_number: i32, from_line_number_post_edit: i32, count: i32) -> ModelRawChange {
    ModelRawChange::LinesInserted(ModelRawLinesInserted { from_line_number, from_line_number_post_edit, count })
}

mod editor_model_model {
    use super::*;

    fn with_model(test: impl FnOnce(&TextModel)) {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let text = format!("{LINE1}\r\n{LINE2}\n{LINE3}\n{LINE4}\r\n{LINE5}");
        let this_model = create_text_model(&text);
        test(&this_model);
        this_model.dispose();
    }

    // --------- inserir texto

    #[test]
    fn model_get_value() {
        with_model(|this_model| {
            assert_eq!(this_model.get_value(), "My First Line\n\t\tMy Second Line\n    Third Line\n\n1");
        });
    }

    #[test]
    fn model_insert_empty_text() {
        with_model(|this_model| {
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 1), "")]);
            assert_eq!(this_model.get_line_count(), 5);
            assert_eq!(this_model.get_line_content(1), "My First Line");
        });
    }

    #[test]
    fn model_insert_text_without_newline_1() {
        with_model(|this_model| {
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 1), "foo ")]);
            assert_eq!(this_model.get_line_count(), 5);
            assert_eq!(this_model.get_line_content(1), "foo My First Line");
        });
    }

    #[test]
    fn model_insert_text_without_newline_2() {
        with_model(|this_model| {
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 3), " foo")]);
            assert_eq!(this_model.get_line_count(), 5);
            assert_eq!(this_model.get_line_content(1), "My foo First Line");
        });
    }

    #[test]
    fn model_insert_text_with_one_newline() {
        with_model(|this_model| {
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 3), " new line\nNo longer")]);
            assert_eq!(this_model.get_line_count(), 6);
            assert_eq!(this_model.get_line_content(1), "My new line");
            assert_eq!(this_model.get_line_content(2), "No longer First Line");
        });
    }

    #[test]
    fn model_insert_text_with_two_newlines() {
        with_model(|this_model| {
            this_model.apply_edits(&[EditOperation::insert(
                Position::new(1, 3),
                " new line\nOne more line in the middle\nNo longer",
            )]);
            assert_eq!(this_model.get_line_count(), 7);
            assert_eq!(this_model.get_line_content(1), "My new line");
            assert_eq!(this_model.get_line_content(2), "One more line in the middle");
            assert_eq!(this_model.get_line_content(3), "No longer First Line");
        });
    }

    #[test]
    fn model_insert_text_with_many_newlines() {
        with_model(|this_model| {
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 3), "\n\n\n\n")]);
            assert_eq!(this_model.get_line_count(), 9);
            assert_eq!(this_model.get_line_content(1), "My");
            assert_eq!(this_model.get_line_content(2), "");
            assert_eq!(this_model.get_line_content(3), "");
            assert_eq!(this_model.get_line_content(4), "");
            assert_eq!(this_model.get_line_content(5), " First Line");
        });
    }

    // --------- eventos de inserção de texto

    /// O view model espião do `withEventCapturing`.
    struct SpyViewModel {
        e: RefCell<Option<ModelRawContentChangedEvent>>,
    }

    impl ViewModel for SpyViewModel {
        fn on_did_change_content_or_injected_text(&self, e: &ContentOrInjectedTextEvent) {
            let mut captured = self.e.borrow_mut();
            match e {
                ContentOrInjectedTextEvent::Content(e) if captured.is_none() => {
                    *captured = Some(e.raw_content_changed_event.clone());
                }
                _ => panic!("Unexpected assertion error"),
            }
        }

        fn emit_content_change_event(&self, _e: &ContentOrInjectedTextEvent) {}
    }

    fn with_event_capturing(this_model: &TextModel, callback: impl FnOnce()) -> Option<ModelRawContentChangedEvent> {
        let spy = Rc::new(SpyViewModel { e: RefCell::new(None) });
        let spy_view_model: Rc<dyn ViewModel> = spy.clone();
        this_model.register_view_model(spy_view_model.clone());
        callback();
        this_model.unregister_view_model(&spy_view_model);
        spy.e.borrow_mut().take()
    }

    #[test]
    fn model_insert_empty_text_does_not_trigger_eventing() {
        with_model(|this_model| {
            let e = with_event_capturing(this_model, || {
                this_model.apply_edits(&[EditOperation::insert(Position::new(1, 1), "")]);
            });
            assert_eq!(e, None, "was not expecting event");
        });
    }

    #[test]
    fn model_insert_text_without_newline_eventing() {
        with_model(|this_model| {
            let e = with_event_capturing(this_model, || {
                this_model.apply_edits(&[EditOperation::insert(Position::new(1, 1), "foo ")]);
            });
            assert_eq!(e, Some(ModelRawContentChangedEvent::new(vec![line_changed(1, 1)], 2, false, false)));
        });
    }

    #[test]
    fn model_insert_text_with_one_newline_eventing() {
        with_model(|this_model| {
            let e = with_event_capturing(this_model, || {
                this_model.apply_edits(&[EditOperation::insert(Position::new(1, 3), " new line\nNo longer")]);
            });
            assert_eq!(
                e,
                Some(ModelRawContentChangedEvent::new(vec![line_changed(1, 1), lines_inserted(2, 2, 1)], 2, false, false))
            );
        });
    }

    // --------- apagar texto

    #[test]
    fn model_delete_empty_text() {
        with_model(|this_model| {
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 1, 1))]);
            assert_eq!(this_model.get_line_count(), 5);
            assert_eq!(this_model.get_line_content(1), "My First Line");
        });
    }

    #[test]
    fn model_delete_text_from_one_line() {
        with_model(|this_model| {
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 1, 2))]);
            assert_eq!(this_model.get_line_count(), 5);
            assert_eq!(this_model.get_line_content(1), "y First Line");
        });
    }

    #[test]
    fn model_delete_text_from_one_line_2() {
        with_model(|this_model| {
            this_model.apply_edits(&[EditOperation::insert(Position::new(1, 1), "a")]);
            assert_eq!(this_model.get_line_content(1), "aMy First Line");

            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 2, 1, 4))]);
            assert_eq!(this_model.get_line_count(), 5);
            assert_eq!(this_model.get_line_content(1), "a First Line");
        });
    }

    #[test]
    fn model_delete_all_text_from_a_line() {
        with_model(|this_model| {
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 1, 14))]);
            assert_eq!(this_model.get_line_count(), 5);
            assert_eq!(this_model.get_line_content(1), "");
        });
    }

    #[test]
    fn model_delete_text_from_two_lines() {
        with_model(|this_model| {
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 4, 2, 6))]);
            assert_eq!(this_model.get_line_count(), 4);
            assert_eq!(this_model.get_line_content(1), "My Second Line");
        });
    }

    #[test]
    fn model_delete_text_from_many_lines() {
        with_model(|this_model| {
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 4, 3, 5))]);
            assert_eq!(this_model.get_line_count(), 3);
            assert_eq!(this_model.get_line_content(1), "My Third Line");
        });
    }

    #[test]
    fn model_delete_everything() {
        with_model(|this_model| {
            this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 5, 2))]);
            assert_eq!(this_model.get_line_count(), 1);
            assert_eq!(this_model.get_line_content(1), "");
        });
    }

    // --------- eventos de apagar texto

    #[test]
    fn model_delete_empty_text_does_not_trigger_eventing() {
        with_model(|this_model| {
            let e = with_event_capturing(this_model, || {
                this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 1, 1))]);
            });
            assert_eq!(e, None, "was not expecting event");
        });
    }

    #[test]
    fn model_delete_text_from_one_line_eventing() {
        with_model(|this_model| {
            let e = with_event_capturing(this_model, || {
                this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 1, 2))]);
            });
            assert_eq!(e, Some(ModelRawContentChangedEvent::new(vec![line_changed(1, 1)], 2, false, false)));
        });
    }

    #[test]
    fn model_delete_all_text_from_a_line_eventing() {
        with_model(|this_model| {
            let e = with_event_capturing(this_model, || {
                this_model.apply_edits(&[EditOperation::delete(Range::new(1, 1, 1, 14))]);
            });
            assert_eq!(e, Some(ModelRawContentChangedEvent::new(vec![line_changed(1, 1)], 2, false, false)));
        });
    }

    #[test]
    fn model_delete_text_from_two_lines_eventing() {
        with_model(|this_model| {
            let e = with_event_capturing(this_model, || {
                this_model.apply_edits(&[EditOperation::delete(Range::new(1, 4, 2, 6))]);
            });
            assert_eq!(
                e,
                Some(ModelRawContentChangedEvent::new(vec![line_changed(1, 1), lines_deleted(2, 2, 1)], 2, false, false))
            );
        });
    }

    #[test]
    fn model_delete_text_from_many_lines_eventing() {
        with_model(|this_model| {
            let e = with_event_capturing(this_model, || {
                this_model.apply_edits(&[EditOperation::delete(Range::new(1, 4, 3, 5))]);
            });
            assert_eq!(
                e,
                Some(ModelRawContentChangedEvent::new(vec![line_changed(1, 1), lines_deleted(2, 3, 1)], 2, false, false))
            );
        });
    }

    // --------- getValueInRange

    #[test]
    fn get_value_in_range() {
        with_model(|this_model| {
            assert_eq!(this_model.get_value_in_range(&Range::new(1, 1, 1, 1)), "");
            assert_eq!(this_model.get_value_in_range(&Range::new(1, 1, 1, 2)), "M");
            assert_eq!(this_model.get_value_in_range(&Range::new(1, 2, 1, 3)), "y");
            assert_eq!(this_model.get_value_in_range(&Range::new(1, 1, 1, 14)), "My First Line");
            assert_eq!(this_model.get_value_in_range(&Range::new(1, 1, 2, 1)), "My First Line\n");
            assert_eq!(this_model.get_value_in_range(&Range::new(1, 1, 2, 2)), "My First Line\n\t");
            assert_eq!(this_model.get_value_in_range(&Range::new(1, 1, 2, 3)), "My First Line\n\t\t");
            assert_eq!(this_model.get_value_in_range(&Range::new(1, 1, 2, 17)), "My First Line\n\t\tMy Second Line");
            assert_eq!(this_model.get_value_in_range(&Range::new(1, 1, 3, 1)), "My First Line\n\t\tMy Second Line\n");
            assert_eq!(
                this_model.get_value_in_range(&Range::new(1, 1, 4, 1)),
                "My First Line\n\t\tMy Second Line\n    Third Line\n"
            );
        });
    }

    // --------- getValueLengthInRange

    #[test]
    fn get_value_length_in_range() {
        with_model(|this_model| {
            assert_eq!(this_model.get_value_length_in_range(&Range::new(1, 1, 1, 1)), "".len());
            assert_eq!(this_model.get_value_length_in_range(&Range::new(1, 1, 1, 2)), "M".len());
            assert_eq!(this_model.get_value_length_in_range(&Range::new(1, 2, 1, 3)), "y".len());
            assert_eq!(this_model.get_value_length_in_range(&Range::new(1, 1, 1, 14)), "My First Line".len());
            assert_eq!(this_model.get_value_length_in_range(&Range::new(1, 1, 2, 1)), "My First Line\n".len());
            assert_eq!(this_model.get_value_length_in_range(&Range::new(1, 1, 2, 2)), "My First Line\n\t".len());
            assert_eq!(this_model.get_value_length_in_range(&Range::new(1, 1, 2, 3)), "My First Line\n\t\t".len());
            assert_eq!(
                this_model.get_value_length_in_range(&Range::new(1, 1, 2, 17)),
                "My First Line\n\t\tMy Second Line".len()
            );
            assert_eq!(
                this_model.get_value_length_in_range(&Range::new(1, 1, 3, 1)),
                "My First Line\n\t\tMy Second Line\n".len()
            );
            assert_eq!(
                this_model.get_value_length_in_range(&Range::new(1, 1, 4, 1)),
                "My First Line\n\t\tMy Second Line\n    Third Line\n".len()
            );
        });
    }

    // --------- setValue

    #[test]
    fn set_value_eventing() {
        with_model(|this_model| {
            let e = with_event_capturing(this_model, || {
                this_model.set_value("new value".into());
            });
            assert_eq!(e, Some(ModelRawContentChangedEvent::new(vec![ModelRawChange::Flush], 2, false, false)));
        });
    }

    #[test]
    fn issue_46342_maintain_edit_operation_order_in_apply_edits() {
        with_model(|this_model| {
            let res = this_model.apply_edits_computing_undo(&[
                SingleEditOperation::new(Range::new(2, 1, 2, 1), Some(JsString::from("a")), false),
                SingleEditOperation::new(Range::new(1, 1, 1, 1), Some(JsString::from("b")), false),
            ]);

            assert_eq!(res[0].range, Range::new(2, 1, 2, 2));
            assert_eq!(res[1].range, Range::new(1, 1, 1, 2));
        });
    }
}

// --------- O caractere especial LINE SEPARATOR do Unicode
mod editor_model_model_line_separators {
    use super::*;

    fn with_model(test: impl FnOnce(&TextModel)) {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let text = format!("{LINE1}\u{2028}{LINE2}\n{LINE3}\u{2028}{LINE4}\r\n{LINE5}");
        let this_model = create_text_model(&text);
        test(&this_model);
        this_model.dispose();
    }

    #[test]
    fn model_get_value() {
        with_model(|this_model| {
            assert_eq!(this_model.get_value(), "My First Line\u{2028}\t\tMy Second Line\n    Third Line\u{2028}\n1");
        });
    }

    #[test]
    fn model_lines() {
        with_model(|this_model| {
            assert_eq!(this_model.get_line_count(), 3);
        });
    }

    #[test]
    /// `Bug 13333:Model should line break on lonely CR too`
    fn bug_13333_model_should_line_break_on_lonely_cr_too() {
        with_model(|_this_model| {
            let model = create_text_model("Hello\rWorld!\r\nAnother line");
            assert_eq!(model.get_line_count(), 3);
            assert_eq!(model.get_value(), "Hello\r\nWorld!\r\nAnother line");
            model.dispose();
        });
    }
}

// --------- Palavras

mod editor_model_words {
    use super::*;

    fn word(word: &str, start_column: i32, end_column: i32) -> Option<WordAtPosition> {
        Some(WordAtPosition { word: JsString::from(word), start_column, end_column })
    }

    #[test]
    fn get_word_at_position() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let text = ["This text has some  words. "];
        let this_model = create_text_model(&text.join("\n"));

        assert_eq!(this_model.get_word_at_position(Position::new(1, 1)), word("This", 1, 5));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 2)), word("This", 1, 5));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 4)), word("This", 1, 5));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 5)), word("This", 1, 5));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 6)), word("text", 6, 10));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 19)), word("some", 15, 19));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 20)), None);
        assert_eq!(this_model.get_word_at_position(Position::new(1, 21)), word("words", 21, 26));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 26)), word("words", 21, 26));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 27)), None);
        assert_eq!(this_model.get_word_at_position(Position::new(1, 28)), None);

        // o teardown do original descarta o modelo
        this_model.dispose();
    }

    const OUTER_LANGUAGE_ID: &str = "outerMode";
    const INNER_LANGUAGE_ID: &str = "innerMode";

    /// O tokenizador do `OuterMode`: cada `x` é da linguagem interna e o resto, da externa.
    struct OuterModeTokenizationSupport {
        language_id_codec: Rc<dyn ILanguageIdCodec>,
    }

    impl ITokenizationSupport for OuterModeTokenizationSupport {
        fn get_initial_state(&self) -> State {
            null_state()
        }

        fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
            unreachable!("`tokenize: undefined!` no original")
        }

        fn tokenize_encoded(&self, line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
            let mut tokens: Vec<u32> = Vec::new();
            let mut prev_language_id: Option<&str> = None;
            for (i, unit) in line.units().iter().enumerate() {
                let language_id = if *unit == u16::from(b'x') { INNER_LANGUAGE_ID } else { OUTER_LANGUAGE_ID };
                let encoded_language_id = self.language_id_codec.encode_language_id(language_id);
                if prev_language_id != Some(language_id) {
                    tokens.push(i as u32);
                    tokens.push(encoded_language_id << metadata_consts::LANGUAGEID_OFFSET);
                }
                prev_language_id = Some(language_id);
            }
            EncodedTokenizationResult { tokens, font_info: Vec::new(), end_state: state.clone() }
        }
    }

    /// `class OuterMode`: registra a linguagem, a configuração vazia e o tokenizador.
    fn register_outer_mode(services: &LanguageModelServices) -> Rc<Disposable> {
        let mode = Disposable::new();
        mode.register(services.language_service.register_language(LanguageExtensionPoint::new(OUTER_LANGUAGE_ID)));
        mode.register(services.language_configuration_service.register(
            OUTER_LANGUAGE_ID,
            LanguageConfiguration::default(),
            None,
        ));

        let support = OuterModeTokenizationSupport { language_id_codec: services.language_service.language_id_codec() };
        mode.register(tokenization_registry().register(OUTER_LANGUAGE_ID, Rc::new(support)));
        mode
    }

    /// `class InnerMode`: registra a linguagem e a configuração vazia.
    fn register_inner_mode(services: &LanguageModelServices) -> Rc<Disposable> {
        let mode = Disposable::new();
        mode.register(services.language_service.register_language(LanguageExtensionPoint::new(INNER_LANGUAGE_ID)));
        mode.register(services.language_configuration_service.register(
            INNER_LANGUAGE_ID,
            LanguageConfiguration::default(),
            None,
        ));
        mode
    }

    /// `disposables.add(model)`: o descarte do modelo, para o `DisposableStore` do caso.
    fn dispose_with(model: &Rc<TextModel>) -> DisposableRef {
        let model = model.clone();
        plain_disposable(move || model.dispose())
    }

    #[test]
    fn get_word_at_position_at_embedded_language_boundaries() {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let services = create_language_model_services(&guard);
        guard.add_rc(register_outer_mode(&services));
        guard.add_rc(register_inner_mode(&services));

        let model = instantiate_text_model_with_languages(&services, "ab<xx>ab<x>", OUTER_LANGUAGE_ID);
        guard.add(dispose_with(&model));

        assert_eq!(model.get_word_at_position(Position::new(1, 1)), word("ab", 1, 3));
        assert_eq!(model.get_word_at_position(Position::new(1, 2)), word("ab", 1, 3));
        assert_eq!(model.get_word_at_position(Position::new(1, 3)), word("ab", 1, 3));
        assert_eq!(model.get_word_at_position(Position::new(1, 4)), word("xx", 4, 6));
        assert_eq!(model.get_word_at_position(Position::new(1, 5)), word("xx", 4, 6));
        assert_eq!(model.get_word_at_position(Position::new(1, 6)), word("xx", 4, 6));
        assert_eq!(model.get_word_at_position(Position::new(1, 7)), word("ab", 7, 9));

        guard.store().dispose();
    }

    #[test]
    fn issue_61296_vs_code_freezes_when_editing_css_file_with_emoji() {
        const MODE_ID: &str = "testMode";
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let services = create_language_model_services(&guard);

        guard.add(services.language_service.register_language(LanguageExtensionPoint::new(MODE_ID)));
        guard.add(services.language_configuration_service.register(
            MODE_ID,
            LanguageConfiguration {
                word_pattern: Some(re(r"(#?-?\d*\.\d\w*%?)|(::?[\w-]*(?=[^,{;]*[,{]))|(([@#.!])?[\w-?]+%?|[@#!.])", "g")),
                ..LanguageConfiguration::default()
            },
            None,
        ));

        let this_model = instantiate_text_model_with_languages(&services, ".\u{1F437}-a-b", MODE_ID);
        guard.add(dispose_with(&this_model));

        assert_eq!(this_model.get_word_at_position(Position::new(1, 1)), word(".", 1, 2));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 2)), word(".", 1, 2));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 3)), None);
        assert_eq!(this_model.get_word_at_position(Position::new(1, 4)), word("-a-b", 4, 8));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 5)), word("-a-b", 4, 8));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 6)), word("-a-b", 4, 8));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 7)), word("-a-b", 4, 8));
        assert_eq!(this_model.get_word_at_position(Position::new(1, 8)), word("-a-b", 4, 8));

        guard.store().dispose();
    }
}
