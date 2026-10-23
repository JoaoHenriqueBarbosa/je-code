//! Porte de `src/vs/platform/markers/common`: o contrato e o serviço de marcadores (diagnósticos).

pub mod marker_service;
#[allow(clippy::module_inception)] // `markers.rs` é o porte de `markers.ts`, como no original
pub mod markers;

pub use markers::*;
