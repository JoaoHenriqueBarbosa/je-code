//! Porte de `src/vs/workbench/contrib/testing/test/common/testCoverage.test.ts` (suíte `TestCoverage`), caso a caso e na
//! mesma ordem.
//!
//! Divergências:
//! - **`setup`, `teardown` e `ensureNoDisposablesAreLeakedInTestSuite()`.** O guarda de vazamentos é criado no começo de
//!   cada caso e o `setup` ([`setup`]) logo depois; o `ds` do original é o próprio guarda. O `sandbox` do sinon e o
//!   `sandbox.restore()` não existem: o `getCoverageDetails: sandbox.stub().resolves([])` é o [`ResolvingAccessor`], que
//!   resolve uma lista vazia.
//! - **`{} as LiveTestResult`** é um `LiveTestResult` real e vazio, mantido vivo pelo `Fixture` (a cobertura só guarda um
//!   `Weak` dele e nenhum caso o lê).
//! - **`upcastDeepPartial<IUriIdentityService>({ extUri: upcastPartial({ ignorePathCasing: () => true }) })`** é o
//!   `PartialUriIdentityService` de `support::test_stubs` com um `ExtUri` cujo `ignorePathCasing` é sempre verdadeiro; o
//!   `asCanonicalUri` não é definido no original e não é usado pela cobertura.
//! - **`onObservableChange(testCoverage.didAddCoverage, ...)`** é [`on_observable_change`]; o delta do `didAddCoverage` do
//!   porte é a lista dos valores dos nós da cadeia, então o `v.value!.uri` do original é `v.uri`.
//! - **Os casos `async`** não esperam nada e são funções síncronas.
//! - **`assert.deepEqual` sobre contagens** compara os [`ICoverageCount`] (`Option` para `branch` e `declaration`, que são
//!   opcionais); `assert.strictEqual` sobre nós é a identidade do `Rc`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::resources::{host_environment, uri_file};
use je_platform::telemetry::telemetry_utils::null_telemetry_service;
use je_workbench::contrib::testing::common::observable_utils::on_observable_change;
use je_workbench::contrib::testing::common::test_coverage::{AbstractFileCoverage, ICoverageAccessor, TestCoverage};
use je_workbench::contrib::testing::common::test_result::LiveTestResult;
use je_workbench::contrib::testing::common::test_types::{CoverageDetails, ICoverageCount, IFileCoverage, ResolvedTestRunRequest, test_run_profile_bitset};
use support::test_stubs::PartialUriIdentityService;

/// `{ getCoverageDetails: sandbox.stub().resolves([]) }`.
struct ResolvingAccessor;

impl ICoverageAccessor for ResolvingAccessor {
    fn get_coverage_details(&self, _id: &str, _test_id: Option<&str>, _token: CancellationToken) -> Promise<Vec<CoverageDetails>> {
        Promise::resolved(Vec::new())
    }
}

struct Fixture {
    ds: LeakGuard,
    _result: Rc<LiveTestResult>,
    test_coverage: Rc<TestCoverage>,
}

fn setup() -> Fixture {
    let ds = ensure_no_disposables_are_leaked_in_test_suite();
    let result = ds.add_rc(LiveTestResult::new(
        "",
        false,
        ResolvedTestRunRequest { targets: Vec::new(), group: test_run_profile_bitset::RUN, exclude: None, continuous: None, preserve_focus: None },
        1,
        null_telemetry_service(),
    ));
    let uri_identity_service = Rc::new(PartialUriIdentityService {
        canonical: Box::new(|uri: &URI| uri.clone()),
        ext_uri: Some(ExtUri::new(host_environment(), |_| true)),
    });
    let test_coverage = TestCoverage::new(&result, "taskId", uri_identity_service, Rc::new(ResolvingAccessor));
    Fixture { ds, _result: result, test_coverage }
}

fn count(covered: f64, total: f64) -> ICoverageCount {
    ICoverageCount { covered, total }
}

