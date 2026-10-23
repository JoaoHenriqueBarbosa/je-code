//! Erros de sintaxe: o padrão (ou as flags) que o Node rejeita, com a mensagem do JavaScriptCore.
//!
//! A rejeição confere com o Node real (V8 v26.12.1); a mensagem é a do Bun 1.4.2 (JavaScriptCore), que o
//! zyarr reproduz (o V8 escreve outro texto). Arquivo gerado; não editar à mão.

mod common;

use common::check_error;

#[test]
fn error_00() {
    check_error("(", "", "Invalid regular expression: missing )");
}

#[test]
fn error_01() {
    check_error(")", "", "Invalid regular expression: unmatched parentheses");
}

#[test]
fn error_02() {
    check_error("a)", "", "Invalid regular expression: unmatched parentheses");
}

#[test]
fn error_03() {
    check_error("(?", "", "Invalid regular expression: unrecognized character after (?");
}

#[test]
fn error_04() {
    check_error("(?:", "", "Invalid regular expression: missing )");
}

#[test]
fn error_05() {
    check_error("(?<n>", "", "Invalid regular expression: missing )");
}

#[test]
fn error_06() {
    check_error("[", "", "Invalid regular expression: missing terminating ] for character class");
}

#[test]
fn error_07() {
    check_error("[a", "", "Invalid regular expression: missing terminating ] for character class");
}

#[test]
fn error_08() {
    check_error("[b-a]", "", "Invalid regular expression: range out of order in character class");
}

#[test]
fn error_09() {
    check_error("*", "", "Invalid regular expression: nothing to repeat");
}

#[test]
fn error_10() {
    check_error("+a", "", "Invalid regular expression: nothing to repeat");
}

#[test]
fn error_11() {
    check_error("?", "", "Invalid regular expression: nothing to repeat");
}

#[test]
fn error_12() {
    check_error("a**", "", "Invalid regular expression: nothing to repeat");
}

#[test]
fn error_13() {
    check_error("a{2,1}", "", "Invalid regular expression: numbers out of order in {} quantifier");
}

#[test]
fn error_14() {
    check_error("{1}", "", "Invalid regular expression: nothing to repeat");
}

#[test]
fn error_15() {
    check_error("a{1", "u", "Invalid regular expression: incomplete {} quantifier for Unicode pattern");
}

#[test]
fn error_16() {
    check_error("a{", "u", "Invalid regular expression: incomplete {} quantifier for Unicode pattern");
}

#[test]
fn error_17() {
    check_error("}", "u", "Invalid regular expression: unmatched ] or } bracket for Unicode pattern");
}

#[test]
fn error_18() {
    check_error("]", "u", "Invalid regular expression: unmatched ] or } bracket for Unicode pattern");
}

#[test]
fn error_19() {
    check_error("\\", "", "Invalid regular expression: \\ at end of pattern");
}

#[test]
fn error_20() {
    check_error("(?<a>x)(?<a>y)", "", "Invalid regular expression: duplicate group specifier name");
}

#[test]
fn error_21() {
    check_error("(?<1a>x)", "", "Invalid regular expression: invalid group specifier name");
}

#[test]
fn error_22() {
    check_error("(?<>x)", "", "Invalid regular expression: invalid group specifier name");
}

#[test]
fn error_23() {
    check_error("(?<a", "", "Invalid regular expression: invalid group specifier name");
}

#[test]
fn error_24() {
    check_error("\\k<a>", "u", "Invalid regular expression: invalid \\k<> named backreference");
}

#[test]
fn error_25() {
    check_error("(?<a>x)\\k<b>", "", "Invalid regular expression: invalid \\k<> named backreference");
}

#[test]
fn error_26() {
    check_error("\\u{110000}", "u", "Invalid regular expression: invalid Unicode code point \\u{} escape");
}

#[test]
fn error_27() {
    check_error("\\u{}", "u", "Invalid regular expression: invalid Unicode code point \\u{} escape");
}

