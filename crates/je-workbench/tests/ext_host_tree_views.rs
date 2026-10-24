//! Porte de `src/vs/workbench/api/test/browser/extHostTreeViews.test.ts` (suíte `ExtHostTreeView`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado em [`run_test`]; todos os casos são `async` e rodam
//!   em `run_with_faked_timers` (o `setup` do original devolve `loadCompleteTree(...)`, que `run_test` espera antes do
//!   corpo). Os casos `done` do original (`test('...', (done) => ...)`) esperam uma promessa que o ouvinte resolve, e uma
//!   asserção que falha dentro do ouvinte rejeita essa promessa ([`after_refresh`]), em vez de ficar pendente. O
//!   `debounce` de 200 ms roda no relógio virtual.
//! - `loadCompleteTree` espera só a primeira chamada de `$getChildren`: o `.then(elements => elements[0].slice(1).map(...))`
//!   devolve um array de promessas, que o `then` NÃO espera, e as cargas dos filhos continuam em segundo plano. O
//!   `mocha` só começa o caso depois da macrotarefa seguinte (as microtarefas já esvaziaram), que [`run_test`] reproduz
//!   com `timeout(0)`.
//! - `mock<MainThreadTreeViewsShape>()` com `override` é [`RecordingShape`]: um método sem `override`
//!   (`$setMessage`, `$setTitle`...) falha com `panic!`, como o método ausente do objeto vazio lançaria `TypeError`.
//!   `sinon.spy(target, '$reveal')` e `'$disposeTree'`/`'$registerTreeViewDataProvider'` são os registros de chamadas do
//!   dublê (`calledOnce` é uma chamada a partir do início do caso). `onRefresh` é um [`Emitter`] com o
//!   `itemsToRefresh` (`None` é o `undefined`).
//! - `new TestRPCProtocol()` com `rpcProtocol.set(MainContext.MainThreadCommands, ...)` é [`TreeViewsContext`]: escolhe o
//!   proxy pelo `sid` (o `MainThreadCommands` do caso, e um `MainThreadTelemetry` vazio que o construtor de
//!   `ExtHostCommands` pede). `new class extends mock<IExtHostTelemetry>()` é [`MockExtHostTelemetry`].
//! - O elemento `{ key }` (a classe `Key`) e o `TreeItem` do provedor são [`Dynamic`]; `tree`, `labels` e `nodes` do
//!   original são o [`Fixture`]. `removeUnsetKeys(item)` é [`ITreeItem::to_dynamic`] (o item sem as propriedades
//!   `undefined`), comparado com `deepStrictEqual` por [`assert_deep_strict_equal`].
//! - `unBatchChildren` é [`un_batch_children`] (o `(number | ITreeItem)[]` do original é o struct `TreeChildrenBatch`).
//! - `Reflect.get(testObject, '_treeDragAndDropService')` e `Reflect.get(service, '_dragOperations')` são
//!   `tree_drag_and_drop_service()` e `drag_operations()` do serviço.
//! - O `Promise.all` do caso `construct node tree` e dos seus irmãos é `Promise::all` de promessas de funções `async`.
//! - `new Key(parentKey)` em `getParent` cria um objeto novo a cada chamada, e `getNode(key)` devolve sempre o mesmo
//!   objeto do cache `nodes`, como no original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{json_stringify, Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::Emitter;
use je_base::js_string::JsString;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::objects;
use je_base::promise::{DeferredPromise, MaybePromise, Promise, Resolver};
use je_editor::languages::StringOrMarkdown;
use je_extensions::manifest::ExtensionIdentifier;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_commands::ExtHostCommands;
use je_workbench::api::common::ext_host_protocol::{
    CommandArgs, DataTransferDto, ExtHostTreeViewsShape, IExtHostTelemetry, IMainContext, ItemsToRefresh, MainThreadCommandsShape, MainThreadTelemetryShape,
    MainThreadTreeViewsShape, RevealItemInfo, TreeChildrenBatch, TreeViewRegistrationOptions,
};
use je_workbench::api::common::ext_host_tree_views::{
    ExtHostTreeViews, TreeDataProvider, TreeDragAndDropController, TreeHandleDrag, TreeHandleDrop, TreeViewHandle, TreeViewOptions,
};
use je_workbench::api::common::ext_host_types::{DataTransfer, DataTransferItem};
use je_workbench::common::views::{IRevealOptions, ITreeItem, IViewBadge, RevealExpand, TreeItemCollapsibleState};
use je_workbench::services::extensions::extensions::null_extension_description;
use je_editor::tree_views_dnd::TreeViewsDnDService;

// ---- os dublês --------------------------------------------------------------------------------------

/// `new class extends mock<MainThreadCommandsShape>() { override $registerCommand() { } }`.
struct MockMainThreadCommands;

impl IDisposable for MockMainThreadCommands {
    fn dispose(&self) {}
}

impl MainThreadCommandsShape for MockMainThreadCommands {
    fn rpc_register_command(&self, _id: &str) {}

    fn rpc_unregister_command(&self, _id: &str) {
        panic!("$unregisterCommand is not a function")
    }

    fn rpc_fire_command_activation_event(&self, _id: &str) {
        panic!("$fireCommandActivationEvent is not a function")
    }

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

/// O `TestRPCProtocol` do caso: devolve o proxy certo por `sid`.
struct TreeViewsContext {
    commands: Rc<dyn MainThreadCommandsShape>,
    telemetry: Rc<dyn MainThreadTelemetryShape>,
}

impl IMainContext for TreeViewsContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        match sid {
            "MainThreadCommands" => Rc::new(self.commands.clone()),
            "MainThreadTelemetry" => Rc::new(self.telemetry.clone()),
            other => panic!("proxy inesperado: {other}"),
        }
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

/// `class RecordingShape extends mock<MainThreadTreeViewsShape>()`.
struct RecordingShape {
    on_refresh: Rc<Emitter<Option<ItemsToRefresh>>>,
    /// Os argumentos de cada `$reveal` (o `sinon.spy`).
    reveal_calls: RefCell<Vec<(String, Option<RevealItemInfo>, IRevealOptions)>>,
    dispose_tree_calls: Cell<usize>,
    register_calls: Cell<usize>,
}

impl RecordingShape {
    fn new() -> Rc<RecordingShape> {
        Rc::new(RecordingShape { on_refresh: Emitter::new(), reveal_calls: RefCell::new(Vec::new()), dispose_tree_calls: Cell::new(0), register_calls: Cell::new(0) })
    }
}

impl IDisposable for RecordingShape {
    fn dispose(&self) {}
}

impl MainThreadTreeViewsShape for RecordingShape {
    fn rpc_register_tree_view_data_provider(&self, _tree_view_id: &str, _options: TreeViewRegistrationOptions) -> Promise<()> {
        self.register_calls.set(self.register_calls.get() + 1);
        async_fn(async { Ok(()) })
    }

    fn rpc_refresh(&self, _view_id: &str, items_to_refresh: Option<ItemsToRefresh>) -> Promise<()> {
        let on_refresh = self.on_refresh.clone();
        Promise::resolved(()).then(move |_| {
            on_refresh.fire(items_to_refresh);
        })
    }

    fn rpc_reveal(&self, tree_view_id: &str, item_info: Option<RevealItemInfo>, options: IRevealOptions) -> Promise<()> {
        self.reveal_calls.borrow_mut().push((tree_view_id.to_owned(), item_info, options));
        Promise::resolved(())
    }

    fn rpc_set_message(&self, _tree_view_id: &str, _message: StringOrMarkdown) {
        panic!("$setMessage is not a function")
    }

