//! Porte de `WTF/wtf/dragonbox/dragonbox_to_chars.h` e `dragonbox_to_chars.cpp`: a impressão
//! decimal do resultado do Dragonbox (modos `ToShortest` e `ToExponential`).
//!
//! O `std::span<char>` do C++ que avança com `skip`/`consume` vira `&mut &mut [u8]` e a operação
//! `skip` é `skip(&mut buffer, n)`, que reduz a fatia; a "próxima posição" que o C++ devolve (e
//! que o chamador converte em tamanho com `cursor.data() - buffer.data()`) vira o número de bytes
//! escritos.

use crate::wtf::dragonbox::dragonbox::{
    compute_power_with_max_16, count_digits_base10_with_max_17, max_string_length,
    to_decimal, to_exponential_max_string_length, valid_shortest_representation, ComputeMulImpl,
    Mode, PrintTrailingZero,
};
use crate::wtf::dragonbox::ieee754_format::{FloatBits, FloatFormat, FloatTraits, Ieee754Binary64};
use crate::wtf::dtoa::utils::StringBuilder;

/// `radix_100_table`: os pares de dígitos "00" a "99".
static RADIX_100_TABLE: [u8; 200] = {
    let mut table = [0u8; 200];
    let mut n = 0;
    while n < 100 {
        table[n * 2] = b'0' + (n / 10) as u8;
        table[n * 2 + 1] = b'0' + (n % 10) as u8;
        n += 1;
    }
    table
};

/// `radix_100_head_table`: para cada `n` de 0 a 99, o primeiro dígito de `n` seguido de '.'.
static RADIX_100_HEAD_TABLE: [u8; 200] = {
    let mut table = [0u8; 200];
    let mut n = 0;
    while n < 100 {
        table[n * 2] = if n < 10 { b'0' + n as u8 } else { b'0' + (n / 10) as u8 };
        table[n * 2 + 1] = b'.';
        n += 1;
    }
    table
};

/// `skip(std::span<T>&, n)`.
fn skip(data: &mut &mut [u8], amount_to_skip: u32) {
    let taken = std::mem::take(data);
    *data = &mut taken[amount_to_skip as usize..];
}

/// `consume(buffer) = c`.
fn consume(data: &mut &mut [u8], c: u8) {
    data[0] = c;
    skip(data, 1);
}

/// `memcpySpan(buffer, source)`.
fn memcpy_span(destination: &mut [u8], source: &[u8]) {
    destination[..source.len()].copy_from_slice(source);
}

fn print_1_digit(n: u32, buffer: &mut [u8]) {
    const _: () = assert!(b'0' & 0xf == 0);
    buffer[0] = (b'0' as u32 | n) as u8;
}

fn print_2_digits(n: u32, buffer: &mut [u8]) {
    let index = (n * 2) as usize;
    memcpy_span(buffer, &RADIX_100_TABLE[index..index + 2]);
}

/// Copia `count` caracteres da tabela de cabeças a partir do dígito `head_digits`.
fn print_head_digits(head_digits: u32, count: u32, buffer: &mut [u8]) {
    let index = (head_digits * 2) as usize;
    memcpy_span(buffer, &RADIX_100_HEAD_TABLE[index..index + count as usize]);
}

// These digit generation routines are inspired by James Anhalt's itoa algorithm:
// https://github.com/jeaiii/itoa
// The main idea is for given n, find y such that floor(10^k * y / 2^32) = n holds,
// where k is an appropriate integer depending on the length of n.
// For example, if n = 1234567, we set k = 6. In this case, we have
// floor(y / 2^32) = 1,
// floor(10^2 * ((10^0 * y) mod 2^32) / 2^32) = 23,
// floor(10^2 * ((10^2 * y) mod 2^32) / 2^32) = 45, and
// floor(10^2 * ((10^4 * y) mod 2^32) / 2^32) = 67.
// See https://jk-jeon.github.io/posts/2022/02/jeaiii-algorithm/ for more explanation.
// Note that this fuction ignores trailing zeros.

