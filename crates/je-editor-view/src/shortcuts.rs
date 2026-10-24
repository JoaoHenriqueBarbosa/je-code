//! A resolução de atalhos do editor pelo `KeybindingResolver` portado, como o `WorkbenchKeybindingService` a monta:
//! os atalhos de fábrica (os `kbOpts` que os módulos do `je-editor` registram no `KeybindingsRegistry`) com os do
//! `keybindings.json` do usuário por cima (o do usuário vence; `-comando` tira o atalho de fábrica), cada um
//! resolvido para o teclado americano (`USLayoutResolvedKeybinding`), e as condições `when` avaliadas contra as
//! chaves de contexto do editor. Acordes em sequência (`Ctrl+K Ctrl+C`) esperam o próximo acorde.
//!
//! O `keybindings.json` é lido uma vez por thread com [`set_user_keybindings`] e vale para todos os editores, como
//! o serviço do workbench; o resolvedor é refeito quando os atalhos do usuário ou os registrados mudam.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::key_codes::{KeyCode, KeyCodeUtils};
use je_base::keybindings::{Chord, KeyCodeChord, Keybinding};
use je_base::platform::{OperatingSystem, Platform};
use je_platform::contextkey::contextkey::IContext;
use je_platform::keybinding::keybinding_resolver::{KeybindingResolver, ResolutionResult};
use je_platform::keybinding::keybindings_registry::{KeybindingItem, keybindings_registry};
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use je_platform::keybinding::us_layout_resolved_keybinding::USLayoutResolvedKeybinding;
use je_ui::Modifiers;
use je_workbench::services::keybinding::keybinding_io::{KeybindingIO, UserKeybindingItem};

use crate::input::EditorKey;

/// O que um acorde resolveu.
pub(crate) enum Resolution {
    /// O comando e os argumentos do atalho.
    Command(String, Dynamic),
    /// O acorde abre uma sequência: o próximo acorde decide.
    Pending,
    /// Nenhum atalho.
    None,
}

/// As chaves de contexto de um editor de texto gravável com foco (`EditorContextKeys`).
pub(crate) struct EditorContext {
    pub has_selection: bool,
    pub has_multiple_selections: bool,
    /// `editorLangId`.
    pub language_id: String,
}

impl IContext for EditorContext {
    fn get_value(&self, key: &str) -> Dynamic {
        let flag = match key {
            "editorLangId" => return Dynamic::from(self.language_id.as_str()),
            "editorTextFocus" | "textInputFocus" | "editorFocus" | "inputFocus" => true,
            "editorHasSelection" => self.has_selection,
            "editorHasMultipleSelections" => self.has_multiple_selections,
            "editorReadonly" | "editorTabMovesFocus" => false,
            "isLinux" => cfg!(target_os = "linux"),
            "isWindows" => cfg!(windows),
            "isMac" => cfg!(target_os = "macos"),
            _ => return Dynamic::Undefined,
        };
        Dynamic::Bool(flag)
    }
}

