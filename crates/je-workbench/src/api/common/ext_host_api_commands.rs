//! Porte parcial de `src/vs/workbench/api/common/extHostApiCommands.ts`: os comandos `vscode.execute*` (e afins) que a
//! extensão chama e que o host traduz para os comandos internos `_execute*` do workbench (ver
//! [`editor_execute_commands`](super::editor_execute_commands)).
//!
//! Registrados aqui (cada um com o argumento, o resultado e a descrição do original):
//! `vscode.executeDocumentHighlights`, `vscode.executeDocumentSymbolProvider`, `vscode.executeFormatDocumentProvider`,
//! `vscode.executeFormatRangeProvider`, `vscode.executeFormatOnTypeProvider`, `vscode.executeHoverProvider` e a experimental,
//! `vscode.executeLinkProvider`, `vscode.executeCodeLensProvider`, `vscode.executeDocumentColorProvider`,
//! `vscode.executeColorPresentationProvider`, as oito consultas de localização
//! (`vscode.executeDefinitionProvider`, `TypeDefinition`, `Declaration`, `Implementation` e as quatro
//! `vscode.experimental.execute*Provider_recursive`), `vscode.executeReferenceProvider` e a experimental,
//! `vscode.executeSelectionRangeProvider`, `vscode.prepareRename`, `vscode.executeDocumentRenameProvider`,
//! `vscode.executeWorkspaceSymbolProvider`, `vscode.executeCompletionItemProvider`, `vscode.executeSignatureHelpProvider`,
//! `vscode.executeCodeActionProvider`, `vscode.executeInlayHintProvider`, as seis de hierarquia de chamadas e de tipos
//! (`prepareCallHierarchy`, `provideIncomingCalls`, `provideOutgoingCalls`, `prepareTypeHierarchy`, `provideSupertypes` e
//! `provideSubtypes`), `setContext` e `vscode.experimental.editSession.continue`.
//!
//! NÃO PORTADO (cada item pede um conversor ou um recurso que ainda não existe no porte; ficam por nome do comando do original):
//! `vscode.provideDocumentSemanticTokensLegend`, `vscode.provideDocumentSemanticTokens`,
//! `vscode.provideDocumentRangeSemanticTokensLegend`, `vscode.provideDocumentRangeSemanticTokens`,
//! `vscode.executeFoldingRangeProvider`, `vscode.resolveNotebookContentProviders`,
//! `vscode.executeInlineValueProvider`, `vscode.open`, `vscode.openWith`, `vscode.diff` (os três pedem
//! `typeConverters.TextEditorOpenOptions`), `vscode.changes`, os três de testes (`TestItem` e `TestRunProfile` não têm porte),
//! `vscode.editorChat.start`, `vscode.extensionPromptFileProvider` e o comando `_validateWhenClauses` (o resultado de
//! `validateWhenClauses` é uma lista de erros tipados, sem forma de [`Dynamic`]).
//!
//! Divergências:
//! - O `typeConverters.WorkspaceEdit.to` do resultado de `vscode.executeDocumentRenameProvider` e de `vscode.executeCodeActionProvider`
//!   lê o `{ edits }` do RPC (`workspace_edit_dto_from_dynamic`) e o `types.WorkspaceEdit` volta como objeto marcado `WorkspaceEdit`
//!   com `has(uri)` e `get(uri)`.
//! - `typeConverters.CompletionItem.to`, `InlayHint.to` e `SignatureHelp.to` leem o item do RPC (`languages.CompletionItem`,
//!   `languages.InlayHint` e `languages.SignatureHelp`) e devolvem o objeto da API marcado (`CompletionItem`, `CompletionList`,
//!   `InlayHint`, `InlayHintLabelPart`), só com as propriedades definidas; o `SignatureHelp` é um objeto literal, sem classe.
//! - O `CustomCodeAction._isSynthetic` é o campo `is_synthetic` do `CodeAction` do editor (ver `editor_execute_commands`).
//! - Os resultados dos comandos internos chegam como [`Dynamic`] (o JSON que o RPC entregaria) e o conversor devolve o
//!   [`Dynamic`] da classe da API, sem os genéricos `ApiCommandResult<V, O>`. O que a API tem como classe e este porte não
//!   tem como [`ApiValue`] (`SymbolInformation`, `SelectionRange`, `DocumentHighlight`) cruza como objeto marcado com
//!   `$$class`; a marca vale para a leitura direta da propriedade (o `instanceof` de [`is_instance_of`] só enxerga as classes
//!   de `CLASSES`).
//! - `MergedInfo` (a classe local de `vscode.executeDocumentSymbolProvider`, que estende `SymbolInformation` e implementa
//!   `DocumentSymbol`) é um objeto marcado `SymbolInformation` com as propriedades de `SymbolInformation` e as quatro
//!   de `DocumentSymbol` (`detail`, `range`, `selectionRange` e `children`).
//! - `typeConverters.SymbolKind.to` é a identidade numérica (os dois enums têm os mesmos 26 valores), então o número do
//!   DTO vira o `SymbolKind` da API por [`SymbolKind::from_value`](types::SymbolKind::from_value).
//! - `ExtHostApiCommands.register` devolve [`Result`], porque `registerApiCommand` lança no original quando o id já existe.

use std::rc::Rc;

use je_base::dynamic::{Dynamic, FunctionRef, Truthy};
use je_base::errors::Error;
use je_base::uri::URI;
use je_editor::code_action::languages::{Command as EditorCommand, IWorkspaceTextEdit, TextEdit as EditorTextEdit};
use je_editor::edit_operation::SingleEditOperation as EditorSingleEditOperation;
use je_editor::language_features_service::{
    InlayHint as EditorInlayHint, InlayHintKind as EditorInlayHintKind, InlayHintLabel as EditorInlayHintLabel, InlayHintLabelPart as EditorInlayHintLabelPart,
    Location as EditorLocation,
};
use je_editor::languages::{
    CompletionItem as EditorCompletionItem, CompletionItemKind as EditorCompletionItemKind, CompletionItemLabel as EditorCompletionItemLabel,
    CompletionItemRange as EditorCompletionItemRange, CompletionItemTag as EditorCompletionItemTag, CompletionLabel as EditorCompletionLabel, StringOrMarkdown,
};
use je_editor::parameter_hints::languages::{
    ParameterInformation as EditorParameterInformation, ParameterLabel as EditorParameterLabel, SignatureHelp as EditorSignatureHelp,
    SignatureInformation as EditorSignatureInformation,
};
use je_editor::language_providers::{DocumentHighlight as EditorDocumentHighlight, DocumentHighlightKind as EditorHighlightKind};
use je_editor::model::EndOfLineSequence;

use super::ext_host_commands::{ApiCommand, ApiCommandArgument, ApiCommandResult, CommandsConverter, ExtHostCommands};
use super::ext_host_language_features_converters::{
    Color as ColorConverter, ColorPresentation as ColorPresentationConverter, CompletionItem as CompletionItemConverter, Hover as HoverConverter,
    InlayHint as InlayHintConverter, SignatureHelp as SignatureHelpConverter,
};
use super::ext_host_protocol::{ICommandDto, IColorPresentationDto, IWorkspaceEditDto, IWorkspaceFileEditDto, WorkspaceEditEntryDto, WorkspaceFileEditOptionsDto};
use super::ext_host_type_converters::{self as type_converters, ICommandsConverter, Location, LocationDto};
use super::ext_host_types as types;
use super::ext_host_values::{ApiValue, is_instance_of, tag};

/// `tryMapWith(f)`: o resultado mapeado, ou `undefined` quando o valor não é uma lista.
fn try_map_with(
    f: impl Fn(&Dynamic) -> Result<Dynamic, Error> + 'static,
) -> impl Fn(&Dynamic, &[Dynamic], &CommandsConverter) -> Result<Dynamic, Error> + 'static {
    move |value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter| match value {
        Dynamic::Array(items) => items.to_vec().iter().map(&f).collect::<Result<Vec<_>, _>>().map(|mapped| Dynamic::array(mapped)),
        _ => Ok(Dynamic::Undefined),
    }
}

/// `Range.isIRange(value)` e `typeConverters.Range.to(value)`.
fn range_to_api(value: &Dynamic) -> Result<types::Range, Error> {
    let range = type_converters::Range::from_dto(value).ok_or_else(|| Error::type_error("expected an IRange"))?;
    type_converters::Range::to(&range)
}

/// `typeConverters.location.to(value)`.
fn location_to(value: &Dynamic) -> Result<Dynamic, Error> {
    let dto: LocationDto = Location::from_dto(value).ok_or_else(|| Error::type_error("expected a languages.Location"))?;
    Ok(Location::to(&dto)?.to_dynamic())
}

