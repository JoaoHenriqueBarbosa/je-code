//! Porte de `src/vs/editor/common/services/languageFeatures.ts` e `languageFeaturesService.ts`: o
//! `ILanguageFeaturesService`, o agregado de registros de provedores (`LanguageFeatureRegistry<T>`) de cada recurso de
//! linguagem, mais o recorte de `src/vs/editor/common/languages.ts` que o `MainThreadLanguageFeatures` registra e que o
//! editor ainda não tinha (localização, hover, lente de código, formatação, renomeação, tokens semânticos, dicas
//! embutidas, links, cores, valores embutidos, edição ligada, colar e soltar).
//!
//! Pendentes: o registro `inlineEditProvider` (as edições em linha ainda não têm porte); o `inlineCompletionsProvider`
//! (`languages::InlineCompletionsProvider`) já está registrado. As hierarquias de chamada e de tipo e os símbolos do workspace são registros de `src/vs/workbench/contrib`
//! (ver o porte de `mainThreadLanguageFeatures`).
//!
//! Representação:
//! - `ProviderResult<T>` é [`ProviderResult`]; `ITextModel` é o `Rc<TextModel>`; `IRange` é o [`Range`] e `IPosition` é
//!   a [`Position`] (ver `language_providers`).
//! - Um provedor é um trait e cada membro opcional do original (`resolveX?`, `onDidChange?`) é um método com
//!   implementação padrão ("ausente"), junto de um `has_*` quando a presença muda o comportamento do consumidor.
//! - Os quatro provedores de localização (`DefinitionProvider`, `DeclarationProvider`, `ImplementationProvider` e
//!   `TypeDefinitionProvider`, de `provideDefinition`, `provideDeclaration`, `provideImplementation` e
//!   `provideTypeDefinition`) têm a mesma forma e um trait só, [`LocationLinkProvider`]; cada um mora no seu registro
//!   do serviço.
//! - Os objetos que o original compartilha por referência e o provedor reconhece depois (`CodeLens`, `InlayHint`, `ILink`,
//!   as edições de colar e de soltar) carregam o `cacheId` do host de extensões no campo `cache_id` (o `CachedSessionItem`
//!   da interseção de tipos do `ICodeLensDto`, `IInlayHintDto`, `ILinkDto`, `IPasteEditDto` e `IDocumentDropEditDto`).
//! - `Hover` carrega o `id` do `HoverWithId` no campo `id`.
//! - `languages.TextEdit` é o [`TextEdit`] de `code_action::languages` (com os dois campos de snippet a mais), como no
//!   protocolo.
//! - `RegExp` é um `Rc<JsRegExp>`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::data_transfer::VSDataTransfer;
use je_base::event::Event;
use je_base::hierarchical_kind::HierarchicalKind;
use je_base::html_content::IMarkdownString;
use je_base::js_regexp::JsRegExp;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_instantiation::ServiceId;

use crate::code_action::languages::{CodeActionProvider, Command, TextEdit, WorkspaceEdit};
use crate::folding::syntax_range_provider::FoldingRangeProvider;
use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_providers::{
    DocumentHighlightProvider, DocumentSymbolProvider, MultiDocumentHighlightProvider, ProviderResult, SelectionRangeProvider,
};
use crate::languages::{CompletionItemProvider, ILink, StringOrMarkdown};
use crate::parameter_hints::languages::SignatureHelpProvider;
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;

/// `[cacheId, itemIdx]`: o `ChainedCacheId` do protocolo.
pub type ChainedCacheId = (u32, u32);

/// Um registro do serviço: o `LanguageFeatureRegistry<T>` compartilhado.
pub type Registry<T> = Rc<LanguageFeatureRegistry<T>>;

// ---- localização --------------------------------------------------------------------------------------

/// `languages.Location`.
#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub uri: URI,
    pub range: Range,
}

/// `languages.LocationLink`.
#[derive(Clone, Debug, PartialEq)]
pub struct LocationLink {
    pub origin_selection_range: Option<Range>,
    pub uri: URI,
    pub range: Range,
    pub target_selection_range: Option<Range>,
}

