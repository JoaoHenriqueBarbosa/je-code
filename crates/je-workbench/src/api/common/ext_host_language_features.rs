//! Porte de `src/vs/workbench/api/common/extHostLanguageFeatures.ts` (3 060 linhas): o lado da extensão dos
//! provedores de recursos de linguagem. Cada provedor da API ganha um adaptador (a conversão entre o provedor da
//! extensão e os DTOs do protocolo) e a classe [`ExtHostLanguageFeatures`] registra os adaptadores, guarda o mapa de
//! handles e despacha os `$provide*` da thread principal.
//!
//! PARTE 1 (esta). Portado: a infraestrutura (`AdapterData`, o mapa de handles, `_createDisposable`, `_withAdapter`,
//! `_nextHandle`, `_transformDocumentSelector`, `_addNewAdapter`, `_extLabel`, `_extId`) e os adaptadores, os métodos
//! `register*Provider` e os `$provide*`/`$release*`/`$resolve*` de:
//! `DocumentSymbol`, `CodeLens`, `Definition`, `Declaration`, `Implementation`, `TypeDefinition`, `Hover`,
//! `DocumentHighlight`, `MultiDocumentHighlight` e `Reference`.
//!
//! PENDENTES para a parte 2 (ainda não existem aqui): `EvaluatableExpressionAdapter`, `InlineValuesAdapter`,
//! `LinkedEditingRangeAdapter`, `CodeActionAdapter` (e `CustomCodeAction`), `DocumentPasteEditProvider`,
//! `DocumentFormattingAdapter`, `RangeFormattingAdapter`, `OnTypeFormattingAdapter`, `NavigateTypeAdapter`,
//! `RenameAdapter`, `NewSymbolNamesAdapter`, `SemanticTokensPreviousResult`, `DocumentSemanticTokensAdapter`,
//! `DocumentRangeSemanticTokensAdapter`, `CompletionsAdapter`, `InlineCompletionAdapter`, `ReferenceMap`,
//! `SignatureHelpAdapter`, `InlayHintsAdapter`, `LinkProviderAdapter`, `ColorProviderAdapter`,
//! `FoldingProviderAdapter`, `SelectionRangeAdapter`, `CallHierarchyAdapter`, `TypeHierarchyAdapter`,
//! `DocumentDropEditAdapter`, os `register*`/`$provide*` correspondentes, `_serializeRegExp`, o estado de unificação
//! das completações em linha (`inlineCompletionsUnificationState` e o seu evento) e, junto dele, a base `Disposable`
//! de `ExtHostLanguageFeatures` (o único `_register` do original é o emissor desse evento). O construtor também
//! ganha `ExtHostDiagnostics` e `IExtHostApiDeprecationService` (só o `CodeActionAdapter` os usa).
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

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::Instant;

use je_base::async_::{async_fn, await_maybe, race_cancellation_error};
use je_base::cancellation::CancellationToken;
use je_base::errors::{Error, is_cancellation_error};
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::uri_ipc::IURITransformer;
use je_editor::language_providers::{DocumentHighlight as EditorDocumentHighlight, DocumentSymbol as EditorDocumentSymbol};
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_extensions::scanner::ExtensionDescription;
use je_platform::log::{LogMessage, LogServiceRef, Logger};

use super::cache::Cache;
use super::ext_host_commands::ExtHostCommands;
use super::ext_host_documents::ExtHostDocuments;
use super::ext_host_language_features_api::{
    ApiCodeLensProvider, ApiDocumentHighlightProvider, ApiDocumentSymbolProvider, ApiDocumentSymbols, ApiHoverContext, ApiHoverProvider,
    ApiLocationLinkProvider, ApiLocationResult, ApiMultiDocumentHighlightProvider, ApiReferenceProvider, CodeLensRef, DocumentSymbolProviderMetadata,
};
use super::ext_host_language_features_converters::{DefinitionLink, DocumentSelector, Hover, MultiDocumentHighlight};
use super::ext_host_protocol::{
    ExtHostLanguageFeaturesShape, HoverContextDto, HoverWithId, IDocumentFilterDto, IExtHostTelemetry, ICodeLensDto, ICodeLensListDto, ILocationDto,
    ILocationLinkDto, IMainContext, MainContext, MainThreadLanguageFeaturesShape, MultiDocumentHighlightDto, ReferenceContext,
};
use super::ext_host_type_converters::{self as type_convert, DocumentSelectorInput, ICommandsConverter};
use super::ext_host_types::{self as types, SymbolInformation, VerboseHover};

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
    log_service: LogServiceRef,
    extension_telemetry: Rc<dyn IExtHostTelemetry>,
}

impl ExtHostLanguageFeatures {
    pub fn new(
        main_context: &dyn IMainContext,
        uri_transformer: Option<Rc<dyn IURITransformer>>,
        documents: Rc<ExtHostDocuments>,
        commands: Rc<ExtHostCommands>,
        log_service: LogServiceRef,
        extension_telemetry: Rc<dyn IExtHostTelemetry>,
    ) -> Rc<Self> {
        Rc::new(Self {
            proxy: main_context.get_proxy(&MainContext::MAIN_THREAD_LANGUAGE_FEATURES),
            adapters: Rc::new(RefCell::new(HashMap::new())),
            uri_transformer,
            documents,
            commands,
            log_service,
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
}
