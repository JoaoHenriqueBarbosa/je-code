//! Porte de `src/vs/platform/window`: os tipos de janela do `common/window.ts` usados pelo workbench e, em
//! [`electron_main`], o estado de UI da janela do `electron-main/window.ts`.

pub mod electron_main;
#[allow(clippy::module_inception)] // `window.rs` é o porte de `window.ts`, como no original
pub mod window;

pub use window::*;
