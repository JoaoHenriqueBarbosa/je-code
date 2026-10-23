//! Porte de `src/vs/platform/workspaces/node`.

pub mod md5;
#[allow(clippy::module_inception)] // `workspaces.rs` é o porte de `workspaces.ts`, como no original
pub mod workspaces;

pub use workspaces::*;