/// `languages.isLocationLink(thing)`.
fn is_location_link(value: &Dynamic) -> bool {
    is_instance_of(&value.get_str("uri"), "Uri")
        && type_converters::Range::from_dto(&value.get_str("range")).is_some()
        && (type_converters::Range::from_dto(&value.get_str("originSelectionRange")).is_some()
            || type_converters::Range::from_dto(&value.get_str("targetSelectionRange")).is_some())
}

/// `typeConverters.DefinitionLink.to(value)`: o `vscode.LocationLink` (um objeto literal, sem classe).
fn definition_link_to(value: &Dynamic) -> Result<Dynamic, Error> {
    let optional_range = |key: &str| -> Result<Option<Dynamic>, Error> {
        let range = value.get_str(key);
        if range.is_nullish() { Ok(None) } else { Ok(Some(range_to_api(&range)?.to_dynamic())) }
    };
    let entries = [
        ("targetUri", Some(value.get_str("uri"))),
        ("targetRange", Some(range_to_api(&value.get_str("range"))?.to_dynamic())),
        ("targetSelectionRange", optional_range("targetSelectionRange")?),
        ("originSelectionRange", optional_range("originSelectionRange")?),
    ];
    Ok(Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item)))))
}

/// `mapLocationOrLocationLink(values)`.
fn map_location_or_location_link(values: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    let Dynamic::Array(items) = values else {
        return Ok(Dynamic::Undefined);
    };
    let mut result = Vec::new();
    for item in items.to_vec() {
        result.push(if is_location_link(&item) { definition_link_to(&item)? } else { location_to(&item)? });
    }
    Ok(Dynamic::array(result))
}

/// `typeConverters.DocumentHighlight.to(occurrence)`.
fn document_highlight_to(value: &Dynamic) -> Result<Dynamic, Error> {
    let range = type_converters::Range::from_dto(&value.get_str("range")).ok_or_else(|| Error::type_error("expected an IRange"))?;
    let kind = match value.get_str("kind") {
        Dynamic::Number(kind) => Some(match kind as i32 {
            1 => EditorHighlightKind::Read,
            2 => EditorHighlightKind::Write,
            _ => EditorHighlightKind::Text,
        }),
        _ => None,
    };
    let highlight = type_converters::DocumentHighlight::to(&EditorDocumentHighlight { range, kind })?;
    Ok(tag(Dynamic::object([("range", highlight.range.to_dynamic()), ("kind", Dynamic::from(highlight.kind.value()))]), "DocumentHighlight"))
}

/// A leitura do `languages.TextEdit` do RPC (`{ range, text, eol? }`).
fn text_edit_from_dto(value: &Dynamic) -> Result<EditorTextEdit, Error> {
    let range = type_converters::Range::from_dto(&value.get_str("range")).ok_or_else(|| Error::type_error("expected an IRange"))?;
    let text = match value.get_str("text") {
        Dynamic::String(text) => text.to_string_lossy(),
        _ => String::new(),
    };
    let eol = match value.get_str("eol") {
        Dynamic::Number(eol) => Some(if eol as i32 == 1 { EndOfLineSequence::CrLf } else { EndOfLineSequence::Lf }),
        _ => None,
    };
    Ok(EditorTextEdit { range, text, eol, insert_as_snippet: None, keep_whitespace: None })
}

/// `typeConverters.TextEdit.to(edit)`.
fn text_edit_to(value: &Dynamic) -> Result<Dynamic, Error> {
    Ok(type_converters::TextEdit::to(&text_edit_from_dto(value)?)?.to_dynamic())
}

/// `new types.ColorInformation(typeConverters.Range.to(ci.range), typeConverters.Color.to(ci.color))` como valor (objeto marcado
/// `ColorInformation`).
fn color_information_to(value: &Dynamic) -> Result<Dynamic, Error> {
    let range = range_to_api(&value.get_str("range"))?;
    let channels = match value.get_str("color") {
        Dynamic::Array(channels) if channels.len() == 4 => channels.to_vec(),
        _ => return Err(Error::type_error("expected a color of four channels")),
    };
    let channel = |index: usize| match &channels[index] {
        Dynamic::Number(channel) => *channel,
        _ => f64::NAN,
    };
    let color = ColorConverter::to([channel(0), channel(1), channel(2), channel(3)]);
    let information = types::ColorInformation::new(range, color)?;
    Ok(tag(Dynamic::object([("range", information.range.to_dynamic()), ("color", information.color.to_dynamic())]), "ColorInformation"))
}

/// O resultado de `vscode.executeDocumentColorProvider`: `if (result) { return result.map(...); } return [];`.
fn color_informations_to(value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    match value {
        Dynamic::Array(items) => items.to_vec().iter().map(color_information_to).collect::<Result<Vec<_>, _>>().map(|colors| Dynamic::array(colors)),
        value if !value.is_truthy() => Ok(Dynamic::array(Vec::<Dynamic>::new())),
        _ => Err(Error::type_error("result.map is not a function")),
    }
}

/// `typeConverters.ColorPresentation.to(presentation)` como valor (objeto marcado `ColorPresentation`).
fn color_presentation_to(value: &Dynamic) -> Result<Dynamic, Error> {
    let label = match value.get_str("label") {
        Dynamic::String(label) => label.to_string_lossy(),
        _ => String::new(),
    };
    let text_edit = match value.get_str("textEdit") {
        edit if edit.is_truthy() => Some(text_edit_from_dto(&edit)?),
        _ => None,
    };
    let additional_text_edits = match value.get_str("additionalTextEdits") {
        Dynamic::Array(edits) => Some(edits.to_vec().iter().map(text_edit_from_dto).collect::<Result<Vec<_>, _>>()?),
        _ => None,
    };
    let presentation = ColorPresentationConverter::to(&IColorPresentationDto { label, text_edit, additional_text_edits })?;
    let entries = [
        ("label", Some(Dynamic::string(&presentation.label))),
        ("textEdit", presentation.text_edit.as_ref().map(ApiValue::to_dynamic)),
        ("additionalTextEdits", presentation.additional_text_edits.as_ref().map(|edits| Dynamic::array(edits.iter().map(ApiValue::to_dynamic)))),
    ];
    Ok(tag(Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item)))), "ColorPresentation"))
}

/// O resultado de `vscode.executeColorPresentationProvider`: `if (result) { return result.map(...); } return [];`.
fn color_presentations_to(value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    match value {
        Dynamic::Array(items) => items.to_vec().iter().map(color_presentation_to).collect::<Result<Vec<_>, _>>().map(|presentations| Dynamic::array(presentations)),
        value if !value.is_truthy() => Ok(Dynamic::array(Vec::<Dynamic>::new())),
        _ => Err(Error::type_error("result.map is not a function")),
    }
}

/// `MergedInfo.to(symbol)`: o `SymbolInformation` que também é um `DocumentSymbol` (ver as divergências do módulo).
fn merged_info_to(symbol: &Dynamic, uri: &URI) -> Result<Dynamic, Error> {
    let range = range_to_api(&symbol.get_str("range"))?;
    let selection_range = range_to_api(&symbol.get_str("selectionRange"))?;
    let kind = match symbol.get_str("kind") {
        Dynamic::Number(kind) => types::SymbolKind::from_value(kind as i32),
        _ => None,
    }
    .ok_or_else(|| Error::type_error("expected a SymbolKind"))?;
    // `symbol.containerName || ''`
    let container_name = match symbol.get_str("containerName") {
        Dynamic::String(name) if !name.is_empty() => Dynamic::String(name),
        _ => Dynamic::string(""),
    };
    let location = types::Location::new(Some(uri.clone()), Some(types::RangeOrPosition::Range(range)));
    // `symbol.children ? symbol.children.map(MergedInfo.to) : []`
    let children = match symbol.get_str("children") {
        Dynamic::Array(children) => {
            Dynamic::array(children.to_vec().iter().map(|child| merged_info_to(child, uri)).collect::<Result<Vec<_>, _>>()?)
        }
        _ => Dynamic::array(Vec::<Dynamic>::new()),
    };
    Ok(tag(
        Dynamic::object([
            ("name", symbol.get_str("name")),
            ("kind", Dynamic::from(kind.value())),
            ("containerName", container_name),
            ("location", location.to_dynamic()),
            ("detail", symbol.get_str("detail")),
            ("range", range.to_dynamic()),
            ("selectionRange", selection_range.to_dynamic()),
            ("children", children),
        ]),
        "SymbolInformation",
    ))
}

