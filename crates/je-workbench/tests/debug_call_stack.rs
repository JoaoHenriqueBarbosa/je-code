//! Porte de `src/vs/workbench/contrib/debug/test/browser/callStack.test.ts` (suíte `Debug - CallStack`), caso a caso e na mesma ordem.
//!
//! Os 13 casos: `threads simple`, `threads multiple with allThreadsStopped`, `allThreadsStopped in multiple events`,
//! `threads multiple without allThreadsStopped`, `stack frame get specific source name`, `stack frame toString()`,
//! `debug child sessions are added in correct order`, `replacing an inactive root session removes its child sessions`,
//! `adding a concurrent root session preserves inactive children of an active root session`, `decorations`, `contexts`,
//! `item-level stack depth contexts`, `focusStackFrameThreadAndSession`. Nenhum ficou de fora.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` de cada caso; `disposables.add(x)` é o `store` do guarda.
//! - `createTestSession` (`tests/support/mock_debug.rs`) e `createMockDebugModel` seguem o original; o `store` extra é onde
//!   vai o `ViewModel` dos serviços da sessão.
//! - `new class extends Thread { getCallStack() { return stackFrames; } }` é [`Thread::set_call_stack`], chamado depois de
//!   criar os quadros (que apontam para a thread).
//! - `session.raw = upcastPartial<RawDebugSession>(mockRawSession)` é `session.set_raw(...)` com o `MockRawSession`;
//!   `sinon.spy(mockRawSession, 'stackTrace')` é o contador `stack_trace_call_count`.
//! - `new class extends DebugSession { override get state() { return State.Stopped; } }` do último caso é uma sessão do
//!   `createTestSession` com [`DebugSession::override_state`]; o original passa `undefined` como `debugService`, e o estado
//!   sobrescrito nunca o lê. O `new StackFrame(thread, 5, undefined!, ...)` ganha uma `Source` sem dados, porque o
//!   `StackFrame` do porte exige a fonte (o caso só lê a thread e a sessão do quadro).
//! - `assert.deepStrictEqual(toFocus, { stackFrame, thread, session })` compara cada campo por identidade (`Rc`).
//! - Os valores do overlay de contexto (`ContextKeyValue`) são `Dynamic`; `stackDepth` lê o booleano.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::dynamic::Dynamic;
use je_base::uint::constants::MAX_SAFE_SMALL_INTEGER;
use je_editor::range::Range;
use je_platform::log::NullLogService;
use je_workbench::contrib::debug::call_stack::{
    CallStackItem, ContributedActionsContext, StackFrameThreadAndSession, create_decorations_for_stack_frame, debug_stackframe_class_name,
    debug_stackframe_focused_class_name, get_context, get_context_for_contributed_actions, get_session_context_overlay, get_specific_source_name,
    get_stack_frame_thread_and_session_to_focus, get_thread_context_overlay,
};
use je_workbench::contrib::debug::debug::IDebugSession;
use je_workbench::contrib::debug::debug_model::{DebugModel, StackFrame, Thread};
use je_workbench::contrib::debug::debug_protocol::Source as ProtocolSource;
use je_workbench::contrib::debug::debug_session::{DebugSession, DebugSessionOptions};
use je_workbench::contrib::debug::debug_source::Source;
use je_workbench::contrib::debug::debug_types::{DapThread, IDebugSessionFull, IRawModelUpdate, IRawStoppedDetails, State, StoppedDetails, new_stopped_details};
use je_workbench::contrib::debug::debug_view_model::CONTEXT_THREAD_HAS_MULTIPLE_STACK_FRAMES_KEY;
use support::mock_debug::{MockRawSession, create_mock_debug_model, create_test_session, mock_uri_identity_service};

fn session_of(model: &Rc<DebugModel>, name: &str, options: Option<DebugSessionOptions>, guard: &common::LeakGuard) -> Rc<DebugSession> {
    guard.add_rc(create_test_session(model, name, options, &guard.store()))
}

fn source(name: &str, path: &str, source_reference: i64) -> Rc<Source> {
    Rc::new(Source::new(
        Some(ProtocolSource { name: Some(name.to_owned()), path: Some(path.to_owned()), source_reference: Some(source_reference), ..ProtocolSource::default() }),
        "aDebugSessionId",
        &*mock_uri_identity_service(),
        &NullLogService,
    ))
}

