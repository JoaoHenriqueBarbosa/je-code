//! Porte de `src/vs/workbench/contrib/mergeEditor/browser/model/lineRange.ts`.
//!
//! `MergeEditorLineRange` estende `LineRange` no original; aqui é um tipo novo que expõe o `LineRange` por
//! `Deref`, de modo que os métodos herdados (`contains`, `contains_range`, `intersects_or_touches`,
//! `length`, `to_exclusive_range`...) valem sem repasse. Só o que o `mapping`, o `modifiedBaseRange` e o
//! `mergeEditorModel` usam foi portado: `deltaEnd` e `deltaStart` ficam de fora. `getLines` recebe o
//! `TextModel` do je-editor (o `ITextModel` do original).

use std::fmt;

use je_base::uint::constants::MAX_SAFE_SMALL_INTEGER;
use std::ops::Deref;

use je_base::JsString;
use je_editor::cursor_common::CursorSimpleModel;
use je_editor::line_range::LineRange;
use je_editor::range::Range;
use je_editor::text_model::TextModel;

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

    pub fn is_after(&self, range: &MergeEditorLineRange) -> bool {
        self.start_line_number >= range.end_line_number_exclusive
    }

    pub fn is_before(&self, range: &MergeEditorLineRange) -> bool {
        range.start_line_number >= self.end_line_number_exclusive
    }

    pub fn delta(&self, line_delta: i32) -> MergeEditorLineRange {
        Self::from_length(self.start_line_number + line_delta, self.length())
    }

    pub fn delta_start(&self, line_delta: i32) -> MergeEditorLineRange {
        Self::from_length(self.start_line_number + line_delta, self.length() - line_delta)
    }

    pub fn get_lines(&self, model: &TextModel) -> Vec<JsString> {
        (0..self.length()).map(|i| model.get_line_content(self.start_line_number + i)).collect()
    }

    pub fn to_inclusive_range_or_empty(&self) -> Range {
        if self.is_empty() {
            return Range::new(self.start_line_number, 1, self.start_line_number, 1);
        }
        Range::new(self.start_line_number, 1, self.end_line_number_exclusive - 1, MAX_SAFE_SMALL_INTEGER)
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
