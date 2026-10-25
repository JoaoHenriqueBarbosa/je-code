//! Porte de `src/vs/workbench/contrib/onboarding/common/onboardingScenarioService.ts`: o contrato do motor de
//! onboarding, as chaves de configuração e a leitura do modo desenvolvedor.
//!
//! Divergências:
//! - `createDecorator<IOnboardingScenarioService>('onboardingScenarioService')` é o [`I_ONBOARDING_SCENARIO_SERVICE`].
//! - `runScenario(id, token = CancellationToken.None)` recebe o token sempre; quem não tem um passa
//!   [`CancellationToken::None`].
//! - `shouldShowNudge(id, reader?: IReader)` recebe `Option<&dyn IReader>`; o `throw` de cenário desconhecido é
//!   [`errors::throw`].
//! - `getScenarios(): readonly IOnboardingScenario[]` devolve os `Rc<OnboardingScenario>` do registro.
//! - `typeof value === 'object' && value !== null && value[scenarioId] === true` é o `type_of` do [`Dynamic`] mais a
//!   leitura da chave: um valor que não é objeto (ou é nulo) não habilita nada.
//! - Os tipos `OnboardingDeveloperMode` e `OnboardingDeveloperModeVariations` são só tipos do TypeScript (o formato do
//!   `Dynamic` que a configuração devolve) e não têm equivalente.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::observable::base::IReader;
use je_base::promise::Promise;
use je_instantiation::service_id;
use je_platform::configuration::service::IConfigurationService;

use super::onboarding_scenario::{OnboardingOutcome, OnboardingScenario};

service_id!(pub I_ONBOARDING_SCENARIO_SERVICE: dyn IOnboardingScenarioService = "onboardingScenarioService");

/// Global setting that enables/disables *automatic* onboarding scenarios.
pub const ONBOARDING_ENABLED_CONFIG: &str = "onboarding.enabled";

/// Developer/advanced setting. A map of scenario/tour id to a boolean: when a scenario's flag is `true`, that scenario
/// bypasses the gating that normally keeps it from showing, so it can be triggered on demand for testing. Concretely,
/// developer mode for a scenario:
///  - bypasses usage-based eligibility gating (e.g. the new-session tour's "first few sessions" check),
///  - bypasses the once-per-user persisted "shown" state, and
///  - bypasses the experiment gate: a tour with a linked experiment will show even if the experiment is not running,
///    or the user is in the control arm. A developer-mode preview never opens the assignment-context telemetry gate (in
///    any arm), so it can never affect an experiment scorecard.
///
/// Developer mode does NOT override the global `onboarding.enabled` switch: when onboarding is disabled, nothing shows
/// automatically regardless of this setting.
///
/// The scenario is still shown at most once per window session (tracked in memory, not persisted), so reload the
/// window to replay it: the "Reset Onboarding Shown State" command only clears the persisted state and does not affect
/// developer-mode replays.
///
/// The default value lists every registered scenario id set to `false`, so the available ids are discoverable (and
/// toggleable) in the settings editor.
pub const ONBOARDING_DEVELOPER_MODE_CONFIG: &str = "onboarding.developerMode";

/// Developer override for a scenario's experiment-selected variation.
pub const ONBOARDING_DEVELOPER_MODE_VARIATIONS_CONFIG: &str = "onboarding.developerModeVariations";

/// `typeof value === 'object' && value !== null`.
fn is_object_value(value: &Dynamic) -> bool {
    value.type_of() == "object" && !value.is_null()
}

/// Whether onboarding developer mode is enabled for the given scenario/tour id. Reads
/// [`ONBOARDING_DEVELOPER_MODE_CONFIG`] and returns `true` only when the setting is an object whose entry for
/// `scenario_id` is explicitly `true`.
pub fn is_onboarding_developer_mode_enabled(configuration_service: &dyn IConfigurationService, scenario_id: &str) -> bool {
    let value = configuration_service.get_value(Some(ONBOARDING_DEVELOPER_MODE_CONFIG), None);
    is_object_value(&value) && matches!(value.get_str(scenario_id), Dynamic::Bool(true))
}

/// `getOnboardingDeveloperModeVariation`.
pub fn get_onboarding_developer_mode_variation(configuration_service: &dyn IConfigurationService, scenario_id: &str) -> Option<String> {
    if !is_onboarding_developer_mode_enabled(configuration_service, scenario_id) {
        return None;
    }
    let value = configuration_service.get_value(Some(ONBOARDING_DEVELOPER_MODE_VARIATIONS_CONFIG), None);
    let variation = if is_object_value(&value) { value.get_str(scenario_id) } else { Dynamic::Undefined };
    match variation {
        Dynamic::String(variation) if !variation.is_empty() => Some(variation.to_string_lossy()),
        _ => None,
    }
}

/// The presentation-agnostic onboarding engine. It decides *when* and *whether* a scenario runs (eligibility,
/// scheduling, once-per-user persistence) and delegates *how* it is rendered to a registered presentation.
pub trait IOnboardingScenarioService {
    /// Begin watching scenario eligibility. Automatic scenarios may start as soon as they become eligible. Safe to call
    /// multiple times (no-op after first).
    fn start(&self);

    /// Run on demand, bypassing the once-per-user and `onboarding.enabled` gates but serialized with other scenarios.
    /// Cancellation aborts the queued or active run (including joined callers) and waits for presentation cleanup.
    fn run_scenario(&self, id: &str, token: CancellationToken) -> Promise<OnboardingOutcome>;

    /// All registered scenarios (across presentation kinds). Useful for a tutorial page that lists everything the user
    /// can replay.
    fn get_scenarios(&self) -> Vec<Rc<OnboardingScenario>>;

    /// Whether the scenario has already been shown to the user.
    fn has_been_shown(&self, id: &str) -> bool;

    /// Checks eligibility before a pre-tour nudge and records experiment exposure in both arms. A reader tracks
    /// assignment resolution.
    fn should_show_nudge(&self, id: &str, reader: Option<&dyn IReader>) -> bool;

    /// Clear persisted and in-memory "shown" state so a single scenario can be retried.
    fn reset(&self, id: &str);

    /// Clear persisted "shown" state for all scenarios without resetting developer-mode replay guards.
    fn reset_all(&self);
}
