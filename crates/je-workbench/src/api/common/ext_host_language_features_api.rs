//! A superfície da API `vscode` que `extHostLanguageFeatures.ts` consome: as interfaces de provedor
//! (`vscode.DocumentSymbolProvider`, `vscode.CodeLensProvider`, `vscode.DefinitionProvider`...) e os tipos soltos que
//! elas usam (`vscode.LocationLink`, `vscode.HoverContext`...).
//!
//! PARTE 1: os provedores de símbolos do documento, lentes de código, definição, declaração, implementação, tipo da
//! definição, hover, destaque de ocorrências (um e vários documentos) e referências.
//!
//! PARTE 2 (ao fim do arquivo): expressão avaliável, valores embutidos, edição ligada, ação de código, formatação (documento,
//! intervalo, ao digitar), símbolos do workspace, renomeação, novos nomes de símbolo, tokens semânticos (documento e
//! intervalo), completar, assinatura, dicas embutidas, links, cor, dobra, seleção, hierarquias de chamada e de tipo, e
//! soltar e colar. Os provedores de completação em linha entram na parte 3.
//!
//! Divergências da parte 2:
//! - O objeto compartilhado por referência entre o provedor, o cache e o `resolveX` (que o altera no lugar) é
//!   `Rc<RefCell<T>>`: [`ApiCodeActionRef`], [`CompletionItemRef`], [`InlayHintRef`], [`DocumentLinkRef`] e
//!   [`ApiSignatureHelpRef`]. A edição de soltar e de colar não é alterada por ninguém e é um `Rc` simples.
//! - `Thenable<T> | T` de uma propriedade do provedor (`supportsAutomaticTriggerKind`) é um [`MaybePromise`].
//! - `vscode.SemanticTokens | { resultId?; data: number[] }` (o tipo "relaxado" que o adaptador conserta) não existe: o
//!   [`SemanticTokens`] já tem `Vec<u32>`.
//! - `vscode.Command | vscode.CodeAction` é [`CommandOrCodeAction`]; o teste `_isCommand(...)` e o `instanceof CodeAction` do
//!   adaptador viram o `match` no enum.
//! - `vscode.DocumentDropEdit` e `vscode.DocumentPasteEdit` têm o mesmo formato para o adaptador e são o
//!   [`DocumentDropOrPasteEdit`]; `vscode.DocumentDropOrPasteEditKind` é o [`DocumentDropOrPasteEditKind`] (só o `value`, o
//!   único membro que o adaptador lê).
//! - `IExtHostApiDeprecationService` (`extHostApiDeprecationService.ts`, ainda sem porte) é o trait mínimo
//!   [`IExtHostApiDeprecationService`], só com o `report`.
//! - `vscode.FormattingOptions` é o [`FormattingOptions`] do protocolo (`tabSize` e `insertSpaces`).
//! - `vscode.FoldingContext` (vazio) é o [`FoldingContext`] do editor.
//!
//! Divergências:
//! - `ProviderResult<T>` (`T | undefined | null | Thenable<T | undefined | null>`) é [`ApiProviderResult`], um
//!   `MaybePromise<Option<T>>`: o valor síncrono é `MaybePromise::Value`, a promessa é `MaybePromise::Promise`, e `undefined`
//!   e `null` são `None`. Um `throw` síncrono do provedor é uma promessa rejeitada.
//! - Um provedor da API é um trait, e o método opcional do TypeScript (`resolveCodeLens?`, `onDidChangeCodeLenses?`) é um
//!   método com implementação padrão, mais um `has_*` onde o original testa `typeof provider.x === 'function'`.
//! - `vscode.DefinitionProvider`, `DeclarationProvider`, `ImplementationProvider` e `TypeDefinitionProvider` têm a mesma
//!   forma (`provideX(document, position, token)` devolvendo `Definition | DefinitionLink[]`) e são o único trait
//!   [`ApiLocationLinkProvider`]; o adaptador é que sabe de qual das quatro é cada registro.
//! - `vscode.Location | vscode.Location[] | vscode.LocationLink[]` é [`ApiLocationResult`]; o objeto que o original distingue
//!   por duck typing (`targetUri`) é o [`LocationOrLink`].
//! - `vscode.Hover` e `vscode.VerboseHover` são o [`VerboseHover`] (`Hover` mais os dois sinalizadores de verbosidade
//!   opcionais): o hover simples é um [`VerboseHover`] sem sinalizadores.
//! - O `vscode.CodeLens` é compartilhado por referência entre o provedor, o cache e o `resolveCodeLens` (que o altera no
//!   lugar): é o [`CodeLensRef`], `Rc<RefCell<CodeLens>>`.
//! - `vscode.TextDocument` é o [`TextDocument`] do host de documentos.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::event::Event;
use je_base::js_regexp::JsRegExp;
use je_base::promise::MaybePromise;
use je_base::uri::URI;
use je_editor::folding::syntax_range_provider::FoldingContext;
use je_editor::language_configuration::{CharacterPair, CommentRule, IndentationRule, OnEnterRule};
use je_extensions::scanner::ExtensionDescription;

