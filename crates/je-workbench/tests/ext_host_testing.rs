//! Porte de `src/vs/workbench/api/test/browser/extHostTesting.test.ts` (suíte `ExtHost Testing`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `setup` é [`Env::new`]. Os casos
//!   com `await` rodam em `run_with_faked_timers` ([`run_async`]); os demais, em [`run_sync`].
//! - `TestExtHostTestItemCollection.setDiff` é o `set_diff` da coleção (público em Rust), e `single.root`, `.tree` e `.resolveHandler`
//!   são `root()`, `tree` e `set_resolve_handler`. O `await single.expand(...)` é [`Env::expand`] (a coleção devolve `Option<Promise>`
//!   para `Promise<void> | void`) e o `Promise.resolve(single.expand(...))` é [`Env::expansion`].
//! - `sinon.stub(console, 'error')` é o `ErrorSpy` de `je_base::console`: `consoleError.args` é a lista de `(mensagem, erro em texto)`.
//!   O erro lançado entra como texto (`Display`), porque o `console.error` do original recebe o próprio objeto e o `Error` de Rust
//!   não tem identidade comparável; o que se perde é só a distinção entre o mesmo objeto e outro com o mesmo texto.
//! - `new TestItemImpl(...)` devolve `Result` em Rust ([`new_item`] confere). `uri.with({ path: '/different' })` é `uri_file('/different')`
//!   (o mesmo URI de esquema `file`). `[...children]` é `children().entries()`. `item.canResolveChildren = true` é
//!   `set_can_resolve_children` (do trait `ITestItemLike`).
//! - `deepStrictEqual` de objetos literais é `assert_eq!` de tuplas ou da lista de diffs (`TestsDiffOp` tem `PartialEq`); `undefined` e
//!   `null` dos patches são `None` e `Some(None)` (o `PartialTestItem` distingue os dois).
//! - `mockObject<MainThreadTestingShape>()()` (stubs do sinon) é [`MockMainThreadTesting`], que grava os argumentos das chamadas que os
//!   casos conferem (`.args` e `.called`); as demais não fazem nada, como um stub sem comportamento. `AnyCallRPCProtocol` é o
//!   [`TestingContext`], que entrega esse dublê e os de comandos e telemetria do construtor de `ExtHostCommands`.
//! - `vscode.TestRunRequest` literal (`{ include, exclude, profile, preserveFocus }`) é [`TestRunRequest`] com os campos dados, e `{ ...req }` é
//!   uma cópia em outro `Rc` (o `Map` do coordenador é por identidade). O `ext` (`{} as any`) é a `null_extension_description`.
//! - `sinon.useFakeTimers()` e `clock.tick(ms)` são `runtime::with_faked_timers` e `runtime::advance_fake_time`.
//! - `Event.toPromise(profAA.onDidChangeDefault)` é `to_promise(None)`; `new Set()` e `new Map()` do construtor do perfil são `Default`.
//! - O `for (const change of [...])` de "ignores obsolete asynchronous failures" é um laço dentro do `#[test]` (o original registra um
//!   teste por combinação, e o placar conta o literal `test(...)`); cada volta tem o seu `setup`.
//! - `getProfileInternalId` (do `TestExtHostTesting`, que lê o `controllers` protegido) é [`profile_internal_id`], sobre
//!   `ExtHostTesting::controller`.
//! - As formas `Serialized` de `ITestItem` e `ITestMessage` que o `MainThreadTestingShape` carrega são os tipos `*Serialized` do `testTypes`; o
//!   `convert.TestItem.from(x)` esperado vira `ITestItem::serialize(&convert::TestItem::from(x))` ([`serialized`]).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::console::{ConsoleErrorCall, ErrorSpy};
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, throw};
use je_base::event::Event;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_base::runtime;
use je_base::uri::URI;
use je_editor::range::Range as EditorRange;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::NullLogService;
use je_platform::resources::uri_file;
use je_workbench::api::common::ext_host_commands::ExtHostCommands;
use je_workbench::api::common::ext_host_documents_and_editors::ExtHostDocumentsAndEditors;
use je_workbench::api::common::ext_host_protocol::{
    CommandArgs, ExtHostTestingShape, IExtHostTelemetry, IMainContext, ITestControllerPatch, MainThreadCommandsShape, MainThreadTelemetryShape,
    MainThreadTestingShape,
};
use je_workbench::api::common::ext_host_test_item::{ExtHostTestItemCollection, IDocumentVersionSource, TestItemError, TestItemImpl};
use je_workbench::api::common::ext_host_testing::{ExtHostTesting, RunHandler, TestController, TestRunCoordinator, TestRunDto, TestRunProfileImpl, TestRunTracker};
use je_workbench::api::common::ext_host_testing_converters as convert;
use je_workbench::api::common::ext_host_testing_types::{PlainTestItem, TestMessage, TestRunRequest, TestTag};
use je_workbench::api::common::ext_host_type_converters::LocationDto;
use je_workbench::api::common::ext_host_types::{self as types, Documentation, Location, Position, Range};
use je_workbench::contrib::testing::common::test_id::TestId;
use je_workbench::contrib::testing::common::test_item_collection::{ITestChildrenLike, ITestItemChildren, ITestItemLike, ResolveHandler};
use je_workbench::contrib::testing::common::test_types::{
    CoverageDetails, ExtensionRunProfile, ExtensionRunTestsRequest, IFileCoverageSerialized, IRichLocationSerialized, IStartControllerTests, ITestErrorMessageSerialized,
    ITestItem, ITestItemSerialized, ITestItemUpdate, ITestMessageSerialized, ITestRunProfile, ITestRunTask, ITestTagDisplayInfo, InternalTestItem, PartialTestItem,
    PartialTestRunProfile, ResolvedTestRunRequest, TestControllerCapability, TestItemExpandState, TestMessageText, TestResultState, TestsDiff, TestsDiffOp,
    TestsDiffOpSerialized,
};
use je_workbench::services::extensions::extensions::null_extension_description;

const INFINITY: f64 = f64::INFINITY;

// ---- os dublês ----------------------------------------------------------------------------------------

/// `{ getDocument: () => undefined } as Partial<ExtHostDocumentsAndEditors>`.
struct NoDocuments;

impl IDocumentVersionSource for NoDocuments {
    fn get_document_version(&self, _uri: &URI) -> Option<f64> {
        None
    }
}

/// `mockObject<MainThreadTestingShape>()()`: grava as chamadas que os casos conferem.
#[derive(Default)]
struct MockMainThreadTesting {
    started_extension_test_run: RefCell<Vec<ExtensionRunTestsRequest>>,
    finished_extension_test_run: RefCell<Vec<String>>,
    append_output_to_run: RefCell<Vec<AppendOutputCall>>,
    add_tests_to_run: RefCell<Vec<(String, String, Vec<ITestItemSerialized>)>>,
    append_test_messages_in_run: RefCell<Vec<(String, String, String, Vec<ITestMessageSerialized>)>>,
}

/// Os argumentos de `$appendOutputToRun`: `(runId, taskId, output, location, testId)`.
type AppendOutputCall = (String, String, Vec<u8>, Option<LocationDto>, Option<String>);

impl MainThreadTestingShape for MockMainThreadTesting {
    fn rpc_register_test_controller(&self, _controller_id: &str, _label: &str, _capability: TestControllerCapability) {}

    fn rpc_update_controller(&self, _controller_id: &str, _patch: ITestControllerPatch) {}

    fn rpc_unregister_test_controller(&self, _controller_id: &str) {}

    fn rpc_subscribe_to_diffs(&self) {}

    fn rpc_unsubscribe_from_diffs(&self) {}

    fn rpc_publish_diff(&self, _controller_id: &str, _diff: Vec<TestsDiffOpSerialized>) {}

    fn rpc_get_coverage_details(&self, _result_id: &str, _task_index: usize, _uri: &URI, _token: CancellationToken) -> Promise<Vec<CoverageDetails>> {
        Promise::resolved(Vec::new())
    }

