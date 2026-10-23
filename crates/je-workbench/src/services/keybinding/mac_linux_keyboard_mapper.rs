//! Porte de `src/vs/workbench/services/keybinding/common/macLinuxKeyboardMapper.ts`.
//!
//! Notas de tradução:
//! - `IMacLinuxKeyMapping` (união de `IMacKeyMapping` e `ILinuxKeyMapping` em
//!   `src/vs/platform/keyboardLayout/common/keyboardLayout.ts`, de onde o original só lê `value`, `withShift`,
//!   `withAltGr` e `withShiftAltGr`) vira [`MacLinuxKeyMapping`], com `From` para os dois formatos do je-platform.
//!   `IMacLinuxKeyboardMapping` é o iterador de pares `(&String, &MacLinuxKeyMapping)` que o construtor aceita, na ordem
//!   do `for ... in` do original.
//! - Os vetores esparsos do original (`_scanCodeToKeyCode`, `_keyCodeToScanCode`, `_codeInfo`) viram mapas por índice
//!   codificado e por [`ScanCode`]. Os vetores densos (`_scanCodeToLabel`, `_scanCodeToDispatch`) seguem vetores.
//! - Um caractere é uma unidade UTF-16 (`charCodeAt(0)`), por isso `u16`, como em `je_base::char_code`.
//! - `NativeResolvedKeybinding` mantém uma referência ao mapeador. Aqui o estado que os rótulos consultam fica em
//!   [`MacLinuxKeyboardMapperState`], compartilhado por `Rc`, e o tipo é o próprio [`BaseResolvedKeybinding`] com os
//!   rótulos do mapeador ([`MacLinuxNativeLabels`]), que já implementa `ResolvedKeybinding`.
//! - O `BaseResolvedKeybinding` exige `BaseChord`, e a regra do órfão impede implementá-lo para `ScanCodeChord`
//!   (tipo do je-base) fora do je-platform. [`NativeChord`] embrulha o acorde para isso.
//! - `CHAR_CODE_TO_KEY_CODE`, montado numa função anônima ao carregar o módulo, sai de um `OnceLock`.

use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::OnceLock;

use je_base::char_code as cc;
use je_base::key_codes::{
    KeyCode, KeyCodeUtils, ScanCode, ScanCodeUtils, immutable_key_code_to_code, is_modifier_key,
};
use je_base::keybindings::{
    Chord, KeyCodeChord, Keybinding, Modifiers, ResolvedKeybinding, ScanCodeChord, SingleModifierChord,
};
use je_base::platform::OperatingSystem;
use je_platform::keybinding::base_resolved_keybinding::{BaseChord, BaseResolvedKeybinding, ChordLabels};
use je_platform::keybinding::keybinding::KeyboardEvent;
use je_platform::keyboard_layout::IKeyboardMapper;

use je_platform::keyboard_layout::MacLinuxKeyMapping;
use super::windows_keyboard_mapper::{immutable_key_code, left_pad};

/// O valor de `CHAR_CODE_TO_KEY_CODE[charCode]`: `{ keyCode, shiftKey }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharKeyCode {
    pub key_code: KeyCode,
    pub shift_key: bool,
}

/// `CHAR_CODE_TO_KEY_CODE`: de caractere para o código da tecla.
/// Contém entradas como `'/' => { keyCode: KeyCode.US_SLASH, shiftKey: false }` e `'?' => { ..., shiftKey: true }`.
fn char_code_to_key_code_table() -> &'static [Option<CharKeyCode>] {
    static TABLE: OnceLock<Vec<Option<CharKeyCode>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table: Vec<Option<CharKeyCode>> = Vec::new();
        let mut define = |char_code: u16, key_code: KeyCode, shift_key: bool| {
            if table.len() <= char_code as usize {
                table.resize(char_code as usize + 1, None);
            }
            table[char_code as usize] = Some(CharKeyCode { key_code, shift_key });
        };

        for ch_code in cc::A..=cc::Z {
            define(ch_code, KeyCode(KeyCode::KeyA.0 + i32::from(ch_code - cc::A)), true);
        }
        for ch_code in cc::a..=cc::z {
            define(ch_code, KeyCode(KeyCode::KeyA.0 + i32::from(ch_code - cc::a)), false);
        }

        define(cc::Semicolon, KeyCode::Semicolon, false);
        define(cc::Colon, KeyCode::Semicolon, true);

        define(cc::Equals, KeyCode::Equal, false);
        define(cc::Plus, KeyCode::Equal, true);

        define(cc::Comma, KeyCode::Comma, false);
        define(cc::LessThan, KeyCode::Comma, true);

        define(cc::Dash, KeyCode::Minus, false);
        define(cc::Underline, KeyCode::Minus, true);

        define(cc::Period, KeyCode::Period, false);
        define(cc::GreaterThan, KeyCode::Period, true);

        define(cc::Slash, KeyCode::Slash, false);
        define(cc::QuestionMark, KeyCode::Slash, true);

        define(cc::BackTick, KeyCode::Backquote, false);
        define(cc::Tilde, KeyCode::Backquote, true);

        define(cc::OpenSquareBracket, KeyCode::BracketLeft, false);
        define(cc::OpenCurlyBrace, KeyCode::BracketLeft, true);

        define(cc::Backslash, KeyCode::Backslash, false);
        define(cc::Pipe, KeyCode::Backslash, true);

        define(cc::CloseSquareBracket, KeyCode::BracketRight, false);
        define(cc::CloseCurlyBrace, KeyCode::BracketRight, true);

        define(cc::SingleQuote, KeyCode::Quote, false);
        define(cc::DoubleQuote, KeyCode::Quote, true);

        table
    })
}

/// O acorde por `scanCode` com o `BaseChord` que o `BaseResolvedKeybinding` pede (ver as notas do módulo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeChord(pub ScanCodeChord);

impl Modifiers for NativeChord {
    fn ctrl_key(&self) -> bool {
        self.0.ctrl_key
    }
    fn shift_key(&self) -> bool {
        self.0.shift_key
    }
    fn alt_key(&self) -> bool {
        self.0.alt_key
    }
    fn meta_key(&self) -> bool {
        self.0.meta_key
    }
}

impl BaseChord for NativeChord {
    fn is_duplicate_modifier_case(&self) -> bool {
        self.0.is_duplicate_modifier_case()
    }
}

/// `undefined`, `null` e `''` do JavaScript são todos falsos.
fn is_falsy(text: &Option<String>) -> bool {
    text.as_deref().is_none_or(str::is_empty)
}

/// Os rótulos de um acorde por `scanCode` que consultam o mapeador (os métodos `_getLabel`... de `NativeResolvedKeybinding`).
#[derive(Clone)]
pub struct MacLinuxNativeLabels {
    mapper: Rc<MacLinuxKeyboardMapperState>,
}

impl ChordLabels<NativeChord> for MacLinuxNativeLabels {
    fn get_label(&self, _os: OperatingSystem, chord: &NativeChord) -> Option<String> {
        self.mapper.get_ui_label_for_scan_code_chord(Some(&chord.0))
    }

    fn get_aria_label(&self, _os: OperatingSystem, chord: &NativeChord) -> Option<String> {
        self.mapper.get_aria_label_for_scan_code_chord(Some(&chord.0))
    }

    fn get_electron_accelerator(&self, _os: OperatingSystem, chord: &NativeChord) -> Option<String> {
        self.mapper.get_electron_accelerator_label_for_scan_code_chord(Some(&chord.0))
    }

    fn get_user_settings_label(&self, _os: OperatingSystem, chord: &NativeChord) -> Option<String> {
        self.mapper.get_user_settings_label_for_scan_code_chord(Some(&chord.0))
    }

