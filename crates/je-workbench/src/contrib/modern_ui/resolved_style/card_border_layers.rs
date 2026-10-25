//! Os traços de borda dos cartões compactos (`workbench/browser/media/floatingPanels.css`, linhas 475 a 530, e
//! `modernUI/browser/media/editorBorder.css`, linhas 52 a 64).
//!
//! No modo compacto cada cartão pinta as quatro arestas com quatro camadas `linear-gradient` de um traço de espessura
//! (`background-image`, `background-position` e `background-size`, com `background-origin: border-box`), em vez de uma
//! borda: cada cartão desenha só as arestas de baixo e da direita, e as de cima e da esquerda só nas pontas do conjunto.
//! O estilo resolvido devolve as camadas como dados; o widget nativo pinta cada uma como um retângulo de um traço.

use je_theme::color::Color;
use je_theme::sizes::base_sizes::STROKE_THICKNESS;

use super::{ClassList, ModernUiStyle};

/// O tipo de degradê de uma camada de `background-image`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientKind {
    /// `linear-gradient(cor, cor)`: uma faixa lisa.
    Linear,
    /// `radial-gradient(...)`: nenhuma regra do cartão usa este tipo.
    Radial,
}

/// A aresta do cartão que uma camada pinta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardEdge {
    Top,
    Right,
    Bottom,
    Left,
}

/// Uma camada de `background-image` do cartão compacto.
#[derive(Clone, Debug, PartialEq)]
pub struct BorderLayer {
    pub kind: GradientKind,
    pub edge: CardEdge,
    pub color: Color,
    /// `--vscode-strokeThickness`: a espessura da faixa (`background-size`).
    pub thickness: f64,
    /// `background-origin: border-box`.
    pub origin_border_box: bool,
}

impl<'a> ModernUiStyle<'a> {
    /// As camadas de `background-image` de um cartão compacto (`.part:is(.activitybar, .sidebar, .panel, .auxiliarybar,
    /// .editor)`), na ordem do original: cima, direita, baixo e esquerda. Vazio fora do `.floating-panels.modern-ui-compact`,
    /// nas partes sem a regra e quando uma variável da cadeia não tem valor (o editor só tem as variáveis com `.modern-ui`).
    pub fn compact_card_border_layers(&self, part: &ClassList) -> Vec<BorderLayer> {
        if !self.compact() {
            return Vec::new();
        }
        let editor = part.contains("editor");
        let card = part.any(&["activitybar", "sidebar", "panel", "auxiliarybar"]) || (editor && self.has("modern-ui"));
        let Some(stroke) = card.then(|| self.floating_card_stroke_color(part)).flatten() else {
            return Vec::new();
        };
        let rail = part.contains("activitybar");
        let transparent = Color::transparent_color();
        // Linhas 477, 487, 491, 500 e 524 a 526 (e editorBorder.css, linhas 54 e 58 a 60): a esquerda só é traçada na
        // ponta do conjunto e na trilha que encosta na barra lateral.
        let outer_left = part.contains("floating-part-outer-left") || (editor && part.contains("floating-editor-outer-left"));
        let left_stroked = outer_left || (rail && (part.contains("left") || (part.contains("right") && !self.has("nosidebar"))));
        // Linhas 478, 488 e 528 a 530 (e editorBorder.css, linhas 55 e 62 a 64).
        let outer_top = part.contains("floating-part-outer-top") || (editor && part.contains("floating-editor-outer-top"));
        let top_stroked = outer_top || rail;
        // Linha 505: a barra lateral direita cede a aresta da direita à trilha.
        let right_stroked = !(part.contains("sidebar") && part.contains("right") && !self.has("noactivitybar"));
        let thickness = self.size(STROKE_THICKNESS);
        let layer = |edge: CardEdge, stroked: bool| BorderLayer {
            kind: GradientKind::Linear,
            edge,
            color: if stroked { stroke.clone() } else { transparent.clone() },
            thickness,
            origin_border_box: true,
        };
        vec![layer(CardEdge::Top, top_stroked), layer(CardEdge::Right, right_stroked), layer(CardEdge::Bottom, true), layer(CardEdge::Left, left_stroked)]
    }
}
