//! Porte de `src/vs/workbench/contrib/terminalContrib/history/common/terminal.history.ts`: os ids dos comandos do
//! histórico, a lista dos que o terminal não repassa ao shell e a configuração `terminal.integrated.shellIntegration.history`.
//!
//! Divergências:
//! - Os `const enum` de string (`TerminalHistoryCommandId`, `TerminalHistorySettingId`) são enums com `as_str()`
//!   devolvendo o valor do original, como os de `je_platform::terminal::terminal`.
//! - `terminalHistoryConfiguration` (`IStringDictionary<IConfigurationPropertySchema>`) é a lista de pares
//!   `(chave, PropertySchema)` na ordem do objeto: o `PropertySchema` guarda os campos na mesma ordem do literal
//!   (`restricted`, `markdownDescription`, `type`, `default`). Como o esquema é um objeto dinâmico (não pode ser
//!   `const`), a lista vem de uma função.

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_platform::configuration::registry::PropertySchema;

/// `TerminalHistoryCommandId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalHistoryCommandId {
    ClearPreviousSessionHistory,
    GoToRecentDirectory,
    RunRecentCommand,
}

impl TerminalHistoryCommandId {
    /// O valor do `const enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClearPreviousSessionHistory => "workbench.action.terminal.clearPreviousSessionHistory",
            Self::GoToRecentDirectory => "workbench.action.terminal.goToRecentDirectory",
            Self::RunRecentCommand => "workbench.action.terminal.runRecentCommand",
        }
    }
}

/// `defaultTerminalHistoryCommandsToSkipShell`.
pub const DEFAULT_TERMINAL_HISTORY_COMMANDS_TO_SKIP_SHELL: [TerminalHistoryCommandId; 2] =
    [TerminalHistoryCommandId::GoToRecentDirectory, TerminalHistoryCommandId::RunRecentCommand];

/// `TerminalHistorySettingId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalHistorySettingId {
    ShellIntegrationCommandHistory,
}

impl TerminalHistorySettingId {
    /// O valor do `const enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ShellIntegrationCommandHistory => "terminal.integrated.shellIntegration.history",
        }
    }
}

/// `terminalHistoryConfiguration`.
pub fn terminal_history_configuration() -> Vec<(&'static str, PropertySchema)> {
    vec![(
        TerminalHistorySettingId::ShellIntegrationCommandHistory.as_str(),
        PropertySchema::from_entries([
            ("restricted", Dynamic::Bool(true)),
            (
                "markdownDescription",
                Dynamic::string(&localize(
                    "terminal.integrated.shellIntegration.history",
                    "Controls the number of recently used commands to keep in the terminal command history. Set to 0 to disable terminal command history.",
                    &[],
                )),
            ),
            ("type", Dynamic::string("number")),
            ("default", Dynamic::Number(100.0)),
        ]),
    )]
}
