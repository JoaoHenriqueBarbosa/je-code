//! `crypto`, `Crypto` e `SubtleCrypto` do global. Não são do JavaScriptCore: são do bun. Medido no bun 1.4.2:
//!
//! - `crypto` é propriedade de dados comum do global; `Crypto` e `SubtleCrypto` também. `crypto` é a única
//!   instância de `Crypto` (`new Crypto()` lança `Illegal constructor`, e chamar sem `new` lança `Crypto constructor
//!   cannot be invoked without 'new'`, ambos `TypeError` com `code` `ERR_ILLEGAL_CONSTRUCTOR`); não tem chave própria;
//! - `Crypto.prototype`, nesta ordem: `getRandomValues` (`length` 0), `randomUUID` (1), `timingSafeEqual` (2), todos
//!   graváveis, enumeráveis e NÃO configuráveis; `constructor` (não enumerável); o acessor `subtle` (enumerável, não
//!   configurável, getter `get subtle` e setter `set subtle`, o setter ignora tudo, até `this` alheio); e
//!   `@@toStringTag` "Crypto". `this` alheio: `ERR_INVALID_THIS` com `Expected this to be instanceof Crypto` e o
//!   sufixo de `describe_received`; o getter lança `Value of "this" must be of type Crypto` (com `code` `ERR_INVALID_THIS`, medido no golden);
//! - `getRandomValues(x)`: aceita typed array de inteiros (inclui `Buffer`, `Uint8ClampedArray`, `BigInt64Array`,
//!   sobre `SharedArrayBuffer`) e devolve o mesmo objeto; sem limite de bytes (65537 passa; o bun 1.4.2 não lança
//!   `QuotaExceededError`). Float, `DataView`, `ArrayBuffer`, ausente ou qualquer outra coisa lança o `DOMException`
//!   `TypeMismatchError` (código 17) `The data argument must be an integer-type TypedArray`;
//! - `randomUUID()` devolve um UUID v4 de 36 caracteres e ignora argumentos;
//! - `timingSafeEqual(a, b)`: `true` para bytes iguais; tamanhos diferentes é `RangeError` `Input buffers must have
//!   the same byte length` (`ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH`); argumento que não é `ArrayBuffer`/view é
//!   `TypeError` `The "buf1" argument must be an instance of ArrayBuffer, Buffer, TypedArray, or DataView.`
//!   (`ERR_INVALID_ARG_TYPE`);
//! - `SubtleCrypto`: construtor só `Illegal constructor`; `crypto.subtle` é sempre o mesmo objeto. O protótipo tem
//!   `constructor` e os 18 métodos (`encrypt` 3, `decrypt` 3, `sign` 3, `verify` 4, `digest` 2, `generateKey` 3,
//!   `deriveKey` 5, `deriveBits` 2, `importKey` 5, `exportKey` 2, `wrapKey` 4, `unwrapKey` 7, `getPublicKey` 2,
//!   `encapsulateBits` 2, `encapsulateKey` 5, `decapsulateBits` 3, `decapsulateKey` 6), comuns, e `@@toStringTag`
//!   "SubtleCrypto"; o construtor tem a estática `supports`. Todo método devolve PROMESSA: `this` alheio rejeita
//!   com `TypeError` `Can only call SubtleCrypto.<método> on instances of SubtleCrypto` (`ERR_INVALID_THIS`) e
//!   menos argumentos que o `length` rejeita com `TypeError` `Not enough arguments` (`ERR_MISSING_ARGS`).
//!
//! - `digest(algorithm, data)`: valida `data` ANTES do algoritmo (`ERR_INVALID_ARG_TYPE`, `Received ...`), aceita
//!   SHA-1/224/256/384/512 e SHA3-256/384/512 em qualquer caixa (SHA3-224, SHA-512/256 e MD5 não existem), devolve
//!   `ArrayBuffer`; algoritmo desconhecido é `NotSupportedError` `Unrecognized algorithm name`, dicionário sem `name`
//!   é `ERR_MISSING_OPTION`. Os demais métodos reproduzem o caminho de erro (formato, usos, `CryptoKey` ausente).
//!
//! - `CryptoKey` (construtor ilegal, `ERR_ILLEGAL_CONSTRUCTOR` também sem `new`): protótipo com `constructor`, os
//!   acessores `type`, `extractable`, `algorithm` e `usages` (enumeráveis, configuráveis, só getter; `algorithm` e
//!   `usages` devolvem sempre o mesmo objeto) e `@@toStringTag`. Os tipos de chave
//!   existentes estão nos itens abaixo (HMAC, AES, ChaCha20, curva elíptica, RSA, ML-KEM, ML-DSA e as bases de
//!   derivação HKDF e PBKDF2).
//!
//! - HMAC (SHA-1/224/256/384/512 e SHA3, o `hmac` do `ul-common`): `generateKey`, `importKey` (`raw`, `raw-secret`,
//!   `jwk`), `exportKey` (`raw`, `raw-secret`, `jwk`), `sign` e `verify`, com os erros medidos no bun 1.4.2.
//!
//! As listas de algoritmos vêm do fonte do bun (`src/jsc/bindings/webcrypto`): o registro é
//! `CryptoAlgorithmRegistryOpenSSL.cpp::platformRegisterAlgorithms` (nomes em `CryptoAlgorithm*.h::s_name`) e o que cada
//! operação aceita é o `switch` de `normalizeCryptoAlgorithmParameters` em `SubtleCrypto.cpp`.
//!
//! - AES-CBC, AES-CTR, AES-GCM e AES-KW: `generateKey`, `importKey`/`exportKey` (`raw`, `raw-secret`, `jwk`), `encrypt` e
//!   `decrypt` (CBC com PKCS#7, CTR com contador que gira só nos `length` bits baixos, GCM com IV de qualquer tamanho e
//!   etiqueta de 32 a 128 bits, escritos sobre a cifra de bloco da crate `aes`), `wrapKey` e `unwrapKey` (o JWK do `AES-KW`
//!   é completado com espaços até múltiplo de 8 bytes). Uma chave AES não assina: `Key algorithm mismatch`.
//!
//! - AES-CFB-8 (registro `AES-CFB-8`, `alg` JWK `A<bits>CFB8`, registrador de 16 bytes que desliza 1 byte por vez, escrito
//!   sobre a cifra de bloco) e ChaCha20-Poly1305 (chave de 32 bytes sem `length`, `alg` JWK `C20P`, só `raw-secret` e `jwk`:
//!   o `raw` é `NotSupportedError`, IV de 12 bytes, etiqueta sempre de 128 bits, crate `chacha20poly1305`): o mesmo
//!   caminho do AES em `generateKey`, `importKey`, `exportKey`, `encrypt`, `decrypt`, `wrapKey` e `unwrapKey`.
//!
//! - ECDSA e ECDH (P-256, P-384, P-521), Ed25519 e X25519 (primitivas em `crypto_ec`): `generateKey` (`CryptoKeyPair`),
//!   `importKey`/`exportKey` (`raw`, `raw-public`, `spki`, `pkcs8`, `jwk`), `sign`/`verify` (a assinatura ECDSA é
//!   aleatória, como no bun) e `deriveBits`/`deriveKey` (ECDH e X25519).
//!
//! - ML-KEM-768/1024 e ML-DSA-44/65/87 (primitivas e DER em `crypto_pq`; o bun 1.4.2 não tem ML-KEM-512): `generateKey`,
//!   `importKey`/`exportKey` (`raw-public`, `raw-seed`, `spki`, `pkcs8`, `jwk` `AKP`) e `getPublicKey`.
//!
//! Também já existem, conferidos no código: `sign`/`verify` de ML-DSA (com o `context` do dicionário) ao lado de HMAC,
//! RSASSA-PKCS1-v1_5, RSA-PSS, ECDSA e Ed25519; `encapsulateBits`/`encapsulateKey`/`decapsulateBits`/`decapsulateKey` de
//! ML-KEM; a estática `SubtleCrypto.supports`; `encrypt`/`decrypt` de RSA-OAEP, AES-CBC/CTR/GCM/CFB-8 e ChaCha20-Poly1305;
//! `deriveBits`/`deriveKey` também de HKDF e PBKDF2; o `util.inspect.custom` do `CryptoKey` e o `getPublicKey` (curva
//! elíptica, ML-KEM, ML-DSA). O que o bun rejeita (por exemplo `RSAES-PKCS1-v1_5` fora de assinatura) responde com o
//! mesmo erro dele, nunca com um erro inventado. A fonte de aleatoriedade é `/dev/urandom`.

use std::cell::RefCell;
use std::collections::HashMap;

use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::{Aes128, Aes192, Aes256};
use aes_kw::{KekAes128, KekAes192, KekAes256};
use chacha20poly1305::aead::{Aead, KeyInit as _, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};

use crate::runtime::host::host;
use crate::runtime::iterator_operations::for_each_in_iterable;
use crate::runtime::string_regexp_support::get_object_property;
use ul_common::codec::{base64_decode, base64_encode, BASE64_URL};
use ul_common::hash::{hmac, Algo};

use crate::runtime::crypto_ec::Curve;
use crate::runtime::crypto_rsa;

// Filho de `crypto` para usar o que aqui é privado (`KeyState`, `dom_error`...).
#[path = "crypto_kdf.rs"]
mod crypto_kdf;
#[path = "crypto_pq.rs"]
mod crypto_pq;
use crate::runtime::js_array::construct_array;
use crate::runtime::node_error::throw_native_syntax_error;
use crate::runtime::object_constructor::construct_empty_object;

use crate::host_function;
use crate::runtime::array_buffer::{ArrayBuffer, ArrayBufferSharingMode};
use crate::runtime::blob::resolved_promise;
use crate::runtime::body::rejected_type_error;
use crate::runtime::js_array::JSArray;
use crate::runtime::js_array_buffer::JSArrayBuffer;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_promise::JSPromise;
use crate::runtime::json_object::json_parse_quiet;
use crate::runtime::js_typeof::js_type_string_for_value;
use crate::runtime::property_name::PropertyName;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::collection_support::put_to_string_tag;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::internal_function::INTERNAL_FUNCTION_S_INFO;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_dom_exception::throw_dom_exception_from_host;
use crate::runtime::js_function::put_direct_native_function_without_transition;
use crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayView;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::describe_received;
use crate::runtime::js_object::{JSFinalObject, JSObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_boolean, js_number, EncodedJSValue, JSValue};
use crate::runtime::native_class_support::{
    create_native_class, install_global, instance_structure, property_key, put_native_accessor, throw_coded_type_error, throw_native_type_error,
};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::node_error::throw_coded_range_error;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM};
use crate::runtime::text_decoder::input_bytes;
use crate::runtime::typed_array_type::TypedArrayType;
use crate::runtime::uint8_array_base64::create_uint8_array;
use crate::runtime::util_inspect::{inspect_named_fields, InspectOptions};
use crate::wtf::text::wtf_string::String as WtfString;

static CRYPTO_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "Crypto", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static SUBTLE_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "SubtleCrypto", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static CONSTRUCTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "Function", parent_class: Some(&INTERNAL_FUNCTION_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };
static KEY_PROTOTYPE_S_INFO: ClassInfo =
    ClassInfo { class_name: "CryptoKey", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// O estado de um `CryptoKey` (só HMAC por enquanto): o índice do hash em [`HASHES`], o segredo, `extractable`, a máscara
/// de usos (bit = posição em [`KEY_USAGES`]) e os objetos de `algorithm`/`usages`, criados uma vez.
#[derive(Clone)]
struct KeyState {
    /// `Some` quando a chave é AES (`AesCbc`, `AesCtr`, `AesGcm` ou `AesKw`); `None` é HMAC, e só aí `hash` vale.
    aes: Option<AlgorithmId>,
    /// `Some` quando a chave é de curva elíptica (ECDSA, ECDH, Ed25519, X25519); `secret` guarda então o escalar/semente
    /// (privada) ou o ponto público.
    asym: Option<Asym>,
    /// `Some` quando a chave é RSA (RSASSA-PKCS1-v1_5, RSA-PSS, RSA-OAEP); `secret` guarda então o PKCS#8 DER (privada) ou
    /// o SPKI DER (pública), e `hash` é o resumo do algoritmo.
    rsa: Option<Rsa>,
    /// `Some` quando a chave é ML-KEM ou ML-DSA; `secret` guarda então a semente (privada) ou a pública bruta.
    pq: Option<Pq>,
    hash: usize,
    secret: Vec<u8>,
    extractable: bool,
    usages: u16,
    algorithm: Option<EncodedJSValue>,
    usages_value: Option<EncodedJSValue>,
}

/// O que distingue uma chave pós-quântica: o parâmetro (`ML-KEM-768`, `ML-DSA-44`...) e se é privada.
#[derive(Clone, Copy)]
struct Pq {
    alg: crypto_pq::PqAlgorithm,
    private: bool,
}

/// O que distingue uma chave de curva elíptica: o algoritmo (`Ecdsa`, `Ecdh`, `Ed25519`, `X25519`), a curva e se é privada.
#[derive(Clone, Copy)]
struct Asym {
    id: AlgorithmId,
    curve: Curve,
    private: bool,
}

/// O que distingue uma chave RSA: o algoritmo (`RsassaPkcs1V15`, `RsaPss`, `RsaOaep`) e se é privada.
#[derive(Clone, Copy)]
struct Rsa {
    id: AlgorithmId,
    private: bool,
}

thread_local! {
    /// Por realm (`cell_id`): o `crypto` e o `crypto.subtle`, o valor codificado de cada um.
    static INSTANCES: RefCell<Vec<(usize, EncodedJSValue, EncodedJSValue)>> = const { RefCell::new(Vec::new()) };
    /// As chaves do programa (o valor codificado da célula) com o estado.
    static KEYS: RefCell<HashMap<EncodedJSValue, KeyState>> = RefCell::new(HashMap::new());
    /// Por realm (`cell_id`): o protótipo de `CryptoKey`.
    static KEY_PROTOTYPES: RefCell<Vec<(usize, EncodedJSValue)>> = const { RefCell::new(Vec::new()) };
    /// Por realm (`cell_id`): o construtor `SubtleCrypto`, o único `this` que `SubtleCrypto.supports` aceita.
    static SUBTLE_CONSTRUCTORS: RefCell<Vec<(usize, EncodedJSValue)>> = const { RefCell::new(Vec::new()) };
}

/// Fim do programa (`cell_registry::reset_program_state`).
/// As raízes do coletor: as instâncias, os protótipos e os construtores; o estado das chaves é opaco e adia a coleta
/// enquanto existir.
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    INSTANCES.with(|instances| {
        for (cell_id, first, second) in instances.borrow().iter() {
            marker.cell(*cell_id);
            marker.encoded(*first);
            marker.encoded(*second);
        }
    });
    for table in [&KEY_PROTOTYPES, &SUBTLE_CONSTRUCTORS] {
        table.with(|entries| {
            for (cell_id, value) in entries.borrow().iter() {
                marker.cell(*cell_id);
                marker.encoded(*value);
            }
        });
    }
}

/// O material de cada `CryptoKey` é campo do objeto (sem valores do programa dentro): sai com ele.
pub(crate) fn visit_side_table(marker: &mut crate::runtime::gc::Marker, phase: crate::runtime::gc::SidePhase) -> bool {
    crate::runtime::gc::side_table(&KEYS, marker, phase, |_, _| {})
}

pub(crate) fn reset_for_program() {
    let _ = INSTANCES.try_with(|instances| instances.borrow_mut().clear());
    let _ = KEYS.try_with(|keys| keys.borrow_mut().clear());
    let _ = KEY_PROTOTYPES.try_with(|prototypes| prototypes.borrow_mut().clear());
    let _ = SUBTLE_CONSTRUCTORS.try_with(|constructors| constructors.borrow_mut().clear());
}

fn is_crypto(value: JSValue) -> bool {
    INSTANCES.with(|instances| instances.borrow().iter().any(|(_, crypto, _)| *crypto == value.encode()))
}

fn is_subtle(value: JSValue) -> bool {
    INSTANCES.with(|instances| instances.borrow().iter().any(|(_, _, subtle)| *subtle == value.encode()))
}

fn illegal_constructor_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Illegal constructor", "ERR_ILLEGAL_CONSTRUCTOR"))
}

fn crypto_call_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Err(throw_coded_type_error(global_object, "Crypto constructor cannot be invoked without 'new'", "ERR_ILLEGAL_CONSTRUCTOR"))
}

/// O `this` de um método de `Crypto`, ou o `ERR_INVALID_THIS` do bun.
fn crypto_this(global_object: &JSGlobalObject, call: &HostCall) -> Result<JSValue, Thrown> {
    let this = call.this_value();
    if is_crypto(this) {
        return Ok(this);
    }
    let received = describe_received(global_object, this).map(|text| format!(", but received {text}")).unwrap_or_default();
    Err(throw_coded_type_error(global_object, &format!("Expected this to be instanceof Crypto{received}"), "ERR_INVALID_THIS"))
}

fn get_random_values_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    crypto_this(global_object, call)?;
    let data = call.argument(0);
    let integer_view = JSGenericTypedArrayView::from_value(&data)
        .filter(|view| !matches!(view.typed_array_type(), TypedArrayType::Float16 | TypedArrayType::Float32 | TypedArrayType::Float64));
    let Some(view) = integer_view else {
        return Err(throw_dom_exception_from_host(global_object, call, "TypeMismatchError", "The data argument must be an integer-type TypedArray"));
    };
    let length = view.byte_length();
    view.with_vector_mut(|vector| {
        let length = length.min(vector.len());
        host().random_fill(&mut vector[..length]);
    });
    Ok(data)
}

fn random_uuid_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    crypto_this(global_object, call)?;
    let mut bytes = [0u8; 16];
    host().random_fill(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut text = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            text.push('-');
        }
        text.push_str(&format!("{byte:02x}"));
    }
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(text.as_bytes()))))
}

fn timing_safe_equal_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    crypto_this(global_object, call)?;
    timing_safe_equal_core(global_object, call)
}

/// `timingSafeEqual(buf1, buf2)` sem o `this`: o de `Crypto.prototype` e o de `node:crypto` (`crypto.timingSafeEqual`).
fn timing_safe_equal_core(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let mut buffers = Vec::with_capacity(2);
    for (index, name) in ["buf1", "buf2"].iter().enumerate() {
        match input_bytes(call.argument(index)) {
            Some(bytes) => buffers.push(bytes),
            None => {
                let message = format!("The \"{name}\" argument must be an instance of ArrayBuffer, Buffer, TypedArray, or DataView.");
                return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
            }
        }
    }
    if buffers[0].len() != buffers[1].len() {
        return Err(throw_coded_range_error(global_object, "Input buffers must have the same byte length", "ERR_CRYPTO_TIMING_SAFE_EQUAL_LENGTH"));
    }
    let difference = buffers[0].iter().zip(&buffers[1]).fold(0u8, |accumulator, (left, right)| accumulator | (left ^ right));
    Ok(JSValue::Bool(difference == 0))
}

/// O getter `crypto.subtle`.
fn subtle_getter_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = call.this_value();
    INSTANCES
        .with(|instances| instances.borrow().iter().find(|(_, crypto, _)| *crypto == this.encode()).map(|(_, _, subtle)| JSValue::decode(*subtle)))
        .ok_or_else(|| throw_coded_type_error(global_object, "Value of \"this\" must be of type Crypto", "ERR_INVALID_THIS"))
}

/// O setter `crypto.subtle = x` ignora tudo.
fn subtle_setter_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::undefined())
}

/// `ToString` de um valor do script; símbolo lança o `TypeError` do JavaScriptCore.
fn string_of(global_object: &JSGlobalObject, value: JSValue) -> Result<String, Thrown> {
    if js_type_string_for_value(value) == "symbol" {
        return Err(throw_native_type_error(global_object, "Cannot convert a symbol to a string"));
    }
    Ok(rust_string(&value.to_wtf_string()))
}

/// O nome (em minúsculas) de um `AlgorithmIdentifier`: texto, ou dicionário com `name` obrigatório.
fn algorithm_name(global_object: &JSGlobalObject, value: JSValue) -> Result<String, Thrown> {
    algorithm_name_core(global_object, value)?.ok_or_else(|| {
        let message = "Member CryptoAlgorithmParameters.name is required and must be an instance of DOMString";
        throw_coded_type_error(global_object, message, "ERR_MISSING_OPTION")
    })
}

/// O núcleo de [`algorithm_name`]: `None` quando o dicionário não tem `name` (quem lança é o chamador; o `supports` vê `false`).
/// Só o símbolo lança aqui, porque `ToString` de símbolo é erro do próprio motor.
fn algorithm_name_core(global_object: &JSGlobalObject, value: JSValue) -> Result<Option<String>, Thrown> {
    let vm = global_object.vm();
    let name = match JSObject::from_value(&value) {
        Some(object) => {
            let name = object.get(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"name")));
            if name.is_undefined() {
                return Ok(None);
            }
            name
        }
        None => value,
    };
    Ok(Some(string_of(global_object, name)?.to_ascii_lowercase()))
}

/// Um hash do WebCrypto: nome em minúsculas (a chave do registro), nome canônico, `Algo`, o `alg` do JWK do HMAC (os
/// SHA3 não têm) e o tamanho padrão da chave HMAC em bits (`CryptoKeyHMAC.cpp::getKeyLengthFromHash`). Medido no bun
/// 1.4.2: `SHA3-224`, `SHA-512/256` e `MD5` não existem.
struct HashInfo {
    lower: &'static str,
    name: &'static str,
    algo: Algo,
    jwk_alg: Option<&'static str>,
    key_bits: usize,
}

const HASHES: [HashInfo; 8] = [
    HashInfo { lower: "sha-1", name: "SHA-1", algo: Algo::Sha1, jwk_alg: Some("HS1"), key_bits: 512 },
    HashInfo { lower: "sha-224", name: "SHA-224", algo: Algo::Sha224, jwk_alg: Some("HS224"), key_bits: 512 },
    HashInfo { lower: "sha-256", name: "SHA-256", algo: Algo::Sha256, jwk_alg: Some("HS256"), key_bits: 512 },
    HashInfo { lower: "sha-384", name: "SHA-384", algo: Algo::Sha384, jwk_alg: Some("HS384"), key_bits: 1024 },
    HashInfo { lower: "sha-512", name: "SHA-512", algo: Algo::Sha512, jwk_alg: Some("HS512"), key_bits: 1024 },
    HashInfo { lower: "sha3-256", name: "SHA3-256", algo: Algo::Sha3(32), jwk_alg: None, key_bits: 1088 },
    HashInfo { lower: "sha3-384", name: "SHA3-384", algo: Algo::Sha3(48), jwk_alg: None, key_bits: 832 },
    HashInfo { lower: "sha3-512", name: "SHA3-512", algo: Algo::Sha3(64), jwk_alg: None, key_bits: 576 },
];

