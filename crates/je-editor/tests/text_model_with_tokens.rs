//! Porte de `src/vs/editor/test/common/model/textModelWithTokens.test.ts` (suítes `TextModelWithTokens`,
//! `TextModelWithTokens - bracket matching`, `TextModelWithTokens 2`, `TextModelWithTokens regression tests` e
//! `TextModel.getLineIndentGuide`), caso a caso e na mesma ordem.
//!
//! Cada `suite` do original vira um `mod`.
//!
//! Todos os 29 casos estão portados: os de `model.bracketPairs` (`matchBracket`, `findPrevBracket`,
//! `findNextBracket`) e os 18 de `TextModel.getLineIndentGuide` (`model.guides`).
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

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::JsString;
use je_base::lifecycle::IDisposable;
use je_editor::encoded_token_attributes::metadata_consts as mc;
use je_editor::encoded_token_attributes::standard_token_type::{OTHER, STRING};
use je_editor::language_configuration_registry::LanguageConfigurationService;
use je_editor::language_configuration::LanguageConfiguration;
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::languages::{EncodedTokenizationResult, ITokenizationSupport, State, TokenizationResult};
use je_editor::line_tokens::IViewLineTokens;
use je_editor::modes_registry::LanguageExtensionPoint;
use je_editor::null_tokenize::null_state;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model_bracket_pairs::FoundBracket;
use je_editor::text_model::{TextModel, TextModelServices};
use je_editor::tokenization_registry::tokenization_registry;
use support::test_line_token::TestLineToken;
use support::test_text_model::{
    create_language_model_services, create_model_services, create_text_model, instantiate_text_model_with_languages,
};

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

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
}

/// Um suporte de tokenização que decide os tokens (início, metadados) pelo texto da linha.
struct LineSupport<F: Fn(&str) -> Vec<u32>> {
    tokens_for_line: F,
}

impl<F: Fn(&str) -> Vec<u32>> ITokenizationSupport for LineSupport<F> {
    fn get_initial_state(&self) -> State {
        null_state()
    }

    fn tokenize(&self, _line: &JsString, _has_eol: bool, _state: &State) -> TokenizationResult {
        panic!("tokenize não é usado: o modelo só chama tokenize_encoded");
    }

    fn tokenize_encoded(&self, line: &JsString, _has_eol: bool, state: &State) -> EncodedTokenizationResult {
        EncodedTokenizationResult { tokens: (self.tokens_for_line)(&line.to_string_lossy()), font_info: Vec::new(), end_state: state.clone() }
    }
}

fn create_text_model_with_brackets(ds: &LeakGuard, text: &str, brackets: &[(&str, &str)]) -> Rc<TextModel> {
    let language_id = "bracketMode2";
    let services = create_language_model_services(ds);

    ds.add(services.language_service.register_language(LanguageExtensionPoint::new(language_id)));
    ds.add(services.language_configuration_service.register(
        language_id,
        LanguageConfiguration { brackets: Some(pairs(brackets)), ..Default::default() },
        None,
    ));

    instantiate_text_model_with_languages(&services, text, language_id)
}

fn assert_is_not_bracket(model: &TextModel, line_number: i32, column: i32) {
    let match_ = model.bracket_pairs().match_bracket(Position::new(line_number, column), None);
    assert_eq!(match_, None, "is not matching brackets at {line_number}, {column}");
}

fn assert_is_bracket(model: &TextModel, test_position: Position, expected: (Range, Range)) {
    let sort = |pair: (Range, Range)| {
        let mut ranges = [pair.0, pair.1];
        ranges.sort_by(|a, b| Range::compare_ranges_using_starts(Some(a), Some(b)).cmp(&0));
        (ranges[0], ranges[1])
    };
    let expected = sort(expected);
    let actual = model.bracket_pairs().match_bracket(test_position, None).map(sort);
    assert_eq!(actual, Some(expected), "matches brackets at {test_position}");
}

fn r(a: i32, b: i32, c: i32, d: i32) -> Range {
    Range::new(a, b, c, d)
}

