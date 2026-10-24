//! Porte da família exp/log/pow da libm do glibc 2.41 (`sysdeps/ieee754/dbl-64`: `e_exp.c`, `e_exp2.c`,
//! `e_log.c`, `e_log2.c`, `e_pow.c`), que são as "optimized routines" da ARM (Szabolcs Nagy). As tabelas
//! exatas estão em `data.rs`. Nenhum `unsafe`: `asuint64`/`asdouble` são `to_bits`/`from_bits`.
//!
//! Decisão sobre FMA (registrada em `wip/zjsc/wip-notes/math-audit.md`): o glibc do bun roda num x86_64 moderno,
//! onde o ifunc escolhe as variantes `e_exp-fma.c`, `e_log-fma.c`, `e_log2-fma.c` e `e_pow-fma.c`
//! (compiladas com `-mfma -mavx2`). Nelas `__FP_FAST_FMA` está definido, então os ramos `__builtin_fma`
//! valem, e o GCC (`-ffp-contract=fast`) ainda contrai por conta própria cada `a*b+c` cujo produto só
//! alimenta somas. Este módulo porta SOMENTE a variante FMA, com `mul_add` nesses dois tipos de ponto.
//! As contrações do compilador foram deduzidas da árvore de expressão (soma associa à esquerda, o produto
//! de uso único vira `fma`), não medidas: os testes contra o golden do bun (`math_common` do zjsc) é que
//! confirmam. Se divergir
//! em último bit, o suspeito é uma contração (ou a falta dela) numa dessas expressões. O ramo sem FMA
//! (`tab2`, `rhi`/`rlo`) não foi portado, as tabelas `*_TAB2` ficam sem uso.

use super::data::*;

#[inline]
fn d(bits: u64) -> f64 {
    f64::from_bits(bits)
}

use super::words::{fma, math_invalid};

#[inline]
fn top12(x: f64) -> u32 {
    (x.to_bits() >> 52) as u32
}

#[inline]
fn top16(x: f64) -> u32 {
    (x.to_bits() >> 48) as u32
}

/// `__math_oflow(sign)`: `±inf`.
fn math_oflow(negative: bool) -> f64 {
    if negative { f64::NEG_INFINITY } else { f64::INFINITY }
}

/// `__math_uflow(sign)`: `±0`.
fn math_uflow(negative: bool) -> f64 {
    if negative { -0.0 } else { 0.0 }
}

/// `__math_divzero(sign)`: `±inf`.
fn math_divzero(negative: bool) -> f64 {
    if negative { f64::NEG_INFINITY } else { f64::INFINITY }
}

const EXP_N_MASK: u64 = 127;
const EXP_TABLE_BITS: u32 = 7;

/// `specialcase` de `e_exp.c` (e de `e_pow.c`, que só difere em tratar o sinal): resultado
/// `scale * (1 + tmp)` sem arredondamento intermediário perto do overflow e do subnormal.
fn exp_special_case(tmp: f64, mut sbits: u64, ki: u64, signed: bool) -> f64 {
    if ki & 0x8000_0000 == 0 {
        sbits = sbits.wrapping_sub(1009u64 << 52);
        let scale = d(sbits);
        return exp2_pow(1009) * fma(scale, tmp, scale);
    }
    sbits = sbits.wrapping_add(1022u64 << 52);
    let scale = d(sbits);
    let mut y = fma(scale, tmp, scale);
    let limit_hit = if signed { y.abs() < 1.0 } else { y < 1.0 };
    if limit_hit {
        let one = if signed && y < 0.0 { -1.0 } else { 1.0 };
        let mut lo = fma(scale, tmp, scale - y);
        let hi = one + y;
        lo = one - hi + y + lo;
        y = (hi + lo) - one;
        if y == 0.0 {
            y = if signed { d(sbits & 0x8000_0000_0000_0000) } else { 0.0 };
        }
    }
    exp2_pow(-1022) * y
}

/// `2^e` exato como `f64` normal, para `-1022 <= e <= 1023`.
fn exp2_pow(e: i32) -> f64 {
    d(((e + 1023) as u64) << 52)
}

