//! Porte de `src/vs/workbench/contrib/testing/common/storedValue.ts`: um valor tipado guardado no `IStorageService`,
//! com cache em memória e serialização configurável (JSON por padrão).
//!
//! Divergências:
//! - **`get()` e `get(defaultValue)`** são [`StoredValue::get`] (`Option<T>`) e [`StoredValue::get_or`] (`T`). Como no
//!   original, o padrão fica guardado no cache.
//! - **Serialização padrão.** `JSON.parse` e `JSON.stringify` sobre um `T` qualquer são o trait [`StoredJson`], que cada
//!   tipo guardado implementa; [`StoredValue::new`] usa essa forma e [`StoredValue::with_serialization`] a
//!   serialização do chamador (`options.serialization`). Um texto que não é JSON da forma de `T` lança, como o
//!   `JSON.parse`.
//! - **`this._store`** (o `DisposableStore` protegido da base `Disposable`) é um store próprio, registrado no `Disposable`.
//! - **`StoredValue extends Disposable`:** o `Disposable` fica no campo `base`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{Error, throw};
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_platform::storage::{StorageScope, StorageService, StorageTarget, StorageValueChangeEvent};
use je_storage::StorageValue;
use serde_json::Value;

/// `IStoredValueSerialization<T>`.
pub struct IStoredValueSerialization<T> {
    pub deserialize: Rc<dyn Fn(&str) -> T>,
    pub serialize: Rc<dyn Fn(&T) -> String>,
}

impl<T> Clone for IStoredValueSerialization<T> {
    fn clone(&self) -> Self {
        Self { deserialize: self.deserialize.clone(), serialize: self.serialize.clone() }
    }
}

/// A forma JSON de um valor guardado: o que `JSON.parse(d)` e `JSON.stringify(d)` fariam com ele.
pub trait StoredJson: Sized {
    fn to_json(&self) -> Value;
    /// `None` quando o JSON não tem a forma de `Self`.
    fn from_json(value: &Value) -> Option<Self>;
}

impl StoredJson for bool {
    fn to_json(&self) -> Value {
        Value::Bool(*self)
    }

    fn from_json(value: &Value) -> Option<Self> {
        value.as_bool()
    }
}

/// `defaultSerialization`.
pub fn default_serialization<T: StoredJson + 'static>() -> IStoredValueSerialization<T> {
    IStoredValueSerialization {
        deserialize: Rc::new(|data| match serde_json::from_str::<Value>(data).ok().and_then(|value| T::from_json(&value)) {
            Some(value) => value,
            None => throw(Error::named("SyntaxError", format!("Unexpected stored value: {data}"))),
        }),
        serialize: Rc::new(|data| data.to_json().to_string()),
    }
}

/// `IStoredValueOptions<T>` sem a serialização.
pub struct StoredValueOptions {
    pub key: String,
    pub scope: StorageScope,
    pub target: StorageTarget,
}

/// todo@connor4312: is this worthy to be in common?
pub struct StoredValue<T: Clone + 'static> {
    base: Rc<Disposable>,
    serialization: IStoredValueSerialization<T>,
    key: String,
    scope: StorageScope,
    target: StorageTarget,
    value: RefCell<Option<T>>,
    storage: Rc<dyn StorageService>,

    /// Emitted whenever the value is updated or deleted.
    on_did_change: Event<StorageValueChangeEvent>,
}

impl<T: Clone + 'static> StoredValue<T> {
    /// `new StoredValue(options, storage)` com a serialização JSON padrão.
    pub fn new(options: StoredValueOptions, storage: Rc<dyn StorageService>) -> Rc<Self>
    where
        T: StoredJson,
    {
        Self::with_serialization(options, default_serialization::<T>(), storage)
    }

    /// `new StoredValue({ ...options, serialization }, storage)`.
    pub fn with_serialization(options: StoredValueOptions, serialization: IStoredValueSerialization<T>, storage: Rc<dyn StorageService>) -> Rc<Self> {
        let base = Disposable::new();
        let store = DisposableStore::new();
        base.register(store.clone());
        let on_did_change = storage.on_did_change_value(options.scope, Some(&options.key), &store);

        Rc::new(Self {
            base,
            serialization,
            key: options.key,
            scope: options.scope,
            target: options.target,
            value: RefCell::new(None),
            storage,
            on_did_change,
        })
    }

    pub fn on_did_change(&self) -> Event<StorageValueChangeEvent> {
        self.on_did_change.clone()
    }

    /// Reads the value, returning the undefined if it's not set.
    pub fn get(&self) -> Option<T> {
        self.get_with_default(None)
    }

    /// Reads the value, returning the default value if it's not set.
    pub fn get_or(&self, default_value: T) -> T {
        self.get_with_default(Some(default_value)).expect("com o valor padrão, o valor sempre existe")
    }

    fn get_with_default(&self, default_value: Option<T>) -> Option<T> {
        if self.value.borrow().is_none() {
            let value = self.storage.get(&self.key, self.scope, None);
            let resolved = match value {
                None => default_value,
                Some(text) => Some((self.serialization.deserialize)(&text)),
            };
            *self.value.borrow_mut() = resolved;
        }

        self.value.borrow().clone()
    }

    /// Persists changes to the value.
    pub fn store(&self, value: T) {
        let serialized = (self.serialization.serialize)(&value);
        *self.value.borrow_mut() = Some(value);
        self.storage.store(&self.key, StorageValue::String(serialized), self.scope, self.target);
    }

    /// Delete an element stored under the provided key from storage.
    pub fn delete(&self) {
        self.storage.remove(&self.key, self.scope);
    }
}

impl<T: Clone + 'static> IDisposable for StoredValue<T> {
    fn dispose(&self) {
        self.base.dispose();
    }
}
