//! Porte de `src/vs/workbench/api/common/extHostTesting.ts`: o lado da extensão dos testes (`vscode.tests`): o serviço
//! [`ExtHostTesting`] (que implementa o [`ExtHostTestingShape`] e cria os controladores), as execuções de teste
//! ([`TestRunCoordinator`], [`TestRunTracker`], [`TestRun`], [`TestRunDto`]), o espelho dos testes da thread principal
//! (`MirroredTestCollection`, [`TestObservers`]) e os perfis de execução ([`TestRunProfileImpl`]).
//!
//! Divergências:
//! - **Objetos da API.** `vscode.TestController`, `vscode.TestRun`, `vscode.TestRunProfile` e `vscode.TestObserver` são
//!   structs com `Rc` ([`TestController`], [`TestRun`], [`TestRunProfileImpl`], [`TestObserver`]), com getters e `set_*`
//!   no lugar das propriedades. `ControllerInfo` é o próprio [`TestController`] (guarda `profiles`, `collection`,
//!   `extension`, `relatedCodeProvider` e `activeProfiles`), e o `protected controllers` é [`ExtHostTesting::controller`].
//! - **Chaves por identidade.** `Map<vscode.TestRunRequest, TestRunTracker>` é um mapa pelo endereço do
//!   `Rc<TestRunRequest>`, que o mapa mantém vivo; o `WeakMap<TestRunResult, string>` é a lista de pares `(resultado, id)`.
//! - **Opcionais e fechos.** Os `?.()` de métodos opcionais de provedores (`relatedCodeProvider`, `loadDetailedCoverage`)
//!   são `Option` de fecho ([`LoadDetailedCoverage`]) ou os testes `supports_*` do trait [`TestRelatedCodeProvider`].
//!   `vscode.TestFollowupProvider` é o trait [`TestFollowupProvider`] e um comando é o [`Command`] do porte.
//! - **Promessas.** `async` é [`async_fn`], o `try/catch` em volta de `await` é o `match` sobre o `Result` do `await`,
//!   e `Promise<void> | void` é [`MaybePromise`]. `String(e)` do erro de `runHandler` é o `Display` do [`Error`].
//! - **Argumentos de comando.** O processador de argumentos lê os `ITestItemContext` e `ITestMessageMenuArgs` como carga
//!   nativa de um [`Dynamic`] (`ObjectRef::with_host`) e devolve o [`TestItemImpl`] (ou [`TestMessageMenuContext`]) do mesmo jeito;
//!   `testing.getExplorerSelection` devolve um objeto com as listas `include` e `exclude`, cada elemento um [`TestController`]
//!   ou um [`TestItemImpl`] como carga nativa.
//! - **`TestRun` guarda o tracker por referência fraca** (o tracker guarda a execução), então chamar a execução depois que
//!   `disposeTestRun` soltou o tracker não faz nada.
//! - **`profileId`** é `hash(label)` (um `int32`) reinterpretado como `u32`, que é o tipo do `ITestRunProfile`.
//! - **`$startContinuousRun`** recebe [`ICallProfileRunHandler`] (o `IStartControllerTests` do original só difere pelo `runId`
//!   que esse caminho nunca lê).
//! - **`$getTestsRelatedToCode`/`$getCodeRelatedToTest`** convertem a posição com `Position.to`, que devolve [`Result`]; um
//!   erro rejeita a promessa.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use indexmap::{IndexMap, IndexSet};
use je_base::async_::{RunOnceScheduler, async_fn, await_maybe};
use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::{self, Error, throw};
use je_base::event::{Emitter, Event, Relay};
use je_base::hash::hash;
use je_base::lifecycle::{Disposable, DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::{MaybePromise, Promise};
use je_base::runtime;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_editor::position::Position as EditorPosition;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::LogServiceRef;

use super::ext_host_commands::{ArgumentProcessor, ExtHostCommands};
use super::ext_host_document_data::TextDocument;
use super::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use super::ext_host_protocol::{ExtHostTestingShape, ITestControllerPatch, IMainContext, MainContext, MainThreadTestingShape};
use super::ext_host_test_item::{ExtHostTestItemCollection, IDocumentVersionSource, TestItemImpl, to_item_from_context};
use super::ext_host_testing_converters::{self as convert, IdentityCanonicalizer};
use super::ext_host_testing_types::{self as api, FileCoverage, FileCoverageDetail, TestResultSnapshot, TestRunProfileBase, TestRunRequest, TestRunResult};
use super::ext_host_type_converters::{Location, LocationDto, Position, Range};
use super::ext_host_types::{self as types, Command};
use crate::contrib::testing::common::constants::TestCommandId;
use crate::contrib::testing::common::test_id::{TestId, TestPosition};
use crate::contrib::testing::common::test_item_collection::{CollectionItemRef, ITestItemChildren, ITestItemLike, ResolveHandler};
use crate::contrib::testing::common::test_types::{
    AbstractIncrementalTestCollection, CallProfileOrStartController, CoverageDetails, ExtensionRunProfile, ExtensionRunTestsRequest, ICallProfileRunHandler,
    ISerializedTestResults, IStartControllerTests, IStartControllerTestsResult, ITestErrorMessageSerialized, ITestItem, ITestItemContext,
    ITestMessageMenuArgs, ITestMessageSerialized, ITestRunProfile, ITestRunTask, IncrementalChangeCollector, IncrementalCollectionHooks, IncrementalNode,
    IncrementalNodeRef, IncrementalTestCollectionItem, InternalTestItem, PartialTestRunProfile, ResolvedTestRunRequest, TestControllerCapability,
    TestMessageFollowupRequest, TestMessageFollowupResponse, TestResultState, TestRunTarget, TestsDiff, TestsDiffOp, TestsDiffOpSerialized,
    test_controller_capability,
};
use crate::services::extensions::extensions::check_proposed_api_enabled;

/// `DefaultProfileChangeEvent`: por controlador, por perfil, o novo valor de `isDefault`.
pub type DefaultProfileChangeEvent = IndexMap</* controllerId */ String, IndexMap</* profileId */ u32, bool>>;

/// O `Map<number, vscode.TestRunProfile>` dos perfis de um controlador.
pub type ProfileMap = Rc<RefCell<IndexMap<u32, Rc<TestRunProfileImpl>>>>;

/// O `Set<number>` dos perfis ativos de um controlador.
pub type ActiveProfiles = Rc<RefCell<IndexSet<u32>>>;

/// `runHandler`: `(request, token) => Thenable<void> | void`.
pub type RunHandler = Rc<dyn Fn(&Rc<TestRunRequest>, &CancellationToken) -> MaybePromise<()>>;

/// `refreshHandler`: `(token) => Thenable<void> | void`.
pub type RefreshHandler = Rc<dyn Fn(&CancellationToken) -> MaybePromise<()>>;

/// `loadDetailedCoverage(testRun, fileCoverage, token)`.
pub type LoadDetailedCoverage = Rc<dyn Fn(&Rc<TestRun>, &FileCoverage, &CancellationToken) -> Promise<Vec<FileCoverageDetail>>>;

/// `loadDetailedCoverageForTest(testRun, fileCoverage, fromTestItem, token)`.
pub type LoadDetailedCoverageForTest = Rc<dyn Fn(&Rc<TestRun>, &FileCoverage, &TestItemImpl, &CancellationToken) -> Promise<Vec<FileCoverageDetail>>>;

type ControllerMap = Rc<RefCell<IndexMap<String, Rc<TestController>>>>;

thread_local! {
    /// `followupCounter`.
    static FOLLOWUP_COUNTER: Cell<u32> = const { Cell::new(0) };
}

/// `vscode.TestRelatedCodeProvider`. Um método ausente no original é `supports_*` falso.
pub trait TestRelatedCodeProvider {
    fn supports_related_tests(&self) -> bool {
        false
    }

    fn supports_related_code(&self) -> bool {
        false
    }

    fn provide_related_tests(&self, _document: &TextDocument, _position: &types::Position, _token: &CancellationToken) -> Promise<Option<Vec<TestItemImpl>>> {
        Promise::resolved(None)
    }

    fn provide_related_code(&self, _test: &TestItemImpl, _token: &CancellationToken) -> Promise<Option<Vec<types::Location>>> {
        Promise::resolved(None)
    }
}

/// `vscode.TestFollowupProvider`.
pub trait TestFollowupProvider {
    fn provide_followup(
        &self,
        result: &Rc<TestRunResult>,
        test: &Rc<TestResultSnapshot>,
        task_index: usize,
        message_index: usize,
        token: &CancellationToken,
    ) -> Promise<Option<Vec<Command>>>;
}

/// O `{ test, message }` que o processador de argumentos entrega no lugar do `ITestMessageMenuArgs`.
pub struct TestMessageMenuContext {
    pub test: TestItemImpl,
    pub message: api::TestMessage,
}

/// O objeto nativo dentro de um [`Dynamic`] (`ObjectRef::with_host`).
fn host_object<T: 'static>(value: T) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(value)))
}

/// Os argumentos de comando que o `ExtHostTesting` registra um processador para.
struct TestingArgumentProcessor {
    controllers: ControllerMap,
}

impl TestingArgumentProcessor {
    fn item_for(&self, target_test: &str) -> Option<TestItemImpl> {
        let controller = self.controllers.borrow().get(TestId::root_string(target_test)).cloned()?;
        let tree = controller.collection.tree.borrow();
        tree.get(target_test).map(|item| item.borrow().actual.clone())
    }
}

