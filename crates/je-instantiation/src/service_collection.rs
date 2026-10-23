//! Porte de `serviceCollection.ts`.
//!
//! A coleção é "viva": o serviço de instanciação guarda um handle (`Clone` barato, compartilhado) e
//! enxerga o que for registrado depois.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::errors::{self, Error};

use crate::descriptors::{ErasedDescriptor, SyncDescriptor};
use crate::instantiation::{ErasedInstance, ServiceId, ServiceKey, downcast};

/// `T | SyncDescriptor<T>`: uma instância pronta ou um descritor de criação preguiçosa.
pub enum ServiceEntry<T: ?Sized + 'static> {
    Instance(Rc<T>),
    Descriptor(SyncDescriptor<T>),
}

impl<T: ?Sized + 'static> From<Rc<T>> for ServiceEntry<T> {
    fn from(instance: Rc<T>) -> Self {
        Self::Instance(instance)
    }
}

impl<T: ?Sized + 'static> From<SyncDescriptor<T>> for ServiceEntry<T> {
    fn from(descriptor: SyncDescriptor<T>) -> Self {
        Self::Descriptor(descriptor)
    }
}

#[derive(Clone)]
pub(crate) enum Entry {
    Instance(ErasedInstance),
    Descriptor {
        typed: Rc<dyn std::any::Any>,
        erased: Rc<ErasedDescriptor>,
    },
}

impl Entry {
    fn into_typed<T: ?Sized + 'static>(self) -> ServiceEntry<T> {
        match self {
            Self::Instance(instance) => ServiceEntry::Instance(downcast(&instance)),
            Self::Descriptor { typed, .. } => ServiceEntry::Descriptor(
                typed
                    .downcast_ref::<SyncDescriptor<T>>()
                    .expect("descritor do tipo do identificador")
                    .clone(),
            ),
        }
    }
}

/// `ServiceCollection`.
#[derive(Clone, Default)]
pub struct ServiceCollection {
    entries: Rc<RefCell<HashMap<ServiceKey, Entry>>>,
}

impl ServiceCollection {
    pub fn new() -> Self {
        Self::default()
    }

    /// O construtor com entradas do original, encadeável: `new().with(a, x).with(b, y)`.
    #[must_use]
    pub fn with<T: ?Sized + 'static>(
        self,
        id: ServiceId<T>,
        entry: impl Into<ServiceEntry<T>>,
    ) -> Self {
        self.set(id, entry);
        self
    }

    /// `set(id, instanceOrDescriptor)`: devolve a entrada anterior, se havia.
    pub fn set<T: ?Sized + 'static>(
        &self,
        id: ServiceId<T>,
        entry: impl Into<ServiceEntry<T>>,
    ) -> Option<ServiceEntry<T>> {
        let entry = match entry.into() {
            ServiceEntry::Instance(instance) => Entry::Instance(Rc::new(instance)),
            ServiceEntry::Descriptor(descriptor) => {
                if descriptor.supports_delayed_instantiation() && id.proxy.is_none() {
                    errors::throw(Error::new(format!(
                        "service '{id}' was registered as delayed but its identifier declares no proxy (see delayed_proxy!)"
                    )));
                }
                let erased = Rc::new(descriptor.erase(id));
                Entry::Descriptor {
                    typed: Rc::new(descriptor),
                    erased,
                }
            }
        };
        self.entries
            .borrow_mut()
            .insert(id.key(), entry)
            .map(Entry::into_typed)
    }

    pub fn has<T: ?Sized + 'static>(&self, id: ServiceId<T>) -> bool {
        self.entries.borrow().contains_key(&id.key())
    }

    pub fn get<T: ?Sized + 'static>(&self, id: ServiceId<T>) -> Option<ServiceEntry<T>> {
        self.entries
            .borrow()
            .get(&id.key())
            .cloned()
            .map(Entry::into_typed)
    }

    pub(crate) fn get_entry(&self, key: &ServiceKey) -> Option<Entry> {
        self.entries.borrow().get(key).cloned()
    }

    pub(crate) fn set_entry(&self, key: ServiceKey, entry: Entry) {
        self.entries.borrow_mut().insert(key, entry);
    }

    pub(crate) fn remove_entries(&self, keys: &[ServiceKey]) {
        let mut entries = self.entries.borrow_mut();
        for key in keys {
            entries.remove(key);
        }
    }
}
