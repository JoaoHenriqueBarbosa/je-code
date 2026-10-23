//! Casos próprios do porte de `decorators.ts`: o original só testa `memoize` e `throttle` (ver
//! `decorators.rs`); aqui entram `debounce`, o `throttle` sem acumulador e o `cancelPreviousCalls`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationTokenSource;
use je_base::decorators::{CancelPreviousCalls, Debounce, Throttle};
use je_base::lifecycle::IDisposable;
use je_base::runtime;

#[test]
fn debounce_runs_once_with_the_reduced_value() {
    runtime::with_faked_timers(|| {
        let spy: Rc<RefCell<Vec<i32>>> = Rc::default();
        let s = spy.clone();
        let report = Debounce::new(100, |previous: Option<i32>, p: i32| previous.unwrap_or(0) + p, Some(Rc::new(|| 0)), move |p| s.borrow_mut().push(p));

        report.call(1);
        runtime::advance_fake_time(50);
        report.call(2);
        runtime::advance_fake_time(99);
        assert!(spy.borrow().is_empty());
        runtime::advance_fake_time(1);
        assert_eq!(*spy.borrow(), vec![3]);

        // Depois de rodar, o acumulado recomeça do valor inicial.
        report.call(5);
        runtime::advance_fake_time(100);
        assert_eq!(*spy.borrow(), vec![3, 5]);
    });
}

#[test]
fn debounce_without_reducer_passes_the_last_arguments() {
    runtime::with_faked_timers(|| {
        let spy: Rc<RefCell<Vec<String>>> = Rc::default();
        let s = spy.clone();
        let call = Debounce::without_reducer(10, move |text: String| s.borrow_mut().push(text));

        call.call("a".to_string());
        call.call("b".to_string());
        runtime::advance_fake_time(10);
        assert_eq!(*spy.borrow(), vec!["b".to_string()]);
    });
}

#[test]
fn throttle_without_accumulator_runs_leading_and_trailing() {
    runtime::with_faked_timers(|| {
        let count = Rc::new(RefCell::new(0));
        let c = count.clone();
        let tick = Throttle::new(100, |_: Option<()>, _: ()| (), None, move |()| *c.borrow_mut() += 1);

        tick.call(());
        tick.call(());
        tick.call(());
        assert_eq!(*count.borrow(), 1);
        runtime::advance_fake_time(100);
        assert_eq!(*count.borrow(), 2);
    });
}

#[test]
fn cancel_previous_calls_cancels_the_token_of_the_previous_call() {
    let decorator = CancelPreviousCalls::new();
    let first = decorator.next_call(None);
    assert!(!first.is_cancellation_requested());
    let second = decorator.next_call(None);
    assert!(first.is_cancellation_requested());
    assert!(!second.is_cancellation_requested());

    // Com um token de fora, cancelar o de fora cancela o novo.
    let outer = CancellationTokenSource::new(None);
    let third = decorator.next_call(Some(&outer.token()));
    assert!(second.is_cancellation_requested());
    outer.cancel();
    assert!(third.is_cancellation_requested());

    // Descartar não cancela a chamada em andamento.
    let fourth = decorator.next_call(None);
    decorator.dispose();
    assert!(!fourth.is_cancellation_requested());
}
