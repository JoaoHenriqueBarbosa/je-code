//! Porte de `src/vs/workbench/api/common/extHostLanguageFeatures.ts` (3 060 linhas): o lado da extensão dos
//! provedores de recursos de linguagem. Cada provedor da API ganha um adaptador (a conversão entre o provedor da
//! extensão e os DTOs do protocolo) e a classe [`ExtHostLanguageFeatures`] registra os adaptadores, guarda o mapa de
//! handles e despacha os `$provide*` da thread principal.
//!
//! PARTE 1. Portado: a infraestrutura (`AdapterData`, o mapa de handles, `_createDisposable`, `_withAdapter`,
//! `_nextHandle`, `_transformDocumentSelector`, `_addNewAdapter`, `_extLabel`, `_extId`) e os adaptadores, os métodos
//! `register*Provider` e os `$provide*`/`$release*`/`$resolve*` de:
//! `DocumentSymbol`, `CodeLens`, `Definition`, `Declaration`, `Implementation`, `TypeDefinition`, `Hover`,
//! `DocumentHighlight`, `MultiDocumentHighlight` e `Reference`.
//!
//! PARTE 2. Portado, cada adaptador no submódulo de `ext_host_language_features/` indicado:
//! `CodeActionAdapter` (`code_action`), `DocumentFormattingAdapter`, `RangeFormattingAdapter` e `OnTypeFormattingAdapter`
//! (`formatting`), `NavigateTypeAdapter`, `RenameAdapter` e `NewSymbolNamesAdapter` (`navigate_rename`),
//! `DocumentSemanticTokensAdapter` e `DocumentRangeSemanticTokensAdapter` com o delta de `SemanticTokensEdits`
//! (`semantic_tokens`), `CompletionsAdapter` (`completions`), `SignatureHelpAdapter`, `InlayHintsAdapter`,
//! `LinkProviderAdapter`, `ColorProviderAdapter`, `FoldingProviderAdapter` e `SelectionRangeAdapter` (`hints`),
//! `CallHierarchyAdapter` e `TypeHierarchyAdapter` (`hierarchy`), `EvaluatableExpressionAdapter`, `InlineValuesAdapter` e
//! `LinkedEditingRangeAdapter` (`debug_editing`), `DocumentDropEditAdapter` e `DocumentPasteEditProvider`
//! (`drop_paste`), com os `register*`, `$provide*`, `$resolve*` e `$release*` correspondentes, o `_serializeRegExp` e o
//! `ExtHostDiagnostics` e o `IExtHostApiDeprecationService` do construtor.
//!
//! PENDENTES para a parte 3: `InlineCompletionAdapter` e `ReferenceMap` (que só ele usa), `registerInlineCompletionsProvider`
//! e os `$provideInlineCompletions`, `$handleInlineCompletion*`, `$freeInlineCompletionsList` e
//! `$acceptInlineCompletionsUnificationState`, o estado de unificação das completações em linha
//! (`inlineCompletionsUnificationState` e o seu evento) e, junto dele, a base `Disposable` de `ExtHostLanguageFeatures` (o
//! único `_register` do original é o emissor desse evento), `setLanguageConfiguration` (com os `_serialize*` de
//! configuração) e `$setWordDefinitions`.
//!
//! Divergências:
//! - **Provedores.** Um `vscode.XProvider` é um trait de [`super::ext_host_language_features_api`]; o `ProviderResult<T>`
//!   é `MaybePromise<Option<T>>`. Os quatro provedores de localização (definição, declaração, implementação e tipo da
//!   definição) têm a mesma forma e um adaptador só, [`LocationLinkAdapter`], parametrizado pelo tipo marcador do
//!   registro ([`DefinitionKind`], [`DeclarationKind`], [`ImplementationKind`], [`TypeDefinitionKind`]): o
//!   `instanceof` do `_withAdapter` continua distinguindo um do outro.
//! - **`Adapter` e o `instanceof`.** A união `Adapter` é o enum [`Adapter`], e o `data.adapter instanceof ctor` é o
//!   [`AdapterKind::from_adapter`], que devolve `None` quando o handle guarda outro tipo de adaptador.
//! - **`_withAdapter`.** O `callback.toString()` do log de `INVOKE` não existe em Rust: quem chama passa o texto da
//!   função de seta do original (`invocation`). O `Date.now()` é um [`Instant`]. O `fallbackValue` é devolvido, em
//!   promessa já resolvida, quando o handle não existe ou guarda outro adaptador.
//! - **Handles.** `_handlePool` é estático no original e aqui é um `thread_local`. Os `UriComponents` são [`URI`] e o
//!   `URI.revive` é a identidade.
//! - **Dispose.** O `vscode.Disposable` que os `register*` devolvem é um [`DisposableRef`] (um [`types::Disposable`]); o
//!   `Disposable.from(result, subscription)` do `CodeLens` é o [`types::Disposable::from`].
//! - **Parâmetro `extensionId`** de `registerHoverProvider` (nunca lido no original) não existe.
//! - **Verificações de tipo em tempo de execução** (`Range.isRange(lens.range)` do `CodeLensAdapter`) não existem: o tipo
//!   as garante.
//! - **Erros.** Onde o original lança `TypeError` ao ler uma propriedade ausente (`SymbolInformation.location.range`
//!   indefinido), o adaptador devolve o erro na promessa.
//! - **Parte 2, eventos.** O padrão `Disposable.from(result, provider.onDidChangeX(_ => proxy.$emitXEvent(eventHandle)))`
//!   (valores embutidos, tokens semânticos, dicas embutidas, dobra) é o [`ExtHostLanguageFeatures::dispose_with_event`].
//!   O `eventHandle` é `Option<u32>`; o `if (eventHandle)` dos tokens semânticos (falso para `0`) nunca vê `0`, porque o
//!   handle do adaptador é tirado antes.
//! - **Parte 2, parâmetros ignorados.** `extensionId?` de `registerEvaluatableExpressionProvider` e de
//!   `registerInlineValuesProvider` (nunca lido) não existe, como o de `registerHoverProvider`. O
//!   `OnTypeFormattingAdapter.autoFormatTriggerCharacters` (`[]`, marcado `// not here`) não existe.
//! - **Parte 2, `$provide*` sem corrida.** Os que o original chama com `undefined` no `tokenToRaceAgainst`
//!   (`$resolveCodeAction`, `$resolveWorkspaceSymbol`, `$provideDocumentOnDropEdits`...) passam `None`.
//! - **Parte 2, `_serializeRegExp`.** Recebe o `RegExp` (o [`Dynamic`] dele) e devolve `None` para outro valor.

mod code_action;
mod completions;
mod debug_editing;
mod drop_paste;
mod formatting;
mod hierarchy;
mod hints;
mod navigate_rename;
mod semantic_tokens;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::Instant;

use je_base::async_::{async_fn, await_maybe, race_cancellation_error};
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error, is_cancellation_error};
use je_base::event::Event;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::uri_ipc::IURITransformer;
use je_editor::code_action::languages::{CodeActionContext, TextEdit as EditorTextEdit};
use je_editor::folding::syntax_range_provider::{FoldingContext, FoldingRange as EditorFoldingRange};
use je_editor::language_providers::{
    DocumentHighlight as EditorDocumentHighlight, DocumentSymbol as EditorDocumentSymbol, SelectionRange as EditorSelectionRange,
};
use je_editor::languages::CompletionContext as EditorCompletionContext;
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogMessage, LogServiceRef, Logger};

use self::code_action::CodeActionAdapter;
use self::completions::CompletionsAdapter;
use self::debug_editing::{EvaluatableExpressionAdapter, InlineValuesAdapter, LinkedEditingRangeAdapter};
use self::drop_paste::{DocumentDropEditAdapter, DocumentPasteEditProvider};
use self::formatting::{DocumentFormattingAdapter, OnTypeFormattingAdapter, RangeFormattingAdapter};
use self::hierarchy::{CallHierarchyAdapter, TypeHierarchyAdapter};
use self::hints::{ColorProviderAdapter, FoldingProviderAdapter, InlayHintsAdapter, LinkProviderAdapter, SelectionRangeAdapter, SignatureHelpAdapter};
use self::navigate_rename::{NavigateTypeAdapter, NewSymbolNamesAdapter, RenameAdapter};
use self::semantic_tokens::{DocumentRangeSemanticTokensAdapter, DocumentSemanticTokensAdapter};
use super::cache::Cache;
use super::ext_host_commands::ExtHostCommands;
use super::ext_host_diagnostics::ExtHostDiagnostics;
use super::ext_host_documents::ExtHostDocuments;
use super::ext_host_language_features_api::{
    ApiCallHierarchyProvider, ApiCodeActionProvider, ApiCodeLensProvider, ApiCompletionItemProvider, ApiDocumentColorProvider,
    ApiDocumentDropEditProvider, ApiDocumentFormattingEditProvider, ApiDocumentHighlightProvider, ApiDocumentLinkProvider, ApiDocumentPasteEditProvider,
    ApiDocumentRangeFormattingEditProvider, ApiDocumentRangeSemanticTokensProvider, ApiDocumentSemanticTokensProvider, ApiDocumentSymbolProvider,
    ApiDocumentSymbols, ApiEvaluatableExpressionProvider, ApiFoldingRangeProvider, ApiHoverContext, ApiHoverProvider, ApiInlayHintsProvider,
    ApiInlineValuesProvider, ApiLinkedEditingRangeProvider, ApiLocationLinkProvider, ApiLocationResult, ApiMultiDocumentHighlightProvider,
    ApiNewSymbolNamesProvider, ApiOnTypeFormattingEditProvider, ApiReferenceProvider, ApiRenameProvider, ApiSelectionRangeProvider,
    ApiSignatureHelpProvider, ApiTypeHierarchyProvider, ApiWorkspaceSymbolProvider, CodeActionProviderMetadata, CodeLensRef,
    DocumentDropEditProviderMetadata, DocumentPasteProviderMetadata, DocumentSymbolProviderMetadata, IExtHostApiDeprecationService,
    SignatureHelpMetadataOrTriggerChars,
};
use super::ext_host_language_features_converters::{DefinitionLink, DocumentSelector, Hover, MultiDocumentHighlight};
use super::ext_host_protocol::{
    ChainedCacheId, CodeActionDocumentationDto, DataTransferDto, EvaluatableExpressionDto, ExtHostLanguageFeaturesShape, FormattingOptions, HoverContextDto, HoverWithId,
    ICodeActionListDto, ICodeActionProviderMetadataDto, ICodeLensDto, ICodeLensListDto, IDocumentDropEditDto, IDocumentDropEditProviderMetadata,
    IDocumentFilterDto, IDocumentPasteContextDto, IExtHostTelemetry, IInlayHintDto, IInlayHintsDto, IInlineValueContextDto, ILinkDto, ILinkedEditingRangesDto,
    ILinksListDto, ILocationDto, ILocationLinkDto, IMainContext, IPasteEditDto, IRawColorInfo, IRegExpDto, ISignatureHelpContextDto, ISignatureHelpDto,
    ISignatureHelpProviderMetadataDto, ISuggestDataDto, ISuggestResultDto, IWorkspaceSymbolDto, IWorkspaceSymbolsDto, IColorPresentationDto,
    ICallHierarchyItemDto, IIncomingCallDto, IOutgoingCallDto, ITypeHierarchyItemDto, IPasteEditProviderMetadataDto, InlineValueDto,
    LanguagesNewSymbolNameTriggerKind, MainContext, MainThreadLanguageFeaturesShape, MultiDocumentHighlightDto, NewSymbolNameDto, RangeOrSelection,
    ReferenceContext, RenameEditsDto, RenameLocationDto, ResolvedCodeActionDto, ResolvedDropEditDto, ResolvedPasteEditDto,
};
use super::ext_host_type_converters::{self as type_convert, DocumentSelectorInput, ICommandsConverter};
use super::ext_host_types::{self as types, SemanticTokensLegend, SymbolInformation, VerboseHover};

