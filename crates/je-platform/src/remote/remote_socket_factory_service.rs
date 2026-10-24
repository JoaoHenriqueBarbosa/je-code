//! Porte de `src/vs/platform/remote/common/remoteSocketFactoryService.ts`: o serviço que guarda, por tipo de conexão,
//! as fábricas de socket e abre a conexão pela primeira que a suporta.
//!
//! Divergências:
//! - `ISocketFactory<T extends RemoteConnectionType>` recebe um [`RemoteConnection`] (o enum): o `RemoteConnectionOfType<T>`
//!   do original é o estreitamento por tipo, e o serviço só consulta as fábricas registradas para o tipo da conexão.
//!   O tipo `T` do `register` é o [`RemoteConnectionType`] passado.
//! - `ISocket` é o trait de `je-base`, devolvido como `Rc<dyn ISocket>`.
//! - `throw new Error(...)` de `connect` é [`errors::throw`] (pânico com o `Error`).
//! - `declare readonly _serviceBrand` é o identificador [`I_REMOTE_SOCKET_FACTORY_SERVICE`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::parts::ipc::net::ISocket;
use je_base::promise::Promise;
use je_instantiation::service_id;

use super::remote_authority_resolver::{RemoteConnection, RemoteConnectionType};

/// `ISocketFactory`.
pub trait ISocketFactory {
    fn supports(&self, connect_to: &RemoteConnection) -> bool;

    fn connect(&self, connect_to: &RemoteConnection, path: &str, query: &str, debug_label: &str) -> Promise<Rc<dyn ISocket>>;
}

/// `IRemoteSocketFactoryService`.
pub trait IRemoteSocketFactoryService {
    /// Register a socket factory for the given message passing type.
    fn register(&self, r#type: RemoteConnectionType, factory: Rc<dyn ISocketFactory>) -> DisposableRef;

    fn connect(&self, connect_to: &RemoteConnection, path: &str, query: &str, debug_label: &str) -> Promise<Rc<dyn ISocket>>;
}

service_id!(pub I_REMOTE_SOCKET_FACTORY_SERVICE: dyn IRemoteSocketFactoryService = "remoteSocketFactoryService");

type Factories = Rc<RefCell<HashMap<RemoteConnectionType, Vec<Rc<dyn ISocketFactory>>>>>;

/// `RemoteSocketFactoryService`.
#[derive(Default)]
pub struct RemoteSocketFactoryService {
    factories: Factories,
}

impl RemoteSocketFactoryService {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    fn get_socket_factory(&self, message_passing: &RemoteConnection) -> Option<Rc<dyn ISocketFactory>> {
        let factories = self.factories.borrow().get(&message_passing.connection_type()).cloned().unwrap_or_default();
        factories.into_iter().find(|factory| factory.supports(message_passing))
    }
}

impl IRemoteSocketFactoryService for RemoteSocketFactoryService {
    fn register(&self, r#type: RemoteConnectionType, factory: Rc<dyn ISocketFactory>) -> DisposableRef {
        self.factories.borrow_mut().entry(r#type).or_default().push(factory.clone());
        let factories = self.factories.clone();
        to_disposable(move || {
            if let Some(list) = factories.borrow_mut().get_mut(&r#type)
                && let Some(idx) = list.iter().position(|candidate| Rc::ptr_eq(candidate, &factory))
            {
                list.remove(idx);
            }
        })
    }

    fn connect(&self, connect_to: &RemoteConnection, path: &str, query: &str, debug_label: &str) -> Promise<Rc<dyn ISocket>> {
        let Some(socket_factory) = self.get_socket_factory(connect_to) else {
            errors::throw(Error::new(format!("No socket factory found for {connect_to}")))
        };
        socket_factory.connect(connect_to, path, query, debug_label)
    }
}
