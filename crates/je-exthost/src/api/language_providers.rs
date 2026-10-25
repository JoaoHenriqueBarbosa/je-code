//! Os registros de provedor de `vscode.languages` da segunda leva: `registerCodeLensProvider` (com `resolveCodeLens` e
//! `onDidChangeCodeLenses`), `registerReferenceProvider`, `registerRenameProvider` (com `prepareRename`),
//! `registerSignatureHelpProvider` (caracteres soltos ou `{ triggerCharacters, retriggerCharacters }`), `registerDocumentHighlightProvider`,
//! `registerOnTypeFormattingEditProvider`, `registerFoldingRangeProvider` (com `onDidChangeFoldingRanges`),
//! `registerSelectionRangeProvider`, `registerDocumentSemanticTokensProvider` (com `provideDocumentSemanticTokensEdits` e
//! `onDidChangeSemanticTokens`), `registerDocumentRangeSemanticTokensProvider` e `registerInlayHintsProvider` (com `resolveInlayHint` e
//! `onDidChangeInlayHints`), e, na terceira leva, `registerDocumentLinkProvider` (com `resolveDocumentLink`), `registerColorProvider`,
//! `registerCallHierarchyProvider`, `registerTypeHierarchyProvider` e `registerWorkspaceSymbolProvider` (com `resolveWorkspaceSymbol`).
//!
//! Ligam o provedor JavaScript (por referência, com o `this` da classe) ao `ExtHostLanguageFeatures` portado, no mesmo molde dos de
//! `languages`: a infraestrutura (`JsProvider`, `Registrar`, `member`, `simple`, `Originals`) é a de lá, as classes de valor e os leitores
//! estão em `language_provider_types`. Os `has_*` do trait são `typeof provider.resolveX === 'function'` lido no objeto do provedor.
//!
//! Falta (o serviço do host tem, falta só a ponte): `registerEvaluatableExpressionProvider`, `registerInlineValuesProvider`,
//! `registerLinkedEditingRangeProvider`, `registerMultiDocumentHighlightProvider`, `registerDocumentPasteEditProvider`,
//! `registerDocumentDropEditProvider` e as propostas.
//!
//! Divergências:
//! - Os itens de hierarquia (`CallHierarchyItem`, `TypeHierarchyItem`) e o `SymbolInformation` do `resolveWorkspaceSymbol` chegam ao provedor
//!   como objetos novos feitos do valor do host (o host os guarda por valor, não por referência): uma propriedade extra que o provedor
//!   pôs no item devolvido não volta nele.
//! - O evento `onDidChangeX` do provedor é lido uma vez, no registro: o evento que o provedor troca depois não vale.
//! - `FoldingContext` é um objeto vazio (como no `vscode.d.ts`).
//! - `provideDocumentSemanticTokens` devolve `SemanticTokens`, e `provideDocumentSemanticTokensEdits` aceita também `SemanticTokensEdits`
//!   (o objeto com a lista `edits`).

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Event;
use je_editor::folding::syntax_range_provider::FoldingContext;
use je_workbench::api::common::ext_host_document_data::TextDocument;
use je_workbench::api::common::ext_host_language_features::ExtHostLanguageFeatures;
use je_workbench::api::common::ext_host_language_features_api::{
    ApiCallHierarchyProvider, ApiCodeLensProvider, ApiColorPresentationContext, ApiDocumentColorProvider, ApiDocumentHighlightProvider, ApiDocumentLinkProvider,
    ApiDocumentRangeSemanticTokensProvider, ApiDocumentSemanticTokensProvider, ApiFoldingRangeProvider, ApiInlayHintsProvider, ApiOnTypeFormattingEditProvider, ApiPrepareRename,
    ApiProviderResult, ApiReferenceProvider, ApiRenameProvider, ApiSelectionRangeProvider, ApiSemanticTokensResult, ApiSignatureHelpContext, ApiSignatureHelpProvider,
    ApiSignatureHelpRef, ApiTypeHierarchyProvider, ApiWorkspaceSymbolProvider, CodeLensRef, DocumentLinkRef, InlayHintRef, OneOrMany, SignatureHelpMetadataOrTriggerChars,
};
use je_workbench::api::common::ext_host_protocol::{FormattingOptions, ReferenceContext};
use je_workbench::api::common::ext_host_type_converters::DocumentSelectorInput;
use je_workbench::api::common::ext_host_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, CodeLens, Color, ColorInformation, ColorPresentation, DocumentHighlight, DocumentLink,
    FoldingRange, InlayHint, Location, Position, Range, SelectionRange, SemanticTokens, SemanticTokensLegend, SymbolInformation, TextEdit, TypeHierarchyItem, WorkspaceEdit,
};
use je_extensions::scanner::ExtensionDescription;
use je_base::lifecycle::DisposableRef;
use zjsc::api::embed_node::{JsData, NodeRunner};