use super::ext_host_document_data::TextDocument;
use super::ext_host_protocol::{CharacterPairSupportDto, ElectricCharacterSupportDto, FormattingOptions, ReferenceContext};
use super::ext_host_types::{
    SyntaxTokenType,
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, CodeAction, CodeActionKind, CodeActionTriggerKind, CodeLens, Color,
    ColorInformation, ColorPresentation, Command, CompletionContext, CompletionItem, Diagnostic, DocumentHighlight, DocumentLink,
    DocumentPasteTriggerKind, DocumentSymbol, EvaluatableExpression, FoldingRange, InlayHint, InlineValueContext, InlineValueEvaluatableExpression,
    InlineValueText, InlineValueVariableLookup, LinkedEditingRanges, Location, MultiDocumentHighlight, NewSymbolName, NewSymbolNameTriggerKind,
    Position, Range, SelectionRange, Selection, SemanticTokens, SemanticTokensEdits, SignatureHelp, SignatureHelpTriggerKind, SnippetString,
    SymbolInformation, TextEdit, TypeHierarchyItem, VerboseHover, WorkspaceEdit, DataTransfer,
};

/// `ProviderResult<T>` da API `vscode` (ver a nota do módulo).
pub type ApiProviderResult<T> = MaybePromise<Option<T>>;

/// `vscode.DocumentSymbolProviderMetadata`.
#[derive(Clone, Debug, Default)]
pub struct DocumentSymbolProviderMetadata {
    pub label: Option<String>,
}

/// `DocumentSymbol[] | SymbolInformation[]`: o original distingue pelo `instanceof` do primeiro elemento.
#[derive(Clone, Debug)]
pub enum ApiDocumentSymbols {
    Symbols(Vec<DocumentSymbol>),
    Information(Vec<SymbolInformation>),
}

/// `vscode.DocumentSymbolProvider`.
pub trait ApiDocumentSymbolProvider {
    fn provide_document_symbols(&self, document: &TextDocument, token: CancellationToken) -> ApiProviderResult<ApiDocumentSymbols>;
}

/// O `vscode.CodeLens` compartilhado por referência.
pub type CodeLensRef = Rc<RefCell<CodeLens>>;

/// `vscode.CodeLensProvider`.
pub trait ApiCodeLensProvider {
    /// `onDidChangeCodeLenses`, quando o provedor tem o evento.
    fn on_did_change_code_lenses(&self) -> Option<Event<()>> {
        None
    }

    fn provide_code_lenses(&self, document: &TextDocument, token: CancellationToken) -> ApiProviderResult<Vec<CodeLensRef>>;

    /// `typeof provider.resolveCodeLens === 'function'`.
    fn has_resolve_code_lens(&self) -> bool {
        false
    }

    /// `resolveCodeLens(codeLens, token)`; só é chamado quando [`ApiCodeLensProvider::has_resolve_code_lens`] é verdadeiro.
    fn resolve_code_lens(&self, _code_lens: &CodeLensRef, _token: CancellationToken) -> ApiProviderResult<CodeLensRef> {
        MaybePromise::Value(None)
    }
}

/// `vscode.LocationLink`.
#[derive(Clone, Debug, PartialEq)]
pub struct DefinitionLink {
    pub origin_selection_range: Option<Range>,
    pub target_uri: URI,
    pub target_range: Range,
    pub target_selection_range: Option<Range>,
}

/// `vscode.Location | vscode.LocationLink`.
#[derive(Clone, Debug)]
pub enum LocationOrLink {
    Location(Location),
    Link(DefinitionLink),
}

