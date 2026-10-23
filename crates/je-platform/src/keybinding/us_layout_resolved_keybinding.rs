//! Porte de `src/vs/platform/keybinding/common/usLayoutResolvedKeybinding.ts`.
//!
//! Não instancie direto: o serviço de atalhos devolve um `ResolvedKeybinding` semeado com o layout de teclado atual.

use std::rc::Rc;

use je_base::key_codes::{KeyCode, KeyCodeUtils, ScanCode, immutable_code_to_key_code};
use je_base::keybindings::{Chord, KeyCodeChord, Keybinding, ResolvedKeybinding, SingleModifierChord};
use je_base::platform::OperatingSystem;

use super::base_resolved_keybinding::{BaseChord, BaseResolvedKeybinding, ChordLabels};
use super::resolved_keybinding_item::to_empty_array_if_contains_none;

impl BaseChord for KeyCodeChord {
    fn is_duplicate_modifier_case(&self) -> bool {
        KeyCodeChord::is_duplicate_modifier_case(self)
    }
}

/// Os rótulos de um acorde por `keyCode`, no layout US.
#[derive(Clone, Copy, Default)]
pub struct UsLayoutLabels;

fn key_code_to_ui_label(os: OperatingSystem, key_code: KeyCode) -> Option<String> {
    if os == OperatingSystem::Macintosh {
        match key_code {
            KeyCode::LeftArrow => return Some("←".to_string()),
            KeyCode::UpArrow => return Some("↑".to_string()),
            KeyCode::RightArrow => return Some("→".to_string()),
            KeyCode::DownArrow => return Some("↓".to_string()),
            _ => {}
        }
    }
    KeyCodeUtils::to_string(key_code).map(str::to_string)
}

impl ChordLabels<KeyCodeChord> for UsLayoutLabels {
    fn get_label(&self, os: OperatingSystem, chord: &KeyCodeChord) -> Option<String> {
        if chord.is_duplicate_modifier_case() {
            return Some(String::new());
        }
        key_code_to_ui_label(os, chord.key_code)
    }

    fn get_aria_label(&self, _os: OperatingSystem, chord: &KeyCodeChord) -> Option<String> {
        if chord.is_duplicate_modifier_case() {
            return Some(String::new());
        }
        KeyCodeUtils::to_string(chord.key_code).map(str::to_string)
    }

    fn get_electron_accelerator(&self, _os: OperatingSystem, chord: &KeyCodeChord) -> Option<String> {
        KeyCodeUtils::to_electron_accelerator(chord.key_code).map(str::to_string)
    }

    fn get_user_settings_label(&self, _os: OperatingSystem, chord: &KeyCodeChord) -> Option<String> {
        if chord.is_duplicate_modifier_case() {
            return Some(String::new());
        }
        KeyCodeUtils::to_user_settings_us(chord.key_code).map(str::to_lowercase)
    }

    fn is_wysiwyg(&self, _chord: &KeyCodeChord) -> bool {
        true
    }

    fn get_chord_dispatch(&self, chord: &KeyCodeChord) -> Option<String> {
        USLayoutResolvedKeybinding::get_dispatch_str(chord)
    }

    fn get_single_modifier_chord_dispatch(&self, keybinding: &KeyCodeChord) -> Option<SingleModifierChord> {
        if keybinding.key_code == KeyCode::Ctrl && !keybinding.shift_key && !keybinding.alt_key && !keybinding.meta_key {
            return Some(SingleModifierChord::Ctrl);
        }
        if keybinding.key_code == KeyCode::Shift && !keybinding.ctrl_key && !keybinding.alt_key && !keybinding.meta_key {
            return Some(SingleModifierChord::Shift);
        }
        if keybinding.key_code == KeyCode::Alt && !keybinding.ctrl_key && !keybinding.shift_key && !keybinding.meta_key {
            return Some(SingleModifierChord::Alt);
        }
        if keybinding.key_code == KeyCode::Meta && !keybinding.ctrl_key && !keybinding.shift_key && !keybinding.alt_key {
            return Some(SingleModifierChord::Meta);
        }
        None
    }
}

/// `USLayoutResolvedKeybinding`.
pub type USLayoutResolvedKeybinding = BaseResolvedKeybinding<KeyCodeChord, UsLayoutLabels>;

impl USLayoutResolvedKeybinding {
    pub fn from_chords(chords: Vec<KeyCodeChord>, os: OperatingSystem) -> Self {
        BaseResolvedKeybinding::new(os, chords, UsLayoutLabels)
    }

