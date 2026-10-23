//! Porte de `src/vs/workbench/contrib/terminal/test/browser/capabilities/terminalCapabilityStore.test.ts`
//! (13 casos, na ordem do original).
//!
//! O `{} as unknown as ICwdDetectionCapability` do original é um objeto sem membros: os stubs abaixo
//! implementam o trait e falham se qualquer membro for consultado, como o `{}` falharia no JavaScript.
//! O `ensureNoDisposablesAreLeakedInTestSuite` vira o `LeakGuard` de cada fixture, criado antes de tudo e
//! conferido por último, depois que o [`Drop`] do fixture descarta o store do teste.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_platform::terminal::capabilities::capabilities::{
    AnyTerminalCapabilityChangeEvent, CommandDetection, CwdDetection, ICommandDetectionCapability, ICommandInvalidationRequest, ICurrentPartialCommand, ICwdDetectionCapability,
    IHandleCommandOptions, INaiveCwdDetectionCapability, IPromptInputModel, ISerializedCommandDetectionCapability, ITerminalCapabilityStore,
    ITerminalCommand, NaiveCwdDetection, TerminalCapability, TerminalCommandConfidence, TerminalCommandForLine,
};
use je_platform::terminal::capabilities::terminal_capability_store::{TerminalCapabilityStore, TerminalCapabilityStoreMultiplexer};

macro_rules! no_members {
    () => {
        unreachable!("o `{{}}` do original não tem membros")
    };
}

struct CwdStub;

impl ICwdDetectionCapability for CwdStub {
    fn capability_type(&self) -> TerminalCapability {
        no_members!()
    }
    fn on_did_change_cwd(&self) -> Event<String> {
        no_members!()
    }
    fn cwds(&self) -> Vec<String> {
        no_members!()
    }
    fn is_trusted(&self) -> bool {
        no_members!()
    }
    fn get_cwd(&self) -> String {
        no_members!()
    }
    fn update_cwd(&self, _cwd: &str, _is_trusted: Option<bool>) {
        no_members!()
    }
}

struct NaiveCwdStub;

impl INaiveCwdDetectionCapability for NaiveCwdStub {
    fn capability_type(&self) -> TerminalCapability {
        no_members!()
    }
    fn on_did_change_cwd(&self) -> Event<String> {
        no_members!()
    }
    fn get_cwd(&self) -> Promise<String> {
        no_members!()
    }
}

struct CommandStub;

