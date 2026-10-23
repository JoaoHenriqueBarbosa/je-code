//! Porte de `src/vs/editor/common/core/selection.ts`.
//!
//! `Selection extends Range`: aqui a seleção guarda o [`Range`] ordenado e a âncora/posição como no
//! original, e deriva para o `Range` (`Deref`), então os métodos do intervalo valem nela. Os que o
//! original sobrescreve (`setEndPosition`, `setStartPosition`) devolvem `Selection`.

use std::fmt;
use std::ops::Deref;

use crate::position::Position;
use crate::range::Range;

/// `SelectionDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionDirection {
    /// A âncora vem antes da posição.
    Ltr,
    /// A posição vem antes da âncora.
    Rtl,
}

/// Uma seleção: uma âncora (`selection_start_*`) e a posição do cursor (`position_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Selection {
    range: Range,
    pub selection_start_line_number: i32,
    pub selection_start_column: i32,
    pub position_line_number: i32,
    pub position_column: i32,
}

impl Deref for Selection {
    type Target = Range;

    fn deref(&self) -> &Range {
        &self.range
    }
}

impl Selection {
    pub const fn new(
        selection_start_line_number: i32,
        selection_start_column: i32,
        position_line_number: i32,
        position_column: i32,
    ) -> Self {
        Selection {
            range: Range::new(
                selection_start_line_number,
                selection_start_column,
                position_line_number,
                position_column,
            ),
            selection_start_line_number,
            selection_start_column,
            position_line_number,
            position_column,
        }
    }

    pub fn get_direction(&self) -> SelectionDirection {
        if self.selection_start_line_number == self.start_line_number && self.selection_start_column == self.start_column {
            SelectionDirection::Ltr
        } else {
            SelectionDirection::Rtl
        }
    }

    pub fn set_end_position(&self, end_line_number: i32, end_column: i32) -> Selection {
        if self.get_direction() == SelectionDirection::Ltr {
            return Selection::new(self.start_line_number, self.start_column, end_line_number, end_column);
        }
        Selection::new(end_line_number, end_column, self.start_line_number, self.start_column)
    }

    pub fn get_position(&self) -> Position {
        Position::new(self.position_line_number, self.position_column)
    }

    pub fn get_selection_start(&self) -> Position {
        Position::new(self.selection_start_line_number, self.selection_start_column)
    }

    pub fn set_start_position(&self, start_line_number: i32, start_column: i32) -> Selection {
        if self.get_direction() == SelectionDirection::Ltr {
            return Selection::new(start_line_number, start_column, self.end_line_number, self.end_column);
        }
        Selection::new(self.end_line_number, self.end_column, start_line_number, start_column)
    }

    /// `Selection.fromPositions(start, end)`.
    pub fn from_positions(start: Position, end: Position) -> Selection {
        Selection::new(start.line_number, start.column, end.line_number, end.column)
    }

    pub fn from_range(range: &Range, direction: SelectionDirection) -> Selection {
        if direction == SelectionDirection::Ltr {
            Selection::new(range.start_line_number, range.start_column, range.end_line_number, range.end_column)
        } else {
            Selection::new(range.end_line_number, range.end_column, range.start_line_number, range.start_column)
        }
    }

    pub fn create_with_direction(
        start_line_number: i32,
        start_column: i32,
        end_line_number: i32,
        end_column: i32,
        direction: SelectionDirection,
    ) -> Selection {
        if direction == SelectionDirection::Ltr {
            return Selection::new(start_line_number, start_column, end_line_number, end_column);
        }
        Selection::new(end_line_number, end_column, start_line_number, start_column)
    }
}

impl fmt::Display for Selection {
    /// `toString()`: `[âncora -> posição]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{},{} -> {},{}]",
            self.selection_start_line_number, self.selection_start_column, self.position_line_number, self.position_column
        )
    }
}
