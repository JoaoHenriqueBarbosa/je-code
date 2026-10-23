//! Porte de `src/vs/base/test/common/lifecycle.test.ts` (suíte `Lifecycle`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `throw new Error(...)` vira `errors::throw(Error::new(...))`; `catch (e)` vira `errors::catch`.
//! - O `isDisposable(leaked)` do teste `no leak with snapshot-utils` (em `event.rs`) é garantido pelo tipo.
//! - `originalAllocationSite` vira `original_allocation_site`: o nome entra no relatório pela pilha.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, throw_if_disposables_are_leaked, timeout};
use je_base::errors::{self, Error};
use je_base::event::Emitter;
use je_base::lifecycle::{
    DisposableRef, DisposableSet, DisposableStore, DisposableTracker, IDisposable, IDisposableTracker, ReferenceCollection, ReferencedObjects, StackSource,
    dispose_all, dispose_array, dispose_iterator, id_of, mark_as_singleton, then_if_not_disposed, to_disposable,
};
use je_base::promise::Promise;

#[derive(Default)]
struct Disposable {
    is_disposed: Cell<bool>,
}

impl IDisposable for Disposable {
    fn dispose(&self) {
        self.is_disposed.set(true);
    }
}

fn disposable() -> Rc<Disposable> {
    Rc::new(Disposable::default())
}

fn same_objects(actual: &[DisposableRef], expected: &[&DisposableRef]) -> bool {
    actual.len() == expected.len() && actual.iter().zip(expected).all(|(a, e)| id_of(a) == id_of(e))
}

/// A mensagem do erro lançado por `f`.
fn thrown(f: impl FnOnce()) -> Option<Error> {
    errors::catch(f).err()
}

mod lifecycle_suite {
    use super::*;

    #[test]
    fn dispose_single_disposable() {
        let disposable = disposable();

        assert!(!disposable.is_disposed.get());

        disposable.dispose();

        assert!(disposable.is_disposed.get());
    }

    #[test]
    fn dispose_disposable_array() {
        let (disposable, disposable2) = (disposable(), disposable());

        assert!(!disposable.is_disposed.get());
        assert!(!disposable2.is_disposed.get());

        dispose_all(&[disposable.clone() as DisposableRef, disposable2.clone()]);

        assert!(disposable.is_disposed.get());
        assert!(disposable2.is_disposed.get());
    }

    #[test]
    fn dispose_disposables() {
        let (disposable, disposable2) = (disposable(), disposable());

        assert!(!disposable.is_disposed.get());
        assert!(!disposable2.is_disposed.get());

        disposable.dispose();
        disposable2.dispose();

        assert!(disposable.is_disposed.get());
        assert!(disposable2.is_disposed.get());
    }

    #[test]
    fn dispose_array_should_dispose_all_if_a_child_throws_on_dispose() {
        let disposed_values = Rc::new(RefCell::new(Vec::<i32>::new()));

        let (a, c) = (disposed_values.clone(), disposed_values.clone());
        let items = [
            to_disposable(move || a.borrow_mut().push(1)),
            to_disposable(|| errors::throw(Error::new("I am error"))),
            to_disposable(move || c.borrow_mut().push(3)),
        ];
        let thrown_error = thrown(|| dispose_all(&items));

        assert!(disposed_values.borrow().contains(&1));
        assert!(disposed_values.borrow().contains(&3));
        assert_eq!(thrown_error.expect("devia lançar").message, "I am error");
    }

    #[test]
    fn dispose_array_should_rethrow_composite_error_if_multiple_entries_throw_on_dispose() {
        let disposed_values = Rc::new(RefCell::new(Vec::<i32>::new()));

        let (a, d) = (disposed_values.clone(), disposed_values.clone());
        let items = [
            to_disposable(move || a.borrow_mut().push(1)),
            to_disposable(|| errors::throw(Error::new("I am error 1"))),
            to_disposable(|| errors::throw(Error::new("I am error 2"))),
            to_disposable(move || d.borrow_mut().push(4)),
        ];
        let thrown_error = thrown(|| dispose_all(&items)).expect("devia lançar");

        assert!(disposed_values.borrow().contains(&1));
        assert!(disposed_values.borrow().contains(&4));
        assert!(thrown_error.is_aggregate());
        assert_eq!(thrown_error.errors.len(), 2);
        assert_eq!(thrown_error.errors[0].message, "I am error 1");
        assert_eq!(thrown_error.errors[1].message, "I am error 2");
    }

