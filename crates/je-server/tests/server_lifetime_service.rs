//! Porte de `src/vs/server/test/node/serverLifetimeService.test.ts` (suíte `ServerLifetimeService`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o `LeakGuard`, criado no começo de cada caso; o `ds` do original é
//!   o store do guarda.
//! - **`() => undefined as never`** (o `exit` do original) é o fecho sem efeito; **`() => { exits++; ... }`** conta as saídas.
//! - **`await timeout(0)`** é `common::timeout(0).await`, que deixa as microtarefas e os temporizadores zerados rodarem.
//! - **`ds.add(service.active('second'))`** é o `add` do guarda sobre o descartável devolvido.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::lifecycle::IDisposable;
use je_base::promise::DeferredPromise;
use je_platform::log::NullLogService;
use je_server::server_lifetime_service::{IServerLifetimeService, ServerLifetimeOptions, ServerLifetimeService};

fn create_with(ds: &LeakGuard, options: ServerLifetimeOptions, exit: Option<Rc<dyn Fn(i32)>>) -> Rc<ServerLifetimeService> {
    ds.add_rc(ServerLifetimeService::new(options, exit, NullLogService::shared()))
}

fn create(ds: &LeakGuard) -> Rc<ServerLifetimeService> {
    create_with(ds, ServerLifetimeOptions::default(), Some(Rc::new(|_| {})))
}

#[test]
fn starts_with_no_active_consumers() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let service = create(&ds);
    assert!(!service.has_active_consumers());
}

#[test]
fn active_marks_a_consumer_and_dispose_releases_it() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let service = create(&ds);
    let d = service.active("test");
    assert!(service.has_active_consumers());
    d.dispose();
    assert!(!service.has_active_consumers());
}

#[test]
fn multiple_active_consumers_require_all_to_dispose() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let service = create(&ds);
    let d1 = service.active("a");
    let d2 = service.active("b");
    assert!(service.has_active_consumers());
    d1.dispose();
    assert!(service.has_active_consumers());
    d2.dispose();
    assert!(!service.has_active_consumers());
}

#[test]
fn same_consumer_name_counted_multiple_times() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let service = create(&ds);
    let d1 = service.active("ext");
    let d2 = service.active("ext");
    assert!(service.has_active_consumers());
    d1.dispose();
    assert!(service.has_active_consumers());
    d2.dispose();
    assert!(!service.has_active_consumers());
}

#[test]
fn dispose_is_idempotent() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let service = create(&ds);
    let d1 = service.active("a");
    let d2 = service.active("a");
    d1.dispose();
    d1.dispose();
    assert!(service.has_active_consumers());
    d2.dispose();
    assert!(!service.has_active_consumers());
}

#[test]
fn does_not_exit_when_a_consumer_becomes_active_during_shutdown() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let exits = Rc::new(Cell::new(0));
        let aborts = Rc::new(Cell::new(0));
        let exits_in = exits.clone();
        let service = create_with(
            &ds,
            ServerLifetimeOptions { enable_auto_shutdown: true, shutdown_without_delay: true },
            Some(Rc::new(move |_| exits_in.set(exits_in.get() + 1))),
        );
        let shutdown_barrier = DeferredPromise::<()>::new();
        let barrier = shutdown_barrier.promise.clone();
        ds.add(service.on_will_shutdown().listen(move |event| event.join(barrier.clone())));
        let aborts_in = aborts.clone();
        ds.add(service.on_did_abort_shutdown().listen(move |_| aborts_in.set(aborts_in.get() + 1)));

        let first_consumer = service.active("first");
        first_consumer.dispose();
        let second_consumer = ds.add(service.active("second"));
        drop(shutdown_barrier.complete(()));
        timeout(0).await.unwrap();

        assert_eq!((service.has_active_consumers(), exits.get(), aborts.get()), (true, 0, 1));
        second_consumer.dispose();
        timeout(0).await.unwrap();
        assert_eq!(exits.get(), 1);
    });
}

#[test]
fn does_not_exit_when_shutdown_is_delayed_during_a_join() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let exits = Rc::new(Cell::new(0));
        let exits_in = exits.clone();
        let service = create_with(
            &ds,
            ServerLifetimeOptions { enable_auto_shutdown: true, shutdown_without_delay: true },
            Some(Rc::new(move |_| exits_in.set(exits_in.get() + 1))),
        );
        let shutdown_barrier = DeferredPromise::<()>::new();
        let shutdown_count = Rc::new(Cell::new(0));
        let (barrier, count) = (shutdown_barrier.promise.clone(), shutdown_count.clone());
        ds.add(service.on_will_shutdown().listen(move |event| {
            count.set(count.get() + 1);
            if count.get() == 1 {
                event.join(barrier.clone());
            }
        }));

        let consumer = service.active("first");
        consumer.dispose();
        service.delay();
        drop(shutdown_barrier.complete(()));
        timeout(0).await.unwrap();

        assert_eq!((shutdown_count.get(), exits.get()), (2, 1));
    });
}
