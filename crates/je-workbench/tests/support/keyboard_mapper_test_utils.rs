//! Porte parcial de `src/vs/workbench/services/keybinding/test/node/keyboardMapperTestUtils.ts`: só
//! `IResolvedKeybinding`, `assertResolveKeyboardEvent` e `assertResolveKeybinding`. `readRawMapping` e
//! `assertMapping` leem arquivos de mapa de teclas que os testes do mapeador de reserva não usam.

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
