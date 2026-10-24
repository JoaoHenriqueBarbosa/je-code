//! Os conversores de `extHostTypeConverters.ts` que `extHostLanguageFeatures.ts` usa e que ainda não existiam:
//! `DocumentSelector`, `DefinitionLink`, `Hover` e `MultiDocumentHighlight` (parte 1) e, na parte 2,
//! `WorkspaceSymbol`, `EvaluatableExpression`, `InlineValue`, `InlineValueContext`, `CodeActionTriggerKind`,
//! `CompletionTriggerKind`, `CompletionContext`, `CompletionItemTag`, `CompletionItemKind`, `ParameterInformation`,
//! `SignatureInformation`, `SignatureHelp`, `InlayHintKind`, `DocumentLink`, `ColorPresentation`, `Color`,
//! `SelectionRange`, `FoldingRange` e `FoldingRangeKind`.
//!
//! Divergências:
//! - `DocumentSelector.from` recebe o seletor já tipado ([`DocumentSelectorInput`], o mesmo do conversor
//!   `LanguageSelector`) e devolve os [`IDocumentFilterDto`]. O `asArray(value)` percorre a lista (e achata uma lista
//!   aninhada, que o tipo do original não permite). O `uriTransformer` e a `extension` ausentes são `None`.
//! - `DefinitionLink.from` devolve [`Result`]: o original lança um `TypeError` ao ler o `range` de uma `Location` sem
//!   intervalo, e aqui uma `Location` sem `uri` também é erro, porque o `ILocationLinkDto` exige o recurso.
//! - `Hover.from` recebe o [`VerboseHover`] (ver `ext_host_language_features_api`); o `range` ausente fica ausente no DTO.
//!   `Hover.to` e `MultiDocumentHighlight.to` devolvem [`Result`], porque o construtor de `Range` da API valida.
//! - Os mapas `CompletionItemKind.from` e `FoldingRangeKind.from` são totais (os enums da API e do editor têm os mesmos
//!   membros), então o `?? Property` e o `undefined` do original não existem. `ParameterInformation.from` não lança
//!   `TypeError('Invalid label')`: o tipo do rótulo garante a validade. `SignatureHelp.from` devolve o `SignatureHelp` do
//!   editor (sem o `id`, que o adaptador acrescenta); `SignatureHelp.to` recebe os três campos do DTO.
//! - `WorkspaceSymbol.from` devolve o [`IWorkspaceSymbolDto`] sem `cache_id`.
//! - `InlayHintKind` tem os mesmos membros nos dois lados (o original devolve o próprio número): `from` e `to` são o mapa total.
//!   `InlayHint.to` e `InlayHintLabelPart.to` devolvem [`Result`], porque `Position.to`, `TextEdit.to` e `location.to` validam.

use je_base::errors::Error;
use je_base::uri_ipc::IURITransformer;
use je_editor::code_action::languages::{CodeActionTriggerType, TextEdit as EditorTextEdit};
use je_editor::folding::syntax_range_provider::{FoldingRange as EditorFoldingRange, FoldingRangeKind as EditorFoldingRangeKind};
use je_editor::language_features_service::{
    InlayHint as EditorInlayHint, InlayHintKind as EditorInlayHintKind, InlayHintLabel as EditorInlayHintLabel, InlayHintLabelPart as EditorInlayHintLabelPart,
};
use je_editor::language_providers::SelectionRange as EditorSelectionRange;
use je_editor::languages::{
    CompletionContext as EditorCompletionContext, CompletionItem as EditorCompletionItem, CompletionItemKind as EditorCompletionItemKind,
    CompletionItemRange as EditorCompletionItemRange, CompletionItemTag as EditorCompletionItemTag, CompletionLabel as EditorCompletionLabel,
    CompletionTriggerKind as EditorCompletionTriggerKind, StringOrMarkdown, completion_item_insert_text_rule,
};
use je_editor::parameter_hints::languages::{
    ParameterInformation as EditorParameterInformation, ParameterLabel as EditorParameterLabel, SignatureHelp as EditorSignatureHelp,
    SignatureInformation as EditorSignatureInformation,
};
use je_extensions::scanner::ExtensionDescription;

