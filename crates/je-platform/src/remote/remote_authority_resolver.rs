//! Porte parcial de `src/vs/platform/remote/common/remoteAuthorityResolver.ts`: só os tipos de conexão
//! (`RemoteConnectionType`, `ManagedRemoteConnection`, `WebSocketRemoteConnection` e `RemoteConnection`), que o serviço de
//! fábricas de socket e o `MainThreadManagedSockets` usam.
//!
//! Divergências:
//! - Fora do porte (entram junto de quem os usar): `IRemoteAuthorityResolverService`, `ResolvedAuthority`,
//!   `ResolvedOptions`, os erros de resolução e o restante do arquivo.
//! - `ManagedRemoteConnection.id` (`number`) é `u64`, como os identificadores de fábrica e de socket do protocolo.
//! - `RemoteConnection = WebSocketRemoteConnection | ManagedRemoteConnection` é o enum [`RemoteConnection`]; o campo
//!   `type` de cada classe é o [`RemoteConnection::connection_type`]. `RemoteConnectionOfType<T>` (o estreitamento por
//!   tipo) não existe: quem recebe um `RemoteConnection` confere a variante.
//! - `toString()` é o [`std::fmt::Display`].

use std::fmt;

/// `RemoteConnectionType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RemoteConnectionType {
    WebSocket,
    Managed,
}

/// `ManagedRemoteConnection`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedRemoteConnection {
    pub id: u64,
}

impl ManagedRemoteConnection {
    pub fn new(id: u64) -> Self {
        Self { id }
    }
}

impl fmt::Display for ManagedRemoteConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Managed({})", self.id)
    }
}

/// `WebSocketRemoteConnection`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebSocketRemoteConnection {
    pub host: String,
    pub port: u16,
}

impl WebSocketRemoteConnection {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self { host: host.into(), port }
    }
}

impl fmt::Display for WebSocketRemoteConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WebSocket({}:{})", self.host, self.port)
    }
}

/// `RemoteConnection`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemoteConnection {
    WebSocket(WebSocketRemoteConnection),
    Managed(ManagedRemoteConnection),
}

impl RemoteConnection {
    /// O campo `type` da classe da variante.
    pub fn connection_type(&self) -> RemoteConnectionType {
        match self {
            Self::WebSocket(_) => RemoteConnectionType::WebSocket,
            Self::Managed(_) => RemoteConnectionType::Managed,
        }
    }
}

impl fmt::Display for RemoteConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WebSocket(connection) => connection.fmt(f),
            Self::Managed(connection) => connection.fmt(f),
        }
    }
}
