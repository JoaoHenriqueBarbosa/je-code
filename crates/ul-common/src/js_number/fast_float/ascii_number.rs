//! Porte de `WTF/wtf/fast_float/fast_float.h`, seção `FASTFLOAT_ASCII_NUMBER_H` (linhas 1695 a
//! 2539 do amálgama).
//!
//! Mapeamentos:
//!
//! - Os ponteiros `UC const *p`/`pend` viram índices num slice `chars: &[UC]`: `p` começa em zero e
//!   `pend` é `chars.len()`. Todo ponteiro devolvido (`lastmatch`, `from_chars_result_t::ptr`) é um
//!   índice nesse slice. Os `span<UC const>` `integer` e `fraction` viram `Range<usize>` sobre o
//!   mesmo slice (o `span` vazio com ponteiro nulo é a faixa `0..0`).
//! - Como no C++, o chamador garante `p < pend` na entrada (a primeira leitura é sem verificação).
//! - Leituras de 8 e 4 caracteres não alinhadas (`memcpy`) viram `u64::from_le_bytes` e
//!   `u32::from_le_bytes` sobre os bytes baixos dos caracteres, o que dá o mesmo valor do laço
//!   `uint8_t(*chars)` do C++ para `char` e para `char16_t`. Os `byteswap` só existem para big
//!   endian e somem.
//! - Os caminhos SIMD (`FASTFLOAT_SSE2`, que vale em x86_64) viram o equivalente escalar, que
//!   dá o mesmo resultado: `simd_read8_to_u64` empacota com saturação como `_mm_packus_epi16`, e
//!   `simd_parse_if_eight_digits_unrolled` testa os oito caracteres como o par `add`/`cmpgt` do SSE2.
//!   `has_simd_opt<UC>()` vale para `char16_t` (`UC::SIZE == 2`).
//! - O ramo `#if defined(__clang__)` de `loop_parse_if_eight_digits(char...)` entra: o Bun compila
//!   o WebKit com clang.
//! - As tabelas `int_luts`, `ch_to_digit`, `max_digits_u64` e `min_safe_u64` vivem em `float_common`
//!   (linhas 1430 a 1497 do amálgama) e são importadas de lá.
//! - O tipo inteiro `T` de `parse_int_string` vira genérico sobre o trait `ParseableInteger`,
//!   definido aqui e implementado para os inteiros de 8 a 64 bits.
//! - A aritmética sem sinal que estoura no C++ (`i = i * 10 + digit`) vira `wrapping_*`.
#![allow(non_camel_case_types, non_upper_case_globals)]

use crate::js_number::fast_float::float_common::detail::{adjust_for_feature_macros, basic_fortran_fmt};
use crate::js_number::fast_float::float_common::{
    ch_to_digit, chars_format, countr_zero_32, errc, from_chars_result_t, max_digits_u64,
    min_safe_u64, parse_options_t, span,
};
use crate::js_number::CharType;
use std::ops::Range;

/// `UC('c')`: o caractere ASCII `c` no tipo de caractere da entrada.
fn uc<UC: CharType>(c: u8) -> UC {
    UC::from_u16(c as u16)
}

/// O valor numérico do caractere (a promoção para `int` do C++).
fn code_of<UC: CharType>(c: UC) -> u32 {
    c.into()
}

/// `c - UC('0')` com a promoção para `int` do C++, visto como `int64_t`.
fn digit_offset<UC: CharType>(c: UC) -> i64 {
    let code: u32 = c.into();
    code as i64 - '0' as i64
}

/// `uint64_t(*p - UC('0'))`.
fn digit_u64<UC: CharType>(c: UC) -> u64 {
    digit_offset(c) as u64
}

/// `uint8_t(*p - UC('0'))`.
fn digit_u8<UC: CharType>(c: UC) -> u8 {
    digit_offset(c) as u8
}

pub fn has_simd_opt<UC: CharType>() -> bool {
    UC::SIZE == 2
}

