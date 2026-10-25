//! Porte de `src/vs/platform/protocol/electron-main/protocol.ts`: o contrato do `IProtocolMainService`.
//!
//! Decisões de tradução:
//! - **`IIPCObjectUrl<T>` e `createIPCObjectUrl<T>()`** carregam o objeto pelo IPC, que serializa o valor: o `T` é [`Dynamic`], o
//!   valor que o `ipcRenderer.invoke` devolve (e que também faz do método um membro do objeto-trait).
//! - **`(url: URI) => Promise<NodeRemoteResourceResponse>`** é [`RequestRemoteResource`].

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_instantiation::service_id;

use crate::remote::electron_remote_resources::NodeRemoteResourceResponse;

/// O pedido de um recurso a um sistema de arquivos remoto: `(url: URI) => Promise<NodeRemoteResourceResponse>`.
pub type RequestRemoteResource = Rc<dyn Fn(URI) -> Promise<NodeRemoteResourceResponse>>;

/// `IIPCObjectUrl<T>`.
pub trait IIPCObjectUrl: IDisposable {
    /// A `URI` that a renderer can use to retrieve the
    /// object via `ipcRenderer.invoke(resource.toString())`.
    fn resource(&self) -> URI;

    /// Allows to update the value of the object after it
    /// has been created.
    ///
    /// `obj` é o objeto a deixar acessível para o renderizador.
    fn update(&self, obj: Dynamic);
}

/// `IProtocolMainService`.
pub trait IProtocolMainService {
    /// Allows to make an object accessible to a renderer
    /// via `ipcRenderer.invoke(resource.toString())`.
    fn create_ipc_object_url(&self) -> Rc<dyn IIPCObjectUrl>;

    /// Adds a path as root to the list of allowed
    /// resources for file access.
    ///
    /// `root` é o caminho a permitir para o acesso a arquivos.
    fn add_valid_file_root(&self, root: &str) -> DisposableRef;

    /// Registers the protocol used to load resources from a remote file system.
    fn register_managed_remote_resource_protocol(&self, request_remote_resource: RequestRemoteResource);
}

service_id!(pub I_PROTOCOL_MAIN_SERVICE: dyn IProtocolMainService = "protocolMainService");
