//! Porte de `src/vs/workbench/contrib/onboarding/browser/spotlight/spotlightStep.ts`: as fábricas dos passos de
//! pré-requisito do spotlight, concluídos por clique no alvo ou por uma expressão de contexto.
//!
//! Divergências:
//! - `IOnboardingClickStepOptions = Omit<ISpotlightStep, 'advanceOnTargetClick' | 'advanceWhen'>` é o próprio
//!   [`SpotlightStep`]: o `Omit` do TypeScript proíbe os dois campos em tempo de compilação, e aqui as fábricas
//!   descartam o valor que vier neles (a de clique define `advance_on_target_click` e a de contexto define os dois).
//! - `IOnboardingContextStepOptions extends IOnboardingClickStepOptions` é a struct [`OnboardingContextStepOptions`],
//!   que carrega o passo e o `completeWhen`.

use je_platform::contextkey::contextkey::ContextKeyExpression;

use super::onboarding_sequence::OnboardingSequenceStep;
use super::spotlight_types::{
    SPOTLIGHT_PRESENTATION_KIND, SpotlightMissingTargetBehavior, SpotlightOpenTarget, SpotlightStep,
    SpotlightTargetClickBehavior,
};

const DEFAULT_TARGET_WAIT_TIMEOUT: f64 = 10_000.0;

/// `IOnboardingClickStepOptions`.
pub type OnboardingClickStepOptions = SpotlightStep;

/// `IOnboardingContextStepOptions`.
#[derive(Clone)]
pub struct OnboardingContextStepOptions {
    pub step: OnboardingClickStepOptions,
    /// O estado produzido pela ação pedida ao usuário.
    pub complete_when: ContextKeyExpression,
}

fn default_missing_target() -> SpotlightMissingTargetBehavior {
    SpotlightMissingTargetBehavior::Wait { timeout_ms: DEFAULT_TARGET_WAIT_TIMEOUT, on_timeout: None }
}

pub fn create_onboarding_click_step(options: OnboardingClickStepOptions) -> OnboardingSequenceStep<SpotlightStep> {
    create_step(SpotlightStep {
        allow_target_interaction: Some(options.allow_target_interaction.unwrap_or(true)),
        advance_on_target_click: Some(SpotlightTargetClickBehavior::Bool(true)),
        missing_target: Some(options.missing_target.unwrap_or_else(default_missing_target)),
        advance_when: None,
        ..options
    })
}

pub fn create_onboarding_context_step(options: OnboardingContextStepOptions) -> OnboardingSequenceStep<SpotlightStep> {
    let OnboardingContextStepOptions { step, complete_when } = options;
    create_step(SpotlightStep {
        open_target: Some(step.open_target.unwrap_or(SpotlightOpenTarget::Bool(true))),
        allow_target_interaction: Some(step.allow_target_interaction.unwrap_or(true)),
        advance_on_target_click: None,
        advance_when: Some(complete_when),
        missing_target: Some(step.missing_target.unwrap_or_else(default_missing_target)),
        ..step
    })
}

fn create_step(step: SpotlightStep) -> OnboardingSequenceStep<SpotlightStep> {
    OnboardingSequenceStep { id: step.id.clone(), kind: SPOTLIGHT_PRESENTATION_KIND.to_string(), payload: step }
}
