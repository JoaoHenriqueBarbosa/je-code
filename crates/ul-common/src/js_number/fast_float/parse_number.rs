//! Porte de `WTF/wtf/fast_float/fast_float.h`, seção `FASTFLOAT_PARSE_NUMBER_H` (linhas 4546 a
//! 5091 do amálgama), seguida de `WTF/wtf/FastFloat.h` e `WTF/wtf/FastFloat.cpp`.
//!
//! Mapeamentos:
//!
//! - Os ponteiros `first`/`last` viram um slice `chars: &[UC]`. Todo `ptr` devolvido é o índice
//!   nesse slice. Quando a função pula espaços em branco, o resto do slice é passado adiante e o
//!   índice volta rebaseado com o deslocamento.
//! - `from_chars_result_t` e `parse_options_t` vêm de `float_common` (sem o parâmetro `UC` no
//!   resultado).
//! - `T` (`float`/`double`) vira genérico sobre `BinaryFormat` (mais `BitCastWord`, de
//!   `digit_comparison`, onde há leitura dos bits). `T` inteiro vira `ParseableInteger`.
//! - `from_chars_caller<T>` e `from_chars_advanced_caller<TypeIx>` viram o trait
//!   `FromCharsAdvancedCaller`, implementado para `f32`/`f64` (caminho de ponto flutuante) e para os
//!   inteiros de 8 a 64 bits (caminho de inteiro). Os ramos `__STDCPP_FLOAT32_T__` e
//!   `__STDCPP_FLOAT64_T__` somem (o libc++ do Bun não os define).
//! - As sobrecargas de nome igual ganham nomes distintos: `from_chars_advanced_pns` (a que recebe um
//!   `parsed_number_string_t`), `from_chars_advanced` (a que recebe o intervalo e as opções),
//!   `from_chars` (ponto flutuante, com `chars_format`), `from_chars_base` (inteiro, com `base`),
//!   `integer_times_pow10_u64` e `integer_times_pow10_i64`. As sobrecargas `double` e as de `Int`
//!   genérico de `integer_times_pow10` só repassam a esses dois com um `static_cast`; em Rust o
//!   chamador escolhe `T` e o tipo da mantissa, então não existem.
//! - `FLT_EVAL_METHOD` vale 0 em Linux x86_64 (SSE2), então o primeiro `return false` de
//!   `rounds_to_nearest` não entra. O ramo de `clang` de `clinger_fast_path_impl` vale (o Bun
//!   compila o WebKit com clang). `fastfloat_unlikely` e os `#pragma` somem.
//! - `FastFloat.cpp`: `std::span<const Latin1Character>` e `std::span<const char16_t>` viram o
//!   genérico `C: CharType`; `parsedLength` é o retorno por referência, `usize`.
#![allow(non_camel_case_types, non_upper_case_globals)]

use super::ascii_number::{
    parse_int_string, parse_number_string, parsed_number_string_t, ParseableInteger,
};
use super::decimal_to_binary::{compute_error, compute_float};
use super::digit_comparison::{digit_comp, BitCastWord};
use super::float_common::detail::{adjust_for_feature_macros, basic_json_fmt};
use super::float_common::{
    chars_format, errc, from_chars_result_t, is_space, parse_options_t, to_float, BinaryFormat,
};
use crate::js_number::CharType;

pub mod detail {
    use super::super::float_common::{
        chars_format, errc, fastfloat_strncasecmp3, fastfloat_strncasecmp5, from_chars_result_t,
        str_const_inf, str_const_nan, BinaryFormat,
    };
    use crate::js_number::CharType;

    fn uc<UC: CharType>(c: u8) -> UC {
        UC::from_u16(c as u16)
    }

