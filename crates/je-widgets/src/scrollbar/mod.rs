//! Porte de `src/vs/base/browser/ui/scrollbar/`: a barra de rolagem com slider arrastável, a visibilidade
//! com fade, as setas e o elemento rolável com roda do mouse.
//!
//! As cores do tema (`--vscode-scrollbar-*`) entram por [`set_scrollbar_colors`]; o padrão é o do tema
//! escuro do VS Code (`miscColors.ts`).

use std::cell::Cell;

use je_render::Rgba;

pub mod abstract_scrollbar;
pub mod horizontal_scrollbar;
pub mod vertical_scrollbar;
pub mod scrollable_element;
pub mod scrollable_element_options;
pub mod scrollbar_arrow;
pub mod scrollbar_state;
pub mod visibility_controller;

/// As variáveis de CSS de cor da barra de rolagem.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarColors {
    /// `scrollbar.shadow`.
    pub shadow: Rgba,
    /// `scrollbar.background`: o fundo da trilha.
    pub background: Option<Rgba>,
    /// `scrollbarSlider.background`.
    pub slider_background: Rgba,
    /// `scrollbarSlider.hoverBackground`.
    pub slider_hover_background: Rgba,
    /// `scrollbarSlider.activeBackground`.
    pub slider_active_background: Rgba,
    /// A cor do ícone das setas (herda o `foreground` do workbench).
    pub arrow_foreground: Rgba,
}

impl Default for ScrollbarColors {
    fn default() -> Self {
        Self {
            shadow: Rgba::new(0, 0, 0, 255),
            background: None,
            slider_background: Rgba::new(0x79, 0x79, 0x79, 0x66),
            slider_hover_background: Rgba::new(0x64, 0x64, 0x64, 0xB3),
            slider_active_background: Rgba::new(0xBF, 0xBF, 0xBF, 0x66),
            arrow_foreground: Rgba::new(0xCC, 0xCC, 0xCC, 255),
        }
    }
}

thread_local! {
    static COLORS: Cell<ScrollbarColors> = Cell::new(ScrollbarColors::default());
}

pub fn set_scrollbar_colors(colors: ScrollbarColors) {
    COLORS.with(|c| c.set(colors));
}

pub fn scrollbar_colors() -> ScrollbarColors {
    COLORS.with(Cell::get)
}
