//! Grupos nomeados: `groups`, `\k<nome>`, nomes duplicados em alternativas e nomes Unicode.
//!
//! Os valores esperados saem do Node real (V8 v26.12.1, `RegExp.prototype.exec` com a flag
//! `d` para os índices); os de erro, do Bun 1.4.2 (JavaScriptCore), cuja mensagem o zyarr reproduz.
//! Arquivo gerado a partir da execução dos casos; não editar os números à mão.

mod common;

use common::{check, Expected};

#[test]
fn basic() {
    // entrada (JS): 'on 2026-10 ok'
    check("(?<year>\\d{4})-(?<month>\\d{2})", "", &[111, 110, 32, 50, 48, 50, 54, 45, 49, 48, 32, 111, 107], 0, Some(Expected { captures: &[Some((3, 10)), Some((3, 7)), Some((8, 10))], groups: &[("year", Some((3, 7))), ("month", Some((8, 10)))] }));
}

#[test]
fn non_participating() {
    // entrada (JS): 'y'
    check("(?<a>x)|(?<b>y)", "", &[121], 0, Some(Expected { captures: &[Some((0, 1)), None, Some((0, 1))], groups: &[("a", None), ("b", Some((0, 1)))] }));
}

#[test]
fn with_unnamed() {
    // entrada (JS): 'abc'
    check("(a)(?<n>b)(c)", "", &[97, 98, 99], 0, Some(Expected { captures: &[Some((0, 3)), Some((0, 1)), Some((1, 2)), Some((2, 3))], groups: &[("n", Some((1, 2)))] }));
}

#[test]
fn backreference_by_name() {
    // entrada (JS): 'abccd'
    check("(?<c>.)\\k<c>", "", &[97, 98, 99, 99, 100], 0, Some(Expected { captures: &[Some((2, 4)), Some((2, 3))], groups: &[("c", Some((2, 3)))] }));
}

#[test]
fn forward_reference() {
    // entrada (JS): 'x'
    check("\\k<a>(?<a>x)", "", &[120], 0, Some(Expected { captures: &[Some((0, 1)), Some((0, 1))], groups: &[("a", Some((0, 1)))] }));
}

#[test]
fn unicode_name() {
    // entrada (JS): '7'
    check("(?<\u{3c0}>\\d)", "", &[55], 0, Some(Expected { captures: &[Some((0, 1)), Some((0, 1))], groups: &[("\u{3c0}", Some((0, 1)))] }));
}

#[test]
fn unicode_escape_name() {
    // entrada (JS): 'x'
    check("(?<\\u0061b>x)", "", &[120], 0, Some(Expected { captures: &[Some((0, 1)), Some((0, 1))], groups: &[("ab", Some((0, 1)))] }));
}

#[test]
fn astral_name() {
    // entrada (JS): 'x'
    check("(?<\\u{1d4d0}>x)", "u", &[120], 0, Some(Expected { captures: &[Some((0, 1)), Some((0, 1))], groups: &[("\u{1d4d0}", Some((0, 1)))] }));
}

#[test]
fn dollar_underscore_name() {
    // entrada (JS): 'x'
    check("(?<$_a1>x)", "", &[120], 0, Some(Expected { captures: &[Some((0, 1)), Some((0, 1))], groups: &[("$_a1", Some((0, 1)))] }));
}

#[test]
fn duplicate_in_alternatives_first() {
    // entrada (JS): 'x'
    check("(?<a>x)|(?<a>y)", "", &[120], 0, Some(Expected { captures: &[Some((0, 1)), Some((0, 1)), None], groups: &[("a", Some((0, 1)))] }));
}

#[test]
fn duplicate_in_alternatives_second() {
    // entrada (JS): 'y'
    check("(?<a>x)|(?<a>y)", "", &[121], 0, Some(Expected { captures: &[Some((0, 1)), None, Some((0, 1))], groups: &[("a", Some((0, 1)))] }));
}

#[test]
fn duplicate_three_alt() {
    // entrada (JS): '3z'
    check("(?:(?<a>1)|(?<a>2)|(?<a>3))(?<b>z)", "", &[51, 122], 0, Some(Expected { captures: &[Some((0, 2)), None, None, Some((0, 1)), Some((1, 2))], groups: &[("a", Some((0, 1))), ("b", Some((1, 2)))] }));
}

#[test]
fn duplicate_backref() {
    // entrada (JS): 'yy'
    check("(?:(?<a>x)|(?<a>y))\\k<a>", "", &[121, 121], 0, Some(Expected { captures: &[Some((0, 2)), None, Some((0, 1))], groups: &[("a", Some((0, 1)))] }));
}

#[test]
fn duplicate_backref_no_match() {
    // entrada (JS): 'xy'
    check("(?:(?<a>x)|(?<a>y))\\k<a>", "", &[120, 121], 0, None);
}

#[test]
fn duplicate_and_unique() {
    // entrada (JS): 'xbz'
    check("(?<u>.)(?:(?<d>a)|(?<d>b))(?<v>.)", "", &[120, 98, 122], 0, Some(Expected { captures: &[Some((0, 3)), Some((0, 1)), None, Some((1, 2)), Some((2, 3))], groups: &[("u", Some((0, 1))), ("d", Some((1, 2))), ("v", Some((2, 3)))] }));
}

#[test]
fn name_order_first_occurrence() {
    // entrada (JS): 'xyz'
    check("(?<b>.)(?<a>.)(?<c>.)", "", &[120, 121, 122], 0, Some(Expected { captures: &[Some((0, 3)), Some((0, 1)), Some((1, 2)), Some((2, 3))], groups: &[("b", Some((0, 1))), ("a", Some((1, 2))), ("c", Some((2, 3)))] }));
}

#[test]
fn k_without_groups_non_unicode() {
    // entrada (JS): 'k<a>'
    check("\\k<a>", "", &[107, 60, 97, 62], 0, Some(Expected { captures: &[Some((0, 4))], groups: &[] }));
}

#[test]
fn lazy_and_greedy() {
    // entrada (JS): 'aaaa'
    check("(?<l>a+?)(?<g>a+)", "", &[97, 97, 97, 97], 0, Some(Expected { captures: &[Some((0, 4)), Some((0, 1)), Some((1, 4))], groups: &[("l", Some((0, 1))), ("g", Some((1, 4)))] }));
}

#[test]
fn in_lookahead() {
    // entrada (JS): 'a'
    check("(?=(?<x>a))a", "", &[97], 0, Some(Expected { captures: &[Some((0, 1)), Some((0, 1))], groups: &[("x", Some((0, 1)))] }));
}

#[test]
fn in_lookbehind() {
    // entrada (JS): 'ab'
    check("(?<=(?<x>a))b", "", &[97, 98], 0, Some(Expected { captures: &[Some((1, 2)), Some((0, 1))], groups: &[("x", Some((0, 1)))] }));
}

#[test]
fn case_insensitive_backref() {
    // entrada (JS): 'aA'
    check("(?<c>a)\\k<c>", "i", &[97, 65], 0, Some(Expected { captures: &[Some((0, 2)), Some((0, 1))], groups: &[("c", Some((0, 1)))] }));
}

#[test]
fn indices_flag() {
    // entrada (JS): 'zx'
    check("(?<a>x)(?<b>y)?", "d", &[122, 120], 0, Some(Expected { captures: &[Some((1, 2)), Some((1, 2)), None], groups: &[("a", Some((1, 2))), ("b", None)] }));
}
