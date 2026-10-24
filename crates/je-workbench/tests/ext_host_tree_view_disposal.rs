//! Porte de `src/vs/workbench/api/test/browser/extHostTreeViewDisposal.test.ts` (suíte `ExtHostTreeViews registration
//! lifetime`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado em [`run_test`]; os casos rodam em
//!   `run_with_faked_timers`, e o `setup` é [`Env::new`].
//! - `mock<MainThreadCommandsShape>()` com `override` e o `SingleProxyRPCProtocol` são [`SingleProxyContext`]: um método
//!   sem `override` falha com `panic!`, como o método ausente do objeto vazio lançaria `TypeError`. O
//!   `MainThreadTelemetry` vazio é o que o construtor de `ExtHostCommands` pede.
//! - `mock<MainThreadTreeViewsShape>()` é [`DisposalShape`]: `$registerTreeViewDataProvider` devolve a promessa corrente de
//!   `registration` e `$disposeTree` anota o id em `removed`.
//! - `WeakRef<Provider>` é `Weak<TreeDataProvider>`. O oráculo só roda o primeiro caso com `--expose-gc` (senão chama
//!   `this.skip()`); em Rust a coleta é o descarte determinístico do último `Rc`, então o caso roda sempre, sem o
//!   `globalThis.gc`.
//! - `registration.dispose()` de `registerTreeDataProvider` devolve a promessa do `dispose` assíncrono no original; aqui o
//!   `Disposable` é síncrono e o trabalho pendente termina na macrotarefa seguinte (`timeout(0)`).
//! - `service.$getChildren(...)` e `children?.[0][1]` (o primeiro item do primeiro lote) são [`first_label`].

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::RefCell;
use std::future::Future;
use std::rc::{Rc, Weak};

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, timeout};
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::IDisposable;
use je_base::promise::{DeferredPromise, MaybePromise, Promise};
use je_editor::languages::StringOrMarkdown;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::log::NullLogService;
use je_workbench::api::common::ext_host_commands::ExtHostCommands;
use je_workbench::api::common::ext_host_protocol::{
    CommandArgs, ExtHostTreeViewsShape, IExtHostTelemetry, IMainContext, ItemsToRefresh, MainThreadCommandsShape, MainThreadTelemetryShape,
    MainThreadTreeViewsShape, RevealItemInfo, TreeViewRegistrationOptions,
};
use je_workbench::api::common::ext_host_tree_views::{ExtHostTreeViews, TreeDataProvider, TreeViewHandle, TreeViewOptions};
use je_workbench::common::views::{IRevealOptions, IViewBadge};
use je_workbench::services::extensions::extensions::null_extension_description;

// ---- os dublês --------------------------------------------------------------------------------------

/// `new class extends mock<MainThreadCommandsShape>() { override $registerCommand(): void { } override $unregisterCommand(): void { } }`.
struct MockMainThreadCommands;

impl IDisposable for MockMainThreadCommands {
    fn dispose(&self) {}
}

impl MainThreadCommandsShape for MockMainThreadCommands {
    fn rpc_register_command(&self, _id: &str) {}

    fn rpc_unregister_command(&self, _id: &str) {}

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

/// `new class extends mock<IExtHostTelemetry>() { }`: nenhum caso chega a `onExtensionError`.
struct MockExtHostTelemetry;

impl IExtHostTelemetry for MockExtHostTelemetry {
    fn on_extension_error(&self, _extension: &ExtensionIdentifier, _error: &Error) -> bool {
        panic!("onExtensionError is not a function")
    }
}

/// `SingleProxyRPCProtocol(...)`: o proxy dos comandos, e o da telemetria que o construtor pede.
struct SingleProxyContext {
    commands: Rc<dyn MainThreadCommandsShape>,
    telemetry: Rc<dyn MainThreadTelemetryShape>,
}

impl IMainContext for SingleProxyContext {
    fn get_proxy_erased(&self, sid: &'static str) -> Rc<dyn Any> {
        match sid {
            "MainThreadCommands" => Rc::new(self.commands.clone()),
            "MainThreadTelemetry" => Rc::new(self.telemetry.clone()),
            other => panic!("proxy inesperado: {other}"),
        }
    }

    fn set_erased(&self, _sid: &'static str, _value: Rc<dyn Any>) {}
}

/// `new class extends mock<MainThreadTreeViewsShape>() { ... }`.
struct DisposalShape {
    removed: Rc<RefCell<Vec<String>>>,
    registration: Rc<RefCell<Promise<()>>>,
}

impl IDisposable for DisposalShape {
    fn dispose(&self) {}
}

impl MainThreadTreeViewsShape for DisposalShape {
    fn rpc_register_tree_view_data_provider(&self, _tree_view_id: &str, _options: TreeViewRegistrationOptions) -> Promise<()> {
        self.registration.borrow().clone()
    }

    fn rpc_refresh(&self, _view_id: &str, _items_to_refresh: Option<ItemsToRefresh>) -> Promise<()> {
        panic!("$refresh is not a function")
    }

