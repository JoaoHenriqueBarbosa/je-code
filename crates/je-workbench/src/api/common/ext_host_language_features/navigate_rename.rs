//! `NavigateTypeAdapter`, `RenameAdapter` e `NewSymbolNamesAdapter` de `extHostLanguageFeatures.ts`.
//!
//! Divergências:
//! - `RenameAdapter._asMessage(err)` devolve a mensagem de um `string` ou de um `Error`; em Rust todo erro é um [`Error`] e
//!   tem mensagem, então a mensagem vira o `rejectReason` e o ramo genérico (`Promise.reject(err)`) só acontece quando a
//!   mensagem é vazia (`if (rejectReason)` é falso para `''`).
//! - O `mixin(symbol, from(value), true)` de `resolveWorkspaceSymbol` troca os campos de `symbol` pelos de
//!   `WorkspaceSymbol.from(value)` e mantém o `cacheId`, que é o que o `mixin` produz.
//! - O `JSON.stringify(err, null, '\t')` do `NewSymbolNamesAdapter` só roda para um valor sem mensagem (que não é `string`
//!   nem `Error`), caso que não existe em Rust: o log leva `err.message`.

use std::rc::Rc;

use je_base::async_::{async_fn, await_maybe};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_platform::log::{LogServiceRef, Logger};

use crate::api::common::cache::Cache;
use crate::api::common::ext_host_documents::ExtHostDocuments;
use crate::api::common::ext_host_language_features_api::{
    ApiNewSymbolName, ApiNewSymbolNamesProvider, ApiPrepareRename, ApiRenameProvider, ApiWorkspaceSymbolProvider,
};
use crate::api::common::ext_host_language_features_converters::WorkspaceSymbol;
use crate::api::common::ext_host_protocol::{
    IWorkspaceEditDto, IWorkspaceSymbolDto, IWorkspaceSymbolsDto, LanguagesNewSymbolNameTriggerKind, NewSymbolNameDto, RenameEditsDto, RenameLocationDto,
};
use crate::api::common::ext_host_type_converters as type_convert;
use crate::api::common::ext_host_types::{NewSymbolNameTriggerKind, SymbolInformation};

/// `NavigateTypeAdapter`.
pub struct NavigateTypeAdapter {
    cache: Cache<SymbolInformation>,
    provider: Rc<dyn ApiWorkspaceSymbolProvider>,
    log_service: LogServiceRef,
}

impl NavigateTypeAdapter {
    pub fn new(provider: Rc<dyn ApiWorkspaceSymbolProvider>, log_service: LogServiceRef) -> Rc<Self> {
        Rc::new(Self { cache: Cache::new("WorkspaceSymbols"), provider, log_service })
    }

    pub fn provide_workspace_symbols(self: &Rc<Self>, search: String, token: CancellationToken) -> Promise<IWorkspaceSymbolsDto> {
        let this = self.clone();
        async_fn(async move {
            let value = await_maybe(this.provider.provide_workspace_symbols(&search, token.clone())).await?;

            // Canceled requests cannot release their cache entries, so discard late provider results before caching them.
            let Some(value) = value.filter(|symbols| !symbols.is_empty() && !token.is_cancellation_requested()) else {
                return Ok(IWorkspaceSymbolsDto { cache_id: None, symbols: Vec::new() });
            };

            let sid = this.cache.add(value.clone());
            let mut result = IWorkspaceSymbolsDto { cache_id: Some(sid), symbols: Vec::new() };

            for (i, item) in value.iter().enumerate() {
                if item.name.is_empty() {
                    this.log_service.warn(&format!("INVALID SymbolInformation {item:?}"));
                    continue;
                }
                result.symbols.push(IWorkspaceSymbolDto { cache_id: Some((sid, i as u32)), ..WorkspaceSymbol::from(item) });
            }

            Ok(result)
        })
    }

    pub fn resolve_workspace_symbol(self: &Rc<Self>, symbol: IWorkspaceSymbolDto, token: CancellationToken) -> Promise<Option<IWorkspaceSymbolDto>> {
        let this = self.clone();
        async_fn(async move {
            if !this.provider.has_resolve_workspace_symbol() {
                return Ok(Some(symbol));
            }
            let Some((cache_id, index)) = symbol.cache_id else {
                return Ok(Some(symbol));
            };
            let Some(item) = this.cache.get(cache_id, index as usize) else {
                return Ok(None);
            };
            let value = await_maybe(this.provider.resolve_workspace_symbol(&item, token)).await?;
            Ok(value.map(|value| IWorkspaceSymbolDto { cache_id: symbol.cache_id, ..WorkspaceSymbol::from(&value) }))
        })
    }

    pub fn release_workspace_symbols(&self, id: u32) {
        self.cache.delete(id);
    }
}

/// `RenameAdapter`.
pub struct RenameAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiRenameProvider>,
    log_service: LogServiceRef,
}

