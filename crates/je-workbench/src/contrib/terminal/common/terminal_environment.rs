//! Porte de `src/vs/workbench/contrib/terminal/common/terminalEnvironment.ts`: funções do ambiente, do `cwd` e
//! dos caminhos do terminal (a mescla de variáveis de ambiente, o `LANG`, o diretório de trabalho inicial, a
//! preparação de um caminho para o shell e o tempo de espera da integração de shell).
//!
//! Divergências:
//! - O original lê `isWindows`, `isMacintosh` e `language` ao carregar o módulo. Aqui são o
//!   `Platform::detect()` do processo atual. O `path` do original (`base/common/path`) é o
//!   `PathModule::native` sobre o ambiente do processo.
//! - `IProcessEnvironment` é o [`ProcessEnvironment`] (um mapa ordenado de `String` para `String`), então a
//!   ordem de varredura de `mergeEnvironments` no Windows (a ordem de inserção do objeto) é a ordem
//!   alfabética das chaves. O resultado só difere quando duas chaves do ambiente do processo diferem apenas
//!   pela caixa, o que o Windows nunca produz.
//! - `ITerminalEnvironment` é a lista de pares do porte de `terminal.ts` ([`ITerminalEnvironment`]), com o
//!   `null` e o `undefined` distintos em [`TerminalEnvironmentValue`].
//! - Os `string | URI` dos parâmetros são [`ShellResource`] (e `&str` ou `&URI` em `getWorkspaceForTerminal`
//!   e `getUriLabelForShell`). Os `'auto' | 'off' | 'on'` de `detectLocale` são [`DetectLocale`], e os
//!   `'unix-to-win' | 'win-to-unix'` do `getWslPath` são [`WslPathDirection`].
//! - Um `async` que lança é um `Result<_, Error>` (só `preparePathForShell` e `getUriLabelForShell`, que
//!   repassam a rejeição de `getWslPath`; os demais capturam os erros do resolvedor, como o original).
//! - `ITerminalBackend` ainda não foi portado: o `Pick<ITerminalBackend, 'getWslPath'>` do original é o
//!   trait [`ITerminalBackendWslPath`]. O mesmo vale para `IHistoryService` e para
//!   `IConfigurationResolverService`, que aqui só trazem o único método que este módulo usa; os contratos
//!   completos substituem estes quando forem portados.
//! - `VariableResolver` (`(str) => Promise<string>`) é um `Rc<dyn Fn(&str) -> Promise<String>>`.
//! - `resolveConfigurationVariables` e `createTerminalEnvironment` alteram o `env` do `IShellLaunchConfig` no
//!   lugar, como o original, por isso recebem `&mut IShellLaunchConfig`.
//! - `Date.now()` é o `runtime::date_now()`, que respeita o relógio virtual dos testes.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::path::{PathModule, ProcessEnvironment as HostEnvironment};
use je_base::platform::{OperatingSystem, Platform};
use je_base::process::ProcessEnvironment;
use je_base::processes::sanitize_process_environment;
use je_base::promise::Promise;
use je_base::runtime;
use je_base::uri::{URI, uri_to_fs_path};
use je_platform::configuration::service::IConfigurationService;
use je_platform::log::{LogArg, LogMessage, LogService};
use je_platform::resources::{fs_path, host_platform};
use je_platform::terminal::terminal::{IShellLaunchConfig, ITerminalEnvironment, ShellCwd, ShellType, TerminalEnvironmentValue, TerminalShellType, WindowsShellType};
use je_platform::terminal::terminal_environment::{escape_non_windows_path, sanitize_cwd};
use je_platform::terminal::terminal_settings::{ShellIntegrationTimeoutOverride, TerminalSettingId};
use je_platform::workspace::{IWorkspaceContextService, WorkspaceFolder};

/// `'auto' | 'off' | 'on'`: o valor da configuração `terminal.integrated.detectLocale`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectLocale {
    Auto,
    Off,
    On,
}

/// `'unix-to-win' | 'win-to-unix'`: a direção da conversão do `getWslPath`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WslPathDirection {
    UnixToWin,
    WinToUnix,
}

/// `Pick<ITerminalBackend, 'getWslPath'>`.
pub trait ITerminalBackendWslPath {
    fn get_wsl_path(&self, original: &str, direction: WslPathDirection) -> Promise<String>;
}

