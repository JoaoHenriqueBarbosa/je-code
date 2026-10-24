//! Porte de `WTF/wtf/fast_float/fast_float.h`, seção `FASTFLOAT_DECIMAL_TO_BINARY_H` (linhas 3245
//! a 3467 do amálgama).
//!
//! Mapeamentos:
//!
//! - `powers::smallest_power_of_five` e `powers::power_of_five_128` vêm de `fast_table`
//!   (`SMALLEST_POWER_OF_FIVE` e `POWERS_OF_FIVE_128`).
//! - `compute_product_approximation<bit_precision>` recebe `bit_precision` como argumento comum:
//!   em Rust o valor `binary::mantissa_explicit_bits() + 3` não é constante em contexto genérico.
//!   O `static_assert` da faixa `[0, 64]` vira `debug_assert!`.
//! - `template <typename binary>` vira genérico sobre o trait `BinaryFormat`.
//! - A aritmética sem sinal e a de `int` do C++ que pode estourar vira `wrapping_*`.
#![allow(non_camel_case_types, non_upper_case_globals)]

use super::fast_table::{POWERS_OF_FIVE_128, SMALLEST_POWER_OF_FIVE};
use super::float_common::{
    adjusted_mantissa, full_multiplication, leading_zeroes, value128, BinaryFormat, INVALID_AM_BIAS,
};

/// This will compute or rather approximate w * 5**q and return a pair of 64-bit words
/// approximating the result, with the "high" part corresponding to the most significant bits and
/// the low part corresponding to the least significant bits.
pub fn compute_product_approximation(bit_precision: i32, q: i64, w: u64) -> value128 {
    let index: usize = (2 * (q.wrapping_sub(SMALLEST_POWER_OF_FIVE as i64) as i32)) as usize;
    // For small values of q, e.g., q in [0,27], the answer is always exact because The line
    // value128 firstproduct = full_multiplication(w, power_of_five_128[index]); gives the exact
    // answer.
    let mut firstproduct: value128 = full_multiplication(w, POWERS_OF_FIVE_128[index]);
    debug_assert!((0..=64).contains(&bit_precision), " precision should  be in (0,64]");
    let precision_mask: u64 = if bit_precision < 64 {
        0xFFFFFFFFFFFFFFFFu64 >> (bit_precision as u32)
    } else {
        0xFFFFFFFFFFFFFFFFu64
    };
    if (firstproduct.high & precision_mask) == precision_mask {
        // could further guard with (lower + w < lower)
        // regarding the second product, we only need secondproduct.high, but our expectation is
        // that the compiler will optimize this extra work away if needed.
        let secondproduct: value128 = full_multiplication(w, POWERS_OF_FIVE_128[index + 1]);
        firstproduct.low = firstproduct.low.wrapping_add(secondproduct.high);
        if secondproduct.high > firstproduct.low {
            firstproduct.high = firstproduct.high.wrapping_add(1);
        }
    }
    firstproduct
}

pub mod detail {
    /// For q in (0,350), we have that
    ///  f = (((152170 + 65536) * q ) >> 16);
    /// is equal to
    ///   floor(p) + q
    /// where
    ///   p = log(5**q)/log(2) = q * log(5)/log(2)
    ///
    /// For negative values of q in (-400,0), we have that
    ///  f = (((152170 + 65536) * q ) >> 16);
    /// is equal to
    ///   -ceil(p) + q
    /// where
    ///   p = log(5**-q)/log(2) = -q * log(5)/log(2)
    pub const fn power(q: i32) -> i32 {
        ((152170i32 + 65536i32).wrapping_mul(q) >> 16).wrapping_add(63)
    }
}

/// create an adjusted mantissa, biased by the invalid power2 for significant digits already
/// multiplied by 10 ** q.
pub fn compute_error_scaled<binary: BinaryFormat>(q: i64, w: u64, lz: i32) -> adjusted_mantissa {
    let hilz: i32 = ((w >> 63) as i32) ^ 1;
    let mut answer = adjusted_mantissa::default();
    answer.mantissa = w << (hilz as u32);
    let bias: i32 = binary::mantissa_explicit_bits() - binary::minimum_exponent();
    answer.power2 = detail::power(q as i32)
        .wrapping_add(bias)
        .wrapping_sub(hilz)
        .wrapping_sub(lz)
        .wrapping_sub(62)
        .wrapping_add(INVALID_AM_BIAS);
    answer
}

/// w * 10 ** q, without rounding the representation up. the power2 in the exponent will be
/// adjusted by invalid_am_bias.
pub fn compute_error<binary: BinaryFormat>(q: i64, mut w: u64) -> adjusted_mantissa {
    let lz: i32 = leading_zeroes(w);
    w = w.wrapping_shl(lz as u32);
    let product: value128 =
        compute_product_approximation(binary::mantissa_explicit_bits() + 3, q, w);
    compute_error_scaled::<binary>(q, product.high, lz)
}