    fn rpc_set_title(&self, _tree_view_id: &str, _title: &str, _description: Option<&str>) {
        panic!("$setTitle is not a function")
    }

    fn rpc_set_badge(&self, _tree_view_id: &str, _badge: Option<IViewBadge>) {
        panic!("$setBadge is not a function")
    }

    fn rpc_resolve_drop_file_data(&self, _destination_view_id: &str, _request_id: f64, _data_item_id: &str) -> Promise<VSBuffer> {
        panic!("$resolveDropFileData is not a function")
    }

    fn rpc_dispose_tree(&self, _tree_view_id: &str) -> Promise<()> {
        self.dispose_tree_calls.set(self.dispose_tree_calls.get() + 1);
        Promise::resolved(())
    }

    fn rpc_log_resolve_tree_node_failure(&self, _extension_id: &str) {
        panic!("$logResolveTreeNodeFailure is not a function")
    }
}

// ---- o estado do caso: `tree`, `labels`, `nodes` e os emissores ----------------------------------------

/// `class Key { constructor(readonly key: string) { } }`.
fn new_key(key: &str) -> Dynamic {
    Dynamic::object([("key", Dynamic::string(key))])
}

/// `element.key`.
fn key_of(element: &Dynamic) -> String {
    match element.get_str("key") {
        Dynamic::String(key) => key.to_string_lossy(),
        _ => String::new(),
    }
}

/// `object[key] = value`.
fn set_key(object: &Dynamic, key: &str, value: Dynamic) {
    match object {
        Dynamic::Object(object) => object.set(&JsString::from(key), value),
        other => panic!("não é um objeto: {other:?}"),
    }
}

/// Um objeto com as chaves dadas, na ordem.
fn object(entries: Vec<(&str, Dynamic)>) -> Dynamic {
    Dynamic::object(entries)
}

struct Fixture {
    tree: RefCell<Dynamic>,
    labels: RefCell<HashMap<String, String>>,
    nodes: RefCell<HashMap<String, Dynamic>>,
    on_did_change_tree_node: Rc<Emitter<Dynamic>>,
    on_did_change_tree_node_with_id: Rc<Emitter<Dynamic>>,
}

/// O `tree` que o `setup` monta.
fn initial_tree() -> Dynamic {
    object(vec![
        ("a", object(vec![("aa", object(vec![])), ("ab", object(vec![]))])),
        ("b", object(vec![("ba", object(vec![])), ("bb", object(vec![]))])),
    ])
}

impl Fixture {
    fn new() -> Rc<Fixture> {
        Rc::new(Fixture {
            tree: RefCell::new(initial_tree()),
            labels: RefCell::new(HashMap::new()),
            nodes: RefCell::new(HashMap::new()),
            on_did_change_tree_node: Emitter::new(),
            on_did_change_tree_node_with_id: Emitter::new(),
        })
    }

    /// `tree = value`.
    fn set_tree(&self, tree: Dynamic) {
        *self.tree.borrow_mut() = tree;
    }

    fn get_tree_element(&self, element: &str) -> Dynamic {
        let mut parent = self.tree.borrow().clone();
        for index in 0..element.chars().count() {
            let key: String = element.chars().take(index + 1).collect();
            parent = parent.get_str(&key);
            if !parent.is_truthy() {
                return Dynamic::Null;
            }
        }
        parent
    }

    fn get_children(&self, key: Option<&str>) -> Vec<String> {
        let keys = |value: &Dynamic| value.own_keys().iter().map(JsString::to_string_lossy).collect::<Vec<_>>();
        match key.filter(|key| !key.is_empty()) {
            None => keys(&self.tree.borrow()),
            Some(key) => {
                let tree_element = self.get_tree_element(key);
                if tree_element.is_truthy() { keys(&tree_element) } else { Vec::new() }
            }
        }
    }

    fn get_tree_item(&self, key: &str, highlights: Option<&[(f64, f64)]>) -> Dynamic {
        let tree_element = self.get_tree_element(key);
        let label = self.labels.borrow().get(key).filter(|label| !label.is_empty()).cloned().unwrap_or_else(|| key.to_owned());
        let highlights = match highlights {
            Some(highlights) => Dynamic::array(highlights.iter().map(|(start, end)| Dynamic::array([Dynamic::Number(*start), Dynamic::Number(*end)]))),
            None => Dynamic::Undefined,
        };
        let collapsible_state = if tree_element.is_truthy() && !tree_element.own_keys().is_empty() { TreeItemCollapsibleState::Collapsed } else { TreeItemCollapsibleState::None };
        object(vec![
            ("label", object(vec![("label", Dynamic::string(&label)), ("highlights", highlights)])),
            ("collapsibleState", Dynamic::from(collapsible_state.value())),
        ])
    }

    fn get_node(&self, key: &str) -> Dynamic {
        self.nodes.borrow_mut().entry(key.to_owned()).or_insert_with(|| new_key(key)).clone()
    }

    fn set_label(&self, key: &str, label: &str) {
        self.labels.borrow_mut().insert(key.to_owned(), label.to_owned());
    }
}

/// Qual dos quatro provedores do original.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProviderKind {
    /// `aNodeTreeDataProvider`.
    Node,
    /// `aCompleteNodeTreeDataProvider`.
    Complete,
    /// `aNodeWithIdTreeDataProvider`.
    WithId,
    /// `aNodeWithHighlightedLabelTreeDataProvider`.
    Highlighted,
}

fn tree_data_provider(fixture: &Rc<Fixture>, kind: ProviderKind) -> Rc<TreeDataProvider> {
    let for_children = fixture.clone();
    let for_item = fixture.clone();
    let has_id = matches!(kind, ProviderKind::WithId | ProviderKind::Highlighted);
    let highlights: Option<Vec<(f64, f64)>> = if kind == ProviderKind::Highlighted { Some(vec![(0.0, 2.0), (3.0, 5.0)]) } else { None };
    Rc::new(TreeDataProvider {
        get_children: Rc::new(move |element: Option<&Dynamic>| {
            let key = element.map(key_of);
            MaybePromise::Value(Some(for_children.get_children(key.as_deref()).iter().map(|key| for_children.get_node(key)).collect()))
        }),
        get_tree_item: Rc::new(move |element: &Dynamic| {
            let key = key_of(element);
            let tree_item = for_item.get_tree_item(&key, highlights.as_deref());
            if has_id {
                set_key(&tree_item, "id", Dynamic::string(&key));
            }
            MaybePromise::Value(tree_item)
        }),
        get_parent: if kind == ProviderKind::Complete {
            Some(Rc::new(|element: &Dynamic| {
                let key = key_of(element);
                let parent_key: String = key.chars().take(key.chars().count().saturating_sub(1)).collect();
                MaybePromise::Value(if parent_key.is_empty() { Dynamic::Undefined } else { new_key(&parent_key) })
            }))
        } else {
            None
        },
        resolve_tree_item: None,
        on_did_change_tree_data: Some(if has_id { fixture.on_did_change_tree_node_with_id.event() } else { fixture.on_did_change_tree_node.event() }),
    })
}

// ---- o ambiente de cada caso -----------------------------------------------------------------------

#[derive(Clone)]
struct Env {
    test_object: Rc<ExtHostTreeViews>,
    target: Rc<RecordingShape>,
    fixture: Rc<Fixture>,
    store: Rc<DisposableStore>,
    extension: ExtensionDescription,
}

