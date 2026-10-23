//! Porte de `src/vs/editor/test/common/model/bracketPairColorizer/getBracketPairsInRange.test.ts` (suíte `Bracket Pair Colorizer - getBracketPairsInRange`), caso a caso e na mesma ordem.
//!
//! `TokenizedDocument` e `TokenInfo` vêm de `tests/support/tokenized_document.rs`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_editor::encoded_token_attributes::standard_token_type;
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::ILanguageService;
use je_editor::languages::{
    EncodedTokenizationResult, IBackgroundTokenizationStore, IBackgroundTokenizer, ITokenizationSupport, State, TokenizationResult,
};
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_editor::text_model_bracket_pairs::BracketPairInfo;
use je_editor::tokenization_registry::tokenization_registry;
use support::test_text_model::{create_language_model_services, instantiate_text_model_with_languages};
use support::tokenized_document::{TokenInfo, TokenizedDocument};

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
}

fn create_text_model_with_colorized_bracket_pairs(store: &LeakGuard, text: &str) -> Rc<TextModel> {
    let language_id = "testLanguage";
    let services = create_language_model_services(store);
    store.add(services.language_service.register_language(LanguageExtensionPoint::new(language_id)));

    let encoded_mode1 = services.language_service.language_id_codec().encode_language_id(language_id);
    let document = TokenizedDocument::new(vec![TokenInfo::new(text, encoded_mode1, standard_token_type::OTHER, true)]);
    store.add(tokenization_registry().register(language_id, document.get_tokenization_support()));

    store.add(services.language_configuration_service.register(
        language_id,
        LanguageConfiguration {
            brackets: Some(pairs(&[("<", ">")])),
            colorized_bracket_pairs: Some(pairs(&[("{", "}"), ("[", "]"), ("(", ")")])),
            ..Default::default()
        },
        None,
    ));
    instantiate_text_model_with_languages(&services, text, language_id)
}

#[derive(Debug, PartialEq)]
struct PairJson {
    level: i64,
    range: String,
    open_range: String,
    close_range: Option<String>,
}

fn pair_json(level: i64, range: &str, open_range: &str, close_range: &str) -> PairJson {
    PairJson { level, range: range.to_owned(), open_range: open_range.to_owned(), close_range: Some(close_range.to_owned()) }
}

fn bracket_pair_to_json(pair: BracketPairInfo) -> PairJson {
    PairJson {
        level: pair.nesting_level,
        range: pair.opening_bracket_range.to_string(),
        open_range: pair.opening_bracket_range.to_string(),
        close_range: pair.closing_bracket_range.map(|r| r.to_string()),
    }
}

#[test]
fn basic_1() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let doc = AnnotatedDocument::new("{ ( [] ¹ ) [ ² { } ] () } []");
    let model = create_text_model_with_colorized_bracket_pairs(&store, &doc.text);
    model.tokenization().get_line_tokens(1);
    assert_eq!(
        model.bracket_pairs().get_bracket_pairs_in_range(doc.range(1, 2)).map(bracket_pair_to_json).to_array(),
        vec![
            pair_json(0, "[1,1 -> 1,2]", "[1,1 -> 1,2]", "[1,23 -> 1,24]"),
            pair_json(1, "[1,3 -> 1,4]", "[1,3 -> 1,4]", "[1,9 -> 1,10]"),
            pair_json(1, "[1,11 -> 1,12]", "[1,11 -> 1,12]", "[1,18 -> 1,19]"),
        ]
    );
    model.dispose();
}

#[test]
fn basic_2() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let doc = AnnotatedDocument::new("{ ( [] ¹ ²) [  { } ] () } []");
    let model = create_text_model_with_colorized_bracket_pairs(&store, &doc.text);
    assert_eq!(
        model.bracket_pairs().get_bracket_pairs_in_range(doc.range(1, 2)).map(bracket_pair_to_json).to_array(),
        vec![
            pair_json(0, "[1,1 -> 1,2]", "[1,1 -> 1,2]", "[1,23 -> 1,24]"),
            pair_json(1, "[1,3 -> 1,4]", "[1,3 -> 1,4]", "[1,9 -> 1,10]"),
        ]
    );
    model.dispose();
}

#[test]
fn basic_empty() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let doc = AnnotatedDocument::new("¹ ² { ( [] ) [  { } ] () } []");
    let model = create_text_model_with_colorized_bracket_pairs(&store, &doc.text);
    assert_eq!(model.bracket_pairs().get_bracket_pairs_in_range(doc.range(1, 2)).map(bracket_pair_to_json).to_array(), vec![]);
    model.dispose();
}

