//! Porte de `src/vs/editor/common/diff/defaultLinesDiffComputer/algorithms`: os algoritmos de diff de sequências.
//!
//! - [`diff_algorithm`]: `diffAlgorithm.ts` (o contrato, `SequenceDiff` e os prazos);
//! - [`myers_diff_algorithm`]: `myersDiffAlgorithm.ts`;
//! - [`dynamic_programming_diffing`]: `dynamicProgrammingDiffing.ts`.

pub mod diff_algorithm;
pub mod dynamic_programming_diffing;
pub mod myers_diff_algorithm;
