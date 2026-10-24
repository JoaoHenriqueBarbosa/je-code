//! Porte de `WTF/wtf/fast_float/fast_float.h`, seção `FASTFLOAT_DIGIT_COMPARISON_H` (linhas 4094 a
//! 4544 do amálgama).
//!
//! Mapeamentos:
//!
//! - Os ponteiros `UC const *p`/`pend` viram índices num slice `chars: &[UC]`, como em
//!   `ascii_number`; os `span<UC const>` `integer` e `fraction` de `parsed_number_string_t` são
//!   `Range<usize>` sobre o mesmo slice. O `span` nulo de `fraction` (ponteiro nulo) é a faixa
//!   `0..0`: uma fração real começa sempre depois do ponto decimal, então nunca em zero, e o teste
//!   `num.fraction.ptr != nullptr` vira `num.fraction.start != 0`.
//! - Os `memcpy` de oito bytes de `skip_zeros`/`is_truncated` viram a comparação dos
//!   `int_cmp_len<UC>()` caracteres do bloco com `'0'`, que é a mesma conta da palavra de 64 bits
//!   contra `int_cmp_zeros<UC>()`.
//! - `cpp20_and_in_constexpr()` é falso em tempo de execução, então os laços de blocos valem.
//! - `FASTFLOAT_64BIT_LIMB` vale: `step` é 19.
//! - Os `callback` de `round` e `round_nearest_tie_even` (lambdas) viram parâmetros genéricos
//!   `FnOnce`. O `T` de `round<T>` vira genérico sobre `BinaryFormat`.
//! - `to_extended` lê os bits de `T` (`std::bit_cast`): o trait `BitCastWord`, definido aqui,
//!   acrescenta essa operação a `BinaryFormat` (que não a tem em `float_common`).
//! - `FASTFLOAT_ASSERT(x)` avalia o argumento e o descarta, como em `bigint`.
//! - A aritmética de `int32_t`/`size_t` que pode estourar vira `wrapping_*`.
#![allow(non_camel_case_types, non_upper_case_globals)]

use super::ascii_number::{parse_eight_digits_unrolled, parsed_number_string_t};
use super::bigint::{bigint, limb};
use super::float_common::{
    adjusted_mantissa, int_cmp_len, to_float, BinaryFormat, INVALID_AM_BIAS,
};
use crate::js_number::CharType;

/// `std::bit_cast<equiv_uint_t<T>>(value)`, devolvido como `u64`: os bits de `f32` (nos 32 bits
/// baixos) ou de `f64`.
pub trait BitCastWord: BinaryFormat {
    fn to_word(self) -> u64;
}

impl BitCastWord for f64 {
    fn to_word(self) -> u64 {
        self.to_bits()
    }
}

impl BitCastWord for f32 {
    fn to_word(self) -> u64 {
        self.to_bits() as u64
    }
}

// 1e0 to 1e19
pub static powers_of_ten_uint64: [u64; 20] = [
    1,
    10,
    100,
    1000,
    10000,
    100000,
    1000000,
    10000000,
    100000000,
    1000000000,
    10000000000,
    100000000000,
    1000000000000,
    10000000000000,
    100000000000000,
    1000000000000000,
    10000000000000000,
    100000000000000000,
    1000000000000000000,
    10000000000000000000,
];

// calculate the exponent, in scientific notation, of the number.
// this algorithm is not even close to optimized, but it has no practical
// effect on performance: in order to have a faster algorithm, we'd need
// to slow down performance for faster algorithms, and this is still fast.
pub fn scientific_exponent(mut mantissa: u64, mut exponent: i32) -> i32 {
    while mantissa >= 10000 {
        mantissa /= 10000;
        exponent = exponent.wrapping_add(4);
    }
    while mantissa >= 100 {
        mantissa /= 100;
        exponent = exponent.wrapping_add(2);
    }
    while mantissa >= 10 {
        mantissa /= 10;
        exponent = exponent.wrapping_add(1);
    }
    exponent
}

// this converts a native floating-point number to an extended-precision float.
pub fn to_extended<T: BitCastWord>(value: T) -> adjusted_mantissa {
    let exponent_mask: u64 = T::exponent_mask().into();
    let mantissa_mask: u64 = T::mantissa_mask().into();
    let hidden_bit_mask: u64 = T::hidden_bit_mask().into();

    let mut am = adjusted_mantissa::default();
    let bias: i32 = T::mantissa_explicit_bits() - T::minimum_exponent();
    let bits: u64 = value.to_word();
    if (bits & exponent_mask) == 0 {
        // denormal
        am.power2 = 1 - bias;
        am.mantissa = bits & mantissa_mask;
    } else {
        // normal
        am.power2 = ((bits & exponent_mask) >> (T::mantissa_explicit_bits() as u32)) as i32;
        am.power2 -= bias;
        am.mantissa = (bits & mantissa_mask) | hidden_bit_mask;
    }

    am
}

