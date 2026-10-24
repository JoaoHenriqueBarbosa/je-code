//! Porte de `src/vs/workbench/api/common/extHostTypes.ts`: as classes da API `vscode` que as extensões
//! usam. Os assuntos ficam em submódulos de `ext_host_types/`, reexportados aqui:
//!
//! - [`position_range`]: `Position`, `Range`, `Selection`, `Location`.
//! - [`edits`]: `TextEdit`, `SnippetString`, `SnippetTextEdit`, `WorkspaceEdit`, `EndOfLine`.
//! - [`notebook`]: `NotebookRange`, `NotebookCellData`, `NotebookEdit`, `NotebookCellOutput(Item)`.
//! - [`symbols`]: `Diagnostic`, `SymbolInformation`, `CodeActionKind`, `Hover`, `CodeLens`, hierarquias...
//! - [`completion`]: `CompletionItem`, `DocumentLink`, `Color`, `InlineSuggestion`...
//! - [`semantic_tokens`]: `SemanticTokensBuilder` e companhia.
//! - [`text_editor`]: as opções de decoração (`DecorationRenderOptions`...) e `TextEditorDiffInformation`.
//! - [`tree_view`]: `ViewBadge`, `TreeItem.isTreeItem` e `DataTransfer`, `DataTransferItem`, `DataTransferFile`.
//! - [`markdown_string`]: `MarkdownString`.
//! - [`language_model`]: `LanguageModel*` e `Mcp*`.
//! - [`enums`]: as enumerações soltas.
//!
//! Este arquivo tem o resto: `Disposable`, `ResolvedAuthority`, `FileDecoration`, `FileSystemError`,
//! `TreeItem`, `TerminalLink`, `ViewColumn`, `IconPath` e os tipos de apoio (`Command`, `Documentation`,
//! `MarkedString`).
//!
//! Divergências gerais:
//! - Em JavaScript uma classe é objeto com identidade e `instanceof`. Aqui cada classe é um `struct`/`enum`;
//!   onde o original distingue por `instanceof` (partes de mensagem, símbolos) há um `enum`. As
//!   verificações de tipo em tempo de execução (`typeof x !== 'string'`, `URI.isUri`, `Range.isRange`) e
//!   os `isX` com duck typing não existem: o tipo as garante.
//! - Construtores que lançam (`illegalArgument`) devolvem [`Result`]; o `Error` é o de `je-base`.
//! - Os `number` da API são [`f64`]; os `any`/`unknown`, [`Dynamic`]; `toJSON` devolve [`Value`] sem os
//!   campos `undefined`, como o `JSON.stringify` do original.
//! - `@es5ClassCompat` (compatibilidade de herança ES5) não tem equivalente nem necessidade.
//! - Ainda não portado: a família `Task*`/`ProcessExecution`/`ShellExecution`, `Breakpoint*`/`DebugAdapter*`,
//!   `DocumentDropEdit`/`DocumentPasteEdit`, `Test*`/`*Coverage`, `Tab*Input`, as peças de
//!   resposta e depuração de chat (`ChatResponse*`, `ChatDebug*`...), `RelativePattern`,
//!   `RemoteAuthorityResolverError`/`ManagedResolvedAuthority`, `CustomExecution`, `QuickInputButtons`,
//!   `PortAttributes`, `TimelineItem`, `ColorTheme`, `DebugVisualization`, `NotebookRendererScript`,
//!   `NotebookKernelSourceAction`, `NotebookCellStatusBarItem`, `CellErrorStackFrame`,
//!   `ChatSessionCustomizationType` e `TestMessage`. São dependentes de tipos de outros módulos (`vscode.*`
//!   de debug, tarefas, testes, chat) que ainda não existem no porte.

#[macro_use]
mod macros;

pub mod completion;
pub mod edits;
pub mod enums;
pub mod language_model;
pub mod markdown_string;
pub mod notebook;
pub mod position_range;
pub mod semantic_tokens;
pub mod symbols;
pub mod text_editor;
pub mod tree_view;

pub use completion::*;
pub use edits::*;
pub use enums::*;
pub use language_model::*;
pub use markdown_string::MarkdownString;
pub use notebook::*;
pub use position_range::*;
pub use semantic_tokens::*;
pub use symbols::*;
pub use text_editor::*;
pub use tree_view::{DataTransfer, DataTransferFile, DataTransferItem, ViewBadge};

