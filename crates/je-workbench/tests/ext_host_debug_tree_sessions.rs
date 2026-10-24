//! Porte de `src/vs/workbench/api/test/browser/extHostDebugTreeSessions.test.ts` (suíte `Extension host debug visualization session items`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `store` é o `DisposableStore` dele.
//!   O `setup` assíncrono do mocha é o [`setup`] no começo do corpo de cada caso, que roda em `run_with_faked_timers`.
//! - `DeferredPromise<vscode.DebugTreeItem>` é um `DeferredPromise` do `ProviderResult` (`Option<...>`) que o provedor devolve
//!   direto, sem `then` no caminho.
//! - `registerTree(overrides)` é o [`register_tree`] com o [`TreeOverrides`] (`Partial<vscode.DebugVisualizationTree>`). Os `{ label }`
//!   do original são `Rc<DebugTreeItem>` (a identidade do objeto dá o id do item).
//! - `for (const shared of [false, true]) test(...)` vira dois `#[test]` (`retains_separate_items...` e `retains_shared_items...`)
//!   sobre o mesmo corpo, [`retains_items_while_their_sessions_are_active`].
//! - `assert.ok(root)` é o `expect`; `assert.ok(first && second)` e `assert.ok(third && third.id !== second.id)` são `expect` mais
//!   a comparação dos ids.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::lifecycle::DisposableStore;
use je_base::promise::{DeferredPromise, MaybePromise};
use je_workbench::api::common::ext_host_debug_protocol::ExtHostDebugServiceShape;
use je_workbench::api::common::ext_host_debug_service::ExtHostDebugServiceBase;
use je_workbench::api::common::ext_host_debug_types::DebugTreeItem;
use support::ext_host_debug_support::{
    EditItem, GetChildren, GetTreeItem, MockMainThreadDebugService, TREE_ID, TestTree, create_debug_service, session_dto, tree_item, visualization_context,
    visualizer_extension,
};

/// `setup(async () => { ... await service.$acceptDebugSessionStarted(session('first')); })`.
async fn setup(store: &Rc<DisposableStore>) -> Rc<ExtHostDebugServiceBase> {
    let proxy = MockMainThreadDebugService::new(&["$registerDebugTypes", "$sessionCached", "$registerDebugVisualizerTree", "$unregisterDebugVisualizerTree"]);
    let service = create_debug_service(proxy, store);
    service.rpc_accept_debug_session_started(session_dto("first")).await.unwrap();
    service
}

/// `Partial<vscode.DebugVisualizationTree>`.
#[derive(Default)]
struct TreeOverrides {
    get_tree_item: Option<GetTreeItem>,
    get_children: Option<GetChildren>,
    edit_item: Option<EditItem>,
}

/// `registerTree(overrides)`.
fn register_tree(service: &ExtHostDebugServiceBase, store: &DisposableStore, overrides: TreeOverrides) {
    let provider = TestTree {
        get_tree_item: overrides.get_tree_item.unwrap_or_else(|| Box::new(|| MaybePromise::Value(Some(tree_item("root"))))),
        get_children: overrides.get_children.unwrap_or_else(|| Box::new(|| MaybePromise::Value(Some(vec![tree_item("child")])))),
        edit_item: Some(overrides.edit_item.unwrap_or_else(|| Box::new(|item| MaybePromise::Value(Some(item))))),
    };
    store.add(service.register_debug_visualization_tree(&visualizer_extension(), "tree", Rc::new(provider)).unwrap());
}

#[test]
fn releases_roots_and_children_when_their_debug_session_ends() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let service = setup(&store).await;
        register_tree(&service, &store, TreeOverrides::default());
        let root = service.rpc_get_visualizer_tree_item(TREE_ID, visualization_context("first")).await.unwrap().expect("a raiz deveria existir");
        let child = service.rpc_get_visualizer_tree_item_children(TREE_ID, root.id).await.unwrap().into_iter().next().expect("o filho deveria existir");
        service.rpc_accept_debug_session_terminated(session_dto("first")).await.unwrap();
        assert_eq!(service.rpc_edit_visualizer_tree_item(root.id, "value").await.unwrap(), None);
        assert_eq!(service.rpc_edit_visualizer_tree_item(child.id, "value").await.unwrap(), None);
    });
}

