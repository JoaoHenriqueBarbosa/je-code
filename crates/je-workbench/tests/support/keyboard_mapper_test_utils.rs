//! Porte de `src/vs/workbench/services/keybinding/test/node/keyboardMapperTestUtils.ts`:
//! `IResolvedKeybinding`, `assertResolveKeyboardEvent`, `assertResolveKeybinding` e `assertMapping`.
//! `readRawMapping` não tem porte aqui: ele executa o arquivo `.js` do mapa de teclas, e cada teste que o usa lê o
//! formato (um literal de objeto) por conta própria.

use std::fs;
use std::path::PathBuf;

use je_base::keybindings::{Keybinding, ResolvedKeybinding, SingleModifierChord};
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
