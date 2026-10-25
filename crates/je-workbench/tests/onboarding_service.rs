//! Porte de `src/vs/workbench/contrib/onboarding/test/browser/onboardingService.test.ts` (suíte
//! `OnboardingScenarioService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()` e `teardown(() => Memento.clear(...))`.** O [`LeakGuard`] é criado no
//!   começo de cada caso (dentro do corpo assíncrono, que roda em `run_with_real_timers`), e `disposables.add` vira
//!   `guard.add` / `guard.add_rc`. O `teardown` que limpa o cache estático do `Memento` é o `Drop` de
//!   [`MementoTeardown`], criado antes do guarda para rodar depois da conferência de vazamentos.
//! - **`await timeout(0)`.** É `timeout(0).await` do ajudante comum; `await` de promessa é o `.await` do porte, e
//!   `Promise.all([...])` é [`Promise::all`].
//! - **`for (...) test(...)`.** Vira um teste por combinação, na ordem do original, sobre uma função comum.
//! - **`mainWindow`.** É a [`main_window`] do porte (sem DOM), a mesma que o serviço usa.
//! - **`createService(configValues, assignment, storage, telemetry)`.** Os argumentos opcionais são os campos de
//!   [`Options`]; a configuração é o `TestConfigurationService` e o `ContextKeyService` é o real.
//! - **`CapturingTelemetryService extends NullTelemetryServiceShape`.** O porte implementa o `ITelemetryService`
//!   inteiro (sem herança) e guarda `(nome, dados)` de cada `publicLog2`.
//! - **`FakeAssignmentService extends NullWorkbenchAssignmentService`.** É o dublê que devolve tratamentos prontos e
//!   guarda os filtros registrados. A subclasse anônima do caso "re-evaluates when delayed assignments resolve" (que
//!   espera `resolved.p` antes de ler o tratamento) é o campo `gate` do dublê.
//! - **`RecordingPresentation`.** O parâmetro `onRun` do construtor nunca é usado por nenhum caso e não foi portado.
//! - **Apresentações com `run` literal** (casos "joins an in-flight run" e "aborts an active run") são structs
//!   próprias, porque o `IOnboardingPresentation` é um trait; o corpo `async` é [`async_fn`].
//! - **`setUnexpectedErrorHandler(error => errors.push(error))`.** É [`set_unexpected_error_handler`], e restaurar o
//!   tratador original é [`record_unexpected_errors`] (o que o guarda de vazamentos instala).
//! - **`deepStrictEqual` de objetos com `undefined`** é [`assert_deep_strict_equal`] sobre [`Dynamic`]; nos demais casos
//!   é `assert_eq!` de tuplas.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, assert_deep_strict_equal, ensure_no_disposables_are_leaked_in_test_suite, record_unexpected_errors, run_with_real_timers, timeout};
use je_base::actions::ITelemetryData;
use je_base::async_::async_fn;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, set_unexpected_error_handler};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::observable::base::{IObservable, IReader, SettableExt};
use je_base::observable::reactions::autorun::autorun;
use je_base::observable::observable_value;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::promise::{DeferredPromise, Promise};
use je_platform::assignment::{IAssignmentService, TreatmentValue};
use je_platform::configuration::service::IConfigurationService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::{ContextKeyExpr, IContextKeyService};
use je_platform::contextkey::service::ContextKeyService;
use je_platform::storage::{InMemoryStorageService, StorageScope, StorageService};
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_platform::telemetry::{CommonPropertyValue, ITelemetryService, TelemetryLevel};
use je_workbench::common::memento::Memento;
use je_workbench::contrib::onboarding::guided_tryout_presentation::main_window;
use je_workbench::contrib::onboarding::onboarding_presentation::{IOnboardingPresentation, OnboardingRunContext, onboarding_presentation_registry};
use je_workbench::contrib::onboarding::onboarding_presentation_queue::run_with_onboarding_presentation;
use je_workbench::contrib::onboarding::onboarding_registry::onboarding_scenario_registry;
use je_workbench::contrib::onboarding::onboarding_scenario::{
    OnboardingDismissReason, OnboardingExperiment, OnboardingOutcome, OnboardingPresentationRef, OnboardingRunResult, OnboardingScenario,
    OnboardingTrigger,
};
use je_workbench::contrib::onboarding::onboarding_scenario_service::{
    IOnboardingScenarioService, ONBOARDING_DEVELOPER_MODE_CONFIG, ONBOARDING_DEVELOPER_MODE_VARIATIONS_CONFIG, ONBOARDING_ENABLED_CONFIG,
    get_onboarding_developer_mode_variation,
};
use je_workbench::contrib::onboarding::onboarding_service::OnboardingScenarioService;
use je_workbench::contrib::onboarding::onboarding_tryout::OnboardingTryoutMetadata;
use je_workbench::services::assignment::assignment_service::{IAssignmentFilter, ITreatmentWithAssignment, IWorkbenchAssignmentService};
use je_workbench::services::assignment::test::common::null_assignment_service::NullWorkbenchAssignmentService;
use je_workbench::services::lifecycle::lifecycle::ILifecycleService;
use je_workbench::test::common::workbench_test_services::TestLifecycleService;

/// O `teardown(() => Memento.clear(StorageScope.APPLICATION))` da suíte.
struct MementoTeardown;

impl Drop for MementoTeardown {
    fn drop(&mut self) {
        // The Memento maintains a static cache keyed by id; clear it so each test starts with fresh persisted state
        // instead of leaking across tests.
        Memento::clear(StorageScope::Application);
    }
}

/// Um caso assíncrono da suíte: o guarda de vazamentos e o `teardown` do memento em volta do corpo.
fn run_suite_test<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Rc<LeakGuard>) -> Fut + 'static) {
    run_with_real_timers(async move {
        let _memento_teardown = MementoTeardown;
        let guard = Rc::new(ensure_no_disposables_are_leaked_in_test_suite());
        body(guard.clone()).await;
    });
}

