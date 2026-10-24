//! Porte de `src/vs/base/node/powershell.ts`: as instalações do PowerShell que existem na máquina (o
//! `pwsh.exe` do Program Files, do MSIX, da ferramenta global do .NET, do Scoop e o Windows PowerShell).
//!
//! Divergências:
//! - Os geradores assíncronos (`enumerateDefaultPowerShellInstallations`, `enumeratePowerShellInstallations`)
//!   devolvem a lista pronta: a busca dos candidatos só lê o sistema de arquivos e não tem efeito colateral, então
//!   adiantar a leitura dos que o consumidor não chegaria a pedir (o `getFirstAvailablePowerShellInstallation`
//!   para no primeiro) não muda o resultado.
//! - `process.arch` é o `ARCH` do Rust, `process.env` é a cópia do ambiente do processo (`process::env_var`) e
//!   `os.homedir()` é `USERPROFILE` no Windows e `HOME` nos demais.
//! - As regexes `/^\d+$/`, `/^Microsoft.PowerShell_.*\/` e `/^Microsoft.PowerShellPreview_.*\/` são
//!   reconhecedores escritos à mão com as mesmas classes de caracteres.

use crate::node::pfs::{self, SymlinkSupport};
use crate::path::{PathModule, ProcessEnvironment as HostEnvironment};
use crate::process::env_var;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arch {
    X64,
    X86,
    Arm,
}

fn process_arch() -> Arch {
    match std::env::consts::ARCH {
        "x86" => Arch::X86,
        "arm" | "aarch64" => Arch::Arm,
        _ => Arch::X64,
    }
}

/// O `osArch` do módulo, calculado das variáveis `PROCESSOR_ARCHITEW6432` e `PROCESSOR_ARCHITECTURE`.
fn os_arch() -> Arch {
    match env_var("PROCESSOR_ARCHITEW6432").filter(|value| !value.is_empty()) {
        Some(value) => {
            if value == "ARM64" {
                Arch::Arm
            } else {
                Arch::X64
            }
        }
        None => match env_var("PROCESSOR_ARCHITECTURE").as_deref() {
            Some("ARM64") => Arch::Arm,
            Some("X86") => Arch::X86,
            _ => Arch::X64,
        },
    }
}

/// `IPowerShellExeDetails`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IPowerShellExeDetails {
    pub display_name: String,
    pub exe_path: String,
}

/// `PossiblePowerShellExe`: pode não existir; `exists` confere uma vez só.
struct PossiblePowerShellExe {
    details: IPowerShellExeDetails,
    known_to_exist: Option<bool>,
}

impl PossiblePowerShellExe {
    fn new(exe_path: String, display_name: &str, known_to_exist: Option<bool>) -> Self {
        Self { details: IPowerShellExeDetails { display_name: display_name.to_owned(), exe_path }, known_to_exist }
    }

    async fn exists(&mut self) -> bool {
        if self.known_to_exist.is_none() {
            self.known_to_exist = Some(SymlinkSupport::exists_file(&self.details.exe_path).await);
        }
        self.known_to_exist == Some(true)
    }
}

fn path_module() -> PathModule<'static> {
    static HOST: HostEnvironment = HostEnvironment;
    PathModule::native(&HOST)
}

/// `os.homedir()`.
fn home_dir() -> String {
    let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    env_var(name).unwrap_or_default()
}

fn get_program_files_path(use_alternate_bitness: bool) -> Option<String> {
    let non_empty = |name: &str| env_var(name).filter(|value| !value.is_empty());
    if !use_alternate_bitness {
        // Just use the native system bitness
        return non_empty("ProgramFiles");
    }

    // We might be a 64-bit process looking for 32-bit program files
    if process_arch() == Arch::X64 {
        return non_empty("ProgramFiles(x86)");
    }

    // We might be a 32-bit process looking for 64-bit program files
    if os_arch() == Arch::X64 {
        return non_empty("ProgramW6432");
    }

    // We're a 32-bit process on 32-bit Windows, there is no other Program Files dir
    None
}

