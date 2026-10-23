//! Porte de `src/vs/editor/test/common/model/tokensStore.test.ts` (suíte `TokensStore`), caso a caso e na mesma ordem.
//!
//! Os 7 primeiros casos passam os tokens esparsos ao modelo por `model.tokenization().set_semantic_tokens(...)`
//! e leem o resultado de `get_line_tokens` depois de `apply_edits`, com os auxiliares `parseTokensState`,
//! `extractState` e `testTokensAdjustment` do original.
//!
//! Os outros 10 casos usam só `SparseTokensStore`, `SparseMultilineTokens` e `LineTokens`, portados em
//! `src/sparse_tokens_store.rs` e `src/sparse_multiline_tokens.rs`.
//!
//! Notas de tradução:
//! - `Uint32Array` vira `Vec<u32>`; `IRange` e `Range` são o mesmo tipo.
//! - `assert.deepStrictEqual` vira `assert_eq!`.
//! - `ensureNoDisposablesAreLeakedInTestSuite` vem do `common` do `je-base`, incluído por `#[path]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::encoded_token_attributes::{TokenMetadata, font_style, metadata_consts as mc};
use je_editor::languages::ILanguageIdCodec;
use je_editor::languages_registry::LanguageIdCodec;
use je_editor::line_tokens::{DEFAULT_TOKEN_METADATA, IViewLineTokens, InsertToken, LineTokens};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::sparse_multiline_tokens::SparseMultilineTokens;
use je_editor::sparse_tokens_store::SparseTokensStore;
use je_editor::text_model::TextModel;
use support::test_text_model::create_text_model;

fn new_codec() -> Rc<dyn ILanguageIdCodec> {
    Rc::new(LanguageIdCodec::new())
}

/// `new LineTokens(new Uint32Array(tokens), text, codec)`.
fn line_tokens(tokens: Vec<u32>, text: &str, codec: &Rc<dyn ILanguageIdCodec>) -> LineTokens {
    LineTokens::new(tokens, JsString::from(text), codec.clone())
}

mod tokens_store {
    use super::*;

    const SEMANTIC_COLOR: u32 = 5;

    /// `parseTokensState(state)`: o texto e os tokens esparsos marcados entre `|` (os textos do teste são ASCII,
    /// então deslocamentos em bytes coincidem com as unidades UTF-16 do original).
    fn parse_tokens_state(state: &[&str]) -> (String, SparseMultilineTokens) {
        let mut text: Vec<String> = Vec::new();
        let mut tokens: Vec<u32> = Vec::new();
        let mut base_line = 1;
        for (i, line) in state.iter().enumerate() {
            let mut start_offset = 0;
            let mut line_text = String::new();
            while let Some(first_pipe_offset) = line[start_offset..].find('|').map(|p| p + start_offset) {
                let Some(second_pipe_offset) = line[first_pipe_offset + 1..].find('|').map(|p| p + first_pipe_offset + 1)
                else {
                    break;
                };
                if first_pipe_offset + 1 == second_pipe_offset {
                    // skip ||
                    line_text.push_str(&line[start_offset..=second_pipe_offset]);
                    start_offset = second_pipe_offset + 1;
                    continue;
                }

                line_text.push_str(&line[start_offset..first_pipe_offset]);
                let token_start_character = line_text.len();
                let token_length = second_pipe_offset - first_pipe_offset - 1;
                let metadata = (SEMANTIC_COLOR << mc::FOREGROUND_OFFSET) | mc::SEMANTIC_USE_FOREGROUND;

                if tokens.is_empty() {
                    base_line = i as u32 + 1;
                }
                tokens.extend([
                    i as u32 + 1 - base_line,
                    token_start_character as u32,
                    (token_start_character + token_length) as u32,
                    metadata,
                ]);

                line_text.push_str(&line[first_pipe_offset + 1..first_pipe_offset + 1 + token_length]);
                start_offset = second_pipe_offset + 1;
            }

            line_text.push_str(&line[start_offset..]);

            text.push(line_text);
        }

        (text.join("\n"), SparseMultilineTokens::create(base_line as i32, tokens))
    }

