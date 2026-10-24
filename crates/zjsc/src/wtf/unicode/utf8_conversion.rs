//! Porte de `WTF/wtf/unicode/UTF8Conversion.{h,cpp}`.
//!
//! Tipos: `Latin1Character` e `char8_t` são `u8`, `char16_t` é `u16`, `char32_t` é `u32`. Como
//! `Latin1Character` e `char8_t` coincidem em Rust, as sobrecargas e especializações do C++ viram
//! funções com o par de tipos no nome.
//!
//! As macros do ICU que o `.cpp` usa (`U8_NEXT_SPAN`, `U8_NEXT_OR_FFFD_SPAN`, `U16_NEXT`,
//! `U16_NEXT_OR_FFFD`, `U8_APPEND`, `U16_APPEND`, `U_IS_SURROGATE`) são traduzidas como funções
//! privadas com a semântica do ICU 76 (`unicode/utf8.h` e `unicode/utf16.h`). O `simdutf`, que no
//! C++ só acelera os casos que dão o mesmo resultado do laço escalar (`convert` de UTF-16 para
//! UTF-8, que cai no `convertInternal` em qualquer erro) ou que valida UTF-8 (`checkUTF8`), é
//! substituído por equivalentes escalares.

use crate::wtf::ascii_ctype::is_ascii;

/// `U_SENTINEL` do ICU (`-1` como `UChar32`), visto como `char32_t`.
const SENTINEL_CODE_POINT: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ConversionResultCode {
    /// Conversão bem-sucedida.
    Success,
    /// A sequência de origem é inválida ou malformada.
    SourceInvalid,
    /// Não há espaço suficiente no destino para a conversão.
    TargetExhausted,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ConversionResult<'a, CharacterType> {
    pub code: ConversionResultCode,
    pub buffer: &'a mut [CharacterType],
    pub is_all_ascii: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckedUTF8<'a> {
    pub characters: &'a [u8],
    pub length_utf16: usize,
    pub is_all_ascii: bool,
}

// ---------------------------------------------------------------------------------------------
// Macros do ICU.
// ---------------------------------------------------------------------------------------------

/// `U_IS_SURROGATE(c)`.
fn u_is_surrogate(c: u32) -> bool {
    (c & 0xfffff800) == 0xd800
}

/// `U16_IS_LEAD(c)`.
pub fn u16_is_lead(c: u32) -> bool {
    (c & 0xfffffc00) == 0xd800
}

/// `U16_IS_TRAIL(c)`.
pub fn u16_is_trail(c: u32) -> bool {
    (c & 0xfffffc00) == 0xdc00
}

/// `U16_IS_SURROGATE_LEAD(c)`.
fn u16_is_surrogate_lead(c: u32) -> bool {
    (c & 0x400) == 0
}

/// `U16_GET_SUPPLEMENTARY(lead, trail)`.
pub fn u16_get_supplementary(lead: u32, trail: u32) -> u32 {
    (lead << 10)
        .wrapping_add(trail)
        .wrapping_sub((0xd800 << 10) + 0xdc00 - 0x10000)
}

/// `U8_LEAD3_T1_BITS`.
const U8_LEAD3_T1_BITS: [u8; 16] = [
    0x20, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x10, 0x30, 0x30,
];

/// `U8_LEAD4_T1_BITS`.
const U8_LEAD4_T1_BITS: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1E, 0x0F, 0x0F, 0x0F, 0x00, 0x00, 0x00, 0x00,
];

