// Tradução de WTF/wtf/dtoa/ieee.h (double-conversion, V8).
//
// `Double` e `Single` guardam só os bits; `DiyFp` vem de `crate::wtf::dtoa::diy_fp`. As saídas por
// ponteiro de `NormalizedBoundaries` viram uma tupla `(m_minus, m_plus)`.

use crate::wtf::dtoa::diy_fp::DiyFp;
use crate::wtf::dtoa::utils::uint64_2part_c;

/// `double_to_uint64`: assume que double e uint64_t têm a mesma endianness.
pub fn double_to_uint64(d: f64) -> u64 {
    d.to_bits()
}

/// `float_to_uint32`.
pub fn float_to_uint32(f: f32) -> u32 {
    f.to_bits()
}

/// Auxiliares para doubles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Double {
    d64: u64,
}

impl Double {
    pub const K_SIGN_MASK: u64 = uint64_2part_c(0x80000000, 0x00000000);
    pub const K_EXPONENT_MASK: u64 = uint64_2part_c(0x7FF00000, 0x00000000);
    pub const K_SIGNIFICAND_MASK: u64 = uint64_2part_c(0x000FFFFF, 0xFFFFFFFF);
    pub const K_HIDDEN_BIT: u64 = uint64_2part_c(0x00100000, 0x00000000);
    /// Exclui o bit escondido.
    pub const K_PHYSICAL_SIGNIFICAND_SIZE: i32 = 52;
    pub const K_SIGNIFICAND_SIZE: i32 = 53;

    const K_EXPONENT_BIAS: i32 = 0x3FF + Self::K_PHYSICAL_SIGNIFICAND_SIZE;
    const K_DENORMAL_EXPONENT: i32 = -Self::K_EXPONENT_BIAS + 1;
    const K_MAX_EXPONENT: i32 = 0x7FF - Self::K_EXPONENT_BIAS;
    const K_INFINITY: u64 = uint64_2part_c(0x7FF00000, 0x00000000);
    const K_NAN: u64 = uint64_2part_c(0x7FF80000, 0x00000000);

    /// `Double()`: zero.
    pub fn zero() -> Double {
        Double { d64: 0 }
    }

    /// `explicit Double(double d)`.
    pub fn from_f64(d: f64) -> Double {
        Double { d64: double_to_uint64(d) }
    }

    /// `explicit Double(uint64_t d64)`.
    pub fn from_u64(d64: u64) -> Double {
        Double { d64 }
    }

    /// `explicit Double(DiyFp diy_fp)`.
    pub fn from_diy_fp(diy_fp: DiyFp) -> Double {
        Double { d64: Self::diy_fp_to_uint64(diy_fp) }
    }

    /// O valor codificado precisa ser maior ou igual a +0.0 e não pode ser especial (infinito ou
    /// NaN).
    pub fn as_diy_fp(&self) -> DiyFp {
        debug_assert!(self.sign() > 0);
        debug_assert!(!self.is_special());
        DiyFp::new(self.significand(), self.exponent())
    }

    /// O valor codificado precisa ser estritamente maior que 0.
    pub fn as_normalized_diy_fp(&self) -> DiyFp {
        debug_assert!(self.value() > 0.0);
        let mut f = self.significand();
        let mut e = self.exponent();

        // O double atual pode ser um denormal.
        while (f & Self::K_HIDDEN_BIT) == 0 {
            f <<= 1;
            e -= 1;
        }
        // Faz os deslocamentos finais de uma vez.
        f <<= DiyFp::K_SIGNIFICAND_SIZE - Self::K_SIGNIFICAND_SIZE;
        e -= DiyFp::K_SIGNIFICAND_SIZE - Self::K_SIGNIFICAND_SIZE;
        DiyFp::new(f, e)
    }

    /// Os bits do double como uint64.
    pub fn as_uint64(&self) -> u64 {
        self.d64
    }

    /// O próximo double maior. Devolve +infinito na entrada +infinito.
    pub fn next_double(&self) -> f64 {
        if self.d64 == Self::K_INFINITY {
            return Double::from_u64(Self::K_INFINITY).value();
        }
        if self.sign() < 0 && self.significand() == 0 {
            // -0.0
            return 0.0;
        }
        if self.sign() < 0 {
            Double::from_u64(self.d64 - 1).value()
        } else {
            Double::from_u64(self.d64 + 1).value()
        }
    }