use crate::api::language_provider_types::{
    call_hierarchy_incoming_calls_from_js, call_hierarchy_item_to_js, call_hierarchy_items_from_js, call_hierarchy_outgoing_calls_from_js, code_lens_from_js,
    code_lenses_from_js, color_informations_from_js, color_presentations_from_js, document_highlights_from_js, document_link_from_js, document_links_from_js,
    folding_ranges_from_js, inlay_hint_from_js, inlay_hints_from_js, locations_from_js, prepare_rename_from_js, selection_ranges_from_js, semantic_tokens_from_js,
    semantic_tokens_legend_from_js, semantic_tokens_result_from_js, signature_help_from_js, signature_help_to_js, symbol_information_result_from_js,
    symbol_information_to_js, symbol_informations_from_js, type_hierarchy_item_list_from_js, type_hierarchy_item_to_js, type_hierarchy_items_from_js,
};
use crate::api::language_types::{strings_of, text_edits_from_js};
use crate::api::languages::{formatting_options_to_js, member, simple, JsProvider, Originals, Registrar};
use crate::api::types::{position_data, range_data};
use crate::convert::{api_to_js, exported, field, is_nullish, opt_string};

// --- lente de código

struct CodeLensProvider {
    js: JsProvider,
    originals: Rc<Originals<CodeLens>>,
}

impl CodeLensProvider {
    fn new(js: JsProvider) -> Self {
        Self { js, originals: Rc::default() }
    }
}

impl ApiCodeLensProvider for CodeLensProvider {
    fn on_did_change_code_lenses(&self) -> Option<Event<()>> {
        self.js.change_event("onDidChangeCodeLenses")
    }

    fn provide_code_lenses(&self, document: &TextDocument, token: CancellationToken) -> ApiProviderResult<Vec<CodeLensRef>> {
        let originals = self.originals.clone();
        self.js.invoke("provideCodeLenses", vec![self.js.document(document), self.js.token(token)], move |value| {
            let lenses = code_lenses_from_js(value)?;
            if let (Some(lenses), JsData::Array(raw)) = (&lenses, value) {
                originals.remember(lenses.iter().zip(raw));
            }
            Ok(lenses)
        })
    }

    fn has_resolve_code_lens(&self) -> bool {
        self.js.has_method("resolveCodeLens")
    }

    fn resolve_code_lens(&self, code_lens: &CodeLensRef, token: CancellationToken) -> ApiProviderResult<CodeLensRef> {
        self.js.resolve("resolveCodeLens", &self.originals, code_lens, token, code_lens_from_js)
    }
}

// --- referências, destaque

struct ReferenceProvider(JsProvider);

impl ApiReferenceProvider for ReferenceProvider {
    fn provide_references(&self, document: &TextDocument, position: &Position, context: &ReferenceContext, token: CancellationToken) -> ApiProviderResult<Vec<Location>> {
        let context = JsData::Object(vec![("includeDeclaration".to_owned(), JsData::Bool(context.include_declaration))]);
        self.0.invoke("provideReferences", vec![self.0.document(document), position_data(*position), context, self.0.token(token)], locations_from_js)
    }
}

