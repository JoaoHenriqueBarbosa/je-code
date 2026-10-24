//! Porte de `src/vs/workbench/contrib/debug/test/common/debugModel.test.ts` (suíte `DebugModel`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso; o `DisposableStore` do caso
//!   é o `store` do guarda (descartado no fim).
//! - `createModel(disposable)` usa o `MockDebugStorage` (`tests/support/mock_debug.rs`), o `ITextFileService` com
//!   `isDirty => false`, o serviço de identidade de URI `undefined` e o `NullLogService`.
//! - `JSON.stringify(fbp)` é o `to_json()` do breakpoint (o valor que o `JSON.stringify` serializaria).
//! - `BigInt(0x1000)` é `u128`.
//! - A `upcastPartial<Thread>({...})` e o `mockObject<Thread>()` dos dois últimos casos são o dublê `FakeThread` (o trait
//!   `CallStackThread`, que é o que o `refreshTopOfCallstack` lê da thread); o `session.capabilities` é o objeto JSON. A
//!   `IStackFrame` vazia do primeiro é um `StackFrame` sobre uma thread do `MockSession`, porque o `getCallStack` do dublê
//!   devolve `StackFrame`.
//! - `runWithFakedTimers({}, ...)` é o `run_with_faked_timers` do je-base; o `timeout(420)` é o `timeout` dele.
//! - `new DeferredPromise<void>()` e `await deferred.complete()` são o `DeferredPromise` do je-base.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{DeferredPromise, Promise};
use je_platform::log::NullLogService;
use je_workbench::contrib::debug::breakpoints::{
    ExceptionBreakpoint, ExceptionBreakpointOptions, FunctionBreakpoint, FunctionBreakpointOptions, IBaseBreakpointOptions, InstructionBreakpointOptions,
};
use je_workbench::contrib::debug::debug_model::{CallStackThread, DebugModel, StackFrame, Thread};
use je_workbench::contrib::debug::debug_source::Source;
use serde_json::{Value, json};
use support::mock_debug::{MockSession, NotDirty, mock_debug_storage, mock_uri_identity_service, storage_service_of};
use je_workbench::test::common::workbench_test_services::TestStorageService;

fn create_model(disposable: &DisposableStore) -> Rc<DebugModel> {
    let storage = TestStorageService::new();
    disposable.add_rc(storage.clone());
    let debug_storage = mock_debug_storage(storage_service_of(&storage));
    disposable.add_rc(debug_storage.clone());
    let model = DebugModel::new(debug_storage, Rc::new(NotDirty), None, NullLogService::shared());
    disposable.add_rc(model.clone());
    model
}

fn instruction_options(instruction_reference: &str, offset: i64, address: u128) -> InstructionBreakpointOptions {
    InstructionBreakpointOptions {
        instruction_reference: instruction_reference.to_owned(),
        offset,
        address,
        can_persist: false,
        base: IBaseBreakpointOptions { enabled: Some(true), ..IBaseBreakpointOptions::default() },
    }
}

#[test]
fn function_breakpoint_id_is_saved() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let fbp = FunctionBreakpoint::new(
        FunctionBreakpointOptions {
            name: "function".to_owned(),
            base: IBaseBreakpointOptions {
                enabled: Some(true),
                hit_condition: Some("hit condition".to_owned()),
                condition: Some("condition".to_owned()),
                log_message: Some("log message".to_owned()),
                ..IBaseBreakpointOptions::default()
            },
        },
        None,
    );
    let stringified = fbp.to_json().to_string();
    let parsed: Value = serde_json::from_str(&stringified).unwrap();
    assert_eq!(parsed["id"], json!(fbp.get_id()));
}

