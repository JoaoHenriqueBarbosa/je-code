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
//! - `InlayHintKind` é o mesmo enum nos dois lados (o original devolve o próprio número).

use je_base::errors::Error;
use je_base::uri_ipc::IURITransformer;
use je_editor::code_action::languages::CodeActionTriggerType;
use je_editor::folding::syntax_range_provider::{FoldingRange as EditorFoldingRange, FoldingRangeKind as EditorFoldingRangeKind};
use je_editor::language_providers::SelectionRange as EditorSelectionRange;
use je_editor::languages::{
    CompletionContext as EditorCompletionContext, CompletionItemKind as EditorCompletionItemKind, CompletionItemTag as EditorCompletionItemTag,
    CompletionTriggerKind as EditorCompletionTriggerKind, StringOrMarkdown,
};
use je_editor::parameter_hints::languages::{
    ParameterInformation as EditorParameterInformation, ParameterLabel as EditorParameterLabel, SignatureHelp as EditorSignatureHelp,
    SignatureInformation as EditorSignatureInformation,
};
use je_extensions::scanner::ExtensionDescription;

use super::ext_host_language_features_api::{ApiInlineValue, DefinitionLink as ApiDefinitionLink, LocationOrLink};
use super::ext_host_protocol::{
    EvaluatableExpressionDto, HoverDto, IColorPresentationDto, IDocumentFilterDto, IInlineValueContextDto, ILinkDto, ILocationLinkDto, IWorkspaceSymbolDto,
    InlineValueDto, MultiDocumentHighlightDto,
};
use super::ext_host_type_converters::symbols::DocumentHighlight;
use super::ext_host_type_converters::{DocumentSelectorInput, Location, MarkdownString, Range, SymbolKind, SymbolTag, TextEdit};
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

/// `typeConverters.CompletionItemTag` (só o `from`).
pub struct CompletionItemTag;

impl CompletionItemTag {
    pub fn from(kind: types::CompletionItemTag) -> EditorCompletionItemTag {
        match kind {
            types::CompletionItemTag::Deprecated => EditorCompletionItemTag::Deprecated,
        }
    }
}

/// `typeConverters.CompletionItemKind` (só o `from`).
pub struct CompletionItemKind;

impl CompletionItemKind {
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

/// `typeConverters.ColorPresentation` (só o `from`).
pub struct ColorPresentation;

impl ColorPresentation {
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
