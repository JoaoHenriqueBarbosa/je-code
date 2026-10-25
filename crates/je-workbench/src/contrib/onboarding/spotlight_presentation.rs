//! Porte de `src/vs/workbench/contrib/onboarding/browser/spotlight/spotlightPresentation.ts`: a apresentação dos
//! cenários de spotlight, que escurece a janela, percorre os passos e mostra um balão ancorado em cada um.
//!
//! Divergências:
//! - **Os serviços.** `IWorkbenchLayoutService` e `IHostService` do porte não têm contêiner de DOM nem
//!   `setWindowDimmed`; a apresentação lê só o que usa, em dois traços nativos: [`ISpotlightLayoutService`]
//!   (`getContainer(window)` e `onDidLayoutContainer`) e [`ISpotlightHostService`] (`setWindowDimmed`). O
//!   `IContextKeyService` é o do porte.
//! - **`Window` e `HTMLElement`** são a [`OnboardingWindow`] e o [`OnboardingElement`] do modelo sem DOM.
//! - **Os payloads** viajam em [`Dynamic`] como valor nativo de um objeto (`ObjectRef::with_host`):
//!   [`spotlight_payload_value`] e [`spotlight_step_value`] montam, e [`spotlight_payload_of`] e
//!   [`spotlight_step_of`] leem. Um payload que não é do tipo certo vale `steps: []`; um passo de sequência com payload
//!   inválido rejeita a promessa do `runStep`.
//! - **`async`** é [`async_fn`]; o `await` propaga a rejeição (`?`), o `try/finally` que descarta o `store` roda depois
//!   do corpo, e o `try/catch` com `onUnexpectedError` em volta dos ganchos é `errors::catch` (a exceção síncrona) mais
//!   a rejeição da promessa devolvida.
//! - **`getAnimations()`** (esperar as animações finitas do alvo e dos ancestrais terminarem, no máximo 600 ms) não
//!   tem equivalente: o modelo de elemento não tem animações; sobra o quadro de animação que o original espera depois.
//! - **`timeout(ms, token)`** é `timeout_with_token`; `Date.now()` é `runtime::date_now`.
//! - **`extends Disposable`** é o `Disposable` guardado em `base`; o estado compartilhado com as promessas (que não
//!   podem emprestar `self`) é o `Inner`, um `Rc` clonado para dentro delas.
//! - `readonly kind` e `countsAsVisualStep` são os métodos dos traços de apresentação.

use std::cell::Cell;
use std::collections::{BTreeSet, HashSet};
use std::rc::Rc;

use je_base::async_::{async_fn, timeout_with_token};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_platform::contextkey::contextkey::IContextKeyService;

use super::onboarding_presentation::{IOnboardingPresentation, OnboardingRunContext};
use super::onboarding_scenario::{OnboardingDismissReason, OnboardingOutcome, OnboardingRunResult, OnboardingScenario};
use super::onboarding_sequence::{
    IOnboardingSequenceStepPresentation, OnboardingSequenceStep, OnboardingSequenceStepAction, OnboardingSequenceStepContext,
    OnboardingSequenceStepResult,
};
use super::onboarding_target::{
    OnboardingTarget, OnboardingWindow, has_onboarding_target_selection, on_did_select_onboarding_target, resolve_onboarding_target,
};
use super::spotlight_overlay::{
    SpotlightAdvanceSource, SpotlightContainer, SpotlightContent, SpotlightOverlay, SpotlightShowOptions,
};
use super::spotlight_types::{
    SPOTLIGHT_PRESENTATION_KIND, SpotlightMissingTargetBehavior, SpotlightOpenTarget, SpotlightPayload, SpotlightStep,
    SpotlightTimeoutAction,
};

/// How long to wait for a step's target element to appear before skipping it.
const TARGET_RESOLVE_TIMEOUT: f64 = 2000.0;
const TARGET_POLL_INTERVAL: u64 = 50;

