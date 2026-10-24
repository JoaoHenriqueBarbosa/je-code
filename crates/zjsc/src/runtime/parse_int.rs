//! Tradução de `runtime/ParseInt.h`.
//!
//! Não portado aqui, por depender de camadas ainda não existentes:
//!
//! - `parseInt(StringView, int radix)`: escolhe `s.span8()`/`s.span16()` e chama `parse_int`; entra
//!   quando o `StringView` for portado (o chamador passa o span direto, que é o que o C++ faz).
//! - `toStringView(JSGlobalObject*, JSValue, callback)`: usa `JSValue::toStringOrNull`,
//!   `JSString::view`, `DECLARE_THROW_SCOPE` e `RETURN_IF_EXCEPTION`; vive na camada do `JSValue`
//!   e do `VM`, junto de `JSString`.

use crate::runtime::math_common::max_safe_integer;
use crate::wtf::ascii_ctype::is_ascii_digit;
use crate::wtf::fast_float::parse_double;
use crate::wtf::text::string_impl::CharType;

pub use ul_common::js_number::{is_str_white_space, parse_digit, parse_int_overflow, MANTISSA_OVERFLOW_LOWER_BOUND};

pub fn parse_int_double(n: f64) -> Option<f64> {
    // Optimized handling for numbers:
    // If the argument is 0 or a number in range 10^-6 <= n < maxSafeInteger, then parseInt
    // results in a truncation to integer. In the case of -0, this is converted to 0.
    //
    // This is also a truncation for values in the range maxSafeInteger <= n < 10^21,
    // however these values cannot be trivially truncated to int since 10^21 exceeds
    // even the int64_t range. Negative numbers are a little trickier, the case for
    // values in the range -10^21 < n <= -1 are similar to those for integer, but
    // values in the range -1 < n <= -10^-6 need to truncate to -0, not 0.
    const TEN_TO_THE_MINUS_6: f64 = 0.000001;
    if n == 0.0 {
        return Some(0.0);
    }
    const { assert!(max_safe_integer() < 1e+21) };
    const { assert!(max_safe_integer() < MANTISSA_OVERFLOW_LOWER_BOUND) };
    if n.abs() <= max_safe_integer() && (n >= TEN_TO_THE_MINUS_6 || n <= -1.0) {
        return Some(n.trunc());
    }
    None
}