/// O resultado de `vscode.executeDocumentSymbolProvider`.
fn document_symbols_to(value: &Dynamic, api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    // `isFalsyOrEmpty(value)`
    let Dynamic::Array(items) = value else {
        return Ok(Dynamic::Undefined);
    };
    if items.is_empty() {
        return Ok(Dynamic::Undefined);
    }
    let uri = URI::from_dynamic(&api_args.first().cloned().unwrap_or(Dynamic::Undefined))?;
    items.to_vec().iter().map(|symbol| merged_info_to(symbol, &uri)).collect::<Result<Vec<_>, _>>().map(|symbols| Dynamic::array(symbols))
}

/// O `types.SelectionRange` como valor (objeto marcado `SelectionRange`, com o `parent` encadeado).
fn selection_range_to_dynamic(node: &types::SelectionRange) -> Dynamic {
    let parent = node.parent.as_ref().map(|parent| ("parent", selection_range_to_dynamic(parent)));
    tag(Dynamic::object(std::iter::once(("range", node.range.to_dynamic())).chain(parent)), "SelectionRange")
}

/// O resultado de `vscode.executeSelectionRangeProvider`: cada lista de intervalos (do menor para o maior) vira a cadeia de
/// `SelectionRange` pelo pai.
fn selection_ranges_to(value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    let Dynamic::Array(rows) = value else {
        return Err(Error::type_error("result.map is not a function"));
    };
    let mut result = Vec::new();
    for ranges in rows.to_vec() {
        let Dynamic::Array(ranges) = &ranges else {
            return Err(Error::type_error("ranges.reverse is not a function"));
        };
        let mut node: Option<types::SelectionRange> = None;
        for range in ranges.to_vec().iter().rev() {
            node = Some(types::SelectionRange::new(range_to_api(range)?, node)?);
        }
        result.push(node.as_ref().map_or(Dynamic::Undefined, selection_range_to_dynamic));
    }
    Ok(Dynamic::array(result))
}

/// A leitura de um `Dto<languages.IWorkspaceTextEdit>` do RPC: `{ resource, textEdit, versionId? }` (os `metadata` não cruzam).
fn workspace_text_edit_from_dto(value: &Dynamic) -> Result<IWorkspaceTextEdit, Error> {
    let resource = URI::from_dynamic(&value.get_str("resource"))?;
    let dto = value.get_str("textEdit");
    let mut text_edit = text_edit_from_dto(&dto)?;
    let flag = |key: &str| match dto.get_str(key) {
        Dynamic::Bool(flag) => Some(flag),
        _ => None,
    };
    text_edit.insert_as_snippet = flag("insertAsSnippet");
    text_edit.keep_whitespace = flag("keepWhitespace");
    let version_id = match value.get_str("versionId") {
        Dynamic::Number(version_id) => Some(version_id as i64),
        _ => None,
    };
    Ok(IWorkspaceTextEdit { resource, text_edit, version_id, metadata: None })
}

/// A leitura de um `IWorkspaceFileEditDto` do RPC: `{ oldResource?, newResource?, options? }`.
fn workspace_file_edit_from_dto(value: &Dynamic) -> Result<IWorkspaceFileEditDto, Error> {
    let resource = |key: &str| match value.get_str(key) {
        Dynamic::Undefined | Dynamic::Null => Ok(None),
        resource => URI::from_dynamic(&resource).map(Some),
    };
    let options = match value.get_str("options") {
        options @ Dynamic::Object(_) => {
            let flag = |key: &str| match options.get_str(key) {
                Dynamic::Bool(flag) => Some(flag),
                _ => None,
            };
            Some(WorkspaceFileEditOptionsDto {
                overwrite: flag("overwrite"),
                ignore_if_not_exists: flag("ignoreIfNotExists"),
                ignore_if_exists: flag("ignoreIfExists"),
                recursive: flag("recursive"),
                copy: flag("copy"),
                folder: flag("folder"),
                skip_trash_bin: flag("skipTrashBin"),
                max_size: match options.get_str("maxSize") {
                    Dynamic::Number(max_size) => Some(max_size),
                    _ => None,
                },
                contents: None,
            })
        }
        _ => None,
    };
    Ok(IWorkspaceFileEditDto { old_resource: resource("oldResource")?, new_resource: resource("newResource")?, options, metadata: None })
}

/// A leitura do `IWorkspaceEditDto & { rejectReason? }` do RPC: `{ edits, rejectReason? }`. O item com `textEdit` é a edição de
/// texto e o resto é a operação de arquivo (a mesma separação de `WorkspaceEdit.to`).
fn workspace_edit_dto_from_dynamic(value: &Dynamic) -> Result<IWorkspaceEditDto, Error> {
    let Dynamic::Array(edits) = value.get_str("edits") else {
        return Err(Error::type_error("value.edits is not iterable"));
    };
    let mut entries = Vec::new();
    for edit in edits.to_vec() {
        entries.push(if edit.get_str("textEdit").is_truthy() {
            WorkspaceEditEntryDto::Text(workspace_text_edit_from_dto(&edit)?)
        } else {
            WorkspaceEditEntryDto::File(workspace_file_edit_from_dto(&edit)?)
        });
    }
    Ok(IWorkspaceEditDto { edits: entries })
}

/// O `types.WorkspaceEdit` como valor: um objeto marcado `WorkspaceEdit` com `has(uri)` e `get(uri)` (a parte `Maplike` que os
/// casos leem); `get` devolve as `TextEdit` do recurso.
fn workspace_edit_to_dynamic(edit: types::WorkspaceEdit) -> Dynamic {
    let edit = Rc::new(edit);
    let (has_edit, get_edit) = (Rc::clone(&edit), Rc::clone(&edit));
    let uri_of = |args: &[Dynamic]| args.first().and_then(|uri| URI::from_dynamic(uri).ok());
    tag(
        Dynamic::object([
            (
                "has",
                Dynamic::from(FunctionRef::native("has", 1, move |args| Dynamic::Bool(uri_of(args).is_some_and(|uri| has_edit.has(&uri))))),
            ),
            (
                "get",
                Dynamic::from(FunctionRef::native("get", 1, move |args| {
                    Dynamic::array(uri_of(args).map(|uri| get_edit.get(&uri)).unwrap_or_default().iter().map(ApiValue::to_dynamic))
                })),
            ),
        ]),
        "WorkspaceEdit",
    )
}

/// O resultado de `vscode.executeDocumentRenameProvider`: sem valor é `undefined`, com `rejectReason` lança, e o resto é
/// `typeConverters.WorkspaceEdit.to(value)`.
fn rename_edits_to(value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    if !value.is_truthy() {
        return Ok(Dynamic::Undefined);
    }
    if let Dynamic::String(reject_reason) = value.get_str("rejectReason") {
        if !reject_reason.to_string_lossy().is_empty() {
            return Err(Error::new(reject_reason.to_string_lossy()));
        }
    }
    Ok(workspace_edit_to_dynamic(type_converters::WorkspaceEdit::to(&workspace_edit_dto_from_dynamic(value)?)?))
}

/// `editorRange.Range.isIRange(range)` ou o objeto `{ insert, replace }`: o `range` de um `languages.CompletionItem` do RPC.
fn completion_item_range_from_dto(value: &Dynamic) -> Result<EditorCompletionItemRange, Error> {
    if let Some(range) = type_converters::Range::from_dto(value) {
        return Ok(EditorCompletionItemRange::Single(range));
    }
    match (type_converters::Range::from_dto(&value.get_str("insert")), type_converters::Range::from_dto(&value.get_str("replace"))) {
        (Some(insert), Some(replace)) => Ok(EditorCompletionItemRange::InsertReplace { insert, replace }),
        _ => Err(Error::type_error("expected a CompletionItem range")),
    }
}

