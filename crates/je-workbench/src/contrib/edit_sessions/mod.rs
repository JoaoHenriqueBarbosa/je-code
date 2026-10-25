//! Porte de `src/vs/workbench/contrib/editSessions`: a sincronização das edições não salvas entre máquinas.
//!
//! Portado: `common/editSessions.ts` ([`common`]: tipos, constantes, chaves de contexto, contratos `IEditSessionsStorageService` e
//! `IEditSessionsLogService`, `decodeEditSessionFileContent`, `hashedEditSessionId` e a leitura do `EditSession`),
//! `common/editSessionsLogService.ts` ([`edit_sessions_log_service`]), `common/editSessionsStorageClient.ts`
//! ([`edit_sessions_storage_client`]), `browser/editSessionsStorageService.ts` ([`edit_sessions_storage_service`], o
//! `EditSessionsWorkbenchService`), `common/workspaceStateSync.ts` ([`workspace_state_sync`], o `WorkspaceStateSynchroniser`),
//! `browser/editSessionsFileSystemProvider.ts` ([`edit_sessions_file_system_provider`]), `browser/editSessionsViews.ts`
//! ([`edit_sessions_views`], a vista Cloud Changes, as ações dela e o provedor de dados) e `browser/editSessions.contribution.ts`
//! das linhas 78 a 884 ([`edit_sessions_contribution`]: a `EditSessionsContribution` e a ação do módulo
//! `workbench.action.continueOn.extensions`).
//!
//! Pré-requisitos já portados (fatia 1): o `SCMService` com o contrato de `scm.ts`
//! ([`crate::contrib::scm::common::scm_service`], [`crate::contrib::scm::common::scm`]) e o `WorkspaceIdentityService`
//! ([`crate::services::workspaces::common::workspace_identity_service`]).
//!
//! Fatia 2c, ainda fora do porte: `browser/editSessions.contribution.ts` das linhas 886 em diante, que a contribuição chama por
//! [`edit_sessions_contribution::ContinueEditSessionDestinations`] (`registerContributedEditSessionOptions`,
//! `generateStandaloneOptionCommand`, `registerContinueInLocalFolderAction`, `pickContinueEditSessionDestination`,
//! `resolveDestination` e `createPickItems`), a classe `ContinueEditSessionItem`, o ponto de extensão `continueEditSession`
//! (`ICommand`), o registro da contribuição (`registerWorkbenchContribution(EditSessionsContribution, LifecyclePhase.Restored)`)
//! e o das configurações (`workbench.experimental.cloudChanges.autoStore`, `workbench.cloudChanges.autoResume`,
//! `workbench.cloudChanges.continueOn` e `workbench.experimental.cloudChanges.partialMatches.enabled`), mais as linhas 74 e 75
//! (`registerSingleton(IEditSessionsLogService, ...)` e `registerSingleton(IEditSessionsStorageService, ...)`) e as constantes
//! `openLocalFolderCommand` e `queryParamName` não usadas aqui (a 2c lê `CONTINUE_WORKING_ON_COMMAND_ID`,
//! `INSTALL_ADDITIONAL_CONTINUE_ON_OPTIONS_COMMAND_ID`, [`edit_sessions_contribution::install_additional_continue_on_options_title`] e
//! `ContinueDestination` de [`edit_sessions_contribution`]). Com os 5 casos de `test/browser/editSessions.test.ts`, na ordem do
//! original: `Can apply edit session`, `Path traversal in edit session is blocked (posix)`,
//! `Path traversal in edit session is blocked (windows-style backslash)`, `Valid change is applied while traversal sibling is blocked`
//! e `Edit session not stored if there are no edits`. O `suiteSetup` do teste monta o `SCMService` com um `_repositories` vazio
//! ([`SCMService::repositories_by_id`]) e o `WorkspaceIdentityService` sobre o `IEditSessionIdentityService`; os casos chamam
//! [`edit_sessions_contribution::EditSessionsContribution::resume_edit_session`] (com
//! [`edit_sessions_contribution::ResumeEditSessionArgs::default`]) e
//! [`edit_sessions_contribution::EditSessionsContribution::store_edit_session`].
//!
//! [`SCMService::repositories_by_id`]: crate::contrib::scm::common::scm_service::SCMService::repositories_by_id

pub mod common;
pub mod edit_sessions_contribution;
pub mod edit_sessions_file_system_provider;
pub mod edit_sessions_log_service;
pub mod edit_sessions_storage_client;
pub mod edit_sessions_storage_service;
pub mod edit_sessions_views;
pub mod workspace_state_sync;
