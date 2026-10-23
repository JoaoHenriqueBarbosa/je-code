//! Porte de `observableInternal/observables/baseObservable.ts`: o conjunto de observadores com os
//! ganchos `onFirstObserverAdded`/`onLastObserverRemoved` e os métodos de conveniência.
//!
//! O `BaseObservable` do original é uma classe base; aqui é um valor que o observável guarda, e o dono
//! passa a si mesmo como [`ObservableHooks`] em `add_observer`/`remove_observer`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use super::derived::{DerivedOptions, derived_opts};
use crate::lifecycle::DisposableStore;
use crate::observable::base::{AnyObservable, IObservable, IReader, ObservableRef, ObserverRef, Observable, observer_id};
use crate::observable::debug_name::DebugNameSource;
use crate::observable::deps::{LiveSet, StrictEquals, for_each_live};
use crate::observable::utils::utils::{keep_observed, recompute_initially_and_on_change};

/// Os ganchos protegidos do `BaseObservable`.
pub trait ObservableHooks {
    fn on_first_observer_added(&self) {}
    fn on_last_observer_removed(&self) {}
}

/// `BaseObservable`.
pub struct BaseObservable {
    observers: RefCell<LiveSet<ObserverRef>>,
    me: Weak<dyn AnyObservable>,
}

impl BaseObservable {
    /// `me` é o `Weak` do próprio observável (de `Rc::new_cyclic`).
    pub fn new(me: Weak<dyn AnyObservable>) -> Self {
        Self { observers: RefCell::new(LiveSet::default()), me }
    }

    pub fn to_any_observable(&self) -> ObservableRef {
        self.me.upgrade().expect("o observável está vivo enquanto um método dele roda")
    }

    /// `addObserver`.
    pub fn add_observer(&self, observer: ObserverRef, hooks: &dyn ObservableHooks) {
        let len = self.observers.borrow().len();
        self.observers.borrow_mut().add(observer_id(&observer), observer);
        if len == 0 {
            hooks.on_first_observer_added();
        }
    }

    /// `removeObserver`.
    pub fn remove_observer(&self, observer: &ObserverRef, hooks: &dyn ObservableHooks) {
        let deleted = self.observers.borrow_mut().delete(observer_id(observer));
        if deleted && self.observers.borrow().is_empty() {
            hooks.on_last_observer_removed();
        }
    }

    pub fn has_observer(&self, observer: &ObserverRef) -> bool {
        self.observers.borrow().has(observer_id(observer))
    }

    pub fn observer_count(&self) -> usize {
        self.observers.borrow().len()
    }

    /// `[...this._observers]`.
    pub fn observers(&self) -> Vec<ObserverRef> {
        self.observers.borrow().snapshot()
    }

    /// `for (const o of this._observers)`, com a iteração viva do `Set`.
    pub fn for_each_observer(&self, mut f: impl FnMut(&ObserverRef)) {
        for_each_live(&self.observers, |o| {
            f(&o);
            true
        });
    }

    /// `debugGetObservers()`.
    pub fn debug_get_observers(&self) -> Vec<ObserverRef> {
        self.observers()
    }
}

/// Os métodos de `ConvenientObservable` que criam outros observáveis.
pub trait ObservableExt<T: Clone + 'static> {
    /// `map(fn)`. Sem dono, o nome de debug é `<nome> (mapped)`.
    fn map<U: Clone + StrictEquals + 'static>(&self, f: impl Fn(T, &dyn IReader) -> U + 'static) -> IObservable<U>;

    /// `recomputeInitiallyAndOnChange(store, handleValue)`.
    fn recompute_initially_and_on_change(&self, store: &DisposableStore, handle_value: Option<Rc<dyn Fn(&T)>>) -> IObservable<T>;

    /// `keepObserved(store)`.
    fn keep_observed(&self, store: &DisposableStore) -> IObservable<T>;
}

impl<T: Clone + 'static, O: Observable<T> + ?Sized> ObservableExt<T> for O {
    fn map<U: Clone + StrictEquals + 'static>(&self, f: impl Fn(T, &dyn IReader) -> U + 'static) -> IObservable<U> {
        let this = self.to_observable();
        let name_source = this.clone();
        derived_opts(
            DerivedOptions {
                debug_name: Some(DebugNameSource::Lazy(Rc::new(move || Some(format!("{} (mapped)", name_source.debug_name()))))),
                ..DerivedOptions::default()
            },
            move |reader| f(this.read(reader), reader),
        )
    }

    fn recompute_initially_and_on_change(&self, store: &DisposableStore, handle_value: Option<Rc<dyn Fn(&T)>>) -> IObservable<T> {
        let this = self.to_observable();
        store.add(recompute_initially_and_on_change(&this, handle_value));
        this
    }

    fn keep_observed(&self, store: &DisposableStore) -> IObservable<T> {
        let this = self.to_observable();
        store.add(keep_observed(&this));
        this
    }
}

/// `flatten()`: um observável de observável vira o observável do valor.
pub trait FlattenExt<U: Clone + 'static> {
    fn flatten(&self) -> IObservable<U>;
}

impl<U: Clone + StrictEquals + 'static, O: Observable<IObservable<U>> + ?Sized> FlattenExt<U> for O {
    fn flatten(&self) -> IObservable<U> {
        let this = self.to_observable();
        let name_source = this.clone();
        derived_opts(
            DerivedOptions {
                debug_name: Some(DebugNameSource::Lazy(Rc::new(move || Some(format!("{} (flattened)", name_source.debug_name()))))),
                ..DerivedOptions::default()
            },
            move |reader| this.read(reader).read(reader),
        )
    }
}

/// Os ganchos vazios, para quem não sobrescreve `onFirstObserverAdded`/`onLastObserverRemoved`.
pub struct NoHooks;

impl ObservableHooks for NoHooks {}
