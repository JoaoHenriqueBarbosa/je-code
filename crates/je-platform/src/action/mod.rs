//! Porte de `src/vs/platform/action/common`: os tipos de ação de comando (`ICommandAction`) e as categorias comuns.

pub mod action_common_categories;
#[allow(clippy::module_inception)] // `action.rs` é o porte de `action.ts`, como no original
pub mod action;

pub use action::*;