/// `IHistoryService`, só com o `getLastActiveWorkspaceRoot` que o módulo usa.
pub trait IHistoryService {
    /// `getLastActiveWorkspaceRoot(schemeFilter?, authorityFilter?)`.
    fn get_last_active_workspace_root(&self, scheme_filter: Option<&str>, authority_filter: Option<&str>) -> Option<URI>;
}

/// `IConfigurationResolverService`, só com o `resolveWithEnvironment` que o módulo usa.
pub trait IConfigurationResolverService {
    /// `resolveWithEnvironment(environment, folder, value)`.
    fn resolve_with_environment(&self, environment: &ProcessEnvironment, folder: Option<&WorkspaceFolder>, value: &str) -> Promise<String>;
}

/// `VariableResolver`: `(str: string) => Promise<string>`.
pub type VariableResolver = Rc<dyn Fn(&str) -> Promise<String>>;

/// O `string | URI` do `resource` de `preparePathForShell`.
#[derive(Clone, Copy, Debug)]
pub enum ShellResource<'a> {
    Path(&'a str),
    Uri(&'a URI),
}

/// O texto não vazio de um `string | undefined` (a verdade de JavaScript).
fn non_empty(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.is_empty())
}

pub fn merge_environments(parent: &mut ProcessEnvironment, other: Option<&ITerminalEnvironment>) {
    let Some(other) = other else {
        return;
    };

    // On Windows apply the new values ignoring case, while still retaining
    // the case of the original key.
    if Platform::detect().is_windows {
        for (config_key, value) in other {
            let actual_key = parent.keys().find(|env_key| config_key.to_lowercase() == env_key.to_lowercase()).unwrap_or(config_key).clone();
            merge_environment_value(parent, &actual_key, value);
        }
    } else {
        for (key, value) in other {
            merge_environment_value(parent, key, value);
        }
    }
}

/// `_mergeEnvironmentValue`, depois do `value !== undefined` do chamador: o texto grava, o `null` apaga e o
/// `undefined` não faz nada.
fn merge_environment_value(env: &mut ProcessEnvironment, key: &str, value: &TerminalEnvironmentValue) {
    match value {
        TerminalEnvironmentValue::Value(value) => {
            env.insert(key.to_owned(), value.clone());
        }
        TerminalEnvironmentValue::Null => {
            env.remove(key);
        }
        TerminalEnvironmentValue::Undefined => {}
    }
}

pub fn add_terminal_environment_keys(env: &mut ProcessEnvironment, version: Option<&str>, locale: Option<&str>, detect_locale: DetectLocale) {
    env.insert("TERM_PROGRAM".to_owned(), "vscode".to_owned());
    if let Some(version) = non_empty(version) {
        env.insert("TERM_PROGRAM_VERSION".to_owned(), version.to_owned());
    }
    if should_set_lang_env_variable(env, detect_locale) {
        env.insert("LANG".to_owned(), get_lang_env_variable(locale));
    }
    env.insert("COLORTERM".to_owned(), "truecolor".to_owned());
}

fn merge_non_null_keys(env: &mut ProcessEnvironment, other: Option<&ITerminalEnvironment>) {
    let Some(other) = other else {
        return;
    };
    for (key, value) in other {
        if let TerminalEnvironmentValue::Value(value) = value {
            env.insert(key.clone(), value.clone());
        }
    }
}

async fn resolve_configuration_variables(variable_resolver: &VariableResolver, env: &mut ITerminalEnvironment) {
    // O resolvedor é chamado para todas as entradas antes de esperar pela primeira (o `Promise.all` do original).
    let pending: Vec<(usize, String, Promise<String>)> = env
        .iter()
        .enumerate()
        .filter_map(|(index, (_, value))| match value {
            TerminalEnvironmentValue::Value(value) => Some((index, value.clone(), variable_resolver(value))),
            TerminalEnvironmentValue::Null | TerminalEnvironmentValue::Undefined => None,
        })
        .collect();
    for (index, original, resolved) in pending {
        // `catch (e) { env[key] = value; }`
        env[index].1 = TerminalEnvironmentValue::Value(resolved.await.unwrap_or(original));
    }
}

pub fn should_set_lang_env_variable(env: &ProcessEnvironment, detect_locale: DetectLocale) -> bool {
    match detect_locale {
        DetectLocale::On => true,
        DetectLocale::Auto => match non_empty(env.get("LANG").map(String::as_str)) {
            None => true,
            Some(lang) => !lang.ends_with(".UTF-8") && !lang.ends_with(".utf8") && !has_euc_suffix(lang),
        },
        DetectLocale::Off => false,
    }
}

