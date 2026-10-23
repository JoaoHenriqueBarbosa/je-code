//! Porte de `src/vs/editor/test/common/core/lineTokens.test.ts` (suíte `LineTokens`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ILineToken` vira a tupla `(start_index, foreground)` e `ITestViewLineToken` vira `(end_index, foreground)`.
//! - `assert.deepStrictEqual` sobre o vetor de tokens vira `assert_eq!` sobre o vetor de tuplas.
//! - `ensureNoDisposablesAreLeakedInTestSuite` vem do `common` do `je-base`, incluído por `#[path]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::encoded_token_attributes::metadata_consts as mc;
use je_editor::languages::ILanguageIdCodec;
use je_editor::languages_registry::LanguageIdCodec;
use je_editor::line_tokens::{IViewLineTokens, InsertToken, LineTokens};

mod line_tokens {
    use super::*;

    /// `ILineToken`: `(startIndex, foreground)`.
    type LineToken = (u32, u32);

    fn create_line_tokens(text: &str, tokens: &[LineToken]) -> LineTokens {
        let text = JsString::from(text);
        let mut bin_tokens = vec![0u32; tokens.len() << 1];

        for i in 0..tokens.len() {
            bin_tokens[i << 1] = if i + 1 < tokens.len() { tokens[i + 1].0 } else { text.len() as u32 };
            bin_tokens[(i << 1) + 1] = tokens[i].1 << mc::FOREGROUND_OFFSET;
        }

        let codec: Rc<dyn ILanguageIdCodec> = Rc::new(LanguageIdCodec::new());
        LineTokens::new(bin_tokens, text, codec)
    }

    fn create_test_line_tokens() -> LineTokens {
        create_line_tokens(
            "Hello world, this is a lovely day",
            &[
                (0, 1),  // Hello_
                (6, 2),  // world,_
                (13, 3), // this_
                (18, 4), // is_
                (21, 5), // a_
                (23, 6), // lovely_
                (30, 7), // day
            ],
        )
    }

    fn render_line_tokens(tokens: &LineTokens) -> String {
        let mut result = String::new();
        let str = tokens.get_line_content();
        let mut last_offset = 0;
        for i in 0..tokens.get_count() {
            let end = tokens.get_end_offset(i) as usize;
            result.push_str(&str.slice(last_offset, end).to_string_lossy());
            result.push_str(&format!("({})", tokens.get_metadata(i)));
            last_offset = end;
        }
        result
    }

    fn insert(offset: i32, text: &str) -> InsertToken {
        InsertToken { offset, text: JsString::from(text), token_metadata: 0 }
    }

    const RENDERED: &str = "Hello (32768)world, (65536)this (98304)is (131072)a (163840)lovely (196608)day(229376)";

    #[test]
    fn with_inserted_1() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let line_tokens = create_test_line_tokens();
        assert_eq!(render_line_tokens(&line_tokens), RENDERED);

        let line_tokens2 = line_tokens.with_inserted(&[insert(0, "1"), insert(6, "2"), insert(9, "3")]);

