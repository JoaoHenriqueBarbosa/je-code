//! `extHostTypeConverters.ts`: `DiagnosticTag`, `DiagnosticSeverity`, `DiagnosticRelatedInformation` e `Diagnostic`,
//! entre as classes da API e os marcadores da plataforma (`platform/markers`).
//!
//! Divergências:
//! - O `code` de um `Diagnostic` da API é `string | number` aqui ([`DiagnosticCode`]); a forma
//!   `{ value, target }` que o original também trata (`DiagnosticCode.target`) não existe no tipo. Um `code`
//!   falso (`''`, `0`) é descartado, como o `if (value.code)` do original.
//! - Um `DiagnosticRelatedInformation` cuja `Location` não tem `uri` ou `range` não vira informação
//!   relacionada: o original produziria um objeto sem recurso ou sem posição, que o resto do sistema não
//!   aceita; aqui o erro é `TypeError`.
//! - Os números de linha e coluna do marcador são `i64` e os do editor `i32`; a conversão trunca como o
//!   `number` do original soma 1 sem checar.

use je_base::dynamic::number_to_js_string;
use je_base::errors::Error;
use je_editor::range::Range as EditorRange;
use je_platform::markers::markers::{MarkerCode, MarkerData, MarkerSeverity, MarkerTag, RelatedInformation};

use super::geometry::Range;
use crate::api::common::ext_host_types as types;

/// `typeConverters.DiagnosticTag`.
pub struct DiagnosticTag;

impl DiagnosticTag {
    pub fn from(value: types::DiagnosticTag) -> Option<MarkerTag> {
        match value {
            types::DiagnosticTag::Unnecessary => Some(MarkerTag::Unnecessary),
            types::DiagnosticTag::Deprecated => Some(MarkerTag::Deprecated),
        }
    }

    pub fn to(value: MarkerTag) -> Option<types::DiagnosticTag> {
        match value {
            MarkerTag::Unnecessary => Some(types::DiagnosticTag::Unnecessary),
            MarkerTag::Deprecated => Some(types::DiagnosticTag::Deprecated),
        }
    }
}

/// `typeConverters.DiagnosticSeverity`.
pub struct DiagnosticSeverity;

impl DiagnosticSeverity {
    pub fn from(value: types::DiagnosticSeverity) -> MarkerSeverity {
        match value {
            types::DiagnosticSeverity::Error => MarkerSeverity::ERROR,
            types::DiagnosticSeverity::Warning => MarkerSeverity::WARNING,
            types::DiagnosticSeverity::Information => MarkerSeverity::INFO,
            types::DiagnosticSeverity::Hint => MarkerSeverity::HINT,
        }
    }

    /// Um valor fora dos quatro lê como `Error`, como o `default` do original.
    pub fn to(value: MarkerSeverity) -> types::DiagnosticSeverity {
        match value {
            MarkerSeverity::INFO => types::DiagnosticSeverity::Information,
            MarkerSeverity::WARNING => types::DiagnosticSeverity::Warning,
            MarkerSeverity::HINT => types::DiagnosticSeverity::Hint,
            _ => types::DiagnosticSeverity::Error,
        }
    }
}

/// O `IRange` de um marcador ou de uma informação relacionada, a partir dos campos de linha e coluna.
fn marker_range(start_line: i64, start_column: i64, end_line: i64, end_column: i64) -> EditorRange {
    EditorRange { start_line_number: start_line as i32, start_column: start_column as i32, end_line_number: end_line as i32, end_column: end_column as i32 }
}

/// `typeConverters.DiagnosticRelatedInformation`.
pub struct DiagnosticRelatedInformation;

impl DiagnosticRelatedInformation {
    pub fn from(value: &types::DiagnosticRelatedInformation) -> Result<RelatedInformation, Error> {
        let (Some(resource), Some(range)) = (value.location.uri.clone(), value.location.range.as_ref()) else {
            return Err(Error::type_error("DiagnosticRelatedInformation.location must have a uri and a range"));
        };
        let range = Range::from(range);
        Ok(RelatedInformation {
            resource,
            message: value.message.clone(),
            start_line_number: i64::from(range.start_line_number),
            start_column: i64::from(range.start_column),
            end_line_number: i64::from(range.end_line_number),
            end_column: i64::from(range.end_column),
        })
    }

    pub fn to(value: &RelatedInformation) -> Result<types::DiagnosticRelatedInformation, Error> {
        let range = Range::to(&marker_range(value.start_line_number, value.start_column, value.end_line_number, value.end_column))?;
        Ok(types::DiagnosticRelatedInformation::new(types::Location::new(Some(value.resource.clone()), Some(range.into())), value.message.clone()))
    }
}

/// `typeConverters.Diagnostic`.
pub struct Diagnostic;

impl Diagnostic {
    pub fn from(value: &types::Diagnostic) -> Result<MarkerData, Error> {
        let code = match &value.code {
            Some(types::DiagnosticCode::Text(code)) if !code.is_empty() => Some(MarkerCode::Text(code.clone())),
            Some(types::DiagnosticCode::Number(code)) if *code != 0.0 && !code.is_nan() => Some(MarkerCode::Text(number_to_js_string(*code))),
            _ => None,
        };
        let range = Range::from(&value.range);
        let mut marker = MarkerData::new(
            DiagnosticSeverity::from(value.severity),
            value.message.clone(),
            i64::from(range.start_line_number),
            i64::from(range.start_column),
            i64::from(range.end_line_number),
            i64::from(range.end_column),
        );
        marker.source = value.source.clone();
        marker.code = code;
        marker.related_information = value.related_information.as_ref().map(|items| items.iter().map(DiagnosticRelatedInformation::from).collect::<Result<Vec<_>, _>>()).transpose()?;
        marker.tags = value.tags.as_ref().map(|tags| tags.iter().filter_map(|tag| DiagnosticTag::from(*tag)).collect());
        Ok(marker)
    }

    pub fn to(value: &MarkerData) -> Result<types::Diagnostic, Error> {
        let range = Range::to(&marker_range(value.start_line_number, value.start_column, value.end_line_number, value.end_column))?;
        let mut result = types::Diagnostic::new(range, &value.message, Some(DiagnosticSeverity::to(value.severity)))?;
        result.source = value.source.clone();
        result.code = value.code.as_ref().map(|code| match code {
            MarkerCode::Text(code) => types::DiagnosticCode::Text(code.clone()),
            MarkerCode::Target { value, .. } => types::DiagnosticCode::Text(value.clone()),
        });
        result.related_information = value.related_information.as_ref().map(|items| items.iter().map(DiagnosticRelatedInformation::to).collect::<Result<Vec<_>, _>>()).transpose()?;
        result.tags = value.tags.as_ref().map(|tags| tags.iter().filter_map(|tag| DiagnosticTag::to(*tag)).collect());
        Ok(result)
    }
}
