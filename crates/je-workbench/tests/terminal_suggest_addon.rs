//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest/test/browser/terminalSuggestAddon.test.ts` (suíte `Terminal Suggest Addon - Inline Completion, Shell Type Support`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** A suíte não cria nada descartável: o guarda de vazamentos é criado no
//!   começo de cada caso, como o `setup` implícito do original.
//! - **`isInlineCompletionSupported(undefined)`.** É `is_inline_completion_supported(None)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::terminal::terminal::{GeneralShellType, PosixShellType, ShellType, WindowsShellType};
use je_workbench::contrib::terminal_contrib::suggest::terminal_suggest_addon::is_inline_completion_supported;

#[test]
fn should_return_true_for_supported_shell_types() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(is_inline_completion_supported(Some(ShellType::Posix(PosixShellType::Bash))));
    assert!(is_inline_completion_supported(Some(ShellType::Posix(PosixShellType::Zsh))));
    assert!(is_inline_completion_supported(Some(ShellType::Posix(PosixShellType::Fish))));
    assert!(is_inline_completion_supported(Some(ShellType::General(GeneralShellType::PowerShell))));
    assert!(is_inline_completion_supported(Some(ShellType::Windows(WindowsShellType::GitBash))));
}

#[test]
fn should_return_false_for_unsupported_shell_types() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!is_inline_completion_supported(Some(ShellType::General(GeneralShellType::NuShell))));
    assert!(!is_inline_completion_supported(Some(ShellType::General(GeneralShellType::Julia))));
    assert!(!is_inline_completion_supported(Some(ShellType::General(GeneralShellType::Node))));
    assert!(!is_inline_completion_supported(Some(ShellType::General(GeneralShellType::Python))));
    assert!(!is_inline_completion_supported(Some(ShellType::General(GeneralShellType::Xonsh))));
    assert!(!is_inline_completion_supported(Some(ShellType::Posix(PosixShellType::Sh))));
    assert!(!is_inline_completion_supported(Some(ShellType::Posix(PosixShellType::Csh))));
    assert!(!is_inline_completion_supported(Some(ShellType::Posix(PosixShellType::Ksh))));
    assert!(!is_inline_completion_supported(Some(ShellType::Windows(WindowsShellType::CommandPrompt))));
    assert!(!is_inline_completion_supported(Some(ShellType::Windows(WindowsShellType::Wsl))));
    assert!(!is_inline_completion_supported(Some(ShellType::General(GeneralShellType::Python))));
    assert!(!is_inline_completion_supported(None));
}
