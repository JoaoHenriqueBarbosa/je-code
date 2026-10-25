//! Porte de `src/vs/workbench/contrib/onboarding/browser/onboardingService.ts`: o motor de onboarding independente de
//! apresentação. Decide quando e se um cenário roda (elegibilidade, fila por prioridade, estado "mostrado uma vez" no
//! memento, experimentos e o portão de telemetria do contexto de atribuição) e delega o desenho à apresentação
//! registrada.
//!
//! Divergências:
//! - **Injeção.** Os `@IService` do construtor são os parâmetros de [`OnboardingScenarioService::new`], que devolve o
//!   `Rc<Self>` (o serviço se alcança por `Weak` nos ouvintes e nas tarefas assíncronas). `extends Disposable` é o
//!   [`DisposableStore`] guardado em `store`.
//! - **`mainWindow`.** É a [`main_window`] do porte (a [`OnboardingWindow`] por thread que o `GuidedTryoutPresentation`
//!   também usa), porque o porte não tem DOM.
//! - **`Memento<OnboardingMementoData>` e `Partial<OnboardingMementoData>`.** O memento é o [`ObjectRef`] compartilhado
//!   do porte, e cada `IScenarioState` é um objeto `{ shownAt, outcome, seenCount }` dentro dele. Ler `shownAt` e
//!   `outcome` e gravar o objeto novo de `_markShown` seguem o original campo a campo; `outcome: undefined` entra como
//!   [`Dynamic::Undefined`] (o `to_json` do memento o omite, como o `JSON.stringify`).
//! - **`_openedAssignmentContextIds: Set<string>`.** É um `Vec<String>` sem repetição (a ordem de inserção do `Set`,
//!   que `Array.from` preserva ao gravar) compartilhado com o filtro de telemetria por `Rc<RefCell<..>>`.
//!   `addTelemetryAssignmentFilter({ id, exclude, onDidChange })` é o [`OnboardingTelemetryFilter`], que implementa o
//!   [`IAssignmentFilter`].
//! - **`observableSignal(this)`.** É o `observable_signal` nomeado (o nome só serve à depuração).
//! - **`async`.** `runScenario`, `_doPump`, `_runPresentation` e `_showPresentation` são [`async_fn`]: o corpo roda
//!   já, de forma síncrona, até o primeiro `await`, e todo `await` cede uma microtarefa, como no original. O
//!   `try/catch/finally` de cada um é a sequência explícita depois do `await` (o `finally` roda em todos os caminhos
//!   porque nenhum passo entre o `await` e a limpeza pode lançar).
//! - **`await Promise.resolve()` de `_doPump`.** É `Promise::resolved(()).await`, que também cede uma microtarefa, para
//!   que um lote de `_enqueue` do mesmo tick caia na fila e seja reordenado por prioridade antes de consumi-la.
//! - **`Promise.all([...]).then(ok, erro)`.** É [`Promise::all`] com [`Promise::then_result`], tratando as duas saídas.
//! - **`sort`.** O `Array.prototype.sort` do JavaScript é estável, como o `sort_by` do `Vec`; o `NaN` do comparador
//!   conta como empate.
//! - **`type ...Event` e `type ...Classification`.** São só tipos do compilador de telemetria e não têm equivalente em
//!   tempo de execução; os eventos são [`Dynamic::object`] com as chaves na ordem do original.
//! - **`shouldShowNudge` lança** de forma síncrona ([`errors::throw`]) para cenário desconhecido, e `runScenario`
//!   rejeita a promessa.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::async_fn;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{Dynamic, ObjectRef, Truthy, json_quote};
use je_base::errors::{self, Error, is_cancellation_error, on_unexpected_error};
use je_base::event::{Emitter, Event};
use je_base::jsonc::json_parse;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::observable::base::{IReader, Observable};
use je_base::observable::observables::observable_signal::{ObservableSignal, observable_signal};
use je_base::observable::reactions::autorun::autorun;
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime;
use je_platform::assignment::TreatmentValue;
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkey::IContextKeyService;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};
use je_platform::telemetry::ITelemetryService;
use je_storage::StorageValue;

use super::guided_tryout_presentation::main_window;
use super::onboarding_presentation::{IOnboardingPresentation, OnboardingRunContext, onboarding_presentation_registry};
use super::onboarding_presentation_queue::run_with_onboarding_presentation;
use super::onboarding_registry::onboarding_scenario_registry;
use super::onboarding_scenario::{
    ONBOARDING_ASSIGNMENT_CONTEXT_PREFIX, OnboardingOutcome, OnboardingRunResult, OnboardingScenario, OnboardingTrigger,
};
use super::onboarding_scenario_service::{
    IOnboardingScenarioService, ONBOARDING_DEVELOPER_MODE_CONFIG, ONBOARDING_ENABLED_CONFIG, is_onboarding_developer_mode_enabled,
};
use crate::common::memento::Memento;
use crate::services::assignment::assignment_service::{IAssignmentFilter, IWorkbenchAssignmentService};
use crate::services::lifecycle::lifecycle::ILifecycleService;

