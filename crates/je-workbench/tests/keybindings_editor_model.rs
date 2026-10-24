//! Porte de `src/vs/workbench/services/preferences/test/browser/keybindingsEditorModel.test.ts` (suíte `KeybindingsEditorModel`),
//! caso a caso e na mesma ordem.
//!
//! Divergências:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` e o `TestInstantiationService` viram o [`Fixture`]: ele guarda o `LeakGuard`,
//!   os dois dublês (`instantiationService.stub(IKeybindingService, {})` e `stub(IExtensionService, {...})`) e o modelo
//!   criado com o `OS` da máquina. Um método que o original não stuba falha com `panic!` (no original seria um `TypeError`
//!   por chamar `undefined`).
//! - `instantiationService.stub(IKeybindingService, 'getKeybindings', () => items)` é [`StubKeybindingService::set_keybindings`]
//!   (e `set_default_keybindings` para `getDefaultKeybindings`); `prepareKeybindingService` faz os dois.
//! - `instantiationService.createInstance(KeybindingsEditorModel, OperatingSystem.X)` é [`Fixture::recreate`]; o modelo
//!   anterior continua registrado no guarda, como no original.
//! - `testObject.resolve(new Map<string, string>())` é [`Fixture::resolve`] (o `resolve_with_action_labels` com o mapa
//!   vazio); `testObject.fetch(x)` é `fetch(x, false)`, o valor padrão de `sortByPrecedence`.
//! - `Partial<IExtensionDescription>` é um [`ExtensionDescription`] completo com só o identificador e o `displayName`
//!   preenchidos (o resto é neutro): o modelo lê só esses dois campos.
//! - `assert.deepStrictEqual(x.firstPart, { metaKey: true })` compara com um [`KeybindingMatch`] cujas chaves ausentes
//!   são `false` (o `{}` é o valor padrão), ver a divergência de `preferences.rs`.
//! - `registerAction2(class extends Action2 {...})` é [`TitledAction`], e `CommandsRegistry.registerCommand(id, () => {})`
//!   registra um handler que devolve `undefined`.
//! - O `.filter(element => element.keybindingItem.command === command)` repetido nos casos é [`fetch_for_command`].
//! - Cada teste de Rust roda numa thread própria, com os registros (`CommandsRegistry`, `MenuRegistry`) novos; o original
//!   compartilha os registros do processo, e por isso filtra pelo comando.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

#[path = "../../je-platform/tests/common/keybinding.rs"]
#[allow(dead_code)]
mod platform_keybinding;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::key_codes::KeyCode;
use je_base::keybindings::{KeyCodeChord, Keybinding, ResolvedKeybinding};
use je_base::lifecycle::DisposableRef;
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_base::uuid::generate_uuid;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_instantiation::ServicesAccessor;
use je_platform::action::{Category, CommandAction, CommandActionSource, CommandActionTitle, Title};
use je_platform::actions::{Action2, Action2Options, menu_registry, register_action2};
use je_platform::commands::{CommandHandler, commands_registry};
use je_platform::contextkey::{ContextKeyExpr, IContextKeyService, IContextKeyServiceTarget};
use je_platform::keybinding::keybinding::{IKeybindingService, KeybindingsSchemaContribution, KeyboardEvent};
use je_platform::keybinding::keybinding_resolver::ResolutionResult;
use je_platform::keybinding::resolved_keybinding_item::ResolvedKeybindingItem;
use je_platform::keybinding::us_layout_resolved_keybinding::USLayoutResolvedKeybinding;
use je_workbench::services::extensions::extensions::{ActivationKind, IExtensionDescription, IExtensionService};
use je_workbench::services::preferences::keybindings_editor_model::KeybindingsEditorModel;
use je_workbench::services::preferences::preferences::{IKeybindingItemEntry, KeybindingMatch, KeybindingMatches};
use platform_keybinding::os;

/// `interface Modifiers`.
#[derive(Clone, Copy, Default)]
struct Modifiers {
    meta_key: bool,
    ctrl_key: bool,
    alt_key: bool,
    shift_key: bool,
}

impl Modifiers {
    const NONE: Modifiers = Modifiers { meta_key: false, ctrl_key: false, alt_key: false, shift_key: false };
}

/// `{ keyCode: KeyCode; modifiers?: Modifiers }`.
#[derive(Clone, Copy)]
struct ChordSpec {
    key_code: KeyCode,
    modifiers: Option<Modifiers>,
}

/// `{ keyCode }`.
fn key(key_code: KeyCode) -> ChordSpec {
    ChordSpec { key_code, modifiers: None }
}

/// `{ keyCode, modifiers }`.
fn key_with(key_code: KeyCode, modifiers: Modifiers) -> ChordSpec {
    ChordSpec { key_code, modifiers: Some(modifiers) }
}

