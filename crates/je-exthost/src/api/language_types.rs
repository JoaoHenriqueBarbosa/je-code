//! As classes de valor de `vscode.languages` e a leitura do que os provedores devolvem: `Location`, `Diagnostic`,
//! `DiagnosticRelatedInformation`, `TextEdit`, `MarkdownString`, `SnippetString`, `CompletionItem`, `CompletionList`, `Hover`,
//! `CodeAction`, `DocumentSymbol` e `CodeActionKind`, mais os leitores que transformam o valor do motor (copiado) nos tipos de
//! `ext_host_types` que os adaptadores do `ExtHostLanguageFeatures` esperam.
//!
//! Os leitores olham a FORMA do valor (os campos), não a marca `$$class`: um literal `{ label: 'x' }` e um `new CompletionItem('x')` valem
//! o mesmo, como no original (que só usa `instanceof` onde há ambiguidade).
//!
//! Divergências:
//! - O motor entrega o objeto comum por cópia, então um método de classe nunca altera o receptor. Por isso os métodos que MUDAM o
//!   objeto em `MarkdownString` (`appendText`, `appendMarkdown`, `appendCodeblock`), em `SnippetString` (`appendText`,
//!   `appendTabstop`, `appendPlaceholder`, `appendChoice`, `appendVariable`) e a classe `WorkspaceEdit` não existem: chamar um deles
//!   lança `TypeError` em vez de perder a escrita em silêncio. O construtor e as propriedades (`value`, `isTrusted`,
//!   `supportThemeIcons`, `supportHtml`, `baseUri`) funcionam, e `md.value = ...` também.
//! - `CodeActionKind` é um objeto com os membros estáticos (`QuickFix`, `Refactor`, `SourceFixAll`...) cujas instâncias têm `append`,
//!   `contains` e `intersects` (nenhum muda o receptor, como no original); `new CodeActionKind(value)` não existe, porque uma
//!   classe nativa só carrega funções estáticas e os membros estáticos são valores.
//! - `CodeAction#edit` (um `WorkspaceEdit`) é lido da tabela de edições (`workspace_edit`): só vale um `WorkspaceEdit` criado por `new
//!   vscode.WorkspaceEdit()`, qualquer outro valor rejeita a promessa com `CodeAction.edit must be a WorkspaceEdit`.
//! - `Diagnostic#code` como `{ value, target }` vale só o `value` (o `DiagnosticCode` de `ext_host_types` é texto ou número).

use std::cell::RefCell;
use std::rc::Rc;

use je_base::errors::Error;
use je_workbench::api::common::ext_host_language_features_api::{
    ApiCodeActionRef, ApiCompletionList, ApiCompletionResult, ApiDocumentSymbols, ApiLocationResult, CommandOrCodeAction, CompletionItemRef, DefinitionLink, LocationOrLink,
};
use je_workbench::api::common::ext_host_types::{
    CodeAction, CodeActionDisabled, CodeActionKind, Command, CompletionItem, CompletionItemKind, CompletionItemLabel, CompletionItemTag, CompletionLabel, CompletionRange,
    Diagnostic, DiagnosticCode, DiagnosticRelatedInformation, DiagnosticSeverity, DiagnosticTag, Documentation, DocumentSymbol, EndOfLine, InsertText, Location,
    MarkdownString, MarkedString, Position, Range, RangeOrPosition, SnippetString, SymbolInformation, SymbolKind, SymbolTag, TextEdit, VerboseHover,
};
use je_workbench::api::common::ext_host_values::ApiValue;
use zjsc::api::embed_node::{ClassSpec, JsData, MethodCallback, NativeCallback, NodeRunner, CLASS_KEY};

use crate::api::types::{range_data, to_position, to_range};
use crate::api::workspace_edit::WorkspaceEditObjects;
use crate::convert::{api_to_js, field, is_nullish, js_to_dynamic, opt_bool, opt_number, opt_string, string_arg, uri_from_js};

fn error(message: impl Into<String>) -> Error {
    Error::new(message.into())
}

pub(crate) fn range_field(value: &JsData, name: &str) -> Result<Range, Error> {
    let range = field(value, name).ok_or_else(|| error(format!("{name} must be a Range")))?;
    to_range(range).map_err(error)
}

