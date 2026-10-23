//! Porte de `src/vs/editor/common/core/ranges/columnRange.ts`: `ColumnRange`.

use je_base::errors::{self, Error};

use crate::offset_range::OffsetRange;
use crate::range::Range;

/// `ColumnRange`: intervalo de colunas, a partir de 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnRange {
    /// A partir de 1.
    pub start_column: i32,
    pub end_column_exclusive: i32,
}

impl ColumnRange {
    /// `ColumnRange.fromOffsetRange`.
    pub fn from_offset_range(offset_range: OffsetRange) -> Self {
        Self::new(offset_range.start + 1, offset_range.end_exclusive + 1)
    }

    /// O construtor lança `BugIndicatingError` se o início vier depois do fim.
    pub fn new(start_column: i32, end_column_exclusive: i32) -> Self {
        if start_column > end_column_exclusive {
            errors::throw(Error::bug_indicating(Some(&format!(
                "startColumn {start_column} cannot be after endColumnExclusive {end_column_exclusive}"
            ))));
        }
        Self { start_column, end_column_exclusive }
    }

    /// `toRange(lineNumber)`.
    pub fn to_range(&self, line_number: i32) -> Range {
        Range::new(line_number, self.start_column, line_number, self.end_column_exclusive)
    }

    /// `equals`.
    pub fn equals(&self, other: &ColumnRange) -> bool {
        self == other
    }

    /// `toZeroBasedOffsetRange`.
    pub fn to_zero_based_offset_range(&self) -> OffsetRange {
        OffsetRange::new(self.start_column - 1, self.end_column_exclusive - 1)
    }
}