use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::{Dynamic, date_to_iso_string, json_quote, number_to_js_string};
use je_base::errors::Error;
use je_base::json::{Object, Value};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::strings::next_char_length;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use je_platform::files::files::{FileSystemProviderErrorCode, create_file_system_provider_error_from_text};

pub use je_base::themables::{ThemeColor, ThemeIcon};

// ---- apoio ------------------------------------------------------------------------------------------

/// `illegalArgument(name?)`: `Error('Illegal argument: <name>')`, ou `Error('Illegal argument')`.
pub fn illegal_argument(name: Option<&str>) -> Error {
    match name {
        Some(name) => Error::new(format!("Illegal argument: {name}")),
        None => Error::new("Illegal argument"),
    }
}

/// Monta um objeto JSON na ordem dada, omitindo os campos `None` (os `undefined` de `JSON.stringify`).
pub(crate) fn json_object(entries: Vec<(&str, Option<Value>)>) -> Value {
    Value::Object(entries.into_iter().filter_map(|(key, value)| value.map(|value| (key, value))).collect::<Object>())
}

/// `URI#toJSON()` como valor JSON: os componentes não vazios mais `$mid`, `fsPath`, `external` e `_sep`.
pub fn uri_to_json(uri: &URI) -> Value {
    let state = uri.to_json();
    let components = &state.components;
    let text = |field: &Option<je_base::JsString>| field.clone().map(Value::String);
    json_object(vec![
        ("$mid", state.mid.map(|mid| Value::Number(f64::from(mid)))),
        ("fsPath", state.fs_path.clone().map(Value::String)),
        ("external", state.external.clone().map(Value::String)),
        ("path", text(&components.path)),
        ("scheme", text(&components.scheme)),
        ("authority", text(&components.authority)),
        ("query", text(&components.query)),
        ("fragment", text(&components.fragment)),
        ("_sep", state.sep.map(|sep| Value::Number(f64::from(sep)))),
    ])
}

/// `JSON.stringify(value, undefined, '\t')`.
pub fn json_stringify_tabs(value: &Value) -> String {
    fn write(value: &Value, depth: usize, out: &mut String) {
        let indent = |depth: usize, out: &mut String| out.push_str(&"\t".repeat(depth));
        match value {
            Value::Null => out.push_str("null"),
            Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
            Value::Number(number) => out.push_str(&if number.is_finite() { number_to_js_string(*number) } else { "null".to_owned() }),
            Value::String(text) => out.push_str(&json_quote(text)),
            Value::Array(items) if items.is_empty() => out.push_str("[]"),
            Value::Array(items) => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    indent(depth + 1, out);
                    write(item, depth + 1, out);
                    out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
                }
                indent(depth, out);
                out.push(']');
            }
            Value::Object(object) if object.is_empty() => out.push_str("{}"),
            Value::Object(object) => {
                out.push_str("{\n");
                let count = object.len();
                for (i, (key, item)) in object.iter().enumerate() {
                    indent(depth + 1, out);
                    out.push_str(&json_quote(key));
                    out.push_str(": ");
                    write(item, depth + 1, out);
                    out.push_str(if i + 1 < count { ",\n" } else { "\n" });
                }
                indent(depth, out);
                out.push('}');
            }
        }
    }
    let mut out = String::new();
    write(value, 0, &mut out);
    out
}

/// O `JSON.stringify` de um [`Dynamic`] como [`Value`]: `None` para o que o JSON omite (`undefined`,
/// funções) e `null` para números não finitos.
pub fn dynamic_to_json(value: &Dynamic) -> Option<Value> {
    match value {
        Dynamic::Undefined | Dynamic::Function(_) | Dynamic::BigInt(_) | Dynamic::TypedArray(_) => None,
        Dynamic::Null => Some(Value::Null),
        Dynamic::Bool(flag) => Some(Value::Bool(*flag)),
        Dynamic::Number(number) => Some(if number.is_finite() { Value::Number(*number) } else { Value::Null }),
        Dynamic::String(text) => Some(Value::String(text.clone())),
        Dynamic::Array(items) => Some(Value::Array(items.to_vec().iter().map(|item| dynamic_to_json(item).unwrap_or(Value::Null)).collect())),
        Dynamic::Object(object) => Some(Value::Object(
            object.entries().into_iter().filter_map(|(key, item)| dynamic_to_json(&item).map(|item| (key, item))).collect::<Object>(),
        )),
        Dynamic::RegExp(_) => Some(Value::Object(Object::new())),
        Dynamic::Date(date) => Some(date_to_iso_string(date.time_value()).map_or(Value::Null, |iso| Value::from(iso.as_str()))),
    }
}

