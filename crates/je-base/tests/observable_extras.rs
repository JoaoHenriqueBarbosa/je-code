//! Casos que o `observable.test.ts` não cobre, para os utilitários portados de `observableInternal` que
//! ficariam sem nenhum teste. Não é porte de teste do VS Code: o resultado esperado de cada caso vem da
//! leitura do código-fonte do original.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::event::Emitter;
use je_base::lifecycle::{DisposableStore, IDisposable, plain_disposable};
use je_base::observable::base::{IObservable, Observable, SettableExt};
use je_base::observable::debug_name::DebugOwner;
use je_base::observable::experimental::time::was_true_recently;
use je_base::observable::map::ObservableMap;
use je_base::observable::observables::base_observable::{FlattenExt, ObservableExt};
use je_base::observable::observables::observable_value::{ObservableValueOptions, disposable_observable_value, observable_value_opts};
use je_base::observable::reactions::autorun::{Delta, IterableDelta, autorun_delta, autorun_iterable_delta};
use je_base::observable::set::ObservableSet;
use je_base::observable::utils::promise::{ObservableLazy, ObservablePromise};
use je_base::observable::utils::utils::DebounceMs;
use je_base::observable::{
    autorun, autorun_self_disposable, const_observable, debounced_observable, derived, derived_const_once_defined, derived_observable_with_cache,
    derived_with_cancellation_token, derived_with_setter, latest_changed_value, map_observable_array_cached, observable_signal_from_event,
    observable_value, run_on_change, transaction, was_event_triggered_recently,
};

type Log = Rc<RefCell<Vec<String>>>;

fn log() -> Log {
    Rc::default()
}

fn entries(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

fn push(log: &Log, s: impl Into<String>) {
    log.borrow_mut().push(s.into());
}

#[test]
fn map_and_flatten() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let l = log();
    let a = observable_value::<i32, ()>("a", 1);
    let doubled = a.map(|v, _| v * 2);
    {
        let (l, doubled) = (l.clone(), doubled.clone());
        ds.add(autorun(move |reader| push(&l, format!("doubled={}", doubled.read(reader)))));
    }
    a.set(2, None);
    assert_eq!(entries(&l), vec!["doubled=2", "doubled=4"]);

    let inner1 = observable_value::<i32, ()>("inner1", 10);
    let inner2 = observable_value::<i32, ()>("inner2", 20);
    let outer = observable_value::<IObservable<i32>, ()>("outer", inner1.to_observable());
    let flat = outer.flatten();
    assert_eq!(flat.get(), 10);
    outer.set(inner2.to_observable(), None);
    assert_eq!(flat.get(), 20);
}

#[test]
fn lazy_observable_value_notifies_once_per_transaction() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let l = log();
    let v = observable_value_opts::<i32, ()>(ObservableValueOptions { lazy: true, ..ObservableValueOptions::default() }, 0);
    {
        let (l, v) = (l.clone(), v.clone());
        ds.add(autorun(move |reader| push(&l, format!("v={}", v.read(reader)))));
    }
    v.set(1, None);
    transaction(|tx| {
        v.set(2, Some(tx));
        v.set(3, Some(tx));
    });
    assert_eq!(entries(&l), vec!["v=0", "v=1", "v=3"]);
}

#[test]
fn const_observable_never_changes() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let l = log();
    let c = const_observable(7);
    {
        let (l, c) = (l.clone(), c.clone());
        ds.add(autorun(move |reader| push(&l, format!("c={}", c.read(reader)))));
    }
    assert_eq!(c.get(), 7);
    assert_eq!(entries(&l), vec!["c=7"]);
}

#[test]
fn derived_with_setter_calls_the_setter() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let source = observable_value::<i32, ()>("source", 1);
    let d = {
        let (read, write) = (source.clone(), source.clone());
        derived_with_setter(None, move |reader| read.read(reader) * 10, move |v, tx| write.set(v / 10, tx))
    };
    assert_eq!(d.get(), 10);
    d.set(50, None);
    assert_eq!(source.get(), 5);
    assert_eq!(d.get(), 50);
}

#[test]
fn disposable_observable_value_disposes_the_previous_value() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let l = log();
    let make = |name: &'static str| {
        let l = l.clone();
        Some(plain_disposable(move || push(&l, format!("dispose {name}"))))
    };
    let v = disposable_observable_value::<()>("v", make("a"));
    v.set(make("b"), None);
    assert_eq!(entries(&l), vec!["dispose a"]);
    v.dispose();
    assert_eq!(entries(&l), vec!["dispose b"]);
}

#[test]
fn autorun_delta_reports_last_and_new_value() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let l = log();
    let a = observable_value::<i32, ()>("a", 1);
    let d = {
        let l = l.clone();
        autorun_delta(a.to_observable(), move |Delta { last_value, new_value }| push(&l, format!("{last_value:?}->{new_value}")))
    };
    a.set(2, None);
    assert_eq!(entries(&l), vec!["None->1", "Some(1)->2"]);
    d.dispose();
}