/// `await timeout(0)`.
async fn tick() {
    timeout(0).await.unwrap();
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

fn completed_result(outcome: OnboardingOutcome) -> OnboardingRunResult {
    let dismiss_reason = match outcome {
        OnboardingOutcome::Skipped => OnboardingDismissReason::SkipButton,
        OnboardingOutcome::Aborted => OnboardingDismissReason::Aborted,
        _ => OnboardingDismissReason::Completed,
    };
    OnboardingRunResult { outcome, shown: true, dismiss_reason, last_step_index: 0.0, step_count: 1.0 }
}

/// A result for a degenerate run that rendered nothing (no steps / all steps skipped).
fn not_shown_result() -> OnboardingRunResult {
    OnboardingRunResult {
        outcome: OnboardingOutcome::Completed,
        shown: false,
        dismiss_reason: OnboardingDismissReason::Completed,
        last_step_index: 0.0,
        step_count: 0.0,
    }
}

/// Captures the names of `publicLog2` telemetry events.
#[derive(Default)]
struct CapturingTelemetryService {
    event_data: RefCell<Vec<(String, Option<Dynamic>)>>,
}

impl CapturingTelemetryService {
    fn events(&self) -> Vec<String> {
        self.event_data.borrow().iter().map(|(name, _)| name.clone()).collect()
    }
}

impl ITelemetryService for CapturingTelemetryService {
    fn telemetry_level(&self) -> TelemetryLevel {
        TelemetryLevel::None
    }
    fn session_id(&self) -> String {
        "someValue.sessionId".to_string()
    }
    fn machine_id(&self) -> String {
        "someValue.machineId".to_string()
    }
    fn sqm_id(&self) -> String {
        "someValue.sqmId".to_string()
    }
    fn dev_device_id(&self) -> String {
        "someValue.devDeviceId".to_string()
    }
    fn first_session_date(&self) -> String {
        "someValue.firstSessionDate".to_string()
    }
    fn send_error_telemetry(&self) -> bool {
        false
    }
    fn public_log(&self, _event_name: &str, _data: Option<ITelemetryData>) {}
    fn public_log2(&self, event_name: &str, data: Option<Dynamic>) {
        if !event_name.is_empty() {
            self.event_data.borrow_mut().push((event_name.to_string(), data));
        }
    }
    fn public_log_error(&self, _error_event_name: &str, _data: Option<ITelemetryData>) {}
    fn public_log_error2(&self, _event_name: &str, _data: Option<Dynamic>) {}
    fn set_experiment_property(&self, _name: &str, _value: &str) {}
    fn set_common_property(&self, _name: &str, _value: Option<CommonPropertyValue>) {}
}

/// A presentation that resolves with a fixed run result.
struct FixedResultPresentation {
    kind: String,
    result: OnboardingRunResult,
}

impl IOnboardingPresentation for FixedResultPresentation {
    fn kind(&self) -> &str {
        &self.kind
    }

    fn run(&self, _scenario: &OnboardingScenario, _context: &OnboardingRunContext) -> Promise<OnboardingRunResult> {
        Promise::resolved(self.result)
    }
}

/// A presentation that reports its first rendered element before completing.
struct ShownPresentation {
    kind: String,
}

impl IOnboardingPresentation for ShownPresentation {
    fn kind(&self) -> &str {
        &self.kind
    }

    fn run(&self, _scenario: &OnboardingScenario, context: &OnboardingRunContext) -> Promise<OnboardingRunResult> {
        if let Some(on_did_show) = &context.on_did_show {
            on_did_show();
            on_did_show();
        }
        Promise::resolved(completed_result(OnboardingOutcome::Completed))
    }
}

/// Records every scenario it renders, then resolves with a fixed outcome.
struct RecordingPresentation {
    kind: String,
    outcome: OnboardingOutcome,
    runs: RefCell<Vec<String>>,
}

impl RecordingPresentation {
    fn new(kind: String, outcome: OnboardingOutcome) -> Rc<Self> {
        Rc::new(Self { kind, outcome, runs: RefCell::new(Vec::new()) })
    }

    fn runs(&self) -> Vec<String> {
        self.runs.borrow().clone()
    }
}

impl IOnboardingPresentation for RecordingPresentation {
    fn kind(&self) -> &str {
        &self.kind
    }

    fn run(&self, scenario: &OnboardingScenario, _context: &OnboardingRunContext) -> Promise<OnboardingRunResult> {
        self.runs.borrow_mut().push(scenario.id.clone());
        Promise::resolved(completed_result(self.outcome))
    }
}

/// Blocks until the engine aborts the run (used to test shutdown behaviour).
struct BlockingUntilAbortPresentation {
    kind: String,
    runs: RefCell<Vec<String>>,
}

impl BlockingUntilAbortPresentation {
    fn new(kind: String) -> Rc<Self> {
        Rc::new(Self { kind, runs: RefCell::new(Vec::new()) })
    }

    fn runs(&self) -> Vec<String> {
        self.runs.borrow().clone()
    }
}

impl IOnboardingPresentation for BlockingUntilAbortPresentation {
    fn kind(&self) -> &str {
        &self.kind
    }

    fn run(&self, scenario: &OnboardingScenario, context: &OnboardingRunContext) -> Promise<OnboardingRunResult> {
        self.runs.borrow_mut().push(scenario.id.clone());
        let on_abort = context.on_abort.clone();
        Promise::with_executor(move |resolver| {
            let listener: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
            let subscription = on_abort.listen({
                let listener = listener.clone();
                move |_| {
                    let current = listener.borrow_mut().take();
                    if let Some(current) = current {
                        current.dispose();
                    }
                    resolver.resolve(completed_result(OnboardingOutcome::Aborted));
                }
            });
            *listener.borrow_mut() = Some(subscription);
        })
    }
}

/// A apresentação literal do caso "runScenario joins an in-flight run instead of starting a second one".
struct GatedPresentation {
    kind: String,
    runs: Rc<RefCell<Vec<String>>>,
    gate: Promise<()>,
}

impl IOnboardingPresentation for GatedPresentation {
    fn kind(&self) -> &str {
        &self.kind
    }

    fn run(&self, scenario: &OnboardingScenario, _context: &OnboardingRunContext) -> Promise<OnboardingRunResult> {
        self.runs.borrow_mut().push(scenario.id.clone());
        let gate = self.gate.clone();
        async_fn(async move {
            gate.await?;
            Ok(completed_result(OnboardingOutcome::Completed))
        })
    }
}

/// A apresentação literal do caso "runScenario aborts an active run and waits for cleanup before resolving joined
/// callers".
struct CleanupPresentation {
    kind: String,
    started: Rc<DeferredPromise<()>>,
    finish: Rc<DeferredPromise<()>>,
    events: Rc<RefCell<Vec<String>>>,
}

impl IOnboardingPresentation for CleanupPresentation {
    fn kind(&self) -> &str {
        &self.kind
    }

    fn run(&self, _scenario: &OnboardingScenario, context: &OnboardingRunContext) -> Promise<OnboardingRunResult> {
        let (started, finish, events) = (self.started.clone(), self.finish.clone(), self.events.clone());
        let on_abort = context.on_abort.clone();
        async_fn(async move {
            let store = DisposableStore::new();
            store.add(on_abort.listen({
                let events = events.clone();
                move |_| events.borrow_mut().push("abort".to_string())
            }));
            started.complete(());
            let waited = finish.promise.clone().await;
            events.borrow_mut().push("cleanup".to_string());
            let aborted = events.borrow().iter().any(|event| event == "abort");
            store.dispose();
            waited?;
            Ok(completed_result(if aborted { OnboardingOutcome::Aborted } else { OnboardingOutcome::Completed }))
        })
    }
}

/// Assignment service test double that returns canned treatments and records the registered telemetry filter so tests
/// can assert which assignment-context ids would be excluded.
struct FakeAssignmentService {
    treatments: Vec<(String, TreatmentValue)>,
    /// Quando presente, `getTreatment` espera por ele antes de ler o tratamento.
    gate: Option<Promise<()>>,
    filters: RefCell<Vec<Rc<dyn IAssignmentFilter>>>,
}

impl FakeAssignmentService {
    fn new(treatments: &[(&str, TreatmentValue)]) -> Rc<Self> {
        Self::with_gate(treatments, None)
    }

    fn with_gate(treatments: &[(&str, TreatmentValue)], gate: Option<Promise<()>>) -> Rc<Self> {
        Rc::new(Self {
            treatments: treatments.iter().map(|(name, value)| (name.to_string(), value.clone())).collect(),
            gate,
            filters: RefCell::new(Vec::new()),
        })
    }

    /// True when the given assignment-context id is currently excluded from telemetry.
    fn is_excluded(&self, assignment: &str) -> bool {
        self.filters.borrow().iter().any(|filter| filter.exclude(assignment))
    }
}

impl IAssignmentService for FakeAssignmentService {
    fn on_did_refetch_assignments(&self) -> Event<()> {
        Event::none()
    }

    fn get_treatment(&self, name: &str) -> Promise<Option<TreatmentValue>> {
        let value = self.treatments.iter().find(|(treatment, _)| treatment == name).map(|(_, value)| value.clone());
        let gate = self.gate.clone();
        async_fn(async move {
            if let Some(gate) = gate {
                gate.await?;
            }
            Ok(value)
        })
    }
}

impl IWorkbenchAssignmentService for FakeAssignmentService {
    fn get_current_experiments(&self) -> Promise<Option<Vec<String>>> {
        async_fn(async { Ok(Some(Vec::new())) })
    }

    fn add_telemetry_assignment_filter(&self, filter: Rc<dyn IAssignmentFilter>) {
        self.filters.borrow_mut().push(filter);
    }

    fn get_treatment_with_assignment(&self, name: &str) -> Promise<ITreatmentWithAssignment> {
        let value = self.get_treatment(name);
        async_fn(async move { Ok(ITreatmentWithAssignment { value: value.await?, has_assignment: Promise::resolved(false) }) })
    }
}

fn boolean(value: bool) -> TreatmentValue {
    TreatmentValue::Boolean(value)
}

fn text(value: &str) -> TreatmentValue {
    TreatmentValue::String(value.to_string())
}

/// `new TestConfigurationService(values)`, com o emissor entregue ao guarda de vazamentos.
fn test_configuration(guard: &LeakGuard, values: Vec<(&str, Dynamic)>) -> Rc<TestConfigurationService> {
    let configuration = TestConfigurationService::new(Some(Dynamic::object(values)));
    guard.add_rc(configuration.on_did_change_configuration_emitter.clone());
    configuration
}

/// `{ [ONBOARDING_DEVELOPER_MODE_CONFIG]: { id: flag, ... } }`.
fn developer_mode(flags: &[(&str, bool)]) -> (&'static str, Dynamic) {
    (ONBOARDING_DEVELOPER_MODE_CONFIG, Dynamic::object(flags.iter().map(|(id, flag)| (*id, Dynamic::Bool(*flag)))))
}

/// `new InMemoryStorageService()` registrado no guarda.
fn new_storage(guard: &LeakGuard) -> Rc<dyn StorageService> {
    Rc::new(guard.add_rc(InMemoryStorageService::new()))
}

/// Os argumentos opcionais de `createService`.
#[derive(Default)]
struct Options {
    config: Vec<(&'static str, Dynamic)>,
    assignment: Option<Rc<dyn IWorkbenchAssignmentService>>,
    storage: Option<Rc<dyn StorageService>>,
    telemetry: Option<Rc<dyn ITelemetryService>>,
}

struct Fixture {
    service: Rc<OnboardingScenarioService>,
    context_key_service: Rc<ContextKeyService>,
    config: Rc<TestConfigurationService>,
    lifecycle: Rc<TestLifecycleService>,
}

fn create_service(guard: &LeakGuard, options: Options) -> Fixture {
    let storage = options.storage.unwrap_or_else(|| new_storage(guard));
    let config = test_configuration(guard, options.config);
    let context_key_service = guard.add_rc(ContextKeyService::new(config.clone()));
    let lifecycle = guard.add_rc(TestLifecycleService::new());
    let context_service: Rc<dyn IContextKeyService> = context_key_service.clone();
    let configuration_service: Rc<dyn IConfigurationService> = config.clone();
    let lifecycle_service: Rc<dyn ILifecycleService> = lifecycle.clone();
    let assignment = options.assignment.unwrap_or_else(|| Rc::new(NullWorkbenchAssignmentService));
    let telemetry = options.telemetry.unwrap_or_else(null_telemetry_service);
    let service = guard.add_rc(OnboardingScenarioService::new(storage, context_service, configuration_service, lifecycle_service, assignment, telemetry));
    Fixture { service, context_key_service, config, lifecycle }
}

fn register_presentation(guard: &LeakGuard, presentation: Rc<dyn IOnboardingPresentation>) {
    guard.add(onboarding_presentation_registry().register(presentation));
}

fn register_scenario(guard: &LeakGuard, scenario: OnboardingScenario) {
    guard.add(onboarding_scenario_registry().register(scenario));
}

thread_local! {
    static ID_SEED: Cell<usize> = const { Cell::new(0) };
}

fn unique_kind() -> String {
    ID_SEED.with(|seed| {
        let id = seed.get();
        seed.set(id + 1);
        format!("test-presentation-{id}")
    })
}

/// `{ id, trigger, presentation: { kind, payload: undefined } }`.
fn scenario(id: &str, trigger: OnboardingTrigger, kind: &str) -> OnboardingScenario {
    OnboardingScenario {
        id: id.to_string(),
        seen_key: None,
        when: None,
        trigger,
        priority: None,
        presentation: OnboardingPresentationRef { kind: kind.to_string(), payload: Dynamic::Undefined },
        tryout: None,
        experiment: None,
        developer_mode_variations: None,
        repeatable: None,
    }
}

fn auto(id: &str, kind: &str) -> OnboardingScenario {
    scenario(id, OnboardingTrigger::Auto, kind)
}

/// `{ kind: 'command', commandId: 'noop' }`.
fn command(id: &str, kind: &str) -> OnboardingScenario {
    scenario(id, OnboardingTrigger::Command { command_id: "noop".to_string() }, kind)
}

/// `observableValue(name, false)` como gatilho observável.
fn observable_trigger(name: &str) -> (OnboardingTrigger, Rc<ObservableValue<bool, ()>>) {
    let signal = observable_value::<bool, ()>(name, false);
    let observable: IObservable<bool> = signal.clone();
    (OnboardingTrigger::Observable { signal: observable }, signal)
}

/// `{ behaviorFlag: 'exp.show', assignmentContextIdFlag: 'exp.id' }`.
fn exp_config() -> Option<OnboardingExperiment> {
    Some(OnboardingExperiment { behavior_flag: "exp.show".to_string(), assignment_context_id_flag: "exp.id".to_string() })
}

fn event(name: &str, data: Dynamic) -> Dynamic {
    Dynamic::object([("name", Dynamic::string(name)), ("data", data)])
}

#[test]
fn developer_variation_only_overrides_while_developer_mode_is_enabled() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let disabled = test_configuration(
        &guard,
        vec![
            (ONBOARDING_DEVELOPER_MODE_CONFIG, Dynamic::object([("tour", Dynamic::Bool(false))])),
            (ONBOARDING_DEVELOPER_MODE_VARIATIONS_CONFIG, Dynamic::object([("tour", Dynamic::string("githubPrompt"))])),
        ],
    );
    let enabled = test_configuration(
        &guard,
        vec![
            (ONBOARDING_DEVELOPER_MODE_CONFIG, Dynamic::object([("tour", Dynamic::Bool(true))])),
            (ONBOARDING_DEVELOPER_MODE_VARIATIONS_CONFIG, Dynamic::object([("tour", Dynamic::string("githubPrompt"))])),
        ],
    );

    assert_eq!(
        (get_onboarding_developer_mode_variation(&*disabled, "tour"), get_onboarding_developer_mode_variation(&*enabled, "tour")),
        (None, Some("githubPrompt".to_string()))
    );
}

#[test]
fn runs_an_eligible_auto_scenario_exactly_once_and_marks_it_shown() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        register_scenario(&guard, auto("auto-1", &presentation.kind));

        let fixture = create_service(&guard, Options::default());
        fixture.service.start();
        tick().await;

        // Re-evaluating (e.g. another start) must not run it again.
        fixture.service.start();
        tick().await;

        assert_eq!((presentation.runs(), fixture.service.has_been_shown("auto-1")), (strings(&["auto-1"]), true));
    });
}