/// A leitura do `languages.CompletionItem` do RPC.
fn completion_item_from_dto(value: &Dynamic) -> Result<EditorCompletionItem, Error> {
    let optional_text = |key: &str| match value.get_str(key) {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    };
    let label = match value.get_str("label") {
        Dynamic::String(text) => EditorCompletionLabel::Text(text.to_string_lossy()),
        label @ Dynamic::Object(_) => {
            let Dynamic::String(text) = label.get_str("label") else {
                return Err(Error::type_error("expected a CompletionItemLabel"));
            };
            let part = |key: &str| match label.get_str(key) {
                Dynamic::String(text) => Some(text.to_string_lossy()),
                _ => None,
            };
            EditorCompletionLabel::Item(EditorCompletionItemLabel { label: text.to_string_lossy(), detail: part("detail"), description: part("description") })
        }
        _ => return Err(Error::type_error("expected a CompletionItem label")),
    };
    // o que o `_to.get(kind) ?? Property` do original não conhece vira `Property`
    let kind = match value.get_str("kind") {
        Dynamic::Number(kind) => EditorCompletionItemKind::from_index(kind as usize).unwrap_or(EditorCompletionItemKind::Property),
        _ => EditorCompletionItemKind::Property,
    };
    let tags = match value.get_str("tags") {
        Dynamic::Array(tags) => Some(
            tags.to_vec()
                .iter()
                .filter_map(|tag| match tag {
                    Dynamic::Number(tag) if *tag as i32 == 1 => Some(EditorCompletionItemTag::Deprecated),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    };
    let commit_characters = match value.get_str("commitCharacters") {
        Dynamic::Array(characters) => Some(
            characters
                .to_vec()
                .iter()
                .filter_map(|character| match character {
                    Dynamic::String(character) => Some(character.to_string_lossy()),
                    _ => None,
                })
                .collect(),
        ),
        _ => None,
    };
    let additional_text_edits = match value.get_str("additionalTextEdits") {
        Dynamic::Array(edits) => Some(
            edits
                .to_vec()
                .iter()
                .map(|edit| {
                    let range = type_converters::Range::from_dto(&edit.get_str("range")).ok_or_else(|| Error::type_error("expected an IRange"))?;
                    let text = match edit.get_str("text") {
                        Dynamic::String(text) => Some(text),
                        _ => None,
                    };
                    Ok(EditorSingleEditOperation::new(range, text, matches!(edit.get_str("forceMoveMarkers"), Dynamic::Bool(true))))
                })
                .collect::<Result<Vec<_>, Error>>()?,
        ),
        _ => None,
    };
    Ok(EditorCompletionItem {
        label,
        kind,
        range: completion_item_range_from_dto(&value.get_str("range"))?,
        tags,
        detail: optional_text("detail"),
        documentation: string_or_markdown_from_dto(&value.get_str("documentation")),
        sort_text: optional_text("sortText"),
        filter_text: optional_text("filterText"),
        preselect: match value.get_str("preselect") {
            Dynamic::Bool(preselect) => Some(preselect),
            _ => None,
        },
        insert_text: optional_text("insertText").unwrap_or_default(),
        insert_text_rules: match value.get_str("insertTextRules") {
            Dynamic::Number(rules) => Some(rules as u32),
            _ => None,
        },
        commit_characters,
        additional_text_edits,
        command: command_from_dto(&value.get_str("command")),
        action: None,
        extension_id: None,
    })
}

/// O `types.CompletionItem` como valor (objeto marcado `CompletionItem`, só com as propriedades definidas): o `range` é o
/// `Range` da API ou o objeto `{ inserting, replacing }`, e o `insertText` é um texto ou o `SnippetString`.
fn completion_item_to_api(item: &types::CompletionItem) -> Dynamic {
    let label = match &item.label {
        types::CompletionLabel::Text(text) => Dynamic::string(text),
        types::CompletionLabel::Detailed(label) => {
            let entries = [
                ("label", Some(Dynamic::string(&label.label))),
                ("detail", label.detail.as_deref().map(Dynamic::string)),
                ("description", label.description.as_deref().map(Dynamic::string)),
            ];
            Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item))))
        }
    };
    let range = item.range.as_ref().map(|range| match range {
        types::CompletionRange::Range(range) => range.to_dynamic(),
        types::CompletionRange::InsertReplace { inserting, replacing } => Dynamic::object([("inserting", inserting.to_dynamic()), ("replacing", replacing.to_dynamic())]),
    });
    let insert_text = item.insert_text.as_ref().map(|insert_text| match insert_text {
        types::InsertText::Text(text) => Dynamic::string(text),
        types::InsertText::Snippet(snippet) => snippet.to_dynamic(),
    });
    let entries = [
        ("label", Some(label)),
        ("kind", item.kind.map(|kind| Dynamic::from(kind.value()))),
        ("tags", item.tags.as_ref().map(|tags| Dynamic::array(tags.iter().map(|tag| Dynamic::from(tag.value()))))),
        ("detail", item.detail.as_deref().map(Dynamic::string)),
        ("documentation", item.documentation.as_ref().map(documentation_to_dynamic)),
        ("sortText", item.sort_text.as_deref().map(Dynamic::string)),
        ("filterText", item.filter_text.as_deref().map(Dynamic::string)),
        ("preselect", item.preselect.map(Dynamic::Bool)),
        ("insertText", insert_text),
        ("keepWhitespace", item.keep_whitespace.map(Dynamic::Bool)),
        ("range", range),
        ("commitCharacters", item.commit_characters.as_ref().map(|characters| Dynamic::array(characters.iter().map(|character| Dynamic::string(character))))),
        ("textEdit", item.text_edit.as_ref().map(ApiValue::to_dynamic)),
        ("additionalTextEdits", item.additional_text_edits.as_ref().map(|edits| Dynamic::array(edits.iter().map(ApiValue::to_dynamic)))),
        ("command", item.command.as_ref().map(types::Command::to_dynamic)),
    ];
    tag(Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item)))), "CompletionItem")
}

/// O resultado de `vscode.executeCompletionItemProvider`: sem valor é a `CompletionList` vazia, e o resto é
/// `new types.CompletionList(value.suggestions.map(s => typeConverters.CompletionItem.to(s, converter)), value.incomplete)`.
fn completion_list_to(value: &Dynamic, _api_args: &[Dynamic], converter: &CommandsConverter) -> Result<Dynamic, Error> {
    let list = if value.is_truthy() {
        let Dynamic::Array(suggestions) = value.get_str("suggestions") else {
            return Err(Error::type_error("value.suggestions.map is not a function"));
        };
        let items = suggestions
            .to_vec()
            .iter()
            .map(|suggestion| CompletionItemConverter::to(&completion_item_from_dto(suggestion)?, Some(converter)))
            .collect::<Result<Vec<_>, Error>>()?;
        let incomplete = match value.get_str("incomplete") {
            Dynamic::Bool(incomplete) => Some(incomplete),
            _ => None,
        };
        types::CompletionList::new(Some(items), incomplete)
    } else {
        types::CompletionList::new(None, None)
    };
    let entries = [
        ("items", Some(Dynamic::array(list.items.iter().map(completion_item_to_api)))),
        ("isIncomplete", list.is_incomplete.map(Dynamic::Bool)),
    ];
    Ok(tag(Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item)))), "CompletionList"))
}

/// A leitura de um `languages.ParameterInformation` do RPC: o `label` é um texto ou o par `[início, fim]`.
fn parameter_information_from_dto(value: &Dynamic) -> Result<EditorParameterInformation, Error> {
    let label = match value.get_str("label") {
        Dynamic::String(text) => EditorParameterLabel::Text(text.to_string_lossy()),
        Dynamic::Array(offsets) => match offsets.to_vec().as_slice() {
            [Dynamic::Number(start), Dynamic::Number(end)] => EditorParameterLabel::Range(*start as u32, *end as u32),
            _ => return Err(Error::type_error("expected a ParameterInformation label")),
        },
        _ => return Err(Error::type_error("expected a ParameterInformation label")),
    };
    Ok(EditorParameterInformation { label, documentation: string_or_markdown_from_dto(&value.get_str("documentation")) })
}

/// A leitura de um `languages.SignatureInformation` do RPC.
fn signature_information_from_dto(value: &Dynamic) -> Result<EditorSignatureInformation, Error> {
    let Dynamic::String(label) = value.get_str("label") else {
        return Err(Error::type_error("expected a SignatureInformation"));
    };
    let parameters = match value.get_str("parameters") {
        Dynamic::Array(parameters) => parameters.to_vec().iter().map(parameter_information_from_dto).collect::<Result<Vec<_>, _>>()?,
        _ => Vec::new(),
    };
    let active_parameter = match value.get_str("activeParameter") {
        Dynamic::Number(active) => Some(active as i32),
        _ => None,
    };
    Ok(EditorSignatureInformation {
        label: label.to_string_lossy(),
        documentation: string_or_markdown_from_dto(&value.get_str("documentation")),
        parameters,
        active_parameter,
    })
}