    fn rpc_publish_test_run_profile(&self, _config: ITestRunProfile) {}

    fn rpc_update_test_run_config(&self, _controller_id: &str, _config_id: u32, _update: PartialTestRunProfile) {}

    fn rpc_remove_test_profile(&self, _controller_id: &str, _config_id: u32) {}

    fn rpc_run_tests(&self, _req: ResolvedTestRunRequest, _token: CancellationToken) -> Promise<String> {
        Promise::resolved(String::new())
    }

    fn rpc_add_tests_to_run(&self, controller_id: &str, run_id: &str, tests: Vec<ITestItemSerialized>) {
        self.add_tests_to_run.borrow_mut().push((controller_id.to_string(), run_id.to_string(), tests));
    }

    fn rpc_update_test_state_in_run(&self, _run_id: &str, _task_id: &str, _test_id: &str, _state: TestResultState, _duration: Option<f64>) {}

    fn rpc_append_test_messages_in_run(&self, run_id: &str, task_id: &str, test_id: &str, messages: Vec<ITestMessageSerialized>) {
        self.append_test_messages_in_run.borrow_mut().push((run_id.to_string(), task_id.to_string(), test_id.to_string(), messages));
    }

    fn rpc_append_output_to_run(&self, run_id: &str, task_id: &str, output: VSBuffer, location: Option<LocationDto>, test_id: Option<String>) {
        self.append_output_to_run.borrow_mut().push((run_id.to_string(), task_id.to_string(), output.to_vec(), location, test_id));
    }

    fn rpc_append_coverage(&self, _run_id: &str, _task_id: &str, _coverage: IFileCoverageSerialized) {}

    fn rpc_started_test_run_task(&self, _run_id: &str, _task: ITestRunTask) {}

    fn rpc_finished_test_run_task(&self, _run_id: &str, _task_id: &str) {}

    fn rpc_started_extension_test_run(&self, req: ExtensionRunTestsRequest) {
        self.started_extension_test_run.borrow_mut().push(req);
    }

    fn rpc_finished_extension_test_run(&self, run_id: &str) {
        self.finished_extension_test_run.borrow_mut().push(run_id.to_string());
    }

    fn rpc_mark_test_retired(&self, _test_ids: Option<Vec<String>>) {}
}

/// O `MainThreadCommands` que o construtor de `ExtHostCommands` pede e que nenhum caso usa.
struct MockMainThreadCommands;

impl IDisposable for MockMainThreadCommands {
    fn dispose(&self) {}
}

impl MainThreadCommandsShape for MockMainThreadCommands {
    fn rpc_register_command(&self, _id: &str) {}

    fn rpc_unregister_command(&self, _id: &str) {}

    fn rpc_fire_command_activation_event(&self, _id: &str) {}

    fn rpc_execute_command(&self, _id: &str, _args: CommandArgs, _retry: bool) -> Promise<Dynamic> {
        panic!("$executeCommand is not a function")
    }

    fn rpc_get_commands(&self) -> Promise<Vec<String>> {
        panic!("$getCommands is not a function")
    }
}

/// O `MainThreadTelemetry` que o construtor de `ExtHostCommands` pede e que nenhum caso usa.
struct MockMainThreadTelemetry;

impl IDisposable for MockMainThreadTelemetry {
    fn dispose(&self) {}
}

impl MainThreadTelemetryShape for MockMainThreadTelemetry {
    fn rpc_public_log(&self, _event_name: &str, _data: Option<Dynamic>) {}

    fn rpc_public_log2(&self, _event_name: &str, _data: Option<Dynamic>) {}
}

/// `new class extends mock<IExtHostTelemetry>() { override onExtensionError(): boolean { return true; } }`.
struct MockExtHostTelemetry;

impl IExtHostTelemetry for MockExtHostTelemetry {
    fn on_extension_error(&self, _extension: &ExtensionIdentifier, _error: &Error) -> bool {
        true
    }
}

/// `AnyCallRPCProtocol()`: entrega o dublê de cada proxy que o serviço e o `ExtHostCommands` pedem.
struct TestingContext {
    testing: Rc<dyn MainThreadTestingShape>,
    commands: Rc<dyn MainThreadCommandsShape>,
    telemetry: Rc<dyn MainThreadTelemetryShape>,
}

impl IMainContext for TestingContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        match sid {
            "MainThreadTesting" => Rc::new(self.testing.clone()),
            "MainThreadCommands" => Rc::new(self.commands.clone()),
            "MainThreadTelemetry" => Rc::new(self.telemetry.clone()),
            other => panic!("proxy inesperado: {other}"),
        }
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

// ---- auxiliares ---------------------------------------------------------------------------------------

/// `new TestId(parts).toString()`.
fn tid(parts: &[&str]) -> String {
    TestId::new(parts.iter().map(|part| part.to_string()).collect()).to_string()
}

/// `new TestItemImpl('ctrlId', id, label, uri)`.
fn new_item(id: &str, label: &str, uri: Option<URI>) -> TestItemImpl {
    TestItemImpl::new("ctrlId", id, label, uri).expect("o ID do item é válido")
}

/// `convert.TestItem.from(item)`.
fn from_item(item: &TestItemImpl) -> ITestItem {
    convert::TestItem::from(item)
}

/// `convert.TestItem.from(item)` na forma que o `MainThreadTestingShape` carrega.
fn serialized(item: &TestItemImpl) -> ITestItemSerialized {
    ITestItem::serialize(&from_item(item))
}

fn add_op(expand: TestItemExpandState, item: ITestItem) -> TestsDiffOp {
    TestsDiffOp::Add { item: InternalTestItem { controller_id: "ctrlId".to_string(), expand, item } }
}

fn update_op(ext_id: &str, expand: Option<TestItemExpandState>, item: Option<PartialTestItem>) -> TestsDiffOp {
    TestsDiffOp::Update { item: ITestItemUpdate { ext_id: ext_id.to_string(), expand, item } }
}

fn label_patch(label: &str) -> PartialTestItem {
    PartialTestItem { label: Some(label.to_string()), ..PartialTestItem::default() }
}

/// `{ error: '...' }` (um texto) ou `{ error: null }`.
fn error_patch(error: Option<&str>) -> PartialTestItem {
    PartialTestItem { error: Some(error.map(|text| TestMessageText::Text(text.to_string()))), ..PartialTestItem::default() }
}

fn document_synced(uri: &URI) -> TestsDiffOp {
    TestsDiffOp::DocumentSynced { uri: uri.clone(), docv: None }
}

/// O `console.error` do tratador de descoberta de um controlador.
fn resolve_handler_log(error: &Error) -> Vec<ConsoleErrorCall> {
    vec![ConsoleErrorCall { message: "Unhandled error in resolveHandler of test controller \"ctrlId\"".to_string(), detail: error.to_string() }]
}

/// `() => { }` como `runHandler`.
fn noop_run_handler() -> RunHandler {
    Rc::new(|_: &Rc<TestRunRequest>, _: &CancellationToken| MaybePromise::Value(()))
}

/// `(item) => resolution.p` e afins: um `resolveHandler` que devolve a promessa dada.
fn pending_handler(promise: Promise<()>) -> ResolveHandler<TestItemImpl> {
    Rc::new(move |_: Option<&TestItemImpl>| MaybePromise::Promise(promise.clone()))
}

/// `() => { throw new Error('Discovery failed') }`.
fn throwing_handler(error: Error) -> ResolveHandler<TestItemImpl> {
    Rc::new(move |_: Option<&TestItemImpl>| -> MaybePromise<()> { throw(error.clone()) })
}

// ---- o setup ------------------------------------------------------------------------------------------

struct Env {
    single: Rc<ExtHostTestItemCollection>,
    resolve_calls: Rc<RefCell<Vec<Option<String>>>>,
    store: Rc<DisposableStore>,
}

