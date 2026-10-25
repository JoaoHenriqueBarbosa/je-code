//! Porte de `src/vs/workbench/contrib/onboarding/test/browser/spotlightPresentation.test.ts` (suíte
//! `SpotlightPresentation`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso, dentro do corpo
//!   assíncrono (que roda em `run_with_real_timers` ou, onde o original usa `runWithFakedTimers`, em
//!   `run_with_faked_timers`); `disposables.add` vira `guard.add`/`guard.add_rc` ou o `store` do [`Env`].
//! - **DOM.** Não há DOM no porte: o contêiner é um [`SpotlightContainer`], `$('button')` com `position: fixed` e
//!   `100px` por `30px` é [`Env::element`] (`set_position` e `set_size`), `mainWindow` é a [`OnboardingWindow`] do
//!   caso, e as consultas `container.querySelectorAll('.monaco-button')`, `.spotlight-overlay`, `.spotlight-hole`,
//!   `.spotlight-callout-title` e `.spotlight-callout-counter` são `container.buttons()`, `overlays()`,
//!   `hole_top_style()`, `title()` e `counter()`. `button.style.display !== 'none'` é `!is_hidden()` e
//!   `textContent` é `label()`. `target.click()`, `dispatchEvent(new KeyboardEvent(...))` e
//!   `addDisposableListener(el, EventType.X, ...)` são `click`, `key_down`, `key_up` e `add_event_listener`.
//! - **O balão é nativo**: o `Tab` disparado no botão Próximo sobe até o balão por [`SpotlightButton::key_down`].
//! - **`SpotlightTestLayoutService` e `TestHostService`.** São os dublês nativos dos dois traços que a apresentação lê
//!   (o do layout devolve o contêiner do caso; o de host não faz nada).
//! - **`await`.** É o `.await` da promessa do porte; `await Promise.resolve()` é `Promise::resolved(()).await`.
//! - **`deepStrictEqual` de objetos literais.** É `assert_eq!` de tuplas ou de structs com os mesmos campos, na mesma ordem.
//! - **`hasKey(result, { action: true })`.** O resultado do `run` e o do `runStep` são as duas variantes de
//!   [`RunResult`], que expõe `shown`, `completed` e o rótulo `label` que o original lê dos dois.
//! - **`for (...) test(...)`.** Vira um teste por combinação, na ordem do original.
//! - **`disposableTimeout(fn, ms)`** é `disposable_timeout(fn, ms, None)`; `scheduleAtNextAnimationFrame(mainWindow, fn)`
//!   é `window.schedule_at_next_animation_frame(fn)`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, run_with_real_timers};
use je_base::async_::disposable_timeout;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, to_disposable};
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_base::runtime;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::{ContextKeyExpr, IContextKeyService};
use je_platform::contextkey::service::ContextKeyService;
use je_workbench::contrib::onboarding::onboarding_presentation::{IOnboardingPresentation, OnboardingRunContext};
use je_workbench::contrib::onboarding::onboarding_scenario::{
    OnboardingDismissReason, OnboardingOutcome, OnboardingPresentationRef, OnboardingRunResult, OnboardingScenario, OnboardingTrigger,
};
use je_workbench::contrib::onboarding::onboarding_sequence::{
    IOnboardingSequenceStepPresentation, OnboardingSequenceStep, OnboardingSequenceStepAction, OnboardingSequenceStepContext,
    OnboardingSequenceStepResult,
};
use je_workbench::contrib::onboarding::onboarding_target::{
    ONBOARDING_TARGET_ATTR, OnboardingElement, OnboardingEventKind, OnboardingFocusTarget, OnboardingKey, OnboardingTarget,
    OnboardingTargetOpen, OnboardingTargetOptions, OnboardingTargetScope, OnboardingWindow, mark_onboarding_target,
    register_onboarding_target_provider,
};
use je_workbench::contrib::onboarding::spotlight_overlay::SpotlightContainer;
use je_workbench::contrib::onboarding::spotlight_presentation::{
    ISpotlightHostService, ISpotlightLayoutService, SpotlightPresentation, spotlight_payload_value, spotlight_step_value,
};
use je_workbench::contrib::onboarding::spotlight_types::{
    SPOTLIGHT_PRESENTATION_KIND, SpotlightMissingTargetBehavior, SpotlightOpenTarget, SpotlightPayload, SpotlightPrimaryAction,
    SpotlightStep, SpotlightTargetClickBehavior, SpotlightTimeoutAction,
};

/// `SpotlightTestLayoutService`.
struct SpotlightTestLayoutService {
    container: Rc<SpotlightContainer>,
}

impl ISpotlightLayoutService for SpotlightTestLayoutService {
    fn get_container(&self, _window: &OnboardingWindow) -> Rc<SpotlightContainer> {
        self.container.clone()
    }

    fn on_did_layout_container(&self) -> Event<()> {
        Event::none()
    }
}

/// `TestHostService`.
struct TestHostService;

impl ISpotlightHostService for TestHostService {
    fn set_window_dimmed(&self, _window: &OnboardingWindow, _dimmed: bool) {}
}

/// O resultado do `run` (cenário) ou do `runStep` (passo de sequência).
#[derive(Debug, Clone, PartialEq)]
enum RunResult {
    Scenario(OnboardingRunResult),
    Step(OnboardingSequenceStepResult),
}

impl RunResult {
    fn shown(&self) -> bool {
        match self {
            Self::Scenario(result) => result.shown,
            Self::Step(result) => result.shown,
        }
    }

    /// `hasKey(result, { action: true }) ? result.action === 'next' : result.outcome === OnboardingOutcome.Completed`.
    fn completed(&self) -> bool {
        match self {
            Self::Scenario(result) => result.outcome == OnboardingOutcome::Completed,
            Self::Step(result) => result.action == OnboardingSequenceStepAction::Next,
        }
    }

    /// `hasKey(result, { action: true }) ? result.action : result.outcome`.
    fn label(&self) -> &'static str {
        match self {
            Self::Scenario(result) => result.outcome.as_str(),
            Self::Step(result) => match result.action {
                OnboardingSequenceStepAction::Next => "next",
                OnboardingSequenceStepAction::Back => "back",
                OnboardingSequenceStepAction::SkipStep => "skipStep",
                OnboardingSequenceStepAction::SkipSequence => "skipSequence",
                OnboardingSequenceStepAction::Abort => "abort",
            },
        }
    }
}

fn run_result(
    outcome: OnboardingOutcome,
    shown: bool,
    dismiss_reason: OnboardingDismissReason,
    last_step_index: f64,
    step_count: f64,
) -> OnboardingRunResult {
    OnboardingRunResult { outcome, shown, dismiss_reason, last_step_index, step_count }
}

fn step_result(action: OnboardingSequenceStepAction, shown: bool, dismiss_reason: Option<OnboardingDismissReason>) -> OnboardingSequenceStepResult {
    OnboardingSequenceStepResult { action, shown, dismiss_reason }
}

/// O passo com os quatro campos que todo caso preenche.
fn step(id: &str, target_id: &str, title: &str, description: &str) -> SpotlightStep {
    SpotlightStep { id: id.into(), target_id: target_id.into(), title: title.into(), description: description.into(), ..Default::default() }
}

/// `open: () => { ... }`.
fn open_fn(handler: impl Fn() + 'static) -> Option<OnboardingTargetOpen> {
    Some(Rc::new(move || {
        handler();
        None
    }))
}

fn click_behavior(value: bool) -> Option<SpotlightTargetClickBehavior> {
    Some(SpotlightTargetClickBehavior::Bool(value))
}

