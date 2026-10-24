//! As primitivas de curva elíptica do `SubtleCrypto` (ECDSA, ECDH, Ed25519, X25519), sem nada de JavaScript: bytes entram,
//! bytes saem. A chave privada é guardada como o escalar (NIST) ou a semente (Ed25519/X25519), a pública como o ponto SEC1
//! não comprimido (NIST) ou os 32 bytes da curva (25519). `crypto.rs` cuida das mensagens de erro do bun.

use ul_common::hash::Algo;

use crate::runtime::host::host;

/// As curvas que o WebCrypto do bun conhece.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Curve {
    P256,
    P384,
    P521,
    Ed25519,
    X25519,
}

/// O gerador que as crates de curva pedem, sobre o `random_fill` do host (a mesma fonte de `crypto.getRandomValues`).
pub(crate) struct SystemRng;

impl p256::elliptic_curve::rand_core::RngCore for SystemRng {
    fn next_u32(&mut self) -> u32 {
        let mut bytes = [0u8; 4];
        host().random_fill(&mut bytes);
        u32::from_le_bytes(bytes)
    }
    fn next_u64(&mut self) -> u64 {
        let mut bytes = [0u8; 8];
        host().random_fill(&mut bytes);
        u64::from_le_bytes(bytes)
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        host().random_fill(dest);
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), p256::elliptic_curve::rand_core::Error> {
        host().random_fill(dest);
        Ok(())
    }
}

impl p256::elliptic_curve::rand_core::CryptoRng for SystemRng {}

/// O resumo `data` com `hash`, alargado à esquerda com zeros até `size` bytes quando menor (o ECDSA do OpenSSL trata o
/// resumo como inteiro; a crate `ecdsa` recusa resumo menor que metade do campo).
fn padded_digest(hash: Algo, data: &[u8], size: usize) -> Vec<u8> {
    let digest = hash.digest(data);
    if digest.len() >= size {
        return digest;
    }
    let mut padded = vec![0u8; size - digest.len()];
    padded.extend_from_slice(&digest);
    padded
}

/// Gera o módulo `$module` com as operações de uma curva NIST sobre a crate `$krate`.
macro_rules! nist_curve {
    ($module:ident, $krate:ident, $size:expr) => {
        mod $module {
            use super::{padded_digest, SystemRng};
            use $krate::ecdsa::signature::hazmat::{PrehashVerifier, RandomizedPrehashSigner};
            use $krate::ecdsa::{Signature, SigningKey, VerifyingKey};
            use $krate::elliptic_curve::sec1::ToEncodedPoint;
            use $krate::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
            use $krate::{PublicKey, SecretKey};
            use ul_common::hash::Algo;

            pub(super) const SIZE: usize = $size;

            pub(super) fn random_secret() -> Vec<u8> {
                loop {
                    let mut bytes = vec![0u8; SIZE];
                    crate::runtime::host::host().random_fill(&mut bytes);
                    // O P-521 tem 521 bits: o byte alto só carrega um bit.
                    if SIZE == 66 {
                        bytes[0] &= 1;
                    }
                    if SecretKey::from_slice(&bytes).is_ok() {
                        return bytes;
                    }
                }
            }

            pub(super) fn public_of(secret: &[u8]) -> Option<Vec<u8>> {
                let key = SecretKey::from_slice(secret).ok()?;
                Some(key.public_key().to_encoded_point(false).as_bytes().to_vec())
            }

            pub(super) fn normalize_public(raw: &[u8]) -> Option<Vec<u8>> {
                let key = PublicKey::from_sec1_bytes(raw).ok()?;
                Some(key.to_encoded_point(false).as_bytes().to_vec())
            }

            pub(super) fn sign(secret: &[u8], data: &[u8], hash: Algo) -> Option<Vec<u8>> {
                let key = SigningKey::from_slice(secret).ok()?;
                let signature: Signature = key.sign_prehash_with_rng(&mut SystemRng, &padded_digest(hash, data, SIZE)).ok()?;
                Some(signature.to_bytes().to_vec())
            }

            pub(super) fn verify(public: &[u8], data: &[u8], signature: &[u8], hash: Algo) -> bool {
                let Ok(key) = VerifyingKey::from_sec1_bytes(public) else { return false };
                let Ok(signature) = Signature::from_slice(signature) else { return false };
                key.verify_prehash(&padded_digest(hash, data, SIZE), &signature).is_ok()
            }

            pub(super) fn agree(secret: &[u8], public: &[u8]) -> Option<Vec<u8>> {
                let secret = SecretKey::from_slice(secret).ok()?;
                let public = PublicKey::from_sec1_bytes(public).ok()?;
                let shared = $krate::ecdh::diffie_hellman(secret.to_nonzero_scalar(), public.as_affine());
                Some(shared.raw_secret_bytes().to_vec())
            }

            pub(super) fn secret_pkcs8(secret: &[u8]) -> Option<Vec<u8>> {
                Some(SecretKey::from_slice(secret).ok()?.to_pkcs8_der().ok()?.as_bytes().to_vec())
            }

            pub(super) fn parse_pkcs8(der: &[u8]) -> Option<Vec<u8>> {
                Some(SecretKey::from_pkcs8_der(der).ok()?.to_bytes().to_vec())
            }

            pub(super) fn public_spki(public: &[u8]) -> Option<Vec<u8>> {
                Some(PublicKey::from_sec1_bytes(public).ok()?.to_public_key_der().ok()?.as_bytes().to_vec())
            }

            pub(super) fn parse_spki(der: &[u8]) -> Option<Vec<u8>> {
                Some(PublicKey::from_public_key_der(der).ok()?.to_encoded_point(false).as_bytes().to_vec())
            }
        }
    };
}

