//! Porte de `src/vs/base/common/keyCodes.ts`: `KeyCode`, `ScanCode`, as tabelas de nome e de código
//! (`KeyCodeUtils`, `ScanCodeUtils`, `EVENT_KEY_CODE_MAP`, `NATIVE_WINDOWS_KEY_CODE_TO_KEY_CODE`...),
//! `KeyMod` e `KeyChord`.
//!
//! Os enums do original são `const enum` de números, e a decodificação binária de atalhos aceita
//! qualquer valor de 0 a 255 como código de tecla, inclusive os que o enum não nomeia. Por isso
//! [`KeyCode`] e [`ScanCode`] são tipos de um inteiro com uma constante associada por membro (as
//! constantes e a tabela `mappings` saem do gerador `scripts/gen_key_codes.py`, em
//! `key_codes_tables.rs`), e não `enum` do Rust, que não representaria um valor sem nome.
//!
//! As tabelas que o original monta numa função anônima ao carregar o módulo saem aqui de um
//! `OnceLock`, montado na primeira consulta com a mesma lógica (a primeira ocorrência de cada código
//! define o nome, a última define o mapa de eventos).

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::key_codes_tables::MAPPINGS;

/// Virtual Key Codes: o valor não tem significado próprio. Mais gerais que os do Windows, funcionam
/// entre navegadores e sistemas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyCode(pub i32);

/// Código físico da tecla (o `KeyboardEvent.code` do W3C).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScanCode(pub i32);

impl KeyCode {
    /// O valor como o JavaScript o entende numa máscara de bits (`keyCode | KeyMod.Alt`).
    pub fn bits(self) -> u32 {
        self.0 as u32
    }
}

/// Uma linha da tabela `mappings` do original: `[immutable, scanCode, scanCodeStr, keyCode,
/// keyCodeStr, eventKeyCode, vkey, usUserSettingsLabel, generalUserSettingsLabel]`, com `empty` como `""`.
#[derive(Clone, Copy, Debug)]
pub struct KeyCodeMapping {
    pub immutable: bool,
    pub scan_code: ScanCode,
    pub scan_code_str: &'static str,
    pub key_code: KeyCode,
    pub key_code_str: &'static str,
    pub event_key_code: i32,
    pub vkey: &'static str,
    pub us_user_settings_label: &'static str,
    pub general_user_settings_label: &'static str,
}

/// `KeyCodeStrMap`: nome de código e, sem diferenciar caixa, código de nome.
struct KeyCodeStrMap {
    key_code_to_str: Vec<Option<&'static str>>,
    str_to_key_code: HashMap<String, KeyCode>,
}

impl KeyCodeStrMap {
    fn new() -> Self {
        Self { key_code_to_str: vec![None; (KeyCode::MaxValue.0 + 1) as usize], str_to_key_code: HashMap::new() }
    }

    fn define(&mut self, key_code: KeyCode, str: &'static str) {
        self.key_code_to_str[key_code.0 as usize] = Some(str);
        self.str_to_key_code.insert(str.to_lowercase(), key_code);
    }

    fn key_code_to_str(&self, key_code: KeyCode) -> Option<&'static str> {
        usize::try_from(key_code.0).ok().and_then(|i| self.key_code_to_str.get(i)).copied().flatten()
    }

    /// `strToKeyCode`: `KeyCode.Unknown` quando o nome não existe.
    fn str_to_key_code(&self, str: &str) -> KeyCode {
        self.str_to_key_code.get(&str.to_lowercase()).copied().unwrap_or(KeyCode::Unknown)
    }
}

struct Tables {
    ui_map: KeyCodeStrMap,
    user_settings_us_map: KeyCodeStrMap,
    user_settings_general_map: KeyCodeStrMap,
    event_key_code_map: Vec<Option<KeyCode>>,
    native_windows_key_code_to_key_code: HashMap<&'static str, KeyCode>,
    scan_code_str_to_event_key_code: HashMap<&'static str, i32>,
    scan_code_int_to_str: Vec<Option<&'static str>>,
    scan_code_str_to_int: HashMap<&'static str, ScanCode>,
    scan_code_lower_case_str_to_int: HashMap<String, ScanCode>,
    immutable_code_to_key_code: Vec<KeyCode>,
    immutable_key_code_to_code: Vec<ScanCode>,
}

