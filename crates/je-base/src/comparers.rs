//! Porte de `src/vs/base/common/comparers.ts`: comparadores de nomes de arquivo e de caminho do
//! explorador, dos editores abertos, da busca e do quick open.
//!
//! A ordem de `compareFileNames` e companhia vem do `Intl.Collator` com `numeric: true`, então o
//! resultado exato (acentos, caixa, `file2` contra `file10`, pontuação) é o do ICU, e aqui é o do
//! `icu_collator` do ICU4X pelo módulo [`crate::collator`], que descreve o mapeamento das opções e de onde
//! vem a locale (`en` por padrão, trocada por [`crate::collator::set_default_locale`]). Os três colatores do
//! arquivo original e o `localeCompare` do [`compare_anything`] são criados por locale, na primeira
//! chamada, e reaproveitados. Diferente do original, que congela a locale na primeira chamada do processo,
//! trocar a locale padrão vale para as chamadas seguintes.
//!
//! Texto é `&[u16]` (unidades UTF-16, como `a < b` do JavaScript), e `string | null` vira `Option`.
//!
//! Diferenças do original:
//! - `compareFileNames` tem um parâmetro `caseSensitive` que nunca é lido; o porte não o tem, e
//!   [`compare_paths`] o usa só no seu laço de componentes, como o original.
//! - `startsWithLower`/`startsWithUpper` usam `toLocaleUpperCase`/`toLocaleLowerCase`, que seguem a locale
//!   do hospedeiro (`i` vira `İ` em `tr` e `az`); aqui valem sempre as regras de caixa do Unicode sem
//!   tailoring de locale, o mesmo que `toUpperCase`.
//! - `extractNameAndExtension` perde o parâmetro `dotfilesAsNames`, que o arquivo nunca liga.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::JsString;
use crate::collator::{Collator, CollatorOptions, Sensitivity, default_locale};
use crate::platform::Platform;

/// Os colatores de uma locale, os três do `comparers.ts` mais o do `localeCompare`.
struct Collators {
    /// Numérico, sem sensibilidade a caixa, acentos nem diacríticos.
    base_numeric: Collator,
    /// Numérico.
    numeric: Collator,
    /// Numérico, sensível a acentos e diacríticos mas não à caixa.
    numeric_case_insensitive: Collator,
    /// `a.localeCompare(b)`, o colador padrão.
    locale: Collator,
}

fn collators() -> Arc<Collators> {
    static CACHE: Mutex<Option<HashMap<String, Arc<Collators>>>> = Mutex::new(None);
    let tag = default_locale();
    let mut cache = CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    cache
        .get_or_insert_with(HashMap::new)
        .entry(tag.clone())
        .or_insert_with(|| {
            let numeric = |sensitivity| CollatorOptions { numeric: true, sensitivity, ..CollatorOptions::default() };
            Arc::new(Collators {
                base_numeric: Collator::new(Some(&tag), numeric(Sensitivity::Base)),
                numeric: Collator::new(Some(&tag), numeric(Sensitivity::Variant)),
                numeric_case_insensitive: Collator::new(Some(&tag), numeric(Sensitivity::Accent)),
                locale: Collator::new(Some(&tag), CollatorOptions::default()),
            })
        })
        .clone()
}

/// `a < b ? -1 : 1` do JavaScript sobre strings: ordem lexicográfica por unidade UTF-16.
fn unicode_order(a: &[u16], b: &[u16]) -> i32 {
    if a < b { -1 } else { 1 }
}

/// Compara nomes de arquivo sem distinguir o nome da extensão. Desempata por comparação Unicode.
pub fn compare_file_names(one: Option<&[u16]>, other: Option<&[u16]>) -> i32 {
    let collators = collators();
    compare_file_names_with(&collators.base_numeric, one.unwrap_or(&[]), other.unwrap_or(&[]))
}

fn compare_file_names_with(collator: &Collator, a: &[u16], b: &[u16]) -> i32 {
    let result = collator.compare(a, b);

    // A opção numeric faz compare(`foo1`, `foo01`) === 0. Desambigua.
    if collator.is_numeric() && result == 0 && a != b {
        return unicode_order(a, b);
    }
    result
}