impl ArgumentProcessor for TestingArgumentProcessor {
    fn process_argument(&self, arg: &Dynamic, _extension: Option<&ExtensionDescription>) -> Dynamic {
        let Dynamic::Object(object) = arg else {
            return arg.clone();
        };

        if let Some(context) = object.host::<ITestItemContext>() {
            let Some(last) = context.tests.last() else {
                return arg.clone();
            };
            return match self.item_for(&last.item.ext_id).or_else(|| to_item_from_context(&context)) {
                Some(item) => host_object(item),
                None => arg.clone(),
            };
        }

        if let Some(menu_args) = object.host::<ITestMessageMenuArgs>() {
            let ITestMessageSerialized::Error(message) = &menu_args.message else {
                return arg.clone();
            };
            let test = self.item_for(&menu_args.test.item.ext_id).or_else(|| {
                to_item_from_context(&ITestItemContext { mid: menu_args.mid, tests: vec![menu_args.test.clone()] })
            });
            return match (test, convert::TestMessage::to(message)) {
                (Some(test), Ok(message)) => host_object(TestMessageMenuContext { test, message }),
                (_, Err(error)) => throw(error),
                (None, _) => arg.clone(),
            };
        }

        arg.clone()
    }
}

/// `ExtHostTesting`.
pub struct ExtHostTesting {
    this: Weak<ExtHostTesting>,
    base: Rc<Disposable>,
    results_changed_emitter: Rc<Emitter<()>>,
    controllers: ControllerMap,
    proxy: Rc<dyn MainThreadTestingShape>,
    run_tracker: Rc<TestRunCoordinator>,
    observer: TestObservers,
    default_profiles_changed_emitter: Rc<Emitter<DefaultProfileChangeEvent>>,
    followup_providers: RefCell<Vec<Rc<dyn TestFollowupProvider>>>,
    test_followups: RefCell<HashMap<u32, Command>>,
    log_service: LogServiceRef,
    commands: Rc<ExtHostCommands>,
    editors: Rc<ExtHostDocumentsAndEditors>,
    results: RefCell<Vec<(Rc<TestRunResult>, String)>>,
}

impl ExtHostTesting {
    pub fn new(rpc: &dyn IMainContext, log_service: LogServiceRef, commands: Rc<ExtHostCommands>, editors: Rc<ExtHostDocumentsAndEditors>) -> Rc<ExtHostTesting> {
        let base = Disposable::new();
        let results_changed_emitter = Emitter::new();
        base.register(results_changed_emitter.clone());
        let default_profiles_changed_emitter = Emitter::new();
        base.register(default_profiles_changed_emitter.clone());

        let controllers: ControllerMap = Rc::new(RefCell::new(IndexMap::new()));
        let proxy = rpc.get_proxy(&MainContext::MAIN_THREAD_TESTING);
        let observer = TestObservers::new(proxy.clone());
        let run_tracker = Rc::new(TestRunCoordinator::new(proxy.clone(), log_service.clone()));

        commands.register_argument_processor(Rc::new(TestingArgumentProcessor { controllers: controllers.clone() }));

        let this = Rc::new_cyclic(|this: &Weak<ExtHostTesting>| ExtHostTesting {
            this: this.clone(),
            base,
            results_changed_emitter,
            controllers: controllers.clone(),
            proxy,
            run_tracker,
            observer,
            default_profiles_changed_emitter,
            followup_providers: RefCell::new(Vec::new()),
            test_followups: RefCell::new(HashMap::new()),
            log_service,
            commands: commands.clone(),
            editors,
            results: RefCell::new(Vec::new()),
        });

        let explorer_commands = commands.clone();
        let explorer_controllers = controllers;
        commands
            .register_command(
                false,
                "testing.getExplorerSelection",
                Rc::new(move |_args: &[Dynamic]| {
                    let commands = explorer_commands.clone();
                    let controllers = explorer_controllers.clone();
                    MaybePromise::Promise(async_fn(async move {
                        let inner = commands.execute_command(TestCommandId::GET_EXPLORER_SELECTION, Vec::new()).await?;

                        let lookup = |id: &str| -> Option<Dynamic> {
                            let controller = controllers.borrow().get(TestId::root_string(id)).cloned()?;
                            if TestId::is_root_string(id) {
                                return Some(Dynamic::Object(ObjectRef::with_host(controller)));
                            }
                            let tree = controller.collection.tree.borrow();
                            tree.get(id).map(|item| host_object(item.borrow().actual.clone()))
                        };
                        let list = |key: &str| -> Dynamic {
                            let ids = match inner.get_str(key) {
                                Dynamic::Array(array) => array.to_vec(),
                                _ => Vec::new(),
                            };
                            Dynamic::array(ids.iter().filter_map(|id| match id {
                                Dynamic::String(id) => lookup(&id.to_string_lossy()),
                                _ => None,
                            }))
                        };

                        Ok(Dynamic::object([("include", list("include")), ("exclude", list("exclude"))]))
                    }))
                }),
                None,
                None,
            )
            .unwrap_or_else(|error| throw(error));

        this
    }

    /// `onResultsChanged`.
    pub fn on_results_changed(&self) -> Event<()> {
        self.results_changed_emitter.event()
    }

    /// `results`.
    pub fn results(&self) -> Vec<Rc<TestRunResult>> {
        self.results.borrow().iter().map(|(result, _)| result.clone()).collect()
    }

    /// O `controllers.get(id)` (`protected` no original).
    pub fn controller(&self, controller_id: &str) -> Option<Rc<TestController>> {
        self.controllers.borrow().get(controller_id).cloned()
    }

    // #region public API

    /// Implements vscode.test.registerTestProvider
    pub fn create_test_controller(
        &self,
        extension: &ExtensionDescription,
        controller_id: &str,
        label: &str,
        refresh_handler: Option<RefreshHandler>,
    ) -> Result<Rc<TestController>, Error> {
        if self.controllers.borrow().contains_key(controller_id) {
            return Err(Error::new(format!("Attempt to insert a duplicate controller with ID \"{controller_id}\"")));
        }

        let disposable = DisposableStore::new();
        let editors: Rc<dyn IDocumentVersionSource> = self.editors.clone();
        let collection = disposable.add_rc(ExtHostTestItemCollection::new(controller_id, label, editors));
        collection.root().set_label(label);

        let controller = Rc::new(TestController {
            id: controller_id.to_string(),
            label: RefCell::new(label.to_string()),
            refresh_handler: RefCell::new(refresh_handler),
            related_code_provider: RefCell::new(None),
            collection: collection.clone(),
            profiles: ProfileMap::default(),
            active_profiles: ActiveProfiles::default(),
            extension: extension.clone(),
            proxy: self.proxy.clone(),
            run_tracker: self.run_tracker.clone(),
            default_profiles_changed: self.default_profiles_changed_emitter.event(),
            disposable: disposable.clone(),
        });

        let proxy = self.proxy.clone();
        proxy.rpc_register_test_controller(controller_id, label, controller.get_capability());
        {
            let proxy = proxy.clone();
            let id = controller_id.to_string();
            disposable.add(to_disposable(move || proxy.rpc_unregister_test_controller(&id)));
        }

        self.controllers.borrow_mut().insert(controller_id.to_string(), controller.clone());
        {
            let controllers = self.controllers.clone();
            let id = controller_id.to_string();
            disposable.add(to_disposable(move || {
                controllers.borrow_mut().shift_remove(&id);
            }));
        }

        {
            let id = controller_id.to_string();
            disposable.add(collection.on_did_generate_diff().listen(move |diff: &TestsDiff| {
                proxy.rpc_publish_diff(&id, diff.iter().map(TestsDiffOp::serialize).collect());
            }));
        }

        Ok(controller)
    }

    /// Implements vscode.test.createTestObserver
    pub fn create_test_observer(&self) -> Rc<TestObserver> {
        self.observer.checkout()
    }

    /// Implements vscode.test.runTests
    pub fn run_tests(&self, req: &TestRunRequest, token: CancellationToken) -> Promise<()> {
        let Some(profile) = req.profile.clone() else {
            return Promise::rejected(Error::new("The request passed to `vscode.test.runTests` must include a profile"));
        };

        let Some(controller) = self.controller(&profile.base.controller_id) else {
            return Promise::rejected(Error::new("Controller not found"));
        };

        let root_id = controller.collection.root().id();
        let test_ids = match &req.include {
            Some(include) => include.iter().map(|test| TestId::from_ext_host_test_item(test, &root_id, None).to_string()).collect(),
            None => vec![root_id.clone()],
        };
        self.proxy
            .rpc_run_tests(
                ResolvedTestRunRequest {
                    preserve_focus: Some(req.preserve_focus),
                    group: convert::TestRunProfileKind::from(profile.base.kind),
                    targets: vec![TestRunTarget { test_ids, profile_id: profile.base.profile_id, controller_id: profile.base.controller_id.clone() }],
                    exclude: req.exclude.as_ref().map(|exclude| exclude.iter().map(TestItemImpl::id).collect()),
                    continuous: None,
                },
                token,
            )
            .then(|_| ())
    }

