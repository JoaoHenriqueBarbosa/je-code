//! Porte de `src/vs/editor/contrib/codeAction/test/browser/codeActionKeybindingResolver.test.ts` (suíte
//! `CodeActionKeybindingResolver`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `createMockKeyBindingService(items)` é um literal `<IKeybindingService>{ getKeybindings }`: o
//!   [`FixedKeybindingService`] implementa o trait inteiro, devolvendo os itens em `get_keybindings` e o neutro (nada,
//!   falso, vazio) no resto, que o literal do original não tem e o resolvedor nunca chama.
//! - `createCodeActionKeybinding(keycode, command, commandArgs)` monta o `ResolvedKeybindingItem` com o
//!   `USLayoutResolvedKeybinding` de um acorde `KeyCodeChord(false, true, false, false, keycode)` no Linux.
//! - O `assert.strictEqual` entre atalhos resolvidos compara a identidade do objeto: é `Rc::ptr_eq`
//!   ([`assert_same_keybinding`]).
//! - `async function` do caso sem `await` é uma função comum.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::key_codes::KeyCode;
use je_base::keybindings::{KeyCodeChord, Keybinding, ResolvedKeybinding};
use je_base::lifecycle::{DisposableRef, plain_disposable};
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_editor::code_action::code_action::{ORGANIZE_IMPORTS_COMMAND_ID, REFACTOR_COMMAND_ID};
use je_editor::code_action::code_action_keybinding_resolver::CodeActionKeybindingResolver;
use je_editor::code_action::languages::CodeAction;
use je_editor::code_action::types::CodeActionKind;
use je_platform::contextkey::{IContextKeyService, IContextKeyServiceTarget};
use je_platform::keybinding::keybinding_resolver::ResolutionResult;
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use je_platform::keybinding::us_layout_resolved_keybinding::USLayoutResolvedKeybinding;
use je_platform::keybinding::{IKeybindingService, KeyboardEvent, KeybindingsSchemaContribution};

/// `<IKeybindingService>{ getKeybindings: () => items }`.
struct FixedKeybindingService {
    items: Vec<Rc<ResolvedKeybindingItem>>,
}

impl IKeybindingService for FixedKeybindingService {
    fn in_chord_mode(&self) -> bool {
        false
    }

    fn on_did_update_keybindings(&self) -> Event<()> {
        Event::none()
    }

