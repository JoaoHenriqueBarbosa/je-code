//! Porte de `src/vs/workbench/services/authentication/common/authentication.ts` e
//! `browser/authenticationAccessService.ts`: [`authentication`] (só o necessário, ver as divergências lá) e
//! [`authentication_access_service`].

#[allow(clippy::module_inception)] // `authentication.rs` é o porte de `authentication.ts`, como no original
pub mod authentication;
pub mod authentication_access_service;