/// O tamanho do `new Array(230)` do `EVENT_KEY_CODE_MAP`.
const EVENT_KEY_CODE_MAP_LEN: usize = 230;

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(build_tables)
}

fn build_tables() -> Tables {
    let mut t = Tables {
        ui_map: KeyCodeStrMap::new(),
        user_settings_us_map: KeyCodeStrMap::new(),
        user_settings_general_map: KeyCodeStrMap::new(),
        event_key_code_map: vec![None; EVENT_KEY_CODE_MAP_LEN],
        native_windows_key_code_to_key_code: HashMap::new(),
        scan_code_str_to_event_key_code: HashMap::new(),
        scan_code_int_to_str: vec![None; (ScanCode::MaxValue.0 + 1) as usize],
        scan_code_str_to_int: HashMap::new(),
        scan_code_lower_case_str_to_int: HashMap::new(),
        immutable_code_to_key_code: vec![KeyCode::DependsOnKbLayout; (ScanCode::MaxValue.0 + 1) as usize],
        immutable_key_code_to_code: vec![ScanCode::DependsOnKbLayout; (KeyCode::MaxValue.0 + 1) as usize],
    };

    let mut seen_key_code = vec![false; (KeyCode::MaxValue.0 + 1) as usize];
    let mut seen_scan_code = vec![false; (ScanCode::MaxValue.0 + 1) as usize];

    for m in MAPPINGS.iter() {
        let (scan_code, key_code) = (m.scan_code, m.key_code);
        if !seen_scan_code[scan_code.0 as usize] {
            seen_scan_code[scan_code.0 as usize] = true;
            t.scan_code_int_to_str[scan_code.0 as usize] = Some(m.scan_code_str);
            t.scan_code_str_to_int.insert(m.scan_code_str, scan_code);
            t.scan_code_lower_case_str_to_int.insert(m.scan_code_str.to_lowercase(), scan_code);

            if m.immutable {
                t.immutable_code_to_key_code[scan_code.0 as usize] = key_code;
                if key_code != KeyCode::Unknown && key_code != KeyCode::Enter && !is_modifier_key(key_code) {
                    t.immutable_key_code_to_code[key_code.0 as usize] = scan_code;
                }
            }
        }

        if !seen_key_code[key_code.0 as usize] {
            seen_key_code[key_code.0 as usize] = true;
            assert!(
                !m.key_code_str.is_empty(),
                "String representation missing for key code {} around scan code {}",
                key_code.0,
                m.scan_code_str
            );
            t.ui_map.define(key_code, m.key_code_str);
            let us = if m.us_user_settings_label.is_empty() { m.key_code_str } else { m.us_user_settings_label };
            t.user_settings_us_map.define(key_code, us);
            let general = [m.general_user_settings_label, m.us_user_settings_label, m.key_code_str]
                .into_iter()
                .find(|s| !s.is_empty())
                .unwrap_or_default();
            t.user_settings_general_map.define(key_code, general);
        }

        if m.event_key_code != 0 {
            t.event_key_code_map[m.event_key_code as usize] = Some(key_code);
        }
        if !m.scan_code_str.is_empty() {
            t.scan_code_str_to_event_key_code.insert(m.scan_code_str, m.event_key_code);
        }
        if !m.vkey.is_empty() {
            t.native_windows_key_code_to_key_code.insert(m.vkey, key_code);
        }
    }
    t.immutable_key_code_to_code[KeyCode::Enter.0 as usize] = ScanCode::Enter;
    t
}

/// `EVENT_KEY_CODE_MAP[eventKeyCode]`: o `KeyboardEvent.keyCode` do navegador para o [`KeyCode`].
pub fn event_key_code(event_key_code: i32) -> Option<KeyCode> {
    usize::try_from(event_key_code).ok().and_then(|i| tables().event_key_code_map.get(i)).copied().flatten()
}

/// `NATIVE_WINDOWS_KEY_CODE_TO_KEY_CODE[vkey]`: o `VK_*` do Windows para o [`KeyCode`].
pub fn native_windows_key_code_to_key_code(vkey: &str) -> Option<KeyCode> {
    tables().native_windows_key_code_to_key_code.get(vkey).copied()
}

/// `SCAN_CODE_STR_TO_EVENT_KEY_CODE[scanCodeStr]`.
pub fn scan_code_str_to_event_key_code(scan_code_str: &str) -> Option<i32> {
    tables().scan_code_str_to_event_key_code.get(scan_code_str).copied()
}

