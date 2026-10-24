//! Porte de `src/vs/workbench/contrib/onboarding/test/browser/sequencePresentation.test.ts` (suíte
//! `OnboardingSequencePresentation`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso (dentro do corpo
//!   assíncrono, que roda em `run_with_real_timers`); `disposables.add` vira `guard.add` / `guard.add_rc`.
//! - **`await`.** É o `.await` da promessa do porte (`Promise` implementa `IntoFuture`).
//! - **Recorte: o primeiro caso do original** (`renders spotlight counters using only spotlight steps`) NÃO está aqui.
//!   Ele monta o `SpotlightPresentation` com `ContextKeyService`, `TestHostService` e `TestLayoutService`, cria
//!   botões no DOM (`$('button')`, `style.position`, `target.click()`) e lê `.spotlight-callout-counter` do DOM.
//!   Nenhum deles foi portado (`spotlightPresentation.ts`, `spotlightOverlay.ts`); o caso entra com eles, sem stub.
//!   Com isso `createSpotlightTarget`, `SequenceTestLayoutService` e os imports de DOM também ficam de fora.
//! - **`mainWindow`.** É uma [`OnboardingWindow`] por caso (o porte não tem DOM).
//! - **`IOnboardingSequencePayload` e `IRunOnboardingStepPayload`** viajam no `payload` (`unknown`) como valor nativo de
//!   um objeto (`ObjectRef::with_host`), o que as apresentações recuperam pelo tipo concreto.
//! - **`kindSeed`.** É um contador por thread (cada caso roda na sua).
//! - **`run: () => runCalls.push('script')` e `run: () => runCount++`.** Devolvem um número, que o original lê como
//!   `result?.shown` (sempre `undefined`); no porte o callback devolve `None`.
//! - **`throw new Error('run failed')`.** É `errors::throw`; o `try/finally` que restaura o tratador de erros
//!   inesperados chama `record_unexpected_errors()`, o tratador que o guarda instala.
//! - **`deepStrictEqual` de objetos literais.** É `assert_eq!` de tuplas com os mesmos campos, na mesma ordem.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, record_unexpected_errors, run_with_real_timers};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::DisposableRef;
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_workbench::contrib::onboarding::onboarding_presentation::{IOnboardingPresentation, OnboardingRunContext};
use je_workbench::contrib::onboarding::onboarding_scenario::{
    OnboardingDismissReason, OnboardingOutcome, OnboardingPresentationRef, OnboardingRunResult, OnboardingScenario, OnboardingTrigger,
};
use je_workbench::contrib::onboarding::onboarding_sequence::{
    IOnboardingSequenceStepPresentation, ONBOARDING_SEQUENCE_PRESENTATION_KIND, OnboardingSequencePayload, OnboardingSequenceStep,
    OnboardingSequenceStepAction, OnboardingSequenceStepContext, OnboardingSequenceStepResult,
    onboarding_sequence_step_presentation_registry,
};
use je_workbench::contrib::onboarding::onboarding_target::OnboardingWindow;
use je_workbench::contrib::onboarding::run_onboarding_step::{
    RUN_ONBOARDING_STEP_KIND, RunOnboardingStepPayload, RunOnboardingStepPresentation, RunOnboardingStepResult,
};
use je_workbench::contrib::onboarding::sequence_presentation::OnboardingSequencePresentation;

/// O que `TestVisualStepPresentation.contexts` guarda de cada passo.
#[derive(Debug, Clone, PartialEq)]
struct SeenContext {
    id: String,
    index: usize,
    count: usize,
    can_go_back: bool,
    is_last: bool,
}

fn seen(id: &str, index: usize, count: usize, can_go_back: bool, is_last: bool) -> SeenContext {
    SeenContext { id: id.to_string(), index, count, can_go_back, is_last }
}

fn next(shown: bool) -> OnboardingSequenceStepResult {
    OnboardingSequenceStepResult { action: OnboardingSequenceStepAction::Next, shown, dismiss_reason: None }
}

fn action(action: OnboardingSequenceStepAction, shown: bool) -> OnboardingSequenceStepResult {
    OnboardingSequenceStepResult { action, shown, dismiss_reason: None }
}

/// `TestVisualStepPresentation`.
struct TestVisualStepPresentation {
    kind: String,
    actions: RefCell<HashMap<String, VecDeque<OnboardingSequenceStepResult>>>,
    contexts: RefCell<Vec<SeenContext>>,
}

impl TestVisualStepPresentation {
    fn new(kind: String, actions: Vec<(&str, Vec<OnboardingSequenceStepResult>)>) -> Rc<Self> {
        let actions = actions.into_iter().map(|(id, results)| (id.to_string(), results.into())).collect();
        Rc::new(Self { kind, actions: RefCell::new(actions), contexts: RefCell::new(Vec::new()) })
    }

    fn ids(&self) -> Vec<String> {
        self.contexts.borrow().iter().map(|item| item.id.clone()).collect()
    }
}

