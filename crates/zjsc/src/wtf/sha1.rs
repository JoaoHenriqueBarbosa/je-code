//! Tradução de `WTF/wtf/SHA1.h` e `SHA1.cpp` (ramo não Cocoa, o SHA-1 direto do RFC 3174).
//!
//! `addUTF8Bytes(CFStringRef)` e o ramo `PLATFORM(COCOA)` não existem em Linux.

use crate::wtf::text::conversion_mode::ConversionMode;
use crate::wtf::text::string_view::StringView;
use crate::wtf::text::wtf_string::String as WtfString;

/// `SHA1::hashSize`.
pub const HASH_SIZE: usize = 20;

/// `SHA1::Digest`.
pub type Digest = [u8; HASH_SIZE];

fn f(t: i32, b: u32, c: u32, d: u32) -> u32 {
    debug_assert!((0..80).contains(&t));
    if t < 20 {
        return (b & c) | ((!b) & d);
    }
    if t < 40 {
        return b ^ c ^ d;
    }
    if t < 60 {
        return (b & c) | (b & d) | (c & d);
    }
    b ^ c ^ d
}

fn k(t: i32) -> u32 {
    debug_assert!((0..80).contains(&t));
    if t < 20 {
        return 0x5a827999;
    }
    if t < 40 {
        return 0x6ed9eba1;
    }
    if t < 60 {
        return 0x8f1bbcdc;
    }
    0xca62c1d6
}

fn rotate_left(n: u32, x: u32) -> u32 {
    debug_assert!(n < 32);
    (x << n) | (x >> (32 - n))
}

/// `class SHA1`.
pub struct Sha1 {
    buffer: [u8; 64],
    /// `m_cursor`: bytes preenchidos em `buffer` (0 a 64).
    cursor: usize,
    /// `m_totalBytes`.
    total_bytes: u64,
    hash: [u32; 5],
}

impl Default for Sha1 {
    fn default() -> Self {
        Sha1::new()
    }
}

impl Sha1 {
    /// `SHA1::SHA1()`.
    pub fn new() -> Sha1 {
        let mut sha1 = Sha1 { buffer: [0; 64], cursor: 0, total_bytes: 0, hash: [0; 5] };
        sha1.reset();
        sha1
    }

    /// `addBytes(std::span<const std::byte>)`.
    pub fn add_bytes(&mut self, input: &[u8]) {
        for &byte in input {
            debug_assert!(self.cursor < 64);
            self.buffer[self.cursor] = byte;
            self.cursor += 1;
            self.total_bytes += 1;
            if self.cursor == 64 {
                self.process_block();
            }
        }
    }

    /// `addUTF8Bytes(StringView)`.
    pub fn add_utf8_bytes(&mut self, string: StringView) {
        if string.contains_only_ascii() {
            if string.is_8bit() {
                self.add_bytes(string.span8());
            } else {
                let narrowed = WtfString::make_8bit(string.span16());
                self.add_bytes(narrowed.span8());
            }
        } else {
            self.add_bytes(&string.utf8(ConversionMode::LenientConversion));
        }
    }

    /// `computeHash(Digest&)`.
    pub fn compute_hash(&mut self) -> Digest {
        self.finalize();

        let mut digest = [0u8; HASH_SIZE];
        for i in 0..5 {
            // Treat hashValue as a big-endian value.
            let mut hash_value = self.hash[i];
            for j in 0..4 {
                digest[4 * i + (3 - j)] = (hash_value & 0xFF) as u8;
                hash_value >>= 8;
            }
        }

        self.reset();
        digest
    }

    /// `hexDigest(const Digest&)`: o `CString` vira bytes (`toHexCString`, minúsculas).
    pub fn hex_digest(digest: &Digest) -> Vec<u8> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = Vec::with_capacity(HASH_SIZE * 2);
        for &byte in digest {
            out.push(HEX[(byte >> 4) as usize]);
            out.push(HEX[(byte & 0xF) as usize]);
        }
        out
    }

    /// `computeHexDigest()`.
    pub fn compute_hex_digest(&mut self) -> Vec<u8> {
        let digest = self.compute_hash();
        Sha1::hex_digest(&digest)
    }

    fn finalize(&mut self) {
        debug_assert!(self.cursor < 64);
        self.buffer[self.cursor] = 0x80;
        self.cursor += 1;
        if self.cursor > 56 {
            // Pad out to next block.
            while self.cursor < 64 {
                self.buffer[self.cursor] = 0x00;
                self.cursor += 1;
            }
            self.process_block();
        }

        for i in self.cursor..56 {
            self.buffer[i] = 0x00;
        }

        // Write the length as a big-endian 64-bit value.
        let mut bits = self.total_bytes.wrapping_mul(8);
        for i in 0..8 {
            self.buffer[56 + (7 - i)] = (bits & 0xFF) as u8;
            bits >>= 8;
        }
        self.cursor = 64;
        self.process_block();
    }

    fn process_block(&mut self) {
        debug_assert!(self.cursor == 64);

        let mut w = [0u32; 80];
        for t in 0..16 {
            w[t] = ((self.buffer[t * 4] as u32) << 24)
                | ((self.buffer[t * 4 + 1] as u32) << 16)
                | ((self.buffer[t * 4 + 2] as u32) << 8)
                | (self.buffer[t * 4 + 3] as u32);
        }
        for t in 16..80 {
            w[t] = rotate_left(1, w[t - 3] ^ w[t - 8] ^ w[t - 14] ^ w[t - 16]);
        }

        let mut a = self.hash[0];
        let mut b = self.hash[1];
        let mut c = self.hash[2];
        let mut d = self.hash[3];
        let mut e = self.hash[4];

        for t in 0..80 {
            let temp = rotate_left(5, a)
                .wrapping_add(f(t as i32, b, c, d))
                .wrapping_add(e)
                .wrapping_add(w[t])
                .wrapping_add(k(t as i32));
            e = d;
            d = c;
            c = rotate_left(30, b);
            b = a;
            a = temp;
        }

        self.hash[0] = self.hash[0].wrapping_add(a);
        self.hash[1] = self.hash[1].wrapping_add(b);
        self.hash[2] = self.hash[2].wrapping_add(c);
        self.hash[3] = self.hash[3].wrapping_add(d);
        self.hash[4] = self.hash[4].wrapping_add(e);

        self.cursor = 0;
    }

    fn reset(&mut self) {
        self.cursor = 0;
        self.total_bytes = 0;
        self.hash = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];

        // Clear the buffer after use in case it's sensitive.
        self.buffer = [0; 64];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abc_vector() {
        let mut sha1 = Sha1::new();
        sha1.add_bytes(b"abc");
        assert_eq!(sha1.compute_hex_digest(), b"a9993e364706816aba3e25717850c26c9cd0d89d".to_vec());
    }

    #[test]
    fn empty_vector() {
        let mut sha1 = Sha1::new();
        assert_eq!(sha1.compute_hex_digest(), b"da39a3ee5e6b4b0d3255bfef95601890afd80709".to_vec());
    }

    #[test]
    fn two_blocks_vector() {
        let mut sha1 = Sha1::new();
        sha1.add_bytes(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq");
        assert_eq!(sha1.compute_hex_digest(), b"84983e441c3bd26ebaae4aa1f95129e5e54670f1".to_vec());
    }
}
