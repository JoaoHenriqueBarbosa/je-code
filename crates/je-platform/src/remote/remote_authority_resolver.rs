//! Porte parcial de `src/vs/platform/remote/common/remoteAuthorityResolver.ts`: só os tipos de conexão
//! (`RemoteConnectionType`, `ManagedRemoteConnection`, `WebSocketRemoteConnection` e `RemoteConnection`), que o serviço de
//! fábricas de socket e o `MainThreadManagedSockets` usam.
//!
//! Divergências:
//! - `ResolvedAuthority`, `ResolvedOptions`, `ResolverResult` e o `IRemoteAuthorityResolverService` (só `resolveAuthority` e
//!   `getCanonicalURI`) estão no fim do arquivo. Fora do porte (entram junto de quem os usar): `tunnelInformation`, os erros
//!   de resolução, o restante do contrato do serviço e do arquivo.
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

/// `ResolvedAuthority`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedAuthority {
    pub authority: String,
    pub connect_to: RemoteConnection,
    pub connection_token: Option<String>,
}

/// O `authenticationSession` de `ResolvedOptions`: `{ id: string; providerId: string }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedAuthenticationSession {
    pub id: String,
    pub provider_id: String,
}

/// `ResolvedOptions`. `extensionHostEnv` (`{ [key: string]: string | null }`) é a lista de pares, com `None` no `null`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedOptions {
    pub extension_host_env: Option<Vec<(String, Option<String>)>>,
    pub is_trusted: Option<bool>,
    pub authentication_session: Option<ResolvedAuthenticationSession>,
}

/// `ResolverResult` (sem o `tunnelInformation`, que quem o usar porta junto).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolverResult {
    pub authority: ResolvedAuthority,
    pub options: Option<ResolvedOptions>,
}

/// `IRemoteAuthorityResolverService`, só com as duas operações que o serviço de confiança no workspace chama
/// (`resolveAuthority` e `getCanonicalURI`); o restante do contrato entra junto de quem o usar.
pub trait IRemoteAuthorityResolverService {
    /// `resolveAuthority(authority)`.
    fn resolve_authority(&self, authority: &str) -> je_base::promise::Promise<ResolverResult>;

    /// `getCanonicalURI(uri)`: pode rejeitar, por exemplo quando não há resolvedor instalado para a autoridade.
    fn get_canonical_uri(&self, uri: &je_base::uri::URI) -> je_base::promise::Promise<je_base::uri::URI>;
}

je_instantiation::service_id!(pub I_REMOTE_AUTHORITY_RESOLVER_SERVICE: dyn IRemoteAuthorityResolverService = "remoteAuthorityResolverService");
