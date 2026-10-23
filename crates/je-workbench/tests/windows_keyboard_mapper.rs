//! Porte de `src/vs/workbench/services/keybinding/test/node/windowsKeyboardMapper.test.ts` (33 casos), caso a caso
//! e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - O literal `IResolvedKeybinding` do original vira [`expected`], e o `IKeyboardEvent` vira [`event`], para o
//!   corpo de cada caso mostrar só o que varia (o `_standardKeyboardEventBrand` não tem equivalente e `code: null!`
//!   é `None`).
//! - `suiteSetup` (um mapeador por suíte) vira [`create_keyboard_mapper`] em cada caso: montar o mapeador é barato.
//! - `readRawMapping` executa o arquivo `win_*.js` (um `define({...})` com um literal de objeto). Aqui o literal é
//!   lido por um analisador mínimo ([`parse_js_literal`]) que cobre o formato desses arquivos: objetos, chaves sem
//!   aspas ou com aspas, textos entre aspas simples ou duplas com escapes, comentários e vírgula final.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::key_codes::{KeyCode, KeyMod, ScanCode, key_chord};
use je_base::keybindings::{Chord, KeyCodeChord, Keybinding, ScanCodeChord, SingleModifierChord, decode_keybinding};
use je_base::platform::OperatingSystem;
use je_platform::keybinding::keybinding::KeyboardEvent;
use je_platform::keyboard_layout::WindowsKeyMapping;
use je_workbench::services::keybinding::windows_keyboard_mapper::WindowsKeyboardMapper;
use support::keyboard_mapper_test_utils::{
    ResolvedKeybindingSnapshot, assert_mapping, assert_resolve_keybinding, assert_resolve_keyboard_event,
    keybinding_test_file_path,
};

const WRITE_FILE_IF_DIFFERENT: bool = false;

/// Um valor do literal JavaScript: só objetos e textos aparecem nos arquivos de mapa de teclas.
enum JsValue {
    Object(Vec<(String, JsValue)>),
    Text(String),
}

struct JsLiteralParser {
    chars: Vec<char>,
    pos: usize,
}

impl JsLiteralParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> char {
        let char = self.peek().expect("fim inesperado do literal");
        self.pos += 1;
        char
    }

    fn starts_with(&self, prefix: &str) -> bool {
        prefix.chars().enumerate().all(|(i, c)| self.chars.get(self.pos + i) == Some(&c))
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(c) if c.is_whitespace() => self.pos += 1,
                Some('/') if self.starts_with("//") => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.pos += 1;
                    }
                }
                Some('/') if self.starts_with("/*") => {
                    self.pos += 2;
                    while !self.starts_with("*/") {
                        self.next();
                    }
                    self.pos += 2;
                }
                _ => return,
            }
        }
    }

    fn expect(&mut self, expected: char) {
        let found = self.next();
        assert_eq!(found, expected, "esperava `{expected}` na posição {}", self.pos - 1);
    }

    fn parse_hex(&mut self, digits: usize) -> u32 {
        let text: String = (0..digits).map(|_| self.next()).collect();
        u32::from_str_radix(&text, 16).expect("escape hexadecimal inválido")
    }

    fn parse_text(&mut self) -> String {
        let quote = self.next();
        let mut units: Vec<u16> = Vec::new();
        let push_char = |units: &mut Vec<u16>, c: char| {
            let mut buffer = [0u16; 2];
            units.extend_from_slice(c.encode_utf16(&mut buffer));
        };
        loop {
            let char = self.next();
            if char == quote {
                break;
            }
            if char != '\\' {
                push_char(&mut units, char);
                continue;
            }
            match self.next() {
                'n' => units.push(u16::from(b'\n')),
                't' => units.push(u16::from(b'\t')),
                'r' => units.push(u16::from(b'\r')),
                'b' => units.push(8),
                'f' => units.push(12),
                'v' => units.push(11),
                '0' => units.push(0),
                'x' => {
                    let code = self.parse_hex(2);
                    units.push(code as u16);
                }
                'u' => {
                    let code = self.parse_hex(4);
                    units.push(code as u16);
                }
                other => push_char(&mut units, other),
            }
        }
        String::from_utf16(&units).expect("texto UTF-16 inválido no literal")
    }

    fn parse_key(&mut self) -> String {
        if matches!(self.peek(), Some('\'' | '"')) {
            return self.parse_text();
        }
        let mut key = String::new();
        while let Some(c) = self.peek().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '$') {
            key.push(c);
            self.pos += 1;
        }
        assert!(!key.is_empty(), "chave vazia na posição {}", self.pos);
        key
    }

    fn parse_object(&mut self) -> Vec<(String, JsValue)> {
        self.expect('{');
        let mut entries: Vec<(String, JsValue)> = Vec::new();
        loop {
            self.skip_whitespace_and_comments();
            if self.peek() == Some('}') {
                self.pos += 1;
                return entries;
            }
            let key = self.parse_key();
            self.skip_whitespace_and_comments();
            self.expect(':');
            let value = self.parse_value();
            // Uma chave repetida substitui o valor e mantém a posição, como a atribuição de propriedade do JS.
            match entries.iter_mut().find(|(existing, _)| *existing == key) {
                Some(entry) => entry.1 = value,
                None => entries.push((key, value)),
            }
            self.skip_whitespace_and_comments();
            if self.peek() == Some(',') {
                self.pos += 1;
            }
        }
    }

    fn parse_value(&mut self) -> JsValue {
        self.skip_whitespace_and_comments();
        match self.peek() {
            Some('{') => JsValue::Object(self.parse_object()),
            Some('\'' | '"') => JsValue::Text(self.parse_text()),
            other => panic!("valor inesperado {other:?} na posição {}", self.pos),
        }
    }
}

