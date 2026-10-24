//! Resumos MD4, MD5, SHA-1, SHA-2 (SHA-256, SHA-224 e a família SHA-512), SHA-3 e SHAKE, BLAKE2b e BLAKE2s,
//! RIPEMD-160 e SM3 em Rust puro, com a API de uma passada e a incremental (`new`, `update`, `finalize`),
//! mais HMAC, PBKDF2 e scrypt. [`Algo`] e [`Hasher`] escolhem o algoritmo pelo nome do OpenSSL.

use std::sync::OnceLock;

/// `update` e `finalize` dos algoritmos de Merkle-Damgård de bloco de 64 bytes cujo tipo guarda `state` (as
/// palavras) e `blocks` (o `Blocks<64>`): só mudam a função de compressão, a ordem dos bytes do comprimento
/// no preenchimento e a das palavras no resumo. Fica antes dos `mod` para os módulos filhos a enxergarem.
macro_rules! block_hash {
    ($ty:ty, $compress:path, length_big_endian: $length_be:expr, words_little_endian: $words_le:expr, out: $out:literal) => {
        impl $ty {
            pub fn update(&mut self, data: &[u8]) {
                let state = &mut self.state;
                self.blocks.feed(data, &mut |b: &[u8; 64]| $compress(state, b));
            }

            pub fn finalize(mut self) -> [u8; $out] {
                let state = &mut self.state;
                self.blocks.finish($length_be, &mut |b: &[u8; 64]| $compress(state, b));
                store_words(&self.state, $words_le)
            }
        }
    };
}

pub mod algo;
pub mod blake2;
pub mod kdf;
pub mod keccak;
pub mod rapidhash;
pub mod ripemd;
pub mod sha512;
pub mod sm3;
pub mod superfast;

pub use algo::{Algo, Hasher};
pub use kdf::{hmac, pbkdf2, scrypt, Hmac};

/// Acumula os bytes de entrada em blocos de `N` (64 ou 128) e cuida do preenchimento final (`0x80`, zeros
/// e o comprimento em bits, em `N / 8` bytes), que é o mesmo nos algoritmos de Merkle-Damgård; só a ordem
/// dos bytes do comprimento muda.
#[derive(Clone)]
struct Blocks<const N: usize> {
    buf: [u8; N],
    len: usize,
    total: u128,
}

impl<const N: usize> Blocks<N> {
    const fn new() -> Blocks<N> {
        Blocks { buf: [0; N], len: 0, total: 0 }
    }

    fn feed(&mut self, mut data: &[u8], compress: &mut impl FnMut(&[u8; N])) {
        self.total = self.total.wrapping_add(data.len() as u128);
        if self.len > 0 {
            let take = (N - self.len).min(data.len());
            self.buf[self.len..self.len + take].copy_from_slice(&data[..take]);
            self.len += take;
            data = &data[take..];
            if self.len == N {
                compress(&self.buf);
                self.len = 0;
            }
        }
        while let Some((block, rest)) = data.split_first_chunk::<N>() {
            compress(block);
            data = rest;
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.len = data.len();
        }
    }

    fn finish(&mut self, big_endian: bool, compress: &mut impl FnMut(&[u8; N])) {
        let length_bytes = N / 8;
        let bits = self.total.wrapping_mul(8);
        let mut pad = [0u8; N];
        pad[0] = 0x80;
        let limit = N - length_bytes;
        let pad_len = if self.len < limit { limit - self.len } else { N + limit - self.len };
        self.feed(&pad[..pad_len], &mut *compress);
        if big_endian {
            self.feed(&bits.to_be_bytes()[16 - length_bytes..], &mut *compress);
        } else {
            self.feed(&bits.to_le_bytes()[..length_bytes], &mut *compress);
        }
    }
}

/// As 16 palavras de 32 bits de um bloco.
fn load_words(block: &[u8; 64], little_endian: bool) -> [u32; 16] {
    std::array::from_fn(|i| {
        let w = [block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]];
        if little_endian { u32::from_le_bytes(w) } else { u32::from_be_bytes(w) }
    })
}

/// O estado final como bytes (`N` é 4 vezes o número de palavras usadas).
fn store_words<const N: usize>(words: &[u32], little_endian: bool) -> [u8; N] {
    let mut out = [0u8; N];
    for (chunk, w) in out.chunks_mut(4).zip(words) {
        chunk.copy_from_slice(&if little_endian { w.to_le_bytes() } else { w.to_be_bytes() });
    }
    out
}

/// `state[i] += v[i]` (a soma final de cada função de compressão).
fn add_words<const N: usize>(state: &mut [u32; N], v: [u32; N]) {
    for (s, w) in state.iter_mut().zip(v) {
        *s = s.wrapping_add(w);
    }
}

