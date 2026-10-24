//! `base::PersistentHash` do Chromium (o SuperFastHash de Paul Hsieh sobre bytes, com o comprimento como semente): o
//! hash da chave que o cache de disco grava no cabeçalho de cada entrada.

/// O hash de 32 bits de `data`.
pub fn persistent_hash(data: &[u8]) -> u32 {
    let half = |at: usize| u32::from(u16::from_le_bytes([data[at], data[at + 1]]));
    let mut hash = data.len() as u32;
    let rest = data.len() & 3;
    let mut at = 0;
    for _ in 0..data.len() >> 2 {
        hash = hash.wrapping_add(half(at));
        let tmp = (half(at + 2) << 11) ^ hash;
        hash = (hash << 16) ^ tmp;
        at += 4;
        hash = hash.wrapping_add(hash >> 11);
    }
    match rest {
        3 => {
            hash = hash.wrapping_add(half(at));
            hash ^= hash << 16;
            hash ^= u32::from(data[at + 2]) << 18;
            hash = hash.wrapping_add(hash >> 11);
        }
        2 => {
            hash = hash.wrapping_add(half(at));
            hash ^= hash << 11;
            hash = hash.wrapping_add(hash >> 17);
        }
        1 => {
            hash = hash.wrapping_add(u32::from(data[at]));
            hash ^= hash << 10;
            hash = hash.wrapping_add(hash >> 1);
        }
        _ => {}
    }
    hash ^= hash << 3;
    hash = hash.wrapping_add(hash >> 5);
    hash ^= hash << 4;
    hash = hash.wrapping_add(hash >> 17);
    hash ^= hash << 25;
    hash.wrapping_add(hash >> 6)
}

#[cfg(test)]
mod tests {
    use super::persistent_hash;

    /// O `key_hash` que o oráculo gravou no cabeçalho das entradas do cache (chaves de comprimentos distintos).
    #[test]
    fn matches_the_key_hash_the_oracle_wrote_in_cache_entries() {
        assert_eq!(persistent_hash(b"1/0/_dk_http://127.0.0.1 http://127.0.0.1 http://127.0.0.1:8000/ss-none.html"), 0x0b48_595b);
        assert_eq!(persistent_hash(b"1/0/_dk_http://127.0.0.1 http://127.0.0.1 http://127.0.0.1:8000/favicon.ico"), 0x01dd_78d2);
    }
}
