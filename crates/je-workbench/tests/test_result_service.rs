//! Porte de `src/vs/workbench/contrib/testing/test/common/testResultService.test.ts` (suíte `Workbench - Test Results
//! Service`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()` e `setup`.** O guarda de vazamentos é criado no começo de cada caso
//!   e o `setup` assíncrono ([`setup`]) roda logo depois, como no original (onde o `setup` vem depois do guarda); o
//!   `ds` do original é o próprio guarda. O descarte e a conferência rodam no `Drop` do guarda.
//! - **Casos `async`** (todos, por causa do `setup`) rodam em `run_with_faked_timers`; `await` é `.await.unwrap()` (uma
//!   rejeição reprova o caso). Sob o relógio virtual o `Date.now()` começa em 0, e `completedAt && persist` do
//!   `toJSON` é falso para 0: o `setup` fixa o relógio de parede com `runtime::set_system_time`, como o relógio real do
//!   original, que nunca é 0.
//! - **`TestLiveTestResult`** (a subclasse que expõe `setAllToState` e o `disposed`) é o `LiveTestResult` com
//!   `set_all_to_state` público e `is_disposed`; o `ds.add(this)` do construtor é [`new_live`]. O `insertCounter++` é o
//!   [`next_insert_order`].
//! - **`TestTestResultService`** (a subclasse com o `persistScheduler` que persiste na hora) é
//!   `TestResultService::new_with_immediate_persistence`.
//! - **`MockContextKeyService`** é o do porte (`je_platform::keybinding::mock_keybinding_service`); `TestStorageService`
//!   é o `InMemoryStorageService`, entregue ao serviço como `Rc<dyn StorageService>`.
//! - **`{ asCanonicalUri(uri) { return uri; } } as IUriIdentityService`** é o `TestUriIdentityService` de
//!   `support::test_stubs`; **`NullTelemetryService`** é `null_telemetry_service()`.
//! - **`assert.deepStrictEqual(results.results, [r])`** compara a identidade dos resultados (o endereço do `Rc`), que é
//!   o que o `deepStrictEqual` sobre os mesmos objetos conclui.
//! - **`getChangeSummary`** devolve pares `(reason, label)`; o `Set` de mudanças é um `Vec` na ordem de chegada.
//! - **`makeHydrated`** monta o item serializado a partir do nó da coleção da thread principal com `ITestItem::serialize`
//!   (o original espalha o nó direto no objeto, que tem a mesma forma estrutural).
//! - **`serializes and re-hydrates`:** o `expected` é o `TestResultItem` do nó vivo com `item.uri` do hidratado, `retired`
//!   verdadeiro e sem `children` (que o nó guarda à parte). A conferência de `typeof rehydrated.completedAt` é
//!   `completed_at().is_some()` (um `number` sempre presente no hidratado).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::async_::timeout_with_token;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationTokenSource;
use je_base::promise::Promise;
use je_base::runtime;
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::log::NullLogService;
use je_platform::storage::StorageService;
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_workbench::contrib::testing::common::test_id::TestId;
use je_workbench::contrib::testing::common::test_profile_service::{ITestProfileService, TestProfileService};
use je_workbench::contrib::testing::common::test_result::{
    HydratedTestResult, ITestResult, LiveTestResult, TaskRawOutput, TestResultItemChange, TestResultItemChangeReason, result_item_parents,
};
use je_workbench::contrib::testing::common::test_result_service::{ITestResultService, TestResultService};
use je_workbench::contrib::testing::common::test_result_storage::{ITestResultStorage, InMemoryResultStorage};
use je_workbench::contrib::testing::common::test_service::IMainThreadTestCollection;
use je_workbench::contrib::testing::common::test_types::{
    ISerializedTestResults, ITestItem, ITestRunTask, ITestTaskStateSerialized, ResolvedTestRunRequest, SerializedTestResultsTask, TestResultItem,
    TestResultItemSerialized, TestResultState, TestRunTarget, test_run_profile_bitset,
};
use je_workbench::contrib::testing::common::testing_states::make_empty_counts;
use je_workbench::test::common::workbench_test_services::TestStorageService;
use support::test_stubs::{self, TestTestCollection, TestUriIdentityService};

