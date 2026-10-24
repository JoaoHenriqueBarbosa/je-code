//! Porte de `src/vs/workbench/contrib/terminal/test/browser/terminalEvents.test.ts` (suíte `Terminal Events`, subsuíte
//! `createInstanceCapabilityEventMultiplexer`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** Cada caso cria o [`LeakGuard`] antes de tudo; o guarda é o primeiro
//!   local do caso, descartado por último.
//! - **`MockCwdDetectionCapability`.** É o [`MockCwdDetectionCapability`], com os mesmos membros (`cwds`, `isTrusted` verdadeiro,
//!   `onDidChangeCwd`, `getCwd`, `updateCwd`, `fireEvent` e `dispose`, que descarta o emissor).
//! - **`createMockTerminalInstance(instanceId, capabilities)`.** O `{ instanceId, capabilities } as unknown as ITerminalInstance`
//!   é o [`PartialTerminalInstance`] com só esses dois membros (qualquer outro consultado falha).
//! - **`createInstanceCapabilityEventMultiplexer(..., TerminalCapability.CwdDetection, cap => cap.onDidChangeCwd)`.** A
//!   capacidade é o marcador `CwdDetection`, e o `getEvent` é `cap.on_did_change_cwd()`.
//! - **`strictEqual(capturedData?.instance, instance)`.** Compara a identidade do `Rc` (`Rc::ptr_eq`).
//! - **`capabilities.add(TerminalCapability.CwdDetection, capability)`.** É `add::<CwdDetection>(capability)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_platform::terminal::capabilities::capabilities::{CwdDetection, ICwdDetectionCapability, ITerminalCapabilityStore, TerminalCapability};
use je_platform::terminal::capabilities::terminal_capability_store::TerminalCapabilityStore;
use je_workbench::contrib::terminal::browser::terminal::ITerminalInstance;
use je_workbench::contrib::terminal::browser::terminal_events::{InstanceCapabilityEvent, create_instance_capability_event_multiplexer};
use support::terminal_contrib_support::PartialTerminalInstance;

// Mock implementations for testing
struct MockCwdDetectionCapability {
    cwds: RefCell<Vec<String>>,
    on_did_change_cwd: Rc<Emitter<String>>,
}

impl MockCwdDetectionCapability {
    fn new() -> Rc<Self> {
        Rc::new(Self { cwds: RefCell::new(Vec::new()), on_did_change_cwd: Emitter::new() })
    }

    fn fire_event(&self, cwd: &str) {
        self.update_cwd(cwd, None);
    }
}

impl ICwdDetectionCapability for MockCwdDetectionCapability {
    fn capability_type(&self) -> TerminalCapability {
        TerminalCapability::CwdDetection
    }

    fn on_did_change_cwd(&self) -> Event<String> {
        self.on_did_change_cwd.event()
    }

    fn cwds(&self) -> Vec<String> {
        self.cwds.borrow().clone()
    }

    fn is_trusted(&self) -> bool {
        true
    }

    fn get_cwd(&self) -> String {
        self.cwds.borrow().last().cloned().unwrap_or_default()
    }

    fn update_cwd(&self, cwd: &str, _is_trusted: Option<bool>) {
        self.cwds.borrow_mut().push(cwd.to_owned());
        self.on_did_change_cwd.fire(cwd.to_owned());
    }
}

impl IDisposable for MockCwdDetectionCapability {
    fn dispose(&self) {
        self.on_did_change_cwd.dispose();
    }
}

fn create_mock_terminal_instance(instance_id: u32, capabilities: &Rc<TerminalCapabilityStore>) -> Rc<dyn ITerminalInstance> {
    let capabilities: Rc<dyn ITerminalCapabilityStore> = capabilities.clone();
    Rc::new(PartialTerminalInstance { instance_id: Some(instance_id), capabilities: Some(capabilities), ..Default::default() })
}

