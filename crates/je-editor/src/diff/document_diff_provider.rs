//! Porte parcial de `src/vs/editor/common/diff/documentDiffProvider.ts`: as opções do cálculo de diferenças de um
//! documento (`IDocumentDiffProviderOptions`) e o resultado dele (`IDocumentDiff`).
//!
//! Divergências:
//! - `IDocumentDiffProviderOptions` tem os mesmos quatro campos de `ILinesDiffComputerOptions`
//!   (`ignoreTrimWhitespace`, `maxComputationTimeMs`, `computeMoves`, `extendToSubwords`) e o original passa uma à outra
//!   por tipagem estrutural: é um alias para [`LinesDiffComputerOptions`] (DRY).
//! - `IDocumentDiffProvider` (o provedor que o `DiffEditorWidget` consulta) e `nullDocumentDiff` pertencem ao widget de
//!   diff e não têm leitor no porte; ficam para a fatia dele.

use super::lines_diff_computer::{LinesDiffComputerOptions, MovedText};
use super::range_mapping::DetailedLineRangeMapping;

/// `IDocumentDiffProviderOptions`.
pub type DocumentDiffProviderOptions = LinesDiffComputerOptions;

/// `IDocumentDiff`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentDiff {
    /// If true, both text models are identical (byte-by-byte).
    pub identical: bool,
    /// If true, the diff computation hit the timeout and the result might be an approximation.
    pub quit_early: bool,
    /// Maps all modified line ranges in the original to the corresponding line ranges in the modified text model.
    pub changes: Vec<DetailedLineRangeMapping>,
    /// Sorted by original line ranges. The original line ranges and the modified line ranges must be disjoint (but can be
    /// touching).
    pub moves: Vec<MovedText>,
}
