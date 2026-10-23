//! Porte de `src/vs/editor/browser/gpu/raster/raster.ts`, só no que o alocador de atlas usa: o
//! [`IBoundingBox`] e o [`IRasterizedGlyph`].
//!
//! Divergências:
//! - `IGlyphRasterizer` (rasterizador de glifos, com `getTextMetrics` devolvendo o `TextMetrics` do DOM)
//!   fica de fora: o alocador só consome o glifo já rasterizado, e o rasterizador real do je-code é
//!   outro módulo (vello/fontes), ainda por portar.
//! - `source: OffscreenCanvas` vira `Rc<PixelPage>` (leitura apenas; o alocador só copia pixels dela).
//! - Os campos de geometria são `u32` (coordenadas de pixel inclusivas, com `right >= left` e
//!   `bottom >= top`, como o original supõe). `originOffset` é `i32` (pode ser negativo), e os
//!   `fontBoundingBox*` são `f64` (vêm de `TextMetrics`, não são inteiros).
//! - `width` e `height` são métodos novos que dão nome à conta `right - left + 1` e `bottom - top + 1`,
//!   que os dois alocadores repetem (DRY).

use std::rc::Rc;

use super::gpu_utils::{PixelPage, PixelRect};

/// Uma caixa delimitadora simples num plano 2D, com as quatro bordas inclusivas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IBoundingBox {
    /// A coordenada x da esquerda (inclusiva).
    pub left: u32,
    /// A coordenada y do topo (inclusiva).
    pub top: u32,
    /// A coordenada x da direita (inclusiva).
    pub right: u32,
    /// A coordenada y de baixo (inclusiva).
    pub bottom: u32,
}

impl IBoundingBox {
    /// `right - left + 1`.
    pub fn width(&self) -> u32 {
        (self.right + 1).saturating_sub(self.left)
    }

    /// `bottom - top + 1`.
    pub fn height(&self) -> u32 {
        (self.bottom + 1).saturating_sub(self.top)
    }

    /// A região de pixels coberta pela caixa.
    pub fn rect(&self) -> PixelRect {
        PixelRect { x: self.left, y: self.top, w: self.width(), h: self.height() }
    }
}

/// O deslocamento até a origem do glifo (onde ele deve ser desenhado).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IOriginOffset {
    pub x: i32,
    pub y: i32,
}

/// Um glifo que foi rasterizado numa página.
#[derive(Clone, Debug)]
pub struct IRasterizedGlyph {
    /// A página onde o glifo foi rasterizado.
    pub source: Rc<PixelPage>,
    /// A caixa delimitadora do glifo dentro de `source`.
    pub bounding_box: IBoundingBox,
    /// O deslocamento até a origem do glifo (onde ele deve ser desenhado).
    pub origin_offset: IOriginOffset,
    /// A distância da linha de base do glifo até o topo do mais alto retângulo delimitador de todas as
    /// fontes usadas para renderizar o texto (`TextMetrics.fontBoundingBoxAscent`).
    pub font_bounding_box_ascent: f64,
    /// A distância da linha de base do glifo até a base do retângulo delimitador de todas as fontes
    /// usadas para renderizar o texto (`TextMetrics.fontBoundingBoxDescent`).
    pub font_bounding_box_descent: f64,
}
