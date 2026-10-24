//! RIPEMD-160 (o `ripemd160` do OpenSSL), incremental.

use super::{load_words, store_words, Blocks};

/// Ordem das palavras da mensagem, linha esquerda e linha direita.
const WORD_LEFT: [usize; 80] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 7, 4, 13, 1, 10, 6, 15, 3, 12, 0, 9, 5, 2, 14, 11, 8, 3, 10, 14,
    4, 9, 15, 8, 1, 2, 7, 0, 6, 13, 11, 5, 12, 1, 9, 11, 10, 0, 8, 12, 4, 13, 3, 7, 15, 14, 5, 6, 2, 4, 0, 5, 9, 7, 12, 2,
    10, 14, 1, 3, 8, 11, 6, 15, 13,
];
const WORD_RIGHT: [usize; 80] = [
    5, 14, 7, 0, 9, 2, 11, 4, 13, 6, 15, 8, 1, 10, 3, 12, 6, 11, 3, 7, 0, 13, 5, 10, 14, 15, 8, 12, 4, 9, 1, 2, 15, 5, 1,
    3, 7, 14, 6, 9, 11, 8, 12, 2, 10, 0, 4, 13, 8, 6, 4, 1, 3, 11, 15, 0, 5, 12, 2, 13, 9, 7, 10, 14, 12, 15, 10, 4, 1, 5,
    8, 7, 6, 2, 13, 14, 0, 3, 9, 11,
];
/// Rotações à esquerda, linha esquerda e linha direita.
const SHIFT_LEFT: [u32; 80] = [
    11, 14, 15, 12, 5, 8, 7, 9, 11, 13, 14, 15, 6, 7, 9, 8, 7, 6, 8, 13, 11, 9, 7, 15, 7, 12, 15, 9, 11, 7, 13, 12, 11, 13,
    6, 7, 14, 9, 13, 15, 14, 8, 13, 6, 5, 12, 7, 5, 11, 12, 14, 15, 14, 15, 9, 8, 9, 14, 5, 6, 8, 6, 5, 12, 9, 15, 5, 11,
    6, 8, 13, 12, 5, 12, 13, 14, 11, 8, 5, 6,
];
const SHIFT_RIGHT: [u32; 80] = [
    8, 9, 9, 11, 13, 15, 15, 5, 7, 7, 8, 11, 14, 14, 12, 6, 9, 13, 15, 7, 12, 8, 9, 11, 7, 7, 12, 7, 6, 15, 13, 11, 9, 7,
    15, 11, 8, 6, 6, 14, 12, 13, 5, 14, 13, 13, 7, 5, 15, 5, 8, 11, 14, 14, 6, 14, 6, 9, 12, 9, 12, 5, 15, 8, 8, 5, 12, 9,
    12, 5, 14, 6, 8, 13, 6, 5, 15, 13, 11, 11,
];
const CONST_LEFT: [u32; 5] = [0, 0x5a82_7999, 0x6ed9_eba1, 0x8f1b_bcdc, 0xa953_fd4e];
const CONST_RIGHT: [u32; 5] = [0x50a2_8be6, 0x5c4d_d124, 0x6d70_3ef3, 0x7a6d_76e9, 0];

/// A função booleana da rodada `round` (0 a 4).
fn mixer(round: usize, x: u32, y: u32, z: u32) -> u32 {
    match round {
        0 => x ^ y ^ z,
        1 => (x & y) | (!x & z),
        2 => (x | !y) ^ z,
        3 => (x & z) | (y & !z),
        _ => x ^ (y | !z),
    }
}

fn compress(state: &mut [u32; 5], block: &[u8; 64]) {
    let x = load_words(block, true);
    let [mut al, mut bl, mut cl, mut dl, mut el] = *state;
    let [mut ar, mut br, mut cr, mut dr, mut er] = *state;
    for j in 0..80 {
        let t = al
            .wrapping_add(mixer(j / 16, bl, cl, dl))
            .wrapping_add(x[WORD_LEFT[j]])
            .wrapping_add(CONST_LEFT[j / 16])
            .rotate_left(SHIFT_LEFT[j])
            .wrapping_add(el);
        al = el;
        el = dl;
        dl = cl.rotate_left(10);
        cl = bl;
        bl = t;
        let t = ar
            .wrapping_add(mixer(4 - j / 16, br, cr, dr))
            .wrapping_add(x[WORD_RIGHT[j]])
            .wrapping_add(CONST_RIGHT[j / 16])
            .rotate_left(SHIFT_RIGHT[j])
            .wrapping_add(er);
        ar = er;
        er = dr;
        dr = cr.rotate_left(10);
        cr = br;
        br = t;
    }
    let t = state[1].wrapping_add(cl).wrapping_add(dr);
    state[1] = state[2].wrapping_add(dl).wrapping_add(er);
    state[2] = state[3].wrapping_add(el).wrapping_add(ar);
    state[3] = state[4].wrapping_add(al).wrapping_add(br);
    state[4] = state[0].wrapping_add(bl).wrapping_add(cr);
    state[0] = t;
}

/// RIPEMD-160 incremental.
#[derive(Clone)]
pub struct Ripemd160 {
    state: [u32; 5],
    blocks: Blocks<64>,
}

impl Ripemd160 {
    pub fn new() -> Ripemd160 {
        Ripemd160 { state: [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476, 0xc3d2_e1f0], blocks: Blocks::new() }
    }
}

block_hash!(Ripemd160, compress, length_big_endian: false, words_little_endian: true, out: 20);

impl Default for Ripemd160 {
    fn default() -> Ripemd160 {
        Ripemd160::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::hex_lower;

    #[test]
    fn vectors() {
        let hex = |data: &[u8]| {
            let mut h = Ripemd160::new();
            h.update(data);
            hex_lower(&h.finalize())
        };
        assert_eq!(hex(b""), "9c1185a5c5e9fc54612808977ee8f548b2258d31");
        assert_eq!(hex(b"abc"), "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc");
        assert_eq!(hex(b"message digest"), "5d0689ef49d2fae572b881b123a85ffa21595f36");
    }
}