const MEMENTO_ID: &str = "onboarding";

/// Storage key for the set of assignment-context identifiers whose telemetry gate has been opened (the user reached the
/// onboarding moment). Persisted so the identifier keeps flowing across reloads/relaunches until the experiment is
/// stopped.
const OPENED_IDS_STORAGE_KEY: &str = "onboarding.openedAssignmentContextIds";

/// `IExperimentState`: resolved experiment treatment state for a scenario that declares an experiment.
#[derive(Clone)]
struct ExperimentState {
    /// Both treatment flags resolved (the experiment is configured for this user).
    active: bool,
    /// Value of the boolean behavior flag: `true` shows the tour (treatment), `false` is control.
    behavior: bool,
    /// Value of the assignment-context id flag (the id the scorecard keys on).
    assignment_context_id: String,
}

/// A entrada da fila: o cenário e a promessa do desfecho dele.
#[derive(Clone)]
struct QueueEntry {
    scenario: Rc<OnboardingScenario>,
    deferred: Rc<DeferredPromise<OnboardingOutcome>>,
}

/// O filtro de telemetria registrado no construtor: bloqueia todo id com o prefixo reservado de onboarding cujo portão
/// ainda não foi aberto (nesta sessão ou numa anterior).
struct OnboardingTelemetryFilter {
    opened_assignment_context_ids: Rc<RefCell<Vec<String>>>,
    on_did_change: Event<()>,
}

impl IAssignmentFilter for OnboardingTelemetryFilter {
    fn id(&self) -> &str {
        "onboarding"
    }

    fn exclude(&self, assignment: &str) -> bool {
        let variant = get_assignment_context_variant(assignment);
        variant.starts_with(ONBOARDING_ASSIGNMENT_CONTEXT_PREFIX) && !self.opened_assignment_context_ids.borrow().iter().any(|opened| opened == variant)
    }

    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.clone()
    }
}

/// `getAssignmentContextVariant`.
fn get_assignment_context_variant(assignment: &str) -> &str {
    match assignment.rfind(':') {
        Some(separator_index) => &assignment[..separator_index],
        None => assignment,
    }
}

/// `scenario.seenKey ?? scenario.id`: a chave sob a qual o estado "mostrado uma vez" de um cenário é guardado. Cenários
/// podem adotar uma `seen_key` compartilhada para que variações do mesmo onboarding sejam controladas juntas.
fn seen_key(scenario: &OnboardingScenario) -> String {
    scenario.seen_key.clone().unwrap_or_else(|| scenario.id.clone())
}

/// `scenario.priority ?? 0`.
fn priority_of(scenario: &OnboardingScenario) -> f64 {
    scenario.priority.unwrap_or(0.0)
}

/// `(b.priority ?? 0) - (a.priority ?? 0)`: a maior prioridade primeiro; o `NaN` conta como empate.
fn by_priority_descending(a: &OnboardingScenario, b: &OnboardingScenario) -> Ordering {
    priority_of(b).partial_cmp(&priority_of(a)).unwrap_or(Ordering::Equal)
}

