//! Porte de `WTF/wtf/dragonbox/dragonbox.h`: o algoritmo principal (Schubfach/Dragonbox) com a
//! configuração de políticas que a WTF usa (ver `mod.rs`), mais `Mode` e os auxiliares de
//! comprimento e de dígitos usados pela impressão.
//!
//! Escolhas fixas das políticas: o intervalo "normal" é `symmetric_boundary` com `is_closed` igual
//! a `has_even_significand_bits()`, o intervalo "mais curto" é `closed`, o arredondamento de
//! binário para decimal é `to_even` (`prefer_round_down` é `significand % 2`), e a política de zeros
//! à direita é `ignore` (`on_trailing_zeros` e `no_trailing_zeros` devolvem o par como veio).

use crate::wtf::dragonbox::detail::cache_holder::CacheHolder;
use crate::wtf::dragonbox::detail::div;
use crate::wtf::dragonbox::detail::log::{
    floor_log10_pow2, floor_log10_pow2_minus_log10_4_over_3, floor_log2_pow10,
};
use crate::wtf::dragonbox::detail::util::compute_power;
use crate::wtf::dragonbox::detail::wuint;
use crate::wtf::dragonbox::ieee754_format::{
    FloatBits, FloatFormat, FloatTraits, Ieee754Binary32, Ieee754Binary64, SignedSignificandBits,
};
use crate::wtf::dtoa::utils::{
    valid_shortest_representation as double_conversion_valid_shortest_representation,
    DEFAULT_DECIMAL_IN_SHORTEST_HIGH, DEFAULT_DECIMAL_IN_SHORTEST_LOW,
};

/// `decimal_fp<carrier_uint, false, false>`: significando e expoente decimais, sem sinal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecimalFp {
    pub significand: u64,
    pub exponent: i32,
}

/// `compute_mul_result`.
pub struct ComputeMulResult {
    pub integer_part: u64,
    pub is_integer: bool,
}

/// `compute_mul_parity_result`.
pub struct ComputeMulParityResult {
    pub parity: bool,
    pub is_integer: bool,
}

/// `impl::compute_mul_impl<format>`: as contas que dependem do formato. O `cache` do binary32
/// (64 bits) vem nos bits baixos do `u128`.
pub trait ComputeMulImpl: FloatFormat + CacheHolder {
    fn compute_mul(u: u64, cache: u128) -> ComputeMulResult;
    fn compute_delta(cache: u128, beta: i32) -> u32;
    fn compute_mul_parity(two_f: u64, cache: u128, beta: i32) -> ComputeMulParityResult;
    fn compute_left_endpoint_for_shorter_interval_case(cache: u128, beta: i32) -> u64;
    fn compute_right_endpoint_for_shorter_interval_case(cache: u128, beta: i32) -> u64;
    fn compute_round_up_for_shorter_interval_case(cache: u128, beta: i32) -> u64;

    /// `div::divide_by_pow10<kappa + 1, carrier_uint, (carrier_uint(1) << (significand_bits + 1))
    /// * big_divisor - 1>`: a divisão por `10^(kappa+1)` na especialização que o C++ escolhe para
    /// o formato (por 100 em 32 bits, por 1000 em 64 bits com o limite que a habilita).
    fn divide_by_big_pow10(n: u64) -> u64;
}

impl ComputeMulImpl for Ieee754Binary32 {
    fn compute_mul(u: u64, cache: u128) -> ComputeMulResult {
        let r = wuint::umul96_upper64(u as u32, cache as u64);
        ComputeMulResult { integer_part: (r >> 32) as u32 as u64, is_integer: (r as u32) == 0 }
    }

    fn compute_delta(cache: u128, beta: i32) -> u32 {
        ((cache as u64) >> (Self::CACHE_BITS - 1 - beta)) as u32
    }

    fn compute_mul_parity(two_f: u64, cache: u128, beta: i32) -> ComputeMulParityResult {
        debug_assert!(beta >= 1);
        debug_assert!(beta < 64);

        let r = wuint::umul96_lower64(two_f as u32, cache as u64);
        ComputeMulParityResult {
            parity: ((r >> (64 - beta)) & 1) != 0,
            is_integer: ((r >> (32 - beta)) as u32) == 0,
        }
    }