// Next function can be micro-optimized, but compilers are entirely
// able to optimize it well.
pub fn is_integer<UC: CharType>(c: UC) -> bool {
    let code: u32 = c.into();
    code.wrapping_sub('0' as u32) <= 9u32
}

/// Read 8 UC into a u64. Truncates UC if not char.
pub fn read8_to_u64<UC: CharType>(chars: &[UC]) -> u64 {
    let mut bytes = [0u8; 8];
    for (i, byte) in bytes.iter_mut().enumerate() {
        let code: u32 = chars[i].into();
        *byte = code as u8;
    }
    u64::from_le_bytes(bytes)
}

/// Read 4 UC into a u32. Truncates UC if not char.
pub fn read4_to_u32<UC: CharType>(chars: &[UC]) -> u32 {
    let mut bytes = [0u8; 4];
    for (i, byte) in bytes.iter_mut().enumerate() {
        let code: u32 = chars[i].into();
        *byte = code as u8;
    }
    u32::from_le_bytes(bytes)
}

/// `simd_read8_to_u64(__m128i)` (`_mm_packus_epi16(data, data)` e a metade baixa): cada
/// unidade de 16 bits é lida como `int16_t` e saturada em `0..=255`. Para `UC` sem SIMD
/// (`has_simd_opt` falso) é o `dummy for compile`, que devolve zero.
pub fn simd_read8_to_u64<UC: CharType>(chars: &[UC]) -> u64 {
    if !has_simd_opt::<UC>() {
        // dummy for compile
        return 0;
    }
    let mut bytes = [0u8; 8];
    for (i, byte) in bytes.iter_mut().enumerate() {
        let code: u32 = chars[i].into();
        *byte = (code as u16 as i16).clamp(0, 255) as u8;
    }
    u64::from_le_bytes(bytes)
}

// credit  @aqrit
pub fn parse_eight_digits_unrolled_u64(mut val: u64) -> u32 {
    let mask: u64 = 0x000000FF000000FF;
    let mul1: u64 = 0x000F424000000064; // 100 + (1000000ULL << 32)
    let mul2: u64 = 0x0000271000000001; // 1 + (10000ULL << 32)
    val = val.wrapping_sub(0x3030303030303030);
    val = val.wrapping_mul(10).wrapping_add(val >> 8); // val = (val * 2561) >> 8;
    val = (val & mask)
        .wrapping_mul(mul1)
        .wrapping_add(((val >> 16) & mask).wrapping_mul(mul2))
        >> 32;
    val as u32
}

/// Call this if chars are definitely 8 digits.
pub fn parse_eight_digits_unrolled<UC: CharType>(chars: &[UC]) -> u32 {
    if !has_simd_opt::<UC>() {
        return parse_eight_digits_unrolled_u64(read8_to_u64(chars)); // truncation okay
    }
    parse_eight_digits_unrolled_u64(simd_read8_to_u64(chars))
}

// credit @aqrit
pub fn is_made_of_eight_digits_fast(val: u64) -> bool {
    (val.wrapping_add(0x4646464646464646) | val.wrapping_sub(0x3030303030303030))
        & 0x8080808080808080
        == 0
}

pub fn is_made_of_four_digits_fast(val: u32) -> bool {
    (val.wrapping_add(0x46464646) | val.wrapping_sub(0x30303030)) & 0x80808080 == 0
}

pub fn parse_four_digits_unrolled(mut val: u32) -> u32 {
    val = val.wrapping_sub(0x30303030);
    val = val.wrapping_mul(10).wrapping_add(val >> 8);
    (val & 0x00FF00FF).wrapping_mul(0x00640001) >> 16 & 0xFFFF
}