impl IOnboardingSequenceStepPresentation for TestVisualStepPresentation {
    fn kind(&self) -> &str {
        &self.kind
    }

    fn counts_as_visual_step(&self) -> bool {
        true
    }

    fn run_step(&self, step: &OnboardingSequenceStep, context: &OnboardingSequenceStepContext) -> Promise<OnboardingSequenceStepResult> {
        self.contexts.borrow_mut().push(SeenContext {
            id: step.id.clone(),
            index: context.visual_step_index,
            count: context.visual_step_count,
            can_go_back: context.can_go_back,
            is_last: context.is_last_visual_step,
        });
        let queued = self.actions.borrow_mut().get_mut(&step.id).and_then(VecDeque::pop_front);
        Promise::resolved(queued.unwrap_or_else(|| next(true)))
    }
}

thread_local! {
    static KIND_SEED: Cell<u32> = const { Cell::new(0) };
}

/// `` `test-visual-${kindSeed++}` ``.
fn next_visual_kind() -> String {
    KIND_SEED.with(|seed| {
        let value = seed.get();
        seed.set(value + 1);
        format!("test-visual-{value}")
    })
}

fn sequence_payload(steps: Vec<OnboardingSequenceStep>) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(OnboardingSequencePayload { steps })))
}

/// `createScenario(steps)`.
fn create_scenario(steps: Vec<OnboardingSequenceStep>) -> OnboardingScenario {
    OnboardingScenario {
        id: "test.sequence".to_string(),
        seen_key: None,
        when: None,
        trigger: OnboardingTrigger::Auto,
        priority: None,
        presentation: OnboardingPresentationRef { kind: ONBOARDING_SEQUENCE_PRESENTATION_KIND.to_string(), payload: sequence_payload(steps) },
        tryout: None,
        experiment: None,
        developer_mode_variations: None,
        repeatable: None,
    }
}

/// `context(onAbort)`.
fn context(on_abort: Event<()>) -> OnboardingRunContext {
    OnboardingRunContext { target_window: OnboardingWindow::new(), on_did_show: None, on_abort, target_scope: None }
}

fn visual_step(id: &str, kind: &str) -> OnboardingSequenceStep {
    OnboardingSequenceStep { id: id.to_string(), kind: kind.to_string(), payload: Dynamic::Undefined }
}

fn run_step(id: &str, run: impl Fn(&CancellationToken) -> MaybePromise<Option<RunOnboardingStepResult>> + 'static) -> OnboardingSequenceStep {
    OnboardingSequenceStep {
        id: id.to_string(),
        kind: RUN_ONBOARDING_STEP_KIND.to_string(),
        payload: Dynamic::Object(ObjectRef::with_host(Rc::new(RunOnboardingStepPayload { run: Rc::new(run) }))),
    }
}

fn register_visual(guard: &common::LeakGuard, visual: &Rc<TestVisualStepPresentation>) {
    guard.add(onboarding_sequence_step_presentation_registry().register(visual.clone()));
    register_run_step(guard);
}

fn register_run_step(guard: &common::LeakGuard) -> DisposableRef {
    guard.add(onboarding_sequence_step_presentation_registry().register(Rc::new(RunOnboardingStepPresentation)))
}

fn result(shown: bool, dismiss_reason: OnboardingDismissReason, last_step_index: f64, step_count: f64, outcome: OnboardingOutcome) -> OnboardingRunResult {
    OnboardingRunResult { outcome, shown, dismiss_reason, last_step_index, step_count }
}

fn completed(last_step_index: f64, step_count: f64) -> OnboardingRunResult {
    result(true, OnboardingDismissReason::Completed, last_step_index, step_count, OnboardingOutcome::Completed)
}

#[test]
fn counts_only_visual_steps_while_retaining_sequence_indices_in_the_result() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let visual_kind = next_visual_kind();
        let visual = TestVisualStepPresentation::new(visual_kind.clone(), vec![]);
        register_visual(&guard, &visual);
        let run_calls = Rc::new(RefCell::new(Vec::<String>::new()));
        let presentation = guard.add_rc(OnboardingSequencePresentation::new());

        let outcome = presentation
            .run(
                &create_scenario(vec![
                    visual_step("first", &visual_kind),
                    run_step("script", {
                        let run_calls = run_calls.clone();
                        move |_| {
                            run_calls.borrow_mut().push("script".to_string());
                            MaybePromise::Value(None)
                        }
                    }),
                    visual_step("second", &visual_kind),
                ]),
                &context(Event::none()),
            )
            .await
            .unwrap();

        assert_eq!(
            (visual.contexts.borrow().clone(), run_calls.borrow().clone(), outcome),
            (
                vec![seen("first", 0, 2, false, false), seen("second", 1, 2, true, true)],
                vec!["script".to_string()],
                completed(2.0, 3.0),
            )
        );
    });
}