        assert_eq!(
            render_line_tokens(&line_tokens2),
            "1(0)Hello (32768)2(0)wor(65536)3(0)ld, (65536)this (98304)is (131072)a (163840)lovely (196608)day(229376)"
        );
    }

    #[test]
    fn with_inserted_tokens_at_the_same_position() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let line_tokens = create_test_line_tokens();
        assert_eq!(render_line_tokens(&line_tokens), RENDERED);

        let line_tokens2 = line_tokens.with_inserted(&[insert(0, "1"), insert(0, "2"), insert(0, "3")]);

        assert_eq!(
            render_line_tokens(&line_tokens2),
            "1(0)2(0)3(0)Hello (32768)world, (65536)this (98304)is (131072)a (163840)lovely (196608)day(229376)"
        );
    }

    #[test]
    fn with_inserted_tokens_at_the_end() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let line_tokens = create_test_line_tokens();
        assert_eq!(render_line_tokens(&line_tokens), RENDERED);

        let length = "Hello world, this is a lovely day".len() as i32;
        let line_tokens2 = line_tokens.with_inserted(&[insert(length - 1, "1"), insert(length, "2")]);

        assert_eq!(
            render_line_tokens(&line_tokens2),
            "Hello (32768)world, (65536)this (98304)is (131072)a (163840)lovely (196608)da(229376)1(0)y(229376)2(0)"
        );
    }

    #[test]
    fn basics() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let line_tokens = create_test_line_tokens();

        assert_eq!(line_tokens.get_line_content().to_string_lossy(), "Hello world, this is a lovely day");
        assert_eq!(line_tokens.get_line_content().len(), 33);
        assert_eq!(line_tokens.get_count(), 7);

        assert_eq!(line_tokens.get_start_offset(0), 0);
        assert_eq!(line_tokens.get_end_offset(0), 6);
        assert_eq!(line_tokens.get_start_offset(1), 6);
        assert_eq!(line_tokens.get_end_offset(1), 13);
        assert_eq!(line_tokens.get_start_offset(2), 13);
        assert_eq!(line_tokens.get_end_offset(2), 18);
        assert_eq!(line_tokens.get_start_offset(3), 18);
        assert_eq!(line_tokens.get_end_offset(3), 21);
        assert_eq!(line_tokens.get_start_offset(4), 21);
        assert_eq!(line_tokens.get_end_offset(4), 23);
        assert_eq!(line_tokens.get_start_offset(5), 23);
        assert_eq!(line_tokens.get_end_offset(5), 30);
        assert_eq!(line_tokens.get_start_offset(6), 30);
        assert_eq!(line_tokens.get_end_offset(6), 33);
    }

    #[test]
    fn find_token() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let line_tokens = create_test_line_tokens();

        assert_eq!(line_tokens.find_token_index_at_offset(0), 0);
        assert_eq!(line_tokens.find_token_index_at_offset(1), 0);
        assert_eq!(line_tokens.find_token_index_at_offset(2), 0);
        assert_eq!(line_tokens.find_token_index_at_offset(3), 0);
        assert_eq!(line_tokens.find_token_index_at_offset(4), 0);
        assert_eq!(line_tokens.find_token_index_at_offset(5), 0);
        assert_eq!(line_tokens.find_token_index_at_offset(6), 1);
        assert_eq!(line_tokens.find_token_index_at_offset(7), 1);
        assert_eq!(line_tokens.find_token_index_at_offset(8), 1);
        assert_eq!(line_tokens.find_token_index_at_offset(9), 1);
        assert_eq!(line_tokens.find_token_index_at_offset(10), 1);
        assert_eq!(line_tokens.find_token_index_at_offset(11), 1);
        assert_eq!(line_tokens.find_token_index_at_offset(12), 1);
        assert_eq!(line_tokens.find_token_index_at_offset(13), 2);
        assert_eq!(line_tokens.find_token_index_at_offset(14), 2);
        assert_eq!(line_tokens.find_token_index_at_offset(15), 2);
        assert_eq!(line_tokens.find_token_index_at_offset(16), 2);
        assert_eq!(line_tokens.find_token_index_at_offset(17), 2);
        assert_eq!(line_tokens.find_token_index_at_offset(18), 3);
        assert_eq!(line_tokens.find_token_index_at_offset(19), 3);
        assert_eq!(line_tokens.find_token_index_at_offset(20), 3);
        assert_eq!(line_tokens.find_token_index_at_offset(21), 4);
        assert_eq!(line_tokens.find_token_index_at_offset(22), 4);
        assert_eq!(line_tokens.find_token_index_at_offset(23), 5);
        assert_eq!(line_tokens.find_token_index_at_offset(24), 5);
        assert_eq!(line_tokens.find_token_index_at_offset(25), 5);
        assert_eq!(line_tokens.find_token_index_at_offset(26), 5);
        assert_eq!(line_tokens.find_token_index_at_offset(27), 5);
        assert_eq!(line_tokens.find_token_index_at_offset(28), 5);
        assert_eq!(line_tokens.find_token_index_at_offset(29), 5);
        assert_eq!(line_tokens.find_token_index_at_offset(30), 6);
        assert_eq!(line_tokens.find_token_index_at_offset(31), 6);
        assert_eq!(line_tokens.find_token_index_at_offset(32), 6);
        assert_eq!(line_tokens.find_token_index_at_offset(33), 6);
        assert_eq!(line_tokens.find_token_index_at_offset(34), 6);
    }

    /// `assertViewLineTokens(actual, expected)`: confere `(endIndex, foreground)` de cada token.
    fn assert_view_line_tokens(actual: &dyn IViewLineTokens, expected: &[(i32, u32)]) {
        let actual: Vec<(i32, u32)> =
            (0..actual.get_count()).map(|i| (actual.get_end_offset(i), actual.get_foreground(i))).collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn inflate() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let line_tokens = create_test_line_tokens();
        assert_view_line_tokens(&line_tokens.inflate(), &[(6, 1), (13, 2), (18, 3), (21, 4), (23, 5), (30, 6), (33, 7)]);
    }

    #[test]
    fn slice_and_inflate() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let line_tokens = create_test_line_tokens();
        assert_view_line_tokens(&line_tokens.slice_and_inflate(0, 33, 0), &[(6, 1), (13, 2), (18, 3), (21, 4), (23, 5), (30, 6), (33, 7)]);
        assert_view_line_tokens(&line_tokens.slice_and_inflate(0, 32, 0), &[(6, 1), (13, 2), (18, 3), (21, 4), (23, 5), (30, 6), (32, 7)]);
        assert_view_line_tokens(&line_tokens.slice_and_inflate(0, 30, 0), &[(6, 1), (13, 2), (18, 3), (21, 4), (23, 5), (30, 6)]);
        assert_view_line_tokens(&line_tokens.slice_and_inflate(0, 30, 1), &[(7, 1), (14, 2), (19, 3), (22, 4), (24, 5), (31, 6)]);
        assert_view_line_tokens(&line_tokens.slice_and_inflate(6, 18, 0), &[(7, 2), (12, 3)]);
        assert_view_line_tokens(&line_tokens.slice_and_inflate(7, 18, 0), &[(6, 2), (11, 3)]);
        assert_view_line_tokens(&line_tokens.slice_and_inflate(6, 17, 0), &[(7, 2), (11, 3)]);
        assert_view_line_tokens(&line_tokens.slice_and_inflate(6, 19, 0), &[(7, 2), (12, 3), (13, 4)]);
    }
}
