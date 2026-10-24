//! Porte de `src/vs/workbench/contrib/testing/common/observableValue.ts`: um valor com evento de mudança, simples
//! (não é o `observable` do `base`), e a versão mutável que pode se ligar a um [`StoredValue`].
//!
//! Divergências:
//! - **`T`** exige [`StrictEquals`]: o `v !== this._value` do original é a igualdade estrita do JavaScript (primitivos
//!   por valor, objetos por identidade, o `Rc`).
//! - **`value`** é `value()` e `set_value(v)`; **`IObservableValue<T>`** é o trait [`IObservableValue`].
//! - **`MutableObservableValue extends Disposable`:** o `Disposable` fica no campo `base`.
//! - **`MutableObservableValue.stored(stored, defaultValue)`** recebe o `StoredValue` já criado (o `StoredValue` do
//!   original é um `Disposable` que o valor registra para descartar).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::observable::StrictEquals;

use super::stored_value::StoredValue;

/// `IObservableValue<T>`.
pub trait IObservableValue<T: 'static> {
    fn on_did_change(&self) -> Event<T>;
    fn value(&self) -> T;
}

/// `staticObservableValue(value)`: o valor nunca muda.
pub struct StaticObservableValue<T> {
    value: T,
}

impl<T: Clone + 'static> IObservableValue<T> for StaticObservableValue<T> {
    fn on_did_change(&self) -> Event<T> {
        Event::none()
    }

    fn value(&self) -> T {
        self.value.clone()
    }
}

pub fn static_observable_value<T: Clone + 'static>(value: T) -> Rc<dyn IObservableValue<T>> {
    Rc::new(StaticObservableValue { value })
}

pub struct MutableObservableValue<T: Clone + StrictEquals + 'static> {
    base: Rc<Disposable>,
    change_emitter: Rc<Emitter<T>>,
    value: RefCell<T>,
}

impl<T: Clone + StrictEquals + 'static> MutableObservableValue<T> {
    pub fn new(value: T) -> Rc<Self> {
        let base = Disposable::new();
        let change_emitter = Emitter::new();
        base.register(change_emitter.clone());
        Rc::new(Self { base, change_emitter, value: RefCell::new(value) })
    }

    pub fn on_did_change(&self) -> Event<T> {
        self.change_emitter.event()
    }

    pub fn value(&self) -> T {
        self.value.borrow().clone()
    }

    pub fn set_value(&self, v: T) {
        if !v.strict_equals(&self.value.borrow()) {
            *self.value.borrow_mut() = v.clone();
            self.change_emitter.fire(v);
        }
    }

    pub fn stored(stored: Rc<StoredValue<T>>, default_value: T) -> Rc<Self> {
        let o = Self::new(stored.get_or(default_value));
        o.base.register(stored.clone());
        let to_store = stored;
        o.base.register(o.on_did_change().listen(move |value| to_store.store(value.clone())));
        o
    }
}

impl<T: Clone + StrictEquals + 'static> IObservableValue<T> for MutableObservableValue<T> {
    fn on_did_change(&self) -> Event<T> {
        MutableObservableValue::on_did_change(self)
    }

    fn value(&self) -> T {
        MutableObservableValue::value(self)
    }
}

impl<T: Clone + StrictEquals + 'static> IDisposable for MutableObservableValue<T> {
    fn dispose(&self) {
        self.base.dispose();
    }
}