/// `__exp` (`e_exp.c`).
pub fn exp(x: f64) -> f64 {
    let mut abstop = top12(x) & 0x7ff;
    if abstop.wrapping_sub(top12(d(0x3c90_0000_0000_0000))) >= top12(512.0) - top12(d(0x3c90_0000_0000_0000)) {
        if abstop.wrapping_sub(top12(d(0x3c90_0000_0000_0000))) >= 0x8000_0000 {
            return 1.0 + x;
        }
        if abstop >= top12(1024.0) {
            if x.to_bits() == f64::NEG_INFINITY.to_bits() {
                return 0.0;
            }
            if abstop >= top12(f64::INFINITY) {
                return 1.0 + x;
            }
            if x.to_bits() >> 63 != 0 {
                return math_uflow(false);
            }
            return math_oflow(false);
        }
        abstop = 0;
    }

    let mut kd = fma(d(EXP_INVLN2N), x, d(EXP_SHIFT));
    let ki = kd.to_bits();
    kd -= d(EXP_SHIFT);
    let r = fma(kd, d(EXP_NEGLN2LO_N), fma(kd, d(EXP_NEGLN2HI_N), x));
    let idx = (2 * (ki & EXP_N_MASK)) as usize;
    let top = ki << (52 - EXP_TABLE_BITS);
    let tail = d(EXP_TAB[idx]);
    let sbits = EXP_TAB[idx + 1].wrapping_add(top);
    let r2 = r * r;
    let (c2, c3, c4, c5) = (d(EXP_POLY[0]), d(EXP_POLY[1]), d(EXP_POLY[2]), d(EXP_POLY[3]));
    let tmp = fma(r2 * r2, fma(r, c5, c4), fma(r2, fma(r, c3, c2), tail + r));
    if abstop == 0 {
        return exp_special_case(tmp, sbits, ki, false);
    }
    let scale = d(sbits);
    fma(scale, tmp, scale)
}

/// `__exp2` (`e_exp2.c`).
pub fn exp2(x: f64) -> f64 {
    let mut abstop = top12(x) & 0x7ff;
    if abstop.wrapping_sub(top12(d(0x3c90_0000_0000_0000))) >= top12(512.0) - top12(d(0x3c90_0000_0000_0000)) {
        if abstop.wrapping_sub(top12(d(0x3c90_0000_0000_0000))) >= 0x8000_0000 {
            return 1.0 + x;
        }
        if abstop >= top12(1024.0) {
            if x.to_bits() == f64::NEG_INFINITY.to_bits() {
                return 0.0;
            }
            if abstop >= top12(f64::INFINITY) {
                return 1.0 + x;
            }
            if x.to_bits() >> 63 == 0 {
                return math_oflow(false);
            } else if x.to_bits() >= (-1075.0f64).to_bits() {
                return math_uflow(false);
            }
        }
        if 2u64.wrapping_mul(x.to_bits()) > 2u64.wrapping_mul(928.0f64.to_bits()) {
            abstop = 0;
        }
    }

    let shift = d(EXP2_SHIFT);
    let mut kd = x + shift;
    let ki = kd.to_bits();
    kd -= shift;
    let r = x - kd;
    let idx = (2 * (ki & EXP_N_MASK)) as usize;
    let top = ki << (52 - EXP_TABLE_BITS);
    let tail = d(EXP_TAB[idx]);
    let sbits = EXP_TAB[idx + 1].wrapping_add(top);
    let r2 = r * r;
    let c: [f64; 5] = [d(EXP2_POLY[0]), d(EXP2_POLY[1]), d(EXP2_POLY[2]), d(EXP2_POLY[3]), d(EXP2_POLY[4])];
    let tmp = fma(r2 * r2, fma(r, c[4], c[3]), fma(r2, fma(r, c[2], c[1]), fma(r, c[0], tail)));
    if abstop == 0 {
        return exp2_special_case(tmp, sbits, ki);
    }
    let scale = d(sbits);
    fma(scale, tmp, scale)
}

/// `specialcase` de `e_exp2.c`.
fn exp2_special_case(tmp: f64, mut sbits: u64, ki: u64) -> f64 {
    if ki & 0x8000_0000 == 0 {
        sbits = sbits.wrapping_sub(1u64 << 52);
        let scale = d(sbits);
        return 2.0 * fma(scale, tmp, scale);
    }
    sbits = sbits.wrapping_add(1022u64 << 52);
    let scale = d(sbits);
    let mut y = fma(scale, tmp, scale);
    if y < 1.0 {
        let mut lo = fma(scale, tmp, scale - y);
        let hi = 1.0 + y;
        lo = 1.0 - hi + y + lo;
        y = (hi + lo) - 1.0;
        if y == 0.0 {
            y = 0.0;
        }
    }
    exp2_pow(-1022) * y
}

