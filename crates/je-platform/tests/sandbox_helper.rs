//! Porte de `src/vs/platform/sandbox/test/node/sandboxHelper.test.ts` (suíte `SandboxHelperService`), caso a caso e na mesma ordem.
//!
//! Divergências: as funções do original são `async` e aqui são síncronas; os parâmetros que o original
//! omite ou passa como `undefined` são `None`. O laço `for (...) test(...)` de sete distribuições vira
//! sete testes gerados por uma macro, na mesma ordem. `ensureNoDisposablesAreLeakedInTestSuite` não tem o
//! que vigiar (nada aqui é descartável).

use std::cell::{Cell, RefCell};

use je_platform::sandbox::sandbox_helper::{BubblewrapProbeResult, LinuxInstallEnvironment, SandboxHelperService};
use je_platform::sandbox::sandbox_helper_service::SandboxDependencyStatus;

fn found(command: &str) -> Option<String> {
    Some(format!("/usr/bin/{command}"))
}

fn environment(distribution_ids: &[&str], is_root: bool) -> LinuxInstallEnvironment {
    LinuxInstallEnvironment { distribution_ids: distribution_ids.iter().map(|id| (*id).to_owned()).collect(), is_root }
}

#[test]
fn does_not_inspect_sandbox_dependencies_on_non_linux_platforms() {
    let find_called = Cell::new(false);
    let result = SandboxHelperService::check_sandbox_dependencies_with(
        &|_: &str| {
            find_called.set(true);
            None
        },
        Some(false),
        None,
        None,
        None,
    );

    assert_eq!(result, None);
    assert!(!find_called.get());
}

#[test]
fn reports_missing_bubblewrap_without_running_its_capability_probe() {
    let probe_called = Cell::new(false);
    let result = SandboxHelperService::check_sandbox_dependencies_with(
        &|command: &str| (command == "socat").then(|| "/usr/bin/socat".to_owned()),
        Some(true),
        Some(&|_: &str| {
            probe_called.set(true);
            BubblewrapProbeResult { usable: true, error: None }
        }),
        None,
        None,
    );

    assert!(!probe_called.get());
    let result = result.expect("resultado");
    assert!(!result.bubblewrap_installed);
    assert!(!result.bubblewrap_usable);
    assert!(result.socat_installed);
}

#[test]
fn reports_bubblewrap_usable_when_its_capability_probe_succeeds() {
    let probed_command: RefCell<Option<String>> = RefCell::new(None);
    let result = SandboxHelperService::check_sandbox_dependencies_with(
        &found,
        Some(true),
        Some(&|command: &str| {
            *probed_command.borrow_mut() = Some(command.to_owned());
            BubblewrapProbeResult { usable: true, error: None }
        }),
        None,
        None,
    );

    assert_eq!(probed_command.borrow().as_deref(), Some("/usr/bin/bwrap"));
    assert_eq!(
        result,
        Some(SandboxDependencyStatus {
            bubblewrap_installed: true,
            bubblewrap_usable: true,
            bubblewrap_error: None,
            socat_installed: true,
            dependency_install_command: None,
            apparmor_restricts_unprivileged_user_namespaces: None,
        })
    );
}

#[test]
fn reports_the_probe_error_when_bubblewrap_is_unusable() {
    let result = SandboxHelperService::check_sandbox_dependencies_with(
        &found,
        Some(true),
        Some(&|_: &str| BubblewrapProbeResult { usable: false, error: Some("No permissions to create namespace".to_owned()) }),
        None,
        Some(&|| Some(true)),
    );

    assert_eq!(
        result,
        Some(SandboxDependencyStatus {
            bubblewrap_installed: true,
            bubblewrap_usable: false,
            bubblewrap_error: Some("No permissions to create namespace".to_owned()),
            socat_installed: true,
            dependency_install_command: None,
            apparmor_restricts_unprivileged_user_namespaces: Some(true),
        })
    );
}

macro_rules! detects_package_manager {
    ($name:ident, $distribution_id:literal, $package_manager:literal, $expected_command:literal) => {
        #[test]
        fn $name() {
            let result = SandboxHelperService::check_sandbox_dependencies_with(
                &|command: &str| (command == "socat" || command == "sudo" || command == $package_manager).then(|| format!("/usr/bin/{command}")),
                Some(true),
                None,
                Some(&|| environment(&[$distribution_id], false)),
                None,
            );

            assert_eq!(result.and_then(|status| status.dependency_install_command).as_deref(), Some($expected_command));
        }
    };
}

detects_package_manager!(detects_apt_get_for_dependency_installation, "debian", "apt-get", "sudo apt-get update && sudo apt-get install -y");
detects_package_manager!(detects_apt_for_dependency_installation, "ubuntu", "apt", "sudo apt update && sudo apt install -y");
detects_package_manager!(detects_dnf_for_dependency_installation, "fedora", "dnf", "sudo dnf install -y");
detects_package_manager!(detects_yum_for_dependency_installation, "centos", "yum", "sudo yum install -y");
detects_package_manager!(detects_pacman_for_dependency_installation, "arch", "pacman", "sudo pacman -S --needed --noconfirm");
detects_package_manager!(detects_zypper_for_dependency_installation, "opensuse", "zypper", "sudo zypper --non-interactive install");
detects_package_manager!(detects_apk_for_dependency_installation, "alpine", "apk", "sudo apk add");

/// Roda a verificação com `socat` instalado, sem `bwrap`, e devolve o comando de instalação.
fn install_command(available: &'static [&'static str], distribution_ids: &[&str], is_root: bool) -> Option<String> {
    SandboxHelperService::check_sandbox_dependencies_with(
        &|command: &str| available.contains(&command).then(|| format!("/usr/bin/{command}")),
        Some(true),
        None,
        Some(&|| environment(distribution_ids, is_root)),
        None,
    )
    .and_then(|status| status.dependency_install_command)
}

#[test]
fn uses_id_like_to_detect_a_derivative_distribution() {
    assert_eq!(install_command(&["socat", "sudo", "dnf"], &["custom-linux", "fedora"], false).as_deref(), Some("sudo dnf install -y"));
}

#[test]
fn uses_the_native_package_manager_when_multiple_managers_are_available() {
    assert_eq!(install_command(&["socat", "sudo", "apt-get", "pacman"], &["arch"], false).as_deref(), Some("sudo pacman -S --needed --noconfirm"));
}

#[test]
fn does_not_use_sudo_when_running_as_root() {
    assert_eq!(install_command(&["socat", "apk"], &["alpine"], true).as_deref(), Some("apk add"));
}

#[test]
fn does_not_use_sudo_for_chained_apt_get_commands_when_running_as_root() {
    assert_eq!(install_command(&["socat", "apt-get"], &["debian"], true).as_deref(), Some("apt-get update && apt-get install -y"));
}

#[test]
fn does_not_offer_dependency_installation_to_a_non_root_user_without_sudo() {
    assert_eq!(install_command(&["socat", "pacman"], &["arch"], false), None);
}

#[test]
fn does_not_offer_dependency_installation_without_a_supported_package_manager() {
    assert_eq!(install_command(&["socat"], &["unknown"], false), None);
}
