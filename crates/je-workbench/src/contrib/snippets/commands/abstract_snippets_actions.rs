//! Porte de `src/vs/workbench/contrib/snippets/browser/commands/abstractSnippetsActions.ts`: as opções padrão das ações de
//! snippets (a categoria "Snippets").
//!
//! Notas de tradução:
//! - `abstract class SnippetsAction extends Action2` com `super({ ...defaultOptions, ...desc })` é a função
//!   [`snippets_action_options`]: a ação concreta implementa `Action2` e devolve estas opções no `desc`. O espalhamento
//!   deixa a categoria da própria ação vencer a padrão, como em `{ ...defaultOptions, ...desc }`.
//! - `SnippetEditorAction extends EditorAction2` depende do `EditorAction2` do editor, que ainda não existe no porte, e
//!   nenhuma ação portada o usa; entra com a primeira ação que o estender.

use je_platform::action::{Category, CommandAction, localize2};
use je_platform::actions::actions::Action2Options;

/// `defaultOptions.category`.
fn default_category() -> Category {
    Category::Localized(localize2("snippets", "Snippets"))
}

/// O `desc` de uma `SnippetsAction`: o `desc` recebido, com a categoria "Snippets" quando ele não traz uma.
pub fn snippets_action_options(desc: Action2Options) -> Action2Options {
    let Action2Options { command, menu, keybinding, f1 } = desc;
    let command = CommandAction { category: command.category.clone().or_else(|| Some(default_category())), ..command };
    Action2Options { command, menu, keybinding, f1 }
}