// --- adapter

/// `DocumentSymbolAdapter`.
struct DocumentSymbolAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiDocumentSymbolProvider>,
}

impl DocumentSymbolAdapter {
    fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiDocumentSymbolProvider>) -> Rc<Self> {
        Rc::new(Self { documents, provider })
    }

    fn provide_document_symbols(self: &Rc<Self>, resource: URI, token: CancellationToken) -> Promise<Option<Vec<EditorDocumentSymbol>>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let value = await_maybe(this.provider.provide_document_symbols(&doc, token)).await?;
            match value {
                Some(ApiDocumentSymbols::Symbols(symbols)) if !symbols.is_empty() => {
                    Ok(Some(symbols.iter().map(type_convert::DocumentSymbol::from).collect()))
                }
                Some(ApiDocumentSymbols::Information(infos)) if !infos.is_empty() => Self::as_document_symbol_tree(infos).map(Some),
                _ => Ok(None),
            }
        })
    }

    fn as_document_symbol_tree(infos: Vec<SymbolInformation>) -> Result<Vec<EditorDocumentSymbol>, Error> {
        // first sort by start (and end) and then loop over all elements
        // and build a tree based on containment.
        let mut entries = Vec::with_capacity(infos.len());
        for info in infos {
            let range = info.location.range.ok_or_else(|| Error::type_error("Cannot read properties of undefined (reading 'start')"))?;
            entries.push((info, range));
        }
        entries.sort_by(|(_, a), (_, b)| {
            let mut res = a.start().compare_to(&b.start());
            if res == 0 {
                res = b.end().compare_to(&a.end());
            }
            res.cmp(&0)
        });

        // Os nós ficam numa lista e se referem por índice: o `parentStack` do original guarda os mesmos objetos que
        // `res` e `children` guardam, o que em Rust é o índice na lista.
        struct Node {
            symbol: EditorDocumentSymbol,
            children: Vec<usize>,
        }
        let mut nodes: Vec<Node> = Vec::with_capacity(entries.len());
        let mut roots: Vec<usize> = Vec::new();
        let mut parent_stack: Vec<usize> = Vec::new();
        for (info, range) in &entries {
            let editor_range = type_convert::Range::from(range);
            let element = nodes.len();
            nodes.push(Node {
                symbol: EditorDocumentSymbol {
                    name: if info.name.is_empty() { "!!MISSING: name!!".to_owned() } else { info.name.clone() },
                    kind: type_convert::SymbolKind::from(info.kind),
                    tags: info.tags.as_ref().map(|tags| tags.iter().copied().map(type_convert::SymbolTag::from).collect()).unwrap_or_default(),
                    detail: String::new(),
                    container_name: info.container_name.clone(),
                    range: editor_range,
                    selection_range: editor_range,
                    children: Some(Vec::new()),
                },
                children: Vec::new(),
            });

            loop {
                let Some(&parent) = parent_stack.last() else {
                    parent_stack.push(element);
                    roots.push(element);
                    break;
                };
                let parent_range: EditorRange = nodes[parent].symbol.range;
                if parent_range.contains_range(&editor_range) && parent_range != editor_range {
                    nodes[parent].children.push(element);
                    parent_stack.push(element);
                    break;
                }
                parent_stack.pop();
            }
        }

        fn assemble(index: usize, nodes: &mut [Option<Node>]) -> EditorDocumentSymbol {
            let node = nodes[index].take().expect("cada nó entra na árvore uma única vez");
            let children = node.children.iter().map(|&child| assemble(child, nodes)).collect();
            EditorDocumentSymbol { children: Some(children), ..node.symbol }
        }
        let mut nodes: Vec<Option<Node>> = nodes.into_iter().map(Some).collect();
        Ok(roots.into_iter().map(|root| assemble(root, &mut nodes)).collect())
    }
}

/// `CodeLensAdapter`.
struct CodeLensAdapter {
    cache: Cache<CodeLensRef>,
    disposables: RefCell<HashMap<u32, Rc<DisposableStore>>>,
    documents: Rc<ExtHostDocuments>,
    commands: Rc<dyn ICommandsConverter>,
    provider: Rc<dyn ApiCodeLensProvider>,
    extension: Rc<ExtensionDescription>,
    ext_telemetry: Rc<dyn IExtHostTelemetry>,
    log_service: LogServiceRef,
}

impl CodeLensAdapter {
    fn new(
        documents: Rc<ExtHostDocuments>,
        commands: Rc<dyn ICommandsConverter>,
        provider: Rc<dyn ApiCodeLensProvider>,
        extension: Rc<ExtensionDescription>,
        ext_telemetry: Rc<dyn IExtHostTelemetry>,
        log_service: LogServiceRef,
    ) -> Rc<Self> {
        Rc::new(Self {
            cache: Cache::new("CodeLens"),
            disposables: RefCell::new(HashMap::new()),
            documents,
            commands,
            provider,
            extension,
            ext_telemetry,
            log_service,
        })
    }

    fn provide_code_lenses(self: &Rc<Self>, resource: URI, token: CancellationToken) -> Promise<Option<ICodeLensListDto>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;

            let lenses = await_maybe(this.provider.provide_code_lenses(&doc, token.clone())).await?;
            let Some(lenses) = lenses.filter(|_| !token.is_cancellation_requested()) else {
                return Ok(None);
            };
            let cache_id = this.cache.add(lenses.clone());
            let disposables = DisposableStore::new();
            this.disposables.borrow_mut().insert(cache_id, disposables.clone());
            let mut result = ICodeLensListDto { cache_id: Some(cache_id), lenses: Vec::new() };
            for (i, lens) in lenses.iter().enumerate() {
                let lens = lens.borrow();
                result.lenses.push(ICodeLensDto {
                    cache_id: Some((cache_id, i as u32)),
                    range: type_convert::Range::from(&lens.range),
                    id: None,
                    command: this.commands.to_internal(lens.command.as_ref(), &disposables)?,
                });
            }
            Ok(Some(result))
        })
    }

    fn resolve_code_lens(self: &Rc<Self>, mut symbol: ICodeLensDto, token: CancellationToken) -> Promise<Option<ICodeLensDto>> {
        let this = self.clone();
        async_fn(async move {
            let Some(lens) = symbol.cache_id.and_then(|(cache_id, index)| this.cache.get(cache_id, index as usize)) else {
                return Ok(None);
            };

            let resolved_lens = if !this.provider.has_resolve_code_lens() || lens.borrow().is_resolved() {
                Some(lens.clone())
            } else {
                await_maybe(this.provider.resolve_code_lens(&lens, token.clone())).await?
            };
            let resolved_lens = resolved_lens.unwrap_or(lens);

            if token.is_cancellation_requested() {
                return Ok(None);
            }
            let disposables = symbol.cache_id.and_then(|(cache_id, _)| this.disposables.borrow().get(&cache_id).cloned());
            let Some(disposables) = disposables else {
                // disposed in the meantime
                return Ok(None);
            };

            let command = resolved_lens.borrow().command.clone();
            let Some(command) = command else {
                let error = Error::new(format!("INVALID code lens resolved, lacks command: {}", this.extension.identifier.value));
                this.ext_telemetry.on_extension_error(&this.extension.identifier, &error);
                this.log_service.error_args(&LogMessage::Error(error), &[]);
                return Ok(None);
            };

            symbol.command = this.commands.to_internal(Some(&command), &disposables)?;
            Ok(Some(symbol))
        })
    }

    fn release_code_lenses(&self, cached_id: u32) {
        if let Some(disposables) = self.disposables.borrow_mut().remove(&cached_id) {
            disposables.dispose();
        }
        self.cache.delete(cached_id);
    }
}

/// `convertToLocationLinks`.
fn convert_to_location_links(value: Option<ApiLocationResult>) -> Result<Vec<ILocationLinkDto>, Error> {
    match value {
        Some(ApiLocationResult::Many(links)) => links.iter().map(DefinitionLink::from).collect(),
        Some(ApiLocationResult::Single(link)) => Ok(vec![DefinitionLink::from(&link)?]),
        None => Ok(Vec::new()),
    }
}

/// O tipo marcador de qual registro um [`LocationLinkAdapter`] atende.
trait LocationLinkKind: 'static {}

/// Marcador de `DefinitionAdapter`.
struct DefinitionKind;
/// Marcador de `DeclarationAdapter`.
struct DeclarationKind;
/// Marcador de `ImplementationAdapter`.
struct ImplementationKind;
/// Marcador de `TypeDefinitionAdapter`.
struct TypeDefinitionKind;

