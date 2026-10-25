//! Porte de `src/vs/platform/remote/common` do VS Code.
//!
//! Portado até agora: `remoteHosts.ts` (autoridades remotas e hosts de loopback), `remoteAuthorityResolver.ts` (só os tipos
//! de conexão), `remoteSocketFactoryService.ts`, `managedSocket.ts` e `remoteAgentConnection.ts` (só o que o túnel usa:
//! `IAddressProvider`, `IConnectionOptions` e `connectRemoteAgentTunnel`).

pub mod remote_hosts;

pub mod managed_socket;
pub mod remote_agent_connection;
pub mod remote_agent_environment;
pub mod remote_authority_resolver;
pub mod remote_socket_factory_service;
