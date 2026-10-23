//! Porte de `src/vs/editor/common/core/range.ts`.
//!
//! `IRange` e `Range` são o mesmo tipo; as estáticas que só repassam para a instância
//! (`Range.containsPosition(range, p)` e afins) não existem, vale o método. Ficam as estáticas com
//! `null` (`compareRangesUsingStarts`) ou sem instância. `equalsRange` é o `PartialEq`.

use std::fmt;

use crate::position::Position;

/// Um intervalo no editor: início e fim em (linha, coluna), com o início sempre antes do fim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Range {
    pub start_line_number: i32,
    pub start_column: i32,
    pub end_line_number: i32,
    pub end_column: i32,
}

impl Range {
    /// O construtor troca início e fim quando vêm invertidos.
    pub const fn new(start_line_number: i32, start_column: i32, end_line_number: i32, end_column: i32) -> Self {
        if start_line_number > end_line_number || (start_line_number == end_line_number && start_column > end_column) {
            Range {
                start_line_number: end_line_number,
                start_column: end_column,
                end_line_number: start_line_number,
                end_column: start_column,
            }
        } else {
            Range {
                start_line_number,
                start_column,
                end_line_number,
                end_column,
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start_line_number == self.end_line_number && self.start_column == self.end_column
    }

    pub fn contains_position(&self, position: &Position) -> bool {
        if position.line_number < self.start_line_number || position.line_number > self.end_line_number {
            return false;
        }
        if position.line_number == self.start_line_number && position.column < self.start_column {
            return false;
        }
        if position.line_number == self.end_line_number && position.column > self.end_column {
            return false;
        }
        true
    }

    pub fn strict_contains_position(&self, position: &Position) -> bool {
        if position.line_number < self.start_line_number || position.line_number > self.end_line_number {
            return false;
        }
        if position.line_number == self.start_line_number && position.column <= self.start_column {
            return false;
        }
        if position.line_number == self.end_line_number && position.column >= self.end_column {
            return false;
        }
        true
    }

    pub fn contains_range(&self, other: &Range) -> bool {
        if other.start_line_number < self.start_line_number || other.end_line_number < self.start_line_number {
            return false;
        }
        if other.start_line_number > self.end_line_number || other.end_line_number > self.end_line_number {
            return false;
        }
        if other.start_line_number == self.start_line_number && other.start_column < self.start_column {
            return false;
        }
        if other.end_line_number == self.end_line_number && other.end_column > self.end_column {
            return false;
        }
        true
    }

    pub fn strict_contains_range(&self, other: &Range) -> bool {
        if other.start_line_number < self.start_line_number || other.end_line_number < self.start_line_number {
            return false;
        }
        if other.start_line_number > self.end_line_number || other.end_line_number > self.end_line_number {
            return false;
        }
        if other.start_line_number == self.start_line_number && other.start_column <= self.start_column {
            return false;
        }
        if other.end_line_number == self.end_line_number && other.end_column >= self.end_column {
            return false;
        }
        true
    }

    /// `plusRange`: o menor intervalo que contém os dois.
    pub fn plus_range(&self, other: &Range) -> Range {
        let (start_line_number, start_column) = if other.start_line_number < self.start_line_number {
            (other.start_line_number, other.start_column)
        } else if other.start_line_number == self.start_line_number {
            (other.start_line_number, other.start_column.min(self.start_column))
        } else {
            (self.start_line_number, self.start_column)
        };

        let (end_line_number, end_column) = if other.end_line_number > self.end_line_number {
            (other.end_line_number, other.end_column)
        } else if other.end_line_number == self.end_line_number {
            (other.end_line_number, other.end_column.max(self.end_column))
        } else {
            (self.end_line_number, self.end_column)
        };

        Range::new(start_line_number, start_column, end_line_number, end_column)
    }

    /// `intersectRanges`: `None` quando não se cruzam.
    pub fn intersect_ranges(&self, other: &Range) -> Option<Range> {
        let mut result_start_line_number = self.start_line_number;
        let mut result_start_column = self.start_column;
        let mut result_end_line_number = self.end_line_number;
        let mut result_end_column = self.end_column;

        if result_start_line_number < other.start_line_number {
            result_start_line_number = other.start_line_number;
            result_start_column = other.start_column;
        } else if result_start_line_number == other.start_line_number {
            result_start_column = result_start_column.max(other.start_column);
        }

        if result_end_line_number > other.end_line_number {
            result_end_line_number = other.end_line_number;
            result_end_column = other.end_column;
        } else if result_end_line_number == other.end_line_number {
            result_end_column = result_end_column.min(other.end_column);
        }

        // A seleção resultante ficou vazia?
        if result_start_line_number > result_end_line_number {
            return None;
        }
        if result_start_line_number == result_end_line_number && result_start_column > result_end_column {
            return None;
        }
        Some(Range::new(
            result_start_line_number,
            result_start_column,
            result_end_line_number,
            result_end_column,
        ))
    }

    pub fn get_end_position(&self) -> Position {
        Position::new(self.end_line_number, self.end_column)
    }

    pub fn get_start_position(&self) -> Position {
        Position::new(self.start_line_number, self.start_column)
    }

    pub fn set_end_position(&self, end_line_number: i32, end_column: i32) -> Range {
        Range::new(self.start_line_number, self.start_column, end_line_number, end_column)
    }

    pub fn set_start_position(&self, start_line_number: i32, start_column: i32) -> Range {
        Range::new(start_line_number, start_column, self.end_line_number, self.end_column)
    }

    pub fn collapse_to_start(&self) -> Range {
        Range::new(self.start_line_number, self.start_column, self.start_line_number, self.start_column)
    }

    pub fn collapse_to_end(&self) -> Range {
        Range::new(self.end_line_number, self.end_column, self.end_line_number, self.end_column)
    }

    pub fn delta(&self, line_count: i32) -> Range {
        Range::new(
            self.start_line_number + line_count,
            self.start_column,
            self.end_line_number + line_count,
            self.end_column,
        )
    }

    pub fn is_single_line(&self) -> bool {
        self.start_line_number == self.end_line_number
    }

    /// `Range.fromPositions(start, end)`; sem `end`, um intervalo vazio em `start`.
    pub fn from_positions(start: Position, end: Position) -> Range {
        Range::new(start.line_number, start.column, end.line_number, end.column)
    }

    /// `areIntersectingOrTouching`.
    pub fn are_intersecting_or_touching(a: &Range, b: &Range) -> bool {
        if a.end_line_number < b.start_line_number || (a.end_line_number == b.start_line_number && a.end_column < b.start_column) {
            return false;
        }
        if b.end_line_number < a.start_line_number || (b.end_line_number == a.start_line_number && b.end_column < a.start_column) {
            return false;
        }
        true
    }

    /// `areIntersecting`: tocar na borda não conta.
    pub fn are_intersecting(a: &Range, b: &Range) -> bool {
        if a.end_line_number < b.start_line_number || (a.end_line_number == b.start_line_number && a.end_column <= b.start_column) {
            return false;
        }
        if b.end_line_number < a.start_line_number || (b.end_line_number == a.start_line_number && b.end_column <= a.start_column) {
            return false;
        }
        true
    }

    /// `areOnlyIntersecting`.
    pub fn are_only_intersecting(a: &Range, b: &Range) -> bool {
        if a.end_line_number < b.start_line_number - 1 || (a.end_line_number == b.start_line_number && a.end_column < b.start_column - 1) {
            return false;
        }
        if b.end_line_number < a.start_line_number - 1 || (b.end_line_number == a.start_line_number && b.end_column < a.start_column - 1) {
            return false;
        }
        true
    }

    /// `compareRangesUsingStarts`: o `null` do original ordena antes de qualquer intervalo.
    pub fn compare_ranges_using_starts(a: Option<&Range>, b: Option<&Range>) -> i32 {
        if let (Some(a), Some(b)) = (a, b) {
            if a.start_line_number == b.start_line_number {
                if a.start_column == b.start_column {
                    if a.end_line_number == b.end_line_number {
                        return a.end_column - b.end_column;
                    }
                    return a.end_line_number - b.end_line_number;
                }
                return a.start_column - b.start_column;
            }
            return a.start_line_number - b.start_line_number;
        }
        i32::from(a.is_some()) - i32::from(b.is_some())
    }

    /// `compareRangesUsingEnds`.
    pub fn compare_ranges_using_ends(a: &Range, b: &Range) -> i32 {
        if a.end_line_number == b.end_line_number {
            if a.end_column == b.end_column {
                if a.start_line_number == b.start_line_number {
                    return a.start_column - b.start_column;
                }
                return a.start_line_number - b.start_line_number;
            }
            return a.end_column - b.end_column;
        }
        a.end_line_number - b.end_line_number
    }

    /// `spansMultipleLines`.
    pub fn spans_multiple_lines(&self) -> bool {
        self.end_line_number > self.start_line_number
    }
}

impl fmt::Display for Range {
    /// `toString()`: `[l1,c1 -> l2,c2]`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{},{} -> {},{}]",
            self.start_line_number, self.start_column, self.end_line_number, self.end_column
        )
    }
}