impl Env {
    /// O `setup` da suíte `ExtHost Testing`.
    fn new(guard: &LeakGuard) -> Env {
        let resolve_calls: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
        let single = guard.add_rc(ExtHostTestItemCollection::new("ctrlId", "root", Rc::new(NoDocuments)));
        let root = single.root().clone();
        let calls = resolve_calls.clone();
        single.set_resolve_handler(Some(Rc::new(move |item: Option<&TestItemImpl>| {
            calls.borrow_mut().push(item.map(|item| item.id()));
            match item {
                None => {
                    let a = new_item("id-a", "a", Some(uri_file("/")));
                    a.set_can_resolve_children(true);
                    let b = new_item("id-b", "b", Some(uri_file("/")));
                    root.children().add(a);
                    root.children().add(b);
                }
                Some(item) if item.id() == "id-a" => {
                    item.children().add(new_item("id-aa", "aa", Some(uri_file("/"))));
                    item.children().add(new_item("id-ab", "ab", Some(uri_file("/"))));
                }
                Some(_) => {}
            }
            MaybePromise::Value(())
        })));

        let target = single.clone();
        guard.add(single.on_did_generate_diff().listen(move |diff: &TestsDiff| target.set_diff(diff.clone() /* don't clear during testing */)));

        Env { single, resolve_calls, store: guard.store() }
    }

    fn root(&self) -> TestItemImpl {
        self.single.root().clone()
    }

    fn root_id(&self) -> String {
        self.single.root().id()
    }

    /// `single.root.children.get(id)!`.
    fn child(&self, id: &str) -> TestItemImpl {
        self.root().children().get(id).expect("o filho existe")
    }

    /// `Promise.resolve(single.expand(id, levels))`.
    fn expansion(&self, id: &str, levels: f64) -> Promise<()> {
        self.single.expand(id, levels).unwrap_or_else(|| Promise::resolved(()))
    }

    /// `await single.expand(id, levels)`.
    async fn expand(&self, id: &str, levels: f64) {
        self.expansion(id, levels).await.expect("a expansão termina");
    }

    /// `single.tree.get(id)!.expand`.
    fn expand_state(&self, id: &str) -> TestItemExpandState {
        self.single.tree.borrow().get(id).expect("o item está na árvore").borrow().expand
    }

    /// `single.tree.get(id)!.resolveBarrier!.isOpen()`.
    fn barrier_open(&self, id: &str) -> bool {
        self.single.tree.borrow().get(id).expect("o item está na árvore").borrow().resolve_barrier.as_ref().expect("há barreira").is_open()
    }

    fn resolve_calls(&self) -> Vec<Option<String>> {
        self.resolve_calls.borrow().clone()
    }
}

/// Um caso sem `await`.
fn run_sync(body: impl FnOnce(&Env)) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&guard);
    body(&env);
}

/// Um caso com `await`.
fn run_async<F, Fut>(body: F)
where
    F: FnOnce(Rc<Env>) -> Fut,
    Fut: Future<Output = ()> + 'static,
{
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Rc::new(Env::new(&guard));
    run_with_faked_timers(body(env));
}

// ---- OwnedTestCollection --------------------------------------------------------------------------------

#[test]
fn adds_a_root_recursively() {
    run_async(|env| async move {
        env.expand(&env.root_id(), INFINITY).await;
        let a = env.child("id-a");
        let b = env.child("id-b");
        assert_eq!(
            env.single.collect_diff(),
            vec![
                add_op(TestItemExpandState::BusyExpanding, from_item(&env.root())),
                add_op(TestItemExpandState::BusyExpanding, from_item(&a)),
                add_op(TestItemExpandState::NotExpandable, from_item(&a.children().get("id-aa").unwrap())),
                add_op(TestItemExpandState::NotExpandable, from_item(&a.children().get("id-ab").unwrap())),
                update_op(&tid(&["ctrlId", "id-a"]), Some(TestItemExpandState::Expanded), None),
                add_op(TestItemExpandState::NotExpandable, from_item(&b)),
                update_op(&env.root_id(), Some(TestItemExpandState::Expanded), None),
            ]
        );
    });
}

#[test]
fn parents_are_set_correctly() {
    run_sync(|env| {
        let _ = env.single.expand(&env.root_id(), INFINITY);
        env.single.collect_diff();

        let a = env.child("id-a");
        let ab = a.children().get("id-ab").unwrap();
        assert!(a.parent().is_none());
        assert!(ab.parent().expect("ab tem pai").is_same(&a));
    });
}

#[test]
fn can_add_an_item_with_same_id_as_root() {
    run_sync(|env| {
        env.single.collect_diff();

        let child = new_item("ctrlId", "c", None);
        env.root().children().add(child.clone());
        assert_eq!(env.single.collect_diff(), vec![add_op(TestItemExpandState::NotExpandable, from_item(&child))]);
    });
}

#[test]
fn no_ops_if_items_not_changed() {
    run_sync(|env| {
        env.single.collect_diff();
        assert_eq!(env.single.collect_diff(), Vec::<TestsDiffOp>::new());
    });
}

#[test]
fn watches_property_mutations() {
    run_sync(|env| {
        let _ = env.single.expand(&env.root_id(), INFINITY);
        env.single.collect_diff();
        env.child("id-a").set_description(Some("Hello world")); /* item a */

        assert_eq!(
            env.single.collect_diff(),
            vec![update_op(
                &tid(&["ctrlId", "id-a"]),
                None,
                Some(PartialTestItem { description: Some(Some("Hello world".to_string())), ..PartialTestItem::default() })
            )]
        );
    });
}

#[test]
fn removes_children() {
    run_sync(|env| {
        let _ = env.single.expand(&env.root_id(), INFINITY);
        env.single.collect_diff();
        env.root().children().delete("id-a");

        assert_eq!(env.single.collect_diff(), vec![TestsDiffOp::Remove { item_id: tid(&["ctrlId", "id-a"]) }]);
        let mut keys: Vec<String> = env.single.tree.borrow().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, vec![env.root_id(), tid(&["ctrlId", "id-b"])]);
        assert_eq!(env.single.tree.borrow().len(), 2);
    });
}

#[test]
fn adds_new_children() {
    run_sync(|env| {
        let _ = env.single.expand(&env.root_id(), INFINITY);
        env.single.collect_diff();
        let child = new_item("id-ac", "c", None);
        env.child("id-a").children().add(child.clone());

        assert_eq!(env.single.collect_diff(), vec![add_op(TestItemExpandState::NotExpandable, from_item(&child))]);
        let mut ids: Vec<String> = env.single.tree.borrow().values().map(|n| n.borrow().actual.id()).collect();
        ids.sort();
        assert_eq!(ids, vec![env.root_id(), "id-a".to_string(), "id-aa".to_string(), "id-ab".to_string(), "id-ac".to_string(), "id-b".to_string()]);
        assert_eq!(env.single.tree.borrow().len(), 6);
    });
}

#[test]
fn manages_tags_correctly() {
    run_sync(|env| {
        let _ = env.single.expand(&env.root_id(), INFINITY);
        env.single.collect_diff();
        let tag1 = TestTag::new("tag1");
        let tag2 = TestTag::new("tag2");
        let tag3 = TestTag::new("tag3");
        let child = new_item("id-ac", "c", None);
        child.set_tags(vec![tag1, tag2.clone()]);
        env.child("id-a").children().add(child.clone());

        let add_tag = |id: &str| TestsDiffOp::AddTag { tag: ITestTagDisplayInfo { id: id.to_string() } };
        assert_eq!(
            env.single.collect_diff(),
            vec![add_tag("ctrlId\0tag1"), add_tag("ctrlId\0tag2"), add_op(TestItemExpandState::NotExpandable, from_item(&child))]
        );

        child.set_tags(vec![tag2.clone(), tag3]);
        assert_eq!(
            env.single.collect_diff(),
            vec![
                add_tag("ctrlId\0tag3"),
                update_op(
                    &tid(&["ctrlId", "id-a", "id-ac"]),
                    None,
                    Some(PartialTestItem { tags: Some(vec!["ctrlId\0tag2".to_string(), "ctrlId\0tag3".to_string()]), ..PartialTestItem::default() })
                ),
                TestsDiffOp::RemoveTag { id: "ctrlId\0tag1".to_string() },
            ]
        );

        let a = env.child("id-a");
        a.set_tags(vec![tag2]);
        a.children().replace(&[]);
        let removed_tags: Vec<TestsDiffOp> = env.single.collect_diff().into_iter().filter(|op| matches!(op, TestsDiffOp::RemoveTag { .. })).collect();
        assert_eq!(removed_tags, vec![TestsDiffOp::RemoveTag { id: "ctrlId\0tag3".to_string() }]);
    });
}

