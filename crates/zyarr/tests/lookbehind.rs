//! Lookbehind positivo e negativo: casamento para trás, capturas e backreferences dentro dele.
//!
//! Os valores esperados saem do Node real (V8 v26.12.1, `RegExp.prototype.exec` com a flag
//! `d` para os índices); os de erro, do Bun 1.4.2 (JavaScriptCore), cuja mensagem o zyarr reproduz.
//! Arquivo gerado a partir da execução dos casos; não editar os números à mão.

mod common;

use common::{check, check_units, Expected};

#[test]
fn positive() {
    // entrada (JS): 'cbab'
    check("(?<=a)b", "", &[99, 98, 97, 98], 0, Some(Expected { captures: &[Some((3, 4))], groups: &[] }));
}

#[test]
fn negative() {
    // entrada (JS): 'abcb'
    check("(?<!a)b", "", &[97, 98, 99, 98], 0, Some(Expected { captures: &[Some((3, 4))], groups: &[] }));
}

#[test]
fn variable_length() {
    // entrada (JS): 'aaab'
    check("(?<=a+)b", "", &[97, 97, 97, 98], 0, Some(Expected { captures: &[Some((3, 4))], groups: &[] }));
}

#[test]
fn alternation() {
    // entrada (JS): 'abd cd xd'
    check("(?<=ab|c)d", "", &[97, 98, 100, 32, 99, 100, 32, 120, 100], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn capture_backward_order() {
    // entrada (JS): '1053'
    check("(?<=(\\d+)(\\d+))$", "", &[49, 48, 53, 51], 0, Some(Expected { captures: &[Some((4, 4)), Some((0, 1)), Some((1, 4))], groups: &[] }));
}

#[test]
fn capture_greedy_backward() {
    // entrada (JS): 'aab'
    check("(?<=\\1(a))b", "", &[97, 97, 98], 0, Some(Expected { captures: &[Some((2, 3)), Some((1, 2))], groups: &[] }));
}

#[test]
fn backreference_inside() {
    // entrada (JS): 'aab xbb'
    check("(?<=(.)\\1)b", "", &[97, 97, 98, 32, 120, 98, 98], 0, Some(Expected { captures: &[Some((2, 3)), Some((1, 2))], groups: &[] }));
}

#[test]
fn nested_lookahead() {
    // entrada (JS): 'ab'
    check("(?<=a(?=b))b", "", &[97, 98], 0, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn at_start_fails() {
    // entrada (JS): 'b'
    check("(?<=a)b", "", &[98], 0, None);
}

#[test]
fn negative_at_start() {
    // entrada (JS): 'b'
    check("(?<!a)b", "", &[98], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn quantified_inside() {
    // entrada (JS): 'aab ab'
    check("(?<=a{2})b", "", &[97, 97, 98, 32, 97, 98], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn lazy_inside() {
    // entrada (JS): 'aaab'
    check("(?<=(a+?))b", "", &[97, 97, 97, 98], 0, Some(Expected { captures: &[Some((3, 4)), Some((2, 3))], groups: &[] }));
}

#[test]
fn unicode_astral() {
    // entrada (JS): '\ud83d\ude00x'
    check("(?<=\u{1f600})x", "u", &[55357, 56832, 120], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn unicode_astral_class() {
    // entrada (JS): '\ud83d\ude01x'
    check("(?<=[\u{1f600}-\u{1f64f}])x", "u", &[55357, 56833, 120], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn non_unicode_astral_half() {
    // entrada (JS): '\ud83d\ude00x'
    check_units(&[40, 63, 60, 61, 56832, 41, 120], "", &[55357, 56832, 120], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn with_g_offset() {
    // entrada (JS): 'abab'
    check("(?<=a)b", "g", &[97, 98, 97, 98], 2, Some(Expected { captures: &[Some((3, 4))], groups: &[] }));
}

#[test]
fn sees_before_start() {
    // entrada (JS): 'ab'
    check("(?<=a)b", "g", &[97, 98], 1, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn sticky_sees_before() {
    // entrada (JS): 'ab'
    check("(?<=a)b", "y", &[97, 98], 1, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn word_boundary_inside() {
    // entrada (JS): 'foobar'
    check("(?<=\\bfoo)bar", "", &[102, 111, 111, 98, 97, 114], 0, Some(Expected { captures: &[Some((3, 6))], groups: &[] }));
}

#[test]
fn dollar_inside() {
    // entrada (JS): 'a'
    check("(?<=a$)", "", &[97], 0, Some(Expected { captures: &[Some((1, 1))], groups: &[] }));
}

#[test]
fn case_insensitive() {
    // entrada (JS): 'ab'
    check("(?<=A)b", "i", &[97, 98], 0, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}
