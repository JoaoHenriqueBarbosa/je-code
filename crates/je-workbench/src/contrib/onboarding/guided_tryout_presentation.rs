//! Porte de `src/vs/workbench/contrib/onboarding/browser/guidedTryoutPresentation.ts`: o tryout guiado, que abre o
//! recurso pela apresentação de lançamento registrada e depois destaca os controles com uma sequência de passos.
//!
//! Divergências:
//! - `mainWindow` é uma [`OnboardingWindow`] única por thread ([`main_window`]), sem DOM.
//! - `async prepare` é [`async_fn`]; o `throw` de `getPayload` dentro dele vira a promessa rejeitada (`?`), e o
//!   `getAvailability` síncrono lança de verdade ([`errors::throw`]). `localize` é o texto em inglês do original.
//! - Os auxiliares privados que só leem os registros e o payload (`getPayload`, `getLaunchPresentation`,
//!   `createLaunchScenario`, `didLaunch`, `unavailable` e a leitura da disponibilidade) são funções livres, porque as
//!   promessas que o `prepare` devolve não podem emprestar `self`; o único estado, a apresentação de sequência, é um
//!   `Rc` clonado para dentro delas.
//! - `onDidChangeAvailability` é montado a cada chamada de [`IOnboardingTryoutPresentation::on_did_change_availability`]
//!   sobre os mesmos registros globais, o que equivale ao campo do original.
//! - O payload do cenário é o [`GuidedTryoutPayload`] nativo (ver `onboarding_tryout_actions`); o payload do cenário de
//!   sequência é o [`OnboardingSequencePayload`] como valor nativo de um objeto.
//! - `.catch(error => ...)` é `then_result`, que devolve o cancelamento como [`OnboardingTryoutResult::Cancelled`] e
//!   repassa qualquer outro erro.
//! - `extends Disposable` é o `Disposable` guardado em `base`.

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error, is_cancellation_error};
use je_base::event::{self, Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::promise::Promise;

use super::onboarding_presentation::{IOnboardingPresentation, OnboardingRunContext};
use super::onboarding_presentation_queue::run_with_onboarding_presentation;
use super::onboarding_scenario::{OnboardingOutcome, OnboardingPresentationRef, OnboardingScenario};
use super::onboarding_sequence::{
    ONBOARDING_SEQUENCE_PRESENTATION_KIND, OnboardingSequencePayload, onboarding_sequence_step_presentation_registry,
};
use super::onboarding_target::OnboardingWindow;
use super::onboarding_tryout::{
    IOnboardingTryoutPresentation, OnboardingTryoutAvailability, OnboardingTryoutPreparation, OnboardingTryoutResult,
    OnboardingTryoutRunContext, OnboardingTryoutUnavailable, onboarding_tryout_presentation_registry,
};
use super::onboarding_tryout_actions::{GuidedTryoutPayload, guided_tryout_payload_of};
use super::sequence_presentation::OnboardingSequencePresentation;

pub const GUIDED_TRYOUT_PRESENTATION_KIND: &str = "guidedTryout";

thread_local! {
    static MAIN_WINDOW: OnboardingWindow = OnboardingWindow::new();
}

/// `mainWindow`.
pub fn main_window() -> OnboardingWindow {
    MAIN_WINDOW.with(Clone::clone)
}

/// `GuidedTryoutPresentation`.
pub struct GuidedTryoutPresentation {
    base: Rc<Disposable>,
    sequence_presentation: Rc<OnboardingSequencePresentation>,
}

impl GuidedTryoutPresentation {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let sequence_presentation = OnboardingSequencePresentation::new();
        base.register(sequence_presentation.clone());
        Rc::new(Self { base, sequence_presentation })
    }
}

