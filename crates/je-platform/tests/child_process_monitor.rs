//! Porte de `src/vs/platform/terminal/test/node/childProcessMonitor.test.ts`.
//!
//! Suíte `ChildProcessMonitor`, caso a caso e na mesma ordem.
//!
//! Divergências: o `sinon.useFakeTimers()` é o relógio virtual de [`runtime::with_faked_timers`], e o
//! `clock.countTimers()` é [`runtime::pending_timer_count`] (os timers pendentes do laço de eventos). O
//! `clock.tick(7000)` é [`runtime::advance_fake_time`]. O `NullLogService` é o compartilhado, e o monitor é
//! registrado no `LeakGuard` como o `store.add(new ChildProcessMonitor(...))` do original. O `finally` com
//! `monitor.dispose()` e `clock.restore()` é o descarte depois do corpo (o `restore` é o fim de `with_faked_timers`).

mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::lifecycle::IDisposable;
use je_base::runtime;
use je_platform::log::NullLogService;
use je_platform::terminal::child_process_monitor::ChildProcessMonitor;

/// `{ beforeDispose, afterDispose }`.
#[derive(Debug, PartialEq, Eq)]
struct TimerCounts {
    before_dispose: usize,
    after_dispose: usize,
}

fn with_monitor(run: impl FnOnce(&Rc<ChildProcessMonitor>)) {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(|| {
        let log_service = NullLogService::shared();
        let monitor = store.add_rc(ChildProcessMonitor::new(1, log_service));
        run(&monitor);
        monitor.dispose();
    });
}

#[test]
fn cancels_the_pending_input_refresh_on_disposal() {
    with_monitor(|monitor| {
        monitor.handle_input();
        let before_dispose = runtime::pending_timer_count();

        monitor.dispose();

        assert_eq!(TimerCounts { before_dispose, after_dispose: runtime::pending_timer_count() }, TimerCounts { before_dispose: 1, after_dispose: 0 });
    });
}

#[test]
fn cancels_both_pending_output_refreshes_on_disposal() {
    with_monitor(|monitor| {
        monitor.handle_output();
        monitor.handle_output();
        let before_dispose = runtime::pending_timer_count();

        monitor.dispose();

        assert_eq!(TimerCounts { before_dispose, after_dispose: runtime::pending_timer_count() }, TimerCounts { before_dispose: 2, after_dispose: 0 });
    });
}

#[test]
fn does_not_schedule_refreshes_after_disposal() {
    with_monitor(|monitor| {
        monitor.dispose();

        monitor.handle_input();
        monitor.handle_output();
        monitor.handle_output();

        assert_eq!(runtime::pending_timer_count(), 0);
    });
}

#[test]
fn coalesces_repeated_input_and_output_until_disposal() {
    with_monitor(|monitor| {
        for _ in 0..17 {
            monitor.handle_input();
            monitor.handle_output();
        }
        let before_dispose = runtime::pending_timer_count();

        monitor.dispose();
        runtime::advance_fake_time(7000);

        assert_eq!(TimerCounts { before_dispose, after_dispose: runtime::pending_timer_count() }, TimerCounts { before_dispose: 2, after_dispose: 0 });
    });
}
