//! A resolução de atalhos do editor: a tecla apertada vira um acorde (`KeyCodeChord`) e é procurada no
//! `KeybindingsRegistry` (onde os módulos do `je-editor` registram os `kbOpts` dos comandos e das ações) com as
//! condições `when` avaliadas contra as chaves de contexto do editor com foco no texto, como o
//! `KeybindingResolver` do VS Code. Acordes em sequência (`Ctrl+K Ctrl+C`) esperam o segundo acorde.

use je_base::dynamic::Dynamic;
use je_base::key_codes::{KeyCode, KeyCodeUtils};
use je_base::keybindings::{Chord, KeyCodeChord};
use je_platform::contextkey::contextkey::IContext;
use je_platform::keybinding::keybindings_registry::{KeybindingItem, keybindings_registry};
use je_ui::Modifiers;

use crate::input::EditorKey;

/// O que um acorde resolveu.
pub(crate) enum Resolution {
    /// O comando e os argumentos do atalho.
    Command(String, Dynamic),
    /// O acorde abre uma sequência: o próximo acorde decide.
    Pending,
    /// Nenhum atalho do editor.
    None,
}

/// As chaves de contexto de um editor de texto gravável com foco (`EditorContextKeys`).
pub(crate) struct EditorContext {
    pub has_selection: bool,
    pub has_multiple_selections: bool,
}

impl IContext for EditorContext {
    fn get_value(&self, key: &str) -> Dynamic {
        let flag = match key {
            "editorTextFocus" | "textInputFocus" | "editorFocus" => true,
            "editorHasSelection" => self.has_selection,
            "editorHasMultipleSelections" => self.has_multiple_selections,
            "isLinux" => cfg!(target_os = "linux"),
            "isWindows" => cfg!(windows),
            "isMac" => cfg!(target_os = "macos"),
            _ => false,
        };
        Dynamic::Bool(flag)
    }
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

fn first_chord(item: &KeybindingItem) -> Option<&Chord> {
    item.keybinding.as_ref().and_then(|k| k.chords.first())
}

/// O resolvedor, com o primeiro acorde de uma sequência em espera.
#[derive(Default)]
pub(crate) struct Shortcuts {
    pending: Option<KeyCodeChord>,
}

impl Shortcuts {
    /// Resolve `chord` (o segundo da sequência se havia um em espera). O último atalho registrado que casa vence
    /// (os de peso maior ficam no fim da lista).
    pub fn resolve(&mut self, chord: KeyCodeChord, context: &EditorContext) -> Resolution {
        let items = keybindings_registry().get_default_keybindings();
        let applies = |item: &KeybindingItem| item.when.as_ref().is_none_or(|when| when.evaluate(context));
        let is_command = |item: &&KeybindingItem| item.command.as_deref().is_some_and(|c| !c.starts_with('-'));
        let wanted = Chord::KeyCode(chord);
        if let Some(first) = self.pending.take() {
            let first = Chord::KeyCode(first);
            let hit = items.iter().rev().filter(is_command).find(|item| {
                item.keybinding.as_ref().is_some_and(|k| k.chords.len() == 2 && k.chords[0] == first && k.chords[1] == wanted) && applies(item)
            });
            return match hit {
                Some(item) => Resolution::Command(item.command.clone().unwrap_or_default(), item.command_args.clone()),
                None => Resolution::None,
            };
        }
        let single = items.iter().rev().filter(is_command).find(|item| {
            item.keybinding.as_ref().is_some_and(|k| k.chords.len() == 1 && k.chords[0] == wanted) && applies(item)
        });
        if let Some(item) = single {
            return Resolution::Command(item.command.clone().unwrap_or_default(), item.command_args.clone());
        }
        let opens_sequence = items.iter().filter(is_command).any(|item| item.keybinding.as_ref().is_some_and(|k| k.chords.len() == 2) && first_chord(item) == Some(&wanted) && applies(item));
        if opens_sequence {
            self.pending = Some(chord);
            return Resolution::Pending;
        }
        Resolution::None
    }

    /// Há um primeiro acorde esperando o segundo.
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Abandona a sequência em espera (o foco mudou, ou o usuário digitou texto).
    pub fn cancel(&mut self) {
        self.pending = None;
    }
}