fn print_9_digits(mode: Mode, s32: u32, exponent: &mut i32, buffer: &mut &mut [u8]) {
    debug_assert!(s32 != 0);

    // If ToExponential mode then print "d." else just print "d".
    let first_head_digit_chars_count = mode as u32;
    let fhd = first_head_digit_chars_count as usize;

    // -- IEEE-754 binary32
    // Since we do not cut trailing zeros in advance, s32 must be of 6~9 digits
    // unless the original input was subnormal.
    // In particular, when it is of 9 digits it shouldn't have any trailing zeros.
    // -- IEEE-754 binary64
    // In this case, s32 must be of 7~9 digits unless the input is subnormal,
    // and it shouldn't have any trailing zeros if it is of 9 digits.
    if s32 >= 1_0000_0000 {
        // 9 digits.
        // 1441151882 = ceil(2^57 / 1'0000'0000) + 1
        let mut prod = s32 as u64 * 1441151882u64;
        prod >>= 25;
        print_head_digits((prod >> 32) as u32, first_head_digit_chars_count, buffer);

        prod = (prod as u32) as u64 * 100u64;
        print_2_digits((prod >> 32) as u32, &mut buffer[fhd..]);
        prod = (prod as u32) as u64 * 100u64;
        print_2_digits((prod >> 32) as u32, &mut buffer[fhd + 2..]);
        prod = (prod as u32) as u64 * 100u64;
        print_2_digits((prod >> 32) as u32, &mut buffer[fhd + 4..]);
        prod = (prod as u32) as u64 * 100u64;
        print_2_digits((prod >> 32) as u32, &mut buffer[fhd + 6..]);

        *exponent += 8;
        skip(buffer, 8 + first_head_digit_chars_count);
    } else if s32 >= 100_0000 {
        // 7 or 8 digits which consist of the head digits (one or two digits) and the remaining 6 digits (d1, d2, d3, d4, d5, d6).

        // 281474978 = ceil(2^48 / 100'0000) + 1
        let mut prod = s32 as u64 * 281474978u64;
        prod >>= 16;
        let head_digits = (prod >> 32) as u32;
        // Has 2nd head digit also means it's of 8 digits.
        let has_second_head_digit = (head_digits >= 10) as u32;

        // If s32 is of 8 digits, increase the exponent by 7. Otherwise, increase it by 6.
        *exponent += (6 + has_second_head_digit) as i32;

        let first_head_digit_index = head_digits * 2;
        // Write the first head digit and the decimal point if needed.
        print_head_digits(head_digits, first_head_digit_chars_count, buffer);
        // Write the second head digit. This character may be overwritten later but we don't care.
        buffer[fhd] = RADIX_100_TABLE[(first_head_digit_index + 1) as usize];

        if (prod as u32) <= ((1u64 << 32) / 100_0000) as u32 {
            // The remaining 6 digits (d1, d2, d3, d4, d5, d6) are all zero. Then, the number of characters actually need to be written is:
            //   1. Only the first digit is nonzero, which means that either s32 is of 7 digits or it is of 8 digits but the second digit is zero.
            //      Then, we only need the first digit in the buffer.
            //   2. Otherwise, we need first_head_digit_chars_count + 1 digits in the buffer.
            // Note that the first digit is never '0' if s32 is of 7 digits, because the input is never zero.
            let has_non_zero_second_head_digit =
                has_second_head_digit & (buffer[fhd] > b'0') as u32;
            skip(buffer, 1 + has_non_zero_second_head_digit * first_head_digit_chars_count);
        } else {
            // At least one of the remaining 6 digits are nonzero.

            // After this adjustment, now the first destination becomes buffer + first_head_digit_chars_count.
            skip(buffer, has_second_head_digit);

            // Obtain the next two digits (d1, d2).
            prod = (prod as u32) as u64 * 100u64;
            let d1_index = first_head_digit_chars_count;
            print_2_digits((prod >> 32) as u32, &mut buffer[d1_index as usize..]);

            if (prod as u32) <= ((1u64 << 32) / 1_0000) as u32 {
                // The remaining 4 digits (d3, d4, d5, d6) are all zero.
                let d2_index = d1_index + 1;
                let has_non_zero_d2 = (buffer[d2_index as usize] > b'0') as u32;
                skip(buffer, d2_index + has_non_zero_d2);
            } else {
                // At least one of the remaining 4 digits are nonzero.
                // Obtain the next two digits (d3, d4).
                prod = (prod as u32) as u64 * 100u64;
                let d3_index = d1_index + 2;
                print_2_digits((prod >> 32) as u32, &mut buffer[d3_index as usize..]);

                if (prod as u32) <= ((1u64 << 32) / 100) as u32 {
                    // The remaining 2 digits (d5, d6) are all zero.
                    let d4_index = d3_index + 1;
                    let has_non_zero_d4 = (buffer[d4_index as usize] > b'0') as u32;
                    skip(buffer, d4_index + has_non_zero_d4);
                } else {
                    // Obtain the last two digits (d5, d6).
                    prod = (prod as u32) as u64 * 100u64;
                    let d5_index = d3_index + 2;
                    print_2_digits((prod >> 32) as u32, &mut buffer[d5_index as usize..]);

                    let d6_index = d5_index + 1;
                    let has_non_zero_d6 = (buffer[d6_index as usize] > b'0') as u32;
                    skip(buffer, d6_index + has_non_zero_d6);
                }
            }
        }
    } else if s32 >= 1_0000 {
        // 5 or 6 digits which consist of the head digits (one or two digits) and the remaining 4 digits (d1, d2, d3, d4).

        // 429497 = ceil(2^32 / 1'0000)
        let mut prod = s32 as u64 * 429497u64;
        let head_digits = (prod >> 32) as u32;
        // Has 2nd head digit also means it's of 6 digits.
        let has_second_head_digit = (head_digits >= 10) as u32;

        // If s32 is of 6 digits, increase the exponent by 5. Otherwise, increase it by 4.
        *exponent += (4 + has_second_head_digit) as i32;

        let first_head_digit_index = head_digits * 2;
        // Write the first head digit and the decimal point if needed.
        print_head_digits(head_digits, first_head_digit_chars_count, buffer);
        // Write the second head digit. This character may be overwritten later but we don't care.
        buffer[fhd] = RADIX_100_TABLE[(first_head_digit_index + 1) as usize];

        if (prod as u32) <= ((1u64 << 32) / 1_0000) as u32 {
            // The remaining 4 digits (d1, d2, d3, d4) are all zero.
            // The number of characters actually written is 1 or first_head_digit_chars_count + 1, similarly to the case of 7 or 8 digits.
            let has_non_zero_second_head_digit =
                has_second_head_digit & (buffer[fhd] > b'0') as u32;
            skip(buffer, 1 + has_non_zero_second_head_digit * first_head_digit_chars_count);
        } else {
            // At least one of the remaining 4 digits are nonzero.

            // After this adjustment, now the first destination becomes buffer + first_head_digit_chars_count.
            skip(buffer, has_second_head_digit);

            // Obtain the next two digits (d1, d2).
            prod = (prod as u32) as u64 * 100u64;
            let d1_index = first_head_digit_chars_count;
            print_2_digits((prod >> 32) as u32, &mut buffer[d1_index as usize..]);

            if (prod as u32) <= ((1u64 << 32) / 100) as u32 {
                // The remaining 2 digits (d3, d4) are all zero.
                let d2_index = d1_index + 1;
                let has_non_zero_d2 = (buffer[d2_index as usize] > b'0') as u32;
                skip(buffer, d2_index + has_non_zero_d2);
            } else {
                // Obtain the last two digits (d3, d4).
                prod = (prod as u32) as u64 * 100u64;
                let d3_index = d1_index + 2;
                print_2_digits((prod >> 32) as u32, &mut buffer[d3_index as usize..]);

                let d4_index = d3_index + 1;
                let has_non_zero_d4 = (buffer[d4_index as usize] > b'0') as u32;
                skip(buffer, d4_index + has_non_zero_d4);
            }
        }
    } else if s32 >= 100 {
        // 3 or 4 digits which consist of the head digits (one or two digits) and the remaining 2 digits (d1, d2).

        // 42949673 = ceil(2^32 / 100)
        let mut prod = s32 as u64 * 42949673u64;
        let head_digits = (prod >> 32) as u32;
        // Has 2nd head digit also means it's of 4 digits.
        let has_second_head_digit = (head_digits >= 10) as u32;

        // If s32 is of 4 digits, increase the exponent by 3. Otherwise, increase it by 2.
        *exponent += (2 + has_second_head_digit) as i32;

        let first_head_digit_index = head_digits * 2;
        // Write the first head digit and the decimal point if needed.
        print_head_digits(head_digits, first_head_digit_chars_count, buffer);
        // Write the second head digit. This character may be overwritten later but we don't care.
        buffer[fhd] = RADIX_100_TABLE[(first_head_digit_index + 1) as usize];

        if (prod as u32) <= ((1u64 << 32) / 100) as u32 {
            // The remaining 2 digits (d1, d2) are all zero.
            // The number of characters actually written is 1 or first_head_digit_chars_count + 1, similarly to the case of 7 or 8 digits.
            let has_non_zero_second_head_digit =
                has_second_head_digit & (buffer[fhd] > b'0') as u32;
            skip(buffer, 1 + has_non_zero_second_head_digit * first_head_digit_chars_count);
        } else {
            // At least one of the remaining 2 digits (d1, d2) are nonzero.

            // After this adjustment, now the first destination becomes buffer + first_head_digit_chars_count.
            skip(buffer, has_second_head_digit);

            // Obtain the last two digits.
            prod = (prod as u32) as u64 * 100u64;
            let d1_index = first_head_digit_chars_count;
            print_2_digits((prod >> 32) as u32, &mut buffer[d1_index as usize..]);

            let d2_index = d1_index + 1;
            let has_non_zero_d2 = (buffer[d2_index as usize] > b'0') as u32;
            skip(buffer, d2_index + has_non_zero_d2);
        }
    } else {
        // 1 or 2 digits which consist of the head digits (one or two digits).

        // Has 2nd head digit also means it's of 2 digits.
        let has_second_head_digit = (s32 >= 10) as u32;
        // If s32 is of 2 digits, increase the exponent by 1.
        *exponent += has_second_head_digit as i32;

        let first_head_digit_index = s32 * 2;
        // Write the first head digit and the decimal point if needed.
        print_head_digits(s32, first_head_digit_chars_count, buffer);
        // Write the second head digit. This character may be overwritten later but we don't care.
        buffer[fhd] = RADIX_100_TABLE[(first_head_digit_index + 1) as usize];

        // The number of characters actually written is 1 or first_head_digit_chars_count + 1, similarly to the case of 7 or 8 digits.
        let has_non_zero_second_head_digit = has_second_head_digit & (buffer[fhd] > b'0') as u32;
        skip(buffer, 1 + has_non_zero_second_head_digit * first_head_digit_chars_count);
    }
}

