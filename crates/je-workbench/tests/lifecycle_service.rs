//! Porte de `src/vs/workbench/services/lifecycle/test/electron-browser/lifecycleService.test.ts` (suíte `Lifecycleservice`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `workbenchInstantiationService(undefined, disposables)` e `createInstance(TestLifecycleService)` viram a criação direta do
//!   [`NativeLifecycleService`] com o `TestStorageService`, o `NullLogService` e um [`NoopNativeHost`] (o IPC do processo nativo não é
//!   exercido pelos casos). `disposables` e `ensureNoDisposablesAreLeakedInTestSuite()` são o guarda do começo de cada caso.
//! - `TestLifecycleService.testHandleBeforeShutdown` e `testHandleWillShutdown` são os métodos `handle_before_shutdown` e
//!   `handle_will_shutdown`, que são `pub`.
//! - `e.finalVeto(...)` sobre o `BeforeShutdownEvent` do `onBeforeShutdown` é `InternalBeforeShutdownEvent::from_event(e.clone())`.
//! - `new Promise<boolean>(resolve => { ...; resolve(x) })` é `Promise::with_executor`, que roda o executor na hora, como o original.
//! - `e.join(async () => {...}, { order: Last })` é `join_last` com uma função que devolve a promessa de uma função `async`
//!   (`async_fn`, que roda o corpo já até o primeiro `await`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, LeakGuard};
use je_base::async_::{async_fn, timeout};
use je_base::errors::Error;
use je_base::promise::{MaybePromise, Promise};
use je_platform::log::NullLogService;
use je_platform::storage::storage::StorageService;
use je_workbench::services::lifecycle::lifecycle::{
    ILifecycleService, InternalBeforeShutdownEvent, ShutdownReason, WillShutdownEventJoiner, WillShutdownJoinerOrder,
};
use je_workbench::services::lifecycle::native_lifecycle_service::{BeforeUnloadRequest, INativeLifecycleHost, NativeLifecycleService, WillUnloadRequest};
use je_workbench::test::common::workbench_test_services::TestStorageService;

/// O `NativeHostService` que o `workbenchInstantiationService` fornece: nenhum caso troca mensagens com o processo nativo.
struct NoopNativeHost;

impl INativeLifecycleHost for NoopNativeHost {
    fn window_id(&self) -> u32 {
        1
    }

    fn close_window(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn on_before_unload(&self, _listener: Rc<dyn Fn(BeforeUnloadRequest)>) {}

    fn on_will_unload(&self, _listener: Rc<dyn Fn(WillUnloadRequest)>) {}

    fn send(&self, _channel: &str, _window_id: u32) {}
}

/// `setup`.
fn setup(guard: &LeakGuard) -> Rc<NativeLifecycleService> {
    let storage: Rc<dyn StorageService> = Rc::new(guard.add_rc(TestStorageService::new()));
    guard.add_rc(NativeLifecycleService::new(Rc::new(NoopNativeHost), storage, NullLogService::shared()))
}

fn joiner() -> WillShutdownEventJoiner {
    WillShutdownEventJoiner { id: "test".to_owned(), label: "test".to_owned(), order: None }
}

#[test]
fn on_before_shutdown_final_veto_called_after_other_vetos() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lifecycle_service = setup(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        let veto_called = Rc::new(Cell::new(false));
        let final_veto_called = Rc::new(Cell::new(false));

        let order: Rc<RefCell<Vec<i32>>> = Rc::default();

        let (called, recorded) = (veto_called.clone(), order.clone());
        store.add(lifecycle_service.on_before_shutdown().listen(move |e| {
            let (called, recorded) = (called.clone(), recorded.clone());
            e.veto(
                MaybePromise::Promise(Promise::with_executor(move |resolve| {
                    called.set(true);
                    recorded.borrow_mut().push(1);

                    resolve.resolve(false);
                })),
                "test",
            );
        }));

        let (called, recorded) = (final_veto_called.clone(), order.clone());
        store.add(lifecycle_service.on_before_shutdown().listen(move |e| {
            let (called, recorded) = (called.clone(), recorded.clone());
            InternalBeforeShutdownEvent::from_event(e.clone()).final_veto(
                Rc::new(move || {
                    let (called, recorded) = (called.clone(), recorded.clone());
                    MaybePromise::Promise(Promise::with_executor(move |resolve| {
                        called.set(true);
                        recorded.borrow_mut().push(2);

                        resolve.resolve(true);
                    }))
                }),
                "test",
            );
        }));

        let veto = lifecycle_service.handle_before_shutdown(ShutdownReason::Quit).await.expect("veto");

        assert!(veto);
        assert!(veto_called.get());
        assert!(final_veto_called.get());
        assert_eq!(order.borrow()[0], 1);
        assert_eq!(order.borrow()[1], 2);
    });
}