/// Um passo do MD4 e do MD5: `[a, b, c, d]` vira `[d, base + (a + f + addend) <<< shift, b, c]`, com `base` zero no MD4 e
/// `b` no MD5.
fn md_step([a, b, c, d]: [u32; 4], f: u32, addend: u32, shift: u32, base: u32) -> [u32; 4] {
    [d, base.wrapping_add(a.wrapping_add(f).wrapping_add(addend).rotate_left(shift)), b, c]
}

/// A compressão do MD5 (`MD5`, 64 passos) e do MD4 (48 passos): a mesma forma de passo, com a função da rodada, a
/// palavra da mensagem, a rotação e a constante de cada um.
fn md_compress<const MD5: bool>(state: &mut [u32; 4], block: &[u8; 64]) {
    let x = load_words(block, true);
    let k = md5_constants();
    let mut v = *state;
    for i in 0..if MD5 { 64 } else { 48 } {
        let [_, b, c, d] = v;
        let (f, word, shift, constant) = if MD5 {
            let (f, word) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            (f, word, MD5_SHIFTS[i], k[i])
        } else {
            match i / 16 {
                0 => ((b & c) | (!b & d), i, [3, 7, 11, 19][i % 4], 0),
                1 => ((b & c) | (b & d) | (c & d), MD4_ROUND2[i % 16], [3, 5, 9, 13][i % 4], 0x5A82_7999),
                _ => (b ^ c ^ d, MD4_ROUND3[i % 16], [3, 9, 11, 15][i % 4], 0x6ED9_EBA1),
            }
        };
        v = md_step(v, f, x[word].wrapping_add(constant), shift, if MD5 { b } else { 0 });
    }
    add_words(state, v);
}

macro_rules! default_via_new {
    ($t:ty) => {
        impl Default for $t {
            fn default() -> $t {
                <$t>::new()
            }
        }
    };
}

// ---------------------------------------------------------------------------------------------
// MD5
// ---------------------------------------------------------------------------------------------

const MD5_SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20,
    4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15,
    21,
];

/// `floor(abs(sin(i + 1)) * 2^32)`, as constantes do MD5.
fn md5_constants() -> &'static [u32; 64] {
    static K: OnceLock<[u32; 64]> = OnceLock::new();
    K.get_or_init(|| std::array::from_fn(|i| (((i + 1) as f64).sin().abs() * 4_294_967_296.0) as u32))
}

/// MD5 incremental.
#[derive(Clone)]
pub struct Md5 {
    state: [u32; 4],
    blocks: Blocks<64>,
}

impl Md5 {
    pub fn new() -> Md5 {
        Md5 { state: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476], blocks: Blocks::new() }
    }
}

block_hash!(Md5, md_compress::<true>, length_big_endian: false, words_little_endian: true, out: 16);

default_via_new!(Md5);

/// MD5 de um buffer.
pub fn md5(data: &[u8]) -> [u8; 16] {
    let mut h = Md5::new();
    h.update(data);
    h.finalize()
}

// ---------------------------------------------------------------------------------------------
// MD4
// ---------------------------------------------------------------------------------------------

/// A ordem das palavras da mensagem nas rodadas 2 e 3 do MD4 (a rodada 1 usa `0..16`).
const MD4_ROUND2: [usize; 16] = [0, 4, 8, 12, 1, 5, 9, 13, 2, 6, 10, 14, 3, 7, 11, 15];
const MD4_ROUND3: [usize; 16] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];

/// MD4 incremental.
#[derive(Clone)]
pub struct Md4 {
    state: [u32; 4],
    blocks: Blocks<64>,
}

impl Md4 {
    pub fn new() -> Md4 {
        Md4 { state: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476], blocks: Blocks::new() }
    }
}

block_hash!(Md4, md_compress::<false>, length_big_endian: false, words_little_endian: true, out: 16);

default_via_new!(Md4);

// ---------------------------------------------------------------------------------------------
// SHA-1
// ---------------------------------------------------------------------------------------------

fn sha1_compress(state: &mut [u32; 5], block: &[u8; 64]) {
    let mut w = [0u32; 80];
    w[..16].copy_from_slice(&load_words(block, false));
    for i in 16..80 {
        w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *state;
    for (i, wi) in w.iter().enumerate() {
        let (f, k) = match i / 20 {
            0 => ((b & c) | (!b & d), 0x5A82_7999u32),
            1 => (b ^ c ^ d, 0x6ED9_EBA1u32),
            2 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDCu32),
            _ => (b ^ c ^ d, 0xCA62_C1D6u32),
        };
        let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(*wi);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = temp;
    }
    add_words(state, [a, b, c, d, e]);
}

