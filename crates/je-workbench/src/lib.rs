//! Porte de `src/vs/workbench` do VS Code: serviços (`services`) e contribuições (`contrib`).
//!
//! As pastas seguem a árvore do original: `contrib/<área>/common/...` vira `contrib::<área>::...`.

pub mod browser;

pub mod contrib;

pub mod services;

pub mod api;

pub mod common;

// Porte de `src/vs/workbench/test`: os serviços de teste (`workbench_test_services`) usados pelos testes de todas as
// crates acima do workbench. É infraestrutura de teste compartilhada, por isso é um módulo público de produção, e não
// um arquivo de `tests/support` repetido em cada crate.
pub mod test;
