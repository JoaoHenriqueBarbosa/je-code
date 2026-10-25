//! Porte de `src/vs/workbench/contrib/canvases/test/common/canvasDefinitionStore.test.ts` (suíte `CanvasDefinitionStore`), caso
//! a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo do caso; o `store.add(...)` do original é o
//!   store do guarda. Os casos são `async` no mocha e rodam aqui sob `run_with_faked_timers`, com `timeout(0)` esperando as
//!   microtarefas e os timers pendentes.
//! - `definition(canvasId, overrides)` é `definition(canvas_id)` mais a sintaxe de atualização de struct para os `overrides`.
//! - `new class extends NullLogService { override error(message) }` é [`RecordingLogService`], que delega tudo ao
//!   `NullLogService` e grava o texto de cada `error`.
//! - `listCanvases: () => ...` é o [`TestSource`], que guarda um `DeferredPromise` por listagem em `listings`.
//! - `assert.deepStrictEqual` de objetos vira `assert_eq!` dos campos, na mesma ordem do objeto esperado.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::observable::observables::observable_signal::ObservableSignal;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::{IReaderWithStore, Observable, SettableExt, autorun, observable_signal, observable_value, transaction};
use je_base::promise::{DeferredPromise, Promise};
use je_base::uri::URI;
use je_platform::log::{LogArg, LogLevel, LogMessage, LogServiceRef, Logger, NullLogService};
use je_workbench::contrib::canvases::canvas::{CanvasExtensionSource, ICanvasDefinition, ICanvasOwner};
use je_workbench::contrib::canvases::canvas_definition_store::{CanvasDefinitionStore, ICanvasDefinitionSource};

