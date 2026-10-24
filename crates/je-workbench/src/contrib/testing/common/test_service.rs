//! Porte parcial de `src/vs/workbench/contrib/testing/common/testService.ts`: os contratos que o resto do contrib
//! lê (`IMainThreadTestController`, `IMainThreadTestCollection`, `ITestService`) e os auxiliares que operam só sobre a
//! coleção. Entrou junto com o núcleo de resultados porque o serviço de perfis e a coleção da thread principal dependem
//! dos contratos.
//!
//! Fora desta fatia, por dependerem de iteração assíncrona (`AsyncIterable`) e do serviço de testes em execução, que ainda
//! não existe no porte: `waitForTestToBeIdle`, `testsInFile` e `testsUnderUri`.
//!
//! Divergências:
//! - **`IMainThreadTestCollection extends AbstractIncrementalTestCollection`**: o trait cobre os membros novos mais
//!   `apply` e `tags`; os nós são `Rc<RefCell<IncrementalTestCollectionItem>>`.
//! - **`Location`** (de `editor/common/languages.ts`, ainda não portado) é a struct [`Location`], `{ uri, range }`.
//! - **`getContextForTestItem`** devolve [`TestItemContext`]; o original põe o nó da coleção direto em `tests` (a forma
//!   estrutural de `InternalTestItem.Serialized`), aqui o nó passa por `InternalTestItem::serialize`.
//! - **`Iterable<T>`** de retorno são `Vec<T>`. **`Infinity`** de `levels` é `f64::INFINITY`.
//! - **`expandAndGetTestById`** é `async` no original: devolve `Promise`, e o corpo é um `async_fn`.

use std::cell::Cell;
use std::rc::Rc;

use je_base::async_::{AsyncIterable, AsyncIterableEmitter, AsyncIterableProducer, async_fn, race_cancellation_error};
use je_base::cancellation::CancellationToken;
use je_base::errors::{Error, throw};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::marshalling_ids::MarshalledId;
use je_base::observable::IObservable;
use je_base::prefix_tree::{Node, WellDefinedPrefixTree};
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_base::uri::URI;
use je_editor::position::Position;
use je_editor::range::Range;
use je_instantiation::service_id;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use super::observable_value::MutableObservableValue;
use super::test_exclusions::TestExclusions;
use super::test_id::TestId;
use super::test_result::ITestResult;
use super::test_types::{
    ICallProfileRunHandler, IStartControllerTests, IStartControllerTestsResult, ITestItemContext, IncrementalNodeRef, IncrementalTestCollectionItem,
    InternalTestItem, ResolvedTestRunRequest, TestItemExpandState, TestControllerCapability, TestMessageFollowupRequest, TestMessageFollowupResponse, TestRunProfileBitset,
    TestsDiff,
};

/// `TestIdPathParts.Delimiter`.
const TEST_ID_DELIMITER: char = '\0';

/// `Location` de `languages.ts`: um intervalo num recurso.
#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub uri: URI,
    pub range: Range,
}

pub trait IMainThreadTestController {
    fn id(&self) -> String;
    fn label(&self) -> IObservable<String>;
    fn capabilities(&self) -> IObservable<TestControllerCapability>;
    fn sync_tests(&self, token: CancellationToken) -> Promise<()>;
    fn refresh_tests(&self, token: CancellationToken) -> Promise<()>;
    fn configure_run_profile(&self, profile_id: u32);
    fn expand_test(&self, id: &str, levels: f64) -> Promise<()>;
    fn get_related_code(&self, test_id: &str, token: CancellationToken) -> Promise<Vec<Location>>;
    fn start_continuous_run(&self, request: Vec<ICallProfileRunHandler>, token: CancellationToken) -> Promise<Vec<IStartControllerTestsResult>>;
    fn run_tests(&self, request: Vec<IStartControllerTests>, token: CancellationToken) -> Promise<Vec<IStartControllerTestsResult>>;
}

pub trait IMainThreadTestHostProxy {
    fn provide_test_followups(&self, req: TestMessageFollowupRequest, token: CancellationToken) -> Promise<Vec<TestMessageFollowupResponse>>;
    fn get_tests_related_to_code(&self, uri: &URI, position: Position, token: CancellationToken) -> Promise<Vec<String>>;
    fn execute_test_followup(&self, id: u32) -> Promise<()>;
    fn dispose_test_followups(&self, ids: Vec<u32>);
}

