//! Porte de `runtime/StringPrototype.cpp` e `StringPrototype.h`: o `String.prototype` (um `StringObject`
//! com a string vazia) e os algoritmos de cada método, escritos como funções puras sobre o texto e os
//! argumentos já convertidos (`toString`, `toIntegerOrInfinity`, `toLength`).
//!
//! DIVERGÊNCIAS:
//!
//! - As cascas das funções nativas e a tabela de `finishCreation` (nomes, `length`, intrínsecos,
//!   `DONT_ENUM`) estão em `string_prototype_natives.rs`; `StringPrototype::create` a aplica.
//! - `stringPrototypeTable` (os 13 métodos HTML: `anchor` a `sup`) fica no `ClassInfo` e a `Structure` leva
//!   `HasStaticPropertyTable`: reificam no primeiro acesso, como no C++.
//! - Os algoritmos trabalham sobre unidades UTF-16 (`Vec<u16>`), sem os caminhos de 8 bits e de
//!   `createPaddedString`/`repeatCharacter` que só existem por desempenho; o resultado é estreitado para
//!   Latin1 quando todas as unidades cabem.
//! - `localeCompare` e `toLocale*Case` não moram aqui: passam por `intl_collator.rs` e
//!   `intl_case_mapping.rs`.
//! - `normalize`, `match`, `matchAll`, `search`, `replace`, `replaceAll` e o `split` por `RegExp` ficam
//!   fora (precisam de ICU e de `RegExp`).

use std::borrow::Cow;

