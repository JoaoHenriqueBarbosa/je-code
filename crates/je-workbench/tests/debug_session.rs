//! Porte de `src/vs/workbench/contrib/debug/test/browser/debugSession.test.ts` (suítes `DebugSession - ThreadStatusScheduler`
//! e `DebugSession - continued events`), caso a caso e na mesma ordem.
//!
//! São 13 casos (7 do `ThreadStatusScheduler`, 6 dos eventos `continued`, estes com o laço `for (const allThreadsContinued
//! of [false, true])` desdobrado em dois casos cada); nenhum ficou de fora.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` de cada caso; `ds.add(x)` é o `store` do guarda.
//! - `await scheduler.run(Promise.resolve([1]), async (threadId, token) => {...})` é `scheduler.run(...)` com a operação
//!   como `Rc<dyn Fn(i64, CancellationToken) -> Promise<()>>`; os `assert` dentro da operação reprovam o caso se falharem.
//! - `Promise.all([...])` é o `Promise::all` do je-base.
//! - `class extends MockDebugAdapter { override sendMessage }` é [`MockDebugAdapter::set_send_message_override`].
//! - `new RawDebugSession(adapter, undefined!, '', '', undefined!, ...)` é `RawDebugSession::new(adapter, None, "", "", None)`.
//! - `Event.toPromise(Event.filter(...))` é `Event::to_promise` sobre o `filter` do evento (a promessa resolve com o valor).
//! - O `teardown(async () => { await session?.disconnect(); })` roda no fim de cada caso `continued`, antes do guarda.
//! - `assert.deepStrictEqual(session.getStoppedDetails(), { reason, threadId, totalFrames })` compara o
//!   `IRawStoppedDetails` com os três campos preenchidos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;
use je_workbench::contrib::debug::debug_session::{DebugSession, ThreadStatusScheduler};
use je_workbench::contrib::debug::debug_types::{IDebugSessionFull, IRawStoppedDetails};
use je_workbench::contrib::debug::debug_protocol::ProtocolMessage;
use je_workbench::contrib::debug::raw_debug_session::RawDebugSession;
use serde_json::json;
use support::mock_debug::{MockDebugAdapter, create_mock_debug_model, create_test_session};

type Operation = Rc<dyn Fn(i64, CancellationToken) -> Promise<()>>;

fn operation(body: impl Fn(i64, CancellationToken) -> Promise<()> + 'static) -> Operation {
    Rc::new(body)
}

// DebugSession - ThreadStatusScheduler

#[test]
fn cancel_base_case() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let scheduler = guard.add_rc(ThreadStatusScheduler::new());
        let inner_scheduler = scheduler.clone();

        scheduler
            .run(
                Promise::resolved(vec![1]),
                operation(move |thread_id, token| {
                    assert_eq!(thread_id, 1);
                    assert!(!token.is_cancellation_requested());
                    inner_scheduler.cancel(Some(&[1]));
                    assert!(token.is_cancellation_requested());
                    Promise::resolved(())
                }),
            )
            .await
            .unwrap();
    });
}

#[test]
fn cancel_global() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let scheduler = guard.add_rc(ThreadStatusScheduler::new());
        let inner_scheduler = scheduler.clone();

        scheduler
            .run(
                Promise::resolved(vec![1]),
                operation(move |thread_id, token| {
                    assert_eq!(thread_id, 1);
                    assert!(!token.is_cancellation_requested());
                    inner_scheduler.cancel(None);
                    assert!(token.is_cancellation_requested());
                    Promise::resolved(())
                }),
            )
            .await
            .unwrap();
    });
}

#[test]
fn cancels_when_new_work_comes_in() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let scheduler = guard.add_rc(ThreadStatusScheduler::new());
        let inner_called = Rc::new(Cell::new(false));
        let outer_scheduler = scheduler.clone();
        let outer_called = inner_called.clone();

        scheduler
            .run(
                Promise::resolved(vec![1]),
                operation(move |thread_id, token1| {
                    assert_eq!(thread_id, 1);
                    assert!(!token1.is_cancellation_requested());
                    let scheduler = outer_scheduler.clone();
                    let inner_called = outer_called.clone();
                    async_fn(async move {
                        scheduler
                            .run(
                                Promise::resolved(vec![1]),
                                operation(move |_thread_id, token2| {
                                    inner_called.set(true);
                                    assert!(token1.is_cancellation_requested());
                                    assert!(!token2.is_cancellation_requested());
                                    Promise::resolved(())
                                }),
                            )
                            .await
                    })
                }),
            )
            .await
            .unwrap();

        assert!(inner_called.get());
    });
}