#[test]
fn replaces_on_uri_change() {
    run_sync(|env| {
        let _ = env.single.expand(&env.root_id(), INFINITY);
        env.single.collect_diff();

        let old_a = env.child("id-a");
        let uri = uri_file("/different");
        let new_a = new_item("id-a", "Hello world", Some(uri));
        let old_children: Vec<TestItemImpl> = old_a.children().entries().into_iter().map(|(_, item)| item).collect();
        new_a.children().replace(&old_children);
        let replaced: Vec<TestItemImpl> = env.root().children().entries().into_iter().map(|(id, i)| if id == "id-a" { new_a.clone() } else { i }).collect();
        env.root().children().replace(&replaced);

        assert_eq!(
            env.single.collect_diff(),
            vec![
                TestsDiffOp::Remove { item_id: tid(&["ctrlId", "id-a"]) },
                add_op(TestItemExpandState::NotExpandable, from_item(&new_a)),
                add_op(TestItemExpandState::NotExpandable, from_item(&new_a.children().get("id-aa").unwrap())),
                add_op(TestItemExpandState::NotExpandable, from_item(&new_a.children().get("id-ab").unwrap())),
            ]
        );
    });
}

#[test]
fn treats_in_place_replacement_as_mutation() {
    run_sync(|env| {
        let _ = env.single.expand(&env.root_id(), INFINITY);
        env.single.collect_diff();

        let old_a = env.child("id-a");
        let uri = old_a.uri().expect("a tem URI");
        let new_a = new_item("id-a", "Hello world", Some(uri.clone()));
        let old_children: Vec<TestItemImpl> = old_a.children().entries().into_iter().map(|(_, item)| item).collect();
        new_a.children().replace(&old_children);
        env.root().children().replace(&[new_a.clone(), new_item("id-b", &env.child("id-b").label(), Some(uri.clone()))]);

        assert_eq!(
            env.single.collect_diff(),
            vec![update_op(&tid(&["ctrlId", "id-a"]), None, Some(label_patch("Hello world"))), document_synced(&uri)]
        );

        new_a.set_label("still connected");
        assert_eq!(env.single.collect_diff(), vec![update_op(&tid(&["ctrlId", "id-a"]), None, Some(label_patch("still connected")))]);

        old_a.set_label("no longer connected");
        assert_eq!(env.single.collect_diff(), Vec::<TestsDiffOp>::new());
    });
}

// ---- discovery errors -----------------------------------------------------------------------------------

#[test]
fn publishes_synchronous_failures_without_mutating_the_provider_error() {
    run_async(|env| async move {
        let console_error = ErrorSpy::install();
        let error = Error::new("Discovery failed");
        env.single.set_resolve_handler(Some(throwing_handler(error.clone())));
        env.single.collect_diff();

        env.expand(&env.root_id(), 0.0).await;

        assert_eq!(
            (env.single.collect_diff(), env.root().error(), env.barrier_open(&env.root_id()), console_error.calls()),
            (
                vec![update_op(&env.root_id(), Some(TestItemExpandState::Expanded), Some(error_patch(Some("Discovery failed"))))],
                None,
                true,
                resolve_handler_log(&error),
            )
        );
    });
}

#[test]
fn publishes_asynchronous_failures_and_completes_expansion() {
    run_async(|env| async move {
        let console_error = ErrorSpy::install();
        let resolution = DeferredPromise::<()>::new();
        env.single.set_resolve_handler(Some(pending_handler(resolution.promise.clone())));
        env.single.collect_diff();

        let expansion = env.expansion(&env.root_id(), 0.0);
        let pending_diff = env.single.collect_diff();
        let error = Error::new("Async discovery failed");
        resolution.error(error.clone()).await.unwrap();
        expansion.await.unwrap();

        assert_eq!(
            (pending_diff, env.single.collect_diff(), env.barrier_open(&env.root_id()), console_error.calls()),
            (
                vec![update_op(&env.root_id(), Some(TestItemExpandState::BusyExpanding), None)],
                vec![update_op(&env.root_id(), Some(TestItemExpandState::Expanded), Some(error_patch(Some("Async discovery failed"))))],
                true,
                resolve_handler_log(&error),
            )
        );
    });
}

#[test]
fn waits_for_concurrent_expansions_of_a_resolving_item() {
    run_async(|env| async move {
        let resolution = DeferredPromise::<()>::new();
        let handler_calls = Rc::new(Cell::new(0usize));
        let pending = resolution.promise.clone();
        let calls = handler_calls.clone();
        env.single.set_resolve_handler(Some(Rc::new(move |_: Option<&TestItemImpl>| {
            calls.set(calls.get() + 1);
            MaybePromise::Promise(pending.clone())
        })));

        let expansion = env.expansion(&env.root_id(), 0.0);
        let concurrent_finished = Rc::new(Cell::new(false));
        let finished = concurrent_finished.clone();
        let concurrent_expansion = env.expansion(&env.root_id(), 0.0).then(move |_| finished.set(true));
        Promise::resolved(()).await.unwrap();
        let finished_before_resolution = concurrent_finished.get();

        resolution.complete(()).await.unwrap();
        Promise::all(vec![expansion, concurrent_expansion]).await.unwrap();

        assert_eq!(
            (finished_before_resolution, concurrent_finished.get(), handler_calls.get(), env.expand_state(&env.root_id())),
            (false, true, 1, TestItemExpandState::Expanded)
        );
    });
}

#[test]
fn keeps_rediscovery_pending_when_an_obsolete_handler_rejects() {
    run_async(|env| async move {
        let console_error = ErrorSpy::install();
        let old_resolution = DeferredPromise::<()>::new();
        env.single.set_resolve_handler(Some(pending_handler(old_resolution.promise.clone())));
        let old_expansion = env.expansion(&env.root_id(), 0.0);

        let resolution = DeferredPromise::<()>::new();
        env.single.set_resolve_handler(Some(pending_handler(resolution.promise.clone())));
        let finished = Rc::new(Cell::new(false));
        let finished_flag = finished.clone();
        let expansion = env.expansion(&env.root_id(), 0.0).then(move |_| finished_flag.set(true));
        env.single.collect_diff();

        let obsolete = Error::new("Obsolete discovery failure");
        old_resolution.error(obsolete.clone()).await.unwrap();
        old_expansion.await.unwrap();
        let after_old_completion = (finished.get(), env.expand_state(&env.root_id()), env.barrier_open(&env.root_id()), env.single.collect_diff());

        resolution.complete(()).await.unwrap();
        expansion.await.unwrap();

        assert_eq!(
            (after_old_completion, finished.get(), env.single.collect_diff(), console_error.calls()),
            (
                (false, TestItemExpandState::BusyExpanding, false, Vec::<TestsDiffOp>::new()),
                true,
                vec![update_op(&env.root_id(), Some(TestItemExpandState::Expanded), None)],
                resolve_handler_log(&obsolete),
            )
        );
    });
}

#[test]
fn clears_failures_when_resetting_the_handler_and_rediscovering_successfully() {
    run_async(|env| async move {
        let _console_error = ErrorSpy::install();
        let fail = Rc::new(Cell::new(true));
        let should_fail = fail.clone();
        let handler: ResolveHandler<TestItemImpl> = Rc::new(move |_: Option<&TestItemImpl>| {
            if should_fail.get() {
                throw(Error::new("Discovery failed"));
            }
            MaybePromise::Value(())
        });
        env.single.set_resolve_handler(Some(handler.clone()));
        env.expand(&env.root_id(), 0.0).await;
        env.single.collect_diff();

        fail.set(false);
        env.single.set_resolve_handler(Some(handler));
        env.expand(&env.root_id(), 0.0).await;

        assert_eq!(env.single.collect_diff(), vec![update_op(&env.root_id(), Some(TestItemExpandState::Expanded), Some(error_patch(None)))]);
    });
}

