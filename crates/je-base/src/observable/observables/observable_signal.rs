//! Porte de `observableInternal/observables/observableSignal.ts`.

use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use super::base_observable::{BaseObservable, NoHooks};
use crate::observable::base::{AnyObservable, IObservable, ITransaction, Observable, ObserverRef};
use crate::observable::debug_name::{DebugNameData, DebugNameOptions, DebugOwner};
use crate::observable::transaction::subtransaction;

/// `observableSignal(debugName)`.
pub fn observable_signal<C: 'static>(debug_name: &str) -> Rc<ObservableSignal<C>> {
    ObservableSignal::new(DebugNameOptions::named(debug_name))
}

/// `observableSignal(owner)`.
pub fn observable_signal_owned<C: 'static>(owner: DebugOwner) -> Rc<ObservableSignal<C>> {
    ObservableSignal::new(DebugNameOptions::owned(owner))
}

/// `IObservableSignal<TChange>`: um observável sem valor que leva um delta ao ser disparado.
pub struct ObservableSignal<C: 'static> {
    base: BaseObservable,
    me: Weak<Self>,
    debug: DebugNameData,
    _c: PhantomData<fn(C)>,
}

impl<C: 'static> ObservableSignal<C> {
    fn new(options: DebugNameOptions) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            base: BaseObservable::new(me.clone() as Weak<dyn AnyObservable>),
            me: me.clone(),
            debug: options.into(),
            _c: PhantomData,
        })
    }

    /// `trigger(tx, change)`.
    pub fn trigger(&self, tx: Option<&dyn ITransaction>, change: C) {
        subtransaction(tx, |tx| {
            let me = self.base.to_any_observable();
            self.base.for_each_observer(|o| {
                tx.update_observer(o.clone(), me.clone());
                o.handle_change(&*me, &change);
            });
        });
    }
}

impl<C: 'static> AnyObservable for ObservableSignal<C> {
    fn report_changes(&self) {}

    fn add_observer(&self, observer: ObserverRef) {
        self.base.add_observer(observer, &NoHooks);
    }

    fn remove_observer(&self, observer: &ObserverRef) {
        self.base.remove_observer(observer, &NoHooks);
    }

    fn debug_name(&self) -> String {
        self.debug.get_debug_name().unwrap_or_else(|| "Observable Signal".into())
    }

    fn to_any_observable(&self) -> Rc<dyn AnyObservable> {
        self.base.to_any_observable()
    }
}

impl<C: 'static> Observable<()> for ObservableSignal<C> {
    fn get(&self) {}

    fn to_observable(&self) -> IObservable<()> {
        self.me.upgrade().expect("vivo")
    }
}