/// `DefinitionProvider`, `DeclarationProvider`, `ImplementationProvider` e `TypeDefinitionProvider` (ver a nota do módulo).
pub trait LocationLinkProvider {
    /// `provideDefinition`/`provideDeclaration`/`provideImplementation`/`provideTypeDefinition(model, position, token)`.
    fn provide_location_links(&self, model: &Rc<TextModel>, position: Position, token: &CancellationToken) -> ProviderResult<Vec<LocationLink>>;
}

/// `languages.ReferenceContext`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReferenceContext {
    pub include_declaration: bool,
}

/// `ReferenceProvider`.
pub trait ReferenceProvider {
    /// `provideReferences(model, position, context, token)`.
    fn provide_references(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        context: &ReferenceContext,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<Location>>;
}

// ---- hover --------------------------------------------------------------------------------------------

/// `languages.Hover`, com o `id` do `HoverWithId` (ver a nota do módulo).
#[derive(Clone, Debug)]
pub struct Hover {
    pub contents: Vec<IMarkdownString>,
    pub range: Option<Range>,
    pub can_increase_verbosity: Option<bool>,
    pub can_decrease_verbosity: Option<bool>,
    pub id: Option<u32>,
}

/// `languages.HoverVerbosityRequest<{ id }>`: o `previousHover` é só o `id` dele.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoverVerbosityRequest {
    pub verbosity_delta: f64,
    pub previous_hover_id: u32,
}

/// `languages.HoverContext`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HoverContext {
    pub verbosity_request: Option<HoverVerbosityRequest>,
}

/// `HoverProvider`.
pub trait HoverProvider {
    /// `provideHover(model, position, token, context?)`.
    fn provide_hover(&self, model: &Rc<TextModel>, position: Position, token: &CancellationToken, context: Option<&HoverContext>) -> ProviderResult<Hover>;
}

// ---- lente de código ----------------------------------------------------------------------------------

/// `languages.CodeLens`, com o `cacheId` do `ICodeLensDto`.
#[derive(Clone, Debug)]
pub struct CodeLens {
    pub range: Range,
    pub id: Option<String>,
    pub command: Option<Command>,
    pub cache_id: Option<ChainedCacheId>,
}

/// `languages.CodeLensList`: `{ lenses, dispose }`.
#[derive(Clone)]
pub struct CodeLensList {
    pub lenses: Vec<CodeLens>,
    dispose: Rc<dyn Fn()>,
}

impl CodeLensList {
    pub fn new(lenses: Vec<CodeLens>, dispose: impl Fn() + 'static) -> Self {
        Self { lenses, dispose: Rc::new(dispose) }
    }
}

impl IDisposable for CodeLensList {
    fn dispose(&self) {
        (self.dispose)();
    }
}

/// `languages.CodeLensProvider`.
pub trait CodeLensProvider {
    /// `onDidChange?`.
    fn on_did_change(&self) -> Option<Event<()>> {
        None
    }

    /// `provideCodeLenses(model, token)`.
    fn provide_code_lenses(&self, model: &Rc<TextModel>, token: &CancellationToken) -> ProviderResult<Rc<CodeLensList>>;

    /// Se o provedor implementa o `resolveCodeLens?` opcional.
    fn has_resolve_code_lens(&self) -> bool {
        false
    }

    /// `resolveCodeLens(model, codeLens, token)`.
    fn resolve_code_lens(&self, _model: &Rc<TextModel>, _code_lens: &CodeLens, _token: &CancellationToken) -> ProviderResult<CodeLens> {
        Promise::resolved(None)
    }
}

// ---- depuração, valores embutidos, edição ligada ----------------------------------------------------

/// `languages.EvaluatableExpression`.
#[derive(Clone, Debug, PartialEq)]
pub struct EvaluatableExpression {
    pub range: Range,
    pub expression: Option<String>,
}

/// `EvaluatableExpressionProvider`.
pub trait EvaluatableExpressionProvider {
    /// `provideEvaluatableExpression(model, position, token)`.
    fn provide_evaluatable_expression(&self, model: &Rc<TextModel>, position: Position, token: &CancellationToken) -> ProviderResult<EvaluatableExpression>;
}