    /// Special case +inf, -inf, nan, infinity, -infinity.
    /// The case comparisons could be made much faster given that we know that the
    /// strings a null-free and fixed.
    pub fn parse_infnan<T: BinaryFormat, UC: CharType>(
        chars: &[UC],
        value: &mut T,
        fmt: chars_format,
    ) -> from_chars_result_t {
        let mut first: usize = 0;
        let last: usize = chars.len();
        let mut answer = from_chars_result_t { ptr: first, ec: errc::success }; // be optimistic
        // assume first < last, so dereference without checks;
        let minus_sign: bool = chars[first] == uc::<UC>(b'-');
        // C++17 20.19.3.(7.1) explicitly forbids '+' sign here
        if chars[first] == uc::<UC>(b'-')
            || ((fmt & chars_format::allow_leading_plus).0 != 0 && chars[first] == uc::<UC>(b'+'))
        {
            first += 1;
        }
        if last - first >= 3 {
            let nan_str = str_const_nan::<UC>();
            if fastfloat_strncasecmp3(&chars[first..], &nan_str[..]) {
                first += 3;
                answer.ptr = first;
                *value = if minus_sign { -T::quiet_nan() } else { T::quiet_nan() };
                // Check for possible nan(n-char-seq-opt), C++17 20.19.3.7,
                // C11 7.20.1.3.3. At least MSVC produces nan(ind) and nan(snan).
                if first != last && chars[first] == uc::<UC>(b'(') {
                    let mut ptr: usize = first + 1;
                    while ptr != last {
                        let c: UC = chars[ptr];
                        if c == uc::<UC>(b')') {
                            answer.ptr = ptr + 1; // valid nan(n-char-seq-opt)
                            break;
                        } else if !((uc::<UC>(b'a') <= c && c <= uc::<UC>(b'z'))
                            || (uc::<UC>(b'A') <= c && c <= uc::<UC>(b'Z'))
                            || (uc::<UC>(b'0') <= c && c <= uc::<UC>(b'9'))
                            || c == uc::<UC>(b'_'))
                        {
                            break; // forbidden char, not nan(n-char-seq-opt)
                        }
                        ptr += 1;
                    }
                }
                return answer;
            }
            let inf_str = str_const_inf::<UC>();
            if fastfloat_strncasecmp3(&chars[first..], &inf_str[..]) {
                if (last - first >= 8) && fastfloat_strncasecmp5(&chars[first + 3..], &inf_str[3..])
                {
                    answer.ptr = first + 8;
                } else {
                    answer.ptr = first + 3;
                }
                *value = if minus_sign { -T::infinity() } else { T::infinity() };
                return answer;
            }
        }
        answer.ec = errc::invalid_argument;
        answer
    }

    /// Returns true if the floating-pointing rounding mode is to 'nearest'.
    /// It is the default on most system. This function is meant to be inexpensive.
    /// Credit : @mwalcott3
    pub fn rounds_to_nearest() -> bool {
        // https://lemire.me/blog/2020/06/26/gcc-not-nearest/
        // FLT_EVAL_METHOD vale 0 em Linux x86_64: o primeiro `return false` não entra.
        //
        // O `volatile` do C++ impede o cálculo em tempo de compilação; `black_box` faz o mesmo.
        // Só quando fegetround() == FE_TONEAREST vale fmin + 1.0f == 1.0f - fmin. Rust seguro
        // não muda o modo de arredondamento, então o resultado em x86_64 com SSE2 é `true`.
        let fmin: f32 = std::hint::black_box(f32::MIN_POSITIVE);
        let fmini: f32 = fmin; // we copy it so that it gets loaded at most once.
        fmini + 1.0f32 == 1.0f32 - fmini
    }
}

