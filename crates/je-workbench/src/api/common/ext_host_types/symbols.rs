//! Classes de linguagem de `extHostTypes.ts` e `extHostTypes/diagnostic.ts`, `symbolInformation.ts`,
//! `codeActionKind.ts`: diagnósticos, símbolos, ações de código, hover, lente, assinatura, dicas
//! embutidas, valores embutidos, hierarquias e intervalos de dobra.
//!
//! Divergências:
//! - `vscode.Command` é [`Command`] e `string | MarkdownString` é [`Documentation`] (em `ext_host_types`).
//! - Os construtores que lançam (`new Diagnostic`, `new SymbolInformation`, `new DocumentSymbol`,
//!   `new SelectionRange`) devolvem [`Result`]. `Range.isRange(range)` do `Diagnostic` é garantido pelo tipo.
//! - `SymbolInformation` tem dois construtores no original (sobrecargas): [`SymbolInformation::new`] (com
//!   `containerName` e `Location`) e [`SymbolInformation::from_range`] (com `Range`, `uri?`, `containerName?`).
//!   A `location` nunca fica sem valor: ambas as formas a criam.
//! - `Diagnostic.isEqual(a, b)` compara por valor (o atalho `a === b` não existe sem identidade).
//! - `CodeActionKind` não é o [`HierarchicalKind`](je_base::hierarchical_kind::HierarchicalKind): o
//!   `contains` de uma classe vazia aqui só contém ela mesma.
//! - `LinkedEditingRanges.wordPattern` (`RegExp`) é [`Dynamic`] (o `RegExp` dinâmico).
//! - `toJSON` devolve [`Value`] sem os campos `undefined`, como `JSON.stringify` do original.

use je_base::arrays::equals as arrays_equal;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::json::Value;
use je_base::uri::URI;

use super::edits::{TextEdit, WorkspaceEdit};
use super::enums::{DocumentHighlightKind, FoldingRangeKind, InlayHintKind};
use super::position_range::{Location, Position, Range};
use super::{Command, Documentation, MarkedString, json_object, uri_to_json};

// ---- diagnostic -------------------------------------------------------------------------------------

api_enum!(DiagnosticTag { Unnecessary = 1, Deprecated = 2 });
api_enum!(DiagnosticSeverity { Hint = 3, Information = 2, Warning = 1, Error = 0 });

/// `DiagnosticRelatedInformation`.
#[derive(Clone, Debug, PartialEq)]
pub struct DiagnosticRelatedInformation {
    pub location: Location,
    pub message: String,
}

impl DiagnosticRelatedInformation {
    pub fn new(location: Location, message: impl Into<String>) -> DiagnosticRelatedInformation {
        DiagnosticRelatedInformation { location, message: message.into() }
    }

    pub fn is_equal(a: &DiagnosticRelatedInformation, b: &DiagnosticRelatedInformation) -> bool {
        a.message == b.message && a.location.range == b.location.range && a.location.uri == b.location.uri
    }
}

/// `string | number`: o `code` de um diagnóstico.
#[derive(Clone, Debug, PartialEq)]
pub enum DiagnosticCode {
    Text(String),
    Number(f64),
}

/// `Diagnostic`.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub range: Range,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub source: Option<String>,
    pub code: Option<DiagnosticCode>,
    pub related_information: Option<Vec<DiagnosticRelatedInformation>>,
    pub tags: Option<Vec<DiagnosticTag>>,
}

impl Diagnostic {
    /// `new Diagnostic(range, message, severity = Error)`: lança `TypeError` com `message` vazia.
    pub fn new(range: Range, message: &str, severity: Option<DiagnosticSeverity>) -> Result<Diagnostic, Error> {
        if message.is_empty() {
            return Err(Error::type_error("message must be set"));
        }
        Ok(Diagnostic {
            range,
            message: message.to_owned(),
            severity: severity.unwrap_or(DiagnosticSeverity::Error),
            source: None,
            code: None,
            related_information: None,
            tags: None,
        })
    }

