//! Porte de `src/vs/workbench/services/keybinding/common/windowsKeyboardMapper.ts`.
//!
//! Notas de tradução:
//! - Os vetores esparsos do original (`_codeInfo`, `_scanCodeToKeyCode`, `_keyCodeToLabel`, `_keyCodeExists`) viram
//!   mapas por [`ScanCode`] e [`KeyCode`], porque a decodificação binária aceita qualquer código de 0 a 255, inclusive
//!   os que o enum não nomeia. Um índice ausente é o `undefined` do original.
//! - `WindowsNativeResolvedKeybinding` mantém uma referência ao mapeador (`_mapper`). Aqui o estado que os rótulos
//!   consultam fica em [`WindowsKeyboardMapperState`], compartilhado por `Rc` entre o mapeador e cada atalho resolvido.
//! - O `IWindowsKeyboardMapping` é um `IndexMap<String, WindowsKeyMapping>`: o construtor aceita qualquer iterador de
//!   pares `(&String, &WindowsKeyMapping)`, e a ordem de iteração é a do `for ... in` do original.
//! - `LOG` e `log` (sempre desligados no original) não têm porte.

use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::rc::Rc;

use je_base::key_codes::{
    KeyCode, KeyCodeUtils, ScanCode, ScanCodeUtils, immutable_code_to_key_code, native_windows_key_code_to_key_code,
};
use je_base::keybinding_labels::ModifierLabelProvider;
use je_base::keybindings::{
    Chord, KeyCodeChord, Keybinding, ResolvedChord, ResolvedKeybinding, ScanCodeChord, SingleModifierChord,
};
use je_base::platform::OperatingSystem;
use je_platform::keybinding::base_resolved_keybinding::{BaseResolvedKeybinding, ChordLabels};
use je_platform::keybinding::keybinding::KeyboardEvent;
use je_platform::keybinding::resolved_keybinding_item::to_empty_array_if_contains_none;
use je_platform::keybinding::us_layout_resolved_keybinding::{USLayoutResolvedKeybinding, UsLayoutLabels};
use je_platform::keyboard_layout::{IKeyboardMapper, WindowsKeyMapping};

/// `IScanCodeMapping`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanCodeMapping {
    pub scan_code: ScanCode,
    pub key_code: KeyCode,
    pub value: String,
    pub with_shift: String,
    pub with_alt_gr: String,
    pub with_shift_alt_gr: String,
}

/// Os rótulos de um acorde por `keyCode` que consultam o mapa de teclas do mapeador do Windows.
#[derive(Clone)]
pub struct WindowsNativeLabels {
    mapper: Rc<WindowsKeyboardMapperState>,
}

impl WindowsNativeLabels {
    /// `__isWYSIWYG`.
    fn key_code_is_wysiwyg(&self, key_code: KeyCode) -> bool {
        if key_code == KeyCode::LeftArrow
            || key_code == KeyCode::UpArrow
            || key_code == KeyCode::RightArrow
            || key_code == KeyCode::DownArrow
        {
            return true;
        }
        let aria_label = self.mapper.get_aria_label_for_key_code(key_code);
        let user_settings_label = self.mapper.get_user_settings_label_for_key_code(key_code);
        aria_label.as_deref() == user_settings_label
    }
}

impl ChordLabels<KeyCodeChord> for WindowsNativeLabels {
    fn get_label(&self, _os: OperatingSystem, chord: &KeyCodeChord) -> Option<String> {
        if chord.is_duplicate_modifier_case() {
            return Some(String::new());
        }
        self.mapper.get_ui_label_for_key_code(chord.key_code)
    }

    fn get_aria_label(&self, _os: OperatingSystem, chord: &KeyCodeChord) -> Option<String> {
        if chord.is_duplicate_modifier_case() {
            return Some(String::new());
        }
        self.mapper.get_aria_label_for_key_code(chord.key_code)
    }

