//! Porte de `src/vs/workbench/services/userAttention/test/browser/userAttentionService.test.ts` (suíte
//! `UserAttentionService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `sinon.useFakeTimers()` no `setup` e `clock.restore()` no `teardown` são o `runtime::with_faked_timers`
//!   que envolve o caso inteiro ([`run`]), e `clock.tick(ms)` é `runtime::advance_fake_time(ms)`. O guarda de
//!   vazamentos nasce e morre dentro do relógio virtual, para a conferência de vazamento rodar antes de
//!   `with_faked_timers` disparar os timers que sobraram.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`; `store.add(x)` é `add`/`add_rc`.
//! - `TestInstantiationService` é um `InstantiationService` sobre um `ServiceCollection` com o `ILogService`
//!   (`NullLogService`) e o adaptador falso. O `sinon.stub(insta, 'createInstance')` que devolve o
//!   `hostAdapterMock` no lugar do `UserAttentionServiceEnv` é o registro do [`HostAdapterMock`] como
//!   `IUserAttentionServiceEnv` (o serviço recebe o adaptador por dependência, ver as divergências em
//!   `user_attention_browser.rs`).
//! - `observableValue('focused', true)` e `observableValue('active', false)` são `observable_value`, e
//!   `isVsCodeFocused.set(f, undefined)` é `set(f, None)`.
//! - O `callback` de `fireAfterGivenFocusTimePassed` é um `Box<dyn Fn()>` que escreve num `Cell` (a variável
//!   `callbackFired` do original).
//! - Os comentários do original que dizem "5 seconds" (o valor real do timeout é 60 s) são mantidos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::lifecycle::IDisposable;
use je_base::observable::base::{IObservable, Observable, SettableExt};
use je_base::observable::observables::observable_value::{ObservableValue, observable_value};
use je_base::runtime;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::log::{I_LOG_SERVICE, NullLogService};
use je_workbench::services::user_attention::user_attention_browser::{I_USER_ATTENTION_SERVICE_ENV, IUserAttentionServiceEnv, UserAttentionService};
use je_workbench::services::user_attention::user_attention_service::IUserAttentionService;

const ONE_MINUTE: u64 = 50_000;
const ATTENTION_TIMEOUT: u64 = 60_000; // USER_ATTENTION_TIMEOUT_MS is 60 seconds

/// O `hostAdapterMock` do original: os dois observáveis e os setters.
struct HostAdapterMock {
    is_vs_code_focused: Rc<ObservableValue<bool, ()>>,
    is_user_active: Rc<ObservableValue<bool, ()>>,
}

impl HostAdapterMock {
    fn set_focus(&self, focused: bool) {
        self.is_vs_code_focused.set(focused, None);
    }

    fn set_active(&self, active: bool) {
        self.is_user_active.set(active, None);
    }
}

impl IUserAttentionServiceEnv for HostAdapterMock {
    fn is_vs_code_focused(&self) -> IObservable<bool> {
        self.is_vs_code_focused.to_observable()
    }

    fn is_user_active(&self) -> IObservable<bool> {
        self.is_user_active.to_observable()
    }
}

struct Fixture {
    user_attention_service: Rc<UserAttentionService>,
    host_adapter_mock: Rc<HostAdapterMock>,
}

fn setup(store: &LeakGuard) -> Fixture {
    let host_adapter_mock = Rc::new(HostAdapterMock {
        is_vs_code_focused: observable_value::<bool, ()>("focused", true),
        is_user_active: observable_value::<bool, ()>("active", false),
    });
    let host_adapter: Rc<dyn IUserAttentionServiceEnv> = host_adapter_mock.clone();

    let insta = store.add_rc(InstantiationService::new(
        ServiceCollection::new().with(I_LOG_SERVICE, NullLogService::shared()).with(I_USER_ATTENTION_SERVICE_ENV, host_adapter),
    ));
    let user_attention_service = store.add_rc(insta.create_instance::<UserAttentionService>(()));

    // Simulate initial activity
    host_adapter_mock.set_active(true);
    host_adapter_mock.set_active(false);

    Fixture { user_attention_service, host_adapter_mock }
}

