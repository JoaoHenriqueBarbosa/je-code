//! Porte de `src/vs/workbench/contrib/debug/test/common/abstractDebugAdapter.test.ts` (suíte `Debug - AbstractDebugAdapter`,
//! sub-suíte `event ordering`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado por [`run_test`], que também faz o `setup` da
//!   sub-suíte (o `MockDebugAdapter`, o `output` e o ouvinte de `onEvent`) e roda o corpo com o relógio virtual: o
//!   `await timeout(0)` e o `setTimeout(..., 0)` do dublê são timers do runtime, e `Promise.resolve().then(...)` é uma
//!   microtarefa do runtime, na mesma ordem do motor JS.
//! - `MockDebugAdapter` (`test/common/mockDebug.ts`) é a struct do mesmo nome, portada inteira (`sendResponseBody`,
//!   `sendEventBody`, `waitForResponseFromClient`, `sendRequestBody`, `evaluate`); o `this` das chamadas assíncronas é o
//!   `Weak` que `Rc::new_cyclic` entrega. Os métodos que este arquivo não chama ficam para os outros testes que usam o
//!   dublê (por isso o `allow(dead_code)`).
//! - O `evaluate` do teste é a função [`evaluate`]; `await new Promise(resolve => adapter.sendRequest(...))` é o par
//!   `Promise::new()` e o callback que resolve com a resposta.
//! - `assert.deepStrictEqual(output, [...])` é `assert_eq!` entre listas de texto.

#![allow(dead_code)]

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::rc::{Rc, Weak};

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::lifecycle::IDisposable;
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime;
use serde_json::{Value, json};

use je_workbench::contrib::debug::abstract_debug_adapter::{AbstractDebugAdapter, DebugAdapterState};
use je_workbench::contrib::debug::debug_protocol::{Event as ProtocolEvent, ProtocolMessage, Request, Response};

/// `MockDebugAdapter` (`mockDebug.ts`).
struct MockDebugAdapter {
    state: Rc<DebugAdapterState>,
    me: Weak<MockDebugAdapter>,
    seq: Cell<i64>,
    pending_responses: RefCell<HashMap<String, Rc<DeferredPromise<Response>>>>,
}

impl MockDebugAdapter {
    fn new() -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            state: DebugAdapterState::new(),
            me: me.clone(),
            seq: Cell::new(0),
            pending_responses: RefCell::new(HashMap::new()),
        })
    }

    fn next_seq(&self) -> i64 {
        self.seq.set(self.seq.get() + 1);
        self.seq.get()
    }

    fn this(&self) -> Rc<MockDebugAdapter> {
        self.me.upgrade().expect("o adaptador está vivo")
    }

    fn send_response_body(&self, request: &Request, body: Value) {
        let response = Response {
            seq: self.next_seq(),
            request_seq: request.seq,
            command: request.command.clone(),
            success: true,
            message: None,
            body: Some(body),
        };
        self.this().accept_message(ProtocolMessage::Response(response));
    }

    fn send_event_body(&self, event: &str, body: Value) {
        let response = ProtocolEvent { seq: self.next_seq(), event: event.to_owned(), body: Some(body) };
        self.this().accept_message(ProtocolMessage::Event(response));
    }

    fn wait_for_response_from_client(&self, command: &str) -> Promise<Response> {
        let deferred = Rc::new(DeferredPromise::<Response>::default());
        if let Some(existing) = self.pending_responses.borrow().get(command) {
            return existing.promise.clone();
        }

        self.pending_responses.borrow_mut().insert(command.to_owned(), deferred.clone());
        deferred.promise.clone()
    }

    fn send_request_body(&self, command: &str, args: Value) {
        let request = Request { seq: self.next_seq(), command: command.to_owned(), arguments: Some(args) };
        self.this().accept_message(ProtocolMessage::Request(request));
    }

    fn evaluate(&self, request: &Request, args: &Value) {
        let expression = args["expression"].as_str().expect("expression").to_owned();
        if expression.starts_with("before.") {
            self.send_event_body("output", json!({ "output": expression }));
        }

        self.send_response_body(request, json!({ "result": format!("={expression}"), "variablesReference": 0 }));

        if expression.starts_with("after.") {
            self.send_event_body("output", json!({ "output": expression }));
        }
    }
}