impl LocationLinkKind for DefinitionKind {}
impl LocationLinkKind for DeclarationKind {}
impl LocationLinkKind for ImplementationKind {}
impl LocationLinkKind for TypeDefinitionKind {}

/// `DefinitionAdapter`, `DeclarationAdapter`, `ImplementationAdapter` e `TypeDefinitionAdapter`: os quatro do original
/// têm o mesmo corpo (só muda o nome do método do provedor).
struct LocationLinkAdapter<K> {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiLocationLinkProvider>,
    kind: PhantomData<K>,
}

impl<K: LocationLinkKind> LocationLinkAdapter<K> {
    fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiLocationLinkProvider>) -> Rc<Self> {
        Rc::new(Self { documents, provider, kind: PhantomData })
    }

    /// `provideDefinition`, `provideDeclaration`, `provideImplementation` e `provideTypeDefinition`.
    fn provide_location_links(self: &Rc<Self>, resource: URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let pos = type_convert::Position::to(&position)?;
            let value = await_maybe(this.provider.provide_location_links(&doc, &pos, token)).await?;
            convert_to_location_links(value)
        })
    }
}

/// `HoverAdapter`.
struct HoverAdapter {
    hover_counter: Cell<u32>,
    hover_map: RefCell<BTreeMap<u32, VerboseHover>>,
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiHoverProvider>,
}

impl HoverAdapter {
    const HOVER_MAP_MAX_SIZE: usize = 10;

    fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiHoverProvider>) -> Rc<Self> {
        Rc::new(Self { hover_counter: Cell::new(0), hover_map: RefCell::new(BTreeMap::new()), documents, provider })
    }

    fn provide_hover(self: &Rc<Self>, resource: URI, position: EditorPosition, context: Option<HoverContextDto>, token: CancellationToken) -> Promise<Option<HoverWithId>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let pos = type_convert::Position::to(&position)?;

            let value = match context.and_then(|context| context.verbosity_request) {
                Some(verbosity_request) => {
                    let previous_hover_id = verbosity_request.previous_hover.id;
                    let previous_hover = this.hover_map.borrow().get(&previous_hover_id).cloned();
                    let Some(previous_hover) = previous_hover else {
                        return Err(Error::new(format!("Hover with id {previous_hover_id} not found")));
                    };
                    let hover_context = ApiHoverContext { verbosity_delta: verbosity_request.verbosity_delta, previous_hover };
                    await_maybe(this.provider.provide_hover(&doc, &pos, token, Some(&hover_context))).await?
                }
                None => await_maybe(this.provider.provide_hover(&doc, &pos, token, None)).await?,
            };
            let Some(mut value) = value.filter(|value| !value.hover.contents.is_empty()) else {
                return Ok(None);
            };
            if value.hover.range.is_none() {
                value.hover.range = doc.get_word_range_at_position(&pos, None)?;
            }
            if value.hover.range.is_none() {
                value.hover.range = Some(types::Range::new(pos, pos));
            }
            let converted_hover = Hover::from(&value);
            let id = this.hover_counter.get();
            // Check if hover map has more than 10 elements and if yes, remove oldest from the map
            {
                let mut hover_map = this.hover_map.borrow_mut();
                if hover_map.len() == Self::HOVER_MAP_MAX_SIZE {
                    if let Some(minimum_id) = hover_map.keys().next().copied() {
                        hover_map.remove(&minimum_id);
                    }
                }
                hover_map.insert(id, value);
            }
            this.hover_counter.set(id + 1);
            Ok(Some(HoverWithId { hover: converted_hover, id }))
        })
    }

    fn release_hover(&self, id: u32) {
        self.hover_map.borrow_mut().remove(&id);
    }
}

/// `DocumentHighlightAdapter`.
struct DocumentHighlightAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiDocumentHighlightProvider>,
}

impl DocumentHighlightAdapter {
    fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiDocumentHighlightProvider>) -> Rc<Self> {
        Rc::new(Self { documents, provider })
    }

    fn provide_document_highlights(self: &Rc<Self>, resource: URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<EditorDocumentHighlight>>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let pos = type_convert::Position::to(&position)?;

            let value = await_maybe(this.provider.provide_document_highlights(&doc, &pos, token)).await?;
            Ok(value.map(|highlights| highlights.iter().map(type_convert::DocumentHighlight::from).collect()))
        })
    }
}

/// `MultiDocumentHighlightAdapter`.
struct MultiDocumentHighlightAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiMultiDocumentHighlightProvider>,
    log_service: LogServiceRef,
}

impl MultiDocumentHighlightAdapter {
    fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiMultiDocumentHighlightProvider>, log_service: LogServiceRef) -> Rc<Self> {
        Rc::new(Self { documents, provider, log_service })
    }

    fn provide_multi_document_highlights(
        self: &Rc<Self>,
        resource: URI,
        position: EditorPosition,
        other_resources: Vec<URI>,
        token: CancellationToken,
    ) -> Promise<Option<Vec<MultiDocumentHighlightDto>>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let other_documents: Vec<_> = other_resources
                .iter()
                .filter_map(|other| match this.documents.get_document(other) {
                    Ok(document) => Some(document),
                    Err(err) => {
                        this.log_service.error(&format!("Error: Unable to retrieve document from URI: {}. Error message: {err}", other.to_string(false).to_string_lossy()));
                        None
                    }
                })
                .collect();

            let pos = type_convert::Position::to(&position)?;

            let value = await_maybe(this.provider.provide_multi_document_highlights(&doc, &pos, &other_documents, token)).await?;
            Ok(value.map(|highlights| highlights.iter().map(MultiDocumentHighlight::from).collect()))
        })
    }
}

/// `ReferenceAdapter`.
struct ReferenceAdapter {
    documents: Rc<ExtHostDocuments>,
    provider: Rc<dyn ApiReferenceProvider>,
}

impl ReferenceAdapter {
    fn new(documents: Rc<ExtHostDocuments>, provider: Rc<dyn ApiReferenceProvider>) -> Rc<Self> {
        Rc::new(Self { documents, provider })
    }

    fn provide_references(self: &Rc<Self>, resource: URI, position: EditorPosition, context: ReferenceContext, token: CancellationToken) -> Promise<Option<Vec<ILocationDto>>> {
        let this = self.clone();
        async_fn(async move {
            let doc = this.documents.get_document(&resource)?;
            let pos = type_convert::Position::to(&position)?;

            let value = await_maybe(this.provider.provide_references(&doc, &pos, &context, token)).await?;
            Ok(value.map(|locations| locations.iter().map(type_convert::Location::from).collect()))
        })
    }
}

/// `type Adapter`: a união dos adaptadores.
#[derive(Clone)]
enum Adapter {
    DocumentSymbol(Rc<DocumentSymbolAdapter>),
    CodeLens(Rc<CodeLensAdapter>),
    Definition(Rc<LocationLinkAdapter<DefinitionKind>>),
    Declaration(Rc<LocationLinkAdapter<DeclarationKind>>),
    Implementation(Rc<LocationLinkAdapter<ImplementationKind>>),
    TypeDefinition(Rc<LocationLinkAdapter<TypeDefinitionKind>>),
    Hover(Rc<HoverAdapter>),
    DocumentHighlight(Rc<DocumentHighlightAdapter>),
    MultiDocumentHighlight(Rc<MultiDocumentHighlightAdapter>),
    Reference(Rc<ReferenceAdapter>),
    EvaluatableExpression(Rc<EvaluatableExpressionAdapter>),
    InlineValues(Rc<InlineValuesAdapter>),
    LinkedEditingRange(Rc<LinkedEditingRangeAdapter>),
    CodeAction(Rc<CodeActionAdapter>),
    DocumentPasteEdit(Rc<DocumentPasteEditProvider>),
    DocumentFormatting(Rc<DocumentFormattingAdapter>),
    RangeFormatting(Rc<RangeFormattingAdapter>),
    OnTypeFormatting(Rc<OnTypeFormattingAdapter>),
    NavigateType(Rc<NavigateTypeAdapter>),
    Rename(Rc<RenameAdapter>),
    NewSymbolNames(Rc<NewSymbolNamesAdapter>),
    DocumentSemanticTokens(Rc<DocumentSemanticTokensAdapter>),
    DocumentRangeSemanticTokens(Rc<DocumentRangeSemanticTokensAdapter>),
    Completions(Rc<CompletionsAdapter>),
    SignatureHelp(Rc<SignatureHelpAdapter>),
    InlayHints(Rc<InlayHintsAdapter>),
    LinkProvider(Rc<LinkProviderAdapter>),
    ColorProvider(Rc<ColorProviderAdapter>),
    FoldingProvider(Rc<FoldingProviderAdapter>),
    SelectionRange(Rc<SelectionRangeAdapter>),
    CallHierarchy(Rc<CallHierarchyAdapter>),
    TypeHierarchy(Rc<TypeHierarchyAdapter>),
    DocumentDropEdit(Rc<DocumentDropEditAdapter>),
}

/// O `ctor` de `data.adapter instanceof ctor`: reconhece o adaptador de um tipo.
trait AdapterKind: Sized {
    fn from_adapter(adapter: &Adapter) -> Option<Rc<Self>>;
}

macro_rules! adapter_kinds {
    ($($variant:ident => $adapter:ty),+ $(,)?) => {
        $(
            impl AdapterKind for $adapter {
                fn from_adapter(adapter: &Adapter) -> Option<Rc<Self>> {
                    match adapter {
                        Adapter::$variant(adapter) => Some(adapter.clone()),
                        _ => None,
                    }
                }
            }

            impl From<Rc<$adapter>> for Adapter {
                fn from(adapter: Rc<$adapter>) -> Self {
                    Adapter::$variant(adapter)
                }
            }
        )+
    };
}