/// `vscode.Location | vscode.Location[] | vscode.LocationLink[]`.
#[derive(Clone, Debug)]
pub enum ApiLocationResult {
    Single(LocationOrLink),
    Many(Vec<LocationOrLink>),
}

/// `vscode.DefinitionProvider`, `vscode.DeclarationProvider`, `vscode.ImplementationProvider` e
/// `vscode.TypeDefinitionProvider` (ver a nota do módulo).
pub trait ApiLocationLinkProvider {
    fn provide_location_links(&self, document: &TextDocument, position: &Position, token: CancellationToken) -> ApiProviderResult<ApiLocationResult>;
}

/// `vscode.HoverContext`.
#[derive(Clone, Debug)]
pub struct ApiHoverContext {
    pub verbosity_delta: f64,
    pub previous_hover: VerboseHover,
}

/// `vscode.HoverProvider`.
pub trait ApiHoverProvider {
    /// `provideHover(document, position, token, context?)`.
    fn provide_hover(
        &self,
        document: &TextDocument,
        position: &Position,
        token: CancellationToken,
        context: Option<&ApiHoverContext>,
    ) -> ApiProviderResult<VerboseHover>;
}

/// `vscode.DocumentHighlightProvider`.
pub trait ApiDocumentHighlightProvider {
    fn provide_document_highlights(&self, document: &TextDocument, position: &Position, token: CancellationToken) -> ApiProviderResult<Vec<DocumentHighlight>>;
}

/// `vscode.MultiDocumentHighlightProvider`.
pub trait ApiMultiDocumentHighlightProvider {
    fn provide_multi_document_highlights(
        &self,
        document: &TextDocument,
        position: &Position,
        other_documents: &[TextDocument],
        token: CancellationToken,
    ) -> ApiProviderResult<Vec<MultiDocumentHighlight>>;
}

/// `vscode.ReferenceProvider`.
pub trait ApiReferenceProvider {
    fn provide_references(
        &self,
        document: &TextDocument,
        position: &Position,
        context: &ReferenceContext,
        token: CancellationToken,
    ) -> ApiProviderResult<Vec<Location>>;
}

// ---- parte 2

/// `T | T[]`: `prepareCallHierarchy` e `provideDocumentDropEdits` aceitam um valor ou uma lista.
#[derive(Clone, Debug)]
pub enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

/// `IExtHostApiDeprecationService` (ver a nota do módulo).
pub trait IExtHostApiDeprecationService {
    /// `report(apiId, extension, migrationSuggestion)`.
    fn report(&self, api_id: &str, extension: &ExtensionDescription, migration_suggestion: &str);
}

/// `vscode.EvaluatableExpressionProvider`.
pub trait ApiEvaluatableExpressionProvider {
    fn provide_evaluatable_expression(&self, document: &TextDocument, position: &Position, token: CancellationToken) -> ApiProviderResult<EvaluatableExpression>;
}

/// `vscode.InlineValue`: `InlineValueText | InlineValueVariableLookup | InlineValueEvaluatableExpression`.
#[derive(Clone, Debug)]
pub enum ApiInlineValue {
    Text(InlineValueText),
    VariableLookup(InlineValueVariableLookup),
    EvaluatableExpression(InlineValueEvaluatableExpression),
}

/// `vscode.InlineValuesProvider`.
pub trait ApiInlineValuesProvider {
    /// `onDidChangeInlineValues`, quando o provedor tem o evento.
    fn on_did_change_inline_values(&self) -> Option<Event<()>> {
        None
    }

    fn provide_inline_values(&self, document: &TextDocument, view_port: &Range, context: &InlineValueContext, token: CancellationToken)
    -> ApiProviderResult<Vec<ApiInlineValue>>;
}

/// `vscode.LinkedEditingRangeProvider`.
pub trait ApiLinkedEditingRangeProvider {
    fn provide_linked_editing_ranges(&self, document: &TextDocument, position: &Position, token: CancellationToken) -> ApiProviderResult<LinkedEditingRanges>;
}

/// `vscode.Command | vscode.CodeAction` (ver a nota do módulo).
#[derive(Clone)]
pub enum CommandOrCodeAction {
    Command(Command),
    CodeAction(ApiCodeActionRef),
}

