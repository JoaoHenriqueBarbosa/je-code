//! Porte de `src/vs/workbench/api/test/browser/extHostSemanticTokens.test.ts` (suíte `Extension host semantic tokens cache`), caso a caso e na mesma ordem.
//!
//! PORTADOS (3 dos 7 casos do original, contando cada passada dos laços como um): `does not reuse tokens returned after
//! cancellation` e `does not reuse edits returned after cancellation` (o laço `for (const useEdits of [false, true])`) e
//! `uses active result IDs for edits until release`.
//!
//! FORA, e por quê (nenhum vira `#[test]` enquanto não puder ser fiel): os quatro casos `preserves active ... over RPC` e
//! `releases canceled ... over RPC before cancellation reaches the extension host` (o laço `useEdits` x `cancel`). Eles montam
//! dois `RPCProtocol` reais ligados por `Emitter<VSBuffer>` (`new RPCProtocol({ onMessage, send })`), registram o host no
//! protocolo, espionam `extHost.$provideDocumentSemanticTokens` e `$releaseDocumentSemanticTokens` com `sinon.spy` e criam
//! o `MainThreadDocumentSemanticTokensProvider` com `mainThreadRpc.getProxy(...)`. Nada disso existe no porte: não há
//! `RPCProtocol`, a classe `MainThreadDocumentSemanticTokensProvider` é `pub(super)` em
//! `src/api/browser/main_thread_language_features/providers.rs` e não há dublê de `ExtHostLanguageFeaturesShape` (cerca de 75
//! métodos) que repasse as chamadas e as registre.
//!
//! Notas de tradução: o `setup` (o host com o dublê de `MainThreadLanguageFeatures` que guarda o `handle`) está em
//! `tests/support/ext_host_language_features_support.rs`; ver as notas dele. O laço do original vira uma função com o
//! `useEdits` como parâmetro, chamada pelos dois `#[test]`.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::is_cancellation_error;
use je_base::promise::{DeferredPromise, MaybePromise};
use je_base::uri::URI;
use je_workbench::api::common::ext_host_document_data::TextDocument;
use je_workbench::api::common::ext_host_language_features_api::{ApiDocumentSemanticTokensProvider, ApiProviderResult, ApiSemanticTokensResult};
use je_workbench::api::common::ext_host_protocol::ExtHostLanguageFeaturesShape;
use je_workbench::api::common::ext_host_types::{SemanticTokens, SemanticTokensEdits, SemanticTokensLegend};
use je_workbench::services::extensions::extensions::null_extension_description;
use support::ext_host_language_features_support::{ExtHostEnv, run_with_faked_timers, test_selector, timeout};

/// `const resource = URI.parse('test://semantic/example.ts')`.
fn resource() -> URI {
    URI::parse("test://semantic/example.ts", false)
}

/// `new SemanticTokens(new Uint32Array([0, 0, 1, 0, 0]), resultId)`.
fn tokens(result_id: &str) -> SemanticTokens {
    SemanticTokens::new(vec![0, 0, 1, 0, 0], Some(result_id.to_owned()))
}

type ProvideTokens = Box<dyn Fn() -> ApiProviderResult<SemanticTokens>>;
type ProvideEdits = Box<dyn Fn(&str) -> ApiProviderResult<ApiSemanticTokensResult>>;

/// O provedor do original: `provideDocumentSemanticTokens` e `provideDocumentSemanticTokensEdits(_document, previousResultId)`.
struct EditsSemanticTokensProvider {
    provide_tokens: ProvideTokens,
    provide_edits: ProvideEdits,
}

impl ApiDocumentSemanticTokensProvider for EditsSemanticTokensProvider {
    fn provide_document_semantic_tokens(&self, _document: &TextDocument, _token: CancellationToken) -> ApiProviderResult<SemanticTokens> {
        (self.provide_tokens)()
    }

    fn has_provide_document_semantic_tokens_edits(&self) -> bool {
        true
    }

    fn provide_document_semantic_tokens_edits(&self, _document: &TextDocument, previous_result_id: &str, _token: CancellationToken) -> ApiProviderResult<ApiSemanticTokensResult> {
        (self.provide_edits)(previous_result_id)
    }
}

/// `new SemanticTokensLegend(['class'])`.
fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend::new(vec!["class".to_owned()], None)
}