pub trait IMainThreadTestCollection {
    fn on_busy_providers_change(&self) -> Event<i32>;

    /// Number of providers working to discover tests.
    fn busy_providers(&self) -> i32;

    /// Root item IDs.
    fn root_ids(&self) -> Vec<String>;

    /// Root items, correspond to registered controllers.
    fn root_items(&self) -> Vec<IncrementalNodeRef<IncrementalTestCollectionItem>>;

    /// Iterates over every test in the collection, in strictly descending
    /// order of depth.
    fn all(&self) -> Vec<IncrementalNodeRef<IncrementalTestCollectionItem>>;

    /// Gets a node in the collection by ID.
    fn get_node_by_id(&self, id: &str) -> Option<IncrementalNodeRef<IncrementalTestCollectionItem>>;

    /// Gets all tests that have the given URL. Tests returned from this
    /// method are *not* in any particular order.
    fn get_node_by_url(&self, uri: &URI) -> Vec<IncrementalNodeRef<IncrementalTestCollectionItem>>;

    /// Requests that children be revealed for the given test. "Levels" may
    /// be infinite.
    fn expand(&self, test_id: &str, levels: f64) -> Promise<()>;

    /// Gets a diff that adds all items currently in the tree to a new collection,
    /// allowing it to fully hydrate.
    fn get_reviver_diff(&self) -> TestsDiff;

    /// `AbstractIncrementalTestCollection.apply`.
    fn apply(&self, diff: &TestsDiff);
}

pub fn test_collection_is_empty(collection: &dyn IMainThreadTestCollection) -> bool {
    !collection.root_items().iter().any(|r| !r.borrow().children.is_empty())
}

/// O que `getContextForTestItem` devolve: `{ controller }` para uma raiz, ou o contexto com os testes do caminho.
#[derive(Clone, Debug, PartialEq)]
pub enum TestItemContext {
    Controller { controller: String },
    Tests(ITestItemContext),
}

pub fn get_context_for_test_item(collection: &dyn IMainThreadTestCollection, id: &str) -> TestItemContext {
    let id = TestId::from_string(id);

    if id.is_root() {
        return TestItemContext::Controller { controller: id.to_string() };
    }

    let mut context = ITestItemContext { mid: MarshalledId::TestItemContext, tests: Vec::new() };
    for i in id.ids_from_root() {
        if !i.is_root()
            && let Some(test) = collection.get_node_by_id(&i.to_string())
        {
            let test = test.borrow();
            context.tests.push(InternalTestItem::serialize(&InternalTestItem {
                controller_id: test.controller_id.clone(),
                expand: test.expand,
                item: test.item.clone(),
            }));
        }
    }

    TestItemContext::Tests(context)
}

/// Ensures the test with the given ID exists in the collection, if possible.
/// If cancellation is requested, or the test cannot be found, it will return
/// undefined.
pub fn expand_and_get_test_by_id(
    collection: Rc<dyn IMainThreadTestCollection>,
    id: &str,
    ct: CancellationToken,
) -> Promise<Option<IncrementalNodeRef<IncrementalTestCollectionItem>>> {
    let id = id.to_string();
    async_fn(async move {
        let id_path = TestId::from_string(&id).ids_from_root();

        let last = id_path.len() - 1;
        let mut expand_to_level = 0;
        let mut i = last;
        while !ct.is_cancellation_requested() && i >= expand_to_level {
            let current_id = id_path[i].to_string();
            let Some(existing) = collection.get_node_by_id(&current_id) else {
                if i == 0 {
                    break;
                }
                i -= 1;
                continue;
            };

            if i == last {
                return Ok(Some(existing));
            }

            // expand children only if it looks like it's necessary
            let next_id = id_path[i + 1].to_string();
            if !existing.borrow().children.contains(&next_id) {
                collection.expand(&current_id, 0.0).await?;
            }

            expand_to_level = i + 1; // avoid an infinite loop if the test does not exist
            i = last;
        }
        Ok(None)
    })
}