/// `setup` e `teardown` do original, com o relógio virtual.
fn run(body: impl FnOnce(&Fixture, &LeakGuard)) {
    runtime::with_faked_timers(|| {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&store);
        body(&fixture, &store);
    });
}

/// Um `callback` que liga o `Cell` (`callbackFired = true`).
fn set_flag(flag: &Rc<Cell<bool>>) -> Box<dyn Fn()> {
    let flag = flag.clone();
    Box::new(move || flag.set(true))
}

#[test]
fn is_vs_code_focused_reflects_window_focus_state() {
    run(|f, _| {
        assert!(f.user_attention_service.is_vs_code_focused().get());

        f.host_adapter_mock.set_focus(false);
        assert!(!f.user_attention_service.is_vs_code_focused().get());

        f.host_adapter_mock.set_focus(true);
        assert!(f.user_attention_service.is_vs_code_focused().get());
    });
}

#[test]
fn has_user_attention_is_true_when_focused_and_has_recent_activity() {
    run(|f, _| {
        // Initially focused with activity
        assert!(f.user_attention_service.has_user_attention().get());
    });
}

#[test]
fn has_user_attention_becomes_false_after_attention_timeout_without_activity() {
    run(|f, _| {
        assert!(f.user_attention_service.has_user_attention().get());

        // Advance time past the attention timeout (5 seconds)
        runtime::advance_fake_time(ATTENTION_TIMEOUT + 1);

        assert!(!f.user_attention_service.has_user_attention().get());
    });
}

#[test]
fn has_user_attention_is_false_when_window_loses_focus() {
    run(|f, _| {
        assert!(f.user_attention_service.has_user_attention().get());

        f.host_adapter_mock.set_focus(false);

        // Attention is not dependent on focus
        assert!(f.user_attention_service.has_user_attention().get());
    });
}

#[test]
fn has_user_attention_is_restored_when_activity_occurs() {
    run(|f, _| {
        // Wait for attention to expire
        runtime::advance_fake_time(ATTENTION_TIMEOUT + 1);
        assert!(!f.user_attention_service.has_user_attention().get());

        // Simulate activity
        f.host_adapter_mock.set_active(true);

        assert!(f.user_attention_service.has_user_attention().get());
    });
}

#[test]
fn activity_keeps_attention_alive() {
    run(|f, _| {
        // Start with attention
        assert!(f.user_attention_service.has_user_attention().get());

        // Advance time halfway, then activity
        runtime::advance_fake_time(ONE_MINUTE / 2);
        f.host_adapter_mock.set_active(true);
        f.host_adapter_mock.set_active(false);

        // Advance another half minute - should still have attention
        runtime::advance_fake_time(ONE_MINUTE / 2);
        assert!(f.user_attention_service.has_user_attention().get());

        // Now let it expire
        runtime::advance_fake_time(ONE_MINUTE + 1);
        assert!(!f.user_attention_service.has_user_attention().get());
    });
}

mod fire_after_given_focus_time_passed {
    use super::*;

    #[test]
    fn fires_callback_after_accumulated_focus_time() {
        run(|f, store| {
            let callback_fired = Rc::new(Cell::new(false));
            let disposable = f.user_attention_service.fire_after_given_focus_time_passed(3 * ONE_MINUTE, set_flag(&callback_fired));
            store.add(disposable);

            // Mark activity to ensure attention is maintained, then advance 1 minute - not yet fired
            f.host_adapter_mock.set_active(true);
            f.host_adapter_mock.set_active(false);
            runtime::advance_fake_time(ONE_MINUTE);
            assert!(!callback_fired.get());

            // Mark activity and advance another minute - still not fired
            f.host_adapter_mock.set_active(true);
            f.host_adapter_mock.set_active(false);
            runtime::advance_fake_time(ONE_MINUTE);
            assert!(!callback_fired.get());

            // Mark activity and advance 3rd minute - should fire
            f.host_adapter_mock.set_active(true);
            f.host_adapter_mock.set_active(false);
            runtime::advance_fake_time(ONE_MINUTE);
            assert!(callback_fired.get());
        });
    }