#[test]
fn error_28() {
    check_error("\\uZZZZ", "u", "Invalid regular expression: invalid Unicode \\u escape");
}

#[test]
fn error_29() {
    check_error("\\x4", "u", "Invalid regular expression: invalid escaped character for Unicode pattern");
}

#[test]
fn error_30() {
    check_error("\\-", "u", "Invalid regular expression: invalid escaped character for Unicode pattern");
}

#[test]
fn error_31() {
    check_error("\\a", "u", "Invalid regular expression: invalid escaped character for Unicode pattern");
}

#[test]
fn error_32() {
    check_error("\\1", "u", "Invalid regular expression: invalid backreference for Unicode pattern");
}

#[test]
fn error_33() {
    check_error("\\01", "u", "Invalid regular expression: invalid octal escape for Unicode pattern");
}

#[test]
fn error_34() {
    check_error("\\c", "u", "Invalid regular expression: invalid escaped character for Unicode pattern");
}

#[test]
fn error_35() {
    check_error("\\c1", "u", "Invalid regular expression: invalid \\c escape for Unicode pattern");
}

#[test]
fn error_36() {
    check_error("\\p{Nope}", "u", "Invalid regular expression: invalid property expression");
}

#[test]
fn error_37() {
    check_error("\\p{L", "u", "Invalid regular expression: invalid property expression");
}

#[test]
fn error_38() {
    check_error("\\p", "u", "Invalid regular expression: invalid property expression");
}

#[test]
fn error_39() {
    check_error("\\p{Script=Nope}", "u", "Invalid regular expression: invalid property expression");
}

#[test]
fn error_40() {
    check_error("\\P{RGI_Emoji}", "v", "Invalid regular expression: negated class set may contain strings");
}

#[test]
fn error_41() {
    check_error("[^\\p{RGI_Emoji}]", "v", "Invalid regular expression: negated class set may contain strings");
}

#[test]
fn error_42() {
    check_error("[a-\\d]", "u", "Invalid regular expression: invalid range in character class for Unicode pattern");
}

#[test]
fn error_43() {
    check_error("(?=a)*", "u", "Invalid regular expression: nothing to repeat");
}

#[test]
fn error_44() {
    check_error("(?<=a)*", "", "Invalid regular expression: invalid quantifier");
}

#[test]
fn error_45() {
    check_error("[a&&&b]", "v", "Invalid regular expression: invalid class set character");
}

#[test]
fn error_46() {
    check_error("[(]", "v", "Invalid regular expression: invalid class set character");
}

#[test]
fn error_47() {
    check_error("[a--]", "v", "Invalid regular expression: invalid class set character");
}

#[test]
fn error_48() {
    check_error("[\\q{a}&&b--c]", "v", "Invalid regular expression: invalid operation in class set");
}

#[test]
fn error_49() {
    check_error("a", "gg", "Invalid flags supplied to RegExp constructor.");
}

#[test]
fn error_50() {
    check_error("a", "x", "Invalid flags supplied to RegExp constructor.");
}

#[test]
fn error_51() {
    check_error("a", "uv", "Invalid flags supplied to RegExp constructor.");
}

#[test]
fn error_52() {
    check_error("a", "G", "Invalid flags supplied to RegExp constructor.");
}

#[test]
fn error_53() {
    check_error("(?ii:a)", "", "Invalid regular expression: invalid regular expression modifier");
}

#[test]
fn error_54() {
    check_error("(?-:a)", "", "Invalid regular expression: invalid regular expression modifier");
}

#[test]
fn error_55() {
    check_error("(?<=a)+", "u", "Invalid regular expression: invalid quantifier");
}

#[test]
fn error_56() {
    check_error("(?:a|b", "", "Invalid regular expression: missing )");
}

#[test]
fn error_57() {
    check_error("a|*", "", "Invalid regular expression: nothing to repeat");
}

#[test]
fn error_58() {
    check_error("(?!", "", "Invalid regular expression: missing )");
}

#[test]
fn error_59() {
    check_error("[\\", "", "Invalid regular expression: \\ at end of pattern");
}
