//! Porte de `src/vs/platform/terminal/node/terminalEnvironment.ts`: os argumentos e as variáveis de
//! ambiente que ligam a integração de shell (`getShellIntegrationInjection`), e a limpeza de um ambiente
//! antes de ele ir para o log (`sanitizeEnvForLogging`). O `terminal_environment` irmão é o de `common/`.
//!
//! Divergências:
//! - O original lê `isWindows`, `isMacintosh` e `process.platform` ao carregar o módulo, o `FileAccess`
//!   global e o `os` do Node. Aqui `isWindows`, `isMacintosh` e `process.platform` são o
//!   `Platform::detect()` do processo atual, o `FileAccess` entra por parâmetro (o porte não tem
//!   singleton), `os.homedir()` e `os.tmpdir()` são as funções de `environment::user_data_path`, e
//!   `os.userInfo().username` é lido de `USER`, `LOGNAME` ou `USERNAME` (cai em `unknown`, como o `catch`).
//! - A promessa que rejeita (`realpath` do diretório temporário) é um `Err`. O original só fica `async` por
//!   causa do `fs/promises` e do número de build do Windows; o porte mantém o `async fn`, e o `fs` é o
//!   síncrono do Rust.
//! - O `Map` de argumentos padrão (`shellIntegrationArgs`) é a função [`shell_integration_args`], que
//!   devolve um vetor novo a cada chamada. Por isso a cópia (`[...newArgs]`) que o original faz para não
//!   alterar o vetor padrão não existe, e o ramo `originalArgs === shellIntegrationArgs.get(...)` (igualdade
//!   de referência com o vetor privado do módulo, que nenhum chamador consegue ter) nunca é verdadeiro e
//!   não é portado.
//! - `IProcessEnvironment` em `sanitizeEnvForLogging` é um mapa de `Option<String>` (o valor `undefined`
//!   existe nesse teste); o resultado nunca tem `undefined`, então volta como [`ProcessEnvironment`].
//! - As expressões regulares fixas viraram reconhecedores escritos à mão, com as mesmas classes de
//!   caracteres (o `i` de `sensitiveEnvVarNames` só afeta ASCII, porque o `RegExp` sem `u` não dobra
//!   caracteres não ASCII para ASCII).

use std::collections::BTreeMap;

use je_base::JsString;
use je_base::arrays::OneOrMany;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::network::FileAccess;
use je_base::node::windows_version::get_windows_build_number_async;
use je_base::path::{PathModule, ProcessEnvironment as HostEnvironment};
use je_base::platform::Platform;
use je_base::process::ProcessEnvironment;
use je_base::strings;

use super::environment_variable::{EnvironmentVariableMutatorType, EnvironmentVariableScope, IMergedEnvironmentVariableCollection};
use super::environment_variable_collection::MergedEnvironmentVariableCollection;
use super::environment_variable_shared::deserialize_environment_variable_collections;
use super::terminal::{
    IShellLaunchConfig, ITerminalEnvironment, ITerminalProcessOptions, ShellIntegrationInjectionFailureReason, TerminalEnvironmentValue,
};
use crate::environment::user_data_path::{home_dir, tmp_dir};
use crate::log::{LogArg, LogMessage, LogService};
use crate::product::IProductService;

/// Um arquivo a copiar de `source` para `dest` (o item de `filesToCopy`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IShellIntegrationFileToCopy {
    pub source: String,
    pub dest: String,
}

/// `IShellIntegrationConfigInjection` (o `type: 'injection'` é a variante de [`ShellIntegrationInjection`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IShellIntegrationConfigInjection {
    /// A new set of arguments to use.
    pub new_args: Option<Vec<String>>,
    /// An optional environment to mixing to the real environment.
    pub env_mixin: Option<ProcessEnvironment>,
    /// An optional array of files to copy from `source` to `dest`.
    pub files_to_copy: Option<Vec<IShellIntegrationFileToCopy>>,
}

/// `IShellIntegrationInjectionFailure` (o `type: 'failure'` é a variante de [`ShellIntegrationInjection`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IShellIntegrationInjectionFailure {
    pub reason: ShellIntegrationInjectionFailureReason,
}

