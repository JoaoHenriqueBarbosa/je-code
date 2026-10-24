//! Porte de `src/vs/workbench/api/browser/mainThreadManagedSockets.ts`: o lado do workbench do RPC
//! `ExtHostManagedSockets`, que registra fábricas de socket cujo transporte vive no extension host
//! (`$registerSocketFactory`) e repassa os dados, o fim e o fechamento dos sockets remotos.
//!
//! Divergências:
//! - **Injeção.** O construtor `(extHostContext, @IRemoteSocketFactoryService)` é o [`Injectable`]: o argumento estático
//!   é o [`IMainContext`] (o `IExtHostContext`, que dá o `getProxy`) e a dependência é o serviço de fábricas.
//! - **`@extHostNamedCustomer(MainContext.MainThreadManagedSockets)`** é o registro do `extHostCustomers.ts`, que não foi
//!   portado: a classe é criada por `create_instance`.
//! - **`class extends ManagedSocket`.** `MainThreadManagedSocket` é o tipo sem estado com a função `connect`; o socket
//!   devolvido é o [`ManagedSocket`] com o backend `MainThreadManagedSocketBackend` (que escreve, termina e drena pelo
//!   proxy), e `ManagedSocket::new` é o `super(debugLabel, half)` do construtor privado.
//! - **A fábrica.** A `new class implements ISocketFactory` é o `ExtHostSocketFactory`, que guarda os campos que o
//!   original captura (`that`, `store`, `socketFactoryId`). O `connectTo: ManagedRemoteConnection` é o [`RemoteConnection`]:
//!   uma conexão que não é `Managed` cai no `reject(new Error('Invalid connectTo'))`.
//! - **`async` sem `await`** (`$registerSocketFactory`, `$unregisterSocketFactory`): fazem o trabalho já e devolvem a
//!   promessa resolvida.
//! - **`.then(socketId => ...).catch(reject)`** é o `then` seguido do `then_result` que rejeita no erro; o
//!   `.then(socket => ..., err => ...)` é um único `then_result` com os dois ramos.
//! - **`error ? new Error(error) : undefined`:** o texto vazio é falso em JavaScript, então vira "sem erro".

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::errors::Error;
use je_base::lifecycle::{Disposable, DisposableMap, DisposableStore, IDisposable};
use je_base::parts::ipc::net::{ISocket, SocketCloseEvent};
use je_base::promise::Promise;
use je_instantiation::Injectable;
use je_platform::remote::managed_socket::{ManagedSocket, ManagedSocketBackend, RemoteSocketHalf, connect_managed_socket};
use je_platform::remote::remote_authority_resolver::{RemoteConnection, RemoteConnectionType};
use je_platform::remote::remote_socket_factory_service::{IRemoteSocketFactoryService, ISocketFactory};

use crate::api::common::ext_host_protocol::{ExtHostContext, ExtHostManagedSocketsShape, IMainContext, MainThreadManagedSocketsShape};

type RemoteSockets = Rc<RefCell<HashMap<u64, RemoteSocketHalf>>>;

/// `MainThreadManagedSockets`.
pub struct MainThreadManagedSockets {
    base: Rc<Disposable>,
    proxy: Rc<dyn ExtHostManagedSocketsShape>,
    registrations: Rc<DisposableMap<u64, DisposableStore>>,
    remote_sockets: RemoteSockets,
    remote_socket_factory_service: Rc<dyn IRemoteSocketFactoryService>,
}

impl Injectable for MainThreadManagedSockets {
    type Args = Rc<dyn IMainContext>;
    type Deps = (Rc<dyn IRemoteSocketFactoryService>,);

    fn create(ext_host_context: Self::Args, (remote_socket_factory_service,): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();
        let registrations = DisposableMap::<u64, DisposableStore>::new();
        base.register(registrations.clone());
        Rc::new(Self {
            base,
            proxy: ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_MANAGED_SOCKETS),
            registrations,
            remote_sockets: Rc::new(RefCell::new(HashMap::new())),
            remote_socket_factory_service,
        })
    }
}

impl IDisposable for MainThreadManagedSockets {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// A `new class implements ISocketFactory<RemoteConnectionType.Managed>` de `$registerSocketFactory`.
struct ExtHostSocketFactory {
    socket_factory_id: u64,
    proxy: Rc<dyn ExtHostManagedSocketsShape>,
    remote_sockets: RemoteSockets,
    store: Rc<DisposableStore>,
}

impl ISocketFactory for ExtHostSocketFactory {
    fn supports(&self, connect_to: &RemoteConnection) -> bool {
        matches!(connect_to, RemoteConnection::Managed(managed) if managed.id == self.socket_factory_id)
    }