const REAL_CLOCK_MS: u64 = 1_700_000_000_000;

fn labels_in(items: Vec<je_workbench::contrib::testing::common::test_result::TestResultItemRef>) -> Vec<String> {
    let mut labels: Vec<String> = items.iter().map(|t| t.data.borrow().item.label.clone()).collect();
    labels.sort();
    labels
}

struct Fixture {
    r: Rc<LiveTestResult>,
    changed: Rc<RefCell<Vec<TestResultItemChange>>>,
    tests: Rc<TestTestCollection>,
}

impl Fixture {
    fn get_change_summary(&self) -> Vec<(TestResultItemChangeReason, String)> {
        self.changed.borrow().iter().map(|c| (c.reason(), c.item.data.borrow().item.label.clone())).collect()
    }
}

fn summary(expected: &[(TestResultItemChangeReason, &str)]) -> Vec<(TestResultItemChangeReason, String)> {
    expected.iter().map(|(reason, label)| (*reason, (*label).to_string())).collect()
}

fn default_opts(test_ids: &[&str]) -> ResolvedTestRunRequest {
    ResolvedTestRunRequest {
        group: test_run_profile_bitset::RUN,
        targets: vec![TestRunTarget { profile_id: 0, controller_id: "ctrlId".to_string(), test_ids: test_ids.iter().map(|id| (*id).to_string()).collect() }],
        exclude: None,
        continuous: None,
        preserve_focus: None,
    }
}

thread_local! {
    static INSERT_COUNTER: Cell<i64> = const { Cell::new(0) };
}

/// `insertCounter++`.
fn next_insert_order() -> i64 {
    INSERT_COUNTER.with(|counter| {
        let current = counter.get();
        counter.set(current + 1);
        current
    })
}

/// `new TestLiveTestResult(id, persist, request)`: o construtor registra o resultado no `ds`.
fn new_live(ds: &LeakGuard, id: &str, persist: bool, request: ResolvedTestRunRequest) -> Rc<LiveTestResult> {
    ds.add_rc(LiveTestResult::new(id, persist, request, next_insert_order(), null_telemetry_service()))
}

fn id_of(parts: &[&str]) -> String {
    TestId::new(parts.iter().map(|part| (*part).to_string()).collect()).to_string()
}

fn addr<T: ?Sized>(rc: &Rc<T>) -> usize {
    Rc::as_ptr(rc) as *const () as usize
}

fn addrs(results: &[Rc<dyn ITestResult>]) -> Vec<usize> {
    results.iter().map(addr).collect()
}

async fn setup(ds: &LeakGuard) -> Fixture {
    runtime::set_system_time(REAL_CLOCK_MS);
    let changed: Rc<RefCell<Vec<TestResultItemChange>>> = Rc::new(RefCell::new(Vec::new()));
    let r = new_live(ds, "foo", true, default_opts(&["id-a"]));

    let sink = changed.clone();
    ds.add(r.on_change().listen(move |e| sink.borrow_mut().push(e.clone())));
    r.add_task(ITestRunTask { id: "t".to_string(), name: "n".to_string(), running: true, ctrl_id: "ctrl".to_string() });

    let tests = ds.add_rc(test_stubs::nested(test_stubs::DEFAULT_ID_PREFIX));
    let cts = ds.add_rc(Rc::new(CancellationTokenSource::new(None)));
    let expansion = tests.expand(&tests.root().id(), f64::INFINITY).unwrap_or_else(|| Promise::resolved(()));
    let ok = Promise::race(vec![expansion.then(|_| true), timeout_with_token(1000, &cts.token()).then(|_| false)]).await.unwrap();
    cts.cancel();

    // todo@connor4312: debug for tests #137853:
    if !ok {
        panic!("timed out while expanding, diff: {:?}", tests.collect_diff());
    }

    let root = tests.root().clone();
    let a = root.children().get("id-a").unwrap();
    let aa = a.children().get("id-aa").unwrap();
    let ab = a.children().get("id-ab").unwrap();
    r.add_test_chain_to_run("ctrlId", &[root.to_test_item(), a.to_test_item(), aa.to_test_item()]);
    r.add_test_chain_to_run("ctrlId", &[a.to_test_item(), ab.to_test_item()]);

    Fixture { r, changed, tests }
}