/// `IShellIntegrationConfigInjection | IShellIntegrationInjectionFailure`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellIntegrationInjection {
    Injection(IShellIntegrationConfigInjection),
    Failure(IShellIntegrationInjectionFailure),
}

impl ShellIntegrationInjection {
    /// O `type` do original: `injection` ou `failure`.
    pub fn type_(&self) -> &'static str {
        match self {
            Self::Injection(_) => "injection",
            Self::Failure(_) => "failure",
        }
    }

    fn failure(reason: ShellIntegrationInjectionFailureReason) -> Self {
        Self::Failure(IShellIntegrationInjectionFailure { reason })
    }
}

/// `os.userInfo().username`, ou `unknown` quando o sistema não diz.
pub fn user_name() -> String {
    ["USER", "LOGNAME", "USERNAME"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// `FileAccess.asFileUri('vs/workbench/contrib/terminal/common/scripts').fsPath`: a pasta dos scripts de
/// integração de shell.
pub fn shell_integration_script_root(file_access: &FileAccess) -> String {
    file_access.as_file_uri("vs/workbench/contrib/terminal/common/scripts").fs_path(Platform::detect().os.path_flavor()).to_string_lossy()
}

/// `!originalArgs || originalArgs.length === 0`: a string vazia é falsa, o vetor vazio tem tamanho zero.
fn args_empty(original_args: &Option<OneOrMany<String>>) -> bool {
    match original_args {
        None => true,
        Some(OneOrMany::One(args)) => args.is_empty(),
        Some(OneOrMany::Many(args)) => args.is_empty(),
    }
}

/// `shellLaunchConfig.args` como o argumento do log (`unknown`).
fn args_to_log_arg(args: &Option<OneOrMany<String>>) -> LogArg {
    LogArg::Value(match args {
        None => Dynamic::Undefined,
        Some(OneOrMany::One(args)) => Dynamic::string(args),
        Some(OneOrMany::Many(args)) => Dynamic::array(args.iter().map(|arg| Dynamic::string(arg))),
    })
}

/// `logService.warn(\`Shell integration cannot be enabled for executable "..." and args\`, args)`.
fn warn_unsupported_shell(log_service: &dyn LogService, shell_launch_config: &IShellLaunchConfig) {
    let message = format!(
        "Shell integration cannot be enabled for executable \"{}\" and args",
        shell_launch_config.executable.as_deref().unwrap_or_default()
    );
    log_service.warn_args(&LogMessage::from(message), &[args_to_log_arg(&shell_launch_config.args)]);
}

/// `chmod(path, 0o1700)`:
/// - Sticky bit is set, preventing non-owners from deleting or renaming files within this directory (1)
/// - Owner has full read (4), write (2), execute (1) permissions
/// - Group has no permissions (0)
/// - Others have no permissions (0)
#[cfg(unix)]
fn set_sticky_bit(path: &str) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o1700))
}

/// O `chmod` do Node fora do Unix só liga ou desliga o somente leitura (o dono tem escrita em `0o1700`).
#[cfg(not(unix))]
fn set_sticky_bit(path: &str) -> std::io::Result<()> {
    let mut permissions = std::fs::metadata(path)?.permissions();
    permissions.set_readonly(false);
    std::fs::set_permissions(path, permissions)
}

/// O que `getShellIntegrationInjection` monta de um shell reconhecido: o contexto que cada ramo lê e o
/// ambiente que ele acumula.
struct Injector<'a> {
    options: &'a ITerminalProcessOptions,
    product_service: &'a dyn IProductService,
    original_args: &'a Option<OneOrMany<String>>,
    shell: &'a str,
    script_root: &'a str,
    env_mixin: ProcessEnvironment,
}