use super::ext_host_language_features_api::{ApiInlineValue, DefinitionLink as ApiDefinitionLink, LocationOrLink};
use super::ext_host_protocol::{
    ICommandDto, EvaluatableExpressionDto, HoverDto, IColorPresentationDto, IDocumentFilterDto, IInlineValueContextDto, ILinkDto, ILocationLinkDto, IWorkspaceSymbolDto,
    InlineValueDto, MultiDocumentHighlightDto,
};
use super::ext_host_type_converters::symbols::DocumentHighlight;
use super::ext_host_type_converters::{
    DocumentSelectorInput, ICommandsConverter, Location, LocationDto, MarkdownString, Position, Range, SymbolKind, SymbolTag, TextEdit,
};
use super::ext_host_types::{self as types, CodeActionTriggerKind as ApiCodeActionTriggerKind, Documentation, MarkedString, VerboseHover};
use super::ext_host_workspace::glob_pattern_from;

/// `typeConverters.DocumentSelector`.
pub struct DocumentSelector;

impl DocumentSelector {
    /// `DocumentSelector.from(value, uriTransformer?, extension?)`.
    pub fn from(value: &DocumentSelectorInput, uri_transformer: Option<&dyn IURITransformer>, extension: Option<&ExtensionDescription>) -> Vec<IDocumentFilterDto> {
        match value {
            DocumentSelectorInput::Array(selectors) => selectors.iter().flat_map(|selector| DocumentSelector::from(selector, uri_transformer, extension)).collect(),
            DocumentSelectorInput::Language(language) => vec![IDocumentFilterDto {
                language: Some(language.clone()),
                is_builtin: extension.map(|extension| extension.is_builtin),
                ..IDocumentFilterDto::default()
            }],
            DocumentSelectorInput::Filter(filter) => vec![IDocumentFilterDto {
                language: filter.language.clone(),
                scheme: filter.scheme.as_deref().map(|scheme| DocumentSelector::transform_scheme(scheme, uri_transformer)),
                pattern: filter.pattern.as_ref().map(glob_pattern_from),
                exclusive: filter.exclusive,
                notebook_type: filter.notebook_type.clone(),
                is_builtin: extension.map(|extension| extension.is_builtin),
            }],
        }
    }

    fn transform_scheme(scheme: &str, uri_transformer: Option<&dyn IURITransformer>) -> String {
        match uri_transformer {
            Some(uri_transformer) => uri_transformer.transform_outgoing_scheme(scheme),
            None => scheme.to_owned(),
        }
    }
}

/// `typeConverters.DefinitionLink`.
pub struct DefinitionLink;

impl DefinitionLink {
    pub fn from(value: &LocationOrLink) -> Result<ILocationLinkDto, Error> {
        match value {
            LocationOrLink::Link(link) => Ok(ILocationLinkDto {
                origin_selection_range: Range::from_optional(link.origin_selection_range.as_ref()),
                uri: link.target_uri.clone(),
                range: Range::from(&link.target_range),
                target_selection_range: Range::from_optional(link.target_selection_range.as_ref()),
            }),
            LocationOrLink::Location(location) => {
                let uri = location.uri.clone().ok_or_else(|| Error::type_error("Cannot read properties of undefined (reading 'uri')"))?;
                let range = location.range.as_ref().ok_or_else(|| Error::type_error("Cannot read properties of undefined (reading 'start')"))?;
                Ok(ILocationLinkDto { origin_selection_range: None, uri, range: Range::from(range), target_selection_range: None })
            }
        }
    }

    pub fn to(value: &ILocationLinkDto) -> Result<ApiDefinitionLink, Error> {
        Ok(ApiDefinitionLink {
            target_uri: value.uri.clone(),
            target_range: Range::to(&value.range)?,
            target_selection_range: Range::to_optional(value.target_selection_range.as_ref())?,
            origin_selection_range: Range::to_optional(value.origin_selection_range.as_ref())?,
        })
    }
}

/// `typeConverters.Hover`.
pub struct Hover;

impl Hover {
    pub fn from(hover: &VerboseHover) -> HoverDto {
        HoverDto {
            range: Range::from_optional(hover.hover.range.as_ref()),
            contents: MarkdownString::from_many(&hover.hover.contents),
            can_increase_verbosity: hover.can_increase_verbosity,
            can_decrease_verbosity: hover.can_decrease_verbosity,
        }
    }