/// O objeto passado a `define(...)` no texto de um arquivo `win_*.js`.
fn parse_js_literal(contents: &str) -> Vec<(String, JsValue)> {
    let start = contents.find("define(").expect("o arquivo não chama define(...)") + "define(".len();
    let mut parser = JsLiteralParser { chars: contents[start..].chars().collect(), pos: 0 };
    parser.skip_whitespace_and_comments();
    parser.parse_object()
}

/// `readRawMapping<IWindowsKeyboardMapping>(file)`: o mapa na ordem em que o arquivo o declara.
fn read_raw_mapping(file: &str) -> Vec<(String, WindowsKeyMapping)> {
    let path = keybinding_test_file_path(&format!("{file}.js"));
    let contents = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("não foi possível ler {}: {error}", path.display()));
    parse_js_literal(&contents)
        .into_iter()
        .map(|(code, mapping)| {
            let JsValue::Object(fields) = mapping else { panic!("o mapeamento de {code} não é um objeto") };
            let field = |name: &str| {
                fields
                    .iter()
                    .find_map(|(key, value)| match value {
                        JsValue::Text(text) if key == name => Some(text.clone()),
                        _ => None,
                    })
                    .unwrap_or_default()
            };
            let mapping = WindowsKeyMapping {
                vkey: field("vkey"),
                value: field("value"),
                with_shift: field("withShift"),
                with_alt_gr: field("withAltGr"),
                with_shift_alt_gr: field("withShiftAltGr"),
            };
            (code, mapping)
        })
        .collect()
}

fn new_mapper(is_us_standard: bool, raw_mappings: &[(String, WindowsKeyMapping)], map_alt_gr_to_ctrl_alt: bool) -> WindowsKeyboardMapper {
    WindowsKeyboardMapper::new(is_us_standard, raw_mappings.iter().map(|(code, mapping)| (code, mapping)), map_alt_gr_to_ctrl_alt)
}

/// `createKeyboardMapper`.
fn create_keyboard_mapper(is_us_standard: bool, file: &str, map_alt_gr_to_ctrl_alt: bool) -> WindowsKeyboardMapper {
    new_mapper(is_us_standard, &read_raw_mapping(file), map_alt_gr_to_ctrl_alt)
}

/// `_assertResolveKeybinding`.
fn assert_binary_resolve_keybinding(mapper: &WindowsKeyboardMapper, k: u32, expected: &[ResolvedKeybindingSnapshot]) {
    let keybinding = decode_keybinding(k, OperatingSystem::Windows).unwrap().unwrap();
    assert_resolve_keybinding(mapper, &keybinding, expected);
}