impl IDisposable for MockDebugAdapter {
    fn dispose(&self) {
        self.state.dispose();
    }
}

impl AbstractDebugAdapter for MockDebugAdapter {
    fn state(&self) -> &Rc<DebugAdapterState> {
        &self.state
    }

    fn start_session(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn stop_session(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn send_message(&self, message: &ProtocolMessage) {
        match message {
            ProtocolMessage::Request(request) => {
                let request = request.clone();
                let this = self.this();
                runtime::set_timeout(
                    move || {
                        if request.command == "evaluate" {
                            this.evaluate(&request, request.arguments.as_ref().expect("arguments"));
                            return;
                        }
                        this.send_response_body(&request, json!({}));
                    },
                    0,
                );
            }
            ProtocolMessage::Response(response) => {
                if let Some(pending) = self.pending_responses.borrow().get(&response.command) {
                    drop(pending.complete(response.clone()));
                }
            }
            _ => {}
        }
    }
}

type Output = Rc<RefCell<Vec<String>>>;

/// `ensureNoDisposablesAreLeakedInTestSuite()` e o `setup` da sub-suíte `event ordering`.
fn run_test<F: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<MockDebugAdapter>, Output) -> F + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        // setup
        let adapter = MockDebugAdapter::new();
        let output: Output = Rc::new(RefCell::new(Vec::new()));
        let listener_output = output.clone();
        adapter.on_event(Box::new(move |ev: &ProtocolEvent| {
            let text = ev.body.as_ref().and_then(|body| body["output"].as_str()).expect("output").to_owned();
            listener_output.borrow_mut().push(text);
            push_end_microtask(&listener_output);
        }));

        body(adapter, output).await;
    });
    drop(guard);
}

/// `Promise.resolve().then(() => output.push('--end microtask--'))`.
fn push_end_microtask(output: &Output) {
    let output = output.clone();
    drop(Promise::resolved(()).then(move |()| output.borrow_mut().push("--end microtask--".to_owned())));
}

/// O `evaluate` do teste.
async fn evaluate(adapter: &Rc<MockDebugAdapter>, output: &Output, expression: &str) {
    let (response, resolver) = Promise::<Response>::new();
    adapter.send_request("evaluate", Some(&json!({ "expression": expression })), Rc::new(move |result: &Response| resolver.resolve(result.clone())), None);
    response.await.unwrap();
    output.borrow_mut().push(format!("={expression}"));
    push_end_microtask(output);
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn inserts_task_boundary_before_response() {
    run_test(|adapter, output| async move {
        evaluate(&adapter, &output, "before.foo").await;
        timeout(0).await.unwrap();

        assert_eq!(*output.borrow(), strings(&["before.foo", "--end microtask--", "=before.foo", "--end microtask--"]));
    });
}

#[test]
fn inserts_task_boundary_after_response() {
    run_test(|adapter, output| async move {
        evaluate(&adapter, &output, "after.foo").await;
        timeout(0).await.unwrap();

        assert_eq!(*output.borrow(), strings(&["=after.foo", "--end microtask--", "after.foo", "--end microtask--"]));
    });
}

#[test]
fn does_not_insert_boundaries_between_events() {
    run_test(|adapter, output| async move {
        adapter.send_event_body("output", json!({ "output": "a" }));
        adapter.send_event_body("output", json!({ "output": "b" }));
        adapter.send_event_body("output", json!({ "output": "c" }));
        timeout(0).await.unwrap();

        assert_eq!(*output.borrow(), strings(&["a", "b", "c", "--end microtask--", "--end microtask--", "--end microtask--"]));
    });
}