    fn get_electron_accelerator(&self, _os: OperatingSystem, chord: &KeyCodeChord) -> Option<String> {
        self.mapper.get_electron_accelerator_for_key_binding(chord).map(str::to_string)
    }

    fn get_user_settings_label(&self, _os: OperatingSystem, chord: &KeyCodeChord) -> Option<String> {
        if chord.is_duplicate_modifier_case() {
            return Some(String::new());
        }
        self.mapper.get_user_settings_label_for_key_code(chord.key_code).map(str::to_lowercase)
    }

    fn is_wysiwyg(&self, chord: &KeyCodeChord) -> bool {
        self.key_code_is_wysiwyg(chord.key_code)
    }

    // O despacho (`_getChordDispatch` e `_getSingleModifierChordDispatch`) é o mesmo do layout US.
    fn get_chord_dispatch(&self, chord: &KeyCodeChord) -> Option<String> {
        USLayoutResolvedKeybinding::get_dispatch_str(chord)
    }

    fn get_single_modifier_chord_dispatch(&self, chord: &KeyCodeChord) -> Option<SingleModifierChord> {
        UsLayoutLabels.get_single_modifier_chord_dispatch(chord)
    }
}

/// `WindowsNativeResolvedKeybinding`: o `BaseResolvedKeybinding` com os rótulos do mapeador, mais `getUSLabel`.
pub struct WindowsNativeResolvedKeybinding {
    base: BaseResolvedKeybinding<KeyCodeChord, WindowsNativeLabels>,
    chords: Vec<KeyCodeChord>,
}

impl WindowsNativeResolvedKeybinding {
    pub fn new(mapper: Rc<WindowsKeyboardMapperState>, chords: Vec<KeyCodeChord>) -> Self {
        let base = BaseResolvedKeybinding::new(OperatingSystem::Windows, chords.clone(), WindowsNativeLabels { mapper });
        Self { base, chords }
    }

    /// `getUSLabel`.
    pub fn get_us_label(&self) -> Option<String> {
        ModifierLabelProvider::ui().to_label(OperatingSystem::Windows, &self.chords, |chord| {
            // `_getUSLabelForKeybinding`
            if chord.is_duplicate_modifier_case() {
                return Some(String::new());
            }
            KeyCodeUtils::to_string(chord.key_code).map(str::to_string)
        })
    }

    /// `getProducedCharCode`: `None` é o `null` de um mapeamento ausente.
    fn get_produced_char_code<'a>(chord: &ScanCodeChord, mapping: Option<&'a ScanCodeMapping>) -> Option<&'a str> {
        let mapping = mapping?;
        if chord.ctrl_key && chord.shift_key && chord.alt_key {
            return Some(&mapping.with_shift_alt_gr);
        }
        if chord.ctrl_key && chord.alt_key {
            return Some(&mapping.with_alt_gr);
        }
        if chord.shift_key {
            return Some(&mapping.with_shift);
        }
        Some(&mapping.value)
    }

    /// `getProducedChar`.
    pub fn get_produced_char(chord: &ScanCodeChord, mapping: Option<&ScanCodeMapping>) -> String {
        match Self::get_produced_char_code(chord, mapping) {
            None | Some("") => " --- ".to_string(),
            Some(char) => format!("  {char}  "),
        }
    }
}

impl ResolvedKeybinding for WindowsNativeResolvedKeybinding {
    fn get_label(&self) -> Option<String> {
        self.base.get_label()
    }

    fn get_aria_label(&self) -> Option<String> {
        self.base.get_aria_label()
    }

    fn get_electron_accelerator(&self) -> Option<String> {
        self.base.get_electron_accelerator()
    }

    fn get_user_settings_label(&self) -> Option<String> {
        self.base.get_user_settings_label()
    }

    fn is_wysiwyg(&self) -> bool {
        self.base.is_wysiwyg()
    }

    fn has_multiple_chords(&self) -> bool {
        self.base.has_multiple_chords()
    }

