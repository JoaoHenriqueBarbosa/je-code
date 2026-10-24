// Tradução de WTF/wtf/dtoa/double-conversion.h e double-conversion.cc (V8 e Apple).
//
// Mapeamentos: `StringBuilder*` vira `&mut StringBuilder`; `std::span<char>`/`BufferReference<char>`
// viram `&mut [u8]` (e `std::span<const char>` vira `&[u8]`); os parâmetros de saída por referência
// (`sign`, `length`, `point`, `processed_characters_count`) ficam como `&mut`, como no C++. Os
// iteradores do `StringToIeee` viram índices sobre a fatia de entrada. `infinity_symbol` e
// `nan_symbol` são `Option<&'static [u8]>` (o `nullptr` do C++ é `None`).

use std::ops::Neg;

use crate::wtf::ascii_ctype::{is_ascii_digit, AsciiChar};
use crate::wtf::dtoa::bignum_dtoa::{bignum_dtoa, BignumDtoaMode};
use crate::wtf::dtoa::fast_dtoa::{fast_dtoa, FastDtoaMode};
use crate::wtf::dtoa::fixed_dtoa::fast_fixed_dtoa;
use crate::wtf::dtoa::ieee::Double;
use crate::wtf::dtoa::strtod::{strtod, strtof};
use crate::wtf::dtoa::utils::{
    max, valid_shortest_representation, StringBuilder, DEFAULT_DECIMAL_IN_SHORTEST_HIGH,
    DEFAULT_DECIMAL_IN_SHORTEST_LOW,
};

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DtoaMode {
    // Produce the shortest correct representation.
    // For example the output of 0.299999999999999988897 is (the less accurate
    // but correct) 0.3.
    SHORTEST,
    // Same as SHORTEST, but for single-precision floats.
    SHORTEST_SINGLE,
    // Produce a fixed number of digits after the decimal point.
    // For instance fixed(0.1, 4) becomes 0.1000
    // If the input number is big, the output will be big.
    FIXED,
    // Fixed number of digits (independent of the decimal point).
    PRECISION,
}

/// `AddPadding(c, count)` do C++ com `count` inteiro: contagem negativa não acrescenta nada.
fn padding_count(count: i32) -> usize {
    if count > 0 { count as usize } else { 0 }
}

pub struct DoubleToStringConverter {
    flags: u32,
    infinity_symbol: Option<&'static [u8]>,
    nan_symbol: Option<&'static [u8]>,
    exponent_character: u8,
    decimal_in_shortest_low: i32,
    decimal_in_shortest_high: i32,
    max_leading_padding_zeroes_in_precision_mode: i32,
    max_trailing_padding_zeroes_in_precision_mode: i32,
}

impl DoubleToStringConverter {
    // When calling ToFixed with a double > 10^kMaxFixedDigitsBeforePoint
    // or a requested_digits parameter > kMaxFixedDigitsAfterPoint then the
    // function returns false.
    pub const MAX_FIXED_DIGITS_BEFORE_POINT: i32 = 21;
    pub const MAX_FIXED_DIGITS_AFTER_POINT: i32 = 100;

    // When calling ToExponential with a requested_digits
    // parameter > kMaxExponentialDigits then the function returns false.
    pub const MAX_EXPONENTIAL_DIGITS: i32 = 100;

    // When calling ToPrecision with a requested_digits
    // parameter < kMinPrecisionDigits or requested_digits > kMaxPrecisionDigits
    // then the function returns false.
    pub const MIN_PRECISION_DIGITS: i32 = 1;
    pub const MAX_PRECISION_DIGITS: i32 = 100;

    // `enum Flags`.
    pub const NO_FLAGS: u32 = 0;
    pub const EMIT_POSITIVE_EXPONENT_SIGN: u32 = 1;
    pub const EMIT_TRAILING_DECIMAL_POINT: u32 = 2;
    pub const EMIT_TRAILING_ZERO_AFTER_POINT: u32 = 4;
    pub const UNIQUE_ZERO: u32 = 8;

    // The maximal number of digits that are needed to emit a double in base 10.
    // A higher precision can be achieved by using more digits, but the shortest
    // accurate representation of any double will never use more digits than
    // kBase10MaximalLength.
    // Note that DoubleToAscii null-terminates its input. So the given buffer
    // should be at least kBase10MaximalLength + 1 characters long.
    pub const BASE10_MAXIMAL_LENGTH: i32 = 17;

