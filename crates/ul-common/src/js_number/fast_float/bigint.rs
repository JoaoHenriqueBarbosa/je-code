//! Porte de `WTF/wtf/fast_float/fast_float.h`, seção `FASTFLOAT_BIGINT_H` (linhas 3469 a 4093 do
//! amálgama).
//!
//! Mapeamentos:
//!
//! - `FASTFLOAT_64BIT` vale em Linux x86_64 e `__sparc` não, então vale o ramo de limb de 64 bits
//!   (`FASTFLOAT_64BIT_LIMB`); o ramo de 32 bits (e a tabela `large_power_of_5` de 32 bits) some.
//! - `stackvec<uint16_t size>` vira `StackVec<const SIZE: usize>`, com array fixo e `length`.
//!   Sem `unsafe`, o ponteiro de `limb_span` é o slice (ver `span` em `float_common`).
//! - `FASTFLOAT_TRY(x)` vira `if !x { return false; }`. `FASTFLOAT_ASSERT` avalia e descarta;
//!   `FASTFLOAT_DEBUG_ASSERT` vira `debug_assert!`.
//! - As sobrecargas do C++ ganham sufixo numérico (`uint64_hi64_1`, `uint64_hi64_2`...).
//! - `pow5_tables<>` é uma classe base; suas constantes viram `pub const`/`pub static` do módulo.
//! - Aritmética sem sinal que estoura no C++ vira `wrapping_*`/`overflowing_*`.
#![allow(non_camel_case_types, non_upper_case_globals)]

use super::float_common::{leading_zeroes, span};

// the limb width: we want efficient multiplication of double the bits in limb, or for 64-bit
// limbs, at least 64-bit multiplication where we can extract the high and low parts efficiently.
// this is every 64-bit architecture except for sparc, which emulates 128-bit multiplication.
// we might have platforms where `CHAR_BIT` is not 8, so let's avoid doing `8 * sizeof(limb)`.
pub type limb = u64;
pub const limb_bits: usize = 64;

pub type limb_span<'a> = span<'a, limb>;

// number of bits in a bigint. this needs to be at least the number of bits required to store the
// largest bigint, which is `log2(10**(digits + max_exp))`, or `log2(10**(767 + 342))`, or ~3600
// bits, so we round to 4000.
pub const bigint_bits: usize = 4000;
pub const bigint_limbs: usize = bigint_bits / limb_bits;

/// vector-like type that is allocated on the stack. the entire buffer is pre-allocated, and only
/// the length changes. (`SIZE` é o `uint16_t size` do template.)
pub struct StackVec<const SIZE: usize> {
    pub data: [limb; SIZE],
    // we never need more than 150 limbs
    pub length: u16,
}

impl<const SIZE: usize> std::ops::Index<usize> for StackVec<SIZE> {
    type Output = limb;

    fn index(&self, index: usize) -> &limb {
        debug_assert!(index < self.length as usize);
        &self.data[index]
    }
}

impl<const SIZE: usize> std::ops::IndexMut<usize> for StackVec<SIZE> {
    fn index_mut(&mut self, index: usize) -> &mut limb {
        debug_assert!(index < self.length as usize);
        &mut self.data[index]
    }
}

impl<const SIZE: usize> StackVec<SIZE> {
    /// `stackvec() = default;`: o C++ deixa `data` sem inicializar, aqui vale zero.
    pub fn new() -> Self {
        StackVec { data: [0; SIZE], length: 0 }
    }

    /// create stack vector from existing limb span.
    pub fn from_span(s: limb_span) -> Self {
        let mut v = Self::new();
        let _ = v.try_extend(s); // FASTFLOAT_ASSERT(try_extend(s))
        v
    }

    /// index from the end of the container
    pub fn rindex(&self, index: usize) -> &limb {
        debug_assert!(index < self.length as usize);
        let rindex: usize = self.length as usize - index - 1;
        &self.data[rindex]
    }

    /// set the length, without bounds checking.
    pub fn set_len(&mut self, len: usize) {
        self.length = len as u16;
    }

