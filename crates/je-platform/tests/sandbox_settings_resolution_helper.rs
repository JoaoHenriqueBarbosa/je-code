//! Porte de `src/vs/platform/sandbox/test/common/sandboxSettingsResolutionHelper.test.ts` (suíte `SandboxSettingsResolutionHelper`), caso a caso e na mesma ordem.
//!
//! Divergências: `undefined` é `None`; `Object.freeze` não tem equivalente porque as funções recebem
//! empréstimos imutáveis (`&[String]`), de modo que "sem mutar as fontes" é garantido pelo compilador e o
//! teste confere o resultado e que as fontes seguem iguais. `ensureNoDisposablesAreLeakedInTestSuite` não
//! tem o que vigiar (nada aqui é descartável).

use je_platform::sandbox::sandbox_settings_resolution_helper::{NetworkRestrictions, ResolvedNetworkHosts, SandboxSettingsResolutionHelper};
use je_platform::sandbox::settings::{AgentSandboxEnabledValue, AgentSandboxUserConfiguredPaths};

/// Uma lista de textos como `Option<Vec<String>>` (presente), para `.as_deref()`.
fn list(items: &[&str]) -> Option<Vec<String>> {
    Some(items.iter().map(|item| (*item).to_owned()).collect())
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn hosts(allowed: &[&str], blocked: &[&str]) -> ResolvedNetworkHosts {
    ResolvedNetworkHosts { allowed_hosts: strings(allowed), blocked_hosts: strings(blocked) }
}

const MANAGED_VALUES: [Option<bool>; 3] = [None, Some(false), Some(true)];

#[test]
fn only_managed_true_forces_enablement() {
    for local in [None, Some(AgentSandboxEnabledValue::Off), Some(AgentSandboxEnabledValue::On)] {
        assert_eq!(MANAGED_VALUES.map(|managed| SandboxSettingsResolutionHelper::resolve_enabled(local, managed)), [local, local, Some(AgentSandboxEnabledValue::On)]);
    }
}

#[test]
fn managed_false_denies_access_without_widening_local_restrictions() {
    for local in MANAGED_VALUES {
        assert_eq!(MANAGED_VALUES.map(|managed| SandboxSettingsResolutionHelper::resolve_allow_access(local, managed)), [local, Some(false), local]);
    }
}

#[test]
fn network_restrictions_distinguish_outbound_blocking_from_host_filtering() {
    assert_eq!(
        [
            SandboxSettingsResolutionHelper::get_network_restrictions(Some(AgentSandboxEnabledValue::Off.into()), Some(true)),
            SandboxSettingsResolutionHelper::get_network_restrictions(Some(AgentSandboxEnabledValue::On.into()), Some(true)),
            SandboxSettingsResolutionHelper::get_network_restrictions(Some(AgentSandboxEnabledValue::On.into()), Some(false)),
        ],
        [
            NetworkRestrictions { sandbox_enabled: false, allow_network: true, apply_domain_restrictions: false },
            NetworkRestrictions { sandbox_enabled: true, allow_network: true, apply_domain_restrictions: true },
            NetworkRestrictions { sandbox_enabled: true, allow_network: false, apply_domain_restrictions: false },
        ]
    );
}

#[test]
fn managed_allowlists_override_local_values_including_an_explicitly_empty_managed_list() {
    let resolve = SandboxSettingsResolutionHelper::resolve_network_hosts;
    assert_eq!(
        [
            resolve(None, None, None, None),
            resolve(list(&[]).as_deref(), list(&[]).as_deref(), list(&[]).as_deref(), list(&[]).as_deref()),
            resolve(None, None, list(&["example.com"]).as_deref(), list(&["malicious.com"]).as_deref()),
            resolve(list(&[]).as_deref(), list(&[]).as_deref(), list(&["example.com"]).as_deref(), list(&["malicious.com"]).as_deref()),
            resolve(list(&["example.com"]).as_deref(), list(&["malicious.com"]).as_deref(), None, None),
            resolve(list(&["example.com"]).as_deref(), list(&["malicious.com"]).as_deref(), list(&[]).as_deref(), list(&[]).as_deref()),
        ],
        [
            hosts(&[], &[]),
            hosts(&[], &[]),
            hosts(&["example.com"], &["malicious.com"]),
            hosts(&["example.com"], &["malicious.com"]),
            hosts(&["example.com"], &["malicious.com"]),
            hosts(&[], &["malicious.com"]),
        ]
    );
}

#[test]
fn uses_the_managed_allowlist_and_combines_blocks_without_mutating_either_source() {
    let local_allowed = strings(&["local.example", "shared.example", "shared.example"]);
    let managed_allowed = strings(&["shared.example", "managed.example"]);
    let local_blocked = strings(&["local.blocked", "shared.blocked"]);
    let managed_blocked = strings(&["shared.blocked", "managed.blocked", "shared.example"]);
    assert_eq!(
        SandboxSettingsResolutionHelper::resolve_network_hosts(Some(&local_allowed), Some(&local_blocked), Some(&managed_allowed), Some(&managed_blocked)),
        hosts(&["shared.example", "managed.example"], &["local.blocked", "shared.blocked", "managed.blocked", "shared.example"])
    );
    assert_eq!(local_allowed, strings(&["local.example", "shared.example", "shared.example"]));
    assert_eq!(managed_blocked, strings(&["shared.blocked", "managed.blocked", "shared.example"]));
}

#[test]
fn preserves_host_patterns_and_wildcard_blocks_for_the_runtime_to_interpret() {
    assert_eq!(
        SandboxSettingsResolutionHelper::resolve_network_hosts(
            list(&["EXAMPLE.COM.", "*.example.com", "[::1]"]).as_deref(),
            list(&["local.blocked"]).as_deref(),
            list(&["example.com", "api.example.com"]).as_deref(),
            list(&["*"]).as_deref(),
        ),
        hosts(&["example.com", "api.example.com"], &["local.blocked", "*"])
    );
}

#[test]
fn uses_managed_path_grants_and_combines_denied_paths_without_mutating_either_source() {
    let local = AgentSandboxUserConfiguredPaths {
        readwrite_paths: list(&["/local-write", "/shared-write"]),
        readonly_paths: list(&["/local-read"]),
        denied_paths: list(&["/local-denied", "/shared-denied"]),
    };
    let managed = AgentSandboxUserConfiguredPaths {
        readwrite_paths: list(&["/managed-write", "/managed-write"]),
        readonly_paths: list(&[]),
        denied_paths: list(&["/shared-denied", "/managed-denied"]),
    };
    assert_eq!(
        SandboxSettingsResolutionHelper::resolve_file_system_paths(Some(&local), Some(&managed)),
        AgentSandboxUserConfiguredPaths {
            readwrite_paths: list(&["/managed-write"]),
            readonly_paths: list(&[]),
            denied_paths: list(&["/local-denied", "/shared-denied", "/managed-denied"]),
        }
    );
    assert_eq!(local.readwrite_paths, list(&["/local-write", "/shared-write"]));
    assert_eq!(managed.readwrite_paths, list(&["/managed-write", "/managed-write"]));
}

#[test]
fn only_managed_true_forces_server_sandboxing() {
    for local in MANAGED_VALUES {
        assert_eq!(MANAGED_VALUES.map(|managed| SandboxSettingsResolutionHelper::resolve_sandbox_servers(local, managed)), [local, local, Some(true)]);
    }
}

#[test]
fn bypass_requires_managed_permission_under_a_forced_sandbox_and_never_widens_the_local_choice() {
    for local in MANAGED_VALUES {
        for enabled in MANAGED_VALUES {
            assert_eq!(
                MANAGED_VALUES.map(|managed| SandboxSettingsResolutionHelper::resolve_allow_bypass(local, managed, enabled)),
                [if enabled == Some(true) { Some(false) } else { local }, Some(false), local]
            );
        }
    }
}
