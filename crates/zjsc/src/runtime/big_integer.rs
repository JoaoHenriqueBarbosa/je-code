//! Tradução de `runtime/BigInteger.h`: o inteiro sem sinal de precisão arbitrária que
//! `toStringWithRadixInternal` (`number_prototype.rs`) usa para escrever a parte inteira de um `double`
//! em qualquer base.

use crate::wtf::math_extras::decompose_double;

/// `class BigInteger`: `m_values` são as palavras de 32 bits, da menos para a mais significativa, sem
/// zeros à direita (o zero é o vetor vazio).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigInteger {
    values: Vec<u32>,
}

impl BigInteger {
    /// `BigInteger(double number)`: `number` é finito, não negativo e inteiro.
    pub fn new(number: f64) -> BigInteger {
        debug_assert!(number.is_finite() && !number.is_sign_negative());
        debug_assert!(number == number.floor());

        let (sign, exponent, mut mantissa) = decompose_double(number);
        debug_assert!(!sign && exponent >= 0);

        let mut zero_bits = exponent - 52;

        if zero_bits < 0 {
            mantissa >>= -zero_bits;
            zero_bits = 0;
        }

        let mut values = Vec::new();
        while zero_bits >= 32 {
            values.push(0);
            zero_bits -= 32;
        }

        // Left align the 53 bits of the mantissa within 96 bits.
        let mut words = [mantissa as u32, (mantissa >> 32) as u32, 0u32];
        // Shift based on the remainder of the exponent.
        if zero_bits != 0 {
            let shift = zero_bits as u32;
            words[2] = words[1] >> (32 - shift);
            words[1] = (words[1] << shift) | (words[0] >> (32 - shift));
            words[0] <<= shift;
        }
        values.extend_from_slice(&words);

        let mut integer = BigInteger { values };
        integer.canonicalize();
        integer
    }

    /// Canonicalize; remove all trailing zeros.
    fn canonicalize(&mut self) {
        while self.values.last() == Some(&0) {
            self.values.pop();
        }
    }

    /// `divide(divisor)`: divide no lugar e devolve o resto.
    pub fn divide(&mut self, divisor: u32) -> u32 {
        let mut carry: u32 = 0;

        for value in self.values.iter_mut().rev() {
            let dividend = ((carry as u64) << 32) + *value as u64;

            let result = dividend / divisor as u64;
            debug_assert!(result == result as u32 as u64);
            let remainder = dividend % divisor as u64;
            debug_assert!(remainder == remainder as u32 as u64);

            *value = result as u32;
            carry = remainder as u32;
        }

        self.canonicalize();

        carry
    }

    /// `operator!`: o valor é zero.
    pub fn is_zero(&self) -> bool {
        self.values.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decimal(number: f64) -> String {
        let mut integer = BigInteger::new(number);
        let mut digits = Vec::new();
        loop {
            digits.push(b'0' + integer.divide(10) as u8);
            if integer.is_zero() {
                break;
            }
        }
        digits.reverse();
        String::from_utf8(digits).unwrap()
    }

    #[test]
    fn zero_is_empty() {
        assert!(BigInteger::new(0.0).is_zero());
    }

    #[test]
    fn small_and_large_integers() {
        assert_eq!(decimal(1.0), "1");
        assert_eq!(decimal(255.0), "255");
        assert_eq!(decimal(4294967296.0), "4294967296");
        assert_eq!(decimal(9007199254740993.0), "9007199254740992");
        assert_eq!(decimal(1e21), "1000000000000000000000");
        assert_eq!(decimal(1.7976931348623157e308).len(), 309);
    }

    #[test]
    fn divide_returns_remainder() {
        let mut integer = BigInteger::new(100.0);
        assert_eq!(integer.divide(7), 2);
        assert_eq!(integer.divide(7), 0);
        assert_eq!(integer.divide(7), 2);
        assert!(integer.is_zero());
    }
}