    pub fn previous_double(&self) -> f64 {
        if self.d64 == (Self::K_INFINITY | Self::K_SIGN_MASK) {
            return -Double::infinity();
        }
        if self.sign() < 0 {
            Double::from_u64(self.d64 + 1).value()
        } else {
            if self.significand() == 0 {
                return -0.0;
            }
            Double::from_u64(self.d64 - 1).value()
        }
    }

    pub fn exponent(&self) -> i32 {
        if self.is_denormal() {
            return Self::K_DENORMAL_EXPONENT;
        }

        let d64 = self.as_uint64();
        let biased_e = ((d64 & Self::K_EXPONENT_MASK) >> Self::K_PHYSICAL_SIGNIFICAND_SIZE) as i32;
        biased_e - Self::K_EXPONENT_BIAS
    }

    pub fn significand(&self) -> u64 {
        let d64 = self.as_uint64();
        let significand = d64 & Self::K_SIGNIFICAND_MASK;
        if !self.is_denormal() {
            significand + Self::K_HIDDEN_BIT
        } else {
            significand
        }
    }

    /// Verdadeiro se o double é um denormal.
    pub fn is_denormal(&self) -> bool {
        let d64 = self.as_uint64();
        (d64 & Self::K_EXPONENT_MASK) == 0
    }

    /// Denormais não são especiais: só infinito e NaN são.
    pub fn is_special(&self) -> bool {
        let d64 = self.as_uint64();
        (d64 & Self::K_EXPONENT_MASK) == Self::K_EXPONENT_MASK
    }

    pub fn is_nan(&self) -> bool {
        let d64 = self.as_uint64();
        ((d64 & Self::K_EXPONENT_MASK) == Self::K_EXPONENT_MASK)
            && ((d64 & Self::K_SIGNIFICAND_MASK) != 0)
    }

    /// `IsInfinite`.
    pub fn is_infinity(&self) -> bool {
        let d64 = self.as_uint64();
        ((d64 & Self::K_EXPONENT_MASK) == Self::K_EXPONENT_MASK)
            && ((d64 & Self::K_SIGNIFICAND_MASK) == 0)
    }

    pub fn sign(&self) -> i32 {
        let d64 = self.as_uint64();
        if (d64 & Self::K_SIGN_MASK) == 0 { 1 } else { -1 }
    }

    /// Pré-condição: o valor codificado precisa ser maior ou igual a +0.0.
    pub fn upper_boundary(&self) -> DiyFp {
        debug_assert!(self.sign() > 0);
        DiyFp::new(self.significand() * 2 + 1, self.exponent() - 1)
    }

    /// Calcula os dois limites de this, devolvidos como `(m_minus, m_plus)`. O limite maior
    /// (m_plus) é normalizado; o menor tem o mesmo expoente que m_plus. Pré-condição: o valor
    /// codificado precisa ser maior que 0.
    pub fn normalized_boundaries(&self) -> (DiyFp, DiyFp) {
        debug_assert!(self.value() > 0.0);
        let v = self.as_diy_fp();
        let m_plus = DiyFp::normalize_value(DiyFp::new((v.f() << 1) + 1, v.e() - 1));
        let m_minus = if self.lower_boundary_is_closer() {
            DiyFp::new((v.f() << 2) - 1, v.e() - 2)
        } else {
            DiyFp::new((v.f() << 1) - 1, v.e() - 1)
        };
        // m_minus.set_f(m_minus.f() << (m_minus.e() - m_plus.e())); m_minus.set_e(m_plus.e());
        let m_minus = DiyFp::new(m_minus.f() << (m_minus.e() - m_plus.e()), m_plus.e());
        (m_minus, m_plus)
    }

    pub fn lower_boundary_is_closer(&self) -> bool {
        // O limite é mais próximo se o significando tem a forma f == 2^p-1: o limite inferior
        // fica mais perto. Pense em v = 1000e10 e v- = 9999e9: o limite (== (v - v-)/2) não está
        // a uma distância de 1e9, e sim de 1e8. A única exceção é o menor normal: o maior
        // denormal está à mesma distância que o seu sucessor. Denormais têm o mesmo expoente
        // que os menores normais.
        let physical_significand_is_zero = (self.as_uint64() & Self::K_SIGNIFICAND_MASK) == 0;
        physical_significand_is_zero && (self.exponent() != Self::K_DENORMAL_EXPONENT)
    }

    pub fn value(&self) -> f64 {
        f64::from_bits(self.d64)
    }