    // Flags should be a bit-or combination of the possible Flags-enum.
    //  - NO_FLAGS: no special flags.
    //  - EMIT_POSITIVE_EXPONENT_SIGN: when the number is converted into exponent
    //    form, emits a '+' for positive exponents. Example: 1.2e+2.
    //  - EMIT_TRAILING_DECIMAL_POINT: when the input number is an integer and is
    //    converted into decimal format then a trailing decimal point is appended.
    //    Example: 2345.0 is converted to "2345.".
    //  - EMIT_TRAILING_ZERO_AFTER_POINT: in addition to a trailing decimal point
    //    emits a trailing '0'-character. This flag requires the
    //    EXMIT_TRAILING_DECIMAL_POINT flag.
    //    Example: 2345.0 is converted to "2345.0".
    //  - UNIQUE_ZERO: "-0.0" is converted to "0.0".
    //
    // Infinity symbol and nan_symbol provide the string representation for these
    // special values. If the string is NULL and the special value is encountered
    // then the conversion functions return false.
    //
    // The exponent_character is used in exponential representations. It is
    // usually 'e' or 'E'.
    //
    // When converting to the shortest representation the converter will
    // represent input numbers in decimal format if they are in the interval
    // [10^decimal_in_shortest_low; 10^decimal_in_shortest_high[
    //    (lower boundary included, greater boundary excluded).
    //
    // When converting to precision mode the converter may add
    // max_leading_padding_zeroes before returning the number in exponential
    // format. Similarily the converter may add up to
    // max_trailing_padding_zeroes_in_precision_mode in precision mode to avoid
    // returning an exponential representation. A zero added by the
    // EMIT_TRAILING_ZERO_AFTER_POINT flag is counted for this limit.
    pub const fn new(
        flags: u32,
        infinity_symbol: Option<&'static [u8]>,
        nan_symbol: Option<&'static [u8]>,
        exponent_character: u8,
        decimal_in_shortest_low: i32,
        decimal_in_shortest_high: i32,
        max_leading_padding_zeroes_in_precision_mode: i32,
        max_trailing_padding_zeroes_in_precision_mode: i32,
    ) -> DoubleToStringConverter {
        // When 'trailing zero after the point' is set, then 'trailing point'
        // must be set too.
        assert!(
            ((flags & Self::EMIT_TRAILING_DECIMAL_POINT) != 0)
                || !((flags & Self::EMIT_TRAILING_ZERO_AFTER_POINT) != 0)
        );
        DoubleToStringConverter {
            flags,
            infinity_symbol,
            nan_symbol,
            exponent_character,
            decimal_in_shortest_low,
            decimal_in_shortest_high,
            max_leading_padding_zeroes_in_precision_mode,
            max_trailing_padding_zeroes_in_precision_mode,
        }
    }

    // Returns a converter following the EcmaScript specification.
    pub const fn ecma_script_converter() -> DoubleToStringConverter {
        const FLAGS: u32 =
            DoubleToStringConverter::UNIQUE_ZERO | DoubleToStringConverter::EMIT_POSITIVE_EXPONENT_SIGN;
        DoubleToStringConverter::new(
            FLAGS,
            Some(b"Infinity"),
            Some(b"NaN"),
            b'e',
            DEFAULT_DECIMAL_IN_SHORTEST_LOW,
            DEFAULT_DECIMAL_IN_SHORTEST_HIGH,
            6,
            0,
        )
    }

    pub const fn ecma_script_converter_with_trailing_point() -> DoubleToStringConverter {
        const FLAGS: u32 = DoubleToStringConverter::UNIQUE_ZERO
            | DoubleToStringConverter::EMIT_POSITIVE_EXPONENT_SIGN
            | DoubleToStringConverter::EMIT_TRAILING_DECIMAL_POINT;
        DoubleToStringConverter::new(
            FLAGS,
            Some(b"Infinity"),
            Some(b"NaN"),
            b'e',
            DEFAULT_DECIMAL_IN_SHORTEST_LOW,
            DEFAULT_DECIMAL_IN_SHORTEST_HIGH,
            6,
            0,
        )
    }

    pub const fn css_converter() -> DoubleToStringConverter {
        const FLAGS: u32 =
            DoubleToStringConverter::UNIQUE_ZERO | DoubleToStringConverter::EMIT_POSITIVE_EXPONENT_SIGN;
        DoubleToStringConverter::new(
            FLAGS,
            Some(b"infinity"),
            Some(b"NaN"),
            b'e',
            DEFAULT_DECIMAL_IN_SHORTEST_LOW,
            DEFAULT_DECIMAL_IN_SHORTEST_HIGH,
            6,
            0,
        )
    }

    // Computes the shortest string of digits that correctly represent the input
    // number. Depending on decimal_in_shortest_low and decimal_in_shortest_high
    // (see constructor) it then either returns a decimal representation, or an
    // exponential representation.
    //
    // Returns true if the conversion succeeds. The conversion always succeeds
    // except when the input value is special and no infinity_symbol or
    // nan_symbol has been given to the constructor.
    pub fn to_shortest(&self, value: f64, result_builder: &mut StringBuilder) -> bool {
        self.to_shortest_ieee_number(value, result_builder, DtoaMode::SHORTEST)
    }