#[test]
fn unmatched_closing_brackets_after_position() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let language_id = "testLanguage";
    let services = create_language_model_services(&store);
    store.add(services.language_service.register_language(LanguageExtensionPoint::new(language_id)));
    store.add(services.language_configuration_service.register(
        language_id,
        LanguageConfiguration { brackets: Some(pairs(&[("{", "}"), ("[", "]")])), ..Default::default() },
        None,
    ));
    let model = instantiate_text_model_with_languages(&services, "} text }", language_id);

    assert_eq!(
        vec![
            model.bracket_pairs().has_unmatched_closing_bracket_after(Position::new(1, 1), "{"),
            model.bracket_pairs().has_unmatched_closing_bracket_after(Position::new(1, 2), "{"),
            model.bracket_pairs().has_unmatched_closing_bracket_after(Position::new(1, 9), "{"),
            model.bracket_pairs().has_unmatched_closing_bracket_after(Position::new(1, 1), "["),
        ],
        vec![true, true, false, false]
    );
    model.dispose();
}

/// `{ ...document.getTokenizationSupport(), createBackgroundTokenizer: () => ({ dispose, requestTokens }) }`.
struct SupportWithBackgroundTokenizer {
    inner: Rc<dyn ITokenizationSupport>,
}

struct NoopBackgroundTokenizer;

impl IDisposable for NoopBackgroundTokenizer {
    fn dispose(&self) {}
}

impl IBackgroundTokenizer for NoopBackgroundTokenizer {
    fn request_tokens(&self, _start_line_number: i32, _end_line_number_exclusive: i32) {}
}

impl ITokenizationSupport for SupportWithBackgroundTokenizer {
    fn get_initial_state(&self) -> State {
        self.inner.get_initial_state()
    }

    fn tokenize(&self, line: &JsString, has_eol: bool, state: &State) -> TokenizationResult {
        self.inner.tokenize(line, has_eol, state)
    }

    fn tokenize_encoded(&self, line: &JsString, has_eol: bool, state: &State) -> EncodedTokenizationResult {
        self.inner.tokenize_encoded(line, has_eol, state)
    }

    fn create_background_tokenizer(&self, _text_model: &Rc<TextModel>, _store: Rc<dyn IBackgroundTokenizationStore>) -> Option<Rc<dyn IBackgroundTokenizer>> {
        Some(Rc::new(NoopBackgroundTokenizer))
    }
}

#[test]
fn unmatched_closing_bracket_query_waits_for_background_tokenization() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let language_id = "testLanguage";
    let services = create_language_model_services(&store);
    store.add(services.language_service.register_language(LanguageExtensionPoint::new(language_id)));
    store.add(services.language_configuration_service.register(
        language_id,
        LanguageConfiguration { brackets: Some(pairs(&[("{", "}")])), ..Default::default() },
        None,
    ));

    let encoded_language_id = services.language_service.language_id_codec().encode_language_id(language_id);
    let document = TokenizedDocument::new(vec![
        TokenInfo::new("\"}\"", encoded_language_id, standard_token_type::STRING, true),
        TokenInfo::new("}", encoded_language_id, standard_token_type::OTHER, true),
    ]);
    store.add(tokenization_registry().register(language_id, Rc::new(SupportWithBackgroundTokenizer { inner: document.get_tokenization_support() })));
    let model = instantiate_text_model_with_languages(&services, document.get_text(), language_id);

    model.bracket_pairs().get_brackets_in_range(model.get_full_model_range(), false).to_array();
    let before_tokenization = model.bracket_pairs().has_unmatched_closing_bracket_after(Position::new(1, 1), "{");
    model.tokenization().force_tokenization(model.get_line_count());
    let after_tokenization = model.bracket_pairs().has_unmatched_closing_bracket_after(Position::new(1, 1), "{");
    model.set_value(format!("x{}", document.get_text()).into());
    assert_eq!(
        vec![before_tokenization, after_tokenization, model.bracket_pairs().has_unmatched_closing_bracket_after(Position::new(1, 1), "{")],
        vec![false, true, false]
    );
    model.dispose();
}

