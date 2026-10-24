//! O módulo `node:path` do bun 1.4.2: o lado posix (`path.posix`, que também é o `path` em si) e o `path.win32`.
//! Forma medida no bun:
//!
//! - `win32.win32`, `win32.posix`, `posix.win32` e `posix.posix` apontam para os dois objetos de sempre (`posix.win32 ===
//!   win32`, `win32.posix === posix`), e `win32` vem antes de `posix` na ordem das chaves;
//! - o `win32` tem as mesmas chaves e nomes; `sep` é `\` e `delimiter` é `;`; o esqueleto compartilhado com o posix
//!   (`normalize_string`, a varredura de `basename`/`extname`/`parse`, `format`) recebe um [`Style`] (separador e
//!   predicado de separador), como o Node faz com `normalizeString(path, allowAboveRoot, separator, isPathSeparator)`;
//! - o `win32.resolve` roda em Linux com o `process.cwd()` posix (`/tmp` vira `C:\tmp` quando só a letra de unidade é
//!   dada), pois o `=C:` do ambiente só existe no Windows;
//! - as funções são nativas ligadas (`bound resolve`, `length` 1); `sep` é `/`, `delimiter` é `:`;
//! - ordem das chaves: `resolve normalize isAbsolute join relative toNamespacedPath dirname basename extname format
//!   parse sep delimiter win32 posix _makeLong`; `_makeLong` é a mesma função de `toNamespacedPath`;
//! - argumento que não é string: `TypeError` com `code` `ERR_INVALID_ARG_TYPE` e a mensagem
//!   `The "path" property must be of type string, got number` (o tipo é o `typeof`, com `array` para arrays;
//!   `join`/`resolve` nomeiam `paths[i]`, `relative` nomeia `from` e `to`, `basename` nomeia `ext`, `format` pede
//!   `object` e nomeia `pathObject`); `toNamespacedPath` devolve o argumento como veio;
//! - `join` valida todos os argumentos, `resolve` valida de trás para frente e para no primeiro caminho absoluto.
//!
//! O trabalho de verdade é feito sobre unidades UTF-16 (`&[u16]`), como o Node, para que par substituto solto
//! atravesse sem perda. Falta (ver o relatório da fatia): o `matchesGlob`, que depende do `Bun.Glob`.

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::put_direct_native_function_with_display_name;
use crate::runtime::js_array::is_js_array;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::{js_boolean, JSValue};
use crate::runtime::native_class_support::{property_key, throw_coded_type_error};
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::process_shape::text_value;
use crate::runtime::property_attribute::{DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::process_system::current_directory;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::string_regexp_support::get_object_property;
use crate::wtf::text::wtf_string::String as WtfString;

const SLASH: u16 = b'/' as u16;
const BACKSLASH: u16 = b'\\' as u16;
const DOT: u16 = b'.' as u16;
const COLON: u16 = b':' as u16;
const QUESTION: u16 = b'?' as u16;

/// O que muda entre `posix` e `win32` no esqueleto compartilhado: o separador que o resultado usa e o predicado
/// de separador de entrada (`isPosixPathSeparator` / `isPathSeparator` do Node).
#[derive(Clone, Copy)]
pub(crate) struct Style {
    sep: u16,
    is_sep: fn(u16) -> bool,
}

fn is_posix_sep(unit: u16) -> bool {
    unit == SLASH
}

fn is_win32_sep(unit: u16) -> bool {
    unit == SLASH || unit == BACKSLASH
}

pub(crate) const POSIX: Style = Style { sep: SLASH, is_sep: is_posix_sep };
pub(crate) const WIN32: Style = Style { sep: BACKSLASH, is_sep: is_win32_sep };

/// `isWindowsDeviceRoot`: letra ASCII.
fn is_device_root(unit: u16) -> bool {
    matches!(unit, 0x41..=0x5A | 0x61..=0x7A)
}

/// `String.prototype.slice(start, end)` sobre unidades, com os índices presos ao tamanho como no JS.
fn js_slice(units: &[u16], start: usize, end: usize) -> Vec<u16> {
    let end = end.min(units.len());
    units[start.min(end)..end].to_vec()
}

/// `toLowerCase` do JS sobre unidades: par substituto solto atravessa intacto.
fn lower_units(units: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(units.len());
    let mut run = String::new();
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(character) => run.push(character),
            Err(lone) => {
                out.extend(run.to_lowercase().encode_utf16());
                run.clear();
                out.push(lone.unpaired_surrogate());
            }
        }
    }
    out.extend(run.to_lowercase().encode_utf16());
    out
}

/// Texto do motor como unidades UTF-16.
fn units_of(text: &WtfString) -> Vec<u16> {
    (0..text.length()).map(|index| text.code_unit_at(index)).collect()
}

fn units_value(global_object: &JSGlobalObject, units: &[u16]) -> JSValue {
    JSValue::from_js_string(crate::runtime::js_string::js_string(global_object.vm(), &WtfString::from_utf16(units)))
}

/// O `typeof` do Node para a mensagem de `ERR_INVALID_ARG_TYPE` de propriedade (`array` para array, `object` para null).
fn kind_name(value: JSValue) -> &'static str {
    if value.is_undefined() {
        "undefined"
    } else if value.is_null() {
        "object"
    } else if value.is_number() {
        "number"
    } else if value.is_boolean() {
        "boolean"
    } else if value.is_string() {
        "string"
    } else if value.is_symbol() {
        "symbol"
    } else if value.is_big_int() {
        "bigint"
    } else if value.is_callable() {
        "function"
    } else if is_js_array(&value) {
        "array"
    } else {
        "object"
    }
}

fn invalid_property_type(global_object: &JSGlobalObject, name: &str, expected: &str, value: JSValue) -> Thrown {
    let message = format!("The \"{name}\" property must be of type {expected}, got {}", kind_name(value));
    throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE")
}

/// `validateString(value, name)` do bun: devolve as unidades da string ou lança.
fn string_argument(global_object: &JSGlobalObject, value: JSValue, name: &str) -> Result<Vec<u16>, Thrown> {
    if value.is_string() {
        Ok(units_of(&value.to_wtf_string()))
    } else {
        Err(invalid_property_type(global_object, name, "string", value))
    }
}

