//! Porte de `src/vs/base/common/extpath.ts`: utilitários de caminho que complementam o módulo `path`
//! (raiz, UNC, nome de arquivo válido, comparação de caminhos, `arquivo:linha:coluna`).
//!
//! O original lê `isWindows` e o módulo `path` da plataforma. Aqui isso vem do [`Environment`]
//! injetado (plataforma e diretório atual), como no [`crate::path`]. Os caminhos são `&str`: os
//! separadores são todos ASCII, então os índices em bytes equivalem aos índices em unidades UTF-16 do
//! original; onde o original usa `.length` ou `indexOf` com resultado devolvido ao chamador, o código
//! conta unidades UTF-16.

use crate::char_code as cc;
use crate::errors::{self, Error};
use crate::js_string::{self, is_js_whitespace};
use crate::random::random_f64;
use crate::path::{Environment, PathModule, Platform};
use crate::strings;
use crate::JsString;

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn is_windows(env: &dyn Environment) -> bool {
    env.platform() == Platform::Win32
}

pub fn is_path_separator(code: u16) -> bool {
    code == cc::Slash || code == cc::Backslash
}

fn is_sep_byte(byte: Option<&u8>) -> bool {
    byte.is_some_and(|&b| u16::from(b) == cc::Slash || u16::from(b) == cc::Backslash)
}

/// `/[\\/]/g` trocado por `replacement`.
fn replace_separators(path: &str, replacement: &str) -> String {
    path.replace(['\\', '/'], replacement)
}

/// Takes a Windows OS path and changes backward slashes to forward slashes.
/// This should only be done for OS paths from Windows (or user provided paths potentially from Windows).
/// Using it on a Linux or MaxOS path might change it.
pub fn to_slashes(os_path: &str) -> String {
    replace_separators(os_path, "/")
}

/// Takes a Windows OS path (using backward or forward slashes) and turns it into a posix path:
/// - turns backward slashes into forward slashes
/// - makes it absolute if it starts with a drive letter
pub fn to_posix_path(os_path: &str) -> String {
    let mut os_path = os_path.to_string();
    if !os_path.contains('/') {
        os_path = to_slashes(&os_path);
    }
    let bytes = os_path.as_bytes();
    // `/^[a-zA-Z]:(\/|$)/`
    if bytes.first().is_some_and(u8::is_ascii_alphabetic) && bytes.get(1) == Some(&b':') && (bytes.len() == 2 || bytes[2] == b'/') {
        os_path = format!("/{os_path}");
    }
    os_path
}

/// Computes the _root_ this path, like `getRoot('c:\files') === c:\`,
/// `getRoot('files:///files/path') === files:///`,
/// or `getRoot('\\server\shares\path') === \\server\shares\`
pub fn get_root(path: &str, sep: &str) -> String {
    if path.is_empty() {
        return String::new();
    }

    let bytes = path.as_bytes();
    let len = bytes.len();
    let first_letter = u16::from(bytes[0]);
    if is_path_separator(first_letter) {
        if is_sep_byte(bytes.get(1)) {
            // UNC candidate \\localhost\shares\ddd
            //               ^^^^^^^^^^^^^^^^^^^
            if !is_sep_byte(bytes.get(2)) {
                let mut pos = 3;
                let start = pos;
                while pos < len && !is_sep_byte(bytes.get(pos)) {
                    pos += 1;
                }
                if start != pos && !is_sep_byte(bytes.get(pos + 1)) {
                    pos += 1;
                    while pos < len {
                        if is_sep_byte(bytes.get(pos)) {
                            // consume this separator
                            return replace_separators(&path[..pos + 1], sep);
                        }
                        pos += 1;
                    }
                }
            }
        }

        // /user/far
        // ^
        return sep.to_string();
    } else if is_windows_drive_letter(i32::from(first_letter)) && bytes.get(1) == Some(&b':') {
        // check for windows drive letter c:\ or c:
        return if is_sep_byte(bytes.get(2)) {
            // C:\fff
            // ^^^
            format!("{}{}", &path[..2], sep)
        } else {
            // C:
            // ^^
            path[..2].to_string()
        };
    }

    // check for URI
    // scheme://authority/path
    // ^^^^^^^^^^^^^^^^^^^
    if let Some(found) = path.find("://") {
        let mut pos = found + 3; // 3 -> "://".length
        while pos < len {
            if is_sep_byte(bytes.get(pos)) {
                return path[..pos + 1].to_string(); // consume this separator
            }
            pos += 1;
        }
    }

    String::new()
}

