//! Porte de `src/vs/base/browser/keyboardEvent.ts`: o [`StandardKeyboardEvent`], que normaliza um `keydown` do navegador
//! para um [`KeyCode`] e os modificadores, e as funções que o acompanham.
//!
//! Divergências:
//! - **Evento.** O `KeyboardEvent` do navegador é o [`KeyboardEvent`] daqui: só os campos que o original lê (`keyCode`,
//!   `charCode`, os modificadores, `code`, `key`, `isComposing`, o estado `AltGraph` de `getModifierState` e o alvo). O
//!   `preventDefault` e o `stopPropagation` marcam o evento ([`KeyboardEvent::default_prevented`] e
//!   [`KeyboardEvent::propagation_stopped`]), que é o que o navegador guarda.
//! - **Navegador.** `browser.isFirefox`, `browser.isWebKit` e `platform.isMacintosh`/`isLinux` são lidos uma vez de
//!   [`KeyboardEnvironment::host`]. Na renderização nativa o motor é o do Chromium, que o `userAgent` anuncia como
//!   `AppleWebKit` (por isso `isWebKit` vale e `isFirefox` não). [`StandardKeyboardEvent::with_environment`] aceita
//!   outro ambiente.

use std::cell::Cell;
use std::sync::OnceLock;

use crate::key_codes::{KeyCode, KeyCodeUtils, KeyMod, event_key_code, is_modifier_key};
use crate::keybindings::KeyCodeChord;
use crate::platform::Platform;

use super::dom::Node;

/// O `KeyboardEvent` do navegador, nos campos que o `keyboardEvent.ts` lê.
#[derive(Clone, Debug, Default)]
pub struct KeyboardEvent {
    /// `keyCode` (legado, mas é o que o `StandardKeyboardEvent` lê).
    pub key_code: i32,
    /// `charCode`: não zero nos eventos `keypress`.
    pub char_code: u32,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    /// `getModifierState('AltGraph')`.
    pub alt_graph_key: bool,
    pub code: String,
    pub key: String,
    /// `isComposing`: há uma composição de IME em andamento.
    pub is_composing: bool,
    /// `target`.
    pub target: Option<Node>,
    /// `defaultPrevented`.
    pub default_prevented: Cell<bool>,
    /// Se o `stopPropagation` foi chamado.
    pub propagation_stopped: Cell<bool>,
}

/// O que o `keyboardEvent.ts` consulta de `browser` e de `platform`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyboardEnvironment {
    pub is_firefox: bool,
    pub is_web_kit: bool,
    pub is_macintosh: bool,
    pub is_linux: bool,
}

impl KeyboardEnvironment {
    /// O ambiente do processo: o sistema detectado por [`Platform::detect`], com o motor do Chromium.
    pub fn host() -> Self {
        static HOST: OnceLock<KeyboardEnvironment> = OnceLock::new();
        *HOST.get_or_init(|| {
            let platform = Platform::detect();
            Self { is_firefox: false, is_web_kit: true, is_macintosh: platform.is_macintosh, is_linux: platform.is_linux }
        })
    }
}

/// `extractKeyCode(e)`.
fn extract_key_code(e: &KeyboardEvent, env: KeyboardEnvironment) -> KeyCode {
    if e.char_code != 0 {
        // "keypress" events mostly
        let Some(character) = char::from_u32(e.char_code & 0xFFFF) else { return KeyCode::Unknown };
        return KeyCodeUtils::from_string(&character.to_uppercase().collect::<String>());
    }

    let key_code = e.key_code;

    // browser quirks
    if key_code == 3 {
        return KeyCode::PauseBreak;
    } else if env.is_firefox {
        match key_code {
            59 => return KeyCode::Semicolon,
            60 if env.is_linux => return KeyCode::IntlBackslash,
            61 => return KeyCode::Equal,
            // based on: https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent/keyCode#numpad_keys
            107 => return KeyCode::NumpadAdd,
            109 => return KeyCode::NumpadSubtract,
            173 => return KeyCode::Minus,
            224 if env.is_macintosh => return KeyCode::Meta,
            _ => {}
        }
    } else if env.is_web_kit {
        if env.is_macintosh && key_code == 93 {
            // the two meta keys in the Mac have different key codes (91 and 93)
            return KeyCode::Meta;
        } else if !env.is_macintosh && key_code == 92 {
            return KeyCode::Meta;
        }
    }

    // cross browser keycodes:
    event_key_code(key_code).filter(|code| *code != KeyCode::Unknown).unwrap_or(KeyCode::Unknown)
}

/// `printKeyboardEvent(e)`.
pub fn print_keyboard_event(e: &KeyboardEvent) -> String {
    let modifiers = modifier_names(e.ctrl_key, e.shift_key, e.alt_key, e.meta_key);
    format!("modifiers: [{modifiers}], code: {}, keyCode: {}, key: {}", e.code, e.key_code, e.key)
}

