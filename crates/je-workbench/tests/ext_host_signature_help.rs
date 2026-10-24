//! Porte de `src/vs/workbench/api/test/browser/extHostSignatureHelp.test.ts` (suíte `Extension host signature help cache`), caso a caso e na mesma ordem.
//!
//! PORTADOS: os 2 casos do original. Nenhum fica de fora.
//!
//! Notas de tradução: o `setup` (o host com o dublê de `MainThreadLanguageFeatures` que guarda o `handle`) está em
//! `tests/support/ext_host_language_features_support.rs`; ver as notas dele. O `provideSignatureHelp` do original devolve a
//! `DeferredPromise` ou `undefined`, e o `activeSignatureHelp` guardado é o `Rc` que o adaptador entrega ao provedor, comparado
//! por identidade (`assert.notStrictEqual` e `assert.strictEqual` são `Rc::ptr_eq`).

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::errors::is_cancellation_error;
use je_base::promise::{DeferredPromise, MaybePromise};
use je_base::uri::URI;
use je_editor::parameter_hints::languages::SignatureHelpTriggerKind;
use je_editor::position::Position;
use je_workbench::api::common::ext_host_document_data::TextDocument;
use je_workbench::api::common::ext_host_language_features_api::{
    ApiProviderResult, ApiSignatureHelpContext, ApiSignatureHelpProvider, ApiSignatureHelpRef, SignatureHelpMetadataOrTriggerChars,
};
use je_workbench::api::common::ext_host_protocol::{ExtHostLanguageFeaturesShape, ISignatureHelpContextDto, ISignatureHelpDto};
use je_workbench::api::common::ext_host_types::{self as types, SignatureHelp};
use je_workbench::services::extensions::extensions::null_extension_description;
use support::ext_host_language_features_support::{ExtHostEnv, run_with_faked_timers, test_selector, timeout};

/// `const resource = URI.parse('test://signature/example.ts')`.
fn resource() -> URI {
    URI::parse("test://signature/example.ts", false)
}

/// `const position = { lineNumber: 1, column: 1 }`.
fn position() -> Position {
    Position::new(1, 1)
}

/// `const context: ISignatureHelpContextDto = { triggerKind: SignatureHelpTriggerKind.Invoke, isRetrigger: false, triggerCharacter: undefined, activeSignatureHelp: undefined }`.
fn context() -> ISignatureHelpContextDto {
    ISignatureHelpContextDto { trigger_kind: SignatureHelpTriggerKind::Invoke, is_retrigger: false, trigger_character: None, active_signature_help: None }
}

/// O provedor do original: um objeto com `provideSignatureHelp(_document, _position, _token, context)`.
struct ContextSignatureHelpProvider(Box<dyn Fn(&ApiSignatureHelpContext) -> ApiProviderResult<ApiSignatureHelpRef>>);

impl ApiSignatureHelpProvider for ContextSignatureHelpProvider {
    fn provide_signature_help(
        &self,
        _document: &TextDocument,
        _position: &types::Position,
        _token: CancellationToken,
        context: &ApiSignatureHelpContext,
    ) -> ApiProviderResult<ApiSignatureHelpRef> {
        (self.0)(context)
    }
}

/// `new SignatureHelp()`.
fn new_signature_help() -> ApiSignatureHelpRef {
    Rc::new(RefCell::new(SignatureHelp::default()))
}

#[test]
fn does_not_reuse_signature_help_returned_after_cancellation() {
    run_with_faked_timers(async move {
        let env = ExtHostEnv::new(&resource());
        let pending: DeferredPromise<Option<ApiSignatureHelpRef>> = DeferredPromise::new();
        let late_result = new_signature_help();
        let active_signature_help: Rc<RefCell<Option<ApiSignatureHelpRef>>> = Rc::new(RefCell::new(None));
        let pending_promise = pending.promise.clone();
        let active_signature_help_seen = Rc::clone(&active_signature_help);
        env.add(env.ext_host.register_signature_help_provider(
            &null_extension_description(),
            &test_selector(),
            Rc::new(ContextSignatureHelpProvider(Box::new(move |context| {
                if !context.is_retrigger {
                    return MaybePromise::Promise(pending_promise.clone());
                }
                *active_signature_help_seen.borrow_mut() = context.active_signature_help.clone();
                MaybePromise::Value(None)
            }))),
            SignatureHelpMetadataOrTriggerChars::TriggerCharacters(Vec::new()),
        ));
        let cancellation = CancellationTokenSource::new(None);
        let request = env.ext_host.rpc_provide_signature_help(env.handle(), &resource(), position(), context(), cancellation.token());
        cancellation.cancel();
        let error = request.await.err().expect("o pedido cancelado deveria ser rejeitado");
        assert!(is_cancellation_error(&error), "{error}");
        pending.complete(Some(Rc::clone(&late_result))).await.expect("complete");
        timeout(0).await.expect("timeout");

        env.ext_host
            .rpc_provide_signature_help(
                env.handle(),
                &resource(),
                position(),
                ISignatureHelpContextDto {
                    is_retrigger: true,
                    active_signature_help: Some(ISignatureHelpDto { id: 1, signatures: Vec::new(), active_signature: 0, active_parameter: 0 }),
                    ..context()
                },
                CancellationToken::None,
            )
            .await
            .expect("provideSignatureHelp");
        assert!(!matches!(&*active_signature_help.borrow(), Some(active) if Rc::ptr_eq(active, &late_result)));
        cancellation.dispose_with(false);
    });
}

#[test]
fn reuses_active_signature_help_until_it_is_released() {
    run_with_faked_timers(async move {
        let env = ExtHostEnv::new(&resource());
        let result = new_signature_help();
        let active_signature_help: Rc<RefCell<Option<ApiSignatureHelpRef>>> = Rc::new(RefCell::new(None));
        let provided = Rc::clone(&result);
        let active_signature_help_seen = Rc::clone(&active_signature_help);
        env.add(env.ext_host.register_signature_help_provider(
            &null_extension_description(),
            &test_selector(),
            Rc::new(ContextSignatureHelpProvider(Box::new(move |context| {
                *active_signature_help_seen.borrow_mut() = context.active_signature_help.clone();
                MaybePromise::Value(if context.is_retrigger { None } else { Some(Rc::clone(&provided)) })
            }))),
            SignatureHelpMetadataOrTriggerChars::TriggerCharacters(Vec::new()),
        ));
        let first = env
            .ext_host
            .rpc_provide_signature_help(env.handle(), &resource(), position(), context(), CancellationToken::None)
            .await
            .expect("provideSignatureHelp")
            .expect("assert.ok(first)");
        let retrigger = ISignatureHelpContextDto { is_retrigger: true, active_signature_help: Some(first.clone()), ..context() };
        env.ext_host
            .rpc_provide_signature_help(env.handle(), &resource(), position(), retrigger.clone(), CancellationToken::None)
            .await
            .expect("provideSignatureHelp");
        assert!(matches!(&*active_signature_help.borrow(), Some(active) if Rc::ptr_eq(active, &result)));
        env.ext_host.rpc_release_signature_help(env.handle(), first.id);
        env.ext_host
            .rpc_provide_signature_help(env.handle(), &resource(), position(), retrigger, CancellationToken::None)
            .await
            .expect("provideSignatureHelp");
        assert!(!matches!(&*active_signature_help.borrow(), Some(active) if Rc::ptr_eq(active, &result)));
    });
}
