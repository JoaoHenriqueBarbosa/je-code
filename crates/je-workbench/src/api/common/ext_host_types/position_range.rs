//! `extHostTypes/position.ts`, `range.ts`, `selection.ts` e `location.ts`.
//!
//! Divergências:
//! - `Position`, `Range` e `Selection` são valores (`Copy`/`Clone`), não objetos com identidade. O `===` do
//!   original (`translate()` devolve o próprio `this`, `Range#union` devolve `this`) vira igualdade de
//!   valor: o resultado é o mesmo valor, e nada observável depende de ser a mesma instância. A única
//!   leitura de identidade do original, `Selection#isReversed` (`_anchor === _end`), é `!anchor.isBefore(active)`.
//! - Os números da API (`number`) são [`f64`].
//! - `null`/`undefined` onde o tipo não aceita (`new Range(pos, undefined)`, `translate(null)`) não
//!   compilam em Rust; os casos de `undefined` aceito viram [`Option`]. Os campos privados com `get` sem
//!   `set` (`line`, `character`, `start`, `end`) não têm setter, então a escrita que o original rejeita em
//!   tempo de execução é erro de compilação.
//! - As sobrecargas `translate(change)`, `with(change)` e `Range#with(change)` coincidem com a forma de dois
//!   argumentos (`Option` para cada campo), então há um método só.
//! - `isPosition`/`isRange`/`of` (duck typing sobre objetos com a forma de um `Position`) não existem:
//!   o tipo já garante a forma.
//! - Os `Symbol.for('debug.description')` são `debug_description`.
//! - Construtores que lançam (`illegalArgument`) devolvem [`Result`].

use std::ops::Deref;

use je_base::dynamic::number_to_js_string;
use je_base::errors::Error;
use je_base::json::Value;
use je_base::uri::URI;

use super::{illegal_argument, json_object, uri_to_json};

/// `Position`: linha e caractere, base zero, nunca negativos.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    line: f64,
    character: f64,
}

impl Position {
    /// `Position.Min(...positions)`: lança `TypeError` sem argumentos.
    pub fn min(positions: &[Position]) -> Result<Position, Error> {
        let (first, rest) = positions.split_first().ok_or_else(|| Error::type_error(""))?;
        Ok(rest.iter().fold(*first, |result, p| if p.is_before(&result) { *p } else { result }))
    }

    /// `Position.Max(...positions)`: lança `TypeError` sem argumentos.
    pub fn max(positions: &[Position]) -> Result<Position, Error> {
        let (first, rest) = positions.split_first().ok_or_else(|| Error::type_error(""))?;
        Ok(rest.iter().fold(*first, |result, p| if p.is_after(&result) { *p } else { result }))
    }

    pub fn new(line: f64, character: f64) -> Result<Position, Error> {
        if line < 0.0 {
            return Err(illegal_argument(Some("line must be non-negative")));
        }
        if character < 0.0 {
            return Err(illegal_argument(Some("character must be non-negative")));
        }
        Ok(Position { line, character })
    }

    pub fn line(&self) -> f64 {
        self.line
    }

    pub fn character(&self) -> f64 {
        self.character
    }

    pub fn is_before(&self, other: &Position) -> bool {
        if self.line < other.line {
            return true;
        }
        if other.line < self.line {
            return false;
        }
        self.character < other.character
    }

    pub fn is_before_or_equal(&self, other: &Position) -> bool {
        if self.line < other.line {
            return true;
        }
        if other.line < self.line {
            return false;
        }
        self.character <= other.character
    }

    pub fn is_after(&self, other: &Position) -> bool {
        !self.is_before_or_equal(other)
    }

    pub fn is_after_or_equal(&self, other: &Position) -> bool {
        !self.is_before(other)
    }

    pub fn is_equal(&self, other: &Position) -> bool {
        self.line == other.line && self.character == other.character
    }

