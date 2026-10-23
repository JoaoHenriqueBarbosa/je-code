//! Porte de `src/vs/platform/terminal/common/terminalEnvironment.ts`: o escape de caminhos para o shell, a
//! compactação do diretório do usuário em `~`, a limpeza do `cwd` e a regra de uso da coleção de variáveis
//! de ambiente.
//!
//! O `OS` do original, lido ao carregar o módulo, é o `Platform::detect().os` do processo atual. Onde o
//! original indexa texto (`path.slice(userHome.length + 1)`, `cwd[1]`), o porte conta unidades UTF-16 como o
//! JavaScript.

use je_base::platform::{OperatingSystem, Platform};

use super::terminal::{GeneralShellType, IShellLaunchConfig, PosixShellType, ShellType, TerminalShellType, WindowsShellType};

/// O `ShellEscapeConfig` do original: como cada família de shell escapa um caminho conforme as aspas que ele
/// contém.
#[derive(Clone, Copy)]
enum ShellEscape {
    Posix,
    Fish,
    PowerShell,
}

impl ShellEscape {
    fn for_shell(shell_type: TerminalShellType) -> Self {
        match shell_type {
            Some(ShellType::Posix(PosixShellType::Bash | PosixShellType::Sh | PosixShellType::Zsh)) | Some(ShellType::Windows(WindowsShellType::GitBash)) => Self::Posix,
            Some(ShellType::Posix(PosixShellType::Fish)) => Self::Fish,
            // PowerShell should be handled separately in preparePathForShell but if we get here, use
            // PowerShell escaping
            Some(ShellType::General(GeneralShellType::PowerShell)) => Self::PowerShell,
            // Default to POSIX shell escaping for unknown shells
            _ => Self::Posix,
        }
    }

    /// How to handle paths with both single and double quotes.
    fn both_quotes(self, path: &str) -> String {
        match self {
            Self::Posix => format!("$'{}'", path.replace('\'', "\\'")),
            Self::Fish => format!("\"{}\"", path.replace('"', "\\\"")),
            Self::PowerShell => format!("\"{}\"", path.replace('"', "`\"")),
        }
    }

    /// How to handle paths with only single quotes.
    fn single_quotes(self, path: &str) -> String {
        match self {
            // A backslash does not escape inside single quotes, so close, escape, and reopen them
            Self::Posix => format!("'{}'", path.replace('\'', "'\\''")),
            Self::Fish => format!("'{}'", path.replace('\'', "\\'")),
            Self::PowerShell => format!("'{}'", path.replace('\'', "''")),
        }
    }

    /// How to handle paths with no single quotes (may have double quotes).
    fn no_single_quotes(path: &str) -> String {
        format!("'{path}'")
    }
}

/// Aggressively escape non-windows paths to prepare for being sent to a shell. This will do some
/// escaping inaccurately to be careful about possible script injection via the file path. For
/// example, we're trying to prevent this sort of attack: `/foo/file$(echo evil)`.
pub fn escape_non_windows_path(path: &str, shell_type: TerminalShellType) -> String {
    let mut new_path = path.to_owned();
    if new_path.contains('\\') {
        new_path = new_path.replace('\\', "\\\\");
    }

    let escape_config = ShellEscape::for_shell(shell_type);

    // Remove dangerous characters except single and double quotes, which we'll escape properly
    new_path.retain(|c| !"`$|&>~#!^*;<".contains(c));

    // Apply shell-specific escaping based on quote content
    let has_single_quote = new_path.contains('\'');
    if has_single_quote && new_path.contains('"') {
        escape_config.both_quotes(&new_path)
    } else if has_single_quote {
        escape_config.single_quotes(&new_path)
    } else {
        ShellEscape::no_single_quotes(&new_path)
    }
}

/// Collapses the user's home directory into `~` if it exists within the path, this gives a shorter
/// path that is more suitable within the context of a terminal.
pub fn collapse_tilde_path(path: Option<&str>, user_home: Option<&str>, separator: &str) -> String {
    let Some(path) = path.filter(|path| !path.is_empty()) else {
        return String::new();
    };
    let Some(mut user_home) = user_home.filter(|user_home| !user_home.is_empty()) else {
        return path.to_owned();
    };
    // Trim the trailing separator from the end if it exists
    if user_home.ends_with(['/', '\\']) {
        user_home = &user_home[..user_home.len() - 1];
    }
    let normalized_path = path.replace('\\', "/").to_lowercase();
    let normalized_user_home = user_home.replace('\\', "/").to_lowercase();
    if !normalized_path.contains(&normalized_user_home) {
        return path.to_owned();
    }
    let path_units: Vec<u16> = path.encode_utf16().collect();
    let rest_start = user_home.encode_utf16().count() + 1;
    let rest = path_units.get(rest_start..).unwrap_or_default();
    format!("~{separator}{}", String::from_utf16_lossy(rest))
}

/// `/^['"].*['"]$/`: o `.` não casa com terminadores de linha.
fn is_wrapped_in_quotes(cwd: &str) -> bool {
    let chars: Vec<char> = cwd.chars().collect();
    let is_quote = |c: &char| matches!(c, '\'' | '"');
    chars.len() >= 2
        && chars.first().is_some_and(is_quote)
        && chars.last().is_some_and(is_quote)
        && chars[1..chars.len() - 1].iter().all(|c| !matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
}

/// Sanitizes a cwd string, removing any wrapping quotes and making the Windows drive letter
/// uppercase.
pub fn sanitize_cwd(cwd: &str) -> String {
    // Sanity check that the cwd is not wrapped in quotes (see #160109)
    let mut cwd = cwd;
    if is_wrapped_in_quotes(cwd) {
        let mut chars = cwd.chars();
        chars.next();
        chars.next_back();
        cwd = chars.as_str();
    }
    // Make the drive letter uppercase on Windows (see #9448)
    if Platform::detect().os == OperatingSystem::Windows {
        let mut chars = cwd.chars();
        // `cwd[1] === ':'` olha a segunda unidade UTF-16, então o primeiro caractere tem de ser da BMP.
        if let (Some(first), Some(':')) = (chars.next().filter(|c| c.len_utf16() == 1), chars.next()) {
            return first.to_uppercase().chain(cwd.chars().skip(1)).collect();
        }
    }
    cwd.to_owned()
}

/// Determines whether the given shell launch config should use the environment variable collection.
pub fn should_use_environment_variable_collection(slc: &IShellLaunchConfig) -> bool {
    !slc.strict_env.unwrap_or(false)
}