fn optional_range_field(value: &JsData, name: &str) -> Result<Option<Range>, Error> {
    match field(value, name) {
        Some(range @ JsData::Object(_)) => Ok(Some(to_range(range).map_err(error)?)),
        _ => Ok(None),
    }
}

fn uri_field(value: &JsData, name: &str) -> Result<je_base::uri::URI, Error> {
    let uri = field(value, name).ok_or_else(|| error(format!("{name} must be a Uri")))?;
    uri_from_js(uri).map_err(error)
}

pub(crate) fn strings_of(value: Option<&JsData>) -> Option<Vec<String>> {
    match value {
        Some(JsData::Array(items)) => Some(items.iter().filter_map(|item| opt_string(Some(item))).collect()),
        _ => None,
    }
}

fn numbers_of(value: Option<&JsData>) -> Option<Vec<i32>> {
    match value {
        Some(JsData::Array(items)) => Some(items.iter().filter_map(|item| opt_number(Some(item)).map(|number| number as i32)).collect()),
        _ => None,
    }
}

// --- leitores

/// A `MarkdownString` de um objeto com `value` (o `ApiValue` faz as validações do original).
pub fn markdown_from_js(value: &JsData) -> Result<MarkdownString, Error> {
    let JsData::Object(fields) = value else { return Err(error("MarkdownString must be an object")) };
    let mut fields: Vec<(String, JsData)> = fields.iter().filter(|(key, _)| key != CLASS_KEY).cloned().collect();
    fields.push((CLASS_KEY.to_owned(), JsData::Str("MarkdownString".to_owned())));
    MarkdownString::from_dynamic(&js_to_dynamic(&JsData::Object(fields)))
}

pub(crate) fn documentation_from_js(value: Option<&JsData>) -> Result<Option<Documentation>, Error> {
    match value {
        Some(JsData::Str(text)) => Ok(Some(Documentation::Text(text.clone()))),
        Some(markdown @ JsData::Object(_)) => Ok(Some(Documentation::Markdown(markdown_from_js(markdown)?))),
        _ => Ok(None),
    }
}

/// `Location`: `{ uri, range }`.
pub fn location_from_js(value: &JsData) -> Result<Location, Error> {
    Ok(Location::new(Some(uri_field(value, "uri")?), Some(RangeOrPosition::Range(range_field(value, "range")?))))
}

fn related_information_from_js(value: &JsData) -> Result<DiagnosticRelatedInformation, Error> {
    let location = field(value, "location").ok_or_else(|| error("DiagnosticRelatedInformation.location must be a Location"))?;
    Ok(DiagnosticRelatedInformation::new(location_from_js(location)?, opt_string(field(value, "message")).unwrap_or_default()))
}

/// `Diagnostic`: `range`, `message` (não vazia), `severity` (padrão `Error`), `source`, `code`, `relatedInformation` e `tags`.
pub fn diagnostic_from_js(value: &JsData) -> Result<Diagnostic, Error> {
    let range = range_field(value, "range")?;
    let message = opt_string(field(value, "message")).unwrap_or_default();
    let severity = match opt_number(field(value, "severity")) {
        Some(number) => DiagnosticSeverity::from_value(number as i32).ok_or_else(|| Error::type_error("Diagnostic.severity must be a DiagnosticSeverity"))?,
        None => DiagnosticSeverity::Error,
    };
    let mut diagnostic = Diagnostic::new(range, &message, Some(severity))?;
    diagnostic.source = opt_string(field(value, "source"));
    let code_of = |code: Option<&JsData>| match code {
        Some(JsData::Str(text)) => Some(DiagnosticCode::Text(text.clone())),
        Some(JsData::Num(number)) => Some(DiagnosticCode::Number(*number)),
        _ => None,
    };
    diagnostic.code = match field(value, "code") {
        Some(object @ JsData::Object(_)) => code_of(field(object, "value")),
        other => code_of(other),
    };
    diagnostic.related_information = match field(value, "relatedInformation") {
        Some(JsData::Array(items)) => Some(items.iter().map(related_information_from_js).collect::<Result<Vec<_>, _>>()?),
        _ => None,
    };
    diagnostic.tags = numbers_of(field(value, "tags")).map(|tags| tags.into_iter().filter_map(DiagnosticTag::from_value).collect());
    Ok(diagnostic)
}

