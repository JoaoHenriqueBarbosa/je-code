//! Porte de `src/vs/workbench/services/remote/common/remoteAgentService.ts`, só com o que os serviços de caminho e de
//! rótulo leem: o `IRemoteAgentService` com `getEnvironment` e `getRawEnvironment`.
//!
//! Não entram `getConnection` (`IRemoteAgentConnection`, a conexão por socket), `getExtensionHostExitInfo`,
//! `getDiagnosticInfo`, `updateTelemetryLevel`, `logTelemetry`, `flushTelemetry`, `getRoundTripTime` e
//! `endConnection`: dependem de tipos de conexão, diagnóstico e telemetria remotos que não foram portados. Cada um
//! entra junto com o tipo de que depende.
//!
//! `Promise<IRemoteAgentEnvironment | null>` é `Promise<Option<RemoteAgentEnvironment>>`; o `null` do ambiente que
//! não resolveu (sem conexão ou com erro) é `None`.

use je_base::promise::Promise;
use je_instantiation::service_id;
use je_platform::remote::remote_agent_environment::RemoteAgentEnvironment;

/// `IRemoteAgentService`.
pub trait IRemoteAgentService {
    /// Get the remote environment. In case of an error, returns `None`.
    fn get_environment(&self) -> Promise<Option<RemoteAgentEnvironment>>;

    /// Get the remote environment. Can return an error.
    fn get_raw_environment(&self) -> Promise<Option<RemoteAgentEnvironment>>;
}

service_id!(pub I_REMOTE_AGENT_SERVICE: dyn IRemoteAgentService = "remoteAgentService");
