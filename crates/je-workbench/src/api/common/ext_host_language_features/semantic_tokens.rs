//! `SemanticTokensPreviousResult`, `DocumentSemanticTokensAdapter` e `DocumentRangeSemanticTokensAdapter` de
//! `extHostLanguageFeatures.ts`.
//!
//! Divergências:
//! - Os tipos "relaxados" (`RelaxedSemanticTokens`, `RelaxedSemanticTokensEdits`) e o `_fixProvidedSemanticTokens` que os
//!   converte em `Uint32Array` não existem: [`SemanticTokens`] e [`SemanticTokensEdits`] já têm `Vec<u32>`.
//! - `_isSemanticTokens` e `_isSemanticTokensEdits` são o `match` no [`ApiSemanticTokensResult`].
//! - O `subarray` de `_convertToEdits` é uma cópia da fatia.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{async_fn, await_maybe};
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::range::Range as EditorRange;
use je_editor::semantic_tokens_dto::{DeltaSemanticTokensDto, FullSemanticTokensDto, SemanticTokensDelta, SemanticTokensDto, encode_semantic_tokens_dto};

use crate::api::common::ext_host_documents::ExtHostDocuments;
use crate::api::common::ext_host_language_features_api::{ApiDocumentRangeSemanticTokensProvider, ApiDocumentSemanticTokensProvider, ApiSemanticTokensResult};
use crate::api::common::ext_host_type_converters as type_convert;
use crate::api::common::ext_host_types::{SemanticTokensEdit, SemanticTokensEdits};

/// `SemanticTokensPreviousResult`.
#[derive(Clone)]
struct SemanticTokensPreviousResult {
    result_id: Option<String>,
    tokens: Option<Vec<u32>>,
}

/// `DocumentSemanticTokensAdapter`.
pub struct DocumentSemanticTokensAdapter {
    previous_results: RefCell<HashMap<u32, SemanticTokensPreviousResult>>,
    next_result_id: Cell<u32>,
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiDocumentSemanticTokensProvider>,
}