/// `languages.InlineValue`: `InlineValueText | InlineValueVariableLookup | InlineValueExpression`.
#[derive(Clone, Debug, PartialEq)]
pub enum InlineValue {
    Text { range: Range, text: String },
    VariableLookup { range: Range, variable_name: Option<String>, case_sensitive_lookup: bool },
    Expression { range: Range, expression: Option<String> },
}

/// `languages.InlineValueContext`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineValueContext {
    pub frame_id: f64,
    pub stopped_location: Range,
}

/// `languages.InlineValuesProvider`.
pub trait InlineValuesProvider {
    /// `onDidChangeInlineValues?`.
    fn on_did_change_inline_values(&self) -> Option<Event<()>> {
        None
    }

    /// `provideInlineValues(model, viewPort, context, token)`.
    fn provide_inline_values(
        &self,
        model: &Rc<TextModel>,
        view_port: Range,
        context: &InlineValueContext,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<InlineValue>>;
}

/// `languages.LinkedEditingRanges`.
#[derive(Clone)]
pub struct LinkedEditingRanges {
    pub ranges: Vec<Range>,
    pub word_pattern: Option<Rc<JsRegExp>>,
}

/// `languages.LinkedEditingRangeProvider`.
pub trait LinkedEditingRangeProvider {
    /// `provideLinkedEditingRanges(model, position, token)`.
    fn provide_linked_editing_ranges(&self, model: &Rc<TextModel>, position: Position, token: &CancellationToken) -> ProviderResult<LinkedEditingRanges>;
}

// ---- formatação ---------------------------------------------------------------------------------------

/// `languages.FormattingOptions`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormattingOptions {
    pub tab_size: f64,
    pub insert_spaces: bool,
}

/// `DocumentFormattingEditProvider`.
pub trait DocumentFormattingEditProvider {
    /// `extensionId?`.
    fn extension_id(&self) -> Option<&ExtensionIdentifier> {
        None
    }

    /// `displayName?`.
    fn display_name(&self) -> Option<&str> {
        None
    }

    /// `provideDocumentFormattingEdits(model, options, token)`.
    fn provide_document_formatting_edits(&self, model: &Rc<TextModel>, options: &FormattingOptions, token: &CancellationToken) -> ProviderResult<Vec<TextEdit>>;
}

/// `DocumentRangeFormattingEditProvider`.
pub trait DocumentRangeFormattingEditProvider {
    /// `extensionId?`.
    fn extension_id(&self) -> Option<&ExtensionIdentifier> {
        None
    }

    /// `displayName?`.
    fn display_name(&self) -> Option<&str> {
        None
    }

    /// `provideDocumentRangeFormattingEdits(model, range, options, token)`.
    fn provide_document_range_formatting_edits(
        &self,
        model: &Rc<TextModel>,
        range: Range,
        options: &FormattingOptions,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<TextEdit>>;

    /// Se o provedor implementa o `provideDocumentRangesFormattingEdits?` opcional.
    fn has_ranges_formatting(&self) -> bool {
        false
    }

    /// `provideDocumentRangesFormattingEdits(model, ranges, options, token)`.
    fn provide_document_ranges_formatting_edits(
        &self,
        _model: &Rc<TextModel>,
        _ranges: &[Range],
        _options: &FormattingOptions,
        _token: &CancellationToken,
    ) -> ProviderResult<Vec<TextEdit>> {
        Promise::resolved(None)
    }
}

/// `OnTypeFormattingEditProvider`.
pub trait OnTypeFormattingEditProvider {
    /// `extensionId?`.
    fn extension_id(&self) -> Option<&ExtensionIdentifier> {
        None
    }

    /// `autoFormatTriggerCharacters`.
    fn auto_format_trigger_characters(&self) -> &[String];

