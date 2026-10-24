//! `SignatureHelpAdapter`, `InlayHintsAdapter`, `LinkProviderAdapter`, `ColorProviderAdapter`, `FoldingProviderAdapter` e
//! `SelectionRangeAdapter` de `extHostLanguageFeatures.ts`.
//!
//! Divergências:
//! - Os `console.log` e `console.warn` do original (`INVALID inlay hint, empty label`, `INVALID inlay hint, empty label
//!   part`) vão para o `trace` do serviço de log do adaptador; o de `LinkProviderAdapter._validateLink` (`DROPPING link
//!   because it is too long`) não tem logger na função estática do original e é omitido.
//! - O `JSON.stringify(ran)` dos logs de dicas embutidas é o objeto de quatro campos do `IRange`.
//! - `ISignatureHelpContextDto` carrega o `triggerKind` do editor; o `{ ...context, activeSignatureHelp }` do
//!   `reviveContext` o converte no enum de mesmo valor da API.
//! - O `SignatureHelp` guardado no cache é compartilhado (`Rc<RefCell<_>>`): o `reviveContext` altera `activeSignature` e
//!   `activeParameter` do objeto guardado, como o original.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::folding::syntax_range_provider::{FoldingContext, FoldingRange as EditorFoldingRange};
use je_editor::language_providers::SelectionRange as EditorSelectionRange;
use je_editor::parameter_hints::languages::SignatureHelp as EditorSignatureHelp;
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogServiceRef, Logger};

use crate::api::common::cache::Cache;
use crate::api::common::ext_host_documents::ExtHostDocuments;
use crate::api::common::ext_host_language_features_api::{
    ApiColorPresentationContext, ApiDocumentColorProvider, ApiDocumentLinkProvider, ApiFoldingRangeProvider, ApiInlayHintsProvider,
    ApiSelectionRangeProvider, ApiSignatureHelpContext, ApiSignatureHelpProvider, ApiSignatureHelpRef, DocumentLinkRef, InlayHintRef,
};
use crate::api::common::ext_host_language_features_converters::{Color, ColorPresentation, DocumentLink, FoldingRange, SelectionRange, SignatureHelp};
use crate::api::common::ext_host_protocol::{
    ChainedCacheId, IColorPresentationDto, IInlayHintDto, IInlayHintsDto, ILinkDto, ILinksListDto, IRawColorInfo, ISignatureHelpContextDto,
    ISignatureHelpDto, InlayHintLabelDto, InlayHintLabelPartDto,
};
use crate::api::common::ext_host_type_converters::{self as type_convert, ICommandsConverter, MarkdownString};
use crate::api::common::ext_host_types::{self as types, SignatureHelpTriggerKind};

/// `JSON.stringify(range)` de um `IRange`.
fn range_json(range: &EditorRange) -> String {
    format!(
        "{{\"startLineNumber\":{},\"startColumn\":{},\"endLineNumber\":{},\"endColumn\":{}}}",
        range.start_line_number, range.start_column, range.end_line_number, range.end_column
    )
}

/// `SignatureHelpAdapter`.
pub struct SignatureHelpAdapter {
    cache: Cache<ApiSignatureHelpRef>,
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiSignatureHelpProvider>,
}

impl SignatureHelpAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiSignatureHelpProvider>) -> Rc<Self> {
        Rc::new(Self { cache: Cache::new("SignatureHelp"), documents, provider })
    }

    pub fn provide_signature_help(
        self: &Rc<Self>,
        resource: URI,
        position: EditorPosition,
        context: ISignatureHelpContextDto,
        token: CancellationToken,
    ) -> Promise<Option<ISignatureHelpDto>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let pos = type_convert::Position::to(&position)?;
            let vscode_context = this.revive_context(&context);

            let value = await_maybe(this.provider.provide_signature_help(&doc, &pos, token.clone(), &vscode_context)).await?;
            if let Some(value) = value.filter(|_| !token.is_cancellation_requested()) {
                let id = this.cache.add(vec![value.clone()]);
                let help = SignatureHelp::from(&value.borrow());
                return Ok(Some(ISignatureHelpDto { id, signatures: help.signatures, active_signature: help.active_signature, active_parameter: help.active_parameter }));
            }
            Ok(None)
        })
    }

    fn revive_context(&self, context: &ISignatureHelpContextDto) -> ApiSignatureHelpContext {
        let mut active_signature_help: Option<ApiSignatureHelpRef> = None;
        if let Some(active) = &context.active_signature_help {
            let revived_signature_help = SignatureHelp::to(&EditorSignatureHelp {
                signatures: active.signatures.clone(),
                active_signature: active.active_signature,
                active_parameter: active.active_parameter,
            });
            let saved = self.cache.get(active.id, 0);
            active_signature_help = Some(match saved {
                Some(saved) => {
                    {
                        let mut saved_help = saved.borrow_mut();
                        saved_help.active_signature = revived_signature_help.active_signature;
                        saved_help.active_parameter = revived_signature_help.active_parameter;
                    }
                    saved
                }
                None => Rc::new(RefCell::new(revived_signature_help)),
            });
        }
        ApiSignatureHelpContext {
            trigger_kind: SignatureHelpTriggerKind::from_value(context.trigger_kind as i32).unwrap_or(SignatureHelpTriggerKind::Invoke),
            trigger_character: context.trigger_character.clone(),
            is_retrigger: context.is_retrigger,
            active_signature_help,
        }
    }

    pub fn release_signature_help(&self, id: u32) {
        self.cache.delete(id);
    }
}