    fn get_chords(&self) -> Vec<ResolvedChord> {
        self.base.get_chords()
    }

    fn get_dispatch_chords(&self) -> Vec<Option<String>> {
        self.base.get_dispatch_chords()
    }

    fn get_single_modifier_dispatch_chords(&self) -> Vec<Option<SingleModifierChord>> {
        self.base.get_single_modifier_dispatch_chords()
    }
}

/// `IMMUTABLE_CODE_TO_KEY_CODE[scanCode]`, com [`KeyCode::DependsOnKbLayout`] para o que o mapa não conhece.
pub(super) fn immutable_key_code(scan_code: ScanCode) -> KeyCode {
    immutable_code_to_key_code(scan_code).unwrap_or(KeyCode::DependsOnKbLayout)
}

/// `NATIVE_WINDOWS_KEY_CODE_TO_KEY_CODE[vkey] || KeyCode.Unknown`.
fn native_key_code(vkey: &str) -> KeyCode {
    native_windows_key_code_to_key_code(vkey).unwrap_or(KeyCode::Unknown)
}

/// Os `KeyCode` das letras de A a Z, na ordem do alfabeto.
const LETTER_KEY_CODES: [KeyCode; 26] = [
    KeyCode::KeyA,
    KeyCode::KeyB,
    KeyCode::KeyC,
    KeyCode::KeyD,
    KeyCode::KeyE,
    KeyCode::KeyF,
    KeyCode::KeyG,
    KeyCode::KeyH,
    KeyCode::KeyI,
    KeyCode::KeyJ,
    KeyCode::KeyK,
    KeyCode::KeyL,
    KeyCode::KeyM,
    KeyCode::KeyN,
    KeyCode::KeyO,
    KeyCode::KeyP,
    KeyCode::KeyQ,
    KeyCode::KeyR,
    KeyCode::KeyS,
    KeyCode::KeyT,
    KeyCode::KeyU,
    KeyCode::KeyV,
    KeyCode::KeyW,
    KeyCode::KeyX,
    KeyCode::KeyY,
    KeyCode::KeyZ,
];

/// As teclas de pontuação que recebem o rótulo do layout US quando o layout não produz nenhuma letra latina.
const US_PUNCTUATION_LABELS: [(KeyCode, char); 11] = [
    (KeyCode::Semicolon, ';'),
    (KeyCode::Equal, '='),
    (KeyCode::Comma, ','),
    (KeyCode::Minus, '-'),
    (KeyCode::Period, '.'),
    (KeyCode::Slash, '/'),
    (KeyCode::Backquote, '`'),
    (KeyCode::BracketLeft, '['),
    (KeyCode::Backslash, '\\'),
    (KeyCode::BracketRight, ']'),
    (KeyCode::Quote, '\''),
];

/// O que o `WindowsKeyboardMapper` guarda depois de ler o mapa de teclas: tudo menos a identidade do mapeador.
/// Os métodos públicos do original (`getUILabelForKeyCode`...) são daqui, e o mapeador os expõe por `Deref`.
pub struct WindowsKeyboardMapperState {
    is_us_standard: bool,
    map_alt_gr_to_ctrl_alt: bool,
    code_info: HashMap<ScanCode, ScanCodeMapping>,
    scan_code_to_key_code: HashMap<ScanCode, KeyCode>,
    /// `None` é o `null` de uma tecla que não produz texto; ausente é o `undefined`.
    key_code_to_label: HashMap<KeyCode, Option<String>>,
    key_code_exists: HashSet<KeyCode>,
}