    /// `toJSON()`: `{ severity, message, range, source, code }`.
    pub fn to_json(&self) -> Value {
        json_object(vec![
            ("severity", Some(Value::from(self.severity.name()))),
            ("message", Some(Value::from(self.message.as_str()))),
            ("range", Some(self.range.to_json())),
            ("source", self.source.as_deref().map(Value::from)),
            (
                "code",
                self.code.as_ref().map(|code| match code {
                    DiagnosticCode::Text(text) => Value::from(text.as_str()),
                    DiagnosticCode::Number(number) => Value::Number(*number),
                }),
            ),
        ])
    }

    pub fn is_equal(a: Option<&Diagnostic>, b: Option<&Diagnostic>) -> bool {
        match (a, b) {
            (Some(a), Some(b)) => {
                a.message == b.message
                    && a.severity == b.severity
                    && a.code == b.code
                    && a.source == b.source
                    && a.range.is_equal(&b.range)
                    && a.tags == b.tags
                    && arrays_equal(a.related_information.as_deref(), b.related_information.as_deref(), DiagnosticRelatedInformation::is_equal)
            }
            (None, None) => true,
            _ => false,
        }
    }
}

// ---- code action kind -------------------------------------------------------------------------------

/// `CodeActionKind`: um tipo hierárquico separado por pontos (`refactor.extract`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CodeActionKind {
    pub value: String,
}

impl CodeActionKind {
    const SEP: &'static str = ".";

    pub fn new(value: impl Into<String>) -> CodeActionKind {
        CodeActionKind { value: value.into() }
    }

    pub fn empty() -> CodeActionKind {
        CodeActionKind::new("")
    }

    pub fn quick_fix() -> CodeActionKind {
        CodeActionKind::empty().append("quickfix")
    }

    pub fn refactor() -> CodeActionKind {
        CodeActionKind::empty().append("refactor")
    }

    pub fn refactor_extract() -> CodeActionKind {
        CodeActionKind::refactor().append("extract")
    }

    pub fn refactor_inline() -> CodeActionKind {
        CodeActionKind::refactor().append("inline")
    }

    pub fn refactor_move() -> CodeActionKind {
        CodeActionKind::refactor().append("move")
    }

    pub fn refactor_rewrite() -> CodeActionKind {
        CodeActionKind::refactor().append("rewrite")
    }

    pub fn source() -> CodeActionKind {
        CodeActionKind::empty().append("source")
    }

    pub fn source_organize_imports() -> CodeActionKind {
        CodeActionKind::source().append("organizeImports")
    }

    pub fn source_fix_all() -> CodeActionKind {
        CodeActionKind::source().append("fixAll")
    }

    pub fn notebook() -> CodeActionKind {
        CodeActionKind::empty().append("notebook")
    }

    pub fn append(&self, parts: &str) -> CodeActionKind {
        if self.value.is_empty() {
            CodeActionKind::new(parts)
        } else {
            CodeActionKind::new(format!("{}{}{}", self.value, Self::SEP, parts))
        }
    }

    pub fn intersects(&self, other: &CodeActionKind) -> bool {
        self.contains(other) || other.contains(self)
    }

    pub fn contains(&self, other: &CodeActionKind) -> bool {
        self.value == other.value || other.value.starts_with(&format!("{}{}", self.value, Self::SEP))
    }
}

// ---- symbols ----------------------------------------------------------------------------------------

api_enum!(SymbolKind {
    File = 0, Module = 1, Namespace = 2, Package = 3, Class = 4, Method = 5, Property = 6, Field = 7,
    Constructor = 8, Enum = 9, Interface = 10, Function = 11, Variable = 12, Constant = 13, String = 14,
    Number = 15, Boolean = 16, Array = 17, Object = 18, Key = 19, Null = 20, EnumMember = 21, Struct = 22,
    Event = 23, Operator = 24, TypeParameter = 25
});
api_enum!(SymbolTag { Deprecated = 1 });

