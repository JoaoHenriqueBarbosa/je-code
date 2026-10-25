//! O que os `import` de `editor.css` e de `currentLineHighlight.js` carregam no documento do fixture de CSS de tema
//! (`fixtureUtilsCss.test.ts`): as regras que o teste lê e o participante de tema do realce da linha atual.
//!
//! Notas de tradução:
//! - **`editor.css` e `currentLineHighlight.css`.** A folha do documento tem as regras de
//!   `src/vs/editor/browser/widget/codeEditor/editor.css` (`.monaco-editor`) e de
//!   `src/vs/editor/browser/viewParts/currentLineHighlight/currentLineHighlight.css`, na ordem e com as declarações do
//!   original. Ficaram só as regras que decidem as propriedades que os casos leem (cor, fundo e borda); o restante dos
//!   arquivos descreve posição e tamanho, que a árvore nativa não resolve por CSS.
//! - **`registerThemingParticipant` do `currentLineHighlight.ts`.** Um `import` de módulo registra o participante uma vez
//!   para o processo; aqui o teste chama [`register_current_line_highlight_theming_participant`] e guarda o resultado
//!   para descartar ao fim (o registro é por thread).

use std::rc::Rc;

use je_base::lifecycle::DisposableRef;
use je_theme::ThemeType;

use super::fixture_css::Stylesheet;
use crate::platform::theme::theme_service::{ICssStyleCollector, register_theming_participant};
use crate::services::themes::color_theme_data::ColorThemeData;

const EDITOR_LINE_HIGHLIGHT: &str = "editor.lineHighlightBackground";
const EDITOR_INACTIVE_LINE_HIGHLIGHT: &str = "editor.inactiveLineHighlightBackground";
const EDITOR_LINE_HIGHLIGHT_BORDER: &str = "editor.lineHighlightBorder";

/// As regras de `editor.css` e de `currentLineHighlight.css` que o fixture usa.
const EDITOR_STYLE_SHEET: &str = "
.monaco-editor {
	position: relative;
	overflow: visible;
	color: var(--vscode-editor-foreground);
	background-color: var(--vscode-editor-background);
	overflow-wrap: initial;
}
.monaco-editor .view-overlays .current-line {
	display: block;
	position: absolute;
	left: 0;
	top: 0;
	box-sizing: border-box;
	height: 100%;
}
.monaco-editor .margin-view-overlays .current-line {
	display: block;
	position: absolute;
	left: 0;
	top: 0;
	box-sizing: border-box;
	height: 100%;
}
.monaco-editor
	.margin-view-overlays
	.current-line.current-line-margin.current-line-margin-both {
	border-right: 0;
}
";

/// A folha do documento com as regras do editor.
pub fn editor_style_sheet() -> Rc<Stylesheet> {
    Rc::new(Stylesheet::parse(EDITOR_STYLE_SHEET, None).unwrap_or_else(|error| panic!("a folha do editor não é válida: {error}")))
}

fn is_high_contrast(theme_type: ThemeType) -> bool {
    matches!(theme_type, ThemeType::HighContrastDark | ThemeType::HighContrastLight)
}

/// O `registerThemingParticipant` do fim de `currentLineHighlight.ts`: o fundo da linha atual com o editor focado e sem
/// foco, e a borda quando o fundo é transparente ou o tema define `editor.lineHighlightBorder`.
pub fn register_current_line_highlight_theming_participant() -> DisposableRef {
    register_theming_participant(|theme: &ColorThemeData, collector: &mut dyn ICssStyleCollector| {
        let line_highlight = theme.get_color(EDITOR_LINE_HIGHLIGHT, true);
        let inactive_line_highlight = theme.get_color(EDITOR_INACTIVE_LINE_HIGHLIGHT, true);

        // Apply active line highlight when editor is focused
        if let Some(line_highlight) = &line_highlight {
            collector.add_rule(&format!(".monaco-editor.focused .view-overlays .current-line {{ background-color: {line_highlight}; }}"));
            collector.add_rule(&format!(".monaco-editor.focused .margin-view-overlays .current-line-margin {{ background-color: {line_highlight}; border: none; }}"));
        }

        // Apply inactive line highlight when editor is not focused
        if let Some(inactive_line_highlight) = &inactive_line_highlight {
            collector.add_rule(&format!(".monaco-editor .view-overlays .current-line {{ background-color: {inactive_line_highlight}; }}"));
            collector.add_rule(&format!(".monaco-editor .margin-view-overlays .current-line-margin {{ background-color: {inactive_line_highlight}; border: none; }}"));
        }

        if line_highlight.as_ref().is_none_or(|color| color.is_transparent()) || theme.defines(EDITOR_LINE_HIGHLIGHT_BORDER) {
            if let Some(line_highlight_border) = theme.get_color(EDITOR_LINE_HIGHLIGHT_BORDER, true) {
                collector.add_rule(&format!(".monaco-editor .view-overlays .current-line-exact {{ border: 2px solid {line_highlight_border}; }}"));
                collector.add_rule(&format!(".monaco-editor .margin-view-overlays .current-line-exact-margin {{ border: 2px solid {line_highlight_border}; }}"));
                if is_high_contrast(theme.theme_type()) {
                    collector.add_rule(".monaco-editor .view-overlays .current-line-exact { border-width: 1px; }");
                    collector.add_rule(".monaco-editor .margin-view-overlays .current-line-exact-margin { border-width: 1px; }");
                }
            }
        }
    })
}
