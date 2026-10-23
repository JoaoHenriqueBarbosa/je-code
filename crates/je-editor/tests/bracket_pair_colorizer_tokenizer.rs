//! Porte de `src/vs/editor/test/common/model/bracketPairColorizer/tokenizer.test.ts` (suíte `Bracket Pair Colorizer - Tokenizer`), caso a caso e na mesma ordem.
//!
//! `TokenizedDocument` e `TokenInfo`, exportados por este arquivo no original, ficam em
//! `tests/support/tokenized_document.rs`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use je_editor::cursor_common::CursorSimpleModel;
use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::brackets::LanguageAgnosticBracketTokens;
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::length::{Length, LENGTH_ZERO, length_add, lengths_to_range};
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::small_immutable_set::DenseKeyProvider;
use je_editor::bracket_pairs_text_model_part::bracket_pairs_tree::tokenizer::{TextBufferTokenizer, Token, TokenKind, Tokenizer};
use je_editor::encoded_token_attributes::standard_token_type;
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::ILanguageService;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::text_model::TextModel;
use je_editor::tokenization_registry::tokenization_registry;
use std::rc::Rc;
use support::test_text_model::{create_language_model_services, instantiate_text_model_with_languages};
use support::tokenized_document::{TokenInfo, TokenizedDocument};

#[test]
fn basic() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();

    let mode1 = "testMode1";
    let services = create_language_model_services(&ds);
    let language_configuration_service = services.language_configuration_service.clone();
    let language_service = services.language_service.clone();
    ds.add(language_service.register_language(LanguageExtensionPoint::new(mode1)));
    let encoded_mode1 = language_service.language_id_codec().encode_language_id(mode1);

    let dense_key_provider = Rc::new(DenseKeyProvider::<String>::new());

    let t_standard = |text: &str| TokenInfo::new(text, encoded_mode1, standard_token_type::OTHER, true);
    let t_comment = |text: &str| TokenInfo::new(text, encoded_mode1, standard_token_type::COMMENT, true);
    let document = TokenizedDocument::new(vec![
        t_standard(" { } "),
        t_standard("be"),
        t_standard("gin end"),
        t_standard("\n"),
        t_standard("hello"),
        t_comment("{"),
        t_standard("}"),
    ]);

    ds.add(tokenization_registry().register(mode1, document.get_tokenization_support()));
    ds.add(language_configuration_service.register(
        mode1,
        LanguageConfiguration {
            brackets: Some(
                [("{", "}"), ("[", "]"), ("(", ")"), ("begin", "end")].iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect(),
            ),
            ..Default::default()
        },
        None,
    ));

    let model = instantiate_text_model_with_languages(&services, document.get_text(), mode1);
    model.tokenization().force_tokenization(model.get_line_count());

    let service = language_configuration_service.clone();
    let brackets = LanguageAgnosticBracketTokens::new(dense_key_provider.clone(), Box::new(move |l| service.get_language_configuration(l)));

    let tokens = read_all_tokens(&mut TextBufferTokenizer::new(&*model, &brackets));

    let expected: Vec<(&str, Option<&str>, Vec<&str>, &str)> = vec![
        (" ", None, vec![], "Text"),
        ("{", Some("testMode1:::{"), vec!["testMode1:::{"], "OpeningBracket"),
        (" ", None, vec![], "Text"),
        ("}", Some("testMode1:::{"), vec!["testMode1:::{"], "ClosingBracket"),
        (" ", None, vec![], "Text"),
        ("begin", Some("testMode1:::begin"), vec!["testMode1:::begin"], "OpeningBracket"),
        (" ", None, vec![], "Text"),
        ("end", Some("testMode1:::begin"), vec!["testMode1:::begin"], "ClosingBracket"),
        ("\nhello{", None, vec![], "Text"),
        ("}", Some("testMode1:::{"), vec!["testMode1:::{"], "ClosingBracket"),
    ];
    let expected: Vec<(String, Option<String>, Vec<String>, String)> = expected
        .into_iter()
        .map(|(t, id, ids, k)| (t.to_owned(), id.map(str::to_owned), ids.into_iter().map(str::to_owned).collect(), k.to_owned()))
        .collect();

    assert_eq!(to_arr(&tokens, &model, &dense_key_provider), expected);

    model.dispose();
}

fn read_all_tokens(tokenizer: &mut dyn Tokenizer) -> Vec<Rc<Token>> {
    let mut tokens = Vec::new();
    while let Some(token) = tokenizer.read() {
        tokens.push(token);
    }
    tokens
}

fn to_arr(tokens: &[Rc<Token>], model: &TextModel, key_provider: &DenseKeyProvider<String>) -> Vec<(String, Option<String>, Vec<String>, String)> {
    let mut result = Vec::new();
    let mut offset = LENGTH_ZERO;
    for token in tokens {
        result.push(token_to_obj(token, offset, model, key_provider));
        offset = length_add(offset, token.length);
    }
    result
}

fn token_to_obj(token: &Token, offset: Length, model: &TextModel, key_provider: &DenseKeyProvider<String>) -> (String, Option<String>, Vec<String>, String) {
    let text: JsString = model.get_value_in_range(&lengths_to_range(offset, length_add(offset, token.length)));
    (
        text.to_string_lossy(),
        usize::try_from(token.bracket_id).ok().and_then(|id| key_provider.reverse_lookup(id)),
        key_provider.reverse_lookup_set(&token.bracket_ids),
        match token.kind {
            TokenKind::ClosingBracket => "ClosingBracket",
            TokenKind::OpeningBracket => "OpeningBracket",
            TokenKind::Text => "Text",
        }
        .to_owned(),
    )
}