adapter_kinds! {
    DocumentSymbol => DocumentSymbolAdapter,
    CodeLens => CodeLensAdapter,
    Definition => LocationLinkAdapter<DefinitionKind>,
    Declaration => LocationLinkAdapter<DeclarationKind>,
    Implementation => LocationLinkAdapter<ImplementationKind>,
    TypeDefinition => LocationLinkAdapter<TypeDefinitionKind>,
    Hover => HoverAdapter,
    DocumentHighlight => DocumentHighlightAdapter,
    MultiDocumentHighlight => MultiDocumentHighlightAdapter,
    Reference => ReferenceAdapter,
    EvaluatableExpression => EvaluatableExpressionAdapter,
    InlineValues => InlineValuesAdapter,
    LinkedEditingRange => LinkedEditingRangeAdapter,
    CodeAction => CodeActionAdapter,
    DocumentPasteEdit => DocumentPasteEditProvider,
    DocumentFormatting => DocumentFormattingAdapter,
    RangeFormatting => RangeFormattingAdapter,
    OnTypeFormatting => OnTypeFormattingAdapter,
    NavigateType => NavigateTypeAdapter,
    Rename => RenameAdapter,
    NewSymbolNames => NewSymbolNamesAdapter,
    DocumentSemanticTokens => DocumentSemanticTokensAdapter,
    DocumentRangeSemanticTokens => DocumentRangeSemanticTokensAdapter,
    Completions => CompletionsAdapter,
    SignatureHelp => SignatureHelpAdapter,
    InlayHints => InlayHintsAdapter,
    LinkProvider => LinkProviderAdapter,
    ColorProvider => ColorProviderAdapter,
    FoldingProvider => FoldingProviderAdapter,
    SelectionRange => SelectionRangeAdapter,
    CallHierarchy => CallHierarchyAdapter,
    TypeHierarchy => TypeHierarchyAdapter,
    DocumentDropEdit => DocumentDropEditAdapter,
}

/// `AdapterData`.
struct AdapterData {
    adapter: Adapter,
    extension: Rc<ExtensionDescription>,
}

thread_local! {
    /// `ExtHostLanguageFeatures._handlePool`.
    static HANDLE_POOL: Cell<u32> = const { Cell::new(0) };
}

/// `ExtHostLanguageFeatures`.
pub struct ExtHostLanguageFeatures {
    proxy: Rc<dyn MainThreadLanguageFeaturesShape>,
    adapters: Rc<RefCell<HashMap<u32, AdapterData>>>,
    uri_transformer: Option<Rc<dyn IURITransformer>>,
    documents: Rc<ExtHostDocuments>,
    commands: Rc<ExtHostCommands>,
    diagnostics: Rc<ExtHostDiagnostics>,
    log_service: LogServiceRef,
    api_deprecation: Rc<dyn IExtHostApiDeprecationService>,
    extension_telemetry: Rc<dyn IExtHostTelemetry>,
}