nist_curve!(p256_ops, p256, 32);
nist_curve!(p384_ops, p384, 48);
nist_curve!(p521_ops, p521, 66);

/// Despacha `$call` para o módulo da curva NIST de `$curve`; os dois de 25519 caem em `$other`.
macro_rules! on_nist {
    ($curve:expr, $module:ident => $call:expr, _ => $other:expr) => {
        match $curve {
            Curve::P256 => {
                use p256_ops as $module;
                $call
            }
            Curve::P384 => {
                use p384_ops as $module;
                $call
            }
            Curve::P521 => {
                use p521_ops as $module;
                $call
            }
            _ => $other,
        }
    };
}

/// O prefixo DER do PKCS#8 (`PrivateKeyInfo` v0, OID e `OCTET STRING` dentro de `OCTET STRING`) das curvas 25519.
fn okp_pkcs8_prefix(oid_last: u8) -> [u8; 16] {
    [0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, oid_last, 0x04, 0x22, 0x04, 0x20]
}

/// O prefixo DER do SPKI das curvas 25519.
fn okp_spki_prefix(oid_last: u8) -> [u8; 12] {
    [0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, oid_last, 0x03, 0x21, 0x00]
}

impl Curve {
    /// `namedCurve` do WebCrypto (as curvas NIST).
    pub(crate) fn from_named(name: &str) -> Option<Curve> {
        match name {
            "P-256" => Some(Curve::P256),
            "P-384" => Some(Curve::P384),
            "P-521" => Some(Curve::P521),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Curve::P256 => "P-256",
            Curve::P384 => "P-384",
            Curve::P521 => "P-521",
            Curve::Ed25519 => "Ed25519",
            Curve::X25519 => "X25519",
        }
    }

    pub(crate) fn is_nist(self) -> bool {
        matches!(self, Curve::P256 | Curve::P384 | Curve::P521)
    }

    /// O tamanho em bytes de um escalar, de uma coordenada e do segredo de ECDH.
    pub(crate) fn field_size(self) -> usize {
        on_nist!(self, ops => ops::SIZE, _ => 32)
    }

    /// O último byte do OID das curvas 25519 (`1.3.101.112` é Ed25519, `1.3.101.110` é X25519).
    fn oid_last(self) -> u8 {
        if self == Curve::Ed25519 { 0x70 } else { 0x6e }
    }

    /// Uma chave privada nova (escalar ou semente).
    pub(crate) fn generate(self) -> Vec<u8> {
        on_nist!(self, ops => ops::random_secret(), _ => {
            let mut seed = vec![0u8; 32];
            host().random_fill(&mut seed);
            seed
        })
    }

    /// A chave pública (ponto não comprimido ou 32 bytes) da privada; `None` se a privada é inválida.
    pub(crate) fn public_of(self, secret: &[u8]) -> Option<Vec<u8>> {
        on_nist!(self, ops => ops::public_of(secret), _ => {
            let seed: [u8; 32] = secret.try_into().ok()?;
            Some(match self {
                Curve::Ed25519 => ed25519_dalek::SigningKey::from_bytes(&seed).verifying_key().to_bytes().to_vec(),
                _ => x25519_dalek::x25519(seed, x25519_dalek::X25519_BASEPOINT_BYTES).to_vec(),
            })
        })
    }

    /// Valida os bytes de uma chave pública e os devolve na forma guardada (ponto NIST não comprimido).
    pub(crate) fn normalize_public(self, raw: &[u8]) -> Option<Vec<u8>> {
        on_nist!(self, ops => ops::normalize_public(raw), _ => {
            let bytes: [u8; 32] = raw.try_into().ok()?;
            match self {
                Curve::Ed25519 => ed25519_dalek::VerifyingKey::from_bytes(&bytes).ok().map(|_| bytes.to_vec()),
                _ => Some(bytes.to_vec()),
            }
        })
    }

