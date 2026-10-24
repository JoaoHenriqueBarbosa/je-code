//! As cores do editor resolvidas do tema (o que `editorColorRegistry.ts` e os `registerThemingParticipant` das
//! partes da view leem). Resolvidas uma vez por troca de tema, não a cada quadro.

use je_render::Rgba;
use je_theme::ColorTheme;

/// As cores que a [`crate::EditorView`] usa.
#[derive(Clone, Debug)]
pub struct EditorColors {
    pub background: Rgba,
    pub foreground: Rgba,
    pub gutter_background: Rgba,
    pub line_number: Rgba,
    pub line_number_active: Rgba,
    /// `editor.lineHighlightBackground` (sem padrão no registro: pode não existir).
    pub line_highlight: Option<Rgba>,
    /// `editor.lineHighlightBorder`, só quando o tema define (o padrão do registro não aparece no oráculo).
    pub line_highlight_border: Option<Rgba>,
    pub selection: Rgba,
    pub selection_inactive: Rgba,
    pub cursor: Rgba,
    pub indent_guide: Rgba,
    pub indent_guide_active: Rgba,
    pub bracket_match_background: Rgba,
    pub bracket_match_border: Rgba,
    /// `editorBracketHighlight.foreground1..6` sem as transparentes (o ciclo do `colorValues` do original).
    pub bracket_colors: Vec<Rgba>,
    pub unexpected_bracket: Rgba,
    /// `editorWhitespace.foreground`, dos espaços e tabulações da seleção.
    pub whitespace: Rgba,
    pub scrollbar_slider: Rgba,
    pub scrollbar_slider_hover: Rgba,
    pub scrollbar_slider_active: Rgba,
    /// `minimap.background` (sem tema: o fundo do editor).
    pub minimap_background: Rgba,
    pub minimap_slider: Rgba,
    pub minimap_slider_hover: Rgba,
    pub minimap_slider_active: Rgba,
}

impl EditorColors {
    pub fn from_theme(theme: &ColorTheme) -> Self {
        let color = |id: &str| theme.get_color(id).unwrap_or(Rgba::opaque(0xff, 0x00, 0xff));
        let bracket_colors = (1..=6)
            .filter_map(|i| theme.get_color(&format!("editorBracketHighlight.foreground{i}")))
            .filter(|c| c.a != 0)
            .collect();
        Self {
            background: color("editor.background"),
            foreground: color("editor.foreground"),
            gutter_background: theme.get_color("editorGutter.background").unwrap_or_else(|| color("editor.background")),
            line_number: color("editorLineNumber.foreground"),
            line_number_active: color("editorLineNumber.activeForeground"),
            line_highlight: theme.get_color("editor.lineHighlightBackground").filter(|c| c.a != 0),
            line_highlight_border: theme.color("editor.lineHighlightBorder").filter(|c| c.a != 0),
            selection: color("editor.selectionBackground"),
            selection_inactive: color("editor.inactiveSelectionBackground"),
            cursor: color("editorCursor.foreground"),
            indent_guide: color("editorIndentGuide.background1"),
            indent_guide_active: color("editorIndentGuide.activeBackground1"),
            bracket_match_background: color("editorBracketMatch.background"),
            bracket_match_border: color("editorBracketMatch.border"),
            bracket_colors,
            unexpected_bracket: color("editorBracketHighlight.unexpectedBracket.foreground"),
            whitespace: color("editorWhitespace.foreground"),
            scrollbar_slider: color("scrollbarSlider.background"),
            scrollbar_slider_hover: color("scrollbarSlider.hoverBackground"),
            scrollbar_slider_active: color("scrollbarSlider.activeBackground"),
            minimap_background: theme.get_color("minimap.background").unwrap_or_else(|| color("editor.background")),
            minimap_slider: color("minimapSlider.background"),
            minimap_slider_hover: color("minimapSlider.hoverBackground"),
            minimap_slider_active: color("minimapSlider.activeBackground"),
        }
    }

    /// A cor de um colchete pelo nível de aninhamento (`level % colorValues.length`).
    pub fn bracket_color(&self, level: i64, invalid: bool) -> Option<Rgba> {
        if invalid {
            return Some(self.unexpected_bracket);
        }
        if self.bracket_colors.is_empty() {
            return None;
        }
        Some(self.bracket_colors[level.rem_euclid(30) as usize % self.bracket_colors.len()])
    }
}
