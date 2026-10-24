//! As primitivas RSA do `SubtleCrypto` (RSASSA-PKCS1-v1_5, RSA-PSS, RSA-OAEP), sem nada de JavaScript: bytes entram, bytes
//! saem. A chave privada é guardada como PKCS#8 DER e a pública como SPKI DER (o `secret` do `KeyState`); `crypto.rs`
//! cuida das mensagens de erro do bun.

use rsa::pkcs1::der::{asn1::UintRef, Encode};
use rsa::pkcs1::{RsaPrivateKey as Pkcs1Private, ALGORITHM_ID};
use rsa::pkcs8::{DecodePublicKey, EncodePrivateKey, EncodePublicKey, PrivateKeyInfo};
use rsa::traits::PublicKeyParts;
use rsa::{BigUint, Pkcs1v15Sign, Pss, RsaPrivateKey, RsaPublicKey};
use ul_common::hash::Algo;

use crate::runtime::crypto_ec::SystemRng;

/// Os componentes de um JWK RSA, em big-endian sem zeros à esquerda: `n`, `e` e, numa privada, `d`, `p`, `q`, `dp`, `dq`
/// e `qi`.
pub(crate) struct RsaComponents {
    pub(crate) n: Vec<u8>,
    pub(crate) e: Vec<u8>,
    pub(crate) private: Option<PrivateComponents>,
}

pub(crate) struct PrivateComponents {
    pub(crate) d: Vec<u8>,
    pub(crate) p: Vec<u8>,
    pub(crate) q: Vec<u8>,
    pub(crate) dp: Vec<u8>,
    pub(crate) dq: Vec<u8>,
    pub(crate) qi: Vec<u8>,
}

/// O prefixo DER do `DigestInfo` (RFC 8017, seção 9.2) de cada resumo que o RSASSA-PKCS1-v1_5 aceita.
fn digest_info_prefix(hash: Algo) -> Option<&'static [u8]> {
    Some(match hash {
        Algo::Sha1 => &[0x30, 0x21, 0x30, 0x09, 0x06, 0x05, 0x2b, 0x0e, 0x03, 0x02, 0x1a, 0x05, 0x00, 0x04, 0x14],
        Algo::Sha224 => &[0x30, 0x2d, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x04, 0x05, 0x00, 0x04, 0x1c],
        Algo::Sha256 => &[0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05, 0x00, 0x04, 0x20],
        Algo::Sha384 => &[0x30, 0x41, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x02, 0x05, 0x00, 0x04, 0x30],
        Algo::Sha512 => &[0x30, 0x51, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x03, 0x05, 0x00, 0x04, 0x40],
        _ => return None,
    })
}

/// Chama `$body` com `$digest` ligado ao tipo de resumo de `$hash` (os da lista de [`digest_info_prefix`]).
macro_rules! with_digest {
    ($hash:expr, $digest:ident, $body:expr) => {
        match $hash {
            Algo::Sha1 => {
                type $digest = sha1::Sha1;
                Some($body)
            }
            Algo::Sha224 => {
                type $digest = sha2::Sha224;
                Some($body)
            }
            Algo::Sha256 => {
                type $digest = sha2::Sha256;
                Some($body)
            }
            Algo::Sha384 => {
                type $digest = sha2::Sha384;
                Some($body)
            }
            Algo::Sha512 => {
                type $digest = sha2::Sha512;
                Some($body)
            }
            _ => None,
        }
    };
}

/// Os componentes de um PKCS#8 RSA exatamente como estão guardados, sem nenhuma validação: o bun importa um JWK privado
/// com `d`, `dp`, `dq` e `qi` que não batem entre si e devolve cada componente como veio no `exportKey`.
fn raw_private(der: &[u8]) -> Option<Pkcs1Private<'_>> {
    let info = PrivateKeyInfo::try_from(der).ok()?;
    if info.algorithm.oid != rsa::pkcs1::ALGORITHM_OID {
        return None;
    }
    Pkcs1Private::try_from(info.private_key).ok()
}

fn to_big(value: UintRef<'_>) -> BigUint {
    BigUint::from_bytes_be(value.as_bytes())
}