fn hash_index(lower_name: &str) -> Option<usize> {
    HASHES.iter().position(|hash| hash.lower == lower_name)
}

fn digest_algo(name: &str) -> Option<Algo> {
    hash_index(name).map(|index| HASHES[index].algo)
}

/// Os identificadores do registro (`CryptoAlgorithmIdentifier.h`), na ordem dele.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AlgorithmId {
    RsaesPkcs1V15,
    RsassaPkcs1V15,
    RsaPss,
    RsaOaep,
    Ecdsa,
    Ecdh,
    AesCtr,
    AesCbc,
    AesGcm,
    AesCfb,
    AesKw,
    Hmac,
    Sha,
    Hkdf,
    Pbkdf2,
    Ed25519,
    X25519,
    ChaCha20Poly1305,
    MlDsa,
    MlKem,
}

/// O registro (`CryptoAlgorithmRegistryOpenSSL.cpp::platformRegisterAlgorithms`, nomes de `CryptoAlgorithm*.h::s_name`,
/// em minúsculas): `AES-CFB` se registra como `AES-CFB-8`. Os resumos SHA vêm de [`HASHES`].
const REGISTRY: &[(&str, AlgorithmId)] = &[
    ("aes-cbc", AlgorithmId::AesCbc),
    ("aes-cfb-8", AlgorithmId::AesCfb),
    ("aes-ctr", AlgorithmId::AesCtr),
    ("aes-gcm", AlgorithmId::AesGcm),
    ("aes-kw", AlgorithmId::AesKw),
    ("chacha20-poly1305", AlgorithmId::ChaCha20Poly1305),
    ("ecdh", AlgorithmId::Ecdh),
    ("ecdsa", AlgorithmId::Ecdsa),
    ("hkdf", AlgorithmId::Hkdf),
    ("hmac", AlgorithmId::Hmac),
    ("pbkdf2", AlgorithmId::Pbkdf2),
    ("rsaes-pkcs1-v1_5", AlgorithmId::RsaesPkcs1V15),
    ("rsassa-pkcs1-v1_5", AlgorithmId::RsassaPkcs1V15),
    ("rsa-oaep", AlgorithmId::RsaOaep),
    ("rsa-pss", AlgorithmId::RsaPss),
    ("ed25519", AlgorithmId::Ed25519),
    ("x25519", AlgorithmId::X25519),
    ("ml-dsa-44", AlgorithmId::MlDsa),
    ("ml-dsa-65", AlgorithmId::MlDsa),
    ("ml-dsa-87", AlgorithmId::MlDsa),
    ("ml-kem-768", AlgorithmId::MlKem),
    ("ml-kem-1024", AlgorithmId::MlKem),
];

fn algorithm_id(lower_name: &str) -> Option<AlgorithmId> {
    if hash_index(lower_name).is_some() {
        return Some(AlgorithmId::Sha);
    }
    REGISTRY.iter().find(|(name, _)| *name == lower_name).map(|(_, id)| *id)
}

/// A operação de `normalizeCryptoAlgorithmParameters` cujo `switch` decide o que o algoritmo aceita.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    GenerateKey,
    ImportKey,
    SignVerify,
    /// `deriveBits` e `deriveKey`.
    Derive,
    /// `encrypt` e `decrypt`.
    Encrypt,
    /// `wrapKey` e `unwrapKey`: o que o `encrypt` aceita mais o `AES-KW`.
    WrapKey,
    /// `encapsulateBits`, `encapsulateKey`, `decapsulateBits` e `decapsulateKey`: só ML-KEM.
    Encapsulate,
}

/// O que cada operação aceita (os `case` de `SubtleCrypto.cpp`); fora da lista é `Unrecognized algorithm name`.
fn operation_accepts(operation: Operation, id: AlgorithmId) -> bool {
    use AlgorithmId::*;
    match operation {
        // `HKDF`, `PBKDF2` e os SHA caem no `default` do `generateKey`.
        Operation::GenerateKey => !matches!(id, Sha | Hkdf | Pbkdf2),
        Operation::ImportKey => id != Sha,
        Operation::SignVerify => matches!(id, RsassaPkcs1V15 | Hmac | Ed25519 | Ecdsa | RsaPss | MlDsa),
        Operation::Derive => matches!(id, Ecdh | X25519 | Hkdf | Pbkdf2),
        Operation::Encrypt => matches!(id, RsaOaep | AesCtr | AesCbc | AesGcm | AesCfb | ChaCha20Poly1305),
        Operation::WrapKey => matches!(id, RsaOaep | AesCtr | AesCbc | AesGcm | AesCfb | AesKw | ChaCha20Poly1305),
        Operation::Encapsulate => id == MlKem,
    }
}

const KEY_FORMATS: &[&str] = &["raw", "jwk", "spki", "pkcs8", "raw-secret", "raw-public", "raw-seed"];
const KEY_USAGES: &[&str] = &[
    "encrypt", "decrypt", "sign", "verify", "deriveKey", "deriveBits", "wrapKey", "unwrapKey", "encapsulateKey", "encapsulateBits", "decapsulateKey",
    "decapsulateBits",
];
const USAGE_ENCRYPT: u16 = 1 << 0;
const USAGE_DECRYPT: u16 = 1 << 1;
const USAGE_SIGN: u16 = 1 << 2;
const USAGE_VERIFY: u16 = 1 << 3;
const USAGE_DERIVE_KEY: u16 = 1 << 4;
const USAGE_DERIVE_BITS: u16 = 1 << 5;
const USAGE_WRAP: u16 = 1 << 6;
const USAGE_UNWRAP: u16 = 1 << 7;
/// Os usos que uma chave HMAC aceita (`usagesAreInvalidForCryptoAlgorithmHMAC` proíbe todos os outros).
const HMAC_USAGES: u16 = USAGE_SIGN | USAGE_VERIFY;
/// Os usos que uma chave AES aceita; o `AES-KW` só embrulha e desembrulha.
fn aes_usages(id: AlgorithmId) -> u16 {
    if id == AlgorithmId::AesKw { USAGE_WRAP | USAGE_UNWRAP } else { USAGE_ENCRYPT | USAGE_DECRYPT | USAGE_WRAP | USAGE_UNWRAP }
}

/// Os algoritmos de chave simétrica de cifragem (AES e ChaCha20-Poly1305), os que usam o caminho de chave AES do módulo.
fn is_symmetric_id(id: AlgorithmId) -> bool {
    matches!(
        id,
        AlgorithmId::AesCbc | AlgorithmId::AesCtr | AlgorithmId::AesGcm | AlgorithmId::AesKw | AlgorithmId::AesCfb | AlgorithmId::ChaCha20Poly1305
    )
}

/// O `SyntaxError` de uso proibido: `an AES key` no AES, `a ChaCha20-Poly1305 key` no ChaCha.
fn unsupported_usage(global_object: &JSGlobalObject, id: AlgorithmId) -> Thrown {
    let kind = if id == AlgorithmId::ChaCha20Poly1305 { "a ChaCha20-Poly1305" } else { "an AES" };
    throw_native_syntax_error(global_object, &format!("Unsupported key usage for {kind} key"))
}

/// O `NotSupportedError` de nome de algoritmo desconhecido.
fn not_supported(global_object: &JSGlobalObject, call: &HostCall) -> Thrown {
    throw_dom_exception_from_host(global_object, call, "NotSupportedError", "Unrecognized algorithm name")
}

/// `normalizeCryptoAlgorithmParameters` até a identidade: o nome tem de estar no registro, `RSAES-PKCS1-v1_5` é recusado
/// como obsoleto e a operação tem de aceitar o algoritmo.
fn normalize_algorithm(global_object: &JSGlobalObject, call: &HostCall, value: JSValue, operation: Operation) -> Result<AlgorithmId, Thrown> {
    normalize_named(global_object, call, &algorithm_name(global_object, value)?, operation)
}

/// [`normalize_algorithm`] com o nome (em minúsculas) já lido: quem precisa do nome depois (ML-KEM e ML-DSA) o lê uma vez só,
/// porque `name` pode ser um getter observável.
fn normalize_named(global_object: &JSGlobalObject, call: &HostCall, lower_name: &str, operation: Operation) -> Result<AlgorithmId, Thrown> {
    resolve_algorithm(lower_name, operation).map_err(|failure| match failure {
        NormalizeFailure::Unrecognized => not_supported(global_object, call),
        NormalizeFailure::Deprecated => {
            throw_dom_exception_from_host(global_object, call, "NotSupportedError", "RSAES-PKCS1-v1_5 support is deprecated")
        }
    })
}

/// Por que um nome não normaliza: o `NotSupportedError` de cada caso é montado por [`normalize_named`].
enum NormalizeFailure {
    Unrecognized,
    Deprecated,
}

/// O núcleo de [`normalize_named`], sem lançar nada: o `supports` trata qualquer falha como `false`.
fn resolve_algorithm(lower_name: &str, operation: Operation) -> Result<AlgorithmId, NormalizeFailure> {
    let id = algorithm_id(lower_name).ok_or(NormalizeFailure::Unrecognized)?;
    if id == AlgorithmId::RsaesPkcs1V15 && operation != Operation::SignVerify {
        return Err(NormalizeFailure::Deprecated);
    }
    if !operation_accepts(operation, id) {
        return Err(NormalizeFailure::Unrecognized);
    }
    Ok(id)
}

/// A lista de `KeyUsage` como máscara de bits (posição em [`KEY_USAGES`]): não iterável é `Value is not a sequence`,
/// objeto não array é `Type error`, elemento que não é texto ou fora do enum, os erros do WebIDL do bun.
fn check_usages(global_object: &JSGlobalObject, value: JSValue) -> Result<u16, Thrown> {
    // Sequência do WebIDL: qualquer iterável de objeto (`Array`, `Set`), medido no bun 1.4.2. Objeto sem `@@iterator` é
    // `Type error`; valor que não é objeto, `Value is not a sequence`.
    if JSObject::from_value(&value).is_none() {
        return Err(throw_coded_type_error(global_object, "Value is not a sequence", "ERR_INVALID_ARG_TYPE"));
    }
    let vm = global_object.vm();
    let iterator = get_object_property(global_object, value, &vm.property_names.iterator_symbol)?;
    if !iterator.is_callable() {
        return Err(throw_native_type_error(global_object, "Type error"));
    }
    let mut mask = 0u16;
    for_each_in_iterable(global_object, value, |element| {
        if !element.is_string() {
            return Err(Thrown::type_error("value must be a string"));
        }
        let Some(bit) = KEY_USAGES.iter().position(|usage| *usage == rust_string(&element.to_wtf_string()).as_str()) else {
            return Err(Thrown::type_error("value must be enumeration (string)"));
        };
        mask |= 1 << bit;
        Ok(())
    })?;
    Ok(mask)
}

/// Os nomes dos bits de `mask`, na ordem de [`KEY_USAGES`].
fn usage_names(mask: u16) -> Vec<&'static str> {
    KEY_USAGES.iter().enumerate().filter(|(bit, _)| mask & (1 << bit) != 0).map(|(_, name)| *name).collect()
}

/// Os membros de um `JsonWebKey` que o HMAC lê, na ordem alfabética em que o WebIDL os converte.
struct Jwk {
    alg: Option<String>,
    crv: Option<String>,
    d: Option<String>,
    dp: Option<String>,
    dq: Option<String>,
    e: Option<String>,
    ext: Option<bool>,
    k: Option<String>,
    key_ops: Option<Vec<String>>,
    kty: Option<String>,
    n: Option<String>,
    p: Option<String>,
    /// `priv` (a semente) e `pub` (a pública bruta) do JWK `AKP` de ML-KEM e ML-DSA.
    priv_: Option<String>,
    pub_: Option<String>,
    q: Option<String>,
    qi: Option<String>,
    use_: Option<String>,
    x: Option<String>,
    y: Option<String>,
}

/// Converte `data` (objeto que não é `BufferSource`) no dicionário `JsonWebKey`.
fn parse_jwk(global_object: &JSGlobalObject, object: &JSObject) -> Result<Jwk, Thrown> {
    let vm = global_object.vm();
    let member = |name: &str| object.get(vm, &property_key(vm, name));
    let text = |value: JSValue| if value.is_undefined() { Ok(None) } else { string_of(global_object, value).map(Some) };
    let alg = text(member("alg"))?;
    let crv = text(member("crv"))?;
    let d = text(member("d"))?;
    let dp = text(member("dp"))?;
    let dq = text(member("dq"))?;
    let e = text(member("e"))?;
    let ext = Some(member("ext")).filter(|value| !value.is_undefined()).map(|value| value.to_boolean());
    let k = text(member("k"))?;
    let key_ops_value = member("key_ops");
    let key_ops = if key_ops_value.is_undefined() {
        None
    } else {
        check_usages(global_object, key_ops_value)?;
        let array = JSArray::from_value(&key_ops_value).expect("key_ops validado como array");
        let object = JSObject::from_value(&key_ops_value).expect("array é objeto");
        Some((0..array.length()).map(|index| rust_string(&object.get_by_index(vm, index).to_wtf_string())).collect())
    };
    let kty = text(member("kty"))?;
    let n = text(member("n"))?;
    let p = text(member("p"))?;
    let priv_ = text(member("priv"))?;
    let pub_ = text(member("pub"))?;
    let q = text(member("q"))?;
    let qi = text(member("qi"))?;
    let use_ = text(member("use"))?;
    let x = text(member("x"))?;
    let y = text(member("y"))?;
    Ok(Jwk { alg, crv, d, dp, dq, e, ext, k, key_ops, kty, n, p, priv_, pub_, q, qi, use_, x, y })
}

fn has_duplicate(operations: &[String]) -> bool {
    operations.iter().enumerate().any(|(index, operation)| operations[..index].contains(operation))
}

fn usage_mask_of(names: &[String]) -> u16 {
    names.iter().filter_map(|name| KEY_USAGES.iter().position(|usage| usage == name)).fold(0, |mask, bit| mask | (1 << bit))
}

fn check_format(global_object: &JSGlobalObject, name: &str, value: JSValue) -> Result<String, Thrown> {
    let format = string_of(global_object, value)?;
    if KEY_FORMATS.contains(&format.as_str()) {
        return Ok(format);
    }
    let message = format!("Failed to execute '{name}' on 'SubtleCrypto': 1st argument '{format}' is not a valid enum value of type KeyFormat.");
    Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_VALUE"))
}

/// O argumento que deveria ser uma chave e não é falha como no bun.
fn not_a_key(global_object: &JSGlobalObject, name: &str, position: usize, argument: &str) -> Thrown {
    let message = format!("Argument {position} ('{argument}') to SubtleCrypto.{name} must be an instance of CryptoKey");
    throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE")
}

/// A chave que é o argumento `index` da chamada (`position` e `argument` são os da mensagem de erro).
fn key_at(global_object: &JSGlobalObject, call: &HostCall, name: &str, index: usize, position: usize, argument: &str) -> Result<KeyState, Thrown> {
    KEYS.with(|keys| keys.borrow().get(&call.argument(index).encode()).cloned()).ok_or_else(|| not_a_key(global_object, name, position, argument))
}

/// O `BufferSource` do WebIDL do bun: qualquer outra coisa é `TypeError` `Type error`.
fn buffer_source(global_object: &JSGlobalObject, value: JSValue) -> Result<Vec<u8>, Thrown> {
    input_bytes(value).ok_or_else(|| throw_native_type_error(global_object, "Type error"))
}

fn ascii_value(global_object: &JSGlobalObject, text: &str) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_latin1(text.as_bytes())))
}

fn array_buffer_value(global_object: &JSGlobalObject, bytes: &[u8]) -> JSValue {
    let structure = global_object.array_buffer_realm.array_buffer_structure(ArrayBufferSharingMode::Default);
    JSArrayBuffer::create(global_object.vm(), &structure, ArrayBuffer::create_from_span(bytes)).as_value()
}

fn dom_error(global_object: &JSGlobalObject, call: &HostCall, name: &str, message: &str) -> Thrown {
    throw_dom_exception_from_host(global_object, call, name, message)
}

/// A mensagem padrão que `rejectWithException` dá a um erro sem texto (`OperationError`, `NotSupportedError`).
const OPERATION_FAILED: &str = "The operation failed for an operation-specific reason";
const NOT_SUPPORTED: &str = "The algorithm is not supported";

/// `CryptoAlgorithmHmacKeyParams`: o índice do hash em [`HASHES`] e o `length` (bits) opcional. Dicionário sem `hash` (o
/// texto `"HMAC"` também) é `ERR_MISSING_OPTION`; hash fora dos resumos do `digest` é `Unrecognized algorithm name`.
fn hmac_params(global_object: &JSGlobalObject, call: &HostCall, value: JSValue) -> Result<(usize, Option<u32>), Thrown> {
    let vm = global_object.vm();
    let object = JSObject::from_value(&value);
    let hash_value = object.as_ref().map_or_else(JSValue::undefined, |object| object.get(vm, &property_key(vm, "hash")));
    if hash_value.is_undefined() {
        let message = "Member HmacKeyParams.hash is required and must be an instance of (object or DOMString)";
        return Err(throw_coded_type_error(global_object, message, "ERR_MISSING_OPTION"));
    }
    let length_value = object.as_ref().map_or_else(JSValue::undefined, |object| object.get(vm, &property_key(vm, "length")));
    let length = (!length_value.is_undefined()).then(|| {
        let number = length_value.to_number();
        if number.is_finite() { (number.trunc() as i64 & 0xffff_ffff) as u32 } else { 0 }
    });
    let hash = hash_index(&algorithm_name(global_object, hash_value)?).ok_or_else(|| not_supported(global_object, call))?;
    Ok((hash, length))
}

/// Cria o `CryptoKey` (objeto com o protótipo do realm) e guarda o estado dele.
fn create_key(global_object: &JSGlobalObject, state: KeyState) -> JSValue {
    let vm = global_object.vm();
    let realm = global_object.cell_id();
    let prototype = KEY_PROTOTYPES
        .with(|prototypes| prototypes.borrow().iter().find(|(id, _)| *id == realm).map(|(_, prototype)| JSValue::decode(*prototype)))
        .expect("CryptoKey sem protótipo (instalação)");
    let key = JSFinalObject::create(vm, &instance_structure(vm, Some(global_object), prototype)).as_value();
    KEYS.with(|keys| keys.borrow_mut().insert(key.encode(), state));
    key
}

/// `value` é uma `CryptoKey` (o `inherits<JSCryptoKey>` do `util.types.isCryptoKey`).
pub(crate) fn is_crypto_key(value: JSValue) -> bool {
    KEYS.with(|keys| keys.borrow().contains_key(&value.encode()))
}

/// `structuredClone` de uma `CryptoKey` (`CryptoKeyTag` do WebCore): a chave nova leva o mesmo estado (tipo, algoritmo, usos,
/// material e `extractable`); `None` quando `value` não é uma chave.
pub(crate) fn clone_crypto_key(global_object: &JSGlobalObject, value: &JSValue) -> Option<JSValue> {
    let state = KEYS.with(|keys| keys.borrow().get(&value.encode()).cloned())?;
    Some(create_key(global_object, state))
}

fn hmac_secret_key(hash: usize, secret: Vec<u8>, extractable: bool, usages: u16) -> KeyState {
    KeyState { aes: None, asym: None, rsa: None, pq: None, hash, secret, extractable, usages, algorithm: None, usages_value: None }
}

fn aes_secret_key(id: AlgorithmId, secret: Vec<u8>, extractable: bool, usages: u16) -> KeyState {
    KeyState { aes: Some(id), asym: None, rsa: None, pq: None, hash: 0, secret, extractable, usages, algorithm: None, usages_value: None }
}

fn asym_key(asym: Asym, secret: Vec<u8>, extractable: bool, usages: u16) -> KeyState {
    KeyState { aes: None, asym: Some(asym), rsa: None, pq: None, hash: 0, secret, extractable, usages, algorithm: None, usages_value: None }
}

fn rsa_key(rsa: Rsa, hash: usize, der: Vec<u8>, extractable: bool, usages: u16) -> KeyState {
    KeyState { aes: None, asym: None, rsa: Some(rsa), pq: None, hash, secret: der, extractable, usages, algorithm: None, usages_value: None }
}

/// Uma chave ML-KEM ou ML-DSA: `secret` é a semente (privada) ou a pública bruta.
fn pq_key(pq: Pq, secret: Vec<u8>, extractable: bool, usages: u16) -> KeyState {
    KeyState { aes: None, asym: None, rsa: None, pq: Some(pq), hash: 0, secret, extractable, usages, algorithm: None, usages_value: None }
}

/// O nome canônico do algoritmo AES (o `name` de `algorithm` e das mensagens de erro).
fn aes_name(id: AlgorithmId) -> &'static str {
    match id {
        AlgorithmId::AesCbc => "AES-CBC",
        AlgorithmId::AesCfb => "AES-CFB-8",
        AlgorithmId::ChaCha20Poly1305 => "ChaCha20-Poly1305",
        AlgorithmId::AesCtr => "AES-CTR",
        AlgorithmId::AesGcm => "AES-GCM",
        AlgorithmId::Hkdf | AlgorithmId::Pbkdf2 => crypto_kdf::kdf_name(id),
        _ => "AES-KW",
    }
}

/// O nome canônico de um algoritmo de curva elíptica.
fn asym_name(id: AlgorithmId) -> &'static str {
    match id {
        AlgorithmId::Ecdsa => "ECDSA",
        AlgorithmId::Ecdh => "ECDH",
        AlgorithmId::Ed25519 => "Ed25519",
        _ => "X25519",
    }
}

fn is_asym_id(id: AlgorithmId) -> bool {
    matches!(id, AlgorithmId::Ecdsa | AlgorithmId::Ecdh | AlgorithmId::Ed25519 | AlgorithmId::X25519)
}

fn base64_url_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&base64_encode(bytes, BASE64_URL, false)).into_owned()
}

/// A decodificação base64url leniente do bun: entrada inválida vira vazio. O JWK grava base64url sem padding, e o
/// decodificador compartilhado (`binascii`) exige o `=`, então o padding é completado antes.
fn base64_url_bytes(text: &str) -> Vec<u8> {
    // Medido no bun: `+` e `/` não são base64url, a entrada inteira vira vazio.
    if text.contains(['+', '/']) {
        return Vec::new();
    }
    let mut standard = text.replace('-', "+").replace('_', "/");
    while standard.len() % 4 != 0 {
        standard.push('=');
    }
    base64_decode(standard.as_bytes(), false).unwrap_or_default()
}