/// O `vscode.CodeAction` compartilhado por referência.
pub type ApiCodeActionRef = Rc<RefCell<CodeAction>>;

/// `vscode.Range | vscode.Selection`.
#[derive(Clone, Debug)]
pub enum ApiRangeOrSelection {
    Range(Range),
    Selection(Selection),
}

impl ApiRangeOrSelection {
    /// O intervalo (a `Selection` herda de `Range`).
    pub fn range(&self) -> &Range {
        match self {
            ApiRangeOrSelection::Range(range) => range,
            ApiRangeOrSelection::Selection(selection) => selection,
        }
    }
}

/// `vscode.CodeActionContext`.
#[derive(Clone, Debug)]
pub struct ApiCodeActionContext {
    pub diagnostics: Vec<Diagnostic>,
    pub only: Option<CodeActionKind>,
    pub trigger_kind: CodeActionTriggerKind,
}

/// O elemento de `CodeActionProviderMetadata.documentation`.
#[derive(Clone, Debug)]
pub struct CodeActionDocumentation {
    pub kind: CodeActionKind,
    pub command: Command,
}

/// `vscode.CodeActionProviderMetadata`.
#[derive(Clone, Debug, Default)]
pub struct CodeActionProviderMetadata {
    pub provided_code_action_kinds: Option<Vec<CodeActionKind>>,
    pub documentation: Option<Vec<CodeActionDocumentation>>,
}

/// `vscode.CodeActionProvider`.
pub trait ApiCodeActionProvider {
    fn provide_code_actions(
        &self,
        document: &TextDocument,
        range_or_selection: &ApiRangeOrSelection,
        context: &ApiCodeActionContext,
        token: CancellationToken,
    ) -> ApiProviderResult<Vec<CommandOrCodeAction>>;

    /// `Boolean(provider.resolveCodeAction)`.
    fn has_resolve_code_action(&self) -> bool {
        false
    }

    /// `resolveCodeAction(codeAction, token)`; só é chamado quando [`ApiCodeActionProvider::has_resolve_code_action`] é verdadeiro.
    fn resolve_code_action(&self, _code_action: &ApiCodeActionRef, _token: CancellationToken) -> ApiProviderResult<ApiCodeActionRef> {
        MaybePromise::Value(None)
    }
}

/// `vscode.DocumentFormattingEditProvider`.
pub trait ApiDocumentFormattingEditProvider {
    fn provide_document_formatting_edits(&self, document: &TextDocument, options: &FormattingOptions, token: CancellationToken) -> ApiProviderResult<Vec<TextEdit>>;
}

/// `vscode.DocumentRangeFormattingEditProvider`.
pub trait ApiDocumentRangeFormattingEditProvider {
    fn provide_document_range_formatting_edits(
        &self,
        document: &TextDocument,
        range: &Range,
        options: &FormattingOptions,
        token: CancellationToken,
    ) -> ApiProviderResult<Vec<TextEdit>>;

    /// `typeof provider.provideDocumentRangesFormattingEdits === 'function'`.
    fn has_provide_document_ranges_formatting_edits(&self) -> bool {
        false
    }

    /// `provideDocumentRangesFormattingEdits`; só é chamado quando o `has_*` correspondente é verdadeiro.
    fn provide_document_ranges_formatting_edits(
        &self,
        _document: &TextDocument,
        _ranges: &[Range],
        _options: &FormattingOptions,
        _token: CancellationToken,
    ) -> ApiProviderResult<Vec<TextEdit>> {
        MaybePromise::Value(None)
    }
}

/// `vscode.OnTypeFormattingEditProvider`.
pub trait ApiOnTypeFormattingEditProvider {
    fn provide_on_type_formatting_edits(
        &self,
        document: &TextDocument,
        position: &Position,
        ch: &str,
        options: &FormattingOptions,
        token: CancellationToken,
    ) -> ApiProviderResult<Vec<TextEdit>>;
}

/// `vscode.WorkspaceSymbolProvider`.
pub trait ApiWorkspaceSymbolProvider {
    fn provide_workspace_symbols(&self, query: &str, token: CancellationToken) -> ApiProviderResult<Vec<SymbolInformation>>;

    /// `typeof provider.resolveWorkspaceSymbol === 'function'`.
    fn has_resolve_workspace_symbol(&self) -> bool {
        false
    }