/// `__log` (`e_log.c`).
pub fn log(x: f64) -> f64 {
    let mut ix = x.to_bits();
    let top = top16(x);
    let lo_bits = (1.0f64 - d(0x3fb0_0000_0000_0000)).to_bits();
    let hi_bits = (1.0f64 + d(0x3fb0_9000_0000_0000)).to_bits();
    // `B` e `A` do C: `poly1` e `poly`; `B[0]` vale -0.5.
    let b: [f64; 11] = std::array::from_fn(|index| d(LOG_POLY1[index]));
    let a: [f64; 5] = std::array::from_fn(|index| d(LOG_POLY[index]));
    if ix.wrapping_sub(lo_bits) < hi_bits - lo_bits {
        if ix == 1.0f64.to_bits() {
            return 0.0;
        }
        let r = x - 1.0;
        let r2 = r * r;
        let r3 = r * r2;
        let inner2 = fma(r3, b[10], fma(r2, b[9], fma(r, b[8], b[7])));
        let inner1 = fma(r3, inner2, fma(r2, b[6], fma(r, b[5], b[4])));
        let y0 = r3 * fma(r3, inner1, fma(r2, b[3], fma(r, b[2], b[1])));
        let w = r * d(0x41a0_0000_0000_0000);
        let rhi = r + w - w;
        let rlo = r - rhi;
        let w = rhi * rhi * b[0];
        let hi = r + w;
        let mut lo = r - hi + w;
        lo = fma(b[0] * rlo, rhi + r, lo);
        let mut y = y0 + lo;
        y += hi;
        return y;
    }
    if top.wrapping_sub(0x0010) >= 0x7ff0 - 0x0010 {
        if ix.wrapping_mul(2) == 0 {
            return math_divzero(true);
        }
        if ix == f64::INFINITY.to_bits() {
            return x;
        }
        if top & 0x8000 != 0 || top & 0x7ff0 == 0x7ff0 {
            return f64::NAN;
        }
        ix = (x * d(0x4330_0000_0000_0000)).to_bits();
        ix = ix.wrapping_sub(52u64 << 52);
    }

    let tmp = ix.wrapping_sub(0x3fe6_0000_0000_0000);
    let i = ((tmp >> (52 - 7)) % 128) as usize;
    let k = (tmp as i64) >> 52;
    let iz = ix.wrapping_sub(tmp & (0xfffu64 << 52));
    let invc = d(LOG_TAB[i][0]);
    let logc = d(LOG_TAB[i][1]);
    let z = d(iz);
    let r = fma(z, invc, -1.0);
    let kd = k as f64;

    let w = fma(kd, d(LOG_LN2HI), logc);
    let hi = w + r;
    let lo = fma(kd, d(LOG_LN2LO), w - hi + r);

    let r2 = r * r;
    let q = fma(r2, fma(r, a[4], a[3]), fma(r, a[2], a[1]));
    fma(r * r2, q, fma(r2, a[0], lo)) + hi
}

/// `__log2` (`e_log2.c`).
pub fn log2(x: f64) -> f64 {
    let mut ix = x.to_bits();
    let top = top16(x);
    let lo_bits = (1.0f64 - d(0x3fa5_b510_0000_0000)).to_bits();
    let hi_bits = (1.0f64 + d(0x3fa6_ab20_0000_0000)).to_bits();
    let b: [f64; 10] = std::array::from_fn(|index| d(LOG2_POLY1[index]));
    let a: [f64; 6] = std::array::from_fn(|index| d(LOG2_POLY[index]));
    let inv_ln2_hi = d(LOG2_INVLN2HI);
    let inv_ln2_lo = d(LOG2_INVLN2LO);
    if ix.wrapping_sub(lo_bits) < hi_bits - lo_bits {
        if ix == 1.0f64.to_bits() {
            return 0.0;
        }
        let r = x - 1.0;
        let hi = r * inv_ln2_hi;
        let mut lo = fma(r, inv_ln2_lo, fma(r, inv_ln2_hi, -hi));
        let r2 = r * r;
        let r4 = r2 * r2;
        let pfac = fma(r, b[1], b[0]);
        let mut y = fma(r2, pfac, hi);
        lo += fma(r2, pfac, hi - y);
        let u = fma(r2, fma(r, b[9], b[8]), fma(r, b[7], b[6]));
        let s = fma(r4, u, fma(r2, fma(r, b[5], b[4]), fma(r, b[3], b[2])));
        lo = fma(r4, s, lo);
        y += lo;
        return y;
    }
    if top.wrapping_sub(0x0010) >= 0x7ff0 - 0x0010 {
        if ix.wrapping_mul(2) == 0 {
            return math_divzero(true);
        }
        if ix == f64::INFINITY.to_bits() {
            return x;
        }
        if top & 0x8000 != 0 || top & 0x7ff0 == 0x7ff0 {
            return f64::NAN;
        }
        ix = (x * d(0x4330_0000_0000_0000)).to_bits();
        ix = ix.wrapping_sub(52u64 << 52);
    }

    let tmp = ix.wrapping_sub(0x3fe6_0000_0000_0000);
    let i = ((tmp >> (52 - 6)) % 64) as usize;
    let k = (tmp as i64) >> 52;
    let iz = ix.wrapping_sub(tmp & (0xfffu64 << 52));
    let invc = d(LOG2_TAB[i][0]);
    let logc = d(LOG2_TAB[i][1]);
    let z = d(iz);
    let kd = k as f64;

    let r = fma(z, invc, -1.0);
    let t1 = r * inv_ln2_hi;
    let t2 = fma(r, inv_ln2_lo, fma(r, inv_ln2_hi, -t1));

    let t3 = kd + logc;
    let hi = t3 + t1;
    let lo = t3 - hi + t1 + t2;

    let r2 = r * r;
    let r4 = r2 * r2;
    let p = fma(r4, fma(r, a[5], a[4]), fma(r2, fma(r, a[3], a[2]), fma(r, a[1], a[0])));
    fma(r2, p, lo) + hi
}