    /// `extractState(model)`: o inverso de `parse_tokens_state`.
    fn extract_state(model: &TextModel) -> Vec<String> {
        let mut result = Vec::new();
        for line_number in 1..=model.get_line_count() {
            let line_tokens = model.tokenization().get_line_tokens(line_number);
            let line_content = model.get_line_content(line_number);

            let mut line_text = String::new();
            for i in 0..line_tokens.get_count() {
                let token_start_character = line_tokens.get_start_offset(i) as usize;
                let token_end_character = line_tokens.get_end_offset(i) as usize;
                let metadata = line_tokens.get_metadata(i);
                let color = TokenMetadata::get_foreground(metadata);
                let token_text = line_content.slice(token_start_character, token_end_character).to_string_lossy();
                if color == SEMANTIC_COLOR {
                    line_text.push_str(&format!("|{token_text}|"));
                } else {
                    line_text.push_str(&token_text);
                }
            }

            result.push(line_text);
        }
        result
    }

    /// `testTokensAdjustment(rawInitialState, edits, rawFinalState)`.
    fn test_tokens_adjustment(raw_initial_state: &[&str], edits: &[(Range, &str)], raw_final_state: &[&str]) {
        let (text, tokens) = parse_tokens_state(raw_initial_state);
        let model = create_text_model(&text);
        model.tokenization().set_semantic_tokens(Some(vec![tokens]), true);

        let operations: Vec<SingleEditOperation> =
            edits.iter().map(|(range, text)| SingleEditOperation::new(*range, Some(JsString::from(*text)), false)).collect();
        model.apply_edits(&operations);

        let actual_state = extract_state(&model);
        assert_eq!(actual_state, raw_final_state);

        model.dispose();
    }

