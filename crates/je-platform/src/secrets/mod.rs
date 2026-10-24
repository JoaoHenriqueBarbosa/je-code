//! Porte de `src/vs/platform/secrets/common`: o armazenamento de segredos cifrados ([`secrets`]).

#[allow(clippy::module_inception)] // `secrets.rs` é o porte de `secrets.ts`, como no original
pub mod secrets;

pub use secrets::*;
