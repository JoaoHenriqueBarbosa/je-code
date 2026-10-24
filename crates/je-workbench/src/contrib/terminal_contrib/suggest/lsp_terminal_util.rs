//! Porte de `src/vs/workbench/contrib/terminalContrib/suggest/browser/lspTerminalUtil.ts`: o delimitador do documento virtual
//! do terminal e as linguagens com suporte a LSP.
//!
//! Decisões de tradução:
//! - `new Set<{ shellType, languageId, extension }>([...])` é a constante [`TERMINAL_LSP_SUPPORTED_LANGUAGES`] (uma lista na
//!   ordem de inserção, que é a ordem de iteração do `Set`).

/// `VSCODE_LSP_TERMINAL_PROMPT_TRACKER`.
pub const VSCODE_LSP_TERMINAL_PROMPT_TRACKER: &str = "vscode_lsp_terminal_prompt_tracker= {}\n";

/// O elemento de `terminalLspSupportedLanguages`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalLspSupportedLanguage {
    pub shell_type: &'static str,
    pub language_id: &'static str,
    pub extension: &'static str,
}

/// `terminalLspSupportedLanguages`.
pub const TERMINAL_LSP_SUPPORTED_LANGUAGES: [TerminalLspSupportedLanguage; 1] =
    [TerminalLspSupportedLanguage { shell_type: "python", language_id: "python", extension: "py" }];

/// `getTerminalLspSupportedLanguageObj(shellType)`.
pub fn get_terminal_lsp_supported_language_obj(shell_type: &str) -> Option<TerminalLspSupportedLanguage> {
    TERMINAL_LSP_SUPPORTED_LANGUAGES.iter().find(|supported_language| supported_language.shell_type == shell_type).copied()
}
