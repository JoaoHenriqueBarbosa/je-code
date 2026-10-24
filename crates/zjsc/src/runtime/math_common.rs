//! Tradução de `runtime/MathCommon.h` e `MathCommon.cpp`.
//!
//! O que fica de fora:
//!
//! - `JSC_DECLARE_NOEXCEPT_JIT_OPERATION`/`JSC_DEFINE_NOEXCEPT_JIT_OPERATION` são só a decoração de
//!   ABI das entradas chamadas pelo código gerado; não há JIT no porte, e o interpretador chama
//!   direto as funções puras abaixo. As entradas `lowerNameDouble`/`lowerNameFloat` da família
//!   `FOR_EACH_ARITH_UNARY_OP_STD` (`sin`, `sinh`, `cos`, `cosh`, `tan`, `tanh`, `asin`, `asinh`,
//!   `acos`, `acosh`, `atan`, `atanh`, `log`, `log10`, `log2`, `cbrt`, `exp`, `expm1`, `log1p`),
//!   `truncDouble`/`truncFloat`, `ceilDouble`/`ceilFloat`, `floorDouble`/`floorFloat`,
//!   `sqrtDouble`/`sqrtFloat`, `stdPowDouble`/`stdPowFloat` e `fmodDouble` são cada uma só a função
//!   da libm correspondente; no Rust, são os métodos `f64::sin`, ..., `f64::powf` e o operador `%`
//!   (`fmod`), que o chamador usa direto (a camada do `MathObject`).
//! - `roundDouble`, `jsRoundDouble`, `roundFloat` e `jsRound` têm o mesmo corpo: ver `js_round`.
//! - `mathPowInternal` e o ramo `OS(DARWIN) && CPU(ARM_THUMB2)` (`fdlibmPow`, `fdlibmScalbn`):
//!   no Linux x86_64 `mathPowInternal` é `pow`, que entra direto em `operation_math_pow`.
//! - `operationMathPow` e `maxExponentForIntegerMathPow` moram em `ul_common::libm`, com o `pow` do glibc,
//!   porque o transpilador do bun (`wip/zbun`) dobra `a ** b` com a mesma função.
//! - `jsMaxDouble`/`jsMinDouble` têm um ramo `CPU(ARM64)` com assembly; no x86_64 são `fMax`/`fMin`.
//! - `HAVE(FJCVTZS_INSTRUCTION)` (ARM64) em `toInt32`.
//! - `UCPUStrictInt32` é `uint64_t` no alvo de 64 bits (`CPU.h`).
//!
//! `canBeInt32`, `canBeStrictInt32`, `toInt8` e família não existem neste arquivo no WebKit do Bun;
//! `tryConvertToStrictInt32` vive em `wtf::math_extras` (o `MathExtras.h`).

use crate::wtf::math_extras::WtfFloat;

pub use ul_common::libm::operation_math_pow;

pub const fn max_safe_integer() -> f64 {
    // 2 ^ 53 - 1
    9007199254740991.0
}

pub const fn min_safe_integer() -> f64 {
    // -(2 ^ 53 - 1)
    -9007199254740991.0
}

pub const fn max_safe_integer_as_uint64() -> u64 {
    // 2 ^ 53 - 1
    9007199254740991
}

// Use value - trunc(value) == 0.0 which rejects NaN and Infinity without
// an explicit check since NaN - NaN and Inf - Inf both produce NaN.
// (As duas sobrecargas, `double` e `float`, são esta função genérica.)
pub fn is_integer<T: WtfFloat>(value: T) -> bool {
    value - value.trunc() == T::ZERO
}

pub fn is_safe_integer(value: f64) -> bool {
    value.trunc() == value && value.abs() <= max_safe_integer()
}

pub fn is_negative_zero(value: f64) -> bool {
    value.is_sign_negative() && value == 0.0
}

// This in the ToInt32 operation is defined in section 9.5 of the ECMA-262 spec.
// Note that this operation is identical to ToUInt32 other than to interpretation
// of the resulting bit-pattern (as such this method is also called to implement
// ToUInt32).
//
// The operation can be described as round towards zero, then select the 32 or 64 least
// bits of the resulting value in 2s-complement representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToIntMode {
    Generic,
    Int32AfterSensibleConversionAttempt,
}

