//! Porte de `src/vs/workbench/api/test/browser/extHostHierarchy.test.ts` (suíte `Extension host hierarchy sessions`), caso a caso e na mesma ordem.
//!
//! PORTADOS: os 4 casos do original. Nenhum fica de fora.
//!
//! Notas de tradução: o `setup` (o host com o dublê de `MainThreadLanguageFeatures` que guarda o `handle`) está em
//! `tests/support/ext_host_language_features_support.rs`; ver as notas dele. `/missing call hierarchy item/` do
//! `assert.rejects` é a busca do texto na mensagem do erro. O `deepStrictEqual(..., [])` das chamadas é a lista vazia dentro do `Some`.

mod support;

use std::rc::Rc;

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::is_cancellation_error;
use je_base::promise::{DeferredPromise, MaybePromise};
use je_base::uri::URI;
use je_editor::position::Position;
use je_workbench::api::common::ext_host_document_data::TextDocument;
use je_workbench::api::common::ext_host_language_features_api::{ApiCallHierarchyProvider, ApiProviderResult, ApiTypeHierarchyProvider, OneOrMany};
use je_workbench::api::common::ext_host_protocol::ExtHostLanguageFeaturesShape;
use je_workbench::api::common::ext_host_types::{self as types, CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, SymbolKind, TypeHierarchyItem};
use je_workbench::services::extensions::extensions::null_extension_description;
use support::ext_host_language_features_support::{ExtHostEnv, run_with_faked_timers, test_selector, timeout};

/// `const resource = URI.parse('test://hierarchy/example.ts')`.
fn resource() -> URI {
    URI::parse("test://hierarchy/example.ts", false)
}

/// `const position = { lineNumber: 1, column: 1 }`.
fn position() -> Position {
    Position::new(1, 1)
}

/// `new Range(0, 0, 0, 1)`.
fn item_range() -> types::Range {
    types::Range::from_coordinates(0.0, 0.0, 0.0, 1.0).expect("Range")
}

fn call_item(kind: SymbolKind, name: &str) -> CallHierarchyItem {
    CallHierarchyItem::new(kind, name, "", resource(), item_range(), item_range())
}

fn type_item(kind: SymbolKind, name: &str) -> TypeHierarchyItem {
    TypeHierarchyItem::new(kind, name, "", resource(), item_range(), item_range())
}

/// `{ prepareCallHierarchy, provideCallHierarchyIncomingCalls: () => [], provideCallHierarchyOutgoingCalls: () => [] }`.
struct CallProvider(Box<dyn Fn() -> ApiProviderResult<OneOrMany<CallHierarchyItem>>>);

impl ApiCallHierarchyProvider for CallProvider {
    fn prepare_call_hierarchy(&self, _document: &TextDocument, _position: &types::Position, _token: CancellationToken) -> ApiProviderResult<OneOrMany<CallHierarchyItem>> {
        (self.0)()
    }

    fn provide_call_hierarchy_incoming_calls(&self, _item: &CallHierarchyItem, _token: CancellationToken) -> ApiProviderResult<Vec<CallHierarchyIncomingCall>> {
        MaybePromise::Value(Some(Vec::new()))
    }

    fn provide_call_hierarchy_outgoing_calls(&self, _item: &CallHierarchyItem, _token: CancellationToken) -> ApiProviderResult<Vec<CallHierarchyOutgoingCall>> {
        MaybePromise::Value(Some(Vec::new()))
    }
}

/// `{ prepareTypeHierarchy, provideTypeHierarchySupertypes: () => [], provideTypeHierarchySubtypes: () => [] }`.
struct TypeProvider(Box<dyn Fn() -> ApiProviderResult<OneOrMany<TypeHierarchyItem>>>);

impl ApiTypeHierarchyProvider for TypeProvider {
    fn prepare_type_hierarchy(&self, _document: &TextDocument, _position: &types::Position, _token: CancellationToken) -> ApiProviderResult<OneOrMany<TypeHierarchyItem>> {
        (self.0)()
    }

    fn provide_type_hierarchy_supertypes(&self, _item: &TypeHierarchyItem, _token: CancellationToken) -> ApiProviderResult<Vec<TypeHierarchyItem>> {
        MaybePromise::Value(Some(Vec::new()))
    }

    fn provide_type_hierarchy_subtypes(&self, _item: &TypeHierarchyItem, _token: CancellationToken) -> ApiProviderResult<Vec<TypeHierarchyItem>> {
        MaybePromise::Value(Some(Vec::new()))
    }
}

