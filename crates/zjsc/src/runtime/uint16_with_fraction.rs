//! Tradução de `runtime/Uint16WithFraction.h`: um `uint16_t` com fração de precisão infinita, que
//! `toStringWithRadixInternal` (`number_prototype.rs`) usa para escrever a parte fracionária de um
//! `double` em qualquer base. Ao estourar a faixa de `uint16_t` a classe satura em
//! `oneGreaterThanMaxUInt16`.

use crate::wtf::math_extras::decompose_double;

/// `oneGreaterThanMaxUInt16`.
const ONE_GREATER_THAN_MAX_UINT16: u32 = 0x10000;

/// `class Uint16WithFraction`.
///
/// `values` guarda a parte inteira em `values[0]` (na faixa de `uint16_t`, ou o valor de saturação
/// sozinho) e as palavras da fração nas seguintes: o valor é a soma de `values[i] / 2^(32 i)`. Não há
/// zeros à direita (exceto o vetor `[0]`, que é o zero).
#[derive(Clone, Debug)]
pub struct Uint16WithFraction {
    values: Vec<u32>,
    /// `m_leadingZeros`: quantas palavras iniciais de `values` são zero, para acelerar a multiplicação.
    leading_zeros: usize,
}

impl Uint16WithFraction {
    /// `Uint16WithFraction(number, divideByExponent)`: `number` é finito, positivo e não nulo.
    pub fn new(number: f64, divide_by_exponent: u16) -> Uint16WithFraction {
        debug_assert!(number != 0.0 && number.is_finite() && !number.is_sign_negative());

        // Check for values out of uint16_t range.
        if number >= ONE_GREATER_THAN_MAX_UINT16 as f64 {
            return Uint16WithFraction { values: vec![ONE_GREATER_THAN_MAX_UINT16], leading_zeros: 0 };
        }

        // Append the units to m_values.
        let integer_part = number.floor();
        let mut values = vec![integer_part as u32];

        let (sign, mut exponent, mantissa) = decompose_double(number - integer_part);
        debug_assert!(!sign && exponent < 0);
        exponent -= divide_by_exponent as i32;

        let mut zero_bits = -exponent;
        zero_bits -= 1;

        // Append the append words for to m_values.
        while zero_bits >= 32 {
            values.push(0);
            zero_bits -= 32;
        }

        // Left align the 53 bits of the mantissa within 96 bits.
        let mut words = [(mantissa >> 21) as u32, (mantissa << 11) as u32, 0u32];
        // Shift based on the remainder of the exponent.
        if zero_bits != 0 {
            let shift = zero_bits as u32;
            words[2] = words[1] << (32 - shift);
            words[1] = (words[1] >> shift) | (words[0] << (32 - shift));
            words[0] >>= shift;
        }
        values.extend_from_slice(&words);

        // Canonicalize; remove any trailing zeros.
        while values.len() > 1 && values.last() == Some(&0) {
            values.pop();
        }

        // Count the number of leading zero, this is useful in optimizing multiplies.
        let leading_zeros = values.iter().take_while(|&&value| value == 0).count();

        let result = Uint16WithFraction { values, leading_zeros };
        debug_assert!(result.check_consistency());
        result
    }

    /// `operator*=(uint16_t multiplier)`.
    pub fn multiply_assign(&mut self, multiplier: u16) {
        debug_assert!(self.check_consistency());

        // iteratate backwards over the fraction until we reach the leading zeros,
        // passing the carry from one calculation into the next.
        let mut accumulator: u64 = 0;
        for index in (self.leading_zeros..self.values.len()).rev() {
            accumulator += self.values[index] as u64 * multiplier as u64;
            self.values[index] = accumulator as u32;
            accumulator >>= 32;
        }

        if self.leading_zeros == 0 {
            // With a multiplicand and multiplier in the uint16_t range, this cannot carry
            // (even allowing for the infinity value).
            debug_assert!(accumulator == 0);
            // Check for overflow & clamp to 'infinity'.
            if self.values[0] >= ONE_GREATER_THAN_MAX_UINT16 {
                self.values.truncate(1);
                self.values[0] = ONE_GREATER_THAN_MAX_UINT16;
                self.leading_zeros = 0;
                return;
            }
        } else if accumulator != 0 {
            // Check for carry from the last multiply, if so overwrite last leading zero.
            self.leading_zeros -= 1;
            self.values[self.leading_zeros] = accumulator as u32;
            // The limited range of the multiplier should mean that even if we carry into
            // the units, we don't need to check for overflow of the uint16_t range.
            debug_assert!(self.values[0] < ONE_GREATER_THAN_MAX_UINT16);
        }

        // Multiplication by an even value may introduce trailing zeros; if so, clean them
        // up. (Keeping the value in a normalized form makes some of the comparison operations
        // more efficient).
        while self.values.len() > 1 && self.values.last() == Some(&0) {
            self.values.pop();
        }
        debug_assert!(self.check_consistency());
    }

    /// `operator<(const Uint16WithFraction& other)`.
    pub fn less_than(&self, other: &Uint16WithFraction) -> bool {
        debug_assert!(self.check_consistency());
        debug_assert!(other.check_consistency());

        // Iterate over the common lengths of arrays.
        let min_size = self.values.len().min(other.values.len());
        for index in 0..min_size {
            // If we find a value that is not equal, compare and return.
            let from_this = self.values[index];
            let from_other = other.values[index];
            if from_this != from_other {
                return from_this < from_other;
            }
        }
        // If these numbers have the same lengths, they are equal,
        // otherwise which ever number has a longer fraction in larger.
        other.values.len() > min_size
    }