impl ExtHostLanguageFeatures {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        main_context: &dyn IMainContext,
        uri_transformer: Option<Rc<dyn IURITransformer>>,
        documents: Rc<ExtHostDocuments>,
        commands: Rc<ExtHostCommands>,
        diagnostics: Rc<ExtHostDiagnostics>,
        log_service: LogServiceRef,
        api_deprecation: Rc<dyn IExtHostApiDeprecationService>,
        extension_telemetry: Rc<dyn IExtHostTelemetry>,
    ) -> Rc<Self> {
        Rc::new(Self {
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_LANGUAGE_FEATURES),
            adapters: Rc::new(RefCell::new(HashMap::new())),
            uri_transformer,
            documents,
            commands,
            diagnostics,
            log_service,
            api_deprecation,
            extension_telemetry,
        })
    }

    fn transform_document_selector(&self, selector: &DocumentSelectorInput, extension: &ExtensionDescription) -> Vec<IDocumentFilterDto> {
        DocumentSelector::from(selector, self.uri_transformer.as_deref(), Some(extension))
    }

    fn create_disposable(&self, handle: u32) -> DisposableRef {
        let adapters = self.adapters.clone();
        let proxy = self.proxy.clone();
        Rc::new(types::Disposable::new(move || {
            adapters.borrow_mut().remove(&handle);
            proxy.rpc_unregister(handle);
        }))
    }

    fn next_handle() -> u32 {
        HANDLE_POOL.with(|pool| {
            let handle = pool.get();
            pool.set(handle + 1);
            handle
        })
    }

    /// `_withAdapter`. `invocation` é o texto da função de seta do original (o que o log de `INVOKE` mostra).
    fn with_adapter<A: AdapterKind + 'static, R: Clone + 'static>(
        &self,
        handle: u32,
        invocation: &str,
        callback: impl FnOnce(Rc<A>, Rc<ExtensionDescription>) -> Promise<R>,
        fallback_value: R,
        token_to_race_against: Option<&CancellationToken>,
        do_not_log: bool,
    ) -> Promise<R> {
        let data = self.adapters.borrow().get(&handle).map(|data| (data.adapter.clone(), data.extension.clone()));
        let Some((adapter, extension)) = data else {
            return Promise::resolved(fallback_value);
        };
        let Some(adapter) = A::from_adapter(&adapter) else {
            return Promise::resolved(fallback_value);
        };

        let t1 = Instant::now();
        if !do_not_log {
            self.log_service.trace(&format!("[{}] INVOKE provider '{}'", extension.identifier.value, invocation));
        }

        let result = callback(adapter, extension.clone());

        // logging,tracing
        let log_service = self.log_service.clone();
        let extension_telemetry = self.extension_telemetry.clone();
        let _ = result.then_result(move |outcome| {
            if let Err(err) = &outcome {
                if !is_cancellation_error(err) {
                    log_service.error(&format!("[{}] provider FAILED", extension.identifier.value));
                    log_service.error_args(&LogMessage::Error(err.clone()), &[]);

                    extension_telemetry.on_extension_error(&extension.identifier, err);
                }
            }
            if !do_not_log {
                log_service.trace(&format!("[{}] provider DONE after {}ms", extension.identifier.value, t1.elapsed().as_millis()));
            }
            Ok(())
        });

        match token_to_race_against {
            Some(token) => race_cancellation_error(&result, token),
            None => result,
        }
    }

    /// `_addNewAdapter`.
    fn add_new_adapter(&self, adapter: impl Into<Adapter>, extension: &ExtensionDescription) -> u32 {
        let handle = Self::next_handle();
        self.set_adapter(handle, adapter, Rc::new(extension.clone()));
        handle
    }

    fn set_adapter(&self, handle: u32, adapter: impl Into<Adapter>, extension: Rc<ExtensionDescription>) {
        self.adapters.borrow_mut().insert(handle, AdapterData { adapter: adapter.into(), extension });
    }

    /// O corpo comum dos `register*Provider` simples: guarda o adaptador, registra o seletor na thread principal e
    /// devolve o `Disposable` que desfaz as duas coisas.
    fn register_provider(
        &self,
        adapter: impl Into<Adapter>,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        register: impl FnOnce(&dyn MainThreadLanguageFeaturesShape, u32, Vec<IDocumentFilterDto>),
    ) -> DisposableRef {
        let handle = self.add_new_adapter(adapter, extension);
        register(&*self.proxy, handle, self.transform_document_selector(selector, extension));
        self.create_disposable(handle)
    }

    /// `_extLabel`: `ext.displayName || ext.name`.
    fn ext_label(extension: &ExtensionDescription) -> String {
        match &extension.manifest.display_name {
            Some(display_name) if !display_name.is_empty() => display_name.clone(),
            _ => extension.manifest.name.clone(),
        }
    }

    /// `_extId`.
    #[allow(dead_code)]
    fn ext_id(extension: &ExtensionDescription) -> &str {
        &extension.identifier.value
    }

    // --- outline

    pub fn register_document_symbol_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiDocumentSymbolProvider>,
        metadata: Option<&DocumentSymbolProviderMetadata>,
    ) -> DisposableRef {
        let display_name = metadata.and_then(|metadata| metadata.label.clone()).filter(|label| !label.is_empty()).unwrap_or_else(|| Self::ext_label(extension));
        self.register_provider(DocumentSymbolAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_document_symbol_provider(handle, selector, &display_name);
        })
    }

    // --- code lens

    pub fn register_code_lens_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiCodeLensProvider>) -> DisposableRef {
        let handle = Self::next_handle();
        let on_did_change_code_lenses = provider.on_did_change_code_lenses();
        let event_handle = on_did_change_code_lenses.as_ref().map(|_| Self::next_handle());

        let extension_ref = Rc::new(extension.clone());
        let commands: Rc<dyn ICommandsConverter> = self.commands.converter();
        let adapter = CodeLensAdapter::new(
            self.documents.clone(),
            commands,
            provider,
            extension_ref.clone(),
            self.extension_telemetry.clone(),
            self.log_service.clone(),
        );
        self.set_adapter(handle, adapter, extension_ref);
        self.proxy.rpc_register_code_lens_support(handle, self.transform_document_selector(selector, extension), event_handle);
        let result = self.create_disposable(handle);

        match (on_did_change_code_lenses, event_handle) {
            (Some(event), Some(event_handle)) => {
                let proxy = self.proxy.clone();
                let subscription = event.listen(move |_| proxy.rpc_emit_code_lens_event(event_handle, None));
                Rc::new(types::Disposable::from(vec![Some(result), Some(subscription)]))
            }
            _ => result,
        }
    }

    // --- declaration

    pub fn register_definition_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiLocationLinkProvider>) -> DisposableRef {
        self.register_provider(LocationLinkAdapter::<DefinitionKind>::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_definition_support(handle, selector);
        })
    }

    pub fn register_declaration_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiLocationLinkProvider>) -> DisposableRef {
        self.register_provider(LocationLinkAdapter::<DeclarationKind>::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_declaration_support(handle, selector);
        })
    }

    pub fn register_implementation_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiLocationLinkProvider>) -> DisposableRef {
        self.register_provider(LocationLinkAdapter::<ImplementationKind>::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_implementation_support(handle, selector);
        })
    }

    pub fn register_type_definition_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiLocationLinkProvider>) -> DisposableRef {
        self.register_provider(LocationLinkAdapter::<TypeDefinitionKind>::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_type_definition_support(handle, selector);
        })
    }

    /// O corpo comum dos quatro `$provideDefinition`, `$provideDeclaration`, `$provideImplementation` e `$provideTypeDefinition`.
    fn provide_location_links<K: LocationLinkKind>(
        &self,
        handle: u32,
        invocation: &str,
        resource: &URI,
        position: EditorPosition,
        token: CancellationToken,
    ) -> Promise<Vec<ILocationLinkDto>>
    where
        LocationLinkAdapter<K>: AdapterKind,
    {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            invocation,
            move |adapter: Rc<LocationLinkAdapter<K>>, _| adapter.provide_location_links(resource, position, token),
            Vec::new(),
            Some(&token_to_race_against),
            false,
        )
    }

    // --- extra info

    pub fn register_hover_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiHoverProvider>) -> DisposableRef {
        self.register_provider(HoverAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_hover_provider(handle, selector);
        })
    }

    // --- occurrences

    pub fn register_document_highlight_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiDocumentHighlightProvider>) -> DisposableRef {
        self.register_provider(DocumentHighlightAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_document_highlight_provider(handle, selector);
        })
    }

    pub fn register_multi_document_highlight_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiMultiDocumentHighlightProvider>,
    ) -> DisposableRef {
        let adapter = MultiDocumentHighlightAdapter::new(self.documents.clone(), provider, self.log_service.clone());
        self.register_provider(adapter, extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_multi_document_highlight_provider(handle, selector);
        })
    }

    // --- references

    pub fn register_reference_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiReferenceProvider>) -> DisposableRef {
        self.register_provider(ReferenceAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_reference_support(handle, selector);
        })
    }

    /// `Disposable.from(result, provider.onDidChangeX(_ => this._proxy.$emitXEvent(eventHandle)))`: o `result` sozinho
    /// quando o provedor não tem o evento. `emit` é a chamada `$emit...Event` da thread principal.
    fn dispose_with_event(
        &self,
        result: DisposableRef,
        event: Option<Event<()>>,
        event_handle: Option<u32>,
        emit: impl Fn(&dyn MainThreadLanguageFeaturesShape, u32) + 'static,
    ) -> DisposableRef {
        match (event, event_handle) {
            (Some(event), Some(event_handle)) => {
                let proxy = self.proxy.clone();
                let subscription = event.listen(move |_| emit(&*proxy, event_handle));
                Rc::new(types::Disposable::from(vec![Some(result), Some(subscription)]))
            }
            _ => result,
        }
    }

    /// `_serializeRegExp`: o `RegExp` (o [`Dynamic`] dele) como `{ pattern, flags }`.
    fn serialize_reg_exp(reg_exp: &Dynamic) -> Option<IRegExpDto> {
        match reg_exp {
            Dynamic::RegExp(reg_exp) => Some(IRegExpDto { pattern: reg_exp.source().to_string_lossy(), flags: Some(reg_exp.flags().to_string_lossy()) }),
            _ => None,
        }
    }

    // --- debug hover

    pub fn register_evaluatable_expression_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiEvaluatableExpressionProvider>,
    ) -> DisposableRef {
        self.register_provider(EvaluatableExpressionAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_evaluatable_expression_provider(handle, selector);
        })
    }

    // --- debug inline values

    pub fn register_inline_values_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiInlineValuesProvider>) -> DisposableRef {
        let on_did_change_inline_values = provider.on_did_change_inline_values();
        let event_handle = on_did_change_inline_values.as_ref().map(|_| Self::next_handle());
        let handle = self.add_new_adapter(InlineValuesAdapter::new(self.documents.clone(), provider), extension);

        self.proxy.rpc_register_inline_values_provider(handle, self.transform_document_selector(selector, extension), event_handle);
        let result = self.create_disposable(handle);

        self.dispose_with_event(result, on_did_change_inline_values, event_handle, |proxy, event_handle| proxy.rpc_emit_inline_values_event(event_handle, None))
    }

    // --- linked editing

    pub fn register_linked_editing_range_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiLinkedEditingRangeProvider>,
    ) -> DisposableRef {
        self.register_provider(LinkedEditingRangeAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_linked_editing_range_provider(handle, selector);
        })
    }

    // --- code actions

    pub fn register_code_action_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiCodeActionProvider>,
        metadata: Option<&CodeActionProviderMetadata>,
    ) -> DisposableRef {
        let store = DisposableStore::new();
        let extension_ref = Rc::new(extension.clone());
        let converter: Rc<dyn ICommandsConverter> = self.commands.converter();
        let supports_resolve = provider.has_resolve_code_action();
        let adapter = CodeActionAdapter::new(
            self.documents.clone(),
            converter.clone(),
            self.diagnostics.clone(),
            provider,
            self.log_service.clone(),
            extension_ref.clone(),
            self.api_deprecation.clone(),
        );
        let handle = Self::next_handle();
        self.set_adapter(handle, adapter, extension_ref);
        let metadata_dto = ICodeActionProviderMetadataDto {
            provided_kinds: metadata
                .and_then(|metadata| metadata.provided_code_action_kinds.as_ref())
                .map(|kinds| kinds.iter().map(|kind| kind.value.clone()).collect()),
            documentation: metadata.and_then(|metadata| metadata.documentation.as_ref()).map(|documentation| {
                documentation
                    .iter()
                    .filter_map(|x| {
                        let command = converter.to_internal(Some(&x.command), &store).unwrap_or_else(|err| errors::throw(err))?;
                        Some(CodeActionDocumentationDto { kind: x.kind.value.clone(), command })
                    })
                    .collect()
            }),
        };
        self.proxy.rpc_register_code_action_support(
            handle,
            self.transform_document_selector(selector, extension),
            metadata_dto,
            &Self::ext_label(extension),
            Self::ext_id(extension),
            supports_resolve,
        );
        store.add(self.create_disposable(handle));
        store
    }

    // --- formatting

    pub fn register_document_formatting_edit_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiDocumentFormattingEditProvider>,
    ) -> DisposableRef {
        self.register_provider(DocumentFormattingAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_document_formatting_support(handle, selector, &extension.identifier, &Self::ext_label(extension));
        })
    }

    pub fn register_document_range_formatting_edit_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiDocumentRangeFormattingEditProvider>,
    ) -> DisposableRef {
        let can_format_multiple_ranges = provider.has_provide_document_ranges_formatting_edits();
        self.register_provider(RangeFormattingAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_range_formatting_support(handle, selector, &extension.identifier, &Self::ext_label(extension), can_format_multiple_ranges);
        })
    }

    pub fn register_on_type_formatting_edit_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiOnTypeFormattingEditProvider>,
        trigger_characters: Vec<String>,
    ) -> DisposableRef {
        self.register_provider(OnTypeFormattingAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_on_type_formatting_support(handle, selector, trigger_characters, &extension.identifier);
        })
    }

    // --- navigate types

    pub fn register_workspace_symbol_provider(&self, extension: &ExtensionDescription, provider: Rc<dyn ApiWorkspaceSymbolProvider>) -> DisposableRef {
        let supports_resolve = provider.has_resolve_workspace_symbol();
        let handle = self.add_new_adapter(NavigateTypeAdapter::new(provider, self.log_service.clone()), extension);
        self.proxy.rpc_register_navigate_type_support(handle, supports_resolve);
        self.create_disposable(handle)
    }

    // --- rename

    pub fn register_rename_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiRenameProvider>) -> DisposableRef {
        let supports_resolving = RenameAdapter::supports_resolving(&*provider);
        self.register_provider(RenameAdapter::new(self.documents.clone(), provider, self.log_service.clone()), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_rename_support(handle, selector, supports_resolving);
        })
    }

    pub fn register_new_symbol_names_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiNewSymbolNamesProvider>,
    ) -> DisposableRef {
        let adapter = NewSymbolNamesAdapter::new(self.documents.clone(), provider, self.log_service.clone());
        self.register_provider(adapter, extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_new_symbol_names_provider(handle, selector);
        })
    }

    // --- semantic coloring

    pub fn register_document_semantic_tokens_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiDocumentSemanticTokensProvider>,
        legend: SemanticTokensLegend,
    ) -> DisposableRef {
        let on_did_change_semantic_tokens = provider.on_did_change_semantic_tokens();
        let handle = self.add_new_adapter(DocumentSemanticTokensAdapter::new(self.documents.clone(), provider), extension);
        let event_handle = on_did_change_semantic_tokens.as_ref().map(|_| Self::next_handle());
        self.proxy.rpc_register_document_semantic_tokens_provider(handle, self.transform_document_selector(selector, extension), legend, event_handle);
        let result = self.create_disposable(handle);

        self.dispose_with_event(result, on_did_change_semantic_tokens, event_handle, |proxy, event_handle| proxy.rpc_emit_document_semantic_tokens_event(event_handle))
    }

    pub fn register_document_range_semantic_tokens_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiDocumentRangeSemanticTokensProvider>,
        legend: SemanticTokensLegend,
    ) -> DisposableRef {
        let on_did_change_semantic_tokens = provider.on_did_change_semantic_tokens();
        let handle = self.add_new_adapter(DocumentRangeSemanticTokensAdapter::new(self.documents.clone(), provider), extension);
        let event_handle = on_did_change_semantic_tokens.as_ref().map(|_| Self::next_handle());
        self.proxy.rpc_register_document_range_semantic_tokens_provider(handle, self.transform_document_selector(selector, extension), legend, event_handle);
        let result = self.create_disposable(handle);

        self.dispose_with_event(result, on_did_change_semantic_tokens, event_handle, |proxy, event_handle| proxy.rpc_emit_document_range_semantic_tokens_event(event_handle))
    }

    // --- suggestion

    pub fn register_completion_item_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiCompletionItemProvider>,
        trigger_characters: Vec<String>,
    ) -> DisposableRef {
        let supports_resolve = CompletionsAdapter::supports_resolving(&*provider);
        let converter: Rc<dyn ICommandsConverter> = self.commands.converter();
        let adapter = CompletionsAdapter::new(self.documents.clone(), converter, provider, self.api_deprecation.clone(), Rc::new(extension.clone()));
        self.register_provider(adapter, extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_completions_provider(handle, selector, trigger_characters, supports_resolve, &extension.identifier);
        })
    }

    // --- parameter hints

    pub fn register_signature_help_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiSignatureHelpProvider>,
        metadata_or_trigger_chars: SignatureHelpMetadataOrTriggerChars,
    ) -> DisposableRef {
        let metadata = match metadata_or_trigger_chars {
            SignatureHelpMetadataOrTriggerChars::TriggerCharacters(trigger_characters) => {
                ISignatureHelpProviderMetadataDto { trigger_characters, retrigger_characters: Vec::new() }
            }
            SignatureHelpMetadataOrTriggerChars::Metadata { trigger_characters, retrigger_characters } => {
                ISignatureHelpProviderMetadataDto { trigger_characters, retrigger_characters }
            }
        };

        self.register_provider(SignatureHelpAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_signature_help_provider(handle, selector, metadata);
        })
    }

    // --- inline hints

    pub fn register_inlay_hints_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiInlayHintsProvider>) -> DisposableRef {
        let on_did_change_inlay_hints = provider.on_did_change_inlay_hints();
        let event_handle = on_did_change_inlay_hints.as_ref().map(|_| Self::next_handle());
        let supports_resolve = provider.has_resolve_inlay_hint();
        let converter: Rc<dyn ICommandsConverter> = self.commands.converter();
        let adapter = InlayHintsAdapter::new(self.documents.clone(), converter, provider, self.log_service.clone(), Rc::new(extension.clone()));
        let handle = self.add_new_adapter(adapter, extension);

        self.proxy.rpc_register_inlay_hints_provider(handle, self.transform_document_selector(selector, extension), supports_resolve, event_handle, Some(&Self::ext_label(extension)));
        let result = self.create_disposable(handle);

        self.dispose_with_event(result, on_did_change_inlay_hints, event_handle, |proxy, event_handle| proxy.rpc_emit_inlay_hints_event(event_handle))
    }

    // --- links

    pub fn register_document_link_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiDocumentLinkProvider>) -> DisposableRef {
        let supports_resolve = provider.has_resolve_document_link();
        self.register_provider(LinkProviderAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_document_link_provider(handle, selector, supports_resolve);
        })
    }

    pub fn register_color_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiDocumentColorProvider>) -> DisposableRef {
        self.register_provider(ColorProviderAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_document_color_provider(handle, selector);
        })
    }

    pub fn register_folding_range_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiFoldingRangeProvider>) -> DisposableRef {
        let handle = Self::next_handle();
        let on_did_change_folding_ranges = provider.on_did_change_folding_ranges();
        let event_handle = on_did_change_folding_ranges.as_ref().map(|_| Self::next_handle());

        self.set_adapter(handle, FoldingProviderAdapter::new(self.documents.clone(), provider), Rc::new(extension.clone()));
        self.proxy.rpc_register_folding_range_provider(handle, self.transform_document_selector(selector, extension), &extension.identifier, event_handle);
        let result = self.create_disposable(handle);

        self.dispose_with_event(result, on_did_change_folding_ranges, event_handle, |proxy, event_handle| proxy.rpc_emit_folding_range_event(event_handle, None))
    }

    // --- smart select

    pub fn register_selection_range_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiSelectionRangeProvider>) -> DisposableRef {
        let adapter = SelectionRangeAdapter::new(self.documents.clone(), provider, self.log_service.clone());
        self.register_provider(adapter, extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_selection_range_provider(handle, selector);
        })
    }

    // --- call hierarchy

    pub fn register_call_hierarchy_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiCallHierarchyProvider>) -> DisposableRef {
        self.register_provider(CallHierarchyAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_call_hierarchy_provider(handle, selector);
        })
    }

    // --- type hierarchy

    pub fn register_type_hierarchy_provider(&self, extension: &ExtensionDescription, selector: &DocumentSelectorInput, provider: Rc<dyn ApiTypeHierarchyProvider>) -> DisposableRef {
        self.register_provider(TypeHierarchyAdapter::new(self.documents.clone(), provider), extension, selector, |proxy, handle, selector| {
            proxy.rpc_register_type_hierarchy_provider(handle, selector);
        })
    }

    // --- Document on drop

    pub fn register_document_on_drop_edit_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiDocumentDropEditProvider>,
        metadata: Option<&DocumentDropEditProviderMetadata>,
    ) -> DisposableRef {
        let handle = Self::next_handle();
        let supports_resolve = provider.has_resolve_document_drop_edit();
        let extension_ref = Rc::new(extension.clone());
        let adapter = DocumentDropEditAdapter::new(self.proxy.clone(), self.documents.clone(), provider, handle, extension_ref.clone());
        self.set_adapter(handle, adapter, extension_ref);

        self.proxy.rpc_register_document_on_drop_edit_provider(
            handle,
            self.transform_document_selector(selector, extension),
            metadata.map(|metadata| IDocumentDropEditProviderMetadata {
                supports_resolve,
                drop_mime_types: metadata.drop_mime_types.clone(),
                provided_drop_kinds: metadata.provided_drop_edit_kinds.as_ref().map(|kinds| kinds.iter().map(|kind| kind.value.clone()).collect()),
            }),
        );

        self.create_disposable(handle)
    }

    // --- copy/paste actions

    pub fn register_document_paste_edit_provider(
        &self,
        extension: &ExtensionDescription,
        selector: &DocumentSelectorInput,
        provider: Rc<dyn ApiDocumentPasteEditProvider>,
        metadata: &DocumentPasteProviderMetadata,
    ) -> DisposableRef {
        let handle = Self::next_handle();
        let metadata_dto = IPasteEditProviderMetadataDto {
            supports_copy: provider.has_prepare_document_paste(),
            supports_paste: provider.has_provide_document_paste_edits(),
            supports_resolve: provider.has_resolve_document_paste_edit(),
            provided_paste_edit_kinds: metadata.provided_paste_edit_kinds.as_ref().map(|kinds| kinds.iter().map(|kind| kind.value.clone()).collect()),
            copy_mime_types: metadata.copy_mime_types.clone(),
            paste_mime_types: metadata.paste_mime_types.clone(),
        };
        let extension_ref = Rc::new(extension.clone());
        let adapter = DocumentPasteEditProvider::new(self.proxy.clone(), self.documents.clone(), provider, handle, extension_ref.clone());
        self.set_adapter(handle, adapter, extension_ref);
        self.proxy.rpc_register_paste_edit_provider(handle, self.transform_document_selector(selector, extension), metadata_dto);
        self.create_disposable(handle)
    }
}

