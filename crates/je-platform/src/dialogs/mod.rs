//! Porte de `src/vs/platform/dialogs/common/dialogs.ts`: os tipos e os contratos dos serviços de diálogo (sem interface)
//! e a lógica comum dos tratadores de diálogo (`AbstractDialogHandler`).
//!
//! Fora do porte (e por quê): `electron-main/dialogMainService.ts` e `dialogMainUtils.ts` (diálogos nativos do Electron),
//! `electron-browser/dialog.ts` (mapeamento do `IDialogHandler` para o Electron). As interfaces são desenhadas pelos widgets
//! nativos.

#[allow(clippy::module_inception)] // `dialogs.rs` é o porte de `dialogs.ts`, como no original
pub mod dialogs;

pub use dialogs::*;
