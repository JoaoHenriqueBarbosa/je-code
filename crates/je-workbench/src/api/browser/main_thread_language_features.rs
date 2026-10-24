//! Porte de `src/vs/workbench/api/browser/mainThreadLanguageFeatures.ts`: o lado do workbench do RPC de recursos de
//! linguagem. Cada `$register*` do host de extensões cria um provedor (os objetos literais do original, em
//! `providers`, e as classes de colar e soltar, em `paste_drop`) e o registra no `ILanguageFeaturesService` do editor
//! ([`LanguageFeaturesService`]).
//!
//! PARTE 3. Portado: o construtor com `updateAllWordDefinitions` (`$setWordDefinitions` do host para cada linguagem registrada
//! e a cada mudança do `ILanguageConfigurationService`), a assinatura do `IInlineCompletionsUnificationService`
//! (`$acceptInlineCompletionsUnificationState`), `$setLanguageConfiguration` e os `_revive*` de configuração (`_reviveRegExp`,
//! `_reviveIndentationRule`, `_reviveOnEnterRule(s)` e `_reviveAutoClosingPairs`).
//!
//! PENDENTES: `$registerInlineCompletionsSupport`, `$emitInlineCompletionsChange`, `$emitInlineCompletionModelInfoChange`,
//! `$emitInlineCompletionProviderOptionsChange` e a classe `ExtensionBackedInlineCompletionsProvider` (o editor não tem as
//! completações em linha, o `registerInlineCompletionsProvider` do `ILanguageFeaturesService` nem a telemetria de edição de IA).
//! Nenhum deles está no `MainThreadLanguageFeaturesShape` do protocolo.
//!
//! Divergências:
//! - `ILanguageService`, `ILanguageConfigurationService` e `IInlineCompletionsUnificationService` são `Option` no construtor: o
//!   `TestInstantiationService` do original não é estrito e resolve o serviço ausente como `undefined` (o `if (this._languageService)`
//!   do original testa isso). O `$setLanguageConfiguration` sem eles lança `TypeError`, como o original. A implementação do
//!   `IInlineCompletionsUnificationService` não tem porte (só o contrato).
//! - `_reviveRegExp` com um padrão inválido lança (`SyntaxError`), como o `new RegExp(...)`; o `flags` ausente é vazio.
//! - `@extHostNamedCustomer(...)` e o `IExtHostContext` do construtor: o construtor recebe o [`IMainContext`] (por `Rc`) e
//!   pede o proxy de `ExtHostLanguageFeatures` a cada uso (ver [`MainThreadLanguageFeatures::proxy`]). `ILanguageFeaturesService` é o [`LanguageFeaturesService`] do editor (os registros
//!   são compartilhados por `Rc`) e `IUriIdentityService` é um `Rc<dyn IUriIdentityService>`. `IInstantiationService` não entra.
//! - O `selector: IDocumentFilterDto[]` do registro é o [`LanguageSelector::Array`] de [`LanguageFilter`]; o `pattern` de
//!   `IRelativePattern` é o [`RelativePattern`] de `base` e `pattern`, e o `hasAccessToAllModels` nunca vem do DTO (`false`).
//! - `_registrations` (`DisposableMap<number>`) guarda registros e emissores. O `obj instanceof Emitter` dos
//!   `$emit*Event` é uma consulta ao mapa tipado `emitters`, que guarda os mesmos emissores (`Emitter<()>`). O evento carrega
//!   `()`: o argumento `event?` dos `$emitCodeLensEvent`, `$emitInlineValuesEvent` e `$emitFoldingRangeEvent` (que o original
//!   dispara como payload do `Emitter<languages.CodeLensProvider>`, sem consumidor que o leia) é ignorado.
//! - `$resolvePasteFileData` e `$resolveDocumentOnDropFileData` lançam `Error('Could not find provider')` de forma
//!   síncrona no original; aqui o erro é uma promessa rejeitada (o trait devolve `Promise`).
//! - `$registerNavigateTypeSupport`: o `lastResultId` é compartilhado por `Rc<Cell<...>>` entre as chamadas do provedor.
//! - `$registerCodeLensSupport`: o `Emitter<languages.CodeLensProvider>` do original é um `Emitter<()>`.
//! - `$registerNewSymbolNamesProvider`: o `supportsAutomaticNewSymbolNamesTriggerKind` é a promessa de
//!   `$supportsAutomaticNewSymbolNamesTriggerKind(handle)` pedida no registro, como no original.

mod paste_drop;
mod providers;
pub mod registries;

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::cancellation::CancellationToken;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::glob::RelativePattern;
use je_base::js_regexp::JsRegExp;
use je_base::lifecycle::{Disposable, DisposableMap, DisposableRef, IDisposable, combined_disposable, to_disposable};
use je_base::promise::Promise;
use je_editor::code_action::languages::CodeActionProvider;
use je_editor::language_configuration::{AutoClosingPairConditional, DocComment, ElectricCharacterSupportConfig, IndentationRule, LanguageConfiguration, OnEnterRule};
use je_editor::language_configuration_registry::ILanguageConfigurationService;
use je_editor::language_features_service::{LanguageFeaturesService, SemanticTokensLegend as EditorLegend};
use je_editor::language_providers::{DocumentHighlightProvider, DocumentSymbolProvider, MultiDocumentHighlightProvider, ProviderResult, SelectionRangeProvider};
use je_editor::language_selector::{FilterPattern, LanguageFilter, LanguageSelector};
use je_editor::language_service::ILanguageService;
use je_editor::languages::CompletionItemProvider;
use je_editor::parameter_hints::languages::SignatureHelpProvider;
use je_editor::position::Position;
use je_editor::text_model::TextModel;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::uri_identity::uri_identity::IUriIdentityService;

