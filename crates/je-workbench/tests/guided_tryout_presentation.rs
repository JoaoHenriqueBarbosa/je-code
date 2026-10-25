//! Porte de `src/vs/workbench/contrib/onboarding/test/browser/guidedTryoutPresentation.test.ts` (suíte
//! `GuidedTryoutPresentation`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso (dentro do corpo
//!   assíncrono, que roda em `run_with_real_timers`); `disposables.add` vira `guard.add` / `guard.add_rc`.
//! - **`await`.** É o `.await` da promessa do porte.
//! - **`IGuidedTryoutPayload`.** Viaja no `payload` (`unknown`) como valor nativo de um objeto (`ObjectRef::with_host`).
//!   O `payload: { target: 'view' }` do lançamento é opaco para a apresentação guiada (o dublê de lançamento só lê o
//!   `kind`), então vai como `undefined`; `{ targetId: 'test.control' }` do passo também.
//! - **`events: string[]` compartilhado.** É um `Rc<RefCell<Vec<String>>>` que o dublê de lançamento e o de passo
//!   empurram, como o `launch.events` do original.
//! - **`launch.prepare = async () => ...`.** O caso de cancelamento troca o `prepare` do dublê; aqui é o campo
//!   `cancel_after_launch`, que faz o `prepare` devolver o `run` que cancela, sem registrar `prepare:`.
//! - **`for (...) test(...)`.** Vira um teste por combinação, na ordem do original.
//! - **`deepStrictEqual` de objetos literais.** É `assert_eq!` de tuplas com os mesmos campos, na mesma ordem.
//! - **`assert.throws(fn, /invalid presentation/)`.** É `errors::catch` e a conferência do trecho da mensagem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors;
use je_base::lifecycle::DisposableStore;
use je_base::promise::Promise;
use je_workbench::contrib::onboarding::guided_tryout_presentation::{GUIDED_TRYOUT_PRESENTATION_KIND, GuidedTryoutPresentation};
use je_workbench::contrib::onboarding::onboarding_scenario::{
    OnboardingDismissReason, OnboardingOutcome, OnboardingPresentationRef, OnboardingRunResult, OnboardingScenario, OnboardingTrigger,
};
use je_workbench::contrib::onboarding::onboarding_sequence::{
    IOnboardingSequenceStepPresentation, OnboardingSequenceStep, OnboardingSequenceStepAction, OnboardingSequenceStepContext,
    OnboardingSequenceStepResult, onboarding_sequence_step_presentation_registry,
};
use je_workbench::contrib::onboarding::onboarding_tryout::{
    IOnboardingTryoutPresentation, OnboardingTryoutAvailability, OnboardingTryoutLaunchKind, OnboardingTryoutMetadata,
    OnboardingTryoutPreparation, OnboardingTryoutResult, OnboardingTryoutRunContext, OnboardingTryoutUnavailable,
    onboarding_tryout_presentation_registry,
};
use je_workbench::contrib::onboarding::onboarding_tryout_actions::GuidedTryoutPayload;

type Events = Rc<RefCell<Vec<String>>>;

/// `TestLaunchPresentation`.
struct TestLaunchPresentation {
    events: Events,
    availability: RefCell<OnboardingTryoutAvailability>,
    target_scope: RefCell<Option<String>>,
    cancel_after_launch: RefCell<Option<Rc<CancellationTokenSource>>>,
}

impl TestLaunchPresentation {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            events: Rc::new(RefCell::new(Vec::new())),
            availability: RefCell::new(OnboardingTryoutAvailability::Ready),
            target_scope: RefCell::new(None),
            cancel_after_launch: RefCell::new(None),
        })
    }

    fn push(&self, event: impl Into<String>) {
        self.events.borrow_mut().push(event.into());
    }

    fn events(&self) -> Vec<String> {
        self.events.borrow().clone()
    }
}

impl IOnboardingTryoutPresentation for TestLaunchPresentation {
    fn kind(&self) -> &str {
        "test.launch"
    }

    fn get_availability(&self, _scenario: &OnboardingScenario) -> OnboardingTryoutAvailability {
        self.push("availability");
        self.availability.borrow().clone()
    }

