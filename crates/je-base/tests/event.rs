//! Porte de `src/vs/base/test/common/event.test.ts` (suíte `Event`), caso a caso e na mesma ordem.
//!
//! Pendente (depende de `observable.ts`, ainda não portado, sem aproximação):
//! - `Event utils - ensureNoDisposablesAreLeakedInTestSuite` / `fromObservable`: usa `observableValue`,
//!   `obs.set` e `transaction`, e `Event.fromObservable` (que implementa `IObserver` sobre `IObservable`).
//!
//! Notas de tradução:
//! - Os testes `async` rodam em `common::run_with_faked_timers` (o `runWithFakedTimers` do original, com
//!   `await` sobre as `Promise` do `je_base::promise`); os que esperavam `setTimeout` real também, o que só
//!   encurta a espera, a ordem é a mesma.
//! - Os testes com `done` guardam o observado e conferem depois que o loop esvazia, porque uma asserção que
//!   falha dentro de um timer ou microtarefa vira erro inesperado, e o `LeakGuard` reprova o teste por ele.
//! - O listener que lança `9` (um número) lança um `Error` com a mensagem `9`: o porte só lança `Error`.
//! - `thisArgs` não existe; o fecho captura o contexto. `sinon.stub()` vira um `Vec` de argumentos.
//! - `DisposableStoreWithSize.size` vira `DisposableStore::len`.
//! - `Object.hasOwn(emitter, '_leakageMon')` e `emitter._size = n` viram os ganchos
//!   `has_leakage_monitor_for_testing` e `set_size_for_testing` do `Emitter`.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, record_unexpected_errors, run_with_faked_timers, timeout};
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::{
    AsyncEmitter, DebounceEmitter, DebounceOptions, Disposables, DynamicListEventMultiplexer, Emitter, EmitterOptions, Event, EventBufferer, EventMultiplexer,
    ListenerLeakError, MicrotaskEmitter, PauseableEmitter, Relay, ThrottleOptions, WaitUntilEvent, any, set_global_leak_warning_threshold,
};
use je_base::lifecycle::{ById, DisposableRef, DisposableStore, DisposableTracker, IDisposable, set_disposable_tracker};
use je_base::promise::Promise;
use je_base::runtime;
use je_base::symbols::Delay;

type Shared<T> = Rc<RefCell<Vec<T>>>;

fn shared<T>() -> Shared<T> {
    Rc::new(RefCell::new(Vec::new()))
}

fn snapshot<T: Clone>(values: &Shared<T>) -> Vec<T> {
    values.borrow().clone()
}

mod samples {
    use super::*;

    pub struct EventCounter {
        pub count: Cell<i32>,
    }

    impl EventCounter {
        pub fn new() -> Rc<Self> {
            Rc::new(Self { count: Cell::new(0) })
        }

        pub fn on_event(&self) {
            self.count.set(self.count.get() + 1);
        }
    }

    pub struct Document3 {
        on_did_change_emitter: Rc<Emitter<String>>,
    }

    impl Document3 {
        pub fn new() -> Rc<Self> {
            Rc::new(Self { on_did_change_emitter: Emitter::new() })
        }

        pub fn on_did_change(&self) -> Event<String> {
            self.on_did_change_emitter.event()
        }

        pub fn set_text(&self, value: &str) {
            self.on_did_change_emitter.fire(value.to_owned());
        }
    }

    impl IDisposable for Document3 {
        fn dispose(&self) {
            self.on_did_change_emitter.dispose();
        }
    }
}

use samples::{Document3, EventCounter};

mod event_utils_dispose {
    use super::*;

    /// O `setup`/`teardown` da suíte: o `ds` do `ensureNoDisposablesAreLeaked...` e um rastreador próprio.
    struct Fixture {
        ds: common::LeakGuard,
        tracker: Rc<DisposableTracker>,
    }

    impl Fixture {
        fn new() -> Self {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let tracker = Rc::new(DisposableTracker::new());
            set_disposable_tracker(Some(tracker.clone()));
            Self { ds, tracker }
        }

        fn assert_disposables_count(&self, expected: usize) {
            assert_eq!(self.tracker.get_tracked_disposables().len(), expected);
        }

        fn assert_disposables(&self, expected: &[&DisposableRef]) {
            let actual = self.tracker.get_tracked_disposables();
            assert_eq!(actual.len(), expected.len());
            for item in &actual {
                assert!(expected.iter().any(|e| je_base::lifecycle::id_of(e) == je_base::lifecycle::id_of(item)));
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            set_disposable_tracker(None);
        }
    }

    #[test]
    fn no_leak_with_snapshot_utils() {
        let f = Fixture::new();

        let store = DisposableStore::new();
        let emitter = f.ds.add_rc(Emitter::<i32>::new());
        let evens = emitter.event().filter(|n| n % 2 == 0, Some(&store));
        f.assert_disposables_count(1); // o snapshot só escuta quando `evens` é escutado

        let all = Rc::new(Cell::new(0));
        let sum = all.clone();
        let leaked = evens.listen(move |n| sum.set(sum.get() + n));
        // `isDisposable(leaked)`: garantido pelo tipo `DisposableRef`.
        f.assert_disposables_count(3);

        emitter.dispose();
        store.dispose();
        f.assert_disposables(&[&leaked]); // `leaked` ainda está lá
        leaked.dispose();
    }

    #[test]
    fn no_leak_with_debounce_util() {
        let f = Fixture::new();
        let store = DisposableStore::new();
        let emitter = f.ds.add_rc(Emitter::<i32>::new());
        let debounced = emitter.event().debounce(|_, _| 0, DebounceOptions::default(), Some(&store));
        f.assert_disposables_count(1); // o debounce só escuta quando `debounced` é escutado

        let all = Rc::new(Cell::new(0));
        let sum = all.clone();
        let leaked = debounced.listen(move |n| sum.set(sum.get() + n));
        f.assert_disposables_count(3);

        emitter.dispose();
        store.dispose();

        f.assert_disposables(&[&leaked]); // `leaked` ainda está lá
        leaked.dispose();
    }

    #[test]
    fn debounce_util_can_be_disposed_without_listeners() {
        let f = Fixture::new();
        let store = DisposableStore::new();
        let emitter = f.ds.add_rc(Emitter::<i32>::new());
        emitter.event().debounce(|_, _| 0, DebounceOptions::default(), Some(&store));

        store.dispose();
    }
}

mod event_suite {
    use super::*;

    #[test]
    fn emitter_plain() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let counter = EventCounter::new();

        let doc = ds.add_rc(Document3::new());

        let c = counter.clone();
        let subscription = doc.on_did_change().listen(move |_| c.on_event());

        doc.set_text("far");
        doc.set_text("boo");

        // desliga o listener
        subscription.dispose();
        doc.set_text("boo");
        assert_eq!(counter.count.get(), 2);
    }