#[derive(Clone)]
struct Env {
    store: Rc<DisposableStore>,
    window: OnboardingWindow,
    container: Rc<SpotlightContainer>,
    context_keys: Rc<ContextKeyService>,
    presentation: Rc<SpotlightPresentation>,
}

impl Env {
    /// `createContainer()` e a montagem da apresentação que todo caso repete.
    fn new(guard: &LeakGuard) -> Self {
        let container = SpotlightContainer::new();
        let context_keys = guard.add_rc(ContextKeyService::new(TestConfigurationService::new(None)));
        let presentation = guard.add_rc(SpotlightPresentation::new(
            Rc::new(SpotlightTestLayoutService { container: container.clone() }),
            Rc::new(TestHostService),
            context_keys.clone(),
        ));
        Self { store: guard.store(), window: OnboardingWindow::new(), container, context_keys, presentation }
    }

    /// Um elemento conectado, sem caixa nem marca (o `$('input')` do original).
    fn plain_element(&self) -> OnboardingElement {
        let element = OnboardingElement::new(&self.window);
        element.append_to_body();
        let attached = element.clone();
        self.store.add(to_disposable(move || attached.remove()));
        element
    }

    /// `$('button')` com `position: fixed; left: 100px; top: 100px; width: 100px; height: 30px`.
    fn element(&self) -> OnboardingElement {
        let element = self.plain_element();
        element.set_size(100.0, 30.0);
        element.set_position(100.0, 100.0);
        element
    }

    fn mark(&self, element: &OnboardingElement, id: &str, options: OnboardingTargetOptions) {
        self.store.add(mark_onboarding_target(element, id, options));
    }

    /// `createTarget(container, id, options)`.
    fn target(&self, id: &str, options: OnboardingTargetOptions) -> OnboardingElement {
        let element = self.element();
        self.mark(&element, id, options);
        element
    }

    /// Um alvo cujo `open` clica nele mesmo, depois de `before`.
    fn advancing_target(&self, id: &str, before: impl Fn() + 'static) -> OnboardingElement {
        let element = self.element();
        let clicked = element.clone();
        self.mark(&element, id, OnboardingTargetOptions { open: open_fn(move || {
            before();
            clicked.click();
        }), ..Default::default() });
        element
    }

    /// `createScenario(id, ...steps)`.
    fn scenario(&self, id: &str, steps: Vec<SpotlightStep>) -> OnboardingScenario {
        self.scenario_with_payload(id, SpotlightPayload { steps, resolve_steps: None })
    }

    fn scenario_with_payload(&self, id: &str, payload: SpotlightPayload) -> OnboardingScenario {
        OnboardingScenario {
            id: id.into(),
            seen_key: None,
            when: None,
            trigger: OnboardingTrigger::Auto,
            priority: None,
            presentation: OnboardingPresentationRef { kind: SPOTLIGHT_PRESENTATION_KIND.into(), payload: spotlight_payload_value(payload) },
            tryout: None,
            experiment: None,
            developer_mode_variations: None,
            repeatable: None,
        }
    }

    /// `{ targetWindow: mainWindow, onAbort: Event.None }`.
    fn context(&self) -> OnboardingRunContext {
        OnboardingRunContext { target_window: self.window.clone(), on_did_show: None, on_abort: Event::none(), target_scope: None }
    }

    fn context_with_show(&self, on_did_show: impl Fn() + 'static) -> OnboardingRunContext {
        OnboardingRunContext { on_did_show: Some(Rc::new(on_did_show)), ..self.context() }
    }

    fn run(&self, scenario: &OnboardingScenario, context: &OnboardingRunContext) -> Promise<OnboardingRunResult> {
        self.presentation.run(scenario, context)
    }

    fn run_scenario(&self, id: &str, steps: Vec<SpotlightStep>, context: &OnboardingRunContext) -> Promise<RunResult> {
        self.run(&self.scenario(id, steps), context).then(RunResult::Scenario)
    }

    /// O contexto de um `runStep` de um passo só (`stepIndex: 0, visualStepCount: 1, canGoBack: false`).
    fn step_context(&self, base: OnboardingRunContext) -> OnboardingSequenceStepContext {
        OnboardingSequenceStepContext {
            base,
            cancellation_token: CancellationToken::None,
            step_index: 0,
            visual_step_index: 0,
            visual_step_count: 1,
            can_go_back: false,
            is_last_visual_step: true,
        }
    }

    fn run_step(&self, step: SpotlightStep, context: &OnboardingSequenceStepContext) -> Promise<OnboardingSequenceStepResult> {
        let sequence_step = OnboardingSequenceStep { id: step.id.clone(), kind: SPOTLIGHT_PRESENTATION_KIND.into(), payload: spotlight_step_value(step) };
        self.presentation.run_step(&sequence_step, context)
    }

    /// `runAsSequenceStep ? await presentation.runStep(...) : await presentation.run(createScenario(...))`.
    fn run_as(&self, sequence: bool, scenario_id: &str, step: SpotlightStep, context: OnboardingRunContext) -> Promise<RunResult> {
        if sequence {
            self.run_step(step, &self.step_context(context)).then(RunResult::Step)
        } else {
            self.run_scenario(scenario_id, vec![step], &context)
        }
    }

    /// `container.getElementsByClassName('monaco-button')` com o `.style.display !== 'none'` aplicado.
    fn visible_button_labels(&self) -> Vec<String> {
        self.container.buttons().iter().filter(|button| !button.is_hidden()).map(|button| button.label()).collect()
    }

    fn click_button(&self, index: usize) {
        self.container.buttons()[index].click();
    }
}

async fn settled<T: Clone + 'static>(promise: Promise<T>) -> T {
    promise.await.expect("a promessa resolveu")
}

fn counter() -> Rc<Cell<u32>> {
    Rc::new(Cell::new(0))
}

fn strings() -> Rc<RefCell<Vec<String>>> {
    Rc::new(RefCell::new(Vec::new()))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FinishWith {
    Activation,
    Escape,
    AdvanceOnly,
}

async fn separate_keyboard_focus_target(hide_next: bool, finish_with: FinishWith) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    let previous = env.plain_element();
    let keyboard_owner = env.plain_element();
    keyboard_owner.set_tab_index(0);
    previous.focus();
    let activated = guard.add_rc(Emitter::<()>::new());
    let focused = counter();
    let activations = counter();
    let target = env.target(
        "test.focusOwner",
        OnboardingTargetOptions {
            focus_target: Some(OnboardingFocusTarget {
                element: keyboard_owner.clone(),
                focus: Rc::new({
                    let (focused, keyboard_owner) = (focused.clone(), keyboard_owner.clone());
                    move || {
                        focused.set(focused.get() + 1);
                        keyboard_owner.focus();
                    }
                }),
            }),
            on_did_activate: Some(activated.event()),
            ..Default::default()
        },
    );
    target.set_tab_index(-1);
    guard.add(keyboard_owner.add_event_listener(OnboardingEventKind::KeyDown, false, {
        let (activations, activated) = (activations.clone(), activated.clone());
        move |event| {
            if event.key == OnboardingKey::Enter {
                activations.set(activations.get() + 1);
                activated.fire(());
            }
        }
    }));
    let shown = Rc::new(DeferredPromise::<()>::new());
    let result = env.run(
        &env.scenario(
            "test.focusOwner",
            vec![SpotlightStep {
                allow_target_interaction: Some(true),
                advance_on_target_click: Some(if finish_with == FinishWith::AdvanceOnly {
                    SpotlightTargetClickBehavior::AdvanceOnly
                } else {
                    SpotlightTargetClickBehavior::Bool(true)
                }),
                hide_next: Some(hide_next),
                ..step("focusOwner", "test.focusOwner", "New Session", "Start another task.")
            }],
        ),
        &env.context_with_show({
            let shown = shown.clone();
            move || {
                let _ = shown.complete(());
            }
        }),
    );
    settled(shown.promise.clone()).await;
    if !hide_next {
        let next = env.container.buttons().last().cloned().expect("o botão Próximo");
        next.key_down(OnboardingKey::Tab, false);
    }
    assert_eq!(env.window.document().active_element(), Some(keyboard_owner.clone()));
    let key = if finish_with == FinishWith::Escape { OnboardingKey::Escape } else { OnboardingKey::Enter };
    keyboard_owner.key_down(key, false);
    keyboard_owner.key_up(key, false);
    let outcome = settled(result).await.outcome;
    assert_eq!(
        (focused.get(), activations.get(), outcome, env.window.document().active_element() == Some(previous.clone()), env.container.has_callout()),
        (
            1,
            if finish_with == FinishWith::Activation { 1 } else { 0 },
            if finish_with == FinishWith::Escape { OnboardingOutcome::Skipped } else { OnboardingOutcome::Completed },
            finish_with != FinishWith::Activation,
            false,
        )
    );
}

