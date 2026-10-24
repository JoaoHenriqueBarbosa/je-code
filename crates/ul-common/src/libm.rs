//! A libm do glibc 2.41 e o `Math.pow` do JavaScriptCore, para quem precisa do mesmo último bit que o
//! Debian e o bun entregam: o zjsc (`Math.exp`, `Math.log`, `Math.log2`, `Math.pow` e o operador `**`) e o
//! transpilador do bun no `wip/zbun` (a dobra de constante de `a ** b`, que no upstream chama o
//! `operationMathPow` do JSC pela FFI `Bun__JSC__operationMathPow`).
//!
//! - `exp_log_pow`: a família exp/log/pow (`e_exp.c`, `e_exp2.c`, `e_log.c`, `e_log2.c`, `e_pow.c`), só a
//!   variante FMA, que é a que o ifunc escolhe no x86_64 do bun;
//! - `data`: as tabelas dessa família;
//! - `words`: os acessos a palavras de `double` do `math_private.h` e o `fma`, que os demais arquivos da
//!   libm portados no zjsc (`glibc_hyper`, `glibc_trig`) também usam.

mod data;
mod exp_log_pow;
pub mod words;

pub use exp_log_pow::{exp, exp2, log, log2, pow};

/// `maxExponentForIntegerMathPow` (`MathCommon.h`).
pub const MAX_EXPONENT_FOR_INTEGER_MATH_POW: i32 = 1000;

/// `truncateDoubleToInt32` do WTF (`MathExtras.h`): o `_mm_cvttsd_si32` do x86_64, que devolve o "inteiro
/// indefinido" (`i32::MIN`) quando o resultado não cabe, NaN incluído. Não é o `ToInt32` do ECMAScript.
pub fn truncate_double_to_int32(number: f64) -> i32 {
    if number > -2147483649.0 && number < 2147483648.0 {
        number as i32
    } else {
        i32::MIN
    }
}

/// `operationMathPow` (`MathCommon.cpp`): o `Math.pow` e o `**` do JavaScriptCore, com os casos de NaN, ±∞ e
/// ±0 que o `pow` do C trata de outro jeito, a raiz quadrada para ±0,5 e a exponenciação rápida para expoente
/// inteiro pequeno. O resto vai ao `pow` do glibc (`mathPowInternal` no Linux x86_64).
pub fn operation_math_pow(x: f64, y: f64) -> f64 {
    if y.is_nan() {
        return f64::NAN;
    }
    let absolute_base = x.abs();
    if absolute_base == 1.0 && y.is_infinite() {
        return f64::NAN;
    }

    if y == 0.5 {
        if absolute_base == 0.0 {
            return 0.0;
        }
        if absolute_base == f64::INFINITY {
            return f64::INFINITY;
        }
        return x.sqrt();
    }

    if y == -0.5 {
        if absolute_base == 0.0 {
            return f64::INFINITY;
        }
        if absolute_base == f64::INFINITY {
            return 0.0;
        }
        return 1.0 / x.sqrt();
    }

    let mut y_as_int = truncate_double_to_int32(y);
    if (y_as_int as f64) == y && y_as_int >= 0 && y_as_int <= MAX_EXPONENT_FOR_INTEGER_MATH_POW {
        // If the exponent is a small positive int32 integer, we do a fast exponentiation
        let mut result = 1.0;
        let mut xd = x;
        while y_as_int != 0 {
            if y_as_int & 1 != 0 {
                result *= xd;
            }
            xd *= xd;
            y_as_int >>= 1;
        }
        return result;
    }
    pow(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_errors_have_the_nan_bits_of_the_bun() {
        // Medido no bun 1.4.2 com valores de tempo de execução: o `pow` de base negativa com expoente não inteiro dá o
        // NaN com sinal do x86 (`__math_invalid`), e `Math.log` e `Math.log2` de negativo dão o NaN positivo.
        assert_eq!(pow(-8.0, 1.5).to_bits(), 0xfff8_0000_0000_0000);
        assert_eq!(log(-1.0).to_bits(), 0x7ff8_0000_0000_0000);
        assert_eq!(log2(-8.0).to_bits(), 0x7ff8_0000_0000_0000);
    }

    #[test]
    fn operation_math_pow_follows_javascriptcore() {
        assert_eq!(operation_math_pow(2.0, 10.0), 1024.0);
        assert!(operation_math_pow(1.0, f64::INFINITY).is_nan());
        assert!(operation_math_pow(-1.0, f64::NEG_INFINITY).is_nan());
        assert!(operation_math_pow(2.0, f64::NAN).is_nan());
        assert_eq!(operation_math_pow(f64::NAN, 0.0), 1.0);
        assert_eq!(operation_math_pow(4.0, 0.5), 2.0);
        assert_eq!(operation_math_pow(-0.0, 0.5), 0.0);
        assert_eq!(operation_math_pow(f64::NEG_INFINITY, 0.5), f64::INFINITY);
        assert_eq!(operation_math_pow(0.0, -0.5), f64::INFINITY);
        assert_eq!(operation_math_pow(2.0, -1.0), 0.5);
        assert_eq!(operation_math_pow(10.0, 2.5), pow(10.0, 2.5));
    }

    #[test]
    fn truncation_is_the_x86_64_one() {
        assert_eq!(truncate_double_to_int32(-2.9), -2);
        assert_eq!(truncate_double_to_int32(f64::NAN), i32::MIN);
        assert_eq!(truncate_double_to_int32(2147483648.0), i32::MIN);
        assert_eq!(truncate_double_to_int32(-2147483648.9), i32::MIN);
    }
}
