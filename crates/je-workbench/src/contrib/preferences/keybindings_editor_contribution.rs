//! Porte da parte lógica de `src/vs/workbench/contrib/preferences/browser/keybindingsEditorContribution.ts`:
//! `KeybindingEditorDecorationsRenderer._userSettingsFuzzyEquals`. O restante da contribuição (decorações do modelo de
//! texto, `DefineKeybindingWidget`, ações do editor) depende do editor de texto em DOM e não entra.

use je_base::keybinding_parser::KeybindingParser;

use super::settings_editor2::js_trim;

/// `KeybindingEditorDecorationsRenderer._userSettingsFuzzyEquals(a, b)`: duas strings de atalho das configurações do usuário
/// são equivalentes quando, sem caixa nem espaços nas pontas, são iguais ou escrevem o mesmo atalho (ordem dos modificadores
/// e separador `+`/`-` livres).
pub fn user_settings_fuzzy_equals(a: &str, b: &str) -> bool {
    let a = js_trim(a).to_lowercase();
    let b = js_trim(b).to_lowercase();

    if a == b {
        return true;
    }

    let a_keybinding = KeybindingParser::parse_keybinding(&a);
    let b_keybinding = KeybindingParser::parse_keybinding(&b);
    match (a_keybinding, b_keybinding) {
        (None, None) => true,
        (Some(a_keybinding), Some(b_keybinding)) => a_keybinding.equals(Some(&b_keybinding)),
        _ => false,
    }
}
