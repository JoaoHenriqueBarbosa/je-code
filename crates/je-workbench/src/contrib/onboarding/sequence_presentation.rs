//! Porte de `src/vs/workbench/contrib/onboarding/browser/sequence/sequencePresentation.ts`: roda uma sequência
//! heterogênea deixando o desenho de cada passo para o `kind` registrado dele.
//!
//! Divergências:
//! - `(scenario.presentation.payload as IOnboardingSequencePayload)?.steps ?? []` é o [`Dynamic`] objeto que carrega o
//!   [`OnboardingSequencePayload`] como valor nativo (`ObjectRef::with_host`); qualquer outro payload vale `[]`.
//! - `async run` é [`async_fn`]; o `await presentation.runStep(...)` propaga a rejeição (`?`) e o `finally` que
//!   descarta o `store` roda depois do corpo, qualquer que seja o resultado.
//! - `index` e `direction` são `i64` (o índice chega a `-1` ao voltar do primeiro passo); os índices guardados nos
//!   conjuntos e no resultado são `usize`.
//! - `extends Disposable` é o `Disposable` guardado em `base`.

use std::collections::BTreeSet;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::promise::Promise;

use super::onboarding_presentation::{IOnboardingPresentation, OnboardingRunContext};
use super::onboarding_scenario::{OnboardingDismissReason, OnboardingOutcome, OnboardingRunResult, OnboardingScenario};
use super::onboarding_sequence::{
    IOnboardingSequenceStepPresentation, ONBOARDING_SEQUENCE_PRESENTATION_KIND, OnboardingSequencePayload, OnboardingSequenceStep,
    OnboardingSequenceStepAction, OnboardingSequenceStepContext, onboarding_sequence_step_presentation_registry,
};

/// `OnboardingSequencePresentation`: roda uma sequência heterogênea deixando o desenho de cada passo para o `kind`
/// registrado dele.
pub struct OnboardingSequencePresentation {
    base: Rc<Disposable>,
}

impl OnboardingSequencePresentation {
    pub fn new() -> Rc<Self> {
        Rc::new(Self { base: Disposable::new() })
    }
}

impl IDisposable for OnboardingSequencePresentation {
    fn dispose(&self) {
        self.base.dispose();
    }
}

fn run_result(
    outcome: OnboardingOutcome,
    shown: bool,
    dismiss_reason: OnboardingDismissReason,
    last_step_index: usize,
    step_count: usize,
) -> OnboardingRunResult {
    OnboardingRunResult { outcome, shown, dismiss_reason, last_step_index: last_step_index as f64, step_count: step_count as f64 }
}

fn presentation_of(step: &OnboardingSequenceStep) -> Option<Rc<dyn IOnboardingSequenceStepPresentation>> {
    onboarding_sequence_step_presentation_registry().get(&step.kind)
}

fn visual_step_index(steps: &[OnboardingSequenceStep], index: usize, skipped_visual_steps: &BTreeSet<usize>) -> usize {
    (0..index)
        .filter(|step_index| {
            presentation_of(&steps[*step_index]).is_some_and(|presentation| presentation.counts_as_visual_step())
                && !skipped_visual_steps.contains(step_index)
        })
        .count()
}

fn steps_of(scenario: &OnboardingScenario) -> Vec<OnboardingSequenceStep> {
    let payload = match &scenario.presentation.payload {
        Dynamic::Object(object) => object.host::<OnboardingSequencePayload>(),
        _ => None,
    };
    payload.map(|payload| payload.steps.clone()).unwrap_or_default()
}

