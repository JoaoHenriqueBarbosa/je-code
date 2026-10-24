//! Porte de `src/vs/editor/contrib/parameterHints/browser/provideSignatureHelp.ts`: a pergunta aos provedores de
//! ajuda de assinatura (`provideSignatureHelp`), as chaves de contexto das dicas de parâmetro e o corpo do comando
//! `_executeSignatureHelpProvider`.
//!
//! Representação:
//! - `provideSignatureHelp` é uma função `async`: o corpo roda já, de forma síncrona, até o primeiro `await`
//!   ([`async_fn`]), então o primeiro provedor é chamado antes de a função devolver a promessa. Um erro do provedor (o
//!   `throw` síncrono é um pânico no porte, pego por [`errors::catch`]) é reportado por `onUnexpectedExternalError` e
//!   passa-se ao próximo provedor.
//! - `Context` é o [`ParameterHintsContext`] (as chaves de `RawContextKey` num `thread_local`, lidas por
//!   [`parameter_hints_context`]).
//! - `CommandsRegistry.registerCommand('_executeSignatureHelpProvider', ...)` registra o comando na carga do módulo e
//!   lê `ILanguageFeaturesService` e `ITextModelService`, que o porte não tem; o corpo do comando é
//!   [`execute_signature_help_provider`], que recebe o registro e o modelo já resolvidos (os `assertType` dos argumentos
//!   são o tipo dos parâmetros; `Position.lift` é a identidade).

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, on_unexpected_external_error};
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::runtime;
use je_platform::contextkey::{ContextKeyMeta, RawContextKey};

use super::languages::{SignatureHelp, SignatureHelpContext, SignatureHelpProvider, SignatureHelpResult, SignatureHelpTriggerKind};
use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::position::Position;
use crate::text_model::TextModel;

/// `Context`: as chaves de contexto das dicas de parâmetro.
#[derive(Clone)]
pub struct ParameterHintsContext {
    pub visible: RawContextKey,
    pub multiple_signatures: RawContextKey,
}

thread_local! {
    static CONTEXT: ParameterHintsContext = ParameterHintsContext {
        visible: RawContextKey::new("parameterHintsVisible", false, ContextKeyMeta::None),
        multiple_signatures: RawContextKey::new("parameterHintsMultipleSignatures", false, ContextKeyMeta::None),
    };
}

/// `Context`: as chaves de contexto das dicas de parâmetro (as mesmas a cada chamada, no thread).
pub fn parameter_hints_context() -> ParameterHintsContext {
    CONTEXT.with(Clone::clone)
}

/// `provideSignatureHelp(registry, model, position, context, token)`: o primeiro resultado que um provedor devolve,
/// na ordem do registro, ou `None`.
pub fn provide_signature_help(
    registry: &Rc<LanguageFeatureRegistry<dyn SignatureHelpProvider>>,
    model: &Rc<TextModel>,
    position: Position,
    context: SignatureHelpContext,
    token: &CancellationToken,
) -> Promise<Option<Rc<SignatureHelpResult>>> {
    let registry = Rc::clone(registry);
    let model = Rc::clone(model);
    let token = token.clone();
    async_fn(async move {
        let supports = registry.ordered(&model, false);

        for support in supports {
            let provided = match errors::catch(|| support.provide_signature_help(&model, position, &token, &context)) {
                Ok(provided) => provided.await,
                Err(error) => Err(error),
            };
            match provided {
                Ok(Some(result)) => return Ok(Some(result)),
                Ok(None) => {}
                Err(error) => on_unexpected_external_error(&error),
            }
        }
        Ok(None)
    })
}

/// O corpo do comando `_executeSignatureHelpProvider`: a ajuda de assinatura da posição, ou `None`. O resultado é
/// descartado no próximo ciclo do event loop (`setTimeout(() => result.dispose(), 0)`).
pub fn execute_signature_help_provider(
    registry: &Rc<LanguageFeatureRegistry<dyn SignatureHelpProvider>>,
    model: &Rc<TextModel>,
    position: Position,
    trigger_character: Option<String>,
) -> Promise<Option<Rc<SignatureHelp>>> {
    let registry = Rc::clone(registry);
    let model = Rc::clone(model);
    async_fn(async move {
        let trigger_character = trigger_character.filter(|character| !character.is_empty());
        let context = SignatureHelpContext {
            trigger_kind: SignatureHelpTriggerKind::Invoke,
            trigger_character,
            is_retrigger: false,
            active_signature_help: None,
        };
        let Some(result) = provide_signature_help(&registry, &model, position, context, &CancellationToken::None).await? else {
            return Ok(None);
        };

        let value = Rc::clone(&result.value);
        runtime::set_timeout(move || result.dispose(), 0);
        Ok(Some(value))
    })
}
