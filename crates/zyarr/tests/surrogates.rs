//! Pares substitutos: modo `u` (ponto de código) contra modo não `u` (unidade de código).
//!
//! Os valores esperados saem do Node real (V8 v26.12.1, `RegExp.prototype.exec` com a flag
//! `d` para os índices); os de erro, do Bun 1.4.2 (JavaScriptCore), cuja mensagem o zyarr reproduz.
//! Arquivo gerado a partir da execução dos casos; não editar os números à mão.

mod common;

use common::{check, Expected};

#[test]
fn u_dot_matches_pair() {
    // entrada (JS): '\ud83d\ude00'
    check("^.$", "u", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn non_u_dot_matches_half() {
    // entrada (JS): '\ud83d\ude00'
    check("^.", "", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn non_u_two_dots() {
    // entrada (JS): '\ud83d\ude00'
    check("^..$", "", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn u_pair_literal() {
    // entrada (JS): 'a\ud83d\ude00'
    check("\u{1f600}", "u", &[97, 55357, 56832], 0, Some(Expected { captures: &[Some((1, 3))], groups: &[] }));
}

#[test]
fn u_code_point_escape() {
    // entrada (JS): 'a\ud83d\ude00'
    check("\\u{1F600}", "u", &[97, 55357, 56832], 0, Some(Expected { captures: &[Some((1, 3))], groups: &[] }));
}

#[test]
fn u_surrogate_escape_pair() {
    // entrada (JS): '\ud83d\ude00'
    check("\\ud83d\\ude00", "u", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn non_u_surrogate_escape_pair() {
    // entrada (JS): '\ud83d\ude00'
    check("\\ud83d\\ude00", "", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn u_lone_lead_in_pair_no_match() {
    // entrada (JS): '\ud83d\ude00'
    check("\\ud83d", "u", &[55357, 56832], 0, None);
}

#[test]
fn non_u_lone_lead_in_pair_matches() {
    // entrada (JS): '\ud83d\ude00'
    check("\\ud83d", "", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn u_lone_lead_alone() {
    // entrada (JS): 'a\ud83d b'
    check("\\ud83d", "u", &[97, 55357, 32, 98], 0, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn u_lone_trail_alone() {
    // entrada (JS): 'a\ude00'
    check("\\ude00", "u", &[97, 56832], 0, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn u_lone_trail_after_lead_no_match() {
    // entrada (JS): '\ud83d\ude00'
    check("\\ude00", "u", &[55357, 56832], 0, None);
}

#[test]
fn u_class_astral_range() {
    // entrada (JS): 'a\ud83d\ude01'
    check("[\u{1f600}-\u{1f64f}]", "u", &[97, 55357, 56833], 0, Some(Expected { captures: &[Some((1, 3))], groups: &[] }));
}

#[test]
fn non_u_class_astral_is_halves() {
    // entrada (JS): '\ud83d'
    check("^[\u{1f600}]$", "", &[55357], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn u_class_astral_not_halves() {
    // entrada (JS): '\ud83d'
    check("^[\u{1f600}]$", "u", &[55357], 0, None);
}

#[test]
fn u_negated_class_consumes_pair() {
    // entrada (JS): '\ud83d\ude00'
    check("[^a]", "u", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn non_u_negated_class_one_half() {
    // entrada (JS): '\ud83d\ude00'
    check("[^a]", "", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn u_star_counts_code_points() {
    // entrada (JS): '\ud83d\ude00'
    check("^.{1}$", "u", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn non_u_quantifier_halves() {
    // entrada (JS): '\ud83d\ude00'
    check("^.{2}$", "", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn u_start_inside_pair() {
    // entrada (JS): '\ud83d\ude00x'
    check(".", "gu", &[55357, 56832, 120], 1, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn non_u_start_inside_pair() {
    // entrada (JS): '\ud83d\ude00x'
    check(".", "g", &[55357, 56832, 120], 1, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn u_sticky_inside_pair() {
    // entrada (JS): '\ud83d\ude00x'
    check(".", "yu", &[55357, 56832, 120], 1, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn u_empty_match_inside_pair() {
    // entrada (JS): '\ud83d\ude00'
    check("", "gu", &[55357, 56832], 1, Some(Expected { captures: &[Some((0, 0))], groups: &[] }));
}

#[test]
fn non_u_empty_match_inside_pair() {
    // entrada (JS): '\ud83d\ude00'
    check("", "g", &[55357, 56832], 1, Some(Expected { captures: &[Some((1, 1))], groups: &[] }));
}

#[test]
fn u_search_skips_inside_pair() {
    // entrada (JS): '\ud83d\ude00x'
    check("\\ude00|x", "gu", &[55357, 56832, 120], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn u_lookbehind_pair() {
    // entrada (JS): '\ud83d\ude00x'
    check("(?<=\u{1f600}).", "u", &[55357, 56832, 120], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn u_lookahead_pair() {
    // entrada (JS): 'x\ud83d\ude00'
    check(".(?=\u{1f600})", "u", &[120, 55357, 56832], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn u_ignore_case_astral() {
    // entrada (JS): '\ud801\udc28'
    check("\u{10400}", "iu", &[55297, 56360], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn u_word_boundary_astral() {
    // entrada (JS): '\ud83d\ude00a'
    check("\\b", "gu", &[55357, 56832, 97], 0, Some(Expected { captures: &[Some((2, 2))], groups: &[] }));
}

#[test]
fn u_multiple_pairs() {
    // entrada (JS): '\ud83d\ude00\ud83d\ude01'
    check("(.)(.)", "u", &[55357, 56832, 55357, 56833], 0, Some(Expected { captures: &[Some((0, 4)), Some((0, 2)), Some((2, 4))], groups: &[] }));
}

#[test]
fn non_u_multiple_pairs() {
    // entrada (JS): '\ud83d\ude00\ud83d\ude01'
    check("(.)(.)", "", &[55357, 56832, 55357, 56833], 0, Some(Expected { captures: &[Some((0, 2)), Some((0, 1)), Some((1, 2))], groups: &[] }));
}

#[test]
fn u_backreference_pair() {
    // entrada (JS): '\ud83d\ude00\ud83d\ude00'
    check("(.)\\1", "u", &[55357, 56832, 55357, 56832], 0, Some(Expected { captures: &[Some((0, 4)), Some((0, 2))], groups: &[] }));
}

#[test]
fn u_s_flag_dot_pair() {
    // entrada (JS): 'a\ud83d\ude00b'
    check("a.b", "su", &[97, 55357, 56832, 98], 0, Some(Expected { captures: &[Some((0, 4))], groups: &[] }));
}

#[test]
fn u_hex_astral_class() {
    // entrada (JS): 'x\ud83d\ude00\ud83d\ude01y'
    check("[\\u{1F600}-\\u{1F64F}]+", "u", &[120, 55357, 56832, 55357, 56833, 121], 0, Some(Expected { captures: &[Some((1, 5))], groups: &[] }));
}

#[test]
fn u_empty_class_negated_dot() {
    // entrada (JS): '\ud83d\ude00'
    check("[^]", "u", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn u_lone_trail_start_after_lead() {
    // entrada (JS): 'a\ud83d\ude00'
    check(".", "gu", &[97, 55357, 56832], 2, Some(Expected { captures: &[Some((1, 3))], groups: &[] }));
}

#[test]
fn u_surrogate_pair_lead_then_not_trail() {
    // entrada (JS): '\ud83dx'
    check(".", "gu", &[55357, 120], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn v_pair_literal() {
    // entrada (JS): '\ud83d\ude00'
    check("\u{1f600}", "v", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}
