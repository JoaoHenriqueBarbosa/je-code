//! ML-KEM (FIPS 203) e ML-DSA (FIPS 204) do `SubtleCrypto`, sem nada de JavaScript: bytes entram, bytes saem. O plano e as
//! medições contra o bun 1.4.2 estão em `wip/notes/crypto-pq-plan.md`. O bun não suporta ML-KEM-512, então o nome nem é
//! reconhecido aqui (cai em `Unrecognized algorithm name`). O DER de spki e pkcs8 é feito à mão: o formato é fixo, só varia
//! o OID e o tamanho.

use ml_kem::{Decapsulate, KeyExport, MlKem1024, MlKem768};
use crate::runtime::host::host;

/// Roda `$body` com `$p` sendo o conjunto de parâmetros ML-KEM do algoritmo; `None` nos de assinatura.
macro_rules! with_kem {
    ($algorithm:expr, $p:ident => $body:block) => {
        match $algorithm {
            PqAlgorithm::MlKem768 => {
                type $p = MlKem768;
                $body
            }
            PqAlgorithm::MlKem1024 => {
                type $p = MlKem1024;
                $body
            }
            _ => None,
        }
    };
}

/// Os cinco parâmetros que o bun reconhece.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PqAlgorithm {
    MlKem768,
    MlKem1024,
    MlDsa44,
    MlDsa65,
    MlDsa87,
}

/// Todos, na ordem em que o plano os lista.
pub(crate) const ALL: [PqAlgorithm; 5] =
    [PqAlgorithm::MlKem768, PqAlgorithm::MlKem1024, PqAlgorithm::MlDsa44, PqAlgorithm::MlDsa65, PqAlgorithm::MlDsa87];

/// Posição de cada uso em `KEY_USAGES` de `crypto.rs` (bit = posição).
pub(crate) const USAGE_SIGN: u16 = 1 << 2;
pub(crate) const USAGE_VERIFY: u16 = 1 << 3;
pub(crate) const USAGE_ENCAPSULATE_KEY: u16 = 1 << 8;
pub(crate) const USAGE_ENCAPSULATE_BITS: u16 = 1 << 9;
pub(crate) const USAGE_DECAPSULATE_KEY: u16 = 1 << 10;
pub(crate) const USAGE_DECAPSULATE_BITS: u16 = 1 << 11;

/// Por que o DER não serve: `Invalid keyData` (malformado) ou `Invalid key type` (bem formado, de outro parâmetro).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DerError {
    InvalidKeyData,
    InvalidKeyType,
}

/// Por que os usos pedidos não servem para gerar o par.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum UsageError {
    /// `Unsupported key usage for an ML-KEM-768 key`.
    Unsupported,
    /// `Usages cannot be empty when creating a key.`
    Empty,
}

impl PqAlgorithm {
    /// O parâmetro de um nome já em minúsculas (`ml-kem-512` não existe).
    pub(crate) fn from_lower_name(name: &str) -> Option<Self> {
        ALL.into_iter().find(|algorithm| algorithm.name().eq_ignore_ascii_case(name))
    }