/// `float_to_chars_impl`: o modo `ToExponential` do binary32.
fn float_to_chars_impl(significand: u32, mut exponent: i32, buffer: &mut [u8]) -> &mut [u8] {
    let mut buffer = buffer;

    // Print significand.
    print_9_digits(Mode::ToExponential, significand, &mut exponent, &mut buffer);

    // Print exponent and return
    if exponent < 0 {
        memcpy_span(buffer, b"e-");
        skip(&mut buffer, 2);
        exponent = -exponent;
    } else {
        memcpy_span(buffer, b"e+");
        skip(&mut buffer, 2);
    }

    if exponent >= 10 {
        print_2_digits(exponent as u32, buffer);
        skip(&mut buffer, 2);
    } else {
        print_1_digit(exponent as u32, buffer);
        skip(&mut buffer, 1);
    }

    buffer
}

fn double_to_chars_impl(
    mode: Mode,
    print_trailing_zero: PrintTrailingZero,
    significand: u64,
    mut exponent: i32,
    buffer: &mut [u8],
) -> &mut [u8] {
    let mut buffer = buffer;

    // Print significand by decomposing it into a 9-digit block and a 8-digit block.
    let first_block: u32;
    let mut second_block: u32 = 0;
    let mut has_second_block = true;

    if significand >= 1_0000_0000 {
        first_block = (significand / 1_0000_0000) as u32;
        second_block = (significand as u32).wrapping_sub(first_block.wrapping_mul(1_0000_0000));
        exponent += 8;
    } else {
        first_block = significand as u32;
        has_second_block = false;
    }

    if second_block == 0 && print_trailing_zero == PrintTrailingZero::No {
        print_9_digits(mode, first_block, &mut exponent, &mut buffer);
    } else {
        // If need to print the decimal point then print "d." else just print "d".
        let first_head_digit_chars_count = mode as u32;
        let fhd = first_head_digit_chars_count as usize;

        if first_block >= 1_0000_0000 {
            // We proceed similarly to print_9_digits(), but since we do not need to remove
            // trailing zeros, the procedure is a bit simpler.

            // The input is of 17 digits, thus there should be no trailing zero at all.
            // The first block is of 9 digits.
            // 1441151882 = ceil(2^57 / 1'0000'0000) + 1
            let mut prod = first_block as u64 * 1441151882u64;
            prod >>= 25;
            print_head_digits((prod >> 32) as u32, first_head_digit_chars_count, buffer);
            prod = (prod as u32) as u64 * 100u64;
            print_2_digits((prod >> 32) as u32, &mut buffer[fhd..]);
            prod = (prod as u32) as u64 * 100u64;
            print_2_digits((prod >> 32) as u32, &mut buffer[fhd + 2..]);
            prod = (prod as u32) as u64 * 100u64;
            print_2_digits((prod >> 32) as u32, &mut buffer[fhd + 4..]);
            prod = (prod as u32) as u64 * 100u64;
            print_2_digits((prod >> 32) as u32, &mut buffer[fhd + 6..]);

            // The second block is of 8 digits.
            // 281474978 = ceil(2^48 / 100'0000) + 1
            prod = second_block as u64 * 281474978u64;
            prod >>= 16;
            prod += 1;
            print_2_digits((prod >> 32) as u32, &mut buffer[fhd + 8..]);
            prod = (prod as u32) as u64 * 100u64;
            print_2_digits((prod >> 32) as u32, &mut buffer[fhd + 10..]);
            prod = (prod as u32) as u64 * 100u64;
            print_2_digits((prod >> 32) as u32, &mut buffer[fhd + 12..]);
            prod = (prod as u32) as u64 * 100u64;
            print_2_digits((prod >> 32) as u32, &mut buffer[fhd + 14..]);

            exponent += 8;
            skip(&mut buffer, first_head_digit_chars_count + 16);
        } else {
            if first_block >= 100_0000 {
                // 7 or 8 digits which consist of the head digits (one or two digits) and the remaining 6 digits.
                // 281474978 = ceil(2^48 / 100'0000) + 1
                let mut prod = first_block as u64 * 281474978u64;
                prod >>= 16;
                let head_digits = (prod >> 32) as u32;
                // Has 2nd head digit also means it's of 8 digits.
                let has_second_head_digit = (head_digits >= 10) as u32;

                let first_head_digit_index = head_digits * 2;
                print_head_digits(head_digits, first_head_digit_chars_count, buffer);
                buffer[fhd] = RADIX_100_TABLE[(first_head_digit_index + 1) as usize];

                exponent += (6 + has_second_head_digit) as i32;
                skip(&mut buffer, has_second_head_digit);

                // Print remaining 6 digits.
                prod = (prod as u32) as u64 * 100u64;
                print_2_digits((prod >> 32) as u32, &mut buffer[fhd..]);
                prod = (prod as u32) as u64 * 100u64;
                print_2_digits((prod >> 32) as u32, &mut buffer[fhd + 2..]);
                prod = (prod as u32) as u64 * 100u64;
                print_2_digits((prod >> 32) as u32, &mut buffer[fhd + 4..]);

                skip(&mut buffer, first_head_digit_chars_count + 6);
            } else if first_block >= 1_0000 {
                // 5 or 6 digits which consist of the head digits (one or two digits) and the remaining 4 digits.

                // 429497 = ceil(2^32 / 1'0000)
                let mut prod = first_block as u64 * 429497u64;
                let head_digits = (prod >> 32) as u32;
                // Has 2nd head digit also means it's of 6 digits.
                let has_second_head_digit = (head_digits >= 10) as u32;

                let first_head_digit_index = head_digits * 2;
                print_head_digits(head_digits, first_head_digit_chars_count, buffer);
                buffer[fhd] = RADIX_100_TABLE[(first_head_digit_index + 1) as usize];

                exponent += (4 + has_second_head_digit) as i32;
                skip(&mut buffer, has_second_head_digit);

                // Print remaining 4 digits.
                prod = (prod as u32) as u64 * 100u64;
                print_2_digits((prod >> 32) as u32, &mut buffer[fhd..]);
                prod = (prod as u32) as u64 * 100u64;
                print_2_digits((prod >> 32) as u32, &mut buffer[fhd + 2..]);

                skip(&mut buffer, first_head_digit_chars_count + 4);
            } else if first_block >= 100 {
                // 3 or 4 digits which consist of the head digits (one or two digits) and the remaining 2 digits.

                // 42949673 = ceil(2^32 / 100)
                let mut prod = first_block as u64 * 42949673u64;
                let head_digits = (prod >> 32) as u32;
                // Has 2nd head digit also means it's of 4 digits.
                let has_second_head_digit = (head_digits >= 10) as u32;

                let first_head_digit_index = head_digits * 2;
                print_head_digits(head_digits, first_head_digit_chars_count, buffer);
                buffer[fhd] = RADIX_100_TABLE[(first_head_digit_index + 1) as usize];

                exponent += (2 + has_second_head_digit) as i32;
                skip(&mut buffer, has_second_head_digit);

                // Print remaining 2 digits.
                prod = (prod as u32) as u64 * 100u64;
                print_2_digits((prod >> 32) as u32, &mut buffer[fhd..]);

                skip(&mut buffer, first_head_digit_chars_count + 2);
            } else {
                // 1 or 2 digits which consist of the head digits (one or two digits).

                // Has 2nd head digit also means it's of 2 digits.
                let has_second_head_digit = (first_block >= 10) as u32;

                let first_head_digit_index = first_block * 2;
                print_head_digits(first_block, first_head_digit_chars_count, buffer);
                buffer[fhd] = RADIX_100_TABLE[(first_head_digit_index + 1) as usize];

                exponent += has_second_head_digit as i32;
                skip(&mut buffer, first_head_digit_chars_count + has_second_head_digit);
            }

            // Next, print the second block. The second block is of 8 digits, but we may have trailing zeros.
            if has_second_block {
                // 281474978 = ceil(2^48 / 100'0000) + 1
                let mut prod = second_block as u64 * 281474978u64;
                prod >>= 16;
                prod += 1;
                print_2_digits((prod >> 32) as u32, buffer);

                if print_trailing_zero == PrintTrailingZero::No {
                    // Remaining 6 digits are all zero?
                    if (prod as u32) <= ((1u64 << 32) / 100_0000) as u32 {
                        { let n = 1 + (buffer[1] > b'0') as u32; skip(&mut buffer, n); }
                    } else {
                        // Obtain the next two digits.
                        prod = (prod as u32) as u64 * 100u64;
                        print_2_digits((prod >> 32) as u32, &mut buffer[2..]);

                        // Remaining 4 digits are all zero?
                        if (prod as u32) <= ((1u64 << 32) / 1_0000) as u32 {
                            { let n = 3 + (buffer[3] > b'0') as u32; skip(&mut buffer, n); }
                        } else {
                            // Obtain the next two digits.
                            prod = (prod as u32) as u64 * 100u64;
                            print_2_digits((prod >> 32) as u32, &mut buffer[4..]);

                            // Remaining 2 digits are all zero?
                            if (prod as u32) <= ((1u64 << 32) / 100) as u32 {
                                { let n = 5 + (buffer[5] > b'0') as u32; skip(&mut buffer, n); }
                            } else {
                                // Obtain the last two digits.
                                prod = (prod as u32) as u64 * 100u64;
                                print_2_digits((prod >> 32) as u32, &mut buffer[6..]);
                                { let n = 7 + (buffer[7] > b'0') as u32; skip(&mut buffer, n); }
                            }
                        }
                    }
                } else {
                    // Obtain the remaining 6 digits.
                    prod = (prod as u32) as u64 * 100u64;
                    print_2_digits((prod >> 32) as u32, &mut buffer[2..]);
                    prod = (prod as u32) as u64 * 100u64;
                    print_2_digits((prod >> 32) as u32, &mut buffer[4..]);
                    prod = (prod as u32) as u64 * 100u64;
                    print_2_digits((prod >> 32) as u32, &mut buffer[6..]);
                    skip(&mut buffer, 8);
                }
            }
        }
    }

    if mode == Mode::ToShortest {
        return buffer;
    }

    // Print exponent and return
    if exponent < 0 {
        memcpy_span(buffer, b"e-");
        skip(&mut buffer, 2);
        exponent = -exponent;
    } else {
        memcpy_span(buffer, b"e+");
        skip(&mut buffer, 2);
    }

    if exponent >= 100 {
        // d1 = exponent / 10; d2 = exponent % 10;
        // 6554 = ceil(2^16 / 10)
        let mut prod = (exponent as u32).wrapping_mul(6554u32);
        let d1 = prod >> 16;
        prod = (prod as u16) as u32 * 5u32; // * 10
        let d2 = prod >> 15; // >> 16
        print_2_digits(d1, buffer);
        print_1_digit(d2, &mut buffer[2..]);
        skip(&mut buffer, 3);
    } else if exponent >= 10 {
        print_2_digits(exponent as u32, buffer);
        skip(&mut buffer, 2);
    } else {
        print_1_digit(exponent as u32, buffer);
        skip(&mut buffer, 1);
    }

    buffer
}