/// `SymbolInformation`.
#[derive(Clone, Debug, PartialEq)]
pub struct SymbolInformation {
    pub name: String,
    pub location: Location,
    pub kind: SymbolKind,
    pub tags: Option<Vec<SymbolTag>>,
    pub container_name: Option<String>,
}

impl SymbolInformation {
    pub fn validate(candidate: &SymbolInformation) -> Result<(), Error> {
        if candidate.name.is_empty() {
            return Err(Error::new("name must not be falsy"));
        }
        Ok(())
    }

    /// `new SymbolInformation(name, kind, containerName, location)`.
    pub fn new(name: &str, kind: SymbolKind, container_name: Option<&str>, location: Location) -> Result<SymbolInformation, Error> {
        let info = SymbolInformation { name: name.to_owned(), location, kind, tags: None, container_name: container_name.map(str::to_owned) };
        SymbolInformation::validate(&info)?;
        Ok(info)
    }

    /// `new SymbolInformation(name, kind, range, uri?, containerName?)`.
    pub fn from_range(name: &str, kind: SymbolKind, range: Range, uri: Option<URI>, container_name: Option<&str>) -> Result<SymbolInformation, Error> {
        SymbolInformation::new(name, kind, container_name, Location::new(uri, Some(range.into())))
    }

    /// `toJSON()`: `{ name, kind, location, containerName }`.
    pub fn to_json(&self) -> Value {
        json_object(vec![
            ("name", Some(Value::from(self.name.as_str()))),
            ("kind", Some(Value::from(self.kind.name()))),
            ("location", Some(self.location.to_json())),
            ("containerName", self.container_name.as_deref().map(Value::from)),
        ])
    }
}

/// `DocumentHighlight`.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentHighlight {
    pub range: Range,
    pub kind: DocumentHighlightKind,
}

impl DocumentHighlight {
    /// `new DocumentHighlight(range, kind = Text)`.
    pub fn new(range: Range, kind: Option<DocumentHighlightKind>) -> DocumentHighlight {
        DocumentHighlight { range, kind: kind.unwrap_or(DocumentHighlightKind::Text) }
    }

    /// `toJSON()`: `{ range, kind }` com o nome do tipo.
    pub fn to_json(&self) -> Value {
        json_object(vec![("range", Some(self.range.to_json())), ("kind", Some(Value::from(self.kind.name())))])
    }
}

/// `MultiDocumentHighlight`.
#[derive(Clone, Debug, PartialEq)]
pub struct MultiDocumentHighlight {
    pub uri: URI,
    pub highlights: Vec<DocumentHighlight>,
}

impl MultiDocumentHighlight {
    pub fn to_json(&self) -> Value {
        json_object(vec![
            ("uri", Some(uri_to_json(&self.uri))),
            ("highlights", Some(Value::Array(self.highlights.iter().map(DocumentHighlight::to_json).collect()))),
        ])
    }
}

/// `DocumentSymbol`.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentSymbol {
    pub name: String,
    pub detail: String,
    pub kind: SymbolKind,
    pub tags: Option<Vec<SymbolTag>>,
    pub range: Range,
    pub selection_range: Range,
    pub children: Vec<DocumentSymbol>,
}

impl DocumentSymbol {
    pub fn validate(candidate: &DocumentSymbol) -> Result<(), Error> {
        if candidate.name.is_empty() {
            return Err(Error::new("name must not be falsy"));
        }
        if !candidate.range.contains_range(&candidate.selection_range) {
            return Err(Error::new("selectionRange must be contained in fullRange"));
        }
        candidate.children.iter().try_for_each(DocumentSymbol::validate)
    }

    pub fn new(name: &str, detail: &str, kind: SymbolKind, range: Range, selection_range: Range) -> Result<DocumentSymbol, Error> {
        let symbol = DocumentSymbol {
            name: name.to_owned(),
            detail: detail.to_owned(),
            kind,
            tags: None,
            range,
            selection_range,
            children: Vec::new(),
        };
        DocumentSymbol::validate(&symbol)?;
        Ok(symbol)
    }
}

