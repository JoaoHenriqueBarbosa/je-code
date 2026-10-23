//! Porte de `src/vs/editor/test/common/modes/supports/characterPair.test.ts` (suíte `CharacterPairSupport`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `assert.deepStrictEqual(getSurroundingPairs(), [new StandardAutoClosingPairConditional(...)])`
//!   vira a comparação com `AutoClosingPair`: aqui os pares de envolvimento são só `open` e `close`
//!   (ver a divergência em `character_pair.rs`).
//! - `ensureNoDisposablesAreLeakedInTestSuite` vem do `common` do `je-base`, incluído por `#[path]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::character_pair::CharacterPairSupport;
use je_editor::encoded_token_attributes::standard_token_type;
use je_editor::language_configuration::{
    AutoClosingPair, AutoClosingPairConditional, LanguageConfiguration, StandardAutoClosingPairConditional,
};
use support::modes_test_utils::{TokenText, create_fake_scoped_line_tokens};

fn pair(open: &str, close: &str) -> Rc<StandardAutoClosingPairConditional> {
    Rc::new(StandardAutoClosingPairConditional::new(&AutoClosingPairConditional::new(open, close)))
}

fn brackets(list: &[(&str, &str)]) -> Option<Vec<(String, String)>> {
    Some(list.iter().map(|(open, close)| ((*open).to_owned(), (*close).to_owned())).collect())
}

fn test_should_auto_close(character_pair_support: &CharacterPairSupport, line: &[TokenText], column: i32) -> bool {
    let auto_closing_pair = &character_pair_support.get_auto_closing_pairs()[0];
    auto_closing_pair.should_auto_close(&create_fake_scoped_line_tokens(line), column)
}

fn brace_not_in_string_or_comment() -> CharacterPairSupport {
    CharacterPairSupport::new(&LanguageConfiguration {
        auto_closing_pairs: Some(vec![AutoClosingPairConditional::new("{", "}").not_in(&["string", "comment"])]),
        ..LanguageConfiguration::default()
    })
}

#[test]
fn only_auto_closing_pairs() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let characater_pair_support = CharacterPairSupport::new(&LanguageConfiguration {
        auto_closing_pairs: Some(vec![AutoClosingPairConditional::new("a", "b")]),
        ..LanguageConfiguration::default()
    });
    assert_eq!(characater_pair_support.get_auto_closing_pairs(), &[pair("a", "b")]);
    assert_eq!(characater_pair_support.get_surrounding_pairs(), &[AutoClosingPair::new("a", "b")]);
}

#[test]
fn only_empty_auto_closing_pairs() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let characater_pair_support = CharacterPairSupport::new(&LanguageConfiguration {
        auto_closing_pairs: Some(Vec::new()),
        ..LanguageConfiguration::default()
    });
    assert!(characater_pair_support.get_auto_closing_pairs().is_empty());
    assert!(characater_pair_support.get_surrounding_pairs().is_empty());
}

#[test]
fn only_brackets() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let characater_pair_support = CharacterPairSupport::new(&LanguageConfiguration {
        brackets: brackets(&[("a", "b")]),
        ..LanguageConfiguration::default()
    });
    assert_eq!(characater_pair_support.get_auto_closing_pairs(), &[pair("a", "b")]);
    assert_eq!(characater_pair_support.get_surrounding_pairs(), &[AutoClosingPair::new("a", "b")]);
}

#[test]
fn only_empty_brackets() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let characater_pair_support =
        CharacterPairSupport::new(&LanguageConfiguration { brackets: Some(Vec::new()), ..LanguageConfiguration::default() });
    assert!(characater_pair_support.get_auto_closing_pairs().is_empty());
    assert!(characater_pair_support.get_surrounding_pairs().is_empty());
}

#[test]
fn only_surrounding_pairs() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let characater_pair_support = CharacterPairSupport::new(&LanguageConfiguration {
        surrounding_pairs: Some(vec![AutoClosingPair::new("a", "b")]),
        ..LanguageConfiguration::default()
    });
    assert!(characater_pair_support.get_auto_closing_pairs().is_empty());
    assert_eq!(characater_pair_support.get_surrounding_pairs(), &[AutoClosingPair::new("a", "b")]);
}