    fn is_wysiwyg(&self, chord: &NativeChord) -> bool {
        let binding = &chord.0;
        if immutable_key_code(binding.scan_code) != KeyCode::DependsOnKbLayout {
            return true;
        }
        let a = self.mapper.get_aria_label_for_scan_code_chord(Some(binding));
        let b = self.mapper.get_user_settings_label_for_scan_code_chord(Some(binding));

        if is_falsy(&a) && is_falsy(&b) {
            return true;
        }
        if is_falsy(&a) || is_falsy(&b) {
            return false;
        }
        a.map(|a| a.to_lowercase()) == b.map(|b| b.to_lowercase())
    }

    fn get_chord_dispatch(&self, chord: &NativeChord) -> Option<String> {
        self.mapper.get_dispatch_str_for_scan_code_chord(&chord.0)
    }

    fn get_single_modifier_chord_dispatch(&self, chord: &NativeChord) -> Option<SingleModifierChord> {
        let chord = &chord.0;
        if (chord.scan_code == ScanCode::ControlLeft || chord.scan_code == ScanCode::ControlRight)
            && !chord.shift_key
            && !chord.alt_key
            && !chord.meta_key
        {
            return Some(SingleModifierChord::Ctrl);
        }
        if (chord.scan_code == ScanCode::AltLeft || chord.scan_code == ScanCode::AltRight)
            && !chord.ctrl_key
            && !chord.shift_key
            && !chord.meta_key
        {
            return Some(SingleModifierChord::Alt);
        }
        if (chord.scan_code == ScanCode::ShiftLeft || chord.scan_code == ScanCode::ShiftRight)
            && !chord.ctrl_key
            && !chord.alt_key
            && !chord.meta_key
        {
            return Some(SingleModifierChord::Shift);
        }
        if (chord.scan_code == ScanCode::MetaLeft || chord.scan_code == ScanCode::MetaRight)
            && !chord.ctrl_key
            && !chord.shift_key
            && !chord.alt_key
        {
            return Some(SingleModifierChord::Meta);
        }
        None
    }
}

/// `NativeResolvedKeybinding`: o `BaseResolvedKeybinding` com os rótulos do mapeador. Construa com
/// [`native_resolved_keybinding`].
pub type NativeResolvedKeybinding = BaseResolvedKeybinding<NativeChord, MacLinuxNativeLabels>;

/// `new NativeResolvedKeybinding(mapper, os, chords)`.
pub fn native_resolved_keybinding(
    mapper: Rc<MacLinuxKeyboardMapperState>,
    os: OperatingSystem,
    chords: Vec<ScanCodeChord>,
) -> NativeResolvedKeybinding {
    BaseResolvedKeybinding::new(os, chords.into_iter().map(NativeChord).collect(), MacLinuxNativeLabels { mapper })
}

/// `IScanCodeMapping`.
struct ScanCodeMapping {
    scan_code: ScanCode,
    value: u16,
    with_shift: u16,
    with_alt_gr: u16,
    with_shift_alt_gr: u16,
}

/// `ScanCodeCombo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanCodeCombo {
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub scan_code: ScanCode,
}

impl fmt::Display for ScanCodeCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}{}",
            if self.ctrl_key { "Ctrl+" } else { "" },
            if self.shift_key { "Shift+" } else { "" },
            if self.alt_key { "Alt+" } else { "" },
            ScanCodeUtils::to_string(self.scan_code),
        )
    }
}

impl ScanCodeCombo {
    pub fn new(ctrl_key: bool, shift_key: bool, alt_key: bool, scan_code: ScanCode) -> Self {
        Self { ctrl_key, shift_key, alt_key, scan_code }
    }

    pub fn equals(&self, other: &ScanCodeCombo) -> bool {
        self == other
    }

    /// `getProducedCharCode`: o texto que a tecla produz com esses modificadores; `''` quando não há mapeamento.
    fn get_produced_char_code<'a>(&self, mapping: Option<&'a MacLinuxKeyMapping>) -> &'a str {
        let Some(mapping) = mapping else { return "" };
        if self.ctrl_key && self.shift_key && self.alt_key {
            return &mapping.with_shift_alt_gr;
        }
        if self.ctrl_key && self.alt_key {
            return &mapping.with_alt_gr;
        }
        if self.shift_key {
            return &mapping.with_shift;
        }
        &mapping.value
    }

    pub fn get_produced_char(&self, mapping: Option<&MacLinuxKeyMapping>) -> String {
        let char_code = MacLinuxKeyboardMapper::get_char_code(self.get_produced_char_code(mapping));
        if char_code == 0 {
            return " --- ".to_string();
        }
        if (cc::U_Combining_Grave_Accent..=cc::U_Combining_Latin_Small_Letter_X).contains(&char_code) {
            // combining
            return format!("U+{char_code:x}");
        }
        format!("  {}  ", String::from_utf16_lossy(&[char_code]))
    }
}

/// `KeyCodeCombo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyCodeCombo {
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub key_code: KeyCode,
}

impl fmt::Display for KeyCodeCombo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}{}{}",
            if self.ctrl_key { "Ctrl+" } else { "" },
            if self.shift_key { "Shift+" } else { "" },
            if self.alt_key { "Alt+" } else { "" },
            KeyCodeUtils::to_string(self.key_code).unwrap_or("undefined"),
        )
    }
}

impl KeyCodeCombo {
    pub fn new(ctrl_key: bool, shift_key: bool, alt_key: bool, key_code: KeyCode) -> Self {
        Self { ctrl_key, shift_key, alt_key, key_code }
    }
}

/// Os modificadores `(ctrl, shift, alt)` de uma combinação, cada um 0 ou 1, como os argumentos do original.
type Mods = (u8, u8, u8);

/// `_encode`: os três modificadores nos bits 0 a 2 e o código principal a partir do bit 3.
fn encode(ctrl_key: bool, shift_key: bool, alt_key: bool, principal: i32) -> u32 {
    u32::from(ctrl_key) | (u32::from(shift_key) << 1) | (u32::from(alt_key) << 2) | ((principal as u32) << 3)
}

/// `ScanCodeKeyCodeMapper`.
#[derive(Default)]
struct ScanCodeKeyCodeMapper {
    /// ScanCode combination => KeyCode combination.
    /// Only covers relevant modifiers ctrl, shift, alt (since meta does not influence the mappings).
    scan_code_to_key_code: HashMap<u32, Vec<u32>>,
    /// inverse of `scan_code_to_key_code`.
    /// KeyCode combination => ScanCode combination.
    /// Only covers relevant modifiers ctrl, shift, alt (since meta does not influence the mappings).
    key_code_to_scan_code: HashMap<u32, Vec<u32>>,
}

impl ScanCodeKeyCodeMapper {
    fn registration_complete(&mut self) {
        // IntlHash and IntlBackslash are rare keys, so ensure they don't end up being the preferred...
        self.move_to_end(ScanCode::IntlHash);
        self.move_to_end(ScanCode::IntlBackslash);
    }

    fn move_to_end(&mut self, scan_code: ScanCode) {
        for modifier in 0..8_u32 {
            let Some(encoded_key_code_combos) = self.scan_code_to_key_code.get(&(((scan_code.0 as u32) << 3) + modifier)) else {
                continue;
            };
            for encoded_key_code_combo in encoded_key_code_combos {
                let Some(encoded_scan_code_combos) = self.key_code_to_scan_code.get_mut(encoded_key_code_combo) else {
                    continue;
                };
                let len = encoded_scan_code_combos.len();
                if len == 1 {
                    continue;
                }
                for j in 0..len {
                    let entry = encoded_scan_code_combos[j];
                    let entry_scan_code = entry >> 3;
                    if entry_scan_code == scan_code.0 as u32 {
                        // Move this entry to the end
                        for k in (j + 1)..len {
                            encoded_scan_code_combos[k - 1] = encoded_scan_code_combos[k];
                        }
                        encoded_scan_code_combos[len - 1] = entry;
                    }
                }
            }
        }
    }