impl RenameAdapter {
    /// `RenameAdapter.supportsResolving(provider)`.
    pub fn supports_resolving(provider: &dyn ApiRenameProvider) -> bool {
        provider.has_prepare_rename()
    }

    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiRenameProvider>, log_service: LogServiceRef) -> Rc<Self> {
        Rc::new(Self { documents, provider, log_service })
    }

    pub fn provide_rename_edits(self: &Rc<Self>, resource: URI, position: EditorPosition, new_name: String, token: CancellationToken) -> Promise<Option<RenameEditsDto>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let pos = type_convert::Position::to(&position)?;

            let outcome: Result<Option<IWorkspaceEditDto>, Error> = async {
                let value = await_maybe(this.provider.provide_rename_edits(&doc, &pos, &new_name, token)).await?;
                Ok(value.map(|value| type_convert::WorkspaceEdit::from(&value, None)))
            }
            .await;

            match outcome {
                Ok(None) => Ok(None),
                Ok(Some(workspace_edit)) => Ok(Some(RenameEditsDto { workspace_edit: Some(workspace_edit), reject_reason: None })),
                Err(err) => match Self::as_message(&err) {
                    Some(reject_reason) => Ok(Some(RenameEditsDto { workspace_edit: None, reject_reason: Some(reject_reason) })),
                    // generic error
                    None => Err(err),
                },
            }
        })
    }

    pub fn resolve_rename_location(self: &Rc<Self>, resource: URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<RenameLocationDto>> {
        let this = self.clone();
        async_fn(async move {
            if !this.provider.has_prepare_rename() {
                return Ok(None);
            }

            let doc = this.documents.get_document(&resource)?;
            let pos = type_convert::Position::to(&position)?;

            let outcome: Result<Option<RenameLocationDto>, Error> = async {
                let range_or_location = await_maybe(this.provider.prepare_rename(&doc, &pos, token)).await?;

                let (range, text) = match range_or_location {
                    Some(ApiPrepareRename::Range(range)) => {
                        let text = doc.get_text(Some(&range)).to_string_lossy();
                        (Some(range), Some(text))
                    }
                    Some(ApiPrepareRename::RangeWithPlaceholder { range, placeholder }) => (Some(range), placeholder),
                    None => (None, None),
                };

                let (Some(range), Some(text)) = (range, text.filter(|text| !text.is_empty())) else {
                    return Ok(None);
                };
                if range.start().line() > pos.line() || range.end().line() < pos.line() {
                    this.log_service.warn("INVALID rename location: position line must be within range start/end lines");
                    return Ok(None);
                }
                Ok(Some(RenameLocationDto { range: Some(type_convert::Range::from(&range)), text: Some(text), reject_reason: None }))
            }
            .await;

            match outcome {
                Ok(location) => Ok(location),
                Err(err) => match Self::as_message(&err) {
                    Some(reject_reason) => Ok(Some(RenameLocationDto { range: None, text: None, reject_reason: Some(reject_reason) })),
                    None => Err(err),
                },
            }
        })
    }

    /// `RenameAdapter._asMessage(err)`: a mensagem, quando não é vazia (ver a nota do módulo).
    fn as_message(err: &Error) -> Option<String> {
        if err.message.is_empty() { None } else { Some(err.message.clone()) }
    }
}

/// `NewSymbolNamesAdapter`.
pub struct NewSymbolNamesAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiNewSymbolNamesProvider>,
    log_service: LogServiceRef,
}

impl NewSymbolNamesAdapter {
    /// `languageTriggerKindToVSCodeTriggerKind`.
    fn language_trigger_kind_to_vscode_trigger_kind(kind: LanguagesNewSymbolNameTriggerKind) -> NewSymbolNameTriggerKind {
        match kind {
            LanguagesNewSymbolNameTriggerKind::Invoke => NewSymbolNameTriggerKind::Invoke,
            LanguagesNewSymbolNameTriggerKind::Automatic => NewSymbolNameTriggerKind::Automatic,
        }
    }

    pub fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiNewSymbolNamesProvider>, log_service: LogServiceRef) -> Rc<Self> {
        Rc::new(Self { documents, provider, log_service })
    }

    pub fn supports_automatic_new_symbol_names_trigger_kind(self: &Rc<Self>) -> Promise<Option<bool>> {
        let this = self.clone();
        async_fn(async move { await_maybe(this.provider.supports_automatic_trigger_kind()).await })
    }

    pub fn provide_new_symbol_names(
        self: &Rc<Self>,
        resource: URI,
        range: EditorRange,
        trigger_kind: LanguagesNewSymbolNameTriggerKind,
        token: CancellationToken,
    ) -> Promise<Option<Vec<NewSymbolNameDto>>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let pos = type_convert::Range::to(&range)?;

            let outcome: Result<Option<Vec<NewSymbolNameDto>>, Error> = async {
                let kind = Self::language_trigger_kind_to_vscode_trigger_kind(trigger_kind);
                let value = await_maybe(this.provider.provide_new_symbol_names(&doc, &pos, kind, token)).await?;
                Ok(value.map(|names| {
                    names
                        .into_iter()
                        .map(|name| match name {
                            // @ulugbekna: for backward compatibility because `value` used to be just `string[]`
                            ApiNewSymbolName::Text(new_symbol_name) => NewSymbolNameDto { new_symbol_name, tags: None },
                            ApiNewSymbolName::Name(name) => NewSymbolNameDto { new_symbol_name: name.new_symbol_name, tags: name.tags },
                        })
                        .collect()
                }))
            }
            .await;

            match outcome {
                Ok(names) => Ok(names),
                Err(err) => {
                    this.log_service.error(&err.message);
                    Ok(None)
                }
            }
        })
    }
}
