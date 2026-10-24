//! Porte da lógica de reordenação de `src/vs/workbench/contrib/notebook/browser/view/cellParts/cellDnd.ts`: a função
//! `performCellDropEdits`, que move as células arrastadas (a seleção inteira, se a célula arrastada faz parte dela) para
//! perto da célula sobre a qual se soltou.
//!
//! Divergências:
//! - `CellDragAndDropPart` e `CellDragAndDropController` (classes CSS de arraste, indicador de inserção, `DragEvent`,
//!   `Delayer` de rolagem) são pintura e eventos do DOM e não entram: a UI é nativa.
//! - `dropDirection: 'above' | 'below'` é [`CellDropDirection`].
//! - `editor.getNextVisibleCellIndex(index) ?? index` é o `getNextVisibleCellIndex` do view model (que sempre devolve um
//!   índice, e é o que o widget repassa).
//! - `typeof index !== 'number'` (célula fora do modelo) é o índice negativo de `getCellIndex`.

use crate::contrib::notebook::browser::controller::IActiveNotebookEditor;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_common::{ICellEditOperation, ICellMoveEdit, ISelectionIndexState, ISelectionState};
use crate::contrib::notebook::common::notebook_range::ICellRange;

/// `'above' | 'below'`: de que lado da célula sobre a qual se soltou as células arrastadas entram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellDropDirection {
    Above,
    Below,
}

/// `performCellDropEdits(editor, draggedCell, dropDirection, draggedOverCell)`.
pub fn perform_cell_drop_edits(
    editor: &dyn IActiveNotebookEditor,
    dragged_cell: &CellViewModel,
    drop_direction: CellDropDirection,
    dragged_over_cell: &CellViewModel,
) {
    let dragged_cell_index = editor.get_cell_index(dragged_cell);
    let mut original_to_idx = editor.get_cell_index(dragged_over_cell);

    if dragged_cell_index < 0 || original_to_idx < 0 {
        return;
    }

    // If dropped on a folded markdown range, insert after the folding range
    if drop_direction == CellDropDirection::Below {
        original_to_idx = editor.get_view_model().get_next_visible_cell_index(original_to_idx);
    }

    let mut selections = editor.get_selections();
    if selections.is_empty() {
        selections = vec![editor.get_focus()];
    }

    let mut original_focus_idx = editor.get_focus().start;

    // If the dragged cell is not focused/selected, ignore the current focus/selection and use the dragged idx
    if !selections.iter().any(|s| s.start <= dragged_cell_index && s.end > dragged_cell_index) {
        selections = vec![ICellRange { start: dragged_cell_index, end: dragged_cell_index + 1 }];
        original_focus_idx = dragged_cell_index;
    }

    if let Some(dropped_in_selection) = selections.iter().find(|range| range.start <= original_to_idx && range.end > original_to_idx) {
        original_to_idx = dropped_in_selection.start;
    }

    let mut num_cells: i64 = 0;
    let mut focus_new_idx = original_to_idx;
    let mut new_insertion_idx = original_to_idx;

    // Compute a set of edits which will be applied in reverse order by the notebook text model.
    // `index`: the starting index of the range, after previous edits have been applied
    // `newIdx`: the destination index, after this edit's range has been removed
    selections.sort_by(|a, b| b.start.cmp(&a.start));
    let edits: Vec<ICellMoveEdit> = selections
        .iter()
        .map(|range| {
            let length = range.end - range.start;

            // If this range is before the insertion point, subtract the cells in this range from the "to" index
            let to_index_delta = if range.end <= new_insertion_idx { -length } else { 0 };

            let new_idx = new_insertion_idx + to_index_delta;

            // If this range contains the focused cell, set the new focus index to the new index of the cell
            if original_focus_idx >= range.start && original_focus_idx <= range.end {
                let offset = original_focus_idx - range.start;
                focus_new_idx = new_idx + offset;
            }

            // If below the insertion point, the original index will have been shifted down
            let from_index_delta = if range.start >= original_to_idx { num_cells } else { 0 };

            let edit = ICellMoveEdit { index: (range.start + from_index_delta) as usize, length: length as usize, new_idx: new_idx as usize };
            num_cells += length;

            // If a range was moved down, the insertion index needs to be adjusted
            if range.end < new_insertion_idx {
                new_insertion_idx -= length;
            }

            edit
        })
        .collect();

    let Some(last_edit) = edits.last() else {
        return;
    };
    let final_selection = ICellRange { start: last_edit.new_idx as i64, end: last_edit.new_idx as i64 + num_cells };
    let final_focus = ICellRange { start: focus_new_idx, end: focus_new_idx + 1 };

    let begin_selection_state = ISelectionState::Index(ISelectionIndexState { focus: editor.get_focus(), selections: editor.get_selections() });
    let operations: Vec<ICellEditOperation> = edits.into_iter().map(ICellEditOperation::Move).collect();
    editor.text_model().apply_edits(
        &operations,
        true,
        Some(begin_selection_state),
        &|| Some(ISelectionState::Index(ISelectionIndexState { focus: final_focus, selections: vec![final_selection] })),
        None,
        true,
    );
    editor.reveal_cell_range_in_view(final_selection);
}
