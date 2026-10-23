//! Porte de `src/vs/editor/test/common/model/model.line.test.ts` (suítes `ModelLine - getIndentLevel` e `ModelLinesTokens`), caso a caso e na mesma ordem.
//!
//! Cada `suite` do original vira um `mod`.
//!
//! Pendente: a suíte `ModelLinesTokens` inteira (52 casos). Ela registra um `ITokenizationSupport` manual no
//! `TokenizationRegistry`, empurra tokens por `ContiguousMultilineTokensBuilder` para o armazenamento de
//! tokenização em segundo plano do modelo e confere o resultado com `LineTokens` e `TestLineTokenFactory`
//! depois de cada edição. Nada disso existe no porte (o `TextModel` ainda não tem tokenização), então os
//! casos ficam com o corpo vazio e o motivo no `#[ignore]`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::model_utils::compute_indent_level;

mod model_line_get_indent_level {
    use super::*;

    /// `assertIndentLevel(text, expected, tabSize = 4)`.
    fn assert_indent_level(text: &str, expected: i32) {
        let actual = compute_indent_level(JsString::from(text).units(), 4);
        assert_eq!(actual, expected, "{text}");
    }

    #[test]
    fn get_indent_level() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_indent_level("", -1);
        assert_indent_level(" ", -1);
        assert_indent_level("   \t", -1);
        assert_indent_level("Hello", 0);
        assert_indent_level(" Hello", 1);
        assert_indent_level("   Hello", 3);
        assert_indent_level("\tHello", 4);
        assert_indent_level(" \tHello", 4);
        assert_indent_level("  \tHello", 4);
        assert_indent_level("   \tHello", 4);
        assert_indent_level("    \tHello", 8);
        assert_indent_level("     \tHello", 8);
        assert_indent_level("\t Hello", 5);
        assert_indent_level("\t \tHello", 8);
    }
}

mod model_lines_tokens {
    // Pendente: todos os casos desta suíte dependem da tokenização do modelo (`TokenizationRegistry`,
    // `ITokenizationSupport` com `createBackgroundTokenizer`, `ContiguousMultilineTokensBuilder`,
    // `LineTokens`, `TestLineTokenFactory` e `MetadataConsts.FOREGROUND_OFFSET`), que não foi portada.

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn single_delete_1() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn single_delete_2() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn single_delete_3() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn single_delete_4() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn single_delete_5() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn multi_delete_6() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn multi_delete_7() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn multi_delete_8() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn multi_delete_9() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn single_insert_1() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn single_insert_2() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn single_insert_3() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn single_insert_4() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn single_insert_5() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn multi_insert_6() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn multi_insert_7() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn multi_insert_8() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn multi_insert_9() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn insertion_on_empty_line() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn updates_tokens_on_insertion_1() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn updates_tokens_on_insertion_2() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn updates_tokens_on_insertion_3() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn updates_tokens_on_insertion_4() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn updates_tokens_on_insertion_5() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn updates_tokens_on_insertion_6() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn updates_tokens_on_insertion_7() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn updates_tokens_on_insertion_8() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn updates_tokens_on_insertion_9() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn updates_tokens_on_insertion_10() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn delete_second_token_2() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn insert_right_before_second_token() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn delete_first_char() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn delete_2nd_and_3rd_chars() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn delete_first_token() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn delete_second_token() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    /// `delete second token + a bit of the third one`
    fn delete_second_token_a_bit_of_the_third_one() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn delete_second_and_third_token() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn delete_everything() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn noop() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn equivalent_to_deleting_first_two_chars() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn equivalent_to_deleting_from_5_to_the_end() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn updates_tokens_on_replace_1() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn updates_tokens_on_replace_2() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn split_at_the_beginning() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn split_at_the_end() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn split_inthe_middle_1() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn split_inthe_middle_2() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn append_empty_1() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn append_empty_2() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn append_1() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn append_2() {}

    #[test]
    #[ignore = "Pendente: tokenização (TokenizationRegistry, LineTokens, ContiguousMultilineTokensBuilder) não portada"]
    fn append_3() {}
}