/// `checkint`: 0 se não é inteiro, 1 se ímpar, 2 se par.
fn check_int(iy: u64) -> i32 {
    let e = ((iy >> 52) & 0x7ff) as i32;
    if e < 0x3ff {
        return 0;
    }
    if e > 0x3ff + 52 {
        return 2;
    }
    if iy & ((1u64 << (0x3ff + 52 - e)) - 1) != 0 {
        return 0;
    }
    if iy & (1u64 << (0x3ff + 52 - e)) != 0 {
        return 1;
    }
    2
}

/// `zeroinfnan`: o padrão de bits é 0, infinito ou NaN.
fn zero_inf_nan(i: u64) -> bool {
    i.wrapping_mul(2).wrapping_sub(1) >= 2 * f64::INFINITY.to_bits() - 1
}

/// `issignaling_inline` (x86_64: bit alto da mantissa zero significa sinalizador).
fn is_signaling(x: f64) -> bool {
    let ix = x.to_bits();
    (ix ^ 0x0008_0000_0000_0000).wrapping_mul(2) > 2 * 0x7ff8_0000_0000_0000u64
}

/// `log_inline` de `e_pow.c`: `y + tail = log(x)`.
fn pow_log_inline(ix: u64) -> (f64, f64) {
    let tmp = ix.wrapping_sub(0x3fe6_9555_0000_0000);
    let i = ((tmp >> (52 - 7)) % 128) as usize;
    let k = (tmp as i64) >> 52;
    let iz = ix.wrapping_sub(tmp & (0xfffu64 << 52));
    let z = d(iz);
    let kd = k as f64;
    let invc = d(POW_LOG_TAB[i][0]);
    let logc = d(POW_LOG_TAB[i][2]);
    let logctail = d(POW_LOG_TAB[i][3]);
    let a: [f64; 7] = std::array::from_fn(|index| d(POW_LOG_POLY[index]));

    let r = fma(z, invc, -1.0);
    let t1 = fma(kd, d(POW_LN2HI), logc);
    let t2 = t1 + r;
    let lo1 = fma(kd, d(POW_LN2LO), logctail);
    let lo2 = t1 - t2 + r;
    let ar = a[0] * r;
    let ar2 = r * ar;
    let ar3 = r * ar2;
    let hi = t2 + ar2;
    let lo3 = fma(ar, r, -ar2);
    let lo4 = t2 - hi + ar2;
    let q = fma(ar2, fma(ar2, fma(r, a[6], a[5]), fma(r, a[4], a[3])), fma(r, a[2], a[1]));
    let lo = fma(ar3, q, lo1 + lo2 + lo3 + lo4);
    let y = hi + lo;
    (y, hi - y + lo)
}

