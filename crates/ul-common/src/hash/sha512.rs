//! SHA-512, SHA-384, SHA-512/224 e SHA-512/256 (FIPS 180-4), incrementais.
//!
//! As constantes (os vetores iniciais e as 80 constantes de rodada) são derivadas na primeira chamada das
//! raízes quadradas e cúbicas dos primeiros primos, com aritmética inteira exata, em vez de digitadas. O
//! mesmo vetor inicial do SHA-512 é o do BLAKE2b, e o `blake2` o pede daqui.

use std::cmp::Ordering;
use std::sync::OnceLock;

use super::Blocks;

fn primes(count: usize) -> Vec<u64> {
    let mut out: Vec<u64> = Vec::new();
    let mut n = 2u64;
    while out.len() < count {
        if out.iter().all(|p| n % p != 0) {
            out.push(n);
        }
        n += 1;
    }
    out
}

fn big_mul(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut out = vec![0u32; a.len() + b.len()];
    for (i, &x) in a.iter().enumerate() {
        let mut carry = 0u64;
        for (j, &y) in b.iter().enumerate() {
            let cur = u64::from(out[i + j]) + u64::from(x) * u64::from(y) + carry;
            out[i + j] = cur as u32;
            carry = cur >> 32;
        }
        out[i + b.len()] = carry as u32;
    }
    out
}

fn big_cmp(a: &[u32], b: &[u32]) -> Ordering {
    let trim = |x: &[u32]| {
        let mut n = x.len();
        while n > 0 && x[n - 1] == 0 {
            n -= 1;
        }
        n
    };
    let (la, lb) = (trim(a), trim(b));
    if la != lb {
        return la.cmp(&lb);
    }
    for i in (0..la).rev() {
        if a[i] != b[i] {
            return a[i].cmp(&b[i]);
        }
    }
    Ordering::Equal
}

fn limbs(x: u128) -> Vec<u32> {
    vec![x as u32, (x >> 32) as u32, (x >> 64) as u32, (x >> 96) as u32]
}

/// Maior `x` com `x^n <= p * 2^(64 n)`: a raiz `n`-ésima de `p` com 64 bits fracionários.
fn iroot(p: u64, n: u32) -> u128 {
    let mut target = vec![0u32; (2 * n) as usize];
    target.push(p as u32);
    let power = |x: u128| -> Vec<u32> {
        let l = limbs(x);
        let mut acc = l.clone();
        for _ in 1..n {
            acc = big_mul(&acc, &l);
        }
        acc
    };
    let guess = ((p as f64).powf(1.0 / f64::from(n)) * 18_446_744_073_709_551_616.0) as u128;
    let margin: u128 = 1 << 24;
    let mut lo = guess.saturating_sub(margin);
    let mut hi = guess + margin;
    // Invariante: lo^n <= alvo < hi^n.
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if big_cmp(&power(mid), &target) != Ordering::Greater {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

fn frac_root(p: u64, n: u32) -> u64 {
    (iroot(p, n) & u128::from(u64::MAX)) as u64
}

/// Parte fracionária das raízes quadradas dos 16 primeiros primos (64 bits): os vetores iniciais do SHA-512
/// (os oito primeiros) e do SHA-384 (os oito seguintes).
pub(super) fn sqrt_consts() -> &'static [u64; 16] {
    static CELL: OnceLock<[u64; 16]> = OnceLock::new();
    CELL.get_or_init(|| {
        let ps = primes(16);
        let mut out = [0u64; 16];
        for (i, p) in ps.iter().enumerate() {
            out[i] = frac_root(*p, 2);
        }
        out
    })
}

/// Parte fracionária das raízes cúbicas dos 80 primeiros primos (64 bits).
fn cbrt_consts() -> &'static [u64; 80] {
    static CELL: OnceLock<[u64; 80]> = OnceLock::new();
    CELL.get_or_init(|| {
        let ps = primes(80);
        let mut out = [0u64; 80];
        for (i, p) in ps.iter().enumerate() {
            out[i] = frac_root(*p, 3);
        }
        out
    })
}