/// `vscode.Command`.
#[derive(Clone, Debug)]
pub struct Command {
    pub title: String,
    pub command: String,
    pub tooltip: Option<String>,
    pub arguments: Option<Vec<Dynamic>>,
}

impl Command {
    pub fn new(title: impl Into<String>, command: impl Into<String>) -> Command {
        Command { title: title.into(), command: command.into(), tooltip: None, arguments: None }
    }

    /// `JSON.stringify(command)`.
    pub fn to_json(&self) -> Value {
        json_object(vec![
            ("title", Some(Value::from(self.title.as_str()))),
            ("command", Some(Value::from(self.command.as_str()))),
            ("tooltip", self.tooltip.as_deref().map(Value::from)),
            (
                "arguments",
                self.arguments.as_ref().map(|arguments| Value::Array(arguments.iter().map(|argument| dynamic_to_json(argument).unwrap_or(Value::Null)).collect())),
            ),
        ])
    }
}

/// `string | vscode.MarkdownString`.
#[derive(Clone, Debug)]
pub enum Documentation {
    Text(String),
    Markdown(MarkdownString),
}

/// `vscode.MarkedString`: `string | { language: string; value: string }`, ou uma `MarkdownString`.
#[derive(Clone, Debug)]
pub enum MarkedString {
    Text(String),
    Markdown(MarkdownString),
    Code { language: String, value: String },
}

// ---- ViewColumn, IconPath ---------------------------------------------------------------------------

/// `ViewColumn`: a coluna do editor na API de extensão.
///
/// Divergência: é a enumeração numérica da API (`vscode.ViewColumn`), então os valores são [`f64`] (o
/// `number` do original) e o tipo é um espaço de nomes de constantes.
pub struct ViewColumn;

impl ViewColumn {
    /// A coluna do editor ativo.
    pub const ACTIVE: f64 = -1.0;
    /// A coluna ao lado do editor ativo.
    pub const BESIDE: f64 = -2.0;
    pub const ONE: f64 = 1.0;
    pub const TWO: f64 = 2.0;
    pub const THREE: f64 = 3.0;
    pub const FOUR: f64 = 4.0;
    pub const FIVE: f64 = 5.0;
    pub const SIX: f64 = 6.0;
    pub const SEVEN: f64 = 7.0;
    pub const EIGHT: f64 = 8.0;
    pub const NINE: f64 = 9.0;
}

/// `vscode.IconPath`: `Uri | { light: Uri; dark: Uri } | ThemeIcon`.
#[derive(Clone)]
pub enum IconPath {
    Uri(URI),
    LightDark { light: URI, dark: URI },
    ThemeIcon(ThemeIcon),
}

// ---- Disposable -------------------------------------------------------------------------------------

/// `Disposable`: chama uma função uma única vez ao ser descartada.
///
/// Divergência: `new Disposable(undefined)` é [`Disposable::default`]. Se a função lançar, ela continua
/// guardada (a atribuição de `undefined` do original vem depois da chamada), então um novo `dispose` a
/// chama de novo.
#[derive(Default)]
pub struct Disposable {
    call_on_dispose: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Disposable {
    pub fn new(call_on_dispose: impl Fn() + 'static) -> Disposable {
        Disposable { call_on_dispose: RefCell::new(Some(Rc::new(call_on_dispose))) }
    }

    /// `Disposable.from(...disposables)`: descarta cada um (os ausentes são ignorados), uma única vez.
    pub fn from(disposables: Vec<Option<DisposableRef>>) -> Disposable {
        let disposables = RefCell::new(Some(disposables));
        Disposable::new(move || {
            let pending = disposables.borrow_mut().take();
            for disposable in pending.into_iter().flatten().flatten() {
                disposable.dispose();
            }
        })
    }
}

impl IDisposable for Disposable {
    fn dispose(&self) {
        let call = self.call_on_dispose.borrow().clone();
        if let Some(call) = call {
            call();
            *self.call_on_dispose.borrow_mut() = None;
        }
    }
}

// ---- ResolvedAuthority ------------------------------------------------------------------------------

/// `/^[0-9A-Za-z_\-]+$/` sobre o `connectionToken`.
fn validate_connection_token(connection_token: &str) -> Result<(), Error> {
    if connection_token.is_empty() || !connection_token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(illegal_argument(Some("connectionToken")));
    }
    Ok(())
}