/// A lista de `set`: um elemento ausente (`undefined`, `null`) é o buraco que `coalesce` remove.
pub fn diagnostics_from_js(items: &[JsData]) -> Result<Vec<Option<Diagnostic>>, Error> {
    items.iter().map(|item| if is_nullish(Some(item)) { Ok(None) } else { diagnostic_from_js(item).map(Some) }).collect()
}

/// `TextEdit`: `range`, `newText` e `newEol`.
pub fn text_edit_from_js(value: &JsData) -> Result<TextEdit, Error> {
    let range = range_field(value, "range")?;
    let mut edit = TextEdit::new(range, opt_string(field(value, "newText")).as_deref());
    if let Some(eol) = opt_number(field(value, "newEol")) {
        edit.set_new_eol(Some(EndOfLine::from_value(eol as i32).ok_or_else(|| Error::type_error("TextEdit.newEol must be an EndOfLine"))?));
    }
    Ok(edit)
}

/// `TextEdit[]`; qualquer outro valor (`undefined`, `null`) é "sem edições".
pub fn text_edits_from_js(value: &JsData) -> Result<Option<Vec<TextEdit>>, Error> {
    match value {
        JsData::Array(items) => Ok(Some(items.iter().map(text_edit_from_js).collect::<Result<Vec<_>, _>>()?)),
        _ => Ok(None),
    }
}

pub fn command_from_js(value: &JsData) -> Result<Command, Error> {
    Command::from_dynamic(&js_to_dynamic(value))
}

fn completion_label_from_js(value: Option<&JsData>) -> Result<CompletionLabel, Error> {
    match value {
        Some(JsData::Str(text)) => Ok(CompletionLabel::Text(text.clone())),
        Some(label @ JsData::Object(_)) => Ok(CompletionLabel::Detailed(CompletionItemLabel {
            label: opt_string(field(label, "label")).ok_or_else(|| error("CompletionItem.label.label must be a string"))?,
            detail: opt_string(field(label, "detail")),
            description: opt_string(field(label, "description")),
        })),
        _ => Err(error("CompletionItem.label must be a string or an object with a label")),
    }
}

/// `CompletionItem`: todos os campos do `vscode.d.ts`.
pub fn completion_item_from_js(value: &JsData) -> Result<CompletionItem, Error> {
    let kind = opt_number(field(value, "kind")).and_then(|kind| CompletionItemKind::from_value(kind as i32));
    let mut item = CompletionItem::new(completion_label_from_js(field(value, "label"))?, kind);
    item.tags = numbers_of(field(value, "tags")).map(|tags| tags.into_iter().filter_map(CompletionItemTag::from_value).collect());
    item.detail = opt_string(field(value, "detail"));
    item.documentation = documentation_from_js(field(value, "documentation"))?;
    item.sort_text = opt_string(field(value, "sortText"));
    item.filter_text = opt_string(field(value, "filterText"));
    item.preselect = opt_bool(field(value, "preselect"));
    item.insert_text = match field(value, "insertText") {
        Some(JsData::Str(text)) => Some(InsertText::Text(text.clone())),
        Some(snippet @ JsData::Object(_)) => Some(InsertText::Snippet(SnippetString::new(opt_string(field(snippet, "value")).as_deref()))),
        _ => None,
    };
    item.keep_whitespace = opt_bool(field(value, "keepWhitespace"));
    item.range = match field(value, "range") {
        Some(range @ JsData::Object(_)) if field(range, "start").is_some() => Some(CompletionRange::Range(to_range(range).map_err(error)?)),
        Some(range @ JsData::Object(_)) if field(range, "inserting").is_some() => {
            Some(CompletionRange::InsertReplace { inserting: range_field(range, "inserting")?, replacing: range_field(range, "replacing")? })
        }
        _ => None,
    };
    item.commit_characters = strings_of(field(value, "commitCharacters"));
    item.text_edit = match field(value, "textEdit") {
        Some(edit @ JsData::Object(_)) => Some(text_edit_from_js(edit)?),
        _ => None,
    };
    item.additional_text_edits = match field(value, "additionalTextEdits") {
        Some(edits) => text_edits_from_js(edits)?,
        None => None,
    };
    item.command = match field(value, "command") {
        Some(command @ JsData::Object(_)) => Some(command_from_js(command)?),
        _ => None,
    };
    Ok(item)
}