    #[test]
    fn emitter_duplicate_functions() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let calls = shared::<String>();
        let make = |prefix: &'static str| {
            let calls = calls.clone();
            let f: je_base::event::Listener<String> = Rc::new(move |v: &String| calls.borrow_mut().push(format!("{prefix}{v}")));
            f
        };
        let (a, b) = (make("a"), make("b"));

        let emitter = ds.add_rc(Emitter::<String>::new());

        ds.add(emitter.event().listen_rc(a.clone()));
        ds.add(emitter.event().listen_rc(b));
        let s2 = emitter.event().listen_rc(a);

        emitter.fire("1".to_owned());
        assert_eq!(snapshot(&calls), ["a1", "b1", "a1"]);

        s2.dispose();
        calls.borrow_mut().clear();
        emitter.fire("2".to_owned());
        assert_eq!(snapshot(&calls), ["a2", "b2"]);
    }

    #[test]
    fn emitter_dispose_listener_during_emission() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        for keep_first_mod in 1..4 {
            let emitter = ds.add_rc(Emitter::<()>::new());
            let calls = shared::<usize>();
            let disposables = shared::<DisposableRef>();
            for n in 0..25 {
                let (calls, all) = (calls.clone(), disposables.clone());
                let d = ds.add(emitter.event().listen(move |_| {
                    if n % keep_first_mod == 0 {
                        let d = all.borrow()[n].clone();
                        d.dispose();
                    }
                    calls.borrow_mut().push(n);
                }));
                disposables.borrow_mut().push(d);
            }

            emitter.fire(());
            assert_eq!(snapshot(&calls), (0..25).collect::<Vec<_>>());
        }
    }

    #[test]
    fn emitter_dispose_emitter_during_emission() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let emitter = ds.add_rc(Emitter::<()>::new());
        let calls = shared::<usize>();
        let mut disposables = Vec::new();
        for n in 0..25 {
            let (calls, e) = (calls.clone(), emitter.clone());
            disposables.push(ds.add(emitter.event().listen(move |_| {
                if n == 10 {
                    e.dispose();
                }
                calls.borrow_mut().push(n);
            })));
        }

        emitter.fire(());
        disposables.iter().for_each(|d| d.dispose());
        assert_eq!(snapshot(&calls), (0..11).collect::<Vec<_>>());
    }

    #[test]
    fn emitter_shared_delivery_queue() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let delivery_queue = je_base::event::EventDeliveryQueue::new();
        let options = || EmitterOptions { delivery_queue: Some(delivery_queue.clone()), ..EmitterOptions::default() };
        let emitter1 = ds.add_rc(Emitter::<i32>::with_options(options()));
        let emitter2 = ds.add_rc(Emitter::<i32>::with_options(options()));

        let calls = shared::<String>();
        let (c, e2) = (calls.clone(), emitter2.clone());
        ds.add(emitter1.event().listen(move |d| {
            c.borrow_mut().push(format!("{d}a"));
            if *d == 1 {
                e2.fire(2);
            }
        }));
        let c = calls.clone();
        ds.add(emitter1.event().listen(move |d| c.borrow_mut().push(format!("{d}b"))));

        let (c, e1) = (calls.clone(), emitter1.clone());
        ds.add(emitter2.event().listen(move |d| {
            c.borrow_mut().push(format!("{d}c"));
            e1.dispose();
        }));
        let c = calls.clone();
        ds.add(emitter2.event().listen(move |d| c.borrow_mut().push(format!("{d}d"))));

        emitter1.fire(1);

        // 1. Confere que 2 não é entregue antes de 1 terminar
        // 2. Confere que 2 termina de ser entregue mesmo com um emitter descartado
        assert_eq!(snapshot(&calls), ["1a", "1b", "2c", "2d"]);
    }

    #[test]
    fn emitter_handles_removal_during_3() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let (fn1, fn2) = (shared::<String>(), shared::<String>());
        let emitter = ds.add_rc(Emitter::<String>::new());

        let args = fn1.clone();
        ds.add(emitter.event().listen(move |v| args.borrow_mut().push(v.clone())));
        let h: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let own = h.clone();
        *h.borrow_mut() = Some(emitter.event().listen(move |_| {
            let own = own.borrow().clone();
            if let Some(own) = own {
                own.dispose();
            }
        }));
        let args = fn2.clone();
        ds.add(emitter.event().listen(move |v| args.borrow_mut().push(v.clone())));
        emitter.fire("foo".to_owned());

        assert_eq!(snapshot(&fn2), ["foo"]);
        assert_eq!(snapshot(&fn1), ["foo"]);
    }

    #[test]
    fn emitter_handles_removal_during_2() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fn1 = shared::<String>();
        let emitter = ds.add_rc(Emitter::<String>::new());

        let args = fn1.clone();
        ds.add(emitter.event().listen(move |v| args.borrow_mut().push(v.clone())));
        let h: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
        let own = h.clone();
        *h.borrow_mut() = Some(emitter.event().listen(move |_| {
            let own = own.borrow().clone();
            if let Some(own) = own {
                own.dispose();
            }
        }));
        emitter.fire("foo".to_owned());

        assert_eq!(snapshot(&fn1), ["foo"]);
    }

    #[test]
    fn emitter_bucket() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let counter = EventCounter::new();

        let bucket = shared::<DisposableRef>();
        let doc = ds.add_rc(Document3::new());
        let c = counter.clone();
        let subscription = doc.on_did_change().listen_into(move |_| c.on_event(), &Disposables::Array(bucket.clone()));

        doc.set_text("far");
        doc.set_text("boo");

        // desliga o listener
        loop {
            let last = bucket.borrow_mut().pop();
            let Some(d) = last else { break };
            d.dispose();
        }
        doc.set_text("boo");

        // noop
        subscription.dispose();

        doc.set_text("boo");
        assert_eq!(counter.count.get(), 2);
    }

    #[test]
    fn emitter_store() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let counter = EventCounter::new();

        let bucket = ds.add_rc(DisposableStore::new());
        let doc = ds.add_rc(Document3::new());
        let c = counter.clone();
        let subscription = doc.on_did_change().listen_in(move |_| c.on_event(), &bucket);

        doc.set_text("far");
        doc.set_text("boo");

        // desliga o listener
        bucket.clear();
        doc.set_text("boo");

        // noop
        subscription.dispose();

        doc.set_text("boo");
        assert_eq!(counter.count.get(), 2);
    }

    #[test]
    fn on_first_add_on_last_remove() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        let first_count = Rc::new(Cell::new(0));
        let last_count = Rc::new(Cell::new(0));
        let (first, last) = (first_count.clone(), last_count.clone());
        let a = ds.add_rc(Emitter::<()>::with_options(EmitterOptions {
            on_will_add_first_listener: Some(Rc::new(move |_| first.set(first.get() + 1))),
            on_did_remove_last_listener: Some(Rc::new(move |_| last.set(last.get() + 1))),
            ..EmitterOptions::default()
        }));

        assert_eq!(first_count.get(), 0);
        assert_eq!(last_count.get(), 0);

        let mut subscription1 = ds.add(a.event().listen(|_| {}));
        let subscription2 = ds.add(a.event().listen(|_| {}));
        assert_eq!(first_count.get(), 1);
        assert_eq!(last_count.get(), 0);

        subscription1.dispose();
        assert_eq!(first_count.get(), 1);
        assert_eq!(last_count.get(), 0);

        subscription2.dispose();
        assert_eq!(first_count.get(), 1);
        assert_eq!(last_count.get(), 1);

        subscription1 = ds.add(a.event().listen(|_| {}));
        assert_eq!(first_count.get(), 2);
        assert_eq!(last_count.get(), 1);
        drop(subscription1);
    }

    #[test]
    fn on_did_add_listener() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let count = Rc::new(Cell::new(0));
        let counter = count.clone();
        let a = ds.add_rc(Emitter::<()>::with_options(EmitterOptions {
            on_did_add_listener: Some(Rc::new(move |_| counter.set(counter.get() + 1))),
            ..EmitterOptions::default()
        }));

        assert_eq!(count.get(), 0);

        let mut subscription = ds.add(a.event().listen(|_| {}));
        assert_eq!(count.get(), 1);

        subscription.dispose();
        assert_eq!(count.get(), 1);

        subscription = ds.add(a.event().listen(|_| {}));
        assert_eq!(count.get(), 2);

        subscription.dispose();
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn on_will_remove_listener() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let count = Rc::new(Cell::new(0));
        let counter = count.clone();
        let a = ds.add_rc(Emitter::<()>::with_options(EmitterOptions {
            on_will_remove_listener: Some(Rc::new(move |_| counter.set(counter.get() + 1))),
            ..EmitterOptions::default()
        }));

        assert_eq!(count.get(), 0);

        let mut subscription = ds.add(a.event().listen(|_| {}));
        assert_eq!(count.get(), 0);

        subscription.dispose();
        assert_eq!(count.get(), 1);

        subscription = ds.add(a.event().listen(|_| {}));
        assert_eq!(count.get(), 1);
        drop(subscription);
    }

    #[test]
    fn throwing_listener() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        errors::set_unexpected_error_handler(|_| {});

        let a = ds.add_rc(Emitter::<()>::new());
        let hit = Rc::new(Cell::new(false));
        ds.add(a.event().listen(|_| errors::throw(Error::new("9"))));
        let flag = hit.clone();
        ds.add(a.event().listen(move |_| flag.set(true)));
        a.fire(());
        assert!(hit.get());

        record_unexpected_errors();
    }

    #[test]
    fn throwing_listener_custom_handler() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        let all_error = shared::<String>();
        let errors_seen = all_error.clone();
        let a = ds.add_rc(Emitter::<()>::with_options(EmitterOptions {
            on_listener_error: Some(Rc::new(move |e| errors_seen.borrow_mut().push(e.message.clone()))),
            ..EmitterOptions::default()
        }));
        let hit = Rc::new(Cell::new(false));
        ds.add(a.event().listen(|_| errors::throw(Error::new("9"))));
        let flag = hit.clone();
        ds.add(a.event().listen(move |_| flag.set(true)));
        a.fire(());
        assert!(hit.get());
        assert_eq!(snapshot(&all_error), ["9"]);
    }

    #[test]
    fn throw_listener_leak_error() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        let store = DisposableStore::new();
        let all_error = shared::<Error>();
        let errors_seen = all_error.clone();

        let a = ds.add_rc(Emitter::<()>::with_options(EmitterOptions {
            on_listener_error: Some(Rc::new(move |e| errors_seen.borrow_mut().push(e.clone()))),
            leak_warning_threshold: Some(3),
            ..EmitterOptions::default()
        }));

        for _ in 0..11 {
            a.event().listen_in(|_| {}, &store);
        }

        assert_eq!(all_error.borrow().len(), 5);
        let all_error = snapshot(&all_error);
        let (rest, start) = all_error.split_last().expect("há erros");
        assert_eq!(rest.name, "ListenerRefusalError");

        for item in start {
            assert_eq!(item.name, "ListenerLeakError");
            assert!(ListenerLeakError::from_error(item).is_some());
        }

        store.dispose();
    }

    #[test]
    fn emitter_leak_warnings_track_only_active_listener_stacks() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let errors_seen = shared::<Error>();
        let sink = errors_seen.clone();
        let emitter = ds.add_rc(Emitter::<()>::with_options(EmitterOptions {
            leak_warning_threshold: Some(3),
            leak_warning_name: Some("test".to_owned()),
            on_listener_error: Some(Rc::new(move |error| sink.borrow_mut().push(error.clone()))),
            ..EmitterOptions::default()
        }));

        // Cada fecho abaixo é um ponto de chamada diferente, que é o que a pilha mede.
        let add_stack_a_listener = || emitter.event().listen(|_| {});
        let add_stack_b_listener = || emitter.event().listen(|_| {});
        let add_stack_c_listener = || emitter.event().listen(|_| {});

        emitter.set_size_for_testing(2);
        let stack_a_listeners: Vec<DisposableRef> = (0..3).map(|_| add_stack_a_listener()).collect();
        stack_a_listeners[0].dispose();
        let stack_b_listener = add_stack_b_listener();
        let stack_c_listener = add_stack_c_listener();

        stack_a_listeners[1..].iter().for_each(|listener| listener.dispose());
        stack_b_listener.dispose();
        stack_c_listener.dispose();
        emitter.set_size_for_testing(10);
        emitter.event().listen(|_| {});

        let summary: Vec<(String, Option<String>, bool)> = errors_seen
            .borrow()
            .iter()
            .map(|error| (error.name.clone(), ListenerLeakError::from_error(error).map(|leak| leak.details), error.stack.as_deref() == Some("UNKNOWN stack")))
            .collect();
        let expected = [
            ("ListenerLeakError", "[test] potential listener LEAK detected, having 3 listeners already. MOST frequent listener (1):", false),
            ("ListenerLeakError", "[test] potential listener LEAK detected, having 5 listeners already. MOST frequent listener (3):", false),
            ("ListenerLeakError", "[test] potential listener LEAK detected, having 6 listeners already. MOST frequent listener (2):", false),
            (
                "ListenerRefusalError",
                "[test] REFUSES to accept new listeners because it exceeded its threshold by far (10 vs 3). HINT: Stack shows most frequent listener (-1-times)",
                true,
            ),
        ];
        assert_eq!(summary, expected.map(|(name, details, unknown)| (name.to_owned(), Some(details.to_owned()), unknown)));
    }

    #[test]
    fn emitter_captures_global_leak_warning_configuration_at_construction() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let errors_seen = shared::<String>();
        let handler = || {
            let sink = errors_seen.clone();
            let f: Rc<dyn Fn(&Error)> = Rc::new(move |error| sink.borrow_mut().push(error.message.clone()));
            f
        };
        let restore_threshold = set_global_leak_warning_threshold(3);
        let monitored_emitter = ds.add_rc(Emitter::<()>::with_options(EmitterOptions {
            leak_warning_name: Some("captured".to_owned()),
            on_listener_error: Some(handler()),
            ..EmitterOptions::default()
        }));
        restore_threshold.dispose();

        let unmonitored_emitter = ds.add_rc(Emitter::<()>::with_options(EmitterOptions { on_listener_error: Some(handler()), ..EmitterOptions::default() }));
        let restore_threshold = set_global_leak_warning_threshold(3);
        let listeners = ds.add_rc(DisposableStore::new());
        let mut monitor_allocation = vec![monitored_emitter.has_leakage_monitor_for_testing()];
        for _ in 0..3 {
            monitored_emitter.event().listen_in(|_| {}, &listeners);
            unmonitored_emitter.event().listen_in(|_| {}, &listeners);
            monitor_allocation.push(monitored_emitter.has_leakage_monitor_for_testing());
        }
        restore_threshold.dispose();

        assert_eq!(snapshot(&errors_seen), ["[captured] potential listener LEAK detected, dominated"]);
        assert_eq!(monitor_allocation, [false, false, true, true]);
        assert!(!unmonitored_emitter.has_leakage_monitor_for_testing());
    }

    #[test]
    fn reusing_event_function_and_context() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let counter = Rc::new(Cell::new(0));
        let c = counter.clone();
        let listener: je_base::event::Listener<()> = Rc::new(move |_| c.set(c.get() + 1));

        let emitter = ds.add_rc(Emitter::<()>::new());
        let reg1 = emitter.event().listen_rc(listener.clone());
        let reg2 = emitter.event().listen_rc(listener);

        emitter.fire(());
        assert_eq!(counter.get(), 2);

        reg1.dispose();
        emitter.fire(());
        assert_eq!(counter.get(), 3);

        reg2.dispose();
        emitter.fire(());
        assert_eq!(counter.get(), 3);
    }

    #[test]
    fn debounce_emitter() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let store = ds.store();
        run_with_faked_timers(async move {
            let call_count = Rc::new(Cell::new(0));
            let sum = Rc::new(Cell::new(0));
            let calls = call_count.clone();
            let emitter = DebounceEmitter::<i32>::new(
                move |arr| {
                    calls.set(calls.get() + 1);
                    arr.into_iter().sum()
                },
                None,
            );

            let total = sum.clone();
            store.add(emitter.event().listen(move |e| total.set(*e)));

            let p = emitter.event().to_promise(None);

            emitter.fire(1);
            emitter.fire(2);

            p.await.expect("o evento dispara");

            assert_eq!(call_count.get(), 1);
            assert_eq!(sum.get(), 3);
        });
    }

    mod event_to_promise {
        use super::*;

        #[test]
        fn resolves_on_first_event() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let emitter = store.add_rc(Emitter::<i32>::new());
                let promise = emitter.event().to_promise(None);

                emitter.fire(42);
                let result = promise.await.expect("o evento dispara");

                assert_eq!(result, 42);
            });
        }

        #[test]
        fn disposes_listener_after_resolution() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let emitter = store.add_rc(Emitter::<i32>::new());
                let promise = emitter.event().to_promise(None);

                emitter.fire(1);
                promise.await.expect("o evento dispara");

                // O listener foi descartado: disparar de novo não afeta nada
                emitter.fire(2);
                assert!(!emitter.has_listeners());
            });
        }

        #[test]
        fn adds_to_disposable_store() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let outer = ds.store();
            run_with_faked_timers(async move {
                let emitter = outer.add_rc(Emitter::<i32>::new());
                let store = outer.add_rc(DisposableStore::new());
                let promise = emitter.event().to_promise(Some(Disposables::Store(store.clone())));

                assert_eq!(store.len(), 1);

                emitter.fire(42);
                promise.await.expect("o evento dispara");

                // Sai do store depois da resolução
                assert_eq!(store.len(), 0);
            });
        }

        #[test]
        fn adds_to_disposables_array() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let emitter = store.add_rc(Emitter::<i32>::new());
                let disposables = shared::<DisposableRef>();
                let promise = emitter.event().to_promise(Some(Disposables::Array(disposables.clone())));

                assert_eq!(disposables.borrow().len(), 1);

                emitter.fire(42);
                promise.await.expect("o evento dispara");

                // Sai do array depois da resolução
                assert_eq!(disposables.borrow().len(), 0);
            });
        }

        #[test]
        fn cancel_removes_from_disposable_store() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let emitter = ds.add_rc(Emitter::<i32>::new());
            let store = ds.add_rc(DisposableStore::new());
            let promise = emitter.event().to_promise(Some(Disposables::Store(store.clone())));

            assert_eq!(store.len(), 1);

            promise.cancel();

            // Sai do store depois do cancelamento
            assert_eq!(store.len(), 0);
        }

        #[test]
        fn cancel_removes_from_disposables_array() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let emitter = ds.add_rc(Emitter::<i32>::new());
            let disposables = shared::<DisposableRef>();
            let promise = emitter.event().to_promise(Some(Disposables::Array(disposables.clone())));

            assert_eq!(disposables.borrow().len(), 1);

            promise.cancel();

            // Sai do array depois do cancelamento
            assert_eq!(disposables.borrow().len(), 0);
        }

        #[test]
        fn cancel_does_not_resolve_promise() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let emitter = store.add_rc(Emitter::<i32>::new());
                let promise = emitter.event().to_promise(None);

                promise.cancel();
                emitter.fire(42);

                // A promessa não resolve depois do cancelamento
                let resolved = Rc::new(Cell::new(false));
                let flag = resolved.clone();
                promise.then(move |_| flag.set(true));

                timeout(10).await.expect("timeout");
                assert!(!resolved.get());
            });
        }
    }

    #[test]
    fn microtask_emitter() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let count = Rc::new(Cell::new(0));
        assert_eq!(count.get(), 0);
        let emitter = MicrotaskEmitter::<()>::new();
        let counter = count.clone();
        let listener = emitter.event().listen(move |_| counter.set(counter.get() + 1));
        emitter.fire(());
        assert_eq!(count.get(), 0);
        emitter.fire(());
        assert_eq!(count.get(), 0);

        let (seen_in_microtask, seen_in_timeout) = (Rc::new(Cell::new(-1)), Rc::new(Cell::new(-1)));
        runtime::with_faked_timers(|| {
            // Espera o fim do loop, então é a última coisa chamada
            let (counter, seen) = (count.clone(), seen_in_timeout.clone());
            runtime::set_timeout(move || seen.set(counter.get()), 0);
            let (counter, seen) = (count.clone(), seen_in_microtask.clone());
            runtime::queue_microtask(move || {
                seen.set(counter.get());
                counter.set(counter.get() + 1);
                listener.dispose();
            });
        });
        assert_eq!(seen_in_microtask.get(), 2);
        assert_eq!(seen_in_timeout.get(), 3);
    }

    #[test]
    fn emitter_in_order_delivery() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let a = ds.add_rc(Emitter::<String>::new());
        let listener2_events = shared::<String>();
        let (events, emitter) = (listener2_events.clone(), a.clone());
        ds.add(a.event().listen(move |event| {
            if event == "e1" {
                emitter.fire("e2".to_owned());
                // confere que todos os eventos foram entregues neste ponto
                assert_eq!(snapshot(&events), ["e1", "e2"]);
            }
        }));
        let events = listener2_events.clone();
        ds.add(a.event().listen(move |event| events.borrow_mut().push(event.clone())));
        a.fire("e1".to_owned());

        // confere que todos os eventos foram entregues em ordem
        assert_eq!(snapshot(&listener2_events), ["e1", "e2"]);
    }

    #[test]
    fn emitter_in_order_delivery_3x() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let a = ds.add_rc(Emitter::<String>::new());
        let listener2_events = shared::<String>();
        let (events, emitter) = (listener2_events.clone(), a.clone());
        ds.add(a.event().listen(move |event| {
            if event == "e2" {
                emitter.fire("e3".to_owned());
                // confere que todos os eventos foram entregues neste ponto
                assert_eq!(snapshot(&events), ["e1", "e2", "e3"]);
            }
        }));
        let (events, emitter) = (listener2_events.clone(), a.clone());
        ds.add(a.event().listen(move |event| {
            if event == "e1" {
                emitter.fire("e2".to_owned());
                // confere que todos os eventos foram entregues neste ponto
                assert_eq!(snapshot(&events), ["e1", "e2", "e3"]);
            }
        }));
        let events = listener2_events.clone();
        ds.add(a.event().listen(move |event| events.borrow_mut().push(event.clone())));
        a.fire("e1".to_owned());

        // confere que todos os eventos foram entregues em ordem
        assert_eq!(snapshot(&listener2_events), ["e1", "e2", "e3"]);
    }
}

