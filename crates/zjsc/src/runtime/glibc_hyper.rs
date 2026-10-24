//! Porte do libm do glibc 2.41 (`sysdeps/ieee754/dbl-64`) para `sinh`, `cosh`, `tanh`, `expm1`, `log1p`,
//! `cbrt`, `log10`, `asinh`, `acosh` e `atanh`, bit a bit com o que o bun (glibc, x86_64) devolve.
//!
//! Arquivos de origem: `e_sinh.c`, `e_cosh.c`, `s_tanh.c`, `s_expm1.c`, `s_log1p.c`, `s_cbrt.c`,
//! `e_log10.c`, `s_asinh.c`, `e_acosh.c`, `e_atanh.c`.
//!
//! Contração em FMA: no x86_64 o ifunc escolhe `__expm1_fma` e `__log1p_fma` (compilados com `-mfma`), então
//! `expm1` e `log1p` usam `mul_add` onde o GCC contrai `a*b+c` (deduzido da árvore de expressão, NÃO medido).
//! `exp` e `log` vêm de `ul_common::libm` (também variante FMA). Os demais arquivos não têm multiarch: compilados
//! para o x86_64 base, sem FMA, então aqui são contas simples.
//! Pontos de dúvida se o golden divergir em último bit: em `log1p`, `R = fma(z, Lp1, z2*R2) + ...` (qual dos
//! dois produtos o GCC funde) e `s*(hfsq+R) + (k*ln2_lo + c)` do ramo final.

use ul_common::libm::{exp, log};

const ONE: f64 = 1.0;
const LN2: f64 = 6.931_471_805_599_453_094_17e-1; // 0x3fe62e42fefa39ef

use ul_common::libm::words::{fma, high_word, low_word, math_invalid, set_high_word};

/// Soma `k` ao expoente pelo high word (`SET_HIGH_WORD (y, high + (k << 20))`).
#[inline]
fn add_to_exponent(y: f64, k: i32) -> f64 {
    set_high_word(y, high_word(y).wrapping_add((k as u32) << 20))
}

/// `__expm1` (`s_expm1.c`, variante FMA).
pub fn expm1(x: f64) -> f64 {
    const HUGE: f64 = 1.0e300;
    const TINY: f64 = 1.0e-300;
    const O_THRESHOLD: f64 = 7.097_827_128_933_839_730_96e2;
    const LN2_HI: f64 = 6.931_471_803_691_238_164_90e-1;
    const LN2_LO: f64 = 1.908_214_929_270_587_700_02e-10;
    const INVLN2: f64 = 1.442_695_040_888_963_387_00;
    const Q: [f64; 6] = [
        1.0,
        -3.333_333_333_333_313_164_28e-2,
        1.587_301_587_254_814_601_65e-3,
        -7.936_507_578_674_879_424_73e-5,
        4.008_217_827_329_362_395_52e-6,
        -2.010_992_181_836_243_713_26e-7,
    ];

    let mut x = x;
    let mut hx = high_word(x);
    let xsb = hx & 0x8000_0000;
    hx &= 0x7fff_ffff;

    if hx >= 0x4043_687A {
        if hx >= 0x4086_2E42 {
            if hx >= 0x7ff0_0000 {
                if ((hx & 0xfffff) | low_word(x)) != 0 {
                    return x + x;
                }
                return if xsb == 0 { x } else { -1.0 };
            }
            if x > O_THRESHOLD {
                return HUGE * HUGE;
            }
        }
        if xsb != 0 {
            return TINY - ONE;
        }
    }

    let k: i32;
    let c: f64;
    if hx > 0x3fd6_2e42 {
        let (hi, lo);
        if hx < 0x3FF0_A2B2 {
            if xsb == 0 {
                hi = x - LN2_HI;
                lo = LN2_LO;
                k = 1;
            } else {
                hi = x + LN2_HI;
                lo = -LN2_LO;
                k = -1;
            }
        } else {
            k = fma(INVLN2, x, if xsb == 0 { 0.5 } else { -0.5 }) as i32;
            let t = k as f64;
            hi = fma(-t, LN2_HI, x);
            lo = t * LN2_LO;
        }
        x = hi - lo;
        c = (hi - x) - lo;
    } else if hx < 0x3c90_0000 {
        return x;
    } else {
        k = 0;
        c = 0.0;
    }

    let hfx = 0.5 * x;
    let hxs = x * hfx;
    let r1a = fma(hxs, Q[1], Q[0]);
    let h2 = hxs * hxs;
    let r2 = fma(hxs, Q[3], Q[2]);
    let h4 = h2 * h2;
    let r3 = fma(hxs, Q[5], Q[4]);
    let r1 = fma(h4, r3, fma(h2, r2, r1a));
    let t = fma(-r1, hfx, 3.0);
    let mut e = hxs * ((r1 - t) / fma(-x, t, 6.0));
    if k == 0 {
        return x - fma(x, e, -hxs);
    }
    e = fma(x, e - c, -c);
    e -= hxs;
    if k == -1 {
        return 0.5 * (x - e) - 0.5;
    }
    if k == 1 {
        if x < -0.25 {
            return -2.0 * (e - (x + 0.5));
        }
        return ONE + 2.0 * (x - e);
    }
    if k <= -2 || k > 56 {
        let y = ONE - (e - x);
        return add_to_exponent(y, k) - ONE;
    }
    if k < 20 {
        let t = set_high_word(ONE, 0x3ff0_0000 - (0x20_0000 >> k));
        let y = t - (e - x);
        add_to_exponent(y, k)
    } else {
        let t = set_high_word(ONE, ((0x3ff - k) as u32) << 20);
        let mut y = x - (e + t);
        y += ONE;
        add_to_exponent(y, k)
    }
}

