//! Porte dos tipos de dados de `src/vs/workbench/contrib/onboarding/common/onboardingTryout.ts`: os metadados do
//! "experimente o recurso" que um cenário de onboarding carrega (`IOnboardingScenario.tryout`) e os resultados do
//! serviço.
//!
//! Divergências:
//! - **Só os dados e os validadores puros.** O restante do arquivo é o serviço de tryout
//!   (`IOnboardingTryoutService`, `IOnboardingTryoutPresentation` e o registro de apresentações, que usam
//!   `CancellationToken`, `Promise`, `DisposableStore`, `IOnboardingTryoutRunOptions` de `platform/onboarding`), os
//!   links `command:` e do protocolo do produto (`createOnboardingTryoutUri`, `parseOnboardingTryoutUri`,
//!   `parseExternalOnboardingTryoutUri`) e `registerOnboardingTryout*`. Entram com o motor de onboarding, que é
//!   quem os consome; o registro de cenários (`onboarding_registry`) só precisa dos metadados.
//! - `arguments?: readonly unknown[]` é `Option<Vec<Dynamic>>`.

use je_base::dynamic::Dynamic;

pub const RUN_ONBOARDING_TRYOUT_COMMAND_ID: &str = "workbench.action.onboarding.tryFeature";
pub const ONBOARDING_TRYOUT_URL_AUTHORITY: &str = "tryout";
pub const AGENTS_WINDOW_TRYOUT_PRESENTATION_KIND: &str = "agentsWindow";

/// `IOnboardingTryoutCommand`.
#[derive(Debug, Clone, PartialEq)]
pub struct OnboardingTryoutCommand {
    pub id: String,
    pub arguments: Option<Vec<Dynamic>>,
}

/// `IOnboardingTryoutSetupAction`.
#[derive(Debug, Clone, PartialEq)]
pub struct OnboardingTryoutSetupAction {
    pub label: String,
    pub command: OnboardingTryoutCommand,
}

/// O `targetWindow?: 'agents'` de `IOnboardingTryoutMetadata`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingTryoutTargetWindow {
    Agents,
}

/// `IOnboardingTryoutMetadata`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OnboardingTryoutMetadata {
    pub title: String,
    pub description: String,
    pub is_ai: Option<bool>,
    pub target_window: Option<OnboardingTryoutTargetWindow>,
    /// Se links do protocolo do produto podem pedir este tryout. Padrão `true`, e sempre exige confirmação.
    pub allow_external_launch: Option<bool>,
    pub unavailable_message: Option<String>,
    pub setup: Option<OnboardingTryoutSetupAction>,
}

/// `IOnboardingTryoutUnavailable`.
#[derive(Debug, Clone, PartialEq)]
pub struct OnboardingTryoutUnavailable {
    pub message: String,
    pub action: Option<OnboardingTryoutSetupAction>,
}

/// `OnboardingTryoutAvailability`.
#[derive(Debug, Clone, PartialEq)]
pub enum OnboardingTryoutAvailability {
    Ready,
    Hidden,
    Unavailable(OnboardingTryoutUnavailable),
}

/// O `kind: 'opened' | 'executed' | 'prepared'` de `OnboardingTryoutResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingTryoutLaunchKind {
    Opened,
    Executed,
    Prepared,
}

/// `OnboardingTryoutResult`.
#[derive(Debug, Clone, PartialEq)]
pub enum OnboardingTryoutResult {
    Launched { kind: OnboardingTryoutLaunchKind, target_scope: Option<String> },
    Routed,
    Cancelled,
    Unavailable(OnboardingTryoutUnavailable),
}

/// `isOnboardingTryoutId`: `/^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$/`.
pub fn is_onboarding_tryout_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() <= 128
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes[1..].iter().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// `isOnboardingTargetScope`: texto de 1 a 512 unidades UTF-16.
pub fn is_onboarding_target_scope(value: &str) -> bool {
    (1..=512).contains(&value.encode_utf16().count())
}
