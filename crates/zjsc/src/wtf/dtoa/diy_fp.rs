// Tradução de WTF/wtf/dtoa/diy-fp.h e diy-fp.cc (double-conversion).

/// Implementação de "Do It Yourself Floating Point": um número de ponto flutuante com
/// significando de 64 bits sem sinal e expoente inteiro. Os números normalizados têm o bit
/// mais significativo do significando ligado. Multiplicação e subtração não normalizam o
/// resultado. Não serve para valores especiais (NaN e infinito).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiyFp {
    f: u64,
    e: i32,
}

/// `kUint64MSB`: UINT64_2PART_C(0x80000000, 00000000).
const K_UINT64_MSB: u64 = 0x8000_0000_0000_0000;

impl Default for DiyFp {
    fn default() -> Self {
        DiyFp { f: 0, e: 0 }
    }
}

impl DiyFp {
    pub const K_SIGNIFICAND_SIZE: i32 = 64;

    pub fn new(significand: u64, exponent: i32) -> DiyFp {
        DiyFp {
            f: significand,
            e: exponent,
        }
    }

    /// this = this - other.
    /// Os expoentes devem ser iguais e o significando de `self` deve ser maior que o de
    /// `other`. O resultado não é normalizado.
    pub fn subtract(&mut self, other: &DiyFp) {
        self.f = self.f.wrapping_sub(other.f);
    }

    /// Devolve a - b. Os expoentes devem ser iguais e `a` deve ser maior que `b`. O resultado
    /// não é normalizado.
    pub fn minus(a: DiyFp, b: DiyFp) -> DiyFp {
        let mut result = a;
        result.subtract(&b);
        result
    }

    /// this = this * other.
    pub fn multiply(&mut self, other: &DiyFp) {
        // "Emula" uma multiplicação de 128 bits. O resultado guarda só 64 bits: os 64 bits
        // menos significativos servem apenas para arredondar os mais significativos.
        const K_M32: u64 = 0xFFFF_FFFF;
        let a: u64 = self.f >> 32;
        let b: u64 = self.f & K_M32;
        let c: u64 = other.f >> 32;
        let d: u64 = other.f & K_M32;
        let ac: u64 = a.wrapping_mul(c);
        let bc: u64 = b.wrapping_mul(c);
        let ad: u64 = a.wrapping_mul(d);
        let bd: u64 = b.wrapping_mul(d);
        let mut tmp: u64 = (bd >> 32)
            .wrapping_add(ad & K_M32)
            .wrapping_add(bc & K_M32);
        // Somar 1 << 31 a tmp arredonda o resultado final. Os casos de meio exato sobem.
        tmp = tmp.wrapping_add(1u64 << 31);
        let result_f: u64 = ac
            .wrapping_add(ad >> 32)
            .wrapping_add(bc >> 32)
            .wrapping_add(tmp >> 32);
        self.e = self.e.wrapping_add(other.e).wrapping_add(64);
        self.f = result_f;
    }

    /// Devolve a * b.
    pub fn times(a: DiyFp, b: DiyFp) -> DiyFp {
        let mut result = a;
        result.multiply(&b);
        result
    }

    pub fn normalize(&mut self) {
        let mut significand: u64 = self.f;
        let mut exponent: i32 = self.e;

        // Este método é chamado sobretudo para normalizar fronteiras, que em geral precisam
        // de um deslocamento de 10 bits. Por isso o caso é otimizado.
        const K_10_MS_BITS: u64 = 0xFFC0_0000_0000_0000;
        while (significand & K_10_MS_BITS) == 0 {
            significand <<= 10;
            exponent = exponent.wrapping_sub(10);
        }
        while (significand & K_UINT64_MSB) == 0 {
            significand <<= 1;
            exponent = exponent.wrapping_sub(1);
        }
        self.f = significand;
        self.e = exponent;
    }

    pub fn normalize_value(a: DiyFp) -> DiyFp {
        let mut result = a;
        result.normalize();
        result
    }

    pub fn f(&self) -> u64 {
        self.f
    }

    pub fn e(&self) -> i32 {
        self.e
    }

    pub fn set_f(&mut self, new_value: u64) {
        self.f = new_value;
    }

    pub fn set_e(&mut self, new_value: i32) {
        self.e = new_value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiply_of_two_powers() {
        // 2^63 * 2^63: ac = 2^62, o resto é só o arredondamento, que não carrega.
        let a = DiyFp::new(0x8000_0000_0000_0000, 0);
        let r = DiyFp::times(a, a);
        assert_eq!(r.f(), 1u64 << 62);
        assert_eq!(r.e(), 64);
    }

    #[test]
    fn multiply_rounds_half_up() {
        // (2^32 + 1) * 2^32: ac = 1, ad = 2^32 * ... conferido a mão:
        // a=1,b=1,c=1,d=0 => ac=1, bc=1, ad=0, bd=0; tmp = 0+0+1 + 2^31; resultado = 1.
        let x = DiyFp::new((1u64 << 32) + 1, 3);
        let y = DiyFp::new(1u64 << 32, -5);
        let r = DiyFp::times(x, y);
        assert_eq!(r.f(), 1);
        assert_eq!(r.e(), 3 - 5 + 64);
    }

    #[test]
    fn normalize_small_value() {
        // f = 1: seis deslocamentos de 10 bits (bit 60), depois três de 1 bit (bit 63).
        let mut v = DiyFp::new(1, 0);
        v.normalize();
        assert_eq!(v.f(), 1u64 << 63);
        assert_eq!(v.e(), -63);
    }

    #[test]
    fn normalize_already_normalized() {
        let v = DiyFp::normalize_value(DiyFp::new(0xC000_0000_0000_0000, 7));
        assert_eq!(v.f(), 0xC000_0000_0000_0000);
        assert_eq!(v.e(), 7);
    }

    #[test]
    fn minus_and_subtract() {
        let a = DiyFp::new(10, 4);
        let b = DiyFp::new(3, 4);
        let r = DiyFp::minus(a, b);
        assert_eq!(r.f(), 7);
        assert_eq!(r.e(), 4);
        let mut c = a;
        c.subtract(&b);
        assert_eq!(c, r);
    }
}