    /// `provideOnTypeFormattingEdits(model, position, ch, options, token)`.
    fn provide_on_type_formatting_edits(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        ch: &str,
        options: &FormattingOptions,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<TextEdit>>;
}

// ---- renomeação ---------------------------------------------------------------------------------------

/// `languages.WorkspaceEdit & languages.Rejection`: o lado que falta é `None`.
#[derive(Clone, Default)]
pub struct RenameEdits {
    pub workspace_edit: Option<WorkspaceEdit>,
    pub reject_reason: Option<String>,
}

/// `languages.RenameLocation & languages.Rejection`: o lado que falta é `None`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenameLocation {
    pub range: Option<Range>,
    pub text: Option<String>,
    pub reject_reason: Option<String>,
}

/// `RenameProvider`.
pub trait RenameProvider {
    /// `provideRenameEdits(model, position, newName, token)`.
    fn provide_rename_edits(&self, model: &Rc<TextModel>, position: Position, new_name: &str, token: &CancellationToken) -> ProviderResult<RenameEdits>;

    /// Se o provedor implementa o `resolveRenameLocation?` opcional.
    fn has_resolve_rename_location(&self) -> bool {
        false
    }

    /// `resolveRenameLocation(model, position, token)`.
    fn resolve_rename_location(&self, _model: &Rc<TextModel>, _position: Position, _token: &CancellationToken) -> ProviderResult<RenameLocation> {
        Promise::resolved(None)
    }
}

/// `languages.NewSymbolNameTag`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewSymbolNameTag {
    AIGenerated = 1,
}

/// `languages.NewSymbolNameTriggerKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NewSymbolNameTriggerKind {
    Invoke = 0,
    Automatic = 1,
}

/// `languages.NewSymbolName`.
#[derive(Clone, Debug, PartialEq)]
pub struct NewSymbolName {
    pub new_symbol_name: String,
    pub tags: Option<Vec<NewSymbolNameTag>>,
}

/// `languages.NewSymbolNamesProvider`.
pub trait NewSymbolNamesProvider {
    /// `supportsAutomaticNewSymbolNamesTriggerKind`: a promessa que o host de extensões responde.
    fn supports_automatic_new_symbol_names_trigger_kind(&self) -> Promise<Option<bool>>;

    /// `provideNewSymbolNames(model, range, triggerKind, token)`.
    fn provide_new_symbol_names(
        &self,
        model: &Rc<TextModel>,
        range: Range,
        trigger_kind: NewSymbolNameTriggerKind,
        token: &CancellationToken,
    ) -> ProviderResult<Vec<NewSymbolName>>;
}

// ---- tokens semânticos --------------------------------------------------------------------------------

/// `languages.SemanticTokensLegend`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticTokensLegend {
    pub token_types: Vec<String>,
    pub token_modifiers: Vec<String>,
}

/// `languages.SemanticTokens`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticTokens {
    pub result_id: Option<String>,
    pub data: Vec<u32>,
}

/// `languages.SemanticTokensEdit`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticTokensEdit {
    pub start: u32,
    pub delete_count: u32,
    pub data: Option<Vec<u32>>,
}

/// `languages.SemanticTokensEdits`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticTokensEdits {
    pub result_id: Option<String>,
    pub edits: Vec<SemanticTokensEdit>,
}

/// `languages.SemanticTokens | languages.SemanticTokensEdits`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticTokensResult {
    Tokens(SemanticTokens),
    Edits(SemanticTokensEdits),
}

/// `languages.DocumentSemanticTokensProvider`.
pub trait DocumentSemanticTokensProvider {
    /// `onDidChange?`.
    fn on_did_change(&self) -> Option<Event<()>> {
        None
    }

    /// `getLegend()`.
    fn get_legend(&self) -> SemanticTokensLegend;

    /// `provideDocumentSemanticTokens(model, lastResultId, token)`: `None` é o `null`.
    fn provide_document_semantic_tokens(
        &self,
        model: &Rc<TextModel>,
        last_result_id: Option<&str>,
        token: &CancellationToken,
    ) -> Promise<Option<SemanticTokensResult>>;

    /// `releaseDocumentSemanticTokens(resultId)`.
    fn release_document_semantic_tokens(&self, result_id: Option<&str>);
}

