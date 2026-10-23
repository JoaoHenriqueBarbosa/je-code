//! Porte de `src/vs/editor/common/diff/rangeMapping.ts`.
//!
//! Notas de tradução:
//! - `DetailedLineRangeMapping extends LineRangeMapping` vira composição com `Deref`: os campos `original` e
//!   `modified` e os métodos herdados (`join`, `changedLineCount`, `toRangeMapping`...) continuam acessíveis, e
//!   `flip` é redefinido, como no original;
//! - `Number.MAX_SAFE_INTEGER` das colunas vira `i32::MAX`, a maior coluna que o `Range` representa (a mesma
//!   convenção de `LineRange::to_inclusive_range`);
//! - as estáticas `RangeMapping.join(list)` e `RangeMapping#join(other)` dividem o nome no original; a
//!   estática é [`RangeMapping::join_all`], como em `LineRange::join_all`;
//! - `DetailedLineRangeMapping.toTextEdit` (estática) é [`DetailedLineRangeMapping::to_text_edit`];
//! - `AbstractText` é o trait de [`crate::text::abstract_text`], recebido como `&dyn AbstractText`;
//! - `groupAdjacentBy` é o `group_adjacent_by` de `je_base::arrays`, e o `assertFn` de
//!   `lineRangeMappingFromRangeMappings` é o `assert_fn` de `je_base::assert`.

use std::fmt;
use std::ops::Deref;

use je_base::JsString;
use je_base::arrays::group_adjacent_by;
use je_base::assert::{assert_fn, check_adjacent_items};
use je_base::errors::{self, Error};

use crate::diff::legacy_lines_diff_computer::IChange;
use crate::edits::text_edit::{TextEdit, TextReplacement};
use crate::line_range::LineRange;
use crate::position::Position;
use crate::range::Range;
use crate::text::abstract_text::AbstractText;

/// Maps a line range in the original text model to a line range in the modified text model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRangeMapping {
    /// The line range in the original text model.
    pub original: LineRange,

    /// The line range in the modified text model.
    pub modified: LineRange,
}

impl LineRangeMapping {
    pub fn new(original_range: LineRange, modified_range: LineRange) -> Self {
        Self { original: original_range, modified: modified_range }
    }

    pub fn inverse(mapping: &[LineRangeMapping], original_line_count: i32, modified_line_count: i32) -> Vec<LineRangeMapping> {
        let mut result = Vec::new();
        let mut last_original_end_line_number = 1;
        let mut last_modified_end_line_number = 1;

        for m in mapping {
            let r = LineRangeMapping::new(
                LineRange::new(last_original_end_line_number, m.original.start_line_number),
                LineRange::new(last_modified_end_line_number, m.modified.start_line_number),
            );
            if !r.modified.is_empty() {
                result.push(r);
            }
            last_original_end_line_number = m.original.end_line_number_exclusive;
            last_modified_end_line_number = m.modified.end_line_number_exclusive;
        }
        let r = LineRangeMapping::new(
            LineRange::new(last_original_end_line_number, original_line_count + 1),
            LineRange::new(last_modified_end_line_number, modified_line_count + 1),
        );
        if !r.modified.is_empty() {
            result.push(r);
        }
        result
    }

    pub fn clip(mapping: &[LineRangeMapping], original_range: LineRange, modified_range: LineRange) -> Vec<LineRangeMapping> {
        let mut result = Vec::new();
        for m in mapping {
            let original = m.original.intersect(&original_range);
            let modified = m.modified.intersect(&modified_range);
            if let (Some(original), Some(modified)) = (original, modified)
                && !original.is_empty()
                && !modified.is_empty()
            {
                result.push(LineRangeMapping::new(original, modified));
            }
        }
        result
    }

    pub fn flip(&self) -> LineRangeMapping {
        LineRangeMapping::new(self.modified, self.original)
    }

    pub fn join(&self, other: &LineRangeMapping) -> LineRangeMapping {
        LineRangeMapping::new(self.original.join(&other.original), self.modified.join(&other.modified))
    }

    pub fn changed_line_count(&self) -> i32 {
        self.original.length().max(self.modified.length())
    }

