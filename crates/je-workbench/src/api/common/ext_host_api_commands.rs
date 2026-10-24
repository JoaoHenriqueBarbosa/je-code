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
//! `vscode.executeSelectionRangeProvider`, `vscode.prepareRename`, `vscode.executeWorkspaceSymbolProvider`, `setContext` e
//! `vscode.experimental.editSession.continue`.
//!
//! NÃO PORTADO (cada item pede um conversor ou um recurso que ainda não existe no porte; ficam por nome do comando do original):
//! - `vscode.executeDocumentRenameProvider` (o
//!   `WorkspaceEdit.to` recebe o `IWorkspaceEditDto` tipado e o resultado do comando é o `WorkspaceEdit` do editor),
//!   `vscode.provideDocumentSemanticTokensLegend`, `vscode.provideDocumentSemanticTokens`,
//!   `vscode.provideDocumentRangeSemanticTokensLegend`, `vscode.provideDocumentRangeSemanticTokens`,
//!   `vscode.executeCompletionItemProvider`, `vscode.executeSignatureHelpProvider`,
//!   `vscode.executeCodeActionProvider`, `vscode.executeInlayHintProvider`, `vscode.executeFoldingRangeProvider`, `vscode.resolveNotebookContentProviders`,
//!   `vscode.executeInlineValueProvider`, `vscode.open`, `vscode.openWith`, `vscode.diff` (os três pedem
//!   `typeConverters.TextEditorOpenOptions`), `vscode.changes`, as seis de hierarquia de chamadas e de tipos
//!   (`prepareCallHierarchy`, `provideIncomingCalls`, `provideOutgoingCalls`, `prepareTypeHierarchy`, `provideSupertypes`,
//!   `provideSubtypes`: o `HierarchyItemDto` só sabe virar [`Dynamic`], não ler de um, e os comandos
//!   `_executePrepare*` do workbench não têm porte), os três de testes (`TestItem` e `TestRunProfile` não têm porte),
//!   `vscode.editorChat.start`, `vscode.extensionPromptFileProvider` e o comando `_validateWhenClauses` (o resultado de
//!   `validateWhenClauses` é uma lista de erros tipados, sem forma de [`Dynamic`]).
//!
//! Divergências:
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

use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::uri::URI;
use je_editor::code_action::languages::TextEdit as EditorTextEdit;
use je_editor::language_providers::{DocumentHighlight as EditorDocumentHighlight, DocumentHighlightKind as EditorHighlightKind};
use je_editor::model::EndOfLineSequence;

use super::ext_host_commands::{ApiCommand, ApiCommandArgument, ApiCommandResult, CommandsConverter, ExtHostCommands};
use super::ext_host_language_features_converters::{Color as ColorConverter, ColorPresentation as ColorPresentationConverter, Hover as HoverConverter};
use super::ext_host_protocol::{ICommandDto, IColorPresentationDto};
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
