// Tradução de WTF/wtf/dtoa/fast-dtoa.h e fast-dtoa.cc (double-conversion, Grisu3).
//
// Copyright 2012 the V8 project authors. All rights reserved.
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are
// met:
//
//     * Redistributions of source code must retain the above copyright
//       notice, this list of conditions and the following disclaimer.
//     * Redistributions in binary form must reproduce the above
//       copyright notice, this list of conditions and the following
//       disclaimer in the documentation and/or other materials provided
//       with the distribution.
//     * Neither the name of Google Inc. nor the names of its
//       contributors may be used to endorse or promote products derived
//       from this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
// OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
// LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
// DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use crate::wtf::dtoa::cached_powers::PowersOfTenCache;
use crate::wtf::dtoa::diy_fp::DiyFp;
use crate::wtf::dtoa::ieee::{Double, Single};

#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FastDtoaMode {
    // Computes the shortest representation of the given input. The returned
    // result will be the most accurate number of this length. Longer
    // representations might be more accurate.
    FAST_DTOA_SHORTEST,
    // Same as FAST_DTOA_SHORTEST but for single-precision floats.
    FAST_DTOA_SHORTEST_SINGLE,
    // Computes a representation where the precision (number of digits) is
    // given as input. The precision is independent of the decimal point.
    FAST_DTOA_PRECISION,
}

// FastDtoa will produce at most K_FAST_DTOA_MAXIMAL_LENGTH digits. Isto não
// inclui o terminador '\0'.
pub const K_FAST_DTOA_MAXIMAL_LENGTH: i32 = 17;
// Same for single-precision numbers.
pub const K_FAST_DTOA_MAXIMAL_SINGLE_LENGTH: i32 = 9;

// The minimal and maximal target exponent define the range of w's binary
// exponent, where 'w' is the result of multiplying the input by a cached power
// of ten.
//
// A different range might be chosen on a different platform, to optimize digit
// generation, but a smaller range requires more powers of ten to be cached.
const K_MINIMAL_TARGET_EXPONENT: i32 = -60;
const K_MAXIMAL_TARGET_EXPONENT: i32 = -32;

