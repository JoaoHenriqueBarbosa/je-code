//! O módulo `node:crypto` do bun 1.4.2: o `crypto.ts` do bun (gerado em `api/node_crypto.js` por `scripts/gen-node-crypto.js`)
//! sobre os resumos do `ul-common`. O `$cpp("node_crypto_binding.cpp", ...)` e o `$rust("node_crypto_binding.rs", ...)` do bun
//! são o objeto `host.crypto` montado aqui e o texto JavaScript do gerador: aqui ficam só os resumos (`Hash`, `Hmac`) e os
//! bytes aleatórios; a validação dos argumentos e as mensagens de erro estão no JavaScript, medidas no oráculo.
//!
//! Resolução do nome do algoritmo (medida no bun 1.4.2, fonte `ncrypto.cpp::getDigestByName` e `EVP.rs::ALGORITHM_MAP`):
//!
//! - `createHmac` e a primeira tentativa de `createHash` usam o `getDigestByName` do bun: `dss1` e `DSS1` são SHA-1, `md5` e
//!   `rsa-sha1/224/256/384/512` são os que dizem, `sha-1/224/256/384/512` e `sha-512/224`, `sha-512/256`, `sha-512256`,
//!   `sha-512_256` e `sha128` são apelidos dos SHA-1/2, e o resto vem da tabela do BoringSSL (`md4`, `md5`, `ripemd160`,
//!   `sha1`, `sha224`, `sha256`, `sha384`, `sha512`, `sha512-224`, `sha512-256`, `sha3-*`, `md5-sha1` e os nomes
//!   `...WithRSAEncryption` na grafia exata), sem diferenciar maiúsculas nos nomes curtos;
//! - `createHash` ainda tenta a tabela própria do bun (`rmd160`, `blake2b256`, `blake2b512`, `blake2s256`, `shake128`,
//!   `shake256`, `shake-128`, `shake-256`, `sha-512224`...), e `crypto.hash` usa só essa tabela (`dss1`, `rsa-sha256` e
//!   `md5-sha1` não existem nele).

use std::cell::RefCell;

use ul_common::hash::{Algo, Hasher, Hmac};

use crate::host_function;
use crate::runtime::host::host;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_array::construct_array;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::text_decoder::input_bytes;
use crate::runtime::uint8_array_base64::create_uint8_array;

/// O texto da fábrica de `node:crypto`.
const CRYPTO: &str = include_str!("../api/node_crypto.js");

/// O estado de um resumo em andamento: `Hash` guarda o algoritmo (o comprimento padrão e se a saída é livre), `Hmac` as duas pontas.
enum Slot {
    Hash(Algo, Hasher),
    Hmac(Hmac),
}

thread_local! {
    /// Os resumos em andamento; o `id` do JavaScript é o índice. Um `digest` libera o lugar.
    static SLOTS: RefCell<Vec<Option<Slot>>> = const { RefCell::new(Vec::new()) };
}

/// Fim do programa (`cell_registry::reset_program_state`): os resumos que ninguém terminou saem.
pub(crate) fn reset_for_program() {
    let _ = SLOTS.try_with(|slots| slots.borrow_mut().clear());
}

fn store(slot: Slot) -> usize {
    SLOTS.with(|slots| {
        let mut slots = slots.borrow_mut();
        match slots.iter().position(Option::is_none) {
            Some(index) => {
                slots[index] = Some(slot);
                index
            }
            None => {
                slots.push(Some(slot));
                slots.len() - 1
            }
        }
    })
}

/// Os bytes do resumo de `Algo` quando o programa não pede comprimento: o dos SHAKE é o do bun (16 e 32), não o `0` do `Algo`.
fn default_length(algo: Algo) -> usize {
    match algo {
        Algo::Shake128 => 16,
        Algo::Shake256 => 32,
        other => other.digest_size(),
    }
}