/// `languages.DocumentRangeSemanticTokensProvider`.
pub trait DocumentRangeSemanticTokensProvider {
    /// `onDidChange?`.
    fn on_did_change(&self) -> Option<Event<()>> {
        None
    }

    /// `getLegend()`.
    fn get_legend(&self) -> SemanticTokensLegend;

    /// `provideDocumentRangeSemanticTokens(model, range, token)`: `None` é o `null`.
    fn provide_document_range_semantic_tokens(&self, model: &Rc<TextModel>, range: Range, token: &CancellationToken) -> Promise<Option<SemanticTokens>>;
}

// ---- dicas embutidas ----------------------------------------------------------------------------------

/// `languages.InlayHintKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlayHintKind {
    Type = 1,
    Parameter = 2,
}

/// `languages.InlayHintLabelPart`.
#[derive(Clone, Debug)]
pub struct InlayHintLabelPart {
    pub label: String,
    pub tooltip: Option<StringOrMarkdown>,
    pub location: Option<Location>,
    pub command: Option<Command>,
}

/// `string | languages.InlayHintLabelPart[]`.
#[derive(Clone, Debug)]
pub enum InlayHintLabel {
    Text(String),
    Parts(Vec<InlayHintLabelPart>),
}

/// `languages.InlayHint`, com o `cacheId` do `IInlayHintDto`.
#[derive(Clone)]
pub struct InlayHint {
    pub label: InlayHintLabel,
    pub tooltip: Option<StringOrMarkdown>,
    pub position: Position,
    pub text_edits: Option<Vec<TextEdit>>,
    pub kind: Option<InlayHintKind>,
    pub padding_left: Option<bool>,
    pub padding_right: Option<bool>,
    pub cache_id: Option<ChainedCacheId>,
}

/// `languages.InlayHintList`: `{ hints, dispose }`.
#[derive(Clone)]
pub struct InlayHintList {
    pub hints: Vec<InlayHint>,
    dispose: Rc<dyn Fn()>,
}

impl InlayHintList {
    pub fn new(hints: Vec<InlayHint>, dispose: impl Fn() + 'static) -> Self {
        Self { hints, dispose: Rc::new(dispose) }
    }
}

impl IDisposable for InlayHintList {
    fn dispose(&self) {
        (self.dispose)();
    }
}

/// `languages.InlayHintsProvider`.
pub trait InlayHintsProvider {
    /// `displayName?`.
    fn display_name(&self) -> Option<&str> {
        None
    }

    /// `onDidChangeInlayHints?`.
    fn on_did_change_inlay_hints(&self) -> Option<Event<()>> {
        None
    }

    /// `provideInlayHints(model, range, token)`.
    fn provide_inlay_hints(&self, model: &Rc<TextModel>, range: Range, token: &CancellationToken) -> ProviderResult<Rc<InlayHintList>>;

    /// Se o provedor implementa o `resolveInlayHint?` opcional.
    fn has_resolve_inlay_hint(&self) -> bool {
        false
    }

    /// `resolveInlayHint(hint, token)`.
    fn resolve_inlay_hint(&self, _hint: &InlayHint, _token: &CancellationToken) -> ProviderResult<InlayHint> {
        Promise::resolved(None)
    }
}

// ---- links, cores -------------------------------------------------------------------------------------

/// `languages.ILinksList`: `{ links, dispose? }`.
#[derive(Clone)]
pub struct LinksList {
    pub links: Vec<ILink>,
    dispose: Rc<dyn Fn()>,
}

impl LinksList {
    pub fn new(links: Vec<ILink>, dispose: impl Fn() + 'static) -> Self {
        Self { links, dispose: Rc::new(dispose) }
    }
}

impl IDisposable for LinksList {
    fn dispose(&self) {
        (self.dispose)();
    }
}

/// `languages.LinkProvider`.
pub trait LinkProvider {
    /// `provideLinks(model, token)`.
    fn provide_links(&self, model: &Rc<TextModel>, token: &CancellationToken) -> ProviderResult<Rc<LinksList>>;

    /// Se o provedor implementa o `resolveLink?` opcional.
    fn has_resolve_link(&self) -> bool {
        false
    }

