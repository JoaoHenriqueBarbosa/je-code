//! Tradução parcial de `WTF/wtf/HashFunctions.h`: só o que os módulos já portados usam.

/// `pairIntHash`: hash composto de dois inteiros (aritmética de 32 bits com wrap, como o `unsigned` do C++).
pub fn pair_int_hash(key1: u32, key2: u32) -> u32 {
    let short_random1: u32 = 277_951_225;
    let short_random2: u32 = 95_187_966;
    let long_random: u64 = 19_248_658_165_952_622;

    let mixed = short_random1
        .wrapping_mul(key1)
        .wrapping_add(short_random2.wrapping_mul(key2));
    let product = long_random.wrapping_mul(u64::from(mixed));
    (product >> ((8 - 4) * 8)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_order_sensitive() {
        assert_eq!(pair_int_hash(1, 2), pair_int_hash(1, 2));
        assert_ne!(pair_int_hash(1, 2), pair_int_hash(2, 1));
    }
}