/// `ncrypto::getDigestByName`: os casos especiais do bun e depois a tabela do BoringSSL (`EVP_get_digestbyname`).
fn evp_digest(name: &str) -> Option<Algo> {
    let lower = name.to_ascii_lowercase();
    if name == "dss1" || name == "DSS1" {
        return Some(Algo::Sha1);
    }
    if lower == "md5" {
        return Some(Algo::Md5);
    }
    if let Some(bits) = lower.strip_prefix("rsa-sha") {
        match bits {
            "1" => return Some(Algo::Sha1),
            "224" => return Some(Algo::Sha224),
            "256" => return Some(Algo::Sha256),
            "384" => return Some(Algo::Sha384),
            "512" => return Some(Algo::Sha512),
            _ => {}
        }
    }
    if let Some(remain) = lower.strip_prefix("sha") {
        if let Some(bits) = remain.strip_prefix('-') {
            match bits {
                "1" => return Some(Algo::Sha1),
                "224" => return Some(Algo::Sha224),
                "256" => return Some(Algo::Sha256),
                "384" => return Some(Algo::Sha384),
                _ => {}
            }
            if let Some(more) = bits.strip_prefix("512") {
                match more {
                    "" => return Some(Algo::Sha512),
                    "/224" => return Some(Algo::Sha512Trunc224),
                    "/256" | "256" | "_256" => return Some(Algo::Sha512Trunc256),
                    _ => {}
                }
            }
        }
        if remain == "128" {
            return Some(Algo::Sha1);
        }
    }
    // Os apelidos longos da tabela do OpenSSL só casam na grafia exata (`sha1withrsaencryption` não existe no bun).
    match name {
        "RSA-MD5" | "md5WithRSAEncryption" => return Some(Algo::Md5),
        "sha1WithRSAEncryption" | "ecdsa-with-SHA1" | "DSA-SHA" => return Some(Algo::Sha1),
        "sha224WithRSAEncryption" => return Some(Algo::Sha224),
        "sha256WithRSAEncryption" => return Some(Algo::Sha256),
        "sha384WithRSAEncryption" => return Some(Algo::Sha384),
        "sha512WithRSAEncryption" => return Some(Algo::Sha512),
        _ => {}
    }
    Some(match lower.as_str() {
        "md4" => Algo::Md4,
        "sha1" => Algo::Sha1,
        "sha224" => Algo::Sha224,
        "sha256" => Algo::Sha256,
        "sha384" => Algo::Sha384,
        "sha512" => Algo::Sha512,
        "sha512-224" => Algo::Sha512Trunc224,
        "sha512-256" => Algo::Sha512Trunc256,
        "sha3-224" => Algo::Sha3(28),
        "sha3-256" => Algo::Sha3(32),
        "sha3-384" => Algo::Sha3(48),
        "sha3-512" => Algo::Sha3(64),
        "ripemd160" => Algo::Ripemd160,
        "md5-sha1" => Algo::Md5Sha1,
        _ => return None,
    })
}

/// A tabela própria do bun (`EVP.rs::ALGORITHM_MAP`), sem diferenciar maiúsculas.
fn bun_digest(name: &str) -> Option<Algo> {
    Some(match name.to_ascii_lowercase().as_str() {
        "md4" => Algo::Md4,
        "md5" => Algo::Md5,
        "sha1" | "sha-1" | "sha128" => Algo::Sha1,
        "sha224" | "sha-224" => Algo::Sha224,
        "sha256" | "sha-256" => Algo::Sha256,
        "sha384" | "sha-384" => Algo::Sha384,
        "sha512" | "sha-512" => Algo::Sha512,
        "rmd160" | "ripemd160" => Algo::Ripemd160,
        "sha3-224" => Algo::Sha3(28),
        "sha3-256" => Algo::Sha3(32),
        "sha3-384" => Algo::Sha3(48),
        "sha3-512" => Algo::Sha3(64),
        "shake128" | "shake-128" => Algo::Shake128,
        "shake256" | "shake-256" => Algo::Shake256,
        "blake2b256" => Algo::Blake2b(32),
        "blake2b512" => Algo::Blake2b(64),
        "blake2s256" => Algo::Blake2s(32),
        "sha-512224" | "sha512-224" | "sha-512/224" | "sha-512_224" => Algo::Sha512Trunc224,
        "sha-512256" | "sha512-256" | "sha-512/256" | "sha-512_256" => Algo::Sha512Trunc256,
        _ => return None,
    })
}

