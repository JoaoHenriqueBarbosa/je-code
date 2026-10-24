//! Porte parcial de `src/vs/workbench/contrib/terminalContrib/clipboard`: a decisão de colar texto de várias linhas e a
//! contribuição de área de transferência.
//!
//! Fora do porte (e por quê): as ações de copiar e colar (`registerActiveInstanceAction` e `registerActiveXtermAction` de
//! `terminal.clipboard.contribution.ts`), que dependem do registro de ações do terminal e das chaves de contexto.

pub mod terminal_clipboard;
pub mod terminal_clipboard_contribution;