/// Os atalhos do usuário e a geração deles; o resolvedor em cache com a geração e a contagem de atalhos de fábrica
/// para a qual foi montado.
#[derive(Default)]
struct State {
    user: Vec<UserKeybindingItem>,
    generation: u64,
    resolver: Option<(u64, usize, Rc<KeybindingResolver>)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn os() -> OperatingSystem {
    Platform::detect().os
}

/// `_resolveKeybindingItems(items, isDefault)` do `WorkbenchKeybindingService`: um item por resolução do atalho
/// (em ordem reversa, como o original), e o item sem atalho (uma remoção do usuário) como está.
fn resolve_items(items: impl Iterator<Item = (Option<Keybinding>, Option<String>, Dynamic, Option<je_platform::contextkey::ContextKeyExpression>, Option<String>, bool)>, is_default: bool) -> Vec<Rc<ResolvedKeybindingItem>> {
    let os = os();
    let mut result = Vec::new();
    for (keybinding, command, args, when, extension_id, is_builtin) in items {
        match keybinding {
            None => result.push(ResolvedKeybindingItem::new(None, command.as_deref(), args, when, is_default, extension_id.as_deref(), is_builtin, false)),
            Some(keybinding) => {
                for resolved in USLayoutResolvedKeybinding::resolve_keybinding(&keybinding, os).into_iter().rev() {
                    result.push(ResolvedKeybindingItem::new(Some(resolved), command.as_deref(), args.clone(), when.clone(), is_default, extension_id.as_deref(), is_builtin, false));
                }
            }
        }
    }
    result
}

/// O resolvedor atual (refeito se os atalhos do usuário ou os registrados mudaram).
fn resolver() -> Rc<KeybindingResolver> {
    let defaults = keybindings_registry().get_default_keybindings();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        if let Some((generation, count, resolver)) = &state.resolver
            && *generation == state.generation
            && *count == defaults.len()
        {
            return resolver.clone();
        }
        let default_items = resolve_items(
            defaults.iter().map(|k: &KeybindingItem| (k.keybinding.clone(), k.command.clone(), k.command_args.clone(), k.when.clone(), k.extension_id.clone(), k.is_builtin_extension)),
            true,
        );
        let user_items = resolve_items(state.user.iter().map(|k| (k.keybinding.clone(), k.command.clone(), k.command_args.clone(), k.when.clone(), None, false)), false);
        let resolver = Rc::new(KeybindingResolver::new(default_items, user_items, Rc::new(|_| {})));
        state.resolver = Some((state.generation, defaults.len(), resolver.clone()));
        resolver
    })
}

/// Lê o `keybindings.json` do usuário (JSON com comentários e vírgula final, uma lista de
/// `{ "key", "command", "when", "args" }`) e passa a usá-lo em todos os editores. Devolve quantos itens leu.
pub fn set_user_keybindings(text: &str) -> Result<usize, String> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut errors = Vec::new();
    let value = je_base::json::parse(&units, &mut errors, je_base::json::ParseOptions { allow_empty_content: true, ..Default::default() });
    let items: Vec<UserKeybindingItem> = match value.map(|v| Dynamic::from_json(&v)) {
        Some(Dynamic::Array(list)) => list.to_vec().iter().map(KeybindingIO::read_user_keybinding_item).collect(),
        None | Some(Dynamic::Undefined | Dynamic::Null) => Vec::new(),
        Some(_) => return Err("o keybindings.json precisa ser uma lista".to_owned()),
    };
    let count = items.len();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.user = items;
        state.generation += 1;
    });
    Ok(count)
}

/// O `KeyCode` de uma tecla do editor.
pub(crate) fn key_code_of(key: EditorKey) -> KeyCode {
    match key {
        EditorKey::Left => KeyCode::LeftArrow,
        EditorKey::Right => KeyCode::RightArrow,
        EditorKey::Up => KeyCode::UpArrow,
        EditorKey::Down => KeyCode::DownArrow,
        EditorKey::Home => KeyCode::Home,
        EditorKey::End => KeyCode::End,
        EditorKey::PageUp => KeyCode::PageUp,
        EditorKey::PageDown => KeyCode::PageDown,
        EditorKey::Backspace => KeyCode::Backspace,
        EditorKey::Delete => KeyCode::Delete,
        EditorKey::Enter => KeyCode::Enter,
        EditorKey::Tab => KeyCode::Tab,
        EditorKey::Escape => KeyCode::Escape,
        EditorKey::Char(c) => {
            // Com Shift o caractere do teclado americano é o de cima da tecla (`{` em `[`): o atalho é da tecla.
            let c = match c {
                '{' => '[',
                '}' => ']',
                ')' => '0',
                _ => c,
            };
            let name = c.to_string();
            match KeyCodeUtils::from_user_settings(&name) {
                KeyCode::Unknown => KeyCodeUtils::from_string(&name),
                code => code,
            }
        }
    }
}

/// O acorde da tecla com os modificadores.
pub(crate) fn chord_of(key: EditorKey, mods: Modifiers) -> KeyCodeChord {
    KeyCodeChord::new(mods.ctrl, mods.shift, mods.alt, mods.meta, key_code_of(key))
}

/// O acorde como o `KeyboardMapper` o despacha (`ctrl+shift+K`).
fn dispatch_string(chord: KeyCodeChord) -> Option<String> {
    let keybinding = Keybinding::new(vec![Chord::KeyCode(chord)]).ok()?;
    USLayoutResolvedKeybinding::resolve_keybinding(&keybinding, os()).first()?.get_dispatch_chords().into_iter().next().flatten()
}