impl WindowsKeyboardMapperState {
    fn new<'a, I>(is_us_standard: bool, raw_mappings: I, map_alt_gr_to_ctrl_alt: bool) -> Self
    where
        I: IntoIterator<Item = (&'a String, &'a WindowsKeyMapping)>,
    {
        let mut state = Self {
            is_us_standard,
            map_alt_gr_to_ctrl_alt,
            code_info: HashMap::new(),
            scan_code_to_key_code: HashMap::new(),
            key_code_to_label: HashMap::new(),
            key_code_exists: HashSet::new(),
        };
        state.key_code_to_label.insert(KeyCode::Unknown, KeyCodeUtils::to_string(KeyCode::Unknown).map(str::to_string));

        for scan_code in (ScanCode::None.0..ScanCode::MaxValue.0).map(ScanCode) {
            let immutable = immutable_key_code(scan_code);
            if immutable != KeyCode::DependsOnKbLayout {
                state.scan_code_to_key_code.insert(scan_code, immutable);
                state.key_code_to_label.insert(immutable, KeyCodeUtils::to_string(immutable).map(str::to_string));
                state.key_code_exists.insert(immutable);
            }
        }

        let mut produces_letter = [false; 26];
        let mut produces_letters = false;

        for (str_code, raw_mapping) in raw_mappings {
            let scan_code = ScanCodeUtils::to_enum(str_code);
            if scan_code == ScanCode::None {
                // Unknown scanCode in mapping.
                continue;
            }

            let immutable = immutable_key_code(scan_code);
            let key_code = native_key_code(&raw_mapping.vkey);
            if immutable != KeyCode::DependsOnKbLayout {
                if key_code == KeyCode::Unknown || immutable == key_code {
                    continue;
                }
                if scan_code != ScanCode::NumpadComma {
                    // Looks like ScanCode.NumpadComma doesn't always map to KeyCode.NUMPAD_SEPARATOR
                    // e.g. on POR - PTB
                    continue;
                }
            }

            let value = &raw_mapping.value;
            state.code_info.insert(
                scan_code,
                ScanCodeMapping {
                    scan_code,
                    key_code,
                    value: value.clone(),
                    with_shift: raw_mapping.with_shift.clone(),
                    with_alt_gr: raw_mapping.with_alt_gr.clone(),
                    with_shift_alt_gr: raw_mapping.with_shift_alt_gr.clone(),
                },
            );
            state.scan_code_to_key_code.insert(scan_code, key_code);

            if key_code == KeyCode::Unknown {
                continue;
            }
            state.key_code_exists.insert(key_code);

            let label = match value.encode_utf16().count() {
                // This key does not produce strings
                0 => None,
                // This key produces a letter representable with multiple UTF-16 code units.
                1 => match value.chars().next() {
                    Some(letter @ 'a'..='z') => {
                        let upper = letter.to_ascii_uppercase();
                        produces_letter[(upper as u8 - b'A') as usize] = true;
                        produces_letters = true;
                        Some(upper.to_string())
                    }
                    Some('A'..='Z') => {
                        produces_letter[(value.as_bytes()[0] - b'A') as usize] = true;
                        produces_letters = true;
                        Some(value.clone())
                    }
                    _ => Some(value.clone()),
                },
                _ => Some(value.clone()),
            };
            state.key_code_to_label.insert(key_code, label);
        }

        // Handle keyboard layouts where latin characters are not produced e.g. Cyrillic
        for (index, key_code) in LETTER_KEY_CODES.into_iter().enumerate() {
            if !produces_letter[index] {
                let letter = char::from(b'A' + index as u8);
                state.key_code_to_label.insert(key_code, Some(letter.to_string()));
            }
        }

        if !produces_letters {
            // Since this keyboard layout produces no latin letters at all, most of the UI will use the
            // US kb layout equivalent for UI labels, so also try to render other keys with the US labels
            // for consistency...
            for (key_code, char) in US_PUNCTUATION_LABELS {
                state.key_code_to_label.insert(key_code, Some(char.to_string()));
            }
        }

        state
    }

    pub fn get_ui_label_for_key_code(&self, key_code: KeyCode) -> Option<String> {
        self.get_label_for_key_code(key_code)
    }

    pub fn get_aria_label_for_key_code(&self, key_code: KeyCode) -> Option<String> {
        self.get_label_for_key_code(key_code)
    }

