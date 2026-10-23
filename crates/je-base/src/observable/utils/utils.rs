//! Porte de `observableInternal/utils/utils.ts`.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::event::{Event, Listener};
use crate::lifecycle::{DisposableRef, DisposableStore, IDisposable, plain_disposable, to_disposable};
use crate::observable::base::{AnyObservable, IObservable, IObserver, IReader, ITransaction, Observable, ObserverRef, SettableExt};
use crate::observable::debug_name::{DebugNameSource, DebugOwner};
use crate::observable::deps::StrictEquals;
use crate::observable::observables::derived::{DerivedOptions, derived, derived_opts};
use crate::observable::observables::observable_from_event::observable_from_event;
use crate::observable::observables::observable_signal::{ObservableSignal, observable_signal};
use crate::observable::observables::observable_value::observable_value;
use crate::observable::reactions::autorun::autorun;
use crate::runtime::{self, TimerId};

/// `observableFromPromise(promise)`: o valor é `{ value }` quando resolve (e `{}` antes).
pub fn observable_from_promise<T: Clone + 'static>(promise: crate::promise::Promise<T>) -> IObservable<Rc<Option<T>>> {
    let observable = observable_value::<Rc<Option<T>>, ()>("promiseValue", Rc::new(None));
    let target = observable.clone();
    promise.then(move |value| target.set(Rc::new(Some(value)), None));
    observable
}

/// `signalFromObservable(owner, observable)`: avisa sempre que o observável é recalculado.
pub fn signal_from_observable<T: Clone + 'static>(owner: Option<DebugOwner>, observable: IObservable<T>) -> IObservable<()> {
    derived_opts(
        DerivedOptions { owner, equals_fn: Some(Rc::new(|_, _| false)), ..DerivedOptions::default() },
        move |reader| {
            observable.read(reader);
        },
    )
}

/// O `debounceMs: number | ((lastValue, newValue) => number)`.
pub enum DebounceMs<T> {
    Fixed(u64),
    Dynamic(Rc<dyn Fn(Option<&T>, &T) -> u64>),
}

impl<T> DebounceMs<T> {
    fn duration(&self, last: Option<&T>, new: &T) -> u64 {
        match self {
            Self::Fixed(ms) => *ms,
            Self::Dynamic(f) => f(last, new),
        }
    }
}

/// `debouncedObservable(observable, debounceMs)`.
pub fn debounced_observable<T: Clone + StrictEquals + 'static>(observable: IObservable<T>, debounce_ms: DebounceMs<T>) -> IObservable<T> {
    let has_value = Rc::new(Cell::new(false));
    let last_value: Rc<RefCell<Option<T>>> = Rc::new(RefCell::new(None));
    let timeout: Rc<Cell<Option<TimerId>>> = Rc::new(Cell::new(None));
    let source = observable.clone();
    let (hv, lv, to) = (has_value.clone(), last_value.clone(), timeout.clone());
    let event = Event::<()>::new(move |cb: Listener<()>| {
        let (hv, lv, to, observable, debounce_ms) = (hv.clone(), lv.clone(), to.clone(), observable.clone(), &debounce_ms);
        let debounce = match debounce_ms {
            DebounceMs::Fixed(ms) => DebounceMs::Fixed(*ms),
            DebounceMs::Dynamic(f) => DebounceMs::Dynamic(f.clone()),
        };
        let (hv2, lv2) = (hv.clone(), lv.clone());
        let d = autorun(move |reader| {
            let value = observable.read(reader);
            if !hv.get() {
                hv.set(true);
                *lv.borrow_mut() = Some(value);
            } else {
                if let Some(t) = to.take() {
                    runtime::clear_timeout(t);
                }
                let duration = debounce.duration(lv.borrow().as_ref(), &value);
                if duration == 0 {
                    *lv.borrow_mut() = Some(value);
                    cb(&());
                    return;
                }
                let (lv, cb) = (lv.clone(), cb.clone());
                to.set(Some(runtime::set_timeout(
                    move || {
                        *lv.borrow_mut() = Some(value);
                        cb(&());
                    },
                    duration,
                )));
            }
        });
        plain_disposable(move || {
            d.dispose();
            hv2.set(false);
            *lv2.borrow_mut() = None;
        })
    });
    observable_from_event(event, move |_: Option<&()>| {
        if has_value.get() { last_value.borrow().clone().expect("hasValue") } else { source.get() }
    })
}