    pub fn to(info: &HoverDto) -> Result<VerboseHover, Error> {
        let contents = info.contents.iter().map(|content| MarkedString::Markdown(MarkdownString::to(content))).collect();
        let range = Range::to_optional(info.range.as_ref())?;
        Ok(VerboseHover::new(contents, range, info.can_increase_verbosity, info.can_decrease_verbosity))
    }
}

/// `typeConverters.MultiDocumentHighlight`.
pub struct MultiDocumentHighlight;

impl MultiDocumentHighlight {
    pub fn from(multi_document_highlight: &types::MultiDocumentHighlight) -> MultiDocumentHighlightDto {
        MultiDocumentHighlightDto {
            uri: multi_document_highlight.uri.clone(),
            highlights: multi_document_highlight.highlights.iter().map(DocumentHighlight::from).collect(),
        }
    }

    pub fn to(multi_document_highlight: &MultiDocumentHighlightDto) -> Result<types::MultiDocumentHighlight, Error> {
        let highlights = multi_document_highlight.highlights.iter().map(DocumentHighlight::to).collect::<Result<Vec<_>, _>>()?;
        Ok(types::MultiDocumentHighlight { uri: multi_document_highlight.uri.clone(), highlights })
    }
}

/// `typeConverters.WorkspaceSymbol` (só o `from`).
pub struct WorkspaceSymbol;

impl WorkspaceSymbol {
    pub fn from(info: &types::SymbolInformation) -> IWorkspaceSymbolDto {
        IWorkspaceSymbolDto {
            cache_id: None,
            name: info.name.clone(),
            container_name: info.container_name.clone(),
            kind: SymbolKind::from(info.kind),
            tags: info.tags.as_ref().map(|tags| tags.iter().copied().map(SymbolTag::from).collect()),
            location: Location::from(&info.location),
        }
    }
}

/// `typeConverters.EvaluatableExpression` (só o `from`).
pub struct EvaluatableExpression;

impl EvaluatableExpression {
    pub fn from(expression: &types::EvaluatableExpression) -> EvaluatableExpressionDto {
        EvaluatableExpressionDto { range: Range::from(&expression.range), expression: expression.expression.clone() }
    }
}

/// `typeConverters.InlineValue` (só o `from`).
pub struct InlineValue;

impl InlineValue {
    pub fn from(inline_value: &ApiInlineValue) -> InlineValueDto {
        match inline_value {
            ApiInlineValue::Text(text) => InlineValueDto::Text { range: Range::from(&text.range), text: text.text.clone() },
            ApiInlineValue::VariableLookup(lookup) => InlineValueDto::Variable {
                range: Range::from(&lookup.range),
                variable_name: lookup.variable_name.clone(),
                case_sensitive_lookup: lookup.case_sensitive_lookup,
            },
            ApiInlineValue::EvaluatableExpression(expression) => {
                InlineValueDto::Expression { range: Range::from(&expression.range), expression: expression.expression.clone() }
            }
        }
    }
}

/// `typeConverters.InlineValueContext` (só o `to`).
pub struct InlineValueContext;

impl InlineValueContext {
    pub fn to(inline_value_context: &IInlineValueContextDto) -> Result<types::InlineValueContext, Error> {
        Ok(types::InlineValueContext::new(inline_value_context.frame_id, Range::to(&inline_value_context.stopped_location)?))
    }
}

/// `typeConverters.CodeActionTriggerKind` (só o `to`).
pub struct CodeActionTriggerKind;

impl CodeActionTriggerKind {
    pub fn to(value: CodeActionTriggerType) -> ApiCodeActionTriggerKind {
        match value {
            CodeActionTriggerType::Invoke => ApiCodeActionTriggerKind::Invoke,
            CodeActionTriggerType::Auto => ApiCodeActionTriggerKind::Automatic,
        }
    }
}

/// `typeConverters.CompletionTriggerKind` (só o `to`).
pub struct CompletionTriggerKind;

impl CompletionTriggerKind {
    pub fn to(kind: EditorCompletionTriggerKind) -> types::CompletionTriggerKind {
        match kind {
            EditorCompletionTriggerKind::TriggerCharacter => types::CompletionTriggerKind::TriggerCharacter,
            EditorCompletionTriggerKind::TriggerForIncompleteCompletions => types::CompletionTriggerKind::TriggerForIncompleteCompletions,
            EditorCompletionTriggerKind::Invoke => types::CompletionTriggerKind::Invoke,
        }
    }
}

