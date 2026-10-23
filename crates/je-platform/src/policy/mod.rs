//! Porte de `src/vs/platform/policy/common`: o serviço de políticas ([`PolicyService`], [`NullPolicyService`],
//! [`AbstractPolicyService`]) e o [`FilePolicyService`], que lê as políticas de um arquivo JSON; as
//! configurações gerenciadas de arquivo (`fileManagedSettingsService`, `fileManagedSettingsIpc`) e o estado do
//! portão `managedSettingsFreshness`.
//!
//! `copilotManagedSettings` está portado só em parte (`copilot_managed_settings`): as chaves, a normalização
//! ([`copilot_managed_settings::normalize_managed_settings`]), a resolução entre canais
//! ([`copilot_managed_settings::pick_managed_settings`]), o `value` da política do modelo e o serviço de
//! arquivo, o que as configurações gerenciadas de arquivo exigem. Não portados: o restante de
//! `copilotManagedSettings` (serviço efetivo, serviço nativo, projeção por declaração,
//! `requiresCopilotAgentHost`, `resolveForceRemoteSettingsRefresh`), `multiplexPolicyService`,
//! `nativeManagedSettings*` e `policyIpc` (configurações gerenciadas pelo servidor ou pelo sistema
//! operacional e IPC entre processos; outras fatias).

pub mod file_policy_service;
#[allow(clippy::module_inception)] // `policy.rs` é o porte de `policy.ts`, como no original
pub mod policy;

pub use file_policy_service::FilePolicyService;
pub use policy::*;

pub mod copilot_managed_settings;
pub mod file_managed_settings_ipc;
pub mod file_managed_settings_service;
pub mod managed_settings_freshness;