pub fn clinger_fast_path_impl<T: BinaryFormat>(
    mantissa: u64,
    exponent: i64,
    is_negative: bool,
    value: &mut T,
) -> bool {
    // The implementation of the Clinger's fast path is convoluted because
    // we want round-to-nearest in all cases, irrespective of the rounding mode
    // selected on the thread.
    // We proceed optimistically, assuming that detail::rounds_to_nearest()
    // returns true.
    if (T::min_exponent_fast_path() as i64) <= exponent
        && exponent <= (T::max_exponent_fast_path() as i64)
    {
        // Unfortunately, the conventional Clinger's fast path is only possible
        // when the system rounds to the nearest float.
        //
        // We expect the next branch to almost always be selected.
        // We could check it first (before the previous branch), but
        // there might be performance advantages at having the check
        // be last.
        if detail::rounds_to_nearest() {
            // We have that fegetround() == FE_TONEAREST.
            // Next is Clinger's fast path.
            if mantissa <= T::max_mantissa_fast_path() {
                *value = T::from_u64(mantissa);
                if exponent < 0 {
                    *value = *value / T::exact_power_of_ten(-exponent);
                } else {
                    *value = *value * T::exact_power_of_ten(exponent);
                }
                if is_negative {
                    *value = -*value;
                }
                return true;
            }
        } else {
            // We do not have that fegetround() == FE_TONEAREST.
            // Next is a modified Clinger's fast path, inspired by Jakub Jelínek's
            // proposal
            if exponent >= 0 && mantissa <= T::max_mantissa_fast_path_at(exponent) {
                // Clang may map 0 to -0.0 when fegetround() == FE_DOWNWARD
                if mantissa == 0 {
                    *value = if is_negative { -T::from_u64(0) } else { T::from_u64(0) };
                    return true;
                }
                *value = T::from_u64(mantissa) * T::exact_power_of_ten(exponent);
                if is_negative {
                    *value = -*value;
                }
                return true;
            }
        }
    }
    false
}

/// This function overload takes parsed_number_string_t structure that is created
/// and populated either by from_chars_advanced function taking chars range and
/// parsing options or other parsing custom function implemented by user.
///
/// `chars` é o slice sobre o qual `pns` foi construído (preciso para o `digit_comp`).
pub fn from_chars_advanced_pns<T: BitCastWord, UC: CharType>(
    chars: &[UC],
    pns: &parsed_number_string_t,
    value: &mut T,
) -> from_chars_result_t {
    let mut answer = from_chars_result_t { ptr: pns.lastmatch, ec: errc::success }; // be optimistic

    if !pns.too_many_digits
        && clinger_fast_path_impl(pns.mantissa, pns.exponent, pns.negative, value)
    {
        return answer;
    }

    let mut am = compute_float::<T>(pns.exponent, pns.mantissa);
    if pns.too_many_digits && am.power2 >= 0 {
        if am != compute_float::<T>(pns.exponent, pns.mantissa.wrapping_add(1)) {
            am = compute_error::<T>(pns.exponent, pns.mantissa);
        }
    }
    // If we called compute_float<binary_format<T>>(pns.exponent, pns.mantissa)
    // and we have an invalid power (am.power2 < 0), then we need to go the long
    // way around again. This is very uncommon.
    if am.power2 < 0 {
        am = digit_comp::<T, UC>(chars, pns, am);
    }
    to_float(pns.negative, am, value);
    // Test for over/underflow.
    if (pns.mantissa != 0 && am.mantissa == 0 && am.power2 == 0)
        || am.power2 == T::infinite_power()
    {
        answer.ec = errc::result_out_of_range;
    }
    answer
}

// Slow path: re-parse materializing the integer/fraction spans the hot no-span
// parse skipped, then run the full algorithm. The two callers reach it only
// through a fastfloat_unlikely branch, so the optimizer keeps this re-parse off
// the hot path on its own (no function-level noinline needed).
// from_chars_advanced already handles both the too_many_digits disambiguation
// and the am.power2<0 digit_comp recompute, so both slow branches collapse to
// one helper call.
pub fn parse_number_slow_path<T: BitCastWord, UC: CharType>(
    chars: &[UC],
    value: &mut T,
    options: parse_options_t<UC>,
    bjf: bool,
) -> from_chars_result_t {
    let pns: parsed_number_string_t = if bjf {
        parse_number_string::<true, UC>(chars, options, true)
    } else {
        parse_number_string::<false, UC>(chars, options, true)
    };
    from_chars_advanced_pns(chars, &pns, value)
}