impl DocumentSemanticTokensAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiDocumentSemanticTokensProvider>) -> Rc<Self> {
        Rc::new(Self { previous_results: RefCell::new(HashMap::new()), next_result_id: Cell::new(1), documents, provider })
    }

    pub fn provide_document_semantic_tokens(self: &Rc<Self>, resource: URI, previous_result_id: u32, token: CancellationToken) -> Promise<Option<VSBuffer>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let previous_result = if previous_result_id != 0 { this.previous_results.borrow().get(&previous_result_id).cloned() } else { None };
            let previous_result_string_id = previous_result.as_ref().and_then(|previous| previous.result_id.clone());
            let value = match previous_result_string_id {
                Some(result_id) if this.provider.has_provide_document_semantic_tokens_edits() => {
                    await_maybe(this.provider.provide_document_semantic_tokens_edits(&doc, &result_id, token.clone())).await?
                }
                _ => await_maybe(this.provider.provide_document_semantic_tokens(&doc, token.clone())).await?.map(ApiSemanticTokensResult::Tokens),
            };

            if previous_result.is_some() {
                this.previous_results.borrow_mut().remove(&previous_result_id);
            }
            let Some(value) = value.filter(|_| !token.is_cancellation_requested()) else {
                return Ok(None);
            };
            Ok(this.send(Self::convert_to_edits(previous_result.as_ref(), &value), &value))
        })
    }

    pub fn release_document_semantic_coloring(&self, semantic_coloring_result_id: u32) {
        self.previous_results.borrow_mut().remove(&semantic_coloring_result_id);
    }

    fn convert_to_edits(previous_result: Option<&SemanticTokensPreviousResult>, new_result: &ApiSemanticTokensResult) -> ApiSemanticTokensResult {
        let ApiSemanticTokensResult::Tokens(new_tokens) = new_result else {
            return new_result.clone();
        };
        let Some(old_data) = previous_result.and_then(|previous| previous.tokens.as_ref()) else {
            return new_result.clone();
        };
        let old_length = old_data.len();
        let new_data = &new_tokens.data;
        let new_length = new_data.len();

        let mut common_prefix_length = 0;
        let max_common_prefix_length = old_length.min(new_length);
        while common_prefix_length < max_common_prefix_length && old_data[common_prefix_length] == new_data[common_prefix_length] {
            common_prefix_length += 1;
        }

        if common_prefix_length == old_length && common_prefix_length == new_length {
            // complete overlap!
            return ApiSemanticTokensResult::Edits(SemanticTokensEdits::new(Vec::new(), new_tokens.result_id.clone()));
        }

        let mut common_suffix_length = 0;
        let max_common_suffix_length = max_common_prefix_length - common_prefix_length;
        while common_suffix_length < max_common_suffix_length && old_data[old_length - common_suffix_length - 1] == new_data[new_length - common_suffix_length - 1] {
            common_suffix_length += 1;
        }

        ApiSemanticTokensResult::Edits(SemanticTokensEdits::new(
            vec![SemanticTokensEdit::new(
                common_prefix_length as f64,
                (old_length - common_prefix_length - common_suffix_length) as f64,
                Some(new_data[common_prefix_length..new_length - common_suffix_length].to_vec()),
            )],
            new_tokens.result_id.clone(),
        ))
    }

    fn next_id(&self) -> u32 {
        let id = self.next_result_id.get();
        self.next_result_id.set(id + 1);
        id
    }

    fn send(&self, value: ApiSemanticTokensResult, original: &ApiSemanticTokensResult) -> Option<VSBuffer> {
        match value {
            ApiSemanticTokensResult::Tokens(tokens) => {
                let my_id = self.next_id();
                self.previous_results.borrow_mut().insert(my_id, SemanticTokensPreviousResult { result_id: tokens.result_id.clone(), tokens: Some(tokens.data.clone()) });
                Some(encode_semantic_tokens_dto(&SemanticTokensDto::Full(FullSemanticTokensDto { id: my_id, data: tokens.data })))
            }
            ApiSemanticTokensResult::Edits(edits) => {
                let my_id = self.next_id();
                let previous = match original {
                    // store the original
                    ApiSemanticTokensResult::Tokens(original) => SemanticTokensPreviousResult { result_id: original.result_id.clone(), tokens: Some(original.data.clone()) },
                    ApiSemanticTokensResult::Edits(_) => SemanticTokensPreviousResult { result_id: edits.result_id.clone(), tokens: None },
                };
                self.previous_results.borrow_mut().insert(my_id, previous);
                let deltas = edits
                    .edits
                    .into_iter()
                    .map(|edit| SemanticTokensDelta { start: edit.start as u32, delete_count: edit.delete_count as u32, data: edit.data })
                    .collect();
                Some(encode_semantic_tokens_dto(&SemanticTokensDto::Delta(DeltaSemanticTokensDto { id: my_id, deltas })))
            }
        }
    }
}

/// `DocumentRangeSemanticTokensAdapter`.
pub struct DocumentRangeSemanticTokensAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiDocumentRangeSemanticTokensProvider>,
}

impl DocumentRangeSemanticTokensAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiDocumentRangeSemanticTokensProvider>) -> Rc<Self> {
        Rc::new(Self { documents, provider })
    }

    pub fn provide_document_range_semantic_tokens(self: &Rc<Self>, resource: URI, range: EditorRange, token: CancellationToken) -> Promise<Option<VSBuffer>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let value = await_maybe(this.provider.provide_document_range_semantic_tokens(&doc, &type_convert::Range::to(&range)?, token)).await?;
            Ok(value.map(|tokens| encode_semantic_tokens_dto(&SemanticTokensDto::Full(FullSemanticTokensDto { id: 0, data: tokens.data }))))
        })
    }
}
