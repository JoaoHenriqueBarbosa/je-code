//! Porte de `src/vs/workbench/services/authentication/common/authentication.ts` e
//! `browser/authenticationAccessService.ts`: [`authentication`] (só o necessário, ver as divergências lá) e
//! [`authentication_access_service`].

#[allow(clippy::module_inception)] // `authentication.rs` é o porte de `authentication.ts`, como no original
pub mod authentication;
pub mod authentication_access_service;

// Serviços de uso e de acesso por MCP e a API de consulta (`authenticationUsageService`,
// `authenticationMcpAccessService`, `authenticationMcpUsageService`, `authenticationMcpService` (só a
// interface), `authenticationQuery` e `authenticationQueryService`).
pub mod authentication_mcp_access_service;
pub mod authentication_mcp_service;
pub mod authentication_mcp_usage_service;
pub mod authentication_query;
pub mod authentication_query_service;
pub mod authentication_usage_service;