/// `lang.search(/\.euc.+/) !== -1`: um `.euc` seguido de pelo menos um caractere que não é fim de linha.
fn has_euc_suffix(lang: &str) -> bool {
    lang.match_indices(".euc").any(|(start, matched)| {
        let rest = &lang[start + matched.len()..];
        rest.chars().next().is_some_and(|c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
    })
}

/// A variante padrão de cada língua (`languageVariants`).
fn default_language_variant(language: &str) -> Option<&'static str> {
    Some(match language {
        "af" => "ZA",
        "am" => "ET",
        "be" => "BY",
        "bg" => "BG",
        "ca" => "ES",
        "cs" => "CZ",
        "da" => "DK",
        // de: 'AT',
        // de: 'CH',
        "de" => "DE",
        "el" => "GR",
        // en: 'AU',
        // en: 'CA',
        // en: 'GB',
        // en: 'IE',
        // en: 'NZ',
        "en" => "US",
        "es" => "ES",
        "et" => "EE",
        "eu" => "ES",
        "fi" => "FI",
        // fr: 'BE',
        // fr: 'CA',
        // fr: 'CH',
        "fr" => "FR",
        "he" => "IL",
        "hr" => "HR",
        "hu" => "HU",
        "hy" => "AM",
        "is" => "IS",
        // it: 'CH',
        "it" => "IT",
        "ja" => "JP",
        "kk" => "KZ",
        "ko" => "KR",
        "lt" => "LT",
        // nl: 'BE',
        "nl" => "NL",
        "no" => "NO",
        "pl" => "PL",
        "pt" => "BR",
        // pt: 'PT',
        "ro" => "RO",
        "ru" => "RU",
        "sk" => "SK",
        "sl" => "SI",
        "sr" => "YU",
        "sv" => "SE",
        "tr" => "TR",
        "uk" => "UA",
        "zh" => "CN",
        _ => return None,
    })
}

pub fn get_lang_env_variable(locale: Option<&str>) -> String {
    let mut parts: Vec<String> = match non_empty(locale) {
        Some(locale) => locale.split('-').map(str::to_owned).collect(),
        None => Vec::new(),
    };
    match parts.len() {
        0 => {
            // Fallback to en_US if the locale is unknown
            return "en_US.UTF-8".to_owned();
        }
        1 => {
            // The local may only contain the language, not the variant, if this is the case guess the
            // variant such that it can be used as a valid $LANG variable. The language variant chosen
            // is the original and/or most prominent with help from
            // https://stackoverflow.com/a/2502675/1156119
            // The list of locales was generated by running `locale -a` on macOS
            if let Some(variant) = default_language_variant(&parts[0]) {
                parts.push(variant.to_owned());
            }
        }
        _ => {
            // Ensure the variant is uppercase to be a valid $LANG
            parts[1] = parts[1].to_uppercase();
        }
    }
    format!("{}.UTF-8", parts.join("_"))
}

pub async fn get_cwd(
    shell: &IShellLaunchConfig,
    user_home: Option<&str>,
    variable_resolver: Option<&VariableResolver>,
    root: Option<&URI>,
    custom_cwd: Option<&str>,
    log_service: Option<&dyn LogService>,
) -> String {
    let shell_cwd = shell.cwd.as_ref().filter(|cwd| match cwd {
        ShellCwd::Path(path) => !path.is_empty(),
        ShellCwd::Uri(_) => true,
    });
    if let Some(shell_cwd) = shell_cwd {
        let unresolved = match shell_cwd {
            ShellCwd::Path(path) => path.clone(),
            ShellCwd::Uri(uri) => fs_path(uri),
        };
        let resolved = resolve_cwd(&unresolved, variable_resolver, None).await;
        return sanitize_cwd(non_empty(resolved.as_deref()).unwrap_or(&unresolved));
    }

    let mut cwd: Option<String> = None;

    if !shell.ignore_configuration_cwd.unwrap_or(false)
        && let Some(custom_cwd) = non_empty(custom_cwd)
    {
        let custom_cwd = match variable_resolver {
            Some(_) => resolve_cwd(custom_cwd, variable_resolver, log_service).await,
            None => Some(custom_cwd.to_owned()),
        };
        if let Some(custom_cwd) = non_empty(custom_cwd.as_deref()) {
            let path = PathModule::native(&HostEnvironment);
            if path.is_absolute(custom_cwd) {
                cwd = Some(custom_cwd.to_owned());
            } else if let Some(root) = root {
                cwd = Some(path.join(&[fs_path(root).as_str(), custom_cwd]));
            }
        }
    }

    // If there was no custom cwd or it was relative with no workspace
    let cwd = match cwd.filter(|cwd| !cwd.is_empty()) {
        Some(cwd) => cwd,
        None => match root {
            Some(root) => fs_path(root),
            None => non_empty(user_home).unwrap_or_default().to_owned(),
        },
    };

    sanitize_cwd(&cwd)
}

