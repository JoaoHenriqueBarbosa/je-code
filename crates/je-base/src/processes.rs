//! Porte de `src/vs/base/common/processes.ts`: tipos de execução de processo e a limpeza do ambiente que o
//! VS Code passa a processos filhos.

use crate::process::ProcessEnvironment;

/// `CommandOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandOptions {
    /// O diretório de trabalho do programa ou do shell. Se ausente, vale a raiz do workspace.
    pub cwd: Option<String>,
    /// O ambiente do programa ou do shell. Se ausente, vale o do processo pai.
    pub env: Option<std::collections::BTreeMap<String, String>>,
}

/// `Executable`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Executable {
    /// O comando: um programa externo ou um comando de shell.
    pub command: String,
    /// Se o comando precisa de um interpretador de shell (`cmd.exe`, `bash`...).
    pub is_shell_command: bool,
    pub args: Vec<String>,
    pub options: Option<CommandOptions>,
}

/// `ForkOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ForkOptions {
    pub command: CommandOptions,
    pub exec_argv: Option<Vec<String>>,
}

/// `Source` (`const enum`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Stdout = 0,
    Stderr = 1,
}

/// `SuccessData`.
#[derive(Clone, Debug, Default)]
pub struct SuccessData {
    pub error: Option<crate::errors::Error>,
    pub cmd_code: Option<i32>,
    pub terminated: Option<bool>,
}

/// `ErrorData`.
#[derive(Clone, Debug, Default)]
pub struct ErrorData {
    pub error: Option<crate::errors::Error>,
    pub terminated: Option<bool>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

/// `TerminateResponseCode` (`const enum`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminateResponseCode {
    Success = 0,
    Unknown = 1,
    AccessDenied = 2,
    ProcessNotFound = 3,
}

/// `TerminateResponse`.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminateResponse {
    pub success: bool,
    pub code: Option<TerminateResponseCode>,
    /// O `error: any` do original, reduzido a texto.
    pub error: Option<String>,
}

/// `ProcessItem`.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessItem {
    pub name: String,
    pub cmd: String,
    pub pid: f64,
    pub ppid: f64,
    pub load: f64,
    pub mem: f64,
    pub children: Option<Vec<ProcessItem>>,
}

/// O `.` de uma regex sem a flag `s`: qualquer unidade menos os terminadores de linha.
fn is_dot(c: char) -> bool {
    !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `/^PREFIX.+$/` depois do prefixo.
fn dot_plus(rest: &str) -> bool {
    !rest.is_empty() && rest.chars().all(is_dot)
}

/// Alguma das quatro expressões de `keysToRemove` casa com a chave (`key.search(regex) !== -1`)?
fn is_variable_to_remove(key: &str) -> bool {
    // /^ELECTRON_.+$/
    if key.strip_prefix("ELECTRON_").is_some_and(dot_plus) {
        return true;
    }
    // /^VSCODE_(?!(PORTABLE|SHELL_LOGIN|ENV_REPLACE|ENV_APPEND|ENV_PREPEND)).+$/
    if let Some(rest) = key.strip_prefix("VSCODE_") {
        let preserved = ["PORTABLE", "SHELL_LOGIN", "ENV_REPLACE", "ENV_APPEND", "ENV_PREPEND"].iter().any(|p| rest.starts_with(p));
        if !preserved && dot_plus(rest) {
            return true;
        }
    }
    // /^SNAP(|_.*)$/
    if let Some(rest) = key.strip_prefix("SNAP")
        && (rest.is_empty() || rest.strip_prefix('_').is_some_and(|tail| tail.chars().all(is_dot)))
    {
        return true;
    }
    // /^GDK_PIXBUF_.+$/
    key.strip_prefix("GDK_PIXBUF_").is_some_and(dot_plus)
}

/// `sanitizeProcessEnvironment`: tira do ambiente o que é do Electron, do próprio VS Code, do Snap e do
/// GDK-Pixbuf, menos as chaves em `preserve`.
pub fn sanitize_process_environment(env: &mut ProcessEnvironment, preserve: &[&str]) {
    env.retain(|key, _| preserve.contains(&key.as_str()) || !is_variable_to_remove(key));
}

/// `removeDangerousEnvVariables`: tira variáveis que injetam código no processo filho, sem diferenciar
/// maiúsculas de minúsculas.
pub fn remove_dangerous_env_variables(env: Option<&mut ProcessEnvironment>) {
    let Some(env) = env else { return };
    const DANGEROUS: [&str; 5] = ["DEBUG", "NODE_OPTIONS", "VSCODE_NODE_OPTIONS", "LD_PRELOAD", "DYLD_INSERT_LIBRARIES"];
    env.retain(|key, _| !DANGEROUS.contains(&crate::js_string::JsString::from(key.as_str()).to_upper_case().to_string_lossy().as_str()));
}