    /// Tamanho do significando para uma dada ordem de grandeza. Se v = f*2^e com
    /// 2^p-1 <= f <= 2^p, então p+e é a ordem de grandeza de v. Devolve quantos dígitos binários
    /// significativos v terá codificado num double; quase sempre é `K_SIGNIFICAND_SIZE`, e as
    /// exceções são os denormais, cujo tamanho efetivo é menor.
    pub fn significand_size_for_order_of_magnitude(order: i32) -> i32 {
        if order >= (Self::K_DENORMAL_EXPONENT + Self::K_SIGNIFICAND_SIZE) {
            return Self::K_SIGNIFICAND_SIZE;
        }
        if order <= Self::K_DENORMAL_EXPONENT {
            return 0;
        }
        order - Self::K_DENORMAL_EXPONENT
    }

    pub fn infinity() -> f64 {
        Double::from_u64(Self::K_INFINITY).value()
    }

    pub fn nan() -> f64 {
        Double::from_u64(Self::K_NAN).value()
    }

    fn diy_fp_to_uint64(diy_fp: DiyFp) -> u64 {
        let mut significand = diy_fp.f();
        let mut exponent = diy_fp.e();
        while significand > Self::K_HIDDEN_BIT + Self::K_SIGNIFICAND_MASK {
            significand >>= 1;
            exponent += 1;
        }
        if exponent >= Self::K_MAX_EXPONENT {
            return Self::K_INFINITY;
        }
        if exponent < Self::K_DENORMAL_EXPONENT {
            return 0;
        }
        while exponent > Self::K_DENORMAL_EXPONENT && (significand & Self::K_HIDDEN_BIT) == 0 {
            significand <<= 1;
            exponent -= 1;
        }
        let biased_exponent: u64 = if exponent == Self::K_DENORMAL_EXPONENT
            && (significand & Self::K_HIDDEN_BIT) == 0
        {
            0
        } else {
            (exponent + Self::K_EXPONENT_BIAS) as u64
        };
        (significand & Self::K_SIGNIFICAND_MASK) | (biased_exponent << Self::K_PHYSICAL_SIGNIFICAND_SIZE)
    }
}

/// Auxiliares para floats de 32 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Single {
    d32: u32,
}

impl Single {
    pub const K_SIGN_MASK: u32 = 0x80000000;
    pub const K_EXPONENT_MASK: u32 = 0x7F800000;
    pub const K_SIGNIFICAND_MASK: u32 = 0x007FFFFF;
    pub const K_HIDDEN_BIT: u32 = 0x00800000;
    /// Exclui o bit escondido.
    pub const K_PHYSICAL_SIGNIFICAND_SIZE: i32 = 23;
    pub const K_SIGNIFICAND_SIZE: i32 = 24;

    const K_EXPONENT_BIAS: i32 = 0x7F + Self::K_PHYSICAL_SIGNIFICAND_SIZE;
    const K_DENORMAL_EXPONENT: i32 = -Self::K_EXPONENT_BIAS + 1;
    #[allow(dead_code)]
    const K_MAX_EXPONENT: i32 = 0xFF - Self::K_EXPONENT_BIAS;
    const K_INFINITY: u32 = 0x7F800000;
    const K_NAN: u32 = 0x7FC00000;

    /// `Single()`: zero.
    pub fn zero() -> Single {
        Single { d32: 0 }
    }

    /// `explicit Single(float f)`.
    pub fn from_f32(f: f32) -> Single {
        Single { d32: float_to_uint32(f) }
    }

    /// `explicit Single(uint32_t d32)`.
    pub fn from_u32(d32: u32) -> Single {
        Single { d32 }
    }

    /// O valor codificado precisa ser maior ou igual a +0.0 e não pode ser especial.
    pub fn as_diy_fp(&self) -> DiyFp {
        debug_assert!(self.sign() > 0);
        debug_assert!(!self.is_special());
        DiyFp::new(self.significand() as u64, self.exponent())
    }

    /// Os bits do single como uint32.
    pub fn as_uint32(&self) -> u32 {
        self.d32
    }

    pub fn exponent(&self) -> i32 {
        if self.is_denormal() {
            return Self::K_DENORMAL_EXPONENT;
        }

        let d32 = self.as_uint32();
        let biased_e = ((d32 & Self::K_EXPONENT_MASK) >> Self::K_PHYSICAL_SIGNIFICAND_SIZE) as i32;
        biased_e - Self::K_EXPONENT_BIAS
    }