/// Check if the path follows this pattern: `\\hostname\sharename`.
///
/// @see https://msdn.microsoft.com/en-us/library/gg465305.aspx
/// @return A boolean indication if the path is a UNC path, on none-windows always false.
pub fn is_unc(path: &str, env: &dyn Environment) -> bool {
    if !is_windows(env) {
        // UNC is a windows concept
        return false;
    }

    let units = units(path);
    if units.len() < 5 {
        // at least \\a\b
        return false;
    }
    if units[0] != cc::Backslash || units[1] != cc::Backslash {
        return false;
    }

    let start = 2;
    let pos = units[start..].iter().position(|&u| u == cc::Backslash).map_or(units.len(), |p| p + start);
    if start == pos {
        return false;
    }

    // `isNaN(code)` (fim do texto) ou outra barra invertida
    !matches!(units.get(pos + 1), None | Some(&cc::Backslash))
}

/// `/[\\/:\*\?"<>\|]/` (Windows) e `/[/]/` (demais).
fn has_invalid_file_char(name: &str, is_windows_os: bool) -> bool {
    if is_windows_os {
        name.contains(['\\', '/', ':', '*', '?', '"', '<', '>', '|'])
    } else {
        name.contains('/')
    }
}

/// `/^(con|prn|aux|clock\$|nul|lpt[0-9]|com[0-9])(\.(.*?))?$/i`.
fn is_windows_forbidden_name(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    let (base, rest) = match lowered.split_once('.') {
        Some((base, rest)) => (base, Some(rest)),
        None => (lowered.as_str(), None),
    };
    let reserved = matches!(base, "con" | "prn" | "aux" | "clock$" | "nul")
        || (base.len() == 4 && (base.starts_with("lpt") || base.starts_with("com")) && base.as_bytes()[3].is_ascii_digit());
    // O `.` do trecho `(.*?)` não casa terminadores de linha.
    reserved && rest.is_none_or(|rest| !rest.contains(['\n', '\r', '\u{2028}', '\u{2029}']))
}

/// Reference: https://en.wikipedia.org/wiki/Filename
pub fn is_valid_basename(name: Option<&str>, is_windows_os: bool) -> bool {
    let Some(name) = name else {
        return false;
    };
    let name_units = units(name);
    if name_units.is_empty() || name_units.iter().all(|&u| is_js_whitespace(u)) {
        return false; // require a name that is not just whitespace
    }

    if has_invalid_file_char(name, is_windows_os) {
        return false; // check for certain invalid file characters
    }

    if is_windows_os && is_windows_forbidden_name(name) {
        return false; // check for certain invalid file names
    }

    if name == "." || name == ".." {
        return false; // check for reserved values
    }

    if is_windows_os && name_units.last() == Some(&cc::Period) {
        return false; // Windows: file cannot end with a "."
    }

    if is_windows_os && name_units.len() != js_string::trim(&name_units).len() {
        return false; // Windows: file cannot end with a whitespace
    }

    if name_units.len() > 255 {
        return false; // most file systems do not allow files > 255 length
    }

    true
}

/// @deprecated please use `IUriIdentityService.extUri.isEqual` instead.
pub fn is_equal(path_a: &str, path_b: &str, ignore_case: bool) -> bool {
    let identity_equals = path_a == path_b;
    if !ignore_case || identity_equals {
        return identity_equals;
    }

    if path_a.is_empty() || path_b.is_empty() {
        return false;
    }

    strings::equals_ignore_case(&units(path_a), &units(path_b))
}

