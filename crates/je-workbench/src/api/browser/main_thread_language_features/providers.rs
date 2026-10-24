//! Os provedores do `MainThreadLanguageFeatures` que o original escreve como objetos literais dentro dos `$register*`
//! (e as classes `MainThreadDocumentSemanticTokensProvider` e `MainThreadDocumentRangeSemanticTokensProvider`), mais as
//! funções de reviver os DTOs (`_reviveLocationDto`, `_reviveLocationLinkDto`, `_reviveWorkspaceSymbolDto`,
//! `_reviveCodeActionDto`, `_reviveLinkDTO`, `_reviveRegExp`, `_inflateSuggestDto`) e `reviveWorkspaceEditDto` de
//! `mainThreadBulkEdits.ts`.
//!
//! Divergências:
//! - Um provedor é uma struct com o `handle` e o proxy ([`Remote`]); o objeto literal do original fecha sobre os dois.
//! - `URI.revive` é a identidade (o `UriComponents` do protocolo já é [`URI`]).
//! - Os objetos que o original reconhece depois por identidade e o editor guarda por valor ou por `Rc` ganham uma tabela
//!   do provedor: o `cacheId` de uma ação de código (`CodeActionRef`) e o de uma ajuda de assinatura (`Rc<SignatureHelp>`)
//!   são achados pelo endereço do `Rc`; o `_id` de um item de completar (um `CompletionItem` é copiado, não
//!   compartilhado) é achado pelo conjunto (rótulo, texto inserido, `sortText`, `filterText`), com o último item de
//!   mesma chave vencendo.
//! - `$ident` de um comando (`ICommandDto`) não existe no `Command` do editor: o `Command` volta ao host sem ele.
//! - `reviveWorkspaceEditDto` (de `mainThreadBulkEdits.ts`) é reexportada de [`crate::api::browser::main_thread_bulk_edits`],
//!   onde está o porte dela.
//! - `FoldingRangeProvider.id` (o `extensionId.value`) não existe no provedor de dobra do editor.
//! - `MultiDocumentHighlightProvider.selector` é o [`LanguageSelector`] do registro.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::js_regexp::JsRegExp;
use je_base::lifecycle::IDisposable;
use je_base::map::ResourceMap;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::code_action::languages::{
    CodeAction, CodeActionContext, CodeActionDocumentation, CodeActionList, CodeActionListObject, CodeActionProvider, CodeActionRef, Command, TextEdit,
};
use je_editor::code_editor_widget::SelectionOrRange;
use je_editor::edit_operation::SingleEditOperation;
use je_editor::folding::syntax_range_provider::{FoldingContext, FoldingRange, FoldingRangeProvider};
use je_editor::language_features_service::{
    ChainedCacheId, CodeLens, CodeLensList, CodeLensProvider, DocumentColorProvider, DocumentFormattingEditProvider, DocumentRangeFormattingEditProvider,
    DocumentRangeSemanticTokensProvider, DocumentSemanticTokensProvider, EvaluatableExpression, EvaluatableExpressionProvider, FormattingOptions as EditorFormattingOptions,
    Hover, HoverContext, HoverProvider, IColor, IColorInformation, IColorPresentation, InlayHint, InlayHintKind, InlayHintLabel, InlayHintLabelPart, InlayHintList,
    InlayHintsProvider, InlineValue, InlineValueContext, InlineValuesProvider, LinkProvider, LinkedEditingRangeProvider, LinkedEditingRanges, LinksList, Location,
    LocationLink, LocationLinkProvider, NewSymbolName, NewSymbolNameTag, NewSymbolNameTriggerKind, NewSymbolNamesProvider, OnTypeFormattingEditProvider,
    ReferenceContext as EditorReferenceContext, ReferenceProvider, RenameEdits, RenameLocation, RenameProvider, SemanticTokens, SemanticTokensEdit,
    SemanticTokensEdits, SemanticTokensLegend, SemanticTokensResult,
};
use je_editor::language_providers::{
    DocumentHighlight, DocumentHighlightProvider, DocumentSymbol, DocumentSymbolProvider, HighlightMap, MultiDocumentHighlightProvider, ProviderResult,
    SelectionRange, SelectionRangeProvider,
};
use je_editor::language_selector::LanguageSelector;
use je_editor::languages::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionItemProvider, CompletionItemRange, CompletionLabel, CompletionList, ILink, LinkUrl,
};
use je_editor::parameter_hints::languages::{SignatureHelp, SignatureHelpContext, SignatureHelpProvider, SignatureHelpResult};
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::semantic_tokens_dto::{SemanticTokensDto, decode_semantic_tokens_dto};
use je_editor::text_model::TextModel;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use crate::api::common::ext_host_protocol::{
    CodeActionDocumentationDto, FormattingOptions, HoverContextDto, HoverIdDto, HoverVerbosityRequestDto, HoverWithId, ICodeActionDto, ICodeLensDto,
    ICommandDto, IColorPresentationDto, IInlayHintDto, IInlineValueContextDto, ILinkDto, ILocationDto, ILocationLinkDto, IRawColorInfo, IRegExpDto,
    ISignatureHelpContextDto, ISignatureHelpDto, ISuggestDataDto, ISuggestResultDto, InlayHintLabelDto, InlineValueDto,
    LanguagesNewSymbolNameTriggerKind, RangeOrSelection, ReferenceContext, ExtHostLanguageFeaturesShape,
};

/// O `handle` e o proxy do host de extensões que o objeto literal de cada `$register*` captura.
#[derive(Clone)]
pub(super) struct Remote {
    pub handle: u32,
    pub proxy: Rc<dyn ExtHostLanguageFeaturesShape>,
}

/// `[range, text]` -> o mesmo erro do `new RegExp(pattern, flags)` inválido.
fn regexp_error(error: impl std::fmt::Debug) -> Error {
    Error::named("SyntaxError", format!("Invalid regular expression: {error:?}"))
}

// ---- reviver DTOs -------------------------------------------------------------------------------------

/// `_reviveRegExp(regExp)`.
pub(super) fn revive_reg_exp(reg_exp: &IRegExpDto) -> Result<JsRegExp, Error> {
    JsRegExp::from_str(&reg_exp.pattern, reg_exp.flags.as_deref().unwrap_or("")).map_err(regexp_error)
}

