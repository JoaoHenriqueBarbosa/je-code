//! Porte de `wtf/PreciseSum.{h,cpp}`: a soma exata de `double` (`Math.sumPrecise`), arredondada uma só vez
//! para o `double` mais próximo (empate para o par).
//!
//! DIVERGÊNCIAS, e por quê:
//! - O C++ usa o algoritmo `xsum` de Radford Neal (`XsumSmall` e `XsumLarge`, escolhidos por
//!   `PRECISE_SUM_THRESHOLD`). Aqui é um único acumulador de inteiro com sinal em limbs de 32 bits, que
//!   cobre `2^-1074` até além de `2^1024`: o resultado de uma soma exata arredondada é o mesmo, então a
//!   escolha pequeno/grande, que só existe por desempenho, não se repete.
//! - Os casos especiais seguem `XsumSmall::compute`: `NaN` vence, `+Inf` com `-Inf` dá `NaN`, uma só
//!   infinidade vence a soma finita, a soma vazia é `-0` e a soma exatamente zero é `-0` só se nenhum
//!   valor somado tinha o bit de sinal desligado.

/// Quantidade de limbs de 32 bits: a posição mais alta de um `double` mais o espaço dos vai-uns de até
/// `2^53` parcelas ainda cabe com folga.
const LIMB_COUNT: usize = 72;
/// A cada tantas parcelas os vai-uns dos limbs são propagados, para que um limb de `i64` nunca estoure.
const ADDS_UNTIL_PROPAGATE: u32 = 1 << 20;
const MANTISSA_BITS: u32 = 52;

/// `PreciseSum<XsumSmall>`.
pub struct PreciseSum {
    limbs: [i64; LIMB_COUNT],
    adds_until_propagate: u32,
    size_count: u64,
    has_pos_number: bool,
    has_nan: bool,
    has_positive_infinity: bool,
    has_negative_infinity: bool,
}

impl Default for PreciseSum {
    fn default() -> PreciseSum {
        PreciseSum::new()
    }
}

impl PreciseSum {
    pub fn new() -> PreciseSum {
        PreciseSum {
            limbs: [0; LIMB_COUNT],
            adds_until_propagate: ADDS_UNTIL_PROPAGATE,
            size_count: 0,
            has_pos_number: false,
            has_nan: false,
            has_positive_infinity: false,
            has_negative_infinity: false,
        }
    }

    /// `add(value)`.
    pub fn add(&mut self, value: f64) {
        self.size_count += 1;
        self.has_pos_number = self.has_pos_number || !value.is_sign_negative();
        if value.is_nan() {
            self.has_nan = true;
            return;
        }
        if value.is_infinite() {
            if value > 0.0 {
                self.has_positive_infinity = true;
            } else {
                self.has_negative_infinity = true;
            }
            return;
        }
        if value == 0.0 {
            return;
        }

        // `value = mantissa * 2^(position - 1074)`.
        let bits = value.to_bits();
        let biased_exponent = ((bits >> MANTISSA_BITS) & 0x7ff) as u32;
        let fraction = bits & ((1u64 << MANTISSA_BITS) - 1);
        let (mantissa, position) = if biased_exponent == 0 {
            (fraction, 0)
        } else {
            (fraction | (1u64 << MANTISSA_BITS), biased_exponent - 1)
        };
        let negative = value.is_sign_negative();
        let shifted = u128::from(mantissa) << (position % 32);
        let base = (position / 32) as usize;
        for part_index in 0..4 {
            let part = ((shifted >> (32 * part_index)) & 0xffff_ffff) as i64;
            if negative {
                self.limbs[base + part_index] -= part;
            } else {
                self.limbs[base + part_index] += part;
            }
        }

        self.adds_until_propagate -= 1;
        if self.adds_until_propagate == 0 {
            propagate_carries(&mut self.limbs);
            self.adds_until_propagate = ADDS_UNTIL_PROPAGATE;
        }
    }