#[test]
fn developer_mode_ignores_previously_shown_state_for_auto_scenarios() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        register_scenario(&guard, auto("dev-repeat-1", &presentation.kind));

        let storage = new_storage(&guard);
        let first = create_service(&guard, Options { storage: Some(storage.clone()), ..Options::default() }).service;
        first.start();
        tick().await;

        let second = create_service(
            &guard,
            Options { config: vec![developer_mode(&[("dev-repeat-1", true)])], storage: Some(storage), ..Options::default() },
        );
        second.service.start();
        tick().await;

        second.context_key_service.create_key("onboardingTestDevModeReevaluate", Dynamic::Bool(false)).set(Dynamic::Bool(true));
        tick().await;

        assert_eq!(presentation.runs(), strings(&["dev-repeat-1", "dev-repeat-1"]));
    });
}

#[test]
fn does_not_run_automatically_when_onboarding_enabled_is_false() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        register_scenario(&guard, auto("disabled-1", &presentation.kind));

        let fixture = create_service(&guard, Options { config: vec![(ONBOARDING_ENABLED_CONFIG, Dynamic::Bool(false))], ..Options::default() });
        fixture.service.start();
        tick().await;

        assert_eq!(presentation.runs(), Vec::<String>::new());
    });
}

#[test]
fn respects_the_when_clause_and_reacts_to_context_changes() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        register_scenario(
            &guard,
            OnboardingScenario { when: Some(ContextKeyExpr::equals("onboardingTestReady", true)), ..auto("when-1", &presentation.kind) },
        );

        let fixture = create_service(&guard, Options::default());
        fixture.service.start();
        tick().await;
        assert_eq!(presentation.runs(), Vec::<String>::new(), "should not run while when is unsatisfied");

        let key = fixture.context_key_service.create_key("onboardingTestReady", Dynamic::Bool(false));
        key.set(Dynamic::Bool(true));
        tick().await;

        assert_eq!(presentation.runs(), strings(&["when-1"]));
    });
}