async fn resolve_cwd(cwd: &str, variable_resolver: Option<&VariableResolver>, log_service: Option<&dyn LogService>) -> Option<String> {
    let Some(variable_resolver) = variable_resolver else {
        return Some(cwd.to_owned());
    };
    match variable_resolver(cwd).await {
        Ok(resolved) => Some(resolved),
        Err(error) => {
            if let Some(log_service) = log_service {
                log_service.error_args(&LogMessage::from("Could not resolve terminal cwd"), &[LogArg::from(&error)]);
            }
            None
        }
    }
}

pub fn create_variable_resolver(
    last_active_workspace: Option<&WorkspaceFolder>,
    env: &ProcessEnvironment,
    configuration_resolver_service: Option<Rc<dyn IConfigurationResolverService>>,
) -> Option<VariableResolver> {
    let configuration_resolver_service = configuration_resolver_service?;
    let last_active_workspace = last_active_workspace.cloned();
    let env = env.clone();
    Some(Rc::new(move |value: &str| configuration_resolver_service.resolve_with_environment(&env, last_active_workspace.as_ref(), value)))
}

pub async fn create_terminal_environment(
    shell_launch_config: &mut IShellLaunchConfig,
    env_from_config: Option<&ITerminalEnvironment>,
    variable_resolver: Option<&VariableResolver>,
    version: Option<&str>,
    detect_locale: DetectLocale,
    base_env: &ProcessEnvironment,
) -> ProcessEnvironment {
    // Create a terminal environment based on settings, launch config and permissions
    let mut env = ProcessEnvironment::new();
    if shell_launch_config.strict_env.unwrap_or(false) {
        // strictEnv is true, only use the requested env (ignoring null entries)
        merge_non_null_keys(&mut env, shell_launch_config.env.as_ref());
    } else {
        // Merge process env with the env from config and from shellLaunchConfig
        let platform = Platform::detect();
        env.extend(base_env.clone());

        let mut allowed_env_from_config: ITerminalEnvironment = env_from_config.cloned().unwrap_or_default();

        // Resolve env vars from config and shell
        if let Some(variable_resolver) = variable_resolver {
            resolve_configuration_variables(variable_resolver, &mut allowed_env_from_config).await;
            if let Some(shell_env) = shell_launch_config.env.as_mut() {
                resolve_configuration_variables(variable_resolver, shell_env).await;
            }
        }

        // Workaround for https://github.com/microsoft/vscode/issues/204005
        // We should restore the following environment variables when a user
        // launches the application using the CLI so that integrated terminal
        // can still inherit these variables.
        // We are not bypassing the restrictions implied in https://github.com/electron/electron/pull/40770
        // since this only affects integrated terminal and not the application itself.
        if platform.is_macintosh {
            // Restore NODE_OPTIONS if it was set
            restore_environment_variable(&mut env, "VSCODE_NODE_OPTIONS", "NODE_OPTIONS");

            // Restore NODE_REPL_EXTERNAL_MODULE if it was set
            restore_environment_variable(&mut env, "VSCODE_NODE_REPL_EXTERNAL_MODULE", "NODE_REPL_EXTERNAL_MODULE");
        }

        // Sanitize the environment, removing any undesirable VS Code and Electron environment
        // variables
        sanitize_process_environment(&mut env, &["VSCODE_IPC_HOOK_CLI"]);

        // Merge config (settings) and ShellLaunchConfig environments
        merge_environments(&mut env, Some(&allowed_env_from_config));
        merge_environments(&mut env, shell_launch_config.env.as_ref());

        // Adding other env keys necessary to create the process
        add_terminal_environment_keys(&mut env, version, Some(&platform.language), detect_locale);
    }
    env
}

