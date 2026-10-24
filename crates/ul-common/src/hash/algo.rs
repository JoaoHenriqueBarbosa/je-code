//! `Algo` e `Hasher`: os algoritmos de resumo do módulo `hash` escolhidos por nome, com a mesma interface
//! incremental, no vocabulário do OpenSSL e do `hashlib`.

use super::blake2::{self, Params};
use super::keccak::Keccak;
use super::ripemd::Ripemd160;
use super::sha512::Sha512;
use super::sm3::Sm3;
use super::{Md4, Md5, Sha1, Sha256};

/// Um algoritmo de resumo. `Blake2b` e `Blake2s` levam o tamanho do resumo em bytes e `Sha3` o tamanho do
/// resumo (28, 32, 48 ou 64); `Shake128` e `Shake256` são de saída livre (sem tamanho fixo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algo {
    Md4,
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
    Sha512Trunc224,
    Sha512Trunc256,
    Sha3(usize),
    Shake128,
    Shake256,
    Blake2b(usize),
    Blake2s(usize),
    Ripemd160,
    Sm3,
    /// A concatenação de MD5 e SHA-1 (36 bytes), o `md5-sha1` do OpenSSL.
    Md5Sha1,
}

impl Algo {
    /// O algoritmo pelo nome do OpenSSL ou do `hashlib`, sem diferenciar maiúsculas nem `-`, `_` e `/`
    /// (`sha3_256`, `SHA3-256`, `sha-512/224`, `md5-sha1`).
    pub fn from_name(name: &str) -> Option<Algo> {
        let compact: String = name.chars().filter(|c| !matches!(c, '-' | '_' | '/')).map(|c| c.to_ascii_lowercase()).collect();
        Some(match compact.as_str() {
            "md4" => Algo::Md4,
            "md5" => Algo::Md5,
            "sha1" => Algo::Sha1,
            "sha224" | "sha2224" => Algo::Sha224,
            "sha256" | "sha2256" => Algo::Sha256,
            "sha384" | "sha2384" => Algo::Sha384,
            "sha512" | "sha2512" => Algo::Sha512,
            "sha512224" | "sha2512224" => Algo::Sha512Trunc224,
            "sha512256" | "sha2512256" => Algo::Sha512Trunc256,
            "sha3224" => Algo::Sha3(28),
            "sha3256" => Algo::Sha3(32),
            "sha3384" => Algo::Sha3(48),
            "sha3512" => Algo::Sha3(64),
            "shake128" => Algo::Shake128,
            "shake256" => Algo::Shake256,
            "blake2b" | "blake2b512" => Algo::Blake2b(64),
            "blake2s" | "blake2s256" => Algo::Blake2s(32),
            "ripemd160" | "rmd160" => Algo::Ripemd160,
            "sm3" => Algo::Sm3,
            "md5sha1" => Algo::Md5Sha1,
            _ => return None,
        })
    }

    /// O nome que o `hashlib` mostra em `.name`.
    pub fn name(&self) -> &'static str {
        match self {
            Algo::Md4 => "md4",
            Algo::Md5 => "md5",
            Algo::Sha1 => "sha1",
            Algo::Sha224 => "sha224",
            Algo::Sha256 => "sha256",
            Algo::Sha384 => "sha384",
            Algo::Sha512 => "sha512",
            Algo::Sha512Trunc224 => "sha512_224",
            Algo::Sha512Trunc256 => "sha512_256",
            Algo::Sha3(28) => "sha3_224",
            Algo::Sha3(32) => "sha3_256",
            Algo::Sha3(48) => "sha3_384",
            Algo::Sha3(_) => "sha3_512",
            Algo::Shake128 => "shake_128",
            Algo::Shake256 => "shake_256",
            Algo::Blake2b(_) => "blake2b",
            Algo::Blake2s(_) => "blake2s",
            Algo::Ripemd160 => "ripemd160",
            Algo::Sm3 => "sm3",
            Algo::Md5Sha1 => "md5-sha1",
        }
    }

    /// `(bytes do resumo, bytes por bloco)`: o resumo é zero nos de saída livre; o bloco é o `block_size` do `hashlib`
    /// (a taxa nas esponjas).
    fn sizes(&self) -> (usize, usize) {
        match self {
            Algo::Md4 | Algo::Md5 => (16, 64),
            Algo::Sha1 | Algo::Ripemd160 => (20, 64),
            Algo::Sha224 => (28, 64),
            Algo::Sha256 | Algo::Sm3 => (32, 64),
            Algo::Md5Sha1 => (36, 64),
            Algo::Sha384 => (48, 128),
            Algo::Sha512 => (64, 128),
            Algo::Sha512Trunc224 => (28, 128),
            Algo::Sha512Trunc256 => (32, 128),
            Algo::Shake128 => (0, 168),
            Algo::Shake256 => (0, 136),
            Algo::Blake2b(n) => (*n, blake2::b::BLOCK),
            Algo::Blake2s(n) => (*n, blake2::s::BLOCK),
            Algo::Sha3(n) => (*n, 200 - 2 * n),
        }
    }

    /// Bytes do resumo; zero nos de saída livre.
    pub fn digest_size(&self) -> usize {
        self.sizes().0
    }

    /// Bytes por bloco (o `block_size` do `hashlib`; a taxa nas esponjas).
    pub fn block_size(&self) -> usize {
        self.sizes().1
    }

    /// O algoritmo é de saída livre (SHAKE): o tamanho do resumo vem de quem pede.
    pub fn is_xof(&self) -> bool {
        matches!(self, Algo::Shake128 | Algo::Shake256)
    }

    pub fn hasher(&self) -> Hasher {
        match self {
            Algo::Md4 => Hasher::Md4(Md4::new()),
            Algo::Md5 => Hasher::Md5(Md5::new()),
            Algo::Sha1 => Hasher::Sha1(Sha1::new()),
            Algo::Sha224 => Hasher::Sha256(Sha256::new_224(), 28),
            Algo::Sha256 => Hasher::Sha256(Sha256::new(), 32),
            Algo::Sha384 => Hasher::Sha512(Sha512::new_384()),
            Algo::Sha512 => Hasher::Sha512(Sha512::new()),
            Algo::Sha512Trunc224 => Hasher::Sha512(Sha512::new_t(224)),
            Algo::Sha512Trunc256 => Hasher::Sha512(Sha512::new_t(256)),
            Algo::Sha3(n) => Hasher::Keccak(Keccak::sha3(*n), *n),
            Algo::Shake128 => Hasher::Keccak(Keccak::shake(16), 0),
            Algo::Shake256 => Hasher::Keccak(Keccak::shake(32), 0),
            Algo::Blake2b(n) => Hasher::Blake2b(blake2::b::State::new(&Params::sequential(*n))),
            Algo::Blake2s(n) => Hasher::Blake2s(blake2::s::State::new(&Params::sequential(*n))),
            Algo::Ripemd160 => Hasher::Ripemd160(Ripemd160::new()),
            Algo::Sm3 => Hasher::Sm3(Sm3::new()),
            Algo::Md5Sha1 => Hasher::Md5Sha1(Md5::new(), Sha1::new()),
        }
    }

    /// O resumo de um buffer.
    pub fn digest(&self, data: &[u8]) -> Vec<u8> {
        let mut h = self.hasher();
        h.update(data);
        h.finalize(self.digest_size())
    }
}

