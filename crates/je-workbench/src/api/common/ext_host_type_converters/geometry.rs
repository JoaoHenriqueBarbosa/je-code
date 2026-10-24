//! `extHostTypeConverters.ts`: `Selection`, `Range`, `Position`, `Location` e `NotebookRange`, os conversores
//! entre as classes da API (`ext_host_types`, base zero) e as formas do editor (`je_editor`, base um) e do
//! protocolo.
//!
//! Divergências:
//! - Os DTOs `IPosition`, `IRange` e `ISelection` são os tipos do editor ([`EditorPosition`], [`EditorRange`],
//!   [`EditorSelection`]). `Range.from` escreve os campos sem ordenar (a classe da API já está ordenada);
//!   `Range.to` passa pelo construtor da API, que ordena, como no original. Para os valores que o host lê de
//!   um [`Dynamic`] (os argumentos de comando, que chegam como objetos JSON), há as formas
//!   `*_dto`/`*_from_dto`: a de leitura é o `isIRange`/`isIPosition` do original e devolve `None` quando o
//!   valor não tem a forma.
//! - `Range.from(undefined)` e `Range.to(undefined)` são [`Range::from_optional`] e [`Range::to_optional`].
//! - Os `Position.to`, `Range.to` e `Selection.to` lançam no original quando o DTO tem número negativo (o
//!   construtor da API valida); aqui devolvem [`Result`].
//! - `Location` (linha 148 do original) e `location` (linha 947) fazem a mesma coisa: a segunda tolera um
//!   `range` ausente. Existe um conversor só, [`Location`], com a forma tolerante; o `Dto<languages.Location>`
//!   é [`LocationDto`], com o `uri` e o `range` opcionais.
//! - `ICellRange` (`notebookRange.ts`) ainda não foi portado: [`CellRange`] é o par `{ start, end }`.

use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::uri::URI;
use je_editor::position::Position as EditorPosition;
use je_editor::range::Range as EditorRange;
use je_editor::selection::Selection as EditorSelection;

use crate::api::common::ext_host_types as types;
use crate::api::common::ext_host_values::{ApiValue, is_instance_of};

/// O `i32` de um número da API (um `number` que o original soma a 1 sem checar).
fn int(value: f64) -> i32 {
    value as i32
}

/// `typeof value[key] === 'number'`: o número de uma propriedade, ou `None`.
fn number_property(value: &Dynamic, key: &str) -> Option<i32> {
    match value {
        Dynamic::Object(_) => match value.get_str(key) {
            Dynamic::Number(n) => Some(int(n)),
            _ => None,
        },
        _ => None,
    }
}

/// `typeConverters.Selection`.
pub struct Selection;

impl Selection {
    pub fn to(selection: &EditorSelection) -> Result<types::Selection, Error> {
        let start = types::Position::new(f64::from(selection.selection_start_line_number - 1), f64::from(selection.selection_start_column - 1))?;
        let end = types::Position::new(f64::from(selection.position_line_number - 1), f64::from(selection.position_column - 1))?;
        Ok(types::Selection::new(start, end))
    }

    pub fn from(selection: &types::Selection) -> EditorSelection {
        let (anchor, active) = (selection.anchor(), selection.active());
        EditorSelection::new(int(anchor.line()) + 1, int(anchor.character()) + 1, int(active.line()) + 1, int(active.character()) + 1)
    }

    /// O `ISelection` como objeto (`selectionStartLineNumber`, `selectionStartColumn`, `positionLineNumber`,
    /// `positionColumn`).
    pub fn to_dto(selection: &EditorSelection) -> Dynamic {
        Dynamic::object([
            ("selectionStartLineNumber", Dynamic::from(selection.selection_start_line_number)),
            ("selectionStartColumn", Dynamic::from(selection.selection_start_column)),
            ("positionLineNumber", Dynamic::from(selection.position_line_number)),
            ("positionColumn", Dynamic::from(selection.position_column)),
        ])
    }
}

/// `typeConverters.Range`.
pub struct Range;

impl Range {
    pub fn from(range: &types::Range) -> EditorRange {
        let (start, end) = (range.start(), range.end());
        EditorRange {
            start_line_number: int(start.line()) + 1,
            start_column: int(start.character()) + 1,
            end_line_number: int(end.line()) + 1,
            end_column: int(end.character()) + 1,
        }
    }

    pub fn from_optional(range: Option<&types::Range>) -> Option<EditorRange> {
        range.map(Range::from)
    }

    pub fn to(range: &EditorRange) -> Result<types::Range, Error> {
        types::Range::from_coordinates(
            f64::from(range.start_line_number - 1),
            f64::from(range.start_column - 1),
            f64::from(range.end_line_number - 1),
            f64::from(range.end_column - 1),
        )
    }

