//! Porte de `WTF/wtf/dragonbox/detail/div.h`: divisibilidade e divisão rápida por potências de 10.

use crate::wtf::dragonbox::detail::util::compute_power;
use crate::wtf::dragonbox::detail::wuint;

/// `divide_by_pow10_info<N>`: número mágico e deslocamento, só definidos para N = 1 e N = 2.
const fn divide_by_pow10_info(n: i32) -> (u32, i32) {
    match n {
        1 => (6554, 16),
        2 => (656, 16),
        _ => panic!("divide_by_pow10_info só existe para N = 1 e N = 2"),
    }
}

/// Troca `n` por `floor(n / 10^N)` e devolve se `n` era divisível por `10^N`.
/// Pré-condição: `n <= 10^(N+1)`. Recebe `n` por referência de entrada e saída, como o C++.
pub fn check_divisibility_and_divide_by_pow10(n_pow: i32, n: &mut u32) -> bool {
    debug_assert!(*n <= compute_power(n_pow + 1, 10) as u32);

    let (magic_number, shift_amount) = divide_by_pow10_info(n_pow);
    *n = n.wrapping_mul(magic_number);

    let mask: u32 = (1u32 << shift_amount) - 1;
    let result = (*n & mask) < magic_number;

    *n >>= shift_amount;
    result
}

/// `floor(n / 10^N)` para `n` e `N` pequenos. Pré-condição: `n <= 10^(N+1)`.
pub fn small_division_by_pow10(n_pow: i32, n: u32) -> u32 {
    debug_assert!(n <= compute_power(n_pow + 1, 10) as u32);

    let (magic_number, shift_amount) = divide_by_pow10_info(n_pow);
    n.wrapping_mul(magic_number) >> shift_amount
}

/// `divide_by_pow10<2, uint32_t, n_max>`: a especialização de 32 bits para divisão por 100.
pub fn divide_by_pow10_u32_by_100(n: u32) -> u32 {
    (wuint::umul64(n, 1374389535u32) >> 37) as u32
}

/// `divide_by_pow10<3, uint64_t, n_max>` com `n_max <= 15534100272597517998`: a especialização de
/// 64 bits para divisão por 1000.
pub fn divide_by_pow10_u64_by_1000(n: u64) -> u64 {
    wuint::umul128_upper64(n, 2361183241434822607u64) >> 7
}