/// `InlayHintsAdapter`.
pub struct InlayHintsAdapter {
    cache: Cache<InlayHintRef>,
    disposables: RefCell<HashMap<u32, Rc<DisposableStore>>>,
    documents: Rc<ExtHostDocuments>,
    commands: Rc<dyn ICommandsConverter>,
    provider: Rc<dyn ApiInlayHintsProvider>,
    log_service: LogServiceRef,
    extension: Rc<ExtensionDescription>,
}

impl InlayHintsAdapter {
    pub fn new(
        documents: Rc<ExtHostDocuments>,
        commands: Rc<dyn ICommandsConverter>,
        provider: Rc<dyn ApiInlayHintsProvider>,
        log_service: LogServiceRef,
        extension: Rc<ExtensionDescription>,
    ) -> Rc<Self> {
        Rc::new(Self { cache: Cache::new("InlayHints"), disposables: RefCell::new(HashMap::new()), documents, commands, provider, log_service, extension })
    }

    pub fn provide_inlay_hints(self: &Rc<Self>, resource: URI, ran: EditorRange, token: CancellationToken) -> Promise<Option<IInlayHintsDto>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let range = type_convert::Range::to(&ran)?;

            let hints = await_maybe(this.provider.provide_inlay_hints(&doc, &range, token.clone())).await?;
            let Some(hints) = hints.filter(|hints| !hints.is_empty()) else {
                // bad result
                this.log_service.trace(&format!("[InlayHints] NO inlay hints from '{}' for range {}", this.extension.identifier.value, range_json(&ran)));
                return Ok(None);
            };
            if token.is_cancellation_requested() {
                // cancelled -> return without further ado, esp no caching
                // of results as they will leak
                return Ok(None);
            }
            let pid = this.cache.add(hints.clone());
            this.disposables.borrow_mut().insert(pid, DisposableStore::new());
            let mut result = IInlayHintsDto { hints: Vec::new(), cache_id: Some(pid) };
            for (i, hint) in hints.iter().enumerate() {
                if this.is_valid_inlay_hint(&hint.borrow(), Some(&range)) {
                    result.hints.push(this.convert_inlay_hint(&hint.borrow(), (pid, i as u32))?);
                }
            }
            this.log_service.trace(&format!("[InlayHints] {} inlay hints from '{}' for range {}", result.hints.len(), this.extension.identifier.value, range_json(&ran)));
            Ok(Some(result))
        })
    }

    pub fn resolve_inlay_hint(self: &Rc<Self>, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<IInlayHintDto>> {
        let this = self.clone();
        async_fn(async move {
            if !this.provider.has_resolve_inlay_hint() {
                return Ok(None);
            }
            let Some(item) = this.cache.get(id.0, id.1 as usize) else {
                return Ok(None);
            };
            let Some(hint) = await_maybe(this.provider.resolve_inlay_hint(&item, token)).await? else {
                return Ok(None);
            };
            if !this.is_valid_inlay_hint(&hint.borrow(), None) {
                return Ok(None);
            }
            Ok(Some(this.convert_inlay_hint(&hint.borrow(), id)?))
        })
    }

    pub fn release_hints(&self, id: u32) {
        if let Some(disposables) = self.disposables.borrow_mut().remove(&id) {
            disposables.dispose();
        }
        self.cache.delete(id);
    }

    fn is_valid_inlay_hint(&self, hint: &types::InlayHint, range: Option<&types::Range>) -> bool {
        let empty_label = match &hint.label {
            types::InlayHintLabel::Text(text) => text.is_empty(),
            types::InlayHintLabel::Parts(parts) => parts.is_empty() || parts.iter().all(|part| part.value.is_empty()),
        };
        if empty_label {
            self.log_service.trace("INVALID inlay hint, empty label");
            return false;
        }
        if let Some(range) = range
            && !range.contains_position(&hint.position)
        {
            // console.log('INVALID inlay hint, position outside range', range, hint);
            return false;
        }
        true
    }

    fn convert_inlay_hint(&self, hint: &types::InlayHint, id: ChainedCacheId) -> Result<IInlayHintDto, Error> {
        let disposables = self.disposables.borrow().get(&id.0).cloned();
        let Some(disposables) = disposables else {
            return Err(Error::new("DisposableStore is missing..."));
        };

        let label = match &hint.label {
            types::InlayHintLabel::Text(text) => InlayHintLabelDto::Text(text.clone()),
            types::InlayHintLabel::Parts(parts) => {
                let mut converted = Vec::with_capacity(parts.len());
                for part in parts {
                    if part.value.is_empty() {
                        self.log_service.trace(&format!("INVALID inlay hint, empty label part {}", self.extension.identifier.value));
                        continue;
                    }
                    let mut part2 = InlayHintLabelPartDto { label: part.value.clone(), tooltip: MarkdownString::from_strict(part.tooltip.as_ref()), location: None, command: None };
                    if let Some(location) = &part.location {
                        part2.location = Some(type_convert::Location::from(location));
                    }
                    if let Some(command) = &part.command {
                        part2.command = self.commands.to_internal(Some(command), &disposables)?;
                    }
                    converted.push(part2);
                }
                InlayHintLabelDto::Parts(converted)
            }
        };

        Ok(IInlayHintDto {
            cache_id: Some(id),
            label,
            tooltip: MarkdownString::from_strict(hint.tooltip.as_ref()),
            position: type_convert::Position::from(&hint.position),
            text_edits: hint.text_edits.as_ref().map(|edits| edits.iter().map(type_convert::TextEdit::from).collect()),
            kind: hint.kind,
            padding_left: hint.padding_left,
            padding_right: hint.padding_right,
        })
    }
}