// Adjusts the last digit of the generated number, and screens out generated
// solutions that may be inaccurate. A solution may be inaccurate if it is
// outside the safe interval, or if we cannot prove that it is closer to the
// input than a neighboring representation of the same length.
//
// Input: * buffer containing the digits of too_high / 10^kappa
//        * the buffer's length
//        * distance_too_high_w == (too_high - w).f() * unit
//        * unsafe_interval == (too_high - too_low).f() * unit
//        * rest = (too_high - buffer * 10^kappa).f() * unit
//        * ten_kappa = 10^kappa * unit
//        * unit = the common multiplier
// Output: returns true if the buffer is guaranteed to contain the closest
//    representable number to the input.
//  Modifies the generated digits in the buffer to approach (round towards) w.
fn round_weed(
    buffer: &mut [u8],
    length: i32,
    distance_too_high_w: u64,
    unsafe_interval: u64,
    mut rest: u64,
    ten_kappa: u64,
    unit: u64,
) -> bool {
    let small_distance: u64 = distance_too_high_w.wrapping_sub(unit);
    let big_distance: u64 = distance_too_high_w.wrapping_add(unit);
    // Let w_low  = too_high - big_distance, and
    //     w_high = too_high - small_distance.
    // Note: w_low < w < w_high
    //
    // The real w (* unit) must lie somewhere inside the interval
    // ]w_low; w_high[ (often written as "(w_low; w_high)")
    //
    // Basicamente o buffer contém um número no intervalo inseguro
    // ]too_low; too_high[ com too_low < w < too_high. O buffer pode estar em
    // qualquer ponto entre too_low e too_high. boundary_low, boundary_high e w
    // são aproximações dos limites reais e de v, precisas a menos de uma unidade.
    //
    // Anything that lies outside the unsafe interval is guaranteed not to round
    // to v when read again.
    // Anything that lies inside the safe interval is guaranteed to round to v
    // when read again.
    // If the number inside the buffer lies inside the unsafe interval but not
    // inside the safe interval then we simply do not know and bail out (returning
    // false).
    //
    // By generating the digits of too_high we got the largest (closest to
    // too_high) buffer that is still in the unsafe interval. In the case where
    // w_high < buffer < too_high we try to decrement the buffer.
    // This way the buffer approaches (rounds towards) w.
    // There are 3 conditions that stop the decrementation process:
    //   1) the buffer is already below w_high
    //   2) decrementing the buffer would make it leave the unsafe interval
    //   3) decrementing the buffer would yield a number below w_high and farther
    //      away than the current number. In other words:
    //              (buffer{-1} < w_high) && w_high - buffer{-1} > buffer - w_high
    // Instead of using the buffer directly we use its distance to too_high.
    // Conceptually rest ~= too_high - buffer
    // We need to do the following tests in this order to avoid over- and
    // underflows.
    let last = (length - 1) as usize;
    while rest < small_distance // Negated condition 1
        && unsafe_interval.wrapping_sub(rest) >= ten_kappa // Negated condition 2
        && (rest.wrapping_add(ten_kappa) < small_distance // buffer{-1} > w_high
            || small_distance.wrapping_sub(rest)
                >= rest.wrapping_add(ten_kappa).wrapping_sub(small_distance))
    {
        buffer[last] = buffer[last].wrapping_sub(1);
        rest = rest.wrapping_add(ten_kappa);
    }

    // We have approached w+ as much as possible. We now test if approaching w-
    // would require changing the buffer. If yes, then we have two possible
    // representations close to w, but we cannot decide which one is closer.
    if rest < big_distance
        && unsafe_interval.wrapping_sub(rest) >= ten_kappa
        && (rest.wrapping_add(ten_kappa) < big_distance
            || big_distance.wrapping_sub(rest)
                > rest.wrapping_add(ten_kappa).wrapping_sub(big_distance))
    {
        return false;
    }

    // Weeding test.
    //   The safe interval is [too_low + 2 ulp; too_high - 2 ulp]
    //   Since too_low = too_high - unsafe_interval this is equivalent to
    //      [too_high - unsafe_interval + 4 ulp; too_high - 2 ulp]
    //   Conceptually we have: rest ~= too_high - buffer
    (2u64.wrapping_mul(unit) <= rest)
        && (rest <= unsafe_interval.wrapping_sub(4u64.wrapping_mul(unit)))
}

// Rounds the buffer upwards if the result is closer to v by possibly adding
// 1 to the buffer. If the precision of the calculation is not sufficient to
// round correctly, return false.
// The rounding might shift the whole buffer in which case the kappa is
// adjusted. For example "99", kappa = 3 might become "10", kappa = 4.
//
// If 2*rest > ten_kappa then the buffer needs to be round up.
// rest can have an error of +/- 1 unit. This function accounts for the
// imprecision and returns false, if the rounding direction cannot be
// unambiguously determined.
//
// Precondition: rest < ten_kappa.
fn round_weed_counted(
    buffer: &mut [u8],
    length: i32,
    rest: u64,
    ten_kappa: u64,
    unit: u64,
    kappa: &mut i32,
) -> bool {
    // The following tests are done in a specific order to avoid overflows. They
    // will work correctly with any uint64 values of rest < ten_kappa and unit.
    //
    // If the unit is too big, then we don't know which way to round. For example
    // a unit of 50 means that the real number lies within rest +/- 50. If
    // 10^kappa == 40 then there is no way to tell which way to round.
    if unit >= ten_kappa {
        return false;
    }
    // Even if unit is just half the size of 10^kappa we are already completely
    // lost. (And after the previous test we know that the expression will not
    // over/underflow.)
    if ten_kappa.wrapping_sub(unit) <= unit {
        return false;
    }
    // If 2 * (rest + unit) <= 10^kappa we can safely round down.
    if (ten_kappa.wrapping_sub(rest) > rest)
        && (ten_kappa.wrapping_sub(2u64.wrapping_mul(rest)) >= 2u64.wrapping_mul(unit))
    {
        return true;
    }
    // If 2 * (rest - unit) >= 10^kappa, then we can safely round up.
    if (rest > unit) && (ten_kappa.wrapping_sub(rest.wrapping_sub(unit)) <= rest.wrapping_sub(unit))
    {
        // Increment the last digit recursively until we find a non '9' digit.
        let last = (length - 1) as usize;
        buffer[last] = buffer[last].wrapping_add(1);
        let mut i = length - 1;
        while i > 0 {
            let iu = i as usize;
            if buffer[iu] != b'0' + 10 {
                break;
            }
            buffer[iu] = b'0';
            buffer[iu - 1] = buffer[iu - 1].wrapping_add(1);
            i -= 1;
        }
        // If the first digit is now '0'+ 10 we had a buffer with all '9's. With the
        // exception of the first digit all digits are now '0'. Simply switch the
        // first digit to '1' and adjust the kappa. Example: "99" becomes "10" and
        // the power (the kappa) is increased.
        if buffer[0] == b'0' + 10 {
            buffer[0] = b'1';
            *kappa += 1;
        }
        return true;
    }
    false
}

