//! Porte de `src/vs/editor/common/diff`: o computador de diff de linhas legado, o padrão e os tipos de mapeamento
//! de intervalos que eles produzem.
//!
//! - [`default_lines_diff_computer`]: `defaultLinesDiffComputer/` (o `DefaultLinesDiffComputer`, os algoritmos de Myers e
//!   de programação dinâmica, as heurísticas e a detecção de movimentos);
//! - [`document_diff_provider`]: `documentDiffProvider.ts` (só `IDocumentDiffProviderOptions` e `IDocumentDiff`);
//! - [`lines_diff_computer`]: `linesDiffComputer.ts` (`ILinesDiffComputer`, opções, `LinesDiff`, `MovedText`);
//! - [`range_mapping`]: `rangeMapping.ts` (`LineRangeMapping`, `DetailedLineRangeMapping`, `RangeMapping`);
//! - [`legacy_lines_diff_computer`]: `legacyLinesDiffComputer.ts` (o `DiffComputer` e o
//!   `LegacyLinesDiffComputer`).

pub mod default_lines_diff_computer;
pub mod document_diff_provider;
pub mod legacy_lines_diff_computer;
pub mod lines_diff_computer;
pub mod range_mapping;
