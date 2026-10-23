//! Porte de `src/vs/editor/common/diff`: o computador de diff de linhas legado e os tipos de mapeamento
//! de intervalos que ele produz.
//!
//! - [`lines_diff_computer`]: `linesDiffComputer.ts` (`ILinesDiffComputer`, opções, `LinesDiff`, `MovedText`);
//! - [`range_mapping`]: `rangeMapping.ts` (`LineRangeMapping`, `DetailedLineRangeMapping`, `RangeMapping`);
//! - [`legacy_lines_diff_computer`]: `legacyLinesDiffComputer.ts` (o `DiffComputer` e o
//!   `LegacyLinesDiffComputer`).

pub mod legacy_lines_diff_computer;
pub mod lines_diff_computer;
pub mod range_mapping;
