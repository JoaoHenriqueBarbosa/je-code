//! Porte de `src/vs/editor/test/node/diffing/defaultLinesDiffComputer.test.ts` (suítes `myers`, `lineRangeMapping` e
//! `LinesSliceCharSequence`), caso a caso e na mesma ordem. As suítes de fixtures do mesmo diretório
//! (`diffing.test.ts`) não fazem parte deste arquivo.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o guarda criado no começo de cada caso.
//! - `const a = true ? new MyersDiffAlgorithm() : new DynamicProgrammingDiffing()` é o `MyersDiffAlgorithm` (o ramo do
//!   `DynamicProgrammingDiffing` nunca roda); `a.compute(s1, s2)` é `compute(&s1, &s2, None)` (o prazo padrão).
//! - `Number.MAX_SAFE_INTEGER` da coluna final é `i32::MAX`, o maior valor que o `Range` representa.
//! - `new ArrayText([...])` é `ArrayText::new` (as linhas viram `JsString`), e `.toString()` do mapeamento é o `Display`.
//! - `OffsetRange.ofLength(n).map(...)` é `OffsetRange::of_length(n).map(...)`, e `translateOffset(offset)` usa o padrão
//!   `'right'` (`OffsetPreference::Right`). A lista esperada de `translateOffset` sai de laços (uma linha por laço, como as
//!   27 mudanças idênticas do `diffComputer.rs`): a linha 2 tem 16 posições, a 3 tem 12 e a 4 tem 19.
//! - A `const sequence` da suíte `LinesSliceCharSequence` é criada em cada caso.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_editor::diff::default_lines_diff_computer::algorithms::diff_algorithm::IDiffAlgorithm;
use je_editor::diff::default_lines_diff_computer::algorithms::myers_diff_algorithm::MyersDiffAlgorithm;
use je_editor::diff::default_lines_diff_computer::lines_slice_char_sequence::{LinesSliceCharSequence, OffsetPreference};
use je_editor::diff::range_mapping::{RangeMapping, get_line_range_mapping};
use je_editor::offset_range::OffsetRange;
use je_editor::range::Range;
use je_editor::text::abstract_text::ArrayText;
use je_editor::diff::default_lines_diff_computer::algorithms::diff_algorithm::ISequence;

fn lines(texts: &[&str]) -> Vec<JsString> {
    texts.iter().map(|text| JsString::from(*text)).collect()
}

// #region Suíte `myers`

#[test]
fn myers_1() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let s1 = LinesSliceCharSequence::new(&lines(&["hello world"]), Range::new(1, 1, 1, i32::MAX), true);
    let s2 = LinesSliceCharSequence::new(&lines(&["hallo welt"]), Range::new(1, 1, 1, i32::MAX), true);

    let a = MyersDiffAlgorithm;
    a.compute(&s1, &s2, None);
}

// #endregion

// #region Suíte `lineRangeMapping`

#[test]
fn line_range_mapping_simple() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        get_line_range_mapping(
            &RangeMapping::new(Range::new(2, 1, 3, 1), Range::new(2, 1, 2, 1)),
            &ArrayText::new(lines(&["const abc = \"helloworld\".split(\"\");", "", ""])),
            &ArrayText::new(lines(&["const asciiLower = \"helloworld\".split(\"\");", ""])),
        )
        .to_string(),
        "{[2,3)->[2,2)}"
    );
}

#[test]
fn line_range_mapping_empty_lines() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert_eq!(
        get_line_range_mapping(
            &RangeMapping::new(Range::new(2, 1, 2, 1), Range::new(2, 1, 4, 1)),
            &ArrayText::new(lines(&["", ""])),
            &ArrayText::new(lines(&["", "", "", ""])),
        )
        .to_string(),
        "{[2,2)->[2,4)}"
    );
}

// #endregion

// #region Suíte `LinesSliceCharSequence`

/// A `const sequence` da suíte.
fn sequence() -> LinesSliceCharSequence {
    LinesSliceCharSequence::new(
        &lines(&["line1: foo", "line2: fizzbuzz", "line3: barr", "line4: hello world", "line5: bazz"]),
        Range::new(2, 1, 5, 1),
        true,
    )
}

#[test]
fn lines_slice_char_sequence_translate_offset() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let sequence = sequence();
    let mut expected: Vec<String> = Vec::new();
    expected.extend((1..=16).map(|column| format!("(2,{column})")));
    expected.extend((1..=12).map(|column| format!("(3,{column})")));
    expected.extend((1..=19).map(|column| format!("(4,{column})")));
    assert_eq!(
        OffsetRange::of_length(sequence.length()).map(|offset| sequence.translate_offset(offset, OffsetPreference::Right).to_string()),
        expected
    );
}

#[test]
fn lines_slice_char_sequence_extend_to_full_lines() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let sequence = sequence();
    assert_eq!(sequence.get_text(sequence.extend_to_full_lines(OffsetRange::new(20, 25))), "line3: barr\n");

    assert_eq!(sequence.get_text(sequence.extend_to_full_lines(OffsetRange::new(20, 45))), "line3: barr\nline4: hello world\n");
}

// #endregion