    fn register_if_unknown(&mut self, scan_code_combo: ScanCodeCombo, key_code_combo: KeyCodeCombo) {
        if key_code_combo.key_code == KeyCode::Unknown {
            return;
        }
        let scan_code_combo_encoded = Self::encode_scan_code_combo(&scan_code_combo);
        let key_code_combo_encoded = Self::encode_key_code_combo(&key_code_combo);

        let key_code_is_digit = key_code_combo.key_code >= KeyCode::Digit0 && key_code_combo.key_code <= KeyCode::Digit9;
        let key_code_is_letter = key_code_combo.key_code >= KeyCode::KeyA && key_code_combo.key_code <= KeyCode::KeyZ;

        let existing_key_code_combos = self.scan_code_to_key_code.get(&scan_code_combo_encoded);

        // Allow a scan code to map to multiple key codes if it is a digit or a letter key code
        if key_code_is_digit || key_code_is_letter {
            // Only check that we don't insert the same entry twice
            if existing_key_code_combos.is_some_and(|combos| combos.contains(&key_code_combo_encoded)) {
                // avoid duplicates
                return;
            }
        } else if existing_key_code_combos.is_some_and(|combos| !combos.is_empty()) {
            // Don't allow multiples
            return;
        }

        self.scan_code_to_key_code.entry(scan_code_combo_encoded).or_default().insert(0, key_code_combo_encoded);
        self.key_code_to_scan_code.entry(key_code_combo_encoded).or_default().insert(0, scan_code_combo_encoded);
    }

    /// `_registerIfUnknown` do construtor: modificadores do hardware, `ScanCode`, modificadores da tecla lógica, `KeyCode`.
    fn register_mods(&mut self, hw: Mods, scan_code: ScanCode, kb: Mods, key_code: KeyCode) {
        self.register_if_unknown(
            ScanCodeCombo::new(hw.0 != 0, hw.1 != 0, hw.2 != 0, scan_code),
            KeyCodeCombo::new(kb.0 != 0, kb.1 != 0, kb.2 != 0, key_code),
        );
    }

    /// `_registerAllCombos` do construtor: registra do modificador dado em diante, o mesmo dos dois lados.
    fn register_all_combos(&mut self, start: Mods, scan_code: ScanCode, key_code: KeyCode) {
        for ctrl_key in start.0..=1 {
            for shift_key in start.1..=1 {
                for alt_key in start.2..=1 {
                    self.register_mods((ctrl_key, shift_key, alt_key), scan_code, (ctrl_key, shift_key, alt_key), key_code);
                }
            }
        }
    }

    fn lookup_key_code_combo(&self, key_code_combo: &KeyCodeCombo) -> Vec<ScanCodeCombo> {
        let key_code_combo_encoded = Self::encode_key_code_combo(key_code_combo);
        let Some(scan_code_combos_encoded) = self.key_code_to_scan_code.get(&key_code_combo_encoded) else {
            return Vec::new();
        };

        scan_code_combos_encoded
            .iter()
            .map(|scan_code_combo_encoded| {
                let ctrl_key = scan_code_combo_encoded & 0b001 != 0;
                let shift_key = scan_code_combo_encoded & 0b010 != 0;
                let alt_key = scan_code_combo_encoded & 0b100 != 0;
                let scan_code = ScanCode((scan_code_combo_encoded >> 3) as i32);
                ScanCodeCombo::new(ctrl_key, shift_key, alt_key, scan_code)
            })
            .collect()
    }

    fn lookup_scan_code_combo(&self, scan_code_combo: &ScanCodeCombo) -> Vec<KeyCodeCombo> {
        let scan_code_combo_encoded = Self::encode_scan_code_combo(scan_code_combo);
        let Some(key_code_combos_encoded) = self.scan_code_to_key_code.get(&scan_code_combo_encoded) else {
            return Vec::new();
        };

        key_code_combos_encoded
            .iter()
            .map(|key_code_combo_encoded| {
                let ctrl_key = key_code_combo_encoded & 0b001 != 0;
                let shift_key = key_code_combo_encoded & 0b010 != 0;
                let alt_key = key_code_combo_encoded & 0b100 != 0;
                let key_code = KeyCode((key_code_combo_encoded >> 3) as i32);
                KeyCodeCombo::new(ctrl_key, shift_key, alt_key, key_code)
            })
            .collect()
    }

    fn guess_stable_key_code(&self, scan_code: ScanCode) -> KeyCode {
        if scan_code >= ScanCode::Digit1 && scan_code <= ScanCode::Digit0 {
            // digits are ok
            match scan_code {
                ScanCode::Digit1 => return KeyCode::Digit1,
                ScanCode::Digit2 => return KeyCode::Digit2,
                ScanCode::Digit3 => return KeyCode::Digit3,
                ScanCode::Digit4 => return KeyCode::Digit4,
                ScanCode::Digit5 => return KeyCode::Digit5,
                ScanCode::Digit6 => return KeyCode::Digit6,
                ScanCode::Digit7 => return KeyCode::Digit7,
                ScanCode::Digit8 => return KeyCode::Digit8,
                ScanCode::Digit9 => return KeyCode::Digit9,
                ScanCode::Digit0 => return KeyCode::Digit0,
                _ => {}
            }
        }

        // Lookup the scanCode with and without shift and see if the keyCode is stable
        let key_code_combos1 = self.lookup_scan_code_combo(&ScanCodeCombo::new(false, false, false, scan_code));
        let key_code_combos2 = self.lookup_scan_code_combo(&ScanCodeCombo::new(false, true, false, scan_code));
        if let ([first], [second]) = (key_code_combos1.as_slice(), key_code_combos2.as_slice()) {
            if first.key_code == second.key_code && first.shift_key != second.shift_key {
                // This looks like a stable mapping
                return first.key_code;
            }
        }

        KeyCode::DependsOnKbLayout
    }

    fn encode_scan_code_combo(scan_code_combo: &ScanCodeCombo) -> u32 {
        encode(scan_code_combo.ctrl_key, scan_code_combo.shift_key, scan_code_combo.alt_key, scan_code_combo.scan_code.0)
    }

    fn encode_key_code_combo(key_code_combo: &KeyCodeCombo) -> u32 {
        encode(key_code_combo.ctrl_key, key_code_combo.shift_key, key_code_combo.alt_key, key_code_combo.key_code.0)
    }
}

/// Escreve `value` em `vec[scan_code]`, estendendo com `None` como o vetor esparso do original.
fn put(vec: &mut Vec<Option<String>>, scan_code: ScanCode, value: Option<String>) {
    let index = scan_code.0 as usize;
    if vec.len() <= index {
        vec.resize(index + 1, None);
    }
    vec[index] = value;
}

/// `vec[scanCode]`: `None` também para um índice que o vetor não tem (o `undefined` do original).
fn get(vec: &[Option<String>], scan_code: ScanCode) -> Option<String> {
    usize::try_from(scan_code.0).ok().and_then(|index| vec.get(index)).cloned().flatten()
}

/// O que o `MacLinuxKeyboardMapper` guarda depois de ler o mapa de teclas: tudo menos a identidade do mapeador.
/// Os métodos públicos do original (`getUILabelForScanCodeChord`...) são daqui, e o mapeador os expõe por `Deref`.
pub struct MacLinuxKeyboardMapperState {
    is_us_standard: bool,
    map_alt_gr_to_ctrl_alt: bool,
    os: OperatingSystem,
    /// used only for debug purposes.
    code_info: HashMap<ScanCode, MacLinuxKeyMapping>,
    /// Maps ScanCode combos <-> KeyCode combos.
    scan_code_key_code_mapper: ScanCodeKeyCodeMapper,
    /// UI label for a ScanCode.
    scan_code_to_label: Vec<Option<String>>,
    /// Dispatching string for a ScanCode.
    scan_code_to_dispatch: Vec<Option<String>>,
}