/// O `setup` da suíte `TextModelWithTokens - bracket matching`: a linguagem `bracketMode1`.
fn bracket_mode1_model(ds: &LeakGuard, text: &str) -> Rc<TextModel> {
    let language_id = "bracketMode1";
    let services = create_language_model_services(ds);
    ds.add(services.language_service.register_language(LanguageExtensionPoint::new(language_id)));
    ds.add(services.language_configuration_service.register(
        language_id,
        LanguageConfiguration { brackets: Some(pairs(&[("{", "}"), ("[", "]"), ("(", ")")])), ..Default::default() },
        None,
    ));
    instantiate_text_model_with_languages(&services, text, language_id)
}

/// `toRelaxedFoundBracket`.
fn to_relaxed_found_bracket(a: Option<&FoundBracket>) -> Option<(String, je_editor::language_brackets_configuration::BracketKind)> {
    a.map(|a| (a.range.to_string(), a.bracket_info.clone()))
}

/// `testBrackets(contents, brackets)`.
fn test_brackets(ds: &LeakGuard, contents: &[&str], brackets: &[(&str, &str)]) {
    let language_id = "testMode";
    let services = create_language_model_services(ds);
    ds.add(services.language_service.register_language(LanguageExtensionPoint::new(language_id)));
    ds.add(services.language_configuration_service.register(
        language_id,
        LanguageConfiguration { brackets: Some(pairs(brackets)), ..Default::default() },
        None,
    ));

    let mut char_is_bracket: std::collections::HashSet<char> = std::collections::HashSet::new();
    for (open, close) in brackets {
        char_is_bracket.insert(open.chars().next().expect("abertura"));
        char_is_bracket.insert(close.chars().next().expect("fechamento"));
    }

    let configuration = services.language_configuration_service.get_language_configuration(language_id);
    let mut expected_brackets: Vec<FoundBracket> = Vec::new();
    for (line_index, line_text) in contents.iter().enumerate() {
        for (char_index, ch) in line_text.chars().enumerate() {
            if char_is_bracket.contains(&ch) {
                let text: Vec<u16> = ch.to_string().encode_utf16().collect();
                expected_brackets.push(FoundBracket {
                    bracket_info: configuration.brackets_new.get_bracket_info(&text).expect("é um colchete"),
                    range: Range::new(line_index as i32 + 1, char_index as i32 + 1, line_index as i32 + 1, char_index as i32 + 2),
                });
            }
        }
    }

    let model = instantiate_text_model_with_languages(&services, contents.join("\n"), language_id);

    // findPrevBracket
    {
        let mut expected_bracket_index = expected_brackets.len() as i64 - 1;
        let mut current_expected_bracket = if expected_bracket_index >= 0 { expected_brackets.get(expected_bracket_index as usize) } else { None };
        for line_number in (1..=contents.len() as i32).rev() {
            let line_text = contents[line_number as usize - 1];

            for column in (1..=line_text.len() as i32 + 1).rev() {
                if let Some(current) = current_expected_bracket
                    && line_number == current.range.start_line_number
                    && column < current.range.end_column
                {
                    expected_bracket_index -= 1;
                    current_expected_bracket = if expected_bracket_index >= 0 { expected_brackets.get(expected_bracket_index as usize) } else { None };
                }

                let actual = model.bracket_pairs().find_prev_bracket(Position::new(line_number, column));

                assert_eq!(
                    to_relaxed_found_bracket(actual.as_ref()),
                    to_relaxed_found_bracket(current_expected_bracket),
                    "findPrevBracket of {line_number}, {column}"
                );
            }
        }
    }

    // findNextBracket
    {
        let mut expected_bracket_index = 0usize;
        let mut current_expected_bracket = expected_brackets.get(expected_bracket_index);
        for line_number in 1..=contents.len() as i32 {
            let line_text = contents[line_number as usize - 1];

            for column in 1..=line_text.len() as i32 + 1 {
                if let Some(current) = current_expected_bracket
                    && line_number == current.range.start_line_number
                    && column > current.range.start_column
                {
                    expected_bracket_index += 1;
                    current_expected_bracket = expected_brackets.get(expected_bracket_index);
                }

                let actual = model.bracket_pairs().find_next_bracket(Position::new(line_number, column));

                assert_eq!(
                    to_relaxed_found_bracket(actual.as_ref()),
                    to_relaxed_found_bracket(current_expected_bracket),
                    "findNextBracket of {line_number}, {column}"
                );
            }
        }
    }

    model.dispose();
}

