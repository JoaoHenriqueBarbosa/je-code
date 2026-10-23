//! Classes Unicode `\p{...}` e `\P{...}` em modo `u` e `v`.
//!
//! Os valores esperados saem do Node real (V8 v26.12.1, `RegExp.prototype.exec` com a flag
//! `d` para os índices); os de erro, do Bun 1.4.2 (JavaScriptCore), cuja mensagem o zyarr reproduz.
//! Arquivo gerado a partir da execução dos casos; não editar os números à mão.

mod common;

use common::{check, Expected};

#[test]
fn general_category_letter() {
    // entrada (JS): '12 \u00e9\u00e8\u4e2d x1'
    check("\\p{L}+", "u", &[49, 50, 32, 233, 232, 20013, 32, 120, 49], 0, Some(Expected { captures: &[Some((3, 6))], groups: &[] }));
}

#[test]
fn general_category_long_name() {
    // entrada (JS): '12 abc'
    check("\\p{Letter}+", "u", &[49, 50, 32, 97, 98, 99], 0, Some(Expected { captures: &[Some((3, 6))], groups: &[] }));
}

#[test]
fn general_category_eq() {
    // entrada (JS): 'abCDe'
    check("\\p{General_Category=Lu}+", "u", &[97, 98, 67, 68, 101], 0, Some(Expected { captures: &[Some((2, 4))], groups: &[] }));
}

#[test]
fn gc_short_eq() {
    // entrada (JS): 'a\u0663\u0664b'
    check("\\p{gc=Nd}+", "u", &[97, 1635, 1636, 98], 0, Some(Expected { captures: &[Some((1, 3))], groups: &[] }));
}

#[test]
fn negated_p() {
    // entrada (JS): 'ab12 cd'
    check("\\P{L}+", "u", &[97, 98, 49, 50, 32, 99, 100], 0, Some(Expected { captures: &[Some((2, 5))], groups: &[] }));
}

#[test]
fn script_greek() {
    // entrada (JS): 'abc \u03b1\u03b2\u03b3 d'
    check("\\p{Script=Greek}+", "u", &[97, 98, 99, 32, 945, 946, 947, 32, 100], 0, Some(Expected { captures: &[Some((4, 7))], groups: &[] }));
}

#[test]
fn script_short() {
    // entrada (JS): 'abc \u043f\u0440\u0438\u0432\u0435\u0442'
    check("\\p{sc=Cyrl}+", "u", &[97, 98, 99, 32, 1087, 1088, 1080, 1074, 1077, 1090], 0, Some(Expected { captures: &[Some((4, 10))], groups: &[] }));
}

#[test]
fn script_extensions() {
    // entrada (JS): 'a\u3042\u3044\u30fc'
    check("\\p{Script_Extensions=Hira}+", "u", &[97, 12354, 12356, 12540], 0, Some(Expected { captures: &[Some((1, 4))], groups: &[] }));
}

#[test]
fn script_extensions_short() {
    // entrada (JS): 'a\u30a2\u30fc'
    check("\\p{scx=Kana}+", "u", &[97, 12450, 12540], 0, Some(Expected { captures: &[Some((1, 3))], groups: &[] }));
}

#[test]
fn binary_alphabetic() {
    // entrada (JS): '12abc3'
    check("\\p{Alphabetic}+", "u", &[49, 50, 97, 98, 99, 51], 0, Some(Expected { captures: &[Some((2, 5))], groups: &[] }));
}

#[test]
fn binary_emoji() {
    // entrada (JS): 'a\ud83d\ude00'
    check("\\p{Emoji_Presentation}", "u", &[97, 55357, 56832], 0, Some(Expected { captures: &[Some((1, 3))], groups: &[] }));
}

#[test]
fn binary_ascii() {
    // entrada (JS): '\u00e9abc\u00e9'
    check("\\p{ASCII}+", "u", &[233, 97, 98, 99, 233], 0, Some(Expected { captures: &[Some((1, 4))], groups: &[] }));
}

