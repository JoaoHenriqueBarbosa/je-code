//! Porte de `src/vs/workbench/contrib/debug/node/terminals.ts`: a linha de comando que o depurador
//! digita no terminal integrado, com as regras de citação de cada shell (cmd, PowerShell, bash), e a
//! sonda que diz se um shell está ocupado.
//!
//! Notas de tradução:
//! - `prepareCommand` do original faz `args.shift()` no array do chamador (no ramo PowerShell). Aqui os
//!   argumentos são uma fatia só de leitura: o resultado é o mesmo, sem mexer no que é do chamador.
//! - O `env` do original é um objeto cuja ordem de chaves é a de inserção; aqui é uma fatia de pares
//!   `(chave, valor)` na mesma ordem, e `None` no valor é o `null` (remover a variável).
//! - `hasChildProcesses` era assíncrona; aqui bloqueia até o `pgrep` terminar. No Windows o original
//!   usa o módulo nativo `@vscode/windows-process-tree`, que não existe em Rust seguro: o porte
//!   consulta os processos filhos pelo CIM do PowerShell (`Win32_Process`), com a mesma pergunta
//!   ("o shell tem ao menos um filho?") e a mesma saída de segurança em caso de falha (`true`).

use je_base::platform::Platform;
use std::process::Command;

use je_base::extpath::get_drive_letter;

/// Uma variável de ambiente a definir (`Some(valor)`) ou remover (`None`, o `null` do original).
pub type EnvEntry<'a> = (&'a str, Option<&'a str>);

/// `hasChildProcesses`: se o shell de `process_id` tem ao menos um processo filho (e portanto está
/// ocupado). Sem `process_id` (ou 0), cai para o lado seguro: `true`.
pub fn has_child_processes(process_id: Option<u32>) -> bool {
    let is_windows = Platform::detect().is_windows;
    let Some(pid) = process_id.filter(|&p| p != 0) else {
        // fall back to safe side
        return true;
    };

    // if shell has at least one child process, assume that shell is busy
    if is_windows {
        let script = format!("(Get-CimInstance Win32_Process -Filter 'ParentProcessId={pid}' | Measure-Object).Count");
        return match Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &script]).output() {
            Ok(output) => String::from_utf8_lossy(&output.stdout).trim().parse::<u64>().is_ok_and(|n| n > 0),
            Err(_) => true,
        };
    }

    match Command::new("/usr/bin/pgrep").args(["-lP", &pid.to_string()]).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let r = stdout.trim();
            // ignore 'tmux'; see #43683
            !(r.is_empty() || r.contains(" tmux"))
        }
        Err(_) => true,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellType {
    Cmd,
    Powershell,
    Bash,
}

/// `<` e `>` nunca são citados (são redirecionamentos), a menos que o argumento seja um argumento
/// inteiro e não interpretável.
fn push_arg(command: &mut String, a: &str, args_can_be_interpreted_by_shell: bool, quote: &dyn Fn(&str) -> String) {
    if a == "<" || a == ">" || args_can_be_interpreted_by_shell {
        command.push_str(a);
    } else {
        command.push_str(&quote(a));
    }
    command.push(' ');
}

fn powershell_quote(s: &str) -> String {
    let s = s.replace('\'', "''");
    if s.ends_with('\\') {
        return format!("'{s}\\'");
    }
    format!("'{s}'")
}

fn cmd_quote(s: &str) -> String {
    // Note: Wrapping in cmd /C "..." complicates the escaping.
    // cmd /C "node -e "console.log(process.argv)" """A^>0"""" # prints "A>0"
    // cmd /C "node -e "console.log(process.argv)" "foo^> bar"" # prints foo> bar
    // Outside of the cmd /C, it could be a simple quoting, but here, the ^ is needed too
    let s = s.replace('"', "\"\"");
    let mut escaped = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '>' | '<' | '!' | '^' | '&' | '|') {
            escaped.push('^');
        }
        escaped.push(c);
    }
    if escaped.contains([' ', '"']) || escaped.is_empty() { format!("\"{escaped}\"") } else { escaped }
}

fn bash_quote(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '"' | '\'' | '\\' | '$' | '!' | '>' | '<' | '#' | '(' | ')' | '[' | ']' | '*' | '&' | '^' | '|' | ' ' | ';' | '{' | '}' | '?' | '`'
        ) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    if escaped.is_empty() { "\"\"".to_string() } else { escaped }
}

