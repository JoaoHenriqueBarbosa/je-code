//! Casos que o `event.test.ts` e o `lifecycle.test.ts` não cobrem, para os combinadores e contêineres
//! portados que ficariam sem nenhum teste. Não é porte de teste do VS Code: o resultado esperado de cada
//! caso vem da leitura do `event.ts`/`lifecycle.ts`.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::cancellation::AbortController;
use je_base::event::{Emitter, Event, EventBufferer, ValueWithChangeEvent, ValueWithChangeEventLike, any, from_dom_event_emitter, track_set_changes};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, MandatoryMutableDisposable, MutableDisposable, to_disposable};

type Shared<T> = Rc<RefCell<Vec<T>>>;

fn collect<T: Clone + 'static>(event: &Event<T>) -> (Shared<T>, je_base::lifecycle::DisposableRef) {
    let seen: Shared<T> = Rc::default();
    let sink = seen.clone();
    let subscription = event.listen(move |v| sink.borrow_mut().push(v.clone()));
    (seen, subscription)
}

#[test]
fn once_if_fires_once_when_the_condition_holds() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let emitter = ds.add_rc(Emitter::<i32>::new());
    let (seen, subscription) = collect(&emitter.event().once_if(|n| *n > 1));
    ds.add(subscription);
    for n in [1, 2, 3, 4] {
        emitter.fire(n);
    }
    assert_eq!(*seen.borrow(), [2]);
}

#[test]
fn reduce_accumulates_across_events() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let emitter = ds.add_rc(Emitter::<i32>::new());
    let (seen, subscription) = collect(&emitter.event().reduce(|last: Option<i32>, e| last.unwrap_or(0) + e, None, None));
    ds.add(subscription);
    for n in [1, 2, 3] {
        emitter.fire(n);
    }
    assert_eq!(*seen.borrow(), [1, 3, 6]);
}

#[test]
fn for_each_signal_and_split() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let emitter = ds.add_rc(Emitter::<i32>::new());
    let side = Rc::new(Cell::new(0));
    let counter = side.clone();
    let (seen, subscription) = collect(&emitter.event().for_each(move |n| counter.set(counter.get() + n), None));
    ds.add(subscription);
    let (signals, subscription) = collect(&emitter.event().signal());
    ds.add(subscription);
    let (evens, odds) = emitter.event().split(|n| n % 2 == 0, None);
    let (even_seen, even_subscription) = collect(&evens);
    let (odd_seen, odd_subscription) = collect(&odds);
    ds.add(even_subscription);
    ds.add(odd_subscription);

    for n in [1, 2, 3, 4] {
        emitter.fire(n);
    }
    assert_eq!(*seen.borrow(), [1, 2, 3, 4]);
    assert_eq!(side.get(), 10);
    assert_eq!(signals.borrow().len(), 4);
    assert_eq!(*even_seen.borrow(), [2, 4]);
    assert_eq!(*odd_seen.borrow(), [1, 3]);
}

#[test]
fn any_forward_and_run_and_subscribe() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let (a, b, target) = (ds.add_rc(Emitter::<i32>::new()), ds.add_rc(Emitter::<i32>::new()), ds.add_rc(Emitter::<i32>::new()));
    let (seen, subscription) = collect(&any(vec![a.event(), b.event()]));
    ds.add(subscription);
    ds.add(a.event().forward(&target));
    let (forwarded, subscription) = collect(&target.event());
    ds.add(subscription);
    let handled: Shared<Option<i32>> = Rc::default();
    let sink = handled.clone();
    ds.add(a.event().run_and_subscribe(move |e| sink.borrow_mut().push(e.copied()), Some(&7)));

    a.fire(1);
    b.fire(2);
    assert_eq!(*seen.borrow(), [1, 2]);
    assert_eq!(*forwarded.borrow(), [1]);
    assert_eq!(*handled.borrow(), [Some(7), Some(1)]);
}

#[test]
fn defer_collapses_events_of_the_same_tick() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let store = ds.store();
    run_with_faked_timers(async move {
        let emitter = store.add_rc(Emitter::<i32>::new());
        let (seen, subscription) = collect(&emitter.event().defer(None, None));
        store.add(subscription);
        emitter.fire(1);
        emitter.fire(2);
        emitter.fire(3);
        timeout(1).await.expect("timeout");
        assert_eq!(seen.borrow().len(), 1);
    });
}