#[test]
fn binary_any() {
    // entrada (JS): '\ud83d\ude00'
    check("^\\p{Any}$", "u", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn binary_assigned() {
    // entrada (JS): '\u0378a'
    check("\\p{Assigned}", "u", &[888, 97], 0, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn binary_uppercase() {
    // entrada (JS): 'abCDe'
    check("\\p{Uppercase}+", "u", &[97, 98, 67, 68, 101], 0, Some(Expected { captures: &[Some((2, 4))], groups: &[] }));
}

#[test]
fn binary_white_space() {
    // entrada (JS): 'a \u0009\u00a0b'
    check("\\p{White_Space}+", "u", &[97, 32, 9, 160, 98], 0, Some(Expected { captures: &[Some((1, 4))], groups: &[] }));
}

#[test]
fn binary_id_start() {
    // entrada (JS): '1abc_2'
    check("\\p{ID_Start}\\p{ID_Continue}*", "u", &[49, 97, 98, 99, 95, 50], 0, Some(Expected { captures: &[Some((1, 6))], groups: &[] }));
}

#[test]
fn binary_xid_continue() {
    // entrada (JS): '-ab_1-'
    check("\\p{XID_Continue}+", "u", &[45, 97, 98, 95, 49, 45], 0, Some(Expected { captures: &[Some((1, 5))], groups: &[] }));
}

#[test]
fn binary_hex_digit() {
    // entrada (JS): 'xyz09afg'
    check("\\p{ASCII_Hex_Digit}+", "u", &[120, 121, 122, 48, 57, 97, 102, 103], 0, Some(Expected { captures: &[Some((3, 7))], groups: &[] }));
}

#[test]
fn astral_letter() {
    // entrada (JS): '\ud835\udc00'
    check("\\p{L}", "u", &[55349, 56320], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn in_class() {
    // entrada (JS): 'abC1D2e'
    check("[\\p{Lu}\\d]+", "u", &[97, 98, 67, 49, 68, 50, 101], 0, Some(Expected { captures: &[Some((2, 6))], groups: &[] }));
}

#[test]
fn negated_in_class() {
    // entrada (JS): 'ab12cd'
    check("[^\\p{L}]+", "u", &[97, 98, 49, 50, 99, 100], 0, Some(Expected { captures: &[Some((2, 4))], groups: &[] }));
}

#[test]
fn complement_class_p() {
    // entrada (JS): 'ab12cd'
    check("[\\P{L}]+", "u", &[97, 98, 49, 50, 99, 100], 0, Some(Expected { captures: &[Some((2, 4))], groups: &[] }));
}

#[test]
fn ignore_case_lu() {
    // entrada (JS): 'abC'
    check("\\p{Lu}+", "iu", &[97, 98, 67], 0, Some(Expected { captures: &[Some((0, 3))], groups: &[] }));
}

#[test]
fn ignore_case_negated() {
    // entrada (JS): 'abC'
    check("\\P{Lu}+", "iu", &[97, 98, 67], 0, Some(Expected { captures: &[Some((0, 3))], groups: &[] }));
}

#[test]
fn v_mode_emoji_flag() {
    // entrada (JS): '\ud83c\udde7\ud83c\uddf7'
    check("^\\p{RGI_Emoji_Flag_Sequence}$", "v", &[55356, 56807, 55356, 56823], 0, Some(Expected { captures: &[Some((0, 4))], groups: &[] }));
}

#[test]
fn v_mode_basic_emoji() {
    // entrada (JS): '\ud83d\ude00'
    check("^\\p{Basic_Emoji}$", "v", &[55357, 56832], 0, Some(Expected { captures: &[Some((0, 2))], groups: &[] }));
}

#[test]
fn v_mode_difference() {
    // entrada (JS): '\u0391\u03b1\u03b2'
    check("[\\p{Script=Greek}--\\p{Lu}]+", "v", &[913, 945, 946], 0, Some(Expected { captures: &[Some((1, 3))], groups: &[] }));
}

#[test]
fn no_u_p_is_literal() {
    // entrada (JS): 'p{L}'
    check("\\p{L}", "", &[112, 123, 76, 125], 0, Some(Expected { captures: &[Some((0, 4))], groups: &[] }));
}

#[test]
fn cased() {
    // entrada (JS): '1aB2'
    check("\\p{Cased}+", "u", &[49, 97, 66, 50], 0, Some(Expected { captures: &[Some((1, 3))], groups: &[] }));
}

#[test]
fn math_symbol() {
    // entrada (JS): 'a+=<b'
    check("\\p{Sm}+", "u", &[97, 43, 61, 60, 98], 0, Some(Expected { captures: &[Some((1, 4))], groups: &[] }));
}

#[test]
fn currency() {
    // entrada (JS): 'a\u20ac'
    check("\\p{Sc}", "u", &[97, 8364], 0, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn unassigned() {
    // entrada (JS): 'a\u0378'
    check("\\p{Cn}", "u", &[97, 888], 0, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}

#[test]
fn script_unknown() {
    // entrada (JS): 'a\u0378'
    check("\\p{Script=Zzzz}", "u", &[97, 888], 0, Some(Expected { captures: &[Some((1, 2))], groups: &[] }));
}