fn definition(canvas_id: &str) -> ICanvasDefinition {
    ICanvasDefinition {
        canvas_id: canvas_id.to_string(),
        extension_id: format!("user:{canvas_id}"),
        extension_source: CanvasExtensionSource::User,
        extension_name: None,
        display_name: canvas_id.to_string(),
        description: canvas_id.to_string(),
        requires_input: false,
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

/// O `NullLogService` que grava o texto de cada `error`.
struct RecordingLogService {
    inner: NullLogService,
    errors: Rc<RefCell<Vec<String>>>,
}

impl IDisposable for RecordingLogService {
    fn dispose(&self) {}
}

impl Logger for RecordingLogService {
    fn on_did_change_log_level(&self) -> Event<LogLevel> {
        self.inner.on_did_change_log_level()
    }

    fn get_level(&self) -> LogLevel {
        self.inner.get_level()
    }

    fn set_level(&self, level: LogLevel) {
        self.inner.set_level(level);
    }

    fn trace_args(&self, message: &str, args: &[LogArg]) {
        self.inner.trace_args(message, args);
    }

    fn debug_args(&self, message: &str, args: &[LogArg]) {
        self.inner.debug_args(message, args);
    }

    fn info_args(&self, message: &str, args: &[LogArg]) {
        self.inner.info_args(message, args);
    }

    fn warn_args(&self, message: &LogMessage, args: &[LogArg]) {
        self.inner.warn_args(message, args);
    }

    fn error_args(&self, message: &LogMessage, _args: &[LogArg]) {
        self.errors.borrow_mut().push(match message {
            LogMessage::Text(text) => text.clone(),
            LogMessage::Error(error) => error.to_string(),
        });
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

struct Listing {
    chat: String,
    connection: String,
    result: Rc<DeferredPromise<Vec<ICanvasDefinition>>>,
}

type Listings = Rc<RefCell<Vec<Listing>>>;

struct TestSource {
    owner: ICanvasOwner,
    chat: String,
    connection: String,
    listings: Listings,
}

impl ICanvasDefinitionSource for TestSource {
    fn owner(&self) -> ICanvasOwner {
        self.owner.clone()
    }

    fn list_canvases(&self) -> Promise<Vec<ICanvasDefinition>> {
        let result = Rc::new(DeferredPromise::<Vec<ICanvasDefinition>>::new());
        self.listings.borrow_mut().push(Listing { chat: self.chat.clone(), connection: self.connection.clone(), result: result.clone() });
        result.promise.clone()
    }
}

struct Harness {
    chat: Rc<ObservableValue<Option<String>, ()>>,
    connection: Rc<ObservableValue<String, ()>>,
    invalidated: Rc<ObservableSignal<()>>,
    listings: Listings,
    errors: Rc<RefCell<Vec<String>>>,
    definition_store: Rc<CanvasDefinitionStore>,
}

impl Harness {
    fn result(&self, index: usize) -> Rc<DeferredPromise<Vec<ICanvasDefinition>>> {
        self.listings.borrow()[index].result.clone()
    }

    fn canvas_ids(&self) -> Vec<String> {
        self.definition_store.definitions.get().iter().map(|canvas| canvas.canvas_id.clone()).collect()
    }

    fn pending_chats(&self) -> Vec<String> {
        self.listings.borrow().iter().filter(|listing| !listing.result.is_settled()).map(|listing| listing.chat.clone()).collect()
    }

    fn listing_count(&self) -> usize {
        self.listings.borrow().len()
    }
}

fn create_harness(store: &Rc<je_base::lifecycle::DisposableStore>) -> Harness {
    let chat = observable_value::<Option<String>, ()>("chat", Some("main".to_string()));
    let connection = observable_value::<String, ()>("connection", "first".to_string());
    let invalidated = observable_signal::<()>("invalidated");
    let listings: Listings = Rc::default();
    let errors: Rc<RefCell<Vec<String>>> = Rc::default();
    let log_service: LogServiceRef = Rc::new(RecordingLogService { inner: NullLogService, errors: errors.clone() });
    let source = {
        let (chat, connection, invalidated, listings) = (chat.clone(), connection.clone(), invalidated.clone(), listings.clone());
        move |reader: &dyn IReaderWithStore| -> Option<Rc<dyn ICanvasDefinitionSource>> {
            let name = chat.read(reader);
            let connection_name = connection.read(reader);
            invalidated.read(reader);
            name.map(|name| {
                Rc::new(TestSource {
                    owner: ICanvasOwner {
                        provider_id: "local".to_string(),
                        session: URI::parse("agent-host-session:/session", false),
                        chat: URI::parse(format!("agent-host-chat:/session/{name}"), false),
                    },
                    chat: name,
                    connection: connection_name,
                    listings: listings.clone(),
                }) as Rc<dyn ICanvasDefinitionSource>
            })
        }
    };
    let definition_store = store.add_rc(CanvasDefinitionStore::new(source, "[Test]", log_service));
    Harness { chat, connection, invalidated, listings, errors, definition_store }
}

#[test]
fn lists_openable_definitions_in_menu_order_and_keeps_them_through_pending_and_failed_refreshes() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let harness = create_harness(&store);
        timeout(0).await.unwrap();
        harness.result(0).complete(vec![
            definition("zeta"),
            ICanvasDefinition { requires_input: true, ..definition("form") },
            ICanvasDefinition { extension_id: "github-app".to_string(), extension_source: CanvasExtensionSource::Unknown, ..definition("browser") },
            ICanvasDefinition { extension_id: "project:alpha".to_string(), extension_source: CanvasExtensionSource::Project, ..definition("alpha") },
        ]);
        timeout(0).await.unwrap();
        let listed = harness.canvas_ids();
        harness.invalidated.trigger(None, ());
        timeout(0).await.unwrap();
        let while_refreshing = harness.canvas_ids();
        harness.result(1).error(je_base::errors::Error::new("Host unavailable"));
        timeout(0).await.unwrap();

        assert_eq!(listed, strings(&["alpha", "zeta"]));
        assert_eq!(while_refreshing, strings(&["alpha", "zeta"]));
        assert_eq!(harness.canvas_ids(), strings(&["alpha", "zeta"]));
        assert_eq!(*harness.errors.borrow(), strings(&["[Test] Failed to list registered canvases"]));
    });
}

#[test]
fn coalesces_invalidations_per_owner_and_applies_each_result_in_order() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let harness = create_harness(&store);
        timeout(0).await.unwrap();
        harness.invalidated.trigger(None, ());
        harness.invalidated.trigger(None, ());
        timeout(0).await.unwrap();
        harness.invalidated.trigger(None, ());
        timeout(0).await.unwrap();
        let pending_while_invalidating = harness.pending_chats();
        harness.result(0).complete(vec![definition("plan")]);
        timeout(0).await.unwrap();
        let after_first_result = (harness.canvas_ids(), harness.pending_chats());
        harness.result(1).complete(vec![definition("plan"), definition("board")]);
        timeout(0).await.unwrap();
        let after_follow_up = harness.canvas_ids();
        harness.invalidated.trigger(None, ());
        harness.invalidated.trigger(None, ());
        harness.invalidated.trigger(None, ());
        timeout(0).await.unwrap();
        harness.result(2).complete(vec![definition("plan"), definition("board")]);
        timeout(0).await.unwrap();

        assert_eq!(pending_while_invalidating, strings(&["main"]));
        assert_eq!(after_first_result, (strings(&["plan"]), strings(&["main"])));
        assert_eq!(after_follow_up, strings(&["board", "plan"]));
        assert_eq!(harness.listing_count(), 3);
    });
}

#[test]
fn keeps_the_list_when_the_same_owner_gets_a_new_source_and_refreshes_with_the_newest_source() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let harness = create_harness(&store);
        timeout(0).await.unwrap();
        harness.result(0).complete(vec![definition("plan")]);
        timeout(0).await.unwrap();
        harness.connection.set("second".to_string(), None);
        timeout(0).await.unwrap();
        harness.connection.set("third".to_string(), None);
        timeout(0).await.unwrap();
        harness.connection.set("fourth".to_string(), None);
        timeout(0).await.unwrap();
        let while_replacing = harness.canvas_ids();
        harness.result(1).complete(vec![definition("plan"), definition("board")]);
        timeout(0).await.unwrap();
        let after_replaced_listing = harness.canvas_ids();
        harness.result(2).complete(vec![definition("board")]);
        timeout(0).await.unwrap();

        assert_eq!(while_replacing, strings(&["plan"]));
        assert_eq!(after_replaced_listing, strings(&["board", "plan"]));
        assert_eq!(harness.canvas_ids(), strings(&["board"]));
        let listings: Vec<String> = harness.listings.borrow().iter().map(|listing| format!("{}/{}", listing.chat, listing.connection)).collect();
        assert_eq!(listings, strings(&["main/first", "main/second", "main/fourth"]));
    });
}