    /// `getDispatchStr`: `None` para um acorde só de modificador.
    pub fn get_dispatch_str(chord: &KeyCodeChord) -> Option<String> {
        if chord.is_modifier_key() {
            return None;
        }
        let mut result = String::new();
        if chord.ctrl_key {
            result.push_str("ctrl+");
        }
        if chord.shift_key {
            result.push_str("shift+");
        }
        if chord.alt_key {
            result.push_str("alt+");
        }
        if chord.meta_key {
            result.push_str("meta+");
        }
        result.push_str(KeyCodeUtils::to_string(chord.key_code).unwrap_or(""));
        Some(result)
    }

    /// `_scanCodeToKeyCode`: confira se o retorno é `KeyCode::Unknown`.
    fn scan_code_to_key_code(scan_code: ScanCode) -> KeyCode {
        if let Some(immutable) = immutable_code_to_key_code(scan_code)
            && immutable != KeyCode::DependsOnKbLayout
        {
            return immutable;
        }
        match scan_code {
            ScanCode::KeyA => KeyCode::KeyA,
            ScanCode::KeyB => KeyCode::KeyB,
            ScanCode::KeyC => KeyCode::KeyC,
            ScanCode::KeyD => KeyCode::KeyD,
            ScanCode::KeyE => KeyCode::KeyE,
            ScanCode::KeyF => KeyCode::KeyF,
            ScanCode::KeyG => KeyCode::KeyG,
            ScanCode::KeyH => KeyCode::KeyH,
            ScanCode::KeyI => KeyCode::KeyI,
            ScanCode::KeyJ => KeyCode::KeyJ,
            ScanCode::KeyK => KeyCode::KeyK,
            ScanCode::KeyL => KeyCode::KeyL,
            ScanCode::KeyM => KeyCode::KeyM,
            ScanCode::KeyN => KeyCode::KeyN,
            ScanCode::KeyO => KeyCode::KeyO,
            ScanCode::KeyP => KeyCode::KeyP,
            ScanCode::KeyQ => KeyCode::KeyQ,
            ScanCode::KeyR => KeyCode::KeyR,
            ScanCode::KeyS => KeyCode::KeyS,
            ScanCode::KeyT => KeyCode::KeyT,
            ScanCode::KeyU => KeyCode::KeyU,
            ScanCode::KeyV => KeyCode::KeyV,
            ScanCode::KeyW => KeyCode::KeyW,
            ScanCode::KeyX => KeyCode::KeyX,
            ScanCode::KeyY => KeyCode::KeyY,
            ScanCode::KeyZ => KeyCode::KeyZ,
            ScanCode::Digit1 => KeyCode::Digit1,
            ScanCode::Digit2 => KeyCode::Digit2,
            ScanCode::Digit3 => KeyCode::Digit3,
            ScanCode::Digit4 => KeyCode::Digit4,
            ScanCode::Digit5 => KeyCode::Digit5,
            ScanCode::Digit6 => KeyCode::Digit6,
            ScanCode::Digit7 => KeyCode::Digit7,
            ScanCode::Digit8 => KeyCode::Digit8,
            ScanCode::Digit9 => KeyCode::Digit9,
            ScanCode::Digit0 => KeyCode::Digit0,
            ScanCode::Minus => KeyCode::Minus,
            ScanCode::Equal => KeyCode::Equal,
            ScanCode::BracketLeft => KeyCode::BracketLeft,
            ScanCode::BracketRight => KeyCode::BracketRight,
            ScanCode::Backslash => KeyCode::Backslash,
            ScanCode::IntlHash => KeyCode::Unknown, // missing
            ScanCode::Semicolon => KeyCode::Semicolon,
            ScanCode::Quote => KeyCode::Quote,
            ScanCode::Backquote => KeyCode::Backquote,
            ScanCode::Comma => KeyCode::Comma,
            ScanCode::Period => KeyCode::Period,
            ScanCode::Slash => KeyCode::Slash,
            ScanCode::IntlBackslash => KeyCode::IntlBackslash,
            _ => KeyCode::Unknown,
        }
    }

    fn to_key_code_chord(chord: &Chord) -> Option<KeyCodeChord> {
        match chord {
            Chord::KeyCode(chord) => Some(*chord),
            Chord::ScanCode(chord) => {
                let key_code = Self::scan_code_to_key_code(chord.scan_code);
                if key_code == KeyCode::Unknown {
                    return None;
                }
                Some(KeyCodeChord::new(chord.ctrl_key, chord.shift_key, chord.alt_key, chord.meta_key, key_code))
            }
        }
    }

    /// `resolveKeybinding`.
    pub fn resolve_keybinding(keybinding: &Keybinding, os: OperatingSystem) -> Vec<Rc<dyn ResolvedKeybinding>> {
        let chords: Vec<KeyCodeChord> =
            to_empty_array_if_contains_none(keybinding.chords.iter().map(Self::to_key_code_chord).collect());
        if !chords.is_empty() {
            return vec![Rc::new(Self::from_chords(chords, os))];
        }
        Vec::new()
    }
}
