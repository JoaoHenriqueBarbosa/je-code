//! SM3 (GB/T 32905-2016, o `sm3` do OpenSSL), incremental.

use super::{load_words, store_words, Blocks};

const INIT: [u32; 8] = [0x7380_166f, 0x4914_b2b9, 0x1724_42d7, 0xda8a_0600, 0xa96f_30bc, 0x1631_38aa, 0xe38d_ee4d, 0xb0fb_0e4e];

fn p0(x: u32) -> u32 {
    x ^ x.rotate_left(9) ^ x.rotate_left(17)
}

fn p1(x: u32) -> u32 {
    x ^ x.rotate_left(15) ^ x.rotate_left(23)
}

fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 68];
    w[..16].copy_from_slice(&load_words(block, false));
    for j in 16..68 {
        w[j] = p1(w[j - 16] ^ w[j - 9] ^ w[j - 3].rotate_left(15)) ^ w[j - 13].rotate_left(7) ^ w[j - 6];
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for j in 0..64 {
        let constant: u32 = if j < 16 { 0x79cc_4519 } else { 0x7a87_9d8a };
        let ss1 = a.rotate_left(12).wrapping_add(e).wrapping_add(constant.rotate_left(j as u32 % 32)).rotate_left(7);
        let ss2 = ss1 ^ a.rotate_left(12);
        let (ff, gg) = if j < 16 { (a ^ b ^ c, e ^ f ^ g) } else { ((a & b) | (a & c) | (b & c), (e & f) | (!e & g)) };
        let tt1 = ff.wrapping_add(d).wrapping_add(ss2).wrapping_add(w[j] ^ w[j + 4]);
        let tt2 = gg.wrapping_add(h).wrapping_add(ss1).wrapping_add(w[j]);
        d = c;
        c = b.rotate_left(9);
        b = a;
        a = tt1;
        h = g;
        g = f.rotate_left(19);
        f = e;
        e = p0(tt2);
    }
    for (s, x) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *s ^= x;
    }
}

/// SM3 incremental.
#[derive(Clone)]
pub struct Sm3 {
    state: [u32; 8],
    blocks: Blocks<64>,
}

impl Sm3 {
    pub fn new() -> Sm3 {
        Sm3 { state: INIT, blocks: Blocks::new() }
    }
}

block_hash!(Sm3, compress, length_big_endian: true, words_little_endian: false, out: 32);

impl Default for Sm3 {
    fn default() -> Sm3 {
        Sm3::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::hex_lower;

    #[test]
    fn vectors() {
        let hex = |data: &[u8]| {
            let mut h = Sm3::new();
            h.update(data);
            hex_lower(&h.finalize())
        };
        assert_eq!(hex(b"abc"), "66c7f0f462eeedd9d1f2d46bdc10e4e24167c4875cf2f7a2297da02b8f4ba8e0");
        assert_eq!(
            hex(b"abcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcd"),
            "debe9ff92275b8a138604889c18e5a4d6fdb70e5387e5765293dcba39c0c5732"
        );
    }
}
