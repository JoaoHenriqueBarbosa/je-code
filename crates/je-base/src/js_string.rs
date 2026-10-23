//! Cadeia UTF-16 com a semântica de `string` do JavaScript.

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