#[test]
fn runs_higher_priority_scenarios_before_lower_priority_ones() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        // Track ordering via the recorder's runs array which is appended in run().
        register_presentation(&guard, presentation.clone());
        register_scenario(&guard, OnboardingScenario { priority: Some(1.0), ..auto("low", &presentation.kind) });
        register_scenario(&guard, OnboardingScenario { priority: Some(10.0), ..auto("high", &presentation.kind) });

        let fixture = create_service(&guard, Options::default());
        fixture.service.start();
        tick().await;
        let order = presentation.runs();

        assert_eq!(order, strings(&["high", "low"]));
    });
}

#[test]
fn higher_priority_wins_when_eligible_scenarios_share_a_seen_key() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        register_scenario(&guard, OnboardingScenario { seen_key: Some("shared".to_string()), priority: Some(1.0), ..auto("low-shared", &presentation.kind) });
        register_scenario(&guard, OnboardingScenario { seen_key: Some("shared".to_string()), priority: Some(10.0), ..auto("high-shared", &presentation.kind) });

        let fixture = create_service(&guard, Options::default());
        fixture.service.start();
        tick().await;

        assert_eq!(presentation.runs(), strings(&["high-shared"]));
    });
}

#[test]
fn observable_triggers_start_the_scenario_when_the_signal_turns_true() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        let (trigger, signal) = observable_trigger("onboardingTestSignal");
        register_scenario(&guard, scenario("observable-1", trigger, &presentation.kind));

        let fixture = create_service(&guard, Options::default());
        fixture.service.start();
        tick().await;
        assert_eq!(presentation.runs(), Vec::<String>::new(), "should not run while signal is false");

        signal.set(true, None);
        tick().await;

        assert_eq!(presentation.runs(), strings(&["observable-1"]));
    });
}

#[test]
fn command_triggered_scenarios_never_run_automatically() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        register_scenario(&guard, command("command-1", &presentation.kind));

        let fixture = create_service(&guard, Options::default());
        fixture.service.start();
        tick().await;

        assert_eq!(presentation.runs(), Vec::<String>::new());
    });
}

#[test]
fn tryouts_do_not_run_automatically_show_nudges_or_run_through_the_legacy_replay_path() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        register_scenario(
            &guard,
            OnboardingScenario {
                tryout: Some(Rc::new(OnboardingTryoutMetadata {
                    title: "Example".to_string(),
                    description: "An explicitly launched example.".to_string(),
                    ..OnboardingTryoutMetadata::default()
                })),
                ..auto("guarded-tryout", &presentation.kind)
            },
        );
        let fixture = create_service(&guard, Options::default());
        fixture.service.start();
        tick().await;
        let outcome = fixture.service.run_scenario("guarded-tryout", CancellationToken::None).await.unwrap();

        assert_eq!(
            (presentation.runs(), outcome, fixture.service.has_been_shown("guarded-tryout"), fixture.service.should_show_nudge("guarded-tryout", None)),
            (Vec::<String>::new(), OnboardingOutcome::Aborted, false, false)
        );
    });
}

