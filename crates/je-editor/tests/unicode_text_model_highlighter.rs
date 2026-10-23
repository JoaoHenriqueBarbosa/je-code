//! Porte de `src/vs/editor/test/common/services/unicodeTextModelHighlighter.test.ts` (suíte
//! `UnicodeTextModelHighlighter`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `createTextModel(text)` é `create_text_model` de `tests/support/test_text_model.rs`.
//! - Os espaços entre as palavras do texto do caso são U+00A0 (espaço sem quebra), escritos como `\u{a0}`
//!   para ficarem visíveis; o original os traz literais.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_editor::unicode_text_model_highlighter::{UnicodeHighlighterOptions, UnicodeHighlightsResult, UnicodeTextModelHighlighter};
use support::test_text_model::create_text_model;

/// `t(text, options)`: o resultado com os intervalos já como texto.
fn t(text: &str, options: &UnicodeHighlighterOptions) -> (UnicodeHighlightsResult, Vec<String>) {
    let m = create_text_model(text);
    let r = UnicodeTextModelHighlighter::compute_unicode_highlights(&*m, options, None);
    m.dispose();

    let ranges = r.ranges.iter().map(ToString::to_string).collect();
    (r, ranges)
}

#[test]
fn compute_unicode_highlights_168068() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (result, ranges) = t(
        "\n\tFor\u{a0}å\u{a0}gi\u{a0}et\u{a0}eksempel\n",
        &UnicodeHighlighterOptions {
            allowed_code_points: vec![],
            allowed_locales: vec![],
            ambiguous_characters: true,
            invisible_characters: true,
            include_comments: false,
            include_strings: false,
            non_basic_ascii: false,
        },
    );

    assert_eq!(result.ambiguous_character_count, 0);
    assert!(!result.has_more);
    assert_eq!(result.invisible_character_count, 4);
    assert_eq!(result.non_basic_ascii_character_count, 0);
    assert_eq!(ranges, vec!["[2,5 -> 2,6]", "[2,7 -> 2,8]", "[2,10 -> 2,11]", "[2,13 -> 2,14]"]);
}