/// `IMMUTABLE_CODE_TO_KEY_CODE[scanCode]`: [`KeyCode::DependsOnKbLayout`] quando a tecla muda com o layout.
pub fn immutable_code_to_key_code(scan_code: ScanCode) -> Option<KeyCode> {
    usize::try_from(scan_code.0).ok().and_then(|i| tables().immutable_code_to_key_code.get(i)).copied()
}

/// `IMMUTABLE_KEY_CODE_TO_CODE[keyCode]`: [`ScanCode::DependsOnKbLayout`] quando a tecla muda com o layout.
pub fn immutable_key_code_to_code(key_code: KeyCode) -> Option<ScanCode> {
    usize::try_from(key_code.0).ok().and_then(|i| tables().immutable_key_code_to_code.get(i)).copied()
}

/// `ScanCodeUtils`.
pub struct ScanCodeUtils;

impl ScanCodeUtils {
    /// `lowerCaseToEnum`: [`ScanCode::None`] quando o nome não existe.
    pub fn lower_case_to_enum(scan_code: &str) -> ScanCode {
        tables().scan_code_lower_case_str_to_int.get(scan_code).copied().unwrap_or(ScanCode::None)
    }

    /// `toEnum`: [`ScanCode::None`] quando o nome não existe.
    pub fn to_enum(scan_code: &str) -> ScanCode {
        tables().scan_code_str_to_int.get(scan_code).copied().unwrap_or(ScanCode::None)
    }

    /// `toString`: `"None"` quando o código não tem nome.
    pub fn to_string(scan_code: ScanCode) -> &'static str {
        usize::try_from(scan_code.0).ok().and_then(|i| tables().scan_code_int_to_str.get(i)).copied().flatten().unwrap_or("None")
    }
}

/// `KeyCodeUtils`.
pub struct KeyCodeUtils;

impl KeyCodeUtils {
    pub fn to_string(key_code: KeyCode) -> Option<&'static str> {
        tables().ui_map.key_code_to_str(key_code)
    }

    pub fn from_string(key: &str) -> KeyCode {
        tables().ui_map.str_to_key_code(key)
    }

    pub fn to_user_settings_us(key_code: KeyCode) -> Option<&'static str> {
        tables().user_settings_us_map.key_code_to_str(key_code)
    }

    pub fn to_user_settings_general(key_code: KeyCode) -> Option<&'static str> {
        tables().user_settings_general_map.key_code_to_str(key_code)
    }

    /// `fromUserSettings`: tenta o nome do layout US e depois o geral.
    pub fn from_user_settings(key: &str) -> KeyCode {
        let t = tables();
        let us = t.user_settings_us_map.str_to_key_code(key);
        if us != KeyCode::Unknown { us } else { t.user_settings_general_map.str_to_key_code(key) }
    }

    pub fn to_electron_accelerator(key_code: KeyCode) -> Option<&'static str> {
        if key_code >= KeyCode::Numpad0 && key_code <= KeyCode::NumpadDivide {
            // See https://github.com/electron/electron/blob/master/docs/api/accelerator.md#available-key-codes
            // electron cannot handle numpad keys
            return None;
        }

        match key_code {
            KeyCode::UpArrow => Some("Up"),
            KeyCode::DownArrow => Some("Down"),
            KeyCode::LeftArrow => Some("Left"),
            KeyCode::RightArrow => Some("Right"),
            // electron menu always expects `Delete` and never `Del`
            KeyCode::Delete => Some("Delete"),
            _ => tables().ui_map.key_code_to_str(key_code),
        }
    }
}

/// `KeyMod`: as máscaras da codificação binária de atalhos.
pub struct KeyMod;

impl KeyMod {
    pub const CTRL_CMD: u32 = 1 << 11;
    pub const SHIFT: u32 = 1 << 10;
    pub const ALT: u32 = 1 << 9;
    pub const WIN_CTRL: u32 = 1 << 8;
}

/// `KeyChord`: junta dois acordes na codificação binária (o segundo nos 16 bits altos).
pub fn key_chord(first_part: u32, second_part: u32) -> u32 {
    let chord_part = (second_part & 0x0000_FFFF) << 16;
    first_part | chord_part
}

pub fn is_modifier_key(key_code: KeyCode) -> bool {
    key_code == KeyCode::Ctrl || key_code == KeyCode::Shift || key_code == KeyCode::Alt || key_code == KeyCode::Meta
}
