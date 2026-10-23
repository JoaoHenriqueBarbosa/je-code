//! Porte de `src/vs/platform/contextkey`: as expressões de contexto (`when`), o analisador, o
//! serviço de chaves de contexto com escopos e eventos, e as chaves de plataforma.

#[allow(clippy::module_inception)] // `contextkey.rs` é o porte de `contextkey.ts`, como no original
pub mod contextkey;
pub mod contextkeys;
pub mod scanner;
pub mod service;

pub use contextkey::*;
