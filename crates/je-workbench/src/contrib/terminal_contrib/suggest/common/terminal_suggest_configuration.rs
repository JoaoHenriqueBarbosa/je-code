//! Porte parcial de `src/vs/workbench/contrib/terminalContrib/suggest/common/terminalSuggestConfiguration.ts`:
//! os identificadores das configurações ([`TerminalSuggestSettingId`]), as extensões executáveis padrão do
//! Windows e o nome da seção. O restante do arquivo (a interface `ITerminalSuggestConfiguration`, o registro
//! das propriedades de configuração) não é lido pelo serviço de completions e fica para o porte da
//! configuração.

/// `TerminalSuggestSettingId`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TerminalSuggestSettingId {
    Enabled,
    QuickSuggestions,
    SuggestOnTriggerCharacters,
    RunOnEnter,
    WindowsExecutableExtensions,
    Providers,
    ShowStatusBar,
    CdPath,
    InlineSuggestion,
    UpArrowNavigatesHistory,
    SelectionMode,
    InsertTrailingSpace,
}

impl TerminalSuggestSettingId {
    /// O valor de texto do `const enum` do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "terminal.integrated.suggest.enabled",
            Self::QuickSuggestions => "terminal.integrated.suggest.quickSuggestions",
            Self::SuggestOnTriggerCharacters => "terminal.integrated.suggest.suggestOnTriggerCharacters",
            Self::RunOnEnter => "terminal.integrated.suggest.runOnEnter",
            Self::WindowsExecutableExtensions => "terminal.integrated.suggest.windowsExecutableExtensions",
            Self::Providers => "terminal.integrated.suggest.providers",
            Self::ShowStatusBar => "terminal.integrated.suggest.showStatusBar",
            Self::CdPath => "terminal.integrated.suggest.cdPath",
            Self::InlineSuggestion => "terminal.integrated.suggest.inlineSuggestion",
            Self::UpArrowNavigatesHistory => "terminal.integrated.suggest.upArrowNavigatesHistory",
            Self::SelectionMode => "terminal.integrated.suggest.selectionMode",
            Self::InsertTrailingSpace => "terminal.integrated.suggest.insertTrailingSpace",
        }
    }
}

/// `windowsDefaultExecutableExtensions`.
pub const WINDOWS_DEFAULT_EXECUTABLE_EXTENSIONS: &[&str] = &[
    "exe", // Executable file
    "bat", // Batch file
    "cmd", // Command script
    "com", // Command file
    "msi", // Windows Installer package
    "ps1", // PowerShell script
    "vbs", // VBScript file
    "js",  // JScript file
    "jar", // Java Archive (requires Java runtime)
    "py",  // Python script (requires Python interpreter)
    "rb",  // Ruby script (requires Ruby interpreter)
    "pl",  // Perl script (requires Perl interpreter)
    "sh",  // Shell script (via WSL or third-party tools)
];

/// `terminalSuggestConfigSection`.
pub const TERMINAL_SUGGEST_CONFIG_SECTION: &str = "terminal.integrated.suggest";