    /// `resolveLink(link, token)`.
    fn resolve_link(&self, _link: &ILink, _token: &CancellationToken) -> ProviderResult<ILink> {
        Promise::resolved(None)
    }
}

/// `languages.IColor`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

/// `languages.IColorInformation`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IColorInformation {
    pub range: Range,
    pub color: IColor,
}

/// `languages.IColorPresentation`.
#[derive(Clone)]
pub struct IColorPresentation {
    pub label: String,
    pub text_edit: Option<TextEdit>,
    pub additional_text_edits: Option<Vec<TextEdit>>,
}

/// `languages.DocumentColorProvider`.
pub trait DocumentColorProvider {
    /// `provideDocumentColors(model, token)`.
    fn provide_document_colors(&self, model: &Rc<TextModel>, token: &CancellationToken) -> ProviderResult<Vec<IColorInformation>>;

    /// `provideColorPresentations(model, colorInfo, token)`.
    fn provide_color_presentations(&self, model: &Rc<TextModel>, color_info: &IColorInformation, token: &CancellationToken) -> ProviderResult<Vec<IColorPresentation>>;

    /// O `provider instanceof DefaultDocumentColorProvider` de `_findColorData`: só o provedor padrão embutido responde
    /// `true`.
    fn is_default_document_color_provider(&self) -> bool {
        false
    }
}

// ---- colar e soltar -----------------------------------------------------------------------------------

/// `string | { snippet: string }`: o `insertText` das edições de colar e de soltar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocumentEditInsertText {
    Text(String),
    Snippet(String),
}

/// `{ kind: HierarchicalKind }`: um item de `yieldTo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropYieldTo {
    pub kind: HierarchicalKind,
}

/// `languages.DocumentPasteTriggerKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocumentPasteTriggerKind {
    Automatic = 0,
    PasteAs = 1,
}

/// `languages.DocumentPasteContext`.
#[derive(Clone, Debug)]
pub struct DocumentPasteContext {
    pub only: Option<HierarchicalKind>,
    pub trigger_kind: DocumentPasteTriggerKind,
}

/// `languages.DocumentPasteEdit`, com o `_cacheId` do `IPasteEditDto`.
#[derive(Clone)]
pub struct DocumentPasteEdit {
    pub title: String,
    pub kind: HierarchicalKind,
    pub yield_to: Option<Vec<DropYieldTo>>,
    pub insert_text: DocumentEditInsertText,
    pub additional_edit: Option<WorkspaceEdit>,
    pub cache_id: Option<ChainedCacheId>,
}

/// O objeto de edição compartilhado por referência (o `resolveDocumentPasteEdit` o altera).
pub type DocumentPasteEditRef = Rc<RefCell<DocumentPasteEdit>>;

/// `languages.DocumentPasteEditsSession`.
#[derive(Clone)]
pub struct DocumentPasteEditsSession {
    pub edits: Vec<DocumentPasteEditRef>,
    dispose: Rc<dyn Fn()>,
}

impl DocumentPasteEditsSession {
    pub fn new(edits: Vec<DocumentPasteEditRef>, dispose: impl Fn() + 'static) -> Self {
        Self { edits, dispose: Rc::new(dispose) }
    }
}

impl IDisposable for DocumentPasteEditsSession {
    fn dispose(&self) {
        (self.dispose)();
    }
}

/// `languages.DocumentPasteEditProvider`.
pub trait DocumentPasteEditProvider {
    /// `copyMimeTypes`.
    fn copy_mime_types(&self) -> &[String];

    /// `pasteMimeTypes`.
    fn paste_mime_types(&self) -> &[String];

    /// `providedPasteEditKinds`.
    fn provided_paste_edit_kinds(&self) -> &[HierarchicalKind];

    /// Se o provedor implementa o `prepareDocumentPaste?` opcional.
    fn has_prepare_document_paste(&self) -> bool {
        false
    }