    /// This method assumes that the LineRangeMapping describes a valid diff!
    /// I.e. if one range is empty, the other range cannot be the entire document.
    /// It avoids various problems when the line range points to non-existing line-numbers.
    pub fn to_range_mapping(&self) -> RangeMapping {
        let orig_inclusive_range = self.original.to_inclusive_range();
        let mod_inclusive_range = self.modified.to_inclusive_range();
        if let (Some(orig), Some(modi)) = (orig_inclusive_range, mod_inclusive_range) {
            RangeMapping::new(orig, modi)
        } else if self.original.start_line_number == 1 || self.modified.start_line_number == 1 {
            if !(self.modified.start_line_number == 1 && self.original.start_line_number == 1) {
                // If one line range starts at 1, the other one must start at 1 as well.
                errors::throw(Error::bug_indicating(Some("not a valid diff")));
            }

            // Because one range is empty and both ranges start at line 1, none of the ranges can cover all lines.
            // Thus, `endLineNumberExclusive` is a valid line number.
            RangeMapping::new(
                Range::new(self.original.start_line_number, 1, self.original.end_line_number_exclusive, 1),
                Range::new(self.modified.start_line_number, 1, self.modified.end_line_number_exclusive, 1),
            )
        } else {
            // We can assume here that both startLineNumbers are greater than 1.
            RangeMapping::new(
                Range::new(self.original.start_line_number - 1, i32::MAX, self.original.end_line_number_exclusive - 1, i32::MAX),
                Range::new(self.modified.start_line_number - 1, i32::MAX, self.modified.end_line_number_exclusive - 1, i32::MAX),
            )
        }
    }

    /// This method assumes that the LineRangeMapping describes a valid diff!
    /// I.e. if one range is empty, the other range cannot be the entire document.
    /// It avoids various problems when the line range points to non-existing line-numbers.
    pub fn to_range_mapping2(&self, original: &[JsString], modified: &[JsString]) -> RangeMapping {
        if is_valid_line_number(self.original.end_line_number_exclusive, original)
            && is_valid_line_number(self.modified.end_line_number_exclusive, modified)
        {
            return RangeMapping::new(
                Range::new(self.original.start_line_number, 1, self.original.end_line_number_exclusive, 1),
                Range::new(self.modified.start_line_number, 1, self.modified.end_line_number_exclusive, 1),
            );
        }

        if !self.original.is_empty() && !self.modified.is_empty() {
            return RangeMapping::new(
                Range::from_positions(
                    Position::new(self.original.start_line_number, 1),
                    normalize_position(Position::new(self.original.end_line_number_exclusive - 1, i32::MAX), original),
                ),
                Range::from_positions(
                    Position::new(self.modified.start_line_number, 1),
                    normalize_position(Position::new(self.modified.end_line_number_exclusive - 1, i32::MAX), modified),
                ),
            );
        }

        if self.original.start_line_number > 1 && self.modified.start_line_number > 1 {
            return RangeMapping::new(
                Range::from_positions(
                    normalize_position(Position::new(self.original.start_line_number - 1, i32::MAX), original),
                    normalize_position(Position::new(self.original.end_line_number_exclusive - 1, i32::MAX), original),
                ),
                Range::from_positions(
                    normalize_position(Position::new(self.modified.start_line_number - 1, i32::MAX), modified),
                    normalize_position(Position::new(self.modified.end_line_number_exclusive - 1, i32::MAX), modified),
                ),
            );
        }

        // Situation now: one range is empty and one range touches the last line and one range starts at line 1.
        // I don't think this can happen.

        errors::throw(Error::bug_indicating(None))
    }
}

impl fmt::Display for LineRangeMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{{}->{}}}", self.original, self.modified)
    }
}

fn normalize_position(position: Position, content: &[JsString]) -> Position {
    if position.line_number < 1 {
        return Position::new(1, 1);
    }
    if position.line_number as usize > content.len() {
        return Position::new(content.len() as i32, content[content.len() - 1].len() as i32 + 1);
    }
    let line = &content[position.line_number as usize - 1];
    if position.column > line.len() as i32 + 1 {
        return Position::new(position.line_number, line.len() as i32 + 1);
    }
    position
}

fn is_valid_line_number(line_number: i32, lines: &[JsString]) -> bool {
    line_number >= 1 && line_number as usize <= lines.len()
}

/// Maps a line range in the original text model to a line range in the modified text model.
/// Also contains inner range mappings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetailedLineRangeMapping {
    base: LineRangeMapping,

    /// If inner changes have not been computed, this is set to `None`.
    /// Otherwise, it represents the character-level diff in this line range.
    /// The original range of each range mapping should be contained in the original line range (same for modified), exceptions are new-lines.
    /// Must not be an empty array.
    pub inner_changes: Option<Vec<RangeMapping>>,
}

impl Deref for DetailedLineRangeMapping {
    type Target = LineRangeMapping;

    fn deref(&self) -> &LineRangeMapping {
        &self.base
    }
}