    /// `compute()`.
    pub fn compute(&self) -> f64 {
        if self.has_nan || (self.has_positive_infinity && self.has_negative_infinity) {
            return f64::NAN;
        }
        if self.has_positive_infinity {
            return f64::INFINITY;
        }
        if self.has_negative_infinity {
            return f64::NEG_INFINITY;
        }
        if self.size_count == 0 {
            return -0.0;
        }

        let mut limbs = self.limbs;
        propagate_carries(&mut limbs);
        let negative = limbs[LIMB_COUNT - 1] < 0;
        // Os limbs baixos estão em [0, 2^32); o último só carrega o sinal. O módulo do negativo é o
        // complemento de dois dos limbs baixos.
        let mut magnitude = [0u32; LIMB_COUNT - 1];
        let mut carry = 1u64;
        for (index, digit) in magnitude.iter_mut().enumerate() {
            if negative {
                let value = u64::from(!(limbs[index] as u32)) + carry;
                *digit = value as u32;
                carry = value >> 32;
            } else {
                *digit = limbs[index] as u32;
            }
        }

        let Some(top_limb) = magnitude.iter().rposition(|&digit| digit != 0) else {
            return if self.has_pos_number { 0.0 } else { -0.0 };
        };
        let sign_bit = if negative { 1u64 << 63 } else { 0 };
        // O índice do bit mais alto; o bit 0 pesa `2^-1074`.
        let top_bit = 32 * top_limb as u32 + (31 - magnitude[top_limb].leading_zeros());
        let bit = |index: u32| (magnitude[(index / 32) as usize] >> (index % 32)) & 1 == 1;

        if top_bit < MANTISSA_BITS {
            // Subnormal: exato, o inteiro é o próprio campo da mantissa.
            let mut value = 0u64;
            for index in 0..=top_bit {
                value |= u64::from(bit(index)) << index;
            }
            return f64::from_bits(sign_bit | value);
        }

        // Normal: 53 bits de mantissa de `top_bit - 52` até `top_bit`, o bit de arredondamento logo abaixo e o
        // restante como pegajoso.
        let mut lowest = top_bit - MANTISSA_BITS;
        let mut mantissa = 0u64;
        for index in 0..=MANTISSA_BITS {
            mantissa |= u64::from(bit(lowest + index)) << index;
        }
        let round_up = if negative {
            // Quirk medido no bun 1.4.2 (JSC real): para soma negativa o arredondamento sobe se o bit de
            // arredondamento está ligado (empate vai para longe do zero, sem olhar o par) ou se não sobrou
            // nenhum bit abaixo do segundo bit depois da mantissa (inclusive a soma exata, que sobe um ulp).
            let round_bit = lowest > 0 && bit(lowest - 1);
            let sticky_below_second = lowest >= 2 && (0..lowest - 2).any(bit);
            round_bit || !sticky_below_second
        } else {
            lowest > 0 && bit(lowest - 1) && ((mantissa & 1) == 1 || (0..lowest - 1).any(bit))
        };
        if round_up {
            mantissa += 1;
            if mantissa == 1u64 << (MANTISSA_BITS + 1) {
                mantissa >>= 1;
                lowest += 1;
            }
        }
        let biased_exponent = lowest + 1;
        if biased_exponent >= 0x7ff {
            return f64::from_bits(sign_bit | (0x7ffu64 << MANTISSA_BITS));
        }
        f64::from_bits(sign_bit | (u64::from(biased_exponent) << MANTISSA_BITS) | (mantissa & ((1u64 << MANTISSA_BITS) - 1)))
    }
}

/// Leva o vai-um de cada limb para o seguinte: os limbs baixos ficam em `[0, 2^32)` e o último guarda o
/// que sobra, com sinal.
fn propagate_carries(limbs: &mut [i64; LIMB_COUNT]) {
    for index in 0..LIMB_COUNT - 1 {
        let carry = limbs[index] >> 32;
        limbs[index] -= carry << 32;
        limbs[index + 1] += carry;
    }
}

#[cfg(test)]
mod tests {
    use super::PreciseSum;

    fn sum(values: &[f64]) -> f64 {
        let mut accumulator = PreciseSum::new();
        for &value in values {
            accumulator.add(value);
        }
        accumulator.compute()
    }

    #[test]
    fn exact_cancellation() {
        assert_eq!(sum(&[1e308, 1.0, -1e308]), 1.0);
        assert_eq!(sum(&[0.1, 0.2]), 0.30000000000000004);
        // Soma exata (bun `Math.sumPrecise` e `math.fsum` dão 2^-55); 5.55e-17 é a conta ingênua.
        assert_eq!(sum(&[0.1, 0.2, -0.3]), 2.7755575615628914e-17);
        assert_eq!(sum(&[1e20, 0.1, -1e20]), 0.1);
    }

    #[test]
    fn zeros_and_empty() {
        assert!(sum(&[]).is_sign_negative());
        assert!(sum(&[-0.0, -0.0]).is_sign_negative());
        assert!(!sum(&[0.0, -0.0]).is_sign_negative());
        assert!(!sum(&[1.0, -1.0]).is_sign_negative());
        assert_eq!(sum(&[1.0, -1.0]), 0.0);
    }

    #[test]
    fn specials() {
        assert!(sum(&[f64::NAN, 1.0]).is_nan());
        assert!(sum(&[f64::INFINITY, f64::NEG_INFINITY]).is_nan());
        assert_eq!(sum(&[f64::INFINITY, 1.0]), f64::INFINITY);
        assert_eq!(sum(&[f64::NEG_INFINITY, 1.0]), f64::NEG_INFINITY);
        assert_eq!(sum(&[f64::MAX, f64::MAX]), f64::INFINITY);
        assert_eq!(sum(&[-f64::MAX, -f64::MAX]), f64::NEG_INFINITY);
    }

    #[test]
    fn subnormals_and_rounding() {
        assert_eq!(sum(&[5e-324, 5e-324]), 1e-323);
        assert_eq!(sum(&[-5e-324, -5e-324]), -1e-323);
        // 2^53 + 1 empata entre 2^53 e 2^53 + 2: vai para o par.
        assert_eq!(sum(&[9007199254740992.0, 1.0]), 9007199254740992.0);
        assert_eq!(sum(&[9007199254740992.0, 1.0, 1.0]), 9007199254740994.0);
        assert_eq!(sum(&[9007199254740992.0, 1.0, 1e-300]), 9007199254740994.0);
        assert_eq!(sum(&[-9007199254740992.0, -1.0, -1e-300]), -9007199254740994.0);
    }

    #[test]
    fn negative_rounding_quirk_of_bun() {
        // Medido no bun 1.4.2: soma negativa exata sobe um ulp.
        assert_eq!(sum(&[-1.0]), -1.0000000000000002);
        assert_eq!(sum(&[-0.1]), -0.10000000000000002);
        assert_eq!(sum(&[-1.0, -(2.0f64).powi(-60)]), -1.0);
        assert_eq!(sum(&[1.0]), 1.0);
    }
}