// Returns the biggest power of ten that is less than or equal to the given
// number. We furthermore receive the maximum number of bits 'number' has.
//
// Returns power == 10^(exponent_plus_one-1) such that
//    power <= number < power * 10.
// If number_bits == 0 then 0^(0-1) is returned.
// The number of bits must be <= 32.
// Precondition: number < (1 << (number_bits + 1)).

// Inspired by the method for finding an integer log base 10 from here:
// http://graphics.stanford.edu/~seander/bithacks.html#IntegerLog10
const K_SMALL_POWERS_OF_TEN: [u32; 11] = [
    0, 1, 10, 100, 1000, 10000, 100000, 1000000, 10000000, 100000000, 1000000000,
];

fn biggest_power_ten(number: u32, number_bits: i32, power: &mut u32, exponent_plus_one: &mut i32) {
    // 1233/4096 is approximately 1/lg(10).
    let mut exponent_plus_one_guess: i32 = (number_bits + 1) * 1233 >> 12;
    // We increment to skip over the first entry in the kPowersOf10 table.
    // Note: kPowersOf10[i] == 10^(i-1).
    exponent_plus_one_guess += 1;
    // We don't have any guarantees that 2^number_bits <= number.
    if number < K_SMALL_POWERS_OF_TEN[exponent_plus_one_guess as usize] {
        exponent_plus_one_guess -= 1;
    }
    *power = K_SMALL_POWERS_OF_TEN[exponent_plus_one_guess as usize];
    *exponent_plus_one = exponent_plus_one_guess;
}