fn range(start_line: i32, start_column: i32, end_line: i32, end_column: i32) -> Option<Range> {
    Some(Range::new(start_line, start_column, end_line, end_column))
}

fn thread_update(id: i64, name: &str) -> DapThread {
    DapThread { id, name: name.to_owned() }
}

fn stopped(reason: &str, thread_id: i64, all_threads_stopped: bool) -> StoppedDetails {
    new_stopped_details(IRawStoppedDetails { reason: Some(reason.to_owned()), thread_id: Some(thread_id), all_threads_stopped: Some(all_threads_stopped), ..IRawStoppedDetails::default() })
}

fn create_single_stack_frame(session: &Rc<DebugSession>) -> Rc<StackFrame> {
    let thread = Thread::new(session.clone(), "mockthread", 1);
    let source = source("internalModule.js", "a/b/c/d/internalModule.js", 10);
    let stack_frame = StackFrame::new(thread.clone(), 0, source, "app.js", Some("normal"), range(1, 2, 1, 10), 0, true, None);
    thread.set_call_stack(vec![stack_frame.clone()]);
    // The adapter confirms this is the only frame in the stack.
    thread.set_stopped_details(Some(IRawStoppedDetails { total_frames: Some(1), ..IRawStoppedDetails::default() }));
    stack_frame
}

fn create_partially_loaded_multiple_frame_stack(session: &Rc<DebugSession>) -> Rc<Thread> {
    let thread = Thread::new(session.clone(), "mockthread", 1);
    let source = source("internalModule.js", "z/x/c/d/internalModule.js", 11);
    let loaded_frame = StackFrame::new(thread.clone(), 0, source, "app2.js", Some("normal"), range(1, 2, 1, 10), 0, true, None);
    thread.set_call_stack(vec![loaded_frame]);
    // The adapter reports two frames, but only the top frame has been loaded so far.
    thread.set_stopped_details(Some(IRawStoppedDetails { total_frames: Some(2), ..IRawStoppedDetails::default() }));
    thread
}

fn create_partially_loaded_stack_with_unknown_depth(session: &Rc<DebugSession>) -> Rc<Thread> {
    let thread = Thread::new(session.clone(), "mockthread", 1);
    let source = source("internalModule.js", "z/x/c/d/internalModule.js", 11);
    let loaded_frame = StackFrame::new(thread.clone(), 0, source, "app2.js", Some("normal"), range(1, 2, 1, 10), 0, true, None);
    thread.set_call_stack(vec![loaded_frame]);
    // The adapter does not provide totalFrames, so the stack depth is still unknown.
    thread
}

fn create_two_stack_frames(session: &Rc<DebugSession>) -> (Rc<StackFrame>, Rc<StackFrame>) {
    let thread = Thread::new(session.clone(), "mockthread", 1);

    let first_source = source("internalModule.js", "a/b/c/d/internalModule.js", 10);
    let second_source = source("internalModule.js", "z/x/c/d/internalModule.js", 11);

    let first_stack_frame = StackFrame::new(thread.clone(), 0, first_source, "app.js", Some("normal"), range(1, 2, 1, 10), 0, true, None);
    let second_stack_frame = StackFrame::new(thread.clone(), 1, second_source, "app2.js", Some("normal"), range(1, 2, 1, 10), 1, true, None);
    thread.set_call_stack(vec![first_stack_frame.clone(), second_stack_frame.clone()]);

    (first_stack_frame, second_stack_frame)
}

fn stack_depth(overlay: &[(String, Dynamic)]) -> Option<bool> {
    overlay.iter().find(|(key, _)| key == CONTEXT_THREAD_HAS_MULTIPLE_STACK_FRAMES_KEY).map(|(_, value)| matches!(value, Dynamic::Bool(true)))
}

fn assert_same_session(actual: &Option<Rc<dyn IDebugSessionFull>>, expected: &Rc<dyn IDebugSessionFull>) {
    let actual = actual.as_ref().expect("a sessão em foco");
    assert!(std::ptr::addr_eq(Rc::as_ptr(actual), Rc::as_ptr(expected)));
}