impl Env {
    /// O `setup` síncrono: o resto (`return loadCompleteTree('testNodeTreeProvider')`) é de [`run_test`].
    fn new(guard: &common::LeakGuard) -> Env {
        let fixture = Fixture::new();
        let extension = null_extension_description();

        let context = TreeViewsContext { commands: Rc::new(MockMainThreadCommands), telemetry: Rc::new(MockMainThreadTelemetry) };
        let target = RecordingShape::new();
        let commands = ExtHostCommands::new(&context, NullLogService::shared(), Rc::new(MockExtHostTelemetry));
        let test_object = guard.add_rc(ExtHostTreeViews::new(target.clone(), &commands, NullLogService::shared()));
        for (id, kind) in [
            ("testNodeTreeProvider", ProviderKind::Node),
            ("testNodeWithIdTreeProvider", ProviderKind::WithId),
            ("testNodeWithHighlightsTreeProvider", ProviderKind::Highlighted),
        ] {
            test_object.create_tree_view(id, TreeViewOptions::new(tree_data_provider(&fixture, kind)), &extension).expect("createTreeView");
        }

        Env { test_object, target, fixture, store: guard.store(), extension }
    }

    fn create_tree_view(&self, id: &str, kind: ProviderKind) -> Rc<TreeViewHandle> {
        self.test_object.create_tree_view(id, TreeViewOptions::new(tree_data_provider(&self.fixture, kind)), &self.extension).expect("createTreeView")
    }

    /// `testObject.$getChildren(treeId, treeItemHandles)`.
    fn get_children(&self, tree_id: &str, handles: Option<&[&str]>) -> Promise<Option<Vec<TreeChildrenBatch>>> {
        self.test_object.rpc_get_children(tree_id, handles.map(|handles| handles.iter().map(|handle| (*handle).to_owned()).collect()))
    }

    /// `function loadCompleteTree(treeId, element?)`.
    fn load_complete_tree(&self, tree_id: &str, element: Option<&str>) -> Promise<()> {
        let this = self.clone();
        let (tree_id, element) = (tree_id.to_owned(), element.map(str::to_owned));
        async_fn(async move {
            let elements = this.get_children(&tree_id, element.as_deref().map(|element| vec![element]).as_deref()).await?;
            let Some(elements) = elements else {
                return Ok(());
            };
            if elements.is_empty() {
                return Ok(());
            }
            // `.map(e => loadCompleteTree(...))` devolve promessas que o `then` seguinte não espera.
            for child in &elements[0].children {
                let _ = this.load_complete_tree(&tree_id, Some(&child.handle));
            }
            Ok(())
        })
    }
}

/// O corpo de um caso: o `setup` (a carga da árvore), a macrotarefa do `mocha` e o caso.
fn run_test<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Env) -> Fut + 'static) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&_guard);
    let setup = env.clone();
    run_with_faked_timers(async move {
        setup.load_complete_tree("testNodeTreeProvider", None).await.expect("setup");
        timeout(0).await.expect("timeout");
        body(env).await;
    });
}

// ---- as funções auxiliares do original ----------------------------------------------------------------

/// `unBatchChildren(result)`.
fn un_batch_children(result: Option<Vec<TreeChildrenBatch>>) -> Option<Vec<ITreeItem>> {
    let result = result?;
    if result.is_empty() {
        return None;
    }
    if result.len() > 1 {
        panic!("Unexpected result length, all tests are unbatched.");
    }
    Some(result[0].children.clone())
}

/// `unBatchChildren(...)?.map(e => e.handle)`.
fn handles_of(result: Option<Vec<TreeChildrenBatch>>) -> Option<Vec<String>> {
    un_batch_children(result).map(|items| items.into_iter().map(|item| item.handle).collect())
}

fn strings(values: &[&str]) -> Option<Vec<String>> {
    Some(values.iter().map(|value| (*value).to_owned()).collect())
}

/// `removeUnsetKeys(items)`.
fn remove_unset_keys(items: Option<Vec<ITreeItem>>) -> Dynamic {
    match items {
        Some(items) => Dynamic::array(items.iter().map(ITreeItem::to_dynamic)),
        None => Dynamic::Undefined,
    }
}

/// `assert.deepStrictEqual(actual, expected)` para valores [`Dynamic`].
fn assert_deep_strict_equal(actual: &Dynamic, expected: &Dynamic) {
    assert!(objects::equals(actual, expected), "esperado {:?}, obtido {:?}", json_stringify(expected), json_stringify(actual));
}

/// O objeto `{ handle, parentHandle?, label: { label, highlights? }, collapsibleState }` que os casos esperam.
fn expected_item(handle: &str, parent_handle: Option<&str>, label: &str, highlights: Option<&[(f64, f64)]>, collapsible_state: TreeItemCollapsibleState) -> Dynamic {
    let mut label_entries = vec![("label", Dynamic::string(label))];
    if let Some(highlights) = highlights {
        label_entries.push(("highlights", Dynamic::array(highlights.iter().map(|(start, end)| Dynamic::array([Dynamic::Number(*start), Dynamic::Number(*end)])))));
    }
    let mut entries = vec![("handle", Dynamic::string(handle))];
    if let Some(parent_handle) = parent_handle {
        entries.push(("parentHandle", Dynamic::string(parent_handle)));
    }
    entries.push(("label", object(label_entries)));
    entries.push(("collapsibleState", Dynamic::from(collapsible_state.value())));
    object(entries)
}

fn keys_of(actuals: &ItemsToRefresh) -> Vec<&str> {
    actuals.iter().map(|(handle, _)| handle.as_str()).collect()
}

fn item_at<'a>(actuals: &'a ItemsToRefresh, handle: &str) -> &'a ITreeItem {
    &actuals.iter().find(|(key, _)| key == handle).unwrap_or_else(|| panic!("sem o handle {handle} em itemsToRefresh")).1
}

/// `store.add(target.onRefresh.event(actuals => { ...; done(); }))`: o `check` roda a cada `onRefresh`, e a promessa
/// devolvida resolve (ou rejeita, se uma asserção do `check` falhar) com o resultado dele.
fn after_refresh(env: &Env, check: impl Fn(&Option<ItemsToRefresh>) -> Promise<()> + 'static) -> Promise<()> {
    let (done, resolver) = Promise::new();
    env.store.add(env.target.on_refresh.event().listen(move |actuals| match errors::catch(|| check(actuals)) {
        Ok(result) => result.pipe_to(&resolver),
        Err(error) => resolver.reject(error),
    }));
    done
}

/// `async function runWithEventMerging(action)`, já dentro do `runWithFakedTimers` do caso.
async fn run_with_event_merging(env: &Env, action: impl FnOnce(Resolver<()>)) {
    let (first, first_resolver) = Promise::<()>::new();
    let subscription: Rc<RefCell<Option<DisposableRef>>> = Rc::default();
    let own_subscription = subscription.clone();
    *subscription.borrow_mut() = Some(env.target.on_refresh.event().listen(move |_| {
        if let Some(subscription) = own_subscription.borrow_mut().take() {
            subscription.dispose();
        }
        first_resolver.resolve(());
    }));
    env.fixture.on_did_change_tree_node.fire(env.fixture.get_node("b"));
    first.await.expect("primeiro refresh");

    let (second, second_resolver) = Promise::<()>::new();
    action(second_resolver);
    second.await.expect("refresh das mudanças mescladas");
}

/// `fire(getNode(key))` em `onDidChangeTreeNode`.
fn fire_node(env: &Env, key: &str) {
    env.fixture.on_did_change_tree_node.fire(env.fixture.get_node(key));
}

// ---- os casos de arrastar e soltar ---------------------------------------------------------------