/// `typeConverters.SignatureHelp.to(value)` como valor (um objeto literal `vscode.SignatureHelp`, sem classe).
fn signature_help_to(value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    if !value.is_truthy() {
        return Ok(Dynamic::Undefined);
    }
    let number = |key: &str| match value.get_str(key) {
        Dynamic::Number(number) => number as i32,
        _ => 0,
    };
    let signatures = match value.get_str("signatures") {
        Dynamic::Array(signatures) => signatures.to_vec().iter().map(signature_information_from_dto).collect::<Result<Vec<_>, _>>()?,
        _ => Vec::new(),
    };
    let help = SignatureHelpConverter::to(&EditorSignatureHelp { signatures, active_signature: number("activeSignature"), active_parameter: number("activeParameter") });
    let parameter_to_dynamic = |parameter: &types::ParameterInformation| {
        let label = match &parameter.label {
            types::ParameterLabel::Text(text) => Dynamic::string(text),
            types::ParameterLabel::Offsets(start, end) => Dynamic::array([Dynamic::Number(*start), Dynamic::Number(*end)]),
        };
        let entries = [("label", Some(label)), ("documentation", parameter.documentation.as_ref().map(documentation_to_dynamic))];
        Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item))))
    };
    let signatures = help.signatures.iter().map(|signature| {
        let entries = [
            ("label", Some(Dynamic::string(&signature.label))),
            ("documentation", signature.documentation.as_ref().map(documentation_to_dynamic)),
            ("parameters", Some(Dynamic::array(signature.parameters.iter().map(parameter_to_dynamic)))),
            ("activeParameter", signature.active_parameter.map(Dynamic::Number)),
        ];
        Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item))))
    });
    Ok(Dynamic::object([
        ("signatures", Dynamic::array(signatures)),
        ("activeSignature", Dynamic::Number(help.active_signature)),
        ("activeParameter", Dynamic::Number(help.active_parameter)),
    ]))
}

/// `converter.fromInternal(command)` de um `languages.Command` do RPC: `None` quando o valor não é um comando ou o comando
/// guardado já foi descartado.
fn command_from_internal(value: &Dynamic, converter: &CommandsConverter) -> Option<types::Command> {
    let command = command_from_dto(value)?;
    converter.from_internal(&ICommandDto { ident: None, id: command.id, title: command.title, tooltip: command.tooltip, arguments: command.arguments })
}

/// O `CustomCodeAction` do resultado de `vscode.executeCodeActionProvider`: a ação sintética (um `Command` embrulhado) volta como
/// o `Command` original, e a outra vira o `types.CodeAction` (objeto marcado `CodeAction`, só com as propriedades definidas; o
/// `kind` é um objeto marcado `CodeActionKind`).
fn code_action_to(value: &Dynamic, converter: &CommandsConverter) -> Result<Dynamic, Error> {
    if value.get_str("_isSynthetic").is_truthy() {
        if !value.get_str("command").is_truthy() {
            return Err(Error::new("Synthetic code actions must have a command"));
        }
        return Ok(command_from_internal(&value.get_str("command"), converter).as_ref().map_or(Dynamic::Undefined, types::Command::to_dynamic));
    }

    let title = match value.get_str("title") {
        Dynamic::String(title) => title.to_string_lossy(),
        _ => String::new(),
    };
    let kind = match value.get_str("kind") {
        Dynamic::String(kind) if !kind.to_string_lossy().is_empty() => Some(types::CodeActionKind::new(kind.to_string_lossy())),
        _ => None,
    };
    let mut action = types::CodeAction::new(title, kind);
    if value.get_str("edit").is_truthy() {
        action.edit = Some(type_converters::WorkspaceEdit::to(&workspace_edit_dto_from_dynamic(&value.get_str("edit"))?)?);
    }
    if value.get_str("command").is_truthy() {
        action.command = command_from_internal(&value.get_str("command"), converter);
    }
    action.is_preferred = match value.get_str("isPreferred") {
        Dynamic::Bool(is_preferred) => Some(is_preferred),
        _ => None,
    };
    let entries = [
        ("title", Some(Dynamic::string(&action.title))),
        ("kind", action.kind.as_ref().map(|kind| tag(Dynamic::object([("value", Dynamic::string(&kind.value))]), "CodeActionKind"))),
        ("edit", action.edit.take().map(workspace_edit_to_dynamic)),
        ("command", action.command.as_ref().map(types::Command::to_dynamic)),
        ("isPreferred", action.is_preferred.map(Dynamic::Bool)),
    ];
    Ok(tag(Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item)))), "CodeAction"))
}

/// O resultado de `vscode.executeCodeActionProvider`: `tryMapWith(codeAction => ...)(value)`.
fn code_actions_to(value: &Dynamic, _api_args: &[Dynamic], converter: &CommandsConverter) -> Result<Dynamic, Error> {
    match value {
        Dynamic::Array(items) => items.to_vec().iter().map(|item| code_action_to(item, converter)).collect::<Result<Vec<_>, _>>().map(|actions| Dynamic::array(actions)),
        _ => Ok(Dynamic::Undefined),
    }
}

/// `value.map(f)`: o que não é uma lista lança o `TypeError` de `map`.
fn map_items(value: &Dynamic, f: impl Fn(&Dynamic) -> Result<Dynamic, Error>) -> Result<Dynamic, Error> {
    match value {
        Dynamic::Array(items) => items.to_vec().iter().map(f).collect::<Result<Vec<_>, _>>().map(|mapped| Dynamic::array(mapped)),
        _ => Err(Error::type_error("value.map is not a function")),
    }
}

/// A leitura do `ICallHierarchyItemDto` e do `ITypeHierarchyItemDto` do RPC.
fn hierarchy_item_from_dto(value: &Dynamic) -> Result<type_converters::HierarchyItemDto, Error> {
    type_converters::HierarchyItemDto::from_dynamic(value).ok_or_else(|| Error::type_error("expected a hierarchy item"))
}

/// `typeConverters.CallHierarchyItem.to(item)`.
fn call_hierarchy_item_to(value: &Dynamic) -> Result<Dynamic, Error> {
    Ok(type_converters::CallHierarchyItem::to(&hierarchy_item_from_dto(value)?)?.to_dynamic())
}

/// `typeConverters.TypeHierarchyItem.to(item)`.
fn type_hierarchy_item_to(value: &Dynamic) -> Result<Dynamic, Error> {
    Ok(type_converters::TypeHierarchyItem::to(&hierarchy_item_from_dto(value)?)?.to_dynamic())
}

/// `{ from | to, fromRanges }` de uma chamada da API como valor (objeto marcado com a `class`).
fn hierarchy_call_to_dynamic(key: &str, item: Dynamic, from_ranges: &[types::Range], class: &str) -> Dynamic {
    tag(Dynamic::object([(key, item), ("fromRanges", Dynamic::array(from_ranges.iter().map(ApiValue::to_dynamic)))]), class)
}

/// `typeConverters.CallHierarchyIncomingCall.to(item)`.
fn incoming_call_to(value: &Dynamic) -> Result<Dynamic, Error> {
    let dto = type_converters::IncomingCallDto::from_dynamic(value).ok_or_else(|| Error::type_error("expected an IIncomingCallDto"))?;
    let call = type_converters::CallHierarchyIncomingCall::to(&dto)?;
    Ok(hierarchy_call_to_dynamic("from", call.from.to_dynamic(), &call.from_ranges, "CallHierarchyIncomingCall"))
}

/// `typeConverters.CallHierarchyOutgoingCall.to(item)`.
fn outgoing_call_to(value: &Dynamic) -> Result<Dynamic, Error> {
    let dto = type_converters::OutgoingCallDto::from_dynamic(value).ok_or_else(|| Error::type_error("expected an IOutgoingCallDto"))?;
    let call = type_converters::CallHierarchyOutgoingCall::to(&dto)?;
    Ok(hierarchy_call_to_dynamic("to", call.to.to_dynamic(), &call.from_ranges, "CallHierarchyOutgoingCall"))
}

/// O resultado de `vscode.prepareCallHierarchy`.
fn call_hierarchy_items_to(value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    map_items(value, call_hierarchy_item_to)
}

/// O resultado de `vscode.provideIncomingCalls`.
fn incoming_calls_to(value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    map_items(value, incoming_call_to)
}

/// O resultado de `vscode.provideOutgoingCalls`.
fn outgoing_calls_to(value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    map_items(value, outgoing_call_to)
}

/// O resultado de `vscode.prepareTypeHierarchy`, `vscode.provideSupertypes` e `vscode.provideSubtypes`.
fn type_hierarchy_items_to(value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    map_items(value, type_hierarchy_item_to)
}

/// O resultado de `vscode.prepareRename`.
fn prepare_rename_to(value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    if value.is_nullish() {
        return Ok(Dynamic::Undefined);
    }
    let range = range_to_api(&value.get_str("range"))?;
    Ok(Dynamic::object([("range", range.to_dynamic()), ("placeholder", value.get_str("text"))]))
}

