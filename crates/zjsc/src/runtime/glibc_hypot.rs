//! Porte de `sysdeps/ieee754/dbl-64/e_hypot.c` do glibc 2.41 (`__ieee754_hypot`), bit a bit com o
//! `hypot` de dois argumentos que o bun (glibc, x86_64) devolve.
//!
//! O build genérico do x86_64 não define `__FP_FAST_FMA`, então vale o `kernel` com correção de Borges
//! (o `#else` do C), não o `fma`. O `errno` e as exceções de ponto flutuante não existem aqui.

const SCALE: f64 = f64::from_bits(0x1a70_0000_0000_0000); // 0x1p-600
const LARGE_VAL: f64 = f64::from_bits(0x5fe0_0000_0000_0000); // 0x1p+511
const TINY_VAL: f64 = f64::from_bits(0x2340_0000_0000_0000); // 0x1p-459
const EPS: f64 = f64::from_bits(0x3c90_0000_0000_0000); // 0x1p-54

/// `kernel`: exige `ax >= ay >= 0` e que os quadrados de `ax`, `ay` e `ax - ay` não estourem nem
/// percam o expoente.
fn kernel(ax: f64, ay: f64) -> f64 {
    let mut h = (ax * ax + ay * ay).sqrt();
    let (t1, t2);
    if h <= 2.0 * ay {
        let delta = h - ay;
        t1 = ax * (2.0 * delta - ax);
        t2 = (delta - 2.0 * (ax - ay)) * delta;
    } else {
        let delta = h - ax;
        t1 = 2.0 * delta * (ax - 2.0 * ay);
        t2 = (4.0 * delta - ay) * ay + delta * delta;
    }
    h -= (t1 + t2) / (2.0 * h);
    h
}

/// `__hypot(x, y)` do glibc 2.41.
pub fn hypot(x: f64, y: f64) -> f64 {
    if !x.is_finite() || !y.is_finite() {
        if x.is_infinite() || y.is_infinite() {
            return f64::INFINITY;
        }
        return x + y;
    }

    let x = x.abs();
    let y = y.abs();
    let ax = if x < y { y } else { x };
    let ay = if x < y { x } else { y };

    // `ax` enorme: reduz os dois.
    if ax > LARGE_VAL {
        if ay <= ax * EPS {
            return ax + ay;
        }
        return kernel(ax * SCALE, ay * SCALE) / SCALE;
    }

    // `ay` minúsculo: amplia os dois.
    if ay < TINY_VAL {
        if ax >= ay / EPS {
            return ax + ay;
        }
        return kernel(ax / SCALE, ay / SCALE) * SCALE;
    }

    // Caso comum.
    if ay <= ax * EPS {
        return ax + ay;
    }
    kernel(ax, ay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specials() {
        assert_eq!(hypot(f64::NAN, f64::INFINITY), f64::INFINITY);
        assert!(hypot(f64::NAN, 1.0).is_nan());
        assert_eq!(hypot(-0.0, -0.0).to_bits(), 0);
        assert_eq!(hypot(3.0, -4.0), 5.0);
        assert_eq!(hypot(0.0, f64::from_bits(1)).to_bits(), 1);
    }

    #[test]
    fn matches_bun_golden() {
        let text = include_str!("../../tests/golden/math_bun.tsv");
        let mut checked = 0;
        for line in text.lines() {
            let mut fields = line.split('\t');
            if fields.next() != Some("hypot") {
                continue;
            }
            let (Some(args), Some(want)) = (fields.next(), fields.next()) else { continue };
            let parsed: Vec<u64> = args.split(',').filter_map(|h| u64::from_str_radix(h, 16).ok()).collect();
            if parsed.len() != 2 {
                continue;
            }
            let Ok(want) = u64::from_str_radix(want.trim(), 16) else { continue };
            let got = hypot(f64::from_bits(parsed[0]), f64::from_bits(parsed[1]));
            let nan_both = got.is_nan() && f64::from_bits(want).is_nan();
            assert!(nan_both || got.to_bits() == want, "hypot({args}) = {:016x}, bun {want:016x}", got.to_bits());
            checked += 1;
        }
        // O golden também tem `Math.hypot` variádico (0, 1 e 3 a 9 argumentos), que não passa pelo glibc
        // e fica de fora: só os 756 casos de dois argumentos são conferidos aqui.
        assert!(checked > 700, "só {checked} casos de hypot lidos");
    }
}