// get the extended precision value of the halfway point between b and b+u.
// we are given a native float that represents b, so we need to adjust it
// halfway between b and b+u.
pub fn to_extended_halfway<T: BitCastWord>(value: T) -> adjusted_mantissa {
    let mut am = to_extended(value);
    am.mantissa <<= 1;
    am.mantissa = am.mantissa.wrapping_add(1);
    am.power2 -= 1;
    am
}

// round an extended-precision float to the nearest machine float.
pub fn round<T: BinaryFormat, F: FnOnce(&mut adjusted_mantissa, i32)>(
    am: &mut adjusted_mantissa,
    cb: F,
) {
    let mantissa_shift: i32 = 64 - T::mantissa_explicit_bits() - 1;
    if -am.power2 >= mantissa_shift {
        // have a denormal float
        let shift: i32 = -am.power2 + 1;
        cb(am, if shift < 64 { shift } else { 64 });
        // check for round-up: if rounding-nearest carried us to the hidden bit.
        am.power2 = if am.mantissa < (1u64 << (T::mantissa_explicit_bits() as u32)) {
            0
        } else {
            1
        };
        return;
    }

    // have a normal float, use the default shift.
    cb(am, mantissa_shift);

    // check for carry
    if am.mantissa >= (2u64 << (T::mantissa_explicit_bits() as u32)) {
        am.mantissa = 1u64 << (T::mantissa_explicit_bits() as u32);
        am.power2 += 1;
    }

    // check for infinite: we could have carried to an infinite power
    am.mantissa &= !(1u64 << (T::mantissa_explicit_bits() as u32));
    if am.power2 >= T::infinite_power() {
        am.power2 = T::infinite_power();
        am.mantissa = 0;
    }
}

pub fn round_nearest_tie_even<F: FnOnce(bool, bool, bool) -> bool>(
    am: &mut adjusted_mantissa,
    shift: i32,
    cb: F,
) {
    let mask: u64 = if shift == 64 { u64::MAX } else { (1u64 << (shift as u32)) - 1 };
    let halfway: u64 = if shift == 0 { 0 } else { 1u64 << ((shift - 1) as u32) };
    let truncated_bits: u64 = am.mantissa & mask;
    let is_above: bool = truncated_bits > halfway;
    let is_halfway: bool = truncated_bits == halfway;

    // shift digits into position
    if shift == 64 {
        am.mantissa = 0;
    } else {
        am.mantissa >>= shift as u32;
    }
    am.power2 += shift;

    let is_odd: bool = (am.mantissa & 1) == 1;
    am.mantissa = am.mantissa.wrapping_add(cb(is_odd, is_halfway, is_above) as u64);
}

pub fn round_down(am: &mut adjusted_mantissa, shift: i32) {
    if shift == 64 {
        am.mantissa = 0;
    } else {
        am.mantissa >>= shift as u32;
    }
    am.power2 += shift;
}

/// `skip_zeros(first, last)`: `first` é o índice corrente em `chars` e `last` o índice final.
pub fn skip_zeros<UC: CharType>(chars: &[UC], first: &mut usize, last: usize) {
    let block: usize = int_cmp_len::<UC>() as usize;
    // cpp20_and_in_constexpr() é falso: vale o laço de blocos de 64 bits.
    while (last - *first) as i64 >= int_cmp_len::<UC>() as i64 {
        if !is_block_of_zeros(&chars[*first..*first + block]) {
            break;
        }
        *first += block;
    }
    while *first != last {
        if chars[*first] != UC::from_u16(b'0' as u16) {
            break;
        }
        *first += 1;
    }
}

/// A palavra de 64 bits lida por `memcpy` é igual a `int_cmp_zeros<UC>()` quando todos os
/// caracteres do bloco são `'0'`.
fn is_block_of_zeros<UC: CharType>(block: &[UC]) -> bool {
    block.iter().all(|&c| c == UC::from_u16(b'0' as u16))
}

// determine if any non-zero digits were truncated.
// all characters must be valid digits.
pub fn is_truncated<UC: CharType>(chars: &[UC], mut first: usize, last: usize) -> bool {
    let block: usize = int_cmp_len::<UC>() as usize;
    // do 8-bit optimizations, can just compare to 8 literal 0s.
    while (last - first) as i64 >= int_cmp_len::<UC>() as i64 {
        if !is_block_of_zeros(&chars[first..first + block]) {
            return true;
        }
        first += block;
    }
    while first != last {
        if chars[first] != UC::from_u16(b'0' as u16) {
            return true;
        }
        first += 1;
    }
    false
}

