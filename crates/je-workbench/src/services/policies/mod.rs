//! Porte de `src/vs/workbench/services/policies`: `browser/policyTelemetry.contribution.ts` vira o módulo
//! `policy_telemetry_contribution`, e `common/accountPolicyService.ts` o `account_policy_service` (com a ponte do
//! callback `value` de uma política em `policy_value_callback`).

pub mod account_policy_service;
pub mod policy_telemetry_contribution;
pub mod policy_value_callback;