/// `__log1p` (`s_log1p.c`, variante FMA).
pub fn log1p(x: f64) -> f64 {
    const LN2_HI: f64 = 6.931_471_803_691_238_164_90e-1;
    const LN2_LO: f64 = 1.908_214_929_270_587_700_02e-10;
    const TWO54: f64 = 1.801_439_850_948_198_400_00e16;
    const LP: [f64; 8] = [
        0.0,
        6.666_666_666_666_735_130e-1,
        3.999_999_999_940_941_908e-1,
        2.857_142_874_366_239_149e-1,
        2.222_219_843_214_978_396e-1,
        1.818_357_216_161_805_012e-1,
        1.531_383_769_920_937_332e-1,
        1.479_819_860_511_658_591e-1,
    ];

    let hx = high_word(x) as i32;
    let ax = hx & 0x7fff_ffff;
    let mut k: i32 = 1;
    let mut f = 0.0;
    let mut hu: i32 = 0;
    let mut c = 0.0;
    if hx < 0x3FDA_827A {
        if ax >= 0x3ff0_0000 {
            if x == -1.0 {
                return -TWO54 / 0.0;
            }
            return math_invalid(x);
        }
        if ax < 0x3e20_0000 {
            if ax < 0x3c90_0000 {
                return x;
            }
            return x - x * x * 0.5;
        }
        if hx > 0 || hx <= 0xbfd2_bec3_u32 as i32 {
            k = 0;
            f = x;
            hu = 1;
        }
    } else if hx >= 0x7ff0_0000 {
        return x + x;
    }
    if k != 0 {
        let mut u;
        if hx < 0x4340_0000 {
            u = 1.0 + x;
            hu = high_word(u) as i32;
            k = (hu >> 20) - 1023;
            c = if k > 0 { 1.0 - (u - x) } else { x - (u - 1.0) };
            c /= u;
        } else {
            u = x;
            hu = high_word(u) as i32;
            k = (hu >> 20) - 1023;
            c = 0.0;
        }
        hu &= 0x000f_ffff;
        if hu < 0x6a09e {
            u = set_high_word(u, (hu | 0x3ff0_0000) as u32);
        } else {
            k += 1;
            u = set_high_word(u, (hu | 0x3fe0_0000) as u32);
            hu = (0x0010_0000 - hu) >> 2;
        }
        f = u - 1.0;
    }
    let kd = k as f64;
    let hfsq = 0.5 * f * f;
    if hu == 0 {
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            c = fma(kd, LN2_LO, c);
            return fma(kd, LN2_HI, c);
        }
        let r = hfsq * fma(-0.666_666_666_666_666_66, f, 1.0);
        if k == 0 {
            return f - r;
        }
        return fma(kd, LN2_HI, -((r - fma(kd, LN2_LO, c)) - f));
    }
    let s = f / (2.0 + f);
    let z = s * s;
    let z2 = z * z;
    let r2 = fma(z, LP[3], LP[2]);
    let z4 = z2 * z2;
    let r3 = fma(z, LP[5], LP[4]);
    let z6 = z4 * z2;
    let r4 = fma(z, LP[7], LP[6]);
    let r = fma(z6, r4, fma(z4, r3, fma(z, LP[1], z2 * r2)));
    // O fim dos dois ramos sai sem contração em FMA (medido contra o bun em scripts/log1p-probe-k.js: 0
    // divergências em 24000 pontos; a versão fundida divergia em 1 ulp, ex.: log1p(0xbfc899451f000000)).
    if k == 0 {
        return f - (hfsq - s * (hfsq + r));
    }
    kd * LN2_HI - ((hfsq - (s * (hfsq + r) + (kd * LN2_LO + c))) - f)
}