/// `normalizeString` do Node: remove `.`, resolve `..`, colapsa separadores repetidos (sem barra inicial nem final).
fn normalize_string(path: &[u16], allow_above_root: bool, style: Style) -> Vec<u16> {
    let mut result: Vec<u16> = Vec::new();
    let mut last_segment_length: isize = 0;
    let mut last_slash: isize = -1;
    let mut dots: i32 = 0;
    let mut code: u16 = 0;
    for index in 0..=path.len() {
        if index < path.len() {
            code = path[index];
        } else if (style.is_sep)(code) {
            break;
        } else {
            code = SLASH;
        }
        let at = index as isize;
        if (style.is_sep)(code) {
            if last_slash == at - 1 || dots == 1 {
                // Nada: segmento vazio ou `.`.
            } else if dots == 2 {
                let ends_with_dots = result.len() >= 2 && result[result.len() - 1] == DOT && result[result.len() - 2] == DOT;
                if result.len() < 2 || last_segment_length != 2 || !ends_with_dots {
                    if result.len() > 2 {
                        match result.iter().rposition(|&unit| unit == style.sep) {
                            None => {
                                result.clear();
                                last_segment_length = 0;
                            }
                            Some(position) => {
                                result.truncate(position);
                                last_segment_length = result.len() as isize - 1 - result.iter().rposition(|&unit| unit == style.sep).map_or(-1, |p| p as isize);
                            }
                        }
                        last_slash = at;
                        dots = 0;
                        continue;
                    } else if !result.is_empty() {
                        result.clear();
                        last_segment_length = 0;
                        last_slash = at;
                        dots = 0;
                        continue;
                    }
                }
                if allow_above_root {
                    if !result.is_empty() {
                        result.push(style.sep);
                    }
                    result.extend([DOT, DOT]);
                    last_segment_length = 2;
                }
            } else {
                let segment = &path[(last_slash + 1) as usize..index];
                if !result.is_empty() {
                    result.push(style.sep);
                }
                result.extend_from_slice(segment);
                last_segment_length = at - last_slash - 1;
            }
            last_slash = at;
            dots = 0;
        } else if code == DOT && dots != -1 {
            dots += 1;
        } else {
            dots = -1;
        }
    }
    result
}

/// `posix.normalize` sobre unidades.
pub(crate) fn normalize_units(path: &[u16]) -> Vec<u16> {
    if path.is_empty() {
        return vec![DOT];
    }
    let absolute = path[0] == SLASH;
    let trailing = path[path.len() - 1] == SLASH;
    let mut normalized = normalize_string(path, !absolute, POSIX);
    if normalized.is_empty() {
        if absolute {
            return vec![SLASH];
        }
        return if trailing { vec![DOT, SLASH] } else { vec![DOT] };
    }
    if trailing {
        normalized.push(SLASH);
    }
    if absolute {
        normalized.insert(0, SLASH);
    }
    normalized
}

/// Os argumentos não vazios de `join`, unidos pelo separador do estilo (o esqueleto que as duas plataformas dividem).
fn concat_non_empty(parts: &[Vec<u16>], sep: u16) -> Vec<u16> {
    let mut joined: Vec<u16> = Vec::new();
    for part in parts.iter().filter(|part| !part.is_empty()) {
        if !joined.is_empty() {
            joined.push(sep);
        }
        joined.extend_from_slice(part);
    }
    joined
}

/// `posix.join` sobre as unidades já validadas.
pub(crate) fn join_units(parts: &[Vec<u16>]) -> Vec<u16> {
    let joined = concat_non_empty(parts, SLASH);
    if joined.is_empty() {
        return vec![DOT];
    }
    normalize_units(&joined)
}

/// O fim de `posix.resolve`: `resolved` já montado de trás para frente (cada caminho seguido de `/`).
fn finish_resolve(resolved: &[u16], absolute: bool) -> Vec<u16> {
    let mut normalized = normalize_string(resolved, !absolute, POSIX);
    if absolute {
        normalized.insert(0, SLASH);
        return normalized;
    }
    if normalized.is_empty() {
        vec![DOT]
    } else {
        normalized
    }
}

/// `posix.resolve` sobre caminhos já validados (do primeiro ao último) e o diretório corrente.
pub(crate) fn resolve_units(parts: &[Vec<u16>], cwd: &[u16]) -> Vec<u16> {
    let mut resolved: Vec<u16> = Vec::new();
    let mut absolute = false;
    for part in parts.iter().rev().map(Vec::as_slice).chain(std::iter::once(cwd)) {
        if absolute {
            break;
        }
        if part.is_empty() {
            continue;
        }
        let mut next = part.to_vec();
        if !resolved.is_empty() {
            next.push(SLASH);
            next.extend_from_slice(&resolved);
        }
        resolved = next;
        absolute = part[0] == SLASH;
    }
    finish_resolve(&resolved, absolute)
}

/// `posix.relative` sobre caminhos já resolvidos.
pub(crate) fn relative_units(from: &[u16], to: &[u16]) -> Vec<u16> {
    if from == to {
        return Vec::new();
    }
    let from_end = from.len();
    let from_len = from_end - 1;
    let to_len = to.len() - 1;
    let length = from_len.min(to_len);
    let mut last_common: isize = -1;
    let mut index = 0usize;
    while index < length {
        let code = from[1 + index];
        if code != to[1 + index] {
            break;
        }
        if code == SLASH {
            last_common = index as isize;
        }
        index += 1;
    }
    if index == length {
        if to_len > length {
            if to[1 + index] == SLASH {
                return to[1 + index + 1..].to_vec();
            }
            if index == 0 {
                return to[1 + index..].to_vec();
            }
        } else if from_len > length {
            if from[1 + index] == SLASH {
                last_common = index as isize;
            } else if index == 0 {
                last_common = 0;
            }
        }
    }
    let mut out: Vec<u16> = Vec::new();
    let mut position = (1 + last_common + 1) as usize;
    while position <= from_end {
        if position == from_end || from[position] == SLASH {
            if out.is_empty() {
                out.extend([DOT, DOT]);
            } else {
                out.extend([SLASH, DOT, DOT]);
            }
        }
        position += 1;
    }
    out.extend_from_slice(&to[(1 + last_common) as usize..]);
    out
}

/// `posix.dirname`.
pub(crate) fn dirname_units(path: &[u16]) -> Vec<u16> {
    if path.is_empty() {
        return vec![DOT];
    }
    let has_root = path[0] == SLASH;
    let mut end: isize = -1;
    let mut matched_slash = true;
    for index in (1..path.len()).rev() {
        if path[index] == SLASH {
            if !matched_slash {
                end = index as isize;
                break;
            }
        } else {
            matched_slash = false;
        }
    }
    if end == -1 {
        return vec![if has_root { SLASH } else { DOT }];
    }
    if has_root && end == 1 {
        return vec![SLASH, SLASH];
    }
    path[..end as usize].to_vec()
}

