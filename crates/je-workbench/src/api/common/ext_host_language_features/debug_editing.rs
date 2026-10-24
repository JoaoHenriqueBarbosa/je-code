//! `EvaluatableExpressionAdapter`, `InlineValuesAdapter` e `LinkedEditingRangeAdapter` de `extHostLanguageFeatures.ts`.
//!
//! Divergências:
//! - `LinkedEditingRangeAdapter.provideLinkedEditingRanges` devolve o `languages.LinkedEditingRanges` (com o `wordPattern`
//!   ainda `RegExp`): é o [`LinkedEditingRangesResult`], e quem serializa o `RegExp` é o `$provideLinkedEditingRanges`.
//! - `Array.isArray(value)` (valores embutidos) e `Array.isArray(value.ranges)` (edição ligada) são garantidos pelo tipo.

use std::rc::Rc;

use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;

use crate::api::common::ext_host_documents::ExtHostDocuments;
use crate::api::common::ext_host_language_features_api::{ApiEvaluatableExpressionProvider, ApiInlineValuesProvider, ApiLinkedEditingRangeProvider};
use crate::api::common::ext_host_language_features_converters::{EvaluatableExpression, InlineValue, InlineValueContext};
use crate::api::common::ext_host_protocol::{EvaluatableExpressionDto, IInlineValueContextDto, InlineValueDto};
use crate::api::common::ext_host_type_converters as type_convert;

/// `EvaluatableExpressionAdapter`.
pub struct EvaluatableExpressionAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiEvaluatableExpressionProvider>,
}

impl EvaluatableExpressionAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiEvaluatableExpressionProvider>) -> Rc<Self> {
        Rc::new(Self { documents, provider })
    }

    pub fn provide_evaluatable_expression(self: &Rc<Self>, resource: URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<EvaluatableExpressionDto>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let pos = type_convert::Position::to(&position)?;

            let value = await_maybe(this.provider.provide_evaluatable_expression(&doc, &pos, token)).await?;
            Ok(value.map(|value| EvaluatableExpression::from(&value)))
        })
    }
}

/// `InlineValuesAdapter`.
pub struct InlineValuesAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiInlineValuesProvider>,
}

impl InlineValuesAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiInlineValuesProvider>) -> Rc<Self> {
        Rc::new(Self { documents, provider })
    }

    pub fn provide_inline_values(
        self: &Rc<Self>,
        resource: URI,
        view_port: EditorRange,
        context: IInlineValueContextDto,
        token: CancellationToken,
    ) -> Promise<Option<Vec<InlineValueDto>>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let value = await_maybe(this.provider.provide_inline_values(&doc, &type_convert::Range::to(&view_port)?, &InlineValueContext::to(&context)?, token)).await?;
            Ok(value.map(|values| values.iter().map(InlineValue::from).collect()))
        })
    }
}

/// `languages.LinkedEditingRanges`: os intervalos e o `wordPattern` (um `RegExp`, como [`Dynamic`]).
#[derive(Clone)]
pub struct LinkedEditingRangesResult {
    pub ranges: Vec<EditorRange>,
    pub word_pattern: Option<Dynamic>,
}

/// `LinkedEditingRangeAdapter`.
pub struct LinkedEditingRangeAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiLinkedEditingRangeProvider>,
}

impl LinkedEditingRangeAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiLinkedEditingRangeProvider>) -> Rc<Self> {
        Rc::new(Self { documents, provider })
    }

    pub fn provide_linked_editing_ranges(self: &Rc<Self>, resource: URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<LinkedEditingRangesResult>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let pos = type_convert::Position::to(&position)?;

            let value = await_maybe(this.provider.provide_linked_editing_ranges(&doc, &pos, token)).await?;
            Ok(value.map(|value| LinkedEditingRangesResult {
                ranges: value.ranges.iter().map(type_convert::Range::from).collect(),
                word_pattern: value.word_pattern,
            }))
        })
    }
}