/// `__ieee754_sinh` (`e_sinh.c`).
pub fn sinh(x: f64) -> f64 {
    const SHUGE: f64 = 1.0e307;
    let jx = high_word(x) as i32;
    let ix = jx & 0x7fff_ffff;
    if ix >= 0x7ff0_0000 {
        return x + x;
    }
    let h = if jx < 0 { -0.5 } else { 0.5 };
    if ix < 0x4036_0000 {
        if ix < 0x3e30_0000 && SHUGE + x > ONE {
            return x;
        }
        let t = expm1(x.abs());
        if ix < 0x3ff0_0000 {
            return h * (2.0 * t - t * t / (t + ONE));
        }
        return h * (t + t / (t + ONE));
    }
    if ix < 0x4086_2e42 {
        return h * exp(x.abs());
    }
    let lx = low_word(x);
    if ix < 0x4086_33ce || (ix == 0x4086_33ce && lx <= 0x8fb9_f87d) {
        let w = exp(0.5 * x.abs());
        let t = h * w;
        return t * w;
    }
    x * SHUGE
}

/// `__ieee754_cosh` (`e_cosh.c`).
pub fn cosh(x: f64) -> f64 {
    const HALF: f64 = 0.5;
    const HUGE: f64 = 1.0e300;
    let ix = (high_word(x) & 0x7fff_ffff) as i32;
    if ix < 0x4036_0000 {
        if ix < 0x3fd6_2e43 {
            if ix < 0x3c80_0000 {
                return ONE;
            }
            let t = expm1(x.abs());
            let w = ONE + t;
            return ONE + (t * t) / (w + w);
        }
        let t = exp(x.abs());
        return HALF * t + HALF / t;
    }
    if ix < 0x4086_2e42 {
        return HALF * exp(x.abs());
    }
    let fix = x.to_bits() & 0x7fff_ffff_ffff_ffff;
    if fix <= 0x4086_33ce_8fb9_f87d {
        let w = exp(HALF * x.abs());
        let t = HALF * w;
        return t * w;
    }
    if ix >= 0x7ff0_0000 {
        return x * x;
    }
    HUGE * HUGE
}