    // Same as ToShortest, but for single-precision floats.
    pub fn to_shortest_single(&self, value: f32, result_builder: &mut StringBuilder) -> bool {
        self.to_shortest_ieee_number(value as f64, result_builder, DtoaMode::SHORTEST_SINGLE)
    }

    // Computes a decimal representation with a fixed number of digits after the
    // decimal point. The last emitted digit is rounded.
    //
    // If requested_digits equals 0, then the tail of the result depends on
    // the EMIT_TRAILING_DECIMAL_POINT and EMIT_TRAILING_ZERO_AFTER_POINT.
    //
    // Returns true if the conversion succeeds. The conversion always succeeds
    // except for the following cases:
    //   - the input value is special and no infinity_symbol or nan_symbol has
    //     been provided to the constructor,
    //   - 'value' > 10^kMaxFixedDigitsBeforePoint, or
    //   - 'requested_digits' > kMaxFixedDigitsAfterPoint.
    pub fn to_fixed(
        &self,
        value: f64,
        requested_digits: i32,
        result_builder: &mut StringBuilder,
    ) -> bool {
        const _: () = assert!(DoubleToStringConverter::MAX_FIXED_DIGITS_BEFORE_POINT == 21);
        let first_non_fixed: f64 = 1e21;

        if Double::from_f64(value).is_special() {
            return self.handle_special_values(value, result_builder);
        }

        if requested_digits > Self::MAX_FIXED_DIGITS_AFTER_POINT {
            return false;
        }
        if value >= first_non_fixed || value <= -first_non_fixed {
            return false;
        }

        // Add space for the '\0' byte.
        const DECIMAL_REP_CAPACITY: usize = (DoubleToStringConverter::MAX_FIXED_DIGITS_BEFORE_POINT
            + DoubleToStringConverter::MAX_FIXED_DIGITS_AFTER_POINT
            + 1) as usize;
        let mut decimal_rep = [0u8; DECIMAL_REP_CAPACITY];
        self.to_fixed_internal(value, requested_digits, &mut decimal_rep, result_builder)
    }

    // The same as ToFixed, except without a limit on the maximum number
    // of digits before the decimal point.
    pub fn to_fixed_uncapped(
        &self,
        value: f64,
        requested_digits: i32,
        result_builder: &mut StringBuilder,
    ) -> bool {
        // Max double is 1e308, so we could have 310 digits including a negative sign.
        const MAX_POSSIBLE_DIGITS_BEFORE_POINT: i32 = 310;

        if Double::from_f64(value).is_special() {
            return self.handle_special_values(value, result_builder);
        }

        if requested_digits > Self::MAX_FIXED_DIGITS_AFTER_POINT {
            return false;
        }

        // Add space for the '\0' byte.
        const DECIMAL_REP_CAPACITY: usize =
            (MAX_POSSIBLE_DIGITS_BEFORE_POINT + DoubleToStringConverter::MAX_FIXED_DIGITS_AFTER_POINT + 1)
                as usize;
        let mut decimal_rep = [0u8; DECIMAL_REP_CAPACITY];
        self.to_fixed_internal(value, requested_digits, &mut decimal_rep, result_builder)
    }

    // Computes a representation in exponential format with requested_digits
    // after the decimal point. The last emitted digit is rounded.
    // If requested_digits equals -1, then the shortest exponential representation
    // is computed.
    //
    // Returns true if the conversion succeeds. The conversion always succeeds
    // except for the following cases:
    //   - the input value is special and no infinity_symbol or nan_symbol has
    //     been provided to the constructor,
    //   - 'requested_digits' > kMaxExponentialDigits.
    pub fn to_exponential(
        &self,
        value: f64,
        requested_digits: i32,
        result_builder: &mut StringBuilder,
    ) -> bool {
        if Double::from_f64(value).is_special() {
            return self.handle_special_values(value, result_builder);
        }

        if requested_digits < -1 {
            return false;
        }
        if requested_digits > Self::MAX_EXPONENTIAL_DIGITS {
            return false;
        }

        let mut decimal_point: i32 = 0;
        let mut sign = false;
        // Add space for digit before the decimal point and the '\0' character.
        const DECIMAL_REP_CAPACITY: usize = (DoubleToStringConverter::MAX_EXPONENTIAL_DIGITS + 2) as usize;
        const _: () = assert!(DECIMAL_REP_CAPACITY > DoubleToStringConverter::BASE10_MAXIMAL_LENGTH as usize);
        let mut decimal_rep = [0u8; DECIMAL_REP_CAPACITY];
        let mut decimal_rep_length: i32 = 0;

        if requested_digits == -1 {
            Self::double_to_ascii(
                value,
                DtoaMode::SHORTEST,
                0,
                &mut decimal_rep,
                &mut sign,
                &mut decimal_rep_length,
                &mut decimal_point,
            );
        } else {
            Self::double_to_ascii(
                value,
                DtoaMode::PRECISION,
                requested_digits + 1,
                &mut decimal_rep,
                &mut sign,
                &mut decimal_rep_length,
                &mut decimal_point,
            );
            debug_assert!(decimal_rep_length <= requested_digits + 1);

            if decimal_rep_length < requested_digits + 1 {
                for i in decimal_rep_length..requested_digits + 1 {
                    decimal_rep[i as usize] = b'0';
                }
                decimal_rep_length = requested_digits + 1;
                decimal_rep[decimal_rep_length as usize] = 0;
            }
        }

        let unique_zero = (self.flags & Self::UNIQUE_ZERO) != 0;
        if sign && (value != 0.0 || !unique_zero) {
            result_builder.add_character(b'-');
        }

        let exponent = decimal_point - 1;
        self.create_exponential_representation(
            &decimal_rep[..decimal_rep_length as usize],
            exponent,
            result_builder,
        );
        true
    }