/// `to_chars_impl<Float, default_float_traits<Float>, Mode::ToExponential, PrintTrailingZero::No>`:
/// a especialização por tipo do modo exponencial.
pub trait ToCharsImpl: FloatTraits {
    fn to_chars_impl_exponential(significand: u64, exponent: i32, buffer: &mut [u8]) -> &mut [u8];
}

impl ToCharsImpl for f32 {
    fn to_chars_impl_exponential(significand: u64, exponent: i32, buffer: &mut [u8]) -> &mut [u8] {
        float_to_chars_impl(significand as u32, exponent, buffer)
    }
}

impl ToCharsImpl for f64 {
    fn to_chars_impl_exponential(significand: u64, exponent: i32, buffer: &mut [u8]) -> &mut [u8] {
        double_to_chars_impl(Mode::ToExponential, PrintTrailingZero::No, significand, exponent, buffer)
    }
}

/// `detail::to_shortest`.
pub fn to_shortest(significand: u64, exponent: i32, buffer: &mut [u8]) -> &mut [u8] {
    let mut buffer = buffer;
    debug_assert!(significand != 0);

    // significand = 12340, exponent = -2, result = 123.4
    // significand = 12345, exponent = -2, result = 123.45
    // significand = 12345, exponent =  2, result = 1234500
    let significand_digits_count = count_digits_base10_with_max_17(significand) as i32;
    let mut integral_digits_count = significand_digits_count + exponent;
    if !valid_shortest_representation(integral_digits_count) {
        return double_to_chars_impl(
            Mode::ToExponential,
            PrintTrailingZero::No,
            significand,
            exponent,
            buffer,
        );
    }

    if exponent >= 0 {
        buffer = double_to_chars_impl(
            Mode::ToShortest,
            PrintTrailingZero::Yes,
            significand,
            exponent,
            buffer,
        );
        let mut remaining = exponent;
        while remaining != 0 {
            remaining -= 1;
            consume(&mut buffer, b'0');
        }
        return buffer;
    }

    if integral_digits_count > 0 {
        // significand = 12345, exponent = -2, significand_digits_count = 5, integral_digits_count = 3, fractional_digits_count = 2
        let fractional_digits_count = significand_digits_count - integral_digits_count;
        debug_assert!(
            0 < fractional_digits_count
                && fractional_digits_count < Ieee754Binary64::DECIMAL_DIGITS
                && fractional_digits_count == -exponent
        );
        debug_assert!(
            0 < integral_digits_count && integral_digits_count < Ieee754Binary64::DECIMAL_DIGITS
        );
        let base = compute_power_with_max_16(10, fractional_digits_count as u32);

        // Obtain and write the integral part.
        let integral = significand / base;
        buffer = double_to_chars_impl(
            Mode::ToShortest,
            PrintTrailingZero::Yes,
            integral,
            exponent,
            buffer,
        );

        // Obtain and write the fractional part if needed.
        let fractional = significand % base;
        if fractional != 0 {
            // Given this case:
            //     significand = 12305, exponent = -2, integral_digits_count = 3, fractional_digits_count = 2, integral = 123, fractional = 5
            // Write ".0" first and then write "5".
            let mut actual_fractional_digits_count =
                count_digits_base10_with_max_17(fractional) as i32;
            consume(&mut buffer, b'.');
            while {
                let keep_going = actual_fractional_digits_count < fractional_digits_count;
                actual_fractional_digits_count += 1;
                keep_going
            } {
                consume(&mut buffer, b'0');
            }
            buffer = double_to_chars_impl(
                Mode::ToShortest,
                PrintTrailingZero::No,
                fractional,
                exponent,
                buffer,
            );
        }
    } else {
        // Given this case:
        //     significand = 12345, exponent = -7, integral_digits_count = -2, result = 0.0012345
        // Write "0.00" first and then write "12345".
        memcpy_span(buffer, b"0.");
        skip(&mut buffer, 2);
        while {
            let keep_going = integral_digits_count < 0;
            integral_digits_count += 1;
            keep_going
        } {
            consume(&mut buffer, b'0');
        }
        buffer = double_to_chars_impl(
            Mode::ToShortest,
            PrintTrailingZero::No,
            significand,
            exponent,
            buffer,
        );
    }
    buffer
}