/// Os itens que `completion_result_from_js` leu de `value`, como o motor os entregou (a lista direta ou o `items` de uma `CompletionList`),
/// na ordem dos [`CompletionItemRef`] do resultado.
pub(crate) fn completion_raw_items(value: &JsData) -> &[JsData] {
    match value {
        JsData::Array(items) => items,
        _ => match field(value, "items") {
            Some(JsData::Array(items)) => items,
            _ => &[],
        },
    }
}

fn completion_items(items: &[JsData]) -> Result<Vec<CompletionItemRef>, Error> {
    items.iter().map(|item| completion_item_from_js(item).map(|item| Rc::new(RefCell::new(item)))).collect()
}

/// `CompletionItem | undefined | null` (o resultado de `resolveCompletionItem`).
pub fn completion_item_ref_from_js(value: &JsData) -> Result<Option<CompletionItemRef>, Error> {
    if is_nullish(Some(value)) {
        return Ok(None);
    }
    completion_item_from_js(value).map(|item| Some(Rc::new(RefCell::new(item))))
}

/// `CompletionItem[] | CompletionList | undefined | null`.
pub fn completion_result_from_js(value: &JsData) -> Result<Option<ApiCompletionResult>, Error> {
    match value {
        JsData::Array(items) => Ok(Some(ApiCompletionResult::Items(completion_items(items)?))),
        JsData::Object(_) => match field(value, "items") {
            Some(JsData::Array(items)) => {
                Ok(Some(ApiCompletionResult::List(ApiCompletionList { is_incomplete: opt_bool(field(value, "isIncomplete")), items: completion_items(items)? })))
            }
            _ => Err(error("CompletionList.items must be an array")),
        },
        _ => Ok(None),
    }
}

fn marked_string_from_js(value: &JsData) -> Result<MarkedString, Error> {
    match value {
        JsData::Str(text) => Ok(MarkedString::Text(text.clone())),
        JsData::Object(_) => match (opt_string(field(value, "language")), opt_string(field(value, "value"))) {
            (Some(language), Some(code)) => Ok(MarkedString::Code { language, value: code }),
            (None, Some(_)) => Ok(MarkedString::Markdown(markdown_from_js(value)?)),
            _ => Err(error("Hover.contents must hold strings, code blocks or MarkdownString")),
        },
        _ => Err(error("Hover.contents must hold strings, code blocks or MarkdownString")),
    }
}

/// `Hover | undefined | null`.
pub fn hover_from_js(value: &JsData) -> Result<Option<VerboseHover>, Error> {
    if is_nullish(Some(value)) {
        return Ok(None);
    }
    let contents = match field(value, "contents") {
        Some(JsData::Array(items)) => items.iter().map(marked_string_from_js).collect::<Result<Vec<_>, _>>()?,
        Some(single) if !is_nullish(Some(single)) => vec![marked_string_from_js(single)?],
        _ => return Err(error("Illegal argument, contents must be defined")),
    };
    Ok(Some(VerboseHover::new(
        contents,
        optional_range_field(value, "range")?,
        opt_bool(field(value, "canIncreaseVerbosity")),
        opt_bool(field(value, "canDecreaseVerbosity")),
    )))
}

fn location_or_link_from_js(value: &JsData) -> Result<LocationOrLink, Error> {
    if field(value, "targetUri").is_some() {
        return Ok(LocationOrLink::Link(DefinitionLink {
            origin_selection_range: optional_range_field(value, "originSelectionRange")?,
            target_uri: uri_field(value, "targetUri")?,
            target_range: range_field(value, "targetRange")?,
            target_selection_range: optional_range_field(value, "targetSelectionRange")?,
        }));
    }
    Ok(LocationOrLink::Location(location_from_js(value)?))
}

/// `Location | Location[] | LocationLink[] | undefined | null`.
pub fn location_result_from_js(value: &JsData) -> Result<Option<ApiLocationResult>, Error> {
    match value {
        JsData::Array(items) => Ok(Some(ApiLocationResult::Many(items.iter().map(location_or_link_from_js).collect::<Result<Vec<_>, _>>()?))),
        JsData::Object(_) => Ok(Some(ApiLocationResult::Single(location_or_link_from_js(value)?))),
        _ => Ok(None),
    }
}