// Generates the digits of input number w.
// w is a floating-point number (DiyFp), consisting of a significand and an
// exponent. Its exponent is bounded by kMinimalTargetExponent and
// kMaximalTargetExponent.
//       Hence -60 <= w.e() <= -32.
//
// Returns false if it fails, in which case the generated digits in the buffer
// should not be used.
// Preconditions:
//  * low, w and high are correct up to 1 ulp (unit in the last place). That
//    is, their error must be less than a unit of their last digits.
//  * low.e() == w.e() == high.e()
//  * low < w < high, and taking into account their error: low~ <= high~
//  * kMinimalTargetExponent <= w.e() <= kMaximalTargetExponent
// Postconditions: returns false if procedure fails.
//   otherwise:
//     * buffer is not null-terminated, but len contains the number of digits.
//     * buffer contains the shortest possible decimal digit-sequence
//       such that LOW < buffer * 10^kappa < HIGH, where LOW and HIGH are the
//       correct values of low and high (without their error).
//     * if more than one decimal representation gives the minimal number of
//       decimal digits then the one closest to W (where W is the correct value
//       of w) is chosen.
// Remark: this procedure takes into account the imprecision of its input
//   numbers. If the precision is not enough to guarantee all the postconditions
//   then false is returned. This usually happens rarely (~0.5%).
//
// Say, for the sake of example, that
//   w.e() == -48, and w.f() == 0x1234567890abcdef
// w's value can be computed by w.f() * 2^w.e()
// We can obtain w's integral digits by simply shifting w.f() by -w.e().
//  -> w's integral part is 0x1234
//  w's fractional part is therefore 0x567890abcdef.
// Printing w's integral part is easy (simply print 0x1234 in decimal).
// In order to print its fraction we repeatedly multiply the fraction by 10 and
// get each digit. Example the first digit after the point would be computed by
//   (0x567890abcdef * 10) >> 48. -> 3
// The whole thing becomes slightly more complicated because we want to stop
// once we have enough digits. That is, once the digits inside the buffer
// represent 'w' we can stop. Everything inside the interval low - high
// represents w. However we have to pay attention to low, high and w's
// imprecision.
fn digit_gen(
    low: DiyFp,
    w: DiyFp,
    high: DiyFp,
    buffer: &mut [u8],
    length: &mut i32,
    kappa: &mut i32,
) -> bool {
    // low, w and high are imprecise, but by less than one ulp (unit in the last
    // place).
    // If we remove (resp. add) 1 ulp from low (resp. high) we are certain that
    // the new numbers are outside of the interval we want the final
    // representation to lie in.
    // Inversely adding (resp. removing) 1 ulp from low (resp. high) would yield
    // numbers that are certain to lie in the interval. We will use this fact
    // later on.
    // We will now start by generating the digits within the uncertain
    // interval. Later we will weed out representations that lie outside the safe
    // interval and thus _might_ lie outside the correct interval.
    let mut unit: u64 = 1;
    let too_low = DiyFp::new(low.f().wrapping_sub(unit), low.e());
    let too_high = DiyFp::new(high.f().wrapping_add(unit), high.e());
    // too_low and too_high are guaranteed to lie outside the interval we want the
    // generated number in.
    let unsafe_interval0 = DiyFp::minus(too_high, too_low);
    let mut unsafe_interval_f: u64 = unsafe_interval0.f();
    // We now cut the input number into two parts: the integral digits and the
    // fractionals. We will not write any decimal separator though, but adapt
    // kappa instead.
    // Reminder: we are currently computing the digits (stored inside the buffer)
    // such that:   too_low < buffer * 10^kappa < too_high
    // We use too_high for the digit_generation and stop as soon as possible.
    // If we stop early we effectively round down.
    let one = DiyFp::new(1u64 << ((-w.e()) as u32), w.e());
    let one_shift = (-one.e()) as u32;
    // Division by one is a shift.
    let mut integrals: u32 = (too_high.f() >> one_shift) as u32;
    // Modulo by one is an and.
    let mut fractionals: u64 = too_high.f() & one.f().wrapping_sub(1);
    let mut divisor: u32 = 0;
    let mut divisor_exponent_plus_one: i32 = 0;
    biggest_power_ten(
        integrals,
        (DiyFp::K_SIGNIFICAND_SIZE as i32) - (-one.e()),
        &mut divisor,
        &mut divisor_exponent_plus_one,
    );
    *kappa = divisor_exponent_plus_one;
    *length = 0;
    // Loop invariant: buffer = too_high / 10^kappa  (integer division)
    // The invariant holds for the first iteration: kappa has been initialized
    // with the divisor exponent + 1. And the divisor is the biggest power of ten
    // that is smaller than integrals.
    while *kappa > 0 {
        let digit: u32 = integrals / divisor;
        buffer[*length as usize] = b'0' + digit as u8;
        *length += 1;
        integrals %= divisor;
        *kappa -= 1;
        // Note that kappa now equals the exponent of the divisor and that the
        // invariant thus holds again.
        let rest: u64 = ((integrals as u64) << one_shift).wrapping_add(fractionals);
        // Invariant: too_high = buffer * 10^kappa + DiyFp(rest, one.e())
        // Reminder: unsafe_interval.e() == one.e()
        if rest < unsafe_interval_f {
            // Rounding down (by not emitting the remaining digits) yields a number
            // that lies within the unsafe interval.
            return round_weed(
                buffer,
                *length,
                DiyFp::minus(too_high, w).f(),
                unsafe_interval_f,
                rest,
                (divisor as u64) << one_shift,
                unit,
            );
        }
        divisor /= 10;
    }

    // The integrals have been generated. We are at the point of the decimal
    // separator. In the following loop we simply multiply the remaining digits by
    // 10 and divide by one. We just need to pay attention to multiply associated
    // data (like the interval or 'unit'), too.
    // Note that the multiplication by 10 does not overflow, because w.e >= -60
    // and thus one.e >= -60.
    loop {
        fractionals = fractionals.wrapping_mul(10);
        unit = unit.wrapping_mul(10);
        unsafe_interval_f = unsafe_interval_f.wrapping_mul(10);
        // Integer division by one.
        let digit: i32 = (fractionals >> one_shift) as i32;
        buffer[*length as usize] = b'0' + digit as u8;
        *length += 1;
        fractionals &= one.f().wrapping_sub(1); // Modulo by one.
        *kappa -= 1;
        if fractionals < unsafe_interval_f {
            return round_weed(
                buffer,
                *length,
                DiyFp::minus(too_high, w).f().wrapping_mul(unit),
                unsafe_interval_f,
                fractionals,
                one.f(),
                unit,
            );
        }
    }
}