// ES5.1 15.1.2.2
pub fn parse_int<C: CharType>(data: &[C], radix: i32) -> f64 {
    let mut radix = radix;
    const NUMBER_OF_DIGITS_FOR_SAFE_INT32: usize = 9;

    let length = data.len();

    if (radix == 10 || radix == 0) && length != 0 && length <= NUMBER_OF_DIGITS_FOR_SAFE_INT32 {
        let first = data[0].to_u16();
        if first >= '1' as u16 && first <= '9' as u16 {
            let mut int_number: i32 = first as i32 - '0' as i32;
            for c in &data[1..length] {
                let c = c.to_u16();
                if !is_ascii_digit(c) {
                    return int_number as f64;
                }
                int_number = int_number * 10 + (c as i32 - '0' as i32);
            }
            return int_number as f64;
        }
        if first == '0' as u16 && length == 1 {
            return 0.0;
        }
    }

    // 1. Let inputString be ToString(string).
    // 2. Let S be a newly created substring of inputString consisting of the first character that is not a
    //    StrWhiteSpaceChar and all characters following that character. (In other words, remove leading white
    //    space.) If inputString does not contain any such characters, let S be the empty string.
    let mut p: usize = 0;
    while p < length && is_str_white_space(data[p]) {
        p += 1;
    }

    // 3. Let sign be 1.
    // 4. If S is not empty and the first character of S is a minus sign -, let sign be -1.
    // 5. If S is not empty and the first character of S is a plus sign + or a minus sign -, then remove the first character from S.
    let mut sign: f64 = 1.0;
    if p < length {
        if data[p].to_u16() == '+' as u16 {
            p += 1;
        } else if data[p].to_u16() == '-' as u16 {
            sign = -1.0;
            p += 1;
        }
    }

    // 6. Let R = ToInt32(radix).
    // 7. Let stripPrefix be true.
    // 8. If R != 0,then
    //   b. If R != 16, let stripPrefix be false.
    // 9. Else, R == 0
    //   a. LetR = 10.
    // 10. If stripPrefix is true, then
    //   a. If the length of S is at least 2 and the first two characters of S are either ―0x or ―0X,
    //      then remove the first two characters from S and let R = 16.
    // 11. If S contains any character that is not a radix-R digit, then let Z be the substring of S
    //     consisting of all characters before the first such character; otherwise, let Z be S.
    if (radix == 0 || radix == 16)
        && length - p >= 2
        && data[p].to_u16() == '0' as u16
        && (data[p + 1].to_u16() == 'x' as u16 || data[p + 1].to_u16() == 'X' as u16)
    {
        radix = 16;
        p += 2;
    } else if radix == 0 {
        radix = 10;
    }

    // 8.a If R < 2 or R > 36, then return NaN.
    if !(2..=36).contains(&radix) {
        return f64::NAN;
    }

    if radix == 10 {
        let first_digit_position = p;
        let mut int_number: i32 = 0;
        let int_end = length.min(p + NUMBER_OF_DIGITS_FOR_SAFE_INT32);
        while p < int_end && is_ascii_digit(data[p].to_u16()) {
            int_number = int_number * 10 + (data[p].to_u16() as i32 - '0' as i32);
            p += 1;
        }
        if p == first_digit_position {
            return f64::NAN;
        }
        if p == length || !is_ascii_digit(data[p].to_u16()) {
            return sign * int_number as f64;
        }
        let mut number = int_number as f64;
        loop {
            number = number * 10.0 + (data[p].to_u16() as i32 - '0' as i32) as f64;
            p += 1;
            if !(p < length && is_ascii_digit(data[p].to_u16())) {
                break;
            }
        }
        if number >= MANTISSA_OVERFLOW_LOWER_BOUND {
            let mut parsed_length: usize = 0;
            number = parse_double(&data[first_digit_position..p], &mut parsed_length);
        }
        return sign * number;
    }

    // 13. Let mathInt be the mathematical integer value that is represented by Z in radix-R notation, using the letters
    //     A-Z and a-z for digits with values 10 through 35. (However, if R is 10 and Z contains more than 20 significant
    //     digits, every significant digit after the 20th may be replaced by a 0 digit, at the option of the implementation;
    //     and if R is not 2, 4, 8, 10, 16, or 32, then mathInt may be an implementation-dependent approximation to the
    //     mathematical integer value that is represented by Z in radix-R notation.)
    // 14. Let number be the Number value for mathInt.
    let first_digit_position = p;
    let mut saw_digit = false;
    let mut number: f64 = 0.0;
    while p < length {
        let digit = parse_digit(data[p].to_u16(), radix);
        if digit == -1 {
            break;
        }
        saw_digit = true;
        number *= radix as f64;
        number += digit as f64;
        p += 1;
    }

    // 12. If Z is empty, return NaN.
    if !saw_digit {
        return f64::NAN;
    }

    // Alternate code path for certain large numbers.
    if number >= MANTISSA_OVERFLOW_LOWER_BOUND && matches!(radix, 2 | 4 | 8 | 16 | 32) {
        number = parse_int_overflow(&data[first_digit_position..p], radix);
    }

    // 15. Return sign x number.
    sign * number
}

// Mapping from integers 0..35 to digit identifying this value, for radix 2..36.
// (O `char[37]` do C++ inclui o terminador nulo.)
pub const RADIX_DIGITS: [u8; 37] = *b"0123456789abcdefghijklmnopqrstuvwxyz\0";

#[cfg(test)]
mod tests {
    use super::*;

    fn int8(text: &str, radix: i32) -> f64 {
        parse_int(text.as_bytes(), radix)
    }

    fn int16(text: &str, radix: i32) -> f64 {
        let units: Vec<u16> = text.encode_utf16().collect();
        parse_int(&units, radix)
    }