#[test]
fn only_empty_surrounding_pairs() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let characater_pair_support = CharacterPairSupport::new(&LanguageConfiguration {
        surrounding_pairs: Some(Vec::new()),
        ..LanguageConfiguration::default()
    });
    assert!(characater_pair_support.get_auto_closing_pairs().is_empty());
    assert!(characater_pair_support.get_surrounding_pairs().is_empty());
}

#[test]
fn brackets_is_ignored_when_having_auto_closing_pairs() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let characater_pair_support = CharacterPairSupport::new(&LanguageConfiguration {
        auto_closing_pairs: Some(Vec::new()),
        brackets: brackets(&[("a", "b")]),
        ..LanguageConfiguration::default()
    });
    assert!(characater_pair_support.get_auto_closing_pairs().is_empty());
    assert!(characater_pair_support.get_surrounding_pairs().is_empty());
}

#[test]
fn should_auto_close_pair_in_empty_line() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sup = brace_not_in_string_or_comment();
    let token_text: Vec<TokenText> = Vec::new();
    assert!(test_should_auto_close(&sup, &token_text, 1));
}

#[test]
fn should_auto_close_pair_in_not_interesting_line_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sup = brace_not_in_string_or_comment();
    let token_text = [TokenText { text: "do", type_: standard_token_type::OTHER }];
    assert!(test_should_auto_close(&sup, &token_text, 3));
}

#[test]
fn should_auto_close_pair_in_not_interesting_line_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sup = CharacterPairSupport::new(&LanguageConfiguration {
        auto_closing_pairs: Some(vec![AutoClosingPairConditional::new("{", "}")]),
        ..LanguageConfiguration::default()
    });
    let token_text = [TokenText { text: "do", type_: standard_token_type::STRING }];
    assert!(test_should_auto_close(&sup, &token_text, 3));
}

#[test]
fn should_auto_close_pair_in_interesting_line_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sup = brace_not_in_string_or_comment();
    let token_text = [TokenText { text: "\"a\"", type_: standard_token_type::STRING }];
    assert!(!test_should_auto_close(&sup, &token_text, 1));
    assert!(!test_should_auto_close(&sup, &token_text, 2));
    assert!(!test_should_auto_close(&sup, &token_text, 3));
    assert!(!test_should_auto_close(&sup, &token_text, 4));
}

#[test]
fn should_auto_close_pair_in_interesting_line_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sup = brace_not_in_string_or_comment();
    let token_text = [
        TokenText { text: "x=", type_: standard_token_type::OTHER },
        TokenText { text: "\"a\"", type_: standard_token_type::STRING },
        TokenText { text: ";", type_: standard_token_type::OTHER },
    ];
    assert!(test_should_auto_close(&sup, &token_text, 1));
    assert!(test_should_auto_close(&sup, &token_text, 2));
    assert!(test_should_auto_close(&sup, &token_text, 3));
    assert!(!test_should_auto_close(&sup, &token_text, 4));
    assert!(!test_should_auto_close(&sup, &token_text, 5));
    assert!(!test_should_auto_close(&sup, &token_text, 6));
    assert!(test_should_auto_close(&sup, &token_text, 7));
}

#[test]
fn should_auto_close_pair_in_interesting_line_3() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let sup = brace_not_in_string_or_comment();
    let token_text = [
        TokenText { text: " ", type_: standard_token_type::OTHER },
        TokenText { text: "//a", type_: standard_token_type::COMMENT },
    ];
    assert!(test_should_auto_close(&sup, &token_text, 1));
    assert!(test_should_auto_close(&sup, &token_text, 2));
    assert!(!test_should_auto_close(&sup, &token_text, 3));
    assert!(!test_should_auto_close(&sup, &token_text, 4));
    assert!(!test_should_auto_close(&sup, &token_text, 5));
}