    /// `resolveWorkspaceSymbol(symbol, token)`; só é chamado quando o `has_*` correspondente é verdadeiro.
    fn resolve_workspace_symbol(&self, _symbol: &SymbolInformation, _token: CancellationToken) -> ApiProviderResult<SymbolInformation> {
        MaybePromise::Value(None)
    }
}

/// `Range | { range: Range; placeholder: string }`: o resultado de `prepareRename`.
#[derive(Clone, Debug)]
pub enum ApiPrepareRename {
    Range(Range),
    RangeWithPlaceholder { range: Range, placeholder: Option<String> },
}

/// `vscode.RenameProvider`.
pub trait ApiRenameProvider {
    fn provide_rename_edits(&self, document: &TextDocument, position: &Position, new_name: &str, token: CancellationToken) -> ApiProviderResult<WorkspaceEdit>;

    /// `typeof provider.prepareRename === 'function'`.
    fn has_prepare_rename(&self) -> bool {
        false
    }

    /// `prepareRename(document, position, token)`; só é chamado quando o `has_*` correspondente é verdadeiro.
    fn prepare_rename(&self, _document: &TextDocument, _position: &Position, _token: CancellationToken) -> ApiProviderResult<ApiPrepareRename> {
        MaybePromise::Value(None)
    }
}

/// `string | NewSymbolName`: o elemento do resultado de `provideNewSymbolNames`.
#[derive(Clone, Debug)]
pub enum ApiNewSymbolName {
    Text(String),
    Name(NewSymbolName),
}

/// `vscode.NewSymbolNamesProvider`.
pub trait ApiNewSymbolNamesProvider {
    /// `supportsAutomaticTriggerKind`: `Thenable<boolean | undefined>`.
    fn supports_automatic_trigger_kind(&self) -> MaybePromise<Option<bool>> {
        MaybePromise::Value(None)
    }

    fn provide_new_symbol_names(
        &self,
        document: &TextDocument,
        range: &Range,
        trigger_kind: NewSymbolNameTriggerKind,
        token: CancellationToken,
    ) -> ApiProviderResult<Vec<ApiNewSymbolName>>;
}

/// `vscode.SemanticTokens | vscode.SemanticTokensEdits`: o resultado de `provideDocumentSemanticTokensEdits`.
#[derive(Clone, Debug)]
pub enum ApiSemanticTokensResult {
    Tokens(SemanticTokens),
    Edits(SemanticTokensEdits),
}

/// `vscode.DocumentSemanticTokensProvider`.
pub trait ApiDocumentSemanticTokensProvider {
    /// `onDidChangeSemanticTokens`, quando o provedor tem o evento.
    fn on_did_change_semantic_tokens(&self) -> Option<Event<()>> {
        None
    }

    fn provide_document_semantic_tokens(&self, document: &TextDocument, token: CancellationToken) -> ApiProviderResult<SemanticTokens>;

    /// `typeof provider.provideDocumentSemanticTokensEdits === 'function'`.
    fn has_provide_document_semantic_tokens_edits(&self) -> bool {
        false
    }

    /// `provideDocumentSemanticTokensEdits(document, previousResultId, token)`; só é chamado quando o `has_*` correspondente é verdadeiro.
    fn provide_document_semantic_tokens_edits(
        &self,
        _document: &TextDocument,
        _previous_result_id: &str,
        _token: CancellationToken,
    ) -> ApiProviderResult<ApiSemanticTokensResult> {
        MaybePromise::Value(None)
    }
}

/// `vscode.DocumentRangeSemanticTokensProvider`.
pub trait ApiDocumentRangeSemanticTokensProvider {
    /// `onDidChangeSemanticTokens`, quando o provedor tem o evento.
    fn on_did_change_semantic_tokens(&self) -> Option<Event<()>> {
        None
    }

    fn provide_document_range_semantic_tokens(&self, document: &TextDocument, range: &Range, token: CancellationToken) -> ApiProviderResult<SemanticTokens>;
}

/// O `vscode.CompletionItem` compartilhado por referência.
pub type CompletionItemRef = Rc<RefCell<CompletionItem>>;

/// `vscode.CompletionList`, com os itens compartilhados por referência.
#[derive(Clone)]
pub struct ApiCompletionList {
    pub is_incomplete: Option<bool>,
    pub items: Vec<CompletionItemRef>,
}