// ITestChildrenLike::get
use je_workbench::contrib::testing::common::test_item_collection::ITestChildrenLike;

fn own_state(r: &LiveTestResult, id: &str) -> Option<TestResultState> {
    r.get_state_by_id(id).map(|item| item.data.borrow().own_computed_state)
}

fn computed_state(r: &LiveTestResult, id: &str) -> Option<TestResultState> {
    r.get_state_by_id(id).map(|item| item.data.borrow().computed_state)
}

// suite('LiveTestResult')

#[test]
fn is_empty_if_no_tests_are_yet_present() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let _fixture = setup(&ds).await;

        assert_eq!(labels_in(new_live(&ds, "foo", false, default_opts(&["id-a"])).tests()), Vec::<String>::new());
    });
}

#[test]
fn initially_queues_nothing() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;

        assert_eq!(fixture.get_change_summary(), summary(&[]));
    });
}

#[test]
fn initializes_with_the_subtree_of_requested_tests() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;

        assert_eq!(labels_in(fixture.r.tests()), vec!["a", "aa", "ab", "root"]);
    });
}

#[test]
fn initializes_with_valid_counts() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;

        let mut c = make_empty_counts();
        c[TestResultState::Unset] = 4;
        assert_eq!(fixture.r.counts(), c);
    });
}

#[test]
fn set_all_to_state() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;
        let r = &fixture.r;

        fixture.changed.borrow_mut().clear();
        r.set_all_to_state(TestResultState::Queued, "t", |_, t| t.item.label != "root");
        let mut c = make_empty_counts();
        c[TestResultState::Unset] = 1;
        c[TestResultState::Queued] = 3;
        assert_eq!(r.counts(), c);

        r.set_all_to_state(TestResultState::Failed, "t", |_, t| t.item.label != "root");
        let mut c2 = make_empty_counts();
        c2[TestResultState::Unset] = 1;
        c2[TestResultState::Failed] = 3;
        assert_eq!(r.counts(), c2);

        assert_eq!(own_state(r, &id_of(&["ctrlId", "id-a"])), Some(TestResultState::Failed));
        assert_eq!(r.get_state_by_id(&id_of(&["ctrlId", "id-a"])).map(|item| item.data.borrow().tasks[0].state), Some(TestResultState::Failed));
        assert_eq!(
            fixture.get_change_summary(),
            summary(&[
                (TestResultItemChangeReason::OwnStateChange, "a"),
                (TestResultItemChangeReason::ComputedStateChange, "root"),
                (TestResultItemChangeReason::OwnStateChange, "aa"),
                (TestResultItemChangeReason::OwnStateChange, "ab"),
                (TestResultItemChangeReason::OwnStateChange, "a"),
                (TestResultItemChangeReason::ComputedStateChange, "root"),
                (TestResultItemChangeReason::OwnStateChange, "aa"),
                (TestResultItemChangeReason::OwnStateChange, "ab"),
            ])
        );
    });
}

#[test]
fn update_state() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;
        let r = &fixture.r;

        fixture.changed.borrow_mut().clear();
        let test_id = id_of(&["ctrlId", "id-a", "id-aa"]);
        r.update_state(&test_id, "t", TestResultState::Running, None);
        let mut c = make_empty_counts();
        c[TestResultState::Running] = 1;
        c[TestResultState::Unset] = 3;
        assert_eq!(r.counts(), c);
        assert_eq!(own_state(r, &test_id), Some(TestResultState::Running));
        // update computed state:
        assert_eq!(computed_state(r, &fixture.tests.root().id()), Some(TestResultState::Running));
        assert_eq!(
            fixture.get_change_summary(),
            summary(&[
                (TestResultItemChangeReason::OwnStateChange, "aa"),
                (TestResultItemChangeReason::ComputedStateChange, "a"),
                (TestResultItemChangeReason::ComputedStateChange, "root"),
            ])
        );

        r.update_state(&test_id, "t", TestResultState::Passed, None);
        assert_eq!(own_state(r, &test_id), Some(TestResultState::Passed));

        r.update_state(&test_id, "t", TestResultState::Errored, None);
        assert_eq!(own_state(r, &test_id), Some(TestResultState::Errored));

        r.update_state(&test_id, "t", TestResultState::Passed, None);
        assert_eq!(own_state(r, &test_id), Some(TestResultState::Errored));
    });
}

