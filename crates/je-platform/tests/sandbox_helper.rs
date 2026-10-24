//! Porte de `src/vs/platform/sandbox/test/node/sandboxHelper.test.ts` (suíte `SandboxHelperService`), caso a caso e na mesma ordem.
//!
//! Divergências: só o `findCommand` é assíncrono (um futuro já resolvido); a sonda e os resolvedores são
//! síncronos. Cada caso `async` do original roda em `run_with_real_timers` (o original não usa relógio
//! falso); os parâmetros que o original omite ou passa como `undefined` são `None`. O laço
//! `for (...) test(...)` de sete distribuições vira sete testes gerados por uma macro, na mesma ordem.
//! `ensureNoDisposablesAreLeakedInTestSuite` não tem o que vigiar (nada aqui é descartável).

mod common;

use std::cell::{Cell, RefCell};

use common::run_with_real_timers;
use je_base::node::pfs::LocalFuture;
use je_platform::sandbox::sandbox_helper::{BubblewrapProbeResult, LinuxInstallEnvironment, SandboxHelperService};
use je_platform::sandbox::sandbox_helper_service::SandboxDependencyStatus;

/// O `async () => value` do original: um futuro já resolvido.
fn ready(value: Option<String>) -> LocalFuture<'static, Option<String>> {
    Box::pin(std::future::ready(value))
}

fn found(command: &str) -> LocalFuture<'static, Option<String>> {
    ready(Some(format!("/usr/bin/{command}")))
}

fn environment(distribution_ids: &[&str], is_root: bool) -> LinuxInstallEnvironment {
    LinuxInstallEnvironment { distribution_ids: distribution_ids.iter().map(|id| (*id).to_owned()).collect(), is_root }
}

#[test]
fn does_not_inspect_sandbox_dependencies_on_non_linux_platforms() {
    run_with_real_timers(async {
        let find_called = Cell::new(false);
        let result = SandboxHelperService::check_sandbox_dependencies_with(
            &|_: &str| {
                find_called.set(true);
                ready(None)
            },
            Some(false),
            None,
            None,
            None,
        )
        .await;

        assert_eq!(result, None);
        assert!(!find_called.get());
    });
}

#[test]
fn reports_missing_bubblewrap_without_running_its_capability_probe() {
    run_with_real_timers(async {
        let probe_called = Cell::new(false);
        let result = SandboxHelperService::check_sandbox_dependencies_with(
            &|command: &str| ready((command == "socat").then(|| "/usr/bin/socat".to_owned())),
            Some(true),
            Some(&|_: &str| {
                probe_called.set(true);
                BubblewrapProbeResult { usable: true, error: None }
            }),
            None,
            None,
        )
        .await;

        assert!(!probe_called.get());
        let result = result.expect("resultado");
        assert!(!result.bubblewrap_installed);
        assert!(!result.bubblewrap_usable);
        assert!(result.socat_installed);
    });
}

#[test]
fn reports_bubblewrap_usable_when_its_capability_probe_succeeds() {
    run_with_real_timers(async {
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
        )
        .await;

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
    });
}

#[test]
fn reports_the_probe_error_when_bubblewrap_is_unusable() {
    run_with_real_timers(async {
        let result = SandboxHelperService::check_sandbox_dependencies_with(
            &found,
            Some(true),
            Some(&|_: &str| BubblewrapProbeResult { usable: false, error: Some("No permissions to create namespace".to_owned()) }),
            None,
            Some(&|| Some(true)),
        )
        .await;

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
    });
}

macro_rules! detects_package_manager {
    ($name:ident, $distribution_id:literal, $package_manager:literal, $expected_command:literal) => {
        #[test]
        fn $name() {
            run_with_real_timers(async {
                let result = SandboxHelperService::check_sandbox_dependencies_with(
                    &|command: &str| ready((command == "socat" || command == "sudo" || command == $package_manager).then(|| format!("/usr/bin/{command}"))),
                    Some(true),
                    None,
                    Some(&|| environment(&[$distribution_id], false)),
                    None,
                )
                .await;

                assert_eq!(result.and_then(|status| status.dependency_install_command).as_deref(), Some($expected_command));
            });
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
async fn install_command(available: &'static [&'static str], distribution_ids: &[&str], is_root: bool) -> Option<String> {
    SandboxHelperService::check_sandbox_dependencies_with(
        &|command: &str| ready(available.contains(&command).then(|| format!("/usr/bin/{command}"))),
        Some(true),
        None,
        Some(&|| environment(distribution_ids, is_root)),
        None,
    )
    .await
    .and_then(|status| status.dependency_install_command)
}

#[test]
fn uses_id_like_to_detect_a_derivative_distribution() {
    run_with_real_timers(async {
        assert_eq!(install_command(&["socat", "sudo", "dnf"], &["custom-linux", "fedora"], false).await.as_deref(), Some("sudo dnf install -y"));
    });
}

#[test]
fn uses_the_native_package_manager_when_multiple_managers_are_available() {
    run_with_real_timers(async {
        assert_eq!(install_command(&["socat", "sudo", "apt-get", "pacman"], &["arch"], false).await.as_deref(), Some("sudo pacman -S --needed --noconfirm"));
    });
}

#[test]
fn does_not_use_sudo_when_running_as_root() {
    run_with_real_timers(async {
        assert_eq!(install_command(&["socat", "apk"], &["alpine"], true).await.as_deref(), Some("apk add"));
    });
}

#[test]
fn does_not_use_sudo_for_chained_apt_get_commands_when_running_as_root() {
    run_with_real_timers(async {
        assert_eq!(install_command(&["socat", "apt-get"], &["debian"], true).await.as_deref(), Some("apt-get update && apt-get install -y"));
    });
}

#[test]
fn does_not_offer_dependency_installation_to_a_non_root_user_without_sudo() {
    run_with_real_timers(async {
        assert_eq!(install_command(&["socat", "pacman"], &["arch"], false).await, None);
    });
}

#[test]
fn does_not_offer_dependency_installation_without_a_supported_package_manager() {
    run_with_real_timers(async {
        assert_eq!(install_command(&["socat"], &["unknown"], false).await, None);
    });
}
