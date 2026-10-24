//! Porte de `src/vs/workbench/contrib/testing/test/common/testService.test.ts` (suíte `Workbench - Test Service`, com as
//! suítes `simplifyTestsToExecute`, `testsInFile` e `waitForTestToBeIdle`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o `LeakGuard`, criado no começo de cada caso; o `ds` do original é
//!   o store do guarda.
//! - **`upcastPartial<ITestService>({ collection, onDidProcessDiff })`** é o `MockTestService` de
//!   `support::test_service_mocks`, com a `collection` e o `onDidProcessDiff` dados; **`upcastPartial<IUriIdentityService>`**
//!   é o `PartialUriIdentityService` de `support::test_stubs`.
//! - **`for await (const batch of testsInFile(...))`** é `for_await` sobre o `AsyncIterable` devolvido; o iterador manual
//!   (`[Symbol.asyncIterator]().next()`) é `async_iterator().next()`.
//! - **`assert.rejects(p, /Discovery failed/)` e `assert.rejects(p, CancellationError)`** são a conferência do erro da
//!   promessa rejeitada (`to_string` contém o texto, ou `is_cancellation_error`).
//! - **`await new Promise<void>(() => { })`** (uma promessa que nunca assenta) é uma promessa cujo `Resolver` fica guardado
//!   no teste e nunca é usado.
//! - **`diffs.hasListeners()`** é `Emitter::has_listeners`.
//! - Os `apply` com `uri: undefined`, `range: null`, `description: null`... têm `None` nos campos opcionais do `ITestItem`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::{Cell, RefCell};
use std::ops::ControlFlow;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::for_await;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::{Error, is_cancellation_error};
use je_base::event::Emitter;
use je_base::promise::{DeferredPromise, Promise};
use je_base::resources::ExtUri;
use je_base::uri::{URI, UriComponents};
use je_base::JsString;
use je_platform::resources::{host_environment, uri_file};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_workbench::contrib::testing::common::main_thread_test_collection::MainThreadTestCollection;
use je_workbench::contrib::testing::common::test_id::TestId;
use je_workbench::contrib::testing::common::test_service::{
    IMainThreadTestCollection, ITestService, simplify_tests_to_execute, tests_in_file, wait_for_test_to_be_idle,
};
use je_workbench::contrib::testing::common::test_types::{
    ITestItem, ITestItemUpdate, IncrementalNodeRef, IncrementalTestCollectionItem, InternalTestItem, PartialTestItem, TestItemExpandState, TestsDiff, TestsDiffOp,
};
use support::test_service_mocks::MockTestService;
use support::test_stubs::{PartialUriIdentityService, StubTreeIds, TestTestCollection, TestTestItem, get_initialized_main_test_collection, make_simple_stub_tree};

type Node = IncrementalNodeRef<IncrementalTestCollectionItem>;

fn id(path: &[&str]) -> String {
    TestId::new(path.iter().map(|part| (*part).to_owned()).collect()).to_string()
}

fn ext_ids(nodes: &[Node]) -> Vec<String> {
    nodes.iter().map(|node| node.borrow().item.ext_id.clone()).collect()
}

fn leaf() -> Option<StubTreeIds> {
    None
}

fn tree1() -> StubTreeIds {
    StubTreeIds(vec![(
        "a",
        Some(StubTreeIds(vec![
            ("b1", Some(StubTreeIds(vec![("c1", Some(StubTreeIds(vec![("d", leaf())]))), ("c2", Some(StubTreeIds(vec![("d", leaf())])))]))),
            ("b2", leaf()),
        ])),
    )])
}

fn node(collection: &dyn IMainThreadTestCollection, path: &[&str]) -> Node {
    collection.get_node_by_id(&id(path)).expect("o nó existe")
}

fn ext_uri_case_sensitive() -> ExtUri {
    ExtUri::new(host_environment(), |_| false)
}

// ---- simplifyTestsToExecute ----------------------------------------------------------------------------------------