/// `if (env[from]) { env[to] = env[from]; delete env[from]; }`
fn restore_environment_variable(env: &mut ProcessEnvironment, from: &str, to: &str) {
    if env.get(from).is_some_and(|value| !value.is_empty())
        && let Some(value) = env.remove(from)
    {
        env.insert(to.to_owned(), value);
    }
}

/// `/[\x00-\x1F\x7F-\x9F]/`: os caracteres de controle.
fn is_control_character(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{1F}' | '\u{7F}'..='\u{9F}')
}

/// Takes a path and returns the properly escaped path to send to a given shell. On Windows, this
/// included trying to prepare the path for WSL if needed.
///
/// - `resource`: The path to be escaped and formatted.
/// - `executable`: The executable off the shellLaunchConfig.
/// - `title`: The terminal's title.
/// - `shell_type`: The type of shell the path is being sent to.
/// - `backend`: The backend for the terminal.
/// - `is_windows_frontend`: Whether the frontend is Windows, this is only exposed for injection via
///   tests (`None` é o `isWindows` do processo).
///
/// Devolve a versão escapada do caminho, para ser executada no terminal.
pub async fn prepare_path_for_shell(
    resource: ShellResource<'_>,
    executable: Option<&str>,
    title: &str,
    shell_type: TerminalShellType,
    backend: Option<&dyn ITerminalBackendWslPath>,
    os: Option<OperatingSystem>,
    is_windows_frontend: Option<bool>,
) -> Result<String, Error> {
    let is_windows_frontend = is_windows_frontend.unwrap_or_else(|| Platform::detect().is_windows);
    let mut original_path = match resource {
        ShellResource::Path(path) => path.to_owned(),
        ShellResource::Uri(uri) => {
            let mut original_path = fs_path(uri);
            // Apply backend OS-specific formatting to the path since URI.fsPath uses the frontend's OS
            if is_windows_frontend && os != Some(OperatingSystem::Windows) {
                original_path = original_path.replace('\\', "/");
            } else if !is_windows_frontend && os == Some(OperatingSystem::Windows) {
                original_path = original_path.replace('/', "\\");
            }
            original_path
        }
    };

    // Control characters such as a carriage return would submit the line early
    original_path.retain(|c| !is_control_character(c));

    let Some(executable) = non_empty(executable) else {
        return Ok(original_path);
    };

    let has_space = original_path.contains(' ');
    let has_parens = original_path.contains('(') || original_path.contains(')');

    let path_basename = PathModule::native(&HostEnvironment).basename(executable, Some(".exe"));
    let is_power_shell = path_basename == "pwsh" || title == "pwsh" || path_basename == "powershell" || title == "powershell";

    if is_power_shell && (has_space || original_path.contains('\'')) {
        return Ok(format!("& '{}'", original_path.replace('\'', "''")));
    }

    if has_parens && is_power_shell {
        return Ok(format!("& '{original_path}'"));
    }

    if os == Some(OperatingSystem::Windows) {
        // 17063 is the build number where wsl path was introduced.
        // Update Windows uriPath to be executed in WSL.
        if shell_type.is_some() {
            return match shell_type {
                Some(ShellType::Windows(WindowsShellType::GitBash)) => Ok(escape_non_windows_path(&original_path.replace('\\', "/"), shell_type)),
                Some(ShellType::Windows(WindowsShellType::Wsl)) => win_to_unix_path(backend, original_path).await,
                _ if has_space => Ok(format!("\"{original_path}\"")),
                _ => Ok(original_path),
            };
        }
        let lower_executable = executable.to_lowercase();
        if lower_executable.contains("wsl") || (lower_executable.contains("bash.exe") && !lower_executable.contains("git")) {
            return win_to_unix_path(backend, original_path).await;
        } else if has_space {
            return Ok(format!("\"{original_path}\""));
        }
        return Ok(original_path);
    }

    Ok(escape_non_windows_path(&original_path, shell_type))
}

/// `backend?.getWslPath(originalPath, 'win-to-unix') || originalPath`.
async fn win_to_unix_path(backend: Option<&dyn ITerminalBackendWslPath>, original_path: String) -> Result<String, Error> {
    let Some(backend) = backend else {
        return Ok(original_path);
    };
    let converted = backend.get_wsl_path(&original_path, WslPathDirection::WinToUnix).await?;
    Ok(if converted.is_empty() { original_path } else { converted })
}

