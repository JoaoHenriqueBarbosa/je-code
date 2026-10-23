//! Texto dos widgets: a fonte de UI do workbench, medida e corte com reticências.
//!
//! O DOM mede texto por layout; aqui a medida sai do `je-text` (a mesma do Chromium, arredondada para
//! cima em 1/64 px). Os widgets do VS Code que misturam trechos de texto na mesma linha (iconLabel,
//! highlightedLabel, breadcrumbs, menu) posicionam um nó por trecho com estas medidas.

use je_render::Rgba;
use je_text::{TextStyle, with_text_system};
use je_ui::{Content, NodeId, Tree};

/// A lista de famílias da UI no Linux (`--vscode-font-family`).
pub const UI_FONT_FAMILIES: &str = "system-ui, \"Ubuntu\", \"Droid Sans\", sans-serif";
/// O `--vscode-font-size` padrão.
pub const UI_FONT_SIZE: f32 = 13.0;

/// O `line-height: 1.4em` de `.monaco-workbench` (13 px), que o texto sem `line-height` próprio herda como
/// comprimento absoluto, em unidades de layout do Chromium (1/64 px): 18,203125 px. Medido no oráculo (H26).
pub const WORKBENCH_LINE_HEIGHT: f64 = 18.203_125;

/// A fonte de UI no tamanho e peso dados.
pub fn ui_font(size_px: f32, weight: u16) -> TextStyle {
    TextStyle::new(UI_FONT_FAMILIES, size_px, weight)
}

/// A largura do texto numa linha, em px lógicos (`getBoundingClientRect().width` de um `span`).
pub fn measure_text(text: &str, style: &TextStyle) -> f64 {
    if text.is_empty() {
        return 0.0;
    }
    f64::from(with_text_system(|t| t.line(text, style)).width)
}

/// A altura da caixa de linha natural da fonte (a de `line-height: normal`).
pub fn natural_line_height(style: &TextStyle) -> f64 {
    f64::from(with_text_system(|t| t.line("M", style)).line_height)
}

/// O maior prefixo de `chars`, em quantidade de `char`, que com `suffix` cabe em `max_width`.
fn longest_fitting_prefix(chars: &[char], suffix: &str, style: &TextStyle, max_width: f64) -> usize {
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + suffix;
        if measure_text(&candidate, style) <= max_width {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

/// Corta `text` para caber em `max_width` com `…` no fim (o `text-overflow: ellipsis`). Devolve o texto
/// inteiro se couber. Corta em fronteira de `char`.
pub fn ellipsize(text: &str, style: &TextStyle, max_width: f64) -> String {
    if measure_text(text, style) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let fit = longest_fitting_prefix(&chars, "\u{2026}", style, max_width);
    chars[..fit].iter().collect::<String>() + "\u{2026}"
}

/// Corta `text` para caber em `max_width` sem marca no fim (o `overflow: hidden` com `text-overflow: clip`, por
/// glifos inteiros: o navegador mostra o glifo da borda pela metade). Devolve o texto inteiro se couber.
pub fn clip_text(text: &str, style: &TextStyle, max_width: f64) -> String {
    if measure_text(text, style) <= max_width {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let fit = longest_fitting_prefix(&chars, "", style, max_width);
    chars[..fit].iter().collect()
}

/// Põe `text` como conteúdo do nó (o `textContent` de um elemento sem filhos), com a caixa de linha de
/// `line_height` px, e devolve a largura do texto.
pub fn set_node_text(tree: &mut Tree, node: NodeId, text: &str, style: &TextStyle, color: Rgba, line_height: f64) -> f64 {
    tree.style_mut(node).content = Content::Text { text: text.to_owned(), style: style.clone(), color, line_height };
    measure_text(text, style)
}

/// Tira o conteúdo próprio do nó.
pub fn clear_node_content(tree: &mut Tree, node: NodeId) {
    tree.style_mut(node).content = Content::None;
}