fn assert_focus(
    actual: &StackFrameThreadAndSession,
    stack_frame: Option<&Rc<StackFrame>>,
    thread: Option<&Rc<Thread>>,
    session: Option<&Rc<dyn IDebugSessionFull>>,
) {
    match (&actual.stack_frame, stack_frame) {
        (None, None) => {}
        (Some(a), Some(b)) => assert!(Rc::ptr_eq(a, b)),
        _ => panic!("o quadro de pilha em foco difere"),
    }
    match (&actual.thread, thread) {
        (None, None) => {}
        (Some(a), Some(b)) => assert!(Rc::ptr_eq(a, b)),
        _ => panic!("a thread em foco difere"),
    }
    match (&actual.session, session) {
        (None, None) => {}
        (Some(_), Some(expected)) => assert_same_session(&actual.session, expected),
        _ => panic!("a sessão em foco difere"),
    }
}

// Threads

#[test]
fn threads_simple() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let thread_id = 1;
    let thread_name = "firstThread";
    let session = session_of(&model, "mockSession", None, &guard);
    model.add_session(session.clone());

    assert_eq!(model.get_sessions(true).len(), 1);
    model.raw_update(IRawModelUpdate { session_id: session.get_id(), threads: vec![thread_update(thread_id, thread_name)], stopped_details: None });

    assert_eq!(*session.get_thread(thread_id).unwrap().name.borrow(), thread_name);

    model.clear_threads(&session.get_id(), true, None);
    assert!(session.get_thread(thread_id).is_none());
    assert_eq!(model.get_sessions(true).len(), 1);
}

#[test]
fn threads_multiple_with_all_threads_stopped() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let mock_raw_session = MockRawSession::new();
    run_with_faked_timers(async move {
        let thread_id1 = 1;
        let thread_name1 = "firstThread";
        let thread_id2 = 2;
        let thread_name2 = "secondThread";
        let stopped_reason = "breakpoint";

        // Add the threads
        let session = session_of(&model, "mockSession", None, &guard);
        model.add_session(session.clone());

        session.set_raw(Some(mock_raw_session.clone()));

        model.raw_update(IRawModelUpdate { session_id: session.get_id(), threads: vec![thread_update(thread_id1, thread_name1)], stopped_details: None });

        // Stopped event with all threads stopped
        model.raw_update(IRawModelUpdate {
            session_id: session.get_id(),
            threads: vec![thread_update(thread_id1, thread_name1), thread_update(thread_id2, thread_name2)],
            stopped_details: Some(new_stopped_details(IRawStoppedDetails {
                reason: Some(stopped_reason.to_owned()),
                thread_id: Some(1),
                all_threads_stopped: Some(true),
                ..IRawStoppedDetails::default()
            })),
        });

        let thread1 = session.get_thread(thread_id1).unwrap();
        let thread2 = session.get_thread(thread_id2).unwrap();

        // at the beginning, callstacks are obtainable but not available
        assert_eq!(session.get_all_threads().len(), 2);
        assert_eq!(*thread1.name.borrow(), thread_name1);
        assert!(thread1.stopped.get());
        assert_eq!(thread1.get_call_stack().len(), 0);
        assert_eq!(thread1.stopped_details().unwrap().reason.as_deref(), Some(stopped_reason));
        assert_eq!(*thread2.name.borrow(), thread_name2);
        assert!(thread2.stopped.get());
        assert_eq!(thread2.get_call_stack().len(), 0);
        assert_eq!(thread2.stopped_details().unwrap().reason, None);

        // after calling getCallStack, the callstack becomes available
        // and results in a request for the callstack in the debug adapter
        thread1.fetch_call_stack(None).await.unwrap();
        assert_ne!(thread1.get_call_stack().len(), 0);

        thread2.fetch_call_stack(None).await.unwrap();
        assert_ne!(thread2.get_call_stack().len(), 0);

        // calling multiple times getCallStack doesn't result in multiple calls
        // to the debug adapter
        thread1.fetch_call_stack(None).await.unwrap();
        thread2.fetch_call_stack(None).await.unwrap();

        // clearing the callstack results in the callstack not being available
        thread1.clear_call_stack();
        assert!(thread1.stopped.get());
        assert_eq!(thread1.get_call_stack().len(), 0);

        thread2.clear_call_stack();
        assert!(thread2.stopped.get());
        assert_eq!(thread2.get_call_stack().len(), 0);

        model.clear_threads(&session.get_id(), true, None);
        assert!(session.get_thread(thread_id1).is_none());
        assert!(session.get_thread(thread_id2).is_none());
        assert_eq!(session.get_all_threads().len(), 0);
    });
}

