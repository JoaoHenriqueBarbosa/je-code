//! Porte de `observableInternal/utils/valueWithChangeEvent.ts`.
//!
//! O atalho `value instanceof ValueWithChangeEventFromObservable` de `observableFromValueWithChangeEvent`
//! não existe sobre um trait object; use [`ValueWithChangeEventFromObservable::observable`] diretamente.

use std::rc::Rc;

use crate::event::{Event, ValueWithChangeEventLike};
use crate::observable::base::IObservable;
use crate::observable::debug_name::DebugOwner;
use crate::observable::deps::StrictEquals;
use crate::observable::observables::observable_from_event::observable_from_event_owned;

/// `ValueWithChangeEventFromObservable<T>`.
pub struct ValueWithChangeEventFromObservable<T: Clone + 'static> {
    pub observable: IObservable<T>,
}

impl<T: Clone + 'static> ValueWithChangeEventLike<T> for ValueWithChangeEventFromObservable<T> {
    fn on_did_change(&self) -> Event<()> {
        Event::<()>::from_observable_light(self.observable.clone())
    }

    fn value(&self) -> T {
        self.observable.get()
    }
}

/// `observableFromValueWithChangeEvent(owner, value)`.
pub fn observable_from_value_with_change_event<T: Clone + StrictEquals + 'static>(owner: DebugOwner, value: Rc<dyn ValueWithChangeEventLike<T>>) -> IObservable<T> {
    let event = value.on_did_change();
    observable_from_event_owned(owner, event, move |_| value.value())
}
