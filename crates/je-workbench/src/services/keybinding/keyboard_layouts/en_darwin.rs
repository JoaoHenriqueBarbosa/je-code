//! Porte de `src/vs/workbench/services/keybinding/browser/keyboardLayouts/en.darwin.ts`: o layout `U.S.` do macOS.
//! Ver as notas de `keyboard_layouts/mod.rs` sobre o registro e o travessão do literal.

use std::cell::Cell;

use je_platform::keyboard_layout::{KeyboardLayoutInfo, KeyboardLayoutKind, MacKeyboardLayoutInfo};

use super::contribution::KeyboardLayoutContribution;
use super::{serialized_mapping, KeyRow};
use crate::services::keybinding::keymap_info::KeymapInfoSource;

/// Um `{ id, lang, localizedName }` do macOS.
pub(super) fn mac_layout(id: &str, lang: &str, localized_name: &str, is_us_standard: bool) -> KeyboardLayoutInfo {
    KeyboardLayoutInfo {
        kind: KeyboardLayoutKind::Mac(MacKeyboardLayoutInfo { id: id.to_string(), lang: lang.to_string(), localized_name: Some(localized_name.to_string()) }),
        is_user_keyboard_layout: None,
        is_us_standard: if is_us_standard { Some(true) } else { None },
    }
}

fn k(value: &'static str, with_shift: &'static str, with_alt_gr: &'static str, with_shift_alt_gr: &'static str, mask: u32) -> Option<KeyRow> {
    Some((value, with_shift, with_alt_gr, with_shift_alt_gr, mask))
}