/// `basename(path, suffix)` a partir de `start` (o win32 pula a letra de unidade): o laço que as duas plataformas dividem.
fn basename_from(path: &[u16], suffix: Option<&[u16]>, mut start: usize, style: Style) -> Vec<u16> {
    let mut end: isize = -1;
    let mut matched_slash = true;
    if let Some(suffix) = suffix.filter(|suffix| !suffix.is_empty() && suffix.len() <= path.len()) {
        if suffix == path {
            return Vec::new();
        }
        let mut ext_index = suffix.len() as isize - 1;
        let mut first_non_slash_end: isize = -1;
        for index in (start..path.len()).rev() {
            let code = path[index];
            if (style.is_sep)(code) {
                if !matched_slash {
                    start = index + 1;
                    break;
                }
            } else {
                if first_non_slash_end == -1 {
                    matched_slash = false;
                    first_non_slash_end = index as isize + 1;
                }
                if ext_index >= 0 {
                    if code == suffix[ext_index as usize] {
                        ext_index -= 1;
                        if ext_index == -1 {
                            end = index as isize;
                        }
                    } else {
                        ext_index = -1;
                        end = first_non_slash_end;
                    }
                }
            }
        }
        if start as isize == end {
            end = first_non_slash_end;
        } else if end == -1 {
            end = path.len() as isize;
        }
        return path[start..end as usize].to_vec();
    }
    for index in (start..path.len()).rev() {
        if (style.is_sep)(path[index]) {
            if !matched_slash {
                start = index + 1;
                break;
            }
        } else if end == -1 {
            matched_slash = false;
            end = index as isize + 1;
        }
    }
    if end == -1 {
        return Vec::new();
    }
    path[start..end as usize].to_vec()
}

/// `posix.basename(path, suffix)`.
pub(crate) fn basename_units(path: &[u16], suffix: Option<&[u16]>) -> Vec<u16> {
    basename_from(path, suffix, 0, POSIX)
}

/// Onde começa o resto do caminho win32: depois de `X:` (2) ou no início (0).
fn win32_drive_prefix(path: &[u16]) -> usize {
    if path.len() >= 2 && is_device_root(path[0]) && path[1] == COLON {
        2
    } else {
        0
    }
}

/// `win32.basename(path, suffix)`.
pub(crate) fn win32_basename_units(path: &[u16], suffix: Option<&[u16]>) -> Vec<u16> {
    basename_from(path, suffix, win32_drive_prefix(path), WIN32)
}

/// A varredura de trás para frente que `extname` e `parse` compartilham: `(start_dot, start_part, end, pre_dot_state)`.
/// `initial_part` é o `startPart` de partida (o posix `parse` começa em 0 mesmo varrendo a partir de 1).
fn scan_extension(path: &[u16], lowest: usize, initial_part: usize, style: Style) -> (isize, usize, isize, i32) {
    let mut start_dot: isize = -1;
    let mut start_part = initial_part;
    let mut end: isize = -1;
    let mut matched_slash = true;
    let mut pre_dot_state = 0;
    for index in (lowest..path.len()).rev() {
        let code = path[index];
        if (style.is_sep)(code) {
            if !matched_slash {
                start_part = index + 1;
                break;
            }
            continue;
        }
        if end == -1 {
            matched_slash = false;
            end = index as isize + 1;
        }
        if code == DOT {
            if start_dot == -1 {
                start_dot = index as isize;
            } else if pre_dot_state != 1 {
                pre_dot_state = 1;
            }
        } else if start_dot != -1 {
            pre_dot_state = -1;
        }
    }
    (start_dot, start_part, end, pre_dot_state)
}

/// Há extensão? (`startDot` achado, parte não vazia, e não é só `..` ou `.nome`).
fn has_extension(start_dot: isize, start_part: usize, end: isize, pre_dot_state: i32) -> bool {
    !(start_dot == -1 || end == -1 || pre_dot_state == 0 || (pre_dot_state == 1 && start_dot == end - 1 && start_dot == start_part as isize + 1))
}

/// `posix.extname`.
pub(crate) fn extname_units(path: &[u16]) -> Vec<u16> {
    let (start_dot, start_part, end, pre_dot_state) = scan_extension(path, 0, 0, POSIX);
    if !has_extension(start_dot, start_part, end, pre_dot_state) {
        return Vec::new();
    }
    path[start_dot as usize..end as usize].to_vec()
}

/// `win32.extname`: a varredura para na letra de unidade.
pub(crate) fn win32_extname_units(path: &[u16]) -> Vec<u16> {
    let start = win32_drive_prefix(path);
    let (start_dot, start_part, end, pre_dot_state) = scan_extension(path, start, start, WIN32);
    if !has_extension(start_dot, start_part, end, pre_dot_state) {
        return Vec::new();
    }
    path[start_dot as usize..end as usize].to_vec()
}

/// O resultado de `parse`, na ordem do bun: `root dir base ext name`.
#[derive(Default, Debug, PartialEq, Eq)]
pub(crate) struct ParsedPath {
    pub root: Vec<u16>,
    pub dir: Vec<u16>,
    pub base: Vec<u16>,
    pub ext: Vec<u16>,
    pub name: Vec<u16>,
}

/// `posix.parse`.
pub(crate) fn parse_units(path: &[u16]) -> ParsedPath {
    let mut parsed = ParsedPath::default();
    if path.is_empty() {
        return parsed;
    }
    let absolute = path[0] == SLASH;
    let lowest = usize::from(absolute);
    if absolute {
        parsed.root = vec![SLASH];
    }
    let (start_dot, start_part, end, pre_dot_state) = scan_extension(path, lowest, 0, POSIX);
    if end != -1 {
        let start = if start_part == 0 && absolute { 1 } else { start_part };
        let end = end as usize;
        if has_extension(start_dot, start_part, end as isize, pre_dot_state) {
            parsed.name = path[start..start_dot as usize].to_vec();
            parsed.base = path[start..end].to_vec();
            parsed.ext = path[start_dot as usize..end].to_vec();
        } else {
            parsed.base = path[start..end].to_vec();
            parsed.name = parsed.base.clone();
        }
    }
    if start_part > 0 {
        parsed.dir = path[..start_part - 1].to_vec();
    } else if absolute {
        parsed.dir = vec![SLASH];
    }
    parsed
}

/// `format` (`_format(sep, pathObject)` do Node) sobre os campos já convertidos em texto (vazio = ausente ou falso).
pub(crate) fn format_units(style: Style, dir: &[u16], root: &[u16], base: &[u16], name: &[u16], ext: &[u16]) -> Vec<u16> {
    let dir_or_root = if dir.is_empty() { root } else { dir };
    // `formatExt` do Node: a extensão ganha o ponto se não começar por um.
    let ext: Vec<u16> = if ext.is_empty() || ext[0] == u16::from(b'.') { ext.to_vec() } else { [&[u16::from(b'.')][..], ext].concat() };
    let base: Vec<u16> = if base.is_empty() { [name, &ext[..]].concat() } else { base.to_vec() };
    if dir_or_root.is_empty() {
        return base;
    }
    if dir_or_root == root {
        return [dir_or_root, &base].concat();
    }
    [dir_or_root, &[style.sep][..], &base].concat()
}

/// A raiz UNC `\\servidor\compartilhamento` achada por `normalize`, `resolve`, `dirname` e `parse` do win32, que a
/// escaneiam do mesmo jeito: duas barras, um nome, barras, um nome. `end` é onde o segundo nome termina.
struct UncRoot {
    first: (usize, usize),
    second_start: usize,
    end: usize,
}

