//! Porte de `src/vs/workbench/contrib/debug/test/browser/debugViewModel.test.ts` (suíte `Debug - View Model`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - O `setup` (`new MockContextKeyService()` e `new ViewModel(contextKeyService)`) e o `teardown` (`model.dispose()`)
//!   rodam fora do `LeakGuard` de cada caso, como no original (o `ensureNoDisposablesAreLeakedInTestSuite()` é registrado
//!   depois do `setup`).
//! - `contextKeyService.getContextKeyValue(CONTEXT_THREAD_HAS_MULTIPLE_STACK_FRAMES.key)` é o `get_context_key_value` do
//!   `MockContextKeyService`; o valor booleano vira `bool`.
//! - `thread.stoppedDetails.totalFrames = 1` é `update_stopped_details`; `thread.getCallStack().push(frame)` é
//!   `set_call_stack` (a lista que `getCallStack` devolve).
//! - `assert.deepStrictEqual({ states, focusEventCount, focusedThread: model.focusedThread }, {..., focusedThread: thread})`
//!   compara `states` e `focusEventCount` por valor e a thread em foco por identidade.
//! - `model.getSelectedExpression()?.expression === expression` é a comparação do ponteiro do `Rc`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_editor::range::Range;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::NullLogService;
use je_workbench::contrib::debug::debug::IDebugSession;
use je_workbench::contrib::debug::debug_model::{Expression, StackFrame, Thread};
use je_workbench::contrib::debug::debug_protocol::{Source as ProtocolSource, SourcePresentationHint};
use je_workbench::contrib::debug::debug_source::Source;
use je_workbench::contrib::debug::debug_types::{IDebugSessionFull, IExpression, IRawStoppedDetails};
use je_workbench::contrib::debug::debug_view_model::{CONTEXT_THREAD_HAS_MULTIPLE_STACK_FRAMES_KEY, ViewModel};
use support::mock_debug::{MockSession, mock_uri_identity_service};

/// O `setup`.
fn setup() -> (Rc<MockContextKeyService>, Rc<ViewModel>) {
    let context_key_service = MockContextKeyService::new();
    let model = ViewModel::new(context_key_service.clone());
    (context_key_service, model)
}

fn stack_depth(context_key_service: &MockContextKeyService) -> bool {
    match context_key_service.get_context_key_value(CONTEXT_THREAD_HAS_MULTIPLE_STACK_FRAMES_KEY) {
        Dynamic::Bool(value) => value,
        other => panic!("valor inesperado da chave de contexto: {other:?}"),
    }
}

fn session_dyn(session: &Rc<MockSession>) -> Rc<dyn IDebugSessionFull> {
    session.clone()
}

fn one_by_one() -> Option<Range> {
    Some(Range::new(1, 1, 1, 1))
}

#[test]
fn focused_stack_frame() {
    let (_context_key_service, model) = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(model.focused_stack_frame().is_none());
    assert!(model.focused_thread().is_none());
    let session = MockSession::new();
    let thread = Thread::new(session.clone(), "myThread", 1);
    let source = Rc::new(Source::new(
        Some(ProtocolSource {
            name: Some("internalModule.js".to_owned()),
            source_reference: Some(11),
            presentation_hint: Some(SourcePresentationHint::Deemphasize),
            ..ProtocolSource::default()
        }),
        "aDebugSessionId",
        &*mock_uri_identity_service(),
        &NullLogService,
    ));
    let frame = StackFrame::new(thread.clone(), 1, source, "app.js", Some("normal"), one_by_one(), 0, true, None);
    model.set_focus(Some(frame.clone()), Some(thread.clone()), Some(session_dyn(&session)), false);

    assert_eq!(model.focused_stack_frame().unwrap().get_id(), frame.get_id());
    assert_eq!(model.focused_thread().unwrap().thread_id, 1);
    assert_eq!(model.focused_session().unwrap().get_id(), session.get_id());
    model.dispose();
}

#[test]
fn selected_expression() {
    let (_context_key_service, model) = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(model.get_selected_expression().is_none());
    let expression = Expression::new("my expression", None);
    let as_expression: Rc<dyn IExpression> = expression.clone();
    model.set_selected_expression(Some(as_expression), false);

    let selected = model.get_selected_expression().unwrap().expression;
    assert!(std::ptr::addr_eq(Rc::as_ptr(&selected), Rc::as_ptr(&expression)));
    model.dispose();
}