/// `U8_INTERNAL_NEXT_OR_SUB(s, i, length, c, sub)` com `length = s.len()`. Em sequência malformada
/// devolve `sub` e deixa `i` depois dos bytes que o ICU consome: o byte inicial e cada byte de
/// continuação válido que o antecede.
fn u8_internal_next_or_sub(s: &[u8], i: &mut usize, sub: u32) -> u32 {
    let length = s.len();
    let mut c = s[*i] as u32;
    *i += 1;
    // U8_IS_SINGLE(c)
    if (c & 0x80) == 0 {
        return c;
    }
    'ill_formed: {
        if *i == length {
            break 'ill_formed;
        }
        let mut t: u32;
        if c >= 0xe0 {
            if c < 0xf0 {
                // U+0800..U+FFFF, exceto surrogates.
                c &= 0xf;
                t = s[*i] as u32;
                if (U8_LEAD3_T1_BITS[c as usize] as u32) & (1 << (t >> 5)) == 0 {
                    break 'ill_formed;
                }
                t &= 0x3f;
            } else {
                // U+10000..U+10FFFF.
                c -= 0xf0;
                if c > 4 {
                    break 'ill_formed;
                }
                t = s[*i] as u32;
                if (U8_LEAD4_T1_BITS[(t >> 4) as usize] as u32) & (1 << c) == 0 {
                    break 'ill_formed;
                }
                c = (c << 6) | (t & 0x3f);
                *i += 1;
                if *i == length {
                    break 'ill_formed;
                }
                t = s[*i].wrapping_sub(0x80) as u32;
                if t > 0x3f {
                    break 'ill_formed;
                }
            }
            // Penúltimo byte de continuação válido.
            c = (c << 6) | t;
            *i += 1;
            if *i == length {
                break 'ill_formed;
            }
        } else {
            // U+0080..U+07FF.
            if c < 0xc2 {
                break 'ill_formed;
            }
            c &= 0x1f;
        }
        // Último byte de continuação.
        let t = s[*i].wrapping_sub(0x80) as u32;
        if t > 0x3f {
            break 'ill_formed;
        }
        c = (c << 6) | t;
        *i += 1;
        return c;
    }
    sub
}

/// `U8_NEXT_SPAN(s, i, c)`: `U8_NEXT` com `U_SENTINEL` como substituto.
fn u8_next_span(s: &[u8], i: &mut usize) -> u32 {
    u8_internal_next_or_sub(s, i, SENTINEL_CODE_POINT)
}

/// `U8_NEXT_OR_FFFD_SPAN(s, i, c)`.
fn u8_next_or_fffd_span(s: &[u8], i: &mut usize) -> u32 {
    u8_internal_next_or_sub(s, i, 0xfffd)
}

/// `U16_NEXT(s, i, length, c)` com `length = s.len()`.
fn u16_next(s: &[u16], i: &mut usize) -> u32 {
    let mut c = s[*i] as u32;
    *i += 1;
    if u16_is_lead(c) && *i != s.len() {
        let c2 = s[*i] as u32;
        if u16_is_trail(c2) {
            *i += 1;
            c = u16_get_supplementary(c, c2);
        }
    }
    c
}

/// `U16_NEXT_OR_FFFD(s, i, length, c)` com `length = s.len()`.
fn u16_next_or_fffd(s: &[u16], i: &mut usize) -> u32 {
    let mut c = s[*i] as u32;
    *i += 1;
    if u_is_surrogate(c) {
        if u16_is_surrogate_lead(c) && *i != s.len() && u16_is_trail(s[*i] as u32) {
            let c2 = s[*i] as u32;
            *i += 1;
            c = u16_get_supplementary(c, c2);
        } else {
            c = 0xfffd;
        }
    }
    c
}

/// `U8_APPEND(s, i, capacity, c, isError)` com `capacity = s.len()`. Devolve `isError`.
fn u8_append(s: &mut [u8], i: &mut usize, c: u32) -> bool {
    let capacity = s.len();
    if c <= 0x7f {
        s[*i] = c as u8;
        *i += 1;
    } else if c <= 0x7ff && *i + 1 < capacity {
        s[*i] = ((c >> 6) | 0xc0) as u8;
        s[*i + 1] = ((c & 0x3f) | 0x80) as u8;
        *i += 2;
    } else if (c <= 0xd7ff || (0xe000 <= c && c <= 0xffff)) && *i + 2 < capacity {
        s[*i] = ((c >> 12) | 0xe0) as u8;
        s[*i + 1] = (((c >> 6) & 0x3f) | 0x80) as u8;
        s[*i + 2] = ((c & 0x3f) | 0x80) as u8;
        *i += 3;
    } else if 0xffff < c && c <= 0x10ffff && *i + 3 < capacity {
        s[*i] = ((c >> 18) | 0xf0) as u8;
        s[*i + 1] = (((c >> 12) & 0x3f) | 0x80) as u8;
        s[*i + 2] = (((c >> 6) & 0x3f) | 0x80) as u8;
        s[*i + 3] = ((c & 0x3f) | 0x80) as u8;
        *i += 4;
    } else {
        return true;
    }
    false
}

