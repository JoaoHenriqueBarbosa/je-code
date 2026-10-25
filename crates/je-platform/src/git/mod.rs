//! Porte de `src/vs/platform/git` do VS Code: o contrato do serviço de git local (`common/localGitService.ts`) e,
//! fora do alvo `wasm32`, a implementação que roda o executável `git` por processo (`node/localGitService.ts`).
//!
//! A implementação não usa nenhuma biblioteca de git: chama o `git` do sistema, como o original, e por isso o processo
//! filho (o `cp.execFile` do Node) fica atrás do trait [`node_local_git_service::ExecFile`], com a implementação real em
//! [`process_exec_file`].

pub mod local_git_service;
#[cfg(not(target_arch = "wasm32"))]
pub mod node_local_git_service;
#[cfg(not(target_arch = "wasm32"))]
pub mod process_exec_file;

pub use local_git_service::*;