/// `toIntImpl<Int, Mode>`: `INT_BITS` é 32 (`int32_t`) ou 64 (`int64_t`). O resultado é o `Int`
/// estendido para `i64` (o chamador de 32 bits trunca com `as i32`).
fn to_int_impl<const INT_BITS: u32>(number: f64, mode: ToIntMode) -> i64 {
    const { assert!(INT_BITS == 32 || INT_BITS == 64) };
    let int_bits_minus_one = INT_BITS - 1;
    let width_mask: u64 = if INT_BITS == 64 { u64::MAX } else { (1u64 << INT_BITS) - 1 };

    let bits = number.to_bits();
    let exp = ((bits >> 52) as i32 & 0x7ff) - 0x3ff;

    // If exponent < 0 there will be no bits to the left of the decimal point
    // after rounding; if the exponent is > maxExpForLeftShift then no bits of precision can be
    // left in the low intBits range of the result (IEEE-754 doubles have 52 bits
    // of fractional precision).
    // Note this case handles 0, -0, and all infinite, NaN, & denormal value.
    let max_exp_for_left_shift = int_bits_minus_one + 52;

    // We need to check exp > maxExpForLeftShift because:
    // 1. exp may be used as a left shift value below in (exp - 52), and
    // 2. Left shift amounts that exceed intBitsMinusOne results in undefined behavior.
    //
    // Using an unsigned comparison here also gives us a exp < 0 check for free.
    if (exp as u32) > max_exp_for_left_shift {
        return 0;
    }

    // Select the appropriate intBits from the floating point mantissa. If the
    // exponent is 52 then the bits we need to select are already aligned to the
    // lowest bits of the 64-bit integer representation of the number, no need
    // to shift. If the exponent is greater than 52 we need to shift the value
    // left by (exp - 52), if the value is less than 52 we need to shift right
    // accordingly.
    let mut result: u64 = if exp > 52 { bits << (exp - 52) } else { bits >> (52 - exp) } & width_mask;

    // IEEE-754 double precision values are stored omitting an implicit 1 before
    // the decimal point; we need to reinsert this now. We may also the shifted
    // invalid bits into the result that are not a part of the mantissa (the sign
    // and exponent bits from the floatingpoint representation); mask these out.
    // Note that missingOne should be held as UInt since ((1 << intBitsMinusOne) - 1) causes
    // Int overflow.
    if mode == ToIntMode::Int32AfterSensibleConversionAttempt {
        assert!(int_bits_minus_one == 31);
        if exp == int_bits_minus_one as i32 {
            // This is an optimization for when toInt32() is called in the slow path
            // of a JIT operation. Currently, this optimization is only applicable for
            // x86 ports. On x86, the fast path does a sensible double-to-int32 conversion, by
            // first attempting to truncate the double value to int32 using the
            // cvttsd2si_rr instruction. According to Intel's manual, cvttsd2si performs
            // the following truncate operation:
            //
            //     If src = NaN, +-Inf, or |(src)rz| > 0x7fffffff and (src)rz != 0x80000000,
            //     then the result becomes 0x80000000. Otherwise, the operation succeeds.
            //
            // As a result, the exp of the double is always >= 31. We can take advantage
            // of this by specifically checking for (exp == 31).
            let missing_one: u64 = 1u64 << int_bits_minus_one;
            result &= missing_one - 1;
            result = result.wrapping_add(missing_one) & width_mask;
        }
    } else if exp < INT_BITS as i32 {
        let missing_one: u64 = 1u64 << exp;
        result &= missing_one - 1;
        result = result.wrapping_add(missing_one) & width_mask;
    }

    // If the input value was negative (we could test either 'number' or 'bits',
    // but testing 'bits' is likely faster) invert the result appropriately.
    let value: i64 = if INT_BITS == 32 { (result as u32 as i32) as i64 } else { result as i64 };
    if (bits as i64) < 0 { value.wrapping_neg() } else { value }
}

pub fn to_int32(number: f64) -> i32 {
    to_int_impl::<32>(number, ToIntMode::Generic) as i32
}

// This implements ToUInt32, defined in ECMA-262 9.6.
pub fn to_uint32(number: f64) -> u32 {
    // As commented in the spec, the operation of ToInt32 and ToUint32 only differ
    // in how the result is interpreted; see NOTEs in sections 9.5 and 9.6.
    to_int32(number) as u32
}

/// `toUCPUStrictInt32`: StrictInt32 format requires that higher bits are all zeros even if value
/// is negative.
pub const fn to_ucpu_strict_int32(value: i32) -> u64 {
    (value as u32) as u64
}

// This implementation follows https://tc39.es/ecma262/#sec-touint32 but use int64 instead.
pub fn to_int64(number: f64) -> i64 {
    to_int_impl::<64>(number, ToIntMode::Generic)
}

pub fn to_uint64(number: f64) -> u64 {
    to_int64(number) as u64
}