/// `ResolvedAuthority`.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedAuthority {
    pub host: String,
    pub port: f64,
    pub connection_token: Option<String>,
}

impl ResolvedAuthority {
    pub fn new(host: &str, port: f64, connection_token: Option<&str>) -> Result<ResolvedAuthority, Error> {
        if host.is_empty() {
            return Err(illegal_argument(Some("host")));
        }
        if port == 0.0 || port.round() != port {
            return Err(illegal_argument(Some("port")));
        }
        if let Some(token) = connection_token {
            validate_connection_token(token)?;
        }
        Ok(ResolvedAuthority { host: host.to_owned(), port: port.round(), connection_token: connection_token.map(str::to_owned) })
    }
}

// ---- StatusBar, FileDecoration ----------------------------------------------------------------------

/// `asStatusBarItemIdentifier(extension, id)`.
pub fn as_status_bar_item_identifier(extension: &str, id: &str) -> String {
    format!("{}.{}", ExtensionIdentifier::to_key(extension), id)
}

/// `string | ThemeIcon`: o `badge` de uma decoração.
#[derive(Clone)]
pub enum FileDecorationBadge {
    Text(String),
    ThemeIcon(ThemeIcon),
}

/// `FileDecoration`.
#[derive(Clone)]
pub struct FileDecoration {
    pub badge: Option<FileDecorationBadge>,
    pub tooltip: Option<String>,
    pub color: Option<ThemeColor>,
    pub propagate: Option<bool>,
}

impl FileDecoration {
    pub fn new(badge: Option<FileDecorationBadge>, tooltip: Option<&str>, color: Option<ThemeColor>) -> FileDecoration {
        FileDecoration { badge, tooltip: tooltip.map(str::to_owned), color, propagate: None }
    }

    /// `FileDecoration.validate(d)`: o `badge` de texto cabe em até dois caracteres (grafemas); a decoração
    /// não pode ser vazia.
    pub fn validate(d: &FileDecoration) -> Result<bool, Error> {
        let has_badge = match &d.badge {
            Some(FileDecorationBadge::Text(badge)) => {
                let units: Vec<u16> = badge.encode_utf16().collect();
                let mut len = next_char_length(&units, 0);
                if len < units.len() {
                    len += next_char_length(&units, len);
                }
                if units.len() > len {
                    return Err(Error::new("The 'badge'-property must be undefined or a short character"));
                }
                !badge.is_empty()
            }
            Some(FileDecorationBadge::ThemeIcon(_)) => true,
            None => false,
        };
        if d.color.is_none() && !has_badge && d.tooltip.as_deref().is_none_or(str::is_empty) {
            return Err(Error::new("The decoration is empty"));
        }
        Ok(true)
    }
}

// ---- FileSystemError --------------------------------------------------------------------------------

/// `string | URI`: o argumento de `FileSystemError`.
#[derive(Clone, Debug)]
pub enum MessageOrUri {
    Message(String),
    Uri(URI),
}

/// `FileSystemError`: um `Error` marcado com o código do provedor de arquivos. `code` é o nome da função
/// de fábrica (`'FileNotFound'`) ou `'Unknown'`.
#[derive(Clone, Debug, PartialEq)]
pub struct FileSystemError {
    error: Error,
    pub code: String,
}

impl FileSystemError {
    pub fn file_exists(message_or_uri: Option<MessageOrUri>) -> FileSystemError {
        FileSystemError::new(message_or_uri, FileSystemProviderErrorCode::FileExists, Some("FileExists"))
    }

    pub fn file_not_found(message_or_uri: Option<MessageOrUri>) -> FileSystemError {
        FileSystemError::new(message_or_uri, FileSystemProviderErrorCode::FileNotFound, Some("FileNotFound"))
    }

    pub fn file_not_a_directory(message_or_uri: Option<MessageOrUri>) -> FileSystemError {
        FileSystemError::new(message_or_uri, FileSystemProviderErrorCode::FileNotADirectory, Some("FileNotADirectory"))
    }

    pub fn file_is_a_directory(message_or_uri: Option<MessageOrUri>) -> FileSystemError {
        FileSystemError::new(message_or_uri, FileSystemProviderErrorCode::FileIsADirectory, Some("FileIsADirectory"))
    }

    pub fn no_permissions(message_or_uri: Option<MessageOrUri>) -> FileSystemError {
        FileSystemError::new(message_or_uri, FileSystemProviderErrorCode::NoPermissions, Some("NoPermissions"))
    }

