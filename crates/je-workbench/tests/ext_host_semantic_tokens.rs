//! Porte de `src/vs/workbench/api/test/browser/extHostSemanticTokens.test.ts` (suíte `Extension host semantic tokens cache`), caso a caso e na mesma ordem.
//!
//! PORTADOS: 7 dos 7 casos do original, contando cada passada dos laços como um: `does not reuse tokens returned after
//! cancellation` e `does not reuse edits returned after cancellation` (o laço `for (const useEdits of [false, true])`), os quatro
//! `preserves active ... over RPC` e `releases canceled ... over RPC before cancellation reaches the extension host` (o laço
//! `useEdits` x `cancel`) e `uses active result IDs for edits until release`.
//!
//! FORA: nenhum.
//!
//! Notas de tradução:
//! - O `setup` (o host com o dublê de `MainThreadLanguageFeatures` que guarda o `handle`) está em
//!   `tests/support/ext_host_language_features_support.rs`; ver as notas dele. O laço do original vira uma função com os
//!   parâmetros do laço, chamada pelos `#[test]`.
//! - Os casos `over RPC` criam dois `RPCProtocol` reais ligados por `Emitter<VSBuffer>` e entregam as mensagens à mão com
//!   `deliver(messages, receiver)`. O [`QueuedExtHostLanguageFeatures`] (`tests/support/queued_rpc_support.rs`, incluído por
//!   `#[path]`) é o `mainThreadRpc.getProxy(ExtHostContext.ExtHostLanguageFeatures)` ligado ao `extHostRpc`: um
//!   `ExtHostLanguageFeaturesShape` com as duas filas de mensagens, o cancelamento como mensagem e os espiões
//!   (`provide.firstCall.returnValue`, `release.args`) que o `sinon.spy` fazia sobre o `extHost`. Ver as notas dele.
//! - `new MainThreadDocumentSemanticTokensProvider(proxy, handle, { tokenTypes: ['class'], tokenModifiers: [] }, undefined)` é
//!   `MainThreadDocumentSemanticTokensProvider::new` (a classe passou a ser `pub`, como no original, que a exporta).
//!   `new class extends mock<ITextModel>() { override readonly uri = resource; }` é um `TextModel` de verdade (vazio) com o
//!   `resource` como URI, porque o provedor do porte pede o modelo concreto e só lê o `uri`.
//! - `decodeSemanticTokensDto(encoded).id` é o `id` do `SemanticTokensDto` completo ou do delta, e `String(resultId)` é
//!   `resultId.to_string()`. O resultado esperado `{ resultId, edits: [] }` / `{ resultId, data: Uint32Array }` é o
//!   `SemanticTokensResult` do editor, e o `null` é `None`.
//! - O provedor do original recebe o token do host (`provideDocumentSemanticTokens: (_document, token) => ...`); os fechos do
//!   [`EditsSemanticTokensProvider`] recebem o token (os casos que não o usam o ignoram) e `providerToken` é um
//!   `Rc<RefCell<CancellationToken>>`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

mod support;

#[path = "support/queued_rpc_support.rs"]
mod queued_rpc_support;

#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::is_cancellation_error;
use je_base::promise::{DeferredPromise, MaybePromise};
use je_base::uri::URI;
use je_editor::language_features_service::{
    DocumentSemanticTokensProvider, SemanticTokens as EditorSemanticTokens, SemanticTokensEdits as EditorSemanticTokensEdits, SemanticTokensLegend as EditorSemanticTokensLegend,
    SemanticTokensResult,
};
use je_editor::semantic_tokens_dto::{SemanticTokensDto, decode_semantic_tokens_dto};
use je_workbench::api::browser::main_thread_language_features::MainThreadDocumentSemanticTokensProvider;
use je_workbench::api::common::ext_host_document_data::TextDocument;
use je_workbench::api::common::ext_host_language_features_api::{ApiDocumentSemanticTokensProvider, ApiProviderResult, ApiSemanticTokensResult};
use je_workbench::api::common::ext_host_protocol::ExtHostLanguageFeaturesShape;
use je_workbench::api::common::ext_host_types::{SemanticTokens, SemanticTokensEdits, SemanticTokensLegend};
use je_workbench::services::extensions::extensions::null_extension_description;
use queued_rpc_support::QueuedExtHostLanguageFeatures;
use support::ext_host_language_features_support::{ExtHostEnv, run_with_faked_timers, test_selector, timeout};
use test_text_model::create_text_model_with;

/// `const resource = URI.parse('test://semantic/example.ts')`.
fn resource() -> URI {
    URI::parse("test://semantic/example.ts", false)
}