    #[test]
    fn action_bar_has_broken_accessibility_100273() {
        let array: Vec<DisposableRef> = vec![to_disposable(|| {}), to_disposable(|| {})];
        let array2 = dispose_array(&array);

        assert_eq!(array.len(), 2);
        assert_eq!(array2.len(), 0);

        let set: Vec<DisposableRef> = vec![to_disposable(|| {}), to_disposable(|| {})];
        // O iterador volta consumido e é o mesmo objeto (o `===` do original é o próprio valor devolvido).
        let mut set_values2 = dispose_iterator(set.into_iter());
        assert!(set_values2.next().is_none());
    }
}

mod disposable_tracker {
    use super::*;

    fn allocation_source(reads: &Rc<Cell<u32>>) -> StackSource {
        let reads = reads.clone();
        StackSource::new(move || {
            reads.set(reads.get() + 1);
            "Error\n    at allocation (test.js:1:1)".to_owned()
        })
    }

    #[test]
    fn does_not_format_stacks_while_tracking_disposing_or_excluding_singletons() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();

        let tracker = DisposableTracker::new();
        let stack_reads = Rc::new(Cell::new(0));
        let source = allocation_source(&stack_reads);

        let disposed: DisposableRef = disposable();
        let singleton: DisposableRef = disposable();
        let child: DisposableRef = disposable();
        tracker.track_disposable_from(&disposed, Some(source.clone()));
        tracker.track_disposable_from(&disposed, Some(source.clone()));
        tracker.track_disposable_from(&singleton, Some(source.clone()));
        tracker.track_disposable_from(&child, Some(source));
        tracker.set_parent(id_of(&child), Some(id_of(&singleton)));
        let initially_tracked = tracker.get_tracked_disposables();

        disposed.dispose();
        tracker.mark_as_disposed(id_of(&disposed));
        tracker.mark_as_singleton(id_of(&singleton));

        assert!(same_objects(&initially_tracked, &[&disposed, &singleton, &child]));
        assert!(tracker.get_tracked_disposables().is_empty());
        assert!(tracker.compute_leaking_disposables().is_none());
        assert_eq!(stack_reads.get(), 0);
    }

    #[test]
    fn formats_allocation_stacks_when_reporting_leaks() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();

        let tracker = DisposableTracker::new();
        let stack_reads = Rc::new(Cell::new(0));
        let disposable: DisposableRef = self::disposable();
        tracker.track_disposable_from(&disposable, Some(allocation_source(&stack_reads)));
        let reads_before_reporting = stack_reads.get();
        let result = tracker.compute_leaking_disposables();

        assert_eq!(reads_before_reporting, 0);
        assert!(stack_reads.get() > 0);
        let result = result.expect("devia haver vazamento");
        let leaks: Vec<DisposableRef> = result.leaks.iter().map(|leak| leak.value.clone().expect("rastreado")).collect();
        assert!(same_objects(&leaks, &[&disposable]));
        assert!(result.details.contains("at allocation (test.js:1:1)"));
    }

    #[test]
    fn retains_the_original_allocation_site_when_tracking_a_disposable_again() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();

        let tracker = DisposableTracker::new();
        let disposable: DisposableRef = self::disposable();
        #[inline(never)]
        fn original_allocation_site(tracker: &DisposableTracker, disposable: &DisposableRef) {
            tracker.track_disposable(disposable);
        }
        original_allocation_site(&tracker, &disposable);
        tracker.track_disposable(&disposable);

        assert!(same_objects(&tracker.get_tracked_disposables(), &[&disposable]));
        assert!(tracker.compute_leaking_disposables().expect("devia haver vazamento").details.contains("original_allocation_site"));
    }
}

mod disposable_store {
    use super::*;

    #[test]
    fn dispose_should_call_all_child_disposes_even_if_a_child_throws_on_dispose() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let disposed_values = Rc::new(RefCell::new(Vec::<i32>::new()));

        let store = DisposableStore::new();
        let (a, c) = (disposed_values.clone(), disposed_values.clone());
        store.add(to_disposable(move || a.borrow_mut().push(1)));
        store.add(to_disposable(|| errors::throw(Error::new("I am error"))));
        store.add(to_disposable(move || c.borrow_mut().push(3)));

        let thrown_error = thrown(|| store.dispose());