impl MacLinuxKeyboardMapperState {
    fn new<'a, I>(is_us_standard: bool, raw_mappings: I, map_alt_gr_to_ctrl_alt: bool, os: OperatingSystem) -> Self
    where
        I: IntoIterator<Item = (&'a String, &'a MacLinuxKeyMapping)>,
    {
        let raw_mappings: Vec<(&String, &MacLinuxKeyMapping)> = raw_mappings.into_iter().collect();
        let mut code_info: HashMap<ScanCode, MacLinuxKeyMapping> = HashMap::new();
        let mut mapper = ScanCodeKeyCodeMapper::default();
        let mut scan_code_to_label: Vec<Option<String>> = vec![None; ScanCode::MaxValue.0 as usize];
        let mut scan_code_to_dispatch: Vec<Option<String>> = vec![None; ScanCode::MaxValue.0 as usize];

        // Handle immutable mappings
        for scan_code in (ScanCode::None.0..ScanCode::MaxValue.0).map(ScanCode) {
            let key_code = immutable_key_code(scan_code);
            if key_code != KeyCode::DependsOnKbLayout {
                mapper.register_all_combos((0, 0, 0), scan_code, key_code);
                put(&mut scan_code_to_label, scan_code, KeyCodeUtils::to_string(key_code).map(str::to_string));

                if key_code == KeyCode::Unknown || is_modifier_key(key_code) {
                    // cannot dispatch on this ScanCode
                    put(&mut scan_code_to_dispatch, scan_code, None);
                } else {
                    put(&mut scan_code_to_dispatch, scan_code, Some(format!("[{}]", ScanCodeUtils::to_string(scan_code))));
                }
            }
        }

        // Try to identify keyboard layouts where characters A-Z are missing
        // and forcibly map them to their corresponding scan codes if that is the case
        let mut missing_latin_letters_override: HashMap<String, MacLinuxKeyMapping> = HashMap::new();

        {
            let mut produces_latin_letter = [false; 128];
            for (str_scan_code, raw_mapping) in &raw_mappings {
                let scan_code = ScanCodeUtils::to_enum(str_scan_code);
                if scan_code == ScanCode::None {
                    continue;
                }
                if immutable_key_code(scan_code) != KeyCode::DependsOnKbLayout {
                    continue;
                }

                let value = MacLinuxKeyboardMapper::get_char_code(&raw_mapping.value);

                if (cc::a..=cc::z).contains(&value) {
                    let upper_case_value = cc::A + (value - cc::a);
                    produces_latin_letter[usize::from(upper_case_value)] = true;
                }
            }

            // Ensure letters are mapped
            for offset in 0..26_u16 {
                if !produces_latin_letter[usize::from(cc::A + offset)] {
                    let scan_code = ScanCode(ScanCode::KeyA.0 + i32::from(offset));
                    let lower = char::from((cc::a + offset) as u8).to_string();
                    let upper = char::from((cc::A + offset) as u8).to_string();
                    missing_latin_letters_override
                        .insert(ScanCodeUtils::to_string(scan_code).to_string(), MacLinuxKeyMapping::new(&lower, &upper, "", ""));
                }
            }
        }

        let mut mappings: Vec<ScanCodeMapping> = Vec::new();
        for (str_scan_code, raw) in &raw_mappings {
            let scan_code = ScanCodeUtils::to_enum(str_scan_code);
            if scan_code == ScanCode::None {
                continue;
            }
            if immutable_key_code(scan_code) != KeyCode::DependsOnKbLayout {
                continue;
            }

            code_info.insert(scan_code, (*raw).clone());

            let raw_mapping = missing_latin_letters_override.get(str_scan_code.as_str()).unwrap_or(*raw);
            let value = MacLinuxKeyboardMapper::get_char_code(&raw_mapping.value);
            let with_shift = MacLinuxKeyboardMapper::get_char_code(&raw_mapping.with_shift);
            let with_alt_gr = MacLinuxKeyboardMapper::get_char_code(&raw_mapping.with_alt_gr);
            let with_shift_alt_gr = MacLinuxKeyboardMapper::get_char_code(&raw_mapping.with_shift_alt_gr);

            mappings.push(ScanCodeMapping { scan_code, value, with_shift, with_alt_gr, with_shift_alt_gr });

            put(&mut scan_code_to_dispatch, scan_code, Some(format!("[{}]", ScanCodeUtils::to_string(scan_code))));

            let label = if (cc::a..=cc::z).contains(&value) {
                let upper_case_value = cc::A + (value - cc::a);
                Some(String::from_utf16_lossy(&[upper_case_value]))
            } else if value != 0 {
                // Inclui as letras de A a Z, que o original trata num ramo próprio de resultado igual.
                Some(String::from_utf16_lossy(&[value]))
            } else {
                None
            };
            put(&mut scan_code_to_label, scan_code, label);
        }

        // Handle all `withShiftAltGr` entries
        for mapping in mappings.iter().rev() {
            let scan_code = mapping.scan_code;
            let with_shift_alt_gr = mapping.with_shift_alt_gr;
            if with_shift_alt_gr == mapping.with_alt_gr || with_shift_alt_gr == mapping.with_shift || with_shift_alt_gr == mapping.value {
                // handled below
                continue;
            }
            let Some(kb) = MacLinuxKeyboardMapper::char_code_to_kb(with_shift_alt_gr) else { continue };
            let key_code = kb.key_code;

            if kb.shift_key {
                // Ctrl+Shift+Alt+ScanCode => Shift+KeyCode
                mapper.register_mods((1, 1, 1), scan_code, (0, 1, 0), key_code); //       Ctrl+Alt+ScanCode =>          Shift+KeyCode
            } else {
                // Ctrl+Shift+Alt+ScanCode => KeyCode
                mapper.register_mods((1, 1, 1), scan_code, (0, 0, 0), key_code); //       Ctrl+Alt+ScanCode =>                KeyCode
            }
        }
        // Handle all `withAltGr` entries
        for mapping in mappings.iter().rev() {
            let scan_code = mapping.scan_code;
            let with_alt_gr = mapping.with_alt_gr;
            if with_alt_gr == mapping.with_shift || with_alt_gr == mapping.value {
                // handled below
                continue;
            }
            let Some(kb) = MacLinuxKeyboardMapper::char_code_to_kb(with_alt_gr) else { continue };
            let key_code = kb.key_code;

            if kb.shift_key {
                // Ctrl+Alt+ScanCode => Shift+KeyCode
                mapper.register_mods((1, 0, 1), scan_code, (0, 1, 0), key_code); //       Ctrl+Alt+ScanCode =>          Shift+KeyCode
            } else {
                // Ctrl+Alt+ScanCode => KeyCode
                mapper.register_mods((1, 0, 1), scan_code, (0, 0, 0), key_code); //       Ctrl+Alt+ScanCode =>                KeyCode
            }
        }
        // Handle all `withShift` entries
        for mapping in mappings.iter().rev() {
            let scan_code = mapping.scan_code;
            let with_shift = mapping.with_shift;
            if with_shift == mapping.value {
                // handled below
                continue;
            }
            let Some(kb) = MacLinuxKeyboardMapper::char_code_to_kb(with_shift) else { continue };
            let key_code = kb.key_code;

            if kb.shift_key {
                // Shift+ScanCode => Shift+KeyCode
                mapper.register_mods((0, 1, 0), scan_code, (0, 1, 0), key_code); //          Shift+ScanCode =>          Shift+KeyCode
                mapper.register_mods((0, 1, 1), scan_code, (0, 1, 1), key_code); //      Shift+Alt+ScanCode =>      Shift+Alt+KeyCode
                mapper.register_mods((1, 1, 0), scan_code, (1, 1, 0), key_code); //     Ctrl+Shift+ScanCode =>     Ctrl+Shift+KeyCode
                mapper.register_mods((1, 1, 1), scan_code, (1, 1, 1), key_code); // Ctrl+Shift+Alt+ScanCode => Ctrl+Shift+Alt+KeyCode
            } else {
                // Shift+ScanCode => KeyCode
                mapper.register_mods((0, 1, 0), scan_code, (0, 0, 0), key_code); //          Shift+ScanCode =>                KeyCode
                mapper.register_mods((0, 1, 0), scan_code, (0, 1, 0), key_code); //          Shift+ScanCode =>          Shift+KeyCode
                mapper.register_mods((0, 1, 1), scan_code, (0, 0, 1), key_code); //      Shift+Alt+ScanCode =>            Alt+KeyCode
                mapper.register_mods((0, 1, 1), scan_code, (0, 1, 1), key_code); //      Shift+Alt+ScanCode =>      Shift+Alt+KeyCode
                mapper.register_mods((1, 1, 0), scan_code, (1, 0, 0), key_code); //     Ctrl+Shift+ScanCode =>           Ctrl+KeyCode
                mapper.register_mods((1, 1, 0), scan_code, (1, 1, 0), key_code); //     Ctrl+Shift+ScanCode =>     Ctrl+Shift+KeyCode
                mapper.register_mods((1, 1, 1), scan_code, (1, 0, 1), key_code); // Ctrl+Shift+Alt+ScanCode =>       Ctrl+Alt+KeyCode
                mapper.register_mods((1, 1, 1), scan_code, (1, 1, 1), key_code); // Ctrl+Shift+Alt+ScanCode => Ctrl+Shift+Alt+KeyCode
            }
        }
        // Handle all `value` entries
        for mapping in mappings.iter().rev() {
            let scan_code = mapping.scan_code;
            let Some(kb) = MacLinuxKeyboardMapper::char_code_to_kb(mapping.value) else { continue };
            let key_code = kb.key_code;

            if kb.shift_key {
                // ScanCode => Shift+KeyCode
                mapper.register_mods((0, 0, 0), scan_code, (0, 1, 0), key_code); //                ScanCode =>          Shift+KeyCode
                mapper.register_mods((0, 0, 1), scan_code, (0, 1, 1), key_code); //            Alt+ScanCode =>      Shift+Alt+KeyCode
                mapper.register_mods((1, 0, 0), scan_code, (1, 1, 0), key_code); //           Ctrl+ScanCode =>     Ctrl+Shift+KeyCode
                mapper.register_mods((1, 0, 1), scan_code, (1, 1, 1), key_code); //       Ctrl+Alt+ScanCode => Ctrl+Shift+Alt+KeyCode
            } else {
                // ScanCode => KeyCode
                mapper.register_mods((0, 0, 0), scan_code, (0, 0, 0), key_code); //                ScanCode =>                KeyCode
                mapper.register_mods((0, 0, 1), scan_code, (0, 0, 1), key_code); //            Alt+ScanCode =>            Alt+KeyCode
                mapper.register_mods((0, 1, 0), scan_code, (0, 1, 0), key_code); //          Shift+ScanCode =>          Shift+KeyCode
                mapper.register_mods((0, 1, 1), scan_code, (0, 1, 1), key_code); //      Shift+Alt+ScanCode =>      Shift+Alt+KeyCode
                mapper.register_mods((1, 0, 0), scan_code, (1, 0, 0), key_code); //           Ctrl+ScanCode =>           Ctrl+KeyCode
                mapper.register_mods((1, 0, 1), scan_code, (1, 0, 1), key_code); //       Ctrl+Alt+ScanCode =>       Ctrl+Alt+KeyCode
                mapper.register_mods((1, 1, 0), scan_code, (1, 1, 0), key_code); //     Ctrl+Shift+ScanCode =>     Ctrl+Shift+KeyCode
                mapper.register_mods((1, 1, 1), scan_code, (1, 1, 1), key_code); // Ctrl+Shift+Alt+ScanCode => Ctrl+Shift+Alt+KeyCode
            }
        }
        // Handle all left-over available digits
        for (scan_code, key_code) in [
            (ScanCode::Digit1, KeyCode::Digit1),
            (ScanCode::Digit2, KeyCode::Digit2),
            (ScanCode::Digit3, KeyCode::Digit3),
            (ScanCode::Digit4, KeyCode::Digit4),
            (ScanCode::Digit5, KeyCode::Digit5),
            (ScanCode::Digit6, KeyCode::Digit6),
            (ScanCode::Digit7, KeyCode::Digit7),
            (ScanCode::Digit8, KeyCode::Digit8),
            (ScanCode::Digit9, KeyCode::Digit9),
            (ScanCode::Digit0, KeyCode::Digit0),
        ] {
            mapper.register_all_combos((0, 0, 0), scan_code, key_code);
        }

        mapper.registration_complete();

        Self {
            is_us_standard,
            map_alt_gr_to_ctrl_alt,
            os,
            code_info,
            scan_code_key_code_mapper: mapper,
            scan_code_to_label,
            scan_code_to_dispatch,
        }
    }