    // Computes 'precision' leading digits of the given 'value' and returns them
    // either in exponential or decimal format, depending on
    // max_{leading|trailing}_padding_zeroes_in_precision_mode (given to the
    // constructor).
    // The last computed digit is rounded.
    //
    // Returns true if the conversion succeeds. The conversion always succeeds
    // except for the following cases:
    //   - the input value is special and no infinity_symbol or nan_symbol has
    //     been provided to the constructor,
    //   - precision < kMinPericisionDigits
    //   - precision > kMaxPrecisionDigits
    pub fn to_precision(
        &self,
        value: f64,
        precision: i32,
        result_builder: &mut StringBuilder,
    ) -> bool {
        if Double::from_f64(value).is_special() {
            return self.handle_special_values(value, result_builder);
        }

        if precision < Self::MIN_PRECISION_DIGITS || precision > Self::MAX_PRECISION_DIGITS {
            return false;
        }

        // Find a sufficiently precise decimal representation of n.
        let mut decimal_point: i32 = 0;
        let mut sign = false;
        // Add one for the terminating null character.
        const DECIMAL_REP_CAPACITY: usize = (DoubleToStringConverter::MAX_PRECISION_DIGITS + 1) as usize;
        let mut decimal_rep = [0u8; DECIMAL_REP_CAPACITY];
        let mut decimal_rep_length: i32 = 0;

        Self::double_to_ascii(
            value,
            DtoaMode::PRECISION,
            precision,
            &mut decimal_rep,
            &mut sign,
            &mut decimal_rep_length,
            &mut decimal_point,
        );
        debug_assert!(decimal_rep_length <= precision);

        let unique_zero = (self.flags & Self::UNIQUE_ZERO) != 0;
        if sign && (value != 0.0 || !unique_zero) {
            result_builder.add_character(b'-');
        }

        // The exponent if we print the number as x.xxeyyy. That is with the
        // decimal point after the first digit.
        let exponent = decimal_point - 1;

        let extra_zero: i32 = if (self.flags & Self::EMIT_TRAILING_ZERO_AFTER_POINT) != 0 { 1 } else { 0 };
        if (-decimal_point + 1 > self.max_leading_padding_zeroes_in_precision_mode)
            || (decimal_point - precision + extra_zero > self.max_trailing_padding_zeroes_in_precision_mode)
        {
            // Fill buffer to contain 'precision' digits.
            // Usually the buffer is already at the correct length, but 'DoubleToAscii'
            // is allowed to return less characters.
            for i in decimal_rep_length..precision {
                decimal_rep[i as usize] = b'0';
            }

            self.create_exponential_representation(
                &decimal_rep[..precision as usize],
                exponent,
                result_builder,
            );
        } else {
            self.create_decimal_representation(
                &decimal_rep[..decimal_rep_length as usize],
                decimal_point,
                max(0, precision - decimal_point),
                result_builder,
            );
        }
        true
    }

