//! Tradução de `WTF/wtf/MathExtras.h`.
//!
//! Os templates do C++ sobre tipos inteiros viram genéricos sobre o trait `WtfInt` (os dez inteiros
//! primitivos, com as operações de largura fixa e a aritmética que dá a volta, como a dos tipos sem
//! sinal do C++); os templates sobre ponto flutuante viram genéricos sobre `WtfFloat` (`f32`/`f64`).
//!
//! Não portado, por escolha da própria regra de fidelidade:
//!
//! - `wtf_atan2` (só `OS(WINDOWS)`);
//! - as conversões de unidade de ângulo (`deg2rad` e família, `radiansPerDegreeDouble` e família):
//!   são usadas só pelo CSS/transformações do WebCore, nenhum arquivo do JavaScriptCore as chama;
//! - `isNaNConstExpr` para inteiro (devolve sempre `false`, e o chamador inteiro não precisa dela);
//! - `roundUpToMultipleOfNonPowerOfTwo(Checked, Checked)`, que depende de `CheckedArithmetic.h`;
//! - as versões de ponteiro de `roundUpToMultipleOf`/`roundDownToMultipleOf` (Rust seguro não faz
//!   aritmética sobre endereço);
//! - os ramos de `CPU(ARM64)`, `HAVE(FJCVTZS_INSTRUCTION)` e o genérico de outras CPUs: o alvo é
//!   `x86_64`, onde as truncações têm a semântica do `cvttsd2si`/`cvttss2si`;
//! - `WTF_PROVEN_TRUE` (dica de constante em tempo de compilação, sem efeito observável).
//!
//! `roundevenf` é `roundeven::<f32>`; `get_lsb_set` é `ctz`; `roundUpToMultipleOfImpl` está
//! absorvido em `round_up_to_multiple_of`.

use std::ops::{Add, Div, Mul, Neg, Sub};

pub const PI_OVER_TWO_DOUBLE: f64 = std::f64::consts::PI / 2.0;
pub const PI_OVER_TWO_FLOAT: f32 = PI_OVER_TWO_DOUBLE as f32;

pub const PI_OVER_FOUR_DOUBLE: f64 = std::f64::consts::PI / 4.0;
pub const PI_OVER_FOUR_FLOAT: f32 = PI_OVER_FOUR_DOUBLE as f32;

/// Os inteiros primitivos, com o que os templates de `MathExtras.h` pedem de `T`.
pub trait WtfInt: Copy + Eq + Ord {
    /// `countOfBits<T>`.
    const COUNT_OF_BITS: u32;
    /// `std::is_signed_v<T>`.
    const IS_SIGNED: bool;
    /// `countOfMagnitudeBits<T>`.
    const COUNT_OF_MAGNITUDE_BITS: u32;
    const ZERO: Self;
    const ONE: Self;
    /// `std::numeric_limits<T>::min()`.
    const MIN: Self;
    /// `std::numeric_limits<T>::max()`.
    const MAX: Self;

    fn wrapping_add(self, other: Self) -> Self;
    fn wrapping_sub(self, other: Self) -> Self;
    fn wrapping_mul(self, other: Self) -> Self;
    fn wrapping_neg(self) -> Self;
    fn bit_and(self, other: Self) -> Self;
    fn bit_not(self) -> Self;
    /// `value << amount` (os bits que saem somem; quantidade maior ou igual à largura dá zero).
    fn shl(self, amount: u32) -> Self;
    /// `value >> amount` (aritmético para tipo com sinal).
    fn shr(self, amount: u32) -> Self;
    /// `a / b`.
    fn divide(self, other: Self) -> Self;
    /// `a % b`.
    fn modulo(self, other: Self) -> Self;
    /// `std::countl_zero(unsignedCast(value))`.
    fn leading_zeros(self) -> u32;
    /// `std::countr_zero(unsignedCast(value))`.
    fn trailing_zeros(self) -> u32;
    /// `std::popcount(unsignedCast(value))`.
    fn count_ones(self) -> u32;
    /// `static_cast<T>(uint64_t)`: trunca para a largura do tipo.
    fn from_u64(value: u64) -> Self;
    /// `static_cast<uint64_t>(T)`: estende o sinal em tipo com sinal.
    fn to_u64(self) -> u64;
    fn to_i128(self) -> i128;
    /// `static_cast<T>(value)` a partir de um inteiro largo: trunca para a largura do tipo.
    fn from_i128(value: i128) -> Self;
    /// `factor && !(value % factor)` depois das conversões aritméticas usuais do C++ entre `T` e
    /// `unsigned`, com `factor` diferente de zero.
    fn is_divisible_by_u32(self, factor: u32) -> bool;
}

