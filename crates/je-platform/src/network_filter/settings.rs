//! Porte de `src/vs/platform/networkFilter/common/settings.ts`: os identificadores das configurações do
//! filtro de rede do agente.

/// `AgentNetworkDomainSettingId` (`const enum`): os identificadores das configurações de filtragem de
/// domínio de rede do agente.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentNetworkDomainSettingId {
    NetworkFilter,
    AllowedNetworkDomains,
    DeniedNetworkDomains,
}

impl AgentNetworkDomainSettingId {
    /// O valor de texto do membro do `const enum`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NetworkFilter => "chat.agent.networkFilter",
            Self::AllowedNetworkDomains => "chat.agent.sandbox.network.allowedDomains",
            Self::DeniedNetworkDomains => "chat.agent.sandbox.network.deniedDomains",
        }
    }
}