/// `is_truncated(span<UC const> s)`: o `span` é a faixa de índices sobre `chars`.
pub fn is_truncated_span<UC: CharType>(chars: &[UC], s: &std::ops::Range<usize>) -> bool {
    is_truncated(chars, s.start, s.start + s.len())
}

pub fn parse_eight_digits<UC: CharType>(
    chars: &[UC],
    p: &mut usize,
    value: &mut limb,
    counter: &mut usize,
    count: &mut usize,
) {
    *value = value
        .wrapping_mul(100000000)
        .wrapping_add(parse_eight_digits_unrolled(&chars[*p..]) as limb);
    *p += 8;
    *counter += 8;
    *count += 8;
}

pub fn parse_one_digit<UC: CharType>(
    chars: &[UC],
    p: &mut usize,
    value: &mut limb,
    counter: &mut usize,
    count: &mut usize,
) {
    let code: u32 = chars[*p].into();
    let digit: limb = (code as i64 - '0' as i64) as limb;
    *value = value.wrapping_mul(10).wrapping_add(digit);
    *p += 1;
    *counter += 1;
    *count += 1;
}

pub fn add_native(big: &mut bigint, power: limb, value: limb) {
    big.mul(power);
    big.add(value);
}

pub fn round_up_bigint(big: &mut bigint, count: &mut usize) {
    // need to round-up the digits, but need to avoid rounding
    // ....9999 to ...10000, which could cause a false halfway point.
    add_native(big, 10, 1);
    *count += 1;
}

// parse the significant digits into a big integer
pub fn parse_mantissa<UC: CharType>(
    result: &mut bigint,
    chars: &[UC],
    num: &parsed_number_string_t,
    max_digits: usize,
    digits: &mut usize,
) {
    // try to minimize the number of big integer and scalar multiplication.
    // therefore, try to parse 8 digits at a time, and multiply by the largest
    // scalar value (9 or 19 digits) for each step.
    let mut counter: usize = 0;
    *digits = 0;
    let mut value: limb = 0;
    let step: usize = 19;

    // process all integer digits.
    let mut p: usize = num.integer.start;
    let mut pend: usize = p + num.integer.len();
    skip_zeros(chars, &mut p, pend);
    // process all digits, in increments of step per loop
    while p != pend {
        while (pend - p) >= 8
            && step.wrapping_sub(counter) >= 8
            && max_digits.wrapping_sub(*digits) >= 8
        {
            parse_eight_digits(chars, &mut p, &mut value, &mut counter, digits);
        }
        while counter < step && p != pend && *digits < max_digits {
            parse_one_digit(chars, &mut p, &mut value, &mut counter, digits);
        }
        if *digits == max_digits {
            // add the temporary value, then check if we've truncated any digits
            add_native(result, powers_of_ten_uint64[counter] as limb, value);
            let mut truncated: bool = is_truncated(chars, p, pend);
            if num.fraction.start != 0 {
                truncated |= is_truncated_span(chars, &num.fraction);
            }
            if truncated {
                round_up_bigint(result, digits);
            }
            return;
        } else {
            add_native(result, powers_of_ten_uint64[counter] as limb, value);
            counter = 0;
            value = 0;
        }
    }

    // add our fraction digits, if they're available.
    if num.fraction.start != 0 {
        p = num.fraction.start;
        pend = p + num.fraction.len();
        if *digits == 0 {
            skip_zeros(chars, &mut p, pend);
        }
        // process all digits, in increments of step per loop
        while p != pend {
            while (pend - p) >= 8
                && step.wrapping_sub(counter) >= 8
                && max_digits.wrapping_sub(*digits) >= 8
            {
                parse_eight_digits(chars, &mut p, &mut value, &mut counter, digits);
            }
            while counter < step && p != pend && *digits < max_digits {
                parse_one_digit(chars, &mut p, &mut value, &mut counter, digits);
            }
            if *digits == max_digits {
                // add the temporary value, then check if we've truncated any digits
                add_native(result, powers_of_ten_uint64[counter] as limb, value);
                let truncated: bool = is_truncated(chars, p, pend);
                if truncated {
                    round_up_bigint(result, digits);
                }
                return;
            } else {
                add_native(result, powers_of_ten_uint64[counter] as limb, value);
                counter = 0;
                value = 0;
            }
        }
    }

    if counter != 0 {
        add_native(result, powers_of_ten_uint64[counter] as limb, value);
    }
}