macro_rules! impl_wtf_int {
    ($($t:ty, $signed:expr, $promoted:ty;)*) => {
        $(
            impl WtfInt for $t {
                const COUNT_OF_BITS: u32 = <$t>::BITS;
                const IS_SIGNED: bool = $signed;
                const COUNT_OF_MAGNITUDE_BITS: u32 = <$t>::BITS - ($signed as u32);
                const ZERO: Self = 0;
                const ONE: Self = 1;
                const MIN: Self = <$t>::MIN;
                const MAX: Self = <$t>::MAX;

                fn wrapping_add(self, other: Self) -> Self {
                    <$t>::wrapping_add(self, other)
                }
                fn wrapping_sub(self, other: Self) -> Self {
                    <$t>::wrapping_sub(self, other)
                }
                fn wrapping_mul(self, other: Self) -> Self {
                    <$t>::wrapping_mul(self, other)
                }
                fn wrapping_neg(self) -> Self {
                    <$t>::wrapping_neg(self)
                }
                fn bit_and(self, other: Self) -> Self {
                    self & other
                }
                fn bit_not(self) -> Self {
                    !self
                }
                fn shl(self, amount: u32) -> Self {
                    if amount >= <$t>::BITS { 0 } else { self << amount }
                }
                #[allow(unused_comparisons)]
                fn shr(self, amount: u32) -> Self {
                    if amount >= <$t>::BITS {
                        if self < 0 { !0 } else { 0 }
                    } else {
                        self >> amount
                    }
                }
                fn divide(self, other: Self) -> Self {
                    self / other
                }
                fn modulo(self, other: Self) -> Self {
                    self % other
                }
                fn leading_zeros(self) -> u32 {
                    <$t>::leading_zeros(self)
                }
                fn trailing_zeros(self) -> u32 {
                    <$t>::trailing_zeros(self)
                }
                fn count_ones(self) -> u32 {
                    <$t>::count_ones(self)
                }
                fn from_u64(value: u64) -> Self {
                    value as $t
                }
                fn to_u64(self) -> u64 {
                    self as u64
                }
                fn to_i128(self) -> i128 {
                    self as i128
                }
                fn from_i128(value: i128) -> Self {
                    value as $t
                }
                fn is_divisible_by_u32(self, factor: u32) -> bool {
                    (self as $promoted) % (factor as $promoted) == 0
                }
            }
        )*
    };
}

impl_wtf_int! {
    u8, false, u32;
    u16, false, u32;
    u32, false, u32;
    u64, false, u64;
    usize, false, u64;
    i8, true, u32;
    i16, true, u32;
    i32, true, u32;
    i64, true, i64;
    isize, true, i64;
}

/// `std::floating_point` com o que os templates pedem de `T`.
pub trait WtfFloat:
    Copy
    + PartialOrd
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
{
    const ZERO: Self;
    const ONE: Self;
    const HALF: Self;
    /// `std::numeric_limits<T>::max()`.
    const MAX: Self;
    /// `std::numeric_limits<T>::min()` (o menor positivo normal).
    const MIN_POSITIVE: Self;
    /// `std::numeric_limits<T>::epsilon()`.
    const EPSILON: Self;
    /// `std::numeric_limits<T>::quiet_NaN()`.
    const QUIET_NAN: Self;

    fn floor(self) -> Self;
    fn ceil(self) -> Self;
    fn round(self) -> Self;
    fn trunc(self) -> Self;
    fn abs(self) -> Self;
    fn fmod(self, other: Self) -> Self;
    fn copysign(self, sign: Self) -> Self;
    fn is_nan(self) -> bool;
    fn is_infinite(self) -> bool;
    fn is_sign_negative(self) -> bool;
    /// Alargamento exato para `double`.
    fn to_f64(self) -> f64;
    /// `static_cast<Self>(value)` a partir do tipo de destino de um `clampTo`.
    fn from_clamp_target<T: ClampTarget>(value: T) -> Self;
}

macro_rules! impl_wtf_float {
    ($($t:ty, $from_target:ident;)*) => {
        $(
            impl WtfFloat for $t {
                const ZERO: Self = 0.0;
                const ONE: Self = 1.0;
                const HALF: Self = 0.5;
                const MAX: Self = <$t>::MAX;
                const MIN_POSITIVE: Self = <$t>::MIN_POSITIVE;
                const EPSILON: Self = <$t>::EPSILON;
                const QUIET_NAN: Self = <$t>::NAN;

                fn floor(self) -> Self {
                    <$t>::floor(self)
                }
                fn ceil(self) -> Self {
                    <$t>::ceil(self)
                }
                fn round(self) -> Self {
                    <$t>::round(self)
                }
                fn trunc(self) -> Self {
                    <$t>::trunc(self)
                }
                fn abs(self) -> Self {
                    <$t>::abs(self)
                }
                fn fmod(self, other: Self) -> Self {
                    self % other
                }
                fn copysign(self, sign: Self) -> Self {
                    <$t>::copysign(self, sign)
                }
                fn is_nan(self) -> bool {
                    <$t>::is_nan(self)
                }
                fn is_infinite(self) -> bool {
                    <$t>::is_infinite(self)
                }
                fn is_sign_negative(self) -> bool {
                    <$t>::is_sign_negative(self)
                }
                fn to_f64(self) -> f64 {
                    self as f64
                }
                fn from_clamp_target<T: ClampTarget>(value: T) -> Self {
                    value.$from_target()
                }
            }
        )*
    };
}

impl_wtf_float! {
    f32, as_f32;
    f64, as_f64;
}

/// `fabsConstExpr`.
pub fn fabs_const_expr<T: WtfFloat>(value: T) -> T {
    if value != value {
        return value;
    }
    if value == T::ZERO {
        return T::ZERO; // -0.0 should be converted to +0.0
    }
    if value < T::ZERO {
        return -value;
    }
    value
}

/// `isNaNConstExpr` para ponto flutuante.
pub fn is_nan_const_expr<T: WtfFloat>(value: T) -> bool {
    value != value
}

pub fn round_towards_positive_infinity<T: WtfFloat>(value: T) -> T {
    (value + T::HALF).floor()
}

/// `roundeven` (e `roundevenf`, que é `roundeven::<f32>`): o polyfill de C23 do ramo que não é ARM64.
pub fn roundeven<T: WtfFloat>(value: T) -> T {
    let rounded = value.round();
    if (value - rounded).abs() == T::HALF {
        let two = T::ONE + T::ONE;
        if rounded.fmod(two) != T::ZERO {
            // copysign is needed for the tie that rounds to zero: -0.5 lands on
            // -1.0 - -1.0, which is +0.0, but roundeven(-0.5) is -0.0.
            return (rounded - T::ONE.copysign(value)).copysign(value);
        }
    }
    rounded
}

