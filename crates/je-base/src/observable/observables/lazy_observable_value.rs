//! Porte de `observableInternal/observables/lazyObservableValue.ts`.

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use super::base_observable::{BaseObservable, ObservableHooks};
use crate::observable::base::{
    AnyObservable, IObserver, ISettableObservable, ITransaction, Observable, ObserverRef, Settable, is_undefined_change,
};
use crate::observable::debug_name::DebugNameData;
use crate::observable::deps::EqualityComparer;
use crate::observable::transaction::subtransaction;

struct Hooks;
impl ObservableHooks for Hooks {}

/// `LazyObservableValue<T, TChange>`: avisa os observadores só quando alguém lê ou a transação termina.
pub struct LazyObservableValue<T: Clone + 'static, C: Clone + 'static = ()> {
    base: BaseObservable,
    me: Weak<Self>,
    debug: DebugNameData,
    value: RefCell<T>,
    equality: EqualityComparer<T>,
    is_up_to_date: Cell<bool>,
    deltas: RefCell<Vec<C>>,
    update_counter: Cell<usize>,
    _c: PhantomData<fn(C)>,
}

impl<T: Clone + 'static, C: Clone + 'static> LazyObservableValue<T, C> {
    pub fn new(debug: DebugNameData, initial: T, equality: EqualityComparer<T>) -> Rc<Self> {
        Rc::new_cyclic(|me| Self {
            base: BaseObservable::new(me.clone() as Weak<dyn AnyObservable>),
            me: me.clone(),
            debug,
            value: RefCell::new(initial),
            equality,
            is_up_to_date: Cell::new(true),
            deltas: RefCell::new(Vec::new()),
            update_counter: Cell::new(0),
            _c: PhantomData,
        })
    }

    fn update(&self) {
        if self.is_up_to_date.get() {
            return;
        }
        self.is_up_to_date.set(true);
        let me = self.base.to_any_observable();
        let deltas = std::mem::take(&mut *self.deltas.borrow_mut());
        if deltas.is_empty() {
            self.base.for_each_observer(|o| o.handle_change(&*me, &()));
        } else {
            for change in deltas {
                self.base.for_each_observer(|o| o.handle_change(&*me, &change));
            }
        }
    }

    fn begin_update(&self) {
        self.update_counter.set(self.update_counter.get() + 1);
        if self.update_counter.get() == 1 {
            let me = self.base.to_any_observable();
            self.base.for_each_observer(|o| o.begin_update(&*me));
        }
    }

    fn end_update(&self) {
        self.update_counter.set(self.update_counter.get() - 1);
        if self.update_counter.get() == 0 {
            self.update();
            // `end_update` pode mudar a lista de observadores.
            let me = self.base.to_any_observable();
            for o in self.base.observers() {
                o.end_update(&*me);
            }
        }
    }
}

/// O observador interno que o `set` registra na transação (`tx.updateObserver({...}, this)`).
struct TxObserver<T: Clone + 'static, C: Clone + 'static>(Weak<LazyObservableValue<T, C>>);

impl<T: Clone + 'static, C: Clone + 'static> IObserver for TxObserver<T, C> {
    fn begin_update(&self, _o: &dyn AnyObservable) {
        if let Some(v) = self.0.upgrade() {
            v.begin_update();
        }
    }

    fn end_update(&self, _o: &dyn AnyObservable) {
        if let Some(v) = self.0.upgrade() {
            v.end_update();
        }
    }

    fn handle_possible_change(&self, _o: &dyn AnyObservable) {}

    fn handle_change(&self, _o: &dyn AnyObservable, _c: &dyn std::any::Any) {}
}

impl<T: Clone + 'static, C: Clone + 'static> AnyObservable for LazyObservableValue<T, C> {
    fn report_changes(&self) {
        self.get();
    }

    fn add_observer(&self, observer: ObserverRef) {
        let should_call_begin_update = !self.base.has_observer(&observer) && self.update_counter.get() > 0;
        self.base.add_observer(observer.clone(), &Hooks);
        if should_call_begin_update {
            observer.begin_update(self);
        }
    }

    fn remove_observer(&self, observer: &ObserverRef) {
        let should_call_end_update = self.base.has_observer(observer) && self.update_counter.get() > 0;
        self.base.remove_observer(observer, &Hooks);
        if should_call_end_update {
            // Chamar depois de remover garante que `end_update` não é chamado duas vezes.
            observer.end_update(self);
        }
    }

    fn debug_name(&self) -> String {
        self.debug.get_debug_name().unwrap_or_else(|| "LazyObservableValue".into())
    }

    fn to_any_observable(&self) -> Rc<dyn AnyObservable> {
        self.base.to_any_observable()
    }
}

impl<T: Clone + 'static, C: Clone + 'static> Observable<T> for LazyObservableValue<T, C> {
    fn get(&self) -> T {
        self.update();
        self.value.borrow().clone()
    }

    fn to_observable(&self) -> Rc<dyn Observable<T>> {
        self.me.upgrade().expect("vivo")
    }
}

impl<T: Clone + 'static, C: Clone + 'static> Settable<T, C> for LazyObservableValue<T, C> {
    fn set_with_change(&self, value: T, tx: Option<&dyn ITransaction>, change: C) {
        let undefined = is_undefined_change(&change);
        if undefined && (self.equality)(&self.value.borrow(), &value) {
            return;
        }
        subtransaction(tx, |tx| {
            self.is_up_to_date.set(false);
            *self.value.borrow_mut() = value;
            if !undefined {
                self.deltas.borrow_mut().push(change);
            }
            let observer: ObserverRef = Rc::new(TxObserver(self.me.clone()));
            tx.update_observer(observer, self.base.to_any_observable());
            if self.update_counter.get() > 1 {
                // Já começou begin/end update: precisa chamar `handle_possible_change` à mão.
                let me = self.base.to_any_observable();
                self.base.for_each_observer(|o| o.handle_possible_change(&*me));
            }
        });
    }

    fn to_settable(&self) -> ISettableObservable<T, C> {
        self.me.upgrade().expect("vivo")
    }
}