    fn prepare(&self, scenario: &OnboardingScenario, _context: &OnboardingTryoutRunContext) -> Promise<OnboardingTryoutPreparation> {
        let cancellation = self.cancel_after_launch.borrow().clone();
        let events = self.events.clone();
        if let Some(cancellation) = cancellation {
            return Promise::resolved(OnboardingTryoutPreparation::Ready {
                run: Rc::new(move || {
                    events.borrow_mut().push("launch".to_string());
                    cancellation.cancel();
                    Promise::resolved(OnboardingTryoutResult::Launched { kind: OnboardingTryoutLaunchKind::Opened, target_scope: None })
                }),
            });
        }
        self.push(format!("prepare:{}", scenario.presentation.kind));
        let target_scope = self.target_scope.borrow().clone();
        Promise::resolved(OnboardingTryoutPreparation::Ready {
            run: Rc::new(move || {
                events.borrow_mut().push("launch".to_string());
                Promise::resolved(OnboardingTryoutResult::Launched { kind: OnboardingTryoutLaunchKind::Opened, target_scope: target_scope.clone() })
            }),
        })
    }
}

/// `TestGuidanceStep`.
struct TestGuidanceStep {
    events: Events,
    action: RefCell<OnboardingSequenceStepResult>,
}

impl TestGuidanceStep {
    fn new(events: Events) -> Rc<Self> {
        Rc::new(Self {
            events,
            action: RefCell::new(OnboardingSequenceStepResult { action: OnboardingSequenceStepAction::Next, shown: true, dismiss_reason: None }),
        })
    }
}

impl IOnboardingSequenceStepPresentation for TestGuidanceStep {
    fn kind(&self) -> &str {
        "test.guidance"
    }

    fn counts_as_visual_step(&self) -> bool {
        true
    }

    fn run_step(&self, step: &OnboardingSequenceStep, context: &OnboardingSequenceStepContext) -> Promise<OnboardingSequenceStepResult> {
        self.events.borrow_mut().push(format!(
            "guide:{}:{}/{}:{}",
            step.id,
            context.visual_step_index + 1,
            context.visual_step_count,
            context.base.target_scope.as_deref().unwrap_or("global"),
        ));
        Promise::resolved(*self.action.borrow())
    }
}

/// `createScenario(stepKind, launchKind)`.
fn create_scenario(step_kind: &str, launch_kind: &str) -> OnboardingScenario {
    let payload = GuidedTryoutPayload {
        launch: OnboardingPresentationRef { kind: launch_kind.to_string(), payload: Dynamic::Undefined },
        steps: vec![OnboardingSequenceStep { id: "control".to_string(), kind: step_kind.to_string(), payload: Dynamic::Undefined }],
        unavailable_message: Some("The control could not be highlighted.".to_string()),
    };
    OnboardingScenario {
        id: "test.guided".to_string(),
        seen_key: None,
        when: None,
        trigger: OnboardingTrigger::Command { command_id: "workbench.action.onboarding.tryFeature".to_string() },
        priority: None,
        presentation: OnboardingPresentationRef {
            kind: GUIDED_TRYOUT_PRESENTATION_KIND.to_string(),
            payload: Dynamic::Object(ObjectRef::with_host(Rc::new(payload))),
        },
        tryout: Some(Rc::new(OnboardingTryoutMetadata {
            title: "Guided example".to_string(),
            description: "Open and highlight a control.".to_string(),
            ..OnboardingTryoutMetadata::default()
        })),
        experiment: None,
        developer_mode_variations: None,
        repeatable: None,
    }
}

fn default_scenario() -> OnboardingScenario {
    create_scenario("test.guidance", "test.launch")
}

/// `createContext(token)`.
fn create_context(guard: &LeakGuard, token: CancellationToken) -> OnboardingTryoutRunContext {
    OnboardingTryoutRunContext {
        id: "test.guided".to_string(),
        token,
        store: guard.add_rc(DisposableStore::new()),
        on_did_launch: None,
        on_did_finish_guidance: None,
    }
}

/// `registerLaunchAndGuidance()`.
fn register_launch_and_guidance(guard: &LeakGuard) -> (Rc<TestLaunchPresentation>, Rc<TestGuidanceStep>) {
    let launch = TestLaunchPresentation::new();
    let guidance = TestGuidanceStep::new(launch.events.clone());
    guard.add(onboarding_tryout_presentation_registry().register(launch.clone()));
    guard.add(onboarding_sequence_step_presentation_registry().register(guidance.clone()));
    (launch, guidance)
}

