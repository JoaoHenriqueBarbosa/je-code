//! Porte de `WTF/wtf/dragonbox/detail/util.h`: contas simples avaliáveis em tempo de compilação.

/// `compute_power<k>(a)`: `a` elevado a `k`, com a aritmética que o `Int` do C++ teria (aqui `u64`,
/// e o chamador converte para a largura que precisa).
pub const fn compute_power(k: i32, a: u64) -> u64 {
    debug_assert!(k >= 0);
    let mut p: u64 = 1;
    let mut i = 0;
    while i < k {
        p = p.wrapping_mul(a);
        i += 1;
    }
    p
}

/// `count_factors<a>(n)`: quantas vezes `a` divide `n`.
pub const fn count_factors(a: u64, mut n: u64) -> i32 {
    debug_assert!(a > 1);
    let mut c = 0;
    while n % a == 0 {
        n /= a;
        c += 1;
    }
    c
}