/// `_reviveLocationDto(data)`: o item sem `uri` ou sem `range` não é uma `languages.Location`.
pub(super) fn revive_location(data: ILocationDto) -> Option<Location> {
    Some(Location { uri: data.uri?, range: data.range? })
}

/// `_reviveLocationLinkDto(data)`.
pub(super) fn revive_location_link(data: ILocationLinkDto) -> LocationLink {
    LocationLink { origin_selection_range: data.origin_selection_range, uri: data.uri, range: data.range, target_selection_range: data.target_selection_range }
}

/// O `ICommandDto` como `languages.Command`.
pub(super) fn command_from_dto(command: &ICommandDto) -> Command {
    Command { id: command.id.clone(), title: command.title.clone(), tooltip: command.tooltip.clone(), arguments: command.arguments.clone() }
}

/// O `languages.Command` como o `ICommandDto` que volta ao host (sem `$ident`, ver a nota do módulo).
pub(super) fn command_to_dto(command: &Command) -> ICommandDto {
    ICommandDto { ident: None, id: command.id.clone(), title: command.title.clone(), tooltip: command.tooltip.clone(), arguments: command.arguments.clone() }
}

/// `reviveWorkspaceEditDto` e o tipo do `resolveDataTransferFile`: definidos em `mainThreadBulkEdits.ts` (ver
/// [`crate::api::browser::main_thread_bulk_edits`]).
pub(super) use crate::api::browser::main_thread_bulk_edits::{ResolveDataTransferFile, revive_workspace_edit_dto};

/// `_reviveLinkDTO(data)`.
pub(super) fn revive_link_dto(data: &ILinkDto) -> ILink {
    ILink { range: data.range, url: data.url.clone().map(LinkUrl::Uri), tooltip: data.tooltip.clone(), cache_id: data.cache_id }
}

/// `languages.CodeLens` do `ICodeLensDto`.
fn code_lens_from_dto(dto: &ICodeLensDto) -> CodeLens {
    CodeLens { range: dto.range, id: dto.id.clone(), command: dto.command.as_ref().map(command_from_dto), cache_id: dto.cache_id }
}

fn formatting_options_to_dto(options: &EditorFormattingOptions) -> FormattingOptions {
    FormattingOptions { tab_size: options.tab_size, insert_spaces: options.insert_spaces }
}

// ---- outline, lente de código -------------------------------------------------------------------------

/// `documentSymbolProvider`: `{ displayName, provideDocumentSymbols }`.
pub(super) struct MainThreadDocumentSymbolProvider {
    pub remote: Remote,
    pub display_name: String,
}

impl DocumentSymbolProvider for MainThreadDocumentSymbolProvider {
    fn display_name(&self) -> Option<&str> {
        Some(&self.display_name)
    }

    fn provide_document_symbols(&self, model: &Rc<TextModel>, token: &CancellationToken) -> ProviderResult<Vec<DocumentSymbol>> {
        self.remote.proxy.rpc_provide_document_symbols(self.remote.handle, &model.uri(), token.clone())
    }
}

/// `codeLensProvider`.
pub(super) struct MainThreadCodeLensProvider {
    pub remote: Remote,
    pub on_did_change: Option<Event<()>>,
}

impl CodeLensProvider for MainThreadCodeLensProvider {
    fn on_did_change(&self) -> Option<Event<()>> {
        self.on_did_change.clone()
    }

    fn provide_code_lenses(&self, model: &Rc<TextModel>, token: &CancellationToken) -> ProviderResult<Rc<CodeLensList>> {
        let (remote, uri, token) = (self.remote.clone(), model.uri(), token.clone());
        async_fn(async move {
            let Some(list_dto) = remote.proxy.rpc_provide_code_lenses(remote.handle, &uri, token).await? else {
                return Ok(None);
            };
            let lenses = list_dto.lenses.iter().map(code_lens_from_dto).collect();
            let cache_id = list_dto.cache_id;
            Ok(Some(Rc::new(CodeLensList::new(lenses, move || {
                // `listDto.cacheId && release`: o `0` é falso.
                if let Some(cache_id) = cache_id.filter(|cache_id| *cache_id != 0) {
                    remote.proxy.rpc_release_code_lenses(remote.handle, cache_id);
                }
            }))))
        })
    }

    fn has_resolve_code_lens(&self) -> bool {
        true
    }

    fn resolve_code_lens(&self, model: &Rc<TextModel>, code_lens: &CodeLens, token: &CancellationToken) -> ProviderResult<CodeLens> {
        let (remote, model, token) = (self.remote.clone(), Rc::clone(model), token.clone());
        let dto = ICodeLensDto {
            cache_id: code_lens.cache_id,
            range: code_lens.range,
            id: code_lens.id.clone(),
            command: code_lens.command.as_ref().map(command_to_dto),
        };
        async_fn(async move {
            let result = remote.proxy.rpc_resolve_code_lens(remote.handle, dto, token.clone()).await?;
            let Some(result) = result else { return Ok(None) };
            if token.is_cancellation_requested() {
                return Ok(None);
            }
            Ok(Some(CodeLens { range: model.validate_range(&result.range), ..code_lens_from_dto(&result) }))
        })
    }
}

// ---- localização, hover, referências -----------------------------------------------------------------

/// Qual dos quatro `$provide*` de localização um [`MainThreadLocationLinkProvider`] chama.
#[derive(Clone, Copy)]
pub(super) enum LocationKind {
    Definition,
    Declaration,
    Implementation,
    TypeDefinition,
}

/// `definitionProvider`, `declarationProvider`, `implementationProvider` e `typeDefinitionProvider`.
pub(super) struct MainThreadLocationLinkProvider {
    pub remote: Remote,
    pub kind: LocationKind,
}

impl LocationLinkProvider for MainThreadLocationLinkProvider {
    fn provide_location_links(&self, model: &Rc<TextModel>, position: Position, token: &CancellationToken) -> ProviderResult<Vec<LocationLink>> {
        let (handle, uri, token) = (self.remote.handle, model.uri(), token.clone());
        let proxy = &self.remote.proxy;
        let links = match self.kind {
            LocationKind::Definition => proxy.rpc_provide_definition(handle, &uri, position, token),
            LocationKind::Declaration => proxy.rpc_provide_declaration(handle, &uri, position, token),
            LocationKind::Implementation => proxy.rpc_provide_implementation(handle, &uri, position, token),
            LocationKind::TypeDefinition => proxy.rpc_provide_type_definition(handle, &uri, position, token),
        };
        links.then(|links| Some(links.into_iter().map(revive_location_link).collect()))
    }
}