#[test]
fn uses_a_separate_keyboard_focus_target_for_activation_hide_next_false() {
    run_with_real_timers(separate_keyboard_focus_target(false, FinishWith::Activation));
}

#[test]
fn uses_a_separate_keyboard_focus_target_for_escape_hide_next_false() {
    run_with_real_timers(separate_keyboard_focus_target(false, FinishWith::Escape));
}

#[test]
fn uses_a_separate_keyboard_focus_target_for_advance_only_hide_next_false() {
    run_with_real_timers(separate_keyboard_focus_target(false, FinishWith::AdvanceOnly));
}

#[test]
fn uses_a_separate_keyboard_focus_target_for_activation_hide_next_true() {
    run_with_real_timers(separate_keyboard_focus_target(true, FinishWith::Activation));
}

#[test]
fn uses_a_separate_keyboard_focus_target_for_escape_hide_next_true() {
    run_with_real_timers(separate_keyboard_focus_target(true, FinishWith::Escape));
}

#[test]
fn uses_a_separate_keyboard_focus_target_for_advance_only_hide_next_true() {
    run_with_real_timers(separate_keyboard_focus_target(true, FinishWith::AdvanceOnly));
}

#[test]
fn a_primary_action_runs_only_on_click_and_waits_for_accepted_selection() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let selected = guard.add_rc(Emitter::<Promise<bool>>::new());
        let shown = Rc::new(DeferredPromise::<()>::new());
        let accepted = DeferredPromise::<bool>::new();
        env.target("test.action", OnboardingTargetOptions { on_did_select: Some(selected.event()), ..Default::default() });
        let actions = counter();
        let completed = Rc::new(Cell::new(false));
        let result = env.run(
            &env.scenario(
                "test.action",
                vec![SpotlightStep {
                    advance_on_target_selection: Some(true),
                    hide_next: Some(true),
                    primary_action: Some(SpotlightPrimaryAction {
                        label: "Use Copilot".into(),
                        run: Rc::new({
                            let actions = actions.clone();
                            move |_: &CancellationToken| {
                                actions.set(actions.get() + 1);
                                MaybePromise::Value(())
                            }
                        }),
                    }),
                    ..step("action", "test.action", "Choose", "Choose a model.")
                }],
            ),
            &env.context_with_show({
                let shown = shown.clone();
                move || {
                    let _ = shown.complete(());
                }
            }),
        );
        let _ = result.then({
            let completed = completed.clone();
            move |_| completed.set(true)
        });
        settled(shown.promise.clone()).await;
        assert_eq!(
            (actions.get(), env.visible_button_labels()),
            (0, vec!["End Tour".to_string(), "Use Copilot".to_string()])
        );
        env.click_button(2);
        env.click_button(2);
        selected.fire(accepted.promise.clone());
        settled(Promise::resolved(())).await;
        assert_eq!((actions.get(), completed.get()), (1, false));
        let _ = accepted.complete(true);
        assert_eq!(settled(result).await.outcome, OnboardingOutcome::Completed);
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Exit {
    Back,
    EndTour,
    Abort,
    Replace,
}

async fn cancels_a_pending_primary_action(exit: Exit) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    let abort = guard.add_rc(Emitter::<()>::new());
    let shown = Rc::new(DeferredPromise::<()>::new());
    let action = DeferredPromise::<()>::new();
    let cancelled = Rc::new(DeferredPromise::<()>::new());
    let target = env.target("test.cancelAction", OnboardingTargetOptions::default());
    let action_token: Rc<RefCell<Option<CancellationToken>>> = Rc::new(RefCell::new(None));
    let action_promise = action.promise.clone();
    let result = env.run_step(
        SpotlightStep {
            missing_target: Some(SpotlightMissingTargetBehavior::Abort),
            primary_action: Some(SpotlightPrimaryAction {
                label: "Use Copilot".into(),
                run: Rc::new({
                    let (action_token, cancelled, store) = (action_token.clone(), cancelled.clone(), guard.store());
                    move |token: &CancellationToken| {
                        *action_token.borrow_mut() = Some(token.clone());
                        store.add(token.on_cancellation_requested().listen({
                            let cancelled = cancelled.clone();
                            move |_| {
                                let _ = cancelled.complete(());
                            }
                        }));
                        MaybePromise::Promise(action_promise.clone())
                    }
                }),
            }),
            ..step("action", "test.cancelAction", "Choose", "Choose a model.")
        },
        &OnboardingSequenceStepContext {
            base: OnboardingRunContext {
                on_did_show: Some(Rc::new({
                    let shown = shown.clone();
                    move || {
                        let _ = shown.complete(());
                    }
                })),
                on_abort: abort.event(),
                ..env.context()
            },
            cancellation_token: CancellationToken::None,
            step_index: 1,
            visual_step_index: 1,
            visual_step_count: 2,
            can_go_back: true,
            is_last_visual_step: true,
        },
    );
    settled(shown.promise.clone()).await;
    let buttons = env.container.buttons();
    let (end_tour, back, primary) = (buttons[0].clone(), buttons[1].clone(), buttons[2].clone());
    primary.click();
    match exit {
        Exit::Back => back.click(),
        Exit::EndTour => end_tour.click(),
        Exit::Abort => abort.fire(()),
        Exit::Replace => target.remove(),
    }
    settled(cancelled.promise.clone()).await;
    let _ = action.complete(());
    let token = action_token.borrow().clone();
    let action = settled(result).await.action;
    assert_eq!(
        (token.map(|token| token.is_cancellation_requested()), action),
        (
            Some(true),
            match exit {
                Exit::Back => OnboardingSequenceStepAction::Back,
                Exit::EndTour => OnboardingSequenceStepAction::SkipSequence,
                _ => OnboardingSequenceStepAction::Abort,
            }
        )
    );
}

#[test]
fn cancels_a_pending_primary_action_on_back_and_ignores_its_late_result() {
    run_with_real_timers(cancels_a_pending_primary_action(Exit::Back));
}

#[test]
fn cancels_a_pending_primary_action_on_end_tour_and_ignores_its_late_result() {
    run_with_real_timers(cancels_a_pending_primary_action(Exit::EndTour));
}