/// `__tanh` (`s_tanh.c`).
pub fn tanh(x: f64) -> f64 {
    const TWO: f64 = 2.0;
    const TINY: f64 = 1.0e-300;
    let bits = x.to_bits();
    let jx = (bits >> 32) as u32 as i32;
    let lx = bits as u32;
    let ix = jx & 0x7fff_ffff;
    if ix >= 0x7ff0_0000 {
        return if jx >= 0 { ONE / x + ONE } else { ONE / x - ONE };
    }
    let z;
    if ix < 0x4036_0000 {
        if (ix as u32 | lx) == 0 {
            return x;
        }
        if ix < 0x3c80_0000 {
            return x * (ONE + x);
        }
        if ix >= 0x3ff0_0000 {
            let t = expm1(TWO * x.abs());
            z = ONE - TWO / (t + TWO);
        } else {
            let t = expm1(-TWO * x.abs());
            z = -t / (t + TWO);
        }
    } else {
        z = ONE - TINY;
    }
    if jx >= 0 { z } else { -z }
}

/// `frexp` do glibc: mantissa em `[0.5, 1)` e expoente; zero, infinito e NaN voltam com expoente 0.
fn frexp(x: f64) -> (f64, i32) {
    let mut bits = x.to_bits();
    let mut exp_field = ((bits >> 52) & 0x7ff) as i32;
    if exp_field == 0x7ff || x == 0.0 {
        return (x + x, 0);
    }
    let mut adjust = 0;
    if exp_field == 0 {
        // subnormal: normaliza por 2^54
        let scaled = x * f64::from_bits(0x4350_0000_0000_0000);
        bits = scaled.to_bits();
        exp_field = ((bits >> 52) & 0x7ff) as i32;
        adjust = -54;
    }
    let e = exp_field - 1022 + adjust;
    let mantissa = f64::from_bits((bits & !(0x7ff_u64 << 52)) | (1022_u64 << 52));
    (mantissa, e)
}

/// `ldexp`/`scalbn`: `x * 2^n` com um único arredondamento (como o do glibc).
fn ldexp(x: f64, n: i32) -> f64 {
    let two1023 = f64::from_bits(0x7fe0_0000_0000_0000);
    let two_m969 = f64::from_bits(0x0360_0000_0000_0000); // 2^(-1022+53)
    let mut y = x;
    let mut n = n;
    if n > 1023 {
        y *= two1023;
        n -= 1023;
        if n > 1023 {
            y *= two1023;
            n -= 1023;
            n = n.min(1023);
        }
    } else if n < -1022 {
        y *= two_m969;
        n += 1022 - 53;
        if n < -1022 {
            y *= two_m969;
            n += 1022 - 53;
            n = n.max(-1022);
        }
    }
    y * f64::from_bits(((0x3ff + n) as u64) << 52)
}

/// Multiplicação 128 x 128 bits com resultado de 256 bits, como `(alto, baixo)`.
fn wide_mul(a: u128, b: u128) -> (u128, u128) {
    let mask = u128::from(u64::MAX);
    let (a0, a1) = (a & mask, a >> 64);
    let (b0, b1) = (b & mask, b >> 64);
    let (p00, p01, p10, p11) = (a0 * b0, a0 * b1, a1 * b0, a1 * b1);
    let mid = (p00 >> 64) + (p01 & mask) + (p10 & mask);
    let low = (p00 & mask) | (mid << 64);
    let high = p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64);
    (high, low)
}

/// `v << shift` em 256 bits, para `v < 2^53` e `1 <= shift <= 200`.
fn wide_shl(v: u128, shift: u32) -> (u128, u128) {
    if shift >= 128 {
        (v << (shift - 128), 0)
    } else {
        (v >> (128 - shift), v << shift)
    }
}

/// Mantissa inteira e expoente de um `f64` finito positivo: `x = m * 2^e`.
fn mantissa_exponent(x: f64) -> (u64, i32) {
    let bits = x.to_bits();
    let field = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1_u64 << 52) - 1);
    if field == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1_u64 << 52), field - 1075)
    }
}