    pub fn significand(&self) -> u32 {
        let d32 = self.as_uint32();
        let significand = d32 & Self::K_SIGNIFICAND_MASK;
        if !self.is_denormal() {
            significand + Self::K_HIDDEN_BIT
        } else {
            significand
        }
    }

    /// Verdadeiro se o single é um denormal.
    pub fn is_denormal(&self) -> bool {
        let d32 = self.as_uint32();
        (d32 & Self::K_EXPONENT_MASK) == 0
    }

    /// Denormais não são especiais: só infinito e NaN são.
    pub fn is_special(&self) -> bool {
        let d32 = self.as_uint32();
        (d32 & Self::K_EXPONENT_MASK) == Self::K_EXPONENT_MASK
    }

    pub fn is_nan(&self) -> bool {
        let d32 = self.as_uint32();
        ((d32 & Self::K_EXPONENT_MASK) == Self::K_EXPONENT_MASK)
            && ((d32 & Self::K_SIGNIFICAND_MASK) != 0)
    }

    /// `IsInfinite`.
    pub fn is_infinity(&self) -> bool {
        let d32 = self.as_uint32();
        ((d32 & Self::K_EXPONENT_MASK) == Self::K_EXPONENT_MASK)
            && ((d32 & Self::K_SIGNIFICAND_MASK) == 0)
    }

    pub fn sign(&self) -> i32 {
        let d32 = self.as_uint32();
        if (d32 & Self::K_SIGN_MASK) == 0 { 1 } else { -1 }
    }

    /// Calcula os dois limites de this, devolvidos como `(m_minus, m_plus)`. O limite maior
    /// (m_plus) é normalizado; o menor tem o mesmo expoente que m_plus. Pré-condição: o valor
    /// codificado precisa ser maior que 0.
    pub fn normalized_boundaries(&self) -> (DiyFp, DiyFp) {
        debug_assert!(self.value() > 0.0);
        let v = self.as_diy_fp();
        let m_plus = DiyFp::normalize_value(DiyFp::new((v.f() << 1) + 1, v.e() - 1));
        let m_minus = if self.lower_boundary_is_closer() {
            DiyFp::new((v.f() << 2) - 1, v.e() - 2)
        } else {
            DiyFp::new((v.f() << 1) - 1, v.e() - 1)
        };
        // m_minus.set_f(m_minus.f() << (m_minus.e() - m_plus.e())); m_minus.set_e(m_plus.e());
        let m_minus = DiyFp::new(m_minus.f() << (m_minus.e() - m_plus.e()), m_plus.e());
        (m_minus, m_plus)
    }

    /// Pré-condição: o valor codificado precisa ser maior ou igual a +0.0.
    pub fn upper_boundary(&self) -> DiyFp {
        debug_assert!(self.sign() > 0);
        DiyFp::new(self.significand() as u64 * 2 + 1, self.exponent() - 1)
    }

    pub fn lower_boundary_is_closer(&self) -> bool {
        // Mesma explicação de `Double::lower_boundary_is_closer`.
        let physical_significand_is_zero = (self.as_uint32() & Self::K_SIGNIFICAND_MASK) == 0;
        physical_significand_is_zero && (self.exponent() != Self::K_DENORMAL_EXPONENT)
    }

    pub fn value(&self) -> f32 {
        f32::from_bits(self.d32)
    }

    pub fn infinity() -> f32 {
        Single::from_u32(Self::K_INFINITY).value()
    }