// Regression test for microsoft/vscode#289678: if the debug adapter hands out a new `instructionReference` for the same
// memory location (e.g. after a symbol reload or certain stepping operations), removal by reference+offset must still
// succeed when the caller supplies the resolved address.
#[test]
fn instruction_breakpoint_remove_prefers_address_match_when_instruction_reference_has_changed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposable = guard.store();
    let model = create_model(&disposable);
    let address: u128 = 0x1000;
    model.add_instruction_breakpoint(instruction_options("oldRef", 0, address));

    assert_eq!(model.get_instruction_breakpoints().len(), 1);

    // Simulate the disassembly view asking for removal after the debug adapter handed out a new instruction reference.
    model.remove_instruction_breakpoints(Some("newRef"), Some(0), Some(address));

    assert_eq!(model.get_instruction_breakpoints().len(), 0);
}

#[test]
fn instruction_breakpoint_remove_falls_back_to_instruction_reference_and_offset_when_address_not_supplied() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposable = guard.store();
    let model = create_model(&disposable);
    model.add_instruction_breakpoint(instruction_options("ref", 4, 0x2000));

    // Non-matching reference leaves the breakpoint in place.
    model.remove_instruction_breakpoints(Some("other"), Some(4), None);
    assert_eq!(model.get_instruction_breakpoints().len(), 1);

    // Matching reference+offset removes it.
    model.remove_instruction_breakpoints(Some("ref"), Some(4), None);
    assert_eq!(model.get_instruction_breakpoints().len(), 0);
}

#[test]
fn instruction_breakpoint_remove_with_only_address_removes_the_matching_entry_and_leaves_others() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disposable = guard.store();
    let model = create_model(&disposable);
    let mut keep = Vec::new();

    model.add_instruction_breakpoint(instruction_options("refA", 0, 0x3000));
    model.add_instruction_breakpoint(instruction_options("refB", 0, 0x4000));

    model.remove_instruction_breakpoints(None, None, Some(0x3000));

    let remaining = model.get_instruction_breakpoints();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].address, 0x4000);
    keep.extend(remaining);
    assert_eq!(keep.len(), 1);
}

#[test]
fn exception_breakpoint_restored_matches_new() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ebp = ExceptionBreakpoint::new(
        ExceptionBreakpointOptions {
            condition_description: Some("condition description".to_owned()),
            description: Some("description".to_owned()),
            filter: "condition".to_owned(),
            label: "label".to_owned(),
            supports_condition: true,
            base: IBaseBreakpointOptions { enabled: Some(true), ..IBaseBreakpointOptions::default() },
            fallback: None,
        },
        Some("id".to_owned()),
    );
    let stringified = ebp.to_json().to_string();
    let parsed: Value = serde_json::from_str(&stringified).unwrap();
    let new_ebp = ExceptionBreakpoint::new(ExceptionBreakpointOptions::from_json(&parsed), None);
    assert!(ebp.matches(&new_ebp.as_filter()));
}

/// A thread falsa de `refreshTopOfCallstack`.
struct FakeThread {
    id: String,
    frames: Vec<Rc<StackFrame>>,
    reached_end_of_call_stack: Rc<Cell<bool>>,
    fetch: Box<dyn Fn(i64) -> Promise<()>>,
}

impl CallStackThread for FakeThread {
    fn session_capabilities(&self) -> Value {
        json!({ "supportsDelayedStackTraceLoading": true })
    }

    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn fetch_call_stack(&self, levels: Option<i64>) -> Promise<()> {
        (self.fetch)(levels.expect("levels"))
    }

    fn get_stale_call_stack(&self) -> Vec<Rc<StackFrame>> {
        self.frames.clone()
    }

    fn get_call_stack(&self) -> Vec<Rc<StackFrame>> {
        self.frames.clone()
    }

    fn reached_end_of_call_stack(&self) -> bool {
        self.reached_end_of_call_stack.get()
    }
}

