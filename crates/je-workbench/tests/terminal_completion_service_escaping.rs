//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest/test/browser/terminalCompletionService.escaping.test.ts` (suíte `escapeTerminalCompletionLabel`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de cada caso.
//! - **O laço `for (const { char, label, expected } of cases) test(...)`.** São 15 `#[test]` (um por elemento de `cases`, na
//!   ordem), todos pelo mesmo [`check_escape`]; os três casos que vêm depois do laço seguem como no original. O arquivo tem 18
//!   casos, não 4.
//! - **Literais.** `'\\['` do original é `\[` (barra invertida e colchete): em Rust, `"\\["`.
//! - **`shellType` e `pathSeparator`.** `PosixShellType.Bash` e `'/'`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::terminal::terminal::{GeneralShellType, PosixShellType, WindowsShellType};
use je_workbench::contrib::terminal_contrib::suggest::terminal_completion_service::escape_terminal_completion_label;

const PATH_SEPARATOR: &str = "/";

/// `escapeTerminalCompletionLabel(label, PosixShellType.Bash, pathSeparator)` igual a `expected`.
#[track_caller]
fn check_escape(label: &str, expected: &str) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = escape_terminal_completion_label(label, Some(PosixShellType::Bash.into()), PATH_SEPARATOR);
    assert_eq!(result, expected);
}

#[test]
fn should_escape_open_bracket_in_label_abc() {
    check_escape("[abc", "\\[abc");
}

#[test]
fn should_escape_close_bracket_in_abc_label() {
    check_escape("abc]", "abc\\]");
}

#[test]
fn should_escape_open_parenthesis_in_label_abc() {
    check_escape("(abc", "\\(abc");
}

#[test]
fn should_escape_close_parenthesis_in_abc_label() {
    check_escape("abc)", "abc\\)");
}

#[test]
fn should_escape_single_quote_in_label_abc() {
    check_escape("'abc", "\\'abc");
}

#[test]
fn should_escape_double_quote_in_label_abc() {
    check_escape("\"abc", "\\\"abc");
}

#[test]
fn should_escape_backslash_in_abc_label() {
    check_escape("abc\\", "abc\\\\");
}

#[test]
fn should_escape_backtick_in_label_abc() {
    check_escape("`abc", "\\`abc");
}

#[test]
fn should_escape_asterisk_in_label_abc() {
    check_escape("*abc", "\\*abc");
}

#[test]
fn should_escape_question_mark_in_label_abc() {
    check_escape("?abc", "\\?abc");
}

#[test]
fn should_escape_semicolon_in_label_abc() {
    check_escape(";abc", "\\;abc");
}

#[test]
fn should_escape_ampersand_in_label_abc() {
    check_escape("&abc", "\\&abc");
}

#[test]
fn should_escape_pipe_in_label_abc() {
    check_escape("|abc", "\\|abc");
}

#[test]
fn should_escape_less_than_in_label_abc() {
    check_escape("<abc", "\\<abc");
}

#[test]
fn should_escape_greater_than_in_label_abc() {
    check_escape(">abc", "\\>abc");
}

#[test]
fn should_not_escape_when_no_special_chars() {
    check_escape("abc", "abc");
}

#[test]
fn should_not_escape_for_powershell() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = escape_terminal_completion_label("[abc", Some(GeneralShellType::PowerShell.into()), PATH_SEPARATOR);
    assert_eq!(result, "[abc");
}

#[test]
fn should_not_escape_for_command_prompt() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let result = escape_terminal_completion_label("[abc", Some(WindowsShellType::CommandPrompt.into()), PATH_SEPARATOR);
    assert_eq!(result, "[abc");
}
