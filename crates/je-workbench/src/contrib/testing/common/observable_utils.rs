//! Porte de `src/vs/workbench/contrib/testing/common/observableUtils.ts`: o `onObservableChange`, que escuta só os deltas
//! (`handleChange`) de um observável.
//!
//! Divergências:
//! - **`IObservableWithChange<unknown, T>`** é um [`AnyObservable`] cujo delta é do tipo `T`: o `change as unknown as T` do
//!   original é o `downcast_ref::<T>()` do delta apagado (`&dyn Any`); um delta de outro tipo é ignorado.
//! - **O callback** recebe o delta por referência.

use std::any::Any;
use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::observable::{AnyObservable, IObserver};

struct ChangeObserver<T: 'static> {
    callback: Rc<dyn Fn(&T)>,
}

impl<T: 'static> IObserver for ChangeObserver<T> {
    fn begin_update(&self, _observable: &dyn AnyObservable) {}

    fn end_update(&self, _observable: &dyn AnyObservable) {}

    fn handle_possible_change(&self, observable: &dyn AnyObservable) {
        observable.report_changes();
    }

    fn handle_change(&self, _observable: &dyn AnyObservable, change: &dyn Any) {
        if let Some(change) = change.downcast_ref::<T>() {
            (self.callback)(change);
        }
    }
}

/// `onObservableChange(observable, callback)`.
pub fn on_observable_change<T: 'static>(observable: &Rc<impl AnyObservable + ?Sized + 'static>, callback: impl Fn(&T) + 'static) -> DisposableRef {
    let o: Rc<dyn IObserver> = Rc::new(ChangeObserver::<T> { callback: Rc::new(callback) });
    observable.add_observer(o.clone());
    let observable = observable.clone();
    to_disposable(move || observable.remove_observer(&o))
}
