//! Porte dos tipos de dados de `src/vs/workbench/contrib/onboarding/common/onboardingSequence.ts`: o passo
//! heterogêneo de uma sequência, o payload e o resultado da travessia de um passo.
//!
//! Divergências:
//! - **Só os dados.** `IOnboardingSequenceStepContext` (herda `IOnboardingRunContext`), `IOnboardingSequenceStepPresentation`
//!   e o registro de apresentações por `kind` (`onboardingSequenceStepPresentationRegistry`) entram com a apresentação
//!   de onboarding (`onboardingPresentation.ts`), de que dependem.
//! - `IOnboardingSequenceStep<TPayload = unknown>` é genérico com [`Dynamic`] como padrão.

use je_base::dynamic::Dynamic;

use super::onboarding_scenario::OnboardingDismissReason;

pub const ONBOARDING_SEQUENCE_PRESENTATION_KIND: &str = "sequence";

/// `IOnboardingSequenceStep<TPayload>`: uma entrada de uma sequência heterogênea de onboarding.
#[derive(Debug, Clone, PartialEq)]
pub struct OnboardingSequenceStep<P = Dynamic> {
    pub id: String,
    pub kind: String,
    pub payload: P,
}

/// `IOnboardingSequencePayload`: o payload desenhado pela apresentação de sequência.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OnboardingSequencePayload {
    pub steps: Vec<OnboardingSequenceStep>,
}

/// O `action` de `IOnboardingSequenceStepResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingSequenceStepAction {
    Next,
    Back,
    SkipStep,
    SkipSequence,
    Abort,
}

/// `IOnboardingSequenceStepResult`: o resultado da travessia de um passo da sequência.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnboardingSequenceStepResult {
    pub action: OnboardingSequenceStepAction,
    pub shown: bool,
    pub dismiss_reason: Option<OnboardingDismissReason>,
}
