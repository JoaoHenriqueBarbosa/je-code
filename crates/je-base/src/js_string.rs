//! Cadeia UTF-16 com a semântica de `string` do JavaScript.

use std::borrow::Borrow;
use std::fmt;

/// Texto como sequência de unidades UTF-16, igual a uma `string` do JavaScript.
#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JsString(Vec<u16>);

impl JsString {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    pub fn from_units(units: Vec<u16>) -> Self {
        Self(units)
    }

    pub fn units(&self) -> &[u16] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn push_unit(&mut self, unit: u16) {
        self.0.push(unit);
    }

    pub fn push_units(&mut self, units: &[u16]) {
        self.0.extend_from_slice(units);
    }

    /// `String.prototype.substring` com índices já ordenados e dentro do texto.
    pub fn slice(&self, start: usize, end: usize) -> JsString {
        Self(self.0[start..end].to_vec())
    }

    /// `String.prototype.toLowerCase`: conversão Unicode completa (inclusive `İ` virando `i̇` e o sigma
    /// final), com surrogates isolados preservados.
    pub fn to_lower_case(&self) -> JsString {
        convert_case(&self.0, str::to_lowercase)
    }

    /// `String.prototype.toUpperCase`: conversão Unicode completa (`ß` vira `SS`), com surrogates
    /// isolados preservados.
    pub fn to_upper_case(&self) -> JsString {
        convert_case(&self.0, str::to_uppercase)
    }

    /// Conversão com perda: surrogates isolados viram U+FFFD, como no `TextEncoder`.
    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }
}

impl From<&str> for JsString {
    fn from(s: &str) -> Self {
        Self(s.encode_utf16().collect())
    }
}

impl From<String> for JsString {
    fn from(s: String) -> Self {
        Self::from(s.as_str())
    }
}

impl From<&[u16]> for JsString {
    fn from(units: &[u16]) -> Self {
        Self(units.to_vec())
    }
}

impl PartialEq<str> for JsString {
    fn eq(&self, other: &str) -> bool {
        self.0.iter().copied().eq(other.encode_utf16())
    }
}

impl PartialEq<&str> for JsString {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl fmt::Display for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

impl fmt::Debug for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_string_lossy(), f)
    }
}

impl AsRef<[u16]> for JsString {
    fn as_ref(&self) -> &[u16] {
        &self.0
    }
}

/// Permite consultar um `HashMap<JsString, _>` com um `&[u16]` sem alocar. O `Hash` derivado de
/// `JsString` delega ao `Vec<u16>`, que faz o mesmo hash do slice.
impl Borrow<[u16]> for JsString {
    fn borrow(&self) -> &[u16] {
        &self.0
    }
}

/// Aplica a conversão de caixa do Rust aos trechos válidos e deixa passar os surrogates isolados,
/// que o JavaScript não altera. Um surrogate isolado não é letra nem ignorável, então quebrar o
/// texto nele não muda o contexto do sigma final.
fn convert_case(units: &[u16], convert: fn(&str) -> String) -> JsString {
    let mut out = Vec::with_capacity(units.len());
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut Vec<u16>| {
        out.extend(convert(run).encode_utf16());
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
    JsString(out)
}

/// `str.charCodeAt(index)`: `None` é o `NaN` do JavaScript (índice fora do texto).
pub fn char_code_at(units: &[u16], index: i64) -> Option<u16> {
    usize::try_from(index).ok().and_then(|i| units.get(i)).copied()
}

/// `str.codePointAt(index)`: junta o par de surrogates que começa em `index`.
pub fn code_point_at(units: &[u16], index: usize) -> Option<u32> {
    let high = *units.get(index)?;
    if (0xD800..=0xDBFF).contains(&high)
        && let Some(&low) = units.get(index + 1)
        && (0xDC00..=0xDFFF).contains(&low)
    {
        return Some(0x10000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(low) - 0xDC00));
    }
    Some(u32::from(high))
}

/// `str.substring(start, end)`: limita os índices ao texto e troca a ordem se vierem invertidos.
pub fn substring(units: &[u16], start: i64, end: i64) -> &[u16] {
    let clamp = |index: i64| usize::try_from(index.max(0)).map_or(units.len(), |i| i.min(units.len()));
    let (a, b) = (clamp(start), clamp(end));
    &units[a.min(b)..a.max(b)]
}

/// `str.indexOf(needle, from)`: com agulha vazia devolve `from` limitado ao tamanho do texto.
pub fn index_of(haystack: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    let from = from.min(haystack.len());
    if needle.is_empty() {
        return Some(from);
    }
    haystack[from..].windows(needle.len()).position(|window| window == needle).map(|i| i + from)
}

/// `str.lastIndexOf(needle, from)`: procura de trás para frente a partir de `from` (limitado ao tamanho
/// do texto); com agulha vazia devolve `from` limitado ao tamanho do texto.
pub fn last_index_of(haystack: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    let start = from.min(haystack.len() - needle.len());
    (0..=start).rev().find(|&i| haystack[i..i + needle.len()] == *needle)
}

/// `str.startsWith(needle, position)`.
pub fn starts_with_at(haystack: &[u16], needle: &[u16], position: usize) -> bool {
    haystack.get(position..).is_some_and(|rest| rest.starts_with(needle))
}

/// `str.endsWith(needle, endPosition)`.
pub fn ends_with_at(haystack: &[u16], needle: &[u16], end_position: usize) -> bool {
    haystack[..end_position.min(haystack.len())].ends_with(needle)
}

/// Os caracteres que `String.prototype.trim` e o `\s` das expressões regulares tratam como espaço
/// (`WhiteSpace` e `LineTerminator` da ECMAScript). Não coincide com `char::is_whitespace`: o
/// U+FEFF conta e o U+0085 não.
pub fn is_js_whitespace(unit: u16) -> bool {
    matches!(
        unit,
        0x09..=0x0D | 0x20 | 0xA0 | 0x1680 | 0x2000..=0x200A | 0x2028 | 0x2029 | 0x202F | 0x205F | 0x3000 | 0xFEFF
    )
}

/// `str.trimStart()`.
pub fn trim_start(units: &[u16]) -> &[u16] {
    let skipped = units.iter().take_while(|&&u| is_js_whitespace(u)).count();
    &units[skipped..]
}

/// `str.trimEnd()`.
pub fn trim_end(units: &[u16]) -> &[u16] {
    let kept = units.len() - units.iter().rev().take_while(|&&u| is_js_whitespace(u)).count();
    &units[..kept]
}

/// `str.trim()`.
pub fn trim(units: &[u16]) -> &[u16] {
    trim_end(trim_start(units))
}