/// O inverso multiplicativo de `a` módulo `modulus` (Euclides estendido); `None` quando não são coprimos.
fn mod_inverse(a: &BigUint, modulus: &BigUint) -> Option<BigUint> {
    let zero = BigUint::from(0u8);
    let (mut old_r, mut r) = (modulus.clone(), a % modulus);
    let (mut old_t, mut t) = (zero.clone(), BigUint::from(1u8));
    while r != zero {
        let quotient = &old_r / &r;
        let next_r = &old_r % &r;
        let next_t = (&old_t + modulus - (&quotient * &t) % modulus) % modulus;
        (old_r, r, old_t, t) = (r, next_r, t, next_t);
    }
    (old_r == BigUint::from(1u8)).then_some(old_t)
}

/// A chave privada utilizável. Como no bun (BoringSSL), a conta usa o CRT guardado (`p`, `q`, `dp`, `dq`, `qi`) e o `d`
/// não conta: com o CRT incoerente a assinatura falha (`None`); com o CRT coerente o `d` guardado é irrelevante.
fn private_key(der: &[u8]) -> Option<RsaPrivateKey> {
    let raw = raw_private(der)?;
    let (n, e, p, q) = (to_big(raw.modulus), to_big(raw.public_exponent), to_big(raw.prime1), to_big(raw.prime2));
    let (dp, dq, qi) = (to_big(raw.exponent1), to_big(raw.exponent2), to_big(raw.coefficient));
    let one = BigUint::from(1u8);
    if p <= one || q <= one || &p * &q != n {
        return None;
    }
    let (p1, q1) = (&p - &one, &q - &one);
    let d = mod_inverse(&e, &(&p1 * &q1))?;
    if dp % &p1 != &d % &p1 || dq % &q1 != &d % &q1 || (qi * &q) % &p != one % &p {
        return None;
    }
    RsaPrivateKey::from_components(n, e, d, vec![p, q]).ok()
}

fn public_key(der: &[u8]) -> Option<RsaPublicKey> {
    RsaPublicKey::from_public_key_der(der).ok()
}

/// A chave pública de `der`, que é um SPKI (pública) ou um PKCS#8 (privada, de onde se tira a pública sem validar).
fn public_of_any(der: &[u8]) -> Option<RsaPublicKey> {
    public_key(der).or_else(|| raw_private(der).map(|raw| RsaPublicKey::new_unchecked(to_big(raw.modulus), to_big(raw.public_exponent))))
}

/// Gera um par RSA: o PKCS#8 DER da privada. `exponent` é o `publicExponent` em big-endian.
pub(crate) fn generate(bits: usize, exponent: &[u8]) -> Option<Vec<u8>> {
    let trimmed = &exponent[exponent.iter().position(|&byte| byte != 0)?..];
    if trimmed.len() > 8 {
        return None;
    }
    let exp = trimmed.iter().fold(0u64, |acc, &byte| (acc << 8) | u64::from(byte));
    let key = RsaPrivateKey::new_with_exp(&mut SystemRng, bits, &BigUint::from(exp)).ok()?;
    Some(key.to_pkcs8_der().ok()?.as_bytes().to_vec())
}

/// O SPKI DER da pública de uma chave (privada em PKCS#8 ou pública em SPKI).
pub(crate) fn public_spki(der: &[u8]) -> Option<Vec<u8>> {
    Some(public_of_any(der)?.to_public_key_der().ok()?.as_bytes().to_vec())
}

/// Valida um PKCS#8 e o devolve normalizado; recusa o que não é RSA.
pub(crate) fn parse_pkcs8(der: &[u8]) -> Option<Vec<u8>> {
    Some(private_key(der)?.to_pkcs8_der().ok()?.as_bytes().to_vec())
}

/// Valida um SPKI e o devolve normalizado; recusa o que não é RSA.
pub(crate) fn parse_spki(der: &[u8]) -> Option<Vec<u8>> {
    public_spki(&public_key(der)?.to_public_key_der().ok()?.as_bytes().to_vec())
}

/// `true` quando `der` é um SPKI (`private == false`) ou um PKCS#8 (`private == true`) bem formado cujo algoritmo não é RSA.
pub(crate) fn is_foreign_key(der: &[u8], private: bool) -> bool {
    let oid = if private {
        rsa::pkcs8::PrivateKeyInfo::try_from(der).ok().map(|info| info.algorithm.oid)
    } else {
        rsa::pkcs8::spki::SubjectPublicKeyInfoRef::try_from(der).ok().map(|info| info.algorithm.oid)
    };
    oid.is_some_and(|oid| oid != rsa::pkcs1::ALGORITHM_OID)
}

