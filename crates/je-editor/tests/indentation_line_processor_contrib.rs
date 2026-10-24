//! Porte de `src/vs/editor/contrib/indentation/test/browser/indentationLineProcessor.test.ts` (as suítes
//! `Indentation Context Processor - TypeScript/JavaScript` e `Processed Indent Rules Support - TypeScript/JavaScript`),
//! caso a caso e na mesma ordem. Cada `suite` do original é um prefixo do nome dos testes.
//!
//! Notas de tradução:
//! - `Language`, `registerLanguage`, `registerLanguageConfiguration`, `registerTokenizationSupport` e
//!   `StandardTokenTypeData` vêm de `indentation.test.ts`; aqui são o módulo `tests/support/indentation_test_utils.rs`,
//!   que `tests/indentation_contrib.rs` também usa (ver o topo dele).
//! - `instantiationService.get(ILanguageConfigurationService)` é o `language_configuration_service` da fixture
//!   (o serviço não tem identificador no porte).
//! - `assert.fail('indentationRulesSupport should be defined')` é um `panic!` com a mesma mensagem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[path = "support/indentation_test_utils.rs"]
mod indentation_test_utils;
mod support;

use je_editor::encoded_token_attributes::standard_token_type::{COMMENT, OTHER, REG_EX, STRING};
use je_editor::indentation_line_processor::{IndentationContextProcessor, ProcessedIndentRulesSupport};
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::line_tokens::IViewLineTokens;
use je_editor::range::Range;

use indentation_test_utils::{IndentationFixture, Language, run_test, token};

/// `languageConfigurationService.getLanguageConfiguration(languageId).indentRulesSupport` com o `assert.fail`.
fn processed_indent_rules_support<'a>(
    fixture: &'a IndentationFixture,
    model: &'a je_editor::text_model::TextModel,
    configuration: &'a je_editor::language_configuration_registry::ResolvedLanguageConfiguration,
) -> ProcessedIndentRulesSupport<'a> {
    let Some(indentation_rules_support) = configuration.indent_rules_support.as_ref() else {
        panic!("indentationRulesSupport should be defined");
    };
    ProcessedIndentRulesSupport::new(model, indentation_rules_support, &*fixture.language_configuration_service)
}

// suite('Indentation Context Processor - TypeScript/JavaScript')

#[test]
fn indentation_context_processor_typescript_javascript_brackets_inside_of_string() {
    run_test(Language::TypeScript, |f| {
        let model = f.create_text_model(&["const someVar = \"{some text}\""].join("\n"));

        f.with_test_code_editor(&model, "full", |_editor, _view_model, _instantiation_service| {
            let tokens = vec![vec![token(0, OTHER), token(16, STRING), token(28, STRING)]];
            f.register_tokenization_support(tokens);
            let indentation_context_processor = IndentationContextProcessor::new(&model, &*f.language_configuration_service);
            let processed_context = indentation_context_processor.get_processed_token_context_around_range(&Range::new(1, 23, 1, 23));
            assert_eq!(processed_context.before_range_processed_tokens.get_line_content().to_string_lossy(), "const someVar = \"some");
            assert_eq!(processed_context.after_range_processed_tokens.get_line_content().to_string_lossy(), " text\"");
            assert_eq!(processed_context.previous_line_processed_tokens.get_line_content().to_string_lossy(), "");
        });
    });
}

#[test]
fn indentation_context_processor_typescript_javascript_brackets_inside_of_comment() {
    run_test(Language::TypeScript, |f| {
        let model = f.create_text_model(
            &["const someVar2 = /*(a])*/", "const someVar = /* [()] some other t{e}xt() */ \"some text\""].join("\n"),
        );

        f.with_test_code_editor(&model, "full", |_editor, _view_model, _instantiation_service| {
            let tokens = vec![
                vec![token(0, OTHER), token(17, COMMENT)],
                vec![token(0, OTHER), token(16, COMMENT), token(46, OTHER), token(47, STRING)],
            ];
            f.register_tokenization_support(tokens);
            let indentation_context_processor = IndentationContextProcessor::new(&model, &*f.language_configuration_service);
            let processed_context = indentation_context_processor.get_processed_token_context_around_range(&Range::new(2, 29, 2, 35));
            assert_eq!(processed_context.before_range_processed_tokens.get_line_content().to_string_lossy(), "const someVar = /*  some");
            assert_eq!(processed_context.after_range_processed_tokens.get_line_content().to_string_lossy(), " text */ \"some text\"");
            assert_eq!(processed_context.previous_line_processed_tokens.get_line_content().to_string_lossy(), "const someVar2 = /*a*/");
        });
    });
}