#[test]
fn does_not_cache_a_root_returned_after_its_session_ends() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let service = setup(&store).await;
        let result: Rc<DeferredPromise<Option<Rc<DebugTreeItem>>>> = Rc::new(DeferredPromise::new());
        register_tree(
            &service,
            &store,
            TreeOverrides {
                get_tree_item: Some(Box::new({
                    let result = result.clone();
                    move || MaybePromise::Promise(result.promise.clone())
                })),
                ..TreeOverrides::default()
            },
        );
        let request = service.rpc_get_visualizer_tree_item(TREE_ID, visualization_context("first"));
        service.rpc_accept_debug_session_terminated(session_dto("first")).await.unwrap();
        result.complete(Some(tree_item("late root"))).await.unwrap();
        assert_eq!(request.await.unwrap(), None);
    });
}

#[test]
fn does_not_cache_children_returned_after_their_session_ends() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let service = setup(&store).await;
        let result: Rc<DeferredPromise<Option<Vec<Rc<DebugTreeItem>>>>> = Rc::new(DeferredPromise::new());
        register_tree(
            &service,
            &store,
            TreeOverrides {
                get_children: Some(Box::new({
                    let result = result.clone();
                    move || MaybePromise::Promise(result.promise.clone())
                })),
                ..TreeOverrides::default()
            },
        );
        let root = service.rpc_get_visualizer_tree_item(TREE_ID, visualization_context("first")).await.unwrap().expect("a raiz deveria existir");
        let request = service.rpc_get_visualizer_tree_item_children(TREE_ID, root.id);
        service.rpc_accept_debug_session_terminated(session_dto("first")).await.unwrap();
        result.complete(Some(vec![tree_item("late child")])).await.unwrap();
        assert!(request.await.unwrap().is_empty());
    });
}

#[test]
fn does_not_cache_an_edit_returned_after_its_session_ends() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let service = setup(&store).await;
        let result: Rc<DeferredPromise<Option<Rc<DebugTreeItem>>>> = Rc::new(DeferredPromise::new());
        register_tree(
            &service,
            &store,
            TreeOverrides {
                edit_item: Some(Box::new({
                    let result = result.clone();
                    move |_| MaybePromise::Promise(result.promise.clone())
                })),
                ..TreeOverrides::default()
            },
        );
        let root = service.rpc_get_visualizer_tree_item(TREE_ID, visualization_context("first")).await.unwrap().expect("a raiz deveria existir");
        let request = service.rpc_edit_visualizer_tree_item(root.id, "value");
        service.rpc_accept_debug_session_terminated(session_dto("first")).await.unwrap();
        result.complete(Some(tree_item("late edit"))).await.unwrap();
        assert_eq!(request.await.unwrap(), None);
    });
}

/// O corpo de `retains ${shared ? 'shared' : 'separate'} items while their sessions are active`.
fn retains_items_while_their_sessions_are_active(shared: bool) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let service = setup(&store).await;
        let item = tree_item("root");
        register_tree(
            &service,
            &store,
            TreeOverrides {
                get_tree_item: Some(Box::new(move || MaybePromise::Value(Some(if shared { item.clone() } else { Rc::new((*item).clone()) })))),
                ..TreeOverrides::default()
            },
        );
        let first = service.rpc_get_visualizer_tree_item(TREE_ID, visualization_context("first")).await.unwrap();
        service.rpc_accept_debug_session_started(session_dto("second")).await.unwrap();
        let second = service.rpc_get_visualizer_tree_item(TREE_ID, visualization_context("second")).await.unwrap();
        let (first, second) = (first.expect("o primeiro item deveria existir"), second.expect("o segundo item deveria existir"));
        assert_eq!(first.id == second.id, shared);
        service.rpc_accept_debug_session_terminated(session_dto("first")).await.unwrap();
        assert!(service.rpc_edit_visualizer_tree_item(second.id, "value").await.unwrap().is_some());
        service.rpc_accept_debug_session_terminated(session_dto("second")).await.unwrap();
        assert_eq!(service.rpc_edit_visualizer_tree_item(second.id, "value").await.unwrap(), None);
        service.rpc_accept_debug_session_started(session_dto("third")).await.unwrap();
        let third = service.rpc_get_visualizer_tree_item(TREE_ID, visualization_context("third")).await.unwrap();
        assert!(third.is_some_and(|third| third.id != second.id));
    });
}

#[test]
fn retains_separate_items_while_their_sessions_are_active() {
    retains_items_while_their_sessions_are_active(false);
}

#[test]
fn retains_shared_items_while_their_sessions_are_active() {
    retains_items_while_their_sessions_are_active(true);
}