    pub fn key_code_chord_to_scan_code_chord(&self, chord: &KeyCodeChord) -> Vec<ScanCodeChord> {
        // Avoid double Enter bindings (both ScanCode.NumpadEnter and ScanCode.Enter point to KeyCode.Enter)
        if chord.key_code == KeyCode::Enter {
            return vec![ScanCodeChord::new(chord.ctrl_key, chord.shift_key, chord.alt_key, chord.meta_key, ScanCode::Enter)];
        }

        let scan_code_combos = self
            .scan_code_key_code_mapper
            .lookup_key_code_combo(&KeyCodeCombo::new(chord.ctrl_key, chord.shift_key, chord.alt_key, chord.key_code));

        scan_code_combos
            .into_iter()
            .map(|combo| ScanCodeChord::new(combo.ctrl_key, combo.shift_key, combo.alt_key, chord.meta_key, combo.scan_code))
            .collect()
    }

    pub fn get_ui_label_for_scan_code_chord(&self, chord: Option<&ScanCodeChord>) -> Option<String> {
        let chord = chord?;
        if chord.is_duplicate_modifier_case() {
            return Some(String::new());
        }
        if self.os == OperatingSystem::Macintosh {
            match chord.scan_code {
                ScanCode::ArrowLeft => return Some("←".to_string()),
                ScanCode::ArrowUp => return Some("↑".to_string()),
                ScanCode::ArrowRight => return Some("→".to_string()),
                ScanCode::ArrowDown => return Some("↓".to_string()),
                _ => {}
            }
        }
        get(&self.scan_code_to_label, chord.scan_code)
    }

    pub fn get_aria_label_for_scan_code_chord(&self, chord: Option<&ScanCodeChord>) -> Option<String> {
        let chord = chord?;
        if chord.is_duplicate_modifier_case() {
            return Some(String::new());
        }
        get(&self.scan_code_to_label, chord.scan_code)
    }

    pub fn get_dispatch_str_for_scan_code_chord(&self, chord: &ScanCodeChord) -> Option<String> {
        let code_dispatch = get(&self.scan_code_to_dispatch, chord.scan_code).filter(|dispatch| !dispatch.is_empty())?;
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
        result.push_str(&code_dispatch);

        Some(result)
    }