/// Escaneia a raiz UNC de um caminho que já começa com dois separadores; `None` se faltar o servidor ou o compartilhamento.
fn scan_unc_root(path: &[u16]) -> Option<UncRoot> {
    let len = path.len();
    let mut j = 2;
    while j < len && !is_win32_sep(path[j]) {
        j += 1;
    }
    if j >= len || j == 2 {
        return None;
    }
    let first = (2, j);
    let separators_start = j;
    while j < len && is_win32_sep(path[j]) {
        j += 1;
    }
    if j >= len || j == separators_start {
        return None;
    }
    let second_start = j;
    while j < len && !is_win32_sep(path[j]) {
        j += 1;
    }
    Some(UncRoot { first, second_start, end: j })
}

/// O dispositivo `\\servidor\compartilhamento` montado com barras invertidas.
fn unc_device(path: &[u16], unc: &UncRoot) -> Vec<u16> {
    [&[BACKSLASH, BACKSLASH][..], &path[unc.first.0..unc.first.1], &[BACKSLASH], &path[unc.second_start..unc.end]].concat()
}

/// `win32.isAbsolute`.
pub(crate) fn win32_is_absolute_units(path: &[u16]) -> bool {
    match path {
        [] => false,
        [first, ..] if is_win32_sep(*first) => true,
        [first, COLON, third, ..] => is_device_root(*first) && is_win32_sep(*third),
        _ => false,
    }
}

/// `win32.normalize` sobre unidades.
pub(crate) fn win32_normalize_units(path: &[u16]) -> Vec<u16> {
    let len = path.len();
    if len == 0 {
        return vec![DOT];
    }
    let code = path[0];
    if len == 1 {
        return if code == SLASH { vec![BACKSLASH] } else { path.to_vec() };
    }
    let mut root_end = 0;
    let mut device: Vec<u16> = Vec::new();
    let mut absolute = false;
    if is_win32_sep(code) {
        absolute = true;
        if is_win32_sep(path[1]) {
            if let Some(unc) = scan_unc_root(path) {
                if unc.end == len {
                    // Só a raiz UNC: `\\servidor\compartilhamento\`.
                    return [&[BACKSLASH, BACKSLASH][..], &path[unc.first.0..unc.first.1], &[BACKSLASH], &path[unc.second_start..], &[BACKSLASH]].concat();
                }
                device = unc_device(path, &unc);
                root_end = unc.end;
            }
        } else {
            root_end = 1;
        }
    } else if is_device_root(code) && path[1] == COLON {
        device = path[..2].to_vec();
        root_end = 2;
        if len > 2 && is_win32_sep(path[2]) {
            absolute = true;
            root_end = 3;
        }
    }
    let mut tail = if root_end < len { normalize_string(&path[root_end..], !absolute, WIN32) } else { Vec::new() };
    if tail.is_empty() && !absolute {
        tail = vec![DOT];
    }
    if !tail.is_empty() && is_win32_sep(path[len - 1]) {
        tail.push(BACKSLASH);
    }
    let prefix: Vec<u16> = match (device.is_empty(), absolute) {
        (true, true) => vec![BACKSLASH],
        (true, false) => Vec::new(),
        (false, true) => [device.as_slice(), &[BACKSLASH]].concat(),
        (false, false) => device,
    };
    [prefix, tail].concat()
}

/// `win32.join` sobre as unidades já validadas.
pub(crate) fn win32_join_units(parts: &[Vec<u16>]) -> Vec<u16> {
    let Some(first) = parts.iter().find(|part| !part.is_empty()) else {
        return vec![DOT];
    };
    let mut joined = concat_non_empty(parts, BACKSLASH);
    // Evita que a junção de duas barras no começo vire, sem querer, uma raiz UNC.
    let mut needs_replace = true;
    let mut slash_count = 0;
    if is_win32_sep(first[0]) {
        slash_count += 1;
        if first.len() > 1 && is_win32_sep(first[1]) {
            slash_count += 1;
            if first.len() > 2 {
                if is_win32_sep(first[2]) {
                    slash_count += 1;
                } else {
                    needs_replace = false;
                }
            }
        }
    }
    if needs_replace {
        while slash_count < joined.len() && is_win32_sep(joined[slash_count]) {
            slash_count += 1;
        }
        if slash_count >= 2 {
            joined = [&[BACKSLASH][..], &joined[slash_count..]].concat();
        }
    }
    win32_normalize_units(&joined)
}

/// `win32.resolve` sobre caminhos já validados (do primeiro ao último) e o diretório corrente.
pub(crate) fn win32_resolve_units(parts: &[Vec<u16>], cwd: &[u16]) -> Vec<u16> {
    let mut resolved_device: Vec<u16> = Vec::new();
    let mut resolved_tail: Vec<u16> = Vec::new();
    let mut resolved_absolute = false;
    for index in (-1..parts.len() as isize).rev() {
        let path: Vec<u16> = if index >= 0 {
            let part = &parts[index as usize];
            if part.is_empty() {
                continue;
            }
            part.clone()
        } else if resolved_device.is_empty() {
            cwd.to_vec()
        } else if lower_units(&cwd[..cwd.len().min(2)]) != lower_units(&resolved_device) && cwd.get(2) == Some(&BACKSLASH) {
            // O diretório corrente não está na unidade pedida: parte da raiz dela.
            [resolved_device.as_slice(), &[BACKSLASH]].concat()
        } else {
            cwd.to_vec()
        };
        let len = path.len();
        let code = path.first().copied().unwrap_or(0);
        let mut root_end = 0;
        let mut device: Vec<u16> = Vec::new();
        let mut absolute = false;
        if len == 1 {
            if is_win32_sep(code) {
                root_end = 1;
                absolute = true;
            }
        } else if is_win32_sep(code) {
            absolute = true;
            if is_win32_sep(path[1]) {
                if let Some(unc) = scan_unc_root(&path) {
                    device = unc_device(&path, &unc);
                    root_end = unc.end;
                }
            } else {
                root_end = 1;
            }
        } else if len > 1 && is_device_root(code) && path[1] == COLON {
            device = path[..2].to_vec();
            root_end = 2;
            if len > 2 && is_win32_sep(path[2]) {
                absolute = true;
                root_end = 3;
            }
        }
        if !device.is_empty() {
            if resolved_device.is_empty() {
                resolved_device = device;
            } else if lower_units(&device) != lower_units(&resolved_device) {
                // Outra unidade: este caminho não se aplica.
                continue;
            }
        }
        if resolved_absolute {
            if !resolved_device.is_empty() {
                break;
            }
        } else {
            resolved_tail = [&path[root_end..], &[BACKSLASH][..], &resolved_tail].concat();
            resolved_absolute = absolute;
            if absolute && !resolved_device.is_empty() {
                break;
            }
        }
    }
    let tail = normalize_string(&resolved_tail, !resolved_absolute, WIN32);
    if resolved_absolute {
        return [resolved_device.as_slice(), &[BACKSLASH], &tail].concat();
    }
    let joined = [resolved_device, tail].concat();
    if joined.is_empty() {
        vec![DOT]
    } else {
        joined
    }
}