/// `EN_US_DARWIN_LAYOUT`.
pub fn en_us_darwin_layout() -> KeymapInfoSource {
    const E: Option<KeyRow> = None;
    KeymapInfoSource {
        layout: mac_layout("com.apple.keylayout.US", "en", "U.S.", true),
        secondary_layouts: vec![
            mac_layout("com.apple.keylayout.ABC", "en", "ABC", false),
            mac_layout("com.sogou.inputmethod.sogou.pinyin", "zh-Hans", "Pinyin - Simplified", false),
            mac_layout("com.apple.inputmethod.Kotoeri.Roman", "en", "Romaji", false),
            mac_layout("com.apple.inputmethod.Kotoeri.Japanese", "ja", "Hiragana", false),
            mac_layout("com.apple.keylayout.Australian", "en", "Australian", false),
            mac_layout("com.apple.keylayout.Canadian", "en", "Canadian English", false),
            mac_layout("com.apple.keylayout.Brazilian", "pt", "Brazilian", false),
        ],
        mapping: serialized_mapping(&[
            ("KeyA", k("a", "A", "å", "Å", 0)),
            ("KeyB", k("b", "B", "∫", "ı", 0)),
            ("KeyC", k("c", "C", "ç", "Ç", 0)),
            ("KeyD", k("d", "D", "∂", "Î", 0)),
            ("KeyE", k("e", "E", "´", "´", 4)),
            ("KeyF", k("f", "F", "ƒ", "Ï", 0)),
            ("KeyG", k("g", "G", "©", "˝", 0)),
            ("KeyH", k("h", "H", "˙", "Ó", 0)),
            ("KeyI", k("i", "I", "ˆ", "ˆ", 4)),
            ("KeyJ", k("j", "J", "∆", "Ô", 0)),
            ("KeyK", k("k", "K", "˚", "", 0)),
            ("KeyL", k("l", "L", "¬", "Ò", 0)),
            ("KeyM", k("m", "M", "µ", "Â", 0)),
            ("KeyN", k("n", "N", "˜", "˜", 4)),
            ("KeyO", k("o", "O", "ø", "Ø", 0)),
            ("KeyP", k("p", "P", "π", "∏", 0)),
            ("KeyQ", k("q", "Q", "œ", "Œ", 0)),
            ("KeyR", k("r", "R", "®", "‰", 0)),
            ("KeyS", k("s", "S", "ß", "Í", 0)),
            ("KeyT", k("t", "T", "†", "ˇ", 0)),
            ("KeyU", k("u", "U", "¨", "¨", 4)),
            ("KeyV", k("v", "V", "√", "◊", 0)),
            ("KeyW", k("w", "W", "∑", "„", 0)),
            ("KeyX", k("x", "X", "≈", "˛", 0)),
            ("KeyY", k("y", "Y", "¥", "Á", 0)),
            ("KeyZ", k("z", "Z", "Ω", "¸", 0)),
            ("Digit1", k("1", "!", "¡", "⁄", 0)),
            ("Digit2", k("2", "@", "™", "€", 0)),
            ("Digit3", k("3", "#", "£", "‹", 0)),
            ("Digit4", k("4", "$", "¢", "›", 0)),
            ("Digit5", k("5", "%", "∞", "ﬁ", 0)),
            ("Digit6", k("6", "^", "§", "ﬂ", 0)),
            ("Digit7", k("7", "&", "¶", "‡", 0)),
            ("Digit8", k("8", "*", "•", "°", 0)),
            ("Digit9", k("9", "(", "ª", "·", 0)),
            ("Digit0", k("0", ")", "º", "‚", 0)),
            ("Enter", E),
            ("Escape", E),
            ("Backspace", E),
            ("Tab", E),
            ("Space", k(" ", " ", " ", " ", 0)),
            ("Minus", k("-", "_", "\u{2013}", "\u{2014}", 0)),
            ("Equal", k("=", "+", "≠", "±", 0)),
            ("BracketLeft", k("[", "{", "“", "”", 0)),
            ("BracketRight", k("]", "}", "‘", "’", 0)),
            ("Backslash", k("\\", "|", "«", "»", 0)),
            ("Semicolon", k(";", ":", "…", "Ú", 0)),
            ("Quote", k("'", "\"", "æ", "Æ", 0)),
            ("Backquote", k("`", "~", "`", "`", 4)),
            ("Comma", k(",", "<", "≤", "¯", 0)),
            ("Period", k(".", ">", "≥", "˘", 0)),
            ("Slash", k("/", "?", "÷", "¿", 0)),
            ("CapsLock", E),
            ("F1", E),
            ("F2", E),
            ("F3", E),
            ("F4", E),
            ("F5", E),
            ("F6", E),
            ("F7", E),
            ("F8", E),
            ("F9", E),
            ("F10", E),
            ("F11", E),
            ("F12", E),
            ("Insert", E),
            ("Home", E),
            ("PageUp", E),
            ("Delete", E),
            ("End", E),
            ("PageDown", E),
            ("ArrowRight", E),
            ("ArrowLeft", E),
            ("ArrowDown", E),
            ("ArrowUp", E),
            ("NumLock", E),
            ("NumpadDivide", k("/", "/", "/", "/", 0)),
            ("NumpadMultiply", k("*", "*", "*", "*", 0)),
            ("NumpadSubtract", k("-", "-", "-", "-", 0)),
            ("NumpadAdd", k("+", "+", "+", "+", 0)),
            ("NumpadEnter", E),
            ("Numpad1", k("1", "1", "1", "1", 0)),
            ("Numpad2", k("2", "2", "2", "2", 0)),
            ("Numpad3", k("3", "3", "3", "3", 0)),
            ("Numpad4", k("4", "4", "4", "4", 0)),
            ("Numpad5", k("5", "5", "5", "5", 0)),
            ("Numpad6", k("6", "6", "6", "6", 0)),
            ("Numpad7", k("7", "7", "7", "7", 0)),
            ("Numpad8", k("8", "8", "8", "8", 0)),
            ("Numpad9", k("9", "9", "9", "9", 0)),
            ("Numpad0", k("0", "0", "0", "0", 0)),
            ("NumpadDecimal", k(".", ".", ".", ".", 0)),
            ("IntlBackslash", k("§", "±", "§", "±", 0)),
            ("ContextMenu", E),
            ("NumpadEqual", k("=", "=", "=", "=", 0)),
            ("F13", E),
            ("F14", E),
            ("F15", E),
            ("F16", E),
            ("F17", E),
            ("F18", E),
            ("F19", E),
            ("F20", E),
            ("AudioVolumeMute", E),
            ("AudioVolumeUp", k("", "=", "", "=", 0)),
            ("AudioVolumeDown", E),
            ("NumpadComma", E),
            ("IntlRo", E),
            ("KanaMode", E),
            ("IntlYen", E),
            ("ControlLeft", E),
            ("ShiftLeft", E),
            ("AltLeft", E),
            ("MetaLeft", E),
            ("ControlRight", E),
            ("ShiftRight", E),
            ("AltRight", E),
            ("MetaRight", E),
        ]),
        is_user_keyboard_layout: None,
    }
}

thread_local! {
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// `KeyboardLayoutContribution.INSTANCE.registerKeyboardLayout(EN_US_DARWIN_LAYOUT)`, o efeito da carga do módulo.
pub fn register() {
    if REGISTERED.with(|registered| registered.replace(true)) {
        return;
    }
    KeyboardLayoutContribution::with_instance(|contribution| contribution.register_keyboard_layout(en_us_darwin_layout()));
}
