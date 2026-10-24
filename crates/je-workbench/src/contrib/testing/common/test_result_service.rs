//! Porte de `src/vs/workbench/contrib/testing/common/testResultService.ts`: o serviço que guarda os resultados de testes
//! (ao vivo e hidratados), mantém a ordem (execuções em andamento primeiro), limita a quantidade e os persiste.
//!
//! Divergências:
//! - **`ResolvedTestRunRequest | ExtensionRunTestsRequest`** é o enum [`RunTestsRequest`] (o original distingue por
//!   `'targets' in req`).
//! - **`push<T extends ITestResult>(result: T): T`** é o método inerente [`TestResultService::push`] (genérico, devolve o
//!   `Rc<T>` recebido) e o do trait [`ITestResultService::push`] (sobre `Rc<dyn ITestResult>`).
//! - **`results`** devolve uma cópia da lista (`Vec`); as mutações internas passam pela célula `results`, depois de
//!   disparar `loadResults` como o getter do original.
//! - **`createSingleCallFunction(() => storage.read().then(...))`** é `load_results`, que guarda a promessa da primeira
//!   chamada; uma chamada reentrante antes de a promessa existir devolve uma promessa resolvida (o original devolve
//!   `undefined`).
//! - **`protected persistScheduler`** (que o `TestTestResultService` do teste do original sobrescreve com
//!   `upcastPartial<RunOnceScheduler>({ schedule: () => this.persistImmediately() })`) é o [`PersistScheduler`], escolhido
//!   por [`TestResultService::new`] (agendado, 500 ms) e [`TestResultService::new_with_immediate_persistence`] (imediato).
//! - **`protected persistImmediately`** é público (Rust não tem herança).
//! - **`DisposableMap<ITestResult, DisposableStore>`** é `DisposableMap<ById<dyn ITestResult>, DisposableStore>`: a chave é a
//!   identidade do resultado.
//! - **`ResultChangeEvent`** é o enum [`ResultChangeEvent`].
//! - **`isRunningTests`** é [`is_running_tests`] sobre o trait.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::arrays_find::find_first_idx_monotonous_or_arr_len;
use je_base::async_::RunOnceScheduler;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{ById, Disposable, DisposableMap, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uuid::generate_uuid;
use je_instantiation::service_id;
use je_platform::contextkey::{IContextKey, IContextKeyService};
use je_platform::telemetry::telemetry::ITelemetryService;

use super::test_profile_service::ITestProfileService;
use super::test_result::{ITestResult, LiveTestResult, TestResultItemChange, TestResultItemChangeKind, TestResultItemRef};
use super::test_result_storage::{ITestResultStorage, RETAIN_MAX_RESULTS};
use super::test_types::{ExtensionRunTestsRequest, ITestRunProfile, ResolvedTestRunRequest, TestResultState, TestRunTarget, test_run_profile_bitset};
use super::testing_context_keys;

/// `ResultChangeEvent`.
#[derive(Clone)]
pub enum ResultChangeEvent {
    Completed(Rc<LiveTestResult>),
    Started(Rc<LiveTestResult>),
    Inserted(Rc<dyn ITestResult>),
    Removed(Vec<Rc<dyn ITestResult>>),
}

/// `ResolvedTestRunRequest | ExtensionRunTestsRequest`.
#[derive(Clone, Debug)]
pub enum RunTestsRequest {
    Resolved(ResolvedTestRunRequest),
    Extension(ExtensionRunTestsRequest),
}

pub trait ITestResultService {
    /// Fired after any results are added, removed, or completed.
    fn on_results_changed(&self) -> Event<ResultChangeEvent>;

    /// Fired when a test changed it state, or its computed state is updated.
    fn on_test_changed(&self) -> Event<TestResultItemChange>;

    /// List of known test results.
    fn results(&self) -> Vec<Rc<dyn ITestResult>>;

    /// Discards all completed test results.
    fn clear(&self);

    /// Creates a new, live test result.
    fn create_live_result(&self, req: RunTestsRequest) -> Rc<LiveTestResult>;

    /// Adds a new test result to the collection.
    fn push(&self, result: Rc<dyn ITestResult>) -> Rc<dyn ITestResult>;

    /// Looks up a set of test results by ID.
    fn get_result(&self, result_id: &str) -> Option<Rc<dyn ITestResult>>;

    /// Looks up a test's most recent state, by its extension-assigned ID.
    fn get_state_by_id(&self, ext_id: &str) -> Option<(Rc<dyn ITestResult>, TestResultItemRef)>;
}

pub fn is_running_tests(service: &dyn ITestResultService) -> bool {
    let results = service.results();
    !results.is_empty() && results[0].completed_at().is_none()
}

service_id!(pub I_TEST_RESULT_SERVICE: dyn ITestResultService = "testResultService");

/// O `persistScheduler`.
enum PersistScheduler {
    /// `new RunOnceScheduler(() => this.persistImmediately(), 500)`.
    Delayed(Rc<RunOnceScheduler>),
    /// O `{ schedule: () => this.persistImmediately() }` do `TestTestResultService`.
    Immediate,
}

/// `!!result.completedAt`.
fn is_completed_truthy(result: &dyn ITestResult) -> bool {
    result.completed_at().is_some_and(|completed_at| completed_at != 0.0)
}

pub struct TestResultService {
    this: Weak<TestResultService>,
    base: Rc<Disposable>,
    change_result_emitter: Rc<Emitter<ResultChangeEvent>>,
    results: RefCell<Vec<Rc<dyn ITestResult>>>,
    results_disposables: Rc<DisposableMap<ById<dyn ITestResult>, DisposableStore>>,
    test_change_emitter: Rc<Emitter<TestResultItemChange>>,
    insert_order_counter: Cell<i64>,

    is_running: Rc<dyn IContextKey>,
    has_any_results: Rc<dyn IContextKey>,
    load_results_called: Cell<bool>,
    load_results_promise: RefCell<Option<Promise<()>>>,

    persist_scheduler: PersistScheduler,

    storage: Rc<dyn ITestResultStorage>,
    test_profiles: Rc<dyn ITestProfileService>,
    telemetry_service: Rc<dyn ITelemetryService>,
}

impl TestResultService {
    pub fn new(
        context_key_service: Rc<dyn IContextKeyService>,
        storage: Rc<dyn ITestResultStorage>,
        test_profiles: Rc<dyn ITestProfileService>,
        telemetry_service: Rc<dyn ITelemetryService>,
    ) -> Rc<Self> {
        Self::create(context_key_service, storage, test_profiles, telemetry_service, false)
    }

    /// O `TestTestResultService`: o `persistScheduler.schedule()` persiste na hora.
    pub fn new_with_immediate_persistence(
        context_key_service: Rc<dyn IContextKeyService>,
        storage: Rc<dyn ITestResultStorage>,
        test_profiles: Rc<dyn ITestProfileService>,
        telemetry_service: Rc<dyn ITelemetryService>,
    ) -> Rc<Self> {
        Self::create(context_key_service, storage, test_profiles, telemetry_service, true)
    }

    fn create(
        context_key_service: Rc<dyn IContextKeyService>,
        storage: Rc<dyn ITestResultStorage>,
        test_profiles: Rc<dyn ITestProfileService>,
        telemetry_service: Rc<dyn ITelemetryService>,
        immediate_persistence: bool,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let change_result_emitter = Emitter::new();
        let test_change_emitter = Emitter::new();
        base.register(change_result_emitter.clone());
        let results_disposables = DisposableMap::new();
        base.register(results_disposables.clone());
        base.register(test_change_emitter.clone());

        let is_running = testing_context_keys::is_running().bind_to(context_key_service.as_ref());
        let has_any_results = testing_context_keys::has_any_results().bind_to(context_key_service.as_ref());

        Rc::new_cyclic(|me: &Weak<TestResultService>| {
            let persist_scheduler = if immediate_persistence {
                PersistScheduler::Immediate
            } else {
                let target = me.clone();
                let scheduler = RunOnceScheduler::new(
                    move || {
                        if let Some(service) = target.upgrade() {
                            let _ = service.persist_immediately();
                        }
                    },
                    500,
                );
                base.register(scheduler.clone());
                PersistScheduler::Delayed(scheduler)
            };

            Self {
                this: me.clone(),
                base: base.clone(),
                change_result_emitter,
                results: RefCell::new(Vec::new()),
                results_disposables,
                test_change_emitter,
                insert_order_counter: Cell::new(0),
                is_running,
                has_any_results,
                load_results_called: Cell::new(false),
                load_results_promise: RefCell::new(None),
                persist_scheduler,
                storage,
                test_profiles,
                telemetry_service,
            }
        })
    }

    fn schedule_persist(&self) {
        match &self.persist_scheduler {
            PersistScheduler::Delayed(scheduler) => scheduler.schedule(None),
            PersistScheduler::Immediate => {
                let _ = self.persist_immediately();
            }
        }
    }

    /// `loadResults`: a primeira chamada lê o armazenamento e empurra o que veio (do mais antigo para o mais novo).
    fn load_results(&self) -> Promise<()> {
        if self.load_results_called.get() {
            return self.load_results_promise.borrow().clone().unwrap_or_else(|| Promise::resolved(()));
        }
        self.load_results_called.set(true);

        let this = self.this.upgrade().expect("o serviço está vivo enquanto é usado");
        let promise = self.storage.read().then(move |loaded| {
            for loaded_result in loaded.iter().rev() {
                let result: Rc<dyn ITestResult> = loaded_result.clone();
                ITestResultService::push(this.as_ref(), result);
            }
        });
        *self.load_results_promise.borrow_mut() = Some(promise.clone());
        promise
    }

    /// `push<T extends ITestResult>(result: T): T`.
    pub fn push<T: ITestResult + 'static>(&self, result: Rc<T>) -> Rc<T> {
        let as_dyn: Rc<dyn ITestResult> = result.clone();
        ITestResultService::push(self, as_dyn);
        result
    }

    fn on_complete(&self, result: &Rc<LiveTestResult>) {
        self.resort();
        self.update_is_running();
        self.schedule_persist();
        self.change_result_emitter.fire(ResultChangeEvent::Completed(result.clone()));
    }

    fn resort(&self) {
        self.load_results();
        self.results.borrow_mut().sort_by(|a, b| {
            // Running tests should always be sorted higher:
            if is_completed_truthy(a.as_ref()) != is_completed_truthy(b.as_ref()) {
                return if a.completed_at().is_none() { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
            }

            // Otherwise sort by insertion order, hydrated tests are always last:
            let a_comp = a.as_live().map_or(-1, |live| live.insert_order);
            let b_comp = b.as_live().map_or(-1, |live| live.insert_order);
            b_comp.cmp(&a_comp)
        });
    }

    fn update_is_running(&self) {
        self.is_running.set(Dynamic::Bool(is_running_tests(self)));
    }

    /// `protected async persistImmediately()`: garante que os resultados foram carregados antes de persistir, para não
    /// apagar os que ainda não temos.
    pub fn persist_immediately(&self) -> Promise<()> {
        let this = self.this.upgrade().expect("o serviço está vivo enquanto é usado");
        self.load_results().then(move |_| {
            let results = this.results();
            let _ = this.storage.persist(&results);
        })
    }
}

impl ITestResultService for TestResultService {
    /// @inheritdoc
    fn on_results_changed(&self) -> Event<ResultChangeEvent> {
        self.change_result_emitter.event()
    }

    /// @inheritdoc
    fn on_test_changed(&self) -> Event<TestResultItemChange> {
        self.test_change_emitter.event()
    }

    /// @inheritdoc
    fn results(&self) -> Vec<Rc<dyn ITestResult>> {
        self.load_results();
        self.results.borrow().clone()
    }

    /// @inheritdoc
    fn get_state_by_id(&self, ext_id: &str) -> Option<(Rc<dyn ITestResult>, TestResultItemRef)> {
        for result in self.results() {
            let lookup = result.get_state_by_id(ext_id);
            if let Some(lookup) = lookup
                && lookup.data.borrow().computed_state != TestResultState::Unset
            {
                return Some((result, lookup));
            }
        }

        None
    }

    /// @inheritdoc
    fn create_live_result(&self, req: RunTestsRequest) -> Rc<LiveTestResult> {
        let live = match req {
            RunTestsRequest::Resolved(req) => {
                let id = generate_uuid();
                let order = self.next_insert_order();
                LiveTestResult::new(&id, true, req, order, self.telemetry_service.clone())
            }
            RunTestsRequest::Extension(req) => {
                let mut profile: Option<ITestRunProfile> = None;
                if let Some(requested) = &req.profile {
                    let profiles = self.test_profiles.get_controller_profiles(&req.controller_id);
                    profile = profiles.into_iter().find(|c| c.profile_id == requested.id);
                }

                let mut resolved = ResolvedTestRunRequest {
                    preserve_focus: Some(req.preserve_focus),
                    targets: Vec::new(),
                    exclude: Some(req.exclude.clone()),
                    continuous: Some(req.continuous),
                    group: profile.as_ref().map_or(test_run_profile_bitset::RUN, |p| p.group),
                };

                if let Some(profile) = &profile {
                    resolved.targets.push(TestRunTarget { profile_id: profile.profile_id, controller_id: req.controller_id.clone(), test_ids: req.include.clone() });
                }

                let order = self.next_insert_order();
                LiveTestResult::new(&req.id, req.persist, resolved, order, self.telemetry_service.clone())
            }
        };

        self.push(live)
    }

    /// @inheritdoc
    fn push(&self, result: Rc<dyn ITestResult>) -> Rc<dyn ITestResult> {
        self.load_results();
        match result.completed_at() {
            None => self.results.borrow_mut().insert(0, result.clone()),
            Some(completed_at) => {
                let index = {
                    let results = self.results.borrow();
                    find_first_idx_monotonous_or_arr_len(&results, |r| r.completed_at().is_some_and(|c| c <= completed_at), 0, results.len())
                };
                self.results.borrow_mut().insert(index, result.clone());
                self.schedule_persist();
            }
        }

        self.has_any_results.set(Dynamic::Bool(true));
        let mut removed: Option<Rc<dyn ITestResult>> = None;
        if self.results.borrow().len() > RETAIN_MAX_RESULTS {
            removed = self.results.borrow_mut().pop();
        }

        let ds = DisposableStore::new();
        self.results_disposables.set(ById(result.clone()), ds.clone(), false);

        let live = result.as_live();
        if let Some(live) = &live {
            ds.add_rc(live.clone());
            let this = self.this.clone();
            let completed = live.clone();
            ds.add(live.on_complete().listen(move |_| {
                if let Some(service) = this.upgrade() {
                    service.on_complete(&completed);
                }
            }));
            let emitter = self.test_change_emitter.clone();
            ds.add(live.on_change().listen(move |change| emitter.fire(change.clone())));
        }

        if let Some(removed) = removed {
            self.results_disposables.delete_and_dispose(&ById(removed));
        }

        if let Some(live) = live {
            self.is_running.set(Dynamic::Bool(true));
            self.change_result_emitter.fire(ResultChangeEvent::Started(live));
        } else {
            self.change_result_emitter.fire(ResultChangeEvent::Inserted(result.clone()));
            // If this is not a new result, go through each of its tests. For each
            // test for which the new result is the most recently inserted, fir
            // a change event so that UI updates.
            for item in result.tests() {
                let ext_id = item.data.borrow().item.ext_id.clone();
                for other_result in self.results() {
                    if Rc::ptr_eq(&other_result, &result) {
                        self.test_change_emitter.fire(TestResultItemChange { item: item.clone(), result: result.clone(), change: TestResultItemChangeKind::ComputedStateChange });
                        break;
                    } else if other_result.get_state_by_id(&ext_id).is_some() {
                        break;
                    }
                }
            }
        }

        result
    }

    /// @inheritdoc
    fn get_result(&self, id: &str) -> Option<Rc<dyn ITestResult>> {
        self.results().into_iter().find(|r| r.id() == id)
    }

    /// @inheritdoc
    fn clear(&self) {
        let mut keep: Vec<Rc<dyn ITestResult>> = Vec::new();
        let mut removed: Vec<Rc<dyn ITestResult>> = Vec::new();
        for result in self.results() {
            if result.completed_at().is_some() {
                removed.push(result);
            } else {
                keep.push(result);
            }
        }

        *self.results.borrow_mut() = keep.clone();
        for result in &removed {
            self.results_disposables.delete_and_dispose(&ById(result.clone()));
        }
        self.schedule_persist();
        if keep.is_empty() {
            self.has_any_results.set(Dynamic::Bool(false));
        }
        self.change_result_emitter.fire(ResultChangeEvent::Removed(removed));
    }
}

impl TestResultService {
    fn next_insert_order(&self) -> i64 {
        let order = self.insert_order_counter.get();
        self.insert_order_counter.set(order + 1);
        order
    }
}

impl IDisposable for TestResultService {
    fn dispose(&self) {
        self.base.dispose();
    }
}