    /// A grafia canônica, a que sai em `algorithm.name`.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::MlKem768 => "ML-KEM-768",
            Self::MlKem1024 => "ML-KEM-1024",
            Self::MlDsa44 => "ML-DSA-44",
            Self::MlDsa65 => "ML-DSA-65",
            Self::MlDsa87 => "ML-DSA-87",
        }
    }

    pub(crate) fn is_kem(self) -> bool {
        matches!(self, Self::MlKem768 | Self::MlKem1024)
    }

    /// Bytes da semente: 64 no ML-KEM (`d || z`), 32 no ML-DSA (`xi`).
    pub(crate) fn seed_len(self) -> usize {
        if self.is_kem() { 64 } else { 32 }
    }

    /// Bytes de `raw-public`.
    pub(crate) fn public_len(self) -> usize {
        match self {
            Self::MlKem768 => 1184,
            Self::MlKem1024 => 1568,
            Self::MlDsa44 => 1312,
            Self::MlDsa65 => 1952,
            Self::MlDsa87 => 2592,
        }
    }

    /// O OID sob `2.16.840.1.101.3.4`: o arco (4 nos KEMs, 3 nas assinaturas) e a folha.
    fn oid_tail(self) -> [u8; 2] {
        match self {
            Self::MlKem768 => [0x04, 0x02],
            Self::MlKem1024 => [0x04, 0x03],
            Self::MlDsa44 => [0x03, 0x11],
            Self::MlDsa65 => [0x03, 0x12],
            Self::MlDsa87 => [0x03, 0x13],
        }
    }

    /// O `AlgorithmIdentifier` sem parâmetros: `30 0b 06 09 60 86 48 01 65 03 04 <arco> <folha>`.
    fn algorithm_identifier(self) -> Vec<u8> {
        let mut bytes = vec![0x30, 0x0b, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04];
        bytes.extend_from_slice(&self.oid_tail());
        bytes
    }

    /// O que vem antes da semente no pkcs8: `30 LL 02 01 00 <alg id> 04 NN 80 SS`.
    fn pkcs8_prefix(self) -> Vec<u8> {
        let seed = self.seed_len() as u8;
        let mut bytes = vec![0x30, seed + 20, 0x02, 0x01, 0x00];
        bytes.extend(self.algorithm_identifier());
        bytes.extend_from_slice(&[0x04, seed + 2, 0x80, seed]);
        bytes
    }

    /// O que vem antes da chave no spki: `30 82 LLLL <alg id> 03 82 BBBB 00`.
    fn spki_prefix(self) -> Vec<u8> {
        let public = self.public_len();
        let mut bytes = vec![0x30, 0x82];
        bytes.extend_from_slice(&((public + 18) as u16).to_be_bytes());
        bytes.extend(self.algorithm_identifier());
        bytes.extend_from_slice(&[0x03, 0x82]);
        bytes.extend_from_slice(&((public + 1) as u16).to_be_bytes());
        bytes.push(0x00);
        bytes
    }

    /// `spki` da chave pública bruta.
    pub(crate) fn encode_spki(self, raw_public: &[u8]) -> Vec<u8> {
        let mut bytes = self.spki_prefix();
        bytes.extend_from_slice(raw_public);
        bytes
    }

    /// `pkcs8` da semente (só a semente, sem parâmetros).
    pub(crate) fn encode_pkcs8(self, seed: &[u8]) -> Vec<u8> {
        let mut bytes = self.pkcs8_prefix();
        bytes.extend_from_slice(seed);
        bytes
    }

    /// A chave pública bruta de um `spki` deste parâmetro.
    pub(crate) fn decode_spki(self, bytes: &[u8]) -> Result<Vec<u8>, DerError> {
        decode(self, bytes, Self::spki_prefix, Self::public_len)
    }

    /// A semente de um `pkcs8` deste parâmetro.
    pub(crate) fn decode_pkcs8(self, bytes: &[u8]) -> Result<Vec<u8>, DerError> {
        decode(self, bytes, Self::pkcs8_prefix, Self::seed_len)
    }

    /// A chave pública bruta derivada da semente (`d`, `z` no ML-KEM; `xi` no ML-DSA). `None` se o tamanho não é o da semente.
    pub(crate) fn public_from_seed(self, seed: &[u8]) -> Option<Vec<u8>> {
        if seed.len() != self.seed_len() {
            return None;
        }
        Some(match self {
            Self::MlKem768 | Self::MlKem1024 => with_kem!(self, P => {
                let key = ml_kem::DecapsulationKey::<P>::from_seed(ml_kem::Seed::from_fn(|index| seed[index]));
                Some(key.encapsulation_key().to_bytes().as_slice().to_vec())
            })?,
            Self::MlDsa44 => dsa_public::<ml_dsa::MlDsa44>(seed),
            Self::MlDsa65 => dsa_public::<ml_dsa::MlDsa65>(seed),
            Self::MlDsa87 => dsa_public::<ml_dsa::MlDsa87>(seed),
        })
    }

    /// Encapsula um segredo novo para a chave pública bruta: `(ciphertext, segredo de 32 bytes)`. `None` se o parâmetro não é
    /// KEM ou a chave não decodifica.
    pub(crate) fn encapsulate(self, public: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
        with_kem!(self, P => {
            let key = ml_kem::EncapsulationKey::<P>::new(&public.try_into().ok()?).ok()?;
            let mut message = [0u8; 32];
            host().random_fill(&mut message);
            let (ciphertext, shared) = key.encapsulate_deterministic(&ml_kem::B32::from(message));
            Some((ciphertext.as_slice().to_vec(), shared.as_slice().to_vec()))
        })
    }

    /// Decapsula com a chave privada da semente. `None` se o `ciphertext` não tem o tamanho do parâmetro; um `ciphertext` do
    /// tamanho certo e adulterado devolve um segredo diferente e determinístico (rejeição implícita do FIPS 203).
    pub(crate) fn decapsulate(self, seed: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
        with_kem!(self, P => {
            let seed = ml_kem::Seed::from_fn(|index| seed[index]);
            let key = ml_kem::DecapsulationKey::<P>::from_seed(seed);
            key.decapsulate_slice(ciphertext).ok().map(|shared| shared.as_slice().to_vec())
        })
    }

    /// Assina `data` com a chave privada da semente (FIPS 204, `ML-DSA.Sign` com aleatoriedade): `M' = 0x00 || len(ctx) || ctx
    /// || M` e 32 bytes aleatórios do host na semente de mascaramento. `None` se o `context` passa de 255 bytes ou o
    /// parâmetro não é de assinatura.
    pub(crate) fn sign(self, seed: &[u8], data: &[u8], context: &[u8]) -> Option<Vec<u8>> {
        if context.len() > 255 || self.is_kem() || seed.len() != 32 {
            return None;
        }
        Some(match self {
            Self::MlDsa44 => dsa_sign::<ml_dsa::MlDsa44>(seed, data, context),
            Self::MlDsa65 => dsa_sign::<ml_dsa::MlDsa65>(seed, data, context),
            _ => dsa_sign::<ml_dsa::MlDsa87>(seed, data, context),
        })
    }

    /// Verifica `signature` (`ML-DSA.Verify`) com a chave pública bruta. `None` se o `context` passa de 255 bytes; assinatura
    /// de tamanho errado ou adulterada é `Some(false)`, nunca erro.
    pub(crate) fn verify(self, public: &[u8], data: &[u8], context: &[u8], signature: &[u8]) -> Option<bool> {
        if context.len() > 255 || self.is_kem() {
            return None;
        }
        Some(match self {
            Self::MlDsa44 => dsa_verify::<ml_dsa::MlDsa44>(public, data, context, signature),
            Self::MlDsa65 => dsa_verify::<ml_dsa::MlDsa65>(public, data, context, signature),
            _ => dsa_verify::<ml_dsa::MlDsa87>(public, data, context, signature),
        })
    }

    /// Os usos de cada metade do par: `(pública, privada)`.
    fn usage_masks(self) -> (u16, u16) {
        if self.is_kem() {
            (USAGE_ENCAPSULATE_KEY | USAGE_ENCAPSULATE_BITS, USAGE_DECAPSULATE_KEY | USAGE_DECAPSULATE_BITS)
        } else {
            (USAGE_VERIFY, USAGE_SIGN)
        }
    }

    /// Os usos válidos de uma chave dela, conforme seja privada ou pública.
    pub(crate) fn allowed_usages(self, private: bool) -> u16 {
        let (public, secret) = self.usage_masks();
        if private { secret } else { public }
    }

    /// Divide os usos pedidos no `generateKey` em `(pública, privada)`. Uso fora do conjunto do parâmetro é
    /// `Unsupported` (checado antes do vazio); privada sem nenhum uso é `Empty`.
    pub(crate) fn split_generate_usages(self, requested: u16) -> Result<(u16, u16), UsageError> {
        let (public, secret) = self.usage_masks();
        if requested & !(public | secret) != 0 {
            return Err(UsageError::Unsupported);
        }
        if requested & secret == 0 {
            return Err(UsageError::Empty);
        }
        Ok((requested & public, requested & secret))
    }

    /// Os usos de `getPublicKey`: só os da pública.
    pub(crate) fn check_public_usages(self, requested: u16) -> Result<u16, UsageError> {
        if requested & !self.allowed_usages(false) != 0 { Err(UsageError::Unsupported) } else { Ok(requested) }
    }

    /// A mensagem de `Unsupported key usage` na geração (`an ML-KEM-768 key`) ou na importação (`a ML-KEM-768 key`).
    pub(crate) fn unsupported_usage_message(self, importing: bool) -> String {
        format!("Unsupported key usage for {} {} key", if importing { "a" } else { "an" }, self.name())
    }

    /// Uma semente nova do tamanho do parâmetro, do mesmo gerador do resto do `SubtleCrypto`.
    pub(crate) fn generate_seed(self) -> Vec<u8> {
        let mut seed = vec![0u8; self.seed_len()];
        host().random_fill(&mut seed);
        seed
    }

    /// A mensagem de `exportKey` num formato que a chave não tem (`private key using spki format`).
    pub(crate) fn unable_to_export_message(self, private: bool, format: &str) -> String {
        format!("Unable to export {} {} key using {format} format", self.name(), if private { "private" } else { "public" })
    }

    /// Se a chave (privada ou pública) tem esse formato de exportação: pública em `raw-public`, `spki`, `jwk`; privada em
    /// `raw-seed`, `pkcs8`, `jwk`. `raw` e `raw-secret` nunca.
    pub(crate) fn exports_format(private: bool, format: &str) -> bool {
        match format {
            "jwk" => true,
            "raw-public" | "spki" => !private,
            "raw-seed" | "pkcs8" => private,
            _ => false,
        }
    }
}