pub fn get_workspace_for_terminal(
    cwd: Option<ShellResource<'_>>,
    workspace_context_service: &dyn IWorkspaceContextService,
    history_service: &dyn IHistoryService,
) -> Option<WorkspaceFolder> {
    let cwd_uri = match cwd {
        Some(ShellResource::Path(path)) => Some(URI::parse(path, false)),
        Some(ShellResource::Uri(uri)) => Some(uri.clone()),
        None => None,
    };
    let workspace_folder = cwd_uri.and_then(|cwd_uri| workspace_context_service.get_workspace_folder(&cwd_uri));
    // fallback to last active workspace if cwd is not available or it is not in workspace
    // TOOD: last active workspace is known to be unreliable, we should remove this fallback eventually
    workspace_folder.or_else(|| {
        let active_workspace_root_uri = history_service.get_last_active_workspace_root(None, None);
        active_workspace_root_uri.and_then(|uri| workspace_context_service.get_workspace_folder(&uri))
    })
}

pub async fn get_uri_label_for_shell(
    uri: ShellResource<'_>,
    backend: &dyn ITerminalBackendWslPath,
    shell_type: TerminalShellType,
    os: Option<OperatingSystem>,
    is_windows_frontend: Option<bool>,
) -> Result<String, Error> {
    let is_windows_frontend = is_windows_frontend.unwrap_or_else(|| Platform::detect().is_windows);
    let mut path = match uri {
        ShellResource::Path(path) => path.to_owned(),
        ShellResource::Uri(uri) => fs_path(uri),
    };
    if os == Some(OperatingSystem::Windows) {
        match shell_type {
            Some(ShellType::Windows(WindowsShellType::Wsl)) => backend.get_wsl_path(&path.replace('/', "\\"), WslPathDirection::WinToUnix).await,
            Some(ShellType::Windows(WindowsShellType::GitBash)) => {
                // Convert \ to / and replace 'c:\' with '/c/'.
                Ok(replace_drive_prefix(&path.replace('\\', "/")))
            }
            _ => {
                // If the frontend is not Windows but the terminal is, convert / to \.
                if let ShellResource::Uri(uri) = uri {
                    path = uri_to_fs_path(uri, true, host_platform()).to_string_lossy();
                }
                Ok(if is_windows_frontend { path } else { path.replace('/', "\\") })
            }
        }
    } else {
        // If the frontend is Windows but the terminal is not, convert \ to /.
        Ok(if is_windows_frontend { path.replace('\\', "/") } else { path })
    }
}

/// `path.replace(/^([a-zA-Z]):\//, '/$1/')`.
fn replace_drive_prefix(path: &str) -> String {
    let mut chars = path.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(drive), Some(':'), Some('/')) if drive.is_ascii_alphabetic() => format!("/{drive}/{}", chars.as_str()),
        _ => path.to_owned(),
    }
}

/// Gets the unified duration to wait for shell integration after the terminal launches before
/// declaring the terminal lacks shell integration.
pub fn get_shell_integration_timeout(
    configuration_service: &dyn IConfigurationService,
    si_injection_enabled: bool,
    is_remote: bool,
    process_ready_timestamp: Option<f64>,
) -> f64 {
    let timeout_value = configuration_service.get_value(Some(TerminalSettingId::ShellIntegrationTimeout.as_str()), None);
    let timeout_number = match timeout_value {
        Dynamic::Number(number) if !number.is_nan() => Some(number),
        _ => None,
    };
    let mut timeout_ms: f64 = match timeout_number {
        // Used for tests
        Some(number) if number == ShellIntegrationTimeoutOverride::DisableForTests.value() => 0.0,
        None => default_shell_integration_timeout(si_injection_enabled, is_remote),
        Some(number) if number < 0.0 => default_shell_integration_timeout(si_injection_enabled, is_remote),
        Some(number) if number == 0.0 => 0.0,
        Some(number) => number.max(500.0),
    };

    // Adjust timeout based on how long the process has already been running
    if let Some(process_ready_timestamp) = process_ready_timestamp {
        let elapsed = runtime::date_now() as f64 - process_ready_timestamp;
        timeout_ms = (timeout_ms - elapsed).max(0.0);
    }

    timeout_ms
}

/// `siInjectionEnabled ? 5000 : (isRemote ? 3000 : 2000)`.
fn default_shell_integration_timeout(si_injection_enabled: bool, is_remote: bool) -> f64 {
    if si_injection_enabled {
        5000.0
    } else if is_remote {
        3000.0
    } else {
        2000.0
    }
}
