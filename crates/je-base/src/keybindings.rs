//! Porte de `src/vs/base/common/keybindings.ts`: a decodificação da forma binária dos atalhos
//! (`decodeKeybinding`, `createSimpleKeybinding`), os acordes por `keyCode` e por `scanCode`
//! ([`KeyCodeChord`], [`ScanCodeChord`]), o [`Keybinding`] (sequência de acordes) e o contrato
//! [`ResolvedKeybinding`].
//!
//! O original distingue os acordes por `instanceof`; aqui o [`Chord`] é um enum com as duas variantes.
//! `decodeKeybinding` aceita `number | number[]`, que vira [`BinaryKeybinding`].

use crate::errors::Error;
use crate::key_codes::{KeyCode, ScanCode};
use crate::platform::OperatingSystem;

/// Máscaras da codificação binária:
/// ```text
///    1111 11
///    5432 1098 7654 3210
///    ---- CSAW KKKK KKKK
///  C = bit 11 = ctrlCmd flag
///  S = bit 10 = shift flag
///  A = bit 9 = alt flag
///  W = bit 8 = winCtrl flag
///  K = bits 0-7 = key code
/// ```
mod mask {
    pub const CTRL_CMD: u32 = 1 << 11;
    pub const SHIFT: u32 = 1 << 10;
    pub const ALT: u32 = 1 << 9;
    pub const WIN_CTRL: u32 = 1 << 8;
    pub const KEY_CODE: u32 = 0x0000_00FF;
}

/// O argumento de `decodeKeybinding`: um número (até dois acordes) ou uma lista de acordes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BinaryKeybinding {
    Number(u32),
    Chords(Vec<u32>),
}

impl From<u32> for BinaryKeybinding {
    fn from(value: u32) -> Self {
        BinaryKeybinding::Number(value)
    }
}

impl From<Vec<u32>> for BinaryKeybinding {
    fn from(value: Vec<u32>) -> Self {
        BinaryKeybinding::Chords(value)
    }
}

/// `decodeKeybinding`: `Ok(None)` é o `null` do número zero; uma lista sem acordes lança no original
/// (`new Keybinding([])`), e aqui é o `Err`.
pub fn decode_keybinding(keybinding: impl Into<BinaryKeybinding>, os: OperatingSystem) -> Result<Option<Keybinding>, Error> {
    let chord = |k: u32| Chord::KeyCode(create_simple_keybinding(k, os));
    match keybinding.into() {
        BinaryKeybinding::Number(0) => Ok(None),
        BinaryKeybinding::Number(keybinding) => {
            let first_chord = keybinding & 0x0000_FFFF;
            let second_chord = (keybinding & 0xFFFF_0000) >> 16;
            if second_chord != 0 {
                return Keybinding::new(vec![chord(first_chord), chord(second_chord)]).map(Some);
            }
            Keybinding::new(vec![chord(first_chord)]).map(Some)
        }
        BinaryKeybinding::Chords(keybinding) => Keybinding::new(keybinding.into_iter().map(chord).collect()).map(Some),
    }
}

pub fn create_simple_keybinding(keybinding: u32, os: OperatingSystem) -> KeyCodeChord {
    let ctrl_cmd = keybinding & mask::CTRL_CMD != 0;
    let win_ctrl = keybinding & mask::WIN_CTRL != 0;

    let mac = os == OperatingSystem::Macintosh;
    let ctrl_key = if mac { win_ctrl } else { ctrl_cmd };
    let shift_key = keybinding & mask::SHIFT != 0;
    let alt_key = keybinding & mask::ALT != 0;
    let meta_key = if mac { ctrl_cmd } else { win_ctrl };
    let key_code = KeyCode((keybinding & mask::KEY_CODE) as i32);

    KeyCodeChord::new(ctrl_key, shift_key, alt_key, meta_key, key_code)
}

/// `Modifiers`.
pub trait Modifiers {
    fn ctrl_key(&self) -> bool;
    fn shift_key(&self) -> bool;
    fn alt_key(&self) -> bool;
    fn meta_key(&self) -> bool;
}

fn modifier_hash(ctrl: bool, shift: bool, alt: bool, meta: bool) -> String {
    let bit = |b: bool| if b { '1' } else { '0' };
    [ctrl, shift, alt, meta].into_iter().map(bit).collect()
}

macro_rules! impl_modifiers {
    ($chord:ty) => {
        impl Modifiers for $chord {
            fn ctrl_key(&self) -> bool {
                self.ctrl_key
            }
            fn shift_key(&self) -> bool {
                self.shift_key
            }
            fn alt_key(&self) -> bool {
                self.alt_key
            }
            fn meta_key(&self) -> bool {
                self.meta_key
            }
        }
    };
}

/// Um acorde que usa o campo `keyCode` dos eventos de teclado: combinação de teclas pressionadas ao
/// mesmo tempo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyCodeChord {
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    pub key_code: KeyCode,
}

impl_modifiers!(KeyCodeChord);

impl KeyCodeChord {
    pub fn new(ctrl_key: bool, shift_key: bool, alt_key: bool, meta_key: bool, key_code: KeyCode) -> Self {
        Self { ctrl_key, shift_key, alt_key, meta_key, key_code }
    }

    pub fn equals(&self, other: &Chord) -> bool {
        matches!(other, Chord::KeyCode(other) if self == other)
    }

    pub fn get_hash_code(&self) -> String {
        format!("K{}{}", modifier_hash(self.ctrl_key, self.shift_key, self.alt_key, self.meta_key), self.key_code.0)
    }

    pub fn is_modifier_key(&self) -> bool {
        self.key_code == KeyCode::Unknown
            || self.key_code == KeyCode::Ctrl
            || self.key_code == KeyCode::Meta
            || self.key_code == KeyCode::Alt
            || self.key_code == KeyCode::Shift
    }

