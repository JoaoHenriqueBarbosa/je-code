//! Os hashes não criptográficos do `Bun.hash` (bun 1.4.2, `src/hash` e `src/wyhash`), bit a bit iguais às implementações
//! de referência de cada algoritmo: wyhash (a variante final4, 48 bytes por rodada), Adler-32, CityHash32 e CityHash64
//! (com semente), MurmurHash2 de 32 e 64 bits, MurmurHash3 de 32 bits e rapidhash. O xxHash vem do `twox-hash` e o
//! CRC-32 do `ul_common::codec`.

fn read32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

fn read64(bytes: &[u8], offset: usize) -> u64 {
    let mut word = [0u8; 8];
    word.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(word)
}

/// O produto de 128 bits partido nas metades baixa e alta (o `mum` do wyhash e do rapidhash).
fn mum(a: u64, b: u64) -> (u64, u64) {
    let product = u128::from(a) * u128::from(b);
    (product as u64, (product >> 64) as u64)
}

/// `mix`: o `mum` com as metades combinadas por XOR.
fn mix(a: u64, b: u64) -> u64 {
    let (low, high) = mum(a, b);
    low ^ high
}

const WYHASH_SECRET: [u64; 4] = [0xa0761d6478bd642f, 0xe7037ed1a0b428db, 0x8ebc6af09c88c6e3, 0x589965cc75374cc3];

/// `Wyhash::hash(seed, input)` (o `std.hash.Wyhash` do Zig, variante final4).
pub(crate) fn wyhash(seed: u64, input: &[u8]) -> u64 {
    let s0 = seed ^ mix(seed ^ WYHASH_SECRET[0], WYHASH_SECRET[1]);
    let mut state = [s0, s0, s0];
    let (mut a, mut b);
    if input.len() <= 16 {
        if input.len() >= 4 {
            let end = input.len() - 4;
            let quarter = (input.len() >> 3) << 2;
            a = (u64::from(read32(input, 0)) << 32) | u64::from(read32(input, quarter));
            b = (u64::from(read32(input, end)) << 32) | u64::from(read32(input, end - quarter));
        } else if !input.is_empty() {
            a = (u64::from(input[0]) << 16) | (u64::from(input[input.len() >> 1]) << 8) | u64::from(input[input.len() - 1]);
            b = 0;
        } else {
            a = 0;
            b = 0;
        }
    } else {
        let mut i = 0;
        if input.len() >= 48 {
            while i + 48 < input.len() {
                for lane in 0..3 {
                    let offset = i + lane * 16;
                    state[lane] = mix(read64(input, offset) ^ WYHASH_SECRET[lane + 1], read64(input, offset + 8) ^ state[lane]);
                }
                i += 48;
            }
            // `final0`.
            state[0] ^= state[1] ^ state[2];
        }
        // `final1`.
        let rest = &input[i..];
        let mut j = 0;
        while j + 16 < rest.len() {
            state[0] = mix(read64(rest, j) ^ WYHASH_SECRET[1], read64(rest, j + 8) ^ state[0]);
            j += 16;
        }
        a = read64(input, input.len() - 16);
        b = read64(input, input.len() - 8);
    }
    // `final2`.
    a ^= WYHASH_SECRET[1];
    b ^= state[0];
    let (a, b) = mum(a, b);
    mix(a ^ WYHASH_SECRET[0] ^ input.len() as u64, b ^ WYHASH_SECRET[1])
}

/// Adler-32 (o `permute(1, input)` do bun, com o laço de `NMAX` do zlib).
pub(crate) fn adler32(input: &[u8]) -> u32 {
    const BASE: u32 = 65521;
    const NMAX: usize = 5552;
    let (mut s1, mut s2) = (1u32, 0u32);
    for chunk in input.chunks(NMAX) {
        for byte in chunk {
            s1 += u32::from(*byte);
            s2 += s1;
        }
        s1 %= BASE;
        s2 %= BASE;
    }
    s1 | (s2 << 16)
}