/// `_loadOpenedIds`.
fn load_opened_ids(storage_service: &dyn StorageService) -> Vec<String> {
    let Some(raw) = storage_service.get(OPENED_IDS_STORAGE_KEY, StorageScope::Application, None).filter(|raw| !raw.is_empty()) else {
        return Vec::new();
    };
    let units: Vec<u16> = raw.encode_utf16().collect();
    match json_parse(&units) {
        Ok(parsed) => match Dynamic::from_json(&parsed) {
            Dynamic::Array(items) => {
                let mut ids: Vec<String> = Vec::new();
                for item in items.to_vec() {
                    if let Dynamic::String(id) = item {
                        let id = id.to_string_lossy();
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                }
                ids
            }
            _ => Vec::new(),
        },
        Err(error) => {
            on_unexpected_error(&Error::new(error.to_string()));
            Vec::new()
        }
    }
}

/// `OnboardingScenarioService`.
pub struct OnboardingScenarioService {
    store: Rc<DisposableStore>,
    me: Weak<OnboardingScenarioService>,
    storage_service: Rc<dyn StorageService>,
    context_key_service: Rc<dyn IContextKeyService>,
    configuration_service: Rc<dyn IConfigurationService>,
    assignment_service: Rc<dyn IWorkbenchAssignmentService>,
    telemetry_service: Rc<dyn ITelemetryService>,
    memento: Memento,
    state: ObjectRef,

    /// Listeners for `observable` triggers, rebuilt whenever the registry changes.
    trigger_listeners: Rc<DisposableStore>,

    /// Scenario ids currently queued or running (prevents double-scheduling).
    pending: RefCell<HashSet<String>>,
    queue: RefCell<Vec<QueueEntry>>,
    /// Deferreds for scenarios that have been dequeued and are currently running, keyed by id.
    inflight: RefCell<HashMap<String, Rc<DeferredPromise<OnboardingOutcome>>>>,
    pumping: Cell<bool>,

    /// Abort signal for the scenario currently running.
    active_abort: RefCell<Option<Rc<Emitter<()>>>>,

    /// Resolved experiment treatment state, keyed by scenario id.
    experiment_states: RefCell<HashMap<String, ExperimentState>>,
    experiment_states_changed: Rc<ObservableSignal<()>>,

    /// Assignment-context ids whose telemetry gate is open. While an onboarding id is *not* in this set, the
    /// eagerly-registered filter excludes it from telemetry (see the prefix constant). The set is seeded from storage
    /// and grows as users reach the onboarding moment.
    opened_assignment_context_ids: Rc<RefCell<Vec<String>>>,
    on_did_change_opened_ids: Rc<Emitter<()>>,

    started: Cell<bool>,
    stopped: Cell<bool>,
    shown_since_start: RefCell<HashSet<String>>,
}

impl OnboardingScenarioService {
    pub fn new(
        storage_service: Rc<dyn StorageService>,
        context_key_service: Rc<dyn IContextKeyService>,
        configuration_service: Rc<dyn IConfigurationService>,
        lifecycle_service: Rc<dyn ILifecycleService>,
        assignment_service: Rc<dyn IWorkbenchAssignmentService>,
        telemetry_service: Rc<dyn ITelemetryService>,
    ) -> Rc<Self> {
        let store = DisposableStore::new();
        let trigger_listeners = store.add_rc(DisposableStore::new());
        let on_did_change_opened_ids = store.add_rc(Emitter::<()>::new());

        let memento = Memento::new(MEMENTO_ID, storage_service.clone());
        let state = memento.get_memento(StorageScope::Application, StorageTarget::Machine);

        let opened_assignment_context_ids = Rc::new(RefCell::new(load_opened_ids(storage_service.as_ref())));

        // Register the telemetry gate filter eagerly (in the constructor) so onboarding assignment-context ids are
        // excluded from the very first event, before the treatment flags resolve. The filter blocks any id with the
        // reserved onboarding prefix unless its gate has already been opened (this session or persisted from a
        // previous one).
        assignment_service.add_telemetry_assignment_filter(Rc::new(OnboardingTelemetryFilter {
            opened_assignment_context_ids: opened_assignment_context_ids.clone(),
            on_did_change: on_did_change_opened_ids.event(),
        }));

        let this = Rc::new_cyclic(|me| Self {
            store: store.clone(),
            me: me.clone(),
            storage_service,
            context_key_service,
            configuration_service,
            assignment_service,
            telemetry_service,
            memento,
            state,
            trigger_listeners,
            pending: RefCell::new(HashSet::new()),
            queue: RefCell::new(Vec::new()),
            inflight: RefCell::new(HashMap::new()),
            pumping: Cell::new(false),
            active_abort: RefCell::new(None),
            experiment_states: RefCell::new(HashMap::new()),
            experiment_states_changed: observable_signal("experimentStatesChanged"),
            opened_assignment_context_ids,
            on_did_change_opened_ids,
            started: Cell::new(false),
            stopped: Cell::new(false),
            shown_since_start: RefCell::new(HashSet::new()),
        });

        // On shutdown abort the active run and drain anything still queued so no fresh overlay is mounted while the
        // window is going away.
        let weak = this.me.clone();
        store.add(lifecycle_service.on_will_shutdown().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.stop();
            }
        }));
        this
    }

    fn stop(&self) {
        self.stopped.set(true);
        self.fire_active_abort();

        loop {
            let entry = {
                let mut queue = self.queue.borrow_mut();
                if queue.is_empty() { None } else { Some(queue.remove(0)) }
            };
            let Some(entry) = entry else { break };
            self.pending.borrow_mut().remove(&entry.scenario.id);
            entry.deferred.complete(OnboardingOutcome::Aborted);
        }
    }

    /// `this._activeAbort?.fire()`.
    fn fire_active_abort(&self) {
        let abort = self.active_abort.borrow().clone();
        if let Some(abort) = abort {
            abort.fire(());
        }
    }

    /// O ouvinte de cancelamento de `runScenario`: tira o cenário da fila ou aborta a execução em andamento.
    fn cancel_run(&self, id: &str) {
        let index = self.queue.borrow().iter().position(|entry| entry.scenario.id == id);
        if let Some(index) = index {
            let entry = self.queue.borrow_mut().remove(index);
            self.pending.borrow_mut().remove(id);
            entry.deferred.complete(OnboardingOutcome::Aborted);
        } else if self.inflight.borrow().contains_key(id) {
            self.fire_active_abort();
        }
    }

    //#region Eligibility & scheduling

    /// The master switch for *automatic* onboarding. When `onboarding.enabled` is explicitly `false`, no scenario ever
    /// runs automatically (developer mode does NOT override this, see [`Self::evaluate`]). Any other value (including
    /// unset) is treated as enabled. On-demand `runScenario` is intentionally exempt from this switch.
    fn enabled(&self) -> bool {
        !matches!(self.configuration_service.get_value(Some(ONBOARDING_ENABLED_CONFIG), None), Dynamic::Bool(false))
    }

    fn is_developer_mode(&self, scenario_id: &str) -> bool {
        is_onboarding_developer_mode_enabled(self.configuration_service.as_ref(), scenario_id)
    }

    /// Re-evaluate every scenario and enqueue any that are eligible to run automatically. Idempotent: already shown /
    /// queued scenarios are skipped.
    ///
    /// The automatic eligibility rules are:
    /// 1. If `onboarding.enabled` is `false`, nothing runs automatically: this method returns immediately, and
    ///    developer mode does NOT override it.
    /// 2. If a scenario declares an `experiment`, it only runs when the experiment is active AND the user is in the
    ///    treatment arm (see below), OR when developer mode is enabled for that scenario, which bypasses the
    ///    experiment gate so the tour can be previewed locally.
    /// 3. If a scenario has no `experiment`, it runs for every user that meets its `when`/trigger criteria (the
    ///    typical state once an experiment has graduated and the tour is rolled out to everyone).
    ///
    /// For an experiment-active scenario, reaching eligibility *is* the "would-show" moment: the telemetry gate is
    /// opened for the experiment's assignment-context id (in both arms), and then only the treatment arm is enqueued
    /// to actually show the tour. Control opens the gate but renders nothing and is not marked as shown. A pre-tour
    /// nudge opens the same gate earlier, at its own would-show moment.
    ///
    /// Developer mode is the exception: it shows the tour unconditionally and never opens the telemetry gate, so a
    /// local preview can never affect the experiment scorecard regardless of which arm the developer happens to be
    /// assigned to.
    fn evaluate(&self) {
        if !self.enabled() || self.stopped.get() {
            return;
        }

        // Seen keys already claimed by a pending/queued/running non-repeatable scenario. Scenarios that share a
        // `seenKey` are gated together, so once one sibling is scheduled we must not also schedule another in the same
        // pass: shown state is only written when a scenario starts running, after the queue has been populated, so the
        // shared-key check in `is_auto_eligible` cannot see the sibling yet.
        let mut claimed_seen_keys: HashSet<String> = HashSet::new();
        for scenario in onboarding_scenario_registry().get_scenarios() {
            if scenario.repeatable != Some(true) && self.pending.borrow().contains(&scenario.id) {
                claimed_seen_keys.insert(seen_key(&scenario));
            }
        }

        let mut eligible_scenarios: Vec<(usize, Rc<OnboardingScenario>)> = onboarding_scenario_registry()
            .get_scenarios()
            .into_iter()
            .enumerate()
            .filter(|(_, scenario)| self.is_auto_eligible(scenario, false))
            .collect();
        eligible_scenarios.sort_by(|(a_index, a), (b_index, b)| by_priority_descending(a, b).then(a_index.cmp(b_index)));

        for (_, scenario) in eligible_scenarios {
            let key = seen_key(&scenario);
            if scenario.repeatable != Some(true) && claimed_seen_keys.contains(&key) {
                // A sibling sharing this seen key is already scheduled this pass; showing it will mark this scenario
                // seen too.
                continue;
            }

            if !self.should_show(&scenario) {
                continue;
            }

            self.enqueue(scenario.clone());
            if scenario.repeatable != Some(true) {
                claimed_seen_keys.insert(key);
            }
        }
    }

    fn should_show(&self, scenario: &OnboardingScenario) -> bool {
        let experiment = if scenario.experiment.is_some() { self.experiment_states.borrow().get(&scenario.id).cloned() } else { None };
        if let Some(experiment) = experiment
            && experiment.active
            && !self.is_developer_mode(&scenario.id)
        {
            // Record the would-show moment in both arms, before any onboarding UI changes user behavior.
            self.open_gate(&experiment.assignment_context_id);
            return experiment.behavior;
        }
        true
    }

    fn is_auto_eligible(&self, scenario: &OnboardingScenario, ignore_trigger: bool) -> bool {
        // Tryouts and command triggers never run automatically.
        if scenario.tryout.is_some() || matches!(scenario.trigger, OnboardingTrigger::Command { .. }) {
            return false;
        }

        if self.pending.borrow().contains(&scenario.id) {
            return false;
        }

        if scenario.repeatable != Some(true) && self.has_been_shown_key(&seen_key(scenario), &scenario.id) {
            return false;
        }

        if let Some(when) = &scenario.when
            && !self.context_key_service.context_matches_rules(Some(when))
        {
            return false;
        }

        // Experiment-driven scenarios only run once the experiment is active (both treatment flags resolved). The
        // behavior flag does NOT gate eligibility: control still reaches the would-show moment so the gate opens for it
        // too.
        //
        // Developer mode for this scenario bypasses the experiment gate entirely so the tour can be tested locally
        // without the experiment running (or being assigned to the user). A developer-mode preview never opens the
        // assignment-context gate (see `evaluate`), so it never pollutes the scorecard.
        if scenario.experiment.is_some()
            && self.experiment_states.borrow().get(&scenario.id).map(|state| state.active) != Some(true)
            && !self.is_developer_mode(&scenario.id)
        {
            return false;
        }

        if !ignore_trigger
            && let OnboardingTrigger::Observable { signal } = &scenario.trigger
            && !signal.get()
        {
            return false;
        }

        true
    }

    fn enqueue(&self, scenario: Rc<OnboardingScenario>) -> Promise<OnboardingOutcome> {
        if self.stopped.get() {
            return Promise::resolved(OnboardingOutcome::Aborted);
        }

        // De-duplicate against both the queue and the in-flight run so a repeated `runScenario(id)` (e.g. a command
        // invoked while the tour is active) joins the existing run instead of scheduling a second one.
        let queued = self.queue.borrow().iter().find(|entry| entry.scenario.id == scenario.id).map(|entry| entry.deferred.promise.clone());
        if let Some(queued) = queued {
            return queued;
        }
        let inflight = self.inflight.borrow().get(&scenario.id).map(|deferred| deferred.promise.clone());
        if let Some(inflight) = inflight {
            return inflight;
        }

        let deferred = Rc::new(DeferredPromise::<OnboardingOutcome>::new());
        self.pending.borrow_mut().insert(scenario.id.clone());
        let promise = deferred.promise.clone();
        {
            let mut queue = self.queue.borrow_mut();
            queue.push(QueueEntry { scenario, deferred });
            // Highest priority first; stable for equal priorities.
            queue.sort_by(|a, b| by_priority_descending(&a.scenario, &b.scenario));
        }

        self.pump();
        promise
    }

    fn pump(&self) {
        if self.pumping.get() {
            return;
        }
        // Mark as pumping synchronously so a batch of `enqueue` calls made in the same tick all land (and re-sort by
        // priority) before we consume the queue.
        self.pumping.set(true);
        if let Some(this) = self.me.upgrade() {
            Self::do_pump(this);
        }
    }

    fn do_pump(this: Rc<Self>) {
        let _ = async_fn(async move {
            Promise::resolved(()).await?; // let the current synchronous batch of enqueues settle
            loop {
                if this.stopped.get() {
                    break;
                }
                let entry = {
                    let mut queue = this.queue.borrow_mut();
                    if queue.is_empty() { None } else { Some(queue.remove(0)) }
                };
                let Some(QueueEntry { scenario, deferred }) = entry else { break };

                // Track the running scenario so a concurrent `enqueue` for the same id joins this run instead of
                // scheduling another.
                this.inflight.borrow_mut().insert(scenario.id.clone(), deferred.clone());
                let outcome = match Self::run_presentation(this.clone(), scenario.clone()).await {
                    Ok(outcome) => outcome,
                    Err(error) => {
                        on_unexpected_error(&error);
                        OnboardingOutcome::Aborted
                    }
                };
                this.inflight.borrow_mut().remove(&scenario.id);
                this.pending.borrow_mut().remove(&scenario.id);
                deferred.complete(outcome);
            }
            this.pumping.set(false);
            Ok::<(), Error>(())
        });
    }

    fn run_presentation(this: Rc<Self>, scenario: Rc<OnboardingScenario>) -> Promise<OnboardingOutcome> {
        async_fn(async move {
            let presentation = onboarding_presentation_registry().get(&scenario.presentation.kind);
            let Some(presentation) = presentation.filter(|_| scenario.tryout.is_none()) else {
                return Ok(OnboardingOutcome::Aborted);
            };

            let store = DisposableStore::new();
            let cancellation = CancellationTokenSource::new(None);
            store.add_rc(Rc::new(cancellation.clone()));
            let abort = store.add_rc(Emitter::<()>::new());
            let running = Rc::new(Cell::new(false));
            store.add(abort.event().listen({
                let (running, cancellation) = (running.clone(), cancellation.clone());
                move |_| {
                    // Only cancel the queue wait; active presentations must finish their abort cleanup.
                    if !running.get() {
                        cancellation.cancel();
                    }
                }
            }));
            *this.active_abort.borrow_mut() = Some(abort.clone());

            let result = run_with_onboarding_presentation(&main_window(), &cancellation.token(), {
                let (this, scenario, abort, running) = (this.clone(), scenario.clone(), abort.clone(), running.clone());
                move || {
                    if this.stopped.get() {
                        return Promise::rejected(Error::cancellation());
                    }
                    running.set(true);
                    Self::show_presentation(this, scenario, presentation, abort)
                }
            })
            .await;
            let outcome = match result {
                Err(error) if is_cancellation_error(&error) => Ok(OnboardingOutcome::Aborted),
                other => other,
            };

            // finally
            let is_active = this.active_abort.borrow().as_ref().is_some_and(|active| Rc::ptr_eq(active, &abort));
            if is_active {
                *this.active_abort.borrow_mut() = None;
            }
            store.dispose();
            outcome
        })
    }

    fn show_presentation(
        this: Rc<Self>,
        scenario: Rc<OnboardingScenario>,
        presentation: Rc<dyn IOnboardingPresentation>,
        abort: Rc<Emitter<()>>,
    ) -> Promise<OnboardingOutcome> {
        async_fn(async move {
            // Mark shown only after the window is available to present the scenario.
            this.mark_shown(&seen_key(&scenario));
            let start_time = runtime::date_now();
            let did_report_shown = Rc::new(Cell::new(false));
            let context = OnboardingRunContext {
                target_window: main_window(),
                on_did_show: Some(Rc::new({
                    let (weak, scenario, did_report_shown) = (this.me.clone(), scenario.clone(), did_report_shown.clone());
                    move || {
                        if !did_report_shown.get() {
                            did_report_shown.set(true);
                            if let Some(this) = weak.upgrade() {
                                this.report_shown(&scenario);
                            }
                        }
                    }
                })),
                on_abort: abort.event(),
                target_scope: None,
            };
            let result = presentation.run(&scenario, &context).await?;
            this.record_outcome(&seen_key(&scenario), result.outcome);
            // Only emit outcome telemetry when a tour was genuinely displayed.
            if result.shown {
                this.report_outcome(&scenario, &result, runtime::date_now().saturating_sub(start_time));
            }
            Ok(result.outcome)
        })
    }

    /// Emit an impression when a presentation has rendered visible onboarding UI.
    fn report_shown(&self, scenario: &OnboardingScenario) {
        let experiment_state = if scenario.experiment.is_some() { self.experiment_states.borrow().get(&scenario.id).cloned() } else { None };
        let experiment_active = experiment_state.as_ref().is_some_and(|state| state.active);
        let experiment_assignment_context_id = match &experiment_state {
            Some(state) if state.active => Dynamic::string(&state.assignment_context_id),
            _ => Dynamic::Undefined,
        };
        self.telemetry_service.public_log2(
            "onboarding.scenarioShown",
            Some(Dynamic::object([
                ("scenarioId", Dynamic::string(&scenario.id)),
                ("experimentActive", Dynamic::Bool(experiment_active)),
                ("experimentAssignmentContextId", experiment_assignment_context_id),
            ])),
        );
    }

    /// Emit per-tour telemetry. Only called when a tour was actually shown.
    fn report_outcome(&self, scenario: &OnboardingScenario, result: &OnboardingRunResult, duration_ms: u64) {
        let experiment_active = scenario.experiment.is_some() && self.experiment_states.borrow().get(&scenario.id).is_some_and(|state| state.active);
        self.telemetry_service.public_log2(
            "onboarding.scenarioOutcome",
            Some(Dynamic::object([
                ("scenarioId", Dynamic::string(&scenario.id)),
                ("outcome", Dynamic::string(result.outcome.as_str())),
                ("dismissReason", Dynamic::string(result.dismiss_reason.as_str())),
                ("lastStepIndex", Dynamic::Number(result.last_step_index)),
                ("stepCount", Dynamic::Number(result.step_count)),
                ("durationMs", Dynamic::Number(duration_ms as f64)),
                ("experimentActive", Dynamic::Bool(experiment_active)),
            ])),
        );
    }

    //#endregion

    //#region Triggers & experiments

    fn register_trigger_listeners(&self) {
        self.trigger_listeners.clear();
        for scenario in onboarding_scenario_registry().get_scenarios() {
            if let OnboardingTrigger::Observable { signal } = &scenario.trigger {
                let (signal, weak) = (signal.clone(), self.me.clone());
                self.trigger_listeners.add(autorun(move |reader| {
                    let _ = signal.read(reader);
                    if let Some(this) = weak.upgrade() {
                        this.evaluate();
                    }
                }));
            }
        }
    }

    /// Resolve the two experiment treatment flags for each scenario that declares an experiment. The experiment is
    /// only active when both resolve: the boolean to a boolean and the id to a non-empty string that starts with
    /// [`ONBOARDING_ASSIGNMENT_CONTEXT_PREFIX`]. Resolved once per scenario; re-evaluation is triggered when an
    /// experiment becomes active.
    fn resolve_experiments(&self) {
        for scenario in onboarding_scenario_registry().get_scenarios() {
            let Some(experiment) = scenario.experiment.clone() else { continue };
            if self.experiment_states.borrow().contains_key(&scenario.id) {
                continue;
            }
            // Seed an inactive state so the scenario is not eligible until both flags resolve.
            self.experiment_states.borrow_mut().insert(scenario.id.clone(), ExperimentState { active: false, behavior: false, assignment_context_id: String::new() });
            let behavior = self.assignment_service.get_treatment(&experiment.behavior_flag);
            let assignment_context_id = self.assignment_service.get_treatment(&experiment.assignment_context_id_flag);
            let (weak, scenario_id) = (self.me.clone(), scenario.id.clone());
            Promise::all(vec![behavior, assignment_context_id]).then_result(move |result| {
                match result {
                    Ok(treatments) => {
                        if let Some(this) = weak.upgrade() {
                            this.apply_treatments(&scenario_id, &treatments[0], &treatments[1]);
                        }
                    }
                    Err(error) => on_unexpected_error(&error),
                }
                Ok(())
            });
        }
    }

    /// O `then(([behavior, assignmentContextId]) => ...)` de `_resolveExperiments`.
    fn apply_treatments(&self, scenario_id: &str, behavior: &Option<TreatmentValue>, assignment_context_id: &Option<TreatmentValue>) {
        let has_behavior = matches!(behavior, Some(TreatmentValue::Boolean(_)));
        let assignment_context_id = match assignment_context_id {
            Some(TreatmentValue::String(id)) if !id.is_empty() => Some(id.clone()),
            _ => None,
        };
        let has_id = assignment_context_id.is_some();

        // Defensively require the reserved prefix. The eager telemetry gate only blocks ids that start with it, so an
        // id missing the prefix would never be gated and would leak into telemetry from the very first event,
        // silently corrupting the scorecard baseline. Catch the misconfiguration loudly and treat the experiment as
        // inactive rather than running it with an ungated id.
        let has_valid_id = assignment_context_id.as_deref().is_some_and(|id| id.starts_with(ONBOARDING_ASSIGNMENT_CONTEXT_PREFIX));
        if has_id && !has_valid_id {
            on_unexpected_error(&Error::new(format!(
                "Onboarding experiment for scenario '{scenario_id}' resolved an assignment-context id '{}' that does not start with the required '{ONBOARDING_ASSIGNMENT_CONTEXT_PREFIX}' prefix; treating the experiment as inactive.",
                assignment_context_id.as_deref().unwrap_or_default()
            )));
        }

        let active = has_behavior && has_valid_id;
        self.experiment_states.borrow_mut().insert(
            scenario_id.to_string(),
            ExperimentState {
                active,
                behavior: matches!(behavior, Some(TreatmentValue::Boolean(true))),
                assignment_context_id: if active { assignment_context_id.unwrap_or_default() } else { String::new() },
            },
        );
        self.experiment_states_changed.trigger(None, ());
        if active {
            self.evaluate();
        }
    }

    //#endregion

    //#region Telemetry gate

    /// Open the telemetry gate for an assignment-context id: from now on (and after reload) the id is no longer
    /// filtered out, so every event carries it. Idempotent.
    fn open_gate(&self, assignment_context_id: &str) {
        if assignment_context_id.is_empty() || self.opened_assignment_context_ids.borrow().iter().any(|opened| opened == assignment_context_id) {
            return;
        }
        self.opened_assignment_context_ids.borrow_mut().push(assignment_context_id.to_string());
        let serialized = {
            let ids = self.opened_assignment_context_ids.borrow();
            let quoted: Vec<String> = ids.iter().map(|id| json_quote(&JsString::from(id.as_str()))).collect();
            format!("[{}]", quoted.join(","))
        };
        self.storage_service.store(OPENED_IDS_STORAGE_KEY, StorageValue::String(serialized), StorageScope::Application, StorageTarget::Machine);
        // Recompute the filtered assignment context so the id starts flowing immediately.
        self.on_did_change_opened_ids.fire(());
    }

    //#endregion

    //#region Persistence

    /// `this._state[key]`, quando é um objeto.
    fn state_entry(&self, key: &str) -> Option<ObjectRef> {
        match self.state.get(&JsString::from(key)) {
            Some(Dynamic::Object(entry)) => Some(entry),
            _ => None,
        }
    }

    fn has_been_shown_key(&self, key: &str, scenario_id: &str) -> bool {
        if self.is_developer_mode(scenario_id) {
            return self.shown_since_start.borrow().contains(key);
        }
        self.state_entry(key).is_some_and(|entry| entry.get(&JsString::from("shownAt")).is_some_and(|shown_at| shown_at.is_truthy()))
    }

    fn mark_shown(&self, id: &str) {
        self.shown_since_start.borrow_mut().insert(id.to_string());
        let previous = self.state_entry(id);
        let previous_outcome = previous.as_ref().and_then(|entry| entry.get(&JsString::from("outcome"))).unwrap_or(Dynamic::Undefined);
        let previous_seen_count = match previous.as_ref().and_then(|entry| entry.get(&JsString::from("seenCount"))) {
            Some(Dynamic::Number(count)) => count,
            _ => 0.0,
        };
        let next = Dynamic::object([
            ("shownAt", Dynamic::Number(runtime::date_now() as f64)),
            ("outcome", previous_outcome),
            ("seenCount", Dynamic::Number(previous_seen_count + 1.0)),
        ]);
        self.state.set(&JsString::from(id), next);
        self.memento.save_memento();
    }

    fn record_outcome(&self, id: &str, outcome: OnboardingOutcome) {
        if let Some(entry) = self.state_entry(id) {
            entry.set(&JsString::from("outcome"), Dynamic::string(outcome.as_str()));
            self.memento.save_memento();
        }
    }

    //#endregion
}

