//! Tradução de `WTF/wtf/SixCharacterHash.h` e `SixCharacterHash.cpp`.

use crate::wtf::ascii_ctype::{is_ascii_digit, is_ascii_lower, is_ascii_upper};

/// `sixCharacterHashStringToInteger`: o `RELEASE_ASSERT(c)` vira `assert!`.
pub fn six_character_hash_string_to_integer(string: &[u8; 6]) -> u32 {
    let mut hash: u32 = 0;

    for &c in string {
        hash = hash.wrapping_mul(62);
        assert!(c != 0);
        if is_ascii_upper(c) {
            hash = hash.wrapping_add((c - b'A') as u32);
            continue;
        }
        if is_ascii_lower(c) {
            hash = hash.wrapping_add((c - b'a') as u32 + 26);
            continue;
        }
        debug_assert!(is_ascii_digit(c));
        hash = hash.wrapping_add((c.wrapping_sub(b'0')) as u32 + 26 * 2);
    }

    hash
}

/// `integerToSixCharacterHashString`.
pub fn integer_to_six_character_hash_string(hash: u32) -> [u8; 6] {
    const TABLE: [u8; 62] = *b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut buffer = [0u8; 6];
    let mut accumulator = hash;
    for i in (0..6).rev() {
        buffer[i] = TABLE[(accumulator % 62) as usize];
        accumulator /= 62;
    }
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        for hash in [0u32, 1, 61, 62, 0x2d5a93d0, u32::MAX] {
            assert_eq!(six_character_hash_string_to_integer(&integer_to_six_character_hash_string(hash)), hash);
        }
        assert_eq!(&integer_to_six_character_hash_string(0), b"AAAAAA");
        assert_eq!(&integer_to_six_character_hash_string(1), b"AAAAAB");
    }
}