/// @deprecated please use `IUriIdentityService.extUri.isEqualOrParent` instead.
pub fn is_equal_or_parent(base: &str, parent_candidate: &str, ignore_case: bool, force_posix_semantics: bool, env: &dyn Environment) -> bool {
    let module = if force_posix_semantics { PathModule::posix(env) } else { PathModule::native(env) };
    let separator = u16::from(module.sep().as_bytes()[0]);

    if base == parent_candidate {
        return true;
    }

    if base.is_empty() || parent_candidate.is_empty() {
        return false;
    }

    let (mut base, mut parent_candidate) = (base.to_string(), parent_candidate.to_string());
    if base.contains("..") || parent_candidate.contains("..") {
        base = module.normalize(&base);
        parent_candidate = module.normalize(&parent_candidate);
    }

    let base = units(&base);
    let mut parent_candidate = units(&parent_candidate);

    if parent_candidate.len() > base.len() {
        return false;
    }

    if ignore_case {
        if !strings::starts_with_ignore_case(&base, &parent_candidate) {
            return false;
        }

        if parent_candidate.len() == base.len() {
            return true; // same path, different casing
        }

        let mut sep_offset = parent_candidate.len();
        if parent_candidate.last() == Some(&separator) {
            sep_offset -= 1; // adjust the expected sep offset in case our candidate already ends in separator character
        }

        return base.get(sep_offset) == Some(&separator);
    }

    if parent_candidate.last() != Some(&separator) {
        parent_candidate.push(separator);
    }

    base.starts_with(&parent_candidate)
}

/// `isWindowsDriveLetter`: o argumento é um `number` no original (pode ser `-1` ou `NaN`).
pub fn is_windows_drive_letter(char0: i32) -> bool {
    (i32::from(cc::A)..=i32::from(cc::Z)).contains(&char0) || (i32::from(cc::a)..=i32::from(cc::z)).contains(&char0)
}

pub fn sanitize_file_path(candidate: &str, cwd: &str, env: &dyn Environment) -> String {
    let module = PathModule::native(env);
    let mut candidate = candidate.to_string();

    // Special case: allow to open a drive letter without trailing backslash
    if is_windows(env) && candidate.ends_with(':') {
        candidate.push_str(module.sep());
    }

    // Ensure absolute
    if !module.is_absolute(&candidate) {
        candidate = module.join(&[cwd, &candidate]);
    }

    // Ensure normalized
    candidate = module.normalize(&candidate);

    // Ensure no trailing slash/backslash
    remove_trailing_path_separator(&candidate, env)
}

pub fn remove_trailing_path_separator(candidate: &str, env: &dyn Environment) -> String {
    let sep = PathModule::native(env).sep();
    let mut candidate = JsString::from(candidate);
    candidate = JsString::from(strings::rtrim(candidate.units(), &units(sep)));
    let mut candidate = candidate.to_string_lossy();

    if is_windows(env) {
        // Special case: allow to open drive root ('C:\')
        if candidate.ends_with(':') {
            candidate.push_str(sep);
        }
    } else if candidate.is_empty() {
        // Special case: allow to open root ('/')
        candidate = sep.to_string();
    }

    candidate
}

pub fn is_root_or_drive_letter(path: &str, env: &dyn Environment) -> bool {
    let path_normalized = PathModule::native(env).normalize(path);

    if is_windows(env) {
        if units(path).len() > 3 {
            return false;
        }

        return has_drive_letter(&path_normalized, true)
            && (units(path).len() == 2 || path_normalized.as_bytes().get(2) == Some(&b'\\'));
    }

    path_normalized == "/"
}

pub fn has_drive_letter(path: &str, is_windows_os: bool) -> bool {
    if is_windows_os {
        let units = units(path);
        return units.first().is_some_and(|&u| is_windows_drive_letter(i32::from(u))) && units.get(1) == Some(&cc::Colon);
    }

    false
}

pub fn get_drive_letter(path: &str, is_windows_os: bool) -> Option<char> {
    if has_drive_letter(path, is_windows_os) { path.chars().next() } else { None }
}

