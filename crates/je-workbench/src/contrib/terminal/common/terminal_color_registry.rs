//! Porte de `src/vs/workbench/contrib/terminal/common/terminalColorRegistry.ts`: os identificadores das cores do terminal
//! (`TERMINAL_*`) e os das cores ANSI (`ansiColorIdentifiers`).
//!
//! Divergências:
//! - **Registro.** Os `registerColor(...)` do original e o laço de `registerColors()` (que registra as 16 cores `terminal.ansi*`
//!   com os padrões do `ansiColorMap`) não são código de execução aqui: o `je-theme` gera o registro de cores inteiro
//!   (`registered_colors.rs`, a partir das chamadas `registerColor` e do laço de `terminal.ansi*` de `ref/vscode/src/vs`) e o
//!   expõe em `je_theme::registry::color_registry()`. Os padrões, as descrições e a ordem de registro vivem lá, uma vez só.
//! - **Constantes.** `registerColor` devolve o id da cor; cada `TERMINAL_*_COLOR` é esse id (`&str`).
//! - **`ansiColorIdentifiers`.** O array preenchido pelo laço na ordem do `index` do `ansiColorMap` é o [`ANSI_COLOR_IDENTIFIERS`],
//!   com os 16 ids na ordem do índice da cor na tabela do terminal.

pub const TERMINAL_BACKGROUND_COLOR: &str = "terminal.background";
pub const TERMINAL_FOREGROUND_COLOR: &str = "terminal.foreground";
pub const TERMINAL_CURSOR_FOREGROUND_COLOR: &str = "terminalCursor.foreground";
pub const TERMINAL_CURSOR_BACKGROUND_COLOR: &str = "terminalCursor.background";
pub const TERMINAL_SELECTION_BACKGROUND_COLOR: &str = "terminal.selectionBackground";
pub const TERMINAL_INACTIVE_SELECTION_BACKGROUND_COLOR: &str = "terminal.inactiveSelectionBackground";
pub const TERMINAL_SELECTION_FOREGROUND_COLOR: &str = "terminal.selectionForeground";
pub const TERMINAL_COMMAND_DECORATION_DEFAULT_BACKGROUND_COLOR: &str = "terminalCommandDecoration.defaultBackground";
pub const TERMINAL_COMMAND_DECORATION_SUCCESS_BACKGROUND_COLOR: &str = "terminalCommandDecoration.successBackground";
pub const TERMINAL_COMMAND_DECORATION_ERROR_BACKGROUND_COLOR: &str = "terminalCommandDecoration.errorBackground";
pub const TERMINAL_OVERVIEW_RULER_CURSOR_FOREGROUND_COLOR: &str = "terminalOverviewRuler.cursorForeground";
pub const TERMINAL_BORDER_COLOR: &str = "terminal.border";
pub const TERMINAL_OVERVIEW_RULER_BORDER_COLOR: &str = "terminalOverviewRuler.border";
pub const TERMINAL_FIND_MATCH_BACKGROUND_COLOR: &str = "terminal.findMatchBackground";
pub const TERMINAL_HOVER_HIGHLIGHT_BACKGROUND_COLOR: &str = "terminal.hoverHighlightBackground";
pub const TERMINAL_FIND_MATCH_BORDER_COLOR: &str = "terminal.findMatchBorder";
pub const TERMINAL_FIND_MATCH_HIGHLIGHT_BACKGROUND_COLOR: &str = "terminal.findMatchHighlightBackground";
pub const TERMINAL_FIND_MATCH_HIGHLIGHT_BORDER_COLOR: &str = "terminal.findMatchHighlightBorder";
pub const TERMINAL_OVERVIEW_RULER_FIND_MATCH_FOREGROUND_COLOR: &str = "terminalOverviewRuler.findMatchForeground";
pub const TERMINAL_DRAG_AND_DROP_BACKGROUND: &str = "terminal.dropBackground";
pub const TERMINAL_TAB_ACTIVE_BORDER: &str = "terminal.tab.activeBorder";
pub const TERMINAL_INITIAL_HINT_FOREGROUND: &str = "terminal.initialHintForeground";

/// The color identifiers for the terminal's ansi colors. The index in the array corresponds to the index of the color in the
/// terminal color table.
pub const ANSI_COLOR_IDENTIFIERS: [&str; 16] = [
    "terminal.ansiBlack",
    "terminal.ansiRed",
    "terminal.ansiGreen",
    "terminal.ansiYellow",
    "terminal.ansiBlue",
    "terminal.ansiMagenta",
    "terminal.ansiCyan",
    "terminal.ansiWhite",
    "terminal.ansiBrightBlack",
    "terminal.ansiBrightRed",
    "terminal.ansiBrightGreen",
    "terminal.ansiBrightYellow",
    "terminal.ansiBrightBlue",
    "terminal.ansiBrightMagenta",
    "terminal.ansiBrightCyan",
    "terminal.ansiBrightWhite",
];
