//! Porte parcial de `src/vs/editor/browser/widget/diffEditor/diffEditorViewModel.ts`: a região sem mudanças que o editor de
//! diff esconde (`UnchangedRegion`), com o cálculo das regiões a partir das mudanças (`fromDiffs`), que a altura dos editores
//! de diff de notebook usa. O `DiffEditorViewModel` (o modelo do widget) não é portado.
//!
//! Divergências:
//! - `visibleLineCountTop`, `visibleLineCountBottom`, `isDragged` e `shouldHideControls` são `observableValue`/`derived` do
//!   original, lidos só pelo widget de diff (a UI desenhada é nativa). O porte guarda os dois contadores visíveis em campos
//!   simples, já limitados como o construtor limita (`softAssert` sobre a diferença).

use je_base::assert::soft_assert;

use crate::diff::range_mapping::{DetailedLineRangeMapping, LineRangeMapping};
use crate::line_range::LineRange;

/// `UnchangedRegion`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnchangedRegion {
    pub original_line_number: i32,
    pub modified_line_number: i32,
    pub line_count: i32,
    visible_line_count_top: i32,
    visible_line_count_bottom: i32,
}

impl UnchangedRegion {
    pub fn from_diffs(
        changes: &[DetailedLineRangeMapping],
        original_line_count: i32,
        modified_line_count: i32,
        min_hidden_line_count: i32,
        min_context: i32,
    ) -> Vec<UnchangedRegion> {
        let mappings: Vec<LineRangeMapping> = changes.iter().map(|change| LineRangeMapping::new(change.original, change.modified)).collect();
        let inversed_mappings = LineRangeMapping::inverse(&mappings, original_line_count, modified_line_count);
        let mut result: Vec<UnchangedRegion> = Vec::new();

        for mapping in inversed_mappings {
            let mut orig_start = mapping.original.start_line_number;
            let mut mod_start = mapping.modified.start_line_number;
            let mut length = mapping.original.length();

            let at_start = orig_start == 1 && mod_start == 1;
            let at_end = orig_start + length == original_line_count + 1 && mod_start + length == modified_line_count + 1;

            if (at_start || at_end) && length >= min_context + min_hidden_line_count {
                if at_start && !at_end {
                    length -= min_context;
                }
                if at_end && !at_start {
                    orig_start += min_context;
                    mod_start += min_context;
                    length -= min_context;
                }
                result.push(UnchangedRegion::new(orig_start, mod_start, length, 0, 0));
            } else if length >= min_context * 2 + min_hidden_line_count {
                orig_start += min_context;
                mod_start += min_context;
                length -= min_context * 2;
                result.push(UnchangedRegion::new(orig_start, mod_start, length, 0, 0));
            }
        }

        result
    }

    pub fn new(original_line_number: i32, modified_line_number: i32, line_count: i32, visible_line_count_top: i32, visible_line_count_bottom: i32) -> Self {
        let visible_line_count_top2 = visible_line_count_top.min(line_count).max(0);
        let visible_line_count_bottom2 = visible_line_count_bottom.min(line_count - visible_line_count_top).max(0);

        soft_assert(visible_line_count_top == visible_line_count_top2, None);
        soft_assert(visible_line_count_bottom == visible_line_count_bottom2, None);

        Self {
            original_line_number,
            modified_line_number,
            line_count,
            visible_line_count_top: visible_line_count_top2,
            visible_line_count_bottom: visible_line_count_bottom2,
        }
    }

    pub fn original_unchanged_range(&self) -> LineRange {
        LineRange::of_length(self.original_line_number, self.line_count)
    }

    pub fn modified_unchanged_range(&self) -> LineRange {
        LineRange::of_length(self.modified_line_number, self.line_count)
    }

    pub fn visible_line_count_top(&self) -> i32 {
        self.visible_line_count_top
    }

    pub fn visible_line_count_bottom(&self) -> i32 {
        self.visible_line_count_bottom
    }
}