/// Os usos que uma chave de curva elíptica aceita: assinar/verificar (ECDSA, Ed25519) ou derivar (ECDH, X25519, a pública
/// não tem uso).
fn asym_usages(id: AlgorithmId, private: bool) -> u16 {
    match (id, private) {
        (AlgorithmId::Ecdsa | AlgorithmId::Ed25519, true) => USAGE_SIGN,
        (AlgorithmId::Ecdsa | AlgorithmId::Ed25519, false) => USAGE_VERIFY,
        (_, true) => USAGE_DERIVE_KEY | USAGE_DERIVE_BITS,
        (_, false) => 0,
    }
}

/// `type` do `CryptoKey`.
fn key_type_name(key: &KeyState) -> &'static str {
    let private = key.rsa.map(|rsa| rsa.private).or_else(|| key.asym.map(|asym| asym.private)).or_else(|| key.pq.map(|pq| pq.private));
    match private {
        None => "secret",
        Some(true) => "private",
        Some(false) => "public",
    }
}

/// A curva do algoritmo: o `namedCurve` obrigatório do ECDSA/ECDH (curva desconhecida é `NotSupportedError`), ou a fixa do
/// Ed25519/X25519.
/// No `importKey` (`importing`), medido no bun 1.4.2, a curva desconhecida é `DataError` `Invalid keyData`.
fn asym_curve(global_object: &JSGlobalObject, call: &HostCall, id: AlgorithmId, algorithm: JSValue, importing: bool) -> Result<Curve, Thrown> {
    match id {
        AlgorithmId::Ed25519 => Ok(Curve::Ed25519),
        AlgorithmId::X25519 => Ok(Curve::X25519),
        _ => {
            let name = required_member(global_object, algorithm, "EcKeyParams", "namedCurve", "DOMString")?;
            Curve::from_named(&string_of(global_object, name)?).ok_or_else(|| {
                if importing {
                    dom_error(global_object, call, "DataError", "Invalid keyData")
                } else {
                    dom_error(global_object, call, "NotSupportedError", NOT_SUPPORTED)
                }
            })
        }
    }
}

/// O ponto público da chave: o próprio `secret` numa pública, derivado do escalar numa privada.
fn asym_public(key: &KeyState) -> Vec<u8> {
    let asym = key.asym.expect("chave de curva elíptica");
    if asym.private { asym.curve.public_of(&key.secret).unwrap_or_default() } else { key.secret.clone() }
}

/// Um membro do JWK de curva elíptica, na ordem alfabética do `exportKey('jwk')`.
enum JwkMember {
    Text(String),
    Bool(bool),
    Usages(u16),
}

fn asym_jwk_members(key: &KeyState) -> Vec<(&'static str, JwkMember)> {
    if key.rsa.is_some() {
        return rsa_jwk_members(key);
    }
    if let Some(pq) = key.pq {
        return pq_jwk_members(key, pq);
    }
    let asym = key.asym.expect("chave de curva elíptica");
    let public = asym_public(key);
    let mut members = Vec::new();
    if asym.id == AlgorithmId::Ed25519 {
        members.push(("alg", JwkMember::Text("Ed25519".to_owned())));
    }
    members.push(("crv", JwkMember::Text(asym.curve.name().to_owned())));
    if asym.private {
        members.push(("d", JwkMember::Text(base64_url_text(&key.secret))));
    }
    members.push(("ext", JwkMember::Bool(key.extractable)));
    members.push(("key_ops", JwkMember::Usages(key.usages)));
    members.push(("kty", JwkMember::Text(if asym.curve.is_nist() { "EC" } else { "OKP" }.to_owned())));
    if asym.curve.is_nist() {
        let size = asym.curve.field_size();
        members.push(("x", JwkMember::Text(base64_url_text(&public[1..=size]))));
        members.push(("y", JwkMember::Text(base64_url_text(&public[1 + size..]))));
    } else {
        members.push(("x", JwkMember::Text(base64_url_text(&public))));
    }
    members
}

fn asym_jwk_value(global_object: &JSGlobalObject, key: &KeyState) -> JSValue {
    let vm = global_object.vm();
    let object = construct_empty_object(global_object);
    for (name, member) in asym_jwk_members(key) {
        let value = match member {
            JwkMember::Text(text) => ascii_value(global_object, &text),
            JwkMember::Bool(flag) => JSValue::Bool(flag),
            JwkMember::Usages(mask) => usages_array(global_object, mask),
        };
        object.put_direct(vm, &property_key(vm, name), value, 0);
    }
    object.as_value()
}

fn asym_jwk_json(key: &KeyState) -> String {
    let members: Vec<String> = asym_jwk_members(key)
        .into_iter()
        .map(|(name, member)| match member {
            JwkMember::Text(text) => format!("\"{name}\":\"{text}\""),
            JwkMember::Bool(flag) => format!("\"{name}\":{flag}"),
            JwkMember::Usages(mask) => {
                let operations: Vec<String> = usage_names(mask).into_iter().map(|operation| format!("\"{operation}\"")).collect();
                format!("\"{name}\":[{}]", operations.join(","))
            }
        })
        .collect();
    format!("{{{}}}", members.join(","))
}

/// `generateKey` de ML-KEM e ML-DSA: a privada guarda a semente e segue o `extractable` pedido; a pública (derivada da
/// semente) é sempre extraível. Uso fora do conjunto do parâmetro é `Unsupported key usage`, privada sem uso é vazio.
fn generate_pq_key(global_object: &JSGlobalObject, call: &HostCall, alg: crypto_pq::PqAlgorithm, usages: u16) -> HostResult {
    let (public_usages, private_usages) = alg.split_generate_usages(usages).map_err(|error| match error {
        crypto_pq::UsageError::Unsupported => throw_native_syntax_error(global_object, &alg.unsupported_usage_message(false)),
        crypto_pq::UsageError::Empty => throw_native_syntax_error(global_object, "Usages cannot be empty when creating a key."),
    })?;
    let seed = alg.generate_seed();
    let public = alg.public_from_seed(&seed).expect("semente recém-gerada tem o tamanho do parâmetro");
    let private_key = create_key(global_object, pq_key(Pq { alg, private: true }, seed, call.argument(1).to_boolean(), private_usages));
    let public_key = create_key(global_object, pq_key(Pq { alg, private: false }, public, true, public_usages));
    let vm = global_object.vm();
    let pair = construct_empty_object(global_object);
    pair.put_direct(vm, &property_key(vm, "privateKey"), private_key, 0);
    pair.put_direct(vm, &property_key(vm, "publicKey"), public_key, 0);
    Ok(pair.as_value())
}

/// As verificações de `exportKey` de uma chave ML-KEM ou ML-DSA: extraível, formato que o tipo exporta.
fn check_pq_exportable(global_object: &JSGlobalObject, call: &HostCall, key: &KeyState, format: &str) -> Result<(), Thrown> {
    let pq = key.pq.expect("chave pós-quântica");
    if !key.extractable {
        return Err(dom_error(global_object, call, "InvalidAccessError", "key is not extractable"));
    }
    if crypto_pq::PqAlgorithm::exports_format(pq.private, format) {
        return Ok(());
    }
    Err(dom_error(global_object, call, "NotSupportedError", &pq.alg.unable_to_export_message(pq.private, format)))
}

/// `Unsupported key usage for a ML-KEM-768 key` (com `a`, a mensagem da importação) quando `usages` tem algo que o tipo
/// (privado ou público) do parâmetro não aceita.
fn check_pq_usages(global_object: &JSGlobalObject, alg: crypto_pq::PqAlgorithm, private: bool, usages: u16) -> Result<(), Thrown> {
    if usages & !alg.allowed_usages(private) != 0 {
        return Err(throw_native_syntax_error(global_object, &alg.unsupported_usage_message(true)));
    }
    Ok(())
}

/// A semente (privada) ou a pública bruta de um JWK `AKP`, na ordem de validação do bun: `kty`, `alg` e `pub` presentes,
/// `use`, `key_ops` duplicado e incompatível, `ext`, `alg`, os usos do tipo, e por fim a decodificação (a `pub` de uma
/// privada tem de ser a derivada da semente).
fn pq_jwk_material(
    global_object: &JSGlobalObject,
    call: &HostCall,
    jwk: &Jwk,
    alg: crypto_pq::PqAlgorithm,
    extractable: bool,
    usages: u16,
) -> Result<(Vec<u8>, bool), Thrown> {
    let data_error = |message: &str| dom_error(global_object, call, "DataError", message);
    let Some(kty) = jwk.kty.as_deref() else { return Err(data_error("Invalid keyData")) };
    if kty != "AKP" {
        return Err(data_error("Invalid JWK \"kty\" Parameter"));
    }
    let (Some(given_alg), Some(public_text)) = (jwk.alg.as_deref(), jwk.pub_.as_deref()) else { return Err(data_error("Invalid keyData")) };
    let use_value = if alg.is_kem() { "enc" } else { "sig" };
    if usages != 0 && jwk.use_.as_deref().is_some_and(|use_| use_ != use_value) {
        return Err(data_error("Invalid JWK \"use\" Parameter"));
    }
    if jwk.key_ops.as_deref().is_some_and(has_duplicate) {
        return Err(data_error("Duplicate key operation"));
    }
    if jwk.key_ops.as_deref().is_some_and(|operations| usage_mask_of(operations) & usages != usages) {
        return Err(data_error("Key operations and usage mismatch"));
    }
    if jwk.ext == Some(false) && extractable {
        return Err(data_error("JWK \"ext\" Parameter and extractable mismatch"));
    }
    if given_alg != alg.name() {
        return Err(data_error("JWK \"alg\" Parameter and algorithm name mismatch"));
    }
    let private = jwk.priv_.is_some();
    check_pq_usages(global_object, alg, private, usages)?;
    let public = base64_url_bytes(public_text);
    let Some(seed_text) = jwk.priv_.as_deref() else {
        return if public.len() == alg.public_len() { Ok((public, false)) } else { Err(data_error("Invalid keyData")) };
    };
    let seed = base64_url_bytes(seed_text);
    if alg.public_from_seed(&seed).as_deref() != Some(public.as_slice()) {
        return Err(data_error("Invalid keyData"));
    }
    Ok((seed, true))
}

/// `importKey` de ML-KEM e ML-DSA (`raw-seed`, `raw-public`, `spki`, `pkcs8`, `jwk`; `raw` e `raw-secret` não existem). Os
/// usos são conferidos antes dos dados, pelo tipo que o formato produz; usos vazios só são recusados na privada.
fn import_pq_key(
    global_object: &JSGlobalObject,
    call: &HostCall,
    alg: crypto_pq::PqAlgorithm,
    format: &str,
    bytes: Option<Vec<u8>>,
    jwk: Option<Jwk>,
    extractable: bool,
    usages: u16,
) -> Result<KeyState, Thrown> {
    // `toKeyData`: `jwk` ausente (`null`/`undefined`) é `DataError`; outro valor que não é objeto, `TypeError`.
    if format == "jwk" && jwk.is_none() {
        return Err(missing_jwk_error(global_object, call, false));
    }
    if matches!(format, "raw" | "raw-secret") {
        let message = format!("Unable to import {} using {format} format", alg.name());
        return Err(dom_error(global_object, call, "NotSupportedError", &message));
    }
    let (secret, private) = match jwk {
        Some(jwk) => pq_jwk_material(global_object, call, &jwk, alg, extractable, usages)?,
        None => {
            let private = matches!(format, "raw-seed" | "pkcs8");
            check_pq_usages(global_object, alg, private, usages)?;
            let bytes = bytes.unwrap_or_default();
            let der_error = |error: crypto_pq::DerError| {
                let message = if error == crypto_pq::DerError::InvalidKeyType { "Invalid key type" } else { "Invalid keyData" };
                dom_error(global_object, call, "DataError", message)
            };
            let secret = match format {
                "raw-seed" => (bytes.len() == alg.seed_len()).then_some(bytes),
                "raw-public" => (bytes.len() == alg.public_len()).then_some(bytes),
                "spki" => Some(alg.decode_spki(&bytes).map_err(der_error)?),
                _ => Some(alg.decode_pkcs8(&bytes).map_err(der_error)?),
            };
            (secret.ok_or_else(|| dom_error(global_object, call, "DataError", "Invalid keyData"))?, private)
        }
    };
    if private && usages == 0 {
        return Err(throw_native_syntax_error(global_object, "Usages cannot be empty when importing a private key."));
    }
    Ok(pq_key(Pq { alg, private }, secret, extractable, usages))
}

/// Os membros do JWK `AKP` de uma chave ML-KEM ou ML-DSA, em ordem alfabética: `priv` é a semente e `pub` a pública bruta.
fn pq_jwk_members(key: &KeyState, pq: Pq) -> Vec<(&'static str, JwkMember)> {
    let public = if pq.private { pq.alg.public_from_seed(&key.secret).unwrap_or_default() } else { key.secret.clone() };
    let mut members = vec![
        ("alg", JwkMember::Text(pq.alg.name().to_owned())),
        ("ext", JwkMember::Bool(key.extractable)),
        ("key_ops", JwkMember::Usages(key.usages)),
        ("kty", JwkMember::Text("AKP".to_owned())),
    ];
    if pq.private {
        members.push(("priv", JwkMember::Text(base64_url_text(&key.secret))));
    }
    members.push(("pub", JwkMember::Text(base64_url_text(&public))));
    members
}

/// `generateKey` de ECDSA, ECDH, Ed25519 e X25519: o `CryptoKeyPair` (`privateKey` e `publicKey`, nessa ordem).
fn generate_asym_key(global_object: &JSGlobalObject, call: &HostCall, id: AlgorithmId, usages: u16) -> HostResult {
    let curve = asym_curve(global_object, call, id, call.argument(0), false)?;
    // O par aceita os usos das duas metades (`sign` e `verify`); a privada fica com os dela e a pública com os dela
    // (medido no bun 1.4.2). Uso fora das duas metades, `SyntaxError`; privada sem nenhum uso, `Usages cannot be empty`.
    let private_usages = usages & asym_usages(id, true);
    let public_usages = usages & asym_usages(id, false);
    if usages & !(asym_usages(id, true) | asym_usages(id, false)) != 0 {
        return Err(throw_native_syntax_error(global_object, "A required parameter was missing or out-of-range"));
    }
    if private_usages == 0 {
        return Err(throw_native_syntax_error(global_object, "Usages cannot be empty when creating a key."));
    }
    let secret = curve.generate();
    let public = curve.public_of(&secret).expect("chave recém-gerada é válida");
    let extractable = call.argument(1).to_boolean();
    let private_key = create_key(global_object, asym_key(Asym { id, curve, private: true }, secret, extractable, private_usages));
    let public_key = create_key(global_object, asym_key(Asym { id, curve, private: false }, public, true, public_usages));
    let vm = global_object.vm();
    let pair = construct_empty_object(global_object);
    pair.put_direct(vm, &property_key(vm, "privateKey"), private_key, 0);
    pair.put_direct(vm, &property_key(vm, "publicKey"), public_key, 0);
    Ok(pair.as_value())
}

/// O ponto público (e o escalar privado, se houver `d`) de um JWK de curva elíptica, na ordem de validação do bun.
fn asym_jwk_material(global_object: &JSGlobalObject, call: &HostCall, jwk: &Jwk, asym: Asym, extractable: bool, usages: u16) -> Result<(Vec<u8>, bool), Thrown> {
    let invalid = || dom_error(global_object, call, "DataError", "Invalid keyData");
    let nist = asym.curve.is_nist();
    if jwk.kty.as_deref() != Some(if nist { "EC" } else { "OKP" }) {
        return Err(invalid());
    }
    // Medido no bun 1.4.2: o JWK de curva NIST sem `crv` é `Invalid keyData`.
    if nist && jwk.crv.is_none() {
        return Err(invalid());
    }
    if jwk.crv.as_deref().is_some_and(|crv| crv != asym.curve.name()) {
        let message =
            if nist { "JWK \"crv\" does not match the requested algorithm" } else { "JWK \"crv\" Parameter and algorithm name mismatch" };
        return Err(dom_error(global_object, call, "DataError", message));
    }
    let signs = matches!(asym.id, AlgorithmId::Ecdsa | AlgorithmId::Ed25519);
    if signs && usages != 0 && jwk.use_.as_deref().is_some_and(|use_| use_ != "sig") {
        return Err(dom_error(global_object, call, "DataError", "Invalid JWK \"use\" Parameter"));
    }
    let expected_alg = match asym.curve {
        Curve::P256 => Some("ES256"),
        Curve::P384 => Some("ES384"),
        Curve::P521 => Some("ES512"),
        Curve::Ed25519 => Some("Ed25519"),
        Curve::X25519 => None,
    };
    if !matches!(asym.id, AlgorithmId::Ecdh | AlgorithmId::X25519) && jwk.alg.as_deref().is_some_and(|alg| Some(alg) != expected_alg && !(asym.id == AlgorithmId::Ed25519 && alg == "EdDSA")) {
        return Err(dom_error(global_object, call, "DataError", "JWK \"alg\" does not match the requested algorithm"));
    }
    // Medido no bun: `key_ops` duplicado passa; só a incompatibilidade com os usos vale.
    let key_ops_bad = jwk.key_ops.as_deref().is_some_and(|operations| usage_mask_of(operations) & usages != usages);
    if key_ops_bad || (jwk.ext == Some(false) && extractable) {
        return Err(invalid());
    }
    let size = asym.curve.field_size();
    // Medido no bun: numa chave OKP privada o `x` é ignorado (mesmo errado, curto ou vazio) e só o `d` de 32 bytes vale.
    if let (false, Some(d)) = (nist, jwk.d.as_deref()) {
        let secret = base64_url_bytes(d);
        return if secret.len() == size { Ok((secret, true)) } else { Err(invalid()) };
    }
    let x = base64_url_bytes(jwk.x.as_deref().ok_or_else(invalid)?);
    let public = if nist {
        let y = base64_url_bytes(jwk.y.as_deref().ok_or_else(invalid)?);
        let mut point = vec![4u8];
        point.extend_from_slice(&x);
        point.extend_from_slice(&y);
        point
    } else {
        x
    };
    let public = asym.curve.normalize_public(&public).filter(|_| !nist || public.len() == 1 + 2 * size).ok_or_else(invalid)?;
    let Some(d) = jwk.d.as_deref() else { return Ok((public, false)) };
    let mut secret = base64_url_bytes(d);
    if secret.len() > size {
        return Err(invalid());
    }
    secret.splice(0..0, vec![0u8; size - secret.len()]);
    if asym.curve.public_of(&secret).as_deref() != Some(public.as_slice()) {
        return Err(invalid());
    }
    Ok((secret, true))
}

/// `importKey` de ECDSA, ECDH, Ed25519 e X25519 (`raw`, `raw-public`, `spki`, `pkcs8`, `jwk`).
fn import_asym_key(
    global_object: &JSGlobalObject,
    call: &HostCall,
    id: AlgorithmId,
    algorithm: JSValue,
    format: &str,
    bytes: Option<Vec<u8>>,
    jwk: Option<Jwk>,
    extractable: bool,
    usages: u16,
) -> Result<KeyState, Thrown> {
    let curve = asym_curve(global_object, call, id, algorithm, true)?;
    // `toKeyData`: `jwk` ausente (`null`/`undefined`) é `DataError`; outro valor que não é objeto, `TypeError`.
    if format == "jwk" && jwk.is_none() {
        return Err(missing_jwk_error(global_object, call, false));
    }
    let data_error = |message: &str| dom_error(global_object, call, "DataError", message);
    // O `raw-seed` nunca é de curva elíptica, e o bun recusa antes de olhar usos e dados.
    if format == "raw-seed" {
        let message = format!("Unable to import {} using raw-seed format", asym_name(id));
        return Err(dom_error(global_object, call, "NotSupportedError", &message));
    }
    // Os usos são conferidos antes dos dados, pelo tipo que o formato produz (`pkcs8` e JWK com `d` são privados).
    let private_format = format == "pkcs8" || jwk.as_ref().is_some_and(|jwk| jwk.d.is_some());
    check_asym_usages(global_object, id, private_format, usages)?;
    let bytes = bytes.unwrap_or_default();
    let (secret, private) = match (format, jwk) {
        ("raw" | "raw-public" | "raw-secret", _) => (curve.normalize_public(&bytes).filter(|_| !curve.is_nist() || bytes.len() == 1 + 2 * curve.field_size()).ok_or_else(|| data_error("Invalid keyData"))?, false),
        ("spki", _) => (curve.parse_spki(&bytes).map_err(data_error)?, false),
        ("pkcs8", _) => (curve.parse_pkcs8(&bytes).map_err(data_error)?, true),
        ("jwk", Some(jwk)) => asym_jwk_material(global_object, call, &jwk, Asym { id, curve, private: false }, extractable, usages)?,
        _ => return Err(dom_error(global_object, call, "NotSupportedError", NOT_SUPPORTED)),
    };
    check_asym_usages(global_object, id, private, usages)?;
    if private && usages == 0 {
        return Err(throw_native_syntax_error(global_object, "Usages cannot be empty when importing a private key."));
    }
    Ok(asym_key(Asym { id, curve, private }, secret, extractable, usages))
}

/// `Unsupported key usage` quando `usages` tem algo que o tipo (privado ou público) do algoritmo não aceita.
fn check_asym_usages(global_object: &JSGlobalObject, id: AlgorithmId, private: bool, usages: u16) -> Result<(), Thrown> {
    if usages & !asym_usages(id, private) != 0 {
        let message = format!("Unsupported key usage for an {} key", asym_name(id));
        return Err(throw_native_syntax_error(global_object, &message));
    }
    Ok(())
}

