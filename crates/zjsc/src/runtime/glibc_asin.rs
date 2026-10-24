//! Porte de `asin` e `acos` do glibc 2.41 (`sysdeps/ieee754/dbl-64/e_asin.c`, `uasncs.h`, `asincos.tbl`,
//! `root.tbl`, `powtwo.tbl`), sem `unsafe`.
//!
//! As tabelas `asncs` e `inroot` estão em `glibc_asin_table.rs`, geradas por
//! `scripts/gen-glibc-asin-table.py`. `powtwo[i]` é `2^i` exato e sai direto dos bits.
//!
//! Como em `glibc_trig` e `glibc_atan`, este é o caminho sem contração em FMA nos polinômios. Se o golden
//! divergir em um bit, o candidato é a variante `__ieee754_asin_fma`/`__ieee754_acos_fma` do ifunc.

use super::glibc_asin_table::{ASNCS, INROOT};

const F1: f64 = 1.66666666666664110590506577996662E-01;
const F2: f64 = 7.50000000026122686814431784722623E-02;
const F3: f64 = 4.46428561421059750978517350006940E-02;
const F4: f64 = 3.03821268582119319911193410625235E-02;
const F5: f64 = 2.23551211026525610742786300334557E-02;
const F6: f64 = 1.81382903404565056280372531963613E-02;

const HP0: f64 = f64::from_bits(0x3ff921fb54442d18);
const HP1: f64 = f64::from_bits(0x3c91a62633145c07);
const T24: f64 = 16777216.0;
const T27: f64 = 134217728.0;
const RT0: f64 = 9.99999999859990725855365213134618E-01;
const RT1: f64 = 4.99999999495955425917856814202739E-01;
const RT2: f64 = 3.75017500867345182581453026130850E-01;
const RT3: f64 = 3.12523626554518656309172508769531E-01;

fn asncs(n: usize) -> f64 {
    f64::from_bits(ASNCS[n])
}

/// `(((((f6*x2 + f5)*x2 + f4)*x2 + f3)*x2 + f2)*x2 + f1)`, o polinômio de Taylor de arcsin em `v`.
fn taylor(v: f64) -> f64 {
    ((((F6 * v + F5) * v + F4) * v + F3) * v + F2) * v + F1
}

/// Cabeça de `k` (palavra alta sem sinal) para o índice `n` da tabela e o número de coeficientes do
/// polinômio, nas cinco faixas de tabela (0,125 a 0,96875). `k` já está limitado a `0x3fc00000..0x3fef0000`.
fn table_range(k: i32) -> (usize, usize) {
    let kk = k as usize;
    if k < 0x3fe00000 {
        if k < 0x3fd00000 {
            (11 * ((kk & 0x000fffff) >> 15), 5)
        } else {
            (11 * ((kk & 0x000fffff) >> 14) + 352, 5)
        }
    } else if k < 0x3fe80000 {
        (1056 + ((kk & 0x000fe000) >> 11) * 3, 6)
    } else if k < 0x3fed8000 {
        (992 + ((kk & 0x000fe000) >> 13) * 13, 7)
    } else if k < 0x3fee8000 {
        (884 + ((kk & 0x000fe000) >> 13) * 14, 8)
    } else {
        (768 + ((kk & 0x000fe000) >> 13) * 15, 9)
    }
}

/// Parte comum das cinco faixas de tabela: devolve `(t, base)`, com `t` o polinômio já somado e `base`
/// o valor tabelado de arcsin no ponto `x0` (`asncs[n + 3 + a]`). `positive` é `m > 0` do C.
fn table_poly(x: f64, positive: bool, n: usize, a: usize) -> (f64, f64) {
    let xx = if positive { x - asncs(n) } else { -x - asncs(n) };
    let mut t = asncs(n + 1) * xx;
    // xx*(c[n+2] + xx*(c[n+3] + ... + xx*c[n+1+a])): Horner de dentro para fora.
    let mut acc = asncs(n + 1 + a);
    for j in (n + 2..n + 1 + a).rev() {
        acc = asncs(j) + xx * acc;
    }
    let p = xx * xx * acc + asncs(n + 2 + a);
    t += p;
    (t, asncs(n + 3 + a))
}