/// O corpo de `does not reuse ${useEdits ? 'edits' : 'tokens'} returned after cancellation`.
fn does_not_reuse_returned_after_cancellation(use_edits: bool) {
    run_with_faked_timers(async move {
        let env = ExtHostEnv::new(&resource());
        let pending_tokens: DeferredPromise<Option<SemanticTokens>> = DeferredPromise::new();
        let pending_edits: DeferredPromise<Option<ApiSemanticTokensResult>> = DeferredPromise::new();
        let requests: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let cancel_next = Rc::new(Cell::new(!use_edits));
        let (tokens_requests, tokens_cancel_next, tokens_pending) = (Rc::clone(&requests), Rc::clone(&cancel_next), pending_tokens.promise.clone());
        let (edits_requests, edits_cancel_next, edits_pending) = (Rc::clone(&requests), Rc::clone(&cancel_next), pending_edits.promise.clone());
        env.add(env.ext_host.register_document_semantic_tokens_provider(
            &null_extension_description(),
            &test_selector(),
            Rc::new(EditsSemanticTokensProvider {
                provide_tokens: Box::new(move || {
                    tokens_requests.borrow_mut().push("full".to_owned());
                    if tokens_cancel_next.get() { MaybePromise::Promise(tokens_pending.clone()) } else { MaybePromise::Value(Some(tokens("active"))) }
                }),
                provide_edits: Box::new(move |previous_result_id| {
                    edits_requests.borrow_mut().push(previous_result_id.to_owned());
                    if edits_cancel_next.get() { MaybePromise::Promise(edits_pending.clone()) } else { MaybePromise::Value(None) }
                }),
            }),
            legend(),
        ));
        if use_edits {
            env.ext_host.rpc_provide_document_semantic_tokens(env.handle(), &resource(), 0, CancellationToken::None).await.expect("provideDocumentSemanticTokens");
            cancel_next.set(true);
        }
        let cancellation = CancellationTokenSource::new(None);
        let request = env.ext_host.rpc_provide_document_semantic_tokens(env.handle(), &resource(), if use_edits { 1 } else { 0 }, cancellation.token());
        cancellation.cancel();
        let error = request.await.err().expect("o pedido cancelado deveria ser rejeitado");
        assert!(is_cancellation_error(&error), "{error}");
        if use_edits {
            pending_edits.complete(Some(ApiSemanticTokensResult::Edits(SemanticTokensEdits::new(Vec::new(), Some("late".to_owned()))))).await.expect("complete");
        } else {
            pending_tokens.complete(Some(tokens("late"))).await.expect("complete");
        }
        timeout(0).await.expect("timeout");
        cancel_next.set(false);
        env.ext_host
            .rpc_provide_document_semantic_tokens(env.handle(), &resource(), if use_edits { 2 } else { 1 }, CancellationToken::None)
            .await
            .expect("provideDocumentSemanticTokens");
        let expected: &[&str] = if use_edits { &["full", "active", "full"] } else { &["full", "full"] };
        assert_eq!(*requests.borrow(), expected);
        cancellation.dispose_with(false);
    });
}

#[test]
fn does_not_reuse_tokens_returned_after_cancellation() {
    does_not_reuse_returned_after_cancellation(false);
}

#[test]
fn does_not_reuse_edits_returned_after_cancellation() {
    does_not_reuse_returned_after_cancellation(true);
}

#[test]
fn uses_active_result_ids_for_edits_until_release() {
    run_with_faked_timers(async move {
        let env = ExtHostEnv::new(&resource());
        let requests: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let (tokens_requests, edits_requests) = (Rc::clone(&requests), Rc::clone(&requests));
        env.add(env.ext_host.register_document_semantic_tokens_provider(
            &null_extension_description(),
            &test_selector(),
            Rc::new(EditsSemanticTokensProvider {
                provide_tokens: Box::new(move || {
                    tokens_requests.borrow_mut().push("full".to_owned());
                    MaybePromise::Value(Some(tokens("active")))
                }),
                provide_edits: Box::new(move |previous_result_id| {
                    edits_requests.borrow_mut().push(previous_result_id.to_owned());
                    MaybePromise::Value(Some(ApiSemanticTokensResult::Edits(SemanticTokensEdits::new(Vec::new(), Some("updated".to_owned())))))
                }),
            }),
            legend(),
        ));
        env.ext_host.rpc_provide_document_semantic_tokens(env.handle(), &resource(), 0, CancellationToken::None).await.expect("provideDocumentSemanticTokens");
        env.ext_host.rpc_provide_document_semantic_tokens(env.handle(), &resource(), 1, CancellationToken::None).await.expect("provideDocumentSemanticTokens");
        env.ext_host.rpc_release_document_semantic_tokens(env.handle(), 2);
        env.ext_host.rpc_provide_document_semantic_tokens(env.handle(), &resource(), 2, CancellationToken::None).await.expect("provideDocumentSemanticTokens");
        assert_eq!(*requests.borrow(), ["full", "active", "full"]);
    });
}