#[test]
fn run_scenario_runs_manually_even_when_disabled_and_already_shown() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        register_scenario(&guard, auto("manual-1", &presentation.kind));

        let fixture = create_service(&guard, Options { config: vec![(ONBOARDING_ENABLED_CONFIG, Dynamic::Bool(false))], ..Options::default() });
        fixture.service.start();
        tick().await;
        assert_eq!(presentation.runs(), Vec::<String>::new(), "disabled: should not auto-run");

        let outcome = fixture.service.run_scenario("manual-1", CancellationToken::None).await.unwrap();

        assert_eq!((presentation.runs(), outcome), (strings(&["manual-1"]), OnboardingOutcome::Completed));
    });
}

#[test]
fn run_scenario_joins_an_in_flight_run_instead_of_starting_a_second_one() {
    run_suite_test(|guard| async move {
        let gate = DeferredPromise::<()>::new();
        let kind = unique_kind();
        let runs: Rc<RefCell<Vec<String>>> = Rc::default();
        let presentation = Rc::new(GatedPresentation { kind: kind.clone(), runs: runs.clone(), gate: gate.promise.clone() });
        register_presentation(&guard, presentation);
        register_scenario(&guard, command("inflight-1", &kind));

        let fixture = create_service(&guard, Options::default());
        fixture.service.start();

        let first = fixture.service.run_scenario("inflight-1", CancellationToken::None);
        tick().await;
        // Second call while the first run is still in-flight must not start again.
        let second = fixture.service.run_scenario("inflight-1", CancellationToken::None);
        tick().await;

        gate.complete(());
        let outcomes = Promise::all(vec![first, second]).await.unwrap();

        assert_eq!(
            (runs.borrow().clone(), outcomes[0], outcomes[1]),
            (strings(&["inflight-1"]), OnboardingOutcome::Completed, OnboardingOutcome::Completed)
        );
    });
}

#[test]
fn run_scenario_does_not_enqueue_an_already_cancelled_request() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        register_scenario(&guard, command("cancelled", &presentation.kind));
        let fixture = create_service(&guard, Options::default());

        let outcome = fixture.service.run_scenario("cancelled", CancellationToken::Cancelled).await.unwrap();

        assert_eq!(
            (outcome, presentation.runs(), fixture.service.has_been_shown("cancelled")),
            (OnboardingOutcome::Aborted, Vec::<String>::new(), false)
        );
    });
}

#[test]
fn run_scenario_cancels_a_window_queued_presentation_without_affecting_its_current_occupant() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        register_scenario(&guard, command("waiting", &presentation.kind));
        let fixture = create_service(&guard, Options::default());
        let finish = DeferredPromise::<()>::new();
        let occupant = run_with_onboarding_presentation(&main_window(), &CancellationToken::None, {
            let finish = finish.promise.clone();
            move || finish
        });
        let cancellation = guard.add_rc(Rc::new(CancellationTokenSource::new(None)));
        let outcome: Rc<Cell<Option<OnboardingOutcome>>> = Rc::default();
        let waiting = fixture.service.run_scenario("waiting", cancellation.token()).then({
            let outcome = outcome.clone();
            move |result| outcome.set(Some(result))
        });
        tick().await;
        cancellation.cancel();
        tick().await;
        let before_release = (outcome.get(), presentation.runs(), fixture.service.has_been_shown("waiting"));
        finish.complete(());
        occupant.await.unwrap();
        waiting.await.unwrap();
        fixture.service.run_scenario("waiting", CancellationToken::None).await.unwrap();

        assert_eq!(
            (before_release, presentation.runs()),
            ((Some(OnboardingOutcome::Aborted), Vec::<String>::new(), false), strings(&["waiting"]))
        );
    });
}

#[test]
fn run_scenario_aborts_an_active_run_and_waits_for_cleanup_before_resolving_joined_callers() {
    run_suite_test(|guard| async move {
        let kind = unique_kind();
        let started = Rc::new(DeferredPromise::<()>::new());
        let finish = Rc::new(DeferredPromise::<()>::new());
        let events: Rc<RefCell<Vec<String>>> = Rc::default();
        register_presentation(
            &guard,
            Rc::new(CleanupPresentation { kind: kind.clone(), started: started.clone(), finish: finish.clone(), events: events.clone() }),
        );
        register_scenario(&guard, command("active-cancellation", &kind));
        let fixture = create_service(&guard, Options::default());
        let cancellation = guard.add_rc(Rc::new(CancellationTokenSource::new(None)));
        let running = fixture.service.run_scenario("active-cancellation", cancellation.token()).then({
            let events = events.clone();
            move |outcome| {
                events.borrow_mut().push("resolved".to_string());
                outcome
            }
        });
        started.promise.clone().await.unwrap();
        let joined = fixture.service.run_scenario("active-cancellation", CancellationToken::None);
        cancellation.cancel();
        tick().await;
        let during_cleanup = events.borrow().clone();
        finish.complete(());
        let outcomes = Promise::all(vec![running, joined]).await.unwrap();

        assert_eq!(
            (during_cleanup, events.borrow().clone(), outcomes),
            (strings(&["abort"]), strings(&["abort", "cleanup", "resolved"]), vec![OnboardingOutcome::Aborted, OnboardingOutcome::Aborted])
        );
    });
}

fn reset_clears_the_scenarios_shared_shown_state(developer_mode_enabled: bool) {
    run_suite_test(move |guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        let ids = ["reset-first", "reset-variation", "reset-unrelated"];
        for id in ids {
            register_scenario(
                &guard,
                OnboardingScenario {
                    seen_key: if id == "reset-unrelated" { None } else { Some("reset-shared".to_string()) },
                    ..command(id, &presentation.kind)
                },
            );
        }
        let flags: Vec<(&str, bool)> = ids.iter().map(|id| (*id, developer_mode_enabled)).collect();
        let fixture = create_service(&guard, Options { config: vec![developer_mode(&flags)], ..Options::default() });
        fixture.service.run_scenario("reset-first", CancellationToken::None).await.unwrap();
        fixture.service.run_scenario("reset-unrelated", CancellationToken::None).await.unwrap();

        let before: Vec<bool> = ids.iter().map(|id| fixture.service.has_been_shown(id)).collect();
        fixture.service.reset("reset-first");
        let after: Vec<bool> = ids.iter().map(|id| fixture.service.has_been_shown(id)).collect();
        assert_eq!((before, after), (vec![true, true, true], vec![false, false, true]));
    });
}

