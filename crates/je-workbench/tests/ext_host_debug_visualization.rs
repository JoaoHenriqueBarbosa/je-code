//! Porte de `src/vs/workbench/api/test/browser/extHostDebugVisualization.test.ts` (suíte `Extension host debug visualization registrations`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso, e o `store` é o `DisposableStore` dele.
//!   O `setup` assíncrono do mocha é o [`setup`] no começo do corpo de cada caso, que roda em `run_with_faked_timers` (o caso
//!   síncrono `allows a visualization provider to register again after disposal` também, para as microtarefas do construtor do
//!   serviço esvaziarem antes do descarte).
//! - O `extension` do original (`{ ...nullExtensionDescription, identifier, contributes: { debugVisualizers: [...] } }`) é o
//!   [`extension`]; o `context` é o `visualization_context("session")` do suporte.
//! - Os provedores literais do original (`{ provideDebugVisualization: () => ... }`) são o [`TestProvider`], com o contador de
//!   chamadas em um `Rc<Cell<_>>` (o `calls++` do fecho); a árvore usa o `TestTree` do suporte.
//! - `registerDebugVisualizationProvider` lança no original, então devolve `Result` no porte; o `store.add(...)` recebe o `unwrap()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::cancellation::CancellationToken;
use je_base::lifecycle::DisposableStore;
use je_base::promise::MaybePromise;
use je_extensions::manifest::{DebugVisualizationContribution, ExtensionContributions};
use je_extensions::scanner::ExtensionDescription;
use je_workbench::api::common::ext_host_debug_protocol::ExtHostDebugServiceShape;
use je_workbench::api::common::ext_host_debug_service::ExtHostDebugServiceBase;
use je_workbench::api::common::ext_host_debug_types::{DebugVisualization, DebugVisualizationContext, DebugVisualizationProvider, ProviderResult};
use support::ext_host_debug_support::{MockMainThreadDebugService, TestTree, create_debug_service, session_dto, tree_item, visualization_context, visualizer_extension};

/// `{ ...nullExtensionDescription, identifier: new ExtensionIdentifier('sample.visualizers'), contributes: { debugVisualizers: [{ id: 'visualizer', when: 'true' }] } }`.
fn extension() -> ExtensionDescription {
    let mut extension = visualizer_extension();
    extension.manifest.contributes = Some(ExtensionContributions {
        debug_visualizers: Some(vec![DebugVisualizationContribution { id: Some("visualizer".to_owned()), when: Some("true".to_owned()), ..DebugVisualizationContribution::default() }]),
        ..ExtensionContributions::default()
    });
    extension
}

/// `setup(async () => { ... await service.$acceptDebugSessionStarted({ id: 'session', ... }); })`.
async fn setup(store: &Rc<DisposableStore>) -> Rc<ExtHostDebugServiceBase> {
    let proxy = MockMainThreadDebugService::new(&[
        "$registerDebugTypes",
        "$sessionCached",
        "$registerDebugVisualizer",
        "$unregisterDebugVisualizer",
        "$registerDebugVisualizerTree",
        "$unregisterDebugVisualizerTree",
    ]);
    let service = create_debug_service(proxy, store);
    service.rpc_accept_debug_session_started(session_dto("session")).await.unwrap();
    service
}

/// `{ provideDebugVisualization: () => { calls++; return []; } }`.
struct TestProvider {
    calls: Rc<Cell<usize>>,
}

impl DebugVisualizationProvider for TestProvider {
    fn provide_debug_visualization(&self, _context: DebugVisualizationContext, _token: CancellationToken) -> ProviderResult<Vec<DebugVisualization>> {
        self.calls.set(self.calls.get() + 1);
        MaybePromise::Value(Some(Vec::new()))
    }
}

#[test]
fn does_not_call_a_disposed_visualization_provider() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let service = setup(&store).await;
        let calls = Rc::new(Cell::new(0));
        let registration = store.add(service.register_debug_visualization_provider(&extension(), "visualizer", Rc::new(TestProvider { calls: calls.clone() })).unwrap());
        service.rpc_provide_debug_visualizers("sample.visualizers", "visualizer", visualization_context("session"), CancellationToken::None).await.unwrap();
        registration.dispose();
        service.rpc_provide_debug_visualizers("sample.visualizers", "visualizer", visualization_context("session"), CancellationToken::None).await.unwrap();
        assert_eq!(calls.get(), 1);
    });
}

#[test]
fn allows_a_visualization_provider_to_register_again_after_disposal() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let service = setup(&store).await;
        let provider: Rc<dyn DebugVisualizationProvider> = Rc::new(TestProvider { calls: Rc::new(Cell::new(0)) });
        store.add(service.register_debug_visualization_provider(&extension(), "visualizer", provider.clone()).unwrap()).dispose();
        store.add(service.register_debug_visualization_provider(&extension(), "visualizer", provider).unwrap());
    });
}

#[test]
fn does_not_call_a_disposed_visualization_tree_provider() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        let service = setup(&store).await;
        let calls = Rc::new(Cell::new(0));
        let provider = TestTree {
            get_tree_item: Box::new({
                let calls = calls.clone();
                move || {
                    calls.set(calls.get() + 1);
                    MaybePromise::Value(Some(tree_item("value")))
                }
            }),
            get_children: Box::new(|| MaybePromise::Value(Some(Vec::new()))),
            edit_item: None,
        };
        let registration = store.add(service.register_debug_visualization_tree(&extension(), "tree", Rc::new(provider)).unwrap());
        service.rpc_get_visualizer_tree_item("sample.visualizers\0tree", visualization_context("session")).await.unwrap();
        registration.dispose();
        service.rpc_get_visualizer_tree_item("sample.visualizers\0tree", visualization_context("session")).await.unwrap();
        assert_eq!(calls.get(), 1);
    });
}