/// Um resumo em andamento, de qualquer algoritmo de [`Algo`] (e dos BLAKE2 com parâmetros). Clonar copia o
/// estado: é o `copy()` do `hashlib`.
#[derive(Clone)]
pub enum Hasher {
    Md4(Md4),
    Md5(Md5),
    Sha1(Sha1),
    /// SHA-256 ou SHA-224, com o tamanho do resumo em bytes.
    Sha256(Sha256, usize),
    Sha512(Sha512),
    /// SHA-3 ou SHAKE; o número é o tamanho do resumo (zero no SHAKE).
    Keccak(Keccak, usize),
    Blake2b(blake2::b::State),
    Blake2s(blake2::s::State),
    Ripemd160(Ripemd160),
    Sm3(Sm3),
    Md5Sha1(Md5, Sha1),
}

impl Hasher {
    pub fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::Md4(h) => h.update(data),
            Hasher::Md5(h) => h.update(data),
            Hasher::Sha1(h) => h.update(data),
            Hasher::Sha256(h, _) => h.update(data),
            Hasher::Sha512(h) => h.update(data),
            Hasher::Keccak(h, _) => h.update(data),
            Hasher::Blake2b(h) => h.update(data),
            Hasher::Blake2s(h) => h.update(data),
            Hasher::Ripemd160(h) => h.update(data),
            Hasher::Sm3(h) => h.update(data),
            Hasher::Md5Sha1(m, s) => {
                m.update(data);
                s.update(data);
            }
        }
    }

    /// O resumo do que foi alimentado até agora, sem consumir o estado. `xof_len` só vale nos SHAKE (o
    /// tamanho pedido); nos demais o tamanho é o do algoritmo.
    pub fn finalize(&self, xof_len: usize) -> Vec<u8> {
        match self.clone() {
            Hasher::Md4(h) => h.finalize().to_vec(),
            Hasher::Md5(h) => h.finalize().to_vec(),
            Hasher::Sha1(h) => h.finalize().to_vec(),
            Hasher::Sha256(h, n) => h.finalize()[..n].to_vec(),
            Hasher::Sha512(h) => h.finalize(),
            Hasher::Keccak(h, 0) => h.finalize(xof_len),
            Hasher::Keccak(h, n) => h.finalize(n),
            Hasher::Blake2b(h) => h.finalize(),
            Hasher::Blake2s(h) => h.finalize(),
            Hasher::Ripemd160(h) => h.finalize().to_vec(),
            Hasher::Sm3(h) => h.finalize().to_vec(),
            Hasher::Md5Sha1(m, s) => {
                let mut out = m.finalize().to_vec();
                out.extend_from_slice(&s.finalize());
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::hex_lower;

    #[test]
    fn names_resolve_with_aliases() {
        assert_eq!(Algo::from_name("SHA3-256"), Some(Algo::Sha3(32)));
        assert_eq!(Algo::from_name("sha-512/224"), Some(Algo::Sha512Trunc224));
        assert_eq!(Algo::from_name("md5-sha1"), Some(Algo::Md5Sha1));
        assert_eq!(Algo::from_name("nope"), None);
    }

    #[test]
    fn digests_through_the_enum() {
        assert_eq!(hex_lower(&Algo::Md4.digest(b"abc")), "a448017aaf21d8525fc10ae87aa6729d");
        assert_eq!(hex_lower(&Algo::Md5.digest(b"abc")), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(hex_lower(&Algo::Sha224.digest(b"abc")), "23097d223405d8228642a477bda255b32aadbce4bda0b3f7e36c9da7");
        assert_eq!(Algo::Md5Sha1.digest(b"abc").len(), 36);
        let mut h = Algo::Shake128.hasher();
        h.update(b"");
        assert_eq!(hex_lower(&h.finalize(16)), "7f9c2ba4e88f827d616045507605853e");
        for algo in [Algo::Sha3(28), Algo::Sha512, Algo::Blake2b(20), Algo::Blake2s(32), Algo::Sm3, Algo::Ripemd160] {
            assert_eq!(algo.digest(b"abc").len(), algo.digest_size());
        }
    }
}
