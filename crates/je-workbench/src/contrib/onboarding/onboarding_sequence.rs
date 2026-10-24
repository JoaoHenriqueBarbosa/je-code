//! Porte de `src/vs/workbench/contrib/onboarding/common/onboardingSequence.ts`: o passo heterogêneo de uma
//! sequência, o payload, o contexto e o resultado da travessia de um passo, a apresentação de um `kind` de passo e o
//! registro dessas apresentações.
//!
//! Divergências:
//! - `IOnboardingSequenceStep<TPayload = unknown>` é genérico com [`Dynamic`] como padrão.
//! - `IOnboardingSequenceStepContext extends IOnboardingRunContext` (com o spread `...context` da apresentação de
//!   sequência) guarda o contexto herdado em `base`. Os índices e contadores (`number`) são `usize`.
//! - `runOnce?: boolean` é o método [`IOnboardingSequenceStepPresentation::run_once`], com `false` como padrão.
//! - O registro por `kind` é o [`KindRegistry`] genérico de `onboarding_presentation.rs` (mesmo corpo do original, só
//!   muda o texto do erro de duplicata). O global (`onboardingSequenceStepPresentationRegistry`) é
//!   [`onboarding_sequence_step_presentation_registry`], uma instância por thread.

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::promise::Promise;

use super::onboarding_presentation::{KindRegistry, OnboardingRunContext};
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

/// `IOnboardingSequenceStepContext`: o contexto passado à apresentação de um passo da sequência.
#[derive(Clone)]
pub struct OnboardingSequenceStepContext {
    /// O contexto da execução, herdado de `IOnboardingRunContext`.
    pub base: OnboardingRunContext,
    pub cancellation_token: CancellationToken,
    pub step_index: usize,
    pub visual_step_index: usize,
    pub visual_step_count: usize,
    pub can_go_back: bool,
    pub is_last_visual_step: bool,
}

/// `IOnboardingSequenceStepPresentation`: desenha um `kind` de passo de sequência.
pub trait IOnboardingSequenceStepPresentation {
    fn kind(&self) -> &str;

    /// Se este `kind` participa dos contadores de progresso visuais.
    fn counts_as_visual_step(&self) -> bool;

    /// Se uma revisita para a frente pula um passo que já rodou.
    fn run_once(&self) -> bool {
        false
    }

    fn run_step(&self, step: &OnboardingSequenceStep, context: &OnboardingSequenceStepContext) -> Promise<OnboardingSequenceStepResult>;
}

thread_local! {
    static ONBOARDING_SEQUENCE_STEP_PRESENTATION_REGISTRY: Rc<KindRegistry<dyn IOnboardingSequenceStepPresentation>> = Rc::new(KindRegistry::new(
        "An onboarding sequence step presentation",
        |presentation| presentation.kind().to_string(),
    ));
}

/// `onboardingSequenceStepPresentationRegistry`: o registro global das apresentações de passo de sequência.
pub fn onboarding_sequence_step_presentation_registry() -> Rc<KindRegistry<dyn IOnboardingSequenceStepPresentation>> {
    ONBOARDING_SEQUENCE_STEP_PRESENTATION_REGISTRY.with(Rc::clone)
}