impl Injector<'_> {
    /// Os argumentos padrão do shell com a pasta dos scripts (e `extra`) no lugar de `{0}` (e `{1}`).
    fn script_args(&self, executable: ShellIntegrationExecutable, extra: &[&str]) -> Vec<String> {
        let mut new_args = shell_integration_args(executable);
        let values: Vec<JsString> = std::iter::once(self.script_root).chain(extra.iter().copied()).map(JsString::from).collect();
        if let Some(last) = new_args.last_mut() {
            *last = strings::format(JsString::from(last.as_str()).units(), &values).to_string_lossy();
        }
        new_args
    }

    fn stable_flag(&self) -> &'static str {
        if self.product_service.quality() == Some("stable") { "1" } else { "0" }
    }

    fn injection(mut self, new_args: Vec<String>, with_stable_flag: bool) -> ShellIntegrationInjection {
        if with_stable_flag {
            let stable = self.stable_flag();
            self.env_mixin.insert("VSCODE_STABLE".to_owned(), stable.to_owned());
        }
        ShellIntegrationInjection::Injection(IShellIntegrationConfigInjection { new_args: Some(new_args), env_mixin: Some(self.env_mixin), files_to_copy: None })
    }

    fn is_login(&self) -> bool {
        self.original_args.as_ref().is_some_and(are_zsh_bash_fish_login_args)
    }

    /// There are a few situations where some directories are added to the beginning of the PATH.
    /// 1. On macOS when the profile calls path_helper.
    /// 2. For fish terminals, which always prepend "$fish_user_paths" to the PATH.
    ///
    /// This causes significant problems for the environment variable
    /// collection API as the custom paths added to the end will now be somewhere in the middle of
    /// the PATH. To combat this, VSCODE_PATH_PREFIX is used to re-apply any prefix after the profile
    /// has run. This will cause duplication in the PATH but should fix the issue.
    ///
    /// See #99878 for more information.
    fn add_env_mixin_path_prefix(&mut self) {
        let Some(collections) = &self.options.environment_variable_collections else {
            return;
        };
        if !(Platform::detect().is_macintosh || self.shell == "fish") {
            return;
        }
        // Deserialize and merge
        let deserialized = deserialize_environment_variable_collections(collections.clone());
        let merged = MergedEnvironmentVariableCollection::new(deserialized);

        // Get all prepend PATH entries
        let scope = EnvironmentVariableScope { workspace_folder: self.options.workspace_folder.clone() };
        let variable_map = merged.get_variable_map(Some(&scope));
        let prepend_to_path: String = variable_map
            .get(&"PATH".to_owned())
            .into_iter()
            .flatten()
            .filter(|mutator| mutator.mutator_type == EnvironmentVariableMutatorType::Prepend)
            .map(|mutator| mutator.value.as_str())
            .collect();

        // Add to the environment mixin to be applied in the shell integration script
        if !prepend_to_path.is_empty() {
            self.env_mixin.insert("VSCODE_PATH_PREFIX".to_owned(), prepend_to_path);
        }
    }

    fn unsupported_args() -> ShellIntegrationInjection {
        ShellIntegrationInjection::failure(ShellIntegrationInjectionFailureReason::UnsupportedArgs)
    }

    fn pwsh(self, implied: ShellIntegrationExecutable, login: ShellIntegrationExecutable) -> ShellIntegrationInjection {
        let executable = match self.original_args {
            args if args_empty(args) => implied,
            Some(args) if are_pwsh_implied_args(args) => implied,
            Some(args) if are_pwsh_login_args(args) => login,
            _ => return Self::unsupported_args(),
        };
        let new_args = self.script_args(executable, &[""]);
        self.injection(new_args, true)
    }

    fn bash(mut self) -> ShellIntegrationInjection {
        let executable = if args_empty(self.original_args) {
            ShellIntegrationExecutable::Bash
        } else if self.is_login() {
            self.env_mixin.insert("VSCODE_SHELL_LOGIN".to_owned(), "1".to_owned());
            self.add_env_mixin_path_prefix();
            ShellIntegrationExecutable::Bash
        } else {
            return Self::unsupported_args();
        };
        let new_args = self.script_args(executable, &[]);
        self.injection(new_args, true)
    }

    fn fish(mut self) -> ShellIntegrationInjection {
        let executable = if args_empty(self.original_args) {
            ShellIntegrationExecutable::Fish
        } else if self.is_login() {
            ShellIntegrationExecutable::FishLogin
        } else {
            return Self::unsupported_args();
        };

        // On fish, '$fish_user_paths' is always prepended to the PATH, for both login and non-login shells, so we need
        // to apply the path prefix fix always, not only for login shells (see #232291)
        self.add_env_mixin_path_prefix();

        let new_args = self.script_args(executable, &[]);
        self.injection(new_args, false)
    }

    fn zsh(mut self, env: Option<&ITerminalEnvironment>, log_service: &dyn LogService, skip_sticky_bit: bool) -> Result<ShellIntegrationInjection, Error> {
        let executable = if args_empty(self.original_args) {
            ShellIntegrationExecutable::Zsh
        } else if self.is_login() {
            self.add_env_mixin_path_prefix();
            ShellIntegrationExecutable::ZshLogin
        } else {
            return Ok(Self::unsupported_args());
        };
        let new_args = self.script_args(executable, &[]);

        // Move .zshrc into $ZDOTDIR as the way to activate the script
        let username = user_name();

        // Resolve the actual tmp directory so we can set the sticky bit
        let real_tmp_dir = std::fs::canonicalize(tmp_dir()).map_err(|err| Error::new(err.to_string()))?.to_string_lossy().into_owned();
        let path = PathModule::native(&HostEnvironment);
        let zdotdir = path.join(&[real_tmp_dir.as_str(), format!("{username}-{}-zsh", self.product_service.application_name()).as_str()]);

        if !skip_sticky_bit {
            // skip for tests
            if let Err(err) = set_sticky_bit(&zdotdir) {
                if err.kind() != std::io::ErrorKind::NotFound {
                    log_service.error(&format!("Failed to set sticky bit on {zdotdir}: {err}"));
                    return Ok(ShellIntegrationInjection::failure(ShellIntegrationInjectionFailureReason::FailedToSetStickyBit));
                }
                if let Err(err) = std::fs::create_dir_all(&zdotdir) {
                    log_service.error(&format!("Failed to create zdotdir at {zdotdir}: {err}"));
                    return Ok(ShellIntegrationInjection::failure(ShellIntegrationInjectionFailureReason::FailedToCreateTmpDir));
                }
                if let Err(err) = set_sticky_bit(&zdotdir) {
                    log_service.error(&format!("Failed to set sticky bit on {zdotdir}: {err}"));
                    return Ok(ShellIntegrationInjection::failure(ShellIntegrationInjectionFailureReason::FailedToSetStickyBit));
                }
            }
        }
        let user_zdotdir = env
            .and_then(|env| env.iter().rev().find(|(key, _)| key == "ZDOTDIR"))
            .and_then(|(_, value)| match value {
                TerminalEnvironmentValue::Value(value) => Some(value.clone()),
                TerminalEnvironmentValue::Null | TerminalEnvironmentValue::Undefined => None,
            })
            .unwrap_or_else(home_dir);
        self.env_mixin.insert("ZDOTDIR".to_owned(), zdotdir.clone());
        self.env_mixin.insert("USER_ZDOTDIR".to_owned(), user_zdotdir);
        let files_to_copy = [
            ("shellIntegration-rc.zsh", ".zshrc"),
            ("shellIntegration-profile.zsh", ".zprofile"),
            ("shellIntegration-env.zsh", ".zshenv"),
            ("shellIntegration-login.zsh", ".zlogin"),
        ]
        .iter()
        .map(|&(source, dest)| IShellIntegrationFileToCopy { source: path.join(&[self.script_root, source]), dest: path.join(&[zdotdir.as_str(), dest]) })
        .collect();
        Ok(ShellIntegrationInjection::Injection(IShellIntegrationConfigInjection {
            new_args: Some(new_args),
            env_mixin: Some(self.env_mixin),
            files_to_copy: Some(files_to_copy),
        }))
    }
}