/// O tipo de destino de um `clampTo`: inteiros e `f32`/`f64`, com os limites padrão do C++
/// (`defaultMinimumForClamp`: o menor positivo NÃO serve em ponto flutuante, então é `-max`).
pub trait ClampTarget: Copy + PartialOrd {
    const DEFAULT_MINIMUM_FOR_CLAMP: Self;
    const DEFAULT_MAXIMUM_FOR_CLAMP: Self;
    /// `static_cast<float>(self)`.
    fn as_f32(self) -> f32;
    /// `static_cast<double>(self)`.
    fn as_f64(self) -> f64;
    /// `static_cast<Self>(double)`.
    fn from_f64(value: f64) -> Self;
}

macro_rules! impl_clamp_target {
    ($($t:ty, $min:expr;)*) => {
        $(
            impl ClampTarget for $t {
                const DEFAULT_MINIMUM_FOR_CLAMP: Self = $min;
                const DEFAULT_MAXIMUM_FOR_CLAMP: Self = <$t>::MAX;
                fn as_f32(self) -> f32 {
                    self as f32
                }
                fn as_f64(self) -> f64 {
                    self as f64
                }
                fn from_f64(value: f64) -> Self {
                    value as $t
                }
            }
        )*
    };
}

impl_clamp_target! {
    u8, u8::MIN;
    u16, u16::MIN;
    u32, u32::MIN;
    u64, u64::MIN;
    usize, usize::MIN;
    i8, i8::MIN;
    i16, i16::MIN;
    i32, i32::MIN;
    i64, i64::MIN;
    isize, isize::MIN;
    f32, -f32::MAX;
    f64, -f64::MAX;
}

// `clampTo` tem sete sobrecargas no C++, escolhidas pelos tipos de origem e destino. Os limites
// padrão (`defaultMinimumForClamp`/`defaultMaximumForClamp`) são `ClampTarget::DEFAULT_*_FOR_CLAMP`,
// e o chamador os passa quando o C++ usa o argumento padrão.

/// Sobrecarga "mesmo tipo de entrada e saída".
pub fn clamp_to_same<T: PartialOrd + Copy>(value: T, min: T, max: T) -> T {
    if value >= max {
        return max;
    }
    if value <= min {
        return min;
    }
    value
}

/// Sobrecarga "origem em ponto flutuante" (destino diferente da origem e que não seja ponto
/// flutuante mais largo, ver `clamp_to_wider_float`).
pub fn clamp_to_from_float<T: ClampTarget, S: WtfFloat>(value: S, min: T, max: T) -> T {
    if value >= S::from_clamp_target(max) {
        return max;
    }
    // This will return min if value is NaN.
    if !(value > S::from_clamp_target(min)) {
        return min;
    }
    T::from_f64(value.to_f64())
}

/// Sobrecarga "origem `float`, destino `double`" (destino ponto flutuante mais largo que a origem).
pub fn clamp_to_wider_float(value: f32, min: f64, max: f64) -> f64 {
    let converted_value = value as f64;
    if converted_value >= max {
        return max;
    }
    if converted_value <= min {
        return min;
    }
    converted_value
}

/// As sobrecargas entre inteiros (mesmo sinal com origem maior ou igual, sem sinal para com sinal,
/// com sinal para sem sinal do mesmo tamanho ou maior). Todas se reduzem ao limite matemático, porque
/// o `static_cast<SourceType>(max)` do C++ é exato em todas elas.
pub fn clamp_to_int<T: WtfInt, S: WtfInt>(value: S, min: T, max: T) -> T {
    let wide_value = value.to_i128();
    if wide_value >= max.to_i128() {
        return max;
    }
    if wide_value <= min.to_i128() {
        return min;
    }
    T::from_i128(wide_value)
}

pub fn clamp_to_unsigned(value: f64) -> u32 {
    clamp_to_from_float(value, u32::DEFAULT_MINIMUM_FOR_CLAMP, u32::DEFAULT_MAXIMUM_FOR_CLAMP)
}

pub fn clamp_to_float(value: f64) -> f32 {
    clamp_to_from_float(value, f32::DEFAULT_MINIMUM_FOR_CLAMP, f32::DEFAULT_MAXIMUM_FOR_CLAMP)
}

pub fn clamp_to_positive_integer(value: f64) -> i32 {
    clamp_to_from_float(value, 0, i32::DEFAULT_MAXIMUM_FOR_CLAMP)
}

/// Explicitly accept 64bit result when clamping double value.
/// Keep in mind that double can only represent 53bit integer precisely.
pub fn clamp_to_accepting_64<T: ClampTarget>(value: f64, min: T, max: T) -> T {
    if value >= max.as_f64() {
        max
    } else if value <= min.as_f64() {
        min
    } else {
        T::from_f64(value)
    }
}

pub fn is_within_int_range(x: f32) -> bool {
    x > (i32::MIN as f32) && x < (i32::MAX as f32)
}

pub fn normalized_float(value: f32) -> f32 {
    if value > 0.0 && value < f32::MIN_POSITIVE {
        return f32::MIN_POSITIVE;
    }
    if value < 0.0 && value > -f32::MIN_POSITIVE {
        return -f32::MIN_POSITIVE;
    }
    value
}

pub fn has_one_bit_set<T: WtfInt>(value: T) -> bool {
    value.wrapping_sub(T::ONE).bit_and(value) == T::ZERO && value != T::ZERO
}