    #[test]
    fn radix_16_and_36() {
        assert_eq!(int8("ff", 16), 255.0);
        assert_eq!(int8("FF", 16), 255.0);
        assert_eq!(int8("0xff", 16), 255.0);
        assert_eq!(int8("0XFf", 0), 255.0);
        assert_eq!(int8("zz", 36), 1295.0);
        assert_eq!(int8("Zz", 36), 1295.0);
        assert_eq!(int8("-z", 36), -35.0);
        assert_eq!(int16("zz", 36), 1295.0);
        assert_eq!(int8("12", 2), 1.0);
        assert!(int8("0x", 16).is_nan());
        assert!(int8("g", 16).is_nan());
        assert_eq!(int8("0x10", 10), 0.0);
    }

    #[test]
    fn invalid_radix() {
        assert!(int8("10", 1).is_nan());
        assert!(int8("10", 37).is_nan());
        assert!(int8("10", -2).is_nan());
    }

    #[test]
    fn decimal() {
        assert_eq!(int8("123", 0), 123.0);
        assert_eq!(int8("007", 10), 7.0);
        assert_eq!(int8("0", 0), 0.0);
        assert_eq!(int8("  -42abc", 10), -42.0);
        assert_eq!(int8("+8", 0), 8.0);
        assert_eq!(int8("2147483648", 10), 2147483648.0);
        assert_eq!(int8("123456789012", 10), 123456789012.0);
        assert!(int8("", 10).is_nan());
        assert!(int8("   ", 10).is_nan());
        assert!(int8("abc", 10).is_nan());
        assert!(int8("-", 10).is_nan());
        assert!(int8("-0", 10).is_sign_negative());
        assert!(int8("-0", 10) == 0.0);
        assert_eq!(int16("\u{00A0}\u{FEFF}\u{2003}12", 10), 12.0);
        assert_eq!(int8("\t\n 5", 10), 5.0);
    }

    #[test]
    fn overflow_to_double() {
        assert_eq!(int8("1000000000000000000000", 10), 1e21);
        assert_eq!(int8("-1000000000000000000000", 10), -1e21);
        assert_eq!(int8("ffffffffffffffffff", 16), 2f64.powi(72));
        assert_eq!(int8("0xffffffffffffffffff", 0), 2f64.powi(72));
        assert_eq!(int16("1000000000000000000000", 10), 1e21);
        assert_eq!(parse_int_overflow(b"ffffffffffffffffff", 16), 2f64.powi(72));
        let long_zero = format!("1{}", "0".repeat(400));
        assert_eq!(int8(&long_zero, 16), f64::INFINITY);
        assert_eq!(parse_int_overflow(long_zero.as_bytes(), 16), f64::INFINITY);
    }

    #[test]
    fn digit_and_whitespace() {
        assert_eq!(parse_digit('7' as u16, 10), 7);
        assert_eq!(parse_digit('a' as u16, 16), 10);
        assert_eq!(parse_digit('A' as u16, 16), 10);
        assert_eq!(parse_digit('a' as u16, 10), -1);
        assert_eq!(parse_digit('z' as u16, 36), 35);
        assert_eq!(parse_digit('-' as u16, 36), -1);
        assert!(is_str_white_space(b' '));
        assert!(is_str_white_space(b'\n'));
        assert!(is_str_white_space(0xA0u8));
        assert!(!is_str_white_space(b'a'));
        assert!(is_str_white_space(0x2028u16));
        assert!(is_str_white_space(0xFEFFu16));
    }

    #[test]
    fn double_shortcut() {
        assert_eq!(parse_int_double(0.0), Some(0.0));
        assert!(parse_int_double(-0.0).unwrap().is_sign_positive());
        assert_eq!(parse_int_double(5.9), Some(5.0));
        assert_eq!(parse_int_double(-5.9), Some(-5.0));
        assert_eq!(parse_int_double(0.000001), Some(0.0));
        assert_eq!(parse_int_double(0.0000001), None);
        assert_eq!(parse_int_double(-0.5), None);
        assert_eq!(parse_int_double(9007199254740991.0), Some(9007199254740991.0));
        assert_eq!(parse_int_double(9007199254740992.0), None);
        assert_eq!(parse_int_double(f64::NAN), None);
    }

    #[test]
    fn radix_digits_table() {
        assert_eq!(RADIX_DIGITS[0], b'0');
        assert_eq!(RADIX_DIGITS[35], b'z');
        assert_eq!(RADIX_DIGITS[36], 0);
    }
}