        assert!(disposed_values.borrow().contains(&1));
        assert!(disposed_values.borrow().contains(&3));
        assert_eq!(thrown_error.expect("devia lançar").message, "I am error");
    }

    #[test]
    fn dispose_should_throw_composite_error_if_multiple_children_throw_on_dispose() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let disposed_values = Rc::new(RefCell::new(Vec::<i32>::new()));

        let store = DisposableStore::new();
        let (a, d) = (disposed_values.clone(), disposed_values.clone());
        store.add(to_disposable(move || a.borrow_mut().push(1)));
        store.add(to_disposable(|| errors::throw(Error::new("I am error 1"))));
        store.add(to_disposable(|| errors::throw(Error::new("I am error 2"))));
        store.add(to_disposable(move || d.borrow_mut().push(4)));

        let thrown_error = thrown(|| store.dispose()).expect("devia lançar");

        assert!(disposed_values.borrow().contains(&1));
        assert!(disposed_values.borrow().contains(&4));
        assert!(thrown_error.is_aggregate());
        assert_eq!(thrown_error.errors.len(), 2);
        assert_eq!(thrown_error.errors[0].message, "I am error 1");
        assert_eq!(thrown_error.errors[1].message, "I am error 2");
    }

    #[test]
    fn delete_should_evict_and_dispose_of_the_disposables() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let disposed_values = Rc::new(RefCell::new(Vec::<i32>::new()));
        let (a, b) = (disposed_values.clone(), disposed_values.clone());
        let disposables: Vec<DisposableRef> = vec![to_disposable(move || a.borrow_mut().push(1)), to_disposable(move || b.borrow_mut().push(2))];

        let store = DisposableStore::new();
        store.add(disposables[0].clone());
        store.add(disposables[1].clone());

        store.delete(&disposables[0]);

        assert!(disposed_values.borrow().contains(&1));
        assert!(!disposed_values.borrow().contains(&2));

        store.dispose();

        assert!(disposed_values.borrow().contains(&1));
        assert!(disposed_values.borrow().contains(&2));
    }

    #[test]
    fn delete_and_leak_should_evict_and_not_dispose_of_the_disposables() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let disposed_values = Rc::new(RefCell::new(Vec::<i32>::new()));
        let (a, b) = (disposed_values.clone(), disposed_values.clone());
        let disposables: Vec<DisposableRef> = vec![to_disposable(move || a.borrow_mut().push(1)), to_disposable(move || b.borrow_mut().push(2))];

        let store = DisposableStore::new();
        store.add(disposables[0].clone());
        store.add(disposables[1].clone());

        store.delete_and_leak(&disposables[0]);

        assert!(!disposed_values.borrow().contains(&1));
        assert!(!disposed_values.borrow().contains(&2));

        store.dispose();

        assert!(!disposed_values.borrow().contains(&1));
        assert!(disposed_values.borrow().contains(&2));

        disposables[0].dispose();
    }
}

mod disposable_set {
    use super::*;

    fn recording(disposed_values: &Rc<RefCell<Vec<i32>>>, n: i32) -> DisposableRef {
        let disposed_values = disposed_values.clone();
        to_disposable(move || disposed_values.borrow_mut().push(n))
    }

    #[test]
    fn dispose_should_dispose_all_values_and_mark_as_disposed() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let disposed_values = Rc::new(RefCell::new(Vec::new()));

        let set = DisposableSet::<dyn IDisposable>::new();
        set.add(recording(&disposed_values, 1));
        set.add(recording(&disposed_values, 2));
        set.add(recording(&disposed_values, 3));

        assert_eq!(set.size(), 3);

        set.dispose();

