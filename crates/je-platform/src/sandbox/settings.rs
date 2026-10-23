//! Porte de `src/vs/platform/sandbox/common/settings.ts`: os identificadores das configurações do sandbox
//! do agente e os valores que elas assumem.

use je_base::platform::OperatingSystem;

/// `IAgentSandboxFileSystemSetting`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentSandboxFileSystemSetting {
    pub allow_read: Option<Vec<String>>,
    pub allow_write: Option<Vec<String>>,
    pub deny_read: Option<Vec<String>>,
    pub deny_write: Option<Vec<String>>,
}

/// `IAgentSandboxUserConfiguredPaths`: as permissões de caminho configuradas pelo usuário.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentSandboxUserConfiguredPaths {
    pub readwrite_paths: Option<Vec<String>>,
    pub readonly_paths: Option<Vec<String>>,
    pub denied_paths: Option<Vec<String>>,
}

/// `normalizeSandboxFileSystemPath`: converte os separadores para o sistema de execução do sandbox, sem
/// resolver caminhos nem padrões.
pub fn normalize_sandbox_file_system_path(path: &str, os: OperatingSystem) -> String {
    if os == OperatingSystem::Windows { path.replace('/', "\\") } else { path.to_owned() }
}

/// `isTerminalSandboxSupported`: se o harness local consegue isolar comandos de terminal no sistema.
pub fn is_terminal_sandbox_supported(os: OperatingSystem) -> bool {
    os != OperatingSystem::Windows
}

/// `AgentSandboxSettingId` (`const enum`): os identificadores das configurações do sandbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentSandboxSettingId {
    AgentSandboxEnabled,
    AgentSandboxAllowNetwork,
    AgentSandboxAllowLocalNetwork,
    AgentSandboxAllowUnsandboxedCommands,
    AgentSandboxMcpServers,
    AgentSandboxLspServers,
    AgentSandboxAuthenticateGit,
    AgentSandboxAuthenticateGh,
    AgentSandboxAllowDevToolAccess,
    AgentSandboxUserConfiguredPaths,
    AgentSandboxRetryWithAllowNetworkRequests,
    AgentSandboxLinuxFileSystem,
    AgentSandboxMacFileSystem,
    AgentSandboxAdvancedRuntime,
}

impl AgentSandboxSettingId {
    /// O valor de texto do membro do `const enum`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AgentSandboxEnabled => "chat.agent.sandbox.enabled",
            Self::AgentSandboxAllowNetwork => "chat.agent.sandbox.network.allowNetwork",
            Self::AgentSandboxAllowLocalNetwork => "chat.agent.sandbox.network.allowLocalNetwork",
            Self::AgentSandboxAllowUnsandboxedCommands => "chat.agent.sandbox.allowUnsandboxedCommands",
            Self::AgentSandboxMcpServers => "chat.agent.sandbox.mcpServers",
            Self::AgentSandboxLspServers => "chat.agent.sandbox.lspServers",
            Self::AgentSandboxAuthenticateGit => "chat.agent.sandbox.credentials.authenticategit",
            Self::AgentSandboxAuthenticateGh => "chat.agent.sandbox.credentials.authenticategh",
            Self::AgentSandboxAllowDevToolAccess => "chat.agent.sandbox.fileSystem.allowDevToolAccess",
            Self::AgentSandboxUserConfiguredPaths => "chat.agent.sandbox.fileSystem.userConfiguredPaths",
            Self::AgentSandboxRetryWithAllowNetworkRequests => "chat.agent.sandbox.retryWithAllowNetworkRequests",
            Self::AgentSandboxLinuxFileSystem => "chat.agent.sandbox.fileSystem.linux",
            Self::AgentSandboxMacFileSystem => "chat.agent.sandbox.fileSystem.mac",
            Self::AgentSandboxAdvancedRuntime => "chat.agent.sandbox.advanced.runtime",
        }
    }
}

/// `AgentSandboxEnabledValue` (`const enum`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentSandboxEnabledValue {
    Off,
    On,
}

impl AgentSandboxEnabledValue {
    /// O valor de texto do membro do `const enum` (`'off'`, `'on'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
        }
    }
}

/// `AgentSandboxEnabledSettingValue`: `AgentSandboxEnabledValue | boolean`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentSandboxEnabledSettingValue {
    Value(AgentSandboxEnabledValue),
    Bool(bool),
}

impl From<AgentSandboxEnabledValue> for AgentSandboxEnabledSettingValue {
    fn from(value: AgentSandboxEnabledValue) -> Self {
        Self::Value(value)
    }
}

impl From<bool> for AgentSandboxEnabledSettingValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

/// `normalizeAgentSandboxEnabledValue`.
pub fn normalize_agent_sandbox_enabled_value(value: AgentSandboxEnabledSettingValue) -> AgentSandboxEnabledValue {
    match value {
        AgentSandboxEnabledSettingValue::Bool(true) => AgentSandboxEnabledValue::On,
        AgentSandboxEnabledSettingValue::Bool(false) => AgentSandboxEnabledValue::Off,
        AgentSandboxEnabledSettingValue::Value(value) => value,
    }
}

/// `isAgentSandboxEnabledValue`.
pub fn is_agent_sandbox_enabled_value(value: Option<AgentSandboxEnabledSettingValue>) -> bool {
    value.is_some_and(|value| normalize_agent_sandbox_enabled_value(value) != AgentSandboxEnabledValue::Off)
}
