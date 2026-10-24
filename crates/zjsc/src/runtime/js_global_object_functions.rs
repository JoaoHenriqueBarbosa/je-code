//! Porte das funções puras de `runtime/JSGlobalObjectFunctions.cpp`: `parseInt`, `parseFloat`, `isNaN`,
//! `isFinite`, `encodeURI`/`encodeURIComponent`, `decodeURI`/`decodeURIComponent`, `escape`, `unescape`
//! e `jsToNumber(StringView)`.
//!
//! DIVERGÊNCIAS (ligação pendente, a `NativeFunction` está sendo redesenhada):
//!
//! - Cada `globalFuncX` vira uma casca fina: `toStringView` (`toString` do argumento, com
//!   `RETURN_IF_EXCEPTION`), as conversões (`toInt32` do radix, `toNumber`) e a chamada da função daqui;
//!   os erros (`UriError`, `OutOfMemory`) viram `createURIError(globalObject, mensagem)` e
//!   `throwOutOfMemoryError`. O atalho de `parseInt`/`parseFloat` para valor já numérico está em
//!   `parse_int_number` e `parse_float_number`.
//! - As funções trabalham sobre unidades UTF-16 (`&[u16]`), sem as duplicatas de 8 bits do C++; o
//!   resultado é estreitado para Latin1 quando todas as unidades cabem (`string_from_units`).
//! - Não portadas aqui: `eval`, `throwTypeError`, `protoGetter/Setter`, `importModule`,
//!   `copyDataProperties`, `cloneObject`, os handlers de `Proxy` e `speciesGetter` (dependem de
//!   objetos, de `CallFrame` e do `VM`); `toIntegerOrInfinity` e `toLength` (conversões do `JSValue`).

use crate::parser::lexer::Lexer;
use crate::runtime::js_value_conversions::{js_str_decimal_literal, skip_str_white_space};
use crate::runtime::string_prototype::{code_units, string_from_units};
use crate::wtf::ascii_ctype::{is_ascii_digit, is_ascii_hex_digit};
use crate::wtf::text::string_impl::MAX_LENGTH;
use crate::wtf::text::wtf_string::String as WtfString;

/// A mensagem do `URIError` de `encode`.
pub const ILLEGAL_UTF16_SEQUENCE: &str = "String contained an illegal UTF-16 sequence.";

/// A mensagem do `URIError` de `decode`.
pub const URI_ERROR: &str = "URI error";

/// O que as funções de URI podem lançar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalFunctionError {
    /// `createURIError(globalObject, mensagem)`.
    UriError(&'static str),
    /// `throwOutOfMemoryError` (o `StringBuilder` com `RecordOverflow` estourou).
    OutOfMemory,
}

/// `makeLatin1CharacterBitSet(...)` de `encodeURI`.
pub const DO_NOT_ESCAPE_WHEN_ENCODING_URI: &str =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!#$&'()*+,-./:;=?@_~";

/// `makeLatin1CharacterBitSet(...)` de `encodeURIComponent`.
pub const DO_NOT_ESCAPE_WHEN_ENCODING_URI_COMPONENT: &str =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!'()*-._~";

/// `makeLatin1CharacterBitSet(...)` de `decodeURI`.
pub const DO_NOT_UNESCAPE_WHEN_DECODING_URI: &str = "#$&+,/:;=?@";

/// `makeLatin1CharacterBitSet(...)` de `escape`.
pub const DO_NOT_ESCAPE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789*+-./@_";

fn in_set(set: &str, unit: u16) -> bool {
    unit < 128 && set.as_bytes().contains(&(unit as u8))
}

fn is_lead_surrogate(unit: u16) -> bool {
    (0xD800..=0xDBFF).contains(&unit)
}

fn is_trail_surrogate(unit: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&unit)
}

fn push_hex_escape(out: &mut Vec<u16>, octet: u8) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    out.push(u16::from(b'%'));
    out.push(u16::from(HEX[(octet >> 4) as usize]));
    out.push(u16::from(HEX[(octet & 0xF) as usize]));
}

fn finish(units: Vec<u16>) -> Result<WtfString, GlobalFunctionError> {
    if units.len() > MAX_LENGTH as usize {
        return Err(GlobalFunctionError::OutOfMemory);
    }
    Ok(string_from_units(&units))
}

