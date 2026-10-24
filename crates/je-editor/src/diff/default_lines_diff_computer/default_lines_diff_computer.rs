//! Porte de `src/vs/editor/common/diff/defaultLinesDiffComputer/defaultLinesDiffComputer.ts`: o computador de diff de
//! linhas padrão (`DefaultLinesDiffComputer`), que alinha as linhas, refina cada trecho em caracteres e detecta
//! movimentos.
//!
//! Representação:
//! - `computeDiff(originalLines, modifiedLines, options)` é o [`ILinesDiffComputer::compute_diff`], com `string[]` como
//!   `&[JsString]`.
//! - O prazo (`InfiniteTimeout.instance` ou `new DateTimeout(ms)`) é um `Box<dyn ITimeout>` e quem o usa recebe `&dyn ITimeout`.
//! - O `perfectHashes` (`Map<string, number>`) é um `HashMap<JsString, i32>`.
//! - O `scanForWhitespaceChanges`, um fechamento sobre `seq1LastStart` e `seq2LastStart` (que mudam entre as chamadas),
//!   é o método privado [`DefaultLinesDiffComputer::scan_for_whitespace_changes`] com esses valores e o estado mutável
//!   (`alignments`, `hitTimeout`) como parâmetros.
//! - O `{ mappings, hitTimeout }` de `refineDiff` é a struct [`RefinedDiff`].
//! - `assertFn(...)` é `assert_fn` de `je_base::assert`.

use std::collections::HashMap;

use je_base::JsString;
use je_base::assert::assert_fn;
use je_base::js_string::trim;

use super::algorithms::diff_algorithm::{DateTimeout, IDiffAlgorithm, ISequence, ITimeout, InfiniteTimeout, SequenceDiff};
use super::algorithms::dynamic_programming_diffing::DynamicProgrammingDiffing;
use super::algorithms::myers_diff_algorithm::MyersDiffAlgorithm;
use super::compute_moved_lines::compute_moved_lines;
use super::heuristic_sequence_optimizations::{
    extend_diffs_to_entire_word_if_appropriate, optimize_sequence_diffs, remove_short_matches,
    remove_very_short_matching_lines_between_diffs, remove_very_short_matching_text_between_long_diffs,
};
use super::line_sequence::LineSequence;
use super::lines_slice_char_sequence::LinesSliceCharSequence;
use crate::diff::lines_diff_computer::{ILinesDiffComputer, LinesDiff, LinesDiffComputerOptions, MovedText};
use crate::diff::range_mapping::{DetailedLineRangeMapping, LineRangeMapping, RangeMapping, line_range_mapping_from_range_mappings};
use crate::line_range::LineRange;
use crate::offset_range::OffsetRange;
use crate::position::Position;
use crate::range::Range;
use crate::text::abstract_text::ArrayText;

/// O `{ mappings, hitTimeout }` de `refineDiff`.
struct RefinedDiff {
    mappings: Vec<RangeMapping>,
    hit_timeout: bool,
}

/// `DefaultLinesDiffComputer`.
pub struct DefaultLinesDiffComputer {
    dynamic_programming_diffing: DynamicProgrammingDiffing,
    myers_diffing_algorithm: MyersDiffAlgorithm,
}

impl DefaultLinesDiffComputer {
    pub fn new() -> Self {
        Self { dynamic_programming_diffing: DynamicProgrammingDiffing, myers_diffing_algorithm: MyersDiffAlgorithm }
    }

    #[allow(clippy::too_many_arguments)]
    fn compute_moves(
        &self,
        changes: &[DetailedLineRangeMapping],
        original_lines: &[JsString],
        modified_lines: &[JsString],
        hashed_original_lines: &[i32],
        hashed_modified_lines: &[i32],
        timeout: &dyn ITimeout,
        consider_whitespace_changes: bool,
        options: &LinesDiffComputerOptions,
    ) -> Vec<MovedText> {
        let moves = compute_moved_lines(changes, original_lines, modified_lines, hashed_original_lines, hashed_modified_lines, timeout);
        let original = ArrayText::new(original_lines.to_vec());
        let modified = ArrayText::new(modified_lines.to_vec());
        moves
            .into_iter()
            .map(|m| {
                let move_changes = self.refine_diff(
                    original_lines,
                    modified_lines,
                    &SequenceDiff::new(m.original.to_offset_range(), m.modified.to_offset_range()),
                    timeout,
                    consider_whitespace_changes,
                    options,
                );
                let mappings = line_range_mapping_from_range_mappings(&move_changes.mappings, &original, &modified, true);
                MovedText::new(m, mappings)
            })
            .collect()
    }