/// `crypto.subtle.getPublicKey`: a chave pública (extraível) de uma privada, com os `usages` pedidos; a ordem do bun é
/// chave, sequência de usos, tipo da chave, usos aceitos pela pública.
fn get_public_key_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let key = key_at(global_object, call, "getPublicKey", 0, 1, "key")?;
    let usages = check_usages(global_object, call.argument(1))?;
    if let Some(pq) = key.pq {
        if !pq.private {
            return Err(dom_error(global_object, call, "InvalidAccessError", "key must be a private key"));
        }
        check_pq_usages(global_object, pq.alg, false, usages)?;
        let public = pq.alg.public_from_seed(&key.secret).expect("semente guardada tem o tamanho do parâmetro");
        return Ok(create_key(global_object, pq_key(Pq { private: false, ..pq }, public, true, usages)));
    }
    let Some(asym) = key.asym else {
        return Err(dom_error(global_object, call, "NotSupportedError", "key must be a private key"));
    };
    if !asym.private {
        return Err(dom_error(global_object, call, "InvalidAccessError", "key must be a private key"));
    }
    check_asym_usages(global_object, asym.id, false, usages)?;
    let public = asym.curve.public_of(&key.secret).expect("chave privada guardada é válida");
    Ok(create_key(global_object, asym_key(Asym { private: false, ..asym }, public, true, usages)))
}

/// As verificações de `exportKey` de uma chave de curva elíptica: extraível, formato que o tipo exporta.
fn check_asym_exportable(global_object: &JSGlobalObject, call: &HostCall, key: &KeyState, format: &str) -> Result<(), Thrown> {
    let asym = key.asym.expect("chave de curva elíptica");
    // Medido no bun 1.4.2: `raw-public` de chave privada é recusado antes da conferência de `extractable`.
    if format == "raw-public" && asym.private {
        let message = format!("Unable to export {} private key using raw-public format", asym_name(asym.id));
        return Err(dom_error(global_object, call, "NotSupportedError", &message));
    }
    if !key.extractable {
        return Err(dom_error(global_object, call, "InvalidAccessError", "key is not extractable"));
    }
    let valid = match format {
        "jwk" => true,
        "raw" | "raw-public" | "spki" => !asym.private,
        "pkcs8" => asym.private,
        "raw-secret" => return Err(dom_error(global_object, call, "NotSupportedError", NOT_SUPPORTED)),
        _ => {
            let message = format!("Unable to export {} {} key using {format} format", asym_name(asym.id), key_type_name(key));
            return Err(dom_error(global_object, call, "NotSupportedError", &message));
        }
    };
    if valid {
        return Ok(());
    }
    Err(dom_error(global_object, call, "InvalidAccessError", "The requested operation is not valid for the provided key"))
}

/// Os bytes de `exportKey` (`raw`, `spki`, `pkcs8`) de uma chave de qualquer tipo; o JWK tem caminho próprio.
fn export_key_bytes(key: &KeyState, format: &str) -> Vec<u8> {
    if key.rsa.is_some() {
        return if format == "spki" { crypto_rsa::public_spki(&key.secret).unwrap_or_default() } else { key.secret.clone() };
    }
    if let Some(pq) = key.pq {
        return match format {
            "spki" => pq.alg.encode_spki(&key.secret),
            "pkcs8" => pq.alg.encode_pkcs8(&key.secret),
            _ => key.secret.clone(),
        };
    }
    let Some(asym) = key.asym else { return key.secret.clone() };
    match format {
        "spki" => asym.curve.public_spki(&key.secret).unwrap_or_default(),
        "pkcs8" => asym.curve.secret_pkcs8(&key.secret).unwrap_or_default(),
        _ => key.secret.clone(),
    }
}

/// O `hash` obrigatório do `EcdsaParams` (texto ou dicionário com `name`): o índice em [`HASHES`].
fn ecdsa_hash(global_object: &JSGlobalObject, call: &HostCall, algorithm: JSValue) -> Result<usize, Thrown> {
    let hash = required_member(global_object, algorithm, "EcdsaParams", "hash", "(object or DOMString)")?;
    hash_index(&algorithm_name(global_object, hash)?).ok_or_else(|| not_supported(global_object, call))
}

/// O erro de `importKey('jwk', ...)` sem objeto JWK: `null` e `undefined` são `DataError` (`Invalid keyData`, e a mensagem
/// genérica no AES), qualquer outro valor é o `TypeError` do WebIDL (medido no bun 1.4.2).
fn missing_jwk_error(global_object: &JSGlobalObject, call: &HostCall, generic: bool) -> Thrown {
    if call.argument(1).is_undefined_or_null() {
        return dom_error(global_object, call, "DataError", if generic { DATA_ERROR } else { "Invalid keyData" });
    }
    throw_native_type_error(global_object, "Type error")
}

/// A chave que é o `member` (`publicKey` do ECDH/X25519) do dicionário `algorithm`.
fn public_key_member(global_object: &JSGlobalObject, algorithm: JSValue, dictionary: &str) -> Result<KeyState, Thrown> {
    // O dicionário do WebCrypto chama o membro `public`, mas as mensagens do bun o citam como `publicKey` (medido no bun
    // 1.4.2), então a leitura e o texto do erro usam nomes diferentes.
    let member = dictionary_member(global_object, algorithm, "public");
    if member.is_undefined() {
        let message = format!("Member {dictionary}.publicKey is required and must be an instance of CryptoKey");
        return Err(throw_coded_type_error(global_object, &message, "ERR_MISSING_OPTION"));
    }
    KEYS.with(|keys| keys.borrow().get(&member.encode()).cloned()).ok_or_else(|| {
        let message = format!("Member {dictionary}.publicKey must be an instance of CryptoKey");
        throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE")
    })
}

/// Os parâmetros de derivação já convertidos: o dicionário de HKDF/PBKDF2 ou a chave pública de ECDH/X25519.
enum DeriveParams {
    Kdf(crypto_kdf::KdfParams),
    Asym { id: AlgorithmId, public: KeyState },
}

/// Normaliza o algoritmo do primeiro argumento de `deriveBits`/`deriveKey` e converte o dicionário.
fn derive_params(global_object: &JSGlobalObject, call: &HostCall) -> Result<DeriveParams, Thrown> {
    let id = normalize_algorithm(global_object, call, call.argument(0), Operation::Derive)?;
    if crypto_kdf::is_kdf_id(id) {
        return crypto_kdf::kdf_params(global_object, call, id).map(DeriveParams::Kdf);
    }
    let dictionary = if id == AlgorithmId::Ecdh { "EcdhKeyDeriveParams" } else { "X25519Params" };
    Ok(DeriveParams::Asym { id, public: public_key_member(global_object, call.argument(0), dictionary)? })
}

/// O segredo derivado de `base` na ordem de validação do bun (uso, algoritmo, tipo, curva); `length` entrega o tamanho pedido
/// em bits (`None` é o inteiro) e só é avaliado depois desses testes.
fn derive_asym_bits(
    global_object: &JSGlobalObject,
    call: &HostCall,
    params: DeriveParams,
    base: &KeyState,
    length: impl FnOnce() -> Result<Option<u32>, Thrown>,
    usage: (u16, &str),
) -> Result<Vec<u8>, Thrown> {
    let (id, public) = match params {
        DeriveParams::Kdf(params) => return crypto_kdf::derive_kdf_bits(global_object, call, params, base, length, usage),
        DeriveParams::Asym { id, public } => (id, public),
    };
    if base.usages & usage.0 == 0 {
        return Err(dom_error(global_object, call, "InvalidAccessError", &format!("baseKey does not have {} usage", usage.1)));
    }
    let Some(base_asym) = base.asym.filter(|asym| asym.id == id && asym.private) else {
        let mismatch = base.asym.is_none_or(|asym| asym.id != id);
        let message = if mismatch { "Key algorithm mismatch" } else { "The requested operation is not valid for the provided key" };
        return Err(dom_error(global_object, call, "InvalidAccessError", message));
    };
    let Some(public_asym) = public.asym.filter(|asym| !asym.private) else {
        return Err(dom_error(global_object, call, "InvalidAccessError", "The requested operation is not valid for the provided key"));
    };
    if public_asym.id != id {
        return Err(dom_error(global_object, call, "InvalidAccessError", "key algorithm mismatch"));
    }
    if public_asym.curve != base_asym.curve {
        return Err(dom_error(global_object, call, "InvalidAccessError", "Named curve mismatch"));
    }
    let length = length()?;
    let size = base_asym.curve.field_size();
    let bytes = length.map_or(size, |bits| (bits as usize).div_ceil(8));
    let Some(mut shared) = base_asym.curve.agree(&base.secret, &public.secret).filter(|_| bytes <= size) else {
        return Err(operation_error(global_object, call, "derived bit length is too small"));
    };
    shared.truncate(bytes);
    if let Some(bits) = length.filter(|bits| bits % 8 != 0 && bytes > 0) {
        shared[bytes - 1] &= 0xffu8 << (8 - bits % 8);
    }
    Ok(shared)
}

/// O `length` de `deriveBits`: ausente, `undefined` ou `null` é o segredo inteiro; senão `unsigned long`.
fn derive_length(value: JSValue) -> Option<u32> {
    if value.is_undefined() || value.is_null() {
        return None;
    }
    let number = value.to_number();
    Some(if number.is_finite() { (number.trunc() as i64 & 0xffff_ffff) as u32 } else { 0 })
}

/// `crypto.subtle.deriveBits` (ECDH, X25519, HKDF e PBKDF2).
fn derive_bits_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let base = key_at(global_object, call, "deriveBits", 1, 2, "baseKey")?;
    let length = derive_length(call.argument(2));
    let params = derive_params(global_object, call)?;
    let shared = derive_asym_bits(global_object, call, params, &base, || Ok(length), (USAGE_DERIVE_BITS, "deriveBits"))?;
    Ok(array_buffer_value(global_object, &shared))
}

/// `crypto.subtle.deriveKey`: deriva os bits do tamanho que o `derivedKeyType` pede e importa como `raw`. Ordem medida no bun:
/// `baseKey`, `keyUsages`, parâmetros do algoritmo, `derivedKeyType` (nome e membros), uso e algoritmo da chave base, o tamanho
/// pedido pelo `derivedKeyType`, a derivação e, por fim, a importação (extraível e usos).
fn derive_key_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let base = key_at(global_object, call, "deriveKey", 1, 2, "baseKey")?;
    let usages = check_usages(global_object, call.argument(4))?;
    let params = derive_params(global_object, call)?;
    let derived = call.argument(2);
    let id = normalize_algorithm(global_object, call, derived, Operation::ImportKey)?;
    let hmac = match id {
        AlgorithmId::AesCbc | AlgorithmId::AesCtr | AlgorithmId::AesGcm | AlgorithmId::AesKw | AlgorithmId::AesCfb => {
            let length = aes_key_length(global_object, derived)?;
            let target_bits = move || {
                if !matches!(length, 128 | 192 | 256) {
                    return Err(operation_error(global_object, call, "Cannot get key length from derivedKeyType"));
                }
                Ok(Some(length))
            };
            return finish_derive_key(global_object, call, params, &base, target_bits, usages);
        }
        // O ChaCha20-Poly1305 não tem `length`: o bun deriva sempre 256 bits (medido com PBKDF2, HKDF, ECDH e X25519).
        AlgorithmId::ChaCha20Poly1305 => return finish_derive_key(global_object, call, params, &base, || Ok(Some(256)), usages),
        AlgorithmId::Hmac => {
            let parsed = hmac_params(global_object, call, derived)?;
            // `length` é `[EnforceRange] unsigned long`: o `-1` do `derivedKeyType` é `TypeError` antes dos testes da chave base.
            let length = dictionary_member(global_object, derived, "length");
            if !length.is_undefined() {
                enforce_unsigned_long(global_object, length)?;
            }
            parsed
        }
        // Um `derivedKeyType` HKDF/PBKDF2 não tem tamanho: o bun deriva o segredo inteiro (como `deriveBits` com `length` nulo).
        AlgorithmId::Hkdf | AlgorithmId::Pbkdf2 => return finish_derive_key(global_object, call, params, &base, || Ok(None), usages),
        _ => return Err(not_supported(global_object, call)),
    };
    let target_bits = move || {
        if hmac.1 == Some(0) {
            return Err(throw_native_type_error(global_object, "Cannot get key length from derivedKeyType"));
        }
        Ok(Some(hmac.1.unwrap_or(HASHES[hmac.0].key_bits as u32)))
    };
    finish_derive_key(global_object, call, params, &base, target_bits, usages)
}

/// O fim de `deriveKey`: deriva (o tamanho do alvo só é conferido depois dos testes da chave base) e importa como `raw`.
fn finish_derive_key(
    global_object: &JSGlobalObject,
    call: &HostCall,
    params: DeriveParams,
    base: &KeyState,
    target_bits: impl FnOnce() -> Result<Option<u32>, Thrown>,
    usages: u16,
) -> HostResult {
    let shared = derive_asym_bits(global_object, call, params, base, target_bits, (USAGE_DERIVE_KEY, "deriveKey"))?;
    let key = import_secret(global_object, call, "raw-secret", Some(shared), None, call.argument(2), call.argument(3).to_boolean(), usages)?;
    Ok(create_key(global_object, key))
}

/// O nome do algoritmo da chave nas mensagens de erro: `HMAC`, o AES, o RSA ou o de curva elíptica.
fn key_algorithm_name(key: &KeyState) -> &'static str {
    key.rsa
        .map(|rsa| rsa_name(rsa.id))
        .or_else(|| key.asym.map(|asym| asym_name(asym.id)))
        .or_else(|| key.pq.map(|pq| pq.alg.name()))
        .or_else(|| key.aes.map(aes_name))
        .unwrap_or("HMAC")
}

/// O `alg` JWK de uma chave simétrica de `bytes` bytes: `A128CBC` e afins no AES, `A128CFB8` no AES-CFB-8 (sem o segundo
/// hífen) e `C20P` no ChaCha20-Poly1305.
fn jwk_aes_alg(id: AlgorithmId, bytes: usize) -> String {
    match id {
        AlgorithmId::ChaCha20Poly1305 => "C20P".to_owned(),
        AlgorithmId::AesCfb => format!("A{}CFB8", bytes * 8),
        _ => format!("A{}{}", bytes * 8, &aes_name(id)[4..]),
    }
}

/// O `alg` do JWK: `A128CBC` e afins nas chaves AES; o `HS256` e afins do hash no HMAC (SHA3 não tem).
fn jwk_alg(key: &KeyState) -> Option<String> {
    match key.aes {
        Some(id) => Some(jwk_aes_alg(id, key.secret.len())),
        None => HASHES[key.hash].jwk_alg.map(str::to_owned),
    }
}

/// O JWK da chave como texto JSON, com os membros em ordem alfabética (o que `JSON.stringify` dá ao `exportKey('jwk')`).
fn jwk_json(key: &KeyState) -> String {
    if key.asym.is_some() || key.rsa.is_some() || key.pq.is_some() {
        return asym_jwk_json(key);
    }
    let mut members = Vec::new();
    if let Some(alg) = jwk_alg(key) {
        members.push(format!("\"alg\":\"{alg}\""));
    }
    members.push(format!("\"ext\":{}", key.extractable));
    let encoded = base64_encode(&key.secret, BASE64_URL, false);
    members.push(format!("\"k\":\"{}\"", String::from_utf8_lossy(&encoded)));
    let operations: Vec<String> = usage_names(key.usages).into_iter().map(|name| format!("\"{name}\"")).collect();
    members.push(format!("\"key_ops\":[{}]", operations.join(",")));
    members.push("\"kty\":\"oct\"".to_owned());
    format!("{{{}}}", members.join(","))
}

/// `exportKey('jwk')` de uma chave secreta (`CryptoKeyHMAC::exportJwk`/`CryptoKeyAES::exportJwk`), nas chaves em ordem alfabética.
fn jwk_value(global_object: &JSGlobalObject, key: &KeyState) -> JSValue {
    if key.asym.is_some() || key.rsa.is_some() || key.pq.is_some() {
        return asym_jwk_value(global_object, key);
    }
    let vm = global_object.vm();
    let object = construct_empty_object(global_object);
    if let Some(alg) = jwk_alg(key) {
        object.put_direct(vm, &property_key(vm, "alg"), ascii_value(global_object, &alg), 0);
    }
    object.put_direct(vm, &property_key(vm, "ext"), JSValue::Bool(key.extractable), 0);
    let encoded = base64_encode(&key.secret, BASE64_URL, false);
    object.put_direct(vm, &property_key(vm, "k"), ascii_value(global_object, &String::from_utf8_lossy(&encoded)), 0);
    object.put_direct(vm, &property_key(vm, "key_ops"), usages_array(global_object, key.usages), 0);
    object.put_direct(vm, &property_key(vm, "kty"), ascii_value(global_object, "oct"), 0);
    object.as_value()
}

fn usages_array(global_object: &JSGlobalObject, mask: u16) -> JSValue {
    let names: Vec<JSValue> = usage_names(mask).into_iter().map(|name| ascii_value(global_object, name)).collect();
    construct_array(global_object.vm(), &global_object.array_structure(), &names).as_value()
}

fn algorithm_value(global_object: &JSGlobalObject, key: &KeyState) -> JSValue {
    let vm = global_object.vm();
    if let Some(rsa) = key.rsa {
        return rsa_algorithm_value(global_object, key, rsa);
    }
    if let Some(asym) = key.asym {
        let algorithm = construct_empty_object(global_object);
        algorithm.put_direct(vm, &property_key(vm, "name"), ascii_value(global_object, asym_name(asym.id)), 0);
        if asym.curve.is_nist() {
            algorithm.put_direct(vm, &property_key(vm, "namedCurve"), ascii_value(global_object, asym.curve.name()), 0);
        }
        return algorithm.as_value();
    }
    if let Some(pq) = key.pq {
        let algorithm = construct_empty_object(global_object);
        algorithm.put_direct(vm, &property_key(vm, "name"), ascii_value(global_object, pq.alg.name()), 0);
        return algorithm.as_value();
    }
    if let Some(id) = key.aes {
        let algorithm = construct_empty_object(global_object);
        algorithm.put_direct(vm, &property_key(vm, "name"), ascii_value(global_object, aes_name(id)), 0);
        if crypto_kdf::is_kdf_id(id) || id == AlgorithmId::ChaCha20Poly1305 {
            return algorithm.as_value();
        }
        algorithm.put_direct(vm, &property_key(vm, "length"), js_number((key.secret.len() * 8) as f64), 0);
        return algorithm.as_value();
    }
    let hash = construct_empty_object(global_object);
    hash.put_direct(vm, &property_key(vm, "name"), ascii_value(global_object, HASHES[key.hash].name), 0);
    let algorithm = construct_empty_object(global_object);
    algorithm.put_direct(vm, &property_key(vm, "name"), ascii_value(global_object, "HMAC"), 0);
    algorithm.put_direct(vm, &property_key(vm, "hash"), hash.as_value(), 0);
    algorithm.put_direct(vm, &property_key(vm, "length"), js_number((key.secret.len() * 8) as f64), 0);
    algorithm.as_value()
}

/// Converte `value` como o WebIDL com `[EnforceRange]` do bun: NaN ou negativo é `TypeError` `Value N is outside the range
/// [0, max]` (sem `code`); acima do máximo é `Type error`; o resto trunca.
fn enforce_range(global_object: &JSGlobalObject, value: JSValue, max: u32) -> Result<u32, Thrown> {
    let number = value.to_number();
    let truncated = number.trunc();
    // Fora de `unsigned long` (infinito, NaN, negativo, acima de 2^32 - 1) a mensagem cita o valor e o intervalo pedido; entre o
    // `max` e 2^32 - 1 é só `Type error` (medido no bun 1.4.2: `256` dá `Type error`, `Infinity` e `1e10` dão a mensagem).
    if number.is_nan() || truncated < 0.0 || truncated > f64::from(u32::MAX) {
        let text = if number.is_nan() {
            "NaN".to_owned()
        } else if number.is_infinite() {
            (if number < 0.0 { "-Infinity" } else { "Infinity" }).to_owned()
        } else if number.fract() == 0.0 {
            format!("{}", number as i64)
        } else {
            format!("{number}")
        };
        return Err(throw_native_type_error(global_object, &format!("Value {text} is outside the range [0, {max}]")));
    }
    if truncated > f64::from(max) {
        return Err(throw_native_type_error(global_object, "Type error"));
    }
    Ok(truncated as u32)
}

/// `unsigned long` com `[EnforceRange]`: NaN, negativo ou acima de 2^32 - 1 é `TypeError` `Value N is outside the range`.
fn enforce_unsigned_long(global_object: &JSGlobalObject, value: JSValue) -> Result<u32, Thrown> {
    let number = value.to_number().trunc();
    if number > f64::from(u32::MAX) {
        return Err(throw_native_type_error(global_object, &format!("Value {number} is outside the range [0, {}]", u32::MAX)));
    }
    enforce_range(global_object, value, u32::MAX)
}

/// O membro `name` do dicionário `value` (`undefined` se `value` não é objeto).
fn dictionary_member(global_object: &JSGlobalObject, value: JSValue, name: &str) -> JSValue {
    let vm = global_object.vm();
    JSObject::from_value(&value).map_or_else(JSValue::undefined, |object| object.get(vm, &property_key(vm, name)))
}

/// Um membro obrigatório: ausente é `ERR_MISSING_OPTION` com o tipo que o WebIDL do bun cita.
fn required_member(global_object: &JSGlobalObject, value: JSValue, dictionary: &str, name: &str, kind: &str) -> Result<JSValue, Thrown> {
    let member = dictionary_member(global_object, value, name);
    if member.is_undefined() {
        let message = format!("Member {dictionary}.{name} is required and must be an instance of {kind}");
        return Err(throw_coded_type_error(global_object, &message, "ERR_MISSING_OPTION"));
    }
    Ok(member)
}

const BUFFER_KIND: &str = "(ArrayBufferView or ArrayBuffer)";
const DATA_ERROR: &str = "Data provided to an operation does not meet requirements";

/// Os parâmetros de uma operação AES já convertidos (`AesCbcCfbParams`, `AesCtrParams`, `AesGcmParams`); o `AES-KW` não tem.
enum AesParams {
    Cbc { iv: Vec<u8> },
    Cfb { iv: Vec<u8> },
    Ctr { counter: Vec<u8>, length: u32 },
    Gcm { iv: Vec<u8>, additional_data: Vec<u8>, tag_bits: u32 },
    ChaCha { iv: Vec<u8>, additional_data: Vec<u8>, tag_bits: u32 },
    Kw,
    Oaep { label: Vec<u8> },
}

/// Os membros de `AesGcmParams`/`AeadParams` (`additionalData`, `iv`, `tagLength`, nessa ordem): IV, dados adicionais e etiqueta.
fn aead_members(global_object: &JSGlobalObject, value: JSValue, dictionary: &str) -> Result<(Vec<u8>, Vec<u8>, u32), Thrown> {
    let additional = dictionary_member(global_object, value, "additionalData");
    let additional_data = if additional.is_undefined() { Vec::new() } else { buffer_source(global_object, additional)? };
    let iv = required_member(global_object, value, dictionary, "iv", BUFFER_KIND)?;
    let iv = buffer_source(global_object, iv)?;
    let tag = dictionary_member(global_object, value, "tagLength");
    let tag_bits = if tag.is_undefined() { 128 } else { enforce_range(global_object, tag, 255)? };
    Ok((iv, additional_data, tag_bits))
}