#[test]
fn cancels_a_pending_primary_action_on_abort_and_ignores_its_late_result() {
    run_with_real_timers(cancels_a_pending_primary_action(Exit::Abort));
}

#[test]
fn cancels_a_pending_primary_action_on_replace_and_ignores_its_late_result() {
    run_with_real_timers(cancels_a_pending_primary_action(Exit::Replace));
}

#[test]
fn a_scoped_adapter_reports_selection_without_replacing_the_owner_target_marker() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let selected = guard.add_rc(Emitter::<Promise<bool>>::new());
        let target = env.target("test.owner", OnboardingTargetOptions::default());
        guard.add(register_onboarding_target_provider("test.adapter", {
            let (target, selected) = (target.clone(), selected.clone());
            move |_| {
                let mut adapter = OnboardingTarget::new(target.clone());
                adapter.on_did_select = Some(selected.event());
                Some(adapter)
            }
        }));
        let result = settled(env.run(
            &env.scenario(
                "test.adapter",
                vec![SpotlightStep {
                    allow_target_interaction: Some(true),
                    advance_on_target_selection: Some(true),
                    hide_next: Some(true),
                    ..step("choose", "test.adapter", "Open Session", "Use the real row.")
                }],
            ),
            &env.context_with_show({
                let selected = selected.clone();
                move || selected.fire(Promise::resolved(true))
            }),
        ))
        .await;
        assert_eq!(
            (result.outcome, target.get_attribute(ONBOARDING_TARGET_ATTR), selected.has_listeners()),
            (OnboardingOutcome::Completed, Some("test.owner".to_string()), false)
        );
    });
}

#[test]
fn resolves_a_target_within_the_prepared_instance_scope() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let opened = strings();
        let target_id = "test.spotlight.scoped";
        let first = env.element();
        env.mark(
            &first,
            target_id,
            OnboardingTargetOptions {
                scope: Some(OnboardingTargetScope::Fixed("first".into())),
                open: open_fn({
                    let opened = opened.clone();
                    move || opened.borrow_mut().push("first".to_string())
                }),
                ..Default::default()
            },
        );
        let second = env.element();
        env.mark(
            &second,
            target_id,
            OnboardingTargetOptions {
                scope: Some(OnboardingTargetScope::Fixed("second".into())),
                open: open_fn({
                    let (opened, second) = (opened.clone(), second.clone());
                    move || {
                        opened.borrow_mut().push("second".to_string());
                        second.click();
                    }
                }),
                ..Default::default()
            },
        );

        let result = settled(env.run(
            &env.scenario(
                "test.spotlight.scoped",
                vec![SpotlightStep {
                    open_target: Some(SpotlightOpenTarget::Bool(true)),
                    advance_on_target_click: click_behavior(true),
                    ..step("scoped", target_id, "Scoped target", "Use the prepared target.")
                }],
            ),
            &OnboardingRunContext { target_scope: Some("second".into()), ..env.context() },
        ))
        .await;

        assert_eq!(
            (opened.borrow().clone(), result),
            (
                vec!["second".to_string()],
                run_result(OnboardingOutcome::Completed, true, OnboardingDismissReason::TargetClick, 0.0, 1.0)
            )
        );
    });
}

async fn waits_for_a_picker_lost_after_showing(sequence: bool, returns: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    let target_id = "test.spotlight.disappearingPicker";
    let hidden_while_waiting = Rc::new(Cell::new(false));
    let shown = counter();
    let prepared = counter();
    let opened = strings();
    let original = env.element();
    env.mark(
        &original,
        target_id,
        OnboardingTargetOptions {
            open: open_fn({
                let (env, original, hidden_while_waiting, opened) = (env.clone(), original.clone(), hidden_while_waiting.clone(), opened.clone());
                move || {
                    original.remove();
                    env.store.add(env.window.schedule_at_next_animation_frame({
                        let (env, hidden_while_waiting) = (env.clone(), hidden_while_waiting.clone());
                        move || hidden_while_waiting.set(env.container.overlays()[0].is_hidden())
                    }));
                    if returns {
                        env.store.add(disposable_timeout(
                            {
                                let (env, opened) = (env.clone(), opened.clone());
                                move || {
                                    let replacement = env.element();
                                    env.mark(
                                        &replacement,
                                        target_id,
                                        OnboardingTargetOptions {
                                            open: open_fn({
                                                let (env, opened, replacement) = (env.clone(), opened.clone(), replacement.clone());
                                                move || {
                                                    opened.borrow_mut().push(env.container.overlays()[0].hole_top_style().expect("o buraco"));
                                                    replacement.click();
                                                }
                                            }),
                                            ..Default::default()
                                        },
                                    );
                                    replacement.set_position(100.0, 200.0);
                                }
                            },
                            100,
                            None,
                        ));
                    }
                }
            }),
            ..Default::default()
        },
    );
    let next = env.element();
    env.mark(
        &next,
        "test.spotlight.afterMissing",
        OnboardingTargetOptions {
            open: open_fn({
                let (opened, next) = (opened.clone(), next.clone());
                move || {
                    opened.borrow_mut().push("next".to_string());
                    next.click();
                }
            }),
            ..Default::default()
        },
    );
    let picker = SpotlightStep {
        open_target: Some(SpotlightOpenTarget::Bool(true)),
        advance_on_target_click: click_behavior(true),
        missing_target: Some(SpotlightMissingTargetBehavior::Wait { timeout_ms: 300.0, on_timeout: None }),
        on_before_show: Some(Rc::new({
            let prepared = prepared.clone();
            move || {
                prepared.set(prepared.get() + 1);
                MaybePromise::Value(())
            }
        })),
        ..step("picker", target_id, "Picker", "Choose a value.")
    };
    let context = env.context_with_show({
        let shown = shown.clone();
        move || shown.set(shown.get() + 1)
    });
    let result = if sequence {
        settled(env.run_as(true, "test.spotlight.disappearingPicker", picker, context)).await
    } else {
        let next_step = SpotlightStep {
            open_target: Some(SpotlightOpenTarget::Bool(true)),
            advance_on_target_click: click_behavior(true),
            ..step("next", "test.spotlight.afterMissing", "Next", "The tour continues.")
        };
        settled(env.run_scenario("test.spotlight.disappearingPicker", vec![picker, next_step], &context)).await
    };

    let mut expected_opened = Vec::new();
    if returns {
        expected_opened.push("194px".to_string());
    }
    if !sequence {
        expected_opened.push("next".to_string());
    }
    assert_eq!(
        (hidden_while_waiting.get(), prepared.get(), shown.get(), opened.borrow().clone(), result.label()),
        (
            true,
            1,
            if sequence { 1 } else { 2 },
            expected_opened,
            if sequence {
                if returns { "next" } else { "skipStep" }
            } else {
                OnboardingOutcome::Completed.as_str()
            }
        )
    );
}