/// Os argumentos nomeados de `aResolvedKeybindingItem`.
#[derive(Default)]
struct ItemSpec {
    command: Option<String>,
    when: Option<&'static str>,
    is_default: Option<bool>,
    first_chord: Option<ChordSpec>,
    second_chord: Option<ChordSpec>,
    extension_id: Option<String>,
}

const WHEN_CONTEXTS: &str = "whenContext1 && whenContext2";

/// `'x' + uuid.generateUuid()`.
fn prefixed_uuid(prefix: &str) -> String {
    format!("{prefix}{}", generate_uuid())
}

fn not_stubbed(method: &str) -> ! {
    panic!("{method} não foi stubado")
}

/// `instantiationService.stub(IKeybindingService, {})` mais os `getKeybindings` e `getDefaultKeybindings` que os casos stubam.
struct StubKeybindingService {
    keybindings: RefCell<Option<Vec<Rc<ResolvedKeybindingItem>>>>,
    default_keybindings: RefCell<Option<Vec<Rc<ResolvedKeybindingItem>>>>,
}

impl StubKeybindingService {
    fn new() -> Rc<Self> {
        Rc::new(Self { keybindings: RefCell::new(None), default_keybindings: RefCell::new(None) })
    }

    fn set_keybindings(&self, keybindings: Vec<Rc<ResolvedKeybindingItem>>) {
        *self.keybindings.borrow_mut() = Some(keybindings);
    }

    fn set_default_keybindings(&self, keybindings: Vec<Rc<ResolvedKeybindingItem>>) {
        *self.default_keybindings.borrow_mut() = Some(keybindings);
    }
}

impl IKeybindingService for StubKeybindingService {
    fn in_chord_mode(&self) -> bool {
        not_stubbed("inChordMode")
    }
    fn on_did_update_keybindings(&self) -> Event<()> {
        not_stubbed("onDidUpdateKeybindings")
    }
    fn resolve_keybinding(&self, _keybinding: &Keybinding) -> Vec<Rc<dyn ResolvedKeybinding>> {
        not_stubbed("resolveKeybinding")
    }
    fn resolve_keyboard_event(&self, _keyboard_event: &KeyboardEvent) -> Rc<dyn ResolvedKeybinding> {
        not_stubbed("resolveKeyboardEvent")
    }
    fn resolve_user_binding(&self, _user_binding: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        not_stubbed("resolveUserBinding")
    }
    fn dispatch_event(&self, _e: &KeyboardEvent, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> bool {
        not_stubbed("dispatchEvent")
    }
    fn soft_dispatch(&self, _keyboard_event: &KeyboardEvent, _target: Option<Rc<dyn IContextKeyServiceTarget>>) -> ResolutionResult {
        not_stubbed("softDispatch")
    }
    fn enable_keybinding_hold_mode(&self, _command_id: &str) -> Option<Promise<()>> {
        not_stubbed("enableKeybindingHoldMode")
    }
    fn dispatch_by_user_settings_label(&self, _user_settings_label: &str, _target: Option<Rc<dyn IContextKeyServiceTarget>>) {
        not_stubbed("dispatchByUserSettingsLabel")
    }
    fn lookup_keybindings(&self, _command_id: &str) -> Vec<Rc<dyn ResolvedKeybinding>> {
        not_stubbed("lookupKeybindings")
    }
    fn lookup_keybinding(&self, _command_id: &str, _context: Option<&dyn IContextKeyService>, _enforce_context_check: bool) -> Option<Rc<dyn ResolvedKeybinding>> {
        not_stubbed("lookupKeybinding")
    }
    fn get_default_keybindings_content(&self) -> String {
        not_stubbed("getDefaultKeybindingsContent")
    }
    fn get_default_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        self.default_keybindings.borrow().clone().unwrap_or_else(|| not_stubbed("getDefaultKeybindings"))
    }
    fn get_keybindings(&self) -> Vec<Rc<ResolvedKeybindingItem>> {
        self.keybindings.borrow().clone().unwrap_or_else(|| not_stubbed("getKeybindings"))
    }
    fn custom_keybindings_count(&self) -> usize {
        not_stubbed("customKeybindingsCount")
    }
    fn might_produce_printable_character(&self, _event: &KeyboardEvent) -> bool {
        not_stubbed("mightProducePrintableCharacter")
    }
    fn register_schema_contribution(&self, _contribution: Rc<dyn KeybindingsSchemaContribution>) -> DisposableRef {
        not_stubbed("registerSchemaContribution")
    }
    fn toggle_logging(&self) -> bool {
        not_stubbed("_toggleLogging")
    }
    fn append_keybinding(&self, _label: &str, _command_id: Option<&str>, _context: Option<&dyn IContextKeyService>, _enforce_context_check: bool) -> String {
        not_stubbed("appendKeybinding")
    }
    fn dump_debug_info(&self) -> String {
        not_stubbed("dumpDebugInfo")
    }
    fn dump_debug_info_json(&self) -> String {
        not_stubbed("dumpDebugInfoJSON")
    }
}

