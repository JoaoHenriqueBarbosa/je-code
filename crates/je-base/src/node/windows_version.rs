//! Porte de `src/vs/base/node/windowsVersion.ts`: a versão real do Windows (o `release` e o número de
//! build), lida do registro e guardada em cache.
//!
//! No Windows 8.1 ou mais novo, o `os.release()` pode devolver uma versão errada (a API `GetVersionEx`
//! devolve valores de compatibilidade quando o aplicativo não tem um manifesto adequado), então o original
//! lê o registro. Ver https://github.com/microsoft/vscode/issues/197444.
//!
//! Divergências:
//! - O original lê o registro pelo módulo nativo `@vscode/windows-registry`. Aqui o registro é lido com o
//!   `reg query` do próprio Windows (`std::process::Command`, Rust seguro, sem dependência nova), e o
//!   `os.release()` vem de `/proc/sys/kernel/osrelease` ou de `uname -r` (Linux e macOS) e de `cmd /c ver`
//!   (Windows). Qualquer falha de leitura cai no mesmo ramo do `catch` do original (`ignore`).
//! - O cache do módulo (`versionInfo`) é um `OnceLock`. As funções `async` do original continuam `async`.
//! - `isWindows` é o `Platform::detect().is_windows` do processo atual.

use std::process::Command;
use std::sync::OnceLock;

use crate::platform::Platform;

/// O `versionInfo` do original: `{ release: string; buildNumber: number }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsVersionInfo {
    pub release: String,
    pub build_number: i64,
}

static VERSION_INFO: OnceLock<WindowsVersionInfo> = OnceLock::new();

/// `isWindows` do `platform.ts`.
fn is_windows() -> bool {
    Platform::detect().is_windows
}

/// `os.release()`.
pub fn os_release() -> String {
    if let Ok(release) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        return release.trim().to_owned();
    }
    if is_windows() {
        // "Microsoft Windows [Version 10.0.19045.3693]": o Node devolve só os três primeiros componentes.
        let output = run_command("cmd", &["/c", "ver"]).unwrap_or_default();
        let version = output.split("Version ").nth(1).map(|rest| rest.trim_end().trim_end_matches(']')).unwrap_or_default();
        return version.split('.').take(3).collect::<Vec<_>>().join(".");
    }
    run_command("uname", &["-r"]).map(|release| release.trim().to_owned()).unwrap_or_default()
}

fn run_command(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A linha `    <name>    <REG_TYPE>    <data>` que o `reg query` imprime para o valor pedido.
fn query_registry_value(key: &str, name: &str) -> Option<(String, String)> {
    let output = run_command("reg", &["query", &format!("HKEY_LOCAL_MACHINE\\{key}"), "/v", name])?;
    output.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        if !parts.next()?.eq_ignore_ascii_case(name) {
            return None;
        }
        let value_type = parts.next()?.to_owned();
        Some((value_type, parts.collect::<Vec<_>>().join(" ")))
    })
}

/// `Registry.GetStringRegKey('HKEY_LOCAL_MACHINE', key, name)`: `None` é o `undefined`.
fn get_string_reg_key(key: &str, name: &str) -> Option<String> {
    query_registry_value(key, name).filter(|(value_type, _)| value_type == "REG_SZ").map(|(_, data)| data)
}

/// `Registry.GetDWORDRegKey('HKEY_LOCAL_MACHINE', key, name)`: `None` é o `undefined`.
fn get_dword_reg_key(key: &str, name: &str) -> Option<u32> {
    let (value_type, data) = query_registry_value(key, name)?;
    if value_type != "REG_DWORD" {
        return None;
    }
    u32::from_str_radix(data.strip_prefix("0x").or_else(|| data.strip_prefix("0X"))?, 16).ok()
}