mod async_emitter {
    use super::*;

    #[derive(Clone)]
    struct E {
        foo: bool,
        bar: i32,
    }

    #[test]
    fn event_has_wait_until_function() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        let emitter = AsyncEmitter::<E>::new();

        ds.add(emitter.event().listen(|e: &WaitUntilEvent<E>| {
            assert!(e.data.foo);
            assert_eq!(e.data.bar, 1);
            // `typeof e.waitUntil === 'function'`: o método existe; chamar com uma promessa pronta vale.
            e.wait_until(Promise::resolved(()));
        }));

        emitter.fire_async(E { foo: true, bar: 1 }, CancellationToken::None, None);
        emitter.dispose();
    }

    #[test]
    fn sequential_delivery() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let store = ds.store();
        run_with_faked_timers(async move {
            #[derive(Clone)]
            struct E {
                _foo: bool,
            }

            let global_state = Rc::new(Cell::new(0));
            let emitter = AsyncEmitter::<E>::new();

            let state = global_state.clone();
            store.add(emitter.event().listen(move |e| {
                let state = state.clone();
                e.wait_until(timeout(10).then(move |_| {
                    assert_eq!(state.get(), 0);
                    state.set(state.get() + 1);
                }));
            }));

            let state = global_state.clone();
            store.add(emitter.event().listen(move |e| {
                let state = state.clone();
                e.wait_until(timeout(1).then(move |_| {
                    assert_eq!(state.get(), 1);
                    state.set(state.get() + 1);
                }));
            }));

            emitter.fire_async(E { _foo: true }, CancellationToken::None, None).await.expect("fireAsync");
            assert_eq!(global_state.get(), 2);
            emitter.dispose();
        });
    }

    #[test]
    fn sequential_in_order_delivery() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let store = ds.store();
        run_with_faked_timers(async move {
            #[derive(Clone)]
            struct E {
                foo: i32,
            }
            let events = shared::<i32>();
            let done = Rc::new(Cell::new(false));
            let emitter = AsyncEmitter::<E>::new();

            // e1
            let (events1, done1, emitter1) = (events.clone(), done.clone(), emitter.clone());
            store.add(emitter.event().listen(move |e| {
                let (events, done, emitter, foo) = (events1.clone(), done1.clone(), emitter1.clone(), e.data.foo);
                e.wait_until(timeout(10).then_promise(move |_| {
                    let (finished, resolver) = Promise::new();
                    runtime::spawn_local(async move {
                        if foo == 1 {
                            emitter.fire_async(E { foo: 2 }, CancellationToken::None, None).await.expect("fireAsync");
                            assert_eq!(snapshot(&events), [1, 2]);
                            done.set(true);
                        }
                        resolver.resolve(());
                    });
                    finished
                }));
            }));

            // e2
            let events2 = events.clone();
            store.add(emitter.event().listen(move |e| {
                events2.borrow_mut().push(e.data.foo);
                e.wait_until(timeout(7));
            }));

            emitter.fire_async(E { foo: 1 }, CancellationToken::None, None).await.expect("fireAsync");
            assert!(done.get());
            emitter.dispose();
        });
    }

    #[test]
    fn catch_errors() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let store = ds.store();
        errors::set_unexpected_error_handler(|_| {});
        run_with_faked_timers(async move {
            #[derive(Clone)]
            struct E {
                _foo: bool,
            }

            let global_state = Rc::new(Cell::new(0));
            let emitter = AsyncEmitter::<E>::new();

            let state = global_state.clone();
            store.add(emitter.event().listen(move |e| {
                state.set(state.get() + 1);
                e.wait_until(Promise::rejected(Error::new("")));
            }));

            let state = global_state.clone();
            store.add(emitter.event().listen(move |e| {
                state.set(state.get() + 1);
                e.wait_until(timeout(10));
                let state = state.clone();
                // Vários `waitUntil` são suportados e todos são esperados
                e.wait_until(timeout(20).then(move |_| state.set(state.get() + 1)));
            }));

            emitter.fire_async(E { _foo: true }, CancellationToken::None, None).await.expect("fireAsync");
            assert_eq!(global_state.get(), 3);
            emitter.dispose();
        });
        record_unexpected_errors();
    }
}

