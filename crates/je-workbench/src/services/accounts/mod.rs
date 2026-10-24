//! Porte de `src/vs/workbench/services/accounts`: as configurações gerenciadas pela empresa que vêm da conta
//! (`browser/managedSettings.ts`) e o enrolamento de instâncias do GitHub Enterprise
//! (`common/githubEnterprise.ts`).

pub mod github_enterprise;
pub mod managed_settings;