    /// Implements vscode.test.registerTestFollowupProvider
    pub fn register_test_followup_provider(&self, provider: Rc<dyn TestFollowupProvider>) -> DisposableRef {
        self.followup_providers.borrow_mut().push(provider.clone());
        let this = self.this.clone();
        to_disposable(move || {
            if let Some(this) = this.upgrade() {
                this.followup_providers.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, &provider));
            }
        })
    }

    // #endregion

    pub fn get_metadata_for_run(&self, run: &Rc<TestRun>) -> Option<(String, String)> {
        for tracker in self.run_tracker.trackers() {
            if let Some(task_id) = tracker.get_task_id_for_run(run) {
                return Some((task_id, tracker.id()));
            }
        }

        None
    }

    fn run_controller_test_request(&self, req: CallProfileOrStartController, is_continuous: bool, token: CancellationToken) -> Promise<IStartControllerTestsResult> {
        let (controller_id, profile_id, test_ids, exclude_ext_ids) = match &req {
            CallProfileOrStartController::Call(call) => (call.controller_id.clone(), call.profile_id, call.test_ids.clone(), call.exclude_ext_ids.clone()),
            CallProfileOrStartController::Start(start) => (start.controller_id.clone(), start.profile_id, start.test_ids.clone(), start.exclude_ext_ids.clone()),
        };

        let Some(lookup) = self.controller(&controller_id) else {
            return Promise::resolved(IStartControllerTestsResult::default());
        };

        let Some(profile) = lookup.profiles.borrow().get(&profile_id).cloned() else {
            return Promise::resolved(IStartControllerTestsResult::default());
        };

        let tree = lookup.collection.tree.borrow();
        let include_tests: Vec<CollectionItemRef<TestItemImpl>> = test_ids.iter().filter_map(|test_id| tree.get(test_id).cloned()).collect();

        let exclude_tests: Vec<CollectionItemRef<TestItemImpl>> = exclude_ext_ids
            .iter()
            .filter_map(|id| tree.get(id).cloned())
            .filter(|exclude| include_tests.iter().any(|include| include.borrow().full_id.compare(&exclude.borrow().full_id) == TestPosition::IsChild))
            .collect();
        drop(tree);

        if include_tests.is_empty() {
            return Promise::resolved(IStartControllerTestsResult::default());
        }

        let public_req = Rc::new(TestRunRequest::new(
            if include_tests.iter().any(|include| include.borrow().actual.is_root_impl()) {
                None
            } else {
                Some(include_tests.iter().map(|test| test.borrow().actual.clone()).collect())
            },
            Some(exclude_tests.iter().map(|test| test.borrow().actual.clone()).collect()),
            Some(profile.clone()),
            is_continuous,
            true,
        ));

        let tracker = match &req {
            CallProfileOrStartController::Start(start) => Some(self.run_tracker.prepare_for_main_thread_test_run(
                &lookup.extension,
                &public_req,
                TestRunDto::from_internal(start, &lookup.collection),
                &profile,
                token.clone(),
            )),
            CallProfileOrStartController::Call(_) => None,
        };

        async_fn(async move {
            let run_handler = profile.run_handler.borrow().clone();
            let outcome = match errors::catch(|| run_handler(&public_req, &token)) {
                Ok(pending) => await_maybe(pending).await,
                Err(error) => Err(error),
            };
            let result = IStartControllerTestsResult { error: outcome.err().map(|error| error.to_string()) };

            if let Some(tracker) = tracker
                && tracker.has_running_tasks()
                && !token.is_cancellation_requested()
            {
                let _ = tracker.on_end().to_promise(None).await;
            }

            Ok(result)
        })
    }
}

impl IDisposable for ExtHostTesting {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ExtHostTestingShape for ExtHostTesting {
    fn rpc_run_controller_tests(&self, req: Vec<IStartControllerTests>, token: CancellationToken) -> Promise<Vec<IStartControllerTestsResult>> {
        Promise::all(
            req.into_iter()
                .map(|req| self.run_controller_test_request(CallProfileOrStartController::Start(req), false, token.clone()))
                .collect(),
        )
    }

    fn rpc_start_continuous_run(&self, req: Vec<ICallProfileRunHandler>, token: CancellationToken) -> Promise<Vec<IStartControllerTestsResult>> {
        let cts = CancellationTokenSource::new(Some(&token));
        let pending = Promise::all(
            req.into_iter()
                .map(|req| self.run_controller_test_request(CallProfileOrStartController::Call(req), true, cts.token()))
                .collect(),
        );

        async_fn(async move {
            let res = pending.await?;

            // avoid returning until cancellation is requested, otherwise ipc disposes of the token
            if !token.is_cancellation_requested() && !res.iter().any(|result| result.error.is_some()) {
                let (cancelled, resolver) = Promise::<()>::new();
                let _listener = token.on_cancellation_requested().listen(move |_| resolver.resolve(()));
                cancelled.await?;
            }

            cts.dispose_with(true);
            Ok(res)
        })
    }

    /// Cancels an ongoing test run.
    fn rpc_cancel_extension_test_run(&self, run_id: Option<&str>, task_id: Option<&str>) {
        match run_id {
            None => self.run_tracker.cancel_all_runs(),
            Some(run_id) => self.run_tracker.cancel_run_by_id(run_id, task_id),
        }
    }

    /// Receives a test update from the main thread. Called (eventually) whenever tests change.
    fn rpc_accept_diff(&self, diff: Vec<TestsDiffOpSerialized>) {
        let diff: TestsDiff = diff.iter().map(|op| TestsDiffOp::deserialize(&IdentityCanonicalizer, op)).collect();
        self.observer.apply_diff(&diff);
    }

    /// Expands the nodes in the test tree. If levels is less than zero, it will be treated as infinite.
    fn rpc_expand_test(&self, test_id: &str, levels: f64) -> Promise<()> {
        let collection = self.controller(TestId::from_string(test_id).controller_id()).map(|controller| controller.collection.clone());
        let test_id = test_id.to_string();
        async_fn(async move {
            if let Some(collection) = collection {
                if let Some(expansion) = collection.expand(&test_id, if levels < 0.0 { f64::INFINITY } else { levels }) {
                    expansion.await?;
                }
                collection.flush_diff();
            }
            Ok(())
        })
    }

    fn rpc_get_coverage_details(&self, coverage_id: &str, test_id: Option<&str>, token: CancellationToken) -> Promise<Vec<CoverageDetails>> {
        self.run_tracker
            .get_coverage_details(coverage_id, test_id, &token)
            .then_result(|details| details?.iter().map(convert::TestCoverage::from_details).collect())
    }

    fn rpc_dispose_run(&self, run_id: &str) {
        self.run_tracker.dispose_test_run(run_id);
    }

    fn rpc_configure_run_profile(&self, controller_id: &str, config_id: u32) {
        let handler = self
            .controller(controller_id)
            .and_then(|controller| controller.profiles.borrow().get(&config_id).cloned())
            .and_then(|profile| profile.configure_handler());
        if let Some(handler) = handler {
            handler();
        }
    }

    fn rpc_refresh_tests(&self, controller_id: &str, token: CancellationToken) -> Promise<()> {
        let handler = self.controller(controller_id).and_then(|controller| controller.refresh_handler());
        async_fn(async move {
            if let Some(handler) = handler {
                await_maybe(handler(&token)).await?;
            }
            Ok(())
        })
    }

    fn rpc_sync_tests(&self) -> Promise<()> {
        for controller in self.controllers.borrow().values() {
            controller.collection.flush_diff();
        }

        Promise::resolved(())
    }

    fn rpc_set_default_run_profiles(&self, profiles: Vec<(String, Vec<u32>)>) {
        let mut evt: DefaultProfileChangeEvent = IndexMap::new();
        for (controller_id, profile_ids) in profiles {
            let Some(ctrl) = self.controller(&controller_id) else {
                continue;
            };
            let mut changes: IndexMap<u32, bool> = IndexMap::new();
            let added: Vec<u32> = profile_ids.iter().filter(|id| !ctrl.active_profiles.borrow().contains(*id)).copied().collect();
            let removed: Vec<u32> = ctrl.active_profiles.borrow().iter().filter(|id| !profile_ids.contains(id)).copied().collect();
            for id in added {
                changes.insert(id, true);
                ctrl.active_profiles.borrow_mut().insert(id);
            }
            for id in removed {
                changes.insert(id, false);
                ctrl.active_profiles.borrow_mut().shift_remove(&id);
            }
            if !changes.is_empty() {
                evt.insert(controller_id, changes);
            }
        }

        self.default_profiles_changed_emitter.fire(evt);
    }