/// `indexOfPath`: posição (em unidades UTF-16) de `candidate` em `path`, ou `None` no lugar do `-1`.
pub fn index_of_path(path: &str, candidate: &str, ignore_case: bool) -> Option<usize> {
    let (mut path, mut candidate) = (JsString::from(path), JsString::from(candidate));
    if candidate.len() > path.len() {
        return None;
    }

    if path == candidate {
        return Some(0);
    }

    if ignore_case {
        path = path.to_lower_case();
        candidate = candidate.to_lower_case();
    }

    js_string::index_of(path.units(), candidate.units(), 0)
}

/// `IPathWithLineAndColumn`. Linha e coluna são `number` no original.
#[derive(Clone, Debug, PartialEq)]
pub struct PathWithLineAndColumn {
    pub path: String,
    pub line: Option<f64>,
    pub column: Option<f64>,
}

/// `Number(string)` do JavaScript (conversão de `StringToNumber`), `NaN` quando não é um número.
// TODO(integração): trocar pelo equivalente de `numbers`/`types` quando existir.
fn js_string_to_number(text: &str) -> f64 {
    let trimmed = JsString::from_units(js_string::trim(&units(text)).to_vec()).to_string_lossy();
    if trimmed.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = trimmed.strip_prefix(prefix) {
            return u128::from_str_radix(digits, radix).map_or(f64::NAN, |v| v as f64);
        }
    }
    match trimmed.as_str() {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    // Só dígitos, ponto, expoente e sinal: o `parse` do Rust também aceitaria `inf` e `nan`.
    if !trimmed.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')) {
        return f64::NAN;
    }
    trimmed.parse::<f64>().unwrap_or(f64::NAN)
}

pub fn parse_line_and_column_aware(raw_path: &str) -> PathWithLineAndColumn {
    let segments = raw_path.split(':'); // C:\file.txt:<line>:<column>

    let mut path: Option<String> = None;
    let mut line: Option<f64> = None;
    let mut column: Option<f64> = None;

    for segment in segments {
        let segment_as_number = js_string_to_number(segment);
        if segment_as_number.is_nan() {
            // a colon can well be part of a path (e.g. C:\...)
            path = Some(match path.take() {
                Some(existing) if !existing.is_empty() => format!("{existing}:{segment}"),
                _ => segment.to_string(),
            });
        } else if line.is_none() {
            line = Some(segment_as_number);
        } else if column.is_none() {
            column = Some(segment_as_number);
        }
    }

    let Some(path) = path.filter(|p| !p.is_empty()) else {
        errors::throw(Error::new("Format for `--goto` should be: `FILE:LINE(:COLUMN)`"));
    };

    PathWithLineAndColumn {
        path,
        line,
        // if we have a line, make sure column is also set
        column: column.or(line.map(|_| 1.0)),
    }
}

const PATH_CHARS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const WINDOWS_SAFE_PATH_FIRST_CHARS: &str = "BDEFGHIJKMOQRSTUVWXYZbdefghijkmoqrstuvwxyz0123456789";

pub fn random_path(parent: Option<&str>, prefix: Option<&str>, random_length: usize, env: &dyn Environment) -> String {
    let prefix = prefix.filter(|p| !p.is_empty());
    let mut suffix = String::new();
    for i in 0..random_length {
        // Windows has certain reserved file names that cannot be used, such as AUX, CON, PRN, etc.
        // We want to avoid generating a random name that matches that pattern, so we use a different
        // set of characters for the first character of the name that does not include any of the
        // reserved names first characters.
        let path_chars_to_use = if i == 0 && is_windows(env) && prefix.is_none() && (random_length == 3 || random_length == 4) {
            WINDOWS_SAFE_PATH_FIRST_CHARS
        } else {
            PATH_CHARS
        };

        let index = (random_f64() * path_chars_to_use.len() as f64).floor() as usize;
        suffix.push(path_chars_to_use.as_bytes()[index] as char);
    }

    let random_file_name = match prefix {
        Some(prefix) => format!("{prefix}-{suffix}"),
        None => suffix,
    };

    match parent.filter(|p| !p.is_empty()) {
        Some(parent) => PathModule::native(env).join(&[parent, &random_file_name]),
        None => random_file_name,
    }
}