/// `capabilities.add(TerminalCapability.CwdDetection, capability)`.
fn add_cwd_detection(capabilities: &TerminalCapabilityStore, capability: &Rc<MockCwdDetectionCapability>) {
    let capability: Rc<dyn ICwdDetectionCapability> = capability.clone();
    capabilities.add::<CwdDetection>(capability);
}

type CapturedEvent = Rc<RefCell<Option<InstanceCapabilityEvent<String>>>>;

#[test]
fn should_handle_existing_instances_with_capabilities() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let capability = store.add_rc(MockCwdDetectionCapability::new());
    let capabilities = store.add_rc(TerminalCapabilityStore::new());
    add_cwd_detection(&capabilities, &capability);
    let instance = create_mock_terminal_instance(1, &capabilities);

    let on_add_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());
    let on_remove_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());

    let multiplexer = store.add_rc(Rc::new(create_instance_capability_event_multiplexer::<CwdDetection, String>(
        vec![instance.clone()],
        on_add_instance.event(),
        on_remove_instance.event(),
        |cap| cap.on_did_change_cwd(),
    )));

    let event_fired = Rc::new(Cell::new(false));
    let captured_data: CapturedEvent = Rc::default();

    let (fired, captured) = (event_fired.clone(), captured_data.clone());
    store.add(multiplexer.event.listen(move |e| {
        fired.set(true);
        *captured.borrow_mut() = Some(e.clone());
    }));

    capability.fire_event("test-data");

    assert!(event_fired.get(), "Event should be fired");
    let captured = captured_data.borrow();
    let captured = captured.as_ref().expect("o evento foi capturado");
    assert!(Rc::ptr_eq(&captured.instance, &instance), "Event should contain correct instance");
    assert_eq!(captured.data, "test-data", "Event should contain correct data");
}

#[test]
fn should_handle_instances_without_capabilities() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let capabilities = store.add_rc(TerminalCapabilityStore::new());
    let instance = create_mock_terminal_instance(1, &capabilities);
    let on_add_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());
    let on_remove_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());

    let multiplexer = store.add_rc(Rc::new(create_instance_capability_event_multiplexer::<CwdDetection, String>(
        vec![instance],
        on_add_instance.event(),
        on_remove_instance.event(),
        |cap| cap.on_did_change_cwd(),
    )));

    let event_fired = Rc::new(Cell::new(false));
    let fired = event_fired.clone();
    store.add(multiplexer.event.listen(move |_| fired.set(true)));

    assert!(!event_fired.get(), "No event should be fired for instances without capabilities");
}

#[test]
fn should_handle_adding_new_instances() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let on_add_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());
    let on_remove_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());

    let multiplexer = store.add_rc(Rc::new(create_instance_capability_event_multiplexer::<CwdDetection, String>(
        Vec::new(),
        on_add_instance.event(),
        on_remove_instance.event(),
        |cap| cap.on_did_change_cwd(),
    )));

    let event_fired = Rc::new(Cell::new(false));
    let captured_data: CapturedEvent = Rc::default();

    let (fired, captured) = (event_fired.clone(), captured_data.clone());
    store.add(multiplexer.event.listen(move |e| {
        fired.set(true);
        *captured.borrow_mut() = Some(e.clone());
    }));

    // Add a new instance with capability
    let capability = store.add_rc(MockCwdDetectionCapability::new());
    let capabilities = store.add_rc(TerminalCapabilityStore::new());
    let instance = create_mock_terminal_instance(2, &capabilities);

    on_add_instance.fire(instance.clone());

    // Add capability to the instance after it's added to the multiplexer
    add_cwd_detection(&capabilities, &capability);

    // Fire an event from the capability
    capability.fire_event("new-instance-data");

    assert!(event_fired.get(), "Event should be fired from new instance");
    let captured = captured_data.borrow();
    let captured = captured.as_ref().expect("o evento foi capturado");
    assert!(Rc::ptr_eq(&captured.instance, &instance), "Event should contain correct new instance");
    assert_eq!(captured.data, "new-instance-data", "Event should contain correct data");
}

