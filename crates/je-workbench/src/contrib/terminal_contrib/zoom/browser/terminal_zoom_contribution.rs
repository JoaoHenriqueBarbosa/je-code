//! Porte parcial de `src/vs/workbench/contrib/terminalContrib/zoom/browser/terminal.zoom.contribution.ts`: só o
//! `clampTerminalFontSize`.
//!
//! Fora do porte (e por quê): a `TerminalMouseWheelZoomContribution` e as ações `FontZoomIn`, `FontZoomOut` e
//! `FontZoomReset`, que dependem do `ITerminalContribution`, do `registerTerminalContribution`, do `registerTerminalAction`
//! (`contrib/terminal/browser`), do `IXtermTerminal`, do `MouseWheelClassifier` e do `defaultTerminalFontSize`
//! (`terminalConfiguration.ts`, também parcial). Entram com essas peças.

/// Tamanho mínimo da fonte do terminal.
const MINIMUM_FONT_SIZE: f64 = 6.0;

/// Tamanho máximo da fonte do terminal.
const MAXIMUM_FONT_SIZE: f64 = 100.0;

/// `clampTerminalFontSize(fontSize)`: `Math.max(6, Math.min(100, fontSize))`.
pub fn clamp_terminal_font_size(font_size: f64) -> f64 {
    MINIMUM_FONT_SIZE.max(MAXIMUM_FONT_SIZE.min(font_size))
}
