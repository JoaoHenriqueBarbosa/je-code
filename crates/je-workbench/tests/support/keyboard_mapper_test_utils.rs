//! Porte de `src/vs/workbench/services/keybinding/test/node/keyboardMapperTestUtils.ts`:
//! `IResolvedKeybinding`, `assertResolveKeyboardEvent`, `assertResolveKeybinding` e `assertMapping`.
//! `readRawMapping` não tem porte aqui: ele executa o arquivo `.js` do mapa de teclas, e cada teste que o usa lê o
//! formato (um literal de objeto) por conta própria.

use std::fs;
use std::path::PathBuf;

use je_base::key_codes::{KeyCode, ScanCode};
use je_base::keybindings::{Chord, KeyCodeChord, Keybinding, ResolvedKeybinding, ScanCodeChord, SingleModifierChord};
use je_platform::keybinding::keybinding::KeyboardEvent;
use je_platform::keyboard_layout::IKeyboardMapper;

/// `IResolvedKeybinding`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedKeybindingSnapshot {
    pub label: Option<String>,
    pub aria_label: Option<String>,
    pub electron_accelerator: Option<String>,
    pub user_settings_label: Option<String>,
    pub is_wysiwyg: bool,
    pub is_multi_chord: bool,
    pub dispatch_parts: Vec<Option<String>>,
    pub single_modifier_dispatch_parts: Vec<Option<SingleModifierChord>>,
}

/// `Some(text)`: o texto esperado de um campo que o original escreve como literal.
pub fn s(text: &str) -> Option<String> {
    Some(text.to_string())
}

/// O literal `IResolvedKeybinding` do original.
#[allow(clippy::too_many_arguments)]
pub fn expected(
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

/// `new KeyCodeChord(ctrl, shift, alt, meta, keyCode)` como [`Chord`].
pub fn key_code_chord(ctrl: bool, shift: bool, alt: bool, meta: bool, key_code: KeyCode) -> Chord {
    Chord::KeyCode(KeyCodeChord::new(ctrl, shift, alt, meta, key_code))
}

/// `new ScanCodeChord(ctrl, shift, alt, meta, scanCode)` como [`Chord`].
pub fn scan_code_chord(ctrl: bool, shift: bool, alt: bool, meta: bool, scan_code: ScanCode) -> Chord {
    Chord::ScanCode(ScanCodeChord::new(ctrl, shift, alt, meta, scan_code))
}

/// Os atalhos de um acorde só que o original escreve com `[null]` nas partes de modificador único.
pub fn ctrl_chord(
    label: &str,
    aria_label: &str,
    electron: Option<&str>,
    user_settings: &str,
    wysiwyg: bool,
    dispatch: &str,
) -> ResolvedKeybindingSnapshot {
    expected(label, aria_label, electron, user_settings, wysiwyg, false, &[Some(dispatch)], &[None])
}

fn to_resolved_keybinding_snapshot(kb: &dyn ResolvedKeybinding) -> ResolvedKeybindingSnapshot {
    ResolvedKeybindingSnapshot {
        label: kb.get_label(),
        aria_label: kb.get_aria_label(),
        electron_accelerator: kb.get_electron_accelerator(),
        user_settings_label: kb.get_user_settings_label(),
        is_wysiwyg: kb.is_wysiwyg(),
        is_multi_chord: kb.has_multiple_chords(),
        dispatch_parts: kb.get_dispatch_chords(),
        single_modifier_dispatch_parts: kb.get_single_modifier_dispatch_chords(),
    }
}

/// `assertResolveKeyboardEvent`.
pub fn assert_resolve_keyboard_event(
    mapper: &dyn IKeyboardMapper,
    keyboard_event: &KeyboardEvent,
    expected: &ResolvedKeybindingSnapshot,
) {
    let resolved = mapper.resolve_keyboard_event(keyboard_event);
    let actual = to_resolved_keybinding_snapshot(resolved.as_ref());
    assert_eq!(&actual, expected);
}

/// `assertResolveKeybinding`.
pub fn assert_resolve_keybinding(
    mapper: &dyn IKeyboardMapper,
    keybinding: &Keybinding,
    expected: &[ResolvedKeybindingSnapshot],
) {
    let actual: Vec<ResolvedKeybindingSnapshot> = mapper
        .resolve_keybinding(keybinding)
        .iter()
        .map(|kb| to_resolved_keybinding_snapshot(kb.as_ref()))
        .collect();
    assert_eq!(actual, expected);
}

/// O caminho de um arquivo de `vs/workbench/services/keybinding/test/node` no `ref/vscode`
/// (`FileAccess.asFileUri(...)`).
pub fn keybinding_test_file_path(file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../ref/vscode/src/vs/workbench/services/keybinding/test/node")
        .join(file)
}

/// `assertMapping`: a saída de `dumpDebugInfo` tem de ser igual, byte a byte (com `\r\n` normalizado para `\n`), ao
/// arquivo esperado. Com `write_file_if_different`, o arquivo é reescrito com a saída atual antes de a asserção falhar.
pub fn assert_mapping(write_file_if_different: bool, mapper: &dyn IKeyboardMapper, file: &str) {
    let file_path = keybinding_test_file_path(file);
    let buff = fs::read(&file_path).unwrap_or_else(|error| panic!("não foi possível ler {}: {error}", file_path.display()));
    let expected = String::from_utf8_lossy(&buff).replace("\r\n", "\n");
    let actual = mapper.dump_debug_info().replace("\r\n", "\n");
    if actual != expected && write_file_if_different {
        fs::write(&file_path, &actual).unwrap_or_else(|error| panic!("não foi possível escrever {}: {error}", file_path.display()));
    }
    assert_eq!(actual, expected);
}