use self::paste_drop::{MainThreadDocumentOnDropEditProvider, MainThreadPasteEditProvider};
use self::providers::*;
use self::registries::{
    CallHierarchyProvider, CallHierarchyItem, CallHierarchySession, IWorkspaceSymbol, IWorkspaceSymbolProvider, TypeHierarchyItem, TypeHierarchyProvider,
    TypeHierarchySession, WorkspaceSymbolProviderRegistry, call_hierarchy_provider_registry, type_hierarchy_provider_registry,
};
use crate::api::common::ext_host_protocol::{
    ExtHostContext, ExtHostLanguageFeaturesShape, IAutoClosingPairDto, IDocumentDropEditProviderMetadata, IDocumentFilterDto, IIncomingCallDto, IIndentationRuleDto,
    ILanguageConfigurationDto, ILanguageWordDefinitionDto, IMainContext, IOnEnterRuleDto, IOutgoingCallDto, IPasteEditProviderMetadataDto, IRegExpDto,
    ISignatureHelpProviderMetadataDto, IWorkspaceSymbolDto, ICodeActionProviderMetadataDto, MainThreadLanguageFeaturesShape,
};
use crate::api::common::ext_host_types::SemanticTokensLegend;
use crate::api::common::ext_host_workspace::GlobPatternDto;
use crate::services::inline_completions::inline_completions_unification::IInlineCompletionsUnificationService;

/// `MainThreadLanguageFeatures`.
pub struct MainThreadLanguageFeatures {
    /// A base `Disposable` (`extends Disposable`): guarda o mapa `_registrations` e as assinaturas do construtor.
    base: Rc<Disposable>,
    ext_host_context: Rc<dyn IMainContext>,
    registrations: Rc<DisposableMap<u32, dyn IDisposable>>,
    /// Os emissores de `_registrations` por `eventHandle` (ver as divergências).
    emitters: RefCell<HashMap<u32, Rc<dyn Any>>>,
    language_service: Option<Rc<dyn ILanguageService>>,
    language_configuration_service: Option<Rc<dyn ILanguageConfigurationService>>,
    language_features_service: LanguageFeaturesService,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    paste_edit_providers: Rc<RefCell<HashMap<u32, Rc<MainThreadPasteEditProvider>>>>,
    document_on_drop_edit_providers: Rc<RefCell<HashMap<u32, Rc<MainThreadDocumentOnDropEditProvider>>>>,
}

/// O `selector: IDocumentFilterDto[]` como o seletor que o registro entende (ver as divergências).
pub(crate) fn to_selector(selector: &[IDocumentFilterDto]) -> LanguageSelector {
    LanguageSelector::Array(
        selector
            .iter()
            .map(|filter| {
                LanguageSelector::Filter(LanguageFilter {
                    language: filter.language.clone(),
                    scheme: filter.scheme.clone(),
                    pattern: filter.pattern.as_ref().map(|pattern| match pattern {
                        GlobPatternDto::Pattern(pattern) => FilterPattern::Glob(pattern.clone()),
                        GlobPatternDto::Relative(relative) => FilterPattern::Relative(RelativePattern { base: relative.base.clone(), pattern: relative.pattern.clone() }),
                    }),
                    notebook_type: filter.notebook_type.clone(),
                    has_access_to_all_models: false,
                    exclusive: filter.exclusive.unwrap_or(false),
                    is_builtin: filter.is_builtin.unwrap_or(false),
                })
            })
            .collect(),
    )
}

fn legend_to_editor(legend: &SemanticTokensLegend) -> EditorLegend {
    EditorLegend { token_types: legend.token_types.clone(), token_modifiers: legend.token_modifiers.clone() }
}

