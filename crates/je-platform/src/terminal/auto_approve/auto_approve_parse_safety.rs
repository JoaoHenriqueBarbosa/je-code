//! Porte de `src/vs/platform/terminal/common/autoApprove/autoApproveParseSafety.ts`.
//!
//! Divergências: o `language: 'bash' | 'powershell'` do original é o enum [`AutoApproveParseLanguage`]. O
//! `TODO` do original (consolidar os helpers de aprovação automática, issue 329028 do VS Code) é comentário
//! do upstream e fica registrado aqui como origem, não como pendência deste porte.

/// A linguagem do `language: 'bash' | 'powershell'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoApproveParseLanguage {
    Bash,
    Powershell,
}

/// Returns whether a tree-sitter parse error should prevent auto-approval.
/// The command can still run after the user confirms it.
pub fn should_require_confirmation_for_auto_approve_parse(language: AutoApproveParseLanguage, has_error: bool) -> bool {
    language == AutoApproveParseLanguage::Powershell && has_error
}