/// Compara nomes completos sem agrupar por caixa.
pub fn compare_file_names_default(one: Option<&[u16]>, other: Option<&[u16]>) -> i32 {
    compare_and_disambiguate_by_length(&collators().numeric, one.unwrap_or(&[]), other.unwrap_or(&[]))
}

/// Compara nomes completos agrupando os de maiúscula antes dos de minúscula.
pub fn compare_file_names_upper(one: Option<&[u16]>, other: Option<&[u16]>) -> i32 {
    let (one, other) = (one.unwrap_or(&[]), other.unwrap_or(&[]));
    first_non_zero(compare_case_upper_first(one, other), || {
        compare_and_disambiguate_by_length(&collators().numeric, one, other)
    })
}

/// Compara nomes completos agrupando os de minúscula antes dos de maiúscula.
pub fn compare_file_names_lower(one: Option<&[u16]>, other: Option<&[u16]>) -> i32 {
    let (one, other) = (one.unwrap_or(&[]), other.unwrap_or(&[]));
    first_non_zero(compare_case_lower_first(one, other), || {
        compare_and_disambiguate_by_length(&collators().numeric, one, other)
    })
}

/// Compara nomes completos pelo valor Unicode.
pub fn compare_file_names_unicode(one: Option<&[u16]>, other: Option<&[u16]>) -> i32 {
    let (one, other) = (one.unwrap_or(&[]), other.unwrap_or(&[]));
    if one == other { 0 } else { unicode_order(one, other) }
}

/// Compara por extensão e depois pelo nome. Desempata por comparação Unicode.
pub fn compare_file_extensions(one: Option<&[u16]>, other: Option<&[u16]>) -> i32 {
    let (one_name, one_extension) = extract_name_and_extension(one);
    let (other_name, other_extension) = extract_name_and_extension(other);
    let collators = collators();
    let collator = &collators.base_numeric;

    let mut result = collator.compare(one_extension, other_extension);

    if result == 0 {
        // A opção numeric faz compare(`foo1`, `foo01`) === 0. Desambigua.
        if collator.is_numeric() && one_extension != other_extension {
            return unicode_order(one_extension, other_extension);
        }

        // Extensões iguais, compara os nomes
        result = collator.compare(one_name, other_name);

        if collator.is_numeric() && result == 0 && one_name != other_name {
            return unicode_order(one_name, other_name);
        }
    }

    result
}

/// Compara por extensão e depois pelo nome completo, misturando maiúsculas e minúsculas.
pub fn compare_file_extensions_default(one: Option<&[u16]>, other: Option<&[u16]>) -> i32 {
    let (one, other) = (one.unwrap_or(&[]), other.unwrap_or(&[]));
    let collators = collators();
    first_non_zero(
        compare_and_disambiguate_by_length(&collators.numeric_case_insensitive, extract_extension(one), extract_extension(other)),
        || compare_and_disambiguate_by_length(&collators.numeric, one, other),
    )
}

/// Compara por extensão, depois pela caixa, depois pelo nome completo. Agrupa maiúsculas antes de minúsculas.
pub fn compare_file_extensions_upper(one: Option<&[u16]>, other: Option<&[u16]>) -> i32 {
    compare_file_extensions_by_case(one, other, compare_case_upper_first)
}

/// Compara por extensão, depois pela caixa, depois pelo nome completo. Agrupa minúsculas antes de maiúsculas.
pub fn compare_file_extensions_lower(one: Option<&[u16]>, other: Option<&[u16]>) -> i32 {
    compare_file_extensions_by_case(one, other, compare_case_lower_first)
}

fn compare_file_extensions_by_case(one: Option<&[u16]>, other: Option<&[u16]>, compare_case: fn(&[u16], &[u16]) -> i32) -> i32 {
    let (one, other) = (one.unwrap_or(&[]), other.unwrap_or(&[]));
    let collators = collators();
    first_non_zero(
        compare_and_disambiguate_by_length(&collators.numeric_case_insensitive, extract_extension(one), extract_extension(other)),
        || {
            first_non_zero(compare_case(one, other), || {
                compare_and_disambiguate_by_length(&collators.numeric, one, other)
            })
        },
    )
}

