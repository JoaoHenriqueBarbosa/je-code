//! Porte de `runtime/FractionToDouble.{h,cpp}`: `numerador / denominador` com arredondamento único para
//! `double`, onde o numerador é um `Int128` que pode passar de 2^53.
//!
//! As contas seguem Shewchuk (1997), "Adaptive precision floating-point arithmetic and fast robust geometric
//! predicates", e Hida, Li e Bailey (2008), "Library for double-double and quad-double arithmetic": um
//! número em precisão dupla-dupla é a soma não avaliada de dois `double` (o aproximado e o termo de erro).
//!
//! DIVERGÊNCIA: `Int128` é `i128`; `std::fma` é `f64::mul_add` (fusionado, sem arredondamento intermediário,
//! o que a `libm` e o `vfmsub213sd` também garantem).

use crate::runtime::math_common::is_safe_integer;

/// `using DD = std::array<double, 2>`: `[0]` é o termo aproximado, `[1]` o termo de erro.
type Dd = [f64; 2];

/// `int128ToDD(value)`: `hi` é a melhor aproximação em `double` e `lo` o erro.
fn int128_to_dd(value: i128) -> Dd {
    let hi = value as f64;
    let lo = (value - hi as i128) as f64;
    [hi, lo]
}

/// `ddSum(a, b)`: o Two-Sum do teorema 7 de Shewchuk.
fn dd_sum(a: f64, b: f64) -> Dd {
    let sum = a + b;
    let b_virtual = sum - a;
    let a_virtual = sum - b_virtual;
    let b_roundoff = b - b_virtual;
    let a_roundoff = a - a_virtual;
    [sum, a_roundoff + b_roundoff]
}

/// `ddProduct(a, b)`: o produto com o erro via `fma` (seção 2 de Hida, Li e Bailey).
fn dd_product(a: f64, b: f64) -> Dd {
    let product = a * b;
    let error = a.mul_add(b, -product);
    [product, error]
}

/// `fractionToDoubleSlow(const Int128&, double)`: a divisão dupla-dupla da seção 3.5 de Hida, Li e Bailey,
/// arredondada para `double`.
fn fraction_to_double_slow(numerator: i128, denominator: f64) -> f64 {
    let dd_numerator = int128_to_dd(numerator);

    // Primeira aproximação do quociente pela divisão comum.
    let quotient0 = dd_numerator[0] / denominator;

    // Resto: `ddNumerator - quotient0 * denominator`.
    let product = dd_product(quotient0, denominator);
    let remainder = dd_sum(dd_numerator[0], -product[0]);

    // O próximo termo da aproximação.
    let error = remainder[1] + dd_numerator[1] - product[1];
    let quotient1 = (remainder[0] + error) / denominator;

    // Sem a renormalização Fast-Two-Sum: basta precisão simples aqui, o termo de erro é descartado.
    quotient0 + quotient1
}

/// `fractionToDouble(const Int128& numerator, double denominator)`: o chamador garante
/// `isSafeInteger(denominator)` (todo comprimento de unidade da tabela 21 é menor que 2^53).
pub fn fraction_to_double_by_double(numerator: i128, denominator: f64) -> f64 {
    debug_assert!(denominator > 0.0);
    debug_assert!(is_safe_integer(denominator));

    if numerator == 0 {
        return 0.0;
    }

    // Com denominador 1 é só a aproximação em `double` do numerador.
    if denominator == 1.0 {
        return numerator as f64;
    }

    // Numerador exato em `double`: a conta vira uma divisão simples.
    if is_safe_integer(numerator as f64) {
        return numerator as f64 / denominator;
    }

    fraction_to_double_slow(numerator, denominator)
}

/// `fractionToDoubleSlow(const Int128&, const Int128&)`: a extensão da seção 3.5 para denominador `Int128`.
fn fraction_to_double_slow_wide(numerator: i128, denominator: i128) -> f64 {
    // `n0 + n1 = N` e `d0 + d1 = D`, exatos.
    let n = int128_to_dd(numerator);
    let d = int128_to_dd(denominator);

    // `q0`: primeira aproximação de `Q = N / D` (não dá para dividir `N / D` direto, ambos podem passar de
    // 2^53 como `Int128`).
    let q0 = n[0] / d[0];

    // Resíduo `N - q0 * D`, separado em `r0 + error`.
    let p = dd_product(q0, d[0]);
    let r = dd_sum(n[0], -p[0]);
    let error = r[1] + n[1] - p[1] - q0 * d[1];

    // `q1 = resíduo / D`, com `d0` no lugar de `D`: o erro dessa troca é da ordem de `Q * 2^-104`.
    let q1 = (r[0] + error) / d[0];

    q0 + q1
}

/// `fractionToDouble(const Int128& numerator, const Int128& denominator)`: para denominadores que passam de
/// 2^53 (a diferença de época-ns de um ano, por exemplo).
pub fn fraction_to_double(numerator: i128, denominator: i128) -> f64 {
    debug_assert!(denominator > 0);

    if numerator == 0 {
        return 0.0;
    }
    if denominator == 1 {
        return numerator as f64;
    }
    if is_safe_integer(numerator as f64) && is_safe_integer(denominator as f64) {
        return numerator as f64 / denominator as f64;
    }
    fraction_to_double_slow_wide(numerator, denominator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_numerators_divide_directly() {
        assert_eq!(fraction_to_double_by_double(90, 60.0), 1.5);
        assert_eq!(fraction_to_double_by_double(-90, 60.0), -1.5);
        assert_eq!(fraction_to_double_by_double(0, 60.0), 0.0);
        assert_eq!(fraction_to_double_by_double(7, 1.0), 7.0);
    }

    #[test]
    fn large_numerators_keep_the_error_term() {
        // (2^60 + 1) / 3600e9 não cabe em `double` exato, mas o resultado fica a menos de 1 ulp.
        let numerator = (1i128 << 60) + 1;
        let result = fraction_to_double_by_double(numerator, 3_600_000_000_000.0);
        let expected = (numerator as f64) / 3_600_000_000_000.0;
        assert!((result - expected).abs() <= expected * 2.0 * f64::EPSILON);
    }

    #[test]
    fn wide_denominator() {
        assert_eq!(fraction_to_double(3, 4), 0.75);
        let n = 3 * (1i128 << 70);
        let d = 1i128 << 71;
        assert_eq!(fraction_to_double(n, d), 1.5);
    }
}