    /// `-1`, `0` ou `1`.
    pub fn compare_to(&self, other: &Position) -> i32 {
        if self.line < other.line {
            -1
        } else if self.line > other.line {
            1
        } else if self.character < other.character {
            -1
        } else if self.character > other.character {
            1
        } else {
            0
        }
    }

    /// `translate(lineDelta?, characterDelta?)`; sem deslocamento devolve a própria posição.
    pub fn translate(&self, line_delta: Option<f64>, character_delta: Option<f64>) -> Result<Position, Error> {
        let line_delta = line_delta.unwrap_or(0.0);
        let character_delta = character_delta.unwrap_or(0.0);
        if line_delta == 0.0 && character_delta == 0.0 {
            return Ok(*self);
        }
        Position::new(self.line + line_delta, self.character + character_delta)
    }

    /// `with(line?, character?)`; sem mudança devolve a própria posição.
    pub fn with(&self, line: Option<f64>, character: Option<f64>) -> Result<Position, Error> {
        let line = line.unwrap_or(self.line);
        let character = character.unwrap_or(self.character);
        if line == self.line && character == self.character {
            return Ok(*self);
        }
        Position::new(line, character)
    }

    /// `toJSON()`: `{ line, character }`.
    pub fn to_json(&self) -> Value {
        json_object(vec![("line", Some(Value::Number(self.line))), ("character", Some(Value::Number(self.character)))])
    }

    /// `[Symbol.for('debug.description')]()`.
    pub fn debug_description(&self) -> String {
        format!("({}:{})", number_to_js_string(self.line), number_to_js_string(self.character))
    }
}

/// `Range`: duas posições, sempre ordenadas (`start` antes de `end`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    start: Position,
    end: Position,
}

impl Range {
    /// `new Range(start, end)`: as posições são ordenadas.
    pub fn new(start: Position, end: Position) -> Range {
        if start.is_before(&end) { Range { start, end } } else { Range { start: end, end: start } }
    }

    /// `new Range(startLine, startColumn, endLine, endColumn)`.
    pub fn from_coordinates(start_line: f64, start_column: f64, end_line: f64, end_column: f64) -> Result<Range, Error> {
        Ok(Range::new(Position::new(start_line, start_column)?, Position::new(end_line, end_column)?))
    }

    pub fn start(&self) -> Position {
        self.start
    }

    pub fn end(&self) -> Position {
        self.end
    }

    /// `contains(position)`.
    pub fn contains_position(&self, position: &Position) -> bool {
        !(position.is_before(&self.start) || self.end.is_before(position))
    }

    /// `contains(range)`.
    pub fn contains_range(&self, range: &Range) -> bool {
        self.contains_position(&range.start) && self.contains_position(&range.end)
    }

    pub fn is_equal(&self, other: &Range) -> bool {
        self.start.is_equal(&other.start) && self.end.is_equal(&other.end)
    }

    /// `None` quando não há sobreposição.
    pub fn intersection(&self, other: &Range) -> Option<Range> {
        let start = if self.start.is_after(&other.start) { self.start } else { other.start };
        let end = if self.end.is_before(&other.end) { self.end } else { other.end };
        if start.is_after(&end) {
            // não há sobreposição:
            // |-----|
            //          |----|
            return None;
        }
        Some(Range::new(start, end))
    }

    pub fn union(&self, other: &Range) -> Range {
        if self.contains_range(other) {
            return *self;
        }
        if other.contains_range(self) {
            return *other;
        }
        let start = if self.start.is_before(&other.start) { self.start } else { other.start };
        let end = if self.end.is_after(&other.end) { self.end } else { other.end };
        Range::new(start, end)
    }

    pub fn is_empty(&self) -> bool {
        self.start.is_equal(&self.end)
    }

    pub fn is_single_line(&self) -> bool {
        self.start.line == self.end.line
    }

    /// `with(start?, end?)`; sem mudança devolve o próprio intervalo.
    pub fn with(&self, start: Option<Position>, end: Option<Position>) -> Range {
        let start = start.unwrap_or(self.start);
        let end = end.unwrap_or(self.end);
        if start.is_equal(&self.start) && end.is_equal(&self.end) {
            return *self;
        }
        Range::new(start, end)
    }