/// `typeConverters.WorkspaceSymbol.to(info)`: o `SymbolInformation` (objeto marcado `SymbolInformation`).
fn workspace_symbol_to(value: &Dynamic) -> Result<Dynamic, Error> {
    let kind = match value.get_str("kind") {
        Dynamic::Number(kind) => types::SymbolKind::from_value(kind as i32),
        _ => None,
    }
    .ok_or_else(|| Error::type_error("expected a SymbolKind"))?;
    // `location.to(info.location)` tolera o `uri` e o `range` ausentes
    let location = value.get_str("location");
    let uri = location.get_str("uri");
    let dto = LocationDto {
        uri: if is_instance_of(&uri, "Uri") { URI::from_dynamic(&uri).ok() } else { None },
        range: type_converters::Range::from_dto(&location.get_str("range")),
    };
    let tags = value.get_str("tags");
    let entries = [
        ("name", Some(value.get_str("name"))),
        ("kind", Some(Dynamic::from(kind.value()))),
        ("containerName", Some(value.get_str("containerName")).filter(|name| !name.is_undefined())),
        ("location", Some(Location::to(&dto)?.to_dynamic())),
        // `info.tags && info.tags.map(SymbolTag.to)`: a conversão é a identidade numérica
        ("tags", Some(tags).filter(|tags| matches!(tags, Dynamic::Array(_)))),
    ];
    Ok(tag(Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item)))), "SymbolInformation"))
}

/// O resultado de `vscode.executeWorkspaceSymbolProvider`.
fn workspace_symbols_to(value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    match value {
        Dynamic::Array(items) => items.to_vec().iter().map(workspace_symbol_to).collect::<Result<Vec<_>, _>>().map(|symbols| Dynamic::array(symbols)),
        _ => Err(Error::type_error("value.map is not a function")),
    }
}

/// `typeConverters.Hover.to(info)`: o `vscode.VerboseHover` como valor (objeto marcado `VerboseHover`, só com as propriedades
/// definidas).
fn hover_to(value: &Dynamic) -> Result<Dynamic, Error> {
    let info = type_converters::Hover::from_dto(value).ok_or_else(|| Error::type_error("expected a languages.Hover"))?;
    let hover = HoverConverter::to(&info)?;
    let contents = hover.hover.contents.iter().map(|content| match content {
        types::MarkedString::Text(text) => Dynamic::string(text),
        types::MarkedString::Markdown(markdown) => markdown.to_dynamic(),
        types::MarkedString::Code { language, value } => Dynamic::object([("language", Dynamic::string(language)), ("value", Dynamic::string(value))]),
    });
    let entries = [
        ("contents", Some(Dynamic::array(contents))),
        ("range", hover.hover.range.as_ref().map(ApiValue::to_dynamic)),
        ("canIncreaseVerbosity", hover.can_increase_verbosity.map(Dynamic::Bool)),
        ("canDecreaseVerbosity", hover.can_decrease_verbosity.map(Dynamic::Bool)),
    ];
    Ok(tag(Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item)))), "VerboseHover"))
}

/// `typeConverters.DocumentLink.to(link)`: o `vscode.DocumentLink` como valor (objeto marcado `DocumentLink`, só com as
/// propriedades definidas).
fn document_link_to(value: &Dynamic) -> Result<Dynamic, Error> {
    let info = type_converters::DocumentLink::from_dto(value).ok_or_else(|| Error::type_error("expected a languages.ILink"))?;
    let link = type_converters::DocumentLink::to(&info)?;
    let entries = [
        ("range", Some(link.range.to_dynamic())),
        ("target", link.target.as_ref().map(ApiValue::to_dynamic)),
        ("tooltip", link.tooltip.as_deref().map(Dynamic::string)),
    ];
    Ok(tag(Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item)))), "DocumentLink"))
}

/// O resultado de `vscode.executeLinkProvider`: `value.map(typeConverters.DocumentLink.to)`.
fn document_links_to(value: &Dynamic, _api_args: &[Dynamic], _converter: &CommandsConverter) -> Result<Dynamic, Error> {
    match value {
        Dynamic::Array(items) => items.to_vec().iter().map(document_link_to).collect::<Result<Vec<_>, _>>().map(|links| Dynamic::array(links)),
        _ => Err(Error::type_error("value.map is not a function")),
    }
}

/// `new types.CodeLens(typeConverters.Range.to(item.range), item.command && converter.fromInternal(item.command))` como valor
/// (objeto marcado `CodeLens`, só com as propriedades definidas).
fn code_lens_to(value: &Dynamic, converter: &CommandsConverter) -> Result<Dynamic, Error> {
    let item = type_converters::CodeLens::from_dto(value).ok_or_else(|| Error::type_error("expected a languages.CodeLens"))?;
    let range = type_converters::Range::to(&item.range)?;
    let command = item.command.and_then(|command| {
        converter.from_internal(&ICommandDto { ident: None, id: command.id, title: command.title, tooltip: command.tooltip, arguments: command.arguments })
    });
    let lens = types::CodeLens::new(range, command);
    let entries = [("range", Some(lens.range.to_dynamic())), ("command", lens.command.as_ref().map(types::Command::to_dynamic))];
    Ok(tag(Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item)))), "CodeLens"))
}

/// O resultado de `vscode.executeCodeLensProvider`: `tryMapWith(item => new types.CodeLens(...))(value)`.
fn code_lenses_to(value: &Dynamic, _api_args: &[Dynamic], converter: &CommandsConverter) -> Result<Dynamic, Error> {
    match value {
        Dynamic::Array(items) => items.to_vec().iter().map(|item| code_lens_to(item, converter)).collect::<Result<Vec<_>, _>>().map(|lenses| Dynamic::array(lenses)),
        _ => Ok(Dynamic::Undefined),
    }
}

/// `string | IMarkdownString` do DTO do RPC.
fn string_or_markdown_from_dto(value: &Dynamic) -> Option<StringOrMarkdown> {
    match value {
        Dynamic::String(text) => Some(StringOrMarkdown::String(text.to_string_lossy())),
        Dynamic::Object(_) => type_converters::MarkdownString::from_dto(value).map(StringOrMarkdown::Markdown),
        _ => None,
    }
}

/// `languages.Command.is(value)` e a leitura do `languages.Command` do RPC (`{ id, title, tooltip?, arguments? }`).
fn command_from_dto(value: &Dynamic) -> Option<EditorCommand> {
    let (Dynamic::String(id), Dynamic::String(title)) = (value.get_str("id"), value.get_str("title")) else {
        return None;
    };
    Some(EditorCommand {
        id: id.to_string_lossy(),
        title: title.to_string_lossy(),
        tooltip: match value.get_str("tooltip") {
            Dynamic::String(tooltip) => Some(tooltip.to_string_lossy()),
            _ => None,
        },
        arguments: match value.get_str("arguments") {
            Dynamic::Array(arguments) => Some(arguments.to_vec()),
            _ => None,
        },
    })
}

/// A leitura do `languages.InlayHintLabelPart` do RPC.
fn inlay_hint_label_part_from_dto(value: &Dynamic) -> Result<EditorInlayHintLabelPart, Error> {
    let Dynamic::String(label) = value.get_str("label") else {
        return Err(Error::type_error("expected an InlayHintLabelPart"));
    };
    let location = match value.get_str("location") {
        Dynamic::Undefined | Dynamic::Null => None,
        location => {
            let dto: LocationDto = Location::from_dto(&location).ok_or_else(|| Error::type_error("expected a languages.Location"))?;
            match (dto.uri, dto.range) {
                (Some(uri), Some(range)) => Some(EditorLocation { uri, range }),
                _ => return Err(Error::type_error("expected a languages.Location")),
            }
        }
    };
    Ok(EditorInlayHintLabelPart {
        label: label.to_string_lossy(),
        tooltip: string_or_markdown_from_dto(&value.get_str("tooltip")),
        location,
        command: command_from_dto(&value.get_str("command")),
    })
}

/// A leitura do `languages.InlayHint` do RPC.
fn inlay_hint_from_dto(value: &Dynamic) -> Result<EditorInlayHint, Error> {
    let position = type_converters::Position::from_dto(&value.get_str("position")).ok_or_else(|| Error::type_error("expected an IPosition"))?;
    let label = match value.get_str("label") {
        Dynamic::String(text) => EditorInlayHintLabel::Text(text.to_string_lossy()),
        Dynamic::Array(parts) => EditorInlayHintLabel::Parts(parts.to_vec().iter().map(inlay_hint_label_part_from_dto).collect::<Result<Vec<_>, _>>()?),
        _ => return Err(Error::type_error("hint.label.map is not a function")),
    };
    let kind = match value.get_str("kind") {
        Dynamic::Number(kind) if kind as i32 == 1 => Some(EditorInlayHintKind::Type),
        Dynamic::Number(kind) if kind as i32 == 2 => Some(EditorInlayHintKind::Parameter),
        _ => None,
    };
    let text_edits = match value.get_str("textEdits") {
        Dynamic::Array(edits) => Some(edits.to_vec().iter().map(text_edit_from_dto).collect::<Result<Vec<_>, _>>()?),
        _ => None,
    };
    let flag = |key: &str| match value.get_str(key) {
        Dynamic::Bool(flag) => Some(flag),
        _ => None,
    };
    Ok(EditorInlayHint {
        label,
        tooltip: string_or_markdown_from_dto(&value.get_str("tooltip")),
        position,
        text_edits,
        kind,
        padding_left: flag("paddingLeft"),
        padding_right: flag("paddingRight"),
        cache_id: None,
    })
}