/// `U16_APPEND(s, i, capacity, c, isError)` com `capacity = s.len()`. Devolve `isError`.
fn u16_append(s: &mut [u16], i: &mut usize, c: u32) -> bool {
    let capacity = s.len();
    if c <= 0xffff {
        s[*i] = c as u16;
        *i += 1;
    } else if c <= 0x10ffff && *i + 1 < capacity {
        s[*i] = ((c >> 10) + 0xd7c0) as u16;
        s[*i + 1] = ((c & 0x3ff) | 0xdc00) as u16;
        *i += 2;
    } else {
        // c > 0x10ffff ou sem espaço.
        return true;
    }
    false
}

// ---------------------------------------------------------------------------------------------
// next e append (as especializações de template do C++).
// ---------------------------------------------------------------------------------------------

/// `next<Replacement::None, Latin1Character>`.
fn next_latin1(characters: &[u8], offset: &mut usize) -> u32 {
    let character = characters[*offset] as u32;
    *offset += 1;
    character
}

/// `next<Replacement::None, char8_t>`.
fn next_utf8(characters: &[u8], offset: &mut usize) -> u32 {
    let character = u8_next_span(characters, offset);
    if u_is_surrogate(character) {
        SENTINEL_CODE_POINT
    } else {
        character
    }
}

/// `next<Replacement::ReplaceInvalidSequences, char8_t>`.
fn next_utf8_replacing(characters: &[u8], offset: &mut usize) -> u32 {
    u8_next_or_fffd_span(characters, offset)
}

/// `next<Replacement::None, char16_t>`.
fn next_utf16(characters: &[u16], offset: &mut usize) -> u32 {
    let character = u16_next(characters, offset);
    if u_is_surrogate(character) {
        SENTINEL_CODE_POINT
    } else {
        character
    }
}

/// `next<Replacement::ReplaceInvalidSequences, char16_t>`.
fn next_utf16_replacing(characters: &[u16], offset: &mut usize) -> u32 {
    u16_next_or_fffd(characters, offset)
}

type NextFn<S> = fn(&[S], &mut usize) -> u32;
type AppendFn<B> = fn(&mut [B], &mut usize, u32) -> bool;

fn convert_internal<'a, S, B>(
    source: &[S],
    buffer: &'a mut [B],
    next: NextFn<S>,
    append: AppendFn<B>,
) -> ConversionResult<'a, B> {
    let mut result_code = ConversionResultCode::Success;
    let mut buffer_offset = 0;
    let mut or_all_data: u32 = 0;
    let mut source_offset = 0;
    while source_offset < source.len() {
        let character = next(source, &mut source_offset);
        if character == SENTINEL_CODE_POINT {
            result_code = ConversionResultCode::SourceInvalid;
            break;
        }
        if buffer_offset == buffer.len() {
            result_code = ConversionResultCode::TargetExhausted;
            break;
        }
        let saw_error = append(buffer, &mut buffer_offset, character);
        if saw_error {
            result_code = ConversionResultCode::TargetExhausted;
            break;
        }
        or_all_data |= character;
    }
    ConversionResult {
        code: result_code,
        buffer: &mut buffer[..buffer_offset],
        is_all_ascii: is_ascii(or_all_data),
    }
}

/// `convert(span<const char16_t>, span<char8_t>)`. O caminho rápido do `simdutf` só é tomado
/// quando o resultado coincide com o do laço escalar, que é o que roda aqui.
pub fn convert_utf16_to_utf8<'a>(source: &[u16], buffer: &'a mut [u8]) -> ConversionResult<'a, u8> {
    convert_internal(source, buffer, next_utf16, u8_append)
}