/// SHA-1 incremental.
#[derive(Clone)]
pub struct Sha1 {
    state: [u32; 5],
    blocks: Blocks<64>,
}

impl Sha1 {
    pub fn new() -> Sha1 {
        Sha1 { state: [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0], blocks: Blocks::new() }
    }
}

block_hash!(Sha1, sha1_compress, length_big_endian: true, words_little_endian: false, out: 20);

default_via_new!(Sha1);

/// SHA-1 de um buffer.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h = Sha1::new();
    h.update(data);
    h.finalize()
}

// ---------------------------------------------------------------------------------------------
// SHA-256 e SHA-224
// ---------------------------------------------------------------------------------------------

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
    0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
    0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
    0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

const SHA256_INIT: [u32; 8] =
    [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];

const SHA224_INIT: [u32; 8] =
    [0xc1059ed8, 0x367cd507, 0x3070dd17, 0xf70e5939, 0xffc00b31, 0x68581511, 0x64f98fa7, 0xbefa4fa4];

fn sha256_compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    w[..16].copy_from_slice(&load_words(block, false));
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
    }
    let mut v = *state;
    for i in 0..64 {
        let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
        let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
        let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(SHA256_K[i]).wrapping_add(w[i]);
        let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
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
    add_words(state, v);
}

/// SHA-256 incremental. Com `new_224` é o SHA-224: o resumo são os 28 primeiros bytes do que
/// `finalize` devolve.
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    blocks: Blocks<64>,
}

impl Sha256 {
    pub fn new() -> Sha256 {
        Sha256::from_state(SHA256_INIT)
    }

    /// Estado inicial do SHA-224.
    pub fn new_224() -> Sha256 {
        Sha256::from_state(SHA224_INIT)
    }

    fn from_state(state: [u32; 8]) -> Sha256 {
        Sha256 { state, blocks: Blocks::new() }
    }
}

block_hash!(Sha256, sha256_compress, length_big_endian: true, words_little_endian: false, out: 32);

default_via_new!(Sha256);

/// SHA-256 de um buffer.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize()
}

/// SHA-224 de um buffer.
pub fn sha224(data: &[u8]) -> [u8; 28] {
    let mut h = Sha256::new_224();
    h.update(data);
    let full = h.finalize();
    let mut out = [0u8; 28];
    out.copy_from_slice(&full[..28]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::hex_lower;

    #[test]
    fn md5_vectors() {
        assert_eq!(hex_lower(&md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex_lower(&md5(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex_lower(&md5(b"message digest")), "f96b697d7cb7938d525a2f31aaf161d0");
    }

    #[test]
    fn md4_vectors() {
        let md4 = |data: &[u8]| {
            let mut h = Md4::new();
            h.update(data);
            hex_lower(&h.finalize())
        };
        assert_eq!(md4(b""), "31d6cfe0d16ae931b73c59d7e0c089c0");
        assert_eq!(md4(b"abc"), "a448017aaf21d8525fc10ae87aa6729d");
        assert_eq!(md4(b"message digest"), "d9130a8164549fe818874806e1c7014b");
        assert_eq!(md4(b"abcdefghijklmnopqrstuvwxyz"), "d79e1c308aa5bbcdeea8ed63df412da9");
    }

    #[test]
    fn sha1_vectors() {
        assert_eq!(hex_lower(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex_lower(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        // 56 bytes: o comprimento não cabe no mesmo bloco da mensagem.
        assert_eq!(
            hex_lower(&sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn sha2_vectors() {
        assert_eq!(hex_lower(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(hex_lower(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(
            hex_lower(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        assert_eq!(hex_lower(&sha224(b"abc")), "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7");
    }

    #[test]
    fn incremental_matches_one_shot_for_any_split() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7 + 3) as u8).collect();
        for step in [1usize, 3, 55, 56, 63, 64, 65, 128, 999] {
            let (mut m, mut s1, mut s2) = (Md5::new(), Sha1::new(), Sha256::new());
            for chunk in data.chunks(step) {
                m.update(chunk);
                s1.update(chunk);
                s2.update(chunk);
            }
            assert_eq!(m.finalize(), md5(&data), "md5 com passo {step}");
            assert_eq!(s1.finalize(), sha1(&data), "sha1 com passo {step}");
            assert_eq!(s2.finalize(), sha256(&data), "sha256 com passo {step}");
        }
    }

    #[test]
    fn million_a() {
        let mut h = Sha1::new();
        for _ in 0..10_000 {
            h.update(&[b'a'; 100]);
        }
        assert_eq!(hex_lower(&h.finalize()), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
    }
}