    pub fn unavailable(message_or_uri: Option<MessageOrUri>) -> FileSystemError {
        FileSystemError::new(message_or_uri, FileSystemProviderErrorCode::Unavailable, Some("Unavailable"))
    }

    /// `new FileSystemError(uriOrMessage?, code = Unknown, terminator?)`; `terminator` é o nome da fábrica.
    pub fn new(uri_or_message: Option<MessageOrUri>, code: FileSystemProviderErrorCode, terminator: Option<&str>) -> FileSystemError {
        let message = match uri_or_message {
            Some(MessageOrUri::Message(message)) => message,
            Some(MessageOrUri::Uri(uri)) => uri.to_string(true).to_string_lossy(),
            None => String::new(),
        };
        // marca o erro como erro do provedor de arquivos, para extrair o código do outro lado
        FileSystemError { error: create_file_system_provider_error_from_text(&message, code), code: terminator.unwrap_or("Unknown").to_owned() }
    }

    /// O `Error` que a classe estende (`name`, `message`).
    pub fn error(&self) -> &Error {
        &self.error
    }
}

impl std::fmt::Display for FileSystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl std::error::Error for FileSystemError {}

// ---- TreeItem, TerminalLink -------------------------------------------------------------------------

/// `vscode.TreeItemLabel`.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeItemLabel {
    pub label: String,
    pub highlights: Option<Vec<(f64, f64)>>,
}

/// `string | TreeItemLabel`.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeItemLabelValue {
    Text(String),
    Label(TreeItemLabel),
}

/// `string | URI`.
#[derive(Clone)]
pub enum PathOrUri {
    Path(String),
    Uri(URI),
}

/// `TreeItem#iconPath`: `string | URI | { light; dark } | ThemeIcon`.
#[derive(Clone)]
pub enum TreeItemIconPath {
    Path(String),
    Uri(URI),
    LightDark { light: PathOrUri, dark: PathOrUri },
    ThemeIcon(ThemeIcon),
}

/// `TreeItem#checkboxState`: `TreeItemCheckboxState | { state; tooltip? }`.
#[derive(Clone, Debug, PartialEq)]
pub enum TreeItemCheckboxStateValue {
    State(TreeItemCheckboxState),
    WithTooltip { state: TreeItemCheckboxState, tooltip: Option<String> },
}

/// `TreeItem`.
#[derive(Clone)]
pub struct TreeItem {
    pub label: Option<TreeItemLabelValue>,
    pub resource_uri: Option<URI>,
    pub icon_path: Option<TreeItemIconPath>,
    pub command: Option<Command>,
    pub context_value: Option<String>,
    pub tooltip: Option<Documentation>,
    pub checkbox_state: Option<TreeItemCheckboxStateValue>,
    pub collapsible_state: TreeItemCollapsibleState,
}

impl TreeItem {
    /// `new TreeItem(label, collapsibleState = None)`.
    pub fn new(label: TreeItemLabelValue, collapsible_state: Option<TreeItemCollapsibleState>) -> TreeItem {
        TreeItem { label: Some(label), ..TreeItem::empty(collapsible_state) }
    }

    /// `new TreeItem(resourceUri, collapsibleState = None)`.
    pub fn from_resource_uri(resource_uri: URI, collapsible_state: Option<TreeItemCollapsibleState>) -> TreeItem {
        TreeItem { resource_uri: Some(resource_uri), ..TreeItem::empty(collapsible_state) }
    }

    fn empty(collapsible_state: Option<TreeItemCollapsibleState>) -> TreeItem {
        TreeItem {
            label: None,
            resource_uri: None,
            icon_path: None,
            command: None,
            context_value: None,
            tooltip: None,
            checkbox_state: None,
            collapsible_state: collapsible_state.unwrap_or(TreeItemCollapsibleState::None),
        }
    }
}

/// `TerminalLink`.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalLink {
    pub start_index: f64,
    pub length: f64,
    pub tooltip: Option<String>,
}

impl TerminalLink {
    /// `new TerminalLink(startIndex, length, tooltip?)`.
    pub fn new(start_index: f64, length: f64, tooltip: Option<&str>) -> Result<TerminalLink, Error> {
        if start_index < 0.0 || start_index.is_nan() {
            return Err(illegal_argument(Some("startIndex")));
        }
        if length < 1.0 || length.is_nan() {
            return Err(illegal_argument(Some("length")));
        }
        Ok(TerminalLink { start_index, length, tooltip: tooltip.map(str::to_owned) })
    }
}
