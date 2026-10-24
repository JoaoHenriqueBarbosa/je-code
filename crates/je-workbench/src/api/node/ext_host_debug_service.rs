//! Porte de `src/vs/workbench/api/node/extHostDebugService.ts`: as sobrescritas do `ExtHostDebugService` do Node sobre a base
//! (`createDebugAdapter`, `daExecutableFromPackage`), no trait [`ExtHostDebugServiceHooks`] da base.
//!
//! Divergências:
//! - Os adaptadores concretos (`ExecutableDebugAdapter`, `SocketDebugAdapter`, `NamedPipeDebugAdapter`) precisam abrir
//!   processos e sockets, que o porte injeta: [`NodeDebugAdapterIo`] guarda o lançador de processos e os abridores de socket
//!   e de pipe (a camada de plataforma os implementa).
//! - `createSignService()` (`SignService`, o `vsda`) devolve `None`: o serviço de assinatura nativo não existe no porte.
//! - `$runInTerminal` (terminal integrado reutilizável, `DebugTerminalCollection`, terminal externo) NÃO foi portado: ele
//!   depende dos serviços de terminal do host de extensões (`IExtHostTerminalService`,
//!   `IExtHostTerminalShellIntegration`) e dos terminais externos, que não têm contrato no porte do host.

use std::rc::Rc;

use je_base::parts::ipc::node_net::INodeStream;

use crate::api::common::ext_host_debug_service::{DefaultExtHostDebugServiceHooks, ExtHostDebugServiceHooks, ExtHostDebugSession};
use crate::api::common::ext_host_debug_types::{DebugAdapterDescriptor, DebugAdapterExecutable};
use crate::contrib::debug::abstract_debug_adapter::AbstractDebugAdapter;
use crate::contrib::debug::debug_adapter::{ExecutableDebugAdapter, IProcessLauncher, NetworkDebugAdapter};
use crate::services::extensions::extension_description_registry::ExtensionDescriptionRegistry;
use crate::services::extensions::extension_description_registry::IReadOnlyExtensionDescriptionRegistry;

/// Abre um socket TCP (`host`, `port`, `on_connected`).
pub type OpenSocket = Rc<dyn Fn(&str, u16, Box<dyn FnOnce()>) -> Rc<dyn INodeStream>>;
/// Abre um pipe nomeado (`path`, `on_connected`).
pub type OpenPipe = Rc<dyn Fn(&str, Box<dyn FnOnce()>) -> Rc<dyn INodeStream>>;

/// A E/S do sistema que os adaptadores do Node usam.
#[derive(Clone)]
pub struct NodeDebugAdapterIo {
    pub launcher: Rc<dyn IProcessLauncher>,
    pub open_socket: OpenSocket,
    pub open_pipe: OpenPipe,
}

/// As sobrescritas de `ExtHostDebugService` (Node).
pub struct NodeExtHostDebugServiceHooks {
    io: NodeDebugAdapterIo,
    base: DefaultExtHostDebugServiceHooks,
}

impl NodeExtHostDebugServiceHooks {
    pub fn new(io: NodeDebugAdapterIo) -> Self {
        Self { io, base: DefaultExtHostDebugServiceHooks }
    }
}

impl ExtHostDebugServiceHooks for NodeExtHostDebugServiceHooks {
    fn create_debug_adapter(&self, adapter: &DebugAdapterDescriptor, session: &ExtHostDebugSession) -> Option<Rc<dyn AbstractDebugAdapter>> {
        match adapter {
            DebugAdapterDescriptor::Executable(executable) => {
                let adapter: Rc<dyn AbstractDebugAdapter> =
                    ExecutableDebugAdapter::new(self.convert_executable_to_dto(executable), &session.session_type(), self.io.launcher.clone());
                Some(adapter)
            }
            DebugAdapterDescriptor::Server(server) => {
                let adapter: Rc<dyn AbstractDebugAdapter> = NetworkDebugAdapter::socket(self.convert_server_to_dto(server), self.io.open_socket.clone());
                Some(adapter)
            }
            DebugAdapterDescriptor::NamedPipeServer(pipe) => {
                let adapter: Rc<dyn AbstractDebugAdapter> = NetworkDebugAdapter::named_pipe(self.convert_pipe_server_to_dto(pipe), self.io.open_pipe.clone());
                Some(adapter)
            }
            _ => self.base.create_debug_adapter(adapter, session),
        }
    }

    fn da_executable_from_package(&self, session: &ExtHostDebugSession, extension_registry: &ExtensionDescriptionRegistry) -> Option<DebugAdapterExecutable> {
        let descriptions: Vec<_> = extension_registry.get_all_extension_descriptions().iter().map(|description| (**description).clone()).collect();
        ExecutableDebugAdapter::platform_adapter_executable(&descriptions, &session.session_type())
            .map(|dae| DebugAdapterExecutable { command: dae.command, args: dae.args, options: dae.options })
    }
}