/// Waits for the test to no longer be busy, optionally including pending discovery expansion.
///
/// O original lança o `CancellationError` de forma síncrona e devolve `undefined` quando o teste já está ocioso; aqui as
/// duas saídas são uma promessa (rejeitada com o erro de cancelamento, ou já resolvida), que é o que um chamador dentro
/// de uma função `async` observa.
pub fn wait_for_test_to_be_idle(
    test_service: &dyn ITestService,
    test: &IncrementalNodeRef<IncrementalTestCollectionItem>,
    token: CancellationToken,
    wait_for_expansion: bool,
) -> Promise<()> {
    if token.is_cancellation_requested() {
        return Promise::rejected(Error::cancellation());
    }
    let is_busy = move |item: Option<&IncrementalTestCollectionItem>| {
        item.is_some_and(|item| item.item.busy || (wait_for_expansion && item.expand == TestItemExpandState::BusyExpanding))
    };
    if !is_busy(Some(&test.borrow())) {
        return Promise::resolved(());
    }

    let store = DisposableStore::new();
    let (promise, resolver) = Promise::new();
    let ext_id = test.borrow().item.ext_id.clone();
    let collection = test_service.collection();
    let resolver_on_diff = resolver.clone();
    store.add(test_service.on_did_process_diff().listen(move |_| {
        let node = collection.get_node_by_id(&ext_id);
        if !is_busy(node.as_ref().map(|node| node.borrow()).as_deref()) {
            resolver_on_diff.resolve(());
        }
    }));
    store.add(token.on_cancellation_requested().listen(move |_| resolver.reject(Error::cancellation())));
    promise.finally(move || store.dispose())
}

/// Um item da fila de `testsInFile`: ids prontos, ou uma expansão em andamento que dá os ids quando termina.
enum QueueEntry {
    Ids(Vec<String>),
    Deferred(Rc<DeferredPromise<Rc<dyn Fn() -> Result<Vec<String>, Error>>>>),
}

