//! Backreferences numéricas: grupo que não participou, referência adiante, octal legado.
//!
//! Os valores esperados saem do Node real (V8 v26.12.1, `RegExp.prototype.exec` com a flag
//! `d` para os índices); os de erro, do Bun 1.4.2 (JavaScriptCore), cuja mensagem o zyarr reproduz.
//! Arquivo gerado a partir da execução dos casos; não editar os números à mão.

mod common;

use common::{check, Expected};

#[test]
fn basic() {
    // entrada (JS): 'aa'
    check("(a)\\1", "", &[97, 97], 0, Some(Expected { captures: &[Some((0, 2)), Some((0, 1))], groups: &[] }));
}

#[test]
fn basic_no_match() {
    // entrada (JS): 'ab'
    check("(a)\\1", "", &[97, 98], 0, None);
}

#[test]
fn group_not_participating() {
    // entrada (JS): 'b'
    check("(?:(a)|b)\\1", "", &[98], 0, Some(Expected { captures: &[Some((0, 1)), None], groups: &[] }));
}

#[test]
fn forward_reference_matches_empty() {
    // entrada (JS): 'a'
    check("\\1(a)", "", &[97], 0, Some(Expected { captures: &[Some((0, 1)), Some((0, 1))], groups: &[] }));
}

#[test]
fn self_reference() {
    // entrada (JS): 'a'
    check("(a\\1)", "", &[97], 0, Some(Expected { captures: &[Some((0, 1)), Some((0, 1))], groups: &[] }));
}

#[test]
fn quantified() {
    // entrada (JS): 'aaa'
    check("(a)\\1{2}", "", &[97, 97, 97], 0, Some(Expected { captures: &[Some((0, 3)), Some((0, 1))], groups: &[] }));
}

#[test]
fn two_digit() {
    // entrada (JS): 'abcdefghijj'
    check("(a)(b)(c)(d)(e)(f)(g)(h)(i)(j)\\10", "", &[97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 106], 0, Some(Expected { captures: &[Some((0, 11)), Some((0, 1)), Some((1, 2)), Some((2, 3)), Some((3, 4)), Some((4, 5)), Some((5, 6)), Some((6, 7)), Some((7, 8)), Some((8, 9)), Some((9, 10))], groups: &[] }));
}

#[test]
fn legacy_octal_fallback() {
    // entrada (JS): 'a\u0002'
    check("(a)\\2", "", &[97, 2], 0, Some(Expected { captures: &[Some((0, 2)), Some((0, 1))], groups: &[] }));
}

#[test]
fn big_number_literal() {
    // entrada (JS): '8'
    check("\\8", "", &[56], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn ignore_case() {
    // entrada (JS): 'xX'
    check("(\\w)\\1", "i", &[120, 88], 0, Some(Expected { captures: &[Some((0, 2)), Some((0, 1))], groups: &[] }));
}

#[test]
fn ignore_case_unicode() {
    // entrada (JS): '\u00e9\u00c9'
    check("(\u{e9})\\1", "iu", &[233, 201], 0, Some(Expected { captures: &[Some((0, 2)), Some((0, 1))], groups: &[] }));
}

#[test]
fn repeated_capture_resets() {
    // entrada (JS): 'ab'
    check("(?:(a)|(b))+", "", &[97, 98], 0, Some(Expected { captures: &[Some((0, 2)), None, Some((1, 2))], groups: &[] }));
}

#[test]
fn in_repeat() {
    // entrada (JS): 'aabbcd'
    check("(?:(\\w)\\1)+", "", &[97, 97, 98, 98, 99, 100], 0, Some(Expected { captures: &[Some((0, 4)), Some((2, 3))], groups: &[] }));
}

#[test]
fn lazy() {
    // entrada (JS): 'aaaa'
    check("(a+?)\\1", "", &[97, 97, 97, 97], 0, Some(Expected { captures: &[Some((0, 2)), Some((0, 1))], groups: &[] }));
}

#[test]
fn empty_capture() {
    // entrada (JS): 'a'
    check("()\\1a", "", &[97], 0, Some(Expected { captures: &[Some((0, 1)), Some((0, 0))], groups: &[] }));
}

#[test]
fn in_lookahead() {
    // entrada (JS): 'abccd'
    check("(?=(.)\\1)..", "", &[97, 98, 99, 99, 100], 0, Some(Expected { captures: &[Some((2, 4)), Some((2, 3))], groups: &[] }));
}

#[test]
fn unicode_astral() {
    // entrada (JS): '\ud83d\ude00\ud83d\ude00'
    check("(.)\\1", "u", &[55357, 56832, 55357, 56832], 0, Some(Expected { captures: &[Some((0, 4)), Some((0, 2))], groups: &[] }));
}

#[test]
fn non_unicode_astral_halves() {
    // entrada (JS): '\ud83d\ude00\ud83d\ude00'
    check("(.)\\1", "", &[55357, 56832, 55357, 56832], 0, None);
}

#[test]
fn backreference_to_later_group_in_alternation() {
    // entrada (JS): 'ab'
    check("(?:\\1(a)|b)+", "", &[97, 98], 0, Some(Expected { captures: &[Some((0, 2)), None], groups: &[] }));
}