/// `hoverProvider`.
pub(super) struct MainThreadHoverProvider {
    pub remote: Remote,
}

fn hover_from_dto(hover: HoverWithId) -> Hover {
    Hover {
        contents: hover.hover.contents,
        range: hover.hover.range,
        can_increase_verbosity: hover.hover.can_increase_verbosity,
        can_decrease_verbosity: hover.hover.can_decrease_verbosity,
        id: Some(hover.id),
    }
}

impl HoverProvider for MainThreadHoverProvider {
    fn provide_hover(&self, model: &Rc<TextModel>, position: Position, token: &CancellationToken, context: Option<&HoverContext>) -> ProviderResult<Hover> {
        let serialized_context = HoverContextDto {
            verbosity_request: context.and_then(|context| context.verbosity_request).map(|request| HoverVerbosityRequestDto {
                verbosity_delta: request.verbosity_delta,
                previous_hover: HoverIdDto { id: request.previous_hover_id },
            }),
        };
        self.remote
            .proxy
            .rpc_provide_hover(self.remote.handle, &model.uri(), position, Some(serialized_context), token.clone())
            .then(|hover| hover.map(hover_from_dto))
    }
}

/// `evaluatableExpressionProvider`.
pub(super) struct MainThreadEvaluatableExpressionProvider {
    pub remote: Remote,
}

impl EvaluatableExpressionProvider for MainThreadEvaluatableExpressionProvider {
    fn provide_evaluatable_expression(&self, model: &Rc<TextModel>, position: Position, token: &CancellationToken) -> ProviderResult<EvaluatableExpression> {
        self.remote
            .proxy
            .rpc_provide_evaluatable_expression(self.remote.handle, &model.uri(), position, token.clone())
            .then(|expression| expression.map(|dto| EvaluatableExpression { range: dto.range, expression: dto.expression }))
    }
}

/// `inlineValuesProvider`.
pub(super) struct MainThreadInlineValuesProvider {
    pub remote: Remote,
    pub on_did_change_inline_values: Option<Event<()>>,
}

impl InlineValuesProvider for MainThreadInlineValuesProvider {
    fn on_did_change_inline_values(&self) -> Option<Event<()>> {
        self.on_did_change_inline_values.clone()
    }

    fn provide_inline_values(
        &self,
        model: &Rc<TextModel>,
        view_port: Range,
        context: &InlineValueContext,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<InlineValue>> {
        let context = IInlineValueContextDto { frame_id: context.frame_id, stopped_location: context.stopped_location };
        self.remote.proxy.rpc_provide_inline_values(self.remote.handle, &model.uri(), view_port, context, token.clone()).then(|values| {
            values.map(|values| {
                values
                    .into_iter()
                    .map(|value| match value {
                        InlineValueDto::Text { range, text } => InlineValue::Text { range, text },
                        InlineValueDto::Variable { range, variable_name, case_sensitive_lookup } => {
                            InlineValue::VariableLookup { range, variable_name, case_sensitive_lookup }
                        }
                        InlineValueDto::Expression { range, expression } => InlineValue::Expression { range, expression },
                    })
                    .collect()
            })
        })
    }
}

/// `documentHighlightProvider`.
pub(super) struct MainThreadDocumentHighlightProvider {
    pub remote: Remote,
}

impl DocumentHighlightProvider for MainThreadDocumentHighlightProvider {
    fn provide_document_highlights(&self, model: &Rc<TextModel>, position: Position, token: &CancellationToken) -> ProviderResult<Vec<DocumentHighlight>> {
        self.remote.proxy.rpc_provide_document_highlights(self.remote.handle, &model.uri(), position, token.clone())
    }
}

/// `multiDocumentHighlightProvider`.
pub(super) struct MainThreadMultiDocumentHighlightProvider {
    pub remote: Remote,
    pub selector: LanguageSelector,
}

impl MultiDocumentHighlightProvider for MainThreadMultiDocumentHighlightProvider {
    fn selector(&self) -> &LanguageSelector {
        &self.selector
    }