    pub fn get_user_settings_label_for_key_code(&self, key_code: KeyCode) -> Option<&'static str> {
        if self.is_us_standard {
            return KeyCodeUtils::to_user_settings_us(key_code);
        }
        KeyCodeUtils::to_user_settings_general(key_code)
    }

    pub fn get_electron_accelerator_for_key_binding(&self, chord: &KeyCodeChord) -> Option<&'static str> {
        KeyCodeUtils::to_electron_accelerator(chord.key_code)
    }

    /// `_getLabelForKeyCode`: `this._keyCodeToLabel[keyCode] || KeyCodeUtils.toString(KeyCode.Unknown)`.
    fn get_label_for_key_code(&self, key_code: KeyCode) -> Option<String> {
        self.key_code_to_label
            .get(&key_code)
            .cloned()
            .flatten()
            .filter(|label| !label.is_empty())
            .or_else(|| KeyCodeUtils::to_string(KeyCode::Unknown).map(str::to_string))
    }

    /// `_resolveChord`.
    fn resolve_chord(&self, chord: &Chord) -> Option<KeyCodeChord> {
        match chord {
            Chord::KeyCode(chord) => {
                if !self.key_code_exists.contains(&chord.key_code) {
                    return None;
                }
                Some(*chord)
            }
            Chord::ScanCode(chord) => {
                let key_code = self.scan_code_to_key_code.get(&chord.scan_code).copied().unwrap_or(KeyCode::Unknown);
                if key_code == KeyCode::Unknown || !self.key_code_exists.contains(&key_code) {
                    return None;
                }
                Some(KeyCodeChord::new(chord.ctrl_key, chord.shift_key, chord.alt_key, chord.meta_key, key_code))
            }
        }
    }
}

/// `_leftPad`: o comprimento conta unidades UTF-16, como o `str.length` do original.
pub(super) fn left_pad(str: Option<&str>, cnt: usize) -> String {
    let str = str.unwrap_or("null");
    let length = str.encode_utf16().count();
    format!("{}{str}", " ".repeat(cnt.saturating_sub(length)))
}

const DUMP_SEPARATOR: &str =
    "-----------------------------------------------------------------------------------------------------------------------------------------";
const DUMP_HEADER: &str =
    "|       HW Code combination      |  Key  |    KeyCode combination    |          UI label         |        User settings       | WYSIWYG |";

/// `WindowsKeyboardMapper`.
pub struct WindowsKeyboardMapper {
    state: Rc<WindowsKeyboardMapperState>,
}

impl WindowsKeyboardMapper {
    pub fn new<'a, I>(is_us_standard: bool, raw_mappings: I, map_alt_gr_to_ctrl_alt: bool) -> Self
    where
        I: IntoIterator<Item = (&'a String, &'a WindowsKeyMapping)>,
    {
        Self { state: Rc::new(WindowsKeyboardMapperState::new(is_us_standard, raw_mappings, map_alt_gr_to_ctrl_alt)) }
    }

    fn native_resolved_keybinding(&self, chords: Vec<KeyCodeChord>) -> WindowsNativeResolvedKeybinding {
        WindowsNativeResolvedKeybinding::new(Rc::clone(&self.state), chords)
    }

    /// `resolveKeyboardEvent`, com o tipo concreto do resultado.
    pub fn resolve_keyboard_event_native(&self, keyboard_event: &KeyboardEvent) -> WindowsNativeResolvedKeybinding {
        let alt_gr = self.state.map_alt_gr_to_ctrl_alt && keyboard_event.alt_graph_key;
        let ctrl_key = keyboard_event.ctrl_key || alt_gr;
        let alt_key = keyboard_event.alt_key || alt_gr;
        let chord = KeyCodeChord::new(ctrl_key, keyboard_event.shift_key, alt_key, keyboard_event.meta_key, keyboard_event.key_code);
        self.native_resolved_keybinding(vec![chord])
    }
}