impl MainThreadLanguageFeatures {
    /// Os serviços opcionais são os que o `TestInstantiationService` não estrito resolve como `undefined` (ver as divergências).
    pub fn new(
        ext_host_context: Rc<dyn IMainContext>,
        language_service: Option<Rc<dyn ILanguageService>>,
        language_configuration_service: Option<Rc<dyn ILanguageConfigurationService>>,
        language_features_service: LanguageFeaturesService,
        uri_identity_service: Rc<dyn IUriIdentityService>,
        inline_completions_unification_service: Option<Rc<dyn IInlineCompletionsUnificationService>>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let registrations = DisposableMap::new();
        base.register(Rc::clone(&registrations) as DisposableRef);

        if let Some(language_service) = &language_service {
            let configuration_service = language_configuration_service
                .clone()
                .unwrap_or_else(|| errors::throw(Error::type_error("Cannot read properties of undefined (reading 'getLanguageConfiguration')")));
            let update_all_word_definitions: Rc<dyn Fn()> = {
                let (context, language_service, configuration_service) = (Rc::clone(&ext_host_context), Rc::clone(language_service), Rc::clone(&configuration_service));
                Rc::new(move || {
                    let word_definition_dtos = language_service
                        .get_registered_language_ids()
                        .iter()
                        .map(|language_id| word_definition_dto(configuration_service.as_ref(), language_id))
                        .collect();
                    context.get_proxy(&ExtHostContext::EXT_HOST_LANGUAGE_FEATURES).rpc_set_word_definitions(word_definition_dtos);
                })
            };
            let (context, listener_service, update_all) = (Rc::clone(&ext_host_context), Rc::clone(&configuration_service), Rc::clone(&update_all_word_definitions));
            base.register(configuration_service.on_did_change().listen(move |event| match event.language_id.as_deref() {
                None | Some("") => update_all(),
                Some(language_id) => {
                    let dto = word_definition_dto(listener_service.as_ref(), language_id);
                    context.get_proxy(&ExtHostContext::EXT_HOST_LANGUAGE_FEATURES).rpc_set_word_definitions(vec![dto]);
                }
            }));
            update_all_word_definitions();
        }

        if let Some(service) = &inline_completions_unification_service {
            let (context, listener_service) = (Rc::clone(&ext_host_context), Rc::clone(service));
            base.register(service.on_did_state_change().listen(move |_| {
                context.get_proxy(&ExtHostContext::EXT_HOST_LANGUAGE_FEATURES).rpc_accept_inline_completions_unification_state(listener_service.state());
            }));
            ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_LANGUAGE_FEATURES).rpc_accept_inline_completions_unification_state(service.state());
        }

        Rc::new(Self {
            base,
            ext_host_context,
            registrations,
            emitters: RefCell::new(HashMap::new()),
            language_service,
            language_configuration_service,
            language_features_service,
            uri_identity_service,
            paste_edit_providers: Rc::new(RefCell::new(HashMap::new())),
            document_on_drop_edit_providers: Rc::new(RefCell::new(HashMap::new())),
        })
    }

    /// `this._proxy`: pedido ao contexto a cada uso (e não guardado no construtor), porque o host e o lado do workbench
    /// se conhecem mutuamente e o proxy do host só existe depois de o `ExtHostLanguageFeatures` ser criado (o `getProxy` do
    /// `TestRPCProtocol` do original é preguiçoso).
    fn proxy(&self) -> Rc<dyn ExtHostLanguageFeaturesShape> {
        self.ext_host_context.get_proxy(&ExtHostContext::EXT_HOST_LANGUAGE_FEATURES)
    }

    fn remote(&self, handle: u32) -> Remote {
        Remote { handle, proxy: self.proxy() }
    }

    fn set_registration(&self, handle: u32, registration: DisposableRef) {
        self.registrations.set(handle, registration, false);
    }

    /// O `if (typeof eventHandle === 'number') { const emitter = new Emitter(); this._registrations.set(eventHandle, emitter); ... }`
    /// comum a todo `$register*` com evento: devolve o evento do emissor novo.
    fn register_emitter(&self, event_handle: Option<u32>) -> Option<Event<()>> {
        event_handle.map(|event_handle| {
            let emitter = Emitter::<()>::new();
            self.registrations.set(event_handle, Rc::clone(&emitter) as DisposableRef, false);
            self.emitters.borrow_mut().insert(event_handle, Rc::clone(&emitter) as Rc<dyn Any>);
            emitter.event()
        })
    }

    /// O `const obj = this._registrations.get(eventHandle); if (obj instanceof Emitter) { obj.fire(...) }` dos `$emit*Event`.
    fn fire_emitter(&self, event_handle: u32) {
        let emitter = self.emitters.borrow().get(&event_handle).cloned();
        if let Some(emitter) = emitter.and_then(|emitter| emitter.downcast::<Emitter<()>>().ok()) {
            emitter.fire(());
        }
    }
}

impl IDisposable for MainThreadLanguageFeatures {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O `{ languageId, regexSource, regexFlags }` da definição de palavra de uma linguagem, como `updateAllWordDefinitions` monta.
fn word_definition_dto(configuration_service: &dyn ILanguageConfigurationService, language_id: &str) -> ILanguageWordDefinitionDto {
    let word_definition = configuration_service.get_language_configuration(language_id).get_word_definition();
    ILanguageWordDefinitionDto {
        language_id: language_id.to_owned(),
        regex_source: String::from_utf16_lossy(word_definition.source()),
        regex_flags: word_definition.flags().to_flags_string(),
    }
}

// ---- hierarquias e símbolos do workspace -------------------------------------------------------------

struct MainThreadCallHierarchyProvider {
    remote: Remote,
}

impl CallHierarchyProvider for MainThreadCallHierarchyProvider {
    fn prepare_call_hierarchy(&self, document: &Rc<TextModel>, position: Position, token: &CancellationToken) -> ProviderResult<Rc<CallHierarchySession>> {
        let (remote, uri, token) = (self.remote.clone(), document.uri(), token.clone());
        async_fn(async move {
            let items = remote.proxy.rpc_prepare_call_hierarchy(remote.handle, &uri, position, token).await?;
            let Some(items) = items.filter(|items| !items.is_empty()) else {
                return Ok(None);
            };
            let roots = items.clone();
            Ok(Some(Rc::new(CallHierarchySession::new(roots, move || {
                for item in &items {
                    remote.proxy.rpc_release_call_hierarchy(remote.handle, &item.session_id);
                }
            }))))
        })
    }

