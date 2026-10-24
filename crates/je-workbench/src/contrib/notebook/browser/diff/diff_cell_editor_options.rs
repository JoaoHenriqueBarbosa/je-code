//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/diff/diffCellEditorOptions.ts`: só `getEditorPadding`.
//!
//! Divergências:
//! - `fixedEditorOptions` e `fixedDiffEditorOptions` (`IEditorOptions` e `IDiffEditorConstructionOptions` do widget de
//!   edição, que a UI nativa do porte não tem) não são portados.

/// O `{ top, bottom }` do padding do editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorPadding {
    pub top: i64,
    pub bottom: i64,
}

/// Do not leave at 12, when at 12 and we have whitespace and only one line, then there's not enough space for the button
/// `Show Whitespace Differences`.
const FIXED_EDITOR_PADDING_SINGLE_LINE_CELLS: EditorPadding = EditorPadding { top: 24, bottom: 24 };
const FIXED_EDITOR_PADDING: EditorPadding = EditorPadding { top: 12, bottom: 12 };

/// `getEditorPadding`.
pub fn get_editor_padding(line_count: i64) -> EditorPadding {
    if line_count == 1 { FIXED_EDITOR_PADDING_SINGLE_LINE_CELLS } else { FIXED_EDITOR_PADDING }
}