/// O laço de travessia do `run`, sem o `store` (que o chamador descarta no fim).
async fn traverse(
    steps: Vec<OnboardingSequenceStep>,
    context: OnboardingRunContext,
    store: Rc<DisposableStore>,
) -> Result<OnboardingRunResult, Error> {
    let step_count = steps.len();
    let cancellation = store.add_rc(Rc::new(CancellationTokenSource::new(None)));
    store.add(context.on_abort.listen({
        let cancellation = cancellation.clone();
        move |_| cancellation.cancel()
    }));
    let mut skipped_visual_steps = BTreeSet::<usize>::new();
    let mut shown_visual_steps = BTreeSet::<usize>::new();
    let mut executed_run_once_steps = BTreeSet::<usize>::new();
    let visual_step_count =
        steps.iter().filter(|step| presentation_of(step).is_some_and(|presentation| presentation.counts_as_visual_step())).count();
    let mut index: i64 = 0;
    let mut direction: i64 = 1;
    let mut shown = false;
    let mut last_step_index: usize = 0;
    let mut dismiss_reason = OnboardingDismissReason::Completed;

    while index >= 0 && (index as usize) < step_count && !cancellation.token().is_cancellation_requested() {
        let current = index as usize;
        let step = &steps[current];
        let Some(presentation) = presentation_of(step) else {
            errors::on_unexpected_error(&Error::new(format!("No onboarding sequence step presentation registered for '{}'.", step.kind)));
            return Ok(run_result(OnboardingOutcome::Aborted, shown, OnboardingDismissReason::Aborted, last_step_index, step_count));
        };

        if !presentation.counts_as_visual_step() && direction == -1 {
            index -= 1;
            continue;
        }
        if presentation.run_once() && executed_run_once_steps.contains(&current) {
            index += direction;
            continue;
        }
        if presentation.run_once() {
            executed_run_once_steps.insert(current);
        }
        if presentation.counts_as_visual_step() {
            skipped_visual_steps.remove(&current);
        }

        let visual_index = visual_step_index(&steps, current, &skipped_visual_steps);
        let current_visual_step_count = visual_step_count - skipped_visual_steps.len();
        if !presentation.counts_as_visual_step() {
            last_step_index = last_step_index.max(current);
        }
        let result = presentation
            .run_step(
                step,
                &OnboardingSequenceStepContext {
                    base: context.clone(),
                    cancellation_token: cancellation.token(),
                    step_index: current,
                    visual_step_index: visual_index,
                    visual_step_count: current_visual_step_count,
                    can_go_back: shown_visual_steps.iter().any(|step_index| *step_index < current),
                    is_last_visual_step: visual_index + 1 == current_visual_step_count,
                },
            )
            .await?;
        if cancellation.token().is_cancellation_requested() || result.action == OnboardingSequenceStepAction::Abort {
            return Ok(run_result(OnboardingOutcome::Aborted, shown, OnboardingDismissReason::Aborted, last_step_index, step_count));
        }

        if result.shown {
            shown = true;
            last_step_index = last_step_index.max(current);
            if presentation.counts_as_visual_step() {
                shown_visual_steps.insert(current);
            }
        }

        match result.action {
            OnboardingSequenceStepAction::Next => {
                dismiss_reason = result.dismiss_reason.unwrap_or(dismiss_reason);
                direction = 1;
                index += 1;
            }
            OnboardingSequenceStepAction::Back => {
                direction = -1;
                index -= 1;
            }
            OnboardingSequenceStepAction::SkipStep => {
                if presentation.counts_as_visual_step() {
                    skipped_visual_steps.insert(current);
                }
                index += direction;
            }
            OnboardingSequenceStepAction::SkipSequence => {
                return Ok(run_result(
                    OnboardingOutcome::Skipped,
                    shown,
                    result.dismiss_reason.unwrap_or(OnboardingDismissReason::SkipButton),
                    last_step_index,
                    step_count,
                ));
            }
            OnboardingSequenceStepAction::Abort => {}
        }
    }

    Ok(if cancellation.token().is_cancellation_requested() {
        run_result(OnboardingOutcome::Aborted, shown, OnboardingDismissReason::Aborted, last_step_index, step_count)
    } else {
        run_result(OnboardingOutcome::Completed, shown, dismiss_reason, last_step_index, step_count)
    })
}

impl IOnboardingPresentation for OnboardingSequencePresentation {
    fn kind(&self) -> &str {
        ONBOARDING_SEQUENCE_PRESENTATION_KIND
    }

    fn run(&self, scenario: &OnboardingScenario, context: &OnboardingRunContext) -> Promise<OnboardingRunResult> {
        let steps = steps_of(scenario);
        let context = context.clone();
        async_fn(async move {
            if steps.is_empty() {
                return Ok(run_result(OnboardingOutcome::Completed, false, OnboardingDismissReason::Completed, 0, 0));
            }
            let store = DisposableStore::new();
            let result = traverse(steps, context, store.clone()).await;
            store.dispose();
            result
        })
    }
}
