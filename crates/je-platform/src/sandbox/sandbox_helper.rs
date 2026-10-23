//! Porte de `src/vs/platform/sandbox/node/sandboxHelper.ts`: verifica se o `bubblewrap` e o `socat` estão
//! instalados, se o `bubblewrap` consegue criar o sandbox e, quando falta algo, qual comando instala.
//!
//! Divergências: as `Promise` viram chamadas síncronas (o `Promise.all` do original roda as consultas em
//! sequência); os parâmetros opcionais de `checkSandboxDependenciesWith` (`linux`, a sonda, os resolvedores)
//! são `Option` e valem o mesmo padrão do original quando `None`; o `slice(0, 1000)` do erro conta unidades
//! UTF-16 e nunca parte um par substituto ao meio.

use std::time::Duration;

use je_base::platform::Platform;

use super::node_support::{exec_file, find_executable, get_os_release_info, process_is_root};
use super::sandbox_helper_service::{ISandboxHelperService, SandboxDependencyStatus};

/// `FindCommand`.
pub type FindCommand<'a> = dyn Fn(&str) -> Option<String> + 'a;
/// `BubblewrapProbe`.
pub type BubblewrapProbe<'a> = dyn Fn(&str) -> BubblewrapProbeResult + 'a;
/// `ResolveLinuxInstallEnvironment`.
pub type ResolveLinuxInstallEnvironment<'a> = dyn Fn() -> LinuxInstallEnvironment + 'a;
/// `ResolveAppArmorRestriction`.
pub type ResolveAppArmorRestriction<'a> = dyn Fn() -> Option<bool> + 'a;

/// O resultado de uma sonda do `bubblewrap`: `{ usable, error? }`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BubblewrapProbeResult {
    pub usable: bool,
    pub error: Option<String>,
}

/// `{ distributionIds, isRoot }`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LinuxInstallEnvironment {
    pub distribution_ids: Vec<String>,
    pub is_root: bool,
}

struct LinuxDependencyInstaller {
    distribution_ids: &'static [&'static str],
    /// `[executable, command]`.
    commands: &'static [(&'static str, &'static str)],
}

const LINUX_DEPENDENCY_INSTALL_COMMANDS: &[LinuxDependencyInstaller] = &[
    LinuxDependencyInstaller {
        distribution_ids: &["debian", "ubuntu", "linuxmint", "pop", "elementary", "kali", "raspbian"],
        commands: &[("apt-get", "apt-get update && apt-get install -y"), ("apt", "apt update && apt install -y")],
    },
    LinuxDependencyInstaller { distribution_ids: &["fedora", "rhel", "centos", "rocky", "almalinux"], commands: &[("dnf", "dnf install -y"), ("yum", "yum install -y")] },
    LinuxDependencyInstaller { distribution_ids: &["arch", "manjaro", "endeavouros"], commands: &[("pacman", "pacman -S --needed --noconfirm")] },
    LinuxDependencyInstaller { distribution_ids: &["suse", "opensuse", "opensuse-leap", "opensuse-tumbleweed"], commands: &[("zypper", "zypper --non-interactive install")] },
    LinuxDependencyInstaller { distribution_ids: &["alpine"], commands: &[("apk", "apk add")] },
];

/// `findCommand(...)` e o teste de verdade do JavaScript sobre `string | undefined` (`""` é falso).
fn found(find_command: &FindCommand<'_>, command: &str) -> Option<String> {
    find_command(command).filter(|path| !path.is_empty())
}

/// Os primeiros 1000 caracteres UTF-16 (`detail.slice(0, 1000)`).
fn slice_utf16(text: &str, max_units: usize) -> String {
    let mut units = 0;
    text.chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= max_units
        })
        .collect()
}

pub struct SandboxHelperService;