pub fn has_zero_or_one_bits_set<T: WtfInt>(value: T) -> bool {
    value.wrapping_sub(T::ONE).bit_and(value) == T::ZERO
}

pub fn has_two_or_more_bits_set<T: WtfInt>(value: T) -> bool {
    !has_zero_or_one_bits_set(value)
}

/// "Determine if a word has a zero byte" at https://graphics.stanford.edu/~seander/bithacks.html
/// (formula credited there to Alan Mycroft, comp.lang.c, April 27 1987).
pub fn has_zero_byte<T: WtfInt>(value: T) -> bool {
    assert!(!T::IS_SIGNED);
    let low_bits = T::MAX.divide(T::from_u64(0xFF));
    let high_bits = low_bits.wrapping_mul(T::from_u64(0x80));
    value.wrapping_sub(low_bits).bit_and(value.bit_not()).bit_and(high_bits) != T::ZERO
}

/// "Interleave bits by Binary Magic Numbers" at https://graphics.stanford.edu/~seander/bithacks.html.
pub const fn zero_extend_bytes_to_halfwords(value: u32) -> u64 {
    let mut result = value as u64;
    result = (result | (result << 16)) & 0x0000_FFFF_0000_FFFF;
    result = (result | (result << 8)) & 0x00FF_00FF_00FF_00FF;
    result
}

pub fn divide_rounded_up<T: WtfInt>(a: T, b: T) -> T {
    // Mathematically equivalent to (a + b - 1) / b, but does not overflow
    // when a is close to the maximum representable value of T.
    let remainder_flag = if a.modulo(b) != T::ZERO { T::ONE } else { T::ZERO };
    a.divide(b).wrapping_add(remainder_flag)
}

pub fn times_three_plus_one_divided_by_two<T: WtfInt>(value: T) -> T {
    // Mathematically equivalent to:
    //   (value * 3 + 1) / 2;
    // or:
    //   (unsigned)ceil(value * 1.5));
    // This form is not prone to internal overflow.
    value.wrapping_add(value.shr(1)).wrapping_add(value.bit_and(T::ONE))
}

pub fn is_not_zero_and_ordered<T: WtfFloat>(value: T) -> bool {
    value > T::ZERO || value < T::ZERO
}

pub fn is_zero_or_unordered<T: WtfFloat>(value: T) -> bool {
    !is_not_zero_and_ordered(value)
}

pub fn is_greater_than_non_zero_power_of_two<T: WtfInt>(value: T, power: u32) -> bool {
    // The crazy way of testing of index >= 2 ** power
    // (where I use ** to denote pow()).
    value.shr(1).shr(power.wrapping_sub(1)) != T::ZERO
}

pub fn is_multiple_of<T: WtfInt>(factor: u32, value: T) -> bool {
    factor != 0 && value.is_divisible_by_u32(factor)
}

pub fn is_in_range<T: PartialOrd>(a: &T, min: &T, max: &T) -> bool {
    a >= min && a <= max
}

/// `decomposeDouble`: devolve `(sign, exponent, mantissa)`, interpretado como
/// `(sign ? -1 : 1) * pow(2, exponent) * (mantissa / (1 << 52))`.
/// O número tem de ser finito.
pub fn decompose_double(number: f64) -> (bool, i32, u64) {
    assert!(number.is_finite());

    let sign = number.is_sign_negative();

    let bits = number.to_bits();
    let mut exponent = ((bits >> 52) as i32 & 0x7ff) - 0x3ff;
    let mut mantissa = bits & 0xF_FFFF_FFFF_FFFF;

    // Check for zero/denormal values; if so, adjust the exponent,
    // if not insert the implicit, omitted leading 1 bit.
    if exponent == -0x3ff {
        exponent = if mantissa != 0 { -0x3fe } else { 0 };
    } else {
        mantissa |= 0x10_0000_0000_0000;
    }
    (sign, exponent, mantissa)
}

/// `countOfBits<T>`.
pub const fn count_of_bits<T: WtfInt>() -> u32 {
    T::COUNT_OF_BITS
}

/// `countOfMagnitudeBits<T>`.
pub const fn count_of_magnitude_bits<T: WtfInt>() -> u32 {
    T::COUNT_OF_MAGNITUDE_BITS
}

pub const fn power_of_two(e: u32) -> f32 {
    let mut remaining = e;
    let mut p = 1.0f32;
    while remaining != 0 {
        remaining -= 1;
        p *= 2.0;
    }
    p
}

/// `maxPlusOne<T>`.
pub const fn max_plus_one<T: WtfInt>() -> f32 {
    power_of_two(count_of_magnitude_bits::<T>())
}

/// Calculate d % 2^{64}.
pub fn double_to_integer(d: f64) -> u64 {
    if d.is_nan() || d.is_infinite() {
        0
    } else {
        // -2^{64} < fmodValue < 2^{64}.
        let fmod_value = d.trunc() % (max_plus_one::<u64>() as f64);
        if fmod_value >= 0.0 {
            // 0 <= fmodValue < 2^{64}.
            // 0 <= value < 2^{64}. This cast causes no loss.
            fmod_value as u64
        } else {
            // -2^{64} < fmodValue < 0.
            // 0 < fmodValueInUnsignedLongLong < 2^{64}. This cast causes no loss.
            let fmod_value_in_unsigned_long_long = (-fmod_value) as u64;
            // -1 < (std::numeric_limits<unsigned long long>::max() - fmodValueInUnsignedLongLong) < 2^{64} - 1.
            // 0 < value < 2^{64}.
            (u64::MAX - fmod_value_in_unsigned_long_long).wrapping_add(1)
        }
    }
}

