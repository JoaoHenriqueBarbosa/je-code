//! Porte de `src/vs/workbench/contrib/onboarding/browser/sequence/runOnboardingStep.ts`: o passo de sequência sem
//! UI, que roda um callback cancelável uma vez e segue em frente depois de reportar os erros.
//!
//! Divergências:
//! - `payload as IRunOnboardingStepPayload` (o `payload` do passo é `unknown`) é o [`Dynamic`] objeto que carrega o
//!   [`RunOnboardingStepPayload`] como valor nativo (`ObjectRef::with_host`). Um payload sem ele faz o acesso a `run`
//!   lançar `TypeError`, que cai no `catch` como no original.
//! - `run: (token) => Promise<R | void> | R | void` é `Rc<dyn Fn(&CancellationToken) -> MaybePromise<Option<R>>>`
//!   (`void` é `None`). Um `throw` síncrono do callback é um pânico capturado por [`errors::catch`], e uma promessa
//!   rejeitada cai no mesmo `catch`.
//! - `Promise.resolve(x)` é a própria promessa, ou [`Promise::resolved`] para um valor direto.
//! - `raceCancellation` resolve com `None` ao cancelar; o `result` fica `undefined`, como no original.

use std::rc::Rc;

use je_base::async_::{async_fn, race_cancellation};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::promise::{MaybePromise, Promise};

use super::onboarding_sequence::{
    IOnboardingSequenceStepPresentation, OnboardingSequenceStep, OnboardingSequenceStepAction, OnboardingSequenceStepContext,
    OnboardingSequenceStepResult,
};

pub const RUN_ONBOARDING_STEP_KIND: &str = "run";

/// `IRunOnboardingStepResult`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunOnboardingStepResult {
    pub shown: Option<bool>,
}

/// O callback de um passo sem UI.
pub type RunOnboardingStepCallback = Rc<dyn Fn(&CancellationToken) -> MaybePromise<Option<RunOnboardingStepResult>>>;

/// `IRunOnboardingStepPayload`: o payload de um passo de sequência sem UI e cancelável.
#[derive(Clone)]
pub struct RunOnboardingStepPayload {
    pub run: RunOnboardingStepCallback,
}

fn payload_of(step: &OnboardingSequenceStep) -> Result<Rc<RunOnboardingStepPayload>, Error> {
    let payload = match &step.payload {
        Dynamic::Object(object) => object.host::<RunOnboardingStepPayload>(),
        _ => None,
    };
    payload.ok_or_else(|| Error::named("TypeError", "Cannot read properties of undefined (reading 'run')"))
}

fn step_result(action: OnboardingSequenceStepAction, shown: bool) -> OnboardingSequenceStepResult {
    OnboardingSequenceStepResult { action, shown, dismiss_reason: None }
}

/// `RunOnboardingStepPresentation`: executa o callback de um passo uma vez e continua depois dos erros reportados.
pub struct RunOnboardingStepPresentation;

impl IOnboardingSequenceStepPresentation for RunOnboardingStepPresentation {
    fn kind(&self) -> &str {
        RUN_ONBOARDING_STEP_KIND
    }

    fn counts_as_visual_step(&self) -> bool {
        false
    }

    fn run_once(&self) -> bool {
        true
    }

    fn run_step(&self, step: &OnboardingSequenceStep, context: &OnboardingSequenceStepContext) -> Promise<OnboardingSequenceStepResult> {
        let token = context.cancellation_token.clone();
        let step = step.clone();
        async_fn(async move {
            if token.is_cancellation_requested() {
                return Ok(step_result(OnboardingSequenceStepAction::Abort, false));
            }
            let started = payload_of(&step).and_then(|payload| errors::catch(|| (payload.run)(&token)));
            let outcome = match started {
                Ok(value) => {
                    let promise = match value {
                        MaybePromise::Value(value) => Promise::resolved(value),
                        MaybePromise::Promise(promise) => promise,
                    };
                    race_cancellation(&promise, &token).await
                }
                Err(error) => Err(error),
            };
            let result = match outcome {
                Ok(raced) => raced.flatten(),
                Err(error) => {
                    if !token.is_cancellation_requested() {
                        errors::on_unexpected_error(&error);
                    }
                    None
                }
            };
            Ok(if token.is_cancellation_requested() {
                step_result(OnboardingSequenceStepAction::Abort, false)
            } else {
                step_result(OnboardingSequenceStepAction::Next, result.is_some_and(|result| result.shown == Some(true)))
            })
        })
    }
}