    pub fn to_keybinding(self) -> Keybinding {
        Keybinding { chords: vec![Chord::KeyCode(self)] }
    }

    /// Does this keybinding refer to the key code of a modifier and it also has the modifier flag?
    pub fn is_duplicate_modifier_case(&self) -> bool {
        (self.ctrl_key && self.key_code == KeyCode::Ctrl)
            || (self.shift_key && self.key_code == KeyCode::Shift)
            || (self.alt_key && self.key_code == KeyCode::Alt)
            || (self.meta_key && self.key_code == KeyCode::Meta)
    }
}

/// Um acorde que usa o campo `code` dos eventos de teclado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanCodeChord {
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    pub scan_code: ScanCode,
}

impl_modifiers!(ScanCodeChord);

impl ScanCodeChord {
    pub fn new(ctrl_key: bool, shift_key: bool, alt_key: bool, meta_key: bool, scan_code: ScanCode) -> Self {
        Self { ctrl_key, shift_key, alt_key, meta_key, scan_code }
    }

    pub fn equals(&self, other: &Chord) -> bool {
        matches!(other, Chord::ScanCode(other) if self == other)
    }

    pub fn get_hash_code(&self) -> String {
        format!("S{}{}", modifier_hash(self.ctrl_key, self.shift_key, self.alt_key, self.meta_key), self.scan_code.0)
    }

    /// Does this keybinding refer to the key code of a modifier and it also has the modifier flag?
    pub fn is_duplicate_modifier_case(&self) -> bool {
        (self.ctrl_key && (self.scan_code == ScanCode::ControlLeft || self.scan_code == ScanCode::ControlRight))
            || (self.shift_key && (self.scan_code == ScanCode::ShiftLeft || self.scan_code == ScanCode::ShiftRight))
            || (self.alt_key && (self.scan_code == ScanCode::AltLeft || self.scan_code == ScanCode::AltRight))
            || (self.meta_key && (self.scan_code == ScanCode::MetaLeft || self.scan_code == ScanCode::MetaRight))
    }
}

/// `Chord = KeyCodeChord | ScanCodeChord`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chord {
    KeyCode(KeyCodeChord),
    ScanCode(ScanCodeChord),
}

impl Chord {
    pub fn equals(&self, other: &Chord) -> bool {
        self == other
    }

    pub fn get_hash_code(&self) -> String {
        match self {
            Chord::KeyCode(c) => c.get_hash_code(),
            Chord::ScanCode(c) => c.get_hash_code(),
        }
    }

    pub fn is_duplicate_modifier_case(&self) -> bool {
        match self {
            Chord::KeyCode(c) => c.is_duplicate_modifier_case(),
            Chord::ScanCode(c) => c.is_duplicate_modifier_case(),
        }
    }
}

impl From<KeyCodeChord> for Chord {
    fn from(chord: KeyCodeChord) -> Self {
        Chord::KeyCode(chord)
    }
}

impl From<ScanCodeChord> for Chord {
    fn from(chord: ScanCodeChord) -> Self {
        Chord::ScanCode(chord)
    }
}

/// Um atalho é uma sequência de acordes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keybinding {
    pub chords: Vec<Chord>,
}

impl Keybinding {
    /// `new Keybinding(chords)`: sem acordes o original lança `illegalArgument('chords')`.
    pub fn new(chords: Vec<Chord>) -> Result<Self, Error> {
        if chords.is_empty() {
            return Err(Error::new("Illegal argument: chords"));
        }
        Ok(Self { chords })
    }

    pub fn get_hash_code(&self) -> String {
        self.chords.iter().map(Chord::get_hash_code).collect::<Vec<_>>().join(";")
    }

    /// `equals(other: Keybinding | null)`: `None` é o `null`.
    pub fn equals(&self, other: Option<&Keybinding>) -> bool {
        other.is_some_and(|other| self == other)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedChord {
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    pub key_label: Option<String>,
    pub key_aria_label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SingleModifierChord {
    Ctrl,
    Shift,
    Alt,
    Meta,
}

/// Um atalho resolvido: um ou mais acordes.
pub trait ResolvedKeybinding {
    /// This prints the binding in a format suitable for displaying in the UI.
    fn get_label(&self) -> Option<String>;
    /// This prints the binding in a format suitable for ARIA.
    fn get_aria_label(&self) -> Option<String>;
    /// This prints the binding in a format suitable for electron's accelerators.
    /// See <https://github.com/electron/electron/blob/master/docs/api/accelerator.md>
    fn get_electron_accelerator(&self) -> Option<String>;
    /// This prints the binding in a format suitable for user settings.
    fn get_user_settings_label(&self) -> Option<String>;
    /// Is the user settings label reflecting the label?
    fn is_wysiwyg(&self) -> bool;
    /// Does the keybinding consist of more than one chord?
    fn has_multiple_chords(&self) -> bool;
    /// Returns the chords that comprise of the keybinding.
    fn get_chords(&self) -> Vec<ResolvedChord>;
    /// Returns the chords as strings useful for dispatching.
    /// Returns `None` for modifier only chords.
    /// Example: keybinding "Shift" gives `None`; ("D" with shift == true) gives `"shift+D"`.
    fn get_dispatch_chords(&self) -> Vec<Option<String>>;
    /// Returns the modifier only chords as strings useful for dispatching.
    /// Returns `None` for chords that contain more than one modifier or a regular key.
    /// Example: keybinding "Shift" gives `shift`; ("D" with shift == true) gives `None`.
    fn get_single_modifier_dispatch_chords(&self) -> Vec<Option<SingleModifierChord>>;
}
