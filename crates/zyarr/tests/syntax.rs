//! Sintaxe básica: literais, quantificadores, classes, âncoras, lookahead, escapes.
//!
//! Os valores esperados saem do Node real (V8 v26.12.1, `RegExp.prototype.exec` com a flag
//! `d` para os índices); os de erro, do Bun 1.4.2 (JavaScriptCore), cuja mensagem o zyarr reproduz.
//! Arquivo gerado a partir da execução dos casos; não editar os números à mão.

mod common;

use common::{check, Expected};

#[test]
fn literal() {
    // entrada (JS): 'xxabcxx'
    check("abc", "", &[120, 120, 97, 98, 99, 120, 120], 0, Some(Expected { captures: &[Some((2, 5))], groups: &[] }));
}

#[test]
fn literal_no_match() {
    // entrada (JS): 'xxabcxx'
    check("abd", "", &[120, 120, 97, 98, 99, 120, 120], 0, None);
}

#[test]
fn empty_pattern() {
    // entrada (JS): 'abc'
    check("", "", &[97, 98, 99], 0, Some(Expected { captures: &[Some((0, 0))], groups: &[] }));
}

#[test]
fn alternation() {
    // entrada (JS): 'hotdog'
    check("cat|dog|bird", "", &[104, 111, 116, 100, 111, 103], 0, Some(Expected { captures: &[Some((3, 6))], groups: &[] }));
}