    fn resolve_keybinding(&self, _keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>> {
        Vec::new()
    }

    fn resolve_keyboard_event(&self, _keyboard_event: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding> {
        panic!("o literal do teste não tem resolveKeyboardEvent")
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

    fn lookup_keybinding(
        &self,
        _command_id: &str,
        _context: Option<&dyn IContextKeyService>,
        _enforce_context_check: bool,
    ) -> Option<Rc<dyn ResolvedKeybinding>> {
        None
    }

    fn get_default_keybindings_content(&self) -> String {
        String::new()
    }

    fn get_default_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        Vec::new()
    }

    fn get_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        self.items.clone()
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

    fn append_keybinding(
        &self,
        label: &str,
        _command_id: Option<&str>,
        _context: Option<&dyn IContextKeyService>,
        _enforce_context_check: bool,
    ) -> String {
        label.to_owned()
    }

    fn dump_debug_info(&self) -> String {
        String::new()
    }

    fn dump_debug_info_json(&self) -> String {
        String::new()
    }
}

fn create_mock_key_binding_service(items: &[&Rc<ResolvedKeybindingItem>]) -> Rc<dyn IKeybindingService> {
    Rc::new(FixedKeybindingService { items: items.iter().map(|item| Rc::clone(item)).collect() })
}

fn create_code_action_keybinding(key_code: KeyCode, command: &str, command_args: Dynamic) -> Rc<ResolvedKeybindingItem> {
    let resolved: Rc<dyn ResolvedKeybinding> = Rc::new(USLayoutResolvedKeybinding::from_chords(
        vec![KeyCodeChord::new(false, true, false, false, key_code)],
        OperatingSystem::Linux,
    ));
    ResolvedKeybindingItem::new(Some(resolved), Some(command), command_args, None, false, None, false, false)
}

fn kind_args(kind: &str) -> Dynamic {
    Dynamic::object([("kind", Dynamic::string(kind))])
}

/// `assert.strictEqual(resolver(action), expected)`: a mesma instância, ou nenhuma das duas.
fn assert_same_keybinding(actual: Option<Rc<dyn ResolvedKeybinding>>, expected: Option<&Rc<dyn ResolvedKeybinding>>) {
    match (actual, expected) {
        (None, None) => {}
        (Some(actual), Some(expected)) => assert!(Rc::ptr_eq(&actual, expected), "atalhos diferentes"),
        (actual, expected) => panic!("esperado {}, obtido {}", expected.is_some(), actual.is_some()),
    }
}

fn resolved(item: &Rc<ResolvedKeybindingItem>) -> &Rc<dyn ResolvedKeybinding> {
    item.resolved_keybinding.as_ref().expect("o item tem o atalho resolvido")
}

fn action_of_kind(kind: &str) -> CodeAction {
    CodeAction::with_kind("", Some(kind))
}

fn make_items() -> (Rc<ResolvedKeybindingItem>, Rc<ResolvedKeybindingItem>, Rc<ResolvedKeybindingItem>) {
    let refactor_keybinding =
        create_code_action_keybinding(KeyCode::KeyA, REFACTOR_COMMAND_ID, kind_args(&CodeActionKind::refactor().value));

    let refactor_extract_keybinding = create_code_action_keybinding(
        KeyCode::KeyB,
        REFACTOR_COMMAND_ID,
        kind_args(&CodeActionKind::refactor().append(&["extract"]).value),
    );

    let organize_imports_keybinding = create_code_action_keybinding(KeyCode::KeyC, ORGANIZE_IMPORTS_COMMAND_ID, Dynamic::Undefined);
    (refactor_keybinding, refactor_extract_keybinding, organize_imports_keybinding)
}

// suite('CodeActionKeybindingResolver')

#[test]
fn should_match_refactor_keybindings() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (refactor_keybinding, _, _) = make_items();
    let resolver = CodeActionKeybindingResolver::new(create_mock_key_binding_service(&[&refactor_keybinding])).get_resolver();

    assert_same_keybinding(resolver(&CodeAction::new("")), None);

    assert_same_keybinding(resolver(&action_of_kind(&CodeActionKind::refactor().value)), Some(resolved(&refactor_keybinding)));

    assert_same_keybinding(
        resolver(&action_of_kind(&CodeActionKind::refactor().append(&["extract"]).value)),
        Some(resolved(&refactor_keybinding)),
    );

    assert_same_keybinding(resolver(&action_of_kind(&CodeActionKind::quick_fix().value)), None);
}

#[test]
fn should_prefer_most_specific_keybinding() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (refactor_keybinding, refactor_extract_keybinding, organize_imports_keybinding) = make_items();
    let resolver = CodeActionKeybindingResolver::new(create_mock_key_binding_service(&[
        &refactor_keybinding,
        &refactor_extract_keybinding,
        &organize_imports_keybinding,
    ]))
    .get_resolver();

    assert_same_keybinding(resolver(&action_of_kind(&CodeActionKind::refactor().value)), Some(resolved(&refactor_keybinding)));

    assert_same_keybinding(
        resolver(&action_of_kind(&CodeActionKind::refactor().append(&["extract"]).value)),
        Some(resolved(&refactor_extract_keybinding)),
    );
}

#[test]
fn organize_imports_should_still_return_a_keybinding_even_though_it_does_not_have_args() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (refactor_keybinding, refactor_extract_keybinding, organize_imports_keybinding) = make_items();
    let resolver = CodeActionKeybindingResolver::new(create_mock_key_binding_service(&[
        &refactor_keybinding,
        &refactor_extract_keybinding,
        &organize_imports_keybinding,
    ]))
    .get_resolver();

    assert_same_keybinding(
        resolver(&action_of_kind(&CodeActionKind::source_organize_imports().value)),
        Some(resolved(&organize_imports_keybinding)),
    );
}