impl IDisposable for OnboardingScenarioService {
    fn dispose(&self) {
        self.store.dispose();
    }
}

impl IOnboardingScenarioService for OnboardingScenarioService {
    fn start(&self) {
        if self.started.get() {
            return;
        }
        self.started.set(true);

        let weak = self.me.clone();
        self.store.add(onboarding_scenario_registry().on_did_change().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.register_trigger_listeners();
                    this.resolve_experiments();
                    this.evaluate();
                }
            }
        }));

        self.store.add(self.context_key_service.on_did_change_context().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(this) = weak.upgrade() {
                    this.evaluate();
                }
            }
        }));

        self.store.add(self.configuration_service.on_did_change_configuration().listen(move |event| {
            if event.affects_configuration(ONBOARDING_ENABLED_CONFIG, None) || event.affects_configuration(ONBOARDING_DEVELOPER_MODE_CONFIG, None) {
                if let Some(this) = weak.upgrade() {
                    this.evaluate();
                }
            }
        }));

        self.register_trigger_listeners();
        self.resolve_experiments();
        self.evaluate();
    }

    fn get_scenarios(&self) -> Vec<Rc<OnboardingScenario>> {
        onboarding_scenario_registry().get_scenarios()
    }

    fn run_scenario(&self, id: &str, token: CancellationToken) -> Promise<OnboardingOutcome> {
        let this = self.me.upgrade().expect("o serviço está vivo enquanto é usado");
        let id = id.to_string();
        async_fn(async move {
            let Some(scenario) = onboarding_scenario_registry().get_scenario(&id) else {
                return Err(Error::new(format!("Unknown onboarding scenario '{id}'.")));
            };
            if token.is_cancellation_requested() {
                return Ok(OnboardingOutcome::Aborted);
            }
            let store = DisposableStore::new();
            store.add(token.on_cancellation_requested().listen({
                let (weak, id) = (this.me.clone(), id.clone());
                move |_| {
                    if let Some(this) = weak.upgrade() {
                        this.cancel_run(&id);
                    }
                }
            }));
            let outcome = this.enqueue(scenario).await;
            store.dispose();
            outcome
        })
    }

    fn has_been_shown(&self, id: &str) -> bool {
        let key = onboarding_scenario_registry().get_scenario(id).map_or_else(|| id.to_string(), |scenario| seen_key(&scenario));
        self.has_been_shown_key(&key, id)
    }

    fn should_show_nudge(&self, id: &str, reader: Option<&dyn IReader>) -> bool {
        self.experiment_states_changed.read_opt(reader);
        let Some(scenario) = onboarding_scenario_registry().get_scenario(id) else {
            errors::throw(Error::new(format!("Unknown onboarding scenario '{id}'.")));
        };
        self.enabled() && !self.stopped.get() && self.is_auto_eligible(&scenario, true) && self.should_show(&scenario)
    }

    fn reset(&self, id: &str) {
        let key = onboarding_scenario_registry().get_scenario(id).map_or_else(|| id.to_string(), |scenario| seen_key(&scenario));
        self.state.delete(&JsString::from(key.as_str()));
        self.shown_since_start.borrow_mut().remove(&key);
        self.memento.save_memento();
    }

    fn reset_all(&self) {
        for key in self.state.keys() {
            self.state.delete(&key);
        }
        self.memento.save_memento();
    }
}
