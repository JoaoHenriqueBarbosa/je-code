//! Porte de `src/vs/workbench/api/test/browser/extHostDebugAdapters.test.ts` (suíte `Extension host inline debug adapters`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `store` é o `DisposableStore` dele.
//!   Todo caso roda em `run_with_faked_timers`: o `.then` do construtor do serviço (que registra o `onDidChange` do registro de
//!   extensões) precisa rodar antes do descarte, como o mocha deixa as microtarefas esvaziarem antes do `teardown`, mesmo nos
//!   casos síncronos.
//! - O `createAdapter(implementation)` é o [`create_adapter`]; os dublês (`mock<...>()`) estão em `support/ext_host_debug_support.rs`.
//!   `class extends ExtHostDebugServiceBase { createInlineAdapter() {...} }` não precisa de subclasse: `create_debug_adapter` é
//!   público no porte.
//! - O `vscode.DebugAdapter` literal do original (`{ onDidSendMessage, handleMessage, dispose }`) é o [`TestDebugAdapter`].
//! - O `throw error` do `dispose` é `errors::throw`; `assert.throws(() => adapter.stopSession(), /^Error: .../)` é um
//!   `errors::catch` com a conferência do `String(error)`.
//! - `assert.ok(adapter)` é o `expect` da criação do adaptador.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_workbench::api::common::ext_host_debug_service::ExtHostDebugSession;
use je_workbench::api::common::ext_host_debug_types::{DebugAdapter, DebugAdapterDescriptor, DebugAdapterInlineImplementation};
use je_workbench::contrib::debug::abstract_debug_adapter::AbstractDebugAdapter;
use je_workbench::contrib::debug::debug_protocol::{Event as ProtocolEvent, ProtocolMessage};
use support::ext_host_debug_support::{MockMainThreadDebugService, create_debug_service, debug_config};

/// `const message: DebugProtocol.ProtocolMessage = { type: 'event', seq: 1 }`.
fn message() -> ProtocolMessage {
    ProtocolMessage::Event(ProtocolEvent { seq: 1, ..ProtocolEvent::default() })
}

/// O `vscode.DebugAdapter` literal do original.
struct TestDebugAdapter {
    on_did_send_message: Event<ProtocolMessage>,
    handle_message: Box<dyn Fn(&ProtocolMessage)>,
    dispose: Box<dyn Fn()>,
}

impl IDisposable for TestDebugAdapter {
    fn dispose(&self) {
        (self.dispose)();
    }
}

impl DebugAdapter for TestDebugAdapter {
    fn on_did_send_message(&self) -> Event<ProtocolMessage> {
        self.on_did_send_message.clone()
    }

    fn handle_message(&self, message: &ProtocolMessage) {
        (self.handle_message)(message);
    }
}

/// `createAdapter(implementation)`.
fn create_adapter(store: &Rc<DisposableStore>, implementation: TestDebugAdapter) -> Rc<dyn AbstractDebugAdapter> {
    let proxy = MockMainThreadDebugService::new(&["$registerDebugTypes"]);
    let service = create_debug_service(proxy.clone(), store);
    let session = ExtHostDebugSession::new(proxy, "test", "test", "test", None, debug_config("test", "launch", "test"), None);
    let adapter = service
        .create_debug_adapter(&DebugAdapterDescriptor::Inline(DebugAdapterInlineImplementation::new(Rc::new(implementation))), &session)
        .expect("o adaptador inline deveria ser criado");
    store.add(adapter.clone());
    adapter
}

#[test]
fn stopping_releases_a_subscription_to_a_longer_lived_publisher() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let emitter: Rc<Emitter<ProtocolMessage>> = Emitter::new();
        store.add(emitter.clone());
        let disposed = Rc::new(Cell::new(0));
        let adapter = create_adapter(
            &store,
            TestDebugAdapter {
                on_did_send_message: emitter.event(),
                handle_message: Box::new(|_| {}),
                dispose: Box::new({
                    let disposed = disposed.clone();
                    move || disposed.set(disposed.get() + 1)
                }),
            },
        );
        adapter.start_session().await.unwrap();
        adapter.stop_session().await.unwrap();
        assert_eq!((emitter.has_listeners(), disposed.get()), (false, 1));
    });
}

#[test]
fn disposing_releases_the_subscription() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let emitter: Rc<Emitter<ProtocolMessage>> = Emitter::new();
        store.add(emitter.clone());
        let adapter = create_adapter(&store, TestDebugAdapter { on_did_send_message: emitter.event(), handle_message: Box::new(|_| {}), dispose: Box::new(|| {}) });
        adapter.dispose();
        assert!(!emitter.has_listeners());
    });
}

#[test]
fn stopping_preserves_communication_for_surviving_adapters() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let emitter: Rc<Emitter<ProtocolMessage>> = Emitter::new();
        store.add(emitter.clone());
        let received: Rc<RefCell<Vec<&'static str>>> = Rc::default();
        let sent: Rc<RefCell<Vec<ProtocolMessage>>> = Rc::default();
        let create = |name: &'static str| {
            let adapter = create_adapter(
                &store,
                TestDebugAdapter {
                    on_did_send_message: emitter.event(),
                    handle_message: Box::new({
                        let sent = sent.clone();
                        move |message| sent.borrow_mut().push(message.clone())
                    }),
                    dispose: Box::new({
                        let emitter = emitter.clone();
                        move || emitter.fire(message())
                    }),
                },
            );
            let received = received.clone();
            adapter.on_message(Box::new(move |_| received.borrow_mut().push(name)));
            adapter
        };
        let first = create("first");
        let second = create("second");
        first.send_message(&message());
        emitter.fire(message());
        first.stop_session().await.unwrap();
        second.send_message(&message());
        emitter.fire(message());
        second.stop_session().await.unwrap();
        assert_eq!(*received.borrow(), vec!["first", "second", "second", "second"]);
        assert_eq!(*sent.borrow(), vec![message(), message()]);
        assert!(!emitter.has_listeners());
    });
}

#[test]
fn stopping_releases_the_subscription_when_implementation_disposal_throws() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let emitter: Rc<Emitter<ProtocolMessage>> = Emitter::new();
        store.add(emitter.clone());
        let error = Error::new("implementation disposal failed");
        let adapter = create_adapter(
            &store,
            TestDebugAdapter {
                on_did_send_message: emitter.event(),
                handle_message: Box::new(|_| {}),
                dispose: Box::new(move || errors::throw(error.clone())),
            },
        );
        let thrown = errors::catch(|| adapter.stop_session()).err().expect("stopSession deveria lançar");
        assert_eq!(format!("{}: {}", thrown.name, thrown.message), "Error: implementation disposal failed");
        assert!(!emitter.has_listeners());
    });
}