/// O que a apresentação lê do serviço de layout do workbench.
pub trait ISpotlightLayoutService {
    /// `getContainer(window)`: o contêiner que hospeda o overlay.
    fn get_container(&self, window: &OnboardingWindow) -> Rc<SpotlightContainer>;

    /// `onDidLayoutContainer`.
    fn on_did_layout_container(&self) -> Event<()>;
}

/// O que a apresentação usa do serviço de host.
pub trait ISpotlightHostService {
    /// `setWindowDimmed(window, dimmed)`: escurece a janela, inclusive os controles nativos dela.
    fn set_window_dimmed(&self, window: &OnboardingWindow, dimmed: bool);
}

/// O payload de um cenário de spotlight como valor nativo de um objeto.
pub fn spotlight_payload_value(payload: SpotlightPayload) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(payload)))
}

/// O passo de spotlight como payload de um passo de sequência.
pub fn spotlight_step_value(step: SpotlightStep) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(step)))
}

/// `scenario.presentation.payload as ISpotlightPayload`.
pub fn spotlight_payload_of(value: &Dynamic) -> Option<Rc<SpotlightPayload>> {
    match value {
        Dynamic::Object(object) => object.host::<SpotlightPayload>(),
        _ => None,
    }
}

/// `sequenceStep.payload as ISpotlightStep`.
pub fn spotlight_step_of(value: &Dynamic) -> Option<Rc<SpotlightStep>> {
    match value {
        Dynamic::Object(object) => object.host::<SpotlightStep>(),
        _ => None,
    }
}

fn should_abort_on_missing_target(behavior: Option<SpotlightMissingTargetBehavior>) -> bool {
    matches!(
        behavior,
        Some(SpotlightMissingTargetBehavior::Abort)
            | Some(SpotlightMissingTargetBehavior::Wait { on_timeout: Some(SpotlightTimeoutAction::Abort), .. })
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NextVia {
    Button,
    Target,
    Condition,
}

/// The terminal action of a single step, carrying the data needed for telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StepEnd {
    Next(NextVia),
    Back,
    Skip(OnboardingDismissReason),
    MissingTarget,
    Abort,
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

fn sequence_result(
    action: OnboardingSequenceStepAction,
    shown: bool,
    dismiss_reason: Option<OnboardingDismissReason>,
) -> OnboardingSequenceStepResult {
    OnboardingSequenceStepResult { action, shown, dismiss_reason }
}

struct Inner {
    layout: Rc<dyn ISpotlightLayoutService>,
    host: Rc<dyn ISpotlightHostService>,
    context_key_service: Rc<dyn IContextKeyService>,
}

/// Renders [`SpotlightPayload`] scenarios: it dims the window (including the native window controls), walks the
/// steps, and shows an anchored callout for each. Implements the engine's [`IOnboardingPresentation`] contract so
/// the scenario engine can drive it without knowing anything about spotlights.
pub struct SpotlightPresentation {
    base: Rc<Disposable>,
    inner: Rc<Inner>,
}

impl SpotlightPresentation {
    pub fn new(
        layout: Rc<dyn ISpotlightLayoutService>,
        host: Rc<dyn ISpotlightHostService>,
        context_key_service: Rc<dyn IContextKeyService>,
    ) -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), inner: Rc::new(Inner { layout, host, context_key_service }) })
    }

    /// `readonly countsAsVisualStep = true`.
    pub const COUNTS_AS_VISUAL_STEP: bool = true;
}

impl IDisposable for SpotlightPresentation {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl IOnboardingPresentation for SpotlightPresentation {
    fn kind(&self) -> &str {
        SPOTLIGHT_PRESENTATION_KIND
    }

    fn run(&self, scenario: &OnboardingScenario, context: &OnboardingRunContext) -> Promise<OnboardingRunResult> {
        let payload = spotlight_payload_of(&scenario.presentation.payload);
        let (inner, context) = (self.inner.clone(), context.clone());
        async_fn(run_payload(inner, payload, context))
    }
}

impl IOnboardingSequenceStepPresentation for SpotlightPresentation {
    fn kind(&self) -> &str {
        SPOTLIGHT_PRESENTATION_KIND
    }