struct DocumentHighlightProvider(JsProvider);

impl ApiDocumentHighlightProvider for DocumentHighlightProvider {
    fn provide_document_highlights(&self, document: &TextDocument, position: &Position, token: CancellationToken) -> ApiProviderResult<Vec<DocumentHighlight>> {
        self.0.invoke("provideDocumentHighlights", vec![self.0.document(document), position_data(*position), self.0.token(token)], document_highlights_from_js)
    }
}

// --- renomeação

struct RenameProvider(JsProvider);

impl ApiRenameProvider for RenameProvider {
    fn provide_rename_edits(&self, document: &TextDocument, position: &Position, new_name: &str, token: CancellationToken) -> ApiProviderResult<WorkspaceEdit> {
        let edits = self.0.edits.clone();
        let args = vec![self.0.document(document), position_data(*position), JsData::Str(new_name.to_owned()), self.0.token(token)];
        self.0.invoke("provideRenameEdits", args, move |value| {
            if is_nullish(Some(value)) {
                return Ok(None);
            }
            edits.resolved_edit(value).map(Some).ok_or_else(|| Error::new("provideRenameEdits must return a WorkspaceEdit"))
        })
    }

    fn has_prepare_rename(&self) -> bool {
        self.0.has_method("prepareRename")
    }

    fn prepare_rename(&self, document: &TextDocument, position: &Position, token: CancellationToken) -> ApiProviderResult<ApiPrepareRename> {
        self.0.invoke("prepareRename", vec![self.0.document(document), position_data(*position), self.0.token(token)], prepare_rename_from_js)
    }
}

// --- assinatura

struct SignatureHelpProvider(JsProvider);

impl ApiSignatureHelpProvider for SignatureHelpProvider {
    fn provide_signature_help(
        &self,
        document: &TextDocument,
        position: &Position,
        token: CancellationToken,
        context: &ApiSignatureHelpContext,
    ) -> ApiProviderResult<ApiSignatureHelpRef> {
        let context = JsData::Object(vec![
            ("triggerKind".to_owned(), JsData::Num(f64::from(context.trigger_kind.value()))),
            ("triggerCharacter".to_owned(), context.trigger_character.clone().map_or(JsData::Undefined, JsData::Str)),
            ("isRetrigger".to_owned(), JsData::Bool(context.is_retrigger)),
            ("activeSignatureHelp".to_owned(), context.active_signature_help.as_ref().map_or(JsData::Undefined, |help| signature_help_to_js(&help.borrow()))),
        ]);
        self.0.invoke("provideSignatureHelp", vec![self.0.document(document), position_data(*position), self.0.token(token), context], signature_help_from_js)
    }
}

// --- formatação ao digitar

struct OnTypeFormattingProvider(JsProvider);

impl ApiOnTypeFormattingEditProvider for OnTypeFormattingProvider {
    fn provide_on_type_formatting_edits(
        &self,
        document: &TextDocument,
        position: &Position,
        ch: &str,
        options: &FormattingOptions,
        token: CancellationToken,
    ) -> ApiProviderResult<Vec<TextEdit>> {
        let args = vec![self.0.document(document), position_data(*position), JsData::Str(ch.to_owned()), formatting_options_to_js(options), self.0.token(token)];
        self.0.invoke("provideOnTypeFormattingEdits", args, text_edits_from_js)
    }
}

// --- dobra, seleção

struct FoldingProvider(JsProvider);

impl ApiFoldingRangeProvider for FoldingProvider {
    fn on_did_change_folding_ranges(&self) -> Option<Event<()>> {
        self.0.change_event("onDidChangeFoldingRanges")
    }

