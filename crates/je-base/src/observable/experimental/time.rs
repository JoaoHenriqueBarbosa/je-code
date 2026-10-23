//! Porte de `observableInternal/experimental/time.ts`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use crate::observable::base::{IObservable, SettableExt};
use crate::observable::observables::observable_value::observable_value;
use crate::observable::reactions::autorun::autorun;
use crate::runtime::{self, TimerId};

/// `TotalTrueTimeObservable`.
pub struct TotalTrueTimeObservable {
    base: Rc<Disposable>,
    value: IObservable<bool>,
    total_time: Rc<Cell<u64>>,
    start_time: Rc<Cell<Option<u64>>>,
}

impl TotalTrueTimeObservable {
    pub fn new(value: IObservable<bool>) -> Rc<Self> {
        let base = Disposable::new();
        let total_time = Rc::new(Cell::new(0u64));
        let start_time: Rc<Cell<Option<u64>>> = Rc::new(Cell::new(None));
        let (tt, st, v) = (total_time.clone(), start_time.clone(), value.clone());
        base.register(autorun(move |reader| {
            if v.read(reader) {
                st.set(Some(runtime::date_now()));
            } else if let Some(start) = st.get() {
                tt.set(tt.get() + runtime::date_now().saturating_sub(start));
                st.set(None);
            }
        }));
        Rc::new(Self { base, value, total_time, start_time })
    }

    pub fn total_time_ms(&self) -> u64 {
        match self.start_time.get() {
            Some(start) => self.total_time.get() + runtime::date_now().saturating_sub(start),
            None => self.total_time.get(),
        }
    }

    /// `fireWhenTimeIncreasedBy(deltaTimeMs, callback)`.
    pub fn fire_when_time_increased_by(&self, delta_time_ms: u64, callback: impl Fn() + 'static) -> Rc<DisposableStore> {
        let store = DisposableStore::new();
        let accumulated = Rc::new(Cell::new(0u64));
        let start: Rc<Cell<Option<u64>>> = Rc::new(Cell::new(None));
        let callback: Rc<dyn Fn()> = Rc::new(callback);
        let (value, store_in) = (self.value.clone(), store.clone());
        store.add(autorun(move |reader| {
            if !value.read(reader) {
                return;
            }
            start.set(Some(runtime::date_now()));
            let remaining = delta_time_ms.saturating_sub(accumulated.get());
            if remaining == 0 {
                callback();
                store_in.dispose();
                return;
            }
            let (acc, st, cb, store_t) = (accumulated.clone(), start.clone(), callback.clone(), store_in.clone());
            let handle: TimerId = runtime::set_timeout(
                move || {
                    acc.set(acc.get() + runtime::date_now().saturating_sub(st.get().unwrap_or(0)));
                    st.set(None);
                    cb();
                    store_t.dispose();
                },
                remaining,
            );
            let (acc, st) = (accumulated.clone(), start.clone());
            reader.store().add(to_disposable(move || {
                runtime::clear_timeout(handle);
                if let Some(s) = st.get() {
                    acc.set(acc.get() + runtime::date_now().saturating_sub(s));
                    st.set(None);
                }
            }));
        }));
        store
    }
}

impl IDisposable for TotalTrueTimeObservable {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `wasTrueRecently(obs, timeMs, store)`.
pub fn was_true_recently(obs: IObservable<bool>, time_ms: u64, store: &DisposableStore) -> IObservable<bool> {
    let result = observable_value::<bool, ()>("wasTrueRecently", false);
    let timeout: Rc<RefCell<Option<TimerId>>> = Rc::new(RefCell::new(None));
    let (target, to) = (result.clone(), timeout.clone());
    store.add(autorun(move |reader| {
        if obs.read(reader) {
            target.set(true, None);
            if let Some(t) = to.borrow_mut().take() {
                runtime::clear_timeout(t);
            }
        } else {
            let (target, to2) = (target.clone(), to.clone());
            *to.borrow_mut() = Some(runtime::set_timeout(
                move || {
                    target.set(false, None);
                    *to2.borrow_mut() = None;
                },
                time_ms,
            ));
        }
    }));
    store.add(to_disposable(move || {
        if let Some(t) = timeout.borrow_mut().take() {
            runtime::clear_timeout(t);
        }
    }));
    result
}

