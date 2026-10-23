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

use je_base::event::Event;
use je_base::keybindings::{Keybinding, KeyCodeChord};
use je_base::lifecycle::{DisposableRef, plain_disposable};
use je_base::promise::Promise;
use je_platform::contextkey::{IContextKeyService, IContextKeyServiceTarget};
use je_platform::keybinding::keybinding::{IKeybindingService, KeyboardEvent, KeybindingsSchemaContribution};
use je_platform::keybinding::keybinding_resolver::ResolutionResult;

/// O `MockKeybindingService` do `mockKeybindingService.ts`: não tem atalho nenhum e resolve pelo layout US.
pub struct MockKeybindingService;

impl IKeybindingService for MockKeybindingService {
    fn in_chord_mode(&self) -> bool {
        false
    }
    fn on_did_update_keybindings(&self) -> Event<()> {
        Event::none()
    }
    fn resolve_keybinding(&self, keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>> {
        USLayoutResolvedKeybinding::resolve_keybinding(keybinding, os())
    }
    fn resolve_keyboard_event(&self, e: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding> {
        let chord = KeyCodeChord::new(e.ctrl_key, e.shift_key, e.alt_key, e.meta_key, e.key_code);
        self.resolve_keybinding(&chord.to_keybinding()).remove(0)
    }
    fn resolve_user_binding(&self, _user_binding: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        Vec::new()
    }
    fn dispatch_event(&self, _e: &KeyboardEvent, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> bool {
        false
    }
    fn soft_dispatch(&self, _keyboard_event: &KeyboardEvent, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> ResolutionResult {
        ResolutionResult::NoMatchingKb
    }
    fn enable_keybinding_hold_mode(&self, _command_id: &str) -> Option<Promise<()>> {
        None
    }
    fn dispatch_by_user_settings_label(&self, _user_settings_label: &str, _target: Option<Rc<dyn IContextKeyServiceTarget>>) {}
    fn lookup_keybindings(&self, _command_id: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        Vec::new()
    }
    fn lookup_keybinding(&self, _command_id: &str, _context: Option<&dyn IContextKeyService>, _enforce_context_check: bool) -> Option<Rc<dyn ResolvedKeybinding>> {
        None
    }
    fn get_default_keybindings_content(&self) -> String {
        String::new()
    }
    fn get_default_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        Vec::new()
    }
    fn get_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        Vec::new()
    }
    fn custom_keybindings_count(&self) -> usize {
        0
    }
    fn might_produce_printable_character(&self, _event: &KeyboardEvent) -> bool {
        false
    }
    fn register_schema_contribution(&self, _contribution: Rc<dyn KeybindingsSchemaContribution>) -> DisposableRef {
        plain_disposable(|| {})
    }
    fn toggle_logging(&self) -> bool {
        false
    }
    fn append_keybinding(&self, label: &str, _command_id: Option<&str>, _context: Option<&dyn IContextKeyService>, _enforce_context_check: bool) -> String {
        label.to_owned()
    }
    fn dump_debug_info(&self) -> String {
        String::new()
    }
    fn dump_debug_info_json(&self) -> String {
        String::new()
    }
}
