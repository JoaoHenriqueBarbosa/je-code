//! Porte de `WTF/wtf/dtoa` (double-conversion).
pub mod bignum;
pub mod bignum_dtoa;
pub mod cached_powers;
pub mod diy_fp;
pub mod fast_dtoa;
pub mod fixed_dtoa;
pub mod strtod;
pub mod double_conversion;
pub mod ieee;
pub mod utils;

// Tradução de WTF/wtf/dtoa.h e dtoa.cpp.
//
// Mapeamentos: `NumberToStringBuffer` (`std::array<char, 124>`) é `[u8; 124]`; o `NumberToStringSpan`
// (`std::span<const char>` sobre o buffer) é `&[u8]` com o empréstimo do buffer. As sobrecargas
// `float`/`double` do C++ viram genéricos sobre `NumberToStringInput`, implementado só por `f32` e
// `f64`. `StringBuilder` segura a referência mutável do buffer; por isso o texto pronto sai do
// builder já finalizado (`finalize`) e o tamanho dele fatia o buffer devolvido.
//
// `parseDouble(StringView, size_t&)` do `dtoa.h` não está aqui: ele só repassa para o
// `parseDouble(span8/span16)` do `FastFloat.h`, que usa a biblioteca fast_float, e para o `StringView`
// (`wtf/text`); entra com esses módulos.

use crate::wtf::dragonbox::detail::cache_holder::CacheHolder;
use crate::wtf::dragonbox::dragonbox::{max_string_length, ComputeMulImpl, Mode};
use crate::wtf::dragonbox::dragonbox_to_chars::{to_chars_n, ToCharsImpl};
use crate::wtf::dragonbox::ieee754_format::FloatTraits;
use crate::wtf::dtoa::double_conversion::DoubleToStringConverter;
use crate::wtf::dtoa::utils::StringBuilder;

/// Only toFixed() can use all the 124 positions. The format is:
/// <-> + <21 digits> + decimal point + <100 digits> + null char = 124.
pub type NumberToStringBuffer = [u8; 124];

/// <-> + <320 digits> + decimal point + <6 digits> + null char = 329
pub type NumberToCSSStringBuffer = [u8; 329];

pub type NumberToStringSpan<'a> = &'a [u8];

/// Os tipos que as sobrecargas `float` e `double` do `dtoa.h` aceitam.
pub trait NumberToStringInput: ToCharsImpl {
    /// `static_cast<double>(number)`: a sobrecarga de `float` repassa para a de `double`.
    fn to_double(self) -> f64;
}

impl NumberToStringInput for f32 {
    fn to_double(self) -> f64 {
        self as f64
    }
}

impl NumberToStringInput for f64 {
    fn to_double(self) -> f64 {
        self
    }
}

/// `numberToStringAndSize(float|double, buffer)`.
pub fn number_to_string_and_size<T: NumberToStringInput>(
    number: T,
    buffer: &mut NumberToStringBuffer,
) -> NumberToStringSpan<'_> {
    const {
        assert!(
            124 >= max_string_length::<<T as FloatTraits>::Format>() + 1,
            "NumberToStringBuffer é pequeno demais para o formato"
        )
    };
    let length = to_chars_n::<T>(Mode::ToShortest, number, &mut buffer[..]);
    &buffer[..length]
}

/// `numberToStringWithTrailingPoint(double, buffer)`.
pub fn number_to_string_with_trailing_point(
    d: f64,
    buffer: &mut NumberToStringBuffer,
) -> NumberToStringSpan<'_> {
    let length = {
        let mut builder = StringBuilder::new(&mut buffer[..]);
        let converter = DoubleToStringConverter::ecma_script_converter_with_trailing_point();
        converter.to_shortest(d, &mut builder);
        builder.finalize().len()
    };
    &buffer[..length]
}

