//! Porte de `WTF/wtf/ASCIICType.h` e `ASCIICType.cpp`.
//!
//! O comportamento das funções do `<ctype.h>` depende do locale. Estas equivalentes não dependem,
//! e todas devolvem `false` (ou deixam o caractere intacto) para um caractere fora de 0-7F, então
//! servem para texto Unicode quando a intenção é processar só o que é ASCII.
//!
//! Os templates sobre `CharacterType` viram funções genéricas sobre o trait `AsciiChar`. A
//! aritmética é feita em `u32`; `from_u32` trunca para a largura do tipo, como a conversão
//! implícita do C++ no retorno `CharacterType`.

/// Tipo de caractere aceito pelas funções (o `concept Character` do C++).
pub trait AsciiChar: Copy {
    fn to_u32(self) -> u32;
    fn from_u32(v: u32) -> Self;
}

impl AsciiChar for u8 {
    fn to_u32(self) -> u32 {
        self as u32
    }
    fn from_u32(v: u32) -> Self {
        v as u8
    }
}

impl AsciiChar for u16 {
    fn to_u32(self) -> u32 {
        self as u32
    }
    fn from_u32(v: u32) -> Self {
        v as u16
    }
}

impl AsciiChar for u32 {
    fn to_u32(self) -> u32 {
        self
    }
    fn from_u32(v: u32) -> Self {
        v
    }
}

impl AsciiChar for char {
    fn to_u32(self) -> u32 {
        self as u32
    }
    fn from_u32(v: u32) -> Self {
        // As operações deste módulo só mexem no bit 0x20 ou o limpam em letras ASCII, então o
        // resultado de um `char` válido é sempre um `char` válido.
        char::from_u32(v).unwrap_or('\u{FFFD}')
    }
}

/// `WTF::asciiCaseFoldTable`, copiada byte a byte.
pub const ASCII_CASE_FOLD_TABLE: [u8; 256] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
    0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f,
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f,
    0x40, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f,
    0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x5b, 0x5c, 0x5d, 0x5e, 0x5f,
    0x60, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6a, 0x6b, 0x6c, 0x6d, 0x6e, 0x6f,
    0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7a, 0x7b, 0x7c, 0x7d, 0x7e, 0x7f,
    0x80, 0x81, 0x82, 0x83, 0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8a, 0x8b, 0x8c, 0x8d, 0x8e, 0x8f,
    0x90, 0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9a, 0x9b, 0x9c, 0x9d, 0x9e, 0x9f,
    0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0xa9, 0xaa, 0xab, 0xac, 0xad, 0xae, 0xaf,
    0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb, 0xbc, 0xbd, 0xbe, 0xbf,
    0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf,
    0xd0, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7, 0xd8, 0xd9, 0xda, 0xdb, 0xdc, 0xdd, 0xde, 0xdf,
    0xe0, 0xe1, 0xe2, 0xe3, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xe9, 0xea, 0xeb, 0xec, 0xed, 0xee, 0xef,
    0xf0, 0xf1, 0xf2, 0xf3, 0xf4, 0xf5, 0xf6, 0xf7, 0xf8, 0xf9, 0xfa, 0xfb, 0xfc, 0xfd, 0xfe, 0xff,
];

pub fn is_ascii<C: AsciiChar>(character: C) -> bool {
    (character.to_u32() & !0x7F) == 0
}

pub fn is_ascii_lower<C: AsciiChar>(character: C) -> bool {
    let c = character.to_u32();
    c >= 'a' as u32 && c <= 'z' as u32
}

/// Serve para comparar qualquer caractere com uma letra inglesa minúscula.
pub fn to_ascii_lower_unchecked<C: AsciiChar>(character: C) -> C {
    C::from_u32(character.to_u32() | 0x20)
}

pub fn is_ascii_alpha<C: AsciiChar>(character: C) -> bool {
    is_ascii_lower(to_ascii_lower_unchecked(character))
}

pub fn is_ascii_digit<C: AsciiChar>(character: C) -> bool {
    let c = character.to_u32();
    c >= '0' as u32 && c <= '9' as u32
}

pub fn is_ascii_alphanumeric<C: AsciiChar>(character: C) -> bool {
    is_ascii_digit(character) || is_ascii_alpha(character)
}

