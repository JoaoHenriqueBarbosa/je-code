//! Porte de `src/vs/editor/test/common/model/textModelWithTokens.test.ts` (suítes `TextModelWithTokens`,
//! `TextModelWithTokens - bracket matching`, `TextModelWithTokens 2`, `TextModelWithTokens regression tests` e
//! `TextModel.getLineIndentGuide`), caso a caso e na mesma ordem.
//!
//! Cada `suite` do original vira um `mod`.
//!
//! Pendentes (27 dos 29 casos), por duas dependências ausentes no porte:
//! - `model.bracketPairs` (`BracketPairsTextModelPart`: `matchBracket`, `findPrevBracket`, `findNextBracket`):
//!   `brackets1`, `bracket matching 1` a `4`, `issue #95843`, `issue #88075`, `microsoft/monaco-editor#133` e
//!   `issue #11856`.
//! - `model.guides` (`GuidesTextModelPart`: `getLinesIndentGuides`, `getActiveIndentGuide`): os 18 casos de
//!   `TextModel.getLineIndentGuide`.
//!
//! Notas de tradução dos casos portados:
//! - O `TokenizationRegistry` é global por thread (`tokenization_registry()`); cada `#[test]` roda no próprio
//!   thread, e mesmo assim o registro é descartado no fim do caso, como no original.
//! - `NullState` vira `null_state()`; `ITokenizationSupport.tokenize: undefined!` vira um pânico, já que o
//!   modelo só chama `tokenize_encoded` aqui.
//! - `TestLineToken` vem de `tests/support/test_line_token.rs`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_editor::encoded_token_attributes::metadata_consts as mc;
use je_editor::language_configuration_registry::LanguageConfigurationService;
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::languages::{EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult};
use je_editor::line_tokens::IViewLineTokens;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::null_tokenize::null_state;
use je_editor::text_model::{TextModel, TextModelServices};
use je_editor::tokenization_registry::tokenization_registry;
use support::test_line_token::TestLineToken;
use support::test_text_model::{create_model_services, create_text_model};

/// Um suporte de tokenização com um único token por linha, cujos metadados vêm de `metadata`.
struct SingleTokenSupport<F: Fn() -> u32> {
    metadata: F,
}

impl<F: Fn() -> u32> ITokenizationSupport for SingleTokenSupport<F> {
    fn get_initial_state(&self) -> State {
        null_state()
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        panic!("tokenize não é usado: o modelo só chama tokenize_encoded");
    }

    fn tokenize_encoded(&self, _line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        EncodedTokenizationResult { tokens: vec![0, (self.metadata)()], font_info: Vec::new(), end_state: state.clone() }
    }
}

mod text_model_with_tokens {
    #[test]
    #[ignore = "Pendente: `BracketPairsTextModelPart` (`model.bracketPairs`: `matchBracket`/`findPrevBracket`/`findNextBracket`) não portado"]
    fn brackets_1() {}
}

mod text_model_with_tokens_bracket_matching {
    #[test]
    #[ignore = "Pendente: `BracketPairsTextModelPart` (`model.bracketPairs`: `matchBracket`/`findPrevBracket`/`findNextBracket`) não portado"]
    fn bracket_matching_1() {}

    #[test]
    #[ignore = "Pendente: `BracketPairsTextModelPart` (`model.bracketPairs`: `matchBracket`/`findPrevBracket`/`findNextBracket`) não portado"]
    fn bracket_matching_2() {}
}

mod text_model_with_tokens_2 {
    #[test]
    #[ignore = "Pendente: `BracketPairsTextModelPart` (`model.bracketPairs`: `matchBracket`/`findPrevBracket`/`findNextBracket`) não portado"]
    fn bracket_matching_3() {}

    #[test]
    #[ignore = "Pendente: `BracketPairsTextModelPart` (`model.bracketPairs`: `matchBracket`/`findPrevBracket`/`findNextBracket`) não portado"]
    fn bracket_matching_4() {}

    #[test]
    #[ignore = "Pendente: `BracketPairsTextModelPart` (`model.bracketPairs`: `matchBracket`/`findPrevBracket`/`findNextBracket`) não portado"]
    fn issue_95843_highlighting_of_closing_braces_is_indicating_wrong_brace_when_cursor_is_behind_opening_brace() {}

    #[test]
    #[ignore = "Pendente: `BracketPairsTextModelPart` (`model.bracketPairs`: `matchBracket`/`findPrevBracket`/`findNextBracket`) não portado"]
    fn issue_88075_type_script_brace_matching_is_incorrect_in_template_strings() {}
}

mod text_model_with_tokens_regression_tests {
    use super::*;

    #[test]
    fn microsoft_monaco_editor_122_unhandled_exception_type_error_unable_to_get_property_replace_of_undefined_or_null_reference() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        fn create_view_line_token(end_index: i32, foreground: u32) -> TestLineToken {
            let metadata = foreground << mc::FOREGROUND_OFFSET;
            TestLineToken::new(end_index, metadata)
        }

