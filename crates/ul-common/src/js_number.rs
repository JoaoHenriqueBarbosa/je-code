//! O `StringToNumber` do JavaScriptCore (`jsToNumber` de `JSGlobalObjectFunctions.cpp`) e o que ele usa: o
//! `fast_float` do WTF (`parseDouble`), o `isStrWhiteSpace` e o `parseIntOverflow` de `ParseInt.h`, e o `PNaN`.
//!
//! Mora aqui porque dois portes precisam do mesmo número: o zjsc (`JSString::toNumber`, `parseFloat`) e o zbun,
//! cujo `ExprData::to_number` chama no upstream a FFI `JSC__jsToNumber` para dobrar `+"0x10"` em tempo de
//! transpilação. Os nomes são os do C++, para a ressincronização.

pub mod fast_float;

pub use fast_float::{parse_double, parse_fixed_double};

/// O parâmetro de template `CharacterType` do C++ (`LChar` ou `UChar`), na parte que os números usam. O zjsc
/// estende este trait com as operações de `StringImpl` e `StringView`.
pub trait CharType: Copy + Into<u32> + Eq + Ord + 'static {
    /// `sizeof(CharacterType)`.
    const SIZE: usize;

    /// O caractere como unidade de código de 16 bits.
    fn to_u16(self) -> u16;

    /// Constrói a partir de uma unidade de 16 bits (trunca no tipo de 8 bits, como a conversão implícita do C++
    /// onde o chamador já garantiu Latin1).
    fn from_u16(character: u16) -> Self;
}

impl CharType for u8 {
    const SIZE: usize = 1;

    fn to_u16(self) -> u16 {
        self as u16
    }

    fn from_u16(character: u16) -> Self {
        character as u8
    }
}

impl CharType for u16 {
    const SIZE: usize = 2;

    fn to_u16(self) -> u16 {
        self
    }

    fn from_u16(character: u16) -> Self {
        character
    }
}

// ---------------------------------------------------------------------------------------------
// PureNaN.h
// ---------------------------------------------------------------------------------------------

pub const PNAN_AS_BITS: u64 = 0x7ff8000000000000;

/// `PNaN`.
pub fn pnan() -> f64 {
    f64::from_bits(PNAN_AS_BITS)
}

// ---------------------------------------------------------------------------------------------
// Lexer.cpp e LexerUnicodeProperties: os espaços do ECMAScript
// ---------------------------------------------------------------------------------------------

/// `constinit const WTF::BitSet<256> whiteSpaceTable` do `Lexer.cpp`.
pub static WHITE_SPACE_TABLE: [bool; 256] = make_white_space_table();

const fn make_white_space_table() -> [bool; 256] {
    let mut table = [false; 256];
    let mut i = 0;
    while i < 256 {
        let ch = i as u8;
        table[i] = ch == b' ' || ch == b'\t' || ch == 0xB || ch == 0xC || ch == 0xA0;
        i += 1;
    }
    table
}

/// `isNonLatin1WhiteSpace`: os espaços Zs fora do Latin1, mais o BOM (U+FEFF), que o ECMAScript também conta
/// como espaço. Gerado de `LexerUnicodePropertyTables.h` (`generateLexerUnicodePropertyTables.py` sobre o `ucd/`
/// 17.0 do JavaScriptCore).
pub fn is_non_latin1_white_space(ch: u16) -> bool {
    ch == 0xFEFF || ch == 0x1680 || (0x2000..=0x200A).contains(&ch) || ch == 0x202F || ch == 0x205F || ch == 0x3000
}

/// `isStrWhiteSpace` de `ParseInt.h`: o `Lexer<T>::isWhiteSpace` mais o `Lexer<T>::isLineTerminator`.
pub fn is_str_white_space<C: CharType>(c: C) -> bool {
    // https://tc39.github.io/ecma262/#sec-tonumber-applied-to-the-string-type
    let c = c.to_u16();
    let white_space = if c < 256 { WHITE_SPACE_TABLE[c as usize] } else { is_non_latin1_white_space(c) };
    white_space || c == b'\r' as u16 || c == b'\n' as u16 || (c & !1) == 0x2028
}

// ---------------------------------------------------------------------------------------------
// ParseInt.h
// ---------------------------------------------------------------------------------------------

pub const MANTISSA_OVERFLOW_LOWER_BOUND: f64 = 9007199254740992.0;

fn ascii_byte(c: u16) -> Option<u8> {
    u8::try_from(c).ok().filter(u8::is_ascii)
}

pub fn parse_digit(c: u16, radix: i32) -> i32 {
    let digit = match ascii_byte(c) {
        Some(b @ b'0'..=b'9') => (b - b'0') as i32,
        Some(b @ b'A'..=b'Z') => (b - b'A') as i32 + 10,
        Some(b @ b'a'..=b'z') => (b - b'a') as i32 + 10,
        _ => -1,
    };
    if digit >= radix {
        return -1;
    }
    digit
}

