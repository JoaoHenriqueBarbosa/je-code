//! HMAC (RFC 2104), PBKDF2 (RFC 8018) e scrypt (RFC 7914) sobre qualquer [`Algo`] de saída fixa.

use super::algo::{Algo, Hasher};

/// HMAC incremental. Clonar copia o estado (o `copy()` do `hmac`); as duas pontas já guardam a chave
/// absorvida, então cada resumo novo custa só a mensagem e as duas finalizações.
#[derive(Clone)]
pub struct Hmac {
    inner: Hasher,
    outer: Hasher,
}

impl Hmac {
    /// `None` para os algoritmos de saída livre (SHAKE), que o HMAC não aceita.
    pub fn new(algo: Algo, key: &[u8]) -> Option<Hmac> {
        if algo.is_xof() {
            return None;
        }
        let block = algo.block_size();
        let mut k = if key.len() > block { algo.digest(key) } else { key.to_vec() };
        k.resize(block, 0);
        let (mut inner, mut outer) = (algo.hasher(), algo.hasher());
        inner.update(&k.iter().map(|b| b ^ 0x36).collect::<Vec<u8>>());
        outer.update(&k.iter().map(|b| b ^ 0x5c).collect::<Vec<u8>>());
        Some(Hmac { inner, outer })
    }

    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    pub fn finalize(&self) -> Vec<u8> {
        let mut outer = self.outer.clone();
        outer.update(&self.inner.finalize(0));
        outer.finalize(0)
    }
}

/// HMAC de uma mensagem inteira; vazio para um algoritmo de saída livre.
pub fn hmac(algo: Algo, key: &[u8], msg: &[u8]) -> Vec<u8> {
    Hmac::new(algo, key)
        .map(|mut h| {
            h.update(msg);
            h.finalize()
        })
        .unwrap_or_default()
}

/// PBKDF2 com HMAC. `algo` não pode ser de saída livre.
pub fn pbkdf2(algo: Algo, password: &[u8], salt: &[u8], iterations: u64, dklen: usize) -> Vec<u8> {
    let Some(keyed) = Hmac::new(algo, password) else { return Vec::new() };
    let mut out = Vec::with_capacity(dklen);
    let mut index: u32 = 1;
    while out.len() < dklen {
        let mut first = keyed.clone();
        first.update(salt);
        first.update(&index.to_be_bytes());
        let mut u = first.finalize();
        let mut t = u.clone();
        for _ in 1..iterations {
            let mut next = keyed.clone();
            next.update(&u);
            u = next.finalize();
            for (a, b) in t.iter_mut().zip(u.iter()) {
                *a ^= b;
            }
        }
        out.extend_from_slice(&t);
        index += 1;
    }
    out.truncate(dklen);
    out
}

fn salsa_quarter(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    x[b] ^= x[a].wrapping_add(x[d]).rotate_left(7);
    x[c] ^= x[b].wrapping_add(x[a]).rotate_left(9);
    x[d] ^= x[c].wrapping_add(x[b]).rotate_left(13);
    x[a] ^= x[d].wrapping_add(x[c]).rotate_left(18);
}

/// Salsa20/8 (RFC 7914, 3).
fn salsa20_8(block: &mut [u32; 16]) {
    let mut x = *block;
    for _ in 0..4 {
        salsa_quarter(&mut x, 0, 4, 8, 12);
        salsa_quarter(&mut x, 5, 9, 13, 1);
        salsa_quarter(&mut x, 10, 14, 2, 6);
        salsa_quarter(&mut x, 15, 3, 7, 11);
        salsa_quarter(&mut x, 0, 1, 2, 3);
        salsa_quarter(&mut x, 5, 6, 7, 4);
        salsa_quarter(&mut x, 10, 11, 8, 9);
        salsa_quarter(&mut x, 15, 12, 13, 14);
    }
    for (b, v) in block.iter_mut().zip(x) {
        *b = b.wrapping_add(v);
    }
}

