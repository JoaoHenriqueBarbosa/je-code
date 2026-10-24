//! Porte de `src/vs/workbench/services/remote/common/remoteAgentService.ts`, só com o que os serviços de caminho, de
//! rótulo e de histórico do terminal leem: o `IRemoteAgentService` com `getEnvironment`, `getRawEnvironment` e
//! `getConnection`.
//!
//! Não entram `getDiagnosticInfo`, `updateTelemetryLevel`, `logTelemetry`,
//! `flushTelemetry` e `getRoundTripTime`: dependem de tipos de diagnóstico e telemetria remotos que
//! não foram portados. Cada um entra junto com o tipo de que depende. `endConnection` e `getExtensionHostExitInfo` (lidos
//! pelo `AbstractExtensionService`) são métodos com corpo padrão (`Promise<void>` resolvida e `None`, o `null` do original
//! sem conexão), e o `dispose` do `IRemoteAgentConnection` tem corpo padrão vazio: quem tem conexão os sobrescreve.
//!
//! `Promise<IRemoteAgentEnvironment | null>` é `Promise<Option<RemoteAgentEnvironment>>`; o `null` do ambiente que
//! não resolveu (sem conexão ou com erro) é `None`.
//!
//! `getConnection(): IRemoteAgentConnection | null` é `Option<Rc<dyn IRemoteAgentConnection>>`, com o
//! `IRemoteAgentConnection` reduzido a `remoteAuthority` e `getChannel`, o que os serviços portados leem dele.

use std::rc::Rc;

use je_base::parts::ipc::IChannel;
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_platform::remote::remote_agent_environment::RemoteAgentEnvironment;

/// `IRemoteAgentConnection`, reduzido a `remoteAuthority` e `getChannel`.
pub trait IRemoteAgentConnection {
    /// `remoteAuthority`: a autoridade do servidor remoto (por exemplo `ssh-remote+host`).
    fn remote_authority(&self) -> String;

    /// `getChannel(channelName)`: o canal de nome dado, no servidor remoto.
    fn get_channel(&self, channel_name: &str) -> Rc<dyn IChannel>;

    /// `dispose()` (o `IRemoteAgentConnection` é um `IDisposable`).
    fn dispose(&self) {}
}

/// `IExtensionHostExitInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IExtensionHostExitInfo {
    pub code: i32,
    pub signal: String,
}

/// `IRemoteAgentService`.
pub trait IRemoteAgentService {
    /// Get the remote environment. In case of an error, returns `None`.
    fn get_environment(&self) -> Promise<Option<RemoteAgentEnvironment>>;

    /// Get the remote environment. Can return an error.
    fn get_raw_environment(&self) -> Promise<Option<RemoteAgentEnvironment>>;

    /// `getConnection()`: a conexão com o servidor remoto, ou `None` (o `null` do original) sem janela remota.
    fn get_connection(&self) -> Option<Rc<dyn IRemoteAgentConnection>>;

    /// `endConnection()`: encerra a conexão de gerenciamento com o servidor remoto (padrão: nada a encerrar).
    fn end_connection(&self) -> Promise<()> {
        Promise::resolved(())
    }

    /// `getExtensionHostExitInfo(reconnectionToken)`: por que o host de extensões remoto terminou (`None` é o `null`).
    fn get_extension_host_exit_info(&self, _reconnection_token: &str) -> Promise<Option<IExtensionHostExitInfo>> {
        Promise::resolved(None)
    }
}

service_id!(pub I_REMOTE_AGENT_SERVICE: dyn IRemoteAgentService = "remoteAgentService");