    fn rpc_get_tests_related_to_code(&self, uri: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<String>> {
        let Some(doc) = self.editors.get_document(uri) else {
            return Promise::resolved(Vec::new());
        };

        let position = match Position::to(&position) {
            Ok(position) => position,
            Err(error) => return Promise::rejected(error),
        };
        let document = doc.document();
        let related: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let controllers: Vec<Rc<TestController>> = self.controllers.borrow().values().cloned().collect();
        let pending: Vec<Promise<()>> = controllers
            .into_iter()
            .map(|controller| {
                let related = related.clone();
                let document = document.clone();
                let token = token.clone();
                let log_service = self.log_service.clone();
                async_fn(async move {
                    let mut tests: Option<Vec<TestItemImpl>> = None;
                    if let Some(provider) = controller.related_code_provider()
                        && provider.supports_related_tests()
                    {
                        let provided = match errors::catch(|| provider.provide_related_tests(&document, &position, &token)) {
                            Ok(promise) => promise.await,
                            Err(error) => Err(error),
                        };
                        match provided {
                            Ok(found) => tests = found,
                            Err(error) => {
                                if !token.is_cancellation_requested() {
                                    log_service.warn(&format!("Error thrown while providing related tests for {}: {error}", controller.label()));
                                }
                            }
                        }
                    }

                    if let Some(tests) = tests {
                        for test in tests {
                            related.borrow_mut().push(TestId::from_ext_host_test_item(&test, &controller.id, None).to_string());
                        }
                        controller.collection.flush_diff();
                    }
                    Ok(())
                })
            })
            .collect();

        async_fn(async move {
            Promise::all(pending).await?;
            let related = related.borrow().clone();
            Ok(related)
        })
    }

    fn rpc_get_code_related_to_test(&self, test_id: &str, token: CancellationToken) -> Promise<Vec<LocationDto>> {
        let Some(controller) = self.controller(TestId::root_string(test_id)) else {
            return Promise::resolved(Vec::new());
        };

        let Some(test) = controller.collection.tree.borrow().get(test_id).map(|item| item.borrow().actual.clone()) else {
            return Promise::resolved(Vec::new());
        };

        async_fn(async move {
            let locations = match controller.related_code_provider() {
                Some(provider) if provider.supports_related_code() => provider.provide_related_code(&test, &token).await?,
                _ => None,
            };
            Ok(locations.map(|locations| locations.iter().map(Location::from).collect()).unwrap_or_default())
        })
    }

    /// Updates test results shown to extensions.
    fn rpc_publish_test_results(&self, results: Vec<ISerializedTestResults>) {
        let mut converted: Vec<(Rc<TestRunResult>, String)> = results
            .iter()
            .map(|serialized| {
                let result = convert::TestResults::to(serialized);
                if let Some(task_with_coverage) = serialized.tasks.iter().position(|task| task.has_coverage) {
                    let proxy = self.proxy.clone();
                    let result_id = serialized.id.clone();
                    *result.get_detailed_coverage.borrow_mut() = Some(Rc::new(move |uri: &URI, token: &CancellationToken| {
                        proxy
                            .rpc_get_coverage_details(&result_id, task_with_coverage, uri, token.clone())
                            .then_result(|details| details?.iter().map(convert::TestCoverage::to).collect())
                    }));
                }

                (Rc::new(result), serialized.id.clone())
            })
            .collect();
        converted.extend(self.results.borrow().iter().cloned());
        converted.sort_by(|a, b| b.0.completed_at.partial_cmp(&a.0.completed_at).unwrap_or(std::cmp::Ordering::Equal));
        converted.truncate(32);
        *self.results.borrow_mut() = converted;

        self.results_changed_emitter.fire(());
    }

    fn rpc_provide_test_followups(&self, req: TestMessageFollowupRequest, token: CancellationToken) -> Promise<Vec<TestMessageFollowupResponse>> {
        let results = self.results.borrow().iter().find(|(_, id)| *id == req.result_id).map(|(result, _)| result.clone());
        let test = results.as_ref().and_then(|results| find_test_in_result_snapshot(&TestId::from_string(&req.ext_id), &results.results));
        let (Some(results), Some(test)) = (results, test) else {
            return Promise::resolved(Vec::new());
        };

        let providers: Vec<Rc<dyn TestFollowupProvider>> = self.followup_providers.borrow().clone();
        let log_service = self.log_service.clone();
        let this = self.this.clone();
        async_fn(async move {
            let pending: Vec<Promise<Option<Vec<Command>>>> = providers
                .iter()
                .map(|provider| match errors::catch(|| provider.provide_followup(&results, &test, req.task_index, req.message_index, &token)) {
                    Ok(promise) => promise,
                    Err(error) => Promise::rejected(error),
                })
                .collect();

            let mut followups: Vec<Command> = Vec::new();
            for settled in Promise::all_settled(pending).await? {
                match settled {
                    Ok(Some(commands)) => followups.extend(commands),
                    Ok(None) => {}
                    Err(error) => log_service.error(&format!("Error thrown while providing followup for test message {error}")),
                }
            }

            if token.is_cancellation_requested() {
                return Ok(Vec::new());
            }

            let Some(this) = this.upgrade() else {
                return Ok(Vec::new());
            };
            Ok(followups
                .into_iter()
                .map(|command| {
                    let id = FOLLOWUP_COUNTER.with(|counter| {
                        let id = counter.get();
                        counter.set(id + 1);
                        id
                    });
                    let title = command.title.clone();
                    this.test_followups.borrow_mut().insert(id, command);
                    TestMessageFollowupResponse { title, id }
                })
                .collect())
        })
    }

    fn rpc_dispose_test_followups(&self, id: Vec<u32>) {
        for i in id {
            self.test_followups.borrow_mut().remove(&i);
        }
    }

    fn rpc_execute_test_followup(&self, id: u32) -> Promise<()> {
        let Some(command) = self.test_followups.borrow().get(&id).cloned() else {
            return Promise::resolved(());
        };

        self.commands.execute_command(&command.command, command.arguments.clone().unwrap_or_default()).then(|_| ())
    }
}

/// `vscode.TestController`.
pub struct TestController {
    id: String,
    label: RefCell<String>,
    refresh_handler: RefCell<Option<RefreshHandler>>,
    related_code_provider: RefCell<Option<Rc<dyn TestRelatedCodeProvider>>>,
    pub collection: Rc<ExtHostTestItemCollection>,
    profiles: ProfileMap,
    active_profiles: ActiveProfiles,
    extension: ExtensionDescription,
    proxy: Rc<dyn MainThreadTestingShape>,
    run_tracker: Rc<TestRunCoordinator>,
    default_profiles_changed: Event<DefaultProfileChangeEvent>,
    disposable: Rc<DisposableStore>,
}

impl TestController {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// `items`.
    pub fn items(&self) -> Rc<dyn ITestItemChildren<TestItemImpl>> {
        self.collection.root().children()
    }

    pub fn label(&self) -> String {
        self.label.borrow().clone()
    }

    pub fn set_label(&self, value: &str) {
        *self.label.borrow_mut() = value.to_string();
        self.collection.root().set_label(value);
        self.proxy.rpc_update_controller(&self.id, ITestControllerPatch { label: Some(value.to_string()), capabilities: None });
    }

    pub fn refresh_handler(&self) -> Option<RefreshHandler> {
        self.refresh_handler.borrow().clone()
    }

    pub fn set_refresh_handler(&self, value: Option<RefreshHandler>) {
        *self.refresh_handler.borrow_mut() = value;
        self.proxy.rpc_update_controller(&self.id, ITestControllerPatch { label: None, capabilities: Some(self.get_capability()) });
    }

    pub fn related_code_provider(&self) -> Option<Rc<dyn TestRelatedCodeProvider>> {
        self.related_code_provider.borrow().clone()
    }

    /// O setter de `relatedCodeProvider`: exige a proposta `testRelatedCode`.
    pub fn set_related_code_provider(&self, value: Option<Rc<dyn TestRelatedCodeProvider>>) -> Result<(), Error> {
        check_proposed_api_enabled(&self.extension, "testRelatedCode")?;
        *self.related_code_provider.borrow_mut() = value;
        self.proxy.rpc_update_controller(&self.id, ITestControllerPatch { label: None, capabilities: Some(self.get_capability()) });
        Ok(())
    }

    /// `resolveHandler`.
    pub fn resolve_handler(&self) -> Option<ResolveHandler<TestItemImpl>> {
        self.collection.resolve_handler()
    }

    pub fn set_resolve_handler(&self, handler: Option<ResolveHandler<TestItemImpl>>) {
        self.collection.set_resolve_handler(handler);
    }

    /// Os perfis de execução do controlador, na ordem de criação.
    pub fn profiles(&self) -> Vec<(u32, Rc<TestRunProfileImpl>)> {
        self.profiles.borrow().iter().map(|(id, profile)| (*id, profile.clone())).collect()
    }

    fn get_capability(&self) -> TestControllerCapability {
        let mut cap = 0;
        if self.refresh_handler.borrow().is_some() {
            cap |= test_controller_capability::REFRESH;
        }
        if let Some(rcp) = self.related_code_provider.borrow().as_ref() {
            if rcp.supports_related_tests() {
                cap |= test_controller_capability::TEST_RELATED_TO_CODE;
            }
            if rcp.supports_related_code() {
                cap |= test_controller_capability::CODE_RELATED_TO_TEST;
            }
        }
        cap
    }

    /// `createRunProfile(label, group, runHandler, isDefault, tag, supportsContinuousRun)`.
    pub fn create_run_profile(
        &self,
        label: &str,
        group: types::TestRunProfileKind,
        run_handler: RunHandler,
        is_default: bool,
        tag: Option<api::TestTag>,
        supports_continuous_run: bool,
    ) -> Rc<TestRunProfileImpl> {
        // Derive the profile ID from a hash so that the same profile will tend
        // to have the same hashes, allowing re-run requests to work across reloads.
        let mut profile_id = hash(&Dynamic::string(label)) as u32;
        while self.profiles.borrow().contains_key(&profile_id) {
            profile_id = profile_id.wrapping_add(1);
        }

        TestRunProfileImpl::new(
            self.proxy.clone(),
            self.profiles.clone(),
            self.active_profiles.clone(),
            self.default_profiles_changed.clone(),
            &self.id,
            profile_id,
            label,
            group,
            run_handler,
            is_default,
            tag,
            supports_continuous_run,
        )
    }

    /// `createTestItem(id, label, uri)`.
    pub fn create_test_item(&self, id: &str, label: &str, uri: Option<URI>) -> Result<TestItemImpl, Error> {
        TestItemImpl::new(&self.id, id, label, uri)
    }

    /// `createTestRun(request, name, persist = true)`.
    pub fn create_test_run(&self, request: &Rc<TestRunRequest>, name: Option<&str>, persist: bool) -> Rc<TestRun> {
        self.run_tracker.create_test_run(&self.extension, &self.id, &self.collection, request, name, persist)
    }

    /// `invalidateTestResults(items?)`.
    pub fn invalidate_test_results(&self, items: Option<&[TestItemImpl]>) {
        match items {
            None => self.proxy.rpc_mark_test_retired(None),
            Some(items) => self
                .proxy
                .rpc_mark_test_retired(Some(items.iter().map(|item| TestId::from_ext_host_test_item(item, &self.id, None).to_string()).collect())),
        }
    }
}

impl IDisposable for TestController {
    fn dispose(&self) {
        self.disposable.dispose();
    }
}

// Deadline after being requested by a user that a test run is forcibly cancelled.
const RUN_CANCEL_DEADLINE: u64 = 10_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TestRunTrackerState {
    // Default state
    Running,
    // Cancellation is requested, but the run is still going.
    Cancelling,
    // All tasks have ended
    Ended,
}

struct TrackedTask {
    cts: CancellationTokenSource,
    run: Rc<TestRun>,
}

struct PublishedCoverage {
    report: FileCoverage,
    ext_ids: Vec<String>,
}

/// `vscode.TestRun`.
pub struct TestRun {
    tracker: Weak<TestRunTracker>,
    run_id: String,
    ctrl_id: String,
    task_id: String,
    name: Option<String>,
    is_persisted: bool,
    token: CancellationToken,
    on_did_dispose: Event<()>,
    ended: Cell<bool>,
}

impl TestRun {
    pub fn is_persisted(&self) -> bool {
        self.is_persisted
    }

    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    pub fn name(&self) -> Option<String> {
        self.name.clone()
    }