fn s(text: &str) -> Option<String> {
    Some(text.to_string())
}

#[allow(clippy::too_many_arguments)]
fn expected(
    label: &str,
    aria_label: &str,
    electron_accelerator: Option<&str>,
    user_settings_label: &str,
    is_wysiwyg: bool,
    is_multi_chord: bool,
    dispatch_parts: &[Option<&str>],
    single_modifier_dispatch_parts: &[Option<SingleModifierChord>],
) -> ResolvedKeybindingSnapshot {
    ResolvedKeybindingSnapshot {
        label: s(label),
        aria_label: s(aria_label),
        electron_accelerator: electron_accelerator.map(str::to_string),
        user_settings_label: s(user_settings_label),
        is_wysiwyg,
        is_multi_chord,
        dispatch_parts: dispatch_parts.iter().map(|p| p.map(str::to_string)).collect(),
        single_modifier_dispatch_parts: single_modifier_dispatch_parts.to_vec(),
    }
}

/// Um `IKeyboardEvent` na ordem `ctrl, shift, alt, meta, altGraph` do original.
fn event(ctrl: bool, shift: bool, alt: bool, meta: bool, alt_gr: bool, key_code: KeyCode) -> KeyboardEvent {
    KeyboardEvent { ctrl_key: ctrl, shift_key: shift, alt_key: alt, meta_key: meta, alt_graph_key: alt_gr, key_code, code: None }
}

fn key_code_chord(ctrl: bool, shift: bool, alt: bool, meta: bool, key_code: KeyCode) -> Chord {
    Chord::KeyCode(KeyCodeChord::new(ctrl, shift, alt, meta, key_code))
}

fn scan_code_chord(ctrl: bool, shift: bool, alt: bool, meta: bool, scan_code: ScanCode) -> Chord {
    Chord::ScanCode(ScanCodeChord::new(ctrl, shift, alt, meta, scan_code))
}

/// Os atalhos de um acorde só que o original escreve como `Ctrl+X` com `[null]` nas partes de modificador único.
fn ctrl_chord(label: &str, aria_label: &str, electron: Option<&str>, user_settings: &str, wysiwyg: bool, dispatch: &str) -> ResolvedKeybindingSnapshot {
    expected(label, aria_label, electron, user_settings, wysiwyg, false, &[Some(dispatch)], &[None])
}

mod de_ch {
    use super::*;

