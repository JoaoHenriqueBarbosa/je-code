//! Aleatoriedade para o que no VS Code vem de `Math.random` e de `crypto.getRandomValues`.
//!
//! No alvo nativo a semente e os bytes vêm do sistema operacional (`getrandom`). No alvo web
//! (wasm32-unknown-unknown) a crate `getrandom` exige a ponte do navegador; até ela existir o
//! porte usa as chaves aleatórias do `RandomState` do std, que nesse alvo NÃO são aleatórias de
//! verdade (o std não tem fonte de entropia lá). Registrado como pendência da Fase web.

use std::cell::Cell;

/// Preenche `buf` com bytes aleatórios do sistema (`crypto.getRandomValues`).
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn fill_bytes(buf: &mut [u8]) {
    getrandom::fill(buf).expect("o sistema operacional não forneceu bytes aleatórios");
}

/// Fallback do alvo web, ver o topo do módulo.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn fill_bytes(buf: &mut [u8]) {
    use std::collections::hash_map::RandomState;
    use std::hash::BuildHasher;
    let state = RandomState::new();
    for (i, chunk) in buf.chunks_mut(8).enumerate() {
        let word = state.hash_one(i).to_le_bytes();
        chunk.copy_from_slice(&word[..chunk.len()]);
    }
}

thread_local! {
    static STATE: Cell<[u64; 2]> = const { Cell::new([0, 0]) };
}

/// `Math.random()`: um número em `[0, 1)` com 53 bits, de um xorshift128+ semeado pelo sistema.
pub fn random_f64() -> f64 {
    STATE.with(|cell| {
        let mut s = cell.get();
        if s == [0, 0] {
            let mut seed = [0u8; 16];
            fill_bytes(&mut seed);
            s = [
                u64::from_le_bytes(seed[..8].try_into().expect("8 bytes")),
                u64::from_le_bytes(seed[8..].try_into().expect("8 bytes")),
            ];
            if s == [0, 0] {
                s = [0x9E37_79B9_7F4A_7C15, 0xBF58_476D_1CE4_E5B9];
            }
        }
        let mut s1 = s[0];
        let s0 = s[1];
        s[0] = s0;
        s1 ^= s1 << 23;
        s1 ^= s1 >> 17;
        s1 ^= s0 ^ (s0 >> 26);
        s[1] = s1;
        cell.set(s);
        let bits = s[1].wrapping_add(s0) >> 11;
        bits as f64 / (1u64 << 53) as f64
    })
}