/// `roundUpToPowerOfTwo` (`std::bit_ceil`).
pub fn round_up_to_power_of_two<T: WtfInt>(v: T) -> T {
    T::from_u64(v.to_u64().checked_next_power_of_two().unwrap_or(0))
}

/// `isPowerOfTwo` (`std::has_single_bit`).
pub fn is_power_of_two<T: WtfInt>(value: T) -> bool {
    value.count_ones() == 1
}

pub fn mask_for_size(size: u32) -> u32 {
    if size == 0 {
        return 0;
    }
    round_up_to_power_of_two(size).wrapping_sub(1)
}

pub const fn fast_log2(i: u32) -> u32 {
    if i == 0 {
        return 0;
    }
    const UNSIGNED_BIT_WIDTH: u32 = u32::BITS - 1;
    let mut log2 = UNSIGNED_BIT_WIDTH - i.leading_zeros();
    if i & (i - 1) != 0 {
        log2 += 1;
    }
    log2
}

/// A sobrecarga de `fastLog2` para `uint64_t`.
pub const fn fast_log2_u64(value: u64) -> u32 {
    let high = (value >> 32) as u32;
    if high != 0 {
        return fast_log2(high) + 32;
    }
    fast_log2(value as u32)
}

pub fn safe_fp_division<T: WtfFloat>(u: T, v: T) -> T {
    // Protect against overflow / underflow.
    if v < T::ONE && u > v * T::MAX {
        return T::MAX;
    }
    if v > T::ONE && u < v * T::MIN_POSITIVE {
        return T::ZERO;
    }
    u / v
}

// Floating point numbers comparison:
// u is "essentially equal" [1][2] to v if: | u - v | / |u| <= e and | u - v | / |v| <= e
//
// [1] Knuth, D. E. "Accuracy of Floating Point Arithmetic." The Art of Computer Programming. 3rd ed. Vol. 2.
//     Boston: Addison-Wesley, 1998. 229-45.
// [2] http://www.boost.org/doc/libs/1_34_0/libs/test/doc/components/test_tools/floating_point_comparison.html
/// O `epsilon` padrão do C++ é `T::EPSILON`.
pub fn are_essentially_equal<T: WtfFloat>(u: T, v: T, epsilon: T) -> bool {
    if u == v {
        return true;
    }

    let delta = (u - v).abs();
    safe_fp_division(delta, u.abs()) <= epsilon && safe_fp_division(delta, v.abs()) <= epsilon
}

/// Match behavior of Math.min, where NaN is returned if either argument is NaN.
pub fn nan_propagating_min<T: WtfFloat>(a: T, b: T) -> T {
    if is_nan_const_expr(a) || is_nan_const_expr(b) {
        T::QUIET_NAN
    } else if b < a {
        b
    } else {
        a
    }
}

/// Match behavior of Math.max, where NaN is returned if either argument is NaN.
pub fn nan_propagating_max<T: WtfFloat>(a: T, b: T) -> T {
    if is_nan_const_expr(a) || is_nan_const_expr(b) {
        T::QUIET_NAN
    } else if a < b {
        b
    } else {
        a
    }
}

pub fn is_integral(value: f32) -> bool {
    !value.is_infinite() && value.trunc() == value
}

pub fn increment_with_saturation<T: WtfInt>(value: &mut T) {
    if *value != T::MAX {
        *value = value.wrapping_add(T::ONE);
    }
}

/// O `max` padrão do C++ é `T::MAX`.
pub fn left_shift_with_saturation<T: WtfInt>(value: T, shift_amount: u32, max: T) -> T {
    let result = value.shl(shift_amount);
    // We will have saturated if shifting right doesn't recover the original value.
    if result.shr(shift_amount) != value {
        return max;
    }
    if result > max {
        return max;
    }
    result
}

/// Check if two ranges overlap assuming that neither range is empty.
pub fn non_empty_ranges_overlap<T: PartialOrd>(left_min: T, left_max: T, right_min: T, right_max: T) -> bool {
    left_max > right_min && right_max > left_min
}

/// Pass ranges with the min being inclusive and the max being exclusive. For example, this should
/// return false:
///
/// ```text
/// ranges_overlap(0, 8, 8, 16)
/// ```
pub fn ranges_overlap<T: PartialOrd>(left_min: T, left_max: T, right_min: T, right_max: T) -> bool {
    // Empty ranges interfere with nothing.
    if left_min == left_max {
        return false;
    }
    if right_min == right_max {
        return false;
    }

    non_empty_ranges_overlap(left_min, left_max, right_min, right_max)
}

/// `shuffleVector(vector, size, randomFunc)`.
pub fn shuffle_vector_with_size<T, F: Fn(usize) -> usize>(vector: &mut [T], size: usize, random_func: F) {
    let mut i = 0;
    while i + 1 < size {
        let j = i + random_func(size - i);
        vector.swap(i, j);
        i += 1;
    }
}

/// `shuffleVector(vector, randomFunc)`.
pub fn shuffle_vector<T, F: Fn(usize) -> usize>(vector: &mut [T], random_func: F) {
    let size = vector.len();
    shuffle_vector_with_size(vector, size, random_func);
}

pub fn clz<T: WtfInt>(value: T) -> u32 {
    value.leading_zeros()
}

pub fn ctz<T: WtfInt>(value: T) -> u32 {
    value.trailing_zeros()
}

pub use self::ctz as get_lsb_set;

pub fn get_msb_set<T: WtfInt>(t: T) -> u32 {
    T::COUNT_OF_BITS - 1 - clz(t)
}

