//! Porte de `src/vs/workbench/contrib/multiDiffEditor/browser/icons.contribution.ts`: o ícone do rótulo do editor de diff de vários
//! arquivos.
//!
//! Divergências:
//! - **`registerIcon('multi-diff-editor-label-icon', Codicon.diffMultiple, ...)`.** O registro de ícones do porte não existe como
//!   módulo: o ícone do rótulo é o `ThemeIcon` de [`multi_diff_editor_icon`], com o id do original (como o carrossel de imagens).

use je_base::themables::ThemeIcon;

/// `MultiDiffEditorIcon`.
pub fn multi_diff_editor_icon() -> ThemeIcon {
    ThemeIcon::from_id("multi-diff-editor-label-icon")
}