    // Converts the given double 'v' to digit characters. 'v' must not be NaN,
    // +Infinity, or -Infinity. In SHORTEST_SINGLE-mode this restriction also
    // applies to 'v' after it has been casted to a single-precision float. That
    // is, in this mode static_cast<float>(v) must not be NaN, +Infinity or
    // -Infinity.
    //
    // The result should be interpreted as buffer * 10^(point-length).
    //
    // The output depends on the given mode:
    //  - SHORTEST: produce the least amount of digits for which the internal
    //   identity requirement is still satisfied. In this mode the
    //   'requested_digits' parameter is ignored.
    //  - SHORTEST_SINGLE: same as SHORTEST but with single-precision.
    //  - FIXED: produces digits necessary to print a given number with
    //   'requested_digits' digits after the decimal point. The produced digits
    //   might be too short in which case the caller has to fill the remainder
    //   with '0's. Halfway cases are rounded towards +/-Infinity (away from 0).
    //  - PRECISION: produces 'requested_digits' where the first digit is not '0'.
    //   Even though the length of produced digits usually equals
    //   'requested_digits', the function is allowed to return fewer digits, in
    //   which case the caller has to fill the missing digits with '0's.
    //   Halfway cases are again rounded away from 0.
    // DoubleToAscii expects the given buffer to be big enough to hold all
    // digits and a terminating null-character.
    pub fn double_to_ascii(
        v: f64,
        mode: DtoaMode,
        requested_digits: i32,
        buffer: &mut [u8],
        sign: &mut bool,
        length: &mut i32,
        point: &mut i32,
    ) {
        let mut v = v;
        debug_assert!(!Double::from_f64(v).is_special());
        debug_assert!(
            mode == DtoaMode::SHORTEST || mode == DtoaMode::SHORTEST_SINGLE || requested_digits >= 0
        );

        if Double::from_f64(v).sign() < 0 {
            *sign = true;
            v = -v;
        } else {
            *sign = false;
        }

        if mode == DtoaMode::PRECISION && requested_digits == 0 {
            buffer[0] = 0;
            *length = 0;
            return;
        }

        if v == 0.0 {
            buffer[0] = b'0';
            buffer[1] = 0;
            *length = 1;
            *point = 1;
            return;
        }

        let fast_worked = match mode {
            DtoaMode::SHORTEST => {
                fast_dtoa(v, FastDtoaMode::FAST_DTOA_SHORTEST, 0, buffer, length, point)
            }
            DtoaMode::SHORTEST_SINGLE => {
                fast_dtoa(v, FastDtoaMode::FAST_DTOA_SHORTEST_SINGLE, 0, buffer, length, point)
            }
            DtoaMode::FIXED => fast_fixed_dtoa(v, requested_digits, buffer, length, point),
            DtoaMode::PRECISION => {
                fast_dtoa(v, FastDtoaMode::FAST_DTOA_PRECISION, requested_digits, buffer, length, point)
            }
        };
        if fast_worked {
            return;
        }

        // If the fast dtoa didn't succeed use the slower bignum version.
        let bignum_mode = dtoa_to_bignum_dtoa_mode(mode);
        bignum_dtoa(v, bignum_mode, requested_digits, buffer, length, point);
        buffer[*length as usize] = 0;
    }

    // Implementation for ToShortest and ToShortestSingle.
    fn to_shortest_ieee_number(
        &self,
        value: f64,
        result_builder: &mut StringBuilder,
        mode: DtoaMode,
    ) -> bool {
        debug_assert!(mode == DtoaMode::SHORTEST || mode == DtoaMode::SHORTEST_SINGLE);
        if Double::from_f64(value).is_special() {
            return self.handle_special_values(value, result_builder);
        }

        let mut decimal_point: i32 = 0;
        let mut sign = false;
        const DECIMAL_REP_CAPACITY: usize = (DoubleToStringConverter::BASE10_MAXIMAL_LENGTH + 1) as usize;
        let mut decimal_rep = [0u8; DECIMAL_REP_CAPACITY];
        let mut decimal_rep_length: i32 = 0;

        Self::double_to_ascii(
            value,
            mode,
            0,
            &mut decimal_rep,
            &mut sign,
            &mut decimal_rep_length,
            &mut decimal_point,
        );

        let unique_zero = (self.flags & Self::UNIQUE_ZERO) != 0;
        if sign && (value != 0.0 || !unique_zero) {
            result_builder.add_character(b'-');
        }

        let exponent = decimal_point - 1;
        if valid_shortest_representation(
            exponent,
            self.decimal_in_shortest_low,
            self.decimal_in_shortest_high,
        ) {
            self.create_decimal_representation(
                &decimal_rep[..decimal_rep_length as usize],
                decimal_point,
                max(0, decimal_rep_length - decimal_point),
                result_builder,
            );
        } else {
            self.create_exponential_representation(
                &decimal_rep[..decimal_rep_length as usize],
                exponent,
                result_builder,
            );
        }
        true
    }

    // If the value is a special value (NaN or Infinity) constructs the
    // corresponding string using the configured infinity/nan-symbol.
    // If either of them is NULL or the value is not special then the
    // function returns false.
    fn handle_special_values(&self, value: f64, result_builder: &mut StringBuilder) -> bool {
        let double_inspect = Double::from_f64(value);
        if double_inspect.is_infinity() {
            let Some(infinity_symbol) = self.infinity_symbol else {
                return false;
            };
            if value < 0.0 {
                result_builder.add_character(b'-');
            }
            result_builder.add_string(infinity_symbol);
            return true;
        }
        if double_inspect.is_nan() {
            let Some(nan_symbol) = self.nan_symbol else {
                return false;
            };
            result_builder.add_string(nan_symbol);
            return true;
        }
        false
    }

