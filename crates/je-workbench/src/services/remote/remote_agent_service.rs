//! Porte de `src/vs/workbench/services/remote/common/remoteAgentService.ts`, só com o que os serviços de caminho, de
//! rótulo e de histórico do terminal leem: o `IRemoteAgentService` com `getEnvironment`, `getRawEnvironment` e
//! `getConnection`.
//!
//! Não entram `getExtensionHostExitInfo`, `getDiagnosticInfo`, `updateTelemetryLevel`, `logTelemetry`,
//! `flushTelemetry`, `getRoundTripTime` e `endConnection`: dependem de tipos de diagnóstico e telemetria remotos que
//! não foram portados. Cada um entra junto com o tipo de que depende.
//!
//! `Promise<IRemoteAgentEnvironment | null>` é `Promise<Option<RemoteAgentEnvironment>>`; o `null` do ambiente que
//! não resolveu (sem conexão ou com erro) é `None`.
//!
//! `getConnection(): IRemoteAgentConnection | null` é `Option<Rc<dyn IRemoteAgentConnection>>`, com o recorte
//! `IRemoteAgentConnection` de `services::extension_management::extension_gallery_contracts` (que lá tem só `getChannel`
//! e aqui ganhou `remoteAuthority`). O trait `IRemoteAgentConnectionProvider` do mesmo arquivo repete o `getConnection`
//! para a galeria e se funde a este quando o serviço for portado por inteiro.

use std::rc::Rc;

use je_base::promise::Promise;
use je_instantiation::service_id;
use je_platform::remote::remote_agent_environment::RemoteAgentEnvironment;

use crate::services::extension_management::extension_gallery_contracts::IRemoteAgentConnection;

/// `IRemoteAgentService`.
pub trait IRemoteAgentService {
    /// Get the remote environment. In case of an error, returns `None`.
    fn get_environment(&self) -> Promise<Option<RemoteAgentEnvironment>>;

    /// Get the remote environment. Can return an error.
    fn get_raw_environment(&self) -> Promise<Option<RemoteAgentEnvironment>>;

    /// `getConnection()`: a conexão com o servidor remoto, ou `None` (o `null` do original) sem janela remota.
    fn get_connection(&self) -> Option<Rc<dyn IRemoteAgentConnection>>;
}

service_id!(pub I_REMOTE_AGENT_SERVICE: dyn IRemoteAgentService = "remoteAgentService");
