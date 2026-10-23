//! Porte de `src/vs/base/test/common/observables/throttledObservable.test.ts` (suíte `throttledObservable`), caso a caso e na mesma ordem.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::lifecycle::IDisposable;
use je_base::observable::base::{Observable, SettableExt};
use je_base::observable::{autorun, observable_value, throttled_observable};
use je_base::runtime;

type Log<T> = Rc<RefCell<Vec<T>>>;

#[test]
fn emits_initial_value_immediately_without_delay() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let log: Log<String> = Rc::default();
        let source = observable_value::<i32, ()>("source", 0);
        let throttled = throttled_observable(source.to_observable(), 100);

        let l = log.clone();
        ds.add(autorun(move |reader| l.borrow_mut().push(format!("t=0: {}", throttled.read(reader)))));

        assert_eq!(*log.borrow(), vec!["t=0: 0".to_string()]);
    });
}

#[test]
fn delays_first_change_by_throttle_ms() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let log: Log<String> = Rc::default();
        let source = observable_value::<i32, ()>("source", 0);
        let throttled = throttled_observable(source.to_observable(), 100);

        let l = log.clone();
        ds.add(autorun(move |reader| l.borrow_mut().push(format!("value: {}", throttled.read(reader)))));
        log.borrow_mut().clear();

        source.set(1, None);

        // Ainda não emitiu.
        assert!(log.borrow().is_empty());

        // Espera o timer do throttle.
        timeout(100).await.expect("timeout");

        assert_eq!(*log.borrow(), vec!["value: 1".to_string()]);
    });
}

#[test]
fn does_not_reset_timer_on_subsequent_changes_no_starvation() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let log: Log<String> = Rc::default();
        let source = observable_value::<i32, ()>("source", 0);
        let throttled = throttled_observable(source.to_observable(), 100);

        let l = log.clone();
        ds.add(autorun(move |reader| l.borrow_mut().push(format!("value: {}", throttled.read(reader)))));
        log.borrow_mut().clear();

        // A primeira mudança inicia o timer.
        source.set(1, None);
        // Mudanças seguintes dentro da janela NÃO reiniciam o timer.
        timeout(50).await.expect("timeout");
        source.set(2, None);
        timeout(30).await.expect("timeout");
        source.set(3, None);

        // Aqui passaram ~80ms. O timer foi marcado em t=0 para 100ms, não deve ter disparado.
        assert!(log.borrow().is_empty());

        // Espera o resto (100ms desde a primeira mudança).
        timeout(30).await.expect("timeout");

        // Deve ter disparado com o último valor.
        assert_eq!(*log.borrow(), vec!["value: 3".to_string()]);
    });
}

#[test]
fn emits_again_after_previous_throttle_window_completes() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let log: Log<String> = Rc::default();
        let source = observable_value::<i32, ()>("source", 0);
        let throttled = throttled_observable(source.to_observable(), 100);

        let l = log.clone();
        ds.add(autorun(move |reader| l.borrow_mut().push(format!("value: {}", throttled.read(reader)))));
        log.borrow_mut().clear();

        // Primeira janela.
        source.set(1, None);
        timeout(100).await.expect("timeout");
        assert_eq!(*log.borrow(), vec!["value: 1".to_string()]);
        log.borrow_mut().clear();

        // Segunda janela: a nova mudança inicia um timer novo.
        source.set(2, None);
        assert!(log.borrow().is_empty());

        timeout(100).await.expect("timeout");
        assert_eq!(*log.borrow(), vec!["value: 2".to_string()]);
    });
}

#[test]
fn records_correct_timing_sequence() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let timings: Log<(u64, i32)> = Rc::default();
        let start_time = runtime::date_now();
        let source = observable_value::<i32, ()>("source", 0);
        let throttled = throttled_observable(source.to_observable(), 200);

        let t = timings.clone();
        ds.add(autorun(move |reader| {
            let v = throttled.read(reader);
            t.borrow_mut().push((runtime::date_now() - start_time, v));
        }));

        // t=0: valor inicial emitido de forma síncrona.
        // t=50: mudança, inicia o timer de 200ms.
        timeout(50).await.expect("timeout");
        source.set(1, None);

        // t=100: mudança com o timer ativo (o timer NÃO reinicia).
        timeout(50).await.expect("timeout");
        source.set(2, None);

        // t=250: o timer dispara (200ms depois de t=50).
        timeout(160).await.expect("timeout");

        // t=300: outra mudança, inicia um timer novo de 200ms.
        timeout(40).await.expect("timeout");
        source.set(3, None);

        // t=500: o timer dispara (200ms depois de t=300).
        timeout(200).await.expect("timeout");

        assert_eq!(*timings.borrow(), vec![(0, 0), (250, 2), (500, 3)]);
    });
}

#[test]
fn cleanup_on_dispose_stops_pending_timer() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let log: Log<String> = Rc::default();
        let source = observable_value::<i32, ()>("source", 0);
        let throttled = throttled_observable(source.to_observable(), 100);

        let l = log.clone();
        let d = ds.add(autorun(move |reader| l.borrow_mut().push(format!("value: {}", throttled.read(reader)))));
        log.borrow_mut().clear();

        source.set(1, None);
        d.dispose();

        timeout(150).await.expect("timeout");

        // Nada deve ter sido emitido depois do dispose.
        assert!(log.borrow().is_empty());
    });
}