/// `throttledObservable(observable, throttleMs)`.
pub fn throttled_observable<T: Clone + StrictEquals + 'static>(observable: IObservable<T>, throttle_ms: u64) -> IObservable<T> {
    let has_value = Rc::new(Cell::new(false));
    let last_value: Rc<RefCell<Option<T>>> = Rc::new(RefCell::new(None));
    let timeout: Rc<Cell<Option<TimerId>>> = Rc::new(Cell::new(None));
    let source = observable.clone();
    let (hv, lv, to) = (has_value.clone(), last_value.clone(), timeout.clone());
    let event = Event::<()>::new(move |cb: Listener<()>| {
        let (hv, lv, to, observable) = (hv.clone(), lv.clone(), to.clone(), observable.clone());
        let (hv2, lv2, to2) = (hv.clone(), lv.clone(), to.clone());
        let d = autorun(move |reader| {
            let value = observable.read(reader);
            if !hv.get() {
                hv.set(true);
                *lv.borrow_mut() = Some(value);
            } else if to.get().is_none() {
                // Só inicia um timer se não houver um rodando.
                let (to_in, lv_in, obs, cb) = (to.clone(), lv.clone(), observable.clone(), cb.clone());
                to.set(Some(runtime::set_timeout(
                    move || {
                        to_in.set(None);
                        *lv_in.borrow_mut() = Some(obs.get());
                        cb(&());
                    },
                    throttle_ms,
                )));
            }
        });
        plain_disposable(move || {
            d.dispose();
            if let Some(t) = to2.take() {
                runtime::clear_timeout(t);
            }
            hv2.set(false);
            *lv2.borrow_mut() = None;
        })
    });
    observable_from_event(event, move |_: Option<&()>| {
        if has_value.get() { last_value.borrow().clone().expect("hasValue") } else { source.get() }
    })
}

/// `debouncedObservable2(observable, debounceMs)`.
pub fn debounced_observable2<T: Clone + StrictEquals + 'static>(observable: IObservable<T>, debounce_ms: DebounceMs<T>) -> IObservable<T> {
    let s = observable_signal::<()>("handleTimeout");
    let current_value: Rc<RefCell<Option<T>>> = Rc::new(RefCell::new(None));
    let timeout: Rc<Cell<Option<TimerId>>> = Rc::new(Cell::new(None));
    let cv_reset = current_value.clone();
    derived_opts(
        DerivedOptions {
            on_last_observer_removed: Some(Rc::new(move || *cv_reset.borrow_mut() = None)),
            ..DerivedOptions::default()
        },
        move |reader| {
            let val = observable.read(reader);
            s.read(reader);
            let changed = !current_value.borrow().as_ref().is_some_and(|c| c.strict_equals(&val));
            if changed {
                let duration = debounce_ms.duration(current_value.borrow().as_ref(), &val);
                if duration == 0 {
                    *current_value.borrow_mut() = Some(val.clone());
                    return val;
                }
                if let Some(t) = timeout.take() {
                    runtime::clear_timeout(t);
                }
                let (cv, s) = (current_value.clone(), s.clone());
                let v = val.clone();
                timeout.set(Some(runtime::set_timeout(
                    move || {
                        *cv.borrow_mut() = Some(v);
                        s.trigger(None, ());
                    },
                    duration,
                )));
            }
            current_value.borrow().clone().unwrap_or(val)
        },
    )
}

/// `wasEventTriggeredRecently(event, timeoutMs, disposableStore)`.
pub fn was_event_triggered_recently<A: 'static>(event: &Event<A>, timeout_ms: u64, store: &DisposableStore) -> IObservable<bool> {
    let observable = observable_value::<bool, ()>("triggeredRecently", false);
    let timeout: Rc<Cell<Option<TimerId>>> = Rc::new(Cell::new(None));
    let target = observable.clone();
    store.add(event.listen(move |_| {
        target.set(true, None);
        if let Some(t) = timeout.take() {
            runtime::clear_timeout(t);
        }
        let target = target.clone();
        timeout.set(Some(runtime::set_timeout(move || target.set(false, None), timeout_ms)));
    }));
    observable
}

