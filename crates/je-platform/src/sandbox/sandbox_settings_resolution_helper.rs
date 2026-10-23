//! Porte de `src/vs/platform/sandbox/common/sandboxSettingsResolutionHelper.ts`: resolve as configurações
//! locais do sandbox contra o piso gerenciado, sem mudar as preferências salvas.
//!
//! Divergências: `SandboxFileSystemPaths` e `IAgentSandboxUserConfiguredPaths` têm os mesmos três campos,
//! então são o mesmo tipo ([`AgentSandboxUserConfiguredPaths`]); `isSandboxNetworkRestrictions` recebe o
//! valor dinâmico (`unknown`) do original.

use std::collections::HashSet;

use je_base::dynamic::Dynamic;

use super::settings::{AgentSandboxEnabledSettingValue, AgentSandboxEnabledValue, AgentSandboxUserConfiguredPaths, is_agent_sandbox_enabled_value};

/// `SandboxFileSystemPaths`.
pub type SandboxFileSystemPaths = AgentSandboxUserConfiguredPaths;

/// `ISandboxNetworkRestrictions`: as restrições de rede efetivas que o Agent Host do Copilot resolve para
/// uma ferramenta de cliente do navegador integrado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxNetworkRestrictions {
    pub sandbox_enabled: bool,
    pub allow_network: bool,
    pub allowed_domains: Vec<String>,
    pub denied_domains: Vec<String>,
}

/// O que `getNetworkRestrictions` devolve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkRestrictions {
    pub sandbox_enabled: bool,
    pub allow_network: bool,
    pub apply_domain_restrictions: bool,
}

/// O que `resolveNetworkHosts` devolve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedNetworkHosts {
    pub allowed_hosts: Vec<String>,
    pub blocked_hosts: Vec<String>,
}

/// `isSandboxNetworkRestrictions`.
pub fn is_sandbox_network_restrictions(value: &Dynamic) -> bool {
    if value.is_null() || value.type_of() != "object" {
        return false;
    }
    let has_all_fields = ["sandboxEnabled", "allowNetwork", "allowedDomains", "deniedDomains"].into_iter().all(|key| value.has_property(&key.into()));
    let is_string_array = |key: &str| match value.get_str(key) {
        Dynamic::Array(items) => items.to_vec().iter().all(|domain| matches!(domain, Dynamic::String(_))),
        _ => false,
    };
    has_all_fields
        && matches!(value.get_str("sandboxEnabled"), Dynamic::Bool(_))
        && matches!(value.get_str("allowNetwork"), Dynamic::Bool(_))
        && is_string_array("allowedDomains")
        && is_string_array("deniedDomains")
}

/// Os itens sem repetição, na ordem da primeira ocorrência (`[...new Set(items)]`).
fn unique<'a>(items: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut seen = HashSet::new();
    items.into_iter().filter(|item| seen.insert((*item).as_str())).cloned().collect()
}

/// `managed?.field ?? local?.field ?? []`.
fn grant_list(
    local: Option<&SandboxFileSystemPaths>,
    managed: Option<&SandboxFileSystemPaths>,
    field: fn(&SandboxFileSystemPaths) -> Option<&[String]>,
) -> Vec<String> {
    unique(managed.and_then(field).or_else(|| local.and_then(field)).unwrap_or_default())
}

/// Resolve as configurações locais do sandbox contra o piso gerenciado.
pub struct SandboxSettingsResolutionHelper;

impl SandboxSettingsResolutionHelper {
    pub fn get_network_restrictions(enabled: Option<AgentSandboxEnabledSettingValue>, allow_network: Option<bool>) -> NetworkRestrictions {
        let sandbox_enabled = is_agent_sandbox_enabled_value(enabled);
        NetworkRestrictions { sandbox_enabled, allow_network: allow_network.unwrap_or(true), apply_domain_restrictions: sandbox_enabled && allow_network.unwrap_or(true) }
    }

    pub fn resolve_enabled(local: Option<AgentSandboxEnabledValue>, managed: Option<bool>) -> Option<AgentSandboxEnabledValue> {
        if managed == Some(true) { Some(AgentSandboxEnabledValue::On) } else { local }
    }

    pub fn resolve_sandbox_servers(local: Option<bool>, managed: Option<bool>) -> Option<bool> {
        if managed == Some(true) { Some(true) } else { local }
    }

    pub fn resolve_allow_bypass(local: Option<bool>, managed: Option<bool>, managed_enabled: Option<bool>) -> Option<bool> {
        if managed == Some(false) || (managed_enabled == Some(true) && managed != Some(true)) { Some(false) } else { local }
    }

    pub fn resolve_allow_access(local: Option<bool>, managed: Option<bool>) -> Option<bool> {
        if managed == Some(false) { Some(false) } else { local }
    }

    /// Uses a managed allowlist when present and combines blocklists; runtime enforcement remains authoritative.
    pub fn resolve_network_hosts(
        local_allowed_hosts: Option<&[String]>,
        local_blocked_hosts: Option<&[String]>,
        managed_allowed_hosts: Option<&[String]>,
        managed_blocked_hosts: Option<&[String]>,
    ) -> ResolvedNetworkHosts {
        ResolvedNetworkHosts {
            allowed_hosts: unique(managed_allowed_hosts.or(local_allowed_hosts).unwrap_or_default()),
            blocked_hosts: unique(local_blocked_hosts.unwrap_or_default().iter().chain(managed_blocked_hosts.unwrap_or_default())),
        }
    }

    /// Uses managed grant lists when present and combines denied paths; runtime enforcement remains authoritative.
    pub fn resolve_file_system_paths(local: Option<&SandboxFileSystemPaths>, managed: Option<&SandboxFileSystemPaths>) -> AgentSandboxUserConfiguredPaths {
        fn denied(paths: Option<&SandboxFileSystemPaths>) -> &[String] {
            paths.and_then(|paths| paths.denied_paths.as_deref()).unwrap_or_default()
        }
        AgentSandboxUserConfiguredPaths {
            readwrite_paths: Some(grant_list(local, managed, |paths| paths.readwrite_paths.as_deref())),
            readonly_paths: Some(grant_list(local, managed, |paths| paths.readonly_paths.as_deref())),
            denied_paths: Some(unique(denied(local).iter().chain(denied(managed)))),
        }
    }
}
