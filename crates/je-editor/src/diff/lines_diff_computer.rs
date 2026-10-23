//! Porte de `src/vs/editor/common/diff/linesDiffComputer.ts`.

use je_base::JsString;

use crate::diff::range_mapping::{DetailedLineRangeMapping, LineRangeMapping};

/// `ILinesDiffComputer`.
pub trait ILinesDiffComputer {
    fn compute_diff(&self, original_lines: &[JsString], modified_lines: &[JsString], options: &LinesDiffComputerOptions) -> LinesDiff;
}

/// `ILinesDiffComputerOptions`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinesDiffComputerOptions {
    pub ignore_trim_whitespace: bool,
    /// O `number` do original, em milissegundos; `0` desliga o limite.
    pub max_computation_time_ms: f64,
    pub compute_moves: bool,
    /// `extendToSubwords?`.
    pub extend_to_subwords: Option<bool>,
}

/// `LinesDiff`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinesDiff {
    pub changes: Vec<DetailedLineRangeMapping>,

    /// Sorted by original line ranges.
    /// The original line ranges and the modified line ranges must be disjoint (but can be touching).
    pub moves: Vec<MovedText>,

    /// Indicates if the time out was reached.
    /// In that case, the diffs might be an approximation and the user should be asked to rerun the diff with more time.
    pub hit_timeout: bool,
}

impl LinesDiff {
    pub fn new(changes: Vec<DetailedLineRangeMapping>, moves: Vec<MovedText>, hit_timeout: bool) -> Self {
        Self { changes, moves, hit_timeout }
    }
}

/// `MovedText`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MovedText {
    pub line_range_mapping: LineRangeMapping,

    /// The diff from the original text to the moved text.
    /// Must be contained in the original/modified line range.
    /// Can be empty if the text didn't change (only moved).
    pub changes: Vec<DetailedLineRangeMapping>,
}

impl MovedText {
    pub fn new(line_range_mapping: LineRangeMapping, changes: Vec<DetailedLineRangeMapping>) -> Self {
        Self { line_range_mapping, changes }
    }

    pub fn flip(&self) -> MovedText {
        MovedText::new(self.line_range_mapping.flip(), self.changes.iter().map(DetailedLineRangeMapping::flip).collect())
    }
}