pub fn from_chars_float_advanced<T: BitCastWord, UC: CharType>(
    chars: &[UC],
    value: &mut T,
    options: parse_options_t<UC>,
) -> from_chars_result_t {
    let fmt: chars_format = adjust_for_feature_macros(options.format);

    let mut first: usize = 0;
    let last: usize = chars.len();
    if (fmt & chars_format::skip_white_space).0 != 0 {
        while (first != last) && is_space(chars[first]) {
            first += 1;
        }
    }
    if first == last {
        return from_chars_result_t { ptr: first, ec: errc::invalid_argument };
    }
    // O resto do slice: os índices de `parse_number_string` e dos caminhos lentos são relativos a
    // ele e voltam rebaseados com `first`.
    let rest: &[UC] = &chars[first..];
    let bjf: bool = (fmt & basic_json_fmt).0 != 0;

    // Fast path: parse WITHOUT materializing the integer/fraction spans (read
    // only by the rare slow paths). Skipping their stores keeps the fat
    // parsed_number_string_t off the hot path. store_spans is a runtime argument,
    // so this reuses the single parse_number_string instantiation.
    let pns: parsed_number_string_t = if bjf {
        parse_number_string::<true, UC>(rest, options, false)
    } else {
        parse_number_string::<false, UC>(rest, options, false)
    };
    if !pns.valid {
        if (fmt & chars_format::no_infnan).0 != 0 {
            return from_chars_result_t { ptr: first, ec: errc::invalid_argument };
        } else {
            let mut answer = detail::parse_infnan(rest, value, fmt);
            answer.ptr += first;
            return answer;
        }
    }

    // Slow path A (rare): > 19 significant digits. The no-span parse left the
    // mantissa un-truncated and skipped the span-based recompute; the cold helper
    // re-parses with spans and runs the full algorithm.
    if pns.too_many_digits {
        let mut answer = parse_number_slow_path::<T, UC>(rest, value, options, bjf);
        answer.ptr += first;
        return answer;
    }
    let mut answer = from_chars_result_t { ptr: first + pns.lastmatch, ec: errc::success }; // be optimistic

    if clinger_fast_path_impl(pns.mantissa, pns.exponent, pns.negative, value) {
        return answer;
    }

    let am = compute_float::<T>(pns.exponent, pns.mantissa);
    // Slow path B (rare): Eisel-Lemire could not resolve; digit_comp needs the
    // integer/fraction spans. Route to the cold helper (clinger there is a
    // dead-effect since it already failed here; the cold re-parse + digit_comp
    // via from_chars_advanced reproduces this branch).
    if am.power2 < 0 {
        let mut slow = parse_number_slow_path::<T, UC>(rest, value, options, bjf);
        slow.ptr += first;
        return slow;
    }
    to_float(pns.negative, am, value);
    // Test for over/underflow.
    if (pns.mantissa != 0 && am.mantissa == 0 && am.power2 == 0)
        || am.power2 == T::infinite_power()
    {
        answer.ec = errc::result_out_of_range;
    }
    answer
}

pub fn from_chars_int_advanced<T: ParseableInteger, UC: CharType>(
    chars: &[UC],
    value: &mut T,
    options: parse_options_t<UC>,
) -> from_chars_result_t {
    let fmt: chars_format = adjust_for_feature_macros(options.format);
    let base: i32 = options.base;

    let mut first: usize = 0;
    let last: usize = chars.len();
    if (fmt & chars_format::skip_white_space).0 != 0 {
        while (first != last) && is_space(chars[first]) {
            first += 1;
        }
    }
    if first == last || base < 2 || base > 36 {
        return from_chars_result_t { ptr: first, ec: errc::invalid_argument };
    }

    let mut answer = parse_int_string(&chars[first..], value, options);
    answer.ptr += first;
    answer
}

/// `from_chars_advanced_caller<TypeIx>`: o `TypeIx` é `1` para tipo de ponto flutuante e `2` para
/// tipo inteiro; o trait escolhe o caminho pelo tipo de `T`.
pub trait FromCharsAdvancedCaller: Sized {
    fn call<UC: CharType>(
        chars: &[UC],
        value: &mut Self,
        options: parse_options_t<UC>,
    ) -> from_chars_result_t;
}

macro_rules! impl_float_advanced_caller {
    ($($t:ty),*) => {
        $(
            impl FromCharsAdvancedCaller for $t {
                fn call<UC: CharType>(
                    chars: &[UC],
                    value: &mut Self,
                    options: parse_options_t<UC>,
                ) -> from_chars_result_t {
                    from_chars_float_advanced(chars, value, options)
                }
            }
        )*
    };
}