/// For a given shell launch config, returns arguments to replace and an optional environment to
/// mixin to the SLC's environment to enable shell integration. This must be run within the context
/// that creates the process to ensure accuracy. Returns a failure if shell integration cannot be
/// enabled.
pub async fn get_shell_integration_injection(
    shell_launch_config: &IShellLaunchConfig,
    options: &ITerminalProcessOptions,
    env: Option<&ITerminalEnvironment>,
    log_service: &dyn LogService,
    product_service: &dyn IProductService,
    file_access: &FileAccess,
    skip_sticky_bit: bool,
) -> Result<ShellIntegrationInjection, Error> {
    use ShellIntegrationInjectionFailureReason as Reason;

    // The global setting is disabled
    if !options.shell_integration.enabled {
        return Ok(ShellIntegrationInjection::failure(Reason::InjectionSettingDisabled));
    }
    // There is no executable (so there's no way to determine how to inject)
    let Some(executable) = shell_launch_config.executable.as_deref().filter(|executable| !executable.is_empty()) else {
        return Ok(ShellIntegrationInjection::failure(Reason::NoExecutable));
    };
    // It's a feature terminal (tasks, debug), unless it's explicitly being forced
    if shell_launch_config.is_feature_terminal.unwrap_or(false) && !shell_launch_config.force_shell_integration.unwrap_or(false) {
        return Ok(ShellIntegrationInjection::failure(Reason::FeatureTerminal));
    }
    // The ignoreShellIntegration flag is passed (eg. relaunching without shell integration)
    if shell_launch_config.ignore_shell_integration.unwrap_or(false) {
        return Ok(ShellIntegrationInjection::failure(Reason::IgnoreShellIntegrationFlag));
    }
    // Shell integration requires Windows 10 build 18309+ (ConPTY support)
    let is_windows = Platform::detect().is_windows;
    let windows_build_number = if is_windows { get_windows_build_number_async().await } else { 0 };
    if is_windows && windows_build_number < 18309 {
        return Ok(ShellIntegrationInjection::failure(Reason::UnsupportedWindowsBuild));
    }

    let original_args = &shell_launch_config.args;
    let basename = PathModule::native(&HostEnvironment).basename(executable, None);
    let shell = if is_windows { basename.to_lowercase() } else { basename };
    let script_root = shell_integration_script_root(file_access);
    let mut env_mixin = ProcessEnvironment::new();
    env_mixin.insert("VSCODE_INJECTION".to_owned(), "1".to_owned());

    if !options.shell_integration.nonce.is_empty() {
        env_mixin.insert("VSCODE_NONCE".to_owned(), options.shell_integration.nonce.clone());
    }
    // Temporarily pass list of hardcoded env vars for shell env api
    let scoped_down_shell_envs = ["PATH", "VIRTUAL_ENV", "HOME", "SHELL", "PWD"];
    if shell_launch_config.shell_integration_environment_reporting.unwrap_or(false) {
        let enable_env_reporting = !is_windows || options.windows_use_conpty_dll || (windows_build_number >= 22631 && shell != "bash.exe");
        if enable_env_reporting {
            env_mixin.insert("VSCODE_SHELL_ENV_REPORTING".to_owned(), scoped_down_shell_envs.join(","));
        }
    }

    // Windows
    if is_windows && matches!(shell.as_str(), "pwsh.exe" | "powershell.exe") {
        env_mixin.insert("VSCODE_A11Y_MODE".to_owned(), if options.is_screen_reader_optimized { "1" } else { "0" }.to_owned());
    }
    let injector = Injector { options, product_service, original_args, shell: &shell, script_root: &script_root, env_mixin };
    let injection = match (is_windows, shell.as_str()) {
        (true, "pwsh.exe" | "powershell.exe") => Some(injector.pwsh(ShellIntegrationExecutable::WindowsPwsh, ShellIntegrationExecutable::WindowsPwshLogin)),
        (true, "bash.exe") | (false, "bash") => Some(injector.bash()),
        // Linux & macOS
        (false, "fish") => Some(injector.fish()),
        (false, "pwsh") => Some(injector.pwsh(ShellIntegrationExecutable::Pwsh, ShellIntegrationExecutable::PwshLogin)),
        (false, "zsh") => Some(injector.zsh(env, log_service, skip_sticky_bit)?),
        _ => None,
    };
    Ok(injection.unwrap_or_else(|| {
        warn_unsupported_shell(log_service, shell_launch_config);
        ShellIntegrationInjection::failure(Reason::UnsupportedShell)
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShellIntegrationExecutable {
    WindowsPwsh,
    WindowsPwshLogin,
    Pwsh,
    PwshLogin,
    Zsh,
    ZshLogin,
    Bash,
    Fish,
    FishLogin,
}

/// `shellIntegrationArgs.get(executable)`.
fn shell_integration_args(executable: ShellIntegrationExecutable) -> Vec<String> {
    use ShellIntegrationExecutable::*;
    let args: &[&str] = match executable {
        // The try catch swallows execution policy errors in the case of the archive distributable
        WindowsPwsh => &["-noexit", "-command", "try { . \"{0}\\shellIntegration.ps1\" } catch {}{1}"],
        WindowsPwshLogin => &["-l", "-noexit", "-command", "try { . \"{0}\\shellIntegration.ps1\" } catch {}{1}"],
        Pwsh => &["-noexit", "-command", ". \"{0}/shellIntegration.ps1\"{1}"],
        PwshLogin => &["-l", "-noexit", "-command", ". \"{0}/shellIntegration.ps1\""],
        Zsh => &["-i"],
        ZshLogin => &["-il"],
        Bash => &["--init-file", "{0}/shellIntegration-bash.sh"],
        Fish => &["--init-command", "source \"{0}/shellIntegration.fish\""],
        FishLogin => &["-l", "--init-command", "source \"{0}/shellIntegration.fish\""],
    };
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

const PWSH_LOGIN_ARGS: [&str; 2] = ["-login", "-l"];
const SH_LOGIN_ARGS: [&str; 2] = ["--login", "-l"];
const SH_INTERACTIVE_ARGS: [&str; 2] = ["-i", "--interactive"];
const PWSH_IMPLIED_ARGS: [&str; 2] = ["-nol", "-nologo"];

/// `list.includes(arg.toLowerCase())`.
fn includes_lowercase(list: &[&str], arg: &str) -> bool {
    list.contains(&arg.to_lowercase().as_str())
}

fn are_pwsh_login_args(original_args: &OneOrMany<String>) -> bool {
    match original_args {
        OneOrMany::One(arg) => includes_lowercase(&PWSH_LOGIN_ARGS, arg),
        OneOrMany::Many(args) => match args.as_slice() {
            [arg] => includes_lowercase(&PWSH_LOGIN_ARGS, arg),
            [first, second] => {
                (includes_lowercase(&PWSH_LOGIN_ARGS, first) || includes_lowercase(&PWSH_LOGIN_ARGS, second))
                    && (includes_lowercase(&PWSH_IMPLIED_ARGS, first) || includes_lowercase(&PWSH_IMPLIED_ARGS, second))
            }
            _ => false,
        },
    }
}

fn are_pwsh_implied_args(original_args: &OneOrMany<String>) -> bool {
    match original_args {
        OneOrMany::One(arg) => includes_lowercase(&PWSH_IMPLIED_ARGS, arg),
        OneOrMany::Many(args) => match args.as_slice() {
            [] => true,
            [arg] => includes_lowercase(&PWSH_IMPLIED_ARGS, arg),
            _ => false,
        },
    }
}

fn are_zsh_bash_fish_login_args(original_args: &OneOrMany<String>) -> bool {
    match original_args {
        OneOrMany::One(arg) => includes_lowercase(&SH_LOGIN_ARGS, arg),
        OneOrMany::Many(args) => {
            let args: Vec<&String> = args.iter().filter(|arg| !includes_lowercase(&SH_INTERACTIVE_ARGS, arg)).collect();
            matches!(args.as_slice(), [arg] if includes_lowercase(&SH_LOGIN_ARGS, arg))
        }
    }
}

/// `/^(?:.*_)?(?:API_?KEY|TOKEN|...)(?:_.*)?$/i`: os nomes de variável de ambiente que indicam um segredo.
const SENSITIVE_ENV_VAR_WORDS: [&str; 15] = [
    "API_KEY",
    "APIKEY",
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "PASSWD",
    "PWD",
    "CREDENTIAL",
    "AUTH",
    "PRIVATE_KEY",
    "PRIVATEKEY",
    "ACCESS_KEY",
    "ACCESSKEY",
    "CLIENT_SECRET",
    "CLIENTSECRET",
];

/// O `.` do `RegExp` não casa com terminadores de linha.
fn is_line_terminator(c: &char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// Patterns that indicate sensitive environment variable names (`sensitiveEnvVarNames.test(key)`).
fn is_sensitive_env_var_name(key: &str) -> bool {
    let key: Vec<char> = key.chars().map(|c| c.to_ascii_uppercase()).collect();
    (0..=key.len()).any(|start| {
        // `(?:.*_)?`
        let prefix_matches = start == 0 || (key[start - 1] == '_' && !key[..start].iter().any(is_line_terminator));
        prefix_matches
            && SENSITIVE_ENV_VAR_WORDS.iter().any(|word| {
                let word: Vec<char> = word.chars().collect();
                let Some(rest) = key[start..].strip_prefix(word.as_slice()) else {
                    return false;
                };
                // `(?:_.*)?$`
                rest.is_empty() || (rest[0] == '_' && !rest[1..].iter().any(is_line_terminator))
            })
    })
}

fn is_alphanumeric(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

/// `[a-zA-Z0-9]{count}` e nada depois.
fn is_alphanumeric_of_length(text: &str, count: usize) -> bool {
    text.len() == count && text.chars().all(is_alphanumeric)
}

/// JWT tokens: `/^eyJ[a-zA-Z0-9\-_]+\.[a-zA-Z0-9\-_]+\.[a-zA-Z0-9\-_]+$/`.
fn is_jwt(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("eyJ") else {
        return false;
    };
    let parts: Vec<&str> = rest.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.chars().all(|c| is_alphanumeric(c) || matches!(c, '-' | '_')))
}

/// GitHub tokens: `/^gh[psuro]_[a-zA-Z0-9]{36}$/`.
fn is_github_token(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("gh") else {
        return false;
    };
    let mut chars = rest.chars();
    matches!(chars.next(), Some('p' | 's' | 'u' | 'r' | 'o')) && chars.next() == Some('_') && is_alphanumeric_of_length(chars.as_str(), 36)
}

/// GitHub tokens: `/^github_pat_[a-zA-Z0-9]{22}_[a-zA-Z0-9]{59}$/`.
fn is_github_pat(value: &str) -> bool {
    value
        .strip_prefix("github_pat_")
        .and_then(|rest| rest.split_once('_'))
        .is_some_and(|(first, second)| is_alphanumeric_of_length(first, 22) && is_alphanumeric_of_length(second, 59))
}

/// Google API keys: `/^AIza[A-Za-z0-9_\-]{35}$/`.
fn is_google_api_key(value: &str) -> bool {
    value.strip_prefix("AIza").is_some_and(|rest| rest.len() == 35 && rest.chars().all(|c| is_alphanumeric(c) || matches!(c, '-' | '_')))
}

/// Slack tokens: `/^xox[pbar]\-[A-Za-z0-9\-]+$/`.
fn is_slack_token(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("xox") else {
        return false;
    };
    let mut chars = rest.chars();
    matches!(chars.next(), Some('p' | 'b' | 'a' | 'r'))
        && chars.next() == Some('-')
        && !chars.as_str().is_empty()
        && chars.all(|c| is_alphanumeric(c) || c == '-')
}

/// Azure/MS tokens (common patterns): `/^[a-zA-Z0-9]{32,}$/`.
fn is_long_alphanumeric(value: &str) -> bool {
    value.len() >= 32 && value.chars().all(is_alphanumeric)
}

/// `secretValuePatterns.some(pattern => pattern.test(value))`: patterns for detecting secret values in
/// environment variables.
fn is_secret_value(value: &str) -> bool {
    is_jwt(value) || is_github_token(value) || is_github_pat(value) || is_google_api_key(value) || is_slack_token(value) || is_long_alphanumeric(value)
}

/// `IProcessEnvironment` com o `undefined` explícito: `{ [key: string]: string | undefined }`.
pub type OptionalProcessEnvironment = BTreeMap<String, Option<String>>;

/// Sanitizes environment variables for logging by redacting sensitive values.
pub fn sanitize_env_for_logging(env: Option<&OptionalProcessEnvironment>) -> Option<ProcessEnvironment> {
    let env = env?;
    let mut sanitized = ProcessEnvironment::new();
    for (key, value) in env {
        let Some(value) = value else {
            continue;
        };
        // Check if the key name suggests a sensitive value, then if the value matches known secret patterns
        let redact = is_sensitive_env_var_name(key) || is_secret_value(value);
        sanitized.insert(key.clone(), if redact { "<REDACTED>".to_owned() } else { value.clone() });
    }
    Some(sanitized)
}