    fn provide_multi_document_highlights(
        &self,
        primary_model: &Rc<TextModel>,
        position: Position,
        other_models: &[Rc<TextModel>],
        token: &CancellationToken,
    ) -> ProviderResult<HighlightMap> {
        let other_uris: Vec<URI> = other_models.iter().map(|model| model.uri()).collect();
        self.remote
            .proxy
            .rpc_provide_multi_document_highlights(self.remote.handle, &primary_model.uri(), position, other_uris, token.clone())
            .then(|dto| {
                // dto should be non-null + non-undefined
                // dto length of 0 is valid, just no highlights, pass this through.
                let dto = dto?;
                let mut result: ResourceMap<Vec<DocumentHighlight>> = ResourceMap::new();
                for value in dto {
                    // check if the URI exists already, if so, combine the highlights, otherwise create a new entry
                    let mut highlights = result.get(&value.uri).cloned().unwrap_or_default();
                    highlights.extend(value.highlights);
                    result.set(value.uri, highlights);
                }
                Some(Rc::new(result))
            })
    }
}

/// `linkedEditingRangeProvider`.
pub(super) struct MainThreadLinkedEditingRangeProvider {
    pub remote: Remote,
}

impl LinkedEditingRangeProvider for MainThreadLinkedEditingRangeProvider {
    fn provide_linked_editing_ranges(&self, model: &Rc<TextModel>, position: Position, token: &CancellationToken) -> ProviderResult<LinkedEditingRanges> {
        let (remote, uri, token) = (self.remote.clone(), model.uri(), token.clone());
        async_fn(async move {
            let Some(res) = remote.proxy.rpc_provide_linked_editing_ranges(remote.handle, &uri, position, token).await? else {
                return Ok(None);
            };
            let word_pattern = match &res.word_pattern {
                Some(word_pattern) => Some(Rc::new(revive_reg_exp(word_pattern)?)),
                None => None,
            };
            Ok(Some(LinkedEditingRanges { ranges: res.ranges, word_pattern }))
        })
    }
}

/// `referenceProvider`.
pub(super) struct MainThreadReferenceProvider {
    pub remote: Remote,
}

impl ReferenceProvider for MainThreadReferenceProvider {
    fn provide_references(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        context: &EditorReferenceContext,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<Location>> {
        let context = ReferenceContext { include_declaration: context.include_declaration };
        self.remote
            .proxy
            .rpc_provide_references(self.remote.handle, &model.uri(), position, context, token.clone())
            .then(|locations| locations.map(|locations| locations.into_iter().filter_map(revive_location).collect()))
    }
}

// ---- ações de código ----------------------------------------------------------------------------------

/// `codeActionProvider`.
pub(super) struct MainThreadCodeActionProvider {
    pub remote: Remote,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub provided_kinds: Option<Vec<String>>,
    pub documentation: Option<Vec<CodeActionDocumentation>>,
    pub display_name: String,
    pub extension_id: String,
    pub supports_resolve: bool,
    /// O `cacheId` de cada ação devolvida, achado pelo endereço do `CodeActionRef` (ver a nota do módulo).
    pub cache_ids: Rc<RefCell<HashMap<usize, ChainedCacheId>>>,
}

pub(super) fn documentation_from_dto(documentation: &[CodeActionDocumentationDto]) -> Vec<CodeActionDocumentation> {
    documentation.iter().map(|doc| CodeActionDocumentation { kind: doc.kind.clone(), command: command_from_dto(&doc.command) }).collect()
}

fn action_key(action: &CodeActionRef) -> usize {
    Rc::as_ptr(action) as *const () as usize
}

/// `_reviveCodeActionDto(data, uriIdentService)` de uma ação.
fn revive_code_action_dto(data: &ICodeActionDto, uri_identity_service: &Rc<dyn IUriIdentityService>) -> Result<CodeAction, Error> {
    let edit = match &data.edit {
        Some(edit) => Some(revive_workspace_edit_dto(edit, uri_identity_service, None)?),
        None => None,
    };
    Ok(CodeAction {
        is_synthetic: data.is_synthetic,
        title: data.title.clone(),
        command: data.command.as_ref().map(command_from_dto),
        edit,
        diagnostics: data.diagnostics.clone(),
        kind: data.kind.clone(),
        is_preferred: data.is_preferred,
        is_ai: data.is_ai,
        disabled: data.disabled.clone(),
        ranges: data.ranges.clone(),
    })
}

impl CodeActionProvider for MainThreadCodeActionProvider {
    fn display_name(&self) -> Option<&str> {
        Some(&self.display_name)
    }

    fn extension_id(&self) -> Option<&str> {
        Some(&self.extension_id)
    }

    fn provide_code_actions(
        &self,
        model: &Rc<TextModel>,
        range: &SelectionOrRange,
        context: &CodeActionContext,
        token: &CancellationToken,
    ) -> ProviderResult<Rc<dyn CodeActionList>> {
        let (remote, uri, context, token) = (self.remote.clone(), model.uri(), context.clone(), token.clone());
        let (uri_identity_service, cache_ids) = (Rc::clone(&self.uri_identity_service), Rc::clone(&self.cache_ids));
        let range_or_selection = match range {
            SelectionOrRange::Selection(selection) => RangeOrSelection::Selection(selection.clone()),
            SelectionOrRange::Range(range) => RangeOrSelection::Range(range.clone()),
        };
        async_fn(async move {
            let Some(list_dto) = remote.proxy.rpc_provide_code_actions(remote.handle, &uri, range_or_selection, context, token).await? else {
                return Ok(None);
            };
            let mut actions: Vec<CodeActionRef> = Vec::with_capacity(list_dto.actions.len());
            for action_dto in &list_dto.actions {
                let action: CodeActionRef = Rc::new(RefCell::new(revive_code_action_dto(action_dto, &uri_identity_service)?));
                if let Some(cache_id) = action_dto.cache_id {
                    cache_ids.borrow_mut().insert(action_key(&action), cache_id);
                }
                actions.push(action);
            }
            let keys: Vec<usize> = actions.iter().map(action_key).collect();
            let cache_id = list_dto.cache_id;
            let list = CodeActionListObject::new(actions, move || {
                for key in &keys {
                    cache_ids.borrow_mut().remove(key);
                }
                remote.proxy.rpc_release_code_actions(remote.handle, cache_id);
            });
            Ok(Some(list as Rc<dyn CodeActionList>))
        })
    }

    fn has_resolve_code_action(&self) -> bool {
        self.supports_resolve
    }

    fn resolve_code_action(&self, code_action: &CodeActionRef, token: &CancellationToken) -> ProviderResult<CodeActionRef> {
        let (remote, code_action, token) = (self.remote.clone(), Rc::clone(code_action), token.clone());
        let uri_identity_service = Rc::clone(&self.uri_identity_service);
        let cache_id = self.cache_ids.borrow().get(&action_key(&code_action)).copied();
        async_fn(async move {
            let Some(cache_id) = cache_id else {
                return Err(Error::new("Cannot resolve a code action without a cache id"));
            };
            let resolved = remote.proxy.rpc_resolve_code_action(remote.handle, cache_id, token).await?;
            if let Some(edit) = &resolved.edit {
                code_action.borrow_mut().edit = Some(revive_workspace_edit_dto(edit, &uri_identity_service, None)?);
            }
            if let Some(command) = &resolved.command {
                code_action.borrow_mut().command = Some(command_from_dto(command));
            }
            Ok(Some(code_action))
        })
    }

    fn provided_code_action_kinds(&self) -> Option<Vec<String>> {
        self.provided_kinds.clone()
    }

    fn documentation(&self) -> Option<Vec<CodeActionDocumentation>> {
        self.documentation.clone()
    }
}

// ---- formatação ---------------------------------------------------------------------------------------

/// `documentFormattingEditProvider`.
pub(super) struct MainThreadDocumentFormattingProvider {
    pub remote: Remote,
    pub extension_id: ExtensionIdentifier,
    pub display_name: String,
}

impl DocumentFormattingEditProvider for MainThreadDocumentFormattingProvider {
    fn extension_id(&self) -> Option<&ExtensionIdentifier> {
        Some(&self.extension_id)
    }

    fn display_name(&self) -> Option<&str> {
        Some(&self.display_name)
    }

