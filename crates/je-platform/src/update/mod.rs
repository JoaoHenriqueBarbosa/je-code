//! Porte de `src/vs/platform/update/common`: os tipos de estado do serviço de atualização; e de
//! `src/vs/platform/update/electron-main`: o `AbstractUpdateService`, a base dos serviços de atualização.

pub mod abstract_update_service;
#[allow(clippy::module_inception)] // `update.rs` é o porte de `update.ts`, como no original
pub mod update;

pub mod update_relaunch_arguments;
pub mod win32_update_type;

pub use update::*;
