//! Porte de `src/vs/platform/dialogs/common/dialogs.ts`: os tipos e os contratos dos serviços de diálogo (sem interface)
//! e a lógica comum dos tratadores de diálogo (`AbstractDialogHandler`).
//!
//! Fora do porte (e por quê): `electron-main/dialogMainService.ts` (a implementação com os diálogos nativos do Electron; só o
//! contrato usado pelos serviços de workspaces está em [`dialog_main_service`]) e `dialogMainUtils.ts`,
//! `electron-browser/dialog.ts` (mapeamento do `IDialogHandler` para o Electron). As interfaces são desenhadas pelos widgets
//! nativos.

pub mod dialog_main_service;

#[allow(clippy::module_inception)] // `dialogs.rs` é o porte de `dialogs.ts`, como no original
pub mod dialogs;

pub use dialogs::*;