    fn provide_folding_ranges(&self, document: &TextDocument, _context: &FoldingContext, token: CancellationToken) -> ApiProviderResult<Vec<FoldingRange>> {
        self.0.invoke("provideFoldingRanges", vec![self.0.document(document), JsData::Object(vec![]), self.0.token(token)], folding_ranges_from_js)
    }
}

struct SelectionRangeProvider(JsProvider);

impl ApiSelectionRangeProvider for SelectionRangeProvider {
    fn provide_selection_ranges(&self, document: &TextDocument, positions: &[Position], token: CancellationToken) -> ApiProviderResult<Vec<SelectionRange>> {
        let positions = JsData::Array(positions.iter().map(|position| position_data(*position)).collect());
        self.0.invoke("provideSelectionRanges", vec![self.0.document(document), positions, self.0.token(token)], selection_ranges_from_js)
    }
}

// --- tokens semânticos

/// Os provedores de tokens semânticos (documento inteiro e intervalo) têm o mesmo evento e o mesmo objeto; cada registro usa o trait dele.
struct SemanticTokensProvider(JsProvider);

impl ApiDocumentSemanticTokensProvider for SemanticTokensProvider {
    fn on_did_change_semantic_tokens(&self) -> Option<Event<()>> {
        self.0.change_event("onDidChangeSemanticTokens")
    }

    fn provide_document_semantic_tokens(&self, document: &TextDocument, token: CancellationToken) -> ApiProviderResult<SemanticTokens> {
        self.0.invoke("provideDocumentSemanticTokens", vec![self.0.document(document), self.0.token(token)], semantic_tokens_from_js)
    }

    fn has_provide_document_semantic_tokens_edits(&self) -> bool {
        self.0.has_method("provideDocumentSemanticTokensEdits")
    }

    fn provide_document_semantic_tokens_edits(&self, document: &TextDocument, previous_result_id: &str, token: CancellationToken) -> ApiProviderResult<ApiSemanticTokensResult> {
        let args = vec![self.0.document(document), JsData::Str(previous_result_id.to_owned()), self.0.token(token)];
        self.0.invoke("provideDocumentSemanticTokensEdits", args, semantic_tokens_result_from_js)
    }
}

impl ApiDocumentRangeSemanticTokensProvider for SemanticTokensProvider {
    fn on_did_change_semantic_tokens(&self) -> Option<Event<()>> {
        self.0.change_event("onDidChangeSemanticTokens")
    }

    fn provide_document_range_semantic_tokens(&self, document: &TextDocument, range: &Range, token: CancellationToken) -> ApiProviderResult<SemanticTokens> {
        self.0.invoke("provideDocumentRangeSemanticTokens", vec![self.0.document(document), range_data(*range), self.0.token(token)], semantic_tokens_from_js)
    }
}

// --- dicas embutidas

struct InlayHintsProvider {
    js: JsProvider,
    originals: Rc<Originals<InlayHint>>,
}

impl InlayHintsProvider {
    fn new(js: JsProvider) -> Self {
        Self { js, originals: Rc::default() }
    }
}

impl ApiInlayHintsProvider for InlayHintsProvider {
    fn on_did_change_inlay_hints(&self) -> Option<Event<()>> {
        self.js.change_event("onDidChangeInlayHints")
    }

    fn provide_inlay_hints(&self, document: &TextDocument, range: &Range, token: CancellationToken) -> ApiProviderResult<Vec<InlayHintRef>> {
        let originals = self.originals.clone();
        self.js.invoke("provideInlayHints", vec![self.js.document(document), range_data(*range), self.js.token(token)], move |value| {
            let hints = inlay_hints_from_js(value)?;
            if let (Some(hints), JsData::Array(raw)) = (&hints, value) {
                originals.remember(hints.iter().zip(raw));
            }
            Ok(hints)
        })
    }

    fn has_resolve_inlay_hint(&self) -> bool {
        self.js.has_method("resolveInlayHint")
    }

