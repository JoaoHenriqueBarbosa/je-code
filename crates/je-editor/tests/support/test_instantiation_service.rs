//! Porte de `src/vs/platform/instantiation/test/common/instantiationServiceMock.ts`: o `TestInstantiationService`.
//!
//! Divergências:
//! - **Herança**: `TestInstantiationService extends InstantiationService` vira composição com `Deref` para o
//!   [`InstantiationService`], então `create_instance`, `invoke_function` e o resto chamam direto.
//! - **sinon**: `mock`, `stubInstance`, `stubPromise`, `spy`, `createServices` e as formas de `stub` com
//!   propriedade e valor dependem do sinon (espiões, `createStubInstance`, substituição de método), que não tem
//!   equivalente aqui. `stub(id, instance)` guarda o objeto pronto na coleção, que é o que a forma com objeto do
//!   original faz (`_createStub` devolve o próprio objeto).
//! - **`get(id)`** passa pelo `invoke_function`, porque `_getOrCreateServiceInstance` não é público.
//! - **`dispose`**: o original chama `sinon.restore()` (nada a restaurar aqui) e só descarta o serviço de
//!   instanciação com `properDispose`.

use std::ops::Deref;
use std::rc::Rc;

use je_instantiation::{InstantiationOptions, InstantiationService, ServiceCollection, ServiceEntry, ServiceId};
use je_instantiation::je_base::lifecycle::IDisposable;

/// `TestInstantiationService`.
pub struct TestInstantiationService {
    service_collection: ServiceCollection,
    instantiation_service: Rc<InstantiationService>,
    proper_dispose: bool,
}

impl TestInstantiationService {
    /// `new TestInstantiationService()`.
    pub fn new() -> Rc<Self> {
        Self::with_options(ServiceCollection::new(), false, None, false)
    }

    /// `new TestInstantiationService(serviceCollection, strict, parent, properDispose)`.
    pub fn with_options(service_collection: ServiceCollection, strict: bool, parent: Option<Rc<InstantiationService>>, proper_dispose: bool) -> Rc<Self> {
        let instantiation_service =
            InstantiationService::with_options(service_collection.clone(), InstantiationOptions { strict, parent, enable_tracing: false });
        Rc::new(Self { service_collection, instantiation_service, proper_dispose })
    }

    /// `get(service)`.
    pub fn get<T: ?Sized + 'static>(&self, id: ServiceId<T>) -> Rc<T> {
        self.instantiation_service.invoke_function(|accessor| accessor.get(id))
    }

    /// `set(service, instance)`: devolve a entrada anterior, se havia.
    pub fn set<T: ?Sized + 'static>(&self, id: ServiceId<T>, instance: Rc<T>) -> Option<ServiceEntry<T>> {
        self.service_collection.set(id, instance)
    }

    /// `stub(service, obj)`: registra o objeto como o serviço e o devolve.
    pub fn stub<T: ?Sized + 'static>(&self, id: ServiceId<T>, service: Rc<T>) -> Rc<T> {
        self.service_collection.set(id, service.clone());
        service
    }

    /// `createChild(services)`.
    pub fn create_child(&self, services: ServiceCollection) -> Rc<TestInstantiationService> {
        Self::with_options(services, false, Some(self.instantiation_service.clone()), false)
    }
}

impl Deref for TestInstantiationService {
    type Target = InstantiationService;

    fn deref(&self) -> &InstantiationService {
        &self.instantiation_service
    }
}

impl IDisposable for TestInstantiationService {
    fn dispose(&self) {
        if self.proper_dispose {
            self.instantiation_service.dispose();
        }
    }
}
