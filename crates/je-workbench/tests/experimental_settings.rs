//! Porte de `src/vs/workbench/services/configuration/test/common/experimentalSettings.test.ts` (suíte `ExperimentalSettingsService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o `LeakGuard` criado no início de cada corpo e descartado no fim; o
//!   `store.add(...)` do original é `guard.add_rc(...)` (o serviço) e `guard.add(...)` (o ouvinte).
//! - **`await Promise.resolve()` e `await timeout(0)`.** São a microtarefa do runtime e o timer virtual de 0 ms
//!   (`run_with_faked_timers`).
//! - **`[...keys]`.** O conjunto do evento é um `HashSet`, sem ordem de iteração; o porte ordena as chaves para comparar com a
//!   lista esperada (`['test.first', 'test.second']`).
//! - **`assert.deepStrictEqual({ ... }, { ... })`.** É `assert_eq!` entre tuplas com os mesmos campos, na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_workbench::services::configuration::common::experimental_settings::{ExperimentalSettingsService, IExperimentalSettingsService};

/// `const changes: string[][] = []` e o ouvinte `keys => changes.push([...keys])`.
fn track_changes(service: &ExperimentalSettingsService, guard: &common::LeakGuard) -> Rc<RefCell<Vec<Vec<String>>>> {
    let changes: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let sink = changes.clone();
    guard.add(service.on_did_change_assignments().listen(move |keys| {
        let mut sorted: Vec<String> = keys.iter().cloned().collect();
        sorted.sort();
        sink.borrow_mut().push(sorted);
    }));
    changes
}

#[test]
fn batches_changed_settings_across_microtasks_while_updating_membership_synchronously() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let service = ExperimentalSettingsService::new();
        guard.add_rc(service.clone());
        let changes = track_changes(&service, &guard);

        service.set_assignment("test.first", false);
        service.set_assignment("test.first", true);
        service.set_assignment("test.first", true);
        Promise::resolved(()).await.unwrap();
        service.set_assignment("test.second", true);
        service.set_assignment("test.first", false);
        let before_notification = changes.borrow().len();
        let second_assigned_immediately = service.has_assignment("test.second");
        timeout(0).await.unwrap();

        let strings = |items: &[&str]| items.iter().map(|item| (*item).to_owned()).collect::<Vec<String>>();
        assert_eq!(
            (
                changes.borrow().clone(),
                before_notification,
                second_assigned_immediately,
                service.has_assignment("test.first"),
                service.has_assignment("test.second"),
                service.has_assignment("test.missing"),
            ),
            (vec![strings(&["test.first", "test.second"])], 0, true, false, true, false)
        );
        drop(guard);
    });
}

#[test]
fn does_not_notify_for_unchanged_membership_or_after_disposal() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let service = ExperimentalSettingsService::new();
        guard.add_rc(service.clone());
        let changes = track_changes(&service, &guard);
        service.set_assignment("test.setting", false);
        timeout(0).await.unwrap();
        service.set_assignment("test.setting", true);
        service.dispose();
        timeout(0).await.unwrap();
        assert_eq!(*changes.borrow(), Vec::<Vec<String>>::new());
        drop(guard);
    });
}