/// Converte o dicionário de parâmetros do algoritmo AES `id`, na ordem alfabética do WebIDL.
fn aes_params(global_object: &JSGlobalObject, id: AlgorithmId, value: JSValue) -> Result<AesParams, Thrown> {
    match id {
        AlgorithmId::AesCbc | AlgorithmId::AesCfb => {
            let iv = required_member(global_object, value, "AesCbcCfbParams", "iv", BUFFER_KIND)?;
            let iv = buffer_source(global_object, iv)?;
            Ok(if id == AlgorithmId::AesCfb { AesParams::Cfb { iv } } else { AesParams::Cbc { iv } })
        }
        AlgorithmId::AesCtr => {
            let counter = required_member(global_object, value, "AesCtrParams", "counter", BUFFER_KIND)?;
            let counter = buffer_source(global_object, counter)?;
            let length = required_member(global_object, value, "AesCtrParams", "length", "octet")?;
            Ok(AesParams::Ctr { counter, length: enforce_range(global_object, length, 255)? })
        }
        AlgorithmId::AesGcm => {
            let (iv, additional_data, tag_bits) = aead_members(global_object, value, "AesGcmParams")?;
            Ok(AesParams::Gcm { iv, additional_data, tag_bits })
        }
        AlgorithmId::ChaCha20Poly1305 => {
            let (iv, additional_data, tag_bits) = aead_members(global_object, value, "AeadParams")?;
            Ok(AesParams::ChaCha { iv, additional_data, tag_bits })
        }
        AlgorithmId::RsaOaep => {
            let label = dictionary_member(global_object, value, "label");
            let label = if label.is_undefined() { Vec::new() } else { buffer_source(global_object, label)? };
            Ok(AesParams::Oaep { label })
        }
        _ => Ok(AesParams::Kw),
    }
}

/// `AesKeyParams.length`: obrigatório, `unsigned short` com `[EnforceRange]`.
fn aes_key_length(global_object: &JSGlobalObject, value: JSValue) -> Result<u32, Thrown> {
    let length = required_member(global_object, value, "AesKeyParams", "length", "unsigned short")?;
    enforce_range(global_object, length, 65535)
}

/// `crypto.subtle.generateKey` de AES (`CryptoAlgorithmAES_*::generateKey`): usos proibidos, tamanho e uso vazio, nessa ordem.
fn generate_aes_key(global_object: &JSGlobalObject, call: &HostCall, id: AlgorithmId, usages: u16) -> HostResult {
    // O ChaCha20-Poly1305 não tem `length`: a chave é sempre de 256 bits.
    let length = if id == AlgorithmId::ChaCha20Poly1305 { 256 } else { aes_key_length(global_object, call.argument(0))? };
    if usages & !aes_usages(id) != 0 {
        return Err(unsupported_usage(global_object, id));
    }
    if !matches!(length, 128 | 192 | 256) {
        return Err(dom_error(global_object, call, "OperationError", OPERATION_FAILED));
    }
    let mut secret = vec![0u8; length as usize / 8];
    host().random_fill(&mut secret);
    if usages == 0 {
        return Err(throw_native_syntax_error(global_object, "Usages cannot be empty when creating a key."));
    }
    Ok(create_key(global_object, aes_secret_key(id, secret, call.argument(1).to_boolean(), usages)))
}

/// Os identificadores RSA do registro que têm chave: o `RSAES-PKCS1-v1_5` é recusado antes de chegar aqui.
fn is_rsa_id(id: AlgorithmId) -> bool {
    matches!(id, AlgorithmId::RsassaPkcs1V15 | AlgorithmId::RsaPss | AlgorithmId::RsaOaep)
}

/// O nome canônico de um algoritmo RSA.
fn rsa_name(id: AlgorithmId) -> &'static str {
    match id {
        AlgorithmId::RsassaPkcs1V15 => "RSASSA-PKCS1-v1_5",
        AlgorithmId::RsaPss => "RSA-PSS",
        _ => "RSA-OAEP",
    }
}

/// Os usos que uma chave RSA aceita: assinar/verificar (RSASSA, PSS) ou decifrar/desembrulhar e cifrar/embrulhar (OAEP).
fn rsa_usages(id: AlgorithmId, private: bool) -> u16 {
    match (id, private) {
        (AlgorithmId::RsaOaep, true) => USAGE_DECRYPT | USAGE_UNWRAP,
        (AlgorithmId::RsaOaep, false) => USAGE_ENCRYPT | USAGE_WRAP,
        (_, true) => USAGE_SIGN,
        (_, false) => USAGE_VERIFY,
    }
}

/// O `alg` do JWK RSA: `RS256`, `PS256`, `RSA-OAEP-256` e afins (`RS1`, `PS1` e `RSA-OAEP` no SHA-1); os SHA3 não têm.
fn rsa_jwk_alg(id: AlgorithmId, hash: usize) -> Option<String> {
    let bits = &HASHES[hash].jwk_alg?[2..];
    Some(match id {
        AlgorithmId::RsassaPkcs1V15 => format!("RS{bits}"),
        AlgorithmId::RsaPss => format!("PS{bits}"),
        _ if bits == "1" => "RSA-OAEP".to_owned(),
        _ => format!("RSA-OAEP-{bits}"),
    })
}

/// O `algorithm` de uma chave RSA: `name`, `modulusLength`, `publicExponent` (`Uint8Array`) e `hash`, nessa ordem.
fn rsa_algorithm_value(global_object: &JSGlobalObject, key: &KeyState, rsa: Rsa) -> JSValue {
    let vm = global_object.vm();
    let (bits, exponent) = crypto_rsa::parameters(&key.secret).unwrap_or_default();
    let exponent_value = create_uint8_array(global_object, exponent.len()).map_or_else(
        |_| JSValue::undefined(),
        |array| {
            array.with_vector_mut(|destination| destination[..exponent.len()].copy_from_slice(&exponent));
            array.as_value()
        },
    );
    let hash = construct_empty_object(global_object);
    hash.put_direct(vm, &property_key(vm, "name"), ascii_value(global_object, HASHES[key.hash].name), 0);
    let algorithm = construct_empty_object(global_object);
    algorithm.put_direct(vm, &property_key(vm, "name"), ascii_value(global_object, rsa_name(rsa.id)), 0);
    // O bun informa o tamanho em bytes inteiros vezes oito.
    algorithm.put_direct(vm, &property_key(vm, "modulusLength"), js_number((bits / 8 * 8) as f64), 0);
    algorithm.put_direct(vm, &property_key(vm, "publicExponent"), exponent_value, 0);
    algorithm.put_direct(vm, &property_key(vm, "hash"), hash.as_value(), 0);
    algorithm.as_value()
}

/// Os membros do JWK de uma chave RSA, na ordem alfabética do `exportKey('jwk')`.
fn rsa_jwk_members(key: &KeyState) -> Vec<(&'static str, JwkMember)> {
    let (Some(rsa), Some(parts)) = (key.rsa, crypto_rsa::components(&key.secret)) else { return Vec::new() };
    let text = |bytes: &[u8]| JwkMember::Text(base64_url_text(bytes));
    let mut members = Vec::new();
    if let Some(alg) = rsa_jwk_alg(rsa.id, key.hash) {
        members.push(("alg", JwkMember::Text(alg)));
    }
    if let Some(private) = &parts.private {
        if !private.d.is_empty() {
            members.push(("d", text(&private.d)));
        }
        members.push(("dp", text(&private.dp)));
        members.push(("dq", text(&private.dq)));
    }
    members.push(("e", text(&parts.e)));
    members.push(("ext", JwkMember::Bool(key.extractable)));
    members.push(("key_ops", JwkMember::Usages(key.usages)));
    members.push(("kty", JwkMember::Text("RSA".to_owned())));
    members.push(("n", text(&parts.n)));
    if let Some(private) = &parts.private {
        members.push(("p", text(&private.p)));
        members.push(("q", text(&private.q)));
        members.push(("qi", text(&private.qi)));
    }
    members
}

/// As verificações de `exportKey` de uma chave RSA: extraível, formato que o tipo exporta.
fn check_rsa_exportable(global_object: &JSGlobalObject, call: &HostCall, key: &KeyState, format: &str) -> Result<(), Thrown> {
    let private = key.rsa.is_some_and(|rsa| rsa.private);
    if !key.extractable {
        return Err(dom_error(global_object, call, "InvalidAccessError", "key is not extractable"));
    }
    match format {
        "jwk" => Ok(()),
        "spki" | "pkcs8" if (format == "pkcs8") == private => Ok(()),
        "spki" | "pkcs8" => Err(dom_error(global_object, call, "InvalidAccessError", "The requested operation is not valid for the provided key")),
        "raw" | "raw-secret" => Err(dom_error(global_object, call, "NotSupportedError", NOT_SUPPORTED)),
        _ => {
            let message = format!("Unable to export {} {} key using {format} format", key_algorithm_name(key), key_type_name(key));
            Err(dom_error(global_object, call, "NotSupportedError", &message))
        }
    }
}

/// O `hash` obrigatório do dicionário RSA (texto ou dicionário com `name`): o índice em [`HASHES`].
fn rsa_hash(global_object: &JSGlobalObject, call: &HostCall, algorithm: JSValue, dictionary: &str) -> Result<usize, Thrown> {
    let hash = required_member(global_object, algorithm, dictionary, "hash", "(object or DOMString)")?;
    hash_index(&algorithm_name(global_object, hash)?).ok_or_else(|| not_supported(global_object, call))
}

/// O `publicExponent` do `RsaHashedKeyGenParams`: só `Uint8Array` (inclui `Buffer`); qualquer outro tipo é `Type error`.
fn uint8_array_bytes(global_object: &JSGlobalObject, value: JSValue) -> Result<Vec<u8>, Thrown> {
    // Medido no bun 1.4.2: o `Uint8Array` sobre `SharedArrayBuffer` não serve (`TypeError`).
    let is_uint8 = JSGenericTypedArrayView::from_value(&value)
        .is_some_and(|view| matches!(view.typed_array_type(), TypedArrayType::Uint8) && !view.is_shared());
    input_bytes(value).filter(|_| is_uint8).ok_or_else(|| throw_native_type_error(global_object, "Type error"))
}

/// O expoente público de `bytes` (big-endian) quando serve ao bun: ímpar, de 3 até 32 bits.
fn rsa_public_exponent(bytes: &[u8]) -> Option<u32> {
    let trimmed = &bytes[bytes.iter().position(|&byte| byte != 0)?..];
    let value = trimmed.iter().try_fold(0u32, |acc, &byte| acc.checked_mul(256)?.checked_add(u32::from(byte)))?;
    (trimmed.len() <= 4 && value >= 3 && value % 2 == 1).then_some(value)
}

/// `generateKey` de RSASSA-PKCS1-v1_5, RSA-PSS e RSA-OAEP: o `CryptoKeyPair`. A ordem do bun é `modulusLength`,
/// `publicExponent`, `hash`, usos que o algoritmo não aceita, geração (`OperationError`) e, por fim, usos da privada vazios.
fn generate_rsa_key(global_object: &JSGlobalObject, call: &HostCall, id: AlgorithmId, usages: u16) -> HostResult {
    let algorithm = call.argument(0);
    let modulus = required_member(global_object, algorithm, "RsaHashedKeyGenParams", "modulusLength", "unsigned long")?;
    let modulus = enforce_range(global_object, modulus, u32::MAX)?;
    let exponent = required_member(global_object, algorithm, "RsaHashedKeyGenParams", "publicExponent", "Uint8Array")?;
    let exponent = uint8_array_bytes(global_object, exponent)?;
    let hash = rsa_hash(global_object, call, algorithm, "RsaHashedKeyGenParams")?;
    let (private_usages, public_usages) = (rsa_usages(id, true), rsa_usages(id, false));
    if usages & !(private_usages | public_usages) != 0 {
        return Err(throw_native_syntax_error(global_object, "Unsupported key usage for a RSA key"));
    }
    let der = Some(modulus)
        .filter(|bits| (512..=16384).contains(bits))
        .and(rsa_public_exponent(&exponent))
        .and_then(|_| crypto_rsa::generate(modulus as usize, &exponent))
        .ok_or_else(|| dom_error(global_object, call, "OperationError", OPERATION_FAILED))?;
    if usages & private_usages == 0 {
        return Err(throw_native_syntax_error(global_object, "Usages cannot be empty when creating a key."));
    }
    let spki = crypto_rsa::public_spki(&der).ok_or_else(|| dom_error(global_object, call, "OperationError", OPERATION_FAILED))?;
    let extractable = call.argument(1).to_boolean();
    let private_key = create_key(global_object, rsa_key(Rsa { id, private: true }, hash, der, extractable, usages & private_usages));
    let public_key = create_key(global_object, rsa_key(Rsa { id, private: false }, hash, spki, true, usages & public_usages));
    let vm = global_object.vm();
    let pair = construct_empty_object(global_object);
    pair.put_direct(vm, &property_key(vm, "privateKey"), private_key, 0);
    pair.put_direct(vm, &property_key(vm, "publicKey"), public_key, 0);
    Ok(pair.as_value())
}

/// O DER (SPKI ou PKCS#8) de um JWK RSA e se é privado (`d` presente), na ordem de validação do bun: `use`, `alg` e, para
/// todo o resto (`kty`, componentes, `ext`, `key_ops`), o `Invalid keyData`.
fn rsa_jwk_material(global_object: &JSGlobalObject, call: &HostCall, jwk: &Jwk, id: AlgorithmId, hash: usize, extractable: bool, usages: u16) -> Result<(Vec<u8>, bool), Thrown> {
    let expected_use = if id == AlgorithmId::RsaOaep { "enc" } else { "sig" };
    if usages != 0 && jwk.use_.as_deref().is_some_and(|use_| use_ != expected_use) {
        return Err(dom_error(global_object, call, "DataError", "Invalid JWK \"use\" Parameter"));
    }
    if jwk.alg.as_deref().is_some_and(|alg| Some(alg) != rsa_jwk_alg(id, hash).as_deref()) {
        return Err(dom_error(global_object, call, "DataError", "JWK \"alg\" does not match the requested algorithm"));
    }
    let invalid = || dom_error(global_object, call, "DataError", "Invalid keyData");
    let bad_ops = jwk.key_ops.as_deref().is_some_and(|operations| usage_mask_of(operations) & usages != usages);
    if jwk.kty.as_deref() != Some("RSA") || bad_ops || (jwk.ext == Some(false) && extractable) {
        return Err(invalid());
    }
    // Inteiro do JWK: base64url sem zero à esquerda.
    let integer = |text: &Option<String>| Some(base64_url_bytes(text.as_deref()?)).filter(|bytes| bytes.first().is_some_and(|&byte| byte != 0));
    let (n, e) = (integer(&jwk.n).ok_or_else(invalid)?, integer(&jwk.e).ok_or_else(invalid)?);
    // Os componentes privados do bun vêm como estão (zero à esquerda e incoerência passam); só o vazio é recusado.
    let component = |text: &Option<String>| Some(base64_url_bytes(text.as_deref()?)).filter(|bytes| !bytes.is_empty());
    let private = match jwk.d.as_deref() {
        None => None,
        Some(_) => Some(crypto_rsa::PrivateComponents {
            d: component(&jwk.d).ok_or_else(invalid)?,
            p: component(&jwk.p).ok_or_else(invalid)?,
            q: component(&jwk.q).ok_or_else(invalid)?,
            dp: component(&jwk.dp).ok_or_else(invalid)?,
            dq: component(&jwk.dq).ok_or_else(invalid)?,
            qi: component(&jwk.qi).ok_or_else(invalid)?,
        }),
    };
    let is_private = private.is_some();
    let der = crypto_rsa::from_components(&crypto_rsa::RsaComponents { n, e, private }).ok_or_else(invalid)?;
    Ok((der, is_private))
}

/// `importKey` de RSASSA-PKCS1-v1_5, RSA-PSS e RSA-OAEP (`spki`, `pkcs8`, `jwk`; os `raw` não existem).
fn import_rsa_key(
    global_object: &JSGlobalObject,
    call: &HostCall,
    id: AlgorithmId,
    algorithm: JSValue,
    format: &str,
    bytes: Option<Vec<u8>>,
    jwk: Option<Jwk>,
    extractable: bool,
    usages: u16,
) -> Result<KeyState, Thrown> {
    let hash = rsa_hash(global_object, call, algorithm, "RsaHashedImportParams")?;
    // `toKeyData`: `jwk` ausente (`null`/`undefined`) é `DataError`; outro valor que não é objeto, `TypeError`.
    if format == "jwk" && jwk.is_none() {
        return Err(missing_jwk_error(global_object, call, false));
    }
    if matches!(format, "raw" | "raw-public" | "raw-secret" | "raw-seed") {
        let shown = if format == "raw-seed" { format } else { "raw" };
        return Err(dom_error(global_object, call, "NotSupportedError", &format!("Unable to import {} using {shown} format", rsa_name(id))));
    }
    // Os usos são conferidos antes dos dados, pelo tipo que o formato produz (`pkcs8` e JWK com `d` são privados).
    let private_format = format == "pkcs8" || jwk.as_ref().is_some_and(|jwk| jwk.d.is_some());
    if usages & !rsa_usages(id, private_format) != 0 {
        let message = format!("Unsupported key usage for an {} key", rsa_name(id));
        return Err(throw_native_syntax_error(global_object, &message));
    }
    let bytes = bytes.unwrap_or_default();
    let data_error = |message: &str| dom_error(global_object, call, "DataError", message);
    let (der, private) = match (format, jwk) {
        ("spki", _) => (crypto_rsa::parse_spki(&bytes).ok_or_else(|| data_error(rsa_key_error(&bytes, false)))?, false),
        ("pkcs8", _) => (crypto_rsa::parse_pkcs8(&bytes).ok_or_else(|| data_error(rsa_key_error(&bytes, true)))?, true),
        ("jwk", Some(jwk)) => rsa_jwk_material(global_object, call, &jwk, id, hash, extractable, usages)?,
        _ => return Err(dom_error(global_object, call, "NotSupportedError", NOT_SUPPORTED)),
    };
    if private && usages == 0 {
        return Err(throw_native_syntax_error(global_object, "Usages cannot be empty when importing a private key."));
    }
    Ok(rsa_key(Rsa { id, private }, hash, der, extractable, usages))
}

/// A mensagem de um SPKI/PKCS#8 que não é uma chave RSA: `Invalid key type` quando o DER é bem formado mas de outro
/// algoritmo, `Invalid keyData` no resto.
fn rsa_key_error(bytes: &[u8], private: bool) -> &'static str {
    if crypto_rsa::is_foreign_key(bytes, private) { "Invalid key type" } else { "Invalid keyData" }
}

/// `crypto.subtle.generateKey` (`CryptoAlgorithmHMAC::generateKey` e o callback de `SubtleCrypto::generateKey`).
fn generate_key_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let usages = check_usages(global_object, call.argument(2))?;
    let lower_name = algorithm_name(global_object, call.argument(0))?;
    let id = normalize_named(global_object, call, &lower_name, Operation::GenerateKey)?;
    if is_symmetric_id(id) {
        return generate_aes_key(global_object, call, id, usages);
    }
    if id != AlgorithmId::Hmac {
        if is_asym_id(id) {
            return generate_asym_key(global_object, call, id, usages);
        }
        if is_rsa_id(id) {
            return generate_rsa_key(global_object, call, id, usages);
        }
        if let Some(alg) = crypto_pq::PqAlgorithm::from_lower_name(&lower_name) {
            return generate_pq_key(global_object, call, alg, usages);
        }
        return Err(not_supported(global_object, call));
    }
    let (hash, length) = hmac_params(global_object, call, call.argument(0))?;
    if usages & !HMAC_USAGES != 0 {
        return Err(throw_native_syntax_error(global_object, "Unsupported key usage for an HMAC key"));
    }
    if length == Some(0) {
        return Err(dom_error(global_object, call, "OperationError", OPERATION_FAILED));
    }
    let bits = length.map_or(HASHES[hash].key_bits, |bits| bits as usize);
    if bits % 8 != 0 {
        return Err(dom_error(global_object, call, "OperationError", OPERATION_FAILED));
    }
    let mut secret = vec![0u8; bits / 8];
    host().random_fill(&mut secret);
    if usages == 0 {
        return Err(throw_native_syntax_error(global_object, "Usages cannot be empty when creating a key."));
    }
    Ok(create_key(global_object, hmac_secret_key(hash, secret, call.argument(1).to_boolean(), usages)))
}

/// A chave `oct` de um JWK (`CryptoAlgorithmHMAC`/`CryptoAlgorithmChaCha20Poly1305::importKey`, ramo `Jwk`), na ordem de
/// validação do bun: `use_value` é o `use` aceito (`sig` no HMAC, `enc` no ChaCha) e `alg` o `alg` esperado.
fn oct_jwk_secret(
    global_object: &JSGlobalObject,
    call: &HostCall,
    jwk: &Jwk,
    alg: Option<&str>,
    use_value: &str,
    extractable: bool,
    usages: u16,
) -> Result<Vec<u8>, Thrown> {
    if jwk.kty.is_none() {
        return Err(dom_error(global_object, call, "DataError", "Invalid keyData"));
    }
    if jwk.kty.as_deref() != Some("oct") {
        return Err(dom_error(global_object, call, "DataError", "Invalid JWK \"kty\" Parameter"));
    }
    let Some(k) = jwk.k.as_deref() else {
        return Err(dom_error(global_object, call, "DataError", "Invalid keyData"));
    };
    if usages != 0 && jwk.use_.as_deref().is_some_and(|use_| use_ != use_value) {
        return Err(dom_error(global_object, call, "DataError", "Invalid JWK \"use\" Parameter"));
    }
    if jwk.key_ops.as_deref().is_some_and(has_duplicate) {
        return Err(dom_error(global_object, call, "DataError", "Duplicate key operation"));
    }
    if jwk.key_ops.as_deref().is_some_and(|operations| usage_mask_of(operations) & usages != usages) {
        return Err(dom_error(global_object, call, "DataError", "Key operations and usage mismatch"));
    }
    if jwk.ext == Some(false) && extractable {
        return Err(dom_error(global_object, call, "DataError", "JWK \"ext\" Parameter and extractable mismatch"));
    }
    if jwk.alg.as_deref().is_some_and(|given| Some(given) != alg) {
        return Err(dom_error(global_object, call, "DataError", "JWK \"alg\" does not match the requested algorithm"));
    }
    // A decodificação base64url do bun é leniente: entrada inválida vira chave vazia.
    Ok(base64_url_bytes(k))
}