#[test]
fn reset_clears_the_scenarios_shared_shown_state_developer_mode_false() {
    reset_clears_the_scenarios_shared_shown_state(false);
}

#[test]
fn reset_clears_the_scenarios_shared_shown_state_developer_mode_true() {
    reset_clears_the_scenarios_shared_shown_state(true);
}

fn reset_all_clears_persisted_shown_state_without_allowing_developer_mode_replays(developer_mode_enabled: bool) {
    run_suite_test(move |guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        register_scenario(&guard, auto("reset-1", &presentation.kind));

        let fixture = create_service(&guard, Options { config: vec![developer_mode(&[("reset-1", developer_mode_enabled)])], ..Options::default() });
        fixture.service.start();
        tick().await;

        fixture.service.reset_all();
        assert_eq!(fixture.service.has_been_shown("reset-1"), developer_mode_enabled);
    });
}

#[test]
fn reset_all_clears_persisted_shown_state_without_allowing_developer_mode_replays_developer_mode_false() {
    reset_all_clears_persisted_shown_state_without_allowing_developer_mode_replays(false);
}

#[test]
fn reset_all_clears_persisted_shown_state_without_allowing_developer_mode_replays_developer_mode_true() {
    reset_all_clears_persisted_shown_state_without_allowing_developer_mode_replays(true);
}

#[test]
fn emits_scenario_outcome_telemetry_when_a_tour_is_shown_but_not_when_nothing_is_rendered() {
    run_suite_test(|guard| async move {
        let shown_kind = unique_kind();
        let not_shown_kind = unique_kind();
        register_presentation(&guard, Rc::new(FixedResultPresentation { kind: shown_kind.clone(), result: completed_result(OnboardingOutcome::Completed) }));
        register_presentation(&guard, Rc::new(FixedResultPresentation { kind: not_shown_kind.clone(), result: not_shown_result() }));
        register_scenario(&guard, auto("tele-shown", &shown_kind));
        register_scenario(&guard, auto("tele-notshown", &not_shown_kind));

        let telemetry = Rc::new(CapturingTelemetryService::default());
        let fixture = create_service(&guard, Options { telemetry: Some(telemetry.clone()), ..Options::default() });
        fixture.service.start();
        tick().await;
        tick().await;

        // One event for the shown tour; none for the degenerate run that rendered nothing.
        assert_eq!(telemetry.events(), strings(&["onboarding.scenarioOutcome"]));
    });
}

#[test]
fn emits_one_shown_event_only_after_a_presentation_renders_with_its_experiment_assignment() {
    run_suite_test(|guard| async move {
        let presentation = Rc::new(ShownPresentation { kind: unique_kind() });
        register_presentation(&guard, presentation.clone());
        register_scenario(
            &guard,
            OnboardingScenario { experiment: Some(OnboardingExperiment {
                behavior_flag: "onb.newSessionViewV2.show".to_string(),
                assignment_context_id_flag: "onb.newSessionViewV2.id".to_string(),
            }), ..auto("sessions.onboarding.newSessionViewV2", &presentation.kind) },
        );
        let telemetry = Rc::new(CapturingTelemetryService::default());
        let assignment = FakeAssignmentService::new(&[("onb.newSessionViewV2.show", boolean(true)), ("onb.newSessionViewV2.id", text("onb-new-btn-treat2"))]);
        let fixture = create_service(&guard, Options { assignment: Some(assignment), telemetry: Some(telemetry.clone()), ..Options::default() });

        fixture.service.start();
        tick().await;
        tick().await;

        let shown_events: Vec<Dynamic> = telemetry
            .event_data
            .borrow()
            .iter()
            .filter(|(name, _)| name == "onboarding.scenarioShown")
            .map(|(name, data)| event(name, data.clone().unwrap_or(Dynamic::Undefined)))
            .collect();
        assert_deep_strict_equal(
            &Dynamic::array(shown_events),
            &Dynamic::array([event(
                "onboarding.scenarioShown",
                Dynamic::object([
                    ("scenarioId", Dynamic::string("sessions.onboarding.newSessionViewV2")),
                    ("experimentActive", Dynamic::Bool(true)),
                    ("experimentAssignmentContextId", Dynamic::string("onb-new-btn-treat2")),
                ]),
            )]),
        );
        assert_eq!(telemetry.events(), strings(&["onboarding.scenarioShown", "onboarding.scenarioOutcome"]));
    });
}

#[test]
fn experiment_driven_scenario_does_not_run_unless_both_treatment_flags_are_set() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        register_scenario(&guard, OnboardingScenario { experiment: exp_config(), ..auto("exp-off", &presentation.kind) });

        // Only one of the two flags resolves -> treated as not configured -> does not run.
        let fixture = create_service(&guard, Options { assignment: Some(FakeAssignmentService::new(&[("exp.show", boolean(true))])), ..Options::default() });
        fixture.service.start();
        tick().await;
        tick().await;

        assert_eq!(presentation.runs(), Vec::<String>::new());
    });
}

fn pre_tour_nudge_respects(show: Option<bool>, id: Option<&'static str>, developer_mode_enabled: bool, enabled: bool, expected: bool) {
    run_suite_test(move |guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        let mut flags: Vec<(&str, TreatmentValue)> = Vec::new();
        if let Some(show) = show {
            flags.push(("exp.show", boolean(show)));
        }
        if let Some(id) = id {
            flags.push(("exp.id", text(id)));
        }
        let assignment = FakeAssignmentService::new(&flags);
        let (trigger, _signal) = observable_trigger("trigger");
        register_scenario(&guard, OnboardingScenario { experiment: exp_config(), ..scenario("nudge", trigger, &presentation.kind) });
        let fixture = create_service(
            &guard,
            Options {
                config: vec![(ONBOARDING_ENABLED_CONFIG, Dynamic::Bool(enabled)), developer_mode(&[("nudge", developer_mode_enabled)])],
                assignment: Some(assignment.clone()),
                ..Options::default()
            },
        );

        fixture.service.start();
        let before_resolution = fixture.service.should_show_nudge("nudge", None);
        tick().await;
        let excluded_before_nudge = assignment.is_excluded("onb-nudge:12345");
        let show_nudge = fixture.service.should_show_nudge("nudge", None);

        assert_eq!(
            (
                before_resolution,
                excluded_before_nudge,
                show_nudge,
                assignment.is_excluded("onb-nudge:12345"),
                fixture.service.has_been_shown("nudge"),
                presentation.runs(),
            ),
            (
                developer_mode_enabled && enabled,
                true,
                expected,
                !(enabled && !developer_mode_enabled && show.is_some() && id.is_some_and(|id| !id.is_empty())),
                false,
                Vec::<String>::new(),
            )
        );
    });
}