/// `string | MarkdownString` da API como valor.
fn documentation_to_dynamic(documentation: &types::Documentation) -> Dynamic {
    match documentation {
        types::Documentation::Text(text) => Dynamic::string(text),
        types::Documentation::Markdown(markdown) => markdown.to_dynamic(),
    }
}

/// `typeConverters.InlayHint.to(converter, hint)` como valor (objeto marcado `InlayHint`, só com as propriedades definidas).
fn inlay_hint_to(value: &Dynamic, converter: &CommandsConverter) -> Result<Dynamic, Error> {
    let hint = InlayHintConverter::to(converter, &inlay_hint_from_dto(value)?)?;
    let label = match &hint.label {
        types::InlayHintLabel::Text(text) => Dynamic::string(text),
        types::InlayHintLabel::Parts(parts) => Dynamic::array(parts.iter().map(|part| {
            let entries = [
                ("value", Some(Dynamic::string(&part.value))),
                ("tooltip", part.tooltip.as_ref().map(documentation_to_dynamic)),
                ("location", part.location.as_ref().map(ApiValue::to_dynamic)),
                ("command", part.command.as_ref().map(types::Command::to_dynamic)),
            ];
            tag(Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item)))), "InlayHintLabelPart")
        })),
    };
    let entries = [
        ("label", Some(label)),
        ("tooltip", hint.tooltip.as_ref().map(documentation_to_dynamic)),
        ("position", Some(hint.position.to_dynamic())),
        ("textEdits", hint.text_edits.as_ref().map(|edits| Dynamic::array(edits.iter().map(ApiValue::to_dynamic)))),
        ("kind", hint.kind.map(|kind| Dynamic::from(kind.value()))),
        ("paddingLeft", hint.padding_left.map(Dynamic::Bool)),
        ("paddingRight", hint.padding_right.map(Dynamic::Bool)),
    ];
    Ok(tag(Dynamic::object(entries.into_iter().filter_map(|(key, item)| item.map(|item| (key, item)))), "InlayHint"))
}

/// O resultado de `vscode.executeInlayHintProvider`: `result.map(typeConverters.InlayHint.to.bind(undefined, converter))`.
fn inlay_hints_to(value: &Dynamic, _api_args: &[Dynamic], converter: &CommandsConverter) -> Result<Dynamic, Error> {
    map_items(value, |item| inlay_hint_to(item, converter))
}

/// `new ApiCommandArgument('rangeOrSelection', ..., v => types.Range.isRange(v), v => types.Selection.isSelection(v) ?
/// typeConverters.Selection.from(v) : typeConverters.Range.from(v))`.
fn range_or_selection_argument() -> ApiCommandArgument {
    let is_range = ApiCommandArgument::range().validate;
    let selection = ApiCommandArgument::selection();
    let range = ApiCommandArgument::range();
    ApiCommandArgument::new(
        "rangeOrSelection",
        "Range in a text document. Some refactoring provider requires Selection object.",
        move |value| is_range(value),
        move |value| if (selection.validate)(value) { (selection.convert)(value) } else { (range.convert)(value) },
    )
}

/// `new ApiCommandArgument('options', 'Formatting options', _ => true, v => v)`.
fn formatting_options_argument() -> ApiCommandArgument {
    ApiCommandArgument::new("options", "Formatting options", |_| true, |value| Ok(value.clone()))
}

/// Os comandos de localização (`definition`, `type definition`, `declaration`, `implementation`), cada um com a variante
/// `_recursive` experimental: o nome do tipo no id público e no id interno.
const LOCATION_COMMANDS: [(&str, &str); 4] = [("Definition", "all definition"), ("TypeDefinition", "all type definition"), ("Declaration", "all declaration"), ("Implementation", "all implementation")];

