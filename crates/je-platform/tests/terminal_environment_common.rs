//! Porte de `src/vs/platform/terminal/test/common/terminalEnvironment.test.ts` (suíte `terminalEnvironment`),
//! caso a caso e na mesma ordem.
//!
//! Divergências: `ensureNoDisposablesAreLeakedInTestSuite` não tem o que vigiar (nada aqui é descartável).
//! O `undefined` de `collapseTildePath` e de `escapeNonWindowsPath` é `None`. O caso `should make the Windows
//! drive letter uppercase`, que o original só registra quando `OS === Windows`, só existe em `cfg(windows)`.

use je_platform::terminal::terminal::{GeneralShellType, PosixShellType, ShellType, WindowsShellType};
use je_platform::terminal::terminal_environment::{collapse_tilde_path, escape_non_windows_path, sanitize_cwd};

fn shell(shell_type: impl Into<ShellType>) -> Option<ShellType> {
    Some(shell_type.into())
}

// suite('collapseTildePath')

#[test]
fn collapse_tilde_path_should_return_empty_string_for_a_falsy_path() {
    assert_eq!(collapse_tilde_path(Some(""), Some("/foo"), "/"), "");
    assert_eq!(collapse_tilde_path(None, Some("/foo"), "/"), "");
}

#[test]
fn collapse_tilde_path_should_return_path_for_a_falsy_user_home() {
    assert_eq!(collapse_tilde_path(Some("/foo"), Some(""), "/"), "/foo");
    assert_eq!(collapse_tilde_path(Some("/foo"), None, "/"), "/foo");
}

#[test]
fn collapse_tilde_path_should_not_collapse_when_user_home_isnt_present() {
    assert_eq!(collapse_tilde_path(Some("/foo"), Some("/bar"), "/"), "/foo");
    assert_eq!(collapse_tilde_path(Some("C:\\foo"), Some("C:\\bar"), "\\"), "C:\\foo");
}

#[test]
fn collapse_tilde_path_should_collapse_with_windows_separators() {
    assert_eq!(collapse_tilde_path(Some("C:\\foo\\bar"), Some("C:\\foo"), "\\"), "~\\bar");
    assert_eq!(collapse_tilde_path(Some("C:\\foo\\bar"), Some("C:\\foo\\"), "\\"), "~\\bar");
    assert_eq!(collapse_tilde_path(Some("C:\\foo\\bar\\baz"), Some("C:\\foo\\"), "\\"), "~\\bar\\baz");
    assert_eq!(collapse_tilde_path(Some("C:\\foo\\bar\\baz"), Some("C:\\foo"), "\\"), "~\\bar\\baz");
}

#[test]
fn collapse_tilde_path_should_collapse_mixed_case_with_windows_separators() {
    assert_eq!(collapse_tilde_path(Some("c:\\foo\\bar"), Some("C:\\foo"), "\\"), "~\\bar");
    assert_eq!(collapse_tilde_path(Some("C:\\foo\\bar\\baz"), Some("c:\\foo"), "\\"), "~\\bar\\baz");
}

#[test]
fn collapse_tilde_path_should_collapse_with_posix_separators() {
    assert_eq!(collapse_tilde_path(Some("/foo/bar"), Some("/foo"), "/"), "~/bar");
    assert_eq!(collapse_tilde_path(Some("/foo/bar"), Some("/foo/"), "/"), "~/bar");
    assert_eq!(collapse_tilde_path(Some("/foo/bar/baz"), Some("/foo"), "/"), "~/bar/baz");
    assert_eq!(collapse_tilde_path(Some("/foo/bar/baz"), Some("/foo/"), "/"), "~/bar/baz");
}

// suite('sanitizeCwd')

#[cfg(windows)]
#[test]
fn sanitize_cwd_should_make_the_windows_drive_letter_uppercase() {
    assert_eq!(sanitize_cwd("c:\\foo\\bar"), "C:\\foo\\bar");
}

#[test]
fn sanitize_cwd_should_remove_any_wrapping_quotes() {
    assert_eq!(sanitize_cwd("'/foo/bar'"), "/foo/bar");
    assert_eq!(sanitize_cwd("\"/foo/bar\""), "/foo/bar");
}

