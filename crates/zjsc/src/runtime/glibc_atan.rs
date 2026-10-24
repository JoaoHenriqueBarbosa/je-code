//! Porte de `atan` e `atan2` do glibc 2.41 (`sysdeps/ieee754/dbl-64/s_atan.c`, `e_atan2.c`, `atnat.h`,
//! `atnat2.h`, `uatan.tbl`), sem `unsafe`.
//!
//! A 2.41 não tem mais o caminho lento com `mpa` nestas duas funções: o resultado de cada ramo sai direto
//! dos polinômios, então o porte é completo. A tabela `cij` está em `glibc_atan_table.rs`, gerada por
//! `scripts/gen-glibc-atan-table.py`.
//!
//! Como em `glibc_trig`, este é o caminho sem contração em FMA nos polinômios; as transformações exatas
//! (`EMULV`, `EADD`, `ESUB` de `dla.h`) são exatas em qualquer variante. Se o golden divergir em um bit,
//! o candidato é a contração `a * b + c` do ifunc `__atan_fma`/`__ieee754_atan2_fma`.

use super::glibc_atan_table::CIJ;

const D3: f64 = f64::from_bits(0xbfd5555555555555);
const D5: f64 = f64::from_bits(0x3fc99999999997fd);
const D7: f64 = f64::from_bits(0xbfc24924923f7603);
const D9: f64 = f64::from_bits(0x3fbc71c6e5129a3b);
const D11: f64 = f64::from_bits(0xbfb7458022b13c25);
const D13: f64 = f64::from_bits(0x3fb375f08b31cbce);

const A: f64 = f64::from_bits(0x3e4bb67a00000000);
const B: f64 = f64::from_bits(0x3fb0000000000000);
const C: f64 = 1.0;
const D: f64 = 16.0;
const E: f64 = f64::from_bits(0x43349ff200000000);
const HPI: f64 = f64::from_bits(0x3ff921fb54442d18);
const MHPI: f64 = f64::from_bits(0xbff921fb54442d18);
const HPI1: f64 = f64::from_bits(0x3c91a62633145c07);
const OPI: f64 = f64::from_bits(0x400921fb54442d18);
const OPI1: f64 = f64::from_bits(0x3ca1a62633145c07);
const MOPI: f64 = f64::from_bits(0xc00921fb54442d18);
const QPI: f64 = f64::from_bits(0x3fe921fb54442d18);
const MQPI: f64 = f64::from_bits(0xbfe921fb54442d18);
const TQPI: f64 = f64::from_bits(0x4002d97c7f3321d2);
const MTQPI: f64 = f64::from_bits(0xc002d97c7f3321d2);
const TWO500: f64 = f64::from_bits(0x5f30000000000000);
const TWOM500: f64 = f64::from_bits(0x20b0000000000000);
const TWO52: f64 = 4503599627370496.0;
const EP: i32 = 59768832;
const EM: i32 = -59768832;

/// `EMULV` de `dla.h`: produto `z` e o erro exato `zz`.
pub(super) fn emulv(x: f64, y: f64) -> (f64, f64) {
    let z = x * y;
    (z, x.mul_add(y, -z))
}

/// `EADD` de `dla.h`.
pub(super) fn eadd(x: f64, y: f64) -> (f64, f64) {
    let z = x + y;
    (z, if x.abs() > y.abs() { (x - z) + y } else { (y - z) + x })
}

/// `ESUB` de `dla.h`.
pub(super) fn esub(x: f64, y: f64) -> (f64, f64) {
    let z = x - y;
    (z, if x.abs() > y.abs() { (x - z) - y } else { x - (y + z) })
}

/// Índice da linha de `cij`: `(TWO52 + 256 * u) - TWO52 - 16`.
fn row(u: f64) -> &'static [u64; 7] {
    let i = ((TWO52 + 256.0 * u) - TWO52) as i32 - 16;
    &CIJ[i as usize]
}

fn cij(row: &[u64; 7], j: usize) -> f64 {
    f64::from_bits(row[j])
}

/// Polinômio I de `d3..d13` em `v`, sem o fator externo.
fn poly_d(v: f64) -> f64 {
    let mut yy = D11 + v * D13;
    yy = D9 + v * yy;
    yy = D7 + v * yy;
    yy = D5 + v * yy;
    D3 + v * yy
}