/// `to_chars_n_impl<mode, policy_holder>`: devolve o restante do buffer depois do texto escrito.
fn to_chars_n_impl<T: ToCharsImpl>(mode: Mode, br: FloatBits<T>, buffer: &mut [u8]) -> &mut [u8]
where
    T::Format: ComputeMulImpl,
{
    let mut buffer = buffer;
    let exponent_bits = br.extract_exponent_bits();
    let s = br.remove_exponent_bits(exponent_bits);

    if br.is_finite_with(exponent_bits) {
        if s.is_negative() && br.is_nonzero() {
            consume(&mut buffer, b'-');
        }
        if br.is_nonzero() {
            let result = to_decimal::<T>(s, exponent_bits);

            match mode {
                Mode::ToShortest => to_shortest(result.significand, result.exponent, buffer),
                Mode::ToExponential => {
                    T::to_chars_impl_exponential(result.significand, result.exponent, buffer)
                }
            }
        } else {
            match mode {
                Mode::ToShortest => {
                    consume(&mut buffer, b'0');
                    buffer
                }
                Mode::ToExponential => {
                    memcpy_span(buffer, b"0e+0");
                    skip(&mut buffer, 4);
                    buffer
                }
            }
        }
    } else {
        if s.has_all_zero_significand_bits() {
            if s.is_negative() {
                consume(&mut buffer, b'-');
            }
            memcpy_span(buffer, b"Infinity");
            skip(&mut buffer, 8);
            return buffer;
        }
        memcpy_span(buffer, b"NaN");
        skip(&mut buffer, 3);
        buffer
    }
}