/// O algoritmo de `createHash(nome)`; com `only_bun_table` o de `crypto.hash(nome, ...)`.
fn hash_algorithm(name: &str, only_bun_table: bool) -> Option<Algo> {
    if only_bun_table {
        bun_digest(name)
    } else {
        evp_digest(name).or_else(|| bun_digest(name))
    }
}

/// `[id, comprimento padrão, saída livre]` de um resumo recém-criado.
fn created(global_object: &JSGlobalObject, algo: Algo, hasher: Hasher) -> JSValue {
    let id = store(Slot::Hash(algo, hasher));
    let members = [js_number(id as f64), js_number(default_length(algo) as f64), js_boolean(algo.is_xof())];
    construct_array(global_object.vm(), &global_object.array_structure(), &members).as_value()
}

fn slot_id(call: &HostCall) -> usize {
    let value = call.argument(0);
    if value.is_number() { value.as_number() as usize } else { usize::MAX }
}

/// `hashCreate(nome, só a tabela do bun)`: `[id, comprimento padrão, saída livre]`, ou `undefined` sem esse algoritmo.
fn hash_create_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = rust_string(&call.argument(0).to_wtf_string());
    Ok(match hash_algorithm(&name, call.argument(1).to_boolean()) {
        Some(algo) => created(global_object, algo, algo.hasher()),
        None => JSValue::undefined(),
    })
}
host_function!(hash_create_function, hash_create_body);

/// `hashCopy(id)`: o mesmo estado em outro lugar, com o mesmo formato de `hashCreate`.
fn hash_copy_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let id = slot_id(call);
    let copy = SLOTS.with(|slots| match slots.borrow().get(id) {
        Some(Some(Slot::Hash(algo, hasher))) => Some((*algo, hasher.clone())),
        _ => None,
    });
    Ok(copy.map_or_else(JSValue::undefined, |(algo, hasher)| created(global_object, algo, hasher)))
}
host_function!(hash_copy_function, hash_copy_body);

/// `hmacCreate(nome, chave)`: o `id`, ou `-1` quando o `getDigestByName` não conhece o nome.
fn hmac_create_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = rust_string(&call.argument(0).to_wtf_string());
    let key = input_bytes(call.argument(1)).unwrap_or_default();
    let hmac = evp_digest(&name).and_then(|algo| Hmac::new(algo, &key));
    Ok(js_number(hmac.map_or(-1.0, |hmac| store(Slot::Hmac(hmac)) as f64)))
}
host_function!(hmac_create_function, hmac_create_body);

/// `hashUpdate(id, bytes)`: `true` se o estado existe.
fn hash_update_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let data = input_bytes(call.argument(1)).unwrap_or_default();
    let id = slot_id(call);
    let updated = SLOTS.with(|slots| match slots.borrow_mut().get_mut(id) {
        Some(Some(Slot::Hash(_, hasher))) => {
            hasher.update(&data);
            true
        }
        Some(Some(Slot::Hmac(hmac))) => {
            hmac.update(&data);
            true
        }
        _ => false,
    });
    Ok(js_boolean(updated))
}
host_function!(hash_update_function, hash_update_body);

/// `hashDigest(id, comprimento)`: os bytes do resumo (`Uint8Array`) e libera o estado.
fn hash_digest_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let id = slot_id(call);
    let length = call.argument(1).as_number() as usize;
    let slot = SLOTS.with(|slots| slots.borrow_mut().get_mut(id).and_then(Option::take));
    let digest = match slot {
        Some(Slot::Hash(_, hasher)) => hasher.finalize(length),
        Some(Slot::Hmac(hmac)) => hmac.finalize(),
        None => return Ok(JSValue::undefined()),
    };
    bytes_value(global_object, &digest)
}
host_function!(hash_digest_function, hash_digest_body);

/// `hashDrop(id)`: descarta um estado que ninguém vai terminar.
fn hash_drop_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let id = slot_id(call);
    SLOTS.with(|slots| slots.borrow_mut().get_mut(id).map(Option::take));
    Ok(JSValue::undefined())
}
host_function!(hash_drop_function, hash_drop_body);