/// `atan` do glibc (`__atan`).
pub fn atan(x: f64) -> f64 {
    if x.is_nan() {
        return x + x;
    }
    let u = x.abs();
    if u < C {
        if u < B {
            if u < A {
                return x;
            }
            let v = x * x;
            let mut yy = poly_d(v);
            yy *= x * v;
            return x + yy;
        }
        let r = row(u);
        let z = u - cij(r, 0);
        let mut yy = cij(r, 5) + z * cij(r, 6);
        yy = cij(r, 4) + z * yy;
        yy = cij(r, 3) + z * yy;
        yy = cij(r, 2) + z * yy;
        yy *= z;
        return (cij(r, 1) + yy).copysign(x);
    }
    if u < D {
        let w = 1.0 / u;
        let (t1, t2) = emulv(w, u);
        let ww = w * ((1.0 - t1) - t2);
        let r = row(w);
        let z = (w - cij(r, 0)) + ww;
        let mut yy = cij(r, 5) + z * cij(r, 6);
        yy = cij(r, 4) + z * yy;
        yy = cij(r, 3) + z * yy;
        yy = cij(r, 2) + z * yy;
        yy = HPI1 - z * yy;
        let t1 = HPI - cij(r, 1);
        return (t1 + yy).copysign(x);
    }
    if u < E {
        let w = 1.0 / u;
        let v = w * w;
        let (t1, t2) = emulv(w, u);
        let mut yy = poly_d(v);
        yy *= w * v;
        let ww = w * ((1.0 - t1) - t2);
        let (t3, cor) = esub(HPI, w);
        yy = ((HPI1 + cor) - ww) - yy;
        return (t3 + yy).copysign(x);
    }
    if x > 0.0 {
        HPI
    } else {
        MHPI
    }
}

