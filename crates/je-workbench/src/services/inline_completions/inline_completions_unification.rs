//! Porte do contrato de `src/vs/workbench/services/inlineCompletions/common/inlineCompletionsUnification.ts`:
//! [`IInlineCompletionsUnificationState`] e [`IInlineCompletionsUnificationService`].
//!
//! PENDENTE: `InlineCompletionsUnificationImpl` (a implementação do serviço), que lê os serviços de experimentação, de
//! chaves de contexto, de configuração e de habilitação de extensões e ainda não tem o conjunto deles injetável. O
//! `MainThreadLanguageFeatures` só consome o contrato.

use je_base::event::Event;

/// `IInlineCompletionsUnificationState`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IInlineCompletionsUnificationState {
    pub code_unification: bool,
    pub model_unification: bool,
    pub extension_unification: bool,
    pub exp_assignments: Vec<String>,
}

/// `IInlineCompletionsUnificationService`.
pub trait IInlineCompletionsUnificationService {
    fn state(&self) -> IInlineCompletionsUnificationState;

    fn on_did_state_change(&self) -> Event<()>;
}
