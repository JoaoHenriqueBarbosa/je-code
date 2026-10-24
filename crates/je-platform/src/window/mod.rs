//! Porte de `src/vs/platform/window/common`: os tipos de janela do `window.ts` usados pelo workbench.

#[allow(clippy::module_inception)] // `window.rs` é o porte de `window.ts`, como no original
pub mod window;

pub use window::*;