#[test]
fn cancels_slower_lookups_when_new_lookup_is_made() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let scheduler = guard.add_rc(ThreadStatusScheduler::new());
        let inner_called1: Rc<RefCell<Vec<i64>>> = Rc::default();
        let inner_called2: Rc<RefCell<Vec<i64>>> = Rc::default();
        let recorder1 = inner_called1.clone();
        let recorder2 = inner_called2.clone();

        Promise::all(vec![
            scheduler.run(
                Promise::resolved(()).then(|()| ()).then(|()| vec![1, 3]),
                operation(move |thread_id, _token| {
                    recorder1.borrow_mut().push(thread_id);
                    Promise::resolved(())
                }),
            ),
            scheduler.run(
                Promise::resolved(vec![1, 2]),
                operation(move |thread_id, _token| {
                    recorder2.borrow_mut().push(thread_id);
                    Promise::resolved(())
                }),
            ),
        ])
        .await
        .unwrap();

        assert_eq!(*inner_called1.borrow(), vec![3]);
        assert_eq!(*inner_called2.borrow(), vec![1, 2]);
    });
}

#[test]
fn allows_work_with_other_ids() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let scheduler = guard.add_rc(ThreadStatusScheduler::new());
        let inner_called = Rc::new(Cell::new(false));
        let outer_scheduler = scheduler.clone();
        let outer_called = inner_called.clone();

        scheduler
            .run(
                Promise::resolved(vec![1]),
                operation(move |thread_id, token1| {
                    assert_eq!(thread_id, 1);
                    assert!(!token1.is_cancellation_requested());
                    let scheduler = outer_scheduler.clone();
                    let inner_called = outer_called.clone();
                    async_fn(async move {
                        scheduler
                            .run(
                                Promise::resolved(vec![2]),
                                operation(move |_thread_id, token2| {
                                    inner_called.set(true);
                                    assert!(!token1.is_cancellation_requested());
                                    assert!(!token2.is_cancellation_requested());
                                    Promise::resolved(())
                                }),
                            )
                            .await
                    })
                }),
            )
            .await
            .unwrap();

        assert!(inner_called.get());
    });
}

#[test]
fn cancels_when_called_during_resolution() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let scheduler = guard.add_rc(ThreadStatusScheduler::new());
        let inner_called = Rc::new(Cell::new(false));
        let recorder = inner_called.clone();
        let canceller = scheduler.clone();

        scheduler
            .run(
                Promise::resolved(()).then(move |()| canceller.cancel(Some(&[1]))).then(|()| vec![1]),
                operation(move |_thread_id, _token| {
                    recorder.set(true);
                    Promise::resolved(())
                }),
            )
            .await
            .unwrap();

        assert!(!inner_called.get());
    });
}

#[test]
fn global_cancels_when_called_during_resolution() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let scheduler = guard.add_rc(ThreadStatusScheduler::new());
        let inner_called = Rc::new(Cell::new(false));
        let recorder = inner_called.clone();
        let canceller = scheduler.clone();

        scheduler
            .run(
                Promise::resolved(()).then(move |()| canceller.cancel(None)).then(|()| vec![1]),
                operation(move |_thread_id, _token| {
                    recorder.set(true);
                    Promise::resolved(())
                }),
            )
            .await
            .unwrap();

        assert!(!inner_called.get());
    });
}

// DebugSession - continued events

/// `createSession()`.
fn create_session(guard: &common::LeakGuard) -> (Rc<DebugSession>, Rc<MockDebugAdapter>) {
    let model = create_mock_debug_model(&guard.store());
    let session = guard.add_rc(create_test_session(&model, "mockSession", None, &guard.store()));
    model.add_session(session.clone());
    let adapter = guard.add_rc(MockDebugAdapter::new());
    adapter.set_send_message_override(|adapter, message| {
        if let ProtocolMessage::Request(request) = message {
            if request.command == "threads" {
                adapter.send_response_body(request, json!({ "threads": [{ "id": 1, "name": "first" }, { "id": 2, "name": "second" }] }));
                return true;
            }
            if request.command == "stackTrace" {
                adapter.send_response_body(request, json!({ "stackFrames": [], "totalFrames": 0 }));
                return true;
            }
        }
        false
    });
    let raw = guard.add_rc(RawDebugSession::new(adapter.clone(), None, "", "", None));
    session.initialize_for_test(raw);
    (session, adapter)
}

fn details(reason: &str, thread_id: i64, total_frames: i64) -> IRawStoppedDetails {
    IRawStoppedDetails { reason: Some(reason.to_owned()), thread_id: Some(thread_id), total_frames: Some(total_frames), ..IRawStoppedDetails::default() }
}

fn stopped_details(session: &Rc<DebugSession>) -> Option<IRawStoppedDetails> {
    session.get_stopped_details().map(|details| details.borrow().clone())
}

fn thread_stopped(session: &Rc<DebugSession>, thread_id: i64) -> Option<bool> {
    session.get_thread(thread_id).map(|thread| thread.stopped.get())
}

fn run_session_test(body: impl FnOnce(Rc<DebugSession>, Rc<MockDebugAdapter>) -> Promise<()> + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let (session, adapter) = create_session(&guard);
        body(session.clone(), adapter).await.unwrap();
        // teardown(async () => { await session?.disconnect(); })
        session.disconnect(false, false).await.unwrap();
    });
}

