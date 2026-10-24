//! Porte de `src/vs/workbench/services/commands/test/common/commandService.test.ts` (suíte `CommandService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` e o `setup` (`CommandsRegistry.registerCommand('foo', ...)`) viram o guarda e o
//!   registro do começo de cada caso. Os casos que esperam promessas rodam em `run_with_faked_timers`.
//! - `new class extends NullExtensionService { override ... }` é o [`TestExtensionService`], com um fecho para cada método que o
//!   original sobrescreve; sem fecho, vale o do `NullExtensionService` (`activateByEvent` resolve, `activationEventIsDone` é `false`,
//!   `whenInstalledExtensionsRegistered` resolve `true`).
//! - `new InstantiationService()` é o `InstantiationService` com a coleção vazia.
//! - `assert.ok(lastEvent, 'onCommand:foo')` do original só confere que `lastEvent` é verdadeiro (o segundo argumento é a mensagem); o
//!   porte confere o mesmo (`is_some`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::runtime;
use je_instantiation::{InstantiationService, ServiceCollection, ServicesAccessor};
use je_platform::commands::commands::{commands_registry, ICommandService};
use je_platform::log::NullLogService;
use je_workbench::services::commands::command_service::CommandService;
use je_extensions::scanner::ExtensionDescription;
use je_workbench::services::extensions::extensions::{ActivationKind, IExtensionDescription, IExtensionService};

type ActivateFn = Box<dyn Fn(&str) -> Promise<()>>;

/// `NullExtensionService`, com o que cada caso sobrescreve.
#[derive(Default)]
struct TestExtensionService {
    activate_by_event: Option<ActivateFn>,
    when_installed_extensions_registered: Option<Box<dyn Fn() -> Promise<bool>>>,
    activation_event_is_done: Option<bool>,
}

impl IExtensionService for TestExtensionService {
    fn activate_by_event(&self, activation_event: &str, _activation_kind: Option<ActivationKind>) -> Promise<()> {
        match &self.activate_by_event {
            Some(activate_by_event) => activate_by_event(activation_event),
            None => Promise::resolved(()),
        }
    }

    fn activation_event_is_done(&self, _activation_event: &str) -> bool {
        self.activation_event_is_done.unwrap_or(false)
    }

    fn get_extension(&self, _id: &str) -> Promise<Option<IExtensionDescription>> {
        Promise::resolved(None)
    }

    fn on_did_register_extensions(&self) -> Event<()> {
        Event::none()
    }

    fn when_installed_extensions_registered(&self) -> Promise<bool> {
        match &self.when_installed_extensions_registered {
            Some(when_installed_extensions_registered) => when_installed_extensions_registered(),
            None => Promise::resolved(true),
        }
    }

    fn extensions(&self) -> Vec<ExtensionDescription> {
        Vec::new()
    }
}

/// `setup`: `CommandsRegistry.registerCommand('foo', function () { })`.
fn register_foo(guard: &common::LeakGuard) {
    guard.add(commands_registry().register_command_handler("foo", Some(Rc::new(|_: &ServicesAccessor, _: &[Dynamic]| Dynamic::Undefined))));
}

fn new_service(store: &Rc<DisposableStore>, extension_service: Rc<TestExtensionService>) -> Rc<CommandService> {
    store.add_rc(CommandService::new(InstantiationService::new(ServiceCollection::new()), extension_service, NullLogService::shared()))
}

#[test]
fn activate_on_command() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    register_foo(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        let last_event: Rc<RefCell<Option<String>>> = Rc::default();
        let recorder = last_event.clone();
        let service = new_service(
            &store,
            Rc::new(TestExtensionService {
                activate_by_event: Some(Box::new(move |activation_event| {
                    *recorder.borrow_mut() = Some(activation_event.to_owned());
                    Promise::resolved(())
                })),
                ..TestExtensionService::default()
            }),
        );

        service.execute_command("foo", vec![]).await.expect("foo");
        assert!(last_event.borrow().is_some(), "onCommand:foo");
        let result = service.execute_command("unknownCommandId", vec![]).await;
        assert!(result.is_err());
        assert!(last_event.borrow().is_some(), "onCommand:unknownCommandId");
    });
}

#[test]
fn fwd_activation_error() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    register_foo(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        let extension_service = Rc::new(TestExtensionService {
            activate_by_event: Some(Box::new(|_| Promise::rejected(Error::new("bad_activate")))),
            ..TestExtensionService::default()
        });

        let service = new_service(&store, extension_service.clone());

        extension_service.when_installed_extensions_registered().await.expect("registradas");

        let err = service.execute_command("foo", vec![]).await.expect_err("não deveria resolver");
        assert_eq!(err.message, "bad_activate");
    });
}

#[test]
fn not_on_ready_but_execute_command() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    register_foo(&guard);
    let store = guard.store();
    let call_counter = Rc::new(Cell::new(0));
    let counter = call_counter.clone();
    let reg = commands_registry().register_command_handler(
        "bar",
        Some(Rc::new(move |_: &ServicesAccessor, _: &[Dynamic]| {
            counter.set(counter.get() + 1);
            Dynamic::Number(f64::from(counter.get()))
        })),
    );

    let service = new_service(
        &store,
        Rc::new(TestExtensionService {
            when_installed_extensions_registered: Some(Box::new(|| Promise::<bool>::new().0)), /* ignore */
            ..TestExtensionService::default()
        }),
    );

    let _ = service.execute_command("bar", vec![]);
    assert_eq!(call_counter.get(), 1);
    reg.dispose();
}