/// `printStandardKeyboardEvent(e)`.
pub fn print_standard_keyboard_event(e: &StandardKeyboardEvent) -> String {
    let modifiers = modifier_names(e.ctrl_key, e.shift_key, e.alt_key, e.meta_key);
    format!(
        "modifiers: [{modifiers}], code: {}, keyCode: {} ('{}')",
        e.code,
        e.key_code.0,
        KeyCodeUtils::to_string(e.key_code).unwrap_or("undefined")
    )
}

fn modifier_names(ctrl_key: bool, shift_key: bool, alt_key: bool, meta_key: bool) -> String {
    [(ctrl_key, "ctrl"), (shift_key, "shift"), (alt_key, "alt"), (meta_key, "meta")]
        .into_iter()
        .filter_map(|(pressed, name)| pressed.then_some(name))
        .collect::<Vec<_>>()
        .join(",")
}

/// `hasModifierKeys(keyStatus)`.
pub fn has_modifier_keys(ctrl_key: bool, shift_key: bool, alt_key: bool, meta_key: bool) -> bool {
    ctrl_key || shift_key || alt_key || meta_key
}

/// `StandardKeyboardEvent`.
#[derive(Clone, Debug)]
pub struct StandardKeyboardEvent {
    pub browser_event: KeyboardEvent,
    pub target: Option<Node>,
    pub ctrl_key: bool,
    pub shift_key: bool,
    pub alt_key: bool,
    pub meta_key: bool,
    pub alt_graph_key: bool,
    pub key_code: KeyCode,
    pub code: String,
    as_keybinding: u32,
    as_key_code_chord: KeyCodeChord,
    env: KeyboardEnvironment,
}

impl StandardKeyboardEvent {
    /// O ambiente é o do processo ([`KeyboardEnvironment::host`]).
    pub fn new(source: KeyboardEvent) -> Self {
        Self::with_environment(source, KeyboardEnvironment::host())
    }

    pub fn with_environment(source: KeyboardEvent, env: KeyboardEnvironment) -> Self {
        // Browsers are inconsistent while an IME composition is in flight: most keystrokes arrive as
        // `keyCode: 229` (which maps to `KEY_IN_COMPOSITION`), but some platform/IME combinations
        // report the real key code for keys the IME owns - notably the Enter that commits a
        // composition, but also Space, Escape and the arrows used to pick candidates. Normalize to
        // `KEY_IN_COMPOSITION` so that "the IME owns this keystroke" has a single representation
        // that `equals()`, direct `keyCode` readers and keybinding resolution all understand,
        // instead of acting on a key the user never directed at the application.
        let key_code = if source.is_composing { KeyCode::KeyInComposition } else { extract_key_code(&source, env) };

        let ctrl_key = source.ctrl_key || key_code == KeyCode::Ctrl;
        let alt_key = source.alt_key || key_code == KeyCode::Alt;
        let shift_key = source.shift_key || key_code == KeyCode::Shift;
        let meta_key = source.meta_key || key_code == KeyCode::Meta;

        let key = if is_modifier_key(key_code) { KeyCode::Unknown } else { key_code };

        let ctrl_key_mod = if env.is_macintosh { KeyMod::WIN_CTRL } else { KeyMod::CTRL_CMD };
        let meta_key_mod = if env.is_macintosh { KeyMod::CTRL_CMD } else { KeyMod::WIN_CTRL };
        let mut as_keybinding = 0;
        if ctrl_key {
            as_keybinding |= ctrl_key_mod;
        }
        if alt_key {
            as_keybinding |= KeyMod::ALT;
        }
        if shift_key {
            as_keybinding |= KeyMod::SHIFT;
        }
        if meta_key {
            as_keybinding |= meta_key_mod;
        }
        as_keybinding |= key.bits();

        Self {
            target: source.target.clone(),
            alt_graph_key: source.alt_graph_key,
            code: source.code.clone(),
            as_key_code_chord: KeyCodeChord::new(ctrl_key, shift_key, alt_key, meta_key, key),
            browser_event: source,
            ctrl_key,
            shift_key,
            alt_key,
            meta_key,
            key_code,
            as_keybinding,
            env,
        }
    }

    /// O ambiente com que o evento foi lido.
    pub fn environment(&self) -> KeyboardEnvironment {
        self.env
    }

    pub fn prevent_default(&self) {
        self.browser_event.default_prevented.set(true);
    }

    pub fn stop_propagation(&self) {
        self.browser_event.propagation_stopped.set(true);
    }

    pub fn to_key_code_chord(&self) -> KeyCodeChord {
        self.as_key_code_chord
    }

    /// `equals(keybinding)`: igualdade exata da tecla e dos modificadores, na codificação binária de `KeyMod | KeyCode`.
    pub fn equals(&self, other: u32) -> bool {
        self.as_keybinding == other
    }
}