#[test]
fn pre_tour_nudge_respects_inactive_experiment() {
    pre_tour_nudge_respects(None, None, false, true, false);
}

#[test]
fn pre_tour_nudge_respects_missing_assignment_id() {
    pre_tour_nudge_respects(Some(true), None, false, true, false);
}

#[test]
fn pre_tour_nudge_respects_missing_behavior() {
    pre_tour_nudge_respects(None, Some("onb-nudge"), false, true, false);
}

#[test]
fn pre_tour_nudge_respects_control() {
    pre_tour_nudge_respects(Some(false), Some("onb-nudge"), false, true, false);
}

#[test]
fn pre_tour_nudge_respects_treatment() {
    pre_tour_nudge_respects(Some(true), Some("onb-nudge"), false, true, true);
}

#[test]
fn pre_tour_nudge_respects_developer_preview() {
    pre_tour_nudge_respects(None, None, true, true, true);
}

#[test]
fn pre_tour_nudge_respects_disabled_treatment() {
    pre_tour_nudge_respects(Some(true), Some("onb-nudge"), false, false, false);
}

#[test]
fn pre_tour_nudge_respects_disabled_developer_preview() {
    pre_tour_nudge_respects(None, None, true, false, false);
}

fn pre_tour_nudge_re_evaluates_when_delayed_assignments_resolve(behavior: bool) {
    run_suite_test(move |guard| async move {
        let resolved = DeferredPromise::<()>::new();
        let assignment = FakeAssignmentService::with_gate(&[("exp.show", boolean(behavior)), ("exp.id", text("onb-delayed"))], Some(resolved.promise.clone()));
        let (trigger, _signal) = observable_trigger("trigger");
        register_scenario(&guard, OnboardingScenario { experiment: exp_config(), ..scenario("delayed-nudge", trigger, &unique_kind()) });
        let fixture = create_service(&guard, Options { assignment: Some(assignment.clone()), ..Options::default() });
        fixture.service.start();
        let eligibility: Rc<RefCell<Vec<bool>>> = Rc::default();
        guard.add(autorun({
            let (service, eligibility) = (fixture.service.clone(), eligibility.clone());
            move |reader| {
                let reader: &dyn IReader = reader;
                eligibility.borrow_mut().push(service.should_show_nudge("delayed-nudge", Some(reader)));
            }
        }));
        resolved.complete(()).await.unwrap();
        tick().await;

        assert_eq!(
            (eligibility.borrow().clone(), assignment.is_excluded("onb-delayed:12345"), fixture.service.has_been_shown("delayed-nudge")),
            (vec![false, behavior], false, false)
        );
    });
}

#[test]
fn pre_tour_nudge_re_evaluates_when_delayed_assignments_resolve_false() {
    pre_tour_nudge_re_evaluates_when_delayed_assignments_resolve(false);
}

#[test]
fn pre_tour_nudge_re_evaluates_when_delayed_assignments_resolve_true() {
    pre_tour_nudge_re_evaluates_when_delayed_assignments_resolve(true);
}

#[test]
fn pre_tour_nudge_respects_context_shown_state_and_the_global_switch() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        let (trigger, _signal) = observable_trigger("trigger");
        register_scenario(&guard, OnboardingScenario { when: Some(ContextKeyExpr::has("nudgeReady")), ..scenario("nudge", trigger, &presentation.kind) });
        let fixture = create_service(&guard, Options::default());
        let ready = fixture.context_key_service.create_key("nudgeReady", Dynamic::Bool(false));
        let context_blocked = fixture.service.should_show_nudge("nudge", None);
        ready.set(Dynamic::Bool(true));
        let eligible = fixture.service.should_show_nudge("nudge", None);
        fixture.config.set_user_configuration(ONBOARDING_ENABLED_CONFIG, Dynamic::Bool(false), None).await.unwrap();
        let disabled = fixture.service.should_show_nudge("nudge", None);
        fixture.config.set_user_configuration(ONBOARDING_ENABLED_CONFIG, Dynamic::Bool(true), None).await.unwrap();
        fixture.service.run_scenario("nudge", CancellationToken::None).await.unwrap();

        assert_eq!(
            (context_blocked, eligible, disabled, fixture.service.should_show_nudge("nudge", None)),
            (false, true, false, false)
        );
    });
}

#[test]
fn an_assignment_context_id_without_the_reserved_prefix_is_rejected_as_inactive() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        // Misconfigured id: would never be gated by the prefix filter, so it must not run.
        let assignment = FakeAssignmentService::new(&[("exp.show", boolean(true)), ("exp.id", text("newsession-2026q3"))]);
        register_scenario(&guard, OnboardingScenario { experiment: exp_config(), ..auto("exp-badid", &presentation.kind) });

        let errors: Rc<RefCell<Vec<Error>>> = Rc::default();
        set_unexpected_error_handler({
            let errors = errors.clone();
            move |error| errors.borrow_mut().push(error.clone())
        });
        let fixture = create_service(&guard, Options { assignment: Some(assignment), ..Options::default() });
        fixture.service.start();
        tick().await;
        tick().await;

        let result = (
            presentation.runs(),
            fixture.service.has_been_shown("exp-badid"),
            fixture.service.should_show_nudge("exp-badid", None),
            errors.borrow().len() == 1,
        );
        record_unexpected_errors();
        assert_eq!(result, (Vec::<String>::new(), false, false, true));
    });
}

#[test]
fn treatment_arm_shows_the_tour_and_opens_the_telemetry_gate() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        let assignment = FakeAssignmentService::new(&[("exp.show", boolean(true)), ("exp.id", text("onb-tour-q3"))]);
        register_scenario(&guard, OnboardingScenario { experiment: exp_config(), ..auto("exp-treat", &presentation.kind) });

        let assignment_context = "onb-tour-q3:12345";
        let fixture = create_service(&guard, Options { assignment: Some(assignment.clone()), ..Options::default() });
        let excluded_before_would_show = assignment.is_excluded(assignment_context);

        fixture.service.start();
        tick().await;
        tick().await;

        assert_eq!(
            (
                excluded_before_would_show,
                presentation.runs(),
                fixture.service.has_been_shown("exp-treat"),
                assignment.is_excluded(assignment_context),
                assignment.is_excluded("onb-tour-q3-other:12346"),
            ),
            (true, strings(&["exp-treat"]), true, false, true)
        );
    });
}