    fn compute_left_endpoint_for_shorter_interval_case(cache: u128, beta: i32) -> u64 {
        let cache = cache as u64;
        ((cache.wrapping_sub(cache >> (Self::SIGNIFICAND_BITS + 2)))
            >> (Self::CACHE_BITS - Self::SIGNIFICAND_BITS - 1 - beta)) as u32 as u64
    }

    fn compute_right_endpoint_for_shorter_interval_case(cache: u128, beta: i32) -> u64 {
        let cache = cache as u64;
        ((cache.wrapping_add(cache >> (Self::SIGNIFICAND_BITS + 1)))
            >> (Self::CACHE_BITS - Self::SIGNIFICAND_BITS - 1 - beta)) as u32 as u64
    }

    fn compute_round_up_for_shorter_interval_case(cache: u128, beta: i32) -> u64 {
        let cache = cache as u64;
        ((((cache >> (Self::CACHE_BITS - Self::SIGNIFICAND_BITS - 2 - beta)) as u32)
            .wrapping_add(1))
            / 2) as u64
    }

    fn divide_by_big_pow10(n: u64) -> u64 {
        div::divide_by_pow10_u32_by_100(n as u32) as u64
    }
}

impl ComputeMulImpl for Ieee754Binary64 {
    fn compute_mul(u: u64, cache: u128) -> ComputeMulResult {
        let r = wuint::umul192_upper128(u, cache);
        let r_high = (r >> 64) as u64;
        let r_low = r as u64;
        ComputeMulResult { integer_part: r_high, is_integer: r_low == 0 }
    }

    fn compute_delta(cache: u128, beta: i32) -> u32 {
        let cache_high = (cache >> 64) as u64;
        (cache_high >> (64 - 1 - beta)) as u32
    }

    fn compute_mul_parity(two_f: u64, cache: u128, beta: i32) -> ComputeMulParityResult {
        debug_assert!(beta >= 1);
        debug_assert!(beta < 64);

        let r = wuint::umul192_lower128(two_f, cache);
        let r_high = (r >> 64) as u64;
        let r_low = r as u64;
        ComputeMulParityResult {
            parity: ((r_high >> (64 - beta)) & 1) != 0,
            is_integer: ((r_high << beta) | (r_low >> (64 - beta))) == 0,
        }
    }

    fn compute_left_endpoint_for_shorter_interval_case(cache: u128, beta: i32) -> u64 {
        let cache_high = (cache >> 64) as u64;
        (cache_high.wrapping_sub(cache_high >> (Self::SIGNIFICAND_BITS + 2)))
            >> (64 - Self::SIGNIFICAND_BITS - 1 - beta)
    }

    fn compute_right_endpoint_for_shorter_interval_case(cache: u128, beta: i32) -> u64 {
        let cache_high = (cache >> 64) as u64;
        (cache_high.wrapping_add(cache_high >> (Self::SIGNIFICAND_BITS + 1)))
            >> (64 - Self::SIGNIFICAND_BITS - 1 - beta)
    }

    fn compute_round_up_for_shorter_interval_case(cache: u128, beta: i32) -> u64 {
        let cache_high = (cache >> 64) as u64;
        ((cache_high >> (64 - Self::SIGNIFICAND_BITS - 2 - beta)).wrapping_add(1)) / 2
    }

    fn divide_by_big_pow10(n: u64) -> u64 {
        div::divide_by_pow10_u64_by_1000(n)
    }
}

/// `cache::full::get_cache<FloatFormat>(k)`.
fn get_cache<F: CacheHolder>(k: i32) -> u128 {
    debug_assert!(k >= F::MIN_K && k <= F::MAX_K);
    F::cache_entry((k - F::MIN_K) as usize)
}

/// `binary_to_decimal_rounding::to_even::prefer_round_down`.
fn prefer_round_down(significand: u64) -> bool {
    significand % 2 != 0
}

/// `impl::is_right_endpoint_integer_shorter_interval`.
fn is_right_endpoint_integer_shorter_interval<F: FloatFormat>(exponent: i32) -> bool {
    exponent >= F::CASE_SHORTER_INTERVAL_RIGHT_ENDPOINT_LOWER_THRESHOLD
        && exponent <= F::CASE_SHORTER_INTERVAL_RIGHT_ENDPOINT_UPPER_THRESHOLD
}