/// `CompletionItem[] | CompletionList`.
#[derive(Clone)]
pub enum ApiCompletionResult {
    Items(Vec<CompletionItemRef>),
    List(ApiCompletionList),
}

/// `vscode.CompletionItemProvider`.
pub trait ApiCompletionItemProvider {
    /// `provideCompletionItems(document, position, token, context)`.
    fn provide_completion_items(
        &self,
        document: &TextDocument,
        position: &Position,
        token: CancellationToken,
        context: &CompletionContext,
    ) -> ApiProviderResult<ApiCompletionResult>;

    /// `typeof provider.resolveCompletionItem === 'function'`.
    fn has_resolve_completion_item(&self) -> bool {
        false
    }

    /// `resolveCompletionItem(item, token)`; só é chamado quando o `has_*` correspondente é verdadeiro.
    fn resolve_completion_item(&self, _item: &CompletionItemRef, _token: CancellationToken) -> ApiProviderResult<CompletionItemRef> {
        MaybePromise::Value(None)
    }
}

/// O `vscode.SignatureHelp` compartilhado por referência (o adaptador altera `activeSignature` e `activeParameter`).
pub type ApiSignatureHelpRef = Rc<RefCell<SignatureHelp>>;

/// `vscode.SignatureHelpContext`.
#[derive(Clone)]
pub struct ApiSignatureHelpContext {
    pub trigger_kind: SignatureHelpTriggerKind,
    pub trigger_character: Option<String>,
    pub is_retrigger: bool,
    pub active_signature_help: Option<ApiSignatureHelpRef>,
}

/// `vscode.SignatureHelpProvider`.
pub trait ApiSignatureHelpProvider {
    fn provide_signature_help(
        &self,
        document: &TextDocument,
        position: &Position,
        token: CancellationToken,
        context: &ApiSignatureHelpContext,
    ) -> ApiProviderResult<ApiSignatureHelpRef>;
}

/// `string[] | vscode.SignatureHelpProviderMetadata`: o último argumento de `registerSignatureHelpProvider`.
#[derive(Clone, Debug)]
pub enum SignatureHelpMetadataOrTriggerChars {
    TriggerCharacters(Vec<String>),
    Metadata { trigger_characters: Vec<String>, retrigger_characters: Vec<String> },
}

/// O `vscode.InlayHint` compartilhado por referência.
pub type InlayHintRef = Rc<RefCell<InlayHint>>;

/// `vscode.InlayHintsProvider`.
pub trait ApiInlayHintsProvider {
    /// `onDidChangeInlayHints`, quando o provedor tem o evento.
    fn on_did_change_inlay_hints(&self) -> Option<Event<()>> {
        None
    }

    fn provide_inlay_hints(&self, document: &TextDocument, range: &Range, token: CancellationToken) -> ApiProviderResult<Vec<InlayHintRef>>;

    /// `typeof provider.resolveInlayHint === 'function'`.
    fn has_resolve_inlay_hint(&self) -> bool {
        false
    }

    /// `resolveInlayHint(hint, token)`; só é chamado quando o `has_*` correspondente é verdadeiro.
    fn resolve_inlay_hint(&self, _hint: &InlayHintRef, _token: CancellationToken) -> ApiProviderResult<InlayHintRef> {
        MaybePromise::Value(None)
    }
}

/// O `vscode.DocumentLink` compartilhado por referência.
pub type DocumentLinkRef = Rc<RefCell<DocumentLink>>;

/// `vscode.DocumentLinkProvider`.
pub trait ApiDocumentLinkProvider {
    fn provide_document_links(&self, document: &TextDocument, token: CancellationToken) -> ApiProviderResult<Vec<DocumentLinkRef>>;

    /// `typeof provider.resolveDocumentLink === 'function'`.
    fn has_resolve_document_link(&self) -> bool {
        false
    }

    /// `resolveDocumentLink(link, token)`; só é chamado quando o `has_*` correspondente é verdadeiro.
    fn resolve_document_link(&self, _link: &DocumentLinkRef, _token: CancellationToken) -> ApiProviderResult<DocumentLinkRef> {
        MaybePromise::Value(None)
    }
}

/// O `{ document, range }` de `provideColorPresentations`.
#[derive(Clone)]
pub struct ApiColorPresentationContext {
    pub document: TextDocument,
    pub range: Range,
}