mod text_model_with_tokens {
    use super::*;

    #[test]
    fn brackets_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        test_brackets(&ds, &["if (a == 3) { return (7 * (a + 5)); }"], &[("{", "}"), ("[", "]"), ("(", ")")]);
    }
}

mod text_model_with_tokens_bracket_matching {
    use super::*;

    #[test]
    fn bracket_matching_1() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let model = bracket_mode1_model(&ds, ")]}{[(\n)]}{[(");

        assert_is_not_bracket(&model, 1, 1);
        assert_is_not_bracket(&model, 1, 2);
        assert_is_not_bracket(&model, 1, 3);
        assert_is_bracket(&model, Position::new(1, 4), (r(1, 4, 1, 5), r(2, 3, 2, 4)));
        assert_is_bracket(&model, Position::new(1, 5), (r(1, 5, 1, 6), r(2, 2, 2, 3)));
        assert_is_bracket(&model, Position::new(1, 6), (r(1, 6, 1, 7), r(2, 1, 2, 2)));
        assert_is_bracket(&model, Position::new(1, 7), (r(1, 6, 1, 7), r(2, 1, 2, 2)));

        assert_is_bracket(&model, Position::new(2, 1), (r(2, 1, 2, 2), r(1, 6, 1, 7)));
        assert_is_bracket(&model, Position::new(2, 2), (r(2, 2, 2, 3), r(1, 5, 1, 6)));
        assert_is_bracket(&model, Position::new(2, 3), (r(2, 3, 2, 4), r(1, 4, 1, 5)));
        assert_is_bracket(&model, Position::new(2, 4), (r(2, 3, 2, 4), r(1, 4, 1, 5)));
        assert_is_not_bracket(&model, 2, 5);
        assert_is_not_bracket(&model, 2, 6);
        assert_is_not_bracket(&model, 2, 7);

        model.dispose();
    }

    #[test]
    fn bracket_matching_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = ["var bar = {", "foo: {", "}, bar: {hallo: [{", "}, {", "}]}}"].join("\n");
        let model = bracket_mode1_model(&ds, &text);

        let brackets: Vec<(Position, Range, Range)> = vec![
            (Position::new(1, 11), r(1, 11, 1, 12), r(5, 4, 5, 5)),
            (Position::new(1, 12), r(1, 11, 1, 12), r(5, 4, 5, 5)),
            (Position::new(2, 6), r(2, 6, 2, 7), r(3, 1, 3, 2)),
            (Position::new(2, 7), r(2, 6, 2, 7), r(3, 1, 3, 2)),
            (Position::new(3, 1), r(3, 1, 3, 2), r(2, 6, 2, 7)),
            (Position::new(3, 2), r(3, 1, 3, 2), r(2, 6, 2, 7)),
            (Position::new(3, 9), r(3, 9, 3, 10), r(5, 3, 5, 4)),
            (Position::new(3, 10), r(3, 9, 3, 10), r(5, 3, 5, 4)),
            (Position::new(3, 17), r(3, 17, 3, 18), r(5, 2, 5, 3)),
            (Position::new(3, 18), r(3, 18, 3, 19), r(4, 1, 4, 2)),
            (Position::new(3, 19), r(3, 18, 3, 19), r(4, 1, 4, 2)),
            (Position::new(4, 1), r(4, 1, 4, 2), r(3, 18, 3, 19)),
            (Position::new(4, 2), r(4, 1, 4, 2), r(3, 18, 3, 19)),
            (Position::new(4, 4), r(4, 4, 4, 5), r(5, 1, 5, 2)),
            (Position::new(4, 5), r(4, 4, 4, 5), r(5, 1, 5, 2)),
            (Position::new(5, 1), r(5, 1, 5, 2), r(4, 4, 4, 5)),
            (Position::new(5, 2), r(5, 2, 5, 3), r(3, 17, 3, 18)),
            (Position::new(5, 3), r(5, 3, 5, 4), r(3, 9, 3, 10)),
            (Position::new(5, 4), r(5, 4, 5, 5), r(1, 11, 1, 12)),
            (Position::new(5, 5), r(5, 4, 5, 5), r(1, 11, 1, 12)),
        ];

        let mut is_a_bracket: std::collections::HashSet<(i32, i32)> = std::collections::HashSet::new();
        for (test_pos, b1, b2) in &brackets {
            assert_is_bracket(&model, *test_pos, (*b1, *b2));
            is_a_bracket.insert((test_pos.line_number, test_pos.column));
        }

        for i in 1..=model.get_line_count() {
            let line = model.get_line_content(i);
            for j in 1..=line.len() as i32 + 1 {
                if !is_a_bracket.contains(&(i, j)) {
                    assert_is_not_bracket(&model, i, j);
                }
            }
        }

        model.dispose();
    }
}