    pub fn on_did_dispose(&self) -> Event<()> {
        self.on_did_dispose.clone()
    }

    /// `guardTestMutation(fn)`.
    fn guard_test_mutation(&self, test: &TestItemImpl, mutation: impl FnOnce(&TestRunTracker)) {
        let Some(tracker) = self.tracker.upgrade() else {
            return;
        };

        if self.ended.get() {
            tracker.log_service.warn(&format!("Setting the state of test \"{}\" is a no-op after the run ends.", test.id()));
            return;
        }

        tracker.ensure_test_is_known(test);
        mutation(&tracker);
    }

    fn update_state(&self, tracker: &TestRunTracker, test: &TestItemImpl, state: TestResultState, duration: Option<f64>) {
        tracker.proxy.rpc_update_test_state_in_run(
            &self.run_id,
            &self.task_id,
            &TestId::from_ext_host_test_item(test, &self.ctrl_id, None).to_string(),
            state,
            duration,
        );
    }

    fn append_messages(&self, tracker: &TestRunTracker, test: &TestItemImpl, messages: &[api::TestMessage]) {
        let mut converted: Vec<ITestErrorMessageSerialized> = messages.iter().map(convert::TestMessage::from).collect();

        if let (Some(uri), Some(range)) = (test.uri(), test.range()) {
            let default_location = crate::contrib::testing::common::test_types::IRichLocationSerialized { range: Range::from(&range), uri: uri.to_json() };
            for message in &mut converted {
                if message.location.is_none() {
                    message.location = Some(default_location.clone());
                }
            }
        }

        tracker.proxy.rpc_append_test_messages_in_run(
            &self.run_id,
            &self.task_id,
            &TestId::from_ext_host_test_item(test, &self.ctrl_id, None).to_string(),
            converted.into_iter().map(ITestMessageSerialized::Error).collect(),
        );
    }

    /// `addCoverage(coverage)`.
    pub fn add_coverage(&self, coverage: &FileCoverage) -> Result<(), Error> {
        if self.ended.get() {
            return Ok(());
        }
        let Some(tracker) = self.tracker.upgrade() else {
            return Ok(());
        };

        for test in &coverage.includes_tests {
            tracker.ensure_test_is_known(test);
        }

        let uri_str = coverage.uri.to_string(false).to_string_lossy();
        let id = TestId::new(vec![self.run_id.clone(), self.task_id.clone(), uri_str]).to_string();
        // it's a lil funky, but it's possible for a test item's ID to change after
        // it's been reported if it's rehomed under a different parent. Record its
        // ID at the time when the coverage report is generated so we can reference
        // it later if needeed.
        tracker.published_coverage.borrow_mut().insert(
            id.clone(),
            PublishedCoverage {
                report: coverage.clone(),
                ext_ids: coverage.includes_tests.iter().map(|test| TestId::from_ext_host_test_item(test, &self.ctrl_id, None).to_string()).collect(),
            },
        );
        tracker.proxy.rpc_append_coverage(&self.run_id, &self.task_id, convert::TestCoverage::from_file(&self.ctrl_id, &id, coverage)?);
        Ok(())
    }

    pub fn enqueued(&self, test: &TestItemImpl) {
        self.guard_test_mutation(test, |tracker| self.update_state(tracker, test, TestResultState::Queued, None));
    }

    pub fn skipped(&self, test: &TestItemImpl) {
        self.guard_test_mutation(test, |tracker| self.update_state(tracker, test, TestResultState::Skipped, None));
    }

    pub fn started(&self, test: &TestItemImpl) {
        self.guard_test_mutation(test, |tracker| self.update_state(tracker, test, TestResultState::Running, None));
    }

    pub fn errored(&self, test: &TestItemImpl, messages: &[api::TestMessage], duration: Option<f64>) {
        self.guard_test_mutation(test, |tracker| {
            self.append_messages(tracker, test, messages);
            self.update_state(tracker, test, TestResultState::Errored, duration);
        });
    }

    pub fn failed(&self, test: &TestItemImpl, messages: &[api::TestMessage], duration: Option<f64>) {
        self.guard_test_mutation(test, |tracker| {
            self.append_messages(tracker, test, messages);
            self.update_state(tracker, test, TestResultState::Failed, duration);
        });
    }

    pub fn passed(&self, test: &TestItemImpl, duration: Option<f64>) {
        self.guard_test_mutation(test, |tracker| self.update_state(tracker, test, TestResultState::Passed, duration));
    }

    /// `appendOutput(output, location?, test?)`.
    pub fn append_output(&self, output: &str, location: Option<&types::Location>, test: Option<&TestItemImpl>) {
        if self.ended.get() {
            return;
        }
        let Some(tracker) = self.tracker.upgrade() else {
            return;
        };

        if let Some(test) = test {
            tracker.ensure_test_is_known(test);
        }

        tracker.proxy.rpc_append_output_to_run(
            &self.run_id,
            &self.task_id,
            VSBuffer::from_string(output),
            location.map(Location::from),
            test.map(|test| TestId::from_ext_host_test_item(test, &self.ctrl_id, None).to_string()),
        );
    }

    pub fn end(&self) {
        if self.ended.get() {
            return;
        }

        self.ended.set(true);
        let Some(tracker) = self.tracker.upgrade() else {
            return;
        };
        tracker.proxy.rpc_finished_test_run_task(&self.run_id, &self.task_id);
        tracker.running.set(tracker.running.get() - 1);
        if tracker.running.get() == 0 {
            tracker.mark_ended();
        }
    }
}

/// `TestRunTracker`.
pub struct TestRunTracker {
    this: Weak<TestRunTracker>,
    base: Rc<Disposable>,
    state: Cell<TestRunTrackerState>,
    running: Cell<usize>,
    tasks: RefCell<IndexMap<String, TrackedTask>>,
    shared_test_ids: RefCell<HashSet<String>>,
    cts: CancellationTokenSource,
    end_emitter: Rc<Emitter<()>>,
    did_dispose_emitter: Rc<Emitter<()>>,
    published_coverage: RefCell<HashMap<String, PublishedCoverage>>,
    dto: TestRunDto,
    proxy: Rc<dyn MainThreadTestingShape>,
    log_service: LogServiceRef,
    profile: Option<Rc<TestRunProfileImpl>>,
    extension: ExtensionDescription,
}

impl TestRunTracker {
    fn new(
        dto: TestRunDto,
        proxy: Rc<dyn MainThreadTestingShape>,
        log_service: LogServiceRef,
        profile: Option<Rc<TestRunProfileImpl>>,
        extension: ExtensionDescription,
        parent_token: Option<&CancellationToken>,
    ) -> Rc<TestRunTracker> {
        let base = Disposable::new();
        let cts = CancellationTokenSource::new(parent_token);
        base.register(Rc::new(cts.clone()));
        let end_emitter = Emitter::new();
        base.register(end_emitter.clone());
        let did_dispose_emitter: Rc<Emitter<()>> = Emitter::new();

        let tracker = Rc::new_cyclic(|this: &Weak<TestRunTracker>| {
            let forcibly_end = {
                let this = this.clone();
                RunOnceScheduler::new(
                    move || {
                        if let Some(tracker) = this.upgrade() {
                            tracker.forcibly_end_tasks();
                        }
                    },
                    RUN_CANCEL_DEADLINE,
                )
            };
            base.register(forcibly_end.clone());
            base.register(cts.token().on_cancellation_requested().listen(move |_| forcibly_end.schedule(None)));

            let dispose_emitter = did_dispose_emitter.clone();
            base.register(to_disposable(move || {
                dispose_emitter.fire(());
                dispose_emitter.dispose();
            }));

            TestRunTracker {
                this: this.clone(),
                base: base.clone(),
                state: Cell::new(TestRunTrackerState::Running),
                running: Cell::new(0),
                tasks: RefCell::new(IndexMap::new()),
                shared_test_ids: RefCell::new(HashSet::new()),
                cts,
                end_emitter,
                did_dispose_emitter,
                published_coverage: RefCell::new(HashMap::new()),
                dto,
                proxy,
                log_service,
                profile,
                extension,
            }
        });
        tracker
    }

    /// Fires when a test ends, and no more tests are left running.
    pub fn on_end(&self) -> Event<()> {
        self.end_emitter.event()
    }

    /// Gets whether there are any tests running.
    pub fn has_running_tasks(&self) -> bool {
        self.running.get() > 0
    }

    /// Gets the run ID.
    pub fn id(&self) -> String {
        self.dto.id.clone()
    }

    /// Gets the task ID from a test run object.
    pub fn get_task_id_for_run(&self, run: &Rc<TestRun>) -> Option<String> {
        self.tasks.borrow().iter().find(|(_, task)| Rc::ptr_eq(&task.run, run)).map(|(task_id, _)| task_id.clone())
    }