#[test]
fn ignores_outside_run() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;
        let r = &fixture.r;

        fixture.changed.borrow_mut().clear();
        r.update_state(&id_of(&["ctrlId", "id-b"]), "t", TestResultState::Running, None);
        let mut c = make_empty_counts();
        c[TestResultState::Unset] = 4;
        assert_eq!(r.counts(), c);
        assert!(r.get_state_by_id(&id_of(&["ctrlId", "id-b"])).is_none());
    });
}

#[test]
fn mark_complete() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;
        let r = &fixture.r;

        r.set_all_to_state(TestResultState::Queued, "t", |_, _| true);
        r.update_state(&id_of(&["ctrlId", "id-a", "id-aa"]), "t", TestResultState::Passed, None);
        fixture.changed.borrow_mut().clear();

        r.mark_complete();

        let mut c = make_empty_counts();
        c[TestResultState::Skipped] = 3;
        c[TestResultState::Passed] = 1;
        assert_eq!(r.counts(), c);

        assert_eq!(own_state(r, &fixture.tests.root().id()), Some(TestResultState::Skipped));
        assert_eq!(own_state(r, &id_of(&["ctrlId", "id-a", "id-aa"])), Some(TestResultState::Passed));
    });
}

// suite('service')

struct Services {
    storage: Rc<InMemoryResultStorage>,
    results: Rc<TestResultService>,
}

fn test_storage_service(ds: &LeakGuard) -> Rc<dyn StorageService> {
    Rc::new(ds.add_rc(TestStorageService::new()))
}

fn test_profile_service(ds: &LeakGuard) -> Rc<dyn ITestProfileService> {
    let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
    ds.add_rc(TestProfileService::new(context_key_service, test_storage_service(ds)))
}

fn service_setup(ds: &LeakGuard) -> Services {
    let storage = ds.add_rc(InMemoryResultStorage::new(Rc::new(TestUriIdentityService), test_storage_service(ds), NullLogService::shared()));
    let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
    let results = ds.add_rc(TestResultService::new_with_immediate_persistence(
        context_key_service,
        storage.clone() as Rc<dyn ITestResultStorage>,
        test_profile_service(ds),
        null_telemetry_service(),
    ));

    Services { storage, results }
}

#[test]
fn pushes_new_result() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;
        let services = service_setup(&ds);

        services.results.push(fixture.r.clone());
        assert_eq!(addrs(&services.results.results()), vec![addr(&fixture.r)]);
    });
}

#[test]
fn serializes_and_re_hydrates() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;
        let r = &fixture.r;
        let services = service_setup(&ds);

        services.results.push(r.clone());
        r.update_state(&id_of(&["ctrlId", "id-a", "id-aa"]), "t", TestResultState::Passed, Some(42.0));
        r.mark_complete();
        timeout(10).await.unwrap(); // allow persistImmediately async to happen

        let context_key_service: Rc<dyn IContextKeyService> = MockContextKeyService::new();
        let results = ds.add_rc(TestResultService::new(
            context_key_service,
            services.storage.clone() as Rc<dyn ITestResultStorage>,
            test_profile_service(&ds),
            null_telemetry_service(),
        ));

        assert_eq!(0, results.results().len());
        timeout(10).await.unwrap(); // allow load promise to resolve
        assert_eq!(1, results.results().len());

        let root_id = fixture.tests.root().id();
        let (rehydrated, actual) = results.get_state_by_id(&root_id).unwrap();
        let live_root = r.get_state_by_id(&root_id).unwrap();
        let mut expected: TestResultItem = live_root.data.borrow().clone();
        expected.item.uri = actual.data.borrow().item.uri.clone();
        expected.retired = Some(true);
        assert_eq!(*actual.data.borrow(), expected);
        assert_eq!(rehydrated.counts(), r.counts());
        assert!(rehydrated.completed_at().is_some());
    });
}