/// `instantiationService.stub(IExtensionService, { whenInstalledExtensionsRegistered, get extensions() })`.
struct StubExtensionService {
    extensions: RefCell<Vec<ExtensionDescription>>,
}

impl StubExtensionService {
    fn new() -> Rc<Self> {
        Rc::new(Self { extensions: RefCell::new(Vec::new()) })
    }
}

impl IExtensionService for StubExtensionService {
    fn activate_by_event(&self, _activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        not_stubbed("activateByEvent")
    }

    fn activation_event_is_done(&self, _activation_event: &str) -> bool {
        not_stubbed("activationEventIsDone")
    }

    fn get_extension(&self, _id: &str) -> Promise<Option<IExtensionDescription>> {
        not_stubbed("getExtension")
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        not_stubbed("onDidRegisterExtensions")
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        self.extensions.borrow().clone()
    }

    fn activate_by_event(&self, _activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        not_stubbed("activateByEvent")
    }
}

/// O `setup` da suíte: o guarda de vazamentos, os dublês, o modelo e o comando `command_without_keybinding`.
struct Fixture {
    guard: LeakGuard,
    keybinding_service: Rc<StubKeybindingService>,
    extension_service: Rc<StubExtensionService>,
    test_object: Rc<KeybindingsEditorModel>,
}

impl Fixture {
    fn new() -> Self {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let keybinding_service = StubKeybindingService::new();
        let extension_service = StubExtensionService::new();
        let test_object = guard.add_rc(KeybindingsEditorModel::new(os(), keybinding_service.clone(), extension_service.clone()));

        guard.add(register_noop_command("command_without_keybinding"));
        Self { guard, keybinding_service, extension_service, test_object }
    }

    /// `testObject = disposables.add(instantiationService.createInstance(KeybindingsEditorModel, os))`.
    fn recreate(&mut self, os: OperatingSystem) {
        self.test_object = self.guard.add_rc(KeybindingsEditorModel::new(os, self.keybinding_service.clone(), self.extension_service.clone()));
    }

    /// `await testObject.resolve(new Map<string, string>())`.
    fn resolve(&self) {
        let _ = self.test_object.resolve_with_action_labels(&HashMap::new());
    }

    /// `prepareKeybindingService`.
    fn prepare_keybinding_service(&self, keybinding_items: Vec<Rc<ResolvedKeybindingItem>>) -> Vec<Rc<ResolvedKeybindingItem>> {
        self.keybinding_service.set_keybindings(keybinding_items.clone());
        self.keybinding_service.set_default_keybindings(keybinding_items.clone());
        keybinding_items
    }
}

/// `CommandsRegistry.registerCommand(id, () => { })`.
fn register_noop_command(id: &str) -> DisposableRef {
    let handler: CommandHandler = Rc::new(|_, _| Dynamic::Undefined);
    commands_registry().register_command_handler(id, Some(handler))
}

/// A classe anônima de `registerCommandWithTitle`: `title: { value: title, original: title }` e `f1: true`.
struct TitledAction {
    command: String,
    title: String,
}

impl Action2 for TitledAction {
    fn desc(&self) -> Action2Options {
        let title = Title::Localized(CommandActionTitle { value: self.title.clone(), original: self.title.clone(), mnemonic_title: None });
        Action2Options { command: CommandAction::new(self.command.as_str(), title), menu: Vec::new(), keybinding: Vec::new(), f1: true }
    }

    fn run(&self, _accessor: &ServicesAccessor, _args: &[Dynamic]) {}
}

fn register_command_with_title(fixture: &Fixture, command: &str, title: &str) {
    fixture.guard.add(register_action2(Rc::new(TitledAction { command: command.to_string(), title: title.to_string() })));
}

fn assert_keybinding_items(actual: &[Rc<ResolvedKeybindingItem>], expected: &[Rc<ResolvedKeybindingItem>]) {
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_keybinding_item(actual, expected);
    }
}

fn assert_keybinding_item(actual: &ResolvedKeybindingItem, expected: &ResolvedKeybindingItem) {
    assert_eq!(actual.command, expected.command);
    match &actual.when {
        Some(actual_when) => {
            let expected_when = expected.when.as_ref().expect("o esperado tem when");
            assert_eq!(actual_when.serialize(), expected_when.serialize());
        }
        None => assert!(expected.when.is_none()),
    }
    assert_eq!(actual.is_default, expected.is_default);

    match &actual.resolved_keybinding {
        Some(actual_keybinding) => {
            let expected_keybinding = expected.resolved_keybinding.as_ref().expect("o esperado tem resolvedKeybinding");
            assert_eq!(actual_keybinding.get_label(), expected_keybinding.get_label());
        }
        None => assert!(expected.resolved_keybinding.is_none()),
    }
}