/// `async function prepareDragTransfer(handleDrag, handleDrop?)`: devolve o handle do primeiro filho e o serviço
/// (`operations` é o `_dragOperations` dele).
async fn prepare_drag_transfer(env: &Env, handle_drag: TreeHandleDrag, handle_drop: Option<TreeHandleDrop>) -> (String, Rc<TreeViewsDnDService<Rc<DataTransfer>>>) {
    let controller = TreeDragAndDropController {
        drag_mime_types: vec!["text/plain".to_owned()],
        drop_mime_types: vec!["text/plain".to_owned()],
        handle_drag: Some(handle_drag),
        handle_drop,
    };
    let options = TreeViewOptions { drag_and_drop_controller: Some(Rc::new(controller)), ..TreeViewOptions::new(tree_data_provider(&env.fixture, ProviderKind::Node)) };
    env.store.add(env.test_object.create_tree_view("dragTransferTree", options, &env.extension).expect("createTreeView"));
    let children = un_batch_children(env.get_children("dragTransferTree", None).await.expect("$getChildren"));
    assert!(children.as_ref().is_some_and(|children| !children.is_empty()));
    (children.expect("filhos")[0].handle.clone(), env.test_object.tree_drag_and_drop_service())
}

/// `(_items, transfer) => transfer.set('text/plain', new DataTransferItem('Owned transfer'))`.
fn set_owned_transfer() -> TreeHandleDrag {
    Rc::new(|_items: &[Dynamic], transfer: &Rc<DataTransfer>, _token: &CancellationToken| {
        transfer.set("text/plain", DataTransferItem::new(Dynamic::string("Owned transfer")));
        MaybePromise::Value(())
    })
}

/// `async (_items, transfer) => { transfer.set(...); await pending.p; }`.
fn set_owned_transfer_then_wait(pending: Rc<DeferredPromise<()>>) -> TreeHandleDrag {
    Rc::new(move |_items: &[Dynamic], transfer: &Rc<DataTransfer>, _token: &CancellationToken| {
        let (transfer, pending) = (transfer.clone(), pending.clone());
        MaybePromise::Promise(async_fn(async move {
            transfer.set("text/plain", DataTransferItem::new(Dynamic::string("Owned transfer")));
            pending.promise.clone().await?;
            Ok(())
        }))
    })
}

#[test]
fn drag_transfer_remains_available_until_it_is_consumed() {
    run_test(|env| async move {
        let (handle, service) = prepare_drag_transfer(&env, set_owned_transfer(), None).await;
        let result = env.test_object.rpc_handle_drag("dragTransferTree", vec![handle], "active-drag", CancellationToken::None).await.expect("$handleDrag");
        assert_eq!(
            (service.drag_operations().len(), result.map(|result| result.items[0].1.as_string.clone())),
            (1, Some("Owned transfer".to_owned()))
        );
    });
}

#[test]
fn drag_transfer_is_released_when_canceled_before_provider_completion() {
    run_test(|env| async move {
        let pending = Rc::new(DeferredPromise::<()>::new());
        let (handle, service) = prepare_drag_transfer(&env, set_owned_transfer_then_wait(pending.clone()), None).await;
        let cancellation = CancellationTokenSource::new(None);
        env.store.add(Rc::new(cancellation.clone()));
        let creation = env.test_object.rpc_handle_drag("dragTransferTree", vec![handle], "canceled-drag", cancellation.token());
        cancellation.cancel();
        pending.complete(()).await.expect("complete");
        let result = creation.await.expect("$handleDrag");
        assert!(service.drag_operations().is_empty());
        assert!(result.is_none());
    });
}

#[test]
fn drag_transfer_is_released_when_the_provider_rejects() {
    run_test(|env| async move {
        let failing: TreeHandleDrag = Rc::new(|_items: &[Dynamic], _transfer: &Rc<DataTransfer>, _token: &CancellationToken| {
            MaybePromise::Promise(Promise::rejected(Error::new("Expected drag provider failure")))
        });
        let (handle, service) = prepare_drag_transfer(&env, failing, None).await;
        let error = env
            .test_object
            .rpc_handle_drag("dragTransferTree", vec![handle], "failed-drag", CancellationToken::None)
            .await
            .expect_err("o $handleDrag devia rejeitar");
        assert!(error.message.contains("Expected drag provider failure"), "{error}");
        assert_eq!(service.drag_operations().len(), 0);
    });
}

#[test]
fn drag_transfer_release_retires_completed_data_without_touching_a_live_operation() {
    run_test(|env| async move {
        let (handle, service) = prepare_drag_transfer(&env, set_owned_transfer(), None).await;
        env.test_object.rpc_handle_drag("dragTransferTree", vec![handle.clone()], "retired-drag", CancellationToken::None).await.expect("$handleDrag");
        env.test_object.rpc_handle_drag("dragTransferTree", vec![handle], "live-drag", CancellationToken::None).await.expect("$handleDrag");
        let live = service.drag_operation("live-drag").expect("live-drag");
        env.test_object.rpc_release_drag_operation("retired-drag");
        env.test_object.rpc_release_drag_operation("retired-drag");
        env.test_object.rpc_release_drag_operation("unknown-drag");
        let keys: Vec<String> = service.drag_operations().into_iter().map(|(key, _)| key).collect();
        let same_live = service.drag_operation("live-drag").is_some_and(|operation| operation.ptr_eq(&live));
        assert_eq!((keys, same_live), (vec!["live-drag".to_owned()], true));
    });
}

#[test]
fn drag_transfer_release_does_not_restore_data_after_late_provider_completion() {
    run_test(|env| async move {
        let pending = Rc::new(DeferredPromise::<()>::new());
        let (handle, service) = prepare_drag_transfer(&env, set_owned_transfer_then_wait(pending.clone()), None).await;
        let creation = env.test_object.rpc_handle_drag("dragTransferTree", vec![handle], "retired-drag", CancellationToken::None);
        env.test_object.rpc_release_drag_operation("retired-drag");
        assert_eq!(service.drag_operations().len(), 0);
        pending.complete(()).await.expect("complete");
        creation.await.expect("$handleDrag");
        assert_eq!(service.drag_operations().len(), 0);
    });
}

#[test]
fn drag_transfer_release_does_not_accumulate_over_repeated_completed_cancellations() {
    run_test(|env| async move {
        let (handle, service) = prepare_drag_transfer(&env, set_owned_transfer(), None).await;
        for index in 0..37 {
            let uuid = format!("retired-drag-{index}");
            env.test_object.rpc_handle_drag("dragTransferTree", vec![handle.clone()], &uuid, CancellationToken::None).await.expect("$handleDrag");
            env.test_object.rpc_release_drag_operation(&uuid);
        }
        assert_eq!(service.drag_operations().len(), 0);
    });
}