/// `new SemanticTokens(new Uint32Array(data), resultId)`.
fn tokens_with_data(data: Vec<u32>, result_id: &str) -> SemanticTokens {
    SemanticTokens::new(data, Some(result_id.to_owned()))
}

/// `new SemanticTokens(new Uint32Array([0, 0, 1, 0, 0]), resultId)`.
fn tokens(result_id: &str) -> SemanticTokens {
    tokens_with_data(vec![0, 0, 1, 0, 0], result_id)
}

/// `new SemanticTokensEdits([], resultId)`.
fn edits(result_id: &str) -> ApiSemanticTokensResult {
    ApiSemanticTokensResult::Edits(SemanticTokensEdits::new(Vec::new(), Some(result_id.to_owned())))
}

type ProvideTokens = Box<dyn Fn(&CancellationToken) -> ApiProviderResult<SemanticTokens>>;
type ProvideEdits = Box<dyn Fn(&str, &CancellationToken) -> ApiProviderResult<ApiSemanticTokensResult>>;

/// O provedor do original: `provideDocumentSemanticTokens(_document, token)` e
/// `provideDocumentSemanticTokensEdits(_document, previousResultId, token)`.
struct EditsSemanticTokensProvider {
    provide_tokens: ProvideTokens,
    provide_edits: ProvideEdits,
}

impl ApiDocumentSemanticTokensProvider for EditsSemanticTokensProvider {
    fn provide_document_semantic_tokens(&self, _document: &TextDocument, token: CancellationToken) -> ApiProviderResult<SemanticTokens> {
        (self.provide_tokens)(&token)
    }

    fn has_provide_document_semantic_tokens_edits(&self) -> bool {
        true
    }

