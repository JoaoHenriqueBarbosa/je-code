//! Porte de `src/vs/editor/test/common/core/stringBuilder.test.ts` (suíte `decodeUTF16LE`), caso a caso e na mesma ordem.
//!
//! A segunda suíte do arquivo (`StringBuilder`, um caso) vem logo depois dos três de `decodeUTF16LE`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::JsString;
use je_base::char_code;
use je_editor::string_builder::{StringBuilder, decode_utf16le};

fn write_uint16_le(buff: &mut [u8], value: u16, offset: usize) {
    buff[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn issue_118041_unicode_character_undo_bug_1() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut buff = [0u8; 2];
    write_uint16_le(&mut buff, 0xFEFF, 0);
    let actual = decode_utf16le(&buff, 0, 1);
    assert_eq!(actual, JsString::from_units(vec![0xFEFF]));
}

#[test]
fn issue_118041_unicode_character_undo_bug_2() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let text = [u16::from(b'a'), 0xFEFF];
    let mut buff = [0u8; 4];
    write_uint16_le(&mut buff, text[0], 0);
    write_uint16_le(&mut buff, text[1], 2);
    let actual = decode_utf16le(&buff, 0, 2);
    assert_eq!(actual, JsString::from_units(text.to_vec()));
}

#[test]
fn issue_118041_unicode_character_undo_bug_3() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let text = [u16::from(b'a'), 0xFEFF, u16::from(b'b')];
    let mut buff = [0u8; 6];
    write_uint16_le(&mut buff, text[0], 0);
    write_uint16_le(&mut buff, text[1], 2);
    write_uint16_le(&mut buff, text[2], 4);
    let actual = decode_utf16le(&buff, 0, 3);
    assert_eq!(actual, JsString::from_units(text.to_vec()));
}

#[test]
fn basic() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let mut sb = StringBuilder::new(100);
    sb.append_ascii_char_code(char_code::A);
    sb.append_ascii_char_code(char_code::Space);
    sb.append_string(&"😊".encode_utf16().collect::<Vec<u16>>());
    assert_eq!(sb.build(), JsString::from("A 😊"));
}