    /// Requests cancellation of the run. On the second call, forces cancellation.
    pub fn cancel(&self, task_id: Option<&str>) {
        if let Some(task_id) = task_id {
            let cts = self.tasks.borrow().get(task_id).map(|task| task.cts.clone());
            if let Some(cts) = cts {
                cts.cancel();
            }
        } else if self.state.get() == TestRunTrackerState::Running {
            self.cts.cancel();
            self.state.set(TestRunTrackerState::Cancelling);
        } else if self.state.get() == TestRunTrackerState::Cancelling {
            self.forcibly_end_tasks();
        }
    }

    /// Gets details for a previously-emitted coverage object.
    pub fn get_coverage_details(&self, id: &str, test_id: Option<&str>, token: &CancellationToken) -> Promise<Vec<FileCoverageDetail>> {
        let task_id = TestId::from_string(id).path().get(1).cloned().unwrap_or_default(); /** runId, taskId, URI */
        let coverage = self.published_coverage.borrow().get(id).map(|coverage| (coverage.report.clone(), coverage.ext_ids.clone()));
        let Some((report, ext_ids)) = coverage else {
            return Promise::resolved(Vec::new());
        };

        let run = self.tasks.borrow().get(&task_id).map(|task| task.run.clone());
        let Some(run) = run else {
            return Promise::rejected(Error::new("unreachable: run task was not found"));
        };

        let mut test_item: Option<TestItemImpl> = None;
        if let Some(test_id) = test_id {
            let Some(index) = ext_ids.iter().position(|ext_id| ext_id == test_id) else {
                return Promise::resolved(Vec::new()); // ??
            };
            test_item = report.includes_tests.get(index).cloned();
        }

        let details = match (&test_item, &self.profile) {
            (Some(test), Some(profile)) => profile.load_detailed_coverage_for_test().map(|load| load(&run, &report, test, token)),
            (None, Some(profile)) => profile.load_detailed_coverage().map(|load| load(&run, &report, token)),
            _ => None,
        };

        details.unwrap_or_else(|| Promise::resolved(Vec::new()))
    }

    /// Creates the public test run interface to give to extensions.
    pub fn create_run(&self, name: Option<&str>) -> Rc<TestRun> {
        let task_id = generate_uuid();
        // tasks are alive for as long as the tracker is alive, so simple register is fine:
        let cts = CancellationTokenSource::new(Some(&self.cts.token()));
        self.base.register(Rc::new(cts.clone()));

        let run = Rc::new(TestRun {
            tracker: self.this.clone(),
            run_id: self.dto.id.clone(),
            ctrl_id: self.dto.controller_id.clone(),
            task_id: task_id.clone(),
            name: name.map(str::to_string),
            is_persisted: self.dto.is_persisted,
            token: cts.token(),
            on_did_dispose: self.did_dispose_emitter.event(),
            ended: Cell::new(false),
        });

        self.running.set(self.running.get() + 1);
        self.tasks.borrow_mut().insert(task_id.clone(), TrackedTask { cts, run: run.clone() });
        self.proxy.rpc_started_test_run_task(
            &self.dto.id,
            ITestRunTask {
                id: task_id,
                ctrl_id: self.dto.controller_id.clone(),
                name: name
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
                    .or_else(|| self.extension.manifest.display_name.clone().filter(|display_name| !display_name.is_empty()))
                    .unwrap_or_else(|| self.extension.identifier.value.clone()),
                running: true,
            },
        );

        run
    }

    fn forcibly_end_tasks(&self) {
        let runs: Vec<Rc<TestRun>> = self.tasks.borrow().values().map(|task| task.run.clone()).collect();
        for run in runs {
            run.end();
        }
    }

    fn mark_ended(&self) {
        if self.state.get() != TestRunTrackerState::Ended {
            self.state.set(TestRunTrackerState::Ended);
            self.end_emitter.fire(());
        }
    }

    fn ensure_test_is_known(&self, test: &TestItemImpl) {
        let controller_id = &self.dto.controller_id;
        if self.shared_test_ids.borrow().contains(&TestId::from_ext_host_test_item(test, controller_id, None).to_string()) {
            return;
        }

        let mut chain: Vec<crate::contrib::testing::common::test_types::ITestItemSerialized> = Vec::new();
        let root = self.dto.collection.root().clone();
        let mut test = test.clone();
        loop {
            let converted = convert::TestItem::from(&test);
            chain.insert(0, ITestItem::serialize(&converted));

            if self.shared_test_ids.borrow().contains(&converted.ext_id) {
                break;
            }

            self.shared_test_ids.borrow_mut().insert(converted.ext_id.clone());
            if test.is_same(&root) {
                break;
            }

            test = test.parent().unwrap_or_else(|| root.clone());
        }

        self.proxy.rpc_add_tests_to_run(controller_id, &self.dto.id, chain);
    }
}

impl IDisposable for TestRunTracker {
    fn dispose(&self) {
        self.mark_ended();
        self.base.dispose();
    }
}

/// Queues runs for a single extension and provides the currently-executing
/// run so that `createTestRun` can be properly correlated.
pub struct TestRunCoordinator {
    tracked: RefCell<IndexMap<usize, (Rc<TestRunRequest>, Rc<TestRunTracker>)>>,
    tracked_by_id: RefCell<HashMap<String, Rc<TestRunTracker>>>,
    proxy: Rc<dyn MainThreadTestingShape>,
    log_service: LogServiceRef,
}

impl TestRunCoordinator {
    pub fn new(proxy: Rc<dyn MainThreadTestingShape>, log_service: LogServiceRef) -> TestRunCoordinator {
        TestRunCoordinator { tracked: RefCell::new(IndexMap::new()), tracked_by_id: RefCell::new(HashMap::new()), proxy, log_service }
    }

    /// `trackers`.
    pub fn trackers(&self) -> Vec<Rc<TestRunTracker>> {
        self.tracked.borrow().values().map(|(_, tracker)| tracker.clone()).collect()
    }

    /// Gets a coverage report for a given run and task ID.
    pub fn get_coverage_details(&self, id: &str, test_id: Option<&str>, token: &CancellationToken) -> Promise<Vec<FileCoverageDetail>> {
        let tracker = self.tracked_by_id.borrow().get(TestId::root_string(id)).cloned();
        match tracker {
            Some(tracker) => tracker.get_coverage_details(id, test_id, token),
            None => Promise::resolved(Vec::new()),
        }
    }

    /// Disposes the test run, called when the main thread is no longer interested
    /// in associated data.
    pub fn dispose_test_run(&self, run_id: &str) {
        let tracker = self.tracked_by_id.borrow_mut().remove(run_id);
        if let Some(tracker) = tracker {
            tracker.dispose();
        }
        self.tracked.borrow_mut().retain(|_, (_, tracker)| tracker.id() != run_id);
    }

    /// Registers a request as being invoked by the main thread, so
    /// `$startedExtensionTestRun` is not invoked. The run must eventually
    /// be cancelled manually.
    pub fn prepare_for_main_thread_test_run(
        &self,
        extension: &ExtensionDescription,
        req: &Rc<TestRunRequest>,
        dto: TestRunDto,
        profile: &Rc<TestRunProfileImpl>,
        token: CancellationToken,
    ) -> Rc<TestRunTracker> {
        self.get_tracker(req, dto, Some(profile.clone()), extension, Some(&token))
    }

    /// Cancels an existing test run via its cancellation token.
    pub fn cancel_run_by_id(&self, run_id: &str, task_id: Option<&str>) {
        let tracker = self.tracked_by_id.borrow().get(run_id).cloned();
        if let Some(tracker) = tracker {
            tracker.cancel(task_id);
        }
    }

    /// Cancels an existing test run via its cancellation token.
    pub fn cancel_all_runs(&self) {
        for tracker in self.trackers() {
            tracker.cancel(None);
        }
    }

    /// Implements the public `createTestRun` API.
    pub fn create_test_run(
        &self,
        extension: &ExtensionDescription,
        controller_id: &str,
        collection: &Rc<ExtHostTestItemCollection>,
        request: &Rc<TestRunRequest>,
        name: Option<&str>,
        persist: bool,
    ) -> Rc<TestRun> {
        let existing = self.tracked.borrow().get(&(Rc::as_ptr(request) as usize)).map(|(_, tracker)| tracker.clone());
        if let Some(existing) = existing {
            return existing.create_run(name);
        }

        // If there is not an existing tracked extension for the request, start
        // a new, detached session.
        let dto = TestRunDto::from_public(controller_id, collection, request, persist);
        let profile = request.profile.clone();
        let root_id = collection.root().id();
        let to_ids = |items: &[TestItemImpl]| -> Vec<String> { items.iter().map(|test| TestId::from_ext_host_test_item(test, &root_id, None).to_string()).collect() };
        self.proxy.rpc_started_extension_test_run(ExtensionRunTestsRequest {
            controller_id: controller_id.to_string(),
            continuous: request.continuous,
            profile: profile.as_ref().map(|profile| ExtensionRunProfile { group: convert::TestRunProfileKind::from(profile.base.kind), id: profile.base.profile_id }),
            exclude: request.exclude.as_deref().map(to_ids).unwrap_or_default(),
            id: dto.id.clone(),
            include: request.include.as_deref().map(to_ids).unwrap_or_else(|| vec![root_id.clone()]),
            preserve_focus: request.preserve_focus,
            persist,
        });

        let run_id = dto.id.clone();
        let tracker = self.get_tracker(request, dto, profile, extension, None);
        let proxy = self.proxy.clone();
        tracker.on_end().once().listen(move |_| proxy.rpc_finished_extension_test_run(&run_id));

        tracker.create_run(name)
    }