#[test]
fn all_threads_stopped_in_multiple_events() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let mock_raw_session = MockRawSession::new();
    let thread_id1 = 1;
    let thread_name1 = "firstThread";
    let thread_id2 = 2;
    let thread_name2 = "secondThread";
    let stopped_reason = "breakpoint";

    // Add the threads
    let session = session_of(&model, "mockSession", None, &guard);
    model.add_session(session.clone());

    session.set_raw(Some(mock_raw_session));

    // Stopped event with all threads stopped
    model.raw_update(IRawModelUpdate {
        session_id: session.get_id(),
        threads: vec![thread_update(thread_id1, thread_name1), thread_update(thread_id2, thread_name2)],
        stopped_details: Some(stopped(stopped_reason, thread_id1, true)),
    });

    model.raw_update(IRawModelUpdate {
        session_id: session.get_id(),
        threads: vec![thread_update(thread_id1, thread_name1), thread_update(thread_id2, thread_name2)],
        stopped_details: Some(stopped(stopped_reason, thread_id2, true)),
    });

    let thread1 = session.get_thread(thread_id1).unwrap();
    let thread2 = session.get_thread(thread_id2).unwrap();

    assert_eq!(thread1.stopped_details().and_then(|d| d.reason).as_deref(), Some(stopped_reason));
    assert_eq!(thread2.stopped_details().and_then(|d| d.reason).as_deref(), Some(stopped_reason));
}

#[test]
fn threads_multiple_without_all_threads_stopped() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let mock_raw_session = MockRawSession::new();
    run_with_faked_timers(async move {
        let session_stub = mock_raw_session.clone();

        let stopped_thread_id = 1;
        let stopped_thread_name = "stoppedThread";
        let running_thread_id = 2;
        let running_thread_name = "runningThread";
        let stopped_reason = "breakpoint";
        let session = session_of(&model, "mockSession", None, &guard);
        model.add_session(session.clone());

        session.set_raw(Some(mock_raw_session.clone()));

        // Add the threads
        model.raw_update(IRawModelUpdate { session_id: session.get_id(), threads: vec![thread_update(stopped_thread_id, stopped_thread_name)], stopped_details: None });

        // Stopped event with only one thread stopped
        model.raw_update(IRawModelUpdate {
            session_id: session.get_id(),
            threads: vec![thread_update(1, stopped_thread_name), thread_update(running_thread_id, running_thread_name)],
            stopped_details: Some(stopped(stopped_reason, 1, false)),
        });

        let stopped_thread = session.get_thread(stopped_thread_id).unwrap();
        let running_thread = session.get_thread(running_thread_id).unwrap();

        // the callstack for the stopped thread is obtainable but not available
        // the callstack for the running thread is not obtainable nor available
        assert_eq!(*stopped_thread.name.borrow(), stopped_thread_name);
        assert!(stopped_thread.stopped.get());
        assert_eq!(session.get_all_threads().len(), 2);
        assert_eq!(stopped_thread.get_call_stack().len(), 0);
        assert_eq!(stopped_thread.stopped_details().unwrap().reason.as_deref(), Some(stopped_reason));
        assert_eq!(*running_thread.name.borrow(), running_thread_name);
        assert!(!running_thread.stopped.get());
        assert_eq!(running_thread.get_call_stack().len(), 0);
        assert!(running_thread.stopped_details().is_none());

        // after calling getCallStack, the callstack becomes available
        // and results in a request for the callstack in the debug adapter
        stopped_thread.fetch_call_stack(None).await.unwrap();
        assert_ne!(stopped_thread.get_call_stack().len(), 0);
        assert_eq!(running_thread.get_call_stack().len(), 0);
        assert_eq!(session_stub.stack_trace_call_count(), 1);

        // calling getCallStack on the running thread returns empty array
        // and does not return in a request for the callstack in the debug
        // adapter
        running_thread.fetch_call_stack(None).await.unwrap();
        assert_eq!(running_thread.get_call_stack().len(), 0);
        assert_eq!(session_stub.stack_trace_call_count(), 1);

        // clearing the callstack results in the callstack not being available
        stopped_thread.clear_call_stack();
        assert!(stopped_thread.stopped.get());
        assert_eq!(stopped_thread.get_call_stack().len(), 0);

        model.clear_threads(&session.get_id(), true, None);
        assert!(session.get_thread(stopped_thread_id).is_none());
        assert!(session.get_thread(running_thread_id).is_none());
        assert_eq!(session.get_all_threads().len(), 0);
    });
}