pub fn reverse_bits32(value: u32) -> u32 {
    let mut value = value;
    value = ((value & 0xaaaaaaaa) >> 1) | ((value & 0x55555555) << 1);
    value = ((value & 0xcccccccc) >> 2) | ((value & 0x33333333) << 2);
    value = ((value & 0xf0f0f0f0) >> 4) | ((value & 0x0f0f0f0f) << 4);
    value = ((value & 0xff00ff00) >> 8) | ((value & 0x00ff00ff) << 8);
    (value >> 16) | (value << 16)
}

/// For use in places where we could negate std::numeric_limits<T>::min and would like to avoid UB.
pub fn negate<T: WtfInt>(v: T) -> T {
    v.wrapping_neg()
}

/// As sobrecargas de `isIdentical` (`int32_t`, `int64_t`, `double`, `float`): igualdade de bits.
pub trait Identical: Copy {
    fn is_identical(self, other: Self) -> bool;
}

impl Identical for i32 {
    fn is_identical(self, other: Self) -> bool {
        self == other
    }
}

impl Identical for i64 {
    fn is_identical(self, other: Self) -> bool {
        self == other
    }
}

impl Identical for usize {
    fn is_identical(self, other: Self) -> bool {
        self == other
    }
}

impl Identical for f64 {
    fn is_identical(self, other: Self) -> bool {
        self.to_bits() == other.to_bits()
    }
}

impl Identical for f32 {
    fn is_identical(self, other: Self) -> bool {
        self.to_bits() == other.to_bits()
    }
}

/// `isRepresentableAs<ResultType>(value)`: converte para `R` e de volta, e vê se perdeu bits. Os tipos
/// de origem são os das sobrecargas do C++ (`int32_t`, `int64_t`, `size_t`, `double`). Uso:
/// `IsRepresentableAs::<u8>::is_representable_as(valor)`.
pub trait IsRepresentableAs<R> {
    fn is_representable_as(self) -> bool;
}

macro_rules! impl_is_representable_as {
    ($src:ty; $($res:ty),*) => {
        $(
            impl IsRepresentableAs<$res> for $src {
                fn is_representable_as(self) -> bool {
                    // Convert the original value to the desired result type.
                    let result = self as $res;
                    // Convert the converted value back to the original type. The original value is
                    // representable using the new type if such round-tripping doesn't lose bits.
                    let new_value = result as $src;
                    Identical::is_identical(self, new_value)
                }
            }
        )*
    };
}

impl_is_representable_as!(i32; i8, i16, i32, i64, u8, u16, u32, u64, usize, isize, f32, f64);
impl_is_representable_as!(i64; i8, i16, i32, i64, u8, u16, u32, u64, usize, isize, f32, f64);
impl_is_representable_as!(usize; i8, i16, i32, i64, u8, u16, u32, u64, usize, isize, f32, f64);
impl_is_representable_as!(f64; i8, i16, i32, i64, u8, u16, u32, u64, usize, isize, f32, f64);

/// Efficient implementation that takes advantage of powers of two. (`roundUpToMultipleOf(divisor, x)`,
/// já com o `roundUpToMultipleOfImpl` dentro.)
pub fn round_up_to_multiple_of<T: WtfInt>(divisor: usize, x: T) -> T {
    let remainder_mask = T::from_u64(divisor as u64).wrapping_sub(T::ONE);
    x.wrapping_add(remainder_mask).bit_and(remainder_mask.bit_not())
}

/// `roundUpToMultipleOf<divisor>(size_t x)`.
pub fn round_up_to_multiple_of_const<const DIVISOR: usize>(x: usize) -> usize {
    const {
        assert!(DIVISOR != 0 && DIVISOR.is_power_of_two());
    }
    round_up_to_multiple_of(DIVISOR, x)
}

pub fn round_up_to_multiple_of_non_power_of_two<T: WtfInt>(divisor: usize, x: T) -> T {
    let remainder = x.to_u64() % (divisor as u64);
    if remainder == 0 {
        return x;
    }
    x.wrapping_add(T::from_u64((divisor as u64) - remainder))
}

/// Returns positive distance to next multiple of a power-of-two divisor.
pub fn distance_to_multiple_of<const DIVISOR: usize>(x: usize) -> usize {
    const {
        assert!(DIVISOR != 0 && DIVISOR.is_power_of_two());
    }
    (DIVISOR - (x % DIVISOR)) % DIVISOR
}

/// `roundDownToMultipleOf(divisor, x)`: `T` tem a largura de `uintptr_t`.
pub fn round_down_to_multiple_of<T: WtfInt>(divisor: usize, x: T) -> T {
    const {
        assert!(T::COUNT_OF_BITS == usize::BITS);
    }
    x.bit_and(T::from_u64(!(divisor as u64).wrapping_sub(1)))
}

/// `roundDownToMultipleOf<divisor>(x)`.
pub fn round_down_to_multiple_of_const<const DIVISOR: usize, T: WtfInt>(x: T) -> T {
    const {
        assert!(DIVISOR.is_power_of_two(), "'divisor' must be a power of two.");
    }
    round_down_to_multiple_of(DIVISOR, x)
}

// The following truncation helpers perform a direct hardware truncation of
// floating-point values to integer types. Unlike ECMAScript ToInt32/ToInt64
// (modular wrap-around), these produce architecture-defined results for
// out-of-range inputs (e.g., NaN, infinity, values exceeding the target
// range). Aqui, a arquitetura é a `x86_64`: `cvttsd2si`/`cvttss2si` devolvem o "inteiro indefinido"
// (o menor valor do tipo com sinal) quando o resultado não cabe.