impl ExtHostLanguageFeaturesShape for ExtHostLanguageFeatures {
    fn rpc_provide_document_symbols(&self, handle: u32, resource: &URI, token: CancellationToken) -> Promise<Option<Vec<EditorDocumentSymbol>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideDocumentSymbols(URI.revive(resource), token)",
            move |adapter: Rc<DocumentSymbolAdapter>, _| adapter.provide_document_symbols(resource, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_code_lenses(&self, handle: u32, resource: &URI, token: CancellationToken) -> Promise<Option<ICodeLensListDto>> {
        let do_not_log = resource.scheme().to_string_lossy() == "output";
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideCodeLenses(URI.revive(resource), token)",
            move |adapter: Rc<CodeLensAdapter>, _| adapter.provide_code_lenses(resource, token),
            None,
            Some(&token_to_race_against),
            do_not_log,
        )
    }

    fn rpc_resolve_code_lens(&self, handle: u32, symbol: ICodeLensDto, token: CancellationToken) -> Promise<Option<ICodeLensDto>> {
        self.with_adapter(
            handle,
            "adapter => adapter.resolveCodeLens(symbol, token)",
            move |adapter: Rc<CodeLensAdapter>, _| adapter.resolve_code_lens(symbol, token),
            None,
            None,
            true,
        )
    }

    fn rpc_release_code_lenses(&self, handle: u32, cache_id: u32) {
        let _ = self.with_adapter(
            handle,
            "adapter => Promise.resolve(adapter.releaseCodeLenses(cacheId))",
            move |adapter: Rc<CodeLensAdapter>, _| {
                adapter.release_code_lenses(cache_id);
                Promise::resolved(())
            },
            (),
            None,
            true,
        );
    }