    pub fn get_user_settings_label_for_scan_code_chord(&self, chord: Option<&ScanCodeChord>) -> Option<String> {
        let chord = chord?;
        if chord.is_duplicate_modifier_case() {
            return Some(String::new());
        }

        let immutable_key_code = immutable_key_code(chord.scan_code);
        if immutable_key_code != KeyCode::DependsOnKbLayout {
            return KeyCodeUtils::to_user_settings_us(immutable_key_code).map(str::to_lowercase);
        }

        // Check if this scanCode always maps to the same keyCode and back
        let constant_key_code = self.scan_code_key_code_mapper.guess_stable_key_code(chord.scan_code);
        if constant_key_code != KeyCode::DependsOnKbLayout {
            // Verify that this is a good key code that can be mapped back to the same scan code
            let reverse_chords = self.key_code_chord_to_scan_code_chord(&KeyCodeChord::new(
                chord.ctrl_key,
                chord.shift_key,
                chord.alt_key,
                chord.meta_key,
                constant_key_code,
            ));
            if reverse_chords.iter().any(|reverse_chord| reverse_chord.scan_code == chord.scan_code) {
                return KeyCodeUtils::to_user_settings_us(constant_key_code).map(str::to_lowercase);
            }
        }

        get(&self.scan_code_to_dispatch, chord.scan_code)
    }

    pub fn get_electron_accelerator_label_for_scan_code_chord(&self, chord: Option<&ScanCodeChord>) -> Option<String> {
        let chord = chord?;

        let immutable_key_code = immutable_key_code(chord.scan_code);
        if immutable_key_code != KeyCode::DependsOnKbLayout {
            return KeyCodeUtils::to_electron_accelerator(immutable_key_code).map(str::to_string);
        }

        // Check if this scanCode always maps to the same keyCode and back
        let constant_key_code = self.scan_code_key_code_mapper.guess_stable_key_code(chord.scan_code);

        if self.os == OperatingSystem::Linux && !self.is_us_standard {
            // [Electron Accelerators] On Linux, Electron does not handle correctly OEM keys.
            // when using a different keyboard layout than US Standard.
            // See https://github.com/microsoft/vscode/issues/23706
            // See https://github.com/microsoft/vscode/pull/134890#issuecomment-941671791
            let is_oem_key = constant_key_code == KeyCode::Semicolon
                || constant_key_code == KeyCode::Equal
                || constant_key_code == KeyCode::Comma
                || constant_key_code == KeyCode::Minus
                || constant_key_code == KeyCode::Period
                || constant_key_code == KeyCode::Slash
                || constant_key_code == KeyCode::Backquote
                || constant_key_code == KeyCode::BracketLeft
                || constant_key_code == KeyCode::Backslash
                || constant_key_code == KeyCode::BracketRight;

            if is_oem_key {
                return None;
            }
        }

        if constant_key_code != KeyCode::DependsOnKbLayout {
            return KeyCodeUtils::to_electron_accelerator(constant_key_code).map(str::to_string);
        }

        None
    }

    /// `_toResolvedKeybinding`.
    fn to_resolved_keybinding(self: &Rc<Self>, chord_parts: &[Vec<ScanCodeChord>]) -> Vec<NativeResolvedKeybinding> {
        if chord_parts.is_empty() {
            return Vec::new();
        }
        let mut result = Vec::new();
        self.generate_resolved_keybindings(chord_parts, 0, &[], &mut result);
        result
    }

    fn generate_resolved_keybindings(
        self: &Rc<Self>,
        chord_parts: &[Vec<ScanCodeChord>],
        current_index: usize,
        previous_parts: &[ScanCodeChord],
        result: &mut Vec<NativeResolvedKeybinding>,
    ) {
        let chord_part = &chord_parts[current_index];
        let is_final_index = current_index == chord_parts.len() - 1;
        for part in chord_part {
            let mut chords = previous_parts.to_vec();
            chords.push(*part);
            if is_final_index {
                result.push(native_resolved_keybinding(Rc::clone(self), self.os, chords));
            } else {
                self.generate_resolved_keybindings(chord_parts, current_index + 1, &chords, result);
            }
        }
    }

    /// `_resolveChord`.
    fn resolve_chord(&self, chord: &Chord) -> Vec<ScanCodeChord> {
        match chord {
            Chord::ScanCode(chord) => vec![*chord],
            Chord::KeyCode(chord) => self.key_code_chord_to_scan_code_chord(chord),
        }
    }
}

/// `MacLinuxKeyboardMapper`.
pub struct MacLinuxKeyboardMapper {
    state: Rc<MacLinuxKeyboardMapperState>,
}

const DUMP_SEPARATOR: &str =
    "----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------";
const DUMP_HEADER: &str =
    "|       HW Code combination      |  Key  |    KeyCode combination    | Pri |          UI label         |         User settings          |    Electron accelerator   |       Dispatching string       | WYSIWYG |";

impl MacLinuxKeyboardMapper {
    pub fn new<'a, I>(is_us_standard: bool, raw_mappings: I, map_alt_gr_to_ctrl_alt: bool, os: OperatingSystem) -> Self
    where
        I: IntoIterator<Item = (&'a String, &'a MacLinuxKeyMapping)>,
    {
        Self { state: Rc::new(MacLinuxKeyboardMapperState::new(is_us_standard, raw_mappings, map_alt_gr_to_ctrl_alt, os)) }
    }

    /// `resolveKeyboardEvent`, com o tipo concreto do resultado.
    pub fn resolve_keyboard_event_native(&self, keyboard_event: &KeyboardEvent) -> NativeResolvedKeybinding {
        let mut code = keyboard_event.code.as_deref().map_or(ScanCode::None, ScanCodeUtils::to_enum);

        // Treat NumpadEnter as Enter
        if code == ScanCode::NumpadEnter {
            code = ScanCode::Enter;
        }

        let key_code = keyboard_event.key_code;

        if matches!(
            key_code,
            KeyCode::LeftArrow
                | KeyCode::UpArrow
                | KeyCode::RightArrow
                | KeyCode::DownArrow
                | KeyCode::Delete
                | KeyCode::Insert
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::PageDown
                | KeyCode::PageUp
                | KeyCode::Backspace
        ) {
            // "Dispatch" on keyCode for these key codes to workaround issues with remote desktoping software
            // where the scan codes appear to be incorrect (see https://github.com/microsoft/vscode/issues/24107)
            if let Some(immutable_scan_code) = immutable_key_code_to_code(key_code).filter(|scan_code| *scan_code != ScanCode::DependsOnKbLayout) {
                code = immutable_scan_code;
            }
        } else if matches!(
            code,
            ScanCode::Numpad1
                | ScanCode::Numpad2
                | ScanCode::Numpad3
                | ScanCode::Numpad4
                | ScanCode::Numpad5
                | ScanCode::Numpad6
                | ScanCode::Numpad7
                | ScanCode::Numpad8
                | ScanCode::Numpad9
                | ScanCode::Numpad0
                | ScanCode::NumpadDecimal
        ) {
            // "Dispatch" on keyCode for all numpad keys in order for NumLock to work correctly
            if key_code.0 >= 0 {
                if let Some(immutable_scan_code) = immutable_key_code_to_code(key_code).filter(|scan_code| *scan_code != ScanCode::DependsOnKbLayout) {
                    code = immutable_scan_code;
                }
            }
        }

        let alt_gr = self.state.map_alt_gr_to_ctrl_alt && keyboard_event.alt_graph_key;
        let ctrl_key = keyboard_event.ctrl_key || alt_gr;
        let alt_key = keyboard_event.alt_key || alt_gr;
        let chord = ScanCodeChord::new(ctrl_key, keyboard_event.shift_key, alt_key, keyboard_event.meta_key, code);
        native_resolved_keybinding(Rc::clone(&self.state), self.state.os, vec![chord])
    }

