//! Porte de `src/vs/base/common/normalization.ts`: `String.prototype.normalize` (NFC e NFD) com cache, e
//! a normalização "para a base" (sem acentos, em minúsculas) que as buscas usam.
//!
//! A normalização é a do ICU4X (`icu_normalizer`, dados compilados do Unicode), a mesma que o V8 tem no
//! ICU. Texto é `&[u16]`; um surrogate isolado passa intacto, como no `normalize` do JavaScript (ele é um
//! caractere de classe de combinação 0 e não se compõe com nada, então quebrar o texto nele não muda o
//! resultado).

use icu_normalizer::{ComposingNormalizer, DecomposingNormalizer};

use crate::JsString;
use crate::map::LRUCache;

const CACHE_LIMIT: usize = 10_000; // limitado a 10000 elementos

thread_local! {
    static NFC_CACHE: LRUCache<JsString, JsString> = LRUCache::new(CACHE_LIMIT, 1.0);
    static NFD_CACHE: LRUCache<JsString, JsString> = LRUCache::new(CACHE_LIMIT, 1.0);
    static BASE_CACHE: LRUCache<JsString, JsString> = LRUCache::new(CACHE_LIMIT, 1.0);
}

/// `normalizeNFC`.
pub fn normalize_nfc(str: &[u16]) -> JsString {
    normalize(str, &NFC_CACHE, |text| ComposingNormalizer::new_nfc().normalize(text).into_owned())
}

/// `normalizeNFD`.
pub fn normalize_nfd(str: &[u16]) -> JsString {
    normalize(str, &NFD_CACHE, |text| DecomposingNormalizer::new_nfd().normalize(text).into_owned())
}

/// `/[^\u0000-\u0080]/.test(str)`.
fn has_non_ascii_characters(str: &[u16]) -> bool {
    str.iter().any(|&unit| unit > 0x80)
}

fn normalize(
    str: &[u16],
    cache: &'static std::thread::LocalKey<LRUCache<JsString, JsString>>,
    form: impl Fn(&str) -> String,
) -> JsString {
    if str.is_empty() {
        return JsString::from(str);
    }

    let key = JsString::from(str);
    if let Some(cached) = cache.with(|cache| cache.get(&key)).filter(|cached| !cached.is_empty()) {
        return cached;
    }

    let res = if has_non_ascii_characters(str) { normalize_units(str, form) } else { key.clone() };

    // Usa o cache para consultas rápidas
    cache.with(|cache| {
        cache.set(key, res.clone());
    });

    res
}

/// Normaliza os trechos bem formados e deixa passar os surrogates isolados.
fn normalize_units(units: &[u16], form: impl Fn(&str) -> String) -> JsString {
    let mut out = Vec::with_capacity(units.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut Vec<u16>| {
        out.extend(form(run).encode_utf16());
        run.clear();
    };
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(c) => run.push(c),
            Err(lone) => {
                flush(&mut run, &mut out);
                out.push(lone.unpaired_surrogate());
            }
        }
    }
    flush(&mut run, &mut out);
    JsString::from_units(out)
}

/// Tenta normalizar o texto para o formato base do Unicode (NFD, tira os acentos, minúsculas). Quando o
/// original já traz acentos como caracteres combinantes soltos, só põe em minúsculas, para o comprimento
/// continuar igual e os índices não mudarem.
pub fn try_normalize_to_base(str: &[u16]) -> JsString {
    let key = JsString::from(str);
    if let Some(cached) = BASE_CACHE.with(|cache| cache.get(&key)).filter(|cached| !cached.is_empty()) {
        return cached;
    }

    let decomposed = normalize_nfd(str);
    let no_accents: Vec<u16> = decomposed.units().iter().copied().filter(|unit| !(0x0300..=0x036F).contains(unit)).collect();
    let result = if no_accents.len() == str.len() { JsString::from_units(no_accents) } else { key.clone() }.to_lower_case();
    BASE_CACHE.with(|cache| {
        cache.set(key, result.clone());
    });
    result
}
