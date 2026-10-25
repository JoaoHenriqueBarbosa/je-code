//! Porte parcial de `src/vs/editor/browser/widget/diffEditor/components/diffEditorViewZones/diffEditorViewZones.ts`: só as duas
//! funções que as opções do editor de diff leem (`allowsTrueInlineDiffRendering` e `rangeIsSingleLine`). As zonas de visão do
//! editor (`DiffEditorViewZones`, as zonas de alinhamento e as de texto movido) pertencem ao widget de diff e entram com ele.

use crate::diff::range_mapping::DetailedLineRangeMapping;
use crate::range::Range;

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