/// `vscode.DocumentColorProvider`.
pub trait ApiDocumentColorProvider {
    fn provide_document_colors(&self, document: &TextDocument, token: CancellationToken) -> ApiProviderResult<Vec<ColorInformation>>;

    fn provide_color_presentations(&self, color: &Color, context: &ApiColorPresentationContext, token: CancellationToken) -> ApiProviderResult<Vec<ColorPresentation>>;
}

/// `vscode.FoldingRangeProvider`.
pub trait ApiFoldingRangeProvider {
    /// `onDidChangeFoldingRanges`, quando o provedor tem o evento.
    fn on_did_change_folding_ranges(&self) -> Option<Event<()>> {
        None
    }

    fn provide_folding_ranges(&self, document: &TextDocument, context: &FoldingContext, token: CancellationToken) -> ApiProviderResult<Vec<FoldingRange>>;
}

/// `vscode.SelectionRangeProvider`.
pub trait ApiSelectionRangeProvider {
    fn provide_selection_ranges(&self, document: &TextDocument, positions: &[Position], token: CancellationToken) -> ApiProviderResult<Vec<SelectionRange>>;
}

/// `vscode.CallHierarchyProvider`.
pub trait ApiCallHierarchyProvider {
    fn prepare_call_hierarchy(&self, document: &TextDocument, position: &Position, token: CancellationToken) -> ApiProviderResult<OneOrMany<CallHierarchyItem>>;

    fn provide_call_hierarchy_incoming_calls(&self, item: &CallHierarchyItem, token: CancellationToken) -> ApiProviderResult<Vec<CallHierarchyIncomingCall>>;

    fn provide_call_hierarchy_outgoing_calls(&self, item: &CallHierarchyItem, token: CancellationToken) -> ApiProviderResult<Vec<CallHierarchyOutgoingCall>>;
}

/// `vscode.TypeHierarchyProvider`.
pub trait ApiTypeHierarchyProvider {
    fn prepare_type_hierarchy(&self, document: &TextDocument, position: &Position, token: CancellationToken) -> ApiProviderResult<OneOrMany<TypeHierarchyItem>>;

    fn provide_type_hierarchy_supertypes(&self, item: &TypeHierarchyItem, token: CancellationToken) -> ApiProviderResult<Vec<TypeHierarchyItem>>;

    fn provide_type_hierarchy_subtypes(&self, item: &TypeHierarchyItem, token: CancellationToken) -> ApiProviderResult<Vec<TypeHierarchyItem>>;
}

/// `vscode.DocumentDropOrPasteEditKind`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentDropOrPasteEditKind {
    pub value: String,
}

impl DocumentDropOrPasteEditKind {
    pub fn new(value: impl Into<String>) -> Self {
        Self { value: value.into() }
    }
}

/// `string | vscode.SnippetString`: o `insertText` de uma edição de soltar ou de colar.
#[derive(Clone)]
pub enum DocumentEditInsertText {
    Text(String),
    Snippet(SnippetString),
}

/// `vscode.DocumentDropEdit` e `vscode.DocumentPasteEdit` (ver a nota do módulo).
#[derive(Clone)]
pub struct DocumentDropOrPasteEdit {
    pub title: Option<String>,
    pub kind: Option<DocumentDropOrPasteEditKind>,
    pub yield_to: Option<Vec<DocumentDropOrPasteEditKind>>,
    pub insert_text: DocumentEditInsertText,
    pub additional_edit: Option<WorkspaceEdit>,
}

/// A edição de soltar ou de colar compartilhada pelo provedor e pelo cache.
pub type DocumentEditRef = Rc<DocumentDropOrPasteEdit>;

/// `vscode.DocumentDropEditProvider`.
pub trait ApiDocumentDropEditProvider {
    fn provide_document_drop_edits(
        &self,
        document: &TextDocument,
        position: &Position,
        data_transfer: &Rc<DataTransfer>,
        token: CancellationToken,
    ) -> ApiProviderResult<OneOrMany<DocumentEditRef>>;

    /// `Boolean(provider.resolveDocumentDropEdit)`.
    fn has_resolve_document_drop_edit(&self) -> bool {
        false
    }