pub fn is_ascii_hex_digit<C: AsciiChar>(character: C) -> bool {
    is_ascii_digit(character)
        || (to_ascii_lower_unchecked(character).to_u32() >= 'a' as u32
            && to_ascii_lower_unchecked(character).to_u32() <= 'f' as u32)
}

pub fn is_ascii_binary_digit<C: AsciiChar>(character: C) -> bool {
    let c = character.to_u32();
    c == '0' as u32 || c == '1' as u32
}

pub fn is_ascii_octal_digit<C: AsciiChar>(character: C) -> bool {
    let c = character.to_u32();
    c >= '0' as u32 && c <= '7' as u32
}

pub fn is_ascii_printable<C: AsciiChar>(character: C) -> bool {
    let c = character.to_u32();
    c >= ' ' as u32 && c <= '~' as u32
}

pub fn is_ascii_graphic<C: AsciiChar>(character: C) -> bool {
    let c = character.to_u32();
    c >= '!' as u32 && c <= '~' as u32
}

pub fn is_tab_or_space<C: AsciiChar>(character: C) -> bool {
    let c = character.to_u32();
    c == ' ' as u32 || c == '\t' as u32
}

/// O "ASCII whitespace" da Infra (https://infra.spec.whatwg.org/#ascii-whitespace).
pub fn is_ascii_whitespace<C: AsciiChar>(character: C) -> bool {
    let c = character.to_u32();
    c == ' ' as u32 || c == '\n' as u32 || c == '\t' as u32 || c == '\r' as u32 || c == 0x0C
}

/// Diferente de `is_ascii_whitespace`: JSON, HTTP e XML não aceitam `\f` como espaço em branco.
pub fn is_ascii_whitespace_without_ff<C: AsciiChar>(character: C) -> bool {
    let c = character.to_u32();
    c == ' ' as u32 || c == '\n' as u32 || c == '\t' as u32 || c == '\r' as u32
}

pub fn is_unicode_compatible_ascii_whitespace<C: AsciiChar>(character: C) -> bool {
    is_ascii_whitespace(character) || character.to_u32() == 0x0B
}

pub fn is_ascii_upper<C: AsciiChar>(character: C) -> bool {
    let c = character.to_u32();
    c >= 'A' as u32 && c <= 'Z' as u32
}

/// Inverso de `is_ascii_whitespace`, para predicados.
pub fn is_not_ascii_whitespace<C: AsciiChar>(character: C) -> bool {
    !is_ascii_whitespace(character)
}

pub fn to_ascii_lower<C: AsciiChar>(character: C) -> C {
    C::from_u32(character.to_u32() | ((is_ascii_upper(character) as u32) << 5))
}

/// A especialização do C++ para `char` e `Latin1Character`: consulta a tabela.
pub fn to_ascii_lower_latin1(character: u8) -> u8 {
    ASCII_CASE_FOLD_TABLE[character as usize]
}

pub fn to_ascii_upper<C: AsciiChar>(character: C) -> C {
    C::from_u32(character.to_u32() & !((is_ascii_lower(character) as u32) << 5))
}

pub fn to_ascii_hex_value<C: AsciiChar>(character: C) -> u8 {
    debug_assert!(is_ascii_hex_digit(character));
    let c = character.to_u32();
    if c < 'A' as u32 {
        c.wrapping_sub('0' as u32) as u8
    } else {
        (c.wrapping_sub('A' as u32).wrapping_add(10) & 0xF) as u8
    }
}

pub fn to_ascii_hex_value_pair<C: AsciiChar>(first_character: C, second_character: C) -> u8 {
    to_ascii_hex_value(first_character) << 4 | to_ascii_hex_value(second_character)
}

pub const fn lower_nibble_to_ascii_hex_digit(value: u8) -> u8 {
    let nibble = value & 0xF;
    nibble + if nibble < 10 { b'0' } else { b'A' - 10 }
}

pub const fn upper_nibble_to_ascii_hex_digit(value: u8) -> u8 {
    let nibble = value >> 4;
    nibble + if nibble < 10 { b'0' } else { b'A' - 10 }
}

pub const fn lower_nibble_to_lowercase_ascii_hex_digit(value: u8) -> u8 {
    let nibble = value & 0xF;
    nibble + if nibble < 10 { b'0' } else { b'a' - 10 }
}