impl IDisposable for GuidedTryoutPresentation {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `getPayload`.
fn get_payload(scenario: &OnboardingScenario) -> Result<Rc<GuidedTryoutPayload>, Error> {
    match guided_tryout_payload_of(&scenario.presentation.payload) {
        Some(payload) if payload.launch.kind != GUIDED_TRYOUT_PRESENTATION_KIND => Ok(payload),
        _ => Err(Error::new(format!("The guided feature example '{}' has an invalid presentation.", scenario.id))),
    }
}

/// `getLaunchPresentation`.
fn get_launch_presentation(payload: &GuidedTryoutPayload) -> Option<Rc<dyn IOnboardingTryoutPresentation>> {
    onboarding_tryout_presentation_registry().get(&payload.launch.kind)
}

/// `createLaunchScenario`.
fn create_launch_scenario(scenario: &OnboardingScenario, payload: &GuidedTryoutPayload) -> OnboardingScenario {
    OnboardingScenario { presentation: payload.launch.clone(), ..scenario.clone() }
}

/// `payload.steps.some(step => !onboardingSequenceStepPresentationRegistry.get(step.kind))`.
fn has_missing_step_presentation(payload: &GuidedTryoutPayload) -> bool {
    let registry = onboarding_sequence_step_presentation_registry();
    payload.steps.iter().any(|step| registry.get(&step.kind).is_none())
}

/// `unavailable`.
fn unavailable(payload: &GuidedTryoutPayload) -> OnboardingTryoutUnavailable {
    OnboardingTryoutUnavailable {
        message: payload
            .unavailable_message
            .clone()
            .unwrap_or_else(|| "The example opened, but its highlighted control is not available.".to_string()),
        action: None,
    }
}

/// `getAvailability`, com o `throw` de `getPayload` como erro.
fn availability_of(scenario: &OnboardingScenario) -> Result<OnboardingTryoutAvailability, Error> {
    let payload = get_payload(scenario)?;
    let Some(launch) = get_launch_presentation(&payload) else {
        return Ok(OnboardingTryoutAvailability::Unavailable(unavailable(&payload)));
    };
    if has_missing_step_presentation(&payload) {
        return Ok(OnboardingTryoutAvailability::Unavailable(unavailable(&payload)));
    }
    Ok(launch.get_availability(&create_launch_scenario(scenario, &payload)))
}

/// O corpo da tarefa enfileirada de `run`, depois do lançamento já preparado.
async fn run_guided(
    scenario: OnboardingScenario,
    payload: Rc<GuidedTryoutPayload>,
    launch: Rc<dyn IOnboardingTryoutPresentation>,
    launch_run: Rc<dyn Fn() -> Promise<OnboardingTryoutResult>>,
    context: OnboardingTryoutRunContext,
    sequence_presentation: Rc<OnboardingSequencePresentation>,
) -> Result<OnboardingTryoutResult, Error> {
    match get_launch_presentation(&payload) {
        Some(current) if Rc::ptr_eq(&current, &launch) => {}
        _ => return Ok(OnboardingTryoutResult::Unavailable(unavailable(&payload))),
    }
    match availability_of(&scenario)? {
        OnboardingTryoutAvailability::Ready => {}
        OnboardingTryoutAvailability::Hidden => return Ok(OnboardingTryoutResult::Unavailable(unavailable(&payload))),
        OnboardingTryoutAvailability::Unavailable(availability) => return Ok(OnboardingTryoutResult::Unavailable(availability)),
    }
    let launch_result = launch_run().await?;
    let OnboardingTryoutResult::Launched { kind, target_scope } = &launch_result else {
        return Ok(if context.token.is_cancellation_requested() { OnboardingTryoutResult::Cancelled } else { launch_result });
    };
    if context.token.is_cancellation_requested() {
        return Ok(OnboardingTryoutResult::Cancelled);
    }
    if let Some(on_did_launch) = &context.on_did_launch {
        on_did_launch(*kind);
    }

    let guidance_store = DisposableStore::new();
    let abort = Emitter::<()>::new();
    guidance_store.add_rc(abort.clone());
    guidance_store.add(context.token.on_cancellation_requested().listen({
        let abort = abort.clone();
        move |_| abort.fire(())
    }));
    let guidance_scenario = OnboardingScenario {
        presentation: OnboardingPresentationRef {
            kind: ONBOARDING_SEQUENCE_PRESENTATION_KIND.to_string(),
            payload: Dynamic::Object(ObjectRef::with_host(Rc::new(OnboardingSequencePayload { steps: payload.steps.clone() }))),
        },
        ..scenario.clone()
    };
    let guidance_context = OnboardingRunContext { target_window: main_window(), on_did_show: None, on_abort: abort.event(), target_scope: target_scope.clone() };
    let guidance_result = {
        let guidance_store = guidance_store.clone();
        sequence_presentation.run(&guidance_scenario, &guidance_context).finally(move || guidance_store.dispose()).await?
    };

    if let Some(on_did_finish_guidance) = &context.on_did_finish_guidance {
        on_did_finish_guidance(&guidance_result);
    }
    if context.token.is_cancellation_requested() {
        return Ok(OnboardingTryoutResult::Cancelled);
    }
    if !guidance_result.shown || guidance_result.outcome == OnboardingOutcome::Aborted {
        return Ok(OnboardingTryoutResult::Unavailable(unavailable(&payload)));
    }
    Ok(launch_result)
}

impl IOnboardingTryoutPresentation for GuidedTryoutPresentation {
    fn kind(&self) -> &str {
        GUIDED_TRYOUT_PRESENTATION_KIND
    }

    fn on_did_change_availability(&self) -> Option<Event<()>> {
        Some(event::any(vec![
            onboarding_tryout_presentation_registry().on_did_change(),
            onboarding_sequence_step_presentation_registry().on_did_change(),
        ]))
    }

    fn get_availability(&self, scenario: &OnboardingScenario) -> OnboardingTryoutAvailability {
        availability_of(scenario).unwrap_or_else(|error| errors::throw(error))
    }

    fn prepare(&self, scenario: &OnboardingScenario, context: &OnboardingTryoutRunContext) -> Promise<OnboardingTryoutPreparation> {
        let scenario = scenario.clone();
        let context = context.clone();
        let sequence_presentation = self.sequence_presentation.clone();
        async_fn(async move {
            let payload = get_payload(&scenario)?;
            let launch = match get_launch_presentation(&payload) {
                Some(launch) if !has_missing_step_presentation(&payload) => launch,
                _ => return Ok(OnboardingTryoutPreparation::Unavailable(unavailable(&payload))),
            };
            let launch_scenario = create_launch_scenario(&scenario, &payload);
            let preparation = launch.prepare(&launch_scenario, &context).await?;
            let OnboardingTryoutPreparation::Ready { run: launch_run } = &preparation else {
                return Ok(preparation);
            };
            let launch_run = launch_run.clone();
            Ok(OnboardingTryoutPreparation::Ready {
                run: Rc::new(move || {
                    let (scenario, payload, launch, launch_run) = (scenario.clone(), payload.clone(), launch.clone(), launch_run.clone());
                    let (context, sequence_presentation) = (context.clone(), sequence_presentation.clone());
                    run_with_onboarding_presentation::<OnboardingTryoutResult>(&main_window(), &context.token.clone(), move || {
                        async_fn(run_guided(scenario, payload, launch, launch_run, context, sequence_presentation))
                    })
                    .then_result(|result| match result {
                        Err(error) if is_cancellation_error(&error) => Ok(OnboardingTryoutResult::Cancelled),
                        other => other,
                    })
                }),
            })
        })
    }
}