/// O `CodeActionKind` de `{ value }` ou de um texto.
pub fn code_action_kind_from_js(value: &JsData) -> Option<CodeActionKind> {
    match value {
        JsData::Str(text) => Some(CodeActionKind::new(text.clone())),
        JsData::Object(_) => opt_string(field(value, "value")).map(CodeActionKind::new),
        _ => None,
    }
}

/// O `CodeActionKind` como o objeto que a extensão lê.
pub fn code_action_kind_data(kind: &CodeActionKind) -> JsData {
    JsData::Object(vec![(CLASS_KEY.to_owned(), JsData::Str("CodeActionKind".to_owned())), ("value".to_owned(), JsData::Str(kind.value.clone()))])
}

/// `Command` é distinguido de `CodeAction` pelo `command` texto (o `_isCommand` do adaptador).
fn code_action_entry_from_js(value: &JsData, edits: &WorkspaceEditObjects) -> Result<CommandOrCodeAction, Error> {
    if matches!(field(value, "command"), Some(JsData::Str(_))) {
        return command_from_js(value).map(CommandOrCodeAction::Command);
    }
    let title = opt_string(field(value, "title")).ok_or_else(|| error("CodeAction.title must be a string"))?;
    let mut action = CodeAction::new(title, field(value, "kind").and_then(code_action_kind_from_js));
    action.edit = match field(value, "edit") {
        Some(edit) if !is_nullish(Some(edit)) => Some(edits.resolved_edit(edit).ok_or_else(|| error("CodeAction.edit must be a WorkspaceEdit"))?),
        _ => None,
    };
    action.command = match field(value, "command") {
        Some(command @ JsData::Object(_)) => Some(command_from_js(command)?),
        _ => None,
    };
    action.diagnostics = match field(value, "diagnostics") {
        Some(JsData::Array(items)) => Some(items.iter().map(diagnostic_from_js).collect::<Result<Vec<_>, _>>()?),
        _ => None,
    };
    action.is_preferred = opt_bool(field(value, "isPreferred"));
    action.disabled = match field(value, "disabled") {
        Some(disabled @ JsData::Object(_)) => opt_string(field(disabled, "reason")).map(|reason| CodeActionDisabled { reason }),
        _ => None,
    };
    Ok(CommandOrCodeAction::CodeAction(Rc::new(RefCell::new(action))))
}

/// `CodeAction | undefined | null` (o resultado de `resolveCodeAction`); um `Command` não serve.
pub fn code_action_ref_from_js(value: &JsData, edits: &WorkspaceEditObjects) -> Result<Option<ApiCodeActionRef>, Error> {
    if is_nullish(Some(value)) {
        return Ok(None);
    }
    match code_action_entry_from_js(value, edits)? {
        CommandOrCodeAction::CodeAction(action) => Ok(Some(action)),
        CommandOrCodeAction::Command(_) => Err(error("resolveCodeAction must return a CodeAction")),
    }
}

/// `(Command | CodeAction)[] | undefined | null`; os elementos ausentes são descartados (`if (!candidate) continue`).
pub fn code_actions_from_js(value: &JsData, edits: &WorkspaceEditObjects) -> Result<Option<Vec<CommandOrCodeAction>>, Error> {
    match value {
        JsData::Array(entries) => Ok(Some(code_action_raw_entries(entries).map(|entry| code_action_entry_from_js(entry, edits)).collect::<Result<Vec<_>, _>>()?)),
        _ => Ok(None),
    }
}

/// Os elementos de `entries` que `code_actions_from_js` leu (os ausentes ficam de fora), na ordem do resultado.
pub(crate) fn code_action_raw_entries(entries: &[JsData]) -> impl Iterator<Item = &JsData> {
    entries.iter().filter(|entry| !is_nullish(Some(*entry)))
}

fn symbol_kind_of(value: &JsData) -> Result<SymbolKind, Error> {
    opt_number(field(value, "kind"))
        .and_then(|kind| SymbolKind::from_value(kind as i32))
        .ok_or_else(|| Error::type_error("SymbolKind must be a number of the SymbolKind enum"))
}

fn symbol_tags_of(value: &JsData) -> Option<Vec<SymbolTag>> {
    numbers_of(field(value, "tags")).map(|tags| tags.into_iter().filter_map(SymbolTag::from_value).collect())
}