// suite('escapeNonWindowsPath')

#[test]
fn escape_non_windows_path_should_escape_for_bash_sh_zsh_shells() {
    assert_eq!(escape_non_windows_path("/foo/bar", shell(PosixShellType::Bash)), r"'/foo/bar'");
    assert_eq!(escape_non_windows_path("/foo/bar'baz", shell(PosixShellType::Bash)), r"'/foo/bar'\''baz'");
    assert_eq!(escape_non_windows_path("/foo/bar\"baz", shell(PosixShellType::Bash)), r#"'/foo/bar"baz'"#);
    assert_eq!(escape_non_windows_path("/foo/bar'baz\"qux", shell(PosixShellType::Bash)), r#"$'/foo/bar\'baz"qux'"#);
    assert_eq!(escape_non_windows_path("/foo/bar", shell(PosixShellType::Sh)), r"'/foo/bar'");
    assert_eq!(escape_non_windows_path("/foo/bar'baz", shell(PosixShellType::Sh)), r"'/foo/bar'\''baz'");
    assert_eq!(escape_non_windows_path("/foo/bar", shell(PosixShellType::Zsh)), r"'/foo/bar'");
    assert_eq!(escape_non_windows_path("/foo/bar'baz", shell(PosixShellType::Zsh)), r"'/foo/bar'\''baz'");
}

#[test]
fn escape_non_windows_path_should_escape_for_git_bash() {
    assert_eq!(escape_non_windows_path("/foo/bar", shell(WindowsShellType::GitBash)), r"'/foo/bar'");
    assert_eq!(escape_non_windows_path("/foo/bar'baz", shell(WindowsShellType::GitBash)), r"'/foo/bar'\''baz'");
    assert_eq!(escape_non_windows_path("/foo/bar\"baz", shell(WindowsShellType::GitBash)), r#"'/foo/bar"baz'"#);
}

#[test]
fn escape_non_windows_path_should_escape_for_fish_shell() {
    assert_eq!(escape_non_windows_path("/foo/bar", shell(PosixShellType::Fish)), r"'/foo/bar'");
    assert_eq!(escape_non_windows_path("/foo/bar'baz", shell(PosixShellType::Fish)), r"'/foo/bar\'baz'");
    assert_eq!(escape_non_windows_path("/foo/bar\"baz", shell(PosixShellType::Fish)), r#"'/foo/bar"baz'"#);
    assert_eq!(escape_non_windows_path("/foo/bar'baz\"qux", shell(PosixShellType::Fish)), r#""/foo/bar'baz\"qux""#);
}

#[test]
fn escape_non_windows_path_should_escape_for_power_shell() {
    assert_eq!(escape_non_windows_path("/foo/bar", shell(GeneralShellType::PowerShell)), r"'/foo/bar'");
    assert_eq!(escape_non_windows_path("/foo/bar'baz", shell(GeneralShellType::PowerShell)), r"'/foo/bar''baz'");
    assert_eq!(escape_non_windows_path("/foo/bar\"baz", shell(GeneralShellType::PowerShell)), r#"'/foo/bar"baz'"#);
    assert_eq!(escape_non_windows_path("/foo/bar'baz\"qux", shell(GeneralShellType::PowerShell)), r#""/foo/bar'baz`"qux""#);
}

#[test]
fn escape_non_windows_path_should_default_to_posix_escaping_for_unknown_shells() {
    assert_eq!(escape_non_windows_path("/foo/bar", None), r"'/foo/bar'");
    assert_eq!(escape_non_windows_path("/foo/bar'baz", None), r"'/foo/bar'\''baz'");
}

#[test]
fn escape_non_windows_path_should_remove_dangerous_characters() {
    assert_eq!(escape_non_windows_path("/foo/bar$(echo evil)", shell(PosixShellType::Bash)), r"'/foo/bar(echo evil)'");
    assert_eq!(escape_non_windows_path("/foo/bar`whoami`", shell(PosixShellType::Bash)), r"'/foo/barwhoami'");
}
