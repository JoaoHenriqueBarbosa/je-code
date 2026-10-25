//! Porte de `src/vs/workbench/contrib/editSessions`: a sincronização das edições não salvas entre máquinas.
//!
//! Portado: `common/editSessions.ts` ([`common`]: tipos, constantes, chaves de contexto, contratos `IEditSessionsStorageService` e
//! `IEditSessionsLogService`, `decodeEditSessionFileContent` e `hashedEditSessionId`), `common/editSessionsLogService.ts`
//! ([`edit_sessions_log_service`]), `common/editSessionsStorageClient.ts` ([`edit_sessions_storage_client`]) e
//! `browser/editSessionsStorageService.ts` ([`edit_sessions_storage_service`], o `EditSessionsWorkbenchService`).
//!
//! Pré-requisitos já portados (fatia 1): o `SCMService` com o contrato de `scm.ts`
//! ([`crate::contrib::scm::common::scm_service`], [`crate::contrib::scm::common::scm`]) e o `WorkspaceIdentityService`
//! ([`crate::services::workspaces::common::workspace_identity_service`]).
//!
//! Fatia 2b, ainda fora do porte: `browser/editSessions.contribution.ts` (a `EditSessionsContribution`, o registro dos singletons
//! `IEditSessionsLogService` e `IEditSessionsStorageService` e as vistas, com `EDIT_SESSIONS_VIEW_ICON`), com os 5 casos de
//! `test/browser/editSessions.test.ts`, na ordem do original: `Can apply edit session`,
//! `Path traversal in edit session is blocked (posix)`, `Path traversal in edit session is blocked (windows-style backslash)`,
//! `Valid change is applied while traversal sibling is blocked` e `Edit session not stored if there are no edits`. O
//! `suiteSetup` do teste monta o `SCMService` com um `_repositories` vazio ([`SCMService::repositories_by_id`]) e o
//! `WorkspaceIdentityService` sobre o `IEditSessionIdentityService`.
//!
//! [`SCMService::repositories_by_id`]: crate::contrib::scm::common::scm_service::SCMService::repositories_by_id

pub mod common;
pub mod edit_sessions_log_service;
pub mod edit_sessions_storage_client;
pub mod edit_sessions_storage_service;