    fn refine_diff(
        &self,
        original_lines: &[JsString],
        modified_lines: &[JsString],
        diff: &SequenceDiff,
        timeout: &dyn ITimeout,
        consider_whitespace_changes: bool,
        options: &LinesDiffComputerOptions,
    ) -> RefinedDiff {
        let line_range_mapping = to_line_range_mapping(diff);
        let range_mapping = line_range_mapping.to_range_mapping2(original_lines, modified_lines);

        let slice1 = LinesSliceCharSequence::new(original_lines, range_mapping.original_range, consider_whitespace_changes);
        let slice2 = LinesSliceCharSequence::new(modified_lines, range_mapping.modified_range, consider_whitespace_changes);

        let diff_result = if slice1.length() + slice2.length() < 500 {
            self.dynamic_programming_diffing.compute(&slice1, &slice2, Some(timeout))
        } else {
            self.myers_diffing_algorithm.compute(&slice1, &slice2, Some(timeout))
        };

        let mut diffs = diff_result.diffs;
        diffs = optimize_sequence_diffs(&slice1, &slice2, diffs);
        diffs = extend_diffs_to_entire_word_if_appropriate(
            &slice1,
            &slice2,
            diffs,
            &|seq: &LinesSliceCharSequence, idx: i32| seq.find_word_containing(idx),
            false,
        );

        if options.extend_to_subwords.unwrap_or(false) {
            diffs = extend_diffs_to_entire_word_if_appropriate(
                &slice1,
                &slice2,
                diffs,
                &|seq: &LinesSliceCharSequence, idx: i32| seq.find_sub_word_containing(idx),
                true,
            );
        }

        diffs = remove_short_matches(&slice1, &slice2, diffs);
        diffs = remove_very_short_matching_text_between_long_diffs(&slice1, &slice2, diffs);

        let result: Vec<RangeMapping> =
            diffs.iter().map(|d| RangeMapping::new(slice1.translate_range(d.seq1_range), slice2.translate_range(d.seq2_range))).collect();

        // Assert: result applied on original should be the same as diff applied to original

        RefinedDiff { mappings: result, hit_timeout: diff_result.hit_timeout }
    }

    /// O `scanForWhitespaceChanges(equalLinesCount)` de `computeDiff`.
    #[allow(clippy::too_many_arguments)]
    fn scan_for_whitespace_changes(
        &self,
        equal_lines_count: i32,
        seq1_last_start: i32,
        seq2_last_start: i32,
        original_lines: &[JsString],
        modified_lines: &[JsString],
        timeout: &dyn ITimeout,
        consider_whitespace_changes: bool,
        options: &LinesDiffComputerOptions,
        alignments: &mut Vec<RangeMapping>,
        hit_timeout: &mut bool,
    ) {
        if !consider_whitespace_changes {
            return;
        }

        for i in 0..equal_lines_count {
            let seq1_offset = seq1_last_start + i;
            let seq2_offset = seq2_last_start + i;
            if original_lines[seq1_offset as usize] != modified_lines[seq2_offset as usize] {
                // This is because of whitespace changes, diff these lines
                let character_diffs = self.refine_diff(
                    original_lines,
                    modified_lines,
                    &SequenceDiff::new(OffsetRange::new(seq1_offset, seq1_offset + 1), OffsetRange::new(seq2_offset, seq2_offset + 1)),
                    timeout,
                    consider_whitespace_changes,
                    options,
                );
                alignments.extend(character_diffs.mappings);
                if character_diffs.hit_timeout {
                    *hit_timeout = true;
                }
            }
        }
    }
}

impl Default for DefaultLinesDiffComputer {
    fn default() -> Self {
        Self::new()
    }
}