async fn waits_for_a_picker_replaced_while_the_target_settles(sequence: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    let target_id = "test.spotlight.replacedPicker";
    let original = env.target(target_id, OnboardingTargetOptions::default());
    let replacement: Rc<RefCell<Option<OnboardingElement>>> = Rc::new(RefCell::new(None));
    let shown: Rc<RefCell<Vec<(bool, String)>>> = Rc::new(RefCell::new(Vec::new()));
    let picker = SpotlightStep {
        missing_target: Some(SpotlightMissingTargetBehavior::Wait { timeout_ms: 1000.0, on_timeout: None }),
        on_before_show: Some(Rc::new({
            let (env, original, replacement) = (env.clone(), original.clone(), replacement.clone());
            move || {
                env.store.add(env.window.schedule_at_next_animation_frame({
                    let (env, original, replacement) = (env.clone(), original.clone(), replacement.clone());
                    move || {
                        original.remove();
                        env.store.add(disposable_timeout(
                            {
                                let (env, replacement) = (env.clone(), replacement.clone());
                                move || {
                                    let element = env.target(target_id, OnboardingTargetOptions::default());
                                    element.set_position(100.0, 200.0);
                                    *replacement.borrow_mut() = Some(element);
                                }
                            },
                            100,
                            None,
                        ));
                    }
                }));
                MaybePromise::Value(())
            }
        })),
        ..step("picker", target_id, "Picker", "Choose a value.")
    };
    let context = env.context_with_show({
        let (env, replacement, shown) = (env.clone(), replacement.clone(), shown.clone());
        move || {
            shown.borrow_mut().push((
                replacement.borrow().as_ref().is_some_and(OnboardingElement::is_connected),
                env.container.overlays()[0].hole_top_style().expect("o buraco"),
            ));
            env.click_button(2);
        }
    });
    let result = settled(env.run_as(sequence, "test.spotlight.replacedPicker", picker, context)).await;

    assert_eq!((result.completed(), shown.borrow().clone()), (true, vec![(true, "194px".to_string())]));
}

async fn waits_for_an_owner_provided_control(sequence: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    let target = env.element();
    let ready = Rc::new(Cell::new(false));
    let opened = counter();
    guard.add(register_onboarding_target_provider("test.provided", {
        let (target, ready, opened) = (target.clone(), ready.clone(), opened.clone());
        move |scope| {
            if ready.get() && scope == Some("prepared") {
                let mut provided = OnboardingTarget::new(target.clone());
                provided.open = open_fn({
                    let (target, opened) = (target.clone(), opened.clone());
                    move || {
                        opened.set(opened.get() + 1);
                        target.click();
                    }
                });
                Some(provided)
            } else {
                None
            }
        }
    }));
    let picker = SpotlightStep {
        open_target: Some(SpotlightOpenTarget::Bool(true)),
        advance_on_target_click: click_behavior(true),
        missing_target: Some(SpotlightMissingTargetBehavior::Wait { timeout_ms: 500.0, on_timeout: None }),
        on_before_show: Some(Rc::new({
            let (store, ready) = (guard.store(), ready.clone());
            move || {
                store.add(disposable_timeout({
                    let ready = ready.clone();
                    move || ready.set(true)
                }, 100, None));
                MaybePromise::Value(())
            }
        })),
        ..step("provided", "test.provided", "Provided control", "Use the control exposed by its owner.")
    };
    let context = OnboardingRunContext { target_scope: Some("prepared".into()), ..env.context() };
    let result = settled(env.run_as(sequence, "test.provided", picker, context)).await;

    assert_eq!(
        (result.shown(), result.completed(), opened.get(), target.has_attribute(ONBOARDING_TARGET_ATTR)),
        (true, true, 1, false)
    );
}

#[test]
fn waits_for_a_picker_lost_after_showing_then_skips_run_as_sequence_step_false() {
    run_with_real_timers(waits_for_a_picker_lost_after_showing(false, false));
}

#[test]
fn waits_for_a_picker_lost_after_showing_then_reattaches_run_as_sequence_step_false() {
    run_with_real_timers(waits_for_a_picker_lost_after_showing(false, true));
}

#[test]
fn waits_for_a_picker_replaced_while_the_target_settles_run_as_sequence_step_false() {
    run_with_real_timers(waits_for_a_picker_replaced_while_the_target_settles(false));
}

#[test]
fn waits_for_an_owner_provided_control_and_invokes_its_opener_run_as_sequence_step_false() {
    run_with_faked_timers(waits_for_an_owner_provided_control(false));
}

#[test]
fn waits_for_a_picker_lost_after_showing_then_skips_run_as_sequence_step_true() {
    run_with_real_timers(waits_for_a_picker_lost_after_showing(true, false));
}

#[test]
fn waits_for_a_picker_lost_after_showing_then_reattaches_run_as_sequence_step_true() {
    run_with_real_timers(waits_for_a_picker_lost_after_showing(true, true));
}

#[test]
fn waits_for_a_picker_replaced_while_the_target_settles_run_as_sequence_step_true() {
    run_with_real_timers(waits_for_a_picker_replaced_while_the_target_settles(true));
}

#[test]
fn waits_for_an_owner_provided_control_and_invokes_its_opener_run_as_sequence_step_true() {
    run_with_faked_timers(waits_for_an_owner_provided_control(true));
}

#[test]
fn can_abort_while_waiting_for_a_picker_that_disappeared_after_showing() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let abort = guard.add_rc(Emitter::<()>::new());
        let target_id = "test.spotlight.disappearingAbort";
        let target = env.element();
        env.mark(
            &target,
            target_id,
            OnboardingTargetOptions {
                open: open_fn({
                    let (target, abort, store) = (target.clone(), abort.clone(), guard.store());
                    move || {
                        target.remove();
                        store.add(disposable_timeout({
                            let abort = abort.clone();
                            move || abort.fire(())
                        }, 100, None));
                    }
                }),
                ..Default::default()
            },
        );
        let result = settled(env.run(
            &env.scenario(
                "test.spotlight.disappearingAbort",
                vec![SpotlightStep {
                    open_target: Some(SpotlightOpenTarget::Bool(true)),
                    missing_target: Some(SpotlightMissingTargetBehavior::Wait { timeout_ms: 10_000.0, on_timeout: None }),
                    ..step("picker", target_id, "Picker", "Choose a value.")
                }],
            ),
            &OnboardingRunContext { on_abort: abort.event(), ..env.context() },
        ))
        .await;

        assert_eq!((result.outcome, env.container.overlays().len()), (OnboardingOutcome::Aborted, 0));
    });
}

#[test]
fn waits_for_a_late_target_and_skips_a_missing_target_immediately() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);

        let late_target_id = "test.spotlight.lateTarget";
        let shown = counter();
        let late_scenario = env.scenario(
            "test.spotlight.wait",
            vec![SpotlightStep {
                missing_target: Some(SpotlightMissingTargetBehavior::Wait { timeout_ms: 500.0, on_timeout: None }),
                advance_on_target_click: click_behavior(true),
                open_target: Some(SpotlightOpenTarget::Bool(true)),
                on_before_show: Some(Rc::new({
                    let (env, store) = (env.clone(), guard.store());
                    move || {
                        store.add(disposable_timeout({
                            let env = env.clone();
                            move || {
                                env.advancing_target(late_target_id, || {});
                            }
                        }, 100, None));
                        MaybePromise::Value(())
                    }
                })),
                ..step("late", late_target_id, "Late target", "Late target description")
            }],
        );
        let counting_context = env.context_with_show({
            let shown = shown.clone();
            move || shown.set(shown.get() + 1)
        });
        let late_result = settled(env.run(&late_scenario, &counting_context)).await;

        let missing_scenario = env.scenario(
            "test.spotlight.skip",
            vec![SpotlightStep {
                missing_target: Some(SpotlightMissingTargetBehavior::Skip),
                ..step("missing", "test.spotlight.missingTarget", "Missing target", "Missing target description")
            }],
        );
        let missing_result = settled(env.run(&missing_scenario, &counting_context)).await;

        assert_eq!(
            (late_result, missing_result, shown.get()),
            (
                run_result(OnboardingOutcome::Completed, true, OnboardingDismissReason::TargetClick, 0.0, 1.0),
                run_result(OnboardingOutcome::Completed, false, OnboardingDismissReason::Completed, 0.0, 1.0),
                1,
            )
        );
    });
}