#[test]
fn control_arm_opens_the_gate_but_shows_nothing_and_stays_eligible() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        let assignment = FakeAssignmentService::new(&[("exp.show", boolean(false)), ("exp.id", text("onb-tour-q3"))]);
        register_scenario(&guard, OnboardingScenario { experiment: exp_config(), ..auto("exp-control", &presentation.kind) });

        let fixture = create_service(&guard, Options { assignment: Some(assignment.clone()), ..Options::default() });
        fixture.service.start();
        tick().await;
        tick().await;

        // No tour shown, not marked shown (re-eligible later), but the id now flows.
        assert_eq!(
            (presentation.runs(), fixture.service.has_been_shown("exp-control"), assignment.is_excluded("onb-tour-q3:12345")),
            (Vec::<String>::new(), false, false)
        );
    });
}

#[test]
fn developer_mode_shows_an_experiment_scenario_whose_experiment_is_not_active() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        // Neither treatment flag resolves: the experiment is inactive, so it would not run automatically, but developer
        // mode bypasses the experiment gate.
        let assignment = FakeAssignmentService::new(&[]);
        register_scenario(&guard, OnboardingScenario { experiment: exp_config(), ..auto("exp-dev-inactive", &presentation.kind) });

        let fixture = create_service(
            &guard,
            Options { config: vec![developer_mode(&[("exp-dev-inactive", true)])], assignment: Some(assignment.clone()), ..Options::default() },
        );
        fixture.service.start();
        tick().await;
        tick().await;

        // The tour is shown, but since the experiment is not active no telemetry gate is opened (there is no
        // assignment-context id to flow).
        assert_eq!((presentation.runs(), assignment.is_excluded("onb-tour-q3")), (strings(&["exp-dev-inactive"]), true));
    });
}

#[test]
fn developer_mode_shows_the_tour_even_when_the_user_is_in_the_control_arm() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        let assignment = FakeAssignmentService::new(&[("exp.show", boolean(false)), ("exp.id", text("onb-tour-q3"))]);
        register_scenario(&guard, OnboardingScenario { experiment: exp_config(), ..auto("exp-dev-control", &presentation.kind) });

        let fixture = create_service(
            &guard,
            Options { config: vec![developer_mode(&[("exp-dev-control", true)])], assignment: Some(assignment.clone()), ..Options::default() },
        );
        fixture.service.start();
        tick().await;
        tick().await;

        // Developer mode shows the tour unconditionally and never opens the telemetry gate, so the assignment-context
        // id stays excluded even though the user is in the (active) control arm: a local preview can't affect the
        // scorecard.
        assert_eq!((presentation.runs(), assignment.is_excluded("onb-tour-q3")), (strings(&["exp-dev-control"]), true));
    });
}

#[test]
fn an_opened_gate_persists_so_the_id_keeps_flowing_after_a_reload() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        let storage = new_storage(&guard);
        register_scenario(&guard, OnboardingScenario { experiment: exp_config(), ..auto("exp-persist", &presentation.kind) });

        // First "session": control reaches would-show and opens the gate.
        let first = create_service(
            &guard,
            Options {
                assignment: Some(FakeAssignmentService::new(&[("exp.show", boolean(false)), ("exp.id", text("onb-tour-q3"))])),
                storage: Some(storage.clone()),
                ..Options::default()
            },
        );
        first.service.start();
        tick().await;
        tick().await;

        // Second "session" (reload) with a fresh service + assignment service: the persisted gate must immediately
        // allow the id, even before any would-show this session.
        let second_assignment = FakeAssignmentService::new(&[("exp.show", boolean(false)), ("exp.id", text("onb-tour-q3"))]);
        create_service(&guard, Options { assignment: Some(second_assignment.clone()), storage: Some(storage), ..Options::default() });

        assert!(!second_assignment.is_excluded("onb-tour-q3:12345"));
    });
}

#[test]
fn a_second_experiment_with_a_new_id_is_blocked_for_a_user_who_already_saw_the_tour() {
    run_suite_test(|guard| async move {
        let presentation = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, presentation.clone());
        let storage = new_storage(&guard);

        // Experiment 1: treatment. The user sees the tour and is marked shown.
        register_scenario(&guard, OnboardingScenario { experiment: exp_config(), ..auto("tour", &presentation.kind) });
        let first = create_service(
            &guard,
            Options {
                assignment: Some(FakeAssignmentService::new(&[("exp.show", boolean(true)), ("exp.id", text("onb-tour-2026q3"))])),
                storage: Some(storage.clone()),
                ..Options::default()
            },
        );
        first.service.start();
        tick().await;
        tick().await;
        assert!(first.service.has_been_shown("tour"));

        // Experiment 2 (new id) in a reload: already shown -> not eligible -> id stays blocked.
        let second_assignment = FakeAssignmentService::new(&[("exp.show", boolean(true)), ("exp.id", text("onb-tour-2027q1"))]);
        let second = create_service(&guard, Options { assignment: Some(second_assignment.clone()), storage: Some(storage), ..Options::default() });
        second.service.start();
        tick().await;
        tick().await;

        assert_eq!(
            (second.service.has_been_shown("tour"), second_assignment.is_excluded("onb-tour-2027q1")),
            (true, true)
        );
    });
}

#[test]
fn shutdown_aborts_the_active_scenario_and_never_starts_queued_ones() {
    run_suite_test(|guard| async move {
        let active = BlockingUntilAbortPresentation::new(unique_kind());
        let queued = RecordingPresentation::new(unique_kind(), OnboardingOutcome::Completed);
        register_presentation(&guard, active.clone());
        register_presentation(&guard, queued.clone());
        // `active` has higher priority so it runs first and blocks; `queued` waits.
        register_scenario(&guard, OnboardingScenario { priority: Some(10.0), ..auto("active", &active.kind) });
        register_scenario(&guard, OnboardingScenario { priority: Some(1.0), ..auto("queued", &queued.kind) });

        let fixture = create_service(&guard, Options::default());
        fixture.service.start();
        tick().await;
        // Only the active (blocking) scenario should have started.
        assert_eq!((active.runs(), queued.runs()), (strings(&["active"]), Vec::<String>::new()));

        fixture.lifecycle.fire_shutdown(None);
        tick().await;

        // The queued scenario must never have been presented.
        assert_eq!((active.runs(), queued.runs()), (strings(&["active"]), Vec::<String>::new()));
    });
}

#[test]
fn service_starts_and_disposes_without_leaking() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let _memento_teardown = MementoTeardown;
    let store = DisposableStore::new();
    let storage: Rc<dyn StorageService> = Rc::new(store.add_rc(InMemoryStorageService::new()));
    let config = TestConfigurationService::new(None);
    store.add_rc(config.on_did_change_configuration_emitter.clone());
    let context_key_service = store.add_rc(ContextKeyService::new(config.clone()));
    let lifecycle = store.add_rc(TestLifecycleService::new());
    let service = store.add_rc(OnboardingScenarioService::new(
        storage,
        context_key_service,
        config,
        lifecycle,
        Rc::new(NullWorkbenchAssignmentService),
        null_telemetry_service(),
    ));
    service.start();
    store.dispose();
    drop(guard);
}