#[test]
fn focused_thread_stack_depth_refreshes_without_changing_focus() {
    let (context_key_service, model) = setup();
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let session = MockSession::new();
    let thread = Thread::new(session.clone(), "myThread", 1);
    thread.set_stopped_details(Some(IRawStoppedDetails { total_frames: Some(2), ..IRawStoppedDetails::default() }));
    model.set_focus(None, Some(thread.clone()), Some(session_dyn(&session)), false);

    let focus_event_count = Rc::new(Cell::new(0));
    let thread_counter = focus_event_count.clone();
    guard.add(model.on_did_focus_thread().listen(move |_| thread_counter.set(thread_counter.get() + 1)));
    let stack_frame_counter = focus_event_count.clone();
    guard.add(model.on_did_focus_stack_frame().listen(move |_| stack_frame_counter.set(stack_frame_counter.get() + 1)));
    let session_counter = focus_event_count.clone();
    guard.add(model.on_did_focus_session().listen(move |_| session_counter.set(session_counter.get() + 1)));

    let mut states = vec![stack_depth(&context_key_service)];
    thread.update_stopped_details(|details| details.total_frames = Some(1));
    model.update_focused_thread_has_multiple_stack_frames();
    states.push(stack_depth(&context_key_service));
    thread.update_stopped_details(|details| details.total_frames = Some(2));
    model.update_focused_thread_has_multiple_stack_frames();
    states.push(stack_depth(&context_key_service));

    assert_eq!(states, vec![true, false, true]);
    assert_eq!(focus_event_count.get(), 0);
    assert!(Rc::ptr_eq(&model.focused_thread().unwrap(), &thread));
    model.dispose();
}

#[test]
fn focused_thread_stack_depth_refreshes_when_an_unknown_stack_becomes_complete() {
    let (context_key_service, model) = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let session = MockSession::new();
    let thread = Thread::new(session.clone(), "myThread", 1);
    let source = Rc::new(Source::new(
        Some(ProtocolSource { name: Some("app.js".to_owned()), source_reference: Some(1), ..ProtocolSource::default() }),
        &session.get_id(),
        &*mock_uri_identity_service(),
        &NullLogService,
    ));
    thread.set_call_stack(vec![StackFrame::new(thread.clone(), 1, source, "app.js", Some("normal"), one_by_one(), 0, true, None)]);
    model.set_focus(None, Some(thread.clone()), Some(session_dyn(&session)), false);

    let mut states = vec![stack_depth(&context_key_service)];
    thread.reached_end_of_call_stack.set(true);
    model.update_focused_thread_has_multiple_stack_frames();
    states.push(stack_depth(&context_key_service));

    assert_eq!(states, vec![true, false]);
    model.dispose();
}

#[test]
fn focused_thread_stack_depth_follows_focus_changes_and_resets_when_focus_clears() {
    let (context_key_service, model) = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let session = MockSession::new();
    let first_thread = Thread::new(session.clone(), "firstThread", 1);
    first_thread.set_stopped_details(Some(IRawStoppedDetails { total_frames: Some(1), ..IRawStoppedDetails::default() }));
    let second_thread = Thread::new(session.clone(), "secondThread", 2);
    second_thread.set_stopped_details(Some(IRawStoppedDetails { total_frames: Some(2), ..IRawStoppedDetails::default() }));

    let mut states = vec![stack_depth(&context_key_service)];
    model.set_focus(None, Some(first_thread.clone()), Some(session_dyn(&session)), false);
    states.push(stack_depth(&context_key_service));
    model.set_focus(None, Some(second_thread.clone()), Some(session_dyn(&session)), false);
    states.push(stack_depth(&context_key_service));
    first_thread.update_stopped_details(|details| details.total_frames = Some(2));
    second_thread.update_stopped_details(|details| details.total_frames = Some(1));
    model.update_focused_thread_has_multiple_stack_frames();
    states.push(stack_depth(&context_key_service));
    model.set_focus(None, None, None, false);
    states.push(stack_depth(&context_key_service));
    second_thread.update_stopped_details(|details| details.total_frames = Some(2));
    model.update_focused_thread_has_multiple_stack_frames();
    states.push(stack_depth(&context_key_service));

    assert_eq!(states, vec![false, false, true, false, false, false]);
    model.dispose();
}

#[test]
fn multi_session_view_and_changed_workbench_state() {
    let (_context_key_service, model) = setup();
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    assert!(!model.is_multi_session_view());
    model.set_multi_session_view(true);
    assert!(model.is_multi_session_view());
    model.dispose();
}