    fn provide_document_formatting_edits(&self, model: &Rc<TextModel>, options: &EditorFormattingOptions, token: &CancellationToken) -> ProviderResult<Vec<TextEdit>> {
        self.remote.proxy.rpc_provide_document_formatting_edits(self.remote.handle, &model.uri(), formatting_options_to_dto(options), token.clone())
    }
}

/// `documentRangeFormattingEditProvider`.
pub(super) struct MainThreadRangeFormattingProvider {
    pub remote: Remote,
    pub extension_id: ExtensionIdentifier,
    pub display_name: String,
    pub supports_ranges: bool,
}

impl DocumentRangeFormattingEditProvider for MainThreadRangeFormattingProvider {
    fn extension_id(&self) -> Option<&ExtensionIdentifier> {
        Some(&self.extension_id)
    }

    fn display_name(&self) -> Option<&str> {
        Some(&self.display_name)
    }

    fn provide_document_range_formatting_edits(
        &self,
        model: &Rc<TextModel>,
        range: Range,
        options: &EditorFormattingOptions,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<TextEdit>> {
        self.remote.proxy.rpc_provide_document_range_formatting_edits(self.remote.handle, &model.uri(), range, formatting_options_to_dto(options), token.clone())
    }

    fn has_ranges_formatting(&self) -> bool {
        self.supports_ranges
    }

    fn provide_document_ranges_formatting_edits(
        &self,
        model: &Rc<TextModel>,
        ranges: &[Range],
        options: &EditorFormattingOptions,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<TextEdit>> {
        self.remote.proxy.rpc_provide_document_ranges_formatting_edits(self.remote.handle, &model.uri(), ranges.to_vec(), formatting_options_to_dto(options), token.clone())
    }
}

/// `onTypeFormattingEditProvider`.
pub(super) struct MainThreadOnTypeFormattingProvider {
    pub remote: Remote,
    pub extension_id: ExtensionIdentifier,
    pub auto_format_trigger_characters: Vec<String>,
}

impl OnTypeFormattingEditProvider for MainThreadOnTypeFormattingProvider {
    fn extension_id(&self) -> Option<&ExtensionIdentifier> {
        Some(&self.extension_id)
    }

    fn auto_format_trigger_characters(&self) -> &[String] {
        &self.auto_format_trigger_characters
    }

    fn provide_on_type_formatting_edits(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        ch: &str,
        options: &EditorFormattingOptions,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<TextEdit>> {
        self.remote.proxy.rpc_provide_on_type_formatting_edits(self.remote.handle, &model.uri(), position, ch, formatting_options_to_dto(options), token.clone())
    }
}

// ---- renomeação ---------------------------------------------------------------------------------------

/// `renameProvider`.
pub(super) struct MainThreadRenameProvider {
    pub remote: Remote,
    pub uri_identity_service: Rc<dyn IUriIdentityService>,
    pub supports_resolve_location: bool,
}

impl RenameProvider for MainThreadRenameProvider {
    fn provide_rename_edits(&self, model: &Rc<TextModel>, position: Position, new_name: &str, token: &CancellationToken) -> ProviderResult<RenameEdits> {
        let (remote, uri, new_name, token) = (self.remote.clone(), model.uri(), new_name.to_owned(), token.clone());
        let uri_identity_service = Rc::clone(&self.uri_identity_service);
        async_fn(async move {
            let Some(data) = remote.proxy.rpc_provide_rename_edits(remote.handle, &uri, position, &new_name, token).await? else {
                return Ok(None);
            };
            let workspace_edit = match &data.workspace_edit {
                Some(edit) => Some(revive_workspace_edit_dto(edit, &uri_identity_service, None)?),
                None => None,
            };
            Ok(Some(RenameEdits { workspace_edit, reject_reason: data.reject_reason }))
        })
    }

    fn has_resolve_rename_location(&self) -> bool {
        self.supports_resolve_location
    }

    fn resolve_rename_location(&self, model: &Rc<TextModel>, position: Position, token: &CancellationToken) -> ProviderResult<RenameLocation> {
        self.remote
            .proxy
            .rpc_resolve_rename_location(self.remote.handle, &model.uri(), position, token.clone())
            .then(|location| location.map(|dto| RenameLocation { range: dto.range, text: dto.text, reject_reason: dto.reject_reason }))
    }
}

/// `newSymbolNamesProvider`.
pub(super) struct MainThreadNewSymbolNamesProvider {
    pub remote: Remote,
    pub supports_automatic_new_symbol_names_trigger_kind: Promise<Option<bool>>,
}

impl NewSymbolNamesProvider for MainThreadNewSymbolNamesProvider {
    fn supports_automatic_new_symbol_names_trigger_kind(&self) -> Promise<Option<bool>> {
        self.supports_automatic_new_symbol_names_trigger_kind.clone()
    }

    fn provide_new_symbol_names(
        &self,
        model: &Rc<TextModel>,
        range: Range,
        trigger_kind: NewSymbolNameTriggerKind,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<NewSymbolName>> {
        let trigger_kind = match trigger_kind {
            NewSymbolNameTriggerKind::Invoke => LanguagesNewSymbolNameTriggerKind::Invoke,
            NewSymbolNameTriggerKind::Automatic => LanguagesNewSymbolNameTriggerKind::Automatic,
        };
        self.remote.proxy.rpc_provide_new_symbol_names(self.remote.handle, &model.uri(), range, trigger_kind, token.clone()).then(|names| {
            names.map(|names| {
                names
                    .into_iter()
                    .map(|name| NewSymbolName {
                        new_symbol_name: name.new_symbol_name,
                        tags: name.tags.map(|tags| tags.into_iter().map(|_| NewSymbolNameTag::AIGenerated).collect()),
                    })
                    .collect()
            })
        })
    }
}

// ---- tokens semânticos --------------------------------------------------------------------------------

/// `MainThreadDocumentSemanticTokensProvider`. É `pub` (e reexportado por `main_thread_language_features`) porque o original a
/// exporta e o `extHostSemanticTokens.test.ts` a cria direto, sobre um `RPCProtocol` real; o restante dos provedores deste
/// arquivo é interno ao `MainThreadLanguageFeatures`.
pub struct MainThreadDocumentSemanticTokensProvider {
    pub(super) remote: Remote,
    pub(super) legend: SemanticTokensLegend,
    pub(super) on_did_change: Option<Event<()>>,
}

impl MainThreadDocumentSemanticTokensProvider {
    /// `new MainThreadDocumentSemanticTokensProvider(proxy, handle, legend, onDidChange)`.
    pub fn new(proxy: Rc<dyn ExtHostLanguageFeaturesShape>, handle: u32, legend: SemanticTokensLegend, on_did_change: Option<Event<()>>) -> Self {
        Self { remote: Remote { handle, proxy }, legend, on_did_change }
    }
}

impl DocumentSemanticTokensProvider for MainThreadDocumentSemanticTokensProvider {
    fn on_did_change(&self) -> Option<Event<()>> {
        self.on_did_change.clone()
    }

    fn get_legend(&self) -> SemanticTokensLegend {
        self.legend.clone()
    }

    fn provide_document_semantic_tokens(
        &self,
        model: &Rc<TextModel>,
        last_result_id: Option<&str>,
        token: &CancellationToken,
    ) -> Promise<Option<SemanticTokensResult>> {
        let (remote, uri, token) = (self.remote.clone(), model.uri(), token.clone());
        // `lastResultId ? parseInt(lastResultId, 10) : 0`
        let n_last_result_id = last_result_id.filter(|id| !id.is_empty()).map(parse_int).unwrap_or(0);
        async_fn(async move {
            let Some(encoded_dto) = remote.proxy.rpc_provide_document_semantic_tokens(remote.handle, &uri, n_last_result_id, token.clone()).await? else {
                return Ok(None);
            };
            let dto = decode_semantic_tokens_dto(&encoded_dto);
            if token.is_cancellation_requested() {
                let id = match &dto {
                    SemanticTokensDto::Full(full) => full.id,
                    SemanticTokensDto::Delta(delta) => delta.id,
                };
                remote.proxy.rpc_release_document_semantic_tokens(remote.handle, id);
                return Ok(None);
            }
            Ok(Some(match dto {
                SemanticTokensDto::Full(full) => SemanticTokensResult::Tokens(SemanticTokens { result_id: Some(full.id.to_string()), data: full.data }),
                SemanticTokensDto::Delta(delta) => SemanticTokensResult::Edits(SemanticTokensEdits {
                    result_id: Some(delta.id.to_string()),
                    edits: delta.deltas.into_iter().map(|d| SemanticTokensEdit { start: d.start, delete_count: d.delete_count, data: d.data }).collect(),
                }),
            }))
        })
    }

    fn release_document_semantic_tokens(&self, result_id: Option<&str>) {
        if let Some(result_id) = result_id.filter(|id| !id.is_empty()) {
            self.remote.proxy.rpc_release_document_semantic_tokens(self.remote.handle, parse_int(result_id));
        }
    }
}

/// `parseInt(text, 10)` do `resultId` (`NaN` não existe aqui: o texto vem de `String(dto.id)`).
fn parse_int(text: &str) -> u32 {
    text.trim().parse().unwrap_or(0)
}

/// `MainThreadDocumentRangeSemanticTokensProvider`.
pub(super) struct MainThreadDocumentRangeSemanticTokensProvider {
    pub remote: Remote,
    pub legend: SemanticTokensLegend,
    pub on_did_change: Option<Event<()>>,
}

impl DocumentRangeSemanticTokensProvider for MainThreadDocumentRangeSemanticTokensProvider {
    fn on_did_change(&self) -> Option<Event<()>> {
        self.on_did_change.clone()
    }

    fn get_legend(&self) -> SemanticTokensLegend {
        self.legend.clone()
    }

    fn provide_document_range_semantic_tokens(&self, model: &Rc<TextModel>, range: Range, token: &CancellationToken) -> Promise<Option<SemanticTokens>> {
        let (remote, uri, token) = (self.remote.clone(), model.uri(), token.clone());
        async_fn(async move {
            let Some(encoded_dto) = remote.proxy.rpc_provide_document_range_semantic_tokens(remote.handle, &uri, range, token.clone()).await? else {
                return Ok(None);
            };
            if token.is_cancellation_requested() {
                return Ok(None);
            }
            match decode_semantic_tokens_dto(&encoded_dto) {
                SemanticTokensDto::Full(full) => Ok(Some(SemanticTokens { result_id: Some(full.id.to_string()), data: full.data })),
                SemanticTokensDto::Delta(_) => Err(Error::new("Unexpected")),
            }
        })
    }
}

// ---- completar ----------------------------------------------------------------------------------------

/// `completionProvider`.
pub(super) struct MainThreadCompletionProvider {
    pub remote: Remote,
    pub extension_id: ExtensionIdentifier,
    pub trigger_characters: Vec<String>,
    pub debug_display_name: String,
    pub supports_resolve_details: bool,
}

fn label_text(label: &CompletionLabel) -> &str {
    match label {
        CompletionLabel::Text(text) => text,
        CompletionLabel::Item(item) => &item.label,
    }
}

/// `_inflateSuggestDto(defaultRange, data, extensionId)`.
pub(super) fn inflate_suggest_dto(default_range: CompletionItemRange, data: &ISuggestDataDto, extension_id: &ExtensionIdentifier) -> CompletionItem {
    let label = data.label.clone();
    let command = data.command_id.as_ref().filter(|command_id| !command_id.is_empty()).map(|command_id| Command {
        id: command_id.clone(),
        title: String::new(),
        tooltip: None,
        // Automatically fill in ident as first argument
        arguments: match data.command_ident.as_ref().filter(|ident| !ident.is_empty()) {
            Some(ident) => Some(vec![Dynamic::string(ident)]),
            None => data.command_arguments.clone(),
        },
    });
    let insert_text = data.insert_text.clone().unwrap_or_else(|| label_text(&label).to_owned());
    CompletionItem {
        label,
        extension_id: Some(extension_id.clone()),
        internal_id: data.cache_id,
        kind: data.kind.unwrap_or(CompletionItemKind::Property),
        tags: data.kind_modifier.clone(),
        detail: data.detail.clone(),
        documentation: data.documentation.clone(),
        sort_text: data.sort_text.clone(),
        filter_text: data.filter_text.clone(),
        preselect: data.preselect,
        insert_text,
        range: data.range.unwrap_or(default_range),
        insert_text_rules: data.insert_text_rules,
        commit_characters: data.commit_characters.as_ref().filter(|chars| !chars.is_empty()).map(|chars| chars.chars().map(|c| c.to_string()).collect()),
        additional_text_edits: data.additional_text_edits.as_ref().map(|edits| {
            edits.iter().map(|edit| SingleEditOperation::new(edit.range, Some(edit.text.as_str().into()), false)).collect()
        }),
        command,
        action: None,
    }
}

fn inflate_suggest_result(result: &ISuggestResultDto, extension_id: &ExtensionIdentifier) -> Vec<CompletionItem> {
    let default_range = CompletionItemRange::InsertReplace { insert: result.default_ranges.insert, replace: result.default_ranges.replace };
    result.completions.iter().map(|data| inflate_suggest_dto(default_range, data, extension_id)).collect()
}

impl CompletionItemProvider for MainThreadCompletionProvider {
    fn debug_display_name(&self) -> &str {
        &self.debug_display_name
    }

    fn trigger_characters(&self) -> Option<&[String]> {
        Some(&self.trigger_characters)
    }

    fn provide_completion_items(&self, model: &Rc<TextModel>, position: Position, context: &CompletionContext, token: &CancellationToken) -> ProviderResult<Rc<CompletionList>> {
        let (remote, uri, context, token) = (self.remote.clone(), model.uri(), context.clone(), token.clone());
        let extension_id = self.extension_id.clone();
        async_fn(async move {
            let Some(result) = remote.proxy.rpc_provide_completion_items(remote.handle, &uri, position, context, token).await? else {
                return Ok(None);
            };
            let suggestions = inflate_suggest_result(&result, &extension_id);
            let cache_id = result.cache_id;
            let dispose: Rc<dyn Fn()> = Rc::new(move || {
                if let Some(cache_id) = cache_id {
                    remote.proxy.rpc_release_completion_items(remote.handle, cache_id);
                }
            });
            Ok(Some(Rc::new(CompletionList {
                suggestions,
                incomplete: Some(result.is_incomplete.unwrap_or(false)),
                dispose: Some(dispose),
                duration: Some(result.duration),
            })))
        })
    }

    fn has_resolve_completion_item(&self) -> bool {
        self.supports_resolve_details
    }

    fn resolve_completion_item(&self, suggestion: &CompletionItem, token: &CancellationToken) -> Promise<Option<CompletionItem>> {
        let (remote, suggestion, token) = (self.remote.clone(), suggestion.clone(), token.clone());
        let extension_id = self.extension_id.clone();
        let id = suggestion.internal_id;
        async_fn(async move {
            let Some(id) = id else {
                return Err(Error::new("Cannot resolve a completion item without an id"));
            };
            let Some(result) = remote.proxy.rpc_resolve_completion_item(remote.handle, id, token).await? else {
                return Ok(Some(suggestion));
            };
            // `mixin(suggestion, newSuggestion, true)`: o item inflado traz todos os campos.
            Ok(Some(inflate_suggest_dto(suggestion.range, &result, &extension_id)))
        })
    }
}

// ---- assinatura ---------------------------------------------------------------------------------------

/// `signatureHelpProvider`.
pub(super) struct MainThreadSignatureHelpProvider {
    pub remote: Remote,
    pub trigger_characters: Vec<String>,
    pub retrigger_characters: Vec<String>,
    /// O `id` de cada ajuda devolvida, achado pelo endereço do `Rc` (ver a nota do módulo).
    pub ids: Rc<RefCell<HashMap<usize, u32>>>,
}

impl SignatureHelpProvider for MainThreadSignatureHelpProvider {
    fn signature_help_trigger_characters(&self) -> &[String] {
        &self.trigger_characters
    }

    fn signature_help_retrigger_characters(&self) -> &[String] {
        &self.retrigger_characters
    }

    fn provide_signature_help(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        token: &CancellationToken,
        context: &SignatureHelpContext,
    ) -> ProviderResult<Rc<SignatureHelpResult>> {
        let (remote, uri, token, ids) = (self.remote.clone(), model.uri(), token.clone(), Rc::clone(&self.ids));
        let active_signature_help = context.active_signature_help.as_ref().and_then(|help| {
            let id = ids.borrow().get(&(Rc::as_ptr(help) as usize)).copied()?;
            Some(ISignatureHelpDto { id, signatures: help.signatures.clone(), active_signature: help.active_signature, active_parameter: help.active_parameter })
        });
        let context = ISignatureHelpContextDto {
            trigger_kind: context.trigger_kind,
            trigger_character: context.trigger_character.clone(),
            is_retrigger: context.is_retrigger,
            active_signature_help,
        };
        async_fn(async move {
            let Some(result) = remote.proxy.rpc_provide_signature_help(remote.handle, &uri, position, context, token).await? else {
                return Ok(None);
            };
            let id = result.id;
            let value = Rc::new(SignatureHelp { signatures: result.signatures, active_signature: result.active_signature, active_parameter: result.active_parameter });
            let key = Rc::as_ptr(&value) as usize;
            ids.borrow_mut().insert(key, id);
            Ok(Some(Rc::new(SignatureHelpResult::new(value, move || {
                ids.borrow_mut().remove(&key);
                remote.proxy.rpc_release_signature_help(remote.handle, id);
            }))))
        })
    }
}

// ---- dicas embutidas ----------------------------------------------------------------------------------

/// `inlayHintsProvider`.
pub(super) struct MainThreadInlayHintsProvider {
    pub remote: Remote,
    pub display_name: Option<String>,
    pub supports_resolve: bool,
    pub on_did_change_inlay_hints: Option<Event<()>>,
}

/// `revive<string | languages.InlayHintLabelPart[]>(label)`.
fn inlay_hint_label_from_dto(label: &InlayHintLabelDto) -> InlayHintLabel {
    match label {
        InlayHintLabelDto::Text(text) => InlayHintLabel::Text(text.clone()),
        InlayHintLabelDto::Parts(parts) => InlayHintLabel::Parts(
            parts
                .iter()
                .map(|part| InlayHintLabelPart {
                    label: part.label.clone(),
                    tooltip: part.tooltip.clone(),
                    location: part.location.clone().and_then(revive_location),
                    command: part.command.as_ref().map(command_from_dto),
                })
                .collect(),
        ),
    }
}

fn inlay_hint_from_dto(dto: &IInlayHintDto) -> InlayHint {
    InlayHint {
        label: inlay_hint_label_from_dto(&dto.label),
        tooltip: dto.tooltip.clone(),
        position: dto.position,
        text_edits: dto.text_edits.clone(),
        kind: dto.kind.and_then(|kind| match kind.value() {
            1 => Some(InlayHintKind::Type),
            2 => Some(InlayHintKind::Parameter),
            _ => None,
        }),
        padding_left: dto.padding_left,
        padding_right: dto.padding_right,
        cache_id: dto.cache_id,
    }
}

impl InlayHintsProvider for MainThreadInlayHintsProvider {
    fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    fn on_did_change_inlay_hints(&self) -> Option<Event<()>> {
        self.on_did_change_inlay_hints.clone()
    }

    fn provide_inlay_hints(&self, model: &Rc<TextModel>, range: Range, token: &CancellationToken) -> ProviderResult<Rc<InlayHintList>> {
        let (remote, uri, token) = (self.remote.clone(), model.uri(), token.clone());
        async_fn(async move {
            let Some(result) = remote.proxy.rpc_provide_inlay_hints(remote.handle, &uri, range, token).await? else {
                return Ok(None);
            };
            let hints = result.hints.iter().map(inlay_hint_from_dto).collect();
            let cache_id = result.cache_id;
            Ok(Some(Rc::new(InlayHintList::new(hints, move || {
                if let Some(cache_id) = cache_id.filter(|cache_id| *cache_id != 0) {
                    remote.proxy.rpc_release_inlay_hints(remote.handle, cache_id);
                }
            }))))
        })
    }

    fn has_resolve_inlay_hint(&self) -> bool {
        self.supports_resolve
    }

    fn resolve_inlay_hint(&self, hint: &InlayHint, token: &CancellationToken) -> ProviderResult<InlayHint> {
        let (remote, hint, token) = (self.remote.clone(), hint.clone(), token.clone());
        async_fn(async move {
            let Some(cache_id) = hint.cache_id.filter(|cache_id| cache_id.0 != 0) else {
                return Ok(Some(hint));
            };
            let result = remote.proxy.rpc_resolve_inlay_hint(remote.handle, cache_id, token.clone()).await?;
            if token.is_cancellation_requested() {
                return Err(Error::cancellation());
            }
            let Some(result) = result else {
                return Ok(Some(hint));
            };
            Ok(Some(InlayHint {
                tooltip: result.tooltip.clone(),
                label: inlay_hint_label_from_dto(&result.label),
                text_edits: result.text_edits.clone(),
                ..hint
            }))
        })
    }
}

// ---- links, cores -------------------------------------------------------------------------------------

/// `linkProvider`.
pub(super) struct MainThreadLinkProvider {
    pub remote: Remote,
    pub supports_resolve: bool,
}

impl LinkProvider for MainThreadLinkProvider {
    fn provide_links(&self, model: &Rc<TextModel>, token: &CancellationToken) -> ProviderResult<Rc<LinksList>> {
        let remote = self.remote.clone();
        remote.proxy.rpc_provide_document_links(remote.handle, &model.uri(), token.clone()).then(move |dto| {
            let dto = dto?;
            let links = dto.links.iter().map(revive_link_dto).collect();
            let cache_id = dto.cache_id;
            Some(Rc::new(LinksList::new(links, move || {
                if let Some(cache_id) = cache_id {
                    remote.proxy.rpc_release_document_links(remote.handle, cache_id);
                }
            })))
        })
    }

    fn has_resolve_link(&self) -> bool {
        self.supports_resolve
    }

    fn resolve_link(&self, link: &ILink, token: &CancellationToken) -> ProviderResult<ILink> {
        // `if (!dto.cacheId) return link`
        let Some(cache_id) = link.cache_id.filter(|cache_id| cache_id.0 != 0) else {
            return Promise::resolved(Some(link.clone()));
        };
        self.remote.proxy.rpc_resolve_document_link(self.remote.handle, cache_id, token.clone()).then(|resolved| resolved.map(|dto| revive_link_dto(&dto)))
    }
}

/// `colorProvider`.
pub(super) struct MainThreadColorProvider {
    pub remote: Remote,
}

impl DocumentColorProvider for MainThreadColorProvider {
    fn provide_document_colors(&self, model: &Rc<TextModel>, token: &CancellationToken) -> ProviderResult<Vec<IColorInformation>> {
        self.remote.proxy.rpc_provide_document_colors(self.remote.handle, &model.uri(), token.clone()).then(|document_colors| {
            Some(
                document_colors
                    .into_iter()
                    .map(|document_color| {
                        let [red, green, blue, alpha] = document_color.color;
                        IColorInformation { color: IColor { red, green, blue, alpha }, range: document_color.range }
                    })
                    .collect(),
            )
        })
    }

    fn provide_color_presentations(&self, model: &Rc<TextModel>, color_info: &IColorInformation, token: &CancellationToken) -> ProviderResult<Vec<IColorPresentation>> {
        let raw = IRawColorInfo {
            color: [color_info.color.red, color_info.color.green, color_info.color.blue, color_info.color.alpha],
            range: color_info.range,
        };
        self.remote.proxy.rpc_provide_color_presentations(self.remote.handle, &model.uri(), raw, token.clone()).then(|presentations| {
            presentations.map(|presentations| {
                presentations
                    .into_iter()
                    .map(|IColorPresentationDto { label, text_edit, additional_text_edits }| IColorPresentation { label, text_edit, additional_text_edits })
                    .collect()
            })
        })
    }
}

// ---- dobra, seleção -----------------------------------------------------------------------------------

/// `foldingRangeProvider`.
pub(super) struct MainThreadFoldingRangeProvider {
    pub remote: Remote,
    pub on_did_change: Option<Event<()>>,
}

impl FoldingRangeProvider for MainThreadFoldingRangeProvider {
    fn on_did_change(&self) -> Option<Event<()>> {
        self.on_did_change.clone()
    }

    fn provide_folding_ranges(&self, model: &Rc<TextModel>, _context: &FoldingContext, token: &CancellationToken) -> ProviderResult<Vec<FoldingRange>> {
        self.remote.proxy.rpc_provide_folding_ranges(self.remote.handle, &model.uri(), FoldingContext, token.clone())
    }
}

/// `selectionRangeProvider`.
pub(super) struct MainThreadSelectionRangeProvider {
    pub remote: Remote,
}

impl SelectionRangeProvider for MainThreadSelectionRangeProvider {
    fn provide_selection_ranges(&self, model: &Rc<TextModel>, positions: &[Position], token: &CancellationToken) -> ProviderResult<Vec<Vec<SelectionRange>>> {
        self.remote.proxy.rpc_provide_selection_ranges(self.remote.handle, &model.uri(), positions.to_vec(), token.clone()).then(Some)
    }
}
