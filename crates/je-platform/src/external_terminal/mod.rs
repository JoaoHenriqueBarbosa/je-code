//! Porte de `src/vs/platform/externalTerminal`: o contrato do terminal externo ([`external_terminal`]) e a implementação
//! de Windows, macOS e Linux ([`external_terminal_service`], o `node/externalTerminalService.ts`).

#[allow(clippy::module_inception)] // `external_terminal.rs` é o porte de `externalTerminal.ts`, como no original
pub mod external_terminal;
pub mod external_terminal_service;

pub use external_terminal::*;
pub use external_terminal_service::*;