#[test]
fn drag_transfer_consumption_preserves_original_item_identity_for_a_successful_drop() {
    run_test(|env| async move {
        let original = DataTransferItem::new(object(vec![("label", Dynamic::string("Owned transfer"))]));
        let dropped: Rc<RefCell<Option<Rc<DataTransferItem>>>> = Rc::default();
        let (drag_original, dropped_by_handler) = (original.clone(), dropped.clone());
        let handle_drag: TreeHandleDrag = Rc::new(move |_items: &[Dynamic], transfer: &Rc<DataTransfer>, _token: &CancellationToken| {
            transfer.set("text/plain", drag_original.clone());
            MaybePromise::Value(())
        });
        let handle_drop: TreeHandleDrop = Rc::new(move |_target: Option<&Dynamic>, transfer: &Rc<DataTransfer>, _token: &CancellationToken| {
            *dropped_by_handler.borrow_mut() = transfer.get("text/plain");
            MaybePromise::Promise(Promise::resolved(()))
        });
        let (handle, service) = prepare_drag_transfer(&env, handle_drag, Some(handle_drop)).await;
        env.test_object.rpc_handle_drag("dragTransferTree", vec![handle.clone()], "successful-drag", CancellationToken::None).await.expect("$handleDrag");
        env.test_object
            .rpc_handle_drop(
                "dragTransferTree",
                0.0,
                DataTransferDto { items: Vec::new() },
                None,
                CancellationToken::None,
                Some("successful-drag".to_owned()),
                Some("dragTransferTree".to_owned()),
                Some(vec![handle]),
            )
            .await
            .expect("$handleDrop");
        let original_item = dropped.borrow().as_ref().is_some_and(|dropped| Rc::ptr_eq(dropped, &original));
        assert_eq!((service.drag_operations().len(), original_item), (0, true));
    });
}

// ---- os casos da árvore ------------------------------------------------------------------------------

/// `testObject.$getChildren(id, [handle]).then(children => assert.strictEqual(unBatchChildren(children)?.length, 0))`.
fn assert_leaf(env: &Env, tree_id: &str, handle: &str) -> Promise<()> {
    let (env, tree_id, handle) = (env.clone(), tree_id.to_owned(), handle.to_owned());
    async_fn(async move {
        let children = env.get_children(&tree_id, Some(&[handle.as_str()])).await?;
        assert_eq!(un_batch_children(children).map(|children| children.len()), Some(0));
        Ok(())
    })
}

/// O corpo comum de `construct node tree` e `construct id tree`: `[a, b]` na raiz, os filhos de cada um e as folhas.
fn assert_constructed_tree(env: &Env, tree_id: &'static str, roots: [&'static str; 2], a_children: [&'static str; 2], b_children: [&'static str; 2]) -> Promise<()> {
    let env = env.clone();
    async_fn(async move {
        let elements = env.get_children(tree_id, None).await?;
        assert_eq!(handles_of(elements), strings(&roots));

        let branch = |parent: &'static str, children: [&'static str; 2]| {
            let env = env.clone();
            async_fn(async move {
                let found = env.get_children(tree_id, Some(&[parent])).await?;
                assert_eq!(handles_of(found), strings(&children));
                Promise::all(vec![assert_leaf(&env, tree_id, children[0]), assert_leaf(&env, tree_id, children[1])]).await?;
                Ok(())
            })
        };
        Promise::all(vec![branch(roots[0], a_children), branch(roots[1], b_children)]).await?;
        Ok(())
    })
}

#[test]
fn construct_node_tree() {
    run_test(|env| async move {
        assert_constructed_tree(&env, "testNodeTreeProvider", ["0/0:a", "0/0:b"], ["0/0:a/0:aa", "0/0:a/0:ab"], ["0/0:b/0:ba", "0/0:b/0:bb"]).await.expect("construct node tree");
    });
}

#[test]
fn construct_id_tree() {
    run_test(|env| async move {
        assert_constructed_tree(&env, "testNodeWithIdTreeProvider", ["1/a", "1/b"], ["1/aa", "1/ab"], ["1/ba", "1/bb"]).await.expect("construct id tree");
    });
}

#[test]
fn construct_highlights_tree() {
    run_test(|env| async move {
        const HIGHLIGHTS: [(f64, f64); 2] = [(0.0, 2.0), (3.0, 5.0)];
        let tree_id = "testNodeWithHighlightsTreeProvider";
        let elements = env.get_children(tree_id, None).await.expect("$getChildren");
        assert_deep_strict_equal(
            &remove_unset_keys(un_batch_children(elements)),
            &Dynamic::array([
                expected_item("1/a", None, "a", Some(&HIGHLIGHTS), TreeItemCollapsibleState::Collapsed),
                expected_item("1/b", None, "b", Some(&HIGHLIGHTS), TreeItemCollapsibleState::Collapsed),
            ]),
        );

        let branch = |parent: &'static str, first: &'static str, second: &'static str| {
            let env = env.clone();
            async_fn(async move {
                let children = env.get_children(tree_id, Some(&[parent])).await?;
                assert_deep_strict_equal(
                    &remove_unset_keys(un_batch_children(children)),
                    &Dynamic::array([
                        expected_item(first, Some(parent), &first[first.len() - 2..], Some(&HIGHLIGHTS), TreeItemCollapsibleState::None),
                        expected_item(second, Some(parent), &second[second.len() - 2..], Some(&HIGHLIGHTS), TreeItemCollapsibleState::None),
                    ]),
                );
                Ok(())
            })
        };
        Promise::all(vec![branch("1/a", "1/aa", "1/ab"), branch("1/b", "1/ba", "1/bb")]).await.expect("filhos");
    });
}

#[test]
fn duplicate_id_across_siblings_is_handled_gracefully() {
    run_test(|env| async move {
        let tree = env.fixture.tree.borrow().clone();
        set_key(&tree, "a", object(vec![("aa", object(vec![]))]));
        set_key(&tree, "b", object(vec![("aa", object(vec![])), ("ba", object(vec![]))]));
        let checking = env.clone();
        let done = after_refresh(&env, move |_| {
            let env = checking.clone();
            async_fn(async move {
                let elements = env.get_children("testNodeWithIdTreeProvider", None).await?;
                assert_eq!(handles_of(elements), strings(&["1/a", "1/b"]));
                env.get_children("testNodeWithIdTreeProvider", Some(&["1/a"])).await?;
                let elements = env.get_children("testNodeWithIdTreeProvider", Some(&["1/b"])).await?;
                // Children of 'b' should include both 'aa' and 'ba'
                assert_eq!(handles_of(elements), strings(&["1/aa", "1/ba"]));
                Ok(())
            })
        });
        env.fixture.on_did_change_tree_node.fire(Dynamic::Undefined);
        done.await.expect("duplicate id across siblings");
    });
}

#[test]
fn different_element_instances_with_same_id_are_replaced_gracefully() {
    run_test(|env| async move {
        // Simulates the race condition: two concurrent getChildren calls return
        // different element objects that map to the same tree item ID. The second
        // call should replace the first's registration without error.
        let call_count = Rc::new(Cell::new(0));
        let (element1, element2) = (new_key("x"), new_key("x"));
        let provider = Rc::new(TreeDataProvider {
            get_children: Rc::new(move |_element: Option<&Dynamic>| {
                call_count.set(call_count.get() + 1);
                // Return a different object instance each time
                MaybePromise::Value(Some(vec![if call_count.get() == 1 { element1.clone() } else { element2.clone() }]))
            }),
            get_tree_item: Rc::new(|element: &Dynamic| {
                MaybePromise::Value(object(vec![
                    ("label", object(vec![("label", Dynamic::string(&key_of(element)))])),
                    ("id", Dynamic::string("same-id")),
                    ("collapsibleState", Dynamic::from(TreeItemCollapsibleState::None.value())),
                ]))
            }),
            get_parent: None,
            resolve_tree_item: None,
            on_did_change_tree_data: Some(env.fixture.on_did_change_tree_node.event()),
        });
        let tree_view = env.test_object.create_tree_view("testRaceProvider", TreeViewOptions::new(provider), &env.extension).expect("createTreeView");
        env.store.add(tree_view);

        // First fetch: registers element1 with id 'same-id'
        let first = env.test_object.rpc_get_children("testRaceProvider", None).await.expect("$getChildren");
        let first_children = un_batch_children(first);
        assert_eq!(first_children.as_ref().map(Vec::len), Some(1));
        assert_eq!(first_children.expect("filhos")[0].handle, "1/same-id");

        // Second fetch: different element instance, same id. Should not throw.
        let second = env.test_object.rpc_get_children("testRaceProvider", None).await.expect("$getChildren");
        let second_children = un_batch_children(second);
        assert_eq!(second_children.as_ref().map(Vec::len), Some(1));
        assert_eq!(second_children.expect("filhos")[0].handle, "1/same-id");
    });
}