fn compress(state: &mut [u64; 8], block: &[u8; 128]) {
    let kc = cbrt_consts();
    let mut w = [0u64; 80];
    for i in 0..16 {
        let mut b = [0u8; 8];
        b.copy_from_slice(&block[8 * i..8 * i + 8]);
        w[i] = u64::from_be_bytes(b);
    }
    for i in 16..80 {
        let s0 = w[i - 15].rotate_right(1) ^ w[i - 15].rotate_right(8) ^ (w[i - 15] >> 7);
        let s1 = w[i - 2].rotate_right(19) ^ w[i - 2].rotate_right(61) ^ (w[i - 2] >> 6);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
    }
    let mut v = *state;
    for i in 0..80 {
        let s1 = v[4].rotate_right(14) ^ v[4].rotate_right(18) ^ v[4].rotate_right(41);
        let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
        let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(kc[i]).wrapping_add(w[i]);
        let s0 = v[0].rotate_right(28) ^ v[0].rotate_right(34) ^ v[0].rotate_right(39);
        let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
        let t2 = s0.wrapping_add(maj);
        v[7] = v[6];
        v[6] = v[5];
        v[5] = v[4];
        v[4] = v[3].wrapping_add(t1);
        v[3] = v[2];
        v[2] = v[1];
        v[1] = v[0];
        v[0] = t1.wrapping_add(t2);
    }
    for (s, x) in state.iter_mut().zip(v) {
        *s = s.wrapping_add(x);
    }
}

/// A família SHA-512 incremental; `out_len` é o tamanho do resumo em bytes (64, 48, 28 ou 32).
#[derive(Clone)]
pub struct Sha512 {
    state: [u64; 8],
    blocks: Blocks<128>,
    out_len: usize,
}

impl Sha512 {
    fn with_state(state: [u64; 8], out_len: usize) -> Sha512 {
        Sha512 { state, blocks: Blocks::new(), out_len }
    }

    pub fn new() -> Sha512 {
        let mut init = [0u64; 8];
        init.copy_from_slice(&sqrt_consts()[..8]);
        Sha512::with_state(init, 64)
    }

    pub fn new_384() -> Sha512 {
        let mut init = [0u64; 8];
        init.copy_from_slice(&sqrt_consts()[8..]);
        Sha512::with_state(init, 48)
    }

    /// SHA-512/t (`t` é 224 ou 256): o vetor inicial é o SHA-512 do texto `SHA-512/t`, calculado a partir do
    /// vetor do SHA-512 com cada palavra somada em XOR a `0xa5a5a5a5a5a5a5a5` (FIPS 180-4, 5.3.6).
    pub fn new_t(t: usize) -> Sha512 {
        let mut seed_state = [0u64; 8];
        seed_state.copy_from_slice(&sqrt_consts()[..8]);
        for w in seed_state.iter_mut() {
            *w ^= 0xa5a5_a5a5_a5a5_a5a5;
        }
        let mut seed = Sha512::with_state(seed_state, 64);
        seed.update(format!("SHA-512/{t}").as_bytes());
        let digest = seed.finalize();
        let mut init = [0u64; 8];
        for (i, w) in init.iter_mut().enumerate() {
            let mut b = [0u8; 8];
            b.copy_from_slice(&digest[8 * i..8 * i + 8]);
            *w = u64::from_be_bytes(b);
        }
        Sha512::with_state(init, t / 8)
    }

    pub fn update(&mut self, data: &[u8]) {
        let state = &mut self.state;
        self.blocks.feed(data, &mut |b: &[u8; 128]| compress(state, b));
    }

    pub fn finalize(mut self) -> Vec<u8> {
        let state = &mut self.state;
        self.blocks.finish(true, &mut |b: &[u8; 128]| compress(state, b));
        let mut out: Vec<u8> = self.state.iter().flat_map(|w| w.to_be_bytes()).collect();
        out.truncate(self.out_len);
        out
    }
}

impl Default for Sha512 {
    fn default() -> Sha512 {
        Sha512::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::hex_lower;

    fn one_shot(mut h: Sha512, data: &[u8]) -> String {
        h.update(data);
        hex_lower(&h.finalize())
    }

    #[test]
    fn family_vectors() {
        assert_eq!(
            one_shot(Sha512::new(), b"abc"),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        assert_eq!(
            one_shot(Sha512::new_384(), b"abc"),
            "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"
        );
        assert_eq!(one_shot(Sha512::new_t(224), b"abc"), "4634270f707b6a54daae7530460842e20e37ed265ceee9a43e8924aa");
        assert_eq!(one_shot(Sha512::new_t(256), b"abc"), "53048e2681941ef99b2e29b76b4c7dabe4c2d0c634fc6d46e0e2f13107e7af23");
    }

    #[test]
    fn incremental_matches_one_shot() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 11 + 5) as u8).collect();
        let whole = one_shot(Sha512::new(), &data);
        for step in [1usize, 7, 111, 112, 127, 128, 129, 999] {
            let mut h = Sha512::new();
            for chunk in data.chunks(step) {
                h.update(chunk);
            }
            assert_eq!(hex_lower(&h.finalize()), whole, "passo {step}");
        }
    }
}
