//! Porte de `observableInternal/reactions/autorun.ts`.

use std::cell::RefCell;
use std::rc::Rc;

use super::autorun_impl::AutorunObserver;
use crate::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use crate::observable::base::{IObservable, IReader, IReaderWithStore, ISettableObservable, ObservableRef, SettableExt};
use crate::observable::change_tracker::IChangeTracker;
use crate::observable::debug_name::{DebugNameData, DebugNameOptions};
use crate::observable::deps::StrictEquals;
use crate::observable::observables::observable_value::observable_value;
use crate::observable::transaction::transaction;

/// `autorun(fn)`: roda já e de novo sempre que uma dependência muda. Precisa ser descartado.
pub fn autorun(f: impl Fn(&dyn IReaderWithStore) + 'static) -> DisposableRef {
    AutorunObserver::new_untracked(DebugNameData::default(), f)
}

/// `autorunOpts(options, fn)`.
pub fn autorun_opts(options: DebugNameOptions, f: impl Fn(&dyn IReaderWithStore) + 'static) -> DisposableRef {
    AutorunObserver::new_untracked(options.into(), f)
}

/// `autorunHandleChanges(options, fn)`.
pub fn autorun_handle_changes<S: 'static>(
    options: DebugNameOptions,
    change_tracker: Rc<dyn IChangeTracker<S>>,
    f: impl Fn(&dyn IReader, &S) + 'static,
) -> DisposableRef {
    AutorunObserver::new_tracked(options.into(), move |r, s| f(r, s), change_tracker)
}

/// `autorunWithStoreHandleChanges(options, fn)`.
pub fn autorun_with_store_handle_changes<S: 'static>(
    options: DebugNameOptions,
    change_tracker: Rc<dyn IChangeTracker<S>>,
    f: impl Fn(&dyn IReader, &S, &DisposableStore) + 'static,
) -> DisposableRef {
    let store = DisposableStore::new();
    let s = store.clone();
    let disposable = autorun_handle_changes(options, change_tracker, move |reader, summary| {
        s.clear();
        f(reader, summary, &s);
    });
    to_disposable(move || {
        disposable.dispose();
        store.dispose();
    })
}

/// `autorunWithStore(fn)`.
pub fn autorun_with_store(f: impl Fn(&dyn IReader, &DisposableStore) + 'static) -> DisposableRef {
    let store = DisposableStore::new();
    let s = store.clone();
    let disposable = autorun_opts(DebugNameOptions::default(), move |reader| {
        s.clear();
        f(reader, &s);
    });
    to_disposable(move || {
        disposable.dispose();
        store.dispose();
    })
}

/// O argumento de `autorunDelta`.
pub struct Delta<T> {
    pub last_value: Option<T>,
    pub new_value: T,
}

/// `autorunDelta(observable, handler)`.
pub fn autorun_delta<T: Clone + 'static>(observable: IObservable<T>, handler: impl Fn(Delta<T>) + 'static) -> DisposableRef {
    let last: RefCell<Option<T>> = RefCell::new(None);
    autorun_opts(DebugNameOptions::default(), move |reader| {
        let new_value = observable.read(reader);
        let last_value = last.replace(Some(new_value.clone()));
        handler(Delta { last_value, new_value });
    })
}

/// O argumento de `autorunIterableDelta`.
pub struct IterableDelta<T> {
    pub added_values: Vec<T>,
    pub removed_values: Vec<T>,
}

/// `autorunIterableDelta(getValue, handler, getUniqueIdentifier)`.
pub fn autorun_iterable_delta_by<T: Clone + 'static, K: PartialEq + Clone + 'static>(
    get_value: impl Fn(&dyn IReader) -> Vec<T> + 'static,
    handler: impl Fn(IterableDelta<T>) + 'static,
    get_unique_identifier: impl Fn(&T) -> K + 'static,
) -> DisposableRef {
    let last_values: RefCell<Vec<(K, T)>> = RefCell::new(Vec::new());
    autorun_opts(DebugNameOptions::default(), move |reader| {
        let mut new_values: Vec<(K, T)> = Vec::new();
        let mut removed_values: Vec<(K, T)> = last_values.borrow().clone();
        for value in get_value(reader) {
            let id = get_unique_identifier(&value);
            if last_values.borrow().iter().any(|(k, _)| *k == id) {
                removed_values.retain(|(k, _)| *k != id);
            } else {
                // `Map.set` mantém a posição se a chave já existe.
                match new_values.iter_mut().find(|(k, _)| *k == id) {
                    Some(slot) => slot.1 = value.clone(),
                    None => new_values.push((id.clone(), value.clone())),
                }
                let mut lv = last_values.borrow_mut();
                match lv.iter_mut().find(|(k, _)| *k == id) {
                    Some(slot) => slot.1 = value,
                    None => lv.push((id, value)),
                }
            }
        }
        for (id, _) in &removed_values {
            last_values.borrow_mut().retain(|(k, _)| k != id);
        }
        if !new_values.is_empty() || !removed_values.is_empty() {
            handler(IterableDelta {
                added_values: new_values.into_iter().map(|(_, v)| v).collect(),
                removed_values: removed_values.into_iter().map(|(_, v)| v).collect(),
            });
        }
    })
}

/// `autorunIterableDelta(getValue, handler)` com o identificador padrão (o próprio valor).
pub fn autorun_iterable_delta<T: Clone + PartialEq + 'static>(
    get_value: impl Fn(&dyn IReader) -> Vec<T> + 'static,
    handler: impl Fn(IterableDelta<T>) + 'static,
) -> DisposableRef {
    autorun_iterable_delta_by(get_value, handler, T::clone)
}