/// `keepObserved(observable)`: mantém o cache vivo sem forçar o cálculo.
pub fn keep_observed<T: Clone + 'static>(observable: &IObservable<T>) -> DisposableRef {
    let o: ObserverRef = Rc::new(KeepAliveObserver { counter: Cell::new(0), handle: None });
    observable.add_observer(o.clone());
    let observable = observable.clone();
    to_disposable(move || observable.remove_observer(&o))
}

/// `recomputeInitiallyAndOnChange(observable, handleValue)`.
pub fn recompute_initially_and_on_change<T: Clone + 'static>(observable: &IObservable<T>, handle_value: Option<Rc<dyn Fn(&T)>>) -> DisposableRef {
    let obs = observable.clone();
    let handle: Rc<dyn Fn()> = match handle_value {
        Some(h) => Rc::new(move || h(&obs.get())),
        None => Rc::new(move || obs.report_changes()),
    };
    let o: ObserverRef = Rc::new(KeepAliveObserver { counter: Cell::new(0), handle: Some(handle) });
    observable.add_observer(o.clone());
    let any = observable.to_any_observable();
    crate::observable::deps::try_finally(|| o.begin_update(&*any), || o.end_update(&*any));
    let observable = observable.clone();
    to_disposable(move || observable.remove_observer(&o))
}

/// `KeepAliveObserver`.
pub struct KeepAliveObserver {
    counter: Cell<i32>,
    /// `Some` quando é para recomputar (`forceRecompute`): lê o valor e o entrega a quem pediu.
    handle: Option<Rc<dyn Fn()>>,
}

impl IObserver for KeepAliveObserver {
    fn begin_update(&self, _observable: &dyn AnyObservable) {
        self.counter.set(self.counter.get() + 1);
    }

    fn end_update(&self, _observable: &dyn AnyObservable) {
        if self.counter.get() == 1
            && let Some(h) = &self.handle
        {
            h();
        }
        self.counter.set(self.counter.get() - 1);
    }

    fn handle_possible_change(&self, _observable: &dyn AnyObservable) {}

    fn handle_change(&self, _observable: &dyn AnyObservable, _change: &dyn Any) {}
}

/// `derivedObservableWithCache(owner, computeFn)`.
pub fn derived_observable_with_cache<T: Clone + StrictEquals + 'static>(
    owner: Option<DebugOwner>,
    compute_fn: impl Fn(&dyn IReader, Option<&T>) -> T + 'static,
) -> IObservable<T> {
    let last_value: RefCell<Option<T>> = RefCell::new(None);
    derived_opts(DerivedOptions { owner, ..DerivedOptions::default() }, move |reader| {
        let last = last_value.borrow().clone();
        let v = compute_fn(reader, last.as_ref());
        *last_value.borrow_mut() = Some(v.clone());
        v
    })
}

/// O que `derivedObservableWithWritableCache` devolve: o observável mais `clearCache` e `setCache`.
pub struct DerivedWithWritableCache<T: Clone + 'static> {
    pub observable: IObservable<T>,
    last_value: Rc<RefCell<Option<T>>>,
    on_change: Rc<ObservableSignal<()>>,
}

impl<T: Clone + 'static> DerivedWithWritableCache<T> {
    pub fn clear_cache(&self, tx: &dyn ITransaction) {
        *self.last_value.borrow_mut() = None;
        self.on_change.trigger(Some(tx), ());
    }

    pub fn set_cache(&self, new_value: Option<T>, tx: Option<&dyn ITransaction>) {
        *self.last_value.borrow_mut() = new_value;
        self.on_change.trigger(tx, ());
    }
}

/// `derivedObservableWithWritableCache(owner, computeFn)`.
pub fn derived_observable_with_writable_cache<T: Clone + StrictEquals + 'static>(
    compute_fn: impl Fn(&dyn IReader, Option<&T>) -> T + 'static,
) -> DerivedWithWritableCache<T> {
    let last_value: Rc<RefCell<Option<T>>> = Rc::new(RefCell::new(None));
    let on_change = observable_signal::<()>("derivedObservableWithWritableCache");
    let (lv, oc) = (last_value.clone(), on_change.clone());
    let observable = derived(move |reader| {
        oc.read(reader);
        let last = lv.borrow().clone();
        let v = compute_fn(reader, last.as_ref());
        *lv.borrow_mut() = Some(v.clone());
        v
    });
    DerivedWithWritableCache { observable, last_value, on_change }
}