    fn provide_incoming_calls(&self, item: &CallHierarchyItem, token: &CancellationToken) -> ProviderResult<Vec<IIncomingCallDto>> {
        self.remote.proxy.rpc_provide_call_hierarchy_incoming_calls(self.remote.handle, &item.session_id, &item.item_id, token.clone())
    }

    fn provide_outgoing_calls(&self, item: &CallHierarchyItem, token: &CancellationToken) -> ProviderResult<Vec<IOutgoingCallDto>> {
        self.remote.proxy.rpc_provide_call_hierarchy_outgoing_calls(self.remote.handle, &item.session_id, &item.item_id, token.clone())
    }
}

struct MainThreadTypeHierarchyProvider {
    remote: Remote,
}

impl TypeHierarchyProvider for MainThreadTypeHierarchyProvider {
    fn prepare_type_hierarchy(&self, document: &Rc<TextModel>, position: Position, token: &CancellationToken) -> ProviderResult<Rc<TypeHierarchySession>> {
        let (remote, uri, token) = (self.remote.clone(), document.uri(), token.clone());
        async_fn(async move {
            let Some(items) = remote.proxy.rpc_prepare_type_hierarchy(remote.handle, &uri, position, token).await? else {
                return Ok(None);
            };
            let roots = items.clone();
            Ok(Some(Rc::new(TypeHierarchySession::new(roots, move || {
                for item in &items {
                    remote.proxy.rpc_release_type_hierarchy(remote.handle, &item.session_id);
                }
            }))))
        })
    }

    fn provide_supertypes(&self, item: &TypeHierarchyItem, token: &CancellationToken) -> ProviderResult<Vec<TypeHierarchyItem>> {
        self.remote.proxy.rpc_provide_type_hierarchy_supertypes(self.remote.handle, &item.session_id, &item.item_id, token.clone())
    }

    fn provide_subtypes(&self, item: &TypeHierarchyItem, token: &CancellationToken) -> ProviderResult<Vec<TypeHierarchyItem>> {
        self.remote.proxy.rpc_provide_type_hierarchy_subtypes(self.remote.handle, &item.session_id, &item.item_id, token.clone())
    }
}

/// `_reviveWorkspaceSymbolDto(data)`: o `_reviveLocationDto` do original só revive o `uri`, e o `location` segue sem
/// validação (o `uri` e o `range` podem faltar, ver [`IWorkspaceSymbol::location`]).
fn revive_workspace_symbol_dto(data: &IWorkspaceSymbolDto) -> IWorkspaceSymbol {
    IWorkspaceSymbol {
        name: data.name.clone(),
        container_name: data.container_name.clone(),
        kind: data.kind,
        tags: data.tags.clone(),
        location: data.location.clone(),
        cache_id: data.cache_id,
    }
}

fn workspace_symbol_to_dto(symbol: &IWorkspaceSymbol) -> IWorkspaceSymbolDto {
    IWorkspaceSymbolDto {
        cache_id: symbol.cache_id,
        name: symbol.name.clone(),
        container_name: symbol.container_name.clone(),
        kind: symbol.kind,
        tags: symbol.tags.clone(),
        location: symbol.location.clone(),
    }
}

struct MainThreadWorkspaceSymbolProvider {
    remote: Remote,
    supports_resolve: bool,
    last_result_id: Rc<Cell<Option<u32>>>,
}

impl IWorkspaceSymbolProvider for MainThreadWorkspaceSymbolProvider {
    fn provide_workspace_symbols(&self, search: &str, token: &CancellationToken) -> Promise<Vec<IWorkspaceSymbol>> {
        let (remote, search, token, last_result_id) = (self.remote.clone(), search.to_owned(), token.clone(), Rc::clone(&self.last_result_id));
        async_fn(async move {
            let result = remote.proxy.rpc_provide_workspace_symbols(remote.handle, &search, token).await?;
            if let Some(last_result_id) = last_result_id.get() {
                remote.proxy.rpc_release_workspace_symbols(remote.handle, last_result_id);
            }
            last_result_id.set(result.cache_id);
            Ok(result.symbols.iter().map(revive_workspace_symbol_dto).collect())
        })
    }

    fn has_resolve_workspace_symbol(&self) -> bool {
        self.supports_resolve
    }

    fn resolve_workspace_symbol(&self, item: &IWorkspaceSymbol, token: &CancellationToken) -> ProviderResult<IWorkspaceSymbol> {
        self.remote
            .proxy
            .rpc_resolve_workspace_symbol(self.remote.handle, workspace_symbol_to_dto(item), token.clone())
            .then(|resolved_item| resolved_item.map(|resolved_item| revive_workspace_symbol_dto(&resolved_item)))
    }
}

// ---- MainThreadLanguageFeaturesShape ------------------------------------------------------------------

impl MainThreadLanguageFeaturesShape for MainThreadLanguageFeatures {
    fn rpc_unregister(&self, handle: u32) {
        self.registrations.delete_and_dispose(&handle);
        self.emitters.borrow_mut().remove(&handle);
    }

