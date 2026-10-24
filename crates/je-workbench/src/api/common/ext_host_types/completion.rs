//! Completar, links de documento e cores de `extHostTypes.ts`: `CompletionItem`, `CompletionList`,
//! `InlineSuggestion`, `DocumentLink`, `Color`, `ColorInformation`, `ColorPresentation`,
//! `TerminalCompletionItem`.
//!
//! Divergências:
//! - `string | CompletionItemLabel` é [`CompletionLabel`]; `string | SnippetString` é [`InsertText`];
//!   `Range | { inserting; replacing }` é [`CompletionRange`].
//! - `CompletionItem#toJSON`: `kind && CompletionItemKind[kind]` é `undefined` para `Text` (valor `0`,
//!   falso em JavaScript), então `Text` não aparece no JSON, como no original. Uma `MarkdownString` em
//!   `documentation` serializa como `{}` (o estado dela é um campo privado `#delegate`), e um
//!   `SnippetString` em `insertText` como `{ _tabstop, value }`.
//! - `new DocumentLink(range, target)` e `new ColorInformation(range, color)` lançam com `Range` vazio:
//!   devolvem [`Result`]. `color instanceof Color` e `URI.isUri(target)` são garantidos pelo tipo.
//! - `Color` guarda os quatro canais como [`f64`] (o `number` do original).

use je_base::errors::Error;
use je_base::json::Value;
use je_base::uri::URI;

use super::edits::{SnippetString, TextEdit};
use super::enums::{CompletionItemKind, CompletionItemTag, CompletionTriggerKind, PartialAcceptTriggerKind, TerminalCompletionItemKind};
use super::position_range::Range;
use super::{Command, Documentation, ThemeIcon, illegal_argument, json_object};

/// `CompletionContext`.
#[derive(Clone, Debug, PartialEq)]
pub struct CompletionContext {
    pub trigger_kind: CompletionTriggerKind,
    pub trigger_character: Option<String>,
}

/// `CompletionItemLabel`.
#[derive(Clone, Debug, PartialEq)]
pub struct CompletionItemLabel {
    pub label: String,
    pub detail: Option<String>,
    pub description: Option<String>,
}

impl CompletionItemLabel {
    fn to_json(&self) -> Value {
        json_object(vec![
            ("label", Some(Value::from(self.label.as_str()))),
            ("detail", self.detail.as_deref().map(Value::from)),
            ("description", self.description.as_deref().map(Value::from)),
        ])
    }
}

/// `string | CompletionItemLabel`.
#[derive(Clone, Debug, PartialEq)]
pub enum CompletionLabel {
    Text(String),
    Detailed(CompletionItemLabel),
}

impl CompletionLabel {
    fn to_json(&self) -> Value {
        match self {
            CompletionLabel::Text(text) => Value::from(text.as_str()),
            CompletionLabel::Detailed(label) => label.to_json(),
        }
    }
}

/// `string | SnippetString`.
#[derive(Clone, Debug, PartialEq)]
pub enum InsertText {
    Text(String),
    Snippet(SnippetString),
}

/// `Range | { inserting: Range; replacing: Range }`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CompletionRange {
    Range(Range),
    InsertReplace { inserting: Range, replacing: Range },
}

/// `CompletionItem`.
#[derive(Clone)]
pub struct CompletionItem {
    pub label: CompletionLabel,
    pub kind: Option<CompletionItemKind>,
    pub tags: Option<Vec<CompletionItemTag>>,
    pub detail: Option<String>,
    pub documentation: Option<Documentation>,
    pub sort_text: Option<String>,
    pub filter_text: Option<String>,
    pub preselect: Option<bool>,
    pub insert_text: Option<InsertText>,
    pub keep_whitespace: Option<bool>,
    pub range: Option<CompletionRange>,
    pub commit_characters: Option<Vec<String>>,
    pub text_edit: Option<TextEdit>,
    pub additional_text_edits: Option<Vec<TextEdit>>,
    pub command: Option<Command>,
}

impl CompletionItem {
    pub fn new(label: CompletionLabel, kind: Option<CompletionItemKind>) -> CompletionItem {
        CompletionItem {
            label,
            kind,
            tags: None,
            detail: None,
            documentation: None,
            sort_text: None,
            filter_text: None,
            preselect: None,
            insert_text: None,
            keep_whitespace: None,
            range: None,
            commit_characters: None,
            text_edit: None,
            additional_text_edits: None,
            command: None,
        }
    }

    /// `toJSON()`.
    pub fn to_json(&self) -> Value {
        json_object(vec![
            ("label", Some(self.label.to_json())),
            // `this.kind && CompletionItemKind[this.kind]`: `Text` vale `0`, falso
            ("kind", self.kind.filter(|kind| kind.value() != 0).map(|kind| Value::from(kind.name()))),
            ("detail", self.detail.as_deref().map(Value::from)),
            (
                "documentation",
                self.documentation.as_ref().map(|documentation| match documentation {
                    Documentation::Text(text) => Value::from(text.as_str()),
                    Documentation::Markdown(_) => Value::Object(Default::default()),
                }),
            ),
            ("sortText", self.sort_text.as_deref().map(Value::from)),
            ("filterText", self.filter_text.as_deref().map(Value::from)),
            ("preselect", self.preselect.map(Value::from)),
            (
                "insertText",
                self.insert_text.as_ref().map(|insert_text| match insert_text {
                    InsertText::Text(text) => Value::from(text.as_str()),
                    InsertText::Snippet(snippet) => json_object(vec![
                        ("_tabstop", Some(Value::Number(1.0))),
                        ("value", Some(Value::from(snippet.value.as_str()))),
                    ]),
                }),
            ),
            ("textEdit", self.text_edit.as_ref().map(TextEdit::to_json)),
        ])
    }
}