fn a_resolved_keybinding_item(spec: ItemSpec) -> Rc<ResolvedKeybindingItem> {
    let a_simple_keybinding = |chord: ChordSpec| {
        let modifiers = chord.modifiers.unwrap_or(Modifiers::NONE);
        KeyCodeChord::new(modifiers.ctrl_key, modifiers.shift_key, modifiers.alt_key, modifiers.meta_key, chord.key_code)
    };
    let mut chords: Vec<KeyCodeChord> = Vec::new();
    if let Some(first_chord) = spec.first_chord {
        chords.push(a_simple_keybinding(first_chord));
        if let Some(second_chord) = spec.second_chord {
            chords.push(a_simple_keybinding(second_chord));
        }
    }
    let keybinding: Option<Rc<dyn ResolvedKeybinding>> = if chords.is_empty() { None } else { Some(Rc::new(USLayoutResolvedKeybinding::from_chords(chords, os()))) };
    let command = spec.command.as_deref().filter(|command| !command.is_empty()).unwrap_or("some command");
    ResolvedKeybindingItem::new(
        keybinding,
        Some(command),
        Dynamic::Null,
        spec.when.filter(|when| !when.is_empty()).and_then(|when| ContextKeyExpr::deserialize(Some(when))),
        spec.is_default.unwrap_or(true),
        spec.extension_id.as_deref(),
        false,
        false,
    )
}

fn as_resolved_keybinding_items(keybinding_entries: Vec<IKeybindingItemEntry>, keep_unassigned: bool) -> Vec<Rc<ResolvedKeybindingItem>> {
    keybinding_entries
        .into_iter()
        .filter(|keybinding_entry| keep_unassigned || keybinding_entry.keybinding_item.keybinding.is_some())
        .map(|entry| entry.keybinding_item.keybinding_item.clone())
        .collect()
}

/// `testObject.fetch(query).filter(element => element.keybindingItem.command === command)`.
fn fetch_for_command(fixture: &Fixture, query: &str, command: &str) -> Vec<IKeybindingItemEntry> {
    fixture.test_object.fetch(query, false).into_iter().filter(|element| element.keybinding_item.command == command).collect()
}

fn keybinding_matches_of(entry: &IKeybindingItemEntry) -> &KeybindingMatches {
    entry.keybinding_matches.as_ref().expect("keybindingMatches")
}

/// `Partial<IExtensionDescription>` com `identifier` e `displayName`.
fn an_extension(identifier: &str, display_name: &str) -> ExtensionDescription {
    ExtensionDescription {
        id: identifier.to_string(),
        identifier: ExtensionIdentifier::new(identifier),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: String::new(),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest { display_name: Some(display_name.to_string()), ..ExtensionManifest::default() },
    }
}

#[test]
fn fetch_returns_default_keybindings() {
    let f = Fixture::new();
    let expected = f.prepare_keybinding_service(vec![
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("a")), first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("b")), first_chord: Some(key(KeyCode::Escape)), second_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
    ]);

    f.resolve();
    let actuals = as_resolved_keybinding_items(f.test_object.fetch("", false), false);
    assert_keybinding_items(&actuals, &expected);
}

#[test]
fn fetch_returns_distinct_keybindings() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = f.prepare_keybinding_service(vec![
        a_resolved_keybinding_item(ItemSpec { command: Some(command.clone()), first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(command), first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
    ]);

    f.resolve();
    let actuals = as_resolved_keybinding_items(f.test_object.fetch("", false), false);
    assert_keybinding_items(&actuals, &[expected[0].clone()]);
}

#[test]
fn fetch_returns_default_keybindings_at_the_top() {
    let f = Fixture::new();
    let expected = f.prepare_keybinding_service(vec![
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("a")), first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("b")), first_chord: Some(key(KeyCode::Escape)), second_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
    ]);

    f.resolve();
    let actuals = as_resolved_keybinding_items(f.test_object.fetch("", false).into_iter().take(2).collect(), true);
    assert_keybinding_items(&actuals, &expected);
}

#[test]
fn fetch_returns_default_keybindings_sorted_by_command_id() {
    let f = Fixture::new();
    let keybindings = f.prepare_keybinding_service(vec![
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("b")), first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("c")), first_chord: Some(key(KeyCode::Escape)), second_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("a")), first_chord: Some(key(KeyCode::Backspace)), ..ItemSpec::default() }),
    ]);
    let expected = [keybindings[2].clone(), keybindings[0].clone(), keybindings[1].clone()];

    f.resolve();
    let actuals = as_resolved_keybinding_items(f.test_object.fetch("", false), false);
    assert_keybinding_items(&actuals, &expected);
}

