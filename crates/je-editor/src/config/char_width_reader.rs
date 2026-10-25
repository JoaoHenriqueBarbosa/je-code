//! Parte de `src/vs/editor/browser/config/charWidthReader.ts` que o `FontMeasurements` consome: o pedido de
//! medição de um caractere ([`CharWidthRequest`], com [`CharWidthRequestType`]).
//!
//! Decisões de tradução:
//! - `readCharWidths(targetWindow, bareFontInfo, requests)` desenha os caracteres num elemento do DOM e lê as
//!   larguras. A UI é nativa: quem mede é o rasterizador/shaper de fonte do hospedeiro, que implementa
//!   `FontMeasurementsEnvironment::read_char_widths` (em `font_measurements`) e cumpre cada pedido com
//!   [`CharWidthRequest::fulfill`].
//! - O pedido é compartilhado entre quem o cria e quem mede (`Rc`), e a largura é um `Cell` porque o medidor a escreve
//!   por `&self`, como o `fulfill` do original muda o objeto.

use std::cell::Cell;

/// `CharWidthRequestType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharWidthRequestType {
    Regular,
    Italic,
    Bold,
}

/// `CharWidthRequest`.
#[derive(Debug)]
pub struct CharWidthRequest {
    pub chr: String,
    pub request_type: CharWidthRequestType,
    width: Cell<f64>,
}

impl CharWidthRequest {
    pub fn new(chr: &str, request_type: CharWidthRequestType) -> CharWidthRequest {
        CharWidthRequest { chr: chr.to_owned(), request_type, width: Cell::new(0.0) }
    }

    /// `chr.length`: o comprimento em unidades de código UTF-16, como o `String` do JavaScript.
    pub fn chr_length(&self) -> usize {
        self.chr.encode_utf16().count()
    }

    /// `width`.
    pub fn width(&self) -> f64 {
        self.width.get()
    }

    /// `fulfill(width)`.
    pub fn fulfill(&self, width: f64) {
        self.width.set(width);
    }
}
