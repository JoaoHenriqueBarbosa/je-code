//! Porte de `src/vs/platform/dialogs/common/dialogs.ts`: os tipos e os contratos dos serviços de diálogo (sem interface)
//! e a lógica comum dos tratadores de diálogo (`AbstractDialogHandler`).
//!
//! Também estão aqui `electron-browser/dialog.ts` ([`dialog`], o texto do diálogo "Sobre") e `electron-main/dialogMainUtils.ts`
//! ([`dialog_main_utils`]) e `electron-main/dialogMainService.ts` ([`dialog_main_service`], com o desenho dos diálogos
//! nativos atrás do trait `NativeDialogs`).
//!
//! Fora do porte (e por quê): o mapeamento do `IDialogHandler` para o Electron; as interfaces são desenhadas pelos widgets
//! nativos.

pub mod dialog;
pub mod dialog_main_service;
pub mod dialog_main_utils;

#[allow(clippy::module_inception)] // `dialogs.rs` é o porte de `dialogs.ts`, como no original
pub mod dialogs;

pub use dialogs::*;