pub const fn upper_nibble_to_lowercase_ascii_hex_digit(value: u8) -> u8 {
    let nibble = value >> 4;
    nibble + if nibble < 10 { b'0' } else { b'a' - 10 }
}

/// O nome do argumento diz minúscula, mas ele pode ser letra minúscula, dígito, espaço ou
/// pontuação na faixa 0x21-0x3F. Não pode ser maiúscula, não ASCII, outra pontuação nem controle.
pub fn is_ascii_alpha_caseless_equal<C: AsciiChar>(
    input_character: C,
    expected_ascii_lowercase_letter: u8,
) -> bool {
    debug_assert!(
        to_ascii_lower_unchecked(expected_ascii_lowercase_letter) == expected_ascii_lowercase_letter
    );
    to_ascii_lower_unchecked(input_character).to_u32()
        == C::from_u32(expected_ascii_lowercase_letter as u32).to_u32()
}

pub fn is_ascii_digit_or_punctuation<C: AsciiChar>(character: C) -> bool {
    let c = character.to_u32();
    (c >= '!' as u32 && c <= '@' as u32)
        || (c >= '[' as u32 && c <= '`' as u32)
        || (c >= '{' as u32 && c <= '~' as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification() {
        assert!(is_ascii(b'a'));
        assert!(!is_ascii(0x80u8));
        assert!(!is_ascii(0x100u16));
        assert!(is_ascii_alpha('Z'));
        assert!(!is_ascii_alpha('['));
        assert!(!is_ascii_alpha(0x140u16));
        assert!(is_ascii_digit(b'7'));
        assert!(is_ascii_hex_digit(b'f') && is_ascii_hex_digit(b'F') && !is_ascii_hex_digit(b'g'));
        assert!(is_ascii_binary_digit(b'1') && !is_ascii_binary_digit(b'2'));
        assert!(is_ascii_octal_digit(b'7') && !is_ascii_octal_digit(b'8'));
        assert!(is_ascii_printable(b' ') && !is_ascii_printable(0x7Fu8));
        assert!(is_ascii_graphic(b'!') && !is_ascii_graphic(b' '));
        assert!(is_ascii_whitespace(0x0Cu8) && !is_ascii_whitespace_without_ff(0x0Cu8));
        assert!(is_unicode_compatible_ascii_whitespace(0x0Bu16));
        assert!(!is_ascii_whitespace(0x0Bu16));
        assert!(is_not_ascii_whitespace(b'a'));
        assert!(is_tab_or_space(b'\t'));
        assert!(is_ascii_digit_or_punctuation(b'@') && is_ascii_digit_or_punctuation(b'~'));
        assert!(!is_ascii_digit_or_punctuation(b'A'));
    }

    #[test]
    fn case_conversion() {
        assert_eq!(to_ascii_lower(b'A'), b'a');
        assert_eq!(to_ascii_lower(0xC0u8), 0xC0);
        assert_eq!(to_ascii_upper(b'z'), b'Z');
        assert_eq!(to_ascii_upper(0x161u16), 0x161);
        assert_eq!(to_ascii_lower('Q'), 'q');
        for c in 0..=255u8 {
            assert_eq!(to_ascii_lower_latin1(c), to_ascii_lower(c));
        }
        assert_eq!(to_ascii_lower_unchecked(b'@'), b'`');
        assert!(is_ascii_alpha_caseless_equal(b'S', b's'));
        assert!(!is_ascii_alpha_caseless_equal(b'T', b's'));
    }

    #[test]
    fn hex() {
        assert_eq!(to_ascii_hex_value(b'9'), 9);
        assert_eq!(to_ascii_hex_value(b'a'), 10);
        assert_eq!(to_ascii_hex_value(b'F'), 15);
        assert_eq!(to_ascii_hex_value_pair(b'f', b'0'), 0xF0);
        assert_eq!(lower_nibble_to_ascii_hex_digit(0xAB), b'B');
        assert_eq!(upper_nibble_to_ascii_hex_digit(0xAB), b'A');
        assert_eq!(lower_nibble_to_lowercase_ascii_hex_digit(0xAB), b'b');
        assert_eq!(upper_nibble_to_lowercase_ascii_hex_digit(0x3B), b'3');
    }
}
