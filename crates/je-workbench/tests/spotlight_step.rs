//! Porte de `src/vs/workbench/contrib/onboarding/test/browser/spotlightStep.test.ts` (suíte `Spotlight prerequisite
//! steps`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o [`LeakGuard`] criado no começo de cada caso.
//! - `createOnboardingClickStep({ ... })` recebe o próprio [`SpotlightStep`] (o `Omit` do original some em tempo de
//!   compilação); `createOnboardingContextStep({ ..., completeWhen })` recebe o passo e o `completeWhen` em
//!   [`OnboardingContextStepOptions`].
//! - `assert.deepStrictEqual` é `assert_eq!` sobre o [`OnboardingSequenceStep`] e o [`SpotlightStep`], que comparam
//!   por valor (expressões de contexto por `equals`). `missingTarget: { kind: 'wait', timeoutMs: 10_000 }` é
//!   `Wait { timeout_ms: 10000.0, on_timeout: None }`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::contextkey::contextkey::ContextKeyExpr;
use je_workbench::contrib::onboarding::onboarding_sequence::OnboardingSequenceStep;
use je_workbench::contrib::onboarding::spotlight_step::{
    OnboardingContextStepOptions, create_onboarding_click_step, create_onboarding_context_step,
};
use je_workbench::contrib::onboarding::spotlight_types::{
    SPOTLIGHT_PRESENTATION_KIND, SpotlightMissingTargetBehavior, SpotlightOpenTarget, SpotlightStep,
    SpotlightTargetClickBehavior,
};

fn wait_ten_seconds() -> SpotlightMissingTargetBehavior {
    SpotlightMissingTargetBehavior::Wait { timeout_ms: 10_000.0, on_timeout: None }
}

#[test]
fn creates_an_interactive_click_completed_step_with_bounded_target_wait() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        create_onboarding_click_step(SpotlightStep {
            id: "choose".to_string(),
            target_id: "example.choose".to_string(),
            title: "Choose an item".to_string(),
            description: "Select the item to continue.".into(),
            ..Default::default()
        }),
        OnboardingSequenceStep {
            id: "choose".to_string(),
            kind: SPOTLIGHT_PRESENTATION_KIND.to_string(),
            payload: SpotlightStep {
                id: "choose".to_string(),
                target_id: "example.choose".to_string(),
                title: "Choose an item".to_string(),
                description: "Select the item to continue.".into(),
                allow_target_interaction: Some(true),
                advance_on_target_click: Some(SpotlightTargetClickBehavior::Bool(true)),
                missing_target: Some(wait_ten_seconds()),
                ..Default::default()
            },
        }
    );
}

#[test]
fn creates_an_interactive_context_completed_step_that_opens_its_target() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let complete_when = ContextKeyExpr::has("exampleSelected");

    assert_eq!(
        create_onboarding_context_step(OnboardingContextStepOptions {
            step: SpotlightStep {
                id: "select".to_string(),
                target_id: "example.selector".to_string(),
                title: "Select a value".to_string(),
                description: "Choose a value to continue.".into(),
                ..Default::default()
            },
            complete_when: complete_when.clone(),
        }),
        OnboardingSequenceStep {
            id: "select".to_string(),
            kind: SPOTLIGHT_PRESENTATION_KIND.to_string(),
            payload: SpotlightStep {
                id: "select".to_string(),
                target_id: "example.selector".to_string(),
                title: "Select a value".to_string(),
                description: "Choose a value to continue.".into(),
                open_target: Some(SpotlightOpenTarget::Bool(true)),
                allow_target_interaction: Some(true),
                advance_when: Some(complete_when),
                missing_target: Some(wait_ten_seconds()),
                ..Default::default()
            },
        }
    );
}

#[test]
fn preserves_explicit_interaction_and_target_behavior() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    assert_eq!(
        create_onboarding_context_step(OnboardingContextStepOptions {
            step: SpotlightStep {
                id: "acknowledge".to_string(),
                target_id: "example.status".to_string(),
                title: "Review status".to_string(),
                description: "Review the status, then continue.".into(),
                open_target: Some(SpotlightOpenTarget::Bool(false)),
                allow_target_interaction: Some(false),
                missing_target: Some(SpotlightMissingTargetBehavior::Abort),
                ..Default::default()
            },
            complete_when: ContextKeyExpr::true_(),
        })
        .payload,
        SpotlightStep {
            id: "acknowledge".to_string(),
            target_id: "example.status".to_string(),
            title: "Review status".to_string(),
            description: "Review the status, then continue.".into(),
            open_target: Some(SpotlightOpenTarget::Bool(false)),
            allow_target_interaction: Some(false),
            advance_when: Some(ContextKeyExpr::true_()),
            missing_target: Some(SpotlightMissingTargetBehavior::Abort),
            ..Default::default()
        }
    );
}
