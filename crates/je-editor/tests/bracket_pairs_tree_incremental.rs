//! Teste próprio (sem equivalente no VS Code): a árvore de colchetes atualizada de forma incremental, depois
//! de edições aleatórias, tem de dar o mesmo resultado que uma árvore construída do zero sobre o mesmo texto.
//! Cobre o reaproveitamento de nós (`BeforeEditPositionMapper`, `NodeReader`, `concat23Trees`) com e sem tokens.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use je_editor::cursor_common::CursorSimpleModel;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::encoded_token_attributes::{metadata_consts as mc, standard_token_type};
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::ILanguageService;
use je_editor::languages::{EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult};
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::null_tokenize::null_state;
use je_editor::text_model::TextModel;
use je_editor::tokenization_registry::tokenization_registry;
use support::random::Random;
use support::test_text_model::{LanguageModelServices, create_language_model_services, instantiate_text_model_with_languages};

struct LineSupport {
    metadata: u32,
}

impl ITokenizationSupport for LineSupport {
    fn get_initial_state(&self) -> State {
        null_state()
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        panic!("tokenize não é usado");
    }

    fn tokenize_encoded(&self, _line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        EncodedTokenizationResult { tokens: vec![0, self.metadata], font_info: Vec::new(), end_state: state.clone() }
    }
}

fn services(ds: &LeakGuard, language_id: &str, with_tokens: bool) -> LanguageModelServices {
    let services = create_language_model_services(ds);
    ds.add(services.language_service.register_language(LanguageExtensionPoint::new(language_id)));
    ds.add(services.language_configuration_service.register(
        language_id,
        LanguageConfiguration {
            brackets: Some(vec![("{".into(), "}".into()), ("[".into(), "]".into()), ("(".into(), ")".into()), ("begin".into(), "end".into())]),
            ..Default::default()
        },
        None,
    ));
    if with_tokens {
        let encoded = services.language_service.language_id_codec().encode_language_id(language_id);
        let metadata = (encoded << mc::LANGUAGEID_OFFSET) | (standard_token_type::OTHER << mc::TOKEN_TYPE_OFFSET) | mc::BALANCED_BRACKETS_MASK;
        ds.add(tokenization_registry().register(language_id, Rc::new(LineSupport { metadata })));
    }
    services
}

fn all_brackets(model: &TextModel) -> Vec<(String, i64, i64, bool)> {
    model
        .bracket_pairs()
        .get_brackets_in_range(model.get_full_model_range(), false)
        .map(|b| (b.range.to_string(), b.nesting_level, b.nesting_level_of_equal_bracket_type, b.is_invalid))
        .to_array()
}

fn all_pairs(model: &TextModel) -> Vec<(String, i64, Option<String>)> {
    model
        .bracket_pairs()
        .get_bracket_pairs_in_range(model.get_full_model_range())
        .map(|p| (p.range.to_string(), p.nesting_level, p.closing_bracket_range.map(|r| r.to_string())))
        .to_array()
}

fn run(seed: u32, with_tokens: bool) {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let language_id = "incrementalMode";
    let services = services(&ds, language_id, with_tokens);
    let mut rng = Random::create(seed);

    let alphabet: Vec<&str> = vec!["{", "}", "[", "]", "(", ")", "begin", "end", " ", "x", "\n", "\n", "  ", "foo"];
    let random_text = |rng: &mut Random, len: i64| -> String {
        (0..len).map(|_| alphabet[rng.next_int_range(0, alphabet.len() as i64) as usize]).collect()
    };

    let initial = random_text(&mut rng, 400);
    let model = instantiate_text_model_with_languages(&services, initial, language_id);
    if with_tokens {
        model.tokenization().force_tokenization(model.get_line_count());
    }
    all_brackets(&model);

    for _ in 0..25 {
        let length = model.get_value_length() as i64;
        let start = rng.next_int_range(0, length + 1);
        let end = (start + rng.next_int_range(0, 12)).min(length);
        let text_len = rng_len(&mut rng);
        let text = random_text(&mut rng, text_len);
        let range = je_editor::range::Range::from_positions(model.get_position_at(start), model.get_position_at(end));
        model.apply_edits(&[SingleEditOperation::new(range, Some(JsString::from(text)), false)]);
        if with_tokens {
            model.tokenization().force_tokenization(model.get_line_count());
        }

        // Só consulta de vez em quando, para acumular edições enfileiradas.
        if rng.next_int_range(0, 3) == 0 {
            let fresh = instantiate_text_model_with_languages(&services, model.get_value().to_string_lossy(), language_id);
            if with_tokens {
                fresh.tokenization().force_tokenization(fresh.get_line_count());
            }
            assert_eq!(all_brackets(&model), all_brackets(&fresh), "colchetes (semente {seed})");
            assert_eq!(all_pairs(&model), all_pairs(&fresh), "pares (semente {seed})");
            fresh.dispose();
        }
    }

    model.dispose();
}

fn rng_len(rng: &mut Random) -> i64 {
    rng.next_int_range(0, 6)
}

#[test]
fn incremental_tree_matches_fresh_tree_without_tokens() {
    for seed in 0..30 {
        run(seed, false);
    }
}

#[test]
fn incremental_tree_matches_fresh_tree_with_tokens() {
    for seed in 0..30 {
        run(seed, true);
    }
}