        fn assert_view_line_tokens(model: &TextModel, line_number: i32, force_tokenization: bool, expected: &[TestLineToken]) {
            if force_tokenization {
                model.tokenization().force_tokenization(line_number);
            }
            let actual_tokens = model.tokenization().get_line_tokens(line_number).inflate();
            let actual: Vec<(i32, u32)> = (0..actual_tokens.get_count())
                .map(|i| (actual_tokens.get_end_offset(i), actual_tokens.get_foreground(i)))
                .collect();
            let expected: Vec<(i32, u32)> = expected.iter().map(|token| (token.end_index, token.get_foreground())).collect();
            assert_eq!(actual, expected);
        }

        let token_id = Rc::new(std::cell::Cell::new(10u32));
        const LANG_ID1: &str = "indicisiveMode1";
        const LANG_ID2: &str = "indicisiveMode2";

        let support_id = token_id.clone();
        let tokenization_support: Rc<dyn ITokenizationSupport> = Rc::new(SingleTokenSupport {
            metadata: move || {
                support_id.set(support_id.get() + 1);
                support_id.get() << mc::FOREGROUND_OFFSET
            },
        });

        let registration1 = tokenization_registry().register(LANG_ID1, tokenization_support.clone());
        let registration2 = tokenization_registry().register(LANG_ID2, tokenization_support);

        let model = create_text_model("A model with\ntwo lines");

        assert_view_line_tokens(&model, 1, true, &[create_view_line_token(12, 1)]);
        assert_view_line_tokens(&model, 2, true, &[create_view_line_token(9, 1)]);

        model.set_language(LANG_ID1, None);

        assert_view_line_tokens(&model, 1, true, &[create_view_line_token(12, 11)]);
        assert_view_line_tokens(&model, 2, true, &[create_view_line_token(9, 12)]);

        model.set_language(LANG_ID2, None);

        assert_view_line_tokens(&model, 1, false, &[create_view_line_token(12, 1)]);
        assert_view_line_tokens(&model, 2, false, &[create_view_line_token(9, 1)]);

        model.dispose();
        registration1.dispose();
        registration2.dispose();
    }

    #[test]
    #[ignore = "Pendente: `BracketPairsTextModelPart` (`model.bracketPairs`: `matchBracket`/`findPrevBracket`/`findNextBracket`) não portado"]
    fn microsoft_monaco_editor_133_error_cannot_read_property_mode_id_of_undefined() {}

    #[test]
    #[ignore = "Pendente: `BracketPairsTextModelPart` (`model.bracketPairs`: `matchBracket`/`findPrevBracket`/`findNextBracket`) não portado"]
    fn issue_11856_bracket_matching_does_not_work_as_expected_if_the_opening_brace_symbol_is_contained_in_the_closing_brace_symbol() {}

    #[test]
    fn issue_63822_wrong_embedded_language_detected_for_empty_lines() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        let language_service = LanguageService::new(false);
        let language_configuration_service = LanguageConfigurationService::new(language_service.clone());
        let services = TextModelServices {
            language_service: language_service.clone() as Rc<dyn ILanguageService>,
            language_configuration_service: language_configuration_service.clone(),
        };

        const OUTER_MODE: &str = "outerMode";
        const INNER_MODE: &str = "innerMode";

        let outer_registration = language_service.register_language(LanguageExtensionPoint::new(OUTER_MODE));
        let inner_registration = language_service.register_language(LanguageExtensionPoint::new(INNER_MODE));

        let encoded_inner_mode = language_service.language_id_codec().encode_language_id(INNER_MODE);

        let tokenization_support: Rc<dyn ITokenizationSupport> =
            Rc::new(SingleTokenSupport { metadata: move || encoded_inner_mode << mc::LANGUAGEID_OFFSET });

        let support_registration = tokenization_registry().register(OUTER_MODE, tokenization_support);

        let model = TextModel::new_with_language_service(
            "A model with one line".into(),
            OUTER_MODE,
            TextModel::default_creation_options(),
            None,
            create_model_services(),
            Some(services),
        );

        model.tokenization().force_tokenization(1);
        assert_eq!(model.get_language_id_at_position(1, 1), INNER_MODE);

        support_registration.dispose();
        model.dispose();
        inner_registration.dispose();
        outer_registration.dispose();
        language_configuration_service.dispose();
        language_service.dispose();
    }
}

mod text_model_get_line_indent_guide {
    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn get_line_indent_guide_one_level_2() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn get_line_indent_guide_two_levels() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn get_line_indent_guide_three_levels() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn get_line_indent_guide_decreasing_indent() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn get_line_indent_guide_java() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn get_line_indent_guide_javadoc() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn get_line_indent_guide_whitespace() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn get_line_indent_guide_tabs() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn get_line_indent_guide_checker_ts() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn issue_8425_missing_indentation_lines_for_first_level_indentation() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn issue_8952_indentation_guide_lines_going_through_text_on_yml_file() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn issue_11892_indent_guides_look_funny() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn issue_12398_problem_in_indent_guidelines() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn issue_49173() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn tweaks_no_active() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn tweaks_inside_scope() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn tweaks_scope_start() {}

    #[test]
    #[ignore = "Pendente: `GuidesTextModelPart` (`model.guides`: `getLinesIndentGuides`/`getActiveIndentGuide`) não portado"]
    fn tweaks_empty_line() {}
}