    // Constructs an exponential representation (i.e. 1.234e56).
    // The given exponent assumes a decimal point after the first decimal digit.
    fn create_exponential_representation(
        &self,
        decimal_digits: &[u8],
        exponent: i32,
        result_builder: &mut StringBuilder,
    ) {
        let mut exponent = exponent;
        debug_assert!(!decimal_digits.is_empty());
        result_builder.add_character(decimal_digits[0]);
        if decimal_digits.len() > 1 {
            result_builder.add_character(b'.');
            result_builder.add_substring_span(&decimal_digits[1..]);
        }
        result_builder.add_character(self.exponent_character);
        if exponent < 0 {
            result_builder.add_character(b'-');
            exponent = -exponent;
        } else if (self.flags & Self::EMIT_POSITIVE_EXPONENT_SIGN) != 0 {
            result_builder.add_character(b'+');
        }
        if exponent == 0 {
            result_builder.add_character(b'0');
            return;
        }
        debug_assert!(exponent < 10000);
        const MAX_EXPONENT_LENGTH: usize = 5;
        let mut buffer = [0u8; MAX_EXPONENT_LENGTH + 1];
        buffer[MAX_EXPONENT_LENGTH] = 0;
        let mut first_char_pos = MAX_EXPONENT_LENGTH;
        while exponent > 0 && first_char_pos > 0 {
            first_char_pos -= 1;
            buffer[first_char_pos] = b'0' + (exponent % 10) as u8;
            exponent /= 10;
        }
        result_builder.add_substring_span(&buffer[first_char_pos..MAX_EXPONENT_LENGTH]);
    }

    // Creates a decimal representation (i.e 1234.5678).
    fn create_decimal_representation(
        &self,
        decimal_digits: &[u8],
        decimal_point: i32,
        digits_after_point: i32,
        result_builder: &mut StringBuilder,
    ) {
        let digits_len = decimal_digits.len() as i32;
        // Create a representation that is padded with zeros if needed.
        if decimal_point <= 0 {
            // "0.00000decimal_rep" or "0.000decimal_rep00".
            result_builder.add_character(b'0');
            if digits_after_point > 0 {
                result_builder.add_character(b'.');
                result_builder.add_padding(b'0', padding_count(-decimal_point));
                debug_assert!(digits_len <= digits_after_point - (-decimal_point));
                result_builder.add_substring_span(decimal_digits);
                let remaining_digits = digits_after_point - (-decimal_point) - digits_len;
                result_builder.add_padding(b'0', padding_count(remaining_digits));
            }
        } else if decimal_point >= digits_len {
            // "decimal_rep0000.00000" or "decimal_rep.0000".
            result_builder.add_substring_span(decimal_digits);
            result_builder.add_padding(b'0', padding_count(decimal_point - digits_len));
            if digits_after_point > 0 {
                result_builder.add_character(b'.');
                result_builder.add_padding(b'0', padding_count(digits_after_point));
            }
        } else {
            // "decima.l_rep000".
            debug_assert!(digits_after_point > 0);
            result_builder.add_substring_span(&decimal_digits[..decimal_point as usize]);
            result_builder.add_character(b'.');
            debug_assert!(digits_len - decimal_point <= digits_after_point);
            result_builder.add_substring_span(&decimal_digits[decimal_point as usize..]);
            let remaining_digits = digits_after_point - (digits_len - decimal_point);
            result_builder.add_padding(b'0', padding_count(remaining_digits));
        }
        if digits_after_point == 0 {
            if (self.flags & Self::EMIT_TRAILING_DECIMAL_POINT) != 0 {
                result_builder.add_character(b'.');
            }
            if (self.flags & Self::EMIT_TRAILING_ZERO_AFTER_POINT) != 0 {
                result_builder.add_character(b'0');
            }
        }
    }

    fn to_fixed_internal(
        &self,
        value: f64,
        requested_digits: i32,
        buffer: &mut [u8],
        result_builder: &mut StringBuilder,
    ) -> bool {
        // Find a sufficiently precise decimal representation of n.
        let mut decimal_point: i32 = 0;
        let mut sign = false;
        let mut decimal_rep_length: i32 = 0;
        Self::double_to_ascii(
            value,
            DtoaMode::FIXED,
            requested_digits,
            buffer,
            &mut sign,
            &mut decimal_rep_length,
            &mut decimal_point,
        );

        let unique_zero = (self.flags & Self::UNIQUE_ZERO) != 0;
        if sign && (value != 0.0 || !unique_zero) {
            result_builder.add_character(b'-');
        }

        self.create_decimal_representation(
            &buffer[..decimal_rep_length as usize],
            decimal_point,
            requested_digits,
            result_builder,
        );
        true
    }
}