    /// `toJSON()`: `[start, end]`.
    pub fn to_json(&self) -> Value {
        Value::Array(vec![self.start.to_json(), self.end.to_json()])
    }

    /// `getDebugDescriptionOfRange`.
    pub fn debug_description(&self) -> String {
        let (start, end) = (&self.start, &self.end);
        let (sl, sc) = (number_to_js_string(start.line), number_to_js_string(start.character));
        if self.is_empty() {
            format!("[{sl}:{sc})")
        } else {
            format!("[{sl}:{sc} -> {}:{})", number_to_js_string(end.line), number_to_js_string(end.character))
        }
    }
}

/// `Selection`: um `Range` com sentido (`anchor` e `active`). Herda tudo de [`Range`] por [`Deref`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    range: Range,
    anchor: Position,
    active: Position,
}

impl Deref for Selection {
    type Target = Range;

    fn deref(&self) -> &Range {
        &self.range
    }
}

impl Selection {
    pub fn new(anchor: Position, active: Position) -> Selection {
        Selection { range: Range::new(anchor, active), anchor, active }
    }

    /// `new Selection(anchorLine, anchorColumn, activeLine, activeColumn)`.
    pub fn from_coordinates(anchor_line: f64, anchor_column: f64, active_line: f64, active_column: f64) -> Result<Selection, Error> {
        Ok(Selection::new(Position::new(anchor_line, anchor_column)?, Position::new(active_line, active_column)?))
    }

    pub fn anchor(&self) -> Position {
        self.anchor
    }

    pub fn active(&self) -> Position {
        self.active
    }

    /// No original `_anchor === _end`: o `Range` ordena as posições, e o `anchor` é o `end` exatamente
    /// quando não vem antes do `active` (inclusive numa seleção vazia).
    pub fn is_reversed(&self) -> bool {
        !self.anchor.is_before(&self.active)
    }

    /// `toJSON()`: `{ start, end, active, anchor }`.
    pub fn to_json(&self) -> Value {
        json_object(vec![
            ("start", Some(self.range.start.to_json())),
            ("end", Some(self.range.end.to_json())),
            ("active", Some(self.active.to_json())),
            ("anchor", Some(self.anchor.to_json())),
        ])
    }

    /// `getDebugDescriptionOfSelection`.
    pub fn debug_description(&self) -> String {
        let range = self.range.debug_description();
        if self.range.is_empty() {
            range
        } else if self.active.is_equal(&self.range.start) {
            format!("|{range}")
        } else {
            format!("{range}|")
        }
    }
}

/// `Range | Position`: o segundo argumento do construtor de `Location`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeOrPosition {
    Range(Range),
    Position(Position),
}

impl From<Range> for RangeOrPosition {
    fn from(range: Range) -> Self {
        RangeOrPosition::Range(range)
    }
}

impl From<Position> for RangeOrPosition {
    fn from(position: Position) -> Self {
        RangeOrPosition::Position(position)
    }
}

/// `Location`: um `Range` num recurso. `uri` e `range` podem faltar (o construtor antigo de
/// `SymbolInformation` cria uma `Location` sem `uri`, e `new Location(uri, undefined)` é aceito).
#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub uri: Option<URI>,
    pub range: Option<Range>,
}

impl Location {
    pub fn new(uri: Option<URI>, range_or_position: Option<RangeOrPosition>) -> Location {
        let range = range_or_position.map(|value| match value {
            RangeOrPosition::Range(range) => range,
            RangeOrPosition::Position(position) => Range::new(position, position),
        });
        Location { uri, range }
    }

    /// `toJSON()`: `{ uri, range }`, sem os campos `undefined`.
    pub fn to_json(&self) -> Value {
        json_object(vec![("uri", self.uri.as_ref().map(uri_to_json)), ("range", self.range.as_ref().map(Range::to_json))])
    }
}