/// O resolvedor, com os acordes de uma sequência em espera.
#[derive(Default)]
pub(crate) struct Shortcuts {
    pending: Vec<String>,
}

impl Shortcuts {
    /// Resolve `chord` (o próximo da sequência se havia acordes em espera).
    pub fn resolve(&mut self, chord: KeyCodeChord, context: &EditorContext) -> Resolution {
        let Some(keypress) = dispatch_string(chord) else {
            self.pending.clear();
            return Resolution::None;
        };
        let current = std::mem::take(&mut self.pending);
        match resolver().resolve(context, &current, &keypress) {
            ResolutionResult::MoreChordsNeeded => {
                self.pending = current;
                self.pending.push(keypress);
                Resolution::Pending
            }
            ResolutionResult::KbFound { command_id: Some(id), command_args, .. } => Resolution::Command(id, command_args),
            ResolutionResult::KbFound { command_id: None, .. } | ResolutionResult::NoMatchingKb => Resolution::None,
        }
    }

    /// Há acordes esperando o próximo.
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Abandona a sequência em espera (o foco mudou, ou o usuário digitou texto).
    pub fn cancel(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use je_platform::keybinding::keybindings_registry::{KeybindingRule, KeybindingWeight, Keybindings};

    fn context(language: &str) -> EditorContext {
        EditorContext { has_selection: false, has_multiple_selections: false, language_id: language.to_owned() }
    }

    fn command(resolution: Resolution) -> Option<String> {
        match resolution {
            Resolution::Command(id, _) => Some(id),
            _ => None,
        }
    }

    #[test]
    fn user_keybindings_override_remove_and_respect_when() {
        let ctrl_shift_k = KeyCodeChord::new(true, true, false, false, KeyCode::KeyK);
        let rule = KeybindingRule {
            keybindings: Keybindings { primary: Some(je_base::key_codes::KeyMod::CTRL_CMD | je_base::key_codes::KeyMod::SHIFT | KeyCode::KeyK.0 as u32), ..Default::default() },
            id: "test.deleteLines".to_owned(),
            weight: KeybindingWeight::EDITOR_CORE,
            args: Dynamic::Undefined,
            when: None,
        };
        let _registration = keybindings_registry().register_keybinding_rule(rule);
        let mut shortcuts = Shortcuts::default();
        set_user_keybindings("[]").expect("lista vazia");
        assert_eq!(command(shortcuts.resolve(ctrl_shift_k, &context("rust"))).as_deref(), Some("test.deleteLines"), "o atalho de fábrica");
        // O do usuário vence; e `when` com `editorLangId` só vale na linguagem certa.
        set_user_keybindings(r#"[
            // comentário
            { "key": "ctrl+shift+k", "command": "test.userCommand", "when": "editorLangId == rust", "args": { "n": 1 } },
        ]"#)
        .expect("lê");
        assert_eq!(command(shortcuts.resolve(ctrl_shift_k, &context("rust"))).as_deref(), Some("test.userCommand"));
        assert_eq!(command(shortcuts.resolve(ctrl_shift_k, &context("toml"))).as_deref(), Some("test.deleteLines"), "fora do `when` vale o de fábrica");
        // `-comando` tira o atalho de fábrica.
        set_user_keybindings(r#"[{ "key": "ctrl+shift+k", "command": "-test.deleteLines" }]"#).expect("lê");
        assert!(command(shortcuts.resolve(ctrl_shift_k, &context("rust"))).is_none());
        // Uma sequência do usuário espera o segundo acorde.
        set_user_keybindings(r#"[{ "key": "ctrl+k ctrl+x", "command": "test.chord" }]"#).expect("lê");
        assert!(matches!(shortcuts.resolve(KeyCodeChord::new(true, false, false, false, KeyCode::KeyK), &context("rust")), Resolution::Pending));
        assert_eq!(command(shortcuts.resolve(KeyCodeChord::new(true, false, false, false, KeyCode::KeyX), &context("rust"))).as_deref(), Some("test.chord"));
        set_user_keybindings("[]").expect("lê");
    }
}
