//! Porte de `src/vs/workbench/contrib/editTelemetry/browser/helpers/utils.ts`.
//!
//! Representação: o delta de `IObservableWithChange<T, TDelta>` não faz parte do tipo do observável neste porte
//! (ver `observable_workspace`); `map_observable_delta` e `iterate_observable_changes` recebem o tipo do delta como
//! parâmetro de tipo, que precisa ser o que o produtor passa em `set_with_change`.

use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use je_base::async_::AsyncIterableProducer;
use je_base::lifecycle::{DisposableStore, to_disposable};
use je_base::observable::{IObservable, Observable, StrictEquals, observable_value, run_on_change, transaction};
use je_base::observable::base::Settable;
use je_base::promise::{MaybePromise, Promise};

/// `sumByCategory(items, getValue, getCategory)`: a soma de `getValue` por categoria (só as categorias que aparecem).
pub fn sum_by_category<T, TCategory: Hash + Eq>(
    items: &[T],
    get_value: impl Fn(&T) -> i64,
    get_category: impl Fn(&T) -> TCategory,
) -> HashMap<TCategory, i64> {
    let mut acc: HashMap<TCategory, i64> = HashMap::new();
    for item in items {
        *acc.entry(get_category(item)).or_insert(0) += get_value(item);
    }
    acc
}

/// `mapObservableDelta(obs, mapFn, store)`: um observável com o mesmo valor e os deltas transformados por `map_fn`.
pub fn map_observable_delta<T, TDelta, TDeltaNew>(
    obs: &IObservable<T>,
    map_fn: impl Fn(&TDelta) -> TDeltaNew + 'static,
    store: &DisposableStore,
) -> IObservable<T>
where
    T: Clone + StrictEquals + 'static,
    TDelta: Clone + 'static,
    TDeltaNew: 'static,
{
    let obs_result = observable_value::<T, TDeltaNew>("mapped", obs.get());
    let target = obs_result.clone();
    store.add(run_on_change::<T, TDelta>(obs.clone(), move |value, _prev_value, changes| {
        transaction(|tx| {
            for c in changes {
                target.set_with_change(value.clone(), Some(tx), map_fn(c));
            }
        });
    }));
    obs_result.to_observable()
}

/// O item de `iterateObservableChanges`: `{ value, prevValue, change }`.
#[derive(Clone)]
pub struct ObservableChange<T, TChange> {
    pub value: T,
    pub prev_value: T,
    pub change: Vec<TChange>,
}

/// `iterateObservableChanges(obs, store)`.
pub fn iterate_observable_changes<T, TChange>(obs: &IObservable<T>, store: &Rc<DisposableStore>) -> Rc<AsyncIterableProducer<ObservableChange<T, TChange>>>
where
    T: Clone + 'static,
    TChange: Clone + 'static,
{
    let obs = obs.clone();
    let store = store.clone();
    AsyncIterableProducer::new(
        move |emitter| {
            if store.is_disposed() {
                return MaybePromise::Value(());
            }
            store.add(run_on_change::<T, TChange>(obs, move |value, prev_value, change| {
                emitter.emit_one(ObservableChange { value: value.clone(), prev_value: prev_value.clone(), change: change.to_vec() });
            }));

            let (promise, resolver) = Promise::<()>::new();
            store.add(to_disposable(move || resolver.resolve(())));
            MaybePromise::Promise(promise)
        },
        None,
    )
}