/// `LinkProviderAdapter`.
pub struct LinkProviderAdapter {
    cache: Cache<DocumentLinkRef>,
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiDocumentLinkProvider>,
}

impl LinkProviderAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiDocumentLinkProvider>) -> Rc<Self> {
        Rc::new(Self { cache: Cache::new("DocumentLink"), documents, provider })
    }

    pub fn provide_links(self: &Rc<Self>, resource: URI, token: CancellationToken) -> Promise<Option<ILinksListDto>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;

            let links = await_maybe(this.provider.provide_document_links(&doc, token.clone())).await?;
            let Some(links) = links.filter(|links| !links.is_empty()) else {
                // bad result
                return Ok(None);
            };
            if token.is_cancellation_requested() {
                // cancelled -> return without further ado, esp no caching
                // of results as they will leak
                return Ok(None);
            }
            if !this.provider.has_resolve_document_link() {
                // no resolve -> no caching
                return Ok(Some(ILinksListDto {
                    cache_id: None,
                    links: links.iter().filter(|link| Self::validate_link(&link.borrow())).map(|link| DocumentLink::from(&link.borrow())).collect(),
                }));
            }

            // cache links for future resolving
            let pid = this.cache.add(links.clone());
            let mut result = ILinksListDto { links: Vec::new(), cache_id: Some(pid) };
            for (i, link) in links.iter().enumerate() {
                if !Self::validate_link(&link.borrow()) {
                    continue;
                }

                let dto = ILinkDto { cache_id: Some((pid, i as u32)), ..DocumentLink::from(&link.borrow()) };
                result.links.push(dto);
            }
            Ok(Some(result))
        })
    }

    fn validate_link(link: &types::DocumentLink) -> bool {
        // `console.warn('DROPPING link because it is too long')` não tem logger aqui (ver a nota do módulo).
        !link.target.as_ref().is_some_and(|target| target.path().len() > 50_000)
    }

    pub fn resolve_link(self: &Rc<Self>, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<ILinkDto>> {
        let this = self.clone();
        async_fn(async move {
            if !this.provider.has_resolve_document_link() {
                return Ok(None);
            }
            let Some(item) = this.cache.get(id.0, id.1 as usize) else {
                return Ok(None);
            };
            let link = await_maybe(this.provider.resolve_document_link(&item, token)).await?;
            let Some(link) = link.filter(|link| Self::validate_link(&link.borrow())) else {
                return Ok(None);
            };
            let dto = DocumentLink::from(&link.borrow());
            Ok(Some(dto))
        })
    }

    pub fn release_links(&self, id: u32) {
        self.cache.delete(id);
    }
}

