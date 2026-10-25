//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/model/InlineSuggestAlternativeAction.ts`.

use je_base::promise::Promise;
use je_base::themables::ThemeIcon;

use crate::code_action::languages::Command;

/// `InlineSuggestAlternativeAction`: a ação alternativa de uma sugestão em linha.
#[derive(Clone)]
pub struct InlineSuggestAlternativeAction {
    pub label: String,
    pub icon: ThemeIcon,
    pub command: Command,
    pub count: Promise<f64>,
}

impl InlineSuggestAlternativeAction {
    /// `InlineSuggestAlternativeAction.toString(action)`: o id do comando, ou `None` sem ação.
    pub fn to_display_string(action: Option<&InlineSuggestAlternativeAction>) -> Option<String> {
        action.map(|action| action.command.id.clone())
    }
}