impl ICommandDetectionCapability for CommandStub {
    fn capability_type(&self) -> TerminalCapability {
        no_members!()
    }
    fn prompt_input_model(&self) -> Rc<dyn IPromptInputModel> {
        no_members!()
    }
    fn commands(&self) -> Vec<Rc<dyn ITerminalCommand>> {
        no_members!()
    }
    fn executing_command(&self) -> Option<String> {
        no_members!()
    }
    fn executing_command_object(&self) -> Option<Rc<dyn ITerminalCommand>> {
        no_members!()
    }
    fn executing_command_confidence(&self) -> Option<TerminalCommandConfidence> {
        no_members!()
    }
    fn cwd(&self) -> Option<String> {
        no_members!()
    }
    fn has_rich_command_detection(&self) -> bool {
        no_members!()
    }
    fn current_command(&self) -> Option<Rc<dyn ICurrentPartialCommand>> {
        no_members!()
    }
    fn on_command_started(&self) -> Event<Rc<dyn ITerminalCommand>> {
        no_members!()
    }
    fn on_command_finished(&self) -> Event<Rc<dyn ITerminalCommand>> {
        no_members!()
    }
    fn on_command_executed(&self) -> Event<Rc<dyn ITerminalCommand>> {
        no_members!()
    }
    fn on_command_invalidated(&self) -> Event<Vec<Rc<dyn ITerminalCommand>>> {
        no_members!()
    }
    fn on_current_command_invalidated(&self) -> Event<ICommandInvalidationRequest> {
        no_members!()
    }
    fn on_set_rich_command_detection(&self) -> Event<bool> {
        no_members!()
    }
    fn set_continuation_prompt(&self, _value: &str) {
        no_members!()
    }
    fn set_prompt_terminator(&self, _value: &str, _last_prompt_line: &str) {
        no_members!()
    }
    fn set_cwd(&self, _value: &str) {
        no_members!()
    }
    fn set_is_windows_pty(&self, _value: bool) {
        no_members!()
    }
    fn set_is_command_storage_disabled(&self) {
        no_members!()
    }
    fn get_cwd_for_line(&self, _line: f64) -> Option<String> {
        no_members!()
    }
    fn get_command_for_line(&self, _line: f64) -> Option<TerminalCommandForLine> {
        no_members!()
    }
    fn clear_commands(&self) {
        no_members!()
    }
    fn handle_prompt_start(&self, _options: Option<&IHandleCommandOptions>) {
        no_members!()
    }
    fn handle_continuation_start(&self) {
        no_members!()
    }
    fn handle_continuation_end(&self) {
        no_members!()
    }
    fn handle_right_prompt_start(&self) {
        no_members!()
    }
    fn handle_right_prompt_end(&self) {
        no_members!()
    }
    fn handle_command_start(&self, _options: Option<&IHandleCommandOptions>) {
        no_members!()
    }
    fn handle_command_executed(&self, _options: Option<&IHandleCommandOptions>) {
        no_members!()
    }
    fn handle_command_finished(&self, _exit_code: Option<f64>, _options: Option<&IHandleCommandOptions>) {
        no_members!()
    }
    fn set_has_rich_command_detection(&self, _value: bool) {
        no_members!()
    }
    fn set_command_line(&self, _command_line: &str, _is_trusted: bool) {
        no_members!()
    }
    fn set_next_command_id(&self, _command: &str, _command_id: &str) {
        no_members!()
    }
    fn serialize(&self) -> ISerializedCommandDetectionCapability {
        no_members!()
    }
    fn deserialize(&self, _serialized: &ISerializedCommandDetectionCapability) {
        no_members!()
    }
}

type Events = Rc<RefCell<Vec<TerminalCapability>>>;

/// `store.add(x.onDidAddCapability(e => events.push(e.id)))`.
fn record(event: Event<AnyTerminalCapabilityChangeEvent>, store: &DisposableStore) -> Events {
    let events: Events = Rc::default();
    let sink = events.clone();
    store.add(event.listen(move |e| sink.borrow_mut().push(e.id)));
    events
}

/// `assertEvents(actual, expected)`: confere e esvazia.
fn assert_events(actual: &Events, expected: &[TerminalCapability]) {
    assert_eq!(*actual.borrow(), expected);
    actual.borrow_mut().clear();
}

fn sorted(mut items: Vec<TerminalCapability>) -> Vec<TerminalCapability> {
    items.sort();
    items
}

fn items_of(store: &dyn ITerminalCapabilityStore) -> Vec<TerminalCapability> {
    store.items().collect()
}

/// `strictEqual(getEvent(), getEvent())`.
fn assert_memoized<T: 'static>(get_event: impl Fn() -> Rc<Event<T>>) {
    assert!(Rc::ptr_eq(&get_event(), &get_event()));
}

/// `store.add(getEvent()(() => { }))`.
fn subscribe_and_track<T: 'static>(store: &DisposableStore, get_event: impl Fn() -> Rc<Event<T>>) {
    store.add(get_event().listen(|_| {}));
}

/// `getDerivedEventGetters(capabilityStore)`: chama `$check($args..., getter)` para cada um dos eventos
/// derivados (os tipos dos eventos diferem, então é um macro e não uma lista de fechos).
macro_rules! for_each_derived_event {
    ($capability_store:expr, $check:path $(, $arg:expr)*) => {{
        let capability_store = &$capability_store;
        $check($($arg,)* || capability_store.on_did_change_capabilities());
        $check($($arg,)* || capability_store.on_did_add_command_detection_capability());
        $check($($arg,)* || capability_store.on_did_remove_command_detection_capability());
        $check($($arg,)* || capability_store.on_did_add_cwd_detection_capability());
        $check($($arg,)* || capability_store.on_did_remove_cwd_detection_capability());
    }};
}

// #region suite('TerminalCapabilityStore')

