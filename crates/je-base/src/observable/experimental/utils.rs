//! Porte de `observableInternal/experimental/utils.ts`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::errors::{self, Error};
use crate::event::{Event, Listener};
use crate::lifecycle::{DisposableRef, DisposableStore, plain_disposable};
use crate::observable::base::{IObservable, IReader};
use crate::observable::debug_name::{DebugNameOptions, DebugOwner};
use crate::observable::deps::StrictEquals;
use crate::observable::observables::observable_from_event::observable_from_event_owned;
use crate::observable::reactions::autorun::autorun_opts;
use crate::observable::utils::utils::derived_observable_with_cache;

/// `latestChangedValue(owner, observables)`: o valor do último observável que mudou.
pub fn latest_changed_value<T: Clone + StrictEquals + 'static>(owner: DebugOwner, observables: Vec<IObservable<T>>) -> IObservable<T> {
    if observables.is_empty() {
        errors::throw(Error::bug_indicating(None));
    }
    let has_last = Rc::new(Cell::new(false));
    let last: Rc<RefCell<Option<T>>> = Rc::new(RefCell::new(None));
    let all = Rc::new(observables);
    let (has_in, last_in, all_in) = (has_last.clone(), last.clone(), all.clone());
    let event = Event::<()>::new(move |cb: Listener<()>| -> DisposableRef {
        let store = DisposableStore::new();
        for o in all_in.iter() {
            let (o, has, last, cb) = (o.clone(), has_in.clone(), last_in.clone(), cb.clone());
            store.add(autorun_opts(DebugNameOptions::named("updateLastChangedValue"), move |reader| {
                has.set(true);
                *last.borrow_mut() = Some(o.read(reader));
                cb(&());
            }));
        }
        let (has, last) = (has_in.clone(), last_in.clone());
        store.add(plain_disposable(move || {
            has.set(false);
            *last.borrow_mut() = None;
        }));
        store
    });
    observable_from_event_owned(owner, event, move |_| {
        if has_last.get() { last.borrow().clone().expect("hasLast") } else { all.last().expect("não vazio").get() }
    })
}

/// `derivedConstOnceDefined(owner, fn)`: calcula até ter um valor e depois o mantém.
pub fn derived_const_once_defined<T: Clone + StrictEquals + 'static>(owner: DebugOwner, f: impl Fn(&dyn IReader) -> T + 'static) -> IObservable<Option<T>> {
    derived_observable_with_cache::<Option<T>>(Some(owner), move |reader, last| match last {
        Some(Some(v)) => Some(v.clone()),
        _ => Some(f(reader)),
    })
}
