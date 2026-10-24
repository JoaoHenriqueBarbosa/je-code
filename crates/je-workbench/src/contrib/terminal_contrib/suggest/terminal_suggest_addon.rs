//! Porte parcial de `src/vs/workbench/contrib/terminalContrib/suggest/browser/terminalSuggestAddon.ts`: só o
//! `isInlineCompletionSupported`, a regra de quais shells aceitam o item de sugestão em linha.
//!
//! Fora do porte (e por quê): o `SuggestAddon` (o `ITerminalAddon` que liga o terminal, o serviço de completions, o widget de
//! sugestões, o modelo de entrada do prompt e o `ITerminalLogService`), que depende do widget de sugestões e do DOM do
//! `contrib/terminal/browser` e do `IXtermTerminal` completo, inexistentes no porte.

use je_platform::terminal::terminal::{GeneralShellType, PosixShellType, ShellType, WindowsShellType};

/// `isInlineCompletionSupported(shellType)`.
pub fn is_inline_completion_supported(shell_type: Option<ShellType>) -> bool {
    let Some(shell_type) = shell_type else {
        return false;
    };
    shell_type == ShellType::Posix(PosixShellType::Bash)
        || shell_type == ShellType::Posix(PosixShellType::Zsh)
        || shell_type == ShellType::Posix(PosixShellType::Fish)
        || shell_type == ShellType::General(GeneralShellType::PowerShell)
        || shell_type == ShellType::Windows(WindowsShellType::GitBash)
}