pub fn positive_digit_comp<T: BinaryFormat>(
    bigmant: &mut bigint,
    exponent: i32,
) -> adjusted_mantissa {
    bigmant.pow10(exponent as u32); // FASTFLOAT_ASSERT
    let mut answer = adjusted_mantissa::default();
    let mut truncated: bool = false;
    answer.mantissa = bigmant.hi64(&mut truncated);
    let bias: i32 = T::mantissa_explicit_bits() - T::minimum_exponent();
    answer.power2 = bigmant.bit_length() - 64 + bias;

    round::<T, _>(&mut answer, |a: &mut adjusted_mantissa, shift: i32| {
        round_nearest_tie_even(
            a,
            shift,
            |is_odd: bool, is_halfway: bool, is_above: bool| -> bool {
                is_above || (is_halfway && truncated) || (is_odd && is_halfway)
            },
        );
    });

    answer
}

// the scaling here is quite simple: we have, for the real digits `m * 10^e`,
// and for the theoretical digits `n * 2^f`. Since `e` is always negative,
// to scale them identically, we do `n * 2^f * 5^-f`, so we now have `m * 2^e`.
// we then need to scale by `2^(f- e)`, and then the two significant digits
// are of the same magnitude.
pub fn negative_digit_comp<T: BitCastWord>(
    bigmant: &mut bigint,
    am: adjusted_mantissa,
    exponent: i32,
) -> adjusted_mantissa {
    let real_digits: &mut bigint = bigmant;
    let real_exp: i32 = exponent;

    // get the value of `b`, rounded down, and get a bigint representation of b+h
    let mut am_b: adjusted_mantissa = am;
    round::<T, _>(&mut am_b, |a: &mut adjusted_mantissa, shift: i32| round_down(a, shift));
    let mut b: T = T::from_u64(0);
    to_float(false, am_b, &mut b);
    let theor: adjusted_mantissa = to_extended_halfway(b);
    let mut theor_digits = bigint::from_u64(theor.mantissa);
    let theor_exp: i32 = theor.power2;

    // scale real digits and theor digits to be same power.
    let pow2_exp: i32 = theor_exp.wrapping_sub(real_exp);
    let pow5_exp: u32 = real_exp.wrapping_neg() as u32;
    if pow5_exp != 0 {
        theor_digits.pow5(pow5_exp); // FASTFLOAT_ASSERT
    }
    if pow2_exp > 0 {
        theor_digits.pow2(pow2_exp as u32); // FASTFLOAT_ASSERT
    } else if pow2_exp < 0 {
        real_digits.pow2(pow2_exp.wrapping_neg() as u32); // FASTFLOAT_ASSERT
    }

    // compare digits, and use it to direct rounding
    let ord: i32 = real_digits.compare(&theor_digits);
    let mut answer: adjusted_mantissa = am;
    round::<T, _>(&mut answer, |a: &mut adjusted_mantissa, shift: i32| {
        round_nearest_tie_even(a, shift, |is_odd: bool, _is_halfway: bool, _is_above: bool| -> bool {
            // not needed, since we've done our comparison
            if ord > 0 {
                true
            } else if ord < 0 {
                false
            } else {
                is_odd
            }
        });
    });

    answer
}

// parse the significant digits as a big integer to unambiguously round
// the significant digits. here, we are trying to determine how to round
// an extended float representation close to `b+h`, halfway between `b`
// (the float rounded-down) and `b+u`, the next positive float. this
// algorithm is always correct, and uses one of two approaches. when
// the exponent is positive relative to the significant digits (such as
// 1234), we create a big-integer representation, get the high 64-bits,
// determine if any lower bits are truncated, and use that to direct
// rounding. in case of a negative exponent relative to the significant
// digits (such as 1.2345), we create a theoretical representation of
// `b` as a big-integer type, scaled to the same binary exponent as
// the actual digits. we then compare the big integer representations
// of both, and use that to direct rounding.
pub fn digit_comp<T: BitCastWord, UC: CharType>(
    chars: &[UC],
    num: &parsed_number_string_t,
    mut am: adjusted_mantissa,
) -> adjusted_mantissa {
    // remove the invalid exponent bias
    am.power2 = am.power2.wrapping_sub(INVALID_AM_BIAS);

    let sci_exp: i32 = scientific_exponent(num.mantissa, num.exponent as i32);
    let max_digits: usize = T::max_digits();
    let mut digits: usize = 0;
    let mut bigmant = bigint::new();
    parse_mantissa(&mut bigmant, chars, num, max_digits, &mut digits);
    // can't underflow, since digits is at most max_digits.
    let exponent: i32 = sci_exp.wrapping_add(1).wrapping_sub(digits as i32);
    if exponent >= 0 {
        positive_digit_comp::<T>(&mut bigmant, exponent)
    } else {
        negative_digit_comp::<T>(&mut bigmant, am, exponent)
    }
}
