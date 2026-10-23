//! Porte de `observableInternal/experimental/reducer.ts`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::errors::{self, Error};
use crate::observable::base::{IObservable, ISettableObservable, ITransaction, Settable};
use crate::observable::change_tracker::IChangeTracker;
use crate::observable::debug_name::{DebugNameData, DebugOwner};
use crate::observable::deps::{EqualityComparer, StrictEquals, strict_equals};
use crate::observable::observables::derived::{Derived, IDerivedReader};
use crate::observable::transaction::subtransaction;

/// `IReducerOptions<T, TChangeSummary, TOutChange>`. Sem rastreador de mudanças, use `S = ()` com
/// [`NoTracker`](crate::observable::observables::derived::NoTracker).
pub struct ReducerOptions<T, S, C> {
    pub initial: Rc<dyn Fn() -> T>,
    pub dispose_final: Option<Rc<dyn Fn(&T)>>,
    pub change_tracker: Rc<dyn IChangeTracker<S>>,
    pub equality_comparer: Option<EqualityComparer<T>>,
    #[allow(clippy::type_complexity)]
    pub update: Rc<dyn Fn(&dyn IDerivedReader<C>, &T, &S) -> T>,
}

/// `observableReducer(owner, options)`.
pub fn observable_reducer<T: Clone + StrictEquals + 'static, S: 'static, C: 'static>(owner: DebugOwner, options: ReducerOptions<T, S, C>) -> IObservable<T> {
    observable_reducer_settable(owner, options).to_observable()
}

/// `observableReducerSettable(owner, options)`.
pub fn observable_reducer_settable<T: Clone + StrictEquals + 'static, S: 'static, C: 'static>(
    owner: DebugOwner,
    options: ReducerOptions<T, S, C>,
) -> ISettableObservable<T, C> {
    let prev_value: Rc<RefCell<Option<T>>> = Rc::new(RefCell::new(None));
    let d_slot: Rc<RefCell<Option<Weak<Derived<T, S, C>>>>> = Rc::new(RefCell::new(None));
    let (initial, update) = (options.initial.clone(), options.update.clone());
    let (pv_compute, pv_dispose, pv_set) = (prev_value.clone(), prev_value.clone(), prev_value);
    let dispose_final = options.dispose_final.clone();
    let slot = d_slot.clone();
    let d = Derived::new_tracked(
        DebugNameData::new(Some(owner), None),
        move |reader, summary| {
            if pv_compute.borrow().is_none() {
                *pv_compute.borrow_mut() = Some(initial());
            }
            let prev = pv_compute.borrow().clone().expect("acabou de ser criado");
            let new_value = update(reader, &prev, summary);
            *pv_compute.borrow_mut() = Some(new_value.clone());
            new_value
        },
        options.change_tracker,
        Some(Rc::new(move || {
            let had = pv_dispose.borrow_mut().take();
            if let (Some(v), Some(f)) = (had, &dispose_final) {
                f(&v);
            }
        })),
        options.equality_comparer.unwrap_or_else(strict_equals),
        Some(Rc::new(move |value: T, tx: Option<&dyn ITransaction>, change: C| {
            if pv_set.borrow().is_none() {
                errors::throw(Error::bug_indicating(Some("Can only set when there is a listener! This is to prevent leaks.")));
            }
            subtransaction(tx, |tx| {
                *pv_set.borrow_mut() = Some(value.clone());
                let d = slot.borrow().as_ref().and_then(Weak::upgrade).expect("o reducer está vivo");
                d.set_value(value, tx, change);
            });
        })),
    );
    *d_slot.borrow_mut() = Some(Rc::downgrade(&d));
    d.to_settable()
}