    fn counts_as_visual_step(&self) -> bool {
        Self::COUNTS_AS_VISUAL_STEP
    }

    fn run_step(&self, step: &OnboardingSequenceStep, context: &OnboardingSequenceStepContext) -> Promise<OnboardingSequenceStepResult> {
        let payload = spotlight_step_of(&step.payload);
        let (inner, context, id) = (self.inner.clone(), context.clone(), step.id.clone());
        async_fn(async move {
            let Some(step) = payload else {
                return Err(Error::new(format!("The spotlight step '{id}' has an invalid payload.")));
            };
            run_sequence_step(inner, step, context).await
        })
    }
}

/// `await step.onBeforeShow?.()` dentro de um `try/catch` que reporta o erro.
async fn run_before_show(step: &SpotlightStep) {
    let result = match &step.on_before_show {
        Some(hook) => match errors::catch(|| hook()) {
            Ok(MaybePromise::Promise(promise)) => promise.await,
            Ok(MaybePromise::Value(())) => Promise::resolved(()).await,
            Err(error) => Err(error),
        },
        None => Promise::resolved(()).await,
    };
    if let Err(error) = result {
        errors::on_unexpected_error(&error);
    }
}

fn step_is_skipped_by_when(inner: &Inner, step: &SpotlightStep) -> bool {
    step.when.is_some() && !inner.context_key_service.context_matches_rules(step.when.as_ref())
}

/// O overlay do passo: o contêiner, a janela escurecida e o relayout a cada layout do contêiner. Tudo vai para o `store`.
fn open_overlay(inner: &Inner, window: &OnboardingWindow, store: &DisposableStore) -> Rc<SpotlightOverlay> {
    let container = inner.layout.get_container(window);
    let overlay = store.add_rc(SpotlightOverlay::new(window, &container));
    inner.host.set_window_dimmed(window, true);
    store.add(to_disposable({
        let (host, window) = (inner.host.clone(), window.clone());
        move || host.set_window_dimmed(&window, false)
    }));
    // Keep the callout glued to the target as the workbench re-layouts. Schedule the measurement so it runs after the
    // layout event's work has settled, including position-only shifts.
    store.add(inner.layout.on_did_layout_container().listen({
        let overlay = overlay.clone();
        move |_| overlay.schedule_layout()
    }));
    overlay
}

async fn run_sequence_step(
    inner: Rc<Inner>,
    step: Rc<SpotlightStep>,
    context: OnboardingSequenceStepContext,
) -> Result<OnboardingSequenceStepResult, Error> {
    use OnboardingSequenceStepAction as Action;

    if step_is_skipped_by_when(&inner, &step) {
        return Ok(sequence_result(Action::SkipStep, false, None));
    }

    run_before_show(&step).await;
    if context.cancellation_token.is_cancellation_requested() {
        return Ok(sequence_result(Action::Abort, false, None));
    }

    let window = context.base.target_window.clone();
    let target = resolve_target(
        &window,
        &step.target_id,
        context.base.target_scope.as_deref(),
        &context.cancellation_token,
        step.missing_target,
    )
    .await?;
    let Some(target) = target else {
        return Ok(if context.cancellation_token.is_cancellation_requested() || should_abort_on_missing_target(step.missing_target) {
            sequence_result(Action::Abort, false, None)
        } else {
            sequence_result(Action::SkipStep, false, None)
        });
    };
    let store = DisposableStore::new();
    let result = async {
        let overlay = open_overlay(&inner, &window, &store);
        let end = run_step(
            &inner,
            &overlay,
            &context.base,
            &step,
            target,
            context.visual_step_index,
            context.visual_step_count,
            context.can_go_back,
            context.is_last_visual_step,
        )
        .await?;
        overlay.hide();
        Ok::<OnboardingSequenceStepResult, Error>(match end {
            StepEnd::Next(via) => sequence_result(
                Action::Next,
                true,
                Some(if via == NextVia::Target { OnboardingDismissReason::TargetClick } else { OnboardingDismissReason::Completed }),
            ),
            StepEnd::Back => sequence_result(Action::Back, true, None),
            StepEnd::Skip(reason) => sequence_result(Action::SkipSequence, true, Some(reason)),
            StepEnd::MissingTarget => sequence_result(Action::SkipStep, true, None),
            StepEnd::Abort => sequence_result(Action::Abort, true, None),
        })
    }
    .await;
    store.dispose();
    result
}

async fn run_payload(
    inner: Rc<Inner>,
    payload: Option<Rc<SpotlightPayload>>,
    context: OnboardingRunContext,
) -> Result<OnboardingRunResult, Error> {
    let store = DisposableStore::new();
    let result = run_payload_body(&inner, payload, context, &store).await;
    store.dispose();
    result
}

async fn run_payload_body(
    inner: &Rc<Inner>,
    payload: Option<Rc<SpotlightPayload>>,
    context: OnboardingRunContext,
    store: &Rc<DisposableStore>,
) -> Result<OnboardingRunResult, Error> {
    // Furthest step the user actually saw (0-based). Stays at the last shown step regardless of how the run ends, for
    // telemetry.
    let mut last_step_index: usize = 0;
    // Whether at least one step was actually rendered. Stays `false` if every step is skipped (missing target or
    // unsatisfied `when`) so nothing was ever displayed.
    let mut shown = false;
    let mut skipped_step_indexes = BTreeSet::<usize>::new();

    let aborted = Rc::new(Cell::new(false));
    let target_resolution_cancellation = store.add_rc(Rc::new(CancellationTokenSource::new(None)));
    store.add(context.on_abort.listen({
        let (aborted, cancellation) = (aborted.clone(), target_resolution_cancellation.clone());
        move |_| {
            aborted.set(true);
            cancellation.cancel();
        }
    }));

    let steps: Vec<SpotlightStep> = match payload.as_ref().and_then(|payload| payload.resolve_steps.clone()) {
        Some(resolve_steps) => resolve_steps().await?,
        None => payload.as_ref().map(|payload| payload.steps.clone()).unwrap_or_default(),
    };
    let step_count = steps.len();
    if aborted.get() {
        return Ok(run_result(OnboardingOutcome::Aborted, false, OnboardingDismissReason::Aborted, 0, step_count));
    }
    if step_count == 0 {
        return Ok(run_result(OnboardingOutcome::Completed, false, OnboardingDismissReason::Completed, 0, step_count));
    }

    let overlay = open_overlay(inner, &context.target_window, store);

    let mut index: i64 = 0;
    let mut direction: i64 = 1;

    while index >= 0 && (index as usize) < step_count && !aborted.get() {
        let current = index as usize;
        let step = Rc::new(steps[current].clone());

        if step_is_skipped_by_when(inner, &step) {
            skipped_step_indexes.insert(current);
            index += direction;
            continue;
        }

        run_before_show(&step).await;
        if aborted.get() {
            break;
        }

        let target = resolve_target(
            &context.target_window,
            &step.target_id,
            context.target_scope.as_deref(),
            &target_resolution_cancellation.token(),
            step.missing_target,
        )
        .await?;
        if aborted.get() {
            break;
        }
        let Some(target) = target else {
            if should_abort_on_missing_target(step.missing_target) {
                aborted.set(true);
                break;
            }
            skipped_step_indexes.insert(current);
            index += direction;
            continue;
        };
        skipped_step_indexes.remove(&current);

        last_step_index = last_step_index.max(current);
        shown = true;

        let skipped_before = skipped_step_indexes.iter().filter(|skipped_index| **skipped_index < current).count();
        let display_step_index = current - skipped_before;
        let display_step_count = step_count - skipped_step_indexes.len();
        let end = run_step(
            inner,
            &overlay,
            &context,
            &step,
            target,
            display_step_index,
            display_step_count,
            display_step_index > 0,
            display_step_index + 1 == display_step_count,
        )
        .await?;
        overlay.hide();
        match end {
            StepEnd::Next(via) => {
                if current == step_count - 1 {
                    // Advancing past the final step completes the tour.
                    let dismiss_reason =
                        if via == NextVia::Target { OnboardingDismissReason::TargetClick } else { OnboardingDismissReason::Completed };
                    return Ok(run_result(OnboardingOutcome::Completed, shown, dismiss_reason, last_step_index, step_count));
                }
                direction = 1;
                index += 1;
            }
            StepEnd::Back => {
                direction = -1;
                index -= 1;
            }
            StepEnd::MissingTarget => {
                skipped_step_indexes.insert(current);
                index += direction;
            }
            StepEnd::Skip(reason) => {
                return Ok(run_result(OnboardingOutcome::Skipped, shown, reason, last_step_index, step_count));
            }
            StepEnd::Abort => {
                return Ok(run_result(OnboardingOutcome::Aborted, shown, OnboardingDismissReason::Aborted, last_step_index, step_count));
            }
        }
    }

    if aborted.get() {
        return Ok(run_result(OnboardingOutcome::Aborted, shown, OnboardingDismissReason::Aborted, last_step_index, step_count));
    }
    Ok(run_result(OnboardingOutcome::Completed, shown, OnboardingDismissReason::Completed, last_step_index, step_count))
}

async fn resolve_target(
    window: &OnboardingWindow,
    target_id: &str,
    target_scope: Option<&str>,
    cancellation_token: &CancellationToken,
    behavior: Option<SpotlightMissingTargetBehavior>,
) -> Result<Option<OnboardingTarget>, Error> {
    if cancellation_token.is_cancellation_requested() {
        return Ok(None);
    }
    let timeout_ms = match behavior {
        Some(SpotlightMissingTargetBehavior::Wait { timeout_ms, .. }) => timeout_ms.max(0.0),
        _ => TARGET_RESOLVE_TIMEOUT,
    };
    let deadline = runtime::date_now() as f64 + timeout_ms;
    while !cancellation_token.is_cancellation_requested() {
        if let Some(target) = resolve_onboarding_target(window, target_id, target_scope) {
            wait_for_target_ready(window).await;
            if cancellation_token.is_cancellation_requested() {
                return Ok(None);
            }
            // A picker can be replaced while its render or animation settles.
            let current = resolve_onboarding_target(window, target_id, target_scope);
            if let Some(current) = current
                && current.element == target.element
            {
                return Ok(Some(current));
            }
        }
        if matches!(behavior, Some(SpotlightMissingTargetBehavior::Skip) | Some(SpotlightMissingTargetBehavior::Abort))
            || runtime::date_now() as f64 >= deadline
        {
            return Ok(None);
        }
        if let Err(error) = timeout_with_token(TARGET_POLL_INTERVAL, cancellation_token).await {
            if cancellation_token.is_cancellation_requested() {
                return Ok(None);
            }
            return Err(error);
        }
    }
    Ok(None)
}

/// Espera o próximo quadro de animação (`requestAnimationFrame`).
async fn wait_for_target_ready(window: &OnboardingWindow) {
    let (promise, resolver) = Promise::<()>::new();
    window.request_animation_frame(move || resolver.resolve(()));
    let _ = promise.await;
}

#[allow(clippy::too_many_arguments)]
async fn run_step(
    inner: &Rc<Inner>,
    overlay: &Rc<SpotlightOverlay>,
    context: &OnboardingRunContext,
    step: &Rc<SpotlightStep>,
    target: OnboardingTarget,
    index: usize,
    step_count: usize,
    can_go_back: bool,
    is_last_step: bool,
) -> Result<StepEnd, Error> {
    let store = DisposableStore::new();
    let cancellation = Rc::new(CancellationTokenSource::new(None));
    store.add(to_disposable({
        let cancellation = cancellation.clone();
        move || cancellation.dispose_with(true)
    }));
    store.add(context.on_abort.listen({
        let cancellation = cancellation.clone();
        move |_| cancellation.cancel()
    }));
    let result = run_step_loop(inner, overlay, context, step, target, index, step_count, can_go_back, is_last_step, &cancellation).await;
    store.dispose();
    result
}

#[allow(clippy::too_many_arguments)]
async fn run_step_loop(
    inner: &Rc<Inner>,
    overlay: &Rc<SpotlightOverlay>,
    context: &OnboardingRunContext,
    step: &Rc<SpotlightStep>,
    mut target: OnboardingTarget,
    index: usize,
    step_count: usize,
    can_go_back: bool,
    is_last_step: bool,
    cancellation: &Rc<CancellationTokenSource>,
) -> Result<StepEnd, Error> {
    let mut step_context = context.clone();
    while !cancellation.token().is_cancellation_requested() {
        let end = show_step(inner, overlay, &step_context, step, &target, index, step_count, can_go_back, is_last_step).await?;
        if end != StepEnd::MissingTarget {
            return Ok(end);
        }
        overlay.hide();
        step_context = OnboardingRunContext { on_did_show: None, ..context.clone() };
        let replacement = resolve_target(
            &context.target_window,
            &step.target_id,
            context.target_scope.as_deref(),
            &cancellation.token(),
            step.missing_target,
        )
        .await?;
        let Some(replacement) = replacement else {
            return Ok(if cancellation.token().is_cancellation_requested() || should_abort_on_missing_target(step.missing_target) {
                StepEnd::Abort
            } else {
                StepEnd::MissingTarget
            });
        };
        target = replacement;
    }
    Ok(StepEnd::Abort)
}

#[allow(clippy::too_many_arguments)]
async fn show_step(
    inner: &Rc<Inner>,
    overlay: &Rc<SpotlightOverlay>,
    context: &OnboardingRunContext,
    step: &Rc<SpotlightStep>,
    target: &OnboardingTarget,
    index: usize,
    step_count: usize,
    can_go_back: bool,
    is_last_step: bool,
) -> Result<StepEnd, Error> {
    let step_store = DisposableStore::new();
    let cancellation = Rc::new(CancellationTokenSource::new(None));
    step_store.add(to_disposable({
        let cancellation = cancellation.clone();
        move || cancellation.dispose_with(true)
    }));
    let ended = Rc::new(Cell::new(false));
    let action_started = Rc::new(Cell::new(false));
    let (result, resolver) = Promise::<StepEnd>::new();
    let done: Rc<dyn Fn(StepEnd)> = {
        let (ended, step_store) = (ended.clone(), step_store.clone());
        Rc::new(move |end: StepEnd| {
            if ended.get() {
                return;
            }
            ended.set(true);
            step_store.dispose();
            resolver.resolve(end);
        })
    };

    step_store.add(overlay.on_did_click_next().listen({
        let (step, done, ended, action_started, cancellation, overlay) =
            (step.clone(), done.clone(), ended.clone(), action_started.clone(), cancellation.clone(), overlay.clone());
        move |via| {
            let primary_action = match (&step.primary_action, via) {
                (Some(primary_action), SpotlightAdvanceSource::Button) => primary_action.clone(),
                _ => {
                    done(StepEnd::Next(if *via == SpotlightAdvanceSource::Target { NextVia::Target } else { NextVia::Button }));
                    return;
                }
            };
            if ended.get() || action_started.get() {
                return;
            }
            action_started.set(true);
            overlay.set_primary_action_enabled(false);
            let token = cancellation.token();
            let fail = {
                let (done, ended) = (done.clone(), ended.clone());
                move |error: Error| {
                    if !ended.get() {
                        errors::on_unexpected_error(&error);
                        done(StepEnd::Abort);
                    }
                }
            };
            match errors::catch(|| (primary_action.run)(&token)) {
                Ok(MaybePromise::Value(())) => {}
                Ok(MaybePromise::Promise(promise)) => {
                    promise.then_result(move |outcome| {
                        if let Err(error) = outcome {
                            fail(error);
                        }
                        Ok(())
                    });
                }
                Err(error) => fail(error),
            }
        }
    }));
    step_store.add(overlay.on_did_click_previous().listen({
        let done = done.clone();
        move |_| done(StepEnd::Back)
    }));
    step_store.add(overlay.on_did_skip().listen({
        let done = done.clone();
        move |reason| done(StepEnd::Skip(*reason))
    }));
    step_store.add(overlay.on_did_lose_target().listen({
        let done = done.clone();
        move |_| done(StepEnd::MissingTarget)
    }));
    step_store.add(context.on_abort.listen({
        let done = done.clone();
        move |_| done(StepEnd::Abort)
    }));

    let content = SpotlightContent {
        title: step.title.clone(),
        description: step.description.clone(),
        next_button_label: step.primary_action.as_ref().map(|action| action.label.clone()).or_else(|| step.next_button_label.clone()),
        step_index: index,
        step_count,
        can_go_back,
        is_last_step,
        show_end_tour: step.primary_action.is_some(),
    };

    let open_target = match step.open_target {
        Some(SpotlightOpenTarget::IfUnselected) => Some(!has_onboarding_target_selection(&target.element)),
        Some(SpotlightOpenTarget::Bool(value)) => Some(value),
        None => None,
    };
    if step.advance_on_target_selection == Some(true) {
        let on_did_select = target.on_did_select.clone().unwrap_or_else(|| on_did_select_onboarding_target(&target.element));
        step_store.add(on_did_select.listen({
            let done = done.clone();
            move |accepted| {
                let done = done.clone();
                accepted.then_result(move |outcome| {
                    match outcome {
                        Ok(true) => done(StepEnd::Next(NextVia::Condition)),
                        Ok(false) => {}
                        Err(error) => {
                            errors::on_unexpected_error(&error);
                            done(StepEnd::Abort);
                        }
                    }
                    Ok(())
                });
            }
        }));
    }

    overlay.show(
        &target.element,
        &content,
        SpotlightShowOptions {
            placement: step.placement,
            allow_target_interaction: step.allow_target_interaction,
            advance_on_target_click: step.advance_on_target_click,
            on_did_activate_target: target.on_did_activate.clone(),
            focus_target: target.focus_target.clone(),
            hide_next: if step.primary_action.is_some() {
                Some(false)
            } else if step.advance_when.is_some() {
                Some(true)
            } else {
                step.hide_next
            },
            target_overlay_visible: open_target,
            padding: step.padding,
        },
    );
    if let Some(on_did_show) = &context.on_did_show {
        on_did_show();
    }
    if let Some(on_did_show) = &step.on_did_show {
        on_did_show();
    }

    if let Some(advance_when) = step.advance_when.clone() {
        let keys: HashSet<String> = advance_when.keys().into_iter().collect();
        let advance_if_satisfied: Rc<dyn Fn()> = {
            let (done, inner) = (done.clone(), inner.clone());
            Rc::new(move || {
                if inner.context_key_service.context_matches_rules(Some(&advance_when)) {
                    done(StepEnd::Next(NextVia::Condition));
                }
            })
        };
        step_store.add(inner.context_key_service.on_did_change_context().listen({
            let advance_if_satisfied = advance_if_satisfied.clone();
            move |event| {
                if event.affects_some(&keys) {
                    advance_if_satisfied();
                }
            }
        }));
        advance_if_satisfied();
    }

    if open_target == Some(true)
        && !ended.get()
        && let Some(open) = &target.open
    {
        let opened = match errors::catch(|| open()) {
            Ok(Some(promise)) => promise.await,
            Ok(None) => Promise::resolved(()).await,
            Err(error) => Err(error),
        };
        if let Err(error) = opened {
            errors::on_unexpected_error(&error);
        }
    }

    result.await
}