/// `encode(globalObject, doNotEscape, characters)` (18.2.6.1.1 Encode).
pub fn encode(characters: &[u16], do_not_escape: &str) -> Result<WtfString, GlobalFunctionError> {
    let illegal = GlobalFunctionError::UriError(ILLEGAL_UTF16_SEQUENCE);
    let mut out: Vec<u16> = Vec::with_capacity(characters.len());
    let mut index = 0;
    while index < characters.len() {
        let character = characters[index];
        index += 1;

        if in_set(do_not_escape, character) {
            out.push(character);
            continue;
        }

        // 4-d-i. A trail surrogate sozinho é erro.
        if is_trail_surrogate(character) {
            return Err(illegal);
        }

        let code_point: u32 = if !is_lead_surrogate(character) {
            u32::from(character)
        } else {
            // 4-d-iii. Lead precisa de um trail logo depois.
            let Some(&trail) = characters.get(index) else {
                return Err(illegal);
            };
            index += 1;
            if !is_trail_surrogate(trail) {
                return Err(illegal);
            }
            0x10000 + ((u32::from(character) - 0xD800) << 10) + (u32::from(trail) - 0xDC00)
        };

        // 4-d-iv. UTF-8 do code point (sempre válido aqui: não é surrogate).
        let mut buffer = [0u8; 4];
        let scalar = char::from_u32(code_point).ok_or(illegal)?;
        for &octet in scalar.encode_utf8(&mut buffer).as_bytes() {
            push_hex_escape(&mut out, octet);
        }
    }
    finish(out)
}

/// `encodeURI`.
pub fn encode_uri(characters: &[u16]) -> Result<WtfString, GlobalFunctionError> {
    encode(characters, DO_NOT_ESCAPE_WHEN_ENCODING_URI)
}

/// `encodeURIComponent`.
pub fn encode_uri_component(characters: &[u16]) -> Result<WtfString, GlobalFunctionError> {
    encode(characters, DO_NOT_ESCAPE_WHEN_ENCODING_URI_COMPONENT)
}

fn hex_pair_at(characters: &[u16], k: usize) -> Option<u8> {
    let p1 = *characters.get(k + 1)?;
    let p2 = *characters.get(k + 2)?;
    (characters[k] == u16::from(b'%') && is_ascii_hex_digit(p1) && is_ascii_hex_digit(p2))
        .then(|| Lexer::<u16>::convert_hex(i32::from(p1), i32::from(p2)))
}

/// `U8_COUNT_TRAIL_BYTES` do ICU: bytes de continuação de um byte líder válido (`0xC2..=0xF4`).
fn count_trail_bytes(lead: u8) -> usize {
    if (0xC2..=0xF4).contains(&lead) {
        usize::from(lead >= 0xE0) + usize::from(lead >= 0xF0) + 1
    } else {
        0
    }
}