    /// `resolveKeybinding`, com o tipo concreto do resultado.
    pub fn resolve_keybinding_native(&self, keybinding: &Keybinding) -> Vec<NativeResolvedKeybinding> {
        let chords: Vec<Vec<ScanCodeChord>> = keybinding.chords.iter().map(|chord| self.state.resolve_chord(chord)).collect();
        self.state.to_resolved_keybinding(&chords)
    }

    /// `_redirectCharCode`.
    fn redirect_char_code(char_code: u16) -> u16 {
        match char_code {
            // CJK: 。 「 」 【 】 ； ，
            // map: . [ ] [ ] ; ,
            cc::U_IDEOGRAPHIC_FULL_STOP => cc::Period,
            cc::U_LEFT_CORNER_BRACKET => cc::OpenSquareBracket,
            cc::U_RIGHT_CORNER_BRACKET => cc::CloseSquareBracket,
            cc::U_LEFT_BLACK_LENTICULAR_BRACKET => cc::OpenSquareBracket,
            cc::U_RIGHT_BLACK_LENTICULAR_BRACKET => cc::CloseSquareBracket,
            cc::U_FULLWIDTH_SEMICOLON => cc::Semicolon,
            cc::U_FULLWIDTH_COMMA => cc::Comma,
            _ => char_code,
        }
    }

    /// `_charCodeToKb`: a tecla e o Shift que produzem o caractere no layout US, ou `None`.
    pub fn char_code_to_kb(char_code: u16) -> Option<CharKeyCode> {
        let char_code = Self::redirect_char_code(char_code);
        char_code_to_key_code_table().get(usize::from(char_code)).copied().flatten()
    }

    /// Attempt to map a combining character to a regular one that renders the same way.
    ///
    /// <https://www.compart.com/en/unicode/bidiclass/NSM>
    pub fn get_char_code(char: &str) -> u16 {
        let Some(char_code) = char.encode_utf16().next() else { return 0 };
        match char_code {
            cc::U_Combining_Grave_Accent => cc::U_GRAVE_ACCENT,
            cc::U_Combining_Acute_Accent => cc::U_ACUTE_ACCENT,
            cc::U_Combining_Circumflex_Accent => cc::U_CIRCUMFLEX,
            cc::U_Combining_Tilde => cc::U_SMALL_TILDE,
            cc::U_Combining_Macron => cc::U_MACRON,
            cc::U_Combining_Overline => cc::U_OVERLINE,
            cc::U_Combining_Breve => cc::U_BREVE,
            cc::U_Combining_Dot_Above => cc::U_DOT_ABOVE,
            cc::U_Combining_Diaeresis => cc::U_DIAERESIS,
            cc::U_Combining_Ring_Above => cc::U_RING_ABOVE,
            cc::U_Combining_Double_Acute_Accent => cc::U_DOUBLE_ACUTE_ACCENT,
            _ => char_code,
        }
    }
}

impl Deref for MacLinuxKeyboardMapper {
    type Target = MacLinuxKeyboardMapperState;

    fn deref(&self) -> &MacLinuxKeyboardMapperState {
        &self.state
    }
}

impl IKeyboardMapper for MacLinuxKeyboardMapper {
    fn dump_debug_info(&self) -> String {
        let state = &self.state;
        let mut result: Vec<String> = Vec::new();

        let immutable_samples = [ScanCode::ArrowUp, ScanCode::Numpad0];

        let mut cnt = 0;
        result.push(format!("isUSStandard: {}", state.is_us_standard));
        result.push(DUMP_SEPARATOR.to_string());
        for scan_code in (ScanCode::None.0..ScanCode::MaxValue.0).map(ScanCode) {
            if immutable_key_code(scan_code) != KeyCode::DependsOnKbLayout && !immutable_samples.contains(&scan_code) {
                continue;
            }

            if cnt % 4 == 0 {
                result.push(DUMP_HEADER.to_string());
                result.push(DUMP_SEPARATOR.to_string());
            }
            cnt += 1;

            let mapping = state.code_info.get(&scan_code);

            for modifier in 0..8_u8 {
                let hw_ctrl_key = modifier & 0b001 != 0;
                let hw_shift_key = modifier & 0b010 != 0;
                let hw_alt_key = modifier & 0b100 != 0;
                let scan_code_combo = ScanCodeCombo::new(hw_ctrl_key, hw_shift_key, hw_alt_key, scan_code);
                let resolved_kb = self.resolve_keyboard_event_native(&KeyboardEvent {
                    ctrl_key: scan_code_combo.ctrl_key,
                    shift_key: scan_code_combo.shift_key,
                    alt_key: scan_code_combo.alt_key,
                    meta_key: false,
                    alt_graph_key: false,
                    key_code: KeyCode::DependsOnKbLayout,
                    code: Some(ScanCodeUtils::to_string(scan_code).to_string()),
                });

                let out_scan_code_combo = scan_code_combo.to_string();
                let out_key = scan_code_combo.get_produced_char(mapping);
                let aria_label = resolved_kb.get_aria_label().filter(|label| !label.is_empty());
                let out_ui_label = aria_label.map(|label| label.replacen("Control+", "Ctrl+", 1));
                let out_user_settings = resolved_kb.get_user_settings_label();
                let out_electron_accelerator = resolved_kb.get_electron_accelerator();
                let out_dispatch_str = resolved_kb.get_dispatch_chords().into_iter().next().flatten();

                let out_wysiwyg = if resolved_kb.is_wysiwyg() { "       " } else { "   NO  " };

                let kb_combos = state.scan_code_key_code_mapper.lookup_scan_code_combo(&scan_code_combo);
                if kb_combos.is_empty() {
                    result.push(format!(
                        "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                        left_pad(Some(&out_scan_code_combo), 30),
                        out_key,
                        left_pad(Some(""), 25),
                        left_pad(Some(""), 3),
                        left_pad(out_ui_label.as_deref(), 25),
                        left_pad(out_user_settings.as_deref(), 30),
                        left_pad(out_electron_accelerator.as_deref(), 25),
                        left_pad(out_dispatch_str.as_deref(), 30),
                        out_wysiwyg,
                    ));
                } else {
                    for (i, kb_combo) in kb_combos.iter().enumerate() {
                        // find out the priority of this scan code for this key code
                        let scan_code_combos = state.scan_code_key_code_mapper.lookup_key_code_combo(kb_combo);
                        let col_priority = if scan_code_combos.len() == 1 {
                            // no need for priority, this key code combo maps to precisely this scan code combo
                            String::new()
                        } else {
                            let priority = scan_code_combos
                                .iter()
                                .position(|combo| combo.equals(&scan_code_combo))
                                .map_or(-1, |position| position as i64 + 1);
                            priority.to_string()
                        };

                        let out_keybinding = kb_combo.to_string();
                        if i == 0 {
                            result.push(format!(
                                "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                                left_pad(Some(&out_scan_code_combo), 30),
                                out_key,
                                left_pad(Some(&out_keybinding), 25),
                                left_pad(Some(&col_priority), 3),
                                left_pad(out_ui_label.as_deref(), 25),
                                left_pad(out_user_settings.as_deref(), 30),
                                left_pad(out_electron_accelerator.as_deref(), 25),
                                left_pad(out_dispatch_str.as_deref(), 30),
                                out_wysiwyg,
                            ));
                        } else {
                            // secondary keybindings
                            result.push(format!(
                                "| {} |       | {} | {} | {} | {} | {} | {} |         |",
                                left_pad(Some(""), 30),
                                left_pad(Some(&out_keybinding), 25),
                                left_pad(Some(&col_priority), 3),
                                left_pad(Some(""), 25),
                                left_pad(Some(""), 30),
                                left_pad(Some(""), 25),
                                left_pad(Some(""), 30),
                            ));
                        }
                    }
                }
            }
            result.push(DUMP_SEPARATOR.to_string());
        }

        result.join("\n")
    }