/// As duas sobrecargas de `parseIntOverflow` (`Latin1Character` e `char16_t`).
pub fn parse_int_overflow<C: CharType>(s: &[C], radix: i32) -> f64 {
    let mut number = 0.0;
    let mut radix_multiplier = 1.0;

    for p in s.iter().rev() {
        if radix_multiplier == f64::INFINITY {
            if p.to_u16() != '0' as u16 {
                number = f64::INFINITY;
                break;
            }
        } else {
            let digit = parse_digit(p.to_u16(), radix);
            number += digit as f64 * radix_multiplier;
        }

        radix_multiplier *= radix as f64;
    }

    number
}

// ---------------------------------------------------------------------------------------------
// JSGlobalObjectFunctions.cpp: jsToNumber
// ---------------------------------------------------------------------------------------------

const SIZE_OF_INFINITY: usize = 8;

fn is_ascii_digit(c: u16) -> bool {
    ascii_byte(c).is_some_and(|b| b.is_ascii_digit())
}

fn is_ascii_hex_digit(c: u16) -> bool {
    ascii_byte(c).is_some_and(|b| b.is_ascii_hexdigit())
}

fn is_ascii_octal_digit(c: u16) -> bool {
    (b'0' as u16..=b'7' as u16).contains(&c)
}

fn is_ascii_binary_digit(c: u16) -> bool {
    c == b'0' as u16 || c == b'1' as u16
}

/// `toASCIIHexValue` do `ASCIICType.h`; o chamador já conferiu que é dígito hexadecimal.
fn to_ascii_hex_value(c: u16) -> u8 {
    let b = c as u8;
    if b.is_ascii_digit() { b - b'0' } else { (b | 0x20) - b'a' + 10 }
}

/// `skip(data, n)` de `ParsingUtilities.h`.
fn skip<'a, C>(data: &mut &'a [C], n: usize) {
    let current: &'a [C] = *data;
    *data = &current[n..];
}

/// `consume(data)` de `ParsingUtilities.h`.
fn consume<'a, C: Copy>(data: &mut &'a [C]) -> C {
    let current: &'a [C] = *data;
    *data = &current[1..];
    current[0]
}

/// `skipWhile<isStrWhiteSpace>(data)`.
pub fn skip_str_white_space<'a, C: CharType>(data: &mut &'a [C]) {
    while let Some(&c) = data.first() {
        if !is_str_white_space(c) {
            break;
        }
        skip(data, 1);
    }
}

/// `isInfinity`.
fn is_infinity<C: CharType>(data: &[C]) -> bool {
    data.len() >= SIZE_OF_INFINITY
        && b"Infinity".iter().zip(data).all(|(&expected, &actual)| actual.to_u16() == expected as u16)
}

/// `jsBinaryIntegerLiteral`, `jsOctalIntegerLiteral` e `jsHexIntegerLiteral` (ECMA-262 6th 11.8.3), que no C++
/// são três cópias com a base e o teste de dígito trocados.
fn js_radix_integer_literal<'a, C: CharType>(data: &mut &'a [C], radix: u32, is_digit: fn(u16) -> bool) -> f64 {
    skip(data, 2);
    let first_digit_position: &'a [C] = *data;
    let mut number = 0.0;
    loop {
        number = number * radix as f64 + to_ascii_hex_value(consume(data).to_u16()) as f64;
        if data.is_empty() {
            break;
        }
        if !is_digit(data[0].to_u16()) {
            break;
        }
    }
    if number >= MANTISSA_OVERFLOW_LOWER_BOUND {
        let consumed = first_digit_position.len() - data.len();
        number = parse_int_overflow(&first_digit_position[..consumed], radix as i32);
    }
    number
}

/// `jsStrDecimalLiteral` (ECMA-262 6th 11.8.3).
pub fn js_str_decimal_literal<'a, C: CharType>(data: &mut &'a [C]) -> f64 {
    // RELEASE_ASSERT(!data.empty())
    assert!(!data.is_empty());

    let mut parsed_length = 0usize;
    let current: &'a [C] = *data;
    let number = parse_double(current, &mut parsed_length);
    if parsed_length != 0 {
        skip(data, parsed_length);
        return number;
    }

    // Check for [+-]?Infinity
    match data[0].to_u16() {
        0x49 /* 'I' */ => {
            if is_infinity(&data[..]) {
                skip(data, SIZE_OF_INFINITY);
                return f64::INFINITY;
            }
        }
        0x2b /* '+' */ => {
            if is_infinity(&data[1..]) {
                skip(data, SIZE_OF_INFINITY + 1);
                return f64::INFINITY;
            }
        }
        0x2d /* '-' */ => {
            if is_infinity(&data[1..]) {
                skip(data, SIZE_OF_INFINITY + 1);
                return f64::NEG_INFINITY;
            }
        }
        _ => {}
    }

    // Not a number.
    pnan()
}