impl SandboxHelperService {
    /// `checkSandboxDependenciesWith`. `linux`, `probe_bubblewrap`, `resolve_install_environment` e
    /// `resolve_app_armor_restriction` valem, quando `None`, o `isLinux`, a sonda real do `bwrap`, a leitura
    /// do `os-release` e a do `apparmor_restrict_unprivileged_userns`.
    pub fn check_sandbox_dependencies_with(
        find_command: &FindCommand<'_>,
        linux: Option<bool>,
        probe_bubblewrap: Option<&BubblewrapProbe<'_>>,
        resolve_install_environment: Option<&ResolveLinuxInstallEnvironment<'_>>,
        resolve_app_armor_restriction: Option<&ResolveAppArmorRestriction<'_>>,
    ) -> Option<SandboxDependencyStatus> {
        if !linux.unwrap_or_else(|| Platform::detect().is_linux) {
            return None;
        }

        let bubblewrap_path = found(find_command, "bwrap");
        let socat_path = found(find_command, "socat");
        let bubblewrap_probe = match &bubblewrap_path {
            Some(path) => probe_bubblewrap.map_or_else(|| Self::probe_bubblewrap(path), |probe| probe(path)),
            None => BubblewrapProbeResult::default(),
        };
        let dependency_install_command = if bubblewrap_path.is_none() || socat_path.is_none() {
            Self::find_dependency_install_command(find_command, resolve_install_environment)
        } else {
            None
        };
        let apparmor_restricts_unprivileged_user_namespaces = if bubblewrap_path.is_some() && !bubblewrap_probe.usable {
            resolve_app_armor_restriction.map_or_else(Self::resolve_app_armor_restriction, |resolve| resolve())
        } else {
            None
        };

        Some(SandboxDependencyStatus {
            bubblewrap_installed: bubblewrap_path.is_some(),
            bubblewrap_usable: bubblewrap_probe.usable,
            bubblewrap_error: bubblewrap_probe.error,
            socat_installed: socat_path.is_some(),
            dependency_install_command,
            apparmor_restricts_unprivileged_user_namespaces,
        })
    }

    fn resolve_app_armor_restriction() -> Option<bool> {
        let apparmor_restriction = std::fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns").ok();
        apparmor_restriction.map(|restriction| restriction.trim() == "1")
    }

    fn find_dependency_install_command(find_command: &FindCommand<'_>, resolve_install_environment: Option<&ResolveLinuxInstallEnvironment<'_>>) -> Option<String> {
        let environment = resolve_install_environment.map_or_else(Self::resolve_linux_install_environment, |resolve| resolve());
        let installer = LINUX_DEPENDENCY_INSTALL_COMMANDS
            .iter()
            .find(|candidate| candidate.distribution_ids.iter().any(|id| environment.distribution_ids.iter().any(|distribution_id| distribution_id == *id)))?;
        let elevation = if environment.is_root {
            ""
        } else if found(find_command, "sudo").is_some() {
            "sudo "
        } else {
            return None;
        };
        installer
            .commands
            .iter()
            .find(|(executable, _)| found(find_command, executable).is_some())
            .map(|(_, command)| command.split(" && ").map(|command| format!("{elevation}{command}")).collect::<Vec<_>>().join(" && "))
    }

    fn resolve_linux_install_environment() -> LinuxInstallEnvironment {
        let release_info = get_os_release_info(&|_| {});
        let distribution_ids = release_info
            .iter()
            .flat_map(|info| std::iter::once(info.id.as_str()).chain(info.id_like.iter().flat_map(|id_like| id_like.split(|c: char| c.is_whitespace() || c == '\u{feff}'))))
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect();
        LinuxInstallEnvironment { distribution_ids, is_root: process_is_root() }
    }

    fn probe_bubblewrap(command: &str) -> BubblewrapProbeResult {
        let result = exec_file(command, &["--unshare-net", "--dev-bind", "/", "/", "echo", "ok"], Duration::from_millis(5000));
        if result.error.is_none() && result.stdout.trim() == "ok" {
            return BubblewrapProbeResult { usable: true, error: None };
        }

        let stderr = result.stderr.trim();
        let detail = if !stderr.is_empty() {
            stderr.to_owned()
        } else if let Some(message) = result.error.filter(|message| !message.is_empty()) {
            message
        } else {
            format!("Unexpected output: {}", result.stdout.trim())
        };
        BubblewrapProbeResult { usable: false, error: Some(slice_utf16(&detail, 1000)) }
    }
}

impl ISandboxHelperService for SandboxHelperService {
    fn check_sandbox_dependencies(&self) -> Option<SandboxDependencyStatus> {
        Self::check_sandbox_dependencies_with(&|command| find_executable(command, None, None, None, None), None, None, None, None)
    }
}