#[test]
fn fetch_returns_user_keybinding_first_if_default_and_user_has_same_id() {
    let f = Fixture::new();
    let same_id = prefixed_uuid("b");
    let keybindings = f.prepare_keybinding_service(vec![
        a_resolved_keybinding_item(ItemSpec { command: Some(same_id.clone()), first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(same_id), first_chord: Some(key(KeyCode::Escape)), second_chord: Some(key(KeyCode::Escape)), is_default: Some(false), ..ItemSpec::default() }),
    ]);
    let expected = [keybindings[1].clone(), keybindings[0].clone()];

    f.resolve();
    let actuals = as_resolved_keybinding_items(f.test_object.fetch("", false), false);
    assert_keybinding_items(&actuals, &expected);
}

#[test]
fn fetch_returns_keybinding_with_titles_first() {
    let f = Fixture::new();
    let keybindings = f.prepare_keybinding_service(vec![
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("a")), first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("b")), first_chord: Some(key(KeyCode::Escape)), second_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("c")), first_chord: Some(key(KeyCode::Escape)), second_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("d")), first_chord: Some(key(KeyCode::Escape)), second_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
    ]);

    register_command_with_title(&f, keybindings[1].command.as_deref().expect("command"), "B Title");
    register_command_with_title(&f, keybindings[3].command.as_deref().expect("command"), "A Title");

    let expected = [keybindings[3].clone(), keybindings[1].clone(), keybindings[0].clone(), keybindings[2].clone()];
    f.keybinding_service.set_keybindings(keybindings.clone());
    f.keybinding_service.set_default_keybindings(keybindings.clone());

    f.resolve();
    let actuals = as_resolved_keybinding_items(f.test_object.fetch("", false), false);
    assert_keybinding_items(&actuals, &expected);
}

#[test]
fn fetch_returns_keybinding_with_user_first_if_title_and_id_matches() {
    let f = Fixture::new();
    let same_id = prefixed_uuid("b");
    let keybindings = f.prepare_keybinding_service(vec![
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("a")), first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(same_id.clone()), first_chord: Some(key(KeyCode::Escape)), second_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("c")), first_chord: Some(key(KeyCode::Escape)), second_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(same_id), first_chord: Some(key(KeyCode::Escape)), is_default: Some(false), ..ItemSpec::default() }),
    ]);

    register_command_with_title(&f, keybindings[1].command.as_deref().expect("command"), "Same Title");
    let expected = [keybindings[3].clone(), keybindings[1].clone(), keybindings[0].clone(), keybindings[2].clone()];

    f.resolve();
    let actuals = as_resolved_keybinding_items(f.test_object.fetch("", false), false);
    assert_keybinding_items(&actuals, &expected);
}

#[test]
fn fetch_returns_default_keybindings_sorted_by_precedence() {
    let f = Fixture::new();
    let expected = f.prepare_keybinding_service(vec![
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("b")), first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("c")), first_chord: Some(key(KeyCode::Escape)), second_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() }),
        a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("a")), first_chord: Some(key(KeyCode::Backspace)), ..ItemSpec::default() }),
    ]);

    f.resolve();
    let actuals = as_resolved_keybinding_items(f.test_object.fetch("", true), false);
    assert_keybinding_items(&actuals, &expected);
}

#[test]
fn convert_keybinding_without_title_to_entry() {
    let f = Fixture::new();
    let expected = a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("a")), first_chord: Some(key(KeyCode::Escape)), when: Some("context1 && context2"), ..ItemSpec::default() });
    f.prepare_keybinding_service(vec![expected.clone()]);

    f.resolve();
    let actual = f.test_object.fetch("", false).remove(0);
    assert_eq!(Some(actual.keybinding_item.command.as_str()), expected.command.as_deref());
    assert_eq!(actual.keybinding_item.command_label, "");
    assert_eq!(actual.keybinding_item.command_default_label, None);
    let expected_keybinding = expected.resolved_keybinding.as_ref().expect("resolvedKeybinding");
    assert_eq!(actual.keybinding_item.keybinding.as_ref().expect("keybinding").get_aria_label(), expected_keybinding.get_aria_label());
    assert_eq!(actual.keybinding_item.when, expected.when.as_ref().expect("when").serialize());
}

#[test]
fn convert_keybinding_with_title_to_entry() {
    let f = Fixture::new();
    let expected = a_resolved_keybinding_item(ItemSpec { command: Some(prefixed_uuid("a")), first_chord: Some(key(KeyCode::Escape)), when: Some("context1 && context2"), ..ItemSpec::default() });
    f.prepare_keybinding_service(vec![expected.clone()]);
    register_command_with_title(&f, expected.command.as_deref().expect("command"), "Some Title");

    f.resolve();
    let actual = f.test_object.fetch("", false).remove(0);
    assert_eq!(Some(actual.keybinding_item.command.as_str()), expected.command.as_deref());
    assert_eq!(actual.keybinding_item.command_label, "Some Title");
    assert_eq!(actual.keybinding_item.command_default_label, None);
    let expected_keybinding = expected.resolved_keybinding.as_ref().expect("resolvedKeybinding");
    assert_eq!(actual.keybinding_item.keybinding.as_ref().expect("keybinding").get_aria_label(), expected_keybinding.get_aria_label());
    assert_eq!(actual.keybinding_item.when, expected.when.as_ref().expect("when").serialize());
}