#[test]
fn alternation_leftmost_first() {
    // entrada (JS): 'abc'
    check("a|ab", "", &[97, 98, 99], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn star_greedy() {
    // entrada (JS): 'aaab'
    check("a*", "", &[97, 97, 97, 98], 0, Some(Expected { captures: &[Some((0, 3))], groups: &[] }));
}

#[test]
fn star_lazy() {
    // entrada (JS): 'aaab'
    check("a*?", "", &[97, 97, 97, 98], 0, Some(Expected { captures: &[Some((0, 0))], groups: &[] }));
}

#[test]
fn plus_greedy() {
    // entrada (JS): 'baaab'
    check("a+", "", &[98, 97, 97, 97, 98], 0, Some(Expected { captures: &[Some((1, 4))], groups: &[] }));
}

#[test]
fn plus_lazy() {
    // entrada (JS): 'baaab'
    check("a+?", "", &[98, 97, 97, 97, 98], 0, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn question() {
    // entrada (JS): 'color colour'
    check("colou?r", "", &[99, 111, 108, 111, 114, 32, 99, 111, 108, 111, 117, 114], 0, Some(Expected { captures: &[Some((0, 5))], groups: &[] }));
}

#[test]
fn question_lazy() {
    // entrada (JS): 'ab'
    check("a??b", "", &[97, 98], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn counted_exact() {
    // entrada (JS): 'aaaaa'
    check("a{3}", "", &[97, 97, 97, 97, 97], 0, Some(Expected { captures: &[Some((0, 3))], groups: &[] }));
}

#[test]
fn counted_range() {
    // entrada (JS): 'aaaaaa'
    check("a{2,4}", "", &[97, 97, 97, 97, 97, 97], 0, Some(Expected { captures: &[Some((0, 4))], groups: &[] }));
}

#[test]
fn counted_open() {
    // entrada (JS): 'aaaaaa'
    check("a{2,}", "", &[97, 97, 97, 97, 97, 97], 0, Some(Expected { captures: &[Some((0, 6))], groups: &[] }));
}

#[test]
fn counted_lazy() {
    // entrada (JS): 'aaaaaa'
    check("a{2,4}?", "", &[97, 97, 97, 97, 97, 97], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn brace_literal_non_unicode() {
    // entrada (JS): 'a{,3}'
    check("a{,3}", "", &[97, 123, 44, 51, 125], 0, Some(Expected { captures: &[Some((0, 5))], groups: &[] }));
}

#[test]
fn capture_groups() {
    // entrada (JS): 'ac'
    check("(a)(b)?(c)", "", &[97, 99], 0, Some(Expected { captures: &[Some((0, 2)), Some((0, 1)), None, Some((1, 2))], groups: &[] }));
}

#[test]
fn nested_groups() {
    // entrada (JS): 'abab'
    check("((a)(b))+", "", &[97, 98, 97, 98], 0, Some(Expected { captures: &[Some((0, 4)), Some((2, 4)), Some((2, 3)), Some((3, 4))], groups: &[] }));
}

#[test]
fn group_reset_in_repeat() {
    // entrada (JS): 'ab'
    check("(?:(a)|b)+", "", &[97, 98], 0, Some(Expected { captures: &[Some((0, 2)), None], groups: &[] }));
}

#[test]
fn non_capturing() {
    // entrada (JS): 'ababc'
    check("(?:ab)+c", "", &[97, 98, 97, 98, 99], 0, Some(Expected { captures: &[Some((0, 5))], groups: &[] }));
}

#[test]
fn dot() {
    // entrada (JS): 'a\u000ac abc'
    check("a.c", "", &[97, 10, 99, 32, 97, 98, 99], 0, Some(Expected { captures: &[Some((4, 7))], groups: &[] }));
}

#[test]
fn class_range() {
    // entrada (JS): 'xyz 0fa9 g'
    check("[a-f0-9]+", "", &[120, 121, 122, 32, 48, 102, 97, 57, 32, 103], 0, Some(Expected { captures: &[Some((4, 8))], groups: &[] }));
}

#[test]
fn class_negated() {
    // entrada (JS): 'abcdefabc'
    check("[^abc]+", "", &[97, 98, 99, 100, 101, 102, 97, 98, 99], 0, Some(Expected { captures: &[Some((3, 6))], groups: &[] }));
}

#[test]
fn class_escapes_in_class() {
    // entrada (JS): 'ab 12 3x'
    check("[\\d\\s]+", "", &[97, 98, 32, 49, 50, 32, 51, 120], 0, Some(Expected { captures: &[Some((2, 7))], groups: &[] }));
}

#[test]
fn digit_word_space() {
    // entrada (JS): 'x 12ab c'
    check("\\d+\\w+\\s", "", &[120, 32, 49, 50, 97, 98, 32, 99], 0, Some(Expected { captures: &[Some((2, 7))], groups: &[] }));
}

#[test]
fn non_digit_non_word() {
    // entrada (JS): 'a1 -'
    check("\\D\\W", "", &[97, 49, 32, 45], 0, Some(Expected { captures: &[Some((2, 4))], groups: &[] }));
}

#[test]
fn word_boundary() {
    // entrada (JS): 'afoo foo foob'
    check("\\bfoo\\b", "", &[97, 102, 111, 111, 32, 102, 111, 111, 32, 102, 111, 111, 98], 0, Some(Expected { captures: &[Some((5, 8))], groups: &[] }));
}

#[test]
fn non_word_boundary() {
    // entrada (JS): 'foo fooo'
    check("\\Boo\\B", "", &[102, 111, 111, 32, 102, 111, 111, 111], 0, Some(Expected { captures: &[Some((5, 7))], groups: &[] }));
}

#[test]
fn anchors() {
    // entrada (JS): 'abc'
    check("^abc$", "", &[97, 98, 99], 0, Some(Expected { captures: &[Some((0, 3))], groups: &[] }));
}

#[test]
fn anchors_no_match() {
    // entrada (JS): 'abc\u000a'
    check("^abc$", "", &[97, 98, 99, 10], 0, None);
}

#[test]
fn lookahead_pos() {
    // entrada (JS): 'acab'
    check("a(?=b)", "", &[97, 99, 97, 98], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn lookahead_neg() {
    // entrada (JS): 'abac'
    check("a(?!b)", "", &[97, 98, 97, 99], 0, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn lookahead_capture() {
    // entrada (JS): 'baaabac'
    check("(?=(a+))a*b\\1", "", &[98, 97, 97, 97, 98, 97, 99], 0, Some(Expected { captures: &[Some((3, 6)), Some((3, 4))], groups: &[] }));
}

#[test]
fn escapes() {
    // entrada (JS): '\u0009\u000aAB\u000a'
    check("\\t\\n\\x41\\u0042\\cJ", "", &[9, 10, 65, 66, 10], 0, Some(Expected { captures: &[Some((0, 5))], groups: &[] }));
}

#[test]
fn hex_escape_class() {
    // entrada (JS): 'zABCD'
    check("[\\x41-\\x43]+", "", &[122, 65, 66, 67, 68], 0, Some(Expected { captures: &[Some((1, 4))], groups: &[] }));
}

#[test]
fn control_escapes() {
    // entrada (JS): '\u0000\u000b\u000c\u000d'
    check("\\0\\v\\f\\r", "", &[0, 11, 12, 13], 0, Some(Expected { captures: &[Some((0, 4))], groups: &[] }));
}

#[test]
fn identity_escape_legacy() {
    // entrada (JS): 'a-_'
    check("\\a\\-\\_", "", &[97, 45, 95], 0, Some(Expected { captures: &[Some((0, 3))], groups: &[] }));
}

#[test]
fn octal_legacy() {
    // entrada (JS): 'A'
    check("\\101", "", &[65], 0, Some(Expected { captures: &[Some((0, 1))], groups: &[] }));
}

#[test]
fn backspace_in_class() {
    // entrada (JS): 'a\u0008b'
    check("[\\b]", "", &[97, 8, 98], 0, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn quantified_group_empty() {
    // entrada (JS): 'b'
    check("(a*)*", "", &[98], 0, Some(Expected { captures: &[Some((0, 0)), None], groups: &[] }));
}

#[test]
fn quantified_group_empty_plus() {
    // entrada (JS): 'b'
    check("(a*)+", "", &[98], 0, Some(Expected { captures: &[Some((0, 0)), Some((0, 0))], groups: &[] }));
}

#[test]
fn empty_alternative_loop() {
    // entrada (JS): 'aaa'
    check("(?:|a)+", "", &[97, 97, 97], 0, Some(Expected { captures: &[Some((0, 3))], groups: &[] }));
}

#[test]
fn greedy_backtracking() {
    // entrada (JS): 'aaaa'
    check("(a+)(a+)", "", &[97, 97, 97, 97], 0, Some(Expected { captures: &[Some((0, 4)), Some((0, 3)), Some((3, 4))], groups: &[] }));
}

#[test]
fn optional_group_lazy() {
    // entrada (JS): 'ab'
    check("(a)??b", "", &[97, 98], 0, Some(Expected { captures: &[Some((0, 2)), Some((0, 1))], groups: &[] }));
}

#[test]
fn nested_quantifiers() {
    // entrada (JS): 'aaaa'
    check("(a{1,2}){2}", "", &[97, 97, 97, 97], 0, Some(Expected { captures: &[Some((0, 4)), Some((2, 4))], groups: &[] }));
}

#[test]
fn dollar_literal_class() {
    // entrada (JS): 'a^$'
    check("[$^]", "", &[97, 94, 36], 0, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn caret_mid() {
    // entrada (JS): 'a^b'
    check("a^b", "", &[97, 94, 98], 0, None);
}

#[test]
fn start_offset() {
    // entrada (JS): 'aXaXa'
    check("a", "g", &[97, 88, 97, 88, 97], 1, Some(Expected { captures: &[Some((2, 3))], groups: &[] }));
}

#[test]
fn start_offset_past_match() {
    // entrada (JS): 'aXaXa'
    check("a", "g", &[97, 88, 97, 88, 97], 5, None);
}

#[test]
fn start_beyond_length() {
    // entrada (JS): 'aXa'
    check("a", "g", &[97, 88, 97], 4, None);
}

#[test]
fn latin1_chars() {
    // entrada (JS): 'um caf\u00e9'
    check("caf\u{e9}", "", &[117, 109, 32, 99, 97, 102, 233], 0, Some(Expected { captures: &[Some((3, 7))], groups: &[] }));
}

#[test]
fn bmp_non_latin() {
    // entrada (JS): 'x\u3042\u3042y'
    check("\u{3042}+", "", &[120, 12354, 12354, 121], 0, Some(Expected { captures: &[Some((1, 3))], groups: &[] }));
}