/// `impl::is_left_endpoint_integer_shorter_interval`.
fn is_left_endpoint_integer_shorter_interval<F: FloatFormat>(exponent: i32) -> bool {
    exponent >= F::CASE_SHORTER_INTERVAL_LEFT_ENDPOINT_LOWER_THRESHOLD
        && exponent <= F::CASE_SHORTER_INTERVAL_LEFT_ENDPOINT_UPPER_THRESHOLD
}

/// `impl::compute_nearest_normal`. `is_closed` é o argumento adicional do `symmetric_boundary`
/// (`include_left_endpoint()` e `include_right_endpoint()` valem ambos `is_closed`).
fn compute_nearest_normal<F: ComputeMulImpl>(
    two_fc: u64,
    binary_exponent: i32,
    is_closed: bool,
) -> DecimalFp {
    let kappa = F::KAPPA;
    let include_left_endpoint = is_closed;
    let include_right_endpoint = is_closed;

    //////////////////////////////////////////////////////////////////////
    // Step 1: Schubfach multiplier calculation
    //////////////////////////////////////////////////////////////////////

    // Compute k and beta.
    let minus_k = floor_log10_pow2(binary_exponent) - kappa;
    let cache = get_cache::<F>(-minus_k);
    let beta = binary_exponent + floor_log2_pow10(-minus_k);

    // Compute zi and deltai.
    // 10^kappa <= deltai < 10^(kappa + 1)
    let deltai = F::compute_delta(cache, beta);
    // Para o binary32, o teste de inteiro está errado em 29711844 * 2^-82 e 29711844 * 2^-81
    // (os únicos contraexemplos); como 29711844 é par, isso não afeta o cálculo dos extremos, e o
    // ramo que dependeria do teste do centro nunca roda com essas entradas.
    let z_result = F::compute_mul((two_fc | 1) << beta, cache);

    //////////////////////////////////////////////////////////////////////
    // Step 2: Try larger divisor; remove trailing zeros if necessary
    //////////////////////////////////////////////////////////////////////

    let big_divisor = compute_power(kappa + 1, 10) as u32;
    let small_divisor = compute_power(kappa, 10) as u32;

    // Using an upper bound on zi, we might be able to optimize the division
    // better than the compiler; we are computing zi / big_divisor here.
    let mut decimal_significand = F::divide_by_big_pow10(z_result.integer_part);
    let mut r = z_result
        .integer_part
        .wrapping_sub((big_divisor as u64).wrapping_mul(decimal_significand)) as u32;

    'step2: {
        if r < deltai {
            // Exclude the right endpoint if necessary.
            if r == 0 && (z_result.is_integer & !include_right_endpoint) {
                decimal_significand -= 1;
                r = big_divisor;
                break 'step2;
            }
        } else if r > deltai {
            break 'step2;
        } else {
            // r == deltai; compare fractional parts.
            let x_result = F::compute_mul_parity(two_fc - 1, cache, beta);

            if !(x_result.parity | (x_result.is_integer & include_left_endpoint)) {
                break 'step2;
            }
        }

        // We may need to remove trailing zeros.
        return DecimalFp { significand: decimal_significand, exponent: minus_k + kappa + 1 };
    }

    //////////////////////////////////////////////////////////////////////
    // Step 3: Find the significand with the smaller divisor
    //////////////////////////////////////////////////////////////////////

    decimal_significand = decimal_significand.wrapping_mul(10);

    let mut dist = r.wrapping_sub(deltai / 2).wrapping_add(small_divisor / 2);
    let approx_y_parity = ((dist ^ (small_divisor / 2)) & 1) != 0;

    // Is dist divisible by 10^kappa?
    let divisible_by_small_divisor = div::check_divisibility_and_divide_by_pow10(kappa, &mut dist);

    // Add dist / 10^kappa to the significand.
    decimal_significand = decimal_significand.wrapping_add(dist as u64);

    if divisible_by_small_divisor {
        // Check z^(f) >= epsilon^(f).
        // We have either yi == zi - epsiloni or yi == (zi - epsiloni) - 1,
        // where yi == zi - epsiloni if and only if z^(f) >= epsilon^(f).
        // Since there are only 2 possibilities, we only need to care about the
        // parity. Also, zi and r should have the same parity since the divisor
        // is an even number.
        let y_result = F::compute_mul_parity(two_fc, cache, beta);
        if y_result.parity != approx_y_parity {
            decimal_significand -= 1;
        } else {
            // If z^(f) >= epsilon^(f), we might have a tie
            // when z^(f) == epsilon^(f), or equivalently, when y is an integer.
            // For tie-to-up case, we can just choose the upper one.
            if prefer_round_down(decimal_significand) & y_result.is_integer {
                decimal_significand -= 1;
            }
        }
    }
    DecimalFp { significand: decimal_significand, exponent: minus_k + kappa }
}