/// Compara pelo valor Unicode da extensão em minúsculas, depois pelo valor Unicode do nome completo.
pub fn compare_file_extensions_unicode(one: Option<&[u16]>, other: Option<&[u16]>) -> i32 {
    let (one, other) = (one.unwrap_or(&[]), other.unwrap_or(&[]));
    let one_extension = JsString::from(extract_extension(one)).to_lower_case();
    let other_extension = JsString::from(extract_extension(other)).to_lower_case();

    // Confere as diferenças de extensão
    if one_extension != other_extension {
        return unicode_order(one_extension.units(), other_extension.units());
    }

    // Confere as diferenças do nome completo.
    if one != other {
        return unicode_order(one, other);
    }

    0
}

/// `a || b` do JavaScript sobre resultados de comparação: o primeiro que não é zero.
fn first_non_zero(first: i32, second: impl FnOnce() -> i32) -> i32 {
    if first != 0 { first } else { second() }
}

/// `/^(.*?)(\.([^.]*))?$/`: o nome e a extensão, esta vazia quando não há ponto. O `.` da regex não
/// casa terminador de linha, então um terminador antes do último ponto (ou em qualquer lugar, sem ponto)
/// faz a regex falhar, e o original devolve nome e extensão vazios.
fn extract_name_and_extension(str: Option<&[u16]>) -> (&[u16], &[u16]) {
    match str {
        Some(str) if !str.is_empty() => {
            let (name, extension) = match str.iter().rposition(|&unit| unit == u16::from(b'.')) {
                Some(dot) => (&str[..dot], &str[dot + 1..]),
                None => (str, &str[str.len()..]),
            };
            let line_terminator = |&unit: &u16| matches!(unit, 0x0A | 0x0D | 0x2028 | 0x2029);
            if name.iter().any(line_terminator) { (&[], &[]) } else { (name, extension) }
        }
        _ => (&[], &[]),
    }
}

/// A extensão de um nome completo. Trata dotfiles como nomes, não como extensões.
fn extract_extension(str: &[u16]) -> &[u16] {
    match extract_name_and_extension(Some(str)) {
        (name, extension) if !name.is_empty() && name[0] != u16::from(b'.') => extension,
        _ => &[],
    }
}

fn compare_and_disambiguate_by_length(collator: &Collator, one: &[u16], other: &[u16]) -> i32 {
    // Confere as diferenças
    let result = collator.compare(one, other);
    if result != 0 {
        return result;
    }

    // Numa comparação numérica, `foo1` e `foo01` são equivalentes. Desambigua pondo o mais curto antes.
    if one.len() != other.len() {
        return if one.len() < other.len() { -1 } else { 1 };
    }

    0
}

/// A caixa da primeira unidade do texto: `Some(true)` se `toLocaleUpperCase` ou `toLocaleLowerCase` a
/// muda no sentido pedido. O texto vazio (`''.toUpperCase() === ''`) não muda nada.
fn first_unit_changes_with(string: &[u16], convert: fn(&JsString) -> JsString, ascii: fn(u8) -> bool) -> bool {
    let Some(&unit) = string.first() else { return false };
    if unit < 0x80 {
        return ascii(unit as u8);
    }
    convert(&JsString::from_units(vec![unit])).units() != [unit]
}

/// `true` se o texto começa com letra minúscula.
fn starts_with_lower(string: &[u16]) -> bool {
    first_unit_changes_with(string, JsString::to_upper_case, |byte| byte.is_ascii_lowercase())
}

/// `true` se o texto começa com letra maiúscula.
fn starts_with_upper(string: &[u16]) -> bool {
    first_unit_changes_with(string, JsString::to_lower_case, |byte| byte.is_ascii_uppercase())
}

/// Compara a caixa dos textos, minúscula antes de maiúscula: `-1` se `one` é minúsculo e `other`
/// maiúsculo, `1` no caso contrário, `0` nos demais.
fn compare_case_lower_first(one: &[u16], other: &[u16]) -> i32 {
    if starts_with_lower(one) && starts_with_upper(other) {
        return -1;
    }
    if starts_with_upper(one) && starts_with_lower(other) { 1 } else { 0 }
}