#[test]
fn stack_frame_get_specific_source_name() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = session_of(&model, "mockSession", None, &guard);
    model.add_session(session.clone());
    let (first_stack_frame, second_stack_frame) = create_two_stack_frames(&session);

    assert_eq!(get_specific_source_name(&first_stack_frame), ".../b/c/d/internalModule.js");
    assert_eq!(get_specific_source_name(&second_stack_frame), ".../x/c/d/internalModule.js");
}

#[test]
fn stack_frame_to_string() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = session_of(&model, "mockSession", None, &guard);
    let thread = Thread::new(session.clone(), "mockthread", 1);
    let first_source = source("internalModule.js", "a/b/c/d/internalModule.js", 10);
    let stack_frame = StackFrame::new(thread.clone(), 1, first_source, "app", Some("normal"), range(1, 1, 1, 10), 1, true, None);
    assert_eq!(stack_frame.to_display_string(), "app (internalModule.js:1)");

    let second_source = Rc::new(Source::new(None, "aDebugSessionId", &*mock_uri_identity_service(), &NullLogService));
    let stack_frame2 = StackFrame::new(thread, 2, second_source, "module", Some("normal"), None, 2, true, None);
    assert_eq!(stack_frame2.to_display_string(), "module");
}

#[test]
fn debug_child_sessions_are_added_in_correct_order() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let parent_of = |session: &Rc<DebugSession>| Some(DebugSessionOptions { parent_session: Some(session.clone() as Rc<dyn IDebugSessionFull>), ..DebugSessionOptions::default() });

    let session = session_of(&model, "mockSession", None, &guard);
    model.add_session(session.clone());
    let second_session = session_of(&model, "mockSession2", None, &guard);
    model.add_session(second_session.clone());
    let first_child = session_of(&model, "firstChild", parent_of(&session), &guard);
    model.add_session(first_child.clone());
    let second_child = session_of(&model, "secondChild", parent_of(&session), &guard);
    model.add_session(second_child.clone());
    let third_session = session_of(&model, "mockSession3", None, &guard);
    model.add_session(third_session.clone());
    let another_child = session_of(&model, "secondChild", parent_of(&second_session), &guard);
    model.add_session(another_child.clone());

    let sessions = model.get_sessions(false);
    assert_eq!(sessions[0].get_id(), session.get_id());
    assert_eq!(sessions[1].get_id(), first_child.get_id());
    assert_eq!(sessions[2].get_id(), second_child.get_id());
    assert_eq!(sessions[3].get_id(), second_session.get_id());
    assert_eq!(sessions[4].get_id(), another_child.get_id());
    assert_eq!(sessions[5].get_id(), third_session.get_id());
}

#[test]
fn replacing_an_inactive_root_session_removes_its_child_sessions() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    run_with_faked_timers(async move {
        let old_root = create_test_session(&model, "mockSession", None, &guard.store());
        model.add_session(old_root.clone());
        let old_child = create_test_session(
            &model,
            "oldChild",
            Some(DebugSessionOptions { parent_session: Some(old_root.clone() as Rc<dyn IDebugSessionFull>), ..DebugSessionOptions::default() }),
            &guard.store(),
        );
        model.add_session(old_child.clone());
        old_child.terminate(false).await.unwrap();
        old_root.terminate(false).await.unwrap();

        let replacement = session_of(&model, "mockSession", None, &guard);
        model.add_session(replacement.clone());

        let ids: Vec<String> = model.get_sessions(true).iter().map(|session| session.get_id()).collect();
        assert_eq!(ids, vec![replacement.get_id()]);
    });
}

#[test]
fn adding_a_concurrent_root_session_preserves_inactive_children_of_an_active_root_session() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    run_with_faked_timers(async move {
        let active_root = session_of(&model, "mockSession", None, &guard);
        model.add_session(active_root.clone());
        let inactive_child = session_of(
            &model,
            "inactiveChild",
            Some(DebugSessionOptions { parent_session: Some(active_root.clone() as Rc<dyn IDebugSessionFull>), ..DebugSessionOptions::default() }),
            &guard,
        );
        model.add_session(inactive_child.clone());
        inactive_child.terminate(false).await.unwrap();

        let concurrent_root = session_of(&model, "mockSession", None, &guard);
        model.add_session(concurrent_root.clone());

        let ids: Vec<String> = model.get_sessions(true).iter().map(|session| session.get_id()).collect();
        assert_eq!(ids, vec![active_root.get_id(), inactive_child.get_id(), concurrent_root.get_id()]);
    });
}

