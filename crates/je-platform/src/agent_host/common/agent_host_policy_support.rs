//! Porte parcial de `src/vs/platform/agentHost/common/agentHostPolicySupport.ts`.
//!
//! Divergências: só o tipo `AgentHostPolicySupportStatus` foi portado. A tabela `agentHostPolicySupport`
//! (uma entrada por política de `build/lib/policies/policyData.jsonc`) fica para quando o registro de
//! políticas for portado junto de `getAgentHostPolicyGaps`.

/// `AgentHostPolicySupportStatus`.
///
/// - `Enforced`: o Agent Host aplica a política por completo.
/// - `Partial`: a cobertura é incompleta para alguns caminhos ou fontes de entrega.
/// - `NotEnforced`: o Agent Host não aplica a política.
/// - `NotApplicable`: a política está fora do escopo do Copilot Agent Host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentHostPolicySupportStatus {
    Enforced,
    Partial,
    NotEnforced,
    NotApplicable,
}