struct CacheEntry<O> {
    out: O,
    store: Rc<DisposableStore>,
}

struct ArrayMap<I: Clone, O: Clone, K: PartialEq + Clone> {
    cache: RefCell<Vec<(K, CacheEntry<O>)>>,
    items: RefCell<Rc<Vec<O>>>,
    map: Rc<dyn Fn(&I, &Rc<DisposableStore>) -> O>,
    key_selector: Rc<dyn Fn(&I) -> K>,
}

impl<I: Clone, O: Clone, K: PartialEq + Clone> ArrayMap<I, O, K> {
    fn dispose(&self) {
        let entries: Vec<Rc<DisposableStore>> = self.cache.borrow().iter().map(|(_, e)| e.store.clone()).collect();
        for s in entries {
            s.dispose();
        }
        self.cache.borrow_mut().clear();
    }

    fn set_items(&self, items: &[I]) {
        let mut new_items = Vec::new();
        let mut items_to_remove: Vec<K> = self.cache.borrow().iter().map(|(k, _)| k.clone()).collect();
        for item in items {
            let key = (self.key_selector)(item);
            let existing = self.cache.borrow().iter().find(|(k, _)| *k == key).map(|(_, e)| e.out.clone());
            let out = match existing {
                Some(out) => {
                    items_to_remove.retain(|k| *k != key);
                    out
                }
                None => {
                    let store = DisposableStore::new();
                    let out = (self.map)(item, &store);
                    self.cache.borrow_mut().push((key, CacheEntry { out: out.clone(), store }));
                    out
                }
            };
            new_items.push(out);
        }
        for key in items_to_remove {
            let entry_store = self.cache.borrow().iter().find(|(k, _)| *k == key).map(|(_, e)| e.store.clone());
            if let Some(s) = entry_store {
                s.dispose();
            }
            self.cache.borrow_mut().retain(|(k, _)| *k != key);
        }
        *self.items.borrow_mut() = Rc::new(new_items);
    }
}

/// `mapObservableArrayCached(owner, items, map, keySelector)`. Diferença: depois de o observável ficar
/// sem observadores, o original recria o `ArrayMap` sem o `keySelector` (chave = o próprio item); aqui o
/// seletor é mantido, porque o tipo da chave é fixo.
pub fn map_observable_array_cached_by<I: Clone + 'static, O: Clone + StrictEquals + 'static, K: PartialEq + Clone + 'static>(
    owner: Option<DebugOwner>,
    items: IObservable<Rc<Vec<I>>>,
    map: impl Fn(&I, &Rc<DisposableStore>) -> O + 'static,
    key_selector: impl Fn(&I) -> K + 'static,
) -> IObservable<Rc<Vec<O>>> {
    let new_map = {
        let map: Rc<dyn Fn(&I, &Rc<DisposableStore>) -> O> = Rc::new(map);
        let key_selector: Rc<dyn Fn(&I) -> K> = Rc::new(key_selector);
        move || ArrayMap { cache: RefCell::new(Vec::new()), items: RefCell::new(Rc::new(Vec::new())), map: map.clone(), key_selector: key_selector.clone() }
    };
    let m = Rc::new(RefCell::new(Rc::new(new_map())));
    let m_reset = m.clone();
    derived_opts(
        DerivedOptions {
            owner,
            debug_name: Some(DebugNameSource::Name("mapObservableArrayCached".into())),
            on_last_observer_removed: Some(Rc::new(move || {
                m_reset.borrow().dispose();
                let fresh = Rc::new(new_map());
                *m_reset.borrow_mut() = fresh;
            })),
            ..DerivedOptions::default()
        },
        move |reader| {
            let i = items.read(reader);
            let current = m.borrow().clone();
            current.set_items(&i);
            #[allow(clippy::let_and_return)]
            let out = current.items.borrow().clone();
            out
        },
    )
}

/// `mapObservableArrayCached` com a chave padrão (o próprio item).
pub fn map_observable_array_cached<I: Clone + PartialEq + 'static, O: Clone + StrictEquals + 'static>(
    owner: Option<DebugOwner>,
    items: IObservable<Rc<Vec<I>>>,
    map: impl Fn(&I, &Rc<DisposableStore>) -> O + 'static,
) -> IObservable<Rc<Vec<O>>> {
    map_observable_array_cached_by(owner, items, map, I::clone)
}