/// A promessa de `run` de uma preparação pronta.
fn ready_run(preparation: OnboardingTryoutPreparation) -> Rc<dyn Fn() -> Promise<OnboardingTryoutResult>> {
    match preparation {
        OnboardingTryoutPreparation::Ready { run } => run,
        _ => panic!("a preparação deveria estar pronta"),
    }
}

fn is_ready(preparation: &OnboardingTryoutPreparation) -> bool {
    matches!(preparation, OnboardingTryoutPreparation::Ready { .. })
}

fn launch_kind_text(kind: OnboardingTryoutLaunchKind) -> &'static str {
    match kind {
        OnboardingTryoutLaunchKind::Opened => "opened",
        OnboardingTryoutLaunchKind::Executed => "executed",
        OnboardingTryoutLaunchKind::Prepared => "prepared",
    }
}

fn unavailable(message: &str) -> OnboardingTryoutUnavailable {
    OnboardingTryoutUnavailable { message: message.to_string(), action: None }
}

fn opened(target_scope: Option<&str>) -> OnboardingTryoutResult {
    OnboardingTryoutResult::Launched { kind: OnboardingTryoutLaunchKind::Opened, target_scope: target_scope.map(str::to_string) }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

#[test]
fn launches_before_showing_guidance_and_returns_the_launch_result() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (launch, _guidance) = register_launch_and_guidance(&guard);
        *launch.target_scope.borrow_mut() = Some("prepared-instance".to_string());
        let presentation = guard.add_rc(GuidedTryoutPresentation::new());
        let scenario = default_scenario();

        let availability = presentation.get_availability(&scenario);
        let preparation = presentation.prepare(&scenario, &create_context(&guard, CancellationToken::None)).await.unwrap();
        assert!(is_ready(&preparation));
        let events_before_run = launch.events();
        let result = ready_run(preparation)().await.unwrap();

        assert_eq!(
            (availability, events_before_run, result, launch.events()),
            (
                OnboardingTryoutAvailability::Ready,
                strings(&["availability", "prepare:test.launch"]),
                opened(Some("prepared-instance")),
                strings(&["availability", "prepare:test.launch", "availability", "launch", "guide:control:1/1:prepared-instance"]),
            )
        );
    });
}

#[test]
fn does_not_prepare_when_a_guidance_kind_is_unavailable() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let launch = TestLaunchPresentation::new();
        guard.add(onboarding_tryout_presentation_registry().register(launch.clone()));
        let presentation = guard.add_rc(GuidedTryoutPresentation::new());
        let scenario = create_scenario("missing.guidance", "test.launch");

        let availability = presentation.get_availability(&scenario);
        let preparation = presentation.prepare(&scenario, &create_context(&guard, CancellationToken::None)).await.unwrap();

        let message = "The control could not be highlighted.";
        assert_eq!(availability, OnboardingTryoutAvailability::Unavailable(unavailable(message)));
        match preparation {
            OnboardingTryoutPreparation::Unavailable(actual) => assert_eq!(actual, unavailable(message)),
            _ => panic!("a preparação deveria estar indisponível"),
        }
        assert_eq!(launch.events(), Vec::<String>::new());
    });
}

/// O caso de `reports the launch and ${outcome} guidance without changing the launch result`.
fn reports_the_launch_and_guidance_without_changing_the_launch_result(
    action: OnboardingSequenceStepAction,
    outcome: OnboardingOutcome,
    dismiss_reason: OnboardingDismissReason,
) {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (launch, guidance) = register_launch_and_guidance(&guard);
        *launch.target_scope.borrow_mut() = Some("prepared-instance".to_string());
        *guidance.action.borrow_mut() = OnboardingSequenceStepResult { action, shown: true, dismiss_reason: Some(dismiss_reason) };
        let outcomes = Rc::new(RefCell::new(Vec::<OnboardingRunResult>::new()));
        let presentation = guard.add_rc(GuidedTryoutPresentation::new());
        let context = OnboardingTryoutRunContext {
            on_did_launch: Some(Rc::new({
                let events = launch.events.clone();
                move |kind| events.borrow_mut().push(format!("reported:{}", launch_kind_text(kind)))
            })),
            on_did_finish_guidance: Some(Rc::new({
                let outcomes = outcomes.clone();
                move |result| outcomes.borrow_mut().push(*result)
            })),
            ..create_context(&guard, CancellationToken::None)
        };
        let preparation = presentation.prepare(&default_scenario(), &context).await.unwrap();
        assert!(is_ready(&preparation));
        let result = ready_run(preparation)().await.unwrap();
        let events: Vec<String> = launch
            .events()
            .into_iter()
            .filter(|event| event == "launch" || event.starts_with("reported:") || event.starts_with("guide:"))
            .map(|event| if event.starts_with("guide:") { "guide".to_string() } else { event })
            .collect();
        let outcomes: Vec<_> = outcomes.borrow().iter().map(|result| (result.outcome, result.shown, result.dismiss_reason)).collect();
        assert_eq!(
            (result, events, outcomes),
            (opened(Some("prepared-instance")), strings(&["launch", "reported:opened", "guide"]), vec![(outcome, true, dismiss_reason)])
        );
    });
}