#[test]
fn event_bufferer_reduces_buffered_events_into_one() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    // Um `EventBufferer` para cada redução: no original só o primeiro listener de redução por trecho
    // em buffer registra a função de entrega (`buffers.length === 0`), os demais ficam sem entrega.
    let (bufferer, merge_bufferer) = (EventBufferer::new(), EventBufferer::new());
    let emitter = ds.add_rc(Emitter::<i32>::new());
    let (summed, sum_subscription) = collect(&bufferer.wrap_event_reduce(&emitter.event(), |last: Option<i32>, e| last.unwrap_or(0) + e, Some(0)));
    let (merged, merge_subscription) = collect(&merge_bufferer.wrap_event_merge(&emitter.event(), |last: Option<i32>, e| last.map_or(*e, |l| l.max(*e))));
    ds.add(sum_subscription);
    ds.add(merge_subscription);

    // Sem buffer, cada evento passa pela redução com o valor inicial.
    emitter.fire(5);
    assert_eq!(*summed.borrow(), [5]);
    assert_eq!(*merged.borrow(), [5]);

    bufferer.buffer_events(|| {
        merge_bufferer.buffer_events(|| {
            emitter.fire(1);
            emitter.fire(2);
            emitter.fire(3);
            assert_eq!(summed.borrow().len(), 1);
        });
    });
    assert_eq!(*summed.borrow(), [5, 6]);
    assert_eq!(*merged.borrow(), [5, 3]);
}

#[test]
fn track_set_changes_handles_added_and_removed_items() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let changed = ds.add_rc(Emitter::<()>::new());
    let data: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(vec![1, 2]));
    let log: Shared<String> = Rc::default();

    let (current, sink) = (data.clone(), log.clone());
    let tracker = track_set_changes(
        move || current.borrow().clone(),
        &changed.event(),
        move |item| {
            sink.borrow_mut().push(format!("+{item}"));
            let (sink, item) = (sink.clone(), *item);
            to_disposable(move || sink.borrow_mut().push(format!("-{item}")))
        },
    );
    assert_eq!(*log.borrow(), ["+1", "+2"]);

    *data.borrow_mut() = vec![2, 3];
    changed.fire(());
    assert_eq!(*log.borrow(), ["+1", "+2", "-1", "+3"]);

    tracker.dispose();
    assert_eq!(*log.borrow(), ["+1", "+2", "-1", "+3", "-2", "-3"]);
}

#[test]
fn value_with_change_event_fires_only_on_real_changes() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let value = ValueWithChangeEvent::new(1);
    let (seen, subscription) = collect(&value.on_did_change());
    ds.add(subscription);

    value.set(1);
    value.set(2);
    assert_eq!(value.value(), 2);
    assert_eq!(seen.borrow().len(), 1);

    let constant = ValueWithChangeEvent::constant(9);
    assert_eq!(constant.value(), 9);
    let (never, subscription) = collect(&constant.on_did_change());
    subscription.dispose();
    assert!(never.borrow().is_empty());
}

#[test]
fn from_dom_event_emitter_attaches_with_the_first_listener_and_detaches_with_the_last() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();

    let controller = AbortController::new();
    let (seen, subscription) = collect(&from_dom_event_emitter(Rc::new(controller.signal()), "abort", |_| ()));
    ds.add(subscription);
    assert!(seen.borrow().is_empty());
    controller.abort(None);
    assert_eq!(seen.borrow().len(), 1);

    // Sem listener, o emitter se soltou do sinal: abortar não entrega nada.
    let controller = AbortController::new();
    let (seen, subscription) = collect(&from_dom_event_emitter(Rc::new(controller.signal()), "abort", |_| ()));
    subscription.dispose();
    controller.abort(None);
    assert!(seen.borrow().is_empty());
}

#[test]
fn disposable_base_disposes_what_it_registers() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let disposed = Rc::new(Cell::new(false));
    let flag = disposed.clone();
    let base = Disposable::new();
    base.register(to_disposable(move || flag.set(true)));
    assert!(!disposed.get());
    base.dispose();
    assert!(disposed.get());
}

#[test]
fn mutable_disposables_dispose_the_replaced_value() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log: Shared<&'static str> = Rc::default();
    let make = |name: &'static str| {
        let log = log.clone();
        to_disposable(move || log.borrow_mut().push(name))
    };

    let mutable = MutableDisposable::<dyn IDisposable>::new();
    mutable.set(Some(make("a")));
    mutable.set(Some(make("b")));
    assert_eq!(*log.borrow(), ["a"]);
    let leaked = mutable.clear_and_leak().expect("havia valor");
    assert_eq!(*log.borrow(), ["a"]);
    leaked.dispose();
    mutable.dispose();
    // Depois de descartado o setter não faz nada: quem criou o valor continua dono dele.
    let late_value = make("c");
    mutable.set(Some(late_value.clone()));
    assert!(mutable.value().is_none());
    late_value.dispose();

    let mandatory = MandatoryMutableDisposable::<dyn IDisposable>::new(make("m1"));
    mandatory.set(make("m2"));
    mandatory.dispose();
    assert_eq!(*log.borrow(), ["a", "b", "c", "m1", "m2"]);

    // Um store descartado só avisa e não descarta o que entra depois.
    let store = DisposableStore::new();
    store.dispose();
    DisposableStore::set_disable_disposed_warning(true);
    let late = make("late");
    store.add(late.clone());
    DisposableStore::set_disable_disposed_warning(false);
    assert!(store.is_disposed());
    assert_eq!(log.borrow().len(), 5);
    late.dispose();
}
