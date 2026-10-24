//! Porte de `src/vs/workbench/contrib/testing/test/common/testingContinuousRunService.test.ts` (suíte
//! `TestingContinuousRunService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`setup` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda é criado no começo de cada caso e o `setup` monta o
//!   serviço sobre o `MockTestService`, um `TestStorageService`, o `MockContextKeyService` e o `MockProfilesService`; o `ds`
//!   do original é o store do guarda.
//! - **`mock<ITestService>()` e `mock<ITestProfileService>()`** são os dublês de `support::test_service_mocks`: só o que o
//!   caso sobrescreve responde, e os demais membros lançam `X is not a function`.
//! - **`testService.log`** é uma lista de `(tipo, profileId, testIds)`; o `requests` do original (um `Set` que ninguém lê
//!   nos casos) não é guardado.
//! - **`'testId\0child'`** é o texto com o byte nulo, o delimitador do `TestId`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::storage::StorageService;
use je_base::promise::Promise;
use je_workbench::contrib::testing::common::test_types::{ITestRunProfile, ResolvedTestRunRequest, test_run_profile_bitset};
use je_workbench::contrib::testing::common::testing_continuous_run_service::{ContinuousRunProfiles, ITestingContinuousRunService, TestingContinuousRunService};
use je_workbench::test::common::workbench_test_services::TestStorageService;
use support::test_service_mocks::{MockProfilesService, MockTestService};