#[test]
fn reports_the_launch_and_completed_guidance_without_changing_the_launch_result() {
    reports_the_launch_and_guidance_without_changing_the_launch_result(
        OnboardingSequenceStepAction::Next,
        OnboardingOutcome::Completed,
        OnboardingDismissReason::Completed,
    );
}

#[test]
fn reports_the_launch_and_skipped_guidance_without_changing_the_launch_result() {
    reports_the_launch_and_guidance_without_changing_the_launch_result(
        OnboardingSequenceStepAction::SkipSequence,
        OnboardingOutcome::Skipped,
        OnboardingDismissReason::EscapeKey,
    );
}

#[test]
fn reports_missing_guidance_after_the_target_ui_has_opened() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let (launch, guidance) = register_launch_and_guidance(&guard);
        *guidance.action.borrow_mut() = OnboardingSequenceStepResult { action: OnboardingSequenceStepAction::Abort, shown: false, dismiss_reason: None };
        let presentation = guard.add_rc(GuidedTryoutPresentation::new());
        let preparation = presentation.prepare(&default_scenario(), &create_context(&guard, CancellationToken::None)).await.unwrap();
        assert!(is_ready(&preparation));

        let result = ready_run(preparation)().await.unwrap();
        assert_eq!(
            (result, launch.events()),
            (
                OnboardingTryoutResult::Unavailable(unavailable("The control could not be highlighted.")),
                strings(&["prepare:test.launch", "availability", "launch", "guide:control:1/1:global"]),
            )
        );
    });
}

#[test]
fn cancellation_after_launch_prevents_guidance() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let cancellation = guard.add_rc(Rc::new(CancellationTokenSource::new(None)));
        let launch = TestLaunchPresentation::new();
        *launch.cancel_after_launch.borrow_mut() = Some(cancellation.clone());
        let guidance = TestGuidanceStep::new(launch.events.clone());
        guard.add(onboarding_tryout_presentation_registry().register(launch.clone()));
        guard.add(onboarding_sequence_step_presentation_registry().register(guidance));
        let presentation = guard.add_rc(GuidedTryoutPresentation::new());
        let preparation = presentation.prepare(&default_scenario(), &create_context(&guard, cancellation.token())).await.unwrap();
        assert!(is_ready(&preparation));

        let result = ready_run(preparation)().await.unwrap();
        assert_eq!((result, launch.events()), (OnboardingTryoutResult::Cancelled, strings(&["availability", "launch"])));
    });
}

#[test]
fn rejects_recursive_guided_launches() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let presentation = guard.add_rc(GuidedTryoutPresentation::new());
    let scenario = create_scenario("test.guidance", GUIDED_TRYOUT_PRESENTATION_KIND);
    let error = errors::catch(|| presentation.get_availability(&scenario)).expect_err("o lançamento recursivo lança");
    assert!(error.message.contains("invalid presentation"), "{}", error.message);
}

#[test]
fn forwards_launch_unavailability_without_preparing() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let (launch, _guidance) = register_launch_and_guidance(&guard);
    *launch.availability.borrow_mut() = OnboardingTryoutAvailability::Unavailable(unavailable("Open a workspace first."));
    let presentation = guard.add_rc(GuidedTryoutPresentation::new());
    let scenario = default_scenario();

    assert_eq!(presentation.get_availability(&scenario), OnboardingTryoutAvailability::Unavailable(unavailable("Open a workspace first.")));
}
