//! Porte de `src/vs/editor/test/common/modes/languageConfiguration.test.ts` (suíte `StandardAutoClosingPairConditional`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `new TestLanguageConfigurationService()` vem do módulo de apoio `support::test_language_configuration_service`.
//! - `ensureNoDisposablesAreLeakedInTestSuite` vem do `common` do `je-base`, incluído por `#[path]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_editor::encoded_token_attributes::standard_token_type;
use je_editor::language_configuration::{
    AutoClosingPairConditional, CommentRule, LanguageConfiguration, LineComment, StandardAutoClosingPairConditional,
};
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use support::test_language_configuration_service::TestLanguageConfigurationService;

#[test]
fn missing_not_in() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let v = StandardAutoClosingPairConditional::new(&AutoClosingPairConditional::new("{", "}"));
    assert!(v.is_ok(standard_token_type::OTHER));
    assert!(v.is_ok(standard_token_type::COMMENT));
    assert!(v.is_ok(standard_token_type::STRING));
    assert!(v.is_ok(standard_token_type::REG_EX));
}

#[test]
fn empty_not_in() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let v = StandardAutoClosingPairConditional::new(&AutoClosingPairConditional::new("{", "}").not_in(&[]));
    assert!(v.is_ok(standard_token_type::OTHER));
    assert!(v.is_ok(standard_token_type::COMMENT));
    assert!(v.is_ok(standard_token_type::STRING));
    assert!(v.is_ok(standard_token_type::REG_EX));
}

#[test]
fn invalid_not_in() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let v = StandardAutoClosingPairConditional::new(&AutoClosingPairConditional::new("{", "}").not_in(&["bla"]));
    assert!(v.is_ok(standard_token_type::OTHER));
    assert!(v.is_ok(standard_token_type::COMMENT));
    assert!(v.is_ok(standard_token_type::STRING));
    assert!(v.is_ok(standard_token_type::REG_EX));
}

#[test]
fn not_in_in_strings() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let v = StandardAutoClosingPairConditional::new(&AutoClosingPairConditional::new("{", "}").not_in(&["string"]));
    assert!(v.is_ok(standard_token_type::OTHER));
    assert!(v.is_ok(standard_token_type::COMMENT));
    assert!(!v.is_ok(standard_token_type::STRING));
    assert!(v.is_ok(standard_token_type::REG_EX));
}

#[test]
fn not_in_in_comments() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let v = StandardAutoClosingPairConditional::new(&AutoClosingPairConditional::new("{", "}").not_in(&["comment"]));
    assert!(v.is_ok(standard_token_type::OTHER));
    assert!(!v.is_ok(standard_token_type::COMMENT));
    assert!(v.is_ok(standard_token_type::STRING));
    assert!(v.is_ok(standard_token_type::REG_EX));
}

#[test]
fn not_in_in_regex() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let v = StandardAutoClosingPairConditional::new(&AutoClosingPairConditional::new("{", "}").not_in(&["regex"]));
    assert!(v.is_ok(standard_token_type::OTHER));
    assert!(v.is_ok(standard_token_type::COMMENT));
    assert!(v.is_ok(standard_token_type::STRING));
    assert!(!v.is_ok(standard_token_type::REG_EX));
}

#[test]
fn not_in_in_strings_nor_comments() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let v = StandardAutoClosingPairConditional::new(&AutoClosingPairConditional::new("{", "}").not_in(&["string", "comment"]));
    assert!(v.is_ok(standard_token_type::OTHER));
    assert!(!v.is_ok(standard_token_type::COMMENT));
    assert!(!v.is_ok(standard_token_type::STRING));
    assert!(v.is_ok(standard_token_type::REG_EX));
}

#[test]
fn not_in_in_strings_nor_regex() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let v = StandardAutoClosingPairConditional::new(&AutoClosingPairConditional::new("{", "}").not_in(&["string", "regex"]));
    assert!(v.is_ok(standard_token_type::OTHER));
    assert!(v.is_ok(standard_token_type::COMMENT));
    assert!(!v.is_ok(standard_token_type::STRING));
    assert!(!v.is_ok(standard_token_type::REG_EX));
}

#[test]
fn not_in_in_comments_nor_regex() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let v = StandardAutoClosingPairConditional::new(&AutoClosingPairConditional::new("{", "}").not_in(&["comment", "regex"]));
    assert!(v.is_ok(standard_token_type::OTHER));
    assert!(!v.is_ok(standard_token_type::COMMENT));
    assert!(v.is_ok(standard_token_type::STRING));
    assert!(!v.is_ok(standard_token_type::REG_EX));
}

#[test]
fn not_in_in_strings_comments_nor_regex() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let v = StandardAutoClosingPairConditional::new(&AutoClosingPairConditional::new("{", "}").not_in(&["string", "comment", "regex"]));
    assert!(v.is_ok(standard_token_type::OTHER));
    assert!(!v.is_ok(standard_token_type::COMMENT));
    assert!(!v.is_ok(standard_token_type::STRING));
    assert!(!v.is_ok(standard_token_type::REG_EX));
}

#[test]
fn language_configurations_priorities() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let language_configuration_service = TestLanguageConfigurationService::new();
    let id = "testLang1";
    let line_comment = |token: &str| LanguageConfiguration {
        comments: Some(CommentRule { line_comment: Some(LineComment::Token(token.to_owned())), block_comment: None }),
        ..LanguageConfiguration::default()
    };
    let d1 = language_configuration_service.register(id, line_comment("1"), Some(100));
    let d2 = language_configuration_service.register(id, line_comment("2"), Some(10));
    assert_eq!(
        language_configuration_service
            .get_language_configuration(id)
            .comments
            .as_ref()
            .and_then(|c| c.line_comment_token.as_deref()),
        Some("1")
    );
    d1.dispose();
    d2.dispose();
    language_configuration_service.dispose();
}