/// `win32.relative` sobre caminhos já resolvidos por [`win32_resolve_units`].
pub(crate) fn win32_relative_units(from_orig: &[u16], to_orig: &[u16]) -> Vec<u16> {
    if from_orig == to_orig {
        return Vec::new();
    }
    let from = lower_units(from_orig);
    let to = lower_units(to_orig);
    if from == to {
        return Vec::new();
    }
    let leading = |units: &[u16]| units.iter().take_while(|&&unit| unit == BACKSLASH).count();
    let trimmed_end = |units: &[u16], start: usize| {
        let mut end = units.len();
        while end > start + 1 && units[end - 1] == BACKSLASH {
            end -= 1;
        }
        end
    };
    let from_start = leading(&from);
    let from_end = trimmed_end(&from, from_start);
    let from_len = from_end - from_start;
    let mut to_start = leading(&to);
    let to_end = trimmed_end(&to, to_start);
    let to_len = to_end - to_start;
    let length = from_len.min(to_len);
    let mut last_common_sep: isize = -1;
    let mut index = 0;
    while index < length {
        let from_code = from[from_start + index];
        if from_code != to[to_start + index] {
            break;
        } else if from_code == BACKSLASH {
            last_common_sep = index as isize;
        }
        index += 1;
    }
    if index != length {
        if last_common_sep == -1 {
            return to_orig.to_vec();
        }
    } else {
        if to_len > length {
            if to[to_start + index] == BACKSLASH {
                return js_slice(to_orig, to_start + index + 1, usize::MAX);
            }
            if index == 2 {
                return js_slice(to_orig, to_start + index, usize::MAX);
            }
        }
        if from_len > length {
            if from[from_start + index] == BACKSLASH {
                last_common_sep = index as isize;
            } else if index == 2 {
                last_common_sep = 3;
            }
        }
        if last_common_sep == -1 {
            last_common_sep = 0;
        }
    }
    let mut out: Vec<u16> = Vec::new();
    let mut position = from_start as isize + last_common_sep + 1;
    while position <= from_end as isize {
        if position == from_end as isize || from[position as usize] == BACKSLASH {
            if out.is_empty() {
                out.extend([DOT, DOT]);
            } else {
                out.extend([BACKSLASH, DOT, DOT]);
            }
        }
        position += 1;
    }
    to_start = (to_start as isize + last_common_sep) as usize;
    if !out.is_empty() {
        out.extend(js_slice(to_orig, to_start, to_end));
        return out;
    }
    if to_orig.get(to_start) == Some(&BACKSLASH) {
        to_start += 1;
    }
    js_slice(to_orig, to_start, to_end)
}

/// `win32.dirname`.
pub(crate) fn win32_dirname_units(path: &[u16]) -> Vec<u16> {
    let len = path.len();
    if len == 0 {
        return vec![DOT];
    }
    let code = path[0];
    if len == 1 {
        return if is_win32_sep(code) { path.to_vec() } else { vec![DOT] };
    }
    let mut root_end: isize = -1;
    let mut offset = 0;
    if is_win32_sep(code) {
        root_end = 1;
        offset = 1;
        if is_win32_sep(path[1]) {
            if let Some(unc) = scan_unc_root(path) {
                if unc.end == len {
                    return path.to_vec();
                }
                root_end = unc.end as isize + 1;
                offset = unc.end + 1;
            }
        }
    } else if is_device_root(code) && path[1] == COLON {
        offset = if len > 2 && is_win32_sep(path[2]) { 3 } else { 2 };
        root_end = offset as isize;
    }
    let mut end: isize = -1;
    let mut matched_slash = true;
    for index in (offset..len).rev() {
        if is_win32_sep(path[index]) {
            if !matched_slash {
                end = index as isize;
                break;
            }
        } else {
            matched_slash = false;
        }
    }
    if end == -1 {
        if root_end == -1 {
            return vec![DOT];
        }
        end = root_end;
    }
    path[..end as usize].to_vec()
}

/// `win32.parse`.
pub(crate) fn win32_parse_units(path: &[u16]) -> ParsedPath {
    let mut parsed = ParsedPath::default();
    let len = path.len();
    if len == 0 {
        return parsed;
    }
    let code = path[0];
    if len == 1 {
        if is_win32_sep(code) {
            parsed.root = path.to_vec();
            parsed.dir = path.to_vec();
        } else {
            parsed.base = path.to_vec();
            parsed.name = path.to_vec();
        }
        return parsed;
    }
    let mut root_end = 0;
    if is_win32_sep(code) {
        root_end = 1;
        if is_win32_sep(path[1]) {
            if let Some(unc) = scan_unc_root(path) {
                root_end = if unc.end == len { unc.end } else { unc.end + 1 };
            }
        }
    } else if is_device_root(code) && path[1] == COLON {
        if len <= 2 {
            parsed.root = path.to_vec();
            parsed.dir = path.to_vec();
            return parsed;
        }
        root_end = 2;
        if is_win32_sep(path[2]) {
            if len == 3 {
                parsed.root = path.to_vec();
                parsed.dir = path.to_vec();
                return parsed;
            }
            root_end = 3;
        }
    }
    if root_end > 0 {
        parsed.root = path[..root_end].to_vec();
    }
    let (start_dot, start_part, end, pre_dot_state) = scan_extension(path, root_end, root_end, WIN32);
    if end != -1 {
        let end = end as usize;
        if has_extension(start_dot, start_part, end as isize, pre_dot_state) {
            parsed.name = path[start_part..start_dot as usize].to_vec();
            parsed.base = path[start_part..end].to_vec();
            parsed.ext = path[start_dot as usize..end].to_vec();
        } else {
            parsed.base = path[start_part..end].to_vec();
            parsed.name = parsed.base.clone();
        }
    }
    parsed.dir = if start_part > 0 && start_part != root_end { path[..start_part - 1].to_vec() } else { parsed.root.clone() };
    parsed
}

/// `win32.toNamespacedPath` sobre o caminho já resolvido: `Some` com o prefixo `\\?\`, `None` quando o argumento
/// original volta como veio (curto demais, `\\?\`, `\\.\` ou sem raiz de unidade).
pub(crate) fn win32_namespaced_units(resolved: &[u16]) -> Option<Vec<u16>> {
    if resolved.len() <= 2 {
        return None;
    }
    if resolved[0] == BACKSLASH {
        if resolved[1] == BACKSLASH && resolved[2] != QUESTION && resolved[2] != DOT {
            let prefix: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
            return Some([prefix, resolved[2..].to_vec()].concat());
        }
    } else if is_device_root(resolved[0]) && resolved[1] == COLON && resolved[2] == BACKSLASH {
        return Some([&[BACKSLASH, BACKSLASH, QUESTION, BACKSLASH][..], resolved].concat());
    }
    None
}

/// Os argumentos de `join`/`resolve`, validados em ordem (`paths[i]`).
fn path_list(global_object: &JSGlobalObject, call: &HostCall) -> Result<Vec<Vec<u16>>, Thrown> {
    call.arguments().iter().enumerate().map(|(index, &value)| string_argument(global_object, value, &format!("paths[{index}]"))).collect()
}