/// A chave AES de um JWK (`CryptoKeyAES` via `CryptoAlgorithmAES_*::importKey`, ramo `Jwk`): todo defeito é o mesmo
/// `DataError` genérico. O `alg` é `A<bits><sufixo>` e vale para o tamanho da chave decodificada.
fn aes_jwk_secret(id: AlgorithmId, jwk: &Jwk, extractable: bool, usages: u16) -> Option<Vec<u8>> {
    let k = jwk.k.as_deref().filter(|_| jwk.kty.as_deref() == Some("oct"))?;
    let invalid = (usages != 0 && jwk.use_.as_deref().is_some_and(|use_| use_ != "enc"))
        // Medido no bun: `key_ops` duplicado passa no AES (o ChaCha20-Poly1305 e o HMAC recusam); só a incompatibilidade vale.
        || jwk.key_ops.as_deref().is_some_and(|operations| usage_mask_of(operations) & usages != usages)
        || (jwk.ext == Some(false) && extractable);
    if invalid {
        return None;
    }
    let secret = base64_url_bytes(k);
    let alg = jwk_aes_alg(id, secret.len());
    (jwk.alg.as_deref().is_none_or(|given| given == alg)).then_some(secret)
}

/// `importKey` de AES: formato, usos, material e tamanho (16, 24 ou 32 bytes), na ordem do bun.
fn import_aes_key(
    global_object: &JSGlobalObject,
    call: &HostCall,
    id: AlgorithmId,
    format: &str,
    bytes: Option<Vec<u8>>,
    jwk: Option<Jwk>,
    extractable: bool,
    usages: u16,
) -> Result<KeyState, Thrown> {
    // `toKeyData`: `jwk` ausente (`null`/`undefined`) é `DataError`; outro valor que não é objeto, `TypeError`.
    if format == "jwk" && jwk.is_none() {
        return Err(missing_jwk_error(global_object, call, true));
    }
    if matches!(format, "raw-public" | "raw-seed") {
        return Err(dom_error(global_object, call, "NotSupportedError", &format!("Unable to import {} using {format} format", aes_name(id))));
    }
    if usages & !aes_usages(id) != 0 {
        return Err(unsupported_usage(global_object, id));
    }
    // O ChaCha20-Poly1305 só importa `raw-secret` e `jwk` (o `raw` é recusado), com chave de exatos 32 bytes e mensagens próprias.
    let chacha = id == AlgorithmId::ChaCha20Poly1305;
    let secret = match (format, jwk) {
        ("raw", _) if chacha => return Err(dom_error(global_object, call, "NotSupportedError", NOT_SUPPORTED)),
        ("raw" | "raw-secret", _) => bytes.unwrap_or_default(),
        ("jwk", Some(jwk)) if chacha => oct_jwk_secret(global_object, call, &jwk, Some("C20P"), "enc", extractable, usages)?,
        ("jwk", Some(jwk)) => aes_jwk_secret(id, &jwk, extractable, usages).unwrap_or_default(),
        _ => return Err(dom_error(global_object, call, "NotSupportedError", NOT_SUPPORTED)),
    };
    if chacha && secret.len() != 32 {
        return Err(dom_error(global_object, call, "DataError", "Invalid key length"));
    }
    if !chacha && !matches!(secret.len(), 16 | 24 | 32) {
        return Err(dom_error(global_object, call, "DataError", DATA_ERROR));
    }
    if usages == 0 {
        return Err(throw_native_syntax_error(global_object, "Usages cannot be empty when importing a secret key."));
    }
    Ok(aes_secret_key(id, secret, extractable, usages))
}

/// O `importKey` depois da leitura dos dados e dos usos (compartilhado com `unwrapKey`): lê o `name` do algoritmo uma
/// vez, normaliza e despacha.
fn import_secret(
    global_object: &JSGlobalObject,
    call: &HostCall,
    format: &str,
    bytes: Option<Vec<u8>>,
    jwk: Option<Jwk>,
    algorithm: JSValue,
    extractable: bool,
    usages: u16,
) -> Result<KeyState, Thrown> {
    let lower_name = algorithm_name(global_object, algorithm)?;
    import_named(global_object, call, &lower_name, format, bytes, jwk, algorithm, extractable, usages)
}

/// O despacho do `importKey` com o `name` já lido (o `encapsulateKey` e o `decapsulateKey` já o leram uma vez, e o getter
/// do `sharedKeyAlgorithm` não pode rodar de novo).
fn import_named(
    global_object: &JSGlobalObject,
    call: &HostCall,
    lower_name: &str,
    format: &str,
    bytes: Option<Vec<u8>>,
    jwk: Option<Jwk>,
    algorithm: JSValue,
    extractable: bool,
    usages: u16,
) -> Result<KeyState, Thrown> {
    let id = normalize_named(global_object, call, lower_name, Operation::ImportKey)?;
    if is_symmetric_id(id) {
        return import_aes_key(global_object, call, id, format, bytes, jwk, extractable, usages);
    }
    if crypto_kdf::is_kdf_id(id) {
        if format == "jwk" && jwk.is_none() {
            return Err(throw_native_type_error(global_object, "Type error"));
        }
        return crypto_kdf::import_kdf_key(global_object, call, id, format, bytes, extractable, usages);
    }
    if id != AlgorithmId::Hmac {
        if is_asym_id(id) {
            return import_asym_key(global_object, call, id, algorithm, format, bytes, jwk, extractable, usages);
        }
        if is_rsa_id(id) {
            return import_rsa_key(global_object, call, id, algorithm, format, bytes, jwk, extractable, usages);
        }
        if let Some(alg) = crypto_pq::PqAlgorithm::from_lower_name(lower_name) {
            return import_pq_key(global_object, call, alg, format, bytes, jwk, extractable, usages);
        }
        Err(not_supported(global_object, call))
    } else {
        import_hmac_key(global_object, call, format, bytes, jwk, algorithm, extractable, usages)
    }
}

/// `importKey` de HMAC (o caminho que sobra depois de AES, KDF, curva elíptica, RSA e pós-quântico).
fn import_hmac_key(
    global_object: &JSGlobalObject,
    call: &HostCall,
    format: &str,
    bytes: Option<Vec<u8>>,
    jwk: Option<Jwk>,
    algorithm: JSValue,
    extractable: bool,
    usages: u16,
) -> Result<KeyState, Thrown> {
    let (hash, length) = hmac_params(global_object, call, algorithm)?;
    // `toKeyData`: `jwk` ausente (`null`/`undefined`) é `DataError`; outro valor que não é objeto, `TypeError`.
    if format == "jwk" && jwk.is_none() {
        return Err(missing_jwk_error(global_object, call, false));
    }
    // `aliasImportKeyFormat`: o HMAC não é AKP nem tem `raw-public`/`raw-seed`.
    if matches!(format, "raw-public" | "raw-seed") {
        return Err(dom_error(global_object, call, "NotSupportedError", &format!("Unable to import HMAC using {format} format")));
    }
    if usages & !HMAC_USAGES != 0 {
        return Err(throw_native_syntax_error(global_object, "Unsupported key usage for HMAC key"));
    }
    if length == Some(0) {
        return Err(dom_error(global_object, call, "DataError", "HmacImportParams.length cannot be 0"));
    }
    if length.is_some_and(|bits| bits % 8 != 0) {
        return Err(dom_error(global_object, call, "NotSupportedError", "Unsupported HmacImportParams.length"));
    }
    let secret = match (format, jwk) {
        ("raw" | "raw-secret", _) => bytes.unwrap_or_default(),
        ("jwk", Some(jwk)) => oct_jwk_secret(global_object, call, &jwk, HASHES[hash].jwk_alg, "sig", extractable, usages)?,
        _ => return Err(dom_error(global_object, call, "NotSupportedError", NOT_SUPPORTED)),
    };
    if secret.is_empty() {
        return Err(dom_error(global_object, call, "DataError", "Zero-length key is not supported"));
    }
    if length.is_some_and(|bits| bits as usize != secret.len() * 8) {
        return Err(dom_error(global_object, call, "DataError", "Invalid key length"));
    }
    if usages == 0 {
        return Err(throw_native_syntax_error(global_object, "Usages cannot be empty when importing a secret key."));
    }
    Ok(hmac_secret_key(hash, secret, extractable, usages))
}

/// `crypto.subtle.importKey` (`SubtleCrypto::importKey` e `CryptoAlgorithmHMAC::importKey`/`CryptoAlgorithmAES_*`).
fn import_key_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = "importKey";
    let format = check_format(global_object, name, call.argument(0))?;
    let data = call.argument(1);
    let bytes = input_bytes(data);
    let mut jwk = None;
    if format == "jwk" {
        // `null` e `undefined` seguem sem `jwk`: o erro (`DataError`) depende do algoritmo e sai de `missing_jwk_error`.
        match JSObject::from_value(&data) {
            Some(object) if bytes.is_none() => jwk = Some(parse_jwk(global_object, &object)?),
            Some(_) => {}
            None if data.is_undefined_or_null() => {}
            None => return Err(throw_native_type_error(global_object, "Type error")),
        }
    } else if bytes.is_none() {
        let message = "Failed to execute 'importKey' on 'SubtleCrypto': 2nd argument is not instance of ArrayBuffer, Buffer, TypedArray, or DataView.";
        return Err(throw_coded_type_error(global_object, message, "ERR_INVALID_ARG_TYPE"));
    }
    let usages = check_usages(global_object, call.argument(4))?;
    let key = import_secret(global_object, call, &format, bytes, jwk, call.argument(2), call.argument(3).to_boolean(), usages)?;
    Ok(create_key(global_object, key))
}

/// As verificações de `exportKey` de uma chave secreta (extraível, formato que ela exporta), antes de produzir os dados.
fn check_exportable(global_object: &JSGlobalObject, call: &HostCall, key: &KeyState, format: &str) -> Result<(), Thrown> {
    if key.rsa.is_some() {
        return check_rsa_exportable(global_object, call, key, format);
    }
    if key.asym.is_some() {
        return check_asym_exportable(global_object, call, key, format);
    }
    if key.pq.is_some() {
        return check_pq_exportable(global_object, call, key, format);
    }
    if key.aes.is_some_and(crypto_kdf::is_kdf_id) {
        return Err(crypto_kdf::export_unsupported(global_object, call));
    }
    if !key.extractable {
        return Err(dom_error(global_object, call, "InvalidAccessError", "key is not extractable"));
    }
    match format {
        "raw-public" | "raw-seed" => {
            let message = format!("Unable to export {} secret key using {format} format", key_algorithm_name(key));
            Err(dom_error(global_object, call, "NotSupportedError", &message))
        }
        "raw" if key.aes == Some(AlgorithmId::ChaCha20Poly1305) => Err(dom_error(global_object, call, "NotSupportedError", NOT_SUPPORTED)),
        "raw" | "raw-secret" | "jwk" => Ok(()),
        _ => Err(dom_error(global_object, call, "NotSupportedError", NOT_SUPPORTED)),
    }
}

/// `crypto.subtle.exportKey` de uma chave secreta.
fn export_key_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = "exportKey";
    let format = check_format(global_object, name, call.argument(0))?;
    let key = key_at(global_object, call, name, 1, 2, "key")?;
    check_exportable(global_object, call, &key, &format)?;
    if format == "jwk" {
        return Ok(jwk_value(global_object, &key));
    }
    Ok(array_buffer_value(global_object, &export_key_bytes(&key, &format)))
}

/// Uma cifra de bloco AES com a chave de 128, 192 ou 256 bits.
enum AesCipher {
    K128(Aes128),
    K192(Aes192),
    K256(Aes256),
}

impl AesCipher {
    fn new(key: &[u8]) -> Option<Self> {
        match key.len() {
            16 => Aes128::new_from_slice(key).ok().map(Self::K128),
            24 => Aes192::new_from_slice(key).ok().map(Self::K192),
            32 => Aes256::new_from_slice(key).ok().map(Self::K256),
            _ => None,
        }
    }

    fn encrypt_block(&self, block: &mut [u8; 16]) {
        let array = GenericArray::from_mut_slice(block);
        match self {
            Self::K128(cipher) => cipher.encrypt_block(array),
            Self::K192(cipher) => cipher.encrypt_block(array),
            Self::K256(cipher) => cipher.encrypt_block(array),
        }
    }

    fn decrypt_block(&self, block: &mut [u8; 16]) {
        let array = GenericArray::from_mut_slice(block);
        match self {
            Self::K128(cipher) => cipher.decrypt_block(array),
            Self::K192(cipher) => cipher.decrypt_block(array),
            Self::K256(cipher) => cipher.decrypt_block(array),
        }
    }
}

fn xor_block(block: &mut [u8; 16], other: &[u8; 16]) {
    block.iter_mut().zip(other).for_each(|(left, right)| *left ^= right);
}

/// Um bloco de 16 bytes com o início de `chunk`, completado com zeros.
fn padded_block(chunk: &[u8]) -> [u8; 16] {
    let mut block = [0u8; 16];
    block[..chunk.len()].copy_from_slice(chunk);
    block
}

/// AES-CBC com preenchimento PKCS#7.
fn cbc_transform(cipher: &AesCipher, iv: [u8; 16], data: &[u8], decrypt: bool) -> Result<Vec<u8>, String> {
    let mut previous = iv;
    let mut output = Vec::with_capacity(data.len() + 16);
    if !decrypt {
        let pad = 16 - data.len() % 16;
        let mut padded = data.to_vec();
        padded.extend(std::iter::repeat_n(pad as u8, pad));
        for chunk in padded.chunks(16) {
            let mut block = padded_block(chunk);
            xor_block(&mut block, &previous);
            cipher.encrypt_block(&mut block);
            output.extend_from_slice(&block);
            previous = block;
        }
        return Ok(output);
    }
    if data.is_empty() || data.len() % 16 != 0 {
        return Err(OPERATION_FAILED.to_owned());
    }
    for chunk in data.chunks(16) {
        let encrypted = padded_block(chunk);
        let mut block = encrypted;
        cipher.decrypt_block(&mut block);
        xor_block(&mut block, &previous);
        output.extend_from_slice(&block);
        previous = encrypted;
    }
    let pad = usize::from(output.last().copied().unwrap_or(0));
    if pad == 0 || pad > 16 || output[output.len() - pad..].iter().any(|byte| usize::from(*byte) != pad) {
        return Err(OPERATION_FAILED.to_owned());
    }
    output.truncate(output.len() - pad);
    Ok(output)
}

/// AES-CTR: só os `length` bits baixos do contador giram; mais blocos do que `2^length` é erro.
fn ctr_transform(cipher: &AesCipher, counter: [u8; 16], length: u32, data: &[u8]) -> Result<Vec<u8>, String> {
    if length == 0 || length > 128 {
        return Err(OPERATION_FAILED.to_owned());
    }
    let blocks = data.len().div_ceil(16) as u128;
    if length < 128 && blocks > 1u128 << length {
        return Err(OPERATION_FAILED.to_owned());
    }
    let mask = if length == 128 { u128::MAX } else { (1u128 << length) - 1 };
    let mut value = u128::from_be_bytes(counter);
    let mut output = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut stream = value.to_be_bytes();
        cipher.encrypt_block(&mut stream);
        output.extend(chunk.iter().zip(stream).map(|(byte, key)| byte ^ key));
        value = (value & !mask) | ((value & mask).wrapping_add(1) & mask);
    }
    Ok(output)
}

/// A multiplicação em GF(2^128) do GHASH (NIST SP 800-38D, algoritmo 1), com o bit mais significativo primeiro.
fn gf_multiply(x: u128, y: u128) -> u128 {
    let mut product = 0u128;
    let mut value = y;
    for bit in 0..128 {
        if (x >> (127 - bit)) & 1 == 1 {
            product ^= value;
        }
        value = if value & 1 == 1 { (value >> 1) ^ (0xe1u128 << 120) } else { value >> 1 };
    }
    product
}

/// GHASH sobre `data` completado com zeros até o bloco, a partir de `state`.
fn ghash_update(subkey: u128, mut state: u128, data: &[u8]) -> u128 {
    for chunk in data.chunks(16) {
        state = gf_multiply(state ^ u128::from_be_bytes(padded_block(chunk)), subkey);
    }
    state
}

/// AES-GCM com IV de qualquer tamanho (não vazio) e etiqueta de `tag_bits` bits; a decifragem confere a etiqueta.
fn gcm_transform(cipher: &AesCipher, iv: &[u8], additional_data: &[u8], tag_bits: u32, data: &[u8], decrypt: bool) -> Result<Vec<u8>, String> {
    if !matches!(tag_bits, 32 | 64 | 96 | 104 | 112 | 120 | 128) {
        return Err(format!("{tag_bits} is not a valid AES-GCM tag length"));
    }
    if iv.is_empty() {
        return Err(OPERATION_FAILED.to_owned());
    }
    let tag_length = tag_bits as usize / 8;
    let mut zero = [0u8; 16];
    cipher.encrypt_block(&mut zero);
    let subkey = u128::from_be_bytes(zero);
    let mut counter = if iv.len() == 12 {
        let mut block = [0u8; 16];
        block[..12].copy_from_slice(iv);
        block[15] = 1;
        u128::from_be_bytes(block)
    } else {
        let state = ghash_update(subkey, 0, iv);
        gf_multiply(state ^ ((iv.len() as u128 * 8) & u128::from(u64::MAX)), subkey)
    };
    let mut first = counter.to_be_bytes();
    cipher.encrypt_block(&mut first);
    let (body, received_tag) = if decrypt {
        if data.len() < tag_length {
            return Err(OPERATION_FAILED.to_owned());
        }
        data.split_at(data.len() - tag_length)
    } else {
        (data, &[][..])
    };
    let mut transformed = Vec::with_capacity(body.len() + tag_length);
    for chunk in body.chunks(16) {
        // `inc32`: só os 32 bits baixos giram.
        counter = (counter & !u128::from(u32::MAX)) | u128::from((counter as u32).wrapping_add(1));
        let mut stream = counter.to_be_bytes();
        cipher.encrypt_block(&mut stream);
        transformed.extend(chunk.iter().zip(stream).map(|(byte, key)| byte ^ key));
    }
    let ciphertext: &[u8] = if decrypt { body } else { &transformed };
    let mut state = ghash_update(subkey, 0, additional_data);
    state = ghash_update(subkey, state, ciphertext);
    let lengths = ((additional_data.len() as u128 * 8) << 64) | (ciphertext.len() as u128 * 8);
    state = gf_multiply(state ^ lengths, subkey);
    let mut tag = state.to_be_bytes();
    xor_block(&mut tag, &first);
    if decrypt {
        let difference = tag[..tag_length].iter().zip(received_tag).fold(0u8, |accumulator, (left, right)| accumulator | (left ^ right));
        return if difference == 0 { Ok(transformed) } else { Err(OPERATION_FAILED.to_owned()) };
    }
    transformed.extend_from_slice(&tag[..tag_length]);
    Ok(transformed)
}

/// `AES-KW` (RFC 3394): `wrap_vec`/`unwrap_vec` da crate, que recusam o que não é múltiplo de 8 bytes (mínimo 16).
fn kw_transform(key: &[u8], data: &[u8], unwrap: bool) -> Result<Vec<u8>, String> {
    macro_rules! run {
        ($kek:ident) => {{
            let kek = $kek::new(GenericArray::from_slice(key));
            if unwrap { kek.unwrap_vec(data) } else { kek.wrap_vec(data) }
        }};
    }
    // Medido no bun 1.4.2: o `wrapKey` recusa dado que não seja múltiplo de 8 bytes ou tenha menos de 16.
    if !unwrap && (data.len() % 8 != 0 || data.len() < 16) {
        return Err(OPERATION_FAILED.to_owned());
    }
    let result = match key.len() {
        16 => run!(KekAes128),
        24 => run!(KekAes192),
        _ => run!(KekAes256),
    };
    result.map_err(|_| OPERATION_FAILED.to_owned())
}

/// O IV de exatos `N` bytes; o erro é a mensagem do `OperationError`.
fn fixed_iv<const N: usize>(iv: &[u8]) -> Result<[u8; N], String> {
    iv.try_into().map_err(|_| format!("algorithm.iv must contain exactly {N} bytes"))
}

/// AES-CFB-8: o registrador de 16 bytes começa no IV, cada byte se mistura com o primeiro byte do registrador cifrado e o
/// byte de texto cifrado entra pela direita do registrador (na decifragem é o byte de entrada).
fn cfb8_transform(cipher: &AesCipher, iv: [u8; 16], data: &[u8], decrypt: bool) -> Vec<u8> {
    let mut register = iv;
    data.iter()
        .map(|&byte| {
            let mut block = register;
            cipher.encrypt_block(&mut block);
            let output = byte ^ block[0];
            register.copy_within(1.., 0);
            register[15] = if decrypt { byte } else { output };
            output
        })
        .collect()
}

/// ChaCha20-Poly1305 (RFC 8439): a etiqueta é sempre de 128 bits e vai colada ao fim do texto cifrado. A conferência da
/// etiqueta vem antes da do IV.
fn chacha_transform(secret: &[u8], iv: &[u8], additional_data: &[u8], tag_bits: u32, data: &[u8], decrypt: bool) -> Result<Vec<u8>, String> {
    if tag_bits != 128 {
        return Err(format!("{tag_bits} is not a valid ChaCha20-Poly1305 tag length"));
    }
    let iv = fixed_iv::<12>(iv)?;
    let cipher = ChaCha20Poly1305::new_from_slice(secret).map_err(|_| OPERATION_FAILED.to_owned())?;
    let payload = Payload { msg: data, aad: additional_data };
    let result = if decrypt { cipher.decrypt(Nonce::from_slice(&iv), payload) } else { cipher.encrypt(Nonce::from_slice(&iv), payload) };
    result.map_err(|_| OPERATION_FAILED.to_owned())
}