/// `impl::compute_nearest_shorter`, com o `shorter_interval_type` `closed` (os dois extremos
/// incluídos).
fn compute_nearest_shorter<F: ComputeMulImpl>(binary_exponent: i32) -> DecimalFp {
    let include_left_endpoint = true;
    let include_right_endpoint = true;

    // Compute k and beta.
    let minus_k = floor_log10_pow2_minus_log10_4_over_3(binary_exponent);
    let beta = binary_exponent + floor_log2_pow10(-minus_k);

    // Compute xi and zi.
    let cache = get_cache::<F>(-minus_k);

    let mut xi = F::compute_left_endpoint_for_shorter_interval_case(cache, beta);
    let mut zi = F::compute_right_endpoint_for_shorter_interval_case(cache, beta);

    // If we don't accept the right endpoint and
    // if the right endpoint is an integer, decrease it.
    if !include_right_endpoint && is_right_endpoint_integer_shorter_interval::<F>(binary_exponent) {
        zi -= 1;
    }
    // If we don't accept the left endpoint or
    // if the left endpoint is not an integer, increase it.
    if !include_left_endpoint || !is_left_endpoint_integer_shorter_interval::<F>(binary_exponent) {
        xi += 1;
    }

    // Try bigger divisor.
    let mut decimal_significand = zi / 10;

    // If succeed, remove trailing zeros if necessary and return.
    if decimal_significand * 10 >= xi {
        return DecimalFp { significand: decimal_significand, exponent: minus_k + 1 };
    }

    // Otherwise, compute the round-up of y.
    decimal_significand = F::compute_round_up_for_shorter_interval_case(cache, beta);

    // When tie occurs, choose one of them according to the rule.
    if prefer_round_down(decimal_significand)
        && binary_exponent >= F::SHORTER_INTERVAL_TIE_LOWER_THRESHOLD
        && binary_exponent <= F::SHORTER_INTERVAL_TIE_UPPER_THRESHOLD
    {
        decimal_significand -= 1;
    } else if decimal_significand < xi {
        decimal_significand += 1;
    }
    DecimalFp { significand: decimal_significand, exponent: minus_k }
}

/// `to_decimal(signed_significand_bits, exponent_bits, ...)` na configuração da WTF (`to_decimal_impl`
/// com `decimal_to_binary_rounding::nearest_to_even`, `sign::ignore`, `trailing_zero::ignore`).
pub fn to_decimal<T: FloatTraits>(
    signed_significand_bits: SignedSignificandBits<T>,
    exponent_bits: u32,
) -> DecimalFp
where
    T::Format: ComputeMulImpl,
{
    let mut two_fc = signed_significand_bits.remove_sign_bit_and_shift();
    let mut exponent = exponent_bits as i32;

    // Is the input a normal number?
    if exponent != 0 {
        exponent += <T::Format as FloatFormat>::EXPONENT_BIAS
            - <T::Format as FloatFormat>::SIGNIFICAND_BITS;

        // Shorter interval case; proceed like Schubfach.
        // One might think this condition is wrong, since when exponent_bits ==
        // 1 and two_fc == 0, the interval is actually regular. However, it
        // turns out that this seemingly wrong condition is actually fine,
        // because the end result is anyway the same.
        if two_fc == 0 {
            return compute_nearest_shorter::<T::Format>(exponent);
        }

        two_fc |= 1u64 << (<T::Format as FloatFormat>::SIGNIFICAND_BITS + 1);
    } else {
        // Is the input a subnormal number?
        exponent = <T::Format as FloatFormat>::MIN_EXPONENT
            - <T::Format as FloatFormat>::SIGNIFICAND_BITS;
    }

    compute_nearest_normal::<T::Format>(
        two_fc,
        exponent,
        signed_significand_bits.has_even_significand_bits(),
    )
}