// Call this if chars might not be 8 digits.
// Using this style (instead of is_made_of_eight_digits_fast() then
// parse_eight_digits_unrolled()) ensures we don't load SIMD registers twice.
//
// `chars` começa no primeiro dos oito caracteres. Para `UC` sem SIMD é o `dummy for compile`,
// que devolve falso.
pub fn simd_parse_if_eight_digits_unrolled<UC: CharType>(chars: &[UC], i: &mut u64) -> bool {
    if !has_simd_opt::<UC>() {
        // dummy for compile
        return false;
    }
    // (x - '0') <= 9
    // http://0x80.pl/articles/simd-parsing-int-sequences.html
    // O `_mm_add_epi16` com 32720 seguido de `_mm_cmpgt_epi16` com -32759 marca, por unidade de
    // 16 bits, o que não está em '0'..='9'.
    let all_digits = chars[..8].iter().all(|&c| {
        let code: u32 = c.into();
        let t0 = (code as u16 as i16).wrapping_add(32720);
        !(t0 > -32759)
    });
    if all_digits {
        *i = i
            .wrapping_mul(100000000)
            .wrapping_add(parse_eight_digits_unrolled_u64(simd_read8_to_u64(chars)) as u64);
        true
    } else {
        false
    }
}

/// As duas sobrecargas de `loop_parse_if_eight_digits`: a de `char` (`UC::SIZE == 1`) e a
/// genérica para os demais `UC`, que só age quando há SIMD. `p` é o índice corrente em `chars` e
/// `pend` é `chars.len()`.
pub fn loop_parse_if_eight_digits<UC: CharType>(p: &mut usize, chars: &[UC], i: &mut u64) {
    let pend = chars.len();
    if UC::SIZE == 1 {
        // optimizes better than parse_if_eight_digits_unrolled() for UC = char.
        while (pend - *p) >= 8 && is_made_of_eight_digits_fast(read8_to_u64(&chars[*p..])) {
            *i = i
                .wrapping_mul(100000000)
                .wrapping_add(parse_eight_digits_unrolled_u64(read8_to_u64(&chars[*p..])) as u64); // in rare cases, this will overflow, but that's ok
            *p += 8;
        }
        // Consume a remaining 4-7 digit run in a single SWAR step instead of
        // byte-by-byte (reuses the existing 4-digit helpers). The parsed result is
        // identical either way. Gated to clang: on gcc the extra 4-digit check
        // regresses inputs whose remainder is shorter than 4 digits (it becomes pure
        // overhead there); clang does not show that.
        if (pend - *p) >= 4 {
            let val4 = read4_to_u32(&chars[*p..]);
            if is_made_of_four_digits_fast(val4) {
                *i = i
                    .wrapping_mul(10000)
                    .wrapping_add(parse_four_digits_unrolled(val4) as u64); // may overflow, that's ok
                *p += 4;
            }
        }
        return;
    }
    if !has_simd_opt::<UC>() {
        return;
    }
    while (pend - *p) >= 8 && simd_parse_if_eight_digits_unrolled(&chars[*p..], i) {
        // in rare cases, this will overflow, but that's ok
        *p += 8;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum parse_error {
    #[default]
    no_error,
    // [JSON-only] The minus sign must be followed by an integer.
    missing_integer_after_sign,
    // A sign must be followed by an integer or dot.
    missing_integer_or_dot_after_sign,
    // [JSON-only] The integer part must not have leading zeros.
    leading_zeros_in_integer_part,
    // [JSON-only] The integer part must have at least one digit.
    no_digits_in_integer_part,
    // [JSON-only] If there is a decimal point, there must be digits in the
    // fractional part.
    no_digits_in_fractional_part,
    // The mantissa must have at least one digit.
    no_digits_in_mantissa,
    // Scientific notation requires an exponential part.
    missing_exponential_part,
}

/// `parsed_number_string_t<UC>`: os ponteiros viram índices no slice de entrada, então o struct não
/// precisa do parâmetro `UC`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct parsed_number_string_t {
    pub exponent: i64,
    pub mantissa: u64,
    /// Índice logo depois do último caractere reconhecido.
    pub lastmatch: usize,
    pub negative: bool,
    pub valid: bool,
    pub too_many_digits: bool,
    // contains the range of the significant digits
    pub integer: Range<usize>, // non-nullable
    pub fraction: Range<usize>, // nullable
    pub error: parse_error,
}

/// `using byte_span = span<char const>;`
pub type byte_span<'a> = span<'a, u8>;
/// `using parsed_number_string = parsed_number_string_t<char>;`
pub type parsed_number_string = parsed_number_string_t;

pub fn report_parse_error(p: usize, error: parse_error) -> parsed_number_string_t {
    let mut answer = parsed_number_string_t::default();
    answer.valid = false;
    answer.lastmatch = p;
    answer.error = error;
    answer
}

// Assuming that you use no more than 19 digits, this will
// parse an ASCII string.
//
// store_spans is a *runtime* flag (not a template parameter, deliberately: a
// template would create a second instantiation of this whole function and the
// extra icache pressure wipes out the gain). When false, the integer/fraction
// spans (read only by the rare digit_comp slow path) are not materialized,
// which keeps the fat parsed_number_string_t off the hot path. The caller
// re-parses with store_spans=true if the slow path is actually reached.
pub fn parse_number_string<const BASIC_JSON_FMT: bool, UC: CharType>(
    chars: &[UC],
    options: parse_options_t<UC>,
    store_spans: bool,
) -> parsed_number_string_t {
    let mut p: usize = 0;
    let pend = chars.len();
    let fmt = adjust_for_feature_macros(options.format);
    let decimal_point: UC = options.decimal_point;

    let mut answer = parsed_number_string_t::default();
    answer.valid = false;
    answer.too_many_digits = false;
    // assume p < pend, so dereference without checks;
    answer.negative = chars[p] == uc::<UC>(b'-');
    // C++17 20.19.3.(7.1) explicitly forbids '+' sign here
    if chars[p] == uc::<UC>(b'-')
        || ((fmt & chars_format::allow_leading_plus).0 != 0
            && !BASIC_JSON_FMT
            && chars[p] == uc::<UC>(b'+'))
    {
        p += 1;
        if p == pend {
            return report_parse_error(p, parse_error::missing_integer_or_dot_after_sign);
        }
        if BASIC_JSON_FMT {
            if !is_integer(chars[p]) {
                // a sign must be followed by an integer
                return report_parse_error(p, parse_error::missing_integer_after_sign);
            }
        } else if !is_integer(chars[p]) && chars[p] != decimal_point {
            // a sign must be followed by an integer or the dot
            return report_parse_error(p, parse_error::missing_integer_or_dot_after_sign);
        }
    }
    let start_digits = p;

    let mut i: u64 = 0; // an unsigned int avoids signed overflows (which are bad)

    // Straight-line unroll of the integer-part scan: most integer parts are
    // 1-5 digits, so peeling the first iterations eliminates the loop back-edge
    // for the common case. Semantics are identical to the original `while` loop:
    // i = 10*i + digit, advancing p.
    if p != pend && is_integer(chars[p]) {
        i = digit_u64(chars[p]);
        p += 1;
        if p != pend && is_integer(chars[p]) {
            i = 10u64.wrapping_mul(i).wrapping_add(digit_u64(chars[p]));
            p += 1;
            if p != pend && is_integer(chars[p]) {
                i = 10u64.wrapping_mul(i).wrapping_add(digit_u64(chars[p]));
                p += 1;
                if p != pend && is_integer(chars[p]) {
                    i = 10u64.wrapping_mul(i).wrapping_add(digit_u64(chars[p]));
                    p += 1;
                    if p != pend && is_integer(chars[p]) {
                        i = 10u64.wrapping_mul(i).wrapping_add(digit_u64(chars[p]));
                        p += 1;
                        while p != pend && is_integer(chars[p]) {
                            // a multiplication by 10 is cheaper than an arbitrary integer
                            // multiplication
                            i = 10u64.wrapping_mul(i).wrapping_add(digit_u64(chars[p])); // might overflow, handled later
                            p += 1;
                        }
                    }
                }
            }
        }
    }
    let end_of_integer_part = p;
    let mut digit_count: i64 = end_of_integer_part as i64 - start_digits as i64;
    if store_spans {
        answer.integer = start_digits..start_digits + digit_count as usize;
    }
    if BASIC_JSON_FMT {
        // at least 1 digit in integer part, without leading zeros
        if digit_count == 0 {
            return report_parse_error(p, parse_error::no_digits_in_integer_part);
        }
        if chars[start_digits] == uc::<UC>(b'0') && digit_count > 1 {
            return report_parse_error(start_digits, parse_error::leading_zeros_in_integer_part);
        }
    }

    let mut exponent: i64 = 0;
    let has_decimal_point = p != pend && chars[p] == decimal_point;
    if has_decimal_point {
        p += 1;
        let before = p;
        // can occur at most twice without overflowing, but let it occur more, since
        // for integers with many digits, digit parsing is the primary bottleneck.
        loop_parse_if_eight_digits(&mut p, chars, &mut i);

        while p != pend && is_integer(chars[p]) {
            let digit = digit_u8(chars[p]);
            p += 1;
            i = i.wrapping_mul(10).wrapping_add(digit as u64); // in rare cases, this will overflow, but that's ok
        }
        exponent = before as i64 - p as i64;
        if store_spans {
            answer.fraction = before..p;
        }
        digit_count -= exponent;
    }
    if BASIC_JSON_FMT {
        // at least 1 digit in fractional part
        if has_decimal_point && exponent == 0 {
            return report_parse_error(p, parse_error::no_digits_in_fractional_part);
        }
    } else if digit_count == 0 {
        // we must have encountered at least one integer!
        return report_parse_error(p, parse_error::no_digits_in_mantissa);
    }
    let mut exp_number: i64 = 0; // explicit exponential part
    if ((fmt & chars_format::scientific).0 != 0
        && p != pend
        && (uc::<UC>(b'e') == chars[p] || uc::<UC>(b'E') == chars[p]))
        || ((fmt & basic_fortran_fmt).0 != 0
            && p != pend
            && (uc::<UC>(b'+') == chars[p]
                || uc::<UC>(b'-') == chars[p]
                || uc::<UC>(b'd') == chars[p]
                || uc::<UC>(b'D') == chars[p]))
    {
        let location_of_e = p;
        if uc::<UC>(b'e') == chars[p]
            || uc::<UC>(b'E') == chars[p]
            || uc::<UC>(b'd') == chars[p]
            || uc::<UC>(b'D') == chars[p]
        {
            p += 1;
        }
        let mut neg_exp = false;
        if p != pend && uc::<UC>(b'-') == chars[p] {
            neg_exp = true;
            p += 1;
        } else if p != pend && uc::<UC>(b'+') == chars[p] {
            // '+' on exponent is allowed by C++17 20.19.3.(7.1)
            p += 1;
        }
        if p == pend || !is_integer(chars[p]) {
            if (fmt & chars_format::fixed).0 == 0 {
                // The exponential part is invalid for scientific notation, so it must
                // be a trailing token for fixed notation. However, fixed notation is
                // disabled, so report a scientific notation error.
                return report_parse_error(p, parse_error::missing_exponential_part);
            }
            // Otherwise, we will be ignoring the 'e'.
            p = location_of_e;
        } else {
            while p != pend && is_integer(chars[p]) {
                let digit = digit_u8(chars[p]);
                if exp_number < 0x10000000 {
                    exp_number = 10 * exp_number + digit as i64;
                }
                p += 1;
            }
            if neg_exp {
                exp_number = -exp_number;
            }
            exponent += exp_number;
        }
    } else {
        // If it scientific and not fixed, we have to bail out.
        if (fmt & chars_format::scientific).0 != 0 && (fmt & chars_format::fixed).0 == 0 {
            return report_parse_error(p, parse_error::missing_exponential_part);
        }
    }
    answer.lastmatch = p;
    answer.valid = true;

    // If we frequently had to deal with long strings of digits,
    // we could extend our code by using a 128-bit integer instead
    // of a 64-bit integer. However, this is uncommon.
    //
    // We can deal with up to 19 digits.
    if digit_count > 19 {
        // this is uncommon
        // It is possible that the integer had an overflow.
        // We have to handle the case where we have 0.0000somenumber.
        // We need to be mindful of the case where we only have zeroes...
        // E.g., 0.000000000...000.
        let mut start = start_digits;
        while start != pend && (chars[start] == uc::<UC>(b'0') || chars[start] == decimal_point) {
            if chars[start] == uc::<UC>(b'0') {
                digit_count -= 1;
            }
            start += 1;
        }

        if digit_count > 19 {
            answer.too_many_digits = true;
            // The truncation recompute below reads the integer/fraction spans. When
            // store_spans is false we didn't materialize them, so just flag
            // too_many_digits; the caller re-parses with store_spans=true to obtain
            // the corrected mantissa/exponent before taking the slow path.
            if store_spans {
                // Let us start again, this time, avoiding overflows.
                // We don't need to call if is_integer, since we use the
                // pre-tokenized spans from above.
                i = 0;
                p = answer.integer.start;
                let int_end = answer.integer.end;
                let minimal_nineteen_digit_integer: u64 = 1000000000000000000;
                while i < minimal_nineteen_digit_integer && p != int_end {
                    i = i.wrapping_mul(10).wrapping_add(digit_u64(chars[p]));
                    p += 1;
                }
                if i >= minimal_nineteen_digit_integer {
                    // We have a big integer
                    exponent = end_of_integer_part as i64 - p as i64 + exp_number;
                } else {
                    // We have a value with a fractional component.
                    p = answer.fraction.start;
                    let frac_end = answer.fraction.end;
                    while i < minimal_nineteen_digit_integer && p != frac_end {
                        i = i.wrapping_mul(10).wrapping_add(digit_u64(chars[p]));
                        p += 1;
                    }
                    exponent = answer.fraction.start as i64 - p as i64 + exp_number;
                }
                // We have now corrected both exponent and i, to a truncated value
            }
        }
    }
    answer.exponent = exponent;
    answer.mantissa = i;
    answer
}

/// O parâmetro de template `T` de `parse_int_string`: um tipo inteiro de 8 a 64 bits.
pub trait ParseableInteger: Copy {
    /// `std::is_signed<T>::value`.
    const IS_SIGNED: bool;
    /// `std::is_same<T, std::uint8_t>::value`.
    const IS_U8: bool;
    /// `std::is_same<T, std::uint16_t>::value`.
    const IS_U16: bool;
    /// `std::is_same<T, uint64_t>::value`.
    const IS_U64: bool;
    /// `uint64_t(std::numeric_limits<T>::max())`.
    const MAX_U64: u64;
    /// `T(i)` a partir de `uint64_t`: a conversão do C++ trunca no tamanho de `T`.
    fn from_u64(i: u64) -> Self;
    /// `-x` em `T`, módulo 2^bits (o C++ só o usa em `T` com sinal, sem estouro observável).
    fn wrapping_neg(self) -> Self;
    /// `x - y` em `T`, módulo 2^bits.
    fn wrapping_sub(self, other: Self) -> Self;
}

macro_rules! impl_parseable_integer {
    ($t:ty, signed: $signed:expr, u8: $is_u8:expr, u16: $is_u16:expr, u64: $is_u64:expr) => {
        impl ParseableInteger for $t {
            const IS_SIGNED: bool = $signed;
            const IS_U8: bool = $is_u8;
            const IS_U16: bool = $is_u16;
            const IS_U64: bool = $is_u64;
            const MAX_U64: u64 = <$t>::MAX as u64;
            fn from_u64(i: u64) -> Self {
                i as $t
            }
            fn wrapping_neg(self) -> Self {
                <$t>::wrapping_neg(self)
            }
            fn wrapping_sub(self, other: Self) -> Self {
                <$t>::wrapping_sub(self, other)
            }
        }
    };
}

impl_parseable_integer!(u8, signed: false, u8: true, u16: false, u64: false);
impl_parseable_integer!(u16, signed: false, u8: false, u16: true, u64: false);
impl_parseable_integer!(u32, signed: false, u8: false, u16: false, u64: false);
impl_parseable_integer!(u64, signed: false, u8: false, u16: false, u64: true);
impl_parseable_integer!(i8, signed: true, u8: false, u16: false, u64: false);
impl_parseable_integer!(i16, signed: true, u8: false, u16: false, u64: false);
impl_parseable_integer!(i32, signed: true, u8: false, u16: false, u64: false);
impl_parseable_integer!(i64, signed: true, u8: false, u16: false, u64: false);

pub fn parse_int_string<T: ParseableInteger, UC: CharType>(
    chars: &[UC],
    value: &mut T,
    options: parse_options_t<UC>,
) -> from_chars_result_t {
    let mut p: usize = 0;
    let pend = chars.len();
    let fmt = adjust_for_feature_macros(options.format);
    let base: i32 = options.base;

    let first = p;

    let negative = chars[p] == uc::<UC>(b'-');
    if !T::IS_SIGNED && negative {
        return from_chars_result_t { ptr: first, ec: errc::invalid_argument };
    }
    if chars[p] == uc::<UC>(b'-')
        || ((fmt & chars_format::allow_leading_plus).0 != 0 && chars[p] == uc::<UC>(b'+'))
    {
        p += 1;
    }

    let start_num = p;

    while p != pend && chars[p] == uc::<UC>(b'0') {
        p += 1;
    }

    let has_leading_zeros = p > start_num;

    let start_digits = p;

    if T::IS_U8 && UC::SIZE == 1 && base == 10 {
        let len = pend - p;
        if len == 0 {
            if has_leading_zeros {
                *value = T::from_u64(0);
                return from_chars_result_t { ptr: p, ec: errc::success };
            }
            return from_chars_result_t { ptr: first, ec: errc::invalid_argument };
        }

        let mut digits: u32;

        if len >= 4 {
            digits = read4_to_u32(&chars[p..]);
        } else {
            let b0: u32 = code_of(chars[p]) & 0xFF;
            let b1: u32 = if len > 1 { code_of(chars[p + 1]) & 0xFF } else { 0xFF };
            let b2: u32 = if len > 2 { code_of(chars[p + 2]) & 0xFF } else { 0xFF };
            let b3: u32 = 0xFF;
            digits = b0 | (b1 << 8) | (b2 << 16) | (b3 << 24);
        }

        let magic: u32 =
            (digits.wrapping_add(0x46464646) | digits.wrapping_sub(0x30303030)) & 0x80808080;
        let tz = countr_zero_32(magic) as u32; // 7, 15, 23, 31, or 32
        let mut nd: u32 = if tz == 32 { 4 } else { tz >> 3 };
        nd = if (nd as usize) < len { nd } else { len as u32 };
        if nd == 0 {
            if has_leading_zeros {
                *value = T::from_u64(0);
                return from_chars_result_t { ptr: p, ec: errc::success };
            }
            return from_chars_result_t { ptr: first, ec: errc::invalid_argument };
        }
        if nd > 3 {
            let mut q = p + nd as usize;
            let mut rem = len - nd as usize;
            while rem != 0 {
                let code: u32 = chars[q].into();
                if code < '0' as u32 || code > '9' as u32 {
                    break;
                }
                q += 1;
                rem -= 1;
            }
            return from_chars_result_t { ptr: q, ec: errc::result_out_of_range };
        }

        digits ^= 0x30303030;
        digits <<= (4 - nd) * 8;

        let check: u32 =
            ((digits >> 24) & 0xff) | ((digits >> 8) & 0xff00) | ((digits << 8) & 0xff0000);
        if check > 0x00020505 {
            return from_chars_result_t { ptr: p + nd as usize, ec: errc::result_out_of_range };
        }
        *value = T::from_u64((0x640a01u32.wrapping_mul(digits) >> 24) as u8 as u64);
        return from_chars_result_t { ptr: p + nd as usize, ec: errc::success };
    }

    if T::IS_U16 && UC::SIZE == 1 && base == 10 {
        let len = pend - p;
        if len == 0 {
            if has_leading_zeros {
                *value = T::from_u64(0);
                return from_chars_result_t { ptr: p, ec: errc::success };
            }
            return from_chars_result_t { ptr: first, ec: errc::invalid_argument };
        }

        if len >= 4 {
            let digits = read4_to_u32(&chars[p..]);
            if is_made_of_four_digits_fast(digits) {
                let mut v = parse_four_digits_unrolled(digits);
                if len >= 5 && is_integer(chars[p + 4]) {
                    let fifth: u32 = chars[p + 4].into();
                    v = v * 10 + (fifth - '0' as u32);
                    if len >= 6 && is_integer(chars[p + 5]) {
                        let mut q = p + 5;
                        while q != pend && is_integer(chars[q]) {
                            q += 1;
                        }
                        return from_chars_result_t { ptr: q, ec: errc::result_out_of_range };
                    }
                    if v > 65535 {
                        return from_chars_result_t { ptr: p + 5, ec: errc::result_out_of_range };
                    }
                    *value = T::from_u64(v as u16 as u64);
                    return from_chars_result_t { ptr: p + 5, ec: errc::success };
                }
                // 4 digits
                *value = T::from_u64(v as u16 as u64);
                return from_chars_result_t { ptr: p + 4, ec: errc::success };
            }
        }
    }

    let mut i: u64 = 0;
    if base == 10 {
        loop_parse_if_eight_digits(&mut p, chars, &mut i); // use SIMD if possible
    }
    while p != pend {
        let digit = ch_to_digit(chars[p]);
        if digit as i32 >= base {
            break;
        }
        i = (base as u64).wrapping_mul(i).wrapping_add(digit as u64); // might overflow, check this later
        p += 1;
    }

    let digit_count = p - start_digits;

    if digit_count == 0 {
        if has_leading_zeros {
            *value = T::from_u64(0);
            return from_chars_result_t { ptr: p, ec: errc::success };
        }
        return from_chars_result_t { ptr: first, ec: errc::invalid_argument };
    }

    let ptr = p;

    // check u64 overflow
    let max_digits = max_digits_u64(base);
    if digit_count > max_digits {
        return from_chars_result_t { ptr, ec: errc::result_out_of_range };
    }
    // this check can be eliminated for all other types, but they will all require
    // a max_digits(base) equivalent
    if digit_count == max_digits {
        // At the max_digits boundary the accumulator `i` may have wrapped around
        // 2^64. A plain `i < min_safe_u64(base)` test is not sufficient: for any
        // base whose max_digits-length range exceeds 2^64 (base 10 reaches
        // ~5.4 * 2^64 at 20 digits) the value can wrap a whole multiple of 2^64 and
        // land back above min_safe, slipping through. Decide exactly in O(1) using
        // the leading digit, following the approach used in simdjson:
        //   ms   == min_safe_u64(base) == base^(max_digits-1), the smallest
        //           max_digits-length value.
        //   dmax == the largest leading digit whose number can still fit in u64.
        // The leading-digit band [d*ms, (d+1)*ms) has width ms < 2^64, so within
        // the single band where d == dmax the value straddles 2^64 at most once,
        // and a single threshold separates wrapped from non-wrapped values. A
        // leading digit above dmax always overflows; below dmax always fits.
        let ms: u64 = min_safe_u64(base);
        let dmax: u64 = u64::MAX / ms;
        let lead: u64 = ch_to_digit(chars[start_digits]) as u64;
        if lead > dmax || (lead == dmax && i < dmax.wrapping_mul(ms)) {
            return from_chars_result_t { ptr, ec: errc::result_out_of_range };
        }
    }

    // check other types overflow
    if !T::IS_U64 && i > T::MAX_U64 + negative as u64 {
        return from_chars_result_t { ptr, ec: errc::result_out_of_range };
    }

    if negative {
        // this weird workaround is required because:
        // - converting unsigned to signed when its value is greater than signed max
        // is UB pre-C++23.
        // - reinterpret_casting (~i + 1) would work, but it is not constexpr
        // this is always optimized into a neg instruction (note: T is an integer
        // type)
        *value = T::from_u64(T::MAX_U64)
            .wrapping_neg()
            .wrapping_sub(T::from_u64(i.wrapping_sub(T::MAX_U64)));
    } else {
        *value = T::from_u64(i);
    }

    from_chars_result_t { ptr, ec: errc::success }
}