    /// `resolveDocumentDropEdit(edit, token)`; só é chamado quando o `has_*` correspondente é verdadeiro.
    fn resolve_document_drop_edit(&self, _edit: &DocumentEditRef, _token: CancellationToken) -> ApiProviderResult<DocumentEditRef> {
        MaybePromise::Value(None)
    }
}

/// O `{ only, triggerKind }` de `provideDocumentPasteEdits`.
#[derive(Clone, Debug)]
pub struct ApiDocumentPasteContext {
    pub only: Option<DocumentDropOrPasteEditKind>,
    pub trigger_kind: DocumentPasteTriggerKind,
}

/// `vscode.DocumentPasteProviderMetadata`.
#[derive(Clone, Debug, Default)]
pub struct DocumentPasteProviderMetadata {
    pub provided_paste_edit_kinds: Option<Vec<DocumentDropOrPasteEditKind>>,
    pub copy_mime_types: Option<Vec<String>>,
    pub paste_mime_types: Option<Vec<String>>,
}

/// `vscode.DocumentDropEditProviderMetadata`.
#[derive(Clone, Debug, Default)]
pub struct DocumentDropEditProviderMetadata {
    pub drop_mime_types: Vec<String>,
    pub provided_drop_edit_kinds: Option<Vec<DocumentDropOrPasteEditKind>>,
}

/// `vscode.DocumentPasteEditProvider`.
pub trait ApiDocumentPasteEditProvider {
    /// `Boolean(provider.prepareDocumentPaste)`.
    fn has_prepare_document_paste(&self) -> bool {
        false
    }

    /// `prepareDocumentPaste(document, ranges, dataTransfer, token)`; só é chamado quando o `has_*` correspondente é verdadeiro.
    fn prepare_document_paste(
        &self,
        _document: &TextDocument,
        _ranges: &[Range],
        _data_transfer: &Rc<DataTransfer>,
        _token: CancellationToken,
    ) -> MaybePromise<()> {
        MaybePromise::Value(())
    }

    /// `Boolean(provider.provideDocumentPasteEdits)`.
    fn has_provide_document_paste_edits(&self) -> bool {
        false
    }

    /// `provideDocumentPasteEdits(document, ranges, dataTransfer, context, token)`; só é chamado quando o `has_*` correspondente é verdadeiro.
    fn provide_document_paste_edits(
        &self,
        _document: &TextDocument,
        _ranges: &[Range],
        _data_transfer: &Rc<DataTransfer>,
        _context: &ApiDocumentPasteContext,
        _token: CancellationToken,
    ) -> ApiProviderResult<Vec<DocumentEditRef>> {
        MaybePromise::Value(None)
    }

    /// `Boolean(provider.resolveDocumentPasteEdit)`.
    fn has_resolve_document_paste_edit(&self) -> bool {
        false
    }

    /// `resolveDocumentPasteEdit(edit, token)`; só é chamado quando o `has_*` correspondente é verdadeiro.
    fn resolve_document_paste_edit(&self, _edit: &DocumentEditRef, _token: CancellationToken) -> ApiProviderResult<DocumentEditRef> {
        MaybePromise::Value(None)
    }
}


// ---- parte 3: configuração de linguagem

/// `vscode.AutoClosingPair`.
#[derive(Clone, Debug)]
pub struct ApiAutoClosingPair {
    pub open: String,
    pub close: String,
    pub not_in: Option<Vec<SyntaxTokenType>>,
    /// Exige a API proposta `autoClosingPairBeforeText`.
    pub before_text: Option<Rc<JsRegExp>>,
}

/// `vscode.LanguageConfiguration`. O `CommentRule`, o `IndentationRule` e o `OnEnterRule` da API têm a forma dos do editor e são
/// os do editor; o `__electricCharacterSupport` e o `__characterPairSupport` têm a forma dos DTOs e são os do protocolo.
#[derive(Clone, Default)]
pub struct ApiLanguageConfiguration {
    pub comments: Option<CommentRule>,
    pub brackets: Option<Vec<CharacterPair>>,
    pub word_pattern: Option<Rc<JsRegExp>>,
    pub indentation_rules: Option<IndentationRule>,
    pub on_enter_rules: Option<Vec<OnEnterRule>>,
    pub electric_character_support: Option<ElectricCharacterSupportDto>,
    pub character_pair_support: Option<CharacterPairSupportDto>,
    pub auto_closing_pairs: Option<Vec<ApiAutoClosingPair>>,
}
