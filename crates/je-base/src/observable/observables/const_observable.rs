//! Porte de `observableInternal/observables/constObservable.ts`.

use std::rc::{Rc, Weak};

use crate::observable::base::{AnyObservable, IObservable, Observable, ObserverRef};

/// `constObservable(value)`.
pub fn const_observable<T: Clone + 'static>(value: T) -> IObservable<T> {
    Rc::new_cyclic(|me| ConstObservable { value, me: me.clone() })
}

struct ConstObservable<T: Clone + 'static> {
    value: T,
    me: Weak<Self>,
}

impl<T: Clone + 'static> AnyObservable for ConstObservable<T> {
    fn report_changes(&self) {}

    fn add_observer(&self, _observer: ObserverRef) {}

    fn remove_observer(&self, _observer: &ObserverRef) {}

    fn debug_name(&self) -> String {
        "Const".into()
    }

    fn to_any_observable(&self) -> Rc<dyn AnyObservable> {
        self.me.upgrade().expect("vivo")
    }
}

impl<T: Clone + 'static> Observable<T> for ConstObservable<T> {
    fn get(&self) -> T {
        self.value.clone()
    }

    fn to_observable(&self) -> IObservable<T> {
        self.me.upgrade().expect("vivo")
    }
}