mod text_model_with_tokens_2 {
    use super::*;

    #[test]
    fn bracket_matching_3() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = [
            "begin",
            "    loop",
            "        if then",
            "        end if;",
            "    end loop;",
            "end;",
            "",
            "begin",
            "    loop",
            "        if then",
            "        end ifa;",
            "    end loop;",
            "end;",
        ]
        .join("\n");

        let model = create_text_model_with_brackets(&ds, &text, &[("if", "end if"), ("loop", "end loop"), ("begin", "end")]);

        // <if> ... <end ifa> is not matched
        assert_is_not_bracket(&model, 10, 9);

        // <if> ... <end if> is matched
        assert_is_bracket(&model, Position::new(3, 9), (r(3, 9, 3, 11), r(4, 9, 4, 15)));
        assert_is_bracket(&model, Position::new(4, 9), (r(4, 9, 4, 15), r(3, 9, 3, 11)));

        // <loop> ... <end loop> is matched
        assert_is_bracket(&model, Position::new(2, 5), (r(2, 5, 2, 9), r(5, 5, 5, 13)));
        assert_is_bracket(&model, Position::new(5, 5), (r(5, 5, 5, 13), r(2, 5, 2, 9)));

        // <begin> ... <end> is matched
        assert_is_bracket(&model, Position::new(1, 1), (r(1, 1, 1, 6), r(6, 1, 6, 4)));
        assert_is_bracket(&model, Position::new(6, 1), (r(6, 1, 6, 4), r(1, 1, 1, 6)));

