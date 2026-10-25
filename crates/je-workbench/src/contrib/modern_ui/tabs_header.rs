//! Porte das regras do cabeçalho do grupo de `src/vs/workbench/contrib/modernUI/browser/media/tabs.css` (linhas 64 a 80) e
//! de `connectedEditorTabs.css` (a linha do contêiner de abas conectado): o fundo opaco do `.title`, a camada de tema do
//! cabeçalho e o fundo da fileira conectada.
//!
//! Divergências:
//! - **Sem cascata de CSS.** Cada regra é uma condição sobre as classes que o seletor testa (`.title.tabs`,
//!   `.modern-ui-connected-editor-tabs`) e devolve a declaração, como a cascata a escolheria. As cores chegam como o texto
//!   CSS que `var(--vscode-...)` resolveria.

/// O que as regras de Modern UI declaram para o `.title` e para a fileira conectada de abas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModernHeaderBacking {
    /// `background-color` do `.title`: `--modern-ui-editor-title-background`, que é `var(--vscode-editor-background)`. O
    /// fundo é sempre o do editor, para o cabeçalho não ficar translúcido sobre o que está atrás.
    pub backing: String,
    /// `background-image` do `.title`: o degradê que pinta a cor de tema do cabeçalho por cima do fundo, ou `none`.
    pub theme_layer: String,
    /// `background-color` do `.tabs-and-actions-container` conectado, `None` fora do estilo conectado com abas.
    pub connected_row_theme_layer: Option<String>,
}

/// `linear-gradient(color, color)`: a camada de uma cor só.
fn solid_layer(color: &str) -> String {
    format!("linear-gradient({color}, {color})")
}

/// As declarações de `.modern-ui-tabs.monaco-workbench .part.editor > .content .editor-group-container > .title` para o
/// estado do cabeçalho: `show_tabs` é a classe `tabs` do `.title`, `connected` é `.modern-ui-connected-editor-tabs` na raiz.
///
/// - `--modern-ui-editor-title-background: var(--vscode-editor-background)` e `background-color` com ele (todo cabeçalho);
/// - `.title:not(.tabs)`: `background-image` com `--vscode-editorGroupHeader-noTabsBackground`;
/// - `:not(.modern-ui-connected-editor-tabs) .title.tabs`: `background-image` com `--vscode-editorGroupHeader-tabsBackground`;
/// - `.modern-ui-connected-editor-tabs .title.tabs > .tabs-and-actions-container`: `background-color` com
///   `var(--modern-ui-connected-tab-strip-background, var(--vscode-editorGroupHeader-tabsBackground))`.
pub fn modern_header_backing(editor_background: &str, tabs_background: &str, no_tabs_background: &str, connected: bool, show_tabs: bool) -> ModernHeaderBacking {
    let theme_layer = if !show_tabs {
        solid_layer(no_tabs_background)
    } else if !connected {
        solid_layer(tabs_background)
    } else {
        "none".to_string()
    };

    ModernHeaderBacking {
        backing: editor_background.to_string(),
        theme_layer,
        connected_row_theme_layer: (connected && show_tabs).then(|| tabs_background.to_string()),
    }
}