    pub fn to_optional(range: Option<&EditorRange>) -> Result<Option<types::Range>, Error> {
        range.map(Range::to).transpose()
    }

    /// O `IRange` como objeto (`startLineNumber`, `startColumn`, `endLineNumber`, `endColumn`).
    pub fn to_dto(range: &EditorRange) -> Dynamic {
        Dynamic::object([
            ("startLineNumber", Dynamic::from(range.start_line_number)),
            ("startColumn", Dynamic::from(range.start_column)),
            ("endLineNumber", Dynamic::from(range.end_line_number)),
            ("endColumn", Dynamic::from(range.end_column)),
        ])
    }

    /// `Range.isIRange(value)` e a leitura dos quatro campos, sem ordenar; `None` se o valor não tem a forma.
    pub fn from_dto(value: &Dynamic) -> Option<EditorRange> {
        Some(EditorRange {
            start_line_number: number_property(value, "startLineNumber")?,
            start_column: number_property(value, "startColumn")?,
            end_line_number: number_property(value, "endLineNumber")?,
            end_column: number_property(value, "endColumn")?,
        })
    }
}

/// `typeConverters.Position`.
pub struct Position;

impl Position {
    pub fn to(position: &EditorPosition) -> Result<types::Position, Error> {
        types::Position::new(f64::from(position.line_number - 1), f64::from(position.column - 1))
    }

    pub fn from(position: &types::Position) -> EditorPosition {
        EditorPosition::new(int(position.line()) + 1, int(position.character()) + 1)
    }

    /// O `IPosition` como objeto (`lineNumber`, `column`).
    pub fn to_dto(position: &EditorPosition) -> Dynamic {
        Dynamic::object([("lineNumber", Dynamic::from(position.line_number)), ("column", Dynamic::from(position.column))])
    }

    /// `Position.isIPosition(value)` e a leitura dos dois campos; `None` se o valor não tem a forma.
    pub fn from_dto(value: &Dynamic) -> Option<EditorPosition> {
        Some(EditorPosition::new(number_property(value, "lineNumber")?, number_property(value, "column")?))
    }
}

/// `Dto<languages.Location>`: o recurso e o intervalo de uma localização. O `uri` e o `range` são opcionais
/// porque a classe da API aceita uma `Location` sem eles.
#[derive(Clone, Debug, PartialEq)]
pub struct LocationDto {
    pub uri: Option<URI>,
    pub range: Option<EditorRange>,
}

/// `typeConverters.Location` e `typeConverters.location`.
pub struct Location;

impl Location {
    pub fn from(location: &types::Location) -> LocationDto {
        LocationDto { uri: location.uri.clone(), range: Range::from_optional(location.range.as_ref()) }
    }

    pub fn to(location: &LocationDto) -> Result<types::Location, Error> {
        let range = Range::to_optional(location.range.as_ref())?;
        Ok(types::Location::new(location.uri.clone(), range.map(types::RangeOrPosition::Range)))
    }

    /// O `LocationDto` como objeto (`uri`, `range`), com o `uri` como valor `Uri`.
    pub fn to_dto(location: &LocationDto) -> Dynamic {
        let entries = [("uri", location.uri.as_ref().map(ApiValue::to_dynamic)), ("range", location.range.as_ref().map(Range::to_dto))];
        Dynamic::object(entries.into_iter().filter_map(|(key, value)| value.map(|value| (key, value))))
    }

    /// A leitura do objeto que `Range.isIRange(obj.range) && URI.isUri(obj.uri)` reconhece; `None` se o valor
    /// não tem a forma.
    pub fn from_dto(value: &Dynamic) -> Option<LocationDto> {
        if !matches!(value, Dynamic::Object(_)) {
            return None;
        }
        let range = Range::from_dto(&value.get_str("range"))?;
        let uri = value.get_str("uri");
        if !is_instance_of(&uri, "Uri") {
            return None;
        }
        Some(LocationDto { uri: URI::from_dynamic(&uri).ok(), range: Some(range) })
    }
}

/// `ICellRange`: um intervalo de células de notebook.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellRange {
    pub start: f64,
    pub end: f64,
}

/// `typeConverters.NotebookRange`.
pub struct NotebookRange;

impl NotebookRange {
    pub fn from(range: &types::NotebookRange) -> CellRange {
        CellRange { start: range.start(), end: range.end() }
    }

    pub fn to(range: &CellRange) -> Result<types::NotebookRange, Error> {
        types::NotebookRange::new(range.start, range.end)
    }

    /// O `ICellRange` como objeto (`start`, `end`), sem classe.
    pub fn to_dto(range: &CellRange) -> Dynamic {
        Dynamic::object([("start", Dynamic::Number(range.start)), ("end", Dynamic::Number(range.end))])
    }
}
