//! Porte de `src/vs/workbench/services/userActivity/test/common/userActivityService.test.ts` (suíte
//! `UserActivityService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `sinon.useFakeTimers()` e `clock.tick(ms)` viram `runtime::with_faked_timers` e `runtime::advance_fake_time`;
//!   o `setup` e o `teardown` viram o corpo e o fim de `run`. `ensureNoDisposablesAreLeakedInTestSuite()` vira o
//!   `LeakGuard`, que confere os vazamentos ao sair do corpo.
//! - `new TestInstantiationService()` é o `InstantiationService` sobre uma coleção vazia (o teste não registra
//!   nenhum serviço, o `userActivityRegistry` está vazio).
//! - Os casos `async` do original não esperam nada, então saem síncronos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::runtime;
use je_instantiation::{Injectable, InstantiationService, ServiceCollection};
use je_workbench::services::user_activity::user_activity_service::{IUserActivityService, MarkActiveOptions, UserActivityService};

const MARK_INACTIVE_DEBOUNCE: u64 = 10_000;

fn tick(ms: u64) {
    runtime::advance_fake_time(ms);
}

/// `setup`, o corpo do teste e `teardown`.
fn run(body: impl FnOnce(&dyn IUserActivityService)) {
    runtime::with_faked_timers(|| {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let instantiation = ds.add_rc(InstantiationService::new(ServiceCollection::new()));
        let user_activity_service = ds.add_rc(UserActivityService::create((), (instantiation,)));
        body(user_activity_service.as_ref());
    });
}

#[test]
fn is_active_should_be_true_initially() {
    run(|user_activity_service| {
        assert!(user_activity_service.is_active());
    });
}

#[test]
fn mark_active_should_be_inactive_when_all_handles_gone() {
    run(|user_activity_service| {
        let h1 = user_activity_service.mark_active(None);
        let h2 = user_activity_service.mark_active(None);
        assert!(user_activity_service.is_active());
        h1.dispose();
        assert!(user_activity_service.is_active());
        h2.dispose();
        tick(MARK_INACTIVE_DEBOUNCE);
        assert!(!user_activity_service.is_active());
    });
}

#[test]
fn mark_active_sets_active_when_held_for() {
    run(|user_activity_service| {
        user_activity_service.mark_active(None).dispose();
        tick(MARK_INACTIVE_DEBOUNCE);

        let duration = 100; // milliseconds
        let opts = MarkActiveOptions { when_held_for: Some(duration), ..Default::default() };
        let handle = user_activity_service.mark_active(Some(opts));
        assert!(!user_activity_service.is_active());
        tick(duration - 1);
        assert!(!user_activity_service.is_active());
        tick(1);
        assert!(user_activity_service.is_active());
        handle.dispose();

        tick(MARK_INACTIVE_DEBOUNCE);
        assert!(!user_activity_service.is_active());
    });
}

#[test]
fn mark_active_when_held_for_before_triggers() {
    run(|user_activity_service| {
        user_activity_service.mark_active(None).dispose();
        tick(MARK_INACTIVE_DEBOUNCE);

        let duration = 100; // milliseconds
        let opts = MarkActiveOptions { when_held_for: Some(duration), ..Default::default() };
        user_activity_service.mark_active(Some(opts)).dispose();
        assert!(!user_activity_service.is_active());
        tick(duration + MARK_INACTIVE_DEBOUNCE);
        assert!(!user_activity_service.is_active());
    });
}

#[test]
fn mark_active_with_extend_only_only_extends_if_already_active() {
    run(|user_activity_service| {
        // Make user inactive
        user_activity_service.mark_active(None).dispose();
        tick(MARK_INACTIVE_DEBOUNCE);
        assert!(!user_activity_service.is_active());

        // Should not activate if inactive and extendOnly is true
        let handle = user_activity_service.mark_active(Some(MarkActiveOptions { extend_only: true, ..Default::default() }));
        assert!(!user_activity_service.is_active());
        handle.dispose();

        // Activate normally
        let h1 = user_activity_service.mark_active(None);
        assert!(user_activity_service.is_active());

        // Should extend activity if already active
        let h2 = user_activity_service.mark_active(Some(MarkActiveOptions { extend_only: true, ..Default::default() }));
        h1.dispose();
        // Still active because h2 is holding
        assert!(user_activity_service.is_active());
        h2.dispose();
        tick(MARK_INACTIVE_DEBOUNCE);
        assert!(!user_activity_service.is_active());
    });
}
