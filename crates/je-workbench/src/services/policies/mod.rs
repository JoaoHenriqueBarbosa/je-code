//! Porte de `src/vs/workbench/services/policies`: `browser/policyTelemetry.contribution.ts` vira o módulo
//! `policy_telemetry_contribution`, e `common/accountPolicyService.ts` o `account_policy_service` (com a ponte do
//! callback `value` de uma política em `policy_value_callback`). A apresentação do bloqueio por versão mínima do cliente
//! é `common/managedSettingsUpdate.ts` (`managed_settings_update`), `browser/managedSettingsUpdateService.ts`
//! (`managed_settings_update_service`) e `browser/managedSettingsUpdate.contribution.ts`
//! (`managed_settings_update_contribution`).

pub mod account_policy_service;
pub mod policy_telemetry_contribution;
pub mod policy_value_callback;
pub mod managed_settings_update;
pub mod managed_settings_update_contribution;
pub mod managed_settings_update_service;