/// `/[^\w@%\/+=,.:^-]/.test(s) ? '...' : s`.
fn bash_hard_quote(s: &str) -> String {
    let needs_quotes = s.chars().any(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '@' | '%' | '/' | '+' | '=' | ',' | '.' | ':' | '^' | '-')));
    if needs_quotes { format!("'{}'", s.replace('\'', "'\\''")) } else { s.to_string() }
}

/// `prepareCommand`: monta a linha que o terminal recebe para rodar `args` em `shell`, na pasta `cwd` e
/// com as variáveis `env`. Quando `args_can_be_interpreted_by_shell` é verdadeiro, os argumentos entram
/// crus (o shell os interpreta); senão cada um é citado conforme o shell.
pub fn prepare_command(
    shell: &str,
    args: &[String],
    args_can_be_interpreted_by_shell: bool,
    cwd: Option<&str>,
    env: Option<&[EnvEntry<'_>]>,
) -> String {
    let is_windows = Platform::detect().is_windows;
    // `String.prototype.trim` e `toLowerCase`
    let shell = shell.trim().to_lowercase();

    // try to determine the shell type
    let shell_type = if shell.contains("powershell") || shell.contains("pwsh") {
        ShellType::Powershell
    } else if shell.contains("cmd.exe") {
        ShellType::Cmd
    } else if shell.contains("bash") {
        ShellType::Bash
    } else if is_windows {
        ShellType::Cmd // pick a good default for Windows
    } else {
        ShellType::Bash // pick a good default for anything else
    };

    // begin command with a space to avoid polluting shell history
    let mut command = String::from(" ");
    let cwd = cwd.filter(|c| !c.is_empty());

    match shell_type {
        ShellType::Powershell => {
            if let Some(cwd) = cwd {
                if let Some(drive_letter) = get_drive_letter(cwd, is_windows) {
                    command.push_str(&format!("{drive_letter}:; "));
                }
                command.push_str(&format!("cd {}; ", powershell_quote(cwd)));
            }
            if let Some(env) = env {
                for (key, value) in env {
                    match value {
                        None => command.push_str(&format!("Remove-Item env:{key}; ")),
                        Some(value) => {
                            let mut doubled = String::with_capacity(value.len());
                            for c in value.chars() {
                                doubled.push(c);
                                if matches!(c, '\'' | '\u{2018}' | '\u{2019}') {
                                    doubled.push(c);
                                }
                            }
                            command.push_str(&format!("${{env:{key}}}='{doubled}'; "));
                        }
                    }
                }
            }
            if let Some((first, rest)) = args.split_first() {
                let cmd = if args_can_be_interpreted_by_shell { first.clone() } else { powershell_quote(first) };
                if cmd.starts_with('\'') {
                    command.push_str(&format!("& {cmd} "));
                } else {
                    command.push_str(&format!("{cmd} "));
                }
                for a in rest {
                    push_arg(&mut command, a, args_can_be_interpreted_by_shell, &powershell_quote);
                }
            }
        }

        ShellType::Cmd => {
            if let Some(cwd) = cwd {
                if let Some(drive_letter) = get_drive_letter(cwd, is_windows) {
                    command.push_str(&format!("{drive_letter}: && "));
                }
                command.push_str(&format!("cd {} && ", cmd_quote(cwd)));
            }
            if let Some(env) = env {
                command.push_str("cmd /C \"");
                for (key, value) in env {
                    match value {
                        None => command.push_str(&format!("set \"{key}=\" && ")),
                        Some(value) => {
                            let mut escaped = String::with_capacity(value.len());
                            for c in value.chars() {
                                if matches!(c, '&' | '^' | '|' | '<' | '>') {
                                    escaped.push('^');
                                }
                                escaped.push(c);
                            }
                            command.push_str(&format!("set \"{key}={escaped}\" && "));
                        }
                    }
                }
            }
            for a in args {
                push_arg(&mut command, a, args_can_be_interpreted_by_shell, &cmd_quote);
            }
            if env.is_some() {
                command.push('"');
            }
        }

        ShellType::Bash => {
            if let Some(cwd) = cwd {
                command.push_str(&format!("cd {} ; ", bash_quote(cwd)));
            }
            if let Some(env) = env {
                command.push_str("/usr/bin/env");
                for (key, value) in env {
                    match value {
                        None => command.push_str(&format!(" -u {}", bash_hard_quote(key))),
                        Some(value) => command.push_str(&format!(" {}", bash_hard_quote(&format!("{key}={value}")))),
                    }
                }
                command.push(' ');
            }
            for a in args {
                push_arg(&mut command, a, args_can_be_interpreted_by_shell, &bash_quote);
            }
        }
    }

    command
}