#[test]
fn refresh_top_of_callstack_notifies_when_a_repeated_stop_confirms_the_stack_is_complete_without_changing_frames() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let session = MockSession::new();
        let mock_thread = Thread::new(session, "thread", 1);
        let source = Rc::new(Source::new(None, "mock", &*mock_uri_identity_service(), &NullLogService));
        let frame = StackFrame::new(mock_thread, 0, source, "frame", None, None, 0, true, None);

        let reached = Rc::new(Cell::new(false));
        let fetch_reached = reached.clone();
        let fake_thread = Rc::new(FakeThread {
            id: "1".to_owned(),
            frames: vec![frame],
            reached_end_of_call_stack: reached,
            fetch: Box::new(move |levels| {
                if levels > 1 {
                    fetch_reached.set(true);
                }
                Promise::resolved(())
            }),
        });

        let disposable = DisposableStore::new();
        let storage = TestStorageService::new();
        disposable.add_rc(storage.clone());
        let model = DebugModel::new(mock_debug_storage(storage_service_of(&storage)), Rc::new(NotDirty), None, NullLogService::shared());
        disposable.add_rc(model.clone());

        let call_stack_change_count = Rc::new(Cell::new(0));
        let counter = call_stack_change_count.clone();
        disposable.add(model.on_did_change_call_stack().listen(move |_| counter.set(counter.get() + 1)));
        let refresh = model.refresh_top_of_callstack(fake_thread, true);
        refresh.top_call_stack.await.unwrap();
        assert_eq!(call_stack_change_count.get(), 1);

        // Match the 420 ms delay hard-coded in refreshTopOfCallstack before it fetches the rest of the stack.
        timeout(420).await.unwrap();
        refresh.whole_call_stack.await.unwrap();
        assert_eq!(call_stack_change_count.get(), 2);

        disposable.dispose();
        store.dispose();
    });
}

#[test]
fn refresh_top_of_callstack_resolves_all_returned_promises_when_called_multiple_times() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let top_frame_deferred = Rc::new(DeferredPromise::<()>::new());
        let whole_stack_deferred = Rc::new(DeferredPromise::<()>::new());
        let top = top_frame_deferred.promise.clone();
        let whole = whole_stack_deferred.promise.clone();
        let fake_thread = Rc::new(FakeThread {
            id: "1".to_owned(),
            frames: Vec::new(),
            reached_end_of_call_stack: Rc::new(Cell::new(false)),
            fetch: Box::new(move |levels| if levels == 1 { top.clone() } else { whole.clone() }),
        });

        let disposable = DisposableStore::new();
        let storage = TestStorageService::new();
        disposable.add_rc(storage.clone());
        let model = DebugModel::new(mock_debug_storage(storage_service_of(&storage)), Rc::new(NotDirty), None, NullLogService::shared());
        disposable.add_rc(model.clone());

        let flags: Rc<RefCell<[bool; 4]>> = Rc::new(RefCell::new([false; 4]));
        let track = |promise: &Promise<()>, index: usize| {
            let flags = flags.clone();
            drop(promise.then(move |()| flags.borrow_mut()[index] = true));
        };
        let result1 = model.refresh_top_of_callstack(fake_thread.clone(), true);
        track(&result1.top_call_stack, 0);
        track(&result1.whole_call_stack, 1);

        let result2 = model.refresh_top_of_callstack(fake_thread.clone(), true);
        track(&result2.top_call_stack, 2);
        track(&result2.whole_call_stack, 3);

        assert!(!flags.borrow()[0]);
        assert!(!flags.borrow()[1]);
        assert!(!flags.borrow()[2]);
        assert!(!flags.borrow()[3]);

        top_frame_deferred.complete(()).await.unwrap();
        result1.top_call_stack.await.unwrap();
        result2.top_call_stack.await.unwrap();
        assert!(!flags.borrow()[1]);
        assert!(!flags.borrow()[3]);

        whole_stack_deferred.complete(()).await.unwrap();
        result1.whole_call_stack.await.unwrap();
        result2.whole_call_stack.await.unwrap();

        disposable.dispose();
        store.dispose();
    });
}