#[test]
fn does_not_cache_call_hierarchy_items_returned_after_cancellation() {
    run_with_faked_timers(async move {
        let env = ExtHostEnv::new(&resource());
        let pending: DeferredPromise<Option<OneOrMany<CallHierarchyItem>>> = DeferredPromise::new();
        let pending_promise = pending.promise.clone();
        env.add(env.ext_host.register_call_hierarchy_provider(
            &null_extension_description(),
            &test_selector(),
            Rc::new(CallProvider(Box::new(move || MaybePromise::Promise(pending_promise.clone())))),
        ));
        let cancellation = CancellationTokenSource::new(None);
        let request = env.ext_host.rpc_prepare_call_hierarchy(env.handle(), &resource(), position(), cancellation.token());
        cancellation.cancel();
        let error = request.await.err().expect("o pedido cancelado deveria ser rejeitado");
        assert!(is_cancellation_error(&error), "{error}");
        pending.complete(Some(OneOrMany::One(call_item(SymbolKind::Function, "canceled")))).await.expect("complete");
        timeout(0).await.expect("timeout");

        let error = env
            .ext_host
            .rpc_provide_call_hierarchy_incoming_calls(env.handle(), "1", "0", CancellationToken::None)
            .await
            .err()
            .expect("deveria ser rejeitado");
        assert!(error.message.contains("missing call hierarchy item"), "{}", error.message);
        cancellation.dispose_with(false);
    });
}

#[test]
fn does_not_cache_type_hierarchy_items_returned_after_cancellation() {
    run_with_faked_timers(async move {
        let env = ExtHostEnv::new(&resource());
        let pending: DeferredPromise<Option<OneOrMany<TypeHierarchyItem>>> = DeferredPromise::new();
        let pending_promise = pending.promise.clone();
        env.add(env.ext_host.register_type_hierarchy_provider(
            &null_extension_description(),
            &test_selector(),
            Rc::new(TypeProvider(Box::new(move || MaybePromise::Promise(pending_promise.clone())))),
        ));
        let cancellation = CancellationTokenSource::new(None);
        let request = env.ext_host.rpc_prepare_type_hierarchy(env.handle(), &resource(), position(), cancellation.token());
        cancellation.cancel();
        let error = request.await.err().expect("o pedido cancelado deveria ser rejeitado");
        assert!(is_cancellation_error(&error), "{error}");
        pending.complete(Some(OneOrMany::One(type_item(SymbolKind::Class, "canceled")))).await.expect("complete");
        timeout(0).await.expect("timeout");

        let error = env
            .ext_host
            .rpc_provide_type_hierarchy_supertypes(env.handle(), "1", "0", CancellationToken::None)
            .await
            .err()
            .expect("deveria ser rejeitado");
        assert!(error.message.contains("missing type hierarchy item"), "{}", error.message);
        cancellation.dispose_with(false);
    });
}

#[test]
fn keeps_an_active_call_hierarchy_session_until_release() {
    run_with_faked_timers(async move {
        let env = ExtHostEnv::new(&resource());
        env.add(env.ext_host.register_call_hierarchy_provider(
            &null_extension_description(),
            &test_selector(),
            Rc::new(CallProvider(Box::new(|| MaybePromise::Value(Some(OneOrMany::One(call_item(SymbolKind::Function, "active"))))))),
        ));
        let result = env.ext_host.rpc_prepare_call_hierarchy(env.handle(), &resource(), position(), CancellationToken::None).await.expect("prepareCallHierarchy");
        let item = result.expect("assert.ok(result)").remove(0);
        let calls = env
            .ext_host
            .rpc_provide_call_hierarchy_incoming_calls(env.handle(), &item.session_id, &item.item_id, CancellationToken::None)
            .await
            .expect("provideCallHierarchyIncomingCalls");
        assert!(matches!(&calls, Some(calls) if calls.is_empty()));
        env.ext_host.rpc_release_call_hierarchy(env.handle(), &item.session_id);
        let error = env
            .ext_host
            .rpc_provide_call_hierarchy_incoming_calls(env.handle(), &item.session_id, &item.item_id, CancellationToken::None)
            .await
            .err()
            .expect("deveria ser rejeitado");
        assert!(error.message.contains("missing call hierarchy item"), "{}", error.message);
    });
}

#[test]
fn keeps_an_active_type_hierarchy_session_until_release() {
    run_with_faked_timers(async move {
        let env = ExtHostEnv::new(&resource());
        env.add(env.ext_host.register_type_hierarchy_provider(
            &null_extension_description(),
            &test_selector(),
            Rc::new(TypeProvider(Box::new(|| MaybePromise::Value(Some(OneOrMany::One(type_item(SymbolKind::Class, "Active"))))))),
        ));
        let result = env.ext_host.rpc_prepare_type_hierarchy(env.handle(), &resource(), position(), CancellationToken::None).await.expect("prepareTypeHierarchy");
        let item = result.expect("assert.ok(result)").remove(0);
        let supertypes = env
            .ext_host
            .rpc_provide_type_hierarchy_supertypes(env.handle(), &item.session_id, &item.item_id, CancellationToken::None)
            .await
            .expect("provideTypeHierarchySupertypes");
        assert!(matches!(&supertypes, Some(supertypes) if supertypes.is_empty()));
        env.ext_host.rpc_release_type_hierarchy(env.handle(), &item.session_id);
        let error = env
            .ext_host
            .rpc_provide_type_hierarchy_supertypes(env.handle(), &item.session_id, &item.item_id, CancellationToken::None)
            .await
            .err()
            .expect("deveria ser rejeitado");
        assert!(error.message.contains("missing type hierarchy item"), "{}", error.message);
    });
}
