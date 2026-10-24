//! `HashObject.rs` do bun 1.4.2, o `Bun.hash`: a própria função `hash` é o wyhash e carrega os algoritmos nomeados
//! (`wyhash`, `adler32`, `crc32`, `cityHash32`, `cityHash64`, `xxHash32`, `xxHash64`, `xxHash3`, `murmur32v2`,
//! `murmur32v3`, `murmur64v2`, `rapidhash`), cada um com `length` 1, propriedades de dados comuns.
//!
//! A entrada é o conteúdo do `Blob`, os bytes vistos por typed array, `DataView` ou `ArrayBuffer`, ou o texto em UTF-8
//! de qualquer outro valor (`toString`, o surrogate solto vira U+FFFD). A semente (o segundo argumento) só vale se for
//! número ou BigInt: o BigInt entra módulo 2^64; o número é truncado e entra módulo 2^64, e de 2^64 para cima satura
//! (`toUInt64NoTruncate`). A saída de 32 bits é número; a de 64 bits é sempre BigInt (`fromUInt64NoTruncate`).

use crate::host_function;
use crate::runtime::hash_algorithms;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_big_int::{ImplResult, JSBigInt};
use crate::runtime::js_big_int_ops::{impl_result_value, to_big_uint64_value};
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_name::PropertyName;
use crate::wtf::text::wtf_string::String as WtfString;

/// Os algoritmos, com a largura da semente e da saída de cada um.
#[derive(Clone, Copy)]
enum Algorithm {
    Wyhash,
    Adler32,
    Crc32,
    CityHash32,
    CityHash64,
    XxHash32,
    XxHash64,
    XxHash3,
    Murmur32v2,
    Murmur32v3,
    Murmur64v2,
    Rapidhash,
}

/// O resultado: 32 bits viram número, 64 bits viram BigInt.
enum Digest {
    Narrow(u32),
    Wide(u64),
}

impl Algorithm {
    /// `HashAlgorithm::hash(seed, input)` de cada um: a semente chega como u64 e cada algoritmo a trunca para a sua
    /// largura; os que não têm semente a ignoram.
    fn hash(self, seed: u64, input: &[u8]) -> Digest {
        match self {
            Algorithm::Wyhash => Digest::Wide(hash_algorithms::wyhash(seed, input)),
            Algorithm::Adler32 => Digest::Narrow(hash_algorithms::adler32(input)),
            Algorithm::Crc32 => Digest::Narrow(ul_common::codec::crc32_update(seed as u32, input)),
            Algorithm::CityHash32 => Digest::Narrow(hash_algorithms::city_hash32(input)),
            Algorithm::CityHash64 => Digest::Wide(hash_algorithms::city_hash64(input, seed)),
            Algorithm::XxHash32 => Digest::Narrow(twox_hash::XxHash32::oneshot(seed as u32, input)),
            Algorithm::XxHash64 => Digest::Wide(twox_hash::XxHash64::oneshot(seed, input)),
            // A semente passa por u32 antes de voltar a u64 (o bun preserva essa truncagem).
            Algorithm::XxHash3 => Digest::Wide(twox_hash::XxHash3_64::oneshot_with_seed(u64::from(seed as u32), input)),
            Algorithm::Murmur32v2 => Digest::Narrow(hash_algorithms::murmur2_32(input, seed as u32)),
            Algorithm::Murmur32v3 => Digest::Narrow(hash_algorithms::murmur3_32(input, seed as u32)),
            Algorithm::Murmur64v2 => Digest::Wide(hash_algorithms::murmur2_64(input, seed)),
            Algorithm::Rapidhash => Digest::Wide(hash_algorithms::rapidhash(seed, input)),
        }
    }
}

