//! Porte de `src/vs/platform/workspaces`: o formato `.code-workspace`, os recentes, os identificadores
//! estáveis e o contrato do `IWorkspacesService`, mais o lado "main" dos workspaces (o
//! `WorkspacesManagementMainService`, em [`workspaces_management_main_service`]).
//!
//! Fora do porte (e por quê): `workspacesHistoryMainService` e `workspacesMainService` (`electron-main`: dependem
//! de janelas e menu nativo do processo principal do Electron).

pub mod node;
pub mod workspace_identifier;
#[allow(clippy::module_inception)] // `workspaces.rs` é o porte de `workspaces.ts`, como no original
pub mod workspaces;

pub use workspaces::*;

pub mod workspaces_management_main_service;
