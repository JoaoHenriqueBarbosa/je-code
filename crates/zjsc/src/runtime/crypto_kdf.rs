//! HKDF e PBKDF2 do `SubtleCrypto` (`CryptoAlgorithmHKDF.cpp`, `CryptoAlgorithmPBKDF2.cpp`, `CryptoKeyRaw.cpp`): `importKey` raw,
//! `deriveBits` e `deriveKey` (para HMAC e AES, em `crypto.rs`). É filho de `crypto` (via `#[path]`) para usar o que lá é
//! privado (`KeyState`, `required_member`, `dom_error`...). As mensagens e a ordem de validação foram medidas no bun 1.4.2.
//! A chave HKDF/PBKDF2 é um `KeyState` com `aes: Some(Hkdf | Pbkdf2)`, nunca extraível, que só deriva.

use super::*;
use ul_common::hash::pbkdf2;

/// Os usos que uma chave HKDF/PBKDF2 aceita.
const KDF_USAGES: u16 = USAGE_DERIVE_KEY | USAGE_DERIVE_BITS;
/// `exportKey` e `wrapKey` de uma chave KDF: a mensagem tem ponto final, ao contrário de [`NOT_SUPPORTED`].
const EXPORT_NOT_SUPPORTED: &str = "The operation is not supported.";

pub(super) fn is_kdf_id(id: AlgorithmId) -> bool {
    matches!(id, AlgorithmId::Hkdf | AlgorithmId::Pbkdf2)
}

pub(super) fn kdf_name(id: AlgorithmId) -> &'static str {
    if id == AlgorithmId::Hkdf { "HKDF" } else { "PBKDF2" }
}

/// `exportKey`/`wrapKey` de uma chave KDF: o bun recusa antes de olhar `extractable`.
pub(super) fn export_unsupported(global_object: &JSGlobalObject, call: &HostCall) -> Thrown {
    dom_error(global_object, call, "NotSupportedError", EXPORT_NOT_SUPPORTED)
}

/// `importKey` de HKDF/PBKDF2: só o material cru. Ordem medida: `raw-seed`, formato que não é cru, usos fora de
/// `deriveKey`/`deriveBits`, `extractable`, uso vazio. `raw-public` e `raw-secret` valem como `raw`.
pub(super) fn import_kdf_key(
    global_object: &JSGlobalObject,
    call: &HostCall,
    id: AlgorithmId,
    format: &str,
    bytes: Option<Vec<u8>>,
    extractable: bool,
    usages: u16,
) -> Result<KeyState, Thrown> {
    let name = kdf_name(id);
    if format == "raw-seed" {
        return Err(dom_error(global_object, call, "NotSupportedError", &format!("Unable to import {name} using raw-seed format")));
    }
    let Some(secret) = bytes.filter(|_| matches!(format, "raw" | "raw-secret" | "raw-public")) else {
        return Err(dom_error(global_object, call, "NotSupportedError", NOT_SUPPORTED));
    };
    if usages & !KDF_USAGES != 0 {
        return Err(throw_native_syntax_error(global_object, &format!("Unsupported key usage for a {name} key")));
    }
    if extractable {
        return Err(throw_native_syntax_error(global_object, &format!("{name} keys are not extractable")));
    }
    if usages == 0 {
        return Err(throw_native_syntax_error(global_object, "Usages cannot be empty when importing a secret key."));
    }
    Ok(aes_secret_key(id, secret, false, usages))
}

/// O nome do `hash` já lido do dicionário (texto ou objeto com `name`); hash fora dos resumos é `Unrecognized algorithm name`.
/// O bun só resolve o nome depois de converter os outros membros do dicionário.
fn resolve_hash(global_object: &JSGlobalObject, call: &HostCall, hash: JSValue) -> Result<Algo, Thrown> {
    let index = hash_index(&algorithm_name(global_object, hash)?).ok_or_else(|| not_supported(global_object, call))?;
    Ok(HASHES[index].algo)
}

/// Os parâmetros de `deriveBits`/`deriveKey` de HKDF e PBKDF2, já convertidos (`iterations` vale 0 no HKDF, `info` é vazio no PBKDF2).
pub(super) struct KdfParams {
    id: AlgorithmId,
    algo: Algo,
    salt: Vec<u8>,
    info: Vec<u8>,
    iterations: u32,
}

