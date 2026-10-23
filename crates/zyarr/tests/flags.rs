//! As flags `i m s u y` (e `d g v`): comportamento de cada uma sobre os mesmos padrões.
//!
//! Os valores esperados saem do Node real (V8 v26.12.1, `RegExp.prototype.exec` com a flag
//! `d` para os índices); os de erro, do Bun 1.4.2 (JavaScriptCore), cuja mensagem o zyarr reproduz.
//! Arquivo gerado a partir da execução dos casos; não editar os números à mão.

mod common;

use common::{check, Expected};

#[test]
fn i_ascii() {
    // entrada (JS): 'xABCx'
    check("abc", "i", &[120, 65, 66, 67, 120], 0, Some(Expected { captures: &[Some((1, 4))], groups: &[] }));
}

#[test]
fn i_class() {
    // entrada (JS): 'xAbCx'
    check("[a-c]+", "i", &[120, 65, 98, 67, 120], 0, Some(Expected { captures: &[Some((1, 4))], groups: &[] }));
}

#[test]
fn i_backreference() {
    // entrada (JS): 'aA'
    check("(a)\\1", "i", &[97, 65], 0, Some(Expected { captures: &[Some((0, 2)), Some((0, 1))], groups: &[] }));
}

#[test]
fn i_non_unicode_sharp_s() {
    // entrada (JS): 'SS \u00df \u1e9e'
    check("\u{df}", "i", &[83, 83, 32, 223, 32, 7838], 0, Some(Expected { captures: &[Some((3, 4))], groups: &[] }));
}

#[test]
fn i_non_unicode_kelvin() {
    // entrada (JS): '\u212a'
    check("k", "i", &[8490], 0, None);
}