#[test]
fn excludes_skipped_steps_from_displayed_progress() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let progress: Rc<RefCell<Vec<(String, bool, String)>>> = Rc::new(RefCell::new(Vec::new()));

        let create_advancing_target = |id: &str| {
            let (env_for_open, progress) = (env.clone(), progress.clone());
            env.advancing_target(id, move || {
                let buttons = env_for_open.container.buttons();
                progress.borrow_mut().push((env_for_open.container.overlays()[0].counter(), buttons[1].is_hidden(), buttons[2].label()));
            });
        };

        create_advancing_target("test.spotlight.second");
        create_advancing_target("test.spotlight.third");
        let result = settled(env.run(
            &env.scenario(
                "test.spotlight.skippedProgress",
                vec![
                    SpotlightStep {
                        when: Some(ContextKeyExpr::equals("testSpotlightShowFirst", true)),
                        ..step("first", "test.spotlight.first", "First", "Skipped first step")
                    },
                    SpotlightStep {
                        open_target: Some(SpotlightOpenTarget::Bool(true)),
                        advance_on_target_click: click_behavior(true),
                        ..step("second", "test.spotlight.second", "Second", "First visible step")
                    },
                    SpotlightStep {
                        open_target: Some(SpotlightOpenTarget::Bool(true)),
                        advance_on_target_click: click_behavior(true),
                        ..step("third", "test.spotlight.third", "Third", "Second visible step")
                    },
                ],
            ),
            &env.context(),
        ))
        .await;

        assert_eq!(
            (progress.borrow().clone(), result),
            (
                vec![
                    ("1 of 2".to_string(), true, "Next".to_string()),
                    ("2 of 2".to_string(), false, "Done".to_string()),
                ],
                run_result(OnboardingOutcome::Completed, true, OnboardingDismissReason::TargetClick, 2.0, 3.0)
            )
        );
    });
}

async fn forwards_acknowledgment_options(sequence: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    let visible_buttons: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let acknowledge = SpotlightStep {
        next_button_label: Some("Understood".into()),
        missing_target: Some(SpotlightMissingTargetBehavior::Abort),
        on_before_show: Some(Rc::new({
            let env = env.clone();
            move || {
                MaybePromise::Promise(Promise::resolved(()).then({
                    let env = env.clone();
                    move |_| {
                        env.target("test.spotlight.acknowledge", OnboardingTargetOptions::default());
                    }
                }))
            }
        })),
        ..step("acknowledge", "test.spotlight.acknowledge", "Archive", "Archive the session")
    };
    let context = env.context_with_show({
        let (env, visible_buttons) = (env.clone(), visible_buttons.clone());
        move || {
            *visible_buttons.borrow_mut() = env.visible_button_labels();
            env.click_button(2);
        }
    });

    let result = settled(env.run_as(sequence, "test.spotlight.acknowledgment", acknowledge, context)).await;

    assert_eq!(
        (visible_buttons.borrow().clone(), result),
        (
            vec!["Understood".to_string()],
            if sequence {
                RunResult::Step(step_result(OnboardingSequenceStepAction::Next, true, Some(OnboardingDismissReason::Completed)))
            } else {
                RunResult::Scenario(run_result(OnboardingOutcome::Completed, true, OnboardingDismissReason::Completed, 0.0, 1.0))
            }
        )
    );
}

async fn consumed_target_activation_completes_the_last_step(sequence: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    let target = env.target("test.spotlight.advanceOnly", OnboardingTargetOptions::default());
    let native_actions = counter();
    guard.add(target.add_event_listener(OnboardingEventKind::Click, false, {
        let native_actions = native_actions.clone();
        move |_| native_actions.set(native_actions.get() + 1)
    }));
    let archive = SpotlightStep {
        next_button_label: Some("Understood".into()),
        advance_on_target_click: Some(SpotlightTargetClickBehavior::AdvanceOnly),
        hide_next: Some(false),
        ..step("archive", "test.spotlight.advanceOnly", "Archive", "Archive the session")
    };
    let visible_buttons: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let context = env.context_with_show({
        let (env, visible_buttons, target) = (env.clone(), visible_buttons.clone(), target.clone());
        move || {
            *visible_buttons.borrow_mut() = env.visible_button_labels();
            target.click();
        }
    });
    let result = settled(env.run_as(sequence, "test.spotlight.advanceOnly", archive, context)).await;

    assert_eq!(
        (native_actions.get(), visible_buttons.borrow().clone(), result),
        (
            0,
            vec!["Understood".to_string()],
            if sequence {
                RunResult::Step(step_result(OnboardingSequenceStepAction::Next, true, Some(OnboardingDismissReason::TargetClick)))
            } else {
                RunResult::Scenario(run_result(OnboardingOutcome::Completed, true, OnboardingDismissReason::TargetClick, 0.0, 1.0))
            }
        )
    );
}

async fn missing_abort_target_aborts_without_showing(sequence: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    let prepared = Rc::new(Cell::new(false));
    let shown = counter();
    let missing = SpotlightStep {
        missing_target: Some(SpotlightMissingTargetBehavior::Abort),
        on_before_show: Some(Rc::new({
            let prepared = prepared.clone();
            move || {
                MaybePromise::Promise(Promise::resolved(()).then({
                    let prepared = prepared.clone();
                    move |_| prepared.set(true)
                }))
            }
        })),
        ..step("missing", "test.spotlight.abortMissing", "Missing", "Missing target")
    };
    let context = env.context_with_show({
        let shown = shown.clone();
        move || shown.set(shown.get() + 1)
    });

    let result = settled(env.run_as(sequence, "test.spotlight.abortMissing", missing, context)).await;

    assert_eq!(
        (result, prepared.get(), shown.get()),
        (
            if sequence {
                RunResult::Step(step_result(OnboardingSequenceStepAction::Abort, false, None))
            } else {
                RunResult::Scenario(run_result(OnboardingOutcome::Aborted, false, OnboardingDismissReason::Aborted, 0.0, 1.0))
            },
            true,
            0
        )
    );
}

async fn missing_target_waits_before_timeout(sequence: bool, on_timeout: Option<SpotlightTimeoutAction>) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    let shown = counter();
    let missing = SpotlightStep {
        missing_target: Some(SpotlightMissingTargetBehavior::Wait { timeout_ms: 100.0, on_timeout }),
        ..step("missing", "test.spotlight.timeoutMissing", "Missing", "Missing target")
    };
    let context = env.context_with_show({
        let shown = shown.clone();
        move || shown.set(shown.get() + 1)
    });
    let start_time = runtime::date_now();
    let result = settled(env.run_as(sequence, "test.spotlight.timeoutMissing", missing, context)).await;

    let aborts = on_timeout == Some(SpotlightTimeoutAction::Abort);
    assert_eq!(
        (runtime::date_now() - start_time, result, shown.get()),
        (
            100,
            if sequence {
                RunResult::Step(step_result(
                    if aborts { OnboardingSequenceStepAction::Abort } else { OnboardingSequenceStepAction::SkipStep },
                    false,
                    None,
                ))
            } else {
                RunResult::Scenario(run_result(
                    if aborts { OnboardingOutcome::Aborted } else { OnboardingOutcome::Completed },
                    false,
                    if aborts { OnboardingDismissReason::Aborted } else { OnboardingDismissReason::Completed },
                    0.0,
                    1.0,
                ))
            },
            0
        )
    );
}

