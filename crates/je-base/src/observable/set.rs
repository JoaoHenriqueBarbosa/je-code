//! Porte de `observableInternal/set.ts`: um `Set` cujo observável avisa a cada mudança (ver `map.rs`).

use std::cell::RefCell;
use std::rc::Rc;

use super::base::{IObservable, ISettableObservable, ITransaction};
use super::observables::observable_value::{ObservableValueOptions, observable_value_opts};

/// O conteúdo do conjunto, o valor do observável.
pub struct SetData<T> {
    data: RefCell<Vec<T>>,
}

impl<T: PartialEq + Clone> SetData<T> {
    pub fn size(&self) -> usize {
        self.data.borrow().len()
    }

    pub fn has(&self, value: &T) -> bool {
        self.data.borrow().contains(value)
    }

    pub fn values(&self) -> Vec<T> {
        self.data.borrow().clone()
    }
}

/// `ObservableSet<T>`.
pub struct ObservableSet<T: PartialEq + Clone + 'static> {
    data: Rc<SetData<T>>,
    obs: ISettableObservable<Rc<SetData<T>>>,
    /// `observable`.
    pub observable: IObservable<Rc<SetData<T>>>,
}

impl<T: PartialEq + Clone + 'static> Default for ObservableSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: PartialEq + Clone + 'static> ObservableSet<T> {
    pub fn new() -> Self {
        let data = Rc::new(SetData { data: RefCell::new(Vec::new()) });
        let obs = observable_value_opts::<Rc<SetData<T>>, ()>(
            ObservableValueOptions { equals_fn: Some(Rc::new(|_, _| false)), ..ObservableValueOptions::default() },
            data.clone(),
        );
        let observable: IObservable<Rc<SetData<T>>> = obs.clone();
        Self { data, obs, observable }
    }

    pub fn size(&self) -> usize {
        self.data.size()
    }

    pub fn has(&self, value: &T) -> bool {
        self.data.has(value)
    }

    pub fn data(&self) -> &Rc<SetData<T>> {
        &self.data
    }

    pub fn add(&self, value: T, tx: Option<&dyn ITransaction>) {
        if !self.data.has(&value) {
            self.data.data.borrow_mut().push(value);
            self.obs.set_with_change(self.data.clone(), tx, ());
        }
    }

    pub fn delete(&self, value: &T, tx: Option<&dyn ITransaction>) -> bool {
        let len = self.data.size();
        self.data.data.borrow_mut().retain(|v| v != value);
        let result = self.data.size() != len;
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