    pub fn len(&self) -> usize {
        self.length as usize
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn capacity(&self) -> usize {
        SIZE
    }

    /// append item to vector, without bounds checking
    pub fn push_unchecked(&mut self, value: limb) {
        self.data[self.length as usize] = value;
        self.length += 1;
    }

    /// append item to vector, returning if item was added
    pub fn try_push(&mut self, value: limb) -> bool {
        if self.len() < self.capacity() {
            self.push_unchecked(value);
            true
        } else {
            false
        }
    }

    /// add items to the vector, from a span, without bounds checking
    pub fn extend_unchecked(&mut self, s: limb_span) {
        let start: usize = self.length as usize;
        self.data[start..start + s.len()].copy_from_slice(&s.ptr[..s.len()]);
        self.set_len(self.len() + s.len());
    }

    /// try to add items to the vector, returning if items were added
    pub fn try_extend(&mut self, s: limb_span) -> bool {
        if self.len() + s.len() <= self.capacity() {
            self.extend_unchecked(s);
            true
        } else {
            false
        }
    }

    /// resize the vector, without bounds checking
    /// if the new size is longer than the vector, assign value to each
    /// appended item.
    pub fn resize_unchecked(&mut self, new_len: usize, value: limb) {
        if new_len > self.len() {
            let first: usize = self.len();
            self.data[first..new_len].fill(value);
            self.set_len(new_len);
        } else {
            self.set_len(new_len);
        }
    }

    /// try to resize the vector, returning if the vector was resized.
    pub fn try_resize(&mut self, new_len: usize, value: limb) -> bool {
        if new_len > self.capacity() {
            false
        } else {
            self.resize_unchecked(new_len, value);
            true
        }
    }

    /// check if any limbs are non-zero after the given index.
    /// this needs to be done in reverse order, since the index
    /// is relative to the most significant limbs.
    pub fn nonzero(&self, mut index: usize) -> bool {
        while index < self.len() {
            if *self.rindex(index) != 0 {
                return true;
            }
            index += 1;
        }
        false
    }

    /// normalize the big integer, so most-significant zero limbs are removed.
    pub fn normalize(&mut self) {
        while self.len() > 0 && *self.rindex(0) == 0 {
            self.length -= 1;
        }
    }
}

pub fn empty_hi64(truncated: &mut bool) -> u64 {
    *truncated = false;
    0
}

/// `uint64_hi64(uint64_t r0, bool &truncated)`.
pub fn uint64_hi64_1(r0: u64, truncated: &mut bool) -> u64 {
    *truncated = false;
    let shl: i32 = leading_zeroes(r0);
    r0.wrapping_shl(shl as u32)
}

/// `uint64_hi64(uint64_t r0, uint64_t r1, bool &truncated)`.
pub fn uint64_hi64_2(r0: u64, r1: u64, truncated: &mut bool) -> u64 {
    let shl: i32 = leading_zeroes(r0);
    if shl == 0 {
        *truncated = r1 != 0;
        r0
    } else {
        let shr: i32 = 64 - shl;
        *truncated = r1.wrapping_shl(shl as u32) != 0;
        r0.wrapping_shl(shl as u32) | r1.wrapping_shr(shr as u32)
    }
}

/// `uint32_hi64(uint32_t r0, bool &truncated)`.
pub fn uint32_hi64_1(r0: u32, truncated: &mut bool) -> u64 {
    uint64_hi64_1(r0 as u64, truncated)
}

/// `uint32_hi64(uint32_t r0, uint32_t r1, bool &truncated)`.
pub fn uint32_hi64_2(r0: u32, r1: u32, truncated: &mut bool) -> u64 {
    let x0: u64 = r0 as u64;
    let x1: u64 = r1 as u64;
    uint64_hi64_1((x0 << 32) | x1, truncated)
}

/// `uint32_hi64(uint32_t r0, uint32_t r1, uint32_t r2, bool &truncated)`.
pub fn uint32_hi64_3(r0: u32, r1: u32, r2: u32, truncated: &mut bool) -> u64 {
    let x0: u64 = r0 as u64;
    let x1: u64 = r1 as u64;
    let x2: u64 = r2 as u64;
    uint64_hi64_2(x0, (x1 << 32) | x2, truncated)
}

/// add two small integers, checking for overflow. we want an efficient operation. for msvc, where
/// we don't have built-in intrinsics, this is still pretty fast. (`__builtin_add_overflow`.)
pub fn scalar_add(x: limb, y: limb, overflow: &mut bool) -> limb {
    let (z, o) = x.overflowing_add(y);
    *overflow = o;
    z
}

/// multiply two small integers, getting both the high and low bits. (Ramo `__uint128_t` do C++.)
pub fn scalar_mul(x: limb, y: limb, carry: &mut limb) -> limb {
    let z: u128 = (x as u128) * (y as u128) + (*carry as u128);
    *carry = (z >> limb_bits) as limb;
    z as limb
}

/// add scalar value to bigint starting from offset. used in grade school multiplication
pub fn small_add_from<const SIZE: usize>(vec: &mut StackVec<SIZE>, y: limb, start: usize) -> bool {
    let mut index: usize = start;
    let mut carry: limb = y;
    let mut overflow: bool = false;
    while carry != 0 && index < vec.len() {
        vec[index] = scalar_add(vec[index], carry, &mut overflow);
        carry = overflow as limb;
        index += 1;
    }
    if carry != 0 {
        if !vec.try_push(carry) {
            return false;
        }
    }
    true
}

/// add scalar value to bigint.
pub fn small_add<const SIZE: usize>(vec: &mut StackVec<SIZE>, y: limb) -> bool {
    small_add_from(vec, y, 0)
}

/// multiply bigint by scalar value.
pub fn small_mul<const SIZE: usize>(vec: &mut StackVec<SIZE>, y: limb) -> bool {
    let mut carry: limb = 0;
    for index in 0..vec.len() {
        vec[index] = scalar_mul(vec[index], y, &mut carry);
    }
    if carry != 0 {
        if !vec.try_push(carry) {
            return false;
        }
    }
    true
}

/// add bigint to bigint starting from index. used in grade school multiplication
pub fn large_add_from<const SIZE: usize>(x: &mut StackVec<SIZE>, y: limb_span, start: usize) -> bool {
    // the effective x buffer is from `xstart..x.len()`, so exit early
    // if we can't get that current range.
    if x.len() < start || y.len() > x.len() - start {
        if !x.try_resize(y.len() + start, 0) {
            return false;
        }
    }

    let mut carry: bool = false;
    for index in 0..y.len() {
        let mut xi: limb = x[index + start];
        let yi: limb = y[index];
        let mut c1: bool = false;
        let mut c2: bool = false;
        xi = scalar_add(xi, yi, &mut c1);
        if carry {
            xi = scalar_add(xi, 1, &mut c2);
        }
        x[index + start] = xi;
        carry = c1 | c2;
    }

    // handle overflow
    if carry {
        if !small_add_from(x, 1, y.len() + start) {
            return false;
        }
    }
    true
}

/// add bigint to bigint. (`large_add_from(x, y)`, com `start = 0`.)
pub fn large_add<const SIZE: usize>(x: &mut StackVec<SIZE>, y: limb_span) -> bool {
    large_add_from(x, y, 0)
}

/// grade-school multiplication algorithm
pub fn long_mul<const SIZE: usize>(x: &mut StackVec<SIZE>, y: limb_span) -> bool {
    let z: StackVec<SIZE> = StackVec::from_span(span::new(&x.data[..], x.len()));
    let zs: limb_span = span::new(&z.data[..], z.len());

    if y.len() != 0 {
        let y0: limb = y[0];
        if !small_mul(x, y0) {
            return false;
        }
        for index in 1..y.len() {
            let yi: limb = y[index];
            let mut zi: StackVec<SIZE> = StackVec::new();
            if yi != 0 {
                // re-use the same buffer throughout
                zi.set_len(0);
                if !zi.try_extend(zs) {
                    return false;
                }
                if !small_mul(&mut zi, yi) {
                    return false;
                }
                let zis: limb_span = span::new(&zi.data[..], zi.len());
                if !large_add_from(x, zis, index) {
                    return false;
                }
            }
        }
    }

    x.normalize();
    true
}

/// grade-school multiplication algorithm
pub fn large_mul<const SIZE: usize>(x: &mut StackVec<SIZE>, y: limb_span) -> bool {
    if y.len() == 1 {
        if !small_mul(x, y[0]) {
            return false;
        }
    } else if !long_mul(x, y) {
        return false;
    }
    true
}

// `pow5_tables<>`.
pub const large_step: u32 = 135;

pub static small_power_of_5: [u64; 28] = [
    1,
    5,
    25,
    125,
    625,
    3125,
    15625,
    78125,
    390625,
    1953125,
    9765625,
    48828125,
    244140625,
    1220703125,
    6103515625,
    30517578125,
    152587890625,
    762939453125,
    3814697265625,
    19073486328125,
    95367431640625,
    476837158203125,
    2384185791015625,
    11920928955078125,
    59604644775390625,
    298023223876953125,
    1490116119384765625,
    7450580596923828125,
];

pub static large_power_of_5: [limb; 5] = [
    1414648277510068013,
    9180637584431281687,
    4539964771860779200,
    10482974169319127550,
    198276706040285095,
];

/// big integer type. implements a small subset of big integer arithmetic, using simple algorithms
/// since asymptotically faster algorithms are slower for a small number of limbs. all operations
/// assume the big-integer is normalized.
pub struct bigint {
    /// storage of the limbs, in little-endian order.
    pub vec: StackVec<bigint_limbs>,
}

impl bigint {
    /// `bigint()`.
    pub fn new() -> Self {
        bigint { vec: StackVec::new() }
    }

