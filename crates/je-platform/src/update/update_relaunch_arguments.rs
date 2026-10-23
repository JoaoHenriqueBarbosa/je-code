//! Porte de `src/vs/platform/update/electron-main/updateRelaunchArguments.ts`: os argumentos de linha de
//! comando que atravessam o relançamento da atualização no Windows.
//!
//! Decisões de tradução:
//! - `keyof NativeParsedArgs` é o nome da chave (`&str`), lido do objeto dinâmico com os leitores de
//!   [`NativeParsedArgs`]: `typeof value === 'string'` é `get_string` e `args[key] === true` é um `Dynamic::Bool(true)`
//!   estrito (um texto ou um número não valem).
//! - `currentWorkingDirectory: string = cwd()` é um `Option<&str>`: `None` usa o diretório atual do processo.
//! - `win32` do `path.ts` é o [`PathModule::win32`] sobre um ambiente fixo cujo diretório atual é o recebido.

use je_base::dynamic::Dynamic;
use je_base::path::{Environment, FixedEnvironment, PathModule, Platform, ProcessEnvironment};

use crate::environment::native_parsed_args::NativeParsedArgs;

/// Single-value string CLI arguments that define the installation's environment (data/extension dirs, locale, secret
/// storage, proxy, rendering, diagnostics). Preserved across an update relaunch so the relaunched instance does not fall
/// back to the defaults (e.g. an empty extensions dir). See #322663.
const RELAUNCH_STRING_ARGUMENTS: &[&str] = &[
    // Data & extension directories
    "user-data-dir",
    "extensions-dir",
    "builtin-extensions-dir",
    "extensions-download-dir",
    "shared-data-dir",
    "agents-user-data-dir",
    "agents-extensions-dir",
    "agent-plugins-dir",
    // Localization
    "locale",
    // Diagnostics
    "crash-reporter-directory",
    // Secret storage
    "password-store",
    // Proxy
    "proxy-server",
    "proxy-bypass-list",
    "proxy-pac-url",
    // Rendering / Chromium / tracing
    "force-device-scale-factor",
    "ozone-platform",
    "js-flags",
    "enable-tracing",
    "trace-startup-format",
    "trace-startup-file",
    "trace-startup-duration",
];

/// Boolean CLI switches the user opted into for this installation (rendering, sandboxing, networking, persistent
/// opt-outs) and that should survive an update relaunch. See #322663.
const RELAUNCH_FLAG_ARGUMENTS: &[&str] = &[
    // Rendering / sandboxing
    "disable-gpu",
    "disable-lcd-text",
    "disable-chromium-sandbox",
    "disable-gpu-sandbox",
    "disable-dev-shm-usage",
    "enable-coi",
    "force-renderer-accessibility",
    "enable-rdp-display-tracking",
    // Networking
    "ignore-certificate-errors",
    "allow-insecure-localhost",
    // Persistent opt-outs / environment
    "disable-crash-reporter",
    "disable-telemetry",
    "disable-updates",
    "disable-workspace-trust",
    "disable-experiments",
    "disable-layout-restore",
    "use-inmemory-secretstorage",
];

const RELAUNCH_NEGATED_FLAG_ARGUMENTS: &[&str] = &["no-sandbox", "no-proxy-server"];

/// Subset of [`RELAUNCH_STRING_ARGUMENTS`] whose values are file system paths. The installer relaunches the
/// application from the install directory, so relative values would resolve against a different working directory and
/// must be made absolute before they are carried forward.
const RELAUNCH_PATH_ARGUMENTS: &[&str] = &[
    "user-data-dir",
    "extensions-dir",
    "builtin-extensions-dir",
    "extensions-download-dir",
    "shared-data-dir",
    "agents-user-data-dir",
    "agents-extensions-dir",
    "agent-plugins-dir",
    "crash-reporter-directory",
    "trace-startup-file",
];

/// Quotes a single argument per `CommandLineToArgvW` rules so it survives being appended to the relaunched `Code.exe`
/// command line even when it contains spaces or quotes (e.g. a path).
pub fn quote_windows_argument(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_owned(); // nothing to quote
    }

    let mut result = String::from("\"");
    let mut backslashes = 0;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                result.push_str(&"\\".repeat(backslashes * 2 + 1));
                result.push('"');
                backslashes = 0;
            }
            _ => {
                result.push_str(&"\\".repeat(backslashes));
                result.push(ch);
                backslashes = 0;
            }
        }
    }
    result.push_str(&"\\".repeat(backslashes * 2));
    result.push('"');

    result
}

/// Resolves a path-valued argument against the current working directory of this process, so it keeps pointing at the
/// same location once the installer relaunches the application from the install directory. Windows path semantics are
/// used explicitly because these arguments are only ever consumed by the Windows installer.
fn to_absolute_path(value: &str, current_working_directory: &str) -> String {
    let environment = FixedEnvironment::new(Platform::Win32, current_working_directory);
    let win32 = PathModule::win32(&environment);
    if win32.is_absolute(value) { value.to_owned() } else { win32.resolve(&[current_working_directory, value]) }
}

/// Builds the Windows-quoted command line tail carrying the curated persistent arguments forward across an update
/// relaunch. Returns an empty string when there are no such arguments.
pub fn get_relaunch_arguments(args: &NativeParsedArgs, raw_args: &[&str], current_working_directory: Option<&str>) -> String {
    let current_working_directory = current_working_directory.map_or_else(|| ProcessEnvironment.cwd(), str::to_owned);
    let mut argv: Vec<String> = Vec::new();

    for key in RELAUNCH_STRING_ARGUMENTS {
        if let Some(value) = args.get_string(key).filter(|value| !value.is_empty()) {
            let value = if RELAUNCH_PATH_ARGUMENTS.contains(key) { to_absolute_path(&value, &current_working_directory) } else { value };
            argv.push(format!("--{key}={value}"));
        }
    }

    for key in RELAUNCH_FLAG_ARGUMENTS {
        if matches!(args.get(key), Dynamic::Bool(true)) {
            argv.push(format!("--{key}"));
        }
    }

    // Everything after the `--` end-of-options marker is a positional argument (e.g. a file literally named
    // `--no-sandbox`) and must not be mistaken for a switch the user opted into.
    let end_of_options = raw_args.iter().position(|arg| *arg == "--").unwrap_or(raw_args.len());
    let raw_options = &raw_args[..end_of_options];

    for key in RELAUNCH_NEGATED_FLAG_ARGUMENTS {
        let switch = format!("--{key}");
        if raw_options.contains(&switch.as_str()) {
            argv.push(switch);
        }
    }

    argv.iter().map(|arg| quote_windows_argument(arg)).collect::<Vec<_>>().join(" ")
}
