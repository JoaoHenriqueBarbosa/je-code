//! Porte de `observableInternal/map.ts`: um `Map` cujo observável avisa a cada mudança.
//!
//! O valor do observável é o próprio mapa ([`MapData`], como o `this` do original); a igualdade é sempre
//! falsa, então toda mudança notifica. A ordem de inserção é a do `Map` do JS.

use std::cell::RefCell;
use std::rc::Rc;

use super::base::{IObservable, ISettableObservable, ITransaction};
use super::deps::StrictEquals;
use super::observables::observable_value::{ObservableValueOptions, observable_value_opts};

/// O conteúdo do mapa, o valor do observável.
pub struct MapData<K, V> {
    data: RefCell<Vec<(K, V)>>,
}

impl<K: PartialEq + Clone, V: Clone> MapData<K, V> {
    pub fn size(&self) -> usize {
        self.data.borrow().len()
    }

    pub fn has(&self, key: &K) -> bool {
        self.data.borrow().iter().any(|(k, _)| k == key)
    }

    pub fn get(&self, key: &K) -> Option<V> {
        self.data.borrow().iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    pub fn entries(&self) -> Vec<(K, V)> {
        self.data.borrow().clone()
    }

    pub fn keys(&self) -> Vec<K> {
        self.data.borrow().iter().map(|(k, _)| k.clone()).collect()
    }

    pub fn values(&self) -> Vec<V> {
        self.data.borrow().iter().map(|(_, v)| v.clone()).collect()
    }
}

/// `ObservableMap<K, V>`.
pub struct ObservableMap<K: PartialEq + Clone + 'static, V: Clone + StrictEquals + 'static> {
    data: Rc<MapData<K, V>>,
    obs: ISettableObservable<Rc<MapData<K, V>>>,
    /// `observable`.
    pub observable: IObservable<Rc<MapData<K, V>>>,
}

impl<K: PartialEq + Clone + 'static, V: Clone + StrictEquals + 'static> Default for ObservableMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: PartialEq + Clone + 'static, V: Clone + StrictEquals + 'static> ObservableMap<K, V> {
    pub fn new() -> Self {
        let data = Rc::new(MapData { data: RefCell::new(Vec::new()) });
        let obs = observable_value_opts::<Rc<MapData<K, V>>, ()>(
            ObservableValueOptions { equals_fn: Some(Rc::new(|_, _| false)), ..ObservableValueOptions::default() },
            data.clone(),
        );
        let observable: IObservable<Rc<MapData<K, V>>> = obs.clone();
        Self { data, obs, observable }
    }

    pub fn size(&self) -> usize {
        self.data.size()
    }

    pub fn has(&self, key: &K) -> bool {
        self.data.has(key)
    }

    pub fn get(&self, key: &K) -> Option<V> {
        self.data.get(key)
    }

    pub fn data(&self) -> &Rc<MapData<K, V>> {
        &self.data
    }

    /// `set(key, value, tx)`: só notifica se a chave é nova ou o valor mudou.
    pub fn set(&self, key: K, value: V, tx: Option<&dyn ITransaction>) {
        let old = self.data.get(&key);
        let changed = match &old {
            None => true,
            Some(o) => !o.strict_equals(&value),
        };
        if changed {
            let mut d = self.data.data.borrow_mut();
            match d.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => d.push((key, value)),
            }
            drop(d);
            self.obs.set_with_change(self.data.clone(), tx, ());
        }
    }

    pub fn delete(&self, key: &K, tx: Option<&dyn ITransaction>) -> bool {
        let len = self.data.data.borrow().len();
        self.data.data.borrow_mut().retain(|(k, _)| k != key);
        let result = self.data.data.borrow().len() != len;
        if result {
            self.obs.set_with_change(self.data.clone(), tx, ());
        }
        result
    }

    pub fn clear(&self, tx: Option<&dyn ITransaction>) {
        if self.data.size() > 0 {
            self.data.data.borrow_mut().clear();
            self.obs.set_with_change(self.data.clone(), tx, ());
        }
    }
}