#[test]
fn simplify_noop_on_single_item() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let c = get_initialized_main_test_collection(make_simple_stub_tree(&tree1())).await.unwrap();

        let t = simplify_tests_to_execute(&*c, vec![node(&*c, &["ctrlId", "a", "b1"])]);

        assert_eq!(ext_ids(&t), vec![id(&["ctrlId", "a", "b1"])]);
    });
}

#[test]
fn simplify_goes_to_common_root_1() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let c = get_initialized_main_test_collection(make_simple_stub_tree(&tree1())).await.unwrap();

        let t = simplify_tests_to_execute(&*c, vec![node(&*c, &["ctrlId", "a", "b1", "c1", "d"]), node(&*c, &["ctrlId", "a", "b1", "c2"])]);

        assert_eq!(ext_ids(&t), vec![id(&["ctrlId", "a", "b1"])]);
    });
}

#[test]
fn simplify_goes_to_common_root_2() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let c = get_initialized_main_test_collection(make_simple_stub_tree(&tree1())).await.unwrap();

        let t = simplify_tests_to_execute(&*c, vec![node(&*c, &["ctrlId", "a", "b1", "c1"]), node(&*c, &["ctrlId", "a", "b1"])]);

        assert_eq!(ext_ids(&t), vec![id(&["ctrlId", "a", "b1"])]);
    });
}

#[test]
fn simplify_goes_to_common_root_3() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let c = get_initialized_main_test_collection(make_simple_stub_tree(&tree1())).await.unwrap();

        let t = simplify_tests_to_execute(&*c, vec![node(&*c, &["ctrlId", "a", "b1", "c1", "d"]), node(&*c, &["ctrlId", "a", "b1", "c2"])]);

        assert_eq!(ext_ids(&t), vec![id(&["ctrlId", "a", "b1"])]);
    });
}

#[test]
fn simplify_goes_to_common_root_4() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let c = get_initialized_main_test_collection(make_simple_stub_tree(&tree1())).await.unwrap();

        let t = simplify_tests_to_execute(&*c, vec![node(&*c, &["ctrlId", "a", "b2"]), node(&*c, &["ctrlId", "a", "b1"])]);

        assert_eq!(ext_ids(&t), vec![id(&["ctrlId"])]);
    });
}

#[test]
fn simplify_no_op_divergent_trees() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let c = get_initialized_main_test_collection(make_simple_stub_tree(&tree1())).await.unwrap();

        let t = simplify_tests_to_execute(&*c, vec![node(&*c, &["ctrlId", "a", "b1", "c2"]), node(&*c, &["ctrlId", "a", "b2"])]);

        assert_eq!(ext_ids(&t), vec![id(&["ctrlId", "a", "b1", "c2"]), id(&["ctrlId", "a", "b2"])]);
    });
}

// ---- testsInFile ---------------------------------------------------------------------------------------------------

struct Expandable {
    ident: Rc<dyn IUriIdentityService>,
    service: Rc<MockTestService>,
    collection: Rc<MainThreadTestCollection>,
}

fn identity(canonical: impl Fn(&URI) -> URI + 'static, ext_uri: Option<ExtUri>) -> Rc<dyn IUriIdentityService> {
    Rc::new(PartialUriIdentityService { canonical: Box::new(canonical), ext_uri })
}

fn service_with(collection: Rc<dyn IMainThreadTestCollection>) -> Rc<MockTestService> {
    let service = Rc::new(MockTestService::default());
    *service.collection.borrow_mut() = Some(collection);
    service
}

/// `createExpandableCollection(expand)`.
fn create_expandable_collection(expand: impl Fn() -> Promise<()> + 'static) -> Expandable {
    let ident = identity(|uri| uri.clone(), Some(ext_uri_case_sensitive()));
    let collection = Rc::new(MainThreadTestCollection::new(Rc::new(PartialCanonicalizer), Rc::new(move |_id: &str, _levels: f64| expand())));
    collection.apply(&vec![TestsDiffOp::Add {
        item: InternalTestItem {
            controller_id: "ctrlId".to_owned(),
            expand: TestItemExpandState::Expandable,
            item: TestTestItem::new(TestId::new(vec!["ctrlId".to_owned()]), "root", Some(uri_file("/workspace"))).to_test_item(),
        },
    }]);
    let service = service_with(collection.clone());
    Expandable { ident, service, collection }
}