/// `parseInt(text, 10)`: o prefixo de dígitos depois dos espaços e do sinal; `None` é o `NaN`.
fn parse_int(text: &str) -> Option<i64> {
    let text = text.trim_start();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    if end == 0 {
        return None;
    }
    let value = digits[..end].parse::<i64>().unwrap_or(i64::MAX);
    Some(if negative { -value } else { value })
}

/// Parses the Windows build number from os.release().
/// This is used as a fallback when registry reading is not available.
///
/// `(/(\d+)\.(\d+)\.(\d+)/g).exec(os.release())`, devolvendo o `parseInt` do terceiro grupo.
fn get_windows_build_number_from_os_release() -> i64 {
    let release: Vec<char> = os_release().chars().collect();
    let digits_from = |start: usize| release[start..].iter().take_while(|c| c.is_ascii_digit()).count();
    for start in 0..release.len() {
        let first = digits_from(start);
        if first == 0 || release.get(start + first) != Some(&'.') {
            continue;
        }
        let second_start = start + first + 1;
        let second = digits_from(second_start);
        if second == 0 || release.get(second_start + second) != Some(&'.') {
            continue;
        }
        let third_start = second_start + second + 1;
        let third = digits_from(third_start);
        if third == 0 {
            continue;
        }
        return parse_int(&release[third_start..third_start + third].iter().collect::<String>()).unwrap_or(0);
    }
    0
}

/// O corpo de `initWindowsVersionInfo` depois do teste de cache.
fn read_version_info() -> WindowsVersionInfo {
    if !is_windows() {
        return WindowsVersionInfo { release: os_release(), build_number: 0 };
    }

    let version_key = "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion";
    let mut build_number: Option<i64> = None;
    let mut release: Option<String> = None;

    let build = get_string_reg_key(version_key, "CurrentBuild");
    if let Some(build) = &build {
        build_number = parse_int(build);
    }

    let major = get_dword_reg_key(version_key, "CurrentMajorVersionNumber");
    let minor = get_dword_reg_key(version_key, "CurrentMinorVersionNumber");
    if let (Some(major), Some(minor), Some(build)) = (major, minor, &build) {
        release = Some(format!("{major}.{minor}.{build}"));
    }

    WindowsVersionInfo {
        release: release.filter(|release| !release.is_empty()).unwrap_or_else(os_release),
        build_number: build_number.filter(|build_number| *build_number != 0).unwrap_or_else(get_windows_build_number_from_os_release),
    }
}

/// Initializes the Windows version cache by reading from the registry.
///
/// On Windows 8.1+, the `os.release()` function may return incorrect version numbers
/// due to the deprecated GetVersionEx API returning compatibility-shimmed values
/// when the application doesn't have a proper manifest. Reading from the registry
/// gives us the real version.
///
/// See: https://github.com/microsoft/vscode/issues/197444
pub async fn init_windows_version_info() {
    VERSION_INFO.get_or_init(read_version_info);
}

/// Gets Windows version information from the registry.
/// Devolve a versão no formato Major.Minor.Build (por exemplo, "10.0.19041").
pub async fn get_windows_release() -> String {
    VERSION_INFO.get_or_init(read_version_info).release.clone()
}

/// Gets the Windows build number from the registry.
/// Devolve o número de build do Windows (por exemplo, 19041 para o Windows 10 2004).
pub async fn get_windows_build_number_async() -> i64 {
    VERSION_INFO.get_or_init(read_version_info).build_number
}

/// Synchronous version of getWindowsBuildNumberAsync().
/// Devolve o número de build do Windows (por exemplo, 19041 para o Windows 10 2004).
pub fn get_windows_build_number_sync() -> i64 {
    match VERSION_INFO.get() {
        Some(info) => info.build_number,
        None if is_windows() => get_windows_build_number_from_os_release(),
        None => 0,
    }
}

/// Gets the cached Windows release string synchronously.
/// Falls back to os.release() if the cache hasn't been initialized yet.
pub fn get_windows_release_sync() -> String {
    VERSION_INFO.get().map_or_else(os_release, |info| info.release.clone())
}