#[test]
fn decorations() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = session_of(&model, "mockSession", None, &guard);
    model.add_session(session.clone());
    let (first_stack_frame, second_stack_frame) = create_two_stack_frames(&session);
    let mut decorations = create_decorations_for_stack_frame(&first_stack_frame, true, false);
    assert_eq!(decorations.len(), 3);
    assert_eq!(decorations[0].range, Range::new(1, 2, 1, 3));
    assert_eq!(decorations[0].options.glyph_margin_class_name, Some(debug_stackframe_class_name()));
    assert_eq!(decorations[1].range, Range::new(1, 2, 1, MAX_SAFE_SMALL_INTEGER));
    assert_eq!(decorations[1].options.class_name.as_deref(), Some("debug-top-stack-frame-line"));
    assert!(decorations[1].options.is_whole_line);

    decorations = create_decorations_for_stack_frame(&second_stack_frame, true, false);
    assert_eq!(decorations.len(), 2);
    assert_eq!(decorations[0].range, Range::new(1, 2, 1, 3));
    assert_eq!(decorations[0].options.glyph_margin_class_name, Some(debug_stackframe_focused_class_name()));
    assert_eq!(decorations[1].range, Range::new(1, 2, 1, MAX_SAFE_SMALL_INTEGER));
    assert_eq!(decorations[1].options.class_name.as_deref(), Some("debug-focused-stack-frame-line"));
    assert!(decorations[1].options.is_whole_line);

    decorations = create_decorations_for_stack_frame(&first_stack_frame, true, false);
    assert_eq!(decorations.len(), 3);
    assert_eq!(decorations[0].range, Range::new(1, 2, 1, 3));
    assert_eq!(decorations[0].options.glyph_margin_class_name, Some(debug_stackframe_class_name()));
    assert_eq!(decorations[1].range, Range::new(1, 2, 1, MAX_SAFE_SMALL_INTEGER));
    assert_eq!(decorations[1].options.class_name.as_deref(), Some("debug-top-stack-frame-line"));
    assert!(decorations[1].options.is_whole_line);
    // Inline decoration gets rendered in this case
    assert_eq!(decorations[2].options.before.as_ref().and_then(|before| before.inline_class_name.as_deref()), Some("debug-top-stack-frame-column"));
    assert_eq!(decorations[2].range, Range::new(1, 2, 1, MAX_SAFE_SMALL_INTEGER));
}

#[test]
fn contexts() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = session_of(&model, "mockSession", None, &guard);
    model.add_session(session.clone());
    let (first_stack_frame, second_stack_frame) = create_two_stack_frames(&session);
    let session_item = CallStackItem::Session(session.clone());
    let mut context = get_context(Some(&CallStackItem::StackFrame(first_stack_frame.clone()))).unwrap();
    assert_eq!(context.session_id, first_stack_frame.thread.session.get_id());
    assert_eq!(context.thread_id, Some(first_stack_frame.thread.get_id()));
    assert_eq!(context.frame_id, Some(first_stack_frame.get_id()));

    context = get_context(Some(&CallStackItem::Thread(second_stack_frame.thread.clone()))).unwrap();
    assert_eq!(context.session_id, second_stack_frame.thread.session.get_id());
    assert_eq!(context.thread_id, Some(second_stack_frame.thread.get_id()));
    assert_eq!(context.frame_id, None);

    context = get_context(Some(&session_item)).unwrap();
    assert_eq!(context.session_id, session.get_id());
    assert_eq!(context.thread_id, None);
    assert_eq!(context.frame_id, None);

    let mut contributed_context = get_context_for_contributed_actions(Some(&CallStackItem::StackFrame(first_stack_frame.clone())));
    assert_eq!(contributed_context, ContributedActionsContext::Text(first_stack_frame.source.raw.borrow().path.clone().unwrap()));
    contributed_context = get_context_for_contributed_actions(Some(&CallStackItem::Thread(first_stack_frame.thread.clone())));
    assert_eq!(contributed_context, ContributedActionsContext::Number(first_stack_frame.thread.thread_id));
    contributed_context = get_context_for_contributed_actions(Some(&session_item));
    assert_eq!(contributed_context, ContributedActionsContext::Text(session.get_id()));
}

