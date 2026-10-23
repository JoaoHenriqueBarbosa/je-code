//! Porte de `src/vs/platform/remoteTunnel/common/remoteTunnel.ts`: a sessão, o modo e o estado do túnel
//! remoto, o contrato do serviço e a derivação dos nomes de dev tunnel.
//!
//! Divergências:
//! - `IRemoteTunnelService` é um trait usado como `Rc<dyn IRemoteTunnelService>`; o `createDecorator` e o
//!   `_serviceBrand` não existem (a injeção de dependência fica com quem monta os serviços). Os métodos que
//!   devolvem `Promise<T>` no original devolvem [`Promise`]; `Promise<void>` é `Promise<()>`.
//! - A união `TunnelStatus` é o enum [`TunnelStatus`] sobre as structs do namespace `TunnelStates`, que vira o
//!   módulo [`tunnel_states`] (com as mesmas funções de construção). `TunnelMode` é o enum [`TunnelMode`].
//! - `INACTIVE_TUNNEL_MODE` (constante de objeto) é a função [`inactive_tunnel_mode`], e o
//!   `TunnelStates.uninitialized` é [`tunnel_states::uninitialized`].
//! - `LOGGER_NAME` chama `localize` ao carregar o módulo no original; aqui é a função [`logger_name`],
//!   porque o `localize` do `je-base` devolve `String` e não há inicializador de módulo.
//! - Os nomes de túnel entram como `&str` e saem como `String`: o que a derivação mantém é só ASCII, então
//!   a troca de UTF-16 por UTF-8 não muda o resultado. `Array.from(hostname)` itera pontos de código, igual
//!   a `str::chars`.

use std::rc::Rc;

use je_base::event::Event;
use je_base::nls::localize;
use je_base::promise::Promise;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IRemoteTunnelSession {
    pub provider_id: String,
    pub session_id: String,
    pub account_label: String,
    pub token: Option<String>,
}

pub trait IRemoteTunnelService {
    fn on_did_change_tunnel_status(&self) -> Event<TunnelStatus>;
    fn get_tunnel_status(&self) -> Promise<TunnelStatus>;

    fn get_mode(&self) -> Promise<TunnelMode>;
    fn on_did_change_mode(&self) -> Event<TunnelMode>;

    fn on_did_token_failed(&self) -> Event<Option<IRemoteTunnelSession>>;
    fn initialize(&self, mode: TunnelMode) -> Promise<TunnelStatus>;

    fn start_tunnel(&self, mode: ActiveTunnelMode) -> Promise<TunnelStatus>;
    fn stop_tunnel(&self) -> Promise<()>;
    fn get_tunnel_name(&self) -> Promise<Option<String>>;
}

/// Um `Rc<dyn IRemoteTunnelService>`, a forma em que o serviço circula.
pub type RemoteTunnelServiceRef = Rc<dyn IRemoteTunnelService>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveTunnelMode {
    /// No original é o literal `true`; fica implícito no tipo.
    pub session: IRemoteTunnelSession,
    pub as_service: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct InactiveTunnelMode;

/// `INACTIVE_TUNNEL_MODE`.
pub fn inactive_tunnel_mode() -> InactiveTunnelMode {
    InactiveTunnelMode
}

/// Saved mode for the tunnel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TunnelMode {
    Active(ActiveTunnelMode),
    Inactive(InactiveTunnelMode),
}

impl TunnelMode {
    /// O campo `active` da união do original.
    pub fn active(&self) -> bool {
        matches!(self, TunnelMode::Active(_))
    }
}

impl From<ActiveTunnelMode> for TunnelMode {
    fn from(mode: ActiveTunnelMode) -> Self {
        TunnelMode::Active(mode)
    }
}