    fn get_tracker(
        &self,
        req: &Rc<TestRunRequest>,
        dto: TestRunDto,
        profile: Option<Rc<TestRunProfileImpl>>,
        extension: &ExtensionDescription,
        token: Option<&CancellationToken>,
    ) -> Rc<TestRunTracker> {
        let tracker = TestRunTracker::new(dto, self.proxy.clone(), self.log_service.clone(), profile, extension.clone(), token);
        self.tracked.borrow_mut().insert(Rc::as_ptr(req) as usize, (req.clone(), tracker.clone()));
        self.tracked_by_id.borrow_mut().insert(tracker.id(), tracker.clone());
        tracker
    }
}

/// `TestRunDto`.
#[derive(Clone)]
pub struct TestRunDto {
    pub controller_id: String,
    pub id: String,
    pub is_persisted: bool,
    pub collection: Rc<ExtHostTestItemCollection>,
}

impl TestRunDto {
    pub fn from_public(controller_id: &str, collection: &Rc<ExtHostTestItemCollection>, _request: &Rc<TestRunRequest>, persist: bool) -> TestRunDto {
        TestRunDto { controller_id: controller_id.to_string(), id: generate_uuid(), is_persisted: persist, collection: collection.clone() }
    }

    pub fn from_internal(request: &IStartControllerTests, collection: &Rc<ExtHostTestItemCollection>) -> TestRunDto {
        TestRunDto { controller_id: request.controller_id.clone(), id: request.run_id.clone(), is_persisted: true, collection: collection.clone() }
    }
}

/// `vscode.TestsChangeEvent`: os itens simples (`revived`) que mudaram.
#[derive(Clone)]
pub struct TestsChangeEvent {
    pub added: Vec<TestItemRef>,
    pub updated: Vec<TestItemRef>,
    pub removed: Vec<TestItemRef>,
}

/// O `vscode.TestItem` simples (`revived`) de um nó espelhado, atualizado no lugar (`Object.assign`).
pub type TestItemRef = Rc<RefCell<api::PlainTestItem>>;

/// `MirroredCollectionTestItem` (`@private`).
struct MirroredCollectionTestItem {
    base: IncrementalTestCollectionItem,
    revived: TestItemRef,
    depth: usize,
}

impl IncrementalNode for MirroredCollectionTestItem {
    fn node(&self) -> &IncrementalTestCollectionItem {
        &self.base
    }

    fn node_mut(&mut self) -> &mut IncrementalTestCollectionItem {
        &mut self.base
    }
}

type MirroredNode = IncrementalNodeRef<MirroredCollectionTestItem>;

fn push_unique(list: &RefCell<Vec<MirroredNode>>, node: &MirroredNode) {
    if !list.borrow().iter().any(|candidate| Rc::ptr_eq(candidate, node)) {
        list.borrow_mut().push(node.clone());
    }
}

/// `MirroredChangeCollector`.
struct MirroredChangeCollector {
    emitter: Rc<Emitter<TestsChangeEvent>>,
    added: RefCell<Vec<MirroredNode>>,
    updated: RefCell<Vec<MirroredNode>>,
    removed: RefCell<Vec<MirroredNode>>,
    already_removed: RefCell<HashSet<String>>,
}

impl MirroredChangeCollector {
    fn is_empty(&self) -> bool {
        self.added.borrow().is_empty() && self.removed.borrow().is_empty() && self.updated.borrow().is_empty()
    }

    fn get_change_event(&self) -> TestsChangeEvent {
        let revived = |nodes: &RefCell<Vec<MirroredNode>>| nodes.borrow().iter().map(|node| node.borrow().revived.clone()).collect();
        TestsChangeEvent { added: revived(&self.added), updated: revived(&self.updated), removed: revived(&self.removed) }
    }
}

impl IncrementalChangeCollector<MirroredCollectionTestItem> for MirroredChangeCollector {
    fn add(&self, node: &MirroredNode) {
        push_unique(&self.added, node);
    }

    fn update(&self, node: &MirroredNode) {
        let plain = convert::TestItem::to_plain(&node.borrow().base.item);
        *node.borrow().revived.borrow_mut() = plain;
        if !self.added.borrow().iter().any(|candidate| Rc::ptr_eq(candidate, node)) {
            push_unique(&self.updated, node);
        }
    }

    fn remove(&self, node: &MirroredNode, _is_nested_operation: bool) {
        let added_at = self.added.borrow().iter().position(|candidate| Rc::ptr_eq(candidate, node));
        if let Some(index) = added_at {
            self.added.borrow_mut().remove(index);
            return;
        }

        self.updated.borrow_mut().retain(|candidate| !Rc::ptr_eq(candidate, node));

        let ext_id = node.borrow().base.item.ext_id.clone();
        if let Some(parent_id) = TestId::parent_id_string(&ext_id)
            && self.already_removed.borrow().contains(parent_id)
        {
            self.already_removed.borrow_mut().insert(ext_id);
            return;
        }

        push_unique(&self.removed, node);
    }

    fn complete(&self) {
        if !self.is_empty() {
            self.emitter.fire(self.get_change_event());
        }
    }
}

/// Os membros de `MirroredTestCollection` que sobrescrevem os da base.
struct MirroredHooks {
    change_emitter: Rc<Emitter<TestsChangeEvent>>,
}

impl IncrementalCollectionHooks<MirroredCollectionTestItem> for MirroredHooks {
    fn create_item(&self, item: &InternalTestItem, parent: Option<&MirroredNode>) -> MirroredCollectionTestItem {
        MirroredCollectionTestItem {
            base: IncrementalTestCollectionItem {
                controller_id: item.controller_id.clone(),
                expand: item.expand,
                item: item.item.clone(),
                children: IndexSet::new(),
            },
            // todo@connor4312: make this work well again with children
            revived: Rc::new(RefCell::new(convert::TestItem::to_plain(&item.item))),
            depth: parent.map_or(0, |parent| parent.borrow().depth + 1),
        }
    }

    fn create_change_collector(&self) -> Rc<dyn IncrementalChangeCollector<MirroredCollectionTestItem>> {
        Rc::new(MirroredChangeCollector {
            emitter: self.change_emitter.clone(),
            added: RefCell::new(Vec::new()),
            updated: RefCell::new(Vec::new()),
            removed: RefCell::new(Vec::new()),
            already_removed: RefCell::new(HashSet::new()),
        })
    }
}

/// Maintains tests in this extension host sent from the main thread.
struct MirroredTestCollection {
    inner: AbstractIncrementalTestCollection<MirroredCollectionTestItem>,
    change_emitter: Rc<Emitter<TestsChangeEvent>>,
}

impl MirroredTestCollection {
    fn new() -> MirroredTestCollection {
        let change_emitter = Emitter::new();
        let hooks = Rc::new(MirroredHooks { change_emitter: change_emitter.clone() });
        MirroredTestCollection { inner: AbstractIncrementalTestCollection::new(Rc::new(IdentityCanonicalizer), hooks), change_emitter }
    }

    /// Change emitter that fires with the same semantics as `TestObserver.onDidChangeTests`.
    fn on_did_change_tests(&self) -> Event<TestsChangeEvent> {
        self.change_emitter.event()
    }

    /// Gets a list of root test items.
    fn root_tests(&self) -> Vec<TestItemRef> {
        self.inner.roots.borrow().iter().map(|root| root.borrow().revived.clone()).collect()
    }
}

struct ObserverData {
    observers: Cell<usize>,
    tests: MirroredTestCollection,
}

/// `vscode.TestObserver`.
pub struct TestObserver {
    on_did_change_test: Event<TestsChangeEvent>,
    current: Rc<ObserverData>,
    disposables: Rc<DisposableStore>,
    proxy: Rc<dyn MainThreadTestingShape>,
    slot: Rc<RefCell<Option<Rc<ObserverData>>>>,
    disposed: Cell<bool>,
}

impl TestObserver {
    /// `onDidChangeTest`.
    pub fn on_did_change_test(&self) -> Event<TestsChangeEvent> {
        self.on_did_change_test.clone()
    }

    /// `tests`: os itens raiz.
    pub fn tests(&self) -> Vec<TestItemRef> {
        self.current.tests.root_tests()
    }
}

impl IDisposable for TestObserver {
    /// `createSingleCallFunction`: só a primeira chamada tem efeito.
    fn dispose(&self) {
        if self.disposed.replace(true) {
            return;
        }

        self.disposables.dispose();
        self.current.observers.set(self.current.observers.get() - 1);
        if self.current.observers.get() == 0 {
            self.proxy.rpc_unsubscribe_from_diffs();
            *self.slot.borrow_mut() = None;
        }
    }
}

/// `TestObservers`.
pub struct TestObservers {
    proxy: Rc<dyn MainThreadTestingShape>,
    current: Rc<RefCell<Option<Rc<ObserverData>>>>,
}

impl TestObservers {
    pub fn new(proxy: Rc<dyn MainThreadTestingShape>) -> TestObservers {
        TestObservers { proxy, current: Rc::new(RefCell::new(None)) }
    }

    pub fn checkout(&self) -> Rc<TestObserver> {
        let existing = self.current.borrow().clone();
        let current = match existing {
            Some(current) => current,
            None => {
                let current = self.create_observer_data();
                *self.current.borrow_mut() = Some(current.clone());
                current
            }
        };

        current.observers.set(current.observers.get() + 1);
        let disposables = DisposableStore::new();
        let relay = disposables.add_rc(Relay::<TestsChangeEvent>::new());
        relay.set_input(current.tests.on_did_change_tests());

        Rc::new(TestObserver {
            on_did_change_test: relay.event(),
            current,
            disposables,
            proxy: self.proxy.clone(),
            slot: self.current.clone(),
            disposed: Cell::new(false),
        })
    }

    /// Applies test diffs to the current set of observed tests.
    pub fn apply_diff(&self, diff: &TestsDiff) {
        let current = self.current.borrow().clone();
        if let Some(current) = current {
            current.tests.inner.apply(diff);
        }
    }