    fn mapper() -> WindowsKeyboardMapper {
        create_keyboard_mapper(false, "win_de_ch", false)
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "win_de_ch.txt");
    }

    #[test]
    fn resolve_keybinding_ctrl_a() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::KeyA.bits(),
            &[ctrl_chord("Ctrl+A", "Control+A", Some("Ctrl+A"), "ctrl+a", true, "ctrl+A")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::KeyZ.bits(),
            &[ctrl_chord("Ctrl+Z", "Control+Z", Some("Ctrl+Z"), "ctrl+z", true, "ctrl+Z")],
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::KeyZ),
            &ctrl_chord("Ctrl+Z", "Control+Z", Some("Ctrl+Z"), "ctrl+z", true, "ctrl+Z"),
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::BracketRight.bits(),
            &[ctrl_chord("Ctrl+^", "Control+^", Some("Ctrl+]"), "ctrl+oem_6", false, "ctrl+]")],
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::BracketRight),
            &ctrl_chord("Ctrl+^", "Control+^", Some("Ctrl+]"), "ctrl+oem_6", false, "ctrl+]"),
        );
    }

    #[test]
    fn resolve_keybinding_shift_bracket_right() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::SHIFT | KeyCode::BracketRight.bits(),
            &[ctrl_chord("Shift+^", "Shift+^", Some("Shift+]"), "shift+oem_6", false, "shift+]")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::Slash.bits(),
            &[ctrl_chord("Ctrl+§", "Control+§", Some("Ctrl+/"), "ctrl+oem_2", false, "ctrl+/")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_shift_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyMod::SHIFT | KeyCode::Slash.bits(),
            &[ctrl_chord("Ctrl+Shift+§", "Control+Shift+§", Some("Ctrl+Shift+/"), "ctrl+shift+oem_2", false, "ctrl+shift+/")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_k_ctrl_backslash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Backslash.bits()),
            &[expected(
                "Ctrl+K Ctrl+ä",
                "Control+K Control+ä",
                None,
                "ctrl+k ctrl+oem_5",
                false,
                true,
                &[Some("ctrl+K"), Some("ctrl+\\")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_k_ctrl_equal() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Equal.bits()),
            &[],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_down_arrow() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::DownArrow.bits(),
            &[ctrl_chord("Ctrl+DownArrow", "Control+DownArrow", Some("Ctrl+Down"), "ctrl+down", true, "ctrl+DownArrow")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_numpad_0() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::Numpad0.bits(),
            &[ctrl_chord("Ctrl+NumPad0", "Control+NumPad0", None, "ctrl+numpad0", true, "ctrl+NumPad0")],
        );
    }

    #[test]
    fn resolve_keybinding_ctrl_home() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            KeyMod::CTRL_CMD | KeyCode::Home.bits(),
            &[ctrl_chord("Ctrl+Home", "Control+Home", Some("Ctrl+Home"), "ctrl+home", true, "ctrl+Home")],
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_home() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::Home),
            &ctrl_chord("Ctrl+Home", "Control+Home", Some("Ctrl+Home"), "ctrl+home", true, "ctrl+Home"),
        );
    }

    #[test]
    fn resolve_user_binding_ctrl_comma_ctrl_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keybinding(
            &mapper(),
            &Keybinding::new(vec![
                scan_code_chord(true, false, false, false, ScanCode::Comma),
                key_code_chord(true, false, false, false, KeyCode::Slash),
            ])
            .unwrap(),
            &[expected(
                "Ctrl+, Ctrl+§",
                "Control+, Control+§",
                None,
                "ctrl+oem_comma ctrl+oem_2",
                false,
                true,
                &[Some("ctrl+,"), Some("ctrl+/")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_ctrl() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::Ctrl),
            &expected("Ctrl", "Control", None, "ctrl", true, false, &[None], &[Some(SingleModifierChord::Ctrl)]),
        );
    }
}

mod en_us {
    use super::*;

    fn mapper() -> WindowsKeyboardMapper {
        create_keyboard_mapper(true, "win_en_us", false)
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "win_en_us.txt");
    }

    #[test]
    fn resolve_keybinding_ctrl_k_ctrl_backslash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::Backslash.bits()),
            &[expected(
                "Ctrl+K Ctrl+\\",
                "Control+K Control+\\",
                None,
                "ctrl+k ctrl+\\",
                true,
                true,
                &[Some("ctrl+K"), Some("ctrl+\\")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_user_binding_ctrl_comma_ctrl_slash() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keybinding(
            &mapper(),
            &Keybinding::new(vec![
                scan_code_chord(true, false, false, false, ScanCode::Comma),
                key_code_chord(true, false, false, false, KeyCode::Slash),
            ])
            .unwrap(),
            &[expected(
                "Ctrl+, Ctrl+/",
                "Control+, Control+/",
                None,
                "ctrl+, ctrl+/",
                true,
                true,
                &[Some("ctrl+,"), Some("ctrl+/")],
                &[None, None],
            )],
        );
    }

    #[test]
    fn resolve_user_binding_ctrl_comma() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keybinding(
            &mapper(),
            &Keybinding::new(vec![scan_code_chord(true, false, false, false, ScanCode::Comma)]).unwrap(),
            &[ctrl_chord("Ctrl+,", "Control+,", Some("Ctrl+,"), "ctrl+,", true, "ctrl+,")],
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_ctrl() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::Ctrl),
            &expected("Ctrl", "Control", None, "ctrl", true, false, &[None], &[Some(SingleModifierChord::Ctrl)]),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_shift() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(false, true, false, false, false, KeyCode::Shift),
            &expected("Shift", "Shift", None, "shift", true, false, &[None], &[Some(SingleModifierChord::Shift)]),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_alt() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(false, false, true, false, false, KeyCode::Alt),
            &expected("Alt", "Alt", None, "alt", true, false, &[None], &[Some(SingleModifierChord::Alt)]),
        );
    }

    #[test]
    fn resolve_keyboard_event_single_modifier_meta() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(false, false, false, true, false, KeyCode::Meta),
            &expected("Windows", "Windows", None, "win", true, false, &[None], &[Some(SingleModifierChord::Meta)]),
        );
    }

    #[test]
    fn resolve_keyboard_event_only_modifiers_ctrl_shift() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, true, false, false, false, KeyCode::Shift),
            &expected("Ctrl+Shift", "Control+Shift", None, "ctrl+shift", true, false, &[None], &[None]),
        );
    }

    #[test]
    fn resolve_keyboard_event_map_alt_gr_to_ctrl_alt_alt_gr_z() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mapper = create_keyboard_mapper(true, "win_en_us", true);

        assert_resolve_keyboard_event(
            &mapper,
            &event(false, false, false, false, true, KeyCode::KeyZ),
            &ctrl_chord("Ctrl+Alt+Z", "Control+Alt+Z", Some("Ctrl+Alt+Z"), "ctrl+alt+z", true, "ctrl+alt+Z"),
        );
    }
}