impl From<InactiveTunnelMode> for TunnelMode {
    fn from(mode: InactiveTunnelMode) -> Self {
        TunnelMode::Inactive(mode)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TunnelStatus {
    Connected(tunnel_states::Connected),
    Disconnected(tunnel_states::Disconnected),
    Connecting(tunnel_states::Connecting),
    Uninitialized(tunnel_states::Uninitialized),
}

impl TunnelStatus {
    /// O campo `type` da união do original.
    pub fn type_(&self) -> &'static str {
        match self {
            TunnelStatus::Connected(_) => "connected",
            TunnelStatus::Disconnected(_) => "disconnected",
            TunnelStatus::Connecting(_) => "connecting",
            TunnelStatus::Uninitialized(_) => "uninitialized",
        }
    }
}

/// O namespace `TunnelStates` do original.
pub mod tunnel_states {
    use super::{ConnectionInfo, IRemoteTunnelSession};

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub struct Uninitialized;

    #[derive(Clone, Debug, PartialEq, Eq, Default)]
    pub struct Connecting {
        pub progress: Option<String>,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Connected {
        pub info: ConnectionInfo,
        pub service_install_failed: bool,
    }

    #[derive(Clone, Debug, PartialEq, Eq, Default)]
    pub struct Disconnected {
        pub on_token_failed: Option<IRemoteTunnelSession>,
    }

    pub fn disconnected(on_token_failed: Option<IRemoteTunnelSession>) -> Disconnected {
        Disconnected { on_token_failed }
    }

    pub fn connected(info: ConnectionInfo, service_install_failed: bool) -> Connected {
        Connected { info, service_install_failed }
    }

    pub fn connecting(progress: Option<String>) -> Connecting {
        Connecting { progress }
    }

    pub fn uninitialized() -> Uninitialized {
        Uninitialized
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectionInfo {
    pub link: Option<String>,
    pub domain: Option<String>,
    pub tunnel_name: String,
    pub tunnel_id: Option<String>,
    pub is_attached: bool,
}

pub const CONFIGURATION_KEY_PREFIX: &str = "remote.tunnels.access";
pub const CONFIGURATION_KEY_HOST_NAME: &str = "remote.tunnels.access.hostNameOverride";
pub const CONFIGURATION_KEY_PREVENT_SLEEP: &str = "remote.tunnels.access.preventSleep";

/// Maximum length of a dev tunnel name, matching `MAX_TUNNEL_NAME_LENGTH` in the CLI's `cli/src/tunnels/dev_tunnels.rs`.
pub const MAX_TUNNEL_NAME_LENGTH: usize = 20;

/// Stand-in the CLI uses when a host name cleans down to almost nothing.
const TUNNEL_NAME_PLACEHOLDER: &str = "remote-machine";

/// Derive a dev tunnel name from a machine's host name, mirroring
/// `clean_hostname_for_tunnel` and `get_placeholder_name` in the CLI's
/// `cli/src/tunnels/dev_tunnels.rs`: `-`, `_` and spaces all collapse to `-`,
/// anything non-alphanumeric is dropped, and dashes are trimmed from both ends.
///
/// `code tunnel` and in-editor remote session hosting must derive the same name
/// for a machine, otherwise each registers its own dev tunnel for it.
pub fn tunnel_name_from_hostname(hostname: &str) -> String {
    let mut cleaned = String::new();
    for char in hostname.chars().take(60) {
        if char == '-' || char == '_' || char == ' ' {
            cleaned.push('-');
        } else if char.is_ascii_alphanumeric() {
            cleaned.push(char);
        }
    }

    let trimmed = cleaned.trim_matches('-');
    let name = if trimmed.len() < 2 { TUNNEL_NAME_PLACEHOLDER } else { trimmed };
    name.to_ascii_lowercase().chars().take(MAX_TUNNEL_NAME_LENGTH).collect()
}

/// Normalize an explicitly configured tunnel name. The CLI only lowercases and
/// validates a name the user supplied rather than rewriting it (`is_valid_name`),
/// so the sanitization here stays minimal; host names go through
/// [`tunnel_name_from_hostname`] instead.
pub fn normalize_tunnel_name(name: &str) -> String {
    name.trim_start_matches('-')
        .chars()
        .filter(|char| char.is_ascii_alphanumeric() || *char == '_' || *char == '-')
        .take(MAX_TUNNEL_NAME_LENGTH)
        .collect::<String>()
        .to_ascii_lowercase()
}

pub const LOG_ID: &str = "remoteTunnelService";

pub fn logger_name() -> String {
    localize("remoteTunnelLog", "Remote Tunnel Service", &[])
}
