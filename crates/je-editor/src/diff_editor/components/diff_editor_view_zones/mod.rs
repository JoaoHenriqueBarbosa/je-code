//! Porte de `src/vs/editor/browser/widget/diffEditor/components/diffEditorViewZones/`, parte 1: `renderLines.ts` ([`render_lines`]),
//! `inlineDiffDeletedCodeMargin.ts` ([`inline_diff_deleted_code_margin`]) e `copySelection.ts` ([`copy_selection`]).
//!
//! Parte 2 (ainda não portada): o `diffEditorViewZones.ts` (`DiffEditorViewZones`, as zonas de alinhamento e as de texto movido). Ao
//! portá-lo, a criação direta dele substitui o parâmetro [`crate::diff_editor::diff_editor_features::CreateDiffEditorViewZones`] do
//! construtor de `DiffEditorFeaturesImpl`, e o parâmetro some. As duas funções abaixo são exportadas pelo `diffEditorViewZones.ts` no
//! original; ficam aqui, com o resto da pasta, até a parte 2 trazer o arquivo (que então as recebe e este módulo as reexporta).

use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::range::Range;

pub mod copy_selection;
pub mod inline_diff_deleted_code_margin;
pub mod render_lines;

/// `allowsTrueInlineDiffRendering(mapping)`.
pub fn allows_true_inline_diff_rendering(mapping: &DetailedLineRangeMapping) -> bool {
    let Some(inner_changes) = &mapping.inner_changes else {
        return false;
    };
    inner_changes.iter().all(|c| {
        (range_is_single_line(&c.modified_range) && range_is_single_line(&c.original_range)) || c.original_range == Range::new(1, 1, 1, 1)
    })
}

/// `rangeIsSingleLine(range)`.
pub fn range_is_single_line(range: &Range) -> bool {
    range.start_line_number == range.end_line_number
}