#[test]
fn clears_results_but_keeps_ongoing_tests() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;
        let r = &fixture.r;
        let services = service_setup(&ds);
        let results = &services.results;

        results.push(r.clone());
        r.mark_complete();

        let r2 = results.push(LiveTestResult::new("", false, default_opts(&[]), next_insert_order(), null_telemetry_service()));
        results.clear();

        assert_eq!(addrs(&results.results()), vec![addr(&r2)]);
        assert!(r.is_disposed());
    });
}

#[test]
fn keeps_ongoing_tests_on_top_restored_order_when_done() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;
        let r = &fixture.r;
        let services = service_setup(&ds);
        let results = &services.results;

        results.push(r.clone());
        let r2 = results.push(LiveTestResult::new("", false, default_opts(&[]), next_insert_order(), null_telemetry_service()));

        assert_eq!(addrs(&results.results()), vec![addr(&r2), addr(r)]);
        r2.mark_complete();
        assert_eq!(addrs(&results.results()), vec![addr(r), addr(&r2)]);
        r.mark_complete();
        assert_eq!(addrs(&results.results()), vec![addr(&r2), addr(r)]);
    });
}

async fn make_hydrated(completed_at: f64, state: TestResultState) -> Rc<HydratedTestResult> {
    let collection = test_stubs::get_initialized_main_test_collection(test_stubs::nested(test_stubs::DEFAULT_ID_PREFIX)).await.unwrap();
    let node = collection.get_node_by_id(&id_of(&["ctrlId", "id-a"])).unwrap();
    let node = node.borrow();
    HydratedTestResult::new(
        &TestUriIdentityService,
        Rc::new(ISerializedTestResults {
            completed_at,
            id: "some-id".to_string(),
            tasks: vec![SerializedTestResultsTask { id: "t".to_string(), name: None, ctrl_id: "ctrl".to_string(), has_coverage: false }],
            name: "hello world".to_string(),
            request: default_opts(&[]),
            items: vec![TestResultItemSerialized {
                expand: node.expand,
                item: ITestItem::serialize(&node.item),
                tasks: vec![ITestTaskStateSerialized { state, duration: Some(0.0), messages: Vec::new() }],
                computed_state: state,
                own_computed_state: state,
            }],
        }),
    )
}

#[test]
fn pushes_hydrated_results() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;
        let services = service_setup(&ds);
        let results = &services.results;

        results.push(fixture.r.clone());
        let hydrated = make_hydrated(42.0, TestResultState::Passed).await;
        results.push(hydrated.clone());
        assert_eq!(addrs(&results.results()), vec![addr(&fixture.r), addr(&hydrated)]);
    });
}

#[test]
fn inserts_in_correct_order() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;
        let services = service_setup(&ds);
        let results = &services.results;

        results.push(fixture.r.clone());
        let hydrated1 = make_hydrated(42.0, TestResultState::Passed).await;
        results.push(hydrated1.clone());
        assert_eq!(addrs(&results.results()), vec![addr(&fixture.r), addr(&hydrated1)]);
    });
}

#[test]
fn inserts_in_correct_order_2() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;
        let services = service_setup(&ds);
        let results = &services.results;

        results.push(fixture.r.clone());
        let hydrated1 = make_hydrated(42.0, TestResultState::Passed).await;
        results.push(hydrated1.clone());
        let hydrated2 = make_hydrated(30.0, TestResultState::Passed).await;
        results.push(hydrated2.clone());
        assert_eq!(addrs(&results.results()), vec![addr(&fixture.r), addr(&hydrated1), addr(&hydrated2)]);
    });
}

