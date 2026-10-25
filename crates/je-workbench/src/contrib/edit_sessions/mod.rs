//! Porte de `src/vs/workbench/contrib/editSessions`: a sincronização das edições não salvas entre máquinas.
//!
//! Portado: `common/editSessions.ts` (tipos, constantes, `decodeEditSessionFileContent` e `hashedEditSessionId`).
//!
//! Pré-requisitos já portados (fatia 1): o `SCMService` com o contrato de `scm.ts`
//! ([`crate::contrib::scm::common::scm_service`], [`crate::contrib::scm::common::scm`]) e o `WorkspaceIdentityService`
//! ([`crate::services::workspaces::common::workspace_identity_service`]).
//!
//! Fatia 2, ainda fora do porte: `browser/editSessionsStorageService.ts` (o `EditSessionsWorkbenchService`) e
//! `browser/editSessions.contribution.ts` (a `EditSessionsContribution` e as vistas), com os 5 casos de
//! `test/browser/editSessions.test.ts`, na ordem do original: `Can apply edit session`,
//! `Path traversal in edit session is blocked (posix)`, `Path traversal in edit session is blocked (windows-style backslash)`,
//! `Valid change is applied while traversal sibling is blocked` e `Edit session not stored if there are no edits`. O
//! `suiteSetup` do teste monta o `SCMService` com um `_repositories` vazio ([`SCMService::repositories_by_id`]) e o
//! `WorkspaceIdentityService` sobre o `IEditSessionIdentityService`.
//!
//! [`SCMService::repositories_by_id`]: crate::contrib::scm::common::scm_service::SCMService::repositories_by_id

pub mod common;
