//! Porte de `src/vs/workbench/contrib/debug/test/browser/rawDebugSession.test.ts` (suíte `RawDebugSession`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` de cada caso; `disposables.add(session)` e
//!   `disposables.add(debugAdapter)` são o `add_rc` do guarda.
//! - `mockObject<IDebugger>()({ type: 'mock-debug' })` com `startDebugging.returns(Promise.resolve(x))` é o dublê
//!   [`MockDebugger`] (`type` `mock-debug`, `startDebugging` devolvendo o valor configurado); os outros membros lançam, como
//!   um stub sem comportamento.
//! - Os três serviços `new (mock<...>())` e o `IDialogService` do construtor são `None` (nenhum deles é usado pelo
//!   `startDebugging`).
//! - O caso roda no relógio virtual (a fila do adaptador usa `setTimeout(0)`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::promise::Promise;
use je_workbench::contrib::debug::debug::{IConfig, IDebugAdapter};
use je_workbench::contrib::debug::debug_types::{IDebugSessionFull, IDebugger, ITelemetryEndpoint};
use je_workbench::contrib::debug::raw_debug_session::RawDebugSession;
use serde_json::{Value, json};
use support::mock_debug::MockDebugAdapter;

/// O `IDebugger` do `mockObject`.
struct MockDebugger {
    start_debugging_result: Cell<bool>,
}

impl IDebugger for MockDebugger {
    fn debug_type(&self) -> String {
        "mock-debug".to_owned()
    }

    fn create_debug_adapter(&self, _session: Rc<dyn IDebugSessionFull>) -> Promise<Rc<dyn IDebugAdapter>> {
        panic!("createDebugAdapter não foi configurado")
    }

    fn run_in_terminal(&self, _args: Value, _session_id: &str) -> Promise<Option<i64>> {
        panic!("runInTerminal não foi configurado")
    }

    fn start_debugging(&self, _args: IConfig, _parent_session_id: &str) -> Promise<bool> {
        Promise::resolved(self.start_debugging_result.get())
    }

    fn get_custom_telemetry_endpoint(&self) -> Option<ITelemetryEndpoint> {
        panic!("getCustomTelemetryEndpoint não foi configurado")
    }
}

fn start_debugging_request() -> Value {
    json!({ "request": "launch", "configuration": { "type": "some-other-type" } })
}

#[test]
fn handles_start_debugging_request_success() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let debug_adapter = guard.add_rc(MockDebugAdapter::new());
        let dbgr = Rc::new(MockDebugger { start_debugging_result: Cell::new(true) });
        let session = guard.add_rc(RawDebugSession::new(debug_adapter.clone(), Some(dbgr), "sessionId", "name", None));
        let _ = &session;

        debug_adapter.send_request_body("startDebugging", start_debugging_request());
        let response = debug_adapter.wait_for_response_from_client("startDebugging").await.unwrap();
        assert_eq!(response.command, "startDebugging");
        assert!(response.success);
    });
}

#[test]
fn handles_start_debugging_request_failure() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let debug_adapter = guard.add_rc(MockDebugAdapter::new());
        let dbgr = Rc::new(MockDebugger { start_debugging_result: Cell::new(false) });
        let session = guard.add_rc(RawDebugSession::new(debug_adapter.clone(), Some(dbgr), "sessionId", "name", None));
        let _ = &session;

        debug_adapter.send_request_body("startDebugging", start_debugging_request());
        let response = debug_adapter.wait_for_response_from_client("startDebugging").await.unwrap();
        assert_eq!(response.command, "startDebugging");
        assert!(!response.success);
    });
}