/// Compara o cubo do ponto médio entre o double `bits` e o seguinte com `x = mx * 2^ex`, em aritmética exata.
fn midpoint_cube_cmp(bits: u64, mx: u64, ex: i32) -> std::cmp::Ordering {
    let (m, e) = mantissa_exponent(f64::from_bits(bits));
    let t = u128::from(2 * m + 1);
    let cube = wide_mul(t * t, t);
    let d = 3 * e - 3 - ex;
    if d >= 0 {
        return std::cmp::Ordering::Greater;
    }
    let shift = (-d) as u32;
    if shift > 200 {
        return std::cmp::Ordering::Less;
    }
    cube.cmp(&wide_shl(u128::from(mx), shift))
}

/// `cbrt` do bun 1.4.2 (glibc 2.41 em x86_64): resultado corretamente arredondado. O `__cbrt` antigo
/// (`s_cbrt.c`) serve de semente e a escolha final entre os vizinhos é feita em aritmética inteira exata.
pub fn cbrt(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() || x == 0.0 {
        return x + x;
    }
    let ax = x.abs();
    let (mx, ex) = mantissa_exponent(ax);
    let mut bits = cbrt_seed(ax).to_bits();
    loop {
        if midpoint_cube_cmp(bits - 1, mx, ex) == std::cmp::Ordering::Greater {
            bits -= 1;
        } else if midpoint_cube_cmp(bits, mx, ex) == std::cmp::Ordering::Less {
            bits += 1;
        } else {
            break;
        }
    }
    let y = f64::from_bits(bits);
    if x < 0.0 { -y } else { y }
}

/// `__cbrt` (`s_cbrt.c`), sem FMA.
fn cbrt_seed(x: f64) -> f64 {
    const CBRT2: f64 = 1.259_921_049_894_873_164_8;
    const SQR_CBRT2: f64 = 1.587_401_051_968_199_474_8;
    const FACTOR: [f64; 5] = [1.0 / SQR_CBRT2, 1.0 / CBRT2, 1.0, CBRT2, SQR_CBRT2];
    let (xm, xe) = frexp(x.abs());
    if xe == 0 && (x.is_nan() || x.is_infinite() || x == 0.0) {
        return x + x;
    }
    let u = 0.354_895_765_043_919_860
        + ((1.508_191_937_815_848_96
            + ((-2.114_994_941_673_712_87
                + ((2.446_931_225_635_344_30
                    + ((-1.834_692_774_836_130_86 + (0.784_932_344_976_639_262 - 0.145_263_899_385_486_377 * xm) * xm)
                        * xm))
                    * xm))
                * xm))
            * xm);
    let t2 = u * u * u;
    let ym = u * (t2 + 2.0 * xm) / (2.0 * t2 + xm) * FACTOR[(2 + xe % 3) as usize];
    ldexp(if x > 0.0 { ym } else { -ym }, xe / 3)
}

/// `__ieee754_log10` (`e_log10.c`), sem FMA própria; usa o `log` FMA.
pub fn log10(x: f64) -> f64 {
    const TWO54: f64 = 1.801_439_850_948_198_400_00e16;
    const IVLN10: f64 = 4.342_944_819_032_518_166_68e-1;
    const LOG10_2HI: f64 = 3.010_299_956_636_117_713_06e-1;
    const LOG10_2LO: f64 = 3.694_239_077_158_930_786_16e-13;
    let mut x = x;
    let mut hx = x.to_bits() as i64;
    let mut k: i32 = 0;
    if hx < 0x0010_0000_0000_0000 {
        if (hx & 0x7fff_ffff_ffff_ffff) == 0 {
            return -TWO54 / x.abs();
        }
        if hx < 0 {
            return math_invalid(x);
        }
        k -= 54;
        x *= TWO54;
        hx = x.to_bits() as i64;
    }
    if hx >= 0x7ff0_0000_0000_0000 {
        return x + x;
    }
    k += ((hx >> 52) - 1023) as i32;
    let i = ((k as i64 as u64) >> 63) as i64;
    hx = (hx & 0x000f_ffff_ffff_ffff) | ((0x3ff - i) << 52);
    let y = (k as i64 + i) as f64;
    let x = f64::from_bits(hx as u64);
    let z = y * LOG10_2LO + IVLN10 * log(x);
    z + y * LOG10_2HI
}