// ---- code action, selection range, hierarchies ------------------------------------------------------

/// `CodeAction`.
#[derive(Clone)]
pub struct CodeAction {
    pub title: String,
    pub command: Option<Command>,
    pub edit: Option<WorkspaceEdit>,
    pub diagnostics: Option<Vec<Diagnostic>>,
    pub kind: Option<CodeActionKind>,
    pub is_preferred: Option<bool>,
}

impl CodeAction {
    pub fn new(title: impl Into<String>, kind: Option<CodeActionKind>) -> CodeAction {
        CodeAction { title: title.into(), command: None, edit: None, diagnostics: None, kind, is_preferred: None }
    }
}

/// `SelectionRange`.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionRange {
    pub range: Range,
    pub parent: Option<Box<SelectionRange>>,
}

impl SelectionRange {
    /// `new SelectionRange(range, parent?)`: o pai precisa conter o intervalo.
    pub fn new(range: Range, parent: Option<SelectionRange>) -> Result<SelectionRange, Error> {
        if parent.as_ref().is_some_and(|parent| !parent.range.contains_range(&range)) {
            return Err(Error::new("Invalid argument: parent must contain this range"));
        }
        Ok(SelectionRange { range, parent: parent.map(Box::new) })
    }
}

/// `CallHierarchyItem`.
#[derive(Clone, Debug, PartialEq)]
pub struct CallHierarchyItem {
    pub session_id: Option<String>,
    pub item_id: Option<String>,
    pub kind: SymbolKind,
    pub tags: Option<Vec<SymbolTag>>,
    pub name: String,
    pub detail: Option<String>,
    pub uri: URI,
    pub range: Range,
    pub selection_range: Range,
}

impl CallHierarchyItem {
    pub fn new(kind: SymbolKind, name: &str, detail: &str, uri: URI, range: Range, selection_range: Range) -> CallHierarchyItem {
        CallHierarchyItem {
            session_id: None,
            item_id: None,
            kind,
            tags: None,
            name: name.to_owned(),
            detail: Some(detail.to_owned()),
            uri,
            range,
            selection_range,
        }
    }
}

/// `CallHierarchyIncomingCall`.
#[derive(Clone, Debug, PartialEq)]
pub struct CallHierarchyIncomingCall {
    pub from: CallHierarchyItem,
    pub from_ranges: Vec<Range>,
}

impl CallHierarchyIncomingCall {
    pub fn new(item: CallHierarchyItem, from_ranges: Vec<Range>) -> CallHierarchyIncomingCall {
        CallHierarchyIncomingCall { from: item, from_ranges }
    }
}

/// `CallHierarchyOutgoingCall`.
#[derive(Clone, Debug, PartialEq)]
pub struct CallHierarchyOutgoingCall {
    pub to: CallHierarchyItem,
    pub from_ranges: Vec<Range>,
}

impl CallHierarchyOutgoingCall {
    pub fn new(item: CallHierarchyItem, from_ranges: Vec<Range>) -> CallHierarchyOutgoingCall {
        CallHierarchyOutgoingCall { to: item, from_ranges }
    }
}

/// `TypeHierarchyItem`.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeHierarchyItem {
    pub session_id: Option<String>,
    pub item_id: Option<String>,
    pub kind: SymbolKind,
    pub tags: Option<Vec<SymbolTag>>,
    pub name: String,
    pub detail: Option<String>,
    pub uri: URI,
    pub range: Range,
    pub selection_range: Range,
}

impl TypeHierarchyItem {
    pub fn new(kind: SymbolKind, name: &str, detail: &str, uri: URI, range: Range, selection_range: Range) -> TypeHierarchyItem {
        TypeHierarchyItem {
            session_id: None,
            item_id: None,
            kind,
            tags: None,
            name: name.to_owned(),
            detail: Some(detail.to_owned()),
            uri,
            range,
            selection_range,
        }
    }
}

