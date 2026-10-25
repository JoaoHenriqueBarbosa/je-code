//! Porte de `src/vs/editor/contrib/semanticTokens/common/getSemanticTokens.ts`: a busca dos tokens semânticos do documento
//! e de um intervalo nos provedores registrados.
//!
//! Representação:
//! - `SemanticTokens | SemanticTokensEdits` é o [`SemanticTokensResult`]; `isSemanticTokens` e `isSemanticTokensEdits`
//!   olham a variante (o tipo já garante o `data` e o `edits` que o original confere em tempo de execução).
//! - `result.error: unknown` é `Option<Error>`; o `throw result.error` é a promessa rejeitada com ele.
//! - Um provedor que lança de forma síncrona (um pânico de [`errors::throw`]) cai no mesmo `catch` que um que rejeita.
//!
//! Pendente: os comandos internos `_provideDocumentSemanticTokensLegend`, `_provideDocumentSemanticTokens`,
//! `_provideDocumentRangeSemanticTokensLegend` e `_provideDocumentRangeSemanticTokens` (registrados no
//! `CommandsRegistry` com o `accessor` de serviços, que o porte ainda não tem para o editor).

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::promise::Promise;

use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_features_service::{
    DocumentRangeSemanticTokensProvider, DocumentSemanticTokensProvider, SemanticTokens, SemanticTokensResult,
};
use crate::range::Range;
use crate::text_model::TextModel;

/// `isSemanticTokens(v)`.
pub fn is_semantic_tokens(value: &SemanticTokensResult) -> bool {
    matches!(value, SemanticTokensResult::Tokens(_))
}

/// `isSemanticTokensEdits(v)`.
pub fn is_semantic_tokens_edits(value: &SemanticTokensResult) -> bool {
    matches!(value, SemanticTokensResult::Edits(_))
}

/// `DocumentSemanticTokensResult`.
#[derive(Clone)]
pub struct DocumentSemanticTokensResult {
    pub provider: Rc<dyn DocumentSemanticTokensProvider>,
    pub tokens: Option<SemanticTokensResult>,
    pub error: Option<Error>,
}

/// `hasDocumentSemanticTokensProvider(registry, model)`.
pub fn has_document_semantic_tokens_provider(registry: &LanguageFeatureRegistry<dyn DocumentSemanticTokensProvider>, model: &TextModel) -> bool {
    registry.has(model)
}

fn get_document_semantic_tokens_providers(registry: &LanguageFeatureRegistry<dyn DocumentSemanticTokensProvider>, model: &TextModel) -> Vec<Rc<dyn DocumentSemanticTokensProvider>> {
    registry.ordered_groups(model).into_iter().next().unwrap_or_default()
}

/// `getDocumentSemanticTokens(registry, model, lastProvider, lastResultId, token)`.
pub fn get_document_semantic_tokens(
    registry: &LanguageFeatureRegistry<dyn DocumentSemanticTokensProvider>,
    model: &Rc<TextModel>,
    last_provider: Option<&Rc<dyn DocumentSemanticTokensProvider>>,
    last_result_id: Option<&str>,
    token: &CancellationToken,
) -> Promise<Option<DocumentSemanticTokensResult>> {
    let providers = get_document_semantic_tokens_providers(registry, model);

    // Get tokens from all providers at the same time.
    let results: Vec<Promise<DocumentSemanticTokensResult>> = providers
        .into_iter()
        .map(|provider| {
            let is_last_provider = last_provider.is_some_and(|last| std::ptr::addr_eq(Rc::as_ptr(last), Rc::as_ptr(&provider)));
            let result_id = if is_last_provider { last_result_id } else { None };
            match errors::catch(|| provider.provide_document_semantic_tokens(model, result_id, token)) {
                Err(error) => Promise::resolved(DocumentSemanticTokensResult { provider, tokens: None, error: Some(error) }),
                Ok(request) => request.then_result(move |result| match result {
                    Ok(tokens) => Ok(DocumentSemanticTokensResult { provider, tokens, error: None }),
                    Err(error) => Ok(DocumentSemanticTokensResult { provider, tokens: None, error: Some(error) }),
                }),
            }
        })
        .collect();

    Promise::all(results).then_result(|results| {
        let results = results?;
        // Try to return the first result with actual tokens or
        // the first result which threw an error (!!)
        for result in &results {
            if let Some(error) = &result.error {
                return Err(error.clone());
            }
            if result.tokens.is_some() {
                return Ok(Some(result.clone()));
            }
        }

        // Return the first result, even if it doesn't have tokens
        Ok(results.into_iter().next())
    })
}

/// `DocumentRangeSemanticTokensResult`.
#[derive(Clone)]
pub struct DocumentRangeSemanticTokensResult {
    pub provider: Rc<dyn DocumentRangeSemanticTokensProvider>,
    pub tokens: Option<SemanticTokens>,
}

/// `hasDocumentRangeSemanticTokensProvider(providers, model)`.
pub fn has_document_range_semantic_tokens_provider(providers: &LanguageFeatureRegistry<dyn DocumentRangeSemanticTokensProvider>, model: &TextModel) -> bool {
    providers.has(model)
}

fn get_document_range_semantic_tokens_providers(providers: &LanguageFeatureRegistry<dyn DocumentRangeSemanticTokensProvider>, model: &TextModel) -> Vec<Rc<dyn DocumentRangeSemanticTokensProvider>> {
    providers.ordered_groups(model).into_iter().next().unwrap_or_default()
}

/// `getDocumentRangeSemanticTokens(registry, model, range, token)`.
pub fn get_document_range_semantic_tokens(
    registry: &LanguageFeatureRegistry<dyn DocumentRangeSemanticTokensProvider>,
    model: &Rc<TextModel>,
    range: Range,
    token: &CancellationToken,
) -> Promise<Option<DocumentRangeSemanticTokensResult>> {
    let providers = get_document_range_semantic_tokens_providers(registry, model);

    // Get tokens from all providers at the same time.
    let results: Vec<Promise<DocumentRangeSemanticTokensResult>> = providers
        .into_iter()
        .map(|provider| match errors::catch(|| provider.provide_document_range_semantic_tokens(model, range, token)) {
            Err(error) => {
                errors::on_unexpected_external_error(&error);
                Promise::resolved(DocumentRangeSemanticTokensResult { provider, tokens: None })
            }
            Ok(request) => request.then_result(move |result| {
                let tokens = match result {
                    Ok(tokens) => tokens,
                    Err(error) => {
                        errors::on_unexpected_external_error(&error);
                        None
                    }
                };
                Ok(DocumentRangeSemanticTokensResult { provider, tokens })
            }),
        })
        .collect();

    Promise::all(results).then_result(|results| {
        let results = results?;
        // Try to return the first result with actual tokens
        if let Some(result) = results.iter().find(|result| result.tokens.is_some()) {
            return Ok(Some(result.clone()));
        }

        // Return the first result, even if it doesn't have tokens
        Ok(results.into_iter().next())
    })
}
