//! Porte de `src/vs/platform/policy/common`: o serviço de políticas ([`PolicyService`], [`NullPolicyService`],
//! [`AbstractPolicyService`]) e o [`FilePolicyService`], que lê as políticas de um arquivo JSON.
//!
//! Não portados: `copilotManagedSettings`, `fileManagedSettings*`, `managedSettingsFreshness`,
//! `multiplexPolicyService`, `nativeManagedSettingsIpc` e `policyIpc` (configurações gerenciadas pelo servidor
//! ou pelo sistema operacional e IPC entre processos; dependem de `vs/base/common/defaultAccount` e
//! `vs/base/parts/ipc`, que não existem aqui, e nenhum teste do escopo os exige).

pub mod file_policy_service;
#[allow(clippy::module_inception)] // `policy.rs` é o porte de `policy.ts`, como no original
pub mod policy;

pub use file_policy_service::FilePolicyService;
pub use policy::*;