// Generates (at most) requested_digits digits of input number w.
// w is a floating-point number (DiyFp), consisting of a significand and an
// exponent. Its exponent is bounded by kMinimalTargetExponent and
// kMaximalTargetExponent.
//       Hence -60 <= w.e() <= -32.
//
// Returns false if it fails, in which case the generated digits in the buffer
// should not be used.
// Preconditions:
//  * w is correct up to 1 ulp (unit in the last place). That
//    is, its error must be strictly less than a unit of its last digit.
//  * kMinimalTargetExponent <= w.e() <= kMaximalTargetExponent
//
// Postconditions: returns false if procedure fails.
//   otherwise:
//     * buffer is not null-terminated, but length contains the number of
//       digits.
//     * the representation in buffer is the most precise representation of
//       requested_digits digits.
//     * buffer contains at most requested_digits digits of w. If there are less
//       than requested_digits digits then some trailing '0's have been removed.
//     * kappa is such that
//            w = buffer * 10^kappa + eps with |eps| < 10^kappa / 2.
//
// Remark: This procedure takes into account the imprecision of its input
//   numbers. If the precision is not enough to guarantee all the postconditions
//   then false is returned. This usually happens rarely, but the failure-rate
//   increases with higher requested_digits.
fn digit_gen_counted(
    w: DiyFp,
    mut requested_digits: i32,
    buffer: &mut [u8],
    length: &mut i32,
    kappa: &mut i32,
) -> bool {
    const _: () = assert!(K_MINIMAL_TARGET_EXPONENT >= -60);
    const _: () = assert!(K_MAXIMAL_TARGET_EXPONENT <= -32);
    // w is assumed to have an error less than 1 unit. Whenever w is scaled we
    // also scale its error.
    let mut w_error: u64 = 1;
    // We cut the input number into two parts: the integral digits and the
    // fractional digits. We don't emit any decimal separator, but adapt kappa
    // instead. Example: instead of writing "1.2" we put "12" into the buffer and
    // increase kappa by 1.
    let one = DiyFp::new(1u64 << ((-w.e()) as u32), w.e());
    let one_shift = (-one.e()) as u32;
    // Division by one is a shift.
    let mut integrals: u32 = (w.f() >> one_shift) as u32;
    // Modulo by one is an and.
    let mut fractionals: u64 = w.f() & one.f().wrapping_sub(1);
    let mut divisor: u32 = 0;
    let mut divisor_exponent_plus_one: i32 = 0;
    biggest_power_ten(
        integrals,
        (DiyFp::K_SIGNIFICAND_SIZE as i32) - (-one.e()),
        &mut divisor,
        &mut divisor_exponent_plus_one,
    );
    *kappa = divisor_exponent_plus_one;
    *length = 0;

    // Loop invariant: buffer = w / 10^kappa  (integer division)
    // The invariant holds for the first iteration: kappa has been initialized
    // with the divisor exponent + 1. And the divisor is the biggest power of ten
    // that is smaller than 'integrals'.
    while *kappa > 0 {
        let digit: u32 = integrals / divisor;
        buffer[*length as usize] = b'0' + digit as u8;
        *length += 1;
        requested_digits -= 1;
        integrals %= divisor;
        *kappa -= 1;
        // Note that kappa now equals the exponent of the divisor and that the
        // invariant thus holds again.
        if requested_digits == 0 {
            break;
        }
        divisor /= 10;
    }

    if requested_digits == 0 {
        let rest: u64 = ((integrals as u64) << one_shift).wrapping_add(fractionals);
        return round_weed_counted(
            buffer,
            *length,
            rest,
            (divisor as u64) << one_shift,
            w_error,
            kappa,
        );
    }

    // The integrals have been generated. We are at the point of the decimal
    // separator. In the following loop we simply multiply the remaining digits by
    // 10 and divide by one. We just need to pay attention to multiply associated
    // data (the 'unit'), too.
    // Note that the multiplication by 10 does not overflow, because w.e >= -60
    // and thus one.e >= -60.
    while requested_digits > 0 && fractionals > w_error {
        fractionals = fractionals.wrapping_mul(10);
        w_error = w_error.wrapping_mul(10);
        // Integer division by one.
        let digit: i32 = (fractionals >> one_shift) as i32;
        buffer[*length as usize] = b'0' + digit as u8;
        *length += 1;
        requested_digits -= 1;
        fractionals &= one.f().wrapping_sub(1); // Modulo by one.
        *kappa -= 1;
    }
    if requested_digits != 0 {
        return false;
    }
    round_weed_counted(buffer, *length, fractionals, one.f(), w_error, kappa)
}

