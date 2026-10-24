//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/contrib/clipboard/notebookClipboard.ts`: copiar e recortar células
//! (`runCopyCells` e `runCutCells`), com o texto indo para a área de transferência e as células para a área de células do
//! `INotebookService`. O colar, a `NotebookClipboardContribution` e as ações registradas dependem do editor e dos comandos de edição
//! do widget.
//!
//! Divergências:
//! - `accessor.get(IClipboardService)` e `accessor.get(INotebookService)` são os serviços recebidos como argumento.
//! - `editor: INotebookEditor` é o [`IActiveNotebookEditor`] (`hasModel()` é sempre verdadeiro). `editor.hasOutputTextSelection()` (seleção de
//!   texto numa saída renderizada no webview, que dispara `document.execCommand('copy')`) é do widget DOM e não entra: o corpo age como no
//!   caso sem seleção de saída.
//! - `clipboardService.writeText(...)` é chamado sem esperar a promessa (como no original).

use je_editor::services::clipboard_service::IClipboardService;

use crate::contrib::notebook::browser::controller::{IActiveNotebookEditor, cell_range_to_view_cells, expand_cell_ranges_with_hidden_cells};
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::notebook_common::{ICellEditOperation, ICellReplaceEdit, ISelectionIndexState, ISelectionState};
use crate::contrib::notebook::common::notebook_range::ICellRange;
use crate::contrib::notebook::common::notebook_service::INotebookService;

/// `{ editType: CellEditType.Replace, index, count, cells: [] }`.
fn delete_cells(index: usize, count: usize) -> ICellEditOperation {
    ICellEditOperation::Replace(ICellReplaceEdit { index, count, cells: Vec::new() })
}

/// `{ kind: SelectionStateType.Index, focus, selections }`.
fn index_state(focus: ICellRange, selections: Vec<ICellRange>) -> Option<ISelectionState> {
    Some(ISelectionState::Index(ISelectionIndexState { focus, selections }))
}

/// `runCopyCells(accessor, editor, targetCell)`.
pub fn run_copy_cells(
    clipboard_service: &dyn IClipboardService,
    notebook_service: &dyn INotebookService,
    editor: &dyn IActiveNotebookEditor,
    target_cell: Option<&CellViewModel>,
) -> bool {
    let selections = editor.get_selections();

    if let Some(target_cell) = target_cell {
        let target_cell_index = editor.get_cell_index(target_cell);
        let containing_selection = selections.iter().find(|selection| selection.start <= target_cell_index && target_cell_index < selection.end);

        if containing_selection.is_none() {
            let _ = clipboard_service.write_text(&target_cell.get_text());
            notebook_service.set_to_copy(vec![target_cell.model()], true);
            return true;
        }
    }

    let selection_ranges = expand_cell_ranges_with_hidden_cells(editor, &editor.get_selections());
    let selected_cells = cell_range_to_view_cells(editor, &selection_ranges);

    if selected_cells.is_empty() {
        return false;
    }

    let _ = clipboard_service.write_text(&selected_cells.iter().map(CellViewModel::get_text).collect::<Vec<_>>().join("\n"));
    notebook_service.set_to_copy(selected_cells.iter().map(CellViewModel::model).collect(), true);

    true
}

/// `runCutCells(accessor, editor, targetCell)`.
pub fn run_cut_cells(
    clipboard_service: &dyn IClipboardService,
    notebook_service: &dyn INotebookService,
    editor: &dyn IActiveNotebookEditor,
    target_cell: Option<&CellViewModel>,
) -> bool {
    if editor.is_read_only() {
        return false;
    }

    let text_model = editor.text_model();
    let selections = editor.get_selections();

    if let Some(target_cell) = target_cell {
        // from ui
        let target_cell_index = editor.get_cell_index(target_cell);
        let containing_selection = selections.iter().find(|selection| selection.start <= target_cell_index && target_cell_index < selection.end);

        if containing_selection.is_none() {
            let _ = clipboard_service.write_text(&target_cell.get_text());
            // delete cell
            let focus = editor.get_focus();
            let new_focus = if focus.end <= target_cell_index { focus } else { ICellRange { start: focus.start - 1, end: focus.end - 1 } };
            let new_selections: Vec<ICellRange> = selections
                .iter()
                .map(|selection| if selection.end <= target_cell_index { *selection } else { ICellRange { start: selection.start - 1, end: selection.end - 1 } })
                .collect();

            text_model.apply_edits(
                &[delete_cells(target_cell_index as usize, 1)],
                true,
                index_state(editor.get_focus(), selections.clone()),
                &|| index_state(new_focus, new_selections.clone()),
                None,
                true,
            );

            notebook_service.set_to_copy(vec![target_cell.model()], false);
            return true;
        }
    }

    let focus = editor.get_focus();
    let containing_selection = selections.iter().find(|selection| selection.start <= focus.start && focus.end <= selection.end);

    if containing_selection.is_none() {
        // focus is out of any selection, we should only cut this cell
        let target_cell = editor.cell_at(focus.start).expect("a célula com foco existe");
        let _ = clipboard_service.write_text(&target_cell.get_text());
        let new_focus = if focus.end == editor.get_length() as i64 { ICellRange { start: focus.start - 1, end: focus.end - 1 } } else { focus };
        let new_selections: Vec<ICellRange> =
            selections.iter().map(|selection| if selection.end <= focus.start { *selection } else { ICellRange { start: selection.start - 1, end: selection.end - 1 } }).collect();
        text_model.apply_edits(
            &[delete_cells(focus.start as usize, 1)],
            true,
            index_state(editor.get_focus(), selections.clone()),
            &|| index_state(new_focus, new_selections.clone()),
            None,
            true,
        );

        notebook_service.set_to_copy(vec![target_cell.model()], false);
        return true;
    }

    let selection_ranges = expand_cell_ranges_with_hidden_cells(editor, &editor.get_selections());
    let selected_cells = cell_range_to_view_cells(editor, &selection_ranges);

    if selected_cells.is_empty() {
        return false;
    }

    let _ = clipboard_service.write_text(&selected_cells.iter().map(CellViewModel::get_text).collect::<Vec<_>>().join("\n"));
    let edits: Vec<ICellEditOperation> = selection_ranges.iter().map(|range| delete_cells(range.start as usize, (range.end - range.start) as usize)).collect();
    let first_select_index = selection_ranges[0].start;

    // If we have cells, 0, 1, 2, 3, 4, 5, 6
    // and cells 1, 2 are selected, and then we delete cells 1 and 2
    // the new focused cell should still be at index 1
    let cells_length = text_model.cells().len() as i64;
    let new_focused_cell_index = if first_select_index < cells_length - 1 { first_select_index } else { (cells_length - 2).max(0) };

    text_model.apply_edits(
        &edits,
        true,
        index_state(editor.get_focus(), selection_ranges.clone()),
        &|| {
            index_state(
                ICellRange { start: new_focused_cell_index, end: new_focused_cell_index + 1 },
                vec![ICellRange { start: new_focused_cell_index, end: new_focused_cell_index + 1 }],
            )
        },
        None,
        true,
    );
    notebook_service.set_to_copy(selected_cells.iter().map(CellViewModel::model).collect(), false);

    true
}