struct StoreFixture {
    store: Rc<DisposableStore>,
    capability_store: Rc<TerminalCapabilityStore>,
    add_events: Events,
    remove_events: Events,
    _leaks: LeakGuard,
}

impl StoreFixture {
    fn new() -> Self {
        let leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let store = DisposableStore::new();
        let capability_store = store.add_rc(TerminalCapabilityStore::new());
        let add_events = record(capability_store.on_did_add_capability(), &store);
        let remove_events = record(capability_store.on_did_remove_capability(), &store);
        Self { store, capability_store, add_events, remove_events, _leaks: leaks }
    }
}

impl Drop for StoreFixture {
    fn drop(&mut self) {
        self.store.dispose();
    }
}

#[test]
fn should_fire_events_when_capabilities_are_added() {
    let f = StoreFixture::new();
    assert_events(&f.add_events, &[]);
    f.capability_store.add::<CwdDetection>(Rc::new(CwdStub));
    assert_events(&f.add_events, &[TerminalCapability::CwdDetection]);
}

#[test]
fn should_fire_events_when_capabilities_are_removed() {
    let f = StoreFixture::new();
    assert_events(&f.remove_events, &[]);
    f.capability_store.add::<CwdDetection>(Rc::new(CwdStub));
    assert_events(&f.remove_events, &[]);
    f.capability_store.remove(TerminalCapability::CwdDetection);
    assert_events(&f.remove_events, &[TerminalCapability::CwdDetection]);
}

#[test]
fn has_should_return_whether_a_capability_is_present() {
    let f = StoreFixture::new();
    assert!(!f.capability_store.has(TerminalCapability::CwdDetection));
    f.capability_store.add::<CwdDetection>(Rc::new(CwdStub));
    assert!(f.capability_store.has(TerminalCapability::CwdDetection));
    f.capability_store.remove(TerminalCapability::CwdDetection);
    assert!(!f.capability_store.has(TerminalCapability::CwdDetection));
}

#[test]
fn items_should_reflect_current_state() {
    let f = StoreFixture::new();
    assert_eq!(items_of(&*f.capability_store), Vec::<TerminalCapability>::new());
    f.capability_store.add::<CwdDetection>(Rc::new(CwdStub));
    assert_eq!(items_of(&*f.capability_store), vec![TerminalCapability::CwdDetection]);
    f.capability_store.add::<NaiveCwdDetection>(Rc::new(NaiveCwdStub));
    assert_eq!(items_of(&*f.capability_store), vec![TerminalCapability::CwdDetection, TerminalCapability::NaiveCwdDetection]);
    f.capability_store.remove(TerminalCapability::CwdDetection);
    assert_eq!(items_of(&*f.capability_store), vec![TerminalCapability::NaiveCwdDetection]);
}

#[test]
fn ensure_events_are_memoized() {
    let f = StoreFixture::new();
    for_each_derived_event!(f.capability_store, assert_memoized);
}

#[test]
fn ensure_events_are_cleaned_up() {
    let f = StoreFixture::new();
    for_each_derived_event!(f.capability_store, subscribe_and_track, &f.store);
}

// #endregion

// #region suite('TerminalCapabilityStoreMultiplexer')

struct MultiplexerFixture {
    store: Rc<DisposableStore>,
    multiplexer: Rc<TerminalCapabilityStoreMultiplexer>,
    store1: Rc<TerminalCapabilityStore>,
    store2: Rc<TerminalCapabilityStore>,
    add_events: Events,
    remove_events: Events,
    _leaks: LeakGuard,
}

impl MultiplexerFixture {
    fn new() -> Self {
        let leaks = ensure_no_disposables_are_leaked_in_test_suite();
        let store = DisposableStore::new();
        let multiplexer = store.add_rc(TerminalCapabilityStoreMultiplexer::new());
        let add_events = record(multiplexer.on_did_add_capability(), &store);
        let remove_events = record(multiplexer.on_did_remove_capability(), &store);
        let store1 = store.add_rc(TerminalCapabilityStore::new());
        let store2 = store.add_rc(TerminalCapabilityStore::new());
        Self { store, multiplexer, store1, store2, add_events, remove_events, _leaks: leaks }
    }
}

impl Drop for MultiplexerFixture {
    fn drop(&mut self) {
        self.store.dispose();
    }
}