    fn resolve_inlay_hint(&self, hint: &InlayHintRef, token: CancellationToken) -> ApiProviderResult<InlayHintRef> {
        self.js.resolve("resolveInlayHint", &self.originals, hint, token, inlay_hint_from_js)
    }
}

// --- links, cores

struct DocumentLinkProvider {
    js: JsProvider,
    originals: Rc<Originals<DocumentLink>>,
}

impl ApiDocumentLinkProvider for DocumentLinkProvider {
    fn provide_document_links(&self, document: &TextDocument, token: CancellationToken) -> ApiProviderResult<Vec<DocumentLinkRef>> {
        let originals = self.originals.clone();
        self.js.invoke("provideDocumentLinks", vec![self.js.document(document), self.js.token(token)], move |value| {
            let links = document_links_from_js(value)?;
            if let (Some(links), JsData::Array(raw)) = (&links, value) {
                originals.remember(links.iter().zip(raw));
            }
            Ok(links)
        })
    }

    fn has_resolve_document_link(&self) -> bool {
        self.js.has_method("resolveDocumentLink")
    }

    fn resolve_document_link(&self, link: &DocumentLinkRef, token: CancellationToken) -> ApiProviderResult<DocumentLinkRef> {
        self.js.resolve("resolveDocumentLink", &self.originals, link, token, document_link_from_js)
    }
}

struct ColorProvider(JsProvider);

impl ApiDocumentColorProvider for ColorProvider {
    fn provide_document_colors(&self, document: &TextDocument, token: CancellationToken) -> ApiProviderResult<Vec<ColorInformation>> {
        self.0.invoke("provideDocumentColors", vec![self.0.document(document), self.0.token(token)], color_informations_from_js)
    }

    fn provide_color_presentations(&self, color: &Color, context: &ApiColorPresentationContext, token: CancellationToken) -> ApiProviderResult<Vec<ColorPresentation>> {
        let context = JsData::Object(vec![("document".to_owned(), self.0.document(&context.document)), ("range".to_owned(), range_data(context.range))]);
        self.0.invoke("provideColorPresentations", vec![api_to_js(color), context, self.0.token(token)], color_presentations_from_js)
    }
}

// --- hierarquias

struct CallHierarchyProvider(JsProvider);

impl ApiCallHierarchyProvider for CallHierarchyProvider {
    fn prepare_call_hierarchy(&self, document: &TextDocument, position: &Position, token: CancellationToken) -> ApiProviderResult<OneOrMany<CallHierarchyItem>> {
        self.0.invoke("prepareCallHierarchy", vec![self.0.document(document), position_data(*position), self.0.token(token)], call_hierarchy_items_from_js)
    }

    fn provide_call_hierarchy_incoming_calls(&self, item: &CallHierarchyItem, token: CancellationToken) -> ApiProviderResult<Vec<CallHierarchyIncomingCall>> {
        self.0.invoke("provideCallHierarchyIncomingCalls", vec![call_hierarchy_item_to_js(item), self.0.token(token)], call_hierarchy_incoming_calls_from_js)
    }

    fn provide_call_hierarchy_outgoing_calls(&self, item: &CallHierarchyItem, token: CancellationToken) -> ApiProviderResult<Vec<CallHierarchyOutgoingCall>> {
        self.0.invoke("provideCallHierarchyOutgoingCalls", vec![call_hierarchy_item_to_js(item), self.0.token(token)], call_hierarchy_outgoing_calls_from_js)
    }
}

struct TypeHierarchyProvider(JsProvider);

impl ApiTypeHierarchyProvider for TypeHierarchyProvider {
    fn prepare_type_hierarchy(&self, document: &TextDocument, position: &Position, token: CancellationToken) -> ApiProviderResult<OneOrMany<TypeHierarchyItem>> {
        self.0.invoke("prepareTypeHierarchy", vec![self.0.document(document), position_data(*position), self.0.token(token)], type_hierarchy_items_from_js)
    }