// ---- hover, lens, signature, inlay hints ------------------------------------------------------------

/// `Hover`.
#[derive(Clone, Debug)]
pub struct Hover {
    pub contents: Vec<MarkedString>,
    pub range: Option<Range>,
}

impl Hover {
    pub fn new(contents: Vec<MarkedString>, range: Option<Range>) -> Hover {
        Hover { contents, range }
    }
}

/// `VerboseHover`: um [`Hover`] que sabe se aceita mais ou menos detalhe.
#[derive(Clone, Debug)]
pub struct VerboseHover {
    pub hover: Hover,
    pub can_increase_verbosity: Option<bool>,
    pub can_decrease_verbosity: Option<bool>,
}

impl VerboseHover {
    pub fn new(contents: Vec<MarkedString>, range: Option<Range>, can_increase_verbosity: Option<bool>, can_decrease_verbosity: Option<bool>) -> VerboseHover {
        VerboseHover { hover: Hover::new(contents, range), can_increase_verbosity, can_decrease_verbosity }
    }
}

/// `CodeLens`.
#[derive(Clone)]
pub struct CodeLens {
    pub range: Range,
    pub command: Option<Command>,
}

impl CodeLens {
    pub fn new(range: Range, command: Option<Command>) -> CodeLens {
        CodeLens { range, command }
    }

    pub fn is_resolved(&self) -> bool {
        self.command.is_some()
    }

    /// `JSON.stringify`: `{ range, command }`, sem os campos `undefined`.
    pub fn to_json(&self) -> Value {
        json_object(vec![("range", Some(self.range.to_json())), ("command", self.command.as_ref().map(Command::to_json))])
    }
}

/// `string | [number, number]`: o rótulo de um parâmetro de assinatura.
#[derive(Clone, Debug, PartialEq)]
pub enum ParameterLabel {
    Text(String),
    Offsets(f64, f64),
}

/// `ParameterInformation`.
#[derive(Clone, Debug)]
pub struct ParameterInformation {
    pub label: ParameterLabel,
    pub documentation: Option<Documentation>,
}

impl ParameterInformation {
    pub fn new(label: ParameterLabel, documentation: Option<Documentation>) -> ParameterInformation {
        ParameterInformation { label, documentation }
    }
}

/// `SignatureInformation`.
#[derive(Clone, Debug)]
pub struct SignatureInformation {
    pub label: String,
    pub documentation: Option<Documentation>,
    pub parameters: Vec<ParameterInformation>,
    pub active_parameter: Option<f64>,
}

impl SignatureInformation {
    pub fn new(label: impl Into<String>, documentation: Option<Documentation>) -> SignatureInformation {
        SignatureInformation { label: label.into(), documentation, parameters: Vec::new(), active_parameter: None }
    }
}

/// `SignatureHelp`.
#[derive(Clone, Debug)]
pub struct SignatureHelp {
    pub signatures: Vec<SignatureInformation>,
    pub active_signature: f64,
    pub active_parameter: f64,
}

impl Default for SignatureHelp {
    fn default() -> Self {
        SignatureHelp { signatures: Vec::new(), active_signature: 0.0, active_parameter: 0.0 }
    }
}

/// `InlayHintLabelPart`.
#[derive(Clone)]
pub struct InlayHintLabelPart {
    pub value: String,
    pub tooltip: Option<Documentation>,
    pub location: Option<Location>,
    pub command: Option<Command>,
}

impl InlayHintLabelPart {
    pub fn new(value: impl Into<String>) -> InlayHintLabelPart {
        InlayHintLabelPart { value: value.into(), tooltip: None, location: None, command: None }
    }
}

/// `string | InlayHintLabelPart[]`.
#[derive(Clone)]
pub enum InlayHintLabel {
    Text(String),
    Parts(Vec<InlayHintLabelPart>),
}

