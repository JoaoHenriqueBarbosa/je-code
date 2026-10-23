//! O `keybindingsTestUtils.ts` (`createUSLayoutResolvedKeybinding`) e a igualdade estrutural de itens
//! resolvidos que o `assert.deepStrictEqual` dos testes do `KeybindingResolver` faz sobre as listas.
#![allow(dead_code)]

use std::rc::Rc;

use je_base::keybindings::{BinaryKeybinding, ResolvedKeybinding, decode_keybinding};
use je_base::platform::{OperatingSystem, Platform};
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use je_platform::keybinding::us_layout_resolved_keybinding::USLayoutResolvedKeybinding;

/// O `OS` do `platform.ts`.
pub fn os() -> OperatingSystem {
    Platform::detect().os
}

/// `createUSLayoutResolvedKeybinding`: `0` e o que não decodifica dão `None`.
pub fn create_us_layout_resolved_keybinding(encoded: impl Into<BinaryKeybinding>, os: OperatingSystem) -> Option<Rc<dyn ResolvedKeybinding>> {
    let encoded = encoded.into();
    if encoded == BinaryKeybinding::Number(0) {
        return None;
    }
    let keybinding = decode_keybinding(encoded, os).ok().flatten()?;
    USLayoutResolvedKeybinding::resolve_keybinding(&keybinding, os).into_iter().next()
}

/// A igualdade estrutural de dois itens: o que o `deepStrictEqual` compara nos campos públicos.
pub fn items_equal(a: &ResolvedKeybindingItem, b: &ResolvedKeybindingItem) -> bool {
    let kb = |item: &ResolvedKeybindingItem| {
        item.resolved_keybinding.as_ref().map(|k| (k.get_user_settings_label(), k.get_dispatch_chords(), k.get_label()))
    };
    kb(a) == kb(b)
        && a.chords == b.chords
        && a.bubble == b.bubble
        && a.command == b.command
        && a.command_args == b.command_args
        && a.when.as_ref().map(|w| w.serialize()) == b.when.as_ref().map(|w| w.serialize())
        && a.is_default == b.is_default
        && a.extension_id == b.extension_id
        && a.is_builtin_extension == b.is_builtin_extension
        && a.system_wide == b.system_wide
}

pub fn lists_equal(a: &[Rc<ResolvedKeybindingItem>], b: &[Rc<ResolvedKeybindingItem>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| items_equal(x, y))
}
