//! Os registros de provedor de `vscode.languages` da segunda leva: `registerCodeLensProvider` (com `resolveCodeLens` e
//! `onDidChangeCodeLenses`), `registerReferenceProvider`, `registerRenameProvider` (com `prepareRename`),
//! `registerSignatureHelpProvider` (caracteres soltos ou `{ triggerCharacters, retriggerCharacters }`), `registerDocumentHighlightProvider`,
//! `registerOnTypeFormattingEditProvider`, `registerFoldingRangeProvider` (com `onDidChangeFoldingRanges`),
//! `registerSelectionRangeProvider`, `registerDocumentSemanticTokensProvider` (com `provideDocumentSemanticTokensEdits` e
//! `onDidChangeSemanticTokens`), `registerDocumentRangeSemanticTokensProvider` e `registerInlayHintsProvider` (com `resolveInlayHint` e
//! `onDidChangeInlayHints`).
//!
//! Ligam o provedor JavaScript (por referência, com o `this` da classe) ao `ExtHostLanguageFeatures` portado, no mesmo molde dos de
//! `languages`: a infraestrutura (`JsProvider`, `Registrar`, `member`, `simple`, `Originals`) é a de lá, as classes de valor e os leitores
//! estão em `language_provider_types`. Os `has_*` do trait são `typeof provider.resolveX === 'function'` lido no objeto do provedor.
//!
//! Falta (o serviço do host tem, falta só a ponte): `registerDocumentLinkProvider`, `registerColorProvider`,
//! `registerCallHierarchyProvider`, `registerTypeHierarchyProvider`, `registerWorkspaceSymbolProvider`,
//! `registerEvaluatableExpressionProvider`, `registerInlineValuesProvider`, `registerLinkedEditingRangeProvider`,
//! `registerMultiDocumentHighlightProvider`, `registerDocumentPasteEditProvider`, `registerDocumentDropEditProvider` e as propostas.
//!
//! Divergências:
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
    ApiCodeLensProvider, ApiDocumentHighlightProvider, ApiDocumentRangeSemanticTokensProvider, ApiDocumentSemanticTokensProvider, ApiFoldingRangeProvider, ApiInlayHintsProvider,
    ApiOnTypeFormattingEditProvider, ApiPrepareRename, ApiProviderResult, ApiReferenceProvider, ApiRenameProvider, ApiSelectionRangeProvider, ApiSemanticTokensResult,
    ApiSignatureHelpContext, ApiSignatureHelpProvider, ApiSignatureHelpRef, CodeLensRef, InlayHintRef, SignatureHelpMetadataOrTriggerChars,
};
use je_workbench::api::common::ext_host_protocol::{FormattingOptions, ReferenceContext};
use je_workbench::api::common::ext_host_type_converters::DocumentSelectorInput;
use je_workbench::api::common::ext_host_types::{
    CodeLens, DocumentHighlight, FoldingRange, InlayHint, Location, Position, Range, SelectionRange, SemanticTokens, SemanticTokensLegend, TextEdit, WorkspaceEdit,
};
use je_extensions::scanner::ExtensionDescription;
use je_base::lifecycle::DisposableRef;
use zjsc::api::embed_node::{JsData, NodeRunner};

use crate::api::language_provider_types::{
    code_lens_from_js, code_lenses_from_js, document_highlights_from_js, folding_ranges_from_js, inlay_hint_from_js, inlay_hints_from_js, locations_from_js,
    prepare_rename_from_js, selection_ranges_from_js, semantic_tokens_from_js, semantic_tokens_legend_from_js, semantic_tokens_result_from_js, signature_help_from_js,
    signature_help_to_js,
};
use crate::api::language_types::{strings_of, text_edits_from_js};
use crate::api::languages::{formatting_options_to_js, member, simple, JsProvider, Originals, Registrar};
use crate::api::types::{position_data, range_data};
use crate::convert::{exported, field, is_nullish, opt_string};

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