/// Confere um DER de prefixo fixo: serve ao `algorithm`, ou é de outro parâmetro (`InvalidKeyType`), ou é lixo.
fn decode(
    algorithm: PqAlgorithm,
    bytes: &[u8],
    prefix: fn(PqAlgorithm) -> Vec<u8>,
    payload_len: fn(PqAlgorithm) -> usize,
) -> Result<Vec<u8>, DerError> {
    let matches = |candidate: PqAlgorithm| {
        let head = prefix(candidate);
        bytes.len() == head.len() + payload_len(candidate) && bytes.starts_with(&head)
    };
    if matches(algorithm) {
        let head = prefix(algorithm).len();
        return Ok(bytes[head..].to_vec());
    }
    if ALL.into_iter().any(matches) { Err(DerError::InvalidKeyType) } else { Err(DerError::InvalidKeyData) }
}

fn dsa_public<P>(seed: &[u8]) -> Vec<u8>
where
    P: ml_dsa::MlDsaParams,
{
    let seed = ml_dsa::Seed::from_fn(|index| seed[index]);
    ml_dsa::SigningKey::<P>::from_seed(&seed).expanded_key().verifying_key().encode().as_slice().to_vec()
}

fn dsa_sign<P>(seed: &[u8], data: &[u8], context: &[u8]) -> Vec<u8>
where
    P: ml_dsa::MlDsaParams,
{
    let seed = ml_dsa::Seed::from_fn(|index| seed[index]);
    let mut random = [0u8; 32];
    host().random_fill(&mut random);
    let random = ml_dsa::B32::from_fn(|index| random[index]);
    let key = ml_dsa::SigningKey::<P>::from_seed(&seed);
    let signature = key.expanded_key().sign_internal(&[&[0u8], &[context.len() as u8], context, data], &random);
    signature.encode().as_slice().to_vec()
}