#[test]
fn clears_failures_when_removing_the_handler() {
    run_async(|env| async move {
        let _console_error = ErrorSpy::install();
        env.single.set_resolve_handler(Some(throwing_handler(Error::new("Discovery failed"))));
        env.expand(&env.root_id(), 0.0).await;
        env.single.collect_diff();

        env.single.set_resolve_handler(None);

        assert_eq!(env.single.collect_diff(), vec![update_op(&env.root_id(), Some(TestItemExpandState::NotExpandable), Some(error_patch(None)))]);
    });
}

#[test]
fn preserves_provider_errors_on_failure_and_successful_rediscovery() {
    run_async(|env| async move {
        let _console_error = ErrorSpy::install();
        let provider_error = Rc::new(types::MarkdownString::new(Some("Provider error"), false));
        env.root().set_error(Some(TestItemError::Markdown(provider_error.clone())));
        env.single.set_resolve_handler(Some(throwing_handler(Error::new("Discovery failed"))));
        env.single.collect_diff();
        env.expand(&env.root_id(), 0.0).await;
        let failed_diff = env.single.collect_diff();

        env.single.set_resolve_handler(Some(Rc::new(|_: Option<&TestItemImpl>| MaybePromise::Promise(Promise::resolved(())))));
        env.expand(&env.root_id(), 0.0).await;

        assert_eq!(
            (failed_diff, env.single.collect_diff(), env.root().error()),
            (
                vec![update_op(&env.root_id(), Some(TestItemExpandState::Expanded), None)],
                vec![update_op(&env.root_id(), Some(TestItemExpandState::Expanded), None)],
                Some(TestItemError::Markdown(provider_error)),
            )
        );
    });
}

#[test]
fn uses_provider_errors_in_preference_to_a_discovery_failure() {
    run_async(|env| async move {
        let _console_error = ErrorSpy::install();
        env.single.set_resolve_handler(Some(throwing_handler(Error::new("Discovery failed"))));
        env.expand(&env.root_id(), 0.0).await;
        env.single.collect_diff();

        let root = env.root();
        root.set_error(Some(TestItemError::Text("Provider error".to_string())));
        let provider_diff = env.single.collect_diff();
        root.set_error(None);

        assert_eq!(
            (provider_diff, env.single.collect_diff()),
            (
                vec![update_op(&env.root_id(), None, Some(error_patch(Some("Provider error"))))],
                vec![update_op(&env.root_id(), None, Some(error_patch(Some("Discovery failed"))))],
            )
        );
    });
}

#[test]
fn clears_failures_when_replacing_an_item() {
    run_async(|env| async move {
        let _console_error = ErrorSpy::install();
        env.expand(&env.root_id(), 0.0).await;
        let item = env.child("id-a");
        let id = TestId::from_ext_host_test_item(&item, "ctrlId", None).to_string();
        let failing = item.clone();
        env.single.set_resolve_handler(Some(Rc::new(move |resolved: Option<&TestItemImpl>| {
            if resolved.is_some_and(|resolved| resolved.is_same(&failing)) {
                throw(Error::new("Discovery failed"));
            }
            MaybePromise::Value(())
        })));
        env.expand(&id, 0.0).await;
        env.single.collect_diff();

        let replacement = new_item(&item.id(), &item.label(), item.uri());
        replacement.set_can_resolve_children(true);
        env.root().children().add(replacement);
        env.expand(&id, 0.0).await;

        assert_eq!(
            env.single.collect_diff(),
            vec![update_op(&id, Some(TestItemExpandState::Expanded), Some(error_patch(None))), document_synced(&item.uri().unwrap())]
        );
    });
}

#[test]
fn ignores_obsolete_asynchronous_failures_after_change() {
    for change in ["remove", "replace", "dispose", "reset"] {
        run_async(move |env| async move {
            let console_error = ErrorSpy::install();
            env.expand(&env.root_id(), 0.0).await;
            let item = env.child("id-a");
            let id = TestId::from_ext_host_test_item(&item, "ctrlId", None).to_string();
            let resolution = DeferredPromise::<()>::new();
            env.single.set_resolve_handler(Some(pending_handler(resolution.promise.clone())));
            let expansion = env.expansion(&id, 0.0);

            match change {
                "remove" => env.root().children().delete(&item.id()),
                "replace" => env.root().children().add(new_item(&item.id(), &item.label(), item.uri())),
                "dispose" => env.single.dispose(),
                "reset" => env.single.set_resolve_handler(Some(Rc::new(|_: Option<&TestItemImpl>| MaybePromise::Value(())))),
                other => panic!("mudança desconhecida: {other}"),
            }
            env.single.collect_diff();
            let obsolete = Error::new("Obsolete discovery failure");
            resolution.error(obsolete.clone()).await.unwrap();
            expansion.await.unwrap();

            assert_eq!((env.single.collect_diff(), console_error.calls()), (Vec::<TestsDiffOp>::new(), resolve_handler_log(&obsolete)), "após {change}");
        });
    }
}

// ---- expandibility restoration --------------------------------------------------------------------------

/// `doReplace(canResolveChildren = true)`.
async fn do_replace(env: &Env, can_resolve_children: bool) {
    let uri = env.child("id-a").uri();
    let new_a = new_item("id-a", "Hello world", uri.clone());
    new_a.set_can_resolve_children(can_resolve_children);
    env.root().children().replace(&[new_a, new_item("id-b", &env.child("id-b").label(), uri)]);
    common::timeout(0).await.unwrap(); // drain microtasks
}

#[test]
fn does_not_restore_an_unexpanded_state() {
    run_async(|env| async move {
        env.expand(&env.root_id(), 0.0).await;
        assert_eq!(env.resolve_calls(), vec![None]);
        do_replace(&env, true).await;
        assert_eq!(env.resolve_calls(), vec![None]);
    });
}

#[test]
fn restores_resolve_state_on_replacement() {
    run_async(|env| async move {
        env.expand(&env.root_id(), INFINITY).await;
        assert_eq!(env.resolve_calls(), vec![None, Some("id-a".to_string())]);
        do_replace(&env, true).await;
        assert_eq!(env.resolve_calls(), vec![None, Some("id-a".to_string()), Some("id-a".to_string())]);
    });
}

#[test]
fn does_not_expand_if_new_child_is_not_expandable() {
    run_async(|env| async move {
        env.expand(&env.root_id(), INFINITY).await;
        assert_eq!(env.resolve_calls(), vec![None, Some("id-a".to_string())]);
        do_replace(&env, false).await;
        assert_eq!(env.resolve_calls(), vec![None, Some("id-a".to_string())]);
    });
}

// ---- o restante da suíte OwnedTestCollection ------------------------------------------------------------

#[test]
fn treats_in_place_replacement_as_mutation_deeply() {
    run_sync(|env| {
        let _ = env.single.expand(&env.root_id(), INFINITY);
        env.single.collect_diff();

        let old_a = env.child("id-a");
        let uri = old_a.uri().expect("a tem URI");
        let new_a = new_item("id-a", &env.child("id-a").label(), Some(uri.clone()));
        let old_aa = old_a.children().get("id-aa").unwrap();
        let old_ab = old_a.children().get("id-ab").unwrap();
        let new_ab = new_item("id-ab", "Hello world", Some(uri.clone()));
        new_a.children().replace(&[old_aa.clone(), new_ab.clone()]);
        env.root().children().replace(&[new_a.clone(), env.child("id-b")]);

        assert_eq!(
            env.single.collect_diff(),
            vec![
                update_op(&TestId::from_ext_host_test_item(&old_ab, "ctrlId", None).to_string(), None, Some(label_patch("Hello world"))),
                document_synced(&uri),
            ]
        );

        old_aa.set_label("still connected1");
        new_ab.set_label("still connected2");
        old_ab.set_label("not connected3");
        assert_eq!(
            env.single.collect_diff(),
            vec![
                update_op(&tid(&["ctrlId", "id-a", "id-aa"]), None, Some(label_patch("still connected1"))),
                update_op(&tid(&["ctrlId", "id-a", "id-ab"]), None, Some(label_patch("still connected2"))),
            ]
        );

        assert!(new_ab.parent().expect("new_ab tem pai").is_same(&new_a));
        assert!(old_aa.parent().expect("old_aa tem pai").is_same(&new_a));
        assert!(new_a.parent().is_none());
    });
}