/// `to_decimal(Float x, ...)`: pré-condição, `x` finito.
pub fn to_decimal_float<T: FloatTraits>(x: T) -> DecimalFp
where
    T::Format: ComputeMulImpl,
{
    let br = FloatBits::<T>::new(x);
    let exponent_bits = br.extract_exponent_bits();
    let s = br.remove_exponent_bits(exponent_bits);
    debug_assert!(br.is_finite());

    to_decimal::<T>(s, exponent_bits)
}

// ------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    ToShortest = 1,
    ToExponential = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PrintTrailingZero {
    Yes,
    No,
}

/// `to_exponential_max_string_length<FloatFormat>()`: tamanho máximo do buffer no modo
/// exponencial, sem o terminador.
///
/// - binary32: sinal(1) + significando(9) + ponto decimal(1) + marcador do expoente(1) + sinal do
///   expoente(1) + expoente(2).
/// - binary64: sinal(1) + significando(17) + ponto decimal(1) + marcador do expoente(1) + sinal do
///   expoente(1) + expoente(3).
pub const fn to_exponential_max_string_length<F: FloatFormat>() -> usize {
    if F::DECIMAL_DIGITS == 9 { 1 + 9 + 1 + 1 + 1 + 2 } else { 1 + 17 + 1 + 1 + 1 + 3 }
}

/// `to_shortest_max_string_length<FloatFormat>()`.
pub const fn to_shortest_max_string_length<F: FloatFormat>() -> usize {
    let decimal_in_shortest_low = DEFAULT_DECIMAL_IN_SHORTEST_LOW;
    let decimal_in_shortest_high = DEFAULT_DECIMAL_IN_SHORTEST_HIGH;
    assert!(decimal_in_shortest_low <= 0);
    assert!(decimal_in_shortest_high >= F::DECIMAL_DIGITS);

    // sinal(1) + significando(dígitos) + ponto decimal(1) + max(-low, high - dígitos).
    let a = -decimal_in_shortest_low;
    let b = decimal_in_shortest_high - F::DECIMAL_DIGITS;
    1 + F::DECIMAL_DIGITS as usize + 1 + (if a > b { a } else { b }) as usize
}

/// `max_string_length<FloatFormat>()`: tamanho máximo exigido do buffer, sem o terminador.
pub const fn max_string_length<F: FloatFormat>() -> usize {
    let a = to_exponential_max_string_length::<F>();
    let b = to_shortest_max_string_length::<F>();
    if a > b { a } else { b }
}

/// `valid_shortest_representation(decimal_point)`.
pub const fn valid_shortest_representation(decimal_point: i32) -> bool {
    let decimal_in_shortest_low = DEFAULT_DECIMAL_IN_SHORTEST_LOW;
    let decimal_in_shortest_high = DEFAULT_DECIMAL_IN_SHORTEST_HIGH;

    let exponent = decimal_point - 1;
    double_conversion_valid_shortest_representation(
        exponent,
        decimal_in_shortest_low,
        decimal_in_shortest_high,
    )
}

pub fn count_digits_base10_with_max_17(v: u64) -> u32 {
    if v >= 10000000000000000 {
        return 17;
    }
    if v >= 1000000000000000 {
        return 16;
    }
    if v >= 100000000000000 {
        return 15;
    }
    if v >= 10000000000000 {
        return 14;
    }
    if v >= 1000000000000 {
        return 13;
    }
    if v >= 100000000000 {
        return 12;
    }
    if v >= 10000000000 {
        return 11;
    }
    if v >= 1000000000 {
        return 10;
    }
    if v >= 100000000 {
        return 9;
    }
    if v >= 10000000 {
        return 8;
    }
    if v >= 1000000 {
        return 7;
    }
    if v >= 100000 {
        return 6;
    }
    if v >= 10000 {
        return 5;
    }
    if v >= 1000 {
        return 4;
    }
    if v >= 100 {
        return 3;
    }
    if v >= 10 {
        return 2;
    }
    1
}

/// `compute_power_with_max_16(base, k)`: `base^k` para `0 < k < 17`, zero fora disso.
pub fn compute_power_with_max_16(base: u64, k: u32) -> u64 {
    debug_assert!(0 < k && (k as i32) < Ieee754Binary64::DECIMAL_DIGITS);
    match k {
        1..=16 => compute_power(k as i32, base),
        _ => 0,
    }
}