/// `__asinh` (`s_asinh.c`).
pub fn asinh(x: f64) -> f64 {
    const HUGE: f64 = 1.0e300;
    let hx = high_word(x) as i32;
    let ix = hx & 0x7fff_ffff;
    if ix < 0x3e30_0000 && HUGE + x > ONE {
        return x;
    }
    let w;
    if ix > 0x41b0_0000 {
        if ix >= 0x7ff0_0000 {
            return x + x;
        }
        w = log(x.abs()) + LN2;
    } else {
        let xa = x.abs();
        if ix > 0x4000_0000 {
            w = log(2.0 * xa + ONE / ((xa * xa + ONE).sqrt() + xa));
        } else {
            let t = xa * xa;
            w = log1p(xa + t / (ONE + (ONE + t).sqrt()));
        }
    }
    w.copysign(x)
}

/// `__ieee754_acosh` (`e_acosh.c`).
pub fn acosh(x: f64) -> f64 {
    let hx = x.to_bits() as i64;
    if hx > 0x4000_0000_0000_0000 {
        if hx >= 0x41b0_0000_0000_0000 {
            if hx >= 0x7ff0_0000_0000_0000 {
                return x + x;
            }
            return log(x) + LN2;
        }
        let t = x * x;
        return log(2.0 * x - ONE / (x + (t - ONE).sqrt()));
    }
    if hx > 0x3ff0_0000_0000_0000 {
        let t = x - ONE;
        return log1p(t + (2.0 * t + t * t).sqrt());
    }
    if hx == 0x3ff0_0000_0000_0000 {
        return 0.0;
    }
    math_invalid(x)
}