/// Os bytes de um membro `BufferSource` obrigatório do dicionário.
fn buffer_member(global_object: &JSGlobalObject, value: JSValue, dictionary: &str, name: &str) -> Result<Vec<u8>, Thrown> {
    buffer_source(global_object, required_member(global_object, value, dictionary, name, BUFFER_KIND)?)
}

/// HKDF-Expand (RFC 5869) com `HMAC` do `algo`; `None` quando `length` passa de 255 vezes o tamanho do resumo.
fn hkdf(algo: Algo, secret: &[u8], salt: &[u8], info: &[u8], length: usize) -> Option<Vec<u8>> {
    let hash_len = hmac(algo, &[], &[]).len();
    if length > 255 * hash_len {
        return None;
    }
    let pseudo_random_key = hmac(algo, salt, secret);
    let mut output = Vec::with_capacity(length + hash_len);
    let mut block: Vec<u8> = Vec::new();
    for counter in 1..=length.div_ceil(hash_len.max(1)) {
        let mut message = block;
        message.extend_from_slice(info);
        message.push(counter as u8);
        block = hmac(algo, &pseudo_random_key, &message);
        output.extend_from_slice(&block);
    }
    output.truncate(length);
    Some(output)
}

/// Converte o dicionário `HkdfParams`/`Pbkdf2Params` na ordem medida no bun: `hash` presente, os demais membros em ordem
/// alfabética (`info` e `salt`; `iterations` e `salt`), e só então o nome do `hash`.
pub(super) fn kdf_params(global_object: &JSGlobalObject, call: &HostCall, id: AlgorithmId) -> Result<KdfParams, Thrown> {
    let params = call.argument(0);
    let dictionary = if id == AlgorithmId::Hkdf { "HkdfParams" } else { "Pbkdf2Params" };
    let hash = required_member(global_object, params, dictionary, "hash", "(object or DOMString)")?;
    let (info, iterations) = if id == AlgorithmId::Hkdf {
        (buffer_member(global_object, params, dictionary, "info")?, 0)
    } else {
        let iterations = required_member(global_object, params, dictionary, "iterations", "unsigned long")?;
        (Vec::new(), enforce_range(global_object, iterations, u32::MAX)?)
    };
    let salt = buffer_member(global_object, params, dictionary, "salt")?;
    let algo = resolve_hash(global_object, call, hash)?;
    Ok(KdfParams { id, algo, salt, info, iterations })
}

/// `deriveBits` de HKDF/PBKDF2 (também a metade de `deriveKey`): uso, algoritmo da chave base e o tamanho (`null` ou ausente é
/// `length cannot be null`, não múltiplo de 8 é recusado). `length` só é avaliado depois dos testes da chave base.
pub(super) fn derive_kdf_bits(
    global_object: &JSGlobalObject,
    call: &HostCall,
    params: KdfParams,
    base: &KeyState,
    length: impl FnOnce() -> Result<Option<u32>, Thrown>,
    usage: (u16, &str),
) -> Result<Vec<u8>, Thrown> {
    let KdfParams { id, algo, salt, info, iterations } = params;
    if base.usages & usage.0 == 0 {
        return Err(dom_error(global_object, call, "InvalidAccessError", &format!("baseKey does not have {} usage", usage.1)));
    }
    if base.aes != Some(id) {
        return Err(dom_error(global_object, call, "InvalidAccessError", "Key algorithm mismatch"));
    }
    let Some(bits) = length()? else {
        return Err(operation_error(global_object, call, "length cannot be null"));
    };
    if bits % 8 != 0 {
        return Err(operation_error(global_object, call, "length must be a multiple of 8"));
    }
    let bytes = bits as usize / 8;
    let derived = if id == AlgorithmId::Hkdf {
        hkdf(algo, &base.secret, &salt, &info, bytes)
    } else {
        (iterations != 0).then(|| if bytes == 0 { Vec::new() } else { pbkdf2(algo, &base.secret, &salt, u64::from(iterations), bytes) })
    };
    derived.ok_or_else(|| operation_error(global_object, call, OPERATION_FAILED))
}