    fn rpc_provide_definition(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>> {
        self.provide_location_links::<DefinitionKind>(handle, "adapter => adapter.provideDefinition(URI.revive(resource), position, token)", resource, position, token)
    }

    fn rpc_provide_declaration(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>> {
        self.provide_location_links::<DeclarationKind>(handle, "adapter => adapter.provideDeclaration(URI.revive(resource), position, token)", resource, position, token)
    }

    fn rpc_provide_implementation(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>> {
        self.provide_location_links::<ImplementationKind>(handle, "adapter => adapter.provideImplementation(URI.revive(resource), position, token)", resource, position, token)
    }

    fn rpc_provide_type_definition(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Vec<ILocationLinkDto>> {
        self.provide_location_links::<TypeDefinitionKind>(handle, "adapter => adapter.provideTypeDefinition(URI.revive(resource), position, token)", resource, position, token)
    }

    fn rpc_provide_hover(&self, handle: u32, resource: &URI, position: EditorPosition, context: Option<HoverContextDto>, token: CancellationToken) -> Promise<Option<HoverWithId>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideHover(URI.revive(resource), position, context, token)",
            move |adapter: Rc<HoverAdapter>, _| adapter.provide_hover(resource, position, context, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_release_hover(&self, handle: u32, id: u32) {
        let _ = self.with_adapter(
            handle,
            "adapter => Promise.resolve(adapter.releaseHover(id))",
            move |adapter: Rc<HoverAdapter>, _| {
                adapter.release_hover(id);
                Promise::resolved(())
            },
            (),
            None,
            false,
        );
    }

    fn rpc_provide_document_highlights(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<EditorDocumentHighlight>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideDocumentHighlights(URI.revive(resource), position, token)",
            move |adapter: Rc<DocumentHighlightAdapter>, _| adapter.provide_document_highlights(resource, position, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_multi_document_highlights(
        &self,
        handle: u32,
        resource: &URI,
        position: EditorPosition,
        other_models: Vec<URI>,
        token: CancellationToken,
    ) -> Promise<Option<Vec<MultiDocumentHighlightDto>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideMultiDocumentHighlights(URI.revive(resource), position, otherModels.map(model => URI.revive(model)), token)",
            move |adapter: Rc<MultiDocumentHighlightAdapter>, _| adapter.provide_multi_document_highlights(resource, position, other_models, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_references(&self, handle: u32, resource: &URI, position: EditorPosition, context: ReferenceContext, token: CancellationToken) -> Promise<Option<Vec<ILocationDto>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideReferences(URI.revive(resource), position, context, token)",
            move |adapter: Rc<ReferenceAdapter>, _| adapter.provide_references(resource, position, context, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_evaluatable_expression(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<EvaluatableExpressionDto>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideEvaluatableExpression(URI.revive(resource), position, token)",
            move |adapter: Rc<EvaluatableExpressionAdapter>, _| adapter.provide_evaluatable_expression(resource, position, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_inline_values(
        &self,
        handle: u32,
        resource: &URI,
        range: EditorRange,
        context: IInlineValueContextDto,
        token: CancellationToken,
    ) -> Promise<Option<Vec<InlineValueDto>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideInlineValues(URI.revive(resource), range, context, token)",
            move |adapter: Rc<InlineValuesAdapter>, _| adapter.provide_inline_values(resource, range, context, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_linked_editing_ranges(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<ILinkedEditingRangesDto>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "async adapter => { const res = await adapter.provideLinkedEditingRanges(URI.revive(resource), position, token); if (res) { return { ranges: res.ranges, wordPattern: res.wordPattern ? ExtHostLanguageFeatures._serializeRegExp(res.wordPattern) : undefined }; } return undefined; }",
            move |adapter: Rc<LinkedEditingRangeAdapter>, _| {
                async_fn(async move {
                    let res = adapter.provide_linked_editing_ranges(resource, position, token).await?;
                    Ok(res.map(|res| ILinkedEditingRangesDto {
                        ranges: res.ranges,
                        word_pattern: res.word_pattern.as_ref().and_then(ExtHostLanguageFeatures::serialize_reg_exp),
                    }))
                })
            },
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_code_actions(
        &self,
        handle: u32,
        resource: &URI,
        range_or_selection: RangeOrSelection,
        context: CodeActionContext,
        token: CancellationToken,
    ) -> Promise<Option<ICodeActionListDto>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideCodeActions(URI.revive(resource), rangeOrSelection, context, token)",
            move |adapter: Rc<CodeActionAdapter>, _| adapter.provide_code_actions(resource, range_or_selection, context, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_resolve_code_action(&self, handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedCodeActionDto> {
        self.with_adapter(
            handle,
            "adapter => adapter.resolveCodeAction(id, token)",
            move |adapter: Rc<CodeActionAdapter>, _| adapter.resolve_code_action(id, token),
            ResolvedCodeActionDto::default(),
            None,
            false,
        )
    }

    fn rpc_release_code_actions(&self, handle: u32, cache_id: u32) {
        let _ = self.with_adapter(
            handle,
            "adapter => Promise.resolve(adapter.releaseCodeActions(cacheId))",
            move |adapter: Rc<CodeActionAdapter>, _| {
                adapter.release_code_actions(cache_id);
                Promise::resolved(())
            },
            (),
            None,
            false,
        );
    }

    fn rpc_prepare_document_paste(
        &self,
        handle: u32,
        uri: &URI,
        ranges: Vec<EditorRange>,
        data_transfer: DataTransferDto,
        token: CancellationToken,
    ) -> Promise<Option<DataTransferDto>> {
        let uri = uri.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.prepareDocumentPaste(URI.revive(resource), ranges, dataTransfer, token)",
            move |adapter: Rc<DocumentPasteEditProvider>, _| adapter.prepare_document_paste(uri, ranges, data_transfer, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_paste_edits(
        &self,
        handle: u32,
        request_id: u32,
        uri: &URI,
        ranges: Vec<EditorRange>,
        data_transfer: DataTransferDto,
        context: IDocumentPasteContextDto,
        token: CancellationToken,
    ) -> Promise<Option<Vec<IPasteEditDto>>> {
        let uri = uri.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.providePasteEdits(requestId, URI.revive(resource), ranges, dataTransferDto, context, token)",
            move |adapter: Rc<DocumentPasteEditProvider>, _| adapter.provide_paste_edits(request_id, uri, ranges, data_transfer, context, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_resolve_paste_edit(&self, handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedPasteEditDto> {
        self.with_adapter(
            handle,
            "adapter => adapter.resolvePasteEdit(id, token)",
            move |adapter: Rc<DocumentPasteEditProvider>, _| adapter.resolve_paste_edit(id, token),
            ResolvedPasteEditDto::default(),
            None,
            false,
        )
    }

    fn rpc_release_paste_edits(&self, handle: u32, cache_id: u32) {
        let _ = self.with_adapter(
            handle,
            "adapter => Promise.resolve(adapter.releasePasteEdits(cacheId))",
            move |adapter: Rc<DocumentPasteEditProvider>, _| {
                adapter.release_paste_edits(cache_id);
                Promise::resolved(())
            },
            (),
            None,
            false,
        );
    }

    fn rpc_provide_document_formatting_edits(&self, handle: u32, resource: &URI, options: FormattingOptions, token: CancellationToken) -> Promise<Option<Vec<EditorTextEdit>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideDocumentFormattingEdits(URI.revive(resource), options, token)",
            move |adapter: Rc<DocumentFormattingAdapter>, _| adapter.provide_document_formatting_edits(resource, options, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_document_range_formatting_edits(
        &self,
        handle: u32,
        resource: &URI,
        range: EditorRange,
        options: FormattingOptions,
        token: CancellationToken,
    ) -> Promise<Option<Vec<EditorTextEdit>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideDocumentRangeFormattingEdits(URI.revive(resource), range, options, token)",
            move |adapter: Rc<RangeFormattingAdapter>, _| adapter.provide_document_range_formatting_edits(resource, range, options, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_document_ranges_formatting_edits(
        &self,
        handle: u32,
        resource: &URI,
        ranges: Vec<EditorRange>,
        options: FormattingOptions,
        token: CancellationToken,
    ) -> Promise<Option<Vec<EditorTextEdit>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideDocumentRangesFormattingEdits(URI.revive(resource), ranges, options, token)",
            move |adapter: Rc<RangeFormattingAdapter>, _| adapter.provide_document_ranges_formatting_edits(resource, ranges, options, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_on_type_formatting_edits(
        &self,
        handle: u32,
        resource: &URI,
        position: EditorPosition,
        ch: &str,
        options: FormattingOptions,
        token: CancellationToken,
    ) -> Promise<Option<Vec<EditorTextEdit>>> {
        let resource = resource.clone();
        let ch = ch.to_owned();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideOnTypeFormattingEdits(URI.revive(resource), position, ch, options, token)",
            move |adapter: Rc<OnTypeFormattingAdapter>, _| adapter.provide_on_type_formatting_edits(resource, position, ch, options, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_workspace_symbols(&self, handle: u32, search: &str, token: CancellationToken) -> Promise<IWorkspaceSymbolsDto> {
        let search = search.to_owned();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideWorkspaceSymbols(search, token)",
            move |adapter: Rc<NavigateTypeAdapter>, _| adapter.provide_workspace_symbols(search, token),
            IWorkspaceSymbolsDto::default(),
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_resolve_workspace_symbol(&self, handle: u32, symbol: IWorkspaceSymbolDto, token: CancellationToken) -> Promise<Option<IWorkspaceSymbolDto>> {
        self.with_adapter(
            handle,
            "adapter => adapter.resolveWorkspaceSymbol(symbol, token)",
            move |adapter: Rc<NavigateTypeAdapter>, _| adapter.resolve_workspace_symbol(symbol, token),
            None,
            None,
            false,
        )
    }

    fn rpc_release_workspace_symbols(&self, handle: u32, id: u32) {
        let _ = self.with_adapter(
            handle,
            "adapter => adapter.releaseWorkspaceSymbols(id)",
            move |adapter: Rc<NavigateTypeAdapter>, _| {
                adapter.release_workspace_symbols(id);
                Promise::resolved(())
            },
            (),
            None,
            false,
        );
    }

    fn rpc_provide_rename_edits(&self, handle: u32, resource: &URI, position: EditorPosition, new_name: &str, token: CancellationToken) -> Promise<Option<RenameEditsDto>> {
        let resource = resource.clone();
        let new_name = new_name.to_owned();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideRenameEdits(URI.revive(resource), position, newName, token)",
            move |adapter: Rc<RenameAdapter>, _| adapter.provide_rename_edits(resource, position, new_name, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_resolve_rename_location(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<RenameLocationDto>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.resolveRenameLocation(URI.revive(resource), position, token)",
            move |adapter: Rc<RenameAdapter>, _| adapter.resolve_rename_location(resource, position, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_supports_automatic_new_symbol_names_trigger_kind(&self, handle: u32) -> Promise<Option<bool>> {
        self.with_adapter(
            handle,
            "adapter => adapter.supportsAutomaticNewSymbolNamesTriggerKind()",
            move |adapter: Rc<NewSymbolNamesAdapter>, _| adapter.supports_automatic_new_symbol_names_trigger_kind(),
            Some(false),
            None,
            false,
        )
    }

    fn rpc_provide_new_symbol_names(
        &self,
        handle: u32,
        resource: &URI,
        range: EditorRange,
        trigger_kind: LanguagesNewSymbolNameTriggerKind,
        token: CancellationToken,
    ) -> Promise<Option<Vec<NewSymbolNameDto>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideNewSymbolNames(URI.revive(resource), range, triggerKind, token)",
            move |adapter: Rc<NewSymbolNamesAdapter>, _| adapter.provide_new_symbol_names(resource, range, trigger_kind, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_document_semantic_tokens(&self, handle: u32, resource: &URI, previous_result_id: u32, token: CancellationToken) -> Promise<Option<VSBuffer>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideDocumentSemanticTokens(URI.revive(resource), previousResultId, token)",
            move |adapter: Rc<DocumentSemanticTokensAdapter>, _| adapter.provide_document_semantic_tokens(resource, previous_result_id, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_release_document_semantic_tokens(&self, handle: u32, semantic_coloring_result_id: u32) {
        let _ = self.with_adapter(
            handle,
            "adapter => adapter.releaseDocumentSemanticColoring(semanticColoringResultId)",
            move |adapter: Rc<DocumentSemanticTokensAdapter>, _| {
                adapter.release_document_semantic_coloring(semantic_coloring_result_id);
                Promise::resolved(())
            },
            (),
            None,
            false,
        );
    }

    fn rpc_provide_document_range_semantic_tokens(&self, handle: u32, resource: &URI, range: EditorRange, token: CancellationToken) -> Promise<Option<VSBuffer>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideDocumentRangeSemanticTokens(URI.revive(resource), range, token)",
            move |adapter: Rc<DocumentRangeSemanticTokensAdapter>, _| adapter.provide_document_range_semantic_tokens(resource, range, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_completion_items(
        &self,
        handle: u32,
        resource: &URI,
        position: EditorPosition,
        context: EditorCompletionContext,
        token: CancellationToken,
    ) -> Promise<Option<ISuggestResultDto>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideCompletionItems(URI.revive(resource), position, context, token)",
            move |adapter: Rc<CompletionsAdapter>, _| adapter.provide_completion_items(resource, position, context, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_resolve_completion_item(&self, handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<ISuggestDataDto>> {
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.resolveCompletionItem(id, token)",
            move |adapter: Rc<CompletionsAdapter>, _| adapter.resolve_completion_item(id, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_release_completion_items(&self, handle: u32, id: u32) {
        let _ = self.with_adapter(
            handle,
            "adapter => adapter.releaseCompletionItems(id)",
            move |adapter: Rc<CompletionsAdapter>, _| {
                adapter.release_completion_items(id);
                Promise::resolved(())
            },
            (),
            None,
            false,
        );
    }

    fn rpc_provide_signature_help(
        &self,
        handle: u32,
        resource: &URI,
        position: EditorPosition,
        context: ISignatureHelpContextDto,
        token: CancellationToken,
    ) -> Promise<Option<ISignatureHelpDto>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideSignatureHelp(URI.revive(resource), position, context, token)",
            move |adapter: Rc<SignatureHelpAdapter>, _| adapter.provide_signature_help(resource, position, context, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_release_signature_help(&self, handle: u32, id: u32) {
        let _ = self.with_adapter(
            handle,
            "adapter => adapter.releaseSignatureHelp(id)",
            move |adapter: Rc<SignatureHelpAdapter>, _| {
                adapter.release_signature_help(id);
                Promise::resolved(())
            },
            (),
            None,
            false,
        );
    }

    fn rpc_provide_inlay_hints(&self, handle: u32, resource: &URI, range: EditorRange, token: CancellationToken) -> Promise<Option<IInlayHintsDto>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideInlayHints(URI.revive(resource), range, token)",
            move |adapter: Rc<InlayHintsAdapter>, _| adapter.provide_inlay_hints(resource, range, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_resolve_inlay_hint(&self, handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<IInlayHintDto>> {
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.resolveInlayHint(id, token)",
            move |adapter: Rc<InlayHintsAdapter>, _| adapter.resolve_inlay_hint(id, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_release_inlay_hints(&self, handle: u32, id: u32) {
        let _ = self.with_adapter(
            handle,
            "adapter => adapter.releaseHints(id)",
            move |adapter: Rc<InlayHintsAdapter>, _| {
                adapter.release_hints(id);
                Promise::resolved(())
            },
            (),
            None,
            false,
        );
    }

    fn rpc_provide_document_links(&self, handle: u32, resource: &URI, token: CancellationToken) -> Promise<Option<ILinksListDto>> {
        let do_not_log = resource.scheme().to_string_lossy() == "output";
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideLinks(URI.revive(resource), token)",
            move |adapter: Rc<LinkProviderAdapter>, _| adapter.provide_links(resource, token),
            None,
            Some(&token_to_race_against),
            do_not_log,
        )
    }

    fn rpc_resolve_document_link(&self, handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<Option<ILinkDto>> {
        self.with_adapter(
            handle,
            "adapter => adapter.resolveLink(id, token)",
            move |adapter: Rc<LinkProviderAdapter>, _| adapter.resolve_link(id, token),
            None,
            None,
            true,
        )
    }

    fn rpc_release_document_links(&self, handle: u32, id: u32) {
        let _ = self.with_adapter(
            handle,
            "adapter => adapter.releaseLinks(id)",
            move |adapter: Rc<LinkProviderAdapter>, _| {
                adapter.release_links(id);
                Promise::resolved(())
            },
            (),
            None,
            true,
        );
    }

    fn rpc_provide_document_colors(&self, handle: u32, resource: &URI, token: CancellationToken) -> Promise<Vec<IRawColorInfo>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideColors(URI.revive(resource), token)",
            move |adapter: Rc<ColorProviderAdapter>, _| adapter.provide_colors(resource, token),
            Vec::new(),
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_color_presentations(&self, handle: u32, resource: &URI, color_info: IRawColorInfo, token: CancellationToken) -> Promise<Option<Vec<IColorPresentationDto>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideColorPresentations(URI.revive(resource), colorInfo, token)",
            move |adapter: Rc<ColorProviderAdapter>, _| adapter.provide_color_presentations(resource, color_info, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_folding_ranges(&self, handle: u32, resource: &URI, context: FoldingContext, token: CancellationToken) -> Promise<Option<Vec<EditorFoldingRange>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "(adapter) => adapter.provideFoldingRanges(URI.revive(resource), context, token)",
            move |adapter: Rc<FoldingProviderAdapter>, _| adapter.provide_folding_ranges(resource, context, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_selection_ranges(&self, handle: u32, resource: &URI, positions: Vec<EditorPosition>, token: CancellationToken) -> Promise<Vec<Vec<EditorSelectionRange>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideSelectionRanges(URI.revive(resource), positions, token)",
            move |adapter: Rc<SelectionRangeAdapter>, _| adapter.provide_selection_ranges(resource, positions, token),
            Vec::new(),
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_prepare_call_hierarchy(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<ICallHierarchyItemDto>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => Promise.resolve(adapter.prepareSession(URI.revive(resource), position, token))",
            move |adapter: Rc<CallHierarchyAdapter>, _| adapter.prepare_session(resource, position, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_call_hierarchy_incoming_calls(&self, handle: u32, session_id: &str, item_id: &str, token: CancellationToken) -> Promise<Option<Vec<IIncomingCallDto>>> {
        let (session_id, item_id) = (session_id.to_owned(), item_id.to_owned());
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideCallsTo(sessionId, itemId, token)",
            move |adapter: Rc<CallHierarchyAdapter>, _| adapter.provide_calls_to(session_id, item_id, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_call_hierarchy_outgoing_calls(&self, handle: u32, session_id: &str, item_id: &str, token: CancellationToken) -> Promise<Option<Vec<IOutgoingCallDto>>> {
        let (session_id, item_id) = (session_id.to_owned(), item_id.to_owned());
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideCallsFrom(sessionId, itemId, token)",
            move |adapter: Rc<CallHierarchyAdapter>, _| adapter.provide_calls_from(session_id, item_id, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_release_call_hierarchy(&self, handle: u32, session_id: &str) {
        let session_id = session_id.to_owned();
        let _ = self.with_adapter(
            handle,
            "adapter => Promise.resolve(adapter.releaseSession(sessionId))",
            move |adapter: Rc<CallHierarchyAdapter>, _| {
                adapter.release_session(&session_id);
                Promise::resolved(())
            },
            (),
            None,
            false,
        );
    }

    fn rpc_prepare_type_hierarchy(&self, handle: u32, resource: &URI, position: EditorPosition, token: CancellationToken) -> Promise<Option<Vec<ITypeHierarchyItemDto>>> {
        let resource = resource.clone();
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => Promise.resolve(adapter.prepareSession(URI.revive(resource), position, token))",
            move |adapter: Rc<TypeHierarchyAdapter>, _| adapter.prepare_session(resource, position, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_type_hierarchy_supertypes(&self, handle: u32, session_id: &str, item_id: &str, token: CancellationToken) -> Promise<Option<Vec<ITypeHierarchyItemDto>>> {
        let (session_id, item_id) = (session_id.to_owned(), item_id.to_owned());
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideSupertypes(sessionId, itemId, token)",
            move |adapter: Rc<TypeHierarchyAdapter>, _| adapter.provide_supertypes(session_id, item_id, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_provide_type_hierarchy_subtypes(&self, handle: u32, session_id: &str, item_id: &str, token: CancellationToken) -> Promise<Option<Vec<ITypeHierarchyItemDto>>> {
        let (session_id, item_id) = (session_id.to_owned(), item_id.to_owned());
        let token_to_race_against = token.clone();
        self.with_adapter(
            handle,
            "adapter => adapter.provideSubtypes(sessionId, itemId, token)",
            move |adapter: Rc<TypeHierarchyAdapter>, _| adapter.provide_subtypes(session_id, item_id, token),
            None,
            Some(&token_to_race_against),
            false,
        )
    }

    fn rpc_release_type_hierarchy(&self, handle: u32, session_id: &str) {
        let session_id = session_id.to_owned();
        let _ = self.with_adapter(
            handle,
            "adapter => Promise.resolve(adapter.releaseSession(sessionId))",
            move |adapter: Rc<TypeHierarchyAdapter>, _| {
                adapter.release_session(&session_id);
                Promise::resolved(())
            },
            (),
            None,
            false,
        );
    }

    fn rpc_provide_document_on_drop_edits(
        &self,
        handle: u32,
        request_id: u32,
        resource: &URI,
        position: EditorPosition,
        data_transfer: DataTransferDto,
        token: CancellationToken,
    ) -> Promise<Option<Vec<IDocumentDropEditDto>>> {
        let resource = resource.clone();
        self.with_adapter(
            handle,
            "adapter => Promise.resolve(adapter.provideDocumentOnDropEdits(requestId, URI.revive(resource), position, dataTransferDto, token))",
            move |adapter: Rc<DocumentDropEditAdapter>, _| adapter.provide_document_on_drop_edits(request_id, resource, position, data_transfer, token),
            None,
            None,
            false,
        )
    }

    fn rpc_resolve_drop_edit(&self, handle: u32, id: ChainedCacheId, token: CancellationToken) -> Promise<ResolvedDropEditDto> {
        self.with_adapter(
            handle,
            "adapter => adapter.resolveDropEdit(id, token)",
            move |adapter: Rc<DocumentDropEditAdapter>, _| adapter.resolve_drop_edit(id, token),
            ResolvedDropEditDto::default(),
            None,
            false,
        )
    }

    fn rpc_release_document_on_drop_edits(&self, handle: u32, cache_id: u32) {
        let _ = self.with_adapter(
            handle,
            "adapter => Promise.resolve(adapter.releaseDropEdits(cacheId))",
            move |adapter: Rc<DocumentDropEditAdapter>, _| {
                adapter.release_drop_edits(cache_id);
                Promise::resolved(())
            },
            (),
            None,
            false,
        );
    }
}