#[test]
fn reports_a_user_visible_run_step_as_shown_when_preceding_visuals_are_skipped() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let visual_kind = next_visual_kind();
        let visual = TestVisualStepPresentation::new(
            visual_kind.clone(),
            vec![("skipped", vec![action(OnboardingSequenceStepAction::SkipStep, false)])],
        );
        register_visual(&guard, &visual);
        let presentation = guard.add_rc(OnboardingSequencePresentation::new());

        let outcome = presentation
            .run(
                &create_scenario(vec![
                    visual_step("skipped", &visual_kind),
                    run_step("script", |_| MaybePromise::Value(Some(RunOnboardingStepResult { shown: Some(true) }))),
                ]),
                &context(Event::none()),
            )
            .await
            .unwrap();

        assert_eq!(outcome, completed(1.0, 2.0));
    });
}

#[test]
fn back_skips_run_steps_and_forward_traversal_runs_them_at_most_once() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let visual_kind = next_visual_kind();
        let visual = TestVisualStepPresentation::new(
            visual_kind.clone(),
            vec![
                ("first", vec![next(true), next(true)]),
                ("second", vec![action(OnboardingSequenceStepAction::Back, true), next(true)]),
            ],
        );
        register_visual(&guard, &visual);
        let run_count = Rc::new(Cell::new(0));
        let presentation = guard.add_rc(OnboardingSequencePresentation::new());

        let outcome = presentation
            .run(
                &create_scenario(vec![
                    visual_step("first", &visual_kind),
                    run_step("script", {
                        let run_count = run_count.clone();
                        move |_| {
                            run_count.set(run_count.get() + 1);
                            MaybePromise::Value(None)
                        }
                    }),
                    visual_step("second", &visual_kind),
                ]),
                &context(Event::none()),
            )
            .await
            .unwrap();

        assert_eq!(
            (visual.ids(), run_count.get(), outcome),
            (vec!["first".to_string(), "second".to_string(), "first".to_string(), "second".to_string()], 1, completed(2.0, 3.0))
        );
    });
}

#[test]
fn reports_run_errors_and_continues_to_the_next_step() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let visual_kind = next_visual_kind();
        let visual = TestVisualStepPresentation::new(visual_kind.clone(), vec![]);
        register_visual(&guard, &visual);
        let reported = Rc::new(RefCell::new(Vec::<String>::new()));
        errors::set_unexpected_error_handler({
            let reported = reported.clone();
            move |error| reported.borrow_mut().push(error.message.clone())
        });
        let presentation = guard.add_rc(OnboardingSequencePresentation::new());

        let outcome = presentation
            .run(
                &create_scenario(vec![
                    run_step("script", |_| -> MaybePromise<Option<RunOnboardingStepResult>> { errors::throw(errors::Error::new("run failed")) }),
                    visual_step("visual", &visual_kind),
                ]),
                &context(Event::none()),
            )
            .await
            .unwrap();

        let observed = (reported.borrow().clone(), visual.ids(), outcome);
        record_unexpected_errors();
        assert_eq!(observed, (vec!["run failed".to_string()], vec!["visual".to_string()], completed(1.0, 2.0)));
    });
}

#[test]
fn cancels_an_awaited_run_step_and_aborts_before_later_steps() {
    run_with_real_timers(async move {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let visual_kind = next_visual_kind();
        let visual = TestVisualStepPresentation::new(visual_kind.clone(), vec![]);
        register_visual(&guard, &visual);
        let abort = guard.add_rc(Emitter::<()>::new());
        let token_cancelled = Rc::new(Cell::new(false));
        let started = Rc::new(DeferredPromise::<()>::new());
        let presentation = guard.add_rc(OnboardingSequencePresentation::new());

        let result_promise = presentation.run(
            &create_scenario(vec![
                visual_step("before", &visual_kind),
                run_step("script", {
                    let started = started.clone();
                    let token_cancelled = token_cancelled.clone();
                    move |token| {
                        let (started, token_cancelled, token) = (started.clone(), token_cancelled.clone(), token.clone());
                        MaybePromise::Promise(Promise::with_executor(move |resolver| {
                            let _ = started.complete(());
                            let slot: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
                            let listener = token.on_cancellation_requested().listen({
                                let slot = slot.clone();
                                move |_| {
                                    let listener = slot.borrow_mut().take();
                                    if let Some(listener) = listener {
                                        listener.dispose();
                                    }
                                    token_cancelled.set(true);
                                    resolver.resolve(None);
                                }
                            });
                            *slot.borrow_mut() = Some(listener);
                        }))
                    }
                }),
                visual_step("after", &visual_kind),
            ]),
            &context(abort.event()),
        );
        started.promise.clone().await.unwrap();
        abort.fire(());
        let outcome = result_promise.await.unwrap();

        assert_eq!(
            (token_cancelled.get(), visual.ids(), outcome),
            (
                true,
                vec!["before".to_string()],
                result(true, OnboardingDismissReason::Aborted, 1.0, 3.0, OnboardingOutcome::Aborted),
            )
        );
    });
}
