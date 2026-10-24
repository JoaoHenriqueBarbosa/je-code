//! Porte de `tan` do glibc 2.41 (`sysdeps/ieee754/dbl-64/s_tan.c`, `utan.h`, `utan.tbl`), sem `unsafe`.
//!
//! O bun usa a libm do glibc; só o porte próprio dá o mesmo resultado bit a bit. Na 2.41 o caminho lento
//! com `mpa` já não existe em `s_tan.c`: todo argumento termina num dos ramos abaixo. A redução de
//! argumento é a de `glibc_trig` (`reduce_sincos`, `branred`) e as operações de precisão dupla vêm de
//! `glibc_atan` (`emulv`, `eadd`); a tabela `xfg` está em `glibc_tan_table.rs`, GERADA POR SCRIPT
//! (`scripts/gen-glibc-tan-table.py`).
//!
//! Ressalva do FMA: sem contração nos polinômios (mesma dos demais portes de glibc deste diretório).

use super::glibc_atan::{eadd, emulv};
use super::glibc_tan_table::XFG;
use super::glibc_trig::{branred, reduce_sincos};

const D3: f64 = f64::from_bits(0x3FD5555555555555);
const D5: f64 = f64::from_bits(0x3FC11111111107C6);
const D7: f64 = f64::from_bits(0x3FABA1BA1CDB8745);
const D9: f64 = f64::from_bits(0x3F9664ED49CFC666);
const D11: f64 = f64::from_bits(0x3F82385A3CF2E4EA);
const E0: f64 = f64::from_bits(0x3FD5555555554DBD);
const E1: f64 = f64::from_bits(0x3FC11112E0A6B45F);
const MFFTNHF: f64 = -15.5;
const G1: f64 = f64::from_bits(0x3E4B096C00000000);
const G2: f64 = f64::from_bits(0x3FAF212D00000000);
const G3: f64 = f64::from_bits(0x3FE92F1A00000000);
const G4: f64 = 25.0;
const G5: f64 = f64::from_bits(0x4197D78400000000);
const GY2: f64 = G2;
const MP1: f64 = f64::from_bits(0x3FF921FB58000000);
const MP2: f64 = f64::from_bits(0xBE4DDE973C000000);
const MP3: f64 = f64::from_bits(0xBC8CB3B399D747F2);
const HPINV: f64 = f64::from_bits(0x3FE45F306DC9C883);
const TOINT: f64 = f64::from_bits(0x4338000000000000);

/// Linha `i` de `xfg`: `(xi, Fi, Gi)`.
fn xfg(i: i32) -> (f64, f64, f64) {
    let row = &XFG[i as usize];
    (f64::from_bits(row[0]), f64::from_bits(row[1]), f64::from_bits(row[2]))
}

/// Índice da tabela: `(int) (mfftnhf + 256 * w)`.
fn table_index(w: f64) -> i32 {
    (MFFTNHF + 256.0 * w) as i32
}

/// Polinômio de `d3..d11` em `a2` (casos VI, VIII e X).
fn poly_small(a2: f64) -> f64 {
    let mut t = D9 + a2 * D11;
    t = D7 + a2 * t;
    t = D5 + a2 * t;
    D3 + a2 * t
}

/// `DIV2` de `dla.h`: `(x + xx) / (y + yy)` em precisão dupla, devolve `(z, zz)`.
fn div2(x: f64, xx: f64, y: f64, yy: f64) -> (f64, f64) {
    let c = x / y;
    let (u, uu) = emulv(c, y);
    let cc = ((((x - u) - uu) + xx) - c * yy) / y;
    let z = c + cc;
    (z, (c - z) + cc)
}

/// Casos VI a XI: `x = n * pi/2 + (a + da)` já reduzido; `n` é o bit baixo do quociente.
fn tan_reduced(a: f64, da: f64, n: bool) -> f64 {
    let (ya, yya, sy) = if a < 0.0 { (-a, -da, -1.0) } else { (a, da, 1.0) };

    if ya <= GY2 {
        let a2 = a * a;
        let t2 = da + a * a2 * poly_small(a2);
        if n {
            // -cot
            let (b, db) = eadd(a, t2);
            let (c, dc) = div2(1.0, 0.0, b, db);
            return -(c + dc);
        }
        return a + t2;
    }

    let i = table_index(ya);
    let (xi, fi, gi) = xfg(i);
    let z = (ya - xi) + yya;
    let z2 = z * z;
    let pz = z + z * z2 * (E0 + z2 * E1);
    if n {
        // -cot
        let t2 = pz * (fi + gi) / (fi + pz);
        let y = gi - t2;
        -sy * y
    } else {
        let t2 = pz * (gi + fi) / (gi - pz);
        let y = fi + t2;
        sy * y
    }
}