#[test]
fn autorun_iterable_delta_reports_added_and_removed() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let l = log();
    let items = observable_value::<Rc<Vec<i32>>, ()>("items", Rc::new(vec![1, 2]));
    let d = {
        let (l, items) = (l.clone(), items.clone());
        autorun_iterable_delta(
            move |reader| items.read(reader).to_vec(),
            move |IterableDelta { added_values, removed_values }| push(&l, format!("+{added_values:?} -{removed_values:?}")),
        )
    };
    items.set(Rc::new(vec![2, 3]), None);
    assert_eq!(entries(&l), vec!["+[1, 2] -[]", "+[3] -[1]"]);
    d.dispose();
}

#[test]
fn run_on_change_reports_value_and_previous_value() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let l = log();
    let a = observable_value::<i32, ()>("a", 1);
    let d = {
        let l = l.clone();
        run_on_change::<i32, ()>(a.to_observable(), move |v, prev, _| push(&l, format!("{prev}->{v}")))
    };
    assert!(entries(&l).is_empty());
    a.set(2, None);
    a.set(3, None);
    assert_eq!(entries(&l), vec!["1->2", "2->3"]);
    d.dispose();
}

#[test]
fn observable_signal_from_event_reruns_on_each_event() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let l = log();
    let emitter = ds.add_rc(Emitter::<i32>::new());
    let signal = observable_signal_from_event("sig", emitter.event());
    {
        let (l, signal) = (l.clone(), signal.clone());
        ds.add(autorun(move |reader| {
            signal.read(reader);
            push(&l, "run");
        }));
    }
    emitter.fire(1);
    emitter.fire(2);
    assert_eq!(entries(&l), vec!["run", "run", "run"]);
}

#[test]
fn was_event_triggered_recently_resets_after_timeout() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let store = DisposableStore::new();
        let emitter = Emitter::<()>::new();
        let recently = was_event_triggered_recently(&emitter.event(), 100, &store);
        assert!(!recently.get());
        emitter.fire(());
        assert!(recently.get());
        timeout(100).await.expect("timeout");
        assert!(!recently.get());
        store.dispose();
        emitter.dispose();
        drop(ds);
    });
}

#[test]
fn debounced_observable_waits_for_the_quiet_period() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let l = log();
        let source = observable_value::<i32, ()>("source", 0);
        let debounced = debounced_observable(source.to_observable(), DebounceMs::Fixed(100));
        {
            let l = l.clone();
            ds.add(autorun(move |reader| push(&l, format!("v={}", debounced.read(reader)))));
        }
        source.set(1, None);
        timeout(50).await.expect("timeout");
        source.set(2, None);
        timeout(99).await.expect("timeout");
        assert_eq!(entries(&l), vec!["v=0"]);
        timeout(1).await.expect("timeout");
        assert_eq!(entries(&l), vec!["v=2"]);
    });
}

#[test]
fn was_true_recently_stays_true_for_the_period() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let store = DisposableStore::new();
        let source = observable_value::<bool, ()>("source", false);
        let recently = was_true_recently(source.to_observable(), 100, &store);
        source.set(true, None);
        assert!(recently.get());
        source.set(false, None);
        assert!(recently.get());
        timeout(100).await.expect("timeout");
        assert!(!recently.get());
        store.dispose();
    });
}

#[test]
fn derived_observable_with_cache_receives_the_last_value() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let a = observable_value::<i32, ()>("a", 1);
    let acc = {
        let a = a.clone();
        derived_observable_with_cache::<i32>(None, move |reader, last| last.copied().unwrap_or(0) + a.read(reader))
    };
    let l = log();
    {
        let (l, acc) = (l.clone(), acc.clone());
        ds.add(autorun(move |reader| push(&l, format!("acc={}", acc.read(reader)))));
    }
    a.set(5, None);
    assert_eq!(entries(&l), vec!["acc=1", "acc=6"]);
}

#[test]
fn map_observable_array_cached_creates_each_item_once() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let l = log();
    let items = observable_value::<Rc<Vec<i32>>, ()>("items", Rc::new(vec![1, 2]));
    let mapped = {
        let l = l.clone();
        map_observable_array_cached(None, items.to_observable(), move |i, store| {
            push(&l, format!("create {i}"));
            let l = l.clone();
            let i = *i;
            store.add(plain_disposable(move || push(&l, format!("dispose {i}"))));
            Rc::new(i * 10)
        })
    };
    {
        let (l, mapped) = (l.clone(), mapped.clone());
        let d = autorun(move |reader| push(&l, format!("out={:?}", mapped.read(reader).iter().map(|v| **v).collect::<Vec<_>>())));
        ds.add(d);
    }
    items.set(Rc::new(vec![2, 3]), None);
    assert_eq!(entries(&l), vec!["create 1", "create 2", "out=[10, 20]", "create 3", "dispose 1", "out=[20, 30]"]);
    drop(ds);
    assert_eq!(entries(&l), vec!["dispose 2", "dispose 3"]);
}