#[test]
fn iu_kelvin() {
    // entrada (JS): '\u212a'
    check("k", "iu", &[8490], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn iu_long_s() {
    // entrada (JS): '\u017f'
    check("s", "iu", &[383], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn i_long_s() {
    // entrada (JS): '\u017f'
    check("s", "i", &[383], 0, None);
}

#[test]
fn i_greek_sigma() {
    // entrada (JS): '\u03a3 \u03c2'
    check("\u{3c3}", "i", &[931, 32, 962], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn iu_greek_sigma() {
    // entrada (JS): '\u03a3 \u03c2'
    check("\u{3c3}", "iu", &[931, 32, 962], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn i_dotted_capital_i() {
    // entrada (JS): 'i \u0130'
    check("\u{130}", "i", &[105, 32, 304], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn i_word_boundary() {
    // entrada (JS): '\u017f x'
    check("\\b\\w+\\b", "i", &[383, 32, 120], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn iu_word_boundary_kelvin() {
    // entrada (JS): '\u212a'
    check("\\w", "iu", &[8490], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn iu_negated_class_kelvin() {
    // entrada (JS): '\u212a'
    check("[^\\W]", "iu", &[8490], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn i_range_surrogate_pair_deseret() {
    // entrada (JS): '\ud801\udc28'
    check("\u{10400}", "iu", &[55297, 56360], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn m_anchors() {
    // entrada (JS): 'a\u000ab\u000ac'
    check("^b$", "m", &[97, 10, 98, 10, 99], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn m_anchors_cr() {
    // entrada (JS): 'a\u000db\u000dc'
    check("^b$", "m", &[97, 13, 98, 13, 99], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn m_anchors_line_sep() {
    // entrada (JS): 'a\u2028b\u2029c'
    check("^b$", "m", &[97, 8232, 98, 8233, 99], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn no_m_anchors() {
    // entrada (JS): 'a\u000ab\u000ac'
    check("^b$", "", &[97, 10, 98, 10, 99], 0, None);
}

#[test]
fn m_all_lines() {
    // entrada (JS): 'ab\u000acd'
    check("^.*$", "mg", &[97, 98, 10, 99, 100], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn m_all_lines_second() {
    // entrada (JS): 'ab\u000acd'
    check("^.*$", "mg", &[97, 98, 10, 99, 100], 2, Some(Expected { captures: &[Some((3, 5))], groups: &[] }));
}

#[test]
fn s_dot_newline() {
    // entrada (JS): 'a\u000ab'
    check("a.b", "s", &[97, 10, 98], 0, Some(Expected { captures: &[Some((0, 3))], groups: &[] }));
}

#[test]
fn no_s_dot_newline() {
    // entrada (JS): 'a\u000ab'
    check("a.b", "", &[97, 10, 98], 0, None);
}

#[test]
fn no_s_dot_line_sep() {
    // entrada (JS): 'a\u2028b'
    check("a.b", "", &[97, 8232, 98], 0, None);
}

#[test]
fn s_dot_line_sep() {
    // entrada (JS): 'a\u2028b'
    check("a.b", "s", &[97, 8232, 98], 0, Some(Expected { captures: &[Some((0, 3))], groups: &[] }));
}

#[test]
fn u_dot_astral() {
    // entrada (JS): '\ud83d\ude00'
    check("^.$", "u", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn no_u_dot_astral() {
    // entrada (JS): '\ud83d\ude00'
    check("^.$", "", &[55357, 56832], 0, None);
}

#[test]
fn y_at_start() {
    // entrada (JS): 'ab'
    check("a", "y", &[97, 98], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn y_not_at_start() {
    // entrada (JS): 'ab'
    check("b", "y", &[97, 98], 0, None);
}

#[test]
fn y_at_offset() {
    // entrada (JS): 'ab'
    check("b", "y", &[97, 98], 1, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn y_with_alternation() {
    // entrada (JS): 'cab'
    check("a|b", "y", &[99, 97, 98], 1, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn y_anchor_bol() {
    // entrada (JS): 'ba'
    check("^a", "y", &[98, 97], 1, None);
}

#[test]
fn y_anchor_bol_multiline() {
    // entrada (JS): 'b\u000aa'
    check("^a", "my", &[98, 10, 97], 2, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn y_empty_match() {
    // entrada (JS): 'abc'
    check("", "y", &[97, 98, 99], 2, Some(Expected { captures: &[Some((2, 2))], groups: &[] }));
}

#[test]
fn g_from_offset() {
    // entrada (JS): 'aXaXa'
    check("a", "g", &[97, 88, 97, 88, 97], 3, Some(Expected { captures: &[Some((4, 5))], groups: &[] }));
}

#[test]
fn g_empty_at_end() {
    // entrada (JS): 'abc'
    check("", "g", &[97, 98, 99], 3, Some(Expected { captures: &[Some((3, 3))], groups: &[] }));
}

#[test]
fn d_flag_is_accepted() {
    // entrada (JS): 'ac a'
    check("(a)(?<n>b)?", "d", &[97, 99, 32, 97], 0, Some(Expected { captures: &[Some((0, 1)), Some((0, 1)), None], groups: &[("n", None)] }));
}

#[test]
fn v_flag_basic() {
    // entrada (JS): 'abcXYZdef'
    check("[\\p{L}--[a-z]]+", "v", &[97, 98, 99, 88, 89, 90, 100, 101, 102], 0, Some(Expected { captures: &[Some((3, 6))], groups: &[] }));
}

#[test]
fn v_flag_intersection() {
    // entrada (JS): 'ab12cd'
    check("[\\w&&\\d]+", "v", &[97, 98, 49, 50, 99, 100], 0, Some(Expected { captures: &[Some((2, 4))], groups: &[] }));
}

#[test]
fn v_flag_nested_class() {
    // entrada (JS): 'dayzb'
    check("[[a-c][x-z]]+", "v", &[100, 97, 121, 122, 98], 0, Some(Expected { captures: &[Some((1, 5))], groups: &[] }));
}

#[test]
fn v_flag_string_literal() {
    // entrada (JS): 'xabc'
    check("[\\q{abc|d}]", "v", &[120, 97, 98, 99], 0, Some(Expected { captures: &[Some((1, 4))], groups: &[] }));
}

#[test]
fn v_flag_emoji_sequence() {
    // entrada (JS): '\ud83d\udc68\u200d\ud83d\udc69\u200d\ud83d\udc67'
    check("^\\p{RGI_Emoji}$", "v", &[55357, 56424, 8205, 55357, 56425, 8205, 55357, 56423], 0, Some(Expected { captures: &[Some((0, 8))], groups: &[] }));
}

#[test]
fn all_flags_combo() {
    // entrada (JS): 'Hello world'
    check("^(?<w>\\w+)\\s", "dgimsy", &[72, 101, 108, 108, 111, 32, 119, 111, 114, 108, 100], 0, Some(Expected { captures: &[Some((0, 6)), Some((0, 5))], groups: &[("w", Some((0, 5)))] }));
}