// Provides a decimal representation of v.
// Returns true if it succeeds, otherwise the result cannot be trusted.
// There will be *length digits inside the buffer (not null-terminated).
// If the function returns true then
//        v == (double) (buffer * 10^decimal_exponent).
// The digits in the buffer are the shortest representation possible: no
// 0.09999999999999999 instead of 0.1. The shorter representation will even be
// chosen even if the longer one would be closer to v.
// The last digit will be closest to the actual v. That is, even if several
// digits might correctly yield 'v' when read again, the closest will be
// computed.
fn grisu3(
    v: f64,
    mode: FastDtoaMode,
    buffer: &mut [u8],
    length: &mut i32,
    decimal_exponent: &mut i32,
) -> bool {
    let w = Double::from_f64(v).as_normalized_diy_fp();
    // boundary_minus and boundary_plus are the boundaries between v and its
    // closest floating-point neighbors. Any number strictly between
    // boundary_minus and boundary_plus will round to v when convert to a double.
    // Grisu3 will never output representations that lie exactly on a boundary.
    let mut boundary_minus = DiyFp::new(0, 0);
    let mut boundary_plus = DiyFp::new(0, 0);
    if mode == FastDtoaMode::FAST_DTOA_SHORTEST {
        (boundary_minus, boundary_plus) = Double::from_f64(v).normalized_boundaries();
    } else {
        let single_v: f32 = v as f32;
        (boundary_minus, boundary_plus) = Single::from_f32(single_v).normalized_boundaries();
    }
    let mut ten_mk = DiyFp::new(0, 0); // Cached power of ten: 10^-k
    let mut mk: i32 = 0; // -k
    let ten_mk_minimal_binary_exponent: i32 =
        K_MINIMAL_TARGET_EXPONENT - (w.e() + DiyFp::K_SIGNIFICAND_SIZE as i32);
    let ten_mk_maximal_binary_exponent: i32 =
        K_MAXIMAL_TARGET_EXPONENT - (w.e() + DiyFp::K_SIGNIFICAND_SIZE as i32);
    PowersOfTenCache::get_cached_power_for_binary_exponent_range(
        ten_mk_minimal_binary_exponent,
        ten_mk_maximal_binary_exponent,
        &mut ten_mk,
        &mut mk,
    );
    // Note that ten_mk is only an approximation of 10^-k. A DiyFp only contains a
    // 64 bit significand and ten_mk is thus only precise up to 64 bits.

    // The DiyFp::Times procedure rounds its result, and ten_mk is approximated
    // too. The variable scaled_w (as well as scaled_boundary_minus/plus) are now
    // off by a small amount.
    // In fact: scaled_w - w*10^k < 1ulp (unit in the last place) of scaled_w.
    // In other words: let f = scaled_w.f() and e = scaled_w.e(), then
    //           (f-1) * 2^e < w*10^k < (f+1) * 2^e
    let scaled_w = DiyFp::times(w, ten_mk);
    // In theory it would be possible to avoid some recomputations by computing
    // the difference between w and boundary_minus/plus (a power of 2) and to
    // compute scaled_boundary_minus/plus by subtracting/adding from
    // scaled_w. However the code becomes much less readable and the speed
    // enhancements are not terriffic.
    let scaled_boundary_minus = DiyFp::times(boundary_minus, ten_mk);
    let scaled_boundary_plus = DiyFp::times(boundary_plus, ten_mk);

    // DigitGen will generate the digits of scaled_w. Therefore we have
    // v == (double) (scaled_w * 10^-mk).
    // Set decimal_exponent == -mk and pass it to DigitGen. If scaled_w is not an
    // integer than it will be updated. For instance if scaled_w == 1.23 then
    // the buffer will be filled with "123" und the decimal_exponent will be
    // decreased by 2.
    let mut kappa: i32 = 0;
    let result = digit_gen(
        scaled_boundary_minus,
        scaled_w,
        scaled_boundary_plus,
        buffer,
        length,
        &mut kappa,
    );
    *decimal_exponent = -mk + kappa;
    result
}