/// `convert(span<const char8_t>, span<char16_t>)`.
pub fn convert_utf8_to_utf16<'a>(source: &[u8], buffer: &'a mut [u16]) -> ConversionResult<'a, u16> {
    convert_internal(source, buffer, next_utf8, u16_append)
}

/// `convert(span<const Latin1Character>, span<char8_t>)`.
pub fn convert_latin1_to_utf8<'a>(source: &[u8], buffer: &'a mut [u8]) -> ConversionResult<'a, u8> {
    convert_internal(source, buffer, next_latin1, u8_append)
}

/// `convertReplacingInvalidSequences(span<const char16_t>, span<char8_t>)`.
pub fn convert_replacing_invalid_sequences_utf16_to_utf8<'a>(
    source: &[u16],
    buffer: &'a mut [u8],
) -> ConversionResult<'a, u8> {
    convert_internal(source, buffer, next_utf16_replacing, u8_append)
}

/// `convertReplacingInvalidSequences(span<const char8_t>, span<char16_t>)`.
pub fn convert_replacing_invalid_sequences_utf8_to_utf16<'a>(
    source: &[u8],
    buffer: &'a mut [u16],
) -> ConversionResult<'a, u16> {
    convert_internal(source, buffer, next_utf8_replacing, u16_append)
}

/// Posição do primeiro byte de uma sequência UTF-8 inválida ou incompleta, ou `source.len()` se
/// tudo é válido (o `count` do `simdutf::validate_utf8_with_errors` em caso de erro).
fn valid_utf8_prefix_length(source: &[u8]) -> usize {
    let is_trail = |index: usize| index < source.len() && (source[index] & 0xC0) == 0x80;
    let mut position = 0;
    while position < source.len() {
        let lead = source[position];
        if lead < 0x80 {
            position += 1;
            continue;
        }
        let length = match lead {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => return position,
        };
        if position + length > source.len() {
            return position;
        }
        let second = source[position + 1];
        let second_ok = match lead {
            0xE0 => (0xA0..=0xBF).contains(&second),
            0xED => (0x80..=0x9F).contains(&second),
            0xF0 => (0x90..=0xBF).contains(&second),
            0xF4 => (0x80..=0x8F).contains(&second),
            _ => (0x80..=0xBF).contains(&second),
        };
        if !second_ok {
            return position;
        }
        for offset in 2..length {
            if !is_trail(position + offset) {
                return position;
            }
        }
        position += length;
    }
    position
}

/// `simdutf::utf16_length_from_utf8` para UTF-8 válido.
fn utf16_length_from_utf8(source: &[u8]) -> usize {
    let mut length = 0;
    for &byte in source {
        if (byte & 0xC0) != 0x80 {
            length += 1;
        }
        if byte >= 0xF0 {
            length += 1;
        }
    }
    length
}

/// `checkUTF8WithoutUTF16Length`.
pub fn check_utf8_without_utf16_length(source: &[u8]) -> &[u8] {
    &source[..valid_utf8_prefix_length(source)]
}

/// `checkUTF8`.
pub fn check_utf8(source: &[u8]) -> CheckedUTF8<'_> {
    let valid_span = check_utf8_without_utf16_length(source);
    let length_utf16 = utf16_length_from_utf8(valid_span);
    CheckedUTF8 {
        characters: valid_span,
        length_utf16,
        is_all_ascii: valid_span.len() == length_utf16,
    }
}

fn equal_internal<A, B>(a: &[A], b: &[B], next_a: NextFn<A>, next_b: NextFn<B>) -> bool {
    let mut offset_a = 0;
    let mut offset_b = 0;
    while offset_a < a.len() && offset_b < b.len() {
        let character_a = next_a(a, &mut offset_a);
        // SENTINEL_CODE_POINT é U_SENTINEL (não U+FFFD): a decodificação falhou sem produzir
        // ponto de código. Duas falhas independentes não podem ser iguais.
        if character_a == SENTINEL_CODE_POINT || character_a != next_b(b, &mut offset_b) {
            return false;
        }
    }
    offset_a == a.len() && offset_b == b.len()
}