    fn create_observer_data(&self) -> Rc<ObserverData> {
        let tests = MirroredTestCollection::new();
        self.proxy.rpc_subscribe_to_diffs();
        Rc::new(ObserverData { observers: Cell::new(0), tests })
    }
}

/// `TestRunProfileImpl`.
pub struct TestRunProfileImpl {
    pub base: TestRunProfileBase,
    proxy: Rc<dyn MainThreadTestingShape>,
    active_profiles: ActiveProfiles,
    on_did_change_default_profiles: Event<DefaultProfileChangeEvent>,
    initial_publish: RefCell<Option<ITestRunProfile>>,
    profiles: RefCell<Option<ProfileMap>>,
    label: RefCell<String>,
    tag: RefCell<Option<api::TestTag>>,
    supports_continuous_run: Cell<bool>,
    configure_handler: RefCell<Option<Rc<dyn Fn()>>>,
    pub run_handler: RefCell<RunHandler>,
    load_detailed_coverage: RefCell<Option<LoadDetailedCoverage>>,
    load_detailed_coverage_for_test: RefCell<Option<LoadDetailedCoverageForTest>>,
}

/// `Object.assign(initial, update)`.
fn apply_partial_profile(initial: &mut ITestRunProfile, update: &PartialTestRunProfile) {
    if let Some(controller_id) = &update.controller_id {
        initial.controller_id = controller_id.clone();
    }
    if let Some(profile_id) = update.profile_id {
        initial.profile_id = profile_id;
    }
    if let Some(label) = &update.label {
        initial.label = label.clone();
    }
    if let Some(group) = update.group {
        initial.group = group;
    }
    if let Some(is_default) = update.is_default {
        initial.is_default = is_default;
    }
    if let Some(tag) = &update.tag {
        initial.tag = tag.clone();
    }
    if let Some(has_configuration_handler) = update.has_configuration_handler {
        initial.has_configuration_handler = has_configuration_handler;
    }
    if let Some(supports_continuous_run) = update.supports_continuous_run {
        initial.supports_continuous_run = supports_continuous_run;
    }
}

impl TestRunProfileImpl {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        proxy: Rc<dyn MainThreadTestingShape>,
        profiles: ProfileMap,
        active_profiles: ActiveProfiles,
        on_did_change_active_profiles: Event<DefaultProfileChangeEvent>,
        controller_id: &str,
        profile_id: u32,
        label: &str,
        kind: types::TestRunProfileKind,
        run_handler: RunHandler,
        is_default: bool,
        tag: Option<api::TestTag>,
        supports_continuous_run: bool,
    ) -> Rc<TestRunProfileImpl> {
        let group_bitset = convert::TestRunProfileKind::from(kind);
        if is_default {
            active_profiles.borrow_mut().insert(profile_id);
        }

        let initial_publish = ITestRunProfile {
            profile_id,
            controller_id: controller_id.to_string(),
            tag: tag.as_ref().map(|tag| convert::TestTag::namespace(controller_id, &tag.id)),
            label: label.to_string(),
            group: group_bitset,
            is_default,
            has_configuration_handler: false,
            supports_continuous_run,
        };

        let profile = Rc::new(TestRunProfileImpl {
            base: TestRunProfileBase::new(controller_id, profile_id, kind),
            proxy,
            active_profiles,
            on_did_change_default_profiles: on_did_change_active_profiles,
            initial_publish: RefCell::new(Some(initial_publish)),
            profiles: RefCell::new(Some(profiles.clone())),
            label: RefCell::new(label.to_string()),
            tag: RefCell::new(tag),
            supports_continuous_run: Cell::new(supports_continuous_run),
            configure_handler: RefCell::new(None),
            run_handler: RefCell::new(run_handler),
            load_detailed_coverage: RefCell::new(None),
            load_detailed_coverage_for_test: RefCell::new(None),
        });
        profiles.borrow_mut().insert(profile_id, profile.clone());

        // we send the initial profile publish out on the next microtask so that
        // initially setting the isDefault value doesn't overwrite a user-configured value
        let published = profile.clone();
        runtime::queue_microtask(move || {
            let initial = published.initial_publish.borrow_mut().take();
            if let Some(initial) = initial {
                published.proxy.rpc_publish_test_run_profile(initial);
            }
        });

        profile
    }

    /// `updateProfile(impl, proxy, initial, update)`.
    fn update_profile(&self, update: PartialTestRunProfile) {
        if let Some(initial) = self.initial_publish.borrow_mut().as_mut() {
            apply_partial_profile(initial, &update);
        } else {
            self.proxy.rpc_update_test_run_config(&self.base.controller_id, self.base.profile_id, update);
        }
    }

    pub fn controller_id(&self) -> &str {
        &self.base.controller_id
    }

    pub fn profile_id(&self) -> u32 {
        self.base.profile_id
    }

    pub fn kind(&self) -> types::TestRunProfileKind {
        self.base.kind
    }

    pub fn label(&self) -> String {
        self.label.borrow().clone()
    }

    pub fn set_label(&self, label: &str) {
        if *self.label.borrow() != label {
            *self.label.borrow_mut() = label.to_string();
            self.update_profile(PartialTestRunProfile { label: Some(label.to_string()), ..PartialTestRunProfile::default() });
        }
    }

    pub fn supports_continuous_run(&self) -> bool {
        self.supports_continuous_run.get()
    }

    pub fn set_supports_continuous_run(&self, supports: bool) {
        if supports != self.supports_continuous_run.get() {
            self.supports_continuous_run.set(supports);
            self.update_profile(PartialTestRunProfile { supports_continuous_run: Some(supports), ..PartialTestRunProfile::default() });
        }
    }

    pub fn is_default(&self) -> bool {
        self.active_profiles.borrow().contains(&self.base.profile_id)
    }

    pub fn set_is_default(&self, is_default: bool) {
        if is_default != self.is_default() {
            // activeProfiles is synced from the main thread, so we can make
            // provisional changes here that will get confirmed momentarily
            if is_default {
                self.active_profiles.borrow_mut().insert(self.base.profile_id);
            } else {
                self.active_profiles.borrow_mut().shift_remove(&self.base.profile_id);
            }

            self.update_profile(PartialTestRunProfile { is_default: Some(is_default), ..PartialTestRunProfile::default() });
        }
    }

    pub fn tag(&self) -> Option<api::TestTag> {
        self.tag.borrow().clone()
    }

    pub fn set_tag(&self, tag: Option<api::TestTag>) {
        if tag.as_ref().map(|tag| &tag.id) != self.tag.borrow().as_ref().map(|tag| &tag.id) {
            let namespaced = tag.as_ref().map(|tag| convert::TestTag::namespace(&self.base.controller_id, &tag.id));
            *self.tag.borrow_mut() = tag;
            self.update_profile(PartialTestRunProfile { tag: Some(namespaced), ..PartialTestRunProfile::default() });
        }
    }

    pub fn configure_handler(&self) -> Option<Rc<dyn Fn()>> {
        self.configure_handler.borrow().clone()
    }

    pub fn set_configure_handler(&self, handler: Option<Rc<dyn Fn()>>) {
        let same = match (&*self.configure_handler.borrow(), &handler) {
            (None, None) => true,
            (Some(current), Some(next)) => Rc::ptr_eq(current, next),
            _ => false,
        };
        if !same {
            let has_handler = handler.is_some();
            *self.configure_handler.borrow_mut() = handler;
            self.update_profile(PartialTestRunProfile { has_configuration_handler: Some(has_handler), ..PartialTestRunProfile::default() });
        }
    }

    pub fn load_detailed_coverage(&self) -> Option<LoadDetailedCoverage> {
        self.load_detailed_coverage.borrow().clone()
    }

    pub fn set_load_detailed_coverage(&self, handler: Option<LoadDetailedCoverage>) {
        *self.load_detailed_coverage.borrow_mut() = handler;
    }

    pub fn load_detailed_coverage_for_test(&self) -> Option<LoadDetailedCoverageForTest> {
        self.load_detailed_coverage_for_test.borrow().clone()
    }

    pub fn set_load_detailed_coverage_for_test(&self, handler: Option<LoadDetailedCoverageForTest>) {
        *self.load_detailed_coverage_for_test.borrow_mut() = handler;
    }

    /// `onDidChangeDefault`: o novo valor de `isDefault` deste perfil, quando um evento o traz.
    pub fn on_did_change_default(&self) -> Event<bool> {
        let controller_id = self.base.controller_id.clone();
        let profile_id = self.base.profile_id;
        self.on_did_change_default_profiles
            .map(move |event: &DefaultProfileChangeEvent| event.get(&controller_id).and_then(|changes| changes.get(&profile_id)).copied(), None)
            .filter(|value| value.is_some(), None)
            .map(|value: &Option<bool>| value.unwrap_or(false), None)
    }
}

impl IDisposable for TestRunProfileImpl {
    fn dispose(&self) {
        let profiles = self.profiles.borrow_mut().take();
        if let Some(profiles) = profiles
            && profiles.borrow_mut().shift_remove(&self.base.profile_id).is_some()
        {
            self.proxy.rpc_remove_test_profile(&self.base.controller_id, self.base.profile_id);
        }
        *self.initial_publish.borrow_mut() = None;
    }
}

/// `findTestInResultSnapshot`.
fn find_test_in_result_snapshot(ext_id: &TestId, snapshot: &[Rc<TestResultSnapshot>]) -> Option<Rc<TestResultSnapshot>> {
    let path = ext_id.path();
    let mut snapshot: Vec<Rc<TestResultSnapshot>> = snapshot.to_vec();
    for (i, part) in path.iter().enumerate() {
        let item = snapshot.iter().find(|s| s.item.id == *part)?.clone();

        if i == path.len() - 1 {
            return Some(item);
        }

        let children = item.children.borrow().clone();
        snapshot = children;
    }

    None
}