#[test]
fn lists_a_new_owner_immediately_and_never_lists_or_applies_the_previous_owner() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let harness = create_harness(&store);
        timeout(0).await.unwrap();
        harness.invalidated.trigger(None, ());
        timeout(0).await.unwrap();
        harness.chat.set(Some("peer".to_string()), None);
        timeout(0).await.unwrap();
        let pending_after_switch = harness.pending_chats();
        harness.result(0).complete(vec![definition("plan")]);
        timeout(0).await.unwrap();

        assert_eq!(pending_after_switch, strings(&["main", "peer"]));
        let listed_chats: Vec<String> = harness.listings.borrow().iter().map(|listing| listing.chat.clone()).collect();
        assert_eq!(listed_chats, strings(&["main", "peer"]));
        assert_eq!(harness.canvas_ids(), Vec::<String>::new());
    });
}

#[test]
fn clears_on_owner_changes_without_pairing_an_owner_with_another_owners_definitions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let harness = create_harness(&store);
        let observed: Rc<RefCell<Vec<String>>> = Rc::default();
        store.add(autorun({
            let (chat, definitions, observed) = (harness.chat.clone(), harness.definition_store.definitions.clone(), observed.clone());
            move |reader| {
                let ids: Vec<String> = definitions.read(reader).iter().map(|canvas| canvas.canvas_id.clone()).collect();
                observed.borrow_mut().push(format!("{}: {}", chat.read(reader).unwrap_or_else(|| "undefined".to_string()), ids.join(", ")));
            }
        }));
        timeout(0).await.unwrap();
        harness.result(0).complete(vec![definition("plan")]);
        timeout(0).await.unwrap();
        let mut read_in_switch_transaction: Option<Vec<String>> = None;
        transaction(|tx| {
            harness.chat.set(Some("peer".to_string()), Some(tx));
            read_in_switch_transaction = Some(harness.canvas_ids());
        });
        timeout(0).await.unwrap();
        harness.result(1).complete(vec![definition("board")]);
        timeout(0).await.unwrap();
        harness.chat.set(Some("main".to_string()), None);
        timeout(0).await.unwrap();

        assert_eq!(*observed.borrow(), strings(&["main: ", "main: plan", "peer: ", "peer: board", "main: "]));
        assert_eq!(read_in_switch_transaction, Some(Vec::new()));
    });
}

#[test]
fn does_not_notify_observers_when_a_refresh_returns_structurally_identical_definitions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let harness = create_harness(&store);
        let notifications = Rc::new(std::cell::Cell::new(0));
        store.add(autorun({
            let (definitions, notifications) = (harness.definition_store.definitions.clone(), notifications.clone());
            move |reader| {
                definitions.read(reader);
                notifications.set(notifications.get() + 1);
            }
        }));
        timeout(0).await.unwrap();
        harness.result(0).complete(vec![definition("plan")]);
        timeout(0).await.unwrap();
        harness.invalidated.trigger(None, ());
        timeout(0).await.unwrap();
        harness.result(1).complete(vec![definition("plan")]);
        timeout(0).await.unwrap();

        assert_eq!(notifications.get(), 2);
        assert_eq!(harness.canvas_ids(), strings(&["plan"]));
    });
}

#[test]
fn clears_while_unavailable_and_ignores_results_that_arrive_late_or_after_disposal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let harness = create_harness(&store);
        timeout(0).await.unwrap();
        harness.result(0).complete(vec![definition("plan")]);
        timeout(0).await.unwrap();
        harness.invalidated.trigger(None, ());
        timeout(0).await.unwrap();
        harness.chat.set(None, None);
        let while_unavailable = harness.canvas_ids();
        harness.result(1).complete(vec![definition("late")]);
        timeout(0).await.unwrap();
        let after_late_result = harness.canvas_ids();
        harness.chat.set(Some("main".to_string()), None);
        timeout(0).await.unwrap();
        harness.invalidated.trigger(None, ());
        harness.definition_store.dispose();
        harness.result(2).complete(vec![definition("afterDispose")]);
        timeout(0).await.unwrap();

        assert_eq!(while_unavailable, Vec::<String>::new());
        assert_eq!(after_late_result, Vec::<String>::new());
        assert_eq!(harness.canvas_ids(), Vec::<String>::new());
        assert_eq!(harness.listing_count(), 3);
        assert_eq!(*harness.errors.borrow(), Vec::<String>::new());
    });
}