    fn provide_type_hierarchy_supertypes(&self, item: &TypeHierarchyItem, token: CancellationToken) -> ApiProviderResult<Vec<TypeHierarchyItem>> {
        self.0.invoke("provideTypeHierarchySupertypes", vec![type_hierarchy_item_to_js(item), self.0.token(token)], type_hierarchy_item_list_from_js)
    }

    fn provide_type_hierarchy_subtypes(&self, item: &TypeHierarchyItem, token: CancellationToken) -> ApiProviderResult<Vec<TypeHierarchyItem>> {
        self.0.invoke("provideTypeHierarchySubtypes", vec![type_hierarchy_item_to_js(item), self.0.token(token)], type_hierarchy_item_list_from_js)
    }
}

// --- símbolos do espaço de trabalho

struct WorkspaceSymbolProvider(JsProvider);

impl ApiWorkspaceSymbolProvider for WorkspaceSymbolProvider {
    fn provide_workspace_symbols(&self, query: &str, token: CancellationToken) -> ApiProviderResult<Vec<SymbolInformation>> {
        self.0.invoke("provideWorkspaceSymbols", vec![JsData::Str(query.to_owned()), self.0.token(token)], symbol_informations_from_js)
    }

    fn has_resolve_workspace_symbol(&self) -> bool {
        self.0.has_method("resolveWorkspaceSymbol")
    }

    fn resolve_workspace_symbol(&self, symbol: &SymbolInformation, token: CancellationToken) -> ApiProviderResult<SymbolInformation> {
        self.0.invoke("resolveWorkspaceSymbol", vec![symbol_information_to_js(symbol), self.0.token(token)], symbol_information_result_from_js)
    }
}

// --- os registros

/// `registerSignatureHelpProvider(selector, provider, firstItem?, ...remaining)`: um objeto é o `SignatureHelpProviderMetadata`, o resto são
/// os caracteres de disparo.
fn signature_help_metadata(registrar: &Registrar, args: &[JsData]) -> SignatureHelpMetadataOrTriggerChars {
    match args.get(2) {
        Some(first @ JsData::Handle(_)) => {
            let data = exported(&registrar.runner, first);
            SignatureHelpMetadataOrTriggerChars::Metadata {
                trigger_characters: strings_of(field(&data, "triggerCharacters")).unwrap_or_default(),
                retrigger_characters: strings_of(field(&data, "retriggerCharacters")).unwrap_or_default(),
            }
        }
        _ => SignatureHelpMetadataOrTriggerChars::TriggerCharacters(args.get(2..).unwrap_or(&[]).iter().filter_map(|character| opt_string(Some(character))).collect()),
    }
}

/// `register*SemanticTokensProvider(selector, provider, legend)`.
fn register_semantic_tokens(
    registrar: &Registrar,
    args: &[JsData],
    register: impl FnOnce(&ExtHostLanguageFeatures, &ExtensionDescription, &DocumentSelectorInput, JsProvider, SemanticTokensLegend) -> DisposableRef,
) -> Result<JsData, String> {
    let legend = semantic_tokens_legend_from_js(&exported(&registrar.runner, args.get(2).ok_or("a legenda (SemanticTokensLegend) é obrigatória")?))?;
    registrar.register(args, |features, extension, selector, provider, _| register(features, extension, selector, provider, legend))
}

