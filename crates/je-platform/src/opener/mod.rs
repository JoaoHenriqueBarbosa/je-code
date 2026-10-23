//! Porte de `src/vs/platform/opener`: o serviço de abertura de recursos e a seleção codificada no URI.

pub mod editor_types;
#[allow(clippy::module_inception)] // `opener.rs` é o porte de `opener.ts`, como no original
pub mod opener;

pub use opener::*;