#[test]
fn moves_an_item_to_be_a_new_child() {
    run_async(|env| async move {
        env.expand(&env.root_id(), 0.0).await;
        env.single.collect_diff();
        let b = env.child("id-b");
        let a = env.child("id-a");
        a.children().add(b.clone());
        assert_eq!(
            env.single.collect_diff(),
            vec![TestsDiffOp::Remove { item_id: tid(&["ctrlId", "id-b"]) }, add_op(TestItemExpandState::NotExpandable, from_item(&b))]
        );

        b.set_label("still connected");
        assert_eq!(env.single.collect_diff(), vec![update_op(&tid(&["ctrlId", "id-a", "id-b"]), None, Some(label_patch("still connected")))]);

        let remaining: Vec<TestItemImpl> = env.root().children().entries().into_iter().map(|(_, item)| item).collect();
        assert_eq!(remaining.len(), 1);
        assert!(remaining[0].is_same(&env.child("id-a")));
        assert!(b.parent().expect("b tem pai").is_same(&a));
    });
}

#[test]
fn sends_document_sync_events() {
    run_async(|env| async move {
        env.expand(&env.root_id(), 0.0).await;
        env.single.collect_diff();

        let a = env.child("id-a");
        let range = Range::new(Position::new(0.0, 0.0).unwrap(), Position::new(1.0, 0.0).unwrap());
        a.set_range(Some(range));

        assert_eq!(
            env.single.collect_diff(),
            vec![
                document_synced(&uri_file("/")),
                update_op(
                    &tid(&["ctrlId", "id-a"]),
                    None,
                    Some(PartialTestItem {
                        range: Some(Some(EditorRange { start_line_number: 1, start_column: 1, end_line_number: 2, end_column: 1 })),
                        ..PartialTestItem::default()
                    })
                ),
            ]
        );

        // sends on replace even if it's a no-op
        a.set_range(a.range());
        assert_eq!(env.single.collect_diff(), vec![document_synced(&uri_file("/"))]);

        // sends on a child replacement
        let uri = uri_file("/");
        let a2 = new_item("id-a", "a", Some(uri.clone()));
        a2.set_range(a.range());
        env.root().children().replace(&[a2, env.child("id-b")]);
        assert_eq!(env.single.collect_diff(), vec![document_synced(&uri)]);
    });
}

// ---- MirroredTestCollection (os casos estão comentados no original) -------------------------------------

// ---- TestRunTracker -------------------------------------------------------------------------------------

struct TrackerEnv {
    env: Env,
    proxy: Rc<MockMainThreadTesting>,
    c: Rc<TestRunCoordinator>,
    cts: CancellationTokenSource,
    configuration: Rc<TestRunProfileImpl>,
    req: Rc<TestRunRequest>,
    dto: TestRunDto,
    ext: ExtensionDescription,
}

impl TrackerEnv {
    /// O `setup` da suíte `TestRunTracker`.
    fn new(guard: &LeakGuard) -> TrackerEnv {
        let env = Env::new(guard);
        let proxy = Rc::new(MockMainThreadTesting::default());
        let c = Rc::new(TestRunCoordinator::new(proxy.clone(), NullLogService::shared()));
        let configuration = TestRunProfileImpl::new(
            Rc::new(MockMainThreadTesting::default()),
            Default::default(),
            Default::default(),
            Event::none(),
            "ctrlId",
            42,
            "Do Run",
            types::TestRunProfileKind::Run,
            noop_run_handler(),
            false,
            None,
            false,
        );

        let _ = env.single.expand(&env.root_id(), INFINITY);
        env.single.collect_diff();

        let req = Rc::new(TestRunRequest {
            include: None,
            exclude: Some(vec![env.child("id-b")]),
            profile: Some(configuration.clone()),
            continuous: false,
            preserve_focus: false,
        });

        let dto = TestRunDto::from_internal(
            &IStartControllerTests {
                controller_id: "ctrl".to_string(),
                profile_id: configuration.profile_id(),
                exclude_ext_ids: vec!["id-b".to_string()],
                run_id: "run-id".to_string(),
                test_ids: vec![env.root_id()],
            },
            &env.single,
        );

        TrackerEnv { env, proxy, c, cts: CancellationTokenSource::new(None), configuration, req, dto, ext: null_extension_description() }
    }

    fn first_tracker(&self) -> Rc<TestRunTracker> {
        self.c.trackers().first().cloned().expect("há um tracker")
    }

    fn prepare(&self) -> Rc<TestRunTracker> {
        self.env.store.add_rc(self.c.prepare_for_main_thread_test_run(&self.ext, &self.req, self.dto.clone(), &self.configuration, self.cts.token()))
    }
}

impl Drop for TrackerEnv {
    /// O `teardown` da suíte: `for (const { id } of c.trackers) c.disposeTestRun(id)`.
    fn drop(&mut self) {
        for tracker in self.c.trackers() {
            self.c.dispose_test_run(&tracker.id());
        }
    }
}

fn run_tracker(body: impl FnOnce(&TrackerEnv)) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = TrackerEnv::new(&guard);
    body(&env);
}

#[test]
fn tracks_a_run_started_from_a_main_thread_request() {
    run_tracker(|t| {
        let tracker = t.prepare();
        assert!(!tracker.has_running_tasks());

        let task1 = t.c.create_test_run(&t.ext, "ctrl", &t.env.single, &t.req, Some("run1"), true);
        let task2 = t.c.create_test_run(&t.ext, "ctrl", &t.env.single, &t.req, Some("run2"), true);
        assert!(t.proxy.started_extension_test_run.borrow().is_empty());
        assert!(tracker.has_running_tasks());

        task1.append_output("hello", None, None);
        let task_id = t.proxy.append_output_to_run.borrow().first().map(|call| call.1.clone()).expect("houve saída");
        assert_eq!(*t.proxy.append_output_to_run.borrow(), vec![("run-id".to_string(), task_id, VSBuffer::from_string("hello").to_vec(), None, None)]);
        task1.end();

        assert!(t.proxy.finished_extension_test_run.borrow().is_empty());
        assert!(tracker.has_running_tasks());

        task2.end();

        assert!(t.proxy.finished_extension_test_run.borrow().is_empty());
        assert!(!tracker.has_running_tasks());
    });
}

#[test]
fn run_cancel_force_ends_after_a_timeout() {
    run_tracker(|t| {
        runtime::with_faked_timers(|| {
            let tracker = t.prepare();
            let task = t.c.create_test_run(&t.ext, "ctrl", &t.env.single, &t.req, Some("run1"), true);
            let ended = Rc::new(Cell::new(false));
            let on_ended = ended.clone();
            t.env.store.add(tracker.on_end().listen(move |_| on_ended.set(true)));

            assert!(!task.token().is_cancellation_requested());
            assert!(tracker.has_running_tasks());
            tracker.cancel(None);

            assert!(task.token().is_cancellation_requested());
            assert!(tracker.has_running_tasks());

            runtime::advance_fake_time(9999);
            assert!(tracker.has_running_tasks());
            assert!(!ended.get());

            runtime::advance_fake_time(1);
            assert!(ended.get());
            assert!(!tracker.has_running_tasks());
        });
    });
}

