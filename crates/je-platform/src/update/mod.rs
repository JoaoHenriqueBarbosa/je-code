//! Porte de `src/vs/platform/update/common`: os tipos de estado do serviço de atualização.

#[allow(clippy::module_inception)] // `update.rs` é o porte de `update.ts`, como no original
pub mod update;

pub use update::*;