#[test]
fn indentation_context_processor_typescript_javascript_brackets_inside_of_regex() {
    run_test(Language::TypeScript, |f| {
        let model = f.create_text_model(&["const someRegex2 = /(()))]/;", "const someRegex = /()a{h}{s}[(a}87(9a9()))]/;"].join("\n"));

        f.with_test_code_editor(&model, "full", |_editor, _view_model, _instantiation_service| {
            let tokens = vec![
                vec![token(0, OTHER), token(19, REG_EX), token(27, OTHER)],
                vec![token(0, OTHER), token(18, REG_EX), token(44, OTHER)],
            ];
            f.register_tokenization_support(tokens);
            let indentation_context_processor = IndentationContextProcessor::new(&model, &*f.language_configuration_service);
            let processed_context = indentation_context_processor.get_processed_token_context_around_range(&Range::new(1, 25, 2, 33));
            assert_eq!(processed_context.before_range_processed_tokens.get_line_content().to_string_lossy(), "const someRegex2 = /");
            assert_eq!(processed_context.after_range_processed_tokens.get_line_content().to_string_lossy(), "879a9/;");
            assert_eq!(processed_context.previous_line_processed_tokens.get_line_content().to_string_lossy(), "");
        });
    });
}

// suite('Processed Indent Rules Support - TypeScript/JavaScript')

#[test]
fn processed_indent_rules_support_typescript_javascript_should_increase() {
    run_test(Language::TypeScript, |f| {
        let model = f.create_text_model(&["const someVar = {", "const someVar2 = \"{\"", "const someVar3 = /*{*/"].join("\n"));

        f.with_test_code_editor(&model, "full", |_editor, _view_model, _instantiation_service| {
            let tokens = vec![
                vec![token(0, OTHER)],
                vec![token(0, OTHER), token(17, STRING)],
                vec![token(0, OTHER), token(17, COMMENT)],
            ];
            f.register_tokenization_support(tokens);
            let language_configuration_service: &dyn ILanguageConfigurationService = &*f.language_configuration_service;
            let configuration = language_configuration_service.get_language_configuration(f.language_id());
            let processed_indent_rules_support = processed_indent_rules_support(f, &model, &configuration);
            assert!(processed_indent_rules_support.should_increase(1, None));
            assert!(!processed_indent_rules_support.should_increase(2, None));
            assert!(!processed_indent_rules_support.should_increase(3, None));
        });
    });
}

#[test]
fn processed_indent_rules_support_typescript_javascript_should_decrease() {
    run_test(Language::TypeScript, |f| {
        let model = f.create_text_model(&["}", "\"])some text}\"", "])*/"].join("\n"));

        f.with_test_code_editor(&model, "full", |_editor, _view_model, _instantiation_service| {
            let tokens = vec![vec![token(0, OTHER)], vec![token(0, STRING)], vec![token(0, COMMENT)]];
            f.register_tokenization_support(tokens);
            let language_configuration_service: &dyn ILanguageConfigurationService = &*f.language_configuration_service;
            let configuration = language_configuration_service.get_language_configuration(f.language_id());
            let processed_indent_rules_support = processed_indent_rules_support(f, &model, &configuration);
            assert!(processed_indent_rules_support.should_decrease(1, None));
            assert!(!processed_indent_rules_support.should_decrease(2, None));
            assert!(!processed_indent_rules_support.should_decrease(3, None));
        });
    });
}

#[test]
fn processed_indent_rules_support_typescript_javascript_should_increase_next_line() {
    run_test(Language::TypeScript, |f| {
        let model = f.create_text_model(&["if()", "const someString = \"if()\"", "const someRegex = /if()/"].join("\n"));

        f.with_test_code_editor(&model, "full", |_editor, _view_model, _instantiation_service| {
            let tokens = vec![
                vec![token(0, OTHER)],
                vec![token(0, OTHER), token(19, STRING)],
                vec![token(0, OTHER), token(18, REG_EX)],
            ];
            f.register_tokenization_support(tokens);
            let language_configuration_service: &dyn ILanguageConfigurationService = &*f.language_configuration_service;
            let configuration = language_configuration_service.get_language_configuration(f.language_id());
            let processed_indent_rules_support = processed_indent_rules_support(f, &model, &configuration);
            assert!(processed_indent_rules_support.should_indent_next_line(1, None));
            assert!(!processed_indent_rules_support.should_indent_next_line(2, None));
            assert!(!processed_indent_rules_support.should_indent_next_line(3, None));
        });
    });
}
