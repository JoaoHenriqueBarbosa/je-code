//! Porte de `observableInternal/observables/observableSignalFromEvent.ts`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use super::base_observable::{BaseObservable, ObservableHooks};
use crate::event::Event;
use crate::lifecycle::DisposableRef;
use crate::observable::base::{AnyObservable, IObservable, Observable, ObserverRef};
use crate::observable::debug_name::{DebugNameData, DebugOwner};
use crate::observable::transaction::transaction;

/// `observableSignalFromEvent(owner | name, event)`.
pub fn observable_signal_from_event<E: 'static>(name: &str, event: Event<E>) -> IObservable<()> {
    FromEventObservableSignal::new(name.to_owned(), event)
}

/// `observableSignalFromEvent(owner, event)`.
pub fn observable_signal_from_event_owned<E: 'static>(owner: DebugOwner, event: Event<E>) -> IObservable<()> {
    let name = DebugNameData::new(Some(owner), None).get_debug_name().unwrap_or_else(|| "Observable Signal From Event".into());
    FromEventObservableSignal::new(name, event)
}

struct FromEventObservableSignal<E: 'static> {
    base: BaseObservable,
    me: Weak<Self>,
    debug_name: String,
    event: Event<E>,
    subscription: RefCell<Option<DisposableRef>>,
}

impl<E: 'static> FromEventObservableSignal<E> {
    fn new(debug_name: String, event: Event<E>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            base: BaseObservable::new(me.clone() as Weak<dyn AnyObservable>),
            me: me.clone(),
            debug_name,
            event,
            subscription: RefCell::new(None),
        })
    }

    fn handle_event(&self) {
        transaction(|tx| {
            let me = self.base.to_any_observable();
            self.base.for_each_observer(|o| {
                tx.update_observer(o.clone(), me.clone());
                o.handle_change(&*me, &());
            });
        });
    }
}

impl<E: 'static> ObservableHooks for FromEventObservableSignal<E> {
    fn on_first_observer_added(&self) {
        let me = self.me.clone();
        let sub = self.event.listen(move |_| {
            if let Some(this) = me.upgrade() {
                this.handle_event();
            }
        });
        *self.subscription.borrow_mut() = Some(sub);
    }

    fn on_last_observer_removed(&self) {
        let sub = self.subscription.borrow_mut().take();
        if let Some(sub) = sub {
            sub.dispose();
        }
    }
}

impl<E: 'static> AnyObservable for FromEventObservableSignal<E> {
    fn report_changes(&self) {}

    fn add_observer(&self, observer: ObserverRef) {
        self.base.add_observer(observer, self);
    }

    fn remove_observer(&self, observer: &ObserverRef) {
        self.base.remove_observer(observer, self);
    }

    fn debug_name(&self) -> String {
        self.debug_name.clone()
    }

    fn to_any_observable(&self) -> Rc<dyn AnyObservable> {
        self.base.to_any_observable()
    }
}

impl<E: 'static> Observable<()> for FromEventObservableSignal<E> {
    fn get(&self) {}

    fn to_observable(&self) -> IObservable<()> {
        self.me.upgrade().expect("vivo")
    }
}

use crate::lifecycle::IDisposable;