/// `toUInt64NoTruncate`: BigInt módulo 2^64; número truncado módulo 2^64 (`JSC::toUInt64`), saturado de 2^64 para cima.
fn seed_of(value: JSValue) -> Option<u64> {
    if value.is_big_int() {
        return Some(to_big_uint64_value(value));
    }
    if !value.is_number() {
        return None;
    }
    let number = value.as_number();
    if number >= 18446744073709551616.0 {
        return Some(u64::MAX);
    }
    if !number.is_finite() {
        return Some(0);
    }
    // `toInt64`: a parte inteira módulo 2^64.
    Some((number.trunc() % 18446744073709551616.0) as i128 as u64)
}

/// `hash_wrap<H>`: a entrada, a semente e o resultado de um algoritmo.
fn hash_wrap(global_object: &JSGlobalObject, call: &HostCall, algorithm: Algorithm) -> HostResult {
    let input = if call.argument_count() == 0 {
        Vec::new()
    } else {
        let argument = call.argument(0);
        match crate::runtime::blob::binary_bytes(argument) {
            Some(bytes) => bytes,
            None => crate::runtime::blob::string_bytes(global_object, argument)?,
        }
    };
    let seed = seed_of(call.argument(1)).unwrap_or(0);
    Ok(match algorithm.hash(seed, &input) {
        Digest::Narrow(value) => js_number(f64::from(value)),
        Digest::Wide(value) => impl_result_value(JSBigInt::create_from_u64(value).map(ImplResult::Heap)),
    })
}

macro_rules! hash_host_functions {
    ($($host:ident, $body:ident, $algorithm:ident;)*) => {
        $(
            fn $body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                hash_wrap(global_object, call, Algorithm::$algorithm)
            }
            host_function!($host, $body);
        )*
    };
}

hash_host_functions! {
    hash_wyhash, wyhash_body, Wyhash;
    hash_adler32, adler32_body, Adler32;
    hash_crc32, crc32_body, Crc32;
    hash_city_hash32, city_hash32_body, CityHash32;
    hash_city_hash64, city_hash64_body, CityHash64;
    hash_xx_hash32, xx_hash32_body, XxHash32;
    hash_xx_hash64, xx_hash64_body, XxHash64;
    hash_xx_hash3, xx_hash3_body, XxHash3;
    hash_murmur32v2, murmur32v2_body, Murmur32v2;
    hash_murmur32v3, murmur32v3_body, Murmur32v3;
    hash_murmur64v2, murmur64v2_body, Murmur64v2;
    hash_rapidhash, rapidhash_body, Rapidhash;
}

/// Os membros de `Bun.hash`, na ordem de `put_host_functions`.
const MEMBERS: [(&str, NativeFunction); 12] = [
    ("wyhash", hash_wyhash),
    ("adler32", hash_adler32),
    ("crc32", hash_crc32),
    ("cityHash32", hash_city_hash32),
    ("cityHash64", hash_city_hash64),
    ("xxHash32", hash_xx_hash32),
    ("xxHash64", hash_xx_hash64),
    ("xxHash3", hash_xx_hash3),
    ("murmur32v2", hash_murmur32v2),
    ("murmur32v3", hash_murmur32v3),
    ("murmur64v2", hash_murmur64v2),
    ("rapidhash", hash_rapidhash),
];

fn native_function(global_object: &JSGlobalObject, name: &str, function: NativeFunction) -> JSValue {
    JSFunction::create_native(
        global_object.vm(),
        global_object,
        1,
        &WtfString::from_utf8(name.as_bytes()),
        function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    )
    .as_value()
}

/// `HashObject::create`: a função `hash` (o wyhash) com os algoritmos nomeados.
pub(crate) fn create(global_object: &JSGlobalObject) -> JSValue {
    let vm = global_object.vm();
    let hash = native_function(global_object, "hash", hash_wyhash);
    let object = hash.as_object();
    for (name, function) in MEMBERS {
        let key = PropertyName::from_identifier(&Identifier::from_span(vm, name.as_bytes()));
        object.put_direct(vm, &key, native_function(global_object, name, function), 0);
    }
    hash
}