    fn connect(&self, connect_to: &RemoteConnection, path: &str, query: &str, debug_label: &str) -> Promise<Rc<dyn ISocket>> {
        let proxy = self.proxy.clone();
        let remote_sockets = self.remote_sockets.clone();
        let store = self.store.clone();
        let socket_factory_id = self.socket_factory_id;
        let connect_to = connect_to.clone();
        let (path, query, debug_label) = (path.to_string(), query.to_string(), debug_label.to_string());
        Promise::with_executor(move |resolver| {
            let factory_id = match &connect_to {
                RemoteConnection::Managed(managed) if managed.id == socket_factory_id => managed.id,
                _ => return resolver.reject(Error::new("Invalid connectTo")),
            };

            let on_error = resolver.clone();
            proxy
                .rpc_open_remote_socket(factory_id)
                .then({
                    let proxy = proxy.clone();
                    move |socket_id| {
                        let half = RemoteSocketHalf::new();
                        remote_sockets.borrow_mut().insert(socket_id, half.clone());

                        MainThreadManagedSocket::connect(socket_id, proxy, &path, &query, &debug_label, half).then_result(move |result| {
                            match result {
                                Ok(socket) => {
                                    let remote_sockets_on_dispose = remote_sockets.clone();
                                    store.add(socket.on_did_dispose().once().listen(move |_| {
                                        remote_sockets_on_dispose.borrow_mut().remove(&socket_id);
                                    }));
                                    let socket: Rc<dyn ISocket> = socket;
                                    resolver.resolve(socket);
                                }
                                Err(err) => {
                                    remote_sockets.borrow_mut().remove(&socket_id);
                                    resolver.reject(err);
                                }
                            }
                            Ok(())
                        });
                    }
                })
                .then_result(move |result| {
                    if let Err(error) = result {
                        on_error.reject(error);
                    }
                    Ok(())
                });
        })
    }
}

impl MainThreadManagedSocketsShape for MainThreadManagedSockets {
    fn rpc_register_socket_factory(&self, socket_factory_id: u64) -> Promise<()> {
        let store = DisposableStore::new();
        let socket_factory = Rc::new(ExtHostSocketFactory {
            socket_factory_id,
            proxy: self.proxy.clone(),
            remote_sockets: self.remote_sockets.clone(),
            store: store.clone(),
        });
        store.add(self.remote_socket_factory_service.register(RemoteConnectionType::Managed, socket_factory));
        self.registrations.set(socket_factory_id, store, false);
        Promise::resolved(())
    }

    fn rpc_unregister_socket_factory(&self, socket_factory_id: u64) -> Promise<()> {
        self.registrations.delete_and_dispose(&socket_factory_id);
        Promise::resolved(())
    }

    fn rpc_on_did_managed_socket_have_data(&self, socket_id: u64, data: VSBuffer) {
        let half = self.remote_sockets.borrow().get(&socket_id).cloned();
        if let Some(half) = half {
            half.on_data.fire(data);
        }
    }

    fn rpc_on_did_managed_socket_close(&self, socket_id: u64, error: Option<String>) {
        let half = self.remote_sockets.borrow().get(&socket_id).cloned();
        if let Some(half) = half {
            let error = error.filter(|error| !error.is_empty());
            half.on_close.fire(Some(SocketCloseEvent::Node { had_error: error.is_some(), error: error.map(Error::new) }));
        }
        self.remote_sockets.borrow_mut().remove(&socket_id);
    }

    fn rpc_on_did_managed_socket_end(&self, socket_id: u64) {
        let half = self.remote_sockets.borrow().get(&socket_id).cloned();
        if let Some(half) = half {
            half.on_end.fire(());
        }
    }
}

/// Os métodos que `MainThreadManagedSocket` sobrescreve: `write`, `closeRemote` e `drain`, repassados ao proxy.
struct MainThreadManagedSocketBackend {
    socket_id: u64,
    proxy: Rc<dyn ExtHostManagedSocketsShape>,
}

impl ManagedSocketBackend for MainThreadManagedSocketBackend {
    fn write(&self, buffer: VSBuffer) {
        self.proxy.rpc_remote_socket_write(self.socket_id, buffer);
    }

    fn close_remote(&self) {
        self.proxy.rpc_remote_socket_end(self.socket_id);
    }

    fn drain(&self) -> Promise<()> {
        self.proxy.rpc_remote_socket_drain(self.socket_id)
    }
}

/// `MainThreadManagedSocket`.
pub struct MainThreadManagedSocket;

impl MainThreadManagedSocket {
    pub fn connect(socket_id: u64, proxy: Rc<dyn ExtHostManagedSocketsShape>, path: &str, query: &str, debug_label: &str, half: RemoteSocketHalf) -> Promise<Rc<ManagedSocket>> {
        let socket = ManagedSocket::new(debug_label, half.clone(), Rc::new(MainThreadManagedSocketBackend { socket_id, proxy }));
        connect_managed_socket(socket, path, query, debug_label, half)
    }
}