/// O `newCommands` do original.
fn new_commands() -> Vec<ApiCommand> {
    let location_result = |description: &'static str| ApiCommandResult::new(description, map_location_or_location_link);
    let uri_and_position = || vec![ApiCommandArgument::uri(), ApiCommandArgument::position()];
    let text_edits_result = || ApiCommandResult::new("A promise that resolves to an array of TextEdits.", try_map_with(text_edit_to));

    let mut commands = vec![
        // -- document highlights
        ApiCommand::new(
            "vscode.executeDocumentHighlights",
            "_executeDocumentHighlights",
            "Execute document highlight provider.",
            uri_and_position(),
            ApiCommandResult::new("A promise that resolves to an array of DocumentHighlight-instances.", try_map_with(document_highlight_to)),
        ),
        // -- document symbols
        ApiCommand::new(
            "vscode.executeDocumentSymbolProvider",
            "_executeDocumentSymbolProvider",
            "Execute document symbol provider.",
            vec![ApiCommandArgument::uri()],
            ApiCommandResult::new("A promise that resolves to an array of SymbolInformation and DocumentSymbol instances.", document_symbols_to),
        ),
        // -- formatting
        ApiCommand::new(
            "vscode.executeFormatDocumentProvider",
            "_executeFormatDocumentProvider",
            "Execute document format provider.",
            vec![ApiCommandArgument::uri(), formatting_options_argument()],
            text_edits_result(),
        ),
        ApiCommand::new(
            "vscode.executeFormatRangeProvider",
            "_executeFormatRangeProvider",
            "Execute range format provider.",
            vec![ApiCommandArgument::uri(), ApiCommandArgument::range(), formatting_options_argument()],
            text_edits_result(),
        ),
        ApiCommand::new(
            "vscode.executeFormatOnTypeProvider",
            "_executeFormatOnTypeProvider",
            "Execute format on type provider.",
            vec![
                ApiCommandArgument::uri(),
                ApiCommandArgument::position(),
                ApiCommandArgument::new("ch", "Trigger character", |value| matches!(value, Dynamic::String(_)), |value| Ok(value.clone())),
                formatting_options_argument(),
            ],
            text_edits_result(),
        ),
    ];

    // -- rename
    commands.push(ApiCommand::new(
        "vscode.executeDocumentRenameProvider",
        "_executeDocumentRenameProvider",
        "Execute rename provider.",
        vec![ApiCommandArgument::uri(), ApiCommandArgument::position(), ApiCommandArgument::string().with(Some("newName"), Some("The new symbol name"))],
        ApiCommandResult::new("A promise that resolves to a WorkspaceEdit.", rename_edits_to),
    ));

    // -- completions
    commands.push(ApiCommand::new(
        "vscode.executeCompletionItemProvider",
        "_executeCompletionItemProvider",
        "Execute completion item provider.",
        vec![
            ApiCommandArgument::uri(),
            ApiCommandArgument::position(),
            ApiCommandArgument::string()
                .with(Some("triggerCharacter"), Some("Trigger completion when the user types the character, like `,` or `(`"))
                .optional(),
            ApiCommandArgument::number().with(Some("itemResolveCount"), Some("Number of completions to resolve (too large numbers slow down completions)")).optional(),
        ],
        ApiCommandResult::new("A promise that resolves to a CompletionList-instance.", completion_list_to),
    ));

    // -- signature help
    commands.push(ApiCommand::new(
        "vscode.executeSignatureHelpProvider",
        "_executeSignatureHelpProvider",
        "Execute signature help provider.",
        vec![
            ApiCommandArgument::uri(),
            ApiCommandArgument::position(),
            ApiCommandArgument::string()
                .with(Some("triggerCharacter"), Some("Trigger signature help when the user types the character, like `,` or `(`"))
                .optional(),
        ],
        ApiCommandResult::new("A promise that resolves to SignatureHelp.", signature_help_to),
    ));

    // -- go to symbol (definition, type definition, declaration, impl)
    for (name, what) in LOCATION_COMMANDS {
        commands.push(ApiCommand::new(
            &format!("vscode.execute{name}Provider"),
            &format!("_execute{name}Provider"),
            &format!("Execute {what} providers."),
            uri_and_position(),
            location_result("A promise that resolves to an array of Location or LocationLink instances."),
        ));
        commands.push(ApiCommand::new(
            &format!("vscode.experimental.execute{name}Provider_recursive"),
            &format!("_execute{name}Provider_recursive"),
            &format!("Execute {what} providers."),
            uri_and_position(),
            location_result("A promise that resolves to an array of Location or LocationLink instances."),
        ));
    }

    // -- references
    let references_result = || ApiCommandResult::new("A promise that resolves to an array of Location-instances.", try_map_with(location_to));
    commands.push(ApiCommand::new(
        "vscode.executeReferenceProvider",
        "_executeReferenceProvider",
        "Execute all reference providers.",
        uri_and_position(),
        references_result(),
    ));
    commands.push(ApiCommand::new(
        "vscode.experimental.executeReferenceProvider",
        "_executeReferenceProvider_recursive",
        "Execute all reference providers.",
        uri_and_position(),
        references_result(),
    ));

    // -- hover
    let hover_result = || ApiCommandResult::new("A promise that resolves to an array of Hover-instances.", try_map_with(hover_to));
    commands.push(ApiCommand::new("vscode.executeHoverProvider", "_executeHoverProvider", "Execute all hover providers.", uri_and_position(), hover_result()));
    commands.push(ApiCommand::new(
        "vscode.experimental.executeHoverProvider_recursive",
        "_executeHoverProvider_recursive",
        "Execute all hover providers.",
        uri_and_position(),
        hover_result(),
    ));

    // -- selection range
    commands.push(ApiCommand::new(
        "vscode.executeSelectionRangeProvider",
        "_executeSelectionRangeProvider",
        "Execute selection range provider.",
        vec![
            ApiCommandArgument::uri(),
            ApiCommandArgument::arr(&ApiCommandArgument::position()).with(Some("position"), Some("A position in a text document")),
        ],
        ApiCommandResult::new("A promise that resolves to an array of ranges.", selection_ranges_to),
    ));

    // --- rename
    commands.push(ApiCommand::new(
        "vscode.prepareRename",
        "_executePrepareRename",
        "Execute the prepareRename of rename provider.",
        uri_and_position(),
        ApiCommandResult::new("A promise that resolves to a range and placeholder text.", prepare_rename_to),
    ));

    // -- code actions
    commands.push(ApiCommand::new(
        "vscode.executeCodeActionProvider",
        "_executeCodeActionProvider",
        "Execute code action provider.",
        vec![
            ApiCommandArgument::uri(),
            range_or_selection_argument(),
            ApiCommandArgument::string().with(Some("kind"), Some("Code action kind to return code actions for")).optional(),
            ApiCommandArgument::number().with(Some("itemResolveCount"), Some("Number of code actions to resolve (too large numbers slow down code actions)")).optional(),
        ],
        ApiCommandResult::new("A promise that resolves to an array of Command-instances.", code_actions_to),
    ));

    // -- call hierarchy
    commands.push(ApiCommand::new(
        "vscode.prepareCallHierarchy",
        "_executePrepareCallHierarchy",
        "Prepare call hierarchy at a position inside a document",
        uri_and_position(),
        ApiCommandResult::new("A promise that resolves to an array of CallHierarchyItem-instances", call_hierarchy_items_to),
    ));
    commands.push(ApiCommand::new(
        "vscode.provideIncomingCalls",
        "_executeProvideIncomingCalls",
        "Compute incoming calls for an item",
        vec![ApiCommandArgument::call_hierarchy_item()],
        ApiCommandResult::new("A promise that resolves to an array of CallHierarchyIncomingCall-instances", incoming_calls_to),
    ));
    commands.push(ApiCommand::new(
        "vscode.provideOutgoingCalls",
        "_executeProvideOutgoingCalls",
        "Compute outgoing calls for an item",
        vec![ApiCommandArgument::call_hierarchy_item()],
        ApiCommandResult::new("A promise that resolves to an array of CallHierarchyOutgoingCall-instances", outgoing_calls_to),
    ));

    // -- type hierarchy
    commands.push(ApiCommand::new(
        "vscode.prepareTypeHierarchy",
        "_executePrepareTypeHierarchy",
        "Prepare type hierarchy at a position inside a document",
        uri_and_position(),
        ApiCommandResult::new("A promise that resolves to an array of TypeHierarchyItem-instances", type_hierarchy_items_to),
    ));
    commands.push(ApiCommand::new(
        "vscode.provideSupertypes",
        "_executeProvideSupertypes",
        "Compute supertypes for an item",
        vec![ApiCommandArgument::type_hierarchy_item()],
        ApiCommandResult::new("A promise that resolves to an array of TypeHierarchyItem-instances", type_hierarchy_items_to),
    ));
    commands.push(ApiCommand::new(
        "vscode.provideSubtypes",
        "_executeProvideSubtypes",
        "Compute subtypes for an item",
        vec![ApiCommandArgument::type_hierarchy_item()],
        ApiCommandResult::new("A promise that resolves to an array of TypeHierarchyItem-instances", type_hierarchy_items_to),
    ));

    // -- code lens
    commands.push(ApiCommand::new(
        "vscode.executeCodeLensProvider",
        "_executeCodeLensProvider",
        "Execute code lens provider.",
        vec![
            ApiCommandArgument::uri(),
            ApiCommandArgument::number()
                .with(Some("itemResolveCount"), Some("Number of lenses that should be resolved and returned. Will only return resolved lenses, will impact performance)"))
                .optional(),
        ],
        ApiCommandResult::new("A promise that resolves to an array of CodeLens-instances.", code_lenses_to),
    ));

    // -- links
    commands.push(ApiCommand::new(
        "vscode.executeLinkProvider",
        "_executeLinkProvider",
        "Execute document link provider.",
        vec![
            ApiCommandArgument::uri(),
            ApiCommandArgument::number().with(Some("linkResolveCount"), Some("Number of links that should be resolved, only when links are unresolved.")).optional(),
        ],
        ApiCommandResult::new("A promise that resolves to an array of DocumentLink-instances.", document_links_to),
    ));

    // -- colors
    commands.push(ApiCommand::new(
        "vscode.executeDocumentColorProvider",
        "_executeDocumentColorProvider",
        "Execute document color provider.",
        vec![ApiCommandArgument::uri()],
        ApiCommandResult::new("A promise that resolves to an array of ColorInformation objects.", color_informations_to),
    ));
    commands.push(ApiCommand::new(
        "vscode.executeColorPresentationProvider",
        "_executeColorPresentationProvider",
        "Execute color presentation provider.",
        vec![
            ApiCommandArgument::new(
                "color",
                "The color to show and insert",
                |value| is_instance_of(value, "Color"),
                |value| Ok(Dynamic::array(ColorConverter::from(&types::Color::from_dynamic(value)?).map(Dynamic::Number))),
            ),
            ApiCommandArgument::new("context", "Context object with uri and range", |_| true, |value| {
                let range = type_converters::Range::from(&types::Range::from_dynamic(&value.get_str("range"))?);
                Ok(Dynamic::object([("uri", value.get_str("uri")), ("range", type_converters::Range::to_dto(&range))]))
            }),
        ],
        ApiCommandResult::new("A promise that resolves to an array of ColorPresentation objects.", color_presentations_to),
    ));

    // -- inline hints
    commands.push(ApiCommand::new(
        "vscode.executeInlayHintProvider",
        "_executeInlayHintProvider",
        "Execute inlay hints provider",
        vec![ApiCommandArgument::uri(), ApiCommandArgument::range()],
        ApiCommandResult::new("A promise that resolves to an array of Inlay objects", inlay_hints_to),
    ));

    // -- symbol search
    commands.push(ApiCommand::new(
        "vscode.executeWorkspaceSymbolProvider",
        "_executeWorkspaceSymbolProvider",
        "Execute all workspace symbol providers.",
        vec![ApiCommandArgument::string().with(Some("query"), Some("Search string"))],
        ApiCommandResult::new("A promise that resolves to an array of SymbolInformation-instances.", workspace_symbols_to),
    ));

    // --- continue edit session
    commands.push(ApiCommand::new(
        "vscode.experimental.editSession.continue",
        "_workbench.editSessions.actions.continueEditSession",
        "Continue the current edit session in a different workspace",
        vec![ApiCommandArgument::uri().with(Some("workspaceUri"), Some("The target workspace to continue the current edit session in"))],
        ApiCommandResult::void(),
    ));

    // --- context keys
    commands.push(ApiCommand::new(
        "setContext",
        "_setContext",
        "Set a custom context key value that can be used in when clauses.",
        vec![
            ApiCommandArgument::string().with(Some("name"), Some("The context key name")),
            ApiCommandArgument::new("value", "The context key value", |_| true, |value| Ok(value.clone())),
        ],
        ApiCommandResult::void(),
    ));

    commands
}

/// `ExtHostApiCommands`.
pub struct ApiCommands;

impl ApiCommands {
    /// `ExtHostApiCommands.register(commands)`: `newCommands.forEach(commands.registerApiCommand, commands)`.
    pub fn register(commands: &ExtHostCommands) -> Result<(), Error> {
        for command in new_commands() {
            commands.register_api_command(command)?;
        }
        Ok(())
    }
}
