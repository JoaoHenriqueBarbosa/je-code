//! Porte de `src/vs/editor/contrib/format/browser/format.ts`, a parte que só consulta os provedores:
//! `getRealAndSyntheticDocumentFormattersOrdered`, `getDocumentRangeFormattingEditsUntilResult`,
//! `getDocumentFormattingEditsUntilResult` e `getOnTypeFormattingEdits`.
//!
//! Decisões de tradução:
//! - O formatador sintético de `getRealAndSyntheticDocumentFormattersOrdered` (o objeto literal que embrulha um
//!   formatador de intervalo) é o [`SyntheticDocumentFormatter`]; `displayName` e `extensionId` vêm do formatador de
//!   intervalo.
//! - `IEditorWorkerService` é o `Rc<dyn IEditorWorkerService>` e `ILanguageFeaturesService` é o
//!   [`LanguageFeaturesService`].
//! - `await Promise.resolve(provider.provideX(...)).catch(onUnexpectedExternalError)` é o `await` da promessa do trait,
//!   com a rejeição enviada a `onUnexpectedExternalError` e lida como `undefined`.
//! - `isNonEmptyArray(rawEdits)` é `Some` de uma lista que não está vazia.
//! - `TextEdit[] | null | undefined` e `TextEdit[] | undefined` são `Option<Vec<TextEdit>>`.
//! - Ficam de fora o que precisa de `ServicesAccessor`, do editor com DOM, do `IAccessibilitySignalService`, do
//!   `ITextModelService` ou do seletor de conflitos (`FormattingConflicts`, `formatDocumentRangesWithSelectedProvider`,
//!   `formatDocumentRangesWithProvider`, `formatDocumentWithSelectedProvider`, `formatDocumentWithProvider`,
//!   `getDocumentFormattingEditsWithSelectedProvider`, os três comandos `_executeFormat*Provider`).

use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::errors::on_unexpected_external_error;
use je_base::promise::Promise;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionIdentifierSet};

use crate::code_action::languages::TextEdit;
use crate::editor_worker::IEditorWorkerService;
use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_features_service::{
    DocumentFormattingEditProvider, DocumentRangeFormattingEditProvider, FormattingOptions, LanguageFeaturesService,
};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::TextModel;

/// O objeto literal que `getRealAndSyntheticDocumentFormattersOrdered` cria para cada formatador de intervalo.
struct SyntheticDocumentFormatter {
    formatter: Rc<dyn DocumentRangeFormattingEditProvider>,
}

impl DocumentFormattingEditProvider for SyntheticDocumentFormatter {
    fn extension_id(&self) -> Option<&ExtensionIdentifier> {
        self.formatter.extension_id()
    }

    fn display_name(&self) -> Option<&str> {
        self.formatter.display_name()
    }

    fn provide_document_formatting_edits(
        &self,
        model: &Rc<TextModel>,
        options: &FormattingOptions,
        token: &CancellationToken,
    ) -> crate::language_providers::ProviderResult<Vec<TextEdit>> {
        self.formatter.provide_document_range_formatting_edits(model, model.get_full_model_range(), options, token)
    }
}

/// `getRealAndSyntheticDocumentFormattersOrdered(documentFormattingEditProvider, documentRangeFormattingEditProvider, model)`.
pub fn get_real_and_synthetic_document_formatters_ordered(
    document_formatting_edit_provider: &LanguageFeatureRegistry<dyn DocumentFormattingEditProvider>,
    document_range_formatting_edit_provider: &LanguageFeatureRegistry<dyn DocumentRangeFormattingEditProvider>,
    model: &Rc<TextModel>,
) -> Vec<Rc<dyn DocumentFormattingEditProvider>> {
    let mut result: Vec<Rc<dyn DocumentFormattingEditProvider>> = Vec::new();
    let mut seen = ExtensionIdentifierSet::default();

    // (1) add all document formatter
    for formatter in document_formatting_edit_provider.ordered(model, false) {
        if let Some(extension_id) = formatter.extension_id() {
            seen.add(&extension_id.value);
        }
        result.push(formatter);
    }

    // (2) add all range formatter as document formatter (unless the same extension already did that)
    for formatter in document_range_formatting_edit_provider.ordered(model, false) {
        if let Some(extension_id) = formatter.extension_id() {
            if seen.has(&extension_id.value) {
                continue;
            }
            seen.add(&extension_id.value);
        }
        result.push(Rc::new(SyntheticDocumentFormatter { formatter }));
    }
    result
}