/// `randomBytes(n)`: `n` bytes do sistema num `Uint8Array`.
fn random_bytes_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let mut bytes = vec![0u8; call.argument(0).as_number() as usize];
    host().random_fill(&mut bytes);
    bytes_value(global_object, &bytes)
}
host_function!(random_bytes_function, random_bytes_body);

fn bytes_value(global_object: &JSGlobalObject, bytes: &[u8]) -> HostResult {
    let array = create_uint8_array(global_object, bytes.len())?;
    array.with_vector_mut(|destination| destination[..bytes.len()].copy_from_slice(bytes));
    Ok(array.as_value())
}

/// Instalador de `node:crypto`.
pub(crate) fn install_crypto_module(global_object: &JSGlobalObject) -> HostResult {
    let host_object = crate::api::node_js_host::module_host(
        global_object,
        &[("utilInternal", 1, crate::api::node_util::util_internal), ("requireInternal", 1, crate::api::node_stream::require_internal)],
    )?;
    let vm = global_object.vm();
    let natives = crate::runtime::object_constructor::construct_empty_object(global_object);
    crate::api::node_js_host::put_functions(
        global_object,
        &natives,
        &[
            ("hashCreate", 2, hash_create_function),
            ("hashCopy", 1, hash_copy_function),
            ("hmacCreate", 2, hmac_create_function),
            ("hashUpdate", 2, hash_update_function),
            ("hashDigest", 2, hash_digest_function),
            ("hashDrop", 1, hash_drop_function),
            ("randomBytes", 1, random_bytes_function),
            ("timingSafeEqual", 2, crate::runtime::crypto::timing_safe_equal_function),
        ],
    );
    host_object.put_direct(vm, &crate::api::eval::key_of(vm, "crypto"), natives.as_value(), 0);
    Ok(crate::api::node_js_host::run_factory(global_object, CRYPTO, "crypto", &host_object))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Os nomes medidos no oráculo (bun 1.4.2): `createHmac` aceita só o que o BoringSSL e o `getDigestByName` conhecem.
    #[test]
    fn evp_names_match_the_oracle() {
        for name in ["sha1", "sha-1", "SHA128", "dss1", "DSS1", "RSA-SHA512", "sha512WithRSAEncryption", "RSA-MD5", "sha-512/256", "sha-512_256", "MD4", "RIPEMD160", "md5-sha1", "SHA-256"] {
            assert!(evp_digest(name).is_some(), "{name}");
        }
        for name in ["blake2b256", "blake2s256", "sm3", "rmd160", "ssl3-sha1", "sha-512224", "shake256", "blake2b512", "ripemd", "RSA-SHA1-2", "rsa-ripemd160", "sha2-256", "sha512/256", ""] {
            assert!(evp_digest(name).is_none(), "{name}");
        }
        assert_eq!(evp_digest("sha-512/224"), Some(Algo::Sha512Trunc224));
        assert_eq!(evp_digest("sha512-256"), Some(Algo::Sha512Trunc256));
    }

    /// `createHash` soma a tabela do bun; `crypto.hash` usa só ela.
    #[test]
    fn hash_names_match_the_oracle() {
        for name in ["sha-512224", "SHA128", "shake-128", "SHAKE-256", "blake2b256", "BLAKE2B512", "Blake2s256", "rmd160", "RSA-MD5", "ecdsa-with-SHA1"] {
            assert!(hash_algorithm(name, false).is_some(), "{name}");
        }
        for name in ["RSA-SHA1-2", "ssl3-sha1", "RSA-RIPEMD160", "RSA-SHA3-256", "sha-3-256", "sha1withrsaencryption"] {
            assert!(hash_algorithm(name, false).is_none(), "{name}");
        }
        for name in ["dss1", "rsa-sha256", "md5-sha1", "sha2-256"] {
            assert!(hash_algorithm(name, true).is_none(), "{name}");
        }
        assert_eq!(hash_algorithm("sha-512/256", true), Some(Algo::Sha512Trunc256));
    }

    #[test]
    fn shake_default_lengths_are_the_bun_ones() {
        assert_eq!(default_length(Algo::Shake128), 16);
        assert_eq!(default_length(Algo::Shake256), 32);
        assert_eq!(default_length(Algo::Sha256), 32);
    }
}