/// As funções `register*Provider` desta leva, cada uma por referência.
pub(crate) fn members(runner: &NodeRunner, registrar: &Rc<Registrar>) -> Vec<(String, JsData)> {
    vec![
        simple(runner, registrar, "registerDocumentLinkProvider", |features, extension, selector, provider| {
            features.register_document_link_provider(extension, selector, Rc::new(DocumentLinkProvider { js: provider, originals: Rc::default() }))
        }),
        simple(runner, registrar, "registerColorProvider", |features, extension, selector, provider| {
            features.register_color_provider(extension, selector, Rc::new(ColorProvider(provider)))
        }),
        simple(runner, registrar, "registerCallHierarchyProvider", |features, extension, selector, provider| {
            features.register_call_hierarchy_provider(extension, selector, Rc::new(CallHierarchyProvider(provider)))
        }),
        simple(runner, registrar, "registerTypeHierarchyProvider", |features, extension, selector, provider| {
            features.register_type_hierarchy_provider(extension, selector, Rc::new(TypeHierarchyProvider(provider)))
        }),
        // `registerWorkspaceSymbolProvider(provider)`: sem seletor.
        member(runner, registrar, "registerWorkspaceSymbolProvider", 1, |registrar, args| {
            let Some(JsData::Handle(provider)) = args.first() else { return Err("o provedor precisa ser um objeto".to_owned()) };
            let registration = registrar.features.register_workspace_symbol_provider(&registrar.extension, Rc::new(WorkspaceSymbolProvider(registrar.js_provider(provider))));
            Ok(registrar.table.to_js(registration))
        }),
        simple(runner, registrar, "registerCodeLensProvider", |features, extension, selector, provider| {
            features.register_code_lens_provider(extension, selector, Rc::new(CodeLensProvider::new(provider)))
        }),
        simple(runner, registrar, "registerReferenceProvider", |features, extension, selector, provider| {
            features.register_reference_provider(extension, selector, Rc::new(ReferenceProvider(provider)))
        }),
        simple(runner, registrar, "registerRenameProvider", |features, extension, selector, provider| {
            features.register_rename_provider(extension, selector, Rc::new(RenameProvider(provider)))
        }),
        simple(runner, registrar, "registerDocumentHighlightProvider", |features, extension, selector, provider| {
            features.register_document_highlight_provider(extension, selector, Rc::new(DocumentHighlightProvider(provider)))
        }),
        simple(runner, registrar, "registerFoldingRangeProvider", |features, extension, selector, provider| {
            features.register_folding_range_provider(extension, selector, Rc::new(FoldingProvider(provider)))
        }),
        simple(runner, registrar, "registerSelectionRangeProvider", |features, extension, selector, provider| {
            features.register_selection_range_provider(extension, selector, Rc::new(SelectionRangeProvider(provider)))
        }),
        simple(runner, registrar, "registerInlayHintsProvider", |features, extension, selector, provider| {
            features.register_inlay_hints_provider(extension, selector, Rc::new(InlayHintsProvider::new(provider)))
        }),
        member(runner, registrar, "registerSignatureHelpProvider", 3, |registrar, args| {
            let metadata = signature_help_metadata(registrar, args);
            registrar.register(args, |features, extension, selector, provider, _| {
                features.register_signature_help_provider(extension, selector, Rc::new(SignatureHelpProvider(provider)), metadata)
            })
        }),
        member(runner, registrar, "registerOnTypeFormattingEditProvider", 3, |registrar, args| {
            registrar.register(args, |features, extension, selector, provider, rest| {
                let trigger_characters = rest.iter().filter_map(|character| opt_string(Some(character))).collect();
                features.register_on_type_formatting_edit_provider(extension, selector, Rc::new(OnTypeFormattingProvider(provider)), trigger_characters)
            })
        }),
        member(runner, registrar, "registerDocumentSemanticTokensProvider", 3, |registrar, args| {
            register_semantic_tokens(registrar, args, |features, extension, selector, provider, legend| {
                features.register_document_semantic_tokens_provider(extension, selector, Rc::new(SemanticTokensProvider(provider)), legend)
            })
        }),
        member(runner, registrar, "registerDocumentRangeSemanticTokensProvider", 3, |registrar, args| {
            register_semantic_tokens(registrar, args, |features, extension, selector, provider, legend| {
                features.register_document_range_semantic_tokens_provider(extension, selector, Rc::new(SemanticTokensProvider(provider)), legend)
            })
        }),
    ]
}
