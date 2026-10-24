//! Porte parcial de `src/vs/platform/agentHost/common/agentHostPolicyReadiness.ts`.
//!
//! Divergências: só `IAgentHostPolicyGap` e `getAgentHostPolicyGapImpact` foram portados, que é o que o
//! relatório Markdown do workbench usa. `getAgentHostPolicyGaps` e `hasPolicyRequirement` dependem do
//! registro de configuração com políticas, de `readAgentHostOTelPolicySettings` e das configurações do
//! sandbox e do filtro de rede, e ficam para o porte dessas partes. O `localize` do original é o
//! [`je_base::nls::localize`], com a chave do original.

use je_base::nls::localize;

use super::agent_host_policy_support::AgentHostPolicySupportStatus;

/// `IAgentHostPolicyGap`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentHostPolicyGap {
    pub policy_name: String,
    pub setting_id: String,
    pub status: AgentHostPolicySupportStatus,
}

/// `getAgentHostPolicyGapImpact`: o impacto visível ao usuário, sem expor valores de política,
/// endpoints nem credenciais.
pub fn get_agent_host_policy_gap_impact(policy_name: &str) -> String {
    match policy_name {
        "ChatToolsEligibleForAutoApproval" => localize("policyGap.toolApproval", "Allow All is disabled session-wide, but tools excluded by policy can still be approved automatically through other approval paths.", &[]),
        "ChatToolsTerminalEnableAutoApprove" => localize("policyGap.terminalApproval", "Native shell commands require approval through the bridge. Custom terminal tools do not have equivalent managed-ask coverage in every approval mode.", &[]),
        "ChatAgentNetworkFilter" | "ChatAgentAllowedNetworkDomains" | "ChatAgentDeniedNetworkDomains" => localize("policyGap.network", "Shared browser tools and custom terminal sandboxing honor domain lists, but native SDK sandboxing does not apply those lists. The bridge covers only supported runtime URL denies. Access may be allowed or blocked differently from Local.", &[]),
        "ChatMCP" => localize("policyGap.mcpAccess", "The workbench filters forwarded MCP collections, but Agent Host can independently discover and run MCP servers despite the VS Code MCP access restriction.", &[]),
        "ChatAllowedMcpServers" | "ChatDeniedMcpServers" | "ChatAllowManagedMcpServersOnly" => localize("policyGap.mcpLists", "VS Code-only server restrictions are not delivered to all Agent Host discovery paths. Runtime managed settings have separate enforcement; this report does not assume the two sources are equivalent.", &[]),
        "McpEnterpriseManagedAuthIdp" => localize("policyGap.mcpAuth", "Forwarded MCP authentication does not use Local's enterprise-managed identity-provider lookup. Enterprise single sign-on may be unavailable; runtime-owned authentication paths are not certified by this check.", &[]),
        "ChatPluginsEnabled" | "ChatEnabledPlugins" => localize("policyGap.plugins", "The workbench filters synchronized plugins, but runtime discovery and more specific customization decisions do not have equivalent policy coverage.", &[]),
        "ChatExtraMarketplaces" => localize("policyGap.marketplaces", "Shared marketplace discovery and installation honor additional marketplaces. Runtime-owned marketplace operations do not receive VS Code-only values and need separate verification.", &[]),
        "ChatStrictPluginOnlyCustomization" => localize("policyGap.customizations", "The workbench filters synchronized customizations, but runtime discovery and standalone instructions do not have equivalent coverage across policy sources.", &[]),
        "ChatHooks" => localize("policyGap.hooks", "The Local-only hooks switch does not disable runtime-discovered Agent Host hooks.", &[]),
        "ChatAllowManagedHooksOnly" => localize("policyGap.managedHooks", "Shared prompt discovery filters hooks, but the VS Code-only requirement is not delivered to runtime file hooks. Runtime-native managed-hook and SDK callback behavior needs separate verification.", &[]),
        "ChatAgentMode" => localize("policyGap.agentMode", "Agent Host session targets can remain available in the editor even when agent mode is disabled by policy.", &[]),
        "CopilotOtelProtocol" => localize("policyGap.otelProtocol", "For this local Copilot/VS Code policy diagnostic, inherited OTLP protocol configuration can still override an active collector exporter. It does not certify remote hosts or independently fetched runtime policy.", &[]),
        "CopilotOtelCaptureIdentity" => localize("policyGap.otelIdentity", "The Agent Host pipeline honors identity capture and suppression. Authenticated runtime account attribution still requires a runtime update and end-to-end verification; direct runtime exports use their own identity controls.", &[]),
        "CopilotOtelHeaders" => localize("policyGap.otelHeaders", "VS Code-only exporter headers do not reach Agent Host. Export requiring those headers may fail; runtime-managed telemetry does not establish coverage of the host's own exporter.", &[]),
        _ => localize("policyGap.unknown", "Agent Host does not fully enforce this configured requirement. Review its policy support before migrating.", &[]),
    }
}