fn dtoa_to_bignum_dtoa_mode(dtoa_mode: DtoaMode) -> BignumDtoaMode {
    match dtoa_mode {
        DtoaMode::SHORTEST => BignumDtoaMode::BIGNUM_DTOA_SHORTEST,
        DtoaMode::SHORTEST_SINGLE => BignumDtoaMode::BIGNUM_DTOA_SHORTEST_SINGLE,
        DtoaMode::FIXED => BignumDtoaMode::BIGNUM_DTOA_FIXED,
        DtoaMode::PRECISION => BignumDtoaMode::BIGNUM_DTOA_PRECISION,
    }
}

// Maximum number of significant digits in decimal representation.
// The longest possible double in decimal representation is
// (2^53 - 1) * 2 ^ -1074 that is (2 ^ 53 - 1) * 5 ^ 1074 / 10 ^ 1074
// (768 digits). If we parse a number whose first digits are equal to a
// mean of 2 adjacent doubles (that could have up to 769 digits) the result
// must be rounded to the bigger one unless the tail consists of zeros, so
// we don't need to preserve all the digits.
pub const MAX_SIGNIFICANT_DIGITS: i32 = 772;

fn signed_zero(sign: bool) -> f64 {
    if sign { -0.0 } else { 0.0 }
}

// Returns true, when the iterator is equal to end.
fn advance(it: &mut usize, end: usize) -> bool {
    *it += 1;
    *it == end
}

/// Os dois `StringToFloatingPointType` do C++ (`double` e `float`).
trait FloatingPointType: Copy + Neg<Output = Self> {
    /// Conversão implícita de `double` (`0.0`, `-0.0` e `SignedZero`) para o tipo de retorno.
    fn from_double(value: f64) -> Self;
    fn from_buffer(buffer: &[u8], exponent: i32) -> Self;
}

impl FloatingPointType for f64 {
    fn from_double(value: f64) -> f64 {
        value
    }

    fn from_buffer(buffer: &[u8], exponent: i32) -> f64 {
        strtod(buffer, exponent)
    }
}

impl FloatingPointType for f32 {
    fn from_double(value: f64) -> f32 {
        value as f32
    }

    fn from_buffer(buffer: &[u8], exponent: i32) -> f32 {
        strtof(buffer, exponent)
    }
}