        assert!(disposed_values.borrow().contains(&1));
        assert!(disposed_values.borrow().contains(&2));
        assert!(disposed_values.borrow().contains(&3));
        assert_eq!(set.size(), 0);
    }

    #[test]
    fn dispose_should_call_all_child_disposes_even_if_a_child_throws_on_dispose() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let disposed_values = Rc::new(RefCell::new(Vec::new()));

        let set = DisposableSet::<dyn IDisposable>::new();
        set.add(recording(&disposed_values, 1));
        set.add(to_disposable(|| errors::throw(Error::new("I am error"))));
        set.add(recording(&disposed_values, 3));

        let thrown_error = thrown(|| set.dispose());

        assert!(disposed_values.borrow().contains(&1));
        assert!(disposed_values.borrow().contains(&3));
        assert_eq!(thrown_error.expect("devia lançar").message, "I am error");
    }

    #[test]
    fn clear_and_dispose_all_should_dispose_values_but_not_mark_as_disposed() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let disposed_values = Rc::new(RefCell::new(Vec::new()));

        let set = DisposableSet::<dyn IDisposable>::new();
        let d1 = recording(&disposed_values, 1);
        set.add(d1);

        set.clear_and_dispose_all();

        assert!(disposed_values.borrow().contains(&1));
        assert_eq!(set.size(), 0);

        // Ainda dá para adicionar valores novos
        let d2 = recording(&disposed_values, 2);
        set.add(d2);
        assert_eq!(set.size(), 1);

        set.dispose();
        assert!(disposed_values.borrow().contains(&2));
    }

    #[test]
    fn has_should_return_true_if_value_exists() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();

        let set = DisposableSet::<dyn IDisposable>::new();
        let d = to_disposable(|| {});
        set.add(d.clone());

        let other = to_disposable(|| {});
        assert!(set.has(&d));
        assert!(!set.has(&other));

        set.dispose();
        other.dispose();
    }

    #[test]
    fn delete_and_dispose_should_remove_and_dispose_the_value() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let disposed_values = Rc::new(RefCell::new(Vec::new()));

        let set = DisposableSet::<dyn IDisposable>::new();
        let d1 = recording(&disposed_values, 1);
        let d2 = recording(&disposed_values, 2);
        set.add(d1.clone());
        set.add(d2.clone());

        set.delete_and_dispose(&d1);

        assert!(disposed_values.borrow().contains(&1));
        assert!(!disposed_values.borrow().contains(&2));
        assert_eq!(set.size(), 1);
        assert!(!set.has(&d1));
        assert!(set.has(&d2));

        set.dispose();
        assert!(disposed_values.borrow().contains(&2));
    }

    #[test]
    fn delete_and_leak_should_remove_but_not_dispose_the_value() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let disposed_values = Rc::new(RefCell::new(Vec::new()));

        let set = DisposableSet::<dyn IDisposable>::new();
        let d1 = recording(&disposed_values, 1);
        let d2 = recording(&disposed_values, 2);
        set.add(d1.clone());
        set.add(d2.clone());

        let leaked = set.delete_and_leak(&d1);

        assert!(leaked.is_some_and(|leaked| id_of(&leaked) == id_of(&d1)));
        assert!(!disposed_values.borrow().contains(&1));
        assert!(!disposed_values.borrow().contains(&2));
        assert_eq!(set.size(), 1);

        set.dispose();

        assert!(!disposed_values.borrow().contains(&1));
        assert!(disposed_values.borrow().contains(&2));

        // Quem recebeu o valor é quem descarta
        d1.dispose();
    }

    #[test]
    fn delete_and_leak_should_return_undefined_if_value_not_in_set() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();

        let set = DisposableSet::<dyn IDisposable>::new();
        let d = to_disposable(|| {});

        let leaked = set.delete_and_leak(&d);

        assert!(leaked.is_none());

        set.dispose();
        d.dispose();
    }

    #[test]
    fn values_should_iterate_over_all_values() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();

        let set = DisposableSet::<dyn IDisposable>::new();
        let d1 = to_disposable(|| {});
        let d2 = to_disposable(|| {});
        set.add(d1.clone());
        set.add(d2.clone());

        let values = set.values();
        assert_eq!(values.len(), 2);
        assert!(values.iter().any(|v| id_of(v) == id_of(&d1)));
        assert!(values.iter().any(|v| id_of(v) == id_of(&d2)));

        set.dispose();
    }

    #[test]
    fn symbol_iterator_should_allow_for_of_iteration() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();

        let set = DisposableSet::<dyn IDisposable>::new();
        let d1 = to_disposable(|| {});
        let d2 = to_disposable(|| {});
        set.add(d1.clone());
        set.add(d2.clone());

        // O `for...of` do original é a iteração por `values()` aqui.
        let mut values: Vec<DisposableRef> = Vec::new();
        for v in set.values() {
            values.push(v);
        }

        assert_eq!(values.len(), 2);
        assert!(values.iter().any(|v| id_of(v) == id_of(&d1)));
        assert!(values.iter().any(|v| id_of(v) == id_of(&d2)));

        set.dispose();
    }
}

mod reference_collection {
    use super::*;

    struct Counting {
        count: Rc<Cell<i32>>,
    }

