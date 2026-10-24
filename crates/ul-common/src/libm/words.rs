//! Os acessos a palavras de um `double` que o libm do glibc usa em todo arquivo (`math_private.h`:
//! `GET_HIGH_WORD`, `GET_LOW_WORD`, `SET_HIGH_WORD`) e o `fma`, num lugar só para `exp_log_pow` e para os
//! `glibc_hyper` e `glibc_trig` do zjsc não terem cópia própria.

/// `GET_HIGH_WORD`.
#[inline]
pub fn high_word(x: f64) -> u32 {
    (x.to_bits() >> 32) as u32
}

/// `GET_LOW_WORD`.
#[inline]
pub fn low_word(x: f64) -> u32 {
    x.to_bits() as u32
}

/// `SET_HIGH_WORD`.
#[inline]
pub fn set_high_word(x: f64, high: u32) -> f64 {
    f64::from_bits(((high as u64) << 32) | (x.to_bits() & 0xffff_ffff))
}

/// `__builtin_fma`: o nome do C nas fórmulas portadas (a conta é `mul_add`, com um único arredondamento).
#[inline]
pub fn fma(a: f64, b: f64, c: f64) -> f64 {
    a.mul_add(b, c)
}

/// `__math_invalid` (`math_err.c`): o resultado de domínio inválido. A conta `(x - x) / (x - x)` no lugar de uma
/// constante é o que dá o NaN com sinal do x86 (`0xfff8000000000000`) quando `x` é finito, e preserva o NaN de
/// entrada quando `x` já é NaN. O JavaScript vê a diferença pelos bits num `Float64Array`.
#[inline]
pub fn math_invalid(x: f64) -> f64 {
    (x - x) / (x - x)
}
