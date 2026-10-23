//! Porte de `extensions.ts`: o registro global de singletons.
//!
//! O registro é por thread (o thread da UI), como o módulo do original é por processo.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::{self, Error};

use crate::descriptors::SyncDescriptor;
use crate::instantiation::{ServiceId, ServiceImpl};
use crate::service_collection::ServiceCollection;

/// `InstantiationType`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstantiationType {
    /// Cria o serviço assim que um consumidor depende dele.
    Eager = 0,
    /// Cria o serviço quando o consumidor o usa (precisa do proxy no identificador).
    Delayed = 1,
}

/// Uma entrada de `getSingletonServiceDescriptors()`: o `[id, descriptor]`, instalável numa coleção.
#[derive(Clone)]
pub struct SingletonRegistration {
    name: &'static str,
    install: Rc<dyn Fn(&ServiceCollection)>,
}

impl SingletonRegistration {
    pub fn service_name(&self) -> &'static str {
        self.name
    }

    /// `services.set(id, descriptor)`.
    pub fn install(&self, services: &ServiceCollection) {
        (self.install)(services);
    }
}

thread_local! {
    static REGISTRY: RefCell<Vec<SingletonRegistration>> = const { RefCell::new(Vec::new()) };
}

/// `registerSingleton(id, ctor, supportsDelayedInstantiation)`.
pub fn register_singleton<C: ServiceImpl<Args = ()>>(
    id: ServiceId<C::Service>,
    instantiation_type: InstantiationType,
) {
    let descriptor = SyncDescriptor::<C::Service>::new::<C>(());
    register_singleton_descriptor(
        id,
        if instantiation_type == InstantiationType::Delayed {
            descriptor.delayed()
        } else {
            descriptor
        },
    );
}

/// `registerSingleton(id, descriptor)`.
pub fn register_singleton_descriptor<T: ?Sized + 'static>(
    id: ServiceId<T>,
    descriptor: SyncDescriptor<T>,
) {
    if descriptor.supports_delayed_instantiation() && id.proxy.is_none() {
        errors::throw(Error::new(format!(
            "service '{id}' was registered as delayed but its identifier declares no proxy (see delayed_proxy!)"
        )));
    }
    REGISTRY.with(|registry| {
        registry.borrow_mut().push(SingletonRegistration {
            name: id.name(),
            install: Rc::new(move |services| {
                services.set(id, descriptor.clone());
            }),
        });
    });
}

/// `getSingletonServiceDescriptors()`.
pub fn get_singleton_service_descriptors() -> Vec<SingletonRegistration> {
    REGISTRY.with(|registry| registry.borrow().clone())
}