/// `equal(span<const char16_t>, span<const char8_t>)`.
pub fn equal_utf16_utf8(a: &[u16], b: &[u8]) -> bool {
    equal_internal(a, b, next_utf16, next_utf8)
}

/// `equal(span<const Latin1Character>, span<const char8_t>)`.
pub fn equal_latin1_utf8(a: &[u8], b: &[u8]) -> bool {
    equal_internal(a, b, next_latin1, next_utf8)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMOJI_UTF16: [u16; 2] = [0xD83D, 0xDE00];
    const EMOJI_UTF8: [u8; 4] = [0xF0, 0x9F, 0x98, 0x80];

    #[test]
    fn utf16_to_utf8_emoji_and_ascii() {
        let mut buffer = [0u8; 8];
        let result = convert_utf16_to_utf8(&EMOJI_UTF16, &mut buffer);
        assert_eq!(result.code, ConversionResultCode::Success);
        assert_eq!(result.buffer, &EMOJI_UTF8);
        assert!(!result.is_all_ascii);

        let mut buffer = [0u8; 8];
        let ascii: Vec<u16> = "abc".encode_utf16().collect();
        let result = convert_utf16_to_utf8(&ascii, &mut buffer);
        assert_eq!(result.code, ConversionResultCode::Success);
        assert_eq!(result.buffer, b"abc");
        assert!(result.is_all_ascii);
    }

    #[test]
    fn utf8_to_utf16_emoji() {
        let mut buffer = [0u16; 4];
        let result = convert_utf8_to_utf16(&EMOJI_UTF8, &mut buffer);
        assert_eq!(result.code, ConversionResultCode::Success);
        assert_eq!(result.buffer, &EMOJI_UTF16);
    }

    #[test]
    fn isolated_surrogate_is_invalid() {
        let mut buffer = [0u8; 8];
        let result = convert_utf16_to_utf8(&[0x61, 0xD800, 0x62], &mut buffer);
        assert_eq!(result.code, ConversionResultCode::SourceInvalid);
        assert_eq!(result.buffer, b"a");

        let mut buffer = [0u8; 8];
        let result = convert_utf16_to_utf8(&[0xDC00], &mut buffer);
        assert_eq!(result.code, ConversionResultCode::SourceInvalid);
        assert!(result.buffer.is_empty());
    }

    #[test]
    fn isolated_surrogate_is_replaced() {
        let mut buffer = [0u8; 16];
        let result =
            convert_replacing_invalid_sequences_utf16_to_utf8(&[0x61, 0xD800, 0x62, 0xDC00], &mut buffer);
        assert_eq!(result.code, ConversionResultCode::Success);
        assert_eq!(result.buffer, "a\u{FFFD}b\u{FFFD}".as_bytes());
    }

    #[test]
    fn malformed_utf8_is_invalid() {
        for source in [
            &[0x61u8, 0xC0, 0x80][..],
            &[0x61, 0x80],
            &[0x61, 0xE2, 0x82],
            &[0x61, 0xED, 0xA0, 0x80],
            &[0x61, 0xF5, 0x80, 0x80, 0x80],
            &[0x61, 0xF4, 0x90, 0x80, 0x80],
        ] {
            let mut buffer = [0u16; 8];
            let result = convert_utf8_to_utf16(source, &mut buffer);
            assert_eq!(result.code, ConversionResultCode::SourceInvalid, "{source:?}");
            assert_eq!(result.buffer, &[0x61]);
        }
    }

    #[test]
    fn malformed_utf8_is_replaced_with_icu_consumption() {
        // Truncada: E2 82 consome os dois bytes e vira um U+FFFD.
        let mut buffer = [0u16; 8];
        let result = convert_replacing_invalid_sequences_utf8_to_utf16(&[0xE2, 0x82], &mut buffer);
        assert_eq!(result.buffer, &[0xFFFD]);
        // Surrogate codificado: ED consome só o byte inicial, A0 e 80 viram um U+FFFD cada.
        let mut buffer = [0u16; 8];
        let result = convert_replacing_invalid_sequences_utf8_to_utf16(&[0xED, 0xA0, 0x80], &mut buffer);
        assert_eq!(result.buffer, &[0xFFFD, 0xFFFD, 0xFFFD]);
        // Sobrelongo C0 80: dois U+FFFD.
        let mut buffer = [0u16; 8];
        let result = convert_replacing_invalid_sequences_utf8_to_utf16(&[0xC0, 0x80], &mut buffer);
        assert_eq!(result.buffer, &[0xFFFD, 0xFFFD]);
        // F0 9F 98 truncada consome os três bytes.
        let mut buffer = [0u16; 8];
        let result = convert_replacing_invalid_sequences_utf8_to_utf16(&[0xF0, 0x9F, 0x98, 0x41], &mut buffer);
        assert_eq!(result.buffer, &[0xFFFD, 0x41]);
        // Emoji válido continua intacto.
        let mut buffer = [0u16; 8];
        let result = convert_replacing_invalid_sequences_utf8_to_utf16(&EMOJI_UTF8, &mut buffer);
        assert_eq!(result.buffer, &EMOJI_UTF16);
    }

    #[test]
    fn target_exhausted() {
        let mut buffer = [0u8; 3];
        let result = convert_utf16_to_utf8(&EMOJI_UTF16, &mut buffer);
        assert_eq!(result.code, ConversionResultCode::TargetExhausted);
        assert!(result.buffer.is_empty());

        let mut buffer = [0u16; 1];
        let result = convert_utf8_to_utf16(&EMOJI_UTF8, &mut buffer);
        assert_eq!(result.code, ConversionResultCode::TargetExhausted);
        assert!(result.buffer.is_empty());

        let mut buffer = [0u8; 2];
        let result = convert_latin1_to_utf8(b"abc", &mut buffer);
        assert_eq!(result.code, ConversionResultCode::TargetExhausted);
        assert_eq!(result.buffer, b"ab");
    }

    #[test]
    fn latin1_to_utf8() {
        let mut buffer = [0u8; 4];
        let result = convert_latin1_to_utf8(&[0x61, 0xE9], &mut buffer);
        assert_eq!(result.code, ConversionResultCode::Success);
        assert_eq!(result.buffer, &[0x61, 0xC3, 0xA9]);
        assert!(!result.is_all_ascii);
    }

    #[test]
    fn check_utf8_valid_prefix() {
        let checked = check_utf8(b"abc");
        assert_eq!(checked.characters, b"abc");
        assert_eq!(checked.length_utf16, 3);
        assert!(checked.is_all_ascii);

        let mut source = b"a".to_vec();
        source.extend_from_slice(&EMOJI_UTF8);
        source.extend_from_slice(&[0xE2, 0x82]);
        let checked = check_utf8(&source);
        assert_eq!(checked.characters.len(), 5);
        assert_eq!(checked.length_utf16, 3);
        assert!(!checked.is_all_ascii);

        assert_eq!(check_utf8_without_utf16_length(&[0x61, 0xED, 0xA0, 0x80]), &[0x61]);
        assert_eq!(check_utf8_without_utf16_length(&[0xC0, 0x80]), &[] as &[u8]);
    }

    #[test]
    fn equality() {
        let text: Vec<u16> = "aé😀".encode_utf16().collect();
        assert!(equal_utf16_utf8(&text, "aé😀".as_bytes()));
        assert!(!equal_utf16_utf8(&text, "aé".as_bytes()));
        assert!(!equal_utf16_utf8(&[0xD800], &[0xED, 0xA0, 0x80]));
        assert!(!equal_utf16_utf8(&[0x61], &[0xFF]));
        assert!(equal_latin1_utf8(&[0x61, 0xE9], "aé".as_bytes()));
        assert!(!equal_latin1_utf8(&[0x61, 0xE9], &[0x61, 0xE9]));
    }
}
