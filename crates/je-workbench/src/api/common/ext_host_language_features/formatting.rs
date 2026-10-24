//! `DocumentFormattingAdapter`, `RangeFormattingAdapter` e `OnTypeFormattingAdapter` de `extHostLanguageFeatures.ts`.
//!
//! Divergências:
//! - O `<any>options` que o original passa ao provedor é o [`FormattingOptions`] do protocolo.
//! - O `assertType(typeof provider.provideDocumentRangesFormattingEdits === 'function', ...)` é o `has_*` do trait, e a
//!   falha devolve o `Error` que o `assertType` lançaria.

use std::rc::Rc;

use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_action::languages::TextEdit as EditorTextEdit;
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;

use crate::api::common::ext_host_documents::ExtHostDocuments;
use crate::api::common::ext_host_language_features_api::{
    ApiDocumentFormattingEditProvider, ApiDocumentRangeFormattingEditProvider, ApiOnTypeFormattingEditProvider,
};
use crate::api::common::ext_host_protocol::FormattingOptions;
use crate::api::common::ext_host_type_converters as type_convert;
use crate::api::common::ext_host_types::TextEdit;

/// `value.map(typeConvert.TextEdit.from)` quando o provedor devolveu uma lista (`Array.isArray(value)`).
fn convert_edits(value: Option<Vec<TextEdit>>) -> Option<Vec<EditorTextEdit>> {
    value.map(|edits| edits.iter().map(type_convert::TextEdit::from).collect())
}

/// `DocumentFormattingAdapter`.
pub struct DocumentFormattingAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiDocumentFormattingEditProvider>,
}

impl DocumentFormattingAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiDocumentFormattingEditProvider>) -> Rc<Self> {
        Rc::new(Self { documents, provider })
    }

    pub fn provide_document_formatting_edits(self: &Rc<Self>, resource: URI, options: FormattingOptions, token: CancellationToken) -> Promise<Option<Vec<EditorTextEdit>>> {
        let this = self.clone();
        async_fn(async move {
            let document = this.documents.get_document(&resource)?;

            let value = await_maybe(this.provider.provide_document_formatting_edits(&document, &options, token)).await?;
            Ok(convert_edits(value))
        })
    }
}

/// `RangeFormattingAdapter`.
pub struct RangeFormattingAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiDocumentRangeFormattingEditProvider>,
}

impl RangeFormattingAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiDocumentRangeFormattingEditProvider>) -> Rc<Self> {
        Rc::new(Self { documents, provider })
    }

    pub fn provide_document_range_formatting_edits(
        self: &Rc<Self>,
        resource: URI,
        range: EditorRange,
        options: FormattingOptions,
        token: CancellationToken,
    ) -> Promise<Option<Vec<EditorTextEdit>>> {
        let this = self.clone();
        async_fn(async move {
            let document = this.documents.get_document(&resource)?;
            let ran = type_convert::Range::to(&range)?;

            let value = await_maybe(this.provider.provide_document_range_formatting_edits(&document, &ran, &options, token)).await?;
            Ok(convert_edits(value))
        })
    }

    pub fn provide_document_ranges_formatting_edits(
        self: &Rc<Self>,
        resource: URI,
        ranges: Vec<EditorRange>,
        options: FormattingOptions,
        token: CancellationToken,
    ) -> Promise<Option<Vec<EditorTextEdit>>> {
        let this = self.clone();
        async_fn(async move {
            if !this.provider.has_provide_document_ranges_formatting_edits() {
                return Err(Error::new("Unexpected type, expected 'INVALID invocation of `provideDocumentRangesFormattingEdits`'"));
            }

            let document = this.documents.get_document(&resource)?;
            let converted_ranges = ranges.iter().map(type_convert::Range::to).collect::<Result<Vec<_>, _>>()?;
            let value = await_maybe(this.provider.provide_document_ranges_formatting_edits(&document, &converted_ranges, &options, token)).await?;
            Ok(convert_edits(value))
        })
    }
}

/// `OnTypeFormattingAdapter`.
pub struct OnTypeFormattingAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiOnTypeFormattingEditProvider>,
}

impl OnTypeFormattingAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiOnTypeFormattingEditProvider>) -> Rc<Self> {
        Rc::new(Self { documents, provider })
    }

    pub fn provide_on_type_formatting_edits(
        self: &Rc<Self>,
        resource: URI,
        position: EditorPosition,
        ch: String,
        options: FormattingOptions,
        token: CancellationToken,
    ) -> Promise<Option<Vec<EditorTextEdit>>> {
        let this = self.clone();
        async_fn(async move {
            let document = this.documents.get_document(&resource)?;
            let pos = type_convert::Position::to(&position)?;

            let value = await_maybe(this.provider.provide_on_type_formatting_edits(&document, &pos, &ch, &options, token)).await?;
            Ok(convert_edits(value))
        })
    }
}