/// O tamanho do módulo em bits e o expoente público (big-endian), o que `algorithm` da chave expõe.
pub(crate) fn parameters(der: &[u8]) -> Option<(usize, Vec<u8>)> {
    let key = public_of_any(der)?;
    Some((key.n().bits(), key.e().to_bytes_be()))
}

/// Os componentes do JWK de uma chave; a parte privada só existe numa chave privada.
pub(crate) fn components(der: &[u8]) -> Option<RsaComponents> {
    let public = public_of_any(der)?;
    let bytes = |value: UintRef<'_>| to_big(value).to_bytes_be();
    let private = raw_private(der).map(|raw| PrivateComponents {
        d: if exponent_is_present(&raw) { bytes(raw.private_exponent) } else { Vec::new() },
        p: bytes(raw.prime1),
        q: bytes(raw.prime2),
        dp: bytes(raw.exponent1),
        dq: bytes(raw.exponent2),
        qi: bytes(raw.coefficient),
    });
    Some(RsaComponents { n: public.n().to_bytes_be(), e: public.e().to_bytes_be(), private })
}

/// Medido no bun 1.4.2: um JWK privado com `d` zero vira uma chave privada cujo `exportKey('jwk')` não traz `d` (o
/// OpenSSL trata o expoente zerado como ausente); qualquer outro `d`, coerente ou não, sai como veio.
fn exponent_is_present(raw: &Pkcs1Private<'_>) -> bool {
    raw.private_exponent.as_bytes().iter().any(|byte| *byte != 0)
}

/// Monta a chave de um JWK: o SPKI DER (sem parte privada) ou o PKCS#8 DER (com `d`, `p`, `q`, `dp`, `dq` e `qi`,
/// guardados como vieram, sem checar coerência: quem recusa é a operação, como no bun). `None` quando a pública não é
/// uma chave RSA válida ou um componente não cabe num inteiro DER.
pub(crate) fn from_components(parts: &RsaComponents) -> Option<Vec<u8>> {
    let n = BigUint::from_bytes_be(&parts.n);
    let e = BigUint::from_bytes_be(&parts.e);
    match &parts.private {
        None => Some(RsaPublicKey::new(n, e).ok()?.to_public_key_der().ok()?.as_bytes().to_vec()),
        Some(private) => {
            let key = Pkcs1Private {
                modulus: UintRef::new(&parts.n).ok()?,
                public_exponent: UintRef::new(&parts.e).ok()?,
                private_exponent: UintRef::new(&private.d).ok()?,
                prime1: UintRef::new(&private.p).ok()?,
                prime2: UintRef::new(&private.q).ok()?,
                exponent1: UintRef::new(&private.dp).ok()?,
                exponent2: UintRef::new(&private.dq).ok()?,
                coefficient: UintRef::new(&private.qi).ok()?,
                other_prime_infos: None,
            };
            let pkcs1 = key.to_der().ok()?;
            PrivateKeyInfo::new(ALGORITHM_ID, &pkcs1).to_der().ok()
        }
    }
}

/// RSASSA-PKCS1-v1_5 (determinístico). `None` quando a chave não é privada ou o resumo não é suportado.
pub(crate) fn sign_pkcs1(der: &[u8], data: &[u8], hash: Algo) -> Option<Vec<u8>> {
    let key = private_key(der)?;
    let scheme = Pkcs1v15Sign { hash_len: Some(hash.digest(&[]).len()), prefix: digest_info_prefix(hash)?.into() };
    key.sign(scheme, &hash.digest(data)).ok()
}

pub(crate) fn verify_pkcs1(der: &[u8], data: &[u8], signature: &[u8], hash: Algo) -> bool {
    let (Some(key), Some(prefix)) = (public_of_any(der), digest_info_prefix(hash)) else { return false };
    let scheme = Pkcs1v15Sign { hash_len: Some(hash.digest(&[]).len()), prefix: prefix.into() };
    key.verify(scheme, &hash.digest(data), signature).is_ok()
}

/// RSA-PSS com sal de `salt_length` bytes (aleatório: a saída muda a cada chamada).
pub(crate) fn sign_pss(der: &[u8], data: &[u8], hash: Algo, salt_length: usize) -> Option<Vec<u8>> {
    let key = private_key(der)?;
    with_digest!(hash, D, key.sign_with_rng(&mut SystemRng, Pss::new_with_salt::<D>(salt_length), &hash.digest(data)).ok())?
}