/// Cifra ou decifra `data` com a chave AES e os parâmetros; o erro é a mensagem do `OperationError`.
fn aes_transform(secret: &[u8], params: &AesParams, data: &[u8], decrypt: bool) -> Result<Vec<u8>, String> {
    let cipher = AesCipher::new(secret).ok_or_else(|| OPERATION_FAILED.to_owned())?;
    match params {
        AesParams::Cbc { iv } => cbc_transform(&cipher, fixed_iv::<16>(iv)?, data, decrypt),
        AesParams::Cfb { iv } => Ok(cfb8_transform(&cipher, fixed_iv::<16>(iv)?, data, decrypt)),
        AesParams::ChaCha { iv, additional_data, tag_bits } => chacha_transform(secret, iv, additional_data, *tag_bits, data, decrypt),
        AesParams::Ctr { counter, length } => {
            let counter: [u8; 16] = counter.as_slice().try_into().map_err(|_| OPERATION_FAILED.to_owned())?;
            ctr_transform(&cipher, counter, *length, data)
        }
        AesParams::Gcm { iv, additional_data, tag_bits } => gcm_transform(&cipher, iv, additional_data, *tag_bits, data, decrypt),
        AesParams::Kw => kw_transform(secret, data, decrypt),
        // O RSA-OAEP não usa a chave AES: `crypt_body_impl` o resolve antes de chegar aqui.
        AesParams::Oaep { .. } => Err(OPERATION_FAILED.to_owned()),
    }
}

/// A chave `key` tem de ser do algoritmo `id` (`Key algorithm mismatch`) e ter o uso `usage` (`Unable to use this key to ...`).
fn check_key_use(global_object: &JSGlobalObject, call: &HostCall, key: &KeyState, id: AlgorithmId, usage: u16, name: &str) -> Result<(), Thrown> {
    let matches = if is_rsa_id(id) { key.rsa.is_some_and(|rsa| rsa.id == id) } else { key.aes == Some(id) };
    if !matches {
        return Err(dom_error(global_object, call, "InvalidAccessError", "Key algorithm mismatch"));
    }
    if key.usages & usage == 0 {
        return Err(dom_error(global_object, call, "InvalidAccessError", &format!("Unable to use this key to {name}")));
    }
    Ok(())
}

/// Normaliza o algoritmo de cifragem/embrulho e converte os parâmetros. `normalize_algorithm` só deixa passar os algoritmos
/// que `Encrypt` e `WrapKey` aceitam (AES, ChaCha20-Poly1305 e RSA-OAEP); o resto já saiu como `Unrecognized algorithm name`.
fn crypt_algorithm(global_object: &JSGlobalObject, call: &HostCall, value: JSValue, operation: Operation) -> Result<(AlgorithmId, AesParams), Thrown> {
    let id = normalize_algorithm(global_object, call, value, operation)?;
    Ok((id, aes_params(global_object, id, value)?))
}

fn operation_error(global_object: &JSGlobalObject, call: &HostCall, message: &str) -> Thrown {
    dom_error(global_object, call, "OperationError", message)
}

/// `encrypt` e `decrypt` (`SubtleCrypto::encrypt`/`decrypt` e `CryptoAlgorithmAES_*`).
fn crypt_body_impl(global_object: &JSGlobalObject, call: &HostCall, decrypt: bool) -> HostResult {
    let name = if decrypt { "decrypt" } else { "encrypt" };
    let key = key_at(global_object, call, name, 1, 2, "key")?;
    let data = buffer_source(global_object, call.argument(2))?;
    let (id, params) = crypt_algorithm(global_object, call, call.argument(0), Operation::Encrypt)?;
    check_key_use(global_object, call, &key, id, if decrypt { USAGE_DECRYPT } else { USAGE_ENCRYPT }, name)?;
    let output = key_transform(global_object, call, &key, &params, &data, decrypt)?;
    Ok(array_buffer_value(global_object, &output))
}

/// Cifra ou decifra `data` com a chave `key` (AES ou RSA-OAEP), o miolo comum de `encrypt`, `decrypt`, `wrapKey` e `unwrapKey`.
fn key_transform(global_object: &JSGlobalObject, call: &HostCall, key: &KeyState, params: &AesParams, data: &[u8], decrypt: bool) -> Result<Vec<u8>, Thrown> {
    match params {
        AesParams::Oaep { label } => {
            let hash = HASHES[key.hash].algo;
            // Medido no bun 1.4.2: módulo pequeno demais para o hash é `OperationError` (a conta falha dentro do OAEP).
            let result = if decrypt { crypto_rsa::decrypt_oaep(&key.secret, data, hash, label) } else { crypto_rsa::encrypt_oaep(&key.secret, data, hash, label) };
            result.ok_or_else(|| operation_error(global_object, call, OPERATION_FAILED))
        }
        _ => aes_transform(&key.secret, params, data, decrypt).map_err(|message| operation_error(global_object, call, &message)),
    }
}

/// `wrapKey`: exporta a chave no formato e a cifra com a chave de embrulho. O JWK do `AES-KW` completa o texto com espaços até
/// múltiplo de 8 bytes.
fn wrap_key_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = "wrapKey";
    let format = check_format(global_object, name, call.argument(0))?;
    let key = key_at(global_object, call, name, 1, 2, "key")?;
    let wrapping = key_at(global_object, call, name, 2, 3, "wrappingKey")?;
    let (id, params) = crypt_algorithm(global_object, call, call.argument(3), Operation::WrapKey)?;
    check_key_use(global_object, call, &wrapping, id, USAGE_WRAP, name)?;
    check_exportable(global_object, call, &key, &format)?;
    let mut bytes = if format == "jwk" { jwk_json(&key).into_bytes() } else { export_key_bytes(&key, &format) };
    if format == "jwk" && id == AlgorithmId::AesKw {
        bytes.resize(bytes.len().next_multiple_of(8), b' ');
    }
    let output = key_transform(global_object, call, &wrapping, &params, &bytes, false)?;
    Ok(array_buffer_value(global_object, &output))
}

/// `unwrapKey`: decifra com a chave de embrulho e importa o resultado como o algoritmo pedido.
fn unwrap_key_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = "unwrapKey";
    let format = check_format(global_object, name, call.argument(0))?;
    let Some(wrapped) = input_bytes(call.argument(1)) else {
        return Err(throw_native_type_error(global_object, "Type error"));
    };
    let unwrapping = key_at(global_object, call, name, 2, 3, "unwrappingKey")?;
    let usages = check_usages(global_object, call.argument(6))?;
    let (id, params) = crypt_algorithm(global_object, call, call.argument(3), Operation::WrapKey)?;
    check_key_use(global_object, call, &unwrapping, id, USAGE_UNWRAP, name)?;
    let plain = key_transform(global_object, call, &unwrapping, &params, &wrapped, true)?;
    let (bytes, jwk) = if format == "jwk" {
        let text = WtfString::from_utf8(&plain);
        let parsed = json_parse_quiet(global_object, &text).ok().flatten().and_then(|value| JSObject::from_value(&value));
        let Some(object) = parsed else {
            return Err(dom_error(global_object, call, "DataError", "WrappedKey cannot be converted to a JSON object"));
        };
        (None, Some(parse_jwk(global_object, &object)?))
    } else {
        (Some(plain), None)
    };
    let key = import_secret(global_object, call, &format, bytes, jwk, call.argument(4), call.argument(5).to_boolean(), usages)?;
    Ok(create_key(global_object, key))
}

/// `sign` e `verify` (`SubtleCrypto::sign`/`verify` e `CryptoAlgorithmHMAC`): `verify` resolve com booleano.
fn sign_verify_body_impl(global_object: &JSGlobalObject, call: &HostCall, verify: bool) -> HostResult {
    let name = if verify { "verify" } else { "sign" };
    let key = key_at(global_object, call, name, 1, 2, "key")?;
    let signature = if verify { Some(buffer_source(global_object, call.argument(2))?) } else { None };
    let data = buffer_source(global_object, call.argument(if verify { 3 } else { 2 }))?;
    let lower_name = algorithm_name(global_object, call.argument(0))?;
    let id = normalize_named(global_object, call, &lower_name, Operation::SignVerify)?;
    let mismatch = || dom_error(global_object, call, "InvalidAccessError", "Key algorithm mismatch");
    let mut ecdsa_digest = Algo::Sha256;
    let mut salt_length = 0u32;
    let mut context = Vec::new();
    match id {
        AlgorithmId::MlDsa => {
            let member = dictionary_member(global_object, call.argument(0), "context");
            if !member.is_undefined() {
                context = buffer_source(global_object, member)?;
            }
            let alg = crypto_pq::PqAlgorithm::from_lower_name(&lower_name);
            if key.pq.is_none_or(|pq| Some(pq.alg) != alg) {
                return Err(mismatch());
            }
        }
        AlgorithmId::RsaPss | AlgorithmId::RsassaPkcs1V15 => {
            if id == AlgorithmId::RsaPss {
                let member = required_member(global_object, call.argument(0), "RsaPssParams", "saltLength", "unsigned long")?;
                salt_length = enforce_unsigned_long(global_object, member)?;
            }
            if key.rsa.is_none_or(|rsa| rsa.id != id) {
                return Err(mismatch());
            }
        }
        AlgorithmId::Hmac => {
            if key.aes.is_some() || key.asym.is_some() || key.rsa.is_some() || key.pq.is_some() {
                return Err(mismatch());
            }
        }
        AlgorithmId::Ecdsa | AlgorithmId::Ed25519 => {
            if id == AlgorithmId::Ecdsa {
                ecdsa_digest = HASHES[ecdsa_hash(global_object, call, call.argument(0))?].algo;
            }
            if key.asym.is_none_or(|asym| asym.id != id) {
                return Err(mismatch());
            }
        }
        _ => return Err(mismatch()),
    }
    let needed = if verify { USAGE_VERIFY } else { USAGE_SIGN };
    if key.usages & needed == 0 {
        return Err(dom_error(global_object, call, "InvalidAccessError", &format!("Unable to use this key to {name}")));
    }
    if let Some(pq) = key.pq {
        let failed = || operation_error(global_object, call, OPERATION_FAILED);
        return match signature {
            Some(signature) => pq.alg.verify(&key.secret, &data, &context, &signature).map(JSValue::Bool).ok_or_else(failed),
            None => pq.alg.sign(&key.secret, &data, &context).map(|bytes| array_buffer_value(global_object, &bytes)).ok_or_else(failed),
        };
    }
    if let Some(rsa) = key.rsa {
        let hash = HASHES[key.hash].algo;
        let pss = rsa.id == AlgorithmId::RsaPss;
        if let Some(signature) = signature {
            let valid = if pss {
                crypto_rsa::verify_pss(&key.secret, &data, &signature, hash, salt_length as usize)
            } else {
                crypto_rsa::verify_pkcs1(&key.secret, &data, &signature, hash)
            };
            return Ok(JSValue::Bool(valid));
        }
        let output = if pss { crypto_rsa::sign_pss(&key.secret, &data, hash, salt_length as usize) } else { crypto_rsa::sign_pkcs1(&key.secret, &data, hash) };
        return output.map(|bytes| array_buffer_value(global_object, &bytes)).ok_or_else(|| operation_error(global_object, call, OPERATION_FAILED));
    }
    if let Some(asym) = key.asym {
        if verify {
            let public = asym_public(&key);
            return Ok(JSValue::Bool(asym.curve.verify(&public, &data, &signature.unwrap_or_default(), ecdsa_digest)));
        }
        let signature = asym.curve.sign(&key.secret, &data, ecdsa_digest);
        return signature.map(|bytes| array_buffer_value(global_object, &bytes)).ok_or_else(|| operation_error(global_object, call, OPERATION_FAILED));
    }
    let mac = hmac(HASHES[key.hash].algo, &key.secret, &data);
    match signature {
        None => Ok(array_buffer_value(global_object, &mac)),
        Some(signature) => {
            let difference = mac.iter().zip(&signature).fold(0u8, |accumulator, (left, right)| accumulator | (left ^ right));
            Ok(JSValue::Bool(mac.len() == signature.len() && difference == 0))
        }
    }
}

/// O `this` de um acessor de `CryptoKey` e o estado dele; senão o `TypeError` sem `code` do bun.
fn key_accessor_this(global_object: &JSGlobalObject, call: &HostCall, attribute: &str) -> Result<(JSValue, KeyState), Thrown> {
    let this = call.this_value();
    KEYS.with(|keys| keys.borrow().get(&this.encode()).cloned()).map(|state| (this, state)).ok_or_else(|| {
        throw_native_type_error(global_object, &format!("The CryptoKey.{attribute} getter can only be used on instances of CryptoKey"))
    })
}

fn key_type_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (_, state) = key_accessor_this(global_object, call, "type")?;
    Ok(ascii_value(global_object, key_type_name(&state)))
}

fn key_extractable_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(JSValue::Bool(key_accessor_this(global_object, call, "extractable")?.1.extractable))
}

/// `algorithm` e `usages` são criados na primeira leitura e devolvidos sempre iguais (`m_algorithm`/`m_usages`).
fn cached_key_value(global_object: &JSGlobalObject, call: &HostCall, attribute: &str, build: fn(&JSGlobalObject, &KeyState) -> JSValue) -> HostResult {
    let (this, state) = key_accessor_this(global_object, call, attribute)?;
    let cached = if attribute == "algorithm" { state.algorithm } else { state.usages_value };
    if let Some(cached) = cached {
        return Ok(JSValue::decode(cached));
    }
    let value = build(global_object, &state);
    KEYS.with(|keys| {
        if let Some(stored) = keys.borrow_mut().get_mut(&this.encode()) {
            if attribute == "algorithm" {
                stored.algorithm = Some(value.encode());
            } else {
                stored.usages_value = Some(value.encode());
            }
        }
    });
    Ok(value)
}

fn key_algorithm_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    cached_key_value(global_object, call, "algorithm", algorithm_value)
}

fn key_usages_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    cached_key_value(global_object, call, "usages", |global_object, key| usages_array(global_object, key.usages))
}

fn digest_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let shared = JSArrayBuffer::from_value(&call.argument(1)).is_some_and(|buffer| buffer.impl_().is_shared());
    let data = if shared { None } else { input_bytes(call.argument(1)) };
    let Some(data) = data else {
        let received = describe_received(global_object, call.argument(1)).unwrap_or_else(|| "undefined".to_owned());
        let message = format!("The \"data\" argument must be of type ArrayBuffer, Buffer, TypedArray, or DataView. Received {received}");
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    };
    let Some(algo) = digest_algo(&algorithm_name(global_object, call.argument(0))?) else {
        return Err(not_supported(global_object, call));
    };
    Ok(array_buffer_value(global_object, &algo.digest(&data)))
}

/// O caminho de cada método (medido no bun 1.4.2), na ordem em que o bun valida.
fn subtle_body(global_object: &JSGlobalObject, call: &HostCall, name: &str) -> HostResult {
    match name {
        "digest" => digest_body_impl(global_object, call),
        "sign" => sign_verify_body_impl(global_object, call, false),
        "verify" => sign_verify_body_impl(global_object, call, true),
        "generateKey" => generate_key_body_impl(global_object, call),
        "importKey" => import_key_body_impl(global_object, call),
        "exportKey" => export_key_body_impl(global_object, call),
        "encrypt" => crypt_body_impl(global_object, call, false),
        "decrypt" => crypt_body_impl(global_object, call, true),
        "deriveBits" => derive_bits_body_impl(global_object, call),
        "deriveKey" => derive_key_body_impl(global_object, call),
        "getPublicKey" => get_public_key_body_impl(global_object, call),
        "encapsulateBits" => encapsulate_bits_body_impl(global_object, call),
        "encapsulateKey" => encapsulate_key_body_impl(global_object, call),
        "decapsulateBits" => decapsulate_bits_body_impl(global_object, call),
        "decapsulateKey" => decapsulate_key_body_impl(global_object, call),
        "wrapKey" => wrap_key_body_impl(global_object, call),
        // `SUBTLE_METHODS` é fechado: o único nome que sobra é `unwrapKey`.
        _ => unwrap_key_body_impl(global_object, call),
    }
}

/// Os operandos de um dos quatro métodos de encapsulamento, já validados.
struct KemCall {
    alg: crypto_pq::PqAlgorithm,
    key: KeyState,
    /// O `ciphertext` (argumento 2) dos métodos de decapsulamento; vazio nos de encapsulamento.
    ciphertext: Vec<u8>,
    /// Os usos da chave `secret` que sai dos métodos `...Key`.
    usages: u16,
    /// O `name` do `sharedKeyAlgorithm` dos métodos `...Key`, lido uma única vez; vazio nos `...Bits`.
    shared_name: String,
}

/// O `sharedKeyAlgorithm` dos métodos `...Key`: normalizado como o do `importKey`, com os membros obrigatórios dos
/// dicionários de curva e de HMAC (medido: isso vem antes de qualquer checagem da chave e do `ciphertext`). Devolve o
/// `name` já lido, para o `importKey` do segredo não rodar o getter outra vez.
fn check_shared_algorithm(global_object: &JSGlobalObject, call: &HostCall, value: JSValue) -> Result<String, Thrown> {
    let lower_name = algorithm_name(global_object, value)?;
    let id = normalize_named(global_object, call, &lower_name, Operation::ImportKey)?;
    if matches!(id, AlgorithmId::Ecdsa | AlgorithmId::Ecdh) {
        required_member(global_object, value, "EcKeyParams", "namedCurve", "DOMString")?;
    }
    if id == AlgorithmId::Hmac {
        hmac_params(global_object, call, value)?;
    }
    Ok(lower_name)
}

/// Lê e valida os argumentos de um método de encapsulamento, na ordem medida no bun. Nos `...Bits`: nome do algoritmo,
/// chave, `ciphertext`. Nos `...Key`, a conversão do WebIDL (chave, `ciphertext`, lista de usos) vem antes de o nome do
/// algoritmo e o `sharedKeyAlgorithm` serem normalizados. Em todos, o tipo e o tamanho da chave vêm antes do uso dela.
fn kem_call(global_object: &JSGlobalObject, call: &HostCall, name: &str) -> Result<KemCall, Thrown> {
    let lower_name = algorithm_name(global_object, call.argument(0))?;
    let decapsulate = name.starts_with("decapsulate");
    let wraps_key = name.ends_with("Key");
    let resolve_alg = || {
        normalize_named(global_object, call, &lower_name, Operation::Encapsulate)?;
        crypto_pq::PqAlgorithm::from_lower_name(&lower_name).ok_or_else(|| not_supported(global_object, call))
    };
    // O golden do bun (`encapsulateBits(1, 1)`) mostra a chave sendo validada antes de o algoritmo ser normalizado também nos
    // `...Bits`; só o nome é lido antes.
    let mut early = None;
    let argument = if decapsulate { "decapsulationKey" } else { "encapsulationKey" };
    let key = key_at(global_object, call, name, 1, 2, argument)?;
    let ciphertext = if decapsulate { buffer_source(global_object, call.argument(2))? } else { Vec::new() };
    let mut usages = 0;
    let mut shared_name = String::new();
    if wraps_key {
        let shared_at = if decapsulate { 3 } else { 2 };
        usages = check_usages(global_object, call.argument(shared_at + 2))?;
        early = Some(resolve_alg()?);
        shared_name = check_shared_algorithm(global_object, call, call.argument(shared_at))?;
    }
    let alg = match early {
        Some(alg) => alg,
        None => resolve_alg()?,
    };
    check_kem_key(global_object, call, name, alg, &key)?;
    Ok(KemCall { alg, key, ciphertext, usages, shared_name })
}

/// A chave tem de ser ML-KEM do mesmo parâmetro (`key algorithm mismatch`, medido antes do uso) e ter o uso do método
/// (`encapsulationKey does not have encapsulateBits usage`).
fn check_kem_key(global_object: &JSGlobalObject, call: &HostCall, name: &str, alg: crypto_pq::PqAlgorithm, key: &KeyState) -> Result<Pq, Thrown> {
    let mismatch = || dom_error(global_object, call, "InvalidAccessError", "key algorithm mismatch");
    let Some(pq) = key.pq.filter(|pq| pq.alg == alg) else { return Err(mismatch()) };
    let (argument, usage) = match name {
        "encapsulateBits" => ("encapsulationKey", crypto_pq::USAGE_ENCAPSULATE_BITS),
        "encapsulateKey" => ("encapsulationKey", crypto_pq::USAGE_ENCAPSULATE_KEY),
        "decapsulateBits" => ("decapsulationKey", crypto_pq::USAGE_DECAPSULATE_BITS),
        _ => ("decapsulationKey", crypto_pq::USAGE_DECAPSULATE_KEY),
    };
    if key.usages & usage == 0 {
        return Err(dom_error(global_object, call, "InvalidAccessError", &format!("{argument} does not have {name} usage")));
    }
    Ok(pq)
}

/// Objeto simples com as propriedades na ordem dada (`sharedKey`, `ciphertext` ou o inverso).
fn kem_result_object(global_object: &JSGlobalObject, entries: [(&str, JSValue); 2]) -> JSValue {
    let vm = global_object.vm();
    let object = construct_empty_object(global_object);
    for (name, value) in entries {
        object.put_direct(vm, &property_key(vm, name), value, 0);
    }
    object.as_value()
}

/// Encapsula para a chave pública da chamada: `(ciphertext, segredo, usos da chave de saída, nome do algoritmo dela)`.
fn encapsulate_for(global_object: &JSGlobalObject, call: &HostCall, name: &str) -> Result<(Vec<u8>, Vec<u8>, u16, String), Thrown> {
    let operands = kem_call(global_object, call, name)?;
    let (ciphertext, shared) = operands.alg.encapsulate(&operands.key.secret).ok_or_else(|| operation_error(global_object, call, OPERATION_FAILED))?;
    Ok((ciphertext, shared, operands.usages, operands.shared_name))
}

/// Decapsula o `ciphertext` (argumento 2) com a chave privada da chamada: `(segredo, usos da chave de saída, nome do
/// algoritmo dela)`.
fn decapsulate_for(global_object: &JSGlobalObject, call: &HostCall, name: &str) -> Result<(Vec<u8>, u16, String), Thrown> {
    let operands = kem_call(global_object, call, name)?;
    let shared = operands.alg.decapsulate(&operands.key.secret, &operands.ciphertext).ok_or_else(|| operation_error(global_object, call, OPERATION_FAILED))?;
    Ok((shared, operands.usages, operands.shared_name))
}