use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_type::JSType;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_type_info::{TypeInfo, HAS_STATIC_PROPERTY_TABLE};
use crate::runtime::js_value::JSValue;
use crate::runtime::lookup::{HashTable, HashTableValue, Kind};
use crate::runtime::lookup::{native_entry};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::{DONT_ENUM, FUNCTION};
use crate::runtime::string_prototype_natives_part2 as part2;
use crate::runtime::parse_int::is_str_white_space;
use crate::runtime::string_object::{StringObject, StringObjectRef, STRING_OBJECT_S_INFO};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::string_impl::MAX_LENGTH;
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo StringPrototype::s_info`.
pub static STRING_PROTOTYPE_S_INFO: ClassInfo = ClassInfo {
    class_name: "String",
    parent_class: Some(&STRING_OBJECT_S_INFO),
    static_prop_hash_table: Some(&STRING_PROTOTYPE_TABLE),
    inherits_js_type_range: None,
};

/// `stringPrototypeTableValues`, na ordem do `@begin` (os métodos HTML do Annex B).
static STRING_PROTOTYPE_TABLE_VALUES: [HashTableValue; 13] = [
    native_entry("anchor", part2::string_proto_func_anchor, 1),
    native_entry("big", part2::string_proto_func_big, 0),
    native_entry("bold", part2::string_proto_func_bold, 0),
    native_entry("blink", part2::string_proto_func_blink, 0),
    native_entry("fixed", part2::string_proto_func_fixed, 0),
    native_entry("fontcolor", part2::string_proto_func_fontcolor, 1),
    native_entry("fontsize", part2::string_proto_func_fontsize, 1),
    native_entry("italics", part2::string_proto_func_italics, 0),
    native_entry("link", part2::string_proto_func_link, 1),
    native_entry("small", part2::string_proto_func_small, 0),
    native_entry("strike", part2::string_proto_func_strike, 0),
    native_entry("sub", part2::string_proto_func_sub, 0),
    native_entry("sup", part2::string_proto_func_sup, 0),
];

/// `stringPrototypeTable`.
static STRING_PROTOTYPE_TABLE: HashTable = HashTable { class_for_this: None, values: &STRING_PROTOTYPE_TABLE_VALUES };

/// Os erros que os algoritmos devolvem para quem chama lançar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StringOpError {
    /// `throwVMRangeError(globalObject, scope, message)`.
    RangeError(&'static str),
    /// `throwOutOfMemoryError`.
    OutOfMemory,
}

/// A mensagem do `RangeError` de `repeat`.
pub const REPEAT_RANGE_ERROR: &str = "String.prototype.repeat argument must be greater than or equal to 0 and not be Infinity";

/// `class StringPrototype final : public StringObject`.
pub struct StringPrototype;

impl StringPrototype {
    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::StringObjectType, StringPrototype::STRUCTURE_FLAGS),
            &STRING_PROTOTYPE_S_INFO,
        )
    }

    /// `StructureFlags = Base::StructureFlags | HasStaticPropertyTable`.
    pub const STRUCTURE_FLAGS: u32 = StringObject::STRUCTURE_FLAGS | HAS_STATIC_PROPERTY_TABLE;

    /// `create(vm, globalObject, structure)`: o `StringObject` com a string vazia
    /// (`Base::finishCreation(vm, jsEmptyString(vm))`). A tabela de funções do `finishCreation` e o
    /// `constructor` entram com a `NativeFunction` final (ver o cabeçalho).
    pub fn create(vm: &VM, global_object: &JSGlobalObject, structure: StructureRef) -> StringObjectRef {
        let prototype = StringObject::create(vm, structure);
        crate::runtime::string_prototype_natives::add_string_prototype_properties(&prototype, vm, global_object);
        prototype
    }
}

/// As unidades UTF-16 do texto.
pub fn code_units(string: &WtfString) -> Cow<'_, [u16]> {
    if string.is_null() || string.is_empty() {
        return Cow::Borrowed(&[]);
    }
    if string.is_8bit() {
        Cow::Owned(string.span8().iter().map(|&unit| u16::from(unit)).collect())
    } else {
        Cow::Borrowed(string.span16())
    }
}

/// O texto das unidades, em 8 bits quando todas cabem em Latin1.
pub fn string_from_units(units: &[u16]) -> WtfString {
    if units.iter().all(|&unit| unit <= 0xFF) {
        WtfString::make_8bit(units)
    } else {
        WtfString::from_utf16(units)
    }
}

/// O recorte `[from, to)` do texto.
fn substring_of(units: &[u16], from: usize, to: usize) -> WtfString {
    string_from_units(&units[from..to])
}

/// `ToIntegerOrInfinity` já aplicado sobre `value`, limitado a `[0, len]`.
fn clamp_to_length(value: f64, len: usize) -> usize {
    if value.is_nan() || value <= 0.0 {
        0
    } else if value >= len as f64 {
        len
    } else {
        value as usize
    }
}

/// O índice relativo de `slice` e `substr`: negativo conta do fim.
fn relative_index(value: f64, len: usize) -> usize {
    if value < 0.0 {
        let from_end = len as f64 + value;
        if from_end <= 0.0 { 0 } else { from_end as usize }
    } else {
        clamp_to_length(value, len)
    }
}

/// `stringProtoFuncCharAt`: `position` já passou por `toIntegerOrInfinity`.
pub fn char_at(string: &WtfString, position: f64) -> WtfString {
    let units = code_units(string);
    if position < 0.0 || position >= units.len() as f64 {
        return WtfString::from_latin1(b"");
    }
    substring_of(&units, position as usize, position as usize + 1)
}

/// `stringProtoFuncCharCodeAt`: `None` é o `NaN`.
pub fn char_code_at(string: &WtfString, position: f64) -> Option<u16> {
    let units = code_units(string);
    if position < 0.0 || position >= units.len() as f64 {
        return None;
    }
    Some(units[position as usize])
}

/// `codePointAt` do `String`: `None` é o `undefined`.
pub fn code_point_at(string: &WtfString, position: f64) -> Option<u32> {
    let units = code_units(string);
    if position < 0.0 || position >= units.len() as f64 {
        return None;
    }
    Some(code_point_in(&units, position as usize).0)
}

/// O ponto de código em `index` e quantas unidades ele ocupa (um substituto isolado ocupa uma).
fn code_point_in(units: &[u16], index: usize) -> (u32, usize) {
    let lead = units[index];
    if (0xD800..0xDC00).contains(&lead) {
        if let Some(&trail) = units.get(index + 1) {
            if (0xDC00..0xE000).contains(&trail) {
                return (0x10000 + (((u32::from(lead) - 0xD800) << 10) | (u32::from(trail) - 0xDC00)), 2);
            }
        }
    }
    (u32::from(lead), 1)
}

/// A primeira ocorrência de `needle` em `haystack` a partir de `start`.
pub fn find_units(haystack: &[u16], needle: &[u16], start: usize) -> Option<usize> {
    if start > haystack.len() || needle.len() > haystack.len() - start {
        return None;
    }
    if needle.is_empty() {
        return Some(start);
    }
    haystack[start..].windows(needle.len()).position(|window| window == needle).map(|offset| start + offset)
}

/// `stringProtoFuncIndexOf`: `position` já passou por `toIntegerOrInfinity`; `-1` quando não acha.
pub fn index_of(string: &WtfString, search: &WtfString, position: f64) -> i32 {
    let units = code_units(string);
    let needle = code_units(search);
    let start = clamp_to_length(position, units.len());
    find_units(&units, &needle, start).map_or(-1, |index| index as i32)
}

/// `stringProtoFuncLastIndexOf`: `position` é o `toNumber` com o `NaN` já trocado por `+Infinity` (o
/// `toIntegerPreserveNaN` do C++ devolve o `NaN` e o corpo faz a troca).
pub fn last_index_of(string: &WtfString, search: &WtfString, position: f64) -> i32 {
    let units = code_units(string);
    let needle = code_units(search);
    if units.len() < needle.len() {
        return -1;
    }
    let position = if position.is_nan() { f64::INFINITY } else { position.trunc() };
    let max_start = units.len() - needle.len();
    let start = clamp_to_length(position, max_start);
    (0..=start).rev().find(|&index| units[index..index + needle.len()] == needle[..]).map_or(-1, |index| index as i32)
}

/// `stringProtoFuncSlice`: `start` e `end` já passaram por `toIntegerOrInfinity`; `end` ausente é o
/// `undefined`.
pub fn slice(string: &WtfString, start: f64, end: Option<f64>) -> WtfString {
    let units = code_units(string);
    let from = relative_index(start, units.len());
    let to = end.map_or(units.len(), |end| relative_index(end, units.len()));
    if from >= to {
        return WtfString::from_latin1(b"");
    }
    substring_of(&units, from, to)
}

/// `stringProtoFuncSubstring`.
pub fn substring(string: &WtfString, start: f64, end: Option<f64>) -> WtfString {
    let units = code_units(string);
    let first = clamp_to_length(start, units.len());
    let second = end.map_or(units.len(), |end| clamp_to_length(end, units.len()));
    substring_of(&units, first.min(second), first.max(second))
}

/// `stringProtoFuncSubstr`.
pub fn substr(string: &WtfString, start: f64, length: Option<f64>) -> WtfString {
    let units = code_units(string);
    let from = relative_index(start, units.len());
    let count = length.map_or(units.len(), |length| clamp_to_length(length, units.len()));
    let to = from.saturating_add(count).min(units.len());
    substring_of(&units, from, to)
}

/// `stringProtoFuncToUpperCase`.
pub fn to_upper_case(string: &WtfString) -> WtfString {
    string.convert_to_uppercase_without_locale()
}

/// `stringProtoFuncToLowerCase`.
pub fn to_lower_case(string: &WtfString) -> WtfString {
    string.convert_to_lowercase_without_locale()
}

/// `stringProtoFuncTrim`.
pub fn trim(string: &WtfString) -> WtfString {
    string.trim(is_str_white_space::<u16>)
}

/// `stringProtoFuncTrimStart`.
pub fn trim_start(string: &WtfString) -> WtfString {
    let units = code_units(string);
    let from = units.iter().position(|&unit| !is_str_white_space::<u16>(unit)).unwrap_or(units.len());
    substring_of(&units, from, units.len())
}

/// `stringProtoFuncTrimEnd`.
pub fn trim_end(string: &WtfString) -> WtfString {
    let units = code_units(string);
    let to = units.iter().rposition(|&unit| !is_str_white_space::<u16>(unit)).map_or(0, |index| index + 1);
    substring_of(&units, 0, to)
}

/// `padString<padKind>`: `max_length` já passou por `toLength`; `fill` vazio devolve a própria string.
pub fn pad(string: &WtfString, max_length: f64, fill: &WtfString, at_start: bool) -> Result<WtfString, StringOpError> {
    let units = code_units(string);
    // `!(a > b)` também cobre o NaN, que no C++ (`maxLength <= length` após `toLength`) nunca chega aqui;
    // sem isso o NaN viraria 0 no cast e a subtração abaixo estouraria.
    if !(max_length > units.len() as f64) {
        return Ok(string.clone());
    }
    let fill_units = code_units(fill);
    if fill_units.is_empty() {
        return Ok(string.clone());
    }
    if max_length > f64::from(MAX_LENGTH) {
        return Err(StringOpError::OutOfMemory);
    }
    let fill_length = max_length as usize - units.len();
    // O preenchimento é o `fill` repetido e cortado em `fill_length`; copiar por dobra (`slice::repeat`) em vez de
    // um iterador `cycle().take()` unidade a unidade, que levava minutos num `padEnd(2 ** 30)` sem otimização.
    let fits_latin1 = |slice: &[u16]| slice.iter().all(|&unit| unit <= 0xFF);
    let visible_fill = &fill_units[..fill_units.len().min(fill_length)];
    if fits_latin1(&units[..]) && fits_latin1(visible_fill) {
        let narrow = |slice: &[u16]| slice.iter().map(|&unit| unit as u8).collect::<Vec<u8>>();
        let body = narrow(&units[..]);
        // Um só buffer final (corpo + preenchimento), sem o `tile` intermediário nem o `concat`: num `padEnd(2 ** 30)`
        // os três buffers de 1 GiB somados esgotavam a memória do processo de teste.
        let pattern = narrow(&fill_units[..]);
        let mut result = Vec::new();
        result.try_reserve_exact(max_length as usize).map_err(|_| StringOpError::OutOfMemory)?;
        if !at_start {
            result.extend_from_slice(&body);
        }
        let mut remaining = fill_length;
        while remaining > 0 {
            let take = remaining.min(pattern.len());
            result.extend_from_slice(&pattern[..take]);
            remaining -= take;
        }
        if at_start {
            result.extend_from_slice(&body);
        }
        return Ok(WtfString::from_latin1(&result));
    }
    let padding = tile(&fill_units[..], fill_length);
    let result: Vec<u16> = if at_start { [&padding[..], &units[..]].concat() } else { [&units[..], &padding[..]].concat() };
    Ok(string_from_units(&result))
}

/// `pattern` repetido e cortado em `length` unidades (`pattern` não vazio).
fn tile<T: Copy>(pattern: &[T], length: usize) -> Vec<T> {
    let mut tiled = pattern.repeat(length.div_ceil(pattern.len()));
    tiled.truncate(length);
    tiled
}

/// `stringProtoFuncRepeat`: `count` já passou por `toIntegerOrInfinity`.
pub fn repeat(string: &WtfString, count: f64) -> Result<WtfString, StringOpError> {
    if count < 0.0 || count.is_infinite() {
        return Err(StringOpError::RangeError(REPEAT_RANGE_ERROR));
    }
    let units = code_units(string);
    if units.is_empty() || count == 0.0 {
        return Ok(WtfString::from_latin1(b""));
    }
    if count > f64::from(MAX_LENGTH) {
        return Err(StringOpError::OutOfMemory);
    }
    let count = count as usize;
    if count == 1 {
        return Ok(string.clone());
    }
    match units.len().checked_mul(count) {
        Some(total) if total <= MAX_LENGTH as usize => {
            // Estreitar o padrão (curto) e repetir os bytes: varrer as `total` unidades para decidir a largura
            // custava um laço de 2^30 passos sem otimização.
            if units.iter().all(|&unit| unit <= 0xFF) {
                let pattern: Vec<u8> = units.iter().map(|&unit| unit as u8).collect();
                Ok(WtfString::from_latin1(&pattern.repeat(count)))
            } else {
                Ok(WtfString::from_utf16(&units.repeat(count)))
            }
        }
        _ => Err(StringOpError::OutOfMemory),
    }
}

/// `stringProtoFuncStartsWith`: `position` já passou por `toIntegerOrInfinity`.
pub fn starts_with(string: &WtfString, search: &WtfString, position: f64) -> bool {
    let units = code_units(string);
    let needle = code_units(search);
    let start = clamp_to_length(position, units.len());
    units.len() - start >= needle.len() && units[start..start + needle.len()] == needle[..]
}

/// `stringProtoFuncEndsWith`: `end_position` ausente é o `undefined` (o tamanho da string).
pub fn ends_with(string: &WtfString, search: &WtfString, end_position: Option<f64>) -> bool {
    let units = code_units(string);
    let needle = code_units(search);
    let end = end_position.map_or(units.len(), |end| clamp_to_length(end, units.len()));
    end >= needle.len() && units[end - needle.len()..end] == needle[..]
}

/// `stringProtoFuncIncludes`.
pub fn includes(string: &WtfString, search: &WtfString, position: f64) -> bool {
    index_of(string, search, position) != -1
}

/// `stringProtoFuncConcat`: as partes já passaram por `toString`.
pub fn concat(parts: &[WtfString]) -> Result<WtfString, StringOpError> {
    let mut result: Vec<u16> = Vec::new();
    for part in parts {
        let units = code_units(part);
        if result.len() + units.len() > MAX_LENGTH as usize {
            return Err(StringOpError::OutOfMemory);
        }
        result.extend_from_slice(&units);
    }
    Ok(string_from_units(&result))
}

/// `stringProtoFuncAt`: `index` já passou por `toIntegerOrInfinity`; `None` é o `undefined`.
pub fn at(string: &WtfString, index: f64) -> Option<WtfString> {
    let units = code_units(string);
    let len = units.len() as f64;
    let position = if index >= 0.0 { index } else { len + index };
    if position < 0.0 || position >= len {
        return None;
    }
    Some(substring_of(&units, position as usize, position as usize + 1))
}

/// `stringProtoFuncSplit` com separador de texto (sem `RegExp`): `separator` ausente é o `undefined`;
/// `limit` já passou por `toUint32` (o `undefined` é `2^32 - 1`).
pub fn split(string: &WtfString, separator: Option<&WtfString>, limit: u32) -> Vec<WtfString> {
    if limit == 0 {
        return Vec::new();
    }
    let Some(separator) = separator else {
        return vec![string.clone()];
    };
    let units = code_units(string);
    let separator_units = code_units(separator);
    if units.is_empty() {
        return if separator_units.is_empty() { Vec::new() } else { vec![string.clone()] };
    }
    let limit = limit as usize;
    let mut pieces = Vec::new();
    if separator_units.is_empty() {
        for index in 0..units.len().min(limit) {
            pieces.push(substring_of(&units, index, index + 1));
        }
        return pieces;
    }
    let mut start = 0;
    while let Some(found) = find_units(&units, &separator_units, start) {
        pieces.push(substring_of(&units, start, found));
        if pieces.len() >= limit {
            return pieces;
        }
        start = found + separator_units.len();
    }
    pieces.push(substring_of(&units, start, units.len()));
    pieces
}

/// `stringProtoFuncIsWellFormed`.
pub fn is_well_formed(string: &WtfString) -> bool {
    let units = code_units(string);
    let mut index = 0;
    while index < units.len() {
        let (code_point, width) = code_point_in(&units, index);
        if (0xD800..0xE000).contains(&code_point) {
            return false;
        }
        index += width;
    }
    true
}

/// `stringProtoFuncToWellFormed`: cada substituto isolado vira U+FFFD.
pub fn to_well_formed(string: &WtfString) -> WtfString {
    let units = code_units(string);
    let mut result = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        let (code_point, width) = code_point_in(&units, index);
        if (0xD800..0xE000).contains(&code_point) {
            result.push(0xFFFD);
        } else {
            result.extend_from_slice(&units[index..index + width]);
        }
        index += width;
    }
    string_from_units(&result)
}

/// O passo de `%StringIteratorPrototype%.next`: o ponto de código (como texto) em `index` e o próximo
/// índice; `None` quando acabou (o `stringIteratorNext` do `StringIteratorPrototype.js`).
pub fn next_code_point(string: &WtfString, index: u32) -> Option<(WtfString, u32)> {
    let units = code_units(string);
    let index = index as usize;
    if index >= units.len() {
        return None;
    }
    let (_, width) = code_point_in(&units, index);
    Some((substring_of(&units, index, index + width), (index + width) as u32))
}

/// A iteração por pontos de código (`String.prototype[Symbol.iterator]`).
pub struct CodePoints {
    units: Vec<u16>,
    index: usize,
}

impl CodePoints {
    /// `CodePoints` sobre o texto.
    pub fn new(string: &WtfString) -> CodePoints {
        CodePoints { units: code_units(string).into_owned(), index: 0 }
    }
}

impl Iterator for CodePoints {
    type Item = WtfString;

    fn next(&mut self) -> Option<WtfString> {
        if self.index >= self.units.len() {
            return None;
        }
        let (_, width) = code_point_in(&self.units, self.index);
        let piece = substring_of(&self.units, self.index, self.index + width);
        self.index += width;
        Some(piece)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> WtfString {
        string_from_units(&text.encode_utf16().collect::<Vec<u16>>())
    }

    fn text(string: &WtfString) -> std::string::String {
        std::string::String::from_utf16_lossy(&code_units(string))
    }

    #[test]
    fn slicing_family() {
        assert_eq!(text(&slice(&s("abcdef"), -3.0, None)), "def");
        assert_eq!(text(&slice(&s("abcdef"), 1.0, Some(-1.0))), "bcde");
        assert_eq!(text(&substring(&s("abcdef"), 4.0, Some(1.0))), "bcd");
        assert_eq!(text(&substr(&s("abcdef"), -3.0, Some(2.0))), "de");
    }

    #[test]
    fn searching() {
        assert_eq!(index_of(&s("abcabc"), &s("c"), 3.0), 5);
        assert_eq!(index_of(&s("abc"), &s(""), 10.0), 3);
        assert_eq!(last_index_of(&s("abcabc"), &s("b"), f64::NAN), 4);
        assert_eq!(last_index_of(&s("abcabc"), &s("b"), 3.0), 1);
        assert!(starts_with(&s("abc"), &s("bc"), 1.0));
        assert!(ends_with(&s("abc"), &s("ab"), Some(2.0)));
    }

    #[test]
    fn padding_repeat_and_split() {
        assert_eq!(text(&pad(&s("5"), 3.0, &s("0"), true).unwrap()), "005");
        assert_eq!(text(&pad(&s("ab"), 7.0, &s("xyz"), false).unwrap()), "abxyzxy");
        assert_eq!(text(&repeat(&s("ab"), 3.0).unwrap()), "ababab");
        assert!(matches!(repeat(&s("a"), -1.0), Err(StringOpError::RangeError(_))));
        let pieces: Vec<_> = split(&s("a,b,,c"), Some(&s(",")), u32::MAX).iter().map(text).collect();
        assert_eq!(pieces, ["a", "b", "", "c"]);
        assert_eq!(split(&s(""), Some(&s("")), u32::MAX).len(), 0);
    }

    #[test]
    fn code_points() {
        let value = s("a\u{1F600}b");
        assert_eq!(code_point_at(&value, 1.0), Some(0x1F600));
        assert_eq!(CodePoints::new(&value).count(), 3);
        assert!(is_well_formed(&value));
        assert!(!is_well_formed(&WtfString::from_utf16(&[0xD800])));
    }

    #[test]
    fn trimming_and_compare() {
        assert_eq!(text(&trim(&s("  a b \n"))), "a b");
        assert_eq!(text(&trim_start(&s("  a "))), "a ");
        assert_eq!(text(&trim_end(&s("  a "))), "  a");
    }
}