/// Compara a caixa dos textos, maiúscula antes de minúscula: `-1` se `one` é maiúsculo e `other`
/// minúsculo, `1` no caso contrário, `0` nos demais.
fn compare_case_upper_first(one: &[u16], other: &[u16]) -> i32 {
    if starts_with_upper(one) && starts_with_lower(other) {
        return -1;
    }
    if starts_with_lower(one) && starts_with_upper(other) { 1 } else { 0 }
}

fn compare_path_components(one: &[u16], other: &[u16], case_sensitive: bool) -> i32 {
    if case_sensitive {
        return if one == other { 0 } else { unicode_order(one, other) };
    }
    let (one, other) = (JsString::from(one).to_lower_case(), JsString::from(other).to_lower_case());
    if one == other { 0 } else { unicode_order(one.units(), other.units()) }
}

/// Compara caminhos componente a componente (o separador é o da `platform`), e o último com
/// [`compare_file_names`].
pub fn compare_paths(one: &[u16], other: &[u16], case_sensitive: bool, platform: &Platform) -> i32 {
    let separator = if platform.is_windows { u16::from(b'\\') } else { u16::from(b'/') };
    let one_parts: Vec<&[u16]> = one.split(|&unit| unit == separator).collect();
    let other_parts: Vec<&[u16]> = other.split(|&unit| unit == separator).collect();

    let last_one = one_parts.len() - 1;
    let last_other = other_parts.len() - 1;

    let mut i = 0;
    loop {
        let (end_one, end_other) = (last_one == i, last_other == i);

        if end_one && end_other {
            return compare_file_names(Some(one_parts[i]), Some(other_parts[i]));
        } else if end_one {
            return -1;
        } else if end_other {
            return 1;
        }

        let result = compare_path_components(one_parts[i], other_parts[i], case_sensitive);

        if result != 0 {
            return result;
        }
        i += 1;
    }
}

/// Compara pela busca: prefixo antes, sufixo antes, depois por nome de arquivo, e por fim `localeCompare`.
/// `look_for` entra como está, o chamador o põe em minúsculas.
pub fn compare_anything(one: &[u16], other: &[u16], look_for: &[u16]) -> i32 {
    let element_a_name = JsString::from(one).to_lower_case();
    let element_b_name = JsString::from(other).to_lower_case();

    // Prefixos antes de não prefixos
    let prefix_compare = compare_by_prefix(one, other, look_for);
    if prefix_compare != 0 {
        return prefix_compare;
    }

    // Sufixos antes de não sufixos
    let element_a_suffix_match = element_a_name.units().ends_with(look_for);
    let element_b_suffix_match = element_b_name.units().ends_with(look_for);
    if element_a_suffix_match != element_b_suffix_match {
        return if element_a_suffix_match { -1 } else { 1 };
    }

    // Entende nomes de arquivo
    let r = compare_file_names(Some(element_a_name.units()), Some(element_b_name.units()));
    if r != 0 {
        return r;
    }

    // Compara pelo nome
    collators().locale.compare(element_a_name.units(), element_b_name.units())
}

/// Prefixos antes de não prefixos e, entre prefixos, os mais curtos primeiro.
pub fn compare_by_prefix(one: &[u16], other: &[u16], look_for: &[u16]) -> i32 {
    let element_a_name = JsString::from(one).to_lower_case();
    let element_b_name = JsString::from(other).to_lower_case();

    // Prefixos antes de não prefixos
    let element_a_prefix_match = element_a_name.units().starts_with(look_for);
    let element_b_prefix_match = element_b_name.units().starts_with(look_for);
    if element_a_prefix_match != element_b_prefix_match {
        return if element_a_prefix_match { -1 } else { 1 };
    }

    // Mesmo prefixo: os mais curtos primeiro, para ficarem no topo os que casam com mais precisão
    if element_a_prefix_match && element_b_prefix_match {
        if element_a_name.len() < element_b_name.len() {
            return -1;
        }

        if element_a_name.len() > element_b_name.len() {
            return 1;
        }
    }

    0
}