/// `truncateTrailingZeros(buffer, builder)`. O C++ recebe o buffer à parte; aqui ele é o do próprio
/// `builder` (ver `StringBuilder::buffer`).
fn truncate_trailing_zeros(builder: &mut StringBuilder) {
    let length = builder.position() as usize;
    let (decimal_point_position, past_mantissa, truncated_length) = {
        let buffer = builder.buffer();
        let mut decimal_point_position = 0;
        while decimal_point_position < length {
            if buffer[decimal_point_position] == b'.' {
                break;
            }
            decimal_point_position += 1;
        }

        // No decimal separator found, early exit.
        if decimal_point_position == length {
            return;
        }

        let mut past_mantissa = decimal_point_position + 1;
        while past_mantissa < length {
            if buffer[past_mantissa] == b'e' {
                break;
            }
            past_mantissa += 1;
        }

        let mut truncated_length = past_mantissa;
        while truncated_length > decimal_point_position + 1 {
            if buffer[truncated_length - 1] != b'0' {
                break;
            }
            truncated_length -= 1;
        }

        (decimal_point_position, past_mantissa, truncated_length)
    };

    // No trailing zeros found to strip.
    if truncated_length == past_mantissa {
        return;
    }

    // If we removed all trailing zeros, remove the decimal point as well.
    let truncated_length = if truncated_length == decimal_point_position + 1 {
        decimal_point_position
    } else {
        truncated_length
    };

    // Truncate the mantissa, and return the final result.
    builder.remove_characters(truncated_length, past_mantissa);
}

/// `numberToFixedPrecisionString(float|double, significantFigures, buffer, truncateTrailingZeros)`.
/// A versão de `float` chama a de `double`, como no C++.
pub fn number_to_fixed_precision_string<T: NumberToStringInput>(
    number: T,
    significant_figures: u32,
    buffer: &mut NumberToStringBuffer,
    should_truncate_trailing_zeros: bool,
) -> NumberToStringSpan<'_> {
    let d = number.to_double();

    // Mimic sprintf("%.[precision]g", ...).
    // "g": Signed value printed in f or e format, whichever is more compact for the given value and precision.
    // The e format is used only when the exponent of the value is less than -4 or greater than or equal to the
    // precision argument. Trailing zeros are truncated, and the decimal point appears only if one or more digits follow it.
    // "precision": The precision specifies the maximum number of significant digits printed.
    let length = {
        let mut builder = StringBuilder::new(&mut buffer[..]);
        let converter = DoubleToStringConverter::ecma_script_converter();
        converter.to_precision(d, significant_figures as i32, &mut builder);
        if should_truncate_trailing_zeros {
            truncate_trailing_zeros(&mut builder);
        }
        builder.finalize().len()
    };
    &buffer[..length]
}

/// `numberToFixedWidthString(float|double, decimalPlaces, buffer)`. A versão de `float` chama a de
/// `double`, como no C++.
pub fn number_to_fixed_width_string<T: NumberToStringInput>(
    number: T,
    decimal_places: u32,
    buffer: &mut NumberToStringBuffer,
) -> NumberToStringSpan<'_> {
    let d = number.to_double();

    // Mimic sprintf("%.[precision]f", ...).
    // "f": Signed value having the form [ - ]dddd.dddd, where dddd is one or more decimal digits.
    // The number of digits before the decimal point depends on the magnitude of the number, and
    // the number of digits after the decimal point depends on the requested precision.
    // "precision": The precision value specifies the number of digits after the decimal point.
    // If a decimal point appears, at least one digit appears before it.
    // The value is rounded to the appropriate number of digits.
    let length = {
        let mut builder = StringBuilder::new(&mut buffer[..]);
        let converter = DoubleToStringConverter::ecma_script_converter();
        converter.to_fixed(d, decimal_places as i32, &mut builder);
        builder.finalize().len()
    };
    &buffer[..length]
}

/// `numberToCSSString(double, buffer)`: largura fixa com até 6 casas decimais, zeros à direita
/// truncados.
pub fn number_to_css_string(d: f64, buffer: &mut NumberToCSSStringBuffer) -> NumberToStringSpan<'_> {
    // Mimic sprintf("%.[precision]f", ...).
    // "f": Signed value having the form [ - ]dddd.dddd, where dddd is one or more decimal digits.
    // The number of digits before the decimal point depends on the magnitude of the number, and
    // the number of digits after the decimal point depends on the requested precision.
    // "precision": The precision value specifies the number of digits after the decimal point.
    // If a decimal point appears, at least one digit appears before it.
    // The value is rounded to the appropriate number of digits.
    let length = {
        let mut builder = StringBuilder::new(&mut buffer[..]);
        let converter = DoubleToStringConverter::css_converter();
        converter.to_fixed_uncapped(d, 6, &mut builder);
        truncate_trailing_zeros(&mut builder);
        // If we've truncated the trailing zeros and a trailing decimal, we may have a -0. Remove the negative sign in this case.
        if builder.position() == 2 && builder.buffer()[0] == b'-' && builder.buffer()[1] == b'0' {
            builder.remove_characters(0, 1);
        }
        builder.finalize().len()
    };
    &buffer[..length]
}