impl Deref for WindowsKeyboardMapper {
    type Target = WindowsKeyboardMapperState;

    fn deref(&self) -> &WindowsKeyboardMapperState {
        &self.state
    }
}

impl IKeyboardMapper for WindowsKeyboardMapper {
    fn dump_debug_info(&self) -> String {
        let mut result: Vec<String> = Vec::new();

        let immutable_samples = [ScanCode::ArrowUp, ScanCode::Numpad0];

        let mut cnt = 0;
        result.push(DUMP_SEPARATOR.to_string());
        for scan_code in (ScanCode::None.0..ScanCode::MaxValue.0).map(ScanCode) {
            if immutable_key_code(scan_code) != KeyCode::DependsOnKbLayout && !immutable_samples.contains(&scan_code) {
                continue;
            }

            if cnt % 6 == 0 {
                result.push(DUMP_HEADER.to_string());
                result.push(DUMP_SEPARATOR.to_string());
            }
            cnt += 1;

            let mapping = self.state.code_info.get(&scan_code);
            let str_code = ScanCodeUtils::to_string(scan_code);

            for bits in [0b000_u8, 0b010, 0b101, 0b111] {
                let ctrl_key = bits & 0b001 != 0;
                let shift_key = bits & 0b010 != 0;
                let alt_key = bits & 0b100 != 0;
                let scan_code_chord = ScanCodeChord::new(ctrl_key, shift_key, alt_key, false, scan_code);
                let key_code_chord = self.state.resolve_chord(&Chord::ScanCode(scan_code_chord));
                let str_key_code = key_code_chord.and_then(|chord| KeyCodeUtils::to_string(chord.key_code));
                let resolved_kb = key_code_chord.map(|chord| self.native_resolved_keybinding(vec![chord]));

                let modifiers = format!(
                    "{}{}{}",
                    if ctrl_key { "Ctrl+" } else { "" },
                    if shift_key { "Shift+" } else { "" },
                    if alt_key { "Alt+" } else { "" },
                );
                let out_scan_code = format!("{modifiers}{str_code}");
                let aria_label = resolved_kb.as_ref().and_then(|kb| kb.get_aria_label());
                let out_ui_label = aria_label.filter(|label| !label.is_empty()).map(|label| label.replacen("Control+", "Ctrl+", 1));
                let out_user_settings = resolved_kb.as_ref().and_then(|kb| kb.get_user_settings_label());
                let out_key = WindowsNativeResolvedKeybinding::get_produced_char(&scan_code_chord, mapping);
                let out_kb = str_key_code.filter(|key| !key.is_empty()).map(|key| format!("{modifiers}{key}"));
                let is_wysiwyg = resolved_kb.as_ref().is_some_and(|kb| kb.is_wysiwyg());
                let out_wysiwyg = if is_wysiwyg { "       " } else { "   NO  " };
                result.push(format!(
                    "| {} | {} | {} | {} |  {} | {} |",
                    left_pad(Some(out_scan_code.as_str()), 30),
                    out_key,
                    left_pad(out_kb.as_deref(), 25),
                    left_pad(out_ui_label.as_deref(), 25),
                    left_pad(out_user_settings.as_deref(), 25),
                    out_wysiwyg,
                ));
            }
            result.push(DUMP_SEPARATOR.to_string());
        }

        result.join("\n")
    }

    fn resolve_keyboard_event(&self, keyboard_event: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding> {
        Rc::new(self.resolve_keyboard_event_native(keyboard_event))
    }

    fn resolve_keybinding(&self, keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>> {
        let chords: Vec<KeyCodeChord> =
            to_empty_array_if_contains_none(keybinding.chords.iter().map(|chord| self.state.resolve_chord(chord)).collect());
        if !chords.is_empty() {
            return vec![Rc::new(self.native_resolved_keybinding(chords))];
        }
        Vec::new()
    }
}