/// `toDouble(std::span<const CharacterType>)`.
fn to_double<C: CharType>(characters: &[C]) -> f64 {
    let mut characters = characters;

    // Skip leading white space.
    skip_str_white_space(&mut characters);

    // Empty string.
    if characters.is_empty() {
        return 0.0;
    }

    let number = if characters[0].to_u16() == '0' as u16 && characters.len() > 2 {
        let radix_marker = characters[1].to_u16() | 0x20;
        let first_digit = characters[2].to_u16();
        if radix_marker == 'x' as u16 && is_ascii_hex_digit(first_digit) {
            js_radix_integer_literal(&mut characters, 16, is_ascii_hex_digit)
        } else if radix_marker == 'o' as u16 && is_ascii_octal_digit(first_digit) {
            js_radix_integer_literal(&mut characters, 8, is_ascii_octal_digit)
        } else if radix_marker == 'b' as u16 && is_ascii_binary_digit(first_digit) {
            js_radix_integer_literal(&mut characters, 2, is_ascii_binary_digit)
        } else {
            js_str_decimal_literal(&mut characters)
        }
    } else {
        js_str_decimal_literal(&mut characters)
    };

    // Allow trailing white space.
    skip_str_white_space(&mut characters);

    if !characters.is_empty() {
        return pnan();
    }

    number
}

/// `jsToNumber(std::span<const CharacterType>)` (ECMA-262 6th 11.8.3).
pub fn js_to_number_characters<C: CharType>(characters: &[C]) -> f64 {
    if characters.len() == 1 {
        let c = characters[0].to_u16();
        if is_ascii_digit(c) {
            return (c - '0' as u16) as f64;
        }
        if is_str_white_space(characters[0]) {
            return 0.0;
        }
        return pnan();
    }

    if characters.len() == 2 && characters[0].to_u16() == '-' as u16 {
        let c = characters[1].to_u16();
        if c == '0' as u16 {
            return -0.0;
        }
        if is_ascii_digit(c) {
            return -((c - '0' as u16) as i32) as f64;
        }
        return pnan();
    }

    to_double(characters)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(text: &str) -> f64 {
        js_to_number_characters(text.as_bytes())
    }

    fn number16(text: &str) -> f64 {
        js_to_number_characters(&text.encode_utf16().collect::<Vec<u16>>())
    }

    #[test]
    fn empty_and_white_space_are_zero() {
        assert_eq!(number(""), 0.0);
        assert_eq!(number(" "), 0.0);
        assert_eq!(number(" \t\n\r\u{b}\u{c}"), 0.0);
        assert_eq!(number16("\u{a0}\u{feff}\u{2028}\u{3000}"), 0.0);
    }

    #[test]
    fn surrounding_white_space_is_skipped() {
        assert_eq!(number("  42  "), 42.0);
        assert_eq!(number("\n1.5\t"), 1.5);
        assert_eq!(number16("\u{2003}7\u{2029}"), 7.0);
        assert!(number("4 2").is_nan());
    }

    #[test]
    fn negative_zero_keeps_its_sign() {
        assert!(number("-0").is_sign_negative());
        assert_eq!(number("-0"), 0.0);
        assert!(number(" -0 ").is_sign_negative());
        assert!(number("-0.0").is_sign_negative());
        assert!(!number("0").is_sign_negative());
    }

    #[test]
    fn radix_prefixes() {
        assert_eq!(number("0x10"), 16.0);
        assert_eq!(number("0XfF"), 255.0);
        assert_eq!(number("0o17"), 15.0);
        assert_eq!(number("0B101"), 5.0);
        assert!(number("0x").is_nan());
        assert!(number("0xg").is_nan());
        assert!(number("-0x10").is_nan());
        assert!(number("0b2").is_nan());
        assert_eq!(number("0x20000000000001"), 9007199254740993.0_f64);
    }

    #[test]
    fn infinity() {
        assert_eq!(number("Infinity"), f64::INFINITY);
        assert_eq!(number("+Infinity"), f64::INFINITY);
        assert_eq!(number("-Infinity"), f64::NEG_INFINITY);
        assert_eq!(number(" Infinity\n"), f64::INFINITY);
        assert!(number("infinity").is_nan());
        assert!(number("Infinit").is_nan());
        assert!(number("Infinityx").is_nan());
    }

    #[test]
    fn decimals_and_garbage() {
        assert_eq!(number("1e3"), 1000.0);
        assert_eq!(number(".5"), 0.5);
        assert_eq!(number("5."), 5.0);
        assert_eq!(number("+1"), 1.0);
        assert_eq!(number("-7"), -7.0);
        assert!(number("abc").is_nan());
        assert!(number("1_000").is_nan());
        assert!(number("NaN").is_nan());
        assert!(number("-").is_nan());
        assert_eq!(number("9"), 9.0);
        assert!(number("x").is_nan());
    }
}