    fn provide_document_semantic_tokens_edits(&self, _document: &TextDocument, previous_result_id: &str, token: CancellationToken) -> ApiProviderResult<ApiSemanticTokensResult> {
        (self.provide_edits)(previous_result_id, &token)
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
                provide_tokens: Box::new(move |_token| {
                    tokens_requests.borrow_mut().push("full".to_owned());
                    if tokens_cancel_next.get() { MaybePromise::Promise(tokens_pending.clone()) } else { MaybePromise::Value(Some(tokens("active"))) }
                }),
                provide_edits: Box::new(move |previous_result_id, _token| {
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

/// `decodeSemanticTokensDto(buffer).id`.
fn dto_id(buffer: &VSBuffer) -> u32 {
    match decode_semantic_tokens_dto(buffer) {
        SemanticTokensDto::Full(full) => full.id,
        SemanticTokensDto::Delta(delta) => delta.id,
    }
}

/// O corpo de `${cancel ? 'releases canceled' : 'preserves active'} ${useEdits ? 'edits' : 'tokens'} over RPC${cancel ? ' before
/// cancellation reaches the extension host' : ''}`.
fn over_rpc(use_edits: bool, cancel: bool) {
    run_with_faked_timers(async move {
        let env = ExtHostEnv::new(&resource());
        let handle = env.handle();
        let pending_tokens: DeferredPromise<Option<SemanticTokens>> = DeferredPromise::new();
        let pending_edits: DeferredPromise<Option<ApiSemanticTokensResult>> = DeferredPromise::new();
        let requests: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let defer_result = Rc::new(Cell::new(false));
        let provider_token = Rc::new(RefCell::new(CancellationToken::None));
        let (tokens_requests, tokens_defer, tokens_provider_token, tokens_pending) =
            (Rc::clone(&requests), Rc::clone(&defer_result), Rc::clone(&provider_token), pending_tokens.promise.clone());
        let (edits_requests, edits_defer, edits_provider_token, edits_pending) =
            (Rc::clone(&requests), Rc::clone(&defer_result), Rc::clone(&provider_token), pending_edits.promise.clone());
        env.add(env.ext_host.register_document_semantic_tokens_provider(
            &null_extension_description(),
            &test_selector(),
            Rc::new(EditsSemanticTokensProvider {
                provide_tokens: Box::new(move |token| {
                    tokens_requests.borrow_mut().push("full".to_owned());
                    *tokens_provider_token.borrow_mut() = token.clone();
                    if tokens_defer.get() { MaybePromise::Promise(tokens_pending.clone()) } else { MaybePromise::Value(Some(tokens("active"))) }
                }),
                provide_edits: Box::new(move |previous_result_id, token| {
                    edits_requests.borrow_mut().push(previous_result_id.to_owned());
                    *edits_provider_token.borrow_mut() = token.clone();
                    if edits_defer.get() { MaybePromise::Promise(edits_pending.clone()) } else { MaybePromise::Value(Some(edits("updated"))) }
                }),
            }),
            legend(),
        ));
        let active = env
            .ext_host
            .rpc_provide_document_semantic_tokens(handle, &resource(), 0, CancellationToken::None)
            .await
            .expect("provideDocumentSemanticTokens")
            .expect("active");
        let active_id = dto_id(&active);
        let mut last_result_id: Option<String> = None;
        if use_edits {
            let previous = env
                .ext_host
                .rpc_provide_document_semantic_tokens(handle, &resource(), 0, CancellationToken::None)
                .await
                .expect("provideDocumentSemanticTokens")
                .expect("previous");
            last_result_id = Some(dto_id(&previous).to_string());
        }

        // const provide = sinon.spy(extHost, '$provideDocumentSemanticTokens'); const release = sinon.spy(extHost, '$releaseDocumentSemanticTokens');
        let ext_host: Rc<dyn ExtHostLanguageFeaturesShape> = env.ext_host.clone();
        let rpc = QueuedExtHostLanguageFeatures::new(ext_host);
        let provider = MainThreadDocumentSemanticTokensProvider::new(
            rpc.clone(),
            handle,
            EditorSemanticTokensLegend { token_types: vec!["class".to_owned()], token_modifiers: Vec::new() },
            None,
        );
        let model = create_text_model_with("", None, Default::default(), Some(resource()));
        env.add(model.clone());
        let cancellation = CancellationTokenSource::new(None);
        env.add(Rc::new(cancellation.clone()));
        defer_result.set(true);
        let request = provider.provide_document_semantic_tokens(&model, last_result_id.as_deref(), &cancellation.token());
        rpc.deliver_to_ext_host().await.expect("deliver");
        if cancel {
            cancellation.cancel();
        }
        assert_eq!([cancellation.token().is_cancellation_requested(), provider_token.borrow().is_cancellation_requested()], [cancel, false]);
        if use_edits {
            pending_edits.complete(Some(ApiSemanticTokensResult::Edits(SemanticTokensEdits::new(Vec::new(), Some("late".to_owned()))))).await.expect("complete");
        } else {
            pending_tokens.complete(Some(tokens_with_data(vec![0, 0, 2, 0, 0], "late"))).await.expect("complete");
        }
        let encoded = rpc.first_provide_return_value().await.expect("provideDocumentSemanticTokens").expect("encoded");
        let result_id = dto_id(&encoded);
        rpc.deliver_to_main_thread().await.expect("deliver");
        let result = request.await.expect("provideDocumentSemanticTokens");
        rpc.deliver_to_ext_host().await.expect("deliver");
        rpc.deliver_to_main_thread().await.expect("deliver");

        let expected_result = if cancel {
            None
        } else if use_edits {
            Some(SemanticTokensResult::Edits(EditorSemanticTokensEdits { result_id: Some(result_id.to_string()), edits: Vec::new() }))
        } else {
            Some(SemanticTokensResult::Tokens(EditorSemanticTokens { result_id: Some(result_id.to_string()), data: vec![0, 0, 2, 0, 0] }))
        };
        assert_eq!(result, expected_result);
        let expected_release_args: Vec<(u32, u32)> = if cancel { vec![(handle, result_id)] } else { Vec::new() };
        assert_eq!(rpc.release_args(), expected_release_args);
        defer_result.set(false);
        requests.borrow_mut().clear();
        env.ext_host.rpc_provide_document_semantic_tokens(handle, &resource(), result_id, CancellationToken::None).await.expect("provideDocumentSemanticTokens");
        env.ext_host.rpc_provide_document_semantic_tokens(handle, &resource(), active_id, CancellationToken::None).await.expect("provideDocumentSemanticTokens");
        let expected_requests: &[&str] = if cancel { &["full", "active"] } else { &["late", "active"] };
        assert_eq!(*requests.borrow(), expected_requests);
    });
}

#[test]
fn preserves_active_tokens_over_rpc() {
    over_rpc(false, false);
}

#[test]
fn releases_canceled_tokens_over_rpc_before_cancellation_reaches_the_extension_host() {
    over_rpc(false, true);
}

#[test]
fn preserves_active_edits_over_rpc() {
    over_rpc(true, false);
}

#[test]
fn releases_canceled_edits_over_rpc_before_cancellation_reaches_the_extension_host() {
    over_rpc(true, true);
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
                provide_tokens: Box::new(move |_token| {
                    tokens_requests.borrow_mut().push("full".to_owned());
                    MaybePromise::Value(Some(tokens("active")))
                }),
                provide_edits: Box::new(move |previous_result_id, _token| {
                    edits_requests.borrow_mut().push(previous_result_id.to_owned());
                    MaybePromise::Value(Some(edits("updated")))
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
