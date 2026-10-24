//! Porte de `WTF/wtf/dragonbox/detail/log.h`: logaritmos rápidos, `floor(e * c - s)` por
//! multiplicação e deslocamento aritmético.

/// `compute<m, f, k, e_min, e_max>(e)`: `(e * m - f) >> k`.
const fn compute(m: u32, f: u32, k: u32, e_min: i32, e_max: i32, e: i32) -> i32 {
    debug_assert!(e_min <= e && e <= e_max);
    (e * (m as i32) - (f as i32)) >> k
}

/// `floor_log2(n)`: devolve -1 para `n == 0`.
pub const fn floor_log2(mut n: u64) -> i32 {
    let mut count = -1;
    while n != 0 {
        count += 1;
        n >>= 1;
    }
    count
}

pub const FLOOR_LOG10_POW2_MIN_EXPONENT: i32 = -2620;
pub const FLOOR_LOG10_POW2_MAX_EXPONENT: i32 = 2620;
pub const fn floor_log10_pow2(e: i32) -> i32 {
    compute(315653, 0, 20, FLOOR_LOG10_POW2_MIN_EXPONENT, FLOOR_LOG10_POW2_MAX_EXPONENT, e)
}

pub const FLOOR_LOG2_POW10_MIN_EXPONENT: i32 = -1233;
pub const FLOOR_LOG2_POW10_MAX_EXPONENT: i32 = 1233;
pub const fn floor_log2_pow10(e: i32) -> i32 {
    compute(1741647, 0, 19, FLOOR_LOG2_POW10_MIN_EXPONENT, FLOOR_LOG2_POW10_MAX_EXPONENT, e)
}

pub const FLOOR_LOG10_POW2_MINUS_LOG10_4_OVER_3_MIN_EXPONENT: i32 = -2985;
pub const FLOOR_LOG10_POW2_MINUS_LOG10_4_OVER_3_MAX_EXPONENT: i32 = 2936;
pub const fn floor_log10_pow2_minus_log10_4_over_3(e: i32) -> i32 {
    compute(
        631305,
        261663,
        21,
        FLOOR_LOG10_POW2_MINUS_LOG10_4_OVER_3_MIN_EXPONENT,
        FLOOR_LOG10_POW2_MINUS_LOG10_4_OVER_3_MAX_EXPONENT,
        e,
    )
}

pub const FLOOR_LOG5_POW2_MIN_EXPONENT: i32 = -1831;
pub const FLOOR_LOG5_POW2_MAX_EXPONENT: i32 = 1831;
pub const fn floor_log5_pow2(e: i32) -> i32 {
    compute(225799, 0, 19, FLOOR_LOG5_POW2_MIN_EXPONENT, FLOOR_LOG5_POW2_MAX_EXPONENT, e)
}

pub const FLOOR_LOG5_POW2_MINUS_LOG5_3_MIN_EXPONENT: i32 = -3543;
pub const FLOOR_LOG5_POW2_MINUS_LOG5_3_MAX_EXPONENT: i32 = 2427;
pub const fn floor_log5_pow2_minus_log5_3(e: i32) -> i32 {
    compute(
        451597,
        715764,
        20,
        FLOOR_LOG5_POW2_MINUS_LOG5_3_MIN_EXPONENT,
        FLOOR_LOG5_POW2_MINUS_LOG5_3_MAX_EXPONENT,
        e,
    )
}
