//! Porte das regiões `cursorBlinking` e `cursorStyle` de `editorOptions.ts`: os dois enums e as
//! conversões de e para string.
//!
//! Divergência: as funções `...FromString` do original recebem uma união de strings e, fora dela,
//! devolvem `undefined` (o `switch` sem `default`). Aqui recebem `&str` e devolvem `None` nesse caso,
//! como `auto_indent_from_string`.

/// `TextEditorCursorBlinkingStyle`: o tipo de animação do cursor do editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEditorCursorBlinkingStyle {
    /// Escondido.
    Hidden = 0,
    /// Piscando.
    Blink = 1,
    /// Piscando com esmaecimento suave.
    Smooth = 2,
    /// Piscando com estado cheio prolongado e esmaecimento suave.
    Phase = 3,
    /// Animação de expandir e recolher no eixo y.
    Expand = 4,
    /// Sem piscar.
    Solid = 5,
}

/// `cursorBlinkingStyleFromString`.
pub fn cursor_blinking_style_from_string(cursor_blinking_style: &str) -> Option<TextEditorCursorBlinkingStyle> {
    match cursor_blinking_style {
        "blink" => Some(TextEditorCursorBlinkingStyle::Blink),
        "smooth" => Some(TextEditorCursorBlinkingStyle::Smooth),
        "phase" => Some(TextEditorCursorBlinkingStyle::Phase),
        "expand" => Some(TextEditorCursorBlinkingStyle::Expand),
        "solid" => Some(TextEditorCursorBlinkingStyle::Solid),
        _ => None,
    }
}

/// `TextEditorCursorStyle`: o estilo em que o cursor do editor é desenhado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEditorCursorStyle {
    /// Linha vertical (entre dois caracteres).
    Line = 1,
    /// Bloco (sobre um caractere).
    Block = 2,
    /// Linha horizontal (sob um caractere).
    Underline = 3,
    /// Linha vertical fina (entre dois caracteres).
    LineThin = 4,
    /// Bloco só com o contorno (sobre um caractere).
    BlockOutline = 5,
    /// Linha horizontal fina (sob um caractere).
    UnderlineThin = 6,
}

/// `cursorStyleToString`.
pub const fn cursor_style_to_string(cursor_style: TextEditorCursorStyle) -> &'static str {
    match cursor_style {
        TextEditorCursorStyle::Line => "line",
        TextEditorCursorStyle::Block => "block",
        TextEditorCursorStyle::Underline => "underline",
        TextEditorCursorStyle::LineThin => "line-thin",
        TextEditorCursorStyle::BlockOutline => "block-outline",
        TextEditorCursorStyle::UnderlineThin => "underline-thin",
    }
}

/// `cursorStyleFromString`.
pub fn cursor_style_from_string(cursor_style: &str) -> Option<TextEditorCursorStyle> {
    match cursor_style {
        "line" => Some(TextEditorCursorStyle::Line),
        "block" => Some(TextEditorCursorStyle::Block),
        "underline" => Some(TextEditorCursorStyle::Underline),
        "line-thin" => Some(TextEditorCursorStyle::LineThin),
        "block-outline" => Some(TextEditorCursorStyle::BlockOutline),
        "underline-thin" => Some(TextEditorCursorStyle::UnderlineThin),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_values_match_the_original() {
        assert_eq!(TextEditorCursorBlinkingStyle::Hidden as u8, 0);
        assert_eq!(TextEditorCursorBlinkingStyle::Solid as u8, 5);
        assert_eq!(TextEditorCursorStyle::Line as u8, 1);
        assert_eq!(TextEditorCursorStyle::UnderlineThin as u8, 6);
    }

    #[test]
    fn blinking_style_from_string() {
        assert_eq!(cursor_blinking_style_from_string("blink"), Some(TextEditorCursorBlinkingStyle::Blink));
        assert_eq!(cursor_blinking_style_from_string("smooth"), Some(TextEditorCursorBlinkingStyle::Smooth));
        assert_eq!(cursor_blinking_style_from_string("phase"), Some(TextEditorCursorBlinkingStyle::Phase));
        assert_eq!(cursor_blinking_style_from_string("expand"), Some(TextEditorCursorBlinkingStyle::Expand));
        assert_eq!(cursor_blinking_style_from_string("solid"), Some(TextEditorCursorBlinkingStyle::Solid));
        // `hidden` não tem string: o `switch` do original não a trata.
        assert_eq!(cursor_blinking_style_from_string("hidden"), None);
        assert_eq!(cursor_blinking_style_from_string("Blink"), None);
    }

    #[test]
    fn cursor_style_round_trips() {
        let all = [
            (TextEditorCursorStyle::Line, "line"),
            (TextEditorCursorStyle::Block, "block"),
            (TextEditorCursorStyle::Underline, "underline"),
            (TextEditorCursorStyle::LineThin, "line-thin"),
            (TextEditorCursorStyle::BlockOutline, "block-outline"),
            (TextEditorCursorStyle::UnderlineThin, "underline-thin"),
        ];
        for (style, text) in all {
            assert_eq!(cursor_style_to_string(style), text);
            assert_eq!(cursor_style_from_string(text), Some(style));
        }
        assert_eq!(cursor_style_from_string("line_thin"), None);
        assert_eq!(cursor_style_from_string(""), None);
    }
}