/// Iterator that expands to and iterates through tests in the file. Iterates
/// in strictly descending order.
///
/// Divergência: o gerador assíncrono do original só corre quando o consumidor pede o próximo valor. Aqui o corpo é o
/// executor de um [`AsyncIterableProducer`], que corre numa microtarefa logo depois da criação e acumula os lotes; fechar o
/// iterador (`return()`) o faz parar na próxima volta do laço, e o `token` o interrompe como no original. Para um
/// consumidor que lê tudo (todos os usos do original) o resultado é o mesmo.
pub fn tests_in_file(
    test_service: Rc<dyn ITestService>,
    ident: Rc<dyn IUriIdentityService>,
    uri: URI,
    wait_for_idle: bool,
    descend_in_file: bool,
    token: CancellationToken,
) -> Rc<dyn AsyncIterable<Vec<IncrementalNodeRef<IncrementalTestCollectionItem>>>> {
    let stopped = Rc::new(Cell::new(false));
    let stopped_on_return = stopped.clone();
    let producer = AsyncIterableProducer::new(
        move |emitter: AsyncIterableEmitter<Vec<IncrementalNodeRef<IncrementalTestCollectionItem>>>| {
            MaybePromise::Promise(async_fn(async move {
                // Canonicalize the URI so that comparisons against test item URIs (which are
                // stored in their canonical form via asCanonicalUri() during deserialization)
                // work correctly in remote environments such as WSL, SSH, and dev containers
                // where URI.file(path) may produce a non-canonical form. Fixes #275268.
                let canonical_uri = ident.as_canonical_uri(&uri);
                let ext_uri = ident.ext_uri();
                let collection = test_service.collection();

                // In this function we go to a bit of effort to avoid awaiting unnecessarily
                // and bulking the test collections we do collect for consumers. This fixes
                // a performance issue (#235819) where a large number of tests in a file
                // would cause a long delay switching editors.
                let mut queue: Vec<QueueEntry> = Vec::new();

                let mut existing = collection.get_node_by_url(&canonical_uri);
                existing.sort_by_key(|node| node.borrow().item.ext_id.encode_utf16().count());

                // getNodeByUrl will return all known tests in the URI, but this can include
                // children of tests even when `descendInFile` is false. Remove those cases.
                let mut i = 0;
                while i + 1 < existing.len() {
                    let prefix = format!("{}{}", existing[i].borrow().item.ext_id, TEST_ID_DELIMITER);
                    let mut k = i + 1;
                    while k < existing.len() {
                        if existing[k].borrow().item.ext_id.starts_with(&prefix) {
                            existing.remove(k);
                        } else {
                            k += 1;
                        }
                    }
                    i += 1;
                }

                queue.push(QueueEntry::Ids(if existing.is_empty() {
                    collection.root_ids()
                } else {
                    existing.iter().map(|node| node.borrow().item.ext_id.clone()).collect()
                }));

                let mut gather: Vec<IncrementalNodeRef<IncrementalTestCollectionItem>> = Vec::new();
                while let Some(next) = queue.pop() {
                    if token.is_cancellation_requested() {
                        return Err(Error::cancellation());
                    }
                    if stopped.get() {
                        return Ok(());
                    }
                    let ids: Vec<String> = match next {
                        QueueEntry::Ids(ids) => ids,
                        QueueEntry::Deferred(deferred) if deferred.is_settled() => match deferred.value() {
                            Some(ids) => ids()?,
                            None => Vec::new(),
                        },
                        QueueEntry::Deferred(deferred) => {
                            if !gather.is_empty() {
                                emitter.emit_one(std::mem::take(&mut gather));
                            }
                            let ids = race_cancellation_error(&deferred.promise, &token).await?;
                            ids()?
                        }
                    };

                    for id in ids {
                        if token.is_cancellation_requested() {
                            return Err(Error::cancellation());
                        }
                        let Some(test) = collection.get_node_by_id(&id) else {
                            continue; // possible because we expand async and things could delete
                        };

                        let test_uri = test.borrow().item.uri.clone();
                        let Some(test_uri) = test_uri else {
                            queue.push(QueueEntry::Ids(test.borrow().children.iter().cloned().collect()));
                            continue;
                        };

                        if ext_uri.is_equal(Some(&canonical_uri), Some(&test_uri), false) {
                            gather.push(test.clone());

                            if !descend_in_file {
                                continue;
                            }
                        }

                        if ext_uri.is_equal_or_parent(&canonical_uri, &test_uri, false, false) {
                            let mut prom: Option<Promise<()>> = None;
                            if test.borrow().expand == TestItemExpandState::Expandable {
                                prom = Some(collection.expand(&test.borrow().item.ext_id, 1.0));
                            }
                            if wait_for_idle {
                                if let Some(expanding) = prom.take() {
                                    let (service, test, token) = (test_service.clone(), test.clone(), token.clone());
                                    prom = Some(expanding.then_promise(move |()| wait_for_test_to_be_idle(service.as_ref(), &test, token.clone(), false)));
                                } else if test.borrow().item.busy {
                                    prom = Some(wait_for_test_to_be_idle(test_service.as_ref(), &test, token.clone(), false));
                                }
                            }

                            if let Some(prom) = prom {
                                // Queue errors as values so abandoned batches cannot produce unhandled rejections.
                                let test_for_children = test.clone();
                                let settled = async_fn(async move {
                                    let result: Rc<dyn Fn() -> Result<Vec<String>, Error>> = match prom.await {
                                        Ok(()) => Rc::new(move || Ok(test_for_children.borrow().children.iter().cloned().collect())),
                                        Err(error) => Rc::new(move || Err(error.clone())),
                                    };
                                    Ok(result)
                                });
                                queue.push(QueueEntry::Deferred(Rc::new(DeferredPromise::from_promise(&settled))));
                            } else if !test.borrow().children.is_empty() {
                                queue.push(QueueEntry::Ids(test.borrow().children.iter().cloned().collect()));
                            }
                        }
                    }
                }

                if !gather.is_empty() {
                    emitter.emit_one(gather);
                }
                Ok(())
            }))
        },
        Some(Rc::new(move || stopped_on_return.set(true))),
    );
    producer
}