/// `{ asCanonicalUri: uri => uri }` como `ITestUriCanonicalizer`.
struct PartialCanonicalizer;

impl je_workbench::contrib::testing::common::test_types::ITestUriCanonicalizer for PartialCanonicalizer {
    fn as_canonical_uri(&self, uri: &URI) -> URI {
        uri.clone()
    }
}

#[test]
fn propagates_expansion_errors_instead_of_reporting_completed_empty_discovery() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let e = create_expandable_collection(|| Promise::rejected(Error::new("Discovery failed")));
        let iterable = tests_in_file(e.service.clone(), e.ident.clone(), uri_file("/workspace/test.ts"), true, true, CancellationToken::None);
        let result = for_await(&*iterable, |_batch| panic!("failed discovery must not yield tests")).await;
        let error = result.expect_err("a descoberta que falhou rejeita");
        assert!(error.to_string().contains("Discovery failed"), "{error}");
        let _ = &e.collection;
    });
}

#[test]
fn cancels_an_in_flight_expansion_without_waiting_for_the_provider() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let started = Rc::new(DeferredPromise::<()>::new());
        let never_resolvers: Rc<RefCell<Vec<je_base::promise::Resolver<()>>>> = Rc::default();
        let (started_in, never_in) = (started.clone(), never_resolvers.clone());
        let e = create_expandable_collection(move || {
            drop(started_in.complete(()));
            let (never, resolver) = Promise::new();
            never_in.borrow_mut().push(resolver);
            never
        });
        let token = ds.add_rc(Rc::new(CancellationTokenSource::new(None)));
        let iterable = tests_in_file(e.service.clone(), e.ident.clone(), uri_file("/workspace/test.ts"), true, false, token.token());
        let iterator = iterable.async_iterator();
        let pending = iterator.next();
        started.promise.clone().await.unwrap();
        token.cancel();
        let error = pending.await.expect_err("a expansão cancelada rejeita");
        assert!(is_cancellation_error(&error), "{error}");
    });
}

#[test]
fn does_not_start_discovery_with_a_pre_cancelled_token() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let expanded = Rc::new(Cell::new(false));
        let expanded_in = expanded.clone();
        let e = create_expandable_collection(move || {
            expanded_in.set(true);
            Promise::resolved(())
        });
        let iterable = tests_in_file(e.service.clone(), e.ident.clone(), uri_file("/workspace/test.ts"), true, false, CancellationToken::Cancelled);
        let iterator = iterable.async_iterator();
        let error = iterator.next().await.expect_err("o token já cancelado rejeita");
        assert!(is_cancellation_error(&error), "{error}");
        assert!(!expanded.get());
    });
}

fn remote_canonical_uri() -> URI {
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from("vscode-remote")),
            authority: Some(JsString::from("wsl+Ubuntu")),
            path: Some(JsString::from("/home/user/test.py")),
            ..UriComponents::default()
        },
        false,
    )
}