    /// Valida uma chave privada (escalar dentro da ordem, semente de 32 bytes).
    pub(crate) fn check_secret(self, secret: &[u8]) -> bool {
        self.public_of(secret).is_some()
    }

    /// ECDSA (NIST) ou Ed25519 (`hash` é ignorado).
    pub(crate) fn sign(self, secret: &[u8], data: &[u8], hash: Algo) -> Option<Vec<u8>> {
        on_nist!(self, ops => ops::sign(secret, data, hash), _ => {
            use ed25519_dalek::Signer;
            let seed: [u8; 32] = secret.try_into().ok()?;
            Some(ed25519_dalek::SigningKey::from_bytes(&seed).sign(data).to_bytes().to_vec())
        })
    }

    pub(crate) fn verify(self, public: &[u8], data: &[u8], signature: &[u8], hash: Algo) -> bool {
        on_nist!(self, ops => ops::verify(public, data, signature, hash), _ => {
            use ed25519_dalek::Verifier;
            let (Ok(public), Ok(signature)) = (<[u8; 32]>::try_from(public), <[u8; 64]>::try_from(signature)) else { return false };
            let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(&public) else { return false };
            key.verify(data, &ed25519_dalek::Signature::from_bytes(&signature)).is_ok()
        })
    }

    /// O segredo compartilhado de ECDH/X25519 (`None` se a chave pública não é um ponto válido).
    pub(crate) fn agree(self, secret: &[u8], public: &[u8]) -> Option<Vec<u8>> {
        on_nist!(self, ops => ops::agree(secret, public), _ => {
            let (secret, public): ([u8; 32], [u8; 32]) = (secret.try_into().ok()?, public.try_into().ok()?);
            Some(x25519_dalek::x25519(secret, public).to_vec())
        })
    }

    pub(crate) fn secret_pkcs8(self, secret: &[u8]) -> Option<Vec<u8>> {
        on_nist!(self, ops => ops::secret_pkcs8(secret), _ => {
            let mut der = okp_pkcs8_prefix(self.oid_last()).to_vec();
            der.extend_from_slice(secret);
            Some(der)
        })
    }

    /// Lê um PKCS#8 da curva: `Err` traz a mensagem de `DataError` do bun (outra curva é `Named curve mismatch` nas NIST e
    /// `Invalid key type` nas 25519).
    pub(crate) fn parse_pkcs8(self, der: &[u8]) -> Result<Vec<u8>, &'static str> {
        self.parse_der(
            der,
            |last| okp_pkcs8_prefix(last).to_vec(),
            |curve| on_nist!(curve, ops => ops::parse_pkcs8(der), _ => None),
        )
    }

    /// O miolo comum de `parse_pkcs8` e `parse_spki`: nas 25519 o DER é o prefixo mais 32 bytes (`prefix` dá o prefixo
    /// do OID pedido); nas NIST `nist` lê o DER como a curva dada, e achar numa outra curva é `Named curve mismatch`.
    fn parse_der(
        self,
        der: &[u8],
        prefix: impl Fn(u8) -> Vec<u8>,
        nist: impl Fn(Curve) -> Option<Vec<u8>>,
    ) -> Result<Vec<u8>, &'static str> {
        if !self.is_nist() {
            let own = prefix(self.oid_last());
            let other = prefix(if self == Curve::Ed25519 { 0x6e } else { 0x70 });
            return match (der.strip_prefix(&own[..]), der.starts_with(&other)) {
                (Some(key), _) if key.len() == 32 => Ok(key.to_vec()),
                (None, true) if der.len() == other.len() + 32 => Err("Invalid key type"),
                _ => Err("Invalid keyData"),
            };
        }
        if let Some(key) = nist(self) {
            return Ok(key);
        }
        let others = [Curve::P256, Curve::P384, Curve::P521];
        if others.iter().any(|curve| *curve != self && nist(*curve).is_some()) {
            return Err("Named curve mismatch");
        }
        Err("Invalid keyData")
    }

    pub(crate) fn public_spki(self, public: &[u8]) -> Option<Vec<u8>> {
        on_nist!(self, ops => ops::public_spki(public), _ => {
            let mut der = okp_spki_prefix(self.oid_last()).to_vec();
            der.extend_from_slice(public);
            Some(der)
        })
    }

    /// Lê um SPKI da curva; `Err` traz a mensagem de `DataError` do bun.
    pub(crate) fn parse_spki(self, der: &[u8]) -> Result<Vec<u8>, &'static str> {
        self.parse_der(
            der,
            |last| okp_spki_prefix(last).to_vec(),
            |curve| on_nist!(curve, ops => ops::parse_spki(der), _ => None),
        )
    }
}