#[test]
fn convert_without_title_and_binding_to_entry() {
    let f = Fixture::new();
    f.guard.add(register_noop_command("command_without_keybinding"));
    f.prepare_keybinding_service(Vec::new());

    f.resolve();
    let actual = f.test_object.fetch("", false).into_iter().find(|element| element.keybinding_item.command == "command_without_keybinding").expect("a entrada do comando");
    assert_eq!(actual.keybinding_item.command, "command_without_keybinding");
    assert_eq!(actual.keybinding_item.command_label, "");
    assert_eq!(actual.keybinding_item.command_default_label, None);
    assert!(actual.keybinding_item.keybinding.is_none());
    assert_eq!(actual.keybinding_item.when, "");
}

#[test]
fn convert_with_title_and_without_binding_to_entry() {
    let f = Fixture::new();
    let id = prefixed_uuid("a");
    register_command_with_title(&f, &id, "some title");
    f.prepare_keybinding_service(Vec::new());

    f.resolve();
    let actual = fetch_for_command(&f, "", &id).remove(0);
    assert_eq!(actual.keybinding_item.command, id);
    assert_eq!(actual.keybinding_item.command_label, "some title");
    assert_eq!(actual.keybinding_item.command_default_label, None);
    assert!(actual.keybinding_item.keybinding.is_none());
    assert_eq!(actual.keybinding_item.when, "");
}

#[test]
fn filter_by_command_id() {
    let f = Fixture::new();
    let id = format!("workbench.action.view.size.{}", generate_uuid());
    register_command_with_title(&f, &id, "some title");
    f.prepare_keybinding_service(Vec::new());

    f.resolve();
    let actual = fetch_for_command(&f, "workbench action view size", &id);
    assert!(!actual.is_empty());
}

#[test]
fn filter_by_command_title() {
    let f = Fixture::new();
    let id = prefixed_uuid("a");
    register_command_with_title(&f, &id, "Increase view size");
    f.prepare_keybinding_service(Vec::new());

    f.resolve();
    let actual = fetch_for_command(&f, "increase size", &id);
    assert!(!actual.is_empty());
}

#[test]
fn filter_by_system_source() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec { command: Some(command.clone()), first_chord: Some(key(KeyCode::Escape)), when: Some("context1 && context2"), ..ItemSpec::default() });
    f.prepare_keybinding_service(vec![expected]);

    f.resolve();
    let actual = fetch_for_command(&f, "system", &command);
    assert!(!actual.is_empty());
}

#[test]
fn filter_by_user_source() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec { command: Some(command.clone()), first_chord: Some(key(KeyCode::Escape)), when: Some("context1 && context2"), is_default: Some(false), ..ItemSpec::default() });
    f.prepare_keybinding_service(vec![expected]);

    f.resolve();
    let actual = fetch_for_command(&f, "user", &command);
    assert!(!actual.is_empty());
}

#[test]
fn filter_by_default_source_with_source_prefix() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec { command: Some(command.clone()), first_chord: Some(key(KeyCode::Escape)), when: Some("context1 && context2"), is_default: Some(true), ..ItemSpec::default() });
    f.prepare_keybinding_service(vec![expected]);

    f.resolve();
    let actual = fetch_for_command(&f, "@source: default", &command);
    assert!(!actual.is_empty());
}

#[test]
fn filter_by_user_source_with_source_prefix() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec { command: Some(command.clone()), first_chord: Some(key(KeyCode::Escape)), when: Some("context1 && context2"), is_default: Some(false), ..ItemSpec::default() });
    f.prepare_keybinding_service(vec![expected]);

    f.resolve();
    let actual = fetch_for_command(&f, "@source: user", &command);
    assert!(!actual.is_empty());
}

#[test]
fn filter_by_command_prefix_with_different_commands() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec { command: Some(command.clone()), first_chord: Some(key(KeyCode::Escape)), when: Some("context1 && context2"), is_default: Some(true), ..ItemSpec::default() });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(generate_uuid()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { alt_key: true, ..Modifiers::NONE })),
            when: Some("whenContext1 && whenContext2"),
            is_default: Some(true),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = f.test_object.fetch(&format!("@command:{command}"), false);
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].keybinding_item.command, command);
}

#[test]
fn filter_by_command_prefix_with_same_commands() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec { command: Some(command.clone()), first_chord: Some(key(KeyCode::Escape)), when: Some("context1 && context2"), is_default: Some(true), ..ItemSpec::default() });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { alt_key: true, ..Modifiers::NONE })),
            when: Some("whenContext1 && whenContext2"),
            is_default: Some(true),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = f.test_object.fetch(&format!("@command:{command}"), false);
    assert_eq!(actual.len(), 2);
    assert_eq!(actual[0].keybinding_item.command, command);
    assert_eq!(actual[1].keybinding_item.command, command);
}