fn style_of<const WIN: bool>() -> Style {
    if WIN {
        WIN32
    } else {
        POSIX
    }
}

fn join_body<const WIN: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let parts = path_list(global_object, call)?;
    let joined = if WIN { win32_join_units(&parts) } else { join_units(&parts) };
    Ok(units_value(global_object, &joined))
}

fn cwd_units() -> Vec<u16> {
    current_directory().encode_utf16().collect()
}

/// `resolve` de qualquer estilo sobre caminhos já validados.
fn resolve_for<const WIN: bool>(parts: &[Vec<u16>]) -> Vec<u16> {
    if WIN {
        win32_resolve_units(parts, &cwd_units())
    } else {
        resolve_units(parts, &cwd_units())
    }
}

/// O posix valida de trás para frente e só enquanto não achou caminho absoluto, como o laço do Node; o win32 do bun
/// valida todos os argumentos (o laço de raiz acontece depois, sobre os já validados).
fn resolve_body<const WIN: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let arguments = call.arguments();
    let mut collected: Vec<Vec<u16>> = Vec::new();
    for index in (0..arguments.len()).rev() {
        let path = string_argument(global_object, arguments[index], &format!("paths[{index}]"))?;
        let absolute = !WIN && path.first() == Some(&SLASH);
        collected.push(path);
        if absolute {
            break;
        }
    }
    collected.reverse();
    Ok(units_value(global_object, &resolve_for::<WIN>(&collected)))
}

fn normalize_body<const WIN: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let path = string_argument(global_object, call.argument(0), "path")?;
    let normalized = if WIN { win32_normalize_units(&path) } else { normalize_units(&path) };
    Ok(units_value(global_object, &normalized))
}

fn is_absolute_body<const WIN: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let path = string_argument(global_object, call.argument(0), "path")?;
    Ok(js_boolean(if WIN { win32_is_absolute_units(&path) } else { path.first() == Some(&SLASH) }))
}

fn relative_body<const WIN: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let from = string_argument(global_object, call.argument(0), "from")?;
    let to = string_argument(global_object, call.argument(1), "to")?;
    if from == to {
        return Ok(units_value(global_object, &[]));
    }
    let from = resolve_for::<WIN>(&[from]);
    let to = resolve_for::<WIN>(&[to]);
    let relative = if WIN { win32_relative_units(&from, &to) } else { relative_units(&from, &to) };
    Ok(units_value(global_object, &relative))
}

/// `toNamespacedPath`: no posix devolve o argumento como veio, qualquer que seja o tipo; no win32 só um caminho
/// resolvido com raiz de unidade ou UNC ganha o prefixo `\\?\`, o resto (e o que não é string) volta como veio.
fn to_namespaced_path_body<const WIN: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let argument = call.argument(0);
    if !WIN || !argument.is_string() {
        return Ok(argument);
    }
    let path = units_of(&argument.to_wtf_string());
    if path.is_empty() {
        return Ok(argument);
    }
    // Medido no bun 1.4.2: o que não é caminho de unidade nem UNC volta resolvido (`win32.toNamespacedPath("a/b")` é
    // `\tmp\a\b` com o diretório corrente `/tmp`), e não como veio, que é o que o Node devolve.
    let resolved = resolve_for::<WIN>(&[path]);
    match win32_namespaced_units(&resolved) {
        Some(namespaced) => Ok(units_value(global_object, &namespaced)),
        None if resolved.len() <= 2 => Ok(argument),
        None => Ok(units_value(global_object, &resolved)),
    }
}

fn dirname_body<const WIN: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let path = string_argument(global_object, call.argument(0), "path")?;
    let dirname = if WIN { win32_dirname_units(&path) } else { dirname_units(&path) };
    Ok(units_value(global_object, &dirname))
}

fn basename_body<const WIN: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let path = string_argument(global_object, call.argument(0), "path")?;
    let suffix_value = call.argument(1);
    let suffix = if suffix_value.is_undefined() { None } else { Some(string_argument(global_object, suffix_value, "ext")?) };
    let basename = if WIN { win32_basename_units(&path, suffix.as_deref()) } else { basename_units(&path, suffix.as_deref()) };
    Ok(units_value(global_object, &basename))
}

fn extname_body<const WIN: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let path = string_argument(global_object, call.argument(0), "path")?;
    let extname = if WIN { win32_extname_units(&path) } else { extname_units(&path) };
    Ok(units_value(global_object, &extname))
}

fn parse_body<const WIN: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let path = string_argument(global_object, call.argument(0), "path")?;
    let parsed = if WIN { win32_parse_units(&path) } else { parse_units(&path) };
    let vm = global_object.vm();
    let result = construct_empty_object(global_object);
    for (key, value) in [("root", &parsed.root), ("dir", &parsed.dir), ("base", &parsed.base), ("ext", &parsed.ext), ("name", &parsed.name)] {
        result.put_direct(vm, &property_key(vm, key), units_value(global_object, value), 0);
    }
    Ok(result.as_value())
}

/// Um campo de `format`: falso (ausente, `""`, `0`, `null`...) vira vazio, o resto passa por `ToString`.
fn format_field(global_object: &JSGlobalObject, object: JSValue, name: &str) -> Result<Vec<u16>, Thrown> {
    let value = get_object_property(global_object, object, &Identifier::from_span(global_object.vm(), name.as_bytes()))?;
    if !value.to_boolean() {
        return Ok(Vec::new());
    }
    Ok(units_of(&value.to_wtf_string()))
}

fn format_body<const WIN: bool>(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let object = call.argument(0);
    // Medido no bun 1.4.2: array é recusado (`got array`) e função passa (`format(() => 1)` devolve `""`).
    if !object.is_object() || is_js_array(&object) {
        return Err(invalid_property_type(global_object, "pathObject", "object", object));
    }
    let dir = format_field(global_object, object, "dir")?;
    let root = format_field(global_object, object, "root")?;
    let base = format_field(global_object, object, "base")?;
    let name = format_field(global_object, object, "name")?;
    let ext = format_field(global_object, object, "ext")?;
    Ok(units_value(global_object, &format_units(style_of::<WIN>(), &dir, &root, &base, &name, &ext)))
}

host_function!(posix_join, join_body::<false>);
host_function!(posix_resolve, resolve_body::<false>);
host_function!(posix_normalize, normalize_body::<false>);
host_function!(posix_is_absolute, is_absolute_body::<false>);
host_function!(posix_relative, relative_body::<false>);
host_function!(posix_to_namespaced_path, to_namespaced_path_body::<false>);
host_function!(posix_dirname, dirname_body::<false>);
host_function!(posix_basename, basename_body::<false>);
host_function!(posix_extname, extname_body::<false>);
host_function!(posix_format, format_body::<false>);
host_function!(posix_parse, parse_body::<false>);
host_function!(win32_join, join_body::<true>);
host_function!(win32_resolve, resolve_body::<true>);
host_function!(win32_normalize, normalize_body::<true>);
host_function!(win32_is_absolute, is_absolute_body::<true>);
host_function!(win32_relative, relative_body::<true>);
host_function!(win32_to_namespaced_path, to_namespaced_path_body::<true>);
host_function!(win32_dirname, dirname_body::<true>);
host_function!(win32_basename, basename_body::<true>);
host_function!(win32_extname, extname_body::<true>);
host_function!(win32_format, format_body::<true>);
host_function!(win32_parse, parse_body::<true>);