#[test]
fn issue_34913_not_on_ready_unknown_command() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    register_foo(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        let call_counter = Rc::new(Cell::new(0));
        let (when_installed_extensions_registered, resolve_func) = Promise::<bool>::new();

        let service = new_service(
            &store,
            Rc::new(TestExtensionService {
                when_installed_extensions_registered: Some(Box::new(move || when_installed_extensions_registered.clone())),
                ..TestExtensionService::default()
            }),
        );

        let r = service.execute_command("bar", vec![]);
        assert_eq!(call_counter.get(), 0);

        let counter = call_counter.clone();
        let reg = commands_registry().register_command_handler(
            "bar",
            Some(Rc::new(move |_: &ServicesAccessor, _: &[Dynamic]| {
                counter.set(counter.get() + 1);
                Dynamic::Number(f64::from(counter.get()))
            })),
        );
        resolve_func.resolve(true);

        r.await.expect("bar");
        reg.dispose();
        assert_eq!(call_counter.get(), 1);
    });
}

#[test]
fn stop_waiting_for_extensions_to_activate_when_trigger_is_satisfied_62457() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    register_foo(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        let call_counter = Rc::new(Cell::new(0));
        let disposable = DisposableStore::new();
        let events: Rc<RefCell<Vec<String>>> = Rc::default();
        let (recorded_events, registrations, counter) = (events.clone(), disposable.clone(), call_counter.clone());
        let service = new_service(
            &store,
            Rc::new(TestExtensionService {
                activate_by_event: Some(Box::new(move |event| {
                    recorded_events.borrow_mut().push(event.to_owned());
                    if event == "*" {
                        return Promise::<()>::new().0; // forever promise...
                    }
                    if let Some(command_id) = event.strip_prefix("onCommand:") {
                        let (promise, resolver) = Promise::<()>::new();
                        let (command_id, registrations, counter) = (command_id.to_owned(), registrations.clone(), counter.clone());
                        runtime::set_timeout(
                            move || {
                                let reg = commands_registry().register_command_handler(
                                    &command_id,
                                    Some(Rc::new(move |_: &ServicesAccessor, _: &[Dynamic]| {
                                        counter.set(counter.get() + 1);
                                        Dynamic::Undefined
                                    })),
                                );
                                registrations.add(reg);
                                resolver.resolve(());
                            },
                            0,
                        );
                        return promise;
                    }
                    Promise::resolved(())
                })),
                ..TestExtensionService::default()
            }),
        );

        let result = service.execute_command("farboo", vec![]).await;
        disposable.dispose();
        result.expect("farboo");
        assert_eq!(call_counter.get(), 1);
        let mut recorded = events.borrow().clone();
        recorded.sort();
        let mut expected = vec!["*".to_owned(), "onCommand:farboo".to_owned()];
        expected.sort();
        assert_eq!(recorded, expected);
    });
}

#[test]
fn issue_71471_wait_for_on_command_activation_even_if_a_command_is_registered() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    register_foo(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        let expected_order = ["registering command", "resolving activation event", "executing command"];
        let actual_order: Rc<RefCell<Vec<&'static str>>> = Rc::default();
        let disposables = DisposableStore::new();
        let (order, registrations) = (actual_order.clone(), disposables.clone());
        let service = new_service(
            &store,
            Rc::new(TestExtensionService {
                activate_by_event: Some(Box::new(move |event| {
                    if event == "*" {
                        return Promise::<()>::new().0; // forever promise...
                    }
                    if let Some(command_id) = event.strip_prefix("onCommand:") {
                        let (promise, resolver) = Promise::<()>::new();
                        let (command_id, order, registrations) = (command_id.to_owned(), order.clone(), registrations.clone());
                        runtime::set_timeout(
                            move || {
                                // Register the command after some time
                                order.borrow_mut().push("registering command");
                                let executing = order.clone();
                                let reg = commands_registry().register_command_handler(
                                    &command_id,
                                    Some(Rc::new(move |_: &ServicesAccessor, _: &[Dynamic]| {
                                        executing.borrow_mut().push("executing command");
                                        Dynamic::Undefined
                                    })),
                                );
                                registrations.add(reg);

                                runtime::set_timeout(
                                    move || {
                                        // Resolve the activation event after some more time
                                        order.borrow_mut().push("resolving activation event");
                                        resolver.resolve(());
                                    },
                                    10,
                                );
                            },
                            10,
                        );
                        return promise;
                    }
                    Promise::resolved(())
                })),
                ..TestExtensionService::default()
            }),
        );

        let result = service.execute_command("farboo2", vec![]).await;
        disposables.dispose();
        result.expect("farboo2");
        assert_eq!(*actual_order.borrow(), expected_order);
    });
}

#[test]
fn issue_142155_execute_commands_synchronously_if_possible() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    register_foo(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        let actual_order: Rc<RefCell<Vec<&'static str>>> = Rc::default();

        let disposables = DisposableStore::new();
        let executing = actual_order.clone();
        disposables.add(commands_registry().register_command_handler(
            "bizBaz",
            Some(Rc::new(move |_: &ServicesAccessor, _: &[Dynamic]| {
                executing.borrow_mut().push("executing command");
                Dynamic::Undefined
            })),
        ));
        let extension_service = Rc::new(TestExtensionService { activation_event_is_done: Some(true), ..TestExtensionService::default() });
        let service = new_service(&store, extension_service.clone());

        extension_service.when_installed_extensions_registered().await.expect("registradas");

        actual_order.borrow_mut().push("before call");
        let promise = service.execute_command("bizBaz", vec![]);
        actual_order.borrow_mut().push("after call");
        let result = promise.await;
        actual_order.borrow_mut().push("resolved");
        let order = actual_order.borrow().clone();
        disposables.dispose();
        result.expect("bizBaz");
        assert_eq!(order, ["before call", "executing command", "after call", "resolved"]);
    });
}
