//! Porte de `src/vs/editor/contrib/hover/browser/getHover.ts`: a consulta aos provedores de hover
//! (`getHoverProviderResultsAsAsyncIterable` e `getHoversPromise`) e o resultado de cada um (`HoverProviderResult`).
//!
//! Decisões de tradução:
//! - `AsyncIterable<HoverProviderResult>` é o [`AsyncIterableProducer`] de `fromPromisesResolveOrder(...).coalesce()`, que
//!   o original também usa; o `for await` de `getHoversPromise` é [`for_await`].
//! - `executeProvider` (`async function`) é a promessa de [`execute_provider`]: a rejeição do provedor vai para
//!   `onUnexpectedExternalError` e vira `None` (`.catch(onUnexpectedExternalError)` devolve `undefined`).
//! - `isValid(result)`: `range` presente e `contents` não vazio (`typeof result.contents !== 'undefined' && result.contents
//!   && result.contents.length > 0`; `contents` nunca é `undefined` num `Vec`).
//! - Os comandos `_executeHoverProvider` e `_executeHoverProvider_recursive` (`registerModelAndPositionCommand`)
//!   dependem de `ServicesAccessor` e do `IModelService`: ficam de fora.

use std::ops::ControlFlow;
use std::rc::Rc;

use je_base::async_::{AsyncIterableProducer, async_fn, for_await};
use je_base::cancellation::CancellationToken;
use je_base::errors::on_unexpected_external_error;
use je_base::promise::Promise;

use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_features_service::{Hover, HoverProvider};
use crate::position::Position;
use crate::text_model::TextModel;

/// `HoverProviderResult`.
#[derive(Clone)]
pub struct HoverProviderResult {
    pub provider: Rc<dyn HoverProvider>,
    pub hover: Hover,
    pub ordinal: usize,
}

/// `isValid(result)`.
fn is_valid(result: &Hover) -> bool {
    result.range.is_some() && !result.contents.is_empty()
}

/// `executeProvider(provider, ordinal, model, position, token)`: não lança nem devolve uma promessa rejeitada (devolve
/// `None`).
fn execute_provider(
    provider: &Rc<dyn HoverProvider>,
    ordinal: usize,
    model: &Rc<TextModel>,
    position: Position,
    token: &CancellationToken,
) -> Promise<Option<HoverProviderResult>> {
    let provider_for_result = Rc::clone(provider);
    provider.provide_hover(model, position, token, None).then_result(move |result| {
        let result = match result {
            Ok(result) => result,
            Err(err) => {
                on_unexpected_external_error(&err);
                None
            }
        };
        Ok(result.filter(is_valid).map(|hover| HoverProviderResult { provider: provider_for_result, hover, ordinal }))
    })
}

/// `getHoverProviderResultsAsAsyncIterable(registry, model, position, token, recursive = false)`.
pub fn get_hover_provider_results_as_async_iterable(
    registry: &LanguageFeatureRegistry<dyn HoverProvider>,
    model: &Rc<TextModel>,
    position: Position,
    token: &CancellationToken,
    recursive: bool,
) -> Rc<AsyncIterableProducer<HoverProviderResult>> {
    let providers = registry.ordered(model, recursive);
    let promises: Vec<Promise<Option<HoverProviderResult>>> =
        providers.iter().enumerate().map(|(index, provider)| execute_provider(provider, index, model, position, token)).collect();
    AsyncIterableProducer::from_promises_resolve_order(promises).coalesce()
}

/// `getHoversPromise(registry, model, position, token, recursive = false)`.
pub fn get_hovers_promise(
    registry: &LanguageFeatureRegistry<dyn HoverProvider>,
    model: &Rc<TextModel>,
    position: Position,
    token: &CancellationToken,
    recursive: bool,
) -> Promise<Vec<Hover>> {
    let iterable = get_hover_provider_results_as_async_iterable(registry, model, position, token, recursive);
    async_fn(async move {
        let mut out = Vec::new();
        for_await(&*iterable, |item| {
            out.push(item.hover);
            Ok(ControlFlow::Continue(()))
        })
        .await?;
        Ok(out)
    })
}