fn symbol_information_from_js(value: &JsData) -> Result<SymbolInformation, Error> {
    let location = field(value, "location").ok_or_else(|| error("SymbolInformation.location must be a Location"))?;
    let mut information = SymbolInformation::new(
        &opt_string(field(value, "name")).unwrap_or_default(),
        symbol_kind_of(value)?,
        opt_string(field(value, "containerName")).as_deref(),
        location_from_js(location)?,
    )?;
    information.tags = symbol_tags_of(value);
    Ok(information)
}

fn document_symbol_from_js(value: &JsData) -> Result<DocumentSymbol, Error> {
    let mut symbol = DocumentSymbol::new(
        &opt_string(field(value, "name")).unwrap_or_default(),
        &opt_string(field(value, "detail")).unwrap_or_default(),
        symbol_kind_of(value)?,
        range_field(value, "range")?,
        range_field(value, "selectionRange")?,
    )?;
    symbol.tags = symbol_tags_of(value);
    symbol.children = match field(value, "children") {
        Some(JsData::Array(children)) => children.iter().map(document_symbol_from_js).collect::<Result<Vec<_>, _>>()?,
        _ => Vec::new(),
    };
    Ok(symbol)
}

/// `DocumentSymbol[] | SymbolInformation[] | undefined | null`: o primeiro elemento decide, como o `instanceof` do original.
pub fn document_symbols_from_js(value: &JsData) -> Result<Option<ApiDocumentSymbols>, Error> {
    let JsData::Array(entries) = value else { return Ok(None) };
    if entries.first().is_some_and(|first| field(first, "location").is_some()) {
        return Ok(Some(ApiDocumentSymbols::Information(entries.iter().map(symbol_information_from_js).collect::<Result<Vec<_>, _>>()?)));
    }
    Ok(Some(ApiDocumentSymbols::Symbols(entries.iter().map(document_symbol_from_js).collect::<Result<Vec<_>, _>>()?)))
}

// --- classes

pub(crate) fn define(
    runner: &NodeRunner,
    name: &str,
    length: u32,
    construct: impl Fn(&[JsData]) -> Result<JsData, String> + 'static,
    statics: Vec<(String, u32, NativeCallback)>,
) -> JsData {
    define_with_getters(runner, name, length, construct, statics, vec![])
}

/// [`define`] com acessores de leitura no protótipo (`CodeLens#isResolved`).
pub(crate) fn define_with_getters(
    runner: &NodeRunner,
    name: &str,
    length: u32,
    construct: impl Fn(&[JsData]) -> Result<JsData, String> + 'static,
    statics: Vec<(String, u32, NativeCallback)>,
    getters: Vec<(String, MethodCallback)>,
) -> JsData {
    JsData::Handle(Rc::new(runner.define_class(ClassSpec {
        name: name.to_owned(),
        length,
        construct: Rc::new(construct),
        methods: vec![],
        getters,
        statics,
        parent: None,
    })))
}

pub(crate) fn required<'a>(args: &'a [JsData], index: usize, what: &str) -> Result<&'a JsData, String> {
    args.get(index).filter(|value| !is_nullish(Some(*value))).ok_or_else(|| format!("{what} é obrigatório"))
}

pub(crate) fn text_arg(args: &[JsData], index: usize) -> Option<String> {
    opt_string(args.get(index))
}

pub(crate) fn class_fields(fields: Vec<(&str, Option<JsData>)>) -> JsData {
    JsData::Object(fields.into_iter().filter_map(|(name, value)| value.map(|value| (name.to_owned(), value))).collect())
}

pub(crate) fn number_arg(args: &[JsData], index: usize) -> Option<JsData> {
    opt_number(args.get(index)).map(JsData::Num)
}

