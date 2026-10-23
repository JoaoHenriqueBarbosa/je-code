//! Porte dos tipos do `layoutInfo` (`editorOptions.ts`, linhas 2471 a 2600): `OverviewRulerPosition`,
//! `RenderMinimap`, `EditorLayoutInfo` e `EditorMinimapLayoutInfo`. Só os tipos: o `EditorLayoutInfoComputer`
//! (o computador) é outra fatia.

use crate::config::editor_options::{EditorOption, Opt};

/// Chave tipada de `layoutInfo`.
pub const LAYOUT_INFO: Opt<EditorLayoutInfo> = Opt::new(EditorOption::LayoutInfo);

/// `OverviewRulerPosition`: a posição da régua de visão geral.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverviewRulerPosition {
    /// Largura da régua.
    pub width: f64,
    /// Altura da régua.
    pub height: f64,
    /// Posição do topo da régua.
    pub top: f64,
    /// Posição da direita da régua.
    pub right: f64,
}

/// `RenderMinimap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMinimap {
    None = 0,
    Text = 1,
    Blocks = 2,
}

/// `EditorMinimapLayoutInfo`: o layout interno do minimapa.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorMinimapLayoutInfo {
    pub render_minimap: RenderMinimap,
    pub minimap_left: f64,
    pub minimap_width: f64,
    pub minimap_height_is_editor_height: bool,
    pub minimap_is_sampling: bool,
    pub minimap_scale: f64,
    pub minimap_line_height: f64,
    pub minimap_canvas_inner_width: f64,
    pub minimap_canvas_inner_height: f64,
    pub minimap_canvas_outer_width: f64,
    pub minimap_canvas_outer_height: f64,
}

/// `EditorLayoutInfo`: os detalhes internos do layout do editor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorLayoutInfo {
    /// Largura total do editor.
    pub width: f64,
    /// Altura total do editor.
    pub height: f64,
    /// Posição esquerda da margem de glifos.
    pub glyph_margin_left: f64,
    /// Largura da margem de glifos.
    pub glyph_margin_width: f64,
    /// Quantas faixas de decoração a margem de glifos desenha.
    pub glyph_margin_decoration_lane_count: f64,
    /// Posição esquerda dos números de linha.
    pub line_numbers_left: f64,
    /// Largura dos números de linha.
    pub line_numbers_width: f64,
    /// Posição esquerda das decorações de linha.
    pub decorations_left: f64,
    /// Largura das decorações de linha.
    pub decorations_width: f64,
    /// Posição esquerda do conteúdo (o texto).
    pub content_left: f64,
    /// Largura do conteúdo (o texto).
    pub content_width: f64,
    /// Layout do minimapa.
    pub minimap: EditorMinimapLayoutInfo,
    /// Quantas colunas (de caracteres típicos) cabem numa linha da viewport.
    pub viewport_column: f64,
    pub is_word_wrap_minified: bool,
    pub is_viewport_wrapping: bool,
    pub wrapping_column: f64,
    /// Largura da barra de rolagem vertical.
    pub vertical_scrollbar_width: f64,
    /// Altura da barra de rolagem horizontal.
    pub horizontal_scrollbar_height: f64,
    /// Posição da régua de visão geral.
    pub overview_ruler: OverviewRulerPosition,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_minimap_values_follow_the_original_enum() {
        assert_eq!([RenderMinimap::None as u8, RenderMinimap::Text as u8, RenderMinimap::Blocks as u8], [0, 1, 2]);
    }

    #[test]
    fn layout_key_points_at_the_layout_option() {
        assert_eq!(LAYOUT_INFO.id(), EditorOption::LayoutInfo);
    }
}