/// `IntRegex.test(text)`: `/^\d+$/`.
fn is_int(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// `parseInt(text, 10)` de um texto só de dígitos.
fn parse_int(text: &str) -> i64 {
    text.parse::<i64>().unwrap_or(i64::MAX)
}

async fn find_ps_core_windows_installation(use_alternate_bitness: bool, find_preview: bool) -> Option<PossiblePowerShellExe> {
    let program_files_path = get_program_files_path(use_alternate_bitness)?;

    let path = path_module();
    let power_shell_install_base_dir = path.join(&[&program_files_path, "PowerShell"]);

    // Ensure the base directory exists
    if !SymlinkSupport::exists_directory(&power_shell_install_base_dir).await {
        return None;
    }

    let mut highest_seen_version: i64 = -1;
    let mut pwsh_exe_path: Option<String> = None;
    for item in pfs::readdir(&power_shell_install_base_dir).await.unwrap_or_default() {
        let current_version: i64;
        if find_preview {
            // We are looking for something like "7-preview"

            // Preview dirs all have dashes in them
            let Some(dash_index) = item.find('-') else {
                continue;
            };

            // Verify that the part before the dash is an integer
            // and that the part after the dash is "preview"
            let int_part = &item[..dash_index];
            if !is_int(int_part) || &item[dash_index + 1..] != "preview" {
                continue;
            }

            current_version = parse_int(int_part);
        } else {
            // Search for a directory like "6" or "7"
            if !is_int(&item) {
                continue;
            }

            current_version = parse_int(&item);
        }

        // Ensure we haven't already seen a higher version
        if current_version <= highest_seen_version {
            continue;
        }

        // Now look for the file
        let exe_path = path.join(&[&power_shell_install_base_dir, &item, "pwsh.exe"]);
        if !SymlinkSupport::exists_file(&exe_path).await {
            continue;
        }

        pwsh_exe_path = Some(exe_path);
        highest_seen_version = current_version;
    }

    let pwsh_exe_path = pwsh_exe_path?;

    let bitness = if program_files_path.contains("x86") { " (x86)" } else { "" };
    let preview = if find_preview { " Preview" } else { "" };

    Some(PossiblePowerShellExe::new(pwsh_exe_path, &format!("PowerShell{preview}{bitness}"), Some(true)))
}

/// `/^Microsoft.PowerShell_.*/` (ou a variante `Preview`): o `.` aceita qualquer caractere que não seja
/// terminador de linha.
fn is_pwsh_msix_dir(subdir: &str, product: &str) -> bool {
    let Some(rest) = subdir.strip_prefix("Microsoft") else {
        return false;
    };
    let mut chars = rest.chars();
    matches!(chars.next(), Some(c) if !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')) && chars.as_str().starts_with(product)
}

async fn find_ps_core_msix(find_preview: bool) -> Option<PossiblePowerShellExe> {
    // We can't proceed if there's no LOCALAPPDATA path
    let local_app_data = env_var("LOCALAPPDATA").filter(|value| !value.is_empty())?;

    // Find the base directory for MSIX application exe shortcuts
    let path = path_module();
    let msix_app_dir = path.join(&[&local_app_data, "Microsoft", "WindowsApps"]);

    if !SymlinkSupport::exists_directory(&msix_app_dir).await {
        return None;
    }

    // Define whether we're looking for the preview or the stable
    let (product, pwsh_msix_name) = if find_preview { ("PowerShellPreview_", "PowerShell Preview (Store)") } else { ("PowerShell_", "PowerShell (Store)") };

    // We should find only one such application, so return on the first one
    for subdir in pfs::readdir(&msix_app_dir).await.unwrap_or_default() {
        if is_pwsh_msix_dir(&subdir, product) {
            let pwsh_msix_path = path.join(&[&msix_app_dir, &subdir, "pwsh.exe"]);
            return Some(PossiblePowerShellExe::new(pwsh_msix_path, pwsh_msix_name, None));
        }
    }

    // If we find nothing, return null
    None
}

fn find_ps_core_dotnet_global_tool() -> PossiblePowerShellExe {
    let dotnet_global_tool_exe_path = path_module().join(&[&home_dir(), ".dotnet", "tools", "pwsh.exe"]);

    PossiblePowerShellExe::new(dotnet_global_tool_exe_path, ".NET Core PowerShell Global Tool", None)
}

fn find_ps_core_scoop_installation() -> PossiblePowerShellExe {
    let path = path_module();
    let scoop_apps_dir = path.join(&[&home_dir(), "scoop", "apps"]);
    let scoop_pwsh = path.join(&[&scoop_apps_dir, "pwsh", "current", "pwsh.exe"]);

    PossiblePowerShellExe::new(scoop_pwsh, "PowerShell (Scoop)", None)
}

fn find_win_ps() -> PossiblePowerShellExe {
    let windir = env_var("windir").unwrap_or_default();
    let system_dir = if process_arch() == Arch::X86 && os_arch() != Arch::X86 { "SysNative" } else { "System32" };
    let win_ps_path = path_module().join(&[&windir, system_dir, "WindowsPowerShell", "v1.0", "powershell.exe"]);

    PossiblePowerShellExe::new(win_ps_path, "Windows PowerShell", Some(true))
}

/// Iterates through all the possible well-known PowerShell installations on a machine.
/// Returned values may not exist, but come with an `exists` check.
async fn enumerate_default_power_shell_installations() -> Vec<PossiblePowerShellExe> {
    let mut installations = Vec::new();

    // Find PSCore stable first
    installations.extend(find_ps_core_windows_installation(false, false).await);

    // Windows may have a 32-bit pwsh.exe
    installations.extend(find_ps_core_windows_installation(true, false).await);

    // Also look for the MSIX/UWP installation
    installations.extend(find_ps_core_msix(false).await);

    // Look for the .NET global tool
    // Some older versions of PowerShell have a bug in this where startup will fail,
    // but this is fixed in newer versions
    installations.push(find_ps_core_dotnet_global_tool());

    // Look for PSCore preview
    installations.extend(find_ps_core_windows_installation(false, true).await);

    // Find a preview MSIX
    installations.extend(find_ps_core_msix(true).await);

    // Look for pwsh-preview with the opposite bitness
    installations.extend(find_ps_core_windows_installation(true, true).await);

    installations.push(find_ps_core_scoop_installation());

    // Finally, get Windows PowerShell
    installations.push(find_win_ps());

    installations
}

/// Iterates through PowerShell installations on the machine according
/// to configuration passed in through the constructor.
/// PowerShell items returned by this object are verified
/// to exist on the filesystem.
pub async fn enumerate_power_shell_installations() -> Vec<IPowerShellExeDetails> {
    // Get the default PowerShell installations first
    let mut existing = Vec::new();
    for mut default_pwsh in enumerate_default_power_shell_installations().await {
        if default_pwsh.exists().await {
            existing.push(default_pwsh.details);
        }
    }
    existing
}

/// Returns the first available PowerShell executable found in the search order.
pub async fn get_first_available_power_shell_installation() -> Option<IPowerShellExeDetails> {
    enumerate_power_shell_installations().await.into_iter().next()
}
