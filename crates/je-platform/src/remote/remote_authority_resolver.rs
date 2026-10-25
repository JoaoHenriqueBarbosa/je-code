//! Porte parcial de `src/vs/platform/remote/common/remoteAuthorityResolver.ts`: só os tipos de conexão
//! (`RemoteConnectionType`, `ManagedRemoteConnection`, `WebSocketRemoteConnection` e `RemoteConnection`), que o serviço de
//! fábricas de socket e o `MainThreadManagedSockets` usam.
//!
//! Divergências:
//! - `ResolvedAuthority`, `ResolvedOptions`, `ResolverResult` e o `IRemoteAuthorityResolverService` (só `resolveAuthority` e
//!   `getCanonicalURI`) estão no fim do arquivo, junto de `RemoteAuthorityResolverError`, `RemoteAuthorityResolverErrorCode`
//!   e `getRemoteAuthorityPrefix`. O contrato ganhou `_clearResolvedAuthority`, `_setResolvedAuthority` e
//!   `_setResolvedAuthorityError` (métodos com corpo padrão vazio, lidos pelo `AbstractExtensionService`). Fora do porte (entram
//!   junto de quem os usar): `tunnelInformation`, o restante do contrato do serviço e do arquivo.
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

    /// `_clearResolvedAuthority(authority)`. O padrão não faz nada: serve ao serviço que não guarda autoridades resolvidas.
    fn clear_resolved_authority(&self, _authority: &str) {}

    /// `_setResolvedAuthority(resolvedAuthority, resolvedOptions?)`. O padrão não faz nada (ver `clear_resolved_authority`).
    fn set_resolved_authority(&self, _resolved_authority: ResolvedAuthority, _resolved_options: Option<ResolvedOptions>) {}

    /// `_setResolvedAuthorityError(authority, err)`. O padrão não faz nada (ver `clear_resolved_authority`).
    fn set_resolved_authority_error(&self, _authority: &str, _err: je_base::errors::Error) {}
}

/// `RemoteAuthorityResolverErrorCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteAuthorityResolverErrorCode {
    Unknown,
    NotAvailable,
    TemporarilyNotAvailable,
    NoResolverFound,
    InvalidAuthority,
}

impl RemoteAuthorityResolverErrorCode {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown",
            Self::NotAvailable => "NotAvailable",
            Self::TemporarilyNotAvailable => "TemporarilyNotAvailable",
            Self::NoResolverFound => "NoResolverFound",
            Self::InvalidAuthority => "InvalidAuthority",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "Unknown" => Some(Self::Unknown),
            "NotAvailable" => Some(Self::NotAvailable),
            "TemporarilyNotAvailable" => Some(Self::TemporarilyNotAvailable),
            "NoResolverFound" => Some(Self::NoResolverFound),
            "InvalidAuthority" => Some(Self::InvalidAuthority),
            _ => None,
        }
    }
}

/// `RemoteAuthorityResolverError` (um `ErrorNoTelemetry`).
///
/// O erro do porte é o [`je_base::errors::Error`], que não tem subclasses: o `instanceof RemoteAuthorityResolverError` do
/// original é a propriedade `remoteAuthorityResolverErrorCode`, que só [`RemoteAuthorityResolverError::new`] grava, e as
/// funções `is_*` a leem. `_detail` (`unknown`) não cabe nas propriedades de texto do [`je_base::errors::Error`]: só o seu
/// efeito sobre `isHandled` (`code === NotAvailable && detail === true`) é guardado, na propriedade `isHandled`.
pub struct RemoteAuthorityResolverError;

const CODE_PROP: &str = "remoteAuthorityResolverErrorCode";
const IS_HANDLED_PROP: &str = "isHandled";

impl RemoteAuthorityResolverError {
    /// `new RemoteAuthorityResolverError(message?, code = Unknown, detail?)`. `detail_is_true` é o `detail === true`.
    pub fn new(message: Option<&str>, code: RemoteAuthorityResolverErrorCode, detail_is_true: bool) -> je_base::errors::Error {
        let error = je_base::errors::Error::no_telemetry(message.unwrap_or("")).with_prop(CODE_PROP, code.as_str());
        if code == RemoteAuthorityResolverErrorCode::NotAvailable && detail_is_true {
            error.with_prop(IS_HANDLED_PROP, true)
        } else {
            error
        }
    }

    /// `err._code`, quando `err instanceof RemoteAuthorityResolverError`.
    pub fn code(err: &je_base::errors::Error) -> Option<RemoteAuthorityResolverErrorCode> {
        err.prop(CODE_PROP).and_then(RemoteAuthorityResolverErrorCode::from_str)
    }

    pub fn is_not_available(err: &je_base::errors::Error) -> bool {
        Self::code(err) == Some(RemoteAuthorityResolverErrorCode::NotAvailable)
    }

    pub fn is_temporarily_not_available(err: &je_base::errors::Error) -> bool {
        Self::code(err) == Some(RemoteAuthorityResolverErrorCode::TemporarilyNotAvailable)
    }

    pub fn is_no_resolver_found(err: &je_base::errors::Error) -> bool {
        Self::code(err) == Some(RemoteAuthorityResolverErrorCode::NoResolverFound)
    }

    pub fn is_invalid_authority(err: &je_base::errors::Error) -> bool {
        Self::code(err) == Some(RemoteAuthorityResolverErrorCode::InvalidAuthority)
    }

    pub fn is_handled(err: &je_base::errors::Error) -> bool {
        Self::code(err).is_some() && err.prop(IS_HANDLED_PROP) == Some("true")
    }
}

/// `TunnelPrivacy`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TunnelPrivacy {
    pub theme_icon: String,
    pub id: String,
    pub label: String,
}

/// `getRemoteAuthorityPrefix(remoteAuthority)`.
pub fn get_remote_authority_prefix(remote_authority: &str) -> &str {
    match remote_authority.find('+') {
        None => remote_authority,
        Some(plus_index) => &remote_authority[..plus_index],
    }
}

je_instantiation::service_id!(pub I_REMOTE_AUTHORITY_RESOLVER_SERVICE: dyn IRemoteAuthorityResolverService = "remoteAuthorityResolverService");