#[test]
fn item_level_stack_depth_contexts() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let session = session_of(&model, "mockSession", None, &guard);
    model.add_session(session.clone());

    let single_frame = create_single_stack_frame(&session);
    let partially_loaded_multiple_frame_stack = create_partially_loaded_multiple_frame_stack(&session);

    assert_eq!(stack_depth(&get_thread_context_overlay(&single_frame.thread)), Some(false));
    assert_eq!(stack_depth(&get_thread_context_overlay(&partially_loaded_multiple_frame_stack)), Some(true));

    let partially_loaded_unknown_depth_stack = create_partially_loaded_stack_with_unknown_depth(&session);
    assert_eq!(stack_depth(&get_thread_context_overlay(&partially_loaded_unknown_depth_stack)), Some(true));

    partially_loaded_unknown_depth_stack.reached_end_of_call_stack.set(true);
    assert_eq!(stack_depth(&get_thread_context_overlay(&partially_loaded_unknown_depth_stack)), Some(false));

    model.raw_update(IRawModelUpdate {
        session_id: session.get_id(),
        threads: vec![thread_update(1, "partiallyLoadedThread")],
        stopped_details: Some(new_stopped_details(IRawStoppedDetails {
            reason: Some("breakpoint".to_owned()),
            thread_id: Some(1),
            total_frames: Some(2),
            all_threads_stopped: Some(true),
            ..IRawStoppedDetails::default()
        })),
    });
    let session_full: Rc<dyn IDebugSessionFull> = session.clone();
    assert_eq!(stack_depth(&get_session_context_overlay(&session_full)), Some(true));
}

#[test]
fn focus_stack_frame_thread_and_session() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let model = create_mock_debug_model(&guard.store());
    let mock_raw_session = MockRawSession::new();
    let thread_id1 = 1;
    let thread_name1 = "firstThread";
    let thread_id2 = 2;
    let thread_name2 = "secondThread";
    let stopped_reason = "breakpoint";

    // Add the threads
    let session = session_of(&model, "stoppedSession", None, &guard);
    session.override_state(Some(State::Stopped));

    let running_session = session_of(&model, "mockSession", None, &guard);
    model.add_session(running_session.clone());
    model.add_session(session.clone());

    session.set_raw(Some(mock_raw_session));

    model.raw_update(IRawModelUpdate { session_id: session.get_id(), threads: vec![thread_update(thread_id1, thread_name1)], stopped_details: None });

    // Stopped event with all threads stopped
    model.raw_update(IRawModelUpdate {
        session_id: session.get_id(),
        threads: vec![thread_update(thread_id1, thread_name1), thread_update(thread_id2, thread_name2)],
        stopped_details: Some(stopped(stopped_reason, 1, true)),
    });

    let thread = session.get_thread(thread_id1).unwrap();
    let running_thread = session.get_thread(thread_id2).unwrap();
    let session_full: Rc<dyn IDebugSessionFull> = session.clone();
    let running_session_full: Rc<dyn IDebugSessionFull> = running_session.clone();

    let mut to_focus = get_stack_frame_thread_and_session_to_focus(&model, None, None, None, None);
    // Verify stopped session and stopped thread get focused
    assert_focus(&to_focus, None, Some(&thread), Some(&session_full));

    to_focus = get_stack_frame_thread_and_session_to_focus(&model, None, None, Some(running_session_full.clone()), None);
    assert_focus(&to_focus, None, None, Some(&running_session_full));

    to_focus = get_stack_frame_thread_and_session_to_focus(&model, None, Some(thread.clone()), None, None);
    assert_focus(&to_focus, None, Some(&thread), Some(&session_full));

    to_focus = get_stack_frame_thread_and_session_to_focus(&model, None, Some(running_thread.clone()), None, None);
    assert_focus(&to_focus, None, Some(&running_thread), Some(&session_full));

    let no_source = Rc::new(Source::new(None, "aDebugSessionId", &*mock_uri_identity_service(), &NullLogService));
    let stack_frame = StackFrame::new(thread.clone(), 5, no_source, "stackframename2", None, None, 1, true, None);
    to_focus = get_stack_frame_thread_and_session_to_focus(&model, Some(stack_frame.clone()), None, None, None);
    assert_focus(&to_focus, Some(&stack_frame), Some(&thread), Some(&session_full));
}