#[test]
fn refresh_root() {
    run_test(|env| async move {
        let done = after_refresh(&env, |actuals| {
            assert!(actuals.is_none());
            Promise::resolved(())
        });
        env.fixture.on_did_change_tree_node.fire(Dynamic::Undefined);
        done.await.expect("refresh root");
    });
}

#[test]
fn refresh_a_parent_node() {
    run_test(|env| async move {
        let done = after_refresh(&env, |actuals| {
            let actuals = actuals.as_ref().expect("itemsToRefresh");
            assert_eq!(keys_of(actuals), vec!["0/0:b"]);
            assert_deep_strict_equal(
                &item_at(actuals, "0/0:b").to_dynamic(),
                &expected_item("0/0:b", None, "b", None, TreeItemCollapsibleState::Collapsed),
            );
            Promise::resolved(())
        });
        fire_node(&env, "b");
        done.await.expect("refresh a parent node");
    });
}

#[test]
fn refresh_a_leaf_node() {
    run_test(|env| async move {
        let done = after_refresh(&env, |actuals| {
            let actuals = actuals.as_ref().expect("itemsToRefresh");
            assert_eq!(keys_of(actuals), vec!["0/0:b/0:bb"]);
            assert_deep_strict_equal(
                &item_at(actuals, "0/0:b/0:bb").to_dynamic(),
                &expected_item("0/0:b/0:bb", Some("0/0:b"), "bb", None, TreeItemCollapsibleState::None),
            );
            Promise::resolved(())
        });
        fire_node(&env, "bb");
        done.await.expect("refresh a leaf node");
    });
}

#[test]
fn refresh_parent_and_child_node_trigger_refresh_only_on_parent_scenario_1() {
    run_test(|env| async move {
        let merging = env.clone();
        run_with_event_merging(&env, |resolve| {
            merging.store.add(merging.target.on_refresh.event().listen(move |actuals| {
                let actuals = actuals.as_ref().expect("itemsToRefresh");
                assert_eq!(keys_of(actuals), vec!["0/0:b", "0/0:a/0:aa"]);
                assert_deep_strict_equal(
                    &item_at(actuals, "0/0:b").to_dynamic(),
                    &expected_item("0/0:b", None, "b", None, TreeItemCollapsibleState::Collapsed),
                );
                assert_deep_strict_equal(
                    &item_at(actuals, "0/0:a/0:aa").to_dynamic(),
                    &expected_item("0/0:a/0:aa", Some("0/0:a"), "aa", None, TreeItemCollapsibleState::None),
                );
                resolve.resolve(());
            }));
            fire_node(&merging, "b");
            fire_node(&merging, "aa");
            fire_node(&merging, "bb");
        })
        .await;
    });
}

#[test]
fn refresh_parent_and_child_node_trigger_refresh_only_on_parent_scenario_2() {
    run_test(|env| async move {
        let merging = env.clone();
        run_with_event_merging(&env, |resolve| {
            merging.store.add(merging.target.on_refresh.event().listen(move |actuals| {
                let actuals = actuals.as_ref().expect("itemsToRefresh");
                assert_eq!(keys_of(actuals), vec!["0/0:a/0:aa", "0/0:b"]);
                assert_deep_strict_equal(
                    &item_at(actuals, "0/0:b").to_dynamic(),
                    &expected_item("0/0:b", None, "b", None, TreeItemCollapsibleState::Collapsed),
                );
                assert_deep_strict_equal(
                    &item_at(actuals, "0/0:a/0:aa").to_dynamic(),
                    &expected_item("0/0:a/0:aa", Some("0/0:a"), "aa", None, TreeItemCollapsibleState::None),
                );
                resolve.resolve(());
            }));
            fire_node(&merging, "bb");
            fire_node(&merging, "aa");
            fire_node(&merging, "b");
        })
        .await;
    });
}

#[test]
fn refresh_an_element_for_label_change() {
    run_test(|env| async move {
        env.fixture.set_label("a", "aa");
        let done = after_refresh(&env, |actuals| {
            let actuals = actuals.as_ref().expect("itemsToRefresh");
            assert_eq!(keys_of(actuals), vec!["0/0:a"]);
            assert_deep_strict_equal(
                &item_at(actuals, "0/0:a").to_dynamic(),
                &expected_item("0/0:aa", None, "aa", None, TreeItemCollapsibleState::Collapsed),
            );
            Promise::resolved(())
        });
        fire_node(&env, "a");
        done.await.expect("refresh an element for label change");
    });
}

#[test]
fn refresh_calls_are_throttled_on_roots() {
    run_test(|env| async move {
        let merging = env.clone();
        run_with_event_merging(&env, |resolve| {
            merging.store.add(merging.target.on_refresh.event().listen(move |actuals| {
                assert!(actuals.is_none());
                resolve.resolve(());
            }));
            merging.fixture.on_did_change_tree_node.fire(Dynamic::Undefined);
            merging.fixture.on_did_change_tree_node.fire(Dynamic::Undefined);
            merging.fixture.on_did_change_tree_node.fire(Dynamic::Undefined);
            merging.fixture.on_did_change_tree_node.fire(Dynamic::Undefined);
        })
        .await;
    });
}

#[test]
fn refresh_calls_are_throttled_on_elements() {
    run_test(|env| async move {
        let merging = env.clone();
        run_with_event_merging(&env, |resolve| {
            merging.store.add(merging.target.on_refresh.event().listen(move |actuals| {
                let actuals = actuals.as_ref().expect("itemsToRefresh");
                assert_eq!(keys_of(actuals), vec!["0/0:a", "0/0:b"]);
                resolve.resolve(());
            }));

            fire_node(&merging, "a");
            fire_node(&merging, "b");
            fire_node(&merging, "b");
            fire_node(&merging, "a");
        })
        .await;
    });
}

#[test]
fn refresh_calls_are_throttled_on_unknown_elements() {
    run_test(|env| async move {
        let merging = env.clone();
        run_with_event_merging(&env, |resolve| {
            merging.store.add(merging.target.on_refresh.event().listen(move |actuals| {
                let actuals = actuals.as_ref().expect("itemsToRefresh");
                assert_eq!(keys_of(actuals), vec!["0/0:a", "0/0:b"]);
                resolve.resolve(());
            }));

            fire_node(&merging, "a");
            fire_node(&merging, "b");
            fire_node(&merging, "g");
            fire_node(&merging, "a");
        })
        .await;
    });
}

#[test]
fn refresh_calls_are_throttled_on_unknown_elements_and_root() {
    run_test(|env| async move {
        let merging = env.clone();
        run_with_event_merging(&env, |resolve| {
            merging.store.add(merging.target.on_refresh.event().listen(move |actuals| {
                assert!(actuals.is_none());
                resolve.resolve(());
            }));

            fire_node(&merging, "a");
            fire_node(&merging, "b");
            fire_node(&merging, "g");
            merging.fixture.on_did_change_tree_node.fire(Dynamic::Undefined);
        })
        .await;
    });
}

