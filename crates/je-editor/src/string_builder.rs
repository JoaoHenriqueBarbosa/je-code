//! Porte de `src/vs/editor/common/core/stringBuilder.ts`.
//!
//! O `TextDecoder('UTF-16LE')` do original troca surrogate isolado por U+FFFD; o porte reproduz isso
//! (`String::from_utf16_lossy`), exceto no `decodeUTF16LE` com BOM na frente, que o original decodifica à
//! mão para manter o BOM e os surrogates como estão.

use je_base::JsString;
use je_base::strings;

/// `TextDecoder.decode` de UTF-16 (sem BOM): surrogate isolado vira U+FFFD.
fn text_decoder_decode(units: &[u16]) -> JsString {
    JsString::from(String::from_utf16_lossy(units))
}

/// `decodeUTF16LE(source, offset, len)`: `len` unidades de 16 bits little endian a partir de `offset`.
pub fn decode_utf16le(source: &[u8], offset: usize, len: usize) -> JsString {
    let units: Vec<u16> = (0..len)
        .map(|i| u16::from_le_bytes([source[offset + 2 * i], source[offset + 2 * i + 1]]))
        .collect();
    if len > 0 && (units[0] == 0xFEFF || units[0] == 0xFFFE) {
        // O decodificador comeria o BOM; sabemos que a cadeia é UTF-16LE e queremos mantê-lo.
        return JsString::from_units(units);
    }
    text_decoder_decode(&units)
}

/// Monta uma cadeia grande a partir de pedaços de unidades UTF-16.
#[derive(Debug)]
pub struct StringBuilder {
    capacity: usize,
    buffer: Vec<u16>,
    completed_strings: Option<Vec<JsString>>,
}

impl StringBuilder {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            buffer: Vec::with_capacity(capacity),
            completed_strings: None,
        }
    }

    pub fn reset(&mut self) {
        self.completed_strings = None;
        self.buffer.clear();
    }

    pub fn build(&mut self) -> JsString {
        if self.completed_strings.is_some() {
            self.flush_buffer();
            let mut result = JsString::new();
            for part in self.completed_strings.as_ref().into_iter().flatten() {
                result.push_units(part.units());
            }
            return result;
        }
        self.build_buffer()
    }

    fn build_buffer(&self) -> JsString {
        if self.buffer.is_empty() {
            return JsString::new();
        }
        text_decoder_decode(&self.buffer)
    }

    fn flush_buffer(&mut self) {
        let buffer_string = self.build_buffer();
        self.buffer.clear();
        self.completed_strings.get_or_insert_with(Vec::new).push(buffer_string);
    }

    /// `appendCharCode`: uma unidade de 16 bits.
    pub fn append_char_code(&mut self, char_code: u16) {
        let remaining_space = self.capacity - self.buffer.len();
        if remaining_space <= 1 && (remaining_space == 0 || strings::is_high_surrogate(char_code)) {
            self.flush_buffer();
        }
        self.buffer.push(char_code);
    }

    /// `appendASCIICharCode`: um caractere ASCII.
    pub fn append_ascii_char_code(&mut self, char_code: u16) {
        if self.buffer.len() == self.capacity {
            self.flush_buffer();
        }
        self.buffer.push(char_code);
    }

    pub fn append_string(&mut self, str: &[u16]) {
        if self.buffer.len() + str.len() >= self.capacity {
            // A cadeia não cabe no que sobra do buffer.
            self.flush_buffer();
            self.completed_strings.get_or_insert_with(Vec::new).push(JsString::from(str));
            return;
        }
        self.buffer.extend_from_slice(str);
    }
}