#[test]
fn should_fire_events_when_capabilities_are_enabled() {
    let f = MultiplexerFixture::new();
    assert_events(&f.add_events, &[]);
    f.multiplexer.add(f.store1.clone());
    f.multiplexer.add(f.store2.clone());
    f.store1.add::<CwdDetection>(Rc::new(CwdStub));
    assert_events(&f.add_events, &[TerminalCapability::CwdDetection]);
    f.store2.add::<NaiveCwdDetection>(Rc::new(NaiveCwdStub));
    assert_events(&f.add_events, &[TerminalCapability::NaiveCwdDetection]);
}

#[test]
fn should_fire_events_when_capabilities_are_disabled() {
    let f = MultiplexerFixture::new();
    assert_events(&f.remove_events, &[]);
    f.multiplexer.add(f.store1.clone());
    f.multiplexer.add(f.store2.clone());
    f.store1.add::<CwdDetection>(Rc::new(CwdStub));
    f.store2.add::<NaiveCwdDetection>(Rc::new(NaiveCwdStub));
    assert_events(&f.remove_events, &[]);
    f.store1.remove(TerminalCapability::CwdDetection);
    assert_events(&f.remove_events, &[TerminalCapability::CwdDetection]);
    f.store2.remove(TerminalCapability::NaiveCwdDetection);
    assert_events(&f.remove_events, &[TerminalCapability::NaiveCwdDetection]);
}

#[test]
fn should_fire_events_when_stores_are_added() {
    let f = MultiplexerFixture::new();
    assert_events(&f.add_events, &[]);
    f.store1.add::<CwdDetection>(Rc::new(CwdStub));
    assert_events(&f.add_events, &[]);
    f.store2.add::<NaiveCwdDetection>(Rc::new(NaiveCwdStub));
    f.multiplexer.add(f.store1.clone());
    f.multiplexer.add(f.store2.clone());
    assert_events(&f.add_events, &[TerminalCapability::CwdDetection, TerminalCapability::NaiveCwdDetection]);
}

#[test]
fn items_should_return_items_from_all_stores() {
    let f = MultiplexerFixture::new();
    assert_eq!(sorted(items_of(&*f.multiplexer)), Vec::<TerminalCapability>::new());
    f.multiplexer.add(f.store1.clone());
    f.multiplexer.add(f.store2.clone());
    f.store1.add::<CwdDetection>(Rc::new(CwdStub));
    assert_eq!(sorted(items_of(&*f.multiplexer)), sorted(vec![TerminalCapability::CwdDetection]));
    f.store1.add::<CommandDetection>(Rc::new(CommandStub));
    f.store2.add::<NaiveCwdDetection>(Rc::new(NaiveCwdStub));
    assert_eq!(
        sorted(items_of(&*f.multiplexer)),
        sorted(vec![TerminalCapability::CwdDetection, TerminalCapability::CommandDetection, TerminalCapability::NaiveCwdDetection])
    );
    f.store2.remove(TerminalCapability::NaiveCwdDetection);
    assert_eq!(
        sorted(items_of(&*f.multiplexer)),
        sorted(vec![TerminalCapability::CwdDetection, TerminalCapability::CommandDetection])
    );
}

#[test]
fn multiplexer_has_should_return_whether_a_capability_is_present() {
    let f = MultiplexerFixture::new();
    assert!(!f.multiplexer.has(TerminalCapability::CwdDetection));
    f.multiplexer.add(f.store1.clone());
    f.store1.add::<CwdDetection>(Rc::new(CwdStub));
    assert!(f.multiplexer.has(TerminalCapability::CwdDetection));
    f.store1.remove(TerminalCapability::CwdDetection);
    assert!(!f.multiplexer.has(TerminalCapability::CwdDetection));
}

#[test]
fn multiplexer_ensure_events_are_memoized() {
    let f = MultiplexerFixture::new();
    for_each_derived_event!(f.multiplexer, assert_memoized);
}

#[test]
fn multiplexer_ensure_events_are_cleaned_up() {
    let f = MultiplexerFixture::new();
    for_each_derived_event!(f.multiplexer, subscribe_and_track, &f.store);
}

// #endregion
