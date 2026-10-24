//! Porte de `src/vs/workbench/api/test/browser/extHostWorkspaceSymbols.test.ts` (suíte `Extension host workspace symbol cache`), caso a caso e na mesma ordem.
//!
//! PORTADOS: os 2 casos do original. Nenhum fica de fora.
//!
//! Notas de tradução: o `setup` (o host com o dublê de `MainThreadLanguageFeatures` que guarda o `handle`) está em
//! `tests/support/ext_host_language_features_support.rs`; ver as notas dele. O `resolveWorkspaceSymbol` do original empilha o
//! símbolo recebido em `resolved` e o devolve; aqui o provedor guarda uma cópia (o `SymbolInformation` tem `PartialEq`, e o
//! `assert.deepStrictEqual` é a comparação de valor).

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::is_cancellation_error;
use je_base::promise::{DeferredPromise, MaybePromise};
use je_base::uri::URI;
use je_editor::language_providers::SymbolKind as LanguageSymbolKind;
use je_editor::range::Range as EditorRange;
use je_workbench::api::common::ext_host_language_features_api::{ApiProviderResult, ApiWorkspaceSymbolProvider};
use je_workbench::api::common::ext_host_protocol::{ExtHostLanguageFeaturesShape, ILocationDto, IWorkspaceSymbolDto};
use je_workbench::api::common::ext_host_types::{self as types, SymbolInformation, SymbolKind};
use je_workbench::services::extensions::extensions::null_extension_description;
use support::ext_host_language_features_support::{ExtHostEnv, run_with_faked_timers, timeout};

/// `const resource = URI.parse('test://symbols/example.ts')`.
fn resource() -> URI {
    URI::parse("test://symbols/example.ts", false)
}

/// `new Range(new Position(0, 0), new Position(0, 0))`.
fn empty_range() -> types::Range {
    let origin = types::Position::new(0.0, 0.0).expect("Position");
    types::Range::new(origin, origin)
}

/// `{ name, kind: SymbolKind.Function, containerName: '', location: { uri: resource, range } }`.
fn function_symbol(name: &str) -> SymbolInformation {
    SymbolInformation {
        name: name.to_owned(),
        location: types::Location::new(Some(resource()), Some(types::RangeOrPosition::Range(empty_range()))),
        kind: SymbolKind::Function,
        tags: None,
        container_name: Some(String::new()),
    }
}

/// O provedor do original: `provideWorkspaceSymbols` e `resolveWorkspaceSymbol`, que empilha o símbolo recebido em `resolved`.
struct ResolvingWorkspaceSymbolProvider {
    provide: Box<dyn Fn() -> ApiProviderResult<Vec<SymbolInformation>>>,
    resolved: Rc<RefCell<Vec<SymbolInformation>>>,
}

impl ApiWorkspaceSymbolProvider for ResolvingWorkspaceSymbolProvider {
    fn provide_workspace_symbols(&self, _query: &str, _token: CancellationToken) -> ApiProviderResult<Vec<SymbolInformation>> {
        (self.provide)()
    }

    fn has_resolve_workspace_symbol(&self) -> bool {
        true
    }

    fn resolve_workspace_symbol(&self, symbol: &SymbolInformation, _token: CancellationToken) -> ApiProviderResult<SymbolInformation> {
        self.resolved.borrow_mut().push(symbol.clone());
        MaybePromise::Value(Some(symbol.clone()))
    }
}

#[test]
fn does_not_resolve_workspace_symbols_returned_after_cancellation() {
    run_with_faked_timers(async move {
        let env = ExtHostEnv::new(&resource());
        let pending: DeferredPromise<Option<Vec<SymbolInformation>>> = DeferredPromise::new();
        let resolved: Rc<RefCell<Vec<SymbolInformation>>> = Rc::new(RefCell::new(Vec::new()));
        let pending_promise = pending.promise.clone();
        env.add(env.ext_host.register_workspace_symbol_provider(
            &null_extension_description(),
            Rc::new(ResolvingWorkspaceSymbolProvider { provide: Box::new(move || MaybePromise::Promise(pending_promise.clone())), resolved: Rc::clone(&resolved) }),
        ));
        let cancellation = CancellationTokenSource::new(None);
        let request = env.ext_host.rpc_provide_workspace_symbols(env.handle(), "late", cancellation.token());
        cancellation.cancel();
        let error = request.await.err().expect("o pedido cancelado deveria ser rejeitado");
        assert!(is_cancellation_error(&error), "{error}");
        pending.complete(Some(vec![function_symbol("late")])).await.expect("complete");
        timeout(0).await.expect("timeout");

        // Each provider owns a fresh Cache whose IDs start at 1. Cancellation returns no DTO,
        // so probe the entry that would exist if the late result had been cached.
        env.ext_host
            .rpc_resolve_workspace_symbol(
                env.handle(),
                IWorkspaceSymbolDto {
                    name: "late".to_owned(),
                    kind: LanguageSymbolKind::Function,
                    location: ILocationDto { uri: Some(resource()), range: Some(EditorRange::new(1, 1, 1, 1)) },
                    cache_id: Some((1, 0)),
                    container_name: None,
                    tags: None,
                },
                CancellationToken::None,
            )
            .await
            .expect("resolveWorkspaceSymbol");
        assert_eq!(*resolved.borrow(), Vec::<SymbolInformation>::new());
        cancellation.dispose_with(false);
    });
}

#[test]
fn resolves_active_workspace_symbols_until_release() {
    run_with_faked_timers(async move {
        let env = ExtHostEnv::new(&resource());
        let symbol = function_symbol("active");
        let resolved: Rc<RefCell<Vec<SymbolInformation>>> = Rc::new(RefCell::new(Vec::new()));
        let provided = symbol.clone();
        env.add(env.ext_host.register_workspace_symbol_provider(
            &null_extension_description(),
            Rc::new(ResolvingWorkspaceSymbolProvider { provide: Box::new(move || MaybePromise::Value(Some(vec![provided.clone()]))), resolved: Rc::clone(&resolved) }),
        ));
        let result = env.ext_host.rpc_provide_workspace_symbols(env.handle(), "active", CancellationToken::None).await.expect("provideWorkspaceSymbols");
        let cache_id = result.cache_id.expect("assert.ok(result.cacheId !== undefined)");
        let item = result.symbols[0].clone();
        env.ext_host.rpc_resolve_workspace_symbol(env.handle(), item.clone(), CancellationToken::None).await.expect("resolveWorkspaceSymbol");
        env.ext_host.rpc_release_workspace_symbols(env.handle(), cache_id);
        env.ext_host.rpc_resolve_workspace_symbol(env.handle(), item, CancellationToken::None).await.expect("resolveWorkspaceSymbol");
        assert_eq!(*resolved.borrow(), vec![symbol]);
    });
}