/// `__tan` do glibc 2.41.
pub fn tan(x: f64) -> f64 {
    // x = +-INF, x = NaN
    if x.to_bits() >> 52 & 0x7ff == 0x7ff {
        return x - x;
    }

    let w = x.abs();

    // (I) |x| <= 1.259e-8
    if w <= G1 {
        return x;
    }

    // (II) 1.259e-8 < |x| <= 0.0608
    if w <= G2 {
        let x2 = x * x;
        let t2 = poly_small(x2) * (x * x2);
        return x + t2;
    }

    // (III) 0.0608 < |x| <= 0.787
    if w <= G3 {
        let i = table_index(w);
        let (xi, fi, gi) = xfg(i);
        let z = w - xi;
        let z2 = z * z;
        let s = if x < 0.0 { -1.0 } else { 1.0 };
        let pz = z + z * z2 * (E0 + z2 * E1);
        let t2 = pz * (gi + fi) / (gi - pz);
        return s * (fi + t2);
    }

    // 0.787 < |x| <= 25: redução pelo algoritmo i
    if w <= G4 {
        let t = x * HPINV + TOINT;
        let xn = t - TOINT;
        let t1 = (x - xn * MP1) - xn * MP2;
        let n = t.to_bits() as u32 & 1 != 0;
        let da = xn * MP3;
        let a = t1 - da;
        let da = (t1 - a) - da;
        return tan_reduced(a, da, n);
    }

    // 25 < |x| <= 1e8: redução pelo algoritmo ii (a mesma de `sin`/`cos`, seguida de EADD)
    if w <= G5 {
        let (n, a, da) = reduce_sincos(x);
        let (a, da) = eadd(a, da);
        return tan_reduced(a, da, n & 1 != 0);
    }

    // 1e8 < |x| < 2^1024: redução pelo algoritmo iii
    let (n, a, da) = branred(x);
    let (a, da) = eadd(a, da);
    tan_reduced(a, da, n & 1 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pares `(entrada, esperado)` em bits, tirados de `tests/golden/math_bun.tsv` (linhas `tan`).
    const TAN_CASES: [(u64, u64); 24] = [
        (0x0000000000000000, 0x0000000000000000),
        (0x8000000000000000, 0x8000000000000000),
        (0x3ff0000000000000, 0x3ff8eb245cbee3a6),
        (0xc000000000000000, 0x40017af62e0950f8),
        (0x3ff921fb54442d18, 0x434d02967c31cdb5),
        (0x7fefffffffffffff, 0xbf74530cfe729484),
        (0x4086300000000000, 0x3f0f9bd03136287d),
        (0x41e0000000000000, 0xc010564ff9979ed5),
        (0x412e848000000000, 0xbfd7e9768ab734c0),
        (0x400921fb54442d18, 0xbca1a62633145c07),
        (0x64e1b3ac00174626, 0xc00308683116107b),
        (0x12bbe422a9cdf49d, 0x12bbe422a9cdf49d),
        (0x52de38ed2db9938c, 0xbff276ea8ecc608b),
        (0xe6a8e2b9a5084706, 0xc000913922ff23ca),
        (0x475746435c71400c, 0xbfdf707ebcd679dd),
        (0x431160ac8f9e32ee, 0x3fe572310dd96264),
        (0x5bfafd668e91d6ed, 0x3fed1e00688b1f7a),
        (0x44c0ebba6f476b41, 0x3fe91b9066355183),
        (0xf7d03d5fff0f0922, 0xbfe432cedba1996c),
        (0xc089924bc6a7ef9e, 0xc0245dd805c3f320),
        (0x4128de7a00000000, 0xc00b343d2dd2c4e1),
        (0x409b46d70a3d70a4, 0xbff9ca3f10acf967),
        (0x40f2de9333333333, 0xbfd131bee6a9cefe),
        (0x4057fd42c3c9eecc, 0xc01cab81cd48d629),
    ];

    #[test]
    fn tan_matches_glibc_golden() {
        for (input, expected) in TAN_CASES {
            let got = tan(f64::from_bits(input));
            assert_eq!(got.to_bits(), expected, "tan({input:#018x})");
        }
    }

    #[test]
    fn tan_special_values() {
        assert!(tan(f64::INFINITY).is_nan());
        assert!(tan(f64::NEG_INFINITY).is_nan());
        assert!(tan(f64::NAN).is_nan());
        assert_eq!(tan(-0.0).to_bits(), (-0.0f64).to_bits());
    }
}
