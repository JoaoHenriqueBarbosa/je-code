//! Porte de `src/vs/platform/workspaces`: o formato `.code-workspace`, os recentes, os identificadores
//! estáveis e o contrato do `IWorkspacesService`.
//!
//! Fora do porte (e por quê): `workspacesManagementMainService`, `workspacesHistoryMainService` e
//! `workspacesMainService` (`electron-main`: dependem de janelas, menu nativo e do `IBackupMainService` do
//! processo principal do Electron).

pub mod node;
pub mod workspace_identifier;
#[allow(clippy::module_inception)] // `workspaces.rs` é o porte de `workspaces.ts`, como no original
pub mod workspaces;

pub use workspaces::*;