/// `decode(globalObject, characters, doNotUnescape, strict)`.
pub fn decode(characters: &[u16], do_not_unescape: &str, strict: bool) -> Result<WtfString, GlobalFunctionError> {
    let mut out: Vec<u16> = Vec::with_capacity(characters.len());
    let size = characters.len();
    let mut k = 0;
    while k < size {
        let c = characters[k];
        if c == u16::from(b'%') {
            let mut char_len = 0;
            let mut u: u16 = 0;
            if k + 3 <= size {
                if let Some(b0) = hex_pair_at(characters, k) {
                    let sequence_len = 1 + count_trail_bytes(b0);
                    if k + sequence_len * 3 <= size {
                        char_len = sequence_len * 3;
                        let mut sequence = [0u8; 4];
                        sequence[0] = b0;
                        for (i, slot) in sequence.iter_mut().enumerate().take(sequence_len).skip(1) {
                            match hex_pair_at(characters, k + i * 3) {
                                Some(byte) => *slot = byte,
                                None => {
                                    char_len = 0;
                                    break;
                                }
                            }
                        }
                        if char_len != 0 {
                            // `U8_NEXT`: decodifica e valida (excesso, surrogates, > U+10FFFF).
                            match std::str::from_utf8(&sequence[..sequence_len]).ok().and_then(|text| text.chars().next()) {
                                None => char_len = 0,
                                Some(character) => {
                                    let code_point = character as u32;
                                    if code_point > 0xFFFF {
                                        let offset = code_point - 0x10000;
                                        out.push(0xD800 + (offset >> 10) as u16);
                                        u = 0xDC00 + (offset & 0x3FF) as u16;
                                    } else {
                                        u = code_point as u16;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if char_len == 0 {
                if strict {
                    return Err(GlobalFunctionError::UriError(URI_ERROR));
                }
                // O caso `unescape` do WinIE: `%uXXXX`.
                if k + 6 <= size
                    && characters[k + 1] == u16::from(b'u')
                    && characters[k + 2..k + 6].iter().all(|&unit| is_ascii_hex_digit(unit))
                {
                    char_len = 6;
                    u = Lexer::<u16>::convert_unicode(
                        i32::from(characters[k + 2]),
                        i32::from(characters[k + 3]),
                        i32::from(characters[k + 4]),
                        i32::from(characters[k + 5]),
                    );
                }
            }
            if char_len != 0 && (u >= 128 || !in_set(do_not_unescape, u)) {
                out.push(u);
                k += char_len;
                continue;
            }
        }
        k += 1;
        out.push(c);
    }
    finish(out)
}

/// `decodeURI`.
pub fn decode_uri(characters: &[u16]) -> Result<WtfString, GlobalFunctionError> {
    decode(characters, DO_NOT_UNESCAPE_WHEN_DECODING_URI, true)
}

/// `decodeURIComponent`.
pub fn decode_uri_component(characters: &[u16]) -> Result<WtfString, GlobalFunctionError> {
    decode(characters, "", true)
}

/// `globalFuncEscape`.
pub fn escape(characters: &[u16]) -> Result<WtfString, GlobalFunctionError> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out: Vec<u16> = Vec::with_capacity(characters.len());
    for &character in characters {
        if character >= 256 {
            out.push(u16::from(b'%'));
            out.push(u16::from(b'u'));
            for byte in [(character >> 8) as u8, character as u8] {
                out.push(u16::from(HEX[(byte >> 4) as usize]));
                out.push(u16::from(HEX[(byte & 0xF) as usize]));
            }
        } else if in_set(DO_NOT_ESCAPE, character) {
            out.push(character);
        } else {
            push_hex_escape(&mut out, character as u8);
        }
    }
    finish(out)
}

/// `globalFuncUnescape`.
pub fn unescape(characters: &[u16]) -> Result<WtfString, GlobalFunctionError> {
    // `int` de propósito no C++: `k <= length - 6` precisa valer mesmo com `length < 6`.
    let length = characters.len() as i64;
    let mut k: i64 = 0;
    let mut out: Vec<u16> = Vec::with_capacity(characters.len());
    while k < length {
        let at = |offset: i64| characters[(k + offset) as usize];
        let mut value = at(0);
        let mut advance: i64 = 0;
        if value == u16::from(b'%') && k <= length - 6 && at(1) == u16::from(b'u') {
            if (2..6).all(|offset| is_ascii_hex_digit(at(offset))) {
                out.push(Lexer::<u16>::convert_unicode(
                    i32::from(at(2)),
                    i32::from(at(3)),
                    i32::from(at(4)),
                    i32::from(at(5)),
                ));
                k += 6;
                continue;
            }
        } else if value == u16::from(b'%') && k <= length - 3 && is_ascii_hex_digit(at(1)) && is_ascii_hex_digit(at(2)) {
            value = u16::from(Lexer::<u16>::convert_hex(i32::from(at(1)), i32::from(at(2))));
            advance = 2;
        }
        out.push(value);
        k += advance + 1;
    }
    finish(out)
}

/// `globalFuncParseInt` sobre a string: `parseInt(view, radix)` com o `radix` já passado por `toInt32`.
pub(crate) use crate::runtime::parse_int::parse_int as parse_int_string;

/// O atalho de `globalFuncParseInt` para argumento já numérico com radix `undefined`/`null`/10.
/// `Some(valor)` é o resultado (um int32 passa direto); `None` cai no caminho de string.
pub(crate) use crate::runtime::parse_int::parse_int_double as parse_int_number;

/// O atalho de `globalFuncParseFloat` para argumento já numérico: `-0` vira `+0`.
pub fn parse_float_number(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

/// `parseFloat(StringView)`.
pub fn parse_float(characters: &[u16]) -> f64 {
    if characters.len() == 1 {
        let c = characters[0];
        if is_ascii_digit(c) {
            return f64::from(c - u16::from(b'0'));
        }
        return f64::NAN;
    }

    let mut data = characters;
    skip_str_white_space(&mut data);

    // Empty string.
    if data.is_empty() {
        return f64::NAN;
    }

    js_str_decimal_literal(&mut data)
}

/// Conveniência para as cascas e os testes: o `WtfString` como unidades.
pub fn units_of(string: &WtfString) -> Vec<u16> {
    code_units(string).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_value_conversions::js_to_number_characters as js_to_number;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn text(result: Result<WtfString, GlobalFunctionError>) -> String {
        String::from_utf16(&units_of(&result.unwrap())).unwrap()
    }

    #[test]
    fn encode_cases() {
        assert_eq!(text(encode_uri_component(&units("a b&c/é"))), "a%20b%26c%2F%C3%A9");
        assert_eq!(text(encode_uri(&units("a b&c/é#"))), "a%20b&c/%C3%A9#");
        assert_eq!(text(encode_uri_component(&units("\u{1F600}"))), "%F0%9F%98%80");
        assert_eq!(encode_uri_component(&[0xD800]), Err(GlobalFunctionError::UriError(ILLEGAL_UTF16_SEQUENCE)));
        assert_eq!(encode_uri_component(&[0xDC00]), Err(GlobalFunctionError::UriError(ILLEGAL_UTF16_SEQUENCE)));
        assert_eq!(encode_uri_component(&[0xD800, 0x41]), Err(GlobalFunctionError::UriError(ILLEGAL_UTF16_SEQUENCE)));
    }

    #[test]
    fn decode_cases() {
        assert_eq!(text(decode_uri_component(&units("a%20b%26c%2F%C3%A9"))), "a b&c/é");
        assert_eq!(text(decode_uri(&units("a%20b%26c%2F"))), "a b%26c%2F");
        assert_eq!(text(decode_uri_component(&units("%F0%9F%98%80"))), "\u{1F600}");
        assert_eq!(decode_uri_component(&units("%")), Err(GlobalFunctionError::UriError(URI_ERROR)));
        assert_eq!(decode_uri_component(&units("%E0%80%80")), Err(GlobalFunctionError::UriError(URI_ERROR)));
        assert_eq!(decode_uri_component(&units("%C3")), Err(GlobalFunctionError::UriError(URI_ERROR)));
        assert_eq!(decode_uri_component(&units("%80")), Err(GlobalFunctionError::UriError(URI_ERROR)));
        assert_eq!(text(decode_uri_component(&units("%41"))), "A");
    }

    #[test]
    fn escape_unescape() {
        assert_eq!(text(escape(&units("a b\u{00E9}\u{20AC}*"))), "a%20b%E9%u20AC*");
        assert_eq!(text(unescape(&units("a%20b%E9%u20AC*"))), "a b\u{00E9}\u{20AC}*");
        assert_eq!(text(unescape(&units("%u12"))), "%u12");
        assert_eq!(text(unescape(&units("%zz%4"))), "%zz%4");
        assert_eq!(text(unescape(&units("%u00zz"))), "%u00zz");
    }

    #[test]
    fn numbers() {
        assert_eq!(parse_int_string(&units("  42px"), 10), 42.0);
        assert_eq!(parse_int_string(&units("0x1f"), 0), 31.0);
        assert_eq!(parse_float(&units("3.14abc")), 3.14);
        assert_eq!(parse_float(&units("  -Infinityx")), f64::NEG_INFINITY);
        assert!(parse_float(&units("abc")).is_nan());
        assert!(parse_float(&units("")).is_nan());
        assert_eq!(parse_float(&units("7")), 7.0);
        assert_eq!(js_to_number(&units("  12  ")), 12.0);
        assert_eq!(js_to_number(&units("0x10")), 16.0);
        assert_eq!(js_to_number(&units("0b101")), 5.0);
        assert_eq!(js_to_number(&units("0o17")), 15.0);
        assert_eq!(js_to_number(&units("")), 0.0);
        assert!(js_to_number(&units("12px")).is_nan());
        assert!(js_to_number(&units("-0")).is_sign_negative());
        assert_eq!(js_to_number(&units("+Infinity")), f64::INFINITY);
        assert!(js_to_number(&units("0x")).is_nan());
        assert_eq!(parse_float_number(-0.0).to_bits(), 0.0f64.to_bits());
        assert_eq!(parse_int_number(5.9), Some(5.0));
    }

    #[test]
    fn decode_rejects_malformed_utf8_sequences() {
        let error = Err(GlobalFunctionError::UriError(URI_ERROR));
        // Acima de U+10FFFF, surrogate codificado, byte de continuação truncado e hexadecimal incompleto.
        assert_eq!(decode_uri_component(&units("%F4%90%80%80")), error);
        assert_eq!(decode_uri_component(&units("%ED%A0%80")), error);
        assert_eq!(decode_uri_component(&units("%E2%82")), error);
        assert_eq!(decode_uri_component(&units("%2")), error);
        assert_eq!(decode_uri_component(&units("%E2%82%zz")), error);
        assert_eq!(decode_uri_component(&units("%C0%80")), error);
    }

    #[test]
    fn decode_uri_keeps_reserved_escapes_only() {
        assert_eq!(text(decode_uri(&units("%23%41%25"))), "%23A%");
        assert_eq!(text(decode_uri(&units("%3F%2B"))), "%3F%2B");
        assert_eq!(text(decode_uri_component(&units("%23%3F%2b"))), "#?+");
        assert_eq!(text(decode_uri_component(&units("100%25"))), "100%");
    }

    #[test]
    fn escape_and_unescape_edge_cases() {
        assert_eq!(text(escape(&units("\u{100}"))), "%u0100");
        assert_eq!(text(escape(&units("\u{FF}@_"))), "%FF@_");
        assert_eq!(text(unescape(&units("%u0041%41"))), "AA");
        assert_eq!(text(unescape(&units("100%"))), "100%");
        assert_eq!(text(unescape(&units("%4"))), "%4");
        assert_eq!(text(unescape(&units("%u004"))), "%u004");
        assert_eq!(text(unescape(&units("%%41"))), "%A");
    }

    #[test]
    fn parse_int_large_and_signed_values() {
        assert_eq!(parse_int_string(&units("9007199254740993"), 10), 9007199254740992.0);
        assert_eq!(parse_int_string(&units("123456789012345678901234567890"), 10), 1.2345678901234568e29);
        assert_eq!(parse_int_string(&units("0b11"), 0), 0.0);
        assert_eq!(parse_int_string(&units("  -42"), 0), -42.0);
        assert!(parse_int_string(&units("-0"), 10).is_sign_negative());
        assert!(parse_int_string(&units("  "), 10).is_nan());
        assert!(parse_int_string(&units("0x"), 0).is_nan());
        assert_eq!(parse_int_number(-0.0), Some(0.0));
        assert_eq!(parse_int_number(-0.5), None);
        assert_eq!(parse_int_number(1e-7), None);
        assert_eq!(parse_int_number(-3.9), Some(-3.0));
        assert_eq!(parse_int_number(1e21), None);
    }

    #[test]
    fn parse_float_prefix_rules() {
        assert_eq!(parse_float(&units(".5")), 0.5);
        assert_eq!(parse_float(&units("-.5e-2")), -0.005);
        assert_eq!(parse_float(&units("1e3x")), 1000.0);
        assert_eq!(parse_float(&units("+Infinityx")), f64::INFINITY);
        assert!(parse_float(&units("Infinit")).is_nan());
        assert_eq!(parse_float(&units("0x10")), 0.0);
        assert_eq!(parse_float(&units("\u{FEFF}\n 12.5e1")), 125.0);
    }

    #[test]
    fn to_number_string_grammar() {
        assert_eq!(js_to_number(&units("Infinity")), f64::INFINITY);
        assert_eq!(js_to_number(&units("-Infinity")), f64::NEG_INFINITY);
        assert!(js_to_number(&units("infinity")).is_nan());
        assert_eq!(js_to_number(&units("1e400")), f64::INFINITY);
        assert!(js_to_number(&units("0x1G")).is_nan());
        assert!(js_to_number(&units("0b2")).is_nan());
        assert!(js_to_number(&units("1_0")).is_nan());
        assert!(js_to_number(&units("1e")).is_nan());
        assert!(js_to_number(&units(".")).is_nan());
        assert!(js_to_number(&units("-0x10")).is_nan());
        assert_eq!(js_to_number(&units("  \n5\t ")), 5.0);
        assert_eq!(js_to_number(&units(".5")), 0.5);
        assert_eq!(js_to_number(&units("5.")), 5.0);
        assert_eq!(js_to_number(&units("+.5e1")), 5.0);
        assert_eq!(js_to_number(&units("-7")), -7.0);
        assert!(js_to_number(&units("-x")).is_nan());
        assert_eq!(js_to_number(&units("0XfF")), 255.0);
        assert_eq!(js_to_number(&units("0B11")), 3.0);
        assert_eq!(js_to_number(&units("0o7")), 7.0);
        assert_eq!(js_to_number(&units("0x20000000000001")), 9007199254740992.0);
    }
}