type PathFunction = crate::runtime::native_function::NativeFunction;

/// Um dos dois objetos (`posix` ou `win32`): as onze funções, `sep` e `delimiter`, na ordem do bun. As chaves cruzadas
/// (`win32`, `posix`, `_makeLong`) entram depois, em [`install_path_module`], quando os dois já existem.
fn create_flavor(global_object: &JSGlobalObject, win: bool, functions: [PathFunction; 11]) -> crate::runtime::js_object::JSObjectRef {
    let vm = global_object.vm();
    let module = construct_empty_object(global_object);
    let names = ["resolve", "normalize", "isAbsolute", "join", "relative", "toNamespacedPath", "dirname", "basename", "extname", "format", "parse"];
    for (name, function) in names.into_iter().zip(functions) {
        let installed = put_direct_native_function_with_display_name(
            vm,
            global_object,
            &module,
            &Identifier::from_span(vm, name.as_bytes()),
            &WtfString::from_latin1(name.as_bytes()),
            1,
            function,
            ImplementationVisibility::Public,
            Intrinsic::NoIntrinsic,
            0,
        );
        // Medido no bun 1.4.2: `path.join.name` é `bound join`, mas `String(path.join)` é `function join() { [native code] }`
        // (o nome do executável). O `name` próprio troca de valor sem mudar de lugar nas chaves (`length,name`); o
        // `define_own_property` da função é o caminho que enxerga a propriedade `name` da função nativa.
        let bound_name = PropertyDescriptor::new(text_value(vm, &format!("bound {name}")), DONT_ENUM | READ_ONLY);
        let _ = installed.define_own_property(global_object, &PropertyName::from_identifier(&vm.property_names.name), &bound_name, false);
    }
    module.put_direct(vm, &property_key(vm, "sep"), text_value(vm, if win { "\\" } else { "/" }), 0);
    module.put_direct(vm, &property_key(vm, "delimiter"), text_value(vm, if win { ";" } else { ":" }), 0);
    module
}