mod pausable_emitter {
    use super::*;

    #[test]
    fn basic() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let data = shared::<i32>();
        let emitter = ds.add_rc(PauseableEmitter::<i32>::new());

        let sink = data.clone();
        ds.add(emitter.event().listen(move |e| sink.borrow_mut().push(*e)));
        emitter.fire(1);
        emitter.fire(2);

        assert_eq!(snapshot(&data), [1, 2]);
    }

    #[test]
    fn pause_resume_no_merge() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let data = shared::<i32>();
        let emitter = ds.add_rc(PauseableEmitter::<i32>::new());

        let sink = data.clone();
        ds.add(emitter.event().listen(move |e| sink.borrow_mut().push(*e)));
        emitter.fire(1);
        emitter.fire(2);
        assert_eq!(snapshot(&data), [1, 2]);

        emitter.pause();
        emitter.fire(3);
        emitter.fire(4);
        assert_eq!(snapshot(&data), [1, 2]);

        emitter.resume();
        assert_eq!(snapshot(&data), [1, 2, 3, 4]);
        emitter.fire(5);
        assert_eq!(snapshot(&data), [1, 2, 3, 4, 5]);
    }

    #[test]
    fn pause_resume_merge() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let data = shared::<i32>();
        let emitter = ds.add_rc(PauseableEmitter::<i32>::with_options(EmitterOptions::default(), Some(Rc::new(|a: Vec<i32>| a.iter().sum()))));

        let sink = data.clone();
        ds.add(emitter.event().listen(move |e| sink.borrow_mut().push(*e)));
        emitter.fire(1);
        emitter.fire(2);
        assert_eq!(snapshot(&data), [1, 2]);

        emitter.pause();
        emitter.fire(3);
        emitter.fire(4);
        assert_eq!(snapshot(&data), [1, 2]);

        emitter.resume();
        assert_eq!(snapshot(&data), [1, 2, 7]);

        emitter.fire(5);
        assert_eq!(snapshot(&data), [1, 2, 7, 5]);
    }

    #[test]
    fn double_pause_resume() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let data = shared::<i32>();
        let emitter = ds.add_rc(PauseableEmitter::<i32>::new());

        let sink = data.clone();
        ds.add(emitter.event().listen(move |e| sink.borrow_mut().push(*e)));
        emitter.fire(1);
        emitter.fire(2);
        assert_eq!(snapshot(&data), [1, 2]);

        emitter.pause();
        emitter.pause();
        emitter.fire(3);
        emitter.fire(4);
        assert_eq!(snapshot(&data), [1, 2]);

        emitter.resume();
        assert_eq!(snapshot(&data), [1, 2]);

        emitter.resume();
        assert_eq!(snapshot(&data), [1, 2, 3, 4]);

        emitter.resume();
        assert_eq!(snapshot(&data), [1, 2, 3, 4]);
    }

    #[test]
    fn resume_no_pause() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let data = shared::<i32>();
        let emitter = ds.add_rc(PauseableEmitter::<i32>::new());

        let sink = data.clone();
        ds.add(emitter.event().listen(move |e| sink.borrow_mut().push(*e)));
        emitter.fire(1);
        emitter.fire(2);
        assert_eq!(snapshot(&data), [1, 2]);

        emitter.resume();
        emitter.fire(3);
        assert_eq!(snapshot(&data), [1, 2, 3]);
    }

    #[test]
    fn nested_pause() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let data = shared::<i32>();
        let emitter = ds.add_rc(PauseableEmitter::<i32>::new());

        let once = Rc::new(Cell::new(true));
        let (sink, e, once_flag) = (data.clone(), emitter.clone(), once.clone());
        ds.add(emitter.event().listen(move |value| {
            sink.borrow_mut().push(*value);

            if once_flag.get() {
                e.pause();
                once_flag.set(false);
            }
        }));
        let sink = data.clone();
        ds.add(emitter.event().listen(move |value| sink.borrow_mut().push(*value)));

        emitter.pause();
        emitter.fire(1);
        emitter.fire(2);
        assert_eq!(snapshot(&data), Vec::<i32>::new());

        emitter.resume();
        assert_eq!(snapshot(&data), [1, 1]); // pausou depois do primeiro evento

        emitter.resume();
        assert_eq!(snapshot(&data), [1, 1, 2, 2]); // o evento que sobrou foi entregue

        emitter.fire(3);
        assert_eq!(snapshot(&data), [1, 1, 2, 2, 3, 3]);
    }

    #[test]
    fn empty_pause_with_merge() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let data = shared::<i32>();
        let emitter = ds.add_rc(PauseableEmitter::<i32>::with_options(EmitterOptions::default(), Some(Rc::new(|a: Vec<i32>| a[0]))));
        let sink = data.clone();
        ds.add(emitter.event().listen(move |_| sink.borrow_mut().push(1)));

        emitter.pause();
        emitter.resume();
        assert_eq!(snapshot(&data), Vec::<i32>::new());
    }
}

