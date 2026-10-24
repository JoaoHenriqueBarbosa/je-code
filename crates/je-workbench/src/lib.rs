//! Porte de `src/vs/workbench` do VS Code: serviços (`services`) e contribuições (`contrib`).
//!
//! As pastas seguem a árvore do original: `contrib/<área>/common/...` vira `contrib::<área>::...`.

pub mod browser;

pub mod contrib;

pub mod services;

pub mod api;

pub mod common;
