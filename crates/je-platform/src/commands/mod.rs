//! Porte de `src/vs/platform/commands/common/commands.ts`: o registro de comandos e o contrato do
//! serviço de comandos.

#[allow(clippy::module_inception)] // `commands.rs` é o porte de `commands.ts`, como no original
pub mod commands;

pub use commands::*;