/// `exp_inline` de `e_pow.c`: `sign * exp(x + xtail)`.
fn pow_exp_inline(x: f64, xtail: f64, sign_bias: u32) -> f64 {
    let mut abstop = top12(x) & 0x7ff;
    if abstop.wrapping_sub(top12(d(0x3c90_0000_0000_0000))) >= top12(512.0) - top12(d(0x3c90_0000_0000_0000)) {
        if abstop.wrapping_sub(top12(d(0x3c90_0000_0000_0000))) >= 0x8000_0000 {
            let one = 1.0 + x;
            return if sign_bias != 0 { -one } else { one };
        }
        if abstop >= top12(1024.0) {
            if x.to_bits() >> 63 != 0 {
                return math_uflow(sign_bias != 0);
            }
            return math_oflow(sign_bias != 0);
        }
        abstop = 0;
    }

    let mut kd = fma(d(EXP_INVLN2N), x, d(EXP_SHIFT));
    let ki = kd.to_bits();
    kd -= d(EXP_SHIFT);
    let mut r = fma(kd, d(EXP_NEGLN2LO_N), fma(kd, d(EXP_NEGLN2HI_N), x));
    r += xtail;
    let idx = (2 * (ki & EXP_N_MASK)) as usize;
    let top = ki.wrapping_add(sign_bias as u64) << (52 - EXP_TABLE_BITS);
    let tail = d(EXP_TAB[idx]);
    let sbits = EXP_TAB[idx + 1].wrapping_add(top);
    let r2 = r * r;
    let (c2, c3, c4, c5) = (d(EXP_POLY[0]), d(EXP_POLY[1]), d(EXP_POLY[2]), d(EXP_POLY[3]));
    let tmp = fma(r2 * r2, fma(r, c5, c4), fma(r2, fma(r, c3, c2), tail + r));
    if abstop == 0 {
        return exp_special_case(tmp, sbits, ki, true);
    }
    let scale = d(sbits);
    fma(scale, tmp, scale)
}

/// `__pow` (`e_pow.c`), sem o tratamento de `operationMathPow` do JSC (que o chamador faz antes).
pub fn pow(x: f64, y: f64) -> f64 {
    let mut sign_bias: u32 = 0;
    let mut ix = x.to_bits();
    let iy = y.to_bits();
    let mut topx = top12(x);
    let topy = top12(y);

    if topx.wrapping_sub(0x001) >= 0x7ff - 0x001 || (topy & 0x7ff).wrapping_sub(0x3be) >= 0x43e - 0x3be {
        if zero_inf_nan(iy) {
            if iy.wrapping_mul(2) == 0 {
                return if is_signaling(x) { x + y } else { 1.0 };
            }
            if ix == 1.0f64.to_bits() {
                return if is_signaling(y) { x + y } else { 1.0 };
            }
            let inf2 = 2 * f64::INFINITY.to_bits();
            if ix.wrapping_mul(2) > inf2 || iy.wrapping_mul(2) > inf2 {
                return x + y;
            }
            if ix.wrapping_mul(2) == 2 * 1.0f64.to_bits() {
                return 1.0;
            }
            if (ix.wrapping_mul(2) < 2 * 1.0f64.to_bits()) == ((iy >> 63) == 0) {
                return 0.0;
            }
            return y * y;
        }
        if zero_inf_nan(ix) {
            let mut x2 = x * x;
            if ix >> 63 != 0 && check_int(iy) == 1 {
                x2 = -x2;
                sign_bias = 1;
            }
            if ix.wrapping_mul(2) == 0 && iy >> 63 != 0 {
                return math_divzero(sign_bias != 0);
            }
            return if iy >> 63 != 0 { 1.0 / x2 } else { x2 };
        }
        if ix >> 63 != 0 {
            let yint = check_int(iy);
            if yint == 0 {
                return math_invalid(x);
            }
            if yint == 1 {
                sign_bias = 0x800 << 7;
            }
            ix &= 0x7fff_ffff_ffff_ffff;
            topx &= 0x7ff;
        }
        if (topy & 0x7ff).wrapping_sub(0x3be) >= 0x43e - 0x3be {
            if ix == 1.0f64.to_bits() {
                return 1.0;
            }
            if (topy & 0x7ff) < 0x3be {
                return if ix > 1.0f64.to_bits() { 1.0 + y } else { 1.0 - y };
            }
            return if (ix > 1.0f64.to_bits()) == (topy < 0x800) { math_oflow(false) } else { math_uflow(false) };
        }
        if topx == 0 {
            ix = (x * d(0x4330_0000_0000_0000)).to_bits();
            ix &= 0x7fff_ffff_ffff_ffff;
            ix = ix.wrapping_sub(52u64 << 52);
        }
    }

    let (hi, lo) = pow_log_inline(ix);
    let ehi = y * hi;
    let elo = fma(y, lo, fma(y, hi, -ehi));
    pow_exp_inline(ehi, elo, sign_bias)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp_and_log_special_values() {
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(exp(f64::NEG_INFINITY), 0.0);
        assert_eq!(exp(1000.0), f64::INFINITY);
        assert_eq!(log(1.0), 0.0);
        assert_eq!(log(0.0), f64::NEG_INFINITY);
        assert!(log(-1.0).is_nan());
        assert_eq!(exp2(10.0), 1024.0);
        assert_eq!(log2(8.0), 3.0);
        assert_eq!(pow(2.0, 10.0), 1024.0);
    }
}