#[test]
fn should_handle_removing_instances() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let capability = store.add_rc(MockCwdDetectionCapability::new());
    let capabilities = store.add_rc(TerminalCapabilityStore::new());
    add_cwd_detection(&capabilities, &capability);
    let instance = create_mock_terminal_instance(3, &capabilities);

    let on_add_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());
    let on_remove_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());

    let multiplexer = store.add_rc(Rc::new(create_instance_capability_event_multiplexer::<CwdDetection, String>(
        vec![instance.clone()],
        on_add_instance.event(),
        on_remove_instance.event(),
        |cap| cap.on_did_change_cwd(),
    )));

    let event_count = Rc::new(Cell::new(0));
    let count = event_count.clone();
    store.add(multiplexer.event.listen(move |_| count.set(count.get() + 1)));

    // Fire event before removal
    capability.fire_event("before-removal");
    assert_eq!(event_count.get(), 1, "Event should be fired before removal");

    // Remove the instance
    on_remove_instance.fire(instance);

    // Fire event after removal - should not be received
    capability.fire_event("after-removal");
    assert_eq!(event_count.get(), 1, "Event should not be fired after instance removal");
}

#[test]
fn should_handle_adding_capabilities_to_existing_instances() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let capabilities = store.add_rc(TerminalCapabilityStore::new());
    let instance = create_mock_terminal_instance(4, &capabilities);
    let on_add_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());
    let on_remove_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());

    let multiplexer = store.add_rc(Rc::new(create_instance_capability_event_multiplexer::<CwdDetection, String>(
        vec![instance.clone()],
        on_add_instance.event(),
        on_remove_instance.event(),
        |cap| cap.on_did_change_cwd(),
    )));

    let event_fired = Rc::new(Cell::new(false));
    let captured_data: CapturedEvent = Rc::default();

    let (fired, captured) = (event_fired.clone(), captured_data.clone());
    store.add(multiplexer.event.listen(move |e| {
        fired.set(true);
        *captured.borrow_mut() = Some(e.clone());
    }));

    // Add capability to existing instance
    let capability = store.add_rc(MockCwdDetectionCapability::new());
    add_cwd_detection(&capabilities, &capability);

    // Fire an event from the newly added capability
    capability.fire_event("added-capability-data");

    assert!(event_fired.get(), "Event should be fired from newly added capability");
    let captured = captured_data.borrow();
    let captured = captured.as_ref().expect("o evento foi capturado");
    assert!(Rc::ptr_eq(&captured.instance, &instance), "Event should contain correct instance");
    assert_eq!(captured.data, "added-capability-data", "Event should contain correct data");
}

#[test]
fn should_handle_removing_capabilities_from_existing_instances() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let capability = store.add_rc(MockCwdDetectionCapability::new());
    let capabilities = store.add_rc(TerminalCapabilityStore::new());
    add_cwd_detection(&capabilities, &capability);
    let instance = create_mock_terminal_instance(5, &capabilities);

    let on_add_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());
    let on_remove_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());

    let multiplexer = store.add_rc(Rc::new(create_instance_capability_event_multiplexer::<CwdDetection, String>(
        vec![instance],
        on_add_instance.event(),
        on_remove_instance.event(),
        |cap| cap.on_did_change_cwd(),
    )));

    let event_count = Rc::new(Cell::new(0));
    let count = event_count.clone();
    store.add(multiplexer.event.listen(move |_| count.set(count.get() + 1)));

    // Fire event before removing capability
    capability.fire_event("before-capability-removal");
    assert_eq!(event_count.get(), 1, "Event should be fired before capability removal");

    // Remove the capability
    capabilities.remove(TerminalCapability::CwdDetection); // Fire event after capability removal - should not be received
    capability.fire_event("after-capability-removal");
    assert_eq!(event_count.get(), 1, "Event should not be fired after capability removal");
}