/// `BlockMix` (RFC 7914, 4): `block` tem `2 * r` blocos de 16 palavras.
fn block_mix(block: &[u32], r: usize) -> Vec<u32> {
    let mut x = [0u32; 16];
    x.copy_from_slice(&block[(2 * r - 1) * 16..2 * r * 16]);
    let mut out = vec![0u32; block.len()];
    for i in 0..2 * r {
        for (a, b) in x.iter_mut().zip(&block[i * 16..(i + 1) * 16]) {
            *a ^= b;
        }
        salsa20_8(&mut x);
        // Os blocos de índice par vão para a primeira metade, os ímpares para a segunda.
        let slot = if i % 2 == 0 { i / 2 } else { r + i / 2 };
        out[slot * 16..(slot + 1) * 16].copy_from_slice(&x);
    }
    out
}

/// `ROMix` (RFC 7914, 5) sobre um bloco de `32 * r` palavras.
fn ro_mix(block: &mut [u32], r: usize, n: usize) {
    let words = block.len();
    let mut v = vec![0u32; n * words];
    let mut x = block.to_vec();
    for i in 0..n {
        v[i * words..(i + 1) * words].copy_from_slice(&x);
        x = block_mix(&x, r);
    }
    for _ in 0..n {
        let last = (2 * r - 1) * 16;
        let j = ((u64::from(x[last + 1]) << 32 | u64::from(x[last])) % n as u64) as usize;
        for (a, b) in x.iter_mut().zip(&v[j * words..(j + 1) * words]) {
            *a ^= b;
        }
        x = block_mix(&x, r);
    }
    block.copy_from_slice(&x);
}

/// scrypt (RFC 7914). Os parâmetros já chegam validados (`n` potência de 2 maior que 1, `r` e `p`
/// positivos e a memória `128 * r * n` dentro do que quem chama permite).
pub fn scrypt(password: &[u8], salt: &[u8], n: u64, r: u32, p: u32, dklen: usize) -> Vec<u8> {
    let (r, p, n) = (r as usize, p as usize, n as usize);
    let words = 32 * r;
    let mut b = pbkdf2(Algo::Sha256, password, salt, 1, p * 128 * r);
    for chunk in b.chunks_mut(128 * r) {
        let mut x: Vec<u32> = chunk
            .chunks(4)
            .map(|w| {
                let mut a = [0u8; 4];
                a.copy_from_slice(w);
                u32::from_le_bytes(a)
            })
            .collect();
        debug_assert_eq!(x.len(), words);
        ro_mix(&mut x, r, n);
        for (dst, w) in chunk.chunks_mut(4).zip(x) {
            dst.copy_from_slice(&w.to_le_bytes());
        }
    }
    pbkdf2(Algo::Sha256, password, &b, 1, dklen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::hex_lower;

    #[test]
    fn hmac_rfc_2202_and_4231() {
        assert_eq!(
            hex_lower(&hmac(Algo::Md5, &[0x0b; 16], b"Hi There")),
            "9294727a3638bb1c13f48ef8158bfc9d"
        );
        assert_eq!(
            hex_lower(&hmac(Algo::Sha256, &[0x0b; 20], b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
        assert_eq!(
            hex_lower(&hmac(Algo::Sha256, b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn pbkdf2_rfc_6070() {
        assert_eq!(hex_lower(&pbkdf2(Algo::Sha1, b"password", b"salt", 1, 20)), "0c60c80f961f0e71f3a9b524af6012062fe037a6");
        assert_eq!(hex_lower(&pbkdf2(Algo::Sha1, b"password", b"salt", 2, 20)), "ea6c014dc72d6f8ccd1ed92ace1d41f0d8de8957");
    }

    #[test]
    fn scrypt_rfc_7914() {
        assert_eq!(
            hex_lower(&scrypt(b"", b"", 16, 1, 1, 64)),
            "77d6576238657b203b19ca42c18a0497f16b4844e3074ae8dfdffa3fede21442fcd0069ded0948f8326a753a0fc81f17e8d3e0fb2e0d3628cf35e20c38d18906"
        );
        assert_eq!(
            hex_lower(&scrypt(b"password", b"NaCl", 1024, 8, 16, 64)),
            "fdbabe1c9d3472007856e7190d01e9fe7c6ad7cbc8237830e77376634b3731622eaf30d92e22a3886ff109279d9830dac727afb94a83ee6d8360cbdfa2cc0640"
        );
    }
}
