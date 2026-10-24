//! Recortes dos contratos de serviço que o serviço do manifesto da galeria e o da conta da galeria leem e que o porte
//! ainda não tem por inteiro: `ISharedProcessService` (`platform/ipc/electron-browser/services.ts`), o
//! `getConnection` do `IRemoteAgentService` (`workbench/services/remote/common/remoteAgentService.ts`) e o `restart` do
//! `IHostService` (`workbench/services/host/browser/host.ts`).
//!
//! Por que traits à parte e não mais métodos nos recortes que já existem
//! ([`crate::services::remote::remote_agent_service::IRemoteAgentService`] e [`super::service_contracts::IHostService`]):
//! cada recorte existente é implementado por dublês de outros testes, que só têm os membros que esses testes usam;
//! acrescentar membro obrigatório quebraria todos. Cada trait daqui traz só o que a galeria chama, com o nome do
//! contrato original e o prefixo do que ele recorta, e se funde ao recorte existente quando o serviço dono do contrato
//! for portado por inteiro.

use std::rc::Rc;

use je_base::parts::ipc::IChannel;
use je_base::promise::Promise;
use je_instantiation::service_id;

/// `ISharedProcessService`, reduzido a `getChannel`.
pub trait ISharedProcessService {
    /// `getChannel(channelName)`: o canal de nome dado, no processo compartilhado.
    fn get_channel(&self, channel_name: &str) -> Rc<dyn IChannel>;
}

service_id!(pub I_SHARED_PROCESS_SERVICE: dyn ISharedProcessService = "sharedProcessService");

/// `IRemoteAgentConnection`, reduzido a `remoteAuthority` e `getChannel`.
pub trait IRemoteAgentConnection {
    /// `remoteAuthority`: a autoridade do servidor remoto (por exemplo `ssh-remote+host`).
    fn remote_authority(&self) -> String;

    /// `getChannel(channelName)`: o canal de nome dado, no servidor remoto.
    fn get_channel(&self, channel_name: &str) -> Rc<dyn IChannel>;
}

/// O `getConnection` do `IRemoteAgentService`: a conexão com o servidor remoto, ou `None` (o `null` do original) sem
/// janela remota.
pub trait IRemoteAgentConnectionProvider {
    fn get_connection(&self) -> Option<Rc<dyn IRemoteAgentConnection>>;
}

service_id!(pub I_REMOTE_AGENT_CONNECTION_PROVIDER: dyn IRemoteAgentConnectionProvider = "remoteAgentConnectionProvider");

/// O `restart` do `IHostService`: reinicia a aplicação.
pub trait IHostRestartService {
    fn restart(&self) -> Promise<()>;
}

service_id!(pub I_HOST_RESTART_SERVICE: dyn IHostRestartService = "hostRestartService");