#[test]
fn forwards_acknowledgment_options_after_awaiting_target_preparation_run_as_sequence_step_false() {
    run_with_real_timers(forwards_acknowledgment_options(false));
}

#[test]
fn consumed_target_activation_completes_the_last_step_run_as_sequence_step_false() {
    run_with_real_timers(consumed_target_activation_completes_the_last_step(false));
}

#[test]
fn missing_abort_target_aborts_without_showing_run_as_sequence_step_false() {
    run_with_real_timers(missing_abort_target_aborts_without_showing(false));
}

#[test]
fn missing_target_waits_before_default_skip_run_as_sequence_step_false() {
    run_with_faked_timers(missing_target_waits_before_timeout(false, None));
}

#[test]
fn missing_target_waits_before_skip_run_as_sequence_step_false() {
    run_with_faked_timers(missing_target_waits_before_timeout(false, Some(SpotlightTimeoutAction::Skip)));
}

#[test]
fn missing_target_waits_before_abort_run_as_sequence_step_false() {
    run_with_faked_timers(missing_target_waits_before_timeout(false, Some(SpotlightTimeoutAction::Abort)));
}

#[test]
fn forwards_acknowledgment_options_after_awaiting_target_preparation_run_as_sequence_step_true() {
    run_with_real_timers(forwards_acknowledgment_options(true));
}

#[test]
fn consumed_target_activation_completes_the_last_step_run_as_sequence_step_true() {
    run_with_real_timers(consumed_target_activation_completes_the_last_step(true));
}

#[test]
fn missing_abort_target_aborts_without_showing_run_as_sequence_step_true() {
    run_with_real_timers(missing_abort_target_aborts_without_showing(true));
}

#[test]
fn missing_target_waits_before_default_skip_run_as_sequence_step_true() {
    run_with_faked_timers(missing_target_waits_before_timeout(true, None));
}

#[test]
fn missing_target_waits_before_skip_run_as_sequence_step_true() {
    run_with_faked_timers(missing_target_waits_before_timeout(true, Some(SpotlightTimeoutAction::Skip)));
}

#[test]
fn missing_target_waits_before_abort_run_as_sequence_step_true() {
    run_with_faked_timers(missing_target_waits_before_timeout(true, Some(SpotlightTimeoutAction::Abort)));
}

#[test]
fn hides_the_previous_step_while_waiting_for_the_next_target() {
    run_with_faked_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        env.advancing_target("test.spotlight.firstVisible", || {});
        let hidden_while_waiting = Rc::new(Cell::new(false));

        let result = settled(env.run(
            &env.scenario(
                "test.spotlight.hiddenWhileWaiting",
                vec![
                    SpotlightStep {
                        open_target: Some(SpotlightOpenTarget::Bool(true)),
                        advance_on_target_click: click_behavior(true),
                        ..step("first", "test.spotlight.firstVisible", "First", "First step")
                    },
                    SpotlightStep {
                        missing_target: Some(SpotlightMissingTargetBehavior::Wait { timeout_ms: 500.0, on_timeout: None }),
                        open_target: Some(SpotlightOpenTarget::Bool(true)),
                        advance_on_target_click: click_behavior(true),
                        on_before_show: Some(Rc::new({
                            let (env, hidden_while_waiting, store) = (env.clone(), hidden_while_waiting.clone(), guard.store());
                            move || {
                                hidden_while_waiting.set(env.container.overlays()[0].is_hidden());
                                store.add(disposable_timeout({
                                    let env = env.clone();
                                    move || {
                                        env.advancing_target("test.spotlight.secondLate", || {});
                                    }
                                }, 100, None));
                                MaybePromise::Value(())
                            }
                        })),
                        ..step("second", "test.spotlight.secondLate", "Second", "Late second step")
                    },
                ],
            ),
            &env.context(),
        ))
        .await;

        assert_eq!(
            (hidden_while_waiting.get(), result),
            (true, run_result(OnboardingOutcome::Completed, true, OnboardingDismissReason::TargetClick, 1.0, 2.0))
        );
    });
}

#[test]
fn aborts_immediately_while_waiting_for_a_target() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let abort = guard.add_rc(Emitter::<()>::new());

        let result = settled(env.run(
            &env.scenario(
                "test.spotlight.abortWait",
                vec![SpotlightStep {
                    missing_target: Some(SpotlightMissingTargetBehavior::Wait { timeout_ms: 60_000.0, on_timeout: None }),
                    on_before_show: Some(Rc::new({
                        let (abort, store) = (abort.clone(), guard.store());
                        move || {
                            store.add(disposable_timeout({
                                let abort = abort.clone();
                                move || abort.fire(())
                            }, 0, None));
                            MaybePromise::Value(())
                        }
                    })),
                    ..step("missing", "test.spotlight.abortMissing", "Missing", "Missing target")
                }],
            ),
            &OnboardingRunContext { on_abort: abort.event(), ..env.context() },
        ))
        .await;

        assert_eq!(result, run_result(OnboardingOutcome::Aborted, false, OnboardingDismissReason::Aborted, 0.0, 1.0));
    });
}

#[test]
fn opens_the_target_and_advances_when_its_context_condition_becomes_true() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let workspace_selected = env.context_keys.create_key("testSpotlightWorkspaceSelected", Dynamic::Bool(false));
        let state_at_open: Rc<RefCell<Option<(bool, bool)>>> = Rc::new(RefCell::new(None));
        env.target(
            "test.spotlight.workspace",
            OnboardingTargetOptions {
                open: open_fn({
                    let (env, state_at_open, workspace_selected) = (env.clone(), state_at_open.clone(), workspace_selected.clone());
                    move || {
                        let overlay = env.container.overlays()[0].clone();
                        let next_hidden = env.container.buttons().last().is_some_and(|button| button.is_hidden());
                        *state_at_open.borrow_mut() = Some((next_hidden, overlay.has_target_overlay_visible()));
                        workspace_selected.set(Dynamic::Bool(true));
                    }
                }),
                ..Default::default()
            },
        );

        let result = settled(env.run(
            &env.scenario(
                "test.spotlight.advanceWhen",
                vec![SpotlightStep {
                    open_target: Some(SpotlightOpenTarget::Bool(true)),
                    allow_target_interaction: Some(true),
                    advance_when: Some(ContextKeyExpr::equals("testSpotlightWorkspaceSelected", true)),
                    ..step("workspace", "test.spotlight.workspace", "Workspace", "Choose a workspace")
                }],
            ),
            &env.context(),
        ))
        .await;

        assert_eq!(
            (*state_at_open.borrow(), result),
            (
                Some((true, true)),
                run_result(OnboardingOutcome::Completed, true, OnboardingDismissReason::Completed, 0.0, 1.0)
            )
        );
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Advance {
    Next,
    Select,
}