/// `truncateDoubleToInt32`: `_mm_cvttsd_si32`. Mora em `ul_common::libm`, onde o `operationMathPow` a usa.
pub use ul_common::libm::truncate_double_to_int32;

/// `truncateDoubleToInt64`: `_mm_cvttsd_si64`.
pub fn truncate_double_to_int64(number: f64) -> i64 {
    if number >= -9223372036854775808.0 && number < 9223372036854775808.0 {
        number as i64
    } else {
        i64::MIN
    }
}

/// `truncateDoubleToUint32`: os 32 bits baixos do `cvttsd2si` de 64 bits.
pub fn truncate_double_to_uint32(number: f64) -> u32 {
    truncate_double_to_int64(number) as u32
}

/// `truncateDoubleToUint64`.
pub fn truncate_double_to_uint64(number: f64) -> u64 {
    // Branchless conversion matching compiler codegen for static_cast<uint64_t>(double).
    // cvttsd2si returns 0x8000000000000000 (negative) on overflow, including for
    // values >= 2^63. When that happens, subtract 2^63 and convert again; the
    // arithmetic-right-shift mask selects the adjusted result only on overflow.
    const TWO_TO_63: f64 = 9223372036854775808.0; // 0x43e0000000000000
    let direct = truncate_double_to_int64(number);
    let shifted = number - TWO_TO_63;
    let from_shifted = truncate_double_to_int64(shifted);
    let mask = direct >> 63;
    ((from_shifted & mask) | direct) as u64
}

// As versões de `float` têm o mesmo resultado que as de `double` aplicadas ao valor alargado (o
// alargamento é exato e as faixas válidas são as mesmas).

/// `truncateFloatToInt32`: `_mm_cvttss_si32`.
pub fn truncate_float_to_int32(number: f32) -> i32 {
    truncate_double_to_int32(number as f64)
}

/// `truncateFloatToInt64`: `_mm_cvttss_si64`.
pub fn truncate_float_to_int64(number: f32) -> i64 {
    truncate_double_to_int64(number as f64)
}

/// `truncateFloatToUint32`.
pub fn truncate_float_to_uint32(number: f32) -> u32 {
    truncate_float_to_int64(number) as u32
}

/// `truncateFloatToUint64`.
pub fn truncate_float_to_uint64(number: f32) -> u64 {
    truncate_double_to_uint64(number as f64)
}

