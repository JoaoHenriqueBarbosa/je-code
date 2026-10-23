//! Porte de `src/vs/platform/terminal/common/capabilities` do VS Code.
//!
//! Portado até agora: as interfaces e o enum de capacidades ([`capabilities`]) e o armazenamento de
//! capacidades com o seu multiplexador ([`terminal_capability_store`]).

#[allow(clippy::module_inception)] // `capabilities.rs` é o porte de `capabilities.ts`, como no original
pub mod capabilities;
pub mod terminal_capability_store;