    // --- outline

    fn rpc_register_document_symbol_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, display_name: &str) {
        let provider: Rc<dyn DocumentSymbolProvider> = Rc::new(MainThreadDocumentSymbolProvider { remote: self.remote(handle), display_name: display_name.to_owned() });
        self.set_registration(handle, self.language_features_service.document_symbol_provider.register(to_selector(&selector), provider));
    }

    // --- code lens

    fn rpc_register_code_lens_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>, event_handle: Option<u32>) {
        let on_did_change = self.register_emitter(event_handle);
        let provider = Rc::new(MainThreadCodeLensProvider { remote: self.remote(handle), on_did_change });
        self.set_registration(handle, self.language_features_service.code_lens_provider.register(to_selector(&selector), provider));
    }

    fn rpc_emit_code_lens_event(&self, event_handle: u32, _event: Option<je_base::dynamic::Dynamic>) {
        self.fire_emitter(event_handle);
    }

    // --- declaration

    fn rpc_register_definition_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider = Rc::new(MainThreadLocationLinkProvider { remote: self.remote(handle), kind: LocationKind::Definition });
        self.set_registration(handle, self.language_features_service.definition_provider.register(to_selector(&selector), provider));
    }

    fn rpc_register_declaration_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider = Rc::new(MainThreadLocationLinkProvider { remote: self.remote(handle), kind: LocationKind::Declaration });
        self.set_registration(handle, self.language_features_service.declaration_provider.register(to_selector(&selector), provider));
    }

    fn rpc_register_implementation_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider = Rc::new(MainThreadLocationLinkProvider { remote: self.remote(handle), kind: LocationKind::Implementation });
        self.set_registration(handle, self.language_features_service.implementation_provider.register(to_selector(&selector), provider));
    }

    fn rpc_register_type_definition_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider = Rc::new(MainThreadLocationLinkProvider { remote: self.remote(handle), kind: LocationKind::TypeDefinition });
        self.set_registration(handle, self.language_features_service.type_definition_provider.register(to_selector(&selector), provider));
    }

    // --- extra info

    fn rpc_register_hover_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider = Rc::new(MainThreadHoverProvider { remote: self.remote(handle) });
        self.set_registration(handle, self.language_features_service.hover_provider.register(to_selector(&selector), provider));
    }

    // --- debug hover

    fn rpc_register_evaluatable_expression_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider = Rc::new(MainThreadEvaluatableExpressionProvider { remote: self.remote(handle) });
        self.set_registration(handle, self.language_features_service.evaluatable_expression_provider.register(to_selector(&selector), provider));
    }

    // --- inline values

    fn rpc_register_inline_values_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, event_handle: Option<u32>) {
        let on_did_change_inline_values = self.register_emitter(event_handle);
        let provider = Rc::new(MainThreadInlineValuesProvider { remote: self.remote(handle), on_did_change_inline_values });
        self.set_registration(handle, self.language_features_service.inline_values_provider.register(to_selector(&selector), provider));
    }

    fn rpc_emit_inline_values_event(&self, event_handle: u32, _event: Option<je_base::dynamic::Dynamic>) {
        self.fire_emitter(event_handle);
    }

    // --- occurrences

    fn rpc_register_document_highlight_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider: Rc<dyn DocumentHighlightProvider> = Rc::new(MainThreadDocumentHighlightProvider { remote: self.remote(handle) });
        self.set_registration(handle, self.language_features_service.document_highlight_provider.register(to_selector(&selector), provider));
    }

    fn rpc_register_multi_document_highlight_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let language_selector = to_selector(&selector);
        let provider: Rc<dyn MultiDocumentHighlightProvider> =
            Rc::new(MainThreadMultiDocumentHighlightProvider { remote: self.remote(handle), selector: language_selector.clone() });
        self.set_registration(handle, self.language_features_service.multi_document_highlight_provider.register(language_selector, provider));
    }

    // --- linked editing

    fn rpc_register_linked_editing_range_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider = Rc::new(MainThreadLinkedEditingRangeProvider { remote: self.remote(handle) });
        self.set_registration(handle, self.language_features_service.linked_editing_range_provider.register(to_selector(&selector), provider));
    }

    // --- references

    fn rpc_register_reference_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider = Rc::new(MainThreadReferenceProvider { remote: self.remote(handle) });
        self.set_registration(handle, self.language_features_service.reference_provider.register(to_selector(&selector), provider));
    }

    // --- code actions

    fn rpc_register_code_action_support(
        &self,
        handle: u32,
        selector: Vec<IDocumentFilterDto>,
        metadata: ICodeActionProviderMetadataDto,
        display_name: &str,
        extension_id: &str,
        supports_resolve: bool,
    ) {
        let provider: Rc<dyn CodeActionProvider> = Rc::new(MainThreadCodeActionProvider {
            remote: self.remote(handle),
            uri_identity_service: Rc::clone(&self.uri_identity_service),
            provided_kinds: metadata.provided_kinds,
            documentation: metadata.documentation.as_deref().map(documentation_from_dto),
            display_name: display_name.to_owned(),
            extension_id: extension_id.to_owned(),
            supports_resolve,
            cache_ids: Rc::new(RefCell::new(HashMap::new())),
        });
        self.set_registration(handle, self.language_features_service.code_action_provider.register(to_selector(&selector), provider));
    }

    // --- copy paste action provider

    fn rpc_register_paste_edit_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, metadata: IPasteEditProviderMetadataDto) {
        let provider = MainThreadPasteEditProvider::new(handle, self.proxy(), &metadata, Rc::clone(&self.uri_identity_service));
        self.paste_edit_providers.borrow_mut().insert(handle, Rc::clone(&provider));
        let providers = Rc::clone(&self.paste_edit_providers);
        self.set_registration(
            handle,
            combined_disposable(vec![
                self.language_features_service.document_paste_edit_provider.register(to_selector(&selector), provider),
                to_disposable(move || {
                    providers.borrow_mut().remove(&handle);
                }),
            ]),
        );
    }

    fn rpc_resolve_paste_file_data(&self, handle: u32, request_id: u32, data_id: &str) -> Promise<VSBuffer> {
        match self.paste_edit_providers.borrow().get(&handle) {
            Some(provider) => provider.resolve_file_data(request_id, data_id),
            None => Promise::rejected(Error::new("Could not find provider")),
        }
    }

    // --- formatting

    fn rpc_register_document_formatting_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>, extension_id: &ExtensionIdentifier, display_name: &str) {
        let provider = Rc::new(MainThreadDocumentFormattingProvider { remote: self.remote(handle), extension_id: extension_id.clone(), display_name: display_name.to_owned() });
        self.set_registration(handle, self.language_features_service.document_formatting_edit_provider.register(to_selector(&selector), provider));
    }

    fn rpc_register_range_formatting_support(
        &self,
        handle: u32,
        selector: Vec<IDocumentFilterDto>,
        extension_id: &ExtensionIdentifier,
        display_name: &str,
        support_ranges: bool,
    ) {
        let provider = Rc::new(MainThreadRangeFormattingProvider {
            remote: self.remote(handle),
            extension_id: extension_id.clone(),
            display_name: display_name.to_owned(),
            supports_ranges: support_ranges,
        });
        self.set_registration(handle, self.language_features_service.document_range_formatting_edit_provider.register(to_selector(&selector), provider));
    }

    fn rpc_register_on_type_formatting_support(
        &self,
        handle: u32,
        selector: Vec<IDocumentFilterDto>,
        auto_format_trigger_characters: Vec<String>,
        extension_id: &ExtensionIdentifier,
    ) {
        let provider = Rc::new(MainThreadOnTypeFormattingProvider { remote: self.remote(handle), extension_id: extension_id.clone(), auto_format_trigger_characters });
        self.set_registration(handle, self.language_features_service.on_type_formatting_edit_provider.register(to_selector(&selector), provider));
    }

    // --- navigate type

    fn rpc_register_navigate_type_support(&self, handle: u32, supports_resolve: bool) {
        let provider = Rc::new(MainThreadWorkspaceSymbolProvider { remote: self.remote(handle), supports_resolve, last_result_id: Rc::new(Cell::new(None)) });
        self.set_registration(handle, WorkspaceSymbolProviderRegistry::register(provider));
    }

    // --- rename

    fn rpc_register_rename_support(&self, handle: u32, selector: Vec<IDocumentFilterDto>, supports_resolve_initial_values: bool) {
        let provider = Rc::new(MainThreadRenameProvider {
            remote: self.remote(handle),
            uri_identity_service: Rc::clone(&self.uri_identity_service),
            supports_resolve_location: supports_resolve_initial_values,
        });
        self.set_registration(handle, self.language_features_service.rename_provider.register(to_selector(&selector), provider));
    }

    fn rpc_register_new_symbol_names_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider = Rc::new(MainThreadNewSymbolNamesProvider {
            remote: self.remote(handle),
            supports_automatic_new_symbol_names_trigger_kind: self.proxy().rpc_supports_automatic_new_symbol_names_trigger_kind(handle),
        });
        self.set_registration(handle, self.language_features_service.new_symbol_names_provider.register(to_selector(&selector), provider));
    }

    // --- semantic tokens

    fn rpc_register_document_semantic_tokens_provider(
        &self,
        handle: u32,
        selector: Vec<IDocumentFilterDto>,
        legend: SemanticTokensLegend,
        event_handle: Option<u32>,
    ) {
        let on_did_change = self.register_emitter(event_handle);
        let provider = Rc::new(MainThreadDocumentSemanticTokensProvider { remote: self.remote(handle), legend: legend_to_editor(&legend), on_did_change });
        self.set_registration(handle, self.language_features_service.document_semantic_tokens_provider.register(to_selector(&selector), provider));
    }

    fn rpc_emit_document_semantic_tokens_event(&self, event_handle: u32) {
        self.fire_emitter(event_handle);
    }

    fn rpc_register_document_range_semantic_tokens_provider(
        &self,
        handle: u32,
        selector: Vec<IDocumentFilterDto>,
        legend: SemanticTokensLegend,
        event_handle: Option<u32>,
    ) {
        let on_did_change = self.register_emitter(event_handle);
        let provider = Rc::new(MainThreadDocumentRangeSemanticTokensProvider { remote: self.remote(handle), legend: legend_to_editor(&legend), on_did_change });
        self.set_registration(handle, self.language_features_service.document_range_semantic_tokens_provider.register(to_selector(&selector), provider));
    }

    fn rpc_emit_document_range_semantic_tokens_event(&self, event_handle: u32) {
        self.fire_emitter(event_handle);
    }

    // --- suggest

    fn rpc_register_completions_provider(
        &self,
        handle: u32,
        selector: Vec<IDocumentFilterDto>,
        trigger_characters: Vec<String>,
        supports_resolve_details: bool,
        extension_id: &ExtensionIdentifier,
    ) {
        let provider: Rc<dyn CompletionItemProvider> = Rc::new(MainThreadCompletionProvider {
            remote: self.remote(handle),
            extension_id: extension_id.clone(),
            debug_display_name: format!("{}({})", extension_id.value, trigger_characters.join("")),
            trigger_characters,
            supports_resolve_details,
            item_ids: Rc::new(RefCell::new(HashMap::new())),
        });
        self.set_registration(handle, self.language_features_service.completion_provider.register(to_selector(&selector), provider));
    }

    // --- parameter hints

    fn rpc_register_signature_help_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, metadata: ISignatureHelpProviderMetadataDto) {
        let provider: Rc<dyn SignatureHelpProvider> = Rc::new(MainThreadSignatureHelpProvider {
            remote: self.remote(handle),
            trigger_characters: metadata.trigger_characters,
            retrigger_characters: metadata.retrigger_characters,
            ids: Rc::new(RefCell::new(HashMap::new())),
        });
        self.set_registration(handle, self.language_features_service.signature_help_provider.register(to_selector(&selector), provider));
    }

    // --- inline hints

    fn rpc_register_inlay_hints_provider(
        &self,
        handle: u32,
        selector: Vec<IDocumentFilterDto>,
        supports_resolve: bool,
        event_handle: Option<u32>,
        display_name: Option<&str>,
    ) {
        let on_did_change_inlay_hints = self.register_emitter(event_handle);
        let provider = Rc::new(MainThreadInlayHintsProvider {
            remote: self.remote(handle),
            display_name: display_name.map(str::to_owned),
            supports_resolve,
            on_did_change_inlay_hints,
        });
        self.set_registration(handle, self.language_features_service.inlay_hints_provider.register(to_selector(&selector), provider));
    }

    fn rpc_emit_inlay_hints_event(&self, event_handle: u32) {
        self.fire_emitter(event_handle);
    }

    // --- links

    fn rpc_register_document_link_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, supports_resolve: bool) {
        let provider = Rc::new(MainThreadLinkProvider { remote: self.remote(handle), supports_resolve });
        self.set_registration(handle, self.language_features_service.link_provider.register(to_selector(&selector), provider));
    }

    // --- colors

    fn rpc_register_document_color_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider = Rc::new(MainThreadColorProvider { remote: self.remote(handle) });
        self.set_registration(handle, self.language_features_service.color_provider.register(to_selector(&selector), provider));
    }

    // --- folding

    fn rpc_register_folding_range_provider(
        &self,
        handle: u32,
        selector: Vec<IDocumentFilterDto>,
        _extension_id: &ExtensionIdentifier,
        event_handle: Option<u32>,
    ) {
        let on_did_change = self.register_emitter(event_handle);
        let provider = Rc::new(MainThreadFoldingRangeProvider { remote: self.remote(handle), on_did_change });
        self.set_registration(handle, self.language_features_service.folding_range_provider.register(to_selector(&selector), provider));
    }

    fn rpc_emit_folding_range_event(&self, event_handle: u32, _event: Option<je_base::dynamic::Dynamic>) {
        self.fire_emitter(event_handle);
    }

    // -- smart select

    fn rpc_register_selection_range_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider: Rc<dyn SelectionRangeProvider> = Rc::new(MainThreadSelectionRangeProvider { remote: self.remote(handle) });
        self.set_registration(handle, self.language_features_service.selection_range_provider.register(to_selector(&selector), provider));
    }

    // --- call hierarchy

    fn rpc_register_call_hierarchy_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider = Rc::new(MainThreadCallHierarchyProvider { remote: self.remote(handle) });
        self.set_registration(handle, call_hierarchy_provider_registry().register(to_selector(&selector), provider));
    }

    // --- type hierarchy

    fn rpc_register_type_hierarchy_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>) {
        let provider = Rc::new(MainThreadTypeHierarchyProvider { remote: self.remote(handle) });
        self.set_registration(handle, type_hierarchy_provider_registry().register(to_selector(&selector), provider));
    }

    // --- document drop Edits

    fn rpc_register_document_on_drop_edit_provider(&self, handle: u32, selector: Vec<IDocumentFilterDto>, metadata: Option<IDocumentDropEditProviderMetadata>) {
        let provider = MainThreadDocumentOnDropEditProvider::new(handle, self.proxy(), metadata.as_ref(), Rc::clone(&self.uri_identity_service));
        self.document_on_drop_edit_providers.borrow_mut().insert(handle, Rc::clone(&provider));
        let providers = Rc::clone(&self.document_on_drop_edit_providers);
        self.set_registration(
            handle,
            combined_disposable(vec![
                self.language_features_service.document_drop_edit_provider.register(to_selector(&selector), provider),
                to_disposable(move || {
                    providers.borrow_mut().remove(&handle);
                }),
            ]),
        );
    }

    fn rpc_resolve_document_on_drop_file_data(&self, handle: u32, request_id: u32, data_id: &str) -> Promise<VSBuffer> {
        match self.document_on_drop_edit_providers.borrow().get(&handle) {
            Some(provider) => provider.resolve_document_on_drop_file_data(request_id, data_id),
            None => Promise::rejected(Error::new("Could not find provider")),
        }
    }

    // --- configuration

    fn rpc_set_language_configuration(&self, handle: u32, language_id: &str, configuration: ILanguageConfigurationDto) {
        let mut revived = LanguageConfiguration {
            comments: configuration.comments.clone(),
            brackets: configuration.brackets.clone(),
            word_pattern: configuration.word_pattern.as_ref().map(revive_reg_exp),
            indentation_rules: configuration.indentation_rules.as_ref().map(revive_indentation_rule),
            on_enter_rules: configuration.on_enter_rules.as_deref().map(revive_on_enter_rules),

            auto_closing_pairs: None,
            surrounding_pairs: None,
            electric_character_support: None,
            ..LanguageConfiguration::default()
        };

        if let Some(auto_closing_pairs) = &configuration.auto_closing_pairs {
            revived.auto_closing_pairs = Some(revive_auto_closing_pairs(auto_closing_pairs));
        } else if let Some(character_pair_support) = &configuration.character_pair_support {
            // backwards compatibility
            revived.auto_closing_pairs = Some(
                character_pair_support
                    .auto_closing_pairs
                    .iter()
                    .map(|pair| AutoClosingPairConditional { open: pair.open.clone(), close: pair.close.clone(), not_in: pair.not_in.clone(), before_text: None })
                    .collect(),
            );
        }

        if let Some(doc_comment) = configuration.electric_character_support.as_ref().and_then(|support| support.doc_comment.as_ref()) {
            revived.electric_character_support = Some(ElectricCharacterSupportConfig {
                doc_comment: Some(DocComment { open: doc_comment.open.clone(), close: doc_comment.close.clone() }),
            });
        }

        let (Some(language_service), Some(language_configuration_service)) = (&self.language_service, &self.language_configuration_service) else {
            errors::throw(Error::type_error("Cannot read properties of undefined (reading 'isRegisteredLanguageId')"));
        };
        if language_service.is_registered_language_id(Some(language_id)) {
            self.set_registration(handle, language_configuration_service.register(language_id, revived, Some(100)));
        }
    }
}