    /// `bigint(uint64_t value)`.
    pub fn from_u64(value: u64) -> Self {
        let mut b = bigint::new();
        b.vec.push_unchecked(value);
        b.vec.normalize();
        b
    }

    /// get the high 64 bits from the vector, and if bits were truncated. this is to get the
    /// significant digits for the float.
    pub fn hi64(&self, truncated: &mut bool) -> u64 {
        if self.vec.len() == 0 {
            empty_hi64(truncated)
        } else if self.vec.len() == 1 {
            uint64_hi64_1(*self.vec.rindex(0), truncated)
        } else {
            let result: u64 = uint64_hi64_2(*self.vec.rindex(0), *self.vec.rindex(1), truncated);
            *truncated |= self.vec.nonzero(2);
            result
        }
    }

    /// compare two big integers, returning the large value. assumes both are normalized. if the
    /// return value is negative, other is larger, if the return value is positive, this is larger,
    /// otherwise they are equal. the limbs are stored in little-endian order, so we must compare
    /// the limbs in ever order.
    pub fn compare(&self, other: &bigint) -> i32 {
        if self.vec.len() > other.vec.len() {
            1
        } else if self.vec.len() < other.vec.len() {
            -1
        } else {
            let mut index: usize = self.vec.len();
            while index > 0 {
                let xi: limb = self.vec[index - 1];
                let yi: limb = other.vec[index - 1];
                if xi > yi {
                    return 1;
                } else if xi < yi {
                    return -1;
                }
                index -= 1;
            }
            0
        }
    }

