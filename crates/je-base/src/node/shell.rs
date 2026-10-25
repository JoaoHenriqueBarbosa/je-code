//! Porte de `src/vs/base/node/shell.ts`: `getSystemShell`, o shell padrão do _sistema_ (não o shell padrão do
//! terminal do VS Code).
//!
//! Notas de tradução:
//! - `userInfo().shell` do Node lê o `/etc/passwd` pelo usuário do processo (`getpwuid`). Sem chamada de libc,
//!   o porte procura no `/etc/passwd` o usuário de `USER` (ou `LOGNAME`) e lê o sétimo campo; sem entrada, o
//!   resultado é o mesmo de uma exceção do original (nenhum shell), e o fluxo cai em `sh`.
//! - Os caches de módulo (`_TERMINAL_DEFAULT_SHELL_UNIX_LIKE` e `_TERMINAL_DEFAULT_SHELL_WINDOWS`) são
//!   `static`s com `Mutex`.

use std::sync::Mutex;

use super::powershell::get_first_available_power_shell_installation;
use super::processes::get_windows_shell;
use crate::platform::{OperatingSystem, Platform};
use crate::process::ProcessEnvironment;

static TERMINAL_DEFAULT_SHELL_UNIX_LIKE: Mutex<Option<String>> = Mutex::new(None);
static TERMINAL_DEFAULT_SHELL_WINDOWS: Mutex<Option<String>> = Mutex::new(None);

/// Gets the detected default shell for the _system_, not to be confused with VS Code's _default_
/// shell that the terminal uses by default.
/// `os`: the platform to detect the shell of.
pub async fn get_system_shell(os: OperatingSystem, env: &ProcessEnvironment) -> String {
    let platform = Platform::detect();
    if os == OperatingSystem::Windows {
        if platform.is_windows {
            return get_system_shell_windows().await;
        }
        // Don't detect Windows shell when not on Windows
        return get_windows_shell(Some(env));
    }

    get_system_shell_unix_like(os, env, &platform)
}

/// `userInfo().shell`: o shell do usuário no `/etc/passwd`.
fn user_info_shell() -> Option<String> {
    let user = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).ok()?;
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.next()? == user).then(|| fields.nth(5).map(str::to_owned)).flatten()
    })
}

fn get_system_shell_unix_like(os: OperatingSystem, env: &ProcessEnvironment, platform: &Platform) -> String {
    // Only use $SHELL for the current OS
    if (platform.is_linux && os == OperatingSystem::Macintosh) || (platform.is_macintosh && os == OperatingSystem::Linux) {
        return "/bin/bash".to_owned();
    }

    let mut cached = TERMINAL_DEFAULT_SHELL_UNIX_LIKE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if cached.as_deref().is_none_or(str::is_empty) {
        let unix_like_terminal = if platform.is_windows {
            "/bin/bash".to_owned() // for WSL
        } else {
            let mut shell = env.get("SHELL").cloned().filter(|value| !value.is_empty());

            if shell.is_none() {
                // It's possible for $SHELL to be unset, this API reads /etc/passwd. See https://github.com/github/codespaces/issues/1639
                // Node docs: "Throws a SystemError if a user has no username or homedir."
                shell = user_info_shell().filter(|value| !value.is_empty());
            }

            let shell = shell.unwrap_or_else(|| "sh".to_owned());

            // Some systems have $SHELL set to /bin/false which breaks the terminal
            if shell == "/bin/false" { "/bin/bash".to_owned() } else { shell }
        };
        *cached = Some(unix_like_terminal);
    }
    cached.clone().unwrap_or_default()
}

async fn get_system_shell_windows() -> String {
    let cached = TERMINAL_DEFAULT_SHELL_WINDOWS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    if let Some(shell) = cached.filter(|shell| !shell.is_empty()) {
        return shell;
    }
    let exe_path = get_first_available_power_shell_installation().await.map(|installation| installation.exe_path).unwrap_or_default();
    *TERMINAL_DEFAULT_SHELL_WINDOWS.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(exe_path.clone());
    exe_path
}