/// `ColorProviderAdapter`.
pub struct ColorProviderAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiDocumentColorProvider>,
}

impl ColorProviderAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiDocumentColorProvider>) -> Rc<Self> {
        Rc::new(Self { documents, provider })
    }

    pub fn provide_colors(self: &Rc<Self>, resource: URI, token: CancellationToken) -> Promise<Vec<IRawColorInfo>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let colors = await_maybe(this.provider.provide_document_colors(&doc, token)).await?;
            let Some(colors) = colors else {
                return Ok(Vec::new());
            };
            Ok(colors.iter().map(|ci| IRawColorInfo { color: Color::from(&ci.color), range: type_convert::Range::from(&ci.range) }).collect())
        })
    }

    pub fn provide_color_presentations(self: &Rc<Self>, resource: URI, raw: IRawColorInfo, token: CancellationToken) -> Promise<Option<Vec<IColorPresentationDto>>> {
        let this = self.clone();
        async_fn(async move {
            let document = this.documents.get_document(&resource)?;
            let range = type_convert::Range::to(&raw.range)?;
            let color = Color::to(raw.color);
            let value = await_maybe(this.provider.provide_color_presentations(&color, &ApiColorPresentationContext { document, range }, token)).await?;
            Ok(value.map(|presentations| presentations.iter().map(ColorPresentation::from).collect()))
        })
    }
}

/// `FoldingProviderAdapter`.
pub struct FoldingProviderAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiFoldingRangeProvider>,
}

impl FoldingProviderAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiFoldingRangeProvider>) -> Rc<Self> {
        Rc::new(Self { documents, provider })
    }

    pub fn provide_folding_ranges(self: &Rc<Self>, resource: URI, context: FoldingContext, token: CancellationToken) -> Promise<Option<Vec<EditorFoldingRange>>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let ranges = await_maybe(this.provider.provide_folding_ranges(&doc, &context, token)).await?;
            Ok(ranges.map(|ranges| ranges.iter().map(FoldingRange::from).collect()))
        })
    }
}

/// `SelectionRangeAdapter`.
pub struct SelectionRangeAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiSelectionRangeProvider>,
    log_service: LogServiceRef,
}

/// `vscode.Position | vscode.Range`: o `last` de `provideSelectionRanges`.
enum Last {
    Position(types::Position),
    Range(types::Range),
}

impl SelectionRangeAdapter {
    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiSelectionRangeProvider>, log_service: LogServiceRef) -> Rc<Self> {
        Rc::new(Self { documents, provider, log_service })
    }

    pub fn provide_selection_ranges(self: &Rc<Self>, resource: URI, pos: Vec<EditorPosition>, token: CancellationToken) -> Promise<Vec<Vec<EditorSelectionRange>>> {
        let this = self.clone();
        async_fn(async move {
            let document = this.documents.get_document(&resource)?;
            let positions = pos.iter().map(type_convert::Position::to).collect::<Result<Vec<_>, _>>()?;

            let all_provider_ranges = await_maybe(this.provider.provide_selection_ranges(&document, &positions, token)).await?;
            let Some(all_provider_ranges) = all_provider_ranges.filter(|ranges| !ranges.is_empty()) else {
                return Ok(Vec::new());
            };
            if all_provider_ranges.len() != positions.len() {
                this.log_service.warn("BAD selection ranges, provider must return ranges for each position");
                return Ok(Vec::new());
            }
            let mut all_results: Vec<Vec<EditorSelectionRange>> = Vec::with_capacity(positions.len());
            for (i, position) in positions.iter().enumerate() {
                let mut one_result = Vec::new();

                let mut last = Last::Position(*position);
                let mut selection_range = &all_provider_ranges[i];

                loop {
                    let contains = match &last {
                        Last::Position(position) => selection_range.range.contains_position(position),
                        Last::Range(range) => selection_range.range.contains_range(range),
                    };
                    if !contains {
                        return Err(Error::new("INVALID selection range, must contain the previous range"));
                    }
                    one_result.push(SelectionRange::from(selection_range));
                    let Some(parent) = &selection_range.parent else {
                        break;
                    };
                    last = Last::Range(selection_range.range);
                    selection_range = parent.as_ref();
                }
                all_results.push(one_result);
            }
            Ok(all_results)
        })
    }
}