        model.dispose();
    }

    #[test]
    fn bracket_matching_4() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let text = ["recordbegin", "  simplerecordbegin", "  endrecord", "endrecord"].join("\n");

        let model = create_text_model_with_brackets(&ds, &text, &[("recordbegin", "endrecord"), ("simplerecordbegin", "endrecord")]);

        // <recordbegin> ... <endrecord> is matched
        assert_is_bracket(&model, Position::new(1, 1), (r(1, 1, 1, 12), r(4, 1, 4, 10)));
        assert_is_bracket(&model, Position::new(4, 1), (r(4, 1, 4, 10), r(1, 1, 1, 12)));

        // <simplerecordbegin> ... <endrecord> is matched
        assert_is_bracket(&model, Position::new(2, 3), (r(2, 3, 2, 20), r(3, 3, 3, 12)));
        assert_is_bracket(&model, Position::new(3, 3), (r(3, 3, 3, 12), r(2, 3, 2, 20)));

        model.dispose();
    }

    #[test]
    fn issue_95843_highlighting_of_closing_braces_is_indicating_wrong_brace_when_cursor_is_behind_opening_brace() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let services = create_language_model_services(&ds);
        let mode1 = "testMode1";
        let mode2 = "testMode2";

        let language_id_codec = services.language_service.language_id_codec();

        ds.add(services.language_service.register_language(LanguageExtensionPoint::new(mode1)));
        ds.add(services.language_service.register_language(LanguageExtensionPoint::new(mode2)));
        let encoded_mode1 = language_id_codec.encode_language_id(mode1);
        let encoded_mode2 = language_id_codec.encode_language_id(mode2);

        let other_metadata1 = (encoded_mode1 << mc::LANGUAGEID_OFFSET) | (OTHER << mc::TOKEN_TYPE_OFFSET) | mc::BALANCED_BRACKETS_MASK;
        let other_metadata2 = (encoded_mode2 << mc::LANGUAGEID_OFFSET) | (OTHER << mc::TOKEN_TYPE_OFFSET) | mc::BALANCED_BRACKETS_MASK;

        let tokenization_support: Rc<dyn ITokenizationSupport> = Rc::new(LineSupport {
            tokens_for_line: move |line: &str| match line {
                "function f() {" => vec![
                    0, other_metadata1, 8, other_metadata1, 9, other_metadata1, 10, other_metadata1, 11, other_metadata1, 12, other_metadata1, 13,
                    other_metadata1,
                ],
                "  return <p>{true}</p>;" => vec![
                    0, other_metadata1, 2, other_metadata1, 8, other_metadata1, 9, other_metadata2, 10, other_metadata2, 11, other_metadata2, 12,
                    other_metadata2, 13, other_metadata1, 17, other_metadata2, 18, other_metadata2, 20, other_metadata2, 21, other_metadata2, 22,
                    other_metadata2,
                ],
                "}" => vec![0, other_metadata1],
                _ => panic!("Unexpected"),
            },
        });

        ds.add(tokenization_registry().register(mode1, tokenization_support));
        ds.add(services.language_configuration_service.register(
            mode1,
            LanguageConfiguration { brackets: Some(pairs(&[("{", "}"), ("[", "]"), ("(", ")")])), ..Default::default() },
            None,
        ));
        ds.add(services.language_configuration_service.register(
            mode2,
            LanguageConfiguration { brackets: Some(pairs(&[("{", "}"), ("[", "]"), ("(", ")")])), ..Default::default() },
            None,
        ));

        let model = instantiate_text_model_with_languages(
            &services,
            ["function f() {", "  return <p>{true}</p>;", "}"].join("\n"),
            mode1,
        );

        model.tokenization().force_tokenization(1);
        model.tokenization().force_tokenization(2);
        model.tokenization().force_tokenization(3);

        assert_eq!(model.bracket_pairs().match_bracket(Position::new(2, 14), None), Some((r(2, 13, 2, 14), r(2, 18, 2, 19))));

        model.dispose();
    }

    #[test]
    fn issue_88075_type_script_brace_matching_is_incorrect_in_template_strings() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let services = create_language_model_services(&ds);
        let mode = "testMode";

        ds.add(services.language_service.register_language(LanguageExtensionPoint::new(mode)));
        let encoded_mode = services.language_service.language_id_codec().encode_language_id(mode);

        let other_metadata = (encoded_mode << mc::LANGUAGEID_OFFSET) | (OTHER << mc::TOKEN_TYPE_OFFSET);
        let string_metadata = (encoded_mode << mc::LANGUAGEID_OFFSET) | (STRING << mc::TOKEN_TYPE_OFFSET);

        let tokenization_support: Rc<dyn ITokenizationSupport> = Rc::new(LineSupport {
            tokens_for_line: move |line: &str| match line {
                "function hello() {" => vec![0, other_metadata],
                "    console.log(`${100}`);" => {
                    vec![0, other_metadata, 16, string_metadata, 19, other_metadata, 22, string_metadata, 24, other_metadata]
                }
                "}" => vec![0, other_metadata],
                _ => panic!("Unexpected"),
            },
        });

        ds.add(tokenization_registry().register(mode, tokenization_support));
        ds.add(services.language_configuration_service.register(
            mode,
            LanguageConfiguration { brackets: Some(pairs(&[("{", "}"), ("[", "]"), ("(", ")")])), ..Default::default() },
            None,
        ));

        let model = instantiate_text_model_with_languages(&services, ["function hello() {", "    console.log(`${100}`);", "}"].join("\n"), mode);

        model.tokenization().force_tokenization(1);
        model.tokenization().force_tokenization(2);
        model.tokenization().force_tokenization(3);

        assert_eq!(model.bracket_pairs().match_bracket(Position::new(2, 23), None), None);
        assert_eq!(model.bracket_pairs().match_bracket(Position::new(2, 20), None), None);

        model.dispose();
    }
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
    fn microsoft_monaco_editor_133_error_cannot_read_property_mode_id_of_undefined() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model_with_brackets(
            &ds,
            &[
                "Imports System",
                "Imports System.Collections.Generic",
                "",
                "Module m1",
                "",
                "\tSub Main()",
                "\tEnd Sub",
                "",
                "End Module",
            ]
            .join("\n"),
            &[("module", "end module"), ("sub", "end sub")],
        );

        let actual = model.bracket_pairs().match_bracket(Position::new(4, 1), None);
        assert_eq!(actual, Some((r(4, 1, 4, 7), r(9, 1, 9, 11))));

        model.dispose();
    }

    #[test]
    fn issue_11856_bracket_matching_does_not_work_as_expected_if_the_opening_brace_symbol_is_contained_in_the_closing_brace_symbol() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model_with_brackets(
            &ds,
            &["sequence \"outer\"", "     sequence \"inner\"", "     endsequence", "endsequence"].join("\n"),
            &[("sequence", "endsequence"), ("feature", "endfeature")],
        );

        let actual = model.bracket_pairs().match_bracket(Position::new(3, 9), None);
        assert_eq!(actual, Some((r(2, 6, 2, 14), r(3, 6, 3, 17))));

        model.dispose();
    }

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
    use je_editor::model::IndentSize;
    use je_editor::text_model::TextModel;
    use je_editor::text_model_guides::ActiveIndentGuideInfo;

    use super::*;

    fn assert_indent_guides(lines: &[(i32, i32, i32, i32, &str)], indent_size: i32) {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let language_id = "testLang";
        let services = create_language_model_services(&ds);
        ds.add(services.language_service.register_language(LanguageExtensionPoint::new(language_id)));

        let text = lines.iter().map(|l| l.4).collect::<Vec<_>>().join("\n");
        let model = instantiate_text_model_with_languages(&services, text, language_id);
        model.update_options(je_editor::model::TextModelUpdateOptions { indent_size: Some(IndentSize::Size(indent_size)), ..Default::default() });

        let actual_indents = model.guides().get_lines_indent_guides(1, model.get_line_count());

        let mut actual: Vec<(i32, i32, i32, i32, String)> = Vec::new();
        for line in 1..=model.get_line_count() {
            let active_indent_guide = model.guides().get_active_indent_guide(line, 1, model.get_line_count());
            actual.push((
                actual_indents[line as usize - 1],
                active_indent_guide.start_line_number,
                active_indent_guide.end_line_number,
                active_indent_guide.indent,
                model.get_line_content(line).to_string_lossy(),
            ));
        }

        let expected: Vec<(i32, i32, i32, i32, String)> = lines.iter().map(|l| (l.0, l.1, l.2, l.3, l.4.to_owned())).collect();
        assert_eq!(actual, expected);

        model.dispose();
    }

    #[test]
    fn get_line_indent_guide_one_level_2() {
        assert_indent_guides(&[(0, 2, 4, 1, "A"), (1, 2, 4, 1, "  A"), (1, 2, 4, 1, "  A"), (1, 2, 4, 1, "  A")], 2);
    }

    #[test]
    fn get_line_indent_guide_two_levels() {
        assert_indent_guides(
            &[(0, 2, 5, 1, "A"), (1, 2, 5, 1, "  A"), (1, 4, 5, 2, "  A"), (2, 4, 5, 2, "    A"), (2, 4, 5, 2, "    A")],
            2,
        );
    }

    #[test]
    fn get_line_indent_guide_three_levels() {
        assert_indent_guides(
            &[(0, 2, 4, 1, "A"), (1, 3, 4, 2, "  A"), (2, 4, 4, 3, "    A"), (3, 4, 4, 3, "      A"), (0, 5, 5, 0, "A")],
            2,
        );
    }

    #[test]
    fn get_line_indent_guide_decreasing_indent() {
        assert_indent_guides(&[(2, 1, 1, 2, "    A"), (1, 1, 1, 2, "  A"), (0, 1, 2, 1, "A")], 2);
    }

    #[test]
    fn get_line_indent_guide_java() {
        assert_indent_guides(
            &[
                /* 1*/ (0, 2, 9, 1, "class A {"),
                /* 2*/ (1, 3, 4, 2, "  void foo() {"),
                /* 3*/ (2, 3, 4, 2, "    console.log(1);"),
                /* 4*/ (2, 3, 4, 2, "    console.log(2);"),
                /* 5*/ (1, 3, 4, 2, "  }"),
                /* 6*/ (1, 2, 9, 1, ""),
                /* 7*/ (1, 8, 8, 2, "  void bar() {"),
                /* 8*/ (2, 8, 8, 2, "    console.log(3);"),
                /* 9*/ (1, 8, 8, 2, "  }"),
                /*10*/ (0, 2, 9, 1, "}"),
                /*11*/ (0, 12, 12, 1, "interface B {"),
                /*12*/ (1, 12, 12, 1, "  void bar();"),
                /*13*/ (0, 12, 12, 1, "}"),
            ],
            2,
        );
    }

    #[test]
    fn get_line_indent_guide_javadoc() {
        assert_indent_guides(
            &[
                (0, 2, 3, 1, "/**"),
                (1, 2, 3, 1, " * Comment"),
                (1, 2, 3, 1, " */"),
                (0, 5, 6, 1, "class A {"),
                (1, 5, 6, 1, "  void foo() {"),
                (1, 5, 6, 1, "  }"),
                (0, 5, 6, 1, "}"),
            ],
            2,
        );
    }

    #[test]
    fn get_line_indent_guide_whitespace() {
        assert_indent_guides(
            &[
                (0, 2, 7, 1, "class A {"),
                (1, 2, 7, 1, ""),
                (1, 4, 5, 2, "  void foo() {"),
                (2, 4, 5, 2, "    "),
                (2, 4, 5, 2, "    return 1;"),
                (1, 4, 5, 2, "  }"),
                (1, 2, 7, 1, "      "),
                (0, 2, 7, 1, "}"),
            ],
            2,
        );
    }

    #[test]
    fn get_line_indent_guide_tabs() {
        assert_indent_guides(
            &[
                (0, 2, 7, 1, "class A {"),
                (1, 2, 7, 1, "\t\t"),
                (1, 4, 5, 2, "\tvoid foo() {"),
                (2, 4, 5, 2, "\t \t//hello"),
                (2, 4, 5, 2, "\t    return 2;"),
                (1, 4, 5, 2, "  \t}"),
                (1, 2, 7, 1, "      "),
                (0, 2, 7, 1, "}"),
            ],
            4,
        );
    }

    #[test]
    fn get_line_indent_guide_checker_ts() {
        assert_indent_guides(
            &[
                /* 1*/ (0, 1, 1, 0, "/// <reference path=\"binder.ts\"/>"),
                /* 2*/ (0, 2, 2, 0, ""),
                /* 3*/ (0, 3, 3, 0, "/* @internal */"),
                /* 4*/ (0, 5, 16, 1, "namespace ts {"),
                /* 5*/ (1, 5, 16, 1, "    let nextSymbolId = 1;"),
                /* 6*/ (1, 5, 16, 1, "    let nextNodeId = 1;"),
                /* 7*/ (1, 5, 16, 1, "    let nextMergeId = 1;"),
                /* 8*/ (1, 5, 16, 1, "    let nextFlowId = 1;"),
                /* 9*/ (1, 5, 16, 1, ""),
                /*10*/ (1, 11, 15, 2, "    export function getNodeId(node: Node): number {"),
                /*11*/ (2, 12, 13, 3, "        if (!node.id) {"),
                /*12*/ (3, 12, 13, 3, "            node.id = nextNodeId;"),
                /*13*/ (3, 12, 13, 3, "            nextNodeId++;"),
                /*14*/ (2, 12, 13, 3, "        }"),
                /*15*/ (2, 11, 15, 2, "        return node.id;"),
                /*16*/ (1, 11, 15, 2, "    }"),
                /*17*/ (0, 5, 16, 1, "}"),
            ],
            4,
        );
    }

    #[test]
    fn issue_8425_missing_indentation_lines_for_first_level_indentation() {
        assert_indent_guides(&[(1, 2, 3, 2, "\tindent1"), (2, 2, 3, 2, "\t\tindent2"), (2, 2, 3, 2, "\t\tindent2"), (1, 2, 3, 2, "\tindent1")], 4);
    }

    #[test]
    fn issue_8952_indentation_guide_lines_going_through_text_on_yml_file() {
        assert_indent_guides(
            &[
                (0, 2, 5, 1, "properties:"),
                (1, 3, 5, 2, "    emailAddress:"),
                (2, 3, 5, 2, "        - bla"),
                (2, 5, 5, 3, "        - length:"),
                (3, 5, 5, 3, "            max: 255"),
                (0, 6, 6, 0, "getters:"),
            ],
            4,
        );
    }

    #[test]
    fn issue_11892_indent_guides_look_funny() {
        assert_indent_guides(
            &[
                (0, 2, 7, 1, "function test(base) {"),
                (1, 3, 6, 2, "\tswitch (base) {"),
                (2, 4, 4, 3, "\t\tcase 1:"),
                (3, 4, 4, 3, "\t\t\treturn 1;"),
                (2, 6, 6, 3, "\t\tcase 2:"),
                (3, 6, 6, 3, "\t\t\treturn 2;"),
                (1, 2, 7, 1, "\t}"),
                (0, 2, 7, 1, "}"),
            ],
            4,
        );
    }

    #[test]
    fn issue_12398_problem_in_indent_guidelines() {
        assert_indent_guides(&[(2, 2, 2, 3, "\t\t.bla"), (3, 2, 2, 3, "\t\t\tlabel(for)"), (0, 3, 3, 0, "include script")], 4);
    }

    #[test]
    fn issue_49173() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model: Rc<TextModel> = create_text_model(
            &[
                "class A {",
                "\tpublic m1(): void {",
                "\t}",
                "\tpublic m2(): void {",
                "\t}",
                "\tpublic m3(): void {",
                "\t}",
                "\tpublic m4(): void {",
                "\t}",
                "\tpublic m5(): void {",
                "\t}",
                "}",
            ]
            .join("\n"),
        );

        let actual = model.guides().get_active_indent_guide(2, 4, 9);
        assert_eq!(actual, ActiveIndentGuideInfo { start_line_number: 2, end_line_number: 9, indent: 1 });
        model.dispose();
    }

    #[test]
    fn tweaks_no_active() {
        assert_indent_guides(&[(0, 1, 1, 0, "A"), (0, 2, 2, 0, "A")], 2);
    }

    #[test]
    fn tweaks_inside_scope() {
        assert_indent_guides(&[(0, 2, 2, 1, "A"), (1, 2, 2, 1, "  A")], 2);
    }

    #[test]
    fn tweaks_scope_start() {
        assert_indent_guides(&[(0, 2, 2, 1, "A"), (1, 2, 2, 1, "  A"), (0, 2, 2, 1, "A")], 2);
    }

    #[test]
    fn tweaks_empty_line() {
        assert_indent_guides(&[(0, 2, 4, 1, "A"), (1, 2, 4, 1, "  A"), (1, 2, 4, 1, ""), (1, 2, 4, 1, "  A"), (0, 2, 4, 1, "A")], 2);
    }
}