/// `typeConverters.CompletionContext` (só o `to`).
pub struct CompletionContext;

impl CompletionContext {
    pub fn to(context: &EditorCompletionContext) -> types::CompletionContext {
        types::CompletionContext { trigger_kind: CompletionTriggerKind::to(context.trigger_kind), trigger_character: context.trigger_character.clone() }
    }
}

/// `typeConverters.CompletionItemTag`.
pub struct CompletionItemTag;

impl CompletionItemTag {
    pub fn from(kind: types::CompletionItemTag) -> EditorCompletionItemTag {
        match kind {
            types::CompletionItemTag::Deprecated => EditorCompletionItemTag::Deprecated,
        }
    }

    pub fn to(kind: EditorCompletionItemTag) -> types::CompletionItemTag {
        match kind {
            EditorCompletionItemTag::Deprecated => types::CompletionItemTag::Deprecated,
        }
    }
}

/// `typeConverters.CompletionItemKind`.
pub struct CompletionItemKind;

impl CompletionItemKind {
    /// `to(kind)`: os dois membros do editor sem par na API (`Customcolor` e `Tool`) são o `?? Property` do original.
    pub fn to(kind: EditorCompletionItemKind) -> types::CompletionItemKind {
        use EditorCompletionItemKind as Editor;
        use types::CompletionItemKind as Api;
        match kind {
            Editor::Method => Api::Method,
            Editor::Function => Api::Function,
            Editor::Constructor => Api::Constructor,
            Editor::Field => Api::Field,
            Editor::Variable => Api::Variable,
            Editor::Class => Api::Class,
            Editor::Interface => Api::Interface,
            Editor::Struct => Api::Struct,
            Editor::Module => Api::Module,
            Editor::Property => Api::Property,
            Editor::Unit => Api::Unit,
            Editor::Value => Api::Value,
            Editor::Constant => Api::Constant,
            Editor::Enum => Api::Enum,
            Editor::EnumMember => Api::EnumMember,
            Editor::Keyword => Api::Keyword,
            Editor::Snippet => Api::Snippet,
            Editor::Text => Api::Text,
            Editor::Color => Api::Color,
            Editor::File => Api::File,
            Editor::Reference => Api::Reference,
            Editor::Folder => Api::Folder,
            Editor::Event => Api::Event,
            Editor::Operator => Api::Operator,
            Editor::TypeParameter => Api::TypeParameter,
            Editor::User => Api::User,
            Editor::Issue => Api::Issue,
            Editor::Customcolor | Editor::Tool => Api::Property,
        }
    }
    pub fn from(kind: types::CompletionItemKind) -> EditorCompletionItemKind {
        use types::CompletionItemKind as Api;
        match kind {
            Api::Method => EditorCompletionItemKind::Method,
            Api::Function => EditorCompletionItemKind::Function,
            Api::Constructor => EditorCompletionItemKind::Constructor,
            Api::Field => EditorCompletionItemKind::Field,
            Api::Variable => EditorCompletionItemKind::Variable,
            Api::Class => EditorCompletionItemKind::Class,
            Api::Interface => EditorCompletionItemKind::Interface,
            Api::Struct => EditorCompletionItemKind::Struct,
            Api::Module => EditorCompletionItemKind::Module,
            Api::Property => EditorCompletionItemKind::Property,
            Api::Unit => EditorCompletionItemKind::Unit,
            Api::Value => EditorCompletionItemKind::Value,
            Api::Constant => EditorCompletionItemKind::Constant,
            Api::Enum => EditorCompletionItemKind::Enum,
            Api::EnumMember => EditorCompletionItemKind::EnumMember,
            Api::Keyword => EditorCompletionItemKind::Keyword,
            Api::Snippet => EditorCompletionItemKind::Snippet,
            Api::Text => EditorCompletionItemKind::Text,
            Api::Color => EditorCompletionItemKind::Color,
            Api::File => EditorCompletionItemKind::File,
            Api::Reference => EditorCompletionItemKind::Reference,
            Api::Folder => EditorCompletionItemKind::Folder,
            Api::Event => EditorCompletionItemKind::Event,
            Api::Operator => EditorCompletionItemKind::Operator,
            Api::TypeParameter => EditorCompletionItemKind::TypeParameter,
            Api::Issue => EditorCompletionItemKind::Issue,
            Api::User => EditorCompletionItemKind::User,
        }
    }
}