fn define_text_edit(runner: &NodeRunner) -> JsData {
    let statics: Vec<(String, u32, NativeCallback)> = vec![
        (
            "replace".to_owned(),
            2,
            Rc::new(|args| Ok(api_to_js(&TextEdit::new(to_range(required(args, 0, "TextEdit.replace: o intervalo")?)?, text_arg(args, 1).as_deref())))),
        ),
        (
            "insert".to_owned(),
            2,
            Rc::new(|args| {
                let position = to_position(required(args, 0, "TextEdit.insert: a posição")?)?;
                Ok(api_to_js(&TextEdit::new(Range::new(position, position), text_arg(args, 1).as_deref())))
            }),
        ),
        ("delete".to_owned(), 1, Rc::new(|args| Ok(api_to_js(&TextEdit::new(to_range(required(args, 0, "TextEdit.delete: o intervalo")?)?, Some("")))))),
        (
            "setEndOfLine".to_owned(),
            1,
            Rc::new(|args| {
                let eol = EndOfLine::from_value(opt_number(args.first()).unwrap_or(0.0) as i32).ok_or("Illegal argument: endOfLine")?;
                let origin = Position::new(0.0, 0.0).map_err(|error| error.message)?;
                let mut edit = TextEdit::new(Range::new(origin, origin), Some(""));
                edit.set_new_eol(Some(eol));
                Ok(api_to_js(&edit))
            }),
        ),
    ];
    define(
        runner,
        "TextEdit",
        2,
        |args| Ok(api_to_js(&TextEdit::new(to_range(required(args, 0, "TextEdit: o intervalo")?)?, text_arg(args, 1).as_deref()))),
        statics,
    )
}

fn define_code_action_kind(runner: &NodeRunner) -> JsData {
    let kind_of = |this: &JsData| CodeActionKind::new(opt_string(field(this, "value")).unwrap_or_default());
    let other_of = |args: &[JsData]| code_action_kind_from_js(args.first().unwrap_or(&JsData::Undefined)).ok_or_else(|| "esperava um CodeActionKind".to_owned());
    let methods: Vec<(String, u32, MethodCallback)> = vec![
        ("append".to_owned(), 1, Rc::new(move |this, args| Ok(code_action_kind_data(&kind_of(this).append(&string_arg(args, 0, "append: o trecho")?))))),
        ("contains".to_owned(), 1, Rc::new(move |this, args| Ok(JsData::Bool(kind_of(this).contains(&other_of(args)?))))),
        ("intersects".to_owned(), 1, Rc::new(move |this, args| Ok(JsData::Bool(kind_of(this).intersects(&other_of(args)?))))),
    ];
    runner.define_class(ClassSpec {
        name: "CodeActionKind".to_owned(),
        length: 1,
        construct: Rc::new(|args| Ok(JsData::Object(vec![("value".to_owned(), JsData::Str(string_arg(args, 0, "CodeActionKind: o valor")?))]))),
        methods,
        getters: vec![],
        statics: vec![],
        parent: None,
    });
    JsData::Object(
        [
            ("Empty", CodeActionKind::empty()),
            ("QuickFix", CodeActionKind::quick_fix()),
            ("Refactor", CodeActionKind::refactor()),
            ("RefactorExtract", CodeActionKind::refactor_extract()),
            ("RefactorInline", CodeActionKind::refactor_inline()),
            ("RefactorMove", CodeActionKind::refactor_move()),
            ("RefactorRewrite", CodeActionKind::refactor_rewrite()),
            ("Source", CodeActionKind::source()),
            ("SourceOrganizeImports", CodeActionKind::source_organize_imports()),
            ("SourceFixAll", CodeActionKind::source_fix_all()),
            ("Notebook", CodeActionKind::notebook()),
        ]
        .into_iter()
        .map(|(name, kind)| (name.to_owned(), code_action_kind_data(&kind)))
        .collect(),
    )
}