macro_rules! impl_int_advanced_caller {
    ($($t:ty),*) => {
        $(
            impl FromCharsAdvancedCaller for $t {
                fn call<UC: CharType>(
                    chars: &[UC],
                    value: &mut Self,
                    options: parse_options_t<UC>,
                ) -> from_chars_result_t {
                    from_chars_int_advanced(chars, value, options)
                }
            }
        )*
    };
}

impl_float_advanced_caller!(f32, f64);
impl_int_advanced_caller!(u8, u16, u32, u64, i8, i16, i32, i64);

pub fn from_chars_advanced<T: FromCharsAdvancedCaller, UC: CharType>(
    chars: &[UC],
    value: &mut T,
    options: parse_options_t<UC>,
) -> from_chars_result_t {
    T::call(chars, value, options)
}

/// `from_chars(first, last, value, fmt = chars_format::general)` para tipos de ponto flutuante
/// (`from_chars_caller<T>::call` com `parse_options_t<UC>(fmt)`).
pub fn from_chars<T: BinaryFormat + FromCharsAdvancedCaller, UC: CharType>(
    chars: &[UC],
    value: &mut T,
    fmt: chars_format,
) -> from_chars_result_t {
    from_chars_advanced(chars, value, parse_options_t::new(fmt, UC::from_u16(b'.' as u16), 10))
}

/// `from_chars(first, last, value, base = 10)` para tipos inteiros.
pub fn from_chars_base<T: ParseableInteger + FromCharsAdvancedCaller, UC: CharType>(
    chars: &[UC],
    value: &mut T,
    base: i32,
) -> from_chars_result_t {
    let mut options: parse_options_t<UC> = parse_options_t::default();
    options.base = base;
    from_chars_advanced(chars, value, options)
}

/// `integer_times_pow10(uint64_t mantissa, int decimal_exponent)` para `T`.
pub fn integer_times_pow10_u64<T: BinaryFormat>(mantissa: u64, decimal_exponent: i32) -> T {
    let mut value: T = T::from_u64(0);
    if clinger_fast_path_impl(mantissa, decimal_exponent as i64, false, &mut value) {
        return value;
    }

    let am = compute_float::<T>(decimal_exponent as i64, mantissa);
    to_float(false, am, &mut value);
    value
}

/// `integer_times_pow10(int64_t mantissa, int decimal_exponent)` para `T`.
pub fn integer_times_pow10_i64<T: BinaryFormat>(mantissa: i64, decimal_exponent: i32) -> T {
    let is_negative: bool = mantissa < 0;
    let m: u64 = mantissa.unsigned_abs();

    let mut value: T = T::from_u64(0);
    if clinger_fast_path_impl(m, decimal_exponent as i64, is_negative, &mut value) {
        return value;
    }

    let am = compute_float::<T>(decimal_exponent as i64, m);
    to_float(is_negative, am, &mut value);
    value
}

// ---------------------------------------------------------------------------------------------
// WTF/wtf/FastFloat.h e FastFloat.cpp
// ---------------------------------------------------------------------------------------------

/// `WTF::parseDouble`.
pub fn parse_double<C: CharType>(string: &[C], parsed_length: &mut usize) -> f64 {
    let mut double_value: f64 = 0.0;
    let result = from_chars::<f64, C>(
        string,
        &mut double_value,
        chars_format::general | chars_format::no_infnan | chars_format::allow_leading_plus,
    );
    *parsed_length = result.ptr;
    double_value
}

/// `WTF::parseFixedDouble`.
pub fn parse_fixed_double<C: CharType>(string: &[C], parsed_length: &mut usize) -> f64 {
    let mut double_value: f64 = 0.0;
    let result = from_chars::<f64, C>(
        string,
        &mut double_value,
        chars_format::fixed | chars_format::no_infnan,
    );
    *parsed_length = result.ptr;
    double_value
}