#[test]
fn refresh_calls_are_throttled_on_elements_and_root() {
    run_test(|env| async move {
        let merging = env.clone();
        run_with_event_merging(&env, |resolve| {
            merging.store.add(merging.target.on_refresh.event().listen(move |actuals| {
                assert!(actuals.is_none());
                resolve.resolve(());
            }));

            fire_node(&merging, "a");
            fire_node(&merging, "b");
            merging.fixture.on_did_change_tree_node.fire(Dynamic::Undefined);
            fire_node(&merging, "a");
        })
        .await;
    });
}

#[test]
fn generate_unique_handles_from_labels_by_escaping_them() {
    run_test(|env| async move {
        env.fixture.set_tree(object(vec![("a/0:b", object(vec![]))]));

        let checking = env.clone();
        let done = after_refresh(&env, move |_| {
            let env = checking.clone();
            async_fn(async move {
                let elements = env.get_children("testNodeTreeProvider", None).await?;
                assert_eq!(handles_of(elements), strings(&["0/0:a//0:b"]));
                Ok(())
            })
        });
        env.fixture.on_did_change_tree_node.fire(Dynamic::Undefined);
        done.await.expect("generate unique handles from labels by escaping them");
    });
}

#[test]
fn tree_with_duplicate_labels() {
    run_test(|env| async move {
        let adup1 = "c";
        let adup2 = "g";
        let bdup1 = "e";
        let hdup1 = "i";
        let hdup2 = "l";
        let jdup1 = "k";

        env.fixture.set_label("c", "a");
        env.fixture.set_label("e", "b");
        env.fixture.set_label("g", "a");
        env.fixture.set_label("i", "h");
        env.fixture.set_label("l", "h");
        env.fixture.set_label("k", "j");

        let tree = env.fixture.tree.borrow().clone();
        set_key(&tree, adup1, object(vec![]));
        set_key(&tree, "d", object(vec![]));

        let bdup1_tree = object(vec![]);
        set_key(&bdup1_tree, "h", object(vec![]));
        set_key(&bdup1_tree, hdup1, object(vec![]));
        set_key(&bdup1_tree, "j", object(vec![]));
        set_key(&bdup1_tree, jdup1, object(vec![]));
        set_key(&bdup1_tree, hdup2, object(vec![]));

        set_key(&tree, bdup1, bdup1_tree);
        set_key(&tree, "f", object(vec![]));
        set_key(&tree, adup2, object(vec![]));

        let checking = env.clone();
        let done = after_refresh(&env, move |_| {
            let env = checking.clone();
            async_fn(async move {
                let elements = env.get_children("testNodeTreeProvider", None).await?;
                assert_eq!(handles_of(elements), strings(&["0/0:a", "0/0:b", "0/1:a", "0/0:d", "0/1:b", "0/0:f", "0/2:a"]));
                let elements = env.get_children("testNodeTreeProvider", Some(&["0/1:b"])).await?;
                assert_eq!(handles_of(elements), strings(&["0/1:b/0:h", "0/1:b/1:h", "0/1:b/0:j", "0/1:b/1:j", "0/1:b/2:h"]));
                Ok(())
            })
        });

        env.fixture.on_did_change_tree_node.fire(Dynamic::Undefined);
        done.await.expect("tree with duplicate labels");
    });
}

#[test]
fn get_children_is_not_returned_from_cache_if_refreshed() {
    run_test(|env| async move {
        env.fixture.set_tree(object(vec![("c", object(vec![]))]));

        let checking = env.clone();
        let done = after_refresh(&env, move |_| {
            let env = checking.clone();
            async_fn(async move {
                let elements = env.get_children("testNodeTreeProvider", None).await?;
                assert_eq!(handles_of(elements), strings(&["0/0:c"]));
                Ok(())
            })
        });

        env.fixture.on_did_change_tree_node.fire(Dynamic::Undefined);
        done.await.expect("getChildren is not returned from cache if refreshed");
    });
}

#[test]
fn get_children_is_returned_from_cache_if_not_refreshed() {
    run_test(|env| async move {
        env.fixture.set_tree(object(vec![("c", object(vec![]))]));

        let elements = env.get_children("testNodeTreeProvider", None).await.expect("$getChildren");
        assert_eq!(handles_of(elements), strings(&["0/0:a", "0/0:b"]));
    });
}

#[test]
fn dispose_and_re_register_tree_view() {
    run_test(|env| async move {
        let dispose_tree_before = env.target.dispose_tree_calls.get();
        let register_before = env.target.register_calls.get();

        // Create, dispose, and re-register a tree view with the same id
        let tree_view1 = env.create_tree_view("reRegisterTreeProvider", ProviderKind::Node);
        tree_view1.dispose();
        let tree_view2 = env.create_tree_view("reRegisterTreeProvider", ProviderKind::Node);

        // Let all pending microtasks (the async dispose) settle
        timeout(0).await.expect("timeout");

        // The new view should work, $getChildren should return results, not reject
        let elements = env.test_object.rpc_get_children("reRegisterTreeProvider", None).await.expect("$getChildren");
        assert_eq!(handles_of(elements), strings(&["0/0:a", "0/0:b"]));

        // $registerTreeViewDataProvider should have been called twice (once per createTreeView)
        assert_eq!(env.target.register_calls.get() - register_before, 2);
        // $disposeTree should NOT have been called, the old async dispose should detect it was replaced
        assert_eq!(env.target.dispose_tree_calls.get() - dispose_tree_before, 0);

        tree_view2.dispose();
    });
}

#[test]
fn reveal_will_throw_an_error_if_get_parent_is_not_implemented() {
    run_test(|env| async move {
        let tree_view = env.create_tree_view("treeDataProvider", ProviderKind::Node);
        let result = tree_view.reveal(new_key("a"), None).await;
        assert!(result.is_err(), "Reveal should throw an error as getParent is not implemented");
    });
}

/// `revealTarget.args[0]`: o `$reveal` que o caso espera ter acontecido uma vez só (`calledOnce`) desde `start`.
fn single_reveal(env: &Env, start: usize) -> (String, Option<RevealItemInfo>, IRevealOptions) {
    let calls = env.target.reveal_calls.borrow();
    assert_eq!(calls.len() - start, 1, "o $reveal devia ter sido chamado uma vez");
    calls[start].clone()
}

/// `removeUnsetKeys(args[0][1])` de um `{ item, parentChain }`.
fn reveal_info_to_dynamic(info: &RevealItemInfo) -> Dynamic {
    object(vec![
        ("item", info.item.to_dynamic()),
        ("parentChain", Dynamic::array(info.parent_chain.iter().map(ITreeItem::to_dynamic))),
    ])
}

const DEFAULT_REVEAL_OPTIONS: IRevealOptions = IRevealOptions { select: Some(true), focus: Some(false), expand: Some(RevealExpand::Bool(false)) };

#[test]
fn reveal_will_return_empty_array_for_root_element() {
    run_test(|env| async move {
        let start = env.target.reveal_calls.borrow().len();
        let tree_view = env.create_tree_view("treeDataProvider", ProviderKind::Complete);
        let expected = object(vec![
            ("item", expected_item("0/0:a", None, "a", None, TreeItemCollapsibleState::Collapsed)),
            ("parentChain", Dynamic::array([])),
        ]);
        tree_view.reveal(new_key("a"), None).await.expect("reveal");
        let (view_id, item_info, options) = single_reveal(&env, start);
        assert_eq!(view_id, "treeDataProvider");
        assert_deep_strict_equal(&expected, &reveal_info_to_dynamic(&item_info.expect("itemInfo")));
        assert_eq!(options, DEFAULT_REVEAL_OPTIONS);
    });
}