    #[test]
    fn issue_86303_color_shifting_between_different_tokens() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_tokens_adjustment(
            &["import { |URI| } from 'vs/base/common/uri';", "const foo = |URI|.parse('hey');"],
            &[(Range::new(2, 9, 2, 10), "")],
            &["import { |URI| } from 'vs/base/common/uri';", "const fo = |URI|.parse('hey');"],
        );
    }

    #[test]
    fn deleting_a_newline() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_tokens_adjustment(
            &["import { |URI| } from 'vs/base/common/uri';", "const foo = |URI|.parse('hey');"],
            &[(Range::new(1, 42, 2, 1), "")],
            &["import { |URI| } from 'vs/base/common/uri';const foo = |URI|.parse('hey');"],
        );
    }

    #[test]
    fn inserting_a_newline() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_tokens_adjustment(
            &["import { |URI| } from 'vs/base/common/uri';const foo = |URI|.parse('hey');"],
            &[(Range::new(1, 42, 1, 42), "\n")],
            &["import { |URI| } from 'vs/base/common/uri';", "const foo = |URI|.parse('hey');"],
        );
    }

    #[test]
    fn deleting_a_newline_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_tokens_adjustment(
            &["import { ", "    |URI| } from 'vs/base/common/uri';const foo = |URI|.parse('hey');"],
            &[(Range::new(1, 10, 2, 5), "")],
            &["import { |URI| } from 'vs/base/common/uri';const foo = |URI|.parse('hey');"],
        );
    }

    #[test]
    fn issue_179268_a_complex_edit() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        test_tokens_adjustment(
            &[
                "|export| |'interior_material_selector.dart'|;",
                "|export| |'mileage_selector.dart'|;",
                "|export| |'owners_selector.dart'|;",
                "|export| |'price_selector.dart'|;",
                "|export| |'seat_count_selector.dart'|;",
                "|export| |'year_selector.dart'|;",
                "|export| |'winter_options_selector.dart'|;|export| |'camera_selector.dart'|;",
            ],
            &[
                (Range::new(1, 9, 1, 9), "camera_selector.dart';\nexport '"),
                (Range::new(6, 9, 7, 9), ""),
                (Range::new(7, 39, 7, 39), "\n"),
                (Range::new(7, 47, 7, 48), "ye"),
                (Range::new(7, 49, 7, 51), ""),
                (Range::new(7, 52, 7, 53), ""),
            ],
            &[
                "|export| |'|camera_selector.dart';",
                "export 'interior_material_selector.dart';",
                "|export| |'mileage_selector.dart'|;",
                "|export| |'owners_selector.dart'|;",
                "|export| |'price_selector.dart'|;",
                "|export| |'seat_count_selector.dart'|;",
                "|export| |'||winter_options_selector.dart'|;",
                "|export| |'year_selector.dart'|;",
            ],
        );
    }

    #[test]
    fn issue_91936_semantic_token_color_highlighting_fails_on_line_with_selected_text() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let model = create_text_model("                    else if ($s = 08) then '\\b'");
        model.tokenization().set_semantic_tokens(
            Some(vec![SparseMultilineTokens::create(
                1,
                vec![
                    0, 20, 24, 0b01111000000000010000,
                    0, 25, 27, 0b01111000000000010000,
                    0, 28, 29, 0b00001000000000010000,
                    0, 29, 31, 0b10000000000000010000,
                    0, 32, 33, 0b00001000000000010000,
                    0, 34, 36, 0b00110000000000010000,
                    0, 36, 37, 0b00001000000000010000,
                    0, 38, 42, 0b01111000000000010000,
                    0, 43, 47, 0b01011000000000010000,
                ],
            )]),
            true,
        );
        let line_tokens = model.tokenization().get_line_tokens(1);
        let mut decoded_tokens: Vec<u32> = Vec::new();
        for i in 0..line_tokens.get_count() {
            decoded_tokens.push(line_tokens.get_end_offset(i) as u32);
            decoded_tokens.push(line_tokens.get_metadata(i));
        }

        assert_eq!(
            decoded_tokens,
            vec![
                20, 0b10000000001000010000000001,
                24, 0b10000001111000010000000001,
                25, 0b10000000001000010000000001,
                27, 0b10000001111000010000000001,
                28, 0b10000000001000010000000001,
                29, 0b10000000001000010000000001,
                31, 0b10000010000000010000000001,
                32, 0b10000000001000010000000001,
                33, 0b10000000001000010000000001,
                34, 0b10000000001000010000000001,
                36, 0b10000000110000010000000001,
                37, 0b10000000001000010000000001,
                38, 0b10000000001000010000000001,
                42, 0b10000001111000010000000001,
                43, 0b10000000001000010000000001,
                47, 0b10000001011000010000000001,
            ]
        );

        model.dispose();
    }

    #[test]
    fn issue_147944_language_id_vs_editor_null_language_is_not_configured_nor_known() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // O `ILanguageConfigurationService` do original é injetado; aqui o modelo cria o seu.
        let model = create_text_model("--[[\n\n]]");
        model.tokenization().set_semantic_tokens(
            Some(vec![SparseMultilineTokens::create(
                1,
                vec![
                    0, 2, 4, 0b100000000000010000,
                    1, 0, 0, 0b100000000000010000,
                    2, 0, 2, 0b100000000000010000,
                ],
            )]),
            true,
        );
        assert_eq!(model.get_word_at_position(Position::new(2, 1)), None);
        model.dispose();
    }

    #[test]
    fn partial_tokens_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let codec = new_codec();
        let mut store = SparseTokensStore::new(codec.clone());

        // setPartial: [1,1 -> 31,2], [(5,5-10),(10,5-10),(15,5-10),(20,5-10),(25,5-10),(30,5-10)]
        // setPartial: [18,1 -> 42,1], [(20,5-10),(25,5-10),(30,5-10),(35,5-10),(40,5-10)]
        // setPartial: [1,1 -> 31,2], [(5,5-10),(10,5-10),(15,5-10),(20,5-10),(25,5-10),(30,5-10)]
        store.set_partial(
            &Range::new(1, 1, 31, 2),
            vec![SparseMultilineTokens::create(
                5,
                vec![
                    0, 5, 10, 1,
                    5, 5, 10, 2,
                    10, 5, 10, 3,
                    15, 5, 10, 4,
                    20, 5, 10, 5,
                    25, 5, 10, 6,
                ],
            )],
        );

        store.set_partial(
            &Range::new(18, 1, 42, 1),
            vec![SparseMultilineTokens::create(
                20,
                vec![
                    0, 5, 10, 4,
                    5, 5, 10, 5,
                    10, 5, 10, 6,
                    15, 5, 10, 7,
                    20, 5, 10, 8,
                ],
            )],
        );

        store.set_partial(
            &Range::new(1, 1, 31, 2),
            vec![SparseMultilineTokens::create(
                5,
                vec![
                    0, 5, 10, 1,
                    5, 5, 10, 2,
                    10, 5, 10, 3,
                    15, 5, 10, 4,
                    20, 5, 10, 5,
                    25, 5, 10, 6,
                ],
            )],
        );

        let line_tokens = store.add_sparse_tokens(10, line_tokens(vec![12, 1], "enum Enum1 {", &codec));
        assert_eq!(line_tokens.get_count(), 3);
    }

    #[test]
    fn partial_tokens_2() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let codec = new_codec();
        let mut store = SparseTokensStore::new(codec.clone());

        // setPartial: [1,1 -> 31,2], [(5,5-10),(10,5-10),(15,5-10),(20,5-10),(25,5-10),(30,5-10)]
        // setPartial: [6,1 -> 36,2], [(10,5-10),(15,5-10),(20,5-10),(25,5-10),(30,5-10),(35,5-10)]
        // setPartial: [17,1 -> 42,1], [(20,5-10),(25,5-10),(30,5-10),(35,5-10),(40,5-10)]
        store.set_partial(
            &Range::new(1, 1, 31, 2),
            vec![SparseMultilineTokens::create(
                5,
                vec![
                    0, 5, 10, 1,
                    5, 5, 10, 2,
                    10, 5, 10, 3,
                    15, 5, 10, 4,
                    20, 5, 10, 5,
                    25, 5, 10, 6,
                ],
            )],
        );

        store.set_partial(
            &Range::new(6, 1, 36, 2),
            vec![SparseMultilineTokens::create(
                10,
                vec![
                    0, 5, 10, 2,
                    5, 5, 10, 3,
                    10, 5, 10, 4,
                    15, 5, 10, 5,
                    20, 5, 10, 6,
                ],
            )],
        );

        store.set_partial(
            &Range::new(17, 1, 42, 1),
            vec![SparseMultilineTokens::create(
                20,
                vec![
                    0, 5, 10, 4,
                    5, 5, 10, 5,
                    10, 5, 10, 6,
                    15, 5, 10, 7,
                    20, 5, 10, 8,
                ],
            )],
        );

        let line_tokens = store.add_sparse_tokens(20, line_tokens(vec![12, 1], "enum Enum1 {", &codec));
        assert_eq!(line_tokens.get_count(), 3);
    }

    #[test]
    fn partial_tokens_3() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let codec = new_codec();
        let mut store = SparseTokensStore::new(codec.clone());

        // setPartial: [1,1 -> 31,2], [(5,5-10),(10,5-10),(15,5-10),(20,5-10),(25,5-10),(30,5-10)]
        // setPartial: [11,1 -> 16,2], [(15,5-10),(20,5-10)]
        store.set_partial(
            &Range::new(1, 1, 31, 2),
            vec![SparseMultilineTokens::create(
                5,
                vec![
                    0, 5, 10, 1,
                    5, 5, 10, 2,
                    10, 5, 10, 3,
                    15, 5, 10, 4,
                    20, 5, 10, 5,
                    25, 5, 10, 6,
                ],
            )],
        );

        store.set_partial(
            &Range::new(11, 1, 16, 2),
            vec![SparseMultilineTokens::create(
                10,
                vec![
                    0, 5, 10, 3,
                    5, 5, 10, 4,
                ],
            )],
        );

        let line_tokens = store.add_sparse_tokens(5, line_tokens(vec![12, 1], "enum Enum1 {", &codec));
        assert_eq!(line_tokens.get_count(), 3);
    }

    #[test]
    fn issue_94133_semantic_colors_stick_around_when_using_only_range_provider() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let codec = new_codec();
        let mut store = SparseTokensStore::new(codec.clone());

        // setPartial: [1,1 -> 1,20] [(1,9-11)]
        store.set_partial(&Range::new(1, 1, 1, 20), vec![SparseMultilineTokens::create(1, vec![0, 9, 11, 1])]);

        // setPartial: [1,1 -> 1,20], []
        store.set_partial(&Range::new(1, 1, 1, 20), vec![]);

        let line_tokens = store.add_sparse_tokens(1, line_tokens(vec![12, 1], "enum Enum1 {", &codec));
        assert_eq!(line_tokens.get_count(), 1);
    }

    #[test]
    fn bug() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        fn create_tokens(str: &str) -> SparseMultilineTokens {
            let str = str.strip_prefix("[(").unwrap_or(str);
            let str = str.strip_suffix(")]").unwrap_or(str);
            let mut result: Vec<u32> = Vec::new();
            let mut first_line_number: u32 = 0;
            for str_token in str.split("),(") {
                let pieces: Vec<&str> = str_token.split(',').collect();
                let chars: Vec<&str> = pieces[1].split('-').collect();
                let line_number: u32 = pieces[0].parse().expect("número de linha");
                let start_char: u32 = chars[0].parse().expect("coluna inicial");
                let end_char: u32 = chars[1].parse().expect("coluna final");
                if first_line_number == 0 {
                    // this is the first line
                    first_line_number = line_number;
                }
                result.extend([line_number - first_line_number, start_char, end_char, (line_number + start_char) % 13]);
            }
            SparseMultilineTokens::create(first_line_number as i32, result)
        }

        let codec = new_codec();
        let mut store = SparseTokensStore::new(codec.clone());
        // setPartial [36446,1 -> 36475,115]
        store.set_partial(
            &Range::new(36446, 1, 36475, 115),
            vec![create_tokens(
                "[(36448,24-29),(36448,33-46),(36448,47-54),(36450,25-35),(36450,36-50),(36451,28-33),(36451,36-49),(36451,50-57),(36452,35-53),(36452,54-62),(36454,33-38),(36454,41-54),(36454,55-60),(36455,35-53),(36455,54-62),(36457,33-44),(36457,45-49),(36457,50-56),(36457,62-83),(36457,84-88),(36458,35-53),(36458,54-62),(36460,33-37),(36460,38-42),(36460,47-57),(36460,58-67),(36461,35-53),(36461,54-62),(36463,34-38),(36463,39-45),(36463,46-51),(36463,54-63),(36463,64-71),(36463,76-80),(36463,81-87),(36463,88-92),(36463,97-107),(36463,108-119),(36464,35-53),(36464,54-62),(36466,33-71),(36466,72-76),(36467,35-53),(36467,54-62),(36469,24-29),(36469,33-46),(36469,47-54),(36470,24-35),(36470,38-46),(36473,25-35),(36473,36-51),(36474,28-33),(36474,36-49),(36474,50-58),(36475,35-53),(36475,54-62)]",
            )],
        );
        // setPartial [36436,1 -> 36464,142]
        store.set_partial(
            &Range::new(36436, 1, 36464, 142),
            vec![create_tokens(
                "[(36437,33-37),(36437,38-42),(36437,47-57),(36437,58-67),(36438,35-53),(36438,54-62),(36440,24-29),(36440,33-46),(36440,47-53),(36442,25-35),(36442,36-50),(36443,30-39),(36443,42-46),(36443,47-53),(36443,54-58),(36443,63-73),(36443,74-84),(36443,87-91),(36443,92-98),(36443,101-105),(36443,106-112),(36443,113-119),(36444,28-37),(36444,38-42),(36444,47-57),(36444,58-75),(36444,80-95),(36444,96-105),(36445,35-53),(36445,54-62),(36448,24-29),(36448,33-46),(36448,47-54),(36450,25-35),(36450,36-50),(36451,28-33),(36451,36-49),(36451,50-57),(36452,35-53),(36452,54-62),(36454,33-38),(36454,41-54),(36454,55-60),(36455,35-53),(36455,54-62),(36457,33-44),(36457,45-49),(36457,50-56),(36457,62-83),(36457,84-88),(36458,35-53),(36458,54-62),(36460,33-37),(36460,38-42),(36460,47-57),(36460,58-67),(36461,35-53),(36461,54-62),(36463,34-38),(36463,39-45),(36463,46-51),(36463,54-63),(36463,64-71),(36463,76-80),(36463,81-87),(36463,88-92),(36463,97-107),(36463,108-119),(36464,35-53),(36464,54-62)]",
            )],
        );
        // setPartial [36457,1 -> 36485,140]
        store.set_partial(
            &Range::new(36457, 1, 36485, 140),
            vec![create_tokens(
                "[(36457,33-44),(36457,45-49),(36457,50-56),(36457,62-83),(36457,84-88),(36458,35-53),(36458,54-62),(36460,33-37),(36460,38-42),(36460,47-57),(36460,58-67),(36461,35-53),(36461,54-62),(36463,34-38),(36463,39-45),(36463,46-51),(36463,54-63),(36463,64-71),(36463,76-80),(36463,81-87),(36463,88-92),(36463,97-107),(36463,108-119),(36464,35-53),(36464,54-62),(36466,33-71),(36466,72-76),(36467,35-53),(36467,54-62),(36469,24-29),(36469,33-46),(36469,47-54),(36470,24-35),(36470,38-46),(36473,25-35),(36473,36-51),(36474,28-33),(36474,36-49),(36474,50-58),(36475,35-53),(36475,54-62),(36477,28-32),(36477,33-37),(36477,42-52),(36477,53-69),(36478,32-36),(36478,37-41),(36478,46-56),(36478,57-74),(36479,32-36),(36479,37-41),(36479,46-56),(36479,57-76),(36480,32-36),(36480,37-41),(36480,46-56),(36480,57-68),(36481,32-36),(36481,37-41),(36481,46-56),(36481,57-68),(36482,39-57),(36482,58-66),(36484,34-38),(36484,39-45),(36484,46-50),(36484,55-65),(36484,66-82),(36484,86-97),(36484,98-102),(36484,103-109),(36484,111-124),(36484,125-133),(36485,39-57),(36485,58-66)]",
            )],
        );
        // setPartial [36441,1 -> 36469,56]
        store.set_partial(
            &Range::new(36441, 1, 36469, 56),
            vec![create_tokens(
                "[(36442,25-35),(36442,36-50),(36443,30-39),(36443,42-46),(36443,47-53),(36443,54-58),(36443,63-73),(36443,74-84),(36443,87-91),(36443,92-98),(36443,101-105),(36443,106-112),(36443,113-119),(36444,28-37),(36444,38-42),(36444,47-57),(36444,58-75),(36444,80-95),(36444,96-105),(36445,35-53),(36445,54-62),(36448,24-29),(36448,33-46),(36448,47-54),(36450,25-35),(36450,36-50),(36451,28-33),(36451,36-49),(36451,50-57),(36452,35-53),(36452,54-62),(36454,33-38),(36454,41-54),(36454,55-60),(36455,35-53),(36455,54-62),(36457,33-44),(36457,45-49),(36457,50-56),(36457,62-83),(36457,84-88),(36458,35-53),(36458,54-62),(36460,33-37),(36460,38-42),(36460,47-57),(36460,58-67),(36461,35-53),(36461,54-62),(36463,34-38),(36463,39-45),(36463,46-51),(36463,54-63),(36463,64-71),(36463,76-80),(36463,81-87),(36463,88-92),(36463,97-107),(36463,108-119),(36464,35-53),(36464,54-62),(36466,33-71),(36466,72-76),(36467,35-53),(36467,54-62),(36469,24-29),(36469,33-46),(36469,47-54),(36470,24-35)]",
            )],
        );

        let line_tokens = store.add_sparse_tokens(
            36451,
            line_tokens(vec![60, 1], "                        if (flags & ModifierFlags.Ambient) {", &codec),
        );
        assert_eq!(line_tokens.get_count(), 7);
    }

    #[test]
    fn issue_95949_identifiers_are_colored_in_bold_when_targetting_keywords() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();

        fn create_tm_metadata(foreground: u32, font_style: u32, language_id: u32) -> u32 {
            (language_id << mc::LANGUAGEID_OFFSET)
                | (font_style << mc::FONT_STYLE_OFFSET)
                | (foreground << mc::FOREGROUND_OFFSET)
        }

        fn to_arr(line_tokens: &LineTokens) -> Vec<u32> {
            let mut r = Vec::new();
            for i in 0..line_tokens.get_count() {
                r.push(line_tokens.get_end_offset(i) as u32);
                r.push(line_tokens.get_metadata(i));
            }
            r
        }

        let codec = new_codec();
        let mut store = SparseTokensStore::new(codec.clone());

        store.set(
            Some(vec![SparseMultilineTokens::create(
                1,
                vec![0, 6, 11, (1 << mc::FOREGROUND_OFFSET) | mc::SEMANTIC_USE_FOREGROUND],
            )]),
            true,
            None,
        );

        let line_tokens = store.add_sparse_tokens(
            1,
            line_tokens(
                vec![
                    5,
                    create_tm_metadata(5, font_style::BOLD, 53),
                    14,
                    create_tm_metadata(1, font_style::NONE, 53),
                    17,
                    create_tm_metadata(6, font_style::NONE, 53),
                    18,
                    create_tm_metadata(1, font_style::NONE, 53),
                ],
                "const hello = 123;",
                &codec,
            ),
        );

        let actual = to_arr(&line_tokens);
        assert_eq!(
            actual,
            vec![
                5,
                create_tm_metadata(5, font_style::BOLD, 53),
                6,
                create_tm_metadata(1, font_style::NONE, 53),
                11,
                create_tm_metadata(1, font_style::NONE, 53),
                14,
                create_tm_metadata(1, font_style::NONE, 53),
                17,
                create_tm_metadata(6, font_style::NONE, 53),
                18,
                create_tm_metadata(1, font_style::NONE, 53),
            ]
        );
    }

    #[test]
    fn bug_set_partial_with_start_line_number_greater_than_1_and_token_removal_creates_invalid_state() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // The bug is the same regardless of the starting line number.
        // If a piece starts at line 5 and all tokens are removed via setPartial:
        // - startLineNumber stays at 5
        // - endLineNumber becomes 5 + (-1) = 4
        let codec = new_codec();
        let mut store = SparseTokensStore::new(codec);

        // Set initial tokens on line 5
        store.set(
            Some(vec![SparseMultilineTokens::create(
                5,
                vec![0, 5, 10, 1], // line 5, chars 5-10
            )]),
            false,
            None,
        );

        assert!(!store.is_empty());

        // Remove all tokens via setPartial
        store.set_partial(&Range::new(5, 1, 5, 20), vec![]);

        // BUG: During processing, pieces can have invalid line numbers
        // The store should remove empty pieces and remain valid
        assert!(store.is_empty(), "Store should be empty after setPartial removes all tokens");
    }

    #[test]
    fn bug_set_partial_with_split_that_creates_empty_first_piece_with_invalid_line_numbers() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let codec = new_codec();
        let mut store = SparseTokensStore::new(codec.clone());

        // Set initial tokens - token is on line 11
        store.set(
            Some(vec![SparseMultilineTokens::create(
                1,
                vec![10, 5, 10, 1], // line 11 (deltaLine=10 from startLineNumber=1), chars 5-10
            )]),
            false,
            None,
        );

        // setPartial with a range [1,1 -> 5,1] that will cause a split where the first piece is empty
        store.set_partial(&Range::new(1, 1, 5, 1), vec![]);

        assert!(!store.is_empty(), "Store should still have the token on line 11");

        // The token at line 11 should be retrievable after the split
        let line_tokens = store.add_sparse_tokens(11, line_tokens(vec![22, 1], "    test line text    ", &codec));
        assert_eq!(
            line_tokens.get_count(),
            3,
            "Should have 3 tokens: base token start + semantic token from line 11 + base token end"
        );
        assert_eq!(line_tokens.get_start_offset(1), 5, "Semantic token should start at offset 5");
        assert_eq!(line_tokens.get_end_offset(1), 10, "Semantic token should end at offset 10");
    }

    #[test]
    fn add_sparse_tokens_skips_overlapping_semantic_tokens_that_produce_backward_end_offsets() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        // This test reproduces a rendering glitch where characters are duplicated in the DOM.
        // When typing at a semantic token boundary, `acceptInsertText` can expand a token
        // and create overlapping ranges (e.g., token '+' at (3,5) and token '2' at (4,5)).
        // The merge in `addSparseTokens` must not produce backward endOffset sequences,
        // otherwise `LineTokens.withInserted` re-copies characters causing duplication.
        let codec = new_codec();
        let mut store = SparseTokensStore::new(codec.clone());

        // Simulate overlapping semantic tokens after an edit:
        // Original: f=1+2 with tokens at (0,1), (1,2), (2,3), (3,4), (4,5)
        // After inserting 'a' at offset 4: token (3,4) expands to (3,5), token (4,5) stays
        // This creates overlap: (3,5) and (4,5)
        let semantic_meta1 = (1 << mc::FOREGROUND_OFFSET) | mc::SEMANTIC_USE_FOREGROUND;
        let semantic_meta2 = (2 << mc::FOREGROUND_OFFSET) | mc::SEMANTIC_USE_FOREGROUND;
        store.set(
            Some(vec![SparseMultilineTokens::create(
                1,
                vec![
                    // deltaLine, startChar, endChar, metadata
                    0, 0, 1, semantic_meta1, // 'f' at (0,1)
                    0, 1, 2, semantic_meta2, // '=' at (1,2)
                    0, 2, 3, semantic_meta1, // '1' at (2,3)
                    0, 3, 5, semantic_meta2, // '+a' at (3,5) - expanded after edit
                    0, 4, 5, semantic_meta1, // overlapping: 'a' at (4,5) - stale position
                ],
            )]),
            true,
            None,
        );

        let tm_meta = 3 << mc::FOREGROUND_OFFSET;
        let line_tokens = store.add_sparse_tokens(
            1,
            line_tokens(
                vec![
                    6, tm_meta, // entire line "f=1+a2" covered by one TM token
                ],
                "f=1+a2",
                &codec,
            ),
        );

        // Verify endOffsets are monotonically increasing (no backward sequences)
        let end_offsets: Vec<i32> = (0..line_tokens.get_count()).map(|i| line_tokens.get_end_offset(i)).collect();
        for i in 1..end_offsets.len() {
            assert!(
                end_offsets[i] > end_offsets[i - 1],
                "endOffset[{}]={} should be > endOffset[{}]={}",
                i,
                end_offsets[i],
                i - 1,
                end_offsets[i - 1]
            );
        }

        // When used with injected text, the resulting LineTokens must not duplicate characters.
        // Simulate injected text "  " at offset 0 (like the repro's `before: { content: "  " }`)
        let with_injected = line_tokens.with_inserted(&[InsertToken {
            offset: 0,
            text: JsString::from("  "),
            token_metadata: DEFAULT_TOKEN_METADATA,
        }]);
        assert_eq!(
            with_injected.get_line_content().to_string_lossy(),
            "  f=1+a2",
            "withInserted must not duplicate characters when semantic tokens overlap"
        );
    }

    #[test]
    fn piece_with_start_line_number_0_and_end_line_number_minus_1_after_encompassing_deletion() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let codec = new_codec();
        let mut store = SparseTokensStore::new(codec);

        // Set initial tokens on lines 5-10
        let piece = SparseMultilineTokens::create(
            5,
            vec![
                0, 0, 5, 1, // line 5, chars 0-5
                5, 0, 5, 2, // line 10, chars 0-5
            ],
        );

        // O armazenamento guarda a peça por valor (o original compartilha a mesma instância), então os
        // asserts sobre a peça conferem uma cópia que recebe a mesma edição que o armazenamento.
        let mut piece_copy = piece.clone();
        store.set(Some(vec![piece]), false, None);

        // Verify initial state
        assert_eq!(piece_copy.start_line_number(), 5);
        assert_eq!(piece_copy.end_line_number(), 10);
        assert!(!piece_copy.is_empty());

        // Perform an edit that completely encompasses the token range
        // Delete from line 1 to line 20 (encompasses lines 5-10)
        // This triggers the case in _acceptDeleteRange where:
        // if (firstLineIndex < 0 && lastLineIndex >= tokenMaxDeltaLine + 1)
        // Which sets this._startLineNumber = 0 and calls this._tokens.clear()
        let range = Range::new(1, 1, 20, 1);
        store.accept_edit(
            &range, 0, // eolCount - no new lines inserted
            0, // firstLineLength
            0, // lastLineLength
            0, // firstCharCode
        );
        piece_copy.accept_edit(&range, 0, 0, 0, 0);

        // After an encompassing deletion, the piece should be empty
        assert!(piece_copy.is_empty(), "Piece should be empty after encompassing deletion");

        // EXPECTED BEHAVIOR: The store should be empty (no pieces with invalid line numbers)
        // Currently fails because the piece remains with startLineNumber=0, endLineNumber=-1
        assert!(store.is_empty(), "Store should be empty after all tokens are deleted by encompassing edit");
    }
}
