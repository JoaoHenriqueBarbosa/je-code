//! Porte de `src/vs/workbench/contrib/terminal/browser/xterm/terminalFontRendering.ts`: a política de renderização da fonte do
//! terminal (tons de cinza ou herdada do sistema).
//!
//! Divergências:
//! - **`terminal.element.classList` e `terminalFontRendering.css`.** O original liga a classe CSS
//!   `terminal-font-rendering-grayscale` ao elemento do xterm, e o CSS desliga o antialiasing por subpixel. Na UI nativa não há
//!   elemento nem CSS: a classe vira o estado [`IFontRenderingTerminal::grayscale_font_rendering`], que o renderizador (wgpu na
//!   GPU, rasterizador em software na CPU) lê ao montar o atlas de glifos para escolher a máscara de cobertura em tons de
//!   cinza em vez da máscara por subpixel.
//! - **`terminal.element`.** É [`IFontRenderingTerminal::is_open`]: o elemento só existe depois de `open`.
//! - **`isMacintosh`.** É [`Platform::detect`], como o resto do porte.

use je_base::dynamic::Dynamic;
use je_base::platform::Platform;

/// O que `updateTerminalFontRendering` lê e escreve no `Terminal` do xterm.
pub trait IFontRenderingTerminal {
    /// `terminal.element !== undefined`: o terminal foi aberto.
    fn is_open(&self) -> bool;

    /// `terminal.element.classList.contains(CssClasses.Grayscale)`: o estado que o renderizador lê.
    fn grayscale_font_rendering(&self) -> bool;

    /// `terminal.element.classList.toggle(CssClasses.Grayscale, grayscale)`.
    fn set_grayscale_font_rendering(&self, grayscale: bool);

    /// `terminal.clearTextureAtlas()`: descarta os bitmaps de glifos que o renderizador guarda.
    fn clear_texture_atlas(&self);
}

/// `updateTerminalFontRendering(terminal, fontRendering)`. O `fontRendering` é o valor da configuração
/// (`'inherit'`, `'grayscale'` ou `undefined`).
pub fn update_terminal_font_rendering(terminal: &dyn IFontRenderingTerminal, font_rendering: &Dynamic) {
    if !terminal.is_open() {
        return;
    }

    let grayscale = Platform::detect().is_macintosh && font_rendering.strict_equals(&Dynamic::string("grayscale"));
    if terminal.grayscale_font_rendering() == grayscale {
        return;
    }
    terminal.set_grayscale_font_rendering(grayscale);

    // This clears glyph bitmaps, but xterm's cached Canvas2D font state can still retain the previous policy.
    terminal.clear_texture_atlas();
}