fn add_tests(f: &Fixture) -> (IFileCoverage, IFileCoverage) {
    let raw1 = IFileCoverage {
        id: "1".to_owned(),
        uri: uri_file("/path/to/file"),
        test_ids: None,
        statement: count(10.0, 20.0),
        branch: Some(count(5.0, 10.0)),
        declaration: Some(count(2.0, 5.0)),
    };

    f.test_coverage.append(&raw1, None);

    let raw2 = IFileCoverage {
        id: "1".to_owned(),
        uri: uri_file("/path/to/file2"),
        test_ids: None,
        statement: count(5.0, 10.0),
        branch: Some(count(1.0, 5.0)),
        declaration: None,
    };

    f.test_coverage.append(&raw2, None);

    (raw1, raw2)
}

#[test]
fn should_look_up_file_coverage() {
    let f = setup();
    let (raw1, _raw2) = add_tests(&f);

    let file_coverage = f.test_coverage.get_uri(&raw1.uri).unwrap();
    assert_eq!(*file_coverage.id.borrow(), raw1.id);
    assert_eq!(Some(file_coverage.statement.get()), Some(raw1.statement));
    assert_eq!(file_coverage.branch.get(), raw1.branch);
    assert_eq!(file_coverage.declaration.get(), raw1.declaration);

    assert!(Rc::ptr_eq(&f.test_coverage.get_computed_for_uri(&raw1.uri).unwrap(), &f.test_coverage.get_uri(&raw1.uri).unwrap()));
    assert!(f.test_coverage.get_computed_for_uri(&uri_file("/path/to/x")).is_none());
    assert!(f.test_coverage.get_uri(&uri_file("/path/to/x")).is_none());
}

#[test]
fn should_compute_coverage_for_directories() {
    let f = setup();
    let (raw1, _raw2) = add_tests(&f);
    let dir_coverage = f.test_coverage.get_computed_for_uri(&uri_file("/path/to")).unwrap();
    assert_eq!(dir_coverage.statement.get(), count(15.0, 30.0));
    assert_eq!(dir_coverage.branch.get(), Some(count(6.0, 15.0)));
    assert_eq!(dir_coverage.declaration.get(), raw1.declaration);
}

#[test]
fn should_incrementally_diff_updates_to_existing_files() {
    let f = setup();
    add_tests(&f);

    let raw3 = IFileCoverage {
        id: "1".to_owned(),
        uri: uri_file("/path/to/file"),
        test_ids: None,
        statement: count(12.0, 24.0),
        branch: Some(count(7.0, 10.0)),
        declaration: Some(count(2.0, 5.0)),
    };

    f.test_coverage.append(&raw3, None);

    let file_coverage = f.test_coverage.get_uri(&raw3.uri).unwrap();
    assert_eq!(file_coverage.statement.get(), raw3.statement);
    assert_eq!(file_coverage.branch.get(), raw3.branch);
    assert_eq!(file_coverage.declaration.get(), raw3.declaration);

    let dir_coverage = f.test_coverage.get_computed_for_uri(&uri_file("/path/to")).unwrap();
    assert_eq!(dir_coverage.statement.get(), count(17.0, 34.0));
    assert_eq!(dir_coverage.branch.get(), Some(count(8.0, 15.0)));
    assert_eq!(dir_coverage.declaration.get(), raw3.declaration);
}

#[test]
fn should_emit_changes() {
    let f = setup();
    let changes: Rc<RefCell<Vec<Vec<String>>>> = Rc::default();
    let sink = changes.clone();
    f.ds.add(on_observable_change(&f.test_coverage.did_add_coverage, move |value: &Vec<Rc<AbstractFileCoverage>>| {
        sink.borrow_mut().push(value.iter().map(|v| v.uri.to_string(false).to_string_lossy()).collect());
    }));

    add_tests(&f);

    let expected: Vec<Vec<&str>> = vec![
        vec!["file:///", "file:///", "file:///", "file:///path", "file:///path/to", "file:///path/to/file"],
        vec!["file:///", "file:///", "file:///", "file:///path", "file:///path/to", "file:///path/to/file2"],
    ];
    assert_eq!(*changes.borrow(), expected.iter().map(|c| c.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>()).collect::<Vec<_>>());
}