// The "counted" version of grisu3 (see above) only generates requested_digits
// number of digits. This version does not generate the shortest representation,
// and with enough requested digits 0.1 will at some point print as 0.9999999...
// Grisu3 is too imprecise for real halfway cases (1.5 will not work) and
// therefore the rounding strategy for halfway cases is irrelevant.
fn grisu3_counted(
    v: f64,
    requested_digits: i32,
    buffer: &mut [u8],
    length: &mut i32,
    decimal_exponent: &mut i32,
) -> bool {
    let w = Double::from_f64(v).as_normalized_diy_fp();
    let mut ten_mk = DiyFp::new(0, 0); // Cached power of ten: 10^-k
    let mut mk: i32 = 0; // -k
    let ten_mk_minimal_binary_exponent: i32 =
        K_MINIMAL_TARGET_EXPONENT - (w.e() + DiyFp::K_SIGNIFICAND_SIZE as i32);
    let ten_mk_maximal_binary_exponent: i32 =
        K_MAXIMAL_TARGET_EXPONENT - (w.e() + DiyFp::K_SIGNIFICAND_SIZE as i32);
    PowersOfTenCache::get_cached_power_for_binary_exponent_range(
        ten_mk_minimal_binary_exponent,
        ten_mk_maximal_binary_exponent,
        &mut ten_mk,
        &mut mk,
    );
    // Note that ten_mk is only an approximation of 10^-k. A DiyFp only contains a
    // 64 bit significand and ten_mk is thus only precise up to 64 bits.

    // The DiyFp::Times procedure rounds its result, and ten_mk is approximated
    // too. The variable scaled_w (as well as scaled_boundary_minus/plus) are now
    // off by a small amount.
    // In fact: scaled_w - w*10^k < 1ulp (unit in the last place) of scaled_w.
    // In other words: let f = scaled_w.f() and e = scaled_w.e(), then
    //           (f-1) * 2^e < w*10^k < (f+1) * 2^e
    let scaled_w = DiyFp::times(w, ten_mk);

    // We now have (double) (scaled_w * 10^-mk).
    // DigitGen will generate the first requested_digits digits of scaled_w and
    // return together with a kappa such that scaled_w ~= buffer * 10^kappa. (It
    // will not always be exactly the same since DigitGenCounted only produces a
    // limited number of digits.)
    let mut kappa: i32 = 0;
    let result = digit_gen_counted(scaled_w, requested_digits, buffer, length, &mut kappa);
    *decimal_exponent = -mk + kappa;
    result
}

