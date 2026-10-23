//! Porte de `src/vs/editor/test/common/model/bracketPairColorizer/brackets.test.ts` (suíte `Bracket Pair Colorizer - Brackets`), caso a caso e na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::brackets::LanguageAgnosticBracketTokens;
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::small_immutable_set::{DenseKeyProvider, IDENTITY_KEY_PROVIDER, SmallImmutableSet};
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::tokenizer::{Token, TokenKind};
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use support::test_language_configuration_service::TestLanguageConfigurationService;

#[derive(Debug, PartialEq)]
struct TokenObject {
    text: String,
    length: i64,
    bracket_id: i64,
    bracket_ids: SmallImmutableSetEq,
    kind: &'static str,
}

/// O `deepStrictEqual` do original compara os conjuntos por estrutura; aqui, pelo `equals`.
#[derive(Debug)]
struct SmallImmutableSetEq(SmallImmutableSet);

impl PartialEq for SmallImmutableSetEq {
    fn eq(&self, other: &Self) -> bool {
        self.0.equals(&other.0)
    }
}

fn token_to_object(token: Option<Rc<Token>>, text: &str) -> Option<TokenObject> {
    let token = token?;
    Some(TokenObject {
        text: text.to_owned(),
        length: je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::length::length_to_obj(token.length).column_count,
        bracket_id: token.bracket_id,
        bracket_ids: SmallImmutableSetEq(token.bracket_ids.clone()),
        kind: match token.kind {
            TokenKind::ClosingBracket => "ClosingBracket",
            TokenKind::OpeningBracket => "OpeningBracket",
            TokenKind::Text => "Text",
        },
    })
}

#[test]
fn basic() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let language_id = "testMode1";
    let dense_key_provider = Rc::new(DenseKeyProvider::<String>::new());
    let get_key = |value: &str| dense_key_provider.get_key(&format!("{language_id}:::{value}"));
    let get_immutable_set = |elements: &[&str]| {
        let mut new_set = SmallImmutableSet::get_empty();
        for x in elements {
            new_set = new_set.add(&get_key(x), &IDENTITY_KEY_PROVIDER);
        }
        new_set
    };

    let language_config_service = TestLanguageConfigurationService::new();
    let registration = language_config_service.register(
        language_id,
        LanguageConfiguration {
            brackets: Some(
                [
                    ("{", "}"),
                    ("[", "]"),
                    ("(", ")"),
                    ("begin", "end"),
                    ("case", "endcase"),
                    ("casez", "endcase"), // Verilog
                    ("\\left(", "\\right)"),
                    ("\\left(", "\\right."),
                    ("\\left.", "\\right)"), // LaTeX Parentheses
                    ("\\left[", "\\right]"),
                    ("\\left[", "\\right."),
                    ("\\left.", "\\right]"), // LaTeX Brackets
                ]
                .iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect(),
            ),
            ..Default::default()
        },
        None,
    );

    let service = language_config_service.clone();
    let brackets = LanguageAgnosticBracketTokens::new(dense_key_provider.clone(), Box::new(move |l| service.get_language_configuration(l)));

    // (texto, comprimento, tipo, bracketId, bracketIds)
    let expected: Vec<(&str, i64, &'static str, &str, Vec<&str>)> = vec![
        ("{", 1, "OpeningBracket", "{", vec!["{"]),
        ("[", 1, "OpeningBracket", "[", vec!["["]),
        ("(", 1, "OpeningBracket", "(", vec!["("]),
        ("begin", 5, "OpeningBracket", "begin", vec!["begin"]),
        ("case", 4, "OpeningBracket", "case", vec!["case"]),
        ("casez", 5, "OpeningBracket", "casez", vec!["casez"]),
        ("\\left(", 6, "OpeningBracket", "\\left(", vec!["\\left("]),
        ("\\left.", 6, "OpeningBracket", "\\left.", vec!["\\left."]),
        ("\\left[", 6, "OpeningBracket", "\\left[", vec!["\\left["]),
        ("}", 1, "ClosingBracket", "{", vec!["{"]),
        ("]", 1, "ClosingBracket", "[", vec!["["]),
        (")", 1, "ClosingBracket", "(", vec!["("]),
        ("end", 3, "ClosingBracket", "begin", vec!["begin"]),
        ("endcase", 7, "ClosingBracket", "case", vec!["case", "casez"]),
        ("\\right)", 7, "ClosingBracket", "\\left(", vec!["\\left(", "\\left."]),
        ("\\right.", 7, "ClosingBracket", "\\left(", vec!["\\left(", "\\left["]),
        ("\\right]", 7, "ClosingBracket", "\\left[", vec!["\\left[", "\\left."]),
    ];

    let brackets_expected: Vec<Option<TokenObject>> = expected
        .iter()
        .map(|(text, length, kind, bracket_id, bracket_ids)| {
            Some(TokenObject {
                text: (*text).to_owned(),
                length: *length,
                bracket_id: get_key(bracket_id) as i64,
                bracket_ids: SmallImmutableSetEq(get_immutable_set(bracket_ids)),
                kind,
            })
        })
        .collect();
    let brackets_actual: Vec<Option<TokenObject>> = expected
        .iter()
        .map(|(text, ..)| {
            let units: Vec<u16> = text.encode_utf16().collect();
            token_to_object(brackets.get_token(&units, language_id), text)
        })
        .collect();

    assert_eq!(brackets_actual, brackets_expected);

    registration.dispose();
    language_config_service.dispose();
}