/// Computers w * 10 ** q. The returned value should be a valid number that simply needs to be
/// packed. However, in some very rare cases, the computation will fail. In such cases, we return
/// an adjusted_mantissa with a negative power of 2: the caller should recompute in such cases.
pub fn compute_float<binary: BinaryFormat>(q: i64, mut w: u64) -> adjusted_mantissa {
    let mut answer = adjusted_mantissa::default();
    if (w == 0) || (q < binary::smallest_power_of_ten() as i64) {
        answer.power2 = 0;
        answer.mantissa = 0;
        // result should be zero
        return answer;
    }
    if q > binary::largest_power_of_ten() as i64 {
        // we want to get infinity:
        answer.power2 = binary::infinite_power();
        answer.mantissa = 0;
        return answer;
    }
    // At this point in time q is in [powers::smallest_power_of_five,
    // powers::largest_power_of_five].

    // We want the most significant bit of i to be 1. Shift if needed.
    let lz: i32 = leading_zeroes(w);
    w = w.wrapping_shl(lz as u32);

    // The required precision is binary::mantissa_explicit_bits() + 3 because
    // 1. We need the implicit bit
    // 2. We need an extra bit for rounding purposes
    // 3. We might lose a bit due to the "upperbit" routine (result too small, requiring a shift)

    let product: value128 =
        compute_product_approximation(binary::mantissa_explicit_bits() + 3, q, w);
    // The computed 'product' is always sufficient.
    // Mathematical proof:
    // Noble Mushtak and Daniel Lemire, Fast Number Parsing Without Fallback (to appear) See
    // script/mushtak_lemire.py

    // The "compute_product_approximation" function can be slightly slower than a branchless
    // approach: value128 product = compute_product(q, w); but in practice, we can win big with the
    // compute_product_approximation if its additional branch is easily predicted. Which is best is
    // data specific.
    let upperbit: i32 = (product.high >> 63) as i32;
    let shift: i32 = upperbit + 64 - binary::mantissa_explicit_bits() - 3;

    answer.mantissa = product.high >> (shift as u32);

    answer.power2 = detail::power(q as i32)
        .wrapping_add(upperbit)
        .wrapping_sub(lz)
        .wrapping_sub(binary::minimum_exponent());
    if answer.power2 <= 0 {
        // we have a subnormal?
        // Here have that answer.power2 <= 0 so -answer.power2 >= 0
        if -answer.power2 + 1 >= 64 {
            // if we have more than 64 bits below the minimum exponent, you have a zero for sure.
            answer.power2 = 0;
            answer.mantissa = 0;
            // result should be zero
            return answer;
        }
        // next line is safe because -answer.power2 + 1 < 64
        answer.mantissa >>= (-answer.power2 + 1) as u32;
        // Thankfully, we can't have both "round-to-even" and subnormals because "round-to-even"
        // only occurs for powers close to 0 in the 32-bit and and 64-bit case (with no more than
        // 19 digits).
        answer.mantissa = answer.mantissa.wrapping_add(answer.mantissa & 1); // round up
        answer.mantissa >>= 1;
        // There is a weird scenario where we don't have a subnormal but just.
        // Suppose we start with 2.2250738585072013e-308, we end up
        // with 0x3fffffffffffff x 2^-1023-53 which is technically subnormal
        // whereas 0x40000000000000 x 2^-1023-53  is normal. Now, we need to round
        // up 0x3fffffffffffff x 2^-1023-53  and once we do, we are no longer
        // subnormal, but we can only know this after rounding.
        // So we only declare a subnormal if we are smaller than the threshold.
        answer.power2 =
            if answer.mantissa < (1u64 << (binary::mantissa_explicit_bits() as u32)) { 0 } else { 1 };
        return answer;
    }

    // usually, we round *up*, but if we fall right in between and and we have an even basis, we
    // need to round down
    // We are only concerned with the cases where 5**q fits in single 64-bit word.
    if (product.low <= 1)
        && (q >= binary::min_exponent_round_to_even() as i64)
        && (q <= binary::max_exponent_round_to_even() as i64)
        && ((answer.mantissa & 3) == 1)
    {
        // we may fall between two floats!
        // To be in-between two floats we need that in doing
        //   answer.mantissa = product.high >> (upperbit + 64 -
        //   binary::mantissa_explicit_bits() - 3);
        // ... we dropped out only zeroes. But if this happened, then we can go
        // back!!!
        if (answer.mantissa << (shift as u32)) == product.high {
            answer.mantissa &= !1u64; // flip it so that we do not round up
        }
    }

    answer.mantissa = answer.mantissa.wrapping_add(answer.mantissa & 1); // round up
    answer.mantissa >>= 1;
    if answer.mantissa >= (2u64 << (binary::mantissa_explicit_bits() as u32)) {
        answer.mantissa = 1u64 << (binary::mantissa_explicit_bits() as u32);
        answer.power2 = answer.power2.wrapping_add(1); // undo previous addition
    }

    answer.mantissa &= !(1u64 << (binary::mantissa_explicit_bits() as u32));
    if answer.power2 >= binary::infinite_power() {
        // infinity
        answer.power2 = binary::infinite_power();
        answer.mantissa = 0;
    }
    answer
}