    impl ReferencedObjects<usize> for Counting {
        fn create_referenced_object(&self, key: &str) -> usize {
            self.count.set(self.count.get() + 1);
            key.len()
        }

        fn destroy_referenced_object(&self, _key: &str, _object: &usize) {
            self.count.set(self.count.get() - 1);
        }
    }

    #[test]
    fn simple() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let count = Rc::new(Cell::new(0));
        let collection = ReferenceCollection::new(Counting { count: count.clone() });

        let ref1 = collection.acquire("test");
        assert_eq!(ref1.object, 4);
        assert_eq!(count.get(), 1);
        ref1.dispose();
        assert_eq!(count.get(), 0);

        let ref2 = collection.acquire("test");
        let ref3 = collection.acquire("test");
        assert_eq!(ref2.object, ref3.object);
        assert_eq!(count.get(), 1);

        let ref4 = collection.acquire("monkey");
        assert_eq!(ref4.object, 6);
        assert_eq!(count.get(), 2);

        ref2.dispose();
        assert_eq!(count.get(), 2);

        ref3.dispose();
        assert_eq!(count.get(), 1);

        ref4.dispose();
        assert_eq!(count.get(), 0);
    }
}

fn assert_throws(f: impl FnOnce(), test: impl Fn(&Error) -> bool) {
    match errors::catch(f) {
        Ok(()) => panic!("Expected function to throw, but it did not."),
        Err(e) => assert!(test(&e)),
    }
}

mod no_leakage_utilities {
    use super::*;

    mod throw_if_disposables_are_leaked_suite {
        use super::*;

        #[test]
        fn throws_if_an_event_subscription_is_not_cleaned_up() {
            let event_emitter = Emitter::<()>::new();

            assert_throws(
                || {
                    throw_if_disposables_are_leaked(
                        || {
                            event_emitter.event().listen(|_| {
                                // noop
                            });
                        },
                        false,
                    );
                },
                |e| e.message.contains("undisposed disposables"),
            );
        }

        #[test]
        fn throws_if_a_disposable_is_not_disposed() {
            assert_throws(
                || {
                    throw_if_disposables_are_leaked(
                        || {
                            DisposableStore::new();
                        },
                        false,
                    );
                },
                |e| e.message.contains("undisposed disposables"),
            );
        }

        #[test]
        fn does_not_throw_if_all_event_subscriptions_are_cleaned_up() {
            let event_emitter = Emitter::<()>::new();
            throw_if_disposables_are_leaked(
                || {
                    event_emitter
                        .event()
                        .listen(|_| {
                            // noop
                        })
                        .dispose();
                },
                true,
            );
        }

        #[test]
        fn does_not_throw_if_all_disposables_are_disposed() {
            // Este disposable é criado antes do teste e não é rastreado.
            to_disposable(|| {});

            throw_if_disposables_are_leaked(
                || {
                    // Este disposable é marcado como singleton
                    mark_as_singleton(to_disposable(|| {}));

                    // Estes disposables também são marcados como singleton
                    let disposable_store = DisposableStore::new();
                    disposable_store.add(to_disposable(|| {}));
                    mark_as_singleton(disposable_store);

                    to_disposable(|| {}).dispose();
                },
                true,
            );
        }
    }

    mod ensure_no_disposables_are_leaked_in_test {
        use super::*;

        #[test]
        fn basic_test() {
            let _ds = ensure_no_disposables_are_leaked_in_test_suite();

            to_disposable(|| {}).dispose();
        }
    }

    mod then_if_not_disposed_suite {
        use super::*;

        #[test]
        fn normal_case() {
            let ds = ensure_no_disposables_are_leaked_in_test_suite();
            let store = ds.store();
            run_with_faked_timers(async move {
                let result = Rc::new(Cell::new(None));
                let observed = result.clone();
                store.add(then_if_not_disposed(&Promise::resolved(123), move |value: i32| observed.set(Some(value))));

                let _ = timeout(0).await;
                assert_eq!(result.get(), Some(123));
            });
        }

        #[test]
        fn disposed_before_promise_resolves() {
            let _ds = ensure_no_disposables_are_leaked_in_test_suite();
            run_with_faked_timers(async move {
                let called = Rc::new(Cell::new(false));
                let observed = called.clone();
                let disposable = then_if_not_disposed(&Promise::resolved(123), move |_: i32| observed.set(true));

                disposable.dispose();
                let _ = timeout(0).await;
                assert!(!called.get());
            });
        }
    }
}
