//! Porte parcial de `src/vs/workbench/contrib/terminalContrib/clipboard`: a decisão de colar texto de várias linhas.
//!
//! Fora do porte (e por quê): a `TerminalClipboardContribution` e as ações de copiar e colar
//! (`terminal.clipboard.contribution.ts`), que dependem do `ITerminalContribution` e do `registerTerminalContribution`
//! (`contrib/terminal/browser`), que não existem no porte.

pub mod terminal_clipboard;