type Log = Rc<RefCell<Vec<(&'static str, u32, Vec<String>)>>>;

struct Fixture {
    _ds: LeakGuard,
    log: Log,
    cr: Rc<TestingContinuousRunService>,
}

fn profile(profile_id: u32) -> ITestRunProfile {
    ITestRunProfile {
        profile_id,
        controller_id: "ctrl".to_owned(),
        group: test_run_profile_bitset::RUN,
        label: "label".to_owned(),
        supports_continuous_run: true,
        is_default: false,
        has_configuration_handler: true,
        tag: None,
    }
}

fn start(f: &Fixture, profiles: Vec<ITestRunProfile>, test_id: &str) {
    f.cr.start(ContinuousRunProfiles::Profiles(profiles), Some(test_id));
}

fn setup() -> Fixture {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let log: Log = Rc::default();

    let test_service = Rc::new(MockTestService::default());
    let (log_for_start, store) = (log.clone(), ds.store());
    *test_service.start_continuous_run.borrow_mut() = Some(Box::new(move |req: ResolvedTestRunRequest, token| {
        let target = req.targets[0].clone();
        log_for_start.borrow_mut().push(("start", target.profile_id, target.test_ids.clone()));
        let log_for_stop = log_for_start.clone();
        store.add(token.on_cancellation_requested().listen(move |_| {
            log_for_stop.borrow_mut().push(("stop", target.profile_id, target.test_ids.clone()));
        }));
        Promise::resolved(())
    }));

    let storage_service: Rc<dyn StorageService> = Rc::new(ds.add_rc(TestStorageService::new()));
    let context_key_service = ds.add_rc(MockContextKeyService::new());
    let profiles_service = ds.add_rc(MockProfilesService::new());
    let cr = ds.add_rc(TestingContinuousRunService::new(test_service, storage_service, context_key_service, profiles_service));
    Fixture { _ds: ds, log, cr }
}

fn entry(kind: &'static str, profile_id: u32, test_ids: &[&str]) -> (&'static str, u32, Vec<String>) {
    (kind, profile_id, test_ids.iter().map(|id| (*id).to_owned()).collect())
}

#[test]
fn is_specifically_enabled_for() {
    let f = setup();
    assert!(!f.cr.is_enabled());
    assert!(!f.cr.is_specifically_enabled_for("testId"));

    start(&f, vec![profile(1)], "testId\0child");
    assert!(!f.cr.is_specifically_enabled_for("testId"));
    assert!(f.cr.is_specifically_enabled_for("testId\0child"));

    assert_eq!(*f.log.borrow(), vec![entry("start", 1, &["testId\0child"])]);
}

#[test]
fn is_enabled_for_a_parent_of() {
    let f = setup();
    assert!(!f.cr.is_enabled());
    assert!(!f.cr.is_enabled_for_a_parent_of("testId"));
    start(&f, vec![profile(1)], "parentTestId\0testId");
    assert!(!f.cr.is_enabled_for_a_parent_of("parentTestId"));
    assert!(f.cr.is_enabled_for_a_parent_of("parentTestId\0testId"));
    assert!(f.cr.is_enabled_for_a_parent_of("parentTestId\0testId\0nestd"));
    assert!(f.cr.is_enabled());

    assert_eq!(*f.log.borrow(), vec![entry("start", 1, &["parentTestId\0testId"])]);
}

#[test]
fn is_enabled_for_a_child_of() {
    let f = setup();
    assert!(!f.cr.is_enabled());
    assert!(!f.cr.is_enabled_for_a_child_of("testId"));
    start(&f, vec![profile(1)], "testId\0childTestId");
    assert!(f.cr.is_enabled_for_a_child_of("testId"));
    assert!(f.cr.is_enabled_for_a_child_of("testId\0childTestId"));
    assert!(!f.cr.is_enabled_for_a_child_of("testId\0childTestId\0neested"));
    assert!(f.cr.is_enabled());
}

#[test]
fn lifecycle_stops_general_in_dfs_order() {
    let f = setup();
    start(&f, vec![profile(1)], "a\0b\0c\0d");
    start(&f, vec![profile(1)], "a\0b");
    start(&f, vec![profile(1)], "a\0b\0c");
    f.cr.stop(None);
    assert_eq!(
        *f.log.borrow(),
        vec![
            entry("start", 1, &["a\0b\0c\0d"]),
            entry("start", 1, &["a\0b"]),
            entry("start", 1, &["a\0b\0c"]),
            entry("stop", 1, &["a\0b\0c\0d"]),
            entry("stop", 1, &["a\0b\0c"]),
            entry("stop", 1, &["a\0b"]),
        ]
    );
    assert!(!f.cr.is_enabled());
}

#[test]
fn lifecycle_stops_profiles_in_dfs_order() {
    let f = setup();
    start(&f, vec![profile(1)], "a\0b\0c\0d");
    start(&f, vec![profile(1)], "a\0b");
    start(&f, vec![profile(1)], "a\0b\0c");
    f.cr.stop_profile(&profile(1));
    assert_eq!(
        *f.log.borrow(),
        vec![
            entry("start", 1, &["a\0b\0c\0d"]),
            entry("start", 1, &["a\0b"]),
            entry("start", 1, &["a\0b\0c"]),
            entry("stop", 1, &["a\0b\0c\0d"]),
            entry("stop", 1, &["a\0b\0c"]),
            entry("stop", 1, &["a\0b"]),
        ]
    );
    assert!(!f.cr.is_enabled());
}

#[test]
fn lifecycle_updates_profile_for_a_test_if_profile_is_changed() {
    let f = setup();
    start(&f, vec![profile(1)], "parent\0testId");
    start(&f, vec![profile(2)], "parent\0testId");
    assert!(f.cr.is_enabled());
    f.cr.stop(None);
    assert!(!f.cr.is_enabled());
    assert_eq!(
        *f.log.borrow(),
        vec![
            entry("start", 1, &["parent\0testId"]),
            entry("start", 2, &["parent\0testId"]),
            entry("stop", 1, &["parent\0testId"]),
            entry("stop", 2, &["parent\0testId"]),
        ]
    );
}

#[test]
fn lifecycle_stops_a_single_profile_test() {
    let f = setup();
    start(&f, vec![profile(1), profile(2)], "parent\0testId");
    f.cr.stop_profile(&profile(1));
    assert_eq!(
        *f.log.borrow(),
        vec![entry("start", 1, &["parent\0testId"]), entry("start", 2, &["parent\0testId"]), entry("stop", 1, &["parent\0testId"])]
    );
    assert!(f.cr.is_enabled());

    f.cr.stop_profile(&profile(2));
    assert_eq!(
        *f.log.borrow(),
        vec![
            entry("start", 1, &["parent\0testId"]),
            entry("start", 2, &["parent\0testId"]),
            entry("stop", 1, &["parent\0testId"]),
            entry("stop", 2, &["parent\0testId"]),
        ]
    );
    assert!(!f.cr.is_enabled());
}