#[test]
fn should_handle_multiple_instances_with_same_capability() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let capability1 = store.add_rc(MockCwdDetectionCapability::new());
    let capability2 = store.add_rc(MockCwdDetectionCapability::new());
    let capabilities1 = store.add_rc(TerminalCapabilityStore::new());
    let capabilities2 = store.add_rc(TerminalCapabilityStore::new());
    add_cwd_detection(&capabilities1, &capability1);
    add_cwd_detection(&capabilities2, &capability2);
    let instance1 = create_mock_terminal_instance(6, &capabilities1);
    let instance2 = create_mock_terminal_instance(7, &capabilities2);

    let on_add_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());
    let on_remove_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());

    let multiplexer = store.add_rc(Rc::new(create_instance_capability_event_multiplexer::<CwdDetection, String>(
        vec![instance1.clone(), instance2.clone()],
        on_add_instance.event(),
        on_remove_instance.event(),
        |cap| cap.on_did_change_cwd(),
    )));

    let events: Rc<RefCell<Vec<InstanceCapabilityEvent<String>>>> = Rc::default();
    let sink = events.clone();
    store.add(multiplexer.event.listen(move |e| sink.borrow_mut().push(e.clone())));

    // Fire events from both capabilities
    capability1.fire_event("data-from-instance1");
    capability2.fire_event("data-from-instance2");

    let events = events.borrow();
    assert_eq!(events.len(), 2, "Both events should be received");
    assert!(Rc::ptr_eq(&events[0].instance, &instance1), "First event should be from instance1");
    assert_eq!(events[0].data, "data-from-instance1", "First event should have correct data");
    assert!(Rc::ptr_eq(&events[1].instance, &instance2), "Second event should be from instance2");
    assert_eq!(events[1].data, "data-from-instance2", "Second event should have correct data");
}

#[test]
fn should_properly_dispose_all_resources() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let test_store = DisposableStore::new();
    let capability = test_store.add_rc(MockCwdDetectionCapability::new());
    let capabilities = test_store.add_rc(TerminalCapabilityStore::new());
    add_cwd_detection(&capabilities, &capability);
    let instance = create_mock_terminal_instance(8, &capabilities);

    let on_add_instance = test_store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());
    let on_remove_instance = test_store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());

    let multiplexer = test_store.add_rc(Rc::new(create_instance_capability_event_multiplexer::<CwdDetection, String>(
        vec![instance],
        on_add_instance.event(),
        on_remove_instance.event(),
        |cap| cap.on_did_change_cwd(),
    )));

    let event_count = Rc::new(Cell::new(0));
    let count = event_count.clone();
    test_store.add(multiplexer.event.listen(move |_| count.set(count.get() + 1)));

    // Fire event before disposal
    capability.fire_event("before-disposal");
    assert_eq!(event_count.get(), 1, "Event should be fired before disposal");

    // Dispose everything
    test_store.dispose();

    // Fire event after disposal - should not be received
    capability.fire_event("after-disposal");
    assert_eq!(event_count.get(), 1, "Event should not be fired after disposal");
}

#[test]
fn should_handle_empty_current_instances_array() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let on_add_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());
    let on_remove_instance = store.add_rc(Emitter::<Rc<dyn ITerminalInstance>>::new());

    let multiplexer = store.add_rc(Rc::new(create_instance_capability_event_multiplexer::<CwdDetection, String>(
        Vec::new(),
        on_add_instance.event(),
        on_remove_instance.event(),
        |cap| cap.on_did_change_cwd(),
    )));

    let event_fired = Rc::new(Cell::new(false));
    let fired = event_fired.clone();
    store.add(multiplexer.event.listen(move |_| fired.set(true)));

    // No instances, so no events should be fired initially
    assert!(!event_fired.get(), "No events should be fired with empty instances array");
}
