//! Porte parcial de `src/vs/workbench/contrib/terminalContrib/accessibility`: o cache do conteúdo do buffer.
//!
//! Fora do porte (e por quê): a `TerminalAccessibilityContribution`, o buffer acessível, a ajuda de acessibilidade e o
//! `TextAreaSyncAddon`, que dependem do `ITerminalContribution`, do `registerTerminalContribution`, do editor acessível e do
//! DOM (`contrib/terminal/browser`, inexistentes no porte).

pub mod buffer_content_tracker;