pub fn safe_reciprocal_for_div_by_const(constant: f64) -> Option<f64> {
    // No "weird" numbers (NaN, Denormal, etc).
    if constant == 0.0 || !constant.is_normal() {
        return None;
    }

    // `std::frexp` de um número normal: a fração em [0.5, 1) (com o sinal) e o expoente.
    let bits = constant.to_bits();
    let fraction = f64::from_bits((bits & 0x800F_FFFF_FFFF_FFFF) | (1022u64 << 52));
    let mut exponent = ((bits >> 52) & 0x7ff) as i32 - 1022;
    if fraction != 0.5 {
        return None;
    }

    // Note that frexp() returns the value divided by two
    // so we to offset this exponent by one.
    exponent -= 1;

    // A double exponent is between -1022 and 1023.
    // Nothing we can do to invert 1023.
    if exponent == 1023 {
        return None;
    }

    // `std::ldexp(1, -exponent)`: -exponent está em [-1022, 1022], então o resultado é normal.
    let reciprocal = f64::from_bits(((1023 - exponent) as u64) << 52);

    Some(reciprocal)
}

/// `operationToInt32`.
pub fn operation_to_int32(value: f64) -> u64 {
    to_ucpu_strict_int32(to_int32(value))
}

/// `operationToInt32SensibleSlow`.
pub fn operation_to_int32_sensible_slow(number: f64) -> u64 {
    to_ucpu_strict_int32(to_int_impl::<32>(number, ToIntMode::Int32AfterSensibleConversionAttempt) as i32)
}

/// `jsRound`, `Math::roundDouble`, `Math::jsRoundDouble` (`roundDoubleImpl`) e, para `f32`,
/// `Math::roundFloat` (`roundFloatImpl`, onde `integer - 0.5 > value` é feito em `double`).
pub fn js_round<T: WtfFloat>(value: T) -> T {
    let integer = value.ceil();
    integer - if integer.to_f64() - 0.5 > value.to_f64() { T::ONE } else { T::ZERO }
}

pub mod math {
    use crate::wtf::math_extras::WtfFloat;

    pub fn f_max<T: WtfFloat>(a: T, b: T) -> T {
        if a.is_nan() || b.is_nan() {
            return a + b;
        }
        if a == T::ZERO && b == T::ZERO && a.is_sign_negative() != b.is_sign_negative() {
            return T::ZERO;
        }
        if a < b { b } else { a }
    }

    pub fn f_min<T: WtfFloat>(a: T, b: T) -> T {
        if a.is_nan() || b.is_nan() {
            return a + b;
        }
        if a == T::ZERO && b == T::ZERO && a.is_sign_negative() != b.is_sign_negative() {
            return -T::ZERO;
        }
        if b < a { b } else { a }
    }

    // No Linux x86_64, `jsMaxDouble` e `jsMinDouble` são `fMax` e `fMin`.
    pub use self::f_max as js_max_double;
    pub use self::f_min as js_min_double;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_int32_cases() {
        assert_eq!(to_int32(4294967296.0 + 5.0), 5);
        assert_eq!(to_uint32(4294967296.0 + 5.0), 5);
        assert_eq!(to_int32(-0.0), 0);
        assert_eq!(to_int32(f64::NAN), 0);
        assert_eq!(to_int32(f64::INFINITY), 0);
        assert_eq!(to_int32(2147483648.0), i32::MIN);
        assert_eq!(to_int32(4294967295.0), -1);
        assert_eq!(to_uint32(-1.0), 4294967295);
        assert_eq!(to_int32(-2147483649.0), i32::MAX);
        assert_eq!(to_int32(1.9), 1);
        assert_eq!(to_int32(-1.9), -1);
        assert_eq!(to_int32(0.5), 0);
        assert_eq!(to_int32(1e20), 1661992960);
        assert_eq!(to_int32(5e-324), 0);
    }

    #[test]
    fn to_int64_cases() {
        assert_eq!(to_int64(-1.0), -1);
        assert_eq!(to_int64(9223372036854775808.0), i64::MIN);
        assert_eq!(to_uint64(18446744073709551615.0), 0);
        assert_eq!(to_int64(f64::NAN), 0);
        assert_eq!(to_int64(-3.7), -3);
        assert_eq!(to_int64(18446744073709551616.0 + 4096.0), 4096);
    }

    #[test]
    fn sensible_slow_matches_generic() {
        for value in [2147483648.0, -2147483649.0, 3e9, -3e9, f64::NAN, f64::INFINITY, 4294967296.0 + 7.0, 2147483648.0 * 3.0] {
            assert_eq!(operation_to_int32_sensible_slow(value), operation_to_int32(value), "{value}");
        }
        assert_eq!(operation_to_int32(-1.0), 0xFFFF_FFFF);
    }