#[test]
fn canonicalizes_uri_before_comparing_with_stored_test_uris() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let canonical_uri = remote_canonical_uri();
        let raw_uri = uri_file("/home/user/test.py");
        let ext_uri = ext_uri_case_sensitive();

        // Build a collection where test items are stored with the canonical URI.
        let collection = TestTestCollection::new("ctrlId");
        let root = collection.root().clone();
        let stored_uri = canonical_uri.clone();
        collection.set_resolve_handler(Some(Rc::new(move |item: Option<&support::test_stubs::TestItemHandle>| {
            if item.is_none() {
                root.children().add(TestTestItem::new(TestId::new(vec!["ctrlId".to_owned(), "test1".to_owned()]), "test1", Some(stored_uri.clone())));
            }
            je_base::promise::MaybePromise::Value(())
        })));
        let main_collection = get_initialized_main_test_collection(collection).await.unwrap();

        // Mock IUriIdentityService: asCanonicalUri maps rawUri to canonicalUri,
        // simulating a remote environment where URI.file() produces a non-canonical form.
        let (canonical_for_map, raw_for_map, ext_for_map) = (canonical_uri.clone(), raw_uri.clone(), ext_uri.clone());
        let ident = identity(
            move |u| if ext_for_map.is_equal(Some(u), Some(&raw_for_map), false) { canonical_for_map.clone() } else { u.clone() },
            Some(ext_uri),
        );

        let test_service = service_with(main_collection);

        let mut found: Vec<String> = Vec::new();
        let iterable = tests_in_file(test_service, ident, raw_uri, false, true, CancellationToken::None);
        for_await(&*iterable, |batch: Vec<Node>| {
            for item in batch {
                found.push(item.borrow().item.ext_id.clone());
            }
            Ok(ControlFlow::Continue(()))
        })
        .await
        .unwrap();

        assert_eq!(found, vec![id(&["ctrlId", "test1"])]);
    });
}

fn plain_item(ext_id: String, label: &str, uri: Option<URI>) -> ITestItem {
    ITestItem { ext_id, label: label.to_owned(), tags: Vec::new(), busy: false, uri, range: None, description: None, error: None, sort_text: None }
}

#[test]
fn indexes_live_diff_added_tests_by_canonical_uri_and_still_finds_them_from_raw_uri_input() {
    run_with_faked_timers(async {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let canonical_uri = remote_canonical_uri();
        let raw_uri = uri_file("/home/user/test.py");
        let ext_uri = ext_uri_case_sensitive();

        let (canonical_for_collection, raw_for_collection) = (canonical_uri.clone(), raw_uri.clone());
        let collection = Rc::new(MainThreadTestCollection::new(
            Rc::new(PartialUriCanonicalizer {
                canonical: Box::new(move |u| if u.to_string(false) == raw_for_collection.to_string(false) { canonical_for_collection.clone() } else { u.clone() }),
            }),
            Rc::new(|_id: &str, _levels: f64| Promise::resolved(())),
        ));

        let test_id = id(&["ctrlId", "test1"]);
        collection.apply(&vec![
            TestsDiffOp::Add {
                item: InternalTestItem {
                    controller_id: "ctrlId".to_owned(),
                    expand: TestItemExpandState::NotExpandable,
                    item: plain_item(id(&["ctrlId"]), "root", None),
                },
            },
            TestsDiffOp::Add {
                item: InternalTestItem {
                    controller_id: "ctrlId".to_owned(),
                    expand: TestItemExpandState::NotExpandable,
                    item: plain_item(test_id.clone(), "test1", Some(raw_uri.clone())),
                },
            },
        ]);

        let added = collection.get_node_by_id(&test_id).expect("o teste foi adicionado");
        assert!(ext_uri.is_equal(added.borrow().item.uri.as_ref(), Some(&canonical_uri), false), "added test URI should be canonicalized on apply");
        assert_eq!(collection.get_node_by_url(&canonical_uri).iter().map(|n| n.borrow().item.ext_id.clone()).collect::<Vec<_>>(), vec![test_id.clone()]);
        assert_eq!(collection.get_node_by_url(&raw_uri).iter().map(|n| n.borrow().item.ext_id.clone()).collect::<Vec<_>>(), Vec::<String>::new());

        let (canonical_for_map, raw_for_map, ext_for_map) = (canonical_uri.clone(), raw_uri.clone(), ext_uri.clone());
        let ident = identity(
            move |u| if ext_for_map.is_equal(Some(u), Some(&raw_for_map), false) { canonical_for_map.clone() } else { u.clone() },
            Some(ext_uri),
        );
        let test_service = service_with(collection);

        let mut found: Vec<String> = Vec::new();
        let iterable = tests_in_file(test_service, ident, raw_uri, false, true, CancellationToken::None);
        for_await(&*iterable, |batch: Vec<Node>| {
            for item in batch {
                found.push(item.borrow().item.ext_id.clone());
            }
            Ok(ControlFlow::Continue(()))
        })
        .await
        .unwrap();

        assert_eq!(found, vec![test_id]);
    });
}

