//! BLAKE2b e BLAKE2s (RFC 7693), incrementais e com o bloco de parâmetros inteiro: chave, sal, personalização
//! e os campos de árvore que o `_blake2` do CPython aceita.
//!
//! As duas variantes são a mesma rotina com outra largura de palavra, outras rotações e outro número de
//! rodadas; a macro `blake2_variant!` escreve cada uma. O vetor inicial do BLAKE2b é o do SHA-512 e o do
//! BLAKE2s é o do SHA-256.

use super::{sha512, SHA256_INIT};

/// As permutações de mensagem (as duas variantes usam as dez primeiras, voltando ao início a partir da
/// décima rodada).
const SIGMA: [[usize; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

/// Os parâmetros do bloco de parâmetros (RFC 7693, 2.5). Os limites de cada campo são conferidos por quem
/// chama; aqui eles só são gravados.
#[derive(Clone, Debug)]
pub struct Params {
    pub digest_length: usize,
    pub key: Vec<u8>,
    pub salt: Vec<u8>,
    pub person: Vec<u8>,
    pub fanout: u8,
    pub depth: u8,
    pub leaf_length: u32,
    pub node_offset: u64,
    pub node_depth: u8,
    pub inner_length: u8,
    pub last_node: bool,
}

impl Params {
    /// Resumo sequencial de `digest_length` bytes, sem chave, sal nem personalização.
    pub fn sequential(digest_length: usize) -> Params {
        Params {
            digest_length,
            key: Vec::new(),
            salt: Vec::new(),
            person: Vec::new(),
            fanout: 1,
            depth: 1,
            leaf_length: 0,
            node_offset: 0,
            node_depth: 0,
            inner_length: 0,
            last_node: false,
        }
    }
}

/// O bloco de parâmetros de `P` bytes (64 no BLAKE2b, 32 no BLAKE2s). O `node_offset` ocupa
/// `node_offset_len` bytes (8 e 6), e o sal e a personalização dividem a metade final em duas.
fn param_block<const P: usize>(p: &Params, node_offset_len: usize) -> [u8; P] {
    let mut out = [0u8; P];
    out[0] = p.digest_length as u8;
    out[1] = p.key.len() as u8;
    out[2] = p.fanout;
    out[3] = p.depth;
    out[4..8].copy_from_slice(&p.leaf_length.to_le_bytes());
    out[8..8 + node_offset_len].copy_from_slice(&p.node_offset.to_le_bytes()[..node_offset_len]);
    out[8 + node_offset_len] = p.node_depth;
    out[9 + node_offset_len] = p.inner_length;
    let (half, quarter) = (P / 2, P / 4);
    out[half..half + p.salt.len()].copy_from_slice(&p.salt);
    out[half + quarter..half + quarter + p.person.len()].copy_from_slice(&p.person);
    out
}

macro_rules! blake2_variant {
    (
        $module:ident, word: $word:ty, block: $block:literal, rounds: $rounds:literal,
        rotations: [$r1:literal, $r2:literal, $r3:literal, $r4:literal],
        node_offset_len: $offset_len:literal, iv: $iv:expr
    ) => {
        pub mod $module {
            use super::*;

            /// Bytes por bloco.
            pub const BLOCK: usize = $block;
            const WORD: usize = std::mem::size_of::<$word>();

            fn mix(v: &mut [$word; 16], [a, b, c, d]: [usize; 4], x: $word, y: $word) {
                v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
                v[d] = (v[d] ^ v[a]).rotate_right($r1);
                v[c] = v[c].wrapping_add(v[d]);
                v[b] = (v[b] ^ v[c]).rotate_right($r2);
                v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
                v[d] = (v[d] ^ v[a]).rotate_right($r3);
                v[c] = v[c].wrapping_add(v[d]);
                v[b] = (v[b] ^ v[c]).rotate_right($r4);
            }

            fn compress(h: &mut [$word; 8], block: &[u8; BLOCK], counter: u128, last: bool, last_node: bool) {
                let iv: [$word; 8] = $iv;
                let mut m = [0 as $word; 16];
                for (i, w) in m.iter_mut().enumerate() {
                    let mut b = [0u8; WORD];
                    b.copy_from_slice(&block[WORD * i..WORD * i + WORD]);
                    *w = <$word>::from_le_bytes(b);
                }
                let mut v = [0 as $word; 16];
                v[..8].copy_from_slice(&h[..]);
                v[8..].copy_from_slice(&iv);
                v[12] ^= counter as $word;
                v[13] ^= (counter >> <$word>::BITS) as $word;
                if last {
                    v[14] = !v[14];
                    if last_node {
                        v[15] = !v[15];
                    }
                }
                for round in 0..$rounds {
                    let s = &SIGMA[round % 10];
                    mix(&mut v, [0, 4, 8, 12], m[s[0]], m[s[1]]);
                    mix(&mut v, [1, 5, 9, 13], m[s[2]], m[s[3]]);
                    mix(&mut v, [2, 6, 10, 14], m[s[4]], m[s[5]]);
                    mix(&mut v, [3, 7, 11, 15], m[s[6]], m[s[7]]);
                    mix(&mut v, [0, 5, 10, 15], m[s[8]], m[s[9]]);
                    mix(&mut v, [1, 6, 11, 12], m[s[10]], m[s[11]]);
                    mix(&mut v, [2, 7, 8, 13], m[s[12]], m[s[13]]);
                    mix(&mut v, [3, 4, 9, 14], m[s[14]], m[s[15]]);
                }
                for i in 0..8 {
                    h[i] ^= v[i] ^ v[i + 8];
                }
            }

            /// Estado incremental. O último bloco cheio fica retido até chegar mais um byte ou o fim, porque
            /// só o bloco final leva o sinalizador de último.
            #[derive(Clone)]
            pub struct State {
                h: [$word; 8],
                counter: u128,
                buf: [u8; BLOCK],
                len: usize,
                last_node: bool,
                out_len: usize,
            }

            impl State {
                pub fn new(p: &Params) -> State {
                    let iv: [$word; 8] = $iv;
                    let params = param_block::<{ 8 * WORD }>(p, $offset_len);
                    let mut h = iv;
                    for (i, w) in h.iter_mut().enumerate() {
                        let mut b = [0u8; WORD];
                        b.copy_from_slice(&params[WORD * i..WORD * i + WORD]);
                        *w ^= <$word>::from_le_bytes(b);
                    }
                    let mut state = State { h, counter: 0, buf: [0; BLOCK], len: 0, last_node: p.last_node, out_len: p.digest_length };
                    if !p.key.is_empty() {
                        state.buf[..p.key.len()].copy_from_slice(&p.key);
                        state.len = BLOCK;
                    }
                    state
                }

                pub fn update(&mut self, mut data: &[u8]) {
                    while !data.is_empty() {
                        if self.len == BLOCK {
                            self.counter += BLOCK as u128;
                            compress(&mut self.h, &self.buf, self.counter, false, self.last_node);
                            self.len = 0;
                        }
                        let take = (BLOCK - self.len).min(data.len());
                        self.buf[self.len..self.len + take].copy_from_slice(&data[..take]);
                        self.len += take;
                        data = &data[take..];
                    }
                }

                pub fn finalize(mut self) -> Vec<u8> {
                    self.counter += self.len as u128;
                    self.buf[self.len..].fill(0);
                    compress(&mut self.h, &self.buf, self.counter, true, self.last_node);
                    let mut out: Vec<u8> = self.h.iter().flat_map(|w| w.to_le_bytes()).collect();
                    out.truncate(self.out_len);
                    out
                }
            }
        }
    };
}

blake2_variant!(
    b, word: u64, block: 128, rounds: 12, rotations: [32, 24, 16, 63], node_offset_len: 8,
    iv: {
        let mut iv = [0u64; 8];
        iv.copy_from_slice(&sha512::sqrt_consts()[..8]);
        iv
    }
);

blake2_variant!(
    s, word: u32, block: 64, rounds: 10, rotations: [16, 12, 8, 7], node_offset_len: 6,
    iv: SHA256_INIT
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::hex_lower;

    fn blake2b(data: &[u8], len: usize) -> String {
        let mut h = b::State::new(&Params::sequential(len));
        h.update(data);
        hex_lower(&h.finalize())
    }

    fn blake2s(data: &[u8], len: usize) -> String {
        let mut h = s::State::new(&Params::sequential(len));
        h.update(data);
        hex_lower(&h.finalize())
    }

    #[test]
    fn unkeyed_vectors() {
        assert!(blake2b(b"", 64).starts_with("786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419"));
        assert!(blake2b(b"abc", 64).starts_with("ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d1"));
        assert_eq!(blake2s(b"abc", 32), "508c5e8c327c14e2e1a72ba34eeb452f37458b209ed63a294d999b4c86675982");
        assert_eq!(blake2s(b"", 32), "69217a3079908094e11121d042354a7c1f55b6482ca1a51e1b250dfd1ed0eef9");
    }

    #[test]
    fn incremental_matches_one_shot_around_block_edges() {
        let data: Vec<u8> = (0..600u32).map(|i| (i * 13 + 7) as u8).collect();
        for step in [1usize, 63, 64, 65, 127, 128, 129, 599] {
            let mut x = b::State::new(&Params::sequential(64));
            let mut y = s::State::new(&Params::sequential(32));
            for chunk in data.chunks(step) {
                x.update(chunk);
                y.update(chunk);
            }
            assert_eq!(hex_lower(&x.finalize()), blake2b(&data, 64), "blake2b passo {step}");
            assert_eq!(hex_lower(&y.finalize()), blake2s(&data, 32), "blake2s passo {step}");
        }
    }

    #[test]
    fn keyed_blake2b_matches_rfc_7693_appendix_style_vector() {
        // A chave de 64 bytes 00..3f e a entrada vazia: o primeiro vetor do `blake2b-kat.txt` com chave.
        let key: Vec<u8> = (0..64u8).collect();
        let mut p = Params::sequential(64);
        p.key = key;
        let h = b::State::new(&p);
        assert!(hex_lower(&h.finalize()).starts_with("10ebb67700b1868efb4417987acf4690ae9d972fb7a590c2f02871799aaa4786"));
    }
}