    fn resolve_keyboard_event(&self, keyboard_event: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding> {
        Rc::new(self.resolve_keyboard_event_native(keyboard_event))
    }

    fn resolve_keybinding(&self, keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>> {
        self.resolve_keybinding_native(keybinding)
            .into_iter()
            .map(|resolved| Rc::new(resolved) as Rc<dyn ResolvedKeybinding>)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_char_code_maps_combining_characters_to_spacing_ones() {
        assert_eq!(MacLinuxKeyboardMapper::get_char_code(""), 0);
        assert_eq!(MacLinuxKeyboardMapper::get_char_code("a"), cc::a);
        assert_eq!(MacLinuxKeyboardMapper::get_char_code("\u{0300}"), cc::U_GRAVE_ACCENT);
        assert_eq!(MacLinuxKeyboardMapper::get_char_code("\u{0301}"), cc::U_ACUTE_ACCENT);
        assert_eq!(MacLinuxKeyboardMapper::get_char_code("\u{0302}"), cc::U_CIRCUMFLEX);
        assert_eq!(MacLinuxKeyboardMapper::get_char_code("\u{0303}"), cc::U_SMALL_TILDE);
        assert_eq!(MacLinuxKeyboardMapper::get_char_code("\u{030b}"), cc::U_DOUBLE_ACUTE_ACCENT);
        // Só a primeira unidade UTF-16 conta.
        assert_eq!(MacLinuxKeyboardMapper::get_char_code("xyz"), cc::x);
    }

    #[test]
    fn char_code_to_kb_covers_letters_and_punctuation() {
        let slash = MacLinuxKeyboardMapper::char_code_to_kb(cc::Slash).unwrap();
        assert_eq!((slash.key_code, slash.shift_key), (KeyCode::Slash, false));
        let question = MacLinuxKeyboardMapper::char_code_to_kb(cc::QuestionMark).unwrap();
        assert_eq!((question.key_code, question.shift_key), (KeyCode::Slash, true));
        let upper_c = MacLinuxKeyboardMapper::char_code_to_kb(cc::C).unwrap();
        assert_eq!((upper_c.key_code, upper_c.shift_key), (KeyCode::KeyC, true));
        let lower_z = MacLinuxKeyboardMapper::char_code_to_kb(cc::z).unwrap();
        assert_eq!((lower_z.key_code, lower_z.shift_key), (KeyCode::KeyZ, false));
        let double_quote = MacLinuxKeyboardMapper::char_code_to_kb(cc::DoubleQuote).unwrap();
        assert_eq!((double_quote.key_code, double_quote.shift_key), (KeyCode::Quote, true));
        // Caracteres sem tecla no layout US, e fora da tabela.
        assert_eq!(MacLinuxKeyboardMapper::char_code_to_kb(cc::Digit1), None);
        assert_eq!(MacLinuxKeyboardMapper::char_code_to_kb(0x00e7), None);
    }

    #[test]
    fn char_code_to_kb_redirects_cjk_punctuation() {
        let period = MacLinuxKeyboardMapper::char_code_to_kb(cc::U_IDEOGRAPHIC_FULL_STOP).unwrap();
        assert_eq!((period.key_code, period.shift_key), (KeyCode::Period, false));
        let bracket_left = MacLinuxKeyboardMapper::char_code_to_kb(cc::U_LEFT_BLACK_LENTICULAR_BRACKET).unwrap();
        assert_eq!((bracket_left.key_code, bracket_left.shift_key), (KeyCode::BracketLeft, false));
        let bracket_right = MacLinuxKeyboardMapper::char_code_to_kb(cc::U_RIGHT_CORNER_BRACKET).unwrap();
        assert_eq!((bracket_right.key_code, bracket_right.shift_key), (KeyCode::BracketRight, false));
        let semicolon = MacLinuxKeyboardMapper::char_code_to_kb(cc::U_FULLWIDTH_SEMICOLON).unwrap();
        assert_eq!((semicolon.key_code, semicolon.shift_key), (KeyCode::Semicolon, false));
        let comma = MacLinuxKeyboardMapper::char_code_to_kb(cc::U_FULLWIDTH_COMMA).unwrap();
        assert_eq!((comma.key_code, comma.shift_key), (KeyCode::Comma, false));
    }

    #[test]
    fn combos_encode_modifiers_in_the_low_bits() {
        let combo = ScanCodeCombo::new(true, false, true, ScanCode::KeyA);
        assert_eq!(ScanCodeKeyCodeMapper::encode_scan_code_combo(&combo), 0b101 | ((ScanCode::KeyA.0 as u32) << 3));
        assert_eq!(combo.to_string(), "Ctrl+Alt+KeyA");
        let key_combo = KeyCodeCombo::new(false, true, false, KeyCode::KeyA);
        assert_eq!(ScanCodeKeyCodeMapper::encode_key_code_combo(&key_combo), 0b010 | ((KeyCode::KeyA.0 as u32) << 3));
    }

    #[test]
    fn produced_char_follows_the_modifiers() {
        let mapping = MacLinuxKeyMapping::new("a", "A", "\u{00e5}", "\u{00c5}");
        let plain = ScanCodeCombo::new(false, false, false, ScanCode::KeyA);
        let shift = ScanCodeCombo::new(false, true, false, ScanCode::KeyA);
        let alt_gr = ScanCodeCombo::new(true, false, true, ScanCode::KeyA);
        let shift_alt_gr = ScanCodeCombo::new(true, true, true, ScanCode::KeyA);
        assert_eq!(plain.get_produced_char(Some(&mapping)), "  a  ");
        assert_eq!(shift.get_produced_char(Some(&mapping)), "  A  ");
        assert_eq!(alt_gr.get_produced_char(Some(&mapping)), "  \u{00e5}  ");
        assert_eq!(shift_alt_gr.get_produced_char(Some(&mapping)), "  \u{00c5}  ");
        assert_eq!(plain.get_produced_char(None), " --- ");
        // Um acento combinante sai como o ponto de código, e a tecla vazia como o traço de ausência.
        let dead = MacLinuxKeyMapping::new("", "", "", "");
        assert_eq!(plain.get_produced_char(Some(&dead)), " --- ");
        assert_eq!(MacLinuxKeyboardMapper::get_char_code("\u{0300}"), cc::U_GRAVE_ACCENT);
    }

    #[test]
    fn a_single_letter_layout_resolves_both_ways() {
        let entries = vec![("KeyA".to_string(), MacLinuxKeyMapping::new("a", "A", "", ""))];
        let mapper = MacLinuxKeyboardMapper::new(true, entries.iter().map(|(key, mapping)| (key, mapping)), false, OperatingSystem::Linux);

        let scan_code_chords = mapper.key_code_chord_to_scan_code_chord(&KeyCodeChord::new(false, false, false, false, KeyCode::KeyA));
        assert_eq!(scan_code_chords, vec![ScanCodeChord::new(false, false, false, false, ScanCode::KeyA)]);

        let chord = ScanCodeChord::new(false, false, false, false, ScanCode::KeyA);
        assert_eq!(mapper.get_ui_label_for_scan_code_chord(Some(&chord)), Some("A".to_string()));
        assert_eq!(mapper.get_dispatch_str_for_scan_code_chord(&chord), Some("[KeyA]".to_string()));
        assert_eq!(mapper.get_user_settings_label_for_scan_code_chord(Some(&chord)), Some("a".to_string()));
        assert_eq!(mapper.get_ui_label_for_scan_code_chord(None), None);
    }
}
