//! Porte de `src/vs/platform/policy/common`: o serviço de políticas ([`PolicyService`], [`NullPolicyService`],
//! [`AbstractPolicyService`]) e o [`FilePolicyService`], que lê as políticas de um arquivo JSON; as
//! configurações gerenciadas de arquivo (`fileManagedSettingsService`, `fileManagedSettingsIpc`) e o estado do
//! portão `managedSettingsFreshness`.
//!
//! `copilotManagedSettings` está portado inteiro (`copilot_managed_settings`): as chaves, a normalização
//! ([`copilot_managed_settings::normalize_managed_settings`]), a resolução entre canais
//! ([`copilot_managed_settings::pick_managed_settings`]), os callbacks `value`, os serviços (efetivo, nativo e
//! de arquivo), a projeção por declaração, `requiresCopilotAgentHost` e `resolveForceRemoteSettingsRefresh`.
//! `multiplexPolicyService` (`multiplex_policy_service`), `policyIpc` (`policy_ipc`), `nativeManagedSettingsIpc`
//! (`native_managed_settings_ipc`) e `node/nativeManagedSettingsService` (`native_managed_settings_service`, com o
//! módulo nativo `@vscode/policy-watcher` como fábrica injetável) estão portados. Não há mais nada de
//! `platform/policy` por portar.

pub mod file_policy_service;
#[allow(clippy::module_inception)] // `policy.rs` é o porte de `policy.ts`, como no original
pub mod policy;

pub use file_policy_service::FilePolicyService;
pub use policy::*;

pub mod copilot_managed_settings;
pub mod file_managed_settings_ipc;
pub mod file_managed_settings_service;
pub mod managed_settings_freshness;

pub mod multiplex_policy_service;
pub mod native_managed_settings_ipc;
pub mod native_managed_settings_service;
pub mod policy_ipc;
