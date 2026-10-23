//! Porte de `src/vs/base/common/keybindingParser.ts`: o texto de um atalho das configurações do usuário
//! (`ctrl+k ctrl+s`, `shift+[KeyA]`) para [`Keybinding`].

use crate::js_string::{is_js_whitespace, JsString};
use crate::key_codes::{KeyCodeUtils, ScanCodeUtils};
use crate::keybindings::{Chord, KeyCodeChord, Keybinding, ScanCodeChord};

/// O que sobrou do texto depois de ler os modificadores de um acorde.
struct Modifiers {
    remains: String,
    ctrl: bool,
    shift: bool,
    alt: bool,
    meta: bool,
    key: String,
}

/// `/^<name>(\+|\-)/`: o prefixo `name` seguido de `+` ou `-`.
fn starts_with_modifier(input: &str, name: &str) -> bool {
    input.strip_prefix(name).is_some_and(|rest| rest.starts_with(['+', '-']))
}

pub struct KeybindingParser;

impl KeybindingParser {
    fn read_modifiers(input: &str) -> Modifiers {
        let lowered = JsString::from(input).to_lower_case().to_string_lossy();
        let mut input = lowered.trim_matches(|c: char| u16::try_from(c as u32).is_ok_and(is_js_whitespace)).to_string();

        // ctrl, shift, alt, meta; `win` e `cmd` também ligam o meta.
        let mut flags = [false; 4];

        loop {
            let mut matched_modifier = false;
            // O original corta `'ctrl-'.length` caracteres, e o `+` ou `-` que casou tem o mesmo tamanho.
            for (name, flag) in [("ctrl", 0), ("shift", 1), ("alt", 2), ("meta", 3), ("win", 3), ("cmd", 3)] {
                if starts_with_modifier(&input, name) {
                    flags[flag] = true;
                    input = input[name.len() + 1..].to_string();
                    matched_modifier = true;
                }
            }
            if !matched_modifier {
                break;
            }
        }
        let [ctrl, shift, alt, meta] = flags;

        let (key, remains) = match input.find(' ') {
            Some(first_space_idx) if first_space_idx > 0 => (input[..first_space_idx].to_string(), input[first_space_idx..].to_string()),
            _ => (input, String::new()),
        };

        Modifiers { remains, ctrl, shift, alt, meta, key }
    }

    fn parse_chord(input: &str) -> (Chord, String) {
        let mods = Self::read_modifiers(input);
        // `/^\[([^\]]+)\]$/`
        let scan_code = mods
            .key
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
            .filter(|inner| !inner.is_empty() && !inner.contains(']'));
        if let Some(str_scan_code) = scan_code {
            let scan_code = ScanCodeUtils::lower_case_to_enum(str_scan_code);
            return (Chord::ScanCode(ScanCodeChord::new(mods.ctrl, mods.shift, mods.alt, mods.meta, scan_code)), mods.remains);
        }
        let key_code = KeyCodeUtils::from_user_settings(&mods.key);
        (Chord::KeyCode(KeyCodeChord::new(mods.ctrl, mods.shift, mods.alt, mods.meta, key_code)), mods.remains)
    }

    pub fn parse_keybinding(input: &str) -> Option<Keybinding> {
        if input.is_empty() {
            return None;
        }

        let mut chords = Vec::new();
        let mut input = input.to_string();
        while !input.is_empty() {
            let (chord, remains) = Self::parse_chord(&input);
            chords.push(chord);
            input = remains;
        }
        Keybinding::new(chords).ok()
    }
}
