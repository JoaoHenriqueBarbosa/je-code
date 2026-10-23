//! Porte de `observableInternal/observables/observableValue.ts` e `observableValueOpts.ts`.

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use super::base_observable::{BaseObservable, NoHooks};
use super::lazy_observable_value::LazyObservableValue;
use crate::lifecycle::{DisposableRef, IDisposable, id_of};
use crate::observable::base::{
    AnyObservable, DebugInfo, ISettableObservable, ITransaction, Observable, ObserverRef, Settable, is_undefined_change,
};
use crate::observable::debug_name::{DebugNameData, DebugNameOptions};
use crate::observable::deps::{EqualityComparer, StrictEquals, strict_equals};
use crate::observable::logging::debug_get_dependency_graph::format_value;
use crate::observable::transaction::subtransaction;

/// `observableValue(name, initialValue)`: os observadores são avisados quando o valor muda.
pub fn observable_value<T: Clone + StrictEquals + 'static, C: 'static>(name: &str, initial_value: T) -> Rc<ObservableValue<T, C>> {
    ObservableValue::new(DebugNameOptions::named(name).into(), initial_value, strict_equals())
}

/// `observableValue(owner, initialValue)`.
pub fn observable_value_owned<T: Clone + StrictEquals + 'static, C: 'static>(
    owner: crate::observable::debug_name::DebugOwner,
    initial_value: T,
) -> Rc<ObservableValue<T, C>> {
    ObservableValue::new(DebugNameOptions::owned(owner).into(), initial_value, strict_equals())
}

/// As opções de `observableValueOpts`.
pub struct ObservableValueOptions<T> {
    pub debug: DebugNameOptions,
    pub equals_fn: Option<EqualityComparer<T>>,
    pub lazy: bool,
}

impl<T> Default for ObservableValueOptions<T> {
    fn default() -> Self {
        Self { debug: DebugNameOptions::default(), equals_fn: None, lazy: false }
    }
}

/// `observableValueOpts(options, initialValue)`.
pub fn observable_value_opts<T: Clone + StrictEquals + 'static, C: Clone + 'static>(options: ObservableValueOptions<T>, initial_value: T) -> ISettableObservable<T, C> {
    let eq = options.equals_fn.unwrap_or_else(strict_equals);
    if options.lazy {
        LazyObservableValue::<T, C>::new(options.debug.into(), initial_value, eq)
    } else {
        ObservableValue::<T, C>::new(options.debug.into(), initial_value, eq)
    }
}

type ValueSetter<T> = Box<dyn Fn(&RefCell<T>, T)>;

/// `ObservableValue<T, TChange>`.
pub struct ObservableValue<T: Clone + 'static, C: 'static = ()> {
    base: BaseObservable,
    me: Weak<Self>,
    debug: DebugNameData,
    value: RefCell<T>,
    equality: EqualityComparer<T>,
    /// O `_setValue` sobrescrito (o `DisposableObservableValue`).
    value_setter: Option<ValueSetter<T>>,
    _change: PhantomData<fn(C)>,
}

impl<T: Clone + 'static, C: 'static> ObservableValue<T, C> {
    pub fn new(debug: DebugNameData, initial: T, equality: EqualityComparer<T>) -> Rc<Self> {
        Self::build(debug, initial, equality, None)
    }

    fn build(debug: DebugNameData, initial: T, equality: EqualityComparer<T>, value_setter: Option<ValueSetter<T>>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            base: BaseObservable::new(me.clone() as Weak<dyn AnyObservable>),
            me: me.clone(),
            debug,
            value: RefCell::new(initial),
            equality,
            value_setter,
            _change: PhantomData,
        })
    }

    fn set_value(&self, new_value: T) {
        match &self.value_setter {
            Some(f) => f(&self.value, new_value),
            None => *self.value.borrow_mut() = new_value,
        }
    }

    /// `debugSetValue`.
    pub fn debug_set_value(&self, value: T) {
        *self.value.borrow_mut() = value;
    }
}

impl<T: Clone + 'static, C: 'static> AnyObservable for ObservableValue<T, C> {
    fn report_changes(&self) {
        self.get();
    }

    fn add_observer(&self, observer: ObserverRef) {
        self.base.add_observer(observer, &NoHooks);
    }

    fn remove_observer(&self, observer: &ObserverRef) {
        self.base.remove_observer(observer, &NoHooks);
    }

    fn debug_name(&self) -> String {
        self.debug.get_debug_name().unwrap_or_else(|| "ObservableValue".into())
    }

    fn to_any_observable(&self) -> Rc<dyn AnyObservable> {
        self.base.to_any_observable()
    }

    fn debug_info(&self) -> Option<DebugInfo> {
        Some(DebugInfo {
            source_id: self.id(),
            name: self.debug_name(),
            kind: "observableValue",
            value: format_value(&*self.value.borrow(), 50),
            state: "upToDate".into(),
            dependencies: Vec::new(),
            observers: self.base.observers().into_iter().map(crate::observable::base::DebugNode::Observer).collect(),
        })
    }
}

impl<T: Clone + 'static, C: 'static> Observable<T> for ObservableValue<T, C> {
    fn get(&self) -> T {
        self.value.borrow().clone()
    }

    fn to_observable(&self) -> Rc<dyn Observable<T>> {
        self.me.upgrade().expect("vivo")
    }
}

impl<T: Clone + 'static, C: 'static> Settable<T, C> for ObservableValue<T, C> {
    fn set_with_change(&self, value: T, tx: Option<&dyn ITransaction>, change: C) {
        if is_undefined_change(&change) && (self.equality)(&self.value.borrow(), &value) {
            return;
        }
        subtransaction(tx, |tx| {
            self.set_value(value);
            let me = self.base.to_any_observable();
            self.base.for_each_observer(|observer| {
                tx.update_observer(observer.clone(), me.clone());
                observer.handle_change(&*me, &change);
            });
        });
    }

    fn to_settable(&self) -> ISettableObservable<T, C> {
        self.me.upgrade().expect("vivo")
    }
}

/// `disposableObservableValue`: ao trocar o valor, o anterior é descartado; descartar o observável
/// descarta o valor.
pub fn disposable_observable_value<C: 'static>(name: &str, initial: Option<DisposableRef>) -> Rc<ObservableValue<Option<DisposableRef>, C>> {
    let setter: ValueSetter<Option<DisposableRef>> = Box::new(|cell, new_value| {
        let old = cell.borrow().clone();
        let same = match (&old, &new_value) {
            (Some(a), Some(b)) => id_of(a) == id_of(b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        if let Some(old) = old {
            old.dispose();
        }
        *cell.borrow_mut() = new_value;
    });
    ObservableValue::build(DebugNameOptions::named(name).into(), initial, strict_equals(), Some(setter))
}

impl<C: 'static> IDisposable for ObservableValue<Option<DisposableRef>, C> {
    fn dispose(&self) {
        let v = self.value.borrow().clone();
        if let Some(v) = v {
            v.dispose();
        }
    }
}