#[test]
fn run_cancel_force_ends_on_second_cancellation_request() {
    run_tracker(|t| {
        let tracker = t.prepare();
        let task = t.c.create_test_run(&t.ext, "ctrl", &t.env.single, &t.req, Some("run1"), true);
        let ended = Rc::new(Cell::new(false));
        let on_ended = ended.clone();
        t.env.store.add(tracker.on_end().listen(move |_| on_ended.set(true)));

        assert!(!task.token().is_cancellation_requested());
        assert!(tracker.has_running_tasks());
        tracker.cancel(None);

        assert!(task.token().is_cancellation_requested());
        assert!(tracker.has_running_tasks());
        assert!(!ended.get());
        tracker.cancel(None);

        assert!(!tracker.has_running_tasks());
        assert!(ended.get());
    });
}

#[test]
fn tracks_a_run_started_from_an_extension_request() {
    run_tracker(|t| {
        let task1 = t.c.create_test_run(&t.ext, "ctrl", &t.env.single, &t.req, Some("hello world"), false);

        let tracker = t.first_tracker();
        assert!(tracker.has_running_tasks());
        assert_eq!(
            *t.proxy.started_extension_test_run.borrow(),
            vec![ExtensionRunTestsRequest {
                profile: Some(ExtensionRunProfile { group: 2, id: 42 }),
                controller_id: "ctrl".to_string(),
                id: tracker.id(),
                include: vec![t.env.root_id()],
                exclude: vec![tid(&["ctrlId", "id-b"])],
                persist: false,
                continuous: false,
                preserve_focus: false,
            }]
        );

        let task2 = t.c.create_test_run(&t.ext, "ctrl", &t.env.single, &t.req, Some("run2"), true);
        let task3_detached = t.c.create_test_run(&t.ext, "ctrl", &t.env.single, &Rc::new((*t.req).clone()), Some("task3Detached"), true);

        task1.end();
        assert!(t.proxy.finished_extension_test_run.borrow().is_empty());
        assert!(tracker.has_running_tasks());

        task2.end();
        assert_eq!(*t.proxy.finished_extension_test_run.borrow(), vec![tracker.id()]);
        assert!(!tracker.has_running_tasks());

        task3_detached.end();
    });
}

#[test]
fn adds_tests_to_run_smartly() {
    run_tracker(|t| {
        let task1 = t.c.create_test_run(&t.ext, "ctrlId", &t.env.single, &t.req, Some("hello world"), false);
        let tracker = t.first_tracker();
        let mut expected_args: Vec<(String, String, Vec<ITestItemSerialized>)> = Vec::new();
        assert_eq!(*t.proxy.add_tests_to_run.borrow(), expected_args);

        let a = t.env.child("id-a");
        let aa = a.children().get("id-aa").unwrap();
        let ab = a.children().get("id-ab").unwrap();
        task1.passed(&aa, None);
        expected_args.push(("ctrlId".to_string(), tracker.id(), vec![serialized(&t.env.root()), serialized(&a), serialized(&aa)]));
        assert_eq!(*t.proxy.add_tests_to_run.borrow(), expected_args);

        task1.enqueued(&ab);
        expected_args.push(("ctrlId".to_string(), tracker.id(), vec![serialized(&a), serialized(&ab)]));
        assert_eq!(*t.proxy.add_tests_to_run.borrow(), expected_args);

        task1.passed(&ab, None);
        assert_eq!(*t.proxy.add_tests_to_run.borrow(), expected_args);

        task1.end();
    });
}

#[test]
fn adds_test_messages_to_run() {
    run_tracker(|t| {
        let test1 = new_item("id-c", "test c", Some(uri_file("/testc.txt")));
        let test2 = new_item("id-d", "test d", Some(uri_file("/testd.txt")));
        let range = Range::new(Position::new(0.0, 0.0).unwrap(), Position::new(1.0, 0.0).unwrap());
        test1.set_range(Some(range));
        test2.set_range(Some(range));
        t.env.root().children().replace(&[test1.clone(), test2.clone()]);
        let task = t.c.create_test_run(&t.ext, "ctrlId", &t.env.single, &t.req, Some("hello world"), false);

        let mut message1 = TestMessage::from_text("some message");
        message1.location = Some(Location::new(Some(uri_file("/a.txt")), Some(Position::new(0.0, 0.0).unwrap().into())));
        task.failed(&test1, std::slice::from_ref(&message1), None);

        let rich = |uri: &URI, range: &Range| IRichLocationSerialized { range: je_workbench::api::common::ext_host_type_converters::Range::from(range), uri: uri.to_json() };
        let error_message = |location: IRichLocationSerialized| {
            ITestMessageSerialized::Error(ITestErrorMessageSerialized {
                message: TestMessageText::Text("some message".to_string()),
                expected: None,
                actual: None,
                context_value: None,
                location: Some(location),
                stack_trace: None,
            })
        };
        let (run_id, task_id, _, _) = t.proxy.append_test_messages_in_run.borrow()[0].clone();
        let message1_location = message1.location.as_ref().unwrap();
        assert_eq!(
            t.proxy.append_test_messages_in_run.borrow()[0],
            (
                run_id.clone(),
                task_id.clone(),
                tid(&["ctrlId", "id-c"]),
                vec![error_message(rich(message1_location.uri.as_ref().unwrap(), message1_location.range.as_ref().unwrap()))]
            )
        );

        // should use test location as default
        task.failed(&test2, &[TestMessage::from_text("some message")], None);
        assert_eq!(
            t.proxy.append_test_messages_in_run.borrow()[1],
            (run_id, task_id, tid(&["ctrlId", "id-d"]), vec![error_message(rich(&test2.uri().unwrap(), &test2.range().unwrap()))])
        );

        task.end();
    });
}

#[test]
fn guards_calls_after_runs_are_ended() {
    run_tracker(|t| {
        let task = t.c.create_test_run(&t.ext, "ctrl", &t.env.single, &t.req, Some("hello world"), false);
        task.end();

        task.failed(&t.env.root(), &[TestMessage::new(Documentation::Text("some message".to_string()))], None);
        task.append_output("output", None, None);

        assert!(t.proxy.add_tests_to_run.borrow().is_empty());
        assert!(t.proxy.append_output_to_run.borrow().is_empty());
        assert!(t.proxy.append_test_messages_in_run.borrow().is_empty());
    });
}

#[test]
fn sets_state_of_test_with_identical_local_ids_131827() {
    run_tracker(|t| {
        let test_a = t.env.child("id-a");
        let test_b = t.env.child("id-b");
        let child_a = new_item("id-child", "child", None);
        test_a.children().replace(&[child_a.clone()]);
        let child_b = new_item("id-child", "child", None);
        test_b.children().replace(&[child_b.clone()]);

        let task1 = t.c.create_test_run(&t.ext, "ctrl", &t.env.single, &Rc::new(TestRunRequest::default()), Some("hello world"), false);
        let tracker = t.first_tracker();

        task1.passed(&child_a, None);
        task1.passed(&child_b, None);
        assert_eq!(
            *t.proxy.add_tests_to_run.borrow(),
            vec![
                ("ctrl".to_string(), tracker.id(), [t.env.root(), test_a, child_a].iter().map(serialized).collect::<Vec<ITestItemSerialized>>()),
                ("ctrl".to_string(), tracker.id(), [t.env.root(), test_b, child_b].iter().map(serialized).collect::<Vec<ITestItemSerialized>>()),
            ]
        );

        task1.end();
    });
}

// ---- service --------------------------------------------------------------------------------------------

struct ServiceEnv {
    env: Env,
    ctrl: Rc<ExtHostTesting>,
}

impl ServiceEnv {
    /// O `setup` da suíte `service`.
    fn new(guard: &LeakGuard) -> ServiceEnv {
        let env = Env::new(guard);
        let context: Rc<TestingContext> = Rc::new(TestingContext {
            testing: Rc::new(MockMainThreadTesting::default()),
            commands: Rc::new(MockMainThreadCommands),
            telemetry: Rc::new(MockMainThreadTelemetry),
        });
        let commands = ExtHostCommands::new(&*context, NullLogService::shared(), Rc::new(MockExtHostTelemetry));
        let editors = Rc::new(ExtHostDocumentsAndEditors::new(context.clone(), NullLogService::shared()));
        let ctrl = guard.add_rc(ExtHostTesting::new(&*context, NullLogService::shared(), commands, editors));
        ServiceEnv { env, ctrl }
    }