/// O segredo de 32 bytes como chave `secret`: o mesmo caminho de `importKey('raw-secret')`, com o `name` já lido; o
/// `length` pedido é ignorado.
fn shared_secret_key(global_object: &JSGlobalObject, call: &HostCall, shared: Vec<u8>, usages: u16, shared_name: &str, first_argument: usize) -> HostResult {
    let algorithm = call.argument(first_argument);
    // Medido no bun 1.4.2: o algoritmo da chave de saída que não é de chave secreta (ECDSA, RSA, ML-KEM...) é
    // `NotSupportedError`, antes de qualquer checagem de usos.
    use AlgorithmId::*;
    if !resolve_algorithm(shared_name, Operation::ImportKey).is_ok_and(|id| matches!(id, AesCtr | AesCbc | AesGcm | AesCfb | AesKw | Hmac | Hkdf | Pbkdf2 | ChaCha20Poly1305)) {
        return Err(dom_error(global_object, call, "NotSupportedError", NOT_SUPPORTED));
    }
    let extractable = call.argument(first_argument + 1).to_boolean();
    let key = import_named(global_object, call, shared_name, "raw-secret", Some(shared), None, algorithm, extractable, usages)?;
    Ok(create_key(global_object, key))
}

fn encapsulate_bits_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (ciphertext, shared, _, _) = encapsulate_for(global_object, call, "encapsulateBits")?;
    let entries = [("sharedKey", array_buffer_value(global_object, &shared)), ("ciphertext", array_buffer_value(global_object, &ciphertext))];
    Ok(kem_result_object(global_object, entries))
}

fn encapsulate_key_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (ciphertext, shared, usages, shared_name) = encapsulate_for(global_object, call, "encapsulateKey")?;
    let shared_key = shared_secret_key(global_object, call, shared, usages, &shared_name, 2)?;
    Ok(kem_result_object(global_object, [("ciphertext", array_buffer_value(global_object, &ciphertext)), ("sharedKey", shared_key)]))
}

fn decapsulate_bits_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(array_buffer_value(global_object, &decapsulate_for(global_object, call, "decapsulateBits")?.0))
}

fn decapsulate_key_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let (shared, usages, shared_name) = decapsulate_for(global_object, call, "decapsulateKey")?;
    shared_secret_key(global_object, call, shared, usages, &shared_name, 3)
}

/// Os conversores de parâmetros lançam; no `supports` o lançamento vira `false`: a exceção pendente some e só a falha do
/// próprio motor propaga.
fn succeeds<T>(global_object: &JSGlobalObject, result: Result<T, Thrown>) -> Result<bool, Thrown> {
    // Medido no bun 1.4.2: `valueOf` que lança, símbolo e `BigInt` num membro numérico do dicionário valem `false`. Uma
    // conversão que lança pode devolver `Ok` com a exceção pendente.
    match result {
        Ok(_) if global_object.vm().exception().is_none() => Ok(true),
        Ok(_) | Err(Thrown::Pending | Thrown::TypeError(_) | Thrown::RangeError(_)) => {
            global_object.vm().clear_exception();
            Ok(false)
        }
        Err(other) => Err(other),
    }
}

/// Para que a operação do `supports` lê o dicionário de parâmetros do algoritmo.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ParamUse {
    Generate,
    Import,
    SignVerify,
    Encrypt,
    Derive,
}

/// Os membros obrigatórios do dicionário `algorithm` para `usage`, com os mesmos conversores das operações reais
/// (presença e tipo, nunca o valor: `AES-GCM` com `length: 100` e `ECDSA` com `P-1` valem no `generateKey`).
fn supports_params(global_object: &JSGlobalObject, call: &HostCall, id: AlgorithmId, algorithm: JSValue, usage: ParamUse) -> Result<bool, Thrown> {
    use AlgorithmId::*;
    let present = |dictionary: &str, name: &str| succeeds(global_object, required_member(global_object, algorithm, dictionary, name, "any"));
    let hashed = |dictionary: &str| succeeds(global_object, rsa_hash(global_object, call, algorithm, dictionary));
    match (usage, id) {
        (ParamUse::Generate, AesCtr | AesCbc | AesGcm | AesCfb | AesKw) => succeeds(global_object, aes_key_length(global_object, algorithm)),
        (ParamUse::Generate, Hmac) => match hmac_params(global_object, call, algorithm) {
            Ok((_, length)) => Ok(length != Some(0)),
            Err(error) => succeeds(global_object, Err::<(), _>(error)),
        },
        (ParamUse::Generate, Ecdsa | Ecdh) => present("EcKeyParams", "namedCurve"),
        (ParamUse::Generate, RsassaPkcs1V15 | RsaPss | RsaOaep) => {
            // Os mesmos conversores do `generateKey` (`unsigned long` com EnforceRange e `Uint8Array`), sem olhar o valor: módulo 0 ou
            // expoente vazio valem; `-1`, `NaN`, `{}` e expoente que não é `Uint8Array` não.
            let modulus = required_member(global_object, algorithm, "RsaHashedKeyGenParams", "modulusLength", "unsigned long").and_then(|value| enforce_range(global_object, value, u32::MAX));
            if !succeeds(global_object, modulus)? {
                return Ok(false);
            }
            let exponent = required_member(global_object, algorithm, "RsaHashedKeyGenParams", "publicExponent", "Uint8Array").and_then(|value| uint8_array_bytes(global_object, value));
            Ok(succeeds(global_object, exponent)? && hashed("RsaHashedKeyGenParams")?)
        }
        (ParamUse::Import, RsassaPkcs1V15 | RsaPss | RsaOaep) => hashed("RsaHashedImportParams"),
        (ParamUse::Import, Ecdsa | Ecdh) => present("EcKeyImportParams", "namedCurve"),
        (ParamUse::Import, Hmac) => succeeds(global_object, hmac_params(global_object, call, algorithm)),
        (ParamUse::SignVerify, RsaPss) => present("RsaPssParams", "saltLength"),
        (ParamUse::SignVerify, Ecdsa) => succeeds(global_object, ecdsa_hash(global_object, call, algorithm)),
        (ParamUse::SignVerify, MlDsa) => {
            let context = dictionary_member(global_object, algorithm, "context");
            Ok(context.is_undefined() || succeeds(global_object, buffer_source(global_object, context))?)
        }
        (ParamUse::Encrypt, _) => succeeds(global_object, aes_params(global_object, id, algorithm)),
        (ParamUse::Derive, Hkdf) => Ok(hashed("HkdfParams")? && present("HkdfParams", "salt")? && present("HkdfParams", "info")?),
        (ParamUse::Derive, Pbkdf2) => Ok(hashed("Pbkdf2Params")? && present("Pbkdf2Params", "salt")? && present("Pbkdf2Params", "iterations")?),
        _ => Ok(true),
    }
}

/// O algoritmo `extra` (3º argumento) como chave alvo: do `deriveKey` (AES com `length` de 128, 192 ou 256, ou HMAC com `hash`) e
/// do `encapsulateKey`/`decapsulateKey` (qualquer AES, HMAC com `hash`, `HKDF`, `PBKDF2`; o AES não valida o `length`).
fn supports_target_key(global_object: &JSGlobalObject, call: &HostCall, extra: JSValue, derive: bool) -> Result<bool, Thrown> {
    use AlgorithmId::*;
    // Um `name` ilegível (getter que lança) no dicionário é `false`, como no algoritmo principal.
    let name = match algorithm_name_core(global_object, extra) {
        Err(Thrown::Pending) if JSObject::from_value(&extra).is_some() => None,
        other => other?,
    };
    if global_object.vm().exception().is_some() && JSObject::from_value(&extra).is_some() {
        global_object.vm().clear_exception();
        return Ok(false);
    }
    let Some(name) = name else {
        global_object.vm().clear_exception();
        return Ok(false);
    };
    let Ok(id) = resolve_algorithm(&name, Operation::ImportKey) else { return Ok(false) };
    match id {
        AesCtr | AesCbc | AesGcm | AesCfb | AesKw if derive => {
            let length = dictionary_member(global_object, extra, "length");
            Ok(length.is_number() && [128.0, 192.0, 256.0].contains(&length.as_number()))
        }
        AesCtr | AesCbc | AesGcm | AesCfb | AesKw => Ok(true),
        Hmac => succeeds(global_object, hmac_params(global_object, call, extra)),
        Hkdf | Pbkdf2 => Ok(!derive),
        // O ChaCha20-Poly1305 não tem `length`: vale como alvo nas três operações (medido no bun 1.4.2).
        ChaCha20Poly1305 => Ok(true),
        _ => Ok(false),
    }
}

/// O `deriveBits`/`deriveKey` do `supports` converte o comprimento como `unsigned long` sem EnforceRange (medido no bun 1.4.2: módulo
/// 2^32 depois do truncamento, não finito vira 0, `-8` e `1e10` valem, `2**32` e `0` não) e exige resultado positivo e múltiplo de 8.
/// A conversão `ToNumber` propaga a exceção (símbolo, `BigInt`, `valueOf` que lança).
fn supports_derive_length(global_object: &JSGlobalObject, value: JSValue) -> Result<bool, Thrown> {
    let number = crate::runtime::intl_support::to_number_checked(global_object, value)?;
    let bits = if number.is_finite() { number.trunc().rem_euclid(4_294_967_296.0) } else { 0.0 };
    Ok(bits > 0.0 && bits % 8.0 == 0.0)
}

/// A tabela de `SubtleCrypto.supports` (medida no bun 1.4.2): cada operação resolve o algoritmo com o mesmo
/// [`resolve_algorithm`] das operações reais, e falha de normalização é `false`. `get key length` dá `false` em toda
/// combinação medida.
fn supports_operation(global_object: &JSGlobalObject, call: &HostCall, operation: &str, lower_name: &str, algorithm: JSValue) -> Result<bool, Thrown> {
    use AlgorithmId::*;
    let extra = call.argument(2);
    let resolved = |kind: Operation| resolve_algorithm(lower_name, kind).ok();
    let is_asymmetric = |id: AlgorithmId| matches!(id, RsassaPkcs1V15 | RsaPss | RsaOaep | Ecdsa | Ecdh | Ed25519 | X25519 | MlKem | MlDsa);
    match operation {
        "generateKey" => resolved(Operation::GenerateKey).map_or(Ok(false), |id| supports_params(global_object, call, id, algorithm, ParamUse::Generate)),
        "importKey" => resolved(Operation::ImportKey).map_or(Ok(false), |id| supports_params(global_object, call, id, algorithm, ParamUse::Import)),
        "exportKey" => resolved(Operation::GenerateKey).map_or(Ok(false), |id| supports_params(global_object, call, id, algorithm, ParamUse::Import)),
        "getPublicKey" => resolved(Operation::GenerateKey)
            .filter(|id| is_asymmetric(*id))
            .map_or(Ok(false), |id| supports_params(global_object, call, id, algorithm, ParamUse::Import)),
        "sign" | "verify" => resolved(Operation::SignVerify).map_or(Ok(false), |id| supports_params(global_object, call, id, algorithm, ParamUse::SignVerify)),
        "digest" => Ok(algorithm_id(lower_name) == Some(Sha)),
        "encrypt" | "decrypt" => resolved(Operation::Encrypt).map_or(Ok(false), |id| supports_params(global_object, call, id, algorithm, ParamUse::Encrypt)),
        "deriveBits" | "deriveKey" => {
            let Some(id) = resolved(Operation::Derive).filter(|id| matches!(id, Hkdf | Pbkdf2)) else { return Ok(false) };
            if !supports_params(global_object, call, id, algorithm, ParamUse::Derive)? {
                return Ok(false);
            }
            if operation == "deriveBits" { supports_derive_length(global_object, extra) } else { supports_target_key(global_object, call, extra, true) }
        }
        // Medido no bun 1.4.2: só `AES-KW` e `RSA-OAEP` embrulham.
        "wrapKey" | "unwrapKey" => {
            if !resolved(Operation::WrapKey).is_some_and(|id| matches!(id, AesKw | RsaOaep)) {
                return Ok(false);
            }
            // O 3º argumento é a chave embrulhada: `wrapKey` a exporta (sem HKDF/PBKDF2) e `unwrapKey` a importa, ambos
            // com os parâmetros do algoritmo.
            let Some(name) = algorithm_name_core(global_object, extra)? else { return Ok(false) };
            let kind = if operation == "wrapKey" { Operation::GenerateKey } else { Operation::ImportKey };
            resolve_algorithm(&name, kind).map_or(Ok(false), |id| supports_params(global_object, call, id, extra, ParamUse::Import))
        }
        "encapsulateBits" | "decapsulateBits" => Ok(resolved(Operation::Encapsulate).is_some()),
        "encapsulateKey" | "decapsulateKey" => {
            if resolved(Operation::Encapsulate).is_none() {
                return Ok(false);
            }
            supports_target_key(global_object, call, extra, false)
        }
        _ => Ok(false),
    }
}

/// `SubtleCrypto.supports(operation, algorithm[, extra])`: estático e síncrono, devolve booleano. Faltando argumento lança
/// `ERR_MISSING_ARGS`; `ToString` de símbolo no 1º ou 2º argumento lança o `TypeError` do motor; o resto é `false`.
fn supports_body_impl(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // Medido no bun: o `this` é conferido antes dos argumentos e só o próprio construtor `SubtleCrypto` serve.
    let this_is_constructor = SUBTLE_CONSTRUCTORS.with(|constructors| constructors.borrow().iter().any(|(_, constructor)| *constructor == call.this_value().encode()));
    if !this_is_constructor {
        return Err(throw_coded_type_error(global_object, "Value of \"this\" must be of type SubtleCrypto constructor", "ERR_INVALID_THIS"));
    }
    if call.argument_count() < 2 {
        return Err(throw_coded_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    let operation = string_of(global_object, call.argument(0))?;
    // A conversão que lança deixa a exceção pendente (e um texto vazio): nada mais roda, como no bun (`toString` do
    // primeiro argumento antes de qualquer leitura do dicionário).
    if global_object.vm().exception().is_some() {
        return Err(Thrown::Pending);
    }
    let algorithm = call.argument(1);
    // `ToString` do próprio `algorithm` (símbolo) lança; um `name` ilegível dentro do dicionário (símbolo, getter que lança) é `false`.
    let name = match algorithm_name_core(global_object, algorithm) {
        Err(Thrown::Pending) if JSObject::from_value(&algorithm).is_some() => None,
        other => other?,
    };
    let Some(lower_name) = name else {
        global_object.vm().clear_exception();
        return Ok(js_boolean(false));
    };
    Ok(js_boolean(supports_operation(global_object, call, &operation, &lower_name, algorithm)?))
}

host_function!(subtle_supports, supports_body_impl);

/// Um método de `SubtleCrypto`: sempre promessa; `this` alheio e argumentos a menos rejeitam, e o erro que o bun lança
/// no corpo vira a rejeição da promessa.
fn subtle_method(global_object: &JSGlobalObject, call: &HostCall, name: &str, length: usize) -> HostResult {
    if !is_subtle(call.this_value()) {
        let message = format!("Can only call SubtleCrypto.{name} on instances of SubtleCrypto");
        return Ok(rejected_type_error(global_object, &message, "ERR_INVALID_THIS"));
    }
    if call.argument_count() < length {
        return Ok(rejected_type_error(global_object, "Not enough arguments", "ERR_MISSING_ARGS"));
    }
    match subtle_body(global_object, call, name) {
        Ok(value) => Ok(resolved_promise(global_object, value)),
        Err(Thrown::Pending) => {
            let vm = global_object.vm();
            let error = vm.exception().map_or_else(JSValue::undefined, |exception| exception.value());
            vm.clear_exception();
            Ok(JSPromise::rejected_promise(global_object, error).as_value())
        }
        Err(other) => Err(other),
    }
}

macro_rules! subtle_methods {
    ($(($body:ident, $host:ident, $name:literal, $length:literal)),* $(,)?) => {
        $(
            fn $body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                subtle_method(global_object, call, $name, $length)
            }
            host_function!($host, $body);
        )*
        const SUBTLE_METHODS: &[(&str, u32, NativeFunction)] = &[$(($name, $length, $host as NativeFunction)),*];
    };
}

subtle_methods!(
    (encrypt_body, subtle_encrypt, "encrypt", 3),
    (decrypt_body, subtle_decrypt, "decrypt", 3),
    (sign_body, subtle_sign, "sign", 3),
    (verify_body, subtle_verify, "verify", 4),
    (digest_body, subtle_digest, "digest", 2),
    (generate_key_body, subtle_generate_key, "generateKey", 3),
    (derive_key_body, subtle_derive_key, "deriveKey", 5),
    (derive_bits_body, subtle_derive_bits, "deriveBits", 2),
    (import_key_body, subtle_import_key, "importKey", 5),
    (export_key_body, subtle_export_key, "exportKey", 2),
    (wrap_key_body, subtle_wrap_key, "wrapKey", 4),
    (unwrap_key_body, subtle_unwrap_key, "unwrapKey", 7),
    (get_public_key_body, subtle_get_public_key, "getPublicKey", 2),
    (encapsulate_bits_body, subtle_encapsulate_bits, "encapsulateBits", 2),
    (encapsulate_key_body, subtle_encapsulate_key, "encapsulateKey", 5),
    (decapsulate_bits_body, subtle_decapsulate_bits, "decapsulateBits", 3),
    (decapsulate_key_body, subtle_decapsulate_key, "decapsulateKey", 6),
);

host_function!(call_crypto, crypto_call_body);
host_function!(construct_illegal, illegal_constructor_body);
host_function!(crypto_get_random_values, get_random_values_body);
host_function!(crypto_random_uuid, random_uuid_body);
host_function!(crypto_timing_safe_equal, timing_safe_equal_body);
host_function!(pub timing_safe_equal_function, timing_safe_equal_core);
host_function!(crypto_subtle_getter, subtle_getter_body);
host_function!(crypto_subtle_setter, subtle_setter_body);
/// `[Symbol.for('nodejs.util.inspect.custom')](depth, options)` do `CryptoKey` (medido no bun 1.4.2): fora de uma chave,
/// ou com `depth` negativo, devolve o próprio `this`; `options.depth` zero ou menos dá `CryptoKey [Object]`; senão
/// `CryptoKey { type, extractable, algorithm, usages }` no formato do `util.inspect`.
fn key_inspect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let this = call.this_value();
    let depth = call.argument(0);
    if !KEYS.with(|keys| keys.borrow().contains_key(&this.encode())) || (depth.is_number() && depth.as_number() < 0.0) {
        return Ok(this);
    }
    let text = match InspectOptions::from_options(global_object, call.argument(1))? {
        None => "CryptoKey [Object]".to_owned(),
        Some(options) => {
            let get = |name: &str| crate::runtime::intl_support::get_property(global_object, this, name);
            let fields = [("type", get("type")?), ("extractable", get("extractable")?), ("algorithm", get("algorithm")?), ("usages", get("usages")?)];
            inspect_named_fields(global_object, "CryptoKey", &fields, &options)?
        }
    };
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf16(&text.encode_utf16().collect::<Vec<u16>>()))))
}

host_function!(key_inspect, key_inspect_body);
host_function!(key_type_getter, key_type_body);
host_function!(key_extractable_getter, key_extractable_body);
host_function!(key_algorithm_getter, key_algorithm_body);
host_function!(key_usages_getter, key_usages_body);

/// Instala `crypto`, `Crypto` e `SubtleCrypto` no global; a posição vem da tabela `ORDER`.
pub fn install_crypto(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let constructor_key = crate::runtime::property_name::PropertyName::from_identifier(&vm.property_names.constructor);

    let (subtle_prototype, subtle_constructor) =
        create_native_class(global_object, &SUBTLE_PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "SubtleCrypto", construct_illegal, construct_illegal);
    subtle_prototype.put_direct(vm, &constructor_key, subtle_constructor.as_value(), DONT_ENUM);
    crate::runtime::event_target::put_methods(global_object, &subtle_prototype, SUBTLE_METHODS);
    put_to_string_tag(vm, &subtle_prototype, "SubtleCrypto");
    // A estática `supports` vem depois de `length`, `name` e `prototype`: `length` 2, gravável, enumerável e configurável.
    if let Some(constructor_object) = JSObject::from_value(&subtle_constructor.as_value()) {
        crate::runtime::event_target::put_methods(global_object, &constructor_object, &[("supports", 2, subtle_supports as NativeFunction)]);
    }
    SUBTLE_CONSTRUCTORS.with(|constructors| constructors.borrow_mut().push((global_object.cell_id(), subtle_constructor.as_value().encode())));
    install_global(global_object, "SubtleCrypto", subtle_constructor.as_value());

    let (key_prototype, key_constructor) =
        create_native_class(global_object, &KEY_PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "CryptoKey", construct_illegal, construct_illegal);
    key_prototype.put_direct(vm, &constructor_key, key_constructor.as_value(), DONT_ENUM);
    let getters: [(&str, NativeFunction); 4] =
        [("type", key_type_getter), ("extractable", key_extractable_getter), ("algorithm", key_algorithm_getter), ("usages", key_usages_getter)];
    for (name, getter) in getters {
        put_native_accessor(vm, global_object, &key_prototype, name, getter, None, 0);
    }
    put_to_string_tag(vm, &key_prototype, "CryptoKey");
    crate::runtime::streams::put_inspect_custom_with(global_object, &key_prototype, key_inspect);
    install_global(global_object, "CryptoKey", key_constructor.as_value());
    KEY_PROTOTYPES.with(|prototypes| prototypes.borrow_mut().push((global_object.cell_id(), key_prototype.as_value().encode())));

    let (prototype, constructor) =
        create_native_class(global_object, &CRYPTO_PROTOTYPE_S_INFO, &CONSTRUCTOR_S_INFO, "Crypto", call_crypto, construct_illegal);
    let methods: [(&str, u32, NativeFunction); 3] =
        [("getRandomValues", 0, crypto_get_random_values), ("randomUUID", 1, crypto_random_uuid), ("timingSafeEqual", 2, crypto_timing_safe_equal)];
    for (name, length, function) in methods {
        put_direct_native_function_without_transition(
            vm,
            global_object,
            &prototype,
            &Identifier::from_span(vm, name.as_bytes()),
            length,
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            DONT_DELETE,
        );
    }
    prototype.put_direct(vm, &constructor_key, constructor.as_value(), DONT_ENUM);
    put_native_accessor(vm, global_object, &prototype, "subtle", crypto_subtle_getter, Some(crypto_subtle_setter), DONT_DELETE);
    put_to_string_tag(vm, &prototype, "Crypto");
    install_global(global_object, "Crypto", constructor.as_value());

    let subtle = JSFinalObject::create(vm, &instance_structure(vm, Some(global_object), subtle_prototype.as_value())).as_value();
    let crypto = JSFinalObject::create(vm, &instance_structure(vm, Some(global_object), prototype.as_value())).as_value();
    INSTANCES.with(|instances| instances.borrow_mut().push((global_object.cell_id(), crypto.encode(), subtle.encode())));
    install_global(global_object, "crypto", crypto);
}