mod por_ptb {
    use super::*;

    fn mapper() -> WindowsKeyboardMapper {
        create_keyboard_mapper(false, "win_por_ptb", false)
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "win_por_ptb.txt");
    }

    #[test]
    fn resolve_keyboard_event_ctrl_intl_ro() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::AbntC1),
            &ctrl_chord("Ctrl+/", "Control+/", Some("Ctrl+ABNT_C1"), "ctrl+abnt_c1", false, "ctrl+ABNT_C1"),
        );
    }

    #[test]
    fn resolve_keyboard_event_ctrl_numpad_comma() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_resolve_keyboard_event(
            &mapper(),
            &event(true, false, false, false, false, KeyCode::AbntC2),
            &ctrl_chord("Ctrl+.", "Control+.", Some("Ctrl+ABNT_C2"), "ctrl+abnt_c2", false, "ctrl+ABNT_C2"),
        );
    }
}

mod ru {
    use super::*;

    fn mapper() -> WindowsKeyboardMapper {
        create_keyboard_mapper(false, "win_ru", false)
    }

    #[test]
    fn mapping() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_mapping(WRITE_FILE_IF_DIFFERENT, &mapper(), "win_ru.txt");
    }

    #[test]
    fn issue_24361_resolve_keybinding_ctrl_k_ctrl_k() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        assert_binary_resolve_keybinding(
            &mapper(),
            key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), KeyMod::CTRL_CMD | KeyCode::KeyK.bits()),
            &[expected(
                "Ctrl+K Ctrl+K",
                "Control+K Control+K",
                None,
                "ctrl+k ctrl+k",
                true,
                true,
                &[Some("ctrl+K"), Some("ctrl+K")],
                &[None, None],
            )],
        );
    }
}

mod misc {
    use super::*;

    fn key_mapping(vkey: &str, value: &str, with_shift: &str) -> WindowsKeyMapping {
        WindowsKeyMapping {
            vkey: vkey.to_string(),
            value: value.to_string(),
            with_shift: with_shift.to_string(),
            with_alt_gr: String::new(),
            with_shift_alt_gr: String::new(),
        }
    }

    #[test]
    fn issue_23513_toggle_sidebar_visibility_and_go_to_line_display_same_key_mapping_in_arabic_keyboard() {
        let _guard = ensure_no_disposables_are_leaked_in_test_suite();
        let mapper = new_mapper(
            false,
            &[
                ("KeyB".to_string(), key_mapping("VK_B", "لا", "لآ")),
                ("KeyG".to_string(), key_mapping("VK_G", "ل", "لأ")),
            ],
            false,
        );

        assert_binary_resolve_keybinding(
            &mapper,
            KeyMod::CTRL_CMD | KeyCode::KeyB.bits(),
            &[ctrl_chord("Ctrl+B", "Control+B", Some("Ctrl+B"), "ctrl+b", true, "ctrl+B")],
        );
    }
}
