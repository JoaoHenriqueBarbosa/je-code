//! Porte de `src/vs/workbench/contrib/testing/test/common/testProfileService.test.ts` (suíte `Workbench - TestProfileService`),
//! caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso e o `setup` ([`setup`]) logo depois, com o `ds` do original como o próprio guarda. O `teardown`
//!   (`ds.dispose()`) e a conferência de vazamentos rodam no `Drop` do guarda.
//! - **`idCounter`** é o [`Cell`] do `Fixture`, zerado a cada `setup`.
//! - **`addProfile(profile: Partial<ITestRunProfile>)`** recebe um [`PartialTestRunProfile`] e aplica os campos presentes
//!   sobre os padrões do original (`...profile`).
//! - **`upcastPartial<IMainThreadTestController>({ id: 'ctrlId' })`** é o [`StubController`]: só `id` responde, e os demais
//!   membros lançam `X is not a function`.
//! - **`MockContextKeyService`** é o do porte e `TestStorageService` é o `InMemoryStorageService`, entregue ao serviço como
//!   `Rc<dyn StorageService>`.
//! - **`assert.deepStrictEqual(a.map(label), b.map(label))`** compara listas de textos (`Vec<String>`).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::cancellation::CancellationToken;
use je_base::errors::{Error, throw};
use je_base::observable::IObservable;
use je_base::promise::Promise;
use je_platform::contextkey::IContextKeyService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_platform::storage::StorageService;
use je_workbench::contrib::testing::common::test_profile_service::{ITestProfileService, TestProfileService};
use je_workbench::contrib::testing::common::test_service::{IMainThreadTestController, Location};
use je_workbench::contrib::testing::common::test_types::{
    ICallProfileRunHandler, IStartControllerTests, IStartControllerTestsResult, ITestRunProfile, PartialTestRunProfile, TestControllerCapability,
    TestRunProfileBitset, test_run_profile_bitset,
};
use je_workbench::test::common::workbench_test_services::TestStorageService;

const DEBUG: TestRunProfileBitset = test_run_profile_bitset::DEBUG;
const RUN: TestRunProfileBitset = test_run_profile_bitset::RUN;

/// `upcastPartial<IMainThreadTestController>({ id: 'ctrlId' })`.
struct StubController {
    id: &'static str,
}

impl IMainThreadTestController for StubController {
    fn id(&self) -> String {
        self.id.to_owned()
    }
    fn label(&self) -> IObservable<String> {
        throw(Error::new("label is not a function"))
    }
    fn capabilities(&self) -> IObservable<TestControllerCapability> {
        throw(Error::new("capabilities is not a function"))
    }
    fn sync_tests(&self, _token: CancellationToken) -> Promise<()> {
        throw(Error::new("syncTests is not a function"))
    }
    fn refresh_tests(&self, _token: CancellationToken) -> Promise<()> {
        throw(Error::new("refreshTests is not a function"))
    }
    fn configure_run_profile(&self, _profile_id: u32) {
        throw(Error::new("configureRunProfile is not a function"))
    }
    fn expand_test(&self, _id: &str, _levels: f64) -> Promise<()> {
        throw(Error::new("expandTest is not a function"))
    }
    fn get_related_code(&self, _test_id: &str, _token: CancellationToken) -> Promise<Vec<Location>> {
        throw(Error::new("getRelatedCode is not a function"))
    }
    fn start_continuous_run(&self, _request: Vec<ICallProfileRunHandler>, _token: CancellationToken) -> Promise<Vec<IStartControllerTestsResult>> {
        throw(Error::new("startContinuousRun is not a function"))
    }
    fn run_tests(&self, _request: Vec<IStartControllerTests>, _token: CancellationToken) -> Promise<Vec<IStartControllerTestsResult>> {
        throw(Error::new("runTests is not a function"))
    }
}

struct Fixture {
    _ds: LeakGuard,
    t: Rc<TestProfileService>,
    id_counter: Cell<u32>,
}

fn setup() -> Fixture {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let context_key_service = ds.add_rc(MockContextKeyService::new());
    let context_key_service: Rc<dyn IContextKeyService> = context_key_service;
    let storage_service: Rc<dyn StorageService> = Rc::new(ds.add_rc(TestStorageService::new()));
    let t = ds.add_rc(TestProfileService::new(context_key_service, storage_service));
    Fixture { _ds: ds, t, id_counter: Cell::new(0) }
}

fn add_profile(f: &Fixture, profile: PartialTestRunProfile) -> ITestRunProfile {
    let profile_id = f.id_counter.get();
    f.id_counter.set(profile_id + 1);
    let defaults = ITestRunProfile {
        controller_id: "ctrlId".to_owned(),
        group: RUN,
        is_default: true,
        label: "profile".to_owned(),
        profile_id,
        has_configuration_handler: false,
        tag: None,
        supports_continuous_run: false,
    };
    let p = ITestRunProfile {
        controller_id: profile.controller_id.unwrap_or(defaults.controller_id),
        group: profile.group.unwrap_or(defaults.group),
        is_default: profile.is_default.unwrap_or(defaults.is_default),
        label: profile.label.unwrap_or(defaults.label),
        profile_id: profile.profile_id.unwrap_or(defaults.profile_id),
        has_configuration_handler: profile.has_configuration_handler.unwrap_or(defaults.has_configuration_handler),
        tag: profile.tag.unwrap_or(defaults.tag),
        supports_continuous_run: profile.supports_continuous_run.unwrap_or(defaults.supports_continuous_run),
    };

    f.t.add_profile(Rc::new(StubController { id: "ctrlId" }), p.clone());
    p
}