    #[test]
    fn does_not_accumulate_time_when_user_has_no_attention() {
        run(|f, store| {
            let callback_fired = Rc::new(Cell::new(false));
            let disposable = f.user_attention_service.fire_after_given_focus_time_passed(2 * ONE_MINUTE, set_flag(&callback_fired));
            store.add(disposable);

            // Mark activity and accumulate 1 minute
            f.host_adapter_mock.set_active(true);
            f.host_adapter_mock.set_active(false);
            runtime::advance_fake_time(ONE_MINUTE);
            assert!(!callback_fired.get());

            // Lose focus - should still accumulate (even with activity)
            f.host_adapter_mock.set_focus(false);
            // Mark activity again to ensure attention is maintained
            f.host_adapter_mock.set_active(true);
            f.host_adapter_mock.set_active(false);
            runtime::advance_fake_time(ONE_MINUTE);
            assert!(callback_fired.get());
        });
    }

    #[test]
    fn stops_accumulating_time_when_attention_expires() {
        run(|f, store| {
            let callback_fired = Rc::new(Cell::new(false));
            let disposable = f.user_attention_service.fire_after_given_focus_time_passed(2 * ONE_MINUTE, set_flag(&callback_fired));
            store.add(disposable);

            // Mark activity and accumulate 1 minute
            f.host_adapter_mock.set_active(true);
            f.host_adapter_mock.set_active(false);
            runtime::advance_fake_time(ONE_MINUTE);
            assert!(!callback_fired.get());

            // Let attention expire (don't mark activity before tick)
            // Advance enough time that the activity timeout expires
            runtime::advance_fake_time(ONE_MINUTE + 1);
            assert!(!f.user_attention_service.has_user_attention().get());
            assert!(!callback_fired.get());

            // This minute shouldn't count (no attention)
            runtime::advance_fake_time(ONE_MINUTE);
            assert!(!callback_fired.get());

            // Restore activity and accumulate 1 more minute
            f.host_adapter_mock.set_active(true);
            f.host_adapter_mock.set_active(false);
            runtime::advance_fake_time(ONE_MINUTE);
            assert!(callback_fired.get());
        });
    }

    #[test]
    fn can_be_disposed_before_callback_fires() {
        run(|f, _| {
            let callback_fired = Rc::new(Cell::new(false));
            let disposable = f.user_attention_service.fire_after_given_focus_time_passed(2 * ONE_MINUTE, set_flag(&callback_fired));

            // Mark activity and accumulate 1 minute
            f.host_adapter_mock.set_active(true);
            f.host_adapter_mock.set_active(false);
            runtime::advance_fake_time(ONE_MINUTE);
            assert!(!callback_fired.get());

            // Dispose before it fires
            disposable.dispose();

            // Advance past threshold - should not fire
            f.host_adapter_mock.set_active(true);
            f.host_adapter_mock.set_active(false);
            runtime::advance_fake_time(ONE_MINUTE);
            assert!(!callback_fired.get());
        });
    }

    #[test]
    fn callback_fires_only_once() {
        run(|f, store| {
            let call_count = Rc::new(Cell::new(0));
            let disposable = {
                let call_count = call_count.clone();
                f.user_attention_service.fire_after_given_focus_time_passed(ONE_MINUTE, Box::new(move || call_count.set(call_count.get() + 1)))
            };
            store.add(disposable);

            // Mark activity and advance 1 minute - should fire
            f.host_adapter_mock.set_active(true);
            f.host_adapter_mock.set_active(false);
            runtime::advance_fake_time(ONE_MINUTE);
            assert_eq!(call_count.get(), 1);

            // Keep ticking, should not fire again
            f.host_adapter_mock.set_active(true);
            f.host_adapter_mock.set_active(false);
            runtime::advance_fake_time(ONE_MINUTE);
            assert_eq!(call_count.get(), 1);
        });
    }
}