pub(crate) fn verify_pss(der: &[u8], data: &[u8], signature: &[u8], hash: Algo, salt_length: usize) -> bool {
    let Some(key) = public_of_any(der) else { return false };
    with_digest!(hash, D, key.verify(Pss::new_with_salt::<D>(salt_length), &hash.digest(data), signature).is_ok()).unwrap_or(false)
}

/// MGF1 (RFC 8017, apêndice B.2) sobre `hash`.
fn mgf1(hash: Algo, seed: &[u8], length: usize) -> Vec<u8> {
    let mut mask = Vec::with_capacity(length + hash.digest_size());
    let mut counter = 0u32;
    while mask.len() < length {
        let mut block = seed.to_vec();
        block.extend_from_slice(&counter.to_be_bytes());
        mask.extend_from_slice(&hash.digest(&block));
        counter += 1;
    }
    mask.truncate(length);
    mask
}

fn xor_in_place(target: &mut [u8], mask: &[u8]) {
    target.iter_mut().zip(mask).for_each(|(byte, m)| *byte ^= m);
}

/// Um inteiro em big-endian com zeros à esquerda até `size` bytes.
fn padded_be(value: &BigUint, size: usize) -> Option<Vec<u8>> {
    let bytes = value.to_bytes_be();
    let zeros = size.checked_sub(bytes.len())?;
    let mut out = vec![0u8; zeros];
    out.extend_from_slice(&bytes);
    Some(out)
}

/// RSA-OAEP (RFC 8017, seção 7.1) com o `label` em bytes (vazio quando ausente); a máscara usa o mesmo resumo (MGF1). A
/// `rsa` só aceita o rótulo como `String`, o que corrompe bytes que não são UTF-8, por isso o preenchimento é feito aqui
/// e o `rsa` fica só com a exponenciação.
pub(crate) fn encrypt_oaep(der: &[u8], data: &[u8], hash: Algo, label: &[u8]) -> Option<Vec<u8>> {
    use rsa::rand_core::RngCore;
    let key = public_of_any(der)?;
    digest_info_prefix(hash)?;
    let (size, h_len) = (key.size(), hash.digest_size());
    let room = size.checked_sub(2 * h_len + 2)?;
    if data.len() > room {
        return None;
    }
    let mut block = hash.digest(label);
    block.resize(room + h_len - data.len(), 0);
    block.push(1);
    block.extend_from_slice(data);
    let mut seed = vec![0u8; h_len];
    SystemRng.fill_bytes(&mut seed);
    xor_in_place(&mut block, &mgf1(hash, &seed, size - h_len - 1));
    xor_in_place(&mut seed, &mgf1(hash, &block, h_len));
    let mut encoded = vec![0u8];
    encoded.extend_from_slice(&seed);
    encoded.extend_from_slice(&block);
    let cipher = rsa::hazmat::rsa_encrypt(&key, &BigUint::from_bytes_be(&encoded)).ok()?;
    padded_be(&cipher, size)
}

pub(crate) fn decrypt_oaep(der: &[u8], data: &[u8], hash: Algo, label: &[u8]) -> Option<Vec<u8>> {
    let key = private_key(der)?;
    digest_info_prefix(hash)?;
    let (size, h_len) = (key.size(), hash.digest_size());
    if data.len() != size || size < 2 * h_len + 2 {
        return None;
    }
    let plain = rsa::hazmat::rsa_decrypt_and_check(&key, Some(&mut SystemRng), &BigUint::from_bytes_be(data)).ok()?;
    let encoded = padded_be(&plain, size)?;
    let (first, rest) = encoded.split_first()?;
    let (masked_seed, masked_block) = rest.split_at(h_len);
    let mut seed = masked_seed.to_vec();
    xor_in_place(&mut seed, &mgf1(hash, masked_block, h_len));
    let mut block = masked_block.to_vec();
    xor_in_place(&mut block, &mgf1(hash, &seed, size - h_len - 1));
    let (label_hash, padded) = block.split_at(h_len);
    let separator = padded.iter().position(|&byte| byte != 0)?;
    (*first == 0 && label_hash == hash.digest(label) && padded[separator] == 1).then(|| padded[separator + 1..].to_vec())
}