fn dsa_verify<P>(public: &[u8], data: &[u8], context: &[u8], signature: &[u8]) -> bool
where
    P: ml_dsa::MlDsaParams,
{
    let Ok(encoded) = public.try_into() else { return false };
    let Ok(signature) = ml_dsa::Signature::<P>::try_from(signature) else { return false };
    ml_dsa::VerifyingKey::<P>::decode(&encoded).verify_with_context(data, context, &signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn spki_prefixes_match_bun() {
        assert_eq!(hex(&PqAlgorithm::MlKem768.spki_prefix()), "308204b2300b0609608648016503040402038204a100");
        assert_eq!(hex(&PqAlgorithm::MlKem1024.spki_prefix()), "30820632300b06096086480165030404030382062100");
        assert_eq!(hex(&PqAlgorithm::MlDsa44.spki_prefix()), "30820532300b06096086480165030403110382052100");
        assert_eq!(hex(&PqAlgorithm::MlDsa65.spki_prefix()), "308207b2300b0609608648016503040312038207a100");
        assert_eq!(hex(&PqAlgorithm::MlDsa87.spki_prefix()), "30820a32300b060960864801650304031303820a2100");
    }

    #[test]
    fn pkcs8_prefixes_match_bun() {
        assert_eq!(hex(&PqAlgorithm::MlKem768.pkcs8_prefix()), "3054020100300b060960864801650304040204428040");
        assert_eq!(hex(&PqAlgorithm::MlKem1024.pkcs8_prefix()), "3054020100300b060960864801650304040304428040");
        assert_eq!(hex(&PqAlgorithm::MlDsa44.pkcs8_prefix()), "3034020100300b060960864801650304031104228020");
    }

    #[test]
    fn ml_kem_512_is_unknown() {
        assert_eq!(PqAlgorithm::from_lower_name("ml-kem-512"), None);
        assert_eq!(PqAlgorithm::from_lower_name("ml-kem-768"), Some(PqAlgorithm::MlKem768));
    }

    #[test]
    fn der_round_trip_and_foreign() {
        let seed = vec![7u8; 32];
        let pkcs8 = PqAlgorithm::MlDsa65.encode_pkcs8(&seed);
        assert_eq!(pkcs8.len(), 54);
        assert_eq!(PqAlgorithm::MlDsa65.decode_pkcs8(&pkcs8), Ok(seed));
        assert_eq!(PqAlgorithm::MlDsa44.decode_pkcs8(&pkcs8), Err(DerError::InvalidKeyType));
        assert_eq!(PqAlgorithm::MlDsa65.decode_pkcs8(&pkcs8[..53]), Err(DerError::InvalidKeyData));
    }

    #[test]
    fn generate_usages_follow_the_plan() {
        let kem = PqAlgorithm::MlKem768;
        assert_eq!(kem.split_generate_usages(USAGE_ENCAPSULATE_BITS), Err(UsageError::Empty));
        assert_eq!(kem.split_generate_usages(USAGE_SIGN), Err(UsageError::Unsupported));
        assert_eq!(kem.split_generate_usages(USAGE_DECAPSULATE_BITS), Ok((0, USAGE_DECAPSULATE_BITS)));
        assert_eq!(PqAlgorithm::MlDsa44.split_generate_usages(USAGE_SIGN | USAGE_VERIFY), Ok((USAGE_VERIFY, USAGE_SIGN)));
    }
}