    /// `prepareDocumentPaste(model, selections, dataTransfer, token)`.
    fn prepare_document_paste(
        &self,
        _model: &Rc<TextModel>,
        _selections: &[Range],
        _data_transfer: &Rc<VSDataTransfer>,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<VSDataTransfer>> {
        Promise::resolved(None)
    }

    /// Se o provedor implementa o `provideDocumentPasteEdits?` opcional.
    fn has_provide_document_paste_edits(&self) -> bool {
        false
    }

    /// `provideDocumentPasteEdits(model, selections, dataTransfer, context, token)`.
    fn provide_document_paste_edits(
        &self,
        _model: &Rc<TextModel>,
        _selections: &[Selection],
        _data_transfer: &Rc<VSDataTransfer>,
        _context: &DocumentPasteContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<DocumentPasteEditsSession>> {
        Promise::resolved(None)
    }

    /// Se o provedor implementa o `resolveDocumentPasteEdit?` opcional.
    fn has_resolve_document_paste_edit(&self) -> bool {
        false
    }

    /// `resolveDocumentPasteEdit(edit, token)`.
    fn resolve_document_paste_edit(&self, _edit: &DocumentPasteEditRef, _token: &CancellationToken) -> ProviderResult<DocumentPasteEditRef> {
        Promise::resolved(None)
    }
}

/// `languages.DocumentDropEdit`, com o `_cacheId` do `IDocumentDropEditDto`.
#[derive(Clone)]
pub struct DocumentDropEdit {
    pub title: String,
    pub kind: Option<HierarchicalKind>,
    pub yield_to: Option<Vec<DropYieldTo>>,
    pub insert_text: DocumentEditInsertText,
    pub additional_edit: Option<WorkspaceEdit>,
    pub cache_id: Option<ChainedCacheId>,
}

/// O objeto de edição compartilhado por referência (o `resolveDocumentDropEdit` o altera).
pub type DocumentDropEditRef = Rc<RefCell<DocumentDropEdit>>;

/// `languages.DocumentDropEditsSession`.
#[derive(Clone)]
pub struct DocumentDropEditsSession {
    pub edits: Vec<DocumentDropEditRef>,
    dispose: Rc<dyn Fn()>,
}

impl DocumentDropEditsSession {
    pub fn new(edits: Vec<DocumentDropEditRef>, dispose: impl Fn() + 'static) -> Self {
        Self { edits, dispose: Rc::new(dispose) }
    }
}

impl IDisposable for DocumentDropEditsSession {
    fn dispose(&self) {
        (self.dispose)();
    }
}

/// `languages.DocumentDropEditProvider`.
pub trait DocumentDropEditProvider {
    /// `dropMimeTypes?`.
    fn drop_mime_types(&self) -> &[String];

    /// `providedDropEditKinds?`.
    fn provided_drop_edit_kinds(&self) -> Option<&[HierarchicalKind]>;

    /// `provideDocumentDropEdits(model, position, dataTransfer, token)`.
    fn provide_document_drop_edits(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        data_transfer: &Rc<VSDataTransfer>,
        token: &CancellationToken,
    ) -> ProviderResult<Rc<DocumentDropEditsSession>>;

    /// Se o provedor implementa o `resolveDocumentDropEdit?` opcional.
    fn has_resolve_document_drop_edit(&self) -> bool {
        false
    }

    /// `resolveDocumentDropEdit(edit, token)`.
    fn resolve_document_drop_edit(&self, _edit: &DocumentDropEditRef, _token: &CancellationToken) -> ProviderResult<DocumentDropEditRef> {
        Promise::resolved(None)
    }
}

// ---- o serviço ----------------------------------------------------------------------------------------

/// O identificador do `ILanguageFeaturesService` (o serviço é o [`LanguageFeaturesService`] concreto).
pub const I_LANGUAGE_FEATURES_SERVICE: ServiceId<LanguageFeaturesService> = ServiceId::new("languageFeaturesService");

/// `ILanguageFeaturesService` (a implementação é a `LanguageFeaturesService`): um registro por recurso.
#[derive(Clone)]
pub struct LanguageFeaturesService {
    pub reference_provider: Registry<dyn ReferenceProvider>,
    pub rename_provider: Registry<dyn RenameProvider>,
    pub new_symbol_names_provider: Registry<dyn NewSymbolNamesProvider>,
    pub code_action_provider: Registry<dyn CodeActionProvider>,
    pub definition_provider: Registry<dyn LocationLinkProvider>,
    pub type_definition_provider: Registry<dyn LocationLinkProvider>,
    pub declaration_provider: Registry<dyn LocationLinkProvider>,
    pub implementation_provider: Registry<dyn LocationLinkProvider>,
    pub document_symbol_provider: Registry<dyn DocumentSymbolProvider>,
    pub hover_provider: Registry<dyn HoverProvider>,
    pub document_highlight_provider: Registry<dyn DocumentHighlightProvider>,
    pub multi_document_highlight_provider: Registry<dyn MultiDocumentHighlightProvider>,
    pub selection_range_provider: Registry<dyn SelectionRangeProvider>,
    pub folding_range_provider: Registry<dyn FoldingRangeProvider>,
    pub document_formatting_edit_provider: Registry<dyn DocumentFormattingEditProvider>,
    pub document_range_formatting_edit_provider: Registry<dyn DocumentRangeFormattingEditProvider>,
    pub on_type_formatting_edit_provider: Registry<dyn OnTypeFormattingEditProvider>,
    pub signature_help_provider: Registry<dyn SignatureHelpProvider>,
    pub completion_provider: Registry<dyn CompletionItemProvider>,
    pub code_lens_provider: Registry<dyn CodeLensProvider>,
    pub link_provider: Registry<dyn LinkProvider>,
    pub color_provider: Registry<dyn DocumentColorProvider>,
    pub inlay_hints_provider: Registry<dyn InlayHintsProvider>,
    pub document_semantic_tokens_provider: Registry<dyn DocumentSemanticTokensProvider>,
    pub document_range_semantic_tokens_provider: Registry<dyn DocumentRangeSemanticTokensProvider>,
    pub document_drop_edit_provider: Registry<dyn DocumentDropEditProvider>,
    pub document_paste_edit_provider: Registry<dyn DocumentPasteEditProvider>,
    pub linked_editing_range_provider: Registry<dyn LinkedEditingRangeProvider>,
    pub evaluatable_expression_provider: Registry<dyn EvaluatableExpressionProvider>,
    pub inline_values_provider: Registry<dyn InlineValuesProvider>,
    pub inline_completions_provider: Registry<dyn crate::languages::InlineCompletionsProvider>,
}

impl LanguageFeaturesService {
    /// `new LanguageFeaturesService()`: um registro vazio por recurso.
    pub fn new() -> Self {
        fn registry<T: ?Sized + 'static>() -> Registry<T> {
            Rc::new(LanguageFeatureRegistry::new(None))
        }
        Self {
            reference_provider: registry(),
            rename_provider: registry(),
            new_symbol_names_provider: registry(),
            code_action_provider: registry(),
            definition_provider: registry(),
            type_definition_provider: registry(),
            declaration_provider: registry(),
            implementation_provider: registry(),
            document_symbol_provider: registry(),
            hover_provider: registry(),
            document_highlight_provider: registry(),
            multi_document_highlight_provider: registry(),
            selection_range_provider: registry(),
            folding_range_provider: registry(),
            document_formatting_edit_provider: registry(),
            document_range_formatting_edit_provider: registry(),
            on_type_formatting_edit_provider: registry(),
            signature_help_provider: registry(),
            completion_provider: registry(),
            code_lens_provider: registry(),
            link_provider: registry(),
            color_provider: registry(),
            inlay_hints_provider: registry(),
            document_semantic_tokens_provider: registry(),
            document_range_semantic_tokens_provider: registry(),
            document_drop_edit_provider: registry(),
            document_paste_edit_provider: registry(),
            linked_editing_range_provider: registry(),
            evaluatable_expression_provider: registry(),
            inline_values_provider: registry(),
            inline_completions_provider: registry(),
        }
    }
}

impl Default for LanguageFeaturesService {
    fn default() -> Self {
        Self::new()
    }
}
