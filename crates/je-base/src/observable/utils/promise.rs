//! Porte de `observableInternal/utils/promise.ts`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::errors::{self, Error};
use crate::lifecycle::DisposableStore;
use crate::observable::base::{IObservable, ISettableObservable, SettableExt};
use crate::observable::debug_name::DebugOwner;
use crate::observable::deps::StrictEquals;
use crate::observable::observables::derived::derived_owned;
use crate::observable::observables::observable_value::observable_value_owned;
use crate::observable::reactions::autorun::autorun;
use crate::observable::transaction::transaction;
use crate::promise::Promise;

/// `ObservableLazy<T>`.
pub struct ObservableLazy<T: Clone + StrictEquals + 'static> {
    value: ISettableObservable<Option<T>>,
    compute_value: Rc<dyn Fn() -> T>,
}

impl<T: Clone + StrictEquals + 'static> ObservableLazy<T> {
    pub fn new(compute_value: impl Fn() -> T + 'static) -> Self {
        Self { value: observable_value_owned::<Option<T>, ()>(DebugOwner::new("ObservableLazy"), None), compute_value: Rc::new(compute_value) }
    }

    pub fn cached_value(&self) -> IObservable<Option<T>> {
        self.value.clone()
    }

    pub fn get_value(&self) -> T {
        match self.value.get() {
            Some(v) => v,
            None => {
                let v = (self.compute_value)();
                self.value.set(Some(v.clone()), None);
                v
            }
        }
    }
}

/// `PromiseResult<T>`.
#[derive(Clone)]
pub struct PromiseResult<T> {
    pub data: Option<T>,
    pub error: Option<Error>,
}

impl<T: Clone> PromiseResult<T> {
    pub fn get_data_or_throw(&self) -> T {
        if let Some(e) = &self.error {
            errors::throw(e.clone());
        }
        self.data.clone().expect("sem erro, há dado")
    }
}

type ResultOpt<T> = Option<Rc<PromiseResult<T>>>;

/// `ObservablePromise<T>`.
pub struct ObservablePromise<T: Clone + StrictEquals + 'static> {
    pub promise: Promise<T>,
    pub promise_result: IObservable<ResultOpt<T>>,
    pub resolved_value: IObservable<Option<T>>,
}

impl<T: Clone + StrictEquals + 'static> ObservablePromise<T> {
    pub fn from_fn(f: impl FnOnce() -> Promise<T>) -> Rc<Self> {
        Self::new(f())
    }

    pub fn resolved(value: T) -> Rc<Self> {
        Self::new(Promise::resolved(value))
    }

    pub fn new(promise: Promise<T>) -> Rc<Self> {
        let owner = DebugOwner::new("ObservablePromise");
        let value = observable_value_owned::<ResultOpt<T>, ()>(owner.clone(), None);
        let target = value.clone();
        let promise = promise.then_result(move |r| {
            let result = match &r {
                Ok(v) => PromiseResult { data: Some(v.clone()), error: None },
                Err(e) => PromiseResult { data: None, error: Some(e.clone()) },
            };
            transaction(|tx| target.set(Some(Rc::new(result)), Some(tx)));
            r
        });
        let promise_result: IObservable<ResultOpt<T>> = value;
        let source = promise_result.clone();
        let resolved_value = derived_owned(owner, move |reader| source.read(reader).map(|r| r.get_data_or_throw()));
        Rc::new(Self { promise, promise_result, resolved_value })
    }
}

/// `ObservableResolvedPromise<T>`.
pub struct ObservableResolvedPromise<T: Clone + StrictEquals + 'static> {
    pub last_resolved: IObservable<T>,
    pub is_resolving: IObservable<bool>,
}

impl<T: Clone + StrictEquals + 'static> ObservableResolvedPromise<T> {
    pub fn new(source: IObservable<Rc<ObservablePromise<T>>>, initial_value: T, store: &DisposableStore) -> Self {
        let owner = DebugOwner::new("ObservableResolvedPromise");
        let last_resolved = observable_value_owned::<T, ()>(owner.clone(), initial_value);
        let is_resolving = observable_value_owned::<bool, ()>(owner, false);
        let running: RefCell<Option<Rc<ObservablePromise<T>>>> = RefCell::new(None);
        let (lr, ir) = (last_resolved.clone(), is_resolving.clone());
        store.add(autorun(move |reader| {
            let current = source.read(reader);
            *running.borrow_mut() = Some(current.clone());
            let result = current.promise_result.read(reader);
            match result {
                Some(result) => {
                    if running.borrow().as_ref().is_some_and(|r| Rc::ptr_eq(r, &current)) {
                        ir.set(false, None);
                        lr.set(result.get_data_or_throw(), None);
                    }
                }
                None => ir.set(true, None),
            }
        }));
        Self { last_resolved, is_resolving }
    }
}

/// `ObservableLazyPromise<T>`.
pub struct ObservableLazyPromise<T: Clone + StrictEquals + 'static> {
    lazy_value: Rc<ObservableLazy<Rc<ObservablePromise<T>>>>,
    pub cached_promise_result: IObservable<ResultOpt<T>>,
}

impl<T: Clone + StrictEquals + 'static> ObservableLazyPromise<T> {
    pub fn new(compute_promise: impl Fn() -> Promise<T> + 'static) -> Self {
        let lazy_value = Rc::new(ObservableLazy::new(move || ObservablePromise::new(compute_promise())));
        let lazy = lazy_value.clone();
        let cached_promise_result =
            derived_owned(DebugOwner::new("ObservableLazyPromise"), move |reader| lazy.cached_value().read(reader).and_then(|p| p.promise_result.read(reader)));
        Self { lazy_value, cached_promise_result }
    }

    pub fn get_promise(&self) -> Promise<T> {
        self.lazy_value.get_value().promise.clone()
    }
}
