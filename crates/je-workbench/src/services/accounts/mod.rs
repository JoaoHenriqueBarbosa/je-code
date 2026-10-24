//! Porte de `src/vs/workbench/services/accounts`: as configurações gerenciadas pela empresa que vêm da conta
//! (`browser/managedSettings.ts`), o enrolamento de instâncias do GitHub Enterprise
//! (`common/githubEnterprise.ts`) e o serviço da conta padrão (`browser/defaultAccount.ts`, mais os contratos
//! de `platform/defaultAccount/common/defaultAccount.ts`).

pub mod default_account;
pub mod github_enterprise;
pub mod managed_settings;