/// `{ asCanonicalUri: fn }` como `ITestUriCanonicalizer`.
struct PartialUriCanonicalizer {
    canonical: Box<dyn Fn(&URI) -> URI>,
}

impl je_workbench::contrib::testing::common::test_types::ITestUriCanonicalizer for PartialUriCanonicalizer {
    fn as_canonical_uri(&self, uri: &URI) -> URI {
        (self.canonical)(uri)
    }
}

// ---- waitForTestToBeIdle -------------------------------------------------------------------------------------------

struct Busy {
    collection: Rc<MainThreadTestCollection>,
    test: Node,
    diffs: Rc<Emitter<Rc<TestsDiff>>>,
    service: Rc<MockTestService>,
}

/// `createBusyTest()`.
async fn create_busy_test(ds: &LeakGuard) -> Busy {
    let collection = get_initialized_main_test_collection(make_simple_stub_tree(&StubTreeIds(vec![("a", None)]))).await.unwrap();
    let test = node(&*collection, &["ctrlId", "a"]);
    let ext_id = test.borrow().item.ext_id.clone();
    collection.apply(&vec![TestsDiffOp::Update {
        item: ITestItemUpdate { ext_id, expand: None, item: Some(PartialTestItem { busy: Some(true), ..PartialTestItem::default() }) },
    }]);
    let diffs = ds.add_rc(Emitter::<Rc<TestsDiff>>::new());
    let service = service_with(collection.clone());
    *service.on_did_process_diff.borrow_mut() = Some(diffs.event());
    Busy { collection, test, diffs, service }
}

#[test]
fn wait_for_test_to_be_idle_releases_its_diff_listener_on_cancellation() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let b = create_busy_test(&ds).await;
        let token = ds.add_rc(Rc::new(CancellationTokenSource::new(None)));
        let pending = wait_for_test_to_be_idle(&*b.service, &b.test, token.token(), false);
        token.cancel();
        let error = pending.await.expect_err("a espera cancelada rejeita");
        assert!(is_cancellation_error(&error), "{error}");
        assert!(!b.diffs.has_listeners());
    });
}

#[test]
fn wait_for_test_to_be_idle_preserves_the_default_wait_until_idle_behavior_and_releases_its_listener() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let b = create_busy_test(&ds).await;
        let pending = wait_for_test_to_be_idle(&*b.service, &b.test, CancellationToken::None, false);
        let ext_id = b.test.borrow().item.ext_id.clone();
        let diff: TestsDiff = vec![TestsDiffOp::Update {
            item: ITestItemUpdate { ext_id, expand: None, item: Some(PartialTestItem { busy: Some(false), ..PartialTestItem::default() }) },
        }];
        b.collection.apply(&diff);
        b.diffs.fire(Rc::new(diff));
        pending.await.unwrap();
        assert!(!b.diffs.has_listeners());
    });
}

#[test]
fn wait_for_test_to_be_idle_releases_its_listener_when_the_busy_test_is_removed() {
    run_with_faked_timers(async {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let b = create_busy_test(&ds).await;
        let pending = wait_for_test_to_be_idle(&*b.service, &b.test, CancellationToken::None, false);
        let item_id = b.test.borrow().item.ext_id.clone();
        let diff: TestsDiff = vec![TestsDiffOp::Remove { item_id }];
        b.collection.apply(&diff);
        b.diffs.fire(Rc::new(diff));
        pending.await.unwrap();
        assert!(!b.diffs.has_listeners());
    });
}