#[test]
fn disposes_a_completed_result_that_is_immediately_evicted() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let _fixture = setup(&ds).await;
        let services = service_setup(&ds);
        let results = &services.results;

        let newer_completed_at = runtime::date_now() as f64 + 1000.0;
        for i in 0..128 {
            results.push(make_hydrated(newer_completed_at + f64::from(i), TestResultState::Passed).await);
        }

        let older = new_live(&ds, "older", false, default_opts(&[]));
        older.mark_complete();
        results.push(older.clone());

        let retained = results.results().iter().any(|result| addr(result) == addr(&older));
        assert_eq!((retained, older.is_disposed()), (false, true));
    });
}

#[test]
fn result_item_parents() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let fixture = setup(&ds).await;
        let r = &fixture.r;

        let item_addr = |id: &str| Rc::as_ptr(&r.get_state_by_id(id).unwrap()) as usize;

        let aa = r.get_state_by_id(&id_of(&["ctrlId", "id-a", "id-aa"])).unwrap();
        let parents: Vec<usize> = result_item_parents(r.as_ref(), &aa).iter().map(|item| Rc::as_ptr(item) as usize).collect();
        assert_eq!(parents, vec![item_addr(&id_of(&["ctrlId", "id-a", "id-aa"])), item_addr(&id_of(&["ctrlId", "id-a"])), item_addr(&id_of(&["ctrlId"]))]);

        let root = r.get_state_by_id(&fixture.tests.root().id()).unwrap();
        let parents: Vec<usize> = result_item_parents(r.as_ref(), &root).iter().map(|item| Rc::as_ptr(item) as usize).collect();
        assert_eq!(parents, vec![item_addr(&fixture.tests.root().id())]);
    });
}

// suite('output controller')

#[test]
fn reads_live_output_ranges() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let _fixture = setup(&ds).await;
        let ctrl = TaskRawOutput::new();

        ctrl.append(&VSBuffer::from_string("12345"), None);
        ctrl.append(&VSBuffer::from_string("67890"), None);
        ctrl.append(&VSBuffer::from_string("12345"), None);
        ctrl.append(&VSBuffer::from_string("67890"), None);

        use je_workbench::contrib::testing::common::test_result::ITaskRawOutput;
        assert_eq!(ctrl.get_range(0, 5).to_vec(), VSBuffer::from_string("12345").to_vec());
        assert_eq!(ctrl.get_range(5, 5).to_vec(), VSBuffer::from_string("67890").to_vec());
        assert_eq!(ctrl.get_range(7, 6).to_vec(), VSBuffer::from_string("890123").to_vec());
        assert_eq!(ctrl.get_range(15, 5).to_vec(), VSBuffer::from_string("67890").to_vec());
        assert_eq!(ctrl.get_range(15, 10).to_vec(), VSBuffer::from_string("67890").to_vec());
    });
}

#[test]
fn corrects_offsets_for_marked_ranges() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let _fixture = setup(&ds).await;
        let ctrl = TaskRawOutput::new();

        let a1 = ctrl.append(&VSBuffer::from_string("12345"), Some(1));
        let a2 = ctrl.append(&VSBuffer::from_string("67890"), Some(1234));
        let a3 = ctrl.append(&VSBuffer::from_string("with new line\r\n"), Some(4));

        use je_workbench::contrib::testing::common::test_result::ITaskRawOutput;
        assert_eq!(
            ctrl.get_range(a1.offset, a1.length).to_vec(),
            VSBuffer::from_string("\x1b]633;SetMark;Id=s1;Hidden\x0712345\x1b]633;SetMark;Id=e1;Hidden\x07").to_vec()
        );
        assert_eq!(
            ctrl.get_range(a2.offset, a2.length).to_vec(),
            VSBuffer::from_string("\x1b]633;SetMark;Id=s1234;Hidden\x0767890\x1b]633;SetMark;Id=e1234;Hidden\x07").to_vec()
        );
        assert_eq!(
            ctrl.get_range(a3.offset, a3.length).to_vec(),
            VSBuffer::from_string("\x1b]633;SetMark;Id=s4;Hidden\x07with new line\x1b]633;SetMark;Id=e4;Hidden\x07\r\n").to_vec()
        );
    });
}
