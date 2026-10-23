//! Porte de `src/vs/platform/policy/common/managedSettingsFreshness.ts`: o estado do portão fail-closed
//! `forceRemoteSettingsRefresh`, compartilhado entre a busca, o bloqueio e o diagnóstico.
//!
//! Decisões de tradução:
//! - **`IManagedSettingsFreshness`** (uma união discriminada por `state`) é o `enum`
//!   [`IManagedSettingsFreshness`]; [`ManagedSettingsFreshnessState`] guarda só os quatro estados com o
//!   texto do original em [`ManagedSettingsFreshnessState::as_str`]. `scope` de `Satisfied` é obrigatório,
//!   então essa variante carrega os campos de [`IManagedSettingsFreshnessEffective`] achatados.
//! - **`ManagedSettingsFreshnessBlocked`** exige `httpStatus` só quando a falha é `HttpError`: a falha é um
//!   `enum` cuja variante [`ManagedSettingsFreshnessFailure::HttpError`] carrega o código, e as outras não.
//! - Os instantes (`lastAttemptAt`, `satisfiedAt`) são milissegundos de época em `u64`.
//! - `isSameScope` é a igualdade derivada de [`IManagedSettingsFreshnessScope`].

use super::copilot_managed_settings::ManagedSettingsChannel;

/// Lifecycle of the `forceRemoteSettingsRefresh` fail-closed gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedSettingsFreshnessState {
    /// The control is not effective.
    NotRequired,
    /// A fresh response is required and the request has not completed.
    Pending,
    /// A fresh response, including a fresh 404, was received for the current scope.
    Satisfied,
    /// The required refresh failed.
    Blocked,
}

impl ManagedSettingsFreshnessState {
    /// O texto do original (`'notRequired' | 'pending' | 'satisfied' | 'blocked'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRequired => "notRequired",
            Self::Pending => "pending",
            Self::Satisfied => "satisfied",
            Self::Blocked => "blocked",
        }
    }
}

/// Failure categories that require distinct remediation or diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagedSettingsFreshnessFailure {
    NoUrl,
    NoToken,
    Network,
    RateLimited,
    /// A falha HTTP leva o código de status (`httpStatus`).
    HttpError { http_status: u32 },
    /// JSON parsing failed; runtime-owned schema validation is not duplicated here.
    Malformed,
    UpdateRequired,
}

impl ManagedSettingsFreshnessFailure {
    /// O texto do original (`'noUrl' | 'noToken' | ...`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoUrl => "noUrl",
            Self::NoToken => "noToken",
            Self::Network => "network",
            Self::RateLimited => "rateLimited",
            Self::HttpError { .. } => "httpError",
            Self::Malformed => "malformed",
            Self::UpdateRequired => "updateRequired",
        }
    }
}

/// Account, provider, and endpoint for which freshness was attempted or satisfied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IManagedSettingsFreshnessScope {
    pub account_id: String,
    pub authentication_provider_id: String,
    pub endpoint_origin: String,
}

/// `IManagedSettingsFreshnessEffective`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IManagedSettingsFreshnessEffective {
    pub source: ManagedSettingsChannel,
    pub scope: Option<IManagedSettingsFreshnessScope>,
    pub last_attempt_at: Option<u64>,
}

/// Observable freshness state shared by fetching, gating, and diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IManagedSettingsFreshness {
    NotRequired,
    Pending(IManagedSettingsFreshnessEffective),
    Satisfied { source: ManagedSettingsChannel, scope: IManagedSettingsFreshnessScope, last_attempt_at: Option<u64>, satisfied_at: u64 },
    Blocked { effective: IManagedSettingsFreshnessEffective, failure: ManagedSettingsFreshnessFailure },
}

impl IManagedSettingsFreshness {
    /// O `state` do original.
    pub fn state(&self) -> ManagedSettingsFreshnessState {
        match self {
            Self::NotRequired => ManagedSettingsFreshnessState::NotRequired,
            Self::Pending(_) => ManagedSettingsFreshnessState::Pending,
            Self::Satisfied { .. } => ManagedSettingsFreshnessState::Satisfied,
            Self::Blocked { .. } => ManagedSettingsFreshnessState::Blocked,
        }
    }
}

pub const MANAGED_SETTINGS_FRESHNESS_NOT_REQUIRED: IManagedSettingsFreshness = IManagedSettingsFreshness::NotRequired;

/// Whether AI functionality must be withheld until freshness is established.
pub fn is_managed_settings_freshness_blocking(freshness: &IManagedSettingsFreshness) -> bool {
    matches!(freshness.state(), ManagedSettingsFreshnessState::Pending | ManagedSettingsFreshnessState::Blocked)
}

/// Whether the satisfied result belongs to `scope`.
pub fn is_managed_settings_freshness_satisfied_for(freshness: &IManagedSettingsFreshness, scope: &IManagedSettingsFreshnessScope) -> bool {
    matches!(freshness, IManagedSettingsFreshness::Satisfied { scope: satisfied_scope, .. } if satisfied_scope == scope)
}