    fn rpc_reveal(&self, _tree_view_id: &str, _item_info: Option<RevealItemInfo>, _options: IRevealOptions) -> Promise<()> {
        panic!("$reveal is not a function")
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

    fn rpc_dispose_tree(&self, tree_view_id: &str) -> Promise<()> {
        self.removed.borrow_mut().push(tree_view_id.to_owned());
        Promise::resolved(())
    }

    fn rpc_log_resolve_tree_node_failure(&self, _extension_id: &str) {
        panic!("$logResolveTreeNodeFailure is not a function")
    }
}

/// `class Provider implements TreeDataProvider<string>`: `getChildren()` devolve `['item']` e `getTreeItem(label)` devolve
/// `{ label }`.
fn new_provider() -> Rc<TreeDataProvider> {
    Rc::new(TreeDataProvider {
        get_children: Rc::new(|_element: Option<&Dynamic>| MaybePromise::Value(Some(vec![Dynamic::string("item")]))),
        get_tree_item: Rc::new(|label: &Dynamic| MaybePromise::Value(Dynamic::object([("label", label.clone())]))),
        get_parent: None,
        resolve_tree_item: None,
        on_did_change_tree_data: None,
    })
}

// ---- o ambiente de cada caso -----------------------------------------------------------------------

#[derive(Clone)]
struct Env {
    service: Rc<ExtHostTreeViews>,
    removed: Rc<RefCell<Vec<String>>>,
    registration: Rc<RefCell<Promise<()>>>,
}

impl Env {
    /// O `setup`.
    fn new(guard: &common::LeakGuard) -> Env {
        let removed = Rc::new(RefCell::new(Vec::new()));
        let registration = Rc::new(RefCell::new(Promise::resolved(())));
        let context = SingleProxyContext { commands: Rc::new(MockMainThreadCommands), telemetry: Rc::new(MockMainThreadTelemetry) };
        let commands = ExtHostCommands::new(&context, NullLogService::shared(), Rc::new(MockExtHostTelemetry));
        let shape = Rc::new(DisposalShape { removed: removed.clone(), registration: registration.clone() });
        let service = guard.add_rc(ExtHostTreeViews::new(shape, &commands, NullLogService::shared()));
        Env { service, removed, registration }
    }

    fn create_tree_view(&self, id: &str, provider: Rc<TreeDataProvider>) -> Rc<TreeViewHandle> {
        self.service.create_tree_view(id, TreeViewOptions::new(provider), &null_extension_description()).expect("createTreeView")
    }

    /// `async function disposeView(): Promise<WeakRef<Provider>>`.
    async fn dispose_view(&self) -> Weak<TreeDataProvider> {
        let provider = new_provider();
        let view = self.create_tree_view("disposed", provider.clone());
        view.dispose_async().await.expect("dispose");
        Rc::downgrade(&provider)
    }

    /// `const children = await service.$getChildren(id); const item = children?.[0][1]; item.label?.label` (o texto do
    /// rótulo do primeiro item, se o item for um objeto).
    async fn first_label(&self, id: &str) -> Option<String> {
        let children = self.service.rpc_get_children(id, None).await.expect("getChildren");
        let item = children?.into_iter().next()?.children.into_iter().next()?;
        match item.label?.label {
            StringOrMarkdown::String(label) => Some(label),
            StringOrMarkdown::Markdown(_) => None,
        }
    }

    fn removed(&self) -> Vec<String> {
        self.removed.borrow().clone()
    }
}

/// O corpo de um caso: o `setup`, o caso e o `teardown`.
fn run_test<Fut: Future<Output = ()> + 'static>(body: impl FnOnce(Env) -> Fut + 'static) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let env = Env::new(&_guard);
    run_with_faked_timers(async move {
        body(env).await;
    });
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

// ---- os casos --------------------------------------------------------------------------------------

#[test]
fn disposed_view_providers_become_collectible_while_the_service_remains_alive() {
    run_test(|env| async move {
        let reference = env.dispose_view().await;
        timeout(0).await.expect("timeout");
        assert!(reference.upgrade().is_none(), "Disposed tree view provider is still strongly retained");
    });
}

#[test]
fn disposing_one_view_preserves_other_live_providers() {
    run_test(|env| async move {
        env.create_tree_view("remaining", new_provider());
        env.dispose_view().await;
        let label = env.first_label("remaining").await;
        assert_eq!((env.removed(), label), (strings(&["disposed"]), Some("item".to_owned())));
    });
}

#[test]
fn registered_data_providers_can_still_be_disposed_repeatedly() {
    run_test(|env| async move {
        let registration = env.service.register_tree_data_provider("provider", new_provider(), &null_extension_description()).expect("registerTreeDataProvider");
        registration.dispose();
        timeout(0).await.expect("timeout");
        registration.dispose();
        timeout(0).await.expect("timeout");
        assert_eq!(env.removed(), strings(&["provider"]));
        assert!(env.service.rpc_get_children("provider", None).await.is_err());
    });
}

#[test]
fn shutdown_still_disposes_all_live_registrations() {
    run_test(|env| async move {
        env.create_tree_view("first", new_provider());
        env.create_tree_view("second", new_provider());
        env.service.dispose();
        timeout(0).await.expect("timeout");
        assert_eq!(env.removed(), strings(&["first", "second"]));
    });
}

#[test]
fn pending_old_view_disposal_does_not_remove_a_replacement() {
    run_test(|env| async move {
        let pending = DeferredPromise::<()>::new();
        *env.registration.borrow_mut() = pending.promise.clone();
        let old_view = env.create_tree_view("replaced", new_provider());
        let old_disposal = old_view.dispose_async();
        env.create_tree_view("replaced", new_provider());
        pending.complete(()).await.expect("complete");
        old_disposal.await.expect("dispose");
        let label = env.first_label("replaced").await;
        assert_eq!((env.removed(), label), (Vec::<String>::new(), Some("item".to_owned())));
    });
}