impl DetailedLineRangeMapping {
    pub fn to_text_edit(mapping: &[DetailedLineRangeMapping], modified: &dyn AbstractText) -> TextEdit {
        let mut replacements: Vec<TextReplacement> = Vec::new();
        for m in mapping {
            for r in m.inner_changes.as_deref().unwrap_or(&[]) {
                let replacement = r.to_text_edit(modified);
                replacements.push(replacement);
            }
        }
        TextEdit::new(replacements)
    }

    pub fn new(original_range: LineRange, modified_range: LineRange, inner_changes: Option<Vec<RangeMapping>>) -> Self {
        Self { base: LineRangeMapping::new(original_range, modified_range), inner_changes }
    }

    pub fn from_range_mappings(range_mappings: Vec<RangeMapping>) -> DetailedLineRangeMapping {
        let original_lines: Vec<LineRange> = range_mappings.iter().map(|r| LineRange::from_range_inclusive(&r.original_range)).collect();
        let modified_lines: Vec<LineRange> = range_mappings.iter().map(|r| LineRange::from_range_inclusive(&r.modified_range)).collect();
        let original_range = LineRange::join_all(&original_lines);
        let modified_range = LineRange::join_all(&modified_lines);
        DetailedLineRangeMapping::new(original_range, modified_range, Some(range_mappings))
    }

    pub fn flip(&self) -> DetailedLineRangeMapping {
        DetailedLineRangeMapping::new(
            self.base.modified,
            self.base.original,
            self.inner_changes.as_ref().map(|changes| changes.iter().map(RangeMapping::flip).collect()),
        )
    }

    pub fn with_inner_changes_from_line_ranges(&self) -> DetailedLineRangeMapping {
        DetailedLineRangeMapping::new(self.base.original, self.base.modified, Some(vec![self.base.to_range_mapping()]))
    }
}

/// Maps a range in the original text model to a range in the modified text model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RangeMapping {
    /// The original range.
    pub original_range: Range,

    /// The modified range.
    pub modified_range: Range,
}

impl RangeMapping {
    pub fn from_edit(edit: &TextEdit) -> Vec<RangeMapping> {
        let new_ranges = edit.get_new_ranges();
        edit.replacements.iter().enumerate().map(|(idx, e)| RangeMapping::new(e.range, new_ranges[idx])).collect()
    }

    pub fn from_edit_join(edit: &TextEdit) -> RangeMapping {
        RangeMapping::join_all(&RangeMapping::from_edit(edit))
    }

    pub fn new(original_range: Range, modified_range: Range) -> Self {
        Self { original_range, modified_range }
    }

    /// `RangeMapping.join(rangeMappings)`: lista vazia é erro de lógica.
    pub fn join_all(range_mappings: &[RangeMapping]) -> RangeMapping {
        let Some((first, rest)) = range_mappings.split_first() else {
            errors::throw(Error::bug_indicating(Some("Cannot join an empty list of range mappings")));
        };
        let mut result = *first;
        for mapping in rest {
            result = result.join(mapping);
        }
        result
    }

    pub fn assert_sorted(range_mappings: &[RangeMapping]) {
        for pair in range_mappings.windows(2) {
            let (previous, current) = (&pair[0], &pair[1]);
            if !(previous.original_range.get_end_position().is_before_or_equal(&current.original_range.get_start_position())
                && previous.modified_range.get_end_position().is_before_or_equal(&current.modified_range.get_start_position()))
            {
                errors::throw(Error::bug_indicating(Some("Range mappings must be sorted")));
            }
        }
    }

    pub fn flip(&self) -> RangeMapping {
        RangeMapping::new(self.modified_range, self.original_range)
    }

    /// Creates a single text edit that describes the change from the original to the modified text.
    pub fn to_text_edit(&self, modified: &dyn AbstractText) -> TextReplacement {
        let new_text = modified.get_value_of_range(&self.modified_range);
        TextReplacement::new(self.original_range, new_text)
    }

    pub fn join(&self, other: &RangeMapping) -> RangeMapping {
        RangeMapping::new(self.original_range.plus_range(&other.original_range), self.modified_range.plus_range(&other.modified_range))
    }
}

impl fmt::Display for RangeMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{{}->{}}}", self.original_range, self.modified_range)
    }
}