/// `atan2` do glibc (`__ieee754_atan2`), com `y` primeiro como em C.
pub fn atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() {
        return x + y;
    }
    if y.is_nan() {
        return y + y;
    }
    let ux = (x.to_bits() >> 32) as u32;
    let uy = (y.to_bits() >> 32) as u32;

    // y = +-0
    if y == 0.0 {
        return match (y.is_sign_negative(), x.is_sign_negative()) {
            (false, false) => 0.0,
            (false, true) => OPI,
            (true, false) => -0.0,
            (true, true) => MOPI,
        };
    }
    // x = +-0
    if x == 0.0 {
        return if uy & 0x8000_0000 == 0 { HPI } else { MHPI };
    }
    // x = +-INF
    if x == f64::INFINITY {
        if y == f64::INFINITY {
            return QPI;
        }
        if y == f64::NEG_INFINITY {
            return MQPI;
        }
        return if uy & 0x8000_0000 == 0 { 0.0 } else { -0.0 };
    }
    if x == f64::NEG_INFINITY {
        if y == f64::INFINITY {
            return TQPI;
        }
        if y == f64::NEG_INFINITY {
            return MTQPI;
        }
        return if uy & 0x8000_0000 == 0 { OPI } else { MOPI };
    }
    // y = +-INF
    if y == f64::INFINITY {
        return HPI;
    }
    if y == f64::NEG_INFINITY {
        return MHPI;
    }

    // x/y ou y/x muito perto de zero
    let mut ax = x.abs();
    let mut ay = y.abs();
    let de = (uy & 0x7ff0_0000) as i32 - (ux & 0x7ff0_0000) as i32;
    if de >= EP {
        return if y > 0.0 { HPI } else { MHPI };
    } else if de <= EM {
        if x > 0.0 {
            return (ay / ax).copysign(y);
        }
        return if y > 0.0 { OPI } else { MOPI };
    }

    if ax < TWOM500 || ay < TWOM500 {
        ax *= TWO500;
        ay *= TWO500;
    }
    if ax > TWO500 || ay > TWO500 {
        ax *= TWOM500;
        ay *= TWOM500;
    }

    let (u, du) = if ay < ax {
        let u = ay / ax;
        let (v, vv) = emulv(ax, u);
        (u, ((ay - v) - vv) / ax)
    } else {
        let u = ax / ay;
        let (v, vv) = emulv(ay, u);
        (u, ((ax - v) - vv) / ay)
    };

    if x > 0.0 {
        if ay < ax {
            // (i) x > 0, |y| < |x|: atan(ay/ax)
            if u < B {
                let v = u * u;
                let zz = du + u * v * poly_d(v);
                return (u + zz).copysign(y);
            }
            let r = row(u);
            let t3 = u - cij(r, 0);
            let (v, dv) = eadd(t3, du);
            let t1 = cij(r, 1);
            let t2 = cij(r, 2);
            let zz = v * t2
                + (dv * t2
                    + v * v * (cij(r, 3) + v * (cij(r, 4) + v * (cij(r, 5) + v * cij(r, 6)))));
            return (t1 + zz).copysign(y);
        }
        // (ii) x > 0, |x| <= |y|: pi/2 - atan(ax/ay)
        if u < B {
            let v = u * u;
            let zz = u * v * poly_d(v);
            let (t2, cor) = esub(HPI, u);
            let t3 = ((HPI1 + cor) - du) - zz;
            return (t2 + t3).copysign(y);
        }
        let r = row(u);
        let v = (u - cij(r, 0)) + du;
        let zz = HPI1
            - v * (cij(r, 2) + v * (cij(r, 3) + v * (cij(r, 4) + v * (cij(r, 5) + v * cij(r, 6)))));
        let t1 = HPI - cij(r, 1);
        return (t1 + zz).copysign(y);
    }

    // (iii) x < 0, |x| < |y|: pi/2 + atan(ax/ay)
    if ax < ay {
        if u < B {
            let v = u * u;
            let zz = u * v * poly_d(v);
            let (t2, cor) = eadd(HPI, u);
            let t3 = ((HPI1 + cor) + du) + zz;
            return (t2 + t3).copysign(y);
        }
        let r = row(u);
        let v = (u - cij(r, 0)) + du;
        let zz = HPI1
            + v * (cij(r, 2) + v * (cij(r, 3) + v * (cij(r, 4) + v * (cij(r, 5) + v * cij(r, 6)))));
        let t1 = HPI + cij(r, 1);
        return (t1 + zz).copysign(y);
    }

    // (iv) x < 0, |y| <= |x|: pi - atan(ax/ay)
    if u < B {
        let v = u * u;
        let zz = u * v * poly_d(v);
        let (t2, cor) = esub(OPI, u);
        let t3 = ((OPI1 + cor) - du) - zz;
        return (t2 + t3).copysign(y);
    }
    let r = row(u);
    let v = (u - cij(r, 0)) + du;
    let zz = OPI1 - v * (cij(r, 2) + v * (cij(r, 3) + v * (cij(r, 4) + v * (cij(r, 5) + v * cij(r, 6)))));
    let t1 = OPI - cij(r, 1);
    (t1 + zz).copysign(y)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Casos de `tests/golden/math_bun.tsv` (bun 1.4.2, glibc 2.41): (bits da entrada, bits do resultado).
    const ATAN_CASES: [(u64, u64); 14] = [
        (0x3ff0000000000000, 0x3fe921fb54442d18),
        (0xbff0000000000000, 0xbfe921fb54442d18),
        (0x3fe0000000000000, 0x3fddac670561bb4f),
        (0xbfe0000000000000, 0xbfddac670561bb4f),
        (0x4000000000000000, 0x3ff1b6e192ebbe44),
        (0xc000000000000000, 0xbff1b6e192ebbe44),
        (0x0000000000000000, 0x0000000000000000),
        (0x8000000000000000, 0x8000000000000000),
        (0x0010000000000000, 0x0010000000000000),
        (0x41f0000000000000, 0x3ff921fb54342d18),
        (0x40c81cd6e631f8a1, 0x3ff921a664fe85a5),
        (0x40e6866333333333, 0x3ff921e4994c073a),
        (0x406f8ee147ae147b, 0x3ff911c1ffba463c),
        (0xe6cb9168083db87b, 0xbff921fb54442d18),
    ];

    // (bits de y, bits de x, bits do resultado).
    const ATAN2_CASES: [(u64, u64, u64); 20] = [
        (0x3ff0000000000000, 0x3ff0000000000000, 0x3fe921fb54442d18),
        (0x3ff0000000000000, 0xbff0000000000000, 0x4002d97c7f3321d2),
        (0x3ff0000000000000, 0x3fe0000000000000, 0x3ff1b6e192ebbe44),
        (0x3ff0000000000000, 0xbfe0000000000000, 0x4000468a8ace4df6),
        (0x3ff0000000000000, 0x4000000000000000, 0x3fddac670561bb4f),
        (0x3ff0000000000000, 0xc000000000000000, 0x40056c6e7397f5ae),
        (0x3ff0000000000000, 0x7fe1ccf385ebc8a0, 0x000730d67819e8d2),
        (0x3ff0000000000000, 0x4008000000000000, 0x3fd4978fa3269ee1),
        (0x3ff0000000000000, 0xc008000000000000, 0x40068f095fdf593c),
        (0xbff0000000000000, 0x3ff0000000000000, 0xbfe921fb54442d18),
        (0xbff0000000000000, 0xbff0000000000000, 0xc002d97c7f3321d2),
        (0xbff0000000000000, 0x3fe0000000000000, 0xbff1b6e192ebbe44),
        (0xbff0000000000000, 0xbfe0000000000000, 0xc000468a8ace4df6),
        (0xbff0000000000000, 0x4000000000000000, 0xbfddac670561bb4f),
        (0xbff0000000000000, 0xc000000000000000, 0xc0056c6e7397f5ae),
        (0xbff0000000000000, 0x7fe1ccf385ebc8a0, 0x800730d67819e8d2),
        (0x0000000000000000, 0x0000000000000000, 0x0000000000000000),
        (0x0000000000000000, 0x8000000000000000, 0x400921fb54442d18),
        (0x7ff0000000000000, 0x3ff0000000000000, 0x3ff921fb54442d18),
        (0x4e22606747feaf70, 0xda0184a3b827acc9, 0x400921fb54442d18),
    ];

    #[test]
    fn atan_matches_glibc_golden() {
        for (input, expected) in ATAN_CASES {
            let got = atan(f64::from_bits(input));
            assert_eq!(got.to_bits(), expected, "atan({input:#018x})");
        }
    }

    #[test]
    fn atan2_matches_glibc_golden() {
        for (y, x, expected) in ATAN2_CASES {
            let got = atan2(f64::from_bits(y), f64::from_bits(x));
            assert_eq!(got.to_bits(), expected, "atan2({y:#018x}, {x:#018x})");
        }
    }

    #[test]
    fn atan2_nan_propagates() {
        assert!(atan2(f64::NAN, 1.0).is_nan());
        assert!(atan2(1.0, f64::NAN).is_nan());
        assert!(atan(f64::NAN).is_nan());
    }
}