/// Iterator that iterates to the top-level children of tests under the given
/// the URI.
///
/// Divergência: ver [`tests_in_file`] (o gerador vira o executor de um [`AsyncIterableProducer`]).
pub fn tests_under_uri(
    test_service: Rc<dyn ITestService>,
    ident: Rc<dyn IUriIdentityService>,
    uri: URI,
    wait_for_idle: bool,
) -> Rc<dyn AsyncIterable<IncrementalNodeRef<IncrementalTestCollectionItem>>> {
    let stopped = Rc::new(Cell::new(false));
    let stopped_on_return = stopped.clone();
    AsyncIterableProducer::new(
        move |emitter: AsyncIterableEmitter<IncrementalNodeRef<IncrementalTestCollectionItem>>| {
            MaybePromise::Promise(async_fn(async move {
                let ext_uri = ident.ext_uri();
                let collection = test_service.collection();

                let mut queue: Vec<Vec<String>> = vec![collection.root_ids()];
                while let Some(ids) = queue.pop() {
                    for test_id in ids {
                        if stopped.get() {
                            return Ok(());
                        }
                        let test = collection.get_node_by_id(&test_id);

                        // Expand tests with URIs that are parent of the item, add tests
                        // that are within the URI. Don't add their children, since those
                        // tests already encompass their children.
                        let Some(test) = test else {
                            continue; // no-op
                        };
                        let test_uri = test.borrow().item.uri.clone();
                        match test_uri {
                            Some(test_uri) if ext_uri.is_equal_or_parent(&test_uri, &uri, false, false) => emitter.emit_one(test),
                            test_uri if test_uri.as_ref().is_none_or(|test_uri| ext_uri.is_equal_or_parent(&uri, test_uri, false, false)) => {
                                if test.borrow().expand == TestItemExpandState::Expandable {
                                    let ext_id = test.borrow().item.ext_id.clone();
                                    collection.expand(&ext_id, 1.0).await?;
                                }
                                if wait_for_idle {
                                    wait_for_test_to_be_idle(test_service.as_ref(), &test, CancellationToken::None, false).await?;
                                }
                                queue.push(test.borrow().children.iter().cloned().collect());
                            }
                            _ => {}
                        }
                    }
                }
                Ok(())
            }))
        },
        Some(Rc::new(move || stopped_on_return.set(true))),
    )
}

/// Simplifies the array of tests by preferring test item parents if all of
/// their children are included.
pub fn simplify_tests_to_execute(
    collection: &dyn IMainThreadTestCollection,
    tests: Vec<IncrementalNodeRef<IncrementalTestCollectionItem>>,
) -> Vec<IncrementalNodeRef<IncrementalTestCollectionItem>> {
    if tests.len() < 2 {
        return tests;
    }

    type NodeRef = IncrementalNodeRef<IncrementalTestCollectionItem>;

    let mut tree = WellDefinedPrefixTree::<NodeRef>::new();
    for test in &tests {
        let path = TestId::from_string(&test.borrow().item.ext_id).path().to_vec();
        tree.insert(path, test.clone(), None);
    }

    let mut out: Vec<NodeRef> = Vec::new();

    // Returns the node if it and any children should be included. Otherwise
    // pushes into the `out` any individual children that should be included.
    fn process(collection: &dyn IMainThreadTestCollection, current_id: &mut Vec<String>, node: &Node<NodeRef>, out: &mut Vec<NodeRef>) -> Option<NodeRef> {
        // directly included, don't try to over-specify, and children should be ignored
        if let Some(value) = &node.value {
            return Some(value.clone());
        }

        let Some(children) = &node.children else {
            throw(Error::new("expect to have children"));
        };

        let mut this_children: Vec<NodeRef> = Vec::new();
        for (part, child) in children.iter() {
            current_id.push(part.clone());
            if let Some(c) = process(collection, current_id, child, out) {
                this_children.push(c);
            }
            current_id.pop();
        }

        if this_children.is_empty() {
            return None;
        }

        // If there are multiple children and we have all of them, then tell the
        // parent this node should be included. Otherwise include children individually.
        let id = TestId::new(current_id.clone());
        let test = collection.get_node_by_id(&id.to_string());
        if test.as_ref().is_some_and(|t| t.borrow().children.len() == this_children.len()) {
            return test;
        }

        out.extend(this_children);
        None
    }

    for (id, node) in tree.entries() {
        let n = process(collection, &mut vec![id.clone()], node, &mut out);
        if let Some(n) = n {
            out.push(n);
        }
    }

    out
}