/// `lineRangeMappingFromRangeMappings`.
pub fn line_range_mapping_from_range_mappings(
    alignments: &[RangeMapping],
    original_lines: &dyn AbstractText,
    modified_lines: &dyn AbstractText,
    dont_assert_start_line: bool,
) -> Vec<DetailedLineRangeMapping> {
    let mut changes: Vec<DetailedLineRangeMapping> = Vec::new();
    for g in group_adjacent_by(
        alignments.iter().map(|a| get_line_range_mapping(a, original_lines, modified_lines)).collect::<Vec<_>>(),
        |a1, a2| a1.original.intersects_or_touches(&a2.original) || a1.modified.intersects_or_touches(&a2.modified),
    ) {
        let first = &g[0];
        let last = &g[g.len() - 1];

        changes.push(DetailedLineRangeMapping::new(
            first.original.join(&last.original),
            first.modified.join(&last.modified),
            Some(
                g.iter()
                    .map(|a| a.inner_changes.as_ref().expect("getLineRangeMapping sempre preenche innerChanges")[0])
                    .collect(),
            ),
        ));
    }

    assert_fn(|| {
        if !dont_assert_start_line && !changes.is_empty() {
            if changes[0].modified.start_line_number != changes[0].original.start_line_number {
                return false;
            }

            let last = &changes[changes.len() - 1];
            if modified_lines.length().line_count - last.modified.end_line_number_exclusive
                != original_lines.length().line_count - last.original.end_line_number_exclusive
            {
                return false;
            }
        }
        check_adjacent_items(&changes, |m1, m2| {
            m2.original.start_line_number - m1.original.end_line_number_exclusive
                == m2.modified.start_line_number - m1.modified.end_line_number_exclusive
                // There has to be an unchanged line in between (otherwise both diffs should have been joined)
                && m1.original.end_line_number_exclusive < m2.original.start_line_number
                && m1.modified.end_line_number_exclusive < m2.modified.start_line_number
        })
    });

    changes
}

/// `getLineRangeMapping`.
pub fn get_line_range_mapping(
    range_mapping: &RangeMapping,
    original_lines: &dyn AbstractText,
    modified_lines: &dyn AbstractText,
) -> DetailedLineRangeMapping {
    let mut line_start_delta = 0;
    let mut line_end_delta = 0;
    let original = range_mapping.original_range;
    let modified = range_mapping.modified_range;

    // rangeMapping describes the edit that replaces `rangeMapping.originalRange` with `newText := getText(modifiedLines, rangeMapping.modifiedRange)`.

    // original: ]xxx \n <- this line is not modified
    // modified: ]xx  \n
    if modified.end_column == 1
        && original.end_column == 1
        && original.start_line_number + line_start_delta <= original.end_line_number
        && modified.start_line_number + line_start_delta <= modified.end_line_number
    {
        // We can only do this if the range is not empty yet
        line_end_delta = -1;
    }

    // original: xxx[ \n <- this line is not modified
    // modified: xxx[ \n
    if modified.start_column - 1 >= modified_lines.get_line_length(modified.start_line_number)
        && original.start_column - 1 >= original_lines.get_line_length(original.start_line_number)
        && original.start_line_number <= original.end_line_number + line_end_delta
        && modified.start_line_number <= modified.end_line_number + line_end_delta
    {
        // We can only do this if the range is not empty yet
        line_start_delta = 1;
    }

    let original_line_range =
        LineRange::new(original.start_line_number + line_start_delta, original.end_line_number + 1 + line_end_delta);
    let modified_line_range =
        LineRange::new(modified.start_line_number + line_start_delta, modified.end_line_number + 1 + line_end_delta);

    DetailedLineRangeMapping::new(original_line_range, modified_line_range, Some(vec![*range_mapping]))
}

/// Os intervalos de linhas (original e modificado) que um [`IChange`] descreve: um lado com
/// `EndLineNumber` zero é inserção ou remoção, e o intervalo vazio fica logo depois da linha inicial.
pub fn line_ranges_from_change(change: &impl IChange) -> (LineRange, LineRange) {
    let original_range = if change.original_end_line_number() == 0 {
        // Insertion
        LineRange::new(change.original_start_line_number() + 1, change.original_start_line_number() + 1)
    } else {
        LineRange::new(change.original_start_line_number(), change.original_end_line_number() + 1)
    };

    let modified_range = if change.modified_end_line_number() == 0 {
        // Deletion
        LineRange::new(change.modified_start_line_number() + 1, change.modified_start_line_number() + 1)
    } else {
        LineRange::new(change.modified_start_line_number(), change.modified_end_line_number() + 1)
    };

    (original_range, modified_range)
}

pub fn line_range_mapping_from_change(change: &impl IChange) -> LineRangeMapping {
    let (original_range, modified_range) = line_ranges_from_change(change);
    LineRangeMapping::new(original_range, modified_range)
}