fn stopped_again_event(session: &Rc<DebugSession>) -> Promise<()> {
    let watched = session.clone();
    session.on_did_change_state().filter(move |_| watched.get_thread(1).is_some_and(|thread| thread.stopped.get()), None).to_promise(None).promise.then(|_| ())
}

fn state_changed(session: &Rc<DebugSession>) -> Promise<()> {
    session.on_did_change_state().to_promise(None).promise.then(|_| ())
}

#[test]
fn preserves_a_newer_stop_during_single_thread_continued_cleanup() {
    run_session_test(|session, adapter| {
        async_fn(async move {
            let initially_stopped = state_changed(&session);
            adapter.send_event_body("stopped", json!({ "reason": "step", "threadId": 1 }));
            initially_stopped.await?;
            assert_eq!(session.get_stopped_details().map(|d| d.borrow().reason.clone()), Some(Some("step".to_owned())));

            let stopped_again = stopped_again_event(&session);
            // Adjacent adapter events are dispatched before continued cleanup resumes.
            adapter.send_event_body("continued", json!({ "threadId": 1, "allThreadsContinued": false }));
            adapter.send_event_body("stopped", json!({ "reason": "breakpoint", "threadId": 1 }));
            stopped_again.await?;

            assert_eq!(stopped_details(&session), Some(details("breakpoint", 1, 0)));
            Ok(())
        })
    });
}

#[test]
fn clears_an_earlier_stop_on_single_thread_continuation() {
    run_session_test(|session, adapter| {
        async_fn(async move {
            let stopped = state_changed(&session);
            adapter.send_event_body("stopped", json!({ "reason": "step", "threadId": 1 }));
            stopped.await?;

            let continued = state_changed(&session);
            adapter.send_event_body("continued", json!({ "threadId": 1, "allThreadsContinued": false }));
            continued.await?;

            assert_eq!(stopped_details(&session), None);
            assert_eq!(thread_stopped(&session, 1), Some(false));
            Ok(())
        })
    });
}

#[test]
fn preserves_a_newer_stop_during_all_thread_continued_cleanup() {
    run_session_test(|session, adapter| {
        async_fn(async move {
            let initially_stopped = state_changed(&session);
            adapter.send_event_body("stopped", json!({ "reason": "step", "threadId": 1 }));
            initially_stopped.await?;
            assert_eq!(session.get_stopped_details().map(|d| d.borrow().reason.clone()), Some(Some("step".to_owned())));

            let stopped_again = stopped_again_event(&session);
            // Adjacent adapter events are dispatched before continued cleanup resumes.
            adapter.send_event_body("continued", json!({ "threadId": 1, "allThreadsContinued": true }));
            adapter.send_event_body("stopped", json!({ "reason": "breakpoint", "threadId": 1 }));
            stopped_again.await?;

            assert_eq!(stopped_details(&session), Some(details("breakpoint", 1, 0)));
            Ok(())
        })
    });
}

#[test]
fn clears_an_earlier_stop_on_all_thread_continuation() {
    run_session_test(|session, adapter| {
        async_fn(async move {
            let stopped = state_changed(&session);
            adapter.send_event_body("stopped", json!({ "reason": "step", "threadId": 1 }));
            stopped.await?;

            let continued = state_changed(&session);
            adapter.send_event_body("continued", json!({ "threadId": 1, "allThreadsContinued": true }));
            continued.await?;

            assert_eq!(stopped_details(&session), None);
            assert_eq!(thread_stopped(&session, 1), Some(false));
            Ok(())
        })
    });
}

#[test]
fn preserves_a_stopped_thread_when_another_thread_continues() {
    run_session_test(|session, adapter| {
        async_fn(async move {
            let stopped = state_changed(&session);
            adapter.send_event_body("stopped", json!({ "reason": "breakpoint", "threadId": 2 }));
            stopped.await?;

            let continued = state_changed(&session);
            adapter.send_event_body("continued", json!({ "threadId": 1, "allThreadsContinued": false }));
            continued.await?;

            assert_eq!(stopped_details(&session), Some(details("breakpoint", 2, 0)));
            assert_eq!(thread_stopped(&session, 2), Some(true));
            Ok(())
        })
    });
}

#[test]
fn clears_a_stop_without_a_thread_id_when_all_threads_continue() {
    run_session_test(|session, adapter| {
        async_fn(async move {
            let stopped = state_changed(&session);
            adapter.send_event_body("stopped", json!({ "reason": "pause", "allThreadsStopped": true }));
            stopped.await?;

            let watched = session.clone();
            let continued = session
                .on_did_change_state()
                .filter(move |_| watched.get_all_threads().iter().all(|thread| !thread.stopped.get()), None)
                .to_promise(None)
                .promise
                .then(|_| ());
            adapter.send_event_body("continued", json!({ "threadId": 1 }));
            continued.await?;

            assert_eq!(stopped_details(&session), None);
            Ok(())
        })
    });
}