/// `CompletionList`.
#[derive(Clone)]
pub struct CompletionList {
    pub is_incomplete: Option<bool>,
    pub items: Vec<CompletionItem>,
}

impl CompletionList {
    /// `new CompletionList(items = [], isIncomplete = false)`.
    pub fn new(items: Option<Vec<CompletionItem>>, is_incomplete: Option<bool>) -> CompletionList {
        CompletionList { items: items.unwrap_or_default(), is_incomplete: Some(is_incomplete.unwrap_or(false)) }
    }
}

/// `InlineSuggestion` (`vscode.InlineCompletionItem`).
#[derive(Clone)]
pub struct InlineSuggestion {
    pub filter_text: Option<String>,
    pub insert_text: String,
    pub range: Option<Range>,
    pub command: Option<Command>,
}

impl InlineSuggestion {
    pub fn new(insert_text: impl Into<String>, range: Option<Range>, command: Option<Command>) -> InlineSuggestion {
        InlineSuggestion { filter_text: None, insert_text: insert_text.into(), range, command }
    }
}

/// `Command | { command: Command; icon: ThemeIcon }`: um item de `InlineSuggestionList.commands`.
#[derive(Clone)]
pub enum InlineSuggestionCommand {
    Command(Command),
    WithIcon { command: Command, icon: ThemeIcon },
}

/// `InlineSuggestionList` (`vscode.InlineCompletionList`).
#[derive(Clone)]
pub struct InlineSuggestionList {
    pub items: Vec<InlineSuggestion>,
    pub commands: Option<Vec<InlineSuggestionCommand>>,
    pub suppress_suggestions: Option<bool>,
}

impl InlineSuggestionList {
    pub fn new(items: Vec<InlineSuggestion>) -> InlineSuggestionList {
        InlineSuggestionList { items, commands: None, suppress_suggestions: None }
    }
}

/// `PartialAcceptInfo`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartialAcceptInfo {
    pub kind: PartialAcceptTriggerKind,
    pub accepted_length: f64,
}

/// `DocumentLink`.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentLink {
    pub range: Range,
    pub target: Option<URI>,
    pub tooltip: Option<String>,
}

impl DocumentLink {
    /// `new DocumentLink(range, target)`: lança com `range` vazio.
    pub fn new(range: Range, target: Option<URI>) -> Result<DocumentLink, Error> {
        if range.is_empty() {
            return Err(illegal_argument(Some("range")));
        }
        Ok(DocumentLink { range, target, tooltip: None })
    }
}

/// `Color`: canais de `0` a `1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

impl Color {
    pub fn new(red: f64, green: f64, blue: f64, alpha: f64) -> Color {
        Color { red, green, blue, alpha }
    }
}

/// `ColorInformation`.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorInformation {
    pub range: Range,
    pub color: Color,
}

impl ColorInformation {
    /// `new ColorInformation(range, color)`: lança com `range` vazio.
    pub fn new(range: Range, color: Color) -> Result<ColorInformation, Error> {
        if range.is_empty() {
            return Err(illegal_argument(Some("range")));
        }
        Ok(ColorInformation { range, color })
    }
}

/// `ColorPresentation`.
#[derive(Clone, Debug, PartialEq)]
pub struct ColorPresentation {
    pub label: String,
    pub text_edit: Option<TextEdit>,
    pub additional_text_edits: Option<Vec<TextEdit>>,
}

impl ColorPresentation {
    /// `new ColorPresentation(label)`: lança com `label` vazio.
    pub fn new(label: &str) -> Result<ColorPresentation, Error> {
        if label.is_empty() {
            return Err(illegal_argument(Some("label")));
        }
        Ok(ColorPresentation { label: label.to_owned(), text_edit: None, additional_text_edits: None })
    }
}

/// `TerminalCompletionItem`.
#[derive(Clone, Debug)]
pub struct TerminalCompletionItem {
    pub label: CompletionLabel,
    pub replacement_range: (f64, f64),
    pub detail: Option<String>,
    pub documentation: Option<Documentation>,
    pub kind: Option<TerminalCompletionItemKind>,
    pub is_file: Option<bool>,
    pub is_directory: Option<bool>,
    pub is_keyword: Option<bool>,
}

impl TerminalCompletionItem {
    pub fn new(label: CompletionLabel, replacement_range: (f64, f64), kind: Option<TerminalCompletionItemKind>) -> TerminalCompletionItem {
        TerminalCompletionItem {
            label,
            replacement_range,
            detail: None,
            documentation: None,
            kind,
            is_file: None,
            is_directory: None,
            is_keyword: None,
        }
    }
}

/// `TerminalCompletionResourceOptions`.
#[derive(Clone, Debug, Default)]
pub struct TerminalCompletionResourceOptions {
    pub show_files: Option<bool>,
    pub show_directories: Option<bool>,
    pub file_extensions: Option<Vec<String>>,
    pub cwd: Option<URI>,
}

/// `TerminalCompletionList`.
#[derive(Clone, Debug)]
pub struct TerminalCompletionList {
    pub resource_options: Option<TerminalCompletionResourceOptions>,
    pub items: Vec<TerminalCompletionItem>,
}

impl TerminalCompletionList {
    pub fn new(items: Option<Vec<TerminalCompletionItem>>, resource_options: Option<TerminalCompletionResourceOptions>) -> TerminalCompletionList {
        TerminalCompletionList { items: items.unwrap_or_default(), resource_options }
    }
}