/// O `await Promise.resolve(...).catch(onUnexpectedExternalError)`: a rejeição vira `undefined`.
fn report_failure(result: Result<Option<Vec<TextEdit>>, je_base::errors::Error>) -> Option<Vec<TextEdit>> {
    result.unwrap_or_else(|err| {
        on_unexpected_external_error(&err);
        None
    })
}

/// `getDocumentRangeFormattingEditsUntilResult(workerService, languageFeaturesService, model, range, options, token)`.
pub fn get_document_range_formatting_edits_until_result(
    worker_service: &Rc<dyn IEditorWorkerService>,
    language_features_service: &LanguageFeaturesService,
    model: &Rc<TextModel>,
    range: Range,
    options: FormattingOptions,
    token: &CancellationToken,
) -> Promise<Option<Vec<TextEdit>>> {
    let providers = language_features_service.document_range_formatting_edit_provider.ordered(model, false);
    let (worker_service, model, token) = (Rc::clone(worker_service), Rc::clone(model), token.clone());
    async_fn(async move {
        for provider in providers {
            let raw_edits = report_failure(provider.provide_document_range_formatting_edits(&model, range, &options, &token).await);
            if raw_edits.as_ref().is_some_and(|edits| !edits.is_empty()) {
                return worker_service.compute_more_minimal_edits(&model.uri(), raw_edits).await;
            }
        }
        Ok(None)
    })
}

/// `getDocumentFormattingEditsUntilResult(workerService, languageFeaturesService, model, options, token)`.
pub fn get_document_formatting_edits_until_result(
    worker_service: &Rc<dyn IEditorWorkerService>,
    language_features_service: &LanguageFeaturesService,
    model: &Rc<TextModel>,
    options: FormattingOptions,
    token: &CancellationToken,
) -> Promise<Option<Vec<TextEdit>>> {
    let providers = get_real_and_synthetic_document_formatters_ordered(
        &language_features_service.document_formatting_edit_provider,
        &language_features_service.document_range_formatting_edit_provider,
        model,
    );
    let (worker_service, model, token) = (Rc::clone(worker_service), Rc::clone(model), token.clone());
    async_fn(async move {
        for provider in providers {
            let raw_edits = report_failure(provider.provide_document_formatting_edits(&model, &options, &token).await);
            if raw_edits.as_ref().is_some_and(|edits| !edits.is_empty()) {
                return worker_service.compute_more_minimal_edits(&model.uri(), raw_edits).await;
            }
        }
        Ok(None)
    })
}

/// `getOnTypeFormattingEdits(workerService, languageFeaturesService, model, position, ch, options, token)`.
pub fn get_on_type_formatting_edits(
    worker_service: &Rc<dyn IEditorWorkerService>,
    language_features_service: &LanguageFeaturesService,
    model: &Rc<TextModel>,
    position: Position,
    ch: &str,
    options: FormattingOptions,
    token: &CancellationToken,
) -> Promise<Option<Vec<TextEdit>>> {
    let providers = language_features_service.on_type_formatting_edit_provider.ordered(model, false);

    let Some(provider) = providers.first() else {
        return Promise::resolved(None);
    };

    if !provider.auto_format_trigger_characters().iter().any(|trigger| trigger.as_str() == ch) {
        return Promise::resolved(None);
    }

    let (worker_service, uri) = (Rc::clone(worker_service), model.uri());
    provider
        .provide_on_type_formatting_edits(model, position, ch, &options, token)
        .then_result_promise(move |result| worker_service.compute_more_minimal_edits(&uri, report_failure(result)))
}