#[test]
fn on_before_shutdown_final_veto_not_called_when_veto_happened_before() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lifecycle_service = setup(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        let veto_called = Rc::new(Cell::new(false));
        let final_veto_called = Rc::new(Cell::new(false));

        let called = veto_called.clone();
        store.add(lifecycle_service.on_before_shutdown().listen(move |e| {
            let called = called.clone();
            e.veto(
                MaybePromise::Promise(Promise::with_executor(move |resolve| {
                    called.set(true);

                    resolve.resolve(true);
                })),
                "test",
            );
        }));

        let called = final_veto_called.clone();
        store.add(lifecycle_service.on_before_shutdown().listen(move |e| {
            let called = called.clone();
            InternalBeforeShutdownEvent::from_event(e.clone()).final_veto(
                Rc::new(move || {
                    let called = called.clone();
                    MaybePromise::Promise(Promise::with_executor(move |resolve| {
                        called.set(true);

                        resolve.resolve(true);
                    }))
                }),
                "test",
            );
        }));

        let veto = lifecycle_service.handle_before_shutdown(ShutdownReason::Quit).await.expect("veto");

        assert!(veto);
        assert!(veto_called.get());
        assert!(!final_veto_called.get());
    });
}

#[test]
fn on_before_shutdown_veto_with_error_is_treated_as_veto() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lifecycle_service = setup(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        store.add(lifecycle_service.on_before_shutdown().listen(move |e| {
            e.veto(MaybePromise::Promise(Promise::with_executor(move |resolve| resolve.reject(Error::new("Fail")))), "test");
        }));

        let veto = lifecycle_service.handle_before_shutdown(ShutdownReason::Quit).await.expect("veto");

        assert!(veto);
    });
}

#[test]
fn on_before_shutdown_final_veto_with_error_is_treated_as_veto() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lifecycle_service = setup(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        store.add(lifecycle_service.on_before_shutdown().listen(move |e| {
            InternalBeforeShutdownEvent::from_event(e.clone()).final_veto(
                Rc::new(|| MaybePromise::Promise(Promise::with_executor(move |resolve| resolve.reject(Error::new("Fail"))))),
                "test",
            );
        }));

        let veto = lifecycle_service.handle_before_shutdown(ShutdownReason::Quit).await.expect("veto");

        assert!(veto);
    });
}

#[test]
fn on_will_shutdown_join() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lifecycle_service = setup(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        let join_called = Rc::new(Cell::new(false));

        let called = join_called.clone();
        store.add(lifecycle_service.on_will_shutdown().listen(move |e| {
            let called = called.clone();
            e.join(
                Promise::with_executor(move |resolve| {
                    called.set(true);

                    resolve.resolve(());
                }),
                joiner(),
            );
        }));

        lifecycle_service.handle_will_shutdown(ShutdownReason::Quit).await.expect("desligamento");

        assert!(join_called.get());
    });
}

#[test]
fn on_will_shutdown_join_with_error_is_handled() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lifecycle_service = setup(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        let join_called = Rc::new(Cell::new(false));

        let called = join_called.clone();
        store.add(lifecycle_service.on_will_shutdown().listen(move |e| {
            let called = called.clone();
            e.join(
                Promise::with_executor(move |resolve| {
                    called.set(true);

                    resolve.reject(Error::new("Fail"));
                }),
                joiner(),
            );
        }));

        lifecycle_service.handle_will_shutdown(ShutdownReason::Quit).await.expect("desligamento");

        assert!(join_called.get());
    });
}

#[test]
fn on_will_shutdown_join_order() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lifecycle_service = setup(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        let order: Rc<RefCell<Vec<&'static str>>> = Rc::default();

        let recorded = order.clone();
        store.add(lifecycle_service.on_will_shutdown().listen(move |e| {
            let last = recorded.clone();
            e.join_last(
                Rc::new(move || {
                    let order = last.clone();
                    async_fn(async move {
                        order.borrow_mut().push("disconnect start");
                        timeout(1).await?;
                        order.borrow_mut().push("disconnect end");
                        Ok(())
                    })
                }),
                WillShutdownEventJoiner { id: "test".to_owned(), label: "test".to_owned(), order: Some(WillShutdownJoinerOrder::Last) },
            );

            let default = recorded.clone();
            e.join(
                async_fn(async move {
                    default.borrow_mut().push("default start");
                    timeout(1).await?;
                    default.borrow_mut().push("default end");
                    Ok(())
                }),
                WillShutdownEventJoiner { id: "test".to_owned(), label: "test".to_owned(), order: Some(WillShutdownJoinerOrder::Default) },
            );
        }));

        lifecycle_service.handle_will_shutdown(ShutdownReason::Quit).await.expect("desligamento");

        assert_eq!(*order.borrow(), ["default start", "default end", "disconnect start", "disconnect end"]);
    });
}

#[test]
fn will_shutdown_is_set_when_shutting_down() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let lifecycle_service = setup(&guard);
    let store = guard.store();
    run_with_faked_timers(async move {
        let will_shutdown_set = Rc::new(Cell::new(false));

        let (flag, service) = (will_shutdown_set.clone(), Rc::downgrade(&lifecycle_service));
        store.add(lifecycle_service.on_will_shutdown().listen(move |e| {
            let (flag, service) = (flag.clone(), service.clone());
            e.join(
                Promise::with_executor(move |resolve| {
                    if service.upgrade().is_some_and(|service| service.will_shutdown()) {
                        flag.set(true);
                        resolve.resolve(());
                    }
                }),
                joiner(),
            );
        }));

        lifecycle_service.handle_will_shutdown(ShutdownReason::Quit).await.expect("desligamento");

        assert!(will_shutdown_set.get());
    });
}
