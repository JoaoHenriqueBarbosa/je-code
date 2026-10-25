//! Porte de `src/vs/workbench/contrib/editSessions`: a sincronização das edições não salvas entre máquinas.
//!
//! Portado: `common/editSessions.ts` (tipos, constantes, `decodeEditSessionFileContent` e `hashedEditSessionId`).
//!
//! Fora do porte, por falta da infraestrutura de que dependem (a contribuição `EditSessionsContribution` e os testes de
//! `editSessions.test.ts`): o `SCMService`, o `WorkspaceIdentityService`, o `EditSessionsWorkbenchService` e as vistas.

pub mod common;