/// `to_chars_n<mode>(x, buffer)`: escreve `x` em `buffer` (sem terminador) e devolve a posição do
/// próximo byte, que é também o número de bytes escritos.
pub fn to_chars_n<T: ToCharsImpl>(mode: Mode, x: T, buffer: &mut [u8]) -> usize
where
    T::Format: ComputeMulImpl,
{
    let total = buffer.len();
    let rest = to_chars_n_impl::<T>(mode, FloatBits::<T>::new(x), buffer);
    total - rest.len()
}

/// `to_chars<mode>(x, buffer)`: como `to_chars_n`, mais o terminador nulo logo depois do texto.
pub fn to_chars<T: ToCharsImpl>(mode: Mode, x: T, buffer: &mut [u8]) -> usize
where
    T::Format: ComputeMulImpl,
{
    let position = to_chars_n::<T>(mode, x, buffer);
    buffer[position] = 0;
    position
}

/// Tamanho do buffer local de `ToExponential`/`ToShortest` (`1 + max_string_length`), no pior dos
/// dois formatos.
const LOCAL_BUFFER_LENGTH: usize = 1 + max_string_length::<Ieee754Binary64>();

/// `ToExponential<Float>(value, result_builder)`.
pub fn to_exponential<T: ToCharsImpl>(value: T, result_builder: &mut StringBuilder)
where
    T::Format: ComputeMulImpl,
{
    const _: () = assert!(
        to_exponential_max_string_length::<Ieee754Binary64>() < LOCAL_BUFFER_LENGTH
    );
    let mut buffer = [0u8; LOCAL_BUFFER_LENGTH];
    let length = to_chars_n::<T>(Mode::ToExponential, value, &mut buffer);
    result_builder.add_substring_span(&buffer[..length]);
}

/// `ToShortest<Float>(value, result_builder)`.
pub fn to_shortest_builder<T: ToCharsImpl>(value: T, result_builder: &mut StringBuilder)
where
    T::Format: ComputeMulImpl,
{
    let mut buffer = [0u8; LOCAL_BUFFER_LENGTH];
    let length = to_chars_n::<T>(Mode::ToShortest, value, &mut buffer);
    result_builder.add_substring_span(&buffer[..length]);
}