#[test]
fn unmatched_closing_brackets_are_language_scoped() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let outer_language_id = "outerLanguage";
    let inner_language_id = "innerLanguage";
    let services = create_language_model_services(&store);
    store.add(services.language_service.register_language(LanguageExtensionPoint::new(outer_language_id)));
    store.add(services.language_service.register_language(LanguageExtensionPoint::new(inner_language_id)));
    store.add(services.language_configuration_service.register(
        outer_language_id,
        LanguageConfiguration { brackets: Some(pairs(&[("{", "}")])), ..Default::default() },
        None,
    ));
    store.add(services.language_configuration_service.register(
        inner_language_id,
        LanguageConfiguration { brackets: Some(pairs(&[("{", "}")])), ..Default::default() },
        None,
    ));

    let encoded_outer_language_id = services.language_service.language_id_codec().encode_language_id(outer_language_id);
    let encoded_inner_language_id = services.language_service.language_id_codec().encode_language_id(inner_language_id);
    let document = TokenizedDocument::new(vec![
        TokenInfo::new("x", encoded_outer_language_id, standard_token_type::OTHER, true),
        TokenInfo::new("}", encoded_inner_language_id, standard_token_type::OTHER, true),
    ]);
    store.add(tokenization_registry().register(outer_language_id, document.get_tokenization_support()));
    let model = instantiate_text_model_with_languages(&services, document.get_text(), outer_language_id);
    model.tokenization().force_tokenization(model.get_line_count());
    model.bracket_pairs().get_brackets_in_range(model.get_full_model_range(), false).to_array();

    assert_eq!(
        vec![
            model.bracket_pairs().has_unmatched_closing_bracket_after(Position::new(1, 1), "{"),
            model.bracket_pairs().has_unmatched_closing_bracket_after(Position::new(1, 2), "{"),
        ],
        vec![false, true]
    );
    model.dispose();
}

#[test]
fn basic_all() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let doc = AnnotatedDocument::new("¹ { ( [] ) [  { } ] () } [] ²");
    let model = create_text_model_with_colorized_bracket_pairs(&store, &doc.text);
    assert_eq!(
        model.bracket_pairs().get_bracket_pairs_in_range(doc.range(1, 2)).map(bracket_pair_to_json).to_array(),
        vec![
            pair_json(0, "[1,2 -> 1,3]", "[1,2 -> 1,3]", "[1,23 -> 1,24]"),
            pair_json(1, "[1,4 -> 1,5]", "[1,4 -> 1,5]", "[1,9 -> 1,10]"),
            pair_json(2, "[1,6 -> 1,7]", "[1,6 -> 1,7]", "[1,7 -> 1,8]"),
            pair_json(1, "[1,11 -> 1,12]", "[1,11 -> 1,12]", "[1,18 -> 1,19]"),
            pair_json(2, "[1,14 -> 1,15]", "[1,14 -> 1,15]", "[1,16 -> 1,17]"),
            pair_json(1, "[1,20 -> 1,21]", "[1,20 -> 1,21]", "[1,21 -> 1,22]"),
            pair_json(0, "[1,25 -> 1,26]", "[1,25 -> 1,26]", "[1,26 -> 1,27]"),
        ]
    );
    model.dispose();
}

/// `{ level, levelEqualBracketType, range }`.
fn level_range(level: i64, level_equal_bracket_type: i64, range: &str) -> (i64, i64, String) {
    (level, level_equal_bracket_type, range.to_owned())
}

#[test]
fn get_brackets_in_range() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let doc = AnnotatedDocument::new("¹ { [ ( [ [ (  ) ] ] ) ] } { } ²");
    let model = create_text_model_with_colorized_bracket_pairs(&store, &doc.text);
    assert_eq!(
        model
            .bracket_pairs()
            .get_brackets_in_range(doc.range(1, 2), false)
            .map(|b| (b.nesting_level, b.nesting_level_of_equal_bracket_type, b.range.to_string()))
            .to_array(),
        vec![
            level_range(0, 0, "[1,2 -> 1,3]"),
            level_range(1, 0, "[1,4 -> 1,5]"),
            level_range(2, 0, "[1,6 -> 1,7]"),
            level_range(3, 1, "[1,8 -> 1,9]"),
            level_range(4, 2, "[1,10 -> 1,11]"),
            level_range(5, 1, "[1,12 -> 1,13]"),
            level_range(5, 1, "[1,15 -> 1,16]"),
            level_range(4, 2, "[1,17 -> 1,18]"),
            level_range(3, 1, "[1,19 -> 1,20]"),
            level_range(2, 0, "[1,21 -> 1,22]"),
            level_range(1, 0, "[1,23 -> 1,24]"),
            level_range(0, 0, "[1,25 -> 1,26]"),
            level_range(0, 0, "[1,27 -> 1,28]"),
            level_range(0, 0, "[1,29 -> 1,30]"),
        ]
    );
    model.dispose();
}

