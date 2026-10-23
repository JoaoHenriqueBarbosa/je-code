//! Porte de `observableInternal/utils/runOnChange.ts`.

use std::cell::{Cell, RefCell};

use crate::cancellation::{CancellationToken, cancel_on_dispose};
use crate::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use crate::observable::base::IObservable;
use crate::observable::change_tracker::{FnChangeTracker, IChangeContext};
use crate::observable::debug_name::DebugNameOptions;
use crate::observable::reactions::autorun::autorun_with_store_handle_changes;
use crate::promise::Promise;

struct Summary<C> {
    deltas: RefCell<Vec<C>>,
    did_change: Cell<bool>,
}

/// `runOnChange(observable, cb)`: `C` é o tipo do delta (`()` se não há).
pub fn run_on_change<T: Clone + 'static, C: Clone + 'static>(observable: IObservable<T>, cb: impl Fn(&T, &T, &[C]) + 'static) -> DisposableRef {
    let previous: RefCell<Option<T>> = RefCell::new(None);
    let first_run = Cell::new(true);
    let watched = observable.to_any_observable();
    let tracker = FnChangeTracker::<Summary<C>>::new(
        |_| Summary { deltas: RefCell::new(Vec::new()), did_change: Cell::new(false) },
        move |ctx: &dyn IChangeContext, summary: &Summary<C>| {
            if ctx.did_change(&*watched) {
                // `ctx.change` só existe se não for indefinido.
                let e = <dyn IChangeContext>::change::<C>(ctx);
                if let Some(e) = e {
                    summary.deltas.borrow_mut().push(e.clone());
                }
                summary.did_change.set(true);
            }
            true
        },
    );
    autorun_with_store_handle_changes(DebugNameOptions::default(), tracker, move |reader, summary, _store| {
        let value = observable.read(reader);
        let previous_value = previous.borrow().clone();
        if summary.did_change.get() {
            *previous.borrow_mut() = Some(value.clone());
            // `did_change` nunca é verdadeiro na primeira execução, então há valor anterior.
            let deltas = summary.deltas.borrow().clone();
            cb(&value, previous_value.as_ref().expect("há valor anterior"), &deltas);
        }
        if first_run.get() {
            first_run.set(false);
            *previous.borrow_mut() = Some(value);
        }
    })
}

/// `runOnChangeWithStore(observable, cb)`.
pub fn run_on_change_with_store<T: Clone + 'static, C: Clone + 'static>(
    observable: IObservable<T>,
    cb: impl Fn(&T, &T, &[C], &DisposableStore) + 'static,
) -> DisposableRef {
    let store = DisposableStore::new();
    let s = store.clone();
    let disposable = run_on_change::<T, C>(observable, move |value, previous, deltas| {
        s.clear();
        cb(value, previous, deltas, &s);
    });
    to_disposable(move || {
        disposable.dispose();
        store.dispose();
    })
}

/// `runOnChangeWithCancellationToken(observable, cb)`.
pub fn run_on_change_with_cancellation_token<T: Clone + 'static, C: Clone + 'static>(
    observable: IObservable<T>,
    cb: impl Fn(&T, &T, &[C], CancellationToken) -> Promise<()> + 'static,
) -> DisposableRef {
    run_on_change_with_store::<T, C>(observable, move |value, previous, deltas, store| {
        cb(value, previous, deltas, cancel_on_dispose(store));
    })
}