    /// shift left each limb n bits, carrying over to the new limb
    /// returns true if we were able to shift all the digits.
    pub fn shl_bits(&mut self, n: usize) -> bool {
        // Internally, for each item, we shift left by n, and add the previous
        // right shifted limb-bits.
        // For example, we transform (for u8) shifted left 2, to:
        //      b10100100 b01000010
        //      b10 b10010001 b00001000
        debug_assert!(n != 0);
        debug_assert!(n < std::mem::size_of::<limb>() * 8);

        let shl: usize = n;
        let shr: usize = limb_bits - shl;
        let mut prev: limb = 0;
        for index in 0..self.vec.len() {
            let xi: limb = self.vec[index];
            self.vec[index] = (xi << shl) | (prev >> shr);
            prev = xi;
        }

        let carry: limb = prev >> shr;
        if carry != 0 {
            return self.vec.try_push(carry);
        }
        true
    }

    /// move the limbs left by `n` limbs.
    pub fn shl_limbs(&mut self, n: usize) -> bool {
        debug_assert!(n != 0);
        if n + self.vec.len() > self.vec.capacity() {
            false
        } else if !self.vec.is_empty() {
            // move limbs
            let len: usize = self.vec.len();
            self.vec.data.copy_within(0..len, n);
            // fill in empty limbs
            self.vec.data[0..n].fill(0);
            self.vec.set_len(n + len);
            true
        } else {
            true
        }
    }

    /// move the limbs left by `n` bits.
    pub fn shl(&mut self, n: usize) -> bool {
        let rem: usize = n % limb_bits;
        let div: usize = n / limb_bits;
        if rem != 0 {
            if !self.shl_bits(rem) {
                return false;
            }
        }
        if div != 0 {
            if !self.shl_limbs(div) {
                return false;
            }
        }
        true
    }

    /// get the number of leading zeros in the bigint.
    pub fn ctlz(&self) -> i32 {
        if self.vec.is_empty() {
            0
        } else {
            leading_zeroes(*self.vec.rindex(0))
        }
    }

    /// get the number of bits in the bigint.
    pub fn bit_length(&self) -> i32 {
        let lz: i32 = self.ctlz();
        (limb_bits * self.vec.len()) as i32 - lz
    }

    pub fn mul(&mut self, y: limb) -> bool {
        small_mul(&mut self.vec, y)
    }

    pub fn add(&mut self, y: limb) -> bool {
        small_add(&mut self.vec, y)
    }

    /// multiply as if by 2 raised to a power.
    pub fn pow2(&mut self, exp: u32) -> bool {
        self.shl(exp as usize)
    }

    /// multiply as if by 5 raised to a power.
    pub fn pow5(&mut self, mut exp: u32) -> bool {
        // multiply by a power of 5
        let large_length: usize = large_power_of_5.len();
        let large: limb_span = span::new(&large_power_of_5[..], large_length);
        while exp >= large_step {
            if !large_mul(&mut self.vec, large) {
                return false;
            }
            exp -= large_step;
        }
        let small_step: u32 = 27;
        let max_native: limb = 7450580596923828125;
        while exp >= small_step {
            if !small_mul(&mut self.vec, max_native) {
                return false;
            }
            exp -= small_step;
        }
        if exp != 0 {
            if !small_mul(&mut self.vec, small_power_of_5[exp as usize]) {
                return false;
            }
        }

        true
    }

    /// multiply as if by 10 raised to a power.
    pub fn pow10(&mut self, exp: u32) -> bool {
        if !self.pow5(exp) {
            return false;
        }
        self.pow2(exp)
    }
}
