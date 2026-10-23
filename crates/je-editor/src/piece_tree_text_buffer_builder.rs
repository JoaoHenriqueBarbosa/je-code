//! Porte de `src/vs/editor/common/model/pieceTreeTextBuffer/pieceTreeTextBufferBuilder.ts`: monta o
//! buffer de texto a partir de pedaços (BOM, contagem de fins de linha, RTL, ASCII básico).
//!
//! Diferença de forma: `create` consome a fábrica. O original reescreve os pedaços no lugar ao normalizar o
//! fim de linha, então a fábrica já não servia para um segundo `create`; `getFirstLineText` vale antes.

use je_base::JsString;
use je_base::char_code;
use je_base::strings;

use crate::model::{DefaultEndOfLine, Eol};
use crate::piece_tree_base::{StringBuffer, create_line_starts, create_line_starts_fast, replace_line_breaks};
use crate::piece_tree_text_buffer::PieceTreeTextBuffer;

/// `PieceTreeTextBufferFactory`.
pub struct PieceTreeTextBufferFactory {
    chunks: Vec<StringBuffer>,
    bom: JsString,
    cr: usize,
    lf: usize,
    crlf: usize,
    contains_rtl: bool,
    contains_unusual_line_terminators: bool,
    is_basic_ascii: bool,
    normalize_eol: bool,
}

impl PieceTreeTextBufferFactory {
    fn get_eol(&self, default_eol: DefaultEndOfLine) -> Eol {
        let total_eol_count = self.cr + self.lf + self.crlf;
        let total_cr_count = self.cr + self.crlf;
        if total_eol_count == 0 {
            // É um arquivo vazio ou com exatamente uma linha
            return if default_eol == DefaultEndOfLine::Lf { Eol::Lf } else { Eol::CrLf };
        }
        if total_cr_count as f64 > total_eol_count as f64 / 2.0 {
            // Mais da metade das linhas do arquivo termina em \r\n
            return Eol::CrLf;
        }
        // Pelo menos uma linha a mais termina em \n
        Eol::Lf
    }

    /// `create(defaultEOL)`: o buffer de texto (e, nele, o handle de descarte).
    pub fn create(mut self, default_eol: DefaultEndOfLine) -> PieceTreeTextBuffer {
        let eol = self.get_eol(default_eol);
        if self.normalize_eol && ((eol == Eol::CrLf && (self.cr > 0 || self.lf > 0)) || (eol == Eol::Lf && (self.cr > 0 || self.crlf > 0)))
        {
            // Normaliza as peças
            for chunk in &mut self.chunks {
                *chunk = StringBuffer::from_text(replace_line_breaks(&chunk.buffer, eol.as_units()));
            }
        }
        PieceTreeTextBuffer::new(
            self.chunks,
            self.bom,
            eol,
            self.contains_rtl,
            self.contains_unusual_line_terminators,
            self.is_basic_ascii,
            self.normalize_eol,
        )
    }

    /// `getFirstLineText(lengthLimit)`.
    pub fn get_first_line_text(&self, length_limit: usize) -> JsString {
        let first = &self.chunks[0].buffer;
        let text = &first[..length_limit.min(first.len())];
        let end = text
            .iter()
            .position(|&unit| unit == char_code::CarriageReturn || unit == char_code::LineFeed)
            .unwrap_or(text.len());
        JsString::from(&text[..end])
    }
}

/// `PieceTreeTextBufferBuilder`.
pub struct PieceTreeTextBufferBuilder {
    chunks: Vec<StringBuffer>,
    bom: JsString,
    has_previous_char: bool,
    previous_char: u16,
    cr: usize,
    lf: usize,
    crlf: usize,
    contains_rtl: bool,
    contains_unusual_line_terminators: bool,
    is_basic_ascii: bool,
}

impl Default for PieceTreeTextBufferBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl PieceTreeTextBufferBuilder {
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            bom: JsString::new(),
            has_previous_char: false,
            previous_char: 0,
            cr: 0,
            lf: 0,
            crlf: 0,
            contains_rtl: false,
            contains_unusual_line_terminators: false,
            is_basic_ascii: true,
        }
    }

    pub fn accept_chunk(&mut self, chunk: &[u16]) {
        if chunk.is_empty() {
            return;
        }

        let mut chunk = chunk;
        if self.chunks.is_empty() && strings::starts_with_utf8_bom(chunk) {
            self.bom = JsString::from(&strings::UTF8_BOM_CHARACTER[..]);
            chunk = &chunk[1..];
        }

        let last_char = chunk[chunk.len() - 1];
        if last_char == char_code::CarriageReturn || (0xD800..=0xDBFF).contains(&last_char) {
            // o último caractere é \r ou um high surrogate: guarda-o para o próximo pedaço
            self.accept_chunk1(&chunk[..chunk.len() - 1], false);
            self.has_previous_char = true;
            self.previous_char = last_char;
        } else {
            self.accept_chunk1(chunk, false);
            self.has_previous_char = false;
            self.previous_char = last_char;
        }
    }

    fn accept_chunk1(&mut self, chunk: &[u16], allow_empty_strings: bool) {
        if !allow_empty_strings && chunk.is_empty() {
            // Nada a fazer
            return;
        }

        if self.has_previous_char {
            let mut joined = Vec::with_capacity(chunk.len() + 1);
            joined.push(self.previous_char);
            joined.extend_from_slice(chunk);
            self.accept_chunk2(joined);
        } else {
            self.accept_chunk2(chunk.to_vec());
        }
    }

    fn accept_chunk2(&mut self, chunk: Vec<u16>) {
        let line_starts = create_line_starts(&chunk);

        self.cr += line_starts.cr;
        self.lf += line_starts.lf;
        self.crlf += line_starts.crlf;

        if !line_starts.is_basic_ascii {
            // este pedaço tem caracteres fora do ASCII básico
            self.is_basic_ascii = false;
            if !self.contains_rtl {
                self.contains_rtl = strings::contains_rtl(&chunk);
            }
            if !self.contains_unusual_line_terminators {
                self.contains_unusual_line_terminators = strings::contains_unusual_line_terminators(&chunk);
            }
        }
        self.chunks.push(StringBuffer::new(chunk, line_starts.line_starts));
    }

    pub fn finish(mut self, normalize_eol: bool) -> PieceTreeTextBufferFactory {
        self.finish_internal();
        PieceTreeTextBufferFactory {
            chunks: self.chunks,
            bom: self.bom,
            cr: self.cr,
            lf: self.lf,
            crlf: self.crlf,
            contains_rtl: self.contains_rtl,
            contains_unusual_line_terminators: self.contains_unusual_line_terminators,
            is_basic_ascii: self.is_basic_ascii,
            normalize_eol,
        }
    }

    fn finish_internal(&mut self) {
        if self.chunks.is_empty() {
            self.accept_chunk1(&[], true);
        }

        if self.has_previous_char {
            self.has_previous_char = false;
            // recria o último pedaço
            let last_chunk = self.chunks.last_mut().expect("há ao menos um pedaço");
            last_chunk.buffer.push(self.previous_char);
            last_chunk.line_starts = create_line_starts_fast(&last_chunk.buffer);
            if self.previous_char == char_code::CarriageReturn {
                self.cr += 1;
            }
        }
    }
}