#[test]
fn reveal_will_return_parents_array_for_an_element_when_hierarchy_is_not_loaded() {
    run_test(|env| async move {
        let start = env.target.reveal_calls.borrow().len();
        let tree_view = env.create_tree_view("treeDataProvider", ProviderKind::Complete);
        let expected_item_dynamic = expected_item("0/0:a/0:aa", Some("0/0:a"), "aa", None, TreeItemCollapsibleState::None);
        let expected_parent_chain = Dynamic::array([expected_item("0/0:a", None, "a", None, TreeItemCollapsibleState::Collapsed)]);
        tree_view.reveal(new_key("aa"), None).await.expect("reveal");
        let (view_id, item_info, options) = single_reveal(&env, start);
        let item_info = item_info.expect("itemInfo");
        assert_eq!(view_id, "treeDataProvider");
        assert_deep_strict_equal(&expected_item_dynamic, &item_info.item.to_dynamic());
        assert_deep_strict_equal(&expected_parent_chain, &Dynamic::array(item_info.parent_chain.iter().map(ITreeItem::to_dynamic)));
        assert_eq!(options, DEFAULT_REVEAL_OPTIONS);
    });
}

#[test]
fn reveal_will_return_parents_array_for_an_element_when_hierarchy_is_loaded() {
    run_test(|env| async move {
        let start = env.target.reveal_calls.borrow().len();
        let tree_view = env.create_tree_view("treeDataProvider", ProviderKind::Complete);
        let expected_item_dynamic = expected_item("0/0:a/0:aa", Some("0/0:a"), "aa", None, TreeItemCollapsibleState::None);
        let expected_parent_chain = Dynamic::array([expected_item("0/0:a", None, "a", None, TreeItemCollapsibleState::Collapsed)]);
        env.test_object.rpc_get_children("treeDataProvider", None).await.expect("$getChildren");
        env.test_object.rpc_get_children("treeDataProvider", Some(vec!["0/0:a".to_owned()])).await.expect("$getChildren");
        tree_view.reveal(new_key("aa"), None).await.expect("reveal");
        let (view_id, item_info, options) = single_reveal(&env, start);
        let item_info = item_info.expect("itemInfo");
        assert_eq!(view_id, "treeDataProvider");
        assert_deep_strict_equal(&expected_item_dynamic, &item_info.item.to_dynamic());
        assert_deep_strict_equal(&expected_parent_chain, &Dynamic::array(item_info.parent_chain.iter().map(ITreeItem::to_dynamic)));
        assert_eq!(options, DEFAULT_REVEAL_OPTIONS);
    });
}

#[test]
fn reveal_will_return_parents_array_for_deeper_element_with_no_selection() {
    run_test(|env| async move {
        env.fixture.set_tree(object(vec![("b", object(vec![("ba", object(vec![("bac", object(vec![]))]))]))]));
        let start = env.target.reveal_calls.borrow().len();
        let tree_view = env.create_tree_view("treeDataProvider", ProviderKind::Complete);
        let expected_item_dynamic = expected_item("0/0:b/0:ba/0:bac", Some("0/0:b/0:ba"), "bac", None, TreeItemCollapsibleState::None);
        let expected_parent_chain = Dynamic::array([
            expected_item("0/0:b", None, "b", None, TreeItemCollapsibleState::Collapsed),
            expected_item("0/0:b/0:ba", Some("0/0:b"), "ba", None, TreeItemCollapsibleState::Collapsed),
        ]);
        let options = IRevealOptions { select: Some(false), focus: Some(false), expand: Some(RevealExpand::Bool(false)) };
        tree_view.reveal(new_key("bac"), Some(options)).await.expect("reveal");
        let (view_id, item_info, received_options) = single_reveal(&env, start);
        let item_info = item_info.expect("itemInfo");
        assert_eq!(view_id, "treeDataProvider");
        assert_deep_strict_equal(&expected_item_dynamic, &item_info.item.to_dynamic());
        assert_deep_strict_equal(&expected_parent_chain, &Dynamic::array(item_info.parent_chain.iter().map(ITreeItem::to_dynamic)));
        assert_eq!(received_options, IRevealOptions { select: Some(false), focus: Some(false), expand: Some(RevealExpand::Bool(false)) });
    });
}

#[test]
fn reveal_after_first_udpate() {
    run_test(|env| async move {
        let start = env.target.reveal_calls.borrow().len();
        let tree_view = env.create_tree_view("treeDataProvider", ProviderKind::Complete);
        let expected_item_dynamic = expected_item("0/0:a/0:ac", Some("0/0:a"), "ac", None, TreeItemCollapsibleState::None);
        let expected_parent_chain = Dynamic::array([expected_item("0/0:a", None, "a", None, TreeItemCollapsibleState::Collapsed)]);
        env.load_complete_tree("treeDataProvider", None).await.expect("loadCompleteTree");
        env.fixture.set_tree(object(vec![
            ("a", object(vec![("aa", object(vec![])), ("ac", object(vec![]))])),
            ("b", object(vec![("ba", object(vec![])), ("bb", object(vec![]))])),
        ]));
        fire_node(&env, "a");

        tree_view.reveal(new_key("ac"), None).await.expect("reveal");
        let (view_id, item_info, options) = single_reveal(&env, start);
        let item_info = item_info.expect("itemInfo");
        assert_eq!(view_id, "treeDataProvider");
        assert_deep_strict_equal(&expected_item_dynamic, &item_info.item.to_dynamic());
        assert_deep_strict_equal(&expected_parent_chain, &Dynamic::array(item_info.parent_chain.iter().map(ITreeItem::to_dynamic)));
        assert_eq!(options, DEFAULT_REVEAL_OPTIONS);
    });
}

#[test]
fn reveal_after_second_udpate() {
    run_test(|env| async move {
        let start = env.target.reveal_calls.borrow().len();
        let tree_view = env.create_tree_view("treeDataProvider", ProviderKind::Complete);
        env.load_complete_tree("treeDataProvider", None).await.expect("loadCompleteTree");
        let changing = env.clone();
        run_with_event_merging(&env, |resolve| {
            changing.fixture.set_tree(object(vec![
                ("a", object(vec![("aa", object(vec![])), ("ac", object(vec![]))])),
                ("b", object(vec![("ba", object(vec![])), ("bb", object(vec![]))])),
            ]));
            fire_node(&changing, "a");
            changing.fixture.set_tree(object(vec![
                ("a", object(vec![("aa", object(vec![])), ("ac", object(vec![]))])),
                ("b", object(vec![("ba", object(vec![])), ("bc", object(vec![]))])),
            ]));
            fire_node(&changing, "b");
            resolve.resolve(());
        })
        .await;

        tree_view.reveal(new_key("bc"), None).await.expect("reveal");
        let (view_id, item_info, options) = single_reveal(&env, start);
        let item_info = item_info.expect("itemInfo");
        assert_eq!(view_id, "treeDataProvider");
        assert_deep_strict_equal(
            &expected_item("0/0:b/0:bc", Some("0/0:b"), "bc", None, TreeItemCollapsibleState::None),
            &item_info.item.to_dynamic(),
        );
        assert_deep_strict_equal(
            &Dynamic::array([expected_item("0/0:b", None, "b", None, TreeItemCollapsibleState::Collapsed)]),
            &Dynamic::array(item_info.parent_chain.iter().map(ITreeItem::to_dynamic)),
        );
        assert_eq!(options, DEFAULT_REVEAL_OPTIONS);
    });
}