/// Raiz de `z` por `inroot`/`powtwo` e as duas iterações de Newton, comum às duas funções:
/// devolve `(t, c)` com `c = t0*z` e `t` já refinado.
fn root_part(z: f64) -> (f64, f64) {
    let k = (z.to_bits() >> 32) as u32;
    let power = f64::from_bits(((1023 + 511 - (k >> 21)) as u64) << 52);
    let mut t = f64::from_bits(INROOT[((k & 0x001fffff) >> 14) as usize]) * power;
    let r = 1.0 - t * t * z;
    t *= RT0 + r * (RT1 + r * (RT2 + r * RT3));
    let c = t * z;
    t = c * (1.5 - 0.5 * t * c);
    (t, c)
}

/// `asin` do glibc (`__ieee754_asin`).
pub fn asin(x: f64) -> f64 {
    let bits = x.to_bits();
    let m = (bits >> 32) as u32 as i32;
    let k = 0x7fffffff & m;
    let positive = m > 0;
    let signed = |res: f64| if positive { res } else { -res };

    if k < 0x3e500000 {
        return x;
    }
    if k < 0x3fc00000 {
        let x2 = x * x;
        let t = taylor(x2) * (x2 * x);
        return x + t;
    }
    if k < 0x3fef0000 {
        let (n, a) = table_range(k);
        let (t, base) = table_poly(x, positive, n, a);
        return signed(base + t);
    }
    if k < 0x3ff00000 {
        let z = 0.5 * if positive { 1.0 - x } else { 1.0 + x };
        let (t, c) = root_part(z);
        let y = (c + T24) - T24;
        let cc = (z - y * y) / (t + y);
        let p = taylor(z) * z;
        let cor = (HP1 - 2.0 * cc) - 2.0 * (y + cc) * p;
        let res1 = HP0 - 2.0 * y;
        return signed(res1 + cor);
    }
    if k == 0x3ff00000 && bits as u32 == 0 {
        return signed(HP0);
    }
    (x - x) / (x - x)
}