/// O objeto do módulo `node:path` (o posix, que é o `path` em si) com as chaves na ordem do bun. `matchesGlob` ainda
/// não existe (ver a documentação do módulo). É o instalador de `require("path")`. Os dois objetos apontam um para o outro: `posix.win32 === win32`,
/// `win32.posix === posix`, e cada um aponta para si mesmo pela própria chave.
pub(crate) fn install_path_module(global_object: &JSGlobalObject) -> HostResult {
    let vm = global_object.vm();
    let posix = create_flavor(
        global_object,
        false,
        [posix_resolve, posix_normalize, posix_is_absolute, posix_join, posix_relative, posix_to_namespaced_path, posix_dirname, posix_basename, posix_extname, posix_format, posix_parse],
    );
    let win32 = create_flavor(
        global_object,
        true,
        [win32_resolve, win32_normalize, win32_is_absolute, win32_join, win32_relative, win32_to_namespaced_path, win32_dirname, win32_basename, win32_extname, win32_format, win32_parse],
    );
    for module in [&posix, &win32] {
        module.put_direct(vm, &property_key(vm, "win32"), win32.as_value(), 0);
        module.put_direct(vm, &property_key(vm, "posix"), posix.as_value(), 0);
        // `_makeLong` é a mesma função de `toNamespacedPath`.
        let namespaced = module.get(vm, &property_key(vm, "toNamespacedPath"));
        module.put_direct(vm, &property_key(vm, "_makeLong"), namespaced, 0);
    }
    Ok(posix.as_value())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn s(units: Vec<u16>) -> String {
        String::from_utf16(&units).unwrap()
    }

    fn join(parts: &[&str]) -> String {
        s(join_units(&parts.iter().map(|part| u(part)).collect::<Vec<_>>()))
    }

    fn resolve(parts: &[&str]) -> String {
        s(resolve_units(&parts.iter().map(|part| u(part)).collect::<Vec<_>>(), &u("/tmp")))
    }

    fn relative(from: &str, to: &str) -> String {
        s(relative_units(&u(&resolve(&[from])), &u(&resolve(&[to]))))
    }

    #[test]
    fn join_and_normalize() {
        assert_eq!(join(&[]), ".");
        assert_eq!(join(&["a", "", "b/", "../c"]), "a/c");
        assert_eq!(join(&["/a", "..", ".."]), "/");
        assert_eq!(join(&["..", "a"]), "../a");
        assert_eq!(s(normalize_units(&u("./"))), "./");
        assert_eq!(s(normalize_units(&u(""))), ".");
        assert_eq!(s(normalize_units(&u("/a//b/./c/.."))), "/a/b");
        assert_eq!(s(normalize_units(&u("a/../../b/"))), "../b/");
    }

    #[test]
    fn resolve_uses_cwd_and_stops_at_absolute() {
        assert_eq!(resolve(&[]), "/tmp");
        assert_eq!(resolve(&["a", "../b"]), "/tmp/b");
        assert_eq!(resolve(&["/x", "y", "/z", "w"]), "/z/w");
        assert_eq!(resolve(&["/"]), "/");
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative("/a/b/c", "/a/d"), "../../d");
        assert_eq!(relative("/a", "/a/b/c"), "b/c");
        assert_eq!(relative("/a/b", "/a"), "..");
        assert_eq!(relative("/", "/a"), "a");
        assert_eq!(relative("/a", "/"), "..");
        assert_eq!(relative("/aaa/bbb", "/aaa/bbbb"), "../bbbb");
        assert_eq!(relative("/a/b", "/a/b"), "");
    }

    #[test]
    fn dirname_basename_extname() {
        assert_eq!(s(dirname_units(&u("/a/b/c"))), "/a/b");
        assert_eq!(s(dirname_units(&u("/a"))), "/");
        assert_eq!(s(dirname_units(&u("a"))), ".");
        assert_eq!(s(dirname_units(&u(""))), ".");
        assert_eq!(s(dirname_units(&u("//a"))), "//");
        assert_eq!(s(basename_units(&u("/a/b.txt"), None)), "b.txt");
        assert_eq!(s(basename_units(&u("/a/b.txt"), Some(&u(".txt")))), "b");
        assert_eq!(s(basename_units(&u("/a/b"), Some(&u("b")))), "b");
        assert_eq!(s(basename_units(&u("b"), Some(&u("b")))), "");
        assert_eq!(s(basename_units(&u("/a/b/"), None)), "b");
        assert_eq!(s(extname_units(&u("a.tar.gz"))), ".gz");
        assert_eq!(s(extname_units(&u(".bashrc"))), "");
        assert_eq!(s(extname_units(&u("a."))), ".");
        assert_eq!(s(extname_units(&u(".."))), "");
    }

    #[test]
    fn parse_and_format() {
        let parsed = parse_units(&u("/a/b.tar.gz"));
        assert_eq!((s(parsed.root), s(parsed.dir), s(parsed.base), s(parsed.ext), s(parsed.name)), ("/".into(), "/a".into(), "b.tar.gz".into(), ".gz".into(), "b.tar".into()));
        assert_eq!(parse_units(&u("")), ParsedPath::default());
        let top = parse_units(&u("/a"));
        assert_eq!((s(top.dir), s(top.base)), ("/".into(), "a".into()));
        let f = |dir: &str, root: &str, base: &str, name: &str, ext: &str| s(format_units(POSIX, &u(dir), &u(root), &u(base), &u(name), &u(ext)));
        assert_eq!(f("", "", "a", "", ""), "a");
        assert_eq!(f("", "/", "a", "", ""), "/a");
        assert_eq!(f("/x", "/", "a", "", ""), "/x/a");
        assert_eq!(f("", "", "", "n", "e"), "n.e");
        assert_eq!(f("1", "", "", "", ""), "1/");
    }

    fn w_norm(path: &str) -> String {
        s(win32_normalize_units(&u(path)))
    }

    fn w_join(parts: &[&str]) -> String {
        s(win32_join_units(&parts.iter().map(|part| u(part)).collect::<Vec<_>>()))
    }

    fn w_resolve(parts: &[&str]) -> String {
        s(win32_resolve_units(&parts.iter().map(|part| u(part)).collect::<Vec<_>>(), &u("/tmp")))
    }

    fn w_relative(from: &str, to: &str) -> String {
        s(win32_relative_units(&u(&w_resolve(&[from])), &u(&w_resolve(&[to]))))
    }

    #[test]
    fn win32_normalize_and_join() {
        assert_eq!(w_norm("a:b"), "a:b");
        assert_eq!(w_norm("x\\a:b\\.."), "x");
        assert_eq!(w_norm("/"), "\\");
        assert_eq!(w_norm("//"), "\\");
        assert_eq!(w_norm("//a"), "\\a");
        assert_eq!(w_norm("//a/b"), "\\\\a\\b\\");
        assert_eq!(w_norm("//a//b//c"), "\\\\a\\b\\c");
        assert_eq!(w_norm("C:\\"), "C:\\");
        assert_eq!(w_norm("c:"), "c:.");
        assert_eq!(w_norm("."), ".");
        assert_eq!(w_norm("a\\"), "a\\");
        assert_eq!(w_norm("C:/a/../b/./c//"), "C:\\b\\c\\");
        assert_eq!(w_join(&[]), ".");
        assert_eq!(w_join(&["//a", "b"]), "\\\\a\\b\\");
        assert_eq!(w_join(&["//", "a"]), "\\a");
        assert_eq!(w_join(&["\\\\", "a", "b"]), "\\a\\b");
        assert_eq!(w_join(&["a", "", "b"]), "a\\b");
    }

    #[test]
    fn win32_resolve_and_relative() {
        assert_eq!(w_resolve(&[]), "\\tmp");
        assert_eq!(w_resolve(&["C:a"]), "C:\\tmp\\a");
        assert_eq!(w_resolve(&["C:\\a", "b"]), "C:\\a\\b");
        assert_eq!(w_resolve(&["C:\\a", "D:\\b", "c"]), "D:\\b\\c");
        assert_eq!(w_resolve(&["\\\\srv\\sh\\a", "..\\b"]), "\\\\srv\\sh\\b");
        assert_eq!(w_relative("C:\\a", "D:\\b"), "D:\\b");
        assert_eq!(w_relative("C:\\a\\b", "C:\\a\\c"), "..\\c");
        assert_eq!(w_relative("C:\\a\\B", "c:\\a\\b\\c"), "c");
        assert_eq!(w_relative("\\\\s\\h\\a", "\\\\s\\h\\b"), "..\\b");
        assert_eq!(w_relative("C:\\a", "C:\\a"), "");
    }

    #[test]
    fn win32_dirname_basename_extname() {
        let dirname = |path: &str| s(win32_dirname_units(&u(path)));
        assert_eq!(dirname("C:\\a\\b"), "C:\\a");
        assert_eq!(dirname("C:"), "C:");
        assert_eq!(dirname("\\srv"), "\\");
        assert_eq!(dirname("\\\\srv\\sh\\a"), "\\\\srv\\sh\\");
        assert_eq!(dirname("//srv/share/x/../y"), "//srv/share/x/..");
        assert_eq!(dirname("a"), ".");
        assert_eq!(s(win32_basename_units(&u("C:foo.txt"), Some(&u(".txt")))), "foo");
        assert_eq!(s(win32_basename_units(&u("C:"), None)), "");
        assert_eq!(s(win32_basename_units(&u("C:\\a\\b.c"), None)), "b.c");
        assert_eq!(s(win32_extname_units(&u("C:.a"))), "");
        assert_eq!(s(win32_extname_units(&u("C:\\a\\b.tar.gz"))), ".gz");
    }

    #[test]
    fn win32_parse_absolute_and_namespace() {
        let fields = |parsed: ParsedPath| (s(parsed.root), s(parsed.dir), s(parsed.base), s(parsed.ext), s(parsed.name));
        let expected = |root: &str, dir: &str, base: &str, ext: &str, name: &str| (root.to_string(), dir.to_string(), base.to_string(), ext.to_string(), name.to_string());
        assert_eq!(fields(win32_parse_units(&u("C:\\a\\b.c"))), expected("C:\\", "C:\\a", "b.c", ".c", "b"));
        assert_eq!(fields(win32_parse_units(&u("\\\\s\\h"))), expected("\\\\s\\h", "\\\\s\\h", "", "", ""));
        assert_eq!(fields(win32_parse_units(&u("C:"))), expected("C:", "C:", "", "", ""));
        assert_eq!(win32_parse_units(&u("")), ParsedPath::default());
        assert!(!win32_is_absolute_units(&u("C:")));
        assert!(win32_is_absolute_units(&u("C:/")));
        assert!(win32_is_absolute_units(&u("\\a")));
        assert!(!win32_is_absolute_units(&u("a")));
        let namespaced = |resolved: &str| win32_namespaced_units(&u(resolved)).map(s);
        assert_eq!(namespaced("C:\\a\\b").as_deref(), Some("\\\\?\\C:\\a\\b"));
        assert_eq!(namespaced("\\\\srv\\sh\\a").as_deref(), Some("\\\\?\\UNC\\srv\\sh\\a"));
        assert_eq!(namespaced("\\\\?\\C:\\a"), None);
        assert_eq!(namespaced("\\a"), None);
        assert_eq!(s(format_units(WIN32, &u("C:\\x"), &[], &u("a"), &[], &[])), "C:\\x\\a");
    }
}