mod event_utils {
    use super::*;

    mod event_bufferer {
        use super::*;

        #[test]
        fn should_not_buffer_when_not_wrapped() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let bufferer = EventBufferer::new();
            let counter = EventCounter::new();
            let emitter = ds.add_rc(Emitter::<()>::new());
            let event = bufferer.wrap_event(&emitter.event());
            let c = counter.clone();
            let listener = event.listen(move |_| c.on_event());

            assert_eq!(counter.count.get(), 0);
            emitter.fire(());
            assert_eq!(counter.count.get(), 1);
            emitter.fire(());
            assert_eq!(counter.count.get(), 2);
            emitter.fire(());
            assert_eq!(counter.count.get(), 3);

            listener.dispose();
        }

        #[test]
        fn should_buffer_when_wrapped() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let bufferer = EventBufferer::new();
            let counter = EventCounter::new();
            let emitter = ds.add_rc(Emitter::<()>::new());
            let event = bufferer.wrap_event(&emitter.event());
            let c = counter.clone();
            let listener = event.listen(move |_| c.on_event());

            assert_eq!(counter.count.get(), 0);
            emitter.fire(());
            assert_eq!(counter.count.get(), 1);

            bufferer.buffer_events(|| {
                emitter.fire(());
                assert_eq!(counter.count.get(), 1);
                emitter.fire(());
                assert_eq!(counter.count.get(), 1);
            });

            assert_eq!(counter.count.get(), 3);
            emitter.fire(());
            assert_eq!(counter.count.get(), 4);

