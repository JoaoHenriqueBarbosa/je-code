//! Porte de `src/vs/editor/common/diff/defaultLinesDiffComputer/algorithms/dynamicProgrammingDiffing.ts`: o algoritmo de
//! diff O(MN) com função de pontuação (`DynamicProgrammingDiffing`).
//!
//! Representação:
//! - `compute(sequence1, sequence2, timeout?, equalityScore?)` é [`DynamicProgrammingDiffing::compute_with_equality_score`]
//!   (com os dois parâmetros opcionais como `Option`); o [`IDiffAlgorithm::compute`] do trait é o caso sem `equalityScore`.
//! - `Array2D<number>` é o [`Array2D`] de `utils`, de `f64` (a pontuação do original é um `number` com fração).

use super::diff_algorithm::{DiffAlgorithmResult, IDiffAlgorithm, ISequence, ITimeout, InfiniteTimeout, SequenceDiff};
use crate::diff::default_lines_diff_computer::utils::Array2D;
use crate::offset_range::OffsetRange;

/// A O(MN) diffing algorithm that supports a score function.
/// The algorithm can be improved by processing the 2d array diagonally.
pub struct DynamicProgrammingDiffing;

impl DynamicProgrammingDiffing {
    /// `compute(sequence1, sequence2, timeout, equalityScore)`.
    pub fn compute_with_equality_score(
        &self,
        sequence1: &dyn ISequence,
        sequence2: &dyn ISequence,
        timeout: Option<&dyn ITimeout>,
        equality_score: Option<&dyn Fn(i32, i32) -> f64>,
    ) -> DiffAlgorithmResult {
        let timeout = timeout.unwrap_or(&InfiniteTimeout::INSTANCE);

        if sequence1.length() == 0 || sequence2.length() == 0 {
            return DiffAlgorithmResult::trivial(sequence1, sequence2);
        }

        // lcsLengths.get(i, j): Length of the longest common subsequence of sequence1.substring(0, i + 1) and sequence2.substring(0, j + 1).
        let mut lcs_lengths: Array2D<f64> = Array2D::new(sequence1.length(), sequence2.length());
        let mut directions: Array2D<f64> = Array2D::new(sequence1.length(), sequence2.length());
        let mut lengths: Array2D<f64> = Array2D::new(sequence1.length(), sequence2.length());

        // ==== Initializing lcsLengths ====
        for s1 in 0..sequence1.length() {
            for s2 in 0..sequence2.length() {
                if !timeout.is_valid() {
                    return DiffAlgorithmResult::trivial_timed_out(sequence1, sequence2);
                }

                let horizontal_len = if s1 == 0 { 0.0 } else { lcs_lengths.get(s1 - 1, s2) };
                let vertical_len = if s2 == 0 { 0.0 } else { lcs_lengths.get(s1, s2 - 1) };

                let extended_seq_score: f64;
                if sequence1.get_element(s1) == sequence2.get_element(s2) {
                    let mut score = if s1 == 0 || s2 == 0 { 0.0 } else { lcs_lengths.get(s1 - 1, s2 - 1) };
                    if s1 > 0 && s2 > 0 && directions.get(s1 - 1, s2 - 1) == 3.0 {
                        // Prefer consecutive diagonals
                        score += lengths.get(s1 - 1, s2 - 1);
                    }
                    score += match equality_score {
                        Some(equality_score) => equality_score(s1, s2),
                        None => 1.0,
                    };
                    extended_seq_score = score;
                } else {
                    extended_seq_score = -1.0;
                }

                let new_value = horizontal_len.max(vertical_len).max(extended_seq_score);

                if new_value == extended_seq_score {
                    // Prefer diagonals
                    let prev_len = if s1 > 0 && s2 > 0 { lengths.get(s1 - 1, s2 - 1) } else { 0.0 };
                    lengths.set(s1, s2, prev_len + 1.0);
                    directions.set(s1, s2, 3.0);
                } else if new_value == horizontal_len {
                    lengths.set(s1, s2, 0.0);
                    directions.set(s1, s2, 1.0);
                } else if new_value == vertical_len {
                    lengths.set(s1, s2, 0.0);
                    directions.set(s1, s2, 2.0);
                }

                lcs_lengths.set(s1, s2, new_value);
            }
        }

        // ==== Backtracking ====
        let mut result: Vec<SequenceDiff> = Vec::new();
        let mut last_aligning_pos_s1 = sequence1.length();
        let mut last_aligning_pos_s2 = sequence2.length();

        let mut report_decreasing_aligning_positions = |s1: i32, s2: i32| {
            if s1 + 1 != last_aligning_pos_s1 || s2 + 1 != last_aligning_pos_s2 {
                result.push(SequenceDiff::new(
                    OffsetRange::new(s1 + 1, last_aligning_pos_s1),
                    OffsetRange::new(s2 + 1, last_aligning_pos_s2),
                ));
            }
            last_aligning_pos_s1 = s1;
            last_aligning_pos_s2 = s2;
        };

        let mut s1 = sequence1.length() - 1;
        let mut s2 = sequence2.length() - 1;
        while s1 >= 0 && s2 >= 0 {
            if directions.get(s1, s2) == 3.0 {
                report_decreasing_aligning_positions(s1, s2);
                s1 -= 1;
                s2 -= 1;
            } else if directions.get(s1, s2) == 1.0 {
                s1 -= 1;
            } else {
                s2 -= 1;
            }
        }
        report_decreasing_aligning_positions(-1, -1);
        result.reverse();
        DiffAlgorithmResult::new(result, false)
    }
}

impl IDiffAlgorithm for DynamicProgrammingDiffing {
    fn compute(&self, sequence1: &dyn ISequence, sequence2: &dyn ISequence, timeout: Option<&dyn ITimeout>) -> DiffAlgorithmResult {
        self.compute_with_equality_score(sequence1, sequence2, timeout, None)
    }
}