#[test]
fn filter_by_when_context() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec { command: Some(command.clone()), first_chord: Some(key(KeyCode::Escape)), when: Some(WHEN_CONTEXTS), is_default: Some(false), ..ItemSpec::default() });
    f.prepare_keybinding_service(vec![expected]);

    f.resolve();
    let actual = fetch_for_command(&f, "when context", &command);
    assert!(!actual.is_empty());
}

#[test]
fn filter_by_cmd_key() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);

    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { meta_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![expected]);

    f.resolve();
    let actual = fetch_for_command(&f, "cmd", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { meta_key: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_meta_key() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);

    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { meta_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "meta", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { meta_key: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_command_key() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);

    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { meta_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { alt_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "command", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { meta_key: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_windows_key() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Windows);

    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { meta_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "windows", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { meta_key: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_alt_key() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { alt_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { meta_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "alt", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { alt_key: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_option_key() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { alt_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { meta_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "option", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { alt_key: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_ctrl_key() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "ctrl", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { ctrl_key: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_control_key() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { meta_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "control", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { ctrl_key: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_shift_key() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { meta_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "shift", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { shift_key: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_arrow() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::RightArrow, Modifiers { shift_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { meta_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "arrow", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { key_code: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_modifier_and_key() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::RightArrow, Modifiers { alt_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::RightArrow, Modifiers { meta_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "alt right", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { alt_key: true, key_code: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_key_and_modifier() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::RightArrow, Modifiers { alt_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::RightArrow, Modifiers { meta_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "right alt", &command);
    assert_eq!(0, actual.len());
}

#[test]
fn filter_by_modifiers_and_key() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { alt_key: true, meta_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { meta_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "alt cmd esc", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { alt_key: true, meta_key: true, key_code: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_modifiers_in_random_order_and_key() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { meta_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "cmd shift esc", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { meta_key: true, shift_key: true, key_code: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_first_part() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
        second_chord: Some(key(KeyCode::Delete)),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { meta_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "cmd shift esc", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { meta_key: true, shift_key: true, key_code: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_matches_in_chord_part() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
        second_chord: Some(key(KeyCode::Delete)),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { meta_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "cmd del", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { meta_key: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch { key_code: true, ..KeybindingMatch::default() });
}

#[test]
fn filter_matches_first_part_and_in_chord_part() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
        second_chord: Some(key(KeyCode::Delete)),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
            second_chord: Some(key(KeyCode::UpArrow)),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "cmd shift esc del", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { shift_key: true, meta_key: true, key_code: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch { key_code: true, ..KeybindingMatch::default() });
}

#[test]
fn filter_exact_matches() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
            second_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "\"ctrl c\"", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { ctrl_key: true, key_code: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_exact_matches_with_first_and_chord_part() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
        second_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "\"shift meta escape ctrl c\"", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { shift_key: true, meta_key: true, key_code: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch { ctrl_key: true, key_code: true, ..KeybindingMatch::default() });
}

#[test]
fn filter_exact_matches_with_first_and_chord_part_no_results() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
        second_chord: Some(key_with(KeyCode::Delete, Modifiers { meta_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
            second_chord: Some(key(KeyCode::UpArrow)),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "\"cmd shift esc del\"", &command);
    assert_eq!(0, actual.len());
}

#[test]
fn filter_matches_with_plus_separator() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
            second_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "\"control+c\"", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { ctrl_key: true, key_code: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_by_keybinding_prefix() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
            second_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "@keybinding:control+c", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { ctrl_key: true, key_code: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_matches_with_plus_separator_in_first_and_chord_parts() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
        second_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "\"shift+meta+escape ctrl+c\"", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { shift_key: true, meta_key: true, key_code: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch { key_code: true, ctrl_key: true, ..KeybindingMatch::default() });
}

#[test]
fn filter_by_keybinding_prefix_with_chord() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
        second_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "@keybinding:\"shift+meta+escape ctrl+c\"", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { shift_key: true, meta_key: true, key_code: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch { key_code: true, ctrl_key: true, ..KeybindingMatch::default() });
}

#[test]
fn filter_exact_matches_with_space_32993() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Space, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Backspace, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "\"ctrl+space\"", &command);
    assert_eq!(1, actual.len());
}

#[test]
fn filter_exact_matches_with_user_settings_label() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec { command: Some(command.clone()), first_chord: Some(key(KeyCode::DownArrow)), ..ItemSpec::default() });
    f.prepare_keybinding_service(vec![expected, a_resolved_keybinding_item(ItemSpec { command: Some("down".to_string()), first_chord: Some(key(KeyCode::Escape)), ..ItemSpec::default() })]);

    f.resolve();
    let actual = fetch_for_command(&f, "\"down\"", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { key_code: true, ..KeybindingMatch::default() });
}

#[test]
fn filter_exact_matches_also_return_chords() {
    let f = Fixture::new();
    let command = prefixed_uuid("a");
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::KeyK, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
        second_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
        when: Some(WHEN_CONTEXTS),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some(command.clone()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { shift_key: true, meta_key: true, ..Modifiers::NONE })),
            second_chord: Some(key_with(KeyCode::KeyC, Modifiers { ctrl_key: true, ..Modifiers::NONE })),
            when: Some(WHEN_CONTEXTS),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = fetch_for_command(&f, "\"control+k\"", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { ctrl_key: true, key_code: true, ..KeybindingMatch::default() });
    assert_eq!(keybinding_matches_of(&actual[0]).chord_part, KeybindingMatch::default());
}

#[test]
fn filter_modifiers_are_not_matched_when_not_completely_matched_prefix() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);
    let term = format!("alt.{}", generate_uuid());
    let command = format!("command.{term}");
    let expected = a_resolved_keybinding_item(ItemSpec { command: Some(command.clone()), first_chord: Some(key(KeyCode::Escape)), is_default: Some(false), ..ItemSpec::default() });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some("some_command".to_string()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { alt_key: true, ..Modifiers::NONE })),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = f.test_object.fetch(&term, false);
    assert_eq!(1, actual.len());
    assert_eq!(command, actual[0].keybinding_item.command);
    assert_eq!(Some(1), actual[0].command_id_matches.as_ref().map(Vec::len));
}

#[test]
fn filter_modifiers_are_not_matched_when_not_completely_matched_includes() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);
    let term = format!("abcaltdef.{}", generate_uuid());
    let command = format!("command.{term}");
    let expected = a_resolved_keybinding_item(ItemSpec { command: Some(command.clone()), first_chord: Some(key(KeyCode::Escape)), is_default: Some(false), ..ItemSpec::default() });
    f.prepare_keybinding_service(vec![
        expected,
        a_resolved_keybinding_item(ItemSpec {
            command: Some("some_command".to_string()),
            first_chord: Some(key_with(KeyCode::Escape, Modifiers { alt_key: true, ..Modifiers::NONE })),
            is_default: Some(false),
            ..ItemSpec::default()
        }),
    ]);

    f.resolve();
    let actual = f.test_object.fetch(&term, false);
    assert_eq!(1, actual.len());
    assert_eq!(command, actual[0].keybinding_item.command);
    assert_eq!(Some(1), actual[0].command_id_matches.as_ref().map(Vec::len));
}

#[test]
fn filter_modifiers_are_matched_with_complete_term() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);
    let command = format!("command.{}", generate_uuid());
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { alt_key: true, ..Modifiers::NONE })),
        is_default: Some(false),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![expected, a_resolved_keybinding_item(ItemSpec { command: Some("some_command".to_string()), first_chord: Some(key(KeyCode::Escape)), is_default: Some(false), ..ItemSpec::default() })]);

    f.resolve();
    let actual = fetch_for_command(&f, "alt", &command);
    assert_eq!(1, actual.len());
    assert_eq!(keybinding_matches_of(&actual[0]).first_part, KeybindingMatch { alt_key: true, ..KeybindingMatch::default() });
}

#[test]
fn filter_by_extension() {
    let mut f = Fixture::new();
    f.recreate(OperatingSystem::Macintosh);
    let command1 = format!("command.{}", generate_uuid());
    let command2 = format!("command.{}", generate_uuid());
    let foo = an_extension("foo", "foo bar");
    let bar = an_extension("bar", "bar foo");
    f.extension_service.extensions.borrow_mut().extend([foo.clone(), bar.clone()]);
    f.guard.add(menu_registry().add_command(CommandAction {
        category: Some(Category::from("category")),
        source: Some(CommandActionSource { id: bar.identifier.value.clone(), title: "bar foo".to_string() }),
        ..CommandAction::new(command2.as_str(), "title")
    }));
    let expected = a_resolved_keybinding_item(ItemSpec {
        command: Some(command1.clone()),
        first_chord: Some(key_with(KeyCode::Escape, Modifiers { alt_key: true, ..Modifiers::NONE })),
        is_default: Some(true),
        extension_id: Some(foo.identifier.value.clone()),
        ..ItemSpec::default()
    });
    f.prepare_keybinding_service(vec![expected, a_resolved_keybinding_item(ItemSpec { command: Some(command2.clone()), is_default: Some(true), ..ItemSpec::default() })]);

    f.resolve();
    let mut actual = f.test_object.fetch("@ext:foo", false);
    assert_eq!(1, actual.len());
    assert_eq!(actual[0].keybinding_item.command, command1);

    actual = f.test_object.fetch("@ext:\"bar foo\"", false);
    assert_eq!(1, actual.len());
    assert_eq!(actual[0].keybinding_item.command, command2);
}