            listener.dispose();
        }

        #[test]
        fn once() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let emitter = ds.add_rc(Emitter::<()>::new());

            let (counter1, counter2, counter3) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));

            let c = counter1.clone();
            let listener1 = emitter.event().listen(move |_| c.set(c.get() + 1));
            let c = counter2.clone();
            let listener2 = emitter.event().once().listen(move |_| c.set(c.get() + 1));
            let c = counter3.clone();
            let listener3 = emitter.event().once().listen(move |_| c.set(c.get() + 1));

            assert_eq!(counter1.get(), 0);
            assert_eq!(counter2.get(), 0);
            assert_eq!(counter3.get(), 0);

            listener3.dispose();
            emitter.fire(());
            assert_eq!(counter1.get(), 1);
            assert_eq!(counter2.get(), 1);
            assert_eq!(counter3.get(), 0);

            emitter.fire(());
            assert_eq!(counter1.get(), 2);
            assert_eq!(counter2.get(), 1);
            assert_eq!(counter3.get(), 0);

            listener1.dispose();
            listener2.dispose();
        }
    }

    mod buffer {
        use super::*;

        #[test]
        fn should_buffer_events() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let result = shared::<i32>();
            let emitter = ds.add_rc(Emitter::<i32>::new());
            let event = emitter.event();
            let buffered_event = event.buffer("test", false, Vec::new(), None);

            emitter.fire(1);
            emitter.fire(2);
            emitter.fire(3);
            assert_eq!(snapshot(&result), Vec::<i32>::new());

            let sink = result.clone();
            let listener = buffered_event.listen(move |num| sink.borrow_mut().push(*num));
            assert_eq!(snapshot(&result), [1, 2, 3]);

            emitter.fire(4);
            assert_eq!(snapshot(&result), [1, 2, 3, 4]);

            listener.dispose();
            emitter.fire(5);
            assert_eq!(snapshot(&result), [1, 2, 3, 4]);
        }

        #[test]
        fn should_buffer_events_on_next_tick() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let result = shared::<i32>();
                let emitter = store.add_rc(Emitter::<i32>::new());
                let event = emitter.event();
                let buffered_event = event.buffer("test", true, Vec::new(), None);

                emitter.fire(1);
                emitter.fire(2);
                emitter.fire(3);
                assert_eq!(snapshot(&result), Vec::<i32>::new());

                let sink = result.clone();
                let listener = buffered_event.listen(move |num| sink.borrow_mut().push(*num));
                assert_eq!(snapshot(&result), Vec::<i32>::new());

                timeout(10).await.expect("timeout");
                emitter.fire(4);
                assert_eq!(snapshot(&result), [1, 2, 3, 4]);
                listener.dispose();
                emitter.fire(5);
                assert_eq!(snapshot(&result), [1, 2, 3, 4]);
            });
        }

        #[test]
        fn should_fire_initial_buffer_events() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let result = shared::<i32>();
            let emitter = ds.add_rc(Emitter::<i32>::new());
            let event = emitter.event();
            let buffered_event = event.buffer("test", false, vec![-2, -1, 0], None);

            emitter.fire(1);
            emitter.fire(2);
            emitter.fire(3);
            assert_eq!(snapshot(&result), Vec::<i32>::new());

            let sink = result.clone();
            ds.add(buffered_event.listen(move |num| sink.borrow_mut().push(*num)));
            assert_eq!(snapshot(&result), [-2, -1, 0, 1, 2, 3]);
        }
    }

    mod event_multiplexer {
        use super::*;

        fn collecting(result: &Shared<i32>) -> impl Fn(&i32) + 'static {
            let sink = result.clone();
            move |r| sink.borrow_mut().push(*r)
        }

        #[test]
        fn works() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let result = shared::<i32>();
            let m = EventMultiplexer::<i32>::new();
            ds.add(m.event().listen(collecting(&result)));

            let e1 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e1.event()));

            assert_eq!(snapshot(&result), Vec::<i32>::new());

            e1.fire(0);
            assert_eq!(snapshot(&result), [0]);
        }

        #[test]
        fn multiplexer_dispose_works() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let result = shared::<i32>();
            let m = EventMultiplexer::<i32>::new();
            ds.add(m.event().listen(collecting(&result)));

            let e1 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e1.event()));

            assert_eq!(snapshot(&result), Vec::<i32>::new());

            e1.fire(0);
            assert_eq!(snapshot(&result), [0]);

            m.dispose();
            assert_eq!(snapshot(&result), [0]);

            e1.fire(0);
            assert_eq!(snapshot(&result), [0]);
        }

        #[test]
        fn event_dispose_works() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let result = shared::<i32>();
            let m = EventMultiplexer::<i32>::new();
            ds.add(m.event().listen(collecting(&result)));

            let e1 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e1.event()));

            assert_eq!(snapshot(&result), Vec::<i32>::new());

            e1.fire(0);
            assert_eq!(snapshot(&result), [0]);

            e1.dispose();
            assert_eq!(snapshot(&result), [0]);

            e1.fire(0);
            assert_eq!(snapshot(&result), [0]);
        }

        #[test]
        fn mutliplexer_event_dispose_works() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let result = shared::<i32>();
            let m = EventMultiplexer::<i32>::new();
            ds.add(m.event().listen(collecting(&result)));

            let e1 = ds.add_rc(Emitter::<i32>::new());
            let l1 = m.add(e1.event());

            assert_eq!(snapshot(&result), Vec::<i32>::new());

            e1.fire(0);
            assert_eq!(snapshot(&result), [0]);

            l1.dispose();
            assert_eq!(snapshot(&result), [0]);

            e1.fire(0);
            assert_eq!(snapshot(&result), [0]);
        }

        #[test]
        fn hot_start_works() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let result = shared::<i32>();
            let m = EventMultiplexer::<i32>::new();
            ds.add(m.event().listen(collecting(&result)));

            let e1 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e1.event()));
            let e2 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e2.event()));
            let e3 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e3.event()));

            e1.fire(1);
            e2.fire(2);
            e3.fire(3);
            assert_eq!(snapshot(&result), [1, 2, 3]);
        }

        #[test]
        fn cold_start_works() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let result = shared::<i32>();
            let m = EventMultiplexer::<i32>::new();

            let e1 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e1.event()));
            let e2 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e2.event()));
            let e3 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e3.event()));

            ds.add(m.event().listen(collecting(&result)));

            e1.fire(1);
            e2.fire(2);
            e3.fire(3);
            assert_eq!(snapshot(&result), [1, 2, 3]);
        }

        #[test]
        fn late_add_works() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let result = shared::<i32>();
            let m = EventMultiplexer::<i32>::new();

            let e1 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e1.event()));
            let e2 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e2.event()));

            ds.add(m.event().listen(collecting(&result)));

            e1.fire(1);
            e2.fire(2);

            let e3 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e3.event()));
            e3.fire(3);

            assert_eq!(snapshot(&result), [1, 2, 3]);
        }

        #[test]
        fn add_dispose_works() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let result = shared::<i32>();
            let m = EventMultiplexer::<i32>::new();

            let e1 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e1.event()));
            let e2 = ds.add_rc(Emitter::<i32>::new());
            ds.add(m.add(e2.event()));

            ds.add(m.event().listen(collecting(&result)));

            e1.fire(1);
            e2.fire(2);

            let e3 = ds.add_rc(Emitter::<i32>::new());
            let l3 = m.add(e3.event());
            e3.fire(3);
            assert_eq!(snapshot(&result), [1, 2, 3]);

            l3.dispose();
            e3.fire(4);
            assert_eq!(snapshot(&result), [1, 2, 3]);

            e2.fire(4);
            e1.fire(5);
            assert_eq!(snapshot(&result), [1, 2, 3, 4, 5]);
        }
    }

    mod dynamic_list_event_multiplexer {
        use super::*;

        struct TestItem {
            on_test_event_emitter: Rc<Emitter<i32>>,
        }

        impl TestItem {
            fn new(ds: &common::LeakGuard) -> ById<TestItem> {
                ById(Rc::new(Self { on_test_event_emitter: ds.add_rc(Emitter::new()) }))
            }

            fn on_test_event(&self) -> Event<i32> {
                self.on_test_event_emitter.event()
            }
        }

        /// O `setup` (e o `teardown(() => m.dispose())`, que roda antes do `teardown` do `ds`).
        struct Fixture {
            m: DynamicListEventMultiplexer<ById<TestItem>, i32>,
            add_emitter: Rc<Emitter<ById<TestItem>>>,
            remove_emitter: Rc<Emitter<ById<TestItem>>>,
            recorded_events: Shared<i32>,
            items: Vec<ById<TestItem>>,
            ds: common::LeakGuard,
        }

        impl Fixture {
            fn new() -> Self {
                let ds = ensure_no_disposables_are_leaked_in_test_suite();
                let add_emitter = ds.add_rc(Emitter::<ById<TestItem>>::new());
                let remove_emitter = ds.add_rc(Emitter::<ById<TestItem>>::new());
                let items = vec![TestItem::new(&ds), TestItem::new(&ds)];
                for (i, item) in items.iter().enumerate() {
                    ds.add(item.0.on_test_event().listen(move |e| drop(format!("{i}:{e}"))));
                }
                let m = DynamicListEventMultiplexer::new(items.clone(), &add_emitter.event(), &remove_emitter.event(), |e: &ById<TestItem>| e.0.on_test_event());
                let recorded_events = shared::<i32>();
                let sink = recorded_events.clone();
                ds.add(m.event.listen(move |e| sink.borrow_mut().push(*e)));
                recorded_events.borrow_mut().clear();
                Self { m, add_emitter, remove_emitter, recorded_events, items, ds }
            }
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                self.m.dispose();
            }
        }

        #[test]
        fn should_fire_events_for_initial_items() {
            let f = Fixture::new();
            f.items[0].0.on_test_event_emitter.fire(1);
            f.items[1].0.on_test_event_emitter.fire(2);
            f.items[0].0.on_test_event_emitter.fire(3);
            f.items[1].0.on_test_event_emitter.fire(4);
            assert_eq!(snapshot(&f.recorded_events), [1, 2, 3, 4]);
        }

        #[test]
        fn should_fire_events_for_added_items() {
            let f = Fixture::new();
            let added_item = TestItem::new(&f.ds);
            f.add_emitter.fire(added_item.clone());
            added_item.0.on_test_event_emitter.fire(1);
            f.items[0].0.on_test_event_emitter.fire(2);
            f.items[1].0.on_test_event_emitter.fire(3);
            added_item.0.on_test_event_emitter.fire(4);
            assert_eq!(snapshot(&f.recorded_events), [1, 2, 3, 4]);
        }

        #[test]
        fn should_not_fire_events_for_removed_items() {
            let f = Fixture::new();
            f.remove_emitter.fire(f.items[0].clone());
            f.items[0].0.on_test_event_emitter.fire(1);
            f.items[1].0.on_test_event_emitter.fire(2);
            f.items[0].0.on_test_event_emitter.fire(3);
            f.items[1].0.on_test_event_emitter.fire(4);
            assert_eq!(snapshot(&f.recorded_events), [2, 4]);
        }
    }

    #[test]
    fn latch() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let emitter = ds.add_rc(Emitter::<i32>::new());
        let event = emitter.event().latch(None);

        let result = shared::<i32>();
        let sink = result.clone();
        let listener = ds.add(event.listen(move |num| sink.borrow_mut().push(*num)));

        assert_eq!(snapshot(&result), Vec::<i32>::new());

        emitter.fire(1);
        assert_eq!(snapshot(&result), [1]);

        emitter.fire(2);
        assert_eq!(snapshot(&result), [1, 2]);

        emitter.fire(2);
        assert_eq!(snapshot(&result), [1, 2]);

        emitter.fire(1);
        assert_eq!(snapshot(&result), [1, 2, 1]);

        emitter.fire(1);
        assert_eq!(snapshot(&result), [1, 2, 1]);

        emitter.fire(3);
        assert_eq!(snapshot(&result), [1, 2, 1, 3]);

        emitter.fire(3);
        assert_eq!(snapshot(&result), [1, 2, 1, 3]);

        emitter.fire(3);
        assert_eq!(snapshot(&result), [1, 2, 1, 3]);

        listener.dispose();
    }

    #[test]
    fn dispose_is_reentrant() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let emitter = ds.add_rc(Emitter::<i32>::with_options(EmitterOptions {
            on_did_remove_last_listener: Some(Rc::new(|emitter: &Emitter<i32>| emitter.dispose())),
            ..EmitterOptions::default()
        }));

        let listener = emitter.event().listen(|_| {});
        listener.dispose(); // não deve quebrar
    }

    mod relay {
        use super::*;

        #[test]
        fn should_input_work() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let e1 = ds.add_rc(Emitter::<i32>::new());
            let e2 = ds.add_rc(Emitter::<i32>::new());
            let relay = Relay::<i32>::new();

            let result = shared::<i32>();
            let sink = result.clone();
            let subscription = relay.event().listen(move |num| sink.borrow_mut().push(*num));

            e1.fire(1);
            assert_eq!(snapshot(&result), Vec::<i32>::new());

            relay.set_input(e1.event());
            e1.fire(2);
            assert_eq!(snapshot(&result), [2]);

            relay.set_input(e2.event());
            e1.fire(3);
            e2.fire(4);
            assert_eq!(snapshot(&result), [2, 4]);

            subscription.dispose();
            e1.fire(5);
            e2.fire(6);
            assert_eq!(snapshot(&result), [2, 4]);
        }

        #[test]
        fn should_relay_dispose_work() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let e1 = ds.add_rc(Emitter::<i32>::new());
            let e2 = ds.add_rc(Emitter::<i32>::new());
            let relay = Relay::<i32>::new();

            let result = shared::<i32>();
            let sink = result.clone();
            ds.add(relay.event().listen(move |num| sink.borrow_mut().push(*num)));

            e1.fire(1);
            assert_eq!(snapshot(&result), Vec::<i32>::new());

            relay.set_input(e1.event());
            e1.fire(2);
            assert_eq!(snapshot(&result), [2]);

            relay.set_input(e2.event());
            e1.fire(3);
            e2.fire(4);
            assert_eq!(snapshot(&result), [2, 4]);

            relay.dispose();
            e1.fire(5);
            e2.fire(6);
            assert_eq!(snapshot(&result), [2, 4]);
        }
    }

    mod accumulate {
        use super::*;

        #[test]
        fn should_not_fire_after_a_listener_is_disposed_with_undefined_or_empty() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let event_emitter = store.add_rc(Emitter::<i32>::new());
                let event = event_emitter.event();
                let accumulated = event.accumulate(Delay::Millis(0), None, None);

                let (calls1, calls2) = (shared::<Vec<i32>>(), shared::<Vec<i32>>());
                let sink = calls1.clone();
                let listener1 = store.add(accumulated.listen(move |e| sink.borrow_mut().push(e.clone())));
                let sink = calls2.clone();
                store.add(accumulated.listen(move |e| sink.borrow_mut().push(e.clone())));

                event_emitter.fire(1);
                timeout(1).await.expect("timeout");
                assert_eq!(snapshot(&calls1), [vec![1]]);
                assert_eq!(snapshot(&calls2), [vec![1]]);

                listener1.dispose();
                timeout(1).await.expect("timeout");
                assert_eq!(snapshot(&calls1), [vec![1]]);
                assert_eq!(snapshot(&calls2), [vec![1]], "should not fire after a listener is disposed with undefined or []");
            });
        }

        #[test]
        fn should_accumulate_a_single_event() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let event_emitter = store.add_rc(Emitter::<i32>::new());
                let event = event_emitter.event();
                let accumulated = event.accumulate(Delay::Millis(0), None, None);

                let (promise, resolver) = Promise::<Vec<i32>>::new();
                store.add(accumulated.listen(move |r| resolver.resolve(r.clone())));
                event_emitter.fire(1);
                let results1 = promise.await.expect("acumulou");
                assert_eq!(results1, [1]);

                let (promise, resolver) = Promise::<Vec<i32>>::new();
                store.add(accumulated.listen(move |r| resolver.resolve(r.clone())));
                event_emitter.fire(2);
                let results2 = promise.await.expect("acumulou");
                assert_eq!(results2, [2]);
            });
        }

        #[test]
        fn should_accumulate_multiple_events() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let event_emitter = store.add_rc(Emitter::<i32>::new());
                let event = event_emitter.event();
                let accumulated = event.accumulate(Delay::Millis(0), None, None);

                let (promise, resolver) = Promise::<Vec<i32>>::new();
                store.add(accumulated.listen(move |r| resolver.resolve(r.clone())));
                event_emitter.fire(1);
                event_emitter.fire(2);
                event_emitter.fire(3);
                let results1 = promise.await.expect("acumulou");
                assert_eq!(results1, [1, 2, 3]);

                let (promise, resolver) = Promise::<Vec<i32>>::new();
                store.add(accumulated.listen(move |r| resolver.resolve(r.clone())));
                for n in 4..=8 {
                    event_emitter.fire(n);
                }
                let results2 = promise.await.expect("acumulou");
                assert_eq!(results2, [4, 5, 6, 7, 8]);
            });
        }
    }

    mod debounce {
        use super::*;

        fn merge_keys(prev: Option<Vec<String>>, cur: &String) -> Vec<String> {
            match prev {
                None => vec![cur.clone()],
                Some(mut prev) => {
                    if !prev.contains(cur) {
                        prev.push(cur.clone());
                    }
                    prev
                }
            }
        }

        /// Os dois testes `simple` e `microtask` só diferem no atraso.
        fn debounced_keys(delay: Delay) {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let doc = ds.add_rc(Document3::new());

            let on_doc_did_change = doc.on_did_change().debounce(merge_keys, DebounceOptions { delay, ..DebounceOptions::default() }, None);

            let seen = shared::<Vec<String>>();
            let (sink, d) = (seen.clone(), doc.clone());
            ds.add(on_doc_did_change.listen(move |keys| {
                sink.borrow_mut().push(keys.clone());
                if sink.borrow().len() == 1 {
                    d.set_text("4");
                    // `keys` continua sendo o array do primeiro disparo
                    assert_eq!(*keys, ["1", "2", "3"]);
                }
            }));

            doc.set_text("1");
            doc.set_text("2");
            doc.set_text("3");

            runtime::with_faked_timers(|| {});
            assert_eq!(snapshot(&seen), [vec!["1", "2", "3"], vec!["4"]]);
        }

        #[test]
        fn simple() {
            debounced_keys(Delay::Millis(10));
        }

        #[test]
        fn microtask() {
            debounced_keys(Delay::Microtask);
        }

        #[test]
        fn leading() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let emitter = store.add_rc(Emitter::<()>::new());
                let debounced = emitter.event().debounce(|_, e| *e, DebounceOptions { delay: Delay::Millis(0), leading: true, ..DebounceOptions::default() }, None);

                let calls = Rc::new(Cell::new(0));
                let counter = calls.clone();
                store.add(debounced.listen(move |_| counter.set(counter.get() + 1)));

                // Se o evento de origem dispara uma vez, o debounced (na borda de subida) dispara só uma vez
                emitter.fire(());

                timeout(1).await.expect("timeout");
                assert_eq!(calls.get(), 1);
            });
        }

        #[test]
        fn leading_2() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let emitter = store.add_rc(Emitter::<()>::new());
                let debounced = emitter.event().debounce(|_, e| *e, DebounceOptions { delay: Delay::Millis(0), leading: true, ..DebounceOptions::default() }, None);

                let calls = Rc::new(Cell::new(0));
                let counter = calls.clone();
                store.add(debounced.listen(move |_| counter.set(counter.get() + 1)));

                // Se o evento de origem dispara várias vezes, o debounced (na borda de subida) dispara duas vezes
                emitter.fire(());
                emitter.fire(());
                emitter.fire(());
                timeout(1).await.expect("timeout");
                assert_eq!(calls.get(), 2);
            });
        }

        #[test]
        fn leading_reset() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let emitter = store.add_rc(Emitter::<i32>::new());
                let debounced =
                    emitter.event().debounce(|l: Option<i32>, _| l.map_or(1, |l| l + 1), DebounceOptions { delay: Delay::Millis(0), leading: true, ..DebounceOptions::default() }, None);

                let calls = shared::<i32>();
                let sink = calls.clone();
                store.add(debounced.listen(move |e| sink.borrow_mut().push(*e)));

                emitter.fire(1);
                emitter.fire(1);

                timeout(1).await.expect("timeout");
                assert_eq!(snapshot(&calls), [1, 1]);
            });
        }

        #[test]
        fn should_not_flush_events_when_a_listener_is_disposed() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let emitter = store.add_rc(Emitter::<i32>::new());
                let debounced = emitter.event().debounce(|l: Option<i32>, _| l.map_or(1, |l| l + 1), DebounceOptions { delay: Delay::Millis(0), ..DebounceOptions::default() }, None);

                let calls = shared::<i32>();
                let sink = calls.clone();
                let listener = store.add(debounced.listen(move |e| sink.borrow_mut().push(*e)));

                emitter.fire(1);
                listener.dispose();

                emitter.fire(1);

                timeout(1).await.expect("timeout");
                assert_eq!(snapshot(&calls), Vec::<i32>::new());
            });
        }

        #[test]
        fn flush_on_listener_remove_should_flush_events_when_a_listener_is_disposed() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let emitter = store.add_rc(Emitter::<i32>::new());
                let debounced = emitter.event().debounce(
                    |l: Option<i32>, _| l.map_or(1, |l| l + 1),
                    DebounceOptions { delay: Delay::Millis(0), flush_on_listener_remove: true, ..DebounceOptions::default() },
                    None,
                );

                let calls = shared::<i32>();
                let sink = calls.clone();
                let listener = store.add(debounced.listen(move |e| sink.borrow_mut().push(*e)));

                emitter.fire(1);
                listener.dispose();

                emitter.fire(1);

                timeout(1).await.expect("timeout");
                assert_eq!(snapshot(&calls), [1], "should fire with the first event, not the second (after listener dispose)");
            });
        }

        #[test]
        fn should_flush_events_when_the_emitter_is_disposed() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let emitter = store.add_rc(Emitter::<i32>::new());
                let debounced = emitter.event().debounce(|l: Option<i32>, _| l.map_or(1, |l| l + 1), DebounceOptions { delay: Delay::Millis(0), ..DebounceOptions::default() }, None);

                let calls = shared::<i32>();
                let sink = calls.clone();
                store.add(debounced.listen(move |e| sink.borrow_mut().push(*e)));

                emitter.fire(1);
                emitter.dispose();

                timeout(1).await.expect("timeout");
                assert_eq!(snapshot(&calls), [1]);
            });
        }
    }

    mod throttle {
        use super::*;

        fn count_merge(last: Option<i32>, _event: &i32) -> i32 {
            last.map_or(1, |l| l + 1)
        }

        fn options(delay: u64, leading: bool, trailing: bool) -> ThrottleOptions {
            ThrottleOptions { delay: Delay::Millis(delay), leading, trailing, ..ThrottleOptions::default() }
        }

        /// O preâmbulo comum: emitter, evento com throttle e a lista das chamadas.
        fn setup(
            store: &Rc<DisposableStore>,
            merge: impl Fn(Option<i32>, &i32) -> i32 + 'static,
            options: ThrottleOptions,
        ) -> (Rc<Emitter<i32>>, Shared<i32>, Event<i32>) {
            let emitter = store.add_rc(Emitter::<i32>::new());
            let throttled = emitter.event().throttle(merge, options, None);
            let calls = shared::<i32>();
            let sink = calls.clone();
            store.add(throttled.listen(move |e| sink.borrow_mut().push(*e)));
            (emitter, calls, throttled)
        }

        #[test]
        fn leading_only() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let (emitter, calls, _) = setup(&store, count_merge, options(10, true, false));

                // O primeiro evento dispara na hora
                emitter.fire(1);
                assert_eq!(snapshot(&calls), [1]);

                // Os eventos seguintes durante o período de throttle são ignorados
                emitter.fire(2);
                emitter.fire(3);
                assert_eq!(snapshot(&calls), [1]);

                // Espera o fim do período
                timeout(15).await.expect("timeout");
                assert_eq!(snapshot(&calls), [1], "no trailing edge fire with trailing=false");

                // Depois do período, o próximo evento dispara na hora
                emitter.fire(4);
                assert_eq!(snapshot(&calls), [1, 1]);
            });
        }

        #[test]
        fn trailing_only() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let (emitter, calls, _) = setup(&store, count_merge, options(10, false, true));

                // O primeiro evento não dispara na hora
                emitter.fire(1);
                assert_eq!(snapshot(&calls), Vec::<i32>::new());

                // Vários eventos durante o período
                emitter.fire(2);
                emitter.fire(3);
                assert_eq!(snapshot(&calls), Vec::<i32>::new());

                // Espera o período: dispara com o valor acumulado
                timeout(15).await.expect("timeout");
                assert_eq!(snapshot(&calls), [3]);

                // Eventos novos começam um período novo
                emitter.fire(4);
                emitter.fire(5);
                assert_eq!(snapshot(&calls), [3]);

                timeout(15).await.expect("timeout");
                assert_eq!(snapshot(&calls), [3, 2]);
            });
        }

        #[test]
        fn both_leading_and_trailing() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let (emitter, calls, _) = setup(&store, count_merge, options(10, true, true));

                // O primeiro evento dispara na hora (leading)
                emitter.fire(1);
                assert_eq!(snapshot(&calls), [1]);

                // Os eventos do período são acumulados
                emitter.fire(2);
                emitter.fire(3);
                assert_eq!(snapshot(&calls), [1]);

                // Espera o período: dispara a borda de descida com o valor acumulado
                timeout(15).await.expect("timeout");
                assert_eq!(snapshot(&calls), [1, 2]);
            });
        }

        #[test]
        fn only_leading_edge_if_no_subsequent_events() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let (emitter, calls, _) = setup(&store, count_merge, options(10, true, true));

                // Um evento só dispara na hora (leading)
                emitter.fire(1);
                assert_eq!(snapshot(&calls), [1]);

                // Nenhum evento a mais durante o período
                timeout(15).await.expect("timeout");
                // Não dispara a borda de descida porque não houve mais eventos
                assert_eq!(snapshot(&calls), [1]);
            });
        }

        #[test]
        fn microtask_delay() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let emitter = ds.add_rc(Emitter::<i32>::new());
            let throttled = emitter.event().throttle(count_merge, ThrottleOptions { delay: Delay::Microtask, ..ThrottleOptions::default() }, None);

            let calls = shared::<i32>();
            let sink = calls.clone();
            ds.add(throttled.listen(move |e| sink.borrow_mut().push(*e)));

            // O primeiro evento dispara na hora (leading por padrão)
            emitter.fire(1);
            assert_eq!(snapshot(&calls), [1]);

            // Eventos durante a microtarefa
            emitter.fire(2);
            emitter.fire(3);
            assert_eq!(snapshot(&calls), [1]);

            // Confere depois da microtarefa
            let after_microtask = shared::<i32>();
            let (seen, sink) = (calls.clone(), after_microtask.clone());
            runtime::queue_microtask(move || *sink.borrow_mut() = snapshot(&seen));
            runtime::run_microtasks();
            // Deve ter disparado a borda de descida
            assert_eq!(snapshot(&after_microtask), [1, 2]);
        }

        #[test]
        fn merge_function_accumulates_values() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let (emitter, calls, _) = setup(&store, |last, cur| last.unwrap_or(0) + cur, options(10, true, true));

                // O primeiro evento dispara na hora com o valor 1
                emitter.fire(1);
                assert_eq!(snapshot(&calls), [1]);

                // Acumula mais valores: 2 + 3 = 5
                emitter.fire(2);
                emitter.fire(3);
                assert_eq!(snapshot(&calls), [1]);

                timeout(15).await.expect("timeout");
                // A borda de descida dispara com a soma acumulada
                assert_eq!(snapshot(&calls), [1, 5]);
            });
        }

        #[test]
        fn rapid_consecutive_throttle_periods() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let (emitter, calls, _) = setup(&store, |_, e| *e, options(10, true, true));

                // Período 1
                emitter.fire(1);
                emitter.fire(2);
                assert_eq!(snapshot(&calls), [1]);

                timeout(15).await.expect("timeout");
                assert_eq!(snapshot(&calls), [1, 2]);

                // Período 2
                emitter.fire(3);
                emitter.fire(4);
                assert_eq!(snapshot(&calls), [1, 2, 3]);

                timeout(15).await.expect("timeout");
                assert_eq!(snapshot(&calls), [1, 2, 3, 4]);

                // Período 3
                emitter.fire(5);
                assert_eq!(snapshot(&calls), [1, 2, 3, 4, 5]);

                timeout(15).await.expect("timeout");
                // Sem disparo de descida, só houve um evento
                assert_eq!(snapshot(&calls), [1, 2, 3, 4, 5]);
            });
        }

        #[test]
        fn default_parameters() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                // Padrão: delay=100, leading=true, trailing=true
                let (emitter, calls, _) = setup(&store, |_, e| *e, ThrottleOptions::default());

                emitter.fire(1);
                assert_eq!(snapshot(&calls), [1], "should fire leading edge by default");

                emitter.fire(2);
                timeout(110).await.expect("timeout");
                assert_eq!(snapshot(&calls), [1, 2], "should fire trailing edge by default");
            });
        }

        #[test]
        fn disposal_cleans_up() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let emitter = store.add_rc(Emitter::<i32>::new());
                let throttled = emitter.event().throttle(|_, e| *e, ThrottleOptions { delay: Delay::Millis(10), ..ThrottleOptions::default() }, None);

                let calls = shared::<i32>();
                let sink = calls.clone();
                let listener = throttled.listen(move |e| sink.borrow_mut().push(*e));

                emitter.fire(1);
                emitter.fire(2);
                assert_eq!(snapshot(&calls), [1]);

                listener.dispose();

                // Eventos depois do descarte não disparam
                timeout(15).await.expect("timeout");
                emitter.fire(3);
                assert_eq!(snapshot(&calls), [1]);
            });
        }

        #[test]
        fn no_events_during_throttle_with_trailing_false() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let (emitter, calls, _) = setup(&store, count_merge, options(10, true, false));

                emitter.fire(1);
                assert_eq!(snapshot(&calls), [1]);

                // Nenhum evento a mais
                timeout(15).await.expect("timeout");
                assert_eq!(snapshot(&calls), [1]);

                // O próximo evento depois do período
                emitter.fire(2);
                assert_eq!(snapshot(&calls), [1, 1]);
            });
        }

        #[test]
        fn neither_leading_nor_trailing() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let (emitter, calls, _) = setup(&store, |_, e| *e, options(10, false, false));

                emitter.fire(1);
                emitter.fire(2);
                emitter.fire(3);
                assert_eq!(snapshot(&calls), Vec::<i32>::new());

                timeout(15).await.expect("timeout");
                assert_eq!(snapshot(&calls), Vec::<i32>::new(), "no events should fire with both leading and trailing false");
            });
        }
    }

    #[test]
    fn issue_230401() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let count = Rc::new(Cell::new(0));
        let emitter = ds.add_rc(Emitter::<()>::new());
        let disposables = ds.add_rc(DisposableStore::new());
        let (counter, e, store) = (count.clone(), emitter.clone(), disposables.clone());
        ds.add(emitter.event().listen(move |_| {
            counter.set(counter.get() + 1);
            for _ in 0..2 {
                let counter = counter.clone();
                store.add(e.event().listen(move |_| counter.set(counter.get() + 1)));
            }
            store.clear();
        }));
        let counter = count.clone();
        ds.add(emitter.event().listen(move |_| counter.set(counter.get() + 1)));
        emitter.fire(());
        assert_eq!(count.get(), 2);
    }

    mod chain2 {
        use super::*;

        struct Fixture {
            ds: common::LeakGuard,
            em: Rc<Emitter<i32>>,
            calls: Shared<i32>,
        }

        impl Fixture {
            fn new() -> Self {
                let ds = ensure_no_disposables_are_leaked_in_test_suite();
                let em = ds.add_rc(Emitter::<i32>::new());
                Self { ds, em, calls: shared() }
            }

            fn collect(&self, ev: Event<i32>) {
                let sink = self.calls.clone();
                self.ds.add(ev.listen(move |v| sink.borrow_mut().push(*v)));
            }
        }

        #[test]
        fn maps() {
            let f = Fixture::new();
            let ev = f.em.event().chain(|c| c.map(|v| v * 2));
            f.collect(ev);
            f.em.fire(1);
            f.em.fire(2);
            f.em.fire(3);
            assert_eq!(snapshot(&f.calls), [2, 4, 6]);
        }

        #[test]
        fn filters() {
            let f = Fixture::new();
            let ev = f.em.event().chain(|c| c.filter(|v| v % 2 == 0));
            f.collect(ev);
            f.em.fire(1);
            f.em.fire(2);
            f.em.fire(3);
            f.em.fire(4);
            assert_eq!(snapshot(&f.calls), [2, 4]);
        }

        #[test]
        fn reduces() {
            let f = Fixture::new();
            let ev = f.em.event().chain(|c| c.reduce(|acc: Option<i32>, v| acc.unwrap_or(0) + v, Some(0)));
            f.collect(ev);
            f.em.fire(1);
            f.em.fire(2);
            f.em.fire(3);
            f.em.fire(4);
            assert_eq!(snapshot(&f.calls), [1, 3, 6, 10]);
        }

        #[test]
        fn latches() {
            let f = Fixture::new();
            let ev = f.em.event().chain(|c| c.latch());
            f.collect(ev);
            f.em.fire(1);
            f.em.fire(1);
            f.em.fire(2);
            f.em.fire(2);
            f.em.fire(3);
            f.em.fire(3);
            f.em.fire(1);
            assert_eq!(snapshot(&f.calls), [1, 2, 3, 1]);
        }

        #[test]
        fn does_everything() {
            let f = Fixture::new();
            let ev = f.em.event().chain(|c| c.filter(|v| v % 2 == 0).map(|v| v * 2).reduce(|acc: Option<i32>, v| acc.unwrap_or(0) + v, Some(0)).latch());

            f.collect(ev);
            f.em.fire(1);
            f.em.fire(2);
            f.em.fire(3);
            f.em.fire(4);
            f.em.fire(0);
            assert_eq!(snapshot(&f.calls), [4, 12]);
        }
    }
}

// Os combinadores sem teste no original, só para o compilador conferir a API exposta.
#[allow(dead_code)]
fn api_surface(e: Event<i32>) -> (Event<i32>, Event<()>) {
    (any(vec![e.clone(), e.clone()]), e.defer(None, None).signal())
}