    /// `floorAndSubtract()`: devolve a parte inteira, zerando-a, e deixa a fração.
    pub fn floor_and_subtract(&mut self) -> u32 {
        // 'floor' is simple the integer portion of the value.
        let floor = self.values[0];

        // If floor is non-zero,
        if floor != 0 {
            self.values[0] = 0;
            self.leading_zeros = 1;
            while self.leading_zeros < self.values.len() && self.values[self.leading_zeros] == 0 {
                self.leading_zeros += 1;
            }
        }

        floor
    }

    /// `comparePoint5()`: -1 para menor que 0.5, 0 para igual, 1 para maior.
    pub fn compare_point5(&self) -> i32 {
        debug_assert!(self.check_consistency());
        // If units != 0, this is greater than 0.5.
        if self.values[0] != 0 {
            return 1;
        }
        // If size == 1 this value is 0, hence < 0.5.
        if self.values.len() == 1 {
            return -1;
        }
        // Compare to 0.5.
        if self.values[1] > 0x8000_0000 {
            return 1;
        }
        if self.values[1] < 0x8000_0000 {
            return -1;
        }
        // Check for more words - since normalized numbers have no trailing zeros, if
        // there are more that two digits we can assume at least one more is non-zero,
        // and hence the value is > 0.5.
        if self.values.len() > 2 { 1 } else { 0 }
    }

    /// `sumGreaterThanOne(addend)`: a soma deste valor com `addend` passa de 1.
    pub fn sum_greater_than_one(&self, addend: &Uint16WithFraction) -> bool {
        debug_assert!(self.check_consistency());
        debug_assert!(addend.check_consistency());

        // First, sum the units. If the result is greater than one, return true.
        // If equal to one, return true if either number has a fractional part.
        let mut sum = self.values[0] + addend.values[0];
        if sum != 0 {
            return sum > 1 || self.values.len().max(addend.values.len()) > 1;
        }

        // We could still produce a result greater than zero if addition of the next
        // word from the fraction were to carry, leaving a result > 0.

        // Iterate over the common lengths of arrays.
        let min_size = self.values.len().min(addend.values.len());
        for index in 1..min_size {
            // Sum the next word from this & the addend.
            let from_this = self.values[index];
            let from_addend = addend.values[index];
            sum = from_this.wrapping_add(from_addend);

            // Check for overflow. If so, check whether the remaining result is non-zero,
            // or if there are any further words in the fraction.
            if sum < from_this {
                return sum != 0 || (index + 1) < self.values.len().max(addend.values.len());
            }

            // If the sum is uint32_t max, then we would carry a 1 if addition of the next
            // digits in the number were to overflow.
            if sum != 0xFFFF_FFFF {
                return false;
            }
        }
        false
    }

    /// `checkConsistency()`.
    fn check_consistency(&self) -> bool {
        // All values should have at least one value.
        !self.values.is_empty()
            // The units value must be a uint16_t, or the value is the overflow value.
            && (self.values[0] < ONE_GREATER_THAN_MAX_UINT16
                || (self.values[0] == ONE_GREATER_THAN_MAX_UINT16 && self.values.len() == 1))
            // There should be no trailing zeros (unless this value is zero!).
            && (self.values.last() != Some(&0) || self.values.len() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_five_comparison() {
        assert_eq!(Uint16WithFraction::new(0.25, 0).compare_point5(), -1);
        assert_eq!(Uint16WithFraction::new(0.5, 0).compare_point5(), 0);
        assert_eq!(Uint16WithFraction::new(0.75, 0).compare_point5(), 1);
        assert_eq!(Uint16WithFraction::new(0.5000000000000001, 0).compare_point5(), 1);
        assert_eq!(Uint16WithFraction::new(2.25, 0).compare_point5(), 1);
    }

    #[test]
    fn multiply_and_floor_walk_the_binary_fraction() {
        // 0.1 em base 2: 0.0001100110011...
        let mut fraction = Uint16WithFraction::new(0.1, 0);
        let mut bits = Vec::new();
        for _ in 0..8 {
            fraction.multiply_assign(2);
            bits.push(fraction.floor_and_subtract());
        }
        assert_eq!(bits, vec![0, 0, 0, 1, 1, 0, 0, 1]);
    }

    #[test]
    fn saturates_above_uint16() {
        let mut fraction = Uint16WithFraction::new(70000.0, 0);
        assert_eq!(fraction.compare_point5(), 1);
        fraction.multiply_assign(36);
        assert_eq!(fraction.floor_and_subtract(), 0x10000);
    }

    #[test]
    fn ordering_and_sum() {
        let quarter = Uint16WithFraction::new(0.25, 0);
        let half = Uint16WithFraction::new(0.5, 0);
        assert!(quarter.less_than(&half));
        assert!(!half.less_than(&quarter));
        assert!(!half.less_than(&half));
        // 0.5 + 0.5 == 1, e a soma só é maior que 1 com fração a mais.
        assert!(!half.sum_greater_than_one(&half));
        assert!(Uint16WithFraction::new(0.75, 0).sum_greater_than_one(&half));
        // `divideByExponent = 1` divide por 2: 0.5 vira 0.25.
        assert!(!Uint16WithFraction::new(0.5, 1).less_than(&quarter));
        assert_eq!(Uint16WithFraction::new(0.5, 1).compare_point5(), -1);
    }
}