/// A run request that expresses the intent of the request and allows the
/// test service to resolve the specifics of the group.
#[derive(Clone, Debug)]
pub struct AmbiguousRunTestsRequest {
    /// Group to run
    pub group: TestRunProfileBitset,
    /// Tests to run. Allowed to be from different controllers
    pub tests: Vec<InternalTestItem>,
    /// Tests to exclude. If not given, the current UI excluded tests are used
    pub exclude: Option<Vec<InternalTestItem>>,
    /// Whether this was triggered from an auto run.
    pub continuous: Option<bool>,
    /// Whether this was trigged by a user action in UI. Default=true
    pub preserve_focus: Option<bool>,
}

#[derive(Clone)]
pub struct ITestFollowup {
    pub message: String,
    pub execute: Rc<dyn Fn() -> Promise<()>>,
}

pub trait ITestFollowups: IDisposable {
    fn followups(&self) -> Vec<ITestFollowup>;
}

/// O `{ runId, taskId }` de `onDidCancelTestRun`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelTestRun {
    pub run_id: Option<String>,
    pub task_id: Option<String>,
}

pub trait ITestService {
    /// Fires when the user requests to cancel a test run -- or all runs, if no
    /// runId is given.
    fn on_did_cancel_test_run(&self) -> Event<CancelTestRun>;

    /// Event that fires when the excluded tests change.
    fn excluded(&self) -> Rc<TestExclusions>;

    /// Test collection instance.
    fn collection(&self) -> Rc<dyn IMainThreadTestCollection>;

    /// Event that fires immediately before a diff is processed.
    fn on_will_process_diff(&self) -> Event<Rc<TestsDiff>>;

    /// Event that fires after a diff is processed.
    fn on_did_process_diff(&self) -> Event<Rc<TestsDiff>>;

    /// Whether inline editor decorations should be visible.
    fn show_inline_output(&self) -> Rc<MutableObservableValue<bool>>;

    /// Registers an interface that represents an extension host..
    fn register_ext_host(&self, controller: Rc<dyn IMainThreadTestHostProxy>) -> DisposableRef;

    /// Registers an interface that runs tests for the given provider ID.
    fn register_test_controller(&self, provider_id: &str, controller: Rc<dyn IMainThreadTestController>) -> DisposableRef;

    /// Gets a registered test controller by ID.
    fn get_test_controller(&self, controller_id: &str) -> Option<Rc<dyn IMainThreadTestController>>;

    /// Gets the currently registered test controllers, independently of published test roots.
    fn get_test_controllers(&self) -> Vec<Rc<dyn IMainThreadTestController>>;

    /// Refreshes tests for the controller, or all controllers if no ID is given.
    fn refresh_tests(&self, controller_id: Option<&str>) -> Promise<()>;

    /// Cancels any ongoing test refreshes.
    fn cancel_refresh_tests(&self);

    /// Requests that tests be executed continuously, until the token is cancelled.
    fn start_continuous_run(&self, req: ResolvedTestRunRequest, token: CancellationToken) -> Promise<()>;

    /// Requests that tests be executed.
    fn run_tests(&self, req: AmbiguousRunTestsRequest, token: Option<CancellationToken>) -> Promise<Rc<dyn ITestResult>>;

    /// Requests that tests be executed.
    fn run_resolved_tests(&self, req: ResolvedTestRunRequest, token: Option<CancellationToken>) -> Promise<Rc<dyn ITestResult>>;

    /// Provides followup actions for a test run.
    fn provide_test_followups(&self, req: TestMessageFollowupRequest, token: CancellationToken) -> Promise<Rc<dyn ITestFollowups>>;

    /// Ensures the test diff from the remote ext host is flushed and waits for
    /// any "busy" tests to become idle before resolving.
    fn sync_tests(&self) -> Promise<()>;

    /// Cancels an ongoing test run by its ID, or all runs if no ID is given.
    fn cancel_test_run(&self, run_id: Option<&str>, task_id: Option<&str>);

    /// Publishes a test diff for a controller.
    fn publish_diff(&self, controller_id: &str, diff: TestsDiff);

    /// Gets all tests related to the given code position.
    fn get_tests_related_to_code(&self, uri: &URI, position: Position, token: Option<CancellationToken>) -> Promise<Vec<InternalTestItem>>;

    /// Gets code related to the given test item.
    fn get_code_related_to_test(&self, test: InternalTestItem, token: Option<CancellationToken>) -> Promise<Vec<Location>>;
}

service_id!(pub I_TEST_SERVICE: dyn ITestService = "testService");