fn string_to_ieee<F: FloatingPointType, C: AsciiChar>(
    input: &[C],
    processed_characters_count: &mut usize,
) -> F {
    let length = input.len();
    let mut current: usize = 0;
    let end: usize = length;

    *processed_characters_count = 0;

    // To make sure that iterator dereferencing is valid the following
    // convention is used:
    // 1. Each '++current' statement is followed by check for equality to 'end'.
    // 3. If 'current' becomes equal to 'end' the function returns or goes to
    // 'parsing_done'.
    // 4. 'current' is not dereferenced after the 'parsing_done' label.
    // 5. Code before 'parsing_done' may rely on 'current != end'.

    if current == end {
        return F::from_double(0.0);
    }

    // The longest form of simplified number is: "-<significant digits>.1eXXX\0".
    const BUFFER_SIZE: usize = MAX_SIGNIFICANT_DIGITS as usize + 10;
    let mut buffer = [0u8; BUFFER_SIZE];
    let mut buffer_pos: usize = 0;

    // Exponent will be adjusted if insignificant digits of the integer part
    // or insignificant leading zeros of the fractional part are dropped.
    let mut exponent: i32 = 0;
    let mut significant_digits: i32 = 0;
    let mut insignificant_digits: i32 = 0;
    let mut nonzero_digit_dropped = false;

    let mut sign = false;

    if input[current].to_u32() == '+' as u32 || input[current].to_u32() == '-' as u32 {
        sign = input[current].to_u32() == '-' as u32;
        current += 1;
        if current == end {
            return F::from_double(0.0);
        }
    }

    let mut leading_zero = false;
    if input[current].to_u32() == '0' as u32 {
        if advance(&mut current, end) {
            *processed_characters_count = current;
            return F::from_double(signed_zero(sign));
        }

        leading_zero = true;

        // Ignore leading zeros in the integer part.
        while input[current].to_u32() == '0' as u32 {
            if advance(&mut current, end) {
                *processed_characters_count = current;
                return F::from_double(signed_zero(sign));
            }
        }
    }

    'parsing_done: {
        // Copy significant digits of the integer part (if any) to the buffer.
        while is_ascii_digit(input[current]) {
            if significant_digits < MAX_SIGNIFICANT_DIGITS {
                debug_assert!(buffer_pos < BUFFER_SIZE);
                buffer[buffer_pos] = input[current].to_u32() as u8;
                buffer_pos += 1;
                significant_digits += 1;
            } else {
                insignificant_digits += 1; // Move the digit into the exponential part.
                nonzero_digit_dropped =
                    nonzero_digit_dropped || input[current].to_u32() != '0' as u32;
            }
            if advance(&mut current, end) {
                break 'parsing_done;
            }
        }

        if input[current].to_u32() == '.' as u32 {
            if advance(&mut current, end) {
                if significant_digits == 0 && !leading_zero {
                    return F::from_double(0.0);
                } else {
                    break 'parsing_done;
                }
            }

            if significant_digits == 0 {
                // Integer part consists of 0 or is absent. Significant digits start after
                // leading zeros (if any).
                while input[current].to_u32() == '0' as u32 {
                    if advance(&mut current, end) {
                        *processed_characters_count = current;
                        return F::from_double(signed_zero(sign));
                    }
                    exponent -= 1; // Move this 0 into the exponent.
                }
            }

            // There is a fractional part.
            // We don't emit a '.', but adjust the exponent instead.
            while is_ascii_digit(input[current]) {
                if significant_digits < MAX_SIGNIFICANT_DIGITS {
                    debug_assert!(buffer_pos < BUFFER_SIZE);
                    buffer[buffer_pos] = input[current].to_u32() as u8;
                    buffer_pos += 1;
                    significant_digits += 1;
                    exponent -= 1;
                } else {
                    // Ignore insignificant digits in the fractional part.
                    nonzero_digit_dropped =
                        nonzero_digit_dropped || input[current].to_u32() != '0' as u32;
                }
                if advance(&mut current, end) {
                    break 'parsing_done;
                }
            }
        }

        if !leading_zero && exponent == 0 && significant_digits == 0 {
            // If leading_zeros is true then the string contains zeros.
            // If exponent < 0 then string was [+-]\.0*...
            // If significant_digits != 0 the string is not equal to 0.
            // Otherwise there are no digits in the string.
            return F::from_double(0.0);
        }

        // Parse exponential part.
        if input[current].to_u32() == 'e' as u32 || input[current].to_u32() == 'E' as u32 {
            current += 1;
            if current == end {
                current -= 1;
                break 'parsing_done;
            }
            let mut exponen_sign: u8 = 0;
            if input[current].to_u32() == '+' as u32 || input[current].to_u32() == '-' as u32 {
                exponen_sign = input[current].to_u32() as u8;
                current += 1;
                if current == end {
                    current -= 2;
                    break 'parsing_done;
                }
            }

            if input[current].to_u32() < '0' as u32 || input[current].to_u32() > '9' as u32 {
                if exponen_sign != 0 {
                    current -= 1;
                }
                current -= 1;
                break 'parsing_done;
            }

            let max_exponent: i32 = i32::MAX / 2;
            debug_assert!(-max_exponent / 2 <= exponent && exponent <= max_exponent / 2);
            let mut num: i32 = 0;
            loop {
                // Check overflow.
                let digit = (input[current].to_u32() - '0' as u32) as i32;
                if num >= max_exponent / 10
                    && !(num == max_exponent / 10 && digit <= max_exponent % 10)
                {
                    num = max_exponent;
                } else {
                    num = num * 10 + digit;
                }
                current += 1;
                if !(current != end && is_ascii_digit(input[current])) {
                    break;
                }
            }

            exponent += if exponen_sign == b'-' { -num } else { num };
        }
    }

    // parsing_done:
    exponent += insignificant_digits;

    if nonzero_digit_dropped {
        buffer[buffer_pos] = b'1';
        buffer_pos += 1;
        exponent -= 1;
    }

    debug_assert!(buffer_pos < BUFFER_SIZE);
    buffer[buffer_pos] = 0;

    let converted = F::from_buffer(&buffer[..buffer_pos], exponent);
    *processed_characters_count = current;
    if sign { -converted } else { converted }
}

pub struct StringToDoubleConverter;

impl StringToDoubleConverter {
    // Performs the conversion.
    // The output parameter 'processed_characters_count' is set to the number
    // of characters that have been processed to read the number. As a
    // template, serves both the 8 bit (`char`) and the 16 bit (`uc16`) overloads
    // of the C++.
    pub fn string_to_double<C: AsciiChar>(
        buffer: &[C],
        processed_characters_count: &mut usize,
    ) -> f64 {
        string_to_ieee::<f64, C>(buffer, processed_characters_count)
    }

    // Same as StringToDouble but reads a float.
    // Note that this is not equivalent to static_cast<float>(StringToDouble(...))
    // due to potential double-rounding.
    pub fn string_to_float<C: AsciiChar>(
        buffer: &[C],
        processed_characters_count: &mut usize,
    ) -> f32 {
        string_to_ieee::<f32, C>(buffer, processed_characters_count)
    }
}
