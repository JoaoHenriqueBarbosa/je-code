//! Porte de `src/vs/editor/common/diff/defaultLinesDiffComputer`: o computador de diff de linhas padrão.
//!
//! - [`algorithms`]: o contrato dos algoritmos, Myers e programação dinâmica;
//! - [`utils`]: `utils.ts` (`Array2D`, `isSpace`, `LineRangeFragment`);
//! - [`line_sequence`]: `lineSequence.ts`;
//! - [`lines_slice_char_sequence`]: `linesSliceCharSequence.ts`;
//! - [`heuristic_sequence_optimizations`]: `heuristicSequenceOptimizations.ts`;
//! - [`compute_moved_lines`]: `computeMovedLines.ts`;
//! - [`default_lines_diff_computer`]: `defaultLinesDiffComputer.ts`.

pub mod algorithms;
pub mod compute_moved_lines;
pub mod default_lines_diff_computer;
pub mod heuristic_sequence_optimizations;
pub mod line_sequence;
pub mod lines_slice_char_sequence;
pub mod utils;