/// `__ieee754_atanh` (`e_atanh.c`).
pub fn atanh(x: f64) -> f64 {
    const HUGE: f64 = 1e300;
    let xa = x.abs();
    let t;
    if xa < 0.5 {
        if xa < f64::from_bits(0x3e30_0000_0000_0000) {
            let _ = HUGE + x;
            return x;
        }
        let t2 = xa + xa;
        t = 0.5 * log1p(t2 + t2 * xa / (1.0 - xa));
    } else if xa < 1.0 {
        t = 0.5 * log1p((xa + xa) / (1.0 - xa));
    } else {
        if xa > 1.0 {
            return math_invalid(x);
        }
        return x / 0.0;
    }
    t.copysign(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Casos de `tests/golden/math_bun.tsv` (bun, glibc 2.41): (função, bits da entrada, bits do resultado).
    const CASES: [(&str, u64, u64); 50] = [
        ("sinh", 0x3ff0000000000000, 0x3ff2cd9fc44eb982),
        ("sinh", 0xbff0000000000000, 0xbff2cd9fc44eb982),
        ("sinh", 0x3fe6666666666666, 0x3fe8465153d5bdbc),
        ("sinh", 0x431160ac8f9e32ee, 0x7ff0000000000000),
        ("sinh", 0xebe43442c0e41c74, 0xfff0000000000000),
        ("cosh", 0x3ff0000000000000, 0x3ff8b07551d9f550),
        ("cosh", 0xbff0000000000000, 0x3ff8b07551d9f550),
        ("cosh", 0x3fe6666666666666, 0x3ff4152c1862342f),
        ("cosh", 0x431160ac8f9e32ee, 0x7ff0000000000000),
        ("cosh", 0xebe43442c0e41c74, 0x7ff0000000000000),
        ("tanh", 0x3ff0000000000000, 0x3fe85efab514f394),
        ("tanh", 0xbff0000000000000, 0xbfe85efab514f394),
        ("tanh", 0x3fe6666666666666, 0x3fe356fb17af2e92),
        ("tanh", 0x431160ac8f9e32ee, 0x3ff0000000000000),
        ("tanh", 0xebe43442c0e41c74, 0xbff0000000000000),
        ("expm1", 0x3ff0000000000000, 0x3ffb7e151628aed2),
        ("expm1", 0xbff0000000000000, 0xbfe43a54e4e98864),
        ("expm1", 0x3fe6666666666666, 0x3ff03854c24d130d),
        ("expm1", 0x431160ac8f9e32ee, 0x7ff0000000000000),
        ("expm1", 0xebe43442c0e41c74, 0xbff0000000000000),
        ("log1p", 0x3ff0000000000000, 0x3fe62e42fefa39ef),
        ("log1p", 0xbff0000000000000, 0xfff0000000000000),
        ("log1p", 0x3fe6666666666666, 0x3fe0fae81914a991),
        ("log1p", 0x431160ac8f9e32ee, 0x40415eb6d1f7fd9e),
        ("log1p", 0xebe43442c0e41c74, 0x7ff8000000000000),
        ("cbrt", 0x3ff0000000000000, 0x3ff0000000000000),
        ("cbrt", 0xbff0000000000000, 0xbff0000000000000),
        ("cbrt", 0x3fe6666666666666, 0x3fec69b5a72f1a99),
        ("cbrt", 0x431160ac8f9e32ee, 0x40fa1b7e2f7d1ec5),
        ("cbrt", 0xebe43442c0e41c74, 0xce95c9fa457bd1a0),
        ("log10", 0x3ff0000000000000, 0x0000000000000000),
        ("log10", 0xbff0000000000000, 0x7ff8000000000000),
        ("log10", 0x3fe6666666666666, 0xbfc3d3d3d21ccf04),
        ("log10", 0x431160ac8f9e32ee, 0x402e2cbbd0ee6e10),
        ("log10", 0xebe43442c0e41c74, 0x7ff8000000000000),
        ("asinh", 0x3ff0000000000000, 0x3fec34366179d427),
        ("asinh", 0xbff0000000000000, 0xbfec34366179d427),
        ("asinh", 0x3fe6666666666666, 0x3fe4e2a4fe9085dd),
        ("asinh", 0x431160ac8f9e32ee, 0x4041b76fddf3e686),
        ("asinh", 0xebe43442c0e41c74, 0xc07e8357b6507b85),
        ("acosh", 0x3ff0000000000000, 0x0000000000000000),
        ("acosh", 0xbff0000000000000, 0x7ff8000000000000),
        ("acosh", 0x3fe6666666666666, 0x7ff8000000000000),
        ("acosh", 0x431160ac8f9e32ee, 0x4041b76fddf3e686),
        ("acosh", 0xebe43442c0e41c74, 0x7ff8000000000000),
        ("atanh", 0x3ff0000000000000, 0x7ff0000000000000),
        ("atanh", 0xbff0000000000000, 0xfff0000000000000),
        ("atanh", 0x3fe6666666666666, 0x3febc0ed0947fbe8),
        ("atanh", 0x431160ac8f9e32ee, 0x7ff8000000000000),
        ("atanh", 0xebe43442c0e41c74, 0x7ff8000000000000),
    ];

    #[test]
    fn matches_bun_golden_cases() {
        for (name, input, expected) in CASES {
            let x = f64::from_bits(input);
            let got = match name {
                "sinh" => sinh(x),
                "cosh" => cosh(x),
                "tanh" => tanh(x),
                "expm1" => expm1(x),
                "log1p" => log1p(x),
                "cbrt" => cbrt(x),
                "log10" => log10(x),
                "asinh" => asinh(x),
                "acosh" => acosh(x),
                "atanh" => atanh(x),
                _ => unreachable!(),
            };
            let want = f64::from_bits(expected);
            let same = if want.is_nan() { got.is_nan() } else { got.to_bits() == expected };
            assert!(same, "{name}({input:#018x}) = {:#018x}, esperado {expected:#018x}", got.to_bits());
        }
    }
}