/// `WTF::parseJSONDouble`.
pub fn parse_json_double<C: CharType>(string: &[C], parsed_length: &mut usize) -> Option<f64> {
    let mut double_value: f64 = 0.0;
    let result = from_chars::<f64, C>(string, &mut double_value, chars_format::json);
    if !result.as_bool() && result.ec != errc::result_out_of_range {
        return None;
    }
    *parsed_length = result.ptr;
    Some(double_value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn double_of(text: &[u8]) -> (f64, usize) {
        let mut length: usize = usize::MAX;
        let value = parse_double(text, &mut length);
        (value, length)
    }

    #[test]
    fn parse_double_basic() {
        assert_eq!(double_of(b"1.5"), (1.5, 3));
        assert_eq!(double_of(b"+2e3"), (2000.0, 4));
        assert_eq!(double_of(b"0.1"), (0.1, 3));
        assert_eq!(double_of(b"123abc"), (123.0, 3));
        assert_eq!(double_of(b".5"), (0.5, 2));
        assert_eq!(double_of(b"5."), (5.0, 2));
    }

    #[test]
    fn parse_double_overflow_is_infinite() {
        let (value, length) = double_of(b"1e400");
        assert!(value.is_infinite() && value > 0.0);
        assert_eq!(length, 5);
    }

    #[test]
    fn parse_double_without_number() {
        assert_eq!(double_of(b"abc").1, 0);
        assert_eq!(double_of(b"").1, 0);
        // no_infnan: "inf" e "nan" não são números.
        assert_eq!(double_of(b"inf").1, 0);
    }

    #[test]
    fn parse_double_utf16() {
        let text: Vec<u16> = "-1.25e2xyz".encode_utf16().collect();
        let mut length: usize = 0;
        assert_eq!(parse_double(&text, &mut length), -125.0);
        assert_eq!(length, 7);
    }

    #[test]
    fn parse_fixed_double_ignores_exponent() {
        let mut length: usize = 0;
        assert_eq!(parse_fixed_double(&b"1.5e3"[..], &mut length), 1.5);
        assert_eq!(length, 3);
    }

    #[test]
    fn from_chars_infnan() {
        let mut value: f64 = 0.0;
        let result = from_chars::<f64, u8>(&b"-Infinity!"[..], &mut value, chars_format::general);
        assert_eq!((value, result.ptr, result.ec), (f64::NEG_INFINITY, 9, errc::success));
        let result = from_chars::<f64, u8>(&b"nan(abc)x"[..], &mut value, chars_format::general);
        assert!(value.is_nan());
        assert_eq!((result.ptr, result.ec), (8, errc::success));
    }

    #[test]
    fn from_chars_matches_rust_on_hard_cases() {
        // Casos que passam por Eisel-Lemire, pelo recálculo com mantissa truncada e por digit_comp.
        let cases = [
            "1.7976931348623157e308",
            "4.9406564584124654e-324",
            "2.2250738585072011e-308",
            "2.2250738585072014e-308",
            "9007199254740993",
            "9007199254740992.9999999999999999999999",
            "0.1000000000000000055511151231257827021181583404541015625",
            "123456789012345678901234567890",
            "8.41e21",
            "2.4703282292062327e-324",
            "2.4703282292062328e-324",
            "1e23",
            "1.00000000000000011102230246251565404236316680908203125",
            "1.00000000000000011102230246251565404236316680908203126",
        ];
        for text in cases {
            let (value, length) = double_of(text.as_bytes());
            let expected: f64 = text.parse().unwrap();
            assert_eq!(value.to_bits(), expected.to_bits(), "{text}");
            assert_eq!(length, text.len(), "{text}");
        }
    }

    #[test]
    fn from_chars_f32_and_integers() {
        let mut f: f32 = 0.0;
        let result = from_chars::<f32, u8>(&b"3.4028234e38"[..], &mut f, chars_format::general);
        assert_eq!((f.to_bits(), result.ptr), ((3.4028234e38f32).to_bits(), 12));

        let mut n: i32 = 0;
        let result = from_chars_base::<i32, u8>(&b"-123x"[..], &mut n, 10);
        assert_eq!((n, result.ptr, result.ec), (-123, 4, errc::success));
    }

    #[test]
    fn integer_times_pow10_cases() {
        assert_eq!(integer_times_pow10_u64::<f64>(15, -1), 1.5);
        assert_eq!(integer_times_pow10_i64::<f64>(-15, -1), -1.5);
        assert_eq!(integer_times_pow10_u64::<f64>(1, 400), f64::INFINITY);
    }

    #[test]
    fn rounds_to_nearest_is_true_on_x86_64() {
        assert!(detail::rounds_to_nearest());
    }
}