/// `acos` do glibc (`__ieee754_acos`).
pub fn acos(x: f64) -> f64 {
    let bits = x.to_bits();
    let m = (bits >> 32) as u32 as i32;
    let k = 0x7fffffff & m;
    let positive = m > 0;

    if k < 0x3c880000 {
        return HP0;
    }
    if k < 0x3fc00000 {
        let x2 = x * x;
        let t = taylor(x2) * (x2 * x);
        let r = HP0 - x;
        let cor = (((HP0 - r) - x) + HP1) - t;
        return r + cor;
    }
    if k < 0x3fef0000 {
        let (n, a) = table_range(k);
        let (t, base) = table_poly(x, positive, n, a);
        let y = if positive { HP0 - base } else { HP0 + base };
        let t = if positive { HP1 - t } else { HP1 + t };
        return y + t;
    }
    if k < 0x3ff00000 {
        let z = 0.5 * if positive { 1.0 - x } else { 1.0 + x };
        let (t, c) = root_part(z);
        let y = (T27 * c + c) - T27 * c;
        let cc = (z - y * y) / (t + y);
        let p = taylor(z) * z;
        let res = if positive {
            y + (cc + p * (y + cc))
        } else {
            let cor = (HP1 - cc) - (y + cc) * p;
            (HP0 - y) + cor
        };
        return res + res;
    }
    if k == 0x3ff00000 && bits as u32 == 0 {
        return if positive { 0.0 } else { 2.0 * HP0 };
    }
    (x - x) / (x - x)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Casos de `tests/golden/math_bun.tsv` (bun 1.4.2, glibc 2.41): (bits da entrada, bits do resultado).
    const ASIN_CASES: [(u64, u64); 21] = [
        (0x3e7ad7f29abcaf48, 0x3e7ad7f29abcaf55),
        (0x3ee4f8b588e368f1, 0x3ee4f8b588e4e940),
        (0x3f3d9d9fb8978ba2, 0x3f3d9d9fc980d323),
        (0x3fa38b33a548fa45, 0x3fa38c6ae32ad55d),
        (0x3faa101d84754526, 0x3faa13000b601649),
        (0x3fb999999999999a, 0x3fb9a49276037884),
        (0x3fd3333333333333, 0x3fd380159e14f6ff),
        (0x3fd5555555555555, 0x3fd5bfe34f051112),
        (0x3fdc48556c00d4fe, 0x3fdd4b7bfd1f9b95),
        (0x3fe0000000000000, 0x3fe0c152382d7366),
        (0x3fe492c27a63736d, 0x3fe6587510bfee61),
        (0x3fe6666666666666, 0x3fe8d00e692afd95),
        (0x3fe921fb54442d18, 0x3fece8276c3e139c),
        (0x3fed4c1ebc83a96d, 0x3ff28278af01b4ec),
        (0x3fefffffffffffff, 0x3ff921fb50442d18),
        (0x3ff0000000000000, 0x3ff921fb54442d18),
        (0xbf6f24f3d54f524e, 0xbf6f24f8c007b372),
        (0xbfb999999999999a, 0xbfb9a49276037884),
        (0xbfdd4adff822bbed, 0xbfde6cf83c9b32fc),
        (0xbfe0000000000000, 0xbfe0c152382d7366),
        (0xbff0000000000000, 0xbff921fb54442d18),
    ];

    const ACOS_CASES: [(u64, u64); 23] = [
        (0x3da3c18af7c6cb23, 0x3ff921fb54438f0c),
        (0x3dc9b5ab64e6d2b5, 0x3ff921fb5440f663),
        (0x3e7ad7f29abcaf48, 0x3ff921fb396c3a7e),
        (0x3ee4f8b588e368f1, 0x3ff921f0d7e968a6),
        (0x3f3d9d9fb8978ba2, 0x3ff920217a47950b),
        (0x3fa38b33a548fa45, 0x3ff88597fd2ad66d),
        (0x3faa101d84754526, 0x3ff8516353e92c66),
        (0x3fb999999999999a, 0x3ff787b22ce3f590),
        (0x3fd3333333333333, 0x3ff441f5ecbeef59),
        (0x3fd5555555555555, 0x3ff3b2028082e8d4),
        (0x3fdc48556c00d4fe, 0x3ff1cf1c54fc4633),
        (0x3fe0000000000000, 0x3ff0c152382d7366),
        (0x3fe492c27a63736d, 0x3febeb8197c86bcf),
        (0x3fe6666666666666, 0x3fe973e83f5d5c9b),
        (0x3fe921fb54442d18, 0x3fe55bcf3c4a4694),
        (0x3fed4c1ebc83a96d, 0x3fda7e0a9509e0b2),
        (0x3fefffffffffffff, 0x3e50000000000000),
        (0x3ff0000000000000, 0x0000000000000000),
        (0xbf6f24f3d54f524e, 0x3ff9318dd0a430f2),
        (0xbfb999999999999a, 0x3ffabc447ba464a1),
        (0xbfdd4adff822bbed, 0x40005e9cb1b57cec),
        (0xbfe0000000000000, 0x4000c152382d7366),
        (0xbff0000000000000, 0x400921fb54442d18),
    ];

    #[test]
    fn asin_matches_glibc_golden() {
        for (input, expected) in ASIN_CASES {
            let got = asin(f64::from_bits(input));
            assert_eq!(got.to_bits(), expected, "asin({input:#018x})");
        }
    }

    #[test]
    fn acos_matches_glibc_golden() {
        for (input, expected) in ACOS_CASES {
            let got = acos(f64::from_bits(input));
            assert_eq!(got.to_bits(), expected, "acos({input:#018x})");
        }
    }

    #[test]
    fn asin_acos_edge_cases() {
        assert_eq!(asin(0.0).to_bits(), 0);
        assert_eq!(asin(-0.0).to_bits(), (-0.0f64).to_bits());
        assert_eq!(acos(0.0).to_bits(), HP0.to_bits());
        assert!(asin(2.0).is_nan());
        assert!(acos(-2.0).is_nan());
        assert!(asin(f64::NAN).is_nan());
        assert!(acos(f64::INFINITY).is_nan());
    }
}