#[test]
fn test_error_brackets() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let doc = AnnotatedDocument::new("¹ { () ] ² ");
    let model = create_text_model_with_colorized_bracket_pairs(&store, &doc.text);
    assert_eq!(
        model
            .bracket_pairs()
            .get_brackets_in_range(doc.range(1, 2), false)
            .map(|b| (b.nesting_level, b.range.to_string(), b.is_invalid))
            .to_array(),
        vec![
            (0, "[1,2 -> 1,3]".to_owned(), true),
            (1, "[1,4 -> 1,5]".to_owned(), false),
            (1, "[1,5 -> 1,6]".to_owned(), false),
            (0, "[1,7 -> 1,8]".to_owned(), true),
        ]
    );
    model.dispose();
}

#[test]
fn colorized_brackets_vs_brackets() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();

    let doc = AnnotatedDocument::new("¹ {} [<()>] <{>} ²");
    let model = create_text_model_with_colorized_bracket_pairs(&store, &doc.text);
    assert_eq!(
        model
            .bracket_pairs()
            .get_brackets_in_range(doc.range(1, 2), true)
            .map(|b| (b.nesting_level, b.nesting_level_of_equal_bracket_type, b.range.to_string()))
            .to_array(),
        vec![
            level_range(0, 0, "[1,2 -> 1,3]"),
            level_range(0, 0, "[1,3 -> 1,4]"),
            level_range(0, 0, "[1,5 -> 1,6]"),
            level_range(1, 0, "[1,7 -> 1,8]"),
            level_range(1, 0, "[1,8 -> 1,9]"),
            level_range(0, 0, "[1,10 -> 1,11]"),
            level_range(0, 0, "[1,13 -> 1,14]"),
            level_range(-1, 0, "[1,15 -> 1,16]"),
        ]
    );

    assert_eq!(
        model
            .bracket_pairs()
            .get_brackets_in_range(doc.range(1, 2), false)
            .map(|b| (b.nesting_level, b.nesting_level_of_equal_bracket_type, b.range.to_string()))
            .to_array(),
        vec![
            level_range(0, 0, "[1,2 -> 1,3]"),
            level_range(0, 0, "[1,3 -> 1,4]"),
            level_range(0, 0, "[1,5 -> 1,6]"),
            level_range(1, 0, "[1,6 -> 1,7]"),
            level_range(2, 0, "[1,7 -> 1,8]"),
            level_range(2, 0, "[1,8 -> 1,9]"),
            level_range(1, 0, "[1,9 -> 1,10]"),
            level_range(0, 0, "[1,10 -> 1,11]"),
            level_range(0, 0, "[1,12 -> 1,13]"),
            level_range(1, 0, "[1,13 -> 1,14]"),
            level_range(0, 0, "[1,14 -> 1,15]"),
            level_range(-1, 0, "[1,15 -> 1,16]"),
        ]
    );
    model.dispose();
}

/// `PositionOffsetTransformer`: `getPosition` usa o primeiro `findIndex` com `lineStartOffset <= offset`, como
/// o original.
struct PositionOffsetTransformer {
    line_start_offset_by_line_idx: Vec<usize>,
}

impl PositionOffsetTransformer {
    fn new(text: &str) -> Self {
        let mut line_start_offset_by_line_idx = vec![0];
        for (i, c) in text.encode_utf16().enumerate() {
            if c == u16::from(b'\n') {
                line_start_offset_by_line_idx.push(i + 1);
            }
        }
        Self { line_start_offset_by_line_idx }
    }

    fn get_position(&self, offset: usize) -> Position {
        let line_number = self.line_start_offset_by_line_idx.iter().position(|&start| start <= offset).expect("há uma linha");
        Position::new(line_number as i32 + 1, (offset - self.line_start_offset_by_line_idx[line_number]) as i32 + 1)
    }
}

/// `AnnotatedDocument`: o texto sem as marcas `⁰` a `⁹`, e a posição de cada marca.
struct AnnotatedDocument {
    text: String,
    positions: std::collections::HashMap<usize, Position>,
}

impl AnnotatedDocument {
    fn new(src: &str) -> Self {
        let numbers = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];

        let mut text = String::new();
        let mut offset_positions: Vec<(usize, usize)> = Vec::new();

        let mut offset = 0;
        for c in src.chars() {
            if let Some(idx) = numbers.iter().position(|&n| n == c) {
                offset_positions.push((idx, offset));
            } else {
                text.push(c);
                offset += c.len_utf16();
            }
        }

        let mapper = PositionOffsetTransformer::new(&text);
        let positions = offset_positions.into_iter().map(|(idx, offset)| (idx, mapper.get_position(offset))).collect();
        Self { text, positions }
    }

    fn range(&self, start: usize, end: usize) -> Range {
        Range::from_positions(self.positions[&start], self.positions[&end])
    }
}