/// As classes deste módulo como entradas do objeto `vscode`.
pub fn classes(runner: &NodeRunner) -> Vec<(String, JsData)> {
    let entry = |name: &str, value: JsData| (name.to_owned(), value);
    vec![
        entry(
            "Location",
            define(
                runner,
                "Location",
                2,
                |args| {
                    let uri = uri_from_js(required(args, 0, "Location: a Uri")?)?;
                    let range = match args.get(1) {
                        Some(range @ JsData::Object(_)) if field(range, "start").is_some() => Some(to_range(range)?),
                        Some(position @ JsData::Object(_)) if field(position, "line").is_some() => {
                            let position = to_position(position)?;
                            Some(Range::new(position, position))
                        }
                        _ => None,
                    };
                    Ok(api_to_js(&Location::new(Some(uri), range.map(RangeOrPosition::Range))))
                },
                vec![],
            ),
        ),
        entry(
            "Diagnostic",
            define(
                runner,
                "Diagnostic",
                3,
                |args| {
                    let range = to_range(required(args, 0, "Diagnostic: o intervalo")?)?;
                    let severity = opt_number(args.get(2)).and_then(|severity| DiagnosticSeverity::from_value(severity as i32));
                    let diagnostic = Diagnostic::new(range, &string_arg(args, 1, "Diagnostic: a mensagem")?, severity).map_err(|error| error.message)?;
                    Ok(api_to_js(&diagnostic))
                },
                vec![],
            ),
        ),
        entry(
            "DiagnosticRelatedInformation",
            define(
                runner,
                "DiagnosticRelatedInformation",
                2,
                |args| {
                    Ok(JsData::Object(vec![
                        ("location".to_owned(), required(args, 0, "DiagnosticRelatedInformation: a Location")?.clone()),
                        ("message".to_owned(), JsData::Str(string_arg(args, 1, "DiagnosticRelatedInformation: a mensagem")?)),
                    ]))
                },
                vec![],
            ),
        ),
        entry("TextEdit", define_text_edit(runner)),
        entry(
            "MarkdownString",
            define(
                runner,
                "MarkdownString",
                2,
                |args| Ok(api_to_js(&MarkdownString::new(text_arg(args, 0).as_deref(), opt_bool(args.get(1)).unwrap_or(false)))),
                vec![],
            ),
        ),
        entry("SnippetString", define(runner, "SnippetString", 1, |args| Ok(api_to_js(&SnippetString::new(text_arg(args, 0).as_deref()))), vec![])),
        entry(
            "CompletionItem",
            define(
                runner,
                "CompletionItem",
                2,
                |args| Ok(class_fields(vec![("label", Some(required(args, 0, "CompletionItem: o rótulo")?.clone())), ("kind", number_arg(args, 1))])),
                vec![],
            ),
        ),
        entry(
            "CompletionList",
            define(
                runner,
                "CompletionList",
                2,
                |args| {
                    let items = match args.first() {
                        Some(items @ JsData::Array(_)) => items.clone(),
                        _ => JsData::Array(vec![]),
                    };
                    Ok(JsData::Object(vec![("items".to_owned(), items), ("isIncomplete".to_owned(), JsData::Bool(opt_bool(args.get(1)).unwrap_or(false)))]))
                },
                vec![],
            ),
        ),
        entry(
            "Hover",
            define(
                runner,
                "Hover",
                2,
                |args| {
                    let contents = match required(args, 0, "Hover: o conteúdo")? {
                        contents @ JsData::Array(_) => contents.clone(),
                        single => JsData::Array(vec![single.clone()]),
                    };
                    let range = match args.get(1) {
                        Some(range @ JsData::Object(_)) => Some(range_data(to_range(range)?)),
                        _ => None,
                    };
                    Ok(class_fields(vec![("contents", Some(contents)), ("range", range)]))
                },
                vec![],
            ),
        ),
        entry(
            "CodeAction",
            define(
                runner,
                "CodeAction",
                2,
                |args| {
                    Ok(class_fields(vec![
                        ("title", Some(JsData::Str(string_arg(args, 0, "CodeAction: o título")?))),
                        ("kind", args.get(1).filter(|kind| !is_nullish(Some(*kind))).cloned()),
                    ]))
                },
                vec![],
            ),
        ),
        entry(
            "DocumentSymbol",
            define(
                runner,
                "DocumentSymbol",
                5,
                |args| {
                    let name = string_arg(args, 0, "DocumentSymbol: o nome")?;
                    let detail = text_arg(args, 1).unwrap_or_default();
                    let kind = opt_number(args.get(2)).and_then(|kind| SymbolKind::from_value(kind as i32)).ok_or("DocumentSymbol: o tipo precisa ser um SymbolKind")?;
                    let range = to_range(required(args, 3, "DocumentSymbol: o intervalo")?)?;
                    let selection_range = to_range(required(args, 4, "DocumentSymbol: o intervalo da seleção")?)?;
                    DocumentSymbol::new(&name, &detail, kind, range, selection_range).map_err(|error| error.message)?;
                    Ok(JsData::Object(vec![
                        ("name".to_owned(), JsData::Str(name)),
                        ("detail".to_owned(), JsData::Str(detail)),
                        ("kind".to_owned(), JsData::Num(f64::from(kind.value()))),
                        ("range".to_owned(), range_data(range)),
                        ("selectionRange".to_owned(), range_data(selection_range)),
                        ("children".to_owned(), JsData::Array(vec![])),
                    ]))
                },
                vec![],
            ),
        ),
        entry("CodeActionKind", define_code_action_kind(runner)),
    ]
}
