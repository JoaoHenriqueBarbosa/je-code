//! `StringDecoder` incremental do `setEncoding` dos streams de leitura do bun 1.4.2 (medido com pipe de 65536 bytes
//! por pedaço). Cada `write` decodifica o que está completo e guarda o resto para o pedaço seguinte; `end` solta o resto:
//!
//! - `utf8`: guarda o fim incompleto de uma sequência (1 a 3 bytes); no `end` o resto vira um único U+FFFD. Sequência
//!   inválida na junção segue o `from_utf8_lossy` sobre os bytes concatenados (medido: `e2 | 28 a1` dá `�(�`,
//!   `f0 9f 98 | b` dá `�` e `b`).
//! - `utf16le`/`ucs2`: pares de bytes; um byte ímpar fica guardado e é descartado no `end`; uma unidade alta de par
//!   substituto no fim do pedaço fica guardada junto, e no `end` sai a unidade solta (medido).
//! - `base64`/`base64url`: grupos de 3 bytes; o resto de 1 ou 2 bytes sai no `end` (com `=` no `base64`, sem no `base64url`).
//! - `hex`, `latin1`/`binary` e `ascii` (`& 0x7f`): byte a byte, sem resto.
//!
//! A saída é em unidades UTF-16 porque o `utf16le` pode produzir substituto solto.

use ul_common::codec::{base64_encode, hex_lower, BASE64_STANDARD, BASE64_URL};

use crate::runtime::node_buffer::Encoding;

/// O decodificador com o resto de um caractere cortado entre pedaços.
pub(crate) struct StringDecoder {
    kind: Encoding,
    pending: Vec<u8>,
}

/// Quantos bytes do fim de `data` são o começo de uma sequência UTF-8 ainda incompleta (0 a 3).
fn utf8_incomplete_tail(data: &[u8]) -> usize {
    for back in 1..=data.len().min(3) {
        let byte = data[data.len() - back];
        if byte & 0xC0 == 0x80 {
            continue;
        }
        let needed = match byte {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => return 0,
        };
        return if needed > back { back } else { 0 };
    }
    0
}

fn latin1_units(bytes: &[u8], mask: u8) -> Vec<u16> {
    bytes.iter().map(|&byte| u16::from(byte & mask)).collect()
}

fn ascii_units(text: &[u8]) -> Vec<u16> {
    latin1_units(text, 0xFF)
}

impl StringDecoder {
    pub(crate) fn new(kind: Encoding) -> StringDecoder {
        StringDecoder { kind, pending: Vec::new() }
    }

    /// Decodifica `bytes` junto com o resto guardado e guarda o novo resto.
    pub(crate) fn write(&mut self, bytes: &[u8]) -> Vec<u16> {
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(bytes);
        match self.kind {
            Encoding::Utf8 => {
                let keep = utf8_incomplete_tail(&data);
                self.pending = data.split_off(data.len() - keep);
                String::from_utf8_lossy(&data).encode_utf16().collect()
            }
            Encoding::Utf16Le => {
                let even = data.len() & !1;
                let mut keep_from = even;
                if even >= 2 && (0xD800..=0xDBFF).contains(&u16::from_le_bytes([data[even - 2], data[even - 1]])) {
                    keep_from = even - 2;
                }
                self.pending = data.split_off(keep_from);
                data.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect()
            }
            Encoding::Latin1 => latin1_units(&data, 0xFF),
            Encoding::Ascii => latin1_units(&data, 0x7F),
            Encoding::Hex => ascii_units(hex_lower(&data).as_bytes()),
            Encoding::Base64 | Encoding::Base64Url => {
                let keep = data.len() % 3;
                self.pending = data.split_off(data.len() - keep);
                self.encode_base64(&data)
            }
        }
    }

    /// Solta o resto guardado (o fim do fluxo).
    pub(crate) fn end(&mut self) -> Vec<u16> {
        let data = std::mem::take(&mut self.pending);
        match self.kind {
            Encoding::Utf8 => String::from_utf8_lossy(&data).encode_utf16().collect(),
            Encoding::Utf16Le if data.len() >= 2 => vec![u16::from_le_bytes([data[0], data[1]])],
            Encoding::Base64 | Encoding::Base64Url => self.encode_base64(&data),
            _ => Vec::new(),
        }
    }

    fn encode_base64(&self, data: &[u8]) -> Vec<u16> {
        let text = if self.kind == Encoding::Base64 {
            base64_encode(data, BASE64_STANDARD, true)
        } else {
            base64_encode(data, BASE64_URL, false)
        };
        ascii_units(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(units: Vec<u16>) -> String {
        String::from_utf16_lossy(&units)
    }

    #[test]
    fn utf8_carries_cut_character_between_chunks() {
        let mut decoder = StringDecoder::new(Encoding::Utf8);
        assert_eq!(text(decoder.write(&[b'a', 0xC3])), "a");
        assert_eq!(text(decoder.write(&[0xA9, b'b'])), "\u{e9}b");
        assert_eq!(text(decoder.write(&[0xE2, 0x82])), "");
        assert_eq!(text(decoder.end()), "\u{fffd}");
    }

    #[test]
    fn utf8_invalid_junction_follows_lossy() {
        let mut decoder = StringDecoder::new(Encoding::Utf8);
        assert_eq!(text(decoder.write(&[0xE2])), "");
        assert_eq!(text(decoder.write(&[0x28, 0xA1])), "\u{fffd}(\u{fffd}");
        assert_eq!(text(decoder.write(&[0xF0, 0x9F, 0x98])), "");
        assert_eq!(text(decoder.write(&[b'b'])), "\u{fffd}b");
    }

    #[test]
    fn utf16le_holds_odd_byte_and_high_surrogate() {
        let mut decoder = StringDecoder::new(Encoding::Utf16Le);
        assert_eq!(decoder.write(&[0x3D, 0xD8]), Vec::<u16>::new());
        assert_eq!(decoder.write(&[0x00, 0xDE, 0x62]), vec![0xD83D, 0xDE00]);
        assert_eq!(decoder.end(), Vec::<u16>::new());
        assert_eq!(decoder.write(&[0x3D, 0xD8]), Vec::<u16>::new());
        assert_eq!(decoder.end(), vec![0xD83D]);
    }

    #[test]
    fn base64_holds_remainder_until_end() {
        let mut decoder = StringDecoder::new(Encoding::Base64);
        assert_eq!(text(decoder.write(b"abcd")), "YWJj");
        assert_eq!(text(decoder.end()), "ZA==");
        let mut url = StringDecoder::new(Encoding::Base64Url);
        assert_eq!(text(url.write(b"ab")), "");
        assert_eq!(text(url.end()), "YWI");
    }

    #[test]
    fn byte_encodings_have_no_remainder() {
        assert_eq!(text(StringDecoder::new(Encoding::Hex).write(&[0xC3, 0x01])), "c301");
        assert_eq!(text(StringDecoder::new(Encoding::Latin1).write(&[0xE9])), "\u{e9}");
        assert_eq!(text(StringDecoder::new(Encoding::Ascii).write(&[0xC3])), "C");
    }
}