    pub fn nan() -> f32 {
        Single::from_u32(Self::K_NAN).value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_one_bits() {
        let d = Double::from_f64(1.0);
        assert_eq!(d.as_uint64(), 0x3FF0_0000_0000_0000);
        assert_eq!(d.exponent(), -52);
        assert_eq!(d.significand(), 1u64 << 52);
        assert_eq!(d.sign(), 1);
        assert!(!d.is_denormal() && !d.is_special());
    }

    #[test]
    fn double_half_and_diy_fp() {
        let d = Double::from_f64(0.5);
        assert_eq!(d.exponent(), -53);
        assert_eq!(d.significand(), 1u64 << 52);
        let fp = d.as_diy_fp();
        assert_eq!((fp.f(), fp.e()), (1u64 << 52, -53));
        let n = d.as_normalized_diy_fp();
        assert_eq!((n.f(), n.e()), (1u64 << 63, -64));
    }

    #[test]
    fn double_smallest_denormal() {
        let d = Double::from_u64(1);
        assert!(d.is_denormal());
        assert_eq!(d.exponent(), -1074);
        assert_eq!(d.significand(), 1);
        assert_eq!(d.value(), 5e-324);
        assert!(!d.lower_boundary_is_closer());
        let n = d.as_normalized_diy_fp();
        assert_eq!((n.f(), n.e()), (1u64 << 63, -1074 - 63));
    }

    #[test]
    fn double_specials() {
        assert!(Double::from_f64(f64::INFINITY).is_infinity());
        assert!(Double::from_f64(f64::INFINITY).is_special());
        assert!(!Double::from_f64(f64::INFINITY).is_nan());
        assert!(Double::from_u64(Double::K_NAN).is_nan());
        assert!(Double::nan().is_nan());
        assert_eq!(Double::infinity(), f64::INFINITY);
        assert_eq!(Double::from_f64(-1.0).sign(), -1);
    }

    #[test]
    fn double_neighbors() {
        assert_eq!(Double::from_f64(1.0).next_double(), 1.0 + f64::EPSILON);
        assert_eq!(Double::from_f64(1.0).previous_double(), 1.0 - f64::EPSILON / 2.0);
        assert_eq!(Double::from_f64(-0.0).next_double(), 0.0);
        assert_eq!(Double::from_f64(f64::INFINITY).next_double(), f64::INFINITY);
        assert_eq!(Double::from_f64(f64::NEG_INFINITY).previous_double(), f64::NEG_INFINITY);
        assert_eq!(Double::from_f64(0.0).previous_double(), 0.0);
        assert!(Double::from_f64(0.0).previous_double().is_sign_negative());
    }

    #[test]
    fn double_from_diy_fp_round_trip() {
        let one = Double::from_diy_fp(DiyFp::new(1u64 << 52, -52));
        assert_eq!(one.value(), 1.0);
        let denormal = Double::from_diy_fp(DiyFp::new(1, -1074));
        assert_eq!(denormal.as_uint64(), 1);
        assert_eq!(Double::from_diy_fp(DiyFp::new(1u64 << 52, 2000)).value(), f64::INFINITY);
    }

    #[test]
    fn double_normalized_boundaries_of_one() {
        // v = 2^52 * 2^-52; o limite inferior é mais próximo (significando físico zero).
        let (m_minus, m_plus) = Double::from_f64(1.0).normalized_boundaries();
        assert_eq!((m_plus.f(), m_plus.e()), ((1u64 << 63) + (1 << 10), -63));
        assert_eq!((m_minus.f(), m_minus.e()), ((1u64 << 63) - (1 << 9), -63));
        let up = Double::from_f64(1.0).upper_boundary();
        assert_eq!((up.f(), up.e()), ((1u64 << 53) + 1, -53));
    }

    #[test]
    fn double_significand_size_for_order() {
        assert_eq!(Double::significand_size_for_order_of_magnitude(0), 53);
        assert_eq!(Double::significand_size_for_order_of_magnitude(-1074), 0);
        assert_eq!(Double::significand_size_for_order_of_magnitude(-1073), 1);
    }

    #[test]
    fn single_bits() {
        let s = Single::from_f32(1.0);
        assert_eq!(s.as_uint32(), 0x3F80_0000);
        assert_eq!(s.exponent(), -23);
        assert_eq!(s.significand(), 1u32 << 23);
        let d = Single::from_u32(1);
        assert!(d.is_denormal());
        assert_eq!(d.exponent(), -149);
        assert!(Single::from_f32(f32::INFINITY).is_infinity());
        assert!(Single::nan().is_nan());
        assert_eq!(Single::infinity(), f32::INFINITY);
    }

    #[test]
    fn single_boundaries_of_one() {
        let (m_minus, m_plus) = Single::from_f32(1.0).normalized_boundaries();
        // v = 2^23 * 2^-23: m_plus = (2^24 + 1) normalizado em 64 bits (deslocamento 39).
        assert_eq!((m_plus.f(), m_plus.e()), (((1u64 << 24) + 1) << 39, -24 - 39));
        assert_eq!(m_minus.e(), m_plus.e());
        assert_eq!(m_minus.f(), ((1u64 << 25) - 1) << 38);
        let up = Single::from_f32(1.0).upper_boundary();
        assert_eq!((up.f(), up.e()), ((1u64 << 24) + 1, -24));
    }
}