/// `fmix32` do MurmurHash3, que o CityHash32 também usa.
fn fmix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85ebca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae35);
    h ^ (h >> 16)
}

const MURMUR3_C1: u32 = 0xcc9e2d51;
const MURMUR3_C2: u32 = 0x1b873593;

/// MurmurHash2 de 32 bits.
pub(crate) fn murmur2_32(input: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1e995;
    let length = input.len() as u32;
    let mut h = seed ^ length;
    let blocks = input.chunks_exact(4);
    let tail = blocks.remainder();
    for block in blocks {
        let mut k = read32(block, 0).wrapping_mul(M);
        k ^= k >> 24;
        h = h.wrapping_mul(M) ^ k.wrapping_mul(M);
    }
    if tail.len() >= 3 {
        h ^= u32::from(tail[2]) << 16;
    }
    if tail.len() >= 2 {
        h ^= u32::from(tail[1]) << 8;
    }
    if !tail.is_empty() {
        h ^= u32::from(tail[0]);
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^ (h >> 15)
}

/// MurmurHash2 de 64 bits (MurmurHash64A).
pub(crate) fn murmur2_64(input: &[u8], seed: u64) -> u64 {
    const M: u64 = 0xc6a4a7935bd1e995;
    let mut h = seed ^ (input.len() as u64).wrapping_mul(M);
    let blocks = input.chunks_exact(8);
    let tail = blocks.remainder();
    for block in blocks {
        let mut k = read64(block, 0).wrapping_mul(M);
        k ^= k >> 47;
        h ^= k.wrapping_mul(M);
        h = h.wrapping_mul(M);
    }
    if !tail.is_empty() {
        let mut word = [0u8; 8];
        word[..tail.len()].copy_from_slice(tail);
        h ^= u64::from_le_bytes(word);
        h = h.wrapping_mul(M);
    }
    h ^= h >> 47;
    h = h.wrapping_mul(M);
    h ^ (h >> 47)
}

/// MurmurHash3 de 32 bits (x86_32).
pub(crate) fn murmur3_32(input: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    let blocks = input.chunks_exact(4);
    let tail = blocks.remainder();
    for block in blocks {
        let k = read32(block, 0).wrapping_mul(MURMUR3_C1).rotate_left(15).wrapping_mul(MURMUR3_C2);
        h = (h ^ k).rotate_left(13).wrapping_mul(5).wrapping_add(0xe6546b64);
    }
    let mut k = 0u32;
    if tail.len() == 3 {
        k ^= u32::from(tail[2]) << 16;
    }
    if tail.len() >= 2 {
        k ^= u32::from(tail[1]) << 8;
    }
    if !tail.is_empty() {
        k ^= u32::from(tail[0]);
        h ^= k.wrapping_mul(MURMUR3_C1).rotate_left(15).wrapping_mul(MURMUR3_C2);
    }
    fmix32(h ^ input.len() as u32)
}

/// O `Mur` do CityHash32.
fn city_mur(a: u32, h: u32) -> u32 {
    let a = a.wrapping_mul(MURMUR3_C1).rotate_right(17).wrapping_mul(MURMUR3_C2);
    (h ^ a).rotate_right(19).wrapping_mul(5).wrapping_add(0xe6546b64)
}

/// `rotr(x * c1, 17) * c2`, o embaralhamento de cada palavra do CityHash32.
fn city_scramble(word: u32) -> u32 {
    word.wrapping_mul(MURMUR3_C1).rotate_right(17).wrapping_mul(MURMUR3_C2)
}

/// CityHash32 (v1.1).
pub(crate) fn city_hash32(s: &[u8]) -> u32 {
    let len = s.len();
    let len32 = len as u32;
    if len <= 4 {
        let (mut b, mut c) = (0u32, 9u32);
        for byte in s {
            b = b.wrapping_mul(MURMUR3_C1).wrapping_add(*byte as i8 as i32 as u32);
            c ^= b;
        }
        return fmix32(city_mur(b, city_mur(len32, c)));
    }
    if len <= 12 {
        let a = len32.wrapping_add(read32(s, 0));
        let d = len32.wrapping_mul(5);
        let b = d.wrapping_add(read32(s, len - 4));
        let c = 9u32.wrapping_add(read32(s, (len >> 1) & 4));
        return fmix32(city_mur(c, city_mur(b, city_mur(a, d))));
    }
    if len <= 24 {
        let a = read32(s, (len >> 1) - 4);
        let b = read32(s, 4);
        let c = read32(s, len - 8);
        let d = read32(s, len >> 1);
        let e = read32(s, 0);
        let f = read32(s, len - 4);
        return fmix32(city_mur(f, city_mur(e, city_mur(d, city_mur(c, city_mur(b, city_mur(a, len32)))))));
    }
    let round = |value: u32, rotation: u32| value.rotate_right(rotation).wrapping_mul(5).wrapping_add(0xe6546b64);
    let mut h = len32;
    let mut g = MURMUR3_C1.wrapping_mul(len32);
    let mut f = g;
    let a0 = city_scramble(read32(s, len - 4));
    let a1 = city_scramble(read32(s, len - 8));
    let a2 = city_scramble(read32(s, len - 16));
    let a3 = city_scramble(read32(s, len - 12));
    let a4 = city_scramble(read32(s, len - 20));
    h = round(h ^ a0, 19);
    h = round(h ^ a2, 19);
    g = round(g ^ a1, 19);
    g = round(g ^ a3, 19);
    f = round(f.wrapping_add(a4), 19);
    let mut offset = 0;
    for _ in 0..(len - 1) / 20 {
        let b0 = city_scramble(read32(s, offset));
        let b1 = read32(s, offset + 4);
        let b2 = city_scramble(read32(s, offset + 8));
        let b3 = city_scramble(read32(s, offset + 12));
        let b4 = read32(s, offset + 16);
        h = round(h ^ b0, 18);
        f = f.wrapping_add(b1).rotate_right(19).wrapping_mul(MURMUR3_C1);
        g = round(g.wrapping_add(b2), 18);
        h = round(h ^ b3.wrapping_add(b1), 19);
        g = (g ^ b4).swap_bytes().wrapping_mul(5);
        h = h.wrapping_add(b4.wrapping_mul(5)).swap_bytes();
        f = f.wrapping_add(b0);
        (h, f, g) = (f, g, h);
        offset += 20;
    }
    g = g.rotate_right(11).wrapping_mul(MURMUR3_C1).rotate_right(17).wrapping_mul(MURMUR3_C1);
    f = f.rotate_right(11).wrapping_mul(MURMUR3_C1).rotate_right(17).wrapping_mul(MURMUR3_C1);
    h = round(h.wrapping_add(g), 19);
    h = h.rotate_right(17).wrapping_mul(MURMUR3_C1);
    h = round(h.wrapping_add(f), 19);
    h.rotate_right(17).wrapping_mul(MURMUR3_C1)
}

const CITY_K0: u64 = 0xc3a5c85c97cb3127;
const CITY_K1: u64 = 0xb492b66fbe98f273;
const CITY_K2: u64 = 0x9ae16a3b2f90404f;

fn shift_mix(value: u64) -> u64 {
    value ^ (value >> 47)
}

/// `HashLen16(u, v, mul)`.
fn hash_len16_mul(low: u64, high: u64, mul: u64) -> u64 {
    let a = shift_mix((low ^ high).wrapping_mul(mul));
    let b = shift_mix((high ^ a).wrapping_mul(mul));
    b.wrapping_mul(mul)
}

/// `Hash128to64`.
fn hash_len16(low: u64, high: u64) -> u64 {
    hash_len16_mul(low, high, 0x9ddfea08eb382d69)
}

/// `WeakHashLen32WithSeeds(s + offset, a, b)`.
fn weak_hash_len32_with_seeds(s: &[u8], offset: usize, a: u64, b: u64) -> (u64, u64) {
    let (w, x, y, z) = (read64(s, offset), read64(s, offset + 8), read64(s, offset + 16), read64(s, offset + 24));
    let a = a.wrapping_add(w);
    let b = b.wrapping_add(a).wrapping_add(z).rotate_right(21);
    let c = a;
    let a = a.wrapping_add(x).wrapping_add(y);
    let b = b.wrapping_add(a.rotate_right(44));
    (a.wrapping_add(z), b.wrapping_add(c))
}

/// CityHash64 sem semente (v1.1).
fn city_hash64_unseeded(s: &[u8]) -> u64 {
    let len = s.len();
    let len64 = len as u64;
    if len <= 16 {
        if len >= 8 {
            let mul = CITY_K2.wrapping_add(len64.wrapping_mul(2));
            let a = read64(s, 0).wrapping_add(CITY_K2);
            let b = read64(s, len - 8);
            let c = b.rotate_right(37).wrapping_mul(mul).wrapping_add(a);
            let d = a.rotate_right(25).wrapping_add(b).wrapping_mul(mul);
            return hash_len16_mul(c, d, mul);
        }
        if len >= 4 {
            let mul = CITY_K2.wrapping_add(len64.wrapping_mul(2));
            let a = u64::from(read32(s, 0));
            return hash_len16_mul(len64.wrapping_add(a << 3), u64::from(read32(s, len - 4)), mul);
        }
        if len > 0 {
            let y = u32::from(s[0]).wrapping_add(u32::from(s[len >> 1]) << 8);
            let z = (len as u32).wrapping_add(u32::from(s[len - 1]) << 2);
            return shift_mix(u64::from(y).wrapping_mul(CITY_K2) ^ u64::from(z).wrapping_mul(CITY_K0)).wrapping_mul(CITY_K2);
        }
        return CITY_K2;
    }
    if len <= 32 {
        let mul = CITY_K2.wrapping_add(len64.wrapping_mul(2));
        let a = read64(s, 0).wrapping_mul(CITY_K1);
        let b = read64(s, 8);
        let c = read64(s, len - 8).wrapping_mul(mul);
        let d = read64(s, len - 16).wrapping_mul(CITY_K2);
        return hash_len16_mul(
            a.wrapping_add(b).rotate_right(43).wrapping_add(c.rotate_right(30)).wrapping_add(d),
            a.wrapping_add(b.wrapping_add(CITY_K2).rotate_right(18)).wrapping_add(c),
            mul,
        );
    }
    if len <= 64 {
        let mul = CITY_K2.wrapping_add(len64.wrapping_mul(2));
        let a = read64(s, 0).wrapping_mul(CITY_K2);
        let b = read64(s, 8);
        let c = read64(s, len - 24);
        let d = read64(s, len - 32);
        let e = read64(s, 16).wrapping_mul(CITY_K2);
        let f = read64(s, 24).wrapping_mul(9);
        let g = read64(s, len - 8);
        let h = read64(s, len - 16).wrapping_mul(mul);
        let u = a.wrapping_add(g).rotate_right(43).wrapping_add(b.rotate_right(30).wrapping_add(c).wrapping_mul(9));
        let v = (a.wrapping_add(g) ^ d).wrapping_add(f).wrapping_add(1);
        let w = u.wrapping_add(v).wrapping_mul(mul).swap_bytes().wrapping_add(h);
        let x = e.wrapping_add(f).rotate_right(42).wrapping_add(c);
        let y = v.wrapping_add(w).wrapping_mul(mul).swap_bytes().wrapping_add(g).wrapping_mul(mul);
        let z = e.wrapping_add(f).wrapping_add(c);
        let a = x.wrapping_add(z).wrapping_mul(mul).wrapping_add(y).swap_bytes().wrapping_add(b);
        let b = shift_mix(z.wrapping_add(a).wrapping_mul(mul).wrapping_add(d).wrapping_add(h)).wrapping_mul(mul);
        return b.wrapping_add(x);
    }
    let mut x = read64(s, len - 40);
    let mut y = read64(s, len - 16).wrapping_add(read64(s, len - 56));
    let mut z = hash_len16(read64(s, len - 48).wrapping_add(len64), read64(s, len - 24));
    let mut v = weak_hash_len32_with_seeds(s, len - 64, len64, z);
    let mut w = weak_hash_len32_with_seeds(s, len - 32, y.wrapping_add(CITY_K1), x);
    x = x.wrapping_mul(CITY_K1).wrapping_add(read64(s, 0));
    let mut remaining = (len64 - 1) & !63u64;
    let mut offset = 0;
    loop {
        x = x.wrapping_add(y).wrapping_add(v.0).wrapping_add(read64(s, offset + 8)).rotate_right(37).wrapping_mul(CITY_K1);
        y = y.wrapping_add(v.1).wrapping_add(read64(s, offset + 48)).rotate_right(42).wrapping_mul(CITY_K1);
        x ^= w.1;
        y = y.wrapping_add(v.0).wrapping_add(read64(s, offset + 40));
        z = z.wrapping_add(w.0).rotate_right(33).wrapping_mul(CITY_K1);
        v = weak_hash_len32_with_seeds(s, offset, v.1.wrapping_mul(CITY_K1), x.wrapping_add(w.0));
        w = weak_hash_len32_with_seeds(s, offset + 32, z.wrapping_add(w.1), y.wrapping_add(read64(s, offset + 16)));
        std::mem::swap(&mut z, &mut x);
        offset += 64;
        remaining -= 64;
        if remaining == 0 {
            break;
        }
    }
    hash_len16(
        hash_len16(v.0, w.0).wrapping_add(shift_mix(y).wrapping_mul(CITY_K1)).wrapping_add(z),
        hash_len16(v.1, w.1).wrapping_add(x),
    )
}

/// `CityHash64WithSeed(s, seed)`: `CityHash64WithSeeds(s, k2, seed)`.
pub(crate) fn city_hash64(s: &[u8], seed: u64) -> u64 {
    hash_len16(city_hash64_unseeded(s).wrapping_sub(CITY_K2), seed)
}

/// rapidhash (v1): o núcleo comum ao WTF do WebKit e do Blink, com os vetores do upstream.
pub(crate) use ul_common::hash::rapidhash::rapidhash;

#[cfg(test)]
mod tests {
    use super::*;

    /// A verificação do SMHasher: `buf[i] = i`, o hash de cada prefixo com semente `256 - i`, os resultados
    /// concatenados em little-endian e o hash disso com semente 0, truncado a 32 bits.
    fn smhasher_32(hash: impl Fn(&[u8], u32) -> u32) -> u32 {
        let buffer: Vec<u8> = (0..=255u8).collect();
        let all: Vec<u8> = (0..256usize).flat_map(|i| hash(&buffer[..i], 256 - i as u32).to_le_bytes()).collect();
        hash(&all, 0)
    }

    fn smhasher_64(hash: impl Fn(&[u8], u64) -> u64) -> u32 {
        let buffer: Vec<u8> = (0..=255u8).collect();
        let all: Vec<u8> = (0..256usize).flat_map(|i| hash(&buffer[..i], 256 - i as u64).to_le_bytes()).collect();
        hash(&all, 0) as u32
    }

    #[test]
    fn smhasher_verification_values() {
        assert_eq!(smhasher_32(murmur2_32), 0x27864C1E);
        assert_eq!(smhasher_64(murmur2_64), 0x1F0D3804);
        assert_eq!(smhasher_32(murmur3_32), 0xB0F57EE3);
        assert_eq!(smhasher_32(|bytes, _| city_hash32(bytes)), 0x68254F81);
        assert_eq!(smhasher_64(city_hash64), 0x5FABC5C5);
    }

    #[test]
    fn adler32_vectors() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"Wikipedia"), 0x11E60398);
    }
}