/// `InlayHint`.
#[derive(Clone)]
pub struct InlayHint {
    pub label: InlayHintLabel,
    pub tooltip: Option<Documentation>,
    pub position: Position,
    pub text_edits: Option<Vec<TextEdit>>,
    pub kind: Option<InlayHintKind>,
    pub padding_left: Option<bool>,
    pub padding_right: Option<bool>,
}

impl InlayHint {
    pub fn new(position: Position, label: InlayHintLabel, kind: Option<InlayHintKind>) -> InlayHint {
        InlayHint { label, tooltip: None, position, text_edits: None, kind, padding_left: None, padding_right: None }
    }
}

// ---- inline values, evaluatable expressions ---------------------------------------------------------

/// `EvaluatableExpression`.
#[derive(Clone, Debug, PartialEq)]
pub struct EvaluatableExpression {
    pub range: Range,
    pub expression: Option<String>,
}

impl EvaluatableExpression {
    pub fn new(range: Range, expression: Option<&str>) -> EvaluatableExpression {
        EvaluatableExpression { range, expression: expression.map(str::to_owned) }
    }
}

/// `InlineValueText`.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineValueText {
    pub range: Range,
    pub text: String,
}

impl InlineValueText {
    pub fn new(range: Range, text: impl Into<String>) -> InlineValueText {
        InlineValueText { range, text: text.into() }
    }
}

/// `InlineValueVariableLookup`.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineValueVariableLookup {
    pub range: Range,
    pub variable_name: Option<String>,
    pub case_sensitive_lookup: bool,
}

impl InlineValueVariableLookup {
    /// `new InlineValueVariableLookup(range, variableName?, caseSensitiveLookup = true)`.
    pub fn new(range: Range, variable_name: Option<&str>, case_sensitive_lookup: Option<bool>) -> InlineValueVariableLookup {
        InlineValueVariableLookup {
            range,
            variable_name: variable_name.map(str::to_owned),
            case_sensitive_lookup: case_sensitive_lookup.unwrap_or(true),
        }
    }
}

/// `InlineValueEvaluatableExpression`.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineValueEvaluatableExpression {
    pub range: Range,
    pub expression: Option<String>,
}

impl InlineValueEvaluatableExpression {
    pub fn new(range: Range, expression: Option<&str>) -> InlineValueEvaluatableExpression {
        InlineValueEvaluatableExpression { range, expression: expression.map(str::to_owned) }
    }
}

/// `InlineValueContext`.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineValueContext {
    pub frame_id: f64,
    pub stopped_location: Range,
}

impl InlineValueContext {
    pub fn new(frame_id: f64, range: Range) -> InlineValueContext {
        InlineValueContext { frame_id, stopped_location: range }
    }
}

/// `NewSymbolName`.
#[derive(Clone, Debug, PartialEq)]
pub struct NewSymbolName {
    pub new_symbol_name: String,
    pub tags: Option<Vec<super::enums::NewSymbolNameTag>>,
}

impl NewSymbolName {
    pub fn new(new_symbol_name: impl Into<String>, tags: Option<Vec<super::enums::NewSymbolNameTag>>) -> NewSymbolName {
        NewSymbolName { new_symbol_name: new_symbol_name.into(), tags }
    }
}

// ---- folding, linked editing ------------------------------------------------------------------------

/// `FoldingRange`.
#[derive(Clone, Debug, PartialEq)]
pub struct FoldingRange {
    pub start: f64,
    pub end: f64,
    pub kind: Option<FoldingRangeKind>,
}

impl FoldingRange {
    pub fn new(start: f64, end: f64, kind: Option<FoldingRangeKind>) -> FoldingRange {
        FoldingRange { start, end, kind }
    }
}

/// `LinkedEditingRanges`.
#[derive(Clone)]
pub struct LinkedEditingRanges {
    pub ranges: Vec<Range>,
    pub word_pattern: Option<Dynamic>,
}

impl LinkedEditingRanges {
    pub fn new(ranges: Vec<Range>, word_pattern: Option<Dynamic>) -> LinkedEditingRanges {
        LinkedEditingRanges { ranges, word_pattern }
    }
}