#[test]
fn observable_map_and_set_notify_on_change() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let l = log();
    let map = ObservableMap::<&'static str, i32>::new();
    let set = ObservableSet::<i32>::new();
    {
        let (l, m, s) = (l.clone(), map.observable.clone(), set.observable.clone());
        ds.add(autorun(move |reader| push(&l, format!("map={} set={}", m.read(reader).size(), s.read(reader).size()))));
    }
    map.set("a", 1, None);
    map.set("a", 1, None); // mesmo valor: sem notificação
    set.add(1, None);
    set.add(1, None); // já existe: sem notificação
    assert!(map.delete(&"a", None));
    assert!(!map.delete(&"a", None));
    set.clear(None);
    assert_eq!(entries(&l), vec!["map=0 set=0", "map=1 set=0", "map=1 set=1", "map=0 set=1", "map=0 set=0"]);
}

#[test]
fn latest_changed_value_follows_the_last_observable_that_changed() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let a = observable_value::<i32, ()>("a", 1);
    let b = observable_value::<i32, ()>("b", 2);
    let latest = latest_changed_value(DebugOwner::plain(), vec![a.to_observable(), b.to_observable()]);
    let l = log();
    {
        let (l, latest) = (l.clone(), latest.clone());
        ds.add(autorun(move |reader| push(&l, format!("latest={}", latest.read(reader)))));
    }
    a.set(10, None);
    b.set(20, None);
    assert_eq!(entries(&l).last().map(String::as_str), Some("latest=20"));
    a.set(30, None);
    assert_eq!(entries(&l).last().map(String::as_str), Some("latest=30"));
}

#[test]
fn derived_const_once_defined_keeps_the_first_value() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let source = observable_value::<i32, ()>("source", 1);
    let calls = Rc::new(Cell::new(0));
    let once = {
        let (source, calls) = (source.clone(), calls.clone());
        derived_const_once_defined(DebugOwner::plain(), move |reader| {
            calls.set(calls.get() + 1);
            source.read(reader)
        })
    };
    let l = log();
    {
        let (l, once) = (l.clone(), once.clone());
        ds.add(autorun(move |reader| push(&l, format!("{:?}", once.read(reader)))));
    }
    source.set(2, None);
    // Depois do primeiro valor, o derived devolve o valor guardado sem reler a fonte.
    assert_eq!(entries(&l), vec!["Some(1)"]);
    assert_eq!(calls.get(), 1);
}

#[test]
fn observable_promise_publishes_the_result() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_faked_timers(async move {
        let p = ObservablePromise::resolved(5);
        assert_eq!(p.promise_result.get().map(|_| ()), None);
        p.promise.clone().await.expect("resolve");
        assert_eq!(p.resolved_value.get(), Some(5));
    });
}

#[test]
fn observable_lazy_computes_once() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let calls = Rc::new(Cell::new(0));
    let lazy = {
        let calls = calls.clone();
        ObservableLazy::new(move || {
            calls.set(calls.get() + 1);
            42
        })
    };
    assert_eq!(lazy.cached_value().get(), None);
    assert_eq!(lazy.get_value(), 42);
    assert_eq!(lazy.get_value(), 42);
    assert_eq!(calls.get(), 1);
    assert_eq!(lazy.cached_value().get(), Some(42));
}

#[test]
fn derived_with_cancellation_token_cancels_the_previous_run() {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let a = observable_value::<i32, ()>("a", 0);
    let tokens = Rc::new(RefCell::new(Vec::new()));
    let d = {
        let (a, tokens) = (a.clone(), tokens.clone());
        derived_with_cancellation_token(move |reader, token| {
            tokens.borrow_mut().push(token.clone());
            a.read(reader)
        })
    };
    {
        let d = d.clone();
        ds.add(autorun(move |reader| {
            d.read(reader);
        }));
    }
    a.set(1, None);
    let tokens = tokens.borrow();
    assert_eq!(tokens.len(), 2);
    assert!(tokens[0].is_cancellation_requested());
    assert!(!tokens[1].is_cancellation_requested());
}

#[test]
fn autorun_self_disposable_stops_after_dispose() {
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let l = log();
    let a = observable_value::<i32, ()>("a", 0);
    let d = {
        let (l, a) = (l.clone(), a.clone());
        autorun_self_disposable(move |reader| {
            let v = a.read(reader);
            push(&l, format!("v={v}"));
            if v == 1 {
                reader.dispose();
            }
        })
    };
    a.set(1, None);
    a.set(2, None);
    assert_eq!(entries(&l), vec!["v=0", "v=1"]);
    d.dispose();
}

#[test]
fn observable_from_event_with_an_unused_derived_marker() {
    // A `derived` sem observadores nunca guarda cache: cada `get` recalcula.
    let _ds = ensure_no_disposables_are_leaked_in_test_suite();
    let calls = Rc::new(Cell::new(0));
    let d = {
        let calls = calls.clone();
        derived(move |_| {
            calls.set(calls.get() + 1);
            calls.get()
        })
    };
    assert_eq!((d.get(), d.get()), (1, 2));
}