    /// `await single.expand(root, Infinity); ctrl.$acceptDiff(single.collectDiff().map(TestsDiffOp.serialize))`.
    fn publish_diff(&self) {
        self.ctrl.rpc_accept_diff(self.env.single.collect_diff().iter().map(TestsDiffOp::serialize).collect());
    }
}

fn run_service(body: impl FnOnce(&ServiceEnv)) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = ServiceEnv::new(&guard);
    body(&env);
}

/// `getProfileInternalId(ctrl, profile)`.
fn profile_internal_id(testing: &ExtHostTesting, ctrl: &TestController, profile: &Rc<TestRunProfileImpl>) -> u32 {
    let controller = testing.controller(ctrl.id()).expect("o controlador existe");
    for (id, p) in controller.profiles() {
        if Rc::ptr_eq(profile, &p) {
            return id;
        }
    }

    panic!("profile not found");
}

fn labels(tests: &[Rc<RefCell<PlainTestItem>>]) -> Vec<String> {
    tests.iter().map(|test| test.borrow().label.clone()).collect()
}

#[test]
fn disposing_an_observer_stops_its_events_while_another_observer_remains_live() {
    run_service(|s| {
        let retired = s.env.store.add_rc(s.ctrl.create_test_observer());
        let live = s.env.store.add_rc(s.ctrl.create_test_observer());
        let retired_changes = Rc::new(Cell::new(0));
        let live_changes = Rc::new(Cell::new(0));
        let retired_counter = retired_changes.clone();
        let live_counter = live_changes.clone();
        s.env.store.add(retired.on_did_change_test().listen(move |_| retired_counter.set(retired_counter.get() + 1)));
        s.env.store.add(live.on_did_change_test().listen(move |_| live_counter.set(live_counter.get() + 1)));
        retired.dispose();
        retired.dispose();

        let _ = s.env.single.expand(&s.env.root_id(), INFINITY);
        s.publish_diff();
        assert_eq!((retired_changes.get(), live_changes.get(), labels(&live.tests())), (0, 1, vec!["root".to_string()]));
    });
}

#[test]
fn subscribing_to_a_disposed_observer_does_not_reattach_its_events() {
    run_service(|s| {
        let retired = s.env.store.add_rc(s.ctrl.create_test_observer());
        let live = s.env.store.add_rc(s.ctrl.create_test_observer());
        retired.dispose();
        let changes = Rc::new(Cell::new(0));
        let counter = changes.clone();
        s.env.store.add(retired.on_did_change_test().listen(move |_| counter.set(counter.get() + 1)));
        let _ = s.env.single.expand(&s.env.root_id(), INFINITY);
        s.publish_diff();
        assert_eq!((changes.get(), labels(&live.tests())), (0, vec!["root".to_string()]));
    });
}

#[test]
fn observer_listeners_can_unsubscribe_and_subscribe_again() {
    run_service(|s| {
        let observer = s.env.store.add_rc(s.ctrl.create_test_observer());
        let removed_changes = Rc::new(Cell::new(0));
        let removed_counter = removed_changes.clone();
        let subscription = s.env.store.add(observer.on_did_change_test().listen(move |_| removed_counter.set(removed_counter.get() + 1)));
        subscription.dispose();
        let _ = s.env.single.expand(&s.env.root_id(), INFINITY);
        s.publish_diff();
        let current_changes = Rc::new(Cell::new(0));
        let current_counter = current_changes.clone();
        s.env.store.add(observer.on_did_change_test().listen(move |_| current_counter.set(current_counter.get() + 1)));
        s.env.root().set_label("Updated root");
        s.publish_diff();
        assert_eq!((removed_changes.get(), current_changes.get(), labels(&observer.tests())), (0, 1, vec!["Updated root".to_string()]));
    });
}

#[test]
fn a_new_observer_starts_a_fresh_collection_after_the_last_observer_closes() {
    run_service(|s| {
        let retired = s.env.store.add_rc(s.ctrl.create_test_observer());
        let _ = s.env.single.expand(&s.env.root_id(), INFINITY);
        let initial_diff: Vec<TestsDiffOpSerialized> = s.env.single.collect_diff().iter().map(TestsDiffOp::serialize).collect();
        s.ctrl.rpc_accept_diff(initial_diff.clone());
        retired.dispose();
        let replacement = s.env.store.add_rc(s.ctrl.create_test_observer());
        let initial_roots = replacement.tests();
        let changes = Rc::new(Cell::new(0));
        let counter = changes.clone();
        s.env.store.add(replacement.on_did_change_test().listen(move |_| counter.set(counter.get() + 1)));
        s.ctrl.rpc_accept_diff(initial_diff);
        assert_eq!((initial_roots.len(), changes.get(), labels(&replacement.tests())), (0, 1, vec!["root".to_string()]));
    });
}

#[test]
fn exposes_active_profiles_correctly() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let s = Rc::new(ServiceEnv::new(&guard));
    run_with_faked_timers(async move {
        let extension = |id: &str| {
            let mut extension = null_extension_description();
            extension.identifier = ExtensionIdentifier::new(id);
            extension.manifest.enabled_api_proposals = Some(vec!["testingActiveProfile".to_string()]);
            extension
        };
        let ext_a = extension("ext.a");
        let ext_b = extension("ext.b");
        let store = &s.env.store;

        let ctrl_a = store.add_rc(s.ctrl.create_test_controller(&ext_a, "a", "ctrla", None).unwrap());
        let prof_aa = store.add_rc(ctrl_a.create_run_profile("aa", types::TestRunProfileKind::Run, noop_run_handler(), false, None, false));
        let prof_ab = store.add_rc(ctrl_a.create_run_profile("ab", types::TestRunProfileKind::Run, noop_run_handler(), false, None, false));

        let ctrl_b = store.add_rc(s.ctrl.create_test_controller(&ext_b, "b", "ctrlb", None).unwrap());
        let prof_ba = store.add_rc(ctrl_b.create_run_profile("ba", types::TestRunProfileKind::Run, noop_run_handler(), false, None, false));
        let prof_bb = store.add_rc(ctrl_b.create_run_profile("bb", types::TestRunProfileKind::Run, noop_run_handler(), false, None, false));
        let never_called = Rc::new(Cell::new(false));

        // empty default state:
        assert!(!prof_aa.is_default());
        assert!(!prof_ba.is_default());
        assert!(!prof_bb.is_default());

        // fires a change event:
        let change_a = prof_aa.on_did_change_default().to_promise(None);
        let change_ba = prof_ba.on_did_change_default().to_promise(None);
        let change_bb = prof_bb.on_did_change_default().to_promise(None);

        let flag = never_called.clone();
        store.add(prof_ab.on_did_change_default().listen(move |_| flag.set(true)));
        assert!(!never_called.get());

        s.ctrl.rpc_set_default_run_profiles(vec![
            ("a".to_string(), vec![profile_internal_id(&s.ctrl, &ctrl_a, &prof_aa)]),
            ("b".to_string(), vec![profile_internal_id(&s.ctrl, &ctrl_b, &prof_ba), profile_internal_id(&s.ctrl, &ctrl_b, &prof_bb)]),
        ]);

        assert!(change_a.await.unwrap());
        assert!(change_ba.await.unwrap());
        assert!(change_bb.await.unwrap());

        // updates internal state:
        assert!(prof_aa.is_default());
        assert!(prof_ba.is_default());
        assert!(prof_bb.is_default());
        assert!(!prof_ab.is_default());

        // no-ops if equal
        let flag = never_called.clone();
        store.add(prof_aa.on_did_change_default().listen(move |_| flag.set(true)));
        s.ctrl.rpc_set_default_run_profiles(vec![("a".to_string(), vec![profile_internal_id(&s.ctrl, &ctrl_a, &prof_aa)])]);
        assert!(!never_called.get());
    });
}