async fn conditionally_opens_the_target(preselected: bool, action: Advance) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    env.context_keys.create_key("testSpotlightWorkspaceSelected", Dynamic::Bool(false));
    let selected = guard.add_rc(Emitter::<Promise<bool>>::new());
    let opened = counter();
    let target_id = "test.spotlight.optionalWorkspace";
    env.target(
        target_id,
        OnboardingTargetOptions {
            open: open_fn({
                let opened = opened.clone();
                move || opened.set(opened.get() + 1)
            }),
            has_selection: Some(Rc::new(move || preselected)),
            on_did_select: Some(selected.event()),
            ..Default::default()
        },
    );
    let model_opened = counter();
    env.target(
        "test.spotlight.collapsedModel",
        OnboardingTargetOptions {
            open: open_fn({
                let model_opened = model_opened.clone();
                move || model_opened.set(model_opened.get() + 1)
            }),
            ..Default::default()
        },
    );
    let buttons: Rc<RefCell<Vec<(bool, String)>>> = Rc::new(RefCell::new(Vec::new()));
    let result = settled(env.run(
        &env.scenario(
            "test.spotlight.optionalSelection",
            vec![
                SpotlightStep {
                    open_target: Some(SpotlightOpenTarget::IfUnselected),
                    allow_target_interaction: Some(true),
                    advance_on_target_selection: Some(true),
                    ..step("workspace", target_id, "Workspace", "Choose a workspace or continue")
                },
                SpotlightStep {
                    open_target: Some(SpotlightOpenTarget::Bool(false)),
                    allow_target_interaction: Some(true),
                    ..step("model", "test.spotlight.collapsedModel", "Model", "Choose a model")
                },
            ],
        ),
        &env.context_with_show({
            let (env, buttons, selected, store) = (env.clone(), buttons.clone(), selected.clone(), guard.store());
            move || {
                let next = env.container.buttons()[2].clone();
                buttons.borrow_mut().push((next.is_hidden(), next.label()));
                let is_workspace_step = buttons.borrow().len() == 1;
                store.add(disposable_timeout(
                    {
                        let selected = selected.clone();
                        move || {
                            if is_workspace_step && action == Advance::Select {
                                selected.fire(Promise::resolved(true));
                            } else {
                                next.click();
                            }
                        }
                    },
                    0,
                    None,
                ));
            }
        }),
    ))
    .await;
    assert_eq!(
        (opened.get(), model_opened.get(), buttons.borrow().clone(), selected.has_listeners(), result),
        (
            if preselected { 0 } else { 1 },
            0,
            vec![(false, "Next".to_string()), (false, "Done".to_string())],
            false,
            run_result(OnboardingOutcome::Completed, true, OnboardingDismissReason::Completed, 1.0, 2.0)
        )
    );
}

#[test]
fn conditionally_opens_the_target_and_advances_via_next_with_preselected_false() {
    run_with_real_timers(conditionally_opens_the_target(false, Advance::Next));
}

#[test]
fn conditionally_opens_the_target_and_advances_via_select_with_preselected_false() {
    run_with_real_timers(conditionally_opens_the_target(false, Advance::Select));
}

#[test]
fn conditionally_opens_the_target_and_advances_via_next_with_preselected_true() {
    run_with_real_timers(conditionally_opens_the_target(true, Advance::Next));
}

#[test]
fn conditionally_opens_the_target_and_advances_via_select_with_preselected_true() {
    run_with_real_timers(conditionally_opens_the_target(true, Advance::Select));
}

async fn waits_for_selection_acceptance(accepted: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    let selection = guard.add_rc(Emitter::<Promise<bool>>::new());
    let acceptance = Rc::new(DeferredPromise::<bool>::new());
    env.target("test.spotlight.pendingWorkspace", OnboardingTargetOptions { on_did_select: Some(selection.event()), ..Default::default() });
    let shown = strings();
    let shown_before_acceptance = strings();
    let result = settled(env.run(
        &env.scenario(
            "test.spotlight.pendingAcceptance",
            vec![
                SpotlightStep {
                    advance_on_target_selection: Some(true),
                    ..step("workspace", "test.spotlight.pendingWorkspace", "Workspace", "Choose a workspace")
                },
                SpotlightStep {
                    missing_target: Some(SpotlightMissingTargetBehavior::Wait { timeout_ms: 5_000.0, on_timeout: None }),
                    ..step("model", "test.spotlight.acceptedModel", "Model", "Choose a model")
                },
            ],
        ),
        &env.context_with_show({
            let (env, shown, shown_before_acceptance, selection, acceptance, store) =
                (env.clone(), shown.clone(), shown_before_acceptance.clone(), selection.clone(), acceptance.clone(), guard.store());
            move || {
                let title = env.container.overlays()[0].title();
                shown.borrow_mut().push(title.clone());
                if title == "Model" {
                    env.click_button(2);
                    return;
                }
                selection.fire(acceptance.promise.clone());
                store.add(disposable_timeout(
                    {
                        let (env, shown, shown_before_acceptance, acceptance) =
                            (env.clone(), shown.clone(), shown_before_acceptance.clone(), acceptance.clone());
                        move || {
                            *shown_before_acceptance.borrow_mut() = shown.borrow().clone();
                            env.target("test.spotlight.acceptedModel", OnboardingTargetOptions::default());
                            let completed = acceptance.complete(accepted);
                            if !accepted {
                                let env = env.clone();
                                let _ = completed.then_promise(move |_| Promise::resolved(()).then(move |_| env.click_button(0)));
                            }
                        }
                    },
                    6_000,
                    None,
                ));
            }
        }),
    ))
    .await;
    assert_eq!(
        (shown_before_acceptance.borrow().clone(), shown.borrow().clone(), result.outcome),
        (
            vec!["Workspace".to_string()],
            if accepted { vec!["Workspace".to_string(), "Model".to_string()] } else { vec!["Workspace".to_string()] },
            if accepted { OnboardingOutcome::Completed } else { OnboardingOutcome::Skipped }
        )
    );
}

#[test]
fn waits_for_selection_acceptance_beyond_the_next_target_timeout_accepted_true() {
    run_with_faked_timers(waits_for_selection_acceptance(true));
}

#[test]
fn waits_for_selection_acceptance_beyond_the_next_target_timeout_accepted_false() {
    run_with_faked_timers(waits_for_selection_acceptance(false));
}

#[test]
fn resolves_the_step_variation_at_run_time() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        env.target("test.spotlight.resolvedTarget", OnboardingTargetOptions::default());
        let scenario = env.scenario_with_payload(
            "test.spotlight.resolvedSteps",
            SpotlightPayload {
                steps: Vec::new(),
                resolve_steps: Some(Rc::new(|| {
                    Promise::resolved(vec![step("resolved", "test.spotlight.resolvedTarget", "Resolved step", "Selected variation")])
                })),
            },
        );
        let result = settled(env.run(
            &scenario,
            &env.context_with_show({
                let env = env.clone();
                move || env.click_button(2)
            }),
        ))
        .await;
        assert_eq!(result, run_result(OnboardingOutcome::Completed, true, OnboardingDismissReason::Completed, 0.0, 1.0));
    });
}

#[test]
fn does_not_show_a_step_after_aborting_during_variation_resolution() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();
        let env = Env::new(&guard);
        let abort = guard.add_rc(Emitter::<()>::new());
        let steps = Rc::new(DeferredPromise::<Vec<SpotlightStep>>::new());
        let shown = counter();
        let scenario = env.scenario_with_payload(
            "test.spotlight.abortedResolution",
            SpotlightPayload {
                steps: Vec::new(),
                resolve_steps: Some(Rc::new({
                    let steps = steps.clone();
                    move || steps.promise.clone()
                })),
            },
        );
        let result = env.run(
            &scenario,
            &OnboardingRunContext {
                on_abort: abort.event(),
                on_did_show: Some(Rc::new({
                    let shown = shown.clone();
                    move || shown.set(shown.get() + 1)
                })),
                ..env.context()
            },
        );
        abort.fire(());
        let _ = steps.complete(Vec::new());
        assert_eq!(
            (settled(result).await, shown.get()),
            (run_result(OnboardingOutcome::Aborted, false, OnboardingDismissReason::Aborted, 0.0, 0.0), 0)
        );
    });
}