struct Cell_<T: Clone + StrictEquals + 'static> {
    value: ISettableObservable<T>,
    store: Rc<DisposableStore>,
}

/// `autorunPerKeyedItem(items, keyFn, setup)`.
pub fn autorun_per_keyed_item<T: Clone + StrictEquals + 'static, K: PartialEq + Clone + 'static>(
    items: IObservable<Rc<Vec<T>>>,
    key_fn: impl Fn(&T) -> K + 'static,
    setup: impl Fn(&K, IObservable<T>, &Rc<DisposableStore>) + 'static,
) -> DisposableRef {
    let cells: Rc<RefCell<Vec<(K, Rc<Cell_<T>>)>>> = Rc::new(RefCell::new(Vec::new()));
    let cells_in = cells.clone();
    let ar = autorun_opts(DebugNameOptions::default(), move |reader| {
        let arr = items.read(reader);
        let mut seen: Vec<K> = Vec::new();
        let additions: RefCell<Vec<(K, Rc<Cell_<T>>)>> = RefCell::new(Vec::new());
        transaction(|tx| {
            for item in arr.iter() {
                let key = key_fn(item);
                seen.push(key.clone());
                let existing = cells_in.borrow().iter().find(|(k, _)| *k == key).map(|(_, c)| c.clone());
                match existing {
                    Some(cell) => cell.value.set(item.clone(), Some(tx)),
                    None => {
                        let store = DisposableStore::new();
                        let value: ISettableObservable<T> = observable_value::<T, ()>("keyedItem", item.clone());
                        let cell = Rc::new(Cell_ { value, store });
                        cells_in.borrow_mut().push((key.clone(), cell.clone()));
                        additions.borrow_mut().push((key, cell));
                    }
                }
            }
            let stale: Vec<(K, Rc<Cell_<T>>)> = cells_in.borrow().iter().filter(|(k, _)| !seen.contains(k)).cloned().collect();
            for (k, cell) in stale {
                cell.store.dispose();
                cells_in.borrow_mut().retain(|(key, _)| *key != k);
            }
        });
        // O setup roda depois da transação, para os autoruns de cada chave verem os valores finais.
        for (key, cell) in additions.into_inner() {
            let value: IObservable<T> = cell.value.clone();
            setup(&key, value, &cell.store);
        }
    });
    to_disposable(move || {
        ar.dispose();
        let all: Vec<_> = cells.borrow().iter().map(|(_, c)| c.clone()).collect();
        for cell in all {
            cell.store.dispose();
        }
        cells.borrow_mut().clear();
    })
}

/// `IReaderWithDispose`.
pub trait IReaderWithDispose: IReaderWithStore + IDisposable {}

struct SelfDisposableReader<'a> {
    reader: &'a dyn IReaderWithStore,
    store: Rc<DisposableStore>,
    delayed_store: Rc<DisposableStore>,
    dispose: Rc<dyn Fn()>,
}

impl IReader for SelfDisposableReader<'_> {
    fn read_observable_erased(&self, observable: ObservableRef, get: &mut dyn FnMut()) {
        self.reader.read_observable_erased(observable, get);
    }
}

impl IReaderWithStore for SelfDisposableReader<'_> {
    fn store(&self) -> Rc<DisposableStore> {
        self.store.clone()
    }

    fn delayed_store(&self) -> Rc<DisposableStore> {
        self.delayed_store.clone()
    }
}

impl IDisposable for SelfDisposableReader<'_> {
    fn dispose(&self) {
        (self.dispose)();
    }
}

impl IReaderWithDispose for SelfDisposableReader<'_> {}

/// `autorunSelfDisposable(fn)`: o leitor tem `dispose`, que descarta o próprio autorun.
pub fn autorun_self_disposable(f: impl Fn(&dyn IReaderWithDispose) + 'static) -> DisposableRef {
    let ar: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
    let disposed = Rc::new(std::cell::Cell::new(false));
    let (ar_in, disposed_in) = (ar.clone(), disposed.clone());
    let created = autorun(move |reader| {
        let (ar2, disposed2) = (ar_in.clone(), disposed_in.clone());
        f(&SelfDisposableReader {
            reader,
            delayed_store: reader.delayed_store(),
            store: reader.store(),
            dispose: Rc::new(move || {
                let current = ar2.borrow().clone();
                if let Some(a) = current {
                    a.dispose();
                }
                disposed2.set(true);
            }),
        });
    });
    *ar.borrow_mut() = Some(created.clone());
    if disposed.get() {
        created.dispose();
    }
    created
}

/// `registerAutorunSelfDisposable(store, fn)`.
pub fn register_autorun_self_disposable(store: Rc<DisposableStore>, f: impl Fn(&dyn IReaderWithDispose) + 'static) {
    let ar: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
    let dispose_sync = Rc::new(std::cell::Cell::new(false));
    let (ar_in, sync_in, store_in) = (ar.clone(), dispose_sync.clone(), store.clone());
    let created = autorun(move |reader| {
        let (ar2, sync2, store2) = (ar_in.clone(), sync_in.clone(), store_in.clone());
        f(&SelfDisposableReader {
            reader,
            delayed_store: reader.delayed_store(),
            store: reader.store(),
            dispose: Rc::new(move || {
                let current = ar2.borrow().clone();
                match current {
                    // Descartou na primeira execução: o `ar` ainda não foi atribuído.
                    None => sync2.set(true),
                    // Descartou numa reação: o `ar` já está registrado.
                    Some(a) => store2.delete(&a),
                }
            }),
        });
    });
    *ar.borrow_mut() = Some(created.clone());
    if dispose_sync.get() {
        created.dispose();
    } else {
        store.add(created);
    }
}