// Provides a decimal representation of v.
// The result should be interpreted as buffer * 10^(point - length).
//
// Precondition:
//   * v must be a strictly positive finite double.
//
// Returns true if it succeeds, otherwise the result can not be trusted.
// There will be *length digits inside the buffer followed by a null terminator.
// If the function returns true and mode equals
//   - FAST_DTOA_SHORTEST, then
//     the parameter requested_digits is ignored.
//     The result satisfies
//         v == (double) (buffer * 10^(point - length)).
//     The digits in the buffer are the shortest representation possible. E.g.
//     if 0.099999999999 and 0.1 represent the same double then "1" is returned
//     with point = 0.
//     The last digit will be closest to the actual v. That is, even if several
//     digits might correctly yield 'v' when read again, the buffer will contain
//     the one closest to v.
//   - FAST_DTOA_PRECISION, then
//     the buffer contains requested_digits digits.
//     the difference v - (buffer * 10^(point-length)) is closest to zero for
//     all possible representations of requested_digits digits.
//     If there are two values that are equally close, then FastDtoa returns
//     false.
// For both modes the buffer must be large enough to hold the result.
pub fn fast_dtoa(
    v: f64,
    mode: FastDtoaMode,
    requested_digits: i32,
    buffer: &mut [u8],
    length: &mut i32,
    decimal_point: &mut i32,
) -> bool {
    let mut decimal_exponent: i32 = 0;
    let result = match mode {
        FastDtoaMode::FAST_DTOA_SHORTEST | FastDtoaMode::FAST_DTOA_SHORTEST_SINGLE => {
            grisu3(v, mode, buffer, length, &mut decimal_exponent)
        }
        FastDtoaMode::FAST_DTOA_PRECISION => {
            grisu3_counted(v, requested_digits, buffer, length, &mut decimal_exponent)
        }
    };
    if result {
        *decimal_point = *length + decimal_exponent;
        buffer[*length as usize] = 0;
    }
    result
}

// Testes dependentes dos módulos diy_fp, cached_powers e ieee (escritos em paralelo).
#[cfg(test)]
mod tests {
    use super::*;

    fn shortest(v: f64) -> (bool, String, i32) {
        let mut buffer = [0u8; 32];
        let mut length = 0;
        let mut point = 0;
        let ok = fast_dtoa(
            v,
            FastDtoaMode::FAST_DTOA_SHORTEST,
            0,
            &mut buffer,
            &mut length,
            &mut point,
        );
        let digits = String::from_utf8_lossy(&buffer[..length as usize]).into_owned();
        (ok, digits, point)
    }

    #[test]
    fn shortest_one() {
        assert_eq!(shortest(1.0), (true, "1".to_string(), 1));
    }

    #[test]
    fn shortest_point_one() {
        assert_eq!(shortest(0.1), (true, "1".to_string(), 0));
    }

    #[test]
    fn shortest_one_point_five() {
        assert_eq!(shortest(1.5), (true, "15".to_string(), 1));
    }

    #[test]
    fn precision_integral_digits() {
        let mut buffer = [0u8; 32];
        let mut length = 0;
        let mut point = 0;
        let ok = fast_dtoa(
            123456.0,
            FastDtoaMode::FAST_DTOA_PRECISION,
            3,
            &mut buffer,
            &mut length,
            &mut point,
        );
        assert!(ok);
        assert_eq!(&buffer[..length as usize], b"123");
        assert_eq!(point, 6);
    }

    #[test]
    fn biggest_power_ten_table() {
        let mut power = 0;
        let mut exponent_plus_one = 0;
        biggest_power_ten(1234, 11, &mut power, &mut exponent_plus_one);
        assert_eq!((power, exponent_plus_one), (1000, 4));
    }
}