impl ILinesDiffComputer for DefaultLinesDiffComputer {
    fn compute_diff(&self, original_lines: &[JsString], modified_lines: &[JsString], options: &LinesDiffComputerOptions) -> LinesDiff {
        if original_lines.len() <= 1 && original_lines == modified_lines {
            return LinesDiff::new(Vec::new(), Vec::new(), false);
        }

        if (original_lines.len() == 1 && original_lines[0].is_empty()) || (modified_lines.len() == 1 && modified_lines[0].is_empty()) {
            return LinesDiff::new(
                vec![DetailedLineRangeMapping::new(
                    LineRange::new(1, original_lines.len() as i32 + 1),
                    LineRange::new(1, modified_lines.len() as i32 + 1),
                    Some(vec![RangeMapping::new(
                        Range::new(1, 1, original_lines.len() as i32, original_lines[original_lines.len() - 1].len() as i32 + 1),
                        Range::new(1, 1, modified_lines.len() as i32, modified_lines[modified_lines.len() - 1].len() as i32 + 1),
                    )]),
                )],
                Vec::new(),
                false,
            );
        }

        let timeout_holder: Box<dyn ITimeout> =
            if options.max_computation_time_ms == 0.0 { Box::new(InfiniteTimeout) } else { Box::new(DateTimeout::new(options.max_computation_time_ms)) };
        let timeout: &dyn ITimeout = &*timeout_holder;
        let consider_whitespace_changes = !options.ignore_trim_whitespace;

        let mut perfect_hashes: HashMap<JsString, i32> = HashMap::new();
        let mut get_or_create_hash = |text: JsString| -> i32 {
            let next = perfect_hashes.len() as i32;
            *perfect_hashes.entry(text).or_insert(next)
        };

        let original_lines_hashes: Vec<i32> = original_lines.iter().map(|l| get_or_create_hash(JsString::from(trim(l.units())))).collect();
        let modified_lines_hashes: Vec<i32> = modified_lines.iter().map(|l| get_or_create_hash(JsString::from(trim(l.units())))).collect();

        let sequence1 = LineSequence::new(original_lines_hashes.clone(), original_lines.to_vec());
        let sequence2 = LineSequence::new(modified_lines_hashes.clone(), modified_lines.to_vec());

        let line_alignment_result = if sequence1.length() + sequence2.length() < 1700 {
            // Use the improved algorithm for small files
            self.dynamic_programming_diffing.compute_with_equality_score(
                &sequence1,
                &sequence2,
                Some(timeout),
                Some(&|offset1: i32, offset2: i32| {
                    if original_lines[offset1 as usize] == modified_lines[offset2 as usize] {
                        if modified_lines[offset2 as usize].is_empty() {
                            0.1
                        } else {
                            1.0 + (1.0 + modified_lines[offset2 as usize].len() as f64).ln()
                        }
                    } else {
                        0.99
                    }
                }),
            )
        } else {
            self.myers_diffing_algorithm.compute(&sequence1, &sequence2, Some(timeout))
        };

        let mut line_alignments = line_alignment_result.diffs;
        let mut hit_timeout = line_alignment_result.hit_timeout;
        line_alignments = optimize_sequence_diffs(&sequence1, &sequence2, line_alignments);
        line_alignments = remove_very_short_matching_lines_between_diffs(&sequence1, &sequence2, line_alignments);

        let mut alignments: Vec<RangeMapping> = Vec::new();

        let mut seq1_last_start = 0;
        let mut seq2_last_start = 0;

        for diff in &line_alignments {
            assert_fn(|| diff.seq1_range.start - seq1_last_start == diff.seq2_range.start - seq2_last_start);

            let equal_lines_count = diff.seq1_range.start - seq1_last_start;

            self.scan_for_whitespace_changes(
                equal_lines_count,
                seq1_last_start,
                seq2_last_start,
                original_lines,
                modified_lines,
                timeout,
                consider_whitespace_changes,
                options,
                &mut alignments,
                &mut hit_timeout,
            );

            seq1_last_start = diff.seq1_range.end_exclusive;
            seq2_last_start = diff.seq2_range.end_exclusive;

            let character_diffs = self.refine_diff(original_lines, modified_lines, diff, timeout, consider_whitespace_changes, options);
            if character_diffs.hit_timeout {
                hit_timeout = true;
            }
            alignments.extend(character_diffs.mappings);
        }

        self.scan_for_whitespace_changes(
            original_lines.len() as i32 - seq1_last_start,
            seq1_last_start,
            seq2_last_start,
            original_lines,
            modified_lines,
            timeout,
            consider_whitespace_changes,
            options,
            &mut alignments,
            &mut hit_timeout,
        );

        let original = ArrayText::new(original_lines.to_vec());
        let modified = ArrayText::new(modified_lines.to_vec());

        let changes = line_range_mapping_from_range_mappings(&alignments, &original, &modified, false);

        let mut moves: Vec<MovedText> = Vec::new();
        if options.compute_moves {
            moves = self.compute_moves(
                &changes,
                original_lines,
                modified_lines,
                &original_lines_hashes,
                &modified_lines_hashes,
                timeout,
                consider_whitespace_changes,
                options,
            );
        }

        // Make sure all ranges are valid
        assert_fn(|| {
            fn validate_position(pos: Position, lines: &[JsString]) -> bool {
                if pos.line_number < 1 || pos.line_number as usize > lines.len() {
                    return false;
                }
                let line = &lines[pos.line_number as usize - 1];
                if pos.column < 1 || pos.column as usize > line.len() + 1 {
                    return false;
                }
                true
            }

            fn validate_range(range: LineRange, lines: &[JsString]) -> bool {
                if range.start_line_number < 1 || range.start_line_number as usize > lines.len() + 1 {
                    return false;
                }
                if range.end_line_number_exclusive < 1 || range.end_line_number_exclusive as usize > lines.len() + 1 {
                    return false;
                }
                true
            }

            for c in &changes {
                let Some(inner_changes) = &c.inner_changes else {
                    return false;
                };
                for ic in inner_changes {
                    let valid = validate_position(ic.modified_range.get_start_position(), modified_lines)
                        && validate_position(ic.modified_range.get_end_position(), modified_lines)
                        && validate_position(ic.original_range.get_start_position(), original_lines)
                        && validate_position(ic.original_range.get_end_position(), original_lines);
                    if !valid {
                        return false;
                    }
                }
                if !validate_range(c.modified, modified_lines) || !validate_range(c.original, original_lines) {
                    return false;
                }
            }
            true
        });

        LinesDiff::new(changes, moves, hit_timeout)
    }
}

fn to_line_range_mapping(sequence_diff: &SequenceDiff) -> LineRangeMapping {
    LineRangeMapping::new(
        LineRange::new(sequence_diff.seq1_range.start + 1, sequence_diff.seq1_range.end_exclusive + 1),
        LineRange::new(sequence_diff.seq2_range.start + 1, sequence_diff.seq2_range.end_exclusive + 1),
    )
}
