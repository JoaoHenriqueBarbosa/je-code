//! Os conversores de `extHostTypeConverters.ts` que `extHostLanguageFeatures.ts` usa e que ainda não existiam:
//! `DocumentSelector`, `DefinitionLink`, `Hover` e `MultiDocumentHighlight`.
//!
//! Divergências:
//! - `DocumentSelector.from` recebe o seletor já tipado ([`DocumentSelectorInput`], o mesmo do conversor
//!   `LanguageSelector`) e devolve os [`IDocumentFilterDto`]. O `asArray(value)` percorre a lista (e achata uma lista
//!   aninhada, que o tipo do original não permite). O `uriTransformer` e a `extension` ausentes são `None`.
//! - `DefinitionLink.from` devolve [`Result`]: o original lança um `TypeError` ao ler o `range` de uma `Location` sem
//!   intervalo, e aqui uma `Location` sem `uri` também é erro, porque o `ILocationLinkDto` exige o recurso.
//! - `Hover.from` recebe o [`VerboseHover`] (ver `ext_host_language_features_api`); o `range` ausente fica ausente no DTO.
//!   `Hover.to` e `MultiDocumentHighlight.to` devolvem [`Result`], porque o construtor de `Range` da API valida.

use je_base::errors::Error;
use je_base::uri_ipc::IURITransformer;
use je_extensions::scanner::ExtensionDescription;

use super::ext_host_language_features_api::{DefinitionLink as ApiDefinitionLink, LocationOrLink};
use super::ext_host_protocol::{HoverDto, IDocumentFilterDto, ILocationLinkDto, MultiDocumentHighlightDto};
use super::ext_host_type_converters::symbols::DocumentHighlight;
use super::ext_host_type_converters::{DocumentSelectorInput, MarkdownString, Range};
use super::ext_host_types::{self as types, MarkedString, VerboseHover};
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