// ---- configuração de linguagem (os `_revive*`) ------------------------------------------------------

/// `_reviveRegExp`: `new RegExp(regExp.pattern, regExp.flags)`; um padrão inválido lança, como o `SyntaxError` do original.
fn revive_reg_exp(reg_exp: &IRegExpDto) -> Rc<JsRegExp> {
    match JsRegExp::from_str(&reg_exp.pattern, reg_exp.flags.as_deref().unwrap_or("")) {
        Ok(reg_exp) => Rc::new(reg_exp),
        Err(error) => errors::throw(Error::named("SyntaxError", error.to_string())),
    }
}

/// `_reviveIndentationRule`.
fn revive_indentation_rule(indentation_rule: &IIndentationRuleDto) -> IndentationRule {
    IndentationRule {
        decrease_indent_pattern: revive_reg_exp(&indentation_rule.decrease_indent_pattern),
        increase_indent_pattern: revive_reg_exp(&indentation_rule.increase_indent_pattern),
        indent_next_line_pattern: indentation_rule.indent_next_line_pattern.as_ref().map(revive_reg_exp),
        un_indented_line_pattern: indentation_rule.un_indented_line_pattern.as_ref().map(revive_reg_exp),
    }
}

/// `_reviveOnEnterRule`.
fn revive_on_enter_rule(on_enter_rule: &IOnEnterRuleDto) -> OnEnterRule {
    OnEnterRule {
        before_text: revive_reg_exp(&on_enter_rule.before_text),
        after_text: on_enter_rule.after_text.as_ref().map(revive_reg_exp),
        previous_line_text: on_enter_rule.previous_line_text.as_ref().map(revive_reg_exp),
        action: on_enter_rule.action.clone(),
    }
}

/// `_reviveOnEnterRules`.
fn revive_on_enter_rules(on_enter_rules: &[IOnEnterRuleDto]) -> Vec<OnEnterRule> {
    on_enter_rules.iter().map(revive_on_enter_rule).collect()
}

/// `_reviveAutoClosingPairs`.
fn revive_auto_closing_pairs(auto_closing_pairs: &[IAutoClosingPairDto]) -> Vec<AutoClosingPairConditional> {
    auto_closing_pairs
        .iter()
        .map(|auto_closing_pair| AutoClosingPairConditional {
            open: auto_closing_pair.open.clone(),
            close: auto_closing_pair.close.clone(),
            not_in: auto_closing_pair.not_in.clone(),
            before_text: auto_closing_pair.before_text.as_ref().map(revive_reg_exp),
        })
        .collect()
}