/// tryConvertToStrictInt32: Attempts to convert a double to int32_t, returning
/// None if the value is not exactly representable as int32 (including
/// -0.0, NaN, Infinity, non-integer values, and out-of-range values).
pub fn try_convert_to_strict_int32(value: f64) -> Option<i32> {
    if value.is_infinite() || value.is_nan() {
        return None;
    }

    // Note that -0.0 is not StrictInt32.
    let as_int32 = truncate_double_to_int32(value);
    if !((as_int32 as f64) != value || (as_int32 == 0 && value.is_sign_negative())) {
        return Some(as_int32);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_x86_semantics() {
        assert_eq!(truncate_double_to_int64(f64::NAN), i64::MIN);
        assert_eq!(truncate_double_to_int64(1e20), i64::MIN);
        assert_eq!(truncate_double_to_int64(-1e20), i64::MIN);
        assert_eq!(truncate_double_to_int64(-3.9), -3);
        assert_eq!(truncate_double_to_int64(9223372036854775807.0), i64::MIN);
        assert_eq!(truncate_double_to_int32(f64::INFINITY), i32::MIN);
        assert_eq!(truncate_double_to_int32(2147483647.9), 2147483647);
        assert_eq!(truncate_double_to_int32(-2147483648.9), i32::MIN);
        assert_eq!(truncate_double_to_int32(2147483648.0), i32::MIN);
        assert_eq!(truncate_double_to_uint32(4294967295.0), 4294967295);
        assert_eq!(truncate_double_to_uint64(18446744073709549568.0), 18446744073709549568);
        assert_eq!(truncate_double_to_uint64(9223372036854775808.0), 9223372036854775808);
        assert_eq!(truncate_float_to_int32(-2147483648.0), i32::MIN);
    }

    #[test]
    fn strict_int32() {
        assert_eq!(try_convert_to_strict_int32(5.0), Some(5));
        assert_eq!(try_convert_to_strict_int32(-0.0), None);
        assert_eq!(try_convert_to_strict_int32(0.0), Some(0));
        assert_eq!(try_convert_to_strict_int32(1.5), None);
        assert_eq!(try_convert_to_strict_int32(f64::NAN), None);
        assert_eq!(try_convert_to_strict_int32(2147483648.0), None);
        assert_eq!(try_convert_to_strict_int32(-2147483648.0), Some(i32::MIN));
    }

    #[test]
    fn clamp() {
        assert_eq!(clamp_to_unsigned(-5.0), 0);
        assert_eq!(clamp_to_unsigned(1e20), u32::MAX);
        assert_eq!(clamp_to_unsigned(f64::NAN), 0);
        assert_eq!(clamp_to_positive_integer(-3.0), 0);
        assert_eq!(clamp_to_positive_integer(12.7), 12);
        assert_eq!(clamp_to_int::<i32, i64>(1 << 40, i32::MIN, i32::MAX), i32::MAX);
        assert_eq!(clamp_to_int::<u32, i64>(-4, 0, u32::MAX), 0);
        assert_eq!(clamp_to_int::<i32, u64>(7, i32::MIN, i32::MAX), 7);
        assert_eq!(clamp_to_float(1e300), f32::MAX);
    }

    #[test]
    fn bits() {
        assert_eq!(clz(1u32), 31);
        assert_eq!(ctz(8u64), 3);
        assert_eq!(get_lsb_set(12u32), 2);
        assert_eq!(get_msb_set(12u32), 3);
        assert_eq!(clz(-1i32), 0);
        assert!(is_power_of_two(64usize));
        assert!(!is_power_of_two(0u32));
        assert!(has_one_bit_set(16u32));
        assert!(!has_one_bit_set(0u32));
        assert!(has_zero_or_one_bits_set(0u8));
        assert!(has_two_or_more_bits_set(3u8));
        assert_eq!(round_up_to_power_of_two(5u32), 8);
        assert_eq!(round_up_to_power_of_two(0u32), 1);
        assert_eq!(mask_for_size(5), 7);
        assert_eq!(mask_for_size(0), 0);
        assert_eq!(fast_log2(1), 0);
        assert_eq!(fast_log2(5), 3);
        assert_eq!(fast_log2_u64(1 << 40), 40);
        assert_eq!(reverse_bits32(1), 0x8000_0000);
        assert!(has_zero_byte(0x1100_2233u32));
        assert!(!has_zero_byte(0x1111_2233u32));
        assert_eq!(negate(i32::MIN), i32::MIN);
    }

    #[test]
    fn rounding() {
        assert_eq!(round_up_to_multiple_of(16, 17usize), 32);
        assert_eq!(round_up_to_multiple_of(16, 32usize), 32);
        assert_eq!(round_up_to_multiple_of_const::<8>(9), 16);
        assert_eq!(round_up_to_multiple_of_non_power_of_two(10, 21usize), 30);
        assert_eq!(round_down_to_multiple_of(16, 33usize), 32);
        assert_eq!(distance_to_multiple_of::<16>(17), 15);
        assert_eq!(distance_to_multiple_of::<16>(32), 0);
        assert_eq!(divide_rounded_up(7u32, 2), 4);
        assert_eq!(times_three_plus_one_divided_by_two(5u32), 8);
        assert!(is_multiple_of(4, 12usize));
        assert!(!is_multiple_of(0, 12usize));
    }

    #[test]
    fn saturation() {
        let mut value = u8::MAX;
        increment_with_saturation(&mut value);
        assert_eq!(value, u8::MAX);
        assert_eq!(left_shift_with_saturation(3u32, 31, u32::MAX), u32::MAX);
        assert_eq!(left_shift_with_saturation(3u32, 2, u32::MAX), 12);
        assert!(is_greater_than_non_zero_power_of_two(8u32, 3));
        assert!(!is_greater_than_non_zero_power_of_two(7u32, 3));
    }

    #[test]
    fn floats() {
        assert_eq!(roundeven(0.5f64), 0.0);
        assert!(roundeven(-0.5f64).is_sign_negative());
        assert_eq!(roundeven(1.5f64), 2.0);
        assert_eq!(roundeven(2.5f64), 2.0);
        assert_eq!(roundeven(-2.5f32), -2.0);
        assert_eq!(round_towards_positive_infinity(-2.5f64), -2.0);
        assert!(nan_propagating_min(f64::NAN, 1.0).is_nan());
        assert_eq!(nan_propagating_max(2.0f64, 1.0), 2.0);
        assert!(is_integral(3.0));
        assert!(!is_integral(f32::INFINITY));
        assert!(is_within_int_range(5.0));
        assert!(!is_within_int_range(3e9));
        assert!(are_essentially_equal(1.0f64, 1.0 + f64::EPSILON / 2.0, f64::EPSILON));
        assert_eq!(fabs_const_expr(-0.0f64).to_bits(), 0.0f64.to_bits());
        assert!(is_zero_or_unordered(f64::NAN));
        assert!(is_not_zero_and_ordered(-1.0f64));
        assert_eq!(normalized_float(1e-45), f32::MIN_POSITIVE);
    }

    #[test]
    fn decompose_and_conversion() {
        assert_eq!(decompose_double(1.0), (false, 0, 0x10_0000_0000_0000));
        assert_eq!(decompose_double(-0.0), (true, 0, 0));
        assert_eq!(double_to_integer(-1.0), u64::MAX);
        assert_eq!(double_to_integer(f64::NAN), 0);
        assert_eq!(double_to_integer(18446744073709551616.0 + 4096.0 * 4096.0), 4096 * 4096);
        assert_eq!(power_of_two(10), 1024.0);
        assert_eq!(max_plus_one::<u8>(), 256.0);
        assert_eq!(max_plus_one::<i32>(), 2147483648.0);
        assert_eq!(zero_extend_bytes_to_halfwords(0x0102_0304), 0x0001_0002_0003_0004);
    }

    #[test]
    fn identical_and_representable() {
        assert!(Identical::is_identical(0.0f64, 0.0));
        assert!(!Identical::is_identical(0.0f64, -0.0));
        assert!(IsRepresentableAs::<u8>::is_representable_as(255i32));
        assert!(!IsRepresentableAs::<u8>::is_representable_as(256i32));
        assert!(!IsRepresentableAs::<i32>::is_representable_as(-0.0f64));
        assert!(IsRepresentableAs::<i32>::is_representable_as(12.0f64));
    }

    #[test]
    fn ranges_and_shuffle() {
        assert!(!ranges_overlap(0, 8, 8, 16));
        assert!(ranges_overlap(0, 9, 8, 16));
        assert!(!ranges_overlap(3, 3, 0, 10));
        assert!(is_in_range(&5, &1, &9));
        let mut values = [1, 2, 3, 4];
        shuffle_vector(&mut values, |bound| bound - 1);
        assert_eq!(values, [4, 1, 2, 3]);
    }
}
