//! Porte de `src/vs/workbench/contrib/testing/test/common/testResultStorage.test.ts` (suíte `Workbench - Test Result
//! Storage`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso e o `setup` ([`setup`]) logo depois, com o `ds` do original como o próprio guarda. O `teardown`
//!   (`ds.dispose()`) e a conferência de vazamentos rodam no `Drop` do guarda.
//! - **Casos `async`** rodam em `run_with_faked_timers`; `await` é `.await.unwrap()` (uma rejeição reprova o caso). O
//!   relógio de parede é fixado com `runtime::set_system_time` no `setup`, porque sob o relógio virtual o `Date.now()`
//!   começa em 0 e o `completedAt` de um resultado completo precisa ser diferente de zero para o `toJSON` persistir
//!   (o original roda no relógio real).
//! - **`makeResult`** chama `tests.expand(tests.root.id, Infinity)` sem `await`, como o original: a expansão do `nested`
//!   é síncrona e já acontece na chamada.
//! - **`range(n).map(() => makeResult())`** é `(0..n).map(...)`. Os resultados têm todos o `id` vazio (o original
//!   passa `''`): o `assertStored` compara a lista de `id`s, que é uma lista de textos vazios do tamanho esperado.
//! - **`{ asCanonicalUri(uri) { return uri; } } as IUriIdentityService`** é o `TestUriIdentityService` de
//!   `support::test_stubs`; `TestStorageService` é o `InMemoryStorageService`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::runtime;
use je_platform::log::NullLogService;
use je_platform::storage::StorageService;
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_workbench::contrib::testing::common::test_result::{ITestResult, LiveTestResult};
use je_workbench::contrib::testing::common::test_result_storage::{ITestResultStorage, InMemoryResultStorage, RETAIN_MAX_RESULTS};
use je_workbench::contrib::testing::common::test_types::{ITestRunTask, ResolvedTestRunRequest, test_run_profile_bitset};
use je_workbench::test::common::workbench_test_services::TestStorageService;
use support::test_stubs::{self, TestUriIdentityService};

const REAL_CLOCK_MS: u64 = 1_700_000_000_000;

fn setup(ds: &LeakGuard) -> Rc<InMemoryResultStorage> {
    runtime::set_system_time(REAL_CLOCK_MS);
    let storage_service: Rc<dyn StorageService> = Rc::new(ds.add_rc(TestStorageService::new()));
    ds.add_rc(InMemoryResultStorage::new(Rc::new(TestUriIdentityService), storage_service, NullLogService::shared()))
}

fn make_result(ds: &LeakGuard, task_name: &str) -> Rc<LiveTestResult> {
    let t = ds.add_rc(LiveTestResult::new(
        "",
        true,
        ResolvedTestRunRequest { targets: Vec::new(), group: test_run_profile_bitset::RUN, exclude: None, continuous: None, preserve_focus: None },
        1,
        null_telemetry_service(),
    ));

    t.add_task(ITestRunTask { id: task_name.to_string(), name: "n".to_string(), running: true, ctrl_id: "ctrlId".to_string() });
    let tests = ds.add_rc(test_stubs::nested(test_stubs::DEFAULT_ID_PREFIX));
    tests.expand(&tests.root().id(), f64::INFINITY);
    let root = tests.root().clone();
    let a = root.children().get("id-a").unwrap();
    let aa = a.children().get("id-aa").unwrap();
    t.add_test_chain_to_run("ctrlId", &[root.to_test_item(), a.to_test_item(), aa.to_test_item()]);

    t.mark_complete();
    t
}

// ITestChildrenLike::get
use je_workbench::contrib::testing::common::test_item_collection::ITestChildrenLike;

fn as_results(results: &[Rc<LiveTestResult>]) -> Vec<Rc<dyn ITestResult>> {
    results.iter().map(|r| r.clone() as Rc<dyn ITestResult>).collect()
}

async fn assert_stored(storage: &InMemoryResultStorage, stored: &[Rc<LiveTestResult>]) {
    let read: Vec<String> = storage.read().await.unwrap().iter().map(|r| r.id()).collect();
    assert_eq!(read, stored.iter().map(|s| s.id.clone()).collect::<Vec<_>>());
}

#[test]
fn stores_a_single_result() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let storage = setup(&ds);

        let r: Vec<Rc<LiveTestResult>> = (0..5).map(|_| make_result(&ds, "t")).collect();
        storage.persist(&as_results(&r)).await.unwrap();
        assert_stored(&storage, &r).await;
    });
}

#[test]
fn deletes_old_results() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let storage = setup(&ds);

        let r: Vec<Rc<LiveTestResult>> = (0..5).map(|_| make_result(&ds, "t")).collect();
        storage.persist(&as_results(&r)).await.unwrap();
        let mut r2 = vec![make_result(&ds, "t")];
        r2.extend(r[..3].iter().cloned());
        storage.persist(&as_results(&r2)).await.unwrap();
        assert_stored(&storage, &r2).await;
    });
}

#[test]
fn limits_stored_results() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let storage = setup(&ds);

        let r: Vec<Rc<LiveTestResult>> = (0..100).map(|_| make_result(&ds, "t")).collect();
        storage.persist(&as_results(&r)).await.unwrap();
        assert_stored(&storage, &r[..RETAIN_MAX_RESULTS.min(r.len())]).await;
    });
}

#[test]
fn limits_stored_result_by_budget() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let storage = setup(&ds);

        let r: Vec<Rc<LiveTestResult>> = (0..100).map(|_| make_result(&ds, &"a".repeat(2048))).collect();
        storage.persist(&as_results(&r)).await.unwrap();
        let length = storage.read().await.unwrap().len();
        assert!(length < 50);
    });
}

#[test]
fn always_stores_the_min_number_of_results() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let storage = setup(&ds);

        let r: Vec<Rc<LiveTestResult>> = (0..20).map(|_| make_result(&ds, &"a".repeat(1024 * 10))).collect();
        storage.persist(&as_results(&r)).await.unwrap();
        assert_stored(&storage, &r[..16]).await;
    });
}

#[test]
fn takes_into_account_existing_stored_bytes() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let storage = setup(&ds);

        let r: Vec<Rc<LiveTestResult>> = (0..10).map(|_| make_result(&ds, &"a".repeat(1024 * 10))).collect();
        storage.persist(&as_results(&r)).await.unwrap();
        assert_stored(&storage, &r).await;

        let mut r2 = r.clone();
        r2.extend((0..10).map(|_| make_result(&ds, &"a".repeat(1024 * 10))));
        storage.persist(&as_results(&r2)).await.unwrap();
        assert_stored(&storage, &r2[..16]).await;
    });
}