/// Atalho para o `{ isDefault, controllerId?, group, label }` dos casos.
fn spec(is_default: bool, controller_id: Option<&str>, group: TestRunProfileBitset, label: &str) -> PartialTestRunProfile {
    PartialTestRunProfile {
        is_default: Some(is_default),
        controller_id: controller_id.map(str::to_owned),
        group: Some(group),
        label: Some(label.to_owned()),
        ..PartialTestRunProfile::default()
    }
}

fn labels(profiles: &[ITestRunProfile]) -> Vec<String> {
    profiles.iter().map(|p| p.label.clone()).collect()
}

fn assert_group_defaults(f: &Fixture, group: TestRunProfileBitset, expected: &[&ITestRunProfile]) {
    let actual: Vec<String> = f.t.get_group_default_profiles(group, None).iter().map(|p| p.label.clone()).collect();
    assert_eq!(actual, expected.iter().map(|e| e.label.clone()).collect::<Vec<_>>());
}

fn expect_profiles(expected: &[ITestRunProfile], actual: &[&str]) {
    let mut e = labels(expected);
    e.sort();
    let mut a: Vec<String> = actual.iter().map(|s| (*s).to_owned()).collect();
    a.sort();
    assert_eq!(e, a);
}

#[test]
fn get_group_default_profiles() {
    let f = setup();
    add_profile(&f, spec(true, None, DEBUG, "a"));
    add_profile(&f, spec(false, None, DEBUG, "b"));
    add_profile(&f, spec(true, None, RUN, "c"));
    add_profile(&f, spec(true, Some("2"), RUN, "d"));
    add_profile(&f, spec(false, Some("2"), RUN, "e"));
    expect_profiles(&f.t.get_group_default_profiles(RUN, None), &["c", "d"]);
    expect_profiles(&f.t.get_group_default_profiles(DEBUG, None), &["a"]);
}

#[test]
fn set_group_default_profiles_applies_simple_changes() {
    let f = setup();
    let p1 = add_profile(&f, spec(false, None, DEBUG, "a"));
    add_profile(&f, spec(false, None, DEBUG, "b"));
    let p3 = add_profile(&f, spec(false, None, RUN, "c"));
    add_profile(&f, spec(false, None, RUN, "d"));

    f.t.set_group_default_profiles(RUN, &[p3.clone()]);
    assert_group_defaults(&f, RUN, &[&p3]);
    assert_group_defaults(&f, DEBUG, &[&p1]);
}

#[test]
fn set_group_default_profiles_syncs_labels_if_same() {
    let f = setup();
    let p1 = add_profile(&f, spec(false, None, DEBUG, "a"));
    let p2 = add_profile(&f, spec(false, None, DEBUG, "b"));
    let p3 = add_profile(&f, spec(false, None, RUN, "a"));
    let p4 = add_profile(&f, spec(false, None, RUN, "b"));

    f.t.set_group_default_profiles(RUN, &[p3.clone()]);
    assert_group_defaults(&f, RUN, &[&p3]);
    assert_group_defaults(&f, DEBUG, &[&p1]);

    f.t.set_group_default_profiles(DEBUG, &[p2.clone()]);
    assert_group_defaults(&f, RUN, &[&p4]);
    assert_group_defaults(&f, DEBUG, &[&p2]);
}

#[test]
fn set_group_default_profiles_does_not_mess_up_sync_for_multiple_controllers() {
    let f = setup();
    // ctrl a and b both of have their own labels. ctrl c does not and should be unaffected
    let p1 = add_profile(&f, spec(false, Some("a"), DEBUG, "a"));
    let p2 = add_profile(&f, spec(false, Some("b"), DEBUG, "b1"));
    let p3 = add_profile(&f, spec(false, Some("b"), DEBUG, "b2"));
    let p4 = add_profile(&f, spec(false, Some("c"), DEBUG, "c1"));

    let p5 = add_profile(&f, spec(false, Some("a"), RUN, "a"));
    let p6 = add_profile(&f, spec(false, Some("b"), RUN, "b1"));
    let p7 = add_profile(&f, spec(false, Some("b"), RUN, "b2"));
    let p8 = add_profile(&f, spec(false, Some("b"), RUN, "b3"));

    // same profile on both
    f.t.set_group_default_profiles(DEBUG, &[p3.clone()]);
    assert_group_defaults(&f, RUN, &[&p7]);
    assert_group_defaults(&f, DEBUG, &[&p3]);

    // different profile, other should be unaffected
    f.t.set_group_default_profiles(RUN, &[p8.clone()]);
    assert_group_defaults(&f, RUN, &[&p8]);
    assert_group_defaults(&f, DEBUG, &[&p5]);

    // multiple changes in one go, with unmatched c
    f.t.set_group_default_profiles(DEBUG, &[p1.clone(), p2.clone(), p4.clone()]);
    assert_group_defaults(&f, RUN, &[&p5, &p6, &p8]);
    assert_group_defaults(&f, DEBUG, &[&p1, &p2, &p4]);

    // identity
    f.t.set_group_default_profiles(RUN, &[p5.clone(), p6.clone(), p8.clone()]);
    assert_group_defaults(&f, RUN, &[&p5, &p6, &p8]);
    assert_group_defaults(&f, DEBUG, &[&p1, &p2, &p4]);
}