    #[test]
    fn integers() {
        assert!(is_integer(3.0));
        assert!(!is_integer(3.5));
        assert!(!is_integer(f64::NAN));
        assert!(!is_integer(f64::INFINITY));
        assert!(is_integer(-7.0f32));
        assert!(is_safe_integer(9007199254740991.0));
        assert!(!is_safe_integer(9007199254740992.0));
        assert!(is_safe_integer(-0.0));
        assert!(is_negative_zero(-0.0));
        assert!(!is_negative_zero(0.0));
        assert_eq!(max_safe_integer(), 9007199254740991.0);
        assert_eq!(min_safe_integer(), -9007199254740991.0);
        assert_eq!(max_safe_integer_as_uint64() as f64, max_safe_integer());
    }

    #[test]
    fn reciprocal() {
        assert_eq!(safe_reciprocal_for_div_by_const(4.0), Some(0.25));
        assert_eq!(safe_reciprocal_for_div_by_const(0.5), Some(2.0));
        assert_eq!(safe_reciprocal_for_div_by_const(1.0), Some(1.0));
        assert_eq!(safe_reciprocal_for_div_by_const(3.0), None);
        assert_eq!(safe_reciprocal_for_div_by_const(0.0), None);
        assert_eq!(safe_reciprocal_for_div_by_const(f64::NAN), None);
        assert_eq!(safe_reciprocal_for_div_by_const(-4.0), None);
        assert_eq!(safe_reciprocal_for_div_by_const(8.98846567431158e307), None);
    }

    #[test]
    fn pow_and_round() {
        assert_eq!(operation_math_pow(2.0, 10.0), 1024.0);
        assert!(operation_math_pow(1.0, f64::INFINITY).is_nan());
        assert!(operation_math_pow(2.0, f64::NAN).is_nan());
        assert_eq!(operation_math_pow(4.0, 0.5), 2.0);
        assert_eq!(operation_math_pow(0.0, -0.5), f64::INFINITY);
        assert_eq!(operation_math_pow(2.0, -1.0), 0.5);
        assert_eq!(js_round(2.5f64), 3.0);
        assert_eq!(js_round(-2.5f64), -2.0);
        assert_eq!(js_round(0.49999999999999994f64), 0.0);
        assert_eq!(js_round(-0.4f32), 0.0);
        assert_eq!(js_round(1.5f32), 2.0);
    }

    /// Os casos do golden do bun (`tests/golden/math_bun.tsv`): `nome<TAB>argumentos<TAB>resultado`. Conferem
    /// a família exp/log/pow do glibc que mora em `ul_common::libm`, contra o bun do oráculo.
    fn golden_cases() -> Vec<(String, Vec<u64>, u64)> {
        let text = include_str!("../../tests/golden/math_bun.tsv");
        text.lines()
            .filter_map(|line| {
                let fields: Vec<&str> = line.split('\t').collect();
                if fields.len() != 3 {
                    return None;
                }
                let (name, result) = (fields[0], fields[2]);
                // Os argumentos vêm em hexadecimal separados por vírgula.
                let arguments = fields[1].split(',').map(|f| u64::from_str_radix(f, 16).ok()).collect::<Option<Vec<_>>>()?;
                Some((name.to_string(), arguments, u64::from_str_radix(result, 16).ok()?))
            })
            .collect()
    }

    fn canonical(value: f64) -> u64 {
        if value.is_nan() { 0x7ff8_0000_0000_0000 } else { value.to_bits() }
    }

    fn check(name: &str, count: usize, function: impl Fn(&[f64]) -> f64) {
        let cases: Vec<_> = golden_cases().into_iter().filter(|(case, arguments, _)| case == name && arguments.len() == if name == "pow" { 2 } else { 1 }).take(count).collect();
        assert!(!cases.is_empty(), "sem casos para {name}");
        for (_, arguments, expected) in cases {
            let inputs: Vec<f64> = arguments.iter().map(|&bits| f64::from_bits(bits)).collect();
            assert_eq!(canonical(function(&inputs)), expected, "{name}({:x?})", arguments);
        }
    }

    #[test]
    fn glibc_exp_log_pow_match_bun_golden() {
        use ul_common::libm::{exp, log, log2, pow};
        check("exp", 30, |a| exp(a[0]));
        check("log", 30, |a| log(a[0]));
        check("log2", 30, |a| log2(a[0]));
        check("pow", 30, |a| pow(a[0], a[1]));
    }

    #[test]
    fn min_max() {
        assert!(math::f_max(f64::NAN, 1.0).is_nan());
        assert!(math::f_min(0.0f64, -0.0).is_sign_negative());
        assert!(!math::f_max(0.0f64, -0.0).is_sign_negative());
        assert_eq!(math::js_max_double(1.0, 2.0), 2.0);
        assert_eq!(math::js_min_double(1.0, 2.0), 1.0);
    }
}
