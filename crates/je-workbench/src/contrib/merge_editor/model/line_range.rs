//! Porte de `src/vs/workbench/contrib/mergeEditor/browser/model/lineRange.ts`.
//!
//! `MergeEditorLineRange` estende `LineRange` no original; aqui é um tipo novo que expõe o `LineRange` por
//! `Deref`, de modo que os métodos herdados (`contains`, `contains_range`, `intersects_or_touches`,
//! `length`, `to_exclusive_range`...) valem sem repasse. Só o que o `mapping` usa foi portado:
//! `getLines` (precisa de `ITextModel`), `isAfter`, `deltaEnd`, `deltaStart` e `toInclusiveRangeOrEmpty`
//! ficam de fora.

use std::fmt;
use std::ops::Deref;

use je_editor::line_range::LineRange;

/// Intervalo de linhas do editor de merge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergeEditorLineRange(pub LineRange);

impl MergeEditorLineRange {
    pub fn new(start_line_number: i32, end_line_number_exclusive: i32) -> Self {
        Self(LineRange::new(start_line_number, end_line_number_exclusive))
    }

    pub fn from_line_numbers(start_line_number: i32, end_exclusive_line_number: i32) -> Self {
        Self::from_length(start_line_number, end_exclusive_line_number - start_line_number)
    }

    pub fn from_length(start_line_number: i32, length: i32) -> Self {
        Self::new(start_line_number, start_line_number + length)
    }

    pub fn join(&self, other: &MergeEditorLineRange) -> MergeEditorLineRange {
        Self::from_line_numbers(
            self.start_line_number.min(other.start_line_number),
            self.end_line_number_exclusive.max(other.end_line_number_exclusive),
        )
    }

    pub fn is_before(&self, range: &MergeEditorLineRange) -> bool {
        range.start_line_number >= self.end_line_number_exclusive
    }

    pub fn delta(&self, line_delta: i32) -> MergeEditorLineRange {
        Self::from_length(self.start_line_number + line_delta, self.length())
    }
}

impl Deref for MergeEditorLineRange {
    type Target = LineRange;

    fn deref(&self) -> &LineRange {
        &self.0
    }
}

impl fmt::Display for MergeEditorLineRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