/// `typeConverters.CompletionItem`.
pub struct CompletionItem;

impl CompletionItem {
    /// `to(suggestion, converter?)`: devolve [`Result`], porque `Range.to` e `TextEdit.to` validam. As edições adicionais
    /// (`SingleEditOperation`) lidas como `languages.TextEdit` (`{ range, text }`) não têm `eol`.
    pub fn to(suggestion: &EditorCompletionItem, converter: Option<&dyn ICommandsConverter>) -> Result<types::CompletionItem, Error> {
        let label = match &suggestion.label {
            EditorCompletionLabel::Text(text) => types::CompletionLabel::Text(text.clone()),
            EditorCompletionLabel::Item(item) => types::CompletionLabel::Detailed(types::CompletionItemLabel {
                label: item.label.clone(),
                detail: item.detail.clone(),
                description: item.description.clone(),
            }),
        };
        let mut result = types::CompletionItem::new(label, Some(CompletionItemKind::to(suggestion.kind)));
        result.tags = suggestion.tags.as_ref().map(|tags| tags.iter().copied().map(CompletionItemTag::to).collect());
        result.detail = suggestion.detail.clone();
        result.documentation = suggestion.documentation.as_ref().map(documentation_to);
        result.sort_text = suggestion.sort_text.clone();
        result.filter_text = suggestion.filter_text.clone();
        result.preselect = suggestion.preselect;
        result.commit_characters = suggestion.commit_characters.clone();

        // range
        result.range = Some(match &suggestion.range {
            EditorCompletionItemRange::Single(range) => types::CompletionRange::Range(Range::to(range)?),
            EditorCompletionItemRange::InsertReplace { insert, replace } => {
                types::CompletionRange::InsertReplace { inserting: Range::to(insert)?, replacing: Range::to(replace)? }
            }
        });

        result.keep_whitespace = Some(
            suggestion
                .insert_text_rules
                .is_some_and(|rules| rules & completion_item_insert_text_rule::KEEP_WHITESPACE != 0),
        );
        // 'insertText'-logic
        if suggestion.insert_text_rules.is_some_and(|rules| rules & completion_item_insert_text_rule::INSERT_AS_SNIPPET != 0) {
            result.insert_text = Some(types::InsertText::Snippet(types::SnippetString::new(Some(suggestion.insert_text.as_str()))));
        } else {
            result.insert_text = Some(types::InsertText::Text(suggestion.insert_text.clone()));
            result.text_edit = match &result.range {
                Some(types::CompletionRange::Range(range)) => Some(types::TextEdit::new(*range, Some(suggestion.insert_text.as_str()))),
                _ => None,
            };
        }
        if let Some(edits) = suggestion.additional_text_edits.as_ref().filter(|edits| !edits.is_empty()) {
            result.additional_text_edits = Some(
                edits
                    .iter()
                    .map(|edit| {
                        TextEdit::to(&EditorTextEdit {
                            range: edit.range,
                            text: edit.text.as_ref().map(|text| text.to_string_lossy()).unwrap_or_default(),
                            eol: None,
                            insert_as_snippet: None,
                            keep_whitespace: None,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            );
        }
        result.command = match (converter, &suggestion.command) {
            (Some(converter), Some(command)) => converter.from_internal(&ICommandDto {
                ident: None,
                id: command.id.clone(),
                title: command.title.clone(),
                tooltip: command.tooltip.clone(),
                arguments: command.arguments.clone(),
            }),
            _ => None,
        };

        Ok(result)
    }
}

/// `htmlContent.isMarkdownString(documentation) ? MarkdownString.to(documentation) : documentation`.
fn documentation_to(documentation: &StringOrMarkdown) -> Documentation {
    match documentation {
        StringOrMarkdown::String(text) => Documentation::Text(text.clone()),
        StringOrMarkdown::Markdown(markdown) => Documentation::Markdown(MarkdownString::to(markdown)),
    }
}

/// `typeConverters.ParameterInformation`.
pub struct ParameterInformation;

impl ParameterInformation {
    pub fn from(info: &types::ParameterInformation) -> EditorParameterInformation {
        EditorParameterInformation {
            label: match &info.label {
                types::ParameterLabel::Text(text) => EditorParameterLabel::Text(text.clone()),
                types::ParameterLabel::Offsets(start, end) => EditorParameterLabel::Range(*start as u32, *end as u32),
            },
            documentation: MarkdownString::from_strict(info.documentation.as_ref()),
        }
    }

    pub fn to(info: &EditorParameterInformation) -> types::ParameterInformation {
        types::ParameterInformation {
            label: match &info.label {
                EditorParameterLabel::Text(text) => types::ParameterLabel::Text(text.clone()),
                EditorParameterLabel::Range(start, end) => types::ParameterLabel::Offsets(f64::from(*start), f64::from(*end)),
            },
            documentation: info.documentation.as_ref().map(documentation_to),
        }
    }
}

/// `typeConverters.SignatureInformation`.
pub struct SignatureInformation;

impl SignatureInformation {
    pub fn from(info: &types::SignatureInformation) -> EditorSignatureInformation {
        EditorSignatureInformation {
            label: info.label.clone(),
            documentation: MarkdownString::from_strict(info.documentation.as_ref()),
            parameters: info.parameters.iter().map(ParameterInformation::from).collect(),
            active_parameter: info.active_parameter.map(|active_parameter| active_parameter as i32),
        }
    }

    pub fn to(info: &EditorSignatureInformation) -> types::SignatureInformation {
        types::SignatureInformation {
            label: info.label.clone(),
            documentation: info.documentation.as_ref().map(documentation_to),
            parameters: info.parameters.iter().map(ParameterInformation::to).collect(),
            active_parameter: info.active_parameter.map(f64::from),
        }
    }
}

/// `typeConverters.SignatureHelp`.
pub struct SignatureHelp;

impl SignatureHelp {
    pub fn from(help: &types::SignatureHelp) -> EditorSignatureHelp {
        EditorSignatureHelp {
            active_signature: help.active_signature as i32,
            active_parameter: help.active_parameter as i32,
            signatures: help.signatures.iter().map(SignatureInformation::from).collect(),
        }
    }

    pub fn to(help: &EditorSignatureHelp) -> types::SignatureHelp {
        types::SignatureHelp {
            active_signature: f64::from(help.active_signature),
            active_parameter: f64::from(help.active_parameter),
            signatures: help.signatures.iter().map(SignatureInformation::to).collect(),
        }
    }
}

/// `typeConverters.DocumentLink` (só o `from`).
pub struct DocumentLink;

impl DocumentLink {
    pub fn from(link: &types::DocumentLink) -> ILinkDto {
        ILinkDto { cache_id: None, range: Range::from(&link.range), url: link.target.clone(), tooltip: link.tooltip.clone() }
    }
}

/// `typeConverters.ColorPresentation`.
pub struct ColorPresentation;

impl ColorPresentation {
    pub fn to(color_presentation: &IColorPresentationDto) -> Result<types::ColorPresentation, Error> {
        let mut cp = types::ColorPresentation::new(&color_presentation.label)?;
        if let Some(text_edit) = &color_presentation.text_edit {
            cp.text_edit = Some(TextEdit::to(text_edit)?);
        }
        if let Some(additional_text_edits) = &color_presentation.additional_text_edits {
            cp.additional_text_edits = Some(additional_text_edits.iter().map(TextEdit::to).collect::<Result<Vec<_>, _>>()?);
        }
        Ok(cp)
    }

    pub fn from(color_presentation: &types::ColorPresentation) -> IColorPresentationDto {
        IColorPresentationDto {
            label: color_presentation.label.clone(),
            text_edit: color_presentation.text_edit.as_ref().map(TextEdit::from),
            additional_text_edits: color_presentation.additional_text_edits.as_ref().map(|edits| edits.iter().map(TextEdit::from).collect()),
        }
    }
}

/// `typeConverters.Color`.
pub struct Color;

impl Color {
    pub fn to(c: [f64; 4]) -> types::Color {
        types::Color::new(c[0], c[1], c[2], c[3])
    }

    pub fn from(color: &types::Color) -> [f64; 4] {
        [color.red, color.green, color.blue, color.alpha]
    }
}

/// `typeConverters.SelectionRange` (só o `from`).
pub struct SelectionRange;

impl SelectionRange {
    pub fn from(obj: &types::SelectionRange) -> EditorSelectionRange {
        EditorSelectionRange { range: Range::from(&obj.range) }
    }
}

/// `typeConverters.FoldingRangeKind` (só o `from`).
pub struct FoldingRangeKind;

impl FoldingRangeKind {
    pub fn from(kind: Option<types::FoldingRangeKind>) -> Option<EditorFoldingRangeKind> {
        kind.map(|kind| match kind {
            types::FoldingRangeKind::Comment => EditorFoldingRangeKind::comment(),
            types::FoldingRangeKind::Imports => EditorFoldingRangeKind::imports(),
            types::FoldingRangeKind::Region => EditorFoldingRangeKind::region(),
        })
    }
}

/// `typeConverters.FoldingRange` (só o `from`).
pub struct FoldingRange;

impl FoldingRange {
    pub fn from(r: &types::FoldingRange) -> EditorFoldingRange {
        EditorFoldingRange { start: r.start as i32 + 1, end: r.end as i32 + 1, kind: FoldingRangeKind::from(r.kind) }
    }
}

/// `typeConverters.InlayHintKind`.
pub struct InlayHintKind;

impl InlayHintKind {
    pub fn from(kind: types::InlayHintKind) -> EditorInlayHintKind {
        match kind {
            types::InlayHintKind::Type => EditorInlayHintKind::Type,
            types::InlayHintKind::Parameter => EditorInlayHintKind::Parameter,
        }
    }

    pub fn to(kind: EditorInlayHintKind) -> types::InlayHintKind {
        match kind {
            EditorInlayHintKind::Type => types::InlayHintKind::Type,
            EditorInlayHintKind::Parameter => types::InlayHintKind::Parameter,
        }
    }
}

/// `typeConverters.InlayHintLabelPart`.
pub struct InlayHintLabelPart;

impl InlayHintLabelPart {
    /// `to(converter, part)`: devolve [`Result`], porque `location.to` valida o intervalo.
    pub fn to(converter: &dyn ICommandsConverter, part: &EditorInlayHintLabelPart) -> Result<types::InlayHintLabelPart, Error> {
        let mut result = types::InlayHintLabelPart::new(part.label.clone());
        result.tooltip = part.tooltip.as_ref().map(documentation_to);
        if let Some(command) = &part.command {
            result.command = converter.from_internal(&ICommandDto {
                ident: None,
                id: command.id.clone(),
                title: command.title.clone(),
                tooltip: command.tooltip.clone(),
                arguments: command.arguments.clone(),
            });
        }
        if let Some(location) = &part.location {
            result.location = Some(Location::to(&LocationDto { uri: Some(location.uri.clone()), range: Some(location.range) })?);
        }
        Ok(result)
    }
}

/// `typeConverters.InlayHint`.
pub struct InlayHint;

impl InlayHint {
    /// `to(converter, hint)`: devolve [`Result`], porque `Position.to`, `TextEdit.to` e `location.to` validam.
    pub fn to(converter: &dyn ICommandsConverter, hint: &EditorInlayHint) -> Result<types::InlayHint, Error> {
        let label = match &hint.label {
            EditorInlayHintLabel::Text(text) => types::InlayHintLabel::Text(text.clone()),
            EditorInlayHintLabel::Parts(parts) => {
                types::InlayHintLabel::Parts(parts.iter().map(|part| InlayHintLabelPart::to(converter, part)).collect::<Result<Vec<_>, _>>()?)
            }
        };
        let mut result = types::InlayHint::new(Position::to(&hint.position)?, label, hint.kind.map(InlayHintKind::to));
        result.text_edits = hint.text_edits.as_ref().map(|edits| edits.iter().map(TextEdit::to).collect::<Result<Vec<_>, _>>()).transpose()?;
        result.tooltip = hint.tooltip.as_ref().map(documentation_to);
        result.padding_left = hint.padding_left;
        result.padding_right = hint.padding_right;
        Ok(result)
    }
}
